mod constants;
mod utils;

mod cluster;
mod instruction_table;
mod program_roots;

mod raw_object;
mod snapshot;

mod info_producer;
mod stream;

use flutterdec_adapter::{AdapterInput, PoolGeometry, ProgramModel};

use crate::{
    constants::DART_3_11_1_SNAPSHOT_HASH,
    info_producer::{produce_model_headers, produce_model_object_info, produce_model_object_pool},
    snapshot::{parse_data_snapshot, parse_instr_snapshot},
    stream::Stream,
};

pub const SUPPORTED_SNAPSHOT_HASH: &str = DART_3_11_1_SNAPSHOT_HASH;

pub fn walk_snapshot_and_produce_model(
    adapter_input: &AdapterInput,
) -> anyhow::Result<ProgramModel> {
    const ENTRIES_OFFSET: u64 = 16;
    const WORD_SIZE: u64 = 8; // size in bytes of ObjectPoolEntry objects

    let mut program_model = ProgramModel {
        schema_version: 3,
        adapter_kind: "serwalker".to_owned(),
        dart_version: "3.11.1".to_owned(),
        snapshot_hash: DART_3_11_1_SNAPSHOT_HASH.to_owned(),
        arch: "ARM64".to_owned(),
        libraries: Vec::new(),
        classes: Vec::new(),
        functions: Vec::new(),
        object_pool: Vec::new(),
        pool_geometry: Some(PoolGeometry {
            entries_offset: ENTRIES_OFFSET,
            word_size: WORD_SIZE,
        }),
    };

    let mut isolate_data_stream = Stream::new(adapter_input.isolate_data);
    let mut isolate_instr_stream = Stream::new(adapter_input.isolate_instr);

    let mut isolate_data_snapshot = parse_data_snapshot(&mut isolate_data_stream)?;
    let mut isolate_instr_snapshot = parse_instr_snapshot(
        &mut isolate_instr_stream,
        &mut isolate_data_snapshot.clusters,
    )?;
    // parse_instr_snapshot needs to run after parse_data_snapshot, given resolve_entrypoints must have been executed already
    // in order to call resolve_instructions_len_for_code_objects

    // manually set it before passing the instructions produce_model_object_info
    // the core already determined it for us
    isolate_instr_snapshot.image_va = adapter_input.isolate_instr_va;

    produce_model_headers(&mut program_model, &isolate_data_snapshot)?;
    produce_model_object_info(
        &mut program_model,
        &isolate_data_snapshot,
        &isolate_instr_snapshot,
    )?;
    produce_model_object_pool(&mut program_model, &isolate_data_snapshot)?;

    Ok(program_model)
}
