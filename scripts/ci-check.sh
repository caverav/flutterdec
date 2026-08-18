#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  scripts/ci-check.sh [--skip-tests]

Runs the same checks as CI from the local workspace:
  1) nix flake check
  2) cargo fmt --all --check
  3) scripts/lint-shell.sh
  4) scripts/lint-python.sh
  5) cargo clippy --workspace --all-targets -- -D warnings
  6) cargo test --workspace            (unless --skip-tests)
  7) cargo build -p flutterdec-cli --release
  8) fmt, clippy and tests for the excluded benchmark harness

The benchmark harness is not a workspace member, so --workspace does not reach
it and it is linted and tested through its own manifest. That exclusion is what
keeps its `bench-spans` instrumentation out of every check above.
EOF
}

skip_tests="0"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-tests)
      skip_tests="1"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "$repo_root"

echo "[ci-check] nix flake check"
nix flake check

echo "[ci-check] cargo fmt --all --check"
nix develop -c cargo fmt --all --check

echo "[ci-check] scripts/lint-shell.sh"
nix develop -c ./scripts/lint-shell.sh

echo "[ci-check] scripts/lint-python.sh"
nix develop -c ./scripts/lint-python.sh

echo "[ci-check] cargo clippy --workspace --all-targets -- -D warnings"
nix develop -c cargo clippy --workspace --all-targets -- -D warnings

if [[ "$skip_tests" != "1" ]]; then
  echo "[ci-check] cargo test --workspace"
  nix develop -c cargo test --workspace
fi

echo "[ci-check] cargo build -p flutterdec-cli --release"
nix develop -c cargo build -p flutterdec-cli --release

bench_manifest="crates/flutterdec-bench/Cargo.toml"
# `--all` means every member of the manifest's own workspace, and the harness is
# its own workspace, so the root `cargo fmt --all` above does not reach it.
echo "[ci-check] cargo fmt --manifest-path ${bench_manifest} --all --check"
nix develop -c cargo fmt --manifest-path "$bench_manifest" --all --check

echo "[ci-check] cargo clippy --manifest-path ${bench_manifest} --all-targets -- -D warnings"
nix develop -c cargo clippy --manifest-path "$bench_manifest" --all-targets -- -D warnings

if [[ "$skip_tests" != "1" ]]; then
  echo "[ci-check] cargo test --manifest-path ${bench_manifest}"
  nix develop -c cargo test --manifest-path "$bench_manifest"
fi

echo "[ci-check] all checks passed"
