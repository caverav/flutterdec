# Research baseline: IR, CFG, and emitter

Status: research only. No product source, test, fixture, golden, or benchmark
file changed in the commit that adds this document. Every claim below is either
cited to a repository path and line at the reference commit, or marked as an
open question.

Reference commit: `1371e42` ("docs(research): record the switchable-call shape
and its two fabrications"), which is `origin/main` at the start of this work.
Branch: `research/ir-cfg-emitter`, created from that commit.

Line numbers are as of `1371e42`. When a later commit moves code, the anchor to
trust is the function name, not the line.

The companion document is
[oracle-protocol-ir-cfg-emitter.md](oracle-protocol-ir-cfg-emitter.md), which
fixes the correctness protocol, the case matrix, the protected rulers, and the
adjudication path.

## 1. Stage map, with sources

The whole `decompile` path, in call order.

| Stage | Entry point | Notes |
| --- | --- | --- |
| CLI parse and dispatch | `crates/flutterdec-cli/src/main.rs:41` (`enum Command`), `main.rs:476` (`Command::Decompile`), `main.rs:552` (calls `run_decompile`) | `decompile` flags at `main.rs:67-294`, quality gate flags at `main.rs:201-232` |
| Orchestration | `crates/flutterdec-core/src/pipeline/runners.rs:1184` (`run_decompile`) | `crates/flutterdec-core/src/lib.rs:302-311` pulls the pipeline in with `include!`, so `runners.rs` line numbers are inside one expanded module |
| Load and normalize input | `crates/flutterdec-loader/src/lib.rs:359` (`load_snapshot_bundle`), `lib.rs:351` (`load_snapshot_bundle_from_apk_session`), `lib.rs:63` (`ApkSession::open`) | called at `runners.rs:1189-1190` |
| Adapter model | `crates/flutterdec-core/src/pipeline/model.rs:9` (`load_model`), `crates/flutterdec-adapter/src/lib.rs:227` (`resolve_adapter_exec`), `adapter/src/lib.rs:256` (`run_adapter`) | called at `runners.rs:1191`; backends at `core/src/lib.rs:119-140` |
| Function scope and target selection | `runners.rs:1257-1263` | empty selection is a hard error at `runners.rs:1268-1291` |
| Disassembly | `crates/flutterdec-disasm-arm64/src/lib.rs:1363` (`disassemble_program_with_priorities_and_package_hints`), decode at `disasm/src/lib.rs:239-258`, Dart ABI annotation at `disasm/src/lib.rs:186-211` | called at `runners.rs:1293-1309`; capstone is the decoder (`crates/flutterdec-disasm-arm64/Cargo.toml`) |
| Optional record split | `crates/flutterdec-core/src/pipeline/runners/split.rs:77` (`split_points`), terminator test at `split.rs:141-150` | opt-in at `runners.rs:1314-1318`; it builds its own IR at `split.rs:89` |
| IR and CFG build | `crates/flutterdec-ir/src/lib.rs:188` (`build_function_ir`), `ir/src/lib.rs:377` (`build_program_ir`) | called at `runners.rs:1319` |
| Region analysis | `crates/flutterdec-decompiler/src/control_flow/regions.rs:31` (`Regions::build`) | dominators at `regions.rs:136`, reducibility at `regions.rs:174`, immediate post-dominators at `regions.rs:206`, natural loops at `regions.rs:279` |
| Structured emission | `crates/flutterdec-decompiler/src/control_flow/structured.rs:513` (`try_emit_structured`), `structured.rs:617` (`render_sequence`), `structured.rs:873` (`render_block_body`) | driven from `crates/flutterdec-decompiler/src/lib.rs:444` |
| DFS fallback emission | `crates/flutterdec-decompiler/src/control_flow/emit.rs:1264` (`emit_block`), helpers at `crates/flutterdec-decompiler/src/control_flow/graph.rs:1-117` | entered at `decompiler/src/lib.rs:445-449` |
| Omitted paths and helpers | `decompiler/src/lib.rs:531` (`emit_omitted_path`), `crates/flutterdec-decompiler/src/helper_flow/summary.rs:43` (`append_helper_functions`), `crates/flutterdec-decompiler/src/helper_flow/inlining.rs:148` (`inline_trivial_helpers`), `inlining.rs:189` (`collapse_remaining_helpers`) | sequenced at `decompiler/src/lib.rs:475-480` |
| Text passes and annotation | `decompiler/src/lib.rs:481-493`, program level rewrites at `decompiler/src/lib.rs:617-640` (`emit_program_with_runtime_stubs`) | compaction at `crates/flutterdec-decompiler/src/passes/compaction.rs:4` |
| Artifact write | `runners.rs:1518-1560` (`pseudocode/`, `asm/`, `ir/`), `runners.rs:1562-1594` (Ghidra and IDA scripts) | filenames are `{function_id:05}_{name}` |
| Quality scoring and gate | `crates/flutterdec-core/src/pipeline/quality.rs:36` (`quality_from_artifacts`), thresholds at `quality.rs:106-118`, per line counters at `quality.rs:8` | called at `runners.rs:1596` |
| Reports | `runners.rs:1823-1824` (`quality.json`), `runners.rs:2187-2190` (`report.json`), failure message at `runners.rs:165-183` | gate failure aborts after the artifacts are written (`runners.rs:2192-2206`) |

Two structural facts matter for later work:

- `flutterdec-core` composes its pipeline with `include!` (`core/src/lib.rs:302-311`),
  not with `mod`. A grep for a symbol can therefore land in a file that has no
  `use` statements of its own.
- The record splitter builds a throwaway IR before the real one
  (`split.rs:89`), so an IR change affects both the split decision and the
  emitted result.

## 2. Instruction classification as it stands

`llir_from_disasm` (`ir/src/lib.rs:104`) maps a capstone mnemonic to `IROp`
(`ir/src/lib.rs:5-17`) with the arms at `ir/src/lib.rs:139-176`:

| Mnemonic | `IROp` | Block ended at `ir/src/lib.rs:197-219` | Successors at `ir/src/lib.rs:254-289` |
| --- | --- | --- | --- |
| `bl`, `blr` | `Call` | no | fallthrough to next block |
| `b` | `Jump` | yes | branch target only |
| `b.<cond>` and anything starting with `b.` | `Branch` | yes | target plus fallthrough |
| `cbz`, `cbnz`, `tbz`, `tbnz` | `Branch` | yes | target plus fallthrough |
| `ret` | `Return` | yes | none |
| `ldr` with a `pool[` or `poolOff[` annotation | `LoadPool` | no | fallthrough |
| Dart stack-overflow guard group | `RuntimeCheck` | no | guard edge suppressed, slow path pruned (`ir/src/lib.rs:291-352`) |
| everything else, including `br` and `brk` | `Other` | no | fallthrough to the next block |

The last row is the classification gap recorded as risk R1 in section 7.

The stack-overflow guard is recognized by shape, not by offset
(`ir/src/lib.rs:33-58`), and both the guard and its slow path are removed by a
targeted prune rather than a blanket reachability prune
(`ir/src/lib.rs:291-299`). Blocks unreachable for other reasons are deliberately
kept, because they are usually code that the adapter merged in from a
neighbouring function.

## 3. Invariants the current code depends on

Each is a property some later stage relies on, with the code that establishes it
and the code that consumes it.

1. Dense block ids from zero, entry at id 0. Established at
   `ir/src/lib.rs:335-352` (remap after the guard prune). Consumed by
   `regions.rs:36-41`, which returns `None` if any `id >= n`, and by the split
   path comment at `runners.rs:1310-1313`.
2. Successors sorted and deduplicated: `ir/src/lib.rs:286-288`. Predecessors
   sorted and deduplicated: `ir/src/lib.rs:364-367`.
3. Predecessor and successor reciprocity, rebuilt from the surviving successor
   lists after the prune: `ir/src/lib.rs:354-362`.
4. Analysis is restricted to entry-reachable blocks, and unreachable blocks get
   their successor lists cleared instead of being deleted:
   `regions.rs:44-59`. `Regions::predecessors` is built from the same edges that
   `Regions::is_join` counts, so a caller enumerating a join cannot disagree with
   the join test: `regions.rs:61-69`, `regions.rs:96-106`.
5. No hash iteration order reaches output. The immediate post-dominator picks the
   largest post-dominator set with the block index as tie-break
   (`regions.rs:255-274`); loop exit targets are sorted and deduplicated before
   selection (`regions.rs:315-331`); the recorded provenance order is the
   ascending predecessor order (`regions.rs:100-106`).
6. Structured emission emits every reachable block exactly once, and this is
   verified rather than assumed: the coverage comparison at
   `structured.rs:530-539` compares `structured_emitted.len()` against
   `Regions::reachable_count`.
7. A structured decline is transactional. Lines, register state, counters, call
   anchors, join and loop provenance, and snapshots are all rolled back at
   `structured.rs:541-562`.
8. `_block_N` helpers are a scaffold, not an output form. Whatever survives
   inlining is replaced by `return null;` plus one `// omitted complex paths:`
   summary, and every remaining helper definition is dropped
   (`inlining.rs:189-243`).
9. Quality counters are read out of the emitted text through the same function a
   fixture calls, so a fixture cannot score against its own copy of the rules
   (`quality.rs:1-34`, with the four fixtures at `quality.rs:150-229`).

## 4. Fallbacks in force today

| Fallback | Trigger | Site |
| --- | --- | --- |
| Word decode | capstone unavailable or decode empty | `disasm/src/lib.rs:261-280` |
| Decline structuring | no blocks, non dense ids, or irreducible graph | `regions.rs:31-77` |
| Decline structuring | region tree does not describe an edge, or coverage incomplete | `structured.rs:646-652`, `structured.rs:530-539` |
| Structured depth cap | nesting deeper than 64 | `structured.rs:624-626` |
| Bounded region repeat | at most 16 blocks and 96 instructions | `structured.rs:975-999` |
| DFS depth cap | depth 12 in `emit_block`, 12 in `can_inline`, 10 for loop wrapping | `emit.rs:1269-1272`, `graph.rs:39-42`, `graph.rs:80-83` |
| DFS visit cap | 48 for a short jump or return block, 24 for a join, 14 otherwise | `helper_flow/summary.rs:30-41` |
| Omitted path | cap reached, or target not inlinable | `decompiler/src/lib.rs:531-538`, `emit.rs:1281-1284`, `emit.rs:1361`, `emit.rs:1383`, `emit.rs:1403`, `emit.rs:1448` |
| Helper generation cap | more than 64 distinct helper blocks | `helper_flow/summary.rs:53-55` |
| Helper collapse | any helper call left after inlining | `inlining.rs:189-230` |
| Empty body last resort | structured attempt failed and the DFS body came out empty | `decompiler/src/lib.rs:451-468` |
| Unresolved control flow counter | branch or jump target that resolves to no block | `emit.rs:1364-1370`, `emit.rs:1405-1412`, `structured.rs:750-757`, `structured.rs:908-917` |

The structured emitter never emits an omitted path. Confirmed by absence:
neither `emit_omitted_path` nor `omitted_blocks` appears in `structured.rs` or
in `src/passes/`. Every `_block_N` in an artifact therefore comes from a
function the structured emitter declined.

## 5. Quality rulers and artifacts

The scoring surface, which is what a candidate must not be allowed to move
silently:

- Counters and gates: `quality.rs:36-147`. Four gates fail the run:
  `placeholder_ifs`, `unresolved_cf`, `indirect_call_ratio`,
  `disassembly_ratio` (`quality.rs:106-118`).
- Default thresholds, from the CLI: `--max-placeholder-ifs 0`
  (`main.rs:201-208`), `--max-unresolved-cf 0` (`main.rs:209-216`),
  `--max-indirect-call-ratio 0.30` (`main.rs:217-224`),
  `--min-disassembly-ratio 0.80` (`main.rs:225-232`).
- Six per line text counters, in fixed order, read after the annotation spans
  are stripped: `quality.rs:1-34`. `block_helper_refs` counts `_block_`, which
  is what makes an unresolved helper reference visible in `quality.json`.
- The disassembly ratio denominator is the model function list and the numerator
  is pre-split records, deliberately (`quality.rs:92-99`, `runners.rs:1313`).
- Artifacts per run: `pseudocode/*.dartpseudo` always, `asm/*.s` with
  `--emit-asm`, `ir/*.json` with `--emit-ir`, `quality.json`, `report.json`, and
  optional `ghidra_apply_symbols.py` and `ida_apply_symbols.py`
  (`runners.rs:1518-1594`, `runners.rs:1823-1824`, `runners.rs:2187-2190`).
- No artifact carries a timestamp or a duration. A grep for `elapsed`,
  `duration`, `_ms`, `timestamp`, `generated_at`, `Instant::now`, and
  `SystemTime` over `crates/flutterdec-core/src/pipeline/*.rs` and
  `crates/flutterdec-cli/src/main.rs` returns nothing. The only volatile content
  in the reports today is absolute paths; they are enumerated as JSON pointers in
  the companion protocol.

## 6. Validation surface that exists

`nix develop -c cargo test --workspace` at `1371e42`: 15 test binaries, 432
tests, 0 failures, exit code 0.

Where those tests sit, by area:

- IR: 6 tests inline in `crates/flutterdec-ir/src/lib.rs:381-616`. They cover
  the guard group and its SDK offset drift, a non-guard compare, a block break
  after `ret`, conditional branch plus fallthrough, and `tbnz` target parsing.
- Disassembly: 32 tests in `crates/flutterdec-disasm-arm64/src/lib.rs`.
- Structuring and emission: `crates/flutterdec-decompiler/src/tests/`, largest
  files `cfg_and_stack/structuring.rs` (53), `emit_and_helpers/readability_and_naming.rs`
  (64), `cfg_and_stack/call_and_loops.rs` (46).
- Golden snapshots: 3, at `crates/flutterdec-decompiler/testdata/golden/`,
  compared by `assert_golden` (`src/tests/shared.rs:13-39`) and produced by
  `src/tests/golden_and_parser.rs:1-143`.
- Determinism: `cfg_and_stack/order_totality.rs` asserts one emission
  fingerprint across varied hash seeds, in process
  (`order_totality.rs:150-263`).
- Provenance with planted failures: `crates/flutterdec-decompiler/tests/provenance_audit.rs`
  and `tests/loop_entry_provenance_audit.rs`, each one test per process because
  the audit path is read once per process and the audit file is append only
  (`provenance_audit.rs:1-11`). Python side plants live in
  `scripts/prov_join_audit_plant_test.py`,
  `scripts/scan_annotation_safety_plant_test.py`, and the `--self-test` modes of
  `scripts/check-candidate-whitelist.py` and
  `scripts/prov_cross_audit_reconcile.py`.
- Integration parity: `scripts/ci-check.sh` runs `nix flake check`, format,
  shell lint, python lint, clippy with `-D warnings`, workspace tests, and a
  release build of the CLI.

Coverage gaps found while mapping the above:

- No test asserts a CFG relation against a literal expected graph. `Regions` is
  `pub(super)` and is constructed directly in exactly one place outside its own
  file, `src/tests/cfg_and_stack/structuring.rs:155`, and only to drive
  emission. Reachability, the dominator and post-dominator relations, follow
  nodes, loop membership, loop follow, and reducibility are therefore only
  observed through emitted text.
- No test crosses the 64 helper cap at `helper_flow/summary.rs:53`. The helper
  tests build helper text directly (`emit_and_helpers/helper_inlining.rs`,
  `cfg_and_stack/omitted_path_and_stack.rs`) rather than driving a function with
  more than 64 distinct omitted blocks.
- Determinism is asserted within one process only. Cross process byte identity
  of an artifact set is not checked anywhere.
- `.github/workflows/ci.yml` does not run `scripts/lint-python.sh`, so the
  python self tests and plant tests are enforced by `scripts/ci-check.sh`
  locally, not by the GitHub CI job. `.github/workflows/test-suite.yml` runs
  `scripts/test-suite.sh`.

## 7. Concrete risks

Each risk states its evidence class: proven by a repository test, proven by
inspection of cited code, or open.

**R1. `br` and `brk` get an invented fallthrough edge.** Evidence: inspection.
`ir/src/lib.rs:139-176` has no arm for either mnemonic, so both become
`IROp::Other`. `Other` does not create a leader (`ir/src/lib.rs:203-218`) and
takes the default successor arm, which pushes the next block
(`ir/src/lib.rs:278-282`). The same repository already treats both as path
enders elsewhere: `split.rs:141-150` returns true for `ret`, `brk`, `b`, and
`br`, and the comment at `split.rs:93-97` states that `build_function_ir` opens
a new block after `Branch`, `Jump`, and `Return` only. Consequences reach both
emitters: the DFS emitter falls through at `emit.rs:1437-1451` and the
structured emitter at `structured.rs:948-951`. `br` is also how a dispatch or
tail call leaves a function (`runners/stubs.rs:461`), so the fabricated edge
lands on the shapes that matter most.

**R2. The helper cap counts a block before refusing to define it.**
Evidence: inspection. `helper_flow/summary.rs:48-55` inserts the id into
`generated` and only then breaks on `generated.len() > 64`, so the 65th distinct
block is counted and never emitted. Blocks still queued behind it are dropped
with the loop. What keeps this out of the artifact is
`collapse_remaining_helpers` (`inlining.rs:189-206`), which rewrites any
surviving `return _block_N();` into `return null;` and records the id in the
`// omitted complex paths:` summary. So the visible failure mode is silent path
loss with an honest marker, not a dangling reference, and only while the collapse
net stays exact: `parse_helper_call` matches a trimmed line that starts with
`return _block_` and ends with `();` (`helper_flow/parse.rs:14-22`). Any future
helper call in another syntactic form escapes both the collapse and the removal.
Untested either way, see the gap in section 6.

**R3. `omitted_blocks` is not part of the structured rollback.** Evidence:
inspection. The rollback list at `structured.rs:541-562` restores lines, state,
counters, anchors, snapshots, and provenance, but does not touch
`self.omitted_blocks` (`decompiler/src/lib.rs:187`, written at
`decompiler/src/lib.rs:533`). Today this is unreachable, because no structured
path calls `emit_omitted_path` (section 4). It is recorded because the mission
intends to touch exactly that boundary: any future structured decline that first
emits an omitted path would leave the set populated, and
`decompiler/src/lib.rs:475` would then run helper generation for blocks the DFS
body never called.

**R4. CFG analysis is set based and quadratic in several places.** Evidence:
inspection, cost not yet measured. `dominators` holds one `HashSet` per block and
clones it per predecessor per iteration (`regions.rs:136-172`);
`immediate_post_dominators` does the same and then scores candidates with
`pdom[*p].len()` inside a `max_by_key` (`regions.rs:206-274`). Inside the IR
builder, block construction rescans the whole instruction list per block
(`ir/src/lib.rs:224-247`), the guard reachability closure resolves each successor
with `position` (`ir/src/lib.rs:321-327`), and predecessor rebuild resolves each
successor with `find` (`ir/src/lib.rs:354-362`). Whether any of this is the
dominant cost is the question the baseline milestone has to answer before an
algorithm is replaced.

**R5. The CFG layer has no independent oracle.** Evidence: repository search,
section 6. Any change to dominance or loop analysis is currently only observable
through emitted text, which means a wrong relation can be masked by an emitter
that declines and falls back.

**R6. The golden snapshots can be regenerated by the candidate.**
Evidence: inspection. `assert_golden` rewrites the snapshot when
`FLUTTERDEC_UPDATE_GOLDEN=1` (`src/tests/shared.rs:15-25`). That is convenient
and it is also the exact shape of a fake pass, so the three files are pinned by
digest in the companion protocol and the variable is forbidden for candidate
runs.

**R7. Ruler coverage differs between local parity and GitHub CI.** Evidence:
inspection of `.github/workflows/ci.yml` and `scripts/ci-check.sh`. The python
plant tests and self tests only run in the local parity script.

**R8. `parse_target_hex` accepts a bare token of more than six hex digits.**
Evidence: inspection, `ir/src/lib.rs:85-102`. A decimal immediate of seven or
more digits parses as hexadecimal and becomes the last candidate target. Low
severity, listed because branch target parsing is on the path this mission
touches.

**R9. Back edge detection in the DFS emitter uses address order.** Evidence:
inspection, `graph.rs:52-64` and `graph.rs:66-78` compare `start_va` to decide
whether a predecessor is behind or ahead. That is a proxy for dominance and can
disagree with the region analysis on a graph whose layout is not in address
order.

## 8. Real inputs that are not available

- No APK, AAB, or shared object is committed. A search for `*.apk`, `*.aab`, and
  `*.so` outside `target/` finds nothing.
- `testdata/real-golden/` holds a README, a `.gitignore`, and one
  `profiles/sample/profile.env`. There is no recorded `quality.json`,
  `report_metrics.json`, or `files.txt` baseline, so `scripts/real-golden.sh
  check` has nothing to compare against and `record` would need an external
  input (`scripts/real-golden.sh:7-22`, `testdata/real-golden/README.md`).
- Consequence for evidence: real-binary validation is blocked, not passing. The
  substitute for this milestone is a CLI smoke path that reaches input
  validation. Observed at `1371e42`:
  `cargo run -q -p flutterdec-cli -- decompile /nonexistent.apk -o /tmp/fdout`
  prints `Error: open apk: /nonexistent.apk` with `Caused by: No such file or
  directory (os error 2)` and exits 1.
- The measured numbers quoted in `regions.rs:1-14`, `regions.rs:322-329`,
  `emit.rs:1286-1298`, and `split.rs:75-76` come from sample binaries that are
  not in the repository. They are prior evidence for design intent, not
  reproducible here, and no new claim in this mission may lean on them.

## 9. External primary sources for methods

Repository convention for source lists is
`docs/research-pseudocode-quality.md:747-790`; this extends it rather than
starting a second scheme.

Instruction semantics, for the control-effect table in the companion protocol:

- Arm Architecture Reference Manual for A-profile architecture, document
  `DDI 0487`, Part C, Chapter C6, section C6.2 "Alphabetical list of A64 base
  instructions": `B`, `B.cond`, `BL`, `BLR`, `BR`, `RET`, `CBZ`, `CBNZ`, `TBZ`,
  `TBNZ`, `BRK`. Published at `developer.arm.com/documentation/ddi0487`.
- Dart SDK, `runtime/vm`, as already inventoried in
  `docs/research-pseudocode-quality.md:749-766`: `constants_arm64.h` for the
  register roles this pipeline keys on (`THR` R26, `SPREG` R15),
  `compiler/runtime_offsets_extracted.h` for the `Thread` field offsets behind
  the stack-limit drift that `ir/src/lib.rs:447-451` pins, and
  `compiler/stub_code_compiler_arm64.cc` for stub effects, cited in place at
  `emit.rs:740`.
- Capstone, the decoder actually used (`crates/flutterdec-disasm-arm64/Cargo.toml`),
  for the mnemonic spellings the classifier matches on.

Analysis and structuring methods worth testing later, none adopted by this
document:

- Lengauer and Tarjan, "A Fast Algorithm for Finding Dominators in a Flowgraph",
  ACM TOPLAS 1(1), 1979.
- Cooper, Harvey, and Kennedy, "A Simple, Fast Dominance Algorithm", Rice
  University, 2001: the iterative immediate-dominator formulation that would
  replace the dominator sets at `regions.rs:136-172` if measurement justifies it.
- Cytron, Ferrante, Rosen, Wegman, and Zadeck, "Efficiently Computing Static
  Single Assignment Form and the Control Dependence Graph", ACM TOPLAS 13(4),
  1991: relevant only after explicit definition and use effects exist in the IR,
  which they do not today.
- Havlak, "Nesting of Reducible and Irreducible Loops", ACM TOPLAS 19(4), 1997,
  for loop forests beyond the single-header bodies at `regions.rs:279-335`.
- Yakdan, Eschweiler, Gerhards-Padilla, and Smith, "No More Gotos", NDSS 2015,
  and Cifuentes, "Reverse Compilation Techniques", 1994, chapter 6, for
  pattern-independent and interval structuring of the graphs that
  `regions.rs:75-77` currently declines. Both are already in the repository
  source list.

## 10. Non-goals for this mission

Restating the charter so that a later commit cannot quietly widen scope:

- No full SSA construction, and no dominance-frontier machinery, while the IR
  has no explicit definition and use effects.
- No typed expression AST, and no removal of the text rewrite passes.
- No complete structuring of irreducible graphs, and no node splitting.
- No recompilable Dart. The pseudocode is deliberately not source equivalent, so
  compilation is not an available oracle.
- No snapshot runtime emulation and no recovery of exact source names.
- No speculative scaffolding. A partial abstraction for a later direction does
  not land without measured need.
- No weakening of a threshold, checker, golden, fixture, or baseline to make a
  candidate pass.

## 11. What this document does not establish

- No performance number in sections 1 to 10. There is no benchmark harness in
  the repository at `1371e42`, so nothing in those sections quantifies R4. The
  harness landed as a separate later commit and the measured attribution is in
  section 12, which supersedes this bullet for R4 specifically: R4 is now
  measured, and the answer is that it is not the dominant cost.
- No claim about real binary behavior beyond the input validation path in
  section 8.
- R1, R2, R3, R8, and R9 are inspection findings. Each becomes a failing test
  first, under the case matrix in the companion protocol, before any fix is
  written.

## 12. Measured cost attribution, from the accepted baseline

Everything in sections 12 to 16 is derived from the accepted A/A artifacts
under `docs/baseline/`, described in
[baseline-ir-cfg-emitter.md](baseline-ir-cfg-emitter.md), at product reference
`1371e42` and harness `8e7f080`. No product source, test, fixture, golden,
threshold, workload, or benchmark file changes in the commit that adds these
sections.

Reproduce every number here with:

```
python3 docs/baseline/phase-attribution.py docs/baseline/aa-1 docs/baseline/aa-2
```

Committed output: `docs/baseline/phase-attribution.txt`. The script reads only
the four committed sample streams (`aa-*/samples-{reference,candidate}.tsv`),
the case manifest, and the warmup correctness documents. It writes nothing but
stdout. Section numbers below refer to its output blocks.

| File | sha256 |
| --- | --- |
| `docs/baseline/phase-attribution.py` | `3fad0f287af98ee2657333d3f5889a26b7df66e063b05fd68ba418f9e58120d8` |
| `docs/baseline/phase-attribution.txt` | `820b5f77345a025b0ac3ba7fa597b4b536d66dbbac3f56c64ea52c6b1ccb73a5` |

Neither file is a protected ruler under
[oracle-protocol-ir-cfg-emitter.md](oracle-protocol-ir-cfg-emitter.md) section 7,
and neither moves an existing digest: both are new paths, and no accepted
baseline artifact is touched by this analysis.

### 12.1 Phase share of the workload

Time-weighted share of the whole 33 case workload, per binary. Each cell is the
sum over cases of that case's median phase nanoseconds, divided by the same sum
over `combined` (output block 1).

| Binary | total (ms) | ir | cfg | emission_exclusive | serialization |
| --- | --- | --- | --- | --- | --- |
| aa-1 reference | 8106.3 | 0.01499 | 0.03126 | 0.90847 | 0.04513 |
| aa-1 candidate | 8099.8 | 0.01495 | 0.03114 | 0.90869 | 0.04520 |
| aa-2 reference | 8116.8 | 0.01485 | 0.03103 | 0.90899 | 0.04509 |
| aa-2 candidate | 8093.7 | 0.01499 | 0.03124 | 0.90809 | 0.04582 |
| mean | | 0.01494 | 0.03117 | 0.90856 | 0.04531 |

The four binaries agree to within 0.001 on every phase, which is expected: they
are two builds of one revision to one digest.

This is not the same statistic as the per-phase table in
[baseline-ir-cfg-emitter.md](baseline-ir-cfg-emitter.md) section 7. There the
pooled medians give `emission_exclusive` 40.3 ms against `combined` 60.8 ms, a
ratio of 0.663. That is the median case. The 0.909 above is the share of total
time, and the two differ because the workload is extremely skewed. Both are
correct; the time-weighted one is the one an Amdahl argument needs, and it is
the one used below.

**Dominant term: `emission_exclusive`, 90.9 percent of workload time.** The
whole of IR construction, region analysis, and artifact serialization together
are 9.1 percent.

### 12.2 Where the time sits, by case

Per case share of total workload time, descending (output block 2). The full 33
row table is in the committed output; the head of it is the entire story.

| Case | blocks | combined (ms) | share | cumulative | emission share of the case |
| --- | --- | --- | --- | --- | --- |
| `irreducible/1024/base` | 1024 | 3018.3 | 0.37233 | 0.37233 | 0.98934 |
| `irreducible/256/base` | 256 | 1085.0 | 0.13385 | 0.50618 | 0.99368 |
| `multi-exit/1024/base` | 1024 | 905.5 | 0.11170 | 0.61788 | 0.97612 |
| `fan-in/1024/base` | 1024 | 798.6 | 0.09851 | 0.71640 | 0.97599 |
| `irreducible/64/base` | 64 | 376.0 | 0.04639 | 0.76278 | 0.98718 |
| `diamond-chain/1024/heavy` | 1024 | 339.1 | 0.04183 | 0.80461 | 0.66587 |

Five of 33 cases carry 76.3 percent of the workload. One case carries 37.2
percent.

By topology (output block 2b):

| Topology | combined (ms) | share of total | share of emission | share of emission allocations |
| --- | --- | --- | --- | --- |
| `irreducible` | 4479.3 | 0.55257 | 0.60228 | 0.57729 |
| `multi-exit` | 1049.9 | 0.12952 | 0.13857 | 0.15691 |
| `fan-in` | 904.4 | 0.11157 | 0.11937 | 0.12782 |
| `diamond-chain` | 773.4 | 0.09541 | 0.07065 | 0.07240 |
| `linear` | 609.7 | 0.07521 | 0.04839 | 0.04424 |
| `nested-loop` | 154.0 | 0.01899 | 0.01063 | 0.01093 |
| `no-exit` | 135.6 | 0.01673 | 0.01010 | 0.01042 |
| `irreducible` + `multi-exit` + `fan-in` | 6433.6 | 0.79366 | 0.86022 | 0.86202 |

The per case phase share spread (output block 3) shows how little of this is
uniform: `emission_exclusive` runs from 0.472 of a case (`nested-loop/1024/base`)
to 0.994 (`irreducible/256/base`), and `cfg` from 0.00024 to 0.322.

### 12.3 What the dominant cases actually do

The three topologies that carry 79.4 percent of the time are the three whose
emitted output does not track the graph. From the warmup correctness documents
(`aa-*/warmup-reference.json`, output block 5):

| Case | blocks | emitted lines | helper definitions | helper references |
| --- | --- | --- | --- | --- |
| `linear/64/base` | 64 | 131 | 0 | 0 |
| `linear/256/base` | 256 | 515 | 0 | 0 |
| `linear/1024/base` | 1024 | 2051 | 0 | 0 |
| `fan-in/64/base` | 64 | 255 | 0 | 0 |
| `fan-in/256/base` | 256 | 88 | 0 | 0 |
| `fan-in/1024/base` | 1024 | 88 | 0 | 0 |
| `multi-exit/64/base` | 64 | 506 | 0 | 0 |
| `multi-exit/256/base` | 256 | 88 | 0 | 0 |
| `multi-exit/1024/base` | 1024 | 88 | 0 | 0 |
| `irreducible/64/base` | 64 | 663 | 0 | 0 |
| `irreducible/256/base` | 256 | 663 | 0 | 0 |
| `irreducible/1024/base` | 1024 | 663 | 0 | 0 |

Three readings, in increasing strength of evidence:

1. Structured emission did not run on `fan-in` or `multi-exit` at 256 blocks or
   above. Structured emission emits every reachable block exactly once and
   verifies the count rather than assuming it (`structured.rs:530-539`). Every
   block in these graphs is entry reachable and carries nine instructions, so 88
   lines, which also has to hold the signature, the local declarations, the
   closing brace and the omitted-path summary, cannot be an emit-once rendering
   of 256 blocks. The 64 block rows point the same way from the other side:
   `multi-exit/64/base` emits 506 lines for 64 blocks, about four times the
   density of the structured topologies, which is the DFS inlining signature.
2. `irreducible` is declined by design and the harness asserts it
   (`the_matrix_exercises_both_emitters` in the bench workload module, which
   requires the irreducible body to be more than twice a structured body at 64
   blocks and to stay flat from 64 to 256). Region analysis rejects irreducible
   graphs at `regions.rs:75-77`, so all of that time is DFS fallback.
3. All 33 cases report zero helper definitions and zero helper references, so
   every omitted path that the DFS fallback produced was rewritten to
   `return null;` and every helper definition dropped, by
   `collapse_remaining_helpers` (`inlining.rs:189-206`). That is risk R2's
   silent path loss, now observed rather than inferred, on a quarter of the
   matrix.

So the dominant term is not analysis and not text volume. It is the DFS fallback
searching graphs whose result is then largely discarded. `fan-in/1024/base`
spends 779 ms of emission to produce 88 lines.

### 12.4 How each phase scales

Log ratio of the 1024 block cost to the 64 block cost, base 16, so 1.0 is linear
in block count and 2.0 is quadratic (output block 4).

| Topology | load | ir | cfg | emission_exclusive | serialization | combined |
| --- | --- | --- | --- | --- | --- | --- |
| `linear` | base | 1.342 | 1.920 | 0.982 | 1.004 | 1.090 |
| `linear` | heavy | 1.453 | 1.978 | 0.989 | 1.007 | 1.051 |
| `diamond-chain` | base | 1.404 | 1.902 | 0.995 | 0.992 | 1.063 |
| `nested-loop` | base | 1.409 | 1.947 | 0.971 | 0.992 | 1.122 |
| `no-exit` | base | 1.406 | 1.963 | 0.990 | 0.995 | 1.106 |
| `fan-in` | base | 1.446 | 1.794 | 1.308 | -0.254 | 1.254 |
| `multi-exit` | base | 1.471 | 1.814 | 1.277 | -0.394 | 1.213 |
| `irreducible` | base | 1.434 | 1.948 | 0.752 | 0.118 | 0.751 |

Four facts fall out:

- `cfg` is quadratic, 1.79 to 1.99 everywhere. This is the measured form of risk
  R4: the set based dominator and post-dominator solvers at `regions.rs:136-172`
  and `regions.rs:206-274`. R4 is real and it is quadratic, and it is also 3.1
  percent of the workload.
- `ir` is superlinear at 1.34 to 1.49, consistent with the per block rescan at
  `ir/src/lib.rs:224-247` and the `position` and `find` successor resolution at
  `ir/src/lib.rs:321-327` and `ir/src/lib.rs:354-362`. Also real, also 1.5
  percent.
- `emission_exclusive` is linear on the structured topologies and superlinear
  (1.28 to 1.31) on `fan-in` and `multi-exit`, where the output is constant. Cost
  grows with the graph while the result does not.
- `serialization` scales with the artifact, so it goes negative where the
  artifact shrinks as the graph grows.

Fitting each phase as `t = k * n^e` through the 64 and 1024 block medians of one
topology and solving for the block count where the two curves meet gives the
size at which region analysis would overtake emission on a reducible graph. The
inputs, aa-1 reference medians in milliseconds:

| Topology | cfg at 64 | cfg at 1024 | emission at 64 | emission at 1024 | crossover blocks |
| --- | --- | --- | --- | --- | --- |
| `nested-loop` | 0.183 | 40.415 | 4.011 | 59.175 | 1513 |
| `no-exit` | 0.122 | 28.126 | 3.636 | 56.641 | 2103 |
| `linear` | 0.114 | 23.420 | 3.485 | 53.012 | 2445 |
| `diamond-chain` | 0.107 | 20.816 | 5.259 | 82.889 | 4694 |

Inside the frozen matrix that crossover is never reached, so a CFG algorithm
change is a claim about larger functions than the matrix contains, not about the
matrix. Treat the four numbers as an order of magnitude, per confound 10.

### 12.5 Allocation shape

Allocation counters, summed over the 33 cases, reference side of aa-1 (output
block 6):

| Phase | allocations | bytes | share of count | share of bytes |
| --- | --- | --- | --- | --- |
| ir | 989907 | 82613383 | 0.00355 | 0.01745 |
| cfg | 0 | 0 | 0 | 0 |
| emission_exclusive | 275498776 | 4539361232 | 0.98697 | 0.95900 |
| serialization | 2646648 | 111449076 | 0.00948 | 0.02355 |
| combined | 279135331 | 4733423691 | 1.0 | 1.0 |

The `cfg` row is zero by instrumentation, not by behavior: region analysis runs
inside the emission span and shares its counter, because reading the counter
inside the CFG span is the one place the harness deliberately does not reach
(comment on `Measurement` in the bench crate, quoted in
[baseline-ir-cfg-emitter.md](baseline-ir-cfg-emitter.md) section 11). Every
`HashSet` clone in `dominators` is charged to `emission_exclusive` here.

The strongest single result in this analysis is that emission time is almost
exactly proportional to emission allocation count across the entire matrix
(output block 7):

- nanoseconds per allocation: 22.3 minimum, 34.0 maximum on 32 of 33 cases,
  median 25.8, with one outlier at 57.5 (`fan-in/64/base`, the smallest case of
  the group).
- Pearson correlation of emission nanoseconds against emission allocation count
  over the 33 cases: 0.9984. Least squares fit through the origin: 27.4
  nanoseconds per allocation.
- The spread of the per allocation rate is a factor of 1.5 while emission time
  itself spans a factor of 850, from 3.5 ms to 2986 ms.

Bytes per allocation are 10.3 to 62.9, so these are small allocations. The worst
case is stark: `irreducible/1024/base` performs 106.3 million allocations for
1.10 GB to emit 663 lines, which is 160398 allocations per emitted line.
`linear/1024/base` performs 1025 per emitted line. Neither is a good number, and
the ratio between them is the duplicate work.

The other phases are not allocation bound in the same way. Dividing each phase's
total workload time by its allocation count, both taken from the two tables
above, gives 122.8 nanoseconds per allocation for `ir` (121.5 ms over 989907)
and 138.2 for `serialization` (365.8 ms over 2646648), against 26.7 for
emission (7364.3 ms over 275498776). Those phases do real work between
allocations and emission largely does not.

## 13. Ceiling

Round ceilings for the frozen disclosed matrix, from the mean phase shares
(output block 8). Each column is the fraction of total workload time recovered
if the target phase were made that much cheaper.

| Target | share | -10 percent | -25 percent | -50 percent | removed entirely |
| --- | --- | --- | --- | --- | --- |
| `ir` | 0.01499 | 0.00150 | 0.00375 | 0.00750 | 0.01499 |
| `cfg` | 0.03126 | 0.00313 | 0.00781 | 0.01563 | 0.03126 |
| `emission_exclusive` | 0.90847 | 0.09085 | 0.22712 | 0.45424 | 0.90847 |
| `serialization` | 0.04513 | 0.00451 | 0.01128 | 0.02256 | 0.04513 |
| `ir` + `cfg` | 0.04625 | 0.00462 | 0.01156 | 0.02312 | 0.04625 |
| everything except emission | 0.09138 | 0.00914 | 0.02285 | 0.04569 | 0.09138 |

Three consequences, and they decide the experiment plan more than any preference
does.

1. **The round 1 ceiling on `combined` is 0.908.** No candidate can beat that on
   this matrix, and the practical ceiling of the largest single opportunity, the
   declined structuring group's emission, is 0.86022 x 0.90847 = 0.781 of total
   workload time.
2. **`ir`, `cfg`, and `serialization` cannot be promoted on the `combined`
   span.** Each is below the 5 percent MDE floor even if made free: 0.015, 0.031,
   0.045. Even all three removed together is 0.091, which clears the floor only
   in the impossible limit. A change to those phases must therefore be judged on
   its own phase cells, where the same MDE rule applies to that phase's own
   paired deltas, or on correctness and determinism merits under the mission's
   milestone 4 rule. This is a measured conclusion, not a preference: it is the
   reason risk R4 is not the first performance target despite being genuinely
   quadratic.
3. **The ceiling is exhausted by cases, not by phases.** Because five cases hold
   76.3 percent of the time, a candidate that improves the other 28 cases by 20
   percent moves the workload by 4.7 percent and does not clear the floor.

## 14. Ranked opportunities

Leverage is the measured share of workload time the family can address. Risk is
about the rulers the change has to pass, not about difficulty. Trial cost is what
one measured comparison of a first cut costs, on a 394 second A/A run plus the
implementation.

| Rank | Family | Leverage | Risk | Trial cost | Evidence |
| --- | --- | --- | --- | --- | --- |
| 1 | F1: remove duplicate per block work in the DFS fallback on graphs where structuring declines | 0.781 of workload, 0.860 of emission, 0.862 of emission allocations | High: it is the emitter, so it is one step from the goldens, the quality counters, and R2's collapse behavior | Medium to high | 12.2, 12.3, 12.4 |
| 2 | F2: cut per line and per instruction allocation churn in the shared emission text path | 0.908 ceiling, hits all 33 cases including the five dominant ones | Medium: can be made byte identical, and then the artifact digests prove it | Low to medium | 12.5 |
| 3 | F3: replace the set based dominator and post-dominator solvers (R4) | 0.031 now, quadratic, 0.322 of one case, crossover at 1500 to 4700 blocks | Medium: no independent CFG oracle exists yet, which is R5 | Low | 12.4 |
| 4 | F4: IR construction rescan and linear successor resolution | 0.015, superlinear at 1.34 to 1.49 | Low | Low | 12.4 |
| 5 | F5: serialization | 0.045, linear, tracks artifact size | Low | Low | 12.1, 12.4 |

F1 and F2 are distinct families even though both land in emission: F1 changes how
many times a block is rendered, F2 changes what one rendering costs. They are
separable in the measurement because F2 must leave all 33 artifact digests
unchanged and F1 need not.

Families considered and rejected before round 1, recorded so a later round does
not rediscover them as new:

- **Emit less work by tightening the DFS caps.** Rejected as metric gaming. It
  would cut the dominant term enormously and pass every gate the harness has
  today, while making the silent path loss in 12.3 worse. The artifact digest
  guard in section 15 exists to make this fail rather than pass.
- **Parallelism.** Rejected as out of scope. The metric is a single threaded per
  function span by contract and the harness pins one thread, so threading inside
  a function cannot be measured by this ruler. Threading across functions is not
  an optimization of the measured span at all: a search for `rayon`, `par_iter`,
  `thread::spawn` and `num_threads` over
  `crates/flutterdec-core/src/pipeline/` and `crates/flutterdec-cli/src/main.rs`
  returns nothing, so it would be new architecture, which the mission's
  non-functional requirements exclude.
- **A different allocator or an arena.** Rejected for round 1. A global allocator
  swap needs a dependency, which the benchmark protocol forbids, and it would
  move all four phases at once, which destroys attribution against a per phase
  ruler. A crate local arena is a real option but it is an architecture change
  with no measured need yet. Reducing the number of allocations is the in scope
  version of the same idea and is F2.
- **Caching region analysis across cases.** Rejected: it would be harness
  specific, since the product analyses each function once.
- **F5 as a round 1 candidate.** Rejected on ceiling: 0.045 is below the MDE
  floor on `combined` and the phase is already linear in artifact size.

Correctness defects, ranked independently of speed. These may be prioritized on
their own merits under mission milestones 3 and 5, and no performance argument
is needed to justify them.

| Rank | Defect | Evidence class | Where |
| --- | --- | --- | --- |
| D1 | Coverage collapse on large declined graphs: `fan-in` and `multi-exit` emit 88 lines at 256 and 1024 blocks, `irreducible` emits 663 lines at every size, with zero helper definitions and zero helper references, so every omitted path was rewritten to `return null;` | Measured, from the committed warmup documents; the mechanism is R2 | 12.3, `inlining.rs:189-206`, `helper_flow/summary.rs:48-55` |
| D2 | R1, `br` and `brk` get an invented fallthrough edge | Inspection | Section 7 |
| D3 | R3, `omitted_blocks` is not part of the structured rollback | Inspection | Section 7 |

D1 is new to this document and outranks the performance work in importance: the
current gates cannot see it. All 33 cases pass the harness correctness pass, and
the four quality gates (`quality.rs:106-118`) do not count emitted blocks against
graph blocks.

## 15. Frozen experiment plan

Frozen before any product edit. A later commit may record an outcome against
these rules; it may not restate the rules to fit a result.

### 15.1 Target and scope

- **Target phase: `emission_exclusive`.** Chosen by 12.1 and 13, not by
  preference.
- **Target cases, disclosed:** `irreducible/1024/base`, `irreducible/256/base`,
  `multi-exit/1024/base`, `fan-in/1024/base`, `irreducible/64/base`. These are
  the five cases holding 76.278 percent of workload time.
- **Guard set:** the remaining 28 disclosed cases, all five phases. A candidate
  must not pay for the target with them.
- **Held-out:** drawn by an independent validator after the candidate commit
  exists, per the mission metric protocol. Nothing in this plan may be tuned to
  it, and no worker may hold its seed or manifest.

### 15.2 Protected paths

Unchanged from the companion protocol section 7, restated here so the experiment
plan carries them: the three golden snapshots, the quality thresholds and gate
logic, the provenance checkers and their plant tests, the benchmark workload
definitions and case matrix, the harness patch and its digest, the accepted
baseline artifacts under `docs/baseline/`, and `scripts/ci-check.sh`. A candidate
that needs one of these to move uses the adjudication path in protocol section 9,
in its own commit, before the measured comparison.

### 15.3 First round candidates, at most three

| Id | Family | What it changes | Judged on |
| --- | --- | --- | --- |
| E1 | F2 | Allocation churn in the shared emission text path, required to be byte identical | `emission_exclusive`, pooled and on the five target cases |
| E2 | F1 | Duplicate per block rendering in the DFS fallback on declined graphs | `emission_exclusive` on the five target cases |
| E3 | F3 | Dominator and post-dominator solvers | `cfg` phase cells only, plus determinism and correctness |

E3 is in the round because its trial cost is low and its phase cells are its own
bar. It is not in the round as a `combined` span claim; by 13 it cannot be one.

### 15.4 The MDE rule

Frozen as a rule, not as a number.

For any comparison, take that comparison's own 15 paired runs. For each pair
form the relative delta `d_i = (candidate_i - reference_i) / reference_i`. Then:

```
delta = median(d_i)
noise = median(|d_i - delta|)
MDE   = max(0.05, 3 * noise)
```

Recompute `delta`, `noise` and `MDE` separately for every comparison, every
phase, and every case cell that a decision is taken on. Do not carry a number
across comparisons.

No numeric MDE from [baseline-ir-cfg-emitter.md](baseline-ir-cfg-emitter.md) is
a threshold for any candidate, and none is reproduced here as one. That document
reports what its own two runs measured; an independent pair of A/A runs of the
same binding on the same host measured noise of 0.027 to 0.029 on `ir`, `cfg`
and `serialization` against the 0.017 to 0.026 published there, giving MDEs of
0.081 to 0.088 rather than 0.050 to 0.079. Quoting the published numbers would
understate the bar by up to 0.038 on exactly the phases where the floor does not
bind. The A/A figures are noise floor evidence for the ruler, not thresholds for
a candidate.

### 15.5 Accept

A performance candidate is accepted only when all of these hold in one measured
comparison:

- A1: pooled `emission_exclusive` over the 33 disclosed cases has
  `delta <= -MDE`, with `MDE` recomputed from that comparison's own 495 paired
  deltas.
- A2: each of the five target cases has `delta <= -MDE` on
  `emission_exclusive`, with `MDE` recomputed from that cell's own 15 paired
  deltas.
- A3: no disclosed case and phase cell slows by more than
  `max(0.10, MDE_cell)`, with `MDE_cell` recomputed per cell.
- A4: the held-out matrix, drawn after the candidate commit, clears its own
  recomputed pooled `MDE` on `emission_exclusive` in the same direction.
- A5: all 33 `artifact_sha256` values equal the accepted baseline values for a
  candidate that claims performance only; 33 of 33 correctness cases pass with
  `correctness_failures` empty; emission allocation count does not rise on any
  target case; `within_memory_limit` true and `runs_over_timeout` empty for both
  binaries; span reconciliation stays inside the 2 percent tolerance with every
  residue positive.
- A6: `scripts/ci-check.sh` exits 0, the workspace suite passes, the three golden
  digests are unchanged, and every protected digest in protocol section 7 is
  unchanged or adjudicated under section 9 in its own earlier commit.

For E3, replace A1 and A2 with the `cfg` phase pooled cell and the `cfg` cells of
the cases where `cfg` is largest (`nested-loop/1024/base` at 0.322 of its case,
`linear/1024/base` at 0.232), and keep A3 to A6 as written.

### 15.6 Kill

Any one of these kills the candidate for the round:

- K1: the target phase delta fails to reach `-MDE` after at most two measured
  comparisons of that candidate.
- K2: any `artifact_sha256` moves while the candidate claims performance only.
- K3: any cell slows beyond `max(0.10, MDE_cell)`.
- K4: any correctness case fails, any golden or protected digest moves without
  adjudication, or any lane of `scripts/ci-check.sh` fails.
- K5: reconciliation exceeds 2 percent on any measured pass, or any residue goes
  negative.
- K6: emission allocation count rises on a target case while the delta does not
  clear `MDE`, which is churn moved rather than removed.

A killed candidate is recorded with its measured numbers. It is not retried in
the same round.

### 15.7 Stop

Stop the performance track when either holds:

- S1: no candidate in the round clears its own recomputed `MDE` on the frozen
  target phase, and no untried family has a leverage above 0.05 of workload time,
  which is the MDE floor. By section 13 that is already true of F4 and F5, so
  after F1, F2 and F3 the untried set is empty by construction.
- S2: the ceiling is exhausted: the residual share of `emission_exclusive` on the
  target cases falls below 0.05 of workload time, so no further candidate could
  clear the floor on `combined`.

Correctness work under D1, D2 and D3 does not stop with the performance track. It
lands on its own merits with its own failing test first, per the companion
protocol.

## 16. Confounds

Recorded because each one can make a candidate look better or worse than it is.

1. **Workload skew.** One case is 37.2 percent of the workload and five are 76.3
   percent. Any pooled statistic over the 33 cases is mostly a statement about
   `irreducible`. The per case cells are the honest unit, which is why A2 and A3
   are per cell.
2. **Two different phase shares.** The pooled per phase medians in the baseline
   document give emission 0.663 of `combined`; the time-weighted share is 0.909.
   Quoting one where the other belongs changes every ceiling in section 13.
3. **`cfg` allocations are charged to emission.** The counter is read at the
   emission boundaries only, so every allocation in `Regions::build`, including
   the `HashSet` clones of R4, appears in the `emission_exclusive` allocation
   column. An allocation guardrail therefore cannot separate F2 from F3, and A5
   is written per case rather than per subsystem for that reason.
4. **`cfg` is not all control flow analysis.** The span covers `Regions::build`
   alone. The structured emitter's own region walking, the DFS back edge tests at
   `graph.rs:52-78`, and the visit accounting are all inside
   `emission_exclusive`. "CFG cost is 3.1 percent" is a statement about
   `Regions::build`, not about control flow analysis in general.
5. **Emission time is not proportional to emitted output.** `fan-in/1024/base`
   spends 779 ms to emit 88 lines. A candidate that emits less would look like a
   large win. A5 and K2 exist to reject that.
6. **Residual A/A skew.** The build layout bias is gone, but `emission_exclusive`
   still read +0.07 percent in one A/A run and -0.31 percent in the other. Any
   claim smaller than a few tenths of a percent on one phase is not separable in
   a single run of this size.
7. **Noise is run specific.** See 15.4. The independent A/A pair measured 0.027
   to 0.029 where the published runs measured 0.017 to 0.026.
8. **Fifteen pairs is odd.** The alternating schedule leaves one pair of position
   imbalance, quantified in the baseline document section 8.
9. **Single host, single session.** Nothing here characterises cross machine
   variance or a loaded machine.
10. **The exponents in 12.4 are two point fits.** They are computed from the 64
    and 1024 block cases only, with no intermediate check beyond the 256 block
    case being present in the same table. The crossover block counts derived from
    them are an extrapolation past the matrix and are used only to argue that F3
    matters later, never as an accept criterion.
11. **The matrix is synthetic.** It is deterministic generated ARM64, not a real
    Flutter snapshot. No real binary or baseline is committed (section 8), so the
    share of real work that looks like `irreducible` is unknown. The ranking
    above is a ranking on this matrix.

## 17. Harness activation transient at `6430765`

Disclosure, recorded after the fact. The first harness commit put the benchmark
crate inside the product workspace, and Cargo feature unification then turned the
benchmark instrumentation on for product builds at that one revision. The tip is
isolated, no accepted evidence was measured on the affected revision, and the
semantic suite passes with the instrumentation active. What was wrong is the
earlier structural argument, which was stated without a from-when qualifier: the
`exclude` line is what forbids unification, and that line does not exist before
`1501bce`.

### 17.1 The defect

At `6430765` the root manifest listed the harness as a workspace member, and the
harness manifest asks for the feature on both product crates:

```
git show 6430765:Cargo.toml
git show 6430765:crates/flutterdec-bench/Cargo.toml
```

```
members = [
  "crates/flutterdec-cli",
  "crates/flutterdec-core",
  "crates/flutterdec-loader",
  "crates/flutterdec-adapter",
  "crates/flutterdec-disasm-arm64",
  "crates/flutterdec-ir",
  "crates/flutterdec-decompiler",
  "crates/flutterdec-bench",
]
resolver = "2"
```

```
flutterdec-decompiler = { path = "../flutterdec-decompiler", features = [
  "bench-spans",
] }
flutterdec-core = { path = "../flutterdec-core", features = ["bench-spans"] }
```

Resolver 2 unifies features across the packages selected in one build
invocation. The harness is one of the selected packages in any `--workspace`
build at that revision, so `flutterdec-core` and `flutterdec-decompiler` were
compiled with `bench-spans` on for every `--workspace` build, clippy run and
test, and for a bare `cargo build --release` as well, since the root workspace
declares no `default-members`.

What was compiled in is the timing instrumentation and nothing else, and the
whole of it is three `cfg` gates added by the same commit
(`git diff 209a8fe 6430765 -- crates/flutterdec-core crates/flutterdec-decompiler`,
223 insertions and 1 deletion over 5 files):

- `control_flow/structured.rs`, in `try_emit_structured`: two
  `std::time::Instant` reads around the existing `Regions::build` call and one
  thread-local `Cell<u64>` add. The single deleted line is the old
  `let Some(regions) = Regions::build(self.ir) else {`, replaced by a `built`
  binding that both gate arms produce identically.
- `flutterdec-decompiler/src/lib.rs`: a gated `bench_spans` module holding that
  counter.
- `flutterdec-core/src/lib.rs`: a gated `bench_spans` module that calls the
  existing serialization code and returns a byte count.

Both `[features]` blocks are empty by default. The comment above them reads "Off
in every product build", which is true at the tip and was not true at
`6430765`.

### 17.2 Exact interval

Committer dates from the GitHub API, not from local clocks
(`gh api repos/caverav/flutterdec/commits/<ref> --jq .commit.committer.date`):

| Revision | Committed (UTC) | State |
| --- | --- | --- |
| `209a8fe` | 2026-08-18T03:58:59Z | no harness in the tree |
| `6430765` | 2026-08-18T04:59:03Z | opened: harness added as a workspace member |
| `1501bce` | 2026-08-18T05:32:32Z | closed: `exclude = ["crates/flutterdec-bench"]` |

Elapsed 33 minutes 29 seconds. `6430765` is the parent of `1501bce`
(`git rev-parse 1501bce^` is `6430765`), so the affected set is exactly one
revision of one branch, and no commit on the branch other than `6430765` has the
harness in `members`.

### 17.3 Files that opened and closed it

`git show --stat 1501bce`, 8 files. One of them is the fix and four follow from
it. The commit also carries harness changes that have nothing to do with the
isolation, which is recorded here rather than smoothed over.

| Path | Role |
| --- | --- |
| `Cargo.toml` | the fix: removes the harness from `members`, adds `exclude = ["crates/flutterdec-bench"]` with the reason |
| `.gitignore` | follows: ignores `crates/flutterdec-bench/target`, which the harness now builds into |
| `Cargo.lock`, `crates/flutterdec-bench/Cargo.lock` | follows: 11 lines leave the product lock, the harness gets its own 799-line lock |
| `crates/flutterdec-bench/Cargo.toml` | follows: own `[workspace]` table, and version, edition and license spelled out, because an excluded crate inherits nothing |
| `scripts/ci-check.sh` | follows: 14 lines adding the harness clippy and test lanes, since `--workspace` no longer reaches it; the fmt lane arrives at `1b11f7e` |
| `crates/flutterdec-bench/src/main.rs`, `scripts/bench-pipeline.sh` | unrelated to the isolation: `--runs 0` becomes valid, plus the output-directory lock, the raw-sample refusal and the per-binary warmup schedule |

The commit message of `1501bce` states the defect and the fix in its own words.
This section exists because the message is not where a reader looks for it, and
because the branch forbids force push, so history cannot be amended.

### 17.4 Symbol and binary probes

All three columns below were built in one disposable worktree at the single path
`/tmp/pb-6430765`: first at `6430765` as committed, then in the same worktree
with only the `members` line replaced by `exclude`, then with the tip checked out
into that same worktree. The build path is therefore identical across the three
columns and cannot enter the comparison:

```
git worktree add --detach /tmp/pb-6430765 6430765
nix develop -c cargo tree --workspace -e features | grep -c bench-spans
nix develop -c cargo build --release --workspace
strings target/release/libflutterdec_decompiler.rlib | grep -c take_cfg_nanos
strings target/release/libflutterdec_core.rlib | grep -c serialize_artifacts
sha256sum target/release/flutterdec
git checkout --force --detach 70b2feb
```

| Probe | `6430765` as committed | same tree, `exclude` instead of `members` | tip `70b2feb`, same worktree |
| --- | --- | --- | --- |
| `cargo tree --workspace -e features`, `bench-spans` activations | 3 | 0 | 0 |
| `take_cfg_nanos` in the release decompiler rlib | 3 | 0 | 0 |
| `add_cfg_nanos` in the release decompiler rlib | 3 | 0 | 0 |
| `serialize_artifacts` in the release core rlib | 4 | 0 | 0 |
| `flutterdec` release CLI sha256, path-bound | `1354353263992a45ec44032f2f116b048210b306f66b532a0f6887216e309dd3` | `45b4c30c743ce9589446f18740ea13f6b05294f55c76cd1c1e21646f4ce9d05d` | `45b4c30c743ce9589446f18740ea13f6b05294f55c76cd1c1e21646f4ce9d05d` |
| `flutterdec` release CLI bytes | 17398888 | 17393976 | 17393976 |

The three digests are path-bound and are not portable numbers. The worktree path
enters the crate metadata hash, so one source tree built at two paths yields two
different digests at the same byte count. It is the same mechanism section 1 of
the companion baseline records when it explains why both A/A sides are built in
one canonical path and only the finished binary is copied into a side slot.
Rebuilding this table at another path reproduces the row structure, the symbol
counts, both byte counts and the equality of the last two columns, and reproduces
none of the three digest values. Only equality within one path is a claim about
the code.

Two readings matter here. Flipping that one manifest line changes the shipped CLI
binary, which is the strongest available statement that the product build was
genuinely different. And at this one path the corrected `6430765` tree and the
tip produce a byte-identical CLI, 17393976 bytes under one digest, which is what
places the whole product delta of this branch inside the `cfg` gates.

The bare string `bench_spans` still returns 1 hit per rlib at the tip. That is
the feature name in the crate metadata feature table, not compiled code. Count
the three function symbols separately or the metadata name reads as a false
positive.

### 17.5 Semantics at the transient revision

`nix develop -c cargo test --workspace` at `6430765`: 466 passed, 0 failed, exit
0, over 16 targets. The tip is 432 passed, 0 failed, over 15 targets. The
difference is the harness's own 34 tests, which `--workspace` reached only while
the harness was a member; the 432 product tests are the same 432, and they passed
with the instrumentation compiled in. That includes the three golden snapshots,
so the emitted artifacts are unchanged by the gate.

The instrumentation was also linked into the test build, not only the release
build: `strings target/debug/deps/libflutterdec_decompiler-*.rlib` at that
revision returns 14 hits for `add_cfg_nanos` and 13 for `take_cfg_nanos`.

### 17.6 No baseline or accepted artifact is on the transient revision

- The first commit that adds anything under `docs/baseline/` is `3aa2fe4` at
  2026-08-18T06:16:26Z, which is 43 minutes 54 seconds after `1501bce`. There is
  no earlier measurement record in the branch:
  `git log --diff-filter=A -- docs/baseline` has four commits, all later.
- Exactly two harness patches were ever committed, the invalidated
  `harness-b4b1d8c.patch` (added by `3aa2fe4`, deleted by `bf9a0eb`) and the
  accepted `harness-8e7f080.patch`. Both contain
  `exclude = ["crates/flutterdec-bench"]`, so every worktree either patch ever
  produced was isolated. `grep -c 'exclude = \["crates/flutterdec-bench"\]'` is 1
  on each.
- The two committed A/A runs bind `harness_ref 8e7f080`
  (`docs/baseline/aa-*/binding.txt`) and produced one binary digest `bc06f2bf...`
  for both sides. The two validator runs held under the mission evidence
  directory bind `harness_ref bf9a0eb` instead, which is the docs-only commit
  that recorded the accepted baseline;
  `git diff 8e7f080 bf9a0eb -- . ':!docs'` is empty, so that is a label
  difference and not a different harness. All four runs bind product `1371e42` on
  both sides and the same `patch_sha256 14413796...`, which is the binding that
  actually holds across the four.
- No measured artifact references the transient revision:
  `grep -rl 6430765 docs/baseline/` matches zero files. Under `docs/` the
  revision appears only in disclosure prose, this section included, and in the
  companion protocol's digest-chain table.

### 17.7 The probe that cannot see this

`cargo build -p flutterdec-decompiler -p flutterdec-core` returns zero symbol
hits at `6430765`, at the exact revision where `--workspace` returns three,
because unification applies only to the packages selected in one invocation. Any
future claim that the instrumentation is absent from a product build has to use
`--workspace`, or workspace membership from `cargo metadata`, and never `-p`.

### 17.8 Correction to an earlier claim

The structural argument used in earlier notes on this branch, that the root
manifest's `exclude` makes it impossible for feature unification to turn
`bench-spans` on, holds from `1501bce` onward and not before it. It is a
commit-scoped property of the manifest, not a property of the branch, and it must
be cited with the revision it applies to.

### 17.9 Already adjudicated: `scripts/ci-check.sh`

Separate from the above and disclosed here so the two are not conflated. The
harness commits also changed a protected ruler: `scripts/ci-check.sh` moved at
`1501bce`, `1b11f7e` and `5aa4b4e`, all before baseline acceptance. That change
is adjudicated in the companion protocol section 10, landed in `bcdc017` under
VAL-ORACLE-002, with the digest chain `9d994285...` to `675099447f...` to
`6ee0cdf976...` to `2f76a8b9...`, and the additivity proof: 26 insertions and 3
deletions against `1371e42`, and the only three lines the diff removes are the
usage-heredoc list numbers `5)`, `6)` and `7)`, which reappear with the same
command text as `6)`, `7)` and `8)`. Zero executable lines were removed and four
lanes were added.

### 17.10 What this leaves open

No gate detects a members-versus-`exclude` regression, and the instrumentation is
invisible to every check the repository runs. At `6430765`, with the
instrumentation active, `cargo fmt --all --check` exits 0,
`cargo clippy --workspace --all-targets -- -D warnings` exits 0, and
`cargo test --workspace` is 466 passed and 0 failed. A green check is exactly
what a members regression produces, so the probes in 17.4 remain manual. A gate
asserting zero `bench-spans` activations in `cargo tree --workspace -e features`
would close it in one line; it is not written, because it is a checker change and
this record is docs-only.
