use crate::raw_object::_String;
use crate::snapshot::DataSnapshot;
use crate::utils::SnapshotObject;

pub(super) fn find_object_by_id<T: SnapshotObject>(
    snapshot: &DataSnapshot,
    id: u32,
) -> anyhow::Result<&T> {
    let base_key = (T::CID as u32) << 2;

    for flags in 0..4u32 {
        let Some(cluster) = snapshot.clusters.get(&(base_key | flags)) else {
            continue;
        };

        let Some(object) = cluster.object_by_ref_id(id) else {
            continue;
        };

        return object.downcast_ref::<T>().ok_or_else(|| {
            anyhow::anyhow!("reference ID {id} has the wrong Rust type for {:?}", T::CID)
        });
    }

    anyhow::bail!(
        "no {} object found for reference ID {id}",
        std::any::type_name::<T>()
    )
}

pub(super) fn find_string_by_id(snapshot: &DataSnapshot, id: u32) -> anyhow::Result<String> {
    Ok(find_object_by_id::<_String>(snapshot, id)?
        .internal_str
        .clone())
}
