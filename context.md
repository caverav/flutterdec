## Why this project exists

This repository is a reboot of a Flutter decompiler research effort. The core goal is simple:

- take a real Flutter AOT binary
- recover control flow and data flow
- emit readable pseudo Dart, not just assembly

The project is focused on static analysis first. It is designed for reverse engineering research, security work, and interoperability study on binaries you are legally allowed to analyze.

## What the research says

This repository keeps the research conclusions directly in this document. The important conclusions that drive this implementation are:

- parsing Dart snapshots is hard and changes across versions
- existing tooling already solves parts of parsing well
- the novel part is the decompiler pipeline from machine code to readable pseudo Dart
- adapter based parsing is the safest way to survive Dart and Flutter version churn
- runtime and dynamic instrumentation are useful as optional fallback, not as the default path
- strict quality gates are necessary to stop unreadable pseudocode from looking "done"

That is why this codebase separates snapshot extraction from decompilation logic. We can swap parsing adapters without rewriting the decompiler core.

## High level architecture

The pipeline is:

1. load input (`apk` or `libapp.so`) and locate snapshot blobs and instruction regions
2. run an adapter to produce a normalized program model
3. disassemble ARM64 instructions with Dart ABI aware annotations
4. lift to low level IR and build CFG
5. emit structured pseudo Dart with readability passes
6. write reports and enforce quality gates

Current module layout:

- `crates/flutterdec-loader`: APK and ELF loading, snapshot bundle extraction
- `crates/flutterdec-adapter`: adapter execution and model contract handling
- `crates/flutterdec-disasm-arm64`: ARM64 disassembly and call or branch tagging
- `crates/flutterdec-ir`: LLIR plus basic block and CFG construction
- `crates/flutterdec-decompiler`: pseudo Dart emission and readability transforms
  - internal split:
    - top-level orchestration in `src/lib.rs`
    - CFG flow entry in `src/control_flow.rs`
    - instruction lifting in `src/control_flow/expression_lift.rs`
    - CFG edge logic in `src/control_flow/graph.rs`
    - block and branch emission in `src/control_flow/emit.rs`
    - readability pass pipeline entry in `src/passes.rs`
    - pass internals in `src/passes/compaction.rs`, `src/passes/structural_helpers.rs`, `src/passes/naming.rs`, and `src/passes/expr_cleanup.rs`
    - structural helper details in `src/passes/structural_helpers/block_and_conditions.rs`, `src/passes/structural_helpers/guard_and_flow.rs`, and `src/passes/structural_helpers/naming_support.rs`
    - helper-flow entry in `src/helper_flow.rs`
    - helper parsing in `src/helper_flow/parse.rs`
    - helper inlining and collapse in `src/helper_flow/inlining.rs`
    - helper summary and visit-limit logic in `src/helper_flow/summary.rs`
    - helper utility entry in `src/helpers.rs`
    - register parsing in `src/helpers/registers.rs`
    - expression simplification in `src/helpers/expr.rs`
    - instruction parsing in `src/helpers/instruction_parse.rs`
    - naming helpers in `src/helpers/naming.rs`
    - lift-state and branch-condition helpers in `src/helpers/state_and_flow.rs`
    - regression test entry in `src/tests.rs`
    - test groups in `src/tests/shared.rs`, `src/tests/emit_and_helpers.rs`, `src/tests/cfg_and_stack.rs`, `src/tests/compaction_and_aliasing.rs`, and `src/tests/golden_and_parser.rs`
    - emit/helper test details in `src/tests/emit_and_helpers/helper_inlining.rs` and `src/tests/emit_and_helpers/readability_and_naming.rs`
    - CFG/stack test details in `src/tests/cfg_and_stack/call_and_loops.rs` and `src/tests/cfg_and_stack/omitted_path_and_stack.rs`
    - compaction test details in `src/tests/compaction_and_aliasing/control_flow_compaction.rs` and `src/tests/compaction_and_aliasing/alias_and_expr_cleanup.rs`
- `crates/flutterdec-core`: orchestration, artifact writing, and quality report logic
  - top-level entry in `src/lib.rs`
  - pipeline utilities in `src/pipeline/helpers.rs`
  - adapter-model loading in `src/pipeline/model.rs`
  - quality gate computation in `src/pipeline/quality.rs`
  - command runners in `src/pipeline/runners.rs`
  - stripped/unstripped ELF call-target mapping in `src/pipeline/symbol_map.rs`
  - ELF fingerprint extraction in `src/pipeline/engine_fingerprint.rs`
- `crates/flutterdec-cli`: user facing commands

## Data contracts

The decompiler expects a normalized model from the adapter layer. That model includes:

- functions and entry addresses
- classes and library metadata when available
- object pool entries
- architecture and snapshot metadata

This keeps the rest of the system independent from any single parser implementation.

## Output philosophy

The target output is pseudo Dart that helps humans understand behavior quickly. It is not intended to compile back into the original program.

Readability wins over low level fidelity when there is a tradeoff. For example:

- preserve branch semantics but hide register noise when possible
- normalize raw tokens into stable placeholders
- simplify noisy arithmetic forms into cleaner constants and offsets when safe
- inline helper fragments where practical and collapse remaining helper scaffolding
- represent very complex unresolved paths as a single summary comment per function plus safe fallbacks
- avoid synthetic "alternative path" branches that duplicate control flow noise
- label indirect call targets with semantic placeholders instead of raw register names
- render stack accesses as indexed slots instead of synthetic field names
- alias key registers to semantic names (for example return address and frame pointer)
- collapse empty `if { } else { ... }` forms into negated `if` blocks
- hoist `else` bodies when the `if` branch terminates, to reduce nested indentation noise
- collapse redundant guarded returns (`if (cond) return x; return x;`) into a single `return x;`
- remove redundant repeated null-guard checks when the first guard already terminates and the checked variable was not reassigned
- fold simple nested guard `if` blocks into combined conditions when the outer block contains only the inner guard
- merge consecutive same-scope `if (...) { continue; }` guards into combined `||` guard conditions
- rewrite adjacent `if (x > K) return ...; if (x >= L) continue;` pairs into explicit bounded continue ranges
- rewrite multi-continue `while (true)` loops into explicit retry-flag loops, then collapse one-shot retry wrappers back to straight-line flow
- collapse nested or trailing guard stacks that always return the same value (for example repeated `return null` guards before a final `return null`)
- extract repeated `(<value> - 1)` expressions into a named alias (`codePoint`) when stable across the function
- normalize negated comparison forms like `!((a) != b)` into direct equality checks
- remove redundant condition wrapping parentheses in emitted `if` statements when the outer wrappers carry no meaning
- surface unknowns explicitly instead of inventing fake certainty

## Quality gates and metrics

The CLI writes `quality.json` and fails the run when strict thresholds are violated. The report tracks:

- disassembly coverage ratio
- unresolved control flow count
- placeholder condition count
- indirect call ratio
- readability regressions such as helper block leakage and raw token leakage
- omitted path marker count for complex regions that are currently summarized
- residual loop back-edge summary marker count for loops that are not yet structured

This makes progress measurable and keeps regressions visible in automation.

## Current scope and limits

Current scope:

- Android ARM64 static pipeline
- adapter backed model ingestion
- IR and pseudo Dart generation with iterative readability passes
- readability passes now prune dead statements after terminal control flow and unwrap non-retry `while (true)` wrappers when the body already terminates
- optional stripped vs unstripped ELF symbol mapping to recover readable direct-call targets
- optional ELF engine fingerprinting to estimate build identity from build-id and marker strings

Known limits:

- no full Dart syntax reconstruction yet
- some difficult control flow still remains as retry-flag loops instead of fully intent-aware Dart loop forms
- very complex control-flow regions can be summarized as omitted-path comments
- many symbols remain synthetic when metadata is obfuscated
- direct source level naming is still heuristic

## Language and maintainability choices

Rust is used for the core pipeline because it gives:

- stronger guarantees around low level data handling
- better long term maintainability for performance critical transforms
- easier test isolation across modules

Python remains useful at the adapter boundary for faster version specific parser updates.

## How to work on this repo

- use `nix develop` for a reproducible toolchain
- run `cargo test` before and after changes
- refresh decompiler golden snapshots with `FLUTTERDEC_UPDATE_GOLDEN=1 cargo test -p flutterdec-decompiler golden_` when output changes intentionally
- for end-to-end real binary regression checks, use `scripts/real-golden.sh record|check` for single profiles, or `scripts/real-golden-matrix.sh check` for multi-profile runs
- keep profile configs in `testdata/real-golden/profiles/*/profile.env`
- for naming improvements on direct call targets, use `decompile --extra-symbol-elf /path/to/libflutter.unstripped.so` when addresses align
- test against real Flutter binaries, not only synthetic fixtures
- prioritize output readability improvements that are backed by concrete sample evidence

## Near term roadmap

- improve retry-loop structuring so remaining retry patterns become clearer intent-level flow
- replace omitted-path comments with richer structured reconstructions
- lift more Dart VM idioms into higher level expressions
- improve naming and type inference from object pool and call patterns
- expand validation corpus across more Flutter and Dart versions
