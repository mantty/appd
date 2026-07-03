//! Certificate generation and cache handling for the local mTLS bridge.

use std::fs;
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;

use base64::Engine;
use p12_keystore::{
    Certificate as Pkcs12Certificate, KeyStore, KeyStoreEntry, PrivateKey, PrivateKeyChain,
};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose, SanType, SerialNumber,
};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use x509_parser::pem::parse_x509_pem;

use crate::{RuntimeError, RuntimeResult};

const P12_PASSWORD: &str = "appd-internal";
const VALIDITY_DAYS: i64 = 90;

/// Runtime certificate and key material needed by `workerd` and platform `WebViews`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CertificateBundle {
    /// CA certificate PEM, embedded into workerd as a trusted client CA.
    pub ca_cert_pem: String,
    /// Server certificate PEM, embedded into workerd's TLS keypair.
    pub server_cert_pem: String,
    /// Server private key PEM, embedded into workerd's TLS keypair.
    pub server_key_pem: String,
    /// Client certificate PEM.
    pub client_cert_pem: String,
    /// Client private key PEM.
    pub client_key_pem: String,
    /// CA certificate DER for platform trust APIs.
    pub ca_cert_der: Vec<u8>,
    /// Client identity PKCS#12 DER for platform client certificate APIs.
    pub client_p12_der: Vec<u8>,
    /// Password for [`Self::client_p12_der`].
    pub p12_password: String,
}

impl CertificateBundle {
    /// Generate a fresh ECDSA P-256/SHA-256 certificate bundle.
    ///
    /// The generated shape matches the Zig runtime: a self-signed local CA, a
    /// localhost server certificate with `DNS:localhost` and `IP:127.0.0.1`
    /// SANs, and a client-auth certificate.
    ///
    /// # Errors
    ///
    /// Returns an error when key generation, certificate signing, encoding, or
    /// system clock conversion fails.
    pub fn generate() -> RuntimeResult<Self> {
        let ca_key = KeyPair::generate()?;
        let server_key = KeyPair::generate()?;
        let client_key = KeyPair::generate()?;

        let (ca_params, ca_cert) = build_ca_certificate(&ca_key)?;
        let ca_issuer = Issuer::from_params(&ca_params, &ca_key);
        let server_cert = build_server_certificate(&server_key, &ca_issuer)?;
        let client_cert = build_client_certificate(&client_key, &ca_issuer)?;

        let client_p12_der = build_client_pkcs12(&client_cert, &client_key, &ca_cert)?;

        Ok(Self {
            ca_cert_pem: ca_cert.pem(),
            server_cert_pem: server_cert.pem(),
            server_key_pem: server_key.serialize_pem(),
            client_cert_pem: client_cert.pem(),
            client_key_pem: client_key.serialize_pem(),
            ca_cert_der: ca_cert.der().to_vec(),
            client_p12_der,
            p12_password: P12_PASSWORD.to_owned(),
        })
    }

    /// Load all cached certificate files from a work directory.
    ///
    /// # Errors
    ///
    /// Returns an error when any expected certificate file cannot be read.
    pub fn load_cached(work_dir: impl AsRef<Path>) -> RuntimeResult<Self> {
        let work_dir = work_dir.as_ref();
        Ok(Self {
            ca_cert_pem: fs::read_to_string(work_dir.join(CertificatePaths::CA_CERT_PEM))?,
            server_cert_pem: fs::read_to_string(work_dir.join(CertificatePaths::SERVER_CERT_PEM))?,
            server_key_pem: fs::read_to_string(work_dir.join(CertificatePaths::SERVER_KEY_PEM))?,
            client_cert_pem: fs::read_to_string(work_dir.join(CertificatePaths::CLIENT_CERT_PEM))?,
            client_key_pem: fs::read_to_string(work_dir.join(CertificatePaths::CLIENT_KEY_PEM))?,
            ca_cert_der: fs::read(work_dir.join(CertificatePaths::CA_CERT_DER))?,
            client_p12_der: fs::read(work_dir.join(CertificatePaths::CLIENT_P12))?,
            p12_password: P12_PASSWORD.to_owned(),
        })
    }

    pub(crate) fn cached_material_is_current(&self, now: OffsetDateTime) -> bool {
        [
            self.ca_cert_pem.as_str(),
            self.server_cert_pem.as_str(),
            self.client_cert_pem.as_str(),
        ]
        .into_iter()
        .all(|pem| certificate_is_valid_now(pem, now))
    }

    /// Write all certificate files expected by `workerd` and platform `WebViews`.
    ///
    /// # Errors
    ///
    /// Returns an error when the work directory cannot be created or files
    /// cannot be written.
    pub fn write_all(&self, work_dir: impl AsRef<Path>) -> RuntimeResult<()> {
        let work_dir = work_dir.as_ref();
        fs::create_dir_all(work_dir)?;
        for (name, content) in [
            (CertificatePaths::CA_CERT_PEM, self.ca_cert_pem.as_bytes()),
            (
                CertificatePaths::SERVER_CERT_PEM,
                self.server_cert_pem.as_bytes(),
            ),
            (
                CertificatePaths::SERVER_KEY_PEM,
                self.server_key_pem.as_bytes(),
            ),
            (
                CertificatePaths::CLIENT_CERT_PEM,
                self.client_cert_pem.as_bytes(),
            ),
            (
                CertificatePaths::CLIENT_KEY_PEM,
                self.client_key_pem.as_bytes(),
            ),
            (CertificatePaths::CA_CERT_DER, self.ca_cert_der.as_slice()),
            (CertificatePaths::CLIENT_P12, self.client_p12_der.as_slice()),
        ] {
            fs::write(work_dir.join(name), content)?;
        }
        Ok(())
    }

    /// Return Chromium's `--ignore-certificate-errors-spki-list` value for the
    /// generated server certificate.
    ///
    /// # Errors
    ///
    /// Returns an error when the server certificate cannot be parsed or hashed.
    pub fn server_spki_sha256_base64(&self) -> RuntimeResult<String> {
        let (_, pem) = parse_x509_pem(self.server_cert_pem.as_bytes())
            .map_err(|error| RuntimeError::CertificateParsing(format!("{error:?}")))?;
        let cert = pem
            .parse_x509()
            .map_err(|error| RuntimeError::CertificateParsing(format!("{error:?}")))?;
        let digest = Sha256::digest(cert.public_key().raw);
        Ok(base64::engine::general_purpose::STANDARD.encode(digest))
    }
}

/// Canonical certificate filenames used in each packaged app work directory.
pub struct CertificatePaths;

impl CertificatePaths {
    /// CA certificate PEM filename.
    pub const CA_CERT_PEM: &'static str = "ca.cert.pem";
    /// CA certificate DER filename.
    pub const CA_CERT_DER: &'static str = "ca.cert.der";
    /// Server certificate PEM filename.
    pub const SERVER_CERT_PEM: &'static str = "server.cert.pem";
    /// Server private key PEM filename.
    pub const SERVER_KEY_PEM: &'static str = "server.key.pem";
    /// Client certificate PEM filename.
    pub const CLIENT_CERT_PEM: &'static str = "client.cert.pem";
    /// Client private key PEM filename.
    pub const CLIENT_KEY_PEM: &'static str = "client.key.pem";
    /// Client identity PKCS#12 filename.
    pub const CLIENT_P12: &'static str = "client.p12";

    const ALL: &'static [&'static str] = &[
        Self::CA_CERT_PEM,
        Self::CA_CERT_DER,
        Self::SERVER_CERT_PEM,
        Self::SERVER_KEY_PEM,
        Self::CLIENT_CERT_PEM,
        Self::CLIENT_KEY_PEM,
        Self::CLIENT_P12,
    ];

    /// Return true when all expected certificate files exist.
    #[must_use]
    pub fn all_exist(work_dir: impl AsRef<Path>) -> bool {
        let work_dir = work_dir.as_ref();
        Self::ALL.iter().all(|name| work_dir.join(name).is_file())
    }
}

fn build_ca_certificate(key: &KeyPair) -> RuntimeResult<(CertificateParams, Certificate)> {
    let mut params = base_certificate_params("appd local ca", 1);
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let certificate = params.self_signed(key)?;
    Ok((params, certificate))
}

fn build_server_certificate(
    key: &KeyPair,
    ca_issuer: &Issuer<'_, &KeyPair>,
) -> RuntimeResult<Certificate> {
    let mut params = base_certificate_params("localhost", 2);
    params.subject_alt_names = vec![
        SanType::DnsName("localhost".try_into()?),
        SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)),
    ];
    params.is_ca = IsCa::ExplicitNoCa;
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    Ok(params.signed_by(key, ca_issuer)?)
}

fn build_client_certificate(
    key: &KeyPair,
    ca_issuer: &Issuer<'_, &KeyPair>,
) -> RuntimeResult<Certificate> {
    let mut params = base_certificate_params("appd client", 3);
    params.is_ca = IsCa::ExplicitNoCa;
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    Ok(params.signed_by(key, ca_issuer)?)
}

fn base_certificate_params(common_name: &str, serial: u64) -> CertificateParams {
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::CommonName, common_name);
    let now = OffsetDateTime::now_utc();
    let mut params = CertificateParams::default();
    params.not_before = now - Duration::days(1);
    params.not_after = now + Duration::days(VALIDITY_DAYS);
    params.serial_number = Some(SerialNumber::from(serial));
    params.distinguished_name = distinguished_name;
    params
}

fn certificate_is_valid_now(pem: &str, now: OffsetDateTime) -> bool {
    let Ok((_, pem)) = parse_x509_pem(pem.as_bytes()) else {
        return false;
    };
    let Ok(cert) = pem.parse_x509() else {
        return false;
    };
    let now = now.unix_timestamp();
    let validity = cert.validity();
    validity.not_before.timestamp() <= now && now < validity.not_after.timestamp()
}

fn build_client_pkcs12(
    cert: &Certificate,
    key: &KeyPair,
    ca_cert: &Certificate,
) -> RuntimeResult<Vec<u8>> {
    let client_cert = Pkcs12Certificate::from_der(cert.der().as_ref())?;
    let ca_cert = Pkcs12Certificate::from_der(ca_cert.der().as_ref())?;
    let private_key = PrivateKey::from_der(key.serialized_der())?;
    let key_chain = PrivateKeyChain::new("appd-client", private_key, [client_cert, ca_cert]);

    let mut key_store = KeyStore::new();
    key_store.add_entry("appd-client", KeyStoreEntry::PrivateKeyChain(key_chain));
    Ok(key_store.writer(P12_PASSWORD).write()?)
}
