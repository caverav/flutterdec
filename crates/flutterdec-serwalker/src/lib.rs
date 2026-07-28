mod constants;
mod utils;

mod cluster;
mod object_store;
mod raw_object;
mod snapshot;

mod stream;

use flutterdec_adapter::ProgramModel;

fn walk_and_enrich(data_snapshot: &[u8], instruction_snapshot: &[u8]) -> ProgramModel {
    let program_model: ProgramModel;

    program_model
}
