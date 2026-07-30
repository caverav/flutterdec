pub mod structs;
use structs::{DispatchTable, DispatchTableEntry, FieldTable, ObjectStore};

use crate::stream::Stream;

pub fn parse_object_store(stream: &mut Stream) -> anyhow::Result<ObjectStore> {
    ObjectStore::read(stream)
}

pub fn parse_field_table(stream: &mut Stream) -> anyhow::Result<FieldTable> {
    let mut field_table = FieldTable::default();
    let table_length = stream.read_unsigned()?;

    // extremely unlikely but one never knows
    field_table.length = table_length.try_into().map_err(|table_length: u64| {
        anyhow::anyhow!(
            "field table of length {} does not fit in usize",
            table_length
        )
    })?;

    // field_table.field_refs = Vec::with_capacity(table_length as usize); // passed the try_into above, safe to raw cast
    
    for _ in 0..table_length {
        let refid = stream.read_ref_id()?;
        field_table.field_refs.push(refid);
    }

    Ok(field_table)
}

pub fn parse_dispatch_table(stream: &mut Stream) -> anyhow::Result<DispatchTable> {
    const RECENT_COUNT: usize = 1 << 6;
    const RECENT_MASK: usize = RECENT_COUNT - 1;
    const MAX_REPEAT: i64 = RECENT_COUNT as i64 - 1;
    const RECENT_MIN: i64 = -MAX_REPEAT;
    const INDEX_BASE: i64 = MAX_REPEAT + 1;

    let length: usize = stream.read_unsigned()?.try_into().map_err(|length: u64| {
        anyhow::anyhow!("dispatch table of length {length} does not fit in usize")
    })?;

    // for an empty table, the serializer writes only its length.
    if length == 0 {
        return Ok(DispatchTable::default());
    }

    let first_code_ref = stream
        .read_unsigned()?
        .try_into()
        .map_err(|reference: u64| {
            anyhow::anyhow!("first Code-cluster reference {reference} does not fit in u32")
        })?;

    let mut table = DispatchTable {
        first_code_ref: Some(first_code_ref),
        // dont reserve from an untrusted claimed length before consuming
        // entries, same thing as FieldTable parsing.
        entries: Vec::new(),
    };
    let mut previous = DispatchTableEntry::Invalid;
    let mut recent: [Option<DispatchTableEntry>; RECENT_COUNT] = [None; RECENT_COUNT];
    let mut recent_index = 0;
    let mut repeat_remaining = 0usize;

    while table.entries.len() < length {
        if repeat_remaining != 0 {
            table.entries.push(previous);
            repeat_remaining -= 1;
            continue;
        }

        let encoded = stream.read()? as i64;
        match encoded {
            0 => previous = DispatchTableEntry::Invalid,
            RECENT_MIN..=-1 => {
                let slot = (!encoded) as usize;
                previous = recent[slot].ok_or_else(|| {
                    anyhow::anyhow!(
                        "dispatch table recent-entry reference {slot} appears before it is defined"
                    )
                })?;
            }
            1..=MAX_REPEAT => {
                repeat_remaining = (encoded - 1) as usize;
            }
            _ => {
                let code_index = (encoded - INDEX_BASE) as u64;
                previous = DispatchTableEntry::CodeIndex(code_index);
                recent[recent_index] = Some(previous);
                recent_index = (recent_index + 1) & RECENT_MASK;
            }
        }
        table.entries.push(previous);
    }

    if repeat_remaining != 0 {
        anyhow::bail!(
            "dispatch table repeat encoding exceeds its declared length by {repeat_remaining} entries"
        );
    }

    Ok(table)
}
