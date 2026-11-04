use crate::cluster::{CodeCluster, FunctionCluster};
use crate::constants::ClassKind;
use crate::info_producer::utils::{find_object_by_id, find_string_by_id};
use crate::raw_object::{Class, Code};
use crate::snapshot::{DataSnapshot, InstructionsSnapshot};
use flutterdec_adapter::model::{
    ClassId, CodeRange, Function, FunctionId, Name, ProgramModel, Provenance,
};
use serde_json::json;

pub fn produce_functions_info(
    snapshot: &DataSnapshot,
    instructions: &InstructionsSnapshot,
    model: &mut ProgramModel,
) -> anyhow::Result<()> {
    let mut functions_info = Vec::new();

    let function_cluster = snapshot.get_cluster::<FunctionCluster>(ClassKind::Function)?;
    let code_cluster = snapshot.get_cluster::<CodeCluster>(ClassKind::Code)?;

    let first_entry_with_code = snapshot.instruction_table.first_entry_with_code();
    let instruction_table_len = snapshot.instruction_table.len();
    for (index, function) in function_cluster.objs.iter().enumerate() {
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

        let id = function_cluster
            .first_ref_id
            .checked_add(u32::try_from(index)?)
            .ok_or_else(|| anyhow::anyhow!("Function reference ID overflow"))?;
        let func_name = find_string_by_id(snapshot, function.name)
            .ok()
            .filter(|name| !name.trim().is_empty())
            .map(Name::exact);
        let func_owner_class = find_object_by_id::<Class>(snapshot, function.owner)
            .ok()
            .and_then(|class| u32::try_from(class.id).ok())
            .map(ClassId)
            .filter(|id| model.class(*id).is_some());
        if func_name.is_none() || func_owner_class.is_none() {
            model.extensions.insert(
                format!("serwalker.function.{id}"),
                json!({"name_ref_id": function.name, "owner_ref_id": function.owner,
                       "code_index": function.code_index}),
            );
        }

        let entry_va = instructions
            .image_va
            .checked_add(entry_offset)
            .ok_or_else(|| anyhow::anyhow!("Function entry virtual address overflow"))?;

        /* for code objects we can return three ranges:
            let P = payload_start = instruction_table(first_entry_with_code + code_cluster_index).pc_offset
            S = length of the instructions in bytes and E = end of instructions
            1. SwitchableCallMiss entry: [P, E = P + S]
            2. Monomorphic entry: [P + 8, E = P + S]
            3. Polymorphic entry: [P + 24, E = P + S]

            I return the first one, giving us the a view of the WHOLE function code.
        */
        let function_info = Function {
            id: FunctionId(id),
            name: func_name,
            owner: func_owner_class,
            code: CodeRange {
                start_va: entry_va,
                size: instructions_length,
            },
            code_section_va: instructions.image_va,
            provenance: Provenance::Derived,
        };
        functions_info.push(function_info);
    }

    model.functions = functions_info;
    Ok(())
}
