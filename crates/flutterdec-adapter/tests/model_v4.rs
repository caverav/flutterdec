//! ProgramModel v4 contract tests.
//!
//! Every case here serializes a fixture, hands the *bytes* to
//! `ProgramModel::from_json`, and validates the parsed result. Testing the
//! in-memory struct would prove nothing about the boundary: the failures worth
//! catching are the ones a hostile or broken producer can write into JSON.

mod support;

use flutterdec_adapter::model::{
    schema, CapabilityLevel, Domain, ModelParseError, ProgramModel, MODEL_VERSION,
};
use flutterdec_adapter::validate::{validate, ValidationError};
use serde_json::{json, Value};
use support::{host, maximal_model, unavailable_model, ISO_INSTR_VA, VM_INSTR_VA};

/// Serialize a model, apply a mutation to the JSON, and hand the bytes back.
///
/// The mutation happens on the document rather than the struct so a case can
/// express things the Rust types cannot hold, which is exactly where the
/// interesting rejections live.
/// One named edit to a serialized model. The tables below are the negative
/// fixtures: each entry is a plausible document that must be rejected.
type Mutation = Box<dyn Fn(&mut Value)>;

fn mutated(mut value: Value, mutate: impl FnOnce(&mut Value)) -> Vec<u8> {
    mutate(&mut value);
    serde_json::to_vec(&value).expect("mutated fixture serializes")
}

fn model_json(model: &ProgramModel) -> Value {
    serde_json::from_slice(&model.to_canonical_json()).expect("canonical json is json")
}

/// Parse fresh bytes and validate against the host context, returning the first
/// invariant broken.
fn parse_and_validate(bytes: &[u8]) -> Result<ProgramModel, String> {
    let model = ProgramModel::from_json(bytes).map_err(|err| err.to_string())?;
    validate(&model, &host()).map_err(|err| err.to_string())?;
    Ok(model)
}

fn expect_validation_error(bytes: &[u8]) -> ValidationError {
    let model = ProgramModel::from_json(bytes).expect("fixture still parses as v4");
    validate(&model, &host()).expect_err("fixture is expected to fail validation")
}

// ---------------------------------------------------------------------------
// Positive path
// ---------------------------------------------------------------------------

/// The maximal fixture survives a full round trip through JSON and validation,
/// and comes back byte-identical. If this fails, every negative case below is
/// measuring the wrong thing.
#[test]
fn a_valid_model_round_trips_through_fresh_json() {
    let original = maximal_model();
    let bytes = original.to_canonical_json();
    let parsed = parse_and_validate(&bytes).expect("maximal fixture is valid");
    assert_eq!(parsed, original);
    assert_eq!(parsed.to_canonical_json(), bytes);
}

/// Canonical output is a function of the value alone, so repeated serialization
/// of the same model, and of a model parsed from that output, produce the same
/// bytes. Determinism is what makes model artifacts diffable across runs.
#[test]
fn canonical_serialization_is_byte_stable() {
    let model = maximal_model();
    let first = model.to_canonical_json();
    for _ in 0..8 {
        assert_eq!(model.to_canonical_json(), first);
    }
    let reparsed = ProgramModel::from_json(&first).expect("round trip parses");
    assert_eq!(reparsed.to_canonical_json(), first);
    // Extensions are a map, so their key order has to come from the container
    // rather than from insertion order.
    let mut shuffled = maximal_model();
    shuffled.extensions.insert("aaa".to_string(), json!(1));
    shuffled.extensions.insert("zzz".to_string(), json!(2));
    let a = shuffled.to_canonical_json();
    let mut other = maximal_model();
    other.extensions.insert("zzz".to_string(), json!(2));
    other.extensions.insert("aaa".to_string(), json!(1));
    assert_eq!(a, other.to_canonical_json());
}

/// A producer that recovered nothing has a shape to say so: empty domains,
/// unavailable capabilities, and a diagnostic per domain. This is the model that
/// replaces inventing `package:app/main.dart` and a function called `main`.
#[test]
fn a_model_that_recovered_nothing_is_valid_and_says_so() {
    let model = unavailable_model();
    let parsed =
        parse_and_validate(&model.to_canonical_json()).expect("unavailable model is valid");

    assert!(parsed.libraries.is_empty());
    assert!(parsed.classes.is_empty());
    assert!(parsed.functions.is_empty());
    assert!(parsed.object_pool.entries.is_empty());
    assert!(parsed.object_pool.geometry.is_none());
    for domain in Domain::ALL {
        assert_eq!(
            parsed.capabilities.level(domain),
            CapabilityLevel::Unavailable,
            "{domain} should be unavailable"
        );
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|d| d.subject.as_deref() == Some(domain.as_str())),
            "{domain} should carry a diagnostic"
        );
    }
}

/// A function can have a code range and no name. The point of the whole model
/// change: `None` is representable, so nothing has to be invented to fill a
/// required string.
#[test]
fn an_unnamed_function_keeps_its_code_range() {
    let parsed = parse_and_validate(&maximal_model().to_canonical_json()).expect("valid");
    let unnamed = parsed
        .functions
        .iter()
        .find(|f| f.name.is_none())
        .expect("fixture has an unnamed function");
    assert_eq!(unnamed.code.start_va, VM_INSTR_VA);
    assert_eq!(unnamed.code.size, 0x20);
}

/// A range that ends exactly on a region boundary is inside the region. The
/// off-by-one here decides whether the last function in a section is accepted.
#[test]
fn a_range_ending_exactly_at_the_region_boundary_is_valid() {
    let mut model = maximal_model();
    let last = support::ISO_INSTR_SIZE;
    model.functions[0].code.start_va = ISO_INSTR_VA + last - 8;
    model.functions[0].code.size = 8;
    parse_and_validate(&model.to_canonical_json()).expect("boundary-exact range is valid");
}

// ---------------------------------------------------------------------------
// Version rejection
// ---------------------------------------------------------------------------

/// v2 and v3 documents are rejected as the wrong contract, not repaired. The
/// error has to name the legacy version, because "missing field `producer`" is
/// not something an operator can act on.
#[test]
fn legacy_v2_and_v3_models_are_rejected_without_a_shim() {
    for version in [2u64, 3] {
        let legacy = json!({
            "schema_version": version,
            "adapter_kind": "blutter",
            "dart_version": "3.5.0",
            "snapshot_hash": support::HASH,
            "arch": "arm64",
            "libraries": [{ "id": 0, "uri": "package:app/main.dart", "name_display": "main" }],
            "classes": [{ "id": 0, "name": "Global", "super": "Object", "lib": "package:app/main.dart" }],
            "functions": [{
                "id": 0, "name": "main", "owner_class": "Global",
                "entry_va": 8192, "size": 64, "code_section_va": 8192
            }],
            "object_pool": []
        });
        let bytes = serde_json::to_vec(&legacy).expect("legacy fixture serializes");
        match ProgramModel::from_json(&bytes) {
            Err(ModelParseError::LegacyModel(found)) => assert_eq!(found, version),
            other => panic!("expected a legacy rejection for v{version}, got {other:?}"),
        }
        assert!(ProgramModel::from_json(&bytes)
            .unwrap_err()
            .to_string()
            .contains("no compatibility shim"));
    }
}

/// A future version is rejected too. Accepting an unknown model would mean
/// guessing at fields this build does not know about.
#[test]
fn an_unknown_model_version_is_rejected() {
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["model_version"] = json!(5);
    });
    assert!(matches!(
        ProgramModel::from_json(&bytes),
        Err(ModelParseError::UnsupportedVersion(5))
    ));

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v.as_object_mut().unwrap().remove("model_version");
    });
    assert!(matches!(
        ProgramModel::from_json(&bytes),
        Err(ModelParseError::MissingVersion)
    ));
}

// ---------------------------------------------------------------------------
// Closedness
// ---------------------------------------------------------------------------

/// Every object except `extensions` rejects keys this contract does not define.
/// An ignored unknown key is how a producer ships a field the host silently
/// drops and both sides believe it took effect.
#[test]
fn undeclared_fields_are_rejected_everywhere_except_extensions() {
    let cases: Vec<(&str, Mutation)> = vec![
        (
            "root",
            Box::new(|v: &mut Value| v["surprise"] = json!(true)),
        ),
        (
            "producer",
            Box::new(|v: &mut Value| v["producer"]["surprise"] = json!(true)),
        ),
        (
            "capabilities",
            Box::new(|v: &mut Value| v["capabilities"]["surprise"] = json!("complete")),
        ),
        (
            "library",
            Box::new(|v: &mut Value| v["libraries"][0]["surprise"] = json!(1)),
        ),
        (
            "function",
            Box::new(|v: &mut Value| v["functions"][0]["surprise"] = json!(1)),
        ),
        (
            "function name",
            Box::new(|v: &mut Value| v["functions"][0]["name"]["surprise"] = json!(1)),
        ),
        (
            "pool entry",
            Box::new(|v: &mut Value| v["object_pool"]["entries"][0]["surprise"] = json!(1)),
        ),
        (
            "pool geometry",
            Box::new(|v: &mut Value| v["object_pool"]["geometry"]["surprise"] = json!(1)),
        ),
        (
            "input region",
            Box::new(|v: &mut Value| v["input"]["regions"][0]["surprise"] = json!(1)),
        ),
        (
            "identity",
            Box::new(|v: &mut Value| v["input"]["identity"]["surprise"] = json!(1)),
        ),
        (
            "diagnostic",
            Box::new(|v: &mut Value| v["diagnostics"][0]["surprise"] = json!(1)),
        ),
    ];
    for (label, mutate) in cases {
        let bytes = mutated(model_json(&maximal_model()), mutate);
        assert!(
            ProgramModel::from_json(&bytes).is_err(),
            "{label} accepted an undeclared field"
        );
    }

    // The one controlled exception.
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["extensions"]["anything"] = json!({ "nested": [1, 2, 3] });
    });
    let parsed = parse_and_validate(&bytes).expect("extensions accept undeclared keys");
    assert!(parsed.extensions.contains_key("anything"));
}

// ---------------------------------------------------------------------------
// Host-selected facts: VAL-MODEL-003
// ---------------------------------------------------------------------------

/// The adapter reports host-selected facts back; it does not choose them. Each
/// case changes exactly one field and must be caught, because a model that
/// describes a different snapshot, producer, or compatibility decision is not a
/// model of this run.
#[test]
fn a_model_cannot_change_a_host_selected_fact() {
    let cases: Vec<(&str, Mutation)> = vec![
        (
            "snapshot hash",
            Box::new(|v: &mut Value| {
                v["input"]["identity"]["hash"] = json!("00000000000000000000000000000000")
            }),
        ),
        (
            "hash source",
            Box::new(|v: &mut Value| v["input"]["identity"]["hash_source"] = json!("scan")),
        ),
        (
            "snapshot kind",
            Box::new(|v: &mut Value| v["input"]["identity"]["kind"] = json!("full_jit")),
        ),
        (
            "target architecture",
            Box::new(|v: &mut Value| {
                v["input"]["identity"]["target_arch"] = json!({ "unsupported": "x64" })
            }),
        ),
        (
            "normalized features",
            Box::new(|v: &mut Value| {
                v["input"]["identity"]["features"]["normalized"] = json!(["arm64", "product"])
            }),
        ),
        (
            "pointer compression",
            Box::new(|v: &mut Value| {
                v["input"]["identity"]["pointer_compression"] = json!("uncompressed")
            }),
        ),
        (
            "producer id",
            Box::new(|v: &mut Value| v["producer"]["id"] = json!("someone-else")),
        ),
        (
            "producer version",
            Box::new(|v: &mut Value| v["producer"]["version"] = json!("9.9.9")),
        ),
        (
            "producer artifact digest",
            Box::new(|v: &mut Value| {
                v["producer"]["artifact_sha256"] = json!(support::digest("other").to_string())
            }),
        ),
        // The self-promotion case: an untrusted adapter writing "registered".
        (
            "producer trust",
            Box::new(|v: &mut Value| v["producer"]["trust"] = json!("untrusted")),
        ),
        (
            "compatibility record digest",
            Box::new(|v: &mut Value| {
                v["compatibility"]["record_sha256"] = json!(support::digest("other").to_string())
            }),
        ),
        (
            "parser family",
            Box::new(|v: &mut Value| {
                v["compatibility"]["parser_family_id"] = json!("other-family")
            }),
        ),
        (
            "profile id",
            Box::new(|v: &mut Value| v["compatibility"]["profile_id"] = json!("other-profile")),
        ),
        (
            "profile digest",
            Box::new(|v: &mut Value| {
                v["compatibility"]["profile_sha256"] = json!(support::digest("other").to_string())
            }),
        ),
    ];

    for (field, mutate) in cases {
        let bytes = mutated(model_json(&maximal_model()), mutate);
        assert_eq!(
            expect_validation_error(&bytes),
            ValidationError::HostFactMismatch { field },
            "mutating {field} was not caught"
        );
    }
}

/// The region table has to describe the bytes the host actually handed over.
#[test]
fn a_model_cannot_misreport_the_input_regions() {
    use flutterdec_adapter::model::InputRegionName;

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["input"]["regions"][0]["size"] = json!(999)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::RegionMismatch {
            region: InputRegionName::VmData,
            field: "size"
        }
    );

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["input"]["regions"][0]["sha256"] = json!(support::digest("tampered").to_string())
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::RegionMismatch {
            region: InputRegionName::VmData,
            field: "sha-256 digest"
        }
    );

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["input"]["regions"][2]["virtual_address"] = json!(0x9000)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::RegionMismatch {
            region: InputRegionName::VmInstructions,
            field: "virtual address"
        }
    );

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["input"]["regions"].as_array_mut().unwrap().remove(0);
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::MissingRegion(InputRegionName::VmData)
    );

    // A data region claiming to be executable would let a code range be placed
    // in a section that holds no code.
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["input"]["regions"][0]["executable"] = json!(true)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::RegionExecutabilityMismatch(InputRegionName::VmData)
    );

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["input"]["regions"][2]["virtual_address"] = json!(Value::Null)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::RegionAddressMismatch(InputRegionName::VmInstructions)
    );

    // The region table is ordered, so two models of the same input are the same
    // bytes.
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["input"]["regions"].as_array_mut().unwrap().swap(0, 1);
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::NoncanonicalOrder {
            collection: "input regions"
        }
    );
}

// ---------------------------------------------------------------------------
// Honest capabilities: VAL-MODEL-002
// ---------------------------------------------------------------------------

/// A capability claim that the model's own contents contradict is the failure
/// mode that makes capabilities worth having at all.
#[test]
fn capability_claims_must_match_the_models_contents() {
    let cases: Vec<(&str, Domain, Mutation)> = vec![
        (
            "unavailable libraries with libraries present",
            Domain::Libraries,
            Box::new(|v: &mut Value| v["capabilities"]["libraries"] = json!("unavailable")),
        ),
        (
            "unavailable classes with classes present",
            Domain::Classes,
            Box::new(|v: &mut Value| v["capabilities"]["classes"] = json!("unavailable")),
        ),
        (
            "unavailable functions with functions present",
            Domain::Functions,
            Box::new(|v: &mut Value| v["capabilities"]["functions"] = json!("unavailable")),
        ),
        (
            "unavailable names with a named function",
            Domain::FunctionNames,
            Box::new(|v: &mut Value| v["capabilities"]["function_names"] = json!("unavailable")),
        ),
        (
            "unavailable pool with entries present",
            Domain::ObjectPool,
            Box::new(|v: &mut Value| v["capabilities"]["object_pool"] = json!("unavailable")),
        ),
        (
            "unavailable relationships with a superclass edge",
            Domain::ClassRelationships,
            Box::new(|v: &mut Value| {
                v["capabilities"]["class_relationships"] = json!("unavailable")
            }),
        ),
        // The headline case: a heuristic guess reported inside a complete
        // domain, which is how a guess becomes indistinguishable from a fact.
        (
            "complete functions containing a heuristic range",
            Domain::Functions,
            Box::new(|v: &mut Value| v["capabilities"]["functions"] = json!("complete")),
        ),
        (
            "complete pool containing a heuristic entry",
            Domain::ObjectPool,
            Box::new(|v: &mut Value| v["capabilities"]["object_pool"] = json!("complete")),
        ),
        (
            "complete names leaving a function unnamed",
            Domain::FunctionNames,
            Box::new(|v: &mut Value| v["capabilities"]["function_names"] = json!("complete")),
        ),
        (
            "complete libraries containing a heuristic library",
            Domain::Libraries,
            Box::new(|v: &mut Value| {
                v["capabilities"]["libraries"] = json!("complete");
                v["libraries"][0]["provenance"] = json!("heuristic");
            }),
        ),
        (
            "complete classes containing a heuristic class",
            Domain::Classes,
            Box::new(|v: &mut Value| {
                v["capabilities"]["classes"] = json!("complete");
                v["classes"][0]["provenance"] = json!("heuristic");
            }),
        ),
    ];

    for (label, domain, mutate) in cases {
        let bytes = mutated(model_json(&maximal_model()), mutate);
        match expect_validation_error(&bytes) {
            ValidationError::CapabilityContradiction { domain: got, .. } => {
                assert_eq!(got, domain, "{label} reported the wrong domain")
            }
            other => panic!("{label} produced {other:?}"),
        }
    }
}

/// Domains cannot exist without the domains they depend on. Names without
/// functions and relationships without classes are both claims about records
/// that are not there.
#[test]
fn a_domain_cannot_outlive_the_domain_it_depends_on() {
    let mut model = unavailable_model();
    model.capabilities.function_names = CapabilityLevel::Partial;
    match expect_validation_error(&model.to_canonical_json()) {
        ValidationError::CapabilityContradiction { domain, .. } => {
            assert_eq!(domain, Domain::FunctionNames)
        }
        other => panic!("expected a function-names contradiction, got {other:?}"),
    }

    let mut model = unavailable_model();
    model.capabilities.class_relationships = CapabilityLevel::Partial;
    match expect_validation_error(&model.to_canonical_json()) {
        ValidationError::CapabilityContradiction { domain, .. } => {
            assert_eq!(domain, Domain::ClassRelationships)
        }
        other => panic!("expected a relationships contradiction, got {other:?}"),
    }
}

/// An unavailable domain must say why. Without this, "we did not look" and "we
/// looked and there was nothing" are the same document.
#[test]
fn an_unavailable_domain_without_a_diagnostic_is_rejected() {
    let mut model = unavailable_model();
    model
        .diagnostics
        .retain(|d| d.subject.as_deref() != Some("object_pool"));
    assert_eq!(
        expect_validation_error(&model.to_canonical_json()),
        ValidationError::UnavailableDomainWithoutDiagnostic(Domain::ObjectPool)
    );
}

/// Confidence is only meaningful on a guess. A score attached to an exact fact
/// is the calibrated-looking decoration this contract exists to keep out.
#[test]
fn confidence_requires_heuristic_provenance_and_a_real_range() {
    use flutterdec_adapter::model::Provenance;

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["functions"][0]["name"]["confidence"] = json!(0.87)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::ConfidenceWithoutHeuristicProvenance {
            field: "function name",
            provenance: Provenance::Exact
        }
    );

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["object_pool"]["entries"][0]["confidence"] = json!(0.99)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::ConfidenceWithoutHeuristicProvenance {
            field: "object pool entry",
            provenance: Provenance::Exact
        }
    );

    // Index 3 is the heuristic selector entry, so the provenance check passes
    // and the range check is what fires.
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["object_pool"]["entries"][3]["confidence"] = json!(1.5)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::ConfidenceOutOfRange {
            field: "object pool entry",
            value: 1.5
        }
    );
}

/// A placeholder is not a name. Accepting one puts `unknown` into report output
/// as though a parser had recovered it.
#[test]
fn placeholder_strings_cannot_stand_in_for_unrecovered_names() {
    let cases: Vec<(&str, &str, Mutation)> = vec![
        (
            "function name",
            "<unknown>",
            Box::new(|v: &mut Value| v["functions"][0]["name"]["text"] = json!("<unknown>")),
        ),
        (
            "class name",
            "unnamed",
            Box::new(|v: &mut Value| v["classes"][0]["name"] = json!("unnamed")),
        ),
        (
            "library uri",
            "unknown",
            Box::new(|v: &mut Value| v["libraries"][0]["uri"] = json!("unknown")),
        ),
        (
            "object pool entry value",
            "TODO",
            Box::new(|v: &mut Value| v["object_pool"]["entries"][0]["value"] = json!("TODO")),
        ),
        (
            "library display name",
            "N/A",
            Box::new(|v: &mut Value| v["libraries"][0]["display_name"] = json!("N/A")),
        ),
    ];
    for (field, value, mutate) in cases {
        let bytes = mutated(model_json(&maximal_model()), mutate);
        assert_eq!(
            expect_validation_error(&bytes),
            ValidationError::PlaceholderName {
                field,
                value: value.to_string()
            }
        );
    }

    // Whitespace is not a name either.
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["classes"][0]["name"] = json!("   ")
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::EmptyField {
            field: "class name"
        }
    );
}

/// `main` is a real Dart function name. The placeholder rule rejects admissions
/// of ignorance, not ordinary identifiers that happen to look suspicious.
#[test]
fn ordinary_names_that_resemble_defaults_are_still_accepted() {
    for name in ["main", "Global", "build", "Object"] {
        let bytes = mutated(model_json(&maximal_model()), |v| {
            v["functions"][0]["name"]["text"] = json!(name)
        });
        parse_and_validate(&bytes).unwrap_or_else(|err| panic!("{name} rejected: {err}"));
    }
}

// ---------------------------------------------------------------------------
// References, identity, ordering: VAL-VALIDATE-001
// ---------------------------------------------------------------------------

#[test]
fn duplicate_ids_and_indexes_are_rejected() {
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["libraries"][1]["id"] = json!(1)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::DuplicateLibraryId(1)
    );

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["classes"][1]["id"] = json!(1)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::DuplicateClassId(1)
    );

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["functions"][1]["id"] = json!(1)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::DuplicateFunctionId(1)
    );

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["object_pool"]["entries"][1]["index"] = json!(0)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::DuplicatePoolIndex(0)
    );
}

#[test]
fn dangling_references_are_rejected() {
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["classes"][0]["library"] = json!(99)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::MissingLibraryReference {
            class: 1,
            library: 99
        }
    );

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["classes"][1]["super_class"] = json!(99)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::MissingSuperClassReference {
            class: 2,
            super_class: 99
        }
    );

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["functions"][0]["owner"] = json!(99)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::MissingOwnerReference {
            function: 1,
            owner: 99
        }
    );
}

/// A class that is its own ancestor is not a hierarchy, and a consumer walking
/// it would not terminate.
#[test]
fn superclass_cycles_are_rejected() {
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["classes"][0]["super_class"] = json!(1)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::SuperClassCycle { class: 1 }
    );

    // A two-step cycle, which a self-reference check alone would miss.
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["classes"][0]["super_class"] = json!(2);
        v["classes"][1]["super_class"] = json!(1);
    });
    assert!(matches!(
        expect_validation_error(&bytes),
        ValidationError::SuperClassCycle { .. }
    ));
}

#[test]
fn noncanonical_ordering_is_rejected() {
    for (collection, pointer) in [
        ("libraries", "libraries"),
        ("classes", "classes"),
        ("functions", "functions"),
    ] {
        let bytes = mutated(model_json(&maximal_model()), |v| {
            v[pointer].as_array_mut().unwrap().swap(0, 1);
        });
        assert_eq!(
            expect_validation_error(&bytes),
            ValidationError::NoncanonicalOrder { collection }
        );
    }

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["object_pool"]["entries"]
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::NoncanonicalOrder {
            collection: "object pool entries"
        }
    );
}

// ---------------------------------------------------------------------------
// Addresses, ranges, geometry: VAL-VALIDATE-002
// ---------------------------------------------------------------------------

/// Overflow is the case a naive `start + size` gets wrong, so it is checked with
/// the extreme values rather than a comfortable margin.
#[test]
fn overflowing_and_empty_ranges_are_rejected() {
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["functions"][0]["code"]["size"] = json!(0)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::EmptyCodeRange { function: 1 }
    );

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["functions"][0]["code"]["start_va"] = json!(u64::MAX);
        v["functions"][0]["code"]["size"] = json!(1);
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::AddressOverflow {
            context: "code range of function",
            id: 1
        }
    );

    // Maximum size from a valid base: the sum overflows even though both fields
    // are individually representable.
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["functions"][0]["code"]["size"] = json!(u64::MAX)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::AddressOverflow {
            context: "code range of function",
            id: 1
        }
    );
}

/// Code has to live in a region that holds code. A range in a data region, or
/// past the end of an executable one, cannot be disassembled.
#[test]
fn code_ranges_must_be_contained_by_an_executable_region() {
    // One byte past the end of the isolate instructions.
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["functions"][0]["code"]["start_va"] = json!(ISO_INSTR_VA + support::ISO_INSTR_SIZE - 4);
        v["functions"][0]["code"]["size"] = json!(8);
    });
    assert!(matches!(
        expect_validation_error(&bytes),
        ValidationError::CodeRangeOutsideExecutableRegions { function: 1, .. }
    ));

    // An address in no region at all.
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["functions"][0]["code"]["start_va"] = json!(0xdead_0000u64)
    });
    assert!(matches!(
        expect_validation_error(&bytes),
        ValidationError::CodeRangeOutsideExecutableRegions { function: 1, .. }
    ));

    // A section base that is not the base of the region the code is in: the
    // producer and the host disagree about the address space.
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["functions"][0]["code_section_va"] = json!(VM_INSTR_VA)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::CodeSectionMismatch {
            function: 1,
            code_section_va: VM_INSTR_VA
        }
    );
}

#[test]
fn pool_targets_must_land_inside_an_executable_region() {
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["object_pool"]["entries"][1]["target_va"] = json!(0xdead_0000u64)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::PoolTargetOutsideExecutableRegions {
            index: 3,
            target_va: 0xdead_0000
        }
    );

    // One past the end of the region is outside it.
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["object_pool"]["entries"][1]["target_va"] = json!(ISO_INSTR_VA + support::ISO_INSTR_SIZE)
    });
    assert!(matches!(
        expect_validation_error(&bytes),
        ValidationError::PoolTargetOutsideExecutableRegions { .. }
    ));
}

#[test]
fn pool_entry_shapes_must_agree_with_their_kind() {
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["object_pool"]["entries"][1]["target_va"] = json!(Value::Null)
    });
    assert!(matches!(
        expect_validation_error(&bytes),
        ValidationError::PoolEntryShape { index: 3, .. }
    ));

    // An undecoded slot that carries a decoded value is claiming both.
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["object_pool"]["entries"][2]["value"] = json!("something")
    });
    assert!(matches!(
        expect_validation_error(&bytes),
        ValidationError::PoolEntryShape { index: 5, .. }
    ));

    // A string entry pointing at code.
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["object_pool"]["entries"][0]["target_va"] = json!(ISO_INSTR_VA)
    });
    assert!(matches!(
        expect_validation_error(&bytes),
        ValidationError::PoolEntryShape { index: 0, .. }
    ));

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["object_pool"]["entries"][0]["value"] = json!(Value::Null)
    });
    assert!(matches!(
        expect_validation_error(&bytes),
        ValidationError::PoolEntryShape { index: 0, .. }
    ));
}

/// Geometry decides whether a `ldr xN, [x27, #disp]` resolves to a value or to
/// nothing. Bad geometry silently maps every displacement onto the wrong entry,
/// so the arithmetic constraints are checked rather than assumed.
#[test]
fn pool_geometry_must_be_able_to_describe_an_object_pool() {
    for word_size in [0u64, 3, 12, 16] {
        let bytes = mutated(model_json(&maximal_model()), |v| {
            v["object_pool"]["geometry"]["word_size"] = json!(word_size)
        });
        assert!(
            matches!(
                expect_validation_error(&bytes),
                ValidationError::PoolGeometry { .. }
            ),
            "word size {word_size} accepted"
        );
    }

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["object_pool"]["geometry"]["entries_offset"] = json!(0x11)
    });
    assert!(matches!(
        expect_validation_error(&bytes),
        ValidationError::PoolGeometry { .. }
    ));

    // An index whose displacement leaves the address space is not addressable,
    // however plausible the index looks on its own.
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["object_pool"]["entries"][3]["index"] = json!(u64::MAX)
    });
    assert_eq!(
        expect_validation_error(&bytes),
        ValidationError::PoolIndexOutOfBounds { index: u64::MAX }
    );
}

/// Hardware indexes without geometry, and geometry without hardware indexes, are
/// both ways of letting a consumer resolve a position as though it were an
/// address.
#[test]
fn the_pool_index_space_and_geometry_must_agree() {
    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["object_pool"]["geometry"] = json!(Value::Null)
    });
    match expect_validation_error(&bytes) {
        ValidationError::CapabilityContradiction { domain, .. } => {
            assert_eq!(domain, Domain::PoolIndexSpace)
        }
        other => panic!("expected an index-space contradiction, got {other:?}"),
    }

    let bytes = mutated(model_json(&maximal_model()), |v| {
        v["object_pool"]["index_space"] = json!("ordinal")
    });
    match expect_validation_error(&bytes) {
        ValidationError::CapabilityContradiction { domain, .. } => {
            assert_eq!(domain, Domain::PoolIndexSpace)
        }
        other => panic!("expected an index-space contradiction, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Schema: VAL-MODEL-001
// ---------------------------------------------------------------------------

const SCHEMA_PATH: &str = "../../schemas/program-model-v4.schema.json";

/// The committed schema is the generated schema.
///
/// Set `UPDATE_SCHEMA=1` to rewrite the file after an intentional model change;
/// the drift check below is what stops that from being a way to paper over an
/// unintentional one.
#[test]
fn the_committed_schema_matches_the_generated_one() {
    let generated = format!(
        "{}\n",
        serde_json::to_string_pretty(&schema()).expect("schema serializes")
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(SCHEMA_PATH);
    if std::env::var_os("UPDATE_SCHEMA").is_some() {
        std::fs::write(&path, &generated).expect("write schema");
        return;
    }
    let committed = std::fs::read_to_string(&path).expect("committed schema exists");
    assert_eq!(
        committed, generated,
        "schemas/program-model-v4.schema.json is stale; regenerate with UPDATE_SCHEMA=1"
    );
}

/// Which schema branch describes this instance.
fn branch_for<'a>(schema: &'a Value, instance: &Value) -> &'a Value {
    let Some(branches) = schema.get("oneOf").and_then(Value::as_array) else {
        return schema;
    };
    branches
        .iter()
        .find(|branch| type_matches(branch, instance))
        .unwrap_or_else(|| panic!("no schema branch accepts {instance}"))
}

fn type_matches(schema: &Value, instance: &Value) -> bool {
    let accepts = |name: &str| match name {
        "object" => instance.is_object(),
        "array" => instance.is_array(),
        "string" => instance.is_string(),
        "integer" => instance.is_i64() || instance.is_u64(),
        "number" => instance.is_number(),
        "boolean" => instance.is_boolean(),
        "null" => instance.is_null(),
        _ => false,
    };
    match schema.get("type") {
        Some(Value::String(name)) => accepts(name),
        Some(Value::Array(names)) => names.iter().filter_map(Value::as_str).any(accepts),
        _ => true,
    }
}

/// Walk a serialized model against the schema, in both directions.
///
/// This is the drift check that matters: comparing the committed file to
/// `schema()` only proves the file is current, not that either describes the
/// Rust types. A field added to a struct shows up as an instance key with no
/// schema property; a field removed shows up as a schema property with no
/// instance key. Both fail here.
fn assert_agrees(instance: &Value, schema: &Value, path: &str) {
    let schema = branch_for(schema, instance);
    assert!(
        type_matches(schema, instance),
        "{path}: schema type {:?} does not accept {instance}",
        schema.get("type")
    );
    if let (Some(allowed), Some(text)) = (
        schema.get("enum").and_then(Value::as_array),
        instance.as_str(),
    ) {
        assert!(
            allowed.iter().any(|v| v.as_str() == Some(text)),
            "{path}: {text:?} is not in the schema's enum"
        );
    }
    match instance {
        Value::Object(fields) => {
            if schema.get("additionalProperties") == Some(&Value::Bool(true)) {
                return;
            }
            let properties = schema
                .get("properties")
                .and_then(Value::as_object)
                .unwrap_or_else(|| panic!("{path}: object schema has no properties"));
            for key in fields.keys() {
                assert!(
                    properties.contains_key(key),
                    "{path}: the Rust type serializes {key:?}, which the schema does not declare"
                );
            }
            for key in properties.keys() {
                assert!(
                    fields.contains_key(key),
                    "{path}: the schema declares {key:?}, which the Rust type does not serialize"
                );
            }
            for (key, value) in fields {
                assert_agrees(value, &properties[key], &format!("{path}.{key}"));
            }
        }
        Value::Array(items) => {
            let item_schema = schema
                .get("items")
                .unwrap_or_else(|| panic!("{path}: array schema has no items"));
            for (index, item) in items.iter().enumerate() {
                assert_agrees(item, item_schema, &format!("{path}[{index}]"));
            }
        }
        _ => {}
    }
}

/// The generated schema describes the Rust types, not an older version of them.
#[test]
fn the_schema_matches_the_rust_types() {
    let schema = schema();
    for model in [maximal_model(), unavailable_model()] {
        let instance = model_json(&model);
        assert_agrees(&instance, &schema, "$");
    }
}

/// The schema is closed everywhere except the one object that is documented as
/// open, which is what makes an undeclared field a rejectable condition.
#[test]
fn every_schema_object_except_extensions_is_closed() {
    fn walk(node: &Value, path: &str, open: &mut Vec<String>) {
        if let Some(map) = node.as_object() {
            if map.get("type") == Some(&Value::String("object".to_string()))
                && map.get("additionalProperties") != Some(&Value::Bool(false))
            {
                open.push(path.to_string());
            }
            for (key, value) in map {
                walk(value, &format!("{path}.{key}"), open);
            }
        } else if let Some(items) = node.as_array() {
            for (index, item) in items.iter().enumerate() {
                walk(item, &format!("{path}[{index}]"), open);
            }
        }
    }
    let mut open = Vec::new();
    walk(&schema(), "$", &mut open);
    assert_eq!(
        open,
        vec!["$.properties.extensions".to_string()],
        "exactly one object may be open"
    );
}

#[test]
fn the_schema_pins_the_model_version() {
    assert_eq!(
        schema()["properties"]["model_version"]["const"],
        json!(MODEL_VERSION)
    );
}
