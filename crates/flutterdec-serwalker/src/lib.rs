mod constants;
mod utils;

mod cluster;
mod object_store;
mod raw_object;
mod snapshot;

mod info_producer;
mod stream;

use flutterdec_adapter::ProgramModel;

use crate::{
    info_producer::{enrich_model_headers, enrich_model_object_info},
    snapshot::parse_snapshot,
    stream::Stream,
};

fn walk_snapshot_and_enrich_model(
    isolate_data: &[u8],
    isolate_instr: &[u8],
    vm_data: Option<&[u8]>,
    vm_instr: Option<&[u8]>,
) -> ProgramModel {
    let program_model: ProgramModel;

    let mut isolate_data_stream = Stream::new(isolate_data);
    let isolate_data_snapshot = parse_snapshot(&mut isolate_data_stream)?;

    enrich_model_headers(&mut program_model, &isolate_data_snapshot);
    enrich_model_object_info(&mut program_model, &isolate_data_snapshot);
    enrich_model_object_pool(&mut program_model, &isolate_data_snapshot);

    program_model
}
