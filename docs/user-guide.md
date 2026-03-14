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

2. Install from release binary (`v0.1.0-alpha.2`):

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

If adapter metadata is available, `info` reports `app_package_counts_top` plus compatibility signals (`adapter_kind`, `manifest_entry_present`, `adapter_snapshot_hash_match`, `compatibility_warnings`).
For APK inputs, `info` also reports Android startup summary fields: `android_startup_present`, `android_startup_confidence`, `android_startup_entrypoint_count`, and `android_startup_flutter_activity_count`.

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
When `<N>` matches multiple functions (id and entry address), the command fails and asks for explicit `id:` or `va:`.
Target mode emits only the matched function artifacts and reports selection details in `report.json.target_selection`.
If the match is outside current scope filters, target mode can override scope automatically for that function.

Compare two builds at recovered-function level:

```bash
flutterdec diff --old ./old.apk --new ./new.apk -o ./out-diff --json
```

Diff JSON output also includes `added_packages_top` and `removed_packages_top` to quickly see where most churn happened.

Include raw opcode words in asm output:

```bash
flutterdec decompile ./sample.apk -o ./out --emit-asm --emit-asm-opcodes
```

Generate a Ghidra import script with recovered symbols:

```bash
flutterdec decompile ./sample.apk -o ./out --emit-ghidra-script
```

Generate an IDA import script with recovered symbols:

```bash
flutterdec decompile ./sample.apk -o ./out --emit-ida-script
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

Adapter backend options:

- `--adapter-backend auto` (default): attempt Blutter bridge backend when configured, otherwise fallback to internal adapter
- `--adapter-backend internal`: force internal adapter only
- `--adapter-backend blutter`: require Blutter bridge backend (no fallback)
- `--require-snapshot-hash-match`: fail if adapter snapshot hash does not match loader snapshot hash

Blutter bridge environment variables:

- `FLUTTERDEC_BLUTTER_CMD`: full command used to launch Blutter (example: `python3 /opt/blutter/blutter.py`)
- `FLUTTERDEC_BLUTTER_PY`: direct path to `blutter.py` (uses current Python interpreter)

Nix setup note:

- In `nix develop`, `FLUTTERDEC_BLUTTER_CMD` is exported automatically to a Nix-managed `flutterdec-blutter` wrapper.
- Direct wrapper invocation is available with `nix run .#blutter-bridge -- --help`.

Per-feature toggles (override profile defaults):

- `--with-canonical-model-symbols` / `--no-canonical-model-symbols`
- `--with-pool-value-hints` / `--no-pool-value-hints`
- `--with-pool-semantic-hints` / `--no-pool-semantic-hints`
- `--with-semantic-reporting` / `--no-semantic-reporting`
- `--with-bootflow-category-seeds` / `--no-bootflow-category-seeds`
- `--with-apk-startup-analysis` / `--no-apk-startup-analysis`

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
  -o ./out/symbol-map \
  --register-local-cache
```

Use the cached mapping in later APK decompile runs:

```bash
flutterdec decompile ./sample.apk -o ./out \
  --extra-symbol-elf ./libflutter.unstripped.so
```

When the cached build id matches the APK’s embedded `libflutter.so`, `decompile` auto-loads the registered target summary and reports the match under `report.json.engine_symbol_ingestion`.

## Outputs

Written under output directory:

- `pseudocode/*.dartpseudo`
- `quality.json`
- `report.json`
- `diff_report.json` with `flutterdec diff`
- `asm/*.s` with `--emit-asm`
- opcode-prefixed asm lines with `--emit-asm --emit-asm-opcodes`
- `ghidra_apply_symbols.py` with `--emit-ghidra-script` (symbol names + pool load comments)
- `ida_apply_symbols.py` with `--emit-ida-script` (symbol names + pool load comments)
- `ir/*.json` with `--emit-ir`

`report.json` includes compatibility diagnostics (schema support, manifest-entry presence, and snapshot-hash alignment).
For APK inputs it also includes:

- `android_manifest` with manifest-derived launcher, deeplink, and activity signals
- `android_startup` with `classes*.dex` startup evidence, JNI/bootstrap stages, and recovered `DartEntrypoint` callsites when present
- `bootflow_discovery` entries tagged by `source` (`adapter`, `manifest`, `apk_startup`); `apk_startup` entries may carry `target_va: null` when the Android startup path is known but the Dart-side function has not been resolved yet
- recovered `android_startup.dart_entrypoints` items include `function_name`, `library_uri`, and `app_bundle_path` when those strings are directly visible in APK bytecode
- `android_startup.bootstrap_chain` reports ordered startup-stage observations per source method and, when app-defined method edges can be correlated, emits `paths` that connect entry methods such as `MainActivity.onCreate` or `configureFlutterEngine` to framework stages like `FlutterEngine.<init>`, `FlutterJNI.attachToNative`, and `DartExecutor.executeDartEntrypoint`; each path also carries `anchor_kind`, `anchor_component_name`, and `anchor_confidence` so you can see whether it is tied back to a manifest launcher/deeplink/application component or only to a heuristic startup fragment
- `engine_symbol_ingestion` reports whether a local cached engine symbol target summary was auto-loaded by exact build-id match
