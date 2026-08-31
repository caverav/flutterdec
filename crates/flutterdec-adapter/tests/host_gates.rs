//! Every pre-spawn gate, against a real executable that would leave evidence.
//!
//! Asserting that `run_adapter` returned an error would not distinguish a run
//! refused before anything happened from one that started, failed, and cleaned
//! up after itself. So the authorized artifact in every case here is a real
//! executable whose first line creates a marker file outside the workspace. A
//! gate case then has to show three things at once: the exact typed refusal,
//! that the refusal is classified as pre-spawn, and that the marker is absent.
//!
//! The control at the end runs the same rig with nothing wrong and requires the
//! marker to appear. Without it, every assertion above would also pass against a
//! rig that could never spawn anything at all.

mod support;

use flutterdec_adapter::model::{InputRegionName, ProducerTrust};
use flutterdec_adapter::primitives::Sha256Digest;
use flutterdec_adapter::protocol::RequestedBackend;
use flutterdec_adapter::registry::{
    canonical_feature_fingerprint, CompatibilityRecord, HostArtifactVariant,
};
use flutterdec_adapter::{
    run_adapter, AdapterInput, AdapterRegionInput, HostAuthorization, HostError, LibappSource,
    Limits,
};
use flutterdec_loader::identity::{SnapshotIdentity, SnapshotKind, TargetArch};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const RET: [u8; 4] = 0xD65F_03C0u32.to_le_bytes();

/// A rig whose adapter would run, loudly, if a gate ever let it.
struct Rig {
    _dir: TempDir,
    marker: PathBuf,
    /// The spy before it was published, which is a real executable outside the
    /// adapter store.
    unpublished: PathBuf,
    installed: support::Authorized,
    identity: SnapshotIdentity,
    regions: Vec<Vec<u8>>,
}

impl Rig {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let marker = dir.path().join("adapter_ran.marker");
        let unpublished = dir.path().join("spy_adapter");
        fs::write(
            &unpublished,
            format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
        )
        .expect("write spy");
        fs::set_permissions(&unpublished, fs::Permissions::from_mode(0o755)).expect("chmod spy");

        let identity = support::identity();
        let installed = support::Authorized::install(&unpublished, &identity);
        Self {
            _dir: dir,
            marker,
            unpublished,
            installed,
            identity,
            // Four non-empty regions, so nothing except the gate under test can
            // decide the outcome.
            regions: vec![vec![0u8; 64], vec![0u8; 64], RET.to_vec(), RET.repeat(4)],
        }
    }

    fn spawned(&self) -> bool {
        self.marker.exists()
    }

    fn region_inputs(&self) -> Vec<AdapterRegionInput<'_>> {
        vec![
            AdapterRegionInput {
                region: InputRegionName::VmData,
                bytes: &self.regions[0],
                virtual_address: None,
            },
            AdapterRegionInput {
                region: InputRegionName::IsolateData,
                bytes: &self.regions[1],
                virtual_address: None,
            },
            AdapterRegionInput {
                region: InputRegionName::VmInstructions,
                bytes: &self.regions[2],
                virtual_address: Some(0x1000),
            },
            AdapterRegionInput {
                region: InputRegionName::IsolateInstructions,
                bytes: &self.regions[3],
                virtual_address: Some(0x2000),
            },
        ]
    }

    /// Everything correct. Each case below changes exactly one thing.
    fn input<'a>(&'a self, record: &'a CompatibilityRecord) -> AdapterInput<'a> {
        AdapterInput {
            identity: &self.identity,
            authorization: self.installed.authorization_for(record),
            producer: self.installed.producer_for(record),
            compatibility: self.installed.binding_for(record),
            regions: self.region_inputs(),
            input_path: None,
            libapp: None,
            requested_backend: RequestedBackend::Auto,
            limits: Limits::default(),
        }
    }

    fn run(&self, input: &AdapterInput<'_>) -> Result<(), HostError> {
        run_adapter(&self.installed.exec, input).map(|_| ())
    }

    /// Run and require a refusal that happened before any process existed.
    fn refuse(&self, input: &AdapterInput<'_>) -> HostError {
        let err = self
            .run(input)
            .expect_err("the gate under test must refuse this input");
        assert!(
            err.is_pre_spawn(),
            "{err} is not classified as a pre-spawn refusal"
        );
        assert!(
            !self.spawned(),
            "a gate refused after the adapter had already run: {err}"
        );
        err
    }
}

fn other_record(rig: &Rig, edit: impl FnOnce(&mut CompatibilityRecord)) -> CompatibilityRecord {
    let mut record = rig.installed.record.clone();
    edit(&mut record);
    record
}

#[test]
fn a_snapshot_that_is_not_full_aot_never_reaches_the_registry() {
    let mut rig = Rig::new();
    rig.identity = SnapshotIdentity::from_header(
        TargetArch::Arm64,
        support::HASH,
        SnapshotKind::FullJit,
        support::FEATURES,
    );
    let record = rig.installed.record.clone();
    let err = rig.refuse(&rig.input(&record));
    assert!(
        matches!(err, HostError::IdentityRejected(_)),
        "wrong refusal: {err}"
    );
}

#[test]
fn a_record_that_breaks_its_own_invariants_is_refused() {
    let rig = Rig::new();
    // Features out of order. The registry's own validator rejects it, and the
    // host does not get to accept a record the registry would not.
    let record = other_record(&rig, |record| record.features.reverse());
    let err = rig.refuse(&rig.input(&record));
    assert!(
        matches!(err, HostError::RecordInvalid(_)),
        "wrong refusal: {err}"
    );
}

#[test]
fn a_binding_that_names_another_record_is_refused() {
    let rig = Rig::new();
    let record = rig.installed.record.clone();
    let mut input = rig.input(&record);
    input.compatibility.record_sha256 = Sha256Digest::of(b"some other record");
    let err = rig.refuse(&input);
    assert!(
        matches!(err, HostError::RecordDigestMismatch { .. }),
        "wrong refusal: {err}"
    );
}

#[test]
fn a_record_written_for_another_protocol_or_model_major_is_refused() {
    let rig = Rig::new();
    let record = other_record(&rig, |record| record.protocol_major = 2);
    let err = rig.refuse(&rig.input(&record));
    assert!(
        matches!(
            err,
            HostError::UnsupportedMajors {
                record_protocol: 2,
                record_model: 4
            }
        ),
        "wrong refusal: {err}"
    );
}

#[test]
fn a_record_for_another_snapshot_is_refused() {
    let rig = Rig::new();
    let record = other_record(&rig, |record| {
        record.snapshot_hash = "0123456789abcdef0123456789abcdef".to_string()
    });
    let err = rig.refuse(&rig.input(&record));
    assert!(
        matches!(err, HostError::IdentityRecordMismatch { .. }),
        "wrong refusal: {err}"
    );
}

#[test]
fn a_record_for_another_target_architecture_is_refused() {
    let rig = Rig::new();
    let record = other_record(&rig, |record| {
        record.target_arch = TargetArch::Unsupported("x64".to_string())
    });
    let err = rig.refuse(&rig.input(&record));
    assert!(
        matches!(err, HostError::TargetMismatch { .. }),
        "wrong refusal: {err}"
    );
}

#[test]
fn a_record_for_another_feature_tuple_is_refused() {
    let rig = Rig::new();
    let record = other_record(&rig, |record| {
        record.features.retain(|feature| feature != "product");
        record.feature_fingerprint = canonical_feature_fingerprint(&record.features);
    });
    let err = rig.refuse(&rig.input(&record));
    assert!(
        matches!(err, HostError::FeatureMismatch { .. }),
        "wrong refusal: {err}"
    );
}

#[test]
fn a_variant_the_record_does_not_declare_is_refused() {
    let rig = Rig::new();
    let record = rig.installed.record.clone();
    let mut smuggled: HostArtifactVariant = record.artifact.variants[0].clone();
    smuggled.provenance = "not the variant the record declares".to_string();
    let mut input = rig.input(&record);
    input.authorization = HostAuthorization {
        variant: &smuggled,
        ..input.authorization
    };
    let err = rig.refuse(&input);
    assert!(
        matches!(err, HostError::VariantNotInRecord { .. }),
        "wrong refusal: {err}"
    );
}

#[test]
fn an_artifact_variant_for_another_host_is_refused() {
    let rig = Rig::new();
    // The host architecture and the target architecture are different facts. A
    // record can be right about the snapshot and still name an executable this
    // machine cannot run.
    let record = other_record(&rig, |record| {
        record.artifact.variants[0].host_os = "plan9".to_string()
    });
    let err = rig.refuse(&rig.input(&record));
    assert!(
        matches!(err, HostError::HostVariantMismatch { .. }),
        "wrong refusal: {err}"
    );
}

#[test]
fn an_executable_outside_the_adapter_store_is_refused() {
    let rig = Rig::new();
    let record = rig.installed.record.clone();
    // Byte-identical to the authorized artifact, and in the wrong place.
    let err = run_adapter(&rig.unpublished, &rig.input(&record))
        .expect_err("an executable outside the store cannot run");
    assert!(err.is_pre_spawn(), "{err} is not a pre-spawn refusal");
    assert!(!rig.spawned(), "the out-of-store executable ran: {err}");
    assert!(
        matches!(err, HostError::ArtifactPathRejected(_)),
        "wrong refusal: {err}"
    );
}

#[test]
fn an_artifact_that_is_not_executable_is_refused() {
    let rig = Rig::new();
    fs::set_permissions(&rig.installed.exec, fs::Permissions::from_mode(0o644))
        .expect("drop the execute bit");
    let record = rig.installed.record.clone();
    let err = rig.refuse(&rig.input(&record));
    assert!(
        matches!(err, HostError::ArtifactNotExecutable(_)),
        "wrong refusal: {err}"
    );
}

#[test]
fn an_artifact_that_changed_since_it_was_registered_is_refused() {
    let rig = Rig::new();
    let mut bytes = fs::read(&rig.installed.exec).expect("read artifact");
    bytes.extend_from_slice(b"\n# appended after the registry verified it\n");
    fs::write(&rig.installed.exec, &bytes).expect("rewrite artifact");
    let record = rig.installed.record.clone();
    let err = rig.refuse(&rig.input(&record));
    assert!(
        matches!(err, HostError::ArtifactDigestMismatch { .. }),
        "wrong refusal: {err}"
    );
}

#[test]
fn a_producer_record_that_does_not_follow_from_the_registry_is_refused() {
    let rig = Rig::new();
    let record = rig.installed.record.clone();

    for (label, edit) in [
        (
            "a digest for bytes that are not the ones being executed",
            Box::new(|input: &mut AdapterInput<'_>| {
                input.producer.artifact_sha256 = Sha256Digest::of(b"some other artifact")
            }) as Box<dyn FnOnce(&mut AdapterInput<'_>)>,
        ),
        (
            "a parser family the record does not name",
            Box::new(|input: &mut AdapterInput<'_>| input.producer.id = "someone-else".to_string()),
        ),
        (
            "a trust level a registry-authorized run cannot carry",
            Box::new(|input: &mut AdapterInput<'_>| input.producer.trust = ProducerTrust::Local),
        ),
    ] {
        let mut input = rig.input(&record);
        edit(&mut input);
        let err = rig.refuse(&input);
        assert!(
            matches!(err, HostError::ProducerMismatch(_)),
            "{label}: wrong refusal: {err}"
        );
    }
}

#[test]
fn a_profile_that_does_not_match_the_records_digest_is_refused() {
    let rig = Rig::new();
    fs::write(
        &rig.installed.profile_path,
        br#"{"profiles":{"swapped":{}}}"#,
    )
    .expect("swap the profile");
    let record = rig.installed.record.clone();
    let err = rig.refuse(&rig.input(&record));
    assert!(
        matches!(err, HostError::ProfileRejected(_)),
        "wrong refusal: {err}"
    );
}

#[test]
fn a_binding_that_does_not_follow_from_the_record_is_refused() {
    let rig = Rig::new();
    let record = rig.installed.record.clone();
    let mut input = rig.input(&record);
    input.compatibility.profile_id = "a profile the record does not name".to_string();
    let err = rig.refuse(&input);
    assert!(
        matches!(err, HostError::BindingMismatch(_)),
        "wrong refusal: {err}"
    );
}

#[test]
fn an_unusable_snapshot_region_is_refused() {
    let rig = Rig::new();
    let record = rig.installed.record.clone();

    let empty: Vec<u8> = Vec::new();
    let mut input = rig.input(&record);
    input.regions[0].bytes = &empty;
    let err = rig.refuse(&input);
    assert!(
        matches!(err, HostError::InputRejected(_)),
        "an empty region: wrong refusal: {err}"
    );

    let mut input = rig.input(&record);
    input.limits.max_region_bytes = 4;
    let err = rig.refuse(&input);
    assert!(
        matches!(err, HostError::InputRejected(_)),
        "an oversized region: wrong refusal: {err}"
    );
}

#[test]
fn a_request_the_host_would_not_answer_is_never_asked() {
    let rig = Rig::new();
    let record = rig.installed.record.clone();
    let mut input = rig.input(&record);
    input.regions.truncate(3);
    let err = rig.refuse(&input);
    assert!(
        matches!(err, HostError::RequestRejected(_)),
        "wrong refusal: {err}"
    );
}

/// The control.
///
/// Everything above asserts that a marker did not appear. That claim is worth
/// nothing unless the same rig, unmodified, does produce the marker.
#[test]
fn the_same_rig_with_nothing_wrong_reaches_the_executable() {
    let rig = Rig::new();
    let record = rig.installed.record.clone();
    let err = rig
        .run(&rig.input(&record))
        .expect_err("the spy exits 1 without writing a result");
    assert!(
        !err.is_pre_spawn(),
        "an authorized run must fail as a run, not as a refusal: {err}"
    );
    assert!(
        matches!(err, HostError::NoResult { .. }),
        "wrong failure: {err}"
    );
    assert!(
        rig.spawned(),
        "the control did not reach the executable, so no gate case proves anything"
    );
}

/// An APK member is not a path, and a backend that opens `--libapp-path` needs
/// one. The host writes the member into the private invocation directory.
#[test]
fn an_archive_member_is_materialized_into_a_real_path() {
    let dir = TempDir::new().expect("tempdir");
    let recorder = dir.path().join("recorder");
    let input_path = dir.path().join("app.apk");
    fs::write(&input_path, b"not really a zip").expect("write input");
    // The adapter records what it was handed, next to the input path, which is
    // the one location outside its private workspace that it knows about.
    fs::write(
        &recorder,
        r#"#!/usr/bin/env python3
import argparse, pathlib

p = argparse.ArgumentParser()
p.add_argument("--request", required=True)
p.add_argument("--result", required=True)
p.add_argument("--input-path")
p.add_argument("--libapp-path")
args = p.parse_args()
pathlib.Path(args.input_path + ".libapp").write_text(args.libapp_path or "")
raise SystemExit(1)
"#,
    )
    .expect("write recorder");
    fs::set_permissions(&recorder, fs::Permissions::from_mode(0o755)).expect("chmod");

    let identity = support::identity();
    let installed = support::Authorized::install(&recorder, &identity);
    let regions = [vec![0u8; 64], vec![0u8; 64], RET.to_vec(), RET.repeat(4)];
    let member = b"\x7fELF pretend shared object".to_vec();

    let err = run_adapter(
        &installed.exec,
        &AdapterInput {
            identity: &identity,
            authorization: installed.authorization(),
            producer: installed.producer(),
            compatibility: installed.binding(),
            regions: vec![
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
            ],
            input_path: Some(&input_path),
            libapp: Some(LibappSource::Member {
                name: "lib/arm64-v8a/libapp.so",
                bytes: &member,
            }),
            requested_backend: RequestedBackend::Auto,
            limits: Limits::default(),
        },
    )
    .expect_err("the recorder writes no result");
    assert!(
        matches!(err, HostError::NoResult { .. }),
        "unexpected: {err}"
    );

    let handed = fs::read_to_string(dir.path().join("app.apk.libapp"))
        .expect("the adapter recorded its --libapp-path");
    let handed = Path::new(handed.trim());
    assert!(
        handed.is_absolute(),
        "the adapter was handed {handed:?}, which it cannot open from anywhere"
    );
    assert_eq!(
        handed.file_name().and_then(|name| name.to_str()),
        Some("libapp.so"),
        "the materialized member kept its file name"
    );
    // The workspace is private and torn down, so the path is gone by now; what
    // matters is that it was a real path under the invocation directory rather
    // than the zip entry name.
    assert_ne!(handed, Path::new("lib/arm64-v8a/libapp.so"));
}
