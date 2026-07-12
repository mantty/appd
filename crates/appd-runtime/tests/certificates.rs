use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use appd_runtime::certs::{CertificateBundle, CertificatePaths};
use openssl::nid::Nid;
use openssl::stack::Stack;
use openssl::x509::store::X509StoreBuilder;
use openssl::x509::{X509, X509StoreContext, X509StoreContextRef};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn generates_certificate_bundle_with_platform_required_materials() -> TestResult {
    let bundle = CertificateBundle::generate()?;
    assert_pem_material(&bundle);

    let ca_cert = X509::from_pem(bundle.ca_cert_pem.as_bytes())?;
    let server_cert = X509::from_pem(bundle.server_cert_pem.as_bytes())?;
    let client_cert = X509::from_pem(bundle.client_cert_pem.as_bytes())?;
    assert_certificate_chain(&ca_cert, &server_cert, &client_cert)?;
    assert_server_names(&server_cert)?;
    assert_certificate_usages(&ca_cert, &server_cert, &client_cert)?;
    Ok(())
}

fn assert_pem_material(bundle: &CertificateBundle) {
    let certificate = "-----BEGIN CERTIFICATE-----";
    let key = "-----BEGIN PRIVATE KEY-----";
    assert!(bundle.ca_cert_pem.starts_with(certificate));
    assert!(bundle.server_cert_pem.starts_with(certificate));
    assert!(bundle.server_key_pem.starts_with(key));
    assert!(bundle.client_cert_pem.starts_with(certificate));
    assert!(bundle.client_key_pem.starts_with(key));
    assert!(!bundle.client_key_der.is_empty());
}

fn assert_certificate_chain(ca: &X509, server: &X509, client: &X509) -> TestResult {
    assert_eq!(common_name(ca)?, "appd local ca");
    assert_eq!(common_name(server)?, "localhost");
    assert_eq!(common_name(client)?, "appd client");
    let key = ca.public_key()?;
    assert!(ca.verify(&key)?);
    assert!(server.verify(&key)?);
    assert!(client.verify(&key)?);
    assert!(certificate_verifies_against_ca(server, ca)?);
    assert!(certificate_verifies_against_ca(client, ca)?);
    Ok(())
}

fn assert_server_names(server: &X509) -> TestResult {
    let san = server
        .subject_alt_names()
        .ok_or("server certificate must include SANs")?;
    assert!(san.iter().any(|name| name.dnsname() == Some("localhost")));
    assert!(
        san.iter()
            .any(|name| name.ipaddress() == Some(&[127, 0, 0, 1]))
    );

    Ok(())
}

fn assert_certificate_usages(ca: &X509, server: &X509, client: &X509) -> TestResult {
    let ca_text = cert_text(ca)?;
    assert!(ca_text.contains("CA:TRUE"));
    assert!(ca_text.contains("Certificate Sign"));
    assert!(ca_text.contains("CRL Sign"));

    let server_text = cert_text(server)?;
    assert!(server_text.contains("TLS Web Server Authentication"));
    assert!(!server_text.contains("TLS Web Client Authentication"));

    let client_text = cert_text(client)?;
    assert!(client_text.contains("TLS Web Client Authentication"));
    assert!(!client_text.contains("TLS Web Server Authentication"));

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
    assert_eq!(loaded.client_key_der, bundle.client_key_der);

    for name in [
        "ca.cert.pem",
        "ca.cert.der",
        "server.cert.pem",
        "server.key.pem",
        "client.cert.pem",
        "client.key.pem",
        "client.key.der",
    ] {
        assert!(
            fs::metadata(temp_dir.path().join(name))?.is_file(),
            "{name}"
        );
    }
    assert!(fs::metadata(temp_dir.path().join(CertificatePaths::CACHE_MARKER))?.is_file());
    #[cfg(unix)]
    for name in [
        CertificatePaths::SERVER_KEY_PEM,
        CertificatePaths::CLIENT_KEY_PEM,
        CertificatePaths::CLIENT_KEY_DER,
    ] {
        let mode = fs::metadata(temp_dir.path().join(name))?
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "{name}");
    }

    Ok(())
}
