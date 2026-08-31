//! The adapter store, driven through the real CLI.
//!
//! Every case here runs the built `flutterdec` binary from a temporary
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

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const HASH: &str = "80a49c7111088100a233b2ae788e1f48";
const OTHER_HASH: &str = "ace654289f5abc240509fc941453ebc5";
const FEATURES: &str = "product arm64 android compressed-pointers";
const ARTIFACT_RELATIVE: &str = "artifacts/dart_adapter";

fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// SHA-256 of every regular file under `root`, keyed by relative path.
///
/// Used to assert that a directory was not written to, which is stronger than
/// checking a modification time and does not depend on filesystem timestamp
/// granularity.
fn tree_digests(root: &Path) -> BTreeMap<String, String> {
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
fn store_files(store: &Path) -> Vec<PathBuf> {
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

fn checkout_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize checkout root")
}

/// A temporary release-style package prefix and an isolated home.
struct Prefix {
    dir: TempDir,
    /// Absolute path baked into the fixture producer, touched when it runs.
    marker: PathBuf,
}

impl Prefix {
    fn new() -> Self {
        Self::with_variant(
            ARTIFACT_RELATIVE,
            std::env::consts::OS,
            std::env::consts::ARCH,
        )
    }

    /// `variant_path`, `host_os` and `host_arch` are what the fixture registry
    /// declares, which is how the containment and host cases are set up.
    fn with_variant(variant_path: &str, host_os: &str, host_arch: &str) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let root = dir.path();
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

        let producer = format!("#!/bin/sh\ntouch '{}'\nexit 3\n", marker.display());
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

        Self { dir, marker }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn share(&self) -> PathBuf {
        self.root().join("share/flutterdec")
    }

    /// The store the default discovery rule lands on for this isolated home.
    fn store(&self) -> PathBuf {
        self.root().join("home/.local/share/flutterdec/adapters")
    }

    fn artifact(&self) -> PathBuf {
        self.store().join(ARTIFACT_RELATIVE)
    }

    fn producer(&self) -> PathBuf {
        self.share().join("adapters/python/adapter_template.py")
    }

    /// What the store holds after one successful install and nothing else.
    fn settled_store_files(&self) -> Vec<PathBuf> {
        let mut files = vec![self.artifact(), self.store().join("store.json")];
        files.sort();
        files
    }

    /// A run of the packaged binary from an unrelated working directory.
    fn cmd(&self) -> Command {
        let mut cmd = Command::new(self.root().join("bin/flutterdec"));
        cmd.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.root().join("home"))
            .current_dir(self.root().join("cwd"));
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with(&[], args)
    }

    fn run_with(&self, env: &[(&str, &str)], args: &[&str]) -> Output {
        let mut cmd = self.cmd();
        for (key, value) in env {
            cmd.env(key, value);
        }
        cmd.args(args);
        run(&mut cmd)
    }

    fn install(&self) -> Output {
        self.run(&["adapter", "install", "--dart-hash", HASH, "--json"])
    }

    fn list(&self) -> Output {
        self.run(&["adapter", "list", "--json"])
    }
}

/// One compatibility record as JSON, content-addressing `producer` and
/// `profile` exactly as a real registry does.
fn record_json(
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
fn run(cmd: &mut Command) -> Output {
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

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn json(output: &Output) -> Value {
    serde_json::from_str(&stdout(output))
        .unwrap_or_else(|err| panic!("stdout is not JSON ({err}): {}", stdout(output)))
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

#[test]
fn installs_and_lists_from_a_packaged_prefix_with_no_checkout_in_sight() {
    let prefix = Prefix::new();
    let checkout = checkout_root();
    let checkout_before = tree_digests(&checkout.join("adapters"));
    let data_before = tree_digests(&prefix.share());

    let install = prefix.install();
    assert_eq!(code(&install), 0, "install failed: {}", stderr(&install));
    let report = json(&install);
    assert_eq!(report["idempotent"], Value::Bool(false));
    assert_eq!(
        report["store_dir"].as_str().map(PathBuf::from),
        Some(prefix.store()),
        "the store is not the documented default under HOME"
    );
    assert_eq!(
        report["artifact_path"].as_str().map(PathBuf::from),
        Some(prefix.artifact())
    );
    assert_eq!(report["record"]["snapshot_hash"], text(HASH));
    assert_eq!(report["record"]["target_arch"], text("arm64"));
    assert_eq!(report["record"]["host_os"], text(std::env::consts::OS));
    assert_eq!(report["record"]["protocol_major"], Value::from(1));
    assert_eq!(report["record"]["model_major"], Value::from(4));
    assert_eq!(report["record"]["profile_id"], text("fixture-profile"));
    assert_eq!(
        report["profile_path"].as_str().map(PathBuf::from),
        Some(prefix.share().join("data/fixture-profile.json")),
        "the profile was not resolved inside the package prefix"
    );

    let artifact = prefix.artifact();
    let bytes = fs::read(&artifact).expect("read installed artifact");
    assert_eq!(
        report["record"]["sha256"],
        text(&digest(&bytes)),
        "the reported digest is not the digest of the installed bytes"
    );
    assert_eq!(report["record"]["size"], Value::from(bytes.len()));
    let mode = fs::metadata(&artifact)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(
        mode & 0o777,
        0o755,
        "the installed artifact is not executable"
    );

    let list = prefix.list();
    assert_eq!(code(&list), 0, "list failed: {}", stderr(&list));
    let rows = json(&list);
    assert_eq!(rows.as_array().map(Vec::len), Some(1));
    assert_eq!(rows[0]["state"], text("verified"));
    assert_eq!(rows[0]["snapshot_hash"], text(HASH));
    assert_eq!(rows[0]["detail"], Value::Null);

    // Read-only package data is never mutated, and the source checkout is
    // neither required nor written: the fixture profile id does not exist in the
    // repository registry, so a run that reached the checkout could not have
    // produced this report.
    assert_eq!(data_before, tree_digests(&prefix.share()));
    assert_eq!(checkout_before, tree_digests(&checkout.join("adapters")));
    assert!(
        !checkout.join("adapters/installed").exists(),
        "the install wrote into the source checkout"
    );
    assert!(
        !checkout.join("adapters/manifest.json").exists(),
        "the source checkout still carries an adapter manifest"
    );
}

#[test]
fn a_repeated_install_changes_nothing() {
    let prefix = Prefix::new();
    assert_eq!(code(&prefix.install()), 0);
    let state = prefix.store().join("store.json");
    let before = fs::read(&state).expect("read state");
    let files_before = store_files(&prefix.store());

    let second = prefix.install();
    assert_eq!(code(&second), 0, "{}", stderr(&second));
    assert_eq!(
        json(&second)["idempotent"],
        Value::Bool(true),
        "a repeated install did not report an idempotent result"
    );
    assert_eq!(before, fs::read(&state).expect("read state"));
    assert_eq!(files_before, store_files(&prefix.store()));
}

/// Eight real processes racing on one store. The lock is what makes exactly one
/// of them the installer; without it two readers can both decide the store is
/// empty and both write it.
#[test]
fn concurrent_installs_produce_exactly_one_install() {
    let prefix = Prefix::new();
    let mut children = Vec::new();
    for _ in 0..8 {
        loop {
            let spawned = prefix
                .cmd()
                .args(["adapter", "install", "--dart-hash", HASH, "--json"])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn();
            match spawned {
                Ok(child) => {
                    children.push(child);
                    break;
                }
                Err(err) if err.raw_os_error() == Some(26) => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                Err(err) => panic!("spawn adapter install: {err}"),
            }
        }
    }
    let outputs = children
        .into_iter()
        .map(|child| child.wait_with_output().expect("wait for install"))
        .collect::<Vec<_>>();

    let mut installed = 0;
    for output in &outputs {
        assert_eq!(
            code(output),
            0,
            "a concurrent install failed: {}",
            stderr(output)
        );
        if json(output)["idempotent"] == Value::Bool(false) {
            installed += 1;
        }
    }
    assert_eq!(
        installed, 1,
        "{installed} of 8 concurrent runs claimed to be the install"
    );

    let list = prefix.list();
    assert_eq!(code(&list), 0, "{}", stderr(&list));
    let rows = json(&list);
    assert_eq!(rows.as_array().map(Vec::len), Some(1));
    assert_eq!(rows[0]["state"], text("verified"));

    let state: Value =
        serde_json::from_slice(&fs::read(prefix.store().join("store.json")).expect("read state"))
            .expect("parse state");
    assert_eq!(
        state["adapters"].as_array().map(Vec::len),
        Some(1),
        "concurrent installs left more than one record"
    );
    assert_eq!(
        store_files(&prefix.store()),
        prefix.settled_store_files(),
        "concurrent installs left temporary files behind"
    );
}

/// The race above can pass by luck: if the first process finishes before the
/// last one starts, timing serialized the work and the lock proved nothing. So
/// hold the store lock here and require a real install to wait for it.
#[test]
fn an_install_waits_for_the_store_lock() {
    let prefix = Prefix::new();
    fs::create_dir_all(prefix.store()).expect("mkdir store");
    let lock_path = prefix.store().join(".lock");
    let lock = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .expect("open store lock");
    assert_eq!(
        unsafe { libc::flock(std::os::unix::io::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX) },
        0,
        "could not take the store lock"
    );

    let mut child = loop {
        match prefix
            .cmd()
            .args(["adapter", "install", "--dart-hash", HASH, "--json"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(child) => break child,
            Err(err) if err.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            Err(err) => panic!("spawn adapter install: {err}"),
        }
    };

    // Long enough for an unlocked install to have finished several times over.
    for _ in 0..25 {
        std::thread::sleep(std::time::Duration::from_millis(80));
        assert!(
            child.try_wait().expect("poll install").is_none(),
            "the install did not wait for the store lock"
        );
    }
    assert!(
        store_files(&prefix.store()).is_empty(),
        "the install published while the store was locked: {:?}",
        store_files(&prefix.store())
    );

    assert_eq!(
        unsafe { libc::flock(std::os::unix::io::AsRawFd::as_raw_fd(&lock), libc::LOCK_UN) },
        0
    );
    let output = child.wait_with_output().expect("wait for install");
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(json(&output)["idempotent"], Value::Bool(false));
    assert_eq!(store_files(&prefix.store()), prefix.settled_store_files());
}

#[test]
fn an_injected_failure_before_any_publish_step_leaves_no_partial_state() {
    for step in ["lock", "stage", "publish_artifact", "publish_state"] {
        let prefix = Prefix::new();
        let output = prefix.run_with(
            &[("FLUTTERDEC_INSTALL_FAIL_BEFORE", step)],
            &["adapter", "install", "--dart-hash", HASH],
        );
        assert_ne!(code(&output), 0, "the injected failure at {step} succeeded");
        assert!(
            stderr(&output).contains(step),
            "the error does not name the failed step {step}: {}",
            stderr(&output)
        );
        assert!(
            !prefix.artifact().exists(),
            "{step} left a published artifact behind"
        );
        assert!(
            !prefix.store().join("store.json").exists(),
            "{step} left a state file behind"
        );
        assert!(
            store_files(&prefix.store()).is_empty(),
            "{step} left {:?} behind",
            store_files(&prefix.store())
        );

        // The store is still usable afterwards, so the failure did not poison it.
        let recovered = prefix.install();
        assert_eq!(code(&recovered), 0, "{}", stderr(&recovered));
        assert_eq!(json(&recovered)["idempotent"], Value::Bool(false));
    }
}

/// A failure after the artifact is live must put the artifact back rather than
/// merely stop. Checked separately because the interesting case is an existing
/// install being replaced.
#[test]
fn a_failed_state_publish_restores_the_previous_artifact() {
    let prefix = Prefix::new();
    assert_eq!(code(&prefix.install()), 0);
    let before = fs::read(prefix.artifact()).expect("read artifact");

    // Make the install non-idempotent so it has to republish, then fail it
    // after the artifact rename.
    let state_path = prefix.store().join("store.json");
    let mut state: Value =
        serde_json::from_slice(&fs::read(&state_path).expect("read state")).expect("parse state");
    state["adapters"][0]["source"] = text("stale");
    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&state).expect("serialize state"),
    )
    .expect("write state");
    let stale = fs::read(&state_path).expect("read state");

    let output = prefix.run_with(
        &[("FLUTTERDEC_INSTALL_FAIL_BEFORE", "publish_state")],
        &["adapter", "install", "--dart-hash", HASH],
    );
    assert_ne!(code(&output), 0);
    assert_eq!(
        fs::read(prefix.artifact()).expect("read artifact"),
        before,
        "the artifact was not restored"
    );
    assert_eq!(
        fs::read(&state_path).expect("read state"),
        stale,
        "the state file changed despite the failure"
    );
    assert_eq!(store_files(&prefix.store()), prefix.settled_store_files());
}

#[test]
fn a_record_path_that_escapes_the_store_is_refused() {
    for relative in [
        "../escape",
        "artifacts/../../escape",
        "/tmp/flutterdec-escape-must-not-exist",
        "./escape",
    ] {
        let prefix = Prefix::with_variant(relative, std::env::consts::OS, std::env::consts::ARCH);
        let output = prefix.run(&["adapter", "install", "--dart-hash", HASH]);
        assert_ne!(code(&output), 0, "{relative} was installed");
        assert!(
            stderr(&output).contains("not a contained relative path"),
            "{relative}: {}",
            stderr(&output)
        );
        assert!(
            store_files(&prefix.store()).is_empty(),
            "{relative} wrote to the store"
        );
        assert!(
            !prefix.root().join("escape").exists()
                && !Path::new("/tmp/flutterdec-escape-must-not-exist").exists(),
            "{relative} wrote outside the store"
        );
    }
}

#[test]
fn a_store_directory_that_is_a_symlink_out_of_the_store_is_refused() {
    let prefix = Prefix::new();
    let outside = prefix.root().join("outside");
    fs::create_dir_all(&outside).expect("mkdir outside");
    fs::create_dir_all(prefix.store()).expect("mkdir store");
    std::os::unix::fs::symlink(&outside, prefix.store().join("artifacts"))
        .expect("symlink artifacts");

    let output = prefix.run(&["adapter", "install", "--dart-hash", HASH]);
    assert_ne!(code(&output), 0);
    assert!(
        stderr(&output).contains("escapes the adapter store"),
        "{}",
        stderr(&output)
    );
    assert!(
        fs::read_dir(&outside)
            .expect("read outside")
            .next()
            .is_none(),
        "the install wrote through the symbolic link"
    );
}

#[test]
fn an_artifact_source_that_is_not_the_declared_artifact_is_refused() {
    let prefix = Prefix::new();
    let wrong = prefix.root().join("wrong.sh");
    fs::write(&wrong, "#!/bin/sh\nexit 0\n").expect("write wrong");
    let a_directory = prefix.root().join("a_directory");
    fs::create_dir_all(&a_directory).expect("mkdir");
    let a_link = prefix.root().join("a_link");
    std::os::unix::fs::symlink(prefix.producer(), &a_link).expect("symlink");

    for (source, expected) in [
        (&wrong, "does not match the compatibility record"),
        (&a_directory, "is not a regular file"),
        (&a_link, "is a symbolic link"),
    ] {
        let output = prefix.run(&[
            "adapter",
            "install",
            "--dart-hash",
            HASH,
            "--from",
            source.to_str().expect("path"),
        ]);
        assert_ne!(code(&output), 0, "{} was accepted", source.display());
        assert!(
            stderr(&output).contains(expected),
            "{}: {}",
            source.display(),
            stderr(&output)
        );
        assert!(store_files(&prefix.store()).is_empty());
    }

    // The declared artifact itself, passed explicitly, is accepted and recorded
    // as operator supplied.
    let output = prefix.run(&[
        "adapter",
        "install",
        "--dart-hash",
        HASH,
        "--from",
        prefix.producer().to_str().expect("path"),
        "--json",
    ]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(json(&output)["record"]["source"]
        .as_str()
        .expect("source")
        .starts_with("operator:"));
}

#[test]
fn a_wrong_host_or_target_is_refused() {
    let foreign = Prefix::with_variant(ARTIFACT_RELATIVE, "plan9", "vax");
    let output = foreign.run(&["adapter", "install", "--dart-hash", HASH]);
    assert_ne!(code(&output), 0);
    assert!(
        stderr(&output).contains("no artifact variant for host"),
        "{}",
        stderr(&output)
    );
    assert!(store_files(&foreign.store()).is_empty());

    let list = foreign.list();
    assert_eq!(code(&list), 0, "{}", stderr(&list));
    assert_eq!(json(&list)[0]["state"], text("incompatible"));

    let prefix = Prefix::new();
    let output = prefix.run(&[
        "adapter",
        "install",
        "--dart-hash",
        HASH,
        "--target-arch",
        "x64",
    ]);
    assert_ne!(code(&output), 0);
    assert!(
        stderr(&output).contains("targets x64"),
        "{}",
        stderr(&output)
    );
    assert!(store_files(&prefix.store()).is_empty());
}

#[test]
fn invalid_and_unregistered_input_fails_deterministically() {
    let prefix = Prefix::new();
    for (hash, expected) in [
        ("", "is not 32 lowercase hexadecimal characters"),
        ("80a49c71", "is not 32 lowercase hexadecimal characters"),
        (
            "80A49C7111088100A233B2AE788E1F48",
            "is not 32 lowercase hexadecimal characters",
        ),
        (
            "../../etc/passwd",
            "is not 32 lowercase hexadecimal characters",
        ),
        (OTHER_HASH, "no compatibility record for snapshot hash"),
    ] {
        let first = prefix.run(&["adapter", "install", "--dart-hash", hash]);
        let second = prefix.run(&["adapter", "install", "--dart-hash", hash]);
        assert_ne!(code(&first), 0, "{hash:?} was accepted");
        assert_eq!(
            code(&first),
            code(&second),
            "{hash:?} exit code is not stable"
        );
        assert_eq!(
            stderr(&first),
            stderr(&second),
            "{hash:?} message is not stable"
        );
        assert!(
            stderr(&first).contains(expected),
            "{hash:?}: {}",
            stderr(&first)
        );
        assert!(store_files(&prefix.store()).is_empty());
    }
}

#[test]
fn list_reports_missing_corrupt_and_unavailable_states() {
    // A file with the right name and the right bytes, never installed, is not
    // an install. This is the case an existence check gets wrong.
    let imposter = Prefix::new();
    let artifact = imposter.artifact();
    fs::create_dir_all(artifact.parent().expect("parent")).expect("mkdir");
    fs::copy(imposter.producer(), &artifact).expect("copy imposter");
    let list = imposter.list();
    assert_eq!(code(&list), 0, "{}", stderr(&list));
    assert_eq!(json(&list)[0]["state"], text("unavailable"));

    let prefix = Prefix::new();
    assert_eq!(code(&prefix.install()), 0);

    // Same size, different bytes: a length check alone would call this fine.
    let mut bytes = fs::read(prefix.artifact()).expect("read artifact");
    let last = bytes.len() - 2;
    bytes[last] = b'9';
    fs::write(prefix.artifact(), &bytes).expect("corrupt artifact");
    let list = prefix.list();
    assert_eq!(
        code(&list),
        2,
        "a corrupt store exited 0: {}",
        stdout(&list)
    );
    let rows = json(&list);
    assert_eq!(rows[0]["state"], text("corrupt"));
    assert!(rows[0]["detail"]
        .as_str()
        .expect("detail")
        .contains("SHA-256"));

    fs::remove_file(prefix.artifact()).expect("remove artifact");
    let list = prefix.list();
    assert_eq!(code(&list), 2, "a missing artifact exited 0");
    assert_eq!(json(&list)[0]["state"], text("missing"));

    // Text output carries the same states, and a broken store is still an error.
    let plain = prefix.run(&["adapter", "list"]);
    assert_eq!(code(&plain), 2);
    assert!(
        stdout(&plain).contains("state=missing"),
        "{}",
        stdout(&plain)
    );

    fs::write(prefix.store().join("store.json"), "{ not json").expect("write state");
    let list = prefix.list();
    assert_ne!(code(&list), 0, "a malformed state file exited 0");
    assert!(
        stderr(&list).contains("adapter store state is unusable"),
        "{}",
        stderr(&list)
    );
}

#[test]
fn the_store_override_is_explicit_and_deterministic() {
    let prefix = Prefix::new();
    let alternate = prefix.root().join("alternate-store");
    let alternate_env = alternate.to_str().expect("path").to_string();

    let output = prefix.run_with(
        &[("FLUTTERDEC_ADAPTER_STORE", &alternate_env)],
        &["adapter", "install", "--dart-hash", HASH, "--json"],
    );
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(
        json(&output)["store_dir"].as_str().map(PathBuf::from),
        Some(alternate.clone())
    );
    assert!(alternate.join(ARTIFACT_RELATIVE).is_file());
    assert!(
        !prefix.store().exists(),
        "the override did not replace the default store"
    );

    // The default store cannot see the override's install, and the override can.
    let list = prefix.list();
    assert_eq!(code(&list), 0, "{}", stderr(&list));
    assert_eq!(json(&list)[0]["state"], text("unavailable"));

    let list = prefix.run_with(
        &[("FLUTTERDEC_ADAPTER_STORE", &alternate_env)],
        &["adapter", "list", "--json"],
    );
    assert_eq!(code(&list), 0, "{}", stderr(&list));
    assert_eq!(json(&list)[0]["state"], text("verified"));

    // XDG_DATA_HOME moves the default store without an explicit override.
    let xdg = prefix.root().join("xdg");
    let output = prefix.run_with(
        &[("XDG_DATA_HOME", xdg.to_str().expect("path"))],
        &["adapter", "install", "--dart-hash", HASH, "--json"],
    );
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(
        json(&output)["store_dir"].as_str().map(PathBuf::from),
        Some(xdg.join("flutterdec/adapters"))
    );
}

#[test]
fn package_data_that_cannot_be_written_still_serves_an_install() {
    let prefix = Prefix::new();
    let before = tree_digests(&prefix.share());
    let mut modes = Vec::new();
    for dir in [
        prefix.share().join("adapters/python"),
        prefix.share().join("adapters"),
        prefix.share().join("data"),
        prefix.share(),
    ] {
        let perms = fs::metadata(&dir).expect("metadata").permissions();
        modes.push((dir.clone(), perms.mode()));
        let mut readonly = perms;
        readonly.set_mode(0o555);
        fs::set_permissions(&dir, readonly).expect("chmod read-only");
    }

    let install = prefix.install();
    let list = prefix.list();

    for (dir, mode) in modes.into_iter().rev() {
        let mut perms = fs::metadata(&dir).expect("metadata").permissions();
        perms.set_mode(mode);
        fs::set_permissions(&dir, perms).expect("restore mode");
    }

    assert_eq!(
        code(&install),
        0,
        "a read-only package prefix broke install: {}",
        stderr(&install)
    );
    assert_eq!(code(&list), 0, "{}", stderr(&list));
    assert_eq!(json(&list)[0]["state"], text("verified"));
    assert_eq!(before, tree_digests(&prefix.share()));
}

#[test]
fn a_prefix_without_package_data_says_so_instead_of_guessing() {
    let prefix = Prefix::new();
    fs::remove_file(prefix.share().join("adapters/registry.json")).expect("remove registry");
    let output = prefix.run(&["adapter", "list"]);
    assert_ne!(code(&output), 0);
    let message = stderr(&output);
    assert!(
        message.contains("no packaged data directory holds adapters/registry.json"),
        "{message}"
    );
    assert!(
        message.contains("FLUTTERDEC_DATA_DIR"),
        "the error does not name the override: {message}"
    );

    // An override that holds no registry fails rather than falling back to a
    // directory that happens to have one.
    let empty = prefix.root().join("empty");
    fs::create_dir_all(&empty).expect("mkdir empty");
    let output = prefix.run_with(
        &[("FLUTTERDEC_DATA_DIR", empty.to_str().expect("path"))],
        &["adapter", "list"],
    );
    assert_ne!(code(&output), 0);
    assert!(
        stderr(&output).contains("FLUTTERDEC_DATA_DIR is set to"),
        "{}",
        stderr(&output)
    );

    // An override that does hold one is used instead of the prefix, which is
    // what makes the override deterministic rather than advisory.
    let checkout = checkout_root();
    let output = prefix.run_with(
        &[("FLUTTERDEC_DATA_DIR", checkout.to_str().expect("path"))],
        &["adapter", "list", "--json"],
    );
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        json(&output)
            .as_array()
            .expect("rows")
            .iter()
            .all(|row| row["profile_id"] != text("fixture-profile")),
        "the fixture registry was used despite the override"
    );
}

/// `info` and `decompile` have to resolve the same registry, the same profile,
/// and the same store as `adapter install` and `adapter list`, or the store is
/// only a bookkeeping exercise. The proof is behavioral: the installed artifact
/// in *this* store is the file they execute.
#[test]
fn info_and_decompile_resolve_the_same_registry_profile_and_store() {
    let prefix = Prefix::new();
    let libapp = prefix.root().join("libapp.so");
    fs::write(&libapp, synthetic_libapp(HASH, FEATURES)).expect("write libapp");
    let input = libapp.to_str().expect("path").to_string();

    let before = prefix.run(&["info", &input, "--json"]);
    assert_eq!(code(&before), 0, "{}", stderr(&before));
    let report = json(&before);
    assert_eq!(report["snapshot_hash"], text(HASH));
    assert_eq!(report["registry_record_present"], Value::Bool(true));
    assert_eq!(
        report["adapter_installed"],
        Value::Bool(false),
        "an empty store reported an installed adapter"
    );
    assert!(
        !prefix.marker.exists(),
        "info ran an adapter that was never installed"
    );

    let install = prefix.install();
    assert_eq!(code(&install), 0, "{}", stderr(&install));
    let after = prefix.run(&["info", &input, "--json"]);
    assert_eq!(code(&after), 0, "{}", stderr(&after));
    let report = json(&after);
    assert_eq!(
        report["adapter_installed"],
        Value::Bool(true),
        "info did not see the install: {}",
        stdout(&after)
    );
    assert_eq!(
        report["compatibility_record_sha256"],
        json(&install)["record"]["compatibility_record_sha256"],
        "info and install disagree about the compatibility record"
    );
    assert!(
        prefix.marker.exists(),
        "info did not execute the artifact from the resolved store"
    );

    // Point the same binary at an empty store: the install is invisible again,
    // which is only true if `info` reads the resolved store rather than a path
    // fixed at build time.
    fs::remove_file(&prefix.marker).expect("remove marker");
    let elsewhere = prefix.run_with(
        &[(
            "FLUTTERDEC_ADAPTER_STORE",
            prefix.root().join("nowhere").to_str().expect("path"),
        )],
        &["info", &input, "--json"],
    );
    assert_eq!(code(&elsewhere), 0, "{}", stderr(&elsewhere));
    assert_eq!(json(&elsewhere)["adapter_installed"], Value::Bool(false));
    assert!(
        !prefix.marker.exists(),
        "info ran an artifact from another store"
    );

    // `decompile` resolves the same two locations. The fixture producer writes
    // no model, so both runs fail, but they fail at different points: with no
    // store the artifact is unavailable, and with the install the artifact runs.
    let out = prefix.root().join("out");
    let out_arg = out.to_str().expect("path").to_string();
    let empty = prefix.run_with(
        &[(
            "FLUTTERDEC_ADAPTER_STORE",
            prefix.root().join("nowhere").to_str().expect("path"),
        )],
        &["decompile", &input, "-o", &out_arg],
    );
    assert_ne!(code(&empty), 0);
    assert!(
        stderr(&empty).contains("adapter artifact") && stderr(&empty).contains("unavailable"),
        "decompile did not look for the artifact in the resolved store: {}",
        stderr(&empty)
    );
    assert!(
        !prefix.marker.exists(),
        "decompile ran an uninstalled artifact"
    );

    let installed = prefix.run(&["decompile", &input, "-o", &out_arg]);
    assert_ne!(code(&installed), 0, "the fixture producer emits no model");
    assert!(
        prefix.marker.exists(),
        "decompile did not execute the artifact from the resolved store: {}",
        stderr(&installed)
    );
}

/// A minimal ARM64 `libapp.so` carrying a FullAOT snapshot header.
///
/// One `PT_LOAD` at address zero, so a symbol's virtual address equals its file
/// offset, plus the four `_kDart*` symbols the loader looks for. The snapshot
/// header layout is `runtime/vm/snapshot.h`: magic, `int64` length, `int64`
/// kind, then the 32-character hash and the NUL-terminated features string.
fn synthetic_libapp(hash: &str, features: &str) -> Vec<u8> {
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
