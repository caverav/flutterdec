use flutterdec_adapter::model::{
    CompatibilityBinding, InputRegionName, Producer, ProducerTrust, MODEL_VERSION,
};
use flutterdec_adapter::primitives::{RelativePath, Sha256Digest};
use flutterdec_adapter::protocol::{
    AdapterErrorCode, AdapterRequest, AdapterResult, AdapterStatus, BackendId, InputHandle,
    RequestedBackend, PROFILE_PATH, PROTOCOL_MAJOR,
};
use flutterdec_adapter::registry::{
    canonical_feature_fingerprint, CompatibilityRecord, CompatibilityRegistry,
};
use flutterdec_adapter::{
    run_adapter, store, AdapterInput, AdapterRegionInput, HostAuthorization, Limits,
};
use flutterdec_loader::identity::{SnapshotIdentity, SnapshotKind, TargetArch};
use flutterdec_loader::layout::Layout;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

const HASH: &str = "80a49c7111088100a233b2ae788e1f48";
const FEATURES: &str = "product arm64 android compressed-pointers";

fn adapter_binary() -> PathBuf {
    let declared = PathBuf::from(env!("CARGO_BIN_EXE_flutterdec-serwalker"));
    if declared.is_file() {
        return declared;
    }
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("flutterdec-serwalker")
}

fn profile_bytes(identity: &SnapshotIdentity) -> Vec<u8> {
    let directory = std::env::var_os("FLUTTERDEC_PROFILES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/serwalker_profiles")
        });
    let name = flutterdec_serwalker::profile_file_name(
        identity.hash.as_ref().unwrap(),
        &identity.features.normalized,
    )
    .unwrap();
    fs::read(directory.join(name)).unwrap()
}

fn header(kind: i64, hash: &str) -> Vec<u8> {
    let mut bytes = vec![0xf5, 0xf5, 0xdc, 0xdc];
    bytes.extend_from_slice(&((20 + hash.len() + FEATURES.len() + 1) as u64).to_le_bytes());
    bytes.extend_from_slice(&kind.to_le_bytes());
    bytes.extend_from_slice(hash.as_bytes());
    bytes.extend_from_slice(FEATURES.as_bytes());
    bytes.push(0);
    bytes
}

struct Invocation {
    directory: TempDir,
    request: AdapterRequest,
}

impl Invocation {
    fn new(data: Vec<u8>) -> Self {
        let directory = TempDir::new().unwrap();
        fs::create_dir(directory.path().join("in")).unwrap();
        fs::create_dir(directory.path().join("out")).unwrap();
        let identity =
            SnapshotIdentity::from_header(TargetArch::Arm64, HASH, SnapshotKind::FullAot, FEATURES);
        let mut inputs = Vec::new();
        for (region, bytes) in [
            (InputRegionName::VmData, data.clone()),
            (InputRegionName::IsolateData, data),
            (InputRegionName::VmInstructions, vec![0; 64]),
            (InputRegionName::IsolateInstructions, vec![0; 64]),
        ] {
            let path = RelativePath::parse(&format!("in/{region}.bin")).unwrap();
            fs::write(directory.path().join(path.as_str()), &bytes).unwrap();
            inputs.push(InputHandle {
                region,
                path,
                size: bytes.len() as u64,
                sha256: Sha256Digest::of(&bytes),
                virtual_address: region.is_executable().then_some(
                    if region == InputRegionName::VmInstructions {
                        0x1000
                    } else {
                        0x2000
                    },
                ),
                executable: region.is_executable(),
            });
        }
        let request = AdapterRequest {
            protocol_major: PROTOCOL_MAJOR,
            model_major: MODEL_VERSION,
            compatibility: CompatibilityBinding {
                record_sha256: Sha256Digest::of(b"test-record"),
                parser_family_id: "flutterdec-serwalker".into(),
                profile_id: "test-profile".into(),
                profile_sha256: Sha256Digest::of(&profile_bytes(&identity)),
            },
            producer: Producer {
                id: "flutterdec-serwalker".into(),
                version: "test".into(),
                artifact_sha256: Sha256Digest::of(&fs::read(adapter_binary()).unwrap()),
                trust: ProducerTrust::Registered,
            },
            identity,
            requested_backend: RequestedBackend::Fixed(BackendId::Serwalker),
            inputs,
            output: RelativePath::parse("out/model.json").unwrap(),
        };
        Self { directory, request }
    }

    fn stage_profile(&self) {
        fs::write(
            self.directory.path().join(PROFILE_PATH),
            profile_bytes(&self.request.identity),
        )
        .unwrap();
    }

    fn run(&self) -> AdapterResult {
        fs::write(
            self.directory.path().join("request.json"),
            self.request.to_json(),
        )
        .unwrap();
        let output = Command::new(adapter_binary())
            .current_dir(self.directory.path())
            .args(["--request", "request.json", "--result", "result.json"])
            .env_clear()
            .output()
            .unwrap();
        let bytes = fs::read(self.directory.path().join("result.json"))
            .unwrap_or_else(|cause| panic!("{cause}: {}", String::from_utf8_lossy(&output.stderr)));
        let result = AdapterResult::from_json(&bytes).unwrap();
        assert_eq!(output.status.success(), result.status == AdapterStatus::Ok);
        result.validate_against(&self.request).unwrap();
        result
    }
}

#[test]
fn rejected_identity_is_reported_before_inputs_or_profile_are_opened() {
    let mut invocation = Invocation::new(header(3, HASH));
    invocation.request.identity.kind = Some(SnapshotKind::FullJit);
    invocation.request.inputs[0].path = RelativePath::parse("in/absent.bin").unwrap();
    let result = invocation.run();
    assert_eq!(result.status, AdapterStatus::Unsupported);
    assert_eq!(
        result.error.unwrap().code,
        AdapterErrorCode::UnsupportedSnapshot
    );
    assert!(!invocation.directory.path().join("out/model.json").exists());
}

#[test]
fn payload_identity_is_checked_before_the_profile_is_opened() {
    for data in [
        header(2, HASH),
        header(3, "0123456789abcdef0123456789abcdef"),
    ] {
        let invocation = Invocation::new(data);
        let result = invocation.run();
        assert_eq!(
            result.error.unwrap().code,
            AdapterErrorCode::IdentityMismatch
        );
    }
}

#[test]
fn internal_requests_are_not_routed_through_serwalker() {
    let mut invocation = Invocation::new(header(3, HASH));
    invocation.request.requested_backend = RequestedBackend::Fixed(BackendId::Internal);
    let result = invocation.run();
    assert_eq!(result.status, AdapterStatus::Unsupported);
    assert_eq!(
        result.error.unwrap().code,
        AdapterErrorCode::UnsupportedSnapshot
    );
}

#[test]
fn input_and_profile_digests_are_checked() {
    let mut invocation = Invocation::new(header(3, HASH));
    invocation.request.inputs[0].sha256 = Sha256Digest::of(b"different");
    assert_eq!(
        invocation.run().error.unwrap().code,
        AdapterErrorCode::InputDigestMismatch
    );

    let invocation = Invocation::new(header(3, HASH));
    fs::write(invocation.directory.path().join(PROFILE_PATH), b"{}").unwrap();
    assert_eq!(
        invocation.run().error.unwrap().code,
        AdapterErrorCode::InputDigestMismatch
    );
}

#[test]
fn parse_errors_never_become_heuristic_output_under_fixed_or_auto_selection() {
    for backend in [
        RequestedBackend::Auto,
        RequestedBackend::Fixed(BackendId::Serwalker),
    ] {
        let mut invocation = Invocation::new(header(3, HASH));
        invocation.request.requested_backend = backend;
        invocation.stage_profile();
        let result = invocation.run();
        assert_eq!(result.status, AdapterStatus::Failed);
        assert_eq!(result.error.unwrap().code, AdapterErrorCode::ParseFailed);
        assert!(result.model.is_none() && result.resolved_backend.is_none());
        assert!(!invocation.directory.path().join("out/model.json").exists());
    }
}

#[test]
#[ignore = "requires SERWALKER_TEST_LIBAPP and SERWALKER_TEST_PROFILE"]
fn a_registry_authorized_serwalker_process_parses_a_real_snapshot() {
    let libapp = PathBuf::from(std::env::var_os("SERWALKER_TEST_LIBAPP").unwrap());
    let profile = fs::read(PathBuf::from(
        std::env::var_os("SERWALKER_TEST_PROFILE").unwrap(),
    ))
    .unwrap();
    let bundle = flutterdec_loader::load_snapshot_bundle(&libapp).unwrap();
    let source = adapter_binary();
    let artifact = fs::read(&source).unwrap();
    let directory = TempDir::new().unwrap();
    let data = directory.path().join("data");
    fs::create_dir_all(data.join("adapters")).unwrap();
    fs::write(data.join("profile.json"), &profile).unwrap();
    let layout = Layout::new(
        data.clone(),
        directory.path().join("store"),
        directory.path().join("symbols"),
    );
    let identity = &bundle.identity;
    let record: CompatibilityRecord = serde_json::from_value(serde_json::json!({
        "snapshot_hash": identity.hash,
        "snapshot_kind": identity.kind,
        "target_arch": identity.target_arch,
        "features": identity.features.normalized,
        "feature_fingerprint": canonical_feature_fingerprint(&identity.features.normalized),
        "parser_family": {"id": "flutterdec-serwalker", "version": env!("CARGO_PKG_VERSION")},
        "profile": {"id": "fixture-profile", "path": "profile.json", "sha256": Sha256Digest::of(&profile)},
        "artifact": {"id": "flutterdec-serwalker", "variants": [{
            "host_os": std::env::consts::OS, "host_arch": std::env::consts::ARCH,
            "path": "artifacts/flutterdec-serwalker", "size": artifact.len(),
            "sha256": Sha256Digest::of(&artifact), "provenance": "real snapshot integration test"
        }]},
        "evidence": {"source": "SERWALKER_TEST_LIBAPP", "provenance": "real snapshot integration test"},
        "trust_tier": "experimental", "protocol_major": PROTOCOL_MAJOR, "model_major": MODEL_VERSION
    })).unwrap();
    let registry = CompatibilityRegistry {
        version: 1,
        records: vec![record.clone()],
    };
    fs::write(
        layout.registry_path(),
        serde_json::to_vec(&registry).unwrap(),
    )
    .unwrap();
    let selection = CompatibilityRegistry::load(&layout.registry_path())
        .unwrap()
        .select(identity)
        .unwrap();
    let loaded_profile = selection.load_profile(layout.data_dir()).unwrap();
    assert!(loaded_profile.profile.is_none());
    store::install(
        &layout,
        &registry,
        identity.hash.as_ref().unwrap(),
        None,
        Some(&source),
    )
    .unwrap();
    let installed = selection
        .resolve_current_artifact(layout.store_dir())
        .unwrap();
    let compatibility = CompatibilityBinding {
        record_sha256: Sha256Digest::parse(&record.sha256().unwrap()).unwrap(),
        parser_family_id: record.parser_family.id.clone(),
        profile_id: record.profile.id.clone(),
        profile_sha256: Sha256Digest::of(&profile),
    };
    let producer = Producer {
        id: record.parser_family.id.clone(),
        version: env!("CARGO_PKG_VERSION").into(),
        artifact_sha256: Sha256Digest::of(&artifact),
        trust: ProducerTrust::Registered,
    };
    let profile_path = data.join("profile.json");
    let mut input = AdapterInput {
        identity,
        authorization: HostAuthorization {
            record: &record,
            variant: &installed.variant,
            store_root: layout.store_dir(),
            profile_path: &profile_path,
        },
        producer: producer.clone(),
        compatibility: compatibility.clone(),
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
        requested_backend: RequestedBackend::Fixed(BackendId::Serwalker),
        limits: Limits::default(),
    };
    let run = run_adapter(&installed.path, &input).unwrap();
    assert_eq!(run.resolved_backend, BackendId::Serwalker);
    assert!(run.fallback_reason.is_none());
    assert_eq!(run.model.producer, producer);
    assert_eq!(run.model.compatibility, Some(compatibility));
    assert!(!run.model.functions.is_empty() && !run.model.classes.is_empty());
    input.requested_backend = RequestedBackend::Auto;
    let auto = run_adapter(&installed.path, &input).unwrap();
    assert_eq!(auto.resolved_backend, BackendId::Serwalker);
    assert_eq!(auto.model, run.model);
    eprintln!(
        "external Serwalker: {} classes, {} functions",
        run.model.classes.len(),
        run.model.functions.len()
    );
}
