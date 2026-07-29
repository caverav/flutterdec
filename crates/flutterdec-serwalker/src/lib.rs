mod constants;
mod utils;

mod cluster;
mod program_roots;
mod raw_object;
mod snapshot;

mod info_producer;
mod stream;

use flutterdec_adapter::{AdapterInput, ProgramModel};

use crate::{
    info_producer::{produce_model_headers, produce_model_object_info},
    snapshot::parse_snapshot,
    stream::Stream,
};

fn walk_snapshot_and_produce_model(
    isolate_data: &[u8],
    isolate_instr: &[u8],
    vm_data: Option<&[u8]>,
    vm_instr: Option<&[u8]>,
) -> ProgramModel {
    let program_model: ProgramModel;

    let mut isolate_data_stream = Stream::new(isolate_data);
    let isolate_data_snapshot = parse_snapshot(&mut isolate_data_stream)?;

    produce_model_headers(&mut program_model, &isolate_data_snapshot);
    produce_model_object_info(&mut program_model, &isolate_data_snapshot);
    produce_model_object_pool(&mut program_model, &isolate_data_snapshot);

    program_model
}
