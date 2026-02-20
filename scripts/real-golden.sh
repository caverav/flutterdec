#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  scripts/real-golden.sh record --input <apk|so> --baseline <dir> [--max-functions <n>] [--min-disassembly-ratio <r>] [--files <path>]
  scripts/real-golden.sh check  --input <apk|so> --baseline <dir> [--max-functions <n>] [--min-disassembly-ratio <r>] [--files <path>]

Environment:
  FLUTTERDEC_REAL_GOLDEN_FILES
    Comma-separated relative output paths used only by `record` when files list does not exist.
    Example:
      FLUTTERDEC_REAL_GOLDEN_FILES='pseudocode/00080_sub_65f850.dartpseudo,pseudocode/00081_sub_65f9ac.dartpseudo'

Notes:
  - Baseline directory stores:
      quality.json
      files.txt
      <tracked output files>
  - `check` compares current outputs against baseline snapshots.
EOF
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

input=""
baseline=""
files_list=""
max_functions="120"
min_ratio="0.0"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --input)
      input="${2:-}"
      shift 2
      ;;
    --baseline)
      baseline="${2:-}"
      shift 2
      ;;
    --files)
      files_list="${2:-}"
      shift 2
      ;;
    --max-functions)
      max_functions="${2:-}"
      shift 2
      ;;
    --min-disassembly-ratio)
      min_ratio="${2:-}"
      shift 2
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

if [[ "$mode" != "record" && "$mode" != "check" ]]; then
  echo "Mode must be 'record' or 'check'." >&2
  usage
  exit 1
fi

if [[ -z "$input" || -z "$baseline" ]]; then
  echo "--input and --baseline are required." >&2
  usage
  exit 1
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

if [[ -z "$files_list" ]]; then
  files_list="${baseline}/files.txt"
fi

tmp_out="$(mktemp -d "${TMPDIR:-/tmp}/flutterdec-real-golden.XXXXXX")"
cleanup() {
  rm -rf "$tmp_out"
}
trap cleanup EXIT

cd "$repo_root"

echo "[real-golden] decompiling input into temporary output: $tmp_out"
nix develop -c cargo run -q -p flutterdec-cli -- decompile \
  "$input" \
  -o "$tmp_out" \
  --max-functions "$max_functions" \
  --min-disassembly-ratio "$min_ratio" >/dev/null

if [[ "$mode" == "record" ]]; then
  mkdir -p "$baseline"

  if [[ ! -f "$files_list" ]]; then
    if [[ -n "${FLUTTERDEC_REAL_GOLDEN_FILES:-}" ]]; then
      echo "[real-golden] creating files list at $files_list from FLUTTERDEC_REAL_GOLDEN_FILES"
      mkdir -p "$(dirname "$files_list")"
      printf '%s\n' "$FLUTTERDEC_REAL_GOLDEN_FILES" | tr ',' '\n' | sed '/^\s*$/d' > "$files_list"
    else
      echo "[real-golden] files list not found: $files_list" >&2
      echo "[real-golden] provide --files or set FLUTTERDEC_REAL_GOLDEN_FILES" >&2
      exit 1
    fi
  fi

  cp "$tmp_out/quality.json" "$baseline/quality.json"
  cp "$files_list" "$baseline/files.txt"

  while IFS= read -r rel || [[ -n "$rel" ]]; do
    rel="$(echo "$rel" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
    [[ -z "$rel" ]] && continue
    src="$tmp_out/$rel"
    dst="$baseline/$rel"
    if [[ ! -f "$src" ]]; then
      echo "[real-golden] missing output file in run: $src" >&2
      exit 1
    fi
    mkdir -p "$(dirname "$dst")"
    cp "$src" "$dst"
  done < "$files_list"

  echo "[real-golden] baseline recorded at: $baseline"
  exit 0
fi

if [[ ! -f "$baseline/quality.json" ]]; then
  echo "[real-golden] baseline quality file missing: $baseline/quality.json" >&2
  exit 1
fi
if [[ ! -f "$files_list" ]]; then
  echo "[real-golden] files list missing: $files_list" >&2
  exit 1
fi

echo "[real-golden] comparing quality.json"
diff -u "$baseline/quality.json" "$tmp_out/quality.json"

while IFS= read -r rel || [[ -n "$rel" ]]; do
  rel="$(echo "$rel" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
  [[ -z "$rel" ]] && continue
  base_file="$baseline/$rel"
  run_file="$tmp_out/$rel"
  if [[ ! -f "$base_file" ]]; then
    echo "[real-golden] baseline file missing: $base_file" >&2
    exit 1
  fi
  if [[ ! -f "$run_file" ]]; then
    echo "[real-golden] run file missing: $run_file" >&2
    exit 1
  fi
  echo "[real-golden] comparing $rel"
  diff -u "$base_file" "$run_file"
done < "$files_list"

echo "[real-golden] check passed"
