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

use crate::snapshot::{DataSnapshot, InstructionsSnapshot};

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
    Ok(())
}
