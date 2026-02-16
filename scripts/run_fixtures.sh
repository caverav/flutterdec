#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${ROOT}/build/src/flutterdec"
OUT="${ROOT}/out/fixtures"

if [[ ! -x "${BIN}" ]]; then
  echo "error: build binary missing at ${BIN}" >&2
  exit 1
fi

mkdir -p "${OUT}"

run_case() {
  local name="$1"
  local input="$2"
  if [[ ! -f "${input}" ]]; then
    echo "skip ${name}: missing fixture ${input}"
    return 0
  fi

  local case_out="${OUT}/${name}"
  rm -rf "${case_out}"
  "${BIN}" decompile "${input}" -o "${case_out}" --emit-asm --emit-ir || return 1

  local report="${case_out}/report.json"
  if [[ -f "${report}" ]]; then
    echo "summary ${name}:"
    jq '.counts' "${report}" || true
  fi
}

run_case unobf "${ROOT}/tests/fixtures/sample_app_unobf_arm64/libapp.so"
run_case obf "${ROOT}/tests/fixtures/sample_app_obf_arm64/libapp.so"
