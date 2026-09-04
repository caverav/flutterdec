//! A loop back into the entry block is two incoming paths, not one.
//!
//! The DFS fallback emits a block once and merges path-dependent state wherever
//! more than one path reaches it. No `succs` list names the path the call itself
//! takes into the entry block, so an entry block that is also a branch target
//! used to show a single predecessor: the merge declined, and every value the
//! function entered with - the argument bindings, the pre-call values behind the
//! provenance annotations, the compare and selector expressions - kept describing
//! the first iteration and no other.
//!
//! Every check here goes through the public emitter entry points and reads the
//! artifact a consumer reads. `emit_pseudocode_direct_dfs` is the fallback surface
//! itself and the reference a declined structured attempt must equal
//! (`VAL-EMIT-003`); `emit_pseudocode` is the production surface, which reaches
//! the same fallback through a structured decline.
//!
//! The artifact fields asserted are what the pipeline reports: `source` is the
//! emitted pseudocode, `unresolved_cf` and the `regN` token counts are what
//! `quality.json` sums (`quality.rs` counts exactly these tokens), and
//! `emission` - decline cause, rollback count, traversal events and the block
//! ledger - is what `report.json` carries. `regN` is the emitter's spelling for a
//! register with no value in hand, so a merged register reads as `regN` and a
//! stale one reads as its first-iteration value.

use flutterdec_decompiler::{
    emit_pseudocode, emit_pseudocode_direct_dfs, BlockDisposition, PseudocodeArtifact,
    StructuredDeclineCause, TraversalEventKind, ANNOTATION_LITERALS,
};
use flutterdec_ir::{rebuild_edges, BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::HashMap;

/// The alias the first iteration binds into `x1`: `arg0`, after the naming pass
/// renamed it for the stack slot it is stored into. Any occurrence of it in a loop
/// whose body rewrites `x1` is the stale rendering this target forbids.
const FIRST_ITERATION_VALUE: &str = "slot0";

fn op_of(src: &str) -> IROp {
    if src.starts_with("ret") {
        IROp::Return
    } else if src.starts_with("cbz") || src.starts_with("cbnz") {
        IROp::Branch
    } else if src.starts_with("bl ") {
        IROp::Call
    } else if src.starts_with('b') && src.contains('#') {
        IROp::Jump
    } else {
        IROp::Other
    }
}

/// One block, with the target field production's IR builder fills in for a
/// control instruction.
fn block(id: usize, base_va: u64, srcs: &[&str], succs: &[usize]) -> BasicBlock {
    BasicBlock {
        id,
        start_va: base_va,
        instrs: srcs
            .iter()
            .enumerate()
            .map(|(index, src)| {
                let op = op_of(src);
                let target = match op {
                    IROp::Branch | IROp::Jump | IROp::Call => src
                        .rsplit_once('#')
                        .map(|(_, va)| format!("#{va}"))
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                LlirInstr {
                    va: base_va + 4 * index as u64,
                    op,
                    src: (*src).to_string(),
                    target,
                }
            })
            .collect(),
        succs: succs.to_vec(),
        preds: Vec::new(),
    }
}

struct Case {
    name: &'static str,
    /// What the shape is for, in one line, so a failure says which path broke.
    intent: &'static str,
    blocks: Vec<BasicBlock>,
    /// The explicit predecessors `rebuild_edges` derives for the entry block. The
    /// implicit call path is never one of them, which is the whole defect.
    entry_preds: &'static [usize],
    /// Blocks with exactly one predecessor, which keep the fallback's fast path.
    single_pred_blocks: &'static [usize],
    /// The complete fallback artifact source, exactly.
    dfs_source: &'static str,
    total_calls: usize,
}

fn function(name: &str, id: u64, blocks: Vec<BasicBlock>) -> FunctionIr {
    let mut ir = FunctionIr {
        function_id: id,
        name: name.to_string(),
        entry_va: blocks[0].start_va,
        blocks,
    };
    rebuild_edges(&mut ir.blocks);
    ir
}

fn symbols() -> HashMap<u64, String> {
    HashMap::from([(0x9000u64, "helperCall".to_string())])
}

/// Occurrences of `token` as a whole identifier, the way the quality report counts
/// a register spelling.
fn token_count(source: &str, token: &str) -> usize {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    let mut count = 0usize;
    let mut offset = 0usize;
    while let Some(pos) = source[offset..].find(token) {
        let start = offset + pos;
        let end = start + token.len();
        if !source[..start].chars().next_back().is_some_and(ident)
            && !source[end..].chars().next().is_some_and(ident)
        {
            count += 1;
        }
        offset = end;
    }
    count
}

/// The artifact's statements, without the function and helper signature lines.
/// Every signature names `slot0` through `slot5` as its parameters, and a
/// parameter name is not a rendered value, so counting a value in the signature
/// would report a loss no reader can see. Declarations stay: they are indented.
fn statements(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.starts_with("dynamic "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every annotation span in the artifact, whichever loss site wrote it. An
/// annotation is a provenance claim about one value, so a stale one is a stale
/// claim even when the code beside it is right.
fn annotation_spans(source: &str) -> Vec<String> {
    let mut spans = Vec::new();
    for line in source.lines() {
        let bytes = line.as_bytes();
        for index in 0..bytes.len() {
            for literal in ANNOTATION_LITERALS {
                if line[index..].starts_with(literal.open()) {
                    if let Some(len) = literal.span_len(&bytes[index..]) {
                        spans.push(line[index..index + len].to_string());
                    }
                }
            }
        }
    }
    spans
}

/// Every reachable block is accounted for, the ledger reconciles with the
/// traversal events, and no accounting field drifted.
fn assert_accounting(case: &str, artifact: &PseudocodeArtifact, reachable: usize) {
    artifact
        .emission
        .validate()
        .unwrap_or_else(|error| panic!("{case} accounting: {error}"));
    let ledger = artifact.emission.block_ledger();
    ledger
        .validate(artifact.emission.events())
        .unwrap_or_else(|error| panic!("{case} ledger: {error}"));
    assert_eq!(
        ledger.dispositions.len(),
        reachable,
        "{case} must dispose of every reachable block: {:?}",
        ledger.dispositions
    );
    assert!(
        ledger
            .dispositions
            .iter()
            .all(|record| record.disposition == BlockDisposition::DfsEmitted),
        "{case} is a fallback artifact, so every disposition is a DFS one: {:?}",
        ledger.dispositions
    );
    assert_eq!(
        artifact.emission.decline(),
        None,
        "{case} entered the fallback directly, so it records no decline"
    );
    assert_eq!(
        artifact.emission.rollbacks(),
        0,
        "{case} never mutated structured state, so it rolled nothing back"
    );
}

/// An entry block a back edge targets, in every shape the fallback can meet one.
fn entry_loop_cases() -> Vec<Case> {
    vec![
        Case {
            name: "entrySelfLoop",
            intent: "the entry block is its own latch, so the implicit path is the only other one",
            blocks: vec![
                block(
                    0,
                    0x1000,
                    &["str x1, [x29, #16]", "mov x1, #0x2a", "cbz x0, #0x1000"],
                    &[1, 0],
                ),
                block(1, 0x2000, &["ret"], &[]),
            ],
            entry_preds: &[0],
            single_pred_blocks: &[1],
            dfs_source: "\
dynamic entrySelfLoop(dynamic slot0, dynamic slot1, dynamic slot2, dynamic slot3, dynamic slot4, dynamic slot5) {
  dynamic tmp1;

  // loop back-edges: block 0
  tmp1 = reg1;
  if (reg0 == 0) {
    // control rejoins block 0: already emitted above
  }
  else {
    return null;
  }
}",
            total_calls: 0,
        },
        Case {
            name: "lowerAddressLatch",
            intent: "the latch sits below the entry header, which no address order can classify",
            blocks: vec![
                block(0, 0x4000, &["str x1, [x29, #16]", "cbz x0, #0x8000"], &[1, 2]),
                block(1, 0x1000, &["mov x1, #0x2a", "b #0x4000"], &[0]),
                block(2, 0x8000, &["ret"], &[]),
            ],
            entry_preds: &[1],
            single_pred_blocks: &[1, 2],
            dfs_source: "\
dynamic lowerAddressLatch(dynamic slot0, dynamic slot1, dynamic slot2, dynamic slot3, dynamic slot4, dynamic slot5) {
  dynamic tmp1;

  // loop back-edges: block 0
  tmp1 = reg1;
  if (reg0 == 0) {
    return null;
  }
  // control rejoins block 0: already emitted above
}",
            total_calls: 0,
        },
        Case {
            name: "twoLatchesConflicting",
            intent: "two latches write one register different values, so no path describes it",
            blocks: vec![
                block(0, 0x1000, &["str x1, [x29, #16]", "cbz x0, #0x3000"], &[1, 2]),
                block(1, 0x2000, &["mov x1, #0x2a", "b #0x1000"], &[0]),
                block(2, 0x3000, &["mov x1, #0x33", "cbnz x2, #0x1000"], &[3, 0]),
                block(3, 0x4000, &["ret"], &[]),
            ],
            entry_preds: &[1, 2],
            single_pred_blocks: &[1, 2, 3],
            dfs_source: "\
dynamic twoLatchesConflicting(dynamic slot0, dynamic slot1, dynamic slot2, dynamic slot3, dynamic slot4, dynamic slot5) {
  dynamic tmp1;

  // loop back-edges: block 0
  tmp1 = reg1;
  if (reg0 == 0) {
    if (slot1 != 0) {
      // control rejoins block 0: already emitted above
    }
    else {
      return null;
    }
  }
  else {
    // control rejoins block 0: already emitted above
  }
}",
            total_calls: 0,
        },
        Case {
            name: "twoLatchesCompatible",
            intent: "two latches write one register the same value, still not the entry value",
            blocks: vec![
                block(0, 0x1000, &["str x1, [x29, #16]", "cbz x0, #0x3000"], &[1, 2]),
                block(1, 0x2000, &["mov x1, #0x2a", "b #0x1000"], &[0]),
                block(2, 0x3000, &["mov x1, #0x2a", "cbnz x2, #0x1000"], &[3, 0]),
                block(3, 0x4000, &["ret"], &[]),
            ],
            entry_preds: &[1, 2],
            single_pred_blocks: &[1, 2, 3],
            dfs_source: "\
dynamic twoLatchesCompatible(dynamic slot0, dynamic slot1, dynamic slot2, dynamic slot3, dynamic slot4, dynamic slot5) {
  dynamic tmp1;

  // loop back-edges: block 0
  tmp1 = reg1;
  if (reg0 == 0) {
    if (slot1 != 0) {
      // control rejoins block 0: already emitted above
    }
    else {
      return null;
    }
  }
  else {
    // control rejoins block 0: already emitted above
  }
}",
            total_calls: 0,
        },
        Case {
            name: "conditionalExit",
            intent: "the loop leaves through a condition, and a call clobbers state inside it",
            blocks: vec![
                block(
                    0,
                    0x1000,
                    &["str x1, [x29, #16]", "bl #0x9000", "cbz x0, #0x4000"],
                    &[1, 3],
                ),
                block(1, 0x2000, &["mov x1, #0x2a", "cbnz x2, #0x4000"], &[2, 3]),
                block(2, 0x3000, &["b #0x1000"], &[0]),
                block(3, 0x4000, &["ret"], &[]),
            ],
            entry_preds: &[2],
            single_pred_blocks: &[1, 2],
            dfs_source: "\
dynamic conditionalExit(dynamic slot0, dynamic slot1, dynamic slot2, dynamic slot3, dynamic slot4, dynamic slot5) {
  dynamic tmp1;

  // loop back-edges: block 0
  tmp1 = reg1;
  final t1 = helperCall();
  if (t1 == 0) {
    return null;
  }
  if (reg2 != 0) {
    return null;
  }
  // control rejoins block 0: already emitted above
}",
            total_calls: 1,
        },
        Case {
            name: "annotatedEntryLoop",
            intent: "the loop rewrites the register a pre-call provenance value was read through",
            blocks: vec![
                block(
                    0,
                    0x1000,
                    &[
                        "ldur x9, [x1, #7]",
                        "bl #0x9000",
                        "stur x9, [x19, #7]",
                        "cbz x0, #0x30000",
                    ],
                    &[1, 2],
                ),
                block(1, 0x2000, &["mov x1, #0x2a", "b #0x1000"], &[0]),
                block(2, 0x30000, &["ret"], &[]),
            ],
            entry_preds: &[1],
            single_pred_blocks: &[1, 2],
            dfs_source: "\
dynamic annotatedEntryLoop(dynamic slot0, dynamic slot1, dynamic slot2, dynamic slot3, dynamic slot4, dynamic slot5) {
  // loop back-edges: block 0
  final t1 = helperCall();
  reg19.f8 = reg9;
  if (t1 == 0) {
    return t1;
  }
  // control rejoins block 0: already emitted above
}",
            total_calls: 1,
        },
    ]
}

/// The shapes that must keep the one-predecessor fast path: an entry block no edge
/// targets, and ordinary blocks one path reaches.
fn fast_path_cases() -> Vec<Case> {
    vec![
        Case {
            name: "entryWithoutBackEdge",
            intent: "no edge targets the entry block, so the implicit path is the only one",
            blocks: vec![
                block(0, 0x1000, &["str x1, [x29, #16]", "cbz x0, #0x3000"], &[1, 2]),
                block(1, 0x2000, &["mov x1, #0x2a", "b #0x3000"], &[2]),
                block(2, 0x3000, &["ret"], &[]),
            ],
            entry_preds: &[],
            single_pred_blocks: &[1],
            dfs_source: "\
dynamic entryWithoutBackEdge(dynamic slot0, dynamic slot1, dynamic slot2, dynamic slot3, dynamic slot4, dynamic slot5) {
  dynamic tmp1;

  tmp1 = slot0;
  return null;
}",
            total_calls: 0,
        },
        Case {
            name: "onePredecessorControls",
            intent: "an ordinary block one path reaches keeps the value that path wrote",
            blocks: vec![
                block(0, 0x1000, &["mov x1, #0x2a", "b #0x2000"], &[1]),
                block(1, 0x2000, &["str x1, [x29, #16]", "cbz x0, #0x4000"], &[2, 3]),
                block(2, 0x3000, &["str x1, [x29, #24]", "b #0x4000"], &[3]),
                block(3, 0x4000, &["ret"], &[]),
            ],
            entry_preds: &[],
            single_pred_blocks: &[1, 2],
            dfs_source: "\
dynamic onePredecessorControls(dynamic slot0, dynamic slot1, dynamic slot2, dynamic slot3, dynamic slot4, dynamic slot5) {
  int tmp1;
  int tmp2;

  tmp1 = 0x2a;
  if (reg0 == 0) {
    return null;
  }
  tmp2 = 0x2a;
  return null;
}",
            total_calls: 0,
        },
    ]
}

/// The graph relations each fixture claims, checked against the IR the public
/// builder produces rather than assumed from the successor lists.
fn assert_ir_relations(case: &Case, ir: &FunctionIr) {
    assert_eq!(
        ir.blocks[0].preds, case.entry_preds,
        "{}: the entry block's explicit predecessors are the fixture's premise",
        case.name
    );
    for id in case.single_pred_blocks {
        assert_eq!(
            ir.blocks[*id].preds.len(),
            1,
            "{}: block {id} must have exactly one predecessor",
            case.name
        );
    }
}

#[test]
fn an_entry_loop_merges_the_implicit_path_with_every_back_edge() {
    for (index, case) in entry_loop_cases().into_iter().enumerate() {
        let reachable = case.blocks.len();
        let ir = function(case.name, 0x4200 + index as u64, case.blocks.clone());
        assert_ir_relations(&case, &ir);
        assert!(
            !case.entry_preds.is_empty(),
            "{}: an entry-loop fixture needs a back edge into the entry block",
            case.name
        );

        let artifact = emit_pseudocode_direct_dfs(&ir, &symbols());
        assert_eq!(
            artifact.source, case.dfs_source,
            "{}: {}\nactual:\n{}",
            case.name, case.intent, artifact.source
        );
        assert_eq!(
            token_count(&statements(&artifact.source), FIRST_ITERATION_VALUE),
            0,
            "{}: the loop body rewrites x1, so its entry value describes no iteration but the \
             first:\n{}",
            case.name,
            artifact.source
        );
        for span in annotation_spans(&artifact.source) {
            assert!(
                !span.contains(FIRST_ITERATION_VALUE),
                "{}: a provenance annotation claims the entry value:\n{span}",
                case.name
            );
        }
        assert_eq!(
            artifact.total_calls, case.total_calls,
            "{}: call counter",
            case.name
        );
        assert_eq!(
            artifact.placeholder_ifs, 0,
            "{}: every branch recovered a condition",
            case.name
        );
        assert_eq!(
            artifact.unresolved_cf, 0,
            "{}: no unresolved control flow",
            case.name
        );
        assert_eq!(
            artifact.repeated_blocks, 0,
            "{}: the fallback emitted each block once",
            case.name
        );
        assert_accounting(case.name, &artifact, reachable);
        assert!(
            artifact.emission.events().is_empty(),
            "{}: no traversal budget refused an edge here: {:?}",
            case.name,
            artifact.emission.events()
        );
    }
}

#[test]
fn a_block_one_path_reaches_keeps_the_fast_path() {
    for (index, case) in fast_path_cases().into_iter().enumerate() {
        let reachable = case.blocks.len();
        let ir = function(case.name, 0x4300 + index as u64, case.blocks.clone());
        assert_ir_relations(&case, &ir);
        assert!(
            case.entry_preds.is_empty(),
            "{}: a fast-path fixture has no edge into its entry block",
            case.name
        );

        let artifact = emit_pseudocode_direct_dfs(&ir, &symbols());
        assert_eq!(
            artifact.source, case.dfs_source,
            "{}: {}\nactual:\n{}",
            case.name, case.intent, artifact.source
        );
        assert!(
            token_count(&statements(&artifact.source), FIRST_ITERATION_VALUE) > 0
                || token_count(&statements(&artifact.source), "0x2a") > 0,
            "{}: a value one path carries must survive the fast path:\n{}",
            case.name,
            artifact.source
        );
        assert_accounting(case.name, &artifact, reachable);
    }
}

/// A helper definition renders a block the walk could not inline, from the same
/// merged state: the entry block of a loop deep enough to exhaust the depth budget
/// is emitted once in the body and again in helpers, and a stale copy in a helper
/// is as wrong as a stale copy in the body.
#[test]
fn helper_definitions_for_an_entry_loop_render_the_merged_state() {
    let mut blocks = vec![block(
        0,
        0x1000,
        &["str x1, [x29, #16]", "cbz x0, #0x20000"],
        &[1, 14],
    )];
    for id in 1..14 {
        let last = id == 13;
        let jump = if last {
            "b #0x1000".to_string()
        } else {
            format!("b #{:#x}", 0x1000 + 0x1000 * (id as u64 + 1))
        };
        blocks.push(block(
            id,
            0x1000 + 0x1000 * id as u64,
            &[if last { "mov x1, #0x2a" } else { "nop" }, &jump],
            &[if last { 0 } else { id + 1 }],
        ));
    }
    blocks.push(block(14, 0x20000, &["ret"], &[]));
    let ir = function("deepEntryLoop", 0x4400, blocks);
    assert_eq!(
        ir.blocks[0].preds,
        vec![13],
        "the deep latch is the entry block's only explicit predecessor"
    );

    let artifact = emit_pseudocode_direct_dfs(&ir, &symbols());
    let helpers = artifact
        .source
        .lines()
        .filter(|line| line.starts_with("dynamic _block_"))
        .count();
    assert!(
        helpers > 0,
        "the depth budget must push copies of the loop into helpers:\n{}",
        artifact.source
    );
    assert_eq!(
        token_count(&statements(&artifact.source), FIRST_ITERATION_VALUE),
        0,
        "no helper copy may render the entry value the loop overwrites:\n{}",
        artifact.source
    );
    assert_eq!(
        token_count(&statements(&artifact.source), "reg1"),
        helpers + 1,
        "every rendering of the merged read is unresolved, body and helpers:\n{}",
        artifact.source
    );
    artifact
        .emission
        .validate()
        .expect("deep entry loop accounting");
    artifact
        .emission
        .block_ledger()
        .validate(artifact.emission.events())
        .expect("deep entry loop ledger");
    assert!(
        artifact
            .emission
            .events()
            .iter()
            .any(|event| event.kind == TraversalEventKind::DfsDepthOmission),
        "the depth budget refused an edge, so it owes an event: {:?}",
        artifact.emission.events()
    );
}

/// The production surface reaches the fallback through a structured decline, and
/// the declined artifact must equal direct DFS apart from its cause accounting.
/// The fixture is irreducible - two entries into one cycle - and its entry block is
/// a back-edge target as well, so the decline lands on exactly the shape this
/// target is about.
#[test]
fn a_declined_irreducible_entry_loop_equals_direct_dfs() {
    let ir = function(
        "irreducibleEntryLoop",
        0x4500,
        vec![
            block(
                0,
                0x1000,
                &["str x1, [x29, #16]", "cbz x0, #0x3000"],
                &[1, 2],
            ),
            block(1, 0x2000, &["mov x1, #0x2a", "cbnz x2, #0x3000"], &[3, 2]),
            block(2, 0x3000, &["cbz x3, #0x2000"], &[3, 1]),
            block(3, 0x4000, &["cbnz x5, #0x1000"], &[4, 0]),
            block(4, 0x5000, &["ret"], &[]),
        ],
    );
    assert_eq!(
        ir.blocks[0].preds,
        vec![3],
        "the entry block is a back-edge target"
    );

    let auto = emit_pseudocode(&ir, &symbols());
    let dfs = emit_pseudocode_direct_dfs(&ir, &symbols());
    let decline = auto
        .emission
        .decline()
        .expect("an irreducible graph declines structured emission");
    assert_eq!(
        decline.cause,
        StructuredDeclineCause::Irreducible,
        "the decline names the preflight cause"
    );
    assert_eq!(
        auto.emission.rollbacks(),
        0,
        "a preflight decline mutates nothing, so it rolls nothing back"
    );
    assert_eq!(
        auto.source, dfs.source,
        "the declined artifact is the fallback artifact:\nauto:\n{}\ndfs:\n{}",
        auto.source, dfs.source
    );
    assert_eq!(
        token_count(&statements(&auto.source), FIRST_ITERATION_VALUE),
        0,
        "the declined path merges the entry loop too:\n{}",
        auto.source
    );
    assert_eq!(
        auto.emission.structured_declines(),
        1,
        "the generic count is derived from the one primary cause"
    );
    assert_eq!(
        (
            auto.placeholder_ifs,
            auto.unresolved_cf,
            auto.raw_register_calls,
            auto.total_calls,
            auto.repeated_blocks,
        ),
        (
            dfs.placeholder_ifs,
            dfs.unresolved_cf,
            dfs.raw_register_calls,
            dfs.total_calls,
            dfs.repeated_blocks,
        ),
        "every quality counter equals the direct fallback's"
    );
    assert_eq!(
        auto.emission.block_ledger().dispositions,
        dfs.emission.block_ledger().dispositions,
        "the ledger partitions the function the same way on both paths"
    );
}
