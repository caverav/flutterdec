mod classes_info;
mod functions_info;
mod libraries_info;
mod object_pool_info;
pub mod utils;

use classes_info::produce_classes_info;
use flutterdec_adapter::model::{Class, Function, Library, ObjectPool};
use functions_info::produce_functions_info;
use libraries_info::produce_libraries_info;
use object_pool_info::produce_model_object_pool_info;

use crate::info_producer::utils::find_object_by_id;
use crate::raw_object::ObjectPool as SerwalkerObjectPool;
use crate::snapshot::{DataSnapshot, InstructionsSnapshot};

pub fn produce_model_object_info(
    snapshot: &DataSnapshot,
    instructions: &InstructionsSnapshot,
) -> anyhow::Result<(Vec<Library>, Vec<Class>, Vec<Function>)> {
    let classes = produce_classes_info(snapshot)?;
    let functions = produce_functions_info(snapshot, instructions)?;
    let libraries = produce_libraries_info(snapshot)?;
    Ok((libraries, classes, functions))
}

pub fn produce_model_object_pool(snapshot: &DataSnapshot) -> anyhow::Result<ObjectPool> {
    let global_object_pool_ref_id = snapshot.roots.object_store().global_object_pool();
    let global_object_pool =
        find_object_by_id::<SerwalkerObjectPool>(snapshot, global_object_pool_ref_id)?;
    produce_model_object_pool_info(snapshot, global_object_pool)
}
