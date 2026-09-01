//! Replacing the store artifact after it is verified cannot change what runs.
//!
//! A digest taken from a path says nothing about the file that path names a
//! moment later, so the property worth proving is not "the digest matched" but
//! "the process that started was the bytes that matched". The two adapters here
//! are distinguishable by side effect alone: the authorized one appends to one
//! log, the impostor to another, and exactly one of those files may exist when
//! the run is over.
//!
//! The swap is synchronized rather than timed. The host stops between holding
//! the verified bytes as a nameless executable descriptor and creating the
//! child, this test replaces the store file while it is stopped, and only then
//! releases it. So the swap is
//! provably inside the window an attacker would need, instead of being a race
//! this test might lose.
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
use std::time::{Duration, Instant};
use tempfile::TempDir;

const RET: [u8; 4] = 0xD65F_03C0u32.to_le_bytes();

/// An adapter that records that it ran and then fails, so the run ends in a
/// typed `NoResult` and the only interesting output is the log line.
fn adapter_source(log: &Path) -> String {
    format!("#!/bin/sh\necho ran >> '{}'\nexit 7\n", log.display())
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

fn ran(log: &Path) -> usize {
    fs::read_to_string(log)
        .map(|text| text.lines().filter(|line| !line.is_empty()).count())
        .unwrap_or(0)
}

fn wait_for(path: &Path, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{what} never appeared at {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Run the rig once and return the refusal, with an optional swap performed
/// while the host is stopped before spawning.
fn run_once(
    installed: &support::Authorized,
    identity: &SnapshotIdentity,
    rendezvous: Option<(&Path, &[u8])>,
) -> HostError {
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

    let Some((dir, impostor)) = rendezvous else {
        return run_adapter(&installed.exec, &input)
            .map(|_| ())
            .expect_err("the fixture adapter writes no result");
    };

    std::thread::scope(|scope| {
        let runner = scope.spawn(|| {
            run_adapter(&installed.exec, &input)
                .map(|_| ())
                .expect_err("the fixture adapter writes no result")
        });

        // The host has verified the artifact and is holding it as a descriptor
        // by the time this file appears, and has not created a child yet.
        wait_for(&dir.join("ready"), "the pre-spawn rendezvous");
        fs::write(&installed.exec, impostor).expect("replace the store artifact");
        assert_eq!(
            fs::read(&installed.exec).expect("read the store artifact"),
            impostor,
            "the store artifact was not actually replaced"
        );
        fs::write(dir.join("go"), b"go").expect("release the rendezvous");

        runner.join().expect("the run thread panicked")
    })
}

#[test]
fn replacing_the_store_artifact_after_verification_cannot_change_what_executes() {
    let dir = TempDir::new().expect("tempdir");
    let authorized_log = dir.path().join("authorized.log");
    let impostor_log = dir.path().join("impostor.log");

    let authorized_source = dir.path().join("authorized_adapter");
    write_executable(&authorized_source, &adapter_source(&authorized_log));
    let impostor_source = dir.path().join("impostor_adapter");
    write_executable(&impostor_source, &adapter_source(&impostor_log));
    let impostor_bytes = fs::read(&impostor_source).expect("read impostor");

    let identity = support::identity();

    // The control comes first: the impostor is a real, working adapter, so a
    // later run that produces no impostor line is evidence about which bytes
    // executed rather than evidence that the impostor could never run at all.
    let control = support::Authorized::install(&impostor_source, &identity);
    let err = run_once(&control, &identity, None);
    assert!(
        matches!(err, HostError::NoResult { .. }),
        "the control adapter did not run: {err}"
    );
    assert_eq!(ran(&impostor_log), 1, "the impostor cannot execute at all");
    fs::remove_file(&impostor_log).expect("reset the impostor log");

    // The race. The registry authorizes the first adapter; the store file is
    // replaced with the second one after verification and before the spawn.
    let installed = support::Authorized::install(&authorized_source, &identity);
    let rendezvous = TempDir::new().expect("tempdir");
    let previous = std::env::var_os(PRESPAWN_RENDEZVOUS_VAR);
    std::env::set_var(PRESPAWN_RENDEZVOUS_VAR, rendezvous.path());
    let err = run_once(
        &installed,
        &identity,
        Some((rendezvous.path(), &impostor_bytes)),
    );
    match previous {
        Some(value) => std::env::set_var(PRESPAWN_RENDEZVOUS_VAR, value),
        None => std::env::remove_var(PRESPAWN_RENDEZVOUS_VAR),
    }

    assert!(
        matches!(err, HostError::NoResult { .. }),
        "the authorized adapter did not run: {err}"
    );
    assert_eq!(
        ran(&authorized_log),
        1,
        "the verified adapter did not execute"
    );
    assert_eq!(
        ran(&impostor_log),
        0,
        "the swapped store bytes executed: {} impostor run(s)",
        ran(&impostor_log)
    );

    // And the swap really did land: the store still holds the impostor, so the
    // run above executed bytes the store path no longer names.
    assert_eq!(
        fs::read(&installed.exec).expect("read the store artifact"),
        impostor_bytes,
        "the store artifact was restored, so nothing was raced"
    );
}
