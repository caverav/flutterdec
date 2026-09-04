//! An atomic read-modify-write has to invalidate the register it loads into.
//!
//! `LDADD <Xs>, <Xt>, [<Xn>]` adds Xs to the value in memory and writes the old
//! memory value to Xt, so the destination is the *second* operand and the first
//! is a pure source. Summarised as a first-operand write, the destination kept
//! whatever the last modelled instruction left in it: after `mov x9, #0x2a`,
//! `ldaddal x2, x9, [x3]` rendered `0x2a` at every later read of x9, which is a
//! literal presented as a recovered fact. `swp` has the same shape. `cas`
//! returns the old value in its first operand and keeps the generic rule.
//!
//! The whole spelling space is exercised rather than a sample: nine families
//! times the memory-ordering suffixes (`a`, `l`, `al`) times the sizes (`b`,
//! `h`). `crates/flutterdec-disasm-arm64/src/lib.rs` takes Capstone's mnemonic
//! verbatim with no allowlist, so every one of these reaches the emitter from a
//! real binary.
//!
//! Everything goes through the public emitter and reads the generated artifact.
//! `regN` is the emitter's spelling for a register with no value in hand and is
//! the whole-program unresolved counter as well: `quality.json`'s
//! `raw_register_name_refs` counts exactly these tokens (`quality.rs:17-25`).
//! The artifacts are the same in debug and in `--release`; the lifter has no
//! debug-only assertion, so a release build owes these answers too.

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
        function_id: 4011,
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

/// Every spelling of one atomic family: the base mnemonic, the three
/// memory-ordering suffixes, and the byte and halfword sizes of each.
fn spellings(family: &str) -> Vec<String> {
    let mut out = Vec::new();
    for order in ["", "a", "l", "al"] {
        for size in ["", "b", "h"] {
            out.push(format!("{family}{order}{size}"));
        }
    }
    out
}

const LOAD_DESTINATION_FAMILIES: [&str; 9] = [
    "ldadd", "ldclr", "ldeor", "ldset", "ldsmax", "ldsmin", "ldumax", "ldumin", "swp",
];

/// Render one straight-line fixture, asserting the structured walk and the
/// fallback walk agree. `written_registers` is the summary both walks read, so
/// a gap in one of them would show up here.
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

/// The loaded value overwrites the second operand, so a value bound to it
/// before the instruction is gone afterwards.
#[test]
fn every_atomic_load_form_invalidates_its_second_operand() {
    let mut checked = 0usize;
    for family in LOAD_DESTINATION_FAMILIES {
        for mnemonic in spellings(family) {
            let write = format!("{mnemonic} x2, x9, [x3]");
            let source = render(
                "atomicDestination",
                &["mov x9, #0x2a", &write, "str x9, [x29, #16]", "ret"],
            );
            assert_eq!(
                stored_value(&source),
                "reg9",
                "`{write}` loads into x9, so the planted value is gone:\n{source}"
            );
            assert!(
                !source.contains("0x2a"),
                "`{write}` must not leave the planted value readable:\n{source}"
            );
            assert_eq!(
                unresolved_reads(&source, "reg9"),
                1,
                "the unresolved read has to be accounted for:\n{source}"
            );
            checked += 1;
        }
    }
    assert_eq!(
        checked, 108,
        "nine families at four orderings and three sizes"
    );
}

/// The first operand is the value the instruction applies to memory, not a
/// destination, so a binding for it survives. Naming it would cost a resolved
/// value for nothing, which is the opposite error to the one above.
#[test]
fn an_atomic_load_form_leaves_its_source_operand_alone() {
    for family in LOAD_DESTINATION_FAMILIES {
        for mnemonic in spellings(family) {
            let write = format!("{mnemonic} x2, x9, [x3]");
            let source = render(
                "atomicSource",
                &["mov x2, #0x2a", &write, "str x2, [x29, #16]", "ret"],
            );
            assert_eq!(
                stored_value(&source),
                "0x2a",
                "`{write}` reads x2 and does not write it:\n{source}"
            );
        }
    }
}

/// `CAS <Xs>, <Xt>, [<Xn>]` returns the old memory value in its *first*
/// operand, so the generic rule is already right for it and must stay that way:
/// invalidating the second operand instead would drop a live binding and keep
/// the stale one.
#[test]
fn a_compare_and_swap_keeps_its_first_operand_rule() {
    for mnemonic in spellings("cas") {
        let write = format!("{mnemonic} x9, x2, [x3]");

        let destination = render(
            "casDestination",
            &["mov x9, #0x2a", &write, "str x9, [x29, #16]", "ret"],
        );
        assert_eq!(
            stored_value(&destination),
            "reg9",
            "`{write}` returns the old value in x9:\n{destination}"
        );
        assert!(
            !destination.contains("0x2a"),
            "`{write}` must not leave the planted value readable:\n{destination}"
        );

        let source = render(
            "casSource",
            &["mov x2, #0x2a", &write, "str x2, [x29, #16]", "ret"],
        );
        assert_eq!(
            stored_value(&source),
            "0x2a",
            "`{write}` stores x2 and does not write it:\n{source}"
        );
    }
}

/// The pair form writes both halves of its compare pair, which is a separate
/// rule again and is not disturbed by the second-operand families.
#[test]
fn a_compare_and_swap_pair_still_writes_both_halves() {
    for mnemonic in ["casp", "caspa", "caspl", "caspal"] {
        let write = format!("{mnemonic} x8, x9, x2, x3, [x4]");
        let source = render(
            "caspDestination",
            &[
                "mov x8, #7",
                "mov x9, #0x2a",
                &write,
                "str x9, [x29, #16]",
                "ret",
            ],
        );
        assert_eq!(
            stored_value(&source),
            "reg9",
            "`{write}` writes both halves of the pair:\n{source}"
        );
        assert!(
            !source.contains("0x2a"),
            "`{write}` must not leave the planted value readable:\n{source}"
        );
    }
}

/// A mnemonic that only shares a prefix with these families is not one of them.
///
/// The 128-bit `ldclrp` form writes a register pair, and a store-only form
/// writes no general register at all, so both keep the rules they already had
/// rather than being read as second-operand destinations.
#[test]
fn a_prefix_alone_does_not_make_a_second_operand_destination() {
    // `stadd Xs, [Xn]` has no destination operand, so the register holding the
    // planted value is untouched.
    let store_only = render(
        "atomicStore",
        &[
            "mov x9, #0x2a",
            "staddl x2, [x3]",
            "str x9, [x29, #16]",
            "ret",
        ],
    );
    assert_eq!(
        stored_value(&store_only),
        "0x2a",
        "a store-only atomic writes no general register:\n{store_only}"
    );

    // An unrecognised suffix keeps the generic first-operand rule, which is the
    // conservative direction: the binding under test is dropped rather than
    // left resolved by a claim this does not model.
    let unrecognised = render(
        "atomicPair",
        &[
            "mov x9, #0x2a",
            "ldclrp x9, x8, [x3]",
            "str x9, [x29, #16]",
            "ret",
        ],
    );
    assert_eq!(
        stored_value(&unrecognised),
        "reg9",
        "an unmodelled atomic still drops its first operand:\n{unrecognised}"
    );
}

/// The whole artifact for one atomic case, so the unresolved read is pinned in
/// place and counted rather than only asserted to exist somewhere.
#[test]
fn the_artifact_states_the_atomic_unknown_exactly() {
    let source = render(
        "atomicUnknown",
        &[
            "mov x9, #0x2a",
            "ldaddal x2, x9, [x3]",
            "str x9, [x29, #16]",
            "str x9, [x29, #24]",
            "ret",
        ],
    );
    assert_eq!(
        source,
        "dynamic atomicUnknown(dynamic slot0, dynamic slot1, dynamic slot2, dynamic slot3, dynamic slot4, dynamic slot5) {\n  \
           dynamic tmp1;\n  \
           dynamic tmp2;\n\n  \
           tmp1 = reg9;\n  \
           tmp2 = reg9;\n  \
           return null;\n\
         }",
        "unexpected artifact:\n{source}"
    );
    assert_eq!(
        unresolved_reads(&source, "reg9"),
        2,
        "one unresolved read per read of the invalidated register:\n{source}"
    );
}

/// The invalidation has to survive a merge as well as a straight line.
///
/// `written_registers` is also the summary the join merge reads, so a
/// destination it fails to name keeps a binding alive past a path that
/// redefined it. The value is planted before the branch and only one arm runs
/// the atomic, so the join keeps the planted literal unless the summary names
/// the register that arm loaded into.
#[test]
fn a_merge_after_an_atomic_load_reads_as_unresolved() {
    for family in LOAD_DESTINATION_FAMILIES {
        let write = format!("{family}al x2, x9, [x3]");
        let mut ir = FunctionIr {
            function_id: 4012,
            name: "mergedAtomic".to_string(),
            entry_va: 0x2000,
            blocks: vec![
                block(
                    0,
                    0x2000,
                    &["mov x9, #0x2a", "cmp x0, #1", "b.eq #0x2020"],
                    vec![2, 1],
                ),
                block(1, 0x2010, &["mov x8, #1"], vec![3]),
                block(2, 0x2020, &[&write], vec![3]),
                block(3, 0x2030, &["str x9, [x29, #16]", "ret"], Vec::new()),
            ],
        };
        rebuild_edges(&mut ir.blocks);

        let artifact = emit(&ir);
        let source = &artifact.source;
        assert!(
            !source.contains("0x2a"),
            "a value one predecessor wrote is not the join's value after `{write}`:\n{source}"
        );
        assert!(
            unresolved_reads(source, "reg9") >= 1,
            "the join reads the register as unresolved:\n{source}"
        );

        let direct = emit_pseudocode_direct_dfs(&ir, &HashMap::new());
        assert!(
            !direct.source.contains("0x2a"),
            "the fallback walk must not resolve it either:\n{}",
            direct.source
        );
    }
}
