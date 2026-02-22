# flutterdec

[![CI](https://github.com/caverav/flutterdec/actions/workflows/ci.yml/badge.svg)](https://github.com/caverav/flutterdec/actions/workflows/ci.yml)
[![Release](https://github.com/caverav/flutterdec/actions/workflows/release.yml/badge.svg)](https://github.com/caverav/flutterdec/actions/workflows/release.yml)

`flutterdec` is a static Flutter AOT decompiler research tool for Android ARM64 binaries.

It takes an APK (or `libapp.so`) and emits readable pseudo-Dart plus optional IR/ASM artifacts.

## Who This Is For

- reverse engineers and security researchers
- Flutter internals researchers
- developers comparing stripped/unstripped engine builds

## Quick Start

Recommended install: use GitHub release artifacts.

1. Download and install the first alpha release (current tag: `v0.1.0-alpha.1`):

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

If your platform artifact is not available yet, open:

[v0.1.0-alpha.1 release page](https://github.com/caverav/flutterdec/releases/tag/v0.1.0-alpha.1)

Other install options:

- Run from source (no install, requires Nix with flakes enabled):

```bash
nix develop -c cargo run -p flutterdec-cli -- info ./sample.apk --json
```

- Install into user Cargo bin (requires Nix with flakes enabled):

```bash
nix develop -c cargo install --path crates/flutterdec-cli
~/.cargo/bin/flutterdec --help
```

If `flutterdec` is not found in your shell, add Cargo bin to `PATH`:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Build a standalone release binary (no Cargo install):

```bash
nix develop -c cargo build -p flutterdec-cli --release
./target/release/flutterdec --help
```

Optional system-wide install from that binary:

```bash
sudo install -m 0755 ./target/release/flutterdec /usr/local/bin/flutterdec
flutterdec --help
```

## Typical Workflow

1. Inspect target:

```bash
flutterdec info ./sample.apk --json
```

2. Install adapter for the detected Dart hash:

```bash
flutterdec adapter install --dart-hash <HASH>
```

3. Decompile:

```bash
flutterdec decompile ./sample.apk -o ./out
```

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

You can explicitly enable/disable individual engine toggles:

- `--with-canonical-model-symbols` / `--no-canonical-model-symbols`
- `--with-pool-value-hints` / `--no-pool-value-hints`
- `--with-pool-semantic-hints` / `--no-pool-semantic-hints`
- `--with-semantic-reporting` / `--no-semantic-reporting`

## Output

Main outputs under `-o <OUT_DIR>`:

- `pseudocode/*.dartpseudo`
- `quality.json`
- `report.json`
- `asm/*.s` (if `--emit-asm`)
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
