# User Guide

## Scope

Current supported target scope:

- Flutter AOT Android ARM64 binaries
- static analysis only

Inputs:

- APK
- `libapp.so`

## Install And Run

If you want the shortest path:

- run without installing: `nix run`
- install persistently with Nix: `nix profile install`
- install a release binary

### Run Without Installing

From GitHub:

```bash
nix run github:caverav/flutterdec -- --help
nix run github:caverav/flutterdec -- info ./sample.apk --json
nix run github:caverav/flutterdec -- decompile ./sample.apk -o ./out
```

From a local checkout:

```bash
nix run . -- --help
nix run . -- info ./sample.apk --json
nix run . -- decompile ./sample.apk -o ./out
```

### Install Persistently With Nix

Install from GitHub:

```bash
nix profile install github:caverav/flutterdec
flutterdec --help
```

Install from a local checkout:

```bash
nix profile install .
flutterdec --help
```

Update later:

```bash
nix profile upgrade flutterdec
```

### Install From A Release Binary

Current prerelease: [`v0.1.0-alpha.2`](https://github.com/caverav/flutterdec/releases/tag/v0.1.0-alpha.2)

Linux x64:

```bash
curl -fLO https://github.com/caverav/flutterdec/releases/download/v0.1.0-alpha.2/flutterdec-v0.1.0-alpha.2-Linux-X64.tar.gz
tar -xzf flutterdec-v0.1.0-alpha.2-Linux-X64.tar.gz
sudo install -m 0755 flutterdec /usr/local/bin/flutterdec
flutterdec --help
```

macOS arm64:

```bash
curl -fLO https://github.com/caverav/flutterdec/releases/download/v0.1.0-alpha.2/flutterdec-v0.1.0-alpha.2-macOS-ARM64.tar.gz
tar -xzf flutterdec-v0.1.0-alpha.2-macOS-ARM64.tar.gz
sudo install -m 0755 flutterdec /usr/local/bin/flutterdec
flutterdec --help
```

Other platforms and future tags:

[Releases page](https://github.com/caverav/flutterdec/releases)

### Other Ways To Run It

Install into the user Cargo bin:

```bash
nix develop -c cargo install --path crates/flutterdec-cli
~/.cargo/bin/flutterdec --help
```

Run from source without installing:

```bash
nix develop -c cargo run -p flutterdec-cli -- --help
nix develop -c cargo run -p flutterdec-cli -- info ./sample.apk --json
nix develop -c cargo run -p flutterdec-cli -- decompile ./sample.apk -o ./out
```

Build a local release binary:

```bash
nix develop -c cargo build -p flutterdec-cli --release
./target/release/flutterdec --help
```

## First Use

If this is your first run, this is the shortest useful path.

1. Inspect the target:

```bash
flutterdec info ./sample.apk --json
```

For APK inputs, `info` also reports Android startup summary fields:

- `android_startup_present`
- `android_startup_confidence`
- `android_startup_entrypoint_count`
- `android_startup_flutter_activity_count`

If adapter metadata is available, `info` also reports:

- `app_package_counts_top`
- `adapter_kind`
- `manifest_entry_present`
- `adapter_snapshot_hash_match`
- `compatibility_warnings`

2. Install the adapter for the detected Dart hash:

```bash
flutterdec adapter install --dart-hash <HASH>
flutterdec adapter list
```

3. Decompile:

```bash
flutterdec decompile ./sample.apk -o ./out
```

4. Start with:

- `out/pseudocode/*.dartpseudo`
- `out/report.json`
- `out/quality.json`

## Basic Commands

Inspect target metadata:

```bash
flutterdec info ./sample.apk --json
```

Decompile with the default app-focused scope:

```bash
flutterdec decompile ./sample.apk -o ./out
```

Decompile with extra artifacts:

```bash
flutterdec decompile ./sample.apk -o ./out --emit-asm --emit-ir
```

Compare two builds at recovered-function level:

```bash
flutterdec diff --old ./old.apk --new ./new.apk -o ./out-diff --json
```

Generate a Ghidra import script with recovered symbols:

```bash
flutterdec decompile ./sample.apk -o ./out --emit-ghidra-script
```

Generate an IDA import script with recovered symbols:

```bash
flutterdec decompile ./sample.apk -o ./out --emit-ida-script
```

## Function Scope

Default scope is app-focused:

- `app-unknown` (default): app (`package:*`) plus unknown ownership functions
- `app`: only app (`package:*`) functions
- `all`: include Flutter, Dart runtime, and framework internals too

Include everything:

```bash
flutterdec decompile ./sample.apk -o ./out --function-scope all
```

Limit output to selected app packages:

```bash
flutterdec decompile ./sample.apk -o ./out \
  --function-scope app-unknown \
  --app-package my_app
```

Tip: if you are not sure about package names, check `report.json` under `function_scope.app_package_counts_top`.

## Single-Function Targeting

Decompile and disassemble one specific function:

```bash
flutterdec decompile ./sample.apk -o ./out \
  --target id:42 \
  --emit-asm
```

Target by entry address:

```bash
flutterdec decompile ./sample.apk -o ./out \
  --target va:0x613468 \
  --emit-asm
```

`--target` accepts `id:<N>`, `va:0x<ADDR>`, `0x<ADDR>`, or `<N>`.
When `<N>` matches multiple functions, the command fails and asks for explicit `id:` or `va:`.
Target mode emits only the matched function artifacts and reports selection details in `report.json.target_selection`.
If the match is outside current scope filters, target mode can override scope automatically for that function.

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

Adapter backend options:

- `--adapter-backend auto` (default): attempt Blutter bridge backend when configured, otherwise fallback to internal adapter
- `--adapter-backend internal`: force internal adapter only
- `--adapter-backend blutter`: require Blutter bridge backend with no fallback
- `--require-snapshot-hash-match`: fail if adapter snapshot hash does not match loader snapshot hash

Blutter bridge environment variables:

- `FLUTTERDEC_BLUTTER_CMD`: full command used to launch Blutter, for example `python3 /opt/blutter/blutter.py`
- `FLUTTERDEC_BLUTTER_PY`: direct path to `blutter.py`

Nix setup note:

- in `nix develop`, `FLUTTERDEC_BLUTTER_CMD` is exported automatically to a Nix-managed `flutterdec-blutter` wrapper
- direct wrapper invocation is available with `nix run .#blutter-bridge -- --help`

Per-feature toggles:

- `--with-canonical-model-symbols` / `--no-canonical-model-symbols`
- `--with-pool-value-hints` / `--no-pool-value-hints`
- `--with-pool-semantic-hints` / `--no-pool-semantic-hints`
- `--with-semantic-reporting` / `--no-semantic-reporting`
- `--with-bootflow-category-seeds` / `--no-bootflow-category-seeds`
- `--with-apk-startup-analysis` / `--no-apk-startup-analysis`

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

## Name Recovery With Engine Symbols

Generate direct-call target mapping from a stripped/unstripped engine pair:

```bash
flutterdec map-symbols \
  --stripped ./libflutter.stripped.so \
  --unstripped ./libflutter.unstripped.so \
  -o ./out/symbol-map \
  --register-local-cache
```

Use the cached mapping in later APK decompile runs:

```bash
flutterdec decompile ./sample.apk -o ./out \
  --extra-symbol-elf ./libflutter.unstripped.so
```

When the cached build id matches the APK's embedded `libflutter.so`, `decompile` auto-loads the registered target summary and reports the match under `report.json.engine_symbol_ingestion`.

## Outputs

Main output artifacts:

- `pseudocode/*.dartpseudo`
- `report.json`
- `quality.json`
- `asm/*.s` if `--emit-asm`
- `ir/*.json` if `--emit-ir`
- `ghidra_apply_symbols.py` if `--emit-ghidra-script`
- `ida_apply_symbols.py` if `--emit-ida-script`
- `diff_report.json` if you run `flutterdec diff`

Most users should start by reading:

- `pseudocode/*.dartpseudo`
- `report.json.android_startup`
- `quality.json`
