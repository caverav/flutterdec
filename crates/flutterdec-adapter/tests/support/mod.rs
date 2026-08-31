//! Fixture builders shared by the v4 model and protocol test suites.
//!
//! Everything here goes through the crate's public API only, and every test
//! that consumes a fixture serializes it and parses the bytes back, so the
//! suites exercise fresh JSON rather than in-memory structs that never crossed
//! the boundary.
//!
//! Each test binary links its own copy of this module and uses part of it, so
//! the unused-item warnings that would follow are an artifact of that layout.
#![allow(dead_code)]

use flutterdec_adapter::model::{
    Capabilities, CapabilityLevel, Class, ClassId, CodeRange, CompatibilityBinding, Diagnostic,
    DiagnosticCode, DiagnosticSeverity, Function, FunctionId, InputRegion, InputRegionName,
    Library, LibraryId, Name, ObjectPool, ObservedInput, PoolEntry, PoolEntryKind, PoolGeometry,
    PoolIndexSpace, Producer, ProducerTrust, ProgramModel, Provenance, MODEL_VERSION,
};
use flutterdec_adapter::primitives::Sha256Digest;
use flutterdec_adapter::validate::HostSelectedContext;
use flutterdec_loader::identity::{SnapshotIdentity, SnapshotKind, TargetArch};

pub const HASH: &str = "80a49c7111088100a233b2ae788e1f48";
pub const FEATURES: &str = "product no-code_comments arm64 android compressed-pointers";

pub const VM_INSTR_VA: u64 = 0x1000;
pub const VM_INSTR_SIZE: u64 = 0x100;
pub const ISO_INSTR_VA: u64 = 0x2000;
pub const ISO_INSTR_SIZE: u64 = 0x200;

pub fn identity() -> SnapshotIdentity {
    SnapshotIdentity::from_header(TargetArch::Arm64, HASH, SnapshotKind::FullAot, FEATURES)
}

pub fn digest(seed: &str) -> Sha256Digest {
    Sha256Digest::of(seed.as_bytes())
}

pub fn regions() -> Vec<InputRegion> {
    vec![
        InputRegion {
            region: InputRegionName::VmData,
            size: 64,
            sha256: digest("vm_data"),
            virtual_address: None,
            executable: false,
        },
        InputRegion {
            region: InputRegionName::IsolateData,
            size: 128,
            sha256: digest("isolate_data"),
            virtual_address: None,
            executable: false,
        },
        InputRegion {
            region: InputRegionName::VmInstructions,
            size: VM_INSTR_SIZE,
            sha256: digest("vm_instr"),
            virtual_address: Some(VM_INSTR_VA),
            executable: true,
        },
        InputRegion {
            region: InputRegionName::IsolateInstructions,
            size: ISO_INSTR_SIZE,
            sha256: digest("isolate_instr"),
            virtual_address: Some(ISO_INSTR_VA),
            executable: true,
        },
    ]
}

pub fn producer() -> Producer {
    Producer {
        id: "dartaot".to_string(),
        version: "3.5.0".to_string(),
        artifact_sha256: digest("artifact"),
        trust: ProducerTrust::Registered,
    }
}

pub fn compatibility() -> CompatibilityBinding {
    CompatibilityBinding {
        record_sha256: digest("record"),
        parser_family_id: "dartaot-arm64".to_string(),
        profile_id: "dart-3.5-arm64".to_string(),
        profile_sha256: digest("profile"),
    }
}

pub fn host() -> HostSelectedContext {
    HostSelectedContext {
        identity: identity(),
        producer: producer(),
        compatibility: compatibility(),
        regions: regions(),
    }
}

/// A model where every optional field is populated and every enum that appears
/// in the schema's `properties` is reachable.
///
/// The drift check walks this value against the committed schema in both
/// directions, so it has to leave nothing out.
pub fn maximal_model() -> ProgramModel {
    ProgramModel {
        model_version: MODEL_VERSION,
        producer: producer(),
        input: ObservedInput {
            identity: identity(),
            regions: regions(),
        },
        compatibility: compatibility(),
        capabilities: Capabilities {
            libraries: CapabilityLevel::Complete,
            classes: CapabilityLevel::Complete,
            class_relationships: CapabilityLevel::Partial,
            // A heuristic code range is present, so this cannot be complete.
            functions: CapabilityLevel::Partial,
            // One function has no name at all.
            function_names: CapabilityLevel::Partial,
            // The pool carries an undecoded slot.
            object_pool: CapabilityLevel::Partial,
            pool_index_space: CapabilityLevel::Complete,
        },
        libraries: vec![
            Library {
                id: LibraryId(1),
                uri: "dart:core".to_string(),
                display_name: Some("core".to_string()),
                provenance: Provenance::Exact,
            },
            Library {
                id: LibraryId(2),
                uri: "package:app/main.dart".to_string(),
                display_name: None,
                provenance: Provenance::Derived,
            },
        ],
        classes: vec![
            Class {
                id: ClassId(1),
                name: "Object".to_string(),
                library: Some(LibraryId(1)),
                super_class: None,
                provenance: Provenance::Exact,
            },
            Class {
                id: ClassId(2),
                name: "App".to_string(),
                library: Some(LibraryId(2)),
                super_class: Some(ClassId(1)),
                provenance: Provenance::Exact,
            },
        ],
        functions: vec![
            Function {
                id: FunctionId(1),
                name: Some(Name::exact("build")),
                owner: Some(ClassId(2)),
                code: CodeRange {
                    start_va: ISO_INSTR_VA,
                    size: 0x40,
                },
                code_section_va: ISO_INSTR_VA,
                provenance: Provenance::Exact,
            },
            // A code range with no name: exactly the case v3 could not express
            // without inventing one.
            Function {
                id: FunctionId(2),
                name: None,
                owner: None,
                code: CodeRange {
                    start_va: VM_INSTR_VA,
                    size: 0x20,
                },
                code_section_va: VM_INSTR_VA,
                provenance: Provenance::Heuristic,
            },
        ],
        object_pool: ObjectPool {
            index_space: PoolIndexSpace::Hardware,
            geometry: Some(PoolGeometry {
                entries_offset: 0x10,
                word_size: 8,
            }),
            entries: vec![
                PoolEntry {
                    index: 0,
                    kind: PoolEntryKind::String,
                    value: Some("build".to_string()),
                    target_va: None,
                    provenance: Provenance::Exact,
                    confidence: None,
                },
                PoolEntry {
                    index: 3,
                    kind: PoolEntryKind::Code,
                    value: Some("App.build".to_string()),
                    target_va: Some(ISO_INSTR_VA),
                    provenance: Provenance::Exact,
                    confidence: None,
                },
                PoolEntry {
                    index: 5,
                    kind: PoolEntryKind::Undecoded,
                    value: None,
                    target_va: None,
                    provenance: Provenance::Exact,
                    confidence: None,
                },
                PoolEntry {
                    index: 7,
                    kind: PoolEntryKind::Selector,
                    value: Some("onTap".to_string()),
                    target_va: Some(ISO_INSTR_VA + 0x40),
                    provenance: Provenance::Heuristic,
                    confidence: Some(0.5),
                },
            ],
        },
        diagnostics: vec![Diagnostic {
            code: DiagnosticCode::DomainPartiallyRecovered,
            severity: DiagnosticSeverity::Warning,
            subject: Some("object_pool".to_string()),
            message: "one pool slot was read but not decoded".to_string(),
        }],
        extensions: [(
            "vendor".to_string(),
            serde_json::json!({ "build": "local" }),
        )]
        .into_iter()
        .collect(),
    }
}

/// The model a producer must emit when it recovered nothing.
///
/// This is the shape v3 had no way to write: no libraries, no classes, no
/// functions, no pool, and a stated reason for each.
pub fn unavailable_model() -> ProgramModel {
    let mut model = maximal_model();
    model.capabilities = Capabilities::all_unavailable();
    model.libraries.clear();
    model.classes.clear();
    model.functions.clear();
    model.object_pool = ObjectPool::unavailable();
    model.diagnostics = flutterdec_adapter::model::Domain::ALL
        .iter()
        .map(|domain| Diagnostic::unavailable(*domain, "no parser for this snapshot identity"))
        .collect();
    model.extensions.clear();
    model
}
