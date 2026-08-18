#!/usr/bin/env python3
"""Scan an emitted pseudocode corpus for loop-entry annotations and check that
every one of them matches the shared literal exactly.

This is the direction the provenance checker cannot cover. That one starts from
the audit and asks whether each record's annotation is there; this one starts
from the corpus and asks whether every annotation in it is well formed and
accounted for. A misspelt span with no record would pass the first and fail here.

Checks:

  grammar    every occurrence of the opener is terminated by ` */` on the same
             line, carries at least one non-empty value, and opens no nested
             comment - so the strip parser removes exactly the span the emitter
             wrote and the corpus stays strippable
  near_miss  the words `loop-entry` never appear except as part of the literal,
             which is what catches a hand-rolled second spelling
  paired     with --audit: the corpus and the audit agree on the count, one
             record per emitted annotation

It also prints the coverage figures the round's ledger needs: annotations, their
arity distribution, bytes added, and the longest annotated line.

Line counts are per file via `splitlines()`, never over a concatenation: most
emitted files end without a trailing newline, so `cat corpus | wc -l` splices
line boundaries and undercounts.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys
from collections import Counter

# Mirrors `LOOP_ENTRY_ANNOTATION` in
# crates/flutterdec-decompiler/src/helpers/annotation.rs, delimiters included. A
# unit test asserts these bytes and the constant are the same, so a reworded
# literal fails loudly rather than making this scan report zero.
LOOP_OPEN = " /* loop-entry value: "
ANNOTATION_CLOSE = " */"
CANDIDATE_SEPARATOR = " | "
LABEL = "loop-entry"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pseudocode_dir", type=pathlib.Path)
    parser.add_argument("--audit", type=pathlib.Path)
    parser.add_argument("--show", type=int, default=10)
    args = parser.parse_args()

    files = sorted(args.pseudocode_dir.glob("*.dartpseudo"))
    grammar, near_miss = [], []
    spans = 0
    arity = Counter()
    added = 0
    longest_line = 0
    lines_total = 0
    for path in files:
        text = path.read_text()
        for number, line in enumerate(text.splitlines(), 1):
            lines_total += 1
            longest_line = max(longest_line, len(line))
            index = 0
            while True:
                at = line.find(LOOP_OPEN, index)
                if at < 0:
                    break
                end = line.find(ANNOTATION_CLOSE, at + len(LOOP_OPEN))
                if end < 0:
                    grammar.append((path.name, number, "span is never terminated"))
                    break
                body = line[at + len(LOOP_OPEN) : end]
                values = body.split(CANDIDATE_SEPARATOR)
                if not body or any(not value for value in values):
                    grammar.append((path.name, number, f"empty value in {body!r}"))
                if "/*" in body:
                    grammar.append((path.name, number, f"nested comment in {body!r}"))
                spans += 1
                arity[len(values)] += 1
                added += (end + len(ANNOTATION_CLOSE)) - at
                index = end + len(ANNOTATION_CLOSE)
            # Every mention of the label has to be part of the literal: a second
            # spelling would otherwise sit in the corpus unnoticed by every check
            # that searches for the literal itself.
            if line.count(LABEL) != line.count(LOOP_OPEN):
                near_miss.append((path.name, number, line.strip()[:120]))

    paired = []
    records = None
    if args.audit:
        records = 0
        with args.audit.open() as handle:
            for row in handle:
                if not row.strip():
                    continue
                body = json.loads(row)
                if body.get("record") == "annotation" and body.get("loss_site") == "loop_entry":
                    records += 1
        if records != spans:
            paired.append(("corpus", spans, f"audit has {records} loop-entry records"))

    print(f"pseudocode      {args.pseudocode_dir}")
    print(f"files           {len(files)}")
    print(f"emitted lines   {lines_total}")
    print(f"annotations     {spans}")
    print(f"arity           {dict(sorted(arity.items()))}")
    print(f"bytes added     {added}")
    print(f"longest line    {longest_line}")
    if records is not None:
        print(f"audit records   {records}")
    for name, bad in (("grammar", grammar), ("near_miss", near_miss), ("paired", paired)):
        print(f"violations {name:9} {len(bad)}")
        for entry in bad[: args.show]:
            print(f"    {entry}")
    total = len(grammar) + len(near_miss) + len(paired)
    print(f"violations total     {total}")
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main())
