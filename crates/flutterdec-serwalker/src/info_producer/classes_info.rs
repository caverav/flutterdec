use flutterdec_adapter::ClassInfo;

use crate::cluster::ClassCluster;
use crate::constants::ClassId;
use crate::info_producer::utils::find_object_by_id;
use crate::info_producer::utils::string_or_placeholder;
use crate::raw_object::{Library, Type};
use crate::snapshot::DataSnapshot;

pub fn produce_classes_info(snapshot: &DataSnapshot) -> anyhow::Result<Vec<ClassInfo>> {
    let mut classes_info = Vec::new();
    // obtain all class clusters (there should only be one)
    let class_cluster = snapshot
        .clusters
        .get(&((ClassId::ClassCid as u32) << 2))
        .ok_or_else(|| anyhow::anyhow!("Class cluster is missing"))?
        .as_any()
        .downcast_ref::<ClassCluster>()
        .ok_or_else(|| anyhow::anyhow!("Cluster is not a ClassCluster"))?; // should never happen.

    for class in &class_cluster.objs {
        let cls_id = class.id;
        let cls_name = string_or_placeholder(snapshot, class.name);

        let cls_super_name = match find_object_by_id::<Type>(snapshot, class.super_type) {
            Ok(cls_super) => {
                let cls_super_cid = cls_super.type_class_id();
                match snapshot
                    .class_table
                    .get(&cls_super_cid)
                    .and_then(|index| class_cluster.objs.get(*index))
                {
                    Some(cls_super_class) => string_or_placeholder(snapshot, cls_super_class.name),
                    None => format!("<unresolved Class CID {cls_super_cid}>"),
                }
            }
            Err(_) => format!("<unresolved Type reference ID {}>", class.super_type),
        };

        let cls_library_uri = match find_object_by_id::<Library>(snapshot, class.library) {
            Ok(cls_library) => string_or_placeholder(snapshot, cls_library.url),
            Err(_) => format!("<unresolved Library reference ID {}>", class.library),
        };

        let class_info = ClassInfo {
            id: cls_id as u64,
            name: cls_name,
            super_name: cls_super_name,
            library_uri: cls_library_uri,
        };

        classes_info.push(class_info);
    }

    Ok(classes_info)
}
