//! Dart AOT snapshot layout profiles.
//!
//! Profiles are data-only artifacts. They are loaded by the host at runtime,
//! bounded and SHA-256 verified through the compatibility registry. Snapshot
//! hashes and SDK aliases live in registry records, never in this artifact.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;

pub const MAX_PROFILE_BYTES: u64 = 4 * 1024 * 1024;

/// How a Dart object header encodes its class id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TagStyle {
    #[serde(rename = "CID_INT32")]
    CidInt32,
    #[serde(rename = "CID_SHIFT1")]
    CidShift1,
    #[serde(rename = "OBJECT_HEADER")]
    ObjectHeader,
}

impl TagStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            TagStyle::CidInt32 => "CID_INT32",
            TagStyle::CidShift1 => "CID_SHIFT1",
            TagStyle::ObjectHeader => "OBJECT_HEADER",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DartProfile {
    pub tag_style: TagStyle,
    pub compressed_word_size: u32,
    pub header_fields: u32,
    pub max_alignment: u32,
    pub heap_object_tag: u32,
    pub cids: HashMap<String, u32>,
}

/// A semantic SDK release label attached to a registry record as provenance.
/// It is never used to select a parser or profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SdkAlias {
    pub ecosystem: String,
    pub version: String,
    pub provenance: String,
}

/// A profile plus its content digest and zero-or-more provenance aliases.
#[derive(Debug, Clone)]
pub struct ResolvedDartProfile {
    /// Legacy display value. It is `unverified` when aliases are ambiguous and
    /// must never be used for compatibility selection.
    pub dart_version: String,
    /// Profile artifact id, not a semantic SDK selector.
    pub profile_version: String,
    pub profile_sha256: String,
    pub aliases: Vec<SdkAlias>,
    pub profile: DartProfile,
}

#[derive(Debug, Deserialize)]
struct ProfileTable {
    #[serde(default)]
    profiles: HashMap<String, DartProfile>,
}

fn alias_display(aliases: &[SdkAlias]) -> String {
    if aliases.is_empty() {
        "unavailable".to_string()
    } else {
        // Even a singular alias is provenance, not an exact SDK claim.
        "unverified".to_string()
    }
}

fn valid_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn parse_profile(bytes: &[u8], profile_id: &str) -> Result<DartProfile, String> {
    let table = serde_json::from_slice::<ProfileTable>(bytes)
        .map_err(|err| format!("parse profile JSON: {err}"))?;
    if !table.profiles.is_empty() {
        return table
            .profiles
            .get(profile_id)
            .cloned()
            .ok_or_else(|| format!("profile id {:?} is absent", profile_id));
    }
    serde_json::from_slice::<DartProfile>(bytes)
        .map_err(|err| format!("profile id {:?} is absent or malformed: {err}", profile_id))
}

/// Load one bounded profile artifact and verify its content address.
pub fn load_profile_artifact(
    path: &Path,
    profile_id: &str,
    expected_sha256: &str,
    aliases: Vec<SdkAlias>,
) -> Result<ResolvedDartProfile, String> {
    if profile_id.trim().is_empty() {
        return Err("profile id is empty".to_string());
    }
    if !valid_digest(expected_sha256) {
        return Err("profile digest is not lowercase SHA-256".to_string());
    }
    let metadata = fs::metadata(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if metadata.len() > MAX_PROFILE_BYTES {
        return Err(format!(
            "{} exceeds the {} byte profile limit",
            path.display(),
            MAX_PROFILE_BYTES
        ));
    }
    let mut file = fs::File::open(path).map_err(|err| format!("open {}: {err}", path.display()))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_PROFILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("read {}: {err}", path.display()))?;
    if bytes.len() as u64 > MAX_PROFILE_BYTES {
        return Err(format!(
            "{} exceeds the {} byte profile limit",
            path.display(),
            MAX_PROFILE_BYTES
        ));
    }
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual_sha256 = format!("{:x}", hasher.finalize());
    if actual_sha256 != expected_sha256 {
        return Err(format!(
            "profile SHA-256 mismatch: expected {}, got {}",
            expected_sha256, actual_sha256
        ));
    }
    let profile = parse_profile(&bytes, profile_id)?;
    Ok(ResolvedDartProfile {
        dart_version: alias_display(&aliases),
        profile_version: profile_id.to_string(),
        profile_sha256: expected_sha256.to_string(),
        aliases,
        profile,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    fn profile_json(tag_style: &str, cid: u32) -> String {
        format!(
            r#"{{
                "_comment": "fixture",
                "profiles": {{
                    "profile-a": {{
                        "tag_style": "{tag_style}",
                        "compressed_word_size": 4,
                        "header_fields": 5,
                        "max_alignment": 16,
                        "heap_object_tag": 1,
                        "cids": {{
                            "class": {cid},
                            "object_pool": 23
                        }}
                    }}
                }}
            }}"#
        )
    }

    fn write_profile(contents: &str) -> (tempfile::TempDir, std::path::PathBuf, String) {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("profile.json");
        std::fs::write(&path, contents).expect("write profile");
        let mut hasher = Sha256::new();
        hasher.update(contents.as_bytes());
        (dir, path, format!("{:x}", hasher.finalize()))
    }

    #[test]
    fn runtime_profile_loads_with_alias_provenance() {
        let contents = profile_json("OBJECT_HEADER", 42);
        let (_dir, path, digest) = write_profile(&contents);
        let aliases = vec![
            SdkAlias {
                ecosystem: "dart".to_string(),
                version: "3.5.0".to_string(),
                provenance: "fixture".to_string(),
            },
            SdkAlias {
                ecosystem: "flutter".to_string(),
                version: "3.24.0".to_string(),
                provenance: "fixture".to_string(),
            },
        ];
        let profile =
            load_profile_artifact(&path, "profile-a", &digest, aliases.clone()).expect("profile");
        assert_eq!(profile.profile_version, "profile-a");
        assert_eq!(profile.profile_sha256, digest);
        assert_eq!(profile.aliases, aliases);
        assert_eq!(profile.dart_version, "unverified");
        assert_eq!(profile.profile.cids["class"], 42);
    }

    #[test]
    fn replacing_profile_is_observed_and_wrong_digest_is_rejected() {
        let first = profile_json("OBJECT_HEADER", 42);
        let (_dir, path, first_digest) = write_profile(&first);
        let first_profile =
            load_profile_artifact(&path, "profile-a", &first_digest, Vec::new()).expect("first");
        assert_eq!(first_profile.profile.cids["class"], 42);

        let second = profile_json("CID_SHIFT1", 99);
        std::fs::write(&path, &second).expect("replace profile");
        let mut second_hasher = Sha256::new();
        second_hasher.update(second.as_bytes());
        let second_digest = format!("{:x}", second_hasher.finalize());
        let second_profile =
            load_profile_artifact(&path, "profile-a", &second_digest, Vec::new()).expect("second");
        assert_eq!(second_profile.profile.tag_style, TagStyle::CidShift1);
        assert_eq!(second_profile.profile.cids["class"], 99);
        assert!(load_profile_artifact(&path, "profile-a", &first_digest, Vec::new()).is_err());
    }

    #[test]
    fn malformed_and_oversized_profiles_fail_closed() {
        let (_dir, path, digest) = write_profile("{}");
        assert!(load_profile_artifact(&path, "profile-a", &digest, Vec::new()).is_err());

        let dir = tempdir().expect("tempdir");
        let oversized = dir.path().join("oversized.json");
        let contents = vec![b'x'; (MAX_PROFILE_BYTES + 1) as usize];
        std::fs::write(&oversized, &contents).expect("write oversized profile");
        assert!(load_profile_artifact(&oversized, "profile-a", &digest, Vec::new()).is_err());
    }
}
