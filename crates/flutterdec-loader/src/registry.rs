//! Exact compatibility records for host-side adapter selection.
//!
//! A registry record is the only authority that connects a header-derived
//! snapshot identity to parser/profile/artifact data. Semantic SDK aliases are
//! evidence attached to a record; they never participate in selection.

use crate::dart_profile::{self, ResolvedDartProfile, SdkAlias};
use crate::identity::{
    ExactSelectionKey, IdentityRejection, SnapshotIdentity, SnapshotKind, TargetArch,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub const REGISTRY_VERSION: u32 = 1;
pub const MAX_REGISTRY_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_ARTIFACT_BYTES: u64 = 128 * 1024 * 1024;

/// A parser implementation, independent from the snapshot identities it serves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParserFamilyReference {
    pub id: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
}

/// A content-addressed profile artifact. Multiple records may share one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileReference {
    pub id: String,
    pub path: String,
    pub sha256: String,
}

/// One host-specific executable variant. Multiple records may share one artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostArtifactVariant {
    pub host_os: String,
    pub host_arch: String,
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub provenance: String,
}

/// An artifact identity plus the host variants that can execute it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactReference {
    pub id: String,
    #[serde(default)]
    pub variants: Vec<HostArtifactVariant>,
}

/// Evidence supporting a registry mapping. This is descriptive and never a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityEvidence {
    pub source: String,
    pub provenance: String,
    #[serde(default)]
    pub references: Vec<String>,
}

/// Trust assigned to a checked-in compatibility record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustTier {
    Verified,
    Experimental,
}

/// One exact mapping from a header identity to parser/profile/artifact data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityRecord {
    pub snapshot_hash: String,
    pub snapshot_kind: SnapshotKind,
    pub target_arch: TargetArch,
    /// The normalized layout-affecting feature tuple expected in the header.
    pub features: Vec<String>,
    /// SHA-256 over the canonical sorted/deduplicated `features` tuple.
    pub feature_fingerprint: String,
    /// Recognized tokens that are not expected for this exact record. These
    /// fields make unknown and forbidden input distinguishable to operators.
    #[serde(default)]
    pub known_features: Vec<String>,
    #[serde(default)]
    pub forbidden_features: Vec<String>,
    /// Zero or more semantic SDK aliases. They are provenance only.
    #[serde(default, alias = "aliases")]
    pub sdk_aliases: Vec<SdkAlias>,
    pub parser_family: ParserFamilyReference,
    pub profile: ProfileReference,
    pub artifact: ArtifactReference,
    pub evidence: CompatibilityEvidence,
    pub trust_tier: TrustTier,
    pub protocol_major: u32,
    pub model_major: u32,
}

/// The checked-in registry document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityRegistry {
    pub version: u32,
    pub records: Vec<CompatibilityRecord>,
}

/// A record selected by exact hash, target, and feature fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrySelection {
    key: ExactSelectionKey,
    record: CompatibilityRecord,
}

/// Verified executable selected for this host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedArtifact {
    pub path: PathBuf,
    pub variant: HostArtifactVariant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    Malformed(String),
    UnsupportedVersion(u32),
    Identity(IdentityRejection),
    NoRecord(String),
    TargetMismatch {
        requested: String,
    },
    FeatureMismatch {
        missing: Vec<String>,
        forbidden: Vec<String>,
        unknown: Vec<String>,
    },
    Ambiguous(String),
    InvalidRecord(String),
    Profile(String),
    /// The record names an artifact this host has not installed.
    ///
    /// Kept apart from [`Self::Artifact`] because the two ask the operator for
    /// different things: this one is "run `adapter install`", and that one is
    /// "the bytes in your store are not the bytes the registry authorized".
    /// Only this one is a condition a host may answer by recovering the program
    /// itself.
    ArtifactAbsent(String),
    Artifact(String),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(detail) => write!(f, "malformed compatibility registry: {detail}"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported compatibility registry version {version}")
            }
            Self::Identity(rejection) => write!(f, "snapshot identity rejected before registry selection: {rejection}"),
            Self::NoRecord(hash) => write!(f, "no exact compatibility record for snapshot hash {hash}"),
            Self::TargetMismatch { requested } => {
                write!(f, "no compatibility record for target architecture {requested}")
            }
            Self::FeatureMismatch { missing, forbidden, unknown } => write!(
                f,
                "feature tuple has no exact compatibility record (missing={missing:?}, forbidden={forbidden:?}, unknown={unknown:?})"
            ),
            Self::Ambiguous(detail) => write!(f, "ambiguous compatibility registry selection: {detail}"),
            Self::InvalidRecord(detail) => write!(f, "invalid compatibility registry record: {detail}"),
            Self::Profile(detail) => write!(f, "profile artifact rejected: {detail}"),
            Self::ArtifactAbsent(detail) => write!(f, "adapter artifact unavailable: {detail}"),
            Self::Artifact(detail) => write!(f, "adapter artifact rejected: {detail}"),
        }
    }
}

impl std::error::Error for RegistryError {}

/// Canonicalize and fingerprint a layout feature tuple.
pub fn canonical_feature_fingerprint(features: &[String]) -> String {
    let mut normalized = features
        .iter()
        .map(|feature| feature.to_ascii_lowercase())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    let mut hasher = Sha256::new();
    hasher.update(normalized.join("\n").as_bytes());
    format!("{:x}", hasher.finalize())
}

fn valid_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_relative_path(text: &str) -> Result<(), RegistryError> {
    if text.is_empty() || text.contains('\\') || text.contains('\0') {
        return Err(RegistryError::InvalidRecord(format!(
            "path {:?} is not a contained relative path",
            text
        )));
    }
    let path = Path::new(text);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(RegistryError::InvalidRecord(format!(
            "path {:?} is not a contained relative path",
            text
        )));
    }
    Ok(())
}

fn validate_nonempty(field: &str, value: &str) -> Result<(), RegistryError> {
    if value.trim().is_empty() {
        return Err(RegistryError::InvalidRecord(format!("{field} is empty")));
    }
    Ok(())
}

impl CompatibilityRecord {
    pub fn validate(&self) -> Result<(), RegistryError> {
        if self.snapshot_hash.len() != 32
            || !self
                .snapshot_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(RegistryError::InvalidRecord(format!(
                "snapshot hash {:?} is not lowercase 32-character hex",
                self.snapshot_hash
            )));
        }
        if self.snapshot_kind != SnapshotKind::FullAot {
            return Err(RegistryError::InvalidRecord(
                "only full_aot records can authorize adapter selection".to_string(),
            ));
        }
        validate_nonempty("target architecture", self.target_arch.as_str())?;
        let mut features = self.features.clone();
        for feature in &features {
            if feature.is_empty() || feature != &feature.to_ascii_lowercase() {
                return Err(RegistryError::InvalidRecord(format!(
                    "feature {:?} is not normalized",
                    feature
                )));
            }
        }
        features.sort();
        features.dedup();
        if features != self.features {
            return Err(RegistryError::InvalidRecord(
                "features must be sorted and deduplicated".to_string(),
            ));
        }
        if self.feature_fingerprint != canonical_feature_fingerprint(&self.features)
            || !valid_digest(&self.feature_fingerprint)
        {
            return Err(RegistryError::InvalidRecord(
                "feature fingerprint does not match canonical features".to_string(),
            ));
        }
        for feature in &self.known_features {
            validate_nonempty("known feature", feature)?;
        }
        for feature in &self.forbidden_features {
            validate_nonempty("forbidden feature", feature)?;
            if self.features.iter().any(|expected| expected == feature) {
                return Err(RegistryError::InvalidRecord(format!(
                    "feature {:?} is both expected and forbidden",
                    feature
                )));
            }
        }
        validate_nonempty("parser family id", &self.parser_family.id)?;
        if let Some(digest) = &self.parser_family.sha256 {
            if !valid_digest(digest) {
                return Err(RegistryError::InvalidRecord(
                    "parser family digest is not lowercase SHA-256".to_string(),
                ));
            }
        }
        validate_nonempty("profile id", &self.profile.id)?;
        validate_relative_path(&self.profile.path)?;
        if !valid_digest(&self.profile.sha256) {
            return Err(RegistryError::InvalidRecord(
                "profile digest is not lowercase SHA-256".to_string(),
            ));
        }
        validate_nonempty("artifact id", &self.artifact.id)?;
        validate_nonempty("evidence source", &self.evidence.source)?;
        validate_nonempty("evidence provenance", &self.evidence.provenance)?;
        let mut hosts = HashSet::new();
        for variant in &self.artifact.variants {
            validate_nonempty("artifact host OS", &variant.host_os)?;
            validate_nonempty("artifact host architecture", &variant.host_arch)?;
            validate_relative_path(&variant.path)?;
            if variant.size == 0 {
                return Err(RegistryError::InvalidRecord(
                    "artifact size must be nonzero".to_string(),
                ));
            }
            if !valid_digest(&variant.sha256) {
                return Err(RegistryError::InvalidRecord(
                    "artifact digest is not lowercase SHA-256".to_string(),
                ));
            }
            validate_nonempty("artifact provenance", &variant.provenance)?;
            if !hosts.insert((variant.host_os.clone(), variant.host_arch.clone())) {
                return Err(RegistryError::InvalidRecord(format!(
                    "duplicate artifact host variant {}/{}",
                    variant.host_os, variant.host_arch
                )));
            }
        }
        let mut aliases = HashSet::new();
        for alias in &self.sdk_aliases {
            validate_nonempty("SDK alias ecosystem", &alias.ecosystem)?;
            validate_nonempty("SDK alias version", &alias.version)?;
            validate_nonempty("SDK alias provenance", &alias.provenance)?;
            if !aliases.insert((
                alias.ecosystem.clone(),
                alias.version.clone(),
                alias.provenance.clone(),
            )) {
                return Err(RegistryError::InvalidRecord(
                    "duplicate SDK alias".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Digest of the complete record in deterministic struct-field order.
    pub fn sha256(&self) -> Result<String, RegistryError> {
        let bytes = serde_json::to_vec(self)
            .map_err(|err| RegistryError::Malformed(format!("serialize record: {err}")))?;
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        Ok(format!("{:x}", hasher.finalize()))
    }
}

impl CompatibilityRegistry {
    pub fn from_json(bytes: &[u8]) -> Result<Self, RegistryError> {
        let registry = serde_json::from_slice::<Self>(bytes)
            .map_err(|err| RegistryError::Malformed(err.to_string()))?;
        registry.validate()?;
        Ok(registry)
    }

    pub fn load(path: &Path) -> Result<Self, RegistryError> {
        let metadata = fs::metadata(path)
            .map_err(|err| RegistryError::Malformed(format!("read {}: {err}", path.display())))?;
        if !metadata.is_file() {
            return Err(RegistryError::Malformed(format!(
                "{} is not a regular file",
                path.display()
            )));
        }
        if metadata.len() > MAX_REGISTRY_BYTES {
            return Err(RegistryError::Malformed(format!(
                "{} exceeds the {} byte registry limit",
                path.display(),
                MAX_REGISTRY_BYTES
            )));
        }
        let file = fs::File::open(path)
            .map_err(|err| RegistryError::Malformed(format!("open {}: {err}", path.display())))?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(MAX_REGISTRY_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|err| RegistryError::Malformed(format!("read {}: {err}", path.display())))?;
        if bytes.len() as u64 > MAX_REGISTRY_BYTES {
            return Err(RegistryError::Malformed(format!(
                "{} exceeds the {} byte registry limit",
                path.display(),
                MAX_REGISTRY_BYTES
            )));
        }
        Self::from_json(&bytes)
    }

    pub fn load_from_root(root: &Path) -> Result<Self, RegistryError> {
        Self::load(&root.join("adapters/registry.json"))
    }

    pub fn validate(&self) -> Result<(), RegistryError> {
        if self.version != REGISTRY_VERSION {
            return Err(RegistryError::UnsupportedVersion(self.version));
        }
        let mut keys = HashSet::new();
        for record in &self.records {
            record.validate()?;
            let key = format!(
                "{}|{}|{}",
                record.snapshot_hash,
                record.target_arch.as_str(),
                record.feature_fingerprint
            );
            // Two records under one exact key is the same condition
            // `select_key` reports when it finds more than one match, and it is
            // named the same way here: a registry that cannot say which record
            // covers a snapshot is ambiguous, not merely invalid. Nothing may
            // treat it as "this snapshot is unsupported" and carry on.
            if !keys.insert(key.clone()) {
                return Err(RegistryError::Ambiguous(format!(
                    "two records share the exact compatibility key {key}"
                )));
            }
        }
        Ok(())
    }

    pub fn select(&self, identity: &SnapshotIdentity) -> Result<RegistrySelection, RegistryError> {
        let key = identity
            .exact_selection_key()
            .map_err(RegistryError::Identity)?;
        self.select_key(&key)
    }

    pub fn select_key(&self, key: &ExactSelectionKey) -> Result<RegistrySelection, RegistryError> {
        let same_hash = self
            .records
            .iter()
            .filter(|record| record.snapshot_hash == key.hash)
            .collect::<Vec<_>>();
        if same_hash.is_empty() {
            return Err(RegistryError::NoRecord(key.hash.clone()));
        }
        let same_target = same_hash
            .iter()
            .copied()
            .filter(|record| record.target_arch == key.target_arch)
            .collect::<Vec<_>>();
        if same_target.is_empty() {
            return Err(RegistryError::TargetMismatch {
                requested: key.target_arch.as_str().to_string(),
            });
        }

        let fingerprint = canonical_feature_fingerprint(&key.features);
        let exact = same_target
            .iter()
            .copied()
            .filter(|record| {
                record.features == key.features && record.feature_fingerprint == fingerprint
            })
            .collect::<Vec<_>>();
        if exact.len() > 1 {
            return Err(RegistryError::Ambiguous(format!(
                "{} records match hash {}, target {}, and feature fingerprint {}",
                exact.len(),
                key.hash,
                key.target_arch,
                fingerprint
            )));
        }
        if let Some(record) = exact.into_iter().next() {
            if record.protocol_major != 1 || record.model_major != 4 {
                return Err(RegistryError::InvalidRecord(format!(
                    "record protocol/model majors are {}/{} rather than 1/4",
                    record.protocol_major, record.model_major
                )));
            }
            return Ok(RegistrySelection {
                key: key.clone(),
                record: record.clone(),
            });
        }

        let expected = same_target[0];
        let missing = expected
            .features
            .iter()
            .filter(|feature| !key.features.contains(feature))
            .cloned()
            .collect::<Vec<_>>();
        let forbidden = key
            .features
            .iter()
            .filter(|feature| {
                expected
                    .forbidden_features
                    .iter()
                    .any(|item| item == *feature)
                    || (expected.known_features.iter().any(|item| item == *feature)
                        && !expected.features.contains(feature))
            })
            .cloned()
            .collect::<Vec<_>>();
        let unknown = key
            .features
            .iter()
            .filter(|feature| {
                !expected.features.contains(feature)
                    && !forbidden.iter().any(|item| item == *feature)
            })
            .cloned()
            .collect::<Vec<_>>();
        Err(RegistryError::FeatureMismatch {
            missing,
            forbidden,
            unknown,
        })
    }
}

impl RegistrySelection {
    pub fn key(&self) -> &ExactSelectionKey {
        &self.key
    }

    pub fn record(&self) -> &CompatibilityRecord {
        &self.record
    }

    pub fn record_sha256(&self) -> Result<String, RegistryError> {
        self.record.sha256()
    }

    pub fn load_profile(&self, root: &Path) -> Result<ResolvedDartProfile, RegistryError> {
        let path = resolve_contained(root, &self.record.profile.path, "profile")?;
        dart_profile::load_profile_artifact(
            &path,
            &self.record.profile.id,
            &self.record.profile.sha256,
            self.record.sdk_aliases.clone(),
        )
        .map_err(RegistryError::Profile)
    }

    pub fn resolve_artifact(
        &self,
        root: &Path,
        host_os: &str,
        host_arch: &str,
    ) -> Result<ResolvedArtifact, RegistryError> {
        let variant = self
            .record
            .artifact
            .variants
            .iter()
            .find(|variant| variant.host_os == host_os && variant.host_arch == host_arch)
            .cloned()
            .ok_or_else(|| {
                RegistryError::ArtifactAbsent(format!(
                    "no artifact variant for host {host_os}/{host_arch}"
                ))
            })?;
        // Absence is checked before containment so that "never installed" does
        // not arrive as the same canonicalization failure as "points outside the
        // store". A path that escapes is still refused below.
        if validate_relative_path(&variant.path).is_ok() && !root.join(&variant.path).exists() {
            return Err(RegistryError::ArtifactAbsent(format!(
                "adapter artifact {} is not installed",
                root.join(&variant.path).display()
            )));
        }
        let path = resolve_contained(root, &variant.path, "adapter artifact")?;
        verify_file(
            &path,
            variant.size,
            &variant.sha256,
            MAX_ARTIFACT_BYTES,
            "adapter artifact",
        )?;
        Ok(ResolvedArtifact { path, variant })
    }

    pub fn resolve_current_artifact(&self, root: &Path) -> Result<ResolvedArtifact, RegistryError> {
        self.resolve_artifact(root, std::env::consts::OS, std::env::consts::ARCH)
    }
}

fn resolve_contained(root: &Path, relative: &str, label: &str) -> Result<PathBuf, RegistryError> {
    validate_relative_path(relative)?;
    // Naming the label and the path matters: profiles resolve against the
    // read-only package data and artifacts against the writable store, so
    // "canonicalize registry root" left an operator unable to tell which of the
    // two directories was missing.
    let root = root.canonicalize().map_err(|err| {
        RegistryError::Artifact(format!(
            "{label} root {} is unavailable: {err}",
            root.display()
        ))
    })?;
    let path = root.join(relative);
    let canonical = path.canonicalize().map_err(|err| {
        RegistryError::Artifact(format!("{label} {} is unavailable: {err}", path.display()))
    })?;
    if !canonical.starts_with(&root) {
        return Err(RegistryError::Artifact(format!(
            "{label} {} escapes registry root",
            path.display()
        )));
    }
    let metadata = fs::metadata(&canonical).map_err(|err| {
        RegistryError::Artifact(format!("read {label} {}: {err}", canonical.display()))
    })?;
    if !metadata.is_file() {
        return Err(RegistryError::Artifact(format!(
            "{label} {} is not a regular file",
            canonical.display()
        )));
    }
    Ok(canonical)
}

fn verify_file(
    path: &Path,
    expected_size: u64,
    expected_digest: &str,
    max_bytes: u64,
    label: &str,
) -> Result<(), RegistryError> {
    if !valid_digest(expected_digest) {
        return Err(RegistryError::Artifact(format!(
            "{label} digest is not lowercase SHA-256"
        )));
    }
    let metadata = fs::metadata(path)
        .map_err(|err| RegistryError::Artifact(format!("read {label} metadata: {err}")))?;
    if metadata.len() != expected_size {
        return Err(RegistryError::Artifact(format!(
            "{label} size mismatch: expected {}, got {}",
            expected_size,
            metadata.len()
        )));
    }
    if metadata.len() > max_bytes {
        return Err(RegistryError::Artifact(format!(
            "{label} exceeds the {} byte limit",
            max_bytes
        )));
    }
    let file = fs::File::open(path)
        .map_err(|err| RegistryError::Artifact(format!("open {label}: {err}")))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| RegistryError::Artifact(format!("read {label}: {err}")))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = format!("{:x}", hasher.finalize());
    if digest != expected_digest {
        return Err(RegistryError::Artifact(format!(
            "{label} SHA-256 mismatch: expected {expected_digest}, got {digest}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{SnapshotKind, TargetArch};

    fn digest(seed: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(seed.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn record(features: &[&str]) -> CompatibilityRecord {
        let features = features
            .iter()
            .map(|item| (*item).to_string())
            .collect::<Vec<_>>();
        CompatibilityRecord {
            snapshot_hash: "80a49c7111088100a233b2ae788e1f48".to_string(),
            snapshot_kind: SnapshotKind::FullAot,
            target_arch: TargetArch::Arm64,
            feature_fingerprint: canonical_feature_fingerprint(&features),
            features,
            known_features: Vec::new(),
            forbidden_features: Vec::new(),
            sdk_aliases: vec![SdkAlias {
                ecosystem: "dart".to_string(),
                version: "3.5.0".to_string(),
                provenance: "fixture".to_string(),
            }],
            parser_family: ParserFamilyReference {
                id: "family".to_string(),
                version: None,
                sha256: None,
            },
            profile: ProfileReference {
                id: "profile".to_string(),
                path: "profile.json".to_string(),
                sha256: digest("profile"),
            },
            artifact: ArtifactReference {
                id: "artifact".to_string(),
                variants: Vec::new(),
            },
            evidence: CompatibilityEvidence {
                source: "fixture".to_string(),
                provenance: "test".to_string(),
                references: Vec::new(),
            },
            trust_tier: TrustTier::Verified,
            protocol_major: 1,
            model_major: 4,
        }
    }

    #[test]
    fn reordered_features_share_one_exact_key() {
        let registry = CompatibilityRegistry {
            version: REGISTRY_VERSION,
            records: vec![record(&["arm64", "compressed-pointers", "product"])],
        };
        registry.validate().unwrap();
        let identity = SnapshotIdentity::from_header(
            TargetArch::Arm64,
            "80a49c7111088100a233b2ae788e1f48",
            SnapshotKind::FullAot,
            "product compressed-pointers arm64",
        );
        assert!(registry.select(&identity).is_ok());
    }

    /// A registry that cannot say which record covers a snapshot says so, and
    /// says it the same way whether the duplicate is caught while loading or
    /// while selecting.
    ///
    /// The distinction is not cosmetic. `Ambiguous` is the one refusal a caller
    /// must not answer with "this snapshot is unsupported": it is a fact about
    /// the installation, and a caller that mislabels it hides a broken registry
    /// behind a heuristic result.
    #[test]
    fn two_records_under_one_exact_key_are_ambiguous() {
        let one = record(&["arm64", "compressed-pointers", "product"]);
        let registry = CompatibilityRegistry {
            version: REGISTRY_VERSION,
            records: vec![one.clone(), one.clone()],
        };
        assert!(
            matches!(registry.validate(), Err(RegistryError::Ambiguous(_))),
            "validate reported {:?}",
            registry.validate()
        );
        // Through the parse boundary too, since that is the only way the
        // pipeline ever builds one.
        let bytes = serde_json::to_vec(&registry).expect("serialize");
        assert!(matches!(
            CompatibilityRegistry::from_json(&bytes),
            Err(RegistryError::Ambiguous(_))
        ));
        // The control: one record under that key loads and selects.
        let single = CompatibilityRegistry {
            version: REGISTRY_VERSION,
            records: vec![one],
        };
        single.validate().expect("one record is not ambiguous");
    }

    #[test]
    fn unknown_and_forbidden_features_fail_closed() {
        let mut expected = record(&["arm64", "compressed-pointers", "product"]);
        expected.known_features = vec!["debug".to_string()];
        expected.forbidden_features = vec!["jit".to_string()];
        let registry = CompatibilityRegistry {
            version: REGISTRY_VERSION,
            records: vec![expected],
        };
        let unknown = SnapshotIdentity::from_header(
            TargetArch::Arm64,
            "80a49c7111088100a233b2ae788e1f48",
            SnapshotKind::FullAot,
            "product compressed-pointers arm64 mystery",
        );
        assert!(matches!(
            registry.select(&unknown),
            Err(RegistryError::FeatureMismatch { unknown, .. }) if unknown == vec!["mystery"]
        ));
        let forbidden = SnapshotIdentity::from_header(
            TargetArch::Arm64,
            "80a49c7111088100a233b2ae788e1f48",
            SnapshotKind::FullAot,
            "product compressed-pointers arm64 jit",
        );
        assert!(matches!(
            registry.select(&forbidden),
            Err(RegistryError::FeatureMismatch { forbidden, .. }) if forbidden == vec!["jit"]
        ));
    }
}
