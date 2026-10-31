use serde::Deserialize;
use std::collections::HashMap;

use crate::constants::ClassKind;

#[derive(Default)]
pub enum SnapshotFeatures {
    #[default]
    CompressedPointers,
}

impl TryFrom<&String> for SnapshotFeatures {
    type Error = anyhow::Error;
    fn try_from(value: &String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "compressed-pointers" => Ok(Self::CompressedPointers),
            _ => Err(anyhow::anyhow!("Not a recognized feature.")),
        }
    }
}

// The architecture is ALSO a snapshot feature, but it's one we'll treat differently for the sake of clarity.
#[derive(Default)]
pub enum SnapshotArchitecture {
    ArchX86,
    ArchX64,
    #[default]
    ArchArm64,
    ArchARM,
    ArchRISCV32,
    ArchRISCV64,
}

impl TryFrom<&String> for SnapshotArchitecture {
    type Error = anyhow::Error;
    fn try_from(value: &String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "ia32" => Ok(Self::ArchX86),
            "x64" => Ok(Self::ArchX64),
            "arm64" => Ok(Self::ArchArm64),
            "arm" => Ok(Self::ArchARM),
            "riscv32" => Ok(Self::ArchRISCV32),
            "riscv64" => Ok(Self::ArchRISCV64),
            _ => Err(anyhow::anyhow!("Not a recognized architecture.")),
        }
    }
}

#[derive(Default)]
pub struct DataProfile {
    pub(super) architecture: SnapshotArchitecture,
    pub(super) features: Vec<SnapshotFeatures>,
    pub cid_map: HashMap<u32, ClassKind>, // REAL cid to stable kind lookup
    pub class_kind_to_cid: HashMap<ClassKind, u32>, // and the inverse to the map above
    pub num_predefined_cids: u32,
    pub class_tag_kind: ClassTagKind,
    pub parsing_operations: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassTagKind {
    PackedCidAndCanonical,
    #[default]
    ObjectHeader,
    ObjectHeaderDeeplyImmutable,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum ProfileOperation<F> {
    Core {
        target: F,         // the enum vocabulary (see raw_object/object_vocabulary.rs)
        codec: TargetType, // the type, used to determine which stream reading primitive is used
    },

    Extra {
        target: String, // non-core field
        codec: TargetType,
    },

    Context {
        slot: String, // contextual field, for example, the number of objects to be read is read, but not stored.
        codec: TargetType,
    },

    Discard {
        // read and discard.
        codec: TargetType,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetType {
    RefId,
    Unsigned,
    Signed,
    Byte,
    RawU32,
    RawU64,
    Bytes,
    Array {
        length_from: String,
        #[serde(default = "default_array_multiplier")]
        multiplier: usize,
        for_each: Vec<serde_json::Value>,
    },
}

fn default_array_multiplier() -> usize {
    1
}
