#!/usr/bin/env python3
"""Reconcile the three per-site provenance audits against the emitted corpus.

Each loss site has its own audit assertion, and each declares records outside its
own key space out of scope. That leaves a hole no per-site checker can see: an
annotation emitted from a fourth path, or a record bearing a site tag nobody
claims, is excused by all three and verified by none while every per-site count
reads zero. This script closes it by reading the *population* rather than one
site's rows.

Coordinates and control-flow claims are not derived from the emitter's
bookkeeping. Annotation spans come from this script's own scan of the emitted
`.dartpseudo` files, and predecessor identity comes from the IR emitted by the
same run with `--emit-ir`. Candidate values are checked against the full
register end-state snapshot for that IR predecessor, not against the candidate
array or its deduplicated output.

Eight counts, each of which must be 0:

  1 unmatched_annotation   an annotation span in the corpus with no audit record
                           at its `(function_id, output_line, output_col)`
  2 orphan_record          an audit record whose coordinate carries no annotation
  3 double_claimed         an annotation claimed by more than one record
  4 unclaimed_site_space   a record whose `site_key` carries none of the three
                           declared site tags, so no per-site assertion owns it
  5 rendered_disagrees     the values rendered at the coordinate are not the
                           record's `candidates[].value` deduplicated by first
                           occurrence over ascending predecessor id
  6 site_not_real          `site_key` or a `path_key` does not resolve, in the
                           emitted IR, to a real construct of the declared kind
  7 anchor_disagrees       `(loss_site, site_key, path_key, register)` is not what
                           the output-anchor mapping says produced this annotation
  8 predecessor_disagrees  a join or loop-entry candidate is not the value in
                           the named predecessor's emitted end-state snapshot

Six, seven and eight are different claims. Six proves the label names a *real* construct:
without it, an emitter writing `("join", 7)` for an annotation produced at block 42
is internally coherent at every step - the tag is declared, the coordinate matches,
the snapshot is its own - and the provenance is still false. Seven proves it names
*the* construct at that coordinate, which six cannot: block 7 can be a genuine
non-loop join with a genuine predecessor and a genuine drop of the same register.
Eight binds every claimed value to that exact real predecessor. Predecessor
coverage and the deduplicated rendered list cannot do that: both stay unchanged
when values are permuted among four real predecessors carrying `7, 7, 9, 9`.

The anchor mapping is the `anchor` field: the rendering anchor the emitter
inserted the span from, recorded in terms the IR resolves on its own - `["block",
id]` for a merge, `["call", va]` for a clobber - and never in terms of the label.
This script asks the IR what kind of construct that anchor is and requires the
label to be what the anchor earns, so a kind claim is checked against control flow
rather than against the claim. The register half is checked against the artifact:
the identifier the span was attached to must still be an unrecovered spelling of
the record's register.

What this does *not* establish, stated because a zero is worth only what it
covers: the join and loop sites place their span by searching forward for a line
carrying the register's spellings, so the anchor confirms the coordinate, the
predecessor-bound values and the loss, but not that the annotated line descends
from the anchor block. Count 7 binds the label to the anchor and the anchor to
the IR; it does not close that last step.

Usage:
    prov_cross_audit_reconcile.py AUDIT.jsonl --pseudocode DIR --ir DIR
    prov_cross_audit_reconcile.py --self-test
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys
import tempfile
from collections import defaultdict

# The four annotation literals, delimiters included. Mirrors
# `crates/flutterdec-decompiler/src/helpers/annotation.rs`, which is their one
# definition; a unit test scans every constant here against it.
JOIN_EXHAUSTIVE_OPEN = " /* = "
JOIN_PARTIAL_OPEN = " /* possible (non-exhaustive): "
LOOP_ENTRY_OPEN = " /* loop-entry value: "
PRE_CALL_OPEN = " /* value before this call: "
ANNOTATION_CLOSE = " */"
CANDIDATE_SEPARATOR = " | "

OPENERS = (
    JOIN_EXHAUSTIVE_OPEN,
    JOIN_PARTIAL_OPEN,
    LOOP_ENTRY_OPEN,
    PRE_CALL_OPEN,
)

# The three declared site spaces, and the site each literal belongs to. A loop
# header is also a join by predecessor count, so the tag - not the block id - is
# what keeps the spaces disjoint.
SITE_TAGS = ("join", "loop", "call")
LOSS_SITE_OF_TAG = {"join": "join", "loop": "loop_entry", "call": "call"}
PATH_TAGS = ("block", "call")

IDENT = set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_")


def key_of(value):
    """A tagged key as a comparable tuple, or None when it is not one."""
    if isinstance(value, list) and len(value) == 2 and isinstance(value[0], str):
        return (value[0], value[1])
    return None


# --------------------------------------------------------------------------
# The corpus, read without the emitter's help
# --------------------------------------------------------------------------


def spans_of_line(line: bytes):
    """Every annotation span on one line, as (column, opener, values).

    Byte columns, 1-based, counted as `find_span` counts them. All four literals
    are scanned in one left-to-right walk and the walk resumes past each span, so
    a value that happens to spell an opener cannot start a second one.
    """
    found = []
    index = 0
    openers = [opener.encode() for opener in OPENERS]
    close = ANNOTATION_CLOSE.encode()
    while index < len(line):
        hit = None
        for opener, text in zip(openers, OPENERS):
            at = line.find(opener, index)
            if at >= 0 and (hit is None or at < hit[0]):
                hit = (at, opener, text)
        if hit is None:
            return found
        at, opener, text = hit
        end = line.find(close, at + len(opener))
        if end < 0:
            return found
        body = line[at + len(opener) : end].decode("utf-8", "replace")
        found.append((at + 1, text, body.split(CANDIDATE_SEPARATOR)))
        index = end + len(close)
    return found


def annotated_token(line: bytes, column: int) -> str:
    """The identifier the span at `column` was attached to."""
    head = line[: column - 1].decode("utf-8", "replace")
    return head[len(head.rstrip("".join(IDENT))) :]


def parse_corpus(pseudocode_dir: pathlib.Path):
    """Every annotation in the emitted corpus, keyed by coordinate."""
    found = {}
    for path in sorted(pseudocode_dir.glob("*.dartpseudo")):
        try:
            function_id = int(path.name.split("_", 1)[0])
        except ValueError:
            continue
        for number, line in enumerate(path.read_bytes().split(b"\n"), start=1):
            for column, opener, values in spans_of_line(line):
                found[(function_id, number, column)] = {
                    "opener": opener,
                    "values": values,
                    "token": annotated_token(line, column),
                }
    return found


# --------------------------------------------------------------------------
# The IR, read from the run under test
# --------------------------------------------------------------------------


def reachable_from(succs, start):
    seen, stack = {start}, [start]
    while stack:
        for target in succs.get(stack.pop(), ()):
            if target not in seen:
                seen.add(target)
                stack.append(target)
    return seen


def natural_loop_headers(succs, reachable):
    """Every natural loop header: the target of an edge whose source it dominates.

    Dominance rather than "sits on a cycle", which is what `Regions` computes and
    therefore what a loop site key means. The weaker test over-approximates by
    hundreds of blocks per sample.
    """
    preds = defaultdict(list)
    for block, targets in succs.items():
        for target in targets:
            preds[target].append(block)
    dom = {block: ({0} if block == 0 else set(reachable)) for block in reachable}
    changed = True
    while changed:
        changed = False
        for block in sorted(reachable):
            if block == 0:
                continue
            new = None
            for pred in preds.get(block, []):
                if pred not in reachable:
                    continue
                new = set(dom[pred]) if new is None else (new & dom[pred])
            new = new or set()
            new.add(block)
            if new != dom[block]:
                dom[block] = new
                changed = True
    headers = set()
    for block in reachable:
        for target in succs.get(block, ()):
            if target in reachable and target in dom[block]:
                headers.add(target)
    return headers


class Ir:
    """The control-flow facts a provenance key can be resolved against.

    Predecessors are the *reachable* ones, because that is the set `Regions`
    counts when it calls a block a join; taking every edge instead would call a
    block with one live predecessor and one dead one a join.
    """

    def __init__(self, body):
        blocks = body.get("blocks") or []
        ids = {block.get("id") for block in blocks}
        self.succs = {
            block.get("id"): [s for s in (block.get("succs") or []) if s in ids]
            for block in blocks
        }
        self.reachable = reachable_from(self.succs, 0) if 0 in ids else set()
        self.preds = defaultdict(list)
        for block in sorted(self.reachable):
            for target in self.succs.get(block, ()):
                self.preds[target].append(block)
        self.headers = natural_loop_headers(self.succs, self.reachable)
        self.calls = {
            instruction.get("va")
            for block in blocks
            for instruction in (block.get("instrs") or [])
            if instruction.get("op") == "Call"
        }

    def is_join(self, block):
        return len(self.preds.get(block, [])) > 1

    def is_loop_header(self, block):
        return block in self.headers

    def is_call(self, address):
        return address in self.calls

    def kind_of_block(self, block):
        """The site kind this block earns in the IR, or None when it is neither.

        Loop-header semantics win at a block that is both, which is the precedence
        the per-site assertions declare and the reason the two key spaces do not
        collide at a header reached from two arms.
        """
        if block not in self.reachable:
            return None
        if self.is_loop_header(block):
            return "loop"
        if self.is_join(block):
            return "join"
        return None


def load_ir(ir_dir: pathlib.Path):
    """function_id to file path, so only the functions the audit names are read."""
    index = {}
    for path in sorted(ir_dir.glob("*.json")):
        try:
            index[int(path.name.split("_", 1)[0])] = path
        except ValueError:
            continue
    return index


class IrIndex:
    def __init__(self, ir_dir: pathlib.Path):
        self.paths = load_ir(ir_dir)
        self.cache = {}

    def get(self, function_id):
        if function_id not in self.cache:
            path = self.paths.get(function_id)
            if path is None:
                self.cache[function_id] = None
            else:
                body = json.loads(path.read_text())
                self.cache[function_id] = (
                    Ir(body) if body.get("function_id") == function_id else None
                )
        return self.cache[function_id]


# --------------------------------------------------------------------------
# The audit
# --------------------------------------------------------------------------


def load_audit(path: pathlib.Path):
    snapshots, annotations, malformed = {}, [], []
    with path.open() as handle:
        for number, line in enumerate(handle, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError as error:
                malformed.append((number, str(error)))
                continue
            if row.get("record") == "snapshot":
                snapshots[(row.get("function_id"), row.get("snapshot_id"))] = row
            elif row.get("record") == "annotation":
                annotations.append(row)
    return snapshots, annotations, malformed


def recorded_values(record):
    """The value list this record claims, deduplicated by first occurrence over
    ascending predecessor id.

    The order is stated rather than inherited: audit and emitter deduplicate
    independently, and two correct implementations with different orders would
    disagree here for no reason. A call record has one candidate and the sort is a
    no-op on it.
    """
    ordered = sorted(
        record.get("candidates") or [],
        key=lambda candidate: (key_of(candidate.get("path_key")) or ("", 0))[1],
    )
    values, seen = [], set()
    for candidate in ordered:
        value = candidate.get("value")
        if value not in seen:
            seen.add(value)
            values.append(value)
    return values


def unrecovered_spellings(register: str):
    """Every spelling denoting an unrecovered value of `register`. Mirrors
    `unrecovered_value_spellings`: canonical name, reader alias, indirect-target
    alias."""
    if not register.startswith("x") or not register[1:].isdigit():
        return set()
    index = int(register[1:])
    alias = {30: "returnAddress", 29: "framePointer"}.get(index, f"reg{index}")
    indirect = {30: "dispatchTarget", 2: "cachedTarget"}.get(
        index, f"indirectTarget{index}"
    )
    return {register, alias, indirect}


# --------------------------------------------------------------------------
# The seven counts
# --------------------------------------------------------------------------


def resolve_key(ir: Ir, site):
    """Whether a site key names a real construct of the kind its tag declares."""
    tag, value = site
    if tag == "join":
        if value not in ir.reachable:
            return f"block {value} is not a reachable block"
        if not ir.is_join(value):
            return f"block {value} has {len(ir.preds.get(value, []))} predecessors"
        if ir.is_loop_header(value):
            return f"block {value} is a loop header, so its merge is a loop site"
        return None
    if tag == "loop":
        if value not in ir.reachable:
            return f"block {value} is not a reachable block"
        if not ir.is_loop_header(value):
            return f"block {value} is not a natural loop header"
        return None
    if tag == "call":
        if not ir.is_call(value):
            return f"0x{value:x} is not the address of a call instruction"
        return None
    return f"{tag!r} is not a declared site space"


def resolve_path(ir: Ir, site, path):
    """Whether a path key is an incoming path of the site it is recorded under."""
    tag, value = path
    if tag not in PATH_TAGS:
        return f"{tag!r} is not a declared path space"
    if site[0] == "call":
        if path != site:
            return f"path {path} is not the clobbering call {site}"
        return None
    if tag != "block":
        return f"path {path} is not a block, but its site is a merge"
    if value not in ir.preds.get(site[1], []):
        return f"block {value} is not a predecessor of block {site[1]}"
    return None


def reconcile(audit: pathlib.Path, pseudocode_dir: pathlib.Path, ir_dir: pathlib.Path):
    corpus = parse_corpus(pseudocode_dir)
    snapshots, annotations, malformed = load_audit(audit)
    irs = IrIndex(ir_dir)

    by_coordinate = defaultdict(list)
    for record in annotations:
        by_coordinate[
            (
                record.get("function_id"),
                record.get("output_line"),
                record.get("output_col"),
            )
        ].append(record)

    counts = {name: [] for name in (
        "unmatched_annotation",
        "orphan_record",
        "double_claimed",
        "unclaimed_site_space",
        "rendered_disagrees",
        "site_not_real",
        "anchor_disagrees",
        "predecessor_disagrees",
    )}

    def note(name, where, reason):
        counts[name].append((where, reason))

    for where in sorted(corpus):
        matched = by_coordinate.get(where, [])
        if not matched:
            note("unmatched_annotation", where, f"corpus carries {corpus[where]['opener']!r}")
        elif len(matched) > 1:
            note("double_claimed", where, f"{len(matched)} records claim it")

    for record in annotations:
        where = (
            record.get("function_id"),
            record.get("output_line"),
            record.get("output_col"),
        )
        annotation = corpus.get(where)
        if annotation is None:
            note("orphan_record", where, f"loss_site {record.get('loss_site')!r}")

        site = key_of(record.get("site_key"))
        if site is None or site[0] not in SITE_TAGS:
            note("unclaimed_site_space", where, f"site_key {record.get('site_key')!r}")

        if annotation is not None:
            claimed = recorded_values(record)
            if annotation["values"] != claimed:
                note(
                    "rendered_disagrees",
                    where,
                    f"rendered {annotation['values']} but the record claims {claimed}",
                )

        ir = irs.get(record.get("function_id"))
        if ir is None:
            note("site_not_real", where, "no emitted IR for this function")
            note("anchor_disagrees", where, "no emitted IR for this function")
            continue

        if site is None:
            note("site_not_real", where, "record has no usable site_key")
        else:
            problem = resolve_key(ir, site)
            if problem:
                note("site_not_real", where, f"site_key {site}: {problem}")
            else:
                for index, candidate in enumerate(record.get("candidates") or []):
                    path = key_of(candidate.get("path_key"))
                    if path is None:
                        note("site_not_real", where, f"candidates[{index}] has no path_key")
                        continue
                    problem = resolve_path(ir, site, path)
                    if problem:
                        note("site_not_real", where, f"candidates[{index}] {path}: {problem}")
                    elif site[0] in ("join", "loop"):
                        snapshot = snapshots.get(
                            (record.get("function_id"), candidate.get("snapshot_id"))
                        )
                        snapshot_path = key_of(snapshot.get("site_key")) if snapshot else None
                        registers = dict(snapshot.get("registers") or []) if snapshot else {}
                        register = record.get("register")
                        if (
                            snapshot_path != path
                            or registers.get(register) != candidate.get("value")
                        ):
                            note(
                                "predecessor_disagrees",
                                where,
                                f"candidates[{index}] {path} claims {register}="
                                f"{candidate.get('value')!r}, but snapshot "
                                f"{candidate.get('snapshot_id')!r} records path "
                                f"{snapshot_path} and {register}={registers.get(register)!r}",
                            )

        # Count 7. The anchor is the construct the emitter inserted this span
        # from; the IR says what kind of construct that is, and the label has to
        # be what the anchor earns rather than what the emitter meant.
        anchor = key_of(record.get("anchor"))
        if anchor is None:
            note("anchor_disagrees", where, "record records no rendering anchor")
            continue
        if anchor[0] == "call":
            expected_site = ("call", anchor[1]) if ir.is_call(anchor[1]) else None
            expected_paths = {expected_site} if expected_site else set()
        elif anchor[0] == "block":
            kind = ir.kind_of_block(anchor[1])
            expected_site = (kind, anchor[1]) if kind else None
            expected_paths = {
                ("block", pred) for pred in ir.preds.get(anchor[1], [])
            }
        else:
            expected_site = None
            expected_paths = set()
        if expected_site is None:
            note(
                "anchor_disagrees",
                where,
                f"anchor {anchor} resolves to no construct that can lose a value",
            )
            continue
        if site != expected_site:
            note(
                "anchor_disagrees",
                where,
                f"anchor {anchor} is a {expected_site[0]} at {expected_site[1]}, "
                f"but the record claims site_key {site}",
            )
            continue
        if record.get("loss_site") != LOSS_SITE_OF_TAG[expected_site[0]]:
            note(
                "anchor_disagrees",
                where,
                f"anchor {anchor} is a {expected_site[0]}, but loss_site is "
                f"{record.get('loss_site')!r}",
            )
            continue
        stray = [
            key_of(candidate.get("path_key"))
            for candidate in record.get("candidates") or []
            if key_of(candidate.get("path_key")) not in expected_paths
        ]
        if stray:
            note(
                "anchor_disagrees",
                where,
                f"path keys {stray} are not incoming paths of anchor {anchor}",
            )
            continue
        # The register half, taken off the artifact rather than off the record:
        # the span is attached to a token, and that token has to be an
        # unrecovered spelling of the register whose loss is claimed.
        if annotation is not None:
            spellings = unrecovered_spellings(record.get("register") or "")
            if annotation["token"] not in spellings:
                note(
                    "anchor_disagrees",
                    where,
                    f"the annotated token {annotation['token']!r} is not an "
                    f"unrecovered {record.get('register')}",
                )

    return corpus, annotations, malformed, counts


def report(label, corpus, annotations, malformed, counts, show):
    by_site = defaultdict(int)
    for record in annotations:
        site = key_of(record.get("site_key"))
        by_site[site[0] if site else "?"] += 1
    by_literal = defaultdict(int)
    for annotation in corpus.values():
        by_literal[annotation["opener"]] += 1

    print(f"sample            {label}")
    print(f"corpus spans      {len(corpus)}  ({dict(by_literal)})")
    print(f"audit records     {len(annotations)}  ({dict(by_site)})")
    print(f"malformed lines   {len(malformed)}")
    for index, (name, bad) in enumerate(counts.items(), start=1):
        print(f"{index} {name:22} {len(bad)}")
        for entry in bad[:show]:
            print(f"      {entry}")
    total = sum(len(bad) for bad in counts.values()) + len(malformed)
    print(f"total             {total}")
    return total


# --------------------------------------------------------------------------
# Self-test: a planted violation for every count
# --------------------------------------------------------------------------

SELF_TEST_IR = {
    "function_id": 7,
    "name": "planted",
    "entry_va": 0x1000,
    "blocks": [
        {"id": 0, "start_va": 0x1000, "succs": [1, 2], "instrs": []},
        {
            "id": 1,
            "start_va": 0x1010,
            "succs": [3],
            "instrs": [{"va": 0x1010, "op": "Other", "src": "mov x0, #7", "target": ""}],
        },
        {
            "id": 2,
            "start_va": 0x1020,
            "succs": [3],
            "instrs": [{"va": 0x1020, "op": "Other", "src": "mov x0, #9", "target": ""}],
        },
        # Block 3 is the join: two predecessors, not a loop header.
        {
            "id": 3,
            "start_va": 0x1030,
            "succs": [4],
            "instrs": [
                {"va": 0x1030, "op": "Call", "src": "bl #0x2000", "target": ""},
                {"va": 0x1034, "op": "Other", "src": "mov x1, #3", "target": ""},
            ],
        },
        # Block 4 is a natural loop header: block 5 is dominated by it and
        # branches back.
        {"id": 4, "start_va": 0x1040, "succs": [5], "instrs": []},
        {"id": 5, "start_va": 0x1050, "succs": [4, 6], "instrs": []},
        {"id": 6, "start_va": 0x1060, "succs": [], "instrs": []},
    ],
}

SELF_TEST_LINES = [
    "void planted() {",
    "  return reg0" + JOIN_EXHAUSTIVE_OPEN + "7" + CANDIDATE_SEPARATOR + "9" + ANNOTATION_CLOSE + ";",
    "  var a = reg1" + LOOP_ENTRY_OPEN + "3" + ANNOTATION_CLOSE + ";",
    "  var b = reg2" + PRE_CALL_OPEN + "5" + ANNOTATION_CLOSE + ";",
    "}",
]

SELF_TEST_WIDE_IR = {
    "function_id": 8,
    "name": "wide",
    "entry_va": 0x2000,
    "blocks": [
        {"id": 0, "start_va": 0x2000, "succs": [1, 2], "instrs": []},
        {"id": 1, "start_va": 0x2010, "succs": [3, 4], "instrs": []},
        {"id": 2, "start_va": 0x2020, "succs": [5, 6], "instrs": []},
        {"id": 3, "start_va": 0x2030, "succs": [7], "instrs": []},
        {"id": 4, "start_va": 0x2040, "succs": [7], "instrs": []},
        {"id": 5, "start_va": 0x2050, "succs": [7], "instrs": []},
        {"id": 6, "start_va": 0x2060, "succs": [7], "instrs": []},
        {"id": 7, "start_va": 0x2070, "succs": [], "instrs": []},
    ],
}

SELF_TEST_LOOP_IR = {
    "function_id": 9,
    "name": "loop_paths",
    "entry_va": 0x3000,
    "blocks": [
        {"id": 0, "start_va": 0x3000, "succs": [1, 2], "instrs": []},
        {"id": 1, "start_va": 0x3010, "succs": [3], "instrs": []},
        {"id": 2, "start_va": 0x3020, "succs": [3], "instrs": []},
        {"id": 3, "start_va": 0x3030, "succs": [4, 5], "instrs": []},
        {"id": 4, "start_va": 0x3040, "succs": [3], "instrs": []},
        {"id": 5, "start_va": 0x3050, "succs": [], "instrs": []},
    ],
}


def self_test_records():
    return [
        {
            "schema_version": 1,
            "record": "annotation",
            "function_id": 7,
            "output_line": 2,
            "output_col": len("  return reg0") + 1,
            "loss_site": "join",
            "site_key": ["join", 3],
            "anchor": ["block", 3],
            "register": "x0",
            "candidates": [
                {"path_key": ["block", 1], "value": "7", "snapshot_id": "join:3:pred:1:0"},
                {"path_key": ["block", 2], "value": "9", "snapshot_id": "join:3:pred:2:0"},
            ],
        },
        {
            "schema_version": 1,
            "record": "annotation",
            "function_id": 7,
            "output_line": 3,
            "output_col": len("  var a = reg1") + 1,
            "loss_site": "loop_entry",
            "site_key": ["loop", 4],
            "anchor": ["block", 4],
            "register": "x1",
            "candidates": [
                {"path_key": ["block", 3], "value": "3", "snapshot_id": "loop:4:pred:3:0"},
            ],
        },
        {
            "schema_version": 1,
            "record": "annotation",
            "function_id": 7,
            "output_line": 4,
            "output_col": len("  var b = reg2") + 1,
            "loss_site": "call",
            "site_key": ["call", 0x1030],
            "anchor": ["call", 0x1030],
            "register": "x2",
            "candidates": [
                {"path_key": ["call", 0x1030], "value": "5", "snapshot_id": "call:0x1030:0"},
            ],
        },
        {
            "schema_version": 1,
            "record": "annotation",
            "function_id": 8,
            "output_line": 2,
            "output_col": len("  return reg3") + 1,
            "loss_site": "join",
            "site_key": ["join", 7],
            "anchor": ["block", 7],
            "register": "x3",
            "candidates": [
                {"path_key": ["block", 3], "value": "7", "snapshot_id": "join:7:pred:3:0"},
                {"path_key": ["block", 4], "value": "7", "snapshot_id": "join:7:pred:4:1"},
                {"path_key": ["block", 5], "value": "9", "snapshot_id": "join:7:pred:5:2"},
                {"path_key": ["block", 6], "value": "9", "snapshot_id": "join:7:pred:6:3"},
            ],
        },
        {
            "schema_version": 1,
            "record": "annotation",
            "function_id": 9,
            "output_line": 2,
            "output_col": len("  return reg4") + 1,
            "loss_site": "loop_entry",
            "site_key": ["loop", 3],
            "anchor": ["block", 3],
            "register": "x4",
            "candidates": [
                {"path_key": ["block", 1], "value": "21", "snapshot_id": "loop:3:pred:1:0"},
                {"path_key": ["block", 2], "value": "23", "snapshot_id": "loop:3:pred:2:1"},
            ],
        },
        {
            "schema_version": 1,
            "record": "snapshot",
            "function_id": 7,
            "snapshot_id": "join:3:pred:1:0",
            "site_key": ["block", 1],
            "registers": [["x0", "7"]],
        },
        {
            "schema_version": 1,
            "record": "snapshot",
            "function_id": 7,
            "snapshot_id": "join:3:pred:2:0",
            "site_key": ["block", 2],
            "registers": [["x0", "9"]],
        },
        {
            "schema_version": 1,
            "record": "snapshot",
            "function_id": 7,
            "snapshot_id": "loop:4:pred:3:0",
            "site_key": ["block", 3],
            "registers": [["x1", "3"]],
        },
        {
            "schema_version": 1,
            "record": "snapshot",
            "function_id": 7,
            "snapshot_id": "call:0x1030:0",
            "site_key": ["call", 0x1030],
            "registers": [["x2", "5"]],
        },
        *[
            {
                "schema_version": 1,
                "record": "snapshot",
                "function_id": 8,
                "snapshot_id": f"join:7:pred:{pred}:{pred - 3}",
                "site_key": ["block", pred],
                "registers": [["x3", value]],
            }
            for pred, value in ((3, "7"), (4, "7"), (5, "9"), (6, "9"))
        ],
        {
            "schema_version": 1,
            "record": "snapshot",
            "function_id": 9,
            "snapshot_id": "loop:3:pred:1:0",
            "site_key": ["block", 1],
            "registers": [["x4", "21"]],
        },
        {
            "schema_version": 1,
            "record": "snapshot",
            "function_id": 9,
            "snapshot_id": "loop:3:pred:2:1",
            "site_key": ["block", 2],
            "registers": [["x4", "23"]],
        },
    ]


def write_self_test(root: pathlib.Path, records, lines):
    (root / "pseudocode").mkdir(parents=True, exist_ok=True)
    (root / "ir").mkdir(parents=True, exist_ok=True)
    (root / "pseudocode" / "00007_planted.dartpseudo").write_text("\n".join(lines))
    (root / "ir" / "00007_planted.json").write_text(json.dumps(SELF_TEST_IR))
    (root / "pseudocode" / "00008_wide.dartpseudo").write_text(
        "void wide() {\n  return reg3 /* = 7 | 9 */;\n}"
    )
    (root / "ir" / "00008_wide.json").write_text(json.dumps(SELF_TEST_WIDE_IR))
    (root / "pseudocode" / "00009_loop_paths.dartpseudo").write_text(
        "void loopPaths() {\n  return reg4 /* loop-entry value: 21 | 23 */;\n}"
    )
    (root / "ir" / "00009_loop_paths.json").write_text(json.dumps(SELF_TEST_LOOP_IR))
    audit = root / "audit.jsonl"
    audit.write_text("".join(json.dumps(record) + "\n" for record in records))
    return audit


def self_test():
    """Every count fires on its own planted defect, and none fires on the clean
    fixture. A checker nobody has seen fail is a checker nobody has tested."""
    import copy

    def run(mutate=None, mutate_lines=None):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            records = copy.deepcopy(self_test_records())
            lines = list(SELF_TEST_LINES)
            if mutate:
                mutate(records)
            if mutate_lines:
                lines = mutate_lines(lines)
            audit = write_self_test(root, records, lines)
            _, _, _, counts = reconcile(
                audit, root / "pseudocode", root / "ir"
            )
            return {name: len(bad) for name, bad in counts.items()}

    clean = run()
    assert sum(clean.values()) == 0, f"the clean fixture must reconcile: {clean}"

    plants = [
        # 1 an annotation nobody recorded.
        ("unmatched_annotation", lambda records: records.pop(0), None),
        # 2 a record at a coordinate carrying no annotation.
        ("orphan_record", lambda records: records[0].__setitem__("output_line", 5), None),
        # 3 two records claiming one annotation.
        ("double_claimed", lambda records: records.append(copy.deepcopy(records[0])), None),
        # 4 a site tag no per-site assertion owns.
        (
            "unclaimed_site_space",
            lambda records: records[0].__setitem__("site_key", ["phi", 3]),
            None,
        ),
        # 5 the record and the artifact disagree about what was rendered.
        (
            "rendered_disagrees",
            lambda records: records[0]["candidates"][1].__setitem__("value", "11"),
            None,
        ),
        # 6 the counterexample the contract names: a self-consistent label on a
        # block that is not a join at all.
        (
            "site_not_real",
            lambda records: records[0].__setitem__("site_key", ["join", 1]),
            None,
        ),
        # 6 a path key that is not an incoming path of its site.
        (
            "site_not_real",
            lambda records: records[0]["candidates"][0].__setitem__("path_key", ["block", 5]),
            None,
        ),
        # 7 a real join, a real predecessor, a real drop - and not the construct
        # that produced this annotation.
        (
            "anchor_disagrees",
            lambda records: records[0].__setitem__("anchor", ["block", 4]),
            None,
        ),
        # 7 the label disagrees with the kind its own anchor resolves to.
        (
            "anchor_disagrees",
            lambda records: records[1].__setitem__("site_key", ["join", 4]),
            None,
        ),
        # 7 the span is attached to a token that is not the register it claims.
        (
            "anchor_disagrees",
            None,
            lambda lines: [
                line.replace("reg0", "reg9") if index == 1 else line
                for index, line in enumerate(lines)
            ],
        ),
    ]
    for name, mutate, mutate_lines in plants:
        result = run(mutate, mutate_lines)
        assert result[name] > 0, f"planting for {name} produced {result}"

    def moved_real_predecessor(records):
        records[0]["candidates"][0]["path_key"] = ["block", 2]

    def permuted_four_predecessors(records):
        record = next(row for row in records if row.get("function_id") == 8)
        record["candidates"][1]["value"] = "9"
        record["candidates"][2]["value"] = "7"

    def retargeted_loop_entry(records):
        record = next(row for row in records if row.get("function_id") == 9)
        record["candidates"][0]["path_key"] = ["block", 2]

    binding_plants = (
        ("moved real predecessor", moved_real_predecessor),
        ("four-predecessor permutation", permuted_four_predecessors),
        ("loop-entry retarget", retargeted_loop_entry),
    )
    for name, mutate in binding_plants:
        result = run(mutate)
        legacy_total = sum(
            count for key, count in result.items() if key != "predecessor_disagrees"
        )
        assert legacy_total == 0, f"the legacy seven counts reject {name}: {result}"
        assert result["predecessor_disagrees"] > 0, (
            f"the predecessor binding accepts {name}: {result}"
        )

    print(
        f"self-test ok: clean fixture reconciles, {len(plants)} count plants and "
        f"{len(binding_plants)} legacy-accepted predecessor plants all detected"
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("audit", nargs="?", type=pathlib.Path)
    parser.add_argument("--pseudocode", type=pathlib.Path)
    parser.add_argument("--ir", type=pathlib.Path)
    parser.add_argument("--label", default="")
    parser.add_argument("--show", type=int, default=10)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        return self_test()
    if not (args.audit and args.pseudocode and args.ir):
        parser.error("AUDIT, --pseudocode and --ir are required")

    corpus, annotations, malformed, counts = reconcile(
        args.audit, args.pseudocode, args.ir
    )
    total = report(
        args.label or str(args.audit), corpus, annotations, malformed, counts, args.show
    )
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main())
