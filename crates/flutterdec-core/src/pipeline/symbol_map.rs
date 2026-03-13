use capstone::arch::arm64::ArchMode;
use capstone::prelude::*;
use goblin::elf::header::EM_AARCH64;
use goblin::elf::section_header::SHF_EXECINSTR;
use goblin::elf::sym::{STT_FUNC, STT_NOTYPE};
use goblin::elf::Elf;
use serde::Deserialize;
use std::collections::BTreeMap;

include!("symbol_map/types.rs");
include!("symbol_map/run.rs");
include!("symbol_map/cache.rs");
include!("symbol_map/elf.rs");
include!("symbol_map/analysis.rs");

#[cfg(test)]
#[path = "symbol_map/tests.rs"]
mod tests;
