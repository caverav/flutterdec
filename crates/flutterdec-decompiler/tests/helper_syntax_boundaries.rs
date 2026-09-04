//! Recovered data cannot change what the artifact says about its helpers.
//!
//! An emitted function carries two kinds of text: code the emitter wrote, and
//! bytes it recovered from the snapshot and quoted. Only the first is structure.
//! The helper scan used to count `{` and `}` on every line regardless, so one
//! brace inside a recovered pool string ended a helper body early: the
//! definition stopped being seen as a definition, its live call was rewritten
//! into a "helper budget exhausted" note, and a `HelperCapOmission` event was
//! recorded for a budget that had 54 definitions left. Separately, a recovered
//! symbol spelled exactly like a generated helper put a name nothing generated
//! into the namespace the accounting reads.
//!
//! Every assertion here is a plain `assert!`, not a `debug_assert!`, because the
//! artifacts that ship are built in release, which is precisely where the
//! emitter's own invariants are compiled out.

use flutterdec_decompiler::{
    emit_pseudocode_with_pool_hints, PseudocodeArtifact, TraversalEventKind, TraversalTarget,
};
use flutterdec_ir::{rebuild_edges, BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::{BTreeSet, HashMap};

/// Mirrors `HELPER_DEFINITION_BUDGET`. Not imported: it is crate-private, and a
/// public fixture that could not see it is exactly the reader this budget has.
const HELPER_DEFINITION_BUDGET: usize = 64;

const POOL_SLOT: u64 = 11;
const SYMBOL_VA: u64 = 0x7_0000;

fn ins(va: u64, op: IROp, src: &str, target: &str) -> LlirInstr {
    LlirInstr {
        va,
        op,
        src: src.to_string(),
        target: target.to_string(),
    }
}

/// A ladder of `steps` nested conditionals, each with its own returning leaf.
///
/// Nothing rejoins, so the DFS walk runs out of depth and defers the leaves into
/// helper bodies. Every spine block loads one pool slot and reads it, so the
/// recovered string is rendered on a line *inside* a helper body, which is the
/// only place a stray brace can move a helper's extent: the scan recomputes
/// depth from each header, so one in the main body is harmless.
///
/// When `call_symbol` is set the spine also calls a fixed address, so a name
/// from the symbol table reaches the artifact.
fn ladder(function_id: u64, steps: usize, call_symbol: bool) -> FunctionIr {
    let base = 0x5_0000u64;
    let mut blocks = Vec::new();
    for i in 0..steps {
        let spine = i * 2;
        let leaf = i * 2 + 1;
        let spine_va = base + (spine as u64) * 0x20;
        let leaf_va = base + (leaf as u64) * 0x20;
        // The pool load lands the recovered string on a line as a literal, and
        // the field read off the same pooled object lands it a second time
        // inside a slot comment. Both spellings are then rendered inside a
        // helper body, which is where a stray delimiter can move a helper's
        // extent.
        let mut instrs = vec![
            ins(
                spine_va,
                IROp::LoadPool,
                "x1",
                &format!("pool[{POOL_SLOT}]"),
            ),
            ins(spine_va + 4, IROp::Other, "stur x1, [x29, #-8]", ""),
            ins(spine_va + 8, IROp::Other, "ldr x2, [x1, #0x10]", ""),
            ins(spine_va + 12, IROp::Other, "stur x2, [x29, #-16]", ""),
        ];
        if call_symbol {
            instrs.push(ins(
                spine_va + 16,
                IROp::Call,
                &format!("bl #{SYMBOL_VA:#x}"),
                &format!("{SYMBOL_VA:#x}"),
            ));
        }
        instrs.push(ins(
            spine_va + 20,
            IROp::Branch,
            "cbnz x1",
            &format!("{leaf_va:#x}"),
        ));
        blocks.push(BasicBlock {
            id: spine,
            start_va: spine_va,
            instrs,
            succs: if i + 1 < steps {
                vec![leaf, spine + 2]
            } else {
                vec![leaf]
            },
            preds: Vec::new(),
        });
        blocks.push(BasicBlock {
            id: leaf,
            start_va: leaf_va,
            instrs: vec![
                ins(leaf_va, IROp::Other, "mov x0, x2", ""),
                ins(leaf_va + 4, IROp::Return, "ret", ""),
            ],
            succs: Vec::new(),
            preds: Vec::new(),
        });
    }
    rebuild_edges(&mut blocks);
    FunctionIr {
        function_id,
        name: format!("ladder{function_id}"),
        entry_va: base,
        blocks,
    }
}

fn emit(ir: &FunctionIr, recovered: &str, symbol: Option<&str>) -> PseudocodeArtifact {
    let mut hints = HashMap::new();
    hints.insert(POOL_SLOT, recovered.to_string());
    let mut symbols = HashMap::new();
    if let Some(symbol) = symbol {
        symbols.insert(SYMBOL_VA, symbol.to_string());
    }
    emit_pseudocode_with_pool_hints(ir, &symbols, &hints)
}

fn call_ids(source: &str) -> BTreeSet<usize> {
    source
        .lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix("return _block_")?
                .strip_suffix("();")?
                .parse()
                .ok()
        })
        .collect()
}

fn definitions(source: &str) -> Vec<usize> {
    source
        .lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix("dynamic _block_")?
                .strip_suffix("() {")?
                .parse()
                .ok()
        })
        .collect()
}

fn cap_note_ids(source: &str) -> Vec<usize> {
    source
        .lines()
        .filter(|l| l.contains("helper budget exhausted"))
        .filter_map(|l| {
            l.trim()
                .strip_prefix("// omitted path to block ")?
                .split(':')
                .next()?
                .parse()
                .ok()
        })
        .collect()
}

fn cap_event_ids(artifact: &PseudocodeArtifact) -> Vec<usize> {
    artifact
        .emission
        .events()
        .iter()
        .filter(|e| e.kind == TraversalEventKind::HelperCapOmission)
        .filter_map(|e| match e.target {
            TraversalTarget::Helper { id } => Some(id),
            TraversalTarget::Block { .. } => None,
        })
        .collect()
}

/// Everything the contract asks of helper structure, read off the finished text.
///
/// `expected_cap_events` is stated by the caller rather than derived from the
/// artifact, so a fixture that quietly stopped reaching a cap fails instead of
/// agreeing with itself.
fn assert_helper_structure(
    label: &str,
    artifact: &PseudocodeArtifact,
    expected_cap_events: usize,
    recovered: &str,
) {
    let source = &artifact.source;
    let calls = call_ids(source);
    let defs = definitions(source);
    let def_set: BTreeSet<usize> = defs.iter().copied().collect();

    assert_eq!(
        defs.len(),
        def_set.len(),
        "{label}: a helper is defined more than once: {defs:?}"
    );
    assert_eq!(
        calls,
        def_set,
        "{label}: helper calls and helper definitions must be the same set \
         ({} calls, {} definitions)",
        calls.len(),
        def_set.len()
    );
    assert!(
        def_set.len() <= HELPER_DEFINITION_BUDGET,
        "{label}: {} definitions past the budget",
        def_set.len()
    );

    // An omission note and its event are two spellings of one fact, so they
    // agree in count and in the blocks they name, and neither may name a block
    // the artifact went on to define.
    let mut notes = cap_note_ids(source);
    let mut events = cap_event_ids(artifact);
    notes.sort_unstable();
    events.sort_unstable();
    assert_eq!(
        notes, events,
        "{label}: omission notes and cap events must name the same blocks"
    );
    assert_eq!(
        events.len(),
        expected_cap_events,
        "{label}: expected {expected_cap_events} cap events, artifact has {}",
        events.len()
    );
    for id in &notes {
        assert!(
            !def_set.contains(id),
            "{label}: block {id} is defined and simultaneously reported as never emitted"
        );
    }

    // Nothing else may wear the generated spelling. The rendered pool literal
    // is blanked first, delimiters included: bytes the emitter quoted are the
    // snapshot speaking, and the point of the fix is that they name no helper.
    let quoted = format!("\"{}\"", escaped(recovered));
    for (n, line) in source.lines().enumerate() {
        let line = line.replace(&quoted, "\"<recovered>\"");
        if !line.contains("_block_") {
            continue;
        }
        let t = line.trim();
        let structural = t.starts_with("dynamic _block_") && t.ends_with("() {")
            || t.starts_with("return _block_") && t.ends_with("();")
            || t.starts_with("// omitted path to block ")
            || t.starts_with("// omitted complex paths: ");
        assert!(
            structural,
            "{label}: line {} spells `_block_` outside helper structure: {line}",
            n + 1
        );
    }
}

/// Recovered strings that all used to move helper structure, and the balanced
/// control that never did.
///
/// `value: ${` is not a crafted input: it is an ordinary Dart interpolation
/// prefix, the sort of fragment a real snapshot's string pool is full of.
const RECOVERED_BAIT: [(&str, &str); 10] = [
    ("balanced control", "value: {}"),
    ("unmatched open", "value: ${"),
    ("unmatched close", "value: }"),
    ("bare open", "{"),
    ("bare close", "}"),
    ("quote", "he said \"stop\" and left"),
    ("comment terminator", "*/ } /* still data"),
    ("line comment", "// } not a comment"),
    ("helper definition text", "dynamic _block_999() {"),
    ("helper call text", "return _block_7();"),
];

/// VAL-EMIT-001: no recovered string changes a helper's extent, and none mints
/// an omission for a budget that was never reached.
#[test]
fn recovered_text_inside_a_helper_body_never_moves_helper_structure() {
    for (index, (label, recovered)) in RECOVERED_BAIT.iter().enumerate() {
        let ir = ladder(9_300 + index as u64, 70, false);
        let artifact = emit(&ir, recovered, None);
        assert!(
            !definitions(&artifact.source).is_empty(),
            "{label}: the fixture must actually generate helpers"
        );
        // Both spellings, and both of them past the first helper header, or the
        // case is not exercising a helper body at all.
        let first_helper = artifact
            .source
            .lines()
            .position(|l| l.trim().starts_with("dynamic _block_"))
            .expect("a helper definition");
        let literal = format!("\"{}\"", escaped(recovered));
        let slot_comment = format!("pool[{POOL_SLOT} /* {literal} */]");
        for spelling in [&literal, &slot_comment] {
            assert!(
                artifact
                    .source
                    .lines()
                    .skip(first_helper)
                    .any(|l| l.contains(spelling.as_str())),
                "{label}: `{spelling}` must be rendered inside a helper body"
            );
        }
        assert_helper_structure(label, &artifact, 0, recovered);
    }
}

/// The whole artifact is the same whichever recovered string is in the pool,
/// once the string itself is discounted: structure does not depend on data.
#[test]
fn recovered_text_changes_the_quoted_bytes_and_nothing_else() {
    let control = emit(&ladder(9_320, 70, false), RECOVERED_BAIT[0].1, None);
    // Only the rendered literal is blanked, delimiters included, so a bait
    // string that is itself a brace cannot blank the emitter's own braces.
    let shape = |artifact: &PseudocodeArtifact, recovered: &str| {
        artifact
            .source
            .replace(&format!("\"{}\"", escaped(recovered)), "\"<recovered>\"")
    };
    let expected = shape(&control, RECOVERED_BAIT[0].1);
    for (label, recovered) in RECOVERED_BAIT.iter().skip(1) {
        let artifact = emit(&ladder(9_320, 70, false), recovered, None);
        assert_eq!(
            shape(&artifact, recovered),
            expected,
            "{label}: the recovered string moved something other than itself"
        );
    }
}

/// How `escape_hint_text` renders a value, so the comparison above can blank the
/// quoted form as well as the raw one.
fn escaped(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    let mut out = String::new();
    for (index, c) in chars.iter().copied().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            '/' if chars.get(index.wrapping_sub(1)) == Some(&'*')
                || chars.get(index + 1) == Some(&'/') =>
            {
                out.push_str("\\u{2f}")
            }
            _ => out.push(c),
        }
    }
    out
}

/// VAL-EMIT-002: the helper budget still refuses at the same place, and every
/// refusal is a real one.
///
/// The ladder is sized past the budget with an unbalanced recovered string in
/// every helper body: the cap has to fire for the blocks it actually refused and
/// for no others.
#[test]
fn the_helper_budget_still_caps_with_recovered_braces_in_every_body() {
    let ir = ladder(9_340, 1_600, false);
    let capped = emit(&ir, "value: ${", None);
    let events = cap_event_ids(&capped).len();
    assert_eq!(
        definitions(&capped.source).len(),
        HELPER_DEFINITION_BUDGET,
        "the fixture must fill the helper budget"
    );
    assert!(
        events > 0,
        "the fixture must cross the helper budget, so a real cap event exists"
    );
    assert_helper_structure("capped with braces", &capped, events, "value: ${");

    // Same graph, balanced string: the budget refuses the same blocks, so the
    // recovered bytes changed nothing about where the cap fell.
    let control = emit(&ir, "value: {}", None);
    let mut with_braces = cap_note_ids(&capped.source);
    let mut without = cap_note_ids(&control.source);
    with_braces.sort_unstable();
    without.sort_unstable();
    assert_eq!(
        with_braces, without,
        "the recovered string moved which blocks the budget refused"
    );
    assert_helper_structure(
        "capped control",
        &control,
        cap_event_ids(&control).len(),
        "value: {}",
    );
}

/// VAL-EMIT-001, second defect: a recovered symbol named exactly like a
/// generated helper stays out of the generated namespace.
#[test]
fn a_recovered_symbol_cannot_be_spelled_like_a_generated_helper() {
    for spoof in ["_block_999", "_block_7", "_block_0"] {
        let ir = ladder(9_360, 70, true);
        let artifact = emit(&ir, "value: {}", Some(spoof));
        assert!(
            artifact.source.contains("recovered_block"),
            "the fixture must actually render the recovered symbol"
        );
        assert!(
            !artifact.source.contains(&format!("{spoof}(")),
            "`{spoof}` reached the artifact as a call"
        );
        assert_helper_structure(&format!("symbol spoof {spoof}"), &artifact, 0, "value: {}");
    }
}

/// A recovered symbol that only *resembles* a helper keeps its own name: the
/// reservation covers the generated spelling and nothing wider.
#[test]
fn a_recovered_symbol_that_is_not_a_helper_spelling_survives_intact() {
    for kept in ["_block_", "_block_12a", "_blocks_3", "myBlock_3"] {
        let ir = ladder(9_380, 4, true);
        let artifact = emit(&ir, "value: {}", Some(kept));
        assert!(
            artifact.source.contains(kept),
            "`{kept}` is not a generated helper spelling and must survive"
        );
    }
}
