use crate::cluster::LibraryCluster;
use crate::constants::ClassKind;
use crate::info_producer::utils::string_or_placeholder;
use crate::snapshot::DataSnapshot;
use flutterdec_adapter::LibraryInfo;

pub fn produce_libraries_info(snapshot: &DataSnapshot) -> anyhow::Result<Vec<LibraryInfo>> {
    let mut libraries_info = Vec::new();

    let library_cluster = snapshot.get_cluster::<LibraryCluster>(ClassKind::Library)?;

    let mut id = 1;
    for library in &library_cluster.objs {
        let library_uri = string_or_placeholder(snapshot, library.url);
        let library_name = string_or_placeholder(snapshot, library.name);

        libraries_info.push(LibraryInfo {
            id: id,
            uri: library_uri,
            name_display: library_name,
        });
        id += 1;
    }

    Ok(libraries_info)
}
