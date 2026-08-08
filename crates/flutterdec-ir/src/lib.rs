use flutterdec_disasm_arm64::FunctionDisassembly;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum IROp {
    Call,
    Branch,
    Jump,
    Return,
    LoadPool,
    Other,
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
    d.instructions
        .iter()
        .map(|ins| {
            let mut op = IROp::Other;
            let mut src = if ins.op_str.is_empty() {
                ins.mnemonic.clone()
            } else {
                format!("{} {}", ins.mnemonic, ins.op_str)
            };
            let mut target = String::new();

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
        match ins.op {
            IROp::Branch | IROp::Jump => {
                if let Some(t) = parse_target_hex(&ins.target) {
                    leaders.insert(t);
                }
                if ins.op == IROp::Branch {
                    if let Some(next) = llir.get(idx + 1) {
                        leaders.insert(next.va);
                    }
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
