//! iOS development signing asset discovery.

#[cfg(target_os = "macos")]
use std::collections::BTreeMap;
#[cfg(target_os = "macos")]
use std::env;
#[cfg(any(target_os = "macos", test))]
use std::fmt::Write as _;
#[cfg(target_os = "macos")]
use std::fs;
#[cfg(any(target_os = "macos", test))]
use std::io::Cursor;
#[cfg(target_os = "macos")]
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "macos", test))]
use std::time::SystemTime;

#[cfg(any(target_os = "macos", test))]
use anyhow::Context;
use anyhow::{Result, bail};
use appd_cli::Platform;
#[cfg(any(target_os = "macos", test))]
use plist::{Dictionary, Value as PlistValue};
#[cfg(target_os = "macos")]
use serde::{Deserialize, Serialize};
#[cfg(any(target_os = "macos", test))]
use sha1::{Digest as Sha1Digest, Sha1};
#[cfg(any(target_os = "macos", test))]
use sha2::{Digest as Sha2Digest, Sha256};

/// A signing identity and provisioning profile selected for a physical iOS app.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Selection {
    pub(crate) identity: String,
    pub(crate) profile: PathBuf,
}

/// Resolve signing assets when the selected target is a physical iOS device.
pub(crate) fn resolve(
    platform: Platform,
    project: &Path,
    bundle_id: &str,
    device_id: &str,
) -> Result<Option<Selection>> {
    if platform != Platform::Ios {
        return Ok(None);
    }

    #[cfg(target_os = "macos")]
    {
        resolve_macos(project, bundle_id, device_id).map(Some)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (project, bundle_id, device_id);
        bail!("physical iOS development requires a macOS host");
    }
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug)]
struct Profile {
    path: PathBuf,
    id: String,
    name: String,
    team_id: String,
    application_identifier: String,
    expiration: SystemTime,
    expiration_label: String,
    devices: Vec<String>,
    developer_certificates: Vec<Vec<u8>>,
}

#[cfg(any(target_os = "macos", test))]
impl Profile {
    fn matches(&self, bundle_id: &str, device_id: &str, identity: &Identity) -> bool {
        self.expiration > SystemTime::now()
            && self
                .devices
                .iter()
                .any(|device| device.eq_ignore_ascii_case(device_id))
            && app_identifier_matches(&self.application_identifier, bundle_id)
            && self
                .developer_certificates
                .iter()
                .any(|certificate| certificate == &identity.certificate_der)
    }
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug)]
struct Identity {
    name: String,
    fingerprint: String,
    selector: String,
    certificate_der: Vec<u8>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug)]
struct Candidate {
    identity: Identity,
    profile: Profile,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct SelectionCache {
    entries: BTreeMap<String, CachedSelection>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug, Deserialize, Serialize)]
struct CachedSelection {
    identity_fingerprint: String,
    profile_id: String,
}

#[cfg(target_os = "macos")]
fn resolve_macos(project: &Path, bundle_id: &str, device_id: &str) -> Result<Selection> {
    if let Some(selection) = explicit_selection()? {
        return Ok(selection);
    }

    let profiles = discover_profiles();
    let identities = discover_identities()?;
    let mut candidates = identities
        .iter()
        .flat_map(|identity| {
            profiles
                .iter()
                .filter(move |profile| profile.matches(bundle_id, device_id, identity))
                .map(move |profile| Candidate {
                    identity: identity.clone(),
                    profile: profile.clone(),
                })
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|left, right| {
        left.identity
            .fingerprint
            .cmp(&right.identity.fingerprint)
            .then(left.profile.id.cmp(&right.profile.id))
            .then(left.profile.path.cmp(&right.profile.path))
    });
    candidates.dedup_by(|left, right| {
        left.identity.fingerprint == right.identity.fingerprint
            && left.profile.id == right.profile.id
    });
    candidates.sort_by(|left, right| {
        left.identity
            .name
            .cmp(&right.identity.name)
            .then(left.profile.name.cmp(&right.profile.name))
            .then(left.profile.team_id.cmp(&right.profile.team_id))
            .then(left.profile.id.cmp(&right.profile.id))
    });

    if candidates.is_empty() {
        bail!(
            "no valid iOS development signing profile was found for bundle {bundle_id} and device {device_id}; create a development certificate and register this device in Xcode, or set APPD_IOS_SIGNING_IDENTITY and APPD_IOS_PROVISIONING_PROFILE"
        );
    }

    let key = cache_key(project, bundle_id, device_id);
    let mut cache = load_cache();
    let candidate = match cache.entries.get(&key).and_then(|cached| {
        candidates.iter().find(|candidate| {
            candidate.identity.fingerprint == cached.identity_fingerprint
                && candidate.profile.id == cached.profile_id
        })
    }) {
        Some(candidate) => candidate.clone(),
        None => choose_candidate(&candidates)?,
    };

    let selection = Selection {
        identity: candidate.identity.selector,
        profile: candidate.profile.path,
    };
    cache.entries.insert(
        key,
        CachedSelection {
            identity_fingerprint: candidate.identity.fingerprint,
            profile_id: candidate.profile.id,
        },
    );
    save_cache(&cache);
    Ok(selection)
}

#[cfg(target_os = "macos")]
fn explicit_selection() -> Result<Option<Selection>> {
    let identity = env::var("APPD_IOS_SIGNING_IDENTITY").ok();
    let profile = env::var_os("APPD_IOS_PROVISIONING_PROFILE").map(PathBuf::from);
    match (identity, profile) {
        (Some(identity), Some(profile)) => {
            if identity.trim().is_empty() {
                bail!("APPD_IOS_SIGNING_IDENTITY must not be empty");
            }
            if !profile.is_file() {
                bail!(
                    "APPD_IOS_PROVISIONING_PROFILE does not point to a file: {}",
                    profile.display()
                );
            }
            let profile =
                fs::canonicalize(profile).context("resolve APPD_IOS_PROVISIONING_PROFILE")?;
            Ok(Some(Selection { identity, profile }))
        }
        (None, None) => Ok(None),
        _ => bail!(
            "APPD_IOS_SIGNING_IDENTITY and APPD_IOS_PROVISIONING_PROFILE must be provided together"
        ),
    }
}

#[cfg(target_os = "macos")]
fn discover_identities() -> Result<Vec<Identity>> {
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit, Reference, SearchResult};

    let mut options = ItemSearchOptions::new();
    options
        .class(ItemClass::identity())
        .load_refs(true)
        .limit(Limit::All);
    let results = options
        .search()
        .map_err(|error| anyhow::anyhow!("search macOS signing identities: {error}"))?;

    let mut identities = results
        .into_iter()
        .filter_map(|result| {
            let SearchResult::Ref(Reference::Identity(identity)) = result else {
                return None;
            };
            let certificate = identity.certificate().ok()?;
            identity.private_key().ok()?;
            let certificate_der = certificate.to_der();
            Some(Identity {
                name: certificate.subject_summary(),
                fingerprint: fingerprint(&certificate_der),
                selector: sha1_fingerprint(&certificate_der),
                certificate_der,
            })
        })
        .collect::<Vec<_>>();
    identities.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.fingerprint.cmp(&right.fingerprint))
    });
    identities.dedup_by(|left, right| left.fingerprint == right.fingerprint);
    Ok(identities)
}

#[cfg(target_os = "macos")]
fn discover_profiles() -> Vec<Profile> {
    let mut paths = profile_paths();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let bytes = fs::read(&path).ok()?;
            parse_profile(&path, &bytes).ok()
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn profile_paths() -> Vec<PathBuf> {
    let Some(home) = env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    [
        home.join("Library/Developer/Xcode/UserData/Provisioning Profiles"),
        home.join("Library/MobileDevice/Provisioning Profiles"),
    ]
    .into_iter()
    .flat_map(|directory| {
        let Ok(entries) = fs::read_dir(directory) else {
            return Vec::new();
        };
        entries
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "mobileprovision")
            })
            .collect::<Vec<_>>()
    })
    .collect()
}

#[cfg(any(target_os = "macos", test))]
fn parse_profile(path: &Path, bytes: &[u8]) -> Result<Profile> {
    let value = decode_profile(bytes)?;
    let root = value
        .as_dictionary()
        .context("provisioning profile root is not a dictionary")?;
    let entitlements = root
        .get("Entitlements")
        .and_then(PlistValue::as_dictionary)
        .context("provisioning profile entitlements are missing")?;
    let application_identifier = string_value(entitlements, "application-identifier")
        .context("provisioning profile application identifier is missing")?;
    let team_id = string_array(root, "TeamIdentifier")
        .into_iter()
        .next()
        .or_else(|| application_identifier.split('.').next().map(str::to_owned))
        .context("provisioning profile team identifier is missing")?;
    let expiration_date = root
        .get("ExpirationDate")
        .and_then(PlistValue::as_date)
        .context("provisioning profile expiration date is missing")?;
    let expiration = SystemTime::from(expiration_date);
    let expiration_label = expiration_date.to_xml_format();
    let devices = string_array(root, "ProvisionedDevices");
    let developer_certificates = data_array(root, "DeveloperCertificates");
    if devices.is_empty() {
        bail!("provisioning profile does not contain registered devices");
    }
    if developer_certificates.is_empty() {
        bail!("provisioning profile does not contain developer certificates");
    }
    let platforms = string_array(root, "Platform");
    if !platforms.is_empty()
        && !platforms
            .iter()
            .any(|platform| matches!(platform.as_str(), "iOS" | "iPhoneOS"))
    {
        bail!("provisioning profile is not for iOS");
    }

    let id = string_value(root, "UUID")
        .or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| fingerprint(bytes));
    let name = string_value(root, "Name").unwrap_or_else(|| id.clone());
    Ok(Profile {
        path: path.to_path_buf(),
        id,
        name,
        team_id,
        application_identifier,
        expiration,
        expiration_label,
        devices,
        developer_certificates,
    })
}

#[cfg(any(target_os = "macos", test))]
fn decode_profile(bytes: &[u8]) -> Result<PlistValue> {
    if let Ok(value) = PlistValue::from_reader(Cursor::new(bytes)) {
        return Ok(value);
    }

    #[cfg(target_os = "macos")]
    {
        use security_framework::cms::CMSDecoder;

        let decoder = CMSDecoder::create()
            .map_err(|error| anyhow::anyhow!("create provisioning-profile decoder: {error}"))?;
        decoder
            .update_message(bytes)
            .map_err(|error| anyhow::anyhow!("decode provisioning profile: {error}"))?;
        decoder
            .finalize_message()
            .map_err(|error| anyhow::anyhow!("finalize provisioning profile: {error}"))?;
        let content = decoder
            .get_content()
            .map_err(|error| anyhow::anyhow!("read provisioning-profile contents: {error}"))?;
        PlistValue::from_reader(Cursor::new(content))
            .context("provisioning profile contents are not a plist")
    }

    #[cfg(not(target_os = "macos"))]
    {
        bail!("CMS provisioning profiles can only be decoded on macOS");
    }
}

#[cfg(any(target_os = "macos", test))]
fn string_value(values: &Dictionary, key: &str) -> Option<String> {
    values.get(key)?.as_string().map(str::to_owned)
}

#[cfg(any(target_os = "macos", test))]
fn string_array(values: &Dictionary, key: &str) -> Vec<String> {
    values
        .get(key)
        .and_then(PlistValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(PlistValue::as_string)
        .map(str::to_owned)
        .collect()
}

#[cfg(any(target_os = "macos", test))]
fn data_array(values: &Dictionary, key: &str) -> Vec<Vec<u8>> {
    values
        .get(key)
        .and_then(PlistValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(PlistValue::as_data)
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(any(target_os = "macos", test))]
fn app_identifier_matches(application_identifier: &str, bundle_id: &str) -> bool {
    let Some((_, pattern)) = application_identifier.split_once('.') else {
        return false;
    };
    pattern == bundle_id
        || pattern == "*"
        || pattern.strip_suffix(".*").is_some_and(|prefix| {
            bundle_id.starts_with(prefix) && bundle_id.as_bytes().get(prefix.len()) == Some(&b'.')
        })
}

#[cfg(any(target_os = "macos", test))]
fn fingerprint(bytes: &[u8]) -> String {
    hex_digest(<Sha256 as Sha2Digest>::digest(bytes))
}

#[cfg(any(target_os = "macos", test))]
fn sha1_fingerprint(bytes: &[u8]) -> String {
    hex_digest(<Sha1 as Sha1Digest>::digest(bytes))
}

#[cfg(any(target_os = "macos", test))]
fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(target_os = "macos")]
fn cache_key(project: &Path, bundle_id: &str, device_id: &str) -> String {
    format!("{}\n{bundle_id}\n{device_id}", project.display())
}

#[cfg(target_os = "macos")]
fn cache_path() -> Option<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    }?;
    Some(base.join("appd/ios-signing.json"))
}

#[cfg(target_os = "macos")]
fn load_cache() -> SelectionCache {
    let Some(path) = cache_path() else {
        return SelectionCache::default();
    };
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn save_cache(cache: &SelectionCache) {
    let Some(path) = cache_path() else {
        return;
    };
    let Some(parent) = path.parent().map(Path::to_path_buf) else {
        return;
    };
    if let Err(error) = (|| -> Result<()> {
        fs::create_dir_all(&parent)?;
        fs::write(path, serde_json::to_vec_pretty(cache)?)?;
        Ok(())
    })() {
        eprintln!("warning: could not save iOS signing selection: {error}");
    }
}

#[cfg(target_os = "macos")]
fn choose_candidate(candidates: &[Candidate]) -> Result<Candidate> {
    if candidates.len() == 1 {
        return Ok(candidates[0].clone());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!(
            "multiple valid iOS signing profiles match this app and device; set APPD_IOS_SIGNING_IDENTITY and APPD_IOS_PROVISIONING_PROFILE for non-interactive use"
        );
    }

    println!("Multiple valid iOS signing profiles match this app and device:");
    for (index, candidate) in candidates.iter().enumerate() {
        println!(
            "  {}) {} (SHA-1 {}) — {} [{}] (team {}, expires {})",
            index + 1,
            candidate.identity.name,
            candidate.identity.selector,
            candidate.profile.name,
            candidate.profile.id,
            candidate.profile.team_id,
            candidate.profile.expiration_label
        );
    }
    loop {
        print!("Select a profile [1-{}]: ", candidates.len());
        io::stdout().flush()?;
        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            bail!("no iOS signing profile was selected");
        }
        if let Ok(number) = input.trim().parse::<usize>()
            && (1..=candidates.len()).contains(&number)
        {
            return Ok(candidates[number - 1].clone());
        }
        eprintln!("Enter a number from 1 to {}.", candidates.len());
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use plist::{Dictionary, Value};

    use super::{Identity, app_identifier_matches, fingerprint, parse_profile, sha1_fingerprint};

    fn profile_value(application_identifier: &str, device: &str) -> Value {
        let mut entitlements = Dictionary::new();
        entitlements.insert(
            "application-identifier".to_owned(),
            Value::String(application_identifier.to_owned()),
        );
        let mut root = Dictionary::new();
        root.insert("UUID".to_owned(), Value::String("PROFILE-1".to_owned()));
        root.insert("Name".to_owned(), Value::String("Development".to_owned()));
        root.insert(
            "TeamIdentifier".to_owned(),
            Value::Array(vec![Value::String("TEAM".to_owned())]),
        );
        root.insert(
            "ExpirationDate".to_owned(),
            Value::Date((SystemTime::now() + Duration::from_hours(1)).into()),
        );
        root.insert(
            "ProvisionedDevices".to_owned(),
            Value::Array(vec![Value::String(device.to_owned())]),
        );
        root.insert(
            "DeveloperCertificates".to_owned(),
            Value::Array(vec![Value::Data(vec![1, 2, 3])]),
        );
        root.insert(
            "Platform".to_owned(),
            Value::Array(vec![Value::String("iOS".to_owned())]),
        );
        root.insert("Entitlements".to_owned(), Value::Dictionary(entitlements));
        Value::Dictionary(root)
    }

    #[test]
    fn parses_and_matches_a_development_profile() -> Result<(), Box<dyn std::error::Error>> {
        let value = profile_value("TEAM.com.example.app", "DEVICE");
        let mut bytes = Vec::new();
        value.to_writer_xml(&mut bytes)?;
        let profile = parse_profile(Path::new("PROFILE-1.mobileprovision"), &bytes)?;
        let identity = Identity {
            name: "Apple Development: Test".to_owned(),
            fingerprint: fingerprint(&[1, 2, 3]),
            selector: sha1_fingerprint(&[1, 2, 3]),
            certificate_der: vec![1, 2, 3],
        };

        assert_eq!(profile.path, Path::new("PROFILE-1.mobileprovision"));
        assert_eq!(profile.id, "PROFILE-1");
        assert_eq!(profile.name, "Development");
        assert_eq!(profile.team_id, "TEAM");
        assert!(!profile.expiration_label.is_empty());
        assert_eq!(identity.name, "Apple Development: Test");
        assert_eq!(identity.fingerprint, fingerprint(&[1, 2, 3]));
        assert_eq!(identity.selector, sha1_fingerprint(&[1, 2, 3]));
        assert!(profile.matches("com.example.app", "DEVICE", &identity));
        assert!(!profile.matches("com.example.other", "DEVICE", &identity));
        assert!(!profile.matches("com.example.app", "OTHER", &identity));
        Ok(())
    }

    #[test]
    fn accepts_wildcard_application_identifiers() {
        assert!(app_identifier_matches(
            "TEAM.com.example.*",
            "com.example.app"
        ));
        assert!(!app_identifier_matches(
            "TEAM.com.example.*",
            "com.other.app"
        ));
        assert!(app_identifier_matches("TEAM.*", "com.other.app"));
    }

    #[test]
    fn rejects_profiles_for_other_platforms() -> Result<(), Box<dyn std::error::Error>> {
        let mut value = profile_value("TEAM.com.example.app", "DEVICE");
        let Value::Dictionary(root) = &mut value else {
            unreachable!("profile fixture is a dictionary");
        };
        root.insert(
            "Platform".to_owned(),
            Value::Array(vec![Value::String("macOS".to_owned())]),
        );
        let mut bytes = Vec::new();
        value.to_writer_xml(&mut bytes)?;

        assert!(parse_profile(Path::new("PROFILE-1.mobileprovision"), &bytes).is_err());
        Ok(())
    }

    #[test]
    fn fingerprints_are_stable() {
        assert_eq!(
            fingerprint(b"appd"),
            "202f02b0b11359d092bdff94a65202a11070abf00c75113b0e99f8a1ca387ceb"
        );
        assert_eq!(
            sha1_fingerprint(b"appd"),
            "d178599ca04434b54f4ff2b74ac5a666cfeddbf5"
        );
    }
}
