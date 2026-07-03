use std::fs;

use appd_runtime::certs::{CertificateBundle, CertificatePaths};
use openssl::nid::Nid;
use openssl::pkcs12::Pkcs12;
use openssl::stack::Stack;
use openssl::x509::store::X509StoreBuilder;
use openssl::x509::{X509, X509StoreContext, X509StoreContextRef};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn generates_certificate_bundle_with_platform_required_materials() -> TestResult {
    let bundle = CertificateBundle::generate()?;

    assert!(
        bundle
            .ca_cert_pem
            .starts_with("-----BEGIN CERTIFICATE-----")
    );
    assert!(
        bundle
            .server_cert_pem
            .starts_with("-----BEGIN CERTIFICATE-----")
    );
    assert!(
        bundle
            .server_key_pem
            .starts_with("-----BEGIN PRIVATE KEY-----")
    );
    assert!(
        bundle
            .client_cert_pem
            .starts_with("-----BEGIN CERTIFICATE-----")
    );
    assert!(
        bundle
            .client_key_pem
            .starts_with("-----BEGIN PRIVATE KEY-----")
    );
    assert_eq!(bundle.p12_password, "appd-internal");

    let ca_cert = X509::from_pem(bundle.ca_cert_pem.as_bytes())?;
    let server_cert = X509::from_pem(bundle.server_cert_pem.as_bytes())?;
    let client_cert = X509::from_pem(bundle.client_cert_pem.as_bytes())?;
    assert_eq!(common_name(&ca_cert)?, "appd local ca");
    assert_eq!(common_name(&server_cert)?, "localhost");
    assert_eq!(common_name(&client_cert)?, "appd client");

    let ca_key = ca_cert.public_key()?;
    assert!(ca_cert.verify(&ca_key)?);
    assert!(server_cert.verify(&ca_key)?);
    assert!(client_cert.verify(&ca_key)?);
    assert!(certificate_verifies_against_ca(&server_cert, &ca_cert)?);
    assert!(certificate_verifies_against_ca(&client_cert, &ca_cert)?);

    let san = server_cert
        .subject_alt_names()
        .ok_or("server certificate must include SANs")?;
    assert!(san.iter().any(|name| name.dnsname() == Some("localhost")));
    assert!(
        san.iter()
            .any(|name| name.ipaddress() == Some(&[127, 0, 0, 1]))
    );

    let ca_text = cert_text(&ca_cert)?;
    assert!(ca_text.contains("CA:TRUE"));
    assert!(ca_text.contains("Certificate Sign"));
    assert!(ca_text.contains("CRL Sign"));

    let server_text = cert_text(&server_cert)?;
    assert!(server_text.contains("TLS Web Server Authentication"));
    assert!(!server_text.contains("TLS Web Client Authentication"));

    let client_text = cert_text(&client_cert)?;
    assert!(client_text.contains("TLS Web Client Authentication"));
    assert!(!client_text.contains("TLS Web Server Authentication"));

    let parsed = Pkcs12::from_der(&bundle.client_p12_der)?.parse2(&bundle.p12_password)?;
    assert!(parsed.cert.is_some());
    assert!(parsed.pkey.is_some());
    assert!(!bundle.ca_cert_der.is_empty());

    Ok(())
}

fn common_name(cert: &X509) -> TestResult<String> {
    let entry = cert
        .subject_name()
        .entries_by_nid(Nid::COMMONNAME)
        .next()
        .ok_or("certificate subject is missing a common name")?;
    Ok(entry.data().as_utf8()?.to_string())
}

fn cert_text(cert: &X509) -> TestResult<String> {
    Ok(String::from_utf8(cert.to_text()?)?)
}

fn certificate_verifies_against_ca(cert: &X509, ca: &X509) -> TestResult<bool> {
    let mut store = X509StoreBuilder::new()?;
    store.add_cert(ca.to_owned())?;
    let store = store.build();
    let chain = Stack::new()?;

    let mut context = X509StoreContext::new()?;
    Ok(context.init(&store, cert, &chain, X509StoreContextRef::verify_cert)?)
}

#[test]
fn writes_and_loads_cached_certificate_bundle() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let bundle = CertificateBundle::generate()?;

    bundle.write_all(temp_dir.path())?;
    assert!(CertificatePaths::all_exist(temp_dir.path()));

    let loaded = CertificateBundle::load_cached(temp_dir.path())?;
    assert_eq!(loaded.ca_cert_der, bundle.ca_cert_der);
    assert_eq!(loaded.client_p12_der, bundle.client_p12_der);
    assert_eq!(loaded.p12_password, bundle.p12_password);

    for name in [
        "ca.cert.pem",
        "ca.cert.der",
        "server.cert.pem",
        "server.key.pem",
        "client.cert.pem",
        "client.key.pem",
        "client.p12",
    ] {
        assert!(
            fs::metadata(temp_dir.path().join(name))?.is_file(),
            "{name}"
        );
    }

    Ok(())
}

#[test]
fn computes_chromium_spki_pin_from_server_certificate() -> TestResult {
    let bundle = CertificateBundle::generate()?;
    let pin = bundle.server_spki_sha256_base64()?;

    assert!(!pin.is_empty());
    assert!(
        pin.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
    );

    Ok(())
}
