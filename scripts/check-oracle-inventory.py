#!/usr/bin/env python3
"""Prove that every protected oracle file is compiled into a real test target.

Section 7 of `docs/oracle-protocol-ir-cfg-emitter.md` protects a set of test
files by digest, and a digest proves only that a file's bytes are unchanged. It
says nothing about whether the compiler ever saw the file. Every one of those
files reaches a test binary through a hook - a `mod` declaration, an `include!`
line, or Cargo's automatic discovery of `tests/*.rs` - and none of the hooks can
be digested, because they live in product source and in manifests that ordinary
work has to edit.

The earlier guard asserted the hooks by matching their source text. That is not
an oracle: `/* /* */`, a leading `//`, `#[cfg(any())]`, an undeclared feature, or
a macro that swallows its argument all leave the literal bytes in place while
removing the item from compilation. This checker asks the compiler instead. For
each protected file it names one sentinel test that only exists if that file was
compiled, then lists the tests each target actually contains and requires every
sentinel to be there. Extra tests are always fine: adding cases is expected work.

Run with no arguments from anywhere in the workspace. `--self-test` runs only the
unit checks of the parser and the matcher, which the default run also performs
first.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

PROTOCOL = "docs/oracle-protocol-ir-cfg-emitter.md"

# The sentence that opens the protocol's Oracle test files table. Only that table
# is parsed: the other section 7 tables are goldens, checkers, and fixtures, none
# of which any loader pulls into a test target. Kept byte-identical to the copy in
# `crates/flutterdec-decompiler/tests/provenance_audit.rs`.
ORACLE_TABLE_ANCHOR = "Oracle test files. Adding a case to one of these is expected work;"

# The compiled test targets that hold the protected oracles: how Cargo names each
# one in `cargo metadata`, and how `cargo test` selects it.
#
# Both halves are needed. The metadata half catches a manifest that turns the
# target off with `test = false` or drops it with `autotests = false`, which an
# explicit `--lib` or `--test` selection would otherwise override or fail on
# without saying why. The selection half is what actually compiles the target and
# reports what is in it.
TARGETS = {
    "decompiler-lib": {
        "package": "flutterdec-decompiler",
        "kind": "lib",
        "name": "flutterdec_decompiler",
        "select": ["-p", "flutterdec-decompiler", "--lib"],
    },
    "core-lib": {
        "package": "flutterdec-core",
        "kind": "lib",
        "name": "flutterdec_core",
        "select": ["-p", "flutterdec-core", "--lib"],
    },
    "provenance-audit": {
        "package": "flutterdec-decompiler",
        "kind": "test",
        "name": "provenance_audit",
        "select": ["-p", "flutterdec-decompiler", "--test", "provenance_audit"],
    },
    "loop-entry-audit": {
        "package": "flutterdec-decompiler",
        "kind": "test",
        "name": "loop_entry_provenance_audit",
        "select": ["-p", "flutterdec-decompiler", "--test", "loop_entry_provenance_audit"],
    },
}

# One entry per row of the Oracle test files table: the target that must contain
# the sentinel, and the fully qualified name the harness lists it under.
#
# A row that holds tests of its own gets one of them. A row that holds no test -
# the four loaders and the shared helper file - gets a descendant that cannot
# compile without it: the loaders' sentinels are tests they include, and
# `shared.rs` gets the one test whose fixture is built from `branch_block` and
# `jump_block`, which only `shared.rs` defines. Every sentinel is distinct, so a
# failure names the row that lost its hook rather than a family.
SENTINELS = {
    # Top-level loader: five `include!` lines, no assertion of its own.
    "crates/flutterdec-decompiler/src/tests.rs": (
        "decompiler-lib",
        "tests::golden_structured_loop_emit_snapshot",
    ),
    # Integration tests, discovered by Cargo from `tests/`.
    "crates/flutterdec-decompiler/tests/provenance_audit.rs": (
        "provenance-audit",
        "the_pre_call_audit_traces_each_candidate_and_its_checker_catches_a_wrong_path",
    ),
    "crates/flutterdec-decompiler/tests/loop_entry_provenance_audit.rs": (
        "loop-entry-audit",
        "the_loop_entry_audit_traces_each_candidate_and_its_checker_catches_a_wrong_path",
    ),
    # Helpers only: `assert_golden`, `branch_block`, `jump_block`.
    "crates/flutterdec-decompiler/src/tests/shared.rs": (
        "decompiler-lib",
        "tests::emits_helper_bodies_for_omitted_paths",
    ),
    "crates/flutterdec-decompiler/src/tests/golden_and_parser.rs": (
        "decompiler-lib",
        "tests::golden_null_guard_compaction_snapshot",
    ),
    # Second-level loader: eight `include!` lines, no assertion of its own.
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack.rs": (
        "decompiler-lib",
        "tests::folds_movk_halves_into_the_selector_offset",
    ),
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack/structuring.rs": (
        "decompiler-lib",
        "tests::emits_a_join_block_exactly_once",
    ),
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack/order_totality.rs": (
        "decompiler-lib",
        "tests::candidate_order_is_total_over_every_permutation_of_its_input",
    ),
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack/join_capture.rs": (
        "decompiler-lib",
        "tests::captures_a_candidate_from_every_predecessor_of_a_three_predecessor_join",
    ),
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack/annotation_caps.rs": (
        "decompiler-lib",
        "tests::omits_the_whole_annotation_when_it_exceeds_the_per_annotation_budget",
    ),
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack/omitted_path_and_stack.rs": (
        "decompiler-lib",
        "tests::collapses_helper_calls_into_omitted_path_comments",
    ),
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack/call_and_loops.rs": (
        "decompiler-lib",
        "tests::emits_callable_style_for_generic_indirect_targets",
    ),
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack/call_annotations.rs": (
        "decompiler-lib",
        "tests::a_call_clobber_annotates_the_value_held_immediately_before_that_call",
    ),
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack/dispatch_table.rs": (
        "decompiler-lib",
        "tests::names_dispatch_table_calls_from_the_sub_encoding",
    ),
    # Second-level loader: two `include!` lines, no assertion of its own.
    "crates/flutterdec-decompiler/src/tests/compaction_and_aliasing.rs": (
        "decompiler-lib",
        "tests::collapses_if_else_with_identical_returns",
    ),
    "crates/flutterdec-decompiler/src/tests/compaction_and_aliasing/control_flow_compaction.rs": (
        "decompiler-lib",
        "tests::rewrites_empty_then_else_to_negated_if",
    ),
    "crates/flutterdec-decompiler/src/tests/compaction_and_aliasing/alias_and_expr_cleanup.rs": (
        "decompiler-lib",
        "tests::collapses_nested_guarded_returns_inside_if_body",
    ),
    # Second-level loader: four `include!` lines, no assertion of its own.
    "crates/flutterdec-decompiler/src/tests/emit_and_helpers.rs": (
        "decompiler-lib",
        "tests::no_annotation_consumer_hand_rolls_a_delimiter",
    ),
    "crates/flutterdec-decompiler/src/tests/emit_and_helpers/helper_inlining.rs": (
        "decompiler-lib",
        "tests::inlines_linear_helper_body_at_call_site",
    ),
    "crates/flutterdec-decompiler/src/tests/emit_and_helpers/annotation_literals.rs": (
        "decompiler-lib",
        "tests::each_annotation_literal_has_exactly_one_definition",
    ),
    "crates/flutterdec-decompiler/src/tests/emit_and_helpers/candidate_whitelist.rs": (
        "decompiler-lib",
        "tests::each_allowed_form_is_accepted_as_that_form",
    ),
    "crates/flutterdec-decompiler/src/tests/emit_and_helpers/readability_and_naming.rs": (
        "decompiler-lib",
        "tests::compacts_empty_else_and_duplicate_null_returns",
    ),
    "crates/flutterdec-core/src/pipeline/runners/tests.rs": (
        "core-lib",
        "runners_tests::aggregates_semantic_intent_counts_from_pseudocode",
    ),
    "crates/flutterdec-core/src/pipeline/symbol_map/tests.rs": (
        "core-lib",
        "tests::resolves_exact_before_nearest",
    ),
}


class ParseError(Exception):
    """The protocol table could not be read the way this checker expects."""


def parse_oracle_rows(protocol_text):
    """The backticked paths of exactly the Oracle test files table.

    The table runs from its anchor sentence to the end of section 7, so a row
    added to any other table in the protocol is not mistaken for an oracle, and a
    row added to this one is not missed.
    """
    anchor, separator, after = protocol_text.partition(ORACLE_TABLE_ANCHOR)
    if not separator:
        raise ParseError(
            f"{PROTOCOL} must keep the Oracle test files table anchor verbatim: "
            f"{ORACLE_TABLE_ANCHOR!r}"
        )
    del anchor
    section = after.split("\n## ")[0]
    rows = []
    for line in section.splitlines():
        if line.startswith("| `"):
            rows.append(line.split("`")[1])
    return rows


def check_inventory(rows, sentinels, listings, problems=None):
    """Every row needs a mapping, every mapping needs a row, every sentinel must
    have been compiled. Extra tests in a listing are never a failure.

    `listings` maps a target name to the set of tests that target reported, or to
    `None` when the target produced no usable listing. `problems` carries the
    reason for each such target, so a manifest that switched the target off is
    reported as that rather than as a missing test.
    """
    failures = []
    problems = problems or {}

    for row in rows:
        if not row.endswith(".rs"):
            failures.append(
                f"{row} is a non-Rust row in the Oracle test files table; this checker only "
                f"knows how Rust oracles are compiled, so extend it before adding that row"
            )

    tabled = set(rows)
    mapped = set(sentinels)
    for row in sorted(tabled - mapped):
        failures.append(
            f"{row} is protected by section 7 but has no sentinel here, so nothing proves it is "
            f"compiled; its digest would still match if the compiler never saw it"
        )
    for path in sorted(mapped - tabled):
        failures.append(
            f"{path} has a sentinel here but section 7 no longer protects it; the map and the "
            f"table must name the same files"
        )

    for target in sorted({target for target, _ in sentinels.values()}):
        if listings.get(target) is None:
            reason = problems.get(target, "it could not be listed")
            failures.append(
                f"target {target} holds protected oracles and {reason}, so none of them ran"
            )

    for path in sorted(tabled & mapped):
        target, sentinel = sentinels[path]
        listed = listings.get(target)
        if listed is None:
            continue
        if sentinel not in listed:
            failures.append(
                f"{path} is protected but not compiled: target {target} does not contain its "
                f"sentinel `{sentinel}`. The file's digest can still match while nothing it "
                f"asserts runs"
            )

    return failures


def workspace_root():
    """First ancestor of this script that holds the protocol."""
    for directory in Path(__file__).resolve().parents:
        if (directory / PROTOCOL).is_file():
            return directory
    raise ParseError(f"no ancestor of {__file__} holds {PROTOCOL}")


def testable_targets(root, cargo):
    """The `(package, kind, name)` of every target Cargo would build for tests.

    A target the manifest switched off with `test = false` is present here with
    `test` false, and one dropped by `autotests = false` is absent altogether.
    Neither shows up as a missing test in a listing: an explicit `--lib`
    selection compiles the target anyway, and an explicit `--test` selection
    fails without naming the manifest as the cause.
    """
    argv = [cargo, "metadata", "--no-deps", "--format-version", "1"]
    proc = subprocess.run(
        argv,
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        raise ParseError(f"`{' '.join(argv)}` exited {proc.returncode}")
    metadata = json.loads(proc.stdout)
    testable = set()
    for package in metadata["packages"]:
        for target in package["targets"]:
            if target["test"]:
                for kind in target["kind"]:
                    testable.add((package["name"], kind, target["name"]))
    return testable


def list_target(root, target, cargo):
    """The set of tests `target` reports, or `None` if it cannot be listed.

    `--list` builds the target, so a hook removed by a comment, a `cfg` that is
    never true, or a macro that swallows its argument shows up here as a missing
    test rather than as unchanged source text.
    """
    argv = [cargo, "test", *TARGETS[target]["select"], "--", "--list"]
    print(f"[oracle-inventory] {' '.join(argv)}")
    proc = subprocess.run(
        argv,
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        sys.stdout.write(proc.stdout)
        sys.stderr.write(proc.stderr)
        print(f"[oracle-inventory] {target} exited {proc.returncode}")
        return None
    listed = set()
    for line in proc.stdout.splitlines():
        name, separator, kind = line.rpartition(": ")
        if separator and kind == "test":
            listed.add(name)
    print(f"[oracle-inventory] {target} listed {len(listed)} tests")
    return listed


def hook_diagnostics(root, sentinels):
    """Non-fatal notes about files that are missing outright.

    Source-text observations are diagnostics here and nowhere else. They cannot
    be the oracle: the text of a hook says nothing about whether the item was
    compiled, which is the whole reason this checker exists.
    """
    notes = []
    for path in sorted(sentinels):
        if not (root / path).is_file():
            notes.append(f"{path} does not exist in the worktree")
    return notes


def self_test():
    """Unit checks of the two pieces that are not the compiler: the table parser
    and the sentinel matcher."""
    protocol = (
        "## 7. Protected paths and digests\n"
        "\n"
        "| Path | sha256 |\n"
        "| --- | --- |\n"
        "| `scripts/some-checker.py` | `dead` |\n"
        "\n"
        f"{ORACLE_TABLE_ANCHOR} weakening or\n"
        "removing an existing assertion is a ruler change.\n"
        "\n"
        "| Path | sha256 |\n"
        "| --- | --- |\n"
        "| `crates/a/src/tests.rs` | `beef` |\n"
        "| `crates/b/src/tests/one.rs` | `cafe` |\n"
        "\n"
        "## 8. Evidence\n"
        "\n"
        "| `crates/c/src/tests/after.rs` | `f00d` |\n"
    )
    rows = parse_oracle_rows(protocol)
    assert rows == [
        "crates/a/src/tests.rs",
        "crates/b/src/tests/one.rs",
    ], f"the parser must take exactly the Oracle table rows, got {rows}"

    try:
        parse_oracle_rows("## 7. Protected paths\n\n| `crates/a/src/tests.rs` | `beef` |\n")
    except ParseError:
        pass
    else:  # pragma: no cover - the assert below is the failure report
        raise AssertionError("a protocol without the anchor must not parse as zero rows")

    sentinels = {
        "crates/a/src/tests.rs": ("lib", "tests::one"),
        "crates/b/src/tests/one.rs": ("lib", "tests::two"),
    }

    # Extras are expected work, never a failure: adding a case to a protected
    # oracle grows the listing and must stay green.
    listings = {"lib": {"tests::one", "tests::two", "tests::added_later", "tests::and_another"}}
    failures = check_inventory(rows, sentinels, listings)
    assert not failures, f"extra tests must be allowed, got {failures}"

    # A sentinel that is not in the listing is the whole point.
    failures = check_inventory(rows, sentinels, {"lib": {"tests::one"}})
    assert len(failures) == 1, failures
    assert "crates/b/src/tests/one.rs" in failures[0], failures
    assert "tests::two" in failures[0], failures

    # A target with no usable listing is reported as the one root cause it is,
    # rather than passing over nothing because its listing is empty.
    failures = check_inventory(rows, sentinels, {"lib": None})
    assert len(failures) == 1 and "it could not be listed" in failures[0], failures

    # And when the manifest is the reason, the report says so instead of blaming
    # the tests.
    failures = check_inventory(
        rows, sentinels, {"lib": None}, {"lib": "its manifest sets `test = false`"}
    )
    assert len(failures) == 1 and "test = false" in failures[0], failures

    # Both directions of the table-to-map correspondence.
    failures = check_inventory(rows + ["crates/c/src/tests/new.rs"], sentinels, listings)
    assert len(failures) == 1 and "no sentinel here" in failures[0], failures
    failures = check_inventory(
        rows,
        dict(sentinels, **{"crates/c/src/tests/gone.rs": ("lib", "tests::three")}),
        listings,
    )
    assert len(failures) == 1 and "no longer protects it" in failures[0], failures

    # A non-Rust row is refused rather than silently unmapped.
    failures = check_inventory(rows + ["testdata/thing.json"], sentinels, listings)
    assert any("non-Rust row" in failure for failure in failures), failures

    print("[oracle-inventory] self-test ok")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run only the parser and matcher unit checks",
    )
    args = parser.parse_args(argv)

    self_test()
    if args.self_test:
        return 0

    root = workspace_root()
    protocol_text = (root / PROTOCOL).read_text(encoding="utf-8")
    rows = parse_oracle_rows(protocol_text)
    print(f"[oracle-inventory] {len(rows)} protected oracle rows in {PROTOCOL}")

    for note in hook_diagnostics(root, SENTINELS):
        print(f"[oracle-inventory] diagnostic: {note}")

    cargo = os.environ.get("CARGO", "cargo")
    needed = sorted({target for target, _ in SENTINELS.values()})
    testable = testable_targets(root, cargo)
    listings = {}
    problems = {}
    for target in needed:
        spec = TARGETS[target]
        key = (spec["package"], spec["kind"], spec["name"])
        if key not in testable:
            problems[target] = (
                f"{spec['package']}'s manifest no longer builds `{spec['kind']}` target "
                f"`{spec['name']}` for tests"
            )
            print(f"[oracle-inventory] {target}: {problems[target]}")
            listings[target] = None
            continue
        listings[target] = list_target(root, target, cargo)
        if listings[target] is None:
            problems[target] = "its listing invocation failed"

    for path in sorted(SENTINELS):
        target, sentinel = SENTINELS[path]
        listed = listings.get(target)
        state = "missing" if listed is None or sentinel not in listed else "compiled"
        print(f"[oracle-inventory] {state:8} {path} -> {target} :: {sentinel}")

    failures = check_inventory(rows, SENTINELS, listings, problems)
    if failures:
        print(f"[oracle-inventory] FAILED, {len(failures)} problem(s):")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print(f"[oracle-inventory] ok, {len(rows)} protected oracles are compiled")
    return 0


if __name__ == "__main__":
    sys.exit(main())
