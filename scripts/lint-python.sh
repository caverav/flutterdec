#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "$repo_root"

mapfile -d '' -t files < <(find . -path ./.git -prune -o -type f -name "*.py" -print0 | sort -z)
if [[ ${#files[@]} -eq 0 ]]; then
  echo "[lint-python] no Python files found"
  exit 0
fi

cache_dir="$(mktemp -d)"
trap 'rm -rf "$cache_dir"' EXIT

echo "[lint-python] compiling ${#files[@]} Python file(s)"
PYTHONPYCACHEPREFIX="$cache_dir" python3 -m py_compile "${files[@]}"

echo "[lint-python] scripts/check-candidate-whitelist.py --self-test"
python3 scripts/check-candidate-whitelist.py --self-test

echo "[lint-python] scripts/prov_cross_audit_reconcile.py --self-test"
python3 scripts/prov_cross_audit_reconcile.py --self-test

echo "[lint-python] scripts/prov_join_audit_plant_test.py"
python3 scripts/prov_join_audit_plant_test.py

annotation_sample="$cache_dir/annotation-sample.dart"
printf 'x /* = 1 */\n' >"$annotation_sample"
echo "[lint-python] scripts/scan_annotation_safety_plant_test.py"
python3 scripts/scan_annotation_safety_plant_test.py "$annotation_sample"

echo "[lint-python] ok"
