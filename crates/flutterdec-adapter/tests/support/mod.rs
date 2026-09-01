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
        compatibility: Some(compatibility()),
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
        compatibility: Some(compatibility()),
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

/// A registry-authorized adapter install.
///
/// Everything the host checks before it spawns lives here and is consistent by
/// construction: the executable is published where the record's host variant
/// says, the profile the record pins is on disk with the digest the record
/// declares, and the compatibility binding carries the record's own digest. A
/// negative case is then one field changed, which is what makes "the host
/// refused because of *this*" a claim a test can make.
pub struct Authorized {
    _dir: tempfile::TempDir,
    pub exec: std::path::PathBuf,
    pub store_root: std::path::PathBuf,
    pub profile_path: std::path::PathBuf,
    pub record: flutterdec_loader::registry::CompatibilityRecord,
}

pub const PROFILE_FILE: &str = "dart-profiles.json";
pub const PROFILE_BODY: &[u8] = br#"{"profiles":{}}"#;
pub const PARSER_FAMILY: &str = "flutterdec-local-python";

impl Authorized {
    /// Publish `source` as the adapter this identity's record authorizes.
    pub fn install(source: &std::path::Path, identity: &SnapshotIdentity) -> Self {
        Self::install_named(source, identity, None)
    }

    /// Publish under an arbitrary file name, so a deliberately misleading name
    /// can be shown to change nothing.
    pub fn install_named(
        source: &std::path::Path,
        identity: &SnapshotIdentity,
        file_name: Option<&str>,
    ) -> Self {
        use flutterdec_loader::registry::*;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let key = identity
            .exact_selection_key()
            .expect("a fixture identity must clear the gate");
        let dir = tempfile::TempDir::new().expect("tempdir");
        let store_root = dir.path().join("store");
        let data_root = dir.path().join("data");
        fs::create_dir_all(store_root.join("artifacts")).expect("mkdir store");
        fs::create_dir_all(&data_root).expect("mkdir data");

        // Named after the parser family, not after a snapshot hash: one
        // producer serves every record that names it, and a hash-derived file
        // name is the retired one-wrapper-per-snapshot layout.
        let name = file_name.unwrap_or("flutterdec-local-python").to_string();
        let relative = format!("artifacts/{name}");
        let exec = store_root.join(&relative);
        fs::copy(source, &exec).expect("publish adapter artifact");
        fs::set_permissions(&exec, fs::Permissions::from_mode(0o755)).expect("chmod");
        let bytes = fs::read(&exec).expect("read adapter artifact");

        let profile_path = data_root.join(PROFILE_FILE);
        fs::write(&profile_path, PROFILE_BODY).expect("write profile");

        let record = CompatibilityRecord {
            snapshot_hash: key.hash.clone(),
            snapshot_kind: SnapshotKind::FullAot,
            target_arch: key.target_arch.clone(),
            feature_fingerprint: canonical_feature_fingerprint(&key.features),
            features: key.features.clone(),
            known_features: Vec::new(),
            forbidden_features: Vec::new(),
            sdk_aliases: Vec::new(),
            parser_family: ParserFamilyReference {
                id: PARSER_FAMILY.to_string(),
                version: Some("fixture".to_string()),
                sha256: None,
            },
            profile: ProfileReference {
                id: "fixture-profile".to_string(),
                path: PROFILE_FILE.to_string(),
                sha256: hex_digest(PROFILE_BODY),
            },
            artifact: ArtifactReference {
                id: "fixture-artifact".to_string(),
                variants: vec![HostArtifactVariant {
                    host_os: std::env::consts::OS.to_string(),
                    host_arch: std::env::consts::ARCH.to_string(),
                    path: relative,
                    size: bytes.len() as u64,
                    sha256: hex_digest(&bytes),
                    provenance: "fixture".to_string(),
                }],
            },
            evidence: CompatibilityEvidence {
                source: "fixture".to_string(),
                provenance: "test".to_string(),
                references: Vec::new(),
            },
            trust_tier: TrustTier::Verified,
            protocol_major: 1,
            model_major: 4,
        };
        record.validate().expect("the fixture record is valid");

        Self {
            _dir: dir,
            exec,
            store_root,
            profile_path,
            record,
        }
    }

    /// The authorization for a record, so a negative case can hand in a tweaked
    /// copy and keep the variant pointing into it.
    pub fn authorization_for<'a>(
        &'a self,
        record: &'a flutterdec_loader::registry::CompatibilityRecord,
    ) -> flutterdec_adapter::HostAuthorization<'a> {
        flutterdec_adapter::HostAuthorization {
            record,
            variant: record
                .artifact
                .variants
                .first()
                .expect("the fixture record declares one variant"),
            store_root: &self.store_root,
            profile_path: &self.profile_path,
        }
    }

    pub fn authorization(&self) -> flutterdec_adapter::HostAuthorization<'_> {
        self.authorization_for(&self.record)
    }

    /// The producer record that follows from a record and the published bytes.
    pub fn producer_for(
        &self,
        record: &flutterdec_loader::registry::CompatibilityRecord,
    ) -> Producer {
        Producer {
            id: record.parser_family.id.clone(),
            version: record
                .parser_family
                .version
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            // The bytes on disk when they are readable, so a case that rewrites
            // the artifact still hands in a producer record that matches it.
            // When the published path is not a readable file at all, fall back
            // to what the record declares: the file-type gate is what such a
            // case is about, and it runs long before the producer record.
            // The file type is checked before the read because opening a FIFO
            // blocks until someone writes to it, and one of the gate cases
            // publishes exactly that.
            artifact_sha256: match std::fs::symlink_metadata(&self.exec)
                .ok()
                .filter(|meta| meta.is_file())
                .and_then(|_| std::fs::read(&self.exec).ok())
            {
                Some(bytes) => Sha256Digest::of(&bytes),
                None => Sha256Digest::parse(&record.artifact.variants[0].sha256)
                    .expect("the record declares a hex digest"),
            },
            trust: ProducerTrust::Registered,
        }
    }

    pub fn producer(&self) -> Producer {
        self.producer_for(&self.record)
    }

    /// The compatibility binding that follows from a record.
    pub fn binding_for(
        &self,
        record: &flutterdec_loader::registry::CompatibilityRecord,
    ) -> CompatibilityBinding {
        CompatibilityBinding {
            record_sha256: Sha256Digest::parse(&record.sha256().expect("record digest"))
                .expect("record digest is hex"),
            parser_family_id: record.parser_family.id.clone(),
            profile_id: record.profile.id.clone(),
            profile_sha256: Sha256Digest::parse(&record.profile.sha256)
                .expect("profile digest is hex"),
        }
    }

    pub fn binding(&self) -> CompatibilityBinding {
        self.binding_for(&self.record)
    }
}

pub fn hex_digest(bytes: &[u8]) -> String {
    Sha256Digest::of(bytes).as_str().to_string()
}
