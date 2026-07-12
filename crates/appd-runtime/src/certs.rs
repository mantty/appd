//! Certificate generation and cache handling for the local mTLS bridge.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose, PublicKeyData, SanType,
    SerialNumber,
};
use time::{Duration, OffsetDateTime};
use x509_parser::{parse_x509_certificate, pem::parse_x509_pem};

use crate::RuntimeResult;

const VALIDITY_DAYS: i64 = 90;
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Runtime certificate and key material needed by Bare and platform `WebViews`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CertificateBundle {
    /// CA certificate PEM trusted by Bare for client authentication.
    pub ca_cert_pem: String,
    /// Server certificate PEM used by Bare's TLS server.
    pub server_cert_pem: String,
    /// Server private key PEM used by Bare's TLS server.
    pub server_key_pem: String,
    /// Client certificate PEM.
    pub client_cert_pem: String,
    /// Client private key PEM.
    pub client_key_pem: String,
    /// Client private key PKCS#8 DER for in-memory platform credentials.
    pub client_key_der: Vec<u8>,
    /// CA certificate DER for platform trust APIs.
    pub ca_cert_der: Vec<u8>,
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

        Ok(Self {
            ca_cert_pem: ca_cert.pem(),
            server_cert_pem: server_cert.pem(),
            server_key_pem: server_key.serialize_pem(),
            client_cert_pem: client_cert.pem(),
            client_key_pem: client_key.serialize_pem(),
            client_key_der: client_key.serialized_der().to_vec(),
            ca_cert_der: ca_cert.der().to_vec(),
        })
    }

    /// Load all cached certificate files from a work directory.
    ///
    /// # Errors
    ///
    /// Returns an error when any expected certificate file cannot be read.
    pub fn load_cached(work_dir: impl AsRef<Path>) -> RuntimeResult<Self> {
        let work_dir = work_dir.as_ref();
        if !CertificatePaths::all_exist(work_dir) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "certificate cache is incomplete",
            )
            .into());
        }
        Ok(Self {
            ca_cert_pem: fs::read_to_string(work_dir.join(CertificatePaths::CA_CERT_PEM))?,
            server_cert_pem: fs::read_to_string(work_dir.join(CertificatePaths::SERVER_CERT_PEM))?,
            server_key_pem: fs::read_to_string(work_dir.join(CertificatePaths::SERVER_KEY_PEM))?,
            client_cert_pem: fs::read_to_string(work_dir.join(CertificatePaths::CLIENT_CERT_PEM))?,
            client_key_pem: fs::read_to_string(work_dir.join(CertificatePaths::CLIENT_KEY_PEM))?,
            client_key_der: fs::read(work_dir.join(CertificatePaths::CLIENT_KEY_DER))?,
            ca_cert_der: fs::read(work_dir.join(CertificatePaths::CA_CERT_DER))?,
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
            && certificate_der_matches_pem(&self.ca_cert_pem, &self.ca_cert_der)
            && certificate_matches_key(&self.server_cert_pem, &self.server_key_pem)
            && certificate_matches_key(&self.client_cert_pem, &self.client_key_pem)
            && key_matches_der(&self.client_key_pem, &self.client_key_der)
    }

    /// Write all certificate files expected by Bare and platform `WebViews`.
    ///
    /// # Errors
    ///
    /// Returns an error when the work directory cannot be created or files
    /// cannot be written.
    pub fn write_all(&self, work_dir: impl AsRef<Path>) -> RuntimeResult<()> {
        let work_dir = work_dir.as_ref();
        fs::create_dir_all(work_dir)?;
        set_directory_permissions(work_dir)?;
        remove_if_exists(work_dir.join(CertificatePaths::CACHE_MARKER))?;
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
            (
                CertificatePaths::CLIENT_KEY_DER,
                self.client_key_der.as_slice(),
            ),
            (CertificatePaths::CA_CERT_DER, self.ca_cert_der.as_slice()),
        ] {
            write_atomic(work_dir, name, content, is_private_key(name))?;
        }
        write_atomic(work_dir, CertificatePaths::CACHE_MARKER, &[], true)?;
        Ok(())
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
    /// Client private key PKCS#8 DER filename.
    pub const CLIENT_KEY_DER: &'static str = "client.key.der";
    /// Marker written after every certificate file has been committed.
    pub const CACHE_MARKER: &'static str = ".complete";

    const ALL: &'static [&'static str] = &[
        Self::CA_CERT_PEM,
        Self::CA_CERT_DER,
        Self::SERVER_CERT_PEM,
        Self::SERVER_KEY_PEM,
        Self::CLIENT_CERT_PEM,
        Self::CLIENT_KEY_PEM,
        Self::CLIENT_KEY_DER,
    ];

    /// Return true when all expected certificate files exist.
    #[must_use]
    pub fn all_exist(work_dir: impl AsRef<Path>) -> bool {
        let work_dir = work_dir.as_ref();
        work_dir.join(Self::CACHE_MARKER).is_file()
            && Self::ALL.iter().all(|name| {
                work_dir
                    .join(name)
                    .metadata()
                    .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
            })
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

fn certificate_matches_key(certificate_pem: &str, key_pem: &str) -> bool {
    let Ok((_, pem)) = parse_x509_pem(certificate_pem.as_bytes()) else {
        return false;
    };
    let Ok(certificate) = pem.parse_x509() else {
        return false;
    };
    let Ok(key) = KeyPair::from_pem(key_pem) else {
        return false;
    };
    certificate.public_key().raw == key.subject_public_key_info()
}

fn certificate_der_matches_pem(certificate_pem: &str, certificate_der: &[u8]) -> bool {
    let Ok((_, pem)) = parse_x509_pem(certificate_pem.as_bytes()) else {
        return false;
    };
    pem.contents == certificate_der && parse_x509_certificate(certificate_der).is_ok()
}

fn key_matches_der(key_pem: &str, key_der: &[u8]) -> bool {
    KeyPair::from_pem(key_pem).is_ok_and(|key| key.serialized_der() == key_der)
}

fn is_private_key(name: &str) -> bool {
    matches!(
        name,
        CertificatePaths::SERVER_KEY_PEM
            | CertificatePaths::CLIENT_KEY_PEM
            | CertificatePaths::CLIENT_KEY_DER
    )
}

fn remove_if_exists(path: impl AsRef<Path>) -> RuntimeResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn write_atomic(directory: &Path, name: &str, content: &[u8], private: bool) -> RuntimeResult<()> {
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!(".{name}.{}.{}.tmp", std::process::id(), counter));
    let result = write_temporary(&temporary, content, private)
        .and_then(|()| fs::rename(&temporary, directory.join(name)).map_err(Into::into));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_temporary(path: &Path, content: &[u8], private: bool) -> RuntimeResult<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if private { 0o600 } else { 0o644 });
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options.open(path)?;
    file.write_all(content)?;
    file.sync_all()?;
    Ok(())
}

fn set_directory_permissions(directory: &Path) -> RuntimeResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
