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
    snapshot_hash: &str,
    features: &[String],
) -> anyhow::Result<DataProfile> {
    let file_name = profile_file_name(snapshot_hash, features)?;
    let arch = resolve_snapshot_architecture(features)?;
    let snapshot_features = features
        .iter()
        .filter_map(|feature| SnapshotFeatures::try_from(feature).ok())
        .collect();
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

pub(crate) fn profile_file_name(
    snapshot_hash: &str,
    features: &[String],
) -> anyhow::Result<String> {
    anyhow::ensure!(
        snapshot_hash.len() == 32 && snapshot_hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "snapshot hash must contain 32 hexadecimal characters"
    );
    let arch = resolve_snapshot_architecture(features)?;
    let snapshot_features = features
        .iter()
        .filter_map(|feature| SnapshotFeatures::try_from(feature).ok())
        .collect::<Vec<_>>();
    Ok(format!(
        "profile_{}_{}_{}{}",
        &snapshot_hash[..SNAPSHOT_HASH_SUBSTR],
        features_suffix(&snapshot_features),
        architecture_suffix(&arch),
        PROF_EXTENSION
    ))
}

pub(crate) fn data_profile_from_bytes(
    bytes: &[u8],
    features: &[String],
) -> anyhow::Result<DataProfile> {
    anyhow::ensure!(
        features.iter().any(|feature| feature == "product"),
        "Serwalker currently requires a product snapshot"
    );
    let (ops, cid_map, num_predefined_cids, class_kind_to_cid, class_tag_kind) =
        utils::read_profile_bytes(bytes)?;
    Ok(DataProfile {
        architecture: resolve_snapshot_architecture(features)?,
        features: features
            .iter()
            .filter_map(|feature| SnapshotFeatures::try_from(feature).ok())
            .collect(),
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
    fn profile_file_names_use_snapshot_hash_and_features() {
        let features = vec![
            "product".into(),
            "arm64".into(),
            "compressed-pointers".into(),
        ];
        assert_eq!(
            profile_file_name("80a49c7111088100a233b2ae788e1f48", &features).unwrap(),
            "profile_80a49c71_compr_arm64.json"
        );
        assert_eq!(
            profile_file_name(DART_3_11_1_SNAPSHOT_HASH, &features).unwrap(),
            "profile_78da37fe_compr_arm64.json"
        );
        for hash in ["", "1234567", "not-a-hash-but-exactly-32-letters!"] {
            assert!(profile_file_name(hash, &features).is_err());
        }
    }

    #[test]
    fn external_profile_exposes_cluster_first_recipes_and_tag_kind() {
        let profile = select_data_profile(
            DART_3_11_1_SNAPSHOT_HASH,
            &[
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
            DART_3_11_1_SNAPSHOT_HASH,
            &[
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
