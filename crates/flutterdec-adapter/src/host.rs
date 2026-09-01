//! Bounded one-shot adapter execution.
//!
//! One adapter run is one process, started once, with everything decided before
//! it exists. The order is the whole design: every integrity and compatibility
//! check runs *before* a child is created, so a mismatch is a refusal with no
//! side effects rather than a process that has to be caught afterwards. Once the
//! child does exist it is contained by construction, not by convention: a
//! private workspace it cannot see out of, an allowlisted environment, an
//! overall deadline, bounded output, and a process group the host can terminate
//! whole.
//!
//! The bytes that run are the bytes that were checked, and no pathname is
//! involved in making that true. The store artifact is read once, digested from
//! that buffer, and turned into an executable inode the host holds open; every
//! pathname to that inode is gone before anything else happens, and the child is
//! created from the descriptor with `execveat`. So there is nothing left to
//! race: not the owner-writable store path, which is never opened again, and not
//! a workspace copy, because none exists to find, `chmod`, rename, or overwrite.
//!
//! The registry record is the authority throughout. The host re-derives the
//! record digest, the profile digest, the artifact digest, the host variant, the
//! target and feature tuple, and the protocol and model majors from the record
//! and refuses if any of them disagrees with what the caller believes. An
//! adapter never contributes to a decision about whether it may run.
//!
//! Every refusal is a [`HostError`] variant, so a caller can act on which check
//! stopped the run. Messages carry bounded, escaped excerpts of child output and
//! never the host environment, because a diagnostic that pastes an unbounded
//! child's stderr into a log is an output channel the adapter controls.

use crate::model::{
    CompatibilityBinding, InputRegion, InputRegionName, Producer, ProducerTrust, ProgramModel,
    MODEL_VERSION,
};
use crate::primitives::{RelativePath, Sha256Digest};
use crate::protocol::{
    self, AdapterRequest, AdapterResult, AdapterStatus, BackendId, RequestedBackend, PROTOCOL_MAJOR,
};
use crate::sandbox::{ContainmentReport, Limits};
use crate::validate::{self, HostSelectedContext};
use flutterdec_loader::identity::{IdentityRejection, SnapshotIdentity};
use flutterdec_loader::registry::{
    canonical_feature_fingerprint, CompatibilityRecord, HostArtifactVariant,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::ffi::{CString, OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod exec;
mod image;
use exec::Completion;
use image::{argument, ExecImage};

/// The scratch layout one invocation sees. Every one of these is relative to the
/// working directory, and the working directory is private to the invocation.
const INPUT_DIR: &str = "in";
const OUTPUT_DIR: &str = "out";
const HOME_DIR: &str = "home";
const TEMP_DIR: &str = "tmp";
const ARTIFACT_DIR: &str = "artifact";
/// Used as `argv[0]` when the store path has no usable final component.
const EXEC_FALLBACK_NAME: &str = "adapter";
/// What `Command` is told to run, which is never reached: the last pre-exec hook
/// executes the held image descriptor and does not return. It is a path that
/// cannot exist so that a hook that somehow did not run fails loudly instead of
/// starting something.
const UNREACHABLE_PROGRAM: &str = "/nonexistent/flutterdec-adapter-image-descriptor";
const OUTPUT_MODEL_PATH: &str = "out/model.json";
const REQUEST_PATH: &str = "request.json";
const RESULT_PATH: &str = "result.json";

/// Test hook: rendezvous with another process after the verified bytes are held
/// as a nameless executable descriptor and before the child is created.
///
/// Proving that replacing the store artifact cannot change which bytes execute
/// requires the replacement to land inside that window, and the window is a few
/// microseconds wide. Polling for it is a timing gamble; this makes it a
/// synchronization point. The host creates `<dir>/ready` and blocks until
/// `<dir>/go` appears or the wait times out.
///
/// It can only delay a run. Nothing here reads, re-reads, or re-resolves the
/// artifact, so an operator who sets it cannot change what executes; they can
/// only stall their own process.
pub const PRESPAWN_RENDEZVOUS_VAR: &str = "FLUTTERDEC_ADAPTER_PRESPAWN_RENDEZVOUS";

/// How long the rendezvous waits before giving up as a pre-spawn failure.
const RENDEZVOUS_TIMEOUT: Duration = Duration::from_secs(60);

/// The host variables an adapter may see.
///
/// Everything else is dropped. These are here because the checked-in producer
/// reads them to find an external backend, and because a `PATH` is what makes an
/// interpreter shebang resolvable. `HOME`, `TMPDIR` and `PWD` are not on this
/// list: they are *set* to directories inside the private workspace, so an
/// adapter that writes to either of them writes somewhere that is cleaned up.
///
/// `XDG_CACHE_HOME` is the one exception to "everything the adapter writes is
/// thrown away", and it is here on purpose. An external backend that compiles
/// itself on first use rebuilds on every invocation if its cache is private,
/// which is the difference between a bridge that works and one that nobody
/// uses. Passing the variable through means the operator decides: unset, the
/// backend caches inside the private workspace and the run leaves nothing
/// behind; set, it caches where the operator already keeps caches.
const ENVIRONMENT_ALLOWLIST: &[&str] = &[
    "PATH",
    "LANG",
    "LC_ALL",
    "PYTHON",
    "XDG_CACHE_HOME",
    "FLUTTERDEC_BLUTTER_CMD",
    "FLUTTERDEC_BLUTTER_PY",
    "FLUTTERDEC_R2FLUTTER_CMD",
    "FLUTTERDEC_R2FLUTTER_BIN",
    "FLUTTERDEC_R2FLUTTER_TIMEOUT",
];

/// How much child output a diagnostic may quote.
const EXCERPT_BYTES: usize = 2000;

/// Which output stream breached its cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

impl fmt::Display for OutputStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        })
    }
}

/// Why one adapter invocation was refused or did not produce a usable model.
///
/// The split matters more than the count. Everything from
/// [`Self::IdentityRejected`] through [`Self::ImageNotSealed`] is a
/// pre-spawn refusal: no process was created and nothing outside the host
/// happened. Everything after it describes a child that ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// The snapshot identity may not authorize any adapter at all.
    IdentityRejected(IdentityRejection),
    /// The compatibility record does not satisfy its own invariants.
    RecordInvalid(String),
    /// The record the caller acted on is not the record it says it is.
    RecordDigestMismatch {
        expected: String,
        actual: String,
    },
    /// The record was written for a different protocol or model major.
    UnsupportedMajors {
        record_protocol: u32,
        record_model: u32,
    },
    /// The record does not describe this snapshot.
    IdentityRecordMismatch {
        record: String,
        identity: String,
    },
    /// The record was written for a different target architecture.
    TargetMismatch {
        record: String,
        identity: String,
    },
    /// The snapshot's feature tuple is not the one the record was written for.
    FeatureMismatch {
        record_fingerprint: String,
        identity_fingerprint: String,
    },
    /// The selected artifact variant is not for this host.
    HostVariantMismatch {
        variant_os: String,
        variant_arch: String,
        host_os: String,
        host_arch: String,
    },
    /// The variant handed in is not one the record declares.
    VariantNotInRecord {
        artifact_id: String,
    },
    /// The executable is not where the record says, or is outside the store.
    ArtifactPathRejected(String),
    /// The path resolves to something that is not a regular executable file.
    ArtifactNotExecutable(String),
    /// The bytes about to be executed are not the bytes the registry authorized.
    ArtifactDigestMismatch {
        expected: String,
        actual: String,
        expected_size: u64,
        actual_size: u64,
    },
    /// The runtime profile does not match the digest the record pinned.
    ProfileRejected(String),
    /// The producer record the caller built does not follow from the registry
    /// record.
    ProducerMismatch(String),
    /// The compatibility binding the caller built does not follow from the
    /// registry record.
    BindingMismatch(String),
    /// A snapshot region is empty, oversized, or not one of the four.
    InputRejected(String),
    /// The request the host itself would refuse to answer.
    RequestRejected(String),
    /// The output handle is not a usable place to write a model.
    OutputHandleRejected(String),
    /// The verified bytes could not be held as an executable image whose
    /// contents are provably immutable, so nothing was executed.
    ///
    /// Only reachable where the host has an anonymous file to hold them in. The
    /// alternative to this refusal would be executing something a same-user
    /// process could still reach and rewrite through `/proc`, which is the one
    /// thing the pathless image exists to prevent.
    ImageNotSealed(String),
    /// The private workspace could not be built or torn down.
    Workspace(String),
    /// The child could not be created.
    Spawn(String),
    /// The child was still running at the deadline and its tree was killed.
    Timeout {
        after: Duration,
    },
    /// The child produced more than its cap on one stream and its tree was
    /// killed.
    OutputLimitExceeded {
        stream: OutputStream,
        limit: u64,
    },
    /// The child died on a signal.
    Crashed {
        signal: i32,
        stderr: String,
    },
    /// The child exited nonzero without leaving a result document.
    NoResult {
        status: String,
        stdout: String,
        stderr: String,
    },
    /// A document the child wrote is larger than its cap.
    DocumentTooLarge {
        document: String,
        size: u64,
        limit: u64,
    },
    /// A document the child wrote is not protocol v1 or not model v4.
    MalformedDocument {
        document: String,
        detail: String,
    },
    /// The result does not answer the request that was asked.
    ResultMismatch(String),
    /// The adapter answered with a model at a path other than the one it was
    /// given. Distinct from [`Self::OutputHandleRejected`], which is a refusal
    /// before anything ran.
    ModelPathMismatch {
        wrote: String,
        requested: String,
    },
    /// The adapter answered, and the answer is a failure.
    AdapterFailed {
        status: AdapterStatus,
        code: protocol::AdapterErrorCode,
        message: String,
    },
    /// The model contradicts a fact the host established before the run.
    ModelRejected(String),
    /// The child exists but never reported which containment controls it
    /// established, so the host cannot describe the run it just performed.
    ContainmentUnreported,
    Io(String),
}

impl HostError {
    /// Whether this refusal happened before any child process existed.
    ///
    /// The distinction is the contract: a pre-spawn refusal guarantees zero side
    /// effects outside the host, and callers and tests assert on exactly that.
    pub fn is_pre_spawn(&self) -> bool {
        matches!(
            self,
            Self::IdentityRejected(_)
                | Self::RecordInvalid(_)
                | Self::RecordDigestMismatch { .. }
                | Self::UnsupportedMajors { .. }
                | Self::IdentityRecordMismatch { .. }
                | Self::TargetMismatch { .. }
                | Self::FeatureMismatch { .. }
                | Self::HostVariantMismatch { .. }
                | Self::VariantNotInRecord { .. }
                | Self::ArtifactPathRejected(_)
                | Self::ArtifactNotExecutable(_)
                | Self::ArtifactDigestMismatch { .. }
                | Self::ProfileRejected(_)
                | Self::ProducerMismatch(_)
                | Self::BindingMismatch(_)
                | Self::InputRejected(_)
                | Self::RequestRejected(_)
                | Self::OutputHandleRejected(_)
                | Self::ImageNotSealed(_)
        )
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityRejected(rejection) => write!(f, "{rejection}"),
            Self::RecordInvalid(detail) => {
                write!(f, "compatibility record is not usable: {detail}")
            }
            Self::RecordDigestMismatch { expected, actual } => write!(
                f,
                "compatibility record digest mismatch: the run was authorized under {expected} and the record hashes to {actual}"
            ),
            Self::UnsupportedMajors {
                record_protocol,
                record_model,
            } => write!(
                f,
                "compatibility record declares protocol/model majors {record_protocol}/{record_model}; this host implements {PROTOCOL_MAJOR}/{MODEL_VERSION}"
            ),
            Self::IdentityRecordMismatch { record, identity } => write!(
                f,
                "compatibility record is for snapshot {record} and this snapshot is {identity}"
            ),
            Self::TargetMismatch { record, identity } => write!(
                f,
                "compatibility record targets {record} and this snapshot targets {identity}"
            ),
            Self::FeatureMismatch {
                record_fingerprint,
                identity_fingerprint,
            } => write!(
                f,
                "compatibility record was written for feature fingerprint {record_fingerprint} and this snapshot fingerprints to {identity_fingerprint}"
            ),
            Self::HostVariantMismatch {
                variant_os,
                variant_arch,
                host_os,
                host_arch,
            } => write!(
                f,
                "artifact variant is for host {variant_os}/{variant_arch} and this host is {host_os}/{host_arch}"
            ),
            Self::VariantNotInRecord { artifact_id } => write!(
                f,
                "the selected host variant is not declared by artifact {artifact_id}"
            ),
            Self::ArtifactPathRejected(detail) => {
                write!(f, "adapter executable path rejected: {detail}")
            }
            Self::ArtifactNotExecutable(detail) => {
                write!(f, "adapter executable rejected: {detail}")
            }
            Self::ArtifactDigestMismatch {
                expected,
                actual,
                expected_size,
                actual_size,
            } => write!(
                f,
                "adapter artifact changed after registry verification: expected {expected_size} bytes with {expected}, got {actual_size} bytes with {actual}"
            ),
            Self::ProfileRejected(detail) => write!(f, "runtime profile rejected: {detail}"),
            Self::ProducerMismatch(detail) => write!(f, "producer record rejected: {detail}"),
            Self::BindingMismatch(detail) => {
                write!(f, "compatibility binding rejected: {detail}")
            }
            Self::InputRejected(detail) => write!(f, "snapshot region rejected: {detail}"),
            Self::RequestRejected(detail) => write!(f, "adapter request is invalid: {detail}"),
            Self::OutputHandleRejected(detail) => {
                write!(f, "adapter output handle rejected: {detail}")
            }
            Self::ImageNotSealed(detail) => write!(
                f,
                "the adapter image could not be sealed, so nothing was run: {detail}"
            ),
            Self::Workspace(detail) => write!(f, "adapter workspace failed: {detail}"),
            Self::Spawn(detail) => write!(f, "adapter could not be started: {detail}"),
            Self::Timeout { after } => write!(
                f,
                "adapter exceeded its {:?} deadline; its process tree was terminated",
                after
            ),
            Self::OutputLimitExceeded { stream, limit } => write!(
                f,
                "adapter wrote more than {limit} bytes to {stream}; its process tree was terminated"
            ),
            Self::Crashed { signal, stderr } => write!(
                f,
                "adapter died on signal {signal}\nstderr (bounded):\n{stderr}"
            ),
            Self::NoResult {
                status,
                stdout,
                stderr,
            } => write!(
                f,
                "adapter failed with status {status} and wrote no result document\nstdout (bounded):\n{stdout}\nstderr (bounded):\n{stderr}"
            ),
            Self::DocumentTooLarge {
                document,
                size,
                limit,
            } => write!(
                f,
                "adapter {document} is {size} bytes and the limit is {limit}"
            ),
            Self::MalformedDocument { document, detail } => {
                write!(f, "adapter {document} rejected: {detail}")
            }
            Self::ResultMismatch(detail) => {
                write!(f, "adapter result does not answer the request: {detail}")
            }
            Self::ModelPathMismatch { wrote, requested } => write!(
                f,
                "adapter wrote its model to {wrote:?} instead of the requested {requested:?}"
            ),
            Self::AdapterFailed {
                status,
                code,
                message,
            } => write!(
                f,
                "adapter reported {status:?} ({code:?}): {message}"
            ),
            Self::ModelRejected(detail) => {
                write!(f, "adapter model failed semantic validation: {detail}")
            }
            Self::ContainmentUnreported => f.write_str(
                "the adapter child reported no containment record, so the host cannot state which controls were in force",
            ),
            Self::Io(detail) => write!(f, "adapter invocation I/O failed: {detail}"),
        }
    }
}

impl std::error::Error for HostError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IdentityRejected(rejection) => Some(rejection),
            _ => None,
        }
    }
}

/// One snapshot region, as the host read it.
#[derive(Debug, Clone, Copy)]
pub struct AdapterRegionInput<'a> {
    pub region: InputRegionName,
    pub bytes: &'a [u8],
    /// Load address. Required for executable regions, forbidden for data ones;
    /// [`AdapterRequest::validate`] rejects the other combinations.
    pub virtual_address: Option<u64>,
}

/// Where the shared object the adapter may want to re-read actually lives.
///
/// An APK member is not a path. Passing `lib/arm64-v8a/libapp.so` to a backend
/// that opens files gives it a path relative to a directory it has never seen,
/// so the host writes the member into the private workspace and hands over a
/// real one.
#[derive(Debug, Clone, Copy)]
pub enum LibappSource<'a> {
    /// A regular file on this host.
    File(&'a Path),
    /// A member of the container at `input_path`, by name and content.
    Member { name: &'a str, bytes: &'a [u8] },
}

/// Everything the registry decided, as the host resolved it.
///
/// This is what the pre-spawn gates check against. It is separate from
/// [`AdapterInput`] because it is the *authority*, and mixing authority with
/// operator choices in one struct is how an operator choice ends up being
/// treated as authority.
#[derive(Debug, Clone, Copy)]
pub struct HostAuthorization<'a> {
    /// The record that authorized this run.
    pub record: &'a CompatibilityRecord,
    /// The host variant of `record.artifact` that was selected.
    pub variant: &'a HostArtifactVariant,
    /// The writable store root every adapter executable must stay inside.
    pub store_root: &'a Path,
    /// Where `record.profile.path` resolved to in the read-only package data.
    pub profile_path: &'a Path,
}

/// Everything the host hands one adapter invocation.
#[derive(Debug, Clone)]
pub struct AdapterInput<'a> {
    /// Header-derived identity of the snapshot. Authoritative.
    pub identity: &'a SnapshotIdentity,
    /// What the registry authorized. Re-checked before anything is spawned.
    pub authorization: HostAuthorization<'a>,
    /// Who the host believes is about to run, including the digest of the
    /// artifact it is about to execute.
    pub producer: Producer,
    /// The compatibility decision that authorized this run.
    pub compatibility: CompatibilityBinding,
    pub regions: Vec<AdapterRegionInput<'a>>,
    /// The original artifact, for backends that re-read it themselves.
    pub input_path: Option<&'a Path>,
    pub libapp: Option<LibappSource<'a>>,
    pub requested_backend: RequestedBackend,
    /// What this invocation may consume.
    pub limits: Limits,
}

/// What one adapter invocation produced, with the facts about the run that the
/// core needs and must not re-derive from the model.
#[derive(Debug, Clone)]
pub struct AdapterRun {
    pub model: ProgramModel,
    /// The backend that actually ran, as the protocol reported it.
    pub resolved_backend: BackendId,
    pub fallback_reason: Option<protocol::FallbackReason>,
    pub diagnostics: Vec<crate::model::Diagnostic>,
    /// Which containment controls were established for the child that ran.
    pub containment: ContainmentReport,
}

fn region_file_name(region: InputRegionName) -> &'static str {
    match region {
        InputRegionName::VmData => "in/vm_data.bin",
        InputRegionName::IsolateData => "in/isolate_data.bin",
        InputRegionName::VmInstructions => "in/vm_instructions.bin",
        InputRegionName::IsolateInstructions => "in/isolate_instructions.bin",
    }
}

/// A bounded, printable rendering of child output.
///
/// The tail rather than the head: an interpreter puts the thing that went wrong
/// last. Control characters are replaced so a child cannot rewrite a host log
/// line with an escape sequence.
pub(crate) fn excerpt(bytes: &[u8]) -> String {
    let start = bytes.len().saturating_sub(EXCERPT_BYTES);
    let mut out = String::new();
    if start > 0 {
        out.push_str(&format!("[{start} earlier bytes omitted]\n"));
    }
    for character in String::from_utf8_lossy(&bytes[start..]).chars() {
        match character {
            '\n' | '\t' => out.push(character),
            other if other.is_control() => out.push('\u{fffd}'),
            other => out.push(other),
        }
    }
    out
}

fn digest_of(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Read a file that must not be larger than `limit`.
///
/// Metadata first so an enormous file is refused rather than read, and a capped
/// read after it so a file that grew between the two is still bounded.
fn read_bounded(path: &Path, document: &str, limit: u64) -> Result<Vec<u8>, HostError> {
    let metadata = fs::metadata(path)
        .map_err(|err| HostError::Io(format!("read {document} {}: {err}", path.display())))?;
    if !metadata.is_file() {
        return Err(HostError::MalformedDocument {
            document: document.to_string(),
            detail: format!("{} is not a regular file", path.display()),
        });
    }
    if metadata.len() > limit {
        return Err(HostError::DocumentTooLarge {
            document: document.to_string(),
            size: metadata.len(),
            limit,
        });
    }
    let file = fs::File::open(path)
        .map_err(|err| HostError::Io(format!("open {document} {}: {err}", path.display())))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| HostError::Io(format!("read {document} {}: {err}", path.display())))?;
    if bytes.len() as u64 > limit {
        return Err(HostError::DocumentTooLarge {
            document: document.to_string(),
            size: bytes.len() as u64,
            limit,
        });
    }
    Ok(bytes)
}

/// What the pre-spawn checks approved: the request, and the exact bytes they
/// verified.
///
/// The bytes travel with the request on purpose. A digest proves something
/// about the file that was read; it proves nothing about the file that is later
/// opened by the same path. Carrying the verified bytes out of authorization is
/// what lets the caller execute *those* rather than whatever the store path
/// resolves to next.
struct Authorized {
    request: AdapterRequest,
    /// The digest-verified artifact, read exactly once.
    artifact: Vec<u8>,
}

/// Every integrity and compatibility check, in the order they can be decided.
///
/// Returns the request that the checks approved together with the artifact
/// bytes they verified. Nothing here creates a process, writes outside a
/// caller-owned buffer, or consults the adapter.
fn authorize(input: &AdapterInput<'_>, exec_path: &Path) -> Result<Authorized, HostError> {
    // The identity gate first, so a snapshot that may not select an adapter
    // never reaches a registry record, a digest, or the filesystem.
    let key = input
        .identity
        .exact_selection_key()
        .map_err(HostError::IdentityRejected)?;

    let authorization = &input.authorization;
    let record = authorization.record;
    record
        .validate()
        .map_err(|err| HostError::RecordInvalid(err.to_string()))?;

    let record_digest = record
        .sha256()
        .map_err(|err| HostError::RecordInvalid(err.to_string()))?;
    if record_digest != input.compatibility.record_sha256.as_str() {
        return Err(HostError::RecordDigestMismatch {
            expected: input.compatibility.record_sha256.to_string(),
            actual: record_digest,
        });
    }

    if record.protocol_major != PROTOCOL_MAJOR || record.model_major != MODEL_VERSION {
        return Err(HostError::UnsupportedMajors {
            record_protocol: record.protocol_major,
            record_model: record.model_major,
        });
    }

    if record.snapshot_hash != key.hash {
        return Err(HostError::IdentityRecordMismatch {
            record: record.snapshot_hash.clone(),
            identity: key.hash.clone(),
        });
    }
    if record.target_arch != key.target_arch {
        return Err(HostError::TargetMismatch {
            record: record.target_arch.as_str().to_string(),
            identity: key.target_arch.as_str().to_string(),
        });
    }
    let identity_fingerprint = canonical_feature_fingerprint(&key.features);
    if record.features != key.features || record.feature_fingerprint != identity_fingerprint {
        return Err(HostError::FeatureMismatch {
            record_fingerprint: record.feature_fingerprint.clone(),
            identity_fingerprint,
        });
    }

    // The host architecture is not the target architecture. A record can be
    // right about the snapshot and still name an executable this machine cannot
    // run.
    let variant = authorization.variant;
    if !record
        .artifact
        .variants
        .iter()
        .any(|declared| declared == variant)
    {
        return Err(HostError::VariantNotInRecord {
            artifact_id: record.artifact.id.clone(),
        });
    }
    if variant.host_os != std::env::consts::OS || variant.host_arch != std::env::consts::ARCH {
        return Err(HostError::HostVariantMismatch {
            variant_os: variant.host_os.clone(),
            variant_arch: variant.host_arch.clone(),
            host_os: std::env::consts::OS.to_string(),
            host_arch: std::env::consts::ARCH.to_string(),
        });
    }

    authorize_artifact(exec_path, authorization.store_root, variant)?;
    // This read is the only time the store path is opened. The bytes it returns
    // are what gets digested, and the same buffer is what the caller writes into
    // the private workspace and executes, so a writer with access to the store
    // has nothing left to race against: replacing the file after this point
    // changes a path nobody reads again.
    let artifact_bytes = read_bounded(
        exec_path,
        "artifact",
        flutterdec_loader::registry::MAX_ARTIFACT_BYTES,
    )?;
    let artifact_digest = digest_of(&artifact_bytes);
    if artifact_digest != variant.sha256 || artifact_bytes.len() as u64 != variant.size {
        return Err(HostError::ArtifactDigestMismatch {
            expected: variant.sha256.clone(),
            actual: artifact_digest,
            expected_size: variant.size,
            actual_size: artifact_bytes.len() as u64,
        });
    }
    // The producer record travels to the adapter and into the model, so a digest
    // there that is not the digest of the file being executed would be a claim
    // about a different artifact.
    if input.producer.artifact_sha256.as_str() != artifact_digest {
        return Err(HostError::ProducerMismatch(format!(
            "producer names artifact {} and the executable hashes to {artifact_digest}",
            input.producer.artifact_sha256
        )));
    }
    if input.producer.id != record.parser_family.id {
        return Err(HostError::ProducerMismatch(format!(
            "producer id {:?} is not the record's parser family {:?}",
            input.producer.id, record.parser_family.id
        )));
    }
    if input.producer.trust != ProducerTrust::Registered {
        return Err(HostError::ProducerMismatch(format!(
            "a registry-authorized run cannot carry producer trust {:?}",
            input.producer.trust
        )));
    }

    let profile_bytes = read_bounded(
        authorization.profile_path,
        "profile",
        flutterdec_loader::dart_profile::MAX_PROFILE_BYTES,
    )
    .map_err(|err| HostError::ProfileRejected(err.to_string()))?;
    let profile_digest = digest_of(&profile_bytes);
    if profile_digest != record.profile.sha256 {
        return Err(HostError::ProfileRejected(format!(
            "profile {} hashes to {profile_digest} and the record pins {}",
            authorization.profile_path.display(),
            record.profile.sha256
        )));
    }

    if input.compatibility.parser_family_id != record.parser_family.id
        || input.compatibility.profile_id != record.profile.id
        || input.compatibility.profile_sha256.as_str() != record.profile.sha256
    {
        return Err(HostError::BindingMismatch(format!(
            "binding names {}/{}/{} and the record names {}/{}/{}",
            input.compatibility.parser_family_id,
            input.compatibility.profile_id,
            input.compatibility.profile_sha256,
            record.parser_family.id,
            record.profile.id,
            record.profile.sha256
        )));
    }

    Ok(Authorized {
        request: build_request(input)?,
        artifact: artifact_bytes,
    })
}

/// The executable must be the file the record named, inside the store, and
/// actually executable.
fn authorize_artifact(
    exec_path: &Path,
    store_root: &Path,
    variant: &HostArtifactVariant,
) -> Result<(), HostError> {
    let root = store_root.canonicalize().map_err(|err| {
        HostError::ArtifactPathRejected(format!(
            "adapter store root {} is unavailable: {err}",
            store_root.display()
        ))
    })?;
    let resolved = exec_path.canonicalize().map_err(|err| {
        HostError::ArtifactPathRejected(format!(
            "adapter executable {} is unavailable: {err}",
            exec_path.display()
        ))
    })?;
    if !resolved.starts_with(&root) {
        return Err(HostError::ArtifactPathRejected(format!(
            "adapter executable {} is outside the adapter store {}",
            resolved.display(),
            root.display()
        )));
    }
    // Containment alone is not enough: any file inside the store is contained,
    // and only one of them is the artifact this record authorized.
    let declared = root.join(&variant.path).canonicalize().map_err(|err| {
        HostError::ArtifactPathRejected(format!(
            "artifact {} declared by the record is unavailable: {err}",
            variant.path
        ))
    })?;
    if declared != resolved {
        return Err(HostError::ArtifactPathRejected(format!(
            "the record authorizes {} and the caller resolved {}",
            declared.display(),
            resolved.display()
        )));
    }

    let metadata = fs::symlink_metadata(&resolved).map_err(|err| {
        HostError::ArtifactNotExecutable(format!("read {}: {err}", resolved.display()))
    })?;
    if !metadata.is_file() {
        return Err(HostError::ArtifactNotExecutable(format!(
            "{} is not a regular file",
            resolved.display()
        )));
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(HostError::ArtifactNotExecutable(format!(
            "{} has mode {:o} and no execute bit",
            resolved.display(),
            metadata.permissions().mode() & 0o7777
        )));
    }
    Ok(())
}

/// The request, checked as hard as the host can check it before it is written.
fn build_request(input: &AdapterInput<'_>) -> Result<AdapterRequest, HostError> {
    let mut handles = Vec::with_capacity(input.regions.len());
    for region in &input.regions {
        let size = region.bytes.len() as u64;
        if size == 0 {
            return Err(HostError::InputRejected(format!(
                "region {} is empty",
                region.region
            )));
        }
        if size > input.limits.max_region_bytes {
            return Err(HostError::InputRejected(format!(
                "region {} is {size} bytes and the limit is {}",
                region.region, input.limits.max_region_bytes
            )));
        }
        handles.push(protocol::InputHandle {
            region: region.region,
            path: RelativePath::parse(region_file_name(region.region))
                .map_err(|err| HostError::InputRejected(err.to_string()))?,
            size,
            sha256: Sha256Digest::of(region.bytes),
            virtual_address: region.virtual_address,
            executable: region.region.is_executable(),
        });
    }
    handles.sort_by_key(|handle| handle.region);

    let output = RelativePath::parse(OUTPUT_MODEL_PATH)
        .map_err(|err| HostError::OutputHandleRejected(err.to_string()))?;
    if handles
        .iter()
        .any(|handle| handle.path.as_str() == output.as_str())
    {
        return Err(HostError::OutputHandleRejected(format!(
            "the output handle {} is also an input",
            output.as_str()
        )));
    }

    let request = AdapterRequest {
        protocol_major: PROTOCOL_MAJOR,
        model_major: MODEL_VERSION,
        compatibility: input.compatibility.clone(),
        producer: input.producer.clone(),
        identity: input.identity.clone(),
        requested_backend: input.requested_backend,
        inputs: handles,
        output,
    };
    // A request the host itself would reject is not a request an adapter should
    // get a chance to answer.
    request
        .validate()
        .map_err(|err| HostError::RequestRejected(err.to_string()))?;
    Ok(request)
}

/// The private directory tree one invocation runs in.
///
/// Cleanup is in `Drop` rather than at the end of the happy path, because the
/// paths that matter for cleanup are the ones that do not reach the end of the
/// happy path.
struct Workspace {
    dir: Option<tempfile::TempDir>,
}

impl Workspace {
    fn create() -> Result<Self, HostError> {
        // `TempDir` creates with mode 0700 already; the sub-directories are made
        // the same way rather than through the process umask.
        let dir = tempfile::Builder::new()
            .prefix("flutterdec-adapter-")
            .tempdir()
            .map_err(|err| HostError::Workspace(format!("create scratch directory: {err}")))?;
        let workspace = Self { dir: Some(dir) };
        // `tempfile` creates through the process umask, which on a default host
        // leaves the directory group and world readable. The invocation
        // directory holds the snapshot the operator handed us, so it is set
        // explicitly rather than left to whatever the umask happened to be.
        fs::set_permissions(workspace.path(), fs::Permissions::from_mode(0o700))
            .map_err(|err| HostError::Workspace(format!("seal the invocation directory: {err}")))?;
        for name in [INPUT_DIR, OUTPUT_DIR, HOME_DIR, TEMP_DIR, ARTIFACT_DIR] {
            let path = workspace.path().join(name);
            std::os::unix::fs::DirBuilderExt::mode(&mut fs::DirBuilder::new(), 0o700)
                .create(&path)
                .map_err(|err| HostError::Workspace(format!("create {}: {err}", path.display())))?;
        }
        Ok(workspace)
    }

    fn path(&self) -> &Path {
        self.dir
            .as_ref()
            .expect("workspace outlives its use")
            .path()
    }

    /// Write a file the adapter may read and must not change.
    fn write_readonly(&self, relative: &str, bytes: &[u8]) -> Result<PathBuf, HostError> {
        let path = self.path().join(relative);
        fs::write(&path, bytes)
            .map_err(|err| HostError::Workspace(format!("write {}: {err}", path.display())))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444))
            .map_err(|err| HostError::Workspace(format!("seal {}: {err}", path.display())))?;
        Ok(path)
    }
}

/// The environment one invocation gets, as the `KEY=VALUE` entries `execve`
/// takes.
///
/// An allowlist rather than a filter: `HOME`, `TMPDIR` and `PWD` point inside
/// the private workspace, and nothing else reaches the child unless it is named
/// in [`ENVIRONMENT_ALLOWLIST`].
fn child_environment(work: &Path) -> Result<Vec<CString>, HostError> {
    let mut entries = Vec::with_capacity(ENVIRONMENT_ALLOWLIST.len() + 3);
    let mut push = |name: &str, value: &OsStr| -> Result<(), HostError> {
        let mut entry = OsString::from(name);
        entry.push("=");
        entry.push(value);
        entries.push(argument(&entry, "an environment entry")?);
        Ok(())
    };
    push("HOME", work.join(HOME_DIR).as_os_str())?;
    push("TMPDIR", work.join(TEMP_DIR).as_os_str())?;
    push("PWD", work.as_os_str())?;
    for name in ENVIRONMENT_ALLOWLIST {
        if let Some(value) = std::env::var_os(name) {
            push(name, &value)?;
        }
    }
    Ok(entries)
}

/// Block between holding the verified image and creating the child, when a test
/// asked for it. See [`PRESPAWN_RENDEZVOUS_VAR`].
fn prespawn_rendezvous() -> Result<(), HostError> {
    let Some(dir) = std::env::var_os(PRESPAWN_RENDEZVOUS_VAR).filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let dir = PathBuf::from(dir);
    let ready = dir.join("ready");
    fs::write(&ready, b"ready")
        .map_err(|err| HostError::Workspace(format!("signal {}: {err}", ready.display())))?;
    let go = dir.join("go");
    let deadline = Instant::now() + RENDEZVOUS_TIMEOUT;
    while !go.exists() {
        if Instant::now() >= deadline {
            return Err(HostError::Workspace(format!(
                "the pre-spawn rendezvous at {} was never released",
                dir.display()
            )));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

/// Restore write permission everywhere before removal.
///
/// A hostile adapter that leaves an unwritable directory behind would otherwise
/// defeat `remove_dir_all` and leak its own workspace onto the host.
fn force_writable(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    if metadata.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                force_writable(&entry.path());
            }
        }
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        if let Some(dir) = self.dir.take() {
            force_writable(dir.path());
            // `TempDir::drop` would swallow a failure here; doing it explicitly
            // means the second attempt below is the one that reports nothing,
            // not the only attempt.
            let _ = fs::remove_dir_all(dir.path());
            drop(dir);
        }
    }
}

/// Run one adapter and return a model that has already been checked against the
/// host's own view of the snapshot.
///
/// Every gate runs before the child exists, the child runs inside a private
/// workspace under an explicit set of limits, and the model is validated against
/// host facts before it is handed back.
///
/// `exec_path` names the artifact to authorize, not the file that is executed.
/// The bytes it holds are read and verified once, and what runs is a descriptor
/// onto those bytes with no pathname of its own, so neither the store path nor
/// anything inside the workspace can change what executes after verification.
pub fn run_adapter(exec_path: &Path, input: &AdapterInput<'_>) -> Result<AdapterRun, HostError> {
    let Authorized { request, artifact } = authorize(input, exec_path)?;

    let workspace = Workspace::create()?;
    let work = workspace.path().to_path_buf();

    // Kept as `argv[0]`, so an interpreter, an `argv[0]` check and a diagnostic
    // all still see the artifact the registry authorized.
    let exec_name = exec_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(EXEC_FALLBACK_NAME);

    let mut host_regions: Vec<InputRegion> = Vec::with_capacity(input.regions.len());
    for region in &input.regions {
        let handle = request
            .input(region.region)
            .expect("every region has a handle");
        workspace.write_readonly(handle.path.as_str(), region.bytes)?;
        host_regions.push(InputRegion {
            region: region.region,
            size: handle.size,
            sha256: handle.sha256.clone(),
            virtual_address: region.virtual_address,
            executable: region.region.is_executable(),
        });
    }
    host_regions.sort_by_key(|region| region.region);
    workspace.write_readonly(REQUEST_PATH, &request.to_json())?;

    // `argv` and the environment are assembled here rather than on `Command`,
    // because the hook that finally execs runs between `fork` and `exec` and can
    // do nothing but hand the kernel vectors that already exist.
    let mut argv = vec![
        argument(OsStr::new(exec_name), "the adapter name")?,
        argument(OsStr::new("--request"), "an argument")?,
        argument(OsStr::new(REQUEST_PATH), "an argument")?,
        argument(OsStr::new("--result"), "an argument")?,
        argument(OsStr::new(RESULT_PATH), "an argument")?,
    ];
    if let Some(path) = input.input_path {
        argv.push(argument(OsStr::new("--input-path"), "an argument")?);
        argv.push(argument(absolute(path).as_os_str(), "the input path")?);
    }
    if let Some(source) = input.libapp {
        let path = match source {
            LibappSource::File(path) => absolute(path),
            LibappSource::Member { name, bytes } => {
                // A member name can be any depth, and only its final component
                // is meaningful to a tool that opens it.
                let file_name = Path::new(name)
                    .file_name()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("libapp.so"));
                let relative = Path::new(ARTIFACT_DIR).join(file_name);
                let relative = relative.to_string_lossy().into_owned();
                workspace.write_readonly(&relative, bytes)?;
                work.join(relative)
            }
        };
        argv.push(argument(OsStr::new("--libapp-path"), "an argument")?);
        argv.push(argument(path.as_os_str(), "the libapp path")?);
    }

    // The verified bytes become an executable inode with no name, held open for
    // the rest of the run. `Command` carries only what the standard library
    // applies before a pre-exec hook: the working directory and the streams.
    let image = ExecImage::prepare(
        exec_name,
        &artifact,
        workspace.path(),
        argv,
        child_environment(&work)?,
    )?;
    let mut command = Command::new(UNREACHABLE_PROGRAM);
    command
        .current_dir(&work)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    prespawn_rendezvous()?;
    let execution = exec::run(command, &input.limits, std::sync::Arc::new(image))?;
    let containment = execution.containment;
    match execution.completion {
        Completion::Timeout { after } => return Err(HostError::Timeout { after }),
        Completion::OutputLimit { stream, limit } => {
            return Err(HostError::OutputLimitExceeded { stream, limit })
        }
        Completion::Signalled { signal } => {
            return Err(HostError::Crashed {
                signal,
                stderr: excerpt(&execution.stderr),
            })
        }
        Completion::Exited { .. } => {}
    }

    let result_path = work.join(RESULT_PATH);
    if !result_path.exists() {
        return Err(HostError::NoResult {
            status: match execution.completion {
                Completion::Exited { code } => format!("exit code {code}"),
                _ => "terminated".to_string(),
            },
            stdout: excerpt(&execution.stdout),
            stderr: excerpt(&execution.stderr),
        });
    }

    let result_bytes = read_bounded(&result_path, "result", input.limits.max_result_bytes)?;
    let result =
        AdapterResult::from_json(&result_bytes).map_err(|err| HostError::MalformedDocument {
            document: "result".to_string(),
            detail: err.to_string(),
        })?;
    result
        .validate_against(&request)
        .map_err(|err| HostError::ResultMismatch(err.to_string()))?;

    if result.status != AdapterStatus::Ok {
        let error = result
            .error
            .as_ref()
            .expect("a non-ok result carries an error");
        return Err(HostError::AdapterFailed {
            status: result.status,
            code: error.code,
            message: excerpt(error.message.as_bytes()),
        });
    }

    let model_rel = result.model.as_ref().expect("an ok result carries a model");
    if model_rel.as_str() != request.output.as_str() {
        return Err(HostError::ModelPathMismatch {
            wrote: model_rel.as_str().to_string(),
            requested: request.output.as_str().to_string(),
        });
    }
    let model_bytes = read_bounded(
        &work.join(model_rel.as_str()),
        "model",
        input.limits.max_model_bytes,
    )?;
    let model =
        ProgramModel::from_json(&model_bytes).map_err(|err| HostError::MalformedDocument {
            document: "model".to_string(),
            detail: err.to_string(),
        })?;

    let host = HostSelectedContext {
        identity: input.identity.clone(),
        producer: input.producer.clone(),
        // An adapter run is always authorized by a record, so this arm is always
        // `Some` and a model answering with `null` is rejected as a host-fact
        // mismatch.
        compatibility: Some(input.compatibility.clone()),
        regions: host_regions,
    };
    validate::validate(&model, &host).map_err(|err| HostError::ModelRejected(err.to_string()))?;

    Ok(AdapterRun {
        model,
        resolved_backend: result
            .resolved_backend
            .expect("an ok result names its backend"),
        fallback_reason: result.fallback_reason,
        diagnostics: result.diagnostics,
        containment,
    })
}

/// An absolute path for a caller-supplied artifact.
///
/// The adapter runs with its working directory set to the private workspace, so
/// a relative path handed straight through would resolve somewhere the caller
/// did not mean.
fn absolute(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}
