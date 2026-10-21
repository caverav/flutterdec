use crate::profile_reader::{
    profile::{DataProfile, SnapshotFeatures},
    utils::{
        architecture_suffix, features_suffix, read_profile_file, resolve_snapshot_architecture,
    },
};

pub(crate) mod profile;
pub(crate) mod profile_json_dictionary;
pub(crate) mod utils;

const PROF_EXTENSION: &str = ".json";
const SNAPSHOT_HASH_SUBSTR: usize = 8;

pub fn select_data_profile(
    snapshot_hash: &String,
    features: &Vec<String>,
) -> anyhow::Result<DataProfile> {
    let arch = resolve_snapshot_architecture(features)?;
    let snapshot_features = Vec::from_iter(
        features
            .iter()
            .filter_map(|feature_str| SnapshotFeatures::try_from(feature_str).ok()),
    );

    let file_name = "profile_".to_owned()
        + &(snapshot_hash.as_str()[..SNAPSHOT_HASH_SUBSTR])
        + "_"
        + &features_suffix(&snapshot_features)
        + "_"
        + &architecture_suffix(&arch)
        + PROF_EXTENSION;
    let (ops, cid_map, num_predefined_cids, class_kind_to_cid) = read_profile_file(&file_name)?;

    Ok(DataProfile {
        architecture: arch,
        features: snapshot_features,
        cid_map,
        class_kind_to_cid,
        num_predefined_cids,
        parsing_operations: ops,
    })
}
