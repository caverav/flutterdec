#!/usr/bin/env python3
"""Check the join-site rows of a value-annotation provenance audit.

A violation is counted per *candidate element*, not per record: a record with one
good attribution and two invented ones must count two, or a mostly-fabricated
annotation passes as one satisfied row.

An element is a violation when any of these does not hold:

  1. the snapshot it cites exists, in the same function;
  2. that snapshot is keyed by the predecessor path it is the end state of, which
     is the same key every other site records - naming the site instead makes the
     pairing agree with itself for a value borrowed from a sibling path;
  3. the snapshot holds the record's register bound to exactly this value;
  4. the element's ``path_key`` names the predecessor the cited snapshot was
     taken at - which is what makes a value borrowed from a sibling path visible,
     since the borrowed value does live in *some* snapshot;
  5. the row carries ``schema_version``, ``site_key`` and ``snapshot_id`` at all.

Only ``loss_site == "join"`` rows are checked. Rows of the other loss sites have
their own assertions and their own checkers.

Usage:
    prov_join_audit_check.py AUDIT.jsonl [--schema-version N] [--quiet]

Exits 1 when the violation count is nonzero, 2 on an unusable audit file.
"""

import argparse
import json
import re
import sys

SNAPSHOT_ID = re.compile(r"^join:(\d+):pred:(\d+):(\d+)$")
EXPECTED_SCHEMA_VERSION = 1
JOIN_LOSS_SITE = "join"
PATH_KIND = "block"


def load(path):
    snapshots = {}
    annotations = []
    with open(path, encoding="utf-8") as handle:
        for number, line in enumerate(handle, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError as error:
                raise SystemExit(f"{path}:{number}: not JSON: {error}")
            kind = row.get("record")
            if kind == "snapshot":
                snapshots[(row.get("function_id"), row.get("snapshot_id"))] = row
            elif kind == "annotation":
                annotations.append((number, row))
            else:
                raise SystemExit(f"{path}:{number}: unknown record kind {kind!r}")
    return snapshots, annotations


def site_key_of(row, field="site_key"):
    key = row.get(field)
    if isinstance(key, list) and len(key) == 2:
        return (key[0], key[1])
    return None


def check(path, schema_version):
    snapshots, annotations = load(path)
    violations = []
    join_records = 0
    candidate_elements = 0

    for number, record in annotations:
        if record.get("loss_site") != JOIN_LOSS_SITE:
            continue
        join_records += 1
        where = f"{path}:{number}"

        def fail(reason, element=None):
            violations.append(
                {
                    "where": where,
                    "function_id": record.get("function_id"),
                    "site_key": record.get("site_key"),
                    "register": record.get("register"),
                    "element": element,
                    "reason": reason,
                }
            )

        site = site_key_of(record)
        candidates = record.get("candidates")
        # A structurally broken row is a violation on every element it claims,
        # and at least one when it claims none.
        structural = []
        if record.get("schema_version") != schema_version:
            structural.append(f"schema_version is {record.get('schema_version')!r}")
        if site is None or site[0] != JOIN_LOSS_SITE:
            structural.append(f"site_key {record.get('site_key')!r} is not a tagged join key")
        if not isinstance(candidates, list) or not candidates:
            structural.append("candidates is missing or empty")
        if structural:
            for reason in structural:
                fail(reason)
            if not isinstance(candidates, list):
                continue

        for index, element in enumerate(candidates):
            candidate_elements += 1
            label = f"candidates[{index}]"
            snapshot_id = element.get("snapshot_id")
            value = element.get("value")
            path_key = site_key_of(element, "path_key")
            if not snapshot_id:
                fail("no snapshot_id", label)
                continue
            if path_key is None or path_key[0] != PATH_KIND:
                fail(f"path_key {element.get('path_key')!r} is not a tagged block key", label)
                continue
            parsed = SNAPSHOT_ID.match(snapshot_id)
            if parsed is None:
                fail(f"snapshot_id {snapshot_id!r} does not name a join and a predecessor", label)
                continue
            snapshot_join, snapshot_pred = int(parsed.group(1)), int(parsed.group(2))
            snapshot = snapshots.get((record.get("function_id"), snapshot_id))
            if snapshot is None:
                fail(f"cited snapshot {snapshot_id!r} is not recorded", label)
                continue
            if site is not None and snapshot_join != site[1]:
                fail(
                    f"snapshot {snapshot_id!r} was taken at join {snapshot_join}, "
                    f"but the record claims join {site[1]}",
                    label,
                )
                continue
            if site_key_of(snapshot) != path_key:
                fail(
                    f"snapshot {snapshot_id!r} is the end state of {snapshot.get('site_key')!r}, "
                    f"but the value is attributed to path {element.get('path_key')!r}",
                    label,
                )
                continue
            if snapshot_pred != path_key[1]:
                fail(
                    f"value is attributed to predecessor {path_key[1]} but was read out of "
                    f"predecessor {snapshot_pred}'s snapshot",
                    label,
                )
                continue
            registers = dict(
                (name, bound) for name, bound in snapshot.get("registers", [])
            )
            register = record.get("register")
            if register not in registers:
                fail(f"snapshot {snapshot_id!r} holds no binding for {register!r}", label)
                continue
            if registers[register] != value:
                fail(
                    f"snapshot {snapshot_id!r} holds {register}={registers[register]!r}, "
                    f"not {value!r}",
                    label,
                )

    return {
        "audit": path,
        "join_records": join_records,
        "candidate_elements": candidate_elements,
        "violations": violations,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("audit")
    parser.add_argument("--schema-version", type=int, default=EXPECTED_SCHEMA_VERSION)
    parser.add_argument("--quiet", action="store_true")
    args = parser.parse_args()

    result = check(args.audit, args.schema_version)
    count = len(result["violations"])
    if not args.quiet:
        for violation in result["violations"][:50]:
            print(
                "violation {where} fn={function_id} site={site_key} "
                "reg={register} {element}: {reason}".format(**violation)
            )
    print(
        "audit={audit} join_annotations={join_records} "
        "candidate_elements={candidate_elements} violations={count}".format(
            count=count, **result
        )
    )
    return 1 if count else 0


if __name__ == "__main__":
    sys.exit(main())
