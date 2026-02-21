# Contributing

## Setup

```bash
nix develop
```

## Before Opening a PR

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

For decompiler behavior changes, validate with real binaries using:

- `scripts/real-golden.sh`
- `scripts/real-golden-matrix.sh`

## Commit Style

Use atomic commits with format:

- `type(scope): description`

Examples:

- `feat(decompiler): map internal selector to stdlib name`
- `docs(readability): update selector mapping docs`
- `ci(workflows): add pull request rust checks`

## PR Expectations

- include tests for behavior changes
- keep user docs and development docs updated
- avoid mixing unrelated refactors with feature changes
