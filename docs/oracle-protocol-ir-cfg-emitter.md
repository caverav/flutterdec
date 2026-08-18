# Correctness protocol: IR, CFG, and emitter

This protocol is fixed before any product change. It ranks the evidence that may
decide whether a change to instruction classification, CFG construction, region
analysis, or emission is correct, and it names the rulers that a candidate is not
allowed to move.

Reference commit: `1371e42`. Branch: `research/ir-cfg-emitter`. The pipeline map
and the risk list this protocol tests against are in
[research-ir-cfg-emitter.md](research-ir-cfg-emitter.md).

One rule sits above the rest: candidate output cannot define its own expected
result. Every expected value in section 2 is either written by hand from an
external specification, or is a fixed reference artifact recorded before the
candidate existed and preserved unchanged afterwards.

## 1. Oracle hierarchy

Layers are ranked. A lower layer never overrides a higher one, and a higher layer
passing does not excuse a lower layer being unrun.

**L1. ARM64 control effect, for instruction classification.** Source: Arm
Architecture Reference Manual for A-profile architecture, `DDI 0487`, Part C,
Chapter C6, section C6.2, plus the Dart AOT specifics already cited in the
repository. The expected classification is written from the manual, never from
what the current classifier produces. Table in section 3.

**L2. Literal expected graph relations, for CFG and region analysis.** Each case
is a hand-built `FunctionIr` with the full expected relation set written out as
literals: reachable set, dominator sets, immediate post-dominator per block, join
set, natural loop bodies, loop follow, and the reducibility verdict. Expected
values are derived from the graph on paper. `Regions` is `pub(super)`
(`crates/flutterdec-decompiler/src/control_flow/regions.rs:17`), so the assertions
live inside the decompiler crate, next to
`crates/flutterdec-decompiler/src/tests/cfg_and_stack/`.

**L3. Fixed reference artifacts, for emission.** Two kinds:

- The three golden snapshots at
  `crates/flutterdec-decompiler/testdata/golden/`, compared by `assert_golden`
  (`crates/flutterdec-decompiler/src/tests/shared.rs:13-39`). Digests in
  section 7.
- Differential comparison of a whole artifact set produced from a deterministic
  synthetic input by the reference commit and by the candidate, normalized only
  by the frozen allowlist in section 6. The existing precedent for this shape is
  `scripts/real-golden.sh:213-234`, which diffs `quality.json`, a jq projection
  of `report.json`, and a named file list.

**L4. Planted failures, for provenance and for every checker.** A checker that
cannot be shown to fail on a planted violation is not evidence. The existing
plants and their observed results are recorded in section 8.

**L5. Nix CI, for integration.** `scripts/ci-check.sh` is the hard gate: `nix
flake check`, `cargo fmt --all --check`, `scripts/lint-shell.sh`,
`scripts/lint-python.sh`, `cargo clippy --workspace --all-targets -- -D
warnings`, `cargo test --workspace`, and a release build of the CLI. Note that
`.github/workflows/ci.yml` omits `scripts/lint-python.sh`, so the local parity
script is the authority for the python plant tests.

Outside the hierarchy, and not usable as an oracle here: recompiling the emitted
pseudocode. It is deliberately not source equivalent. Emitted line count is not a
quality measure either, and no promotion may rest on it.

## 2. Case matrix

Class: A adversarial, E edge, S stress, C cap. Status is the state at `1371e42`.
"Expected" is the invariant the case must prove, not a description of current
behavior.

### Instruction classification and block construction (L1)

| Case | Shape | Class | Expected | Status at `1371e42` |
| --- | --- | --- | --- | --- |
| IR-01 | `b` to a known target | E | block ends, one successor, no fallthrough | covered indirectly, `crates/flutterdec-ir/src/lib.rs:400-445` |
| IR-02 | `b.<cond>` | E | block ends, successors are target and fallthrough | covered, `ir/src/lib.rs:542-577` |
| IR-03 | `tbnz` with three operands | A | target parsed from the last operand token | covered, `ir/src/lib.rs:580-615` |
| IR-04 | `bl` and `blr` | E | call, block continues, fallthrough preserved | partially covered, `ir/src/lib.rs:400-445` asserts the elided case only |
| IR-05 | `br xN` | A | block ends, no fallthrough, no invented target | fails by inspection, risk R1 |
| IR-06 | `brk` | A | block ends, no successors | fails by inspection, risk R1 |
| IR-07 | `ret` followed by more code | E | block ends, next instruction is a leader | covered, `ir/src/lib.rs:522-539` |
| IR-08 | stack overflow guard group, several SDK offsets and both scratch registers | A | three `RuntimeCheck` ops, no call, no guard edge, slow path pruned | covered, `ir/src/lib.rs:400-489` |
| IR-09 | `cmp x15` against a non `THR` load | A | not recognized as the guard | covered, `ir/src/lib.rs:494-517` |
| IR-10 | duplicate `start_va` or duplicate ids in a constructed `FunctionIr` | A | rejected or exposed, never silently overwritten in a map | not covered |
| IR-11 | block unreachable for a reason other than the guard | A | retained and reported, not deleted | asserted only by the comment at `ir/src/lib.rs:291-299` |
| IR-12 | 1024 blocks, representative mix | S | successor and predecessor sets stay sorted, unique, and reciprocal | not covered |

### CFG and region analysis (L2)

Every case asserts the full literal relation set, not one relation.

| Case | Shape | Class | Expected | Status at `1371e42` |
| --- | --- | --- | --- | --- |
| CFG-01 | linear chain | E | reachable is all, each block post-dominates its predecessor, no loop, reducible | not covered directly |
| CFG-02 | diamond | E | follow of the branch is the join, join set is exactly the merge block | not covered directly |
| CFG-03 | fan in with three predecessors | A | `predecessors` ascending, `is_join` true, follow correct | predecessors covered indirectly, `src/tests/cfg_and_stack/join_capture.rs:69` |
| CFG-04 | nested natural loops | E | inner body subset of outer body, one header each | emission covered, `src/tests/cfg_and_stack/structuring.rs:538` |
| CFG-05 | loop with several exits | A | loop follow is the header immediate post-dominator outside the body (`regions.rs:322-331`) | not covered directly |
| CFG-06 | loop with no exit | E | loop follow is `None`, no panic, emission still terminates | not covered |
| CFG-07 | irreducible, two entries into one loop | A | `Regions::build` returns `None`, emitter declines and still emits | emission covered, `src/tests/cfg_and_stack/structuring.rs:586` |
| CFG-08 | unreachable block present | A | unreachable excluded from analysis, `reachable_count` unchanged by it, successor list cleared not deleted | not covered |
| CFG-09 | self loop | E | header is its own body member, back edge detected | not covered |
| CFG-10 | two exit blocks, equal sized post-dominator sets | A | immediate post-dominator identical across process hash seeds (`regions.rs:260-272`) | in process only, `src/tests/cfg_and_stack/order_totality.rs:221` |
| CFG-11 | 64, 256, and 1024 blocks, each shape above that scales | S | relations unchanged from the small case, within run limits | not covered |

### Emission (L3)

| Case | Shape | Class | Expected | Status at `1371e42` |
| --- | --- | --- | --- | --- |
| EM-01 | join reachable from two arms | E | emitted exactly once | covered, `structuring.rs:42` |
| EM-02 | irreducible graph | A | DFS fallback runs, body non empty, no structured provenance survives (`structured.rs:541-562`) | covered, `structuring.rs:586` |
| EM-03 | small shared region that is nobody's follow node | A | repeated within 16 blocks and 96 instructions, `repeated_blocks` incremented | covered, `structuring.rs:708` |
| EM-04 | arm that returns | A | its bindings do not leak past the branch | covered, `structuring.rs:617` |
| EM-05 | more than 64 distinct omitted blocks | C | every emitted `_block_N()` either resolves to a definition or is collapsed to an explicit omission that the summary comment names, and `quality.json` `block_helper_refs` is 0 | not covered, risk R2 |
| EM-06 | block visited past its visit limit (48, 24, 14 at `helper_flow/summary.rs:30-41`) | C | omitted path emitted, id recorded once | covered for the collapse half only, `src/tests/cfg_and_stack/omitted_path_and_stack.rs:2` |
| EM-07 | annotation at and one byte past the 3000 character line budget | C | whole annotation omitted, counted against its site | covered, `src/tests/cfg_and_stack/annotation_caps.rs:185` |
| EM-08 | nesting deeper than the structured depth cap of 64 (`structured.rs:624`) | C | decline, not truncation | not covered |
| EM-09 | same synthetic input, three separate processes | A | byte identical artifact set after section 6 normalization | not covered |
| EM-10 | function whose entry block has no instructions | E | last resort path at `crates/flutterdec-decompiler/src/lib.rs:451-468` does not double emit | not covered, unreachable on current samples |

### Provenance and integration (L4, L5)

| Case | Shape | Class | Expected | Status at `1371e42` |
| --- | --- | --- | --- | --- |
| PR-01 | pre-call audit, honest run | E | one annotation row per annotation, one snapshot per cited call, coordinate lands on the annotation | passes, section 8 |
| PR-02 | pre-call audit, value taken from the other call's snapshot | A | checker exits non zero, exactly one violation | passes, section 8 |
| PR-03 | join audit, wrong value and wrong snapshot id | A | checker exits non zero on each plant | passes, section 8 |
| PR-04 | annotation safety scan, four plant kinds | A | non zero for forbidden sequence, over cap, over span, unclosed | passes, section 8 |
| PR-05 | candidate whitelist and cross audit self tests | A | every planted violation detected | passes, section 8 |
| IN-01 | full local parity | E | `scripts/ci-check.sh` exits 0 | tests and build verified, section 8 |
| IN-02 | real binary golden | E | blocked: no committed input and no baseline | blocked, not passed |

## 3. ARM64 control effect table (L1 expected values)

Written from `DDI 0487` C6.2 and, for the guard group, from the Dart AOT shape
already documented at `crates/flutterdec-ir/src/lib.rs:19-32`. "Ends block" means
the following instruction must become a leader.

| Instruction | Architectural effect | Required class | Ends block | Required edges | Forbidden edges |
| --- | --- | --- | --- | --- | --- |
| `B label` | unconditional PC relative branch | jump | yes | the label block | fallthrough |
| `B.cond label` | conditional branch | branch | yes | label block and fallthrough | any third edge |
| `CBZ`, `CBNZ` | compare and branch on zero | branch | yes | label block and fallthrough | any third edge |
| `TBZ`, `TBNZ` | test bit and branch | branch | yes | label block and fallthrough | any third edge |
| `BL label` | branch with link, sets X30, returns | call | no | fallthrough | an edge to the callee |
| `BLR Xn` | indirect branch with link, sets X30, returns | call | no | fallthrough | an edge to a guessed callee |
| `BR Xn` | indirect branch, no link, does not return here | jump | yes | none unless a target set is independently recovered | fallthrough |
| `RET` | return to X30 | return | yes | none | fallthrough |
| `BRK #imm` | breakpoint instruction exception, control does not continue | trap | yes | none | fallthrough |
| Dart guard group `ldr` from `THR`, `cmp` against `SPREG`, `b.ls` | runtime stack limit check, slow path re-enters the body | runtime check | no | fallthrough only | the taken guard edge, and the slow path back edge |

Corroborating repository evidence for the two rows this pipeline currently gets
wrong: `crates/flutterdec-core/src/pipeline/runners/split.rs:141-150` already
treats `ret`, `brk`, `b`, and `br` as path enders, and
`crates/flutterdec-core/src/pipeline/runners/stubs.rs:461` reads `br` as the tail
of a dispatch stub.

## 4. Exact invariants

Numbered so that a test, a review, or a validator verdict can name one.

- I1. For every block, `succs` is sorted and duplicate free.
- I2. For every block, `preds` is sorted and duplicate free.
- I3. For every block `b` and every `s` in `b.succs`, `b.id` is in
  `blocks[s].preds`, and the converse.
- I4. Block ids are exactly `0..blocks.len()`, and the entry block has id 0.
- I5. No block whose last instruction is an unconditional jump, an indirect
  branch, a return, or a trap has a fallthrough successor.
- I6. A conditional branch block has exactly two successors: its target and its
  fallthrough, unless the two coincide.
- I7. `RuntimeCheck` instructions contribute no edge, and the guard slow path is
  the only block the prune removes.
- I8. Analysis relations are computed over entry-reachable blocks only, and the
  unreachable count is preserved rather than hidden.
- I9. Every relation `Regions` exposes is a pure function of the successor lists,
  independent of process hash seed.
- I10. On structured success, every reachable block appears exactly once in the
  body.
- I11. On structured decline, the emitter state is exactly what it was before the
  attempt: lines, register state, all counters, call anchors, join and loop
  provenance, snapshots, and the omitted block set.
- I12. Every `_block_N()` in a finished artifact resolves to an emitted
  `dynamic _block_N() {` definition, or is replaced by an explicit omission form
  whose id appears in the `// omitted complex paths:` summary. Equivalently,
  `quality.json` `block_helper_refs` is 0 for a run with no surviving helper.
- I13. Cap handling never produces an undefined reference and never silently
  drops a path without a marker.
- I14. Two runs of the same input in two separate processes produce byte
  identical artifacts, after normalizing only the fields in section 6.
- I15. Accepted, rejected, and unaccounted annotation candidates reconcile at
  every loss site, and each output coordinate carries at most one claim.
- I16. Text rewrites do not change literals, comments, operator precedence, or
  unrelated identifiers.

## 5. Zero tolerance rules

- A missing artifact, an empty body, or an absent audit row is a failure, never a
  pass by default.
- Any single failing case fails the whole set. An aggregate pass over cases does
  not survive one failing case, and a median or ratio may not absorb it.
- A test that never crosses the cap it claims to test does not count as coverage
  for that cap.
- No ruler may be edited to obtain a pass. Rulers are the paths in section 7.
- No expected value may be produced by running the candidate.
- A skipped case is reported as skipped with its reason. A blocked case, IN-02
  above, is reported as blocked and never as passed.
- `FLUTTERDEC_UPDATE_GOLDEN=1` is forbidden in every candidate and validation
  run. The only path to a changed golden is section 9.

## 6. Frozen volatile field allowlist

Named now, before any candidate run. Nothing may be added to this list after a
comparison has been attempted.

For full pipeline determinism and differential comparison, the only fields that
may be normalized are:

1. Absolute output paths, which vary with the chosen output directory. In
   `report.json`, the exact JSON pointers are:
   - `/input`
   - `/libapp`
   - `/adapter_selection/adapter_exec_path`
   - `/extra_symbol_elfs/*`
   - `/extra_symbol_map_targets/*`
   - `/engine_symbol_ingestion/manifest_path`
   - `/engine_symbol_ingestion/loaded_paths/*`
   - `/ghidra_script/path`
   - `/ida_script/path`
2. Measured wall clock fields. There are none in any artifact at `1371e42`: a
   search for `elapsed`, `duration`, `_ms`, `timestamp`, `generated_at`,
   `Instant::now`, and `SystemTime` across
   `crates/flutterdec-core/src/pipeline/*.rs` and
   `crates/flutterdec-cli/src/main.rs` finds nothing. The allowance therefore
   applies only to timing fields that the later benchmark harness emits into its
   own output, never into `quality.json` or `report.json`.

Everything else is compared verbatim. In particular `quality.json` has no path
or time field at all (`crates/flutterdec-core/src/lib.rs:248-275`), so it is
compared byte for byte.

## 7. Protected paths and digests

Recorded at `1371e42` with `sha256sum`. A change to any of these files is a
ruler change and requires section 9, whether or not a test still passes.

Every digest below is the current worktree value and is re-verified whenever this
table is touched. Two rows have moved since `1371e42`, and both have moved three
times. `scripts/ci-check.sh` is adjudicated in section 10, with its full chain
from the original fixed reference, its second move in section 12, and its third
in section 13. `crates/flutterdec-decompiler/tests/provenance_audit.rs` is
adjudicated in section 11, its second move in section 12, and its third in
section 13. One row is new rather than moved,
`scripts/check-oracle-inventory.py`, added by section 13. Every other row is
byte-identical to `1371e42`. A row that does not match the current worktree is a
failure of this table, not of the file.

Fixed reference emission artifacts:

| Path | sha256 |
| --- | --- |
| `crates/flutterdec-decompiler/testdata/golden/null_guard_compaction.dartpseudo` | `76d3d1e9b4d445d24c2f996487d163d6fcc22629e809c60bbc9be2aa39e73205` |
| `crates/flutterdec-decompiler/testdata/golden/retry_loop_compaction.dartpseudo` | `be8101c932a7c852d5da186ea19c73da580e72bed90aaec9fc0a5570cd710fc6` |
| `crates/flutterdec-decompiler/testdata/golden/structured_loop_emit.dartpseudo` | `1cfec48fee7129ecd47e92bfd0bbd9bed78f9e35d68dfef3da42256147bb7e87` |

Checkers, scanners, and their plant tests:

| Path | sha256 |
| --- | --- |
| `scripts/check-annotation-provenance.py` | `c4e40e0122f1d87c82b5b587d8ed1ac6c74f550bed114463765f2568ea6b6f93` |
| `scripts/check-candidate-whitelist.py` | `d8c67c8565c372c2044f6749bfe2a7b092a374c9758930c7e2ef5b45d3a6cac5` |
| `scripts/check-oracle-inventory.py` | `d882132e87cb4625ebdac88ab310e405b00133bd546e172db282be7e1bbf47bf` |
| `scripts/prov_cross_audit_reconcile.py` | `0633bf7191d62859efcbd35b9b62e186a39005e58ec49efaf24d8e03c6319c41` |
| `scripts/prov_join_audit_check.py` | `99a80ec27496b76737df08ae457838512495ec2e3e82668ac5ba5d73c1c5e995` |
| `scripts/prov_join_audit_plant_test.py` | `d3e9e885878db0b6e752ab421dd9bc851b6142f4a995307d2a7763029c88374a` |
| `scripts/prov_join_output_anchor_check.py` | `b015347d45986a59e2bc3a9af42689f244b3d83cb74e9dbd61ccdd352cec61b9` |
| `scripts/scan-annotation-safety.py` | `946f2a91a3c6df6c81707d455686dfe145b6fa05934391ce6d23933b7367a6c4` |
| `scripts/scan_annotation_safety_plant_test.py` | `2747be4387d23b9083d49668b8abc54ef91d2c63f472dc968287eb8530b8d698` |
| `scripts/scan-loop-entry-annotations.py` | `999753fbe0a59884c5fdaac48d4d8e207994323d9888e016219ef7398ab850b6` |
| `scripts/annotation_boundary_corpus_check.py` | `f925d0c53ffa173fb4d3fe7fc419006382eba69bec38db8c06289167ba187c03` |
| `scripts/build-annotation-ledger.py` | `6376d7dc6cfea2b35147ead30f5d7227b79f0c6c47381def5730e9fd65b1ef35` |

Gate and harness scripts:

| Path | sha256 |
| --- | --- |
| `scripts/ci-check.sh` | `386e0f2a22a25c774ff43da8621e947d9c3a4137e57a5d8ee6bbad973eb25c48` |
| `scripts/test-suite.sh` | `b1d2efd5cda5794dbb9e60c41f92eede0cc65996d66f6c73c19e905be451c38a` |
| `scripts/lint-python.sh` | `eef80907146b5d1b3d662ad823372a8b6a33df99b458582077b0c1578680e2d7` |
| `scripts/lint-shell.sh` | `4554f41d5dbeeadf4d2478ce97af416392b14a78cfa417673b35914877d316ab` |
| `scripts/real-golden.sh` | `89fce6baa6bb564d24535ebdc1b81d706db8b8e905572c1eb5ddae79644d8890` |
| `scripts/real-golden-matrix.sh` | `27a06d9ecfb8ccfeaee95f28df6f9fdcfc0ab3a28abd762d48968f8415116bc3` |

Fixtures and sample data:

| Path | sha256 |
| --- | --- |
| `testdata/provenance/join-audit-sample.jsonl` | `fecb3be22f1b405ef2e1494283036abd8332facad673bafb29acec808d46b299` |
| `testdata/real-golden/profiles/sample/profile.env` | `986535f4ad5b98e9e9d36caa54020527c7a0dbe1ef81a030815da0bc2ebef5c1` |

Oracle test files. Adding a case to one of these is expected work; weakening or
removing an existing assertion is a ruler change.

The first row is the loader, not a test. It holds no assertion of its own; it is
five `include!` lines that are the only thing pulling the five protected
in-crate oracle files into the compiled test target. Deleting one of those lines
silences a whole protected file while every other digest in this table still
matches, so the loader is protected too and a change to it is a ruler change on
everything it includes.

That loader is one of five hook families, and every row below depends on one of
them. None of the hooks can be digested here, because they all live in product
source or in manifests that later work must edit, so a whole-file digest for any
of them would fire on legitimate change and be worthless as a ruler.

They are proved by compilation instead. `scripts/check-oracle-inventory.py` parses
this table, maps every row to one sentinel test that exists only if that row was
compiled, lists what each protected test target actually contains, and fails if
any sentinel is absent. Extra tests are always allowed, because adding a case is
expected work. That checker is the correctness oracle for whether a protected
oracle runs at all, and `scripts/ci-check.sh` and `.github/workflows/ci.yml` each
run it as a lane of their own.

Matching the hooks' source text is not an oracle and this protocol does not treat
it as one: a leading `//`, a nested `/* */`, an added `#[cfg(any())]`, a feature
no manifest declares, or a macro that swallows its argument each leave a hook
byte-identical while removing the item from the build. Those observations survive
as diagnostics in `the_protected_oracle_loader_chain_is_intact`, in
`crates/flutterdec-decompiler/tests/provenance_audit.rs`, an integration test that
compiles as its own crate and so cannot be silenced by any loader it protects.
What that test still asserts is structural: every row has exactly one mapped hook
and every hook has a row, every mapped file exists, no loader has grown an
unrecorded `include!`, neither manifest disables a test target, and both CI lanes
really invoke the named integration targets and the inventory checker.

Section 13 records the compiled inventory, its 24 row-to-sentinel mappings, and
the twenty-two planted silencings that prove it fires. Section 12 records the
source-text guard it replaced, and section 11 that guard's narrower first
version.

The five families are `#[cfg(test)] mod tests;` in
`crates/flutterdec-decompiler/src/lib.rs`, the five `include!` lines in
`src/tests.rs`, the fourteen nested `include!` lines in the three second-level
loaders, the two `#[cfg(test)] #[path = ...]` module declarations in
`crates/flutterdec-core/src/pipeline/runners.rs` and
`crates/flutterdec-core/src/pipeline/symbol_map.rs`, and Cargo's automatic
discovery of `crates/flutterdec-decompiler/tests/*.rs`, which
`autotests = false` would switch off wholesale.

| Path | sha256 |
| --- | --- |
| `crates/flutterdec-decompiler/src/tests.rs` | `a19fe0015869fbfeb259e28f6d4344e18a630edab92b2a7aef2a58811e3ef56b` |
| `crates/flutterdec-decompiler/tests/provenance_audit.rs` | `1bda72504e7ada1c8a2e7798ca314b3843ebc6cf8b8202851de42dd542573abd` |
| `crates/flutterdec-decompiler/tests/loop_entry_provenance_audit.rs` | `02626ee1ba1b4b1b9905654a6254319ee413169341e43ddb74387813f7ecbfc7` |
| `crates/flutterdec-decompiler/src/tests/shared.rs` | `30ef9ef9d6b55acac8d41f5e557d38a78e5a60d2c28ac612e75ccfe80e376d3e` |
| `crates/flutterdec-decompiler/src/tests/golden_and_parser.rs` | `73a74b04ba294f1efc7faa5b067fdbd3c4cedc892c6d15068a07a98d656235ca` |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack.rs` | `1da9784956f09f2c5ede79236081389792d9f2abfac532abd39acfda0dc232c5` |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/structuring.rs` | `76bc84eabcda9bd9b34ee2d8b4ee21178782f0719077e7442670dfa4d1d32153` |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/order_totality.rs` | `690b389b5b83455902bdfa01855d8d70d3e58780d912f3990bbe0730346bdea9` |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/join_capture.rs` | `3d1db5b856a4cde4f98f57195b2828a4e3288003c20d0b0cd63a2812134d2b78` |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/annotation_caps.rs` | `242f6bb4a637dc466dca8be55a6e1671c6be90a7fb1d981a30b884103e2ae953` |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/omitted_path_and_stack.rs` | `e5e53a705aa16f6b27df6d99375da0d76106fc6f16f462301ab858d5e77a21ad` |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/call_and_loops.rs` | `2c2b433a07a8bab0d1a0adf4c09bc9f2982c3f74ecb997361b4729b1b3612630` |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/call_annotations.rs` | `53f90d25f39b9b93717b8ead2e19059f12e2ab72323d3af902d29c11f15f7a85` |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/dispatch_table.rs` | `72f5bc23927c4a9835ff92f4fc381d991b505d76f785c6b12b48ba7c2f816829` |
| `crates/flutterdec-decompiler/src/tests/compaction_and_aliasing.rs` | `69886810735687e26d2ce6b002e9301b1a7a53a3c9225c2eea74920944de1545` |
| `crates/flutterdec-decompiler/src/tests/compaction_and_aliasing/control_flow_compaction.rs` | `e30f62f0391bf3120ff8f95c8c06500bfa01b1d4a33822b9df3a41fa5019376a` |
| `crates/flutterdec-decompiler/src/tests/compaction_and_aliasing/alias_and_expr_cleanup.rs` | `b7ee514006c8b090378a221dfd437fa40f6ca49f509ee8f89e2475e4ad4bfdda` |
| `crates/flutterdec-decompiler/src/tests/emit_and_helpers.rs` | `8922dcf9d0f4fa207e7cb4a755fc5f34bec8ec0e3e499852beec0efc409718bf` |
| `crates/flutterdec-decompiler/src/tests/emit_and_helpers/helper_inlining.rs` | `3bfef133bf92c089a46ef29a31416eec5a6fd19e7e6764032a49d8ef4150a18a` |
| `crates/flutterdec-decompiler/src/tests/emit_and_helpers/annotation_literals.rs` | `4aa98c35997fbcc4d740d81abab1d1b3e5e8861c1f8b897d77fe1763514ecfa5` |
| `crates/flutterdec-decompiler/src/tests/emit_and_helpers/candidate_whitelist.rs` | `db3fdcef810f15ec61b349f15c88b2b82caad4fbcc130c83a41dc1f1bdae198b` |
| `crates/flutterdec-decompiler/src/tests/emit_and_helpers/readability_and_naming.rs` | `7a185febb042ee53b865b8561e7d68c091003fa1a5ef066cb8ac31a9c4639bd2` |
| `crates/flutterdec-core/src/pipeline/runners/tests.rs` | `a65298cde1ed807a838199162397bd51ff7f35e38941a0bd274872116b8c4668` |
| `crates/flutterdec-core/src/pipeline/symbol_map/tests.rs` | `019220e1a5915365e1663a36353ff3ba2177f567bb5e1094e6575b47f01b39f5` |

Threshold rulers, protected by value rather than by digest because they live in
files this mission may legitimately touch:

- `--max-placeholder-ifs` default 0, `crates/flutterdec-cli/src/main.rs:201-208`.
- `--max-unresolved-cf` default 0, `main.rs:209-216`.
- `--max-indirect-call-ratio` default 0.30, `main.rs:217-224`.
- `--min-disassembly-ratio` default 0.80, `main.rs:225-232`.
- The four gate comparisons, `crates/flutterdec-core/src/pipeline/quality.rs:106-118`.
- The six per line counters and their fixed order, `quality.rs:1-34`.

## 8. Evidence recorded at `1371e42`

Environment: every command below was run through
`nix --extra-experimental-features 'nix-command flakes' develop -c`, on branch
`research/ir-cfg-emitter` with a clean tree at `1371e42`.

Workspace tests: `cargo test --workspace` exits 0. 15 test binaries, 432 tests
passed, 0 failed.

Golden snapshots: `cargo test -p flutterdec-decompiler golden` passes
`golden_null_guard_compaction_snapshot`,
`golden_retry_loop_compaction_snapshot`, and
`golden_structured_loop_emit_snapshot`, 3 passed and 263 filtered out.

Planted failures, run individually:

- `cargo test -p flutterdec-decompiler --test provenance_audit`: 1 passed. The
  test itself runs the unmodified checker on an honest audit and requires
  success, then plants a value taken from the other call's snapshot and requires
  `violations snapshot 1` and `violations total 1`, then plants a swapped
  snapshot id and requires a non zero exit
  (`crates/flutterdec-decompiler/tests/provenance_audit.rs:185-252`).
- `cargo test -p flutterdec-decompiler --test loop_entry_provenance_audit`: 1
  passed.
- `python3 scripts/prov_join_audit_plant_test.py`: exit 0, `PASS`. Three observed
  outcomes: the clean audit reports `rc=0 join_annotations=2
  candidate_elements=5 violations=0`; the planted wrong value reports `rc=1` with
  `violations=1` and names `site=['join', 15] reg=x1` plus the snapshot
  `join:15:pred:13:5` that holds `x1='-1'` rather than `'1'`; the planted wrong
  snapshot id reports `rc=1` with `violations=1` and names snapshot
  `join:15:pred:14:4` as the end state of block 14 while the value is attributed
  to block 13.
- `python3 scripts/scan_annotation_safety_plant_test.py <sample>`: exit 0, with
  all four plants detected: `forbidden: found=1 rc=1`, `over_cap: found=1 rc=1`,
  `over_span: found=1 rc=1`, `unclosed: found=1 rc=1`.
- `python3 scripts/check-candidate-whitelist.py --self-test`: exit 0,
  `self-test: 25 planted violations, 10 allowed forms, 0 failures`.
- `python3 scripts/prov_cross_audit_reconcile.py --self-test`: exit 0,
  `self-test ok: clean fixture reconciles, 10 plants all detected`.

CLI reachability without a real input:
`cargo run -q -p flutterdec-cli -- decompile /nonexistent.apk -o /tmp/fdout`
prints `Error: open apk: /nonexistent.apk`, cause `No such file or directory (os
error 2)`, exit 1.

Blocked, and recorded as blocked: `scripts/real-golden.sh check` and
`scripts/real-golden-matrix.sh`. No committed APK or shared object, and
`testdata/real-golden/` contains no recorded `quality.json`,
`report_metrics.json`, or `files.txt`.

## 9. Adjudication path for an intentional output change

A change to a golden snapshot, a fixture, a checker, a threshold, or a recorded
reference is allowed only through all of these steps, in order:

1. State the invariant that makes the new output correct, sourced from L1 or L2,
   independent of the emitter that produced it. "The new output looks better" is
   not an invariant.
2. Add or point to the test that fails on the old behavior and passes on the new
   one, at the layer the change belongs to.
3. Record the full diff of the ruler in the commit message or in a research note,
   with the old and new digests.
4. Preserve the original reference. A superseded golden is kept, either under its
   own name alongside the replacement or as the recorded prior digest plus the
   diff, so a later reader can still compare against the pre change output. The
   reference commit `1371e42` is never rewritten, force pushed, or rebased.
5. Land the ruler change as its own commit, separate from the product change that
   motivates it, so that reverting one does not silently revert the other.
6. Re run L5 in full after the change.

A ruler change discovered without steps 1 through 4 is a failure of the change,
not of the ruler.

## 10. Adjudication record: `scripts/ci-check.sh`

This is the section 9 record for the one protected path whose digest has moved
since `1371e42`. It is landed as its own documentation commit, with no product or
harness change alongside it.

### 10.1 Digest chain

The column order below puts the state before the digest deliberately, so that a
scanner looking for the section 7 row shape, a backticked path followed by a
backticked digest, does not read these history rows as protected-path rows.

| Commit | State | `scripts/ci-check.sh` sha256 |
| --- | --- | --- |
| `1371e42` | fixed reference, preserved | `9d994285d4605f77f725c1d2ba5035b2ce0ef4802bb82d33df94153a15c6d50d` |
| `209a8fe` | unchanged, docs-only commit | `9d994285d4605f77f725c1d2ba5035b2ce0ef4802bb82d33df94153a15c6d50d` |
| `6430765` | unchanged, harness added but not wired into the gate | `9d994285d4605f77f725c1d2ba5035b2ce0ef4802bb82d33df94153a15c6d50d` |
| `1501bce` | intermediate | `675099447f611dcfc89cd26046ba6e6a7fd04f3ff94be54113e3c787ed21e412` |
| `1b11f7e` | intermediate | `6ee0cdf976f4fe02c1b3bebb4495bd2dfe34dc1fbd431b1ce9b52201eebbf878` |
| `b4b1d8c`, `3aa2fe4` | unchanged | `6ee0cdf976f4fe02c1b3bebb4495bd2dfe34dc1fbd431b1ce9b52201eebbf878` |
| `5aa4b4e` | current | `2f76a8b9abac96db026386c0626d248ade81e9690e563cbaaa901b86472b4457` |
| `8e7f080` through `43ef193` | unchanged, docs-only and harness-only commits | `2f76a8b9abac96db026386c0626d248ade81e9690e563cbaaa901b86472b4457` |
| this commit, worktree | current, recorded in section 7, adjudicated in section 12 | `171aa8894675ed2c90ff40c9d6a136bd791c3ae0d51c7965617e682b31d2f067` |

Reproduce any row with
`git show <commit>:scripts/ci-check.sh | sha256sum`, and the last row with
`sha256sum scripts/ci-check.sh`.

The fixed reference is preserved two ways: the `1371e42` digest is recorded above
and in section 7, and the file itself is recoverable verbatim from the reference
commit, which is never rewritten, force pushed, or rebased.

### 10.2 Exact diff intent, per step

`1371e42` to `1501bce`, digest `9d994285...` to `675099447f...`. Adds a clippy
lane and a test lane for the benchmark harness, plus the usage line and the
paragraph explaining why they are needed. From `1501bce` onward the harness is
deliberately not a workspace member, so `cargo clippy --workspace` and
`cargo test --workspace` do not reach it, and that exclusion is what keeps its
`bench-spans` instrumentation out of every existing check. Without these two
lanes the harness would be the one part of the repository no gate covers.

The qualifier is load bearing and the exclusion is not a property of the branch:
at `6430765` the harness was a workspace member and unification did turn
`bench-spans` on for product builds. That transient is disclosed in
[research-ir-cfg-emitter.md](research-ir-cfg-emitter.md) section 17, with the
interval, the probes and the semantic evidence. No accepted measurement is on
that revision.

`1501bce` to `1b11f7e`, digest `675099447f...` to `6ee0cdf976...`. Adds
`cargo fmt --manifest-path crates/flutterdec-bench/Cargo.toml --all --check` for
the same reason: `--all` means every member of the manifest's own workspace, so
the root `cargo fmt --all` does not reach the harness either. The usage line
changes from "clippy and tests" to "fmt, clippy and tests" to match.

`1b11f7e` to `5aa4b4e`, digest `6ee0cdf976...` to `2f76a8b9...`. Adds
`scripts/bench-identity-gate-test.sh` as a gate lane, before the clippy lane, and
renumbers the usage list from 5 through 8 to 6 through 9 to make room for it. The
identity gate is what stops an A/A run whose two sides are different machine code
and an A/B run whose two sides are the same machine code, and a 6 minute pipeline
run is the only other place it executes, so it gets a direct lane.

Whole-file diff against the fixed reference:
`git diff 1371e42 -- scripts/ci-check.sh`, 26 insertions and 3 deletions.

### 10.3 Proof that no gate was removed or weakened

Mechanical, over the whole file rather than over a summary of it. Strip blank and
comment lines from both versions, sort them, and take the set difference:

```
git show 1371e42:scripts/ci-check.sh > /tmp/cic-old.sh
strip() { grep -vE '^[[:space:]]*(#|$)' "$1" | sed 's/[[:space:]]*$//' | sort; }
comm -23 <(strip /tmp/cic-old.sh) <(strip scripts/ci-check.sh)
```

Three lines are reported, and all three are text inside the `usage()` heredoc:

```
  5) cargo clippy --workspace --all-targets -- -D warnings
  6) cargo test --workspace            (unless --skip-tests)
  7) cargo build -p flutterdec-cli --release
```

Each of the three reappears in the current file with the same command text and a
different list number, `6)`, `7)`, and `8)`, because the new identity-gate lane
took position 5. Nothing else present at `1371e42` is absent now: no executable
line, no `set -euo pipefail`, no argument handling, no `exit` path.

The executed check set is therefore a strict superset. At `1371e42`:

```
nix flake check
nix develop -c cargo fmt --all --check
nix develop -c ./scripts/lint-shell.sh
nix develop -c ./scripts/lint-python.sh
nix develop -c cargo clippy --workspace --all-targets -- -D warnings
nix develop -c cargo test --workspace                 (unless --skip-tests)
nix develop -c cargo build -p flutterdec-cli --release
```

Currently: the same seven, in the same order, plus

```
./scripts/bench-identity-gate-test.sh
nix develop -c cargo fmt --manifest-path "$bench_manifest" --all --check
nix develop -c cargo clippy --manifest-path "$bench_manifest" --all-targets -- -D warnings
nix develop -c cargo test --manifest-path "$bench_manifest"   (unless --skip-tests)
```

Strictness only increased. Every input that failed the gate at `1371e42` still
fails it, because every check that could reject it still runs, unchanged and in
the same order; four new ways to fail were added. No threshold moved, no lane
became conditional that was not conditional before, and the one flag that
suppresses work, `--skip-tests`, gained a lane rather than losing one. The nine
`section 7` threshold rulers, the golden digests, and the plant tests are
untouched by this diff, which changes no file other than
`scripts/ci-check.sh`.

### 10.4 Section 9 steps

1. Invariant. Not an L1 or L2 invariant, because no expected value and no product
   output changed: this is an L5 change, and the invariant is the one section 5
   states, that a ruler may never be edited to obtain a pass. It is satisfied in
   the strongest available form, set inclusion proved in 10.3: the gate cannot
   pass anything at the current digest that it rejected at `9d994285...`.
2. Test. `scripts/bench-identity-gate-test.sh`, 9 cases covering both directions
   of the identity rule and both usage errors, is itself the test the third step
   wires in. Run directly: exit 0, `[identity-gate-test] all checks passed`, 9 ok
   lines. It fails on the old behavior in the sense that matters for a gate: at
   `1371e42` the gate never ran it, so a broken identity gate passed CI.
3. Diff and digests. Recorded in 10.1 and 10.2, with the reproducing commands.
4. Original reference preserved. Recorded in 10.1.
5. Own commit. Not satisfied at the time: the three edits rode along inside
   harness commits `1501bce`, `1b11f7e`, and `5aa4b4e`, rather than landing as a
   separate ruler commit, and section 7 was not updated with them. That is the
   defect this record repairs. It is recorded as a deviation rather than
   explained away. The mitigation is that this adjudication is itself an atomic
   documentation commit, the chain in 10.1 lets any reader recover the exact
   pre-change ruler, and 10.3 bounds the blast radius: reverting any one of the
   three harness commits removes only lanes it added and cannot silently restore
   a weaker gate, because none of them weakened anything.
6. L5 re-run. `NIX_CONFIG='experimental-features = nix-command flakes'
   scripts/ci-check.sh` exits 0 at the current digest, all lanes green including
   the four added ones.

## 11. Adjudication record: `crates/flutterdec-decompiler/tests/provenance_audit.rs`

This is the section 9 record for the second protected path whose digest has moved
since `1371e42`. It is landed as its own commit, carrying only this file and the
protected test file, with no product change alongside it, and before any product
source edit of this mission.

### 11.1 Digest chain

Column order matches section 10.1, state before digest, so a scanner looking for
the section 7 row shape does not read these history rows as protected-path rows.

| Commit | State | `tests/provenance_audit.rs` sha256 |
| --- | --- | --- |
| `1371e42` | fixed reference, preserved | `e0b5c675b2510d8c17c05b15a4a33341ba6a24cbec4336512cf63028527ff3b8` |
| `209a8fe` through `e43b33d` | unchanged, docs-only and harness-only commits | `e0b5c675b2510d8c17c05b15a4a33341ba6a24cbec4336512cf63028527ff3b8` |
| this commit, worktree | current, recorded in section 7 | `8124346801612c56e9580d293c16a4e24593df175f8e7e376f16748a26560c0e` |

Reproduce any row with
`git show <commit>:crates/flutterdec-decompiler/tests/provenance_audit.rs | sha256sum`,
and the last row with
`sha256sum crates/flutterdec-decompiler/tests/provenance_audit.rs`.

The fixed reference is preserved two ways: the `1371e42` digest is recorded above,
and the file itself is recoverable verbatim from the reference commit, which is
never rewritten, force pushed, or rebased.

### 11.2 Exact diff intent

Purely additive. One new `#[test]`,
`the_protected_oracle_loader_chain_is_intact`, plus its doc comment, and a
four-line correction to the file's module comment, which previously said there was
exactly one test here and now says which of the two emits.

`git diff --numstat 1371e42 -- crates/flutterdec-decompiler/tests/provenance_audit.rs`
is 54 insertions and 3 deletions. The three deleted lines are the module-comment
sentence that is being corrected, replaced by four. The other 50 insertions are the
new test and its doc comment. No assertion, no fixture, no plant, and no `Command`
invocation of the existing test changed:
`the_pre_call_audit_traces_each_candidate_and_its_checker_catches_a_wrong_path` is
byte-identical to `1371e42` from its signature onward, which is what keeps the
section 8 evidence for it valid. Verify with

```
git show 1371e42:crates/flutterdec-decompiler/tests/provenance_audit.rs \
  | sed -n '/^fn the_pre_call_audit_traces/,$p' | sha256sum
sed -n '/^fn the_pre_call_audit_traces/,$p' \
  crates/flutterdec-decompiler/tests/provenance_audit.rs | sha256sum
```

Both print
`c76130ef412fde06ba1706e924e12fe0d85e802323c6e0d155a88cf6202e6d02`.

### 11.3 What the guard asserts

The exact strings, not a pattern that a rename or a reordering could satisfy by
accident:

- `src/lib.rs` contains `#[cfg(test)]\nmod tests;`, the hook verbatim including
  the newline between attribute and item, so an uncommented or re-attributed
  `mod tests;` does not pass.
- `src/tests.rs` contains all five of `include!("tests/shared.rs");`,
  `include!("tests/emit_and_helpers.rs");`,
  `include!("tests/cfg_and_stack.rs");`,
  `include!("tests/compaction_and_aliasing.rs");`, and
  `include!("tests/golden_and_parser.rs");`.
- `src/tests.rs` contains exactly five occurrences of `include!`, so the loader
  cannot grow a sixth path that this record does not name.

`src/lib.rs` is not added to the section 7 table. It is product source that later
tasks must edit, and a whole-file digest for it would fire on every legitimate
edit, which is the failure mode section 5 calls editing a ruler to obtain a pass,
arrived at from the other direction. The one line that matters is asserted instead
of the whole file.

### 11.4 Planted deletions, both loader levels

Run in disposable worktrees detached at `e43b33d` with the guard copied in, one
worktree per plant, each removed with `git worktree remove --force` afterwards.
`--lib` is the reduced unit-test suite; `--test provenance_audit` is the
independent guard.

| Plant | `--lib` result | Guard result |
| --- | --- | --- |
| none, current worktree | `ok`, 266 passed, 0 failed | `ok`, 2 passed, 0 failed |
| `#[cfg(test)] mod tests;` deleted from `src/lib.rs` | `ok`, 12 passed, 0 failed, exit 0 | `FAILED`, 1 passed 1 failed, exit 101 |
| `include!("tests/golden_and_parser.rs");` deleted from `src/tests.rs` | `ok`, 262 passed, 0 failed, exit 0 | `FAILED`, 1 passed 1 failed, exit 101 |

Both plants leave a unit-test suite that prints `test result: ok` and exits 0, and
that is the whole point: 266 tests fall to 12 or to 262 with no failure anywhere.
The first plant moves no digest in section 7 at all, because `src/lib.rs` has no
row there and the five included files are untouched, so before this guard existed
it was undetectable by the protocol. The second plant does move the
`src/tests.rs` row, so it was already detectable, but only by a reader
recomputing a documentary table; it is now also detectable by a test.

The failure messages name the level and the file:

```
/tmp/loader-probe-a/crates/flutterdec-decompiler/src/lib.rs must keep the
unit-test loader hook `#[cfg(test)] mod tests;` verbatim, or every in-crate
oracle is silenced while its digest still matches

/tmp/loader-probe-b/crates/flutterdec-decompiler/src/tests.rs must keep
`include!("tests/golden_and_parser.rs");`, or that protected oracle file is
never compiled
```

### 11.5 Proof the guard is not compiled under the loader it protects

`crates/flutterdec-decompiler/tests/` is an integration-test directory, so each
file there is its own crate root linking the library through
`use flutterdec_decompiler::...`. Neither of the two files there uses an
`include!` invocation or a `#[path]` attribute, the only `include!` text in the
directory being the guard's own expected-string literals, and `src/tests.rs` with
its five included files is `#[cfg(test)]` unit-test code that an integration crate
cannot reach. Plant A is the runtime
proof: with `mod tests;` gone the library's own test target drops to 12 tests and
still exits 0, while the guard in the separate target compiles unchanged and
fails.

### 11.6 Section 9 steps

1. Invariant. Not an L1 or L2 invariant, because no expected value and no product
   output changed. This is an L4 and L5 change and the invariant is the one
   section 5 states, that a protected oracle may not be silenced. It is satisfied
   in the form 11.4 demonstrates: both loader levels now have a mechanical
   detector that does not depend on the loader.
2. Test. `the_protected_oracle_loader_chain_is_intact` is itself the test. It
   fails on the old behavior in the sense that matters for a guard: the assertion
   did not exist before this commit, so deleting `mod tests;` passed every gate,
   including `scripts/ci-check.sh`, with every digest in this section 7 table
   matching.
3. Diff and digests. Recorded in 11.1 and 11.2, with the reproducing commands.
4. Original reference preserved. Recorded in 11.1.
5. Own commit. Satisfied. This record and the protected test file land together as
   one commit, `test(oracle): protect decompiler test loader chain`, with no
   product source change in it and before any product source edit of this
   mission.
6. L5 re-run. `NIX_CONFIG='experimental-features = nix-command flakes'
   scripts/ci-check.sh` exits 0 at the current digest.

## 12. Adjudication record: the whole oracle loader class

Section 11 protected two hooks: the decompiler's `mod tests;` line and its five
`include!` lines. That left most of the section 7 oracle table resting on hooks
nothing checked. This record closes the class: every Rust row in that table is now
mapped to the hook that compiles it, and the mapping is compared against the table
itself, so a new row without a hook fails.

Two protected paths move here, `crates/flutterdec-decompiler/tests/provenance_audit.rs`
and `scripts/ci-check.sh`, plus one unprotected CI lane,
`.github/workflows/ci.yml`. No product source, no manifest, and no oracle
assertion changes in this commit.

### 12.1 Digest chains

Column order matches sections 10.1 and 11.1, state before digest, so a scanner
looking for the section 7 row shape does not read these history rows as
protected-path rows.

`crates/flutterdec-decompiler/tests/provenance_audit.rs`:

| Commit | State | sha256 |
| --- | --- | --- |
| `1371e42` | fixed reference, preserved | `e0b5c675b2510d8c17c05b15a4a33341ba6a24cbec4336512cf63028527ff3b8` |
| `209a8fe` through `e43b33d` | unchanged | `e0b5c675b2510d8c17c05b15a4a33341ba6a24cbec4336512cf63028527ff3b8` |
| `43ef193` | first, narrow guard, adjudicated in section 11 | `8124346801612c56e9580d293c16a4e24593df175f8e7e376f16748a26560c0e` |
| `0fadd6e`, that commit's worktree | superseded, adjudicated in section 13 | `b5712a66b0f6472a726d4d253555272b54322cd69c3fdb1a3bed514de8cb9765` |

`scripts/ci-check.sh`, continuing the chain in section 10.1:

| Commit | State | sha256 |
| --- | --- | --- |
| `5aa4b4e` through `43ef193` | prior, adjudicated in section 10 | `2f76a8b9abac96db026386c0626d248ade81e9690e563cbaaa901b86472b4457` |
| `0fadd6e`, that commit's worktree | superseded, adjudicated in section 13 | `171aa8894675ed2c90ff40c9d6a136bd791c3ae0d51c7965617e682b31d2f067` |

`.github/workflows/ci.yml` is not a section 7 row and does not become one: it is CI
configuration that later work may legitimately edit, exactly like the threshold
rulers at the end of section 7. Recorded for reproducibility only:
`817f472151aa1553e2a25014bad95cf4418aca116cdfb7ca1fa9f2e9d6599a3c` at `1371e42`
through `43ef193`, `c51642b043cfa254c454282aee5d14b24d899d29bcda83d8e42a9e1da9968c55`
in this commit. Its one load-bearing line is asserted by value by the guard, not
by digest.

Reproduce any row with `git show <commit>:<path> | sha256sum`, and the last row of
each chain with `sha256sum <path>`. The fixed reference is preserved two ways: the
`1371e42` digests are recorded above and in section 7, and both files are
recoverable verbatim from the reference commit, which is never rewritten, force
pushed, or rebased.

### 12.2 Exact diff intent

`git diff --numstat 43ef193` for the three files: 322 insertions and 37 deletions
in `tests/provenance_audit.rs`, 14 insertions and 3 deletions in
`scripts/ci-check.sh`, 5 insertions and 0 deletions in
`.github/workflows/ci.yml`.

The 37 deleted lines in the guard are the section 11 guard's own body and doc
comment. They are replaced, not weakened: the same three assertions survive in
generalized form, and the generalization is what makes them cover 24 rows instead
of 6.

| Section 11 assertion | Where it lives now |
| --- | --- |
| `src/lib.rs` contains `#[cfg(test)]\nmod tests;` | the `Hook::Module` row for `src/tests.rs`, same literal including the newline |
| `src/tests.rs` contains each of five `include!` lines | five `Hook::Include` rows, each expected line derived as `include!("<path relative to the loader's own directory>");` |
| `src/tests.rs` contains exactly five `include!` occurrences | the per-loader exact-count check, whose expected count is the number of mapped rows for that loader, so it is now enforced for four loaders rather than one |

Empirically additive too: both section 11.4 plants were re-run against this guard
and both still fail it, rows 2 and 3 of 12.4.

The existing audit test is still byte-identical to `1371e42` from its signature
onward, which is what keeps the section 8 evidence for it valid:

```
git show 1371e42:crates/flutterdec-decompiler/tests/provenance_audit.rs \
  | sed -n '/^fn the_pre_call_audit_traces/,$p' | sha256sum
sed -n '/^fn the_pre_call_audit_traces/,$p' \
  crates/flutterdec-decompiler/tests/provenance_audit.rs | sha256sum
```

Both still print
`c76130ef412fde06ba1706e924e12fe0d85e802323c6e0d155a88cf6202e6d02`.

`scripts/ci-check.sh` gains one lane at position 7, the two decompiler
integration test targets named explicitly, plus the usage renumbering and the
paragraph explaining why `cargo test --workspace` cannot stand in for it. The
same mechanical proof section 10.3 uses, over the whole file:

```
git show 43ef193:scripts/ci-check.sh > /tmp/cic-prev.sh
strip() { grep -vE '^[[:space:]]*(#|$)' "$1" | sed 's/[[:space:]]*$//' | sort; }
comm -23 <(strip /tmp/cic-prev.sh) <(strip scripts/ci-check.sh)
```

Three lines are reported, and all three are text inside the `usage()` heredoc:

```
  7) cargo test --workspace            (unless --skip-tests)
  8) cargo build -p flutterdec-cli --release
  9) fmt, clippy and tests for the excluded benchmark harness
```

Each reappears with the same command text and a different list number, `8)`, `9)`,
and `10)`, because the new lane took position 7. Nothing executable present at
`43ef193` is absent now, and the same command run against `1371e42` reports only
the three renumbered usage lines section 10.3 already accounts for. The executed
check set is a strict superset of both: one added command,
`nix develop -c cargo test -p flutterdec-decompiler --test provenance_audit
--test loop_entry_provenance_audit`, and nothing removed, reordered, or made
conditional. The new lane deliberately runs even under `--skip-tests`, because it
is the lane that proves the rulers still exist.

`.github/workflows/ci.yml` gains the identical command as one step before its
existing `cargo test --workspace` step, and a two-line comment saying why. No
step is removed or reordered. That lane matters because the GitHub job is not
`scripts/ci-check.sh`: it omits `scripts/lint-python.sh` and the identity gate, so
without this step `autotests = false` would pass GitHub CI while failing the local
parity script.

### 12.3 The mapped inventory

24 Rust rows in the section 7 oracle table, 24 hooks, five families. The guard
holds the map; the table holds the digests; neither is complete alone.

| Family | Hook | Rows |
| --- | --- | --- |
| Cargo integration discovery | automatic discovery of `crates/flutterdec-decompiler/tests/*.rs`, disabled wholesale by `autotests = false` | 2: `tests/provenance_audit.rs`, `tests/loop_entry_provenance_audit.rs` |
| decompiler lib hook | `#[cfg(test)]\nmod tests;` in `crates/flutterdec-decompiler/src/lib.rs` | 1: `src/tests.rs` |
| first-level includes | five `include!` lines in `src/tests.rs` | 5: `src/tests/` `shared.rs`, `emit_and_helpers.rs`, `cfg_and_stack.rs`, `compaction_and_aliasing.rs`, `golden_and_parser.rs` |
| nested includes | 8 in `src/tests/cfg_and_stack.rs`, 2 in `src/tests/compaction_and_aliasing.rs`, 4 in `src/tests/emit_and_helpers.rs` | 14 |
| core path loaders | `#[cfg(test)] #[path = "runners/tests.rs"] mod runners_tests;` in `crates/flutterdec-core/src/pipeline/runners.rs`, and `#[cfg(test)] #[path = "symbol_map/tests.rs"] mod tests;` in `crates/flutterdec-core/src/pipeline/symbol_map.rs` | 2 |

What the guard asserts, exact strings rather than patterns a rename or a
reordering could satisfy by accident:

- Each `Hook::Module` file contains its declaration verbatim, newlines and
  `#[path]` attribute included, so an uncommented, re-attributed, or re-pathed
  `mod tests;` does not pass.
- Each `Hook::Include` loader contains `include!("<relative path>");`, the path
  derived from the protected row so the expectation cannot drift from the table.
- Each include loader contains exactly as many `include!` occurrences as it has
  mapped rows, so no loader can grow an oracle section 7 does not record.
- Every mapped file exists on disk.
- Both `crates/flutterdec-decompiler/Cargo.toml` and
  `crates/flutterdec-core/Cargo.toml` contain no line whose whitespace-stripped,
  comment-stripped form is `test=false`, `harness=false`, or `autotests=false`.
  These are the manifest-level silencers: they need no edit to any loader and move
  no digest in section 7.
- Both CI lanes contain a real invocation line, not an `echo` of one, that names
  every automatically discovered integration target in a single command. The
  distinction is not academic: an earlier version of this guard accepted the
  `echo "[ci-check] cargo test ... --test provenance_audit ..."` line, so deleting
  the actual `nix develop -c` invocation passed. That hole is plant 12 in 12.4.
- The section 7 table parse is bounded to the Oracle test files table, from its
  anchor sentence to the end of the section, and fails if it finds fewer than 21
  rows or a non-Rust row, so a broken parse cannot masquerade as a satisfied
  guard.

The guard resolves paths from the workspace root, found as the first ancestor of
its own crate holding `docs/oracle-protocol-ir-cfg-emitter.md`, so it reads the
same tree from a crate directory, the workspace root, or a disposable worktree
copy. It reaches across crates on purpose: `flutterdec-core`'s hooks cannot be
guarded from inside `flutterdec-core`'s own unit tests, since those are exactly
what a broken hook silences.

No mutable product source or manifest is digested anywhere in this record. The
loader lines, the manifest settings, and the CI invocations are protected by value,
like the threshold rulers at the end of section 7.

### 12.4 Planted silencings

One disposable worktree detached at `43ef193`, with this commit's guard and both
CI lanes copied in, `git checkout -- .` between plants, removed afterwards with
`git worktree remove --force`. Reduced suite is the target the plant silences;
guard is `cargo test -p flutterdec-decompiler --test provenance_audit --test
loop_entry_provenance_audit`, which is exactly the new `scripts/ci-check.sh`
lane, so the guard column is the ci-check result for that lane.

| # | Plant | Reduced suite | Guard, and the ci-check lane |
| --- | --- | --- | --- |
| 1 | none, control | decompiler `--lib` `ok` 266; core `--lib` `ok` 91; `--workspace` `ok` 15 binaries 433 tests, exit 0 | `ok`, 2 passed and 1 passed, exit 0 |
| 2 | `#[cfg(test)] mod tests;` deleted from decompiler `src/lib.rs` | `--lib` `ok` 12 passed, exit 0 | `FAILED`, exit 101, names `src/lib.rs` |
| 3 | `include!("tests/golden_and_parser.rs");` deleted from `src/tests.rs` | `--lib` `ok` 262 passed, exit 0 | `FAILED`, exit 101, names the include and the file |
| 4 | `include!("cfg_and_stack/dispatch_table.rs");` deleted from `src/tests/cfg_and_stack.rs` | `--lib` `ok` 256 passed, exit 0 | `FAILED`, exit 101, names the nested include |
| 5 | `#[cfg(test)] #[path = "runners/tests.rs"] mod runners_tests;` deleted from `crates/flutterdec-core/src/pipeline/runners.rs` | core `--lib` `ok` 39 passed, exit 0 | `FAILED`, exit 101, names `runners.rs` |
| 6 | `#[cfg(test)] #[path = "symbol_map/tests.rs"] mod tests;` deleted from `crates/flutterdec-core/src/pipeline/symbol_map.rs` | core `--lib` `ok` 87 passed, exit 0 | `FAILED`, exit 101, names `symbol_map.rs` |
| 7 | new row `crates/flutterdec-decompiler/src/tests/new_oracle.rs` added to the section 7 oracle table with no hook | not applicable | `FAILED`, exit 101, `protected oracle rows with no loader hook recorded in this guard: ["crates/flutterdec-decompiler/src/tests/new_oracle.rs"]` |
| 8 | `[lib] test = false` in the decompiler manifest | `--workspace` with the guard filtered out: `ok` 14 binaries 166 tests, exit 0 | `FAILED`, exit 101, quotes the manifest line |
| 9 | `[lib] harness = false` in the decompiler manifest | `--workspace` fails to compile, `error[E0601]: main function not found in crate flutterdec_decompiler` | `FAILED`, exit 101, quotes the manifest line |
| 10 | `autotests = false` in the decompiler manifest | `--workspace` `ok` 13 binaries 430 tests, exit 0 | exit 101, `error: no test target named provenance_audit in flutterdec-decompiler package` |
| 11 | `include!("cfg_and_stack/structuring.rs");` deleted | `--lib` fails to compile, 110 errors, shared helpers gone | `FAILED`, exit 101, names the nested include |
| 12 | the `nix develop -c cargo test ... --test ...` line deleted from `scripts/ci-check.sh`, its `echo` of the same text left in place | not applicable | `FAILED`, exit 101, `Invocation lines found: []` |
| 13 | `--test loop_entry_provenance_audit` dropped from the `scripts/ci-check.sh` invocation | not applicable | `FAILED`, exit 101, lists the one-target invocation it found |
| 14 | the guard step deleted from `.github/workflows/ci.yml` | not applicable | `FAILED`, exit 101, names `ci.yml` |

Plants 2 through 6, 8 and 10 are the point of the record: each leaves a suite that
prints `test result: ok` and exits 0 with fewer tests. 266 falls to 12, 262, or
256; core's 91 falls to 39 or 87; `test = false` takes the workspace from 15
binaries and 433 tests to 14 and 166; `autotests = false` takes it to 13 and 430
while the two integration oracles vanish entirely. None of those seven moves a
single digest in section 7, so before this commit five of the seven were
undetectable by the protocol, and plants 3 and 4 were detectable only by a reader
recomputing a documentary table by hand.

Plants 9 and 11 fail loudly on their own in this repository, by compile error, and
are recorded as such rather than claimed as silent. They are not free: `harness =
false` is only loud because this crate has no `main`, and the deleted
`structuring.rs` only because it defines helpers the other seven nested files use.
Neither property is guaranteed for a future file, which is why both are asserted
rather than left to luck.

Method note for whoever repeats this: do not share one `CARGO_TARGET_DIR` across
several plant worktrees. The first run of this matrix did, and cargo served
artifacts built from the first worktree to the others, reporting the same 12-test
count and the same failure message for three different plants. One worktree reused
serially with `git checkout -- .` between plants, and its own target directory, is
both correct and faster.

### 12.5 Proof the guard cannot be silenced by what it protects

`crates/flutterdec-decompiler/tests/` is an integration-test directory, so each
file there is its own crate root that links the library through
`use flutterdec_decompiler::...`. Neither file there uses an `include!` invocation
or a `#[path]` attribute; the only `include!` text in the directory is inside the
guard's own expected-string construction. The unit-test loaders it protects are
`#[cfg(test)]` code an integration crate cannot reach, and `flutterdec-core`'s
loaders live in a different crate entirely. Plants 2, 5, and 6 are the runtime
proof: with a hook gone the affected library test target shrinks and still exits
0, while the guard, compiled separately, fails.

The one hook the guard depends on for its own execution is Cargo's automatic
discovery of its own file, which is precisely why both CI lanes now name the
target explicitly. Plant 10 shows the failure mode that closes: `autotests =
false` leaves `cargo test --workspace` passing with 430 tests and no guard, while
the named-target lane errors with `no test target named provenance_audit`.

### 12.6 Section 9 steps

1. Invariant. Not an L1 or L2 invariant: no expected value and no product output
   changed. This is an L4 and L5 change, and the invariant is the section 5 rule
   that a protected oracle may not be silenced, extended from two hooks to the
   whole class. Satisfied in the form 12.4 demonstrates, and in the form 12.2
   proves for the gate script: the executed check set is a strict superset of
   `43ef193` and of `1371e42`.
2. Test. `the_protected_oracle_loader_chain_is_intact` is itself the test, and
   `scripts/ci-check.sh` step 7 is what makes it unskippable. It fails on the old
   behavior in the sense that matters for a guard: at `43ef193` plants 4 through
   14 all passed every gate with every digest in section 7 matching.
3. Diff and digests. Recorded in 12.1 and 12.2, with the reproducing commands.
4. Original reference preserved. Recorded in 12.1.
5. Own commit. Satisfied. This record, the guard, and the two CI lanes land as one
   commit, `test(oracle): protect all oracle loader hooks`, with no product source
   or manifest change in it and before any product source edit of this mission.
6. L5 re-run. `NIX_CONFIG='experimental-features = nix-command flakes'
   scripts/ci-check.sh` exits 0 at the current digests, all twelve lanes green
   including the new one: `test result: ok` 2 passed for `provenance_audit`, 1
   passed for `loop_entry_provenance_audit`, then `cargo test --workspace` with 15
   binaries and 433 tests, and `[ci-check] all checks passed`.

## 13. Adjudication record: the compiled oracle inventory

Section 12 mapped every protected oracle to the hook that compiles it and then
decided whether the hook was live by matching its source text. That decision
procedure is unsound. Rust block comments nest, so `/* /* */ ... */` wraps a hook
without changing one byte of it; a leading `//` does the same; an added
`#[cfg(any())]` or a `#[cfg(feature = "...")]` naming a feature no manifest
declares removes the item while the recorded literal still appears verbatim; and a
`macro_rules!` arm that expands to nothing swallows the hook whole. Each of those
leaves the affected test target compiling, exiting 0, and reporting `test result:
ok` with a smaller suite, with every digest in section 7 matching and the section
12 guard passing.

This record replaces that procedure with the compiler's own answer.
`scripts/check-oracle-inventory.py` reads the section 7 Oracle test files table,
maps each of its 24 rows to one sentinel test that cannot exist unless that row was
compiled, and lists what each protected target actually contains. Every sentinel
must be present. Extra tests never fail: adding a case to a protected oracle is
expected work. Source-text observations survive in the guard as printed
diagnostics and are asserted nowhere.

Three protected paths move here, `scripts/check-oracle-inventory.py` as a new row,
`crates/flutterdec-decompiler/tests/provenance_audit.rs`, and
`scripts/ci-check.sh`, plus one unprotected CI lane, `.github/workflows/ci.yml`.
No product source, no manifest, and no oracle assertion changes in this commit.

### 13.1 Digest chains

Column order matches sections 10.1, 11.1 and 12.1, state before digest, so a
scanner looking for the section 7 row shape does not read these history rows as
protected-path rows.

`crates/flutterdec-decompiler/tests/provenance_audit.rs`, continuing 12.1:

| Commit | State | sha256 |
| --- | --- | --- |
| `0fadd6e` | prior, source-text guard, adjudicated in section 12 | `b5712a66b0f6472a726d4d253555272b54322cd69c3fdb1a3bed514de8cb9765` |
| this commit, worktree | current, recorded in section 7 | `1bda72504e7ada1c8a2e7798ca314b3843ebc6cf8b8202851de42dd542573abd` |

`scripts/ci-check.sh`, continuing 12.1:

| Commit | State | sha256 |
| --- | --- | --- |
| `0fadd6e` | prior, adjudicated in section 12 | `171aa8894675ed2c90ff40c9d6a136bd791c3ae0d51c7965617e682b31d2f067` |
| this commit, worktree | current, recorded in section 7 | `386e0f2a22a25c774ff43da8621e947d9c3a4137e57a5d8ee6bbad973eb25c48` |

`scripts/check-oracle-inventory.py` is new in this commit and has no prior state.
Its digest is `d882132e87cb4625ebdac88ab310e405b00133bd546e172db282be7e1bbf47bf`,
recorded in section 7 with the other checkers. It is protected because it is now
the ruler: weakening it, for instance by dropping a sentinel or by accepting a
target it failed to list, would make the inventory pass over a silenced oracle.

`.github/workflows/ci.yml` is still not a section 7 row, for the reason 12.1
gives. Recorded for reproducibility only:
`c51642b043cfa254c454282aee5d14b24d899d29bcda83d8e42a9e1da9968c55` at `0fadd6e`,
`6866ce3d8f8f96d8f8ed59c932f1002e962763635702de687cd2dabb18b68c80` in this
commit. Both of its load-bearing lines are asserted by value by the guard, not by
digest.

Reproduce any row with `git show <commit>:<path> | sha256sum`, and the last row of
each chain with `sha256sum <path>`.

### 13.2 Exact diff intent

`git diff --numstat 0fadd6e` for the four non-protocol files: 59 insertions and 10
deletions in `crates/flutterdec-decompiler/tests/provenance_audit.rs`, 16 and 3 in
`scripts/ci-check.sh`, 6 and 0 in `.github/workflows/ci.yml`, and
`scripts/check-oracle-inventory.py` is a new file.

The 10 deleted guard lines are the two `assert!(source.contains(...))` calls for
the `Hook::Module` and `Hook::Include` families and their messages. They are the
two this record disproves, and they are demoted, not dropped: the same
observations are still made and now printed as `loader-hook diagnostic:` lines.
Nothing else in the guard is removed. It still asserts, as hard failures:

| Section 12 assertion | State here |
| --- | --- |
| every table row has a mapped hook, every mapped hook has a row | unchanged |
| every mapped file exists on disk | unchanged |
| each include loader holds exactly its mapped number of `include!` occurrences | unchanged |
| neither manifest sets `test = false`, `harness = false`, or `autotests = false` | unchanged |
| both CI lanes name every discovered integration target in one real invocation | unchanged |
| a `Hook::Module` file holds its declaration verbatim | now a printed diagnostic |
| a `Hook::Include` loader holds its `include!` line verbatim | now a printed diagnostic |
| - | new: both CI lanes run `nix develop -c python3 scripts/check-oracle-inventory.py` as a lane of their own, and that file exists |

Nothing is lost by the demotion, and 13.4 measures it rather than arguing it. The
three plants section 12.4 relied on those two assertions to catch, its plants 2, 3
and 5, are rows `d1`, `i1` and `c3` here, and the compiled inventory rejects all
three. The inventory also rejects the eight plants no source-text check can see.

`scripts/ci-check.sh` gains one lane at position 8, immediately after the named
integration targets and before `cargo test --workspace`, and it also runs under
`--skip-tests`. The same mechanical additivity proof sections 10.3 and 12.2 use:

```
git show 0fadd6e:scripts/ci-check.sh > /tmp/cic-prev.sh
strip() { grep -vE '^[[:space:]]*(#|$)' "$1" | sed 's/[[:space:]]*$//' | sort; }
comm -23 <(strip /tmp/cic-prev.sh) <(strip scripts/ci-check.sh)
```

Three lines are reported, and all three are text inside the `usage()` heredoc:

```
  8) cargo test --workspace            (unless --skip-tests)
  9) cargo build -p flutterdec-cli --release
 10) fmt, clippy and tests for the excluded benchmark harness
```

Each reappears with the same command text and a different list number, `9)`,
`10)` and `11)`, because the new lane took position 8. No executable line present
at `0fadd6e` is absent now; one command is added,
`nix develop -c python3 scripts/check-oracle-inventory.py`.

`.github/workflows/ci.yml` gains the identical command as one step before its
existing `cargo test --workspace` step, and a three-line comment saying why. No
step is removed or reordered. It matters that this lane exists in both places:
the demoted text assertions used to fail inside `cargo test --workspace`, which
the GitHub job does run, so without this step every plant in the `d`, `c` and `i`
families of 13.4 would newly pass GitHub CI.

The existing audit test is still byte-identical to `1371e42` from its signature
onward, by the 12.2 command. Both sides still print
`c76130ef412fde06ba1706e924e12fe0d85e802323c6e0d155a88cf6202e6d02`.

### 13.3 The 24 row-to-sentinel mappings

One key per row of the section 7 Oracle test files table, and the checker fails if
that correspondence breaks in either direction. Target names are the checker's;
they select `-p flutterdec-decompiler --lib`, `-p flutterdec-core --lib`,
`-p flutterdec-decompiler --test provenance_audit`, and
`-p flutterdec-decompiler --test loop_entry_provenance_audit`.

Nineteen rows own their sentinel: the test is defined in that file. Five cannot,
because they hold no test of their own, and they take a descendant that cannot
compile without them. The four loaders take a test they include; `shared.rs` takes
the one test whose fixture is built from `branch_block` and `jump_block`, which
only `shared.rs` defines.

| Protected row | Target | Sentinel | Defined in |
| --- | --- | --- | --- |
| `crates/flutterdec-decompiler/src/tests.rs` | `decompiler-lib` | `tests::golden_structured_loop_emit_snapshot` | descendant `src/tests/golden_and_parser.rs` |
| `crates/flutterdec-decompiler/tests/provenance_audit.rs` | `provenance-audit` | `the_pre_call_audit_traces_each_candidate_and_its_checker_catches_a_wrong_path` | itself |
| `crates/flutterdec-decompiler/tests/loop_entry_provenance_audit.rs` | `loop-entry-audit` | `the_loop_entry_audit_traces_each_candidate_and_its_checker_catches_a_wrong_path` | itself |
| `crates/flutterdec-decompiler/src/tests/shared.rs` | `decompiler-lib` | `tests::emits_helper_bodies_for_omitted_paths` | descendant `emit_and_helpers/helper_inlining.rs`, whose fixture uses `branch_block` and `jump_block` |
| `crates/flutterdec-decompiler/src/tests/golden_and_parser.rs` | `decompiler-lib` | `tests::golden_null_guard_compaction_snapshot` | itself |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack.rs` | `decompiler-lib` | `tests::folds_movk_halves_into_the_selector_offset` | descendant `cfg_and_stack/dispatch_table.rs` |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/structuring.rs` | `decompiler-lib` | `tests::emits_a_join_block_exactly_once` | itself |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/order_totality.rs` | `decompiler-lib` | `tests::candidate_order_is_total_over_every_permutation_of_its_input` | itself |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/join_capture.rs` | `decompiler-lib` | `tests::captures_a_candidate_from_every_predecessor_of_a_three_predecessor_join` | itself |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/annotation_caps.rs` | `decompiler-lib` | `tests::omits_the_whole_annotation_when_it_exceeds_the_per_annotation_budget` | itself |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/omitted_path_and_stack.rs` | `decompiler-lib` | `tests::collapses_helper_calls_into_omitted_path_comments` | itself |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/call_and_loops.rs` | `decompiler-lib` | `tests::emits_callable_style_for_generic_indirect_targets` | itself |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/call_annotations.rs` | `decompiler-lib` | `tests::a_call_clobber_annotates_the_value_held_immediately_before_that_call` | itself |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/dispatch_table.rs` | `decompiler-lib` | `tests::names_dispatch_table_calls_from_the_sub_encoding` | itself |
| `crates/flutterdec-decompiler/src/tests/compaction_and_aliasing.rs` | `decompiler-lib` | `tests::collapses_if_else_with_identical_returns` | descendant `compaction_and_aliasing/control_flow_compaction.rs` |
| `crates/flutterdec-decompiler/src/tests/compaction_and_aliasing/control_flow_compaction.rs` | `decompiler-lib` | `tests::rewrites_empty_then_else_to_negated_if` | itself |
| `crates/flutterdec-decompiler/src/tests/compaction_and_aliasing/alias_and_expr_cleanup.rs` | `decompiler-lib` | `tests::collapses_nested_guarded_returns_inside_if_body` | itself |
| `crates/flutterdec-decompiler/src/tests/emit_and_helpers.rs` | `decompiler-lib` | `tests::no_annotation_consumer_hand_rolls_a_delimiter` | descendant `emit_and_helpers/annotation_literals.rs` |
| `crates/flutterdec-decompiler/src/tests/emit_and_helpers/helper_inlining.rs` | `decompiler-lib` | `tests::inlines_linear_helper_body_at_call_site` | itself |
| `crates/flutterdec-decompiler/src/tests/emit_and_helpers/annotation_literals.rs` | `decompiler-lib` | `tests::each_annotation_literal_has_exactly_one_definition` | itself |
| `crates/flutterdec-decompiler/src/tests/emit_and_helpers/candidate_whitelist.rs` | `decompiler-lib` | `tests::each_allowed_form_is_accepted_as_that_form` | itself |
| `crates/flutterdec-decompiler/src/tests/emit_and_helpers/readability_and_naming.rs` | `decompiler-lib` | `tests::compacts_empty_else_and_duplicate_null_returns` | itself |
| `crates/flutterdec-core/src/pipeline/runners/tests.rs` | `core-lib` | `runners_tests::aggregates_semantic_intent_counts_from_pseudocode` | itself, `runners/tests.rs:1387` |
| `crates/flutterdec-core/src/pipeline/symbol_map/tests.rs` | `core-lib` | `tests::resolves_exact_before_nearest` | itself |

Every sentinel is distinct, and each is defined exactly once across
`crates/**/*.rs`, so a failure names the row that lost its hook rather than a
family.

Two mechanisms carry the checker, not one. Listing a target with `-- --list`
compiles it, which is what catches a hook removed by a comment, a `cfg`, or a
macro. Before listing, `cargo metadata --no-deps` is read and every protected
target must still be built for tests, which is what catches `test = false`: an
explicit `--lib` selection overrides that setting and would have listed the
target anyway. Rows `m1` and `m4` of 13.4 are that case, and they were passing the
inventory before the metadata gate was added.

### 13.4 Planted silencings

One disposable worktree detached at `0fadd6e` with this commit's four files copied
in, its own `CARGO_TARGET_DIR`, `git checkout -- .` and `git clean -fd` between
plants, removed afterwards with `git worktree remove --force`. Every row below was
produced in a single sequential run against the final code, not accumulated across
revisions. Workspace is `cargo test --workspace`, counting result lines and the
passed column; inventory is `python3 scripts/check-oracle-inventory.py`.

| # | Plant | Workspace | Inventory |
| --- | --- | --- | --- |
| 0 | none, control | exit 0, 15 binaries, 433 tests | exit 0, 24 compiled |
| d1 | `#[cfg(test)]\nmod tests;` deleted from decompiler `src/lib.rs` | exit 0, 15 binaries, 179 tests | exit 1, 20 rows not compiled |
| d2 | the same hook prefixed with `//` on both lines | exit 0, 15 binaries, 179 tests | exit 1, 20 rows |
| d3 | the same hook wrapped in a nested `/* /* silenced */ ... */` | exit 0, 15 binaries, 179 tests | exit 1, 20 rows |
| d4 | `#[cfg(any())]` added above the same hook, hook text intact | exit 0, 15 binaries, 179 tests | exit 1, 20 rows |
| d5 | `#[cfg(feature = "no-such-feature")]` added above it, hook text intact | exit 0, 15 binaries, 179 tests | exit 1, 20 rows |
| d6 | the same hook passed to a `macro_rules!` arm that expands to nothing | exit 0, 15 binaries, 179 tests | exit 1, 20 rows |
| c1 | `symbol_map.rs` `#[path]` hook wrapped in a nested block comment | exit 0, 15 binaries, 429 tests | exit 1, names `symbol_map/tests.rs` |
| c2 | `#[cfg(any())]` added above the `runners.rs` `#[path]` hook | exit 0, 15 binaries, 381 tests | exit 1, names `runners/tests.rs` |
| c3 | the `runners.rs` `#[path]` hook deleted | exit 0, 15 binaries, 381 tests | exit 1, names `runners/tests.rs` |
| c4 | the `symbol_map.rs` hook swallowed by a `macro_rules!` arm | exit 0, 15 binaries, 429 tests | exit 1, names `symbol_map/tests.rs` |
| i1 | `include!("tests/golden_and_parser.rs");` deleted from `src/tests.rs` | exit 101, 6 binaries, 375 tests | exit 1, names `src/tests.rs` and `golden_and_parser.rs` |
| i2 | `include!("cfg_and_stack/dispatch_table.rs");` prefixed with `//` | exit 0, 15 binaries, 423 tests | exit 1, names `cfg_and_stack.rs` and `dispatch_table.rs` |
| i3 | `include!("emit_and_helpers/candidate_whitelist.rs");` in a nested block comment | exit 0, 15 binaries, 429 tests | exit 1, names `candidate_whitelist.rs` |
| m1 | `[lib] test = false` appended to the decompiler manifest | exit 101, 5 binaries, 113 tests | exit 1, names the `lib` target |
| m2 | `[lib] harness = false` appended to the decompiler manifest | exit 101, compile error | exit 1, listing invocation failed |
| m3 | `autotests = false` in the decompiler `[package]` | exit 0, 13 binaries, 430 tests | exit 1, names both integration targets |
| m4 | `[lib] test = false` appended to the core manifest | exit 101, 5 binaries, 288 tests | exit 1, names the core `lib` target |
| f1 | protected file `cfg_and_stack/join_capture.rs` deleted | exit 101, compile error | exit 1, listing invocation failed |
| f2 | protected file `tests/loop_entry_provenance_audit.rs` deleted | exit 101, 5 binaries, 378 tests | exit 1, names the missing target |
| t1 | synthetic row `src/tests/ghost.rs` added to the section 7 oracle table | exit 101 | exit 1, `has no sentinel here` |
| l1 | the inventory lane deleted from `scripts/ci-check.sh` | exit 101 | exit 0, as it must be |
| l2 | the inventory lane deleted from `.github/workflows/ci.yml` | exit 101 | exit 0, as it must be |
| e1 | control: a new `#[test]` added to `cfg_and_stack/structuring.rs` | exit 0, 15 binaries, 434 tests | exit 0, 24 compiled |

Thirteen plants leave `cargo test --workspace` green with fewer tests: `d1` through
`d6` take 433 to 179, `c1` and `c4` to 429, `c2` and `c3` to 381, `i2` to 423, `i3`
to 429, and `m3` to 430 with both integration oracles gone. Every one of the
thirteen is rejected by the compiled inventory. Nine of them, `d2` through `d6`,
`c1`, `c2`, `c4` and `i3`, leave the section 12 guard's own recorded hook literal
byte-identical, so before this commit all nine passed every gate in this protocol
with every section 7 digest matching.

`l1` and `l2` are the reverse direction, and their inventory column is the point:
deleting a lane does not silence an oracle, so the checker correctly still passes.
What catches them is the guard, which is why the lane assertion is a hard failure
there. `e1` is the no-false-positive control: adding a case to a protected oracle
raises the suite to 434 and the inventory stays green, because extras are always
allowed.

`m2`, `f1` and `t1` fail loudly on their own in this repository, by compile error
or by the guard's own map check, and are recorded as such rather than claimed as
silent.

### 13.5 The checker's own self-tests

`scripts/check-oracle-inventory.py` runs its unit checks at the start of every
invocation, and `--self-test` runs only those. They cover the two pieces that are
not the compiler:

- The table parser takes exactly the Oracle test files table: a row in an earlier
  section 7 table and a row after the section boundary are both excluded, and a
  protocol missing the anchor sentence raises rather than parsing as zero rows.
- Extras are allowed: a listing holding both sentinels plus two unrelated tests
  passes with no failures.
- A missing sentinel fails once, naming both the protected row and the sentinel.
- A target with no usable listing is reported as that one root cause, and when a
  manifest is the reason, the message says so instead of blaming the tests.
- Both directions of the table-to-map correspondence fail, and a non-Rust row is
  refused rather than silently left unmapped.

### 13.6 Section 9 steps

1. Invariant. Not an L1 or L2 invariant: no expected value and no product output
   changed. This is an L4 and L5 change, and the invariant is the section 5 rule
   that a protected oracle may not be silenced, now decided by compilation rather
   than by source text. Satisfied in the form 13.4 measures.
2. Test. `scripts/check-oracle-inventory.py` is the test, and
   `scripts/ci-check.sh` step 8 plus the `Compiled oracle inventory` step in
   `.github/workflows/ci.yml` are what make it unskippable. It fails on the old
   behavior in the sense that matters: at `0fadd6e` the nine text-preserving
   plants of 13.4 passed every gate with every digest in section 7 matching.
3. Diff and digests. Recorded in 13.1 and 13.2, with the reproducing commands.
4. Original reference preserved. Recorded in 13.1, continuing the chains in 10.1,
   11.1 and 12.1.
5. Own commit. Satisfied. This record, the checker, the guard demotion and the two
   CI lanes land as one commit, `test(oracle): verify compiled oracle inventory`,
   with no product source or manifest change in it.
6. L5 re-run. `NIX_CONFIG='experimental-features = nix-command flakes'
   scripts/ci-check.sh` exits 0 at the current digests, with the new lane green:
   `[oracle-inventory] ok, 24 protected oracles are compiled`.
