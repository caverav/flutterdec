# How flutterdec Works

This document explains the internals of `flutterdec` in a practical way, so you can understand the system without reading all Rust code first.

## What the tool does

Input:

- Android APK with Flutter AOT
- `libapp.so` directly

Output:

- pseudo Dart files (`.dartpseudo`)
- optional assembly files (`.s`)
- optional IR files (`.json`)
- quality report (`quality.json`)
- summary report (`report.json`)

Main goal:

- recover readable behavior from Flutter AOT ARM64 binaries using static analysis

## End to end pipeline

```mermaid
flowchart TB
  A[Input APK or libapp.so] --> B[Loader]
  B --> C[SnapshotBundle]
  C --> D[Adapter Runner]
  D --> E[ProgramModel JSON]
  E --> F[ARM64 Disassembler]
  F --> G[FunctionDisassembly]
  G --> H[LLIR and CFG Builder]
  H --> I[FunctionIr]
  I --> J[Pseudocode Emitter]
  J --> K[PseudocodeArtifact]
  K --> L[Quality Engine]
  L --> M[Artifacts on Disk]
```

## Command flow

```mermaid
sequenceDiagram
  participant User
  participant CLI
  participant Core
  participant Loader
  participant Adapter
  participant Disasm
  participant IR
  participant Decompiler

  User->>CLI: flutterdec decompile input -o out
  CLI->>Core: run_decompile(options)
  Core->>Loader: load_snapshot_bundle(input)
  Loader-->>Core: SnapshotBundle
  Core->>Adapter: run_adapter(exec, AdapterInput)
  Adapter-->>Core: ProgramModel
  Core->>Disasm: disassemble_program(model, isolate_instr)
  Disasm-->>Core: FunctionDisassembly[]
  Core->>IR: build_program_ir(disasm)
  IR-->>Core: FunctionIr[]
  Core->>Decompiler: emit_program(ir, symbol_names)
  Decompiler-->>Core: PseudocodeArtifact[]
  Core->>Core: quality_from_artifacts(...)
  Core-->>CLI: QualityReport
  CLI-->>User: quality JSON stdout
```

## Runtime modes

`flutterdec` currently exposes five command families, each with a different internal path:

1. `info`
- fast metadata path
- loader always runs
- the snapshot identity is gated first: a non-FullAOT, scanned-hash, or unsupported-target
  snapshot reports why it was refused and no manifest, path, or adapter is touched
- adapter runs only if the identity cleared that gate and one is installed for the hash
- APK inputs also run Android startup evidence extraction from `classes*.dex` and surface summary counts in JSON output
- no disassembly, no IR, no pseudocode writing

2. `decompile`
- full path from loader to pseudocode and quality gates
- writes artifacts under output directory
- may fail after writing artifacts if quality gates fail
- supports `--target` to narrow output to one function by id or entry VA
- target mode can override scope filtering when the function is not in the selected scope, and records this in `report.json.target_selection.scope_overridden`
- supports analysis-engine profiles: `balanced` (default) and `light`
- profile defaults can be overridden with per-feature `--with-*` / `--no-*` engine flags (canonical model symbols, pool hints, semantic reporting, bootflow category seeding)
- can ingest extra ELF symbol tables and `map-symbols` target JSON to improve direct call naming
- can also auto-ingest locally cached `map-symbols` target summaries by exact `libflutter.so` build-id match when `symbols/manifest.json` is present
- external descriptive names replace generic placeholders like `sub_*` and `fn_0x*` when addresses match
- auto-generated disassembler names such as `FUN_<hex>`, `nullsub_*`, `loc_*`, and `off_*` are also treated as generic placeholders during merge
- indirect `target_va` symbol rewrites apply the same generic-placeholder filter, so tool placeholders are not emitted as callable names
- pool target metadata now also emits deterministic `package_<pkg>_<Owner>_<method>` symbols for `package:*` libraries (not only `dart:*`/`package:flutter/*`)
- when stronger external names are available for the same VA, heuristic canonical names (`dart_*`/`flutter_*`/`package_*`) are replaced automatically
- recognized call names emit semantic intent comments (stdlib/runtime/native/package) next to call lines
- when intent is deterministic, callsites are rewritten to semantic paths and include `was: <original_name>` for traceability
- canonical `package_<pkg>_<Owner>_<method>` symbols are now rewritten to readable `pkg.Owner.method(...)` callsites with `package:<...>` intent tags
- package intent parsing now supports underscore-heavy owner/method tokens so generated call paths stay stable on sanitized class/method names
- framework intent parsing now supports underscore-heavy class/method tokens from sanitized machine names
- canonical Dart direct-call symbols preserve patch-library and owner segments in emitted semantic paths (for example `dart.core_patch.bool_patch.fromEnvironment` and `dart.typed_data.TypedData.offsetInBytes`)
- selector-backed intent can also rewrite indirect callsites and annotate `indirect via: <target_alias>`
- when a call argument is exactly `pool[<idx>]` and a string hint is known, it is rendered as `"value" /* pool[<idx>] */`
- non-exact pool expressions preserve structure and gain inline pool mapping comments (`pool[<idx> /* "value" */]`)
- selector intent coverage includes additional Flutter/Dart standards (for example `Stream.listen`, `Future.catchError`, `SchedulerBinding.addPostFrameCallback`, and `ChangeNotifier.addListener`)
- selector catalog now also covers more standard APIs such as `Navigator.pushNamed` and `List.removeAt`
- constructor-style standard selectors are recognized too (for example `flutter.widgets.KeyedSubtree.new`, `dart.async.StreamIterator.new`, `dart.typed_data.Float32x4List.new`)
- selector catalog now includes more `dart:io` and typed-data APIs (for example `Stdout.supportsAnsiEscapes`, `TypedData.offsetInBytes`, and `ByteData.setFloat32`)
- selector catalog now also includes internal stdlib constructor names such as `dart.io._NativeSocket.new` and `dart.core._CompileTimeError.new`
- selector catalog also includes internal std/core forms like `match_end_index` -> `dart.core.Match.end`
- internal selector names can also be matched directly when they are deterministic, such as `_current` -> `dart.core.Iterator.current` and `_equivalentYear` -> `dart.core.DateTime.equivalentYear`
- internal selector names can also map framework/runtime helpers when deterministic, such as `_listEquals` -> `flutter.foundation.listEquals` and `_prependTypeArguments` -> `dart_vm.prependTypeArguments`
- internal selector names can also map stdlib constructors when deterministic, such as `_StreamController` -> `dart.async.StreamController.new` and `_RawDatagramSocket` -> `dart.io.RawDatagramSocket.new`
- internal typed-data selector names can also map deterministic stdlib paths, such as `_nativeSetFloat32x4` -> `dart.typed_data.ByteData.setFloat32x4`, `_UnmodifiableUint8ArrayView` -> `dart.typed_data._UnmodifiableUint8ArrayView.new`, and `_Int32ArrayView` -> `dart.typed_data._Int32ArrayView.new`
- runtime helper selectors such as `yieldStarIterable` are now rewritten to runtime semantic paths (`runtime:dart_vm.*`)
- VM-internal constructor selectors such as `_Closure` and `_TypeParameter` are rewritten to runtime constructor paths (`dart_vm.*.new`)
- if selector evidence exists but no known standard mapping applies, indirect callsites use readable selector fallback forms: `dispatch.<selector>(...)` for general selectors and `<Selector>.new(...)` for constructor-like selectors (annotated with `heuristic: constructor-like selector`)
- selector evidence for indirect calls is inferred from both call arguments and indirect target expressions
- selector resolution also uses pool metadata to build deterministic owner-qualified semantic paths; in ProgramModel v4 a pool entry carries only `kind`, `value`, and `target_va`, so owner and library come from the function that `target_va` points at, resolved through typed `ClassId`/`LibraryId` edges
- owner-only metadata (a selector plus a resolved owning class whose library is unknown) can still deterministically rewrite to owner-qualified call paths (`owner:Class.method`)
- where the model resolves no owner at all, host-side `ProgramHints` can supply a selector or owner; a hint never overrides a model fact
- if pool metadata carries `target_va` and symbol resolution for that VA is non-generic, indirect callsites can rewrite through that symbol path with `target_va` traceability comments
- selector extraction ignores file/URI/path-like strings to reduce false-positive standard-call rewrites
- unresolved `dispatchTarget` callsites prefer semantic library invoke names when URI evidence exists (for example `flutter.widgets.invoke(...)` or `spotube.models.connect.load.invoke(...)`), otherwise use callable target form `<resolvedTarget>(...)` when the target expression is known, and only then use `dispatch.invoke(...)` fallback to reduce raw `dynamicCall(...)` noise
- noisy dispatch slot target expressions like `reg21.f0` are normalized behind a readable alias (`dispatchTargetFn`) before unresolved callable calls
- unresolved generic indirect aliases (for example `indirectTarget9`) now render as callable fallback `<target>(...)` before resorting to raw `dynamicCall(...)`
- stack-pointer offset arguments are normalized to slot notation (`sp[-0x10]`) so call arguments stay readable
- repeated read-only stack slots can be hoisted into named locals (for example `stackSlotNeg0x10`) to reduce repeated stack-offset noise
- wrapped member-access chains are normalized to cleaner dotted form when safe (for example `((((obj.f7)).f23)).f7` -> `obj.f7.f23.f7`)
- canonical names derived from adapter class/library ownership can deterministically label Flutter framework calls (`framework:flutter.*`), Dart stdlib calls (`stdlib:dart.*`), and package-owned calls (`package:*`)
- argument/local declaration typing uses deterministic context from semantic call ownership, constructor semantics (`*.new`), and literal assignments, allowing concrete types like `flutter.widgets.State`, `dart.async.Future`, `dart.async.StreamIterator`, `String`, and `bool` instead of defaulting to `dynamic`

3. `adapter`
- management path for adapter installation and listing
- installs into the writable adapter store, verified against the compatibility registry
- reports a verified state per record rather than whether a file exists
- does not inspect binaries directly

4. `map-symbols`
- compares a stripped and unstripped ARM64 ELF pair
- extracts function symbols from the unstripped binary
- scans direct call targets in the stripped binary and resolves them to exact/nearest symbols
- writes mapping artifacts under output directory

5. `engine-fingerprint`
- inspects ELF metadata, note sections, and marker strings
- emits build-id and version hints with a confidence score
- optional JSON artifact for reproducible comparisons

## Pipeline walkthrough with concrete contracts

This table shows what each stage consumes and produces.

| Stage | Consumes | Produces | Main code |
|---|---|---|---|
| Loader | APK or ELF bytes | `SnapshotBundle` | `crates/flutterdec-loader/src/lib.rs` |
| Adapter runner | `SnapshotBundle` slices + VAs | `ProgramModel` | `crates/flutterdec-adapter/src/lib.rs` |
| Disassembler | `ProgramModel.functions` + isolate instructions | `FunctionDisassembly[]` | `crates/flutterdec-disasm-arm64/src/lib.rs` |
| IR builder | `FunctionDisassembly[]` | `FunctionIr[]` | `crates/flutterdec-ir/src/lib.rs` |
| Decompiler | `FunctionIr[]` + symbol map | `PseudocodeArtifact[]` | `crates/flutterdec-decompiler/src/lib.rs` |
| Quality/reporting | model + disasm + pseudo + options | `QualityReport` + report files | `crates/flutterdec-core/src/lib.rs` and `crates/flutterdec-core/src/pipeline/*.rs` |

## Decompile lifecycle in pseudo code

This is the effective high-level control flow in `run_decompile`:

bundle = load_snapshot_bundle(input)
key = bundle.identity.exact_selection_key()   // FullAOT/header gate
record = load_registry("adapters/registry.json").select(key)
profile = verify_profile(record.profile, record.profile.sha256)
artifact = verify_host_variant(record.artifact, host_os, host_arch)
model = run_adapter(artifact, bundle, record, profile)
scoped_model = apply_scope_filter(model, function_scope, app_package_filters)
selected_model = apply_target_filter(scoped_model, model, target?)  // optional --target id/va

if selected_model.arch != "arm64":
    fail

disasm = disassemble_program(
    selected_model,
    bundle.isolate_instr,
    bundle.isolate_instr_va,
    target ? none : focus,
    target ? none : max_functions
)
ir = build_program_ir(disasm)
symbols = merge(selected_model.functions names, disasm names)
symbols = merge(extra ELF symbols, with generic-name replacement policy)
symbols = merge(extra symbol-map targets, exact-only unless nearest explicitly enabled)
symbols = normalize external names (demangle + canonical runtime/native/stdlib aliases)
symbols = canonicalize adapter-owned Dart/Flutter standard names from selected_model class/library metadata
pseudo = emit_program(ir, symbols)

write pseudocode files
optionally write asm files
optionally write ir files

quality = quality_from_artifacts(selected_model, disasm, pseudo, options)
write quality.json
write report.json

if !quality.passed:
    fail with path to quality.json
```

Important detail:

- quality evaluation happens after artifacts are generated
- this is intentional so failed runs still leave useful files for debugging

## Repository map

- `crates/flutterdec-cli`: command parsing and command dispatch
- `crates/flutterdec-core`: orchestration, file output, quality gates, stripped/unstripped call-target mapping, and ELF engine fingerprinting
- `crates/flutterdec-loader`: APK and ELF loading, snapshot symbol extraction, and shared APK session caching
- `crates/flutterdec-adapter`: adapter process management and model contract
- `crates/flutterdec-disasm-arm64`: instruction decode and annotations
- `crates/flutterdec-ir`: LLIR construction and CFG recovery
- `crates/flutterdec-decompiler`: structured pseudo Dart emission and readability passes
- `adapters/python/adapter_template.py`: default adapter implementation
- `schemas/program-model-v4.schema.json`: generated JSON Schema for ProgramModel v4

## Data models

### SnapshotBundle

Produced by loader. Contains:

- snapshot bytes (`vm_data`, `isolate_data`)
- instructions bytes (`vm_instr`, `isolate_instr`)
- base virtual addresses for instruction images
- detected snapshot hash
- normalized arch (`arm64`)

Example shape:

```text
SnapshotBundle {
  input_path: ".../app.apk",
  libapp_path: "lib/arm64-v8a/libapp.so",
  arch: "arm64",
  snapshot_hash: "63f9...abcd",
  vm_data: Vec<u8>,
  isolate_data: Vec<u8>,
  vm_instr: Vec<u8>,
  isolate_instr: Vec<u8>,
  vm_instr_va: 0x....
  isolate_instr_va: 0x....
}
```

### ProgramModel v4

Produced by the adapter. v4 is the only accepted contract: `ProgramModel::from_json`
rejects a document carrying `schema_version` as a legacy v2/v3 model, and there is no
migration path. Fields:

- `model_version` (always 4)
- `producer` (id, version, artifact SHA-256, host-assigned trust)
- `input` (the host's snapshot identity plus the region table with digests)
- `compatibility` (record digest, parser family, profile id and digest)
- `capabilities` (per-domain `complete` / `partial` / `unavailable`)
- `libraries[]`, `classes[]`, `functions[]`
- `object_pool` (index space, optional geometry, entries)
- `diagnostics[]`
- `extensions` (the only object that accepts undeclared keys)

Three properties are what separate it from v3:

- **Unknown is representable.** A function name is `Option`, a class's library is
  `Option`, a superclass edge is `Option`. A function whose name was not recovered has
  no name, rather than being called `sub_1234`.
- **Every fact carries provenance.** `exact`, `derived`, or `heuristic`, and only a
  heuristic fact may carry a `confidence`. Per-domain capabilities say how much of a
  domain was recovered, and validation rejects a model whose capability claims
  contradict its contents.
- **The host decides, the adapter reports.** Identity, producer, and compatibility are
  compared against what the host selected. An adapter cannot promote its own trust,
  change which snapshot it was given, or claim a different compatibility record.
- **Identity is a gate, not a label.** A snapshot that cannot produce an exact selection
  key never reaches manifest loading, executable resolution, or a process spawn; it is
  refused with the typed rejection rather than run under a lower trust level. Every
  producer record that exists therefore says `local`.

Minimal example, from a producer that recovered code ranges and nothing else:

```json
{
  "model_version": 4,
  "producer": {"id": "flutterdec-local-python", "version": "unknown", "artifact_sha256": "9f2c...", "trust": "local"},
  "input": {
    "identity": {"hash": "63f9...", "hash_source": "header", "kind": "full_aot", "target_arch": "arm64",
                 "features": {"raw": "product arm64 compressed-pointers", "normalized": ["arm64", "compressed-pointers", "product"]},
                 "pointer_compression": "compressed"},
    "regions": [{"region": "isolate_instructions", "size": 8192, "sha256": "1a2b...", "virtual_address": 6635520, "executable": true}]
  },
  "compatibility": {"record_sha256": "44de...", "parser_family_id": "flutterdec-local-python", "profile_id": "3.9.0", "profile_sha256": "7b10..."},
  "capabilities": {"libraries": "partial", "classes": "unavailable", "class_relationships": "unavailable",
                   "functions": "partial", "function_names": "unavailable", "object_pool": "partial", "pool_index_space": "unavailable"},
  "libraries": [{"id": 0, "uri": "package:app/main.dart", "display_name": null, "provenance": "heuristic"}],
  "classes": [],
  "functions": [{"id": 0, "name": null, "owner": null, "code": {"start_va": 6640668, "size": 320},
                 "code_section_va": 6635520, "provenance": "heuristic"}],
  "object_pool": {"index_space": "ordinal", "geometry": null,
                  "entries": [{"index": 0, "kind": "string", "value": "package:app/main.dart",
                               "target_va": null, "provenance": "heuristic", "confidence": null}]},
  "diagnostics": [{"code": "domain_not_recovered", "severity": "warning", "subject": "function_names",
                   "message": "no function names are recoverable from instruction bytes alone"}],
  "extensions": {}
}
```

### Adapter protocol v1

One adapter run is one process invocation. The host writes the four snapshot regions
into a scratch directory along with a request document, runs the adapter there, and
reads back a result document plus the model.

The request carries the protocol and model majors, the host's identity, the producer
and compatibility records to echo, the requested backend, one relative
`InputHandle` per region (path, size, SHA-256, load address), and the output path.
Snapshot bytes are never embedded: a request for a half-gigabyte snapshot is a few
hundred bytes. There is no session, no JSON-RPC lifecycle, and no persistent worker.

The result carries the same two majors, a structured status (`ok` / `unsupported` /
`failed`), the model path on success, a stable error code on failure, the backend that
actually ran, an optional fallback reason, and diagnostics.

Backends are a closed vocabulary of three tokens - `internal`, `blutter`, `r2flutter` -
and the request's `requested_backend` and the result's `resolved_backend` spell them the
same way, so a producer can answer with the token it was handed. `requested_backend` is
additionally allowed to be `auto`, which is the only case in which a producer may pick a
backend and the only case in which `fallback_reason` may be set: a pinned backend fails
rather than substituting.

### When no adapter is authorized

Selection can end without an adapter to run, and there are five ways it does:
the operator pinned `internal`, the identity gate refused the snapshot, no
compatibility record covers it, a record exists but not for this target or
feature tuple (or two records claim it), or a record authorizes an artifact that
is not installed. None of those is a fact about a broken installation, and none
of them is a reason to stop.

Core recovers what instruction bytes can support: code ranges from AArch64 frame
prologues (`stp x29, x30, [sp, ...]`) and from targets reached by two or more
`bl` sites, each range bounded by the next start and capped at 32 KiB. Every one
is `heuristic` and carries no name and no owner, because a prologue is evidence
of a boundary and of nothing else. `libraries`, `classes`,
`class_relationships`, `function_names`, `object_pool` and `pool_index_space`
stay `unavailable`, each with a diagnostic naming why, so nothing downstream can
mistake a scan for a parse. The model has no compatibility binding at all
(`compatibility: null`): writing one would mean inventing a record digest, a
parser family and a profile that no registry ever selected. It goes through the
same `validate` an adapter model does.

The conditions that stay loud are the ones about the installation rather than
about the snapshot: a malformed registry, a record that fails its own
invariants, a profile that does not verify, an artifact whose bytes are not the
authorized bytes, and any adapter that was authorized, spawned, and then failed.
A pinned external backend is also refused rather than answered, for the same
reason the protocol refuses substitution inside a run.

### Adapter execution containment

An adapter is a third-party executable, so the host treats one run as a bounded,
one-shot job.

Everything is decided before a process exists. Before `run_adapter` spawns anything it
re-derives, from the compatibility record rather than from the caller: the record's own
SHA-256, the protocol and model majors, the snapshot hash, the target architecture, the
canonical feature tuple, the host artifact variant, the profile digest, and the artifact
digest and size. It also requires the executable to be a regular file with an execute
bit, to live inside the adapter store, and to be exactly the path the record names; the
producer record and compatibility binding to follow from the record; and every snapshot
region and the output handle to be usable. Each refusal is a distinct `HostError`
variant, and the ones that mean "no process was created" answer `true` to
`HostError::is_pre_spawn`.

The child that does run gets a private invocation directory (mode `0700`) holding
read-only input handles under `in/`, its output under `out/`, and its own `HOME` and
`TMPDIR`; a cleared environment plus a small allowlist (`PATH`, locale, `XDG_CACHE_HOME`,
and the variables the checked-in producer reads to find an external backend);
`/dev/null` on stdin; its own session and process group; and close-on-exec on every
inherited descriptor above the standard three. The directory is removed on every path,
including timeout and including one an adapter deliberately made unwritable.

`XDG_CACHE_HOME` is the one variable that lets a backend keep something across runs, and
it is passed through rather than set because the choice is the operator's. With it
unset, an external backend that builds itself on first use caches inside the private
workspace and rebuilds on every invocation; with it set, it caches where the operator
already keeps caches. The Blutter bridge is the backend this matters to: its wrapper
keys its source cache on `XDG_CACHE_HOME`, falling back to `$HOME/.cache`.

The host holds an overall wall-clock deadline and caps stdout, stderr, the result
document and the model. On a timeout or a cap breach it signals the whole process group,
waits, and reaps. It signals the group after a clean exit too: a backend that shelled
out and abandoned a grandchild leaves one behind on the clean path as well, and that
grandchild would otherwise hold the host's pipes open. Diagnostics quote a bounded,
escaped tail of child output, never the whole stream.

The child also applies `RLIMIT_CPU`, `RLIMIT_FSIZE`, `RLIMIT_AS`, `RLIMIT_NPROC` and
`RLIMIT_NOFILE`, and on Linux drops into an empty network namespace where the host
permits one.

None of that is claimed unless it was established. The child applies each control
between `fork` and `exec` and writes one fixed-size record of per-control outcomes back
through a close-on-exec pipe; the host turns that record into a containment report where
each control is either `applied` with its bound or `unavailable` with the reason. The
report appears in `flutterdec info --json` as `adapter_containment` and in
`report.json` under `adapter_selection.containment`.

Platform differences are stated rather than smoothed over. Darwin does not enforce
`RLIMIT_AS`, offers no network namespace, and gives no cheap way to observe the per-user
task count, so all three are reported `unavailable` there instead of being set and
assumed. `RLIMIT_NPROC` counts every task of the real user id, so the budget on Linux is
the host's current task count plus an allowance; when the child gets its own user
namespace the count restarts there, and the budget becomes the allowance alone.

### FunctionDisassembly

Produced by disassembler. Per function:

- function metadata (`id`, `entry_va`, `size`, and optional `name`/`owner_class`).
  `name` is `None` when the model recovered none; the printed label is then derived from
  the entry address (`fn_0x<va>`), so an address-derived string can never be read back as
  a recovered name
- decoded instruction list (`AsmInstruction[]`)
- per instruction annotation (`call`, `branch`, `return`, `pool[<index>]`, `poolOff[<displacement>]`, empty)

Example instruction:

```json
{
  "va": 6640704,
  "word": 2545131072,
  "mnemonic": "bl",
  "op_str": "#0xea32e8",
  "annotation": "call"
}
```

### FunctionIr

Produced by IR builder. Contains:

- basic blocks
- block leader addresses
- block successors and predecessors
- LLIR instruction categories (`Call`, `Branch`, `Jump`, `Return`, `LoadPool`, `Other`)

Design note:

- IR is intentionally low-risk and predictable
- no aggressive graph rewriting at this stage
- most readability work is deferred to the decompiler

### PseudocodeArtifact

Produced by decompiler. Contains:

- full pseudo source text
- per function counters for control flow and call quality

The counters let the quality engine detect regressions without parsing ASTs.

## Stage by stage internals

## 1) Loader

Main tasks:

- if input is APK, locate `libapp.so` in `lib/arm64-v8a/` or fallback paths
- parse ELF symbols
- locate Flutter snapshot symbols:
  - `_kDartVmSnapshotData`
  - `_kDartIsolateSnapshotData`
  - `_kDartVmSnapshotInstructions`
  - `_kDartIsolateSnapshotInstructions`
- convert VA to file offsets
- extract byte ranges
- detect snapshot hash from snapshot headers

Key implementation details:

- APK mode now creates one shared loader session per run, so manifest reads, `classes*.dex` startup scans, `libapp.so` extraction, and `libflutter.so` engine fingerprint lookup reuse one indexed ZIP view instead of reopening the APK for each stage
- APK mode scans preferred lib paths first, then fallback file names
- ELF symbol lookup merges dynamic and static symbol tables
- symbol VA is converted to file offset through program headers
- symbol spans are bounds checked against file size before slicing

Key functions:

- `load_snapshot_bundle`
- `find_libapp_in_apk`
- `collect_symbols`
- `read_symbol_span`
- `detect_snapshot_hash`

Failure modes:

- missing `libapp.so`
- unsupported machine type
- missing or zero-sized symbols

## 2) Adapter runner

The adapter is process based. Core writes input binaries to temp files, runs adapter executable, and reads `model.json`.

The current Python template adapter:

- extracts strings from snapshot data
- guesses libraries from `package:...dart` strings
- recovers function starts with simple ARM64 heuristics
- builds object pool from extracted strings
- can run in `auto|internal|blutter|r2flutter` backend mode (`auto` tries r2flutter, then Blutter, then internal)
- in Blutter mode, parses `asm/*.dart` and `pp.txt`; a declaration it cannot parse yields a function with no name, not a synthesized one
- in r2flutter mode, shells out to `r2flutter -jH/-ji/-jc/-jxz/-jzz/-jp` and maps the AOT instruction table, class table, and ObjectPool-referenced strings onto the model
- emits ProgramModel v4 and an adapter protocol v1 result

No backend invents a library, class, function name, or pool index it did not recover.
When a domain comes back empty it is reported `unavailable` with a diagnostic saying
why, which is what lets a reader tell "this snapshot has no such thing" apart from
"this backend cannot see it".

### Pool index space

`object_pool.index_space` says what an entry's `index` counts. `hardware` means a real
`ObjectPool` entry index, the value a `ldr xN, [x27, #disp]` resolves to; it requires
`geometry` (`entries_offset`, `word_size`), and core converts displacements with
`index = (disp - entries_offset) / word_size`. `ordinal` means a position in the
producer's own list, which carries no address meaning at all.

The internal backend is ordinal: its entries are carved strings numbered by carve
order, which has nothing to do with the hardware index space. Core reads the declared
index space and skips pool value and semantic hints entirely rather than joining two
unrelated index spaces, which would attach real-looking strings to the wrong slots.
Validation makes the two consistent by construction: `hardware` without geometry is
rejected, and `ordinal` with geometry is rejected. `report.json.pool_metadata` records
`index_space_authoritative`, the geometry, and `hints_suppressed_reason`.

Validation in Rust enforces, before any model reaches core analysis:

- `model_version` is 4; a `schema_version` field is a legacy model and is rejected
- identity, producer, compatibility, and the region table match what the host selected
- no duplicate ids or pool indexes, no dangling references, no superclass cycles
- canonical ascending order for every collection, so equal models serialize equal
- no placeholder strings standing in for unrecovered names
- confidence only on heuristic facts, and only within `[0, 1]`
- capability claims that match the model's contents, and a diagnostic for every
  unavailable domain
- checked arithmetic on every address: no overflowing or empty code range, every range
  inside a declared executable region, every pool target inside one too

Execution model:

- core writes the four snapshot regions and a protocol v1 request into a scratch dir
- the adapter runs there as a child process, given only relative paths
- the adapter writes the model to the requested output path and a result document
- Rust parses the result, checks it answers the request, then parses and validates
  the model against the host's own view before returning it

Why process-based adapters:

- keeps parser logic isolated from core binary
- allows faster adapter iteration in Python
- makes version-specific parser replacement simple

Key functions:

- `CompatibilityRegistry::select`
- `RegistrySelection::load_profile`
- `RegistrySelection::resolve_current_artifact`
- `run_adapter`
- `validate::validate`

## 3) Disassembler

Uses Capstone ARM64 mode.

Important behavior:

- emits best effort disassembly; if decode fails, uses raw 4-byte words
- tags instruction classes:
  - direct or indirect call
  - jump
  - conditional branch
  - return
- detects pool loads and annotates them with the resolved entry, `pool[index]`
  - direct form: `ldr xN, [x27, #disp]`
  - page form: `add xD, x27, #K, lsl #S` followed by `ldr xN, [xD, #off]`, which Dart
    emits whenever the displacement exceeds the load-immediate range and which is the
    majority of pool traffic in real binaries
  - page bases are tracked per function and dropped on any write to the register or on
    control flow, so a stale base can never invent a slot
  - with an ordinal index space the annotation is `poolOff[<byte displacement>]`
    instead, which is honest about what is known and never matches a value hint

Filtering behavior:

- `focus_prefix` can restrict functions by name or owner class prefix
- `max_functions` bounds output volume for fast sampling runs

Fallback behavior:

- if Capstone is unavailable or decode fails, function bytes are emitted as 4-byte words
- pipeline continues instead of crashing

Key functions:

- `disassemble_program`
- `decode_function`
- `annotation_for`

## 4) IR and CFG

IR builder is intentionally simple and deterministic:

- classify each asm instruction into LLIR op kind
- detect block leaders:
  - function entry
  - branch and jump targets
  - fallthrough after branch
- build blocks by VA ranges
- derive CFG edges from terminator behavior

This keeps CFG generation stable even with partial instruction quality.

Leader and edge logic:

- branch target becomes leader
- branch fallthrough becomes leader
- jump target becomes leader
- return has no outgoing edge
- non-terminator block falls through to next block

Key functions:

- `build_function_ir`
- `build_program_ir`
- `llir_from_disasm`

## 5) Decompiler and readability pipeline

The decompiler has two layers:

1. lifting while walking blocks:
- register value tracking
- comparison tracking for condition reconstruction
- basic memory and local handling
- call emission for direct and indirect targets

2. post processing passes:
- helper inlining and helper collapse
- loop header wrapping into preliminary `while (true)` scaffolds
- unwrapping synthetic single-iteration `while (true)` wrappers with no `continue`
- hoisting `else` blocks after terminating `if` branches
- collapsing redundant guarded returns with identical fallthrough returns
- collapsing nested/trailing guarded-return stacks that always end in the same return value
- removing redundant repeated null guards after terminating null checks
- merging nested single-guard `if` blocks
- merging consecutive `continue` guards into combined conditions
- rewriting return/continue comparator pairs into bounded continue ranges
- rewriting multi-continue infinite loops into retry-flag loops
- unwrapping retry loops that no longer have retry paths
- arithmetic simplification
- normalizing negated comparisons (`!((a) != b)` -> `((a) == b)`)
- removing redundant outer parentheses in `if` conditions after expression rewrites
- naming and type hinting
- extracting stable repeated `(<value> - 1)` expressions into aliases (for example `codePoint`)
- compacting empty or redundant control flow patterns

Important pass ordering:

1. emit base pseudocode from block walk
2. append helper bodies for omitted blocks
3. inline trivial helpers
4. collapse remaining helper scaffolding
5. insert loop back-edge summaries
6. compact empty or redundant patterns (iterative, up to 16 passes)
7. clean expressions
8. apply naming and type hints
9. extract repeated stable arithmetic aliases

Why this ordering matters:

- helper and loop summaries must run before compaction
- naming should run near the end so it sees near-final text
- expression cleanup should run before naming aliases to avoid odd token interactions

Recent readability features include:

- semantic indirect targets:
  - `dispatchTarget`, `cachedTarget`, `indirectTargetN`
- semantic register aliases:
  - `returnAddress`, `framePointer`
- stack slot notation:
  - `sp[-8]`, `sp[8]`
  - stack-derived base expressions collapse to indexed slots (`((sp - 0x30)).f0` to `sp[-0x30]`)
- arithmetic simplification:
  - `(null + 0x20)` to `0x20`
  - `((sp - 0x20) + 0x10)` to `(sp - 0x10)`
- empty branch folding:
  - `if (cond) { } else { body }` to `if (!(cond)) { body }`
- identical branch return folding:
  - `if (cond) { return x; } else { return x; }` to `return x;`
- redundant guarded return folding:
  - `if (cond) { return x; } return x;` to `return x;`
- nested/trailing same-return guard folding:
  - `if (g1) { return null; } if (g2) { return null; } return null;` to `return null;`
- terminating-branch else hoisting:
  - `if (cond) { return x; } else { body }` to `if (cond) { return x; } body`
- redundant null-check elimination:
  - `if (v == null) { return x; } ... if (v == null) { continue; }` removes the second check when `v` was not reassigned
- nested guard merge:
  - `if (a) { if (b) { body } }` to `if ((a) && (b)) { body }`
- continue-guard merge:
  - `if (c1) { continue; } if (c2) { continue; }` to `if ((c1) || (c2)) { continue; }`
- comparator range compaction:
  - `if (x > K) { return r; } if (x >= L) { continue; }` to bounded continue range plus upper-tail return
- retry-loop rewrite:
  - `while (true)` loops with many `continue` edges become `while (retryLoopN)` using a retry flag initialized to true
  - dead fall-through updates that appear after a guaranteed `return` are pruned as unreachable
  - retry wrappers with no remaining retry paths are unwrapped back to straight-line code
- terminal tail pruning:
  - statements after `return`, `continue`, or `break` in the same block are removed until block close
- negated comparison normalization:
  - `!((a) != b)` to `((a) == b)` and `!((a) == b)` to `((a) != b)`
- condition wrapper cleanup:
  - `if (((x == y))) {` to `if (x == y) {`
- repeated minus-one alias extraction:
  - repeated `(value3 - 1)` style expressions become a named alias such as `final int codePoint = (value3 - 1);`
- early loop structuring:
  - detect loop headers from backward CFG edges
  - emit `while (true)` with `continue` for back-edge paths
- loop wrapper cleanup:
  - remove `while (true)` wrappers that have no `continue` and end with a plain `break`
  - remove `while (true)` wrappers with no `continue` when the body already terminates at top level

Fallback policy for complex unresolved regions:

- summarize omitted paths once per function
- return safe fallback values where needed
- do not emit fake structure that implies false confidence

Key functions:

- `emit_pseudocode`
- `emit_block`
- `emit_call`
- `compact_lines`
- `apply_name_and_type_hints`

## 6) Quality engine

`quality.json` is computed from generated pseudocode and disassembly coverage.

Current metrics include:

- `disassembly_ratio`
- `placeholder_ifs`
- `unresolved_cf`
- `indirect_call_ratio`
- `semantic_direct_calls`
- `semantic_indirect_calls`
- `dispatch_selector_calls`
- `target_va_symbol_calls`
- `block_helper_refs`
- `raw_arg_name_refs`
- `raw_register_name_refs`
- `placeholder_cond_markers`
- `omitted_path_markers`
- `loop_backedge_markers`

The run fails when thresholds are violated.

`report.json` also includes a `semantic_rewrite` aggregate section with direct/indirect/fallback/target-va-symbol counts and ratios.
`report.json` also includes a `semantic_intent` aggregate section with framework/stdlib/runtime/native counts, selector-tagged count, and constructor-call count.
`report.json` also includes `selector_fallback` diagnostics with total/unique unresolved selector fallback counts, top selector names, and sample call lines.
`report.json` also includes `call_fallback` diagnostics for `dynamicCall(...)`, `dispatch.invoke(...)`, `dispatchTarget` non-dispatch fallback call forms (`<target>(...)` and semantic library `*.invoke(...)`), and generic `indirectTargetN(...)` fallback forms.
`report.json` also includes `bootflow_discovery` diagnostics with deterministic categorized candidates (`main`, `runapp`, `deeplink`, `activity`, `bootstrap`) sourced from adapter metadata and selector evidence.
Activity/bootstrap candidate synthesis is context-aware (owner/library gating), and overlapping category hits with the same target/selector are deduplicated.
`report.json` also includes `android_manifest` diagnostics (APK inputs): parse mode (`binary_axml` or heuristic fallback), per-signal confidence (`high`/`medium`/`low`), launcher/deeplink flags, manifest activity candidates, deeplink entries, parse errors, and the number of manifest-derived synthetic bootflow hints injected into the model.

Metric formulas:

- `disassembly_ratio = disassembled_function_count / function_count`
- `indirect_call_ratio = indirect_calls / total_calls`

Gate mapping to CLI options:

- `--max-placeholder-ifs` -> max allowed `placeholder_ifs`
- `--max-unresolved-cf` -> max allowed `unresolved_cf`
- `--max-indirect-call-ratio` -> max allowed `indirect_call_ratio`
- `--min-disassembly-ratio` -> min allowed `disassembly_ratio`

Default strict profile comes from CLI defaults in `DecompileCmd`.

Interpretation guidance:

- high `omitted_path_markers` usually means structuring depth limits were hit
- high `loop_backedge_markers` means loop recovery still needs work for unstructured cases
- high `raw_register_name_refs` means naming pass regressed

## Output layout

`decompile -o out_dir` writes:

- `out_dir/pseudocode/*.dartpseudo`
- `out_dir/asm/*.s` (when `--emit-asm`)
- asm lines include raw opcode words when `--emit-asm-opcodes` is set
- `out_dir/ghidra_apply_symbols.py` (when `--emit-ghidra-script`)
- `out_dir/ida_apply_symbols.py` (when `--emit-ida-script`)
- `out_dir/ir/*.json` (when `--emit-ir`)
- `out_dir/quality.json`
- `out_dir/report.json`

File naming convention:

- pseudocode: `{function_id:05}_{sanitized_name}.dartpseudo`
- asm: `{function_id:05}_{sanitized_name}.s`
- ir: `{function_id:05}_{sanitized_name}.json`

`report.json` includes:

- input metadata
- counts for libraries, classes, functions, pool entries
- `model.function_name_provenance` (exact/derived/heuristic/unnamed)
- `adapter_selection` trace (requested backend, resolved backend, adapter exec, manifest mapping, snapshot hash match, strict hash-match enforcement flag, and the `containment` report naming every execution control as applied or unavailable)
- `compatibility` summary (adapter schema support, manifest-entry presence, snapshot hash alignment, and warning list)
- embedded `quality` object
- `name_resolution` aggregate (final name-quality mix and merge replacement diagnostics)
- `pool_value_hints`, `pool_semantic_hints`, and `pool_target_symbols` counts
- `semantic_rewrite` aggregate
- `semantic_intent` aggregate
- `selector_fallback` aggregate
- `call_fallback` aggregate
- `ghidra_script` output metadata (`enabled`, path, symbol count, and pool-comment count)
- `ida_script` output metadata (`enabled`, path, symbol count, and pool-comment count)
- `engine_fingerprint_context` (best-effort `libflutter.so` fingerprint metadata and errors when unavailable)
- `bootflow_discovery` aggregate (source-tagged entries from adapter, manifest, and APK-startup evidence; startup-derived entries can be targetless when no Dart VA is available yet)
- `android_manifest` aggregate
- `android_startup` aggregate (APK bytecode startup evidence from `classes*.dex`, including embedding calls, JNI/bootstrap stages, parse errors, and recovered `DartEntrypoint` callsites when present)
- recovered `android_startup.dart_entrypoints` records can include literal `function_name`, `library_uri`, and `app_bundle_path` values when constructor/execute arguments are directly backed by `const-string`, simple register propagation, or simple app-helper return propagation
- `android_startup.bootstrap_chain` aggregate (best observed startup-chain completeness plus per-source ordered stages, owner kind, missing-step diagnostics, and correlated `paths` when APK bytecode exposes app-defined method edges between activity/engine/bootstrap code; each path also records whether it was anchored to a manifest launcher activity, deeplink activity, application class, FlutterActivity subclass, or only a heuristic/stage terminal)
- `function_scope.priority_package_hints` (effective package boosts used by capped prioritization)
- `prioritization.selected_package_counts_top` and `prioritization.selected_unknown_library_count` (selected top-N ownership mix)
- `prioritization.selected_scope_mix` and `prioritization.selected_app_like_ratio` (selected top-N scope quality)
- `prioritization.selected_preferred_app_count`, `selected_other_app_count`, and `selected_preferred_app_ratio` (selected app-package precision against preferred hints)
- `prioritization.selected_component_totals_top` (which score components dominated capped selection)
- capped prioritization now applies extra context scoring to startup-frontier functions so app-owned bootstrap-adjacent code outranks framework/bootstrap noise when both are visible
- `prioritization.selected_bootflow_coverage` (how much discovered main/runApp/deeplink/activity/bootstrap bootflow is represented in selected top-N)
- `prioritization.selected_bootflow_hits_top` (top selected functions that matched bootflow targets)

## How `info` differs from `decompile`

`info`:

- always runs loader
- checks whether adapter is installed
- optionally runs adapter to populate counts
- does not create output directories

`decompile`:

- runs full pipeline
- always writes output artifacts
- enforces quality gates at end

## Mental model for extension

If you want to improve results, treat each stage independently:

1. loader correctness
2. adapter model fidelity
3. disassembler annotation quality
4. CFG quality
5. pseudocode readability
6. quality gates and thresholds

This split is intentional. It lets you improve one stage without destabilizing the whole system.

## Adding support for a new snapshot hash

1. add a compatibility record for the hash in `adapters/registry.json`: header identity, target,
   canonical feature tuple and fingerprint, profile path and digest, and one artifact variant per
   supported host with the artifact's size and SHA-256
2. ensure the producer speaks protocol major 1 and emits ProgramModel v4
3. run:

```bash
flutterdec adapter install --dart-hash <hash>
flutterdec adapter list
flutterdec decompile app.apk -o out
```

4. inspect:

- `out/quality.json`
- `out/pseudocode/*`

5. iterate on adapter and decompiler passes

## Debugging checklist

If output quality is poor:

1. confirm loader found correct symbols and instruction bases
2. verify adapter returned realistic function starts and sizes
3. compare disassembly output with expected control flow
4. inspect IR blocks and successors for bad splits
5. inspect pseudocode counters in quality report
6. add focused regression tests in decompiler or IR crate

## Debugging by symptom

Symptom: `info` reports `adapter installed: false`

- run `flutterdec adapter install --dart-hash <hash>`
- verify with `flutterdec adapter list`; the row must read `state=verified`
- `state=missing` or `state=corrupt` means the store holds an install it cannot back, and
  `adapter list` exits 2; reinstall
- `state=incompatible` means no artifact variant in the record serves this host

Symptom: `no packaged data directory holds adapters/registry.json`

- the binary is not in a prefix that carries `share/flutterdec`; set `FLUTTERDEC_DATA_DIR`

Symptom: quality gate fails with low disassembly ratio

- increase sample size with `--max-functions`
- check adapter function boundaries
- inspect emitted asm for many fallback `word` instructions

Symptom: capped `--max-functions` output is dominated by runtime helper paths

- keep `--function-scope app-unknown` (default) or tighten with `--function-scope app`
- inspect `report.json` -> `prioritization.selected[*]` for ranking reasons (`components`) and package/library ownership (`library_uri`)
- if needed, set explicit `--app-package <name>`; otherwise decompile derives package hints from `AndroidManifest.xml` when available (including normalized suffix variants such as `_app`/`_flutter`)
- when package hints are present, non-preferred third-party packages are also downranked, so app package functions should dominate capped selections
- note: scorer downranks explicit `no isolate` markers and `dart:isolate*` library paths by default
- capped mode also seeds one function per discovered bootflow category (`main`, `runapp`, `deeplink`, `activity`, `bootstrap`) before normal diversity selection

Symptom: pseudocode has too many omitted path summaries

- inspect large CFG functions in IR output (`--emit-ir`)
- tune decompiler visit limits or helper inlining logic

Symptom: function names are too synthetic

- improve adapter metadata extraction first
- then run `map-symbols` on stripped/unstripped pairs and either feed `symbol_target_summary.json` into `decompile --extra-symbol-map-target ...` or register it into the local cache with `--register-local-cache`

Symptom: control flow is valid but hard to read

- focus on compaction and naming passes in decompiler
- add regression tests with small synthetic CFGs before real APK testing

## Testing strategy

Current project testing style:

- focused unit tests in each crate
- many behavior tests in `flutterdec-decompiler`
- golden snapshot tests in `crates/flutterdec-decompiler/testdata/golden/` for readability-sensitive output
- optional real-binary golden checks via `scripts/real-golden.sh` (single profile) or `scripts/real-golden-matrix.sh` (multi profile) with baselines in `testdata/real-golden/`
- real-binary baselines now also compare `report_metrics.json`, which extracts startup, bootflow, entrypoint, and engine-symbol-ingestion metrics from `report.json`
- regular real-binary smoke validation for output quality

Recommended loop when changing internals:

1. add or update unit test for the exact behavior
2. run crate-local tests
3. run workspace tests
4. run real APK sample with relaxed thresholds
5. compare `quality.json`, `report_metrics.json`, and representative pseudocode files

When readability output changes intentionally, refresh golden snapshots:

```bash
FLUTTERDEC_UPDATE_GOLDEN=1 cargo test -p flutterdec-decompiler golden_
```

For end-to-end real-binary checks against a recorded baseline:

```bash
scripts/real-golden.sh check --input /path/to/sample.apk --baseline testdata/real-golden/profiles/sample --max-functions 120 --min-disassembly-ratio 0.0
```

For multi-profile checks driven by `profile.env` files:

```bash
scripts/real-golden-matrix.sh check
scripts/real-golden-matrix.sh check --strict
```

## Known limits

- ARM64 only
- no full Dart syntax reconstruction yet
- some complex loops are summarized, not fully structured
- some complex branches are summarized as omitted paths
- naming is heuristic when metadata is obfuscated

## Suggested reading order

If you still want to inspect code after this guide:

1. `crates/flutterdec-core/src/lib.rs` and `crates/flutterdec-core/src/pipeline/*.rs`
2. `crates/flutterdec-loader/src/lib.rs`
3. `crates/flutterdec-adapter/src/lib.rs`
4. `crates/flutterdec-disasm-arm64/src/lib.rs`
5. `crates/flutterdec-ir/src/lib.rs`
6. `crates/flutterdec-decompiler/src/lib.rs`

Recommended newcomer route:

1. read this file once top to bottom
2. run one `info` command and one `decompile` command
3. inspect one function across asm, ir, and pseudocode outputs
4. then open the code files in the reading order above

Decompiler source layout:

- `crates/flutterdec-decompiler/src/lib.rs`: top-level orchestration and artifact assembly
- `crates/flutterdec-decompiler/src/control_flow.rs`: CFG flow entrypoint
- `crates/flutterdec-decompiler/src/control_flow/expression_lift.rs`: instruction lifting and operand reconstruction
- `crates/flutterdec-decompiler/src/control_flow/graph.rs`: branch conditions, target resolution, and loop-header wrapping decisions
- `crates/flutterdec-decompiler/src/control_flow/emit.rs`: call emission and recursive block emission
- `crates/flutterdec-decompiler/src/passes.rs`: readability pass entrypoint
- `crates/flutterdec-decompiler/src/passes/compaction.rs`: loop/guard compaction and structural simplification
- `crates/flutterdec-decompiler/src/passes/structural_helpers.rs`: structural helper entrypoint
- `crates/flutterdec-decompiler/src/passes/structural_helpers/block_and_conditions.rs`: block parsing and branch condition helpers
- `crates/flutterdec-decompiler/src/passes/structural_helpers/guard_and_flow.rs`: guarded-return, null-check, and top-level flow helpers
- `crates/flutterdec-decompiler/src/passes/structural_helpers/naming_support.rs`: identifier replacement/stats and alias-support helpers
- `crates/flutterdec-decompiler/src/passes/naming.rs`: variable naming, alias extraction, and type hints
- `crates/flutterdec-decompiler/src/passes/expr_cleanup.rs`: expression cleanup and comparison rewrites
- `crates/flutterdec-decompiler/src/helper_flow.rs`: helper-flow entrypoint
- `crates/flutterdec-decompiler/src/helper_flow/parse.rs`: helper parsing and helper-shape detection
- `crates/flutterdec-decompiler/src/helper_flow/inlining.rs`: helper inlining/collapse flow
- `crates/flutterdec-decompiler/src/helper_flow/summary.rs`: loop-summary insertion, visit-limit policy, and helper function materialization
- `crates/flutterdec-decompiler/src/helpers.rs`: helper utility entrypoint
- `crates/flutterdec-decompiler/src/helpers/registers.rs`: register canonicalization and zero-register helpers
- `crates/flutterdec-decompiler/src/helpers/expr.rs`: integer parsing and expression simplification helpers
- `crates/flutterdec-decompiler/src/helpers/instruction_parse.rs`: instruction and operand parsing helpers
- `crates/flutterdec-decompiler/src/helpers/naming.rs`: local and target naming helpers
- `crates/flutterdec-decompiler/src/helpers/state_and_flow.rs`: lift-state initialization, stack-offset collection, and cmp-to-branch condition helpers
- `crates/flutterdec-decompiler/src/tests.rs`: decompiler regression test entrypoint
- `crates/flutterdec-decompiler/src/tests/*.rs`: grouped decompiler regression suites and shared fixtures
