use super::*;
use crate::cluster::{
    FunctionCluster, LibraryCluster, ObjectPoolCluster, TypeCluster, _StringCluster,
};
use crate::info_producer::{
    finish_model, observed_regions, produce_model_object_info, produce_model_object_pool,
};
use crate::profile_reader::profile_json_dictionary::{
    CODE, FOR_EACH, GLOBAL_OBJECT_POOL, HANDLER, OBJECT_STORE, READ_ALLOC, READ_FILL,
};
use crate::program_roots::structs::{DispatchTable, FieldTable, ObjectStore};
use crate::raw_object::{
    Class, Function, Library, ObjectPool, ObjectPoolEntry, ObjectPoolEntryValue, Type, _String,
};
use flutterdec_adapter::model::{
    Capabilities, CapabilityLevel, ClassId, FunctionId, InputRegionName, LibraryId,
    ObjectPool as ModelObjectPool, ObservedInput, PoolEntryKind, PoolIndexSpace, Producer,
    ProducerTrust, ProgramModel, MODEL_VERSION,
};
use flutterdec_adapter::primitives::Sha256Digest;
use flutterdec_adapter::validate::{validate, HostSelectedContext};
use flutterdec_adapter::AdapterRegionInput;
use flutterdec_loader::identity::{
    SnapshotIdentity, SnapshotKind as LoaderSnapshotKind, TargetArch,
};
use serde_json::json;

fn empty_model() -> ProgramModel {
    let regions = [AdapterRegionInput {
        region: InputRegionName::IsolateInstructions,
        bytes: &[0; 32],
        virtual_address: Some(0x1000),
    }];
    ProgramModel {
        model_version: MODEL_VERSION,
        producer: Producer {
            id: "serwalker".into(),
            version: "test".into(),
            artifact_sha256: Sha256Digest::of(b"fixture"),
            trust: ProducerTrust::Untrusted,
        },
        input: ObservedInput {
            identity: SnapshotIdentity::from_header(
                TargetArch::Arm64,
                constants::DART_3_11_1_SNAPSHOT_HASH,
                LoaderSnapshotKind::FullAot,
                "product arm64 compressed-pointers",
            ),
            regions: observed_regions(&regions).unwrap(),
        },
        compatibility: None,
        capabilities: Capabilities::all_unavailable(),
        libraries: Vec::new(),
        classes: Vec::new(),
        functions: Vec::new(),
        object_pool: ModelObjectPool::unavailable(),
        diagnostics: Vec::new(),
        extensions: Default::default(),
    }
}

fn validate_model(model: &ProgramModel) {
    validate(
        model,
        &HostSelectedContext {
            identity: model.input.identity.clone(),
            producer: model.producer.clone(),
            compatibility: model.compatibility.clone(),
            regions: model.input.regions.clone(),
        },
    )
    .unwrap();
    let json = serde_json::to_vec(model).unwrap();
    assert_eq!(*model, ProgramModel::from_json(&json).unwrap());
}

fn fixture() -> (DataSnapshot, InstructionsSnapshot) {
    let mut profile = DataProfile::default();
    for kind in [
        ClassKind::_String,
        ClassKind::Library,
        ClassKind::Class,
        ClassKind::Type,
        ClassKind::Code,
        ClassKind::Function,
        ClassKind::ObjectPool,
    ] {
        profile.class_kind_to_cid.insert(kind, kind as u32);
        profile.cid_map.insert(kind as u32, kind);
    }
    profile.parsing_operations.insert(
        CODE.into(),
        json!({
            (READ_ALLOC): { (HANDLER): CODE, (FOR_EACH): [] },
            (READ_FILL): { (HANDLER): CODE, (FOR_EACH): [] }
        }),
    );
    profile.parsing_operations.insert(
        OBJECT_STORE.into(),
        json!({
            (READ_FILL): { (FOR_EACH): [GLOBAL_OBJECT_POOL] }
        }),
    );
    let mut snapshot = DataSnapshot {
        data_profile: profile,
        instr_table_len: 1,
        ..Default::default()
    };
    let mut insert = |kind: ClassKind, mut cluster: Box<dyn Cluster>| {
        cluster.set_metadata(0, kind as u32, false, false);
        snapshot.clusters.insert((kind as u32) << 2, cluster);
    };
    let mut strings = _StringCluster::default();
    strings.first_ref_id = 10;
    strings.objs = [
        "package:one/a.dart",
        "one",
        "Parent",
        "Child",
        "run",
        "",
        "unknown",
    ]
    .into_iter()
    .map(|text| {
        Box::new(_String {
            code_units: text.encode_utf16().collect(),
            internal_str: text.into(),
            ..Default::default()
        })
    })
    .collect();
    insert(ClassKind::_String, Box::new(strings));
    let mut libraries = LibraryCluster::default();
    libraries.first_ref_id = 30;
    libraries.objs = [10, 1, 10]
        .into_iter()
        .map(|url| {
            Box::new(Library {
                url,
                name: 11,
                ..Default::default()
            })
        })
        .collect();
    insert(ClassKind::Library, Box::new(libraries));
    let mut classes = ClassCluster::default();
    classes.first_ref_id = 40;
    classes.objs = vec![
        Box::new(Class {
            id: 301,
            name: 13,
            library: 32,
            super_type: 60,
            ..Default::default()
        }),
        Box::new(Class {
            id: 300,
            name: 12,
            library: 30,
            super_type: 1,
            ..Default::default()
        }),
        Box::new(Class {
            id: 302,
            name: 1,
            library: 30,
            super_type: 60,
            ..Default::default()
        }),
    ];
    insert(ClassKind::Class, Box::new(classes));
    let mut types = TypeCluster::default();
    types.first_ref_id = 60;
    types.objs.push(Box::new(Type {
        flags: 300 << 3,
        ..Default::default()
    }));
    insert(ClassKind::Type, Box::new(types));
    let mut code = CodeCluster::default();
    let mut code_bytes = Stream::new(&[0x81, 0x81, 0x80]);
    code.read_alloc(&mut 70, &mut code_bytes, &snapshot.data_profile)
        .unwrap();
    code.read_fill(&mut code_bytes, &snapshot.data_profile)
        .unwrap();
    insert(ClassKind::Code, Box::new(code));
    let mut functions = FunctionCluster::default();
    functions.first_ref_id = 80;
    functions.objs = vec![
        Box::new(Function {
            name: 14,
            owner: 40,
            code_index: 1,
            ..Default::default()
        }),
        Box::new(Function {
            name: 1,
            owner: 1,
            code_index: 1,
            ..Default::default()
        }),
        Box::new(Function {
            name: 14,
            owner: 40,
            code_index: 2,
            ..Default::default()
        }),
    ];
    for function in &mut functions.objs {
        function.entry_point = u64::MAX;
        function.unchecked_entry_point = u64::MAX;
    }
    insert(ClassKind::Function, Box::new(functions));
    let values = vec![
        ObjectPoolEntryValue::TaggedObjectRef(15),
        ObjectPoolEntryValue::TaggedObjectRef(1),
        ObjectPoolEntryValue::Immediate(-7),
        ObjectPoolEntryValue::TaggedObjectRef(70),
        ObjectPoolEntryValue::TaggedObjectRef(71),
        ObjectPoolEntryValue::TaggedObjectRef(40),
        ObjectPoolEntryValue::TaggedObjectRef(16),
    ];
    let mut pool = ObjectPoolCluster::default();
    pool.first_ref_id = 90;
    pool.objs.push(Box::new(ObjectPool {
        length: values.len(),
        entries: values
            .into_iter()
            .map(|value| ObjectPoolEntry {
                entry_bits: 0,
                value,
                extra_properties: Default::default(),
            })
            .collect(),
        ..Default::default()
    }));
    insert(ClassKind::ObjectPool, Box::new(pool));
    snapshot.cluster_order = snapshot.clusters.keys().copied().collect();
    snapshot
        .cluster_order
        .sort_by_key(|key| snapshot.clusters[key].first_ref_id());
    snapshot.build_class_table().unwrap();
    let object_store =
        ObjectStore::read(&mut Stream::new(&[0xda]), &snapshot.data_profile).unwrap();
    snapshot.roots = ProgramRoots::new(
        object_store,
        FieldTable::default(),
        FieldTable::default(),
        DispatchTable::default(),
    );
    let mut table = vec![0; 16];
    for word in [0u32, 1, 0, 0, 16, 0] {
        table.extend_from_slice(&word.to_le_bytes());
    }
    snapshot.instruction_table = parse_instr_table_from_rodata(&mut Stream::new(&table)).unwrap();
    snapshot.resolve_entrypoints().unwrap();
    let mut instructions =
        parse_instr_snapshot(&mut Stream::new(&32u64.to_le_bytes()), &mut snapshot).unwrap();
    instructions.image_va = 0x1000;
    (snapshot, instructions)
}

#[test]
fn v4_relationships_use_object_ids_instead_of_display_name_matches() {
    let (snapshot, instructions) = fixture();
    let mut model = empty_model();
    produce_model_object_info(&mut model, &snapshot, &instructions).unwrap();
    finish_model(&mut model);
    assert_eq!(
        model
            .libraries
            .iter()
            .map(|library| library.id)
            .collect::<Vec<_>>(),
        [LibraryId(30), LibraryId(32)]
    );
    assert_eq!(
        model
            .classes
            .iter()
            .map(|class| class.id)
            .collect::<Vec<_>>(),
        [ClassId(300), ClassId(301)]
    );
    assert_eq!(
        model.class(ClassId(301)).unwrap().library,
        Some(LibraryId(32))
    );
    assert_eq!(
        model.class(ClassId(301)).unwrap().super_class,
        Some(ClassId(300))
    );
    assert_eq!(model.functions[0].owner, Some(ClassId(301)));
    assert_eq!(model.functions[0].code.start_va, 0x1010);
    assert_eq!(model.functions[0].code.size, 16);
    assert_eq!(model.functions[0].name_text(), Some("run"));
    validate_model(&model);
}

#[test]
fn v4_unknowns_are_absent_or_preserved_in_extensions_and_deferred_code_is_skipped() {
    let (snapshot, instructions) = fixture();
    let mut model = empty_model();
    produce_model_object_info(&mut model, &snapshot, &instructions).unwrap();
    finish_model(&mut model);
    let function = model.function(FunctionId(81)).unwrap();
    assert!(function.name.is_none());
    assert!(function.owner.is_none());
    assert!(model.function(FunctionId(82)).is_none());
    assert!(model.class(ClassId(302)).is_none());
    assert_eq!(model.extensions["serwalker.class.302"]["name_ref_id"], 1);
    assert_eq!(model.extensions["serwalker.function.81"]["owner_ref_id"], 1);
    assert_eq!(model.extensions["serwalker.library.31"]["uri_ref_id"], 1);
    assert_eq!(model.capabilities.function_names, CapabilityLevel::Partial);
    validate_model(&model);
}

#[test]
fn v4_pool_keeps_hardware_indexes_and_distinguishes_undecoded_values() {
    let (snapshot, instructions) = fixture();
    let mut model = empty_model();
    produce_model_object_pool(&mut model, &snapshot, &instructions).unwrap();
    finish_model(&mut model);
    let pool = &model.object_pool;
    assert_eq!(pool.index_space, PoolIndexSpace::Hardware);
    assert_eq!(pool.geometry.unwrap().index_for_displacement(16), Some(0));
    assert_eq!(pool.entries[0].kind, PoolEntryKind::String);
    assert_eq!(pool.entries[0].value.as_deref(), Some("\"\""));
    assert_eq!(pool.entries[1].kind, PoolEntryKind::Undecoded);
    assert!(pool.entries[1].value.is_none());
    assert_eq!(pool.entries[2].value.as_deref(), Some("-7"));
    assert_eq!(pool.entries[3].kind, PoolEntryKind::Code);
    assert_eq!(pool.entries[3].target_va, Some(0x1010));
    assert_eq!(pool.entries[4].kind, PoolEntryKind::Undecoded);
    assert_eq!(pool.entries[5].kind, PoolEntryKind::Class);
    assert_eq!(pool.entries[6].value.as_deref(), Some("\"unknown\""));
    assert!(pool.entries.iter().all(|entry| entry.confidence.is_none()));
    assert_eq!(
        model.extensions["serwalker.pool.1"]["serialized_value"],
        "TaggedObjectRef(1)"
    );
    validate_model(&model);
}

#[test]
fn shipped_profiles_export_lossless_utf16_pool_constants() {
    let code_units: [Vec<u16>; 9] = [
        vec![0xd800],
        vec![0xdc00],
        vec![0xfffd],
        vec![0xd83d, 0xde00],
        r"\uD800".encode_utf16().collect(),
        "\"\\\0\u{8}\u{c}\n\r\t\u{1f}caf\u{e9}"
            .encode_utf16()
            .collect(),
        Vec::new(),
        vec![0xd800, 0xd801, 0x61, 0xdc00, 0xdc01],
        vec![0xdc00, 0xd800],
    ];
    let expected = [
        r#""\uD800""#.to_owned(),
        r#""\uDC00""#.to_owned(),
        serde_json::to_string("\u{fffd}").unwrap(),
        serde_json::to_string("\u{1f600}").unwrap(),
        serde_json::to_string(r"\uD800").unwrap(),
        serde_json::to_string("\"\\\0\u{8}\u{c}\n\r\t\u{1f}caf\u{e9}").unwrap(),
        "\"\"".to_owned(),
        r#""\uD800\uD801a\uDC00\uDC01""#.to_owned(),
        r#""\uDC00\uD800""#.to_owned(),
    ];
    let single_byte_unsigned = |value: usize| {
        u8::try_from(value)
            .unwrap()
            .checked_add(constants::UNSIGNED_M)
            .unwrap()
    };
    let mut bytes = vec![single_byte_unsigned(code_units.len())];
    bytes.extend(
        code_units
            .iter()
            .map(|units| single_byte_unsigned((units.len() << 1) | 1)),
    );
    for units in &code_units {
        bytes.push(single_byte_unsigned((units.len() << 1) | 1));
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    }

    for hash in [
        "80a49c7111088100a233b2ae788e1f48",
        constants::DART_3_11_1_SNAPSHOT_HASH,
    ] {
        let profile = crate::profile_reader::select_data_profile(
            hash,
            &[
                "product".into(),
                "arm64".into(),
                "compressed-pointers".into(),
            ],
        )
        .unwrap();
        let cid = profile.class_kind_to_cid[&ClassKind::_String];
        let mut strings = _StringCluster::default();
        strings.set_metadata(0, cid, false, false);
        let mut stream = Stream::new(&bytes);
        let mut next_ref_id = 10;
        strings
            .read_alloc(&mut next_ref_id, &mut stream, &profile)
            .unwrap();
        strings.read_fill(&mut stream, &profile).unwrap();
        assert_eq!(stream.get_current_pos(), bytes.len());
        for (string, units) in strings.objs.iter().zip(&code_units) {
            assert_eq!(&string.code_units, units, "{hash}");
        }
        let key = cid << 2;
        let mut snapshot = DataSnapshot {
            data_profile: profile,
            clusters: [(key, Box::new(strings) as Box<dyn Cluster>)].into(),
            cluster_order: vec![key],
            ..Default::default()
        };
        let pool = ObjectPool {
            length: code_units.len(),
            entries: (10..u32::try_from(next_ref_id).unwrap())
                .map(|reference_id| ObjectPoolEntry {
                    entry_bits: 0,
                    value: ObjectPoolEntryValue::TaggedObjectRef(reference_id),
                    extra_properties: Default::default(),
                })
                .collect(),
            ..Default::default()
        };
        let pool_cid = snapshot.cid_for_class_kind(ClassKind::ObjectPool).unwrap();
        let pool_key = pool_cid << 2;
        let mut pools = ObjectPoolCluster::default();
        pools.set_metadata(0, pool_cid, false, false);
        pools.first_ref_id = 30;
        pools.objs.push(Box::new(pool));
        snapshot.clusters.insert(pool_key, Box::new(pools));
        snapshot.cluster_order.push(pool_key);
        let object_store_bytes: Vec<_> = snapshot.data_profile.parsing_operations[OBJECT_STORE]
            [READ_FILL][FOR_EACH]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| {
                single_byte_unsigned(if field.as_str() == Some(GLOBAL_OBJECT_POOL) {
                    30
                } else {
                    0
                })
            })
            .collect();
        let object_store = ObjectStore::read(
            &mut Stream::new(&object_store_bytes),
            &snapshot.data_profile,
        )
        .unwrap();
        snapshot.roots = ProgramRoots::new(
            object_store,
            FieldTable::default(),
            FieldTable::default(),
            DispatchTable::default(),
        );
        let mut model = empty_model();
        model.input.identity = SnapshotIdentity::from_header(
            TargetArch::Arm64,
            hash,
            LoaderSnapshotKind::FullAot,
            "product arm64 compressed-pointers",
        );
        produce_model_object_pool(&mut model, &snapshot, &InstructionsSnapshot::default()).unwrap();
        finish_model(&mut model);
        validate_model(&model);
        let exported = serde_json::to_vec(&model).unwrap();
        let restored = ProgramModel::from_json(&exported).unwrap();
        let mut distinct = std::collections::HashSet::new();
        for (index, (entry, expected_value)) in restored
            .object_pool
            .entries
            .iter()
            .zip(&expected)
            .enumerate()
        {
            assert_eq!(entry.kind, PoolEntryKind::String);
            assert_eq!(
                entry.provenance,
                flutterdec_adapter::model::Provenance::Exact
            );
            assert_eq!(entry.value.as_ref(), Some(expected_value), "{hash}");
            assert!(distinct.insert(entry.value.as_ref().unwrap()), "{hash}");
            if [0, 1, 7, 8].contains(&index) {
                let extension = &restored.extensions[&format!("serwalker.pool.{index}")];
                assert_eq!(extension["reference_id"], 10 + index);
                assert_eq!(extension["string_value_encoding"], "utf16_escaped_literal");
                assert_eq!(extension["string_code_units"], json!(code_units[index]));
            }
        }
    }
}

#[test]
fn unpaired_surrogates_are_not_exported_as_exact_replacement_character_names() {
    for (code_unit, expected_name) in [(0xd800, None), (0xdc00, None), (0xfffd, Some("\u{fffd}"))] {
        let (mut snapshot, instructions) = fixture();
        let strings = snapshot
            .clusters
            .get_mut(&((ClassKind::_String as u32) << 2))
            .unwrap()
            .as_any_mut()
            .downcast_mut::<_StringCluster>()
            .unwrap();
        let name = &mut strings.objs[4];
        name.code_units = vec![code_unit];
        name.internal_str = String::from_utf16_lossy(&name.code_units);
        let mut model = empty_model();
        produce_model_object_info(&mut model, &snapshot, &instructions).unwrap();
        finish_model(&mut model);
        assert_eq!(model.functions[0].name_text(), expected_name);
        validate_model(&model);
    }
}

#[test]
fn v4_empty_domains_have_diagnostics_and_validate() {
    let mut model = empty_model();
    finish_model(&mut model);
    assert_eq!(model.capabilities, Capabilities::all_unavailable());
    assert_eq!(model.diagnostics.len(), 7);
    validate_model(&model);
}

#[test]
fn built_in_input_needs_no_external_authorization_and_records_its_profile() {
    let seed = empty_model();
    let regions = [AdapterRegionInput {
        region: InputRegionName::IsolateInstructions,
        bytes: &[0; 32],
        virtual_address: Some(0x1000),
    }];
    let digest = Sha256Digest::of(b"profile");
    let input = crate::SnapshotInput {
        identity: &seed.input.identity,
        regions: &regions,
        producer: &seed.producer,
        compatibility: None,
        profile_path: std::path::Path::new("profile.json"),
        profile_sha256: &digest,
    };
    let mut model = crate::info_producer::produce_model_headers(&input).unwrap();
    assert!(model.compatibility.is_none());
    assert_eq!(
        model.extensions["serwalker.profile"]["file_name"],
        "profile.json"
    );
    assert_eq!(
        model.extensions["serwalker.profile"]["sha256"],
        digest.as_str()
    );
    finish_model(&mut model);
    validate_model(&model);
}

#[test]
fn public_input_checks_matching_conflicting_and_absent_profile_bindings() {
    use flutterdec_adapter::model::CompatibilityBinding;

    let seed = empty_model();
    let profile_dir = std::env::var_os("FLUTTERDEC_PROFILES_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../data/serwalker_profiles")
        });
    let profile_path = profile_dir.join(
        crate::profile_file_name(
            constants::DART_3_11_1_SNAPSHOT_HASH,
            &seed.input.identity.features.normalized,
        )
        .unwrap(),
    );
    let digest = Sha256Digest::of(&std::fs::read(&profile_path).unwrap());
    let matching = CompatibilityBinding {
        record_sha256: Sha256Digest::of(b"record"),
        parser_family_id: "serwalker".into(),
        profile_id: "test-profile".into(),
        profile_sha256: digest.clone(),
    };
    let conflicting = CompatibilityBinding {
        profile_sha256: Sha256Digest::of(b"different-profile"),
        ..matching.clone()
    };
    let missing_region = format!("The {} stream was not found!", InputRegionName::IsolateData);
    for (binding, expected_error) in [
        (Some(&matching), missing_region.as_str()),
        (None, missing_region.as_str()),
        (
            Some(&conflicting),
            "Serwalker profile digest does not match the compatibility binding",
        ),
    ] {
        let input = crate::SnapshotInput {
            identity: &seed.input.identity,
            regions: &[],
            producer: &seed.producer,
            compatibility: binding,
            profile_path: &profile_path,
            profile_sha256: &digest,
        };
        assert_eq!(
            crate::walk_snapshot(&input).unwrap_err().to_string(),
            expected_error
        );
    }
}

#[test]
fn v4_region_lookup_hashing_and_address_checks() {
    let mut regions = vec![
        AdapterRegionInput {
            region: InputRegionName::IsolateInstructions,
            bytes: b"instructions",
            virtual_address: Some(0x1000),
        },
        AdapterRegionInput {
            region: InputRegionName::IsolateData,
            bytes: b"data",
            virtual_address: None,
        },
    ];
    assert!(std::ptr::eq(
        crate::utils::get_snapshot_region(&regions, InputRegionName::IsolateData).unwrap(),
        &regions[1]
    ));
    let observed = observed_regions(&regions).unwrap();
    assert_eq!(observed[0].region, InputRegionName::IsolateData);
    assert_eq!(observed[0].sha256, Sha256Digest::of(b"data"));
    assert!(crate::utils::get_snapshot_region(&regions, InputRegionName::VmData).is_err());
    regions[0].virtual_address = None;
    assert!(observed_regions(&regions).is_err());
    regions[0] = regions[1];
    assert!(observed_regions(&regions).is_err());
}

#[test]
fn v4_supplied_profile_is_used_without_an_independent_filename_lookup() {
    let bytes = serde_json::to_vec(
        &json!({"parsing_operations": {}, "num_predefined_cids": 700,
        "class_tag_kind": "object_header", "cid_map": {"123": "class"}}),
    )
    .unwrap();
    let features = vec![
        "product".into(),
        "arm64".into(),
        "compressed-pointers".into(),
    ];
    let profile = crate::profile_reader::data_profile_from_bytes(&bytes, &features).unwrap();
    let mut header = constants::MAGIC_BYTES.to_le_bytes().to_vec();
    header.extend_from_slice(&0u64.to_le_bytes());
    header.extend_from_slice(&3u64.to_le_bytes());
    header.extend_from_slice(constants::DART_3_11_1_SNAPSHOT_HASH.as_bytes());
    header.extend_from_slice(b"product arm64 compressed-pointers\0");
    header.extend_from_slice(&[0x80; 5]);
    let mut snapshot = DataSnapshot::default();
    snapshot
        .parse_header(&mut Stream::new(&header), Some(profile))
        .unwrap();
    assert_eq!(snapshot.cid_for_class_kind(ClassKind::Class).unwrap(), 123);
    assert_eq!(snapshot.data_profile.num_predefined_cids, 700);
}

#[test]
#[ignore = "requires SERWALKER_TEST_LIBAPP and SERWALKER_TEST_PROFILE"]
fn v4_real_snapshot_round_trips_and_passes_host_validation() {
    use flutterdec_adapter::model::CompatibilityBinding;
    use flutterdec_adapter::registry::CompatibilityRecord;
    use flutterdec_adapter::{AdapterInput, HostAuthorization, Limits};
    use std::path::PathBuf;

    let libapp_path = PathBuf::from(std::env::var_os("SERWALKER_TEST_LIBAPP").unwrap());
    let profile_path = PathBuf::from(std::env::var_os("SERWALKER_TEST_PROFILE").unwrap());
    let bundle = flutterdec_loader::load_snapshot_bundle(&libapp_path).unwrap();
    let profile_digest = Sha256Digest::of(&std::fs::read(&profile_path).unwrap());
    let artifact_digest = Sha256Digest::of(b"serwalker-test");
    let record: CompatibilityRecord = serde_json::from_value(json!({
        "snapshot_hash": bundle.snapshot_hash,
        "snapshot_kind": bundle.identity.kind,
        "target_arch": bundle.identity.target_arch,
        "features": bundle.identity.features.normalized,
        "feature_fingerprint": Sha256Digest::of(b"test-features"),
        "parser_family": {"id": "serwalker"},
        "profile": {"id": "test-profile", "path": "profile.json", "sha256": profile_digest},
        "artifact": {"id": "serwalker-test", "variants": [{"host_os": "linux", "host_arch": "x86_64",
            "path": "serwalker-test", "size": 14, "sha256": artifact_digest, "provenance": "test"}]},
        "evidence": {"source": "test", "provenance": "test"},
        "trust_tier": "experimental", "protocol_major": 1, "model_major": 4
    })).unwrap();
    let input = AdapterInput {
        identity: &bundle.identity,
        authorization: HostAuthorization {
            record: &record,
            variant: &record.artifact.variants[0],
            store_root: libapp_path.parent().unwrap(),
            profile_path: &profile_path,
        },
        producer: Producer {
            id: "serwalker-test".into(),
            version: "test".into(),
            artifact_sha256: artifact_digest,
            trust: ProducerTrust::Untrusted,
        },
        compatibility: CompatibilityBinding {
            record_sha256: Sha256Digest::of(&serde_json::to_vec(&record).unwrap()),
            parser_family_id: "serwalker".into(),
            profile_id: "test-profile".into(),
            profile_sha256: profile_digest,
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
        input_path: Some(&libapp_path),
        libapp: None,
        requested_backend: flutterdec_adapter::protocol::RequestedBackend::Auto,
        limits: Limits::default(),
    };
    let mut invalid = input.clone();
    invalid.compatibility.profile_sha256 = Sha256Digest::of(b"wrong-profile");
    assert!(crate::walk_snapshot_and_produce_model(&invalid)
        .unwrap_err()
        .to_string()
        .contains("authorized profile digest"));
    let model = crate::walk_snapshot_and_produce_model(&input).unwrap();
    assert!(!model.functions.is_empty());
    assert_eq!(model.producer, input.producer);
    assert_eq!(model.compatibility.as_ref(), Some(&input.compatibility));
    assert_eq!(model.input.identity, bundle.identity);
    validate_model(&model);
    let verified_digest = input.compatibility.profile_sha256.clone();
    let conflicting = CompatibilityBinding {
        profile_sha256: Sha256Digest::of(b"different-profile"),
        ..input.compatibility.clone()
    };
    let mut direct_input = crate::SnapshotInput {
        identity: input.identity,
        regions: &input.regions,
        producer: &input.producer,
        compatibility: Some(&conflicting),
        profile_path: &profile_path,
        profile_sha256: &verified_digest,
    };
    assert!(crate::walk_snapshot(&direct_input)
        .unwrap_err()
        .to_string()
        .contains("compatibility binding"));
    direct_input.compatibility = Some(&input.compatibility);
    assert_eq!(crate::walk_snapshot(&direct_input).unwrap(), model);
    direct_input.compatibility = None;
    let native_model = crate::walk_snapshot(&direct_input).unwrap();
    let mut expected_native = model.clone();
    expected_native.compatibility = None;
    assert_eq!(native_model, expected_native);
    validate_model(&native_model);
    eprintln!(
        "v4 real snapshot: {} libraries, {} classes, {} functions, {} pool entries",
        model.libraries.len(),
        model.classes.len(),
        model.functions.len(),
        model.object_pool.entries.len()
    );
}
