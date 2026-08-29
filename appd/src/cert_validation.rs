//! Certificate parsing and validation helpers.

use rcgen::{KeyPair, PublicKeyData};
use time::OffsetDateTime;
use x509_parser::{parse_x509_certificate, pem::parse_x509_pem};

pub(crate) fn certificate_der(pem: &str) -> Option<Vec<u8>> {
    parse_x509_pem(pem.as_bytes())
        .ok()
        .map(|(_, certificate)| certificate.contents)
}

pub(crate) fn certificate_der_matches_pem(certificate_pem: &str, certificate_der: &[u8]) -> bool {
    let Ok((_, pem)) = parse_x509_pem(certificate_pem.as_bytes()) else {
        return false;
    };
    pem.contents == certificate_der && parse_x509_certificate(certificate_der).is_ok()
}

pub(crate) fn key_matches_der(key_pem: &str, key_der: &[u8]) -> bool {
    KeyPair::from_pem(key_pem).is_ok_and(|key| key.serialized_der() == key_der)
}

pub(crate) fn certificate_not_before(pem: &str) -> Option<OffsetDateTime> {
    let (_, pem) = parse_x509_pem(pem.as_bytes()).ok()?;
    let certificate = pem.parse_x509().ok()?;
    OffsetDateTime::from_unix_timestamp(certificate.validity().not_before.timestamp()).ok()
}

pub(crate) fn certificate_is_valid_now(pem: &str, now: OffsetDateTime) -> bool {
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

pub(crate) fn certificate_matches_key(certificate_pem: &str, key_pem: &str) -> bool {
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

pub(crate) fn certificate_is_issued_by(certificate_pem: &str, issuer_pem: &str) -> bool {
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
