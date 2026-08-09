use flutterdec_disasm_arm64::FunctionDisassembly;
use std::collections::HashMap;

/// Dart 3.5, product ARM64, compressed pointers.
const STUB_SLOTS_3_5: &[(u64, &str)] = &[
    (0xc8, "lateInitializationErrorSharedWithoutFpuRegs"),
    (0xd0, "lateInitializationErrorSharedWithFpuRegs"),
    (0xd8, "nullErrorSharedWithoutFpuRegs"),
    (0xe0, "nullErrorSharedWithFpuRegs"),
    (0xe8, "nullArgErrorSharedWithoutFpuRegs"),
    (0xf0, "nullArgErrorSharedWithFpuRegs"),
    (0xf8, "nullCastErrorSharedWithoutFpuRegs"),
    (0x100, "nullCastErrorSharedWithFpuRegs"),
    (0x108, "rangeErrorSharedWithoutFpuRegs"),
    (0x110, "rangeErrorSharedWithFpuRegs"),
    (0x118, "writeErrorSharedWithoutFpuRegs"),
    (0x120, "writeErrorSharedWithFpuRegs"),
    (0x128, "allocateMintWithFpuRegs"),
    (0x130, "allocateMintWithoutFpuRegs"),
    (0x178, "stackOverflowSharedWithoutFpuRegs"),
    (0x180, "stackOverflowSharedWithFpuRegs"),
    (0x1c8, "slowTypeTest"),
];

/// Dart 3.12, product ARM64, compressed pointers.
const STUB_SLOTS_3_12: &[(u64, &str)] = &[
    (0xd0, "invokeDartCode"),
    (0xe8, "lateInitializationErrorSharedWithoutFpuRegs"),
    (0xf0, "lateInitializationErrorSharedWithFpuRegs"),
    (0xf8, "nullErrorSharedWithoutFpuRegs"),
    (0x100, "nullErrorSharedWithFpuRegs"),
    (0x108, "nullArgErrorSharedWithoutFpuRegs"),
    (0x110, "nullArgErrorSharedWithFpuRegs"),
    (0x118, "nullCastErrorSharedWithoutFpuRegs"),
    (0x120, "nullCastErrorSharedWithFpuRegs"),
    (0x128, "rangeErrorSharedWithoutFpuRegs"),
    (0x130, "rangeErrorSharedWithFpuRegs"),
    (0x138, "writeErrorSharedWithoutFpuRegs"),
    (0x140, "writeErrorSharedWithFpuRegs"),
    (0x148, "fieldAccessErrorSharedWithoutFpuRegs"),
    (0x150, "fieldAccessErrorSharedWithFpuRegs"),
    (0x158, "allocateMintWithFpuRegs"),
    (0x160, "allocateMintWithoutFpuRegs"),
    (0x190, "stackOverflowSharedWithoutFpuRegs"),
    (0x198, "stackOverflowSharedWithFpuRegs"),
    (0x1d8, "slowTypeTest"),
];

/// The Dart thread register on ARM64 AOT: `const Register THR = R26`
/// (`runtime/vm/constants_arm64.h`). Confirmed in the instruction stream, where
/// `x26` is the base of 2,971 of the sampled `ldr rD, [rN, #imm]` loads and the
/// next candidate is the frame pointer.
const THREAD_REGISTER: &str = "x26";

/// How far into a function the self-load may appear. `GenerateSharedStubGeneric`
/// emits it directly after the canonical register pushes
/// (`stub_code_compiler_arm64.cc:287-337`), measured at instruction 11 for the
/// without-FPU variants and 27 for the with-FPU ones. The window keeps an
/// ordinary function that happens to load a stub slot deep in its body from
/// being named after it.
const PROLOGUE_WINDOW: usize = 32;

/// Names the shared stubs a binary calls, from each stub's own prologue.
///
/// A generic shared stub loads its own `Code` object out of a fixed `Thread`
/// slot before making its runtime call, so the callee identifies itself and no
/// inference from call frequency or address is involved.
///
/// The slot number means different things in different SDKs -- `0x100` is
/// `nullCastErrorSharedWithFpuRegs` in 3.5 and `nullErrorSharedWithFpuRegs` in
/// 3.12 -- so a wrong table yields a confidently wrong name rather than no
/// name. Both the version and the pointer mode must therefore be known
/// positively, and there is no fallback to another version's table and no
/// nearest-offset match. An unknown version names nothing.
///
/// Ordinary functions are not at risk of a false match: their prologue loads
/// the stack limit (`0x48` on 3.12, 1,537 of the sampled loads), which is a
/// thread field and not a member of the stub slot set.
pub(super) fn shared_stub_names(
    disasm: &[FunctionDisassembly],
    dart_version: Option<&str>,
    compressed_pointers: Option<bool>,
) -> SharedStubNaming {
    let Some(slots) = stub_slots(dart_version, compressed_pointers) else {
        return SharedStubNaming::refused("unknown_key");
    };
    let mut names = HashMap::new();
    for f in disasm {
        if let Some(name) = prologue_stub_name(f, slots) {
            names.insert(f.entry_va, name.to_string());
        }
    }
    if names.is_empty() {
        return SharedStubNaming::refused("no_stub_prologues");
    }
    // The header is not the only evidence of which table applies. Every
    // vendored table matches a different number of prologues in the same
    // binary, because the offsets moved, so the observed offset set fingerprints
    // the SDK independently: on the two samples the correct table matches 14
    // prologues and the other one 7 and 8. If the header's version is not the
    // best-scoring table, the two disagree and naming tens of thousands of call
    // sites off the losing table would be a silent mislabel. Refuse instead.
    if !is_best_scoring(disasm, slots) {
        return SharedStubNaming::refused("table_disagreement");
    }
    SharedStubNaming {
        names,
        status: "named",
    }
}

/// Names plus why there are that many, so a zero is self-explaining in the
/// report rather than indistinguishable from a feature that never ran. The
/// count is sensitive to how much of the binary is in scope: too few stub
/// prologues and the tables cannot be separated, which reports as
/// `table_disagreement` rather than as silence.
pub(super) struct SharedStubNaming {
    pub(super) names: HashMap<u64, String>,
    pub(super) status: &'static str,
}

impl SharedStubNaming {
    fn refused(status: &'static str) -> Self {
        Self {
            names: HashMap::new(),
            status,
        }
    }
}

/// Whether the header's table is the one the binary's own prologues support.
///
/// Each vendored table matches a different number of prologues, because the
/// offsets moved: on the two samples the correct table matches 14 and the other
/// 7 and 8. A table that matches strictly fewer is not this binary's.
///
/// A tie in *count* is not agreement. `0x100` is a member of both tables and
/// names `nullCastErrorSharedWithFpuRegs` on 3.5 but
/// `nullErrorSharedWithFpuRegs` on 3.12, so two tables can match the same
/// prologues and disagree on every name. On a tie the names themselves are
/// compared, and only an identical mapping is accepted.
fn is_best_scoring(disasm: &[FunctionDisassembly], slots: &'static [(u64, &'static str)]) -> bool {
    let names = |table: &'static [(u64, &'static str)]| {
        disasm
            .iter()
            .filter_map(|f| prologue_stub_name(f, table).map(|n| (f.entry_va, n)))
            .collect::<Vec<_>>()
    };
    let mine = names(slots);
    ALL_STUB_TABLES.iter().all(|table| {
        let other = names(table);
        other.len() < mine.len() || other == mine
    })
}

/// Every vendored table, for the cross-check above.
const ALL_STUB_TABLES: &[&[(u64, &str)]] = &[STUB_SLOTS_3_5, STUB_SLOTS_3_12];

/// The table for a binary, or `None` when either key is unknown. The vendored
/// rows are from the product ARM64 `DART_COMPRESSED_POINTERS` block, so an
/// uncompressed binary has no table even on a known version.
fn stub_slots(
    dart_version: Option<&str>,
    compressed_pointers: Option<bool>,
) -> Option<&'static [(u64, &'static str)]> {
    if compressed_pointers != Some(true) {
        return None;
    }
    let version = dart_version?;
    let mut parts = version.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    match (major, minor) {
        (3, 5) => Some(STUB_SLOTS_3_5),
        (3, 12) => Some(STUB_SLOTS_3_12),
        _ => None,
    }
}

/// The stub slot this function loads from `THR` in its prologue, if any.
fn prologue_stub_name(
    f: &FunctionDisassembly,
    slots: &'static [(u64, &'static str)],
) -> Option<&'static str> {
    for ins in f.instructions.iter().take(PROLOGUE_WINDOW) {
        if !ins.mnemonic.eq_ignore_ascii_case("ldr") {
            continue;
        }
        let Some(offset) = thread_slot_offset(&ins.op_str) else {
            continue;
        };
        if let Some((_, name)) = slots.iter().find(|(slot, _)| *slot == offset) {
            return Some(name);
        }
    }
    None
}

/// The displacement of a `ldr rD, [THR, #imm]`, or `None` for any other shape.
/// Pre- and post-indexed forms are excluded: they write the base back, so they
/// are not the stub's self-load.
fn thread_slot_offset(op_str: &str) -> Option<u64> {
    let (_, mem) = op_str.split_once('[')?;
    let (mem, rest) = mem.split_once(']')?;
    if rest.contains('!') {
        return None;
    }
    let (base, disp) = mem.split_once(',')?;
    if base.trim() != THREAD_REGISTER {
        return None;
    }
    let disp = disp.trim().trim_start_matches('#').trim();
    let hex = disp.strip_prefix("0x")?;
    u64::from_str_radix(hex, 16).ok()
}
#[cfg(test)]
mod tests {
    use super::*;
    use flutterdec_disasm_arm64::AsmInstruction;

    fn ins(va: u64, mnemonic: &str, op_str: &str) -> AsmInstruction {
        AsmInstruction {
            va,
            word: 0,
            mnemonic: mnemonic.to_string(),
            op_str: op_str.to_string(),
            annotation: String::new(),
        }
    }

    /// A stub whose prologue pushes, then loads its own `Code` from `slot`.
    fn stub(entry_va: u64, slot: u64) -> FunctionDisassembly {
        let mut instructions = vec![ins(entry_va, "str", "x30, [x15, #-8]!")];
        for i in 1..11 {
            instructions.push(ins(entry_va + i * 4, "stp", "x2, x3, [x15, #-0x10]!"));
        }
        instructions.push(ins(entry_va + 44, "ldr", &format!("x16, [x26, #{slot:#x}]")));
        FunctionDisassembly {
            function_id: entry_va,
            function_name: format!("sub_{entry_va:x}"),
            owner_class: String::new(),
            entry_va,
            size: 48,
            instructions,
        }
    }

    /// An ordinary function: its prologue loads the stack limit, a thread field.
    fn ordinary(entry_va: u64) -> FunctionDisassembly {
        FunctionDisassembly {
            function_id: entry_va,
            function_name: format!("sub_{entry_va:x}"),
            owner_class: String::new(),
            entry_va,
            size: 8,
            instructions: vec![
                ins(entry_va, "ldr", "x16, [x26, #0x48]"),
                ins(entry_va + 4, "cmp", "sp, x16"),
            ],
        }
    }

    /// All eleven slots present in both vendored tables disagree on the name --
    /// `0x118` is `nullCastErrorSharedWithoutFpuRegs` on 3.12 and
    /// `writeErrorSharedWithoutFpuRegs` on 3.5 -- so a version confusion
    /// mislabels every call site rather than some. Matching the wrong table must
    /// produce no name at all.
    ///
    /// The fixture mirrors a real binary: several slots that exist only in 3.12
    /// (`0x148`, `0x190`, `0x1d8`) so that table strictly out-scores the other,
    /// plus one shared slot to show which name wins. A binary with too few stubs
    /// to separate the tables names nothing, which is the conservative direction.
    #[test]
    fn a_version_mismatch_names_nothing_rather_than_naming_it_wrong() {
        let disasm = vec![
            stub(0x2000, 0x118),
            stub(0x2100, 0x148),
            stub(0x2200, 0x190),
            stub(0x2300, 0x1d8),
            ordinary(0x3000),
        ];

        // Control: the scan must actually see the stubs, so that the refusal
        // below is a refusal and not a selector that matches nothing.
        let right = shared_stub_names(&disasm, Some("3.12.1"), Some(true));
        assert_eq!(right.status, "named");
        assert_eq!(
            right.names.get(&0x2000).map(String::as_str),
            Some("nullCastErrorSharedWithoutFpuRegs"),
            "control failed: the prologue scan found no stub at all"
        );
        assert_eq!(
            right.names.get(&0x2200).map(String::as_str),
            Some("stackOverflowSharedWithoutFpuRegs"),
            "a 3.12-only slot must take its 3.12 name: {:?}",
            right.names
        );
        assert!(
            !right.names.contains_key(&0x3000),
            "an ordinary function's stack-limit load is not a stub slot: {:?}",
            right.names
        );

        // The 3.12 table matches four prologues here and the 3.5 table one, so
        // asking for 3.5 is contradicted by the binary's own offset set.
        let wrong = shared_stub_names(&disasm, Some("3.5.0"), Some(true));
        assert!(
            wrong.names.is_empty(),
            "a table the offsets contradict must not be used: {:?}",
            wrong.names
        );
        assert_eq!(wrong.status, "table_disagreement");
    }

    /// The tie branch. One shared slot separates nothing: both tables match the
    /// single prologue, so the count is equal and only the names decide. They
    /// disagree at every shared slot, so neither version is trusted -- including
    /// the one the header actually reports.
    #[test]
    fn a_binary_too_small_to_separate_the_tables_names_nothing() {
        let disasm = vec![stub(0x2000, 0x118)];
        for version in ["3.12.1", "3.5.0"] {
            let named = shared_stub_names(&disasm, Some(version), Some(true));
            assert!(
                named.names.is_empty(),
                "{version} cannot be confirmed by one shared slot: {:?}",
                named.names
            );
            assert_eq!(named.status, "table_disagreement");
        }
    }

    /// The vendored rows are from the compressed-pointer block, and an unknown
    /// SDK has no rows at all. Neither may fall back to a table.
    #[test]
    fn an_unknown_key_names_nothing() {
        let disasm = vec![
            stub(0x2000, 0x118),
            stub(0x2100, 0x148),
            stub(0x2200, 0x190),
            stub(0x2300, 0x1d8),
        ];
        assert_eq!(
            shared_stub_names(&disasm, Some("3.12.1"), Some(true)).status,
            "named",
            "control failed: the known key names nothing"
        );
        for (version, compressed) in [
            (Some("3.12.1"), Some(false)),
            (Some("3.12.1"), None),
            (Some("3.9.0"), Some(true)),
            (Some("unknown"), Some(true)),
            (None, Some(true)),
        ] {
            let named = shared_stub_names(&disasm, version, compressed);
            assert!(
                named.names.is_empty(),
                "({version:?}, {compressed:?}) must name nothing, got {:?}",
                named.names
            );
            assert_eq!(named.status, "unknown_key");
        }
    }
}
