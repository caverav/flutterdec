#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  scripts/ci-check.sh [--skip-tests]

Runs the same checks as CI from the local workspace:
  1) cargo fmt --all --check
  2) cargo clippy --workspace --all-targets -- -D warnings
  3) cargo test --workspace            (unless --skip-tests)
  4) cargo build -p flutterdec-cli --release
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

echo "[ci-check] cargo fmt --all --check"
nix develop -c cargo fmt --all --check

echo "[ci-check] cargo clippy --workspace --all-targets -- -D warnings"
nix develop -c cargo clippy --workspace --all-targets -- -D warnings

if [[ "$skip_tests" != "1" ]]; then
  echo "[ci-check] cargo test --workspace"
  nix develop -c cargo test --workspace
fi

echo "[ci-check] cargo build -p flutterdec-cli --release"
nix develop -c cargo build -p flutterdec-cli --release

echo "[ci-check] all checks passed"

