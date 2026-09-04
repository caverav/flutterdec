# CLI Reference

This reference describes the CLI at the current commit. `flutterdec --help` is
the quickest overview, `flutterdec <COMMAND> --help` covers a single command,
and `flutterdec --version` reports the build you are running.

## `flutterdec info`

Usage:

```bash
flutterdec info <INPUT> [--json] [--adapter-backend <BACKEND>]
```

Arguments:

- `<INPUT>`: APK or `libapp.so`
- `--json`: print JSON output
- `--adapter-backend <auto|internal|blutter|r2-flutter>` (default `auto`; `auto` tries r2flutter, then blutter, then internal)

Resolved from the snapshot hash alone, with or without an adapter, in both JSON and
plain output:

- `dart_version`
- `dart_tag_style` (`CID_INT32`, `CID_SHIFT1`, or `OBJECT_HEADER`)

Both are null for snapshot hashes outside `data/dart-profiles.json`.

If adapter metadata is available, JSON output also includes app-package hints:

- `app_package_count_total`
- `app_package_counts_top`
- `adapter_kind`
- `manifest_entry_present`
- `adapter_snapshot_hash_match`
- `compatibility_warnings`

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

### Emission diagnostics in `quality.json`

`quality.json.emission` is program-level block-ledger diagnostic accounting for
the functions in that output. Every value is a non-negative integer. Function
counters count functions, event counters count keyed traversal events, and
block counters count final block identities. They are not decompiled instruction
counts.

Structured-emission function counters:

| Field under `quality.json.emission` | Unit | Meaning |
| --- | --- | --- |
| `structured_declines` | functions | Functions for which structured emission declined and the DFS emitter was used. This is the sum of the five primary-cause counters below. |
| `structured_rollbacks` | functions | Declined functions whose structured attempt had already changed emitter state, so that attempt was rolled back before DFS emission. This is the sum of `repeat_budget`, `structured_depth_budget`, and `coverage_mismatch`. |
| `irreducible` | functions | Structured attempts declined before mutation because region analysis rejected the graph as irreducible. |
| `unsupported_region` | functions | Structured attempts declined before mutation because a reachable successor shape had no structured rendering rule. |
| `repeat_budget` | functions | Structured attempts declined after a shared region would exceed the repeat budget or cross a loop header. |
| `structured_depth_budget` | functions | Structured attempts declined after the structured walk reached its nesting-depth budget. |
| `coverage_mismatch` | functions | Structured attempts declined after the walk finished without emitting every reachable block. |

Traversal-event counters:

| Field under `quality.json.emission` | Unit | Meaning |
| --- | --- | --- |
| `dfs_depth_omissions` | traversal events | DFS edges omitted because the walk was already at its depth budget. |
| `dfs_visit_omissions` | traversal events | DFS edges omitted because the target had reached its visit budget. The target block may still have been emitted elsewhere. |
| `helper_cap_omissions` | traversal events | Helper paths omitted because the helper-definition budget was exhausted. |

Final block-ledger counters:

| Field under `quality.json.emission` | Unit | Meaning |
| --- | --- | --- |
| `structured_emitted_blocks` | blocks | Final block identities emitted by the structured walk. |
| `dfs_emitted_blocks` | blocks | Final block identities emitted by the DFS walk. |
| `guard_pruned_blocks` | blocks | Block identities removed by guard pruning. |
| `noreturn_pruned_blocks` | blocks | Block identities removed after a no-return path was identified. |
| `retained_unreachable_blocks` | blocks | Final block identities retained in the valid graph but unreachable from the function entry and therefore not emitted. |
| `reachable_unemitted_blocks` | blocks | Final block identities reachable from the function entry but not emitted. Each has a ledger explanation that links it to a traversal event through valid graph edges. |
| `invalid_cfg_rejected_functions` | functions | Functions whose invalid raw CFG was rejected before it could enter the valid block partition. |

The per-function `block_ledger` in `ir/*.json` tracks immutable block identities
through dense-id stages and remaps. For a valid source graph, each terminal block
identity has exactly one final disposition: structured-emitted, DFS-emitted,
guard-pruned, no-return-pruned, retained-unreachable, or reachable-unemitted.
An invalid graph is reported separately and does not carry a partial valid-graph
partition.

Do not compare `quality.json.emission.reachable_unemitted_blocks` directly with
`quality.json.omitted_path_markers`. The former counts distinct final block
identities with a ledger disposition. The latter counts emitted source lines
containing an `omitted complex path` marker. One marker can summarize paths, and
a traversal event can name a block that was emitted elsewhere, so neither text
markers nor event totals are block counts.

`report.json.record_split.rejected_invalid_ir` counts input function records for
which record splitting was abandoned because the graph built from the record
failed the shared block-identity validation. The record is left unsplit. This is
a record count, not a count of rejected or decompiled instructions.

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
- `--require-snapshot-hash-match` (fail if either side has adapter/loader snapshot hash mismatch)
- `--json`

Output:

- writes `diff_report.json` with function-level deltas and package-level summaries (`added_packages_top`, `removed_packages_top`)

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
- `--register-local-cache` (copy the generated target summary into `symbols/` and register it in `symbols/manifest.json` for later auto-ingestion)
- `--json`

## `flutterdec adapter`

Install:

```bash
flutterdec adapter install --dart-hash <HASH>
```

List:

```bash
flutterdec adapter list
```
