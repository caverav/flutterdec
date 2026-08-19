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
  5) scripts/bench-identity-gate-test.sh
  6) cargo clippy --workspace --all-targets -- -D warnings
  7) the protected oracle integration targets, by crate and name
  8) scripts/check-oracle-inventory.py
  9) scripts/check-resource-ruler.py
 10) cargo test --workspace            (unless --skip-tests)
 11) cargo build -p flutterdec-cli --release
 12) fmt, clippy and tests for the excluded benchmark harness

The benchmark harness is not a workspace member, so --workspace does not reach
it and it is linted and tested through its own manifest. That exclusion is what
keeps its `bench-spans` instrumentation out of every check above.

Step 7 names every protected integration test target explicitly and runs even
under --skip-tests. `cargo test --workspace` cannot protect them: with
`autotests = false`, or with any of the files deleted, it reports a smaller suite
and still exits 0. Naming the targets turns both into a hard error.

Step 8 also runs under --skip-tests, and it is the correctness oracle for whether
a protected oracle file is compiled at all. It lists every protected test target
and requires one sentinel test per file that section 7 protects. Source text
cannot answer that question: a comment, a `cfg` that is never true, or a macro
that swallows its argument leaves a loader hook byte-identical while removing it
from the build.
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

echo "[ci-check] scripts/bench-identity-gate-test.sh"
./scripts/bench-identity-gate-test.sh

echo "[ci-check] cargo clippy --workspace --all-targets -- -D warnings"
nix develop -c cargo clippy --workspace --all-targets -- -D warnings

# Named targets, not --workspace: `autotests = false` or a deleted file would
# leave --workspace passing with a quietly smaller suite.
echo "[ci-check] protected decompiler integration targets"
nix develop -c cargo test -p flutterdec-decompiler --test provenance_audit --test loop_entry_provenance_audit --test arm64_control_effects --test cfg_identity --test helper_syntax_boundaries --test rewrite_boundaries --test unmodelled_write_effects --test register_width_provenance --test atomic_rmw_effects --test annotation_anchor_identity --test provenance_accounting

echo "[ci-check] protected core integration targets"
nix develop -c cargo test -p flutterdec-core --test pipeline_determinism

echo "[ci-check] protected IR integration targets"
nix develop -c cargo test -p flutterdec-ir --test branch_target_radix

# The compiled inventory, not the loader source text: this is what fails when a
# protected oracle stops being compiled while its digest still matches.
echo "[ci-check] scripts/check-oracle-inventory.py"
nix develop -c python3 scripts/check-oracle-inventory.py

echo "[ci-check] scripts/check-resource-ruler.py"
nix develop -c python3 scripts/check-resource-ruler.py

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
