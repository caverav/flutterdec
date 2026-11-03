use crate::cluster::LibraryCluster;
use crate::constants::ClassKind;
use crate::info_producer::utils::{find_string_by_id, string_or_placeholder};
use crate::snapshot::DataSnapshot;
use flutterdec_adapter::model::{Library, LibraryId, ProgramModel, Provenance};
use serde_json::json;

pub fn produce_libraries_info(
    snapshot: &DataSnapshot,
    model: &mut ProgramModel,
) -> anyhow::Result<()> {
    let mut libraries_info = Vec::new();

    let library_cluster = snapshot.get_cluster::<LibraryCluster>(ClassKind::Library)?;

    for (index, library) in library_cluster.objs.iter().enumerate() {
        let reference_id = library_cluster
            .first_ref_id
            .checked_add(u32::try_from(index)?)
            .ok_or_else(|| anyhow::anyhow!("Library reference ID overflow"))?;
        let library_uri = match find_string_by_id(snapshot, library.url) {
            Ok(uri) if !uri.trim().is_empty() => uri,
            _ => {
                model.extensions.insert(
                    format!("serwalker.library.{reference_id}"),
                    json!({"reference_id": reference_id, "uri_ref_id": library.url,
                           "name_ref_id": library.name,
                           "unresolved_uri": string_or_placeholder(snapshot, library.url)}),
                );
                continue;
            }
        };
        let library_name = find_string_by_id(snapshot, library.name)
            .ok()
            .filter(|name| !name.trim().is_empty());

        libraries_info.push(Library {
            id: LibraryId(reference_id),
            uri: library_uri,
            display_name: library_name,
            provenance: Provenance::Exact,
        });
    }

    model.libraries = libraries_info;
    Ok(())
}
