use flutterdec_disasm_arm64::{AsmInstruction, FunctionDisassembly};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

mod validate;
pub use validate::{rebuild_edges, validate_block_identity, validate_canonical_cfg, CfgDefect};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum IROp {
    Call,
    Branch,
    Jump,
    Return,
    /// `br Xn`: an indirect branch with no link. Architecturally the block ends
    /// and control leaves through a register value this pipeline does not
    /// recover, so the operation carries no edge in either direction: no
    /// fallthrough, and no guessed target. It is deliberately not `Jump` and not
    /// `Return`; both would name a destination the instruction stream does not
    /// state, and `br` is how a dispatch stub or a tail call leaves a function.
    IndirectBranch,
    /// `brk #imm`: a breakpoint instruction exception. Control does not continue
    /// past it, so the block ends with no successors at all. Distinct from
    /// `Return`, which resumes the caller: a trap resumes nothing.
    Trap,
    LoadPool,
    /// Dart AOT runtime bookkeeping the source program never expressed:
    /// recognised instruction groups that carry no user-level semantics and
    /// must not contribute control-flow edges.
    RuntimeCheck,
    Other,
}

/// Dart AOT ARM64 emits a stack-overflow check as a fixed three-instruction
/// group before the body of any function that can recurse or loop:
///
/// ```text
///   ldr x16, [x26, #stack_limit]   ; TMP = THR->stack_limit_
///   cmp x15, x16                   ; Dart SP vs limit
///   b.ls <slow>                    ; call StackOverflowStub, then jump back
/// ```
///
/// The taken edge is a runtime guard, not program control flow, and its slow
/// path re-enters the body. Left in the CFG it manufactures a spurious
/// conditional, a spurious call and a spurious back edge in every affected
/// function. Returns the indices of the three instructions when the group
/// starts at `cmp`.
fn stack_overflow_check_at(instrs: &[AsmInstruction], cmp_idx: usize) -> Option<[usize; 3]> {
    let cmp = instrs.get(cmp_idx)?;
    if cmp.mnemonic != "cmp" {
        return None;
    }
    // Dart SP (R15) compared against a scratch register.
    let tmp = cmp.op_str.strip_prefix("x15, ")?.trim();
    if !matches!(tmp, "x16" | "x17") {
        return None;
    }
    let ldr = instrs.get(cmp_idx.checked_sub(1)?)?;
    if ldr.mnemonic != "ldr" {
        return None;
    }
    // THR (R26) field load into that same scratch register.
    let rest = ldr.op_str.strip_prefix(tmp)?.strip_prefix(", [x26,")?;
    if !rest.trim_end().ends_with(']') {
        return None;
    }
    let br = instrs.get(cmp_idx + 1)?;
    // Unsigned lower-or-same: SP has crossed the limit.
    if br.mnemonic != "b.ls" {
        return None;
    }
    Some([cmp_idx - 1, cmp_idx, cmp_idx + 1])
}

#[derive(Debug, Clone, Serialize)]
pub struct LlirInstr {
    pub va: u64,
    pub op: IROp,
    pub src: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BasicBlock {
    pub id: usize,
    pub start_va: u64,
    pub instrs: Vec<LlirInstr>,
    pub succs: Vec<usize>,
    pub preds: Vec<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionIr {
    pub function_id: u64,
    pub name: String,
    pub entry_va: u64,
    pub blocks: Vec<BasicBlock>,
}

/// Stage-local identity facts produced while building one function. Kept out of
/// `FunctionIr` so its public schema remains unchanged; artifact writers may
/// attach the additive ledger explicitly.
#[derive(Debug, Clone, Default)]
pub struct IrBuildAccounting {
    pub built: Vec<(usize, u64)>,
    pub guard_pruned: Vec<(usize, u64)>,
    pub guard_remaps: Vec<(usize, u64, usize)>,
}

fn parse_target_hex(s: &str) -> Option<u64> {
    let mut last = None;
    for token in s.split(|c: char| c.is_whitespace() || c == ',') {
        let t = token.trim().trim_start_matches('#');
        if let Some(hex) = t.strip_prefix("0x") {
            if let Ok(v) = u64::from_str_radix(hex, 16) {
                last = Some(v);
                continue;
            }
        }
        if t.chars().all(|c| c.is_ascii_hexdigit()) && t.len() > 6 {
            if let Ok(v) = u64::from_str_radix(t, 16) {
                last = Some(v);
            }
        }
    }
    last
}

fn llir_from_disasm(d: &FunctionDisassembly) -> Vec<LlirInstr> {
    let mut runtime_check = vec![false; d.instructions.len()];
    for idx in 0..d.instructions.len() {
        if let Some(group) = stack_overflow_check_at(&d.instructions, idx) {
            for i in group {
                runtime_check[i] = true;
            }
        }
    }

    d.instructions
        .iter()
        .enumerate()
        .map(|(idx, ins)| {
            let mut op = IROp::Other;
            let mut src = if ins.op_str.is_empty() {
                ins.mnemonic.clone()
            } else {
                format!("{} {}", ins.mnemonic, ins.op_str)
            };
            let mut target = String::new();

            if runtime_check[idx] {
                return LlirInstr {
                    va: ins.va,
                    op: IROp::RuntimeCheck,
                    src: format!("{src} /* dart stack-overflow check */"),
                    // Retained so the slow path it used to reach can be
                    // identified and dropped without a blanket reachability
                    // prune, which would also delete code the adapter merged
                    // in from neighbouring functions.
                    target: ins.op_str.clone(),
                };
            }

            match ins.mnemonic.as_str() {
                "bl" => {
                    op = IROp::Call;
                    target = ins.op_str.clone();
                }
                "blr" => {
                    op = IROp::Call;
                    target = ins.op_str.clone();
                }
                "b.cond" => {
                    op = IROp::Branch;
                    target = ins.op_str.clone();
                }
                m if m.starts_with("b.") => {
                    op = IROp::Branch;
                    target = ins.op_str.clone();
                }
                "cbz" | "cbnz" | "tbz" | "tbnz" => {
                    op = IROp::Branch;
                    target = ins.op_str.clone();
                }
                "b" => {
                    op = IROp::Jump;
                    target = ins.op_str.clone();
                }
                "ret" => {
                    op = IROp::Return;
                }
                // The register is kept as provenance for the emitters, which
                // report which value control left through. It is never parsed as
                // an address: `parse_target_hex` rejects a register name, and no
                // edge is derived from it.
                "br" => {
                    op = IROp::IndirectBranch;
                    target = ins.op_str.clone();
                }
                "brk" => {
                    op = IROp::Trap;
                }
                "ldr"
                    if ins.annotation.starts_with("pool[")
                        || ins.annotation.starts_with("poolOff[") =>
                {
                    op = IROp::LoadPool;
                    src = ins.op_str.clone();
                    target = ins.annotation.clone();
                }
                _ => {}
            }

            LlirInstr {
                va: ins.va,
                op,
                src,
                target,
            }
        })
        .collect()
}

fn build_function_ir_accounted(d: &FunctionDisassembly) -> (FunctionIr, IrBuildAccounting) {
    let llir = llir_from_disasm(d);
    let mut leaders = BTreeSet::new();
    let mut by_va = BTreeMap::new();

    if let Some(first) = llir.first() {
        leaders.insert(first.va);
    }

    for (idx, ins) in llir.iter().enumerate() {
        by_va.insert(ins.va, idx);
        // Code after any terminator starts a new block. Without this, a
        // trailing runtime-check slow path is absorbed into the returning
        // block and its jump back becomes that block's terminator,
        // resurrecting the edge the check elision removed.
        match ins.op {
            IROp::Branch | IROp::Jump => {
                if let Some(t) = parse_target_hex(&ins.target) {
                    leaders.insert(t);
                }
                if let Some(next) = llir.get(idx + 1) {
                    leaders.insert(next.va);
                }
            }
            // A return, an indirect branch and a trap all end the path. The
            // following instruction is only reached by some other edge, so it
            // needs a leader of its own; without one it is glued onto the
            // terminating block and that block's control effect is replaced by
            // whatever the absorbed code ends with.
            IROp::Return | IROp::IndirectBranch | IROp::Trap => {
                if let Some(next) = llir.get(idx + 1) {
                    leaders.insert(next.va);
                }
            }
            IROp::Call | IROp::LoadPool | IROp::RuntimeCheck | IROp::Other => {}
        }
    }

    let leader_vec: Vec<u64> = leaders.into_iter().collect();
    let mut blocks = Vec::new();

    for (i, start) in leader_vec.iter().enumerate() {
        let end = if i + 1 < leader_vec.len() {
            leader_vec[i + 1]
        } else {
            u64::MAX
        };

        let mut instrs = Vec::new();
        for ins in &llir {
            if ins.va >= *start && ins.va < end {
                instrs.push(ins.clone());
            }
        }

        if !instrs.is_empty() {
            blocks.push(BasicBlock {
                id: blocks.len(),
                start_va: *start,
                instrs,
                succs: Vec::new(),
                preds: Vec::new(),
            });
        }
    }

    let mut start_to_id = BTreeMap::new();
    for b in &blocks {
        start_to_id.insert(b.start_va, b.id);
    }

    for i in 0..blocks.len() {
        let mut succs = Vec::new();
        let last = blocks[i].instrs.last().cloned();

        if let Some(last) = last {
            match last.op {
                IROp::Branch => {
                    if let Some(t) = parse_target_hex(&last.target) {
                        if let Some(id) = start_to_id.get(&t) {
                            succs.push(*id);
                        }
                    }
                    if i + 1 < blocks.len() {
                        succs.push(blocks[i + 1].id);
                    }
                }
                IROp::Jump => {
                    if let Some(t) = parse_target_hex(&last.target) {
                        if let Some(id) = start_to_id.get(&t) {
                            succs.push(*id);
                        }
                    }
                }
                // No successor may be invented here. A return leaves the
                // function, an indirect branch leaves through a value that was
                // not recovered, and a trap leaves through an exception.
                IROp::Return | IROp::IndirectBranch | IROp::Trap => {}
                // A call returns to the next instruction, and the remaining
                // classes are not terminators, so all of them fall through.
                IROp::Call | IROp::LoadPool | IROp::RuntimeCheck | IROp::Other => {
                    if i + 1 < blocks.len() {
                        succs.push(blocks[i + 1].id);
                    }
                }
            }
        }

        // Left exactly as derived, duplicates and all: a conditional whose target
        // is its own fallthrough names one block twice. `rebuild_edges` below is
        // the one place that decides what a canonical edge list looks like.
        blocks[i].succs = succs;
    }

    // Drop only what the runtime-check elision stranded: the guard's slow path,
    // which calls a stub and jumps back into the body. Left in place its jump
    // registers as a predecessor of a body block, fabricating a join and a back
    // edge, so the elision has no effect until it is removed.
    //
    // Deliberately not a blanket reachability prune. Blocks unreachable for
    // other reasons are code the adapter merged in from neighbouring functions
    // (broken function-boundary recovery); deleting those would silently lose
    // real program text. They are reported instead.
    let reach = |blocks: &[BasicBlock], extra: bool| {
        let mut seen = vec![false; blocks.len()];
        let mut stack = Vec::new();
        if !blocks.is_empty() {
            seen[0] = true;
            stack.push(0usize);
        }
        while let Some(i) = stack.pop() {
            let mut targets = blocks[i].succs.clone();
            if extra {
                // The guard edge as it was before elision.
                for ins in &blocks[i].instrs {
                    if ins.op == IROp::RuntimeCheck {
                        if let Some(t) = parse_target_hex(&ins.target) {
                            if let Some(id) = start_to_id.get(&t) {
                                targets.push(*id);
                            }
                        }
                    }
                }
            }
            for s in targets {
                if let Some(next) = blocks.iter().position(|b| b.id == s) {
                    if !seen[next] {
                        seen[next] = true;
                        stack.push(next);
                    }
                }
            }
        }
        seen
    };
    let with_guard = reach(&blocks, true);
    let without_guard = reach(&blocks, false);
    let built = blocks.iter().map(|b| (b.id, b.start_va)).collect();

    let mut remap = BTreeMap::new();
    let mut kept = Vec::with_capacity(blocks.len());
    let mut guard_pruned = Vec::new();
    for (i, b) in blocks.into_iter().enumerate() {
        if with_guard[i] && !without_guard[i] {
            guard_pruned.push((b.id, b.start_va));
            continue;
        }
        remap.insert(b.id, kept.len());
        kept.push(b);
    }
    let mut blocks = kept;
    for (i, b) in blocks.iter_mut().enumerate() {
        b.id = i;
        // Remapped, not dropped: the ids move, the edges do not change. Dropping
        // an edge is `rebuild_edges`'s job and only for a target that no longer
        // exists at all.
        b.succs = b
            .succs
            .iter()
            .filter_map(|s| remap.get(s).copied())
            .collect();
    }
    // The only place edges become canonical, on this path and on every later
    // mutation path: successors sorted and unique, predecessors derived from them
    // in full so the two sides cannot disagree.
    rebuild_edges(&mut blocks);

    let ir = FunctionIr {
        function_id: d.function_id,
        name: d.function_name.clone(),
        entry_va: d.entry_va,
        blocks,
    };
    // The builder is the origin of every graph the pipeline analyses, including
    // after the slow-path prune above has removed blocks and remapped every id,
    // so its own output is held to the ruler its consumers apply. Costs nothing
    // in a release build; fires in every test and every debug run.
    debug_assert_eq!(
        validate_canonical_cfg(&ir),
        Ok(()),
        "the builder produced a graph its consumers cannot index"
    );
    let guard_remaps = ir
        .blocks
        .iter()
        .map(|block| {
            let old_id = remap
                .iter()
                .find_map(|(old, new)| (*new == block.id).then_some(*old))
                .expect("every retained block was remapped");
            (old_id, block.start_va, block.id)
        })
        .collect();
    (
        ir,
        IrBuildAccounting {
            built,
            guard_pruned,
            guard_remaps,
        },
    )
}

pub fn build_function_ir(d: &FunctionDisassembly) -> FunctionIr {
    build_function_ir_accounted(d).0
}

pub fn build_program_ir_with_accounting(
    disasm: &[FunctionDisassembly],
) -> Vec<(FunctionIr, IrBuildAccounting)> {
    disasm.iter().map(build_function_ir_accounted).collect()
}

pub fn build_program_ir(disasm: &[FunctionDisassembly]) -> Vec<FunctionIr> {
    disasm.iter().map(build_function_ir).collect()
}

// The ARM64 control-effect ruler. A separate, digest-protected file rather than
// part of `mod tests` below, because this file is product source that later
// work edits, so a digest over it would fire on legitimate change. This
// declaration is the only thing that compiles that file and cannot be digested
// either, so `scripts/check-oracle-inventory.py` proves it by compilation.
#[cfg(test)]
#[path = "tests/control_effects.rs"]
mod control_effect_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use flutterdec_disasm_arm64::AsmInstruction;

    pub(super) fn ins(va: u64, mnemonic: &str, op_str: &str) -> AsmInstruction {
        AsmInstruction {
            va,
            word: 0,
            mnemonic: mnemonic.to_string(),
            op_str: op_str.to_string(),
            annotation: String::new(),
        }
    }

    /// The Dart stack-overflow guard must not reach the CFG: its taken edge is
    /// runtime bookkeeping, and its slow path jumps back into the body, so an
    /// edge for it fabricates a conditional, a call and a back edge.
    #[test]
    fn elides_the_dart_stack_overflow_check_and_its_slow_path() {
        let d = FunctionDisassembly {
            function_id: 7,
            function_name: "guarded".to_string(),
            owner_class: "Global".to_string(),
            entry_va: 0x1000,
            size: 24,
            instructions: vec![
                ins(0x1000, "ldr", "x16, [x26, #0x38]"),
                ins(0x1004, "cmp", "x15, x16"),
                ins(0x1008, "b.ls", "#0x1014"),
                ins(0x100c, "mov", "x0, x1"),
                ins(0x1010, "ret", ""),
                // Slow path: call the stub, then re-enter the body.
                ins(0x1014, "bl", "#0x9000"),
                ins(0x1018, "b", "#0x100c"),
            ],
        };

        let ir = build_function_ir(&d);

        let ops: Vec<&IROp> = ir
            .blocks
            .iter()
            .flat_map(|b| b.instrs.iter().map(|i| &i.op))
            .collect();
        assert_eq!(
            ops.iter().filter(|op| ***op == IROp::RuntimeCheck).count(),
            3,
            "the whole three-instruction group is the guard"
        );
        assert!(
            !ops.contains(&&IROp::Call),
            "the stub call is on the elided slow path: {ops:?}"
        );
        assert!(
            ir.blocks.iter().all(|b| b.succs.len() <= 1),
            "no block should branch two ways: {:?}",
            ir.blocks.iter().map(|b| &b.succs).collect::<Vec<_>>()
        );
        assert!(
            ir.blocks.iter().all(|b| b.preds.len() <= 1),
            "the slow path's jump back must not survive as a predecessor: {:?}",
            ir.blocks.iter().map(|b| &b.preds).collect::<Vec<_>>()
        );
    }

    /// `Thread::stack_limit_offset` moves between SDK releases: 0x38 through Dart
    /// 3.5, 0x48 by 3.12, both observed in real binaries. The scratch register is
    /// TMP or TMP2. Recognition keys on the shape, so none of that matters, and
    /// this pins it: an offset-keyed matcher would silently recover nothing after
    /// a version bump.
    #[test]
    fn recognises_the_guard_across_sdk_offsets_and_scratch_registers() {
        for scratch in ["x16", "x17"] {
            for offset in ["0x38", "0x48", "0x60"] {
                let d = FunctionDisassembly {
                    function_id: 9,
                    function_name: "guarded".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1000,
                    size: 20,
                    instructions: vec![
                        ins(0x1000, "ldr", &format!("{scratch}, [x26, #{offset}]")),
                        ins(0x1004, "cmp", &format!("x15, {scratch}")),
                        ins(0x1008, "b.ls", "#0x1014"),
                        ins(0x100c, "mov", "x0, x1"),
                        ins(0x1010, "ret", ""),
                        ins(0x1014, "bl", "#0x9000"),
                        ins(0x1018, "b", "#0x100c"),
                    ],
                };
                let ir = build_function_ir(&d);
                let ops: Vec<&IROp> = ir
                    .blocks
                    .iter()
                    .flat_map(|b| b.instrs.iter().map(|i| &i.op))
                    .collect();
                assert_eq!(
                    ops.iter().filter(|op| ***op == IROp::RuntimeCheck).count(),
                    3,
                    "guard with {scratch} and offset {offset} should be recognised"
                );
                assert!(
                    !ops.contains(&&IROp::Call),
                    "slow path should be elided for {scratch} at {offset}"
                );
            }
        }
    }

    /// A compare of the Dart stack pointer against something that is not a THR
    /// field is not the guard.
    #[test]
    fn does_not_treat_an_unrelated_stack_compare_as_the_guard() {
        let d = FunctionDisassembly {
            function_id: 10,
            function_name: "notGuarded".to_string(),
            owner_class: "Global".to_string(),
            entry_va: 0x1000,
            size: 12,
            instructions: vec![
                // Loaded from the object pool, not from THR.
                ins(0x1000, "ldr", "x16, [x27, #0x38]"),
                ins(0x1004, "cmp", "x15, x16"),
                ins(0x1008, "b.ls", "#0x1010"),
                ins(0x100c, "ret", ""),
                ins(0x1010, "ret", ""),
            ],
        };
        let ir = build_function_ir(&d);
        assert!(
            !ir.blocks
                .iter()
                .any(|b| b.instrs.iter().any(|i| i.op == IROp::RuntimeCheck)),
            "only a THR field load is the guard"
        );
    }

    /// A `ret` ends a basic block. Without that, a trailing slow path is glued
    /// onto the returning block and its jump becomes that block's terminator.
    #[test]
    fn code_after_a_return_starts_a_new_block() {
        let d = FunctionDisassembly {
            function_id: 8,
            function_name: "two_exits".to_string(),
            owner_class: "Global".to_string(),
            entry_va: 0x2000,
            size: 12,
            instructions: vec![
                ins(0x2000, "ret", ""),
                ins(0x2004, "mov", "x0, x1"),
                ins(0x2008, "ret", ""),
            ],
        };

        let ir = build_function_ir(&d);
        assert_eq!(ir.blocks.len(), 2);
        assert_eq!(ir.blocks[0].instrs.len(), 1);
    }

    #[test]
    fn builds_cfg_with_branch_and_fallthrough() {
        let d = FunctionDisassembly {
            function_id: 1,
            function_name: "f".to_string(),
            owner_class: "Global".to_string(),
            entry_va: 0x1000,
            size: 16,
            instructions: vec![
                AsmInstruction {
                    va: 0x1000,
                    word: 0,
                    mnemonic: "b.cond".to_string(),
                    op_str: "#0x1008".to_string(),
                    annotation: "branch".to_string(),
                },
                AsmInstruction {
                    va: 0x1004,
                    word: 0,
                    mnemonic: "ret".to_string(),
                    op_str: String::new(),
                    annotation: "return".to_string(),
                },
                AsmInstruction {
                    va: 0x1008,
                    word: 0,
                    mnemonic: "ret".to_string(),
                    op_str: String::new(),
                    annotation: "return".to_string(),
                },
            ],
        };

        let ir = build_function_ir(&d);
        assert_eq!(ir.blocks.len(), 3);
        assert_eq!(ir.blocks[0].succs, vec![1, 2]);
    }

    #[test]
    fn parses_tbnz_target_from_last_operand_token() {
        let d = FunctionDisassembly {
            function_id: 2,
            function_name: "g".to_string(),
            owner_class: "Global".to_string(),
            entry_va: 0x2000,
            size: 16,
            instructions: vec![
                AsmInstruction {
                    va: 0x2000,
                    word: 0,
                    mnemonic: "tbnz".to_string(),
                    op_str: "x0, #0x3f, #0x2008".to_string(),
                    annotation: "branch".to_string(),
                },
                AsmInstruction {
                    va: 0x2004,
                    word: 0,
                    mnemonic: "ret".to_string(),
                    op_str: String::new(),
                    annotation: "return".to_string(),
                },
                AsmInstruction {
                    va: 0x2008,
                    word: 0,
                    mnemonic: "ret".to_string(),
                    op_str: String::new(),
                    annotation: "return".to_string(),
                },
            ],
        };

        let ir = build_function_ir(&d);
        assert_eq!(ir.blocks.len(), 3);
        assert_eq!(ir.blocks[0].succs, vec![1, 2]);
    }
}
