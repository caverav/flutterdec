#!/usr/bin/env python3
"""Classify every emitted annotation candidate against the allowed-form whitelist.

VAL-USEFUL-007. A candidate is allowed only when the *whole* value is one of
three forms:

  * a literal        - `0`, `0x3b`, `-1`
  * a field access   - `arg0.f16`, `thread.f104.f1968`
  * a call-shaped    - `smiTag(local_m16)`, `bitField(arg0._tag, 0xc, 0x14)`

Anything else is a violation: an opaque synthesised temporary (`t7`, `tmp3`,
`objTmp2`), a bare identifier, an unrecovered register spelling, and malformed
or truncated text. Compound expressions are *not* an allowed form - a value such
as `(thread.f80 + 1)` contains a field access without being one.

This oracle is written from the contract text and shares no code with the
production classifier in `control_flow/structured.rs`: it reads only the emitted
corpus, so a faulty production classifier cannot validate its own output. The
two are kept honest against each other by disagreement showing up here as a
violation.

Usage:
  check-candidate-whitelist.py --self-test
  check-candidate-whitelist.py --label localsend <corpus-dir> [--label immich <dir> ...]

Exit status is 1 when any candidate falls outside the whitelist, or when the
self-test fails to flag a planted violation.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from collections import Counter

# The annotation literals, delimiters included. Kept byte-identical to the Rust
# definitions in `helpers/annotation.rs`; the committed drift test asserts that.
JOIN_EXHAUSTIVE_OPEN = " /* = "
JOIN_NON_EXHAUSTIVE_OPEN = " /* possible (non-exhaustive): "
LOOP_ENTRY_OPEN = " /* loop-entry value: "
PRE_CALL_OPEN = " /* value before this call: "
CANDIDATE_SEPARATOR = " | "
ANNOTATION_CLOSE = " */"

# Opener -> (loss site, literal name). The three loss sites are what the round
# reports against; the two join literals are one site.
OPENERS = {
    JOIN_EXHAUSTIVE_OPEN: ("join", "exhaustive"),
    JOIN_NON_EXHAUSTIVE_OPEN: ("join", "non-exhaustive"),
    LOOP_ENTRY_OPEN: ("loop", "loop-entry"),
    PRE_CALL_OPEN: ("call", "pre-call"),
}
LOSS_SITES = ("join", "loop", "call")

IDENTIFIER = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
LITERAL = re.compile(r"^-?(?:0[xX][0-9a-fA-F]+|[0-9]+)$")

# An opaque synthesised temporary, as named by the contract: a fixed prefix and
# a counter. `naming.rs` mints objTmp/intTmp/resultTmp/tmp and `emit.rs` mints t.
OPAQUE_TEMPORARY = re.compile(r"^(?:t|tmp|objTmp|intTmp|resultTmp)[0-9]+$")


def unrecovered_register_spellings() -> frozenset[str]:
    """Every spelling that denotes a register whose value was not recovered.

    Re-derived from the naming specification rather than read out of the
    production helper, so this oracle does not inherit a gap in it.
    """
    spellings = {"framePointer", "returnAddress", "cachedTarget", "dispatchTarget"}
    for index in range(31):
        spellings.add(f"x{index}")
        if index not in (29, 30):
            spellings.add(f"reg{index}")
        if index != 2:
            spellings.add(f"indirectTarget{index}")
    return frozenset(spellings)


FORBIDDEN_ATOMS = unrecovered_register_spellings()


def forbidden_atom(value: str) -> str | None:
    """The first identifier atom that may not appear anywhere in a candidate."""
    # `\b` matters: without it `0x14` yields the atom `x14` and every hex
    # literal in a call argument reads as a register spelling.
    for atom in re.findall(r"\b[A-Za-z_][A-Za-z0-9_]*", value):
        if atom in FORBIDDEN_ATOMS or OPAQUE_TEMPORARY.match(atom):
            return atom
    return None


def is_field_access(value: str) -> bool:
    segments = value.split(".")
    return len(segments) > 1 and all(IDENTIFIER.match(part) for part in segments)


def is_call_shaped(value: str) -> bool:
    open_index = value.find("(")
    if open_index <= 0:
        return False
    callee = value[:open_index]
    if not (IDENTIFIER.match(callee) or is_field_access(callee)):
        return False
    parens = 0
    brackets = 0
    rest = value[open_index:]
    for index, char in enumerate(rest):
        if char == "(":
            parens += 1
        elif char == ")":
            parens -= 1
            if parens < 0:
                return False
            if parens == 0:
                return index + 1 == len(rest) and brackets == 0
        elif char == "[":
            brackets += 1
        elif char == "]":
            brackets -= 1
            if brackets < 0:
                return False
    return False


def classify(value: str) -> tuple[str | None, str]:
    """`(form, reason)`. `form` is None when the candidate is a violation."""
    if value != value.strip():
        return None, "leading or trailing whitespace"
    if not value:
        return None, "empty"
    atom = forbidden_atom(value)
    if atom is not None:
        kind = "opaque temporary" if OPAQUE_TEMPORARY.match(atom) else "register spelling"
        return None, f"{kind} `{atom}`"
    if LITERAL.match(value):
        return "literal", ""
    if is_field_access(value):
        return "field-access", ""
    if is_call_shaped(value):
        return "call", ""
    if IDENTIFIER.match(value):
        return None, "bare identifier"
    return None, "not a literal, field access or call"


def scan_line(line: str):
    """Yield `(site, literal, candidate, ok)` and malformed-span reports."""
    index = 0
    while index < len(line):
        found = None
        for opener in OPENERS:
            at = line.find(opener, index)
            if at >= 0 and (found is None or at < found[0]):
                found = (at, opener)
        if found is None:
            return
        at, opener = found
        site, literal = OPENERS[opener]
        body_start = at + len(opener)
        end = line.find(ANNOTATION_CLOSE, body_start)
        if end < 0:
            yield ("annotation", site, literal, line[body_start:], "unterminated annotation")
            return
        body = line[body_start:end]
        for candidate in body.split(CANDIDATE_SEPARATOR):
            form, reason = classify(candidate)
            yield ("candidate", site, literal, candidate, reason if form is None else form)
        yield ("annotation", site, literal, body, None)
        index = end + len(ANNOTATION_CLOSE)


def scan_corpus(root: str):
    forms = {site: Counter() for site in LOSS_SITES}
    literals = Counter()
    annotations = {site: 0 for site in LOSS_SITES}
    violations = []
    files = 0
    for directory, _, names in os.walk(root):
        for name in sorted(names):
            path = os.path.join(directory, name)
            files += 1
            with open(path, encoding="utf-8", errors="surrogateescape") as handle:
                text = handle.read()
            for number, line in enumerate(text.splitlines(), start=1):
                for kind, site, literal, value, outcome in scan_line(line):
                    if kind == "annotation":
                        if outcome is None:
                            annotations[site] += 1
                            literals[literal] += 1
                        else:
                            violations.append(
                                {
                                    "file": os.path.relpath(path, root),
                                    "line": number,
                                    "site": site,
                                    "literal": literal,
                                    "value": value,
                                    "reason": outcome,
                                }
                            )
                        continue
                    if outcome in ("literal", "field-access", "call"):
                        forms[site][outcome] += 1
                    else:
                        forms[site]["VIOLATION"] += 1
                        violations.append(
                            {
                                "file": os.path.relpath(path, root),
                                "line": number,
                                "site": site,
                                "literal": literal,
                                "value": value,
                                "reason": outcome,
                            }
                        )
    return {
        "files": files,
        "annotations": annotations,
        "literals": dict(literals),
        "forms": {site: dict(counts) for site, counts in forms.items()},
        "violations": violations,
    }


PLANTED_VIOLATIONS = [
    ("t7", "opaque temporary"),
    ("tmp3", "opaque temporary"),
    ("objTmp2", "opaque temporary"),
    ("intTmp11", "opaque temporary"),
    ("resultTmp0", "opaque temporary"),
    ("t7.f8", "opaque temporary inside a field access"),
    ("smiTag(tmp3)", "opaque temporary inside a call"),
    ("reg5", "unrecovered register spelling"),
    ("x0", "unrecovered register spelling"),
    ("framePointer", "unrecovered register spelling"),
    ("dispatchTarget.f8", "unrecovered register spelling as a field base"),
    ("obj1", "bare identifier"),
    ("thread", "bare identifier"),
    ("(thread.f80 + 1)", "compound expression, not one of the three forms"),
    ("arg0.f8 + 1", "compound expression"),
    ("smiTag(arg0", "truncated call"),
    ("arg0.", "truncated field chain"),
    (".f8", "truncated field chain"),
    ("smiTag(arg0))", "malformed parentheses"),
    ("local_m8.f12[0x107]", "indexed access, not a field access"),
    ("smiTag(arg0) + 1", "call followed by an operator"),
    ("(arg0.f8)", "parenthesised, not a bare form"),
    ("0x", "malformed literal"),
    ("", "empty"),
    (" arg0.f8", "untrimmed"),
]

PLANTED_ALLOWED = [
    ("0", "literal"),
    ("-1", "literal"),
    ("0x3b", "literal"),
    ("arg0.f16", "field-access"),
    ("thread.f104.f1968", "field-access"),
    ("arg0._tag", "field-access"),
    ("smiTag(local_m16)", "call"),
    ("bitField(arg0._tag, 0xc, 0x14)", "call"),
    ("smiTag(poolOff[20680].f8)", "call"),
    ("obj1.method(arg2)", "call"),
]


def self_test() -> int:
    failures = []
    for value, why in PLANTED_VIOLATIONS:
        form, reason = classify(value)
        if form is not None:
            failures.append(f"accepted {value!r} as {form} ({why} must be rejected)")
    for value, expected in PLANTED_ALLOWED:
        form, reason = classify(value)
        if form != expected:
            failures.append(f"classified {value!r} as {form} ({reason}), expected {expected}")

    # The scanner must find a planted candidate through a real annotation span,
    # not only through `classify`: a scanner that never parses a span would
    # report zero violations on any corpus.
    line = f"  x = reg3;{JOIN_EXHAUSTIVE_OPEN}arg0.f8{CANDIDATE_SEPARATOR}t7{ANNOTATION_CLOSE}"
    seen = [entry for entry in scan_line(line) if entry[0] == "candidate"]
    if len(seen) != 2 or seen[0][4] != "field-access" or seen[1][4] == "field-access":
        failures.append(f"span scan misread the planted line: {seen}")
    for opener, (site, literal) in OPENERS.items():
        planted = f"v = 1;{opener}t7{ANNOTATION_CLOSE}"
        found = [entry for entry in scan_line(planted) if entry[0] == "candidate"]
        if len(found) != 1 or found[0][1] != site or found[0][4] in ("literal", "field-access", "call"):
            failures.append(f"{site} opener did not surface its planted violation: {found}")
    unterminated = [
        entry for entry in scan_line(f"v = 1;{LOOP_ENTRY_OPEN}arg0.f8") if entry[0] == "annotation"
    ]
    if len(unterminated) != 1 or unterminated[0][4] != "unterminated annotation":
        failures.append(f"unterminated span not reported: {unterminated}")

    for failure in failures:
        print(f"self-test FAIL: {failure}")
    print(
        f"self-test: {len(PLANTED_VIOLATIONS)} planted violations, "
        f"{len(PLANTED_ALLOWED)} allowed forms, {len(failures)} failures"
    )
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument(
        "--label",
        action="append",
        nargs=2,
        metavar=("NAME", "DIR"),
        default=[],
        help="a sample name and its pseudocode directory; repeatable, never averaged",
    )
    parser.add_argument("--json", metavar="PATH", help="write the full report here")
    parser.add_argument(
        "--max-violations", type=int, default=25, help="how many violations to print"
    )
    args = parser.parse_args()

    if args.self_test:
        return self_test()
    if not args.label:
        parser.error("nothing to scan: pass --label NAME DIR at least once")

    report = {}
    failed = False
    for name, directory in args.label:
        result = scan_corpus(directory)
        report[name] = result
        print(f"\n=== {name} ({directory}) ===")
        print(f"pseudocode files: {result['files']}")
        for site in LOSS_SITES:
            counts = result["forms"][site]
            total = sum(counts.values())
            print(
                f"  {site:5s} annotations={result['annotations'][site]:6d} "
                f"candidates={total:6d} "
                f"literal={counts.get('literal', 0):6d} "
                f"field-access={counts.get('field-access', 0):6d} "
                f"call={counts.get('call', 0):6d} "
                f"VIOLATIONS={counts.get('VIOLATION', 0):6d}"
            )
        print(f"  literals: {result['literals']}")
        print(f"  violations: {len(result['violations'])}")
        for violation in result["violations"][: args.max_violations]:
            print(
                f"    {violation['file']}:{violation['line']} [{violation['site']}]"
                f" {violation['value']!r}: {violation['reason']}"
            )
        if result["violations"]:
            failed = True

    if args.json:
        with open(args.json, "w", encoding="utf-8") as handle:
            json.dump(report, handle, indent=1)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
