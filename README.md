# flutterdec

`flutterdec` is a Rust-first research CLI for static decompilation of Flutter AOT Android ARM64 binaries.

## Scope

- Android ARM64 only
- Static analysis only
- Adapter-backed snapshot model ingest
- Disassembly -> IR/CFG -> pseudo-Dart output

## Dev environment

```bash
nix develop
cargo test
cargo run -p flutterdec-cli -- info path/to/libapp.so --json
```

## CLI

```bash
flutterdec info <apk|so> [--json]
flutterdec engine-fingerprint <libflutter.so> [--json] [-o out/fingerprint/]
flutterdec decompile <apk|so> -o out/ [--emit-asm] [--emit-ir]
flutterdec map-symbols --stripped <libflutter-stripped.so> --unstripped <libflutter-unstripped.so> -o out/symbol-map/
flutterdec adapter install --dart-hash <hash>
flutterdec adapter list
```

## Usage

You can run the CLI either from source:

```bash
nix develop -c cargo run -p flutterdec-cli -- <command> ...
```

or as an installed binary:

```bash
flutterdec <command> ...
```

### `info`

Arguments:

- `<INPUT>`: path to an APK file or `libapp.so`
- `--json`: print machine-readable JSON output

Examples:

```bash
flutterdec info ./sample.apk
flutterdec info ./libapp.so --json
```

### `decompile`

Arguments:

- `<INPUT>`: path to an APK file or `libapp.so`
- `-o, --out <OUT_DIR>`: output directory (required)
- `--emit-asm`: also write disassembly files
- `--emit-ir`: also write IR JSON files
- `--extra-symbol-elf <PATH>`: load extra ELF function symbols for call naming (repeatable)
- `--extra-symbol-map-targets <PATH>`: load `symbol_target_summary.json` style mappings from `map-symbols` (repeatable)
- `--include-nearest-symbol-map`: also ingest `nearest` symbol matches from symbol-map targets (default is `exact` only)
- `--focus <FOCUS>`: decompile functions matching a filter
- `--max-functions <N>`: limit number of functions to process
- `--max-placeholder-ifs <N>`: quality gate threshold (default `0`)
- `--max-unresolved-cf <N>`: quality gate threshold (default `0`)
- `--max-indirect-call-ratio <R>`: quality gate threshold (default `0.3`)
- `--min-disassembly-ratio <R>`: quality gate threshold (default `0.8`)

Examples:

```bash
flutterdec decompile ./sample.apk -o ./out
flutterdec decompile ./sample.apk -o ./out --emit-asm --emit-ir
flutterdec decompile ./sample.apk -o ./out --max-functions 120 --min-disassembly-ratio 0.0
flutterdec decompile ./sample.apk -o ./out --extra-symbol-elf ./libflutter.unstripped.so
flutterdec decompile ./sample.apk -o ./out --extra-symbol-map-targets ./out/symbol-map/symbol_target_summary.json
```

Output files:

- `out/pseudocode/*.dartpseudo`
- `out/asm/*.s` (when `--emit-asm`)
- `out/ir/*.json` (when `--emit-ir`)
- `out/quality.json`
- `out/report.json`

`quality.json` now also tracks call rewrite counters:
- `semantic_direct_calls`
- `semantic_indirect_calls`
- `dispatch_selector_calls`
- `target_va_symbol_calls`

`report.json` includes a `semantic_rewrite` summary with:
- `total`, `ratio`
- `direct`, `indirect`, `dispatch_fallback`, `target_va_symbol`
- `indirect_ratio`

`report.json` also includes a `semantic_intent` summary with:
- `framework`, `stdlib`, `runtime`, `native`
- `selector_tagged`
- `constructor_calls`

Naming and semantic behavior:

- external symbols are normalized into readable call names (for example runtime/native prefixes)
- descriptive external names can replace generic placeholders (`sub_*`, `fn_0x*`)
- selector-based deterministic inference can tag standard calls from object-pool arguments even when call targets stay generic
- selector inference now also consumes adapter pool metadata (`selector`, `owner_class`, `library_uri`) for deterministic owner-qualified semantic rewrites
- when pool metadata omits selector/owner/library fields, decompile now backfills them from function ownership metadata using `target_va` when possible
- when pool metadata includes a resolvable `target_va`, indirect callsites can also rewrite through that symbol (instead of fallback invoke/dispatch forms)
- high-confidence callsites are rewritten to semantic paths while keeping traceability with `was: <original_name>`
- indirect callsites can also be rewritten when selector evidence is deterministic, with `indirect via: <target_alias>` traceability
- unresolved indirect callsites can fall back to readable selector forms when no standard mapping is known: `dispatch.<selector>(...)` for general selectors, and `<Selector>.new(...)` for constructor-like selectors (annotated with `heuristic: constructor-like selector`)
- unresolved `dispatchTarget` indirect calls now prefer semantic library invoke names when URI evidence exists (for example `flutter.widgets.invoke(...)` or `spotube.models.connect.load.invoke(...)`), and otherwise fall back to `dispatch.invoke(...)`
- unresolved generic indirect aliases now render as `<target>.invoke(...)` (for example `indirectTarget9.invoke(...)`) instead of `dynamicCall(...)`
- selector evidence can be inferred from indirect target expressions too (for example `target: (pool[...]).f7`), not only call arguments
- selector extraction skips file/URI/path-like strings (for example `*.dart` paths) to avoid false-positive rewrites
- exact `pool[<idx>]` call arguments are rendered as `"value" /* pool[<idx>] */` when a string hint is available
- `report.json` includes `pool_semantic_hints` and `pool_target_symbols` alongside `pool_value_hints` to show metadata coverage used by decompile
- argument and local declaration types are inferred from deterministic semantic call ownership and literal assignments (for example `flutter.widgets.State receiver`, `String tmp`, `bool tmp`)
- constructor-like selectors are also mapped when deterministic (for example `flutter.widgets.KeyedSubtree.new`, `dart.async.StreamIterator.new`, `dart.typed_data.Float32x4List.new`)
- recognized calls add intent comments in pseudocode, for example:
  - `// stdlib:dart.core.print`
  - `// stdlib:dart.core.map [selector]`
  - `// stdlib:dart.core.List.removeAt [selector]`
  - `// stdlib:dart.async.Stream.listen [selector]`
  - `// framework:flutter.widgets.State.setState`
  - `// framework:flutter.widgets.Navigator.pushNamed [selector]`
  - `// framework:flutter.widgets.Widget.build [selector]`
  - `// framework:flutter.scheduler.SchedulerBinding.addPostFrameCallback [selector]`
  - `// runtime:dart_vm.invoke`
  - `// native:libc.memcpy`
  - `final t2 = flutter.widgets.Widget.build(...); // framework:flutter.widgets.Widget.build [selector], was: sub_bbb20c`
  - `final t3 = dart.core.map(...); // stdlib:dart.core.map [selector], indirect via: dispatchTarget, target: (pool[40 /* "_offsetInBytes" */]).f7`

### `adapter`

Arguments:

- `install --dart-hash <DART_HASH>`: install adapter metadata for a Dart hash
- `list`: list installed adapters

Examples:

```bash
flutterdec adapter install --dart-hash 4b8f1f
flutterdec adapter list
```

### `engine-fingerprint`

Extract engine-identifying metadata from an ELF (`libflutter.so` or similar) and produce a confidence-based fingerprint.

Arguments:

- `<INPUT>`: path to ELF file
- `-o, --out <OUT_DIR>`: optional output directory for `engine_fingerprint.json`
- `--max-markers <N>`: max marker strings per category (default `24`)
- `--json`: print full JSON report

Examples:

```bash
flutterdec engine-fingerprint ./libflutter.so --json
flutterdec engine-fingerprint ./libflutter.so -o ./out/fingerprint
```

### `map-symbols`

Map direct call targets from a stripped ARM64 ELF to symbols from an unstripped build.

Arguments:

- `--stripped <PATH>`: stripped `libflutter.so` (or other ARM64 ELF)
- `--unstripped <PATH>`: matching unstripped ELF from the same build
- `-o, --out <OUT_DIR>`: output directory (required)
- `--include-branches`: also map direct `b` targets (default maps `bl` calls only)
- `--nearest-max-distance <N>`: max byte distance for nearest-symbol fallback (default `8192`)
- `--require-exec-match`: fail when executable section bytes differ
- `--json`: print summary report as JSON

Examples:

```bash
flutterdec map-symbols \
  --stripped ./libflutter.stripped.so \
  --unstripped ./libflutter.unstripped.so \
  -o ./out/symbol-map

flutterdec map-symbols \
  --stripped ./libflutter.stripped.so \
  --unstripped ./libflutter.unstripped.so \
  -o ./out/symbol-map \
  --require-exec-match \
  --json
```

Output files:

- `out/symbol-map/symbol_map_report.json`
- `out/symbol-map/symbol_target_summary.json`
- `out/symbol-map/symbol_call_sites.tsv`

Combined naming workflow:

```bash
flutterdec engine-fingerprint ./libflutter.unstripped.so --json
flutterdec map-symbols --stripped ./libflutter.stripped.so --unstripped ./libflutter.unstripped.so -o ./out/symbol-map --require-exec-match
flutterdec decompile ./sample.apk -o ./out \
  --extra-symbol-map-targets ./out/symbol-map/symbol_target_summary.json \
  --extra-symbol-elf ./libflutter.unstripped.so
```

## Real Golden Checks (Optional)

For end-to-end readability regression checks on a real binary, use:

```bash
scripts/real-golden.sh record --input /path/to/sample.apk --baseline testdata/real-golden/profiles/sample --max-functions 120 --min-disassembly-ratio 0.0
scripts/real-golden.sh check  --input /path/to/sample.apk --baseline testdata/real-golden/profiles/sample --max-functions 120 --min-disassembly-ratio 0.0
```

Nix app shortcut:

```bash
nix run .#real-golden -- record --input /path/to/sample.apk --baseline testdata/real-golden/profiles/sample --max-functions 120 --min-disassembly-ratio 0.0
nix run .#real-golden -- check  --input /path/to/sample.apk --baseline testdata/real-golden/profiles/sample --max-functions 120 --min-disassembly-ratio 0.0
```

If this is the first baseline record, provide tracked files via `FLUTTERDEC_REAL_GOLDEN_FILES`:

```bash
FLUTTERDEC_REAL_GOLDEN_FILES='pseudocode/00080_sub_65f850.dartpseudo,pseudocode/00081_sub_65f9ac.dartpseudo' \
  scripts/real-golden.sh record --input /path/to/sample.apk --baseline testdata/real-golden/profiles/sample --max-functions 120 --min-disassembly-ratio 0.0
```

Multi-profile runner (uses `testdata/real-golden/profiles/*/profile.env`):

```bash
scripts/real-golden-matrix.sh check
scripts/real-golden-matrix.sh check --profile sample
scripts/real-golden-matrix.sh check --strict
```

Nix app shortcut:

```bash
nix run .#real-golden-matrix -- check
nix run .#real-golden-matrix -- check --strict
```

## Ethics

Use only on binaries you are legally allowed to analyze. This project is for security research and interoperability study.

## Context

Project research context and architecture notes are in [context.md](context.md).

## Internals

Deep internal architecture and pipeline explanation is in [docs/how-it-works.md](docs/how-it-works.md).
