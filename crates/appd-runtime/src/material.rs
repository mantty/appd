//! Certificate generation, validation, and cache handling.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose, PublicKeyData, SanType,
    SerialNumber, SigningKey,
};
use time::{Duration, OffsetDateTime};
use x509_parser::{extensions::GeneralName, parse_x509_certificate, pem::parse_x509_pem};

use crate::Result;

const CA_VALIDITY_DAYS: i64 = 3_650;
const LEAF_VALIDITY_DAYS: i64 = 90;
pub(crate) const LEAF_RENEWAL_DAYS: i64 = 30;
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Runtime certificate and key material needed by appd and platform `WebViews`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CertificateBundle {
    /// CA certificate PEM trusted by the local gateway for client authentication.
    pub ca_cert_pem: String,
    /// CA private key PEM used to issue replacement leaf certificates.
    pub ca_key_pem: String,
    /// Server certificate PEM used by the local TLS gateway.
    pub server_cert_pem: String,
    /// Server private key PEM used by the local TLS gateway.
    pub server_key_pem: String,
    /// Server certificate and key PEM consumed atomically by the local gateway.
    pub server_identity_pem: String,
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
    /// Return a valid bundle for `host`, generating or renewing what is due.
    ///
    /// # Errors
    ///
    /// Returns an error when generation, cache loading, or cache writing fails.
    pub(crate) fn ensure(work_dir: &Path, host: &str, now: OffsetDateTime) -> Result<Self> {
        let Ok(bundle) = Self::load_cached(work_dir) else {
            return Self::renew_or_generate(work_dir, host, now);
        };
        if !bundle.issuer_is_current(now) {
            return Self::generate_and_cache(work_dir, host, now);
        }
        if !bundle.cached_material_is_current(now)
            || !bundle.server_certificate_matches_host(host)
            || bundle.leaf_renewal_is_due(now)
        {
            let replacement = bundle.renew_leaves(host, now)?;
            replacement.write_leaves(work_dir)?;
            return Ok(replacement);
        }
        Ok(bundle)
    }

    fn renew_or_generate(work_dir: &Path, host: &str, now: OffsetDateTime) -> Result<Self> {
        let Ok(issuer) = Self::load_issuer(work_dir) else {
            return Self::generate_and_cache(work_dir, host, now);
        };
        if !issuer.issuer_is_current(now) {
            return Self::generate_and_cache(work_dir, host, now);
        }
        let replacement = issuer.renew_leaves(host, now)?;
        replacement.write_all(work_dir)?;
        Ok(replacement)
    }

    fn generate_and_cache(work_dir: &Path, host: &str, now: OffsetDateTime) -> Result<Self> {
        let bundle = Self::generate_at(host, now)?;
        bundle.write_all(work_dir)?;
        Ok(bundle)
    }

    /// The client certificate in DER, for platform credential APIs.
    pub(crate) fn client_certificate_der(&self) -> Option<Vec<u8>> {
        certificate_der(&self.client_cert_pem)
    }

    /// Generate a self-signed local CA, an app-origin server certificate with
    /// loopback SANs, and a client-auth certificate, all ECDSA P-256/SHA-256.
    pub(crate) fn generate_at(host: &str, now: OffsetDateTime) -> Result<Self> {
        let ca_key = KeyPair::generate()?;
        let server_key = KeyPair::generate()?;
        let client_key = KeyPair::generate()?;

        let (ca_params, ca_cert) = build_ca_certificate(&ca_key, now)?;
        let ca_issuer = Issuer::from_params(&ca_params, &ca_key);
        let server_cert = build_server_certificate(&server_key, &ca_issuer, host, now)?;
        let client_cert = build_client_certificate(&client_key, &ca_issuer, now)?;
        let server_cert_pem = server_cert.pem();
        let server_key_pem = server_key.serialize_pem();
        let client_cert_pem = client_cert.pem();
        let client_key_pem = client_key.serialize_pem();

        Ok(Self {
            ca_cert_pem: ca_cert.pem(),
            ca_key_pem: ca_key.serialize_pem(),
            server_identity_pem: server_identity(&server_cert_pem, &server_key_pem),
            server_cert_pem,
            server_key_pem,
            client_cert_pem,
            client_key_pem,
            client_key_der: client_key.serialized_der().to_vec(),
            ca_cert_der: ca_cert.der().to_vec(),
        })
    }

    pub(crate) fn renew_leaves(&self, host: &str, now: OffsetDateTime) -> Result<Self> {
        let ca_key = KeyPair::from_pem(&self.ca_key_pem)?;
        let ca_issuer = Issuer::from_ca_cert_pem(&self.ca_cert_pem, ca_key)?;
        let server_key = KeyPair::generate()?;
        let client_key = KeyPair::generate()?;
        let server_cert = build_server_certificate(&server_key, &ca_issuer, host, now)?;
        let client_cert = build_client_certificate(&client_key, &ca_issuer, now)?;

        let server_cert_pem = server_cert.pem();
        let server_key_pem = server_key.serialize_pem();
        Ok(Self {
            ca_cert_pem: self.ca_cert_pem.clone(),
            ca_key_pem: self.ca_key_pem.clone(),
            server_identity_pem: server_identity(&server_cert_pem, &server_key_pem),
            server_cert_pem,
            server_key_pem,
            client_cert_pem: client_cert.pem(),
            client_key_pem: client_key.serialize_pem(),
            client_key_der: client_key.serialized_der().to_vec(),
            ca_cert_der: self.ca_cert_der.clone(),
        })
    }

    /// Load all cached certificate files from a work directory.
    ///
    /// # Errors
    ///
    /// Returns an error when any expected certificate file cannot be read.
    pub fn load_cached(work_dir: impl AsRef<Path>) -> Result<Self> {
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
            ca_key_pem: fs::read_to_string(work_dir.join(CertificatePaths::CA_KEY_PEM))?,
            server_cert_pem: fs::read_to_string(work_dir.join(CertificatePaths::SERVER_CERT_PEM))?,
            server_key_pem: fs::read_to_string(work_dir.join(CertificatePaths::SERVER_KEY_PEM))?,
            server_identity_pem: fs::read_to_string(
                work_dir.join(CertificatePaths::SERVER_IDENTITY_PEM),
            )?,
            client_cert_pem: fs::read_to_string(work_dir.join(CertificatePaths::CLIENT_CERT_PEM))?,
            client_key_pem: fs::read_to_string(work_dir.join(CertificatePaths::CLIENT_KEY_PEM))?,
            client_key_der: fs::read(work_dir.join(CertificatePaths::CLIENT_KEY_DER))?,
            ca_cert_der: fs::read(work_dir.join(CertificatePaths::CA_CERT_DER))?,
        })
    }

    pub(crate) fn load_issuer(work_dir: &Path) -> Result<Self> {
        Ok(Self {
            ca_cert_pem: fs::read_to_string(work_dir.join(CertificatePaths::CA_CERT_PEM))?,
            ca_key_pem: fs::read_to_string(work_dir.join(CertificatePaths::CA_KEY_PEM))?,
            server_cert_pem: String::new(),
            server_key_pem: String::new(),
            server_identity_pem: String::new(),
            client_cert_pem: String::new(),
            client_key_pem: String::new(),
            client_key_der: Vec::new(),
            ca_cert_der: fs::read(work_dir.join(CertificatePaths::CA_CERT_DER))?,
        })
    }

    pub(crate) fn cached_material_is_current(&self, now: OffsetDateTime) -> bool {
        self.issuer_is_current(now)
            && self.leaves_are_current(now)
            && certificate_matches_key(&self.server_cert_pem, &self.server_key_pem)
            && certificate_matches_key(&self.client_cert_pem, &self.client_key_pem)
            && key_matches_der(&self.client_key_pem, &self.client_key_der)
            && certificate_is_issued_by(&self.server_cert_pem, &self.ca_cert_pem)
            && certificate_is_issued_by(&self.client_cert_pem, &self.ca_cert_pem)
            && self.server_identity_pem
                == server_identity(&self.server_cert_pem, &self.server_key_pem)
    }

    pub(crate) fn issuer_is_current(&self, now: OffsetDateTime) -> bool {
        certificate_is_valid_now(&self.ca_cert_pem, now)
            && certificate_der_matches_pem(&self.ca_cert_pem, &self.ca_cert_der)
            && certificate_matches_key(&self.ca_cert_pem, &self.ca_key_pem)
    }

    fn leaves_are_current(&self, now: OffsetDateTime) -> bool {
        [self.server_cert_pem.as_str(), self.client_cert_pem.as_str()]
            .into_iter()
            .all(|pem| certificate_is_valid_now(pem, now))
    }

    pub(crate) fn leaf_renewal_is_due(&self, now: OffsetDateTime) -> bool {
        certificate_not_before(&self.server_cert_pem)
            .is_none_or(|not_before| now >= not_before + Duration::days(LEAF_RENEWAL_DAYS))
    }

    pub(crate) fn renewal_delay(&self, now: OffsetDateTime) -> std::time::Duration {
        let Some(not_before) = certificate_not_before(&self.server_cert_pem) else {
            return std::time::Duration::ZERO;
        };
        let seconds = u64::try_from(
            (not_before + Duration::days(LEAF_RENEWAL_DAYS) - now)
                .whole_seconds()
                .max(0),
        )
        .unwrap_or_default();
        std::time::Duration::from_secs(seconds)
    }

    pub(crate) fn server_certificate_matches_host(&self, host: &str) -> bool {
        let Ok((_, pem)) = parse_x509_pem(self.server_cert_pem.as_bytes()) else {
            return false;
        };
        let Ok(certificate) = pem.parse_x509() else {
            return false;
        };
        certificate.extensions().iter().any(|extension| {
            let x509_parser::extensions::ParsedExtension::SubjectAlternativeName(san) =
                extension.parsed_extension()
            else {
                return false;
            };
            san.general_names
                .iter()
                .any(|name| matches!(name, GeneralName::DNSName(value) if *value == host))
        })
    }

    /// Write all certificate files expected by appd and platform `WebViews`.
    ///
    /// # Errors
    ///
    /// Returns an error when the work directory cannot be created or files
    /// cannot be written.
    pub fn write_all(&self, work_dir: impl AsRef<Path>) -> Result<()> {
        let work_dir = work_dir.as_ref();
        fs::create_dir_all(work_dir)?;
        set_directory_permissions(work_dir)?;
        remove_if_exists(work_dir.join(CertificatePaths::CACHE_MARKER))?;
        for (name, content) in [
            (CertificatePaths::CA_CERT_PEM, self.ca_cert_pem.as_bytes()),
            (CertificatePaths::CA_KEY_PEM, self.ca_key_pem.as_bytes()),
            (
                CertificatePaths::SERVER_CERT_PEM,
                self.server_cert_pem.as_bytes(),
            ),
            (
                CertificatePaths::SERVER_KEY_PEM,
                self.server_key_pem.as_bytes(),
            ),
            (
                CertificatePaths::SERVER_IDENTITY_PEM,
                self.server_identity_pem.as_bytes(),
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

    pub(crate) fn write_leaves(&self, work_dir: &Path) -> Result<()> {
        remove_if_exists(work_dir.join(CertificatePaths::CACHE_MARKER))?;
        for (name, content) in [
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
            (
                CertificatePaths::SERVER_IDENTITY_PEM,
                self.server_identity_pem.as_bytes(),
            ),
        ] {
            write_atomic(work_dir, name, content, is_private_key(name))?;
        }
        write_atomic(work_dir, CertificatePaths::CACHE_MARKER, &[], true)
    }
}

/// Canonical certificate filenames used in each packaged app work directory.
pub struct CertificatePaths;

impl CertificatePaths {
    /// CA certificate PEM filename.
    pub const CA_CERT_PEM: &'static str = "ca.cert.pem";
    /// CA private key PEM filename.
    pub const CA_KEY_PEM: &'static str = "ca.key.pem";
    /// CA certificate DER filename.
    pub const CA_CERT_DER: &'static str = "ca.cert.der";
    /// Server certificate PEM filename.
    pub const SERVER_CERT_PEM: &'static str = "server.cert.pem";
    /// Server private key PEM filename.
    pub const SERVER_KEY_PEM: &'static str = "server.key.pem";
    /// Server certificate and key PEM filename.
    pub const SERVER_IDENTITY_PEM: &'static str = "server.identity.pem";
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
        Self::CA_KEY_PEM,
        Self::CA_CERT_DER,
        Self::SERVER_CERT_PEM,
        Self::SERVER_KEY_PEM,
        Self::SERVER_IDENTITY_PEM,
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

fn build_ca_certificate(
    key: &KeyPair,
    now: OffsetDateTime,
) -> Result<(CertificateParams, Certificate)> {
    let mut params = base_certificate_params("appd local ca", 1, now, CA_VALIDITY_DAYS);
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let certificate = params.self_signed(key)?;
    Ok((params, certificate))
}

fn build_server_certificate<S: SigningKey>(
    key: &KeyPair,
    ca_issuer: &Issuer<'_, S>,
    host: &str,
    now: OffsetDateTime,
) -> Result<Certificate> {
    let mut params = base_certificate_params(host, 2, now, LEAF_VALIDITY_DAYS);
    params.subject_alt_names = vec![
        SanType::DnsName(host.try_into()?),
        SanType::DnsName("localhost".try_into()?),
        SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)),
    ];
    params.is_ca = IsCa::ExplicitNoCa;
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    Ok(params.signed_by(key, ca_issuer)?)
}

fn build_client_certificate<S: SigningKey>(
    key: &KeyPair,
    ca_issuer: &Issuer<'_, S>,
    now: OffsetDateTime,
) -> Result<Certificate> {
    let mut params = base_certificate_params("appd client", 3, now, LEAF_VALIDITY_DAYS);
    params.is_ca = IsCa::ExplicitNoCa;
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    Ok(params.signed_by(key, ca_issuer)?)
}

fn base_certificate_params(
    common_name: &str,
    serial: u64,
    now: OffsetDateTime,
    validity_days: i64,
) -> CertificateParams {
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::CommonName, common_name);
    let mut params = CertificateParams::default();
    params.not_before = now - Duration::days(1);
    params.not_after = now + Duration::days(validity_days);
    params.serial_number = Some(SerialNumber::from(serial));
    params.distinguished_name = distinguished_name;
    params
}

fn server_identity(certificate: &str, key: &str) -> String {
    format!("{certificate}{key}")
}

fn certificate_not_before(pem: &str) -> Option<OffsetDateTime> {
    let (_, pem) = parse_x509_pem(pem.as_bytes()).ok()?;
    let certificate = pem.parse_x509().ok()?;
    OffsetDateTime::from_unix_timestamp(certificate.validity().not_before.timestamp()).ok()
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

fn certificate_is_issued_by(certificate_pem: &str, issuer_pem: &str) -> bool {
    let Ok((_, certificate_pem)) = parse_x509_pem(certificate_pem.as_bytes()) else {
        return false;
    };
    let Ok(certificate) = certificate_pem.parse_x509() else {
        return false;
    };
    let Ok((_, issuer_pem)) = parse_x509_pem(issuer_pem.as_bytes()) else {
        return false;
    };
    let Ok(issuer) = issuer_pem.parse_x509() else {
        return false;
    };
    certificate.issuer() == issuer.subject()
        && certificate
            .verify_signature(Some(issuer.public_key()))
            .is_ok()
}

pub(crate) fn certificate_der(pem: &str) -> Option<Vec<u8>> {
    parse_x509_pem(pem.as_bytes())
        .ok()
        .map(|(_, certificate)| certificate.contents)
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
            | CertificatePaths::CA_KEY_PEM
            | CertificatePaths::SERVER_IDENTITY_PEM
            | CertificatePaths::CLIENT_KEY_PEM
            | CertificatePaths::CLIENT_KEY_DER
    )
}

fn remove_if_exists(path: impl AsRef<Path>) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn write_atomic(directory: &Path, name: &str, content: &[u8], private: bool) -> Result<()> {
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!(".{name}.{}.{}.tmp", std::process::id(), counter));
    let result = write_temporary(&temporary, content, private)
        .and_then(|()| fs::rename(&temporary, directory.join(name)).map_err(Into::into));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_temporary(path: &Path, content: &[u8], private: bool) -> Result<()> {
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

#[cfg(unix)]
fn set_directory_permissions(directory: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_directory_permissions(_: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CertificateBundle, CertificatePaths};
    use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
    use time::{Duration, OffsetDateTime};
    use x509_parser::{
        certificate::X509Certificate,
        extensions::{GeneralName, ParsedExtension},
        parse_x509_certificate,
        pem::parse_x509_pem,
    };

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    fn ensure(work_dir: &std::path::Path) -> TestResult<CertificateBundle> {
        Ok(CertificateBundle::ensure(
            work_dir,
            "app.appd.local",
            OffsetDateTime::now_utc(),
        )?)
    }

    #[test]
    fn generates_then_reuses_a_cached_bundle() -> TestResult {
        let directory = tempfile::tempdir()?;

        let first = ensure(directory.path())?;
        assert!(CertificatePaths::all_exist(directory.path()));
        let second = ensure(directory.path())?;

        assert_eq!(first.ca_cert_der, second.ca_cert_der);
        Ok(())
    }

    #[test]
    fn generates_platform_certificate_material() -> TestResult {
        let bundle = CertificateBundle::generate_at("app.appd.local", OffsetDateTime::now_utc())?;
        assert_pem_material(&bundle);

        let ca_der = decode_certificate(&bundle.ca_cert_pem)?;
        let server_der = decode_certificate(&bundle.server_cert_pem)?;
        let client_der = decode_certificate(&bundle.client_cert_pem)?;
        let (_, ca) = parse_x509_certificate(&ca_der)?;
        let (_, server) = parse_x509_certificate(&server_der)?;
        let (_, client) = parse_x509_certificate(&client_der)?;

        assert_certificate_chain(&ca, &server, &client)?;
        assert_server_names(&server)?;
        assert_certificate_usages(&ca, &server, &client)?;
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn writes_private_material_with_private_permissions() -> TestResult {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir()?;
        ensure(directory.path())?;

        for name in [
            CertificatePaths::SERVER_KEY_PEM,
            CertificatePaths::CA_KEY_PEM,
            CertificatePaths::SERVER_IDENTITY_PEM,
            CertificatePaths::CLIENT_KEY_PEM,
            CertificatePaths::CLIENT_KEY_DER,
        ] {
            let mode = std::fs::metadata(directory.path().join(name))?
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "{name}");
        }
        Ok(())
    }

    #[test]
    fn recovers_from_a_partial_cache() -> TestResult {
        let directory = tempfile::tempdir()?;
        let stale = directory.path().join(CertificatePaths::SERVER_CERT_PEM);
        std::fs::write(&stale, "not a certificate")?;

        let bundle = ensure(directory.path())?;

        assert!(CertificatePaths::all_exist(directory.path()));
        assert_eq!(std::fs::read_to_string(stale)?, bundle.server_cert_pem);
        Ok(())
    }

    #[test]
    fn regenerates_an_expired_cache() -> TestResult {
        let directory = tempfile::tempdir()?;
        let first = ensure(directory.path())?;
        let expired = expired_certificate_pem()?;
        std::fs::write(
            directory.path().join(CertificatePaths::SERVER_CERT_PEM),
            expired.as_bytes(),
        )?;

        let second = ensure(directory.path())?;

        assert_ne!(second.server_cert_pem, expired);
        assert_ne!(second.server_cert_pem, first.server_cert_pem);
        assert_eq!(second.ca_cert_pem, first.ca_cert_pem);
        Ok(())
    }

    #[test]
    fn rotates_leaves_without_rotating_the_issuer() -> TestResult {
        let directory = tempfile::tempdir()?;
        let first = ensure(directory.path())?;

        let second = CertificateBundle::ensure(
            directory.path(),
            "app.appd.local",
            OffsetDateTime::now_utc() + Duration::days(31),
        )?;

        assert_eq!(second.ca_cert_pem, first.ca_cert_pem);
        assert_eq!(second.ca_key_pem, first.ca_key_pem);
        assert_ne!(second.server_cert_pem, first.server_cert_pem);
        assert_ne!(second.client_cert_pem, first.client_cert_pem);
        assert!(second.server_identity_pem.contains(&second.server_cert_pem));
        assert!(second.server_identity_pem.contains(&second.server_key_pem));
        Ok(())
    }

    #[test]
    fn recovers_missing_leaves_without_rotating_the_issuer() -> TestResult {
        let directory = tempfile::tempdir()?;
        let first = ensure(directory.path())?;
        std::fs::remove_file(directory.path().join(CertificatePaths::CLIENT_CERT_PEM))?;

        let second = ensure(directory.path())?;

        assert_eq!(second.ca_cert_pem, first.ca_cert_pem);
        assert_ne!(second.client_cert_pem, first.client_cert_pem);
        assert!(CertificatePaths::all_exist(directory.path()));
        Ok(())
    }

    #[test]
    fn regenerates_a_corrupt_authority_cache() -> TestResult {
        let directory = tempfile::tempdir()?;
        let first = ensure(directory.path())?;
        std::fs::write(
            directory.path().join(CertificatePaths::CA_CERT_DER),
            [0_u8, 1_u8],
        )?;

        let second = ensure(directory.path())?;

        assert_ne!(second.ca_cert_der, [0_u8, 1_u8]);
        assert_ne!(second.ca_cert_der, first.ca_cert_der);
        assert!(CertificatePaths::all_exist(directory.path()));
        Ok(())
    }

    #[test]
    fn regenerates_a_corrupt_client_key_cache() -> TestResult {
        let directory = tempfile::tempdir()?;
        let first = ensure(directory.path())?;
        std::fs::write(
            directory.path().join(CertificatePaths::CLIENT_KEY_DER),
            [0_u8, 1_u8],
        )?;

        let second = ensure(directory.path())?;

        assert_ne!(second.client_key_der, [0_u8, 1_u8]);
        assert_ne!(second.client_key_der, first.client_key_der);
        Ok(())
    }

    #[test]
    fn regenerates_a_mismatched_authority_cache() -> TestResult {
        let directory = tempfile::tempdir()?;
        let first = ensure(directory.path())?;
        let foreign = CertificateBundle::generate_at("app.appd.local", OffsetDateTime::now_utc())?;
        std::fs::write(
            directory.path().join(CertificatePaths::CA_CERT_DER),
            &foreign.ca_cert_der,
        )?;

        let second = ensure(directory.path())?;

        assert_ne!(second.ca_cert_der, foreign.ca_cert_der);
        assert_ne!(second.ca_cert_der, first.ca_cert_der);
        Ok(())
    }

    #[test]
    fn exposes_the_client_certificate_in_der() -> TestResult {
        let bundle = CertificateBundle::generate_at("app.appd.local", OffsetDateTime::now_utc())?;

        let der = bundle
            .client_certificate_der()
            .ok_or_else(|| std::io::Error::other("client certificate should decode"))?;

        assert!(!der.is_empty());
        Ok(())
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

    fn assert_pem_material(bundle: &CertificateBundle) {
        const CERTIFICATE: &str = "-----BEGIN CERTIFICATE-----";
        const PRIVATE_KEY: &str = "-----BEGIN PRIVATE KEY-----";

        assert!(bundle.ca_cert_pem.starts_with(CERTIFICATE));
        assert!(bundle.server_cert_pem.starts_with(CERTIFICATE));
        assert!(bundle.server_key_pem.starts_with(PRIVATE_KEY));
        assert!(bundle.ca_key_pem.starts_with(PRIVATE_KEY));
        assert!(bundle.server_identity_pem.contains(&bundle.server_cert_pem));
        assert!(bundle.server_identity_pem.contains(&bundle.server_key_pem));
        assert!(bundle.client_cert_pem.starts_with(CERTIFICATE));
        assert!(bundle.client_key_pem.starts_with(PRIVATE_KEY));
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
                let ParsedExtension::SubjectAlternativeName(san) = extension.parsed_extension()
                else {
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
            names.iter().any(
                |name| matches!(name, GeneralName::IPAddress(value) if *value == [127, 0, 0, 1])
            )
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

    fn common_name(certificate: &X509Certificate<'_>) -> TestResult<String> {
        let entry = certificate
            .subject()
            .iter_common_name()
            .next()
            .ok_or("certificate subject is missing a common name")?;
        Ok(entry.as_str()?.to_owned())
    }

    fn decode_certificate(pem: &str) -> TestResult<Vec<u8>> {
        Ok(parse_x509_pem(pem.as_bytes())?.1.contents)
    }
}
