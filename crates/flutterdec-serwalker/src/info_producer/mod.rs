mod classes_info;
mod functions_info;
mod libraries_info;
mod object_pool_info;
mod utils;

use anyhow::Error;
use classes_info::produce_classes_info;
use flutterdec_adapter::ProgramModel;
use functions_info::produce_functions_info;
use libraries_info::produce_libraries_info;

use crate::{info_producer::{object_pool_info::produce_model_object_pool_info, utils::find_object_by_id}, raw_object::ObjectPool, snapshot::{DataSnapshot, InstructionsSnapshot}};

pub fn produce_model_headers(
    model: &mut ProgramModel,
    snapshot: &DataSnapshot,
) -> anyhow::Result<()> {
    Ok(())
}

pub fn produce_model_object_info(
    model: &mut ProgramModel,
    snapshot: &DataSnapshot,
    instructions: &InstructionsSnapshot,
) -> anyhow::Result<()> {
    model.classes = produce_classes_info(snapshot)?;
    model.functions = produce_functions_info(snapshot, instructions)?;
    model.libraries = produce_libraries_info(snapshot)?;

    Ok(())
}

pub fn produce_model_object_pool(
    model: &mut ProgramModel,
    snapshot: &DataSnapshot,
) -> anyhow::Result<()> {

    let global_object_pool_ref_id =  snapshot.roots.object_store().global_object_pool();
    let global_object_pool = find_object_by_id::<ObjectPool>(snapshot, global_object_pool_ref_id)?;

    model.object_pool = produce_model_object_pool_info(snapshot, global_object_pool)?;
    Ok(())
}
