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
      report_metrics.json
      files.txt
      <tracked output files>
  - `check` compares current outputs against baseline snapshots.
EOF
}

extract_report_metrics() {
  local report_path="$1"
  local metrics_path="$2"
  jq '{
    android_startup: {
      present: .android_startup.present,
      confidence: .android_startup.confidence,
      flutter_activity_count: .android_startup.flutter_activity_count,
      startup_method_count: .android_startup.startup_method_count,
      dart_entrypoint_count: .android_startup.dart_entrypoint_count,
      literal_entrypoint_count: (.android_startup.dart_entrypoints | map(select(.function_name != null or .library_uri != null or .app_bundle_path != null)) | length),
      bootstrap_chain_complete: .android_startup.bootstrap_chain.complete,
      bootstrap_chain_source_count: .android_startup.bootstrap_chain.source_count
    },
    bootflow: {
      main_count: .bootflow_discovery.main_count,
      runapp_count: .bootflow_discovery.runapp_count,
      deeplink_count: .bootflow_discovery.deeplink_count,
      activity_count: .bootflow_discovery.activity_count,
      bootstrap_count: .bootflow_discovery.bootstrap_count,
      selected_bootflow_coverage: .prioritization.selected_bootflow_coverage
    },
    engine_symbols: {
      enabled: .engine_symbol_ingestion.enabled,
      match_kind: .engine_symbol_ingestion.match_kind,
      applied_target_count: .engine_symbol_ingestion.applied_target_count,
      external_name_count: .name_resolution.final_quality.external,
      exact_name_count: .name_resolution.final_quality.exact
    }
  }' "$report_path" > "$metrics_path"
}

print_metrics_summary() {
  local metrics_path="$1"
  jq -r '"[real-golden] metrics: startup.present=\(.android_startup.present) entrypoints=\(.android_startup.dart_entrypoint_count) literal_entrypoints=\(.android_startup.literal_entrypoint_count) bootstrap_sources=\(.android_startup.bootstrap_chain_source_count) bootflow.any.coverage=\(.bootflow.selected_bootflow_coverage.any.coverage) engine_symbols.match=\(.engine_symbols.match_kind // "-") engine_symbols.applied=\(.engine_symbols.applied_target_count) names.external=\(.engine_symbols.external_name_count) names.exact=\(.engine_symbols.exact_name_count)"' "$metrics_path"
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

if [[ ! -f "$tmp_out/report.json" ]]; then
  echo "[real-golden] missing report.json in run output: $tmp_out/report.json" >&2
  exit 1
fi
extract_report_metrics "$tmp_out/report.json" "$tmp_out/report_metrics.json"
print_metrics_summary "$tmp_out/report_metrics.json"

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
  cp "$tmp_out/report_metrics.json" "$baseline/report_metrics.json"
  if [[ "$files_list" != "$baseline/files.txt" ]]; then
    cp "$files_list" "$baseline/files.txt"
  fi

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
if [[ ! -f "$baseline/report_metrics.json" ]]; then
  echo "[real-golden] baseline metrics file missing: $baseline/report_metrics.json" >&2
  exit 1
fi
if [[ ! -f "$files_list" ]]; then
  echo "[real-golden] files list missing: $files_list" >&2
  exit 1
fi

echo "[real-golden] comparing quality.json"
diff -u "$baseline/quality.json" "$tmp_out/quality.json"

echo "[real-golden] comparing report_metrics.json"
diff -u "$baseline/report_metrics.json" "$tmp_out/report_metrics.json"

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
