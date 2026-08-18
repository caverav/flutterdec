#!/usr/bin/env python3
"""Cross-check join provenance rows against the emitted output and the IR.

Three checks, none of which the audit can satisfy by agreeing with itself:

  anchor    the record's ``(output_line, output_col)`` must land on an annotation
            opener in the artifact the corpus is measured from. Naming a real site
            is not naming *the* site.
  dedup     the values rendered at that coordinate must equal the record's
            ``candidates`` deduplicated by first occurrence over their recorded
            order. Audit and emitter deduplicate independently, so this is where a
            divergence shows up - two runs can be stably wrong together, so
            cross-run stability would not catch it.
  coverage  with ``--ir``, an exhaustive annotation's attributed predecessors must
            cover every reachable IR predecessor of the site block, and every
            ``path_key`` must be one. Reachability is recomputed from the IR's own
            successor edges.

Usage:
    prov_join_output_anchor_check.py AUDIT.jsonl --pseudocode DIR [--ir DIR]
"""

import argparse
import glob
import json
import os
import sys

EXHAUSTIVE_OPEN = " /* = "
NON_EXHAUSTIVE_OPEN = " /* possible (non-exhaustive): "
CLOSE = " */"
SEPARATOR = " | "


def annotations(path):
    for number, line in enumerate(open(path, encoding="utf-8"), start=1):
        line = line.strip()
        if not line:
            continue
        row = json.loads(line)
        if row.get("record") == "annotation" and row.get("loss_site") == "join":
            yield number, row


def source_lines(pseudocode, function_id, cache):
    if function_id in cache:
        return cache[function_id]
    matches = glob.glob(os.path.join(pseudocode, f"{function_id:05d}_*"))
    lines = open(matches[0], encoding="utf-8").read().split("\n") if matches else None
    cache[function_id] = lines
    return lines


def ir_of(ir_dir, function_id, cache):
    if function_id in cache:
        return cache[function_id]
    matches = glob.glob(os.path.join(ir_dir, f"{function_id:05d}_*.json"))
    ir = json.load(open(matches[0], encoding="utf-8")) if matches else None
    if ir is not None and ir.get("function_id") != function_id:
        ir = None
    cache[function_id] = ir
    return ir


def reachable_predecessors(ir, block):
    succs = {b["id"]: [s for s in b["succs"]] for b in ir["blocks"]}
    seen, stack = {0}, [0]
    while stack:
        for succ in succs.get(stack.pop(), []):
            if succ not in seen:
                seen.add(succ)
                stack.append(succ)
    return sorted(
        other for other in seen if block in succs.get(other, [])
    )


def dedup(values):
    out = []
    for value in values:
        if value not in out:
            out.append(value)
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("audit")
    parser.add_argument("--pseudocode", required=True)
    parser.add_argument("--ir")
    args = parser.parse_args()

    problems = []
    checked = exhaustive = missing_ir = 0
    source_cache, ir_cache = {}, {}

    for number, record in annotations(args.audit):
        checked += 1
        where = f"{args.audit}:{number}"
        function_id = record["function_id"]
        lines = source_lines(args.pseudocode, function_id, source_cache)
        if lines is None:
            problems.append(f"{where}: no emitted source for function {function_id}")
            continue
        index = record["output_line"] - 1
        if index < 0 or index >= len(lines):
            problems.append(f"{where}: output_line {record['output_line']} is outside the file")
            continue
        line = lines[index]
        column = record["output_col"] - 1
        rest = line[column:]
        opener = next(
            (open_ for open_ in (EXHAUSTIVE_OPEN, NON_EXHAUSTIVE_OPEN) if rest.startswith(open_)),
            None,
        )
        if opener is None:
            problems.append(
                f"{where}: no annotation at line {record['output_line']} col "
                f"{record['output_col']}: {rest[:40]!r}"
            )
            continue
        body = rest[len(opener):]
        end = body.find(CLOSE)
        if end < 0:
            problems.append(f"{where}: unterminated annotation span")
            continue
        rendered = body[:end].split(SEPARATOR)
        expected = dedup([candidate["value"] for candidate in record["candidates"]])
        if rendered != expected:
            problems.append(
                f"{where}: rendered {rendered!r} but candidates dedup to {expected!r}"
            )
            continue

        if opener == EXHAUSTIVE_OPEN:
            exhaustive += 1
        if not args.ir:
            continue
        ir = ir_of(args.ir, function_id, ir_cache)
        if ir is None:
            missing_ir += 1
            continue
        block = record["site_key"][1]
        preds = reachable_predecessors(ir, block)
        attributed = sorted({candidate["path_key"][1] for candidate in record["candidates"]})
        stray = [pred for pred in attributed if pred not in preds]
        if stray:
            problems.append(f"{where}: path_keys {stray} are not IR predecessors of block {block}")
            continue
        if opener == EXHAUSTIVE_OPEN and attributed != preds:
            problems.append(
                f"{where}: exhaustive annotation covers {attributed} of IR predecessors {preds}"
            )

    for problem in problems[:50]:
        print(problem)
    print(
        f"audit={args.audit} join_annotations={checked} exhaustive={exhaustive} "
        f"ir_unavailable={missing_ir} problems={len(problems)}"
    )
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
