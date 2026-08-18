#!/usr/bin/env python3
"""Check a value-annotation provenance audit against its own snapshots, the
emitted IR, and the emitted pseudocode.

The audit is JSONL written by the decompiler when `FLUTTERDEC_PROV_AUDIT` names
a path. Two record kinds share the stream, told apart by `record`:

    snapshot    {snapshot_id, site_key, registers: [[reg, value], ...]}
    annotation  {function_id, output_line, output_col, loss_site, site_key,
                 register, candidates: [{path_key, value, snapshot_id}, ...]}

Violations are counted **per candidate element**, never per annotation: a record
carrying one sound attribution and two invented ones is two violations, not a
satisfied row.

Checks, each reported separately so a zero total is not one check carrying three
that never ran:

  snapshot   the candidate's (site_key, path_key, register, value) appears in
             the snapshot its own `snapshot_id` names
  schema     schema_version, site_key and snapshot_id are present, and every
             site_key tag is one of the three declared spaces
  unique     no two annotation records claim one (function_id, output_line,
             output_col)
  ir         --ir-dir: a call site_key and every call path_key resolve, in the
             independently emitted IR, to a construct of the declared kind, and
             the register really loses its binding there
  anchor     --pseudocode-dir: the call annotation actually at that coordinate is
             the one this record describes
  loop_ir    --ir-dir: the same, for loop-entry records - the site is a loop
             header in the emitted IR, every path_key is one of its non-back-edge
             predecessors, and the register is written under the header, so the
             merge really did drop it
  loop_anchor --pseudocode-dir: the loop-entry annotation at that coordinate is
             this record's, carries its deduplicated values, and sits beside a
             register still spelled as an unrecovered value

Exit status is 1 when any check reports a violation, so the script is usable as
a gate.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys
from collections import Counter, defaultdict

SITE_TAGS = ("join", "loop", "call")

# The tags a candidate's `path_key` may carry. An incoming path is not a site: at a
# join or a loop header it is a predecessor block, and at a call it is the
# clobbering call itself. Checking `path_key` against SITE_TAGS would reject the
# join and loop sites' own correct keys, which is what it did once.
PATH_TAGS = ("block", "call")

# The one call-loss literal, delimiters included. Mirrors
# `PRE_CALL_ANNOTATION` in crates/flutterdec-decompiler/src/helpers/annotation.rs.
CALL_OPEN = " /* value before this call: "
# The one loop-entry literal, same file, same discipline. A unit test asserts the
# constant and this string are the same bytes, so the corpus scan cannot go
# quietly vacuous when the literal is reworded.
LOOP_OPEN = " /* loop-entry value: "
ANNOTATION_CLOSE = " */"
CANDIDATE_SEPARATOR = " | "

# Registers an ordinary call does not preserve, and mnemonics that write no
# register: `CALL_CLOBBERED_REGISTERS` and the store/compare/branch forms in
# crates/flutterdec-decompiler/src/helpers.
VOLATILE = [f"x{index}" for index in list(range(15)) + [16, 17, 18, 30]]
WRITEBACK_PRE = re.compile(r"\[[wx](\d+)[^]]*\]!")
WRITEBACK_POST = re.compile(r"\[[wx](\d+)\]\s*,\s*#")
WRITES_NOTHING = (
    "str", "stur", "strb", "strh", "sturb", "sturh", "stp", "stnp", "stlr", "stxr",
    "cmp", "cmn", "tst", "ret", "nop", "prfm", "dmb", "dsb", "isb", "svc", "brk",
    "b", "bl", "blr", "br", "cbz", "cbnz", "tbz", "tbnz", "hlt", "yield",
)


def load(path: pathlib.Path):
    snapshots, annotations, malformed = {}, [], []
    with path.open() as handle:
        for number, line in enumerate(handle, 1):
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError as error:
                malformed.append((number, str(error)))
                continue
            if row.get("record") == "snapshot":
                # Keyed by function as well as id: a `snapshot_id` names a capture
                # within one function, so a global key silently resolves a
                # candidate against another function's snapshot of the same shape -
                # which reported 38 violations against a sound emitter once.
                snapshots[(row.get("function_id"), row.get("snapshot_id"))] = row
            elif row.get("record") == "annotation":
                annotations.append(row)
            else:
                malformed.append((number, "unknown record kind"))
    return snapshots, annotations, malformed


def key_of(value):
    """A site key as a comparable tuple, or None when it is not one."""
    if isinstance(value, list) and len(value) == 2 and isinstance(value[0], str):
        return (value[0], value[1])
    return None


def check_schema(annotations, snapshots):
    """Required fields, and site keys inside a declared space."""
    bad = []
    for record in annotations:
        where = (record.get("function_id"), record.get("output_line"), record.get("output_col"))
        if "schema_version" not in record:
            bad.append((where, None, "record has no schema_version"))
        site = key_of(record.get("site_key"))
        if site is None:
            bad.append((where, None, "record has no usable site_key"))
        elif site[0] not in SITE_TAGS:
            bad.append((where, None, f"site_key tag {site[0]!r} is not a declared space"))
        for index, candidate in enumerate(record.get("candidates") or []):
            if not candidate.get("snapshot_id"):
                bad.append((where, index, "candidate has no snapshot_id"))
            path = key_of(candidate.get("path_key"))
            if path is None:
                bad.append((where, index, "candidate has no usable path_key"))
            elif path[0] not in PATH_TAGS:
                bad.append((where, index, f"path_key tag {path[0]!r} is not a declared path space"))
    for snapshot in snapshots.values():
        if "schema_version" not in snapshot:
            bad.append(((snapshot.get("snapshot_id"),), None, "snapshot has no schema_version"))
    return bad


def check_snapshot(annotations, snapshots):
    """Every candidate's value is in the snapshot its own id names."""
    bad = []
    for record in annotations:
        where = (record.get("function_id"), record.get("output_line"), record.get("output_col"))
        register = record.get("register")
        site = key_of(record.get("site_key"))
        for index, candidate in enumerate(record.get("candidates") or []):
            snapshot = snapshots.get(
                (record.get("function_id"), candidate.get("snapshot_id"))
            )
            if snapshot is None:
                bad.append((where, index, f"no snapshot {candidate.get('snapshot_id')!r}"))
                continue
            path = key_of(candidate.get("path_key"))
            recorded = key_of(snapshot.get("site_key"))
            # One rule for all three sites: a snapshot names the incoming path it
            # is the end state of. A call captures at the call, so its path is the
            # call itself; a join or a loop header captures at a predecessor's
            # end, so its path is that block. Keying a snapshot by the site
            # instead makes this pairing agree with itself for a value borrowed
            # from any sibling path, which is what it did for 6,206 join
            # candidates.
            if recorded != path:
                bad.append(
                    (where, index, f"snapshot is for {recorded}, candidate claims path {path}")
                )
                continue
            if site is not None and site[0] == "call" and path != site:
                bad.append(
                    (where, index, f"call candidate path {path} is not its own site {site}")
                )
                continue
            registers = {tuple(pair) for pair in snapshot.get("registers") or []}
            if (register, candidate.get("value")) not in registers:
                bad.append(
                    (
                        where,
                        index,
                        f"{register} did not hold {candidate.get('value')!r} in {snapshot.get('snapshot_id')}",
                    )
                )
    return bad


def check_unique(annotations):
    """One record per annotation coordinate."""
    seen = Counter(
        (record.get("function_id"), record.get("output_line"), record.get("output_col"))
        for record in annotations
    )
    return [(where, None, f"{count} records claim one coordinate") for where, count in seen.items() if count > 1]


def load_ir(ir_dir: pathlib.Path):
    """function_id to (call addresses, per-address clobbered registers)."""
    volatile = [f"x{index}" for index in list(range(15)) + [16, 17, 18, 30]]
    calls = {}
    for path in sorted(ir_dir.glob("*.json")):
        try:
            body = json.loads(path.read_text())
        except json.JSONDecodeError:
            continue
        addresses = set()
        for block in body.get("blocks") or []:
            for instruction in block.get("instrs") or []:
                if instruction.get("op") == "Call":
                    addresses.add(instruction.get("va"))
        calls[body.get("function_id")] = (addresses, set(volatile))
    return calls


def check_ir(annotations, ir_dir: pathlib.Path):
    """Keys resolve in the independently emitted IR, and the loss is real."""
    calls = load_ir(ir_dir)
    bad = []
    for record in annotations:
        where = (record.get("function_id"), record.get("output_line"), record.get("output_col"))
        site = key_of(record.get("site_key"))
        if site is None or site[0] != "call":
            continue
        known = calls.get(record.get("function_id"))
        if known is None:
            bad.append((where, None, "no emitted IR for this function"))
            continue
        addresses, clobbered = known
        if site[1] not in addresses:
            bad.append((where, None, f"0x{site[1]:x} is not a Call instruction in this function"))
        # The loss itself: an ordinary call drops exactly the ABI volatile set,
        # so a preserved register annotated here would be a fabricated loss.
        if record.get("register") not in clobbered:
            bad.append((where, None, f"{record.get('register')} is not dropped by a call"))
        for index, candidate in enumerate(record.get("candidates") or []):
            path = key_of(candidate.get("path_key"))
            if path is None or path[0] != "call":
                bad.append((where, index, f"path_key {path} is not a call"))
            elif path[1] not in addresses:
                bad.append((where, index, f"path 0x{path[1]:x} is not a Call in this function"))
    return bad


def spans_in(line: str, opener: str = CALL_OPEN):
    """Every annotation span of one literal on a line, as (column, values)."""
    found, index = [], 0
    while True:
        at = line.find(opener, index)
        if at < 0:
            return found
        end = line.find(ANNOTATION_CLOSE, at + len(opener))
        if end < 0:
            return found
        body = line[at + len(opener) : end]
        found.append((at + 1, body.split(CANDIDATE_SEPARATOR)))
        index = end + len(ANNOTATION_CLOSE)


def rendered_values(record):
    """The value list the annotation should carry: first occurrence over the
    recorded candidate order, which is the emitter's own dedup rule."""
    seen, values = set(), []
    for candidate in record.get("candidates") or []:
        if candidate.get("value") not in seen:
            seen.add(candidate.get("value"))
            values.append(candidate.get("value"))
    return values


def unrecovered_spellings(register: str):
    """Every spelling that denotes an unrecovered value of `register`. Mirrors
    `unrecovered_value_spellings`: the canonical name, the reader alias, and the
    indirect-target alias."""
    if not register.startswith("x") or not register[1:].isdigit():
        return set()
    index = int(register[1:])
    alias = {30: "returnAddress", 29: "framePointer"}.get(index, f"reg{index}")
    indirect = {30: "dispatchTarget", 2: "cachedTarget"}.get(index, f"indirectTarget{index}")
    return {register, alias, indirect}


def loop_graphs(ir_dir: pathlib.Path, wanted):
    """Per function id: successors, predecessors, and per-block reachability.

    Read from the independently emitted IR, so a site key is resolved against the
    control flow rather than against the emitter that produced the record. Only
    functions the audit names are loaded.
    """
    graphs = {}
    for path in sorted(ir_dir.glob("*.json")):
        stem = path.name.split("_", 1)[0]
        try:
            if int(stem) not in wanted:
                continue
        except ValueError:
            continue
        try:
            body = json.loads(path.read_text())
        except json.JSONDecodeError:
            continue
        blocks = body.get("blocks") or []
        count = len(blocks)
        succs = {block.get("id"): [s for s in (block.get("succs") or []) if s < count] for block in blocks}
        preds = defaultdict(list)
        for block, targets in succs.items():
            for target in targets:
                preds[target].append(block)
        writes = {}
        for block in blocks:
            written = set()
            for instruction in block.get("instrs") or []:
                if instruction.get("op") == "Call":
                    written.update(VOLATILE)
                    continue
                src = instruction.get("src") or ""
                # A pre- or post-indexed access writes its base register back, so
                # even a store has a destination: `str x1, [x8, #-8]!` writes x8.
                # Mirrors `writeback_base`; without it a real drop reads as
                # fabricated, which it did for 5 annotations once.
                writeback = WRITEBACK_PRE.search(src) or WRITEBACK_POST.search(src)
                if writeback:
                    written.add("x" + writeback.group(1))
                parts = src.replace(",", " ").split()
                if not parts or parts[0].lower() in WRITES_NOTHING:
                    continue
                # Destination first, plus the second register of a pair load. Any
                # unrecognised mnemonic is treated as writing its first operand,
                # which keeps this a superset of what the lifter drops: a narrower
                # model would report a loss as fabricated when it was real.
                operands = [part for part in parts[1:] if part.lstrip("wx")[:1].isdigit()]
                taken = 2 if parts[0].lower().startswith("ldp") else 1
                for operand in operands[:taken]:
                    written.add("x" + operand.lstrip("wx"))
            writes[block.get("id")] = written
        graphs[body.get("function_id")] = (succs, dict(preds), writes)
    return graphs


def reachable_from(succs, start):
    seen, stack = set(), [start]
    while stack:
        block = stack.pop()
        for target in succs.get(block, ()):
            if target not in seen:
                seen.add(target)
                stack.append(target)
    return seen


def natural_loop_headers(succs):
    """Every natural loop header: the target of an edge whose source it dominates.

    Dominance rather than "sits on a cycle", because that is what `Regions`
    computes and therefore what the site classification means. The weaker test
    accepts a block that is merely part of a cycle, which over-approximates by
    360 and 544 blocks on the two samples - enough to make the check toothless.
    """
    nodes = sorted(succs)
    preds = {block: [] for block in nodes}
    for block, targets in succs.items():
        for target in targets:
            preds.setdefault(target, []).append(block)
    live = reachable_from(succs, 0) | {0}
    dom = {block: ({0} if block == 0 else set(live)) for block in live}
    changed = True
    while changed:
        changed = False
        for block in nodes:
            if block == 0 or block not in live:
                continue
            new = None
            for pred in preds.get(block, []):
                if pred not in live:
                    continue
                new = set(dom[pred]) if new is None else (new & dom[pred])
            new = new or set()
            new.add(block)
            if new != dom[block]:
                dom[block] = new
                changed = True
    headers = set()
    for block in live:
        for target in succs.get(block, ()):
            if target in live and target in dom[block]:
                headers.add(target)
    return headers


def check_loop_ir(annotations, ir_dir: pathlib.Path):
    """A loop-entry key resolves, in the emitted IR, to a loop header; every
    path_key is one of that header's non-back-edge predecessors; and the register
    really is written inside the loop, so the merge really did drop it.

    Reachability is enough for both classifications and needs no dominator
    computation: the structured emitter only runs on a reducible CFG, where a
    retreating edge's target dominates its source. So a predecessor reachable from
    the header is a back edge, and one that is not is an entry path.
    """
    wanted = {record.get("function_id") for record in annotations
              if (key_of(record.get("site_key")) or ("",))[0] == "loop"}
    graphs = loop_graphs(ir_dir, wanted)
    bad = []
    for record in annotations:
        where = (record.get("function_id"), record.get("output_line"), record.get("output_col"))
        site = key_of(record.get("site_key"))
        if site is None or site[0] != "loop":
            continue
        graph = graphs.get(record.get("function_id"))
        if graph is None:
            bad.append((where, None, "no emitted IR for this function"))
            continue
        succs, preds, writes = graph
        header = site[1]
        if header not in succs:
            bad.append((where, None, f"block {header} is not in this function"))
            continue
        body = reachable_from(succs, header)
        # An entry path is one control can be on before the loop: reachable from
        # the function entry without passing through the header. This is the
        # property that makes a value an *entry* value, and it is not the same as
        # "not reachable from the header" - a block inside the cycle can also be
        # reached from outside it, and the value it held on that first arrival is a
        # true entry value. Testing reachability from the header instead would
        # reject those, and testing nothing would accept a pure back edge.
        before_loop = reachable_from(
            {block: [s for s in targets if s != header] for block, targets in succs.items()},
            0,
        ) | {0}
        incoming = preds.get(header, [])
        if header not in natural_loop_headers(succs):
            bad.append((where, None, f"block {header} is not a natural loop header"))
        # The loss: the register has to be written somewhere the header's merge
        # covers, or the annotation reports a drop that never happened.
        register = record.get("register")
        if not any(register in writes.get(block, ()) for block in body | {header}):
            bad.append((where, None, f"{register} is never written under header {header}"))
        for index, candidate in enumerate(record.get("candidates") or []):
            path = key_of(candidate.get("path_key"))
            if path is None or path[0] != "block":
                bad.append((where, index, f"path_key {path} is not a block"))
            elif path[1] not in incoming:
                bad.append((where, index, f"block {path[1]} is not a predecessor of {header}"))
            elif path[1] not in before_loop:
                bad.append(
                    (where, index, f"block {path[1]} is only reachable through header {header}, "
                                   "so its value is a back-edge value")
                )
    return bad


def check_loop_anchor(annotations, pseudocode_dir: pathlib.Path):
    """The annotation at the recorded coordinate is the recorded one, it uses the
    loop-entry literal, and the register it sits beside is still spelled as an
    unrecovered value - which is the emitted proof that the binding was lost.
    """
    by_id = {}
    for path in sorted(pseudocode_dir.glob("*.dartpseudo")):
        stem = path.name.split("_", 1)[0]
        try:
            by_id[int(stem)] = path
        except ValueError:
            continue
    bad, cache = [], {}
    for record in annotations:
        where = (record.get("function_id"), record.get("output_line"), record.get("output_col"))
        site = key_of(record.get("site_key"))
        if site is None or site[0] != "loop":
            continue
        path = by_id.get(record.get("function_id"))
        if path is None:
            bad.append((where, None, "no emitted pseudocode for this function"))
            continue
        if path not in cache:
            cache[path] = path.read_text().split("\n")
        lines = cache[path]
        number = record.get("output_line") or 0
        if not 1 <= number <= len(lines):
            bad.append((where, None, "output_line is outside the emitted file"))
            continue
        line = lines[number - 1]
        column = record.get("output_col")
        at = {found: values for found, values in spans_in(line, LOOP_OPEN)}
        values = at.get(column)
        if values is None:
            bad.append((where, None, "no loop-entry annotation at this coordinate"))
            continue
        if values != rendered_values(record):
            bad.append((where, None, f"annotation carries {values}, record claims {rendered_values(record)}"))
        head = line[: (column or 1) - 1]
        token = head[len(head.rstrip("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_")):]
        if token not in unrecovered_spellings(record.get("register") or ""):
            bad.append((where, None, f"the annotated token {token!r} is not an unrecovered {record.get('register')}"))
    return bad


def check_anchor(annotations, pseudocode_dir: pathlib.Path):
    """The annotation at the recorded coordinate is the recorded one.

    Derived from the emitted file rather than from the emitter, so a record that
    names a real site other than its own fails here.
    """
    by_id = {}
    for path in sorted(pseudocode_dir.glob("*.dartpseudo")):
        stem = path.name.split("_", 1)[0]
        try:
            by_id[int(stem)] = path
        except ValueError:
            continue
    bad, cache = [], {}
    for record in annotations:
        where = (record.get("function_id"), record.get("output_line"), record.get("output_col"))
        site = key_of(record.get("site_key"))
        if site is None or site[0] != "call":
            continue
        path = by_id.get(record.get("function_id"))
        if path is None:
            bad.append((where, None, "no emitted pseudocode for this function"))
            continue
        if path not in cache:
            cache[path] = path.read_text().split("\n")
        lines = cache[path]
        number = record.get("output_line") or 0
        if not 1 <= number <= len(lines):
            bad.append((where, None, "output_line is outside the emitted file"))
            continue
        line = lines[number - 1]
        at = {column: values for column, values in spans_in(line)}
        values = at.get(record.get("output_col"))
        if values is None:
            bad.append((where, None, "no call-loss annotation at this coordinate"))
            continue
        rendered, seen = [], set()
        for candidate in record.get("candidates") or []:
            if candidate.get("value") not in seen:
                seen.add(candidate.get("value"))
                rendered.append(candidate.get("value"))
        if values != rendered:
            bad.append((where, None, f"annotation carries {values}, record claims {rendered}"))
    return bad


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("audit", type=pathlib.Path)
    parser.add_argument("--ir-dir", type=pathlib.Path)
    parser.add_argument("--pseudocode-dir", type=pathlib.Path)
    parser.add_argument("--show", type=int, default=10, help="violations to print per check")
    parser.add_argument(
        "--loss-site",
        action="append",
        help="keep only annotation records with this loss_site, so one site's "
        "violation count is not carried by another's. Repeatable.",
    )
    args = parser.parse_args()

    snapshots, annotations, malformed = load(args.audit)
    if args.loss_site:
        annotations = [
            record for record in annotations if record.get("loss_site") in set(args.loss_site)
        ]
    checks = {
        "schema": check_schema(annotations, snapshots),
        "snapshot": check_snapshot(annotations, snapshots),
        "unique": check_unique(annotations),
    }
    if args.ir_dir:
        checks["ir"] = check_ir(annotations, args.ir_dir)
        checks["loop_ir"] = check_loop_ir(annotations, args.ir_dir)
    if args.pseudocode_dir:
        checks["anchor"] = check_anchor(annotations, args.pseudocode_dir)
        checks["loop_anchor"] = check_loop_anchor(annotations, args.pseudocode_dir)

    candidates = sum(len(record.get("candidates") or []) for record in annotations)
    by_site = defaultdict(int)
    for record in annotations:
        site = key_of(record.get("site_key"))
        by_site[site[0] if site else "?"] += 1

    print(f"audit           {args.audit}")
    print(f"loss sites      {args.loss_site or 'all'}")
    print(f"snapshots       {len(snapshots)}")
    print(f"annotations     {len(annotations)}  ({dict(by_site)})")
    print(f"candidates      {candidates}")
    print(f"malformed lines {len(malformed)}")
    for name, bad in checks.items():
        print(f"violations {name:9} {len(bad)}")
        for entry in bad[: args.show]:
            print(f"    {entry}")
    total = sum(len(bad) for bad in checks.values()) + len(malformed)
    print(f"violations total     {total}")
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main())
