use flutterdec_disasm_arm64::{AsmInstruction, FunctionDisassembly};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum IROp {
    Call,
    Branch,
    Jump,
    Return,
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
            IROp::Return => {
                if let Some(next) = llir.get(idx + 1) {
                    leaders.insert(next.va);
                }
            }
            _ => {}
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
                IROp::Return => {}
                _ => {
                    if i + 1 < blocks.len() {
                        succs.push(blocks[i + 1].id);
                    }
                }
            }
        }

        succs.sort_unstable();
        succs.dedup();
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
        b.succs = b
            .succs
            .iter()
            .filter_map(|s| remap.get(s).copied())
            .collect();
    }

    for i in 0..blocks.len() {
        let succs = blocks[i].succs.clone();
        let pred_id = blocks[i].id;
        for s in succs {
            if let Some(target) = blocks.iter_mut().find(|b| b.id == s) {
                target.preds.push(pred_id);
            }
        }
    }

    for b in &mut blocks {
        b.preds.sort_unstable();
        b.preds.dedup();
    }

    FunctionIr {
        function_id: d.function_id,
        name: d.function_name.clone(),
        entry_va: d.entry_va,
        blocks,
    }
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
