pub mod structs;

pub fn parse_object_store() -> anyhow::Result<ObjectStore> {
    let object_store: ObjectStore;

    Ok(object_store)
}

pub fn parse_field_table() -> anyhow::Result<()> {
    Ok(())
}

pub fn parse_dispatch_table() -> anyhow::Result<()> {
    Ok(())
}

use flutterdec_adapter::LibraryInfo;

use crate::snapshot::DataSnapshot;

pub fn resolve_root_library(
    root_lib_ref: u32,
    data_snapshot: &DataSnapshot,
) -> anyhow::Result<LibraryInfo> {
    let root_lib: LibraryInfo;

    Ok(root_lib)
}
