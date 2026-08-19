//! Dart AOT snapshot layout profiles, resolved from the 32-byte snapshot hash.
//!
//! The snapshot hash is an MD5 over Dart VM serializer sources, so it pins the exact
//! snapshot layout a binary was built with. The mapping from hash to Dart version is
//! not derivable. It has to be tabulated by building each SDK, so the table is
//! vendored as data from `radareorg/r2flutter` (MIT); see `data/dart-profiles.json`.
//!
//! This module deliberately stops at *identification*. It reports which Dart version
//! and tag encoding a snapshot uses so `info`/`report.json` can say something true
//! instead of `unknown`, and so adapter output can be sanity-checked. It does not
//! deserialize snapshots.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;

/// How a Dart object header encodes its class id. Changes across SDK releases, and a
/// decoder that assumes the wrong one silently reads garbage class ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TagStyle {
    /// Dart 2.10 - 2.13: raw `int32` class id.
    #[serde(rename = "CID_INT32")]
    CidInt32,
    /// Dart 2.14 - 3.3: `(cid << 1) | canonical`.
    #[serde(rename = "CID_SHIFT1")]
    CidShift1,
    /// Dart 3.4.3+ (and the 2.18.2 outlier): packed `ObjectHeader` bitfield.
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
pub struct DartProfile {
    pub tag_style: TagStyle,
    pub compressed_word_size: u32,
    pub header_fields: u32,
    pub max_alignment: u32,
    pub heap_object_tag: u32,
    /// Class ids for the handful of classes worth naming; they move between releases.
    pub cids: HashMap<String, u32>,
}

/// A profile plus the versions it was resolved through.
#[derive(Debug, Clone)]
pub struct ResolvedDartProfile {
    /// Exact Dart version the snapshot hash was published under, e.g. `3.9.2`.
    pub dart_version: String,
    /// Profile bucket the layout was taken from, e.g. `3.9.0`. Layouts only change on
    /// some releases, so buckets are sparse and resolved by floor.
    pub profile_version: String,
    pub profile: DartProfile,
}

#[derive(Debug, Deserialize)]
struct ProfileTable {
    hashes: HashMap<String, String>,
    profiles: HashMap<String, DartProfile>,
}

static TABLE: LazyLock<ProfileTable> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../data/dart-profiles.json"))
        .expect("vendored data/dart-profiles.json is malformed")
});

/// Parse a dotted numeric version into comparable components.
fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let mut parts = v.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Greatest profile bucket that is `<= version`.
///
/// Snapshot layouts only change on some SDK releases, so the table stores one entry
/// per change and every version in between inherits it. An exact-match lookup would
/// reject most real snapshots.
fn floor_profile(version: &str) -> Option<(&'static str, &'static DartProfile)> {
    let want = parse_version(version)?;
    TABLE
        .profiles
        .iter()
        .filter_map(|(k, p)| parse_version(k).map(|parsed| (parsed, k.as_str(), p)))
        .filter(|(parsed, _, _)| *parsed <= want)
        .max_by_key(|(parsed, _, _)| *parsed)
        .map(|(_, k, p)| (k, p))
}

/// Resolve a snapshot hash to its Dart version and layout profile.
///
/// Returns `None` for hashes not in the table. Unknown is reported as unknown: a
/// guessed profile would be indistinguishable from a real one downstream.
pub fn profile_for_hash(snapshot_hash: &str) -> Option<ResolvedDartProfile> {
    let key = snapshot_hash.trim().to_ascii_lowercase();
    let version = TABLE.hashes.get(&key)?;
    let (profile_version, profile) = floor_profile(version)?;
    Some(ResolvedDartProfile {
        dart_version: version.clone(),
        profile_version: profile_version.to_string(),
        profile: profile.clone(),
    })
}

/// Number of snapshot hashes the table can name.
pub fn known_hash_count() -> usize {
    TABLE.hashes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_a_known_modern_snapshot_hash() {
        let r = profile_for_hash("97ff04a728735e6b6b098bdf983faaba")
            .expect("hash present in vendored table");
        assert_eq!(r.dart_version, "3.9.2");
        assert_eq!(
            r.profile_version, "3.9.0",
            "3.9.2 inherits the 3.9.0 layout bucket"
        );
        assert_eq!(r.profile.tag_style, TagStyle::ObjectHeader);
        assert_eq!(r.profile.compressed_word_size, 4);
        assert_eq!(r.profile.cids.get("object_pool"), Some(&23));
    }

    #[test]
    fn resolves_a_known_legacy_snapshot_hash() {
        // Dart 2.10 predates both the shift-1 and ObjectHeader tag encodings.
        let r = profile_for_hash("8ee4ef7a67df9845fba331734198a953")
            .expect("hash present in vendored table");
        assert_eq!(r.dart_version, "2.10.0");
        assert_eq!(r.profile.tag_style, TagStyle::CidInt32);
        assert_eq!(r.profile.compressed_word_size, 8);
    }

    #[test]
    fn class_ids_move_between_releases() {
        // The reason a single hardcoded CID table cannot serve every snapshot.
        let old = profile_for_hash("8ee4ef7a67df9845fba331734198a953").unwrap();
        let new = profile_for_hash("97ff04a728735e6b6b098bdf983faaba").unwrap();
        assert_ne!(old.profile.cids["class"], new.profile.cids["class"]);
        assert_ne!(
            old.profile.cids["object_pool"],
            new.profile.cids["object_pool"]
        );
    }

    #[test]
    fn unknown_hash_stays_unknown() {
        assert!(profile_for_hash("ffffffffffffffffffffffffffffffff").is_none());
        assert!(profile_for_hash("unknown").is_none());
    }

    #[test]
    fn floor_resolution_picks_the_greatest_bucket_at_or_below() {
        // 3.7.x ships no layout change of its own and must inherit 3.6.0, not 3.9.0.
        let (bucket, _) = floor_profile("3.7.4").expect("3.7.4 resolves");
        assert_eq!(bucket, "3.6.0");
        let (bucket, _) = floor_profile("2.18.1").expect("2.18.1 resolves");
        assert_eq!(bucket, "2.18.0");
    }

    #[test]
    fn versions_below_the_oldest_bucket_do_not_resolve() {
        assert!(floor_profile("2.9.0").is_none());
        assert!(floor_profile("not-a-version").is_none());
    }

    #[test]
    fn every_tabulated_hash_resolves_to_a_profile() {
        assert!(known_hash_count() >= 61);
        for (hash, version) in &TABLE.hashes {
            assert!(
                floor_profile(version).is_some(),
                "hash {hash} maps to version {version}, which resolves to no profile"
            );
        }
    }
}
