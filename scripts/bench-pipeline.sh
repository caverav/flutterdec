#!/usr/bin/env bash
# Interleaved phase baseline for two product revisions under one harness.
#
# Both revisions are checked out into isolated worktrees and the same harness
# patch is applied to each, so the only thing that differs between the two
# binaries is product code. The patch digest is recorded on both sides: if it
# does not match, the comparison is void.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  scripts/bench-pipeline.sh --reference REF --candidate REF --patch FILE --out DIR
                            [--pairs N] [--warmups N] [--label TEXT]
                            [--matrix disclosed|held-out] [--held-out-seed HEX]

  --reference REF     Product revision measured as the baseline
  --candidate REF     Product revision measured against it. Pass the same
                      revision twice to measure the noise floor.
  --patch FILE        Harness patch applied byte-identically to both worktrees
  --out DIR           Output directory, created if absent
  --pairs N           Interleaved measured pairs (default 15)
  --warmups N         Unmeasured passes before each measured run (default 3)
  --label TEXT        Recorded in both result documents
  --matrix            Case set (default disclosed)
  --held-out-seed HEX 128-bit hex seed, required for --matrix held-out

Every cargo and nix invocation runs inside the flake development shell.
EOF
}

reference=""
candidate=""
patch_file=""
out_dir=""
pairs=15
warmups=3
label=""
matrix="disclosed"
held_out_seed=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --reference) reference="$2"; shift 2 ;;
    --candidate) candidate="$2"; shift 2 ;;
    --patch) patch_file="$2"; shift 2 ;;
    --out) out_dir="$2"; shift 2 ;;
    --pairs) pairs="$2"; shift 2 ;;
    --warmups) warmups="$2"; shift 2 ;;
    --label) label="$2"; shift 2 ;;
    --matrix) matrix="$2"; shift 2 ;;
    --held-out-seed) held_out_seed="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; usage; exit 1 ;;
  esac
done

for required in reference candidate patch_file out_dir; do
  if [[ -z "${!required}" ]]; then
    echo "Missing --${required//_file/}" >&2
    usage
    exit 1
  fi
done

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "$repo_root"

nix_flags=(--extra-experimental-features 'nix-command flakes')
mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd)"
patch_file="$(cd "$(dirname "$patch_file")" && pwd)/$(basename "$patch_file")"
patch_digest="$(sha256sum "$patch_file" | cut -d' ' -f1)"

matrix_args=(--matrix "$matrix")
if [[ "$matrix" == "held-out" ]]; then
  if [[ -z "$held_out_seed" ]]; then
    echo "--matrix held-out needs --held-out-seed" >&2
    exit 1
  fi
  matrix_args+=(--held-out-seed "$held_out_seed")
fi

# One worktree per side, detached at the product revision, with the harness
# patch applied on top. `git apply` fails loudly on any drift, which is the
# check that the two sides really are running the same harness.
prepare() {
  local side="$1" revision="$2" tree="$3"
  echo "[bench] preparing $side at $revision"
  rm -rf "$tree"
  git worktree add --detach "$tree" "$revision" >/dev/null
  git -C "$tree" apply --whitespace=nowarn "$patch_file"
  echo "[bench] $side product revision: $(git -C "$tree" rev-parse HEAD)"
}

work_root="$out_dir/worktrees"
mkdir -p "$work_root"
reference_tree="$work_root/reference"
candidate_tree="$work_root/candidate"
prepare reference "$reference" "$reference_tree"
prepare candidate "$candidate" "$candidate_tree"

# The harness is not a workspace member, so it is built through its own manifest
# and lands in its own target directory. Building it with `-p` from the workspace
# root would not resolve it at all, and making it a member would turn
# `bench-spans` on for every product build in the workspace.
bench_manifest="crates/flutterdec-bench/Cargo.toml"
build() {
  local tree="$1"
  ( cd "$tree" && nix develop "${nix_flags[@]}" -c \
      cargo build --manifest-path "$bench_manifest" --release >/dev/null )
}

echo "[bench] building both revisions"
build "$reference_tree"
build "$candidate_tree"

reference_bin="$reference_tree/crates/flutterdec-bench/target/release/flutterdec-bench"
candidate_bin="$candidate_tree/crates/flutterdec-bench/target/release/flutterdec-bench"
for binary in "$reference_bin" "$candidate_bin"; do
  if [[ ! -x "$binary" ]]; then
    echo "[bench] expected harness binary missing: $binary" >&2
    exit 1
  fi
done
reference_binary_digest="$(sha256sum "$reference_bin" | cut -d' ' -f1)"
candidate_binary_digest="$(sha256sum "$candidate_bin" | cut -d' ' -f1)"
reference_head="$(git -C "$reference_tree" rev-parse HEAD)"
candidate_head="$(git -C "$candidate_tree" rev-parse HEAD)"
harness_head="$(git rev-parse HEAD)"

# The matrix each side will run, before any timing. Different digests here mean
# the two binaries would not be measured on the same work.
"$reference_bin" manifest "${matrix_args[@]}" --out "$out_dir/manifest-reference.json"
"$candidate_bin" manifest "${matrix_args[@]}" --out "$out_dir/manifest-candidate.json"
if ! diff -q "$out_dir/manifest-reference.json" "$out_dir/manifest-candidate.json" >/dev/null; then
  echo "[bench] workload manifests differ between revisions; comparison is void" >&2
  exit 1
fi

measure() {
  local side="$1" binary="$2" product="$3" binary_digest="$4" pair="$5"
  "$binary" run \
    "${matrix_args[@]}" \
    --warmups "$warmups" \
    --runs 1 \
    --product-ref "$product" \
    --harness-ref "$harness_head" \
    --patch-sha256 "$patch_digest" \
    --binary-sha256 "$binary_digest" \
    --label "${label}${label:+ }${side} pair ${pair}" \
    --out "$out_dir/raw/${side}-${pair}.json" \
    --samples "$out_dir/raw/${side}-${pair}.tsv"
}

mkdir -p "$out_dir/raw"
echo "[bench] $pairs interleaved pairs, $warmups warmups before each measured run"
for (( pair = 0; pair < pairs; pair++ )); do
  measure reference "$reference_bin" "$reference_head" "$reference_binary_digest" "$pair"
  measure candidate "$candidate_bin" "$candidate_head" "$candidate_binary_digest" "$pair"
  echo "[bench] pair $((pair + 1))/$pairs done"
done

# Each measured run wrote run index 0, so the pair number becomes the run index
# the aggregator pairs on.
collect() {
  local side="$1" target="$2"
  head -n 1 "$out_dir/raw/${side}-0.tsv" > "$target"
  for (( pair = 0; pair < pairs; pair++ )); do
    tail -n +2 "$out_dir/raw/${side}-${pair}.tsv" | awk -v run="$pair" 'BEGIN{OFS="\t"} {$1=run; print}'
  done >> "$target"
}

collect reference "$out_dir/samples-reference.tsv"
collect candidate "$out_dir/samples-candidate.tsv"

"$reference_bin" aggregate \
  --reference "$out_dir/samples-reference.tsv" \
  --candidate "$out_dir/samples-candidate.tsv" \
  --out "$out_dir/analysis.json"

cat > "$out_dir/binding.txt" <<EOF
harness_ref                $harness_head
patch_sha256               $patch_digest
reference_product_ref      $reference_head
candidate_product_ref      $candidate_head
reference_binary_sha256    $reference_binary_digest
candidate_binary_sha256    $candidate_binary_digest
matrix                     $matrix
pairs                      $pairs
warmups_per_measured_run   $warmups
EOF

echo "[bench] wrote $out_dir/analysis.json"
cat "$out_dir/binding.txt"
