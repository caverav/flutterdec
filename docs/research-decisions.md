# Research Decisions

- Parser strategy: adapter boundary keeps snapshot version churn isolated.
- Core language: Rust for stronger typing and safer low-level handling.
- Adapter language: Python for fast per-hash parser updates.
- Scope baseline: Android ARM64 AOT static-first, correctness prioritized over breadth.
- Quality gates: strict defaults to prevent low-confidence pseudocode output.

## North Star

Recover readable behavior from Flutter AOT ARM64 binaries with enough semantic structure to support real reverse engineering workflows.

## Primary Goals

- maximize deterministic semantic signal in `ProgramModel` (libraries/classes/functions/object pool metadata)
- keep decompiler output stable and readable across repeated runs and versions
- make version upgrades mostly an adapter/backend update problem, not a core rewrite problem

## Non-Goals

- exact recompilable Dart source reconstruction
- runtime emulation as baseline analysis mode
- immediate parity for all architectures and snapshot/runtime modes

## Decision Rubric

Use this rubric for architecture and feature decisions:

1. does this increase deterministic semantic recovery for Android ARM64 AOT?
2. does this improve stability/reproducibility of pseudocode and report outputs?
3. can this be versioned at adapter/backend boundaries without destabilizing core crates?
4. is the complexity justified by measurable quality/report improvements?
