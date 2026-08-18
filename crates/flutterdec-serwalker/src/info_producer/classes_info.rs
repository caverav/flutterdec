use flutterdec_adapter::ClassInfo;

use crate::cluster::ClassCluster;
use crate::constants::ClassId;
use crate::info_producer::utils::{find_object_by_id, find_string_by_id};
use crate::raw_object::{Library, Type};
use crate::snapshot::DataSnapshot;

pub fn produce_class_info(snapshot: &DataSnapshot) -> anyhow::Result<Vec<ClassInfo>> {
    let class_info = Vec::new();
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
        let cls_name = find_string_by_id(snapshot, class.name)?;

        let cls_super: &Type = find_object_by_id(snapshot, class.super_type)?;

        let cls_library: &Library = find_object_by_id(snapshot, class.library)?;
        let cls_library_name = find_string_by_id(snapshot, cls_library.name)?;
    }

    Ok(class_info)
}
