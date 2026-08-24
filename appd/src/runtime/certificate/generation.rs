//! Certificate generation helpers.

use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose, SanType, SerialNumber,
    SigningKey,
};
use std::net::{IpAddr, Ipv4Addr};

use time::{Duration, OffsetDateTime};

use crate::Result;

const CA_VALIDITY_DAYS: i64 = 3_650;
const LEAF_VALIDITY_DAYS: i64 = 90;

pub(super) fn build_ca_certificate(
    key: &KeyPair,
    now: OffsetDateTime,
) -> Result<(CertificateParams, Certificate)> {
    let mut params = base_certificate_params("appd local ca", 1, now, CA_VALIDITY_DAYS);
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let certificate = params.self_signed(key)?;
    Ok((params, certificate))
}

pub(super) fn build_server_certificate<S: SigningKey>(
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

pub(super) fn build_client_certificate<S: SigningKey>(
    key: &KeyPair,
    ca_issuer: &Issuer<'_, S>,
    now: OffsetDateTime,
) -> Result<Certificate> {
    let mut params = base_certificate_params("appd client", 3, now, LEAF_VALIDITY_DAYS);
    params.is_ca = IsCa::ExplicitNoCa;
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    Ok(params.signed_by(key, ca_issuer)?)
}

pub(super) fn base_certificate_params(
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

pub(super) fn server_identity(certificate: &str, key: &str) -> String {
    format!("{certificate}{key}")
}
