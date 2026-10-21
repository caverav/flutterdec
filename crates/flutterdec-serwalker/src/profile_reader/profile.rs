use serde::Deserialize;
use std::collections::HashMap;

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
    pub cid_map: HashMap<String, u32>,
    pub parsing_operations: HashMap<String, serde_json::Value>,
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
}
