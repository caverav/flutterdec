#!/usr/bin/env python3
"""Show that the join provenance checker detects a real violation.

A zero from a checker never shown able to fail is not evidence, and breaking the
checker to make it print a nonzero proves nothing either. So the checker is left
byte-for-byte alone here: this script mutates a *copy of the audit* the way a
defective emitter would, and asserts the unmodified checker reports it.

Both plants are the same defect from two directions - a candidate whose value did
not come from the path it is attributed to:

  wrong value    the element keeps its own predecessor and snapshot, but carries
                 the value a sibling predecessor held. Nothing about the value is
                 invented: it is genuinely in the audit, just not on this path,
                 which is exactly the failure a self-consistent emitter produces.
  wrong snapshot the element keeps its own predecessor and value, but cites the
                 sibling's snapshot, so the attribution and the recorded capture
                 disagree.

Also asserts the clean fixture scores zero, so a checker that failed everything
would not pass this either.

Usage: prov_join_audit_plant_test.py [AUDIT.jsonl]
"""

import copy
import json
import pathlib
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
CHECKER = HERE / "prov_join_audit_check.py"
DEFAULT_AUDIT = HERE.parent / "testdata" / "provenance" / "join-audit-sample.jsonl"


def run_checker(path):
    finished = subprocess.run(
        [sys.executable, str(CHECKER), str(path)],
        capture_output=True,
        text=True,
        check=False,
    )
    summary = finished.stdout.strip().splitlines()[-1]
    violations = int(summary.rsplit("violations=", 1)[1])
    return finished.returncode, violations, finished.stdout.strip()


def rows(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def write(rows_, directory, name):
    path = pathlib.Path(directory) / name
    path.write_text("".join(json.dumps(row) + "\n" for row in rows_))
    return path


def first_distinct_valued_join(rows_):
    """A join annotation with two candidates carrying different values.

    Equal values would make the value plant a no-op, which would pass for a
    checker that never looks at the value at all.
    """
    for index, row in enumerate(rows_):
        if row.get("record") != "annotation" or row.get("loss_site") != "join":
            continue
        candidates = row.get("candidates", [])
        if len(candidates) >= 2 and candidates[0]["value"] != candidates[1]["value"]:
            return index
    raise SystemExit("the audit has no join annotation with two differing candidate values")


def main():
    audit = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_AUDIT
    clean = rows(audit)
    target = first_distinct_valued_join(clean)
    failures = []

    with tempfile.TemporaryDirectory() as directory:
        code, violations, output = run_checker(write(clean, directory, "clean.jsonl"))
        print(f"clean: rc={code} {output}")
        if code != 0 or violations != 0:
            failures.append("the unmutated audit must score zero")

        for label, field in (("wrong value", "value"), ("wrong snapshot", "snapshot_id")):
            planted = copy.deepcopy(clean)
            candidates = planted[target]["candidates"]
            stolen = candidates[1][field]
            candidates[0][field] = stolen
            path = write(planted, directory, f"planted-{field}.jsonl")
            code, violations, output = run_checker(path)
            print(f"planted {label}: rc={code} {output}")
            if code == 0:
                failures.append(f"the {label} plant was not reported")
            # Per candidate element, not per record: one bad attribution in a
            # record with a good one must count exactly one.
            if violations != 1:
                failures.append(
                    f"the {label} plant scored {violations} violations, expected 1"
                )

    for failure in failures:
        print(f"FAIL: {failure}")
    print("PASS" if not failures else "FAILED")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
