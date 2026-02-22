# Development Guide

## Toolchain

Use Nix shell for reproducible builds:

```bash
nix develop
```

Common commands:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI-parity local check:

```bash
scripts/ci-check.sh
# or
nix run .#ci-check
```

## Repository Layout

- `crates/flutterdec-cli`: command parsing and dispatch
- `crates/flutterdec-core`: orchestration, quality, reports, symbol map, engine fingerprint
- `crates/flutterdec-loader`: APK/ELF loading and snapshot extraction
- `crates/flutterdec-adapter`: adapter install/run boundaries
- `crates/flutterdec-disasm-arm64`: ARM64 disassembly
- `crates/flutterdec-ir`: IR + CFG
- `crates/flutterdec-decompiler`: pseudo-Dart emission and readability passes
- `adapters/`: adapter metadata and Python adapter implementation
- `schemas/`: adapter schema
- `scripts/`: regression tooling

## Readability/Decompilation Work

When modifying decompiler behavior:

1. add/adjust focused unit tests first
2. run crate tests
3. validate against real binaries (not only synthetic tests)

Real-binary regression helpers:

- `scripts/real-golden.sh`
- `scripts/real-golden-matrix.sh`

## Quality and Reports

`decompile` writes:

- `quality.json` (quality gate results)
- `report.json` (semantic/fallback/metadata summaries)

Analysis profile and resolved engine options are written into `report.json` under `analysis`.

## CI

CI runs on:

- pull requests
- pushes to `main`
- Linux and macOS runners

Workflow checks:

- `cargo fmt --check`
- `cargo clippy` with warnings denied
- `cargo test --workspace`
- `cargo build -p flutterdec-cli --release`

See: `.github/workflows/ci.yml`

Issue and PR templates live under `.github/` and should be used for all external contributions.
Repository defaults also include `.github/CODEOWNERS` for review routing and `.github/dependabot.yml` for weekly Cargo/Actions update PRs.

## Contribution Conventions

- keep commits atomic
- use commit format: `type(scope): description`
- keep docs updated when behavior changes
- avoid broad refactors mixed with feature logic in a single commit

## Related Docs

- internals: [how-it-works.md](how-it-works.md)
- architecture: [architecture.md](architecture.md)
- research constraints: [research-decisions.md](research-decisions.md)
- project context/history: [../context.md](../context.md)
