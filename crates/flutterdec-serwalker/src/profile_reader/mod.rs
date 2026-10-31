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
    anyhow::ensure!(
        features.iter().any(|feature| feature == "product"),
        "Serwalker currently requires a product snapshot"
    );
    let (ops, cid_map, num_predefined_cids, class_kind_to_cid, class_tag_kind) =
        read_profile_file(&file_name)?;

    Ok(DataProfile {
        architecture: arch,
        features: snapshot_features,
        cid_map,
        class_kind_to_cid,
        num_predefined_cids,
        class_tag_kind,
        parsing_operations: ops,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::DART_3_11_1_SNAPSHOT_HASH;
    use crate::profile_reader::profile::ClassTagKind;
    use crate::profile_reader::profile_json_dictionary::{
        FOR_EACH, FUNCTION, READ_ALLOC, READ_FILL,
    };

    #[test]
    fn external_profile_exposes_cluster_first_recipes_and_tag_kind() {
        let profile = select_data_profile(
            &DART_3_11_1_SNAPSHOT_HASH.to_owned(),
            &vec![
                "product".into(),
                "arm64".into(),
                "compressed-pointers".into(),
            ],
        )
        .unwrap();
        assert_eq!(profile.num_predefined_cids, 175);
        assert_eq!(profile.class_tag_kind, ClassTagKind::ObjectHeader);
        assert!(profile.parsing_operations[FUNCTION][READ_ALLOC][FOR_EACH].is_array());
        assert!(profile.parsing_operations[FUNCTION][READ_FILL][FOR_EACH].is_array());
    }

    #[test]
    fn product_profile_rejects_non_product_features() {
        let error = select_data_profile(
            &DART_3_11_1_SNAPSHOT_HASH.to_owned(),
            &vec![
                "no-product".into(),
                "arm64".into(),
                "compressed-pointers".into(),
            ],
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("requires a product snapshot"));
    }

    #[test]
    fn tag_kind_names_are_explicit_and_unknown_kinds_are_rejected() {
        for (name, expected) in [
            (
                "packed_cid_and_canonical",
                ClassTagKind::PackedCidAndCanonical,
            ),
            ("object_header", ClassTagKind::ObjectHeader),
            (
                "object_header_deeply_immutable",
                ClassTagKind::ObjectHeaderDeeplyImmutable,
            ),
        ] {
            let actual: ClassTagKind = serde_json::from_value(serde_json::json!(name)).unwrap();
            assert_eq!(actual, expected);
        }
        assert!(serde_json::from_value::<ClassTagKind>(serde_json::json!("unknown")).is_err());
    }
}
