mod constants;
mod utils;

mod cluster;
mod instruction_table;
mod program_roots;

mod raw_object;
mod snapshot;

mod info_producer;
mod stream;

use flutterdec_adapter::{AdapterInput, ProgramModel};

use crate::{
    constants::DART_3_11_1_SNAPSHOT_HASH,
    info_producer::{produce_model_headers, produce_model_object_info, produce_model_object_pool},
    snapshot::parse_snapshot,
    stream::Stream,
};

pub fn walk_snapshot_and_produce_model(
    adapter_input: &AdapterInput,
) -> anyhow::Result<ProgramModel> {
    let mut program_model = ProgramModel {
        schema_version: 3,
        adapter_kind: "serwalker".to_owned(),
        dart_version: "3.11.1".to_owned(),
        snapshot_hash: DART_3_11_1_SNAPSHOT_HASH.to_owned(),
        arch: "unknown".to_owned(),
        libraries: Vec::new(),
        classes: Vec::new(),
        functions: Vec::new(),
        object_pool: Vec::new(),
    };

    let mut isolate_data_stream = Stream::new(adapter_input.isolate_data);
    let isolate_data_snapshot = parse_snapshot(&mut isolate_data_stream)?;

    produce_model_headers(&mut program_model, &isolate_data_snapshot)?;
    produce_model_object_info(&mut program_model, &isolate_data_snapshot)?;
    produce_model_object_pool(&mut program_model, &isolate_data_snapshot)?;

    Ok(program_model)
}
