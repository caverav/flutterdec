use crate::stream::Stream;

#[derive(Default)]
pub struct InstructionTable
// in reality this is the representation of InstructionTable::Data of the C++ code
{
    canonical_stack_map_entries_offset: usize,
    length: usize,
    first_entry_with_code: usize,
    padding: usize,
    data: Vec<DataEntry>,
}

// in AOT mode, the instruction table is used to resolve the entry points
// for Function, Code, Closure, etc... Objects
struct DataEntry {
    pc_offset: usize,
    stack_map_offset: usize,
}

pub fn parse_instr_table_from_rodata(stream: &mut Stream) -> anyhow::Result<InstructionTable> {
    // ROData objects are wrapped inside OneByteString objects, so we need to read the syntetic fields first.
    let _tags = stream.read_raw_u64()?;
    let _data_byte_size = stream.read_raw_u64()?;
    // i wont do anything with them for now, for a normal snapshot, the tags should be a OneByteString cid
    // and data_byte_size should be the size of the contained InstructionsTable::Data
    // i.e 4 * sizeof(u32) + (2*sizeof(u32)) * length
    // corresponding to the four fields below + the size of each entry

    let mut instruction_table = InstructionTable {
        canonical_stack_map_entries_offset: stream.read_raw_u32()? as usize,
        length: stream.read_raw_u32()? as usize,
        first_entry_with_code: stream.read_raw_u32()? as usize,
        padding: stream.read_raw_u32()? as usize,
        data: Vec::default(),
    };

    // knowing what i explained in the comment above, we have that necessarily
    // 4 * sizeof(u32) + (2*sizeof(u32)) * length <= data_byte_size

    for _idx in 0..instruction_table.length {
        let entry = DataEntry {
            pc_offset: stream.read_raw_u32()? as usize,
            stack_map_offset: stream.read_raw_u32()? as usize,
        };

        instruction_table.data.push(entry);
    }

    Ok(instruction_table)
}

pub fn resolve_entry_points(
    code_index: u32,
    instr_table: &InstructionTable,
) -> anyhow::Result<usize> {
    let abs_index = instr_table.first_entry_with_code + code_index as usize;
    let entry = instr_table.data.get(abs_index).ok_or_else(|| {
        anyhow::anyhow!(
            "instruction-table index {abs_index} is out of bounds for length {}",
            instr_table.data.len()
        )
    })?;

    Ok(entry.pc_offset)
}
