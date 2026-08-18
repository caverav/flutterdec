// What the emitter omits, and what it says about it.
//
// The fixtures are written as successor lists, so a reader can draw the graph,
// and the assertions are about the artifact and the accounting together: a
// marker with no event, an event with no marker, or a call with no definition is
// what this file exists to catch.

use super::*;
use crate::helper_flow::HELPER_DEFINITION_BUDGET;
use flutterdec_ir::{rebuild_edges, BasicBlock, FunctionIr, IROp, LlirInstr};

fn va(id: usize) -> u64 {
    0x1000 + 0x10 * id as u64
}

fn marker_va(id: usize) -> u64 {
    0x90000 + id as u64 * 4
}

fn instr(va: u64, op: IROp, src: String, target: String) -> LlirInstr {
    LlirInstr { va, op, src, target }
}

/// A graph from successor lists alone: no successors is a return, one is a
/// jump, two is `cbz x0` whose taken edge is the second entry. Every block opens
/// with a call to its own marker symbol, so the artifact names the blocks it
/// emitted.
fn graph(function_id: u64, succs: &[Vec<usize>]) -> FunctionIr {
    let mut blocks: Vec<BasicBlock> = succs
        .iter()
        .enumerate()
        .map(|(id, succs)| {
            let start = va(id);
            let mut instrs = vec![instr(
                start,
                IROp::Call,
                format!("bl #{:#x}", marker_va(id)),
                format!("#{:#x}", marker_va(id)),
            )];
            let end = start + 4;
            match succs.as_slice() {
                [] => instrs.push(instr(end, IROp::Return, "ret".to_string(), String::new())),
                [only] => instrs.push(instr(
                    end,
                    IROp::Jump,
                    format!("b #{:#x}", va(*only)),
                    format!("#{:#x}", va(*only)),
                )),
                [_fallthrough, taken] => instrs.push(instr(
                    end,
                    IROp::Branch,
                    format!("cbz x0, #{:#x}", va(*taken)),
                    format!("#{:#x}", va(*taken)),
                )),
                _ => panic!("a fixture block has more than two successors"),
            }
            BasicBlock {
                id,
                start_va: start,
                instrs,
                succs: succs.clone(),
                preds: Vec::new(),
            }
        })
        .collect();
    rebuild_edges(&mut blocks);
    FunctionIr {
        function_id,
        name: format!("fixture{function_id}"),
        entry_va: va(0),
        blocks,
    }
}

/// A spine whose every block branches to one shared sink.
///
/// The sink is nobody's follow node and the region around it is past the repeat
/// budget, so structuring declines and the DFS walk runs. That walk stops
/// inlining the spine at its depth budget, so the spine is cut into helpers, one
/// per budget's worth of blocks: the block count is how these fixtures choose
/// how many helpers to ask for.
fn fan_in(function_id: u64, n: usize) -> FunctionIr {
    let sink = n - 1;
    let succs: Vec<Vec<usize>> = (0..n)
        .map(|id| {
            if id == sink {
                Vec::new()
            } else if id + 1 == sink {
                vec![sink]
            } else {
                vec![id + 1, sink]
            }
        })
        .collect();
    graph(function_id, &succs)
}

/// Block counts that land below, exactly on, and past the helper budget. All
/// three are asserted against `HELPER_DEFINITION_BUDGET` where they are used, so
/// a budget change fails the test rather than silently re-scoping it.
const BLOCKS_BELOW_HELPER_BUDGET: usize = 512;
const BLOCKS_AT_HELPER_BUDGET: usize = 776;
const BLOCKS_PAST_HELPER_BUDGET: usize = 784;

fn artifact_of(ir: &FunctionIr) -> PseudocodeArtifact {
    emit_pseudocode(ir, &HashMap::new())
}

fn source_lines(artifact: &PseudocodeArtifact) -> Vec<String> {
    artifact.source.lines().map(|l| l.to_string()).collect()
}

fn call_ids(artifact: &PseudocodeArtifact) -> BTreeSet<usize> {
    FuncEmitter::helper_call_ids(&source_lines(artifact))
}

fn definition_ids(artifact: &PseudocodeArtifact) -> BTreeSet<usize> {
    FuncEmitter::helper_definition_ids(&source_lines(artifact))
}

fn events_of(artifact: &PseudocodeArtifact, kind: TraversalEventKind) -> Vec<TraversalEvent> {
    artifact
        .emission
        .events()
        .iter()
        .copied()
        .filter(|e| e.kind == kind)
        .collect()
}

/// Every helper the artifact calls is defined exactly once, and every definition
/// is called. Read off the finished text, not off emitter bookkeeping.
fn assert_helpers_resolve(label: &str, artifact: &PseudocodeArtifact) {
    let calls = call_ids(artifact);
    let defs = definition_ids(artifact);
    assert_eq!(
        calls, defs,
        "{label}: helper calls and helper definitions must be the same set"
    );
    for id in &defs {
        let header = format!("dynamic _block_{id}() {{");
        assert_eq!(
            artifact
                .source
                .lines()
                .filter(|l| l.trim() == header)
                .count(),
            1,
            "{label}: helper {id} must be defined exactly once"
        );
    }
}

/// Every event names this function, a block of it as its source, and carries its
/// position in the event order.
fn assert_event_keys(label: &str, ir: &FunctionIr, artifact: &PseudocodeArtifact) {
    for (index, event) in artifact.emission.events().iter().enumerate() {
        assert_eq!(event.function_id, ir.function_id, "{label}: function key");
        assert!(
            ir.blocks.iter().any(|b| b.start_va == event.source_start_va),
            "{label}: source key {:#x} names no block of this function",
            event.source_start_va
        );
        assert_eq!(event.ordinal, index, "{label}: ordinals are the event order");
        match event.target {
            TraversalTarget::Block { start_va } => assert!(
                ir.blocks.iter().any(|b| b.start_va == start_va),
                "{label}: target key {start_va:#x} names no block of this function"
            ),
            TraversalTarget::Helper { id } => assert!(
                ir.blocks.iter().any(|b| b.id == id),
                "{label}: helper key {id} names no block of this function"
            ),
        }
    }
}

#[test]
fn helper_calls_below_the_budget_all_resolve() {
    let ir = fan_in(9101, BLOCKS_BELOW_HELPER_BUDGET);
    let artifact = artifact_of(&ir);

    let defs = definition_ids(&artifact);
    assert!(
        !defs.is_empty() && defs.len() < HELPER_DEFINITION_BUDGET,
        "the fixture must produce helpers and stay below the budget, got {}",
        defs.len()
    );
    assert_helpers_resolve("below the budget", &artifact);
    assert_eq!(
        artifact
            .emission
            .event_count(TraversalEventKind::HelperCapOmission),
        0,
        "nothing was refused, so nothing may be reported as refused"
    );
    assert!(
        !artifact.source.contains("helper budget exhausted"),
        "an artifact that omitted nothing carries no omission marker"
    );
    assert_event_keys("below the budget", &ir, &artifact);
}

#[test]
fn helper_calls_at_the_budget_all_resolve() {
    let ir = fan_in(9102, BLOCKS_AT_HELPER_BUDGET);
    let artifact = artifact_of(&ir);

    assert_eq!(
        definition_ids(&artifact).len(),
        HELPER_DEFINITION_BUDGET,
        "the fixture must reach the budget exactly"
    );
    assert_helpers_resolve("at the budget", &artifact);
    assert_eq!(
        artifact
            .emission
            .event_count(TraversalEventKind::HelperCapOmission),
        0,
        "reaching the budget is not exceeding it"
    );
    assert!(
        !artifact.source.contains("helper budget exhausted"),
        "no helper was refused at the budget"
    );
}

#[test]
fn helper_calls_above_the_budget_become_explicit_omissions() {
    let ir = fan_in(9103, BLOCKS_PAST_HELPER_BUDGET);
    let artifact = artifact_of(&ir);

    assert_eq!(
        definition_ids(&artifact).len(),
        HELPER_DEFINITION_BUDGET,
        "the budget bounds the definitions"
    );
    assert_helpers_resolve("above the budget", &artifact);
    assert_event_keys("above the budget", &ir, &artifact);

    let refused = events_of(&artifact, TraversalEventKind::HelperCapOmission);
    assert!(
        !refused.is_empty(),
        "a graph past the budget must report the helpers it refused"
    );
    for event in &refused {
        let TraversalTarget::Helper { id } = event.target else {
            panic!("a helper-cap omission targets a helper, not a block");
        };
        assert!(
            artifact.source.contains(&format!(
                "// omitted path to block {id}: helper budget exhausted"
            )),
            "the refused block {id} has no marker in the artifact"
        );
        assert!(
            artifact
                .source
                .contains(&format!("omitted complex paths: block {id}")),
            "the summary must name the refused block {id}"
        );
        assert!(
            !artifact.source.contains(&format!("return _block_{id}();")),
            "block {id} has no definition, so no call to it may survive"
        );
    }

    // The defect this replaces: a referenced helper rewritten into an exit the
    // graph does not contain.
    let markers = artifact.source.matches("// omitted path to block").count();
    assert_eq!(
        markers,
        refused.len(),
        "one marker per refused call site, and one event per marker"
    );
}

/// A visit omission is not a block disposition: the block it names is emitted
/// elsewhere in the same artifact.
#[test]
fn a_visit_omission_can_name_a_block_that_was_also_emitted() {
    // A complete binary tree whose leaves all reach one padded sink, with two
    // leaves entering a two-block cycle from different sides, so the graph is
    // irreducible and the DFS walk is the one that runs.
    let sink = 63usize;
    let (left, right) = (64usize, 65usize);
    let mut succs: Vec<Vec<usize>> = Vec::new();
    for id in 0..sink {
        if id < 31 {
            succs.push(vec![2 * id + 1, 2 * id + 2]);
        } else if id == 31 {
            succs.push(vec![left]);
        } else if id == 32 {
            succs.push(vec![right]);
        } else {
            succs.push(vec![sink]);
        }
    }
    succs.push(Vec::new());
    succs.push(vec![sink, right]);
    succs.push(vec![sink, left]);
    let mut ir = graph(9104, &succs);
    // Four instructions, so the sink's budget is the 24 of a shared block rather
    // than the 48 of a short tail.
    let pad_va = va(sink) + 8;
    let block = ir.blocks.iter_mut().find(|b| b.id == sink).expect("sink");
    let ret = block.instrs.pop().expect("terminator");
    for offset in 0..2u64 {
        block.instrs.push(instr(
            pad_va + offset * 4,
            IROp::RuntimeCheck,
            "cmp x0, #0".to_string(),
            String::new(),
        ));
    }
    block.instrs.push(ret);

    assert!(
        Regions::build(&ir).is_none(),
        "the fixture is irreducible, so the DFS walk is the one under test"
    );
    let artifact = artifact_of(&ir);

    let visits = events_of(&artifact, TraversalEventKind::DfsVisitOmission);
    assert!(
        !visits.is_empty(),
        "a block reached past its visit budget must record a visit omission"
    );
    let sink_va = va(sink);
    assert!(
        visits
            .iter()
            .all(|e| e.target == TraversalTarget::Block { start_va: sink_va }),
        "the shared sink is the only block whose budget this fixture exhausts"
    );
    assert!(
        artifact
            .source
            .contains(&format!("fn_{:#x}", marker_va(sink))),
        "the same block is also emitted, so an event is not a disposition"
    );
    assert_event_keys("visit omission", &ir, &artifact);
    assert_helpers_resolve("visit omission", &artifact);
}

#[test]
fn a_depth_omission_names_the_edge_it_did_not_walk() {
    let ir = fan_in(9105, 128);
    let artifact = artifact_of(&ir);

    let depth_events = events_of(&artifact, TraversalEventKind::DfsDepthOmission);
    assert!(
        !depth_events.is_empty(),
        "a spine past the depth budget must record depth omissions"
    );
    assert_event_keys("depth omission", &ir, &artifact);
    for event in &depth_events {
        let TraversalTarget::Block { start_va } = event.target else {
            panic!("a depth omission targets a block");
        };
        let source = ir
            .blocks
            .iter()
            .find(|b| b.start_va == event.source_start_va)
            .expect("source block");
        let target = ir
            .blocks
            .iter()
            .find(|b| b.start_va == start_va)
            .expect("target block");
        assert!(
            source.succs.contains(&target.id),
            "an omission names an edge of the graph, not an arbitrary pair"
        );
    }
    assert_helpers_resolve("depth omission", &artifact);
}

/// Every reachable block is either emitted or named by an omission event, and
/// a block that is neither is only ever reached through one that is named.
///
/// That is what "bounded omission" means: the walk stops at a named block, and
/// everything behind it is accounted for by that one name rather than
/// disappearing silently.
#[test]
fn every_reachable_block_is_emitted_or_named_by_an_omission() {
    for (function_id, blocks) in [(9106u64, 128usize), (9107, BLOCKS_PAST_HELPER_BUDGET)] {
        let ir = fan_in(function_id, blocks);
        let artifact = artifact_of(&ir);
        let named: BTreeSet<usize> = artifact
            .emission
            .events()
            .iter()
            .filter_map(|e| match e.target {
                TraversalTarget::Helper { id } => Some(id),
                TraversalTarget::Block { start_va } => ir
                    .blocks
                    .iter()
                    .find(|b| b.start_va == start_va)
                    .map(|b| b.id),
            })
            .collect();
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut stack = vec![0usize];
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let emitted = artifact
                .source
                .contains(&format!("fn_{:#x}", marker_va(id)));
            assert!(
                emitted || named.contains(&id),
                "block {id} of {function_id} is neither emitted nor named by an omission"
            );
            if named.contains(&id) && !emitted {
                // The omission bounds the walk here: what lies behind it is
                // accounted for by this one name.
                continue;
            }
            let block = ir.blocks.iter().find(|b| b.id == id).expect("block");
            stack.extend(block.succs.iter().copied());
        }
        assert!(
            artifact.source.contains(&format!("fn_{:#x}", marker_va(0))),
            "the entry block is emitted, so the walk above is not vacuous"
        );
    }
}
