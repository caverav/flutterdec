//! Adapter protocol v1 contract tests.
//!
//! Same rule as the model suite: everything goes through serialized bytes, so
//! what is under test is what an adapter can actually put on disk.

mod support;

use flutterdec_adapter::model::{InputRegionName, MODEL_VERSION};
use flutterdec_adapter::primitives::{RelativePath, Sha256Digest};
use flutterdec_adapter::protocol::{
    AdapterError, AdapterErrorCode, AdapterRequest, AdapterResult, AdapterStatus, InputHandle,
    ProtocolError, PROTOCOL_MAJOR,
};
use serde_json::{json, Value};
use support::{ISO_INSTR_VA, VM_INSTR_VA};

fn path(text: &str) -> RelativePath {
    RelativePath::parse(text).expect("fixture path is valid")
}

fn handle(region: InputRegionName, file: &str, size: u64, va: Option<u64>) -> InputHandle {
    InputHandle {
        region,
        path: path(file),
        size,
        sha256: Sha256Digest::of(file.as_bytes()),
        virtual_address: va,
        executable: region.is_executable(),
    }
}

fn request() -> AdapterRequest {
    AdapterRequest {
        protocol_major: PROTOCOL_MAJOR,
        model_major: MODEL_VERSION,
        compatibility_record_sha256: Sha256Digest::of(b"record"),
        identity: support::identity(),
        inputs: vec![
            handle(InputRegionName::VmData, "in/vm_data.bin", 64, None),
            handle(InputRegionName::IsolateData, "in/iso_data.bin", 128, None),
            handle(
                InputRegionName::VmInstructions,
                "in/vm_instr.bin",
                support::VM_INSTR_SIZE,
                Some(VM_INSTR_VA),
            ),
            handle(
                InputRegionName::IsolateInstructions,
                "in/iso_instr.bin",
                support::ISO_INSTR_SIZE,
                Some(ISO_INSTR_VA),
            ),
        ],
        output: path("out/model.json"),
    }
}

fn request_json() -> Value {
    serde_json::from_slice(&request().to_json()).expect("request serializes to json")
}

fn mutated(mut value: Value, mutate: impl FnOnce(&mut Value)) -> Vec<u8> {
    mutate(&mut value);
    serde_json::to_vec(&value).expect("mutated fixture serializes")
}

#[test]
fn a_request_round_trips_through_fresh_json() {
    let original = request();
    let parsed = AdapterRequest::from_json(&original.to_json()).expect("request is valid");
    assert_eq!(parsed, original);
    assert_eq!(parsed.to_json(), original.to_json());
    assert_eq!(
        parsed
            .input(InputRegionName::IsolateInstructions)
            .map(|i| i.virtual_address),
        Some(Some(ISO_INSTR_VA))
    );
}

#[test]
fn a_result_round_trips_in_both_its_shapes() {
    let ok = AdapterResult::ok(path("out/model.json"), Vec::new());
    assert_eq!(
        AdapterResult::from_json(&ok.to_json()).expect("ok result"),
        ok
    );

    let failed = AdapterResult::failed(AdapterErrorCode::ParseFailed, "isolate data truncated");
    let parsed = AdapterResult::from_json(&failed.to_json()).expect("failed result");
    assert_eq!(parsed, failed);
    assert_eq!(parsed.status, AdapterStatus::Failed);
    assert_eq!(
        parsed.error.as_ref().map(|e| e.code),
        Some(AdapterErrorCode::ParseFailed)
    );

    // `Unsupported` is a distinct outcome from `Failed`: retrying will not help.
    let unsupported =
        AdapterResult::unsupported(AdapterErrorCode::UnsupportedSnapshot, "no parser family");
    let parsed = AdapterResult::from_json(&unsupported.to_json()).expect("unsupported result");
    assert_eq!(parsed.status, AdapterStatus::Unsupported);
}

/// Version negotiation is a rejection, not a translation. A document written for
/// another protocol or model major has to stop here rather than be interpreted
/// under this build's field meanings.
#[test]
fn unsupported_majors_are_rejected_on_both_documents() {
    let bytes = mutated(request_json(), |v| v["protocol_major"] = json!(2));
    assert_eq!(
        AdapterRequest::from_json(&bytes),
        Err(ProtocolError::UnsupportedProtocolMajor(2))
    );

    let bytes = mutated(request_json(), |v| v["model_major"] = json!(3));
    assert_eq!(
        AdapterRequest::from_json(&bytes),
        Err(ProtocolError::UnsupportedModelMajor(3))
    );

    let result = AdapterResult::ok(path("out/model.json"), Vec::new());
    let mut raw: Value = serde_json::from_slice(&result.to_json()).expect("json");
    raw["protocol_major"] = json!(99);
    let bytes = serde_json::to_vec(&raw).expect("serialize");
    assert_eq!(
        AdapterResult::from_json(&bytes),
        Err(ProtocolError::UnsupportedProtocolMajor(99))
    );
}

/// The old adapter interface was a pile of CLI flags: `--vm-data`, `--out`, and
/// a bare JSON blob with no version, no digests, and no identity. Feeding that
/// shape in has to fail, or "the adapter ran" would keep meaning "some process
/// wrote some JSON".
#[test]
fn the_legacy_cli_shape_cannot_satisfy_the_protocol() {
    let legacy = json!({
        "vm_data": "/tmp/vm_data.bin",
        "isolate_data": "/tmp/iso_data.bin",
        "vm_instr": "/tmp/vm_instr.bin",
        "isolate_instr": "/tmp/iso_instr.bin",
        "vm_instr_va": 4096,
        "isolate_instr_va": 8192,
        "out": "/tmp/model.json",
        "backend": "blutter"
    });
    let bytes = serde_json::to_vec(&legacy).expect("legacy fixture serializes");
    let err = AdapterRequest::from_json(&bytes).expect_err("legacy shape is not a v1 request");
    assert!(
        matches!(err, ProtocolError::Malformed(ref detail) if detail.contains("protocol_major")),
        "expected a missing-version rejection, got {err}"
    );

    // Even with the versions bolted on, the legacy fields are undeclared and the
    // required ones are absent.
    let mut with_versions = legacy;
    with_versions["protocol_major"] = json!(PROTOCOL_MAJOR);
    with_versions["model_major"] = json!(MODEL_VERSION);
    let bytes = serde_json::to_vec(&with_versions).expect("serialize");
    assert!(matches!(
        AdapterRequest::from_json(&bytes),
        Err(ProtocolError::Malformed(_))
    ));
}

/// Snapshot bytes stay on disk. The request is a handle list, so its size is a
/// function of the number of regions and not of how big they are.
#[test]
fn a_request_stays_small_regardless_of_snapshot_size() {
    let mut huge = request();
    for input in &mut huge.inputs {
        input.size = 512 * 1024 * 1024;
    }
    let bytes = huge.to_json();
    assert!(
        bytes.len() < 2048,
        "a request for a half-gigabyte snapshot serialized to {} bytes",
        bytes.len()
    );

    // There is nowhere to put content: no field of any input accepts bytes, and
    // an attempt to add one is an undeclared field.
    let bytes = mutated(request_json(), |v| {
        v["inputs"][0]["contents_base64"] = json!("AAAA")
    });
    assert!(matches!(
        AdapterRequest::from_json(&bytes),
        Err(ProtocolError::Malformed(_))
    ));
}

/// Handles are contained by construction, so a traversal or absolute path
/// cannot survive deserialization into a value the host would later join onto a
/// working directory.
#[test]
fn path_handles_cannot_escape_the_working_directory() {
    for escape in [
        "../../etc/passwd",
        "/etc/passwd",
        "in/../../out",
        "",
        "in\\vm.bin",
    ] {
        let bytes = mutated(request_json(), |v| v["inputs"][0]["path"] = json!(escape));
        assert!(
            matches!(
                AdapterRequest::from_json(&bytes),
                Err(ProtocolError::Malformed(_))
            ),
            "path {escape:?} was accepted"
        );
        let bytes = mutated(request_json(), |v| v["output"] = json!(escape));
        assert!(
            matches!(
                AdapterRequest::from_json(&bytes),
                Err(ProtocolError::Malformed(_))
            ),
            "output {escape:?} was accepted"
        );
    }
}

#[test]
fn a_digest_that_is_not_a_digest_is_rejected() {
    for bad in ["", "deadbeef", "not-a-digest", &"F".repeat(64)] {
        let bytes = mutated(request_json(), |v| v["inputs"][0]["sha256"] = json!(bad));
        assert!(
            matches!(
                AdapterRequest::from_json(&bytes),
                Err(ProtocolError::Malformed(_))
            ),
            "digest {bad:?} was accepted"
        );
    }
}

/// The four regions are the contract. A missing one means the adapter would read
/// less than the host loaded; a duplicate means two handles claim the same
/// region and only one can be right.
#[test]
fn the_region_set_must_be_exactly_the_four_regions() {
    let bytes = mutated(request_json(), |v| {
        v["inputs"].as_array_mut().unwrap().remove(0);
    });
    assert_eq!(
        AdapterRequest::from_json(&bytes),
        Err(ProtocolError::MissingRegion(InputRegionName::VmData))
    );

    let bytes = mutated(request_json(), |v| {
        v["inputs"][1]["region"] = json!("vm_data");
        v["inputs"][1]["path"] = json!("in/other.bin");
    });
    assert_eq!(
        AdapterRequest::from_json(&bytes),
        Err(ProtocolError::DuplicateRegion(InputRegionName::VmData))
    );
}

#[test]
fn region_geometry_must_be_self_consistent() {
    let bytes = mutated(request_json(), |v| {
        v["inputs"][0]["executable"] = json!(true)
    });
    assert_eq!(
        AdapterRequest::from_json(&bytes),
        Err(ProtocolError::RegionExecutabilityMismatch(
            InputRegionName::VmData
        ))
    );

    let bytes = mutated(request_json(), |v| {
        v["inputs"][2]["virtual_address"] = json!(Value::Null)
    });
    assert_eq!(
        AdapterRequest::from_json(&bytes),
        Err(ProtocolError::RegionAddressMismatch(
            InputRegionName::VmInstructions
        ))
    );

    let bytes = mutated(request_json(), |v| v["inputs"][0]["size"] = json!(0));
    assert_eq!(
        AdapterRequest::from_json(&bytes),
        Err(ProtocolError::EmptyRegion(InputRegionName::VmData))
    );

    // Checked arithmetic: a load address plus a size that leaves the address
    // space describes no memory.
    let bytes = mutated(request_json(), |v| {
        v["inputs"][2]["virtual_address"] = json!(u64::MAX);
        v["inputs"][2]["size"] = json!(2);
    });
    assert_eq!(
        AdapterRequest::from_json(&bytes),
        Err(ProtocolError::RegionOverflows(
            InputRegionName::VmInstructions
        ))
    );
}

/// An output that aliases an input would let the adapter destroy the bytes it
/// was asked to read.
#[test]
fn handles_must_be_distinct() {
    let bytes = mutated(request_json(), |v| v["output"] = json!("in/vm_data.bin"));
    assert_eq!(
        AdapterRequest::from_json(&bytes),
        Err(ProtocolError::AliasedPath("in/vm_data.bin".to_string()))
    );

    let bytes = mutated(request_json(), |v| {
        v["inputs"][1]["path"] = json!("in/vm_data.bin")
    });
    assert_eq!(
        AdapterRequest::from_json(&bytes),
        Err(ProtocolError::AliasedPath("in/vm_data.bin".to_string()))
    );
}

/// A result has to be actionable: success names an artifact, failure names a
/// reason. Either without the other leaves the host with nothing to do.
#[test]
fn a_results_status_and_payload_must_agree() {
    let mut ok = AdapterResult::ok(path("out/model.json"), Vec::new());
    ok.model = None;
    assert_eq!(
        AdapterResult::from_json(&ok.to_json()),
        Err(ProtocolError::StatusPayloadMismatch)
    );

    let mut ok = AdapterResult::ok(path("out/model.json"), Vec::new());
    ok.error = Some(AdapterError {
        code: AdapterErrorCode::Internal,
        message: "but it worked".to_string(),
    });
    assert_eq!(
        AdapterResult::from_json(&ok.to_json()),
        Err(ProtocolError::StatusPayloadMismatch)
    );

    let mut failed = AdapterResult::failed(AdapterErrorCode::Internal, "boom");
    failed.error = None;
    assert_eq!(
        AdapterResult::from_json(&failed.to_json()),
        Err(ProtocolError::StatusPayloadMismatch)
    );

    let mut failed = AdapterResult::failed(AdapterErrorCode::Internal, "boom");
    failed.model = Some(path("out/model.json"));
    assert_eq!(
        AdapterResult::from_json(&failed.to_json()),
        Err(ProtocolError::StatusPayloadMismatch)
    );
}

/// Error codes are the stable part of a failure. They serialize as the snake
/// case names operators and tests match on, and a code this build does not know
/// is a rejection rather than a silent `Internal`.
#[test]
fn error_codes_are_stable_names() {
    let failed = AdapterResult::failed(AdapterErrorCode::InputDigestMismatch, "digest");
    let raw: Value = serde_json::from_slice(&failed.to_json()).expect("json");
    assert_eq!(raw["error"]["code"], json!("input_digest_mismatch"));
    assert_eq!(raw["status"], json!("failed"));

    let bytes = mutated(raw, |v| v["error"]["code"] = json!("something_new"));
    assert!(matches!(
        AdapterResult::from_json(&bytes),
        Err(ProtocolError::Malformed(_))
    ));
}

/// The request carries the host's identity verbatim, including the gate result,
/// so an adapter cannot be handed a snapshot the host has not cleared.
#[test]
fn the_request_carries_the_host_identity_unchanged() {
    let parsed = AdapterRequest::from_json(&request().to_json()).expect("valid");
    assert_eq!(parsed.identity, support::identity());
    assert!(parsed.identity.exact_selection_key().is_ok());
}
