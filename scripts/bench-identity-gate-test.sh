#!/usr/bin/env bash
# Regression coverage for the pre-measurement identity gate.
#
# The gate is the only thing standing between a path-dependent build and an A/A
# baseline that reports build layout as a noise floor, so it is checked directly
# instead of only through a 6 minute pipeline run.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
gate="$script_dir/bench-identity-gate.sh"

failures=0
check() {
  local name="$1" want="$2"
  shift 2
  local got=0
  "$gate" "$@" >/dev/null 2>&1 || got=$?
  if [[ "$got" != "$want" ]]; then
    echo "[identity-gate-test] FAIL ${name}: expected exit ${want}, got ${got}"
    failures=$((failures + 1))
  else
    echo "[identity-gate-test] ok ${name}"
  fi
}

aa="1371e42549472ec388f58bc1fd5dbdf96e8dcdd1"
bb="209a8fe0000000000000000000000000000000ab"
d1="664a414c0cd2bba8b0e8acf5da00e633308cf5249f6bc4d9115a837c9ba1626c"
d2="7e0d6a11655e5128933aa7c57c6bab9634d2c1fd6f867217c360758f6363af15"

check "A/A with identical binaries is allowed"          0 "$aa" "$aa" "$d1" "$d1"
check "A/A with different binaries aborts"              1 "$aa" "$aa" "$d1" "$d2"
check "A/A with reversed digest mismatch also aborts"   1 "$aa" "$aa" "$d2" "$d1"
check "A/B with different binaries is allowed"          0 "$aa" "$bb" "$d1" "$d2"
check "A/B with identical binaries is allowed"          0 "$aa" "$bb" "$d1" "$d1"
check "missing argument is a usage error"               2 "$aa" "$aa" "$d1"
check "extra argument is a usage error"                 2 "$aa" "$aa" "$d1" "$d1" extra

if [[ "$failures" -ne 0 ]]; then
  echo "[identity-gate-test] ${failures} failure(s)" >&2
  exit 1
fi
echo "[identity-gate-test] all checks passed"
