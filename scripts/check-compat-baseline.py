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
  fetch    download the pinned asset to a temporary sibling of `--dest`, fail
           unless size and SHA-256 match, and replace `--dest` only then, so no
           invocation ever writes or replaces `--dest` with unverified newly
           downloaded bytes, and none ever reuses an existing `--dest` without
           revalidating it first.
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
LOSS_HEADER = "function\treference_line\tcandidate_line\tadjudication_class\tguard_polarity"
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

# Whole-file presence of the two candidate markers; every removal row carries one.
MARKER_VOCABULARY = ("both", "indirect_branch_only", "trap_only", "none")

# The four-way split of the operand-direction losses. The vocabulary is fixed and
# the keys of difference-classes.json.operand_naming_fewer_register_classes are
# exactly these four, so a renamed class fails instead of adding a fifth bucket.
LOSS_CLASS_VOCABULARY = (
    "expression_replaced_by_the_register_holding_it",
    "line_replaced_by_structural_or_comment_line",
    "other",
    "register_named_as_parameter_slot",
)
LOSS_CLASS_DEFINITION = (
    "the four-way split of the fewer_registers rows is committed human adjudication of the two "
    "rendered lines, carried per row in the operand-direction-losses.tsv adjudication_class "
    "column; it is not a syntax metric inferred from the rows, so verify recounts the column "
    "against the recorded totals rather than re-deriving the class"
)

# The guard-polarity flag cuts across the four adjudication classes: three of the 78
# rows are a guard whose terminal comparison flips, and the flagged rows keep the
# adjudication class they already carried. Unlike adjudication_class this column is
# derived from the row bytes, so `verify` re-derives it instead of trusting it.
GUARD_POLARITY_FLAG = "reference_eq_zero_candidate_ne_zero"
GUARD_POLARITY_VOCABULARY = ("none", GUARD_POLARITY_FLAG)
GUARD_POLARITY_SITES = (
    "03119_sub_936128.dartpseudo",
    "05530_sub_c8b9b8.dartpseudo",
    "05548_sub_c93e0c.dartpseudo",
)
GUARD_POLARITY_CLASS = "expression_replaced_by_the_register_holding_it"
GUARD_POLARITY_DEFINITION = (
    "the operand-direction-losses.tsv column, derived from the two rendered lines and not "
    "adjudicated: reference_eq_zero_candidate_ne_zero on a row whose reference_line and "
    "candidate_line both start with 'if (' while the reference ends with '== 0) {' and the "
    "candidate with '!= 0) {', and none on every other row. It cuts across adjudication_class, "
    "which the flagged rows keep, and verify re-derives the column from the row bytes instead "
    "of trusting it. The flip is recorded and not accepted: no committed oracle proves which "
    "polarity is correct."
)
GUARD_POLARITY_OPEN_ITEM = (
    "The sibling guards suggest the candidate corrected the polarity, but nothing committed "
    "here is an independent semantic oracle over the machine code, so the flip is carried as "
    "an open semantic item and is not claimed as an accepted correction."
)

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
PRODUCT_TREE_DERIVATION = (
    "sha256 over one '<path>\\t<git blob object id>\\n' line per tracked file under "
    "product_tree_paths, paths in ascending byte order, no header; see "
    "check-compat-baseline.py product_tree_digest"
)

# The lost_edge_effects column is re-derivable only from this algorithm. The
# neighbouring readings of "that block's unreachable region" answer differently
# on the same trees: taking the region as every unreachable block in the file
# gives 350 and 90 against the recorded 303 and 68. `unreachable_regions` below
# is the same rule in code and `--self-test` runs it on a graph where the
# readings disagree, including one whose edge direction is reversed, which is
# what pins clause 2's undirected reading.
LOST_EDGE_ALGORITHM = (
    "The entry block is the candidate block whose start_va equals the function's entry_va, "
    "and a candidate block is unreachable when no directed path of successor edges reaches "
    "it from that entry block.",
    "The region of a candidate block is its weakly connected component in the subgraph "
    "induced by the unreachable blocks, taking a successor edge between two unreachable "
    "blocks as undirected.",
    "A lost reference edge bounds that region when its head address falls inside the "
    "component: the head resolves to the candidate block that starts at that address, or "
    "failing that to the candidate block holding an instruction at it.",
    "lost_edge_after_<op> is one occurrence per (file, wholly vanished callee, candidate "
    "block holding a Call to that callee, distinct candidate tail op among the lost edges "
    "bounding that block's region).",
    "disposition_<D> is one occurrence per (file, wholly vanished callee, unreachable "
    "candidate block holding a Call to that callee).",
    "dispatch_selector_rendering is one occurrence per (file, wholly vanished sel<N> "
    "callee), not one per lost rendering.",
)
LOST_EDGE_UNIT = (
    "the pseudocode-callee-removals.tsv column, an effect-occurrence count and not a count "
    "of distinct control-flow edges: its keys are keyed on the tuples in "
    "lost_edge_effects_algorithm, so one lost edge bounding a region that holds three "
    "vanished callees is counted three times. Distinct address-level lost edges are counted "
    "separately in callee-removals-aggregate.json distinct_lost_edges."
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


def prose(text):
    """Markdown prose flattened for verbatim anchor checks: no code ticks, no wrapping."""
    return " ".join(text.replace("`", "").split())


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


def tree_oid_digest(entries):
    """sha256 over one `<path>\\t<git blob oid>\\n` line per file, sorted by path."""
    return hashlib.sha256(
        "".join(f"{path}\t{oid}\n" for path, oid in sorted(entries)).encode("utf-8")
    ).hexdigest()


def product_tree_digest(rev, paths):
    """(digest, file count) of the product tree at `rev`, or None if git cannot say.

    The product tree is the part of the repository that decides the emitted bytes.
    Docs and evidence commits are outside it on purpose, which is what lets
    `revisions.candidate` stay pinned to the revision the artifacts came from
    while the record that describes them keeps moving forward.
    """
    result = subprocess.run(
        ["git", "-C", str(REPO), "ls-tree", "-r", rev, "--", *paths],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        return None
    entries = []
    for line in result.stdout.splitlines():
        meta, _, path = line.partition("\t")
        entries.append((path, meta.split()[2]))
    return tree_oid_digest(entries), len(entries)


def check_product_revision(recipe, failures):
    """`revisions.candidate` is the product state, so HEAD must still be it.

    `verify` used to accept any 40-hex string here, so the field could name a
    revision the working product no longer matched and nothing noticed. The
    pinned revision is never advanced to the docs commit that carries this
    evidence - a commit cannot contain its own hash, and moving the pin would
    change what the artifacts are claimed to come from. What is enforced instead
    is that the product tree has not moved since.
    """
    revisions = recipe["revisions"]
    if revisions["product_tree_derivation"] != PRODUCT_TREE_DERIVATION:
        failures.append("revisions.product_tree_derivation does not state the derivation used")
    if not revisions.get("candidate_role") or not revisions.get("evidence_provenance"):
        failures.append("revisions does not say what the candidate revision is, or where the "
                        "evidence revision is recorded instead")
    paths = revisions["product_tree_paths"]
    recorded = revisions["candidate_product_tree_sha256"]
    if not is_hex(recorded, HEX64):
        failures.append("revisions.candidate_product_tree_sha256 is not a sha256 digest")
        return
    at_candidate = product_tree_digest(revisions["candidate"], paths)
    if at_candidate is None:
        print("[compat-baseline] product-tree check skipped: git cannot read "
              f"{revisions['candidate'][:7]} from this checkout")
        return
    if at_candidate != (recorded, revisions["candidate_product_tree_files"]):
        failures.append(
            f"the product tree at revisions.candidate does not match what is recorded: "
            f"{at_candidate[0]} over {at_candidate[1]} files"
        )
    at_head = product_tree_digest("HEAD", paths)
    if at_head is not None and at_head[0] != recorded:
        failures.append(
            f"HEAD has a product-path delta from revisions.candidate "
            f"({revisions['candidate'][:7]}): the product tree hashes {at_head[0]}, so the "
            f"recorded artifacts were not produced by the product state at HEAD"
        )
    at_reference = product_tree_digest(revisions["reference"], paths)
    if at_reference is not None:
        if at_reference[0] != revisions["reference_product_tree_sha256"]:
            failures.append(
                f"the product tree at revisions.reference does not match what is recorded: "
                f"{at_reference[0]}"
            )
        if at_reference[0] == recorded:
            failures.append("the two sides share one product tree; there is nothing to compare")


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


def sum_by_class(rows, column):
    totals = {}
    for row in rows:
        totals[row[1]] = totals.get(row[1], 0) + int(row[column])
    return totals


def count_by(rows, column):
    counts = {}
    for row in rows:
        counts[row[column]] = counts.get(row[column], 0) + 1
    return counts


def check_reference_only_column(rows, aggregate, classes, failures):
    """Every row's reference-only IR count, not only the wholly vanished ones.

    The column was 0 on every `fewer_renderings_same_callees` row because the
    generator never measured them, and nothing caught it: the class sums were
    keyed only by the vanished classes. Reconciling per class, per nonzero row
    count and per value makes a single wrong row fail here.
    """
    column = aggregate["ir_reference_only_instructions"]
    by_class = sum_by_class(rows, 7)
    if by_class != aggregate["ir_reference_only_instructions_by_class"]:
        failures.append(
            "the per-row ir_reference_only_instructions do not sum to "
            f"ir_reference_only_instructions_by_class: {by_class}"
        )
    nonzero = [row for row in rows if int(row[7])]
    if count_by(nonzero, 1) != column["nonzero_rows_by_class"]:
        failures.append(
            "the rows carrying a nonzero ir_reference_only_instructions do not match "
            "ir_reference_only_instructions.nonzero_rows_by_class"
        )
    histogram = {}
    for row in nonzero:
        histogram[row[7]] = histogram.get(row[7], 0) + 1
    if histogram != column["value_histogram_over_nonzero_rows"]:
        failures.append(
            "the ir_reference_only_instructions values do not match the recorded histogram: "
            f"{histogram}"
        )
    if sum(by_class.values()) != column["in_removal_table_total"]:
        failures.append("ir_reference_only_instructions.in_removal_table_total does not match "
                        "the enumerated rows")
    if (column["in_no_callee_rendering_lost_files"]
            + column["in_files_whose_pseudocode_is_identical"]
            != column["outside_removal_table_total"]):
        failures.append("the reference-only instructions outside the removal table do not split")
    if (column["in_removal_table_total"] + column["outside_removal_table_total"]
            != column["all_5800_files_total"]):
        failures.append("the reference-only instructions do not partition the whole-run total")
    # Closing against 6.1 is the point: an instruction the record cannot place in
    # the after_trap:brk class would be an unadjudicated output difference.
    whole_run = classes["ir"]["instructions_only_in_reference"]
    if column["all_5800_files_total"] != sum(whole_run.values()):
        failures.append(
            f"the removal table accounts for {column['all_5800_files_total']} reference-only "
            f"instructions, difference-classes.json records {sum(whole_run.values())}"
        )
    if not column.get("column_scope"):
        failures.append("ir_reference_only_instructions does not state the column's scope")


def check_marker_column(rows, vanished_rows, aggregate, counts, failures):
    """`candidate_marker` is a file property and is recorded on every row."""
    marker = aggregate["candidate_marker"]
    for row in rows:
        if row[6] not in MARKER_VOCABULARY:
            failures.append(f"{row[0]} carries the candidate_marker {row[6]!r}, which is not one "
                            f"of {MARKER_VOCABULARY}")
            break
    if count_by(rows, 6) != marker["counts_over_all_removal_rows"]:
        failures.append("the candidate_marker column does not match the counts over all rows")
    over_vanished = count_by(vanished_rows, 6)
    if over_vanished != marker["counts_over_files_with_a_wholly_vanished_callee"]:
        failures.append("the candidate_marker column does not match the counts over the files "
                        "with a wholly vanished callee")
    for name in MARKER_VOCABULARY:
        recorded = counts.get(f"candidate_marker_{name}")
        if recorded is not None and over_vanished.get(name, 0) != recorded:
            failures.append(f"aggregate.candidate_marker_{name} is scoped to the wholly vanished "
                            f"files and disagrees with the rows: {over_vanished.get(name, 0)}")
    if not marker.get("scope"):
        failures.append("candidate_marker does not state the column's scope")


def unreachable_regions(blocks, entry_va):
    """LOST_EDGE_ALGORITHM clauses 1 and 2 in code: {block id: region id}.

    `blocks` is a candidate `ir/*.json` block list. Reachability is directed over
    `succs` from the entry block; a region is a weakly connected component of the
    subgraph the unreachable blocks induce, identified by its lowest block id.
    Reachable blocks are absent from the result: they have no region.
    """
    by_id = {b["id"]: b for b in blocks}
    entry = next((b["id"] for b in blocks if b["start_va"] == entry_va), None)
    live, stack = set(), [entry] if entry in by_id else []
    while stack:
        block = stack.pop()
        if block in by_id and block not in live:
            live.add(block)
            stack.extend(by_id[block]["succs"])
    dead = set(by_id) - live
    adjacent = {block: set() for block in dead}
    for block in dead:
        for succ in by_id[block]["succs"]:
            if succ in dead:
                adjacent[block].add(succ)
                adjacent[succ].add(block)
    region = {}
    for block in sorted(dead):
        if block in region:
            continue
        component, stack = set(), [block]
        while stack:
            reached = stack.pop()
            if reached not in component:
                component.add(reached)
                stack.extend(adjacent[reached])
        region.update(dict.fromkeys(component, block))
    return region


def check_lost_edge_definition(rows, aggregate, classes, doc, failures):
    """The unit and the algorithm must stand, verbatim, in all four files.

    A wrong reading of the region rule is undetectable from the totals alone -
    the column sums to itself either way - so the record's defence is that the
    definition cannot be dropped or reworded in one place without failing here.
    """
    effects = aggregate["lost_edge_effects"]
    algorithm = list(LOST_EDGE_ALGORITHM)
    if effects.get("unit") != LOST_EDGE_UNIT:
        failures.append("the lost_edge_effects unit in callee-removals-aggregate.json is "
                        "missing or altered")
    if effects.get("algorithm") != algorithm:
        failures.append("the lost_edge_effects algorithm in callee-removals-aggregate.json is "
                        "missing or altered")
    if classes["definitions"].get("lost_edge_effects") != LOST_EDGE_UNIT:
        failures.append("the lost_edge_effects unit in difference-classes.json is missing or "
                        "altered")
    if classes["definitions"].get("lost_edge_effects_algorithm") != algorithm:
        failures.append("the lost_edge_effects algorithm in difference-classes.json is missing "
                        "or altered")
    flowed = prose(doc)
    if prose(LOST_EDGE_UNIT) not in flowed:
        failures.append(f"the lost_edge_effects unit is missing or altered in {DOC.name}")
    for index, clause in enumerate(algorithm, start=1):
        if prose(clause) not in flowed:
            failures.append(f"clause {index} of the lost_edge_effects algorithm is missing or "
                            f"altered in {DOC.name}")
    # The selector key is the one whose unit is checkable from the committed rows:
    # the callee reading and the rendering reading are 34 against 370.
    check = effects["dispatch_selector_rendering_cross_check"]
    callees = renderings = 0
    files = set()
    for row in rows:
        detail = dict(item.rpartition(":")[::2] for item in row[10].split(";"))
        selectors = [name for name in row[9].split(";") if name.startswith("sel")]
        callees += len(selectors)
        renderings += sum(int(detail[name]) for name in selectors)
        if selectors:
            files.add(row[0])
    recounted = {"vanished_sel_callees": callees, "files": len(files),
                 "lost_sel_renderings_the_wrong_unit": renderings}
    if recounted != check:
        failures.append(f"the dispatch_selector_rendering cross-check does not match the rows: "
                        f"{recounted}")
    if effects["totals"]["dispatch_selector_rendering"] != callees:
        failures.append(
            f"dispatch_selector_rendering counts wholly vanished sel<N> callees, so it must be "
            f"{callees}, not {effects['totals']['dispatch_selector_rendering']}"
        )
    if callees == renderings:
        failures.append("the dispatch_selector_rendering cross-check cannot separate the callee "
                        "unit from the rendering unit")
    # The derivation can emit three further keys; the record claims none of them
    # occurred, and a key appearing later would be an unadjudicated effect.
    for key in effects["keys_that_are_zero_in_this_run"]:
        if key in effects["totals"]:
            failures.append(f"lost_edge_effects records {key} as absent from the run but the "
                            f"column carries it")


def check_lost_edge_effects(rows, aggregate, classes, ir_only, doc, failures):
    """The column counts effect occurrences; distinct edges are counted separately."""
    effects = aggregate["lost_edge_effects"]
    totals = {}
    for row in rows:
        for item in filter(None, row[8].split(";")):
            key, _, value = item.rpartition(":")
            totals[key] = totals.get(key, 0) + int(value)
    if totals != effects["totals"]:
        failures.append(f"the lost_edge_effects column does not sum to the recorded totals: "
                        f"{totals}")
    check_lost_edge_definition(rows, aggregate, classes, doc, failures)
    edges = aggregate["distinct_lost_edges"]
    for key, total in (("whole_file_by_candidate_tail_op", "whole_file_total"),
                       ("bounding_an_unreachable_region_by_candidate_tail_op",
                        "bounding_an_unreachable_region_total")):
        if sum(edges[key].values()) != edges[total]:
            failures.append(f"distinct_lost_edges.{total} does not match its per-op counts")
    # A lost edge whose tail is not in the candidate IR is a reference-only
    # instruction; if those two counts drift, one of them is wrong.
    absent = edges["whole_file_by_candidate_tail_op"].get("absent_from_candidate_ir", 0)
    if absent != ir_only.get("vanished_behind_trap"):
        failures.append(
            f"{absent} lost edges have a tail absent from the candidate IR but the trap class "
            f"records {ir_only.get('vanished_behind_trap')} reference-only instructions"
        )
    if sum(edges["whole_file_by_candidate_tail_op"].values()) < edges[
            "bounding_an_unreachable_region_total"]:
        failures.append("more lost edges bound an unreachable region than exist in those files")


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
    for row in rows:
        if int(row[7]) and row[1] in ("vanished_behind_indirect_branch",
                                      "dispatch_selector_rendering_only"):
            failures.append(f"{row[0]} lost reference IR instructions in class {row[1]}")
            break
    check_reference_only_column(rows, aggregate, classes, failures)
    check_marker_column(rows, vanished_rows, aggregate, counts, failures)
    check_lost_edge_effects(rows, aggregate, classes, ir_only, doc, failures)
    for name in by_class:
        if name not in doc:
            failures.append(f"removed-pseudocode class {name!r} is not adjudicated in {DOC.name}")
    files = {row[0] for row in rows}
    for name, representative in aggregate["representative_file_by_class"].items():
        if representative not in files:
            failures.append(f"the representative file for {name} is not in the removal table")
        if representative not in doc:
            failures.append(f"the representative file for {name} is not named in {DOC.name}")


def check_operand_loss_classes(classes, doc, failures):
    """Every one of the 78 operand-direction losses carries its own adjudication.

    The four class counts used to stand only as a prose table, with a three-column
    table underneath that could not be recounted, so a reader had to take the split
    on trust. The class is now a per-row column with a fixed vocabulary, and the
    row-level totals are what the recorded counts are checked against - which also
    means the record has to say out loud that the class is an adjudication of the
    two rendered lines and not a metric derived from them.
    """
    rows = read_table(LOSS_TABLE, LOSS_HEADER)
    recorded = classes["operand_naming_fewer_register_classes"]
    fewer = classes["operand_naming_direction"]["fewer_registers"]
    if tuple(sorted(recorded)) != LOSS_CLASS_VOCABULARY:
        failures.append(f"operand_naming_fewer_register_classes is not keyed on "
                        f"{LOSS_CLASS_VOCABULARY}: {tuple(sorted(recorded))}")
    for row in rows:
        if row[3] not in LOSS_CLASS_VOCABULARY:
            failures.append(f"{LOSS_TABLE.name} row {row[0]} carries the adjudication_class "
                            f"{row[3]!r}, which is not one of {LOSS_CLASS_VOCABULARY}")
            break
    counted = count_by(rows, 3)
    if counted != recorded:
        failures.append(f"the adjudication_class column does not sum to "
                        f"operand_naming_fewer_register_classes: {counted}")
    if len(rows) != fewer:
        failures.append(f"{LOSS_TABLE.name} has {len(rows)} rows, {fewer} are claimed")
    if sum(recorded.values()) != fewer:
        failures.append(f"the four operand-direction classes sum to {sum(recorded.values())}, "
                        f"not the {fewer} fewer_registers rows")
    if classes["definitions"].get("operand_naming_fewer_register_classes") != LOSS_CLASS_DEFINITION:
        failures.append("the operand_naming_fewer_register_classes definition in "
                        "difference-classes.json does not state that the split is a committed "
                        "adjudication recounted from the per-row column")
    if prose(LOSS_CLASS_DEFINITION) not in prose(doc):
        failures.append(f"the operand-direction adjudication is not declared as committed human "
                        f"adjudication in {DOC.name}")
    check_guard_polarity(rows, classes, doc, failures)


def guard_polarity_of(reference_line, candidate_line):
    """The flag, re-derived from the two rendered lines.

    A flagged row is a guard whose terminal comparison against zero flips from
    `== 0` to `!= 0`. The direction is one-way by construction: the reverse flip
    is a different claim and is counted separately so it cannot hide in `none`.
    """
    reference, candidate = reference_line.strip(), candidate_line.strip()
    if not (reference.startswith("if (") and candidate.startswith("if (")):
        return "none"
    if reference.endswith("== 0) {") and candidate.endswith("!= 0) {"):
        return GUARD_POLARITY_FLAG
    return "none"


def check_guard_polarity(rows, classes, doc, failures):
    """The three guard-polarity rows, re-derived rather than trusted.

    Three of the 78 rows are the same shape: the reference guard tests
    `... == 0` where the candidate tests `... != 0`, and the guarded body is the
    same two lines on both sides. That is a semantic difference the four operand
    classes do not describe, so it is carried as its own per-row column, the exact
    three sites are pinned here, and the record has to name them and carry them as
    an open item rather than as an accepted correction.
    """
    recorded = classes["operand_naming_guard_polarity"]
    for row in rows:
        if row[4] not in GUARD_POLARITY_VOCABULARY:
            failures.append(f"{LOSS_TABLE.name} row {row[0]} carries the guard_polarity "
                            f"{row[4]!r}, which is not one of {GUARD_POLARITY_VOCABULARY}")
            break
    derived = {row[0] for row in rows if guard_polarity_of(row[1], row[2]) == GUARD_POLARITY_FLAG}
    flagged = {row[0] for row in rows if row[4] == GUARD_POLARITY_FLAG}
    if derived != flagged:
        failures.append(f"the guard_polarity column does not match the direction re-derived from "
                        f"the rendered lines: column {sorted(flagged)}, rows {sorted(derived)}")
    if tuple(sorted(flagged)) != GUARD_POLARITY_SITES:
        failures.append(f"the guard_polarity sites are {tuple(sorted(flagged))}, not the pinned "
                        f"{GUARD_POLARITY_SITES}")
    if len(flagged) != 3:
        failures.append(f"{len(flagged)} rows carry {GUARD_POLARITY_FLAG}, 3 are claimed")
    reverse = [row[0] for row in rows
               if guard_polarity_of(row[2], row[1]) == GUARD_POLARITY_FLAG]
    if len(reverse) != recorded["reverse_direction_rows"]:
        failures.append(f"{len(reverse)} rows flip a guard the other way, "
                        f"{recorded['reverse_direction_rows']} are recorded: {reverse}")
    counted = count_by(rows, 4)
    if counted != recorded["counts"]:
        failures.append(f"the guard_polarity column does not sum to "
                        f"operand_naming_guard_polarity.counts: {counted}")
    if tuple(recorded["sites"]) != GUARD_POLARITY_SITES:
        failures.append(f"operand_naming_guard_polarity.sites is not the pinned "
                        f"{GUARD_POLARITY_SITES}")
    for row in rows:
        if row[4] == GUARD_POLARITY_FLAG and row[3] != GUARD_POLARITY_CLASS:
            failures.append(f"the guard-polarity row {row[0]} carries the adjudication_class "
                            f"{row[3]!r}; the flag is orthogonal and the class must stay "
                            f"{GUARD_POLARITY_CLASS}")
    siblings = recorded["sibling_guards"]
    if siblings["reference_eq_zero"] + siblings["reference_ne_zero"] != siblings["guards_per_side"]:
        failures.append("sibling_guards does not split guards_per_side on the reference side")
    if siblings["candidate_ne_zero"] != siblings["guards_per_side"]:
        failures.append("sibling_guards claims a candidate guard that is not rendered != 0")
    if siblings["reference_eq_zero"] != len(GUARD_POLARITY_SITES):
        failures.append("sibling_guards disagrees with the three pinned guard-polarity sites")
    inside = siblings["reference_ne_zero_in_flagged_files"]
    if tuple(sorted(inside)) != GUARD_POLARITY_SITES:
        failures.append("sibling_guards.reference_ne_zero_in_flagged_files is not keyed on the "
                        "three pinned guard-polarity sites")
    if sum(inside.values()) > siblings["reference_ne_zero"]:
        failures.append(f"sibling_guards puts {sum(inside.values())} of the reference's "
                        f"{siblings['reference_ne_zero']} != 0 guards inside the flagged files")
    if classes["definitions"].get("operand_naming_guard_polarity") != GUARD_POLARITY_DEFINITION:
        failures.append("the operand_naming_guard_polarity definition in difference-classes.json "
                        "does not state that the column is re-derived and the flip unaccepted")
    flat = prose(doc)
    if prose(siblings["reading"]) not in flat:
        failures.append(f"the sibling-guard reading behind the {siblings['guards_per_side']}/"
                        f"{siblings['files']} figures is missing or altered in {DOC.name}")
    for claim in (f"occurs {siblings['guards_per_side']} times in the same {siblings['files']} "
                  f"files",
                  f"renders {siblings['reference_ne_zero']} of them != 0 and only these "
                  f"{siblings['reference_eq_zero']} == 0",
                  f"renders all {siblings['candidate_ne_zero']} != 0"):
        if claim not in flat:
            failures.append(f"the sibling-guard count {claim!r} is not stated in {DOC.name}")
    if prose(GUARD_POLARITY_DEFINITION) not in flat:
        failures.append(f"the guard_polarity definition is missing or altered in {DOC.name}")
    if prose(GUARD_POLARITY_OPEN_ITEM) not in flat:
        failures.append(f"the guard-polarity flip is not carried as an open semantic item in "
                        f"{DOC.name}")
    for row in rows:
        if row[4] != GUARD_POLARITY_FLAG:
            continue
        if row[0] not in doc:
            failures.append(f"the guard-polarity site {row[0]} is not named in {DOC.name}")
        for line in (row[1], row[2]):
            if line.strip() not in doc:
                failures.append(f"the guard-polarity row for {row[0]} is not rendered verbatim "
                                f"in {DOC.name}: {line.strip()[:60]}...")


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
        by_shape = entry["excluded_by_comment_shape"]
        if sum(by_shape.values()) != counts["excluded_by_code_span_filter_total"]:
            failures.append(f"the {side} code-span exclusions are not split by comment shape")
        # `quality.rs` counts x0..x30 and reg0..reg30. That upper bound drops
        # nothing on this baseline, which is why the unbounded census regex and
        # the whole-line scope agree exactly instead of by luck.
        for key in ("regN_tokens_with_index_above_30", "xN_tokens_with_index_above_30",
                    "reg31_tokens", "x31_tokens"):
            if counts[key] != 0:
                failures.append(
                    f"the {side} text carries {counts[key]} {key}, so the quality.rs 0..=30 "
                    f"boundary drops tokens the census counts and the two scopes are not "
                    f"comparable"
                )
        if counts["highest_emitted_register_index"] > 30:
            failures.append(f"the {side} highest emitted register index is "
                            f"{counts['highest_emitted_register_index']}, past the counted range")
        if counts["census_regN_over_whole_text"] != counts["whole_line_scope_total"]:
            failures.append(f"the {side} census and whole-line scope disagree, which they cannot "
                            f"while no token sits past index 30")


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
    doc = DOC.read_text(encoding="utf-8")
    check_operand_loss_classes(classes, doc, failures)
    for name in (list(classes["ir"]["op_transitions"])
                 + list(classes["ir"]["branch_target_shape_transitions"])
                 + list(classes["ir"]["instructions_only_in_reference"])):
        if name not in doc:
            failures.append(f"difference class {name!r} is not adjudicated in {DOC.name}")
    # A line count nobody can reproduce is a claim. GNU diff -U0 and git diff -U0
    # group hunks differently and give different totals for the same two files.
    definitions = classes["definitions"]
    if "difflib.unified_diff" not in definitions["removed_and_added_lines"]:
        failures.append("difference-classes.json does not name the diff implementation the "
                        "removed and added line counts came from")
    if "difflib.unified_diff(n=0)" not in doc:
        failures.append(f"the diff implementation is not named in {DOC.name}")
    # Section 8 is meant to be runnable as written; on a host without flakes
    # enabled the build line fails before anything else is exercised.
    if recipe["toolchain"]["nix_config_export"] not in doc:
        failures.append(f"the NIX_CONFIG export the build line needs is not in {DOC.name}")
    check_adapter(recipe, doc, failures)
    check_removals(classes, doc, failures)
    check_register_scopes(failures)
    check_product_revision(recipe, failures)
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


def asset_state(path: Path, recipe):
    """((bytes, sha256), matches the recipe) for a file on disk."""
    payload = path.read_bytes()
    measured = (len(payload), hashlib.sha256(payload).hexdigest())
    return measured, measured == (recipe["bytes"], recipe["sha256"])


def curl_download(url, target: Path):
    subprocess.run(["curl", "-fsSL", "-o", str(target), url], check=True)


def fetch_verified(dest: Path, recipe, download=curl_download):
    """Download the pinned asset; never put unverified downloaded bytes at `dest`.

    The download used to go straight to `--dest` and be checked in place, so a
    truncated or re-cut asset stayed there after the failure and every rerun found
    that stale file first. The asset now lands on a temporary sibling, is checked
    there, and replaces `dest` only once size and SHA-256 both match; the
    temporary file is removed on every path out, including the curl failure.

    The guarantee is exactly that, and no more: an invalid file that was already at
    `dest` before the call is re-fetched rather than trusted, but it is only
    removed by a successful replace, so it survives a download that fails. What
    holds unconditionally is that every invocation revalidates whatever is at
    `dest` and never silently reuses it.
    """
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists():
        (size, digest), matches = asset_state(dest, recipe)
        if matches:
            print(f"[compat-baseline] verified {dest} ({size} bytes, sha256 {digest})")
            return
        print(f"[compat-baseline] {dest} does not match the recipe ({size} bytes, sha256 "
              f"{digest}); re-fetching")
    tmp = dest.with_name(dest.name + ".part")
    try:
        download(recipe["url"], tmp)
        (size, digest), matches = asset_state(tmp, recipe)
        if not matches:
            raise SystemExit(
                f"[compat-baseline] fetched bytes do not match the recipe: {size} {digest}"
            )
        tmp.replace(dest)
    finally:
        tmp.unlink(missing_ok=True)
    print(f"[compat-baseline] verified {dest} ({size} bytes, sha256 {digest})")


def fetch(dest: Path):
    fetch_verified(dest, json.loads(RECIPE.read_text(encoding="utf-8"))["input"])


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

    # The per-row reconciliations, on two hand-stated rows: one wrong value in
    # either column has to move a class sum or a marker count.
    table = [["a.dartpseudo", "fewer_renderings_same_callees", "2", "0", "9", "9",
              "trap_only", "2", "", "", "x:2"],
             ["b.dartpseudo", "vanished_behind_trap", "1", "1", "4", "4",
              "trap_only", "6", "disposition_RetainedUnreachable:1;lost_edge_after_Trap:2",
              "y", "y:1"]]
    assert sum_by_class(table, 7) == {"fewer_renderings_same_callees": 2,
                                      "vanished_behind_trap": 6}
    assert count_by(table, 6) == {"trap_only": 2}
    bumped = [list(table[0]), table[1]]
    bumped[0][7] = "3"
    assert sum_by_class(bumped, 7) != sum_by_class(table, 7)
    blanked = [list(table[0]), table[1]]
    blanked[0][6] = ""
    assert count_by(blanked, 6) != count_by(table, 6)
    assert "" not in MARKER_VOCABULARY

    # The product-tree derivation, on entries stated by hand: sorted by path,
    # one line each, and any object-id change moves the digest.
    entries = [("crates/b.rs", "b" * HEX40), ("Cargo.toml", "a" * HEX40)]
    assert tree_oid_digest(entries) == hashlib.sha256(
        (f"Cargo.toml\t{'a' * HEX40}\ncrates/b.rs\t{'b' * HEX40}\n").encode("utf-8")
    ).hexdigest()
    assert tree_oid_digest(entries) == tree_oid_digest(list(reversed(entries)))
    assert tree_oid_digest(entries) != tree_oid_digest(
        [("crates/b.rs", "c" * HEX40), ("Cargo.toml", "a" * HEX40)]
    )

    # The lost_edge_effects region rule, on a graph where the readings disagree:
    # 1 is the only reachable block besides the entry, 2 and 3 are one region
    # joined by an edge whose direction does not matter, and 4 is a second one.
    blocks = [{"id": 0, "start_va": 16, "succs": [1], "instrs": []},
              {"id": 1, "start_va": 20, "succs": [], "instrs": []},
              {"id": 2, "start_va": 24, "succs": [3], "instrs": []},
              {"id": 3, "start_va": 28, "succs": [], "instrs": []},
              {"id": 4, "start_va": 32, "succs": [], "instrs": []}]
    region = unreachable_regions(blocks, 16)
    assert set(region) == {2, 3, 4}, region  # reachable blocks have no region
    assert region[2] == region[3] != region[4], region
    assert len(set(region.values())) == 2, region  # not one region per file
    reversed_edge = [dict(block) for block in blocks]
    reversed_edge[2]["succs"], reversed_edge[3]["succs"] = [], [2]
    assert unreachable_regions(reversed_edge, 16) == region  # undirected, so 3 -> 2 is the same
    # An entry_va that names no block leaves every block unreachable, in one
    # region only where the successor edges connect them.
    orphaned = unreachable_regions(blocks, 99)
    assert set(orphaned) == {0, 1, 2, 3, 4} and len(set(orphaned.values())) == 3, orphaned

    # An edit that drops a load-bearing clause would let verify's anchors pass
    # against a record that no longer determines the recorded totals.
    stated = " ".join(LOST_EDGE_ALGORITHM)
    for phrase in ("start_va equals the function's entry_va", "no directed path of successor",
                   "weakly connected component", "as undirected",
                   "head address falls inside the component",
                   "distinct candidate tail op", "not one per lost rendering"):
        assert phrase in stated, phrase
    assert "not a count of distinct control-flow edges" in LOST_EDGE_UNIT
    assert prose("`a`  b\nc") == "a b c"

    # The operand-direction adjudication: a fixed, sorted, four-value vocabulary
    # recounted per row, and a definition that says the class is adjudicated.
    assert tuple(sorted(LOSS_CLASS_VOCABULARY)) == LOSS_CLASS_VOCABULARY
    assert len(set(LOSS_CLASS_VOCABULARY)) == 4
    losses = [["a", "-", "-", "other"], ["b", "-", "-", "other"],
              ["c", "-", "-", "register_named_as_parameter_slot"]]
    assert count_by(losses, 3) == {"other": 2, "register_named_as_parameter_slot": 1}
    for phrase in ("committed human adjudication", "adjudication_class",
                   "not a syntax metric inferred from the rows"):
        assert phrase in LOSS_CLASS_DEFINITION, phrase

    # The guard-polarity flag: a closed two-value vocabulary, three pinned sites,
    # and a direction re-derived from the row bytes in one direction only.
    assert tuple(sorted(GUARD_POLARITY_VOCABULARY)) == GUARD_POLARITY_VOCABULARY
    assert len(set(GUARD_POLARITY_VOCABULARY)) == 2 and "none" in GUARD_POLARITY_VOCABULARY
    assert len(GUARD_POLARITY_SITES) == 3 and tuple(sorted(GUARD_POLARITY_SITES)) == (
        "03119_sub_936128.dartpseudo",
        "05530_sub_c8b9b8.dartpseudo",
        "05548_sub_c93e0c.dartpseudo",
    ), GUARD_POLARITY_SITES
    guard_eq = "if ((((reg4 + reg3) + smiUntag(1)) & 0xc0000000) == 0) {"
    guard_ne = "if ((((reg4 + reg3) + reg3) & 0xc0000000) != 0) {"
    assert guard_polarity_of(guard_eq, guard_ne) == GUARD_POLARITY_FLAG
    assert guard_polarity_of(guard_ne, guard_eq) == "none"  # the reverse flip is not this class
    assert guard_polarity_of(guard_eq, guard_eq) == "none"  # an unchanged comparison is not
    assert guard_polarity_of(guard_ne, guard_ne) == "none"
    # `== 0` has to be the guard's own comparison, not one nested inside it
    assert guard_polarity_of("if ((x == 0) ? 1 : 0) {", guard_ne) == "none"
    assert guard_polarity_of("final t7 = f(a == 0);", "final t7 = f(a != 0);") == "none"
    assert guard_polarity_of("  " + guard_eq + "  ", "\t" + guard_ne) == GUARD_POLARITY_FLAG
    polarities = [["a", guard_eq, guard_ne, GUARD_POLARITY_CLASS, GUARD_POLARITY_FLAG],
                  ["b", "x;", "y;", "other", "none"]]
    assert count_by(polarities, 4) == {GUARD_POLARITY_FLAG: 1, "none": 1}
    for phrase in ("derived from the two rendered lines and not adjudicated",
                   "'== 0) {'", "'!= 0) {'", "cuts across adjudication_class",
                   "recorded and not accepted"):
        assert phrase in GUARD_POLARITY_DEFINITION, phrase
    for phrase in ("sibling guards suggest the candidate corrected the polarity",
                   "an independent semantic oracle over the machine code",
                   "open semantic item", "not claimed as an accepted correction"):
        assert phrase in GUARD_POLARITY_OPEN_ITEM, phrase

    # `fetch` must never leave an unverified asset at --dest: the download lands on
    # a temporary sibling, is checked there, and replaces the destination only on a
    # size and digest match. Each path below is one of the ways that can go wrong.
    payload = b"the pinned asset"
    recipe = {"url": "https://example.invalid/asset.apk", "bytes": len(payload),
              "sha256": hashlib.sha256(payload).hexdigest()}

    def good(url, target):
        target.write_bytes(payload)

    with tempfile.TemporaryDirectory() as scratch:
        dest = Path(scratch) / "nested" / "asset.apk"
        part = dest.with_name(dest.name + ".part")
        fetch_verified(dest, recipe, good)
        assert dest.read_bytes() == payload and not part.exists()

        def refuse(url, target):  # an asset that already verifies is not re-fetched
            raise AssertionError("a verified destination must not be downloaded again")

        fetch_verified(dest, recipe, refuse)

        bad = dest.with_name("bad.apk")
        try:
            fetch_verified(bad, recipe, lambda url, target: target.write_bytes(b"truncated"))
        except SystemExit as reason:
            assert "do not match the recipe" in str(reason), reason
        else:
            raise AssertionError("a digest mismatch must fail")
        # nothing at the destination, no temporary residue, and the good file kept
        assert not bad.exists() and not bad.with_name("bad.apk.part").exists()
        assert dest.read_bytes() == payload

        dest.write_bytes(b"stale")  # a rerun recovers instead of seeing the stale file
        fetch_verified(dest, recipe, good)
        assert dest.read_bytes() == payload and not part.exists()

        def die(url, target):
            target.write_bytes(b"half")
            raise subprocess.CalledProcessError(1, "curl")

        dead = dest.with_name("dead.apk")
        try:
            fetch_verified(dead, recipe, die)
        except subprocess.CalledProcessError:
            pass
        else:
            raise AssertionError("a failed download must propagate")
        assert not dead.exists() and not dead.with_name("dead.apk.part").exists()
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
