//! Shared Apple certificate and bundle helpers.

use std::path::PathBuf;
use std::ptr;
use std::ptr::NonNull;
use std::sync::{Arc, RwLock};

use block2::DynBlock;
use objc2::ClassType;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2_core_foundation::{CFArray, CFData, CFDictionary, CFRetained, CFString};
use objc2_foundation::{
    NSArray, NSObject, NSURLAuthenticationChallenge, NSURLAuthenticationMethodClientCertificate,
    NSURLAuthenticationMethodServerTrust, NSURLCredential, NSURLCredentialPersistence,
    NSURLProtectionSpace, NSURLSessionAuthChallengeDisposition,
};
use objc2_security::{
    SecCertificate, SecIdentity, SecPKCS12Import, SecTrust, kSecImportExportPassphrase,
    kSecImportItemIdentity,
};

use crate::certs::CertificateBundle;
use crate::host::{work_dir_in_apple_resources, work_dir_next_to_current_exe_or_default};

pub(super) type SharedCertificates = Arc<RwLock<Option<CertificateBundle>>>;
pub(super) type AuthenticationCompletionHandler =
    DynBlock<dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential)>;

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
    let Some(identity) = import_pkcs12_identity(certs) else {
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

fn import_pkcs12_identity(certs: &CertificateBundle) -> Option<Retained<SecIdentity>> {
    let p12_data = CFData::from_bytes(&certs.client_p12_der);
    let password = CFString::from_str(&certs.p12_password);
    let options =
        CFDictionary::from_slices(&[unsafe { kSecImportExportPassphrase }], &[&*password]);
    let mut items: *const CFArray<CFDictionary<CFString, SecIdentity>> = ptr::null();
    let status = unsafe {
        SecPKCS12Import(
            &p12_data,
            options.as_opaque(),
            NonNull::from(&mut items).cast::<*const CFArray>(),
        )
    };
    if status != 0 || items.is_null() {
        return None;
    }
    let items = unsafe { CFRetained::from_raw(NonNull::new_unchecked(items.cast_mut())) };
    let first = items.get(0)?;
    first
        .get(unsafe { kSecImportItemIdentity })
        .map(Retained::from)
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
