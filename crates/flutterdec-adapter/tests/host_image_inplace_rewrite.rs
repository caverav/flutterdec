//! The one attack the frozen-pathname image cannot see, exercised rather than
//! described.
//!
//! Where a descriptor cannot be executed the image has to be a name, and what
//! keeps that name welded to its bytes is `UF_IMMUTABLE` plus a check the child
//! makes immediately before `execve`: the pathname must still resolve to the
//! descriptor the host verified (same device, same inode) and must still be
//! frozen. That catches everything which puts a *different* object at the name,
//! and `host_workspace_race.rs` is where the unlink-and-replace shape of it is
//! held to that outcome.
//!
//! It does not catch this one. The flag is a *user* flag, so its owner can clear
//! it, rewrite the bytes through the same inode with `O_TRUNC`, and set the flag
//! again. Afterwards the device matches, the inode matches, and the flag is
//! present, so the check passes and the rewritten bytes are what executes. Only
//! re-reading the whole image between the check and the `execve` would see it,
//! and the platform cannot make those two atomic.
//!
//! So this file asserts what actually happens, not what would be nice: on Darwin
//! the attacker's bytes run, and the host must not have called that a verified
//! image — `image_integrity` is `unavailable` and says best effort. That is the
//! documented ceiling, and a test is the only thing that keeps the documentation
//! honest about it. On Linux the same attacker finds nothing to rewrite, because
//! the image is an anonymous sealed inode with no name at all.
//!
//! This lives beside `host_workspace_race.rs` rather than inside it because the
//! rendezvous the attacker synchronizes on is selected by a process-wide
//! environment variable: a second test in the same binary would block on a
//! rendezvous it never asked for. Both files therefore hold exactly one test.

mod support;

use flutterdec_adapter::host::PRESPAWN_RENDEZVOUS_VAR;
use flutterdec_adapter::model::{InputRegion, InputRegionName, ObservedInput};
use flutterdec_adapter::primitives::Sha256Digest;
use flutterdec_adapter::protocol::RequestedBackend;
#[cfg(not(target_os = "linux"))]
use flutterdec_adapter::ControlState;
use flutterdec_adapter::{
    run_adapter, AdapterInput, AdapterRegionInput, AdapterRun, HostError, Limits,
};
use flutterdec_loader::identity::SnapshotIdentity;
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tempfile::TempDir;

const RET: [u8; 4] = 0xD65F_03C0u32.to_le_bytes();

/// `UF_IMMUTABLE`, which the attacker reports as a number rather than a name.
#[cfg(not(target_os = "linux"))]
const UF_IMMUTABLE: u64 = 0x0000_0002;

/// An adapter that records which image it is running and then answers correctly.
///
/// The name and the digest come from `argv[0]`, the only name it has, so "which
/// bytes ran" is an equality between two digests. Answering correctly is what
/// makes the containment report observable: the host drops it on every failure
/// path, so a run that has to be *inspected* has to be a run that succeeded.
///
/// The prepared answer is read from a path baked in here rather than embedded,
/// because the answer names the digest of these very bytes and embedding it
/// would not converge.
fn adapter_source(log: &Path, model: &Path) -> String {
    ADAPTER
        .replace("@LOG@", &log.display().to_string())
        .replace("@MODEL@", &model.display().to_string())
}

const ADAPTER: &str = r#"#!/usr/bin/env python3
import argparse, hashlib, json, pathlib, sys

P = argparse.ArgumentParser()
P.add_argument("--request", required=True)
P.add_argument("--result", required=True)
P.add_argument("--input-path")
P.add_argument("--libapp-path")
ARGS = P.parse_args()
REQUEST = json.loads(pathlib.Path(ARGS.request).read_text())
OUTPUT = REQUEST["output"]

image = pathlib.Path(sys.argv[0]).read_bytes()
with open("@LOG@", "a") as fp:
    fp.write(sys.argv[0] + " " + hashlib.sha256(image).hexdigest() + "\n")

pathlib.Path(OUTPUT).write_text(pathlib.Path("@MODEL@").read_text())
pathlib.Path(ARGS.result).write_text(json.dumps({
    "protocol_major": 1,
    "model_major": 4,
    "status": "ok",
    "model": OUTPUT,
    "error": None,
    "resolved_backend": "internal",
    "fallback_reason": None,
    "diagnostics": [],
}))
"#;

/// The attacker: one shape only, and deliberately the gentlest one.
///
/// It never unlinks, renames, or creates a file, so the inode the pre-exec check
/// compares against cannot move. It takes the host's flag off, writes other
/// bytes through the same inode, and puts the flag back, which is the whole
/// residue in five syscalls. Every step's outcome and the inode, device and
/// flags on either side of it are reported, so the Rust side reads what the
/// platform did instead of assuming it.
const ATTACKER: &str = r#"#!/usr/bin/env python3
import errno, json, os, pathlib, stat, sys, time

ready, go, report, impostor, tmp_root = sys.argv[1:6]

deadline = time.monotonic() + 60
while not os.path.exists(ready):
    if time.monotonic() > deadline:
        raise SystemExit("the host never reached the rendezvous")
    time.sleep(0.002)

impostor_bytes = pathlib.Path(impostor).read_bytes()
roots = sorted(pathlib.Path(tmp_root).glob("flutterdec-adapter-*"))

seen, candidates = [], []
for root in roots:
    for base, _dirs, files in os.walk(root):
        for name in files:
            path = pathlib.Path(base) / name
            seen.append(str(path))
            try:
                before = path.lstat()
            except OSError:
                continue
            if not stat.S_ISREG(before.st_mode) or not before.st_mode & 0o111:
                continue

            entry = {
                "path": str(path),
                "device_before": str(before.st_dev),
                "inode_before": str(before.st_ino),
                "flags_before": str(getattr(before, "st_flags", 0)),
            }

            def attempt(key, action):
                try:
                    action()
                    entry[key] = "ok"
                except OSError as err:
                    entry[key] = err.strerror or type(err).__name__

            def chflags(value):
                call = getattr(os, "chflags", None)
                if call is None:
                    raise OSError(errno.ENOSYS, "this platform has no chflags")
                call(path, value)

            def rewrite_in_place():
                fd = os.open(path, os.O_WRONLY | os.O_TRUNC)
                try:
                    os.write(fd, impostor_bytes)
                finally:
                    os.close(fd)

            # Take the flag off, put other bytes through the inode the host is
            # holding open, hand the flag back. The mode has to move too: 0500
            # refuses a write even to its owner, and it is put back so the image
            # is still executable afterwards.
            attempt("chflags_clear", lambda: chflags(0))
            attempt("chmod_writable", lambda: os.chmod(path, 0o700))
            attempt("rewrite_in_place", rewrite_in_place)
            attempt("chmod_back", lambda: os.chmod(path, 0o500))
            attempt("chflags_set", lambda: chflags(stat.UF_IMMUTABLE))

            after = path.lstat()
            entry["device_after"] = str(after.st_dev)
            entry["inode_after"] = str(after.st_ino)
            entry["flags_after"] = str(getattr(after, "st_flags", 0))
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
    candidates: Vec<BTreeMap<String, String>>,
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

/// The answer a correct adapter gives for this invocation.
///
/// It carries the host's own facts, including the digest of the artifact this
/// install published, which is why it can only be built after the install.
fn prepared_model(
    installed: &support::Authorized,
    identity: &SnapshotIdentity,
    regions: &[Vec<u8>],
) -> Vec<u8> {
    let mut model = support::unavailable_model();
    model.producer = installed.producer();
    model.compatibility = Some(installed.binding());
    model.input = ObservedInput {
        identity: identity.clone(),
        regions: region_inputs(regions)
            .into_iter()
            .map(|region| InputRegion {
                region: region.region,
                size: region.bytes.len() as u64,
                sha256: Sha256Digest::of(region.bytes),
                virtual_address: region.virtual_address,
                executable: region.region.is_executable(),
            })
            .collect(),
    };
    model.to_canonical_json()
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

fn run_once(
    installed: &support::Authorized,
    identity: &SnapshotIdentity,
) -> Result<AdapterRun, HostError> {
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
}

#[test]
fn an_owner_who_rewrites_the_frozen_image_in_place_is_not_caught_by_the_identity_check() {
    let dir = TempDir::new().expect("tempdir");
    let authorized_log = dir.path().join("authorized.log");
    let impostor_log = dir.path().join("impostor.log");
    let model_path = dir.path().join("model.json");

    let authorized_source = dir.path().join("authorized_adapter");
    write_executable(
        &authorized_source,
        &adapter_source(&authorized_log, &model_path),
    );
    let impostor_source = dir.path().join("impostor_adapter");
    write_executable(
        &impostor_source,
        &adapter_source(&impostor_log, &model_path),
    );
    let authorized_digest = support::hex_digest(&fs::read(&authorized_source).expect("read"));
    let impostor_digest = support::hex_digest(&fs::read(&impostor_source).expect("read"));

    let identity = support::identity();
    let regions = regions();

    // The control: the impostor is a complete, correct adapter. Without this,
    // "the impostor never ran" would be evidence about the impostor rather than
    // about the host.
    let control = support::Authorized::install(&impostor_source, &identity);
    fs::write(&model_path, prepared_model(&control, &identity, &regions))
        .expect("write the control's prepared answer");
    let run = run_once(&control, &identity).expect("the control adapter answers");
    assert!(run.model.functions.is_empty());
    assert_eq!(
        lines(&impostor_log).len(),
        1,
        "the impostor cannot run at all, so this test proves nothing"
    );
    fs::remove_file(&impostor_log).expect("reset the impostor log");

    // The run under attack, with the answer re-prepared for the artifact this
    // install published.
    let installed = support::Authorized::install(&authorized_source, &identity);
    fs::write(&model_path, prepared_model(&installed, &identity, &regions))
        .expect("write the prepared answer");

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
        .spawn()
        .expect("start the attacker");

    let previous = std::env::var_os(PRESPAWN_RENDEZVOUS_VAR);
    std::env::set_var(PRESPAWN_RENDEZVOUS_VAR, rendezvous.path());
    let outcome = run_once(&installed, &identity);
    match previous {
        Some(value) => std::env::set_var(PRESPAWN_RENDEZVOUS_VAR, value),
        None => std::env::remove_var(PRESPAWN_RENDEZVOUS_VAR),
    }
    let status = attacker.wait().expect("wait for the attacker");
    assert!(status.success(), "the attacker failed: {status}");

    let report: Report = serde_json::from_slice(&fs::read(&report_path).expect("attack report"))
        .expect("the attack report is JSON");

    // The attacker really did walk a live invocation: it saw the request
    // document the host had just written there.
    assert!(
        report
            .seen
            .iter()
            .any(|path| path.contains("flutterdec-adapter-") && path.ends_with("/request.json")),
        "the attacker never found the invocation workspace: roots {:?}, saw {:?}",
        report.roots,
        report.seen
    );

    let workspace_candidates: Vec<&BTreeMap<String, String>> = report
        .candidates
        .iter()
        .filter(|entry| entry["path"].contains("flutterdec-adapter-"))
        .collect();

    // Linux has no name to rewrite: the image is an anonymous sealed inode, so
    // the attacker walks past it, the verified bytes run, and the host is
    // entitled to say so.
    #[cfg(target_os = "linux")]
    {
        assert!(
            workspace_candidates.is_empty(),
            "the invocation workspace exposed an executable pathname: {workspace_candidates:?}"
        );
        let run = outcome.expect("the authorized adapter answers");
        let ran = lines(&authorized_log);
        assert_eq!(ran.len(), 1, "the verified adapter did not run: {ran:?}");
        assert_eq!(
            ran[0].1, authorized_digest,
            "the executed image was not the verified artifact"
        );
        assert_eq!(
            lines(&impostor_log),
            Vec::new(),
            "the attacker's bytes executed"
        );
        assert!(
            run.containment.image_integrity.is_applied(),
            "a sealed anonymous image was not reported as one: {:?}",
            run.containment.image_integrity
        );
        let _ = impostor_digest;
    }

    // Darwin has a name, and this attacker is its owner. What follows is read
    // out of what the platform actually did, and then the run is held to the
    // outcome that really follows from it — including the one this platform
    // cannot prevent.
    #[cfg(not(target_os = "linux"))]
    {
        assert_eq!(
            workspace_candidates.len(),
            1,
            "the invocation workspace exposed something other than the frozen image: {workspace_candidates:?}"
        );
        let entry = workspace_candidates[0];
        let flag = |key: &str| -> u64 {
            entry[key]
                .parse()
                .unwrap_or_else(|_| panic!("the attacker reports {key} as a number: {entry:?}"))
        };
        assert_ne!(
            flag("flags_before") & UF_IMMUTABLE,
            0,
            "the host did not freeze the image it was about to execute: {entry:?}"
        );

        let rewritten = entry["chflags_clear"] == "ok"
            && entry["rewrite_in_place"] == "ok"
            && entry["chflags_set"] == "ok";
        if !rewritten {
            // This platform refused some part of it, which is stronger than the
            // host claims. Then the attacker's bytes must not have run.
            assert_eq!(
                lines(&impostor_log),
                Vec::new(),
                "the attack was refused and the attacker's bytes executed anyway: {entry:?}"
            );
            return;
        }

        // The attack left the pre-exec check nothing to see: same device, same
        // inode, flag back on.
        assert_eq!(
            entry["device_before"], entry["device_after"],
            "the rewrite moved the image to another device, so this is not the in-place case: {entry:?}"
        );
        assert_eq!(
            entry["inode_before"], entry["inode_after"],
            "the rewrite moved the inode, so this is not the in-place case: {entry:?}"
        );
        assert_ne!(
            flag("flags_after") & UF_IMMUTABLE,
            0,
            "the attacker did not put the flag back, so this is not the in-place case: {entry:?}"
        );

        // And so the attacker's bytes are what ran. This is the documented
        // ceiling, asserted as the outcome it is: nothing here claims the attack
        // was prevented, because it was not.
        let run = outcome.unwrap_or_else(|err| {
            panic!("the rewritten image did not run to an answer: {err}");
        });
        let ran = lines(&impostor_log);
        assert_eq!(
            ran.len(),
            1,
            "the in-place rewrite did not reach execution: {ran:?}"
        );
        assert_eq!(
            ran[0].1, impostor_digest,
            "the bytes that ran are not the ones the attacker wrote"
        );
        assert_eq!(
            lines(&authorized_log),
            Vec::new(),
            "the verified bytes ran as well as the attacker's"
        );
        let _ = authorized_digest;

        // What the host is required to do about it: not call that integrity.
        let ControlState::Unavailable { reason } = &run.containment.image_integrity else {
            panic!(
                "the host reported a sealed image for a run whose bytes an owner rewrote: {:?}",
                run.containment.image_integrity
            );
        };
        assert!(
            reason.contains("UF_IMMUTABLE"),
            "the reported image integrity does not name the flag it rests on: {reason}"
        );
        assert!(
            reason.contains("best effort"),
            "the reported image integrity does not say it is best effort: {reason}"
        );
    }
}
