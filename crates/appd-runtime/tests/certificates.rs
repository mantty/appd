use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use appd_runtime::certs::{CertificateBundle, CertificatePaths};
use x509_parser::{
    certificate::X509Certificate,
    extensions::{GeneralName, ParsedExtension},
    parse_x509_certificate,
    pem::parse_x509_pem,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn generates_certificate_bundle_with_platform_required_materials() -> TestResult {
    let bundle = CertificateBundle::generate("app.appd.local")?;
    assert_pem_material(&bundle);

    let ca_der = certificate_der(&bundle.ca_cert_pem)?;
    let server_der = certificate_der(&bundle.server_cert_pem)?;
    let client_der = certificate_der(&bundle.client_cert_pem)?;
    let (_, ca_cert) = parse_x509_certificate(&ca_der)?;
    let (_, server_cert) = parse_x509_certificate(&server_der)?;
    let (_, client_cert) = parse_x509_certificate(&client_der)?;
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
    assert!(bundle.ca_key_pem.starts_with(key));
    assert!(bundle.server_identity_pem.contains(&bundle.server_cert_pem));
    assert!(bundle.server_identity_pem.contains(&bundle.server_key_pem));
    assert!(bundle.client_cert_pem.starts_with(certificate));
    assert!(bundle.client_key_pem.starts_with(key));
    assert!(!bundle.client_key_der.is_empty());
}

fn assert_certificate_chain(
    ca: &X509Certificate<'_>,
    server: &X509Certificate<'_>,
    client: &X509Certificate<'_>,
) -> TestResult {
    assert_eq!(common_name(ca)?, "appd local ca");
    assert_eq!(common_name(server)?, "app.appd.local");
    assert_eq!(common_name(client)?, "appd client");
    assert!(ca.validity().is_valid());
    assert!(server.validity().is_valid());
    assert!(client.validity().is_valid());
    ca.verify_signature(None)?;
    assert_eq!(server.issuer(), ca.subject());
    assert_eq!(client.issuer(), ca.subject());
    server.verify_signature(Some(ca.public_key()))?;
    client.verify_signature(Some(ca.public_key()))?;
    Ok(())
}

fn assert_server_names(server: &X509Certificate<'_>) -> TestResult {
    let names = server
        .extensions()
        .iter()
        .find_map(|extension| {
            let ParsedExtension::SubjectAlternativeName(san) = extension.parsed_extension() else {
                return None;
            };
            Some(&san.general_names)
        })
        .ok_or("server certificate must include SANs")?;
    assert!(
        names
            .iter()
            .any(|name| matches!(name, GeneralName::DNSName("localhost")))
    );
    assert!(
        names
            .iter()
            .any(|name| matches!(name, GeneralName::DNSName("app.appd.local")))
    );
    assert!(
        names
            .iter()
            .any(|name| matches!(name, GeneralName::IPAddress(value) if *value == [127, 0, 0, 1]))
    );

    Ok(())
}

fn assert_certificate_usages(
    ca: &X509Certificate<'_>,
    server: &X509Certificate<'_>,
    client: &X509Certificate<'_>,
) -> TestResult {
    assert!(
        ca.basic_constraints()?
            .ok_or("CA basic constraints are missing")?
            .value
            .ca
    );
    let ca_usage = ca.key_usage()?.ok_or("CA key usage is missing")?.value;
    assert!(ca_usage.key_cert_sign());
    assert!(ca_usage.crl_sign());

    let server_usage = server
        .extended_key_usage()?
        .ok_or("server extended key usage is missing")?
        .value;
    assert!(server_usage.server_auth);
    assert!(!server_usage.client_auth);

    let client_usage = client
        .extended_key_usage()?
        .ok_or("client extended key usage is missing")?
        .value;
    assert!(client_usage.client_auth);
    assert!(!client_usage.server_auth);

    Ok(())
}

fn common_name(cert: &X509Certificate<'_>) -> TestResult<String> {
    let entry = cert
        .subject()
        .iter_common_name()
        .next()
        .ok_or("certificate subject is missing a common name")?;
    Ok(entry.as_str()?.to_owned())
}

fn certificate_der(pem: &str) -> TestResult<Vec<u8>> {
    Ok(parse_x509_pem(pem.as_bytes())?.1.contents)
}

#[test]
fn writes_and_loads_cached_certificate_bundle() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let bundle = CertificateBundle::generate("app.appd.local")?;

    bundle.write_all(temp_dir.path())?;
    assert!(CertificatePaths::all_exist(temp_dir.path()));

    let loaded = CertificateBundle::load_cached(temp_dir.path())?;
    assert_eq!(loaded.ca_cert_der, bundle.ca_cert_der);
    assert_eq!(loaded.client_key_der, bundle.client_key_der);

    for name in [
        "ca.cert.pem",
        "ca.key.pem",
        "ca.cert.der",
        "server.cert.pem",
        "server.key.pem",
        "server.identity.pem",
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
        CertificatePaths::CA_KEY_PEM,
        CertificatePaths::SERVER_IDENTITY_PEM,
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
