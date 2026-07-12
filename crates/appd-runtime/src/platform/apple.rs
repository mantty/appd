//! Shared Apple certificate and bundle helpers.

use std::path::{Path, PathBuf};
use std::ptr;
use std::ptr::NonNull;
use std::sync::{Arc, RwLock};

use block2::DynBlock;
use objc2::ClassType;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2_core_foundation::{CFArray, CFData, CFDictionary, CFRetained, CFType};
use objc2_foundation::{
    NSArray, NSObject, NSURLAuthenticationChallenge, NSURLAuthenticationMethodClientCertificate,
    NSURLAuthenticationMethodServerTrust, NSURLCredential, NSURLCredentialPersistence,
    NSURLProtectionSpace, NSURLSessionAuthChallengeDisposition,
};
use objc2_security::{
    SecCertificate, SecIdentity, SecKey, SecTrust, kSecAttrKeyClass, kSecAttrKeyClassPrivate,
    kSecAttrKeyType, kSecAttrKeyTypeECSECPrimeRandom,
};
use p256::{SecretKey, pkcs8::DecodePrivateKey};

use crate::certs::CertificateBundle;
use crate::host::{work_dir_in_apple_resources, work_dir_next_to_current_exe_or_default};

pub(super) type SharedCertificates = Arc<RwLock<Option<CertificateBundle>>>;
pub(super) type AuthenticationCompletionHandler =
    DynBlock<dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential)>;

const STARTUP_ERROR_FILE: &str = "startup-error.log";

pub(super) fn clear_startup_error(state_dir: &Path) {
    let _ = std::fs::remove_file(state_dir.join(STARTUP_ERROR_FILE));
}

pub(super) fn record_startup_error(state_dir: &Path, error: &crate::RuntimeError) {
    let _ = std::fs::write(state_dir.join(STARTUP_ERROR_FILE), format!("{error:#}"));
}

pub(super) fn handle_authentication_challenge(
    challenge: &NSURLAuthenticationChallenge,
    completion_handler: &AuthenticationCompletionHandler,
    certificates: &SharedCertificates,
) {
    let protection_space = challenge.protectionSpace();
    let method = protection_space.authenticationMethod();
    let certs = certificates.read().ok().and_then(|guard| guard.clone());

    let Some(certs) = certs else {
        reject_challenge(completion_handler);
        return;
    };

    if method.isEqualToString(unsafe { NSURLAuthenticationMethodServerTrust }) {
        handle_server_trust(&protection_space, completion_handler, &certs);
    } else if method.isEqualToString(unsafe { NSURLAuthenticationMethodClientCertificate }) {
        handle_client_certificate(completion_handler, &certs);
    } else {
        completion_handler.call((
            NSURLSessionAuthChallengeDisposition::PerformDefaultHandling,
            ptr::null_mut(),
        ));
    }
}

fn handle_server_trust(
    protection_space: &NSURLProtectionSpace,
    completion_handler: &AuthenticationCompletionHandler,
    certs: &CertificateBundle,
) {
    let Some(server_trust) = protection_space_server_trust(protection_space) else {
        reject_challenge(completion_handler);
        return;
    };

    let ca_data = CFData::from_bytes(&certs.ca_cert_der);
    let Some(ca_cert) = (unsafe { SecCertificate::with_data(None, &ca_data) }) else {
        reject_challenge(completion_handler);
        return;
    };
    let anchor_array = CFArray::from_objects(&[&*ca_cert]);

    let server_trust = unsafe { server_trust.as_ref() };
    let set_anchors =
        unsafe { server_trust.set_anchor_certificates(Some(anchor_array.as_opaque())) };
    let set_only = unsafe { server_trust.set_anchor_certificates_only(true) };
    if set_anchors != 0 || set_only != 0 {
        reject_challenge(completion_handler);
        return;
    }

    let trusted = unsafe { server_trust.evaluate_with_error(ptr::null_mut()) };
    if !trusted {
        reject_challenge(completion_handler);
        return;
    }

    let credential = credential_for_trust(server_trust);
    use_credential(completion_handler, &credential);
}

fn handle_client_certificate(
    completion_handler: &AuthenticationCompletionHandler,
    certs: &CertificateBundle,
) {
    let Some(identity) = create_memory_identity(certs) else {
        reject_challenge(completion_handler);
        return;
    };
    let credential = credential_for_identity(&identity);
    use_credential(completion_handler, &credential);
}

fn use_credential(
    completion_handler: &AuthenticationCompletionHandler,
    credential: &NSURLCredential,
) {
    completion_handler.call((
        NSURLSessionAuthChallengeDisposition::UseCredential,
        ptr::from_ref(credential).cast_mut(),
    ));
}

fn reject_challenge(completion_handler: &AuthenticationCompletionHandler) {
    completion_handler.call((
        NSURLSessionAuthChallengeDisposition::RejectProtectionSpace,
        ptr::null_mut(),
    ));
}

pub(super) fn bundle_work_dir() -> PathBuf {
    let bundle = objc2_foundation::NSBundle::mainBundle();
    if let Some(resource_path) = bundle
        .resourcePath()
        .map(|path| PathBuf::from(path.to_string()))
    {
        return work_dir_in_apple_resources(&resource_path);
    }

    work_dir_next_to_current_exe_or_default()
}

pub(super) fn bundle_state_dir() -> PathBuf {
    let bundle = objc2_foundation::NSBundle::mainBundle();
    let identifier = bundle
        .bundleIdentifier()
        .map_or_else(|| "com.appd.runtime".to_owned(), |value| value.to_string());
    std::env::var_os("HOME").map_or_else(
        || std::env::temp_dir().join("appd").join(&identifier),
        |home| PathBuf::from(home).join("Library/Caches").join(&identifier),
    )
}

fn protection_space_server_trust(
    protection_space: &NSURLProtectionSpace,
) -> Option<NonNull<SecTrust>> {
    // SAFETY: `NSURLProtectionSpace -serverTrust` returns a retained trust
    // object or nil for non-server-trust challenges.
    unsafe { msg_send![protection_space, serverTrust] }
}

fn credential_for_trust(trust: &SecTrust) -> Retained<NSURLCredential> {
    // SAFETY: `NSURLCredential +credentialForTrust:` returns an autoreleased
    // credential. `Retained` follows objc2's retain conventions for `msg_send!`.
    unsafe { msg_send![NSURLCredential::class(), credentialForTrust: trust] }
}

fn create_memory_identity(certs: &CertificateBundle) -> Option<CFRetained<SecIdentity>> {
    let certificate = unsafe {
        SecCertificate::with_data(
            None,
            &CFData::from_bytes(&certificate_der(&certs.client_cert_pem)?),
        )?
    };
    let key_data = CFData::from_bytes(&x963_private_key(&certs.client_key_der)?);
    let attributes = CFDictionary::<CFType, CFType>::from_slices(
        &[
            unsafe { kSecAttrKeyType }.as_ref(),
            unsafe { kSecAttrKeyClass }.as_ref(),
        ],
        &[
            unsafe { kSecAttrKeyTypeECSECPrimeRandom }.as_ref(),
            unsafe { kSecAttrKeyClassPrivate }.as_ref(),
        ],
    );
    let key = unsafe { SecKey::with_data(&key_data, attributes.as_opaque(), ptr::null_mut()) }?;
    unsafe { SecIdentity::new(None, &certificate, &key) }
}

fn certificate_der(pem: &str) -> Option<Vec<u8>> {
    x509_parser::pem::parse_x509_pem(pem.as_bytes())
        .ok()
        .map(|(_, certificate)| certificate.contents)
}

fn x963_private_key(pkcs8_der: &[u8]) -> Option<Vec<u8>> {
    let secret = SecretKey::from_pkcs8_der(pkcs8_der).ok()?;
    let public = secret.public_key().to_sec1_bytes();
    let mut key = Vec::with_capacity(public.len() + secret.to_bytes().len());
    key.extend_from_slice(&public);
    key.extend_from_slice(&secret.to_bytes());
    Some(key)
}

fn credential_for_identity(identity: &SecIdentity) -> Retained<NSURLCredential> {
    // SAFETY: `NSURLCredential +credentialWithIdentity:certificates:persistence:`
    // is the documented client certificate credential constructor.
    unsafe {
        msg_send![
            NSURLCredential::class(),
            credentialWithIdentity: identity,
            certificates: ptr::null::<NSArray<NSObject>>(),
            persistence: NSURLCredentialPersistence::None
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::create_memory_identity;
    use crate::certs::CertificateBundle;

    #[test]
    fn creates_client_identity_without_keychain_import() -> crate::RuntimeResult<()> {
        let certificates = CertificateBundle::generate()?;
        assert!(create_memory_identity(&certificates).is_some());
        Ok(())
    }
}
