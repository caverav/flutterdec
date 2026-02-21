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

Naming and semantic behavior:

- external symbols are normalized into readable call names (for example runtime/native prefixes)
- descriptive external names can replace generic placeholders (`sub_*`, `fn_0x*`)
- selector-based deterministic inference can tag standard calls from object-pool arguments even when call targets stay generic
- recognized calls add intent comments in pseudocode, for example:
  - `// stdlib:dart.core.print`
  - `// stdlib:dart.core.map [selector]`
  - `// framework:flutter.widgets.State.setState`
  - `// framework:flutter.widgets.Widget.build [selector]`
  - `// runtime:dart_vm.invoke`
  - `// native:libc.memcpy`

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
