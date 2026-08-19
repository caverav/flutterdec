#!/usr/bin/env python3
"""Corpus-wide check that the annotation literals are a removable overlay.

`VAL-BOUNDARY-005` asserts that each annotation literal is one opaque span:
stripping it restores the original line exactly, and unrelated comments survive.
The unit fixtures pin that on hand-built lines. This pins it on the real corpora,
where the strongest available statement is the widest one:

    strip(candidate corpus) == strip(reference corpus), byte for byte

Both sides are stripped because the round's reference commit already emits the
two join literals - annotation landed in R28, before the reference was pinned -
so an unstripped reference would report every new annotation as a difference and
prove nothing. With both sides stripped, a pass means every byte the candidate
changed lies inside an annotation span, every span is recognised, and no other
emitted text moved. A leak - a drifted opener, an unterminated span, a candidate
value carrying the terminator - shows up as a differing file rather than as a
counter that happens to agree.

The span walk mirrors `strip_join_annotation_span` in the decompiler: quoted
strings are skipped so a string literal spelling an opener is not mistaken for
one, and an unterminated span is left alone rather than swallowing the line's
tail.

Usage:
    annotation_boundary_corpus_check.py CANDIDATE_OUT REFERENCE_OUT
where each argument is a decompile output directory containing `pseudocode/`.
"""

import json
import pathlib
import re
import sys

# Read from the crate rather than restated here: this script is a consumer of
# the same authority the contract names, and a hand-written copy would be the
# defect it is checking for.
ANNOTATION_SOURCE = (
    pathlib.Path(__file__).resolve().parent.parent
    / "crates/flutterdec-decompiler/src/helpers/annotation.rs"
)


def load_literals():
    """Openers, separator and terminator, parsed out of their one definition."""
    text = ANNOTATION_SOURCE.read_text()
    openers = re.findall(r'open:\s*"((?:[^"\\]|\\.)*)"', text)
    separator = _const(text, "CANDIDATE_SEPARATOR")
    close = _const(text, "ANNOTATION_CLOSE")
    if len(openers) != 4:
        raise SystemExit(f"expected 4 annotation openers, parsed {openers}")
    return [o.encode() for o in openers], separator.encode(), close.encode()


def _const(text, name):
    match = re.search(rf'const {name}\s*:\s*&str\s*=\s*"((?:[^"\\]|\\.)*)"', text)
    if not match:
        raise SystemExit(f"{name} not found in {ANNOTATION_SOURCE}")
    return match.group(1)


OPENERS, SEPARATOR, CLOSE = load_literals()


def strip_annotations(line: bytes, counts=None) -> bytes:
    out = bytearray()
    index = 0
    size = len(line)
    while index < size:
        if line[index : index + 1] == b'"':
            end = index + 1
            while end < size:
                byte = line[end : end + 1]
                if byte == b"\\":
                    end += 2
                elif byte == b'"':
                    end += 1
                    break
                else:
                    end += 1
            end = min(end, size)
            out += line[index:end]
            index = end
            continue
        rest = line[index:]
        opener = next((o for o in OPENERS if rest.startswith(o)), None)
        if opener is None:
            out += line[index : index + 1]
            index += 1
            continue
        body = rest[len(opener) :]
        end = body.find(CLOSE)
        if end < 0:
            # Unterminated: left alone by the decompiler, so left alone here.
            out += line[index:]
            break
        if counts is not None:
            counts[opener] = counts.get(opener, 0) + 1
            counts["candidates"] = counts.get("candidates", 0) + 1 + body[:end].count(
                SEPARATOR
            )
        index += len(opener) + end + len(CLOSE)
    return bytes(out)


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    candidate = pathlib.Path(sys.argv[1]) / "pseudocode"
    reference = pathlib.Path(sys.argv[2]) / "pseudocode"

    cand_files = {p.relative_to(candidate) for p in candidate.rglob("*") if p.is_file()}
    ref_files = {p.relative_to(reference) for p in reference.rglob("*") if p.is_file()}
    result = {
        "candidate_files": len(cand_files),
        "reference_files": len(ref_files),
        "files_only_in_candidate": sorted(str(p) for p in cand_files - ref_files)[:10],
        "files_only_in_reference": sorted(str(p) for p in ref_files - cand_files)[:10],
        "differing_after_strip": [],
        "differing_files": 0,
        "annotated_files": 0,
        "annotations": {},
        "reference_annotations": {},
        "candidate_elements": 0,
        "unterminated_spans": 0,
    }

    counts = {}
    ref_counts = {}
    for relative in sorted(cand_files & ref_files):
        cand_bytes = (candidate / relative).read_bytes()
        ref_bytes = (reference / relative).read_bytes()
        before = dict(counts)
        stripped = b"\n".join(
            strip_annotations(line, counts) for line in cand_bytes.split(b"\n")
        )
        ref_stripped = b"\n".join(
            strip_annotations(line, ref_counts) for line in ref_bytes.split(b"\n")
        )
        if counts != before:
            result["annotated_files"] += 1
        if stripped != ref_stripped:
            result["differing_files"] += 1
            if len(result["differing_after_strip"]) < 10:
                result["differing_after_strip"].append(str(relative))
        # An opener surviving the strip is an unterminated or unrecognised span.
        for opener in OPENERS:
            if opener in stripped:
                result["unterminated_spans"] += stripped.count(opener)

    # All four, zeros included: a literal with no annotation in the corpus is the
    # vacuous-coverage shape this check exists to make visible, so it has to be
    # reported rather than absent from the table.
    result["annotations"] = {opener.decode(): counts.get(opener, 0) for opener in OPENERS}
    result["reference_annotations"] = {
        opener.decode(): ref_counts.get(opener, 0) for opener in OPENERS
    }
    result["candidate_elements"] = counts.get("candidates", 0)
    result["total_annotations"] = sum(result["annotations"].values())
    ok = (
        not result["files_only_in_candidate"]
        and not result["files_only_in_reference"]
        and not result["differing_after_strip"]
        and result["unterminated_spans"] == 0
        and len(result["annotations"]) == 4
        and all(count > 0 for count in result["annotations"].values())
    )
    result["ok"] = ok
    print(json.dumps(result, indent=2))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
