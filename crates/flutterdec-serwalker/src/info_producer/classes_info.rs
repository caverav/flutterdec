use flutterdec_adapter::model::{Class, ClassId, LibraryId, Provenance};

use crate::cluster::ClassCluster;
use crate::constants::ClassId as SerwalkerClassId;
use crate::info_producer::utils::find_object_by_id;
use crate::info_producer::utils::string_or_placeholder;
use crate::raw_object::{Library, Type};
use crate::snapshot::DataSnapshot;

pub fn produce_classes_info(snapshot: &DataSnapshot) -> anyhow::Result<Vec<Class>> {
    let mut classes = Vec::new();
    let class_cluster = snapshot
        .clusters
        .get(&((SerwalkerClassId::ClassCid as u32) << 2))
        .ok_or_else(|| anyhow::anyhow!("Class cluster is missing"))?
        .as_any()
        .downcast_ref::<ClassCluster>()
        .ok_or_else(|| anyhow::anyhow!("Cluster is not a ClassCluster"))?;

    for class in &class_cluster.objs {
        let cls_id = class.id as u32;
        let cls_name = string_or_placeholder(snapshot, class.name);

        let cls_super = match find_object_by_id::<Type>(snapshot, class.super_type) {
            Ok(cls_super_type) => {
                let cls_super_cid = cls_super_type.type_class_id();
                snapshot
                    .class_table
                    .get(&cls_super_cid)
                    .and_then(|index| class_cluster.objs.get(*index))
                    .map(|sc| ClassId(sc.id as u32))
            }
            Err(_) => None,
        };

        let cls_library = match find_object_by_id::<Library>(snapshot, class.library) {
            Ok(_) => Some(LibraryId(class.library)),
            Err(_) => None,
        };

        classes.push(Class {
            id: ClassId(cls_id),
            name: cls_name,
            library: cls_library,
            super_class: cls_super,
            provenance: Provenance::Exact,
        });
    }

    classes.sort_by_key(|c| c.id);
    Ok(classes)
}
