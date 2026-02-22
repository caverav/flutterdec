# User Guide

## Scope

Current supported target scope:

- Flutter AOT Android ARM64 binaries
- static analysis only

Inputs:

- APK
- `libapp.so`

## Setup

1. Run with `nix run` (recommended):

```bash
nix run github:caverav/flutterdec -- --help
nix run github:caverav/flutterdec -- info ./sample.apk --json
```

From this repository checkout:

```bash
nix run . -- --help
```

2. Install from release binary (`v0.1.0-alpha.1`):

Linux x64:

```bash
curl -fLO https://github.com/caverav/flutterdec/releases/download/v0.1.0-alpha.1/flutterdec-v0.1.0-alpha.1-Linux-X64.tar.gz
tar -xzf flutterdec-v0.1.0-alpha.1-Linux-X64.tar.gz
sudo install -m 0755 flutterdec /usr/local/bin/flutterdec
flutterdec --help
```

macOS arm64:

```bash
curl -fLO https://github.com/caverav/flutterdec/releases/download/v0.1.0-alpha.1/flutterdec-v0.1.0-alpha.1-macOS-ARM64.tar.gz
tar -xzf flutterdec-v0.1.0-alpha.1-macOS-ARM64.tar.gz
sudo install -m 0755 flutterdec /usr/local/bin/flutterdec
flutterdec --help
```

Other platforms and future tags:

[Releases page](https://github.com/caverav/flutterdec/releases)

3. Other options:

Install to user Cargo bin (requires Nix with flakes enabled):

```bash
nix develop -c cargo install --path crates/flutterdec-cli
~/.cargo/bin/flutterdec --help
```

Run from source without installing:

```bash
nix develop -c cargo run -p flutterdec-cli -- --help
```

Build local release binary:

```bash
nix develop -c cargo build -p flutterdec-cli --release
./target/release/flutterdec --help
```

## Basic Commands

Inspect target metadata:

```bash
flutterdec info ./sample.apk --json
```

Install adapter for Dart hash:

```bash
flutterdec adapter install --dart-hash <HASH>
flutterdec adapter list
```

Decompile:

```bash
flutterdec decompile ./sample.apk -o ./out
```

Default scope is app-focused:

- `app-unknown` (default): app (`package:*`) + unknown ownership functions
- `app`: only app (`package:*`) functions
- `all`: include Flutter/Dart/runtime/framework internals too

Limit output to selected app packages (repeat `--app-package` as needed):

```bash
flutterdec decompile ./sample.apk -o ./out \
  --function-scope app-unknown \
  --app-package my_app
```

Tip: if you are not sure about package names, check `report.json` under `function_scope.app_package_counts_top`.

Include everything:

```bash
flutterdec decompile ./sample.apk -o ./out --function-scope all
```

Decompile with extra artifacts:

```bash
flutterdec decompile ./sample.apk -o ./out --emit-asm --emit-ir
```

## Analysis Engine Profiles

`decompile` profile controls analysis depth vs throughput.

Profile options:

- `balanced` (default): best readability and semantic recovery
- `light`: reduced analysis for faster large-scale runs

Examples:

```bash
flutterdec decompile ./sample.apk -o ./out --analysis-profile balanced
flutterdec decompile ./sample.apk -o ./out --analysis-profile light
```

Per-feature toggles (override profile defaults):

- `--with-canonical-model-symbols` / `--no-canonical-model-symbols`
- `--with-pool-value-hints` / `--no-pool-value-hints`
- `--with-pool-semantic-hints` / `--no-pool-semantic-hints`
- `--with-semantic-reporting` / `--no-semantic-reporting`

Note: each `--with-*` conflicts with its matching `--no-*` flag.

## Quality Gates

Decompile quality checks are controlled by:

- `--max-placeholder-ifs`
- `--max-unresolved-cf`
- `--max-indirect-call-ratio`
- `--min-disassembly-ratio`

Example for exploratory runs:

```bash
flutterdec decompile ./sample.apk -o ./out \
  --max-functions 250 \
  --max-placeholder-ifs 999999 \
  --max-unresolved-cf 999999 \
  --max-indirect-call-ratio 1.0 \
  --min-disassembly-ratio 0.0
```

## Name Recovery with Engine Symbols

Generate direct-call target mapping from stripped/unstripped engine pair:

```bash
flutterdec map-symbols \
  --stripped ./libflutter.stripped.so \
  --unstripped ./libflutter.unstripped.so \
  -o ./out/symbol-map
```

Use mapping in decompile:

```bash
flutterdec decompile ./sample.apk -o ./out \
  --extra-symbol-map-targets ./out/symbol-map/symbol_target_summary.json \
  --extra-symbol-elf ./libflutter.unstripped.so
```

## Outputs

Written under output directory:

- `pseudocode/*.dartpseudo`
- `quality.json`
- `report.json`
- `asm/*.s` with `--emit-asm`
- `ir/*.json` with `--emit-ir`
