//! Synthetic ARM64 function bodies with known control-flow shape.
//!
//! Two generators live here. The disclosed matrix is fixed by the metric
//! contract and is what the baseline and every candidate are measured on. The
//! held-out generator is a distribution, frozen in the same commit and drawn
//! from only after the candidate is immutable, so a candidate cannot be tuned
//! to the cases it will be scored on.
//!
//! Both emit real disassembly text, not a pre-built `FunctionIr`: the IR span
//! has to start at `build_function_ir`, so the workload must be expressed in
//! the only thing that function accepts.

use crate::rng::Rng;
use crate::sha256::Sha256;
use flutterdec_disasm_arm64::{AsmInstruction, FunctionDisassembly};

/// Fixed instructions per block before any added load: four register
/// operations, one pool load, one direct call, one unmodelled write, one
/// comparison, then the terminator.
pub const DISCLOSED_BODY: usize = 8;

/// The held-out mix is eight instructions per block: three register
/// operations, one pool load, one direct call, one unmodelled write, one
/// comparison, then the terminator.
pub const HELD_OUT_BODY: usize = 7;

pub const DISCLOSED_SIZES: [usize; 3] = [64, 256, 1024];
pub const DISCLOSED_TOPOLOGIES: [&str; 7] = [
    "linear",
    "diamond-chain",
    "fan-in",
    "nested-loop",
    "multi-exit",
    "no-exit",
    "irreducible",
];
/// Only linear and diamond-chain carry the added instruction loads. The other
/// five shapes exist to move the analysis cost, not the per-instruction cost.
pub const LOAD_TOPOLOGIES: [&str; 2] = ["linear", "diamond-chain"];
pub const LOADS: [(&str, usize); 3] = [("base", 0), ("light", 2), ("heavy", 32)];

const ENTRY_VA: u64 = 0x0010_0000;
/// Well clear of the highest block address any case reaches, so a call target
/// can never be mistaken for a block leader.
const CALL_BASE_VA: u64 = 0x0090_0000;

/// What ends a block, in block indices rather than addresses. Addresses are
/// resolved after the layout is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Term {
    /// Conditional branch: taken edge to the target, fall-through to the next
    /// block.
    Cond(usize),
    Jump(usize),
    Ret,
}

pub struct Case {
    pub name: String,
    pub topology: String,
    pub blocks: usize,
    pub load: String,
    pub instructions_per_block: usize,
    pub disasm: FunctionDisassembly,
    /// The edges the generator built, sorted and deduplicated the way
    /// `build_function_ir` reports them. Derived from the terminator list, not
    /// from the IR, so it is an independent expectation rather than a restated
    /// observation.
    pub expected_succs: Vec<Vec<usize>>,
    pub workload_sha256: String,
}

fn reg(block: usize, slot: usize) -> u32 {
    // x1 through x12. Everything the guard matcher and the pool base use
    // (x15, x16, x17, x26, x27) is outside this range, so no generated block
    // can accidentally look like the Dart stack-overflow check.
    1 + ((block * 5 + slot * 3) % 12) as u32
}

fn instruction(va: u64, mnemonic: &str, op_str: &str, annotation: &str) -> AsmInstruction {
    AsmInstruction {
        va,
        // Zero throughout: nothing downstream of `build_function_ir` reads the
        // encoded word, and a fabricated encoding would be a claim the harness
        // cannot back.
        word: 0,
        mnemonic: mnemonic.to_string(),
        op_str: op_str.to_string(),
        annotation: annotation.to_string(),
    }
}

/// One block's instructions. `register_ops` is three for the held-out mix and
/// four for the disclosed one; `extra` is the added instruction load.
fn block_body(
    va: u64,
    index: usize,
    register_ops: usize,
    extra: usize,
    out: &mut Vec<AsmInstruction>,
) -> u64 {
    let mut va = va;
    let mut push = |mnemonic: &str, op_str: String, annotation: &str, va: &mut u64| {
        out.push(instruction(*va, mnemonic, &op_str, annotation));
        *va += 4;
    };

    for slot in 0..register_ops {
        let (mnemonic, ops) = match slot % 4 {
            0 => ("mov", format!("x{}, x{}", reg(index, 0), reg(index, 1))),
            1 => (
                "add",
                format!("x{}, x{}, x{}", reg(index, 2), reg(index, 3), reg(index, 4)),
            ),
            2 => (
                "sub",
                format!("x{}, x{}, x{}", reg(index, 5), reg(index, 6), reg(index, 7)),
            ),
            _ => (
                "orr",
                format!(
                    "x{}, x{}, x{}",
                    reg(index, 8),
                    reg(index, 9),
                    reg(index, 10)
                ),
            ),
        };
        push(mnemonic, ops, "", &mut va);
    }

    // The added load sits here so the tail the contract fixes - pool load,
    // direct call, unmodelled write, comparison, terminator - stays in order,
    // and the comparison stays adjacent to the branch that reads it.
    for slot in 0..extra {
        push(
            if slot % 2 == 0 { "mov" } else { "add" },
            format!(
                "x{}, x{}, x{}",
                reg(index + slot, 0),
                reg(index + slot, 1),
                reg(index + slot, 2)
            ),
            "",
            &mut va,
        );
    }

    let pool_slot = index % 32;
    push(
        "ldr",
        format!("x3, [x27, #0x{:x}]", 0x40 + pool_slot * 8),
        &format!("pool[{pool_slot}]"),
        &mut va,
    );
    push(
        "bl",
        format!("#0x{:x}", CALL_BASE_VA + ((index % 97) * 4) as u64),
        "",
        &mut va,
    );
    push(
        "str",
        format!("x4, [x5, #0x{:x}]", 0x8 + (index % 16) * 8),
        "",
        &mut va,
    );
    push("cmp", "x6, x7".to_string(), "", &mut va);
    va
}

/// Successors as `build_function_ir` reports them: taken edge first if it is
/// lower, sorted and deduplicated, and no fall-through off the end of the
/// function.
fn expected_succs(terms: &[Term]) -> Vec<Vec<usize>> {
    let n = terms.len();
    terms
        .iter()
        .enumerate()
        .map(|(i, term)| {
            let mut succs = match *term {
                Term::Cond(target) => {
                    let mut succs = vec![target];
                    if i + 1 < n {
                        succs.push(i + 1);
                    }
                    succs
                }
                Term::Jump(target) => vec![target],
                Term::Ret => Vec::new(),
            };
            succs.sort_unstable();
            succs.dedup();
            succs
        })
        .collect()
}

/// `body` is the instruction count before the terminator, of which four are
/// always the pool load, direct call, unmodelled write and comparison; the rest
/// are register operations.
fn build_case(
    name: String,
    topology: String,
    load: String,
    terms: &[Term],
    body: usize,
    extra: usize,
) -> Case {
    let register_ops = body - 4;
    let blocks = terms.len();
    let per_block = body + 1 + extra;
    let block_va = |i: usize| ENTRY_VA + (i * per_block * 4) as u64;

    let mut instructions = Vec::with_capacity(blocks * per_block);
    for (i, term) in terms.iter().enumerate() {
        let va = block_body(block_va(i), i, register_ops, extra, &mut instructions);
        match *term {
            Term::Cond(target) => instructions.push(instruction(
                va,
                "b.eq",
                &format!("#0x{:x}", block_va(target)),
                "",
            )),
            Term::Jump(target) => instructions.push(instruction(
                va,
                "b",
                &format!("#0x{:x}", block_va(target)),
                "",
            )),
            Term::Ret => instructions.push(instruction(va, "ret", "", "")),
        }
    }
    assert_eq!(instructions.len(), blocks * per_block, "layout is uniform");

    let disasm = FunctionDisassembly {
        function_id: 1,
        function_name: name.clone(),
        owner_class: "Global".to_string(),
        entry_va: ENTRY_VA,
        size: (instructions.len() * 4) as u64,
        instructions,
    };

    let expected = expected_succs(terms);
    let workload_sha256 = digest_case(&name, &topology, &load, &disasm, &expected);

    Case {
        name,
        topology,
        blocks,
        load,
        instructions_per_block: per_block,
        disasm,
        expected_succs: expected,
        workload_sha256,
    }
}

/// Binds the case identity, every instruction and the expected edge set into
/// one digest. A candidate run whose workload digest differs from the
/// reference's was not measured on the same work, whatever its numbers say.
fn digest_case(
    name: &str,
    topology: &str,
    load: &str,
    disasm: &FunctionDisassembly,
    expected: &[Vec<usize>],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"flutterdec-bench/workload/v1\n");
    for field in [name, topology, load] {
        hasher.update(field.as_bytes());
        hasher.update(b"\n");
    }
    hasher.update(&disasm.entry_va.to_be_bytes());
    for instruction in &disasm.instructions {
        hasher.update(&instruction.va.to_be_bytes());
        hasher.update(instruction.mnemonic.as_bytes());
        hasher.update(b" ");
        hasher.update(instruction.op_str.as_bytes());
        hasher.update(b" ");
        hasher.update(instruction.annotation.as_bytes());
        hasher.update(b"\n");
    }
    for (i, succs) in expected.iter().enumerate() {
        hasher.update(format!("{i}:{succs:?}\n").as_bytes());
    }
    crate::sha256::hex(&hasher.finish())
}

fn linear(n: usize) -> Vec<Term> {
    (0..n)
        .map(|i| {
            if i + 1 < n {
                Term::Jump(i + 1)
            } else {
                Term::Ret
            }
        })
        .collect()
}

/// Groups of four: head, then-arm, else-arm, join. The join is the next
/// diamond's head, so the conditionals chain rather than nest.
fn diamond_chain(n: usize) -> Vec<Term> {
    assert!(
        n.is_multiple_of(4),
        "diamond-chain needs a multiple of four blocks"
    );
    let mut terms = Vec::with_capacity(n);
    for base in (0..n).step_by(4) {
        terms.push(Term::Cond(base + 2));
        terms.push(Term::Jump(base + 3));
        terms.push(Term::Jump(base + 3));
        terms.push(if base + 4 < n {
            Term::Jump(base + 4)
        } else {
            Term::Ret
        });
    }
    terms
}

/// Every block but the last reaches one sink, which ends with `n - 1`
/// predecessors. This is the shape that makes an intersection-based dominator
/// solver do the most work per iteration.
fn fan_in(n: usize) -> Vec<Term> {
    assert!(n >= 3, "fan-in needs a spine and a sink");
    let sink = n - 1;
    (0..n)
        .map(|i| {
            if i == sink {
                Term::Ret
            } else if i + 1 == sink {
                Term::Jump(sink)
            } else {
                Term::Cond(sink)
            }
        })
        .collect()
}

/// One two-deep nest spanning the whole function: outer header at 0, inner
/// header at 1, inner back edge from `n - 3`, outer back edge from `n - 2`,
/// single exit at `n - 1`.
fn nested_loop(n: usize) -> Vec<Term> {
    assert!(n >= 6, "nested-loop needs room for two headers and a latch");
    let mut terms = Vec::with_capacity(n);
    terms.push(Term::Cond(n - 1));
    terms.push(Term::Cond(n - 2));
    for i in 2..n - 3 {
        terms.push(Term::Jump(i + 1));
    }
    terms.push(Term::Jump(1));
    terms.push(Term::Jump(0));
    terms.push(Term::Ret);
    assert_eq!(terms.len(), n);
    terms
}

/// One loop with four distinct exit blocks, which is the case the follow-node
/// rule has to resolve through the header's post-dominator rather than through
/// a single leaving edge.
fn multi_exit(n: usize) -> Vec<Term> {
    assert!(n >= 8, "multi-exit needs a body and four exits");
    let first_exit = n - 4;
    let latch = n - 5;
    (0..n)
        .map(|i| {
            if i >= first_exit {
                Term::Ret
            } else if i == latch {
                Term::Jump(0)
            } else {
                Term::Cond(first_exit + (i % 4))
            }
        })
        .collect()
}

/// A cycle with no reachable return at all, so post-dominance has no exit to
/// anchor on.
fn no_exit(n: usize) -> Vec<Term> {
    assert!(n >= 2, "no-exit needs a cycle");
    (0..n)
        .map(|i| Term::Jump(if i + 1 < n { i + 1 } else { 0 }))
        .collect()
}

/// Groups of four holding a two-node cycle entered from both sides, which no
/// node in the cycle dominates. Region analysis declines these, so the case
/// measures the cost of reaching that decision plus the DFS fallback.
fn irreducible(n: usize) -> Vec<Term> {
    assert!(
        n.is_multiple_of(4),
        "irreducible needs a multiple of four blocks"
    );
    let mut terms = Vec::with_capacity(n);
    for base in (0..n).step_by(4) {
        terms.push(Term::Cond(base + 2));
        terms.push(Term::Cond(base + 3));
        terms.push(Term::Cond(base + 1));
        terms.push(if base + 4 < n {
            Term::Jump(base + 4)
        } else {
            Term::Ret
        });
    }
    terms
}

pub fn topology_terms(topology: &str, blocks: usize) -> Vec<Term> {
    match topology {
        "linear" => linear(blocks),
        "diamond-chain" => diamond_chain(blocks),
        "fan-in" => fan_in(blocks),
        "nested-loop" => nested_loop(blocks),
        "multi-exit" => multi_exit(blocks),
        "no-exit" => no_exit(blocks),
        "irreducible" => irreducible(blocks),
        other => panic!("unknown topology {other}"),
    }
}

/// The full disclosed matrix. Every topology at every size, plus the two added
/// instruction loads on linear and diamond-chain. The contract forbids skipping
/// a pair, so this is built by iteration rather than by a hand-written list.
pub fn disclosed_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for topology in DISCLOSED_TOPOLOGIES {
        for blocks in DISCLOSED_SIZES {
            let terms = topology_terms(topology, blocks);
            for (load, extra) in LOADS {
                if extra != 0 && !LOAD_TOPOLOGIES.contains(&topology) {
                    continue;
                }
                cases.push(build_case(
                    format!("{topology}/{blocks}/{load}"),
                    topology.to_string(),
                    load.to_string(),
                    &terms,
                    DISCLOSED_BODY,
                    extra,
                ));
            }
        }
    }
    cases
}

/// Block sizes the held-out draw may pick: 96 through 2048, with the disclosed
/// sizes removed so a held-out case can never coincide with a tuned one.
pub fn held_out_sizes() -> Vec<usize> {
    (96..=2048)
        .filter(|n| !DISCLOSED_SIZES.contains(n))
        .collect()
}

pub const HELD_OUT_CASES: usize = 6;
const WINDOW: usize = 64;

/// Six mixed-topology cases drawn from the frozen distribution.
///
/// Each 64-block window carries linear edges, at least one diamond, one back
/// edge, two exits and one cross edge, at positions taken from the seed. A
/// trailing partial window is folded into the last full one rather than left
/// short, because a 5-block window cannot hold all five required features and
/// silently dropping them would make the tail of a large case structurally
/// different from its head.
pub fn held_out_cases(seed: u128) -> Vec<Case> {
    let mut rng = Rng::from_seed(seed);
    let sizes = held_out_sizes();
    let mut cases = Vec::with_capacity(HELD_OUT_CASES);
    for index in 0..HELD_OUT_CASES {
        let blocks = sizes[rng.below(sizes.len() as u64) as usize];
        let terms = held_out_terms(&mut rng, blocks);
        cases.push(build_case(
            format!("held-out/{index}/{blocks}"),
            "mixed".to_string(),
            "held-out".to_string(),
            &terms,
            HELD_OUT_BODY,
            0,
        ));
    }
    cases
}

fn held_out_terms(rng: &mut Rng, n: usize) -> Vec<Term> {
    assert!(
        n >= WINDOW + 32,
        "a held-out case is at least one full window"
    );
    // Window boundaries, with the trailing remainder merged into the last one.
    let mut bounds = Vec::new();
    let mut lo = 0usize;
    while n - lo >= 2 * WINDOW {
        bounds.push((lo, lo + WINDOW));
        lo += WINDOW;
    }
    bounds.push((lo, n));

    // Linear edges are the default everywhere; every other feature is an
    // overwrite at a drawn position.
    let mut terms: Vec<Term> = (0..n)
        .map(|i| {
            if i + 1 < n {
                Term::Jump(i + 1)
            } else {
                Term::Ret
            }
        })
        .collect();

    let window_count = bounds.len();
    for (window, (lo, hi)) in bounds.into_iter().enumerate() {
        let last_window = window + 1 == window_count;
        // The window's two exits, and the block that jumps over them into the
        // next window.
        terms[hi - 1] = Term::Ret;
        terms[hi - 2] = Term::Ret;
        terms[hi - 3] = if last_window {
            Term::Ret
        } else {
            Term::Jump(hi)
        };

        // The chain runs lo..=hi-4; every drawn position lands inside it.
        let chain_lo = lo;
        let chain_hi = hi - 4;
        for (i, slot) in terms.iter_mut().enumerate().take(chain_hi).skip(chain_lo) {
            *slot = Term::Jump(i + 1);
        }
        terms[chain_hi] = Term::Jump(chain_hi + 1);

        let span = chain_hi - chain_lo;
        let mut taken: Vec<usize> = Vec::new();
        let draw = |rng: &mut Rng, taken: &mut Vec<usize>, width: usize| -> usize {
            loop {
                let pick = chain_lo + rng.below((span - width) as u64) as usize;
                if (pick..pick + width).any(|p| taken.contains(&p)) {
                    continue;
                }
                taken.extend(pick..pick + width);
                return pick;
            }
        };

        // One diamond: head, two arms, join.
        let diamond = draw(rng, &mut taken, 4);
        terms[diamond] = Term::Cond(diamond + 2);
        terms[diamond + 1] = Term::Jump(diamond + 3);
        terms[diamond + 2] = Term::Jump(diamond + 3);
        terms[diamond + 3] = Term::Jump(diamond + 4);

        // One back edge, from a drawn source to a drawn earlier block.
        let back_source = loop {
            let pick = draw(rng, &mut taken, 1);
            if pick > chain_lo {
                break pick;
            }
        };
        let back_target = chain_lo + rng.below((back_source - chain_lo) as u64) as usize;
        terms[back_source] = Term::Cond(back_target);

        // Two cross edges into the window's exits, which is what gives those
        // blocks a predecessor.
        let first_exit = draw(rng, &mut taken, 1);
        terms[first_exit] = Term::Cond(hi - 2);
        let second_exit = draw(rng, &mut taken, 1);
        terms[second_exit] = Term::Cond(hi - 1);

        // One forward cross edge that skips over intermediate blocks.
        let cross = draw(rng, &mut taken, 1);
        if cross + 2 < chain_hi {
            let ahead = cross + 2 + rng.below((chain_hi - cross - 1) as u64) as usize;
            terms[cross] = Term::Cond(ahead.min(chain_hi));
        }
    }

    terms
}

#[cfg(test)]
mod tests {
    use super::*;
    use flutterdec_ir::build_function_ir;
    use std::collections::HashSet;

    /// The generator's own edge list has to be what the product's block builder
    /// derives from the text. If these ever disagree, every correctness verdict
    /// and every span in the run is measuring a graph nobody described.
    #[test]
    fn every_disclosed_case_lifts_to_the_graph_it_describes() {
        for case in disclosed_cases() {
            let ir = build_function_ir(&case.disasm);
            assert_eq!(ir.blocks.len(), case.blocks, "{}: block count", case.name);
            for (i, block) in ir.blocks.iter().enumerate() {
                assert_eq!(block.id, i, "{}: dense ids", case.name);
                assert_eq!(
                    block.succs, case.expected_succs[i],
                    "{}: successors of block {i}",
                    case.name
                );
            }
        }
    }

    /// The matrix is fixed by contract and no pair may be skipped: seven
    /// topologies at three sizes, plus two added loads on two of them.
    #[test]
    fn the_disclosed_matrix_is_complete() {
        let cases = disclosed_cases();
        assert_eq!(cases.len(), 7 * 3 + 2 * 3 * 2, "33 disclosed cases");
        let names: HashSet<String> = cases.iter().map(|c| c.name.clone()).collect();
        assert_eq!(names.len(), cases.len(), "case names are unique");
        for topology in DISCLOSED_TOPOLOGIES {
            for blocks in DISCLOSED_SIZES {
                assert!(names.contains(&format!("{topology}/{blocks}/base")));
                for (load, _) in LOADS.iter().skip(1) {
                    let expected = LOAD_TOPOLOGIES.contains(&topology);
                    assert_eq!(
                        names.contains(&format!("{topology}/{blocks}/{load}")),
                        expected,
                        "{topology}/{blocks}/{load}"
                    );
                }
            }
        }
    }

    /// The fixed mix is nine instructions per block, and the loads add exactly
    /// two and thirty-two on top of it.
    #[test]
    fn the_instruction_mix_is_what_the_contract_fixes() {
        for case in disclosed_cases() {
            let extra = match case.load.as_str() {
                "base" => 0,
                "light" => 2,
                "heavy" => 32,
                other => panic!("unexpected load {other}"),
            };
            assert_eq!(
                case.instructions_per_block,
                DISCLOSED_BODY + 1 + extra,
                "{}",
                case.name
            );
            assert_eq!(
                case.disasm.instructions.len(),
                case.blocks * case.instructions_per_block,
                "{}",
                case.name
            );
        }

        let case = disclosed_cases()
            .into_iter()
            .find(|c| c.name == "linear/64/base")
            .expect("linear base case");
        let first: Vec<&str> = case.disasm.instructions[..9]
            .iter()
            .map(|i| i.mnemonic.as_str())
            .collect();
        assert_eq!(
            first,
            ["mov", "add", "sub", "orr", "ldr", "bl", "str", "cmp", "b"],
            "four register operations, pool load, direct call, unmodelled write, compare, terminator"
        );
        assert!(case.disasm.instructions[4].annotation.starts_with("pool["));
    }

    /// No generated block may look like the Dart stack-overflow guard: the IR
    /// builder elides that group and prunes its slow path, which would delete
    /// blocks the case claims to have.
    #[test]
    fn no_case_accidentally_spells_the_stack_overflow_guard() {
        for case in disclosed_cases() {
            for instruction in &case.disasm.instructions {
                assert!(
                    !instruction.op_str.contains("x26"),
                    "{}: {} {}",
                    case.name,
                    instruction.mnemonic,
                    instruction.op_str
                );
                assert_ne!(instruction.mnemonic, "b.ls", "{}", case.name);
                assert!(
                    !instruction.op_str.starts_with("x15,"),
                    "{}: {}",
                    case.name,
                    instruction.op_str
                );
            }
        }
    }

    /// Each named shape has to actually be that shape, or the matrix is seven
    /// names for one graph.
    #[test]
    fn each_topology_has_its_defining_property() {
        let by_name = |name: &str| {
            disclosed_cases()
                .into_iter()
                .find(|c| c.name == name)
                .unwrap_or_else(|| panic!("missing {name}"))
        };

        let linear = by_name("linear/64/base");
        assert!(linear.expected_succs[..63]
            .iter()
            .enumerate()
            .all(|(i, s)| s == &vec![i + 1]));
        assert!(linear.expected_succs[63].is_empty());

        let diamond = by_name("diamond-chain/64/base");
        assert_eq!(diamond.expected_succs[0], vec![1, 2]);
        assert_eq!(diamond.expected_succs[1], vec![3]);
        assert_eq!(diamond.expected_succs[2], vec![3]);

        let fan_in = by_name("fan-in/64/base");
        let sink_preds = fan_in
            .expected_succs
            .iter()
            .filter(|s| s.contains(&63))
            .count();
        assert_eq!(sink_preds, 63, "every other block reaches the sink");

        let nested = by_name("nested-loop/64/base");
        assert_eq!(nested.expected_succs[61], vec![1], "inner back edge");
        assert_eq!(nested.expected_succs[62], vec![0], "outer back edge");

        let multi = by_name("multi-exit/64/base");
        let exits: HashSet<usize> = multi
            .expected_succs
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_empty())
            .map(|(i, _)| i)
            .collect();
        assert_eq!(exits, HashSet::from([60, 61, 62, 63]), "four exits");
        assert_eq!(multi.expected_succs[59], vec![0], "back edge to the header");

        let no_exit = by_name("no-exit/64/base");
        assert!(
            no_exit.expected_succs.iter().all(|s| !s.is_empty()),
            "no block may return"
        );
        assert_eq!(no_exit.expected_succs[63], vec![0]);

        let irreducible = by_name("irreducible/64/base");
        assert_eq!(irreducible.expected_succs[0], vec![1, 2]);
        assert_eq!(irreducible.expected_succs[1], vec![2, 3]);
        assert_eq!(irreducible.expected_succs[2], vec![1, 3]);
    }

    /// Every case has to emit a body, and the irreducible one has to take a
    /// visibly different path from the reducible ones: region analysis declines
    /// it, so emission falls back to the DFS walk. Nothing in the public
    /// artifact names the emitter that ran, so two observable consequences
    /// stand in for it. At 64 blocks the fallback inlines both arms of every
    /// branch and emits several times the body the structured shapes do. Going
    /// from 64 to 256 blocks the structured output grows with the graph while
    /// the fallback stays flat, because its depth budget, not the block count,
    /// is what bounds it.
    ///
    /// Restricted to the two smaller sizes. Emission dominates the matrix and
    /// the 1024-block irreducible case takes seconds on its own, which does not
    /// belong in a unit test; every case at every size is checked by the
    /// harness, which runs the same structural pass before every measured run
    /// and fails the run when it does not hold.
    #[test]
    fn the_matrix_exercises_both_emitters() {
        let symbols = std::collections::HashMap::new();
        let mut lines: std::collections::HashMap<(String, usize), usize> =
            std::collections::HashMap::new();
        for case in disclosed_cases()
            .into_iter()
            .filter(|c| c.blocks <= 256 && c.load == "base")
        {
            let ir = build_function_ir(&case.disasm);
            let artifact = flutterdec_decompiler::emit_pseudocode(&ir, &symbols);
            assert!(!artifact.source.is_empty(), "{}", case.name);
            assert!(artifact.source.starts_with("dynamic "), "{}", case.name);
            lines.insert(
                (case.topology.clone(), case.blocks),
                artifact.source.lines().count(),
            );
        }
        assert_eq!(
            lines.len(),
            DISCLOSED_TOPOLOGIES.len() * 2,
            "every topology emitted at both sizes"
        );

        let irreducible_64 = lines[&("irreducible".to_string(), 64)];
        for topology in ["linear", "diamond-chain", "nested-loop", "no-exit"] {
            let structured = lines[&(topology.to_string(), 64)];
            assert!(
                irreducible_64 > 2 * structured,
                "at 64 blocks irreducible emitted {irreducible_64} lines and \
                 {topology} emitted {structured}: the fallback is not expanding the graph"
            );
        }

        let irreducible_256 = lines[&("irreducible".to_string(), 256)];
        let linear_64 = lines[&("linear".to_string(), 64)];
        let linear_256 = lines[&("linear".to_string(), 256)];
        assert!(
            linear_256 > 3 * linear_64,
            "structured output grows with the graph: {linear_64} then {linear_256}"
        );
        assert_eq!(
            irreducible_256, irreducible_64,
            "the fallback is bounded by its depth budget, not by the block count"
        );
    }

    /// The digest has to move when the work moves, or a drifted workload would
    /// pass the binding check.
    #[test]
    fn the_workload_digest_binds_the_instruction_stream() {
        let cases = disclosed_cases();
        let digests: HashSet<String> = cases.iter().map(|c| c.workload_sha256.clone()).collect();
        assert_eq!(digests.len(), cases.len(), "no two cases share a digest");
        assert_eq!(
            disclosed_cases()[0].workload_sha256,
            cases[0].workload_sha256,
            "generation is deterministic"
        );
    }

    /// The held-out draw has to reproduce from its seed and stay inside the
    /// declared size range.
    #[test]
    fn held_out_cases_reproduce_and_avoid_the_disclosed_sizes() {
        let seed = 0x0f0e_0d0c_0b0a_0908_0706_0504_0302_0100u128;
        let first = held_out_cases(seed);
        let second = held_out_cases(seed);
        assert_eq!(first.len(), HELD_OUT_CASES);
        for (a, b) in first.iter().zip(&second) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.workload_sha256, b.workload_sha256);
            assert!((96..=2048).contains(&a.blocks), "{}", a.name);
            assert!(!DISCLOSED_SIZES.contains(&a.blocks), "{}", a.name);
            assert_eq!(a.instructions_per_block, HELD_OUT_BODY + 1, "{}", a.name);
        }
        let other = held_out_cases(seed ^ 1);
        assert_ne!(
            first
                .iter()
                .map(|c| c.workload_sha256.clone())
                .collect::<Vec<_>>(),
            other
                .iter()
                .map(|c| c.workload_sha256.clone())
                .collect::<Vec<_>>(),
            "a different seed draws different work"
        );
    }

    /// Every feature the held-out distribution promises has to be present in
    /// every window, and the graph it describes has to be the graph the product
    /// builds.
    #[test]
    fn every_held_out_window_carries_the_required_features() {
        for seed in [1u128, 2, 3, 1 << 100, u128::MAX] {
            for case in held_out_cases(seed) {
                let ir = build_function_ir(&case.disasm);
                assert_eq!(ir.blocks.len(), case.blocks, "{} @ {seed}", case.name);
                for (i, block) in ir.blocks.iter().enumerate() {
                    assert_eq!(
                        block.succs, case.expected_succs[i],
                        "{} @ {seed}: block {i}",
                        case.name
                    );
                }

                let n = case.blocks;
                let mut lo = 0usize;
                let mut bounds = Vec::new();
                while n - lo >= 2 * WINDOW {
                    bounds.push((lo, lo + WINDOW));
                    lo += WINDOW;
                }
                bounds.push((lo, n));

                for (lo, hi) in bounds {
                    let window = &case.expected_succs[lo..hi];
                    let linear = window
                        .iter()
                        .enumerate()
                        .filter(|(i, s)| s.contains(&(lo + i + 1)))
                        .count();
                    assert!(linear > 8, "{} window {lo}: linear edges", case.name);

                    let diamond = window.iter().enumerate().any(|(i, s)| {
                        s.len() == 2
                            && s.iter().all(|t| *t > lo + i && *t < hi)
                            && case.expected_succs[s[0]] == case.expected_succs[s[1]]
                    });
                    assert!(diamond, "{} window {lo}: a diamond", case.name);

                    let back = window
                        .iter()
                        .enumerate()
                        .any(|(i, s)| s.iter().any(|t| *t <= lo + i));
                    assert!(back, "{} window {lo}: a back edge", case.name);

                    let exits = window.iter().filter(|s| s.is_empty()).count();
                    assert!(exits >= 2, "{} window {lo}: two exits", case.name);

                    let cross = window
                        .iter()
                        .enumerate()
                        .any(|(i, s)| s.iter().any(|t| *t > lo + i + 1));
                    assert!(cross, "{} window {lo}: a cross edge", case.name);
                }
            }
        }
    }
}
