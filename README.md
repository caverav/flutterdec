<p align="center">
  <img src="docs/assets/flutterdec-banner.png" alt="flutterdec banner" width="900">
</p>

# flutterdec

[![CI](https://github.com/caverav/flutterdec/actions/workflows/ci.yml/badge.svg)](https://github.com/caverav/flutterdec/actions/workflows/ci.yml)
[![Release](https://github.com/caverav/flutterdec/actions/workflows/release.yml/badge.svg)](https://github.com/caverav/flutterdec/actions/workflows/release.yml)

`flutterdec` is a static Flutter AOT decompiler research tool for Android ARM64 binaries.

It takes an APK (or `libapp.so`) and emits readable pseudo-Dart plus optional IR/ASM artifacts.

## See The Pipeline

The goal of these examples is simple: show original public source first, then show what `flutterdec` recovers from the shipped APK.

<p align="center"><strong>Original 1: Android Startup Surface</strong></p>

<p align="center">
  <img src="docs/assets/readme/hivpn-mainactivity.svg" alt="hiVPN MainActivity source snippet" width="900">
</p>

<p align="center"><strong>Original 2: App-Side Flutter / Dart Surface</strong></p>

<p align="center">
  <img src="docs/assets/readme/hivpn-flutter-bridge.svg" alt="hiVPN Flutter bridge source snippet" width="900">
</p>

**Compare 1: Startup Source -> Recovered Startup Path**

Source app: `hiVPN v1.0.0` released on October 29, 2025. `MainActivity` and the manifest launcher are public in the repository:
- Source repo: https://github.com/Mr-Dark-debug/hivpn
- Release APK: https://github.com/Mr-Dark-debug/hivpn/releases/tag/release

`Original`

The app enters Flutter from `MainActivity.onCreate`. The second source card shows the app-side Flutter bridge that exposes `MethodChannel('com.example.vpn/VpnChannel')` to Dart code.

<p align="center"><strong>Recovered 1: APK Startup Report</strong></p>

<p align="center">
  <img src="docs/assets/readme/hivpn-startup-report.svg" alt="hiVPN startup report excerpt recovered by flutterdec" width="900">
</p>

`Recovered`

`flutterdec` parsed the APK manifest, recovered `com.example.hivpn.MainActivity` as the launcher, and correlated the startup chain from `MainActivity.onCreate` into Flutter JNI bootstrap calls such as `attachToNative` and `nativeAttach`.

**Compare 2: App Source Using Flutter APIs -> Recovered Named Selectors**

Source app: `ZedSecure v1.2.0`
- Source repo: https://github.com/CluvexStudio/ZedSecure
- Release APK: https://github.com/CluvexStudio/ZedSecure/releases/tag/v1.2.0

This is the comparison you asked for: public app source that uses Flutter APIs, then recovered APK artifacts where `flutterdec` keeps recognizable Dart/Flutter selector names instead of only anonymous control flow.

<p align="center"><strong>Original Source</strong></p>

<p align="center">
  <img src="docs/assets/readme/zedsecure-minwidth-source.svg" alt="Original ZedSecure UI source using BoxConstraints(minWidth: 50)" width="900">
</p>

This source is ordinary app UI code. It builds a ping badge with `BoxConstraints(minWidth: 50)` and other Flutter widget APIs inside the shipped app.

<p align="center"><strong>Recovered 2: From The ZedSecure APK With flutterdec</strong></p>

<p align="center"><strong>↓ ARM64</strong></p>

At the machine-code layer, the APK still looks like indirect selector dispatch through pool-loaded metadata and call targets.

<p align="center">
  <img src="docs/assets/readme/zedsecure-minwidth-asm.svg" alt="ARM64 snippet from the recovered ZedSecure function showing the minWidth selector pool load" width="900">
</p>

<p align="center">
  <strong>↓ Function IR</strong>
</p>

<p align="center">
  <img src="docs/assets/readme/zedsecure-minwidth-ir.svg" alt="IR summary for the recovered ZedSecure minWidth selector flow" width="900">
</p>

The IR stage makes the selector-bearing pool values explicit before readability passes.

<p align="center">
  <strong>↓ Pseudocode</strong>
</p>

<p align="center">
  <img src="docs/assets/readme/zedsecure-minwidth-pseudocode.svg" alt="Recovered pseudocode with named Flutter selectors from the ZedSecure APK" width="900">
</p>

The important part is not the anonymous function name. The important part is that `flutterdec` surfaced readable Flutter selector names from the APK itself, including `dispatch.minWidth(...)`, `dispatch.messageMap(...)`, and the framework-side `flutter.foundation.invoke(...)`.

This gives the README both views the tool is meant to show publicly:
- Android startup recovery from the APK surface
- app-side Flutter selector recovery from the AOT payload

**What this proves**

- `flutterdec` can preserve recognizable Flutter/Dart utility selectors inside app-owned recovered code
- selector-bearing pool metadata survives from asm to IR to pseudocode
- the pipeline is inspectable at every stage: asm, IR, and pseudocode

<p align="center">
  <img src="docs/assets/readme/localsend-diff.svg" alt="LocalSend diff summary across two public releases" width="900">
</p>

**Case 3: Original Release A -> Original Release B -> Recovered Diff**

Source app: `LocalSend`
- `v1.16.1` released on November 5, 2024
- `v1.17.0` released on February 20, 2025
- Releases: https://github.com/localsend/localsend/releases

`Original`

Two public release APKs from the same app line.

`Recovered`

`flutterdec diff` compared the two arm64 APKs directly and emitted added/removed/common function summaries plus package-level change counts. This is useful when you care more about what changed between versions than about reconstructing a single function in isolation.

## What You Inspect

| Need | Artifact | Why it matters |
| --- | --- | --- |
| Recover readable logic | `pseudocode/*.dartpseudo` | Best first-pass view of branches, loops, returns, and named callsites |
| Validate decompiler decisions | `asm/*.s` and `ir/*.json` | Lets you confirm control-flow shape when pseudocode gets suspicious |
| Trace Flutter startup | `report.json.android_startup` | Surfaces manifest anchors, embedder stages, and recovered `DartEntrypoint` evidence |
| Audit analysis quality | `quality.json` and `report.json` | Shows coverage, compatibility, target selection, and symbol-ingestion diagnostics |

## North Star

Recover readable behavior from Flutter AOT ARM64 binaries with enough semantic structure that reverse engineering decisions can be made from pseudocode and reports.

### Primary Goals

- robust semantic extraction from snapshots and metadata (libraries, classes, functions, selectors, pool semantics)
- stable reverse-engineering-oriented pseudocode output for Android ARM64 release builds
- version-aware adapter behavior that can be updated without rewriting core/decompiler logic

### Non-Goals (Current Scope)

- perfect source reconstruction of original Dart code
- broad multi-arch support in the same maturity level (x86, iOS, JIT modes)
- dynamic runtime emulation as the default analysis path

## Who This Is For

- reverse engineers and security researchers
- Flutter internals researchers
- developers comparing stripped/unstripped engine builds

## Quick Start

1. Run with `nix run` (recommended, no install):

```bash
nix run github:caverav/flutterdec -- --help
nix run github:caverav/flutterdec -- info ./sample.apk --json
```

From this repository checkout:

```bash
nix run . -- --help
```

2. Install release binary (`v0.1.0-alpha.2`):

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

Install into user Cargo bin (requires Nix with flakes enabled):

```bash
nix develop -c cargo install --path crates/flutterdec-cli
~/.cargo/bin/flutterdec --help
```

Run from source without installing:

```bash
nix develop -c cargo run -p flutterdec-cli -- info ./sample.apk --json
```

Build local release binary:

```bash
nix develop -c cargo build -p flutterdec-cli --release
./target/release/flutterdec --help
```

## Typical Workflow

1. Inspect target:

```bash
flutterdec info ./sample.apk --json
```

`info` now includes detected app package candidates (`app_package_counts_top`) when adapter metadata is available.
For APK inputs it also reports Android startup summary fields: `android_startup_present`, `android_startup_confidence`, `android_startup_entrypoint_count`, and `android_startup_flutter_activity_count`.

2. Install adapter for the detected Dart hash:

```bash
flutterdec adapter install --dart-hash <HASH>
```

3. Decompile:

```bash
flutterdec decompile ./sample.apk -o ./out
```

By default, `decompile` focuses app reversing (`--function-scope app-unknown`) and excludes known Flutter/Dart framework internals.

To include all functions (app + Flutter + Dart/runtime):

```bash
flutterdec decompile ./sample.apk -o ./out --function-scope all
```

To focus only specific Dart packages (repeatable):

```bash
flutterdec decompile ./sample.apk -o ./out \
  --function-scope app-unknown \
  --app-package my_app
```

If package names are unknown, inspect `report.json` at `function_scope.app_package_counts_top`.
When `--app-package` is not provided, capped prioritization also applies manifest-derived package hints (`function_scope.priority_package_hints`) to favor app-owned code (including normalized variants like `localsend_app` and `localsend` when applicable).

To target a single function for developer-focused decompile/disassembly:

```bash
flutterdec decompile ./sample.apk -o ./out \
  --target va:0x613468 \
  --emit-asm
```

`--target` accepts `id:<N>`, `va:0x<ADDR>`, `0x<ADDR>`, or `<N>` (auto id/address match).
If `<N>` is ambiguous, `flutterdec` asks for explicit `id:` or `va:`. Selection details are emitted in `report.json.target_selection`.

4. Optional: improve call names with stripped/unstripped engine pair:

```bash
flutterdec map-symbols \
  --stripped ./libflutter.stripped.so \
  --unstripped ./libflutter.unstripped.so \
  -o ./out/symbol-map \
  --register-local-cache

flutterdec decompile ./sample.apk -o ./out \
  --extra-symbol-elf ./libflutter.unstripped.so
```

If the cached engine build id matches the APK’s embedded `libflutter.so`, `decompile` auto-loads the cached `symbol_target_summary.json` and reports it under `report.json.engine_symbol_ingestion`.

5. Optional: compare two builds by recovered function signatures:

```bash
flutterdec diff --old ./old.apk --new ./new.apk -o ./out-diff --json
```

`diff_report.json` includes added/removed/common function counts plus `added_packages_top` and `removed_packages_top` summaries.

6. Optional: emit import scripts for RE tools:

```bash
flutterdec decompile ./sample.apk -o ./out \
  --emit-ghidra-script \
  --emit-ida-script
```

## Analysis Profiles

`decompile` exposes analysis-engine profiles so you can trade detail for speed.

Default profile:

- `balanced` (recommended)

Available profiles:

- `balanced`: full semantic naming/hints/reporting
- `light`: lower-overhead analysis for faster large-scale runs

Example:

```bash
flutterdec decompile ./sample.apk -o ./out --analysis-profile light
```

Adapter backend selection:

- `--adapter-backend auto` (default): try Blutter backend if configured, otherwise fallback to internal adapter
- `--adapter-backend internal`: force internal snapshot-string adapter
- `--adapter-backend blutter`: require Blutter backend (fail if unavailable)
- `--require-snapshot-hash-match`: fail early when adapter-reported snapshot hash does not match loader snapshot hash

Blutter backend environment knobs:

- `FLUTTERDEC_BLUTTER_CMD`: full command to launch Blutter (for example `python3 /path/to/blutter.py`)
- `FLUTTERDEC_BLUTTER_PY`: path to `blutter.py` (uses current Python interpreter)

Nix integration:

- `nix develop` now provides `flutterdec-blutter` and auto-exports `FLUTTERDEC_BLUTTER_CMD` to that wrapper.
- You can also run the wrapper directly via `nix run .#blutter-bridge -- --help`.

You can explicitly enable/disable individual engine toggles:

- `--with-canonical-model-symbols` / `--no-canonical-model-symbols`
- `--with-pool-value-hints` / `--no-pool-value-hints`
- `--with-pool-semantic-hints` / `--no-pool-semantic-hints`
- `--with-semantic-reporting` / `--no-semantic-reporting`
- `--with-bootflow-category-seeds` / `--no-bootflow-category-seeds`
- `--with-apk-startup-analysis` / `--no-apk-startup-analysis`

## Output

Main outputs under `-o <OUT_DIR>`:

- `pseudocode/*.dartpseudo`
- `quality.json`
- `report.json`
- `diff_report.json` (if `flutterdec diff`)
- `asm/*.s` (if `--emit-asm`)
- opcode-prefixed asm lines (if `--emit-asm --emit-asm-opcodes`)
- `ghidra_apply_symbols.py` (if `--emit-ghidra-script`; applies symbol names and pool-load comments)
- `ida_apply_symbols.py` (if `--emit-ida-script`; applies symbol names and pool-load comments in IDA)
- `ir/*.json` (if `--emit-ir`)

`report.json` also includes:

- `compatibility` for schema/hash/manifest alignment diagnostics
- `android_manifest` for manifest-derived launcher/deeplink/activity signals
- `android_startup` for APK bytecode startup evidence such as embedding calls, JNI bootstrap stages, and recovered `DartEntrypoint` callsites when present
- recovered `android_startup.dart_entrypoints` entries can now carry `function_name`, `library_uri`, and `app_bundle_path` when those values are directly recoverable from APK bytecode or through simple app-helper return propagation
- `android_startup.bootstrap_chain` summarizes observed Android embedder startup stages per source method, including app-vs-framework ownership, ordered stages, completeness, and missing steps
- `engine_symbol_ingestion` for auto-loaded local engine symbol cache matches keyed by `libflutter.so` build id
- `bootflow_discovery` entries tagged by `source` (`adapter`, `manifest`, `apk_startup`); APK-startup-backed entries may appear with `target_va: null` when the startup signal is real but the Dart function is not yet mapped

## Documentation

- User guide: [docs/user-guide.md](docs/user-guide.md)
- CLI reference: [docs/cli-reference.md](docs/cli-reference.md)
- Development guide: [docs/development.md](docs/development.md)
- Architecture: [docs/architecture.md](docs/architecture.md)
- Internals walkthrough: [docs/how-it-works.md](docs/how-it-works.md)
- Research decisions: [docs/research-decisions.md](docs/research-decisions.md)
- Contributing: [CONTRIBUTING.md](CONTRIBUTING.md)
- Context and project history: [context.md](context.md)

## Issue Types

- Bug report: [new bug issue](https://github.com/caverav/flutterdec/issues/new?template=bug_report.md)
- Feature request: [new feature issue](https://github.com/caverav/flutterdec/issues/new?template=feature_request.md)
- Research finding: [new research issue](https://github.com/caverav/flutterdec/issues/new?template=research_finding.md)
