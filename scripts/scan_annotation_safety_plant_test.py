#!/usr/bin/env python3
"""Negative control for the annotation safety scan.

A scan that reports zero is worth exactly as much as its ability to report one.
This plants each of the four violations into a copy of a real emitted file and
asserts the scan finds it, then asserts the unplanted copy is clean.

Usage: scan_annotation_safety_plant_test.py <a-file-from-the-corpus>
"""

import importlib.util
import json
import pathlib
import subprocess
import sys
import tempfile

SCAN = pathlib.Path(__file__).resolve().parent / "scan-annotation-safety.py"


def scan_module():
    """The scan itself, so the literals come from its one reader of them."""
    spec = importlib.util.spec_from_file_location("scan_annotation_safety", SCAN)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def scan(directory):
    result = subprocess.run(
        [sys.executable, str(SCAN), str(directory)],
        capture_output=True,
        text=True,
        check=False,
    )
    return json.loads(result.stdout), result.returncode


def main():
    source = pathlib.Path(sys.argv[1]).read_text()
    # Whichever literal this file happens to carry, read from the emitter's own
    # definitions through the scan: the four are interchangeable for the purpose
    # of corrupting one, and a spelling written here would drift.
    openers, close = scan_module().literals()
    opener = next((o for o in openers if o in source), None)
    if opener is None:
        raise SystemExit(f"{sys.argv[1]} carries no annotation to corrupt")

    plants = {
        # A brace inside the span: the compaction pass reads a block that is not
        # there.
        "forbidden": source.replace(opener, opener + "{x} ", 1),
        # A line over the budget, carrying no annotation at all: the cap is a
        # property of the emitted line, not only of annotated ones.
        "over_cap": source + "\n" + "x" * 3001,
        # A span longer than the per-annotation budget. Nothing the emitter can
        # produce, which is the point: the scan must say so if one appears.
        "over_span": source.replace(opener, opener + "v" * 600 + " | ", 1),
        # The shape a truncated annotation leaves behind.
        "unclosed": source.replace(close, "", 1),
    }

    failures = []
    with tempfile.TemporaryDirectory() as root:
        clean = pathlib.Path(root) / "clean"
        clean.mkdir()
        (clean / "sample.dart").write_text(source)
        report, code = scan(clean)
        if code != 0:
            failures.append(f"the unplanted copy is not clean: {report['violation_counts']}")

        for name, text in plants.items():
            planted = pathlib.Path(root) / name
            planted.mkdir()
            (planted / "sample.dart").write_text(text)
            report, code = scan(planted)
            found = report["violation_counts"][name]
            print(f"{name}: found={found} rc={code}")
            if found == 0 or code == 0:
                failures.append(f"the scan did not report a planted {name}")

    for failure in failures:
        print(f"FAIL {failure}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
