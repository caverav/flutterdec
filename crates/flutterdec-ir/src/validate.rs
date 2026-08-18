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

use crate::{BasicBlock, FunctionIr};
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
    /// A successor list is not ascending, or names the same block twice. Order is
    /// output-affecting: the emitters render a conditional's arms in successor
    /// order, and a duplicate makes one arm two.
    UnorderedSuccessors { id: usize },
    /// A predecessor list is not ascending, or names the same block twice. Join
    /// provenance is recorded in predecessor order, and a duplicate turns one
    /// incoming path into two claims about the same one.
    UnorderedPredecessors { id: usize },
    /// An edge exists on the successor side and not on the predecessor side, so a
    /// reader of one disagrees with a reader of the other about the graph.
    SuccessorWithoutPredecessor { from: usize, to: usize },
    /// An edge exists on the predecessor side and not on the successor side.
    PredecessorWithoutSuccessor { of: usize, from: usize },
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
            Self::UnorderedSuccessors { id } => {
                write!(f, "successors of {id} are not ascending and unique")
            }
            Self::UnorderedPredecessors { id } => {
                write!(f, "predecessors of {id} are not ascending and unique")
            }
            Self::SuccessorWithoutPredecessor { from, to } => {
                write!(f, "edge {from} -> {to} is missing its predecessor")
            }
            Self::PredecessorWithoutSuccessor { of, from } => {
                write!(f, "predecessor {from} of {of} has no matching edge")
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

/// Whether the graph is canonical: block identity holds *and* every edge is
/// stated once, in ascending order, from both ends.
///
/// This is the ruler every internal producer is held to, and it is the identity
/// ruler plus the edge clauses -- never a different rule set. A consumer that
/// only indexes blocks needs `validate_block_identity`; a producer that mutates
/// edges owes this.
///
/// Edge order matters to output, not just to tidiness: both emitters render a
/// conditional's arms in successor order and record join provenance in
/// predecessor order, so an unstable list is an unstable artifact. Reciprocity
/// matters because both sides have readers -- `helper_flow` scores a block from
/// its predecessors while the emitters walk successors -- and a one-sided edge
/// makes those two readers describe different graphs.
pub fn validate_canonical_cfg(ir: &FunctionIr) -> Result<(), CfgDefect> {
    validate_block_identity(ir)?;
    for b in &ir.blocks {
        if !is_ascending_unique(&b.succs) {
            return Err(CfgDefect::UnorderedSuccessors { id: b.id });
        }
        if !is_ascending_unique(&b.preds) {
            return Err(CfgDefect::UnorderedPredecessors { id: b.id });
        }
    }
    // Identity holds, so `id == position` and every endpoint is in range.
    for b in &ir.blocks {
        for s in &b.succs {
            if !ir.blocks[*s].preds.contains(&b.id) {
                return Err(CfgDefect::SuccessorWithoutPredecessor { from: b.id, to: *s });
            }
        }
        for p in &b.preds {
            if !ir.blocks[*p].succs.contains(&b.id) {
                return Err(CfgDefect::PredecessorWithoutSuccessor { of: b.id, from: *p });
            }
        }
    }
    Ok(())
}

fn is_ascending_unique(ids: &[usize]) -> bool {
    ids.windows(2).all(|w| w[0] < w[1])
}

/// The one path that makes a mutated CFG canonical again.
///
/// Successor lists are the authority and predecessor lists are derived from them
/// in full, so reciprocity cannot be half-applied. A pass that removed an edge by
/// hand from both sides was two copies of this rule that had to agree; the copy
/// on the predecessor side is the one that goes stale, and `helper_flow` reads
/// predecessors directly to score a block.
///
/// A successor naming a block that does not exist is dropped rather than kept: an
/// edge to nothing is not an edge, and leaving it in would make every consumer
/// index out of range. Callers that renumber must remap their successor ids
/// *before* calling this, or the edge is dropped rather than moved.
pub fn rebuild_edges(blocks: &mut [BasicBlock]) {
    let ids: BTreeSet<usize> = blocks.iter().map(|b| b.id).collect();
    for b in blocks.iter_mut() {
        b.succs.retain(|s| ids.contains(s));
        b.succs.sort_unstable();
        b.succs.dedup();
    }
    // A set per target, so the derived list is ascending and unique by
    // construction rather than by a sort a caller could forget.
    let mut incoming: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for b in blocks.iter() {
        for s in &b.succs {
            incoming.entry(*s).or_default().insert(b.id);
        }
    }
    for b in blocks.iter_mut() {
        b.preds = incoming
            .get(&b.id)
            .map(|from| from.iter().copied().collect())
            .unwrap_or_default();
    }
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

    /// One planted failure per edge rule. Every row is one field edit away from a
    /// graph the ruler accepts, so a rejection is the edge and nothing else.
    #[test]
    fn every_planted_edge_failure_is_named() {
        assert_eq!(validate_canonical_cfg(&diamond()), Ok(()));

        let mut ir = diamond();
        ir.blocks[0].succs = vec![1, 1, 2];
        ir.blocks[1].preds = vec![0, 0];
        assert_eq!(
            validate_canonical_cfg(&ir),
            Err(CfgDefect::UnorderedSuccessors { id: 0 }),
            "a duplicate successor makes one arm two"
        );

        let mut ir = diamond();
        ir.blocks[0].succs = vec![2, 1];
        assert_eq!(
            validate_canonical_cfg(&ir),
            Err(CfgDefect::UnorderedSuccessors { id: 0 }),
            "arm order is output-affecting"
        );

        let mut ir = diamond();
        ir.blocks[3].preds = vec![1, 1, 2];
        assert_eq!(
            validate_canonical_cfg(&ir),
            Err(CfgDefect::UnorderedPredecessors { id: 3 }),
            "a duplicate predecessor is a second claim about one path"
        );

        let mut ir = diamond();
        ir.blocks[3].preds = vec![2, 1];
        assert_eq!(
            validate_canonical_cfg(&ir),
            Err(CfgDefect::UnorderedPredecessors { id: 3 }),
            "join provenance is recorded in predecessor order"
        );

        let mut ir = diamond();
        ir.blocks[3].preds = vec![2];
        assert_eq!(
            validate_canonical_cfg(&ir),
            Err(CfgDefect::SuccessorWithoutPredecessor { from: 1, to: 3 }),
            "an edge only the successor side knows about"
        );

        let mut ir = diamond();
        ir.blocks[1].succs = Vec::new();
        assert_eq!(
            validate_canonical_cfg(&ir),
            Err(CfgDefect::PredecessorWithoutSuccessor { of: 3, from: 1 }),
            "an edge only the predecessor side knows about"
        );

        // Identity is checked first, so an edge clause can never mask a defect
        // that would make the edge clauses index the wrong rows.
        let mut ir = diamond();
        ir.blocks[3].id = 9;
        ir.blocks[3].preds = vec![2, 1];
        assert_eq!(
            validate_canonical_cfg(&ir),
            Err(CfgDefect::NonDenseBlockId { position: 3, id: 9 }),
            "identity comes first"
        );
    }

    #[test]
    fn every_edge_defect_renders_a_distinct_one_line_diagnostic() {
        let rendered: Vec<String> = [
            CfgDefect::UnorderedSuccessors { id: 0 },
            CfgDefect::UnorderedPredecessors { id: 3 },
            CfgDefect::SuccessorWithoutPredecessor { from: 1, to: 3 },
            CfgDefect::PredecessorWithoutSuccessor { of: 3, from: 1 },
        ]
        .iter()
        .map(|d| d.to_string())
        .collect();
        assert_eq!(
            rendered,
            vec![
                "successors of 0 are not ascending and unique",
                "predecessors of 3 are not ascending and unique",
                "edge 1 -> 3 is missing its predecessor",
                "predecessor 1 of 3 has no matching edge",
            ]
        );
        for text in &rendered {
            assert!(!text.contains('\n'), "a diagnostic is one line: {text}");
        }
    }

    /// The canonical path takes any of the ways a mutation can leave edges wrong
    /// and produces the one form the ruler accepts, without a caller having to
    /// know which of them applied.
    #[test]
    fn the_canonical_rebuild_repairs_every_edge_defect() {
        let mut ir = diamond();
        // Unsorted, duplicated, and pointing at a block that does not exist.
        ir.blocks[0].succs = vec![2, 1, 1, 9];
        // Stale on both sides: one predecessor that no longer has an edge, one
        // edge whose predecessor was never recorded, and a duplicate.
        ir.blocks[1].preds = vec![3, 3];
        ir.blocks[2].preds = Vec::new();
        ir.blocks[3].preds = vec![2, 1, 1];

        rebuild_edges(&mut ir.blocks);

        assert_eq!(validate_canonical_cfg(&ir), Ok(()));
        assert_eq!(
            ir.blocks[0].succs,
            vec![1, 2],
            "sorted, unique, and existing"
        );
        assert_eq!(ir.blocks[0].preds, Vec::<usize>::new());
        assert_eq!(ir.blocks[1].preds, vec![0], "derived from successors alone");
        assert_eq!(ir.blocks[2].preds, vec![0]);
        assert_eq!(ir.blocks[3].preds, vec![1, 2]);
        assert!(
            !ir.blocks[0].succs.contains(&9),
            "an edge to a block that does not exist is not an edge"
        );
    }

    /// The rebuild is idempotent: running it on its own output changes nothing, so
    /// a pass that calls it twice cannot drift.
    #[test]
    fn the_canonical_rebuild_is_idempotent() {
        let mut once = diamond();
        once.blocks[0].succs = vec![2, 1, 1];
        rebuild_edges(&mut once.blocks);
        let mut twice = once.clone();
        rebuild_edges(&mut twice.blocks);
        for (a, b) in once.blocks.iter().zip(&twice.blocks) {
            assert_eq!(a.succs, b.succs, "block {}", a.id);
            assert_eq!(a.preds, b.preds, "block {}", a.id);
        }
    }

    /// The guard prune removes the guard's own slow path and nothing else. A block
    /// unreachable for any other reason is code the adapter merged in from a
    /// neighbouring function, and deleting it would silently lose real program
    /// text, so it must survive with its ids still dense and its edges canonical.
    #[test]
    fn only_the_guard_stranded_blocks_are_pruned() {
        use flutterdec_disasm_arm64::{AsmInstruction, FunctionDisassembly};

        let ins = |va: u64, mnemonic: &str, op_str: &str| AsmInstruction {
            va,
            word: 0,
            mnemonic: mnemonic.to_string(),
            op_str: op_str.to_string(),
            annotation: String::new(),
        };
        // The guard and its slow path at 0x1014, plus an island at 0x1020 that
        // nothing in the record reaches and that the guard never reached either.
        let d = FunctionDisassembly {
            function_id: 11,
            function_name: "guardedWithIsland".to_string(),
            owner_class: "Global".to_string(),
            entry_va: 0x1000,
            size: 0x2c,
            instructions: vec![
                ins(0x1000, "ldr", "x16, [x26, #0x38]"),
                ins(0x1004, "cmp", "x15, x16"),
                ins(0x1008, "b.ls", "#0x1014"),
                ins(0x100c, "mov", "x0, x1"),
                ins(0x1010, "ret", ""),
                // Guard slow path: calls the stub, jumps back into the body.
                ins(0x1014, "bl", "#0x9000"),
                ins(0x1018, "b", "#0x100c"),
                // Island: unreachable, and not through the guard.
                ins(0x101c, "mov", "x2, x3"),
                ins(0x1020, "ret", ""),
            ],
        };

        let ir = crate::build_function_ir(&d);
        assert_eq!(validate_canonical_cfg(&ir), Ok(()));

        let starts: Vec<u64> = ir.blocks.iter().map(|b| b.start_va).collect();
        assert!(
            !starts.contains(&0x1014),
            "the guard's slow path is the one thing pruned: {starts:x?}"
        );
        assert!(
            starts.contains(&0x101c),
            "an unrelated unreachable block must survive: {starts:x?}"
        );
        assert_eq!(
            ir.blocks
                .iter()
                .find(|b| b.start_va == 0x101c)
                .map(|b| (b.succs.clone(), b.preds.clone())),
            Some((Vec::new(), Vec::new())),
            "it survives as an orphan, with no edge invented in either direction"
        );
    }

    /// The builder's own output has to satisfy the ruler its consumers apply, on
    /// every path it can take: a conditional with a fallthrough, a conditional
    /// whose target *is* its fallthrough so the derived list holds one block
    /// twice, the terminators that take no edge at all, and the guarded shape
    /// whose slow path is pruned and whose ids are all remapped.
    #[test]
    fn the_builder_is_canonical_on_every_path_it_takes() {
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

        let cases = vec![
            (
                "conditional, indirect branch and trap",
                record(vec![
                    ins(0x1000, "cbz", "x0, #0x1010"),
                    ins(0x1004, "br", "x16"),
                    ins(0x1008, "brk", "#0x1"),
                    ins(0x100c, "ret", ""),
                    ins(0x1010, "ret", ""),
                ]),
                None,
            ),
            (
                "conditional whose target is its own fallthrough",
                record(vec![
                    ins(0x1000, "cbz", "x0, #0x1004"),
                    ins(0x1004, "mov", "x0, x1"),
                    ins(0x1008, "ret", ""),
                ]),
                // One block, named once, not twice.
                Some(vec![vec![1usize], vec![]]),
            ),
            (
                "unconditional jump to its own fallthrough",
                record(vec![ins(0x1000, "b", "#0x1004"), ins(0x1004, "ret", "")]),
                Some(vec![vec![1usize], vec![]]),
            ),
            (
                "guard and its pruned slow path",
                record(vec![
                    ins(0x1000, "ldr", "x16, [x26, #0x38]"),
                    ins(0x1004, "cmp", "x15, x16"),
                    ins(0x1008, "b.ls", "#0x1014"),
                    ins(0x100c, "mov", "x0, x1"),
                    ins(0x1010, "ret", ""),
                    ins(0x1014, "bl", "#0x9000"),
                    ins(0x1018, "b", "#0x100c"),
                ]),
                None,
            ),
            (
                "no instructions at all",
                record(Vec::new()),
                Some(Vec::new()),
            ),
        ];

        for (label, case, expected_succs) in cases {
            let ir = crate::build_function_ir(&case);
            assert_eq!(
                validate_canonical_cfg(&ir),
                Ok(()),
                "{label}: the builder must not emit a graph its own consumers refuse: {:?}",
                ir.blocks
                    .iter()
                    .map(|b| (b.id, b.start_va, b.succs.clone(), b.preds.clone()))
                    .collect::<Vec<_>>()
            );
            if let Some(expected) = expected_succs {
                assert_eq!(
                    ir.blocks
                        .iter()
                        .map(|b| b.succs.clone())
                        .collect::<Vec<_>>(),
                    expected,
                    "{label}: edge list"
                );
            }
        }
    }
}
