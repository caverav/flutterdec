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

A digest is only a ruler while something recomputes it. Section 7 records a
sha256 for every protected path, and until this checker grew a digest pass no CI
lane ever recomputed one: a protected oracle could be gutted down to a one-line
stub that keeps nothing but its sentinel's name, and the compiled inventory below
would still be green because the sentinel is still there. So before any of the
Cargo work, this checker verifies section 7 itself. It parses every digest row of
that section - all five tables, not only the Oracle one - against a hardcoded
inventory of the paths that must be there, and requires the row set to match
exactly, every path to appear once, every digest to be 64 lowercase hex
characters, every path to be an existing regular file, and every file's sha256 to
equal its row. `scripts/check-oracle-inventory.py` is one of those rows, so this
file is verified by the pass it implements.

Run with no arguments from anywhere in the workspace. `--self-test` runs only the
unit checks of the parsers, the digest verifier, and the matcher, which the
default run also performs first.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

PROTOCOL = "docs/oracle-protocol-ir-cfg-emitter.md"

# The heading that opens section 7. The digest parser reads from it to the next
# `## ` heading, so a digest-shaped row anywhere else in the protocol - section
# 8's evidence, the adjudication records' before-and-after chains - is neither
# mistaken for a protected row nor able to stand in for a deleted one.
DIGEST_SECTION_HEADING = "## 7. Protected paths and digests"

# The sentence that opens the protocol's Oracle test files table. Only that table
# is parsed: the other section 7 tables are goldens, checkers, and fixtures, none
# of which any loader pulls into a test target. Kept byte-identical to the copy in
# `crates/flutterdec-decompiler/tests/provenance_audit.rs`.
ORACLE_TABLE_ANCHOR = "Oracle test files. Adding a case to one of these is expected work;"

# Every path section 7 protects, in the order the five tables list them. This is
# the ruler for the table, not a copy of it: parsing the protocol alone cannot
# notice a row someone deleted, because a deleted row leaves nothing behind to
# check. The two must match exactly in both directions, so adding or removing a
# protected path is a deliberate edit here as well as there - which is what makes
# it visible in review rather than silent.
PROTECTED_PATHS = (
    # Fixed reference emission artifacts.
    "crates/flutterdec-decompiler/testdata/golden/null_guard_compaction.dartpseudo",
    "crates/flutterdec-decompiler/testdata/golden/retry_loop_compaction.dartpseudo",
    "crates/flutterdec-decompiler/testdata/golden/structured_loop_emit.dartpseudo",
    # Checkers, scanners, and their plant tests. This file is one of them.
    "scripts/check-annotation-provenance.py",
    "scripts/check-candidate-whitelist.py",
    "scripts/check-oracle-inventory.py",
    "scripts/prov_cross_audit_reconcile.py",
    "scripts/prov_join_audit_check.py",
    "scripts/prov_join_audit_plant_test.py",
    "scripts/prov_join_output_anchor_check.py",
    "scripts/scan-annotation-safety.py",
    "scripts/scan_annotation_safety_plant_test.py",
    "scripts/scan-loop-entry-annotations.py",
    "scripts/annotation_boundary_corpus_check.py",
    "scripts/build-annotation-ledger.py",
    # Gate and harness scripts.
    "scripts/ci-check.sh",
    "scripts/test-suite.sh",
    "scripts/lint-python.sh",
    "scripts/lint-shell.sh",
    "scripts/real-golden.sh",
    "scripts/real-golden-matrix.sh",
    # Fixtures and sample data.
    "testdata/provenance/join-audit-sample.jsonl",
    "testdata/real-golden/profiles/sample/profile.env",
    # Oracle test files. These are also the rows `SENTINELS` maps, so each one is
    # proved twice: its bytes here, and its compilation below.
    "crates/flutterdec-decompiler/src/tests.rs",
    "crates/flutterdec-decompiler/tests/provenance_audit.rs",
    "crates/flutterdec-decompiler/tests/loop_entry_provenance_audit.rs",
    "crates/flutterdec-decompiler/src/tests/shared.rs",
    "crates/flutterdec-decompiler/src/tests/golden_and_parser.rs",
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack.rs",
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack/structuring.rs",
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack/order_totality.rs",
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack/join_capture.rs",
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack/annotation_caps.rs",
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack/omitted_path_and_stack.rs",
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack/call_and_loops.rs",
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack/call_annotations.rs",
    "crates/flutterdec-decompiler/src/tests/cfg_and_stack/dispatch_table.rs",
    "crates/flutterdec-decompiler/src/tests/compaction_and_aliasing.rs",
    "crates/flutterdec-decompiler/src/tests/compaction_and_aliasing/control_flow_compaction.rs",
    "crates/flutterdec-decompiler/src/tests/compaction_and_aliasing/alias_and_expr_cleanup.rs",
    "crates/flutterdec-decompiler/src/tests/emit_and_helpers.rs",
    "crates/flutterdec-decompiler/src/tests/emit_and_helpers/helper_inlining.rs",
    "crates/flutterdec-decompiler/src/tests/emit_and_helpers/annotation_literals.rs",
    "crates/flutterdec-decompiler/src/tests/emit_and_helpers/candidate_whitelist.rs",
    "crates/flutterdec-decompiler/src/tests/emit_and_helpers/readability_and_naming.rs",
    "crates/flutterdec-core/src/pipeline/runners/tests.rs",
    "crates/flutterdec-core/src/pipeline/symbol_map/tests.rs",
    "crates/flutterdec-ir/src/tests/control_effects.rs",
    "crates/flutterdec-ir/tests/branch_target_radix.rs",
    "crates/flutterdec-ir/src/validate/tests.rs",
    "crates/flutterdec-core/src/pipeline/quality/control_effect_tests.rs",
    "crates/flutterdec-core/src/pipeline/runners/split/identity_tests.rs",
    "crates/flutterdec-core/src/pipeline/runners/stubs/identity_tests.rs",
    "crates/flutterdec-decompiler/src/control_flow/regions/identity_boundary_tests.rs",
    "crates/flutterdec-decompiler/src/control_flow/relation_oracle.rs",
    "crates/flutterdec-decompiler/src/control_flow/emission_taxonomy_tests.rs",
    "crates/flutterdec-decompiler/src/control_flow/annotation_anchor_tests.rs",
    "crates/flutterdec-decompiler/src/line_identity_tests.rs",
    "crates/flutterdec-decompiler/tests/helper_syntax_boundaries.rs",
    "crates/flutterdec-decompiler/tests/rewrite_boundaries.rs",
    "crates/flutterdec-decompiler/tests/unmodelled_write_effects.rs",
    "crates/flutterdec-decompiler/tests/register_width_provenance.rs",
    "crates/flutterdec-decompiler/tests/atomic_rmw_effects.rs",
    "crates/flutterdec-decompiler/tests/annotation_anchor_identity.rs",
    "crates/flutterdec-decompiler/tests/provenance_accounting.rs",
    "crates/flutterdec-core/tests/pipeline_determinism.rs",
    "crates/flutterdec-decompiler/tests/arm64_control_effects.rs",
    "crates/flutterdec-decompiler/tests/cfg_identity.rs",
    "crates/flutterdec-decompiler/tests/dfs_loop_address_invariance.rs",
    "crates/flutterdec-decompiler/tests/entry_loop_state_merge.rs",
    "crates/flutterdec-decompiler/tests/block_ledger_contract.rs",
)

HEX = frozenset("0123456789abcdef")

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
    "ir-lib": {
        "package": "flutterdec-ir",
        "kind": "lib",
        "name": "flutterdec_ir",
        "select": ["-p", "flutterdec-ir", "--lib"],
    },
    "branch-target-radix": {
        "package": "flutterdec-ir",
        "kind": "test",
        "name": "branch_target_radix",
        "select": ["-p", "flutterdec-ir", "--test", "branch_target_radix"],
    },
    "arm64-control-effects": {
        "package": "flutterdec-decompiler",
        "kind": "test",
        "name": "arm64_control_effects",
        "select": ["-p", "flutterdec-decompiler", "--test", "arm64_control_effects"],
    },
    "cfg-identity": {
        "package": "flutterdec-decompiler",
        "kind": "test",
        "name": "cfg_identity",
        "select": ["-p", "flutterdec-decompiler", "--test", "cfg_identity"],
    },
    "dfs-loop-address-invariance": {
        "package": "flutterdec-decompiler",
        "kind": "test",
        "name": "dfs_loop_address_invariance",
        "select": [
            "-p",
            "flutterdec-decompiler",
            "--test",
            "dfs_loop_address_invariance",
        ],
    },
    "helper-syntax-boundaries": {
        "package": "flutterdec-decompiler",
        "kind": "test",
        "name": "helper_syntax_boundaries",
        "select": ["-p", "flutterdec-decompiler", "--test", "helper_syntax_boundaries"],
    },
    "rewrite-boundaries": {
        "package": "flutterdec-decompiler",
        "kind": "test",
        "name": "rewrite_boundaries",
        "select": ["-p", "flutterdec-decompiler", "--test", "rewrite_boundaries"],
    },
    "unmodelled-write-effects": {
        "package": "flutterdec-decompiler",
        "kind": "test",
        "name": "unmodelled_write_effects",
        "select": ["-p", "flutterdec-decompiler", "--test", "unmodelled_write_effects"],
    },
    "register-width-provenance": {
        "package": "flutterdec-decompiler",
        "kind": "test",
        "name": "register_width_provenance",
        "select": ["-p", "flutterdec-decompiler", "--test", "register_width_provenance"],
    },
    "atomic-rmw-effects": {
        "package": "flutterdec-decompiler",
        "kind": "test",
        "name": "atomic_rmw_effects",
        "select": ["-p", "flutterdec-decompiler", "--test", "atomic_rmw_effects"],
    },
    "annotation-anchor-identity": {
        "package": "flutterdec-decompiler",
        "kind": "test",
        "name": "annotation_anchor_identity",
        "select": ["-p", "flutterdec-decompiler", "--test", "annotation_anchor_identity"],
    },
    "provenance-accounting": {
        "package": "flutterdec-decompiler",
        "kind": "test",
        "name": "provenance_accounting",
        "select": ["-p", "flutterdec-decompiler", "--test", "provenance_accounting"],
    },
    "pipeline-determinism": {
        "package": "flutterdec-core",
        "kind": "test",
        "name": "pipeline_determinism",
        "select": ["-p", "flutterdec-core", "--test", "pipeline_determinism"],
    },
    "entry-loop-state-merge": {
        "package": "flutterdec-decompiler",
        "kind": "test",
        "name": "entry_loop_state_merge",
        "select": [
            "-p",
            "flutterdec-decompiler",
            "--test",
            "entry_loop_state_merge",
        ],
    },
    "block-ledger-contract": {
        "package": "flutterdec-decompiler",
        "kind": "test",
        "name": "block_ledger_contract",
        "select": [
            "-p",
            "flutterdec-decompiler",
            "--test",
            "block_ledger_contract",
        ],
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
    # The IR and CFG boundary oracles. Every one of them was an inline module in
    # product source until it was moved out into a file of its own: a digest can
    # only protect a file later work is not expected to edit, and each of these
    # rulers used to sit in `lib.rs`, `validate.rs`, `quality.rs`, `split.rs`,
    # `stubs.rs` or `regions.rs`, all of which ordinary work edits. The hook left
    # behind in each of those files is a `#[cfg(test)] #[path = ...]`
    # declaration, which is why these rows need the compiler and not a digest to
    # prove they run.
    "crates/flutterdec-ir/src/tests/control_effects.rs": (
        "ir-lib",
        "control_effect_tests::every_arm64_control_effect_has_exactly_the_documented_edges",
    ),
    "crates/flutterdec-ir/tests/branch_target_radix.rs": (
        "branch-target-radix",
        "public_cfg_distinguishes_decimal_and_explicit_hex_targets",
    ),
    "crates/flutterdec-ir/src/validate/tests.rs": (
        "ir-lib",
        "validate::tests::every_planted_identity_failure_is_named",
    ),
    "crates/flutterdec-core/src/pipeline/quality/control_effect_tests.rs": (
        "core-lib",
        "quality_control_effect_tests::serialized_ir_states_every_control_effect_and_its_edges",
    ),
    "crates/flutterdec-core/src/pipeline/runners/split/identity_tests.rs": (
        "core-lib",
        "runners_split::split_identity_tests::every_piece_of_every_split_shape_is_canonical",
    ),
    "crates/flutterdec-core/src/pipeline/runners/stubs/identity_tests.rs": (
        "core-lib",
        "runners_stubs::stubs_identity_tests::every_shape_the_prune_mutates_comes_out_canonical",
    ),
    "crates/flutterdec-decompiler/src/control_flow/regions/identity_boundary_tests.rs": (
        "decompiler-lib",
        "control_flow::identity_boundary_tests::"
        "every_planted_identity_failure_declines_before_any_relation_is_built",
    ),
    # Reached by an `include!` in `src/control_flow.rs`, a loader that also pulls
    # in five product modules, so its include count cannot be pinned the way the
    # pure test loaders' can. The sentinel is the twenty-process determinism
    # check, which no other file defines.
    "crates/flutterdec-decompiler/src/control_flow/relation_oracle.rs": (
        "decompiler-lib",
        "control_flow::relation_oracle::normalized_relations_are_identical_in_twenty_processes",
    ),
    "crates/flutterdec-decompiler/src/control_flow/emission_taxonomy_tests.rs": (
        "decompiler-lib",
        "control_flow::emission_taxonomy_tests::snapshot_and_restore_cover_every_mutable_state_family",
    ),
    "crates/flutterdec-decompiler/src/control_flow/annotation_anchor_tests.rs": (
        "decompiler-lib",
        "control_flow::annotation_anchor_tests::every_candidate_ends_with_a_recorded_outcome",
    ),
    "crates/flutterdec-decompiler/src/line_identity_tests.rs": (
        "decompiler-lib",
        "line_identity_tests::every_length_changing_helper_rejects_a_partial_identity_mismatch",
    ),
    "crates/flutterdec-decompiler/tests/helper_syntax_boundaries.rs": (
        "helper-syntax-boundaries",
        "recovered_text_inside_a_helper_body_never_moves_helper_structure",
    ),
    "crates/flutterdec-decompiler/tests/rewrite_boundaries.rs": (
        "rewrite-boundaries",
        "recovered_data_is_safe_and_disjoint_from_emitter_names",
    ),
    "crates/flutterdec-decompiler/tests/unmodelled_write_effects.rs": (
        "unmodelled-write-effects",
        "an_unmodelled_write_drops_the_binding_at_every_destination_width",
    ),
    "crates/flutterdec-decompiler/tests/register_width_provenance.rs": (
        "register-width-provenance",
        "an_x_produced_non_literal_is_unresolved_through_a_w_read",
    ),
    "crates/flutterdec-decompiler/tests/atomic_rmw_effects.rs": (
        "atomic-rmw-effects",
        "every_atomic_load_form_invalidates_its_second_operand",
    ),
    "crates/flutterdec-decompiler/tests/annotation_anchor_identity.rs": (
        "annotation-anchor-identity",
        "annotations_bind_their_own_line_and_the_reconciler_rejects_every_planted_defect",
    ),
    "crates/flutterdec-decompiler/tests/provenance_accounting.rs": (
        "provenance-accounting",
        "release_audit_accounts_for_accepted_and_rejection_only_streams",
    ),
    "crates/flutterdec-core/tests/pipeline_determinism.rs": (
        "pipeline-determinism",
        "the_whole_artifact_set_is_byte_identical_in_twenty_processes",
    ),
    # Integration tests, discovered by Cargo from `tests/`.
    "crates/flutterdec-decompiler/tests/arm64_control_effects.rs": (
        "arm64-control-effects",
        "both_emitters_render_the_same_control_effects",
    ),
    "crates/flutterdec-decompiler/tests/cfg_identity.rs": (
        "cfg-identity",
        "every_planted_identity_failure_emits_one_diagnostic_and_no_body",
    ),
    "crates/flutterdec-decompiler/tests/dfs_loop_address_invariance.rs": (
        "dfs-loop-address-invariance",
        "public_dfs_loop_artifacts_ignore_block_address_order",
    ),
    "crates/flutterdec-decompiler/tests/entry_loop_state_merge.rs": (
        "entry-loop-state-merge",
        "an_entry_loop_merges_the_implicit_path_with_every_back_edge",
    ),
    "crates/flutterdec-decompiler/tests/block_ledger_contract.rs": (
        "block-ledger-contract",
        "complete_partition_reconciles_and_plants_fail_closed",
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


def parse_digest_rows(protocol_text):
    """Every `| path | sha256 |` row of section 7, in order, as `(path, digest)`.

    Bounded to that one section: the protocol's later adjudication records quote
    before-and-after digests in the same table shape, and a row moved out of
    section 7 into one of them must read as a deleted row, not as a live one.
    """
    _, separator, after = protocol_text.partition(f"\n{DIGEST_SECTION_HEADING}\n")
    if not separator:
        raise ParseError(
            f"{PROTOCOL} must keep the section 7 heading verbatim: {DIGEST_SECTION_HEADING!r}"
        )
    section = after.split("\n## ")[0]
    rows = []
    for line in section.splitlines():
        if not line.startswith("| `"):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if len(cells) != 2:
            raise ParseError(f"{PROTOCOL} section 7 row is not two columns: {line!r}")
        rows.append(tuple(cell[1:-1] if cell[:1] == "`" == cell[-1:] else cell for cell in cells))
    return rows


def check_digests(root, rows, expected):
    """Section 7's digest tables against the worktree and against `expected`.

    Four independent ways a digest ruler stops ruling, all of them failures here:
    the row set drifted from `expected` in either direction, a path is listed
    twice so one of the two digests is unenforced, a digest is not a sha256 at
    all, or the file's bytes no longer hash to what the row says. The last is the
    one that catches an oracle emptied out into a stub that keeps only its
    sentinel: the compiled inventory below cannot see that, because the sentinel
    is exactly what such a stub preserves.
    """
    failures = []

    seen = {}
    for path, digest in rows:
        if path in seen:
            failures.append(
                f"{path} is listed twice in {PROTOCOL} section 7, as `{seen[path]}` and as "
                f"`{digest}`; a duplicated path leaves one of its two digests unenforced"
            )
        else:
            seen[path] = digest

    tabled = set(seen)
    wanted = set(expected)
    for path in sorted(wanted - tabled):
        failures.append(
            f"{path} is a protected path with no digest row in {PROTOCOL} section 7; deleting a "
            f"row is how a protected file stops being protected while every remaining row matches"
        )
    for path in sorted(tabled - wanted):
        failures.append(
            f"{path} has a digest row in {PROTOCOL} section 7 but is not in this checker's "
            f"protected inventory; the table and the inventory must name the same paths"
        )

    for path in sorted(tabled & wanted):
        digest = seen[path]
        if len(digest) != 64 or not set(digest) <= HEX:
            failures.append(
                f"{path} has digest `{digest}` in {PROTOCOL} section 7, which is not 64 lowercase "
                f"hex characters, so no file can ever match it"
            )
            continue
        target = root / path
        if not target.is_file():
            failures.append(
                f"{path} is protected by {PROTOCOL} section 7 but is not an existing regular file "
                f"in the worktree, so its digest protects nothing"
            )
            continue
        actual = hashlib.sha256(target.read_bytes()).hexdigest()
        if actual != digest:
            failures.append(
                f"{path} does not match its {PROTOCOL} section 7 digest: the table says `{digest}` "
                f"and the file hashes to `{actual}`. Changing a protected file is a ruler change "
                f"and needs a section 9 adjudication, not a new digest"
            )

    return failures


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


def digest_self_test():
    """Unit checks of the digest pass: the section-bounded parser, and each way a
    digest ruler can stop ruling."""
    body = (
        "## 6. Frozen fields\n"
        "\n"
        "| `crates/decoy/before.rs` | `" + "0" * 64 + "` |\n"
        "\n"
        f"{DIGEST_SECTION_HEADING}\n"
        "\n"
        "| Path | sha256 |\n"
        "| --- | --- |\n"
        "| `scripts/one.sh` | `{one}` |\n"
        "\n"
        "Oracle test files.\n"
        "\n"
        "| Path | sha256 |\n"
        "| --- | --- |\n"
        "| `crates/a/src/tests.rs` | `{two}` |\n"
        "\n"
        "## 8. Evidence\n"
        "\n"
        "| `crates/decoy/after.rs` | `" + "1" * 64 + "` |\n"
    )

    with tempfile.TemporaryDirectory() as scratch:
        root = Path(scratch)
        (root / "scripts").mkdir()
        (root / "crates/a/src").mkdir(parents=True)
        one = root / "scripts/one.sh"
        two = root / "crates/a/src/tests.rs"
        one.write_bytes(b"#!/usr/bin/env bash\ntrue\n")
        two.write_bytes(b"#[test]\nfn sentinel() {\n    assert_eq!(1 + 1, 2);\n}\n")
        one_digest = hashlib.sha256(one.read_bytes()).hexdigest()
        two_digest = hashlib.sha256(two.read_bytes()).hexdigest()
        expected = ("scripts/one.sh", "crates/a/src/tests.rs")

        def protocol(**changes):
            return body.format(**{"one": one_digest, "two": two_digest, **changes})

        # The parser takes every table of section 7 and nothing outside it: the
        # section 6 row above and the section 8 row below are the same shape.
        rows = parse_digest_rows(protocol())
        assert rows == [
            ("scripts/one.sh", one_digest),
            ("crates/a/src/tests.rs", two_digest),
        ], f"the parser must take exactly the section 7 digest rows, got {rows}"

        try:
            parse_digest_rows("## 7. Protected paths\n\n| `scripts/one.sh` | `beef` |\n")
        except ParseError:
            pass
        else:  # pragma: no cover - the assert below is the failure report
            raise AssertionError("a protocol without the section 7 heading must not parse")

        assert not check_digests(root, rows, expected), "the clean tree must pass"

        # A deleted row. Nothing is left in the table to notice it, which is why
        # the inventory above is hardcoded.
        deleted = [row for row in rows if row[0] != "scripts/one.sh"]
        failures = check_digests(root, deleted, expected)
        assert len(failures) == 1 and "no digest row" in failures[0], failures

        # An added row nothing here protects.
        failures = check_digests(root, rows + [("crates/a/src/extra.rs", "2" * 64)], expected)
        assert len(failures) == 1 and "protected inventory" in failures[0], failures

        # A duplicated path: the second digest is unenforced, so both are.
        failures = check_digests(root, rows + [("scripts/one.sh", "3" * 64)], expected)
        assert len(failures) == 1 and "listed twice" in failures[0], failures

        # Digests that no file can ever hash to: too short, and uppercase hex.
        for broken in ("dead", two_digest.upper()):
            failures = check_digests(root, [rows[0], ("crates/a/src/tests.rs", broken)], expected)
            assert len(failures) == 1, failures
            assert "64 lowercase hex characters" in failures[0], failures

        # A protected path that is not an existing regular file. A directory is
        # the interesting half: `Path.exists` would accept it.
        two.unlink()
        failures = check_digests(root, rows, expected)
        assert len(failures) == 1 and "not an existing regular file" in failures[0], failures
        two.mkdir()
        failures = check_digests(root, rows, expected)
        assert len(failures) == 1 and "not an existing regular file" in failures[0], failures
        two.rmdir()

        # A mutated protected file. The stub keeps the sentinel's name, so the
        # compiled inventory stays green and only the digest fires.
        two.write_bytes(b"#[test]\nfn sentinel() {}\n")
        failures = check_digests(root, rows, expected)
        assert len(failures) == 1 and "does not match its" in failures[0], failures
        assert two_digest in failures[0], failures

    print("[oracle-inventory] digest self-test ok")


def self_test():
    """Unit checks of the pieces that are not the compiler: the table parsers, the
    digest verifier, and the sentinel matcher."""
    digest_self_test()
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

    # The digest pass first, and it is fatal on its own. It costs no build, and
    # every answer the Cargo work below gives is worthless if the files it
    # compiles are not the files section 7 protects.
    digest_rows = parse_digest_rows(protocol_text)
    print(f"[oracle-inventory] {len(digest_rows)} digest rows in {PROTOCOL} section 7")
    digest_failures = check_digests(root, digest_rows, PROTECTED_PATHS)
    if digest_failures:
        print(f"[oracle-inventory] FAILED, {len(digest_failures)} digest problem(s):")
        for failure in digest_failures:
            print(f"  - {failure}")
        return 1
    print(
        f"[oracle-inventory] ok, {len(PROTECTED_PATHS)} protected paths match their section 7 "
        f"digests"
    )

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
