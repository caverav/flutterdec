use flutterdec_adapter::model::{
    ObjectPool, PoolEntry, PoolEntryKind, PoolGeometry, PoolIndexSpace, Provenance,
};

use crate::cluster::ClassCluster;
use crate::constants::ClassId;
use crate::info_producer::utils::string_or_placeholder;
use crate::raw_object::{ObjectPool as SerwalkerObjectPool, ObjectPoolEntryValue};
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
    global_object_pool: &SerwalkerObjectPool,
) -> anyhow::Result<ObjectPool> {
    let mut entries = Vec::with_capacity(global_object_pool.length);

    for (index, entry) in global_object_pool.entries.iter().enumerate() {
        let (kind_str, pool_kind) = match &entry.value {
            ObjectPoolEntryValue::TaggedObjectRef(reference_id) => {
                let k = tagged_object_kind(snapshot, *reference_id)?;
                let pk = if k == "String" || k == "_OneByteString" || k == "_TwoByteString" {
                    PoolEntryKind::String
                } else if k == "Class" {
                    PoolEntryKind::Class
                } else if k == "Function" || k == "Code" {
                    PoolEntryKind::Code
                } else if k == "Field" {
                    PoolEntryKind::Field
                } else {
                    PoolEntryKind::Undecoded
                };
                (k, pk)
            }
            ObjectPoolEntryValue::Immediate(_imm) => {
                (String::from("Immediate"), PoolEntryKind::Immediate)
            }
            _ => (
                String::from("Non-immediate, non-object entry"),
                PoolEntryKind::Undecoded,
            ),
        };

        let ref_id = if let ObjectPoolEntryValue::TaggedObjectRef(ref_id) = &entry.value {
            *ref_id
        } else {
            0
        };

        let value = if ref_id == 0 {
            if let ObjectPoolEntryValue::Immediate(imm) = &entry.value {
                Some(imm.to_string())
            } else {
                None
            }
        } else if kind_str == "String"
            || kind_str == "_OneByteString"
            || kind_str == "_TwoByteString"
        {
            let s = string_or_placeholder(snapshot, ref_id);
            if s.is_empty() || s.starts_with('<') {
                None
            } else {
                Some(s)
            }
        } else {
            let s = format!("{kind_str}_{ref_id}");
            Some(s)
        };

        entries.push(PoolEntry {
            index: u64::try_from(index)
                .map_err(|_| anyhow::anyhow!("ObjectPool index does not fit in u64"))?,
            kind: pool_kind,
            value,
            target_va: None,
            provenance: Provenance::Exact,
            confidence: None,
        });
    }

    Ok(ObjectPool {
        index_space: PoolIndexSpace::Hardware,
        geometry: Some(PoolGeometry {
            entries_offset: 16,
            word_size: 8,
        }),
        entries,
    })
}
