# flutterdec

[![CI](https://github.com/caverav/flutterdec/actions/workflows/ci.yml/badge.svg)](https://github.com/caverav/flutterdec/actions/workflows/ci.yml)
[![Release](https://github.com/caverav/flutterdec/actions/workflows/release.yml/badge.svg)](https://github.com/caverav/flutterdec/actions/workflows/release.yml)

`flutterdec` is a static Flutter AOT decompiler research tool for Android ARM64 binaries.

It takes an APK (or `libapp.so`) and emits readable pseudo-Dart plus optional IR/ASM artifacts.

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

2. Install release binary (`v0.1.0-alpha.1`):

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

4. Optional: improve call names with stripped/unstripped engine pair:

```bash
flutterdec map-symbols \
  --stripped ./libflutter.stripped.so \
  --unstripped ./libflutter.unstripped.so \
  -o ./out/symbol-map

flutterdec decompile ./sample.apk -o ./out \
  --extra-symbol-map-targets ./out/symbol-map/symbol_target_summary.json \
  --extra-symbol-elf ./libflutter.unstripped.so
```

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
