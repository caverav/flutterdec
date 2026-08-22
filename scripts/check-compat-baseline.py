#!/usr/bin/env python3
"""Self-check for the public real-binary compatibility baseline.

`docs/compat-baseline-real-binary.md` and `docs/compat-evidence/` record one
whole-APK decompile of a pinned public LocalSend release at the fixed reference
`1371e42` and at the branch head. This script is the guard that keeps that
record usable:

  verify   (default) offline: the recipe is fetchable and pinned, the offline
           adapter step the run needs is recorded and documented, the four
           aggregate manifest digests recompute from the per-artifact rows, the
           manifest agrees with the counts every other file claims, no public
           schema key was dropped, both register-counter scopes reconcile, and
           every observed difference class - including every class of removed
           pseudocode - is adjudicated in the prose document.
  fetch    download the pinned asset and fail unless size and SHA-256 match.
  replay   compare a fresh candidate output tree against the committed
           per-artifact manifest, which is what proves deterministic bytes, and
           recompute that tree's aggregate manifest digest.

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
import tempfile
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
REMOVAL_TABLE = EVIDENCE / "pseudocode-callee-removals.tsv"
REMOVAL_AGGREGATE = EVIDENCE / "callee-removals-aggregate.json"
REGISTER_SCOPES = EVIDENCE / "register-counter-scopes.json"
REMOVAL_HEADER = (
    "file\tclass\tcallee_renderings_lost\twholly_vanished_callees\tremoved_lines\t"
    "added_lines\tcandidate_marker\tir_reference_only_instructions\t"
    "lost_edge_effects\tvanished_callee_names\tlost_rendering_detail"
)
REPORT = "report.json"

HEX40 = 40
HEX64 = 64

# The three candidate processes wrote the same bytes, so one derivation covers
# all four recorded digests.
CANDIDATE_DIGEST_FIELDS = (
    "candidate_manifest_sha256",
    "second_candidate_process_manifest_sha256",
    "third_candidate_process_manifest_sha256",
)
MANIFEST_DIGEST_DERIVATION = (
    "sha256 over one line per emitted artifact, '<path>\\t<bytes>\\t<sha256>\\n', "
    "paths in ascending byte order, no header; see check-compat-baseline.py "
    "side_manifest_text and tree_manifest_digest"
)


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


def side_manifest_text(rows, side):
    """The one-run manifest whose digest is `artifacts.<side>_manifest_sha256`.

    One `<path>\\t<bytes>\\t<sha256>\\n` line per emitted artifact, paths in
    ascending byte order, no header, trailing newline included. This is the
    exact text the baseline run hashed, so the recorded aggregate digests are
    recomputable offline from the committed per-artifact rows alone.
    """
    index = 0 if side == "reference" else 2
    return "".join(
        f"{path}\t{row[index]}\t{row[index + 1]}\n" for path, row in sorted(rows.items())
    )


def manifest_digest(rows, side):
    return hashlib.sha256(side_manifest_text(rows, side).encode("utf-8")).hexdigest()


def tree_manifest_digest(out: Path):
    """The same derivation, over a freshly emitted output tree."""
    lines = []
    for path in sorted(p.relative_to(out).as_posix() for p in out.rglob("*") if p.is_file()):
        payload = (out / path).read_bytes()
        lines.append(f"{path}\t{len(payload)}\t{hashlib.sha256(payload).hexdigest()}\n")
    return hashlib.sha256("".join(lines).encode("utf-8")).hexdigest()


def parse_table(text, header, name):
    lines = text.splitlines()
    if not lines or lines[0] != header:
        raise ValueError(f"{name} header must be {header!r}")
    return [line.split("\t") for line in lines[1:]]


def read_table(path, header):
    return parse_table(path.read_text(encoding="utf-8"), header, path.name)


def check_adapter(recipe, doc, failures):
    """The run needs one offline adapter step; a recipe without it emits nothing.

    `adapters/installed/*` is gitignored, so a cold checkout has no adapter and
    the decompile aborts before writing a single artifact. The step is recorded
    here and required to appear verbatim in the rerun recipe.
    """
    adapter = recipe["adapter"]
    if adapter["snapshot_hash"] != recipe["input"]["snapshot_hash"]:
        failures.append("adapter.snapshot_hash disagrees with input.snapshot_hash")
    if adapter["snapshot_hash"] not in adapter["install_command"]:
        failures.append("adapter.install_command does not carry the pinned snapshot hash")
    if adapter["install_command"] not in doc:
        failures.append(f"the adapter install step is not in {DOC.name}")
    if not (REPO / adapter["template"]).exists():
        failures.append(f"the tracked adapter template {adapter['template']} is missing")
    if not is_hex(adapter["installed_sha256"], HEX64):
        failures.append("adapter.installed_sha256 is not a sha256 digest")
    installed = REPO / adapter["installed_path"]
    if adapter["snapshot_hash"] not in adapter["installed_path"]:
        failures.append("adapter.installed_path does not name the pinned snapshot hash")
    if installed.exists():
        payload = installed.read_bytes()
        digest = hashlib.sha256(payload).hexdigest()
        if len(payload) != adapter["installed_bytes"] or digest != adapter["installed_sha256"]:
            failures.append(
                f"the installed adapter does not match the recipe: {len(payload)} {digest}"
            )


def check_removals(classes, doc, failures):
    """Removed pseudocode is accounted for per file, per class, and per callee."""
    aggregate = json.loads(REMOVAL_AGGREGATE.read_text(encoding="utf-8"))
    counts = aggregate["aggregate"]
    by_class = aggregate["removed_lines_by_class"]
    summary = classes["pseudocode"]["callee_rendering_removals"]
    if summary["removed_lines_by_class"] != by_class:
        failures.append("difference-classes.json and the removal aggregate disagree on the classes")
    for key, value in summary.items():
        if key in counts and counts[key] != value:
            failures.append(f"difference-classes.json and the removal aggregate disagree on {key}")
    rows = read_table(REMOVAL_TABLE, REMOVAL_HEADER)
    if len(rows) != counts["files_losing_a_callee_rendering"]:
        failures.append(
            f"{REMOVAL_TABLE.name} has {len(rows)} rows, "
            f"{counts['files_losing_a_callee_rendering']} files are claimed"
        )
    if sum(int(row[2]) for row in rows) != counts["callee_renderings_lost"]:
        failures.append("the removal table does not account for every lost callee rendering")
    vanished_rows = [row for row in rows if int(row[3])]
    if len(vanished_rows) != counts["files_with_a_wholly_vanished_callee"]:
        failures.append(
            f"{len(vanished_rows)} rows carry a wholly vanished callee, "
            f"{counts['files_with_a_wholly_vanished_callee']} are claimed"
        )
    named = {}
    for row in vanished_rows:
        for item in row[10].split(";"):
            callee, _, count = item.rpartition(":")
            if callee in row[9].split(";"):
                named[callee] = named.get(callee, 0) + int(count)
    if named != aggregate["vanished_callees"]:
        failures.append("the enumerated vanished callees do not sum to the recorded totals")
    if len(named) != counts["distinct_wholly_vanished_callees"]:
        failures.append(
            f"{len(named)} distinct vanished callees are enumerated, "
            f"{counts['distinct_wholly_vanished_callees']} are claimed"
        )
    per_class = {}
    for row in rows:
        per_class[row[1]] = per_class.get(row[1], 0) + int(row[4])
    for name, removed in per_class.items():
        if by_class.get(name) != removed:
            failures.append(f"removed lines for class {name} do not match the enumerated rows")
    if sum(by_class.values()) != classes["pseudocode"]["removed_lines_total"]:
        failures.append(
            "the removed-line classes do not partition pseudocode.removed_lines_total"
        )
    # The indirect-branch class is only an emitter-surface removal if the IR kept
    # every reference instruction; one lost instruction would make it a real loss.
    ir_only = aggregate["ir_reference_only_instructions_by_class"]
    if ir_only.get("vanished_behind_indirect_branch") != 0:
        failures.append(
            "the indirect-branch removal class lost reference IR instructions, "
            "so it is not an emitter-surface removal"
        )
    for row in vanished_rows:
        if int(row[7]) and "trap" not in row[1]:
            failures.append(f"{row[0]} lost reference IR instructions outside the trap class")
            break
    for name in by_class:
        if name not in doc:
            failures.append(f"removed-pseudocode class {name!r} is not adjudicated in {DOC.name}")
    files = {row[0] for row in rows}
    for name, representative in aggregate["representative_file_by_class"].items():
        if representative not in files:
            failures.append(f"the representative file for {name} is not in the removal table")
        if representative not in doc:
            failures.append(f"the representative file for {name} is not named in {DOC.name}")


def check_register_scopes(failures):
    """The census counts text; the quality counter counts a scope inside it."""
    scopes = json.loads(REGISTER_SCOPES.read_text(encoding="utf-8"))
    for side in ("reference", "candidate"):
        entry = scopes[side]
        counts = entry["counts"]
        quality = json.loads((EVIDENCE / f"quality-{side}.json").read_text(encoding="utf-8"))
        census = json.loads(
            (EVIDENCE / f"structural-census-{side}.json").read_text(encoding="utf-8")
        )["counts"]["register_operand"]
        scope = entry["quality_counter_scope"]
        if scope not in ("whole_line", "code_span"):
            failures.append(f"{side} declares an unknown register-counter scope: {scope}")
            continue
        recounted = counts[f"{scope}_scope_total"]
        if recounted != quality["raw_register_name_refs"]:
            failures.append(
                f"the {scope} scope recounts {recounted} register tokens on the {side}, "
                f"quality-{side}.json reports {quality['raw_register_name_refs']}"
            )
        if counts["census_regN_over_whole_text"] != census:
            failures.append(f"the {side} register census does not match structural-census-{side}")
        excluded = counts["whole_line_scope_total"] - counts["code_span_scope_total"]
        if excluded != counts["excluded_by_code_span_filter_total"]:
            failures.append(f"the {side} code-span exclusion total does not close")
        split = sum(value for key, value in counts.items()
                    if key.startswith("excluded_by_code_span_filter_in_"))
        if split != counts["excluded_by_code_span_filter_total"]:
            failures.append(f"the {side} code-span exclusions are not split by span kind")


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
    check_adapter(recipe, doc, failures)
    check_removals(classes, doc, failures)
    check_register_scopes(failures)
    for digest_field in ("reference_manifest_sha256",) + CANDIDATE_DIGEST_FIELDS:
        if not is_hex(artifacts[digest_field], HEX64):
            failures.append(f"artifacts.{digest_field} is not a sha256 digest")
    if len({artifacts[f] for f in CANDIDATE_DIGEST_FIELDS}) != 1:
        failures.append("the candidate processes did not agree; the baseline is not deterministic")
    if artifacts["candidate_processes_compared"] != len(CANDIDATE_DIGEST_FIELDS):
        failures.append("candidate_processes_compared does not match the recorded process digests")
    # A recorded aggregate digest nobody can recompute is a claim, not evidence.
    if artifacts["manifest_digest_derivation"] != MANIFEST_DIGEST_DERIVATION:
        failures.append("artifacts.manifest_digest_derivation does not state the derivation used")
    for side, fields in (("reference", ("reference_manifest_sha256",)),
                         ("candidate", CANDIDATE_DIGEST_FIELDS)):
        recomputed = manifest_digest(rows, side)
        for field in fields:
            if artifacts[field] != recomputed:
                failures.append(
                    f"artifacts.{field} does not recompute from the per-artifact "
                    f"manifest: {recomputed}"
                )
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
    # The same derivation as the recorded aggregates. It matches only when the
    # replay ran from the recorded workspace, because report.json carries the
    # three volatile strings; the per-artifact comparison above is the check
    # that has to hold from any checkout.
    recorded = json.loads(RECIPE.read_text(encoding="utf-8"))["artifacts"]
    fresh = tree_manifest_digest(out)
    print(f"[compat-baseline] replayed tree manifest digest {fresh}"
          f" ({'equal to' if fresh == recorded['candidate_manifest_sha256'] else 'differs from'}"
          f" the recorded candidate digest)")


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

    # The aggregate-digest derivation, on a manifest small enough to state by
    # hand: `=` resolves to the reference row, the order is by path, and the two
    # sides differ exactly where their digests do.
    rows, _ = read_manifest(
        MANIFEST_HEADER
        + "\nir/b.json\t2\t" + "cd" * 32 + "\t3\t" + "ef" * 32
        + "\nasm/a.s\t1\t" + "ab" * 32 + "\t=\t=\n"
    )
    expected_reference = (
        "asm/a.s\t1\t" + "ab" * 32 + "\nir/b.json\t2\t" + "cd" * 32 + "\n"
    )
    assert side_manifest_text(rows, "reference") == expected_reference
    assert side_manifest_text(rows, "candidate") == (
        "asm/a.s\t1\t" + "ab" * 32 + "\nir/b.json\t3\t" + "ef" * 32 + "\n"
    )
    assert manifest_digest(rows, "reference") == hashlib.sha256(
        expected_reference.encode("utf-8")
    ).hexdigest()
    assert manifest_digest(rows, "reference") != manifest_digest(rows, "candidate")
    # tree_manifest_digest is the same function of the same bytes, so a tree
    # written from a manifest hashes to that manifest's digest.
    with tempfile.TemporaryDirectory() as scratch:
        tree = Path(scratch)
        (tree / "asm").mkdir()
        (tree / "asm" / "a.s").write_bytes(b"x")
        assert tree_manifest_digest(tree) == hashlib.sha256(
            ("asm/a.s\t1\t" + hashlib.sha256(b"x").hexdigest() + "\n").encode("utf-8")
        ).hexdigest()

    try:
        parse_table("path\tonly\n", REMOVAL_HEADER, REMOVAL_TABLE.name)
    except ValueError:
        pass
    else:  # pragma: no cover - guarded by the assert below
        raise AssertionError("a bad table header must be rejected")
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
