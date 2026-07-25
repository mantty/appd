//! Shared Apple certificate and bundle helpers.

#[cfg(feature = "bare-runtime")]
use std::path::{Path, PathBuf};
use std::ptr;
use std::ptr::NonNull;
#[cfg(feature = "bare-runtime")]
use std::sync::{Arc, Mutex, OnceLock};

use block2::DynBlock;
use objc2::ClassType;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2_core_foundation::{CFArray, CFData, CFDictionary, CFError, CFRetained, CFType};
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

pub(super) use crate::certs::SharedCertificateBundle as SharedCertificates;
#[cfg(feature = "bare-runtime")]
use crate::host::{work_dir_in_apple_resources, work_dir_next_to_current_exe_or_default};
use crate::{RuntimeError, RuntimeResult, certs::CertificateBundle};

pub(super) type AuthenticationCompletionHandler =
    DynBlock<dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential)>;

#[cfg(feature = "bare-runtime")]
const STARTUP_ERROR_FILE: &str = "startup-error.log";

#[cfg(feature = "bare-runtime")]
static RUNTIME: OnceLock<Mutex<RuntimeState>> = OnceLock::new();

#[cfg(feature = "bare-runtime")]
#[derive(Default)]
struct RuntimeState {
    runtime: Option<crate::host::StartedRuntime>,
    suspended: bool,
}

#[cfg(feature = "bare-runtime")]
pub(super) fn clear_startup_error(state_dir: &Path) {
    let _ = std::fs::remove_file(state_dir.join(STARTUP_ERROR_FILE));
}

#[cfg(feature = "bare-runtime")]
pub(super) fn record_startup_error(state_dir: &Path, error: &crate::RuntimeError) {
    let _ = std::fs::write(state_dir.join(STARTUP_ERROR_FILE), format!("{error:#}"));
}

#[cfg(feature = "bare-runtime")]
pub(super) fn start_runtime(
    packaged_dir: &Path,
    state_dir: &Path,
    host: &str,
    certificates: &SharedCertificates,
) -> RuntimeResult<crate::host::StartedRuntime> {
    match crate::host::start_bare_runtime(packaged_dir, state_dir, host, Arc::clone(certificates)) {
        Ok(runtime) => {
            clear_startup_error(state_dir);
            Ok(runtime)
        }
        Err(error) => {
            record_startup_error(state_dir, &error);
            eprintln!("appd runtime startup failed: {error:#}");
            Err(error)
        }
    }
}

#[cfg(feature = "bare-runtime")]
pub(super) fn store_runtime(runtime: crate::host::StartedRuntime) {
    let state = RUNTIME.get_or_init(|| Mutex::new(RuntimeState::default()));
    let Ok(mut state) = state.lock() else {
        return;
    };
    state.runtime = Some(runtime);
    if state.suspended {
        transition_runtime(state.runtime.as_ref(), true);
    }
}

#[cfg(feature = "bare-runtime")]
pub(super) fn set_runtime_suspended(suspended: bool) {
    let state = RUNTIME.get_or_init(|| Mutex::new(RuntimeState::default()));
    let Ok(mut state) = state.lock() else {
        return;
    };
    if state.suspended == suspended {
        return;
    }
    state.suspended = suspended;
    transition_runtime(state.runtime.as_ref(), suspended);
}

#[cfg(feature = "bare-runtime")]
fn transition_runtime(runtime: Option<&crate::host::StartedRuntime>, suspended: bool) {
    let Some(runtime) = runtime else {
        return;
    };
    let result = if suspended {
        runtime.suspend()
    } else {
        runtime.resume()
    };
    if let Err(error) = result {
        eprintln!("appd runtime lifecycle transition failed: {error}");
    }
}

pub(super) fn handle_authentication_challenge(
    challenge: &NSURLAuthenticationChallenge,
    completion_handler: &AuthenticationCompletionHandler,
    certificates: &SharedCertificates,
    app_host: &str,
) {
    let protection_space = challenge.protectionSpace();
    let method = protection_space.authenticationMethod();
    let host = protection_space.host().to_string();
    if method.isEqualToString(unsafe { NSURLAuthenticationMethodServerTrust }) {
        if !same_dns_host(&host, app_host) {
            completion_handler.call((
                NSURLSessionAuthChallengeDisposition::PerformDefaultHandling,
                ptr::null_mut(),
            ));
            return;
        }
        let certs = certificates.read().ok().and_then(|guard| guard.clone());
        let Some(certs) = certs else {
            cancel_challenge(completion_handler);
            return;
        };
        handle_server_trust(&protection_space, completion_handler, &certs);
    } else if method.isEqualToString(unsafe { NSURLAuthenticationMethodClientCertificate }) {
        if !same_dns_host(&host, app_host) {
            completion_handler.call((
                NSURLSessionAuthChallengeDisposition::PerformDefaultHandling,
                ptr::null_mut(),
            ));
            return;
        }
        let certs = certificates.read().ok().and_then(|guard| guard.clone());
        let Some(certs) = certs else {
            cancel_challenge(completion_handler);
            return;
        };
        handle_client_certificate(challenge, completion_handler, &certs);
    } else {
        completion_handler.call((
            NSURLSessionAuthChallengeDisposition::PerformDefaultHandling,
            ptr::null_mut(),
        ));
    }
}

fn same_dns_host(left: &str, right: &str) -> bool {
    left.trim_end_matches('.')
        .eq_ignore_ascii_case(right.trim_end_matches('.'))
}

fn handle_server_trust(
    protection_space: &NSURLProtectionSpace,
    completion_handler: &AuthenticationCompletionHandler,
    certs: &CertificateBundle,
) {
    let Some(server_trust) = protection_space_server_trust(protection_space) else {
        cancel_challenge(completion_handler);
        return;
    };

    let ca_data = CFData::from_bytes(&certs.ca_cert_der);
    let Some(ca_cert) = (unsafe { SecCertificate::with_data(None, &ca_data) }) else {
        cancel_challenge(completion_handler);
        return;
    };
    let server_trust = unsafe { server_trust.as_ref() };
    let anchor_array = CFArray::from_objects(&[&*ca_cert]);
    let set_anchors =
        unsafe { server_trust.set_anchor_certificates(Some(anchor_array.as_opaque())) };
    let set_only = unsafe { server_trust.set_anchor_certificates_only(true) };
    if set_anchors != 0 {
        cancel_challenge(completion_handler);
        return;
    }
    if set_only != 0 {
        cancel_challenge(completion_handler);
        return;
    }

    let mut error: *mut CFError = ptr::null_mut();
    let trusted = unsafe { server_trust.evaluate_with_error(&raw mut error) };
    if !trusted {
        if let Some(error) = NonNull::new(error) {
            drop(unsafe { CFRetained::<CFError>::from_raw(error) });
        }
        cancel_challenge(completion_handler);
        return;
    }

    let credential = credential_for_trust(server_trust);
    use_credential(completion_handler, &credential);
}

fn handle_client_certificate(
    challenge: &NSURLAuthenticationChallenge,
    completion_handler: &AuthenticationCompletionHandler,
    certs: &CertificateBundle,
) {
    if challenge.previousFailureCount() > 0 {
        cancel_challenge(completion_handler);
        return;
    }
    let Some(identity) = create_memory_identity(certs) else {
        cancel_challenge(completion_handler);
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

fn cancel_challenge(completion_handler: &AuthenticationCompletionHandler) {
    completion_handler.call((
        NSURLSessionAuthChallengeDisposition::CancelAuthenticationChallenge,
        ptr::null_mut(),
    ));
}

#[cfg(feature = "bare-runtime")]
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

#[cfg(feature = "bare-runtime")]
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

pub(super) fn app_host() -> RuntimeResult<String> {
    let bundle = objc2_foundation::NSBundle::mainBundle();
    let identifier = bundle.bundleIdentifier().map_or_else(
        || "<missing>".to_owned(),
        |identifier| identifier.to_string(),
    );
    let Some(name) = identifier
        .rsplit('.')
        .next()
        .map(str::to_owned)
        .filter(|name| !name.is_empty())
    else {
        return Err(RuntimeError::InvalidAppName(identifier));
    };
    app_host_for_name(&name).ok_or(RuntimeError::InvalidAppName(identifier))
}

fn app_host_for_name(name: &str) -> Option<String> {
    let name = name.to_ascii_lowercase();
    if crate::is_valid_app_name(&name) {
        Some(format!("{name}.appd.local"))
    } else {
        None
    }
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
        let certificates = CertificateBundle::generate("app.appd.local")?;
        assert!(create_memory_identity(&certificates).is_some());
        Ok(())
    }

    #[test]
    fn compares_authentication_hosts_as_dns_names() {
        assert!(super::same_dns_host("APP.APPD.LOCAL.", "app.appd.local"));
        assert!(!super::same_dns_host("other.appd.local", "app.appd.local"));
    }

    #[test]
    fn derives_the_appd_local_host_from_an_app_name() {
        assert_eq!(
            super::app_host_for_name("my-app"),
            Some("my-app.appd.local".to_owned())
        );
        assert_eq!(
            super::app_host_for_name("Invalid"),
            Some("invalid.appd.local".to_owned())
        );
    }
}
