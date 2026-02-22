# User Guide

## Scope

Current supported target scope:

- Flutter AOT Android ARM64 binaries
- static analysis only

Inputs:

- APK
- `libapp.so`

## Setup

Recommended install: GitHub release artifact.

Install the first alpha release (current tag: `v0.1.0-alpha.1`):

```bash
TAG="v0.1.0-alpha.1"
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux) os_name="Linux" ;;
  Darwin) os_name="macOS" ;;
  *) echo "Unsupported OS: $OS" >&2; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64) arch_name="X64" ;;
  arm64|aarch64) arch_name="ARM64" ;;
  *) echo "Unsupported arch: $ARCH" >&2; exit 1 ;;
esac

asset="flutterdec-${TAG}-${os_name}-${arch_name}.tar.gz"
url="https://github.com/caverav/flutterdec/releases/download/${TAG}/${asset}"

curl -fL -o "$asset" "$url"
tar -xzf "$asset"
sudo install -m 0755 flutterdec /usr/local/bin/flutterdec
flutterdec --help
```

If your platform artifact is not available, open:

[v0.1.0-alpha.1 release page](https://github.com/caverav/flutterdec/releases/tag/v0.1.0-alpha.1)

Other options (source install):

- Prerequisite: Nix with flakes enabled

Run without installing:

```bash
nix develop -c cargo run -p flutterdec-cli -- --help
```

Install CLI to user Cargo bin:

```bash
nix develop -c cargo install --path crates/flutterdec-cli
~/.cargo/bin/flutterdec --help
```

If `flutterdec` is not found:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Build release binary:

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
