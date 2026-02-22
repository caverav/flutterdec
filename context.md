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
    - selector catalog in `src/helpers/selector_table.rs`
    - selector catalog categories in `src/helpers/selector_table/categories.rs`
    - selector candidate normalization in `src/helpers/selector_table/candidates.rs`
    - selector catalog matching in `src/helpers/selector_table/matching.rs`
    - call-intent entry in `src/helpers/call_intent.rs`
    - call-intent intent mapping in `src/helpers/call_intent/intent.rs`
    - call-intent library-context mapping in `src/helpers/call_intent/library.rs`
    - call-intent selector resolution in `src/helpers/call_intent/selector_resolution.rs`
    - call-intent text extraction helpers in `src/helpers/call_intent/extract.rs`
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
  - command-runner orchestration entry in `src/pipeline/runners.rs`
  - runner reporting helpers in `src/pipeline/runners/reporting.rs`
  - runner symbol and pool naming helpers in `src/pipeline/runners/symbols.rs`
  - runner-focused tests in `src/pipeline/runners/tests.rs`
  - symbol-map entry in `src/pipeline/symbol_map.rs`
  - symbol-map types in `src/pipeline/symbol_map/types.rs`
  - symbol-map run/load path in `src/pipeline/symbol_map/run.rs`
  - symbol-map ELF section and symbol helpers in `src/pipeline/symbol_map/elf.rs`
  - symbol-map call-scan and target-resolution helpers in `src/pipeline/symbol_map/analysis.rs`
  - symbol-map tests in `src/pipeline/symbol_map/tests.rs`
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
- semantic direct-call rewrite count
- semantic indirect-call rewrite count
- dispatch-selector fallback count
- target-va symbol rewrite count
- report-level semantic intent counts (`framework`, `stdlib`, `runtime`, `native`, selector-tagged, constructor calls)
- metadata coverage counters in `report.json` (`pool_value_hints`, `pool_semantic_hints`, `pool_target_symbols`)
- selector fallback diagnostics in `report.json` (`total`, `unique`, top unresolved `selector:` names, and sample call lines)
- call fallback diagnostics in `report.json` (`dynamicCall`, `dispatch.invoke`, `dispatchTarget` non-dispatch fallback calls, and generic `indirectTargetN(...)` fallback counts)
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
- decompile can now ingest `map-symbols` target JSON directly to inject mapped call names into pseudocode
- external symbol names are normalized (including C++ demangle and runtime/native prefixes) before pseudocode emission
- pseudocode call sites now include semantic intent comments for recognized stdlib/runtime/native targets
- when intent is deterministic, callsites are rewritten to semantic paths and keep traceability via `was: <original_name>`
- deterministic selector evidence can also rewrite indirect callsites and records `indirect via: <target_alias>` in comments
- when a call argument is exactly `pool[<idx>]` and a string hint exists, it is rendered as `"value" /* pool[<idx>] */`
- non-exact pool expressions now keep structure and add inline pool mapping comments (for example `pool[40 /* "_offsetInBytes" */]`)
- selector coverage now includes more Flutter and Dart standard methods (for example `Stream.listen`, `Future.catchError`, `SchedulerBinding.addPostFrameCallback`, and ChangeNotifier listener APIs)
- when selector evidence exists but no standard mapping matches, indirect callsites now use readable selector fallback forms: `dispatch.<selector>(...)` for general selectors and `<Selector>.new(...)` for constructor-like selectors (annotated with `heuristic: constructor-like selector`)
- indirect target expressions are now scanned for selector hints too (not only call args), enabling more deterministic rewrites away from `dynamicCall(...)`
- unresolved `dispatchTarget` calls now prefer semantic library invoke names when URI evidence exists (for example `flutter.widgets.invoke(...)` or `spotube.models.connect.load.invoke(...)`); otherwise they fall back to callable target form `<resolvedTarget>(...)` when target expressions are known, and only then to `dispatch.invoke(...)`; unresolved generic aliases render as callable `<target>(...)`, so raw `dynamicCall(...)` only remains for truly unknown target forms
- unresolved generic direct call targets (`sub_*`/`fn_0x*`) can now also rewrite to semantic owner invoke paths when call arguments carry both a library URI marker and an owner-class marker (for example `framework:flutter.widgets.RenderErrorBox.invoke` from `package:flutter/src/widgets/heroes.dart` + `RenderErrorBox.`), still preserving `was: <original>` traceability
- noncanonical indirect targets (for example `xzr`) now also prefer callable fallback form (`xzr(...)`) with traceability comments, further reducing raw `dynamicCall(...)` output noise
- low-level dispatch slot expressions such as `reg21.f0` are now surfaced through a readable alias (`dispatchTargetFn`) before unresolved callable callsites
- selector extraction now ignores likely file/URI/path-like strings (`*.dart`, paths, URLs) to reduce false-positive standard-call labeling
- declaration typing now uses deterministic context: semantic call ownership (`flutter.*`/`dart.*`/owner-qualified package paths), constructor semantics (`*.new`), and literal assignments can upgrade `dynamic` declarations into concrete types (for example `flutter.widgets.State`, `dart.async.Future`, `dart.async.StreamIterator`, `String`, `bool`)
- declaration typing now also infers local return types from deterministic semantic call paths (for example `String`, `bool`, `int`, `double`, `Type`, `dart.async.Future`, `dart.async.StreamSubscription`) to reduce `dynamic` noise on non-constructor standard calls
- declaration typing now also recognizes constructor-like fallback call paths with PascalCase roots (for example `AndroidPermission.new(...)`) so inferred local types stay concrete even when standard library ownership metadata is missing
- declaration typing now also treats pool-mapped literal assignments (`"value" /* pool[...] */`) as concrete `String` locals instead of leaving them as `dynamic`
- declaration typing now also infers `bool` from condition context (`if (x)`, `x && y`, `x == true`) so argument/local declarations keep less `dynamic` noise in control-flow-heavy functions
- repeated pool-mapped selector literals now hoist into local `String` aliases (for example `poolStr42`) so repeated callsites stay compact and readable
- adapter object-pool metadata fields (`decoded_kind`, `selector`, `target_va`, `owner_class`, `library_uri`) are now consumed by decompile for deterministic owner-qualified selector rewrites
- adapter execution now supports backend selection (`auto`, `internal`, `blutter`) so deterministic parser backends can be introduced without changing decompiler core contracts
- default adapter backend mode is `auto`: it attempts Blutter bridge parsing when configured (`FLUTTERDEC_BLUTTER_CMD` or `FLUTTERDEC_BLUTTER_PY`) and falls back to internal parsing for resilience
- Blutter bridge parsing currently normalizes `asm/*.dart` and `pp.txt` output into `ProgramModel` (`libraries`, `classes`, `functions`, and best-effort `object_pool` target metadata)
- owner-only metadata (selector + owner_class without library URI) can still rewrite indirect selector calls to deterministic owner-qualified call paths
- if pool entries miss selector/owner/library metadata, core now backfills semantic hints from function ownership metadata keyed by `target_va`
- when metadata includes `target_va` and that address resolves to a non-generic symbol, indirect calls can be rewritten to the resolved symbol path (with `target_va` traceability in comments)
- model-backed canonical naming now deterministically tags Dart stdlib (`dart:*`), Flutter framework (`package:flutter/*`), and package-owned calls (`package:*`) when adapter metadata includes class/library ownership
- selector coverage now includes additional standard families such as `Navigator.pushNamed` and `List.removeAt`, improving deterministic semantic rewrites on real samples
- selector coverage also includes internal/std selector forms such as `match_end_index` -> `dart.core.Match.end`
- constructor-like standard selectors are now recognized too (for example `KeyedSubtree`, `StreamIterator`, `Float32x4List`, `Int64List`) and rewritten to semantic `.new` paths
- stack-pointer-derived base expressions now collapse into indexed stack slots (for example `sp[-0x30]`) instead of synthetic field forms
- selector resolution now also handles `dart:io` and typed-data style selectors such as `supportsAnsiEscapes`, `offsetInBytes`, and `nativeSetFloat32`
- selector mapping now also recognizes internal stdlib constructors such as `_NativeSocket` and `_CompileTimeError`
- internal selector-only stdlib forms now include `_current` -> `dart.core.Iterator.current` and `_equivalentYear` -> `dart.core.DateTime.equivalentYear`
- internal selector-only mappings now also include framework/runtime helpers such as `_listEquals` -> `flutter.foundation.listEquals` and `_prependTypeArguments` -> `dart_vm.prependTypeArguments`
- internal selector-only stdlib constructor mappings now also include `_StreamController` -> `dart.async.StreamController.new` and `_RawDatagramSocket` -> `dart.io.RawDatagramSocket.new`
- internal typed-data selector mappings now also include `_nativeSetFloat32x4` -> `dart.typed_data.ByteData.setFloat32x4`, `_UnmodifiableUint8ArrayView` -> `dart.typed_data._UnmodifiableUint8ArrayView.new`, and `_Int32ArrayView` -> `dart.typed_data._Int32ArrayView.new`
- selector coverage now also includes additional deterministic Flutter observer/scheduler/navigation APIs (for example `didPushRouteInformation`, `handleCommitBackGesture`, `scheduleWarmUpFrame`, `restorablePushNamed`) plus broader async/core/typed-data standard selectors (for example `scheduleMicrotask`, `runtimeType`, `setInt64`, `getFloat64x2`)
- runtime helper selectors such as `yieldStarIterable` are now tagged and rewritten to readable runtime semantic paths
- VM-internal selector constructors such as `_Closure` and `_TypeParameter` now rewrite to runtime semantic constructor paths
- standalone stack-pointer offset arguments now normalize to slot notation (`sp[-0x10]`) instead of raw arithmetic (`(sp - 0x10)`)
- repeated read-only stack slots now hoist into named locals (for example `stackSlotNeg0x10`) to reduce repeated low-level stack syntax
- noisy wrapped field-access chains are now simplified (`((((obj.f7)).f23)).f7` -> `obj.f7.f23.f7`)
- optional ELF engine fingerprinting to estimate build identity from build-id and marker strings
- decompile now exposes engine-level analysis profiles (`balanced` and `light`) plus per-feature `--with-*`/`--no-*` toggles for canonical model symbols, pool hints, and semantic reporting to trade throughput vs readability
- decompile now defaults to app-focused function scoping (`app-unknown`) so reverse-engineering output prioritizes app/user-defined code; you can switch to `--function-scope app` or `--function-scope all` when needed
- decompile now also supports package-level scoping via repeatable `--app-package <name>`, so researchers can isolate pseudocode to selected app Dart packages and exclude unknown/dependency/framework noise more aggressively
- report output now includes detected app package frequency (`function_scope.app_package_counts_top`) to guide package scoping without guesswork
- `info` output now surfaces top detected app packages too, so package scoping can be selected before a full decompile run
- capped-function disassembly ordering now prioritizes likely high-value targets (entrypoint-like names, `main.dart` app library ownership, and deeplink or activity-intent related signals, including object-pool `target_va` hints)

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
- use `README.md` for user-facing quick usage and command flow, and `docs/development.md` for contributor/development workflows
- CI now runs formatting, clippy, full workspace tests, and a release CLI build on both Linux and Darwin runners for PRs and on `main` pushes (`.github/workflows/ci.yml`)
- CI also lint-checks repository shell scripts via `scripts/lint-shell.sh` to keep automation scripts maintainable
- CI validates Nix project configuration with `nix flake check` before Rust checks
- tag pushes (`v*`) trigger cross-platform release artifact builds and GitHub release publishing in `.github/workflows/release.yml`
- GitHub contribution hygiene is bootstrapped with issue templates, PR template, CODEOWNERS routing, and weekly Dependabot update PRs under `.github/`
- local CI-parity validation is available via `scripts/ci-check.sh` (also exposed as `nix run .#ci-check`)
- refresh decompiler golden snapshots with `FLUTTERDEC_UPDATE_GOLDEN=1 cargo test -p flutterdec-decompiler golden_` when output changes intentionally
- for end-to-end real binary regression checks, use `scripts/real-golden.sh record|check` for single profiles, or `scripts/real-golden-matrix.sh check` for multi-profile runs
- keep profile configs in `testdata/real-golden/profiles/*/profile.env`
- for naming improvements on direct call targets, use `map-symbols` on stripped/unstripped ELF pairs, then pass `decompile --extra-symbol-map-targets /path/to/symbol_target_summary.json`
- `decompile` prefers external descriptive names over generic internal names (`sub_*`, `fn_0x*`) when addresses match
- test against real Flutter binaries, not only synthetic fixtures
- prioritize output readability improvements that are backed by concrete sample evidence

## Near term roadmap

- improve retry-loop structuring so remaining retry patterns become clearer intent-level flow
- replace omitted-path comments with richer structured reconstructions
- lift more Dart VM idioms into higher level expressions
- improve naming and type inference from object pool and call patterns
- expand validation corpus across more Flutter and Dart versions
