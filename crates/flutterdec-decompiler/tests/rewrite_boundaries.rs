//! Text rewrites have to stop at syntax boundaries.
//!
//! The emitter finishes a body by rewriting its text: expressions are cleaned up,
//! repeated shapes are aliased, and identifiers are renamed. Every one of those is
//! a byte-level substitution over a line that also carries recovered program data
//! and the emitter's own comments, so each rewrite is only correct while it stays
//! inside code.
//!
//! These cases drive the public emitter with a recovered pool string that contains
//! the exact text every rewrite looks for, plus symbol names that have an emitter
//! identifier as a prefix. The string is the strongest fixture available: it is
//! real program data, it reaches the artifact as a literal and as a comment, and a
//! rewrite that edits it has changed what the binary said.

use flutterdec_decompiler::{emit_pseudocode, emit_pseudocode_with_pool_hints};
use flutterdec_ir::{BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::HashMap;

/// One recovered string carrying the pattern of every rewrite in the emitter: the
/// compressed-pointer strip, the stack-slot alias, the minus-one alias, an
/// argument and a register name, a wrapped member access, the class-id rewrite, a
/// negated comparison, an escaped quote, and a non-ASCII character.
const HAZARD: &str = "peer collected (nullptr). \
     \"x\" + x28 sp[0x10] (value3 - 1) arg0 x28 ((obj)).f7 \
     bitField(a, 0xc, 0x14) !((p != q)) Mo\u{017e}ete";

fn ins(va: u64, op: IROp, src: &str, target: &str) -> LlirInstr {
    LlirInstr {
        va,
        op,
        src: src.to_string(),
        target: target.to_string(),
    }
}

fn one_block(function_id: u64, name: &str, base_va: u64, instrs: Vec<LlirInstr>) -> FunctionIr {
    FunctionIr {
        function_id,
        name: name.to_string(),
        entry_va: base_va,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: base_va,
            instrs,
            succs: Vec::new(),
            preds: Vec::new(),
        }],
    }
}

/// The literal as the emitter writes it: a quote inside a recovered string is
/// escaped, so the expected text is escaped the same way rather than assumed.
fn quoted(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Delimiters balance and quotes pair on every line.
///
/// A rewrite that runs off the end of a literal or a comment leaves an unbalanced
/// line, so this holds for every artifact here whatever else the case asserts.
fn assert_well_formed(source: &str) {
    for line in source.lines() {
        let mut parens = 0i32;
        let mut brackets = 0i32;
        let mut quotes = 0usize;
        let mut in_string = false;
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' if in_string => i += 1,
                b'"' => {
                    quotes += 1;
                    in_string = !in_string;
                }
                b'(' if !in_string => parens += 1,
                b')' if !in_string => parens -= 1,
                b'[' if !in_string => brackets += 1,
                b']' if !in_string => brackets -= 1,
                _ => {}
            }
            i += 1;
        }
        assert_eq!(parens, 0, "unbalanced parentheses:\n{line}");
        assert_eq!(brackets, 0, "unbalanced brackets:\n{line}");
        assert_eq!(quotes % 2, 0, "unpaired quote:\n{line}");
        assert!(!in_string, "line ends inside a string:\n{line}");
    }
}

/// A recovered string reaches the artifact twice over: as a literal where its
/// value is used, and inside a slot comment where the pooled object is
/// dereferenced. Both are program data and both must arrive byte for byte.
#[test]
fn a_recovered_string_and_its_slot_comment_survive_every_rewrite() {
    let ir = one_block(
        7001,
        "poolHazard",
        0x1000,
        vec![
            ins(0x1000, IROp::LoadPool, "x1", "pool[40]"),
            ins(0x1004, IROp::Other, "stur x1, [x29, #-8]", ""),
            // A field read off the pooled object keeps the slot and puts the
            // recovered text in a comment instead of a literal.
            ins(0x1008, IROp::Other, "ldr x2, [x1, #0x10]", ""),
            ins(0x100c, IROp::Other, "stur x2, [x29, #-16]", ""),
            ins(0x1010, IROp::Other, "mov x0, x1", ""),
            ins(0x1014, IROp::Return, "ret", ""),
        ],
    );
    let mut pool = HashMap::new();
    pool.insert(40u64, HAZARD.to_string());
    let source = emit_pseudocode_with_pool_hints(&ir, &HashMap::new(), &pool).source;
    assert_well_formed(&source);

    let literal = quoted(HAZARD);
    assert_eq!(
        source.matches(&format!("{literal} /* pool[40] */")).count(),
        2,
        "the literal must reach both of its value uses verbatim:\n{source}"
    );
    assert_eq!(
        source.matches(&format!("pool[40 /* {literal} */]")).count(),
        1,
        "the slot comment must carry the same bytes:\n{source}"
    );
    assert!(
        source.contains("\\\"x\\\""),
        "an escaped quote stays escaped:\n{source}"
    );
    assert!(
        source.contains("Mo\u{017e}ete"),
        "a non-ASCII character is not re-encoded:\n{source}"
    );

    // Each of these is what one rewrite would have left behind. Named one by one,
    // because a single "unchanged" assertion says nothing about which rewrite
    // crossed the boundary.
    for damage in [
        // the compressed-pointer strip
        "peer collected (nullptr). \\\"x\\\" sp[0x10]",
        // the wrapped member-access simplifier
        "obj.f7 bitField",
        "(obj).f7",
        // the class-id rewrite
        "classId(a)",
        // the negated-comparison rewrite
        "(p == q)",
        // the argument and register renames
        "slot0 x28",
        "arg0 reg28",
        // the stack-slot and minus-one aliases
        "stackSlot0x10",
        "codePoint",
    ] {
        assert!(
            !source.contains(damage),
            "a rewrite reached inside the recovered text and left `{damage}`:\n{source}"
        );
    }
    // The one member access outside the literal did simplify, so the rewrites are
    // still doing their job on code.
    assert!(
        source.contains("*/].f16;"),
        "the field read off the pooled object is still emitted:\n{source}"
    );
}

/// An identifier the emitter renames is a whole token, never a prefix.
///
/// `arg0Helper` and `reg3Helper` are recovered symbol names that begin with the
/// two identifier families the naming pass rewrites: the argument slots and the
/// unresolved register spellings.
#[test]
fn an_identifier_prefix_is_not_a_rename_target() {
    let ir = one_block(
        7002,
        "prefixNames",
        0x2000,
        vec![
            ins(0x2000, IROp::Other, "mov x3, x1", ""),
            ins(0x2004, IROp::Call, "bl #0x6100", "#0x6100"),
            ins(0x2008, IROp::Call, "bl #0x6200", "#0x6200"),
            ins(0x200c, IROp::Other, "stur x3, [x29, #-8]", ""),
            ins(0x2010, IROp::Return, "ret", ""),
        ],
    );
    let mut symbols = HashMap::new();
    symbols.insert(0x6100u64, "arg0Helper".to_string());
    symbols.insert(0x6200u64, "reg3Helper".to_string());
    let source = emit_pseudocode(&ir, &symbols).source;
    assert_well_formed(&source);

    assert!(
        source.contains("arg0Helper("),
        "a callee whose name starts with an argument name is not renamed:\n{source}"
    );
    assert!(
        source.contains("reg3Helper("),
        "a callee whose name starts with a register name is not renamed:\n{source}"
    );
    // The tokens themselves were renamed, so the case is about the boundary and
    // not about the renames being off.
    assert!(
        source.contains("dynamic prefixNames(dynamic slot0,"),
        "the argument itself is renamed:\n{source}"
    );
    assert!(
        source.contains("= reg3;"),
        "the register itself renders through its alias:\n{source}"
    );
    assert!(
        !source.contains("slot0Helper") && !source.contains("Helper3"),
        "no substring of a name was replaced:\n{source}"
    );
}

/// Shifts, comparisons and conditional values keep the grouping the machine
/// computed.
///
/// `asr #1` is a shift of the operand, not of the comparison, and a conditional
/// value composes into a larger expression, so both need their own parentheses.
#[test]
fn shifts_comparisons_and_conditionals_keep_their_grouping() {
    let ir = one_block(
        7003,
        "shiftsAndCmp",
        0x3000,
        vec![
            ins(0x3000, IROp::Other, "cmp x3, x1, asr #1", ""),
            ins(0x3004, IROp::Other, "cset x0, ne", ""),
            ins(0x3008, IROp::Other, "lsr x4, x1, #2", ""),
            ins(0x300c, IROp::Other, "orr x5, x4, x0", ""),
            ins(0x3010, IROp::Other, "stur x5, [x29, #-8]", ""),
            ins(0x3014, IROp::Return, "ret", ""),
        ],
    );
    let source = emit_pseudocode(&ir, &HashMap::new()).source;
    assert_well_formed(&source);

    assert!(
        source.contains("= ((slot0 >>> 2) | ((slot2 != (slot0 >> 1)) ? 1 : 0));"),
        "every operand keeps the parentheses its precedence needs:\n{source}"
    );
}

/// A nested call is one expression, and nothing rewrites its inner call away.
#[test]
fn a_nested_call_argument_keeps_its_own_call() {
    let ir = one_block(
        7004,
        "nestedCalls",
        0x4000,
        vec![
            ins(0x4000, IROp::Call, "bl #0x7100", "#0x7100"),
            // The result of the first call is the argument of the second.
            ins(0x4004, IROp::Other, "mov x1, x0", ""),
            ins(0x4008, IROp::Call, "bl #0x7200", "#0x7200"),
            ins(0x400c, IROp::Return, "ret", ""),
        ],
    );
    let mut symbols = HashMap::new();
    symbols.insert(0x7100u64, "inner".to_string());
    symbols.insert(0x7200u64, "outer".to_string());
    let source = emit_pseudocode(&ir, &symbols).source;
    assert_well_formed(&source);

    assert!(
        source.contains("final t1 = inner("),
        "the inner call is emitted:\n{source}"
    );
    assert!(
        source.contains("outer(t1"),
        "the outer call takes the inner result as an argument:\n{source}"
    );
}
