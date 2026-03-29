#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/test-suite.sh

Runs the full Rust test suite from the workspace:
  1) cargo test --workspace --all-targets
  2) cargo test --workspace --release
USAGE
}

if [[ ${1:-} == "-h" || ${1:-} == "--help" ]]; then
  usage
  exit 0
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "$repo_root"

echo "[test-suite] cargo test --workspace --all-targets"
nix develop -c cargo test --workspace --all-targets

echo "[test-suite] cargo test --workspace --release"
nix develop -c cargo test --workspace --release

echo "[test-suite] all tests passed"
