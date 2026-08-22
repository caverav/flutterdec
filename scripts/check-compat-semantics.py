#!/usr/bin/env python3
"""Machine-level oracle for the real-binary semantic differences.

`docs/compat-baseline-real-binary.md` classifies every difference between the
reference and candidate decompiles of the pinned LocalSend release. Four of them
are behaviour-affecting and cannot be settled by counting: a guard whose terminal
comparison flips, two files that lose `sel<N>(` renderings, one call that loses
an argument, and the unresolved-control-flow accounting. This script adjudicates
those four from the machine code, and nothing else:

  derive   read both output trees, re-derive each claim from the ARM64
           instruction spans, the branch destinations they encode and the two
           emitted pseudocode files, and print the derivation.
  check    the same derivation, compared against the committed record in
           docs/compat-evidence/semantic-adjudication.json; any disagreement is
           a non-zero exit.
  verify   offline: the committed record is internally consistent, its
           conclusions are the closed vocabulary, and every site it names is
           named in the prose document.
  --self-test  the derivation rules themselves, each against a planted
           violation.

Nothing here reads a prior report, a prior pseudocode rendering, or the
emitter's own opinion of what it did: a guard's polarity comes from the
condition code and the destination of the branch that guards it, reachability
comes from the ARM64 control effect of each terminator, and the argument
question comes from the writes to the argument registers before the `bl`.
"""

import argparse
import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
EVIDENCE = REPO / "docs" / "compat-evidence"
RECORD = EVIDENCE / "semantic-adjudication.json"
DOC = REPO / "docs" / "compat-baseline-real-binary.md"

ASM_LINE = re.compile(r"^0x([0-9a-f]+): (?:[0-9a-f]{8} )?(.*)$")

# Dart's stack-limit check, which is the body every flagged guard guards:
# `ldp` of two adjacent Thread fields, `+ 0x10`, `cmp`, and a `b.ls` into the
# shared stack-overflow stub. `x26` is THR in Dart's ARM64 ABI.
THREAD_REG = "x26"
STACK_LIMIT_SLOT = "#0x50"
GUARD_MASK = "0xc0000000"
STACK_OVERFLOW_STUB = "0xd51af0"

# `DART_ARGUMENT_REGISTERS` in the emitter, which is the position list a call
# rendering claims arity against. `x0` is not in it: it is the return register.
DART_ARGUMENT_REGISTERS = ("x1", "x2", "x3", "x5", "x6", "x7")

# The statements that report unresolved control flow, by prefix. The first three
# are the emitters' own; the fourth is the whole body of a function whose CFG did
# not validate, which is one unresolved site and is emitted by neither walk.
UNRESOLVED_PREFIXES = (
    "// indirect branch",
    "// unresolved branch target",
    "// unresolved jump",
    "// invalid CFG",
)

# A conditional branch tests flags a comparison set. `tst Xn, #imm` is
# `ANDS xzr, Xn, #imm`, so Z is set exactly when `(Xn & imm) == 0` (Arm ARM
# DDI 0487, C6.2 TST, ANDS). A guard whose body is the *fall-through* of the
# branch therefore runs under the negation of the branch condition.
FALLTHROUGH_POLARITY = {"b.eq": "!= 0", "b.ne": "== 0"}

# ARM64 terminators with no fall-through: control does not continue to the next
# instruction. `br`/`braa`-style register branches leave through a value, `ret`
# returns, `brk`/`hlt`/`udf` trap, and an unconditional `b` transfers.
NO_FALLTHROUGH = ("br ", "braa", "brab", "ret", "brk", "hlt", "udf", "b ")

CONCLUSIONS = (
    "candidate_correct_reference_wrong",
    "candidate_correct_reference_unreachable_code",
    "candidate_correct_no_recovered_value_lost",
    "candidate_corrected_after_this_slice",
)


def fail(message):
    print(f"[compat-semantics] {message}", file=sys.stderr)
    sys.exit(1)


# ---------------------------------------------------------------- asm decoding


def read_asm(tree, stem):
    """`{va: text}` and the ascending address list for one emitted asm file."""
    path = tree / "asm" / f"{stem}.s"
    instrs = {}
    order = []
    for line in path.read_text().splitlines():
        match = ASM_LINE.match(line)
        if not match:
            continue
        va = int(match.group(1), 16)
        instrs[va] = match.group(2).split(";")[0].strip()
        order.append(va)
    if not order:
        fail(f"{path} decoded no instruction")
    return instrs, order


def branch_target(text):
    """The absolute destination an ARM64 branch encodes, or None."""
    match = re.search(r"#(0x[0-9a-f]+)$", text)
    return int(match.group(1), 16) if match else None


def falls_through(text):
    return not any(text.startswith(prefix) for prefix in NO_FALLTHROUGH)


def address_successors(instrs, order, invented_fallthrough=()):
    """Address-level successors, from the ARM64 control effect of each text.

    `invented_fallthrough` adds a fall-through edge after an instruction that
    does not have one, which is the reference's classification error expressed
    as a graph edge rather than as a claim about the reference.
    """
    succs = {}
    for index, va in enumerate(order):
        text = instrs[va]
        out = set()
        nxt = order[index + 1] if index + 1 < len(order) else None
        if (falls_through(text) or va in invented_fallthrough) and nxt is not None:
            out.add(nxt)
        target = branch_target(text)
        # A `bl`/`blr` returns to the next instruction; its destination is
        # another function and is not an edge in this graph.
        if target is not None and not text.startswith("bl") and target in instrs:
            out.add(target)
        succs[va] = out
    return succs


def reachable(succs, entry):
    seen = {entry}
    stack = [entry]
    while stack:
        for nxt in succs.get(stack.pop(), ()):
            if nxt not in seen:
                seen.add(nxt)
                stack.append(nxt)
    return seen


# -------------------------------------------------------------- emitted text


def pseudocode(tree, stem):
    return (tree / "pseudocode" / f"{stem}.dartpseudo").read_text().splitlines()


def guard_lines(lines):
    """Emitted guards over the Dart stack-limit check, with their polarity."""
    found = []
    for index, line in enumerate(lines):
        text = line.strip()
        for polarity in ("== 0", "!= 0"):
            if not text.endswith(f"& {GUARD_MASK}) {polarity}) {{"):
                continue
            if index + 1 < len(lines) and "thread.f88 <=" in lines[index + 1]:
                found.append((polarity, text))
    return found


def unresolved_statements(lines):
    return [
        line.strip()
        for line in lines
        if any(line.strip().startswith(prefix) for prefix in UNRESOLVED_PREFIXES)
    ]


def callee_renderings(lines, callee):
    return [line.strip() for line in lines if f"{callee}(" in line]


# ------------------------------------------------------- derivation: polarity


def guard_sites(instrs, order):
    """Every `tst`-plus-conditional-branch guard over the stack-limit check.

    A site is only a site when the fall-through really is the checked body: the
    instructions between the branch and its destination hold the `ldp` of the
    Thread limit pair, the `+ 0x10`, the `cmp`, and a `b.ls` whose destination
    calls the shared stack-overflow stub. That is what makes the guarded body
    the one the emitted `if` renders.
    """
    sites = []
    for index, va in enumerate(order):
        text = instrs[va]
        if not (text.startswith("tst ") and GUARD_MASK in text):
            continue
        operand = text.split()[1].rstrip(",")
        branch_va = order[index + 1] if index + 1 < len(order) else None
        if branch_va is None:
            continue
        branch = instrs[branch_va]
        mnemonic = branch.split()[0] if branch else ""
        if mnemonic not in FALLTHROUGH_POLARITY:
            continue
        dest = branch_target(branch)
        body_start = order[index + 2] if index + 2 < len(order) else None
        if dest is None or body_start is None or dest <= body_start:
            continue
        body = [a for a in order if body_start <= a < dest]
        limit_load = [
            a
            for a in body
            if instrs[a].startswith("ldp ") and f"[{THREAD_REG}, {STACK_LIMIT_SLOT}]" in instrs[a]
        ]
        bump = [a for a in body if instrs[a].startswith("add ") and instrs[a].endswith("#0x10")]
        slow = [a for a in body if instrs[a].startswith("b.ls ")]
        if not (limit_load and bump and slow):
            continue
        slow_dest = branch_target(instrs[slow[0]])
        stub_call = None
        if slow_dest is not None:
            for a in order:
                if a >= slow_dest and instrs[a].startswith("b"):
                    if instrs[a].startswith("bl ") and STACK_OVERFLOW_STUB in instrs[a]:
                        stub_call = a
                    break
                if a >= slow_dest and instrs[a].startswith("bl ") and STACK_OVERFLOW_STUB in instrs[a]:
                    stub_call = a
                    break
        if stub_call is None:
            continue
        sites.append(
            {
                "mask_test_va": f"0x{va:x}",
                "mask_test": text,
                "tested_register": operand,
                "branch_va": f"0x{branch_va:x}",
                "branch": branch,
                "branch_destination": f"0x{dest:x}",
                "guarded_body_first_va": f"0x{body_start:x}",
                "stack_limit_load_va": f"0x{limit_load[0]:x}",
                "slow_path_branch_va": f"0x{slow[0]:x}",
                "stack_overflow_call_va": f"0x{stub_call:x}",
                "stack_overflow_call": instrs[stub_call],
                "derived_guard": FALLTHROUGH_POLARITY[mnemonic],
            }
        )
    return sites


def derive_polarity(ref, cand, files):
    out = []
    for stem in files:
        instrs, order = read_asm(cand, stem)
        ref_instrs, ref_order = read_asm(ref, stem)
        if (instrs, order) != (ref_instrs, ref_order):
            fail(f"asm/{stem}.s is not identical on the two sides")
        sites = guard_sites(instrs, order)
        ref_guards = guard_lines(pseudocode(ref, stem))
        cand_guards = guard_lines(pseudocode(cand, stem))
        out.append(
            {
                "function": f"{stem}.dartpseudo",
                "sites": sites,
                "derived_guards": sorted({s["derived_guard"] for s in sites}),
                "reference_guards": [p for p, _ in ref_guards],
                "candidate_guards": [p for p, _ in cand_guards],
            }
        )
    return out


# ------------------------------------------------------ derivation: selectors


def derive_selectors(ref, cand, entries):
    out = []
    for entry in entries:
        stem = entry["function"].replace(".dartpseudo", "")
        selector = entry["selector"]
        instrs, order = read_asm(cand, stem)
        entry_va = order[0]
        honest = reachable(address_successors(instrs, order), entry_va)
        indirect = [va for va in order if instrs[va].startswith("br ")]
        invented = reachable(
            address_successors(instrs, order, invented_fallthrough=set(indirect)), entry_va
        )
        dispatch = [va for va in order if instrs[va].startswith("blr ")]
        out.append(
            {
                "function": entry["function"],
                "selector": selector,
                "entry_va": f"0x{entry_va:x}",
                "register_branch_vas": [f"0x{va:x}" for va in indirect],
                "dispatch_call_vas": [f"0x{va:x}" for va in dispatch],
                "dispatch_calls_reachable_without_invented_fallthrough": sorted(
                    f"0x{va:x}" for va in dispatch if va in honest
                ),
                "dispatch_calls_reachable_with_invented_fallthrough": sorted(
                    f"0x{va:x}" for va in dispatch if va in invented
                ),
                "reference_renderings": len(callee_renderings(pseudocode(ref, stem), selector)),
                "candidate_renderings": len(callee_renderings(pseudocode(cand, stem), selector)),
            }
        )
    return out


# ------------------------------------------------------- derivation: argument


def derive_argument(ref, cand, entry):
    stem = entry["function"].replace(".dartpseudo", "")
    callee = entry["callee"]
    instrs, order = read_asm(cand, stem)
    call_vas = [va for va in order if instrs[va].startswith("bl ") and entry["callee_va"] in instrs[va]]
    if len(call_vas) != 1:
        fail(f"{stem}: expected one call to {callee}, found {len(call_vas)}")
    call_va = call_vas[0]
    # Every write to an argument register between function entry and the call,
    # in order. A `mov Xd, Xs` is the only shape that has to be followed to
    # decide a pass-through, so anything else is reported as an opaque write.
    writes = []
    values = {}
    for va in order:
        if va >= call_va:
            break
        text = instrs[va]
        parts = text.split()
        if len(parts) < 2:
            continue
        dest = parts[1].rstrip(",")
        if dest not in DART_ARGUMENT_REGISTERS + ("x0",):
            continue
        if text.startswith("mov ") and len(parts) == 3:
            source = parts[2]
            values[dest] = values.get(source, f"entry:{source}")
            writes.append({"va": f"0x{va:x}", "text": text, "value": values[dest]})
        elif not text.startswith(("stp", "str", "stur", "cmp", "tst", "b")):
            values[dest] = f"opaque@0x{va:x}"
            writes.append({"va": f"0x{va:x}", "text": text, "value": values[dest]})
    argument_values = {
        reg: values.get(reg, f"entry:{reg}") for reg in DART_ARGUMENT_REGISTERS
    }
    return {
        "function": entry["function"],
        "callee": callee,
        "call_va": f"0x{call_va:x}",
        "call": instrs[call_va],
        "argument_register_order": list(DART_ARGUMENT_REGISTERS),
        "argument_register_writes": writes,
        "argument_register_values_at_call": argument_values,
        "pass_through_positions": [
            reg
            for reg in DART_ARGUMENT_REGISTERS
            if argument_values[reg] == f"entry:{reg}"
        ],
        "informative_positions": [
            reg
            for reg in DART_ARGUMENT_REGISTERS
            if argument_values[reg] not in (f"entry:{reg}",)
            and not argument_values[reg].startswith("opaque")
        ],
        "reference_renderings": callee_renderings(pseudocode(ref, stem), callee),
        "candidate_renderings": callee_renderings(pseudocode(cand, stem), callee),
    }


# ----------------------------------------------------- derivation: accounting


def derive_accounting(tree, residue_function):
    total = 0
    functions = 0
    for path in sorted((tree / "pseudocode").glob("*.dartpseudo")):
        statements = unresolved_statements(path.read_text().splitlines())
        if statements:
            functions += 1
            total += len(statements)
    quality = json.loads((tree / "quality.json").read_text())
    report = json.loads((tree / "report.json").read_text())
    stem = residue_function.replace(".dartpseudo", "")
    residue = unresolved_statements(pseudocode(tree, stem))
    ir = json.loads((tree / "ir" / f"{stem}.json").read_text())
    sites = [
        {"start_va": f"0x{block['start_va']:x}", "va": f"0x{instr['va']:x}", "src": instr["src"]}
        for block in ir["blocks"]
        for instr in block["instrs"]
        if instr["op"] == "IndirectBranch"
    ]
    return {
        "functions_with_statements": functions,
        "unresolved_cf_statements": total,
        "quality_unresolved_cf": quality["unresolved_cf"],
        "report_unresolved_cf": report["quality"]["unresolved_cf"],
        "residue_function": residue_function,
        "residue_function_statements": len(residue),
        "residue_function_indirect_branches": sites,
    }


# ------------------------------------------------------------------ adjudicate


def derive(ref, cand, record):
    return {
        "guard_polarity": derive_polarity(ref, cand, record["guard_polarity"]["asm_files"]),
        "selector_losses": derive_selectors(ref, cand, record["selector_losses"]["files"]),
        "dropped_call_argument": derive_argument(
            ref, cand, record["dropped_call_argument"]["site"]
        ),
        "accounting_candidate": derive_accounting(
            cand, record["control_flow_accounting"]["residue_function"]
        ),
        "accounting_reference": derive_accounting(
            ref, record["control_flow_accounting"]["residue_function"]
        ),
    }


def check(ref, cand):
    record = json.loads(RECORD.read_text())
    derived = derive(ref, cand, record)
    problems = []

    # 1. Guard polarity. Every site derives the same guard from the machine
    #    code, the candidate renders only that guard, and the reference renders
    #    the recorded number of inverted ones.
    polarity = record["guard_polarity"]
    derived_guards = {g for f in derived["guard_polarity"] for g in f["derived_guards"]}
    if derived_guards != {polarity["derived_guard"]}:
        problems.append(
            f"the machine code derives {sorted(derived_guards)}, the record claims "
            f"{polarity['derived_guard']}"
        )
    sites = sum(len(f["sites"]) for f in derived["guard_polarity"])
    if sites != polarity["sites"]:
        problems.append(f"{sites} guard sites in the asm, the record claims {polarity['sites']}")
    cand_guards = [g for f in derived["guard_polarity"] for g in f["candidate_guards"]]
    ref_guards = [g for f in derived["guard_polarity"] for g in f["reference_guards"]]
    if set(cand_guards) != {polarity["derived_guard"]}:
        problems.append(f"the candidate renders {sorted(set(cand_guards))}, not only the derived guard")
    inverted = [g for g in ref_guards if g != polarity["derived_guard"]]
    if len(inverted) != polarity["reference_inverted_guards"]:
        problems.append(
            f"the reference renders {len(inverted)} inverted guards, the record claims "
            f"{polarity['reference_inverted_guards']}"
        )
    if len(ref_guards) != polarity["emitted_guards"] or len(cand_guards) != polarity["emitted_guards"]:
        problems.append(
            f"{len(ref_guards)} reference and {len(cand_guards)} candidate emitted guards, the "
            f"record claims {polarity['emitted_guards']} on each side"
        )
    recorded_sites = {s["mask_test_va"]: s for f in polarity["files"] for s in f["sites"]}
    for f in derived["guard_polarity"]:
        for site in f["sites"]:
            recorded = recorded_sites.get(site["mask_test_va"])
            if recorded is None:
                problems.append(f"guard site {site['mask_test_va']} is not in the record")
            elif recorded != site:
                problems.append(f"guard site {site['mask_test_va']} does not match the record")

    # 2. Selector losses. The dispatch calls are unreachable under the ARM64
    #    control effect of `br`, and reachable only with the fall-through the
    #    reference invented after it.
    for derived_entry, recorded in zip(derived["selector_losses"], record["selector_losses"]["files"]):
        if derived_entry["dispatch_calls_reachable_without_invented_fallthrough"]:
            problems.append(
                f"{derived_entry['function']}: dispatch calls "
                f"{derived_entry['dispatch_calls_reachable_without_invented_fallthrough']} are "
                "reachable without an invented fall-through"
            )
        gained = set(derived_entry["dispatch_calls_reachable_with_invented_fallthrough"])
        if set(recorded["reachable_only_with_invented_fallthrough"]) != gained:
            problems.append(
                f"{derived_entry['function']}: the invented fall-through reaches {sorted(gained)}, "
                f"the record claims {recorded['reachable_only_with_invented_fallthrough']}"
            )
        if derived_entry["reference_renderings"] != recorded["reference_renderings"]:
            problems.append(
                f"{derived_entry['function']}: {derived_entry['reference_renderings']} reference "
                f"renderings, the record claims {recorded['reference_renderings']}"
            )
        if derived_entry["candidate_renderings"] != 0:
            problems.append(
                f"{derived_entry['function']}: the candidate still renders "
                f"{derived_entry['candidate_renderings']} of {derived_entry['selector']}"
            )

    # 3. Dropped call argument. The only argument-register value at the call is
    #    a pass-through of the register's own entry value, which the emitter's
    #    lower bound does not claim, and the token the reference printed names a
    #    register that is not an argument position at all.
    argument = derived["dropped_call_argument"]
    recorded = record["dropped_call_argument"]
    if argument["call_va"] != recorded["site"]["call_va"]:
        problems.append(f"the call is at {argument['call_va']}, the record claims {recorded['site']['call_va']}")
    if argument["informative_positions"]:
        problems.append(
            f"argument positions {argument['informative_positions']} carry a defined value, so the "
            "candidate dropped a recovered argument"
        )
    if argument["pass_through_positions"] != recorded["pass_through_positions"]:
        problems.append(
            f"pass-through positions {argument['pass_through_positions']} do not match the record "
            f"{recorded['pass_through_positions']}"
        )
    if argument["reference_renderings"] != recorded["reference_renderings"]:
        problems.append("the reference call rendering is not the recorded one")
    if argument["candidate_renderings"] != recorded["candidate_renderings"]:
        problems.append("the candidate call rendering is not the recorded one")
    if recorded["dropped_token_register"] in argument["argument_register_order"]:
        problems.append(
            f"{recorded['dropped_token_register']} is an argument position, so the dropped token "
            "was an arity claim"
        )

    # 4. Accounting. The counter is the number of unresolved-control-flow
    #    statements the artifacts carry, on both sides.
    for side, derived_side in (("candidate", derived["accounting_candidate"]), ("reference", derived["accounting_reference"])):
        recorded = record["control_flow_accounting"][side]
        for key in ("unresolved_cf_statements", "quality_unresolved_cf", "report_unresolved_cf", "functions_with_statements"):
            if derived_side[key] != recorded[key]:
                problems.append(
                    f"{side}.{key} derives {derived_side[key]}, the record claims {recorded[key]}"
                )
        if derived_side["unresolved_cf_statements"] != derived_side["quality_unresolved_cf"]:
            problems.append(
                f"{side}: {derived_side['unresolved_cf_statements']} statements against "
                f"quality.json.unresolved_cf {derived_side['quality_unresolved_cf']}"
            )
    residue = derived["accounting_candidate"]
    recorded = record["control_flow_accounting"]
    if residue["residue_function_statements"] != recorded["residue_function_statements"]:
        problems.append("the residue function does not carry the recorded statement count")
    if len(residue["residue_function_indirect_branches"]) != recorded["residue_function_indirect_branches"]:
        problems.append("the residue function does not carry the recorded indirect branches")

    if problems:
        for problem in problems:
            print(f"[compat-semantics] {problem}", file=sys.stderr)
        sys.exit(1)
    print(
        "[compat-semantics] ok: "
        f"{sites} guard sites derive {polarity['derived_guard']}, "
        f"{len(record['selector_losses']['files'])} selector files unreachable without an invented "
        "fall-through, "
        f"1 call argument declined with no recovered value lost, "
        f"{residue['unresolved_cf_statements']} unresolved-control-flow statements equal "
        f"quality.json.unresolved_cf"
    )


def verify():
    record = json.loads(RECORD.read_text())
    doc = DOC.read_text()
    problems = []
    for item, block in record.items():
        if not isinstance(block, dict):
            continue
        conclusion = block.get("conclusion")
        if conclusion not in CONCLUSIONS:
            problems.append(f"{item} carries the conclusion {conclusion!r}, which is not one of {CONCLUSIONS}")
        if not block.get("evidence"):
            problems.append(f"{item} records no evidence sentence")
    for entry in record["guard_polarity"]["files"]:
        for site in entry["sites"]:
            if site["mask_test_va"] not in doc:
                problems.append(f"the guard site {site['mask_test_va']} is not named in {DOC.name}")
            if site["derived_guard"] != record["guard_polarity"]["derived_guard"]:
                problems.append(f"guard site {site['mask_test_va']} disagrees with the derived guard")
    recorded_sites = sum(len(entry["sites"]) for entry in record["guard_polarity"]["files"])
    if recorded_sites != record["guard_polarity"]["sites"]:
        problems.append(
            f"the per-file guard sites sum to {recorded_sites}, the record claims "
            f"{record['guard_polarity']['sites']}"
        )
    for entry in record["selector_losses"]["files"]:
        if entry["function"] not in doc:
            problems.append(f"the selector-loss file {entry['function']} is not named in {DOC.name}")
        for va in entry["reachable_only_with_invented_fallthrough"]:
            if va not in doc:
                problems.append(f"the dispatch call {va} is not named in {DOC.name}")
    site = record["dropped_call_argument"]["site"]
    for value in (site["function"], site["call_va"], site["callee"]):
        if value not in doc:
            problems.append(f"{value} is not named in {DOC.name}")
    accounting = record["control_flow_accounting"]
    if str(accounting["candidate"]["unresolved_cf_statements"]) not in doc:
        problems.append("the candidate statement count is not stated in the prose document")
    if accounting["residue_function"] not in doc:
        problems.append(f"{accounting['residue_function']} is not named in {DOC.name}")
    for open_item in ("carried as an open semantic item", "no committed oracle proves"):
        if open_item in doc:
            problems.append(f"{DOC.name} still carries the phrase {open_item!r}")
    if problems:
        for problem in problems:
            print(f"[compat-semantics] {problem}", file=sys.stderr)
        sys.exit(1)
    print(
        "[compat-semantics] ok: 4 adjudications, "
        f"{recorded_sites} guard sites, every site named in {DOC.name}"
    )


def self_test():
    # A `tst` sets Z when the masked bits are zero, so the fall-through body of
    # a `b.eq` runs when they are not, and a `b.ne` is the other way round.
    assert FALLTHROUGH_POLARITY["b.eq"] == "!= 0"
    assert FALLTHROUGH_POLARITY["b.ne"] == "== 0"

    instrs = {
        0x100: "tst x3, #0xc0000000",
        0x104: "b.eq #0x120",
        0x108: "ldp x0, x4, [x26, #0x50]",
        0x10C: "add x0, x0, #0x10",
        0x110: "cmp x4, x0",
        0x114: "b.ls #0x130",
        0x118: "str x0, [x26, #0x50]",
        0x11C: "mov x1, x0",
        0x120: "ret",
        0x130: "bl #0xd51af0",
    }
    order = sorted(instrs)
    sites = guard_sites(instrs, order)
    assert len(sites) == 1, sites
    assert sites[0]["derived_guard"] == "!= 0", sites
    assert sites[0]["stack_overflow_call_va"] == "0x130", sites
    # Planted: the same span with `b.ne` derives the other polarity, so the rule
    # reads the condition code instead of hardcoding the answer.
    flipped = {**instrs, 0x104: "b.ne #0x120"}
    assert guard_sites(flipped, order)[0]["derived_guard"] == "== 0"
    # Planted: without the stack-overflow call the span is not this guard shape.
    without_stub = {**instrs, 0x130: "nop"}
    assert guard_sites(without_stub, order) == []
    # Planted: a mask test whose branch is not the next instruction is not a site.
    detached = {0x100: instrs[0x100], 0x104: "nop", 0x108: "b.eq #0x120", 0x120: "ret"}
    assert guard_sites(detached, sorted(detached)) == []

    # `br` has no fall-through, so the bytes after it are unreachable unless a
    # branch lands on them; inventing the fall-through is what reaches them.
    graph = {
        0x200: "cbz x0, #0x210",
        0x204: "br x16",
        0x208: "blr x9",
        0x20C: "ret",
        0x210: "ret",
    }
    order = sorted(graph)
    honest = reachable(address_successors(graph, order), 0x200)
    assert 0x208 not in honest, honest
    invented = reachable(address_successors(graph, order, invented_fallthrough={0x204}), 0x200)
    assert 0x208 in invented, invented
    # A `bl` destination is another function, so it is not an edge here, and it
    # does fall through to its own return address.
    call = {0x300: "bl #0x400", 0x304: "ret", 0x400: "ret"}
    succs = address_successors(call, sorted(call))
    assert succs[0x300] == {0x304}, succs

    # Statement counting is by prefix, so an annotation spliced into a marker
    # keeps it, and a trailing `// indirect via:` comment on a call line is not
    # one.
    lines = [
        "  // indirect branch through reg2: target not recovered",
        "  // indirect branch through reg2 /* = slot0.f8 */: target not recovered",
        "  final t1 = cachedTarget(reg1, reg2); // indirect via: cachedTarget",
        "  // control rejoins block 4: already emitted above",
        "  // unresolved jump",
    ]
    assert len(unresolved_statements(lines)) == 3, unresolved_statements(lines)

    # A guard is only an emitted guard when the next line is the checked body.
    guards = guard_lines(
        [
            "if ((reg3 & 0xc0000000) != 0) {",
            "  if (thread.f88 <= (thread.f80 + 0x10)) {",
            "if ((reg4 & 0xc0000000) == 0) {",
            "  final t1 = sub_1234();",
        ]
    )
    assert guards == [("!= 0", "if ((reg3 & 0xc0000000) != 0) {")], guards
    print("[compat-semantics] self-test ok: polarity, reachability, statements, guard shape")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", nargs="?", default="verify", choices=("derive", "check", "verify"))
    parser.add_argument("--reference", type=Path)
    parser.add_argument("--candidate", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    if args.mode == "verify":
        verify()
        return
    if not (args.reference and args.candidate):
        fail(f"{args.mode} needs --reference and --candidate output trees")
    if args.mode == "derive":
        record = json.loads(RECORD.read_text())
        print(json.dumps(derive(args.reference, args.candidate, record), indent=2, sort_keys=True))
        return
    check(args.reference, args.candidate)


main()
