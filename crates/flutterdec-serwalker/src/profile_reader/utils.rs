use crate::profile_reader::profile::{SnapshotArchitecture, SnapshotFeatures};
use std::{collections::HashMap, env, fs, hash::Hash};
use anyhow::Context;
use serde_json;

pub fn features_suffix(features: &Vec<SnapshotFeatures>) -> String {
    let mut feature_string = String::new();

    for (idx, feature) in features.iter().enumerate() {
        match feature {
            SnapshotFeatures::CompressedPointers => feature_string += "compr",
            // maybe add more in the future, for now, its just this
        }

        if idx == features.len() - 1 {
            break;
        }

        feature_string += "-";
    }

    feature_string
}

pub fn architecture_suffix(arch: &SnapshotArchitecture) -> String {
    let arch_string = match arch {
        SnapshotArchitecture::ArchARM => String::from("arm"),
        SnapshotArchitecture::ArchRISCV32 => String::from("riscv32"),
        SnapshotArchitecture::ArchRISCV64 => String::from("riscv64"),
        SnapshotArchitecture::ArchX64 => String::from("x64"),
        SnapshotArchitecture::ArchX86 => String::from("ia32"),
        SnapshotArchitecture::ArchArm64 => String::from("arm64"),
    };

    arch_string
}

pub(super) fn resolve_snapshot_architecture(
    features_string: &Vec<String>,
) -> anyhow::Result<SnapshotArchitecture> {
    for feature in features_string {
        if let Ok(arch) = SnapshotArchitecture::try_from(feature) {
            return Ok(arch);
        }
    }

    Err(anyhow::anyhow!(
        "Architecture not found in the features string..."
    ))
}

const PROFILE_PATH_ENV_VAR_NAME: &str = "PROFILES_DIR";

pub(super) fn read_profile_file(file_name: &String) -> anyhow::Result<HashMap<String, serde_json::Value>> {
    if let Ok(profiles_dirpath) = env::var(PROFILE_PATH_ENV_VAR_NAME) {
        let full_path = profiles_dirpath + "/" + file_name;
        let profile_file = fs::read(&full_path).with_context(|| format!("read data profile: {}", full_path))?;

        let profile = serde_json::from_slice::<HashMap<String, serde_json::Value>>(&profile_file)?;

        return Ok(profile)
    }
    
    // fallback #1 try reading the working directory
    let profile_file = fs::read(format!("./{}", file_name)).with_context(|| format!("read data profile from current working directory: {}", file_name))?;
    let profile = serde_json::from_slice::<HashMap<String, serde_json::Value>>(&profile_file)?;

    Ok(profile)
}