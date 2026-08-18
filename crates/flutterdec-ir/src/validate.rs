//! The one well-formedness ruler for a `FunctionIr`.
//!
//! Every consumer downstream of the builder indexes blocks by id or by start
//! address: the emitter builds `block_by_id` and `va_to_id`, region analysis
//! builds id-indexed successor and predecessor vectors and reads them back by
//! id, and the record splitter maps every instruction address to its containing
//! block. A map keyed on a value that is not unique silently keeps one entry, so
//! every one of those consumers would then read a relation off a graph that does
//! not exist, with no failure anywhere to point at.
//!
//! The rules live here once. A consumer that carried its own weaker copy would
//! disagree with the rest about what a usable graph is, which is the failure
//! this module exists to remove.

use crate::FunctionIr;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Why a `FunctionIr` cannot be indexed or analysed.
///
/// Deliberately not `#[non_exhaustive]`: a new defect must break every match on
/// it, because a consumer that silently lumps an unknown defect in with the ones
/// it already handles is how a graph reaches analysis unchecked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfgDefect {
    /// Two blocks carry the same id, so an id-keyed map keeps one of them and
    /// every edge naming that id resolves to whichever one it kept.
    DuplicateBlockId { id: usize },
    /// No block carries id 0. Both emitters and region analysis take the entry
    /// from position 0, so there is no entry to walk from.
    MissingEntryBlock,
    /// Ids are not exactly `0..blocks.len()` in order. `structured.rs` iterates
    /// that range as ids and region analysis indexes its vectors by id, so a
    /// sparse or reordered numbering reads another block's relations.
    NonDenseBlockId { position: usize, id: usize },
    /// Two blocks claim the same start address, so an address-keyed map resolves
    /// a branch target to whichever one it kept.
    DuplicateStartVa {
        start_va: u64,
        first: usize,
        second: usize,
    },
    /// A successor names a block that does not exist.
    MissingSuccessorBlock { from: usize, to: usize },
    /// A predecessor names a block that does not exist.
    MissingPredecessorBlock { of: usize, from: usize },
}

impl fmt::Display for CfgDefect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateBlockId { id } => write!(f, "duplicate block id {id}"),
            Self::MissingEntryBlock => write!(f, "no entry block 0"),
            Self::NonDenseBlockId { position, id } => {
                write!(f, "block id {id} at position {position} is not dense")
            }
            Self::DuplicateStartVa {
                start_va,
                first,
                second,
            } => write!(f, "blocks {first} and {second} both start at {start_va:#x}"),
            Self::MissingSuccessorBlock { from, to } => {
                write!(f, "edge {from} -> {to} names a block that does not exist")
            }
            Self::MissingPredecessorBlock { of, from } => {
                write!(
                    f,
                    "predecessor {from} of {of} names a block that does not exist"
                )
            }
        }
    }
}

/// Whether the graph's block identity holds, so it can be indexed by id and by
/// start address and walked by either emitter.
///
/// Call this *before* building any id or address map, never after: after is too
/// late, the map has already collapsed the duplicate it was supposed to detect.
///
/// A function with no blocks is accepted. There is nothing to index and nothing
/// to walk, so there is no identity to be wrong; a record that decoded to no
/// instructions is an empty function, not a malformed graph.
///
/// The first defect in a fixed scan order is returned, so the same graph always
/// reports the same defect.
pub fn validate_block_identity(ir: &FunctionIr) -> Result<(), CfgDefect> {
    let mut ids = BTreeSet::new();
    for b in &ir.blocks {
        if !ids.insert(b.id) {
            return Err(CfgDefect::DuplicateBlockId { id: b.id });
        }
    }
    if ir.blocks.is_empty() {
        return Ok(());
    }
    if !ids.contains(&0) {
        return Err(CfgDefect::MissingEntryBlock);
    }
    // Ids are unique and 0 is among them, so `id == position` for every block is
    // exactly "dense, ascending, and the entry is first", which is what makes
    // position and id interchangeable for every consumer.
    for (position, b) in ir.blocks.iter().enumerate() {
        if b.id != position {
            return Err(CfgDefect::NonDenseBlockId { position, id: b.id });
        }
    }

    let mut starts: BTreeMap<u64, usize> = BTreeMap::new();
    for b in &ir.blocks {
        if let Some(first) = starts.insert(b.start_va, b.id) {
            return Err(CfgDefect::DuplicateStartVa {
                start_va: b.start_va,
                first,
                second: b.id,
            });
        }
    }

    let n = ir.blocks.len();
    for b in &ir.blocks {
        for s in &b.succs {
            if *s >= n {
                return Err(CfgDefect::MissingSuccessorBlock { from: b.id, to: *s });
            }
        }
        for p in &b.preds {
            if *p >= n {
                return Err(CfgDefect::MissingPredecessorBlock { of: b.id, from: *p });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BasicBlock, LlirInstr};

    fn blk(id: usize, start_va: u64, succs: Vec<usize>, preds: Vec<usize>) -> BasicBlock {
        BasicBlock {
            id,
            start_va,
            instrs: vec![LlirInstr {
                va: start_va,
                op: crate::IROp::Other,
                src: "mov x0, x1".to_string(),
                target: String::new(),
            }],
            succs,
            preds,
        }
    }

    /// Every field of both public structs written as a literal, which is the way
    /// every fixture in the workspace and every downstream consumer builds one.
    /// Adding a field or sealing either struct breaks this, and breaking it is a
    /// source-compatibility break for every such caller.
    fn diamond() -> FunctionIr {
        FunctionIr {
            function_id: 1,
            name: "diamond".to_string(),
            entry_va: 0x1000,
            blocks: vec![
                blk(0, 0x1000, vec![1, 2], vec![]),
                blk(1, 0x1004, vec![3], vec![0]),
                blk(2, 0x1008, vec![3], vec![0]),
                blk(3, 0x100c, vec![], vec![1, 2]),
            ],
        }
    }

    #[test]
    fn a_well_formed_graph_is_accepted() {
        assert_eq!(validate_block_identity(&diamond()), Ok(()));
    }

    /// An empty function has no identity to be wrong. Rejecting it would report a
    /// defect for a record that simply decoded to nothing.
    #[test]
    fn a_function_with_no_blocks_is_accepted() {
        let ir = FunctionIr {
            function_id: 2,
            name: "empty".to_string(),
            entry_va: 0x1000,
            blocks: Vec::new(),
        };
        assert_eq!(validate_block_identity(&ir), Ok(()));
    }

    /// One planted failure per identity rule, each rejected with the defect that
    /// names it rather than with whichever check happened to run first.
    #[test]
    fn every_planted_identity_failure_is_named() {
        let mut ir = diamond();
        ir.blocks[2].id = 1;
        assert_eq!(
            validate_block_identity(&ir),
            Err(CfgDefect::DuplicateBlockId { id: 1 }),
            "a duplicate id is what an id-keyed map collapses"
        );

        let mut ir = diamond();
        for (offset, b) in ir.blocks.iter_mut().enumerate() {
            b.id = offset + 1;
        }
        assert_eq!(
            validate_block_identity(&ir),
            Err(CfgDefect::MissingEntryBlock),
            "there is no entry to walk from"
        );

        let mut ir = diamond();
        ir.blocks[3].id = 9;
        assert_eq!(
            validate_block_identity(&ir),
            Err(CfgDefect::NonDenseBlockId { position: 3, id: 9 }),
            "a sparse id reads another block's relations"
        );

        let mut ir = diamond();
        ir.blocks[0].id = 1;
        ir.blocks[1].id = 0;
        assert_eq!(
            validate_block_identity(&ir),
            Err(CfgDefect::NonDenseBlockId { position: 0, id: 1 }),
            "the entry must be first: position and id are read interchangeably"
        );

        let mut ir = diamond();
        ir.blocks[2].start_va = 0x1004;
        assert_eq!(
            validate_block_identity(&ir),
            Err(CfgDefect::DuplicateStartVa {
                start_va: 0x1004,
                first: 1,
                second: 2
            }),
            "an address-keyed map collapses a duplicate start"
        );

        let mut ir = diamond();
        ir.blocks[1].succs = vec![7];
        assert_eq!(
            validate_block_identity(&ir),
            Err(CfgDefect::MissingSuccessorBlock { from: 1, to: 7 }),
            "an edge to a block that does not exist is not an edge"
        );

        let mut ir = diamond();
        ir.blocks[1].preds = vec![7];
        assert_eq!(
            validate_block_identity(&ir),
            Err(CfgDefect::MissingPredecessorBlock { of: 1, from: 7 }),
            "a predecessor naming no block is not a predecessor"
        );
    }

    /// The diagnostic text is what the emitter puts in an artifact, so it has to
    /// be stable and it has to name the defect.
    #[test]
    fn every_defect_renders_a_distinct_one_line_diagnostic() {
        let rendered: Vec<String> = [
            CfgDefect::DuplicateBlockId { id: 1 },
            CfgDefect::MissingEntryBlock,
            CfgDefect::NonDenseBlockId { position: 3, id: 9 },
            CfgDefect::DuplicateStartVa {
                start_va: 0x1004,
                first: 1,
                second: 2,
            },
            CfgDefect::MissingSuccessorBlock { from: 1, to: 7 },
            CfgDefect::MissingPredecessorBlock { of: 1, from: 7 },
        ]
        .iter()
        .map(|d| d.to_string())
        .collect();

        assert_eq!(
            rendered,
            vec![
                "duplicate block id 1",
                "no entry block 0",
                "block id 9 at position 3 is not dense",
                "blocks 1 and 2 both start at 0x1004",
                "edge 1 -> 7 names a block that does not exist",
                "predecessor 7 of 1 names a block that does not exist",
            ]
        );
        for text in &rendered {
            assert!(!text.contains('\n'), "a diagnostic is one line: {text}");
        }
    }

    /// The builder's own output has to satisfy the ruler its consumers apply,
    /// including on the paths that renumber: the runtime-check slow-path prune
    /// removes blocks and remaps every id.
    #[test]
    fn the_builder_produces_a_graph_the_ruler_accepts() {
        use flutterdec_disasm_arm64::{AsmInstruction, FunctionDisassembly};

        let ins = |va: u64, mnemonic: &str, op_str: &str| AsmInstruction {
            va,
            word: 0,
            mnemonic: mnemonic.to_string(),
            op_str: op_str.to_string(),
            annotation: String::new(),
        };
        let record = |instructions: Vec<AsmInstruction>| FunctionDisassembly {
            function_id: 3,
            function_name: "built".to_string(),
            owner_class: "Global".to_string(),
            entry_va: 0x1000,
            size: 64,
            instructions,
        };

        // Plain conditional, an indirect branch and a trap, then the guarded
        // shape whose slow path is pruned and whose ids are all remapped.
        let cases = vec![
            record(vec![
                ins(0x1000, "cbz", "x0, #0x1010"),
                ins(0x1004, "br", "x16"),
                ins(0x1008, "brk", "#0x1"),
                ins(0x100c, "ret", ""),
                ins(0x1010, "ret", ""),
            ]),
            record(vec![
                ins(0x1000, "ldr", "x16, [x26, #0x38]"),
                ins(0x1004, "cmp", "x15, x16"),
                ins(0x1008, "b.ls", "#0x1014"),
                ins(0x100c, "mov", "x0, x1"),
                ins(0x1010, "ret", ""),
                ins(0x1014, "bl", "#0x9000"),
                ins(0x1018, "b", "#0x100c"),
            ]),
            record(Vec::new()),
        ];

        for case in cases {
            let ir = crate::build_function_ir(&case);
            assert_eq!(
                validate_block_identity(&ir),
                Ok(()),
                "the builder must not emit a graph its own consumers refuse: {:?}",
                ir.blocks
                    .iter()
                    .map(|b| (b.id, b.start_va, b.succs.clone(), b.preds.clone()))
                    .collect::<Vec<_>>()
            );
        }
    }
}
