mod classes_info;
mod functions_info;
mod libraries_info;
mod object_pool_info;
mod utils;

use anyhow::Error;
use flutterdec_adapter::ProgramModel;

use crate::snapshot::DataSnapshot;

pub fn produce_model_headers(
    model: &mut ProgramModel,
    snapshot: &DataSnapshot,
) -> anyhow::Result<()> {
    Ok(())
}

pub fn produce_model_object_info(
    model: &mut ProgramModel,
    snapshot: &DataSnapshot,
) -> anyhow::Result<()> {
    Ok(())
}

pub fn produce_model_object_pool(
    model: &mut ProgramModel,
    snapshot: &DataSnapshot,
) -> anyhow::Result<()> {
    Ok(())
}
