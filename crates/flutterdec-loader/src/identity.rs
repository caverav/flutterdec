//! Typed, header-derived snapshot identity.
//!
//! Everything a compatibility decision is allowed to rest on comes from the
//! snapshot header itself: the 32-character version hash, the snapshot kind, and
//! the features string the VM wrote next to them. Filenames, semantic Dart
//! versions, adapter output, and byte scans are not identity; they are at best
//! evidence *about* a snapshot, and this module keeps that distinction in the
//! type system so no caller can lose it.
//!
//! `runtime/vm/snapshot.h` fixes the header layout and `WriteVersionAndFeatures`
//! (`runtime/vm/app_snapshot.cc`) fixes what follows it.

use serde::{Deserialize, Serialize};
use std::fmt;

/// `Snapshot::Kind` from `runtime/vm/snapshot.h`, in declaration order.
///
/// Only `FullAot` can reach exact adapter selection. The other kinds are real
/// Dart snapshots with different serializer output, so accepting one under an
/// AOT parser would produce confident nonsense rather than an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotKind {
    Full,
    FullCore,
    FullJit,
    FullAot,
    /// The header parsed but its kind field is not one the VM writes.
    Unrecognized,
}

impl SnapshotKind {
    /// Decode the header's `int64` kind field.
    pub fn from_header_value(value: i64) -> Self {
        match value {
            0 => Self::Full,
            1 => Self::FullCore,
            2 => Self::FullJit,
            3 => Self::FullAot,
            _ => Self::Unrecognized,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::FullCore => "full_core",
            Self::FullJit => "full_jit",
            Self::FullAot => "full_aot",
            Self::Unrecognized => "unrecognized",
        }
    }
}

impl fmt::Display for SnapshotKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where the hash came from, which decides what it may authorize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HashSource {
    /// Read at its fixed offset in a validated snapshot header. Exact.
    Header,
    /// Recovered by scanning bytes for something hash-shaped. Heuristic: a
    /// 32-hex run in a data section is not proof of a snapshot version.
    Scan,
    /// Neither worked. Nothing may be assumed.
    Unavailable,
}

/// The architecture the snapshot's code was generated *for*.
///
/// Distinct from the host architecture, and distinct from the ELF machine of the
/// container: those coincide today only because the sole supported input is an
/// Android ARM64 `libapp.so`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetArch {
    Arm64,
    /// A target this build does not support, kept verbatim for diagnostics.
    Unsupported(String),
}

impl TargetArch {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Arm64 => "arm64",
            Self::Unsupported(other) => other,
        }
    }
}

impl fmt::Display for TargetArch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Pointer width mode, read from the features string rather than inferred.
///
/// `Dart::FeaturesString` (`runtime/vm/dart.cc`) appends exactly one of
/// `compressed-pointers` or `no-compressed-pointers`. Both present is not a
/// snapshot the VM writes, so it is a conflict rather than a value to pick from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerCompression {
    Compressed,
    Uncompressed,
    /// The features string said nothing about it.
    Unavailable,
    /// The features string said both.
    Conflicting,
}

/// The snapshot's features string, raw and normalized.
///
/// Normalization is what a registry key can compare: whitespace-split, ASCII
/// lowercased, sorted, deduplicated. The raw string is retained because it is
/// the actual evidence and normalization is lossy about ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureEvidence {
    /// Absent when no header parsed.
    pub raw: Option<String>,
    pub normalized: Vec<String>,
}

impl FeatureEvidence {
    pub fn unavailable() -> Self {
        Self {
            raw: None,
            normalized: Vec::new(),
        }
    }

    pub fn parse(raw: &str) -> Self {
        let mut normalized: Vec<String> = raw
            .split_whitespace()
            .map(|token| token.to_ascii_lowercase())
            .collect();
        normalized.sort();
        normalized.dedup();
        Self {
            raw: Some(raw.to_string()),
            normalized,
        }
    }

    pub fn has(&self, token: &str) -> bool {
        self.normalized.iter().any(|t| t == token)
    }

    /// All architecture tokens the VM declared, in normalized order.
    pub fn declared_targets(&self) -> Vec<String> {
        self.declared_target_tokens()
            .map(ToString::to_string)
            .collect()
    }

    /// The architecture token the features string declares, if it declares one.
    ///
    /// These are the values `Dart::FeaturesString` can append; anything else in
    /// the string is a build flag, not an architecture.
    pub fn declared_target(&self) -> Option<&str> {
        self.declared_target_tokens().next()
    }

    fn declared_target_tokens(&self) -> impl Iterator<Item = &str> {
        const ARCH_TOKENS: [&str; 6] = ["ia32", "x64", "arm", "arm64", "riscv32", "riscv64"];
        self.normalized
            .iter()
            .map(String::as_str)
            .filter(|token| ARCH_TOKENS.contains(token))
    }

    pub fn pointer_compression(&self) -> PointerCompression {
        let compressed = self.has("compressed-pointers");
        let uncompressed = self.has("no-compressed-pointers");
        match (compressed, uncompressed) {
            (true, true) => PointerCompression::Conflicting,
            (true, false) => PointerCompression::Compressed,
            (false, true) => PointerCompression::Uncompressed,
            (false, false) => PointerCompression::Unavailable,
        }
    }
}

/// What is actually known about a loaded snapshot, and how well it is known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotIdentity {
    /// Normalized lowercase 32-character hash. `None` when unrecoverable.
    pub hash: Option<String>,
    pub hash_source: HashSource,
    /// `None` when no header parsed, so the kind is genuinely unknown rather
    /// than assumed to be AOT.
    pub kind: Option<SnapshotKind>,
    /// Read from the ELF container's machine field.
    pub target_arch: TargetArch,
    pub features: FeatureEvidence,
    pub pointer_compression: PointerCompression,
}

/// Why an identity may not authorize exact adapter selection.
///
/// Each variant names the check that stopped it, so a caller can report the
/// rejection point rather than a generic failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityRejection {
    /// No header parsed, or the hash was only scanned out of the bytes.
    HashNotHeaderDerived(HashSource),
    /// FullAOT is a pre-lookup hard gate, not a registry key component.
    NotFullAot(Option<SnapshotKind>),
    UnsupportedTarget(String),
    /// The features string names more than one target architecture.
    ConflictingTargetFeatures(Vec<String>),
    /// The features string names a different architecture than the container.
    TargetArchConflict {
        declared: String,
        container: String,
    },
    PointerCompressionUnavailable(PointerCompression),
}

impl fmt::Display for IdentityRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HashNotHeaderDerived(source) => write!(
                f,
                "snapshot hash is not header-derived (source: {:?}); a scanned or missing hash cannot select an exact parser",
                source
            ),
            Self::NotFullAot(kind) => write!(
                f,
                "snapshot kind is {}; only full_aot reaches adapter compatibility selection",
                kind.map(SnapshotKind::as_str).unwrap_or("unknown")
            ),
            Self::UnsupportedTarget(arch) => {
                write!(f, "unsupported target architecture {}", arch)
            }
            Self::ConflictingTargetFeatures(declared) => write!(
                f,
                "features string declares contradictory target architectures {:?}",
                declared
            ),
            Self::TargetArchConflict {
                declared,
                container,
            } => write!(
                f,
                "features string declares target {} but the container is {}",
                declared, container
            ),
            Self::PointerCompressionUnavailable(state) => write!(
                f,
                "pointer compression evidence is {:?}; the word size of a reference field cannot be assumed",
                state
            ),
        }
    }
}

impl std::error::Error for IdentityRejection {}

/// The tuple exact compatibility selection is allowed to key on.
///
/// FullAOT is absent by design: it is a gate that must already have passed, so
/// putting it in the key would multiply the registry with rows that can never
/// match. Semantic Dart versions are absent for the same reason plus a stronger
/// one, they are aliases of a hash and never selectors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactSelectionKey {
    pub hash: String,
    pub target_arch: TargetArch,
    pub features: Vec<String>,
}

impl SnapshotIdentity {
    /// The identity of a container whose snapshot header did not parse.
    pub fn without_header(target_arch: TargetArch, scanned_hash: Option<String>) -> Self {
        let (hash, hash_source) = match scanned_hash {
            Some(hash) => (Some(hash), HashSource::Scan),
            None => (None, HashSource::Unavailable),
        };
        Self {
            hash,
            hash_source,
            kind: None,
            target_arch,
            features: FeatureEvidence::unavailable(),
            pointer_compression: PointerCompression::Unavailable,
        }
    }

    /// The identity of a container whose snapshot header parsed.
    pub fn from_header(
        target_arch: TargetArch,
        hash: &str,
        kind: SnapshotKind,
        features_raw: &str,
    ) -> Self {
        let features = FeatureEvidence::parse(features_raw);
        let pointer_compression = features.pointer_compression();
        Self {
            hash: Some(hash.to_ascii_lowercase()),
            hash_source: HashSource::Header,
            kind: Some(kind),
            target_arch,
            features,
            pointer_compression,
        }
    }

    /// Whether this identity was read out of a real header.
    pub fn is_exact(&self) -> bool {
        self.hash_source == HashSource::Header
    }

    /// The pre-lookup gate. `Ok` means, and only means, that exact compatibility
    /// selection may be attempted; it is not a claim that a record exists.
    pub fn exact_selection_key(&self) -> Result<ExactSelectionKey, IdentityRejection> {
        if self.hash_source != HashSource::Header {
            return Err(IdentityRejection::HashNotHeaderDerived(self.hash_source));
        }
        let hash = self
            .hash
            .clone()
            .ok_or(IdentityRejection::HashNotHeaderDerived(
                HashSource::Unavailable,
            ))?;
        if self.kind != Some(SnapshotKind::FullAot) {
            return Err(IdentityRejection::NotFullAot(self.kind));
        }
        let TargetArch::Arm64 = self.target_arch else {
            return Err(IdentityRejection::UnsupportedTarget(
                self.target_arch.as_str().to_string(),
            ));
        };
        let declared_targets = self.features.declared_targets();
        if declared_targets.len() > 1 {
            return Err(IdentityRejection::ConflictingTargetFeatures(
                declared_targets,
            ));
        }
        if let Some(declared) = self.features.declared_target() {
            if declared != self.target_arch.as_str() {
                return Err(IdentityRejection::TargetArchConflict {
                    declared: declared.to_string(),
                    container: self.target_arch.as_str().to_string(),
                });
            }
        }
        match self.pointer_compression {
            PointerCompression::Compressed | PointerCompression::Uncompressed => {}
            other => return Err(IdentityRejection::PointerCompressionUnavailable(other)),
        }
        Ok(ExactSelectionKey {
            hash,
            target_arch: self.target_arch.clone(),
            features: self.features.normalized.clone(),
        })
    }
}
