# Research Decisions

- Parser strategy: adapter boundary keeps snapshot version churn isolated.
- Core language: Rust for stronger typing and safer low-level handling.
- Adapter language: Python for fast per-hash parser updates.
- Scope baseline: Android ARM64 AOT static-first, correctness prioritized over breadth.
- Quality gates: strict defaults to prevent low-confidence pseudocode output.
- Pseudocode quality: instruction-stream semantics (Dart ABI idioms, dispatch-table
  selectors, single-emission structuring) are recovered independently of snapshot
  metadata, and joined to it by integer keys rather than reimplemented. See
  `docs/research-pseudocode-quality.md`.
- Naming: a fabricated name is worse than an honest placeholder. Names come from
  metadata or from an encoding that provably identifies the callee; guesses are
  reported as comments, never emitted as identifiers.
- Snapshot metadata: prefer an external snapshot-aware backend at the adapter boundary
  over reimplementing Dart's clustered deserializer in core.
- Version tables: vendor as data, never as code.

## Third-party data: `data/dart-profiles.json`

Maps 61 Dart AOT snapshot hashes to 19 layout profiles (Dart version, object-header
tag style, compressed word size, class-id table). Imported from
[radareorg/r2flutter](https://github.com/radareorg/r2flutter) (MIT), `offsets.json`.

Why vendor rather than derive: the snapshot hash is an MD5 over Dart VM serializer
sources, so the hash-to-version mapping cannot be computed from a binary. It has to be
tabulated by building every SDK release, which is exactly the kind of maintenance work
worth sharing instead of duplicating.

Why data and not code: it costs nothing to keep current, has no build or runtime
dependency, and stays useful no matter which backend parses the snapshot. `flutterdec`
uses it for identification only (`info.dart_version`, `report.json.dart_profile`); it
does not deserialize snapshots with it.

Two facts from that table constrain any future in-tree parser, including a native one:

- there are three object-header tag encodings, not one (`CID_INT32` for Dart 2.10-2.13,
  `CID_SHIFT1` for 2.14-3.3, `OBJECT_HEADER` for 3.4.3+ and the 2.18.2 outlier)
- class ids move between releases, so a `#[repr(u32)]` enum of class ids can only ever
  be correct for one profile; the mapping has to be a runtime table

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
