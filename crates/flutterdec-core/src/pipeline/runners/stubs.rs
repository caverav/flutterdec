use flutterdec_disasm_arm64::FunctionDisassembly;
use flutterdec_decompiler::RuntimeStubEffect;
use flutterdec_ir::{FunctionIr, IROp};
use std::collections::{HashMap, HashSet};

/// One vendored `Thread` slot: its displacement, the SDK stub it holds, and
/// whether control can come back from that stub.
struct StubSlot {
    offset: u64,
    name: &'static str,
    returns: bool,
    writes_result: bool,
    preserves_registers: bool,
}

/// Dart 3.5, product ARM64, compressed pointers.
///
/// `returns` is `allow_return` from `GenerateSharedStub`: `false` means the stub
/// raises and control never comes back. Corroborated in both sample binaries,
/// where every `false` row reaches `brk` before any `ret` and every `true` row
/// reaches `ret` first.
///
/// `writes_result` is `store_runtime_result_in_result_register`, defaulted false
/// by the SDK and set only for the mint allocator
/// (`stub_code_compiler_arm64.cc:1481,1501`). Separate from `returns`:
/// `stackOverflow` comes back but defines no value, so binding its result would
/// be as false as binding a throw. The type test does not define one either:
/// `TypeTestABI::kInstanceReg` is `R0` and sits in `kPreservedAbiRegisters`, so
/// x0 still holds the instance afterwards, and the answer goes to
/// `kSubtypeTestCacheResultReg` = `R7`, which is not preserved
/// (`constants_arm64.h:237-258`). Only `invokeDartCode` returns in `R0`.
///
/// `preserves_registers` marks the `GenerateSharedStub` family, which pushes and
/// pops `AddAllNonReservedRegisters` around the runtime call
/// (`stub_code_compiler_arm64.cc:300,309`; `locations.h:692-703`). Those calls
/// clobber nothing, and the mint allocator writes its result into the *saved*
/// slot of `SharedSlowPathStubABI::kResultReg` = `R0`
/// (`stub_code_compiler_arm64.cc:326-331`; `constants_arm64.h:170-172`), so x0
/// alone changes. The two non-shared rows carry no such guarantee.
const STUB_SLOTS_3_5: &[StubSlot] = &[
    StubSlot { offset: 0xc8, name: "lateInitializationErrorSharedWithoutFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0xd0, name: "lateInitializationErrorSharedWithFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0xd8, name: "nullErrorSharedWithoutFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0xe0, name: "nullErrorSharedWithFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0xe8, name: "nullArgErrorSharedWithoutFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0xf0, name: "nullArgErrorSharedWithFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0xf8, name: "nullCastErrorSharedWithoutFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x100, name: "nullCastErrorSharedWithFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x108, name: "rangeErrorSharedWithoutFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x110, name: "rangeErrorSharedWithFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x118, name: "writeErrorSharedWithoutFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x120, name: "writeErrorSharedWithFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x128, name: "allocateMintWithFpuRegs", returns: true, writes_result: true, preserves_registers: true },
    StubSlot { offset: 0x130, name: "allocateMintWithoutFpuRegs", returns: true, writes_result: true, preserves_registers: true },
    StubSlot { offset: 0x178, name: "stackOverflowSharedWithoutFpuRegs", returns: true, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x180, name: "stackOverflowSharedWithFpuRegs", returns: true, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x1c8, name: "slowTypeTest", returns: true, writes_result: false, preserves_registers: false },
];

/// Dart 3.12, product ARM64, compressed pointers.
///
/// `returns` is `allow_return` from `GenerateSharedStub`: `false` means the stub
/// raises and control never comes back. Corroborated in both sample binaries,
/// where every `false` row reaches `brk` before any `ret` and every `true` row
/// reaches `ret` first.
///
/// `writes_result` is `store_runtime_result_in_result_register`, defaulted false
/// by the SDK and set only for the mint allocator
/// (`stub_code_compiler_arm64.cc:1481,1501`). Separate from `returns`:
/// `stackOverflow` comes back but defines no value, so binding its result would
/// be as false as binding a throw. The type test does not define one either:
/// `TypeTestABI::kInstanceReg` is `R0` and sits in `kPreservedAbiRegisters`, so
/// x0 still holds the instance afterwards, and the answer goes to
/// `kSubtypeTestCacheResultReg` = `R7`, which is not preserved
/// (`constants_arm64.h:237-258`). Only `invokeDartCode` returns in `R0`.
///
/// `preserves_registers` marks the `GenerateSharedStub` family, which pushes and
/// pops `AddAllNonReservedRegisters` around the runtime call
/// (`stub_code_compiler_arm64.cc:300,309`; `locations.h:692-703`). Those calls
/// clobber nothing, and the mint allocator writes its result into the *saved*
/// slot of `SharedSlowPathStubABI::kResultReg` = `R0`
/// (`stub_code_compiler_arm64.cc:326-331`; `constants_arm64.h:170-172`), so x0
/// alone changes. The two non-shared rows carry no such guarantee.
const STUB_SLOTS_3_12: &[StubSlot] = &[
    StubSlot { offset: 0xd0, name: "invokeDartCode", returns: true, writes_result: true, preserves_registers: false },
    StubSlot { offset: 0xe8, name: "lateInitializationErrorSharedWithoutFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0xf0, name: "lateInitializationErrorSharedWithFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0xf8, name: "nullErrorSharedWithoutFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x100, name: "nullErrorSharedWithFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x108, name: "nullArgErrorSharedWithoutFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x110, name: "nullArgErrorSharedWithFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x118, name: "nullCastErrorSharedWithoutFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x120, name: "nullCastErrorSharedWithFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x128, name: "rangeErrorSharedWithoutFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x130, name: "rangeErrorSharedWithFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x138, name: "writeErrorSharedWithoutFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x140, name: "writeErrorSharedWithFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x148, name: "fieldAccessErrorSharedWithoutFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x150, name: "fieldAccessErrorSharedWithFpuRegs", returns: false, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x158, name: "allocateMintWithFpuRegs", returns: true, writes_result: true, preserves_registers: true },
    StubSlot { offset: 0x160, name: "allocateMintWithoutFpuRegs", returns: true, writes_result: true, preserves_registers: true },
    StubSlot { offset: 0x190, name: "stackOverflowSharedWithoutFpuRegs", returns: true, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x198, name: "stackOverflowSharedWithFpuRegs", returns: true, writes_result: false, preserves_registers: true },
    StubSlot { offset: 0x1d8, name: "slowTypeTest", returns: true, writes_result: false, preserves_registers: false },
];

/// The Dart thread register on ARM64 AOT: `const Register THR = R26`
/// (`runtime/vm/constants_arm64.h`). Confirmed in the instruction stream, where
/// `x26` is the base of 2,971 of the sampled `ldr rD, [rN, #imm]` loads and the
/// next candidate is the frame pointer.
const THREAD_REGISTER: &str = "x26";

/// How far into a function the self-load may appear. `GenerateSharedStubGeneric`
/// emits it directly after the canonical register pushes
/// (`stub_code_compiler_arm64.cc:287-337`). Measured across both samples the
/// self-load sits at instruction 11 for the without-FPU variants, 21 or 22 and
/// 37 or 38 for the mint allocators, and 27 for the with-FPU ones, so a window
/// of 32 silently dropped `allocateMintWithFpuRegs` on every binary. The window
/// keeps an ordinary function that happens to load a stub slot deep in its body
/// from being named after it.
const PROLOGUE_WINDOW: usize = 48;

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
    // The allocation pass runs first and is deliberately outside every gate
    // below. It needs no vendored slot table and no SDK version: `SizeTagBits`
    // occupies bits 8..11 and `ClassIdTag` the 20 above it in both 3.5 and 3.12
    // (`raw_object.h:258-303`), so the shift is not version-dependent. Gating it
    // with the shared-stub table would have discarded 1,059 and 1,353 exact names
    // on any binary where that table refuses -- which is not hypothetical, since
    // one model yields `no_stub_prologues` on a binary whose allocation stubs are
    // perfectly nameable.
    // Per-class allocation stubs identify their class exactly. The ARM64
    // generator materialises `MakeTagWordForNewSpaceObject(cid, instance_size)`
    // into a register before tail-calling the shared allocate entry
    // (`stub_code_compiler_arm64.cc:2389-2451`), and the tag's layout is fixed:
    // `SizeTagBits` occupies bits 8..11 and `ClassIdTag` the 20 bits above it
    // (`raw_object.h:258-303`), so the class id is `(tag >> 12) & 0xfffff`.
    //
    // Validated by the shift being wrong any other way: on the two samples all
    // 1,059 and 1,353 recognised stubs yield an id below 30,000 at shift 12,
    // against 17.7% and 21.1% at shift 8, and every stub yields a distinct id,
    // which is what per-class stubs must do.
    //
    // The id is not a name. Turning it into one needs the snapshot's class table
    // (`ClassTable::At(cid)`), which this pipeline does not read, so the number is
    // reported as a number.
    let mut names = HashMap::new();
    for f in disasm {
        if let Some(cid) = allocation_stub_class_id(f) {
            names.insert(f.entry_va, format!("allocateClassId{cid}"));
        }
    }
    let allocation_named = names.len();
    let Some(slots) = stub_slots(dart_version, compressed_pointers) else {
        return SharedStubNaming::partial("unknown_key", disasm.len(), names, allocation_named);
    };
    let mut shared = 0usize;
    for f in disasm {
        if let Some(name) = prologue_stub_name(f, slots) {
            names.insert(f.entry_va, name.to_string());
            shared += 1;
        }
    }
    if shared == 0 {
        return SharedStubNaming::partial("no_stub_prologues", disasm.len(), names, allocation_named);
    }
    // The header is not the only evidence of which table applies. Every
    // vendored table matches a different number of prologues in the same
    // binary, because the offsets moved, so the observed offset set fingerprints
    // the SDK independently: on the two samples the correct table matches 14
    // prologues and the other one 7 and 8. If the header's version is not the
    // best-scoring table, the two disagree and naming tens of thousands of call
    // sites off the losing table would be a silent mislabel. Refuse instead.
    if !is_best_scoring(disasm, slots) {
        names.retain(|va, _| allocation_stub_class_id_at(disasm, *va));
        return SharedStubNaming::partial("table_disagreement", disasm.len(), names, allocation_named);
    }
    let non_returning = disasm
        .iter()
        .filter(|f| {
            prologue_stub_slot(f, slots).is_some_and(|slot| !slot.returns)
                && names.contains_key(&f.entry_va)
        })
        .map(|f| f.entry_va)
        .collect();
    // A trampoline is included too, inheriting the slot it hands off to. Its
    // register effect *is* the wrapped stub's, since its whole body is the load
    // and the branch, and its inputs are that stub's ABI rather than
    // `DartCallingConvention`. Without an entry the 409 and 457 type-test
    // trampoline sites were modelled as Dart calls: bound, argument list
    // inferred, every volatile register dropped, all three wrong.
    //
    // `non_returning` deliberately stays keyed on `prologue_stub_slot`, which
    // requires the register save. A trampoline may guard its tail call -- one of
    // the two does, with `cmp w0, w22` -- so its own fall-through is real even
    // when the stub it wraps raises.
    let effects = disasm
        .iter()
        .filter(|f| names.contains_key(&f.entry_va))
        .filter_map(|f| {
            match prologue_stub_hit(f, slots) {
                Some((slot, _, _)) => Some((
                    f.entry_va,
                    RuntimeStubEffect {
                        writes_result: slot.writes_result,
                        preserves_registers: slot.preserves_registers,
                    },
                )),
                // An allocation stub returns the new object in R0. It is not a
                // `GenerateSharedStub`, so it carries no register-preservation
                // guarantee and the conservative clobber applies.
                None => allocation_stub_class_id(f).map(|_| {
                    (
                        f.entry_va,
                        RuntimeStubEffect {
                            writes_result: true,
                            preserves_registers: false,
                        },
                    )
                }),
            }
        })
        .collect();
    SharedStubNaming {
        names,
        non_returning,
        effects,
        status: "named",
        scanned: disasm.len(),
        allocation_named,
    }
}

/// Names plus why there are that many, so a zero is self-explaining in the
/// report rather than indistinguishable from a feature that never ran. The
/// count is sensitive to how much of the binary is in scope: too few stub
/// prologues and the tables cannot be separated, which reports as
/// `table_disagreement` rather than as silence.
pub(super) struct SharedStubNaming {
    pub(super) names: HashMap<u64, String>,
    /// Entry VAs of the named stubs that raise. `allow_return=false` in the SDK,
    /// so a call to one of these never comes back and the fall-through edge the
    /// disassembler recorded does not exist.
    pub(super) non_returning: HashSet<u64>,
    /// Every named stub, mapped to whether it defines a value in the result
    /// register. Presence also means the callee is a runtime stub, so the Dart
    /// argument convention does not describe its inputs.
    pub(super) effects: HashMap<u64, RuntimeStubEffect>,
    pub(super) status: &'static str,
    /// How many of the names came from the allocation pass, which is independent
    /// of the shared-stub table and its version gate.
    pub(super) allocation_named: usize,
    /// How many functions the prologue scan looked at. Without it
    /// `no_stub_prologues` reads the same whether the binary has no stubs or the
    /// model's function table never covered the stub range -- the observed case,
    /// where one model listed 39,343 functions and none of them were the stubs
    /// while another listed 5,800 and named 14.
    pub(super) scanned: usize,
}

impl SharedStubNaming {
    /// The shared-stub table did not apply, but the allocation pass does not
    /// depend on it, so its names survive. Their effects survive with them: an
    /// allocation stub returns the new object in `R0`.
    fn partial(
        status: &'static str,
        scanned: usize,
        names: HashMap<u64, String>,
        allocation_named: usize,
    ) -> Self {
        let effects = names
            .keys()
            .map(|va| {
                (
                    *va,
                    RuntimeStubEffect {
                        writes_result: true,
                        preserves_registers: false,
                    },
                )
            })
            .collect();
        Self {
            names,
            non_returning: HashSet::new(),
            effects,
            status,
            scanned,
            allocation_named,
        }
    }
}

/// Whether the function at `va` is a per-class allocation stub.
fn allocation_stub_class_id_at(disasm: &[FunctionDisassembly], va: u64) -> bool {
    disasm
        .iter()
        .find(|f| f.entry_va == va)
        .and_then(allocation_stub_class_id)
        .is_some()
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
fn is_best_scoring(disasm: &[FunctionDisassembly], slots: &'static [StubSlot]) -> bool {
    let names = |table: &'static [StubSlot]| {
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
const ALL_STUB_TABLES: &[&[StubSlot]] = &[STUB_SLOTS_3_5, STUB_SLOTS_3_12];

/// The table for a binary, or `None` when either key is unknown. The vendored
/// rows are from the product ARM64 `DART_COMPRESSED_POINTERS` block, so an
/// uncompressed binary has no table even on a known version.
fn stub_slots(
    dart_version: Option<&str>,
    compressed_pointers: Option<bool>,
) -> Option<&'static [StubSlot]> {
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
///
/// The load alone is not enough. A thunk that tail-calls a stub reads the same
/// slot -- `ldr x24, [x26, #0x1d8]; ldur x16, [x24, #7]; br x16` -- and naming
/// that thunk after the stub names a function after the one it calls. Two per
/// sample, and one of them guards the tail call with `cmp w0, w22`, so the name
/// would have hidden a null check. `GenerateSharedStubGeneric` always saves the
/// register set first, so the self-load must follow at least one push onto the
/// Dart stack. The thunks push nothing.
fn prologue_stub_name(
    f: &FunctionDisassembly,
    slots: &'static [StubSlot],
) -> Option<String> {
    let (slot, index, saw_push) = prologue_stub_hit(f, slots)?;
    if saw_push {
        return Some(slot.name.to_string());
    }
    // No register save, so this is not the stub. It is still exactly derivable
    // when it hands straight off to the slot it read: the two per sample are
    // trampolines, and their own call sites -- 520 on LocalSend and 696 on
    // Immich -- would otherwise stay anonymous. `Thunk` says it wraps the stub
    // rather than being it, which matters because one of them guards the tail
    // call with `cmp w0, w22`.
    tail_calls_immediately(f, index).then(|| format!("{}Thunk", slot.name))
}

/// The slot a function is named from, if any: the slot, the instruction index of
/// the load, and whether a register save preceded it.
fn prologue_stub_hit(
    f: &FunctionDisassembly,
    slots: &'static [StubSlot],
) -> Option<(&'static StubSlot, usize, bool)> {
    let mut saw_push = false;
    for (index, ins) in f.instructions.iter().take(PROLOGUE_WINDOW).enumerate() {
        if is_dart_stack_push(ins) {
            saw_push = true;
            continue;
        }
        if !ins.mnemonic.eq_ignore_ascii_case("ldr") {
            continue;
        }
        let Some(offset) = thread_slot_offset(&ins.op_str) else {
            continue;
        };
        let Some(slot) = slots.iter().find(|s| s.offset == offset) else {
            continue;
        };
        return Some((slot, index, saw_push));
    }
    None
}

/// The slot a function *is*, ignoring trampolines: only a stub that saved the
/// register set identifies itself, and only that stub's `returns` flag describes
/// what a call to it does. A trampoline inherits nothing here on purpose -- it
/// may guard the tail call, so its own fall-through is real.
fn prologue_stub_slot(
    f: &FunctionDisassembly,
    slots: &'static [StubSlot],
) -> Option<&'static StubSlot> {
    match prologue_stub_hit(f, slots) {
        Some((slot, _, true)) => Some(slot),
        _ => None,
    }
}

/// Whether the function is a trampoline: the whole body after the slot load is
/// pulling the entry point out of the loaded `Code` and branching to it.
///
/// The measured sequence is `ldr x24, [x26, #slot]; ldur x16, [x24, #7];
/// br x16`, so both following instructions are required. `br` never returns,
/// which is what makes the thunk equivalent to the stub. An ordinary function
/// that calls a shared stub uses `blr` and carries on afterwards, so it is not a
/// trampoline and takes no name -- naming it would be the same false claim as
/// naming the thunk after the stub itself.
fn tail_calls_immediately(f: &FunctionDisassembly, load_index: usize) -> bool {
    let mut after = f.instructions.iter().skip(load_index + 1);
    let Some(entry_load) = after.next() else {
        return false;
    };
    let loads_entry_point = matches!(
        entry_load.mnemonic.to_ascii_lowercase().as_str(),
        "ldr" | "ldur"
    ) && entry_load.op_str.contains(", #7]");
    if !loads_entry_point {
        return false;
    }
    after
        .next()
        .is_some_and(|ins| ins.mnemonic.eq_ignore_ascii_case("br"))
}

/// A push onto the Dart stack: `str`/`stp` through the stack pointer with
/// writeback, as in `str x30, [x15, #-8]!`.
fn is_dart_stack_push(ins: &flutterdec_disasm_arm64::AsmInstruction) -> bool {
    if !ins.mnemonic.eq_ignore_ascii_case("str") && !ins.mnemonic.eq_ignore_ascii_case("stp") {
        return false;
    }
    ins.op_str.contains("[x15,") && ins.op_str.trim_end().ends_with("]!")
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
/// The class id a per-class allocation stub allocates, from the tag word its
/// prologue materialises.
///
/// The shape is `mov rD, #lo` then `movk rD, #hi, lsl #16`, which is how the
/// assembler builds a 32-bit constant. Anything else is not this stub.
fn allocation_stub_class_id(f: &FunctionDisassembly) -> Option<u64> {
    let mut instrs = f.instructions.iter();
    let mov = instrs.next()?;
    let movk = instrs.next()?;
    if !mov.mnemonic.eq_ignore_ascii_case("mov") || !movk.mnemonic.eq_ignore_ascii_case("movk") {
        return None;
    }
    let dst = mov.op_str.split(',').next()?.trim();
    if movk.op_str.split(',').next()?.trim() != dst {
        return None;
    }
    // Only the `lsl #16` form composes a tag word; without the shift the second
    // write would replace the low lane rather than extend it.
    if !movk.op_str.contains("lsl #16") {
        return None;
    }
    let tag = immediate(&mov.op_str)? | (immediate(&movk.op_str)? << 16);
    let cid = (tag >> 12) & 0xf_ffff;
    // Class 0 is the illegal id and the tag would then encode nothing useful.
    (cid > 0).then_some(cid)
}

/// The first `#`-prefixed immediate in an operand list.
fn immediate(op_str: &str) -> Option<u64> {
    let rest = op_str.split('#').nth(1)?.trim();
    let token: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == 'x')
        .collect();
    match token.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => token.parse().ok(),
    }
}

/// How much unreachable code a prune removed, for the report.
#[derive(Debug, Default, Clone)]
pub(super) struct NoreturnPrune {
    pub(super) functions: usize,
    pub(super) blocks_cut: usize,
    pub(super) instructions_cut: usize,
    /// Functions skipped because their graph failed the shared identity ruler.
    /// Should stay zero: the builder is the only producer, so a nonzero count is a
    /// builder regression and must be visible rather than silently leaving a
    /// fabricated fall-through in place.
    pub(super) skipped_invalid_ir: usize,
    pub(super) pruned: Vec<flutterdec_decompiler::BlockIdentity>,
}

/// Removes the control flow that follows a call which never returns.
///
/// The disassembler records a fall-through edge after every non-terminator, so a
/// call to a raising stub looks like it comes back. It does not, and the cost is
/// not cosmetic: on the samples 45.7% and 42.5% of functions contain such a
/// call, and 20.6% and 19.8% of all reachable blocks are only reachable through
/// one. The emitter was rendering them as live code -- binding the result of a
/// throw, reading that binding, returning it, and merging register state from a
/// path that cannot execute, which is the same impossible-output class as a field
/// read off `null`.
///
/// Blocks are deliberately NOT removed. `regions.rs` requires block ids to be
/// dense (`if b.id >= n { return None; }`) and `structured.rs` iterates
/// `0..blocks.len()` as ids, so dropping a block without renumbering would make
/// region recovery fail and push the function onto the very fallback emitter
/// this is meant to stop feeding. Cutting the edge is enough: both emitters walk
/// from the entry along successors, so an orphan is simply never visited, and
/// `Regions` recomputes reachability from what it is given.
pub(super) fn prune_calls_that_never_return(
    ir: &mut [FunctionIr],
    non_returning: &HashSet<u64>,
) -> NoreturnPrune {
    let mut stats = NoreturnPrune::default();
    if non_returning.is_empty() {
        return stats;
    }
    for f in ir {
        // Before the reachability walk below, which indexes blocks by id: on a
        // graph with a duplicate id or an edge to a block that does not exist the
        // walk reads another block's successors, and the count it produces is what
        // the report publishes as blocks removed.
        if flutterdec_ir::validate_canonical_cfg(f).is_err() {
            stats.skipped_invalid_ir += 1;
            continue;
        }
        // Measured as reachable-before minus reachable-after. The IR already
        // contains blocks no path reaches, so counting every unreachable block
        // after the cut would credit this pass with them: on one sample that
        // reads 162,081 instead of the 13,696 it actually removes.
        let reachable_before = reachable_block_ids(f);
        let mut cut_any = false;
        // Which blocks terminate, and after which instruction.
        let terminators: Vec<(usize, usize)> = f
            .blocks
            .iter()
            .filter_map(|b| {
                b.instrs
                    .iter()
                    .position(|ins| {
                        matches!(ins.op, IROp::Call)
                            && parse_target_va(&ins.target)
                                .is_some_and(|va| non_returning.contains(&va))
                    })
                    .map(|at| (b.id, at))
            })
            .collect();
        for (id, at) in terminators {
            let Some(index) = f.blocks.iter().position(|b| b.id == id) else {
                continue;
            };
            let dropped_succs = std::mem::take(&mut f.blocks[index].succs);
            let tail = f.blocks[index].instrs.len().saturating_sub(at + 1);
            f.blocks[index].instrs.truncate(at + 1);
            stats.instructions_cut += tail;
            if !dropped_succs.is_empty() || tail > 0 {
                cut_any = true;
            }
        }
        if !cut_any {
            continue;
        }
        // Successors are the authority and predecessors are re-derived from them,
        // through the one canonical path rather than through a second copy of the
        // reciprocity rule here. `helper_flow/summary.rs` reads predecessors
        // directly to score a block without cross-checking the successor side, so
        // the predecessor side is exactly the one that used to go stale.
        flutterdec_ir::rebuild_edges(&mut f.blocks);
        debug_assert_eq!(
            flutterdec_ir::validate_canonical_cfg(f),
            Ok(()),
            "the noreturn prune left a graph its consumers cannot index"
        );
        stats.functions += 1;
        let reachable_after = reachable_block_ids(f);
        for id in reachable_before.difference(&reachable_after) {
            if let Some(block) = f.blocks.iter().find(|block| block.id == *id) {
                stats.pruned.push(flutterdec_decompiler::BlockIdentity {
                    function_id: f.function_id,
                    start_va: block.start_va,
                });
            }
        }
        stats.blocks_cut += reachable_before.len().saturating_sub(reachable_after.len());
    }
    stats
        .pruned
        .sort_unstable_by_key(|identity| (identity.function_id, identity.start_va));
    stats
}

/// Blocks reachable from the entry along successor edges.
fn reachable_block_ids(f: &FunctionIr) -> HashSet<usize> {
    let Some(entry) = f.blocks.first().map(|b| b.id) else {
        return HashSet::new();
    };
    let mut seen = HashSet::new();
    let mut stack = vec![entry];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(b) = f.blocks.iter().find(|b| b.id == id) {
            stack.extend(b.succs.iter().copied());
        }
    }
    seen
}

/// The VA a `Call` target names, e.g. `"#0x17368d0"`.
fn parse_target_va(target: &str) -> Option<u64> {
    u64::from_str_radix(target.trim().trim_start_matches('#').strip_prefix("0x")?, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flutterdec_disasm_arm64::AsmInstruction;

    pub(super) fn ins(va: u64, mnemonic: &str, op_str: &str) -> AsmInstruction {
        AsmInstruction {
            va,
            word: 0,
            mnemonic: mnemonic.to_string(),
            op_str: op_str.to_string(),
            annotation: String::new(),
        }
    }

    /// A stub whose prologue pushes, then loads its own `Code` from `slot`.
    pub(super) fn stub(entry_va: u64, slot: u64) -> FunctionDisassembly {
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
#[cfg(test)]
mod window_tests {
    use super::tests::{ins, stub};
    use super::*;

    /// A stub whose self-load sits `depth` instructions in, after that many
    /// pushes.
    fn deep_stub(entry_va: u64, slot: u64, depth: usize) -> FunctionDisassembly {
        let mut instructions = Vec::with_capacity(depth + 1);
        for i in 0..depth {
            instructions.push(ins(
                entry_va + (i as u64) * 4,
                "stp",
                "x2, x3, [x15, #-0x10]!",
            ));
        }
        instructions.push(ins(
            entry_va + (depth as u64) * 4,
            "ldr",
            &format!("x16, [x26, #{slot:#x}]"),
        ));
        FunctionDisassembly {
            function_id: entry_va,
            function_name: format!("sub_{entry_va:x}"),
            owner_class: String::new(),
            entry_va,
            size: (depth as u64 + 1) * 4,
            instructions,
        }
    }

    /// The mint allocators save the FPU set as well, which puts their self-load
    /// at instruction 37 on 3.5 and 38 on 3.12. A 32-instruction window dropped
    /// them from every binary and said nothing, because a stub that goes unnamed
    /// is indistinguishable from a stub that is not there.
    #[test]
    fn a_deep_prologue_is_still_inside_the_window() {
        let disasm = vec![
            deep_stub(0x5000, 0x158, 38),
            stub(0x2100, 0x148),
            stub(0x2200, 0x190),
            stub(0x2300, 0x1d8),
        ];
        let named = shared_stub_names(&disasm, Some("3.12.1"), Some(true));
        assert!(
            named.names.contains_key(&0x2200),
            "control failed: a shallow stub went unnamed: {:?}",
            named.names
        );
        assert_eq!(
            named.names.get(&0x5000).map(String::as_str),
            Some("allocateMintWithFpuRegs"),
            "the deepest measured self-load must be reachable: {:?}",
            named.names
        );
    }
}
#[cfg(test)]
mod trampoline_tests {
    use super::tests::{ins, stub};
    use super::*;

    fn body(entry_va: u64, instrs: Vec<flutterdec_disasm_arm64::AsmInstruction>) -> FunctionDisassembly {
        FunctionDisassembly {
            function_id: entry_va,
            function_name: format!("sub_{entry_va:x}"),
            owner_class: String::new(),
            entry_va,
            size: (instrs.len() as u64) * 4,
            instructions: instrs,
        }
    }

    /// A trampoline reads the stub's `Code` from the slot, pulls the entry point
    /// out of it and branches. It never returns, so it is equivalent to the stub
    /// and its own call sites -- 520 on LocalSend, 696 on Immich -- deserve a
    /// name. `Thunk` keeps it distinct: one of the real ones guards the tail call
    /// with `cmp w0, w22`, so calling it the stub outright would hide a check.
    ///
    /// An ordinary caller uses `blr` and continues afterwards. Naming that after
    /// the stub it calls is the same false claim, so it stays anonymous.
    #[test]
    fn a_trampoline_is_named_for_what_it_wraps_but_a_caller_is_not() {
        let trampoline = body(
            0x4000,
            vec![
                ins(0x4000, "cmp", "w0, w22"),
                ins(0x4004, "b.eq", "#0x4014"),
                ins(0x4008, "ldr", "x24, [x26, #0x118]"),
                ins(0x400c, "ldur", "x16, [x24, #7]"),
                ins(0x4010, "br", "x16"),
            ],
        );
        let caller = body(
            0x6000,
            vec![
                ins(0x6000, "ldr", "x24, [x26, #0x118]"),
                ins(0x6004, "ldur", "x16, [x24, #7]"),
                ins(0x6008, "blr", "x16"),
                ins(0x600c, "ret", ""),
            ],
        );
        let disasm = vec![
            trampoline,
            caller,
            stub(0x2100, 0x148),
            stub(0x2200, 0x190),
            stub(0x2300, 0x1d8),
        ];
        let named = shared_stub_names(&disasm, Some("3.12.1"), Some(true));
        assert!(
            named.names.contains_key(&0x2200),
            "control failed: a real stub went unnamed: {:?}",
            named.names
        );
        assert_eq!(
            named.names.get(&0x4000).map(String::as_str),
            Some("nullCastErrorSharedWithoutFpuRegsThunk"),
            "a trampoline takes the wrapped name, marked: {:?}",
            named.names
        );
        assert!(
            !named.names.contains_key(&0x6000),
            "a `blr` caller is not a trampoline: {:?}",
            named.names
        );

        // The trampoline inherits the wrapped slot's call effects. Its body is
        // the load and the branch, so its register effect *is* that stub's, and
        // its inputs are that stub's ABI rather than `DartCallingConvention`.
        // Without an entry here it is modelled as a Dart call: result bound,
        // arguments inferred, every volatile register dropped.
        let thunk = named
            .effects
            .get(&0x4000)
            .copied()
            .expect("a trampoline needs the wrapped stub's effects");
        let wrapped = named
            .effects
            .get(&0x2200)
            .copied()
            .expect("control failed: the wrapped stub has no effects");
        assert_eq!(
            (thunk.writes_result, thunk.preserves_registers),
            (wrapped.writes_result, wrapped.preserves_registers),
            "a trampoline must inherit the slot it hands off to"
        );
        assert!(
            !named.effects.contains_key(&0x6000),
            "a `blr` caller is not a runtime stub and keeps the Dart-call model"
        );
    }
}

// The no-return prune identity boundary. A separate, digest-protected file rather than
// part of `prune_tests` below, because this file is product source that later
// work edits, so a digest over it would fire on legitimate change. This
// declaration is the only thing that compiles that file and cannot be digested
// either, so `scripts/check-oracle-inventory.py` proves it by compilation.
#[cfg(test)]
#[path = "stubs/identity_tests.rs"]
mod stubs_identity_tests;

#[cfg(test)]
mod prune_tests {
    use super::*;
    use flutterdec_ir::{BasicBlock, LlirInstr};

    pub(super) fn call(va: u64, target: &str) -> LlirInstr {
        LlirInstr {
            va,
            op: IROp::Call,
            src: format!("bl {target}"),
            target: target.to_string(),
        }
    }

    pub(super) fn other(va: u64, src: &str) -> LlirInstr {
        LlirInstr {
            va,
            op: IROp::Other,
            src: src.to_string(),
            target: String::new(),
        }
    }

    pub(super) fn blk(
        id: usize,
        start_va: u64,
        instrs: Vec<LlirInstr>,
        succs: Vec<usize>,
    ) -> BasicBlock {
        BasicBlock {
            id,
            start_va,
            instrs,
            succs,
            preds: Vec::new(),
        }
    }

    /// The fall-through after a raising stub is not real, and the instructions
    /// after the call in the same block are unreachable bytes of another slow
    /// path. Both go. What must NOT happen is a block being removed: `regions.rs`
    /// rejects a CFG whose ids are not dense, so dropping one would push the
    /// function onto the fallback emitter -- the opposite of the intent.
    #[test]
    fn a_call_that_never_returns_ends_its_block_without_renumbering() {
        let mut ir = vec![FunctionIr {
            function_id: 1,
            name: "sub_1000".to_string(),
            entry_va: 0x1000,
            blocks: vec![
                blk(0, 0x1000, vec![other(0x1000, "mov x0, x1")], vec![1]),
                blk(
                    1,
                    0x1004,
                    vec![
                        call(0x1004, "#0x9000"),
                        other(0x1008, "mov x2, x3"),
                        other(0x100c, "mov x4, x5"),
                    ],
                    vec![2],
                ),
                blk(2, 0x1010, vec![other(0x1010, "ret")], vec![]),
            ],
        }];
        flutterdec_ir::rebuild_edges(&mut ir[0].blocks);

        let stats = prune_calls_that_never_return(&mut ir, &HashSet::from([0x9000]));

        assert_eq!(
            flutterdec_ir::validate_canonical_cfg(&ir[0]),
            Ok(()),
            "the prune must leave a canonical graph"
        );
        assert_eq!(stats.functions, 1);
        assert_eq!(stats.blocks_cut, 1, "block 2 becomes unreachable");
        assert_eq!(stats.instructions_cut, 2, "the two bytes after the throw");

        let blocks = &ir[0].blocks;
        assert_eq!(blocks.len(), 3, "no block may be removed: ids must stay dense");
        assert_eq!(
            blocks.iter().map(|b| b.id).collect::<Vec<_>>(),
            vec![0, 1, 2],
            "ids must stay dense"
        );
        assert!(blocks[1].succs.is_empty(), "the fall-through edge is fake");
        assert_eq!(blocks[1].instrs.len(), 1, "the call itself is kept");
        assert!(
            blocks[2].preds.is_empty(),
            "preds must not go stale: helper_flow reads it directly"
        );
    }

    #[test]
    fn noreturn_pruned_identities_are_structurally_ordered() {
        let mut blocks = vec![blk(0, 0x1000, vec![call(0x1000, "#0x9000")], vec![1])];
        blocks.extend((1..=10).map(|id| {
            blk(
                id,
                0x1000 + id as u64 * 4,
                vec![other(0x1000 + id as u64 * 4, "mov x0, x1")],
                (id < 10).then_some(id + 1).into_iter().collect(),
            )
        }));
        let mut ir = vec![FunctionIr {
            function_id: 7,
            name: "sub_1000".to_string(),
            entry_va: 0x1000,
            blocks,
        }];
        flutterdec_ir::rebuild_edges(&mut ir[0].blocks);

        let stats = prune_calls_that_never_return(&mut ir, &HashSet::from([0x9000]));
        assert_eq!(
            stats
                .pruned
                .iter()
                .map(|identity| identity.start_va)
                .collect::<Vec<_>>(),
            (1..=10).map(|id| 0x1000 + id * 4).collect::<Vec<_>>()
        );
    }

    /// A stub that returns keeps its fall-through. `stackOverflow` does work and
    /// comes back, and `allocateMint` produces a value, so the flag is per slot
    /// rather than per family.
    #[test]
    fn a_call_that_returns_keeps_its_edge() {
        let mut ir = vec![FunctionIr {
            function_id: 1,
            name: "sub_1000".to_string(),
            entry_va: 0x1000,
            blocks: vec![
                blk(0, 0x1000, vec![call(0x1000, "#0x8000")], vec![1]),
                blk(1, 0x1004, vec![other(0x1004, "ret")], vec![]),
            ],
        }];
        // Through the canonical path, not by hand: a fixture whose predecessors
        // are empty fails the ruler the prune applies, so it would be skipped and
        // every assertion below would pass for the wrong reason.
        flutterdec_ir::rebuild_edges(&mut ir[0].blocks);
        let stats = prune_calls_that_never_return(&mut ir, &HashSet::from([0x9000]));
        assert_eq!(stats.functions, 0);
        assert_eq!(stats.skipped_invalid_ir, 0, "the fixture is well formed");
        assert_eq!(ir[0].blocks[0].succs, vec![1], "a returning call falls through");
    }

    /// Every row of both vendored tables must agree with the SDK's `allow_return`
    /// split. The seven error families raise; `stackOverflow`, `allocateMint`,
    /// `invokeDartCode` and `slowTypeTest` come back. Independently corroborated
    /// in both sample binaries: every non-returning stub reaches `brk` before any
    /// `ret`, and every returning one reaches `ret` first.
    #[test]
    fn the_tables_agree_with_the_sdk_allow_return_split() {
        let raises = [
            "nullError",
            "nullArgError",
            "nullCastError",
            "rangeError",
            "writeError",
            "fieldAccessError",
            "lateInitializationError",
        ];
        let mut checked = 0;
        for table in ALL_STUB_TABLES {
            for slot in *table {
                let should_raise = raises.iter().any(|r| slot.name.starts_with(r));
                assert_eq!(
                    slot.returns,
                    !should_raise,
                    "{} returns={}",
                    slot.name,
                    slot.returns
                );
                // `store_runtime_result_in_result_register` is set only for the
                // mint allocator among the shared stubs. The type test and the
                // Dart entry are not shared stubs and do produce a value.
                // Only these define a value in the result register. The type
                // test is deliberately excluded: `TypeTestABI::kInstanceReg` is
                // `R0` and preserved, so x0 still holds the instance, and the
                // answer lands in `R7` (`constants_arm64.h:237-258`).
                let defines_value =
                    slot.name.starts_with("allocateMint") || slot.name == "invokeDartCode";
                assert_eq!(
                    slot.writes_result,
                    defines_value,
                    "{} writes_result={}",
                    slot.name,
                    slot.writes_result
                );
                // A stub that raises defines nothing, so the two flags are never
                // both set: `ASSERT(!store_runtime_result || allow_return)`.
                assert!(
                    slot.returns || !slot.writes_result,
                    "{} claims to raise and to define a value",
                    slot.name
                );
                checked += 1;
            }
        }
        assert!(checked >= 30, "control failed: only {checked} slots checked");
    }
}

#[cfg(test)]
mod allocation_tests {
    use super::tests::{ins, stub};
    use super::*;

    /// A per-class allocation stub materialises
    /// `MakeTagWordForNewSpaceObject(cid, size)` before tail-calling the shared
    /// allocate entry, and the tag's layout is fixed: `SizeTagBits` at bits 8..11
    /// and `ClassIdTag` in the 20 above it, so the class id is
    /// `(tag >> 12) & 0xfffff`. Identical in 3.5 and 3.12, so this needs no
    /// version gate -- and must not sit behind one.
    fn alloc_stub(entry_va: u64, cid: u64, size: u64) -> FunctionDisassembly {
        let tag = (cid << 12) | (size << 8) | 0b1_1100;
        let lo = tag & 0xffff;
        let hi = (tag >> 16) & 0xffff;
        FunctionDisassembly {
            function_id: entry_va,
            function_name: format!("sub_{entry_va:x}"),
            owner_class: String::new(),
            entry_va,
            size: 12,
            instructions: vec![
                ins(entry_va, "mov", &format!("x2, #{lo:#x}")),
                ins(entry_va + 4, "movk", &format!("x2, #{hi:#x}, lsl #16")),
                ins(entry_va + 8, "br", "x4"),
            ],
        }
    }

    #[test]
    fn an_allocation_stub_yields_its_class_id() {
        let disasm = vec![alloc_stub(0x7000, 8270, 6)];
        let named = shared_stub_names(&disasm, Some("3.12.1"), Some(true));
        assert_eq!(
            named.names.get(&0x7000).map(String::as_str),
            Some("allocateClassId8270"),
            "the class id comes out of the tag word: {:?}",
            named.names
        );
        let effect = named.effects.get(&0x7000).copied().expect("needs effects");
        assert!(
            effect.writes_result,
            "an allocation stub returns the new object in R0"
        );
        assert!(
            !effect.preserves_registers,
            "it is not a GenerateSharedStub and carries no preservation guarantee"
        );
    }

    /// The allocation pass must survive every shared-stub refusal, because it
    /// depends on neither the vendored slot table nor the SDK version. Gating it
    /// discarded 1,059 and 1,353 exact names on a binary whose shared-stub table
    /// did not apply.
    #[test]
    fn allocation_names_survive_a_shared_stub_refusal() {
        let disasm = vec![alloc_stub(0x7000, 8270, 6)];
        for (version, compressed, expected) in [
            (Some("3.12.1"), Some(true), "no_stub_prologues"),
            (Some("9.9.9"), Some(true), "unknown_key"),
            (Some("3.12.1"), Some(false), "unknown_key"),
            (None, None, "unknown_key"),
        ] {
            let named = shared_stub_names(&disasm, version, compressed);
            assert_eq!(named.status, expected, "({version:?}, {compressed:?})");
            assert_eq!(
                named.names.get(&0x7000).map(String::as_str),
                Some("allocateClassId8270"),
                "the allocation name must survive {expected}: {:?}",
                named.names
            );
            assert_eq!(named.allocation_named, 1, "reported separately");
        }
    }

    /// Only the shifted form composes a tag word. Without `lsl #16` the second
    /// write replaces the low lane instead of extending it, so the constant is
    /// not a tag and the function is not this stub.
    #[test]
    fn an_unshifted_pair_is_not_a_tag_word() {
        let mut disasm = vec![alloc_stub(0x7000, 8270, 6), stub(0x2200, 0x190)];
        disasm[0].instructions[1] = ins(0x7004, "movk", "x2, #0x2");
        let named = shared_stub_names(&disasm, Some("3.12.1"), Some(true));
        assert!(
            !named.names.contains_key(&0x7000),
            "an unshifted movk does not build a tag: {:?}",
            named.names
        );
    }
}
