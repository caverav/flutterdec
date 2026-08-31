//! The local adapter store.
//!
//! The store is the one writable place in an installation. Package data stays
//! read-only: the compatibility registry decides *what* may be installed, and
//! this module only ever publishes bytes that already match a registry-declared
//! digest, size, host variant, and contained relative path.
//!
//! Publication is atomic in the sense that matters to a concurrent reader: each
//! file is staged under a temporary name **in its final directory** and then
//! renamed into place, so a reader sees either the old file or the new one and
//! never a half-written one. A cross-directory rename could land on another
//! filesystem and degrade to copy-then-truncate, which is exactly the partial
//! state this is meant to prevent.
//!
//! Concurrent installs are serialized by an exclusive `flock` on
//! `<store>/.lock` held across the read/decide/publish sequence. Two processes
//! installing the same adapter at once therefore produce one install and one
//! idempotent no-op, not two racing rewrites of the state file.

use flutterdec_loader::dart_profile::load_profile_artifact;
use flutterdec_loader::layout::Layout;
use flutterdec_loader::registry::{
    CompatibilityRecord, CompatibilityRegistry, HostArtifactVariant, MAX_ARTIFACT_BYTES,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::io::Read;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const STORE_VERSION: u32 = 1;
pub const STATE_FILE: &str = "store.json";
pub const LOCK_FILE: &str = ".lock";
pub const MAX_STATE_BYTES: u64 = 4 * 1024 * 1024;

const ARTIFACT_MODE: u32 = 0o755;
const STATE_MODE: u32 = 0o644;

/// Test hook: fail immediately before a named publish step.
///
/// Injected failure is the only way to prove "no partial state" for a step that
/// otherwise always succeeds, so the hook is part of the product rather than a
/// test-only build.
pub const FAIL_BEFORE_VAR: &str = "FLUTTERDEC_INSTALL_FAIL_BEFORE";

/// A publish step that can be failed on purpose through [`FAIL_BEFORE_VAR`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishStep {
    /// Before the store lock is taken.
    Lock,
    /// After validation, before any temporary file is created.
    Stage,
    /// After staging, before the artifact rename.
    PublishArtifact,
    /// After the artifact is live, before the state file rename.
    PublishState,
}

impl PublishStep {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lock => "lock",
            Self::Stage => "stage",
            Self::PublishArtifact => "publish_artifact",
            Self::PublishState => "publish_state",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        [
            Self::Lock,
            Self::Stage,
            Self::PublishArtifact,
            Self::PublishState,
        ]
        .into_iter()
        .find(|step| step.as_str() == text)
    }
}

// In-crate tests select the step through a thread-local rather than the
// environment variable, because a process-global variable set by one test is
// visible to every other test running beside it.
#[cfg(test)]
thread_local! {
    static FAIL_OVERRIDE: std::cell::Cell<Option<PublishStep>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn set_fail_before(step: Option<PublishStep>) {
    FAIL_OVERRIDE.with(|cell| cell.set(step));
}

fn requested_fail_step() -> Option<String> {
    #[cfg(test)]
    if let Some(step) = FAIL_OVERRIDE.with(|cell| cell.get()) {
        return Some(step.as_str().to_string());
    }
    std::env::var(FAIL_BEFORE_VAR)
        .ok()
        .filter(|value| !value.is_empty())
}

fn injected_failure(step: PublishStep) -> Result<(), StoreError> {
    let requested = requested_fail_step();
    let Some(requested) = requested.as_deref() else {
        return Ok(());
    };
    match PublishStep::parse(requested) {
        Some(parsed) if parsed == step => Err(StoreError::Injected(step)),
        Some(_) => Ok(()),
        None => Err(StoreError::InvalidInput(format!(
            "{FAIL_BEFORE_VAR}={requested} is not a publish step"
        ))),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    InvalidInput(String),
    /// The registry has no record for this snapshot hash.
    NoRecord(String),
    /// More than one record shares the hash, so `install` cannot pick one.
    Ambiguous(String),
    /// The record does not serve this host or this target.
    Incompatible(String),
    /// The artifact source is not a usable regular file.
    Source(String),
    /// The bytes do not match the registry-declared content address.
    DigestMismatch {
        expected: String,
        actual: String,
        expected_size: u64,
        actual_size: u64,
    },
    /// A path would leave the store, or the destination is not a regular file.
    Containment(String),
    /// The profile the record points at is missing, oversized, or corrupt.
    Profile(String),
    /// The store's own state file cannot be trusted.
    Malformed(String),
    Io(String),
    Injected(PublishStep),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(detail) => write!(f, "invalid input: {detail}"),
            Self::NoRecord(hash) => write!(
                f,
                "no compatibility record for snapshot hash {hash}; the registry is the only install authority"
            ),
            Self::Ambiguous(detail) => write!(f, "ambiguous compatibility record: {detail}"),
            Self::Incompatible(detail) => write!(f, "incompatible compatibility record: {detail}"),
            Self::Source(detail) => write!(f, "adapter artifact source rejected: {detail}"),
            Self::DigestMismatch {
                expected,
                actual,
                expected_size,
                actual_size,
            } => write!(
                f,
                "adapter artifact does not match the compatibility record: expected {expected_size} bytes with {expected}, got {actual_size} bytes with {actual}"
            ),
            Self::Containment(detail) => write!(f, "adapter store path rejected: {detail}"),
            Self::Profile(detail) => write!(f, "profile artifact rejected: {detail}"),
            Self::Malformed(detail) => write!(f, "adapter store state is unusable: {detail}"),
            Self::Io(detail) => write!(f, "adapter store I/O failed: {detail}"),
            Self::Injected(step) => write!(
                f,
                "install failed on purpose before {} ({FAIL_BEFORE_VAR})",
                step.as_str()
            ),
        }
    }
}

impl std::error::Error for StoreError {}

fn io(context: &str, err: std::io::Error) -> StoreError {
    StoreError::Io(format!("{context}: {err}"))
}

/// One installed adapter, as the store claims it.
///
/// Every field here is copied from the compatibility record that authorized the
/// install, so a later run can tell a stale install from a current one without
/// re-deriving anything from the file on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledAdapter {
    pub snapshot_hash: String,
    pub target_arch: String,
    pub host_os: String,
    pub host_arch: String,
    pub artifact_id: String,
    /// Store-relative path of the published executable.
    pub artifact_path: String,
    pub size: u64,
    pub sha256: String,
    pub parser_family_id: String,
    pub profile_id: String,
    pub profile_sha256: String,
    pub compatibility_record_sha256: String,
    pub protocol_major: u32,
    pub model_major: u32,
    /// Where the published bytes came from.
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreState {
    pub version: u32,
    #[serde(default)]
    pub adapters: Vec<InstalledAdapter>,
}

impl Default for StoreState {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            adapters: Vec::new(),
        }
    }
}

/// The verified state of one compatibility record on this host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryState {
    /// Installed, present, regular, and byte-for-byte what the record declares.
    Verified,
    /// The store claims an install whose artifact file is gone.
    Missing,
    /// The artifact is present but is not what the record declares.
    Corrupt,
    /// No artifact variant, or no supported protocol/model major, for this host.
    Incompatible,
    /// Authorized by a record but not installed.
    Unavailable,
}

impl EntryState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Missing => "missing",
            Self::Corrupt => "corrupt",
            Self::Incompatible => "incompatible",
            Self::Unavailable => "unavailable",
        }
    }

    /// Whether this state is broken store content rather than a fact about the
    /// host or an absent install.
    pub fn is_failure(self) -> bool {
        matches!(self, Self::Missing | Self::Corrupt)
    }
}

impl fmt::Display for EntryState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One row of `adapter list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoreEntry {
    pub snapshot_hash: String,
    pub state: EntryState,
    pub artifact_id: String,
    pub target_arch: String,
    pub host_os: String,
    pub host_arch: String,
    pub artifact_path: Option<String>,
    pub expected_sha256: Option<String>,
    pub expected_size: Option<u64>,
    pub profile_id: Option<String>,
    pub profile_sha256: Option<String>,
    pub compatibility_record_sha256: Option<String>,
    /// Why the state is not `verified`.
    pub detail: Option<String>,
}

/// What one `install` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Installation {
    pub record: InstalledAdapter,
    /// True when the store already held exactly this install, so nothing was
    /// written.
    pub idempotent: bool,
    pub store_dir: PathBuf,
    pub artifact_path: PathBuf,
    /// Absolute path of the verified profile the record points at.
    pub profile_path: PathBuf,
}

fn valid_snapshot_hash(hash: &str) -> bool {
    hash.len() == 32
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn digest_of(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Reject anything that is not a plain contained relative path.
///
/// Absolute paths, `..`, `.`, backslashes, and NUL all fail here rather than at
/// the filesystem, so a hostile registry cannot aim a write outside the store.
fn contained_relative(text: &str, label: &str) -> Result<PathBuf, StoreError> {
    if text.is_empty() || text.contains('\\') || text.contains('\0') {
        return Err(StoreError::Containment(format!(
            "{label} {text:?} is not a contained relative path"
        )));
    }
    let path = Path::new(text);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(StoreError::Containment(format!(
            "{label} {text:?} is not a contained relative path"
        )));
    }
    Ok(path.to_path_buf())
}

/// Read a regular file with a hard cap, refusing symlinks and non-files.
///
/// `symlink_metadata` rather than `metadata`: a symlink that happens to point at
/// a regular file is still a path whose target can be swapped between the check
/// and the read.
fn read_regular_file(path: &Path, label: &str, max_bytes: u64) -> Result<Vec<u8>, StoreError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|err| StoreError::Source(format!("read {label} {}: {err}", path.display())))?;
    if metadata.file_type().is_symlink() {
        return Err(StoreError::Source(format!(
            "{label} {} is a symbolic link",
            path.display()
        )));
    }
    if !metadata.is_file() {
        return Err(StoreError::Source(format!(
            "{label} {} is not a regular file",
            path.display()
        )));
    }
    if metadata.len() > max_bytes {
        return Err(StoreError::Source(format!(
            "{label} {} exceeds the {max_bytes} byte limit",
            path.display()
        )));
    }
    let file = fs::File::open(path)
        .map_err(|err| StoreError::Source(format!("open {label} {}: {err}", path.display())))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| StoreError::Source(format!("read {label} {}: {err}", path.display())))?;
    if bytes.len() as u64 > max_bytes {
        return Err(StoreError::Source(format!(
            "{label} {} exceeds the {max_bytes} byte limit",
            path.display()
        )));
    }
    Ok(bytes)
}

/// An exclusive lock over the whole store, released on drop.
struct StoreLock {
    file: fs::File,
}

impl StoreLock {
    fn acquire(store_dir: &Path) -> Result<Self, StoreError> {
        let path = store_dir.join(LOCK_FILE);
        let file = fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|err| io(&format!("open store lock {}", path.display()), err))?;
        // Blocking, so a concurrent install waits instead of failing. The
        // alternative, a try-lock plus retry loop, turns contention into a
        // spurious error the operator has to interpret.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc != 0 {
            return Err(io(
                &format!("lock store {}", path.display()),
                std::io::Error::last_os_error(),
            ));
        }
        Ok(Self { file })
    }
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

/// A staged temporary file that removes itself unless it is published.
struct Staged {
    path: PathBuf,
    published: bool,
}

impl Staged {
    /// Stage `bytes` in `dest`'s own directory.
    fn write(dest: &Path, bytes: &[u8], mode: u32) -> Result<Self, StoreError> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let parent = dest.parent().ok_or_else(|| {
            StoreError::Containment(format!("{} has no parent directory", dest.display()))
        })?;
        let name = dest
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                StoreError::Containment(format!("{} has no file name", dest.display()))
            })?;
        let path = parent.join(format!(
            ".{name}.tmp-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let staged = Self {
            path,
            published: false,
        };
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(mode)
            .open(&staged.path)
            .map_err(|err| io(&format!("stage {}", staged.path.display()), err))?;
        use std::io::Write;
        file.write_all(bytes)
            .map_err(|err| io(&format!("write {}", staged.path.display()), err))?;
        // Durability before visibility: a rename that beats its own data to disk
        // publishes a name with no bytes behind it after a crash.
        file.sync_all()
            .map_err(|err| io(&format!("sync {}", staged.path.display()), err))?;
        Ok(staged)
    }

    fn publish(mut self, dest: &Path) -> Result<(), StoreError> {
        fs::rename(&self.path, dest).map_err(|err| {
            io(
                &format!("publish {} as {}", self.path.display(), dest.display()),
                err,
            )
        })?;
        self.published = true;
        if let Some(parent) = dest.parent() {
            // Directory entries need their own fsync; the file's sync says
            // nothing about the name pointing at it.
            if let Ok(dir) = fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
        Ok(())
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Resolve a store-relative destination, creating its directory inside the
/// store and refusing anything that leaves the store or is not a regular file.
fn destination(store_dir: &Path, relative: &str) -> Result<PathBuf, StoreError> {
    let relative = contained_relative(relative, "artifact path")?;
    fs::create_dir_all(store_dir)
        .map_err(|err| io(&format!("create store {}", store_dir.display()), err))?;
    let canonical_store = store_dir
        .canonicalize()
        .map_err(|err| io(&format!("canonicalize store {}", store_dir.display()), err))?;
    let dest = canonical_store.join(&relative);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| io(&format!("create {}", parent.display()), err))?;
        // The directory chain is canonicalized after creation, so a component
        // that is a symbolic link out of the store is caught even though the
        // final file does not exist yet.
        let canonical_parent = parent
            .canonicalize()
            .map_err(|err| io(&format!("canonicalize {}", parent.display()), err))?;
        if !canonical_parent.starts_with(&canonical_store) {
            return Err(StoreError::Containment(format!(
                "{} escapes the adapter store {}",
                parent.display(),
                canonical_store.display()
            )));
        }
    }
    if let Ok(metadata) = fs::symlink_metadata(&dest) {
        if metadata.file_type().is_symlink() {
            return Err(StoreError::Containment(format!(
                "{} is a symbolic link",
                dest.display()
            )));
        }
        if !metadata.is_file() {
            return Err(StoreError::Containment(format!(
                "{} is not a regular file",
                dest.display()
            )));
        }
    }
    Ok(dest)
}

pub fn state_path(store_dir: &Path) -> PathBuf {
    store_dir.join(STATE_FILE)
}

/// Read the store state. An absent store is an empty store, a malformed one is
/// an error.
pub fn load_state(store_dir: &Path) -> Result<StoreState, StoreError> {
    let path = state_path(store_dir);
    match fs::symlink_metadata(&path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(StoreState::default()),
        Err(err) => return Err(io(&format!("read {}", path.display()), err)),
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(StoreError::Malformed(format!(
                    "{} is not a regular file",
                    path.display()
                )));
            }
            if metadata.len() > MAX_STATE_BYTES {
                return Err(StoreError::Malformed(format!(
                    "{} exceeds the {MAX_STATE_BYTES} byte limit",
                    path.display()
                )));
            }
        }
    }
    let bytes = fs::read(&path).map_err(|err| io(&format!("read {}", path.display()), err))?;
    let state = serde_json::from_slice::<StoreState>(&bytes)
        .map_err(|err| StoreError::Malformed(format!("parse {}: {err}", path.display())))?;
    if state.version != STORE_VERSION {
        return Err(StoreError::Malformed(format!(
            "{} declares version {} rather than {STORE_VERSION}",
            path.display(),
            state.version
        )));
    }
    Ok(state)
}

fn variant_for_host<'a>(
    record: &'a CompatibilityRecord,
    host_os: &str,
    host_arch: &str,
) -> Option<&'a HostArtifactVariant> {
    record
        .artifact
        .variants
        .iter()
        .find(|variant| variant.host_os == host_os && variant.host_arch == host_arch)
}

/// Reject a record whose wire majors this build cannot speak.
fn supported_majors(record: &CompatibilityRecord) -> Result<(), String> {
    if record.protocol_major != 1 || record.model_major != crate::model::MODEL_VERSION {
        return Err(format!(
            "record declares protocol/model majors {}/{} rather than 1/{}",
            record.protocol_major,
            record.model_major,
            crate::model::MODEL_VERSION
        ));
    }
    Ok(())
}

/// Select the single record that authorizes installing `hash`.
///
/// `install` has no snapshot in scope, so it cannot resolve a feature tuple.
/// More than one record for a hash is therefore refused rather than guessed at.
fn select_record<'a>(
    registry: &'a CompatibilityRegistry,
    hash: &str,
    target_arch: Option<&str>,
) -> Result<&'a CompatibilityRecord, StoreError> {
    let candidates = registry
        .records
        .iter()
        .filter(|record| record.snapshot_hash == hash)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(StoreError::NoRecord(hash.to_string()));
    }
    let candidates = match target_arch {
        Some(requested) => {
            let filtered = candidates
                .iter()
                .copied()
                .filter(|record| record.target_arch.as_str() == requested)
                .collect::<Vec<_>>();
            if filtered.is_empty() {
                return Err(StoreError::Incompatible(format!(
                    "no record for snapshot hash {hash} targets {requested}"
                )));
            }
            filtered
        }
        None => candidates,
    };
    if candidates.len() > 1 {
        let fingerprints = candidates
            .iter()
            .map(|record| record.feature_fingerprint.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(StoreError::Ambiguous(format!(
            "{} records share snapshot hash {hash} (feature fingerprints {fingerprints})",
            candidates.len()
        )));
    }
    Ok(candidates[0])
}

/// Verify the profile the record points at, in the read-only data directory.
fn verify_profile(layout: &Layout, record: &CompatibilityRecord) -> Result<PathBuf, StoreError> {
    let relative = contained_relative(&record.profile.path, "profile path")?;
    let path = layout.data_dir().join(&relative);
    load_profile_artifact(
        &path,
        &record.profile.id,
        &record.profile.sha256,
        record.sdk_aliases.clone(),
    )
    .map_err(StoreError::Profile)?;
    Ok(path)
}

/// Install the adapter authorized for `hash` into the layout's store.
///
/// `source` overrides the artifact bytes; the default is the checked-in producer
/// in the read-only data directory. Either way the bytes must match the record's
/// declared digest and size, so an operator cannot install something the
/// registry did not authorize.
pub fn install(
    layout: &Layout,
    registry: &CompatibilityRegistry,
    hash: &str,
    target_arch: Option<&str>,
    source: Option<&Path>,
) -> Result<Installation, StoreError> {
    if !valid_snapshot_hash(hash) {
        return Err(StoreError::InvalidInput(format!(
            "snapshot hash {hash:?} is not 32 lowercase hexadecimal characters"
        )));
    }
    let record = select_record(registry, hash, target_arch)?;
    supported_majors(record).map_err(StoreError::Incompatible)?;

    let host_os = std::env::consts::OS;
    let host_arch = std::env::consts::ARCH;
    let variant = variant_for_host(record, host_os, host_arch).ok_or_else(|| {
        StoreError::Incompatible(format!(
            "record for snapshot hash {hash} has no artifact variant for host {host_os}/{host_arch}"
        ))
    })?;

    let profile_path = verify_profile(layout, record)?;
    let record_sha256 = record
        .sha256()
        .map_err(|err| StoreError::Malformed(err.to_string()))?;

    let (bytes, source_label) = match source {
        Some(path) => (
            read_regular_file(path, "adapter artifact source", MAX_ARTIFACT_BYTES)?,
            format!("operator:{}", path.display()),
        ),
        None => (
            read_regular_file(
                &layout.producer_path(),
                "packaged producer",
                MAX_ARTIFACT_BYTES,
            )?,
            "packaged-producer".to_string(),
        ),
    };
    let actual = digest_of(&bytes);
    if actual != variant.sha256 || bytes.len() as u64 != variant.size {
        return Err(StoreError::DigestMismatch {
            expected: variant.sha256.clone(),
            actual,
            expected_size: variant.size,
            actual_size: bytes.len() as u64,
        });
    }

    let installed = InstalledAdapter {
        snapshot_hash: hash.to_string(),
        target_arch: record.target_arch.as_str().to_string(),
        host_os: host_os.to_string(),
        host_arch: host_arch.to_string(),
        artifact_id: record.artifact.id.clone(),
        artifact_path: variant.path.clone(),
        size: variant.size,
        sha256: variant.sha256.clone(),
        parser_family_id: record.parser_family.id.clone(),
        profile_id: record.profile.id.clone(),
        profile_sha256: record.profile.sha256.clone(),
        compatibility_record_sha256: record_sha256,
        protocol_major: record.protocol_major,
        model_major: record.model_major,
        source: source_label,
    };

    injected_failure(PublishStep::Lock)?;
    fs::create_dir_all(layout.store_dir()).map_err(|err| {
        io(
            &format!("create store {}", layout.store_dir().display()),
            err,
        )
    })?;
    let _lock = StoreLock::acquire(layout.store_dir())?;

    let dest = destination(layout.store_dir(), &variant.path)?;
    let state_dest = destination(layout.store_dir(), STATE_FILE)?;
    let mut state = load_state(layout.store_dir())?;

    // Idempotence is decided under the lock, against both the state entry and
    // the bytes on disk, so "already installed" cannot be claimed for a record
    // whose artifact was deleted or edited after the fact.
    let existing = state.adapters.iter().position(|entry| {
        entry.snapshot_hash == installed.snapshot_hash
            && entry.host_os == installed.host_os
            && entry.host_arch == installed.host_arch
    });
    if let Some(index) = existing {
        if state.adapters[index] == installed && artifact_matches(&dest, &installed).is_ok() {
            return Ok(Installation {
                record: installed,
                idempotent: true,
                store_dir: layout.store_dir().to_path_buf(),
                artifact_path: dest,
                profile_path,
            });
        }
    }

    injected_failure(PublishStep::Stage)?;
    // Captured before anything is replaced. If the state file cannot be
    // published, the artifact is put back exactly as it was: a live artifact no
    // state file mentions is precisely the partial state this must not leave.
    let previous_artifact = match fs::read(&dest) {
        Ok(bytes) => Some(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(io(&format!("read {}", dest.display()), err)),
    };
    let staged_artifact = Staged::write(&dest, &bytes, ARTIFACT_MODE)?;
    match existing {
        Some(index) => state.adapters[index] = installed.clone(),
        None => state.adapters.push(installed.clone()),
    }
    state.adapters.sort_by(|left, right| {
        (&left.snapshot_hash, &left.host_os, &left.host_arch).cmp(&(
            &right.snapshot_hash,
            &right.host_os,
            &right.host_arch,
        ))
    });
    let mut state_bytes = serde_json::to_vec_pretty(&state)
        .map_err(|err| StoreError::Malformed(format!("serialize store state: {err}")))?;
    state_bytes.push(b'\n');
    let staged_state = Staged::write(&state_dest, &state_bytes, STATE_MODE)?;

    injected_failure(PublishStep::PublishArtifact)?;
    staged_artifact.publish(&dest)?;
    if let Err(err) = injected_failure(PublishStep::PublishState)
        .and_then(move |()| staged_state.publish(&state_dest))
    {
        return Err(
            match restore_artifact(&dest, previous_artifact.as_deref()) {
                Ok(()) => err,
                Err(rollback) => StoreError::Io(format!(
                    "{err}; and restoring {} failed: {rollback}",
                    dest.display()
                )),
            },
        );
    }

    Ok(Installation {
        record: installed,
        idempotent: false,
        store_dir: layout.store_dir().to_path_buf(),
        artifact_path: dest,
        profile_path,
    })
}

/// Undo a published artifact, back to absent or back to its previous bytes.
fn restore_artifact(dest: &Path, previous: Option<&[u8]>) -> Result<(), StoreError> {
    match previous {
        Some(bytes) => Staged::write(dest, bytes, ARTIFACT_MODE)?.publish(dest),
        None => fs::remove_file(dest).map_err(|err| io(&format!("remove {}", dest.display()), err)),
    }
}

/// Compare the file at `path` against what the store claims about it.
fn artifact_matches(path: &Path, installed: &InstalledAdapter) -> Result<(), (EntryState, String)> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err((
                EntryState::Missing,
                format!("{} is registered but absent", path.display()),
            ))
        }
        Err(err) => {
            return Err((
                EntryState::Corrupt,
                format!("read {}: {err}", path.display()),
            ))
        }
    };
    if metadata.file_type().is_symlink() {
        return Err((
            EntryState::Corrupt,
            format!("{} is a symbolic link", path.display()),
        ));
    }
    if !metadata.is_file() {
        return Err((
            EntryState::Corrupt,
            format!("{} is not a regular file", path.display()),
        ));
    }
    if metadata.len() != installed.size {
        return Err((
            EntryState::Corrupt,
            format!(
                "{} is {} bytes, expected {}",
                path.display(),
                metadata.len(),
                installed.size
            ),
        ));
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err((
            EntryState::Corrupt,
            format!("{} is not executable", path.display()),
        ));
    }
    let bytes = match read_regular_file(path, "installed adapter", MAX_ARTIFACT_BYTES) {
        Ok(bytes) => bytes,
        Err(err) => return Err((EntryState::Corrupt, err.to_string())),
    };
    let actual = digest_of(&bytes);
    if actual != installed.sha256 {
        return Err((
            EntryState::Corrupt,
            format!(
                "{} has SHA-256 {actual}, expected {}",
                path.display(),
                installed.sha256
            ),
        ));
    }
    Ok(())
}

/// The state of every record the registry authorizes, plus any installed
/// adapter the registry no longer authorizes.
///
/// File existence is never the answer: a `verified` row means the bytes were
/// read and hashed against the record that authorized them.
pub fn inspect(
    layout: &Layout,
    registry: &CompatibilityRegistry,
) -> Result<Vec<StoreEntry>, StoreError> {
    let state = load_state(layout.store_dir())?;
    let host_os = std::env::consts::OS;
    let host_arch = std::env::consts::ARCH;
    let mut rows = Vec::new();
    // Computed before the record walk rather than inside it: a record that
    // turns out to be incompatible still accounts for its own store entry, and
    // reporting that entry twice would read as two separate problems.
    let claimed = state
        .adapters
        .iter()
        .filter(|entry| {
            entry.host_os == host_os
                && entry.host_arch == host_arch
                && registry
                    .records
                    .iter()
                    .any(|record| record.snapshot_hash == entry.snapshot_hash)
        })
        .map(|entry| {
            (
                entry.snapshot_hash.clone(),
                entry.host_os.clone(),
                entry.host_arch.clone(),
            )
        })
        .collect::<Vec<_>>();

    for record in &registry.records {
        let mut row = StoreEntry {
            snapshot_hash: record.snapshot_hash.clone(),
            state: EntryState::Unavailable,
            artifact_id: record.artifact.id.clone(),
            target_arch: record.target_arch.as_str().to_string(),
            host_os: host_os.to_string(),
            host_arch: host_arch.to_string(),
            artifact_path: None,
            expected_sha256: None,
            expected_size: None,
            profile_id: Some(record.profile.id.clone()),
            profile_sha256: Some(record.profile.sha256.clone()),
            compatibility_record_sha256: record.sha256().ok(),
            detail: None,
        };
        if let Err(detail) = supported_majors(record) {
            row.state = EntryState::Incompatible;
            row.detail = Some(detail);
            rows.push(row);
            continue;
        }
        let Some(variant) = variant_for_host(record, host_os, host_arch) else {
            row.state = EntryState::Incompatible;
            row.detail = Some(format!(
                "no artifact variant for host {host_os}/{host_arch}"
            ));
            rows.push(row);
            continue;
        };
        row.artifact_path = Some(variant.path.clone());
        row.expected_sha256 = Some(variant.sha256.clone());
        row.expected_size = Some(variant.size);

        let installed = state.adapters.iter().find(|entry| {
            entry.snapshot_hash == record.snapshot_hash
                && entry.host_os == host_os
                && entry.host_arch == host_arch
        });
        let Some(installed) = installed else {
            row.detail = Some("not installed in the local adapter store".to_string());
            rows.push(row);
            continue;
        };
        if installed.sha256 != variant.sha256
            || installed.size != variant.size
            || installed.artifact_path != variant.path
        {
            row.state = EntryState::Corrupt;
            row.detail = Some(format!(
                "installed record claims {} bytes with {} at {}, the compatibility record declares {} bytes with {} at {}",
                installed.size,
                installed.sha256,
                installed.artifact_path,
                variant.size,
                variant.sha256,
                variant.path
            ));
            rows.push(row);
            continue;
        }
        let path = layout.store_dir().join(&installed.artifact_path);
        match artifact_matches(&path, installed) {
            Ok(()) => row.state = EntryState::Verified,
            Err((state, detail)) => {
                row.state = state;
                row.detail = Some(detail);
            }
        }
        rows.push(row);
    }

    // An install the registry no longer authorizes is reported rather than
    // hidden: the registry is the authority, so the store entry is the thing
    // that is wrong.
    for installed in &state.adapters {
        let key = (
            installed.snapshot_hash.clone(),
            installed.host_os.clone(),
            installed.host_arch.clone(),
        );
        if claimed.contains(&key) {
            continue;
        }
        if registry
            .records
            .iter()
            .any(|record| record.snapshot_hash == installed.snapshot_hash)
            && (installed.host_os != host_os || installed.host_arch != host_arch)
        {
            // Installed for a different host. Not this host's problem.
            continue;
        }
        rows.push(StoreEntry {
            snapshot_hash: installed.snapshot_hash.clone(),
            state: EntryState::Incompatible,
            artifact_id: installed.artifact_id.clone(),
            target_arch: installed.target_arch.clone(),
            host_os: installed.host_os.clone(),
            host_arch: installed.host_arch.clone(),
            artifact_path: Some(installed.artifact_path.clone()),
            expected_sha256: Some(installed.sha256.clone()),
            expected_size: Some(installed.size),
            profile_id: Some(installed.profile_id.clone()),
            profile_sha256: Some(installed.profile_sha256.clone()),
            compatibility_record_sha256: Some(installed.compatibility_record_sha256.clone()),
            detail: Some("no compatibility record authorizes this installed adapter".to_string()),
        });
    }

    rows.sort_by(|left, right| {
        (&left.snapshot_hash, &left.host_os, &left.host_arch).cmp(&(
            &right.snapshot_hash,
            &right.host_os,
            &right.host_arch,
        ))
    });
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flutterdec_loader::identity::{SnapshotKind, TargetArch};
    use flutterdec_loader::registry::{
        canonical_feature_fingerprint, ArtifactReference, CompatibilityEvidence,
        CompatibilityRecord, CompatibilityRegistry, ParserFamilyReference, ProfileReference,
        TrustTier, REGISTRY_VERSION,
    };
    use tempfile::TempDir;

    const HASH: &str = "80a49c7111088100a233b2ae788e1f48";
    const PRODUCER: &str = "#!/bin/sh\nexit 0\n";

    /// A packaged data directory plus an empty store, both in one temp tree.
    struct Fixture {
        _dir: TempDir,
        layout: Layout,
        registry: CompatibilityRegistry,
    }

    fn profile_json() -> String {
        serde_json::to_string_pretty(&serde_json::json!({
            "profiles": {
                "test-profile": {
                    "tag_style": "CID_INT32",
                    "compressed_word_size": 4,
                    "header_fields": 5,
                    "max_alignment": 16,
                    "heap_object_tag": 1,
                    "cids": {"class": 1, "object_pool": 23}
                }
            }
        }))
        .expect("profile json")
    }

    /// `variant` decides the store-relative artifact path, which is what the
    /// traversal and symlink cases need to control.
    fn fixture(artifact_relative: &str, host_os: &str, host_arch: &str) -> Fixture {
        let dir = TempDir::new().expect("tempdir");
        let data = dir.path().join("share/flutterdec");
        let store = dir.path().join("store");
        fs::create_dir_all(data.join("data")).expect("mkdir data");
        fs::create_dir_all(data.join("adapters/python")).expect("mkdir adapters");
        let profile = profile_json();
        fs::write(data.join("data/test-profile.json"), &profile).expect("write profile");
        fs::write(data.join(PRODUCER_RELATIVE), PRODUCER).expect("write producer");

        let features = ["android", "arm64", "compressed-pointers", "product"]
            .into_iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let record = CompatibilityRecord {
            snapshot_hash: HASH.to_string(),
            snapshot_kind: SnapshotKind::FullAot,
            target_arch: TargetArch::Arm64,
            feature_fingerprint: canonical_feature_fingerprint(&features),
            features,
            known_features: Vec::new(),
            forbidden_features: Vec::new(),
            sdk_aliases: Vec::new(),
            parser_family: ParserFamilyReference {
                id: "fixture-family".to_string(),
                version: Some("1".to_string()),
                sha256: None,
            },
            profile: ProfileReference {
                id: "test-profile".to_string(),
                path: "data/test-profile.json".to_string(),
                sha256: digest_of(profile.as_bytes()),
            },
            artifact: ArtifactReference {
                id: "fixture-artifact".to_string(),
                variants: vec![HostArtifactVariant {
                    host_os: host_os.to_string(),
                    host_arch: host_arch.to_string(),
                    path: artifact_relative.to_string(),
                    size: PRODUCER.len() as u64,
                    sha256: digest_of(PRODUCER.as_bytes()),
                    provenance: "fixture".to_string(),
                }],
            },
            evidence: CompatibilityEvidence {
                source: "fixture".to_string(),
                provenance: "unit test".to_string(),
                references: Vec::new(),
            },
            trust_tier: TrustTier::Experimental,
            protocol_major: 1,
            model_major: crate::model::MODEL_VERSION,
        };
        let registry = CompatibilityRegistry {
            version: REGISTRY_VERSION,
            records: vec![record],
        };
        let layout = Layout::new(data, store, dir.path().join("symbols"));
        Fixture {
            _dir: dir,
            layout,
            registry,
        }
    }

    const PRODUCER_RELATIVE: &str = "adapters/python/adapter_template.py";

    fn host() -> (&'static str, &'static str) {
        (std::env::consts::OS, std::env::consts::ARCH)
    }

    fn default_fixture() -> Fixture {
        let (os, arch) = host();
        fixture("artifacts/dart_adapter", os, arch)
    }

    fn install_default(fixture: &Fixture) -> Result<Installation, StoreError> {
        install(&fixture.layout, &fixture.registry, HASH, None, None)
    }

    /// Nothing of the install exists: no artifact, no state entry, no leftover
    /// temporary file anywhere under the store.
    fn assert_store_is_untouched(store: &Path) {
        let state = state_path(store);
        assert!(!state.exists(), "{} exists", state.display());
        let mut leftovers = Vec::new();
        let mut stack = vec![store.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.file_name().and_then(|name| name.to_str()) == Some(LOCK_FILE) {
                    continue;
                }
                leftovers.push(path);
            }
        }
        assert!(
            leftovers.is_empty(),
            "store holds files after a failed install: {leftovers:?}"
        );
    }

    #[test]
    fn install_publishes_a_verified_artifact_and_is_idempotent() {
        let fixture = default_fixture();
        let first = install_default(&fixture).expect("install");
        assert!(!first.idempotent);
        assert_eq!(
            fs::read(&first.artifact_path).expect("read"),
            PRODUCER.as_bytes()
        );
        let mode = fs::metadata(&first.artifact_path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, ARTIFACT_MODE, "artifact is not executable");

        let state = load_state(fixture.layout.store_dir()).expect("state");
        assert_eq!(state.adapters.len(), 1);
        assert_eq!(state.adapters[0].sha256, digest_of(PRODUCER.as_bytes()));

        let before = fs::read(state_path(fixture.layout.store_dir())).expect("state bytes");
        let second = install_default(&fixture).expect("reinstall");
        assert!(second.idempotent, "a repeated install rewrote the store");
        assert_eq!(second.record, first.record);
        assert_eq!(
            fs::read(state_path(fixture.layout.store_dir())).expect("state bytes"),
            before
        );

        let rows = inspect(&fixture.layout, &fixture.registry).expect("inspect");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, EntryState::Verified);
    }

    #[test]
    fn a_source_that_does_not_match_the_record_publishes_nothing() {
        let fixture = default_fixture();
        let other = fixture.layout.data_dir().join("other.sh");
        fs::write(&other, "#!/bin/sh\nexit 1\n").expect("write other");
        let err = install(&fixture.layout, &fixture.registry, HASH, None, Some(&other))
            .expect_err("wrong bytes cannot be installed");
        assert!(matches!(err, StoreError::DigestMismatch { .. }), "{err}");
        assert_store_is_untouched(fixture.layout.store_dir());
    }

    #[test]
    fn a_source_that_is_not_a_regular_file_is_refused() {
        let fixture = default_fixture();
        let dir = fixture.layout.data_dir().join("a_directory");
        fs::create_dir_all(&dir).expect("mkdir");
        let err = install(&fixture.layout, &fixture.registry, HASH, None, Some(&dir))
            .expect_err("a directory is not an artifact");
        assert!(matches!(err, StoreError::Source(_)), "{err}");

        let link = fixture.layout.data_dir().join("a_link");
        std::os::unix::fs::symlink(fixture.layout.producer_path(), &link).expect("symlink");
        let err = install(&fixture.layout, &fixture.registry, HASH, None, Some(&link))
            .expect_err("a symbolic link is not an artifact");
        assert!(matches!(err, StoreError::Source(_)), "{err}");
        assert_store_is_untouched(fixture.layout.store_dir());
    }

    #[test]
    fn a_record_path_that_leaves_the_store_is_refused() {
        for relative in [
            "../escape",
            "/etc/escape",
            "artifacts/../../escape",
            "./escape",
        ] {
            let (os, arch) = host();
            let fixture = fixture(relative, os, arch);
            match install_default(&fixture) {
                Err(StoreError::Containment(_)) => {}
                other => panic!("{relative:?} was not refused: {other:?}"),
            }
            assert_store_is_untouched(fixture.layout.store_dir());
        }
    }

    #[test]
    fn a_store_directory_that_is_a_symlink_out_of_the_store_is_refused() {
        let fixture = default_fixture();
        let outside = fixture.layout.data_dir().parent().unwrap().join("outside");
        fs::create_dir_all(&outside).expect("mkdir outside");
        fs::create_dir_all(fixture.layout.store_dir()).expect("mkdir store");
        std::os::unix::fs::symlink(&outside, fixture.layout.store_dir().join("artifacts"))
            .expect("symlink artifacts");

        let err = install_default(&fixture).expect_err("a symlinked store directory escapes");
        assert!(matches!(err, StoreError::Containment(_)), "{err}");
        assert!(
            fs::read_dir(&outside)
                .expect("read outside")
                .next()
                .is_none(),
            "the install wrote outside the store"
        );
    }

    #[test]
    fn a_host_the_record_does_not_serve_is_refused() {
        let fixture = fixture("artifacts/dart_adapter", "plan9", "vax");
        let err = install_default(&fixture).expect_err("no variant for this host");
        assert!(matches!(err, StoreError::Incompatible(_)), "{err}");
        assert_store_is_untouched(fixture.layout.store_dir());

        let rows = inspect(&fixture.layout, &fixture.registry).expect("inspect");
        assert_eq!(rows[0].state, EntryState::Incompatible);
    }

    #[test]
    fn a_target_the_record_does_not_serve_is_refused() {
        let fixture = default_fixture();
        let err = install(&fixture.layout, &fixture.registry, HASH, Some("x64"), None)
            .expect_err("the record targets arm64");
        assert!(matches!(err, StoreError::Incompatible(_)), "{err}");
        assert_store_is_untouched(fixture.layout.store_dir());
    }

    #[test]
    fn an_unregistered_or_malformed_hash_is_refused() {
        let fixture = default_fixture();
        for hash in ["", "ZZZ", "80A49C7111088100A233B2AE788E1F48", "80a49c71"] {
            let err = install(&fixture.layout, &fixture.registry, hash, None, None)
                .expect_err("bad hash syntax");
            assert!(matches!(err, StoreError::InvalidInput(_)), "{hash}: {err}");
        }
        let err = install(
            &fixture.layout,
            &fixture.registry,
            "00000000000000000000000000000000",
            None,
            None,
        )
        .expect_err("no record");
        assert!(matches!(err, StoreError::NoRecord(_)), "{err}");
        assert_store_is_untouched(fixture.layout.store_dir());
    }

    /// Every publish step, failed on purpose, leaves the store as it was.
    #[test]
    fn an_injected_failure_leaves_no_partial_state() {
        for step in [
            PublishStep::Lock,
            PublishStep::Stage,
            PublishStep::PublishArtifact,
            PublishStep::PublishState,
        ] {
            let fixture = default_fixture();
            set_fail_before(Some(step));
            let result = install_default(&fixture);
            set_fail_before(None);
            let err = result.expect_err("the injected failure did not fail");
            assert_eq!(err, StoreError::Injected(step));
            assert_store_is_untouched(fixture.layout.store_dir());
        }
    }

    /// A file with the right name is not an install. This is the distinction the
    /// old existence check could not make.
    #[test]
    fn inspect_never_treats_existence_as_installation() {
        let fixture = default_fixture();
        let path = fixture.layout.store_dir().join("artifacts/dart_adapter");
        fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        fs::write(&path, PRODUCER).expect("write imposter");

        let rows = inspect(&fixture.layout, &fixture.registry).expect("inspect");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].state,
            EntryState::Unavailable,
            "a file with the right name was reported as installed"
        );
    }

    #[test]
    fn inspect_separates_a_missing_artifact_from_a_corrupt_one() {
        let fixture = default_fixture();
        let installed = install_default(&fixture).expect("install");

        let mut edited = PRODUCER.as_bytes().to_vec();
        let last = edited.len() - 2;
        edited[last] = b'1';
        fs::write(&installed.artifact_path, &edited).expect("edit artifact");
        let rows = inspect(&fixture.layout, &fixture.registry).expect("inspect");
        assert_eq!(rows[0].state, EntryState::Corrupt);
        assert!(rows[0].detail.as_deref().unwrap().contains("SHA-256"));

        fs::remove_file(&installed.artifact_path).expect("remove artifact");
        let rows = inspect(&fixture.layout, &fixture.registry).expect("inspect");
        assert_eq!(rows[0].state, EntryState::Missing);
    }

    #[test]
    fn a_malformed_state_file_is_an_error_rather_than_an_empty_store() {
        let fixture = default_fixture();
        fs::create_dir_all(fixture.layout.store_dir()).expect("mkdir store");
        fs::write(state_path(fixture.layout.store_dir()), "{ not json").expect("write state");
        let err = inspect(&fixture.layout, &fixture.registry).expect_err("malformed state");
        assert!(matches!(err, StoreError::Malformed(_)), "{err}");
    }

    /// The profile is verified in the read-only data directory, so a record
    /// pointing at a digest that no longer matches cannot be installed.
    #[test]
    fn a_profile_that_does_not_match_its_digest_is_refused() {
        let fixture = default_fixture();
        fs::write(
            fixture.layout.data_dir().join("data/test-profile.json"),
            "{\"profiles\": {}}",
        )
        .expect("rewrite profile");
        let err = install_default(&fixture).expect_err("profile digest changed");
        assert!(matches!(err, StoreError::Profile(_)), "{err}");
        assert_store_is_untouched(fixture.layout.store_dir());
    }
}
