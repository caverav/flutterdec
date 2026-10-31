use std::collections::HashMap;

use anyhow::Context;

use crate::{
    profile_reader::{
        profile::DataProfile,
        profile_json_dictionary::{FOR_EACH, GLOBAL_OBJECT_POOL, OBJECT_STORE, READ_FILL},
    },
    stream::Stream,
};

#[derive(Default)]
pub(crate) struct ObjectStore {
    fields: HashMap<String, u32>,
}

impl ObjectStore {
    pub fn read(stream: &mut Stream, profile: &DataProfile) -> anyhow::Result<Self> {
        let field_names = profile
            .parsing_operations
            .get(OBJECT_STORE)
            .and_then(|store| store.get(READ_FILL))
            .and_then(|fill| fill.get(FOR_EACH))
            .ok_or_else(|| {
                anyhow::anyhow!("profile has no {OBJECT_STORE}.{READ_FILL}.{FOR_EACH} field list")
            })?;
        let field_names: Vec<String> = serde_json::from_value(field_names.clone())
            .context("decode ordered ObjectStore field names")?;

        let mut fields = HashMap::new();
        for field_name in field_names {
            anyhow::ensure!(
                !fields.contains_key(&field_name),
                "duplicate ObjectStore field {field_name:?} in profile"
            );
            let reference = stream
                .read_ref_id()
                .with_context(|| format!("read ObjectStore field {field_name:?}"))?;
            fields.insert(field_name, reference);
        }

        Ok(Self { fields })
    }

    pub fn global_object_pool(&self) -> anyhow::Result<u32> {
        self.fields
            .get(GLOBAL_OBJECT_POOL)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("ObjectStore has no {GLOBAL_OBJECT_POOL} field"))
    }
}

pub(super) struct IsolateObjectStore
// small one, not too important, skip for now
{
    dart_args_1: u32,         // ArrayPtr
    dart_args_2: u32,         // ArrayPtr
    resume_capabilities: u32, // GrowableObjectArrayPtr
    exit_listeners: u32,      // GrowableObjectArrayPtr
    error_listeners: u32,     // GrowableObjectArrayPtr
}
#[derive(Default)]
pub(crate) struct FieldTable {
    pub(super) length: usize,
    pub(super) field_refs: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DispatchTableEntry {
    Invalid,
    CodeIndex(u64), // this is an index INTO the instruction table
}

#[derive(Debug, Default)]
pub(crate) struct DispatchTable {
    pub(super) first_code_ref: Option<u32>,
    pub(super) entries: Vec<DispatchTableEntry>,
}

#[derive(Default)]
pub struct ProgramRoots {
    object_store: ObjectStore,
    field_table: FieldTable,
    shared_field_table: FieldTable,
    dispatch_table: DispatchTable,
}

impl ProgramRoots {
    pub(crate) fn new(
        object_store: ObjectStore,
        field_table: FieldTable,
        shared_field_table: FieldTable,
        dispatch_table: DispatchTable,
    ) -> Self {
        Self {
            object_store,
            field_table,
            shared_field_table,
            dispatch_table,
        }
    }

    pub(crate) fn object_store(&self) -> &ObjectStore {
        &self.object_store
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_with_fields(field_names: &[&str]) -> DataProfile {
        let mut profile = DataProfile::default();
        profile.parsing_operations.insert(
            OBJECT_STORE.into(),
            serde_json::json!({
                (READ_FILL): { (FOR_EACH): field_names }
            }),
        );
        profile
    }

    #[test]
    fn object_store_follows_profile_order_and_stops_before_the_next_root() {
        let layouts: &[(&[&str], &[u8])] = &[
            (
                &["new_vm_root", GLOBAL_OBJECT_POOL, "root_library"],
                &[0x81, 0x01, 0x80, 0x83, 0x5a],
            ),
            (
                &[GLOBAL_OBJECT_POOL, "root_library"],
                &[0x01, 0x80, 0x83, 0x5a],
            ),
        ];
        for (field_names, bytes) in layouts {
            let profile = profile_with_fields(field_names);
            let mut stream = Stream::new(bytes);
            let store = ObjectStore::read(&mut stream, &profile).unwrap();

            assert_eq!(store.fields.len(), field_names.len());
            assert_eq!(store.global_object_pool().unwrap(), 128);
            assert_eq!(store.fields["root_library"], 3);
            assert_eq!(stream.get_current_pos(), bytes.len() - 1);
            assert_eq!(stream.read_byte().unwrap(), 0x5a);
        }
    }

    #[test]
    fn object_store_requires_an_ordered_profile_field_list() {
        for recipe in [
            None,
            Some(serde_json::json!({
                (READ_FILL): { (FOR_EACH): { "root_library": 1 } }
            })),
        ] {
            let mut profile = DataProfile::default();
            if let Some(recipe) = recipe {
                profile
                    .parsing_operations
                    .insert(OBJECT_STORE.into(), recipe);
            }
            let mut stream = Stream::new(&[0x81]);

            assert!(ObjectStore::read(&mut stream, &profile).is_err());
            assert_eq!(stream.get_current_pos(), 0);
        }
    }

    #[test]
    fn object_store_reports_a_missing_global_pool() {
        let profile = profile_with_fields(&["root_library"]);
        let mut stream = Stream::new(&[0x81]);
        let store = ObjectStore::read(&mut stream, &profile).unwrap();

        assert!(store
            .global_object_pool()
            .unwrap_err()
            .to_string()
            .contains(GLOBAL_OBJECT_POOL));
    }

    #[test]
    fn object_store_rejects_duplicate_names_and_truncated_references() {
        let profile = profile_with_fields(&["root_library", "root_library"]);
        assert!(ObjectStore::read(&mut Stream::new(&[0x81, 0x82]), &profile).is_err());

        let profile = profile_with_fields(&[GLOBAL_OBJECT_POOL]);
        let error = ObjectStore::read(&mut Stream::new(&[0x01]), &profile)
            .err()
            .unwrap();
        assert!(error.to_string().contains(GLOBAL_OBJECT_POOL));
    }
}
