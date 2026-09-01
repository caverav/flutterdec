//! The adapter store, driven through the real CLI.
//!
//! Install, listing, rollback, containment and store containment, each proven
//! through the packaged binary rather than through the library. The rig itself
//! lives in `support`, which every CLI integration test shares.

mod support;

use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use support::*;

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
    // The fixture producer exits without writing a result, so this run fails.
    // That is the point: the artifact in *this* store is the one that ran, and
    // `info` reports the failure rather than printing a report that looks like
    // a snapshot with nothing in it.
    let after = prefix.run(&["info", &input, "--json"]);
    assert_ne!(code(&after), 0, "a producer that answered nothing exited 0");
    assert!(
        stderr(&after).contains("error category: adapter_no_result"),
        "info did not name the failure category: {}",
        stderr(&after)
    );
    let report = json(&after);
    assert_eq!(
        report["adapter_installed"],
        Value::Bool(true),
        "info did not see the install: {}",
        stdout(&after)
    );
    assert_eq!(
        report["adapter_error_category"],
        text("adapter_no_result"),
        "info swallowed the adapter failure: {}",
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

    // `decompile` resolves the same two locations. With no store there is
    // nothing to execute, so core recovers the program and names the reason;
    // with the install the artifact runs. The two arms differ in exactly one
    // observable: whether the fixture producer's marker appears.
    let out = prefix.root().join("out");
    let out_arg = out.to_str().expect("path").to_string();
    let empty = prefix.run_with(
        &[(
            "FLUTTERDEC_ADAPTER_STORE",
            prefix.root().join("nowhere").to_str().expect("path"),
        )],
        &[
            "decompile",
            &input,
            "-o",
            &out_arg,
            "--function-scope",
            "all",
            "--min-disassembly-ratio",
            "0.0",
        ],
    );
    assert_eq!(code(&empty), 0, "{}", stderr(&empty));
    let summary: Value =
        serde_json::from_slice(&fs::read(out.join("report.json")).expect("read report"))
            .expect("report JSON");
    assert_eq!(
        summary["adapter_selection"]["provider"]["core_fallback_reason"],
        text("adapter_not_installed"),
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

/// What the host says it established has to reach the operator, and it has to
/// say the same thing through both surfaces that report a run.
#[test]
fn info_and_the_decompile_report_state_which_containment_controls_were_established() {
    let prefix = Prefix::answering();
    let libapp = prefix.root().join("libapp.so");
    fs::write(&libapp, synthetic_libapp(HASH, FEATURES)).expect("write libapp");
    let input = libapp.to_str().expect("path").to_string();

    let install = prefix.install();
    assert_eq!(code(&install), 0, "{}", stderr(&install));

    let info = prefix.run(&["info", &input, "--json"]);
    assert_eq!(code(&info), 0, "{}", stderr(&info));
    let report = json(&info);
    assert_eq!(
        report["resolved_backend"],
        text("internal"),
        "the fixture producer did not run: {}",
        stdout(&info)
    );
    assert_controls_are_accurate(&report["adapter_containment"], "flutterdec info");

    let out = prefix.root().join("out");
    let out_arg = out.to_str().expect("path").to_string();
    // The fixture producer recovers nothing, so the default app-only scope has
    // nothing to emit. The scope is not what this test is about.
    let decompile = prefix.run(&[
        "decompile",
        &input,
        "-o",
        &out_arg,
        "--function-scope",
        "all",
    ]);
    let report_path = out.join("report.json");
    assert!(
        report_path.is_file(),
        "decompile wrote no report (exit {}): {}",
        code(&decompile),
        stderr(&decompile)
    );
    let summary: Value =
        serde_json::from_slice(&fs::read(&report_path).expect("read report")).expect("report JSON");
    assert_controls_are_accurate(
        &summary["adapter_selection"]["provider"]["containment"],
        "decompile report.json",
    );
    assert_eq!(
        comparable_across_runs(&summary["adapter_selection"]["provider"]["containment"]),
        comparable_across_runs(&report["adapter_containment"]),
        "info and report.json disagree about what was established"
    );
}

/// The feature tuple both shipped records declare, as a snapshot header spells
/// it. Order is not significant: the selection key normalizes before comparing.
const SHIPPED_FEATURES: &str = "product no-code_comments compressed-pointers arm64 android";

/// The store ledger, as JSON, for an observation to be reported alongside.
fn ledger(store: &Path) -> Value {
    match fs::read(store.join("store.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).expect("store.json is JSON"),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(err) => panic!("read store.json: {err}"),
    }
}

/// The `adapter list` row for one snapshot hash.
fn row_for<'a>(rows: &'a Value, hash: &str) -> &'a Value {
    rows.as_array()
        .expect("rows")
        .iter()
        .find(|row| row["snapshot_hash"] == text(hash))
        .unwrap_or_else(|| panic!("no row for {hash}: {rows}"))
}

/// Installing for one record must not authorize a different record that names
/// identical artifact bytes, and what `adapter list` reports must be what
/// `info` does — in both directions.
///
/// This drives the *shipped* registry rather than the fixture because that is
/// where the two records collide: both name `artifacts/flutterdec-local-python`
/// with the same digest, so the file on disk cannot tell them apart. Only the
/// store ledger records which record something was installed for, and it is the
/// ledger `adapter list` reports from.
#[test]
fn an_install_for_one_record_does_not_authorize_another_sharing_its_artifact() {
    let prefix = Prefix::new();
    let checkout = checkout_root();
    let checkout_before = tree_digests(&checkout.join("adapters"));
    let store = prefix.root().join("shipped-store");
    let env = [
        ("FLUTTERDEC_DATA_DIR", checkout.to_str().expect("path")),
        (
            "FLUTTERDEC_ADAPTER_STORE",
            store.to_str().expect("store path"),
        ),
    ];

    let install = prefix.run_with(
        &env,
        &["adapter", "install", "--dart-hash", OTHER_HASH, "--json"],
    );
    assert_eq!(code(&install), 0, "install failed: {}", stderr(&install));

    // Exactly one record is installed, and the ledger says which one.
    let claimed = ledger(&store);
    assert_eq!(
        claimed["adapters"].as_array().map(Vec::len),
        Some(1),
        "store.json: {claimed:#}"
    );
    assert_eq!(
        claimed["adapters"][0]["snapshot_hash"],
        text(OTHER_HASH),
        "store.json: {claimed:#}"
    );

    let rows = json(&prefix.run_with(&env, &["adapter", "list", "--json"]));
    assert_eq!(
        row_for(&rows, OTHER_HASH)["state"],
        text("verified"),
        "store.json: {claimed:#}\nrows: {rows:#}"
    );
    assert_eq!(
        row_for(&rows, HASH)["state"],
        text("unavailable"),
        "store.json: {claimed:#}\nrows: {rows:#}"
    );
    // Both records really do name one file, which is what makes an existence
    // check unable to answer this.
    assert_eq!(
        row_for(&rows, HASH)["artifact_path"],
        row_for(&rows, OTHER_HASH)["artifact_path"],
        "the shipped records no longer share an artifact path: {rows:#}"
    );

    let libapp = prefix.root().join("other-record-libapp.so");
    fs::write(&libapp, synthetic_libapp(HASH, SHIPPED_FEATURES)).expect("write libapp");
    let input = libapp.to_str().expect("path").to_string();

    let info = prefix.run_with(&env, &["info", &input, "--json"]);
    let report = json(&info);
    assert_eq!(
        report["snapshot_hash"],
        text(HASH),
        "the fixture snapshot does not match the uninstalled record: {report:#}"
    );
    assert_eq!(
        report["registry_record_present"],
        Value::Bool(true),
        "the uninstalled record was not selected at all: {report:#}"
    );
    // The split: `adapter list` calls this record unavailable, so nothing may
    // report it as a registered adapter that executed.
    assert_eq!(
        report["adapter_installed"],
        Value::Bool(false),
        "info claims an install the ledger does not hold\nstore.json: {claimed:#}\ninfo: {report:#}"
    );
    assert_eq!(
        report["provider"]["adapter_executed"],
        Value::Bool(false),
        "an adapter installed for {OTHER_HASH} executed for {HASH}\nstore.json: {claimed:#}\ninfo: {report:#}"
    );
    assert_ne!(
        report["provider"]["producer_trust"],
        text("registered"),
        "an unavailable record produced a registered producer\nstore.json: {claimed:#}\ninfo: {report:#}"
    );

    // The other direction: once the ledger does hold this record, `list` says
    // verified and authorization does not refuse for want of an installation.
    let install = prefix.run_with(&env, &["adapter", "install", "--dart-hash", HASH, "--json"]);
    assert_eq!(code(&install), 0, "install failed: {}", stderr(&install));
    let claimed = ledger(&store);
    assert_eq!(
        claimed["adapters"].as_array().map(Vec::len),
        Some(2),
        "store.json: {claimed:#}"
    );

    let rows = json(&prefix.run_with(&env, &["adapter", "list", "--json"]));
    assert_eq!(
        row_for(&rows, HASH)["state"],
        text("verified"),
        "store.json: {claimed:#}\nrows: {rows:#}"
    );

    let info = prefix.run_with(&env, &["info", &input, "--json"]);
    let report = json(&info);
    assert_eq!(
        report["adapter_installed"],
        Value::Bool(true),
        "info did not see the install\nstore.json: {claimed:#}\ninfo: {report:#}"
    );
    assert_eq!(
        report["provider"]["adapter_executed"],
        Value::Bool(true),
        "a verified record did not execute\nstore.json: {claimed:#}\ninfo: {report:#}\nstderr: {}",
        stderr(&info)
    );
    assert_eq!(
        report["provider"]["producer_trust"],
        text("registered"),
        "store.json: {claimed:#}\ninfo: {report:#}"
    );

    assert_eq!(
        checkout_before,
        tree_digests(&checkout.join("adapters")),
        "the run mutated read-only package data"
    );
}
