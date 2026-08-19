# Correctness protocol: IR, CFG, and emitter

This protocol is fixed before any product change. It ranks the evidence that may
decide whether a change to instruction classification, CFG construction, region
analysis, or emission is correct, and it names the rulers that a candidate is not
allowed to move.

Reference commit: `1371e42`. Branch: `research/ir-cfg-emitter`. The pipeline map
and the risk list this protocol tests against are in
[research-ir-cfg-emitter.md](research-ir-cfg-emitter.md).

Scope of the sections, because the two halves of this document are read
differently. Sections 1 through 9 are the standing sections: every path, symbol,
count, and digest in them describes the tree at `HEAD` unless the sentence names
a commit, and each is kept true as the tree moves. Two parts of them are pinned
to `1371e42` instead, by their own headings and again where they are used: the
Status column of the case matrix in section 2, and the recorded evidence in
section 8. Every section from 10 on is an adjudication record for one commit and
describes that commit's tree only; section 9 gives the mechanical rule for
reading those. Within the standing sections a line-number citation is used only
for a file section 7 pins by digest, whose bytes cannot move without a section 9
record of their own; product source, which ordinary work renumbers, is cited by
symbol.

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
(`crates/flutterdec-decompiler/src/control_flow/regions.rs`), so the assertions
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
`scripts/lint-python.sh`, `scripts/bench-identity-gate-test.sh`, `cargo clippy
--workspace --all-targets -- -D warnings`, the three named protected-target
lanes, `scripts/check-oracle-inventory.py`,
`scripts/check-resource-ruler.py`, `cargo test --workspace`, a release build of
the CLI, and fmt, clippy and tests for the excluded benchmark harness.
`.github/workflows/ci.yml` runs every one of those commands, byte-identical, so
neither lane is the sole authority for any guard. The single exception is the
harness test lane, which runs on the Linux runner only because the harness reads
its peak RSS from `/proc/self/status`; the local gate is a Linux gate too.
Section 24 adjudicates that parity, measures that exception, and records the
guard that now fails when one lane drops a command the other still runs.

Outside the hierarchy, and not usable as an oracle here: recompiling the emitted
pseudocode. It is deliberately not source equivalent. Emitted line count is not a
quality measure either, and no promotion may rest on it.

## 2. Case matrix

Class: A adversarial, E edge, S stress, C cap. Status is the state at `1371e42`.
"Expected" is the invariant the case must prove, not a description of current
behavior.

Every citation in a Status cell is a location in the `1371e42` tree and is read
there, with `git show 1371e42:<path>`, never against `HEAD`. Each resolves at that
commit to the first line of the test the cell describes, or to the comment a cell
calls a comment. A Status cell is therefore history and makes no claim about the
current tree: `crates/flutterdec-ir/src/lib.rs` is product source and has been
renumbered many times since, and
`crates/flutterdec-decompiler/src/tests/cfg_and_stack/omitted_path_and_stack.rs`
was moved by section 16. Expected cells cite current code, by symbol.

### Instruction classification and block construction (L1)

| Case | Shape | Class | Expected | Status at `1371e42` |
| --- | --- | --- | --- | --- |
| IR-01 | `b` to a known target | E | block ends, one successor, no fallthrough | covered indirectly, `crates/flutterdec-ir/src/lib.rs:400-445` |
| IR-02 | `b.<cond>` | E | block ends, successors are target and fallthrough | covered, `crates/flutterdec-ir/src/lib.rs:542-577` |
| IR-03 | `tbnz` with three operands | A | target parsed from the last operand token | covered, `crates/flutterdec-ir/src/lib.rs:580-615` |
| IR-04 | `bl` and `blr` | E | call, block continues, fallthrough preserved | partially covered, `crates/flutterdec-ir/src/lib.rs:400-445` asserts the elided case only |
| IR-05 | `br xN` | A | block ends, no fallthrough, no invented target | fails by inspection, risk R1 |
| IR-06 | `brk` | A | block ends, no successors | fails by inspection, risk R1 |
| IR-07 | `ret` followed by more code | E | block ends, next instruction is a leader | covered, `crates/flutterdec-ir/src/lib.rs:522-539` |
| IR-08 | stack overflow guard group, several SDK offsets and both scratch registers | A | three `RuntimeCheck` ops, no call, no guard edge, slow path pruned | covered, `crates/flutterdec-ir/src/lib.rs:400-489` |
| IR-09 | `cmp x15` against a non `THR` load | A | not recognized as the guard | covered, `crates/flutterdec-ir/src/lib.rs:494-517` |
| IR-10 | duplicate `start_va` or duplicate ids in a constructed `FunctionIr` | A | rejected or exposed, never silently overwritten in a map | not covered |
| IR-11 | block unreachable for a reason other than the guard | A | retained and reported, not deleted | asserted only by the comment at `crates/flutterdec-ir/src/lib.rs:291-299` |
| IR-12 | 1024 blocks, representative mix | S | successor and predecessor sets stay sorted, unique, and reciprocal | not covered |
| IR-13 | direct target radix spellings | A | prefixed hex and bare hex containing `a`-`f` select hexadecimal; all-digit operands select decimal at every length; malformed or ambiguous operands remain unknown | covered through public CFG construction, `crates/flutterdec-ir/tests/branch_target_radix.rs` |

### CFG and region analysis (L2)

Every case asserts the full literal relation set, not one relation.

| Case | Shape | Class | Expected | Status at `1371e42` |
| --- | --- | --- | --- | --- |
| CFG-01 | linear chain | E | reachable is all, each block post-dominates its predecessor, no loop, reducible | not covered directly |
| CFG-02 | diamond | E | follow of the branch is the join, join set is exactly the merge block | not covered directly |
| CFG-03 | fan in with three predecessors | A | `predecessors` ascending, `is_join` true, follow correct | predecessors covered indirectly, `src/tests/cfg_and_stack/join_capture.rs:69` |
| CFG-04 | nested natural loops | E | inner body subset of outer body, one header each | emission covered, `src/tests/cfg_and_stack/structuring.rs:538` |
| CFG-05 | loop with several exits | A | loop follow is the header immediate post-dominator outside the body (`natural_loops` in `control_flow/regions.rs`) | not covered directly |
| CFG-06 | loop with no exit | E | loop follow is `None`, no panic, emission still terminates | not covered |
| CFG-07 | irreducible, two entries into one loop | A | `Regions::build` returns `None`, emitter declines and still emits | emission covered, `src/tests/cfg_and_stack/structuring.rs:586` |
| CFG-08 | unreachable block present | A | unreachable excluded from analysis, `reachable_count` unchanged by it, successor list cleared not deleted | not covered |
| CFG-09 | self loop | E | header is its own body member, back edge detected | not covered |
| CFG-10 | two exit blocks, equal sized post-dominator sets | A | immediate post-dominator identical across process hash seeds (the size-then-index tie-break in `immediate_post_dominators`, `control_flow/regions.rs`) | in process only, `src/tests/cfg_and_stack/order_totality.rs:221` |
| CFG-11 | 64, 256, and 1024 blocks, each shape above that scales | S | relations unchanged from the small case, within run limits | not covered |

### Emission (L3)

| Case | Shape | Class | Expected | Status at `1371e42` |
| --- | --- | --- | --- | --- |
| EM-01 | join reachable from two arms | E | emitted exactly once | covered, `structuring.rs:42` |
| EM-02 | irreducible graph | A | DFS fallback runs, body non empty, no structured provenance survives (`restore_emitter`, `control_flow/structured.rs`) | covered, `structuring.rs:586` |
| EM-03 | small shared region that is nobody's follow node | A | repeated within 16 blocks and 96 instructions, `repeated_blocks` incremented | covered, `structuring.rs:708` |
| EM-04 | arm that returns | A | its bindings do not leak past the branch | covered, `structuring.rs:617` |
| EM-05 | more than 64 distinct omitted blocks | C | every emitted `_block_N()` either resolves to a definition or is collapsed to an explicit omission that the summary comment names, and `quality.json` `block_helper_refs` is 0 | not covered, risk R2 |
| EM-06 | block visited past its visit limit (48, 24, 14 in `FuncEmitter::visit_limit`, `helper_flow/summary.rs`) | C | omitted path emitted, id recorded once | covered for the collapse half only, `src/tests/cfg_and_stack/omitted_path_and_stack.rs:2` |
| EM-07 | annotation at and one byte past the 3000 character line budget | C | whole annotation omitted, counted against its site | covered, `src/tests/cfg_and_stack/annotation_caps.rs:185` |
| EM-08 | nesting deeper than the structured depth cap of 64 (`STRUCTURED_MAX_DEPTH`, `control_flow/structured.rs`) | C | decline, not truncation | not covered |
| EM-09 | same synthetic input, three separate processes | A | byte identical artifact set after section 6 normalization | not covered |
| EM-10 | function whose entry block has no instructions | E | the empty-body last resort in `FuncEmitter::emit_with_plan` (`crates/flutterdec-decompiler/src/lib.rs`) does not double emit | not covered, unreachable on current samples |

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
already documented on `IROp::RuntimeCheck` in `crates/flutterdec-ir/src/lib.rs`.
"Ends block" means the following instruction must become a leader.

| Instruction | Architectural effect | Required class | Ends block | Required edges | Forbidden edges |
| --- | --- | --- | --- | --- | --- |
| `B label` | unconditional PC relative branch | jump | yes | the label block | fallthrough |
| `B.cond label` | conditional branch | branch | yes | label block and fallthrough | any third edge |
| `CBZ`, `CBNZ` | compare and branch on zero | branch | yes | label block and fallthrough | any third edge |
| `TBZ`, `TBNZ` | test bit and branch | branch | yes | label block and fallthrough | any third edge |
| `BL label` | branch with link, sets X30, returns | call | no | fallthrough | an edge to the callee |
| `BLR Xn` | indirect branch with link, sets X30, returns | call | no | fallthrough | an edge to a guessed callee |
| `BR Xn` | indirect branch, no link, does not return here | indirect branch | yes | none unless a target set is independently recovered | fallthrough |
| `RET` | return to X30 | return | yes | none | fallthrough |
| `BRK #imm` | breakpoint instruction exception, control does not continue | trap | yes | none | fallthrough |
| Dart guard group `ldr` from `THR`, `cmp` against `SPREG`, `b.ls` | runtime stack limit check, slow path re-enters the body | runtime check | no | fallthrough only | the taken guard edge, and the slow path back edge |

Repository evidence for the two indirect-control rows, `BR Xn` and `BRK #imm`.
Both were classified as `IROp::Other` with an invented fallthrough at `1371e42`,
which is the state the section 2 rows IR-05 and IR-06 record, and both were
brought to the values this table demands at `ac544ca`. In the current tree, all
four of those mechanisms live in `crates/flutterdec-ir/src/lib.rs`, and each is
named here by symbol rather than by line, because that file is product source
this mission keeps editing. The `"br"` and `"brk"` arms of the mnemonic match in
`llir_from_disasm` classify `br` as `IROp::IndirectBranch` and `brk` as
`IROp::Trap`. In `build_function_ir_accounted`, the
`IROp::Return | IROp::IndirectBranch | IROp::Trap` arm of the leader loop makes
the instruction after either one a leader, and the arm of the same three classes
in that function's successor loop leaves the successor list empty, so neither can
take a fallthrough and neither can take a guessed target. The register operand of
`br` is kept only as provenance for the emitters, in the `"br"` arm that copies
`op_str` into the instruction's `target` field; `parse_direct_target`, which
replaced `parse_target_hex` in section 20, rejects a register name, so no edge is
derived from it. The same expectation is stated as data in the twelve-row
`CONTROL_EFFECTS` table, which section 15 moved out of that file into
`crates/flutterdec-ir/src/tests/control_effects.rs`: its `br` row asserts
`IROp::IndirectBranch` with `succ_starts: &[]` and its `brk` row asserts
`IROp::Trap` with `succ_starts: &[]`. That table is driven by two tests in the
same file, `every_arm64_control_effect_has_exactly_the_documented_edges` for the
edges and
`only_a_control_effect_that_ends_a_block_makes_the_next_instruction_a_leader`
for the block-ending column, and two more tests there,
`an_indirect_branch_keeps_its_register_and_takes_no_edge` and
`a_trap_ends_the_block_with_no_successors`, assert the two rows on their own.
That file is a digest-pinned row of section 7, so those four names cannot be
weakened without a section 9 record. Downstream evidence is unchanged from
`1371e42` and still corroborates the same reading:
`is_terminator` in `crates/flutterdec-core/src/pipeline/runners/split.rs` treats
`ret`, `brk`, `b`, and `br` as path enders, and `tail_calls_immediately` in
`crates/flutterdec-core/src/pipeline/runners/stubs.rs` reads `br` as the tail of a
dispatch stub. Section 14 adjudicates the class name in the `BR Xn` row.

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
   `crates/flutterdec-cli/src/main.rs` finds nothing, and the same search over
   the same paths still finds nothing at `HEAD`. The allowance therefore applies
   only to timing fields that the later benchmark harness emits into its own
   output, never into `quality.json` or `report.json`.

Everything else is compared verbatim. In particular `quality.json` has no path
or time field at all: it is `QualityReport` in
`crates/flutterdec-core/src/lib.rs`, whose every field, and every field of the
`EmissionReport` it nests, is a mode string, a boolean, a failure list, a count,
or a ratio. So it is compared byte for byte.

## 7. Protected paths and digests

Recorded at `1371e42` with `sha256sum`. A change to any of these files is a
ruler change and requires section 9, whether or not a test still passes.

Every digest below is the current worktree value, and since section 17 it is
recomputed on every CI run rather than only whenever this table is touched.
`scripts/check-oracle-inventory.py` verifies all 71 rows below before it does any
Cargo work, against a hardcoded inventory of exactly these paths, so a row
deleted from this table, a row added to it, a duplicated path, a digest that is
not 64 lowercase hex characters, a protected path that is no longer an existing
regular file, and a protected file whose bytes changed are each a hard CI
failure. That checker is itself one of the rows below, so it verifies its own
bytes.

Seven rows now hold a digest other than the one first pinned for them. The rule
is the same for every row, whenever it joined this table: a row's first pinned
digest is the digest it carried in the commit that added it here, and the row
counts if its current digest differs from that one. Five of the seven were
already protected at `1371e42`. The other two joined later and have moved since,
so they count under exactly that rule: `scripts/check-oracle-inventory.py`,
which became a row in section 13 at
`d882132e87cb4625ebdac88ab310e405b00133bd546e172db282be7e1bbf47bf`, and
`crates/flutterdec-decompiler/src/control_flow/relation_oracle.rs`, which became
a row in section 15 at
`75fe720e04cfa6bbb859981f1b39ebba0e0ed932e8973d3bab730058cedcfa96` and has moved
twice since, in section 19 and in section 21. A row that joined after `1371e42`
and still carries its first pinned digest is not one of the seven.

`scripts/ci-check.sh` has moved twelve times. Section 10 adjudicates the first
three as one chain from the original fixed reference. The fourth is in section
12, the fifth in section 13, the sixth in section 15, and the seventh in section
19. The eighth belongs to the auxiliary resource inventory rather than to this
protocol: `docs/resource-ruler-protocol.md` pinned this file when that inventory
was created, at
`b1600c29ccbda98b751e8a337c6aa875dfc56eef3dc66efb9edb00952c78188c`, which is the
prior value section 20 starts from, and every later move is adjudicated in both
documents. The ninth is in section 20, the tenth in section 21, the eleventh in
section 22, and the twelfth in section 23. Section 24 leaves it byte-unchanged.

`crates/flutterdec-decompiler/tests/provenance_audit.rs` has moved ten times: the
first in section 11, the second in section 12, the third in section 13, the
fourth in section 15, the fifth in section 19, the sixth in section 20, the
seventh in section 21, the eighth in section 22, the ninth in section 23, and the
tenth in section 24.

`crates/flutterdec-decompiler/src/tests/cfg_and_stack/omitted_path_and_stack.rs`
and `crates/flutterdec-core/src/pipeline/runners/tests.rs` each moved once, in
section 16. `scripts/prov_cross_audit_reconcile.py` moved once, in section 18.

`scripts/check-oracle-inventory.py` became a row in section 13, and has moved
seven times since: in section 15, section 17, section 19, section 20,
section 21, section 22, and section 23.

Forty-five rows were pinned at `1371e42`, and twenty-six were added afterwards,
which is every one of the 71. The twenty-six are: one in section 10,
`crates/flutterdec-decompiler/src/tests.rs`, the decompiler test loader that
section 10's commit protects alongside the `scripts/ci-check.sh` chain that
record is named for; one in section 13, `scripts/check-oracle-inventory.py`;
nine in section 15, the IR and CFG boundary oracles; eleven emitter repair
rulers in section 19; and one in each of section 20,
`crates/flutterdec-ir/tests/branch_target_radix.rs`, section 21,
`crates/flutterdec-decompiler/tests/dfs_loop_address_invariance.rs`, section 22,
`crates/flutterdec-decompiler/tests/entry_loop_state_merge.rs`, and section 23,
the block-ledger contract row. Section 24 adds no row. The section 10 row has
never moved since it was added; its file already existed at `1371e42` with the
same bytes it has today, but it was not a protected row until section 10, which
is why it is counted here and not among the forty-five.

The other sixty-four rows hold the digest first pinned for them, and they split
the same way the forty-five and the twenty-six do. Forty of the forty-five rows
pinned at `1371e42` still hold their `1371e42` bytes; the other five are the five
movers named above. Twenty-four of the twenty-six later rows still hold the
digest they joined with; the other two are the two later joiners named above. No
claim is made here about `1371e42` bytes for the twenty-five paths that did not
exist at `1371e42`: every later row except the section 10 loader is such a path,
and that loader is the one whose file did exist there, unchanged since. One of
the forty,
`crates/flutterdec-decompiler/src/tests/cfg_and_stack/call_and_loops.rs`, left
them for exactly one commit and came back: `0e8b7d6` moved it to
`ed336192386451a0db795918530a63eece3e0367f251b43b4a82d5a3a416c9fc` and `6b79f0e`
restored `2c2b433a07a8bab0d1a0adf4c09bc9f2982c3f74ecb997361b4729b1b3612630`, the
value in the table below, so the row reads unchanged today. A row that does not
match the current worktree is a failure of this table, not of the file.

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
| `scripts/check-oracle-inventory.py` | `3900f505ea8aea59500c99fcf598013cffac55e9128ec9498f7811738bcbf71a` |
| `scripts/prov_cross_audit_reconcile.py` | `f7a21d2c497ff2c47e118cf3df208d869265eb11a561605f4f14d9c50febe870` |
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
| `scripts/ci-check.sh` | `9dac174b7a2a4e3a0d14d182d292dac0dbc8b6c63679e859da7cc8dad21ea45e` |
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

That loader is one of seven hook families, and every row below depends on one of
them. None of the hooks can be digested here, because they all live in product
source or in manifests that later work must edit, so a whole-file digest for any
of them would fire on legitimate change and be worthless as a ruler.

That constraint decides where an oracle may live, not just how it is proved. Nine
rows below are IR and CFG boundary rulers that were written as inline
`#[cfg(test)] mod` blocks inside `crates/flutterdec-ir/src/lib.rs`,
`crates/flutterdec-ir/src/validate.rs`,
`crates/flutterdec-core/src/pipeline/quality.rs`, `runners/split.rs`,
`runners/stubs.rs` and `crates/flutterdec-decompiler/src/control_flow/regions.rs`.
Every one of those six files is product source that later work edits, so none of
them can carry a digest, and while the assertions lived inside them the whole
control-effect table, the well-formedness ruler's own tests, and every
identity-boundary gate could be deleted with every digest in this table still
matching. Section 15 moved each ruler into a test-only file of its own, protected
below, and left a hook behind. The product files themselves are deliberately
absent from this table for the same reason their hooks are: a digest over a file
ordinary work must edit fires on legitimate change and protects nothing.

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

Section 13 records the compiled inventory, its first 24 row-to-sentinel mappings,
and the twenty-two plants that prove it fires, thirteen of which leave `cargo test
--workspace` green with a quietly smaller suite. Section 12 records the
source-text guard it replaced, and section 11 that guard's narrower first
version. Section 15 records the nine IR and CFG boundary rows, the two new hook
families, the three new targets, and the thirty-two plants across every new
family, twenty-six of which silence a protected ruler while `cargo test
--workspace` still exits 0.

The loader families are `#[cfg(test)] mod tests;` in
`crates/flutterdec-decompiler/src/lib.rs`, the five `include!` lines in
`src/tests.rs`, the fourteen nested `include!` lines in the three second-level
loaders, the eight `#[cfg(test)] #[path = ...]` module declarations in
`crates/flutterdec-core/src/pipeline/runners.rs`,
`crates/flutterdec-core/src/pipeline/symbol_map.rs`, `pipeline/quality.rs`,
`pipeline/runners/split.rs`, `pipeline/runners/stubs.rs`,
`crates/flutterdec-ir/src/lib.rs`, `crates/flutterdec-ir/src/validate.rs` and
`crates/flutterdec-decompiler/src/control_flow/regions.rs`, the one `include!` in
`crates/flutterdec-decompiler/src/control_flow.rs`, which loads six product
modules beside its single oracle and so cannot have its include count pinned, and
the three emitter-repair module declarations in `emission_taxonomy.rs`,
`structured.rs`, and the decompiler `lib.rs`. Cargo automatically discovers the
protected integration targets under `crates/flutterdec-decompiler/tests/`,
`crates/flutterdec-core/tests/`, and `crates/flutterdec-ir/tests/`; `autotests =
false` would switch any crate's targets off wholesale.

| Path | sha256 |
| --- | --- |
| `crates/flutterdec-decompiler/src/tests.rs` | `a19fe0015869fbfeb259e28f6d4344e18a630edab92b2a7aef2a58811e3ef56b` |
| `crates/flutterdec-decompiler/tests/provenance_audit.rs` | `1627b7b9a0b5634fa3d76c9aa71c0d12dbb386371e26783b251b565467a3a34d` |
| `crates/flutterdec-decompiler/tests/loop_entry_provenance_audit.rs` | `02626ee1ba1b4b1b9905654a6254319ee413169341e43ddb74387813f7ecbfc7` |
| `crates/flutterdec-decompiler/src/tests/shared.rs` | `30ef9ef9d6b55acac8d41f5e557d38a78e5a60d2c28ac612e75ccfe80e376d3e` |
| `crates/flutterdec-decompiler/src/tests/golden_and_parser.rs` | `73a74b04ba294f1efc7faa5b067fdbd3c4cedc892c6d15068a07a98d656235ca` |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack.rs` | `1da9784956f09f2c5ede79236081389792d9f2abfac532abd39acfda0dc232c5` |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/structuring.rs` | `76bc84eabcda9bd9b34ee2d8b4ee21178782f0719077e7442670dfa4d1d32153` |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/order_totality.rs` | `690b389b5b83455902bdfa01855d8d70d3e58780d912f3990bbe0730346bdea9` |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/join_capture.rs` | `3d1db5b856a4cde4f98f57195b2828a4e3288003c20d0b0cd63a2812134d2b78` |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/annotation_caps.rs` | `242f6bb4a637dc466dca8be55a6e1671c6be90a7fb1d981a30b884103e2ae953` |
| `crates/flutterdec-decompiler/src/tests/cfg_and_stack/omitted_path_and_stack.rs` | `9b8d3117e3e1c510fbbf6a1a8217ac795968aba1661d938a02ffd0abedeaf79c` |
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
| `crates/flutterdec-core/src/pipeline/runners/tests.rs` | `7d1d87fa9401d07ab19b4bbb190edf1c53538a6da83ccd0e891b851b63200e63` |
| `crates/flutterdec-core/src/pipeline/symbol_map/tests.rs` | `019220e1a5915365e1663a36353ff3ba2177f567bb5e1094e6575b47f01b39f5` |
| `crates/flutterdec-ir/src/tests/control_effects.rs` | `9d6755a0001bfc839cd814fd326648e8cb8ccc186eeb3b30f3551dc8384fdf59` |
| `crates/flutterdec-ir/tests/branch_target_radix.rs` | `989b28c3c64a271eec2afc26eb4373e3325ac1f9b5d5d477a96a472d14c37af0` |
| `crates/flutterdec-ir/src/validate/tests.rs` | `2e3e3b3bd980c1edd2de99da166e09d5bf154cbed390e87f701a50f8316e8470` |
| `crates/flutterdec-core/src/pipeline/quality/control_effect_tests.rs` | `2dd80c63de5919b45de14a89259f90824d827c37ab7b7d943e58221587ed8c69` |
| `crates/flutterdec-core/src/pipeline/runners/split/identity_tests.rs` | `6de85f091ee07dd84c2469784f8f4288d982b83cf38ebffa1ae3691d28fdc4d2` |
| `crates/flutterdec-core/src/pipeline/runners/stubs/identity_tests.rs` | `a13fd1a26bafc8224edfbc9d1e8e1aa6441e935e50a5a4021315c119973e120d` |
| `crates/flutterdec-decompiler/src/control_flow/regions/identity_boundary_tests.rs` | `84b619616bab352c6e49fab1190d80a1df4606b691811c72835266003dfcf42d` |
| `crates/flutterdec-decompiler/src/control_flow/relation_oracle.rs` | `e53dd455ddbbdf2c0b00d184f1f2d788833cbfd6a0db070ad69b9372297da849` |
| `crates/flutterdec-decompiler/src/control_flow/emission_taxonomy_tests.rs` | `35263822b004ebe7083c9a2f7c0fdfff94202be641c46174e30161893fa9df94` |
| `crates/flutterdec-decompiler/src/control_flow/annotation_anchor_tests.rs` | `b7b4e4553fa93614ed7277c76a79a77c19884be64448a0c0192bcbc589252b7e` |
| `crates/flutterdec-decompiler/src/line_identity_tests.rs` | `cbc8bc9be4e84e90a3e1f52302c3bcdd16d4ec4b8c69ee7291f8324177b8e178` |
| `crates/flutterdec-decompiler/tests/helper_syntax_boundaries.rs` | `9a2832926b7871c2fd066277dd0ec6275e3ac9378c2fde519c7905b021ee7719` |
| `crates/flutterdec-decompiler/tests/rewrite_boundaries.rs` | `f5b4ec6ac0754bef3fb2bf6bf8b86681c8896765deabdb105a73fdee26c153e1` |
| `crates/flutterdec-decompiler/tests/unmodelled_write_effects.rs` | `4dac7f08cec237e3a611372f2e61ba766e8b7f88b616c514cabd0e6c11a991d4` |
| `crates/flutterdec-decompiler/tests/register_width_provenance.rs` | `e14bdeccf9337131055032444ecb4708d9a99e81cb1439f3164e47fb12585292` |
| `crates/flutterdec-decompiler/tests/atomic_rmw_effects.rs` | `32e189118af32909ef6bcd501924f0ecc35c2c70d8fe40ce9f1ca88758cde31d` |
| `crates/flutterdec-decompiler/tests/annotation_anchor_identity.rs` | `ed8d10588cf72adf42753152313e1795cde307eb9064a178093a18b0aa365004` |
| `crates/flutterdec-decompiler/tests/provenance_accounting.rs` | `adf39625d5a0c222f160cb5df2e916eb1fb3a4434f7d62aebad3f24e5a9b2bbd` |
| `crates/flutterdec-core/tests/pipeline_determinism.rs` | `0e278f988febcec7701881f4058ab98d9afe3cb67caabd73564e182886a763c8` |
| `crates/flutterdec-decompiler/tests/arm64_control_effects.rs` | `c50439b4e6157d6d8e5321a6c49c22a1a74a9405d7c7924b6235c84db8ca3617` |
| `crates/flutterdec-decompiler/tests/cfg_identity.rs` | `a5e0177808c50050bfd3517a7f89a234f650ae9ec01cc75b413ba8e2b4014ac8` |
| `crates/flutterdec-decompiler/tests/dfs_loop_address_invariance.rs` | `1c2c0403303e619de9fe840f62f61c1af92dbec77fe554fd66d3505755b37db3` |
| `crates/flutterdec-decompiler/tests/entry_loop_state_merge.rs` | `d562cc31ddd244e11d83a0a1bb6e3e4b3d76bf6a052417170496da4198d80dd9` |
| `crates/flutterdec-decompiler/tests/block_ledger_contract.rs` | `6dba60e7d22f129d5178f430174508d404dca297f7296984b69b3fe9ed248b21` |

Threshold rulers, protected by value rather than by digest because they live in
files this mission may legitimately touch:

Each is named by the symbol that carries it, not by a line, because both files
are product source:

- `--max-placeholder-ifs` default 0, the `max_placeholder_ifs` field of
  `DecompileCmd` in `crates/flutterdec-cli/src/main.rs`.
- `--max-unresolved-cf` default 0, the `max_unresolved_cf` field of the same
  struct.
- `--max-indirect-call-ratio` default 0.30, its `max_indirect_call_ratio` field.
- `--min-disassembly-ratio` default 0.80, its `min_disassembly_ratio` field.
- The four gate comparisons that read those four values into `failures`, in
  `quality_from_artifacts`, `crates/flutterdec-core/src/pipeline/quality.rs`.
- The six per line counters and their fixed order, `source_text_counters` in the
  same file.

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
  (`crates/flutterdec-decompiler/tests/provenance_audit.rs:185-252` at `1371e42`;
  that file has moved ten times since, so read the range with
  `git show 1371e42:<path>`. The test is
  `the_pre_call_audit_traces_each_candidate_and_its_checker_catches_a_wrong_path`,
  which still carries those three plants at `HEAD`).
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

How to read the records that follow. Every section from 10 on is an adjudication
record for one commit, and its counts, its gate output and its narrative describe
the tree at that commit, not the tree today. Only section 7 states the current
digests, and only a row that names no commit at all is claiming to be current. So
that this is mechanically checkable rather than a reading convention: every row
of every digest chain in sections 10 through 24 names the commit or the commit
interval that held its value, every one of those values that section 7 no longer
carries says where it was superseded, and every reproduce instruction for such a
row is a `git show <commit>:<path> | sha256sum`, never a `sha256sum` of the
worktree. A history row labelled current, or reproduced from worktree bytes, is a
defect of this document.

The same rule decides how to check a count. A number in a record from 10 on is
read at that record's own commit, so `git show <commit>:<path>` and the section 7
table as it stood there are what confirm or refute it, and a number that is right
today but was not right there is still a defect. The standing sections named at
the top of this document, 1 through 9, are the mirror image: their counts are
read at `HEAD`, except the `1371e42` state that section 2's Status column and
section 8 record. Every record from 10 on that needs to state where the table
stands today says so in a sentence of its own that points at section 7, rather
than letting a historical count be read as current.

## 10. Adjudication record: `scripts/ci-check.sh`

This is the section 9 record for the first three moves of `scripts/ci-check.sh`,
which at `e3d7d2f`, where this record was written, was the only protected path
whose digest had moved since `1371e42`. It is landed as its own documentation
commit, with no product or harness change alongside it. The chain in 10.1 was
carried one row further at `65abbf0`, whose move section 12 adjudicates. That is
no longer the standing of the table: section 7 records seven rows away from their
first pinned digest and twelve moves of this file, and names the section that
adjudicates each of the later ones.

### 10.1 Digest chain

The column order below puts the state before the digest deliberately, so that a
scanner looking for the section 7 row shape, a backticked path followed by a
backticked digest, does not read these history rows as protected-path rows.

| Commit | State | `scripts/ci-check.sh` sha256 |
| --- | --- | --- |
| `1371e42` | fixed reference, preserved | `9d994285d4605f77f725c1d2ba5035b2ce0ef4802bb82d33df94153a15c6d50d` |
| `282a1b3` | unchanged, docs-only commit | `9d994285d4605f77f725c1d2ba5035b2ce0ef4802bb82d33df94153a15c6d50d` |
| `4e8a9b2` | unchanged, harness added but not wired into the gate | `9d994285d4605f77f725c1d2ba5035b2ce0ef4802bb82d33df94153a15c6d50d` |
| `61e89fd` | intermediate | `675099447f611dcfc89cd26046ba6e6a7fd04f3ff94be54113e3c787ed21e412` |
| `5bf6595` | intermediate | `6ee0cdf976f4fe02c1b3bebb4495bd2dfe34dc1fbd431b1ce9b52201eebbf878` |
| `059c7b1`, `e2e66a7` | unchanged | `6ee0cdf976f4fe02c1b3bebb4495bd2dfe34dc1fbd431b1ce9b52201eebbf878` |
| `5f6a39f` | third move, the value this record was written against | `2f76a8b9abac96db026386c0626d248ade81e9690e563cbaaa901b86472b4457` |
| `4c127ab` through `9757328` | unchanged, docs-only and harness-only commits | `2f76a8b9abac96db026386c0626d248ade81e9690e563cbaaa901b86472b4457` |
| `65abbf0` | fourth move, adjudicated in section 12, superseded at `c95daa6` | `171aa8894675ed2c90ff40c9d6a136bd791c3ae0d51c7965617e682b31d2f067` |

Reproduce any row with `git show <commit>:scripts/ci-check.sh | sha256sum`,
against the commit that row names. Every value in this chain is historical: the
last of them was held by `65abbf0` alone, and section 7 records
`9dac174b7a2a4e3a0d14d182d292dac0dbc8b6c63679e859da7cc8dad21ea45e` today,
adjudicated in section 23. `sha256sum scripts/ci-check.sh` therefore reproduces
no row of this table.

The fixed reference is preserved two ways: the `1371e42` digest is recorded above
and in section 7, and the file itself is recoverable verbatim from the reference
commit, which is never rewritten, force pushed, or rebased.

### 10.2 Exact diff intent, per step

`1371e42` to `61e89fd`, digest `9d994285...` to `675099447f...`. Adds a clippy
lane and a test lane for the benchmark harness, plus the usage line and the
paragraph explaining why they are needed. From `61e89fd` onward the harness is
deliberately not a workspace member, so `cargo clippy --workspace` and
`cargo test --workspace` do not reach it, and that exclusion is what keeps its
`bench-spans` instrumentation out of every existing check. Without these two
lanes the harness would be the one part of the repository no gate covers.

The qualifier is load bearing and the exclusion is not a property of the branch:
at `4e8a9b2` the harness was a workspace member and unification did turn
`bench-spans` on for product builds. That transient is disclosed in
[research-ir-cfg-emitter.md](research-ir-cfg-emitter.md) section 17, with the
interval, the probes and the semantic evidence. No accepted measurement is on
that revision.

`61e89fd` to `5bf6595`, digest `675099447f...` to `6ee0cdf976...`. Adds
`cargo fmt --manifest-path crates/flutterdec-bench/Cargo.toml --all --check` for
the same reason: `--all` means every member of the manifest's own workspace, so
the root `cargo fmt --all` does not reach the harness either. The usage line
changes from "clippy and tests" to "fmt, clippy and tests" to match.

`5bf6595` to `5f6a39f`, digest `6ee0cdf976...` to `2f76a8b9...`. Adds
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
   harness commits `61e89fd`, `5bf6595`, and `5f6a39f`, rather than landing as a
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

This is the section 9 record for the first move of
`crates/flutterdec-decompiler/tests/provenance_audit.rs`, which at `9757328`,
where this record was written, was the second protected path whose digest had
moved since `1371e42`. Section 7 records ten moves of this file and names the
section that adjudicates each of the later nine. It is landed as its own commit,
carrying only this file and the protected test file, with no product change
alongside it, and before any product source edit of this mission.

### 11.1 Digest chain

Column order matches section 10.1, state before digest, so a scanner looking for
the section 7 row shape does not read these history rows as protected-path rows.

| Commit | State | `tests/provenance_audit.rs` sha256 |
| --- | --- | --- |
| `1371e42` | fixed reference, preserved | `e0b5c675b2510d8c17c05b15a4a33341ba6a24cbec4336512cf63028527ff3b8` |
| `282a1b3` through `02bc42c` | unchanged, docs-only and harness-only commits | `e0b5c675b2510d8c17c05b15a4a33341ba6a24cbec4336512cf63028527ff3b8` |
| `9757328` | first move, adjudicated here, superseded at `65abbf0` | `8124346801612c56e9580d293c16a4e24593df175f8e7e376f16748a26560c0e` |

Reproduce any row with
`git show <commit>:crates/flutterdec-decompiler/tests/provenance_audit.rs | sha256sum`,
against the commit that row names. Both values are historical: `9757328` alone
held the second, and section 7 records
`1627b7b9a0b5634fa3d76c9aa71c0d12dbb386371e26783b251b565467a3a34d` today,
adjudicated in section 24. `sha256sum` on the worktree file reproduces neither.

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

Run in disposable worktrees detached at `02bc42c` with the guard copied in, one
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
| `282a1b3` through `02bc42c` | unchanged | `e0b5c675b2510d8c17c05b15a4a33341ba6a24cbec4336512cf63028527ff3b8` |
| `9757328` | first, narrow guard, adjudicated in section 11 | `8124346801612c56e9580d293c16a4e24593df175f8e7e376f16748a26560c0e` |
| `65abbf0` | second move, superseded at `c95daa6`, adjudicated in section 13 | `b5712a66b0f6472a726d4d253555272b54322cd69c3fdb1a3bed514de8cb9765` |

`scripts/ci-check.sh`, continuing the chain in section 10.1:

| Commit | State | sha256 |
| --- | --- | --- |
| `5f6a39f` through `9757328` | prior, adjudicated in section 10 | `2f76a8b9abac96db026386c0626d248ade81e9690e563cbaaa901b86472b4457` |
| `65abbf0` | fourth move, superseded at `c95daa6`, adjudicated in section 13 | `171aa8894675ed2c90ff40c9d6a136bd791c3ae0d51c7965617e682b31d2f067` |

`.github/workflows/ci.yml` is not a section 7 row and does not become one: it is CI
configuration that later work may legitimately edit, exactly like the threshold
rulers at the end of section 7. Recorded for reproducibility only:
`817f472151aa1553e2a25014bad95cf4418aca116cdfb7ca1fa9f2e9d6599a3c` at `1371e42`
through `9757328`, `c51642b043cfa254c454282aee5d14b24d899d29bcda83d8e42a9e1da9968c55`
in this commit. Its one load-bearing line is asserted by value by the guard, not
by digest.

Reproduce any row with `git show <commit>:<path> | sha256sum`, against the commit
that row names. Both chains end at `65abbf0` and both of those values were
superseded at `c95daa6`, so `sha256sum <path>` reproduces neither: section 7
records `1627b7b9...` and `9dac174b...` today. The fixed reference is preserved
two ways: the `1371e42` digests are recorded above and in section 7, and both
files are recoverable verbatim from the reference commit, which is never
rewritten, force pushed, or rebased.

### 12.2 Exact diff intent

`git diff --numstat 9757328` for the three files: 322 insertions and 37 deletions
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
git show 9757328:scripts/ci-check.sh > /tmp/cic-prev.sh
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
`9757328` is absent now, and the same command run against `1371e42` reports only
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

One disposable worktree detached at `9757328`, with this commit's guard and both
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
   `9757328` and of `1371e42`.
2. Test. `the_protected_oracle_loader_chain_is_intact` is itself the test, and
   `scripts/ci-check.sh` step 7 is what makes it unskippable. It fails on the old
   behavior in the sense that matters for a guard: at `9757328` plants 4 through
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
| `65abbf0` | prior, source-text guard, adjudicated in section 12 | `b5712a66b0f6472a726d4d253555272b54322cd69c3fdb1a3bed514de8cb9765` |
| `c95daa6` through `af32bb1` | third move, adjudicated here, superseded at `6d501c8` | `1bda72504e7ada1c8a2e7798ca314b3843ebc6cf8b8202851de42dd542573abd` |

`scripts/ci-check.sh`, continuing 12.1:

| Commit | State | sha256 |
| --- | --- | --- |
| `65abbf0` | prior, adjudicated in section 12 | `171aa8894675ed2c90ff40c9d6a136bd791c3ae0d51c7965617e682b31d2f067` |
| `c95daa6` through `af32bb1` | fifth move, adjudicated here, superseded at `6d501c8` | `386e0f2a22a25c774ff43da8621e947d9c3a4137e57a5d8ee6bbad973eb25c48` |

`scripts/check-oracle-inventory.py` is new in this commit and has no prior state.
Its digest at `c95daa6` is
`d882132e87cb4625ebdac88ab310e405b00133bd546e172db282be7e1bbf47bf`, which is the
value section 7 carried for it from `c95daa6` through `af32bb1`; it has moved
seven times since, the first of them at `6d501c8` in section 15, and section 7
records `3900f505ea8aea59500c99fcf598013cffac55e9128ec9498f7811738bcbf71a`
today. It is protected because it is now the ruler: weakening it, for instance by
dropping a sentinel or by accepting a target it failed to list, would make the
inventory pass over a silenced oracle.

`.github/workflows/ci.yml` is still not a section 7 row, for the reason 12.1
gives. Recorded for reproducibility only:
`c51642b043cfa254c454282aee5d14b24d899d29bcda83d8e42a9e1da9968c55` at `65abbf0`,
`6866ce3d8f8f96d8f8ed59c932f1002e962763635702de687cd2dabb18b68c80` in this
commit. Both of its load-bearing lines are asserted by value by the guard, not by
digest.

Reproduce any row with `git show <commit>:<path> | sha256sum`, against the commit
that row names. Every value in both chains is historical: both were superseded at
`6d501c8` in section 15, so `sha256sum <path>` reproduces neither.

### 13.2 Exact diff intent

`git diff --numstat 65abbf0` for the four non-protocol files: 59 insertions and 10
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
git show 65abbf0:scripts/ci-check.sh > /tmp/cic-prev.sh
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
at `65abbf0` is absent now; one command is added,
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

One disposable worktree detached at `65abbf0` with this commit's four files copied
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
   behavior in the sense that matters: at `65abbf0` the nine text-preserving
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

## 14. Adjudication record: the indirect-control class in the L1 table

Section 3 is the L1 expected-value table, the highest layer in this hierarchy.
One cell in it named the wrong class for `BR Xn`, and the paragraph beneath it
described `BR` and `BRK` as the two rows the pipeline gets wrong, which stopped
being true at `ac544ca`. This record adjudicates both edits.

This protocol carries no digest row in section 7 and no oracle-inventory
sentinel, so no protected digest moves here and nothing mechanical fails if this
record is wrong. It is adjudicated anyway, because section 3 is an L1 ruler in
substance: it is the written-from-the-manual source of every expected value the
instruction-classification cases are held to, and editing an expected value after
the candidate exists is precisely the shape section 5 forbids. What follows is
the evidence that the edit did not follow the code.

### 14.1 What changed, and what did not

Two edits, both inside section 3. Nothing else in this protocol is touched.

| Cell | Before | After |
| --- | --- | --- |
| `BR Xn`, Required class | `jump` | `indirect branch` |
| `BR Xn`, Architectural effect | `indirect branch, no link, does not return here` | unchanged |
| `BR Xn`, Ends block | `yes` | unchanged |
| `BR Xn`, Required edges | `none unless a target set is independently recovered` | unchanged |
| `BR Xn`, Forbidden edges | `fallthrough` | unchanged |
| `BRK #imm`, all five value columns | `breakpoint instruction exception, control does not continue` / `trap` / `yes` / `none` / `fallthrough` | unchanged |
| corroborating paragraph | "the two rows this pipeline currently gets wrong" | current code and test citations |

The edge oracle was not weakened, and it was not touched. `BR` and `BRK` already
required no edges and already forbade fallthrough in this table, from the day it
was written. Those two columns are the ones a candidate could gain something by
moving, and both rows are byte-identical to the original. Reproduce the original
rows with

```
git show 282a1b3:docs/oracle-protocol-ir-cfg-emitter.md \
  | grep -E '^\| `(BR Xn|BRK #imm)`'
```

which prints

```
| `BR Xn` | indirect branch, no link, does not return here | jump | yes | none unless a target set is independently recovered | fallthrough |
| `BRK #imm` | breakpoint instruction exception, control does not continue | trap | yes | none | fallthrough |
```

Column five is `none unless a target set is independently recovered` and `none`,
column six is `fallthrough` in both, and the current table says the same. Only
the third column of the first row differs. `BRK` is unchanged in every column,
because `trap` was already the right class name and `IROp::Trap` implements it
under that name.

The section 2 rows IR-05 and IR-06 are also untouched. Their Status column is
explicitly "the state at `1371e42`", where both instructions did fall through, so
`fails by inspection, risk R1` is still the correct historical value and
rewriting it would be rewriting frozen history. The present state is recorded in
section 3 and in section 18 of
[research-ir-cfg-emitter.md](research-ir-cfg-emitter.md), not in a column pinned
to the reference commit.

### 14.2 The semantic class split

`jump` in this table is the class of `B label`: the block ends and control
transfers to a destination the instruction stream states. The pipeline
implements it that way and nowhere else. `IROp::Jump` is the one class whose
target is parsed and turned into an edge,
`crates/flutterdec-ir/src/lib.rs:300-306`, and `B label`'s Required edges cell is
`the label block` for exactly that reason.

`BR Xn` shares the block-ending half of that and none of the destination half.
Its destination is a register value this pipeline does not recover, which is why
its own Required edges cell has always read `none unless a target set is
independently recovered`. Holding both instructions in one class therefore
demands one of two wrong things: either `BR` renders as a resolved jump to a
destination nobody recovered, or `Jump` acquires a second meaning, "sometimes
carries a target and sometimes does not", which is a wildcard none of the four
exhaustive `match` sites on `IROp` can enforce.

So the original cell was a compromise, not a mistake: with only `jump`, `branch`,
`call`, `return`, `trap` and `runtime check` available, `jump` was the closest
name for behavior the table already specified correctly in its other columns.
`ac544ca` created the class that behavior deserves, `IROp::IndirectBranch`
(`crates/flutterdec-ir/src/lib.rs:14-20`), whose doc comment states the same
split in the same terms, and this edit makes the table name it. Both halves of
the split are named now: `indirect branch` for the register destination, `trap`
for `BRK`, which resumes nothing and is distinct from `return`, which resumes the
caller.

The split is semantic and not behavioral, and that is checkable rather than
asserted. The machine-readable restatement of this table is `CONTROL_EFFECTS`,
`crates/flutterdec-ir/src/lib.rs:451-540`, whose fields are the Rust variant, the
block-ending column and the successor-start list. It reads the Rust variant name,
never this document's prose class, so no value of the Required class cell can
make a case pass or fail. What the two driving tests compare is the behavior:
`every_arm64_control_effect_has_exactly_the_documented_edges` (`:563`) pins the
Required and Forbidden edges columns for all twelve rows, and
`only_a_control_effect_that_ends_a_block_makes_the_next_instruction_a_leader`
(`:637`) pins the Ends block column. Neither could be satisfied by renaming a
class here.

### 14.3 Exact diff intent

One file, `docs/oracle-protocol-ir-cfg-emitter.md`, and nothing else in the
commit. `git diff --numstat b2ed966` reports 292 insertions and 5 deletions. The
deletion count is 5 rather than 6 because the last line of the replaced
paragraph, the one holding the `stubs.rs:461` citation, survives verbatim into
the replacement and the diff keeps it as context.

Change one, in the section 3 table: the third column of the `BR Xn` row, `jump`
to `indirect branch`. One line, one cell, no other cell of that row and no other
row.

Change two, replacing the five-line paragraph under the table. The old paragraph
opened "Corroborating repository evidence for the two rows this pipeline
currently gets wrong" and cited `split.rs:141-150` and `stubs.rs:461`. Both of
those were downstream corroboration, not the classifier, and the sentence they
supported has been false since `ac544ca`. The replacement states the `1371e42`
state as history, then cites the classifier itself, the leader rule, the
successor rule, the twelve-row table and the two tests that drive it, and keeps
both downstream citations with `split.rs` renumbered to `174-183` for its current
position. `is_terminator` and the `stubs.rs` dispatch-stub tail are both
byte-identical to `1371e42`:

```
git show 1371e42:crates/flutterdec-core/src/pipeline/runners/split.rs \
  | sed -n '/^fn is_terminator/,/^}/p' | sha256sum
sed -n '/^fn is_terminator/,/^}/p' \
  crates/flutterdec-core/src/pipeline/runners/split.rs | sha256sum
```

Both print
`4032e0a8d7fe3b7da0fefb42f6fe05dbfc31d39987e127964228458b78d8864e`.

Change three is this section, appended after section 13.

No digest chain table is needed, because no protected path moves. For
reproducibility only, this protocol's own sha256 at `b2ed966`, the commit
immediately before this one, is
`904695f860a033ad099e8568f8cd2bcc13eebfaa03748f163ddab0824ccc3a88`, recoverable
with `git show b2ed966:docs/oracle-protocol-ir-cfg-emitter.md | sha256sum`. The
post-change digest is deliberately not recorded here: this record is inside the
file it would digest. Recompute it with
`sha256sum docs/oracle-protocol-ir-cfg-emitter.md`.

### 14.4 Code before doc

The behavior landed first and this document follows it. That direction is the
whole point of the record, because a class name written into an L1 table
*before* the code would be an expected value; written after, with every
behavioral column already fixed, it is a reconciliation.

| Commit | Time | Paths | What |
| --- | --- | --- | --- |
| `282a1b3` | 2026-08-18T03:58:59Z | 3, all docs | this protocol written, section 3 table included, both indirect-control rows already forbidding fallthrough |
| `ac544ca` | 2026-08-18T09:11:58-04:00 | 7, 0 docs | `IROp::IndirectBranch` and `IROp::Trap`, the classifier, leader and successor rules, and the tests |
| `dff08ac` | 2026-08-18T09:28:43-04:00 | 8, 0 docs | block-identity validation at every consumer boundary |
| `325447f` | 2026-08-18T09:34:35-04:00 | 4, 0 docs | one canonical edge-rebuild path |
| `ecc2892` | 2026-08-18T09:39:05-04:00 | 1, 0 docs | guard-prune scope pinned |
| `b2ed966` | 2026-08-18T10:27:41-04:00 | 1, all docs | research doc section 18, risk R1 closed |
| this commit | after `b2ed966` | 1, all docs | section 3 reconciliation and this record |

Reproduce with `git log --format='%h %cI %s' c95daa6..fe4b31c`, which lists the six
rows below `282a1b3` and nothing else, and
`git diff-tree -r --no-commit-id --name-only <commit>` for the path counts. The
range ends at this record's own commit, not at `HEAD`, so it stays the six rows
above as the branch grows.

The behavioral columns of both rows predate all of it: they were written at
`282a1b3` from `DDI 0487` C6.2, before any of this code existed, and 14.1 shows
they are unchanged. The one cell written afterwards is a class name that 14.2
shows no test reads. So no expected value in this table was produced by running
the candidate, which is the section 5 rule this edit is most exposed to.

### 14.5 Accepted gate evidence

Run at this record's own worktree, with the section 3 edit and this record in
place. The counts below are that commit's, not today's.

- `NIX_CONFIG='experimental-features = nix-command flakes' scripts/ci-check.sh`
  exits 0 and prints `[ci-check] all checks passed`. All thirteen lanes are
  green, 20 `test result:` lines and 504 tests passed with 0 failed across the
  whole script.
- The `cargo test --workspace` lane inside it is 17 result lines, 466 passed and
  0 failed. `cargo fmt --all --check` and
  `cargo clippy --workspace --all-targets -- -D warnings` are lanes 2 and 6 and
  both pass.
- `scripts/check-oracle-inventory.py` is lane 8 and prints
  `[oracle-inventory] 24 protected oracle rows in
  docs/oracle-protocol-ir-cfg-emitter.md` and
  `[oracle-inventory] ok, 24 protected oracles are compiled`. This is the one
  gate that parses this document, and its parse is bounded to the section 7
  Oracle test files table, so it is what proves an edit to section 3 and an
  appended section 14 leave the table it reads undisturbed. The same is true of
  `the_protected_oracle_loader_chain_is_intact`, which passes in lane 7.
- The 47 section 7 rows all matched the worktree at this commit, 0 mismatches,
  recomputed with `sha256sum` per row. A doc-wide scan for the section 7 row
  shape also returned 47, so neither of the two tables added by this record leaks
  into that count.

The tests named in 14.2 fail on the old behavior, which is section 9 step 2 and
is measured rather than argued. In a disposable worktree at this commit's tree,
deleting only the `"br"` and `"brk"` arms from `llir_from_disasm`
(`crates/flutterdec-ir/src/lib.rs:185-191`) restores the `1371e42`
classification exactly: both mnemonics then fall to the `IROp::Other` default at
`:132` and take the fallthrough arm of the successor match, which is what
`1371e42` did, since it had no arm for either mnemonic. Against that plant,
`cargo test --workspace --no-fail-fast` exits 101 with 6 failures in 2 of the 17
result lines and 460 of 466 still passing:

| Test binary | Result | Failing tests |
| --- | --- | --- |
| `flutterdec-ir` lib | 16 passed, 4 failed | `every_arm64_control_effect_has_exactly_the_documented_edges`, `only_a_control_effect_that_ends_a_block_makes_the_next_instruction_a_leader`, `an_indirect_branch_keeps_its_register_and_takes_no_edge`, `a_trap_ends_the_block_with_no_successors` |
| `flutterdec-core` lib | 97 passed, 2 failed | `quality_tests::serialized_ir_states_every_control_effect_and_its_edges`, `quality_tests::the_pipeline_reports_an_indirect_branch_as_unresolved_control_flow` |

Both tests that drive the section 3 table are in that list, which is what step 2
needs, and so are both pipeline tests in
`crates/flutterdec-core/src/pipeline/quality.rs` (`:243` for the serialized edges
and `:307` for `unresolved_cf`).

The three cross-emitter tests in
`crates/flutterdec-decompiler/tests/arm64_control_effects.rs` (`:156`, `:177`,
`:203`) do *not* fail under this plant, and the decompiler unit suite stays at
268 passed. That is not a gap in them and it is recorded rather than glossed:
they build their `FunctionIr` by hand, constructing `IROp::IndirectBranch` and
`IROp::Trap` directly (`:50-56`), so they pin what the emitters do with those
classes and are deliberately independent of which mnemonic the classifier maps
to them. The classifier is what this plant breaks, and the classifier is covered
by the four `flutterdec-ir` rows above. The two layers are separate oracles on
purpose, and a plant that reaches only one of them is the evidence that they are.

### 14.6 No product, test, or digest change

This commit changes one file and it is a document. `git diff-tree -r
--no-commit-id --name-only` for it lists `docs/oracle-protocol-ir-cfg-emitter.md`
and nothing else: no product source, no test, no fixture, no golden, no manifest,
no script and no CI lane.

Every protected digest row was exact. The 47 rows section 7 held at this commit
all matched the worktree, and the intersection of those 47 paths with the 14 paths
changed by the whole IR work, `git diff --name-only c95daa6 fe4b31c^`, was empty.
So none of the five commits this record reconciles against moved a protected
digest either, and at this commit exactly two rows had moved since `1371e42`:
`scripts/ci-check.sh`, adjudicated in sections 10, 12 and 13, and
`crates/flutterdec-decompiler/tests/provenance_audit.rs`, adjudicated in sections
11, 12 and 13. Section 13's third mover is not one of them:
`scripts/check-oracle-inventory.py` did not exist at `1371e42` and joined section 7
there as a new row, at the first pinned digest it still carried here, which is the
distinction section 13 draws when it names that row as a new one.

Five more rows have left their first pinned digest since this commit, first
`scripts/check-oracle-inventory.py` in section 15, then
`crates/flutterdec-core/src/pipeline/runners/tests.rs` and
`crates/flutterdec-decompiler/src/tests/cfg_and_stack/omitted_path_and_stack.rs`
in section 16, `scripts/prov_cross_audit_reconcile.py` in section 18, and
`crates/flutterdec-decompiler/src/control_flow/relation_oracle.rs` in section 19.
Two plus five is the standing count of seven that section 7 carries.

That is also the honest limit of this record. Nothing under `crates/flutterdec-ir/`
carries a section 7 digest row or an oracle-inventory sentinel, so the twelve-row
control-effect table and the tests that drive it are not themselves protected
rulers yet. This edit does not change that either way, and closing it is separate
work.

### 14.7 Section 9 steps

1. Invariant. L1, and the invariant is I5: no block whose last instruction is an
   unconditional jump, an indirect branch, a return, or a trap has a fallthrough
   successor. It is not moved by this edit; it is what the unchanged Required and
   Forbidden edges cells state, sourced from `DDI 0487` C6.2 at `282a1b3`. No
   product output changed here, because this commit contains no product change.
2. Test. `every_arm64_control_effect_has_exactly_the_documented_edges` and
   `only_a_control_effect_that_ends_a_block_makes_the_next_instruction_a_leader`,
   both in `crates/flutterdec-ir/src/lib.rs`, are the two that read this table.
   They fail on the old behavior and pass on the new one, together with four
   others, measured in 14.5.
3. Diff and digests. Recorded in 14.3, with the reproducing commands. No
   protected digest moves, and 14.6 shows the section 7 table is exact.
4. Original reference preserved. The original section 3 rows are quoted verbatim
   in 14.1 and recoverable from `282a1b3`, and the pre-change whole-file digest is
   in 14.3. `1371e42` is immutable. `282a1b3` is the fixed rewritten equivalent
   and is not rewritten again.
5. Own commit. Satisfied. This record and the section 3 reconciliation land as one
   documentation commit, `docs(protocol): reconcile indirect control class`, with
   no product, test or harness change in it, and after the code it reconciles
   against rather than before it.
6. L5 re-run. Recorded in 14.5.

## 15. Adjudication record: the IR and CFG boundary oracles

Between `c95daa6` and `af32bb1` this mission added, in eleven commits, the ARM64
control-effect table, the `FunctionIr` well-formedness ruler and its own
assertions, identity gates at the region-analysis, record-splitter and no-return
prune boundaries, a hand-derived CFG relation oracle over twelve literal graphs
with a twenty-process determinism check, and two integration test files. Not one
of them had a row in section 7 or a sentinel in
`scripts/check-oracle-inventory.py`.

Every one could therefore be deleted outright with every digest in section 7 still
matching, every gate in this protocol still green, and `cargo test --workspace`
still exiting 0 with a quietly smaller suite. That is the section 5 failure the
compiled inventory exists to remove, reopened by new work faster than the
inventory was extended to cover it.

Six of those rulers were written as inline `#[cfg(test)] mod` blocks inside
product source: `crates/flutterdec-ir/src/lib.rs`,
`crates/flutterdec-ir/src/validate.rs`,
`crates/flutterdec-core/src/pipeline/quality.rs`,
`crates/flutterdec-core/src/pipeline/runners/split.rs`,
`crates/flutterdec-core/src/pipeline/runners/stubs.rs`, and
`crates/flutterdec-decompiler/src/control_flow/regions.rs`. A digest over any of
those six is worthless, because ordinary product work edits all six. This record
moves each ruler into a test-only file that carries a digest, leaves the hook
behind in the product file, adds the three remaining test-only files to the table,
and extends the inventory to prove all nine by compilation.

No assertion is added, weakened, reordered or removed. No product logic changes.
No mutable product file and no manifest receives a digest row.

### 15.1 Digest chains

Column order matches sections 10.1, 11.1, 12.1 and 13.1, state before digest, so a
scanner looking for the section 7 row shape does not read these history rows as
protected-path rows.

`crates/flutterdec-decompiler/tests/provenance_audit.rs`, continuing 13.1:

| Commit | State | sha256 |
| --- | --- | --- |
| `c95daa6` | prior, adjudicated in section 13 | `1bda72504e7ada1c8a2e7798ca314b3843ebc6cf8b8202851de42dd542573abd` |
| `ac544ca` through `af32bb1` | unchanged, eleven commits of product and oracle work | `1bda72504e7ada1c8a2e7798ca314b3843ebc6cf8b8202851de42dd542573abd` |
| `6d501c8` through `5fa97f2` | fourth move, adjudicated here, superseded at `7b8628a` | `e93e04f71f67dc57379fcca164af80d58a403889095bcc4aced48de990b44c59` |

`scripts/ci-check.sh`, continuing 13.1:

| Commit | State | sha256 |
| --- | --- | --- |
| `c95daa6` | prior, adjudicated in section 13 | `386e0f2a22a25c774ff43da8621e947d9c3a4137e57a5d8ee6bbad973eb25c48` |
| `ac544ca` through `af32bb1` | unchanged | `386e0f2a22a25c774ff43da8621e947d9c3a4137e57a5d8ee6bbad973eb25c48` |
| `6d501c8` through `5fa97f2` | sixth move, adjudicated here, superseded at `7b8628a` | `ec5e015bc65317c8b477c582b52a9a6d91c618e6becfe31da019b4bb34995401` |

`scripts/check-oracle-inventory.py`, first move since section 13 created it:

| Commit | State | sha256 |
| --- | --- | --- |
| `c95daa6` | as created, adjudicated in section 13 | `d882132e87cb4625ebdac88ab310e405b00133bd546e172db282be7e1bbf47bf` |
| `ac544ca` through `af32bb1` | unchanged | `d882132e87cb4625ebdac88ab310e405b00133bd546e172db282be7e1bbf47bf` |
| `6d501c8` through `b396a62` | first move since section 13, adjudicated here, superseded at `c58013d` | `b8e06c148c0268f23acbb9547e5b9248b3f4ebc6903a48e8d21112be41e3ef49` |

The nine new rows have no prior digest state. Six are files this commit creates by
moving an inline module out of the product file named beside them; three already
existed as test-only files and were simply unprotected. The pre-move location is
given so the moved text is recoverable from history:

| New protected row | Origin | State before this commit |
| --- | --- | --- |
| `crates/flutterdec-ir/src/tests/control_effects.rs` | `crates/flutterdec-ir/src/lib.rs:434-740` at `af32bb1` | inline `mod tests`, unprotected |
| `crates/flutterdec-ir/src/validate/tests.rs` | `crates/flutterdec-ir/src/validate.rs:241-695` at `af32bb1` | inline `mod tests`, unprotected |
| `crates/flutterdec-core/src/pipeline/quality/control_effect_tests.rs` | `crates/flutterdec-core/src/pipeline/quality.rs:154-352` at `af32bb1` | inline `mod quality_tests`, unprotected |
| `crates/flutterdec-core/src/pipeline/runners/split/identity_tests.rs` | `crates/flutterdec-core/src/pipeline/runners/split.rs:428-584` at `af32bb1` | inline `mod tests`, unprotected |
| `crates/flutterdec-core/src/pipeline/runners/stubs/identity_tests.rs` | `crates/flutterdec-core/src/pipeline/runners/stubs.rs:1068-1279` at `af32bb1` | inline `mod prune_tests`, unprotected |
| `crates/flutterdec-decompiler/src/control_flow/regions/identity_boundary_tests.rs` | `crates/flutterdec-decompiler/src/control_flow/regions.rs:442-533` at `af32bb1` | inline `mod identity_boundary_tests`, unprotected |
| `crates/flutterdec-decompiler/src/control_flow/relation_oracle.rs` | already test-only, added at `5ec63ef` | unprotected, no sentinel |
| `crates/flutterdec-decompiler/tests/arm64_control_effects.rs` | already test-only, added at `ac544ca` | unprotected, not named by any lane |
| `crates/flutterdec-decompiler/tests/cfg_identity.rs` | already test-only, added at `dff08ac` | unprotected, not named by any lane |

Their digests at `6d501c8` are the nine rows this commit adds to the section 7
Oracle test files table. Eight of the nine still hold that first pinned digest.
The exception is
`crates/flutterdec-decompiler/src/control_flow/relation_oracle.rs`, which joined
at `75fe720e04cfa6bbb859981f1b39ebba0e0ed932e8973d3bab730058cedcfa96` and has
moved twice since, in section 19 and in section 21, which is why section 7 counts
it among the seven.

`.github/workflows/ci.yml` is still not a section 7 row, for the reason 12.1
gives. Recorded for reproducibility only:
`6866ce3d8f8f96d8f8ed59c932f1002e962763635702de687cd2dabb18b68c80` at `af32bb1`,
`479cd6f7ea7e0cbdce791244f9e3c2560b536d62cfcddc3d4a43601c379920eb` in this
commit. Its load-bearing lines are asserted by value by the guard, not by digest.

Reproduce any row with `git show <commit>:<path> | sha256sum`, against the commit
that row names. Every value in the three chains is historical: the two moved at
`7b8628a` in section 19 and the third at `c58013d` in section 17, so
`sha256sum <path>` reproduces none of them.

### 15.2 Exact diff intent

`git diff --numstat af32bb1` for the non-protocol files: 10 insertions and 308
deletions in `crates/flutterdec-ir/src/lib.rs`, 7 and 454 in
`crates/flutterdec-ir/src/validate.rs`, 9 and 199 in
`crates/flutterdec-core/src/pipeline/quality.rs`, 11 and 159 in
`runners/split.rs`, 17 and 215 in `runners/stubs.rs`, 8 and 91 in
`crates/flutterdec-decompiler/src/control_flow/regions.rs`, 105 and 10 in
`crates/flutterdec-decompiler/tests/provenance_audit.rs`, 68 and 0 in
`scripts/check-oracle-inventory.py`, 6 and 6 in `scripts/ci-check.sh`, and 1 and 1
in `.github/workflows/ci.yml`. Six files are new.

**The moved assertions are the same text.** Every deletion in the six product
files is a line that reappears verbatim in the test-only file beside it. Proved by
token stream rather than claimed: take the cut range out of the pre-move file at
`af32bb1`, take the new file from its first item onward, collapse every run of
whitespace in both, and compare. `cargo fmt` legitimately rejoins one wrapped line
in `control_effects.rs`, which gained four columns of room when the block was
dedented out of its `mod` block, and that is the only formatting difference in the
six files.

| Moved block | Collapsed length | sha256 of the collapsed stream, first 16 |
| --- | --- | --- |
| `crates/flutterdec-ir/src/tests/control_effects.rs` | 7379 chars | `b33c4ed788a0287d` |
| `crates/flutterdec-ir/src/validate/tests.rs` | 12189 chars | `8101b75114624ca0` |
| `crates/flutterdec-core/src/pipeline/quality/control_effect_tests.rs` | 5901 chars | `ef09d8e28f59288c` |
| `crates/flutterdec-core/src/pipeline/runners/split/identity_tests.rs` | 4478 chars | `8c8dc5aa2b9f95e2` |
| `crates/flutterdec-core/src/pipeline/runners/stubs/identity_tests.rs` | 6539 chars | `96a8f03a7ffc385b` |
| `crates/flutterdec-decompiler/src/control_flow/regions/identity_boundary_tests.rs` | 1932 chars | `6237541843316f88` |

Each hash is over the pre-move text and equals the hash over the post-move text;
the assertion count is 4, 10, 2, 3, 3 and 2 respectively, unchanged in every file.

**What is added to the six product files.** One `#[cfg(test)] #[path = ...] mod`
declaration each, with a comment saying why the assertions are not inline and why
the declaration itself cannot be digested. Nothing else, with one exception: five
test helpers keep the fixtures shared instead of duplicated, so they change
visibility and nothing else. `ins` in `crates/flutterdec-ir/src/lib.rs`'s
`mod tests`, `ins` and `two_functions` in `runners/split.rs`'s `mod tests`, and
`call`, `other` and `blk` in `runners/stubs.rs`'s `mod prune_tests` become
`pub(super)` so the moved tests can import them. `blk` is also rewrapped across
five lines by `cargo fmt` because the longer signature no longer fits. No
pre-existing test moves, and no pre-existing assertion changes.

**`crates/flutterdec-decompiler/tests/provenance_audit.rs`.** Nine rows are added
to `loader_map()`: six `Hook::Module` rows for the new `#[path]` declarations, one
`Hook::Include` row for `relation_oracle.rs`, and two `Hook::Autotest` rows for
the new integration tests. `Hook::Include` gains an `exclusive` field. The four
loaders under `src/tests/` are exclusive, so their `include!` count is still
pinned exactly and they still cannot grow an unrecorded oracle.
`crates/flutterdec-decompiler/src/control_flow.rs` is not, because it loads five
product modules beside its one oracle, and pinning that number would fire the
moment a control-flow module is added, which is the same defect that keeps every
hook out of section 7. The ten deleted lines are the previous `Hook::Include`
shape and the doc paragraph describing the family counts. `IR_MANIFEST` joins the
`test = false` / `harness = false` / `autotests = false` scan, which previously
covered only the decompiler and core manifests, so `flutterdec-ir` could have been
switched off silently. No assertion is removed or demoted.

**`scripts/check-oracle-inventory.py`.** Three targets are added, `ir-lib`,
`arm64-control-effects` and `cfg-identity`, and nine sentinels. Nothing is
removed; both mechanisms 13.3 describes, the `cargo metadata` gate and the
`-- --list` compile, now cover `flutterdec-ir` too. Extras remain allowed:
compilation, never source text, remains the oracle, and adding a case to any of
the thirty-three rows keeps the inventory green.

**`scripts/ci-check.sh` and `.github/workflows/ci.yml`.** Exactly one existing
lane changes in each, gaining `--test arm64_control_effects --test cfg_identity`
on the command that already named the two audit targets, and in the shell script
the matching `echo` and two usage lines that count the targets. Additivity by the
13.2 command:

```
git show af32bb1:scripts/ci-check.sh > /tmp/cic-prev.sh
strip() { grep -vE '^[[:space:]]*(#|$)' "$1" | sed 's/[[:space:]]*$//' | sort; }
comm -23 <(strip /tmp/cic-prev.sh) <(strip scripts/ci-check.sh)
```

Six lines are reported. Two are the `echo` and the `cargo test` invocation of the
integration lane, and both reappear with the two extra targets appended and no
other change. The remaining four are prose inside the `usage()` heredoc: the
`7)` list entry, whose wording changes from `oracle-loader guard targets` to
`oracle integration targets`, and three lines of the paragraph beneath it, which
changes `the two decompiler integration test targets` to `the four` and `either
file deleted` to `any of the files deleted`. No lane is removed, reordered, or
made conditional. The two executable lines that changed are the only ones, and
each new form is a strict superset of the old: every target `af32bb1` named is
still named, plus two more.

### 15.3 The nine new row-to-sentinel mappings

Thirty-three rows now, twenty-four from 13.3 and these nine. Every one of the nine
owns its sentinel: the test is defined in the protected file itself, so no row
here depends on a descendant.

| Protected row | Target | Sentinel |
| --- | --- | --- |
| `crates/flutterdec-ir/src/tests/control_effects.rs` | `ir-lib` | `control_effect_tests::every_arm64_control_effect_has_exactly_the_documented_edges` |
| `crates/flutterdec-ir/src/validate/tests.rs` | `ir-lib` | `validate::tests::every_planted_identity_failure_is_named` |
| `crates/flutterdec-core/src/pipeline/quality/control_effect_tests.rs` | `core-lib` | `quality_control_effect_tests::serialized_ir_states_every_control_effect_and_its_edges` |
| `crates/flutterdec-core/src/pipeline/runners/split/identity_tests.rs` | `core-lib` | `runners_split::split_identity_tests::every_piece_of_every_split_shape_is_canonical` |
| `crates/flutterdec-core/src/pipeline/runners/stubs/identity_tests.rs` | `core-lib` | `runners_stubs::stubs_identity_tests::every_shape_the_prune_mutates_comes_out_canonical` |
| `crates/flutterdec-decompiler/src/control_flow/regions/identity_boundary_tests.rs` | `decompiler-lib` | `control_flow::identity_boundary_tests::every_planted_identity_failure_declines_before_any_relation_is_built` |
| `crates/flutterdec-decompiler/src/control_flow/relation_oracle.rs` | `decompiler-lib` | `control_flow::relation_oracle::normalized_relations_are_identical_in_twenty_processes` |
| `crates/flutterdec-decompiler/tests/arm64_control_effects.rs` | `arm64-control-effects` | `both_emitters_render_the_same_control_effects` |
| `crates/flutterdec-decompiler/tests/cfg_identity.rs` | `cfg-identity` | `every_planted_identity_failure_emits_one_diagnostic_and_no_body` |

The three new targets select `-p flutterdec-ir --lib`,
`-p flutterdec-decompiler --test arm64_control_effects`, and
`-p flutterdec-decompiler --test cfg_identity`. Every sentinel is distinct and
defined exactly once across `crates/**/*.rs`, so a failure names the row that lost
its hook rather than a family.


### 15.4 Planted silencings

One disposable worktree detached at this commit under `~/.cache`, with its own
`TMPDIR` and `CARGO_TARGET_DIR` because this machine's `/tmp` is at its inode cap,
`git checkout -- . && git clean -fd` between plants and a `touch` sweep over every
`.rs` file afterwards, since restoring a file with an older mtime lets cargo reuse
the previous row's artifacts and the control then reports phantom results. Removed
with `git worktree remove --force`. Every row was produced in one sequential run
against the final code.

Three columns, because three different things have to be true. Workspace is
`cargo test --workspace`, counting result lines and the passed column, and it is
the column that shows the fake pass. Named-target lane is the `scripts/ci-check.sh`
step-7 command, which is the only thing that catches a manifest switching a whole
family off. Inventory is `python3 scripts/check-oracle-inventory.py`, the
correctness oracle.

| # | Plant | `cargo test --workspace` | Named-target lane | Compiled inventory |
| --- | --- | --- | --- | --- |
| ctl | none, control | exit 0, 17 result lines, 496 tests | exit 0 | exit 0, 33 compiled |
| ir1 | hook deleted from crates/flutterdec-ir/src/lib.rs | exit 0, 17 result lines, 492 tests | exit 0 | exit 1, 1 problem, names `control_effects.rs` |
| ir2 | hook line-commented in crates/flutterdec-ir/src/lib.rs | exit 0, 17 result lines, 492 tests | exit 0 | exit 1, 1 problem, names `control_effects.rs` |
| ir3 | hook in a nested block comment in crates/flutterdec-ir/src/lib.rs | exit 0, 17 result lines, 492 tests | exit 0 | exit 1, 1 problem, names `control_effects.rs` |
| ir4 | `#[cfg(any())]` above the crates/flutterdec-ir/src/lib.rs hook | exit 0, 17 result lines, 492 tests | exit 0 | exit 1, 1 problem, names `control_effects.rs` |
| ir5 | crates/flutterdec-ir/src/lib.rs hook swallowed by a macro | exit 0, 17 result lines, 492 tests | exit 0 | exit 1, 1 problem, names `control_effects.rs` |
| ir6 | hook deleted from crates/flutterdec-ir/src/validate.rs | exit 0, 17 result lines, 486 tests | exit 0 | exit 1, 1 problem, names `validate/tests.rs` |
| ir7 | undeclared feature above the crates/flutterdec-ir/src/validate.rs hook | exit 0, 17 result lines, 486 tests | exit 0 | exit 1, 1 problem, names `validate/tests.rs` |
| ir8 | `[lib] test = false` in the ir manifest | exit 101, 8 result lines, 428 tests | exit 101 | exit 1, 1 problem, names the `ir-lib` target and its manifest |
| co1 | hook deleted from crates/flutterdec-core/src/pipeline/quality.rs | exit 0, 17 result lines, 494 tests | exit 0 | exit 1, 1 problem, names `quality/control_effect_tests.rs` |
| co2 | hook line-commented in crates/flutterdec-core/src/pipeline/runners/split.rs | exit 0, 17 result lines, 493 tests | exit 0 | exit 1, 1 problem, names `split/identity_tests.rs` |
| co3 | hook in a nested block comment in crates/flutterdec-core/src/pipeline/runners/stubs.rs | exit 0, 17 result lines, 493 tests | exit 0 | exit 1, 1 problem, names `stubs/identity_tests.rs` |
| co4 | `#[cfg(any())]` above the crates/flutterdec-core/src/pipeline/quality.rs hook | exit 0, 17 result lines, 494 tests | exit 0 | exit 1, 1 problem, names `quality/control_effect_tests.rs` |
| co5 | crates/flutterdec-core/src/pipeline/runners/split.rs hook swallowed by a macro | exit 0, 17 result lines, 493 tests | exit 0 | exit 1, 1 problem, names `split/identity_tests.rs` |
| co6 | crates/flutterdec-core/src/pipeline/runners/stubs.rs hook swallowed by a macro | exit 0, 17 result lines, 493 tests | exit 0 | exit 1, 1 problem, names `stubs/identity_tests.rs` |
| co7 | `[lib] test = false` in the core manifest | exit 101, 7 result lines, 329 tests | exit 101 | exit 1, 1 problem, names the `core-lib` target and its manifest |
| rg1 | hook deleted from crates/flutterdec-decompiler/src/control_flow/regions.rs | exit 0, 17 result lines, 494 tests | exit 0 | exit 1, 1 problem, names `regions/identity_boundary_tests.rs` |
| rg2 | hook line-commented in crates/flutterdec-decompiler/src/control_flow/regions.rs | exit 0, 17 result lines, 494 tests | exit 0 | exit 1, 1 problem, names `regions/identity_boundary_tests.rs` |
| rg3 | hook in a nested block comment in crates/flutterdec-decompiler/src/control_flow/regions.rs | exit 0, 17 result lines, 494 tests | exit 0 | exit 1, 1 problem, names `regions/identity_boundary_tests.rs` |
| rg4 | `#[cfg(any())]` above the crates/flutterdec-decompiler/src/control_flow/regions.rs hook | exit 0, 17 result lines, 494 tests | exit 0 | exit 1, 1 problem, names `regions/identity_boundary_tests.rs` |
| rg5 | crates/flutterdec-decompiler/src/control_flow/regions.rs hook swallowed by a macro | exit 0, 17 result lines, 494 tests | exit 0 | exit 1, 1 problem, names `regions/identity_boundary_tests.rs` |
| ro1 | relation-oracle include deleted | exit 0, 17 result lines, 466 tests | exit 0 | exit 1, 1 problem, names `relation_oracle.rs` |
| ro2 | relation-oracle include line-commented | exit 0, 17 result lines, 466 tests | exit 0 | exit 1, 1 problem, names `relation_oracle.rs` |
| ro3 | relation-oracle include in a nested block comment | exit 0, 17 result lines, 466 tests | exit 0 | exit 1, 1 problem, names `relation_oracle.rs` |
| ro4 | `#[cfg(any())]` above the relation-oracle include | exit 0, 17 result lines, 466 tests | exit 0 | exit 1, 1 problem, names `relation_oracle.rs` |
| ro5 | relation-oracle include swallowed by a macro | exit 0, 17 result lines, 466 tests | exit 0 | exit 1, 1 problem, names `relation_oracle.rs` |
| at1 | `autotests = false` in the decompiler manifest | exit 0, 13 result lines, 484 tests | exit 101 | exit 1, 4 problems, names the decompiler integration targets |
| at2 | protected file tests/cfg_identity.rs deleted, target gone | exit 101, 7 result lines, 422 tests | exit 101 | exit 1, 1 problem, names the `cfg-identity` target |
| at3 | protected file tests/arm64_control_effects.rs deleted, target gone | exit 101, 7 result lines, 425 tests | exit 101 | exit 1, 1 problem, names the decompiler integration targets |
| sr1 | sentinel renamed in control_effects.rs | exit 0, 17 result lines, 496 tests | exit 0 | exit 1, 1 problem, names `control_effects.rs` |
| sr2 | sentinel renamed in relation_oracle.rs | exit 101, 4 result lines, 416 tests | exit 0 | exit 1, 1 problem, names `relation_oracle.rs` |
| sr3 | sentinel renamed in tests/cfg_identity.rs | exit 0, 17 result lines, 496 tests | exit 0 | exit 1, 1 problem, names `tests/cfg_identity.rs` |
| ur1 | unmapped ghost row added to the section 7 oracle table | exit 101, 8 result lines, 428 tests | exit 101 | exit 1, 1 problem, `has no sentinel here` |
| ext | control: a case added to a protected oracle | exit 0, 17 result lines, 497 tests | exit 0 | exit 0, 33 compiled |

Twenty-six of the thirty-four rows are silencings that leave
`cargo test --workspace` exiting 0. Twenty-two of those also leave the suite
visibly smaller and nothing else complains: `ir1` through `ir5` take 496 tests to
492, `ir6` and `ir7` to 486, `co1` and `co4` to 494, `co2`, `co3`, `co5` and `co6`
to 493, `rg1` through `rg5` to 494, `ro1` through `ro5` to 466, and `at1` to 484
with all four integration targets gone and four fewer result lines. Every one of
the twenty-six is rejected by the compiled inventory, which names the exact
protected row that stopped being compiled.

Twenty of the twenty-six leave the hook's own bytes byte-identical: the four
`//`-prefixed and nested-block-comment rows, the three `#[cfg]` rows, the five
macro-swallowed rows, `at1`, and the three sentinel renames, which do not touch a
hook at all. Source text cannot see any of them, which is why 13.2 demoted text
matching to a diagnostic and why this record extends the compiled inventory rather
than the guard's text checks.

`sr1` and `sr3` are the sharpest rows in the table. Renaming a sentinel changes no
count at all: the workspace suite stays at exactly 496 tests and exits 0, because
the test still exists and still runs under its new name. Only the inventory
notices, and what it reports is exactly right - the mapping in
`scripts/check-oracle-inventory.py` no longer points at a test that exists, so
nothing proves that file is compiled. A rename is legitimate work; the fix is to
update the mapping in the same commit, which is what the failure asks for. `sr2` is
the same plant on `relation_oracle.rs` and is additionally loud, because the
twenty-process determinism check re-executes itself by exact test path and cannot
find its own new name.

Six rows fail loudly on their own and are recorded as such rather than claimed as
silent: `ir8` and `co7` (`test = false` breaks the build of the crate's own test
target), `at2` and `at3` (the guard asserts every protected file exists), `sr2`,
and `ur1` (the guard's unmapped-row check). Their inventory column still matters:
each names the specific target or row rather than reporting a generic build
failure, so the diagnosis does not depend on reading a compiler error.

`ext` is the no-false-positive control. Adding a case to a protected oracle raises
the suite to 497 and the inventory stays green at 33 compiled, because extras are
always allowed. Compilation, not source text, is the oracle, and growing a ruler is
expected work.

### 15.5 Why the six product files carry no digest

The rulers this record protects were inline modules in six files that ordinary
work edits. A digest over any of them would fire on the next unrelated change to
the surrounding product code, and a digest that fires on legitimate change gets
relaxed, deleted, or routinely re-recorded, which is the same as no digest. That
is the reason section 7 has never held a hook, and it is the reason it does not
hold `crates/flutterdec-ir/src/lib.rs`, `crates/flutterdec-ir/src/validate.rs`,
`crates/flutterdec-core/src/pipeline/quality.rs`, `runners/split.rs`,
`runners/stubs.rs`, `crates/flutterdec-decompiler/src/control_flow/regions.rs`,
`crates/flutterdec-decompiler/src/control_flow.rs`, or any `Cargo.toml`.

So the assertions moved instead. Each test-only file is one this mission does not
expect later work to edit except by adding a case, which is exactly what a digest
plus an extras-allowed inventory can protect. The product file keeps only the
declaration, which nothing but the compiler can vouch for, and that is what
`scripts/check-oracle-inventory.py` asks.

Two protections therefore compose, and neither is sufficient alone. The digest
says the ruler's bytes are unchanged; it cannot say the compiler saw them. The
inventory says the compiler saw them; it cannot say they still assert what they
did. Section 15.4 measures the second half. The first half is measured by the
digest table itself: all fifty-six rows of section 7 match the worktree in this
commit, verified by `sha256sum` per row.

### 15.6 Section 9 steps

1. Invariant. Not an L1 or L2 invariant: no expected value, no product logic and
   no emitted output changes. This is an L4 and L5 change, and the invariant is
   the section 5 rule that a protected oracle may not be silenced, applied to the
   nine IR and CFG rulers that had no protection at all. 15.2 proves the
   assertions are the same text by token stream, and 15.4 measures the protection.
2. Test. `scripts/check-oracle-inventory.py` is the test, extended to
   thirty-three rows and three new targets, and `scripts/ci-check.sh` step 8 plus
   the `Compiled oracle inventory` step in `.github/workflows/ci.yml` are what
   make it unskippable. It fails on the old behavior in the sense that matters:
   at `af32bb1` every one of the twenty-six silencings in 15.4 passed every gate
   in this protocol with every digest in section 7 matching, because none of the
   nine files had a digest or a sentinel.
3. Diff and digests. Recorded in 15.1 and 15.2, with the reproducing commands. All
   fifty-six section 7 rows were re-verified against the worktree with `sha256sum`
   per row, 0 mismatches.
4. Original reference preserved. Recorded in 15.1, continuing the chains in 10.1,
   11.1, 12.1 and 13.1. The nine new rows record their pre-move location at
   `af32bb1`, so the text is recoverable from history. `1371e42` is untouched.
5. Own commit. Satisfied. This record, the six moves and their hooks, the guard
   extension, the checker extension and the two CI lane changes land as one
   commit, `test(oracle): protect IR and CFG boundary tests`, with no product
   logic change in it.
6. L5 re-run. `TMPDIR=... CARGO_TARGET_DIR=...
   NIX_CONFIG='experimental-features = nix-command flakes' scripts/ci-check.sh`
   exits 0 at the current digests, 13 lanes, `[ci-check] all checks passed`, 22
   result lines and 543 tests, with the inventory lane reporting
   `[oracle-inventory] ok, 33 protected oracles are compiled` and the step-7 lane
   now naming all four integration targets.

## 16. Adjudication record: the omitted-path collapse

Commit `92c14a8` changed what a `_block_N()` call means in a finished artifact,
which moves two protected test files. This record is the section 9 adjudication
for that change.

Before it, `collapse_remaining_helpers` rewrote **every** surviving
`return _block_N();` into `return null;` and deleted **every** helper
definition, whether or not any budget had been reached. The artifact therefore
carried, for each deferred edge, a return the graph does not contain, and the
block's body was not in the artifact at all. The `// omitted complex paths:`
summary named the ids, so the loss was announced, but it was announced as a
return.

After it, a call whose helper was defined keeps both call and definition. Only a
call the helper budget refused to define is rewritten, into
`// omitted path to block N: helper budget exhausted, block not emitted`, and
that id still appears in the summary. Definitions nothing calls are dropped to a
fixpoint, so the call set and the definition set of a finished artifact are
equal.

### 16.1 The invariant that makes the new output correct

Invariant I12 of section 5 already required exactly this: every `_block_N()`
either resolves to an emitted definition or is replaced by an explicit omission
form whose id appears in the summary. I13 forbids an undefined reference and a
path dropped without a marker. Neither is what the old collapse did for a
*defined* helper: it deleted a definition that existed and put an exit in its
place, which is the L2 rule against fabricated control applied to a return
rather than to a `goto` or a `tailCall_`.

The trailing clause of I12, "equivalently, `quality.json` `block_helper_refs` is
0 for a run with no surviving helper", is conditional and still holds: it is a
statement about runs where no helper survives. The same clause in the EM-05 row
of section 2 is written without that condition, because at `1371e42` no helper
ever survived, so the two readings could not be told apart. **EM-05's
`block_helper_refs is 0` clause is superseded by this record.** The row's Status
column stays as it is: it is pinned to `1371e42` and describes what that
revision did.

### 16.2 The tests that fail before and pass after

- `crates/flutterdec-decompiler/src/control_flow/emission_taxonomy_tests.rs`:
  `helper_calls_below_the_budget_all_resolve`,
  `helper_calls_at_the_budget_all_resolve` and
  `helper_calls_above_the_budget_become_explicit_omissions` are the below, at and
  above-budget cases EM-05 asks for. All three fail on the old behavior, which
  leaves an artifact with zero calls and zero definitions and a `return null;`
  at every deferred edge.
- `every_reachable_block_is_emitted_or_named_by_an_omission` is the
  reconciliation: a block is emitted, named by an omission event, or reached only
  through a block that is.
- The two protected tests in `omitted_path_and_stack.rs` keep their names,
  their fixtures and their subject. Their assertions are inverted where the
  behavior inverted, and one of them now also asserts call-definition set
  equality.

### 16.3 Digest chains

Column order matches sections 10.1 through 15.1, state before digest.

`crates/flutterdec-decompiler/src/tests/cfg_and_stack/omitted_path_and_stack.rs`:

| Commit | State | sha256 |
| --- | --- | --- |
| `1371e42` | reference | `e5e53a705aa16f6b27df6d99375da0d76106fc6f16f462301ab858d5e77a21ad` |
| `6d501c8` | unchanged through every prior mission commit | `e5e53a705aa16f6b27df6d99375da0d76106fc6f16f462301ab858d5e77a21ad` |
| `92c14a8` | current, recorded in section 7 | `9b8d3117e3e1c510fbbf6a1a8217ac795968aba1661d938a02ffd0abedeaf79c` |

`crates/flutterdec-core/src/pipeline/runners/tests.rs`:

| Commit | State | sha256 |
| --- | --- | --- |
| `1371e42` | reference | `a65298cde1ed807a838199162397bd51ff7f35e38941a0bd274872116b8c4668` |
| `6d501c8` | unchanged through every prior mission commit | `a65298cde1ed807a838199162397bd51ff7f35e38941a0bd274872116b8c4668` |
| `92c14a8` | current, recorded in section 7 | `7d1d87fa9401d07ab19b4bbb190edf1c53538a6da83ccd0e891b851b63200e63` |

### 16.4 Exact diff intent, per file

`omitted_path_and_stack.rs`, +41 lines, -14, two tests, both keeping their
names, which are the section 13 sentinels for this file:

1. `collapses_helper_calls_into_omitted_path_comments`. The fixture gains a
   second call, to a block the budget refused, and the assertions split in two:
   the defined helper keeps its call and its body, the refused one becomes the
   omission marker and is named by the summary, no `return null;` appears
   anywhere, and the call set equals the definition set. The old file asserted
   the opposite of the first and third of those, which is the behavior this
   record supersedes.
2. `summarizes_duplicate_omitted_blocks_once`. The helper definition is removed
   from the fixture, so both call sites are refused ones; the summary is still
   asserted to name the block exactly once, the two call sites each carry their
   own marker, and `return null;` is asserted absent rather than present twice.

`runners/tests.rs`, +6 lines, -0: one `emission: Default::default(),` line in
each of the five `PseudocodeArtifact` literals and one in the `QualityReport`
literal, because both structs gained a field. No assertion changes.

### 16.5 What did not change

No assertion was deleted, no test was renamed or removed, and no fixture was
weakened: the decompiler unit suite goes from 298 to 309 tests and the workspace
from 496 to 508, with 0 removed. The three golden snapshots are byte-identical
and were not rewritten. `docs/baseline/aa-1/warmup-reference.json` and the other
recorded references are untouched, so the pre-change artifact digests, line
counts and helper counts for all 33 benchmark cases remain available for
comparison.

Seven of those 33 cases move, all in the direction this record describes, with
every correctness flag still passing and `correctness_failures` empty:

| Case | Lines | Helper definitions | Helper references |
| --- | --- | --- | --- |
| `fan-in/256/base` | 88 to 1846 | 0 to 20 | 0 to 20 |
| `fan-in/1024/base` | 88 to 5657 | 0 to 64 | 0 to 64 |
| `multi-exit/256/base` | 88 to 1910 | 0 to 21 | 0 to 21 |
| `multi-exit/1024/base` | 88 to 5657 | 0 to 64 | 0 to 64 |
| `irreducible/64/base` | 663 to 29776 | 0 to 45 | 0 to 45 |
| `irreducible/256/base` | 663 to 42889 | 0 to 61 | 0 to 61 |
| `irreducible/1024/base` | 663 to 55875 | 0 to 63 | 0 to 63 |

Those figures are also the size of the underlying problem: the DFS fallback
duplicates a block once per reaching path, and the collapse was hiding that
duplication rather than bounding it. Bounding it belongs to the fallback, not to
the collapse.

### 16.6 Section 9 steps

1. Invariant: section 16.1, sourced from I12 and I13 and from the L2 rule
   against fabricated control.
2. Tests: section 16.2, at the emission layer the change belongs to.
3. Diff and digests: sections 16.3 and 16.4.
4. Reference preserved: `1371e42` is untouched, both prior digests are recorded
   above, and every recorded benchmark reference keeps its pre-change values.
5. **Not followed as written.** The two protected test files changed in
   `92c14a8`, the same commit as the product change, rather than in a commit of
   their own. Both assert behavior that does not exist before that commit, so a
   separate ruler commit would have been a knowingly failing revision under the
   then-current no-rewrite policy. The later one-time sole-author rewrite
   preserved the commit tree. The diff is recorded here instead, and the two
   files are separable: `git show 92c14a8 -- <the two paths>` is the whole ruler
   change
   and reverting it reverts nothing else.
6. L5 re-run in full after the change: `scripts/ci-check.sh` exit 0, 21 result
   lines, 520 tests, including the three goldens and the oracle inventory lane.

## 17. Adjudication record: executable section 7 digests

Section 7 has recorded a sha256 for every protected path since `1371e42`, and
until this record nothing recomputed one. Every earlier adjudication treats those
digests as the ruler that decides whether a protected file changed, and section
13's checker says so in as many words - "a digest proves only that a file's bytes
are unchanged" - but no CI lane, no test, and no script ever hashed a protected
file. The table was a claim about the worktree that only a human comparing it by
hand could falsify.

That gap is the exact complement of the one section 13 closed. Section 13 proved
that a protected file is still *compiled*, because a digest cannot see the
loader. This record proves that the file the compiler saw is still the file the
table protects, because the compiled inventory cannot see the bytes: gut a
protected oracle down to a one-line stub that keeps nothing but its sentinel's
name, and the inventory reports it compiled and exits 0. Both halves are needed,
and neither substitutes for the other.

Commit `c58013d` moves `scripts/check-oracle-inventory.py`, which is a protected
row itself. This record is the section 9 adjudication for that move.

### 17.1 What the checker now does before any Cargo work

`scripts/check-oracle-inventory.py` gained two functions and one hardcoded
inventory, and its `main` runs them ahead of `cargo metadata`:

- `parse_digest_rows` reads every `| path | sha256 |` row of section 7 and of no
  other section. It is bounded by the `## 7. Protected paths and digests`
  heading and the next `## ` heading, so the before-and-after digest chains in
  sections 10.1 through 16.3 at this commit, and every chain table added after
  it, and section 8's recorded evidence, are the same table shape and are
  correctly invisible to it. A row moved out of section 7 into one of those
  records therefore reads as a deleted row.
- `PROTECTED_PATHS` is the hardcoded expected inventory, all 56 paths at this
  commit, in the order the five tables list them. It is the ruler for the table
  rather than a copy of it: parsing the protocol alone cannot notice a deleted
  row, because a deleted row leaves nothing behind to check.
- `check_digests` requires, in this order, that no path is listed twice, that the
  parsed row set and `PROTECTED_PATHS` are equal in both directions, that every
  digest is exactly 64 lowercase hex characters, that every path is an existing
  regular file, and that every file's sha256 equals its row. Its failures are
  fatal and the run returns 1 before a single Cargo invocation.

The compiled-inventory pass is unchanged. Nothing was removed from it, no row
lost its sentinel, and extra tests are still expected work.

`scripts/check-oracle-inventory.py` is row 6 of the Checkers table, so this pass
verifies its own bytes. A change to the checker that is not recorded in section 7
fails the checker.

### 17.2 The exact expected inventory

56 rows at `c58013d`, which was every digest row of section 7 and no other row of
this document at that commit. The table has grown since, one adjudicated record
at a time, to the 71 rows section 7 lists today:

| Table | Rows |
| --- | --- |
| Fixed reference emission artifacts | 3 |
| Checkers, scanners, and their plant tests | 12 |
| Gate and harness scripts | 6 |
| Fixtures and sample data | 2 |
| Oracle test files | 33 |
| Total | 56 |

Those 33 Oracle test files rows were exactly the 33 keys of `SENTINELS` at this
commit, so every one of them became proved twice: its bytes here, and its
compilation by the pass section 13 records. The other 23 rows - the three
goldens, the twelve checkers and scanners, the six gate and harness scripts, and
the two fixtures - had no executable protection of any kind before this record,
because `SENTINELS` does not map them and nothing else read them.

The clean run reports both counts:

```
[oracle-inventory] 56 digest rows in docs/oracle-protocol-ir-cfg-emitter.md section 7
[oracle-inventory] ok, 56 protected paths match their section 7 digests
[oracle-inventory] 33 protected oracle rows in docs/oracle-protocol-ir-cfg-emitter.md
[oracle-inventory] ok, 33 protected oracles are compiled
```

### 17.3 Digest chain

Column order matches sections 10.1 through 16.3, state before digest.

`scripts/check-oracle-inventory.py`:

| Commit | State | sha256 |
| --- | --- | --- |
| `c95daa6` | new, added by section 13 | `d882132e87cb4625ebdac88ab310e405b00133bd546e172db282be7e1bbf47bf` |
| `6d501c8` | three targets and nine rows added, section 15 | `b8e06c148c0268f23acbb9547e5b9248b3f4ebc6903a48e8d21112be41e3ef49` |
| `c58013d` through `5fa97f2` | second move, adjudicated here, superseded at `7b8628a` | `98e7f29f8ebebaf68dc28c82ec465eb359cf3b91280f808ce1dfb3d17221bbf0` |

All three values are re-derived here with
`git show <commit>:scripts/check-oracle-inventory.py | sha256sum`, against the
commit each row names; the second is the value section 7 carried at `b396a62`,
and the third is the value it carried from `c58013d` through `5fa97f2`. All three
are historical: this file moved four more times after `7b8628a`, and section 7
records `3900f505ea8aea59500c99fcf598013cffac55e9128ec9498f7811738bcbf71a`
today, adjudicated in section 23.

No other protected file changed. `git diff --name-only` for `c58013d` is two
paths, `scripts/check-oracle-inventory.py` and this document, and the second is
not protected.

### 17.4 Code before ruler, and why the order is forced here

Section 9 asks for the ruler change to be separable from the behavior change.
Here they are the same file. The checker *is* the ruler, and its own digest row
is one of the rows it verifies, so:

1. At `b396a62`, the parent, the table said
   `b8e06c148c0268f23acbb9547e5b9248b3f4ebc6903a48e8d21112be41e3ef49` and the
   checker did not hash anything. Both were consistent and the run exited 0.
2. The intermediate state - new checker code, old digest row - is a knowingly
   failing revision. It was reached and observed during the work: the checker
   rejects itself by name, `scripts/check-oracle-inventory.py does not match its
   ... section 7 digest`, and returns 1 before any Cargo work. That is the
   correct behavior, and it is why the code and its digest row cannot land in two
   commits under the then-current no-rewrite policy. The later one-time
   sole-author rewrite preserved this commit tree.
3. `c58013d` therefore carries both. The code was written first and the row was
   computed from its final bytes with `sha256sum`, which is the only order that
   terminates: any edit to the checker after the row is written invalidates the
   row.

The ruler is still separable for review. `git show c58013d --
scripts/check-oracle-inventory.py` is the whole executable change, and reverting
that path together with the one-line row in section 7 restores `b396a62`'s
behavior exactly.

### 17.5 The CI change is additive, and there is none

Neither `scripts/ci-check.sh` nor `.github/workflows/ci.yml` was edited. Both
already run the checker as a lane of their own, as the byte-identical command
`nix develop -c python3 scripts/check-oracle-inventory.py`, which
`the_protected_oracle_loader_chain_is_intact` asserts verbatim in
`crates/flutterdec-decompiler/tests/provenance_audit.rs`. The digest pass is
inside that invocation, so both lanes exercise it with no new lane, no new
command, and no move of `scripts/ci-check.sh`'s own digest.

Additive in the strict sense: every check those lanes made at `b396a62` is still
made, in the same order, and the digest pass runs ahead of them. Proved by
running both surfaces verbatim.

| Surface | Clean | With one byte appended to a protected golden |
| --- | --- | --- |
| The `Compiled oracle inventory` step of `.github/workflows/ci.yml`, extracted verbatim | exit 0 | exit 1, naming `structured_loop_emit.dartpseudo` |
| `scripts/ci-check.sh --skip-tests` | exit 0 | exit 1 at the `[ci-check] scripts/check-oracle-inventory.py` lane, same message |

A full `scripts/ci-check.sh` on the clean tree at `c58013d` exits 0 with 14
lanes, 22 result lines and 556 tests, and reports both inventory counts above.
`cargo test --workspace` exits 0 with 17 result lines and 509 tests.

### 17.6 Planted silencings

Every plant is one edit against a clean `c58013d`, restored between rows. All
seventeen are rejected with exit 1, and each names the row or path it broke.
Plants 1 through 15 never reach Cargo: the digest pass returns first.

| # | Plant | Rejected by |
| --- | --- | --- |
| 1 | A section 7 row deleted (`scripts/lint-shell.sh`) | `is a protected path with no digest row` |
| 2 | A row added for an unprotected path (`scripts/bench-identity-gate-test.sh`) | `is not in this checker's protected inventory` |
| 3 | An existing row duplicated verbatim | `is listed twice` |
| 4 | A path listed twice with a different second digest | `is listed twice` |
| 5 | A digest rewritten in uppercase hex | `is not 64 lowercase hex characters` |
| 6 | A digest truncated to 8 characters | `is not 64 lowercase hex characters` |
| 7 | A digest of 64 non-hex characters | `is not 64 lowercase hex characters` |
| 8 | A protected file deleted from the worktree | `is not an existing regular file` |
| 9 | A protected file replaced by a directory of the same name | `is not an existing regular file` |
| 10 | One line appended to a protected golden | `does not match its ... digest` |
| 11 | Whitespace-only change to a protected golden | `does not match its ... digest` |
| 12 | A protected oracle gutted to its sentinel, `structuring.rs` | `does not match its ... digest` |
| 13 | The checker's own bytes changed | `scripts/check-oracle-inventory.py does not match its ... digest` |
| 14 | A row moved out of section 7 into section 8, bytes intact | `is a protected path with no digest row` |
| 15 | All 36 `crates/` rows deleted from section 7 | 36 problems, one per path |
| 16 | `crates/flutterdec-ir/src/validate/tests.rs` gutted to its sentinel | `does not match its ... digest` |
| 17 | Plant 10 run through both real CI surfaces | Both lanes, section 17.5 |

Plant 16 is the one that measures what this record adds, because it is
sentinel-preserving and otherwise green. The file goes from its full ruler to

```rust
#[test]
fn every_planted_identity_failure_is_named() {}
```

and under that plant:

- The checker at `b396a62` exits **0**. It prints
  `compiled crates/flutterdec-ir/src/validate/tests.rs -> ir-lib ::
  validate::tests::every_planted_identity_failure_is_named` and
  `ok, 33 protected oracles are compiled`. The only visible trace is
  `ir-lib listed 11 tests` where the clean tree lists 20, and a smaller listing
  is not a failure: adding and removing cases both change it, and extras are
  expected work.
- `cargo test --workspace` exits **0**, with the same 17 result lines, at 500
  passed instead of 509. Nine assertions of the IR well-formedness ruler are
  gone and every gate is green.
- The checker at `c58013d` exits **1** and names the file and both digests.

Plant 12 is the same silencing applied to a file other code depends on, so it
also fails to compile; it is recorded for completeness, not as the measure. Plant
16 is the measure.

### 17.7 What this does not close

- The digest pass proves bytes, not meaning. A protected file rewritten to assert
  something weaker fails here as a byte change and must go through section 9,
  which is the intended outcome, but the protocol still relies on adjudication to
  judge whether the new assertions are as strong.
- `PROTECTED_PATHS` and section 7 must be edited together. That is deliberate -
  it is what makes adding or dropping a protected path visible in review - but
  neither side can add a path the other lacks, so a genuinely new protected file
  is two edits, not one.
- `#[ignore]` on a sentinel is still invisible to the compiled inventory, as
  section 15 recorded. The digest pass does not change that: an `#[ignore]` added
  to a protected file is caught as a byte change, but one added to an unprotected
  test is not.
- `crates/flutterdec-decompiler/src/control_flow/emission_taxonomy_tests.rs` has
  no section 7 row and no sentinel, so neither pass reaches it.

### 17.8 Section 9 steps

1. Invariant: section 7's digests are the ruler that decides whether a protected
   file changed. A ruler nothing recomputes is not a ruler; section 17.1 makes it
   executable.
2. Tests: the checker's own `--self-test`, extended with `digest_self_test`,
   which covers the section-bounded parser and each failure class against a
   temporary tree, standard library only. The default invocation runs it first,
   so both CI lanes run it.
3. Diff and digests: sections 17.1 and 17.3.
4. Reference preserved: `1371e42` is untouched, and every digest in section 7
   except the checker's own is byte-identical to what it was at `b396a62`.
5. **Not followed as written**, for the reason in section 17.4: the ruler and the
   code are the same file, so they land in one commit. The revert path is
   recorded there.
6. L5 re-run in full after the change: `scripts/ci-check.sh` exit 0, 14 lanes, 22
   result lines, 556 tests, including the three goldens, the named integration
   targets, and both passes of the oracle inventory lane.

## 18. Adjudication record: predecessor-bound provenance reconciliation

This is the section 9 record for the protected
`scripts/prov_cross_audit_reconcile.py` ruler change. It strengthens the ruler
after the product-side candidate accounting repair and changes no product code,
emitted pseudocode, emitted IR, fixture, threshold, or golden.

### 18.1 Invariant and exact diff intent

An annotation candidate is a claim about one incoming path, not only a member of
a rendered value set. For each join and loop-entry element, the path must be a
real predecessor in the independently emitted IR, the cited full-register
snapshot must be the end state of that same predecessor, and the record's
register must have exactly the claimed value in that snapshot. Predecessor set
membership, predecessor coverage, and first-occurrence deduplication are
necessary but cannot replace this binding.

The old ruler checked only the first half. Its `site_not_real` count established
that every `path_key` named some real predecessor, and `rendered_disagrees`
compared the deduplicated candidate values with the pseudocode. It never joined
one candidate's path, register, and value to the full end-state snapshot for
that path. The new eighth count, `predecessor_disagrees`, performs that join for
every join and loop-entry candidate after the IR has established the path as a
real predecessor. Missing snapshots, a snapshot for another predecessor, a
missing register binding, and a different value all fail the same stated
invariant.

The docstring now states all eight counts and retains the prior boundary: the
checker still does not prove that a later annotated line descends from the
anchor block. It no longer says that no check reads emitted bookkeeping, because
the new count deliberately reads the full-register snapshot while deriving the
predecessor identity from emitted IR.

### 18.2 Digest chain and preserved reference

Column order keeps these rows outside the section 7 parser's path-and-digest
shape.

| State | sha256 |
| --- | --- |
| `1371e42`, preserved fixed reference | `0633bf7191d62859efcbd35b9b62e186a39005e58ec49efaf24d8e03c6319c41` |
| `58c3269`, old seven-count ruler | `0633bf7191d62859efcbd35b9b62e186a39005e58ec49efaf24d8e03c6319c41` |
| This commit, eight-count ruler recorded in section 7 | `f7a21d2c497ff2c47e118cf3df208d869265eb11a561605f4f14d9c50febe870` |

The original bytes remain available at both named commits. Reproduce either old
row with `git show <commit>:scripts/prov_cross_audit_reconcile.py | sha256sum`
and the new row with `sha256sum scripts/prov_cross_audit_reconcile.py`.

### 18.3 Detection evidence

The checker's standard-library `--self-test` now carries three focused plants in
addition to one plant for each count:

| Plant | Legacy seven counts | New eighth count |
| --- | --- | --- |
| Move one join candidate from block 1 to real predecessor block 2, leaving rendered `7 | 9` unchanged | 0, accepted | nonzero, rejected |
| Permute four real predecessors from `7, 7, 9, 9` to `7, 9, 7, 9`, preserving coverage and rendered `7 | 9` | 0, accepted | nonzero, rejected |
| Retarget a loop-entry candidate from real entry predecessor block 1 to real entry predecessor block 2, leaving rendered `21 | 23` unchanged | 0, accepted | nonzero, rejected |

Each plant assertion computes the legacy result from the same reconciliation by
requiring all original seven counts to remain zero, then requires
`predecessor_disagrees` to be nonzero. The clean synthetic corpus and the fresh
22,376-function localsend corpus remain at zero for all eight counts. The latter
contains 5,151 annotations: 4,486 join, 394 loop-entry, and 271 pre-call records.

### 18.4 Atomicity and section 9 steps

1. Invariant: section 18.1, independent of the deduplicated annotation output.
2. Test: the three detection-proven plants in section 18.3, plus the existing
   count plants and honest synthetic corpus.
3. Diff and digests: sections 18.1 and 18.2.
4. Reference preserved: section 18.2 records both old commits and the old digest.
5. The checker, its embedded plant fixtures, this adjudication, and the section 7
   digest move land in one atomic protected-ruler commit. Splitting them would
   intentionally leave either stale protected bytes or a digest for behavior not
   yet present.
6. L5 was re-run in full: every provenance checker, old-versus-new plant
   detection, the oracle inventory and digest pass, Python lint, and
   `scripts/ci-check.sh` exited 0.

## 19. Adjudication record: emitter repair oracle protection

This is the section 9 record for closing VAL-ORACLE-004 after the emitter
repairs and their ruler refreshes. It adds no mutable product file, temporary
probe, generated artifact, or performance fixture to section 7. It protects
only stable test-only rulers, strengthens the existing relation oracle, and
makes every protected file independently visible to the compiler inventory.

### 19.1 Exact protected inventory and loader map

The eleven new section 7 rows and their independently listed sentinels are:

| Protected test-only path | Target | Sentinel |
| --- | --- | --- |
| `crates/flutterdec-decompiler/src/control_flow/emission_taxonomy_tests.rs` | `decompiler-lib` | `control_flow::emission_taxonomy_tests::snapshot_and_restore_cover_every_mutable_state_family` |
| `crates/flutterdec-decompiler/src/control_flow/annotation_anchor_tests.rs` | `decompiler-lib` | `control_flow::annotation_anchor_tests::every_candidate_ends_with_a_recorded_outcome` |
| `crates/flutterdec-decompiler/src/line_identity_tests.rs` | `decompiler-lib` | `line_identity_tests::every_length_changing_helper_rejects_a_partial_identity_mismatch` |
| `crates/flutterdec-decompiler/tests/helper_syntax_boundaries.rs` | `helper-syntax-boundaries` | `recovered_text_inside_a_helper_body_never_moves_helper_structure` |
| `crates/flutterdec-decompiler/tests/rewrite_boundaries.rs` | `rewrite-boundaries` | `recovered_data_is_safe_and_disjoint_from_emitter_names` |
| `crates/flutterdec-decompiler/tests/unmodelled_write_effects.rs` | `unmodelled-write-effects` | `an_unmodelled_write_drops_the_binding_at_every_destination_width` |
| `crates/flutterdec-decompiler/tests/register_width_provenance.rs` | `register-width-provenance` | `an_x_produced_non_literal_is_unresolved_through_a_w_read` |
| `crates/flutterdec-decompiler/tests/atomic_rmw_effects.rs` | `atomic-rmw-effects` | `every_atomic_load_form_invalidates_its_second_operand` |
| `crates/flutterdec-decompiler/tests/annotation_anchor_identity.rs` | `annotation-anchor-identity` | `annotations_bind_their_own_line_and_the_reconciler_rejects_every_planted_defect` |
| `crates/flutterdec-decompiler/tests/provenance_accounting.rs` | `provenance-accounting` | `release_audit_accounts_for_accepted_and_rejection_only_streams` |
| `crates/flutterdec-core/tests/pipeline_determinism.rs` | `pipeline-determinism` | `the_whole_artifact_set_is_byte_identical_in_twenty_processes` |

The first three use explicit `#[cfg(test)]` module declarations. The next seven
are separate decompiler integration targets. The last is a separate core
integration target. Both CI lanes name every protected integration target, and
the inventory lists every target separately before matching its sentinel.
At this record section 7 contained 67 protected digests, of which 44 were Rust
oracle rows. Each later record states the counts of its own state, and the
current exact counts are in section 23.

These rulers directly preserve VAL-EMIT-001 through VAL-EMIT-007: helper
resolution and syntax boundaries, the closed emission taxonomy and rollback
state, width and unknown-write effects, exact annotation identity and complete
provenance accounting, recovered-data rewrite boundaries, and byte-identical
twenty-process pipeline output. Product hooks, `helpers/expr.rs`, emitter source,
the synthetic model generated inside the determinism test, and benchmark inputs
remain unprotected because later product and performance work may legitimately
change them.

### 19.2 Relation-oracle repair and detection

`control_statements_outside_a_loop` formerly counted every `{` and `}` byte on a
rendered line. A recovered literal or comment containing an unmatched `}` could
therefore close the recorded loop before a real `break;` or `continue;`, falsely
reporting both as stranded. It now uses the same `code_brace_counts` scanner as
the repaired emitter passes, so only code spans change structural depth.

The protected test plants unmatched `{`, `}`, `${`, escaped and ordinary
quotes, `/* */`, and `//` around real loop-control statements. Its embedded
legacy implementation reports both statements stranded. The repaired ruler
reports neither. A second body keeps genuine `break;` and `continue;` outside a
loop and proves both are still rejected, so syntax awareness cannot become a
false-pass path.

### 19.3 Digest chains

Column order keeps this history outside the section 7 path-and-digest parser.

| Protected ruler | Prior sha256 at `5fa97f2` | New sha256 at `7b8628a` |
| --- | --- | --- |
| `scripts/check-oracle-inventory.py` | `98e7f29f8ebebaf68dc28c82ec465eb359cf3b91280f808ce1dfb3d17221bbf0` | `4af2d9445f2cf43413c8d70f12a673b8536750ded8a6d8bc19ff206331acfb26` |
| `scripts/ci-check.sh` | `ec5e015bc65317c8b477c582b52a9a6d91c618e6becfe31da019b4bb34995401` | `b75bcdfbae8562785e174c1d360319ff83d72fbd4bdcbe1659e6d71d375a79e9` |
| `crates/flutterdec-decompiler/tests/provenance_audit.rs` | `e93e04f71f67dc57379fcca164af80d58a403889095bcc4aced48de990b44c59` | `469e3e98ae2a6002e5f5a75f99972df230375848cc82e79e8744c3ab47127c84` |
| `crates/flutterdec-decompiler/src/control_flow/relation_oracle.rs` | `75fe720e04cfa6bbb859981f1b39ebba0e0ed932e8973d3bab730058cedcfa96` | `c0558822e6f33e8201b26617d38c448adb78795d9d4325f1dc49e7dd99904cfe` |

The checker changes only the expected paths, test targets, and sentinels. The CI
script and workflow add explicit named invocations. The provenance guard adds
the same module and Cargo-discovery hooks in both directions and checks the core
and decompiler lanes independently. The relation ruler changes only its brace
reader and the old-versus-new plant. Reproduce either column with
`git show <commit>:<path> | sha256sum`. All four rulers have moved again since
`7b8628a`, so none of these values is the current section 7 digest; for that,
use `sha256sum <path>` against the table in section 7.

### 19.4 Deletion and bypass plants

Each of the eleven new protected files was moved out of the tree one at a time.
The default inventory invocation exited 1 before Cargo work, naming that exact
path as not an existing regular file. Each file was restored before the next
plant. The three explicit module hooks were then disabled one at a time with an
always-false `cfg`; digest verification stayed green, target listing succeeded
with a smaller suite, and the compiled inventory exited 1 naming the missing
sentinel. Finally, `autotests = false` was planted independently in the
decompiler and core manifests; each inventory run exited 1 because protected
integration targets disappeared from Cargo metadata. All plants were restored
and the clean inventory returned the 67 matching digests and 44 compiled oracles
of that record's state.

### 19.5 Section 9 steps

1. Invariant: every stable repair ruler is digest-pinned and independently
   compiled, and relation depth is derived from code spans only.
2. Tests: section 19.2 plus one deletion per new file and one bypass per new
   loader family in section 19.4.
3. Diff and digests: sections 19.1 and 19.3. No product behavior changes.
4. Reference preserved: `1371e42` and `5fa97f2` remain addressable; all prior
   bytes are recoverable with `git show`.
5. The relation repair, inventory, sentinels, named CI lanes, section 7 rows,
   and this adjudication land in one atomic oracle-protection commit.
6. L5 was re-run in full: focused relation and ruler targets, loader guard,
   oracle inventory, provenance audit, protected digests, and
   `scripts/ci-check.sh` all exited 0.

## 20. Adjudication record: explicit branch-target radix

The shared IR target parser used token length to decide that any bare
hexadecimal-looking token longer than six characters was hexadecimal. That made
the observed all-digit operand `1000000` select `0x1000000` rather than decimal
1000000. The accepted grammar now parses `0x` and `0X` prefixes as hexadecimal,
bare spellings containing `a` through `f` as hexadecimal, and every all-digit
spelling as decimal. Operand shapes outside a direct target, register plus
target, or register plus bit plus target remain unknown.

### 20.1 Public CFG ruler and loader protection

`crates/flutterdec-ir/tests/branch_target_radix.rs` constructs public
`FunctionDisassembly` values and observes only public `build_function_ir` CFG
blocks and edges. Its five tests cover short and long decimal, the observed
`1000000`, leading-zero decimal, lower and upper prefixed hexadecimal, lower and
upper bare hexadecimal containing a letter, zero and `u64::MAX - 1`, conditional
taken plus fallthrough edges, call fallthrough without a callee edge, overflow,
malformed, and ambiguous operand forms.

The new file is a protected Cargo integration target. Both CI lanes invoke it by
name, the protected provenance loader guard maps it to the IR manifest, and the
compiled inventory requires its public-CFG sentinel. At this record the clean
inventory reported 68 matching digests and 45 compiled oracle rows.

### 20.2 Detection plant

Planting the historical `all hex digits and len > 6` branch immediately before
decimal parsing makes the public CFG target exit 101: three of five tests fail.
The observed `1000000` case loses its target edge, the conditional case loses
its taken edge but keeps fallthrough, and the decimal upper-bound case loses its
target edge. Removing the plant restores all five tests. This demonstrates that
the ruler detects the old heuristic through public CFG behavior rather than a
private parser assertion.

### 20.3 Digest chains

Column order keeps this history outside the section 7 path-and-digest parser.

| Protected ruler | Prior sha256 | New sha256 at `7acb5ae` |
| --- | --- | --- |
| `scripts/check-oracle-inventory.py` | `4af2d9445f2cf43413c8d70f12a673b8536750ded8a6d8bc19ff206331acfb26` | `78ed3b4f1d3e1c30102474f57205725aaf60648a4f20c39f65a37a43016c8cd6` |
| `scripts/ci-check.sh` | `b1600c29ccbda98b751e8a337c6aa875dfc56eef3dc66efb9edb00952c78188c` | `354a21e6ecdfef30e9bc8ea91dbdfd7a33ca8062c4d537c81648328c7e5aeb43` |
| `crates/flutterdec-decompiler/tests/provenance_audit.rs` | `469e3e98ae2a6002e5f5a75f99972df230375848cc82e79e8744c3ab47127c84` | `e9f0e28379c364c9bca72e85d8ca47e73f2d4f9cbe10d292aa0eaa7fc9788f61` |
| `crates/flutterdec-ir/tests/branch_target_radix.rs` | new | `989b28c3c64a271eec2afc26eb4373e3325ac1f9b5d5d477a96a472d14c37af0` |

Only the expected protected path, target, sentinel, named CI invocation, and
loader mapping changed in the three existing rulers. No threshold, golden
fixture, benchmark definition, or unrelated parser changed.

### 20.4 Verification

The public CFG target passed 5 of 5 in both debug and optimized release. The
full IR package passed 25 tests including the integration target. The protected
loader guard passed, the oracle inventory reported 68 matching digests and 45
compiled rows, and the auxiliary resource inventory reported 9 matching
digests and intact loaders. A clean full `scripts/ci-check.sh` run exited 0,
including workspace clippy and tests, the release CLI build, and all 38 excluded
benchmark-harness tests.

## 21. Adjudication record: DFS loop relations ignore addresses

The DFS fallback now reuses the reachable CFG dominators from region analysis.
An edge is a back edge exactly when its target dominates its source; the entry
that permits a wrapper is the current active predecessor on a non-back edge.
Block addresses remain only in stable presentation and accounting identities.

### 21.1 The invariant that makes the new output correct

A loop is a property of the graph, not of where its blocks were laid out. The old
`has_backedge_pred` called an edge a back edge when the predecessor's `start_va`
was greater than or equal to the header's, and `has_forward_pred` called any
predecessor at a lower address a forward entry. Both are address heuristics. They
agree with dominance only when a reducible graph happens to be laid out in
increasing address order, so the same graph under a permuted layout produced a
different artifact, and an irreducible cycle whose latch dominates nothing was
still wrapped in `while (true)` as though it had a real header.

The accepted rule is the standard graph one, taken from L1 rather than from the
emitter that produced the output: an edge is a back edge exactly when its target
dominates its source in the reachable CFG, and the entry that permits a wrapper
is the traversal's own active predecessor on an edge that is not a back edge.
`reachable_edges` and `dominators` in `control_flow/regions.rs` become
`pub(super)` so the DFS fallback answers from the same graph source region
recovery uses, and `FuncEmitter` caches their result in a `OnceLock`. That cache
is derived from the immutable `FunctionIr`, so emitter rollback cannot change it.

### 21.2 Public ruler and loader protection

The protected public fixture target
`crates/flutterdec-decompiler/tests/dfs_loop_address_invariance.rs` builds simple,
nested, multi-exit, and irreducible graphs under ascending, descending, and
permuted address layouts. The permutation includes a lower-address latch entering
a higher-address header. It compares exact pseudocode and every serialized
artifact and accounting field after mapping immutable addresses back to block
ids, and separately pins loop, follow, break, continue, decline, and accounting
validity.

The new file is a protected Cargo integration target. Both CI lanes invoke it by
name, which is what takes the decompiler lane from eleven named targets to
twelve; the protected loader guard records its Cargo autotest hook in
`loader_map`; and the compiled inventory requires the sentinel
`public_dfs_loop_artifacts_ignore_block_address_order` through a new
`dfs-loop-address-invariance` target entry. At this record the clean inventory
reported 69 matching digests and 46 compiled oracle rows.

### 21.3 The rewritten irreducible expectations

The existing hand-derived relation oracle still passes all reducible cases, and
`crates/flutterdec-decompiler/src/control_flow/relation_oracle.rs` is the only
file holding an expected value this record moves. Two of its irreducible artifact
records, `irreducible` and `declined_fallback`, changed in the same direction:
`continues` and `loop_statements` go from 1 to 0, the back-edge comment names both
non-dominating cycle edges rather than one, and the invented `while (true)` body
becomes an explicit `// control rejoins block N: already emitted above` note. No
reducible expectation, counter, golden snapshot, threshold, or benchmark
definition changed.

The change is not this record's own ruler restating the product. The graph
behavior itself is pinned independently by section 21.2's public fixture target,
which never reads a hand-written expected artifact: it compares one graph's
artifacts against the same graph's artifacts under a permuted layout. The prior
bytes stay recoverable at
`c0558822e6f33e8201b26617d38c448adb78795d9d4325f1dc49e7dd99904cfe` with `git show
23f0127^:crates/flutterdec-decompiler/src/control_flow/relation_oracle.rs`.

### 21.4 Detection plant and restoration

In a disposable worktree holding this repository's exact bytes, the plant is the
reverse of this record's own product hunk, which restores the old
`pb.start_va >= block.start_va` back-edge test and the complementary
`pb.start_va < block.start_va` forward-predecessor scan:

```
git worktree add <dir> HEAD
git -C <dir> show 23f0127 -- crates/flutterdec-decompiler/src/control_flow/graph.rs \
  | git -C <dir> apply -R
nix develop -c cargo test -p flutterdec-decompiler --test dfs_loop_address_invariance
nix develop -c cargo test --release -p flutterdec-decompiler --test dfs_loop_address_invariance
git -C <dir> checkout -- crates/flutterdec-decompiler/src/control_flow/graph.rs
```

| State | Profile | Exit | Result line |
| --- | --- | --- | --- |
| planted | debug | 101 | `FAILED. 0 passed; 2 failed` |
| planted | release | 101 | `FAILED. 0 passed; 2 failed` |
| restored | debug | 0 | `ok. 2 passed; 0 failed` |
| restored | release | 0 | `ok. 2 passed; 0 failed` |

Both failures name the graph property, not a layout detail.
`public_dfs_loop_artifacts_ignore_block_address_order` fails on `simple_loop loop
count`, `left: 0` against `right: 1`: under the plant the permuted layout renders
`// control rejoins block 1: already emitted above` where the ascending layout
renders a wrapper, so the simple, nested and multi-exit fixtures lose their loop
wrapper. `public_auto_emission_preserves_isomorphic_meaning_and_declines_irreducible_input`
fails on `irreducible_cycle auto output changed under an address permutation`:
one layout invents `while (true)` under `// loop back-edges: block 1` while the
other emits the explicit rejoin under `// loop back-edges: block 2`. Restoring the
product bytes returns both profiles to 2 of 2 and leaves the worktree clean.

### 21.5 Digest chains

Column order keeps this history outside the section 7 path-and-digest parser.

| Protected ruler | Prior sha256 | New sha256 at `23f0127` |
| --- | --- | --- |
| `scripts/ci-check.sh` | `354a21e6ecdfef30e9bc8ea91dbdfd7a33ca8062c4d537c81648328c7e5aeb43` | `6cb19f223bde0510e2c70eac4c7b759b6fe04d57e74dce9e17ffcb1ce89c6389` |
| `scripts/check-oracle-inventory.py` | `78ed3b4f1d3e1c30102474f57205725aaf60648a4f20c39f65a37a43016c8cd6` | `5101f7e48b890da0124154611a18517ce29432d3bcf11fe9928e12dfb94ddf52` |
| `crates/flutterdec-decompiler/tests/provenance_audit.rs` | `e9f0e28379c364c9bca72e85d8ca47e73f2d4f9cbe10d292aa0eaa7fc9788f61` | `1309c838ea03cdf3299b84dccbc45fe29d9ef4bb6a3d3389a3da713a2f57f1fb` |
| `crates/flutterdec-decompiler/src/control_flow/relation_oracle.rs` | `c0558822e6f33e8201b26617d38c448adb78795d9d4325f1dc49e7dd99904cfe` | `e53dd455ddbbdf2c0b00d184f1f2d788833cbfd6a0db070ad69b9372297da849` |
| `crates/flutterdec-decompiler/tests/dfs_loop_address_invariance.rs` | new | `1c2c0403303e619de9fe840f62f61c1af92dbec77fe554fd66d3505755b37db3` |

Those are the only five section 7 rows this commit touched: the first four moved,
and `crates/flutterdec-decompiler/tests/dfs_loop_address_invariance.rs` joined the
table as a new row, which is why its prior cell reads `new` rather than a digest.
The first three moved on afterwards and their later links are unbroken.
`scripts/ci-check.sh` continues
into section 22 and then into section 23, which leaves it at its current
`9dac174b...`; section 24 leaves it byte-unchanged.
`scripts/check-oracle-inventory.py` continues into section 22 and then into
section 23, which leaves it at its current `3900f505...`; section 24 leaves it
byte-unchanged too.
`crates/flutterdec-decompiler/tests/provenance_audit.rs` continues into section
22, section 23, and then section 24, which leaves it at its current
`1627b7b9...`. The last two rows are still the current section 7 values.

Outside those five, the commit changes only product source
(`control_flow/graph.rs`, `control_flow/regions.rs`, and the `dfs_dominators`
field in the decompiler `lib.rs`), the named invocation in
`.github/workflows/ci.yml`, and three documents: the research note, the
resource-ruler protocol, and this one. The
three moved existing rulers change only by the new protected path, target,
sentinel, loader-map row, and named CI invocation; `relation_oracle.rs`'s one
further change is section 21.3's two irreducible records. `scripts/ci-check.sh` is
also a row of the auxiliary resource inventory in
`docs/resource-ruler-protocol.md`, so the same one-line change is adjudicated
there too, with the same old and new digests. No threshold, golden fixture,
benchmark definition, frozen reference, or unrelated structure changed.

### 21.6 Section 9 steps

1. Invariant: section 21.1. A back edge is dominance, not address order, and the
   wrapper-permitting entry is the traversal's active predecessor. Sourced from
   L1's graph definitions, independent of the emitter that produced the artifact.
2. Tests: section 21.2's public fixture target, which fails on the old behavior
   and passes on the new one at the layer the change belongs to, with the failure
   and the restoration recorded in section 21.4.
3. Diff and digests: sections 21.3 and 21.5, plus the matching adjudication in
   `docs/resource-ruler-protocol.md`.
4. Reference preserved: `1371e42` and `23f0127^` remain addressable, and the two
   superseded irreducible records are recoverable from the prior
   `relation_oracle.rs` digest named in section 21.3.
5. The product fix, the new ruler, the checker mapping, the loader-guard row, both
   named CI invocations, the section 7 digest row, and this record land in one
   atomic commit, `23f0127`.
6. L5 was re-run in full: the new target on its own in both profiles, the
   twelve-target decompiler lane, the compiled inventory, the resource inventory,
   and a clean `scripts/ci-check.sh`.

### 21.7 Verification

The public target passed 2 of 2 in both debug and optimized release, and the
hand-derived relation oracle passed with it. The compiled inventory reported 69
matching digests and 46 compiled oracle rows, the auxiliary resource inventory
reported 9 matching digests with intact loaders, and a full `scripts/ci-check.sh`
run exited 0.

Sections 21.1 through 21.7 were written after `23f0127` and land as a docs-only
commit of their own, because the original record carried the reason and the plant
but no digest chain and no section 9 steps. Nothing in that repair changes product
source, a test, a threshold, an expected value, or a section 7 digest. Every
digest above was recomputed from the commit range with
`git show <commit>:<path> | sha256sum`, and every current section 7 row was
recomputed from workspace bytes, before this text was written.

## 22. Adjudication record: entry loops merge the implicit entry path

No successor list names the path a call takes into a function's entry block, so
the DFS fallback's predecessor map showed one incoming path for an entry block a
back edge also targets. The one-predecessor fast path then skipped the merge, and
every value the function was entered with kept describing the first iteration:
argument bindings, the pre-call values provenance annotations cite, the last
compare, and the selector hints. The fallback now records that implicit path as
the distinct incoming path it is, so an entry block with any explicit incoming
edge merges through the same `merge_state_at_join` every other join uses. An
entry block no edge targets still shows one path, so it and every ordinary block
keep the fast path.

### 22.1 Public ruler and loader protection

`crates/flutterdec-decompiler/tests/entry_loop_state_merge.rs` builds public
`FunctionIr` fixtures and reads only public artifacts from
`emit_pseudocode_direct_dfs` and `emit_pseudocode`. Its four tests cover an entry
self-loop, a lower-address latch, two latches with conflicting writes, two
latches with compatible writes, a conditional exit, a loop that rewrites the
register a pre-call provenance value was read through, an entry block no edge
targets, ordinary one-predecessor blocks, a depth-budget loop whose helper
definitions re-render the merged entry block, and an irreducible entry loop whose
production decline must equal direct DFS. Each case pins the complete artifact
source, the IR predecessor relations it claims, the call, placeholder,
unresolved, repeated and rollback counters, the block ledger and its
reconciliation against the traversal events, and the absence of any annotation
span claiming a first-iteration value.

The new file is a protected Cargo integration target. Both CI lanes invoke it by
name, the protected loader guard records its Cargo autotest hook, and the
compiled inventory requires its merge sentinel. At this record the clean
inventory reported 70 matching digests and 47 compiled oracle rows.

### 22.2 Detection plant

Removing the implicit-entry predecessor from the fallback predecessor map makes
the target exit 101 in both debug and optimized release: three of four tests
fail. The entry self-loop, lower-address latch, conditional exit and annotated
fixtures render the entry value `slot0` where the merged artifact renders `reg1`,
two helper copies of the deep loop render it too, and the declined irreducible
artifact renders it once. The one-predecessor fast-path test stays green under
the plant, which is what shows it is not coupled to the merge. Restoring the
predecessor restores 4 of 4 in both profiles.

### 22.3 Digest chains

Column order keeps this history outside the section 7 path-and-digest parser.

| Protected ruler | Prior sha256 | New sha256 at `00e6115` |
| --- | --- | --- |
| `scripts/ci-check.sh` | `6cb19f223bde0510e2c70eac4c7b759b6fe04d57e74dce9e17ffcb1ce89c6389` | `94cc9b90e935bb1ecdce4280a31315337d5345b5c74b165e395c74de6dd608f5` |
| `scripts/check-oracle-inventory.py` | `5101f7e48b890da0124154611a18517ce29432d3bcf11fe9928e12dfb94ddf52` | `21bea1f4fa438b0aae70b5477a1ec026f13007c0bf569d6f37435089999b47df` |
| `crates/flutterdec-decompiler/tests/provenance_audit.rs` | `1309c838ea03cdf3299b84dccbc45fe29d9ef4bb6a3d3389a3da713a2f57f1fb` | `ea5e8087b6940bb41dfffeb6e5aef0c86b669553e745d761b19f24d9f2ff88f8` |
| `crates/flutterdec-decompiler/tests/entry_loop_state_merge.rs` | new | `d562cc31ddd244e11d83a0a1bb6e3e4b3d76bf6a052417170496da4198d80dd9` |

The only change in the three existing rulers is the new protected path, target,
sentinel, loader-map row and named CI invocation. No threshold, golden fixture, benchmark
definition, frozen expected value, or unrelated emitter behavior changed, and no
existing digest row other than those three moved.

### 22.4 Verification

The public target passed 4 of 4 in both debug and optimized release. The whole
decompiler package, including every existing emitter and CFG oracle, passed
unchanged. The oracle inventory reported 70 matching digests and 47 compiled
rows, the auxiliary resource inventory reported 9 matching digests with intact
loaders, and a full `scripts/ci-check.sh` run exited 0, including the flake
check, both fmt lanes, shell and Python lint, workspace clippy, the named
protected targets, the workspace suite, the release CLI build, and the excluded
benchmark harness.

## 23. Adjudication record: block ledger contract protection

`crates/flutterdec-decompiler/tests/block_ledger_contract.rs` is the public ruler
for the block identity ledger: the stage partition and its dense-id rekeying, the
disposition of every built block, the reconciliation of reachable-unemitted
explanations against real traversal events, the closed cause and event
vocabularies, and the invalid-CFG rejection outcome with its raw-graph witness and
digest. It asserts all of that through the public `flutterdec_decompiler` API and
plants a failure for every accepted defect class, so it is exactly the kind of
file section 7 exists to pin.

It was unprotected. It had no digest row, no compiled sentinel, no entry in the
checker's Cargo target inventory, and no named invocation in either CI lane, so
the entire ledger contract could be removed without any gate noticing. This
record adds all four and nothing else.

### 23.1 The bypass this closes

Measured at `00e6115` before this record, in a disposable worktree, by deleting
`crates/flutterdec-decompiler/tests/block_ledger_contract.rs` and changing
nothing else:

- `cargo test --workspace` exited 0 with 28 result lines and 586 passed tests.
  The six ledger tests were simply absent from the reported suite.
- `scripts/ci-check.sh --skip-tests` exited 0 through every lane: the flake
  check, both fmt lanes, shell and Python lint, the benchmark identity gate,
  workspace clippy, all three named protected-target lanes, the compiled oracle
  inventory reporting 70 matching digests and 47 compiled oracle rows, and the
  auxiliary resource inventory reporting 9 matching digests with intact loaders.

That is the fail condition this record removes: a protected-oracle family whose
deletion leaves the workspace suite green and every guard passing.

### 23.2 Exact protected inventory and loader map

One new section 7 row, one new Cargo integration target in the checker's target
inventory, one new sentinel, one new loader-map row, and one new named invocation
in each CI lane:

| Protected test-only path | Target | Sentinel |
| --- | --- | --- |
| `crates/flutterdec-decompiler/tests/block_ledger_contract.rs` | `block-ledger-contract` | `complete_partition_reconciles_and_plants_fail_closed` |

The sentinel is defined only in this file, and it is the test that both
reconciles a complete partition and requires all eleven planted ledger defects to
fail closed, so the sentinel cannot be satisfied by any other protected row. The
hook is Cargo's automatic discovery of `crates/flutterdec-decompiler/tests/`, the
same `Hook::Autotest` family the other thirteen protected decompiler integration
targets use, so `the_protected_oracle_loader_chain_is_intact` now records
fourteen of them and both CI lanes must name all fourteen in one command line.
The checker's `TARGETS` entry is what makes `cargo metadata` the ruler for the
manifest: a target dropped by `autotests = false` is absent from metadata, and
one switched off with `test = false` is present but not testable, and neither
reads as a missing test in a listing.

No mutable product source, manifest, `.github/workflows/ci.yml`, or hook carrier
gained a digest row. Those stay unprotected for the reason section 7 already
gives: a whole-file digest over a file ordinary work must edit fires on
legitimate change and rules nothing.

### 23.3 Digest chains

Column order keeps this history outside the section 7 path-and-digest parser.

| Protected ruler | Prior sha256 | New sha256 at `addec19` |
| --- | --- | --- |
| `scripts/ci-check.sh` | `94cc9b90e935bb1ecdce4280a31315337d5345b5c74b165e395c74de6dd608f5` | `9dac174b7a2a4e3a0d14d182d292dac0dbc8b6c63679e859da7cc8dad21ea45e` |
| `scripts/check-oracle-inventory.py` | `21bea1f4fa438b0aae70b5477a1ec026f13007c0bf569d6f37435089999b47df` | `3900f505ea8aea59500c99fcf598013cffac55e9128ec9498f7811738bcbf71a` |
| `crates/flutterdec-decompiler/tests/provenance_audit.rs` | `ea5e8087b6940bb41dfffeb6e5aef0c86b669553e745d761b19f24d9f2ff88f8` | `56ecc65ac91a20ed3761aaecead84978aeff29ef204e38bb02e665b1220347e2` |
| `crates/flutterdec-decompiler/tests/block_ledger_contract.rs` | new | `6dba60e7d22f129d5178f430174508d404dca297f7296984b69b3fe9ed248b21` |

`scripts/ci-check.sh` is also a row of the auxiliary resource inventory in
`docs/resource-ruler-protocol.md`, so the same one-line change is adjudicated
there too, with the same old and new digests. The ledger contract file itself is
byte-unchanged by this record: it is protected as it already stood, so no ruler
was rewritten to make protecting it easier. The only changes in the three moved
rulers are the new protected path, target, sentinel, loader-map row, and named CI
invocation. No threshold, golden fixture, benchmark definition, frozen expected
value, product behavior, or existing acceptance changed, and no digest row other
than these moved.

### 23.4 Planted bypasses

Six plants, one at a time in a disposable worktree holding this record's exact
bytes, each restored before the next. `inventory` is
`scripts/check-oracle-inventory.py`, `guard` is
`cargo test -p flutterdec-decompiler --test provenance_audit`, `named` is the
fourteen-target decompiler lane both CI files run, and `workspace` is
`cargo test --workspace`.

| Plant | inventory | guard | named | workspace |
| --- | --- | --- | --- | --- |
| p1, the protected file deleted | 1 | 101 | 101 | 101, 494 passed, 14 of 15 ok |
| p2, `autotests = false` in the decompiler `[package]` | 1, 14 problems | 101 | 101 | 0, 535 passed, 15 of 15 ok |
| p3, `autotests = false` plus explicit `[[test]]` for every other target | 1, exactly 1 problem | 101 | 101 | 101, 494 passed, 14 of 15 ok |
| p4, target declared with `test = false` | 1, exactly 1 problem | 101 | 101 | 101, 494 passed, 14 of 15 ok |
| p5, the named invocation deleted from both CI lanes | 1 | 101 | 101 | 101, 500 passed, 15 of 16 ok |
| p6, one assertion of the protected file weakened | 1 | 0 | 0 | 0, 592 passed, 29 of 29 ok |

Every message names the exact path or target rather than a family. p1 reports
`crates/flutterdec-decompiler/tests/block_ledger_contract.rs is protected ... but
is not an existing regular file`, and the named lane reports `no test target named
block_ledger_contract`. p3 and p4 report one problem, `target
block-ledger-contract holds protected oracles and flutterdec-decompiler's
manifest no longer builds test target block_ledger_contract for tests`, with the
other thirteen targets still live; p4's guard adds `Cargo.toml sets test = false,
which silences protected oracles while every digest in section 7 still matches`.
p5 fires twice over, because `scripts/ci-check.sh` is itself a protected row: the
digest pass names its stale hash, and the guard names the command line the lane
must contain. p6 is the sharpest row of the table. The mutation drops the
fail-closed check for a reachable-unemitted block with no traversal event, keeps
the file compiling, keeps the sentinel's name, and leaves every test green - the
whole workspace suite is byte-for-byte the control's 592 passed and 29 of 29 ok,
and the digest pass is the only thing that fires.

p2 is the fake pass the row exists to remove: `cargo test --workspace` exits 0
while all fourteen protected decompiler integration targets, 57 tests, silently
leave the reported suite. Nothing in `--workspace` can see that; the metadata
half of the checker's target inventory and the named lanes are what fail.

The manifest plant must go inside `[package]`. Appended at the end of the file it
lands in `[dev-dependencies]` and is a manifest syntax error, so every command
fails for the wrong reason and the plant proves nothing.

### 23.5 Section 9 steps

1. Invariant: every stable public ruler is digest-pinned, listed in the Cargo
   target inventory, proved compiled by a sentinel, and invoked by name in both
   CI lanes.
2. Tests: section 23.1's before-state bypass and section 23.4's six plants, each
   restored, with the reduced-suite counts that show what `--workspace` cannot
   see.
3. Diff and digests: sections 23.2 and 23.3, plus the matching adjudication in
   `docs/resource-ruler-protocol.md`. No product behavior changes.
4. Reference preserved: `1371e42` and `00e6115` remain addressable; all prior
   bytes are recoverable with `git show`.
5. The section 7 row, checker inventory, sentinel, loader-map row, both named CI
   invocations, both adjudications, and the refreshed counts land in one atomic
   oracle-protection commit. No product source, threshold, golden fixture,
   benchmark definition, or existing oracle acceptance is touched.
6. L5 was re-run in full: the ledger target on its own, the fourteen-target
   decompiler lane, the core and IR lanes, the compiled inventory, the resource
   inventory, and a clean `scripts/ci-check.sh`.

### 23.6 Verification

The clean state after this record: the compiled inventory reports 71 matching
digests and 48 compiled oracle rows, the auxiliary resource inventory reports 9
matching digests with intact loaders, the fourteen-target decompiler lane passes
all fourteen targets, and the block-ledger target itself passes 6 of 6. The
workspace suite is 29 result lines and 592 passed, exactly 6 more than the 586 the
same suite reported with this file deleted and unprotected. A full
`scripts/ci-check.sh` run exits 0, including the flake check, both fmt lanes,
shell and Python lint, the benchmark identity gate, workspace clippy, all three
named protected-target lanes, both inventories, the workspace suite, the release
CLI build, and the excluded benchmark harness.

## 24. Adjudication record: local and GitHub CI enforce the same guards

Section 1's L5 named `scripts/ci-check.sh` as the hard gate and then conceded
that `.github/workflows/ci.yml` ran a subset of it. A guard that only runs on a
developer machine is enforced by whoever remembers to run it, so the subset was
the real gate for anything the workflow omitted. This record closes that
divergence in the additive direction only: the workflow gains the missing
commands, byte-identical to the local ones, and nothing existing is removed,
reordered into a pipeline, or allowed to fail.

### 24.1 The divergence this closes

Measured at `addec19`, comparing every command `scripts/ci-check.sh` runs against
every `run:` line of `.github/workflows/ci.yml`. Present in both lanes already:
the flake check, root `cargo fmt --all --check`, `scripts/lint-shell.sh`,
workspace clippy, all three named protected-target lanes,
`scripts/check-oracle-inventory.py`, `cargo test --workspace`, and the release
CLI build. Absent from the workflow:

| Local command | What went unenforced on GitHub |
| --- | --- |
| `nix develop -c ./scripts/lint-python.sh` | every Python checker self-test and every Python plant test, including the annotation-safety, join-audit, cross-audit and candidate-whitelist plants, plus a syntax check of every `*.py` in the tree |
| `./scripts/bench-identity-gate-test.sh` | both directions of the pre-measurement identity gate, the only thing that refuses a benchmark comparison whose two sides cannot be compared |
| `nix develop -c python3 scripts/check-resource-ruler.py` | the auxiliary resource inventory: 9 digests plus the allocator, phase-stack and plant loaders of `docs/resource-ruler-protocol.md` |
| `nix develop -c cargo fmt --manifest-path crates/flutterdec-bench/Cargo.toml --all --check` | fmt for the excluded benchmark harness, which no root `--all` reaches |
| `nix develop -c cargo clippy --manifest-path crates/flutterdec-bench/Cargo.toml --all-targets -- -D warnings` | clippy for the harness, same exclusion |
| `nix develop -c cargo test --manifest-path crates/flutterdec-bench/Cargo.toml` | the harness's own 38 tests, including the span-disjointness and allocator-lifecycle rulers |

The protected block-ledger target and the compiled oracle inventory were already
named in both lanes by section 23, and section 24.4 re-proves both rather than
assuming it.

### 24.2 Exact lanes added

Six new steps in `.github/workflows/ci.yml`, each a single `run:` line, placed in
the local script's order: `Lint Python scripts` and `Benchmark identity gate`
after `Lint shell scripts`, `Resource ruler inventory` after
`Compiled oracle inventory`, and `Benchmark harness format`,
`Benchmark harness clippy` and `Benchmark harness tests` after
`Build release CLI`.

Every command is the local one verbatim. Three consequences of that are
deliberate, not oversights. `scripts/lint-python.sh` runs through `nix develop`
because it needs `mapfile -d`, which the macOS runner's bash 3.2 does not have.
`scripts/bench-identity-gate-test.sh` runs without `nix develop`, exactly as
`scripts/ci-check.sh` invokes it: it and `scripts/bench-identity-gate.sh` are
plain POSIX-shell-plus-`[[` bash with no `mapfile`, no associative array and no
toolchain of their own, so wrapping it would add a divergence between the lanes
without adding a dependency it needs. No lane repeats another lane's command with
different options, and no path in the workflow is absolute or machine-specific.

The third is the one platform condition in this record, and it is measured, not
assumed. `Benchmark harness tests` carries `if: runner.os == 'Linux'`. The first
push of these lanes, run `32284265246` at `b97ee86`, passed all twenty steps on
`ubuntu-latest` and failed exactly one test on `macos-latest`:
`measure::tests::host_identity_is_readable_on_this_platform` panicked with `linux
VmHWM` at `crates/flutterdec-bench/src/measure.rs:411`, 37 passed and 1 failed.
That test asserts `peak_rss_bytes()` returns a value, and `peak_rss_bytes` reads
`VmHWM` from `/proc/self/status`, which its own doc comment at `measure.rs:310`
already records as absent outside Linux. So the harness suite is Linux-only by
construction, `scripts/ci-check.sh` is a Linux gate, and the condition runs the
same command on the same platform the local gate runs it on. It is a platform
restriction, not a relaxation: there is no `continue-on-error`, no `|| true`, and
no `if: always()` anywhere in the workflow, and the lane fails the job on
`ubuntu-latest`. `Benchmark harness format` and `Benchmark harness clippy` need no
condition and run on both runners, as they did in that same run. The alternative
would have been editing `crates/flutterdec-bench/src/measure.rs`, a protected row
of `docs/resource-ruler-protocol.md`, to weaken a ruler over a platform question;
that is the wrong direction and was not done.

### 24.3 The guard that keeps the lanes equal

`the_protected_oracle_loader_chain_is_intact` in
`crates/flutterdec-decompiler/tests/provenance_audit.rs` already required both
`CI_LANES` to name every discovered integration target in one command line, and
to run `scripts/check-oracle-inventory.py` as a lane of its own. That
single-command check is generalized to a four-entry `REQUIRED_LANE_COMMANDS`
list - the Python lint, the identity-gate regression suite, the compiled oracle
inventory, and the resource inventory - each matched as a complete line in both
lanes, after trimming the YAML `run: ` prefix. Whole-line matching is what makes
`echo`, a `|| true` suffix, and a divergent flag in one lane fail. A second
assertion requires `INVENTORY_CHECKER`'s command to still be a member of that
list, so moving the checker cannot silently empty the check it used to perform.

The three benchmark-harness lanes are not in the list. They are ordinary lint and
test lanes for a crate whose manifest path is ordinary work to change, and
pinning them by value would fire on legitimate change while proving nothing about
an oracle. `.github/workflows/ci.yml` is still not a section 7 row, for the
reason 12.1 gives: a whole-file digest over a file ordinary work must edit rules
nothing.

### 24.4 Digest chain

Column order keeps this history outside the section 7 path-and-digest parser.

| Protected ruler | Prior sha256 | Current sha256 in section 7 |
| --- | --- | --- |
| `crates/flutterdec-decompiler/tests/provenance_audit.rs` | `56ecc65ac91a20ed3761aaecead84978aeff29ef204e38bb02e665b1220347e2` | `1627b7b9a0b5634fa3d76c9aa71c0d12dbb386371e26783b251b565467a3a34d` |

That is the only moved digest. `scripts/ci-check.sh` is byte-unchanged by this
record, so its rows in this protocol and in `docs/resource-ruler-protocol.md`
both stand as section 23 left them: the parity is reached by adding to the
workflow, never by weakening the local script. No product source, manifest,
threshold, golden fixture, benchmark definition, frozen expected value, sentinel,
protected path, or existing oracle acceptance changed, and the protected
inventory is still 71 digests and 48 compiled oracle rows.

### 24.5 Planted bypasses

Eleven plants, one at a time in a disposable worktree holding this record's exact
bytes, each restored before the next. Every command in the first table is
extracted from `.github/workflows/ci.yml` by step name and run verbatim, so what
is exercised is the workflow's own command line and not a restatement of it.
`guard` is `cargo test -p flutterdec-decompiler --test provenance_audit`.

| Plant | Workflow command exercised | Clean | Planted |
| --- | --- | --- | --- |
| p1, a disposable `docs/plant-broken.py` holding `def broken(` | `Lint Python scripts` | 0, 19 files | 1, `SyntaxError: '(' was never closed` |
| p2, the A/A digest-mismatch abort of `scripts/bench-identity-gate.sh` turned into `if false` | `Benchmark identity gate` | 0, 9 of 9 | 1, 2 failures, both A/A mismatch directions named |
| p3, `scripts/bench-resource.sh` deleted | `Resource ruler inventory` | 0, 9 digests | 1, exactly 1 problem, `scripts/bench-resource.sh: protected file deleted or not regular` |
| p4, `crates/flutterdec-decompiler/tests/block_ledger_contract.rs` deleted | `Compiled oracle inventory` | 0, 48 oracles | 1, names the path as protected but absent |
| p4, same deletion | `Oracle loader guard` | 0, 14 targets | 101, `no test target named block_ledger_contract` |
| p5, `autotests = false` in the decompiler `[package]` | `Oracle loader guard` | 0 | 101, `no test target named provenance_audit` |
| p5, same manifest | `Compiled oracle inventory` | 0 | 1, 14 problems |
| p5, same manifest | `Test` | 0, 29 result lines | 0, 15 result lines, which is the fake pass the named lanes exist to remove |

| Plant | guard, clean | guard, planted |
| --- | --- | --- |
| p6, the `Lint Python scripts` step deleted from the workflow | 0 | 101 |
| p7, the `Benchmark identity gate` step deleted | 0 | 101 |
| p8, the `Resource ruler inventory` step deleted | 0 | 101 |
| p9, the `Compiled oracle inventory` step deleted | 0 | 101 |
| p10, `|| true` appended to the `Lint Python scripts` command | 0 | 101 |
| p11, the same command in `scripts/ci-check.sh` given a divergent `--quiet` flag | 0 | 101, and the section 7 digest pass names the stale `scripts/ci-check.sh` hash |

p6 through p10 are the plants that make 24.2 durable rather than decorative:
before this record, deleting any of those workflow steps was invisible to every
gate. p10 is the sharper half - the step is still there, still named, and still
runs the command, but its failure no longer fails the job, and whole-line
matching is what catches it. p11 is the mirror direction: the guard is symmetric,
so dropping a command from the local script fails for the same reason as
dropping it from the workflow.

### 24.6 Section 9 steps

1. Invariant: every command that makes `scripts/ci-check.sh` fail closed also
   runs in `.github/workflows/ci.yml`, byte-identical, as a step of its own, on
   every runner whose platform the command supports, and the four checker lanes
   are asserted in both files by value.
2. Tests: section 24.5's eleven plants, each restored, with the clean control for
   every exercised command.
3. Diff and digests: sections 24.2 and 24.4. One moved digest, no product
   behavior change, nothing removed from either lane.
4. Reference preserved: `1371e42` and `addec19` remain addressable; all prior
   bytes are recoverable with `git show`.
5. The workflow steps, the generalized guard, this record, the refreshed L5, and
   the refreshed `docs/development.md` and `docs/research-ir-cfg-emitter.md`
   statements land in one atomic CI commit.
6. L5 was re-run in full after the change: `scripts/ci-check.sh` exits 0, and
   `actionlint` accepts all three workflow files.

### 24.7 Verification

`actionlint` 1.7.12 exits 0 on `.github/workflows/ci.yml`,
`.github/workflows/test-suite.yml` and `.github/workflows/release.yml`. Each of
the six added commands was run locally from the workspace root and exits 0:
Python lint over 326 files with four checker suites, the identity gate at 9 of 9
cases, the resource inventory at 9 digests with intact loaders, the compiled
oracle inventory at 71 digests and 48 compiled oracles, harness fmt, harness
clippy under `-D warnings`, and the harness's 38 tests. The full local gate,
`scripts/ci-check.sh`, exits 0 with `all checks passed`, and the workspace suite
is 29 result lines and 592 passed.

On GitHub, run `32284265246` at `b97ee86` is the first push of these lanes and the
proof that every one of them really executes there rather than only parsing.
`Rust Checks (ubuntu-latest)` completed all twenty steps green, including
`Lint Python scripts`, `Benchmark identity gate`, `Resource ruler inventory` and
all three harness lanes. `Rust Checks (macos-latest)` completed nineteen green and
failed only `Benchmark harness tests`, on the Linux-only RSS assertion section
24.2 records; that runner had already passed the Python lint under `nix develop`,
the identity gate under its own bash 3.2, the resource inventory, and harness fmt
and clippy. The `if: runner.os == 'Linux'` condition on that one lane is the whole
difference between that run and this record's bytes.
