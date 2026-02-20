use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryInfo {
    pub id: u64,
    pub uri: String,
    pub name_display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassInfo {
    pub id: u64,
    pub name: String,
    #[serde(rename = "super")]
    pub super_name: String,
    #[serde(rename = "lib")]
    pub library_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
    pub id: u64,
    pub name: String,
    pub owner_class: String,
    pub entry_va: u64,
    pub size: u64,
    pub code_section_va: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectPoolEntry {
    pub index: u64,
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramModel {
    pub schema_version: u32,
    pub adapter_kind: String,
    pub dart_version: String,
    pub snapshot_hash: String,
    pub arch: String,
    pub libraries: Vec<LibraryInfo>,
    pub classes: Vec<ClassInfo>,
    pub functions: Vec<FunctionInfo>,
    pub object_pool: Vec<ObjectPoolEntry>,
}

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

#[derive(Debug, Clone)]
pub struct AdapterInput<'a> {
    pub vm_data: &'a [u8],
    pub isolate_data: &'a [u8],
    pub vm_instr: &'a [u8],
    pub isolate_instr: &'a [u8],
    pub vm_instr_va: u64,
    pub isolate_instr_va: u64,
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

    let script = format!(
        "#!/usr/bin/env python3\nfrom pathlib import Path\nimport sys\nroot = Path(__file__).resolve().parents[1]\nsys.path.insert(0, str(root / 'python'))\nimport adapter_template\nif __name__ == '__main__':\n    raise SystemExit(adapter_template.entrypoint(default_snapshot_hash={:?}, default_version='unknown'))\n",
        dart_hash
    );

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

fn validate_model(model: &ProgramModel) -> Result<()> {
    if model.schema_version != 2 {
        bail!(
            "unsupported adapter schema version {}",
            model.schema_version
        );
    }
    if model.arch != "arm64" {
        bail!("adapter returned unsupported arch {}", model.arch);
    }
    if model.functions.is_empty() {
        bail!("adapter returned no functions");
    }
    Ok(())
}

pub fn run_adapter(exec_path: &Path, input: &AdapterInput<'_>) -> Result<ProgramModel> {
    let tmp = tempdir().context("create tempdir for adapter")?;

    let vm_data = tmp.path().join("vm_data.bin");
    let iso_data = tmp.path().join("iso_data.bin");
    let vm_instr = tmp.path().join("vm_instr.bin");
    let iso_instr = tmp.path().join("iso_instr.bin");
    let out_json = tmp.path().join("model.json");

    fs::File::create(&vm_data)?.write_all(input.vm_data)?;
    fs::File::create(&iso_data)?.write_all(input.isolate_data)?;
    fs::File::create(&vm_instr)?.write_all(input.vm_instr)?;
    fs::File::create(&iso_instr)?.write_all(input.isolate_instr)?;

    let output = Command::new(exec_path)
        .arg("--vm-data")
        .arg(&vm_data)
        .arg("--isolate-data")
        .arg(&iso_data)
        .arg("--vm-instr")
        .arg(&vm_instr)
        .arg("--isolate-instr")
        .arg(&iso_instr)
        .arg("--vm-instr-va")
        .arg(input.vm_instr_va.to_string())
        .arg("--isolate-instr-va")
        .arg(input.isolate_instr_va.to_string())
        .arg("--out")
        .arg(&out_json)
        .output()
        .with_context(|| format!("launch adapter: {}", exec_path.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(anyhow!(
            "adapter failed with status {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            stdout,
            stderr
        ));
    }

    let bytes = fs::read(&out_json)
        .with_context(|| format!("read adapter output: {}", out_json.display()))?;
    let model = serde_json::from_slice::<ProgramModel>(&bytes).context("parse adapter output")?;
    validate_model(&model)?;
    Ok(model)
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
            "def entrypoint(default_snapshot_hash='x', default_version='unknown'): return 0\n",
        )
        .expect("write template");

        let out = install_adapter(repo, "abcd1234").expect("install");
        assert!(out.exists());

        let manifest = load_manifest(repo).expect("load");
        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(manifest.entries[0].snapshot_hash, "abcd1234");
    }
}
