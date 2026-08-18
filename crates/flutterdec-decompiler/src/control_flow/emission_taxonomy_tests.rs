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

    // The output around the first omitted helper, read line by line: the marker
    // sits where the call was, at the call's indentation, inside a body, and the
    // lines on either side of it are not a fabricated exit.
    let lines = source_lines(&artifact);
    let first = lines
        .iter()
        .position(|l| l.trim_start().starts_with("// omitted path to block"))
        .expect("an omission marker");
    let marker = &lines[first];
    assert!(
        marker.starts_with("  ") && marker.trim_start().starts_with("//"),
        "the marker is an indented comment, not a statement: {marker:?}"
    );
    let before = &lines[first - 1];
    let after = lines.get(first + 1).map(String::as_str).unwrap_or("");
    assert!(
        before.trim() != "return null;" && after.trim() != "return null;",
        "the omission must not be padded with the return it replaced:\n{before}\n{marker}\n{after}"
    );
    assert!(
        lines[..first].iter().any(|l| l.starts_with("dynamic ")),
        "the marker is inside a function body"
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


// One fixture per primary decline cause, then the two things a decline owes:
// the right cause, and a body indistinguishable from the DFS walk's own.

/// A two-block cycle entered from two sides, which no dominator makes a loop.
fn irreducible_fixture(function_id: u64) -> FunctionIr {
    graph(function_id, &[vec![1, 2], vec![2, 3], vec![1, 3], Vec::new()])
}

/// A reachable block with three successors: the walk renders a taken arm and one
/// not-taken arm, so the third edge has no rendering at all.
fn unsupported_region_fixture(function_id: u64) -> FunctionIr {
    let mut ir = graph(function_id, &[vec![1, 2], Vec::new(), Vec::new()]);
    if let Some(entry) = ir.blocks.iter_mut().find(|b| b.id == 0) {
        entry.succs = vec![1, 2, 3];
    }
    ir.blocks.push(BasicBlock {
        id: 3,
        start_va: va(3),
        instrs: vec![instr(va(3), IROp::Return, "ret".to_string(), String::new())],
        succs: Vec::new(),
        preds: vec![0],
    });
    ir
}

/// A spine whose blocks all branch into one shared region larger than the repeat
/// budget, and whose last block returns without entering it, so the region is
/// nobody's follow node and would have to be repeated.
fn repeat_budget_fixture(function_id: u64) -> FunctionIr {
    let spine = 6usize;
    let region = 17usize;
    let mut succs: Vec<Vec<usize>> = Vec::new();
    for i in 0..spine {
        if i + 1 < spine {
            succs.push(vec![i + 1, spine]);
        } else {
            succs.push(Vec::new());
        }
    }
    for r in 0..region {
        if r + 1 < region {
            succs.push(vec![spine + r + 1]);
        } else {
            succs.push(Vec::new());
        }
    }
    graph(function_id, &succs)
}

/// A chain of conditionals whose arms never rejoin, so every one of them nests
/// inside the last: region depth grows by one per block.
fn depth_budget_fixture(function_id: u64) -> FunctionIr {
    let spine = STRUCTURED_MAX_DEPTH + 6;
    let mut succs: Vec<Vec<usize>> = Vec::new();
    for i in 0..spine {
        succs.push(vec![i + 1, spine + i]);
    }
    succs.push(Vec::new());
    for _ in 0..spine {
        succs.push(Vec::new());
    }
    graph(function_id, &succs)
}

/// A jump that leaves the function while the graph still records a successor:
/// the walk ends there, so a reachable block is never emitted.
fn coverage_mismatch_fixture(function_id: u64) -> FunctionIr {
    let mut ir = graph(function_id, &[vec![1], Vec::new()]);
    if let Some(entry) = ir.blocks.iter_mut().find(|b| b.id == 0) {
        if let Some(terminator) = entry.instrs.last_mut() {
            terminator.src = "b #0x50000".to_string();
            terminator.target = "#0x50000".to_string();
        }
    }
    ir
}

/// A plain diamond, which structures. The control for everything below.
fn structured_fixture(function_id: u64) -> FunctionIr {
    graph(
        function_id,
        &[vec![1, 2], vec![3], vec![3], Vec::new()],
    )
}

fn declining_fixtures() -> Vec<(StructuredDeclineCause, FunctionIr)> {
    vec![
        (StructuredDeclineCause::Irreducible, irreducible_fixture(9201)),
        (
            StructuredDeclineCause::UnsupportedRegion,
            unsupported_region_fixture(9202),
        ),
        (
            StructuredDeclineCause::RepeatBudget,
            repeat_budget_fixture(9203),
        ),
        (
            StructuredDeclineCause::StructuredDepthBudget,
            depth_budget_fixture(9204),
        ),
        (
            StructuredDeclineCause::CoverageMismatch,
            coverage_mismatch_fixture(9205),
        ),
    ]
}

#[test]
fn every_primary_cause_has_a_fixture_and_exactly_one_verdict() {
    let mut seen: BTreeSet<StructuredDeclineCause> = BTreeSet::new();
    for (expected, ir) in declining_fixtures() {
        let artifact = artifact_of(&ir);
        let decline = artifact
            .emission
            .decline()
            .unwrap_or_else(|| panic!("{expected:?}: the fixture must decline"));
        assert_eq!(decline.cause, expected, "{expected:?}: primary cause");
        seen.insert(decline.cause);

        // Exactly one cause, and the generic count is their sum.
        let per_cause: usize = StructuredDeclineCause::ALL
            .iter()
            .map(|c| artifact.emission.cause_count(*c))
            .sum();
        assert_eq!(per_cause, 1, "{expected:?}: causes are disjoint");
        assert_eq!(
            artifact.emission.structured_declines(),
            per_cause,
            "{expected:?}: the decline count is derived from the causes"
        );
        assert_eq!(
            artifact.emission.rollbacks(),
            usize::from(expected.is_post_mutation()),
            "{expected:?}: only a post-mutation cause rolls anything back"
        );

        // Keyed by immutable block identity where the cause has one.
        match decline.block_start_va {
            Some(start_va) => assert!(
                ir.blocks.iter().any(|b| b.start_va == start_va),
                "{expected:?}: the key names no block of this function"
            ),
            None => assert_eq!(
                expected,
                StructuredDeclineCause::Irreducible,
                "{expected:?}: only a whole-function cause may have no block key"
            ),
        }
    }
    assert_eq!(
        seen.len(),
        StructuredDeclineCause::ALL.len(),
        "every primary cause needs a fixture"
    );
}

#[test]
fn a_structured_function_records_no_decline() {
    let ir = structured_fixture(9206);
    let artifact = artifact_of(&ir);
    assert_eq!(artifact.emission.decline(), None);
    assert_eq!(artifact.emission.structured_declines(), 0);
    assert_eq!(artifact.emission.rollbacks(), 0);
    // The differential below is only worth anything if the two walks can
    // differ at all.
    let direct = emit_pseudocode_direct_dfs(&ir, &HashMap::new());
    assert_ne!(
        direct.source, artifact.source,
        "a structured body must differ from the DFS body, or the comparison proves nothing"
    );
}

/// Everything about the emitter that emission may write, rendered in a stable
/// order. Two fingerprints that differ name the family that leaked.
fn fingerprint(emitter: &FuncEmitter) -> String {
    let sorted = |map: &HashMap<String, String>| {
        let mut rows: Vec<(&String, &String)> = map.iter().collect();
        rows.sort();
        format!("{rows:?}")
    };
    let visits = {
        let mut rows: Vec<(&usize, &usize)> = emitter.inline_visits.iter().collect();
        rows.sort();
        format!("{rows:?}")
    };
    let writes = {
        let mut rows: Vec<(usize, Vec<&String>)> = emitter
            .dfs_block_writes
            .iter()
            .map(|(id, regs)| {
                let mut regs: Vec<&String> = regs.iter().collect();
                regs.sort();
                (*id, regs)
            })
            .collect();
        rows.sort();
        format!("{rows:?}")
    };
    let clobbers = {
        let mut rows: Vec<&String> = emitter.state.call_clobbers.keys().collect();
        rows.sort();
        format!("{rows:?}")
    };
    let mut emitted: Vec<&usize> = emitter.emitted.iter().collect();
    emitted.sort();
    let mut structured: Vec<&usize> = emitter.structured_emitted.iter().collect();
    structured.sort();
    let mut loop_sites: Vec<&usize> = emitter.loop_annotation_sites.iter().collect();
    loop_sites.sort();
    let mut candidate_regs: Vec<(&usize, &Vec<String>)> =
        emitter.join_candidate_regs.iter().collect();
    candidate_regs.sort();
    let mut candidates: Vec<&(usize, String)> = emitter.join_candidates.keys().collect();
    candidates.sort();
    format!(
        "lines={:?}\nrender_lines={:?}\nregs={}\nselectors={}\nlast_cmp={:?}\nclobbers={}\n\
         counters={:?}\nlocals={:?}\nrenames={:?}\ncall_index={} snapshot_index={} rendering={}\n\
         structured={structured:?} loop_stack={:?} candidates={candidates:?} candidate_regs={candidate_regs:?}\n\
         join_anchors={:?}\ncall_anchors={:?}\nloop_sites={loop_sites:?} block_snapshots={:?}\n\
         call_prov={:?}\njoin_prov={:?}\nloop_prov={:?}\n\
         emitted={emitted:?} active={:?} visits={visits} omitted={:?} sources={:?} refused={:?}\n\
         back_edges={:?} loop_context={:?} dfs_preds={:?} writes={writes}\n\
         events={:?}",
        emitter.lines,
        emitter.render_lines,
        sorted(&emitter.state.reg_values),
        sorted(&emitter.state.selector_hints),
        emitter.state.last_cmp,
        clobbers,
        (
            emitter.placeholder_ifs,
            emitter.unresolved_cf,
            emitter.raw_register_calls,
            emitter.total_calls,
            emitter.indirect_calls,
            emitter.semantic_direct_calls,
            emitter.semantic_indirect_calls,
            emitter.dispatch_selector_calls,
            emitter.dispatch_table_calls,
            emitter.repeated_blocks,
            emitter.unlifted_instructions,
            emitter.target_va_symbol_calls,
        ),
        emitter.locals,
        emitter.identifier_renames,
        emitter.call_index,
        emitter.snapshot_index,
        emitter.rendering_call,
        emitter.loop_stack,
        emitter.join_annotation_anchors,
        emitter.call_annotation_anchors,
        emitter.block_snapshots,
        emitter.call_provenance,
        emitter.join_provenance,
        emitter.loop_provenance,
        emitter.active_stack,
        emitter.omitted_blocks,
        emitter.omission_sources,
        emitter.helper_cap_omitted,
        emitter.loop_back_edges,
        emitter.loop_context,
        emitter.dfs_preds,
        emitter.accounting.events(),
    )
}

/// Write to every state family, so a rollback that misses one is visible.
fn poison(emitter: &mut FuncEmitter) {
    emitter
        .state
        .reg_values
        .insert("x9".to_string(), "poisonValue".to_string());
    emitter
        .state
        .selector_hints
        .insert("x10".to_string(), "poisonSelector".to_string());
    emitter.state.last_cmp = Some(("x11".to_string(), "0".to_string()));
    emitter.state.call_clobbers.insert(
        "x12".to_string(),
        CallClobber {
            call_va: 0xdead,
            value: "poisonClobber".to_string(),
            snapshot_id: "poison-snapshot".to_string(),
        },
    );

    emitter.placeholder_ifs += 3;
    emitter.unresolved_cf += 5;
    emitter.total_calls += 7;
    emitter.repeated_blocks += 11;
    emitter.unlifted_instructions += 13;

    emitter.omitted_blocks.insert(0);
    emitter.omission_sources.insert(0, 0);
    emitter.helper_cap_omitted.insert(0);

    emitter.call_index = 41;
    emitter.snapshot_index = 17;
    emitter.locals.insert(-8, "poison_local".to_string());
    emitter
        .identifier_renames
        .push(("poison".to_string(), "renamed".to_string()));

    emitter.render_lines.push("  // poison render".to_string());
    emitter.join_annotation_anchors.push(JoinAnnotationAnchor {
        join: 0,
        candidate_regs: vec!["x9".to_string()],
        lines: vec!["  // poison anchor".to_string()],
    });
    emitter.call_annotation_anchors.push(CallAnnotationAnchor {
        call_va: 0xbeef,
        register: "x9".to_string(),
        value: "poisonValue".to_string(),
        snapshot_id: "poison-snapshot".to_string(),
        line_index: 0,
    });
    emitter.block_snapshots.push(BlockSnapshot {
        block: 0,
        reg_values: HashMap::new(),
    });

    for stream in [
        &mut emitter.call_provenance,
        &mut emitter.join_provenance,
        &mut emitter.loop_provenance,
    ] {
        stream.snapshots.push(ValueSnapshot {
            snapshot_id: "poison-snapshot".to_string(),
            site_key: SiteKey("block", 0),
            registers: vec![("x9".to_string(), "poisonValue".to_string())],
        });
    }

    emitter.emitted.insert(usize::MAX);
    emitter.structured_emitted.insert(usize::MAX);
    emitter.inline_visits.insert(usize::MAX, 3);
    emitter.loop_back_edges.insert(usize::MAX);
    emitter.loop_annotation_sites.insert(usize::MAX);
    emitter
        .dfs_block_writes
        .insert(usize::MAX, HashSet::from(["x9".to_string()]));
    emitter.accounting.record_event(
        TraversalEventKind::DfsVisitOmission,
        emitter.ir.function_id,
        0xfeed,
        TraversalTarget::Helper { id: usize::MAX },
    );
}

/// The state a structured attempt leaves behind, whatever it was handed.
fn attempt_on_poisoned_emitter(ir: &FunctionIr) -> (bool, String, String, EmissionAccounting) {
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(ir, &symbols);
    poison(&mut emitter);
    let before = fingerprint(&emitter);
    let took = emitter.try_emit_structured();
    let after = fingerprint(&emitter);
    (took, before, after, emitter.accounting.clone())
}

#[test]
fn a_preflight_decline_writes_nothing_at_all() {
    for (cause, ir) in declining_fixtures()
        .into_iter()
        .filter(|(cause, _)| cause.is_preflight())
    {
        let (took, before, after, accounting) = attempt_on_poisoned_emitter(&ir);
        assert!(!took, "{cause:?}: the fixture must decline");
        assert_eq!(
            before, after,
            "{cause:?}: a preflight decline may not write to emitter state"
        );
        assert_eq!(accounting.decline().map(|d| d.cause), Some(cause));
        assert_eq!(
            accounting.rollbacks(),
            0,
            "{cause:?}: nothing was written, so nothing was rolled back"
        );
    }
}

#[test]
fn a_post_mutation_decline_restores_every_state_family() {
    for (cause, ir) in declining_fixtures()
        .into_iter()
        .filter(|(cause, _)| cause.is_post_mutation())
    {
        let (took, before, after, accounting) = attempt_on_poisoned_emitter(&ir);
        assert!(!took, "{cause:?}: the fixture must decline");
        assert_eq!(
            before, after,
            "{cause:?}: the attempt left state behind that it wrote"
        );
        assert_eq!(accounting.decline().map(|d| d.cause), Some(cause));
        assert_eq!(
            accounting.rollbacks(),
            1,
            "{cause:?}: a post-mutation decline rolls its attempt back"
        );
    }
}

/// A declined function's artifact is the DFS walk's own, down to the byte, and
/// its counters and provenance are too. Only the cause accounting differs.
#[test]
fn a_declined_function_equals_direct_dfs() {
    let symbols = HashMap::new();
    for (cause, ir) in declining_fixtures() {
        let (auto, auto_provenance) = {
            let mut emitter = FuncEmitter::new(&ir, &symbols);
            poison(&mut emitter);
            emitter.emit_with_plan(EmissionPlan::Auto)
        };
        let (direct, direct_provenance) = {
            let mut emitter = FuncEmitter::new(&ir, &symbols);
            poison(&mut emitter);
            emitter.emit_with_plan(EmissionPlan::DirectDfs)
        };

        assert_eq!(auto.source, direct.source, "{cause:?}: body");
        assert_eq!(
            format!("{auto_provenance:?}"),
            format!("{direct_provenance:?}"),
            "{cause:?}: provenance"
        );
        for (name, left, right) in [
            ("placeholder_ifs", auto.placeholder_ifs, direct.placeholder_ifs),
            ("unresolved_cf", auto.unresolved_cf, direct.unresolved_cf),
            (
                "raw_register_calls",
                auto.raw_register_calls,
                direct.raw_register_calls,
            ),
            ("total_calls", auto.total_calls, direct.total_calls),
            ("indirect_calls", auto.indirect_calls, direct.indirect_calls),
            ("repeated_blocks", auto.repeated_blocks, direct.repeated_blocks),
            (
                "unlifted_instructions",
                auto.unlifted_instructions,
                direct.unlifted_instructions,
            ),
            (
                "target_va_symbol_calls",
                auto.target_va_symbol_calls,
                direct.target_va_symbol_calls,
            ),
        ] {
            assert_eq!(left, right, "{cause:?}: {name}");
        }
        assert_eq!(
            auto.emission.events(),
            direct.emission.events(),
            "{cause:?}: the same walk omits the same edges"
        );
        // The one difference, and it is accounting only.
        assert_eq!(auto.emission.decline().map(|d| d.cause), Some(cause));
        assert_eq!(direct.emission.decline(), None);
    }
}
