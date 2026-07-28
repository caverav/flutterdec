mod constants;
mod utils;

mod cluster;
mod object_store;
mod raw_object;
mod snapshot;

mod info_producer;
mod stream;

use flutterdec_adapter::ProgramModel;

#[allow(dead_code)]
fn walk_snapshot_and_enrich_model(
    _isolate_data: &[u8],
    _isolate_instr: &[u8],
    _vm_data: Option<&[u8]>,
    _vm_instr: Option<&[u8]>,
) -> anyhow::Result<ProgramModel> {
    todo!("wire parse_snapshot into the info_producer passes")
}
