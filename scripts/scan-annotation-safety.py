#!/usr/bin/env python3
"""Full-corpus safety scan for value annotations.

Answers five questions about an emitted corpus, over every file, and reports the
counts it used so a zero cannot be a zero obtained by narrowing the scan:

  forbidden  - an annotation span whose body carries `{`, `}` or a second
               comment terminator. A brace steers the brace-sensitive
               compaction pass; an inner terminator ends the span early and
               leaves the rest of the annotation on the line as code.
  over_cap   - a physical line longer than the line budget, annotated or not.
  over_span  - an annotation span longer than the per-annotation budget. Both
               budgets drop the whole span, so a corpus that respects them can
               carry no span over either one; a span at exactly the budget is
               inside it.
  unclosed   - an annotation opener with no terminator after it on its line.
               This is what a truncated annotation looks like from the outside,
               and it is why a budget must drop the whole span.
  spans      - the inventory of annotation spans the corpus carries, which is
               what an omission row is reconciled against.

Line counting is per file: most emitted files have no trailing newline, so
concatenating the corpus splices line boundaries and undercounts.

The literals are read from the emitter's own source rather than spelled here, so
a reworded label makes the scan fail loudly instead of going quietly vacuous.
"""

import argparse
import json
import pathlib
import re
import sys
from collections import Counter

ANNOTATION_SOURCE = pathlib.Path(__file__).resolve().parent.parent / (
    "crates/flutterdec-decompiler/src/helpers/annotation.rs"
)


def literals():
    """Openers and terminator, from the single definition of each."""
    text = ANNOTATION_SOURCE.read_text()
    openers = re.findall(r'AnnotationLiteral\s*\{\s*\n?\s*open:\s*"((?:[^"\\]|\\.)*)"', text)
    openers = [o.encode().decode("unicode_escape") for o in openers]
    close = re.search(r'ANNOTATION_CLOSE:\s*&str\s*=\s*"((?:[^"\\]|\\.)*)"', text)
    if len(openers) != 4 or close is None:
        raise SystemExit(
            f"expected four openers and a terminator in {ANNOTATION_SOURCE}, "
            f"found {len(openers)} and {close is not None}"
        )
    return openers, close.group(1).encode().decode("unicode_escape")


def spans_on(line, openers, close):
    """Every annotation span on `line`, plus the openers left unterminated."""
    found, unclosed = [], []
    index = 0
    while index < len(line):
        opener = next((o for o in openers if line.startswith(o, index)), None)
        if opener is None:
            index += 1
            continue
        end = line.find(close, index + len(opener))
        if end < 0:
            unclosed.append(opener)
            break
        found.append(line[index : end + len(close)])
        index = end + len(close)
    return found, unclosed


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("corpus", help="directory of emitted pseudocode")
    parser.add_argument("--max-line", type=int, default=3000)
    parser.add_argument("--max-span", type=int, default=512)
    parser.add_argument("--out", help="write the JSON report here as well")
    args = parser.parse_args()

    openers, close = literals()
    forbidden_in_body = ("{", "}", close.strip())

    report = {
        "corpus": str(args.corpus),
        "max_line_budget": args.max_line,
        "max_span_budget": args.max_span,
        "openers": openers,
        "close": close,
        "files": 0,
        "lines": 0,
        "longest_line": 0,
        "longest_line_at": None,
        "annotations": 0,
        "longest_annotation": 0,
        "longest_annotation_at": None,
        "annotations_by_literal": {o: 0 for o in openers},
        "violations": {"forbidden": [], "over_cap": [], "over_span": [], "unclosed": []},
    }
    inventory = Counter()

    for path in sorted(pathlib.Path(args.corpus).rglob("*")):
        if not path.is_file():
            continue
        text = path.read_text(errors="surrogateescape")
        report["files"] += 1
        for number, line in enumerate(text.splitlines(), start=1):
            report["lines"] += 1
            if len(line) > report["longest_line"]:
                report["longest_line"] = len(line)
                report["longest_line_at"] = f"{path}:{number}"
            if len(line) > args.max_line:
                report["violations"]["over_cap"].append(
                    {"at": f"{path}:{number}", "length": len(line)}
                )
            if not any(o in line for o in openers):
                continue
            found, unclosed = spans_on(line, openers, close)
            for opener in unclosed:
                report["violations"]["unclosed"].append(
                    {"at": f"{path}:{number}", "opener": opener}
                )
            for span in found:
                report["annotations"] += 1
                inventory[span] += 1
                if len(span) > report["longest_annotation"]:
                    report["longest_annotation"] = len(span)
                    report["longest_annotation_at"] = f"{path}:{number}"
                if len(span) > args.max_span:
                    report["violations"]["over_span"].append(
                        {"at": f"{path}:{number}", "length": len(span)}
                    )
                for opener in openers:
                    if span.startswith(opener):
                        report["annotations_by_literal"][opener] += 1
                        body = span[len(opener) : -len(close)]
                        break
                else:
                    body = span
                hits = [seq for seq in forbidden_in_body if seq in body]
                if hits:
                    report["violations"]["forbidden"].append(
                        {"at": f"{path}:{number}", "sequences": hits, "span": span[:200]}
                    )

    report["distinct_annotations"] = len(inventory)
    report["violation_counts"] = {k: len(v) for k, v in report["violations"].items()}
    report["clean"] = all(v == 0 for v in report["violation_counts"].values())

    if args.out:
        out = pathlib.Path(args.out)
        out.write_text(json.dumps(report, indent=2))
        (out.parent / (out.stem + "-spans.txt")).write_text(
            "".join(f"{count}\t{span}\n" for span, count in sorted(inventory.items()))
        )
    json.dump({k: v for k, v in report.items() if k != "violations"}, sys.stdout, indent=2)
    print()
    return 0 if report["clean"] else 1


if __name__ == "__main__":
    sys.exit(main())
