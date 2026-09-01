//! A same-user attacker who finds the invocation workspace finds nothing to attack.
//!
//! The store artifact is not the only path an attacker could aim at. A host that
//! verifies bytes and then materializes them somewhere to execute them has moved
//! the target rather than removed it: the copy is discoverable, it is owned by
//! the same user, and `0500` stops nothing, because the owner of a file may
//! `chmod` it back, rename it away, unlink it, or drop a different file in its
//! place. So the property under test is not "the copy is hard to write to" but
//! "there is no copy to find".
//!
//! The attacker here is a real process, not a thread pretending to be one. It
//! waits for the host to reach the point where verification is complete and no
//! child exists yet, walks the invocation workspace from the outside, and for
//! every executable file it finds tries all four of overwrite, `chmod` and
//! overwrite again, rename, and unlink. What it may do is proved rather than
//! assumed: a decoy executable is planted in a directory scanned in the same
//! pass, and the run is only evidence if the decoy was found and destroyed.
//!
//! This file holds one test on purpose: the rendezvous is selected by an
//! environment variable, and a second test running beside it in the same binary
//! would block on a rendezvous it never asked for.

mod support;

use flutterdec_adapter::host::PRESPAWN_RENDEZVOUS_VAR;
use flutterdec_adapter::model::InputRegionName;
use flutterdec_adapter::protocol::RequestedBackend;
use flutterdec_adapter::{run_adapter, AdapterInput, AdapterRegionInput, HostError, Limits};
use flutterdec_loader::identity::SnapshotIdentity;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tempfile::TempDir;

const RET: [u8; 4] = 0xD65F_03C0u32.to_le_bytes();

/// An adapter that reports which image it is actually running.
///
/// It reads itself through `argv[0]`, which is the only name it has, and records
/// that name together with the digest of the bytes behind it. That turns "the
/// right adapter ran" into an equality between two digests rather than an
/// inference from which log file grew.
fn adapter_source(log: &Path) -> String {
    format!(
        r#"#!/usr/bin/env python3
import hashlib, pathlib, sys

image = pathlib.Path(sys.argv[0]).read_bytes()
with open({log:?}, "a") as fp:
    fp.write(sys.argv[0] + " " + hashlib.sha256(image).hexdigest() + "\n")
raise SystemExit(7)
"#,
        log = log.display().to_string()
    )
}

/// The attacker, which is deliberately not in a hurry and not clever: it waits
/// for a synchronization point the host publishes, then walks directories.
const ATTACKER: &str = r#"#!/usr/bin/env python3
import json, os, pathlib, stat, sys, time

ready, go, report, impostor, tmp_root, decoy = sys.argv[1:7]

deadline = time.monotonic() + 60
while not os.path.exists(ready):
    if time.monotonic() > deadline:
        raise SystemExit("the host never reached the rendezvous")
    time.sleep(0.002)

impostor_bytes = pathlib.Path(impostor).read_bytes()
roots = sorted(pathlib.Path(tmp_root).glob("flutterdec-adapter-*")) + [pathlib.Path(decoy)]

seen, candidates = [], []
for root in roots:
    for base, _dirs, files in os.walk(root):
        for name in files:
            path = pathlib.Path(base) / name
            seen.append(str(path))
            try:
                mode = path.lstat().st_mode
            except OSError:
                continue
            if not stat.S_ISREG(mode) or not mode & 0o111:
                continue

            entry = {"path": str(path)}

            def attempt(key, action):
                try:
                    action()
                    entry[key] = "ok"
                except OSError as err:
                    entry[key] = err.strerror

            attempt("overwrite", lambda: path.write_bytes(impostor_bytes))
            attempt("chmod", lambda: os.chmod(path, 0o777))
            # The point of the second write: an unwritable mode is not a
            # defence against the user who owns the file.
            attempt("overwrite_after_chmod", lambda: path.write_bytes(impostor_bytes))
            moved = path.with_name(path.name + ".moved")
            attempt("rename", lambda: os.rename(path, moved))
            target = moved if entry.get("rename") == "ok" else path
            attempt("unlink", lambda: os.unlink(target))
            # And replacing the name outright, which even unwritable bytes
            # cannot survive.
            def replace():
                path.write_bytes(impostor_bytes)
                os.chmod(path, 0o755)

            attempt("replace", replace)
            candidates.append(entry)

pathlib.Path(report).write_text(
    json.dumps({"roots": [str(root) for root in roots], "seen": seen, "candidates": candidates})
)
pathlib.Path(go).write_text("go")
"#;

#[derive(Debug, serde::Deserialize)]
struct Report {
    roots: Vec<String>,
    seen: Vec<String>,
    candidates: Vec<std::collections::BTreeMap<String, String>>,
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).expect("write adapter");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod adapter");
}

fn regions() -> Vec<Vec<u8>> {
    vec![vec![0u8; 64], vec![0u8; 64], RET.to_vec(), RET.repeat(4)]
}

fn region_inputs(regions: &[Vec<u8>]) -> Vec<AdapterRegionInput<'_>> {
    vec![
        AdapterRegionInput {
            region: InputRegionName::VmData,
            bytes: &regions[0],
            virtual_address: None,
        },
        AdapterRegionInput {
            region: InputRegionName::IsolateData,
            bytes: &regions[1],
            virtual_address: None,
        },
        AdapterRegionInput {
            region: InputRegionName::VmInstructions,
            bytes: &regions[2],
            virtual_address: Some(0x1000),
        },
        AdapterRegionInput {
            region: InputRegionName::IsolateInstructions,
            bytes: &regions[3],
            virtual_address: Some(0x2000),
        },
    ]
}

/// The `<argv[0]> <sha256>` lines one adapter left behind.
fn lines(log: &Path) -> Vec<(String, String)> {
    fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (name, digest) = line
                .rsplit_once(' ')
                .expect("the adapter writes two fields");
            (name.to_string(), digest.to_string())
        })
        .collect()
}

fn run_once(installed: &support::Authorized, identity: &SnapshotIdentity) -> HostError {
    let regions = regions();
    let record = installed.record.clone();
    let input = AdapterInput {
        identity,
        authorization: installed.authorization_for(&record),
        producer: installed.producer_for(&record),
        compatibility: installed.binding_for(&record),
        regions: region_inputs(&regions),
        input_path: None,
        libapp: None,
        requested_backend: RequestedBackend::Auto,
        limits: Limits::default(),
    };
    run_adapter(&installed.exec, &input)
        .map(|_| ())
        .expect_err("the fixture adapter writes no result")
}

#[test]
fn an_attacker_who_walks_the_invocation_workspace_finds_no_executable_to_subvert() {
    let dir = TempDir::new().expect("tempdir");
    let authorized_log = dir.path().join("authorized.log");
    let impostor_log = dir.path().join("impostor.log");

    let authorized_source = dir.path().join("authorized_adapter");
    write_executable(&authorized_source, &adapter_source(&authorized_log));
    let impostor_source = dir.path().join("impostor_adapter");
    write_executable(&impostor_source, &adapter_source(&impostor_log));
    let authorized_digest = support::hex_digest(&fs::read(&authorized_source).expect("read"));

    let identity = support::identity();

    // The control: the impostor is a working adapter, so "no impostor line"
    // later is evidence about which bytes ran rather than evidence that these
    // bytes could never run at all.
    let control = support::Authorized::install(&impostor_source, &identity);
    let err = run_once(&control, &identity);
    assert!(
        matches!(err, HostError::NoResult { .. }),
        "the control adapter did not run: {err}"
    );
    assert_eq!(
        lines(&impostor_log).len(),
        1,
        "the impostor cannot execute at all, so this test proves nothing"
    );
    fs::remove_file(&impostor_log).expect("reset the impostor log");

    // A file the attacker is allowed to destroy, in a directory it scans in the
    // same pass as the workspace. Everything it reports about the workspace is
    // only meaningful next to what it did here.
    let decoy_dir = dir.path().join("decoy");
    fs::create_dir(&decoy_dir).expect("mkdir decoy");
    let decoy = decoy_dir.join("planted_adapter");
    write_executable(&decoy, "#!/bin/sh\nexit 0\n");

    let attacker_source = dir.path().join("attacker.py");
    write_executable(&attacker_source, ATTACKER);
    let report_path = dir.path().join("attack.json");
    let rendezvous = TempDir::new().expect("tempdir");

    let mut attacker = std::process::Command::new(&attacker_source)
        .arg(rendezvous.path().join("ready"))
        .arg(rendezvous.path().join("go"))
        .arg(&report_path)
        .arg(&impostor_source)
        .arg(std::env::temp_dir())
        .arg(&decoy_dir)
        .spawn()
        .expect("start the attacker");

    let installed = support::Authorized::install(&authorized_source, &identity);
    let previous = std::env::var_os(PRESPAWN_RENDEZVOUS_VAR);
    std::env::set_var(PRESPAWN_RENDEZVOUS_VAR, rendezvous.path());
    let err = run_once(&installed, &identity);
    match previous {
        Some(value) => std::env::set_var(PRESPAWN_RENDEZVOUS_VAR, value),
        None => std::env::remove_var(PRESPAWN_RENDEZVOUS_VAR),
    }
    let status = attacker.wait().expect("wait for the attacker");
    assert!(status.success(), "the attacker failed: {status}");

    let report: Report = serde_json::from_slice(&fs::read(&report_path).expect("attack report"))
        .expect("the attack report is JSON");

    // The attacker really did reach a live invocation: it saw the request
    // document the host had just written into the workspace it walked.
    let workspace_files: Vec<&String> = report
        .seen
        .iter()
        .filter(|path| path.contains("flutterdec-adapter-"))
        .collect();
    assert!(
        workspace_files
            .iter()
            .any(|path| path.ends_with("/request.json")),
        "the attacker never found the invocation workspace: roots {:?}, saw {:?}",
        report.roots,
        report.seen
    );

    // And it really can destroy what it finds: the decoy is the proof, and it is
    // the only thing it found.
    let decoy_entry = report
        .candidates
        .iter()
        .find(|entry| entry["path"] == decoy.display().to_string())
        .unwrap_or_else(|| panic!("the attacker did not find the planted decoy: {report:?}"));
    for step in [
        "overwrite",
        "chmod",
        "overwrite_after_chmod",
        "rename",
        "unlink",
        "replace",
    ] {
        assert_eq!(
            decoy_entry[step], "ok",
            "the attacker could not {step} the planted decoy: {decoy_entry:?}"
        );
    }
    let workspace_candidates: Vec<&std::collections::BTreeMap<String, String>> = report
        .candidates
        .iter()
        .filter(|entry| entry["path"].contains("flutterdec-adapter-"))
        .collect();

    // On Linux the image is an anonymous descriptor, so there is nothing in the
    // workspace for the attacker to aim at in the first place.
    #[cfg(target_os = "linux")]
    assert!(
        workspace_candidates.is_empty(),
        "the invocation workspace exposed an executable pathname: {workspace_candidates:?}"
    );

    // Where a descriptor cannot be executed the image has to be a path, and the
    // property becomes what the attacker got out of it. It found the name — the
    // decoy above proves it destroys what it finds — and every single thing it
    // tried to do to that name was refused by the kernel.
    #[cfg(not(target_os = "linux"))]
    {
        assert_eq!(
            workspace_candidates.len(),
            1,
            "the invocation workspace exposed something other than the frozen image: {workspace_candidates:?}"
        );
        for entry in &workspace_candidates {
            for step in [
                "overwrite",
                "chmod",
                "overwrite_after_chmod",
                "rename",
                "unlink",
                "replace",
            ] {
                assert_ne!(
                    entry[step], "ok",
                    "a same-user attacker could {step} the running image: {entry:?}"
                );
            }
        }
    }

    // The run itself: the authorized bytes executed, under a name that is not a
    // path in the workspace the attacker just walked.
    assert!(
        matches!(err, HostError::NoResult { .. }),
        "the authorized adapter did not run: {err}"
    );
    assert_eq!(
        lines(&impostor_log),
        Vec::new(),
        "the attacker's bytes executed"
    );
    let ran = lines(&authorized_log);
    assert_eq!(
        ran.len(),
        1,
        "the verified adapter did not execute: {ran:?}"
    );
    let (argv0, digest) = &ran[0];
    assert_eq!(
        digest, &authorized_digest,
        "the executed image was not the verified artifact"
    );
    assert!(
        !argv0.contains("flutterdec-adapter-"),
        "the adapter was executed under a workspace pathname: {argv0}"
    );
    // What the kernel hands a `#!` interpreter when the image is a descriptor:
    // a name for the descriptor itself, which exists only inside the child.
    assert!(
        argv0.starts_with("/dev/fd/") || argv0.starts_with("/proc/self/fd/"),
        "the adapter was executed under a filesystem pathname: {argv0}"
    );
}
