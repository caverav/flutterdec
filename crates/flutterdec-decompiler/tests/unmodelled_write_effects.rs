//! An unmodelled write has to drop what it invalidated.
//!
//! Every check here goes through the public emitter and reads the generated
//! artifact, because the artifact is what a reader trusts: a binding the lifter
//! kept across a write it does not model renders as a resolved value, and a
//! resolved value is indistinguishable from a recovered fact. The fixtures plant
//! a known binding, run one instruction whose effect the lifter cannot follow,
//! and read the register afterwards.
//!
//! `regN` is the emitter's spelling for a register with no value in hand, and it
//! is the whole-program unresolved counter as well: `quality.json`'s
//! `raw_register_name_refs` counts exactly these tokens (`quality.rs:17-25`), so
//! a case that renders one is a case that reports one.

use flutterdec_decompiler::{emit_pseudocode, emit_pseudocode_direct_dfs, PseudocodeArtifact};
use flutterdec_ir::{rebuild_edges, BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::HashMap;

fn op_of(src: &str) -> IROp {
    if src.starts_with("ret") {
        IROp::Return
    } else {
        IROp::Other
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
                target: String::new(),
            })
            .collect(),
        succs,
        preds: Vec::new(),
    }
}

/// One straight-line block, which is the smallest shape that shows a binding
/// surviving an instruction.
fn straight_line(name: &str, srcs: &[&str]) -> FunctionIr {
    FunctionIr {
        function_id: 4004,
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

/// The read that follows the unmodelled write, as the artifact renders it.
///
/// The last assignment in the body is the observation point: the fixtures put the
/// read of the register under test after the write, so anything earlier belongs to
/// the setup.
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

/// A write the lifter does not model, at every width and shape it appears in.
///
/// `x` and `w` name one machine register and a 32-bit write clears the high half
/// rather than preserving it, so both spellings invalidate the whole binding.
/// The vector spellings write no general register at all, and the fixture proves
/// the summary does not confuse the two: `frobnicate v1.4s, v2.4s` must leave x1
/// alone.
#[test]
fn an_unmodelled_write_drops_the_binding_at_every_destination_width() {
    let cases = [
        ("frobnicate x1, x2", "reg1"),
        ("frobnicate w1, w2", "reg1"),
        ("frobnicate x1, x2, x3, #4", "reg1"),
        ("mrs x1, tpidr_el0", "reg1"),
        ("fmov x1, d0", "reg1"),
        // Writes a vector register, so the general register keeps its value.
        ("frobnicate v1.4s, v2.4s", "0x2a"),
    ];

    for (write, expected) in cases {
        let ir = straight_line(
            "unmodelledWidth",
            &["mov x1, #0x2a", write, "str x1, [x29, #16]", "ret"],
        );
        let artifact = emit(&ir);
        assert_eq!(
            stored_value(&artifact.source),
            expected,
            "`{write}` must leave x1 reading `{expected}`:\n{}",
            artifact.source
        );
    }
}

/// A pair form writes two registers, so naming one of them leaves the other
/// holding a value from before the instruction ran.
///
/// `ldp` and `ldnp` are lifted and bind both halves; the rest are not modelled at
/// all, and their second destination used to render the planted literal as a
/// resolved fact.
#[test]
fn a_pair_destination_write_invalidates_both_of_its_registers() {
    for write in [
        "ldpsw x0, x1, [x2]",
        "ldxp x0, x1, [x2]",
        "ldaxp x0, x1, [x2]",
        "casp x0, x1, x2, x3, [x4]",
    ] {
        let ir = straight_line(
            "pairDestination",
            &[
                "mov x0, #7",
                "mov x1, #0x2a",
                write,
                "str x1, [x29, #16]",
                "ret",
            ],
        );
        let artifact = emit(&ir);
        let source = &artifact.source;
        assert_eq!(
            stored_value(source),
            "reg1",
            "`{write}` writes its second destination, so x1 is unresolved:\n{source}"
        );
        assert!(
            !source.contains("0x2a"),
            "`{write}` must not leave the planted value readable:\n{source}"
        );
        assert!(
            unresolved_reads(source, "reg1") >= 1,
            "the unresolved read has to be accounted for:\n{source}"
        );
    }
}

/// Pre- and post-index addressing writes the base register back, and the new
/// address is not tracked, so the base cannot keep describing the old one.
#[test]
fn an_index_writeback_invalidates_the_base_register() {
    for (write, still_bound) in [
        ("ldr x2, [x1], #8", false),
        ("str x2, [x1, #8]!", false),
        ("ldr x2, [x1, #8]", true),
    ] {
        let ir = straight_line(
            "writebackBase",
            &["mov x1, #0x2a", write, "str x1, [x29, #16]", "ret"],
        );
        let artifact = emit(&ir);
        let value = stored_value(&artifact.source);
        if still_bound {
            assert_eq!(
                value, "0x2a",
                "`{write}` does not write the base:\n{}",
                artifact.source
            );
        } else {
            assert_eq!(
                value, "reg1",
                "`{write}` writes the base back, so its value is unknown:\n{}",
                artifact.source
            );
        }
    }
}

/// An unmodelled instruction that sets the flags leaves no comparison behind,
/// so a conditional value after it has no condition to name.
///
/// `msr nzcv, x3` is the case the mnemonic alone cannot classify: it writes the
/// flag register and no general register, so it reads as effect-free unless the
/// operand is part of the summary.
#[test]
fn an_unmodelled_flag_write_drops_the_comparison_it_invalidated() {
    for write in ["msr nzcv, x3", "fccmp d0, d1, #0, eq", "rmif x3, #0, #1"] {
        let ir = straight_line(
            "unmodelledFlags",
            &[
                "cmp x1, #1",
                write,
                "cset x0, eq",
                "str x0, [x29, #16]",
                "ret",
            ],
        );
        let artifact = emit(&ir);
        let source = &artifact.source;
        assert_eq!(
            stored_value(source),
            "reg0",
            "`{write}` invalidates the flags, so the conditional value is unknown:\n{source}"
        );
        assert!(
            !source.contains("slot0 == 1"),
            "the comparison before `{write}` must not be claimed after it:\n{source}"
        );
    }
}

/// The same comparison is still readable when nothing invalidates it, so the
/// case above is about the write and not about `cset` being unlifted.
#[test]
fn a_modelled_run_still_names_the_comparison_it_kept() {
    let ir = straight_line(
        "keptFlags",
        &["cmp x1, #1", "cset x0, eq", "str x0, [x29, #16]", "ret"],
    );
    let artifact = emit(&ir);
    assert_eq!(
        stored_value(&artifact.source),
        "((slot0 == 1) ? 1 : 0)",
        "an intact comparison still reads as itself:\n{}",
        artifact.source
    );
}

/// A 32-bit write leaves the low half zero-extended, so a bound value outside
/// the unsigned 32-bit range is not what the register ends up holding.
///
/// Each expected value here is the exact 32-bit truth: `mov w0, w1` after
/// `mov x1, #0x100000000` leaves zero, `add w0, w1, #1` leaves one, and
/// `mov w0, #-1` leaves `0xffffffff` rather than a negative number.
#[test]
fn a_width_specific_write_cannot_keep_an_out_of_range_binding() {
    let cases = [
        (vec!["mov x1, #0x100000000", "mov w0, w1"], "0"),
        (vec!["mov x1, #0x100000000", "add w0, w1, #1"], "1"),
        (vec!["mov x1, #0x100000000", "sub w0, w1, #1"], "0xffffffff"),
        (vec!["mov x1, #0x100000000", "orr w0, w1, #1"], "(0 | 1)"),
        (vec!["mov x1, #0x1ffffffff", "mov w0, w1"], "0xffffffff"),
        (vec!["mov x1, #-1", "mov w0, w1"], "0xffffffff"),
        (vec!["mov w0, #-1"], "0xffffffff"),
        // A 64-bit destination keeps the whole value: the rule is the width of
        // the access, not a blanket truncation.
        (vec!["mov x1, #0x100000000", "mov x0, x1"], "0x100000000"),
        // A value already inside the range is untouched, so nothing is masked
        // for its own sake.
        (vec!["mov x1, #0x2a", "mov w0, w1"], "0x2a"),
    ];

    for (prefix, expected) in cases {
        let mut srcs = prefix.clone();
        srcs.push("str x0, [x29, #16]");
        srcs.push("ret");
        let ir = straight_line("writeWidth", &srcs);
        let artifact = emit(&ir);
        assert_eq!(
            stored_value(&artifact.source),
            expected,
            "`{prefix:?}` must bind `{expected}`:\n{}",
            artifact.source
        );
    }
}

/// The whole artifact for one unknown-effect case, so the unresolved read is
/// pinned in place rather than only asserted to exist somewhere.
#[test]
fn the_artifact_states_the_unknown_exactly_once_and_names_nothing_else() {
    let ir = straight_line(
        "explicitUnknown",
        &[
            "mov x1, #0x2a",
            "frobnicate x1, x2",
            "str x1, [x29, #16]",
            "str x1, [x29, #24]",
            "ret",
        ],
    );
    let artifact = emit(&ir);
    let source = &artifact.source;

    assert_eq!(
        source,
        "dynamic explicitUnknown(dynamic slot0, dynamic slot1, dynamic slot2, dynamic slot3, dynamic slot4, dynamic slot5) {\n  \
           dynamic tmp1;\n  \
           dynamic tmp2;\n\n  \
           tmp1 = reg1;\n  \
           tmp2 = reg1;\n  \
           return null;\n\
         }",
        "unexpected artifact:\n{source}"
    );
    assert_eq!(
        unresolved_reads(source, "reg1"),
        2,
        "one unresolved read per read of the invalidated register:\n{source}"
    );
    assert!(
        !source.contains("0x2a"),
        "the planted value is not readable anywhere:\n{source}"
    );
}

/// The invalidation has to survive a merge as well as a straight line.
///
/// `written_registers` is the summary the join merge reads, so a destination it
/// fails to name keeps a binding alive on a path that redefined it. Here the
/// planted value comes from one predecessor and the unmodelled pair write from
/// the other, and the join must not read either as resolved.
#[test]
fn a_merge_after_an_unmodelled_write_reads_as_unresolved() {
    let mut ir = FunctionIr {
        function_id: 4005,
        name: "mergedUnknown".to_string(),
        entry_va: 0x2000,
        blocks: vec![
            block(0, 0x2000, &["cmp x0, #1", "b.eq #0x2020"], vec![2, 1]),
            block(1, 0x2010, &["mov x1, #0x2a"], vec![3]),
            block(2, 0x2020, &["ldpsw x0, x1, [x2]"], vec![3]),
            block(3, 0x2030, &["str x1, [x29, #16]", "ret"], Vec::new()),
        ],
    };
    rebuild_edges(&mut ir.blocks);

    let artifact = emit(&ir);
    let source = &artifact.source;
    assert!(
        !source.contains("0x2a"),
        "a value one predecessor wrote is not the join's value:\n{source}"
    );
    assert!(
        unresolved_reads(source, "reg1") >= 1,
        "the join reads the register as unresolved:\n{source}"
    );

    // The fallback walk merges through the same summary, so it owes the same
    // answer.
    let direct = emit_pseudocode_direct_dfs(&ir, &HashMap::new());
    assert!(
        !direct.source.contains("0x2a"),
        "the fallback walk must not resolve it either:\n{}",
        direct.source
    );
}
