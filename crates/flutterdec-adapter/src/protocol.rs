//! Adapter protocol v1: one request in, one result out.
//!
//! An adapter run is a single process invocation. There is no session, no
//! JSON-RPC lifecycle, no streaming, and no persistent worker, because none of
//! those are needed to answer one question about one snapshot and all of them
//! add state a hostile or broken adapter could sit inside.
//!
//! Snapshot bytes are never in these documents. Each region is an
//! [`InputHandle`]: a contained relative path, its size, and its digest. That is
//! what keeps a request a few hundred bytes for a hundred-megabyte snapshot, and
//! it is why the request type has nowhere to put base64.
//!
//! Both documents carry the protocol and model majors they were written for.
//! Version negotiation is a rejection, not a translation.

use crate::model::{CompatibilityBinding, Diagnostic, InputRegionName, Producer, MODEL_VERSION};
use crate::primitives::{RelativePath, Sha256Digest};
use flutterdec_loader::identity::SnapshotIdentity;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt;

/// The only accepted protocol major.
pub const PROTOCOL_MAJOR: u32 = 1;

/// A checked-in producer backend.
///
/// A closed enum rather than a string, because the previous design read the
/// resolved backend out of a substring of an adapter-authored free-text field,
/// which meant an adapter could name itself `r2flutter_...` and be treated as
/// one. Membership here is the only way to be a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendId {
    /// String carving plus prologue scanning. No exact names, no real pool.
    Internal,
    Blutter,
    /// `r2flutter`: deserializes the snapshot, so it is the only backend that
    /// can supply exact names and a hardware pool index space.
    ///
    /// Spelled out rather than left to `rename_all`, which would derive
    /// `r2_flutter` and put a second spelling of one backend on the wire: the
    /// request's `requested_backend` serializes through [`Self::as_str`], so a
    /// producer echoing the token it was given back as `resolved_backend` would
    /// be rejected by its own request's vocabulary.
    #[serde(rename = "r2flutter")]
    R2Flutter,
}

impl BackendId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::Blutter => "blutter",
            Self::R2Flutter => "r2flutter",
        }
    }
}

impl fmt::Display for BackendId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the host asked for, which is distinct from what ran.
///
/// Serializes as one flat string, `auto` or a backend name, rather than as a
/// tagged variant: the wire form is what a producer parses, and one token is
/// easier to get right than a nested object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestedBackend {
    /// The producer may pick, and may fall back.
    Auto,
    /// The producer must use this one or fail. No silent substitution.
    Fixed(BackendId),
}

impl RequestedBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Fixed(backend) => backend.as_str(),
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "auto" => Some(Self::Auto),
            "internal" => Some(Self::Fixed(BackendId::Internal)),
            "blutter" => Some(Self::Fixed(BackendId::Blutter)),
            "r2flutter" => Some(Self::Fixed(BackendId::R2Flutter)),
            _ => None,
        }
    }

    pub fn fixed(self) -> Option<BackendId> {
        match self {
            Self::Auto => None,
            Self::Fixed(backend) => Some(backend),
        }
    }
}

impl Serialize for RequestedBackend {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RequestedBackend {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| {
            serde::de::Error::custom(format!("unknown requested backend {:?}", text))
        })
    }
}

impl fmt::Display for RequestedBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why the backend that ran is not the one `auto` would have preferred.
///
/// Closed for the same reason [`DiagnosticCode`] is: a free-text reason cannot
/// be checked, and "fell back for some reason" is not a fact a host can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
    /// The preferred backend's tooling is not installed.
    BackendUnavailable,
    /// The preferred backend ran and failed on this snapshot.
    BackendFailed,
}

impl FallbackReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BackendUnavailable => "backend_unavailable",
            Self::BackendFailed => "backend_failed",
        }
    }
}

impl fmt::Display for FallbackReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One input region, as a handle rather than as content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputHandle {
    pub region: InputRegionName,
    /// Relative to the adapter's working directory, and contained by
    /// construction. The adapter may read it and must not write it.
    pub path: RelativePath,
    pub size: u64,
    /// Digest of the bytes at `path`, so the adapter can confirm it read what
    /// the host meant to send.
    pub sha256: Sha256Digest,
    /// Load address, present exactly for executable regions.
    pub virtual_address: Option<u64>,
    pub executable: bool,
}

/// Everything an adapter is given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterRequest {
    pub protocol_major: u32,
    pub model_major: u32,
    /// The compatibility record the host selected, digest included. Echoed into
    /// the model so a model can be tied back to the decision that produced it,
    /// and so a model produced under a different decision is detectable.
    pub compatibility: CompatibilityBinding,
    /// Who the host believes is running. The adapter reports it back verbatim;
    /// it does not get to describe itself, which is what stops a producer from
    /// promoting its own trust level.
    pub producer: Producer,
    /// The host's identity for the snapshot. Not a suggestion: the adapter must
    /// report it back unchanged.
    pub identity: SnapshotIdentity,
    /// Which backend the host wants. `Fixed` forbids substitution.
    pub requested_backend: RequestedBackend,
    pub inputs: Vec<InputHandle>,
    /// Where the adapter writes its `ProgramModel`.
    pub output: RelativePath,
}

/// The outcome of one adapter invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterStatus {
    /// A model was written to the requested output path.
    Ok,
    /// The adapter understood the request and cannot serve it. Distinct from
    /// `Failed`: retrying or fixing the input will not help.
    Unsupported,
    /// The adapter tried and could not finish.
    Failed,
}

/// Stable failure codes.
///
/// Stable because operators and tests match on them; the message is for humans
/// and may change, the code may not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterErrorCode {
    /// The request's protocol major is not one this adapter implements.
    UnsupportedProtocol,
    /// The requested model major is not one this adapter can emit.
    UnsupportedModelVersion,
    /// The adapter has no parser for this snapshot identity.
    UnsupportedSnapshot,
    /// A declared input handle was not readable.
    InputMissing,
    /// An input's bytes did not match its declared digest or size.
    InputDigestMismatch,
    /// The snapshot's own header disagrees with the identity in the request.
    IdentityMismatch,
    /// The parser ran and failed on the snapshot's contents.
    ParseFailed,
    /// The model could not be written to the output handle.
    OutputWriteFailed,
    /// Anything else. Carries no contract beyond "this run produced nothing".
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterError {
    pub code: AdapterErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterResult {
    pub protocol_major: u32,
    pub model_major: u32,
    pub status: AdapterStatus,
    /// Present exactly when `status` is `Ok`. A path, never a model: the model
    /// is a separate artifact so it can be large without bounding this document.
    pub model: Option<RelativePath>,
    /// Present exactly when `status` is not `Ok`.
    pub error: Option<AdapterError>,
    /// Which backend actually produced the model. Present exactly when `status`
    /// is `Ok`, and the only place the host reads it from.
    pub resolved_backend: Option<BackendId>,
    /// Present only when the host asked for `auto` and the preferred backend
    /// did not run.
    pub fallback_reason: Option<FallbackReason>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Why a protocol document is not usable.
#[derive(Debug, Clone, PartialEq)]
pub enum ProtocolError {
    Malformed(String),
    UnsupportedProtocolMajor(u32),
    UnsupportedModelMajor(u32),
    DuplicateRegion(InputRegionName),
    MissingRegion(InputRegionName),
    /// A region declaring an executability that contradicts what it is.
    RegionExecutabilityMismatch(InputRegionName),
    /// An executable region without a load address, or a data region with one.
    RegionAddressMismatch(InputRegionName),
    /// A region whose load address plus size leaves the address space.
    RegionOverflows(InputRegionName),
    EmptyRegion(InputRegionName),
    /// Two handles pointing at the same path, or an output aliasing an input.
    AliasedPath(String),
    /// `status` and the `model`/`error` fields disagree.
    StatusPayloadMismatch,
    /// `resolved_backend` is present without success, or absent with it.
    ResolvedBackendPayloadMismatch,
    /// The host pinned a backend and a different one answered.
    BackendSubstituted {
        requested: BackendId,
        resolved: BackendId,
    },
    /// A fallback reason on a result whose backend was pinned, so there was
    /// nothing to fall back from.
    FallbackWithoutAuto,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(detail) => write!(f, "malformed protocol document: {}", detail),
            Self::UnsupportedProtocolMajor(major) => write!(
                f,
                "unsupported protocol major {}; this host implements {}",
                major, PROTOCOL_MAJOR
            ),
            Self::UnsupportedModelMajor(major) => write!(
                f,
                "unsupported model major {}; this host implements {}",
                major, MODEL_VERSION
            ),
            Self::DuplicateRegion(region) => write!(f, "input region {} declared twice", region),
            Self::MissingRegion(region) => write!(f, "request omits input region {}", region),
            Self::RegionExecutabilityMismatch(region) => write!(
                f,
                "input {} declares an executability that contradicts what it is",
                region
            ),
            Self::RegionAddressMismatch(region) => write!(
                f,
                "input {} must have a virtual address exactly when it is executable",
                region
            ),
            Self::RegionOverflows(region) => {
                write!(f, "input {} runs past the end of the address space", region)
            }
            Self::EmptyRegion(region) => write!(f, "input {} declares a zero size", region),
            Self::AliasedPath(path) => write!(
                f,
                "path handle {:?} is used more than once; inputs and the output must be distinct",
                path
            ),
            Self::StatusPayloadMismatch => f.write_str(
                "an ok result must carry a model and no error, and a failed result must carry an error and no model",
            ),
            Self::ResolvedBackendPayloadMismatch => f.write_str(
                "an ok result must name the backend that produced the model, and a result that produced nothing must not name one",
            ),
            Self::BackendSubstituted {
                requested,
                resolved,
            } => write!(
                f,
                "host pinned backend {} but {} answered; a pinned backend may fail, never be substituted",
                requested, resolved
            ),
            Self::FallbackWithoutAuto => f.write_str(
                "a fallback reason is only meaningful when the host asked for auto",
            ),
        }
    }
}

impl std::error::Error for ProtocolError {}

/// Read the two majors before deserializing, so a document written for another
/// version is rejected as such rather than as a shape error.
fn check_majors(raw: &Value) -> Result<(), ProtocolError> {
    let protocol = raw
        .get("protocol_major")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProtocolError::Malformed("missing protocol_major".to_string()))?;
    if protocol != u64::from(PROTOCOL_MAJOR) {
        return Err(ProtocolError::UnsupportedProtocolMajor(protocol as u32));
    }
    let model = raw
        .get("model_major")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProtocolError::Malformed("missing model_major".to_string()))?;
    if model != u64::from(MODEL_VERSION) {
        return Err(ProtocolError::UnsupportedModelMajor(model as u32));
    }
    Ok(())
}

impl AdapterRequest {
    /// Parse and fully check fresh request JSON.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let raw: Value = serde_json::from_slice(bytes)
            .map_err(|err| ProtocolError::Malformed(err.to_string()))?;
        check_majors(&raw)?;
        let request: Self =
            serde_json::from_value(raw).map_err(|err| ProtocolError::Malformed(err.to_string()))?;
        request.validate()?;
        Ok(request)
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("AdapterRequest is always serializable")
    }

    /// Every structural rule a request must satisfy before an adapter runs.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.protocol_major != PROTOCOL_MAJOR {
            return Err(ProtocolError::UnsupportedProtocolMajor(self.protocol_major));
        }
        if self.model_major != MODEL_VERSION {
            return Err(ProtocolError::UnsupportedModelMajor(self.model_major));
        }

        let mut seen = BTreeSet::new();
        for input in &self.inputs {
            if !seen.insert(input.region) {
                return Err(ProtocolError::DuplicateRegion(input.region));
            }
            if input.executable != input.region.is_executable() {
                return Err(ProtocolError::RegionExecutabilityMismatch(input.region));
            }
            if input.executable != input.virtual_address.is_some() {
                return Err(ProtocolError::RegionAddressMismatch(input.region));
            }
            if input.size == 0 {
                return Err(ProtocolError::EmptyRegion(input.region));
            }
            if let Some(base) = input.virtual_address {
                if base.checked_add(input.size).is_none() {
                    return Err(ProtocolError::RegionOverflows(input.region));
                }
            }
        }
        for region in InputRegionName::ALL {
            if !seen.contains(&region) {
                return Err(ProtocolError::MissingRegion(region));
            }
        }

        // Distinct paths, and an output that cannot overwrite an input.
        let mut paths = BTreeSet::new();
        for input in &self.inputs {
            if !paths.insert(input.path.as_str()) {
                return Err(ProtocolError::AliasedPath(input.path.to_string()));
            }
        }
        if paths.contains(self.output.as_str()) {
            return Err(ProtocolError::AliasedPath(self.output.to_string()));
        }
        Ok(())
    }

    pub fn input(&self, region: InputRegionName) -> Option<&InputHandle> {
        self.inputs.iter().find(|i| i.region == region)
    }
}

impl AdapterResult {
    pub fn from_json(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let raw: Value = serde_json::from_slice(bytes)
            .map_err(|err| ProtocolError::Malformed(err.to_string()))?;
        check_majors(&raw)?;
        let result: Self =
            serde_json::from_value(raw).map_err(|err| ProtocolError::Malformed(err.to_string()))?;
        result.validate()?;
        Ok(result)
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("AdapterResult is always serializable")
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.protocol_major != PROTOCOL_MAJOR {
            return Err(ProtocolError::UnsupportedProtocolMajor(self.protocol_major));
        }
        if self.model_major != MODEL_VERSION {
            return Err(ProtocolError::UnsupportedModelMajor(self.model_major));
        }
        // Success without a model, or failure without a reason, are both results
        // the host cannot act on.
        let consistent = match self.status {
            AdapterStatus::Ok => self.model.is_some() && self.error.is_none(),
            AdapterStatus::Unsupported | AdapterStatus::Failed => {
                self.model.is_none() && self.error.is_some()
            }
        };
        if !consistent {
            return Err(ProtocolError::StatusPayloadMismatch);
        }
        // A model nobody will admit to producing is a model with no provenance,
        // and a backend named by a run that produced nothing is a claim about
        // work that did not happen.
        if self.resolved_backend.is_some() != matches!(self.status, AdapterStatus::Ok) {
            return Err(ProtocolError::ResolvedBackendPayloadMismatch);
        }
        Ok(())
    }

    /// The checks that need the request the result answers.
    ///
    /// Separate from [`AdapterResult::validate`] because a result read off disk
    /// can be checked for self-consistency on its own, but "did this answer the
    /// question that was asked" is only decidable with the question in hand.
    pub fn validate_against(&self, request: &AdapterRequest) -> Result<(), ProtocolError> {
        self.validate()?;
        if let (Some(requested), Some(resolved)) =
            (request.requested_backend.fixed(), self.resolved_backend)
        {
            if requested != resolved {
                return Err(ProtocolError::BackendSubstituted {
                    requested,
                    resolved,
                });
            }
        }
        if self.fallback_reason.is_some() && request.requested_backend.fixed().is_some() {
            return Err(ProtocolError::FallbackWithoutAuto);
        }
        Ok(())
    }

    pub fn ok(
        model: RelativePath,
        resolved_backend: BackendId,
        fallback_reason: Option<FallbackReason>,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        Self {
            protocol_major: PROTOCOL_MAJOR,
            model_major: MODEL_VERSION,
            status: AdapterStatus::Ok,
            model: Some(model),
            error: None,
            resolved_backend: Some(resolved_backend),
            fallback_reason,
            diagnostics,
        }
    }

    pub fn failed(code: AdapterErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_major: PROTOCOL_MAJOR,
            model_major: MODEL_VERSION,
            status: AdapterStatus::Failed,
            model: None,
            error: Some(AdapterError {
                code,
                message: message.into(),
            }),
            resolved_backend: None,
            fallback_reason: None,
            diagnostics: Vec::new(),
        }
    }

    pub fn unsupported(code: AdapterErrorCode, message: impl Into<String>) -> Self {
        Self {
            status: AdapterStatus::Unsupported,
            ..Self::failed(code, message)
        }
    }
}
