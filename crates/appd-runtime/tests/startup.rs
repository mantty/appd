use appd_runtime::certs::CertificatePaths;
use appd_runtime::{ensure_certificates, frontend_url};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use time::{Duration, OffsetDateTime};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn ensure_certificates_generates_and_then_reuses_cached_files() -> TestResult {
    let temp_dir = tempfile::tempdir()?;

    let first = ensure_certificates(temp_dir.path())?;
    assert!(CertificatePaths::all_exist(temp_dir.path()));

    let second = ensure_certificates(temp_dir.path())?;
    assert_eq!(first.ca_cert_der, second.ca_cert_der);
    assert_eq!(first.client_p12_der, second.client_p12_der);

    Ok(())
}

#[test]
fn ensure_certificates_recovers_from_partial_cache() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let stale_server_cert = temp_dir.path().join(CertificatePaths::SERVER_CERT_PEM);
    std::fs::create_dir_all(temp_dir.path())?;
    std::fs::write(&stale_server_cert, "not a certificate")?;

    let bundle = ensure_certificates(temp_dir.path())?;

    assert!(CertificatePaths::all_exist(temp_dir.path()));
    assert_eq!(
        std::fs::read_to_string(stale_server_cert)?,
        bundle.server_cert_pem
    );
    assert_ne!(bundle.server_cert_pem, "not a certificate");

    Ok(())
}

#[test]
fn ensure_certificates_regenerates_expired_cache() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let first = ensure_certificates(temp_dir.path())?;
    let expired_server_cert = expired_certificate_pem()?;
    std::fs::write(
        temp_dir.path().join(CertificatePaths::SERVER_CERT_PEM),
        expired_server_cert.as_bytes(),
    )?;

    let second = ensure_certificates(temp_dir.path())?;

    assert!(CertificatePaths::all_exist(temp_dir.path()));
    assert_ne!(second.server_cert_pem, expired_server_cert);
    assert_ne!(second.server_cert_pem, first.server_cert_pem);

    Ok(())
}

#[test]
fn frontend_url_uses_windows_fast_ipv4_loopback() {
    assert_eq!(frontend_url(8787, true), "https://127.0.0.1:8787/");
    assert_eq!(frontend_url(8787, false), "https://localhost:8787/");
}

fn expired_certificate_pem() -> TestResult<String> {
    let key = KeyPair::generate()?;
    let now = OffsetDateTime::now_utc();
    let mut params = CertificateParams::default();
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::CommonName, "expired.localhost");
    params.distinguished_name = distinguished_name;
    params.not_before = now - Duration::days(10);
    params.not_after = now - Duration::days(1);
    Ok(params.self_signed(&key)?.pem())
}
