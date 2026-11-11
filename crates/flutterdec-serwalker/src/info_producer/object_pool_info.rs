use flutterdec_adapter::model::{
    ObjectPool as ModelObjectPool, PoolEntry, PoolEntryKind, PoolGeometry, PoolIndexSpace,
    ProgramModel, Provenance,
};
use serde_json::json;

use crate::raw_object::{Class, Code, Field, Mint, ObjectPool, ObjectPoolEntryValue, _String};
use crate::snapshot::{DataSnapshot, InstructionsSnapshot};

fn append_escaped_text(output: &mut String, text: &str) -> serde_json::Result<()> {
    let quoted = serde_json::to_string(text)?;
    output.push_str(&quoted[1..quoted.len() - 1]);
    Ok(())
}

fn string_literal(code_units: &[u16]) -> serde_json::Result<String> {
    let mut output = String::from("\"");
    let mut text = String::new();
    for decoded in char::decode_utf16(code_units.iter().copied()) {
        match decoded {
            Ok(character) => text.push(character),
            Err(error) => {
                append_escaped_text(&mut output, &text)?;
                text.clear();
                output.push_str(&format!("\\u{:04X}", error.unpaired_surrogate()));
            }
        }
    }
    append_escaped_text(&mut output, &text)?;
    output.push('"');
    Ok(output)
}

fn tagged_object_value(
    snapshot: &DataSnapshot,
    instructions: &InstructionsSnapshot,
    reference_id: u32,
    pool_index: usize,
    model: &mut ProgramModel,
) -> anyhow::Result<(PoolEntryKind, Option<String>, Option<u64>)> {
    let undecoded = (PoolEntryKind::Undecoded, None, None);
    let Some(cluster) = snapshot.cluster_by_ref_id(reference_id)? else {
        return Ok(undecoded);
    };
    let object = cluster
        .object_by_ref_id(reference_id)
        .ok_or_else(|| anyhow::anyhow!("cluster no longer contains reference ID {reference_id}"))?;
    if let Some(string) = object.downcast_ref::<_String>() {
        if char::decode_utf16(string.code_units.iter().copied()).any(|decoded| decoded.is_err()) {
            model.extensions.insert(
                format!("serwalker.pool.{pool_index}"),
                json!({"reference_id": reference_id, "string_value_encoding": "utf16_escaped_literal",
                       "string_code_units": string.code_units}),
            );
        }
        return Ok((
            PoolEntryKind::String,
            Some(string_literal(&string.code_units)?),
            None,
        ));
    }
    if let Some(integer) = object.downcast_ref::<Mint>() {
        // A Mint is a heap object, but the entry holds an integer value. The
        // model's Immediate kind describes that value, not the heap shape.
        return Ok((
            PoolEntryKind::Immediate,
            Some(integer.value.to_string()),
            None,
        ));
    }
    if let Some(class) = object.downcast_ref::<Class>() {
        return Ok((PoolEntryKind::Class, Some(class.id.to_string()), None));
    }
    if object.is::<Field>() {
        return Ok((PoolEntryKind::Field, Some(reference_id.to_string()), None));
    }
    if let Some(code) = object.downcast_ref::<Code>() {
        if code.entry_point != u64::MAX {
            let address = instructions
                .image_va
                .checked_add(code.entry_point)
                .ok_or_else(|| anyhow::anyhow!("object-pool code address overflow"))?;
            return Ok((
                PoolEntryKind::Code,
                Some(reference_id.to_string()),
                Some(address),
            ));
        }
    }
    Ok(undecoded)
}

pub fn produce_model_object_pool_info(
    snapshot: &DataSnapshot,
    instructions: &InstructionsSnapshot,
    global_object_pool: &ObjectPool,
    model: &mut ProgramModel,
) -> anyhow::Result<()> {
    const ENTRIES_OFFSET: u64 = 16;
    const WORD_SIZE: u64 = 8; // size in bytes of ObjectPoolEntry objects
    anyhow::ensure!(
        global_object_pool.length == global_object_pool.entries.len(),
        "ObjectPool length disagrees with its entries"
    );
    let mut object_pool_info = Vec::with_capacity(global_object_pool.length);

    for (index, entry) in global_object_pool.entries.iter().enumerate() {
        let (kind, value, target_va) = match &entry.value {
            ObjectPoolEntryValue::TaggedObjectRef(reference_id) => {
                tagged_object_value(snapshot, instructions, *reference_id, index, model)?
            }
            ObjectPoolEntryValue::Immediate(value) => {
                (PoolEntryKind::Immediate, Some(value.to_string()), None)
            }
            _ => (PoolEntryKind::Undecoded, None, None),
        };
        if kind == PoolEntryKind::Undecoded {
            let cid = match entry.value {
                ObjectPoolEntryValue::TaggedObjectRef(reference_id) => snapshot
                    .cluster_by_ref_id(reference_id)?
                    .map(|cluster| cluster.cid()),
                _ => None,
            };
            model.extensions.insert(
                format!("serwalker.pool.{index}"),
                json!({"entry_bits": entry.entry_bits, "cid": cid,
                       "serialized_value": format!("{:?}", entry.value)}),
            );
        }
        object_pool_info.push(PoolEntry {
            index: u64::try_from(index)
                .map_err(|_| anyhow::anyhow!("ObjectPool index does not fit in u64"))?,
            kind,
            value,
            target_va,
            provenance: if target_va.is_some() {
                Provenance::Derived
            } else {
                Provenance::Exact
            },
            confidence: None,
        });
    }

    model.object_pool = if object_pool_info.is_empty() {
        ModelObjectPool::unavailable()
    } else {
        ModelObjectPool {
            index_space: PoolIndexSpace::Hardware,
            geometry: Some(PoolGeometry {
                entries_offset: ENTRIES_OFFSET,
                word_size: WORD_SIZE,
            }),
            entries: object_pool_info,
        }
    };
    Ok(())
}
