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

use crate::model::{Diagnostic, InputRegionName, MODEL_VERSION};
use crate::primitives::{RelativePath, Sha256Digest};
use flutterdec_loader::identity::SnapshotIdentity;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt;

/// The only accepted protocol major.
pub const PROTOCOL_MAJOR: u32 = 1;

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
    /// Digest of the compatibility record that selected this adapter. Echoed
    /// into the model so the run can be tied back to the decision.
    pub compatibility_record_sha256: Sha256Digest,
    /// The host's identity for the snapshot. Not a suggestion: the adapter must
    /// report it back unchanged.
    pub identity: SnapshotIdentity,
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
        Ok(())
    }

    pub fn ok(model: RelativePath, diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            protocol_major: PROTOCOL_MAJOR,
            model_major: MODEL_VERSION,
            status: AdapterStatus::Ok,
            model: Some(model),
            error: None,
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
