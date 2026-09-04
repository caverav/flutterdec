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

// The ruler's own assertions. A separate, digest-protected file rather than
// an inline module here, because this file is product source that later
// work edits, so a digest over it would fire on legitimate change. This
// declaration is the only thing that compiles that file and cannot be digested
// either, so `scripts/check-oracle-inventory.py` proves it by compilation.
#[cfg(test)]
#[path = "validate/tests.rs"]
mod tests;
