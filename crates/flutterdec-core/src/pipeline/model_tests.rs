//! The identity gate, proven by what the pipeline does *not* do.
//!
//! A rejected identity is answered by core recovery, so "it returned an error"
//! was never the property worth asserting, and "it returned a model" is not one
//! either: a model would come back whether or not the gate ran. What the gate
//! guarantees is that nothing was selected and nothing was executed. So each
//! rejection case is run against a scratch repo that is rigged to fail loudly at
//! every step the gate is supposed to precede:
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
use flutterdec_loader::layout::Layout;
use flutterdec_loader::registry::{
    canonical_feature_fingerprint, ArtifactReference, CompatibilityEvidence, HostArtifactVariant,
    ParserFamilyReference, ProfileReference, TrustTier,
};
use std::path::PathBuf;
use tempfile::TempDir;

const HASH: &str = "80a49c7111088100a233b2ae788e1f48";
const FEATURES: &str = "product no-code_comments arm64 android compressed-pointers";

/// A scratch repo whose adapter would run if it were ever reached.
struct SpyRepo {
    _dir: TempDir,
    root: PathBuf,
    /// The record the valid registry declares, kept so a test can hand the
    /// library boundary the same authorization the pipeline would build.
    record: Option<CompatibilityRecord>,
    /// The spy repo doubles as both roots: package data and adapter store point
    /// at the same directory, so the rigging stays one tree.
    layout: Layout,
    marker: PathBuf,
}

impl SpyRepo {
    /// `registry` is written verbatim, so a caller can hand it bytes that are
    /// not JSON at all.
    fn new(registry: &str) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let root = dir.path().to_path_buf();
        let marker = root.join("adapter_ran.marker");
        fs::create_dir_all(root.join("artifacts")).expect("mkdir artifacts");
        fs::create_dir_all(root.join("adapters")).expect("mkdir adapters");
        fs::write(root.join("adapters/registry.json"), registry).expect("write registry");

        let exec = root.join("artifacts/flutterdec-spy");
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

        let layout = Layout::new(root.clone(), root.clone(), root.join("symbols"));
        Self {
            _dir: dir,
            root,
            record: None,
            layout,
            marker,
        }
    }

    fn spawned(&self) -> bool {
        self.marker.exists()
    }

    fn write_valid_registry(&mut self) {
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
            .join("artifacts/flutterdec-spy".to_string());
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
                    path: "artifacts/flutterdec-spy".to_string(),
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
        self.record = Some(record.clone());
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
    let mut repo = SpyRepo::new("{}");
    repo.write_valid_registry();
    repo
}
/// A bundle carrying `identity`, with plausible regions so that nothing except
/// the identity can decide the outcome.
fn bundle(identity: SnapshotIdentity) -> SnapshotBundle {
    SnapshotBundle {
        input_path: PathBuf::from("/nonexistent/app.apk"),
        libapp_path: PathBuf::from("/nonexistent/libapp.so"),
        libapp_entry: None,
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

/// Every non-exact identity, through the shared entry every adapter path uses.
///
/// A rejected identity is not an error any more: it is answered by core
/// recovery. What the gate still guarantees is that nothing was selected and
/// nothing was executed, so the assertions are about the absence of a registry
/// read, the absence of a spawn, and the typed reason on the result.
fn assert_stops_before_lookup(identity: SnapshotIdentity, expected: IdentityRejection) {
    let repo = poisoned_registry_repo();
    let mut bundle = bundle(identity);

    let loaded = load_program(&repo.layout, &mut bundle, AdapterBackend::Auto, None)
        .expect("a rejected identity is answered by core recovery");

    assert_eq!(
        loaded.core_fallback,
        Some(CoreFallbackReason::IdentityRejected),
        "the run did not report an identity-driven fallback"
    );
    let detail = loaded
        .core_fallback_detail
        .clone()
        .expect("the fallback quotes the rejection");
    assert_eq!(
        detail,
        expected.to_string(),
        "the fallback quoted a different rejection"
    );
    // The registry in this repo is not JSON. Reading it at all produces a parse
    // failure, so a run that reports a clean core fallback provably never read
    // it.
    assert!(
        loaded.compatibility_record.is_none() && loaded.compatibility.is_none(),
        "a rejected identity was bound to a compatibility record"
    );
    assert!(
        loaded.adapter_exec.is_none() && loaded.containment.is_none(),
        "a rejected identity resolved an executable"
    );
    assert!(
        !repo.spawned(),
        "the adapter was executed for a rejected identity"
    );
    // And the model it produced is honest about what it is.
    assert_eq!(loaded.resolved_backend, BackendId::Internal);
    assert_eq!(loaded.producer.trust, ProducerTrust::Local);
    assert!(loaded.model.libraries.is_empty() && loaded.model.classes.is_empty());
    assert!(loaded.model.object_pool.entries.is_empty());
    assert!(loaded.model.functions.iter().all(|f| f.name.is_none()));
}

/// The same rejection, with a backend only an external tool can serve. Here it
/// is an error: answering `--adapter-backend r2flutter` with prologue scanning
/// is the substitution the protocol refuses inside a run, done one layer up.
fn assert_pinned_external_is_refused(identity: SnapshotIdentity, expected: IdentityRejection) {
    let repo = poisoned_registry_repo();
    let mut bundle = bundle(identity);

    let err = load_program(&repo.layout, &mut bundle, AdapterBackend::R2Flutter, None)
        .expect_err("a pinned external backend cannot be served by core recovery");

    let rendered = format!("{err:#}");
    assert!(
        rendered.contains("identity_rejected") && rendered.contains(&expected.to_string()),
        "the refusal does not name the deterministic reason: {rendered}"
    );
    assert!(
        !rendered.contains("compatibility registry"),
        "the registry was read before the gate: {rendered}"
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
    assert_pinned_external_is_refused(
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
    let mut bundle = bundle(full_aot());

    let err = load_program(&repo.layout, &mut bundle, AdapterBackend::Auto, None)
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
    // And an adapter that ran and failed stays a failure. Core recovery is for
    // snapshots nothing was authorized to parse, not a way to paper over a
    // producer that broke.
    assert_eq!(error_category(&err), "adapter_no_result", "{err:#}");
}

/// An explicitly internal run reads nothing and executes nothing, even where a
/// perfectly good record and a working artifact are installed.
#[test]
fn an_explicitly_internal_run_selects_nothing_and_executes_nothing() {
    let repo = valid_registry_repo();
    let mut bundle = bundle(full_aot());

    let loaded = load_program(&repo.layout, &mut bundle, AdapterBackend::Internal, None)
        .expect("internal recovery does not depend on a registry");

    assert_eq!(
        loaded.core_fallback,
        Some(CoreFallbackReason::InternalRequested)
    );
    assert!(!repo.spawned(), "an internal run executed an adapter");
    assert!(loaded.compatibility_record.is_none());
    assert!(
        bundle.dart_profile.is_none(),
        "an internal run loaded a compatibility profile it never used"
    );
}

/// An exact identity with no record for it is the unknown-snapshot case: core
/// recovers, and says which of the reasons it was.
#[test]
fn an_unknown_hash_is_recovered_by_core_rather_than_refused() {
    let repo = valid_registry_repo();
    let mut bundle = bundle(SnapshotIdentity::from_header(
        TargetArch::Arm64,
        "0123456789abcdef0123456789abcdef",
        SnapshotKind::FullAot,
        FEATURES,
    ));

    let loaded = load_program(&repo.layout, &mut bundle, AdapterBackend::Auto, None)
        .expect("an unknown hash is answered by core recovery");

    assert_eq!(
        loaded.core_fallback,
        Some(CoreFallbackReason::NoCompatibilityRecord)
    );
    assert!(!repo.spawned(), "an unknown hash executed an adapter");
    assert!(
        !loaded.model.functions.is_empty(),
        "core recovery produced no code candidates at all"
    );
    assert!(
        loaded
            .model
            .functions
            .iter()
            .all(|f| f.provenance == Provenance::Heuristic),
        "core recovery claimed a non-heuristic function"
    );
}

/// A rejected identity has no way to become a run with a lesser trust label.
/// The gate is the only exit, so `Untrusted` never reaches a producer record.
#[test]
fn a_rejected_identity_is_not_downgraded_to_an_untrusted_run() {
    let repo = valid_registry_repo();
    let mut bundle = bundle(full_jit());

    let loaded = load_program(&repo.layout, &mut bundle, AdapterBackend::Auto, None)
        .expect("a FullJIT snapshot is answered by core recovery");

    assert_eq!(
        loaded.core_fallback_detail.as_deref(),
        Some(IdentityRejection::NotFullAot(Some(SnapshotKind::FullJit))
            .to_string()
            .as_str())
    );
    assert_ne!(
        loaded.producer.trust,
        ProducerTrust::Untrusted,
        "a rejected identity produced an untrusted run instead of no run"
    );
    assert!(
        !repo.spawned(),
        "a rejected identity ran an adapter as an untrusted producer"
    );
}

/// The library boundary states the same rule for itself: a caller that skipped
/// the core pipeline still cannot spawn an adapter for a rejected identity.
///
/// Every other fact handed in is deliberately wrong: an untrusted producer, a
/// digest for bytes that were never read, and a binding that belongs to no
/// record. The identity gate is first, so it is the one that answers.
#[test]
fn run_adapter_refuses_a_rejected_identity_before_spawn() {
    let repo = valid_registry_repo();
    let bundle = bundle(full_jit());
    let record = repo.record.clone().expect("the valid registry has a record");
    let exec = repo
        .root
        .join("artifacts/flutterdec-spy".to_string());
    let profile_path = repo.root.join("data/test-profile.json");

    let err = run_adapter(
        &exec,
        &AdapterInput {
            identity: &bundle.identity,
            authorization: HostAuthorization {
                record: &record,
                variant: &record.artifact.variants[0],
                store_root: repo.layout.store_dir(),
                profile_path: &profile_path,
            },
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
            libapp: None,
            requested_backend: RequestedBackend::Auto,
            limits: Limits::default(),
        },
    )
    .expect_err("run_adapter cannot run a rejected identity");

    assert_eq!(
        err,
        HostError::IdentityRejected(IdentityRejection::NotFullAot(Some(SnapshotKind::FullJit))),
        "the identity gate must answer before any other fact is looked at"
    );
    assert!(
        err.is_pre_spawn(),
        "an identity rejection is a refusal, not a failed run"
    );
    assert!(
        !repo.spawned(),
        "run_adapter spawned an adapter for a rejected identity"
    );
}
