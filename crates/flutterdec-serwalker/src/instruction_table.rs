use crate::stream::Stream;

#[derive(Default)]
pub struct InstructionTable
// in reality this is the representation of InstructionTable::Data of the C++ code
{
    #[allow(dead_code)]
    canonical_stack_map_entries_offset: usize,
    length: usize,
    first_entry_with_code: usize,
    #[allow(dead_code)]
    padding: usize,
    data: Vec<DataEntry>,
}

// in AOT mode, the instruction table is used to resolve the entry points
// for Function, Code, Closure, etc... Objects
struct DataEntry {
    pc_offset: usize,
    #[allow(dead_code)]
    stack_map_offset: usize,
}

impl InstructionTable {
    pub(crate) fn len(&self) -> usize {
        self.data.len()
    }

    pub(crate) fn first_entry_with_code(&self) -> usize {
        self.first_entry_with_code
    }

    pub(crate) fn pc_offset_at(&self, index: usize) -> anyhow::Result<u64> {
        let entry = self.data.get(index).ok_or_else(|| {
            anyhow::anyhow!(
                "instruction-table index {index} is out of bounds for length {}",
                self.data.len()
            )
        })?;

        Ok(entry.pc_offset as u64)
    }
}

pub fn parse_instr_table_from_rodata(stream: &mut Stream) -> anyhow::Result<InstructionTable> {
    // ROData objects are wrapped inside OneByteString objects, so we need to read the syntetic fields first.
    let _tags = stream.read_raw_u64()?;
    let data_byte_size = usize::try_from(stream.read_raw_u64()?)
        .map_err(|_| anyhow::anyhow!("instruction-table data size does not fit in usize"))?;
    // The tags are a OneByteString cid for a normal snapshot. The data size is
    // the size of the contained InstructionsTable::Data: a four-u32 header
    // followed by two u32 per entry. This has to bound the entry loop before
    // the loop trusts a length from the file.

    let mut instruction_table = InstructionTable {
        canonical_stack_map_entries_offset: stream.read_raw_u32()? as usize,
        length: stream.read_raw_u32()? as usize,
        first_entry_with_code: stream.read_raw_u32()? as usize,
        padding: stream.read_raw_u32()? as usize,
        data: Vec::default(),
    };

    const HEADER_BYTES: usize = 4 * std::mem::size_of::<u32>();
    const ENTRY_BYTES: usize = 2 * std::mem::size_of::<u32>();
    let entry_bytes = instruction_table
        .length
        .checked_mul(ENTRY_BYTES)
        .ok_or_else(|| anyhow::anyhow!("instruction-table entry size overflows usize"))?;
    let required_bytes = HEADER_BYTES
        .checked_add(entry_bytes)
        .ok_or_else(|| anyhow::anyhow!("instruction-table data size overflows usize"))?;
    anyhow::ensure!(
        required_bytes <= data_byte_size,
        "instruction table declares {} entries ({required_bytes} bytes), but its data is {data_byte_size} bytes",
        instruction_table.length
    );

    for _idx in 0..instruction_table.length {
        let entry = DataEntry {
            pc_offset: stream.read_raw_u32()? as usize,
            stack_map_offset: stream.read_raw_u32()? as usize,
        };

        instruction_table.data.push(entry);
    }

    Ok(instruction_table)
}

pub fn get_pc_offset_from_code_cluster_index(
    code_cluster_index: u32,
    instr_table: &InstructionTable,
) -> anyhow::Result<usize> {
    let abs_index = instr_table
        .first_entry_with_code
        .checked_add(code_cluster_index as usize)
        .ok_or_else(|| anyhow::anyhow!("instruction-table index overflow"))?;
    Ok(instr_table.pc_offset_at(abs_index)? as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_bytes(data_byte_size: u64, length: u32, entries: &[(u32, u32)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&data_byte_size.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        for (pc_offset, stack_map_offset) in entries {
            bytes.extend_from_slice(&pc_offset.to_le_bytes());
            bytes.extend_from_slice(&stack_map_offset.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn rejects_a_length_that_does_not_fit_the_declared_data_size() {
        let bytes = table_bytes(16, 2, &[]);
        let Err(error) = parse_instr_table_from_rodata(&mut Stream::new(&bytes)) else {
            panic!("oversized instruction table should be rejected");
        };
        assert!(
            error.to_string().contains("declares 2 entries"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn parses_a_table_that_fits_the_declared_data_size() {
        let bytes = table_bytes(32, 2, &[(0x10, 0), (0x20, 4)]);
        let table = parse_instr_table_from_rodata(&mut Stream::new(&bytes)).unwrap();
        assert_eq!(table.len(), 2);
        assert_eq!(table.pc_offset_at(0).unwrap(), 0x10);
        assert_eq!(table.pc_offset_at(1).unwrap(), 0x20);
    }
}
