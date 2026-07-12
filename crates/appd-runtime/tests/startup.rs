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
fn ensure_certificates_regenerates_corrupt_der_cache() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let first = ensure_certificates(temp_dir.path())?;
    std::fs::write(
        temp_dir.path().join(CertificatePaths::CA_CERT_DER),
        [0_u8, 1_u8],
    )?;

    let second = ensure_certificates(temp_dir.path())?;

    assert_ne!(second.ca_cert_der, [0_u8, 1_u8]);
    assert_ne!(second.ca_cert_der, first.ca_cert_der);
    assert!(CertificatePaths::all_exist(temp_dir.path()));
    Ok(())
}

#[test]
fn ensure_certificates_regenerates_corrupt_client_key_der_cache() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let first = ensure_certificates(temp_dir.path())?;
    std::fs::write(
        temp_dir.path().join(CertificatePaths::CLIENT_KEY_DER),
        [0_u8, 1_u8],
    )?;

    let second = ensure_certificates(temp_dir.path())?;

    assert_ne!(second.client_key_der, [0_u8, 1_u8]);
    assert_ne!(second.client_key_der, first.client_key_der);
    assert!(CertificatePaths::all_exist(temp_dir.path()));
    Ok(())
}

#[test]
fn ensure_certificates_regenerates_mismatched_ca_der_cache() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let first = ensure_certificates(temp_dir.path())?;
    let foreign = appd_runtime::certs::CertificateBundle::generate()?;
    std::fs::write(
        temp_dir.path().join(CertificatePaths::CA_CERT_DER),
        &foreign.ca_cert_der,
    )?;

    let second = ensure_certificates(temp_dir.path())?;

    assert_ne!(second.ca_cert_der, foreign.ca_cert_der);
    assert_ne!(second.ca_cert_der, first.ca_cert_der);
    assert!(CertificatePaths::all_exist(temp_dir.path()));
    Ok(())
}

#[test]
fn frontend_url_uses_localhost() {
    assert_eq!(frontend_url(8787), "https://localhost:8787/");
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
