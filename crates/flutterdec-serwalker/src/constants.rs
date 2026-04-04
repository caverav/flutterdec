use std::mem::size_of;

pub const SNAPSHOT_MAGIC_NUMBER_SZ: usize = size_of::<u32>();
pub const SNAPSHOT_LEN_SZ: usize = size_of::<u64>();
pub const SNAPSHOT_KIND_SZ: usize = size_of::<u64>();

pub const NUM_BASE_OBJECTS_SZ: usize = size_of::<u64>();
pub const NUM_OBJECTS_SZ: usize = size_of::<u64>();
pub const NUM_CLUSTERS_SZ: usize = size_of::<u64>();

pub const INSTR_TABLE_LEN_SZ: usize = size_of::<u64>();
pub const INSTR_TABLE_OFFSET_SZ: usize = size_of::<u64>();

pub const OBJECT_STORE_ENTRY_SIZE: usize = size_of::<u64>();