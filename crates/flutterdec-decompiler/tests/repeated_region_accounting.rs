//! Regression coverage for repeated structured-region accounting.
//!
//! The structured walk may emit a shared region more than once. The public
//! counter must record each extra copy at the same point that the copy is
//! recorded, in debug and optimized builds alike.

use flutterdec_decompiler::emit_pseudocode;
use flutterdec_ir::{rebuild_edges, BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::HashMap;

fn block_va(id: usize) -> u64 {
    0x1000 + 0x10 * id as u64
}

fn marker_va(id: usize) -> u64 {
    0x9000 + 0x10 * id as u64
}

fn instr(va: u64, op: IROp, src: String, target: String) -> LlirInstr {
    LlirInstr {
        va,
        op,
        src,
        target,
    }
}

fn repeated_region() -> FunctionIr {
    let successors: &[&[usize]] = &[&[1, 2], &[3, 4], &[3, 5], &[], &[6], &[6], &[]];
    let blocks = successors
        .iter()
        .enumerate()
        .map(|(id, succs)| {
            let start = block_va(id);
            let mut instrs = vec![instr(
                start,
                IROp::Call,
                format!("bl #{:#x}", marker_va(id)),
                format!("#{:#x}", marker_va(id)),
            )];
            let end = start + 4;
            match *succs {
                [] => instrs.push(instr(end, IROp::Return, "ret".into(), String::new())),
                [only] => instrs.push(instr(
                    end,
                    IROp::Jump,
                    format!("b #{:#x}", block_va(*only)),
                    format!("#{:#x}", block_va(*only)),
                )),
                [_fallthrough, taken] => instrs.push(instr(
                    end,
                    IROp::Branch,
                    format!("cbz x0, #{:#x}", block_va(*taken)),
                    format!("#{:#x}", block_va(*taken)),
                )),
                _ => unreachable!("fixture blocks have at most two successors"),
            }
            BasicBlock {
                id,
                start_va: start,
                instrs,
                succs: succs.to_vec(),
                preds: Vec::new(),
            }
        })
        .collect();
    let mut ir = FunctionIr {
        function_id: 7000,
        name: "repeated_region_accounting".into(),
        entry_va: block_va(0),
        blocks,
    };
    rebuild_edges(&mut ir.blocks);
    ir
}

#[test]
fn public_artifact_counts_every_repeated_region_copy() {
    let symbols = (0..7)
        .map(|id| (marker_va(id), format!("mark{id}")))
        .collect::<HashMap<_, _>>();
    let artifact = emit_pseudocode(&repeated_region(), &symbols);
    let copies = (0..7)
        .map(|id| {
            artifact
                .source
                .lines()
                .filter(|line| line.contains(&format!("mark{id}()")))
                .count()
        })
        .collect::<Vec<_>>();

    assert_eq!(copies, [1, 1, 1, 2, 1, 1, 2]);
    assert_eq!(artifact.repeated_blocks, 2);
    assert_eq!(
        artifact.repeated_blocks,
        copies.iter().map(|count| count - 1).sum::<usize>()
    );
}
