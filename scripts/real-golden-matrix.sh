#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/real-golden-matrix.sh <record|check> [--profiles-root <dir>] [--profile <name>]... [--strict]

Options:
  --profiles-root <dir>  Root directory containing profile subdirectories (default: testdata/real-golden/profiles)
  --profile <name>       Run only a specific profile name (repeatable)
  --strict               Fail when a profile is skipped due to missing input, env var, or baseline artifacts

Profile format:
  Each profile directory should contain profile.env with shell-style KEY=VALUE entries.

  Supported keys:
    INPUT                    Absolute/relative path to APK or libapp.so
    INPUT_ENV                Env var name that contains INPUT path (used if INPUT is empty)
    MAX_FUNCTIONS            Optional (default: 120)
    MIN_DISASSEMBLY_RATIO    Optional (default: 0.0)
    FILES                    Optional path for files list passed to real-golden.sh --files
    BASELINE                 Optional baseline directory (default: profile directory)

Examples:
  scripts/real-golden-matrix.sh check
  scripts/real-golden-matrix.sh check --profile sample
  scripts/real-golden-matrix.sh check --strict
USAGE
}

if [[ $# -lt 1 ]]; then
  usage
  exit 1
fi

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

mode="$1"
shift

if [[ "$mode" != "record" && "$mode" != "check" ]]; then
  echo "Mode must be 'record' or 'check'." >&2
  usage
  exit 1
fi

profiles_root="testdata/real-golden/profiles"
strict="0"
selected_profiles=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profiles-root)
      profiles_root="${2:-}"
      shift 2
      ;;
    --profile)
      selected_profiles+=("${2:-}")
      shift 2
      ;;
    --strict)
      strict="1"
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
profiles_root_abs="${repo_root}/${profiles_root}"
real_golden_script="${repo_root}/scripts/real-golden.sh"

if [[ ! -d "$profiles_root_abs" ]]; then
  echo "[real-golden-matrix] profiles root does not exist: $profiles_root_abs" >&2
  exit 1
fi

profiles=()
if [[ ${#selected_profiles[@]} -gt 0 ]]; then
  for name in "${selected_profiles[@]}"; do
    profiles+=("$name")
  done
else
  while IFS= read -r -d '' env_file; do
    profiles+=("$(basename "$(dirname "$env_file")")")
  done < <(find "$profiles_root_abs" -mindepth 2 -maxdepth 2 -type f -name profile.env -print0 | sort -z)
fi

if [[ ${#profiles[@]} -eq 0 ]]; then
  echo "[real-golden-matrix] no profile.env files found under: $profiles_root_abs"
  exit 0
fi

passes=0
fails=0
skips=0

for name in "${profiles[@]}"; do
  profile_dir="$profiles_root_abs/$name"
  profile_env="$profile_dir/profile.env"

  if [[ ! -f "$profile_env" ]]; then
    echo "[real-golden-matrix] profile '$name' missing profile.env: $profile_env"
    skips=$((skips + 1))
    if [[ "$strict" == "1" ]]; then
      fails=$((fails + 1))
    fi
    continue
  fi

  INPUT=""
  INPUT_ENV=""
  MAX_FUNCTIONS="120"
  MIN_DISASSEMBLY_RATIO="0.0"
  FILES=""
  BASELINE=""

  # shellcheck disable=SC1090
  source "$profile_env"

  resolved_input="${INPUT:-}"
  if [[ -z "$resolved_input" && -n "${INPUT_ENV:-}" ]]; then
    resolved_input="${!INPUT_ENV:-}"
  fi

  if [[ -z "$resolved_input" ]]; then
    echo "[real-golden-matrix] skip '$name': no INPUT or resolved INPUT_ENV path"
    skips=$((skips + 1))
    if [[ "$strict" == "1" ]]; then
      fails=$((fails + 1))
    fi
    continue
  fi

  baseline_path="${BASELINE:-$profile_dir}"
  if [[ "$mode" == "check" && ! -f "$baseline_path/quality.json" ]]; then
    echo "[real-golden-matrix] skip '$name': missing baseline quality file: $baseline_path/quality.json"
    skips=$((skips + 1))
    if [[ "$strict" == "1" ]]; then
      fails=$((fails + 1))
    fi
    continue
  fi

  cmd=("$real_golden_script" "$mode" "--input" "$resolved_input" "--baseline" "$baseline_path" "--max-functions" "${MAX_FUNCTIONS:-120}" "--min-disassembly-ratio" "${MIN_DISASSEMBLY_RATIO:-0.0}")
  if [[ -n "${FILES:-}" ]]; then
    cmd+=("--files" "$FILES")
  fi

  echo "[real-golden-matrix] running '$name' (${mode})"
  if "${cmd[@]}"; then
    passes=$((passes + 1))
  else
    echo "[real-golden-matrix] profile '$name' failed"
    fails=$((fails + 1))
  fi
done

echo "[real-golden-matrix] summary: pass=$passes fail=$fails skip=$skips"
if [[ "$fails" -gt 0 ]]; then
  exit 1
fi

exit 0
