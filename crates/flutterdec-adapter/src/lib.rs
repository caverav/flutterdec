use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
#[cfg(unix)]
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
    #[serde(default)]
    pub name_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectPoolEntry {
    pub index: u64,
    pub kind: String,
    pub value: String,
    #[serde(default)]
    pub decoded_kind: Option<String>,
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub target_va: Option<u64>,
    #[serde(default)]
    pub owner_class: Option<String>,
    #[serde(default)]
    pub library_uri: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub source: Option<String>,
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
    pub input_path: Option<&'a Path>,
    pub libapp_path: Option<&'a Path>,
    pub vm_data: &'a [u8],
    pub isolate_data: &'a [u8],
    pub vm_instr: &'a [u8],
    pub isolate_instr: &'a [u8],
    pub vm_instr_va: u64,
    pub isolate_instr_va: u64,
    pub backend: Option<&'a str>,
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
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&out)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&out, perms)?;
    }

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
    if model.schema_version != 2 && model.schema_version != 3 {
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

    let mut cmd = Command::new(exec_path);
    cmd.arg("--vm-data")
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
        .arg(&out_json);
    if let Some(path) = input.input_path {
        cmd.arg("--input-path").arg(path);
    }
    if let Some(path) = input.libapp_path {
        cmd.arg("--libapp-path").arg(path);
    }
    if let Some(backend) = input.backend {
        cmd.env("FLUTTERDEC_ADAPTER_BACKEND", backend);
    }
    let output = cmd
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

    #[test]
    fn run_adapter_blutter_backend_synthesizes_entrypoint_candidate() {
        let td = tempdir().expect("tempdir");
        let root = td.path();
        let python_dir = root.join("python");
        fs::create_dir_all(&python_dir).expect("mkdir python");

        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("canonicalize repo root");
        let template_src = repo_root.join("adapters/python/adapter_template.py");
        fs::copy(&template_src, python_dir.join("adapter_template.py"))
            .expect("copy adapter template");

        let fake_blutter = root.join("fake_blutter");
        fs::write(
            &fake_blutter,
            r#"#!/usr/bin/env python3
from pathlib import Path
import sys

if len(sys.argv) < 3:
    raise SystemExit("usage: fake_blutter <input> <outdir>")
out_dir = Path(sys.argv[2])
asm_dir = out_dir / "asm"
asm_dir.mkdir(parents=True, exist_ok=True)
(asm_dir / "main.dart").write_text(
    "// lib: 0, url: package:app/main.dart\n"
    "  dynamic main() {\n"
    "// ** addr: 0x1000, size: 0x10\n"
    "}\n",
    encoding="utf-8",
)
(asm_dir / "router.dart").write_text(
    "// lib: 1, url: package:app/router.dart\n"
    "class RouterHost {\n"
    "  dynamic onNewIntent() {\n"
    "// ** addr: 0x1010, size: 0x10\n"
    "  }\n"
    "}\n",
    encoding="utf-8",
)
(asm_dir / "subject.dart").write_text(
    "// lib: 2, url: package:app/state/subject.dart\n"
    "class Subject {\n"
    "  dynamic onResume() {\n"
    "// ** addr: 0x1020, size: 0x10\n"
    "  }\n"
    "}\n",
    encoding="utf-8",
)
(out_dir / "pp.txt").write_text("", encoding="utf-8")
"#,
        )
        .expect("write fake blutter");
        let mut fake_perms = fs::metadata(&fake_blutter).expect("metadata").permissions();
        fake_perms.set_mode(0o755);
        fs::set_permissions(&fake_blutter, fake_perms).expect("chmod fake blutter");

        let exec = root.join("adapter_exec.py");
        fs::write(
            &exec,
            "#!/usr/bin/env python3\nfrom pathlib import Path\nimport os\nimport sys\nroot = Path(__file__).resolve().parent\nos.environ['FLUTTERDEC_BLUTTER_CMD'] = str(root / 'fake_blutter')\nsys.path.insert(0, str(root / 'python'))\nimport adapter_template\nif __name__ == '__main__':\n    raise SystemExit(adapter_template.entrypoint(default_snapshot_hash='testhash', default_version='unknown'))\n",
        )
        .expect("write exec");
        let mut perms = fs::metadata(&exec).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&exec, perms).expect("chmod exec");

        let input_dir = root.join("input");
        fs::create_dir_all(&input_dir).expect("mkdir input");
        let input_file = input_dir.join("app.apk");
        fs::write(&input_file, b"dummy").expect("write dummy input");

        let vm_data = vec![0u8; 64];
        let iso_data = vec![0u8; 64];
        let vm_instr = vec![0u8; 16];
        let iso_instr = vec![0u8; 16];
        let input = AdapterInput {
            input_path: Some(&input_file),
            libapp_path: None,
            vm_data: &vm_data,
            isolate_data: &iso_data,
            vm_instr: &vm_instr,
            isolate_instr: &iso_instr,
            vm_instr_va: 0,
            isolate_instr_va: 0,
            backend: Some("blutter"),
        };

        let model = run_adapter(&exec, &input).expect("run adapter");
        assert!(model.functions.iter().any(|f| f.entry_va == 0x1000));
        let entrypoint = model
            .object_pool
            .iter()
            .find(|e| e.decoded_kind.as_deref() == Some("EntryPointCandidate"))
            .expect("entrypoint candidate");
        assert_eq!(entrypoint.selector.as_deref(), Some("main"));
        assert_eq!(entrypoint.target_va, Some(0x1000));
        let deeplink = model
            .object_pool
            .iter()
            .find(|e| e.decoded_kind.as_deref() == Some("DeepLinkHandlerCandidate"))
            .expect("deeplink candidate");
        assert_eq!(deeplink.selector.as_deref(), Some("onNewIntent"));
        assert_eq!(deeplink.target_va, Some(0x1010));
        let activity_targets = model
            .object_pool
            .iter()
            .filter(|e| e.decoded_kind.as_deref() == Some("ActivityHandlerCandidate"))
            .map(|e| e.target_va.unwrap_or_default())
            .collect::<Vec<_>>();
        assert!(
            activity_targets.contains(&0x1010),
            "expected onNewIntent to be tagged as activity handler"
        );
        assert!(
            !activity_targets.contains(&0x1020),
            "generic onResume in non-activity owner should not be tagged as activity handler"
        );
    }
}
