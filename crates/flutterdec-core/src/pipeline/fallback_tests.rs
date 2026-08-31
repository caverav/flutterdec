//! What core recovery may and may not claim.
//!
//! The interesting property is not that a scan finds something. It is that the
//! scan finds the starts a hand-built instruction stream really has, stops at
//! the ones it has no evidence for, and that the resulting model still refuses
//! to name a library, a class, a function or a pool slot.

use super::*;
use flutterdec_loader::identity::{
    FeatureEvidence, HashSource, PointerCompression, SnapshotIdentity, SnapshotKind, TargetArch,
};

/// `stp x29, x30, [sp, #-16]!`
const PROLOGUE: u32 = 0xA9BF_7BFD;
/// `stp x29, x30, [sp, #16]`
const PROLOGUE_OFFSET: u32 = 0xA901_7BFD;
const RET: u32 = 0xD65F_03C0;
const NOP: u32 = 0xD503_201F;

fn bl(from_word: i64, to_word: i64) -> u32 {
    let delta = to_word - from_word;
    0b100101 << 26 | ((delta as i32) & 0x03FF_FFFF) as u32
}

fn stream(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn starts(functions: &[Function]) -> Vec<u64> {
    functions.iter().map(|f| f.code.start_va).collect()
}

fn identity(hash: Option<&str>, source: HashSource) -> SnapshotIdentity {
    SnapshotIdentity {
        hash: hash.map(ToString::to_string),
        hash_source: source,
        kind: Some(SnapshotKind::FullAot),
        target_arch: TargetArch::Arm64,
        features: FeatureEvidence::parse("product arm64 compressed-pointers"),
        pointer_compression: PointerCompression::Compressed,
    }
}

fn bundle_with(instr: Vec<u8>, base_va: u64) -> SnapshotBundle {
    SnapshotBundle {
        input_path: PathBuf::from("/fixture/libapp.so"),
        libapp_path: PathBuf::from("/fixture/libapp.so"),
        libapp_entry: None,
        arch: "arm64".to_string(),
        snapshot_hash: "80a49c7111088100a233b2ae788e1f48".to_string(),
        vm_data: vec![7u8; 64],
        isolate_data: vec![9u8; 64],
        vm_instr: stream(&[RET]),
        isolate_instr: instr,
        vm_instr_va: 0x2000,
        isolate_instr_va: base_va,
        dart_profile: None,
        snapshot_features: Some("product arm64".to_string()),
        compressed_pointers: Some(true),
        identity: identity(
            Some("80a49c7111088100a233b2ae788e1f48"),
            HashSource::Header,
        ),
    }
}

#[test]
fn a_frame_prologue_starts_a_candidate_and_the_next_start_ends_it() {
    // Two functions: one at the base, one at word 4.
    let words = [PROLOGUE, NOP, NOP, RET, PROLOGUE_OFFSET, NOP, RET, NOP];
    let functions = recover_code_candidates(&stream(&words), 0x1000);

    assert_eq!(starts(&functions), vec![0x1000, 0x1010]);
    assert_eq!(functions[0].code.size, 0x10, "the next start bounds the first");
    assert_eq!(
        functions[1].code.size,
        0x10,
        "the end of the region bounds the last"
    );
    assert!(
        functions.iter().all(|f| f.provenance == Provenance::Heuristic),
        "a prologue is evidence of a boundary and of nothing else"
    );
    assert!(
        functions.iter().all(|f| f.name.is_none() && f.owner.is_none()),
        "there is no name or owner evidence in an instruction stream"
    );
    assert_eq!(
        functions.iter().map(|f| f.id.0).collect::<Vec<_>>(),
        vec![0, 1],
        "ids are dense and ascending"
    );
}

/// A single `bl` is a tail call or a computed offset as easily as an entry
/// point, so it is not enough on its own. Two independent calls to the same
/// address are.
#[test]
fn a_call_target_needs_a_second_caller_before_it_becomes_a_start() {
    let once = [PROLOGUE, bl(1, 3), RET, NOP, NOP, RET];
    assert_eq!(
        starts(&recover_code_candidates(&stream(&once), 0x1000)),
        vec![0x1000],
        "one call target invented a function boundary"
    );

    let twice = [PROLOGUE, bl(1, 4), bl(2, 4), RET, NOP, RET];
    assert_eq!(
        starts(&recover_code_candidates(&stream(&twice), 0x1000)),
        vec![0x1000, 0x1010],
        "a target reached twice was not promoted to a start"
    );
}

/// A branch that leaves the region is not a start inside it, and a backwards
/// displacement has to decode as one.
#[test]
fn call_targets_outside_the_region_are_dropped() {
    let words = [PROLOGUE, bl(1, -400), bl(2, -400), bl(3, 900), RET];
    let functions = recover_code_candidates(&stream(&words), 0x1000);
    assert_eq!(starts(&functions), vec![0x1000]);

    // Same displacement, a base that puts the target back inside the region.
    let functions = recover_code_candidates(&stream(&words), 0x1000 + 400 * 4);
    assert_eq!(
        starts(&functions),
        vec![0x1000 + 400 * 4],
        "the backward target decoded to something other than base - 400 words"
    );
}

#[test]
fn a_candidate_never_claims_more_than_the_span_cap() {
    let mut words = vec![PROLOGUE];
    words.resize(0x8000, NOP);
    let functions = recover_code_candidates(&stream(&words), 0x1000);
    assert_eq!(functions.len(), 1);
    assert_eq!(
        functions[0].code.size, MAX_CANDIDATE_SIZE,
        "one candidate swallowed the rest of the region"
    );
}

#[test]
fn an_empty_region_recovers_nothing_rather_than_one_empty_candidate() {
    assert!(recover_code_candidates(&[], 0x1000).is_empty());
}

#[test]
fn the_recovered_model_reports_code_and_refuses_every_semantic_domain() {
    let words = [PROLOGUE, NOP, NOP, RET, PROLOGUE_OFFSET, NOP, RET, NOP];
    let bundle = bundle_with(stream(&words), 0x1000);
    let model = core_recovered_model(&bundle, CoreFallbackReason::NoCompatibilityRecord)
        .expect("core recovery");

    assert_eq!(model.functions.len(), 2, "no useful code output");
    assert_eq!(model.capabilities.functions, CapabilityLevel::Partial);
    for domain in [
        Domain::Libraries,
        Domain::Classes,
        Domain::ClassRelationships,
        Domain::FunctionNames,
        Domain::ObjectPool,
        Domain::PoolIndexSpace,
    ] {
        assert_eq!(
            model.capabilities.level(domain),
            CapabilityLevel::Unavailable,
            "{domain} was claimed without a snapshot parser"
        );
        assert!(
            model
                .diagnostics
                .iter()
                .any(|d| d.subject.as_deref() == Some(domain.as_str())),
            "{domain} is unavailable with no diagnostic saying why"
        );
    }
    assert!(model.libraries.is_empty() && model.classes.is_empty());
    assert!(model.object_pool.entries.is_empty() && model.object_pool.geometry.is_none());
    assert_eq!(
        model.compatibility, None,
        "core recovery invented a compatibility binding"
    );
    assert_eq!(model.producer.trust, ProducerTrust::Local);

    // Both executable regions are described even though only the isolate one is
    // scanned, and the identity is the loader's, unaltered.
    let executable = model
        .input
        .executable_regions()
        .map(|r| r.region)
        .collect::<Vec<_>>();
    assert_eq!(
        executable,
        vec![
            InputRegionName::VmInstructions,
            InputRegionName::IsolateInstructions
        ]
    );
    assert_eq!(model.input.identity, bundle.identity);
}

/// The diagnostic for `functions` has to say *which* kind of nothing happened:
/// a heuristic-only domain and an empty one are different facts.
#[test]
fn a_region_with_no_recoverable_code_says_so_instead_of_claiming_a_partial_domain() {
    let bundle = bundle_with(Vec::new(), 0x1000);
    let model =
        core_recovered_model(&bundle, CoreFallbackReason::IdentityRejected).expect("core recovery");

    assert!(model.functions.is_empty());
    assert_eq!(model.capabilities.functions, CapabilityLevel::Unavailable);
    let functions_diagnostic = model
        .diagnostics
        .iter()
        .find(|d| d.subject.as_deref() == Some("functions"))
        .expect("a diagnostic for functions");
    assert_eq!(functions_diagnostic.code, DiagnosticCode::DomainNotRecovered);
}

#[test]
fn every_fallback_reason_carries_a_distinct_stable_token_and_detail() {
    let reasons = [
        CoreFallbackReason::InternalRequested,
        CoreFallbackReason::IdentityRejected,
        CoreFallbackReason::NoCompatibilityRecord,
        CoreFallbackReason::CompatibilityUnsupported,
        CoreFallbackReason::AdapterNotInstalled,
    ];
    let tokens = reasons.iter().map(|r| r.as_str()).collect::<BTreeSet<_>>();
    assert_eq!(tokens.len(), reasons.len(), "two reasons share a token");
    let details = reasons.iter().map(|r| r.detail()).collect::<BTreeSet<_>>();
    assert_eq!(details.len(), reasons.len(), "two reasons share a detail");
    for reason in reasons {
        assert_eq!(
            serde_json::to_value(reason).expect("serialize"),
            serde_json::Value::String(reason.as_str().to_string()),
            "the wire token and as_str disagree"
        );
    }
}
