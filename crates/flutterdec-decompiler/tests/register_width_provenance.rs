//! A 32-bit read may only resolve a value the access actually produces.
//!
//! `w3` and `x3` are one machine register and one binding, so the width of the
//! write that produced the binding is what decides whether a 32-bit read of it
//! is that value. A `w` write zero-extends, leaving nothing above bit 31, so a
//! later `w` read yields exactly the bound value. Every other producer leaves
//! the high half live, and rendering such a binding at a `w` read states a
//! 64-bit value where the access yields 32 bits: `lsl x3, x2, #32` then
//! `mov w0, w3` reads zero for every input.
//!
//! Everything here goes through the public emitter and reads the generated
//! artifact, because the artifact is what a reader trusts. `regN` is the
//! emitter's spelling for a register with no value in hand and is the
//! whole-program unresolved counter as well: `quality.json`'s
//! `raw_register_name_refs` counts exactly these tokens (`quality.rs:17-25`), so
//! a read that degrades here is a read that reports there.
//!
//! The artifacts asserted below are exact and are the same in debug and in
//! `--release`: the lifter has no debug-only assertion, so a defect that shows
//! only in a release build would show here too.

use flutterdec_decompiler::{emit_pseudocode, emit_pseudocode_direct_dfs, PseudocodeArtifact};
use flutterdec_ir::{rebuild_edges, BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::HashMap;

fn op_of(src: &str) -> IROp {
    if src.starts_with("ret") {
        IROp::Return
    } else if src.starts_with("cbz") || src.starts_with("b.") {
        IROp::Branch
    } else {
        IROp::Other
    }
}

/// The branch target, which the emitter reads from the instruction rather than
/// from the successor list.
fn target_of(src: &str) -> String {
    match op_of(src) {
        IROp::Branch => src
            .split_whitespace()
            .next_back()
            .filter(|token| token.starts_with('#'))
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn block(id: usize, base_va: u64, srcs: &[&str], succs: Vec<usize>) -> BasicBlock {
    BasicBlock {
        id,
        start_va: base_va,
        instrs: srcs
            .iter()
            .enumerate()
            .map(|(index, src)| LlirInstr {
                va: base_va + 4 * index as u64,
                op: op_of(src),
                src: (*src).to_string(),
                target: target_of(src),
            })
            .collect(),
        succs,
        preds: Vec::new(),
    }
}

fn straight_line(name: &str, srcs: &[&str]) -> FunctionIr {
    FunctionIr {
        function_id: 4009,
        name: name.to_string(),
        entry_va: 0x1000,
        blocks: vec![block(0, 0x1000, srcs, Vec::new())],
    }
}

fn emit(ir: &FunctionIr) -> PseudocodeArtifact {
    emit_pseudocode(ir, &HashMap::new())
}

/// Occurrences of the unresolved spelling of `reg`, counted the way the quality
/// report counts them: as a whole token, so `reg1` never matches `reg12`.
fn unresolved_reads(source: &str, reg: &str) -> usize {
    let mut count = 0usize;
    let mut rest = source;
    while let Some(pos) = rest.find(reg) {
        let before = rest[..pos].chars().next_back();
        let after = rest[pos + reg.len()..].chars().next();
        let ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
        if !before.is_some_and(ident) && !after.is_some_and(ident) {
            count += 1;
        }
        rest = &rest[pos + reg.len()..];
    }
    count
}

/// The value the last assignment in the body stores. The fixtures put the read
/// under test last, so anything earlier is setup.
fn stored_value(source: &str) -> String {
    let line = source
        .lines()
        .rev()
        .find(|line| {
            line.contains(" = ") && line.trim_end().ends_with(';') && !line.contains("var ")
        })
        .unwrap_or_else(|| panic!("no assignment line in:\n{source}"));
    line.split_once(" = ")
        .map(|(_, rhs)| rhs.trim_end_matches(';').trim().to_string())
        .unwrap_or_default()
}

/// The statements, without the signature or the declarations. The parameters
/// are spelled `slot0`, which is also how a recovered stack value renders, so a
/// search for a value has to start below them.
fn body(source: &str) -> &str {
    source
        .split_once("\n\n")
        .map(|(_, body)| body)
        .unwrap_or(source)
}

/// Render one straight-line fixture, asserting the structured walk and the
/// fallback walk agree. Both merge through the same lifter, so a width rule
/// that lived in one of them would show up here.
fn render(name: &str, srcs: &[&str]) -> String {
    let ir = straight_line(name, srcs);
    let artifact = emit(&ir);
    let direct = emit_pseudocode_direct_dfs(&ir, &HashMap::new());
    assert_eq!(
        artifact.source, direct.source,
        "the fallback walk owes the same artifact for {srcs:?}"
    );
    artifact.source
}

/// A value whose producer was a 64-bit destination is not what a 32-bit read of
/// it yields, so the read is unresolved rather than resolved to the expression.
///
/// The first and last rows are provably incompatible rather than merely
/// unproven: the low 32 bits of a value shifted left by 32 are zero for every
/// input, and the artifact used to render the whole shift as the stored value.
#[test]
fn an_x_produced_non_literal_is_unresolved_through_a_w_read() {
    let cases: [(&[&str], &str, &str); 6] = [
        (
            &["ldur x2, [x1, #8]", "lsl x3, x2, #32", "mov w0, w3"],
            "reg3",
            "<<",
        ),
        (&["ldur x2, [x1, #8]", "mov w0, w2"], "reg2", "slot0.f8"),
        (
            &[
                "ldur x2, [x1, #8]",
                "ldur x3, [x1, #16]",
                "add x4, x2, x3",
                "mov w0, w4",
            ],
            "reg4",
            "+",
        ),
        (
            &["ldur x2, [x1, #8]", "lsl x0, x2, #32", "mov w0, w0"],
            "reg0",
            "<<",
        ),
        (&["mov x9, x1", "mov w9, w9"], "reg9", "slot0"),
        (
            &["mov x10, #0xffffffff", "add x9, x10, x1"],
            "reg9",
            "0xffffffff",
        ),
    ];

    for (prefix, expected, forbidden) in cases {
        // The last row reads the register through the store's own `w` spelling,
        // which is the same access stated at the use rather than at a move.
        let store = if prefix.last().is_some_and(|last| last.starts_with("add x9")) {
            "str w9, [x29, #16]"
        } else if expected == "reg9" {
            "stur x9, [x29, #16]"
        } else {
            "stur x0, [x29, #16]"
        };
        let mut srcs = prefix.to_vec();
        srcs.push(store);
        srcs.push("ret");
        let source = render("widthProvenance", &srcs);
        assert_eq!(
            stored_value(&source),
            expected,
            "`{prefix:?}` reads through a 32-bit token, so it is unresolved:\n{source}"
        );
        assert!(
            !body(&source).contains(forbidden),
            "the incompatible 64-bit value must not survive `{prefix:?}`:\n{source}"
        );
        assert!(
            unresolved_reads(&source, expected) >= 1,
            "the unresolved read has to be accounted for:\n{source}"
        );
    }
}

/// A 32-bit producer proves the register holds nothing above bit 31, so both a
/// 32-bit and a 64-bit read of it are exactly the bound value. Losing these
/// would be a blanket invalidation rather than tracked provenance.
#[test]
fn a_w_produced_value_stays_readable_at_both_widths() {
    let cases: [(&[&str], &str); 6] = [
        (&["ldur w2, [x1, #8]", "mov w0, w2"], "slot0.f8"),
        (&["ldur w2, [x1, #8]", "mov x0, x2"], "slot0.f8"),
        (&["ldur w2, [x1, #8]", "add w0, w2, #1"], "(slot0.f8 + 1)"),
        (
            &["ldur w2, [x1, #8]", "add w3, w2, #1", "mov w0, w3"],
            "(slot0.f8 + 1)",
        ),
        (
            &["ldur w2, [x1, #8]", "lsl w3, w2, #4", "mov w0, w3"],
            "(slot0.f8 << 4)",
        ),
        // A `w` binding taken through a 64-bit read and back through a 32-bit
        // one: the value never left the low half, so neither read loses it.
        (
            &["ldur w2, [x1, #8]", "mov x3, x2", "mov w0, w3"],
            "slot0.f8",
        ),
    ];

    for (prefix, expected) in cases {
        let mut srcs = prefix.to_vec();
        srcs.push("stur x0, [x29, #16]");
        srcs.push("ret");
        let source = render("compatibleWidth", &srcs);
        assert_eq!(
            stored_value(&source),
            expected,
            "`{prefix:?}` was produced at 32 bits, so it reads as itself:\n{source}"
        );
        assert_eq!(
            unresolved_reads(&source, "reg2"),
            0,
            "a compatible read must not be degraded:\n{source}"
        );
    }
}

/// The rule is producer width, not a mask on every 32-bit access. A blanket
/// mask would say the same thing about the incompatible cases and the wrong
/// thing about these, which is why the fix is provenance.
#[test]
fn a_compatible_read_carries_no_blanket_mask() {
    // Compressed pointers: a reference field is a 32-bit offset loaded with
    // `ldur w`, and `add rD, rS, x28, lsl #32` reconstructs the pointer, so the
    // Dart-level value is the field itself.
    let decompressed = render(
        "decompress",
        &[
            "ldur w9, [x1, #7]",
            "add x9, x9, x28, lsl #32",
            "stur x9, [x29, #16]",
            "ret",
        ],
    );
    assert_eq!(
        stored_value(&decompressed),
        "slot0.f8",
        "decompression is transparent:\n{decompressed}"
    );
    assert!(
        !decompressed.contains("0xffffffff"),
        "a compatible read gains no mask:\n{decompressed}"
    );

    // The forms that re-narrow what they read state the narrowing themselves,
    // so the `w` spelling of their operand costs no readability: `signExtend`
    // names the 32 bits it extends, and a complement is homomorphic mod 2^32.
    for (src, expected) in [
        ("sxtw x9, w1", "signExtend(slot0, 32)"),
        ("mvn w9, w1", "(((~slot0)) & 0xffffffff)"),
        ("neg w9, w1", "(((-slot0)) & 0xffffffff)"),
    ] {
        let source = render("selfNarrowing", &[src, "stur x9, [x29, #16]", "ret"]);
        assert_eq!(
            stored_value(&source),
            expected,
            "`{src}` re-narrows its operand, so the operand stays readable:\n{source}"
        );
    }
}

/// A literal is narrowed exactly at any producer width, because the truncation
/// costs nothing to render. That path is what keeps the rule from being a
/// blanket invalidation of everything a 32-bit token reads.
#[test]
fn a_literal_narrows_exactly_whatever_produced_it() {
    let cases = [
        (vec!["mov x1, #0x100000000", "mov w0, w1"], "0"),
        (vec!["mov x1, #0x1ffffffff", "mov w0, w1"], "0xffffffff"),
        (vec!["mov x1, #-1", "mov w0, w1"], "0xffffffff"),
        (vec!["mov x1, #0x2a", "mov w0, w1"], "0x2a"),
        (vec!["mov x1, #0x100000000", "mov x0, x1"], "0x100000000"),
    ];

    for (prefix, expected) in cases {
        let mut srcs = prefix.clone();
        srcs.push("stur x0, [x29, #16]");
        srcs.push("ret");
        let source = render("literalWidth", &srcs);
        assert_eq!(
            stored_value(&source),
            expected,
            "`{prefix:?}` must read `{expected}`:\n{source}"
        );
    }
}

/// The whole artifact for one incompatible case and its compatible twin, so the
/// unresolved read is pinned in place and counted rather than only asserted to
/// exist somewhere.
///
/// The two fixtures differ in exactly one character - the destination width of
/// the load - and that character is the whole provenance.
#[test]
fn the_artifact_states_the_width_gap_exactly_and_only_there() {
    let incompatible = render(
        "widthGap",
        &[
            "ldur x2, [x1, #8]",
            "lsl x3, x2, #32",
            "mov w0, w3",
            "stur x0, [x29, #16]",
            "stur x0, [x29, #24]",
            "ret",
        ],
    );
    assert_eq!(
        incompatible,
        "dynamic widthGap(dynamic slot0, dynamic slot1, dynamic slot2, dynamic slot3, dynamic slot4, dynamic slot5) {\n  \
           dynamic tmp1;\n  \
           dynamic tmp2;\n\n  \
           tmp1 = reg3;\n  \
           tmp2 = reg3;\n  \
           return reg3;\n\
         }",
        "unexpected artifact:\n{incompatible}"
    );
    assert_eq!(
        unresolved_reads(&incompatible, "reg3"),
        3,
        "one unresolved read per read of the incompatible binding, the two stores \
         and the implicit return of x0:\n{incompatible}"
    );

    let compatible = render(
        "widthGap",
        &[
            "ldur w2, [x1, #8]",
            "lsl w3, w2, #4",
            "mov w0, w3",
            "stur x0, [x29, #16]",
            "stur x0, [x29, #24]",
            "ret",
        ],
    );
    assert_eq!(
        compatible,
        "dynamic widthGap(dynamic slot0, dynamic slot1, dynamic slot2, dynamic slot3, dynamic slot4, dynamic slot5) {\n  \
           dynamic tmp1;\n  \
           dynamic tmp2;\n\n  \
           tmp1 = (slot0.f8 << 4);\n  \
           tmp2 = (slot0.f8 << 4);\n  \
           return (slot0.f8 << 4);\n\
         }",
        "unexpected artifact:\n{compatible}"
    );
    assert_eq!(
        unresolved_reads(&compatible, "reg3"),
        0,
        "nothing is unresolved when the producer proves the width:\n{compatible}"
    );
}

/// Provenance has to ride along the structured walk, not just a straight line.
///
/// The emitter clones the register state into a branch arm and restores it
/// before rendering the other one, which is where a width kept beside the
/// bindings would come back out of step with them: the first arm's read would
/// answer one way and the second arm's identical read another. Both arms read
/// the same register, produced before the branch, so both owe the same answer.
#[test]
fn width_provenance_rides_both_branch_arms() {
    let render_arms = |load: &str| {
        let mut ir = FunctionIr {
            function_id: 4010,
            name: "branchedWidth".to_string(),
            entry_va: 0x2000,
            blocks: vec![
                block(0, 0x2000, &[load, "cmp x0, #1", "b.eq #0x2030"], vec![2, 1]),
                block(1, 0x2010, &["mov w9, w2", "stur x9, [x29, #16]"], vec![3]),
                block(2, 0x2030, &["mov w9, w2", "stur x9, [x29, #24]"], vec![3]),
                block(3, 0x2040, &["ret"], Vec::new()),
            ],
        };
        rebuild_edges(&mut ir.blocks);
        // The two walks lay a diamond out differently - the fallback duplicates
        // the tail rather than merging it - so they are compared read by read
        // rather than byte for byte.
        let artifact = emit(&ir);
        let direct = emit_pseudocode_direct_dfs(&ir, &HashMap::new());
        (artifact.source, direct.source)
    };

    let (compatible, compatible_direct) = render_arms("ldur w2, [x1, #8]");
    for source in [&compatible, &compatible_direct] {
        assert_eq!(
            body(source).matches("= slot0.f8;").count(),
            2,
            "a 32-bit producer resolves in both arms, not just the first:\n{source}"
        );
        assert_eq!(
            unresolved_reads(source, "reg2"),
            0,
            "nothing is unresolved when the producer proves the width:\n{source}"
        );
    }

    let (incompatible, incompatible_direct) = render_arms("ldur x2, [x1, #8]");
    for source in [&incompatible, &incompatible_direct] {
        assert_eq!(
            body(source).matches("= reg2;").count(),
            2,
            "a 64-bit producer is unresolved in both arms:\n{source}"
        );
        assert!(
            !body(source).contains("slot0.f8"),
            "the incompatible value must not reach either arm:\n{source}"
        );
    }
}
