use flutterdec_adapter::model::{Class, ClassId, LibraryId, ProgramModel, Provenance};
use serde_json::json;
use std::collections::{BTreeSet, HashSet};

use crate::cluster::ClassCluster;
use crate::constants::ClassKind;
use crate::info_producer::utils::find_object_by_id;
use crate::info_producer::utils::find_string_by_id;
use crate::raw_object::Type;
use crate::snapshot::DataSnapshot;

pub fn produce_classes_info(
    snapshot: &DataSnapshot,
    model: &mut ProgramModel,
) -> anyhow::Result<()> {
    let mut classes_info = Vec::new();
    // obtain the class cluster (there should only be one, the non-canonical, non-immutable one)
    let class_cluster = snapshot.get_cluster::<ClassCluster>(ClassKind::Class)?;
    let known_libraries: HashSet<LibraryId> =
        model.libraries.iter().map(|library| library.id).collect();

    for class in &class_cluster.objs {
        let cls_id = ClassId(u32::try_from(class.id)?);
        let cls_name = match find_string_by_id(snapshot, class.name) {
            Ok(name) if !name.trim().is_empty() => name,
            _ => {
                model.extensions.insert(
                    format!("serwalker.class.{}", cls_id.0),
                    json!({"cid": class.id, "name_ref_id": class.name,
                           "library_ref_id": class.library, "super_type_ref_id": class.super_type}),
                );
                continue;
            }
        };
        let class_info = Class {
            id: cls_id,
            name: cls_name,
            super_class: None,
            library: {
                let library_id = LibraryId(class.library);
                known_libraries.contains(&library_id).then_some(library_id)
            },
            provenance: Provenance::Exact,
        };

        classes_info.push(class_info);
    }

    classes_info.sort_by_key(|class| class.id);
    let class_ids: BTreeSet<_> = classes_info.iter().map(|class| class.id).collect();
    for class_info in &mut classes_info {
        let raw_class = snapshot
            .class_table
            .get(&(class_info.id.0 as i32))
            .and_then(|index| class_cluster.objs.get(*index))
            .ok_or_else(|| anyhow::anyhow!("class table is missing CID {}", class_info.id))?;
        let super_cid = find_object_by_id::<Type>(snapshot, raw_class.super_type)
            .ok()
            .map(|super_type| super_type.type_class_id());
        class_info.super_class = super_cid
            .and_then(|cid| u32::try_from(cid).ok())
            .map(ClassId)
            .filter(|id| class_ids.contains(id));
        if class_info.library.is_none() || class_info.super_class.is_none() {
            model.extensions.insert(
                format!("serwalker.class.{}", class_info.id.0),
                json!({"cid": raw_class.id, "name_ref_id": raw_class.name,
                       "library_ref_id": raw_class.library, "super_type_ref_id": raw_class.super_type,
                       "super_cid": super_cid}),
            );
        }
    }
    model.classes = classes_info;
    Ok(())
}
