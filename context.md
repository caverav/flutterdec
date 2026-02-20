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
- `crates/flutterdec-core`: orchestration, artifact writing, and quality report logic
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
- surface unknowns explicitly instead of inventing fake certainty

## Quality gates and metrics

The CLI writes `quality.json` and fails the run when strict thresholds are violated. The report tracks:

- disassembly coverage ratio
- unresolved control flow count
- placeholder condition count
- indirect call ratio
- readability regressions such as helper block leakage and raw token leakage
- omitted path marker count for complex regions that are currently summarized
- loop back-edge summary marker count for functions that still need loop structuring

This makes progress measurable and keeps regressions visible in automation.

## Current scope and limits

Current scope:

- Android ARM64 static pipeline
- adapter backed model ingestion
- IR and pseudo Dart generation with iterative readability passes

Known limits:

- no full Dart syntax reconstruction yet
- difficult loops can still appear as per-function back-edge summary comments
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
- test against real Flutter binaries, not only synthetic fixtures
- prioritize output readability improvements that are backed by concrete sample evidence

## Near term roadmap

- improve loop structuring so back-edge summaries become real `while` or `for` constructs
- replace omitted-path comments with richer structured reconstructions
- lift more Dart VM idioms into higher level expressions
- improve naming and type inference from object pool and call patterns
- expand validation corpus across more Flutter and Dart versions
