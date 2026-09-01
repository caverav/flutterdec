//! The shared rig every CLI integration test drives the product through.
//!
//! Every case built here runs the built `flutterdec` binary from a temporary
//! *package prefix* (`bin/flutterdec` plus `share/flutterdec/...`), with a
//! cleared environment, an isolated `HOME`, and a current directory that is not
//! a checkout and contains nothing at all. That is deliberate: install,
//! listing, and discovery all used to depend on the current directory sitting
//! inside a source tree, so a test that runs from the repository root cannot
//! tell a fix from the old behavior.
//!
//! The fixture registry and profile are written here as fresh JSON rather than
//! built from the crate's own types, and the fixture producer is a real
//! executable script whose digest the fixture registry content-addresses, so
//! the digest, host, and containment checks are exercised against bytes rather
//! than against a mock.
#![allow(dead_code)]

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

pub const HASH: &str = "80a49c7111088100a233b2ae788e1f48";
pub const OTHER_HASH: &str = "ace654289f5abc240509fc941453ebc5";
pub const FEATURES: &str = "product arm64 android compressed-pointers";
pub const ARTIFACT_RELATIVE: &str = "artifacts/dart_adapter";

pub fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// SHA-256 of every regular file under `root`, keyed by relative path.
///
/// Used to assert that a directory was not written to, which is stronger than
/// checking a modification time and does not depend on filesystem timestamp
/// granularity.
pub fn tree_digests(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let meta = match fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(_) => continue,
            };
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            let key = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            let bytes = fs::read(&path).unwrap_or_default();
            out.insert(key, digest(&bytes));
        }
    }
    out
}

/// Files under the store that are neither the lock nor part of a finished
/// install. A staged temporary left behind is a partial-state failure.
pub fn store_files(store: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
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
            if path.file_name().and_then(|name| name.to_str()) == Some(".lock") {
                continue;
            }
            out.push(path);
        }
    }
    out.sort();
    out
}

pub fn checkout_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize checkout root")
}

/// A temporary release-style package prefix and an isolated home.
pub struct Prefix {
    dir: TempDir,
    /// `dir.path()` with every platform symlink resolved. See `root`.
    root: PathBuf,
    /// Absolute path baked into the fixture producer, touched when it runs.
    pub marker: PathBuf,
}

impl Prefix {
    pub fn new() -> Self {
        Self::with_variant(
            ARTIFACT_RELATIVE,
            std::env::consts::OS,
            std::env::consts::ARCH,
        )
    }

    /// `variant_path`, `host_os` and `host_arch` are what the fixture registry
    /// declares, which is how the containment and host cases are set up.
    pub fn with_variant(variant_path: &str, host_os: &str, host_arch: &str) -> Self {
        Self::build(variant_path, host_os, host_arch, None)
    }

    /// A prefix whose packaged producer answers the request instead of only
    /// proving it ran.
    ///
    /// Needed wherever the assertion is about what a *completed* run reports:
    /// a producer that exits without a model never gets as far as a model, a
    /// containment report, or a `report.json`.
    pub fn answering() -> Self {
        Self::with_producer(&answering_producer())
    }

    /// A prefix carrying an arbitrary packaged producer.
    ///
    /// `__SPAWN_LOG__` in the source is replaced with an absolute path this
    /// prefix owns, so every fixture producer records the fact that it ran. That
    /// count is the evidence behind "no adapter was executed": an assertion
    /// about an absent side effect is only as good as the proof the side effect
    /// would have appeared.
    pub fn with_producer(source: &str) -> Self {
        Self::build(
            ARTIFACT_RELATIVE,
            std::env::consts::OS,
            std::env::consts::ARCH,
            Some(source),
        )
    }

    /// Where each spawn appended a line, and how many did.
    pub fn spawn_log(&self) -> PathBuf {
        self.root().join("producer_spawns.log")
    }

    pub fn spawns(&self) -> usize {
        fs::read_to_string(self.spawn_log())
            .map(|text| text.lines().filter(|line| !line.is_empty()).count())
            .unwrap_or(0)
    }

    pub fn build(
        variant_path: &str,
        host_os: &str,
        host_arch: &str,
        producer_source: Option<&str>,
    ) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let root = fs::canonicalize(dir.path()).expect("resolve the prefix root");
        let root = root.as_path();
        let marker = root.join("producer_ran.marker");
        fs::create_dir_all(root.join("bin")).expect("mkdir bin");
        fs::create_dir_all(root.join("home")).expect("mkdir home");
        fs::create_dir_all(root.join("cwd")).expect("mkdir cwd");
        fs::create_dir_all(root.join("share/flutterdec/adapters/python")).expect("mkdir python");
        fs::create_dir_all(root.join("share/flutterdec/data")).expect("mkdir data");

        // Only release-distributed files are copied in: the binary and the
        // package data. Nothing from the checkout is linked or referenced.
        fs::copy(
            env!("CARGO_BIN_EXE_flutterdec"),
            root.join("bin/flutterdec"),
        )
        .expect("copy release binary");

        let producer = match producer_source {
            Some(source) => source.replace(
                "__SPAWN_LOG__",
                root.join("producer_spawns.log")
                    .to_str()
                    .expect("spawn log path"),
            ),
            None => format!(
                "#!/bin/sh\ntouch '{}'\necho spawn >> '{}'\nexit 3\n",
                marker.display(),
                root.join("producer_spawns.log").display()
            ),
        };
        let producer_path = root.join("share/flutterdec/adapters/python/adapter_template.py");
        fs::write(&producer_path, &producer).expect("write producer");
        let mut perms = fs::metadata(&producer_path)
            .expect("metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&producer_path, perms).expect("chmod producer");

        let profile = serde_json::to_vec_pretty(&serde_json::json!({
            "profiles": {
                "fixture-profile": {
                    "tag_style": "CID_INT32",
                    "compressed_word_size": 4,
                    "header_fields": 5,
                    "max_alignment": 16,
                    "heap_object_tag": 1,
                    "cids": {"class": 1, "object_pool": 23}
                }
            }
        }))
        .expect("serialize profile");
        fs::write(
            root.join("share/flutterdec/data/fixture-profile.json"),
            &profile,
        )
        .expect("write profile");

        let registry = serde_json::json!({
            "version": 1,
            "records": [record_json(
                HASH,
                variant_path,
                host_os,
                host_arch,
                producer.as_bytes(),
                &profile,
            )]
        });
        fs::write(
            root.join("share/flutterdec/adapters/registry.json"),
            serde_json::to_vec_pretty(&registry).expect("serialize registry"),
        )
        .expect("write registry");

        let root = root.to_path_buf();
        Self { dir, root, marker }
    }

    /// The prefix root, as the product will spell it back.
    ///
    /// The temporary directory can sit behind a platform symlink — Darwin hands
    /// out `/var/folders/...` for a tree that really lives at
    /// `/private/var/folders/...` — and anything the product canonicalizes comes
    /// back in the resolved spelling. Resolving here once means every path this
    /// rig derives is comparable to what the product reports, rather than every
    /// assertion having to resolve for itself.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn share(&self) -> PathBuf {
        self.root().join("share/flutterdec")
    }

    /// The store the default discovery rule lands on for this isolated home.
    pub fn store(&self) -> PathBuf {
        self.root().join("home/.local/share/flutterdec/adapters")
    }

    pub fn artifact(&self) -> PathBuf {
        self.store().join(ARTIFACT_RELATIVE)
    }

    pub fn producer(&self) -> PathBuf {
        self.share().join("adapters/python/adapter_template.py")
    }

    /// What the store holds after one successful install and nothing else.
    pub fn settled_store_files(&self) -> Vec<PathBuf> {
        let mut files = vec![self.artifact(), self.store().join("store.json")];
        files.sort();
        files
    }

    /// A run of the packaged binary from an unrelated working directory.
    pub fn cmd(&self) -> Command {
        let mut cmd = Command::new(self.root().join("bin/flutterdec"));
        cmd.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.root().join("home"))
            .current_dir(self.root().join("cwd"));
        cmd
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.run_with(&[], args)
    }

    pub fn run_with(&self, env: &[(&str, &str)], args: &[&str]) -> Output {
        let mut cmd = self.cmd();
        for (key, value) in env {
            cmd.env(key, value);
        }
        cmd.args(args);
        run(&mut cmd)
    }

    pub fn install(&self) -> Output {
        self.run(&["adapter", "install", "--dart-hash", HASH, "--json"])
    }

    pub fn list(&self) -> Output {
        self.run(&["adapter", "list", "--json"])
    }
}

/// One compatibility record as JSON, content-addressing `producer` and
/// `profile` exactly as a real registry does.
pub fn record_json(
    hash: &str,
    variant_path: &str,
    host_os: &str,
    host_arch: &str,
    producer: &[u8],
    profile: &[u8],
) -> Value {
    let features = ["android", "arm64", "compressed-pointers", "product"];
    let mut hasher = Sha256::new();
    hasher.update(features.join("\n").as_bytes());
    let fingerprint = format!("{:x}", hasher.finalize());
    serde_json::json!({
        "snapshot_hash": hash,
        "snapshot_kind": "full_aot",
        "target_arch": "arm64",
        "features": features,
        "feature_fingerprint": fingerprint,
        "known_features": features,
        "forbidden_features": ["no-compressed-pointers"],
        "sdk_aliases": [],
        "parser_family": {"id": "fixture-family", "version": "1", "sha256": null},
        "profile": {
            "id": "fixture-profile",
            "path": "data/fixture-profile.json",
            "sha256": digest(profile)
        },
        "artifact": {
            "id": "fixture-artifact",
            "variants": [{
                "host_os": host_os,
                "host_arch": host_arch,
                "path": variant_path,
                "size": producer.len(),
                "sha256": digest(producer),
                "provenance": "integration fixture"
            }]
        },
        "evidence": {"source": "fixture", "provenance": "integration test", "references": []},
        "trust_tier": "experimental",
        "protocol_major": 1,
        "model_major": 4
    })
}

/// Run a command, retrying while a freshly copied binary is still reported busy.
///
/// Tests run as parallel threads in one process, and a thread that forks while
/// another thread is writing a file can leave the kernel's deny-write count on
/// that inode raised for a moment, which surfaces as `ETXTBSY` from `exec`.
/// That is a property of the harness, not of the binary under test.
pub fn run(cmd: &mut Command) -> Output {
    for _ in 0..200 {
        match cmd.output() {
            Ok(output) => return output,
            Err(err) if err.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            Err(err) => panic!("run {cmd:?}: {err}"),
        }
    }
    panic!("{cmd:?} stayed busy")
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

pub fn json(output: &Output) -> Value {
    serde_json::from_str(&stdout(output))
        .unwrap_or_else(|err| panic!("stdout is not JSON ({err}): {}", stdout(output)))
}

pub fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

pub fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

/// A minimal ARM64 `libapp.so` carrying a FullAOT snapshot header.
///
/// One `PT_LOAD` at address zero, so a symbol's virtual address equals its file
/// offset, plus the four `_kDart*` symbols the loader looks for. The snapshot
/// header layout is `runtime/vm/snapshot.h`: magic, `int64` length, `int64`
/// kind, then the 32-character hash and the NUL-terminated features string.
pub fn synthetic_libapp(hash: &str, features: &str) -> Vec<u8> {
    const EHDR: usize = 64;
    const PHDR: usize = 56;
    const SHDR: usize = 64;
    const SYM: usize = 24;
    const RET: [u8; 4] = 0xD65F_03C0u32.to_le_bytes();

    let mut vm_data = vec![0u8; 8];
    vm_data.extend_from_slice(&[0xf5, 0xf5, 0xdc, 0xdc]);
    let payload = 20 + hash.len() + features.len() + 1;
    vm_data.extend_from_slice(&(payload as i64).to_le_bytes());
    vm_data.extend_from_slice(&3i64.to_le_bytes()); // kFullAOT
    vm_data.extend_from_slice(hash.as_bytes());
    vm_data.extend_from_slice(features.as_bytes());
    vm_data.push(0);

    let mut out = vec![0u8; 128];
    let place = |out: &mut Vec<u8>, bytes: &[u8]| -> (u64, u64) {
        let at = out.len() as u64;
        out.extend_from_slice(bytes);
        (at, bytes.len() as u64)
    };
    let spans = [
        place(&mut out, &vm_data),
        place(&mut out, &[0u8; 32]),
        place(&mut out, &RET),
        place(&mut out, &RET.repeat(4)),
    ];

    let mut strtab = vec![0u8];
    let mut name_offsets = Vec::new();
    for name in [
        "_kDartVmSnapshotData",
        "_kDartIsolateSnapshotData",
        "_kDartVmSnapshotInstructions",
        "_kDartIsolateSnapshotInstructions",
    ] {
        name_offsets.push(strtab.len() as u32);
        strtab.extend_from_slice(name.as_bytes());
        strtab.push(0);
    }

    let mut symtab = vec![0u8; SYM];
    for (index, (value, size)) in spans.iter().enumerate() {
        symtab.extend_from_slice(&name_offsets[index].to_le_bytes());
        symtab.push(0x11); // STB_GLOBAL | STT_OBJECT
        symtab.push(0);
        symtab.extend_from_slice(&1u16.to_le_bytes());
        symtab.extend_from_slice(&value.to_le_bytes());
        symtab.extend_from_slice(&size.to_le_bytes());
    }

    let mut shstrtab = vec![0u8];
    let section_name = |shstrtab: &mut Vec<u8>, name: &str| -> u32 {
        let at = shstrtab.len() as u32;
        shstrtab.extend_from_slice(name.as_bytes());
        shstrtab.push(0);
        at
    };
    let symtab_name = section_name(&mut shstrtab, ".symtab");
    let strtab_name = section_name(&mut shstrtab, ".strtab");
    let shstrtab_name = section_name(&mut shstrtab, ".shstrtab");

    let symtab_off = out.len() as u64;
    out.extend_from_slice(&symtab);
    let strtab_off = out.len() as u64;
    out.extend_from_slice(&strtab);
    let shstrtab_off = out.len() as u64;
    out.extend_from_slice(&shstrtab);
    let shoff = out.len() as u64;

    let mut section = |name: u32, kind: u32, offset: u64, size: u64, link: u32, entsize: u64| {
        let mut hdr = Vec::with_capacity(SHDR);
        hdr.extend_from_slice(&name.to_le_bytes());
        hdr.extend_from_slice(&kind.to_le_bytes());
        hdr.extend_from_slice(&0u64.to_le_bytes());
        hdr.extend_from_slice(&0u64.to_le_bytes());
        hdr.extend_from_slice(&offset.to_le_bytes());
        hdr.extend_from_slice(&size.to_le_bytes());
        hdr.extend_from_slice(&link.to_le_bytes());
        hdr.extend_from_slice(&0u32.to_le_bytes());
        hdr.extend_from_slice(&1u64.to_le_bytes());
        hdr.extend_from_slice(&entsize.to_le_bytes());
        out.extend_from_slice(&hdr);
    };
    section(0, 0, 0, 0, 0, 0);
    section(
        symtab_name,
        2,
        symtab_off,
        symtab.len() as u64,
        2,
        SYM as u64,
    );
    section(strtab_name, 3, strtab_off, strtab.len() as u64, 0, 0);
    section(shstrtab_name, 3, shstrtab_off, shstrtab.len() as u64, 0, 0);

    let total = out.len() as u64;

    let mut header = Vec::with_capacity(EHDR);
    header.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0]);
    header.extend_from_slice(&[0u8; 8]);
    header.extend_from_slice(&3u16.to_le_bytes()); // ET_DYN
    header.extend_from_slice(&183u16.to_le_bytes()); // EM_AARCH64
    header.extend_from_slice(&1u32.to_le_bytes());
    header.extend_from_slice(&0u64.to_le_bytes());
    header.extend_from_slice(&(EHDR as u64).to_le_bytes());
    header.extend_from_slice(&shoff.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    header.extend_from_slice(&(EHDR as u16).to_le_bytes());
    header.extend_from_slice(&(PHDR as u16).to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&(SHDR as u16).to_le_bytes());
    header.extend_from_slice(&4u16.to_le_bytes());
    header.extend_from_slice(&3u16.to_le_bytes());
    out[..EHDR].copy_from_slice(&header);

    let mut phdr = Vec::with_capacity(PHDR);
    phdr.extend_from_slice(&1u32.to_le_bytes()); // PT_LOAD
    phdr.extend_from_slice(&5u32.to_le_bytes()); // R+X
    phdr.extend_from_slice(&0u64.to_le_bytes());
    phdr.extend_from_slice(&0u64.to_le_bytes());
    phdr.extend_from_slice(&0u64.to_le_bytes());
    phdr.extend_from_slice(&total.to_le_bytes());
    phdr.extend_from_slice(&total.to_le_bytes());
    phdr.extend_from_slice(&0x1000u64.to_le_bytes());
    out[EHDR..EHDR + PHDR].copy_from_slice(&phdr);

    out
}

/// The interpreter this test process can see, as an absolute path.
///
/// The CLI runs with `PATH=/usr/bin:/bin`, and the adapter host passes that
/// `PATH` through, so a `/usr/bin/env` shebang would resolve against a
/// deliberately bare search path. Baking the interpreter in keeps the fixture
/// independent of what the packaged prefix happens to have on `PATH`.
pub fn interpreter() -> PathBuf {
    let path = std::env::var_os("PATH").expect("PATH");
    std::env::split_paths(&path)
        .map(|dir| dir.join("python3"))
        .find(|candidate| candidate.is_file())
        .expect("a python3 on the test process PATH")
}

/// A packaged producer that answers correctly and recovers nothing.
///
/// Every host-selected fact is echoed out of the request rather than restated,
/// because a fixture that restates them is a fixture that can disagree with the
/// host for reasons that have nothing to do with what is under test.
pub fn answering_producer() -> String {
    format!(
        r#"#!{}
import argparse, json, pathlib

pathlib.Path("__SPAWN_LOG__").open("a").write("spawn\n")

DOMAINS = [
    "libraries", "classes", "class_relationships", "functions",
    "function_names", "object_pool", "pool_index_space",
]

p = argparse.ArgumentParser()
p.add_argument("--request", required=True)
p.add_argument("--result", required=True)
p.add_argument("--input-path")
p.add_argument("--libapp-path")
args = p.parse_args()
request = json.loads(pathlib.Path(args.request).read_text())
code_region = next(
    handle for handle in request["inputs"] if handle["region"] == "isolate_instructions"
)

model = {{
    "model_version": 4,
    "producer": request["producer"],
    "input": {{
        "identity": request["identity"],
        "regions": [
            {{
                "region": handle["region"],
                "size": handle["size"],
                "sha256": handle["sha256"],
                "virtual_address": handle["virtual_address"],
                "executable": handle["executable"],
            }}
            for handle in request["inputs"]
        ],
    }},
    "compatibility": request["compatibility"],
    "capabilities": dict(
        {{domain: "unavailable" for domain in DOMAINS}}, functions="partial"
    ),
    "libraries": [],
    "classes": [],
    # One unnamed heuristic code range. A model with no functions at all makes
    # `decompile` stop before it writes a report, and the report is the point.
    "functions": [
        {{
            "id": 1,
            "name": None,
            "owner": None,
            "code": {{
                "start_va": code_region["virtual_address"],
                "size": code_region["size"],
            }},
            "code_section_va": code_region["virtual_address"],
            "provenance": "heuristic",
        }}
    ],
    "object_pool": {{"index_space": "ordinal", "geometry": None, "entries": []}},
    "diagnostics": [
        {{
            "code": (
                "domain_heuristic_only"
                if domain == "functions"
                else "domain_not_recovered"
            ),
            "severity": "warning",
            "subject": domain,
            "message": "the packaging fixture parses nothing",
        }}
        for domain in DOMAINS
    ],
    "extensions": {{}},
}}
pathlib.Path(request["output"]).write_text(json.dumps(model))
pathlib.Path(args.result).write_text(json.dumps({{
    "protocol_major": 1,
    "model_major": 4,
    "status": "ok",
    "model": request["output"],
    "error": None,
    "resolved_backend": "internal",
    "fallback_reason": None,
    "diagnostics": [],
}}))
"#,
        interpreter().display()
    )
}

/// Every containment control the host names, so the assertion cannot silently
/// stop covering one that was added later.
pub const CONTROLS: &[&str] = &[
    "wall_clock_deadline",
    "process_group",
    "descriptor_isolation",
    "cpu_seconds",
    "file_size",
    "address_space",
    "process_count",
    "descriptors",
    "network",
    "stdout_bytes",
    "stderr_bytes",
    "model_bytes",
];

/// The same report with the one number in it that is not about this product
/// blanked out.
///
/// The process budget is the host's own task count plus an allowance, so two
/// runs of the binary see different budgets whenever anything else on the
/// machine starts or exits between them. Comparing two runs is a claim about
/// what the product established, not about how busy the machine was, so the
/// budget's value is replaced and everything else — including whether the
/// budget was established at all — is compared exactly.
pub fn comparable_across_runs(report: &Value) -> Value {
    match report {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    if key == "process_count" && value.get("limit").is_some() {
                        let mut control = value.as_object().expect("a control object").clone();
                        control.insert("limit".into(), Value::from("a snapshot of the host"));
                        return (key.clone(), Value::Object(control));
                    }
                    (key.clone(), comparable_across_runs(value))
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(comparable_across_runs).collect()),
        other => other.clone(),
    }
}

pub fn assert_controls_are_accurate(containment: &Value, source: &str) {
    let object = containment
        .as_object()
        .unwrap_or_else(|| panic!("{source} has no containment object: {containment}"));
    for control in CONTROLS {
        let state = object
            .get(*control)
            .unwrap_or_else(|| panic!("{source} does not report {control}: {containment}"));
        match state["state"].as_str() {
            Some("applied") => {}
            Some("unavailable") => assert!(
                state["reason"]
                    .as_str()
                    .is_some_and(|r| !r.trim().is_empty()),
                "{source} reports {control} unavailable without a reason: {state}"
            ),
            other => panic!("{source} reports {control} as {other:?}: {state}"),
        }
    }
    assert!(
        object["process_tree_terminated"].is_boolean(),
        "{source} does not say whether the host had to end the run: {containment}"
    );
    assert_eq!(
        object.len(),
        CONTROLS.len() + 1,
        "{source} reports controls this test does not check: {containment}"
    );

    // Host-side bounds and POSIX controls hold everywhere this crate builds.
    for control in [
        "wall_clock_deadline",
        "stdout_bytes",
        "stderr_bytes",
        "model_bytes",
        "process_group",
        "descriptor_isolation",
        "cpu_seconds",
        "file_size",
        "descriptors",
    ] {
        assert_eq!(
            object[control]["state"], "applied",
            "{source} could not establish {control}: {}",
            object[control]
        );
    }

    if cfg!(target_os = "linux") {
        for control in ["address_space", "process_count"] {
            assert_eq!(
                object[control]["state"], "applied",
                "{source} did not establish {control} on linux: {}",
                object[control]
            );
        }
    } else {
        for control in ["address_space", "process_count", "network"] {
            assert_eq!(
                object[control]["state"], "unavailable",
                "{source} claimed {control} on a platform that cannot establish it: {}",
                object[control]
            );
        }
    }
}

/// A producer that never finishes, so the host's deadline is what ends it.
///
/// It logs the spawn before sleeping, so the timeout case can still prove a
/// child existed. Without that the test could not tell "the deadline killed it"
/// from "nothing ever ran".
pub fn sleeping_producer() -> String {
    format!(
        r#"#!{}
import pathlib, time

pathlib.Path("__SPAWN_LOG__").open("a").write("spawn\n")
time.sleep(3600)
"#,
        interpreter().display()
    )
}

/// A producer that reports success and writes a model that is not JSON.
///
/// The result document is well-formed, so nothing before the model read can
/// catch this: the host has to reject the model itself.
pub fn corrupt_model_producer() -> String {
    format!(
        r#"#!{}
import argparse, json, pathlib

pathlib.Path("__SPAWN_LOG__").open("a").write("spawn\n")
p = argparse.ArgumentParser()
p.add_argument("--request", required=True)
p.add_argument("--result", required=True)
p.add_argument("--input-path")
p.add_argument("--libapp-path")
args = p.parse_args()
request = json.loads(pathlib.Path(args.request).read_text())
pathlib.Path(request["output"]).write_text("{{not json at all")
pathlib.Path(args.result).write_text(json.dumps({{
    "protocol_major": 1,
    "model_major": 4,
    "status": "ok",
    "model": request["output"],
    "error": None,
    "resolved_backend": "internal",
    "fallback_reason": None,
    "diagnostics": [],
}}))
"#,
        interpreter().display()
    )
}

/// A producer that answers correctly about a snapshot it was not given.
///
/// Every other field is echoed from the request, so the only thing wrong with
/// the model is the identity: the host's own fact, restated differently.
pub fn wrong_identity_producer() -> String {
    answering_producer().replace(
        r#""identity": request["identity"],"#,
        r#""identity": dict(request["identity"], hash="ffffffffffffffffffffffffffffffff"),"#,
    )
}
