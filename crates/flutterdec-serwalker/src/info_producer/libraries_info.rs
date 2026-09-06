use flutterdec_adapter::model::{Library, LibraryId, Provenance};

use crate::cluster::LibraryCluster;
use crate::constants::ClassId;
use crate::info_producer::utils::string_or_placeholder;
use crate::snapshot::DataSnapshot;

pub fn produce_libraries_info(snapshot: &DataSnapshot) -> anyhow::Result<Vec<Library>> {
    let mut libraries = Vec::new();

    let library_cluster = snapshot
        .clusters
        .get(&((ClassId::LibraryCid as u32) << 2))
        .ok_or_else(|| anyhow::anyhow!("Library cluster not found"))?
        .as_any()
        .downcast_ref::<LibraryCluster>()
        .ok_or_else(|| anyhow::anyhow!("Library cluster is not of type LibraryCluster"))?;

    for (idx, library) in library_cluster.objs.iter().enumerate() {
        let library_ref_id = library_cluster.first_ref_id + idx as u32;
        let library_uri = string_or_placeholder(snapshot, library.url);
        let library_name = string_or_placeholder(snapshot, library.name);

        let display_name = if library_name.is_empty() || library_name == library_uri {
            None
        } else {
            Some(library_name)
        };

        libraries.push(Library {
            id: LibraryId(library_ref_id),
            uri: library_uri,
            display_name,
            provenance: Provenance::Exact,
        });
    }

    libraries.sort_by_key(|l| l.id);
    Ok(libraries)
}
