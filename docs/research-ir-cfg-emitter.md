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

- No performance number. There is no benchmark harness in the repository at
  `1371e42`, so nothing here quantifies R4. The harness lands as a separate
  later commit, before any baseline is collected.
- No claim about real binary behavior beyond the input validation path in
  section 8.
- R1, R2, R3, R8, and R9 are inspection findings. Each becomes a failing test
  first, under the case matrix in the companion protocol, before any fix is
  written.
