# Validation target records

This index defines every validation identifier cited by public documentation.
Status reflects the committed evidence on this branch; it is not a substitute
for rerunning the named checks.

| Target | Definition | Status | Evidence |
| --- | --- | --- | --- |
| `VAL-IR-004` | Branch target radix is explicit; malformed or ambiguous operands do not invent a target. | Passed | [`IR parser`](../crates/flutterdec-ir/src/lib.rs), [`control_effects.rs`](../crates/flutterdec-ir/src/tests/control_effects.rs) |
| `VAL-CFG-005` | Loop headers and back edges follow dominance and active graph relations, independent of block addresses. | Passed | [`dfs_loop_address_invariance.rs`](../crates/flutterdec-decompiler/tests/dfs_loop_address_invariance.rs), [`oracle protocol`](oracle-protocol-ir-cfg-emitter.md) |
| `VAL-EMIT-015` | Entry loops merge the implicit function-entry path with every explicit back edge before reusing state. | Passed | [`entry_loop_state_merge.rs`](../crates/flutterdec-decompiler/tests/entry_loop_state_merge.rs), [`oracle protocol`](oracle-protocol-ir-cfg-emitter.md) |
| `VAL-ORACLE-005` | The block-ledger integration target is checksum-bound, compiled, and called by named local and GitHub CI lanes. | Passed | [`block_ledger_contract.rs`](../crates/flutterdec-decompiler/tests/block_ledger_contract.rs), [`oracle protocol`](oracle-protocol-ir-cfg-emitter.md), [`ci-check.sh`](../scripts/ci-check.sh) |
| `VAL-CI-001` | Local fail-closed guards have explicit GitHub CI counterparts using the Nix toolchain. | Passed | [`ci-check.sh`](../scripts/ci-check.sh), [`ci.yml`](../.github/workflows/ci.yml) |
| `VAL-COMPAT-001` | A public pinned APK reproduces reference and current artifacts with adjudicated differences and stable current output. | Passed | [`compatibility baseline`](compat-baseline-real-binary.md), [`compatibility evidence`](compat-evidence/) |
| `VAL-COMPAT-002` | Every behaviour-affecting difference in that baseline is corrected or proved from the ARM64 instruction spans, branch destinations and emitted pseudocode, with no open semantic item left recorded. | Passed | [`section 6.5`](compat-baseline-real-binary.md), [`semantic adjudication`](compat-evidence/semantic-adjudication.json), [`check-compat-semantics.py`](../scripts/check-compat-semantics.py), [`unresolved_cf_accounting.rs`](../crates/flutterdec-decompiler/tests/unresolved_cf_accounting.rs) |
| `VAL-METRIC-005` | Raw post-correctness performance evidence binds revisions, binaries, samples, chronology, resources, aggregation, and checksums without a skipped raw-data lane. | Passed | [`post-correctness evidence`](post-correctness/README.md), [`raw evidence checksums`](post-correctness/evidence/SHA256SUMS) |
