use flutterdec_adapter::FunctionInfo;
use crate::cluster::FunctionCluster;
use crate::info_producer::utils::{find_object_by_id, string_or_placeholder};
use crate::raw_object::{Class, Code};
use crate::snapshot::{DataSnapshot, InstructionsSnapshot};
use crate::constants::ClassId;

pub fn produce_functions_info(snapshot: &DataSnapshot, instructions: &InstructionsSnapshot) -> anyhow::Result<Vec<FunctionInfo>> {
    let mut functions_info = Vec::new();

    let function_cluster = snapshot.clusters.
    get(&((ClassId::FunctionCid as u32) << 2))
    .ok_or_else(|| { anyhow::anyhow!("Function cluster not found") })?
    .as_any()
    .downcast_ref::<FunctionCluster>()
    .ok_or_else(|| anyhow::anyhow!("Function cluster is not of type FunctionCluster"))?;

    let mut id: u64 = 1;

    for function in &function_cluster.objs
    {
        let func_name = string_or_placeholder(snapshot, function.name);
        let func_owner_class = match find_object_by_id::<Class>(snapshot, function.owner) {
            Ok(owning_class) => string_or_placeholder(snapshot, owning_class.name),
            Err(_) => format!("<Unresolved Class reference ID {} or the Function is not owned by a class>", function.owner) 
        };

        // first_entry_with_code returns an usize, which is machine-word size, casting it to u32 could slice it
        let cluster_index = function.code_index as usize - snapshot.instruction_table.first_entry_with_code() - 1;
        let instructions_length = match find_object_by_id::<Code>(snapshot, cluster_index as u32) {
            Ok(code) => code.instructions_length_,
            Err(_) => 0 // this is impossible in normal circumstances so 0 works as an error value
        };

        let function_info = FunctionInfo { 
            id: id, 
            name: func_name,
            owner_class: func_owner_class, 
            entry_va: function.entry_point, 
            size: instructions_length, 
            code_section_va: instructions.image_va, 
            name_kind: Some(String::from("exact"))
        };
        functions_info.push(function_info);
        id += 1;
    }

    Ok(functions_info)
}
