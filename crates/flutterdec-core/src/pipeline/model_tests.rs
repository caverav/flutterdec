//! The identity gate, proven by what the pipeline does *not* do.
//!
//! Asserting that `load_model` returns an error would not distinguish a snapshot
//! refused before anything happened from one that was looked up, spawned, and
//! then failed. So each rejection case is run against a scratch repo that is
//! rigged to fail loudly at every step the gate is supposed to precede:
//!
//! * `adapters/registry.json` is not valid JSON, so any registry read reports a
//!   parse failure instead of an identity rejection;
//! * an executable is installed under the registry's expected path, so path
//!   resolution would succeed and the failure would come from somewhere else;
//! * that executable is a spy that creates a marker file on its first line, so
//!   a spawn leaves evidence even though the run itself cannot produce a model.
//!
//! A FullAOT control runs against the same rigging with a valid registry and
//! proves the marker *does* appear, so the rejection cases are showing a gate
//! rather than a scratch repo that could never work.
use super::*;
use flutterdec_loader::identity::{
    HashSource, IdentityRejection, SnapshotIdentity, SnapshotKind, TargetArch,
};
use std::path::PathBuf;
use tempfile::TempDir;

const HASH: &str = "80a49c7111088100a233b2ae788e1f48";
const FEATURES: &str = "product no-code_comments arm64 android compressed-pointers";

/// A scratch repo whose adapter would run if it were ever reached.
struct SpyRepo {
    _dir: TempDir,
    root: PathBuf,
    marker: PathBuf,
}

impl SpyRepo {
    /// `registry` is written verbatim, so a caller can hand it bytes that are
    /// not JSON at all.
    fn new(registry: &str) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let root = dir.path().to_path_buf();
        let marker = root.join("adapter_ran.marker");
        fs::create_dir_all(root.join("adapters/installed")).expect("mkdir installed");
        fs::write(root.join("adapters/registry.json"), registry).expect("write registry");

        let exec = root.join(format!("adapters/installed/dart_adapter_{}", HASH));
        fs::write(
            &exec,
            format!(
                "#!/bin/sh\ntouch '{}'\nexit 1\n",
                marker.display()
            ),
        )
        .expect("write spy adapter");
        let mut perms = fs::metadata(&exec).expect("metadata").permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        fs::set_permissions(&exec, perms).expect("chmod spy adapter");

        Self {
            _dir: dir,
            root,
            marker,
        }
    }

    fn spawned(&self) -> bool {
        self.marker.exists()
    }

    fn write_valid_registry(&self) {
        let profile_bytes = serde_json::to_vec_pretty(&serde_json::json!({
            "profiles": {
                "test-profile": {
                    "tag_style": "CID_INT32",
                    "compressed_word_size": 4,
                    "header_fields": 5,
                    "max_alignment": 16,
                    "heap_object_tag": 1,
                    "cids": {"class": 1, "object_pool": 23}
                }
            }
        }))
        .expect("serialize test profile");
        fs::create_dir_all(self.root.join("data")).expect("mkdir data");
        fs::write(self.root.join("data/test-profile.json"), &profile_bytes)
            .expect("write test profile");

        let exec_path = self
            .root
            .join(format!("adapters/installed/dart_adapter_{}", HASH));
        let exec_bytes = fs::read(&exec_path).expect("read spy adapter");
        let features = vec![
            "android".to_string(),
            "arm64".to_string(),
            "compressed-pointers".to_string(),
            "no-code_comments".to_string(),
            "product".to_string(),
        ];
        let record = CompatibilityRecord {
            snapshot_hash: HASH.to_string(),
            snapshot_kind: SnapshotKind::FullAot,
            target_arch: TargetArch::Arm64,
            features: features.clone(),
            feature_fingerprint: canonical_feature_fingerprint(&features),
            known_features: features.clone(),
            forbidden_features: vec!["no-compressed-pointers".to_string()],
            sdk_aliases: vec![SdkAlias {
                ecosystem: "dart".to_string(),
                version: "test".to_string(),
                provenance: "unit fixture".to_string(),
            }],
            parser_family: ParserFamilyReference {
                id: "flutterdec-spy".to_string(),
                version: Some("test".to_string()),
                sha256: None,
            },
            profile: ProfileReference {
                id: "test-profile".to_string(),
                path: "data/test-profile.json".to_string(),
                sha256: Sha256Digest::of(&profile_bytes).to_string(),
            },
            artifact: ArtifactReference {
                id: "flutterdec-spy".to_string(),
                variants: vec![HostArtifactVariant {
                    host_os: std::env::consts::OS.to_string(),
                    host_arch: std::env::consts::ARCH.to_string(),
                    path: format!("adapters/installed/dart_adapter_{}", HASH),
                    size: exec_bytes.len() as u64,
                    sha256: Sha256Digest::of(&exec_bytes).to_string(),
                    provenance: "unit fixture".to_string(),
                }],
            },
            evidence: CompatibilityEvidence {
                source: "unit fixture".to_string(),
                provenance: "unit fixture".to_string(),
                references: Vec::new(),
            },
            trust_tier: TrustTier::Experimental,
            protocol_major: 1,
            model_major: flutterdec_adapter::model::MODEL_VERSION,
        };
        let registry = CompatibilityRegistry {
            version: 1,
            records: vec![record],
        };
        fs::write(
            self.root.join("adapters/registry.json"),
            serde_json::to_vec_pretty(&registry).expect("serialize test registry"),
        )
        .expect("write valid registry");
    }
}

fn poisoned_registry_repo() -> SpyRepo {
    SpyRepo::new("{ this is not json")
}

fn valid_registry_repo() -> SpyRepo {
    let repo = SpyRepo::new("{}");
    repo.write_valid_registry();
    repo
}
/// A bundle carrying `identity`, with plausible regions so that nothing except
/// the identity can decide the outcome.
fn bundle(identity: SnapshotIdentity) -> SnapshotBundle {
    SnapshotBundle {
        input_path: PathBuf::from("/nonexistent/app.apk"),
        libapp_path: PathBuf::from("/nonexistent/libapp.so"),
        arch: identity.target_arch.as_str().to_string(),
        snapshot_hash: identity.hash.clone().unwrap_or_default(),
        vm_data: vec![0u8; 64],
        isolate_data: vec![0u8; 64],
        vm_instr: 0xD65F_03C0u32.to_le_bytes().to_vec(),
        isolate_instr: 0xD65F_03C0u32.to_le_bytes().repeat(4),
        vm_instr_va: 0x1000,
        isolate_instr_va: 0x2000,
        dart_profile: None,
        snapshot_features: identity.features.raw.clone(),
        compressed_pointers: Some(true),
        identity,
    }
}

fn full_aot() -> SnapshotIdentity {
    SnapshotIdentity::from_header(TargetArch::Arm64, HASH, SnapshotKind::FullAot, FEATURES)
}

fn full_jit() -> SnapshotIdentity {
    SnapshotIdentity::from_header(TargetArch::Arm64, HASH, SnapshotKind::FullJit, FEATURES)
}

/// A hash recovered by scanning bytes, with no header behind it.
fn scanned() -> SnapshotIdentity {
    SnapshotIdentity::without_header(TargetArch::Arm64, Some(HASH.to_string()))
}

fn unsupported_target() -> SnapshotIdentity {
    SnapshotIdentity::from_header(
        TargetArch::Unsupported("x64".to_string()),
        HASH,
        SnapshotKind::FullAot,
        "product x64 android compressed-pointers",
    )
}

fn rejection(err: &anyhow::Error) -> IdentityRejection {
    err.chain()
        .find_map(|cause| cause.downcast_ref::<IdentityRejection>())
        .cloned()
        .unwrap_or_else(|| panic!("expected a typed identity rejection, got: {err:#}"))
}

/// Every non-exact identity, through the shared entry every adapter path uses.
fn assert_stops_before_lookup(identity: SnapshotIdentity, expected: IdentityRejection) {
    let repo = poisoned_registry_repo();
    let bundle = bundle(identity);

    let err = load_model(&repo.root, &bundle, AdapterBackend::Auto)
        .expect_err("a rejected identity cannot load a model");

    assert_eq!(rejection(&err), expected, "wrong rejection: {err:#}");
    let rendered = format!("{err:#}");
    assert!(
        !rendered.contains("compatibility registry"),
        "the registry was read before the gate: {rendered}"
    );
    assert!(
        !rendered.contains("adapter artifact"),
        "the executable was resolved before the gate: {rendered}"
    );
    assert!(
        !repo.spawned(),
        "the adapter was executed for a rejected identity"
    );
}

#[test]
fn a_full_jit_snapshot_stops_before_registry_lookup_or_execution() {
    assert_stops_before_lookup(
        full_jit(),
        IdentityRejection::NotFullAot(Some(SnapshotKind::FullJit)),
    );
}

#[test]
fn a_scanned_hash_stops_before_registry_lookup_or_execution() {
    assert_stops_before_lookup(
        scanned(),
        IdentityRejection::HashNotHeaderDerived(HashSource::Scan),
    );
}

#[test]
fn a_snapshot_with_no_recoverable_hash_stops_before_registry_lookup_or_execution() {
    assert_stops_before_lookup(
        SnapshotIdentity::without_header(TargetArch::Arm64, None),
        IdentityRejection::HashNotHeaderDerived(HashSource::Unavailable),
    );
}

#[test]
fn an_unsupported_target_stops_before_registry_lookup_or_execution() {
    assert_stops_before_lookup(
        unsupported_target(),
        IdentityRejection::UnsupportedTarget("x64".to_string()),
    );
}

/// The control. Same rigging, valid registry, exact identity: selection has to
/// reach the executable and spawn it, or the tests above would pass against a
/// repo that simply never works.
#[test]
fn a_full_aot_snapshot_reaches_selection_and_execution() {
    let repo = valid_registry_repo();
    let bundle = bundle(full_aot());

    let err = load_model(&repo.root, &bundle, AdapterBackend::Auto)
        .expect_err("the spy adapter cannot produce a model");

    assert!(
        repo.spawned(),
        "an exact identity did not reach adapter execution: {err:#}"
    );
    assert!(
        err.chain()
            .all(|cause| cause.downcast_ref::<IdentityRejection>().is_none()),
        "an exact identity was refused by the gate: {err:#}"
    );
}

/// A rejected identity has no way to become a run with a lesser trust label.
/// The gate is the only exit, so `Untrusted` never reaches a producer record.
#[test]
fn a_rejected_identity_is_not_downgraded_to_an_untrusted_run() {
    let repo = valid_registry_repo();
    let bundle = bundle(full_jit());

    let err = load_model(&repo.root, &bundle, AdapterBackend::Auto)
        .expect_err("a FullJIT snapshot cannot load a model");

    assert_eq!(
        rejection(&err),
        IdentityRejection::NotFullAot(Some(SnapshotKind::FullJit))
    );
    assert!(
        !repo.spawned(),
        "a rejected identity ran an adapter as an untrusted producer"
    );
}

/// The library boundary states the same rule for itself: a caller that skipped
/// the core pipeline still cannot spawn an adapter for a rejected identity.
#[test]
fn run_adapter_refuses_a_rejected_identity_before_spawn() {
    let repo = valid_registry_repo();
    let bundle = bundle(full_jit());
    let exec = repo.root.join(format!("adapters/installed/dart_adapter_{}", HASH));

    let err = run_adapter(
        &exec,
        &AdapterInput {
            identity: &bundle.identity,
            producer: Producer {
                id: "flutterdec-local-python".to_string(),
                version: "unknown".to_string(),
                artifact_sha256: Sha256Digest::of(b"spy"),
                trust: ProducerTrust::Untrusted,
            },
            compatibility: CompatibilityBinding {
                record_sha256: Sha256Digest::of(b"gate test record"),
                parser_family_id: "flutterdec-local-python".to_string(),
                profile_id: "unresolved".to_string(),
                profile_sha256: Sha256Digest::of(b"gate test profile"),
            },
            regions: vec![
                AdapterRegionInput {
                    region: InputRegionName::VmData,
                    bytes: &bundle.vm_data,
                    virtual_address: None,
                },
                AdapterRegionInput {
                    region: InputRegionName::IsolateData,
                    bytes: &bundle.isolate_data,
                    virtual_address: None,
                },
                AdapterRegionInput {
                    region: InputRegionName::VmInstructions,
                    bytes: &bundle.vm_instr,
                    virtual_address: Some(bundle.vm_instr_va),
                },
                AdapterRegionInput {
                    region: InputRegionName::IsolateInstructions,
                    bytes: &bundle.isolate_instr,
                    virtual_address: Some(bundle.isolate_instr_va),
                },
            ],
            input_path: None,
            libapp_path: None,
            requested_backend: RequestedBackend::Auto,
        },
    )
        .expect_err("run_adapter cannot run a rejected identity");

    assert_eq!(
        rejection(&err),
        IdentityRejection::NotFullAot(Some(SnapshotKind::FullJit))
    );
    assert!(
        !repo.spawned(),
        "run_adapter spawned an adapter for a rejected identity"
    );
}
