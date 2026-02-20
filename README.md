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
flutterdec decompile <apk|so> -o out/ [--emit-asm] [--emit-ir]
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
```

Output files:

- `out/pseudocode/*.dartpseudo`
- `out/asm/*.s` (when `--emit-asm`)
- `out/ir/*.json` (when `--emit-ir`)
- `out/quality.json`
- `out/report.json`

### `adapter`

Arguments:

- `install --dart-hash <DART_HASH>`: install adapter metadata for a Dart hash
- `list`: list installed adapters

Examples:

```bash
flutterdec adapter install --dart-hash 4b8f1f
flutterdec adapter list
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
