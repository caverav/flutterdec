use flutterdec_adapter::ObjectPoolEntry;

use crate::cluster::ClassCluster;
use crate::constants::ClassId;
use crate::info_producer::utils::string_or_placeholder;
use crate::raw_object::{ObjectPool, ObjectPoolEntryValue};
use crate::snapshot::DataSnapshot;

fn tagged_object_kind(snapshot: &DataSnapshot, reference_id: u32) -> anyhow::Result<String> {
    let Some(cluster) = snapshot.cluster_by_ref_id(reference_id)? else {
        return Ok(format!("<unresolved reference ID {reference_id}>"));
    };

    let cid = i32::try_from(cluster.cid())
        .map_err(|_| anyhow::anyhow!("cluster CID {} does not fit in i32", cluster.cid()))?;
    let Some(class_index) = snapshot.class_table.get(&cid) else {
        return Ok(format!("<unresolved Class CID {cid}>"));
    };

    let class_cluster = snapshot
        .clusters
        .get(&((ClassId::ClassCid as u32) << 2))
        .ok_or_else(|| anyhow::anyhow!("Class cluster is missing"))?
        .as_any()
        .downcast_ref::<ClassCluster>()
        .ok_or_else(|| anyhow::anyhow!("Cluster is not a ClassCluster"))?;
    let class = class_cluster.objs.get(*class_index).ok_or_else(|| {
        anyhow::anyhow!("class table index {class_index} for CID {cid} is out of bounds")
    })?;

    Ok(string_or_placeholder(snapshot, class.name))
}

pub fn produce_model_object_pool_info(
    snapshot: &DataSnapshot,
    global_object_pool: &ObjectPool,
) -> anyhow::Result<Vec<ObjectPoolEntry>> {
    let mut object_pool_info = Vec::with_capacity(global_object_pool.length);

    for (index, entry) in global_object_pool.entries.iter().enumerate() {
        let kind = match &entry.value {
            ObjectPoolEntryValue::TaggedObjectRef(reference_id) => {
                tagged_object_kind(snapshot, *reference_id)?
            }
            ObjectPoolEntryValue::Immediate(_imm) => String::from("Immediate"),
            _ => String::from("Non-immediate, non-object entry")
        };

        let ref_id = if let ObjectPoolEntryValue::TaggedObjectRef(ref_id) = &entry.value {
            *ref_id
        } else {
            0
        };
        
        let value = if ref_id == 0 
        {
            if let ObjectPoolEntryValue::Immediate(imm) = &entry.value {
                imm.to_string() 
            } else {
                String::from("Non-immediate or tagged object in this entry")
            }
        } else { // the internal names for the One and Two Byte classes are prefixed with an "_"
            if kind == "String" || kind == "_OneByteString" || kind == "_TwoByteString" {
                string_or_placeholder(snapshot, ref_id)
            } else {
                 kind.clone() + "_" + &(ref_id.to_string())
            }
        };

        object_pool_info.push(ObjectPoolEntry {
            index: u64::try_from(index)
                .map_err(|_| anyhow::anyhow!("ObjectPool index does not fit in u64"))?,
            kind: kind,
            value: value,
            decoded_kind: None,
            selector: None,
            target_va: None,
            owner_class: None,
            library_uri: None,
            confidence: Some(1.0), // can it be anything else, when using this technique?
            source: None,
        });
    }

    Ok(object_pool_info)
}
