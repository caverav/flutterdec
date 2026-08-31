//! Host side of the adapter boundary.
//!
//! One adapter run is one process: the host writes the snapshot regions and an
//! [`protocol::AdapterRequest`] into a scratch directory, runs the adapter
//! there, and reads back an [`protocol::AdapterResult`] plus a
//! [`model::ProgramModel`]. Nothing about the run is decided by the adapter: the
//! identity, the producer record, the compatibility binding, and the region
//! table are host facts that the model is checked against before it is returned.
//!
//! There is no v2/v3 path. [`model::ProgramModel::from_json`] rejects those
//! documents by version, so an old adapter fails loudly instead of being
//! silently reinterpreted.

pub mod model;
pub mod primitives;
pub mod protocol;
pub mod validate;
/// Host compatibility records live in the loader crate so profile and identity
/// selection cannot depend on adapter model DTOs; re-export them at the adapter
/// boundary for callers that own adapter lifecycle.
pub mod registry {
    pub use flutterdec_loader::registry::*;
}

use anyhow::{anyhow, bail, Context, Result};
use flutterdec_loader::identity::IdentityRejection;
use model::{CompatibilityBinding, InputRegion, InputRegionName, Producer, ProgramModel};
use primitives::{RelativePath, Sha256Digest};
use protocol::{AdapterRequest, AdapterResult, AdapterStatus, BackendId, RequestedBackend};
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;
use validate::HostSelectedContext;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AdapterManifest {
    pub entries: Vec<AdapterManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterManifestEntry {
    pub snapshot_hash: String,
    pub version: String,
    pub adapter: String,
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

/// Everything the host hands one adapter invocation.
///
/// The host-selected facts are here rather than derived from adapter output on
/// the way back, because a fact the adapter supplies cannot check the adapter.
#[derive(Debug, Clone)]
pub struct AdapterInput<'a> {
    /// Header-derived identity of the snapshot. Authoritative.
    pub identity: &'a flutterdec_loader::identity::SnapshotIdentity,
    /// Who the host believes is about to run, including the digest of the
    /// artifact it is about to execute.
    pub producer: Producer,
    /// The compatibility decision that authorized this run.
    pub compatibility: CompatibilityBinding,
    pub regions: Vec<AdapterRegionInput<'a>>,
    /// The original artifact, for backends that re-read it themselves.
    pub input_path: Option<&'a Path>,
    pub libapp_path: Option<&'a Path>,
    pub requested_backend: RequestedBackend,
}

/// What one adapter invocation produced, with the facts about the run that the
/// core needs and must not re-derive from the model.
#[derive(Debug, Clone)]
pub struct AdapterRun {
    pub model: ProgramModel,
    /// The backend that actually ran, as the protocol reported it.
    pub resolved_backend: BackendId,
    pub fallback_reason: Option<protocol::FallbackReason>,
    pub diagnostics: Vec<model::Diagnostic>,
}

fn manifest_path(repo_root: &Path) -> PathBuf {
    repo_root.join("adapters/manifest.json")
}

fn template_path(repo_root: &Path) -> PathBuf {
    repo_root.join("adapters/python/adapter_template.py")
}

fn installed_dir(repo_root: &Path) -> PathBuf {
    repo_root.join("adapters/installed")
}

pub fn load_manifest(repo_root: &Path) -> Result<AdapterManifest> {
    let path = manifest_path(repo_root);
    if !path.exists() {
        return Ok(AdapterManifest::default());
    }
    let bytes = fs::read(&path).with_context(|| format!("read manifest: {}", path.display()))?;
    let m =
        serde_json::from_slice::<AdapterManifest>(&bytes).context("parse adapter manifest JSON")?;
    Ok(m)
}

pub fn save_manifest(repo_root: &Path, manifest: &AdapterManifest) -> Result<()> {
    let path = manifest_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_vec_pretty(manifest)?;
    fs::write(&path, body).with_context(|| format!("write manifest: {}", path.display()))?;
    Ok(())
}

pub fn resolve_adapter_name(repo_root: &Path, dart_hash: &str) -> Result<String> {
    let m = load_manifest(repo_root)?;
    if let Some(entry) = m.entries.iter().find(|e| e.snapshot_hash == dart_hash) {
        return Ok(entry.adapter.clone());
    }
    Ok(format!("dart_adapter_{}", dart_hash))
}

pub fn install_adapter(repo_root: &Path, dart_hash: &str) -> Result<PathBuf> {
    if dart_hash.is_empty() {
        bail!("dart hash cannot be empty");
    }

    let template = template_path(repo_root);
    if !template.exists() {
        bail!("missing adapter template: {}", template.display());
    }

    let mut manifest = load_manifest(repo_root)?;
    if !manifest
        .entries
        .iter()
        .any(|e| e.snapshot_hash == dart_hash)
    {
        manifest.entries.push(AdapterManifestEntry {
            snapshot_hash: dart_hash.to_string(),
            version: "unknown".to_string(),
            adapter: format!("dart_adapter_{}", dart_hash),
        });
        save_manifest(repo_root, &manifest)?;
    }

    let name = resolve_adapter_name(repo_root, dart_hash)?;
    let out_dir = installed_dir(repo_root);
    fs::create_dir_all(&out_dir)?;
    let out = out_dir.join(name);

    let script = "#!/usr/bin/env python3\nfrom pathlib import Path\nimport sys\nroot = Path(__file__).resolve().parents[1]\nsys.path.insert(0, str(root / 'python'))\nimport adapter_template\nif __name__ == '__main__':\n    raise SystemExit(adapter_template.entrypoint())\n";

    fs::write(&out, script).with_context(|| format!("write adapter script: {}", out.display()))?;
    let mut perms = fs::metadata(&out)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&out, perms)?;

    Ok(out)
}

pub fn list_adapters(repo_root: &Path) -> Result<Vec<(AdapterManifestEntry, bool)>> {
    let m = load_manifest(repo_root)?;
    let base = installed_dir(repo_root);
    let mut out = Vec::new();
    for entry in m.entries {
        let installed = base.join(&entry.adapter).exists();
        out.push((entry, installed));
    }
    Ok(out)
}

pub fn resolve_adapter_exec(repo_root: &Path, dart_hash: &str) -> Result<PathBuf> {
    let name = resolve_adapter_name(repo_root, dart_hash)?;
    let exec = installed_dir(repo_root).join(name);
    if !exec.exists() {
        bail!(
            "adapter not installed for hash {}. run: flutterdec adapter install --dart-hash {}",
            dart_hash,
            dart_hash
        );
    }
    Ok(exec)
}

fn region_file_name(region: InputRegionName) -> &'static str {
    match region {
        InputRegionName::VmData => "vm_data.bin",
        InputRegionName::IsolateData => "isolate_data.bin",
        InputRegionName::VmInstructions => "vm_instructions.bin",
        InputRegionName::IsolateInstructions => "isolate_instructions.bin",
    }
}

const OUTPUT_MODEL_PATH: &str = "model.json";
const REQUEST_PATH: &str = "request.json";
const RESULT_PATH: &str = "result.json";

/// Wrap an identity rejection so it survives as a typed cause.
///
/// `anyhow::Error::new` keeps the `IdentityRejection` downcastable, so a caller
/// can act on *which* check refused the snapshot rather than parse a message.
pub fn identity_rejected(rejection: IdentityRejection) -> anyhow::Error {
    anyhow::Error::new(rejection).context("snapshot identity may not authorize an adapter")
}

/// Run one adapter and return a model that has already been checked against the
/// host's own view of the snapshot.
///
/// The order matters: the request is validated before the process is spawned,
/// and the model is validated before it is handed back, so neither a malformed
/// question nor a mismatched answer reaches the core.
pub fn run_adapter(exec_path: &Path, input: &AdapterInput<'_>) -> Result<AdapterRun> {
    // The gate, restated at the boundary itself. Callers gate earlier so that a
    // rejected identity never reaches a manifest or the filesystem, but this is
    // the last place a process can be spawned, and a public entry point that
    // trusts its caller to have checked is a public entry point that will one
    // day be called by a caller that did not.
    input
        .identity
        .exact_selection_key()
        .map_err(identity_rejected)?;

    let tmp = tempdir().context("create scratch directory for adapter")?;
    let work = tmp.path();

    let mut handles = Vec::with_capacity(input.regions.len());
    let mut host_regions: Vec<InputRegion> = Vec::with_capacity(input.regions.len());
    for region in &input.regions {
        let name = region_file_name(region.region);
        fs::write(work.join(name), region.bytes)
            .with_context(|| format!("write adapter input region {}", region.region))?;
        let digest = Sha256Digest::of(region.bytes);
        let size = region.bytes.len() as u64;
        handles.push(protocol::InputHandle {
            region: region.region,
            path: RelativePath::parse(name).map_err(|err| anyhow!(err))?,
            size,
            sha256: digest.clone(),
            virtual_address: region.virtual_address,
            executable: region.region.is_executable(),
        });
        host_regions.push(InputRegion {
            region: region.region,
            size,
            sha256: digest,
            virtual_address: region.virtual_address,
            executable: region.region.is_executable(),
        });
    }
    handles.sort_by_key(|h| h.region);
    host_regions.sort_by_key(|r| r.region);

    let request = AdapterRequest {
        protocol_major: protocol::PROTOCOL_MAJOR,
        model_major: model::MODEL_VERSION,
        compatibility: input.compatibility.clone(),
        producer: input.producer.clone(),
        identity: input.identity.clone(),
        requested_backend: input.requested_backend,
        inputs: handles,
        output: RelativePath::parse(OUTPUT_MODEL_PATH).map_err(|err| anyhow!(err))?,
    };
    // Fail before spawn, not after: a request the host itself would reject is
    // not a request an adapter should get a chance to answer.
    request
        .validate()
        .map_err(|err| anyhow!("adapter request is invalid: {}", err))?;
    fs::write(work.join(REQUEST_PATH), request.to_json()).context("write adapter request")?;

    let mut cmd = Command::new(exec_path);
    cmd.current_dir(work)
        .arg("--request")
        .arg(REQUEST_PATH)
        .arg("--result")
        .arg(RESULT_PATH);
    if let Some(path) = input.input_path {
        cmd.arg("--input-path").arg(absolute(path));
    }
    if let Some(path) = input.libapp_path {
        cmd.arg("--libapp-path").arg(absolute(path));
    }
    let output = cmd
        .output()
        .with_context(|| format!("launch adapter: {}", exec_path.display()))?;

    let result_path = work.join(RESULT_PATH);
    if !output.status.success() && !result_path.exists() {
        return Err(anyhow!(
            "adapter failed with status {} and wrote no result document\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let result_bytes = fs::read(&result_path)
        .with_context(|| format!("read adapter result: {}", result_path.display()))?;
    let result = AdapterResult::from_json(&result_bytes)
        .map_err(|err| anyhow!("adapter result is not protocol v1: {}", err))?;
    result
        .validate_against(&request)
        .map_err(|err| anyhow!("adapter result does not answer the request: {}", err))?;

    if result.status != AdapterStatus::Ok {
        let error = result
            .error
            .as_ref()
            .expect("a non-ok result carries an error");
        return Err(anyhow!(
            "adapter reported {:?} ({:?}): {}\nstderr:\n{}",
            result.status,
            error.code,
            error.message,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let model_rel = result.model.as_ref().expect("an ok result carries a model");
    if model_rel.as_str() != request.output.as_str() {
        bail!(
            "adapter wrote its model to {:?} instead of the requested {:?}",
            model_rel.as_str(),
            request.output.as_str()
        );
    }
    let model_bytes = fs::read(work.join(model_rel.as_str()))
        .with_context(|| format!("read adapter model: {}", model_rel.as_str()))?;
    let model = ProgramModel::from_json(&model_bytes)
        .map_err(|err| anyhow!("adapter model rejected: {}", err))?;

    let host = HostSelectedContext {
        identity: input.identity.clone(),
        producer: input.producer.clone(),
        compatibility: input.compatibility.clone(),
        regions: host_regions,
    };
    validate::validate(&model, &host)
        .map_err(|err| anyhow!("adapter model failed semantic validation: {}", err))?;

    Ok(AdapterRun {
        model,
        resolved_backend: result
            .resolved_backend
            .expect("an ok result names its backend"),
        fallback_reason: result.fallback_reason,
        diagnostics: result.diagnostics,
    })
}

/// An absolute path for a caller-supplied artifact.
///
/// The adapter runs with its working directory set to the scratch dir, so a
/// relative path handed straight through would resolve somewhere the caller did
/// not mean. Canonicalizing fails only for a path that does not exist yet, and
/// joining the current directory is still absolute.
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn install_adds_manifest_entry() {
        let td = tempdir().expect("tempdir");
        let repo = td.path();
        fs::create_dir_all(repo.join("adapters/python")).expect("mkdir");
        fs::create_dir_all(repo.join("adapters/installed")).expect("mkdir");
        fs::write(
            repo.join("adapters/python/adapter_template.py"),
            "def entrypoint(): return 0\n",
        )
        .expect("write template");

        let out = install_adapter(repo, "abcd1234").expect("install");
        assert!(out.exists());

        let manifest = load_manifest(repo).expect("load");
        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(manifest.entries[0].snapshot_hash, "abcd1234");
    }
}
