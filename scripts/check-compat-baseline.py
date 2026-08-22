#!/usr/bin/env python3
"""Self-check for the public real-binary compatibility baseline.

`docs/compat-baseline-real-binary.md` and `docs/compat-evidence/` record one
whole-APK decompile of a pinned public LocalSend release at the fixed reference
`1371e42` and at the branch head. This script is the guard that keeps that
record usable:

  verify   (default) offline: the recipe is fetchable and pinned, the manifest
           agrees with the counts every other file claims, no public schema key
           was dropped, and every observed difference class is adjudicated in
           the prose document.
  fetch    download the pinned asset and fail unless size and SHA-256 match.
  replay   compare a fresh candidate output tree against the committed
           per-artifact manifest, which is what proves deterministic bytes.

`verify` is offline and is the mode to run after touching the baseline;
`scripts/ci-check.sh` does not call it yet, for the protected-path reason in
section 8 of the prose document. `fetch` and `replay` need the network or a real
run and are the independent-rerun path.
"""

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
EVIDENCE = REPO / "docs" / "compat-evidence"
DOC = REPO / "docs" / "compat-baseline-real-binary.md"
RECIPE = EVIDENCE / "input-recipe.json"
MANIFEST = EVIDENCE / "artifact-manifest.tsv"
INVENTORY = EVIDENCE / "function-inventory.tsv"
MANIFEST_HEADER = "path\tref_bytes\tref_sha256\tcand_bytes\tcand_sha256"
REPLAY_MARKER_TABLE = EVIDENCE / "marker-replay.tsv"
LOSS_TABLE = EVIDENCE / "operand-direction-losses.tsv"
REPORT = "report.json"

HEX40 = 40
HEX64 = 64


def normalize_report(doc):
    """Replace the three workspace-dependent strings the recipe declares volatile.

    The two absolute paths are cut at their in-repository segment rather than at a
    known repository root, so a replay from any checkout normalizes to the same
    text as the committed snapshot.
    """
    doc = dict(doc)
    doc["input"] = "<input>"
    for section, key, marker in (("adapter_selection", "adapter_exec_path", "/adapters/"),
                                 ("engine_symbol_ingestion", "manifest_path", "/symbols/")):
        value = doc.get(section, {}).get(key, "")
        if isinstance(value, str) and marker in value:
            doc[section] = {**doc[section], key: "<repo>" + value[value.index(marker):]}
    return doc


def is_hex(value, width):
    return isinstance(value, str) and len(value) == width and all(c in "0123456789abcdef" for c in value)


def read_manifest(text):
    """Rows as {path: (ref_bytes, ref_sha, cand_bytes, cand_sha)}, `=` resolved."""
    lines = text.splitlines()
    if not lines or lines[0] != MANIFEST_HEADER:
        raise ValueError(f"manifest header must be {MANIFEST_HEADER!r}")
    rows = {}
    identical = 0
    for line in lines[1:]:
        path, ref_bytes, ref_sha, cand_bytes, cand_sha = line.split("\t")
        if (cand_bytes, cand_sha) == ("=", "="):
            identical += 1
            cand_bytes, cand_sha = ref_bytes, ref_sha
        rows[path] = (int(ref_bytes), ref_sha, int(cand_bytes), cand_sha)
    return rows, identical


def verify(failures):
    recipe = json.loads(RECIPE.read_text(encoding="utf-8"))
    src = recipe["input"]
    if not src["url"].startswith("https://github.com/localsend/localsend/releases/download/"):
        failures.append("input.url is not a pinned public release asset URL")
    if src["release_tag"] not in src["url"]:
        failures.append("input.url does not carry the pinned release tag")
    if not is_hex(src["sha256"], HEX64):
        failures.append("input.sha256 is not a sha256 digest")
    if not isinstance(src["bytes"], int) or src["bytes"] <= 0:
        failures.append("input.bytes is not a positive byte size")
    for field in ("license", "license_url"):
        if not src.get(field):
            failures.append(f"input.{field} is missing")
    for side in ("reference", "candidate"):
        if not is_hex(recipe["revisions"][side], HEX40):
            failures.append(f"revisions.{side} is not a full commit id")
        if not is_hex(recipe["binaries"][f"{side}_sha256"], HEX64):
            failures.append(f"binaries.{side}_sha256 is not a sha256 digest")

    rows, identical = read_manifest(MANIFEST.read_text(encoding="utf-8"))
    artifacts = recipe["artifacts"]
    if len(rows) != artifacts["files_per_run"]:
        failures.append(f"manifest has {len(rows)} rows, recipe claims {artifacts['files_per_run']}")
    if identical != artifacts["identical_between_reference_and_candidate"]:
        failures.append(
            f"manifest has {identical} identical artifacts, recipe claims "
            f"{artifacts['identical_between_reference_and_candidate']}"
        )
    for kind, suffix in (("pseudocode", ".dartpseudo"), ("ir", ".json"), ("asm", ".s")):
        got = sum(1 for p in rows if p.startswith(f"{kind}/") and p.endswith(suffix))
        if got != artifacts[kind]:
            failures.append(f"manifest has {got} {kind} artifacts, recipe claims {artifacts[kind]}")
    for report in artifacts["reports"]:
        if report not in rows:
            failures.append(f"manifest is missing the {report} row")
    for path, (_, ref_sha, _, cand_sha) in rows.items():
        if not is_hex(ref_sha, HEX64) or not is_hex(cand_sha, HEX64):
            failures.append(f"manifest row {path} does not carry two sha256 digests")
            break

    inventory = INVENTORY.read_text(encoding="utf-8").splitlines()
    if len(inventory) - 1 != artifacts["pseudocode"]:
        failures.append("function inventory row count does not match the emitted function count")
    for line in inventory[1:]:
        _, _, pseudo, ir, asm = line.split("\t")
        missing = [p for p in (pseudo, ir, asm) if p not in rows]
        if missing:
            failures.append(f"function inventory names artifacts absent from the manifest: {missing}")
            break

    # A dropped public key is the compatibility break this baseline exists to catch.
    schema = json.loads((EVIDENCE / "schema-comparison.json").read_text(encoding="utf-8"))
    for surface in ("ir", "report_json", "quality_json"):
        dropped = schema[surface]["removed_in_candidate"]
        if dropped:
            failures.append(f"{surface} dropped public keys in the candidate: {dropped}")

    quality = json.loads((EVIDENCE / "quality-candidate.json").read_text(encoding="utf-8"))
    accounting = json.loads((EVIDENCE / "accounting-reconciliation.json").read_text(encoding="utf-8"))
    if accounting["candidate_quality_emission"] != quality["emission"]:
        failures.append("accounting reconciliation and quality-candidate.json disagree on emission")
    for counter, recorded in accounting["text_counter_reconciliation"]["candidate"].items():
        if recorded["quality_json"] != quality[counter] or not recorded["match"]:
            failures.append(f"text counter {counter} does not reconcile with quality-candidate.json")

    # A committed snapshot that still carries a workspace path is not reproducible
    # anywhere else, and it is how a stale volatile-field list goes unnoticed.
    for side in ("reference", "candidate"):
        report = json.loads((EVIDENCE / f"report-{side}.json").read_text(encoding="utf-8"))
        for field, value in (
            ("input", report["input"]),
            ("adapter_selection.adapter_exec_path", report["adapter_selection"]["adapter_exec_path"]),
            ("engine_symbol_ingestion.manifest_path", report["engine_symbol_ingestion"]["manifest_path"]),
        ):
            if not value.startswith("<"):
                failures.append(f"report-{side}.json {field} is not normalized: {value}")
            if field not in recipe["volatile_fields"][REPORT]:
                failures.append(f"volatile_fields does not declare {field}")

    classes = json.loads((EVIDENCE / "difference-classes.json").read_text(encoding="utf-8"))
    if classes["asm"]["differing_files"] != 0:
        failures.append("assembly output changed; that is not an accepted difference class")

    # The two adjudication tables have to hold the rows the JSON counts claim.
    replay_rows = REPLAY_MARKER_TABLE.read_text(encoding="utf-8").splitlines()[1:]
    residue = accounting["unresolved_control_flow"]
    if len(replay_rows) != residue["marker_bearing_functions"]:
        failures.append(
            f"{REPLAY_MARKER_TABLE.name} has {len(replay_rows)} rows, "
            f"{residue['marker_bearing_functions']} marker-bearing functions are claimed"
        )
    identical = [r.split("\t") for r in replay_rows if r.split("\t")[4] == "true"]
    if len(identical) != residue["per_function_replay_identical"]:
        failures.append(f"{len(identical)} replays reproduced the whole-run pseudocode, "
                        f"{residue['per_function_replay_identical']} are claimed")
    if any(row[2] != row[3] for row in identical):
        failures.append("a reproduced replay disagrees with its own unresolved_cf counter")
    if sum(int(row[1]) for row in [r.split("\t") for r in replay_rows]) != residue["candidate_emitted_markers"]:
        failures.append("the replay table does not account for every emitted marker")
    losses = LOSS_TABLE.read_text(encoding="utf-8").splitlines()[1:]
    if len(losses) != classes["operand_naming_direction"]["fewer_registers"]:
        failures.append(f"{LOSS_TABLE.name} has {len(losses)} rows, "
                        f"{classes['operand_naming_direction']['fewer_registers']} are claimed")
    doc = DOC.read_text(encoding="utf-8")
    for name in (list(classes["ir"]["op_transitions"])
                 + list(classes["ir"]["branch_target_shape_transitions"])
                 + list(classes["ir"]["instructions_only_in_reference"])):
        if name not in doc:
            failures.append(f"difference class {name!r} is not adjudicated in {DOC.name}")
    process_fields = (
        "candidate_manifest_sha256",
        "second_candidate_process_manifest_sha256",
        "third_candidate_process_manifest_sha256",
    )
    for digest_field in ("reference_manifest_sha256",) + process_fields:
        if not is_hex(artifacts[digest_field], HEX64):
            failures.append(f"artifacts.{digest_field} is not a sha256 digest")
    if len({artifacts[f] for f in process_fields}) != 1:
        failures.append("the candidate processes did not agree; the baseline is not deterministic")
    if artifacts["candidate_processes_compared"] != len(process_fields):
        failures.append("candidate_processes_compared does not match the recorded process digests")
    return recipe


def fetch(dest: Path):
    recipe = json.loads(RECIPE.read_text(encoding="utf-8"))["input"]
    dest.parent.mkdir(parents=True, exist_ok=True)
    if not dest.exists():
        subprocess.run(["curl", "-fsSL", "-o", str(dest), recipe["url"]], check=True)
    size = dest.stat().st_size
    digest = hashlib.sha256(dest.read_bytes()).hexdigest()
    if size != recipe["bytes"] or digest != recipe["sha256"]:
        raise SystemExit(
            f"[compat-baseline] fetched bytes do not match the recipe: {size} {digest}"
        )
    print(f"[compat-baseline] verified {dest} ({size} bytes, sha256 {digest})")


def replay(out: Path, failures):
    rows, _ = read_manifest(MANIFEST.read_text(encoding="utf-8"))
    seen = set()
    for path in sorted(out.rglob("*")):
        if path.is_file():
            seen.add(path.relative_to(out).as_posix())
    missing = sorted(set(rows) - seen)
    extra = sorted(seen - set(rows))
    if missing:
        failures.append(f"{len(missing)} baseline artifacts are missing, first: {missing[:3]}")
    if extra:
        failures.append(f"{len(extra)} unexpected artifacts were produced, first: {extra[:3]}")
    differing = []
    for rel in sorted(set(rows) & seen):
        # report.json carries the three volatile workspace strings, so it is compared
        # against the normalized committed snapshot instead of by digest.
        if rel == REPORT:
            fresh = normalize_report(json.loads((out / rel).read_text(encoding="utf-8")))
            committed = json.loads((EVIDENCE / "report-candidate.json").read_text(encoding="utf-8"))
            if fresh != committed:
                changed = sorted(k for k in set(fresh) | set(committed)
                                 if fresh.get(k) != committed.get(k))
                failures.append(f"report.json differs from the baseline outside the volatile "
                                f"fields, in: {changed}")
            continue
        digest = hashlib.sha256((out / rel).read_bytes()).hexdigest()
        if digest != rows[rel][3]:
            differing.append(rel)
    if differing:
        failures.append(f"{len(differing)} artifacts differ from the baseline, first: {differing[:3]}")
    print(f"[compat-baseline] replayed {len(seen)} artifacts against {len(rows)} baseline rows")


def self_test():
    rows, identical = read_manifest(
        MANIFEST_HEADER + "\npseudocode/a.dartpseudo\t1\t" + "ab" * 32 + "\t=\t=\n"
    )
    assert identical == 1
    assert rows["pseudocode/a.dartpseudo"] == (1, "ab" * 32, 1, "ab" * 32)
    assert is_hex("ab" * 32, HEX64) and not is_hex("zz" * 32, HEX64)
    try:
        read_manifest("path\tonly\n")
    except ValueError:
        pass
    else:  # pragma: no cover - guarded by the assert below
        raise AssertionError("a bad manifest header must be rejected")
    doc = normalize_report(
        {
            "input": "/w/in.apk",
            "adapter_selection": {"adapter_exec_path": "/any/checkout/adapters/installed/a",
                                  "kind": "internal"},
            "engine_symbol_ingestion": {"manifest_path": "/elsewhere/nothing.json"},
        }
    )
    assert doc["input"] == "<input>"
    assert doc["adapter_selection"] == {"adapter_exec_path": "<repo>/adapters/installed/a",
                                       "kind": "internal"}
    # a value with no in-repository segment is left alone rather than silently rewritten
    assert doc["engine_symbol_ingestion"]["manifest_path"] == "/elsewhere/nothing.json"
    print("[compat-baseline] self-test ok")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", nargs="?", default="verify", choices=("verify", "fetch", "replay"))
    parser.add_argument("--dest", type=Path, help="fetch target path")
    parser.add_argument("--out", type=Path, help="replay output directory")
    parser.add_argument("--self-test", action="store_true", help="run the parser self-test and exit")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return 0

    failures = []
    if args.mode == "fetch":
        fetch(args.dest or (REPO / ".compat-input" / json.loads(RECIPE.read_text())["input"]["asset"]))
        return 0
    if args.mode == "replay":
        if not args.out:
            raise SystemExit("[compat-baseline] replay needs --out <decompile output directory>")
        verify(failures)
        replay(args.out, failures)
    else:
        recipe = verify(failures)
        print(
            f"[compat-baseline] {recipe['input']['asset']} @ {recipe['input']['release_tag']}, "
            f"reference {recipe['revisions']['reference'][:7]}, "
            f"candidate {recipe['revisions']['candidate'][:7]}, "
            f"{recipe['artifacts']['files_per_run']} artifacts per run"
        )

    for failure in failures:
        print(f"[compat-baseline] FAIL: {failure}", file=sys.stderr)
    if failures:
        return 1
    print("[compat-baseline] ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
