use crate::cluster::{CodeCluster, FunctionCluster};
use crate::constants::ClassId as SerwalkerClassId;
use crate::info_producer::utils::{find_object_by_id, string_or_placeholder};
use crate::raw_object::{Class, Code};
use crate::snapshot::{DataSnapshot, InstructionsSnapshot};
use flutterdec_adapter::model::{ClassId, CodeRange, Function, FunctionId, Name, Provenance};

pub fn produce_functions_info(
    snapshot: &DataSnapshot,
    instructions: &InstructionsSnapshot,
) -> anyhow::Result<Vec<Function>> {
    let mut functions = Vec::new();

    let function_cluster = snapshot
        .clusters
        .get(&((SerwalkerClassId::FunctionCid as u32) << 2))
        .ok_or_else(|| anyhow::anyhow!("Function cluster not found"))?
        .as_any()
        .downcast_ref::<FunctionCluster>()
        .ok_or_else(|| anyhow::anyhow!("Function cluster is not of type FunctionCluster"))?;

    let code_cluster = snapshot
        .clusters
        .get(&((SerwalkerClassId::CodeCid as u32) << 2))
        .ok_or_else(|| anyhow::anyhow!("Code cluster not found"))?
        .as_any()
        .downcast_ref::<CodeCluster>()
        .ok_or_else(|| anyhow::anyhow!("Code cluster is not of type CodeCluster"))?;

    let first_entry_with_code = snapshot.instruction_table.first_entry_with_code();
    let instruction_table_len = snapshot.instruction_table.len();
    let mut id = 0u32;

    for function in &function_cluster.objs {
        if function.code_index == 0 {
            continue;
        }

        let table_index = usize::try_from(function.code_index - 1)
            .map_err(|_| anyhow::anyhow!("Function code index does not fit in usize"))?;

        // discarded code case
        let (entry_offset, instructions_length) = if table_index < first_entry_with_code {
            let entry_offset = snapshot.instruction_table.pc_offset_at(table_index)?;
            anyhow::ensure!(
                function.entry_point == entry_offset,
                "discarded Function code index {} has entry offset {}, expected {}",
                function.code_index,
                function.entry_point,
                entry_offset
            );

            let next_table_index = table_index
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("instruction-table index overflow"))?;
            let next_entry_offset = snapshot.instruction_table.pc_offset_at(next_table_index)?;
            let instructions_length = next_entry_offset
                .checked_sub(entry_offset)
                .ok_or_else(|| anyhow::anyhow!("instruction-table entries are not ordered"))?;

            (entry_offset, instructions_length)
        } else {
            let cluster_index = table_index - first_entry_with_code;
            let cluster_index = u32::try_from(cluster_index)
                .map_err(|_| anyhow::anyhow!("Code cluster index does not fit in u32"))?;
            let code_ref_id = code_cluster
                .first_ref_id
                .checked_add(cluster_index)
                .ok_or_else(|| anyhow::anyhow!("Code reference ID overflow"))?;
            let code = find_object_by_id::<Code>(snapshot, code_ref_id)?;

            // deferred code case
            if table_index >= instruction_table_len {
                continue;
            }

            anyhow::ensure!(
                code.entry_point != u64::MAX,
                "Code object for Function code index {} has an unresolved entry point",
                function.code_index
            );

            let payload_start = snapshot.instruction_table.pc_offset_at(table_index)?;
            (payload_start, code.instructions_length_)
        };

        let func_name_str = string_or_placeholder(snapshot, function.name);
        let func_name = if func_name_str.is_empty() || func_name_str.starts_with('<') {
            None
        } else {
            Some(Name::exact(func_name_str))
        };

        let func_owner = match find_object_by_id::<Class>(snapshot, function.owner) {
            Ok(owning_class) => Some(ClassId(owning_class.id as u32)),
            Err(_) => None,
        };

        let entry_va = instructions
            .image_va
            .checked_add(entry_offset)
            .ok_or_else(|| anyhow::anyhow!("Function entry virtual address overflow"))?;

        let function_item = Function {
            id: FunctionId(id),
            name: func_name,
            owner: func_owner,
            code: CodeRange {
                start_va: entry_va,
                size: instructions_length,
            },
            code_section_va: instructions.image_va,
            provenance: Provenance::Exact,
        };
        functions.push(function_item);
        id += 1;
    }

    functions.sort_by_key(|f| f.id);
    Ok(functions)
}
