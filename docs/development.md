# Development Guide

## Toolchain

Use Nix shell for reproducible builds:

```bash
nix develop
```

`nix develop` also exports `FLUTTERDEC_BLUTTER_CMD` to a Nix-managed `flutterdec-blutter`
wrapper, so `--adapter-backend blutter` can run without manual Blutter path wiring.

The r2flutter backend is not bundled. Build it once against radare2 and point
`FLUTTERDEC_R2FLUTTER_BIN` at the result:

```bash
git clone https://github.com/radareorg/r2flutter && cd r2flutter
./configure --prefix="$HOME/.local" && make
export FLUTTERDEC_R2FLUTTER_BIN="$PWD/bin/r2flutter"
```

It is the only backend that yields exact function names and a real `ObjectPool` index
space, so it is worth having when working on the semantic/naming passes; the internal
adapter cannot exercise them.

Common commands:

```bash
nix flake check
cargo fmt --all --check
./scripts/lint-shell.sh
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
- baselines now also capture `report_metrics.json`, a focused subset of `report.json` covering startup evidence, bootflow coverage, bootstrap-chain source/path counts, manifest-anchored startup-path counts, entrypoint recovery, and engine-symbol-ingestion metrics

## Quality and Reports

`decompile` writes:

- `quality.json` (quality gate results)
- `report.json` (semantic/fallback/metadata summaries)

Analysis profile and resolved engine options are written into `report.json` under `analysis`.

Core quality KPIs used to evaluate changes:

- semantic rewrite rates: `semantic_direct_calls`, `semantic_indirect_calls`, and ratios
- unresolved fallback pressure: selector fallback and dynamic/indirect fallback summaries
- symbol quality: placeholder replacement progress and generic name prevalence
- app-focused selection quality in capped runs: selected scope mix and preferred-package ratios
- metadata coverage: pool value/semantic/target symbol counters
- reproducibility: stable output under repeated runs for same input/options

## CI

CI runs on:

- pull requests
- pushes to `main`
- Linux and macOS runners

Workflow checks, in order, all of them also run by `scripts/ci-check.sh` with the
same commands:

- `nix flake check`
- `cargo fmt --check`
- `scripts/lint-shell.sh`
- `scripts/lint-python.sh`
- `scripts/bench-identity-gate-test.sh`
- `cargo clippy` with warnings denied
- the protected decompiler, core and IR integration targets, by name
- `scripts/check-oracle-inventory.py`
- `scripts/check-resource-ruler.py`
- `cargo test --workspace`
- `cargo build -p flutterdec-cli --release`
- fmt, clippy and tests for the excluded benchmark harness, the tests on the
  Linux runner only because the harness reads its peak RSS from `/proc`

Local and GitHub CI enforce the same surfaces on purpose: a guard that only the
local script runs is a guard the next push can drop.
`the_protected_oracle_loader_chain_is_intact` in
`crates/flutterdec-decompiler/tests/provenance_audit.rs` fails if either lane
drops a checker command the other still runs, and section 24 of
`docs/oracle-protocol-ir-cfg-emitter.md` adjudicates that parity.

See: `.github/workflows/ci.yml`, `scripts/ci-check.sh`
Tag pushes matching `v*` build release CLI artifacts for Linux and macOS and publish them in a GitHub release via `.github/workflows/release.yml`.

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
