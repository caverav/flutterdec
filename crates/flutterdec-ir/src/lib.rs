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

pub fn build_function_ir(d: &FunctionDisassembly) -> FunctionIr {
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

    let mut remap = BTreeMap::new();
    let mut kept = Vec::with_capacity(blocks.len());
    for (i, b) in blocks.into_iter().enumerate() {
        if with_guard[i] && !without_guard[i] {
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
    ir
}

pub fn build_program_ir(disasm: &[FunctionDisassembly]) -> Vec<FunctionIr> {
    disasm.iter().map(build_function_ir).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flutterdec_disasm_arm64::AsmInstruction;

    fn ins(va: u64, mnemonic: &str, op_str: &str) -> AsmInstruction {
        AsmInstruction {
            va,
            word: 0,
            mnemonic: mnemonic.to_string(),
            op_str: op_str.to_string(),
            annotation: String::new(),
        }
    }

    /// One row of the ARM64 control-effect table in section 3 of
    /// `docs/oracle-protocol-ir-cfg-emitter.md`, written from `DDI 0487` C6.2.
    struct ControlEffect {
        mnemonic: &'static str,
        op_str: &'static str,
        op: IROp,
        /// Whether the instruction that follows must become a leader.
        ends_block: bool,
        /// Start addresses of the successors of the block this instruction
        /// ends, ascending. Empty means no edge at all, which is what makes an
        /// invented fallthrough or a guessed target visible.
        succ_starts: &'static [u64],
    }

    /// The whole table, so a new control effect cannot be added without a row.
    /// `0x1008` is the fallthrough block and `0x2000` the branch target block in
    /// the fixture below.
    const CONTROL_EFFECTS: &[ControlEffect] = &[
        ControlEffect {
            mnemonic: "b",
            op_str: "#0x2000",
            op: IROp::Jump,
            ends_block: true,
            succ_starts: &[0x2000],
        },
        ControlEffect {
            mnemonic: "br",
            op_str: "x16",
            op: IROp::IndirectBranch,
            ends_block: true,
            succ_starts: &[],
        },
        ControlEffect {
            mnemonic: "ret",
            op_str: "",
            op: IROp::Return,
            ends_block: true,
            succ_starts: &[],
        },
        ControlEffect {
            mnemonic: "brk",
            op_str: "#0x1",
            op: IROp::Trap,
            ends_block: true,
            succ_starts: &[],
        },
        ControlEffect {
            mnemonic: "b.eq",
            op_str: "#0x2000",
            op: IROp::Branch,
            ends_block: true,
            succ_starts: &[0x1008, 0x2000],
        },
        ControlEffect {
            mnemonic: "cbz",
            op_str: "x0, #0x2000",
            op: IROp::Branch,
            ends_block: true,
            succ_starts: &[0x1008, 0x2000],
        },
        ControlEffect {
            mnemonic: "cbnz",
            op_str: "x0, #0x2000",
            op: IROp::Branch,
            ends_block: true,
            succ_starts: &[0x1008, 0x2000],
        },
        ControlEffect {
            mnemonic: "tbz",
            op_str: "x0, #0x3, #0x2000",
            op: IROp::Branch,
            ends_block: true,
            succ_starts: &[0x1008, 0x2000],
        },
        ControlEffect {
            mnemonic: "tbnz",
            op_str: "x0, #0x3, #0x2000",
            op: IROp::Branch,
            ends_block: true,
            succ_starts: &[0x1008, 0x2000],
        },
        // A call returns, so it keeps its fallthrough and takes no edge to the
        // callee even when the callee is a block of this same function.
        ControlEffect {
            mnemonic: "bl",
            op_str: "#0x2000",
            op: IROp::Call,
            ends_block: false,
            succ_starts: &[0x1008],
        },
        ControlEffect {
            mnemonic: "blr",
            op_str: "x16",
            op: IROp::Call,
            ends_block: false,
            succ_starts: &[0x1008],
        },
        // Control row: an ordinary instruction is the only thing that may take
        // the fallthrough by default.
        ControlEffect {
            mnemonic: "mov",
            op_str: "x0, x1",
            op: IROp::Other,
            ends_block: false,
            succ_starts: &[0x1008],
        },
    ];

    /// The instruction under test at `0x1004`, with instructions following it,
    /// reached from a block that also makes `0x1008` a leader. The successor set
    /// of the middle block is then exactly the instruction's control effect.
    fn control_effect_fixture(row: &ControlEffect) -> FunctionDisassembly {
        FunctionDisassembly {
            function_id: 42,
            function_name: "effect".to_string(),
            owner_class: "Global".to_string(),
            entry_va: 0x1000,
            size: 0x1010,
            instructions: vec![
                ins(0x1000, "cbz", "x9, #0x1008"),
                ins(0x1004, row.mnemonic, row.op_str),
                ins(0x1008, "mov", "x0, x1"),
                ins(0x100c, "b", "#0x2000"),
                ins(0x2000, "ret", ""),
            ],
        }
    }

    #[test]
    fn every_arm64_control_effect_has_exactly_the_documented_edges() {
        for row in CONTROL_EFFECTS {
            let ir = build_function_ir(&control_effect_fixture(row));
            let start_of: BTreeMap<usize, u64> =
                ir.blocks.iter().map(|b| (b.id, b.start_va)).collect();
            let under_test = ir
                .blocks
                .iter()
                .find(|b| b.start_va == 0x1004)
                .unwrap_or_else(|| panic!("{} should start a block of its own", row.mnemonic));

            assert_eq!(
                under_test.instrs.first().map(|i| &i.op),
                Some(&row.op),
                "{} {} should classify as {:?}",
                row.mnemonic,
                row.op_str,
                row.op
            );

            let succs: Vec<u64> = under_test
                .succs
                .iter()
                .map(|s| start_of[s])
                .collect::<BTreeSet<u64>>()
                .into_iter()
                .collect();
            assert_eq!(
                succs,
                row.succ_starts.to_vec(),
                "{} {} has the wrong edges: {:?}",
                row.mnemonic,
                row.op_str,
                under_test.succs
            );

            // I1 through I4: nothing above may be read off a malformed graph.
            for b in &ir.blocks {
                let mut sorted = b.succs.clone();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(sorted, b.succs, "successors must be sorted and unique");
                let mut sorted = b.preds.clone();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(sorted, b.preds, "predecessors must be sorted and unique");
                for s in &b.succs {
                    assert!(
                        ir.blocks[*s].preds.contains(&b.id),
                        "{} {}: edge {} -> {} is missing its predecessor",
                        row.mnemonic,
                        row.op_str,
                        b.id,
                        s
                    );
                }
                for p in &b.preds {
                    assert!(
                        ir.blocks[*p].succs.contains(&b.id),
                        "{} {}: predecessor {} of {} has no matching edge",
                        row.mnemonic,
                        row.op_str,
                        p,
                        b.id
                    );
                }
            }
            for (i, b) in ir.blocks.iter().enumerate() {
                assert_eq!(b.id, i, "block ids are exactly 0..len");
            }
        }
    }

    #[test]
    fn only_a_control_effect_that_ends_a_block_makes_the_next_instruction_a_leader() {
        for row in CONTROL_EFFECTS {
            let d = FunctionDisassembly {
                function_id: 43,
                function_name: "ender".to_string(),
                owner_class: "Global".to_string(),
                entry_va: 0x1000,
                size: 12,
                instructions: vec![
                    ins(0x1000, row.mnemonic, row.op_str),
                    ins(0x1004, "mov", "x0, x1"),
                    ins(0x1008, "ret", ""),
                ],
            };
            let ir = build_function_ir(&d);
            let expected_blocks = if row.ends_block { 2 } else { 1 };
            assert_eq!(
                ir.blocks.len(),
                expected_blocks,
                "{} {} should {} end its block: {:?}",
                row.mnemonic,
                row.op_str,
                if row.ends_block { "" } else { "not " },
                ir.blocks
                    .iter()
                    .map(|b| (b.start_va, b.instrs.len()))
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                ir.blocks[0].instrs.len(),
                if row.ends_block { 1 } else { 3 },
                "{} {} left the wrong instructions in the entry block",
                row.mnemonic,
                row.op_str
            );
        }
    }

    /// `br` is how a dispatch stub or a tail call leaves a function. The register
    /// it leaves through is kept as provenance, and no edge is derived from it in
    /// either direction: not a fallthrough, and not a guessed target.
    #[test]
    fn an_indirect_branch_keeps_its_register_and_takes_no_edge() {
        let d = FunctionDisassembly {
            function_id: 44,
            function_name: "dispatch".to_string(),
            owner_class: "Global".to_string(),
            entry_va: 0x1000,
            size: 12,
            instructions: vec![
                ins(0x1000, "ldur", "x16, [x24, #7]"),
                ins(0x1004, "br", "x16"),
                // Only reachable because something else branches here, which
                // nothing in this function does.
                ins(0x1008, "ret", ""),
            ],
        };
        let ir = build_function_ir(&d);
        let tail = ir.blocks[0].instrs.last().expect("a tail instruction");
        assert_eq!(tail.op, IROp::IndirectBranch);
        assert_eq!(tail.target, "x16", "the register is kept as provenance");
        assert!(
            ir.blocks[0].succs.is_empty(),
            "an indirect branch invents no edge: {:?}",
            ir.blocks[0].succs
        );
        assert!(
            ir.blocks[1].preds.is_empty(),
            "the instruction after it is not fallen into: {:?}",
            ir.blocks[1].preds
        );
    }

    /// `brk` raises; control does not continue past it. Dart AOT ends every
    /// raising stub with it, so a fabricated fallthrough there attaches the next
    /// stub's body to a path that never reaches it.
    #[test]
    fn a_trap_ends_the_block_with_no_successors() {
        let d = FunctionDisassembly {
            function_id: 45,
            function_name: "raise".to_string(),
            owner_class: "Global".to_string(),
            entry_va: 0x1000,
            size: 12,
            instructions: vec![
                ins(0x1000, "brk", "#0x1"),
                ins(0x1004, "mov", "x0, x1"),
                ins(0x1008, "ret", ""),
            ],
        };
        let ir = build_function_ir(&d);
        assert_eq!(ir.blocks[0].instrs[0].op, IROp::Trap);
        assert!(
            ir.blocks[0].succs.is_empty(),
            "a trap has no successors: {:?}",
            ir.blocks[0].succs
        );
        assert!(
            ir.blocks[1].preds.is_empty(),
            "the code after a trap is not fallen into: {:?}",
            ir.blocks[1].preds
        );
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
