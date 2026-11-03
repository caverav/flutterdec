# CLI Reference

This reference describes the CLI at the current commit. `flutterdec --help` is
the quickest overview, `flutterdec <COMMAND> --help` covers a single command,
and `flutterdec --version` reports the build you are running.

## `flutterdec info`

Usage:

```bash
flutterdec info <INPUT> [--json] [--adapter-backend <BACKEND>] [--adapter-timeout <SECONDS>]
```

Arguments:

- `<INPUT>`: APK or `libapp.so`
- `--json`: print JSON output
- `--adapter-backend <auto|internal|blutter|r2-flutter>` (default `auto`; `auto` tries r2flutter, then blutter, then internal)
- `--adapter-timeout <SECONDS>`: wall-clock deadline for one adapter invocation

Exit status is nonzero when an adapter was authorized, ran, and failed. The
report is still printed, and `adapter_error_category` names what went wrong.

Resolved only after a FullAOT header identity exactly matches a host registry record
(hash, target, and canonical layout-feature fingerprint). The runtime profile is
loaded and SHA-256 verified from that record:

- `dart_aliases` (SDK labels with provenance; never selectors)
- `dart_tag_style` (`CID_INT32`, `CID_SHIFT1`, or `OBJECT_HEADER`)
- `registry_record_present`

If adapter metadata is available, JSON output also includes app-package hints:

- `app_package_count_total`
- `app_package_counts_top`
- `requested_backend`, `resolved_backend`, `backend_fallback_reason`
- `producer_id`, `producer_trust`, `compatibility_record_sha256`
- `snapshot_identity_is_exact`, `identity_rejection`, `model_capabilities`
- `compatibility_warnings`
- `adapter_containment` (per control: `applied` with its bound, or `unavailable` with the reason)
- `adapter_error`, `adapter_error_category` when an authorized adapter failed
- `provider`: the block described under [Provider reporting](#provider-reporting)

## `flutterdec decompile`

Usage:

```bash
flutterdec decompile <INPUT> -o <OUT_DIR> [OPTIONS]
```

Required:

- `<INPUT>`: APK or `libapp.so`
- `-o, --out <OUT_DIR>`

General options:

- `--emit-asm`
- `--emit-asm-opcodes` (requires `--emit-asm`; prepends raw 32-bit opcode words in `asm/*.s`)
- `--emit-ghidra-script` (writes `ghidra_apply_symbols.py` with function/label symbol application helpers)
- `--emit-ida-script` (writes `ida_apply_symbols.py` with function/label symbol application helpers)
- `--emit-ir`
- `--focus <FOCUS>`
- `--target <TARGET>` (decompile/disassemble a specific function by selector: `id:<N>`, `va:0x<ADDR>`, `0x<ADDR>`, or `<N>`; ambiguous `<N>` matches fail and require explicit prefix)
- `--max-functions <N>`
- `--function-scope <app-unknown|app|all>` (default `app-unknown`)
- `--app-package <NAME>` (repeatable; restricts to selected `package:<NAME>/...` libraries)
- `--split-records` (split a function record that spans more than one real function). The adapter sizes a
  record as the gap to the next start it recovered, so a function it **missed is swallowed by its
  predecessor and never emitted at all**. On the two research samples that hides roughly three quarters of
  the decoded blocks, which is why every figure in `docs/research-pseudocode-quality.md` is measured with
  this on.

  Off by default, and the reasons matter: it multiplies the emitted function count (5,800 and 8,329 declared
  records yield 22,102 and 28,753 emitted functions), which **moves every absolute quality counter**, and it
  makes `--max-functions` and `--function-scope` apply to *records* rather than to what is emitted.
  `disassembly_ratio` deliberately keeps the model's pre-split function list as its denominator, so the
  ratio is not inflated by split pieces. Comparing a split run against an unsplit one compares unlike
  populations.
- `--adapter-backend <auto|internal|blutter|r2-flutter>` (default `auto`; `auto` tries r2flutter, then blutter, then internal)
- `--adapter-timeout <SECONDS>`: wall-clock deadline for one adapter invocation
- `--require-snapshot-hash-match` (fail if adapter-reported snapshot hash differs from loader hash)

Symbol ingestion:
- `--extra-symbol-map-target <PATH>` (repeatable; `--extra-symbol-map-targets` remains accepted as an alias)
- `--extra-symbol-elf <PATH>` (repeatable)
- `--include-nearest-symbol-map`

Quality-gate options:

- `--max-placeholder-ifs <N>` (default `0`)
- `--max-unresolved-cf <N>` (default `0`)
- `--max-indirect-call-ratio <R>` (default `0.30`)
- `--min-disassembly-ratio <R>` (default `0.80`)

Analysis-engine profile:

- `--analysis-profile <light|balanced>` (default `balanced`)

Analysis-engine feature toggles:

- `--with-canonical-model-symbols`
- `--no-canonical-model-symbols`
- `--with-pool-value-hints`
- `--no-pool-value-hints`
- `--with-pool-semantic-hints`
- `--no-pool-semantic-hints`
- `--with-semantic-reporting`
- `--no-semantic-reporting`
- `--with-bootflow-category-seeds`
- `--no-bootflow-category-seeds`
- `--with-apk-startup-analysis`
- `--no-apk-startup-analysis`

Conflict rule:

- each `--with-*` conflicts with its matching `--no-*`

Target selection behavior:

- when `--target` is set, output is narrowed to the matched function
- if scope filters exclude that function, target mode may override scope to keep the explicit match
- selection diagnostics are written to `report.json.target_selection`

Adapter backend environment:

- `FLUTTERDEC_R2FLUTTER_BIN`: path to the `r2flutter` binary
- `FLUTTERDEC_R2FLUTTER_CMD`: full command to execute the r2flutter backend
- `FLUTTERDEC_R2FLUTTER_TIMEOUT`: per-invocation timeout in seconds (default 900)
- `FLUTTERDEC_BLUTTER_CMD`: full command to execute Blutter bridge backend
- `FLUTTERDEC_BLUTTER_PY`: path to `blutter.py` (uses current Python interpreter)

## `flutterdec diff`

Usage:

```bash
flutterdec diff --old <OLD_INPUT> --new <NEW_INPUT> -o <OUT_DIR> [OPTIONS]
```

Required:

- `--old <OLD_INPUT>`: APK or `libapp.so` baseline
- `--new <NEW_INPUT>`: APK or `libapp.so` candidate
- `-o, --out <OUT_DIR>`

Options:

- `--function-scope <app-unknown|app|all>` (default `app-unknown`)
- `--app-package <NAME>` (repeatable; limit compare set to selected app packages)
- `--adapter-backend <auto|internal|blutter|r2-flutter>` (default `auto`)
- `--adapter-timeout <SECONDS>`: wall-clock deadline for one adapter invocation, applied to each side
- `--require-snapshot-hash-match` (fail if either side has adapter/loader snapshot hash mismatch)
- `--json`

Output:

- writes `diff_report.json` with function-level deltas and package-level summaries (`added_packages_top`, `removed_packages_top`)
- `old_provider` and `new_provider`, each the block under [Provider reporting](#provider-reporting).
  The two sides are selected independently, so a run whose sides were not produced the same way sets
  `provider_mismatch`: a name-bearing model and a core-recovered one differ in every descriptor whether
  or not the code changed.
- `old_uncomparable_function_count` and `new_uncomparable_function_count`: functions with no name, owner
  or library. An address alone does not survive a rebuild, so these are counted and excluded from the
  compared sets rather than collapsed into one descriptor that would read as "unchanged".
- a failure on either side names which side it was, and carries that side's error category

## `flutterdec engine-fingerprint`

Usage:

```bash
flutterdec engine-fingerprint <INPUT> [--json] [-o <OUT_DIR>] [--max-markers <N>]
```

Arguments:

- `<INPUT>`: ELF file (usually `libflutter.so`)
- `-o, --out <OUT_DIR>`
- `--max-markers <N>` (default `24`)
- `--json`

## `flutterdec map-symbols`

Usage:

```bash
flutterdec map-symbols --stripped <PATH> --unstripped <PATH> -o <OUT_DIR> [OPTIONS]
```

Arguments:

- `--stripped <PATH>`
- `--unstripped <PATH>`
- `-o, --out <OUT_DIR>`
- `--include-branches`
- `--nearest-max-distance <N>` (default `8192`)
- `--require-exec-match`
- `--register-local-cache` (copy the generated target summary into the resolved symbol cache and register it in its `manifest.json` for later auto-ingestion; see Resolved locations)
- `--json`

## `flutterdec adapter`

Install:

```bash
flutterdec adapter install --dart-hash <HASH> [--target-arch <ARCH>] [--from <PATH>] [--json]
```

- `--dart-hash <HASH>`: 32 lowercase hexadecimal characters, as `info` reports it
- `--target-arch <ARCH>`: required only when one hash has records for more than one target
- `--from <PATH>`: publish this artifact instead of the packaged producer. It must still match the
  digest and size the compatibility record declares
- `--json`: print the installation record as JSON

The compatibility registry is the only install authority: a hash with no record, a record that serves
no artifact variant for this host, a requested target the record does not serve, a profile whose digest
no longer matches, a source that is not a regular file, and any bytes that do not match the record's
declared digest and size are all refused with a nonzero exit and no store write. The published path is
store-relative and contained, so an absolute path, `..`, or a symlinked directory in the chain is
refused rather than followed.

Output names the store path, the artifact and profile digests, the host variant, the target, the
compatibility record digest, the protocol/model majors, and whether the result was idempotent
(`installed` or `already-installed`). Installing the same content twice writes nothing.

List:

```bash
flutterdec adapter list [--json]
```

Reports one row per compatibility record, with a state that is verified rather than inferred from file
existence: `verified`, `missing`, `corrupt`, `incompatible`, or `unavailable`. Exit status is 2 when any
entry is `missing` or `corrupt`, 0 otherwise, and nonzero with a message when the store's own state file
cannot be read.

## Provider reporting

`info`, `decompile` (`report.json`, under `adapter_selection.provider`) and each
side of `diff` (`old_provider`, `new_provider`) carry the same block. It is
built once from host facts and the protocol result, so the three surfaces cannot
describe one run differently.

- `requested_backend`, `resolved_backend`, `backend_mismatch`,
  `backend_fallback_reason`
- `adapter_executed`, `adapter_exec_path`, `containment`
- `core_fallback_reason`, `core_fallback_detail`, `core_fallback_effect`
- `producer_id`, `producer_version`, `producer_artifact_sha256`, `producer_trust`
- `registry_record_present`, `compatibility_record_sha256`, `parser_family_id`,
  `profile_id`, `profile_sha256`, `artifact_id`, `artifact_sha256`
- `host_os`, `host_arch` (the machine that ran) and `target_arch` (the machine
  the snapshot targets), which are separate facts
- `snapshot_identity_is_exact`, `identity_rejection`, `capabilities`, `warnings`

## Core recovery

When nothing is authorized to parse a snapshot, no adapter is executed and core
recovers what it can from the instruction bytes. `core_fallback_reason` says
which of these it was:

| reason | meaning |
| --- | --- |
| `internal_requested` | `--adapter-backend internal`; no registry read, no execution |
| `identity_rejected` | not a FullAOT snapshot, or the hash did not come from a header |
| `no_compatibility_record` | the identity is exact and no record covers it |
| `compatibility_unsupported` | a record exists for the hash but not for this target or feature tuple |
| `adapter_not_installed` | a record authorizes an artifact and none is installed |

What core recovery produces: ARM64 code candidates from frame prologues and
repeatedly-called targets, every one of them `heuristic` and unnamed. What it
does not: libraries, classes, class relationships, function names, the original
entry function, and any ObjectPool index space, all of which stay `unavailable`
with a diagnostic saying why.

A malformed registry, two records claiming one snapshot (`registry_ambiguous`),
a record that fails its own invariants, a profile that does not verify, and an
artifact whose bytes are not the ones the registry authorized are *not* fallback
conditions. They are integrity failures of the
installation and they stop the command. So does an adapter that was authorized,
spawned, and then failed.

`--adapter-backend blutter` and `--adapter-backend r2-flutter` are refused
rather than answered by core recovery: those flags mean "exact names or
nothing", and substituting prologue scanning is what the protocol already
forbids inside a run.

## Error categories

Every failure prints `error category: <token>` on stderr alongside its message.
The message is for a human and may be reworded; the token is stable.

- identity and registry: `identity_rejected`, `registry_no_record`,
  `registry_target_mismatch`, `registry_feature_mismatch`, `registry_ambiguous`,
  `registry_malformed`, `registry_unsupported_version`, `registry_invalid_record`,
  `registry_profile_rejected`, `registry_artifact_absent`,
  `registry_artifact_rejected`
- refused before any child existed: `record_invalid`, `record_digest_mismatch`,
  `unsupported_majors`, `identity_record_mismatch`, `target_mismatch`,
  `feature_mismatch`, `host_variant_mismatch`, `variant_not_in_record`,
  `artifact_path_rejected`, `artifact_not_executable`,
  `artifact_digest_mismatch`, `profile_rejected`, `producer_mismatch`,
  `binding_mismatch`, `input_rejected`, `request_rejected`,
  `output_handle_rejected`, `image_not_sealed`
- a child ran: `spawn_failed`, `workspace_failed`, `adapter_timeout`,
  `adapter_output_limit_exceeded`, `adapter_crashed`, `adapter_no_result`,
  `adapter_document_too_large`, `adapter_malformed_document`,
  `adapter_result_mismatch`, `adapter_model_path_mismatch`,
  `adapter_reported_failure`, `adapter_model_rejected`,
  `containment_unreported`, `adapter_io`
- installing into or reading the adapter store: `store_invalid_input`,
  `store_no_record`, `store_ambiguous`, `store_incompatible`,
  `store_artifact_source_rejected`, `store_artifact_digest_mismatch`,
  `store_path_rejected`, `store_profile_rejected`, `store_state_malformed`,
  `store_io`, `store_install_interrupted`
- resolving where the packaged data and the writable store are, before any
  command runs: `layout_executable_unknown`,
  `layout_data_dir_override_invalid`, `layout_no_data_directory`,
  `layout_no_data_home`
- `unclassified` for anything else

The category does not depend on which command hit the condition. A registry
record that fails validation reports `registry_invalid_record` through both
`adapter list` and `info`, because the typed error is carried up rather than
flattened into a message.

## Resolved locations

Neither directory depends on the current working directory.

- Read-only package data (`adapters/registry.json`, `data/*.json`, the packaged producer):
  `FLUTTERDEC_DATA_DIR`, else `<binary>/../share/flutterdec`, else `<binary>`, else `<binary>/../..`.
  The first candidate that actually holds `adapters/registry.json` wins, and an override that holds
  none is an error rather than a fallback.
- Writable adapter store: `FLUTTERDEC_ADAPTER_STORE`, else `$XDG_DATA_HOME/flutterdec/adapters`, else
  `$HOME/.local/share/flutterdec/adapters`.
- Local symbol cache: `FLUTTERDEC_SYMBOL_CACHE`, else `<data home>/flutterdec/symbols`.

`FLUTTERDEC_INSTALL_FAIL_BEFORE=<lock|stage|publish_artifact|publish_state>` fails an install on purpose
before a named publish step. It exists so the "no partial state" guarantee can be tested.
