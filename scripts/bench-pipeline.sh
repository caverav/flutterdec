#!/usr/bin/env bash
# Interleaved phase baseline for two product revisions under one harness.
#
# Both revisions are built from the same harness patch, so the only thing that
# differs between the two binaries is product code. The patch digest is recorded
# on both sides: if it does not match, the comparison is void.
#
# The two builds run one after the other in the same canonical build path, and
# only the finished binaries are copied out into stable side slots. Building
# each side in its own worktree does not work: the absolute path enters the
# build, so one source revision built at two paths yields two different
# binaries. Measured on this repository, two same-source builds at different
# paths differ by thousands of bytes even with `--remap-path-prefix` pointing
# both at one virtual root, because the remap argument itself carries the path
# and cargo hashes RUSTFLAGS into the crate metadata. Rebuilt at one path in a
# fresh worktree, the binary is byte-identical.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  scripts/bench-pipeline.sh --reference REF --candidate REF --patch FILE --out DIR
                            [--pairs N] [--warmups N] [--label TEXT] [--clear]
                            [--matrix disclosed|held-out] [--held-out-seed HEX]
                            [--build-root DIR] [--build-only]

  --reference REF     Product revision measured as the baseline
  --candidate REF     Product revision measured against it. Pass the same
                      revision twice to measure the noise floor.
  --patch FILE        Harness patch applied byte-identically to both worktrees
  --out DIR           Output directory, created if absent. Held under an
                      exclusive lock for the whole run, and refused outright if
                      it already holds raw samples unless --clear is given.
  --pairs N           Interleaved measured pairs (default 15). Execution order
                      alternates inside the pair: reference first on even pair
                      indexes, candidate first on odd ones.
  --warmups N         Preliminary unmeasured warmup passes per binary, run once
                      before any measured pair (default 3). Every measured pair
                      itself runs at zero warmups.
  --clear             Delete an existing raw sample directory before measuring
  --label TEXT        Recorded in both result documents
  --matrix            Case set (default disclosed)
  --held-out-seed HEX 128-bit hex seed, required for --matrix held-out
  --build-root DIR    Canonical build path, held under its own exclusive lock.
                      Both revisions are built here in sequence, one at a time,
                      so neither binary carries a side-specific path. Defaults
                      to ${TMPDIR:-/tmp}/flutterdec-bench-build, which keeps the
                      binaries comparable across runs as well as within one.
  --build-only        Build both sides, report their digests, run the identity
                      gate and stop before any warmup. A cheap pre-flight for a
                      harness change, without spending a full measured run.

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
clear_raw="0"
build_root="${TMPDIR:-/tmp}/flutterdec-bench-build"
build_only="0"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --clear) clear_raw="1"; shift ;;
    --reference) reference="$2"; shift 2 ;;
    --candidate) candidate="$2"; shift 2 ;;
    --patch) patch_file="$2"; shift 2 ;;
    --out) out_dir="$2"; shift 2 ;;
    --pairs) pairs="$2"; shift 2 ;;
    --warmups) warmups="$2"; shift 2 ;;
    --label) label="$2"; shift 2 ;;
    --matrix) matrix="$2"; shift 2 ;;
    --held-out-seed) held_out_seed="$2"; shift 2 ;;
    --build-root) build_root="$2"; shift 2 ;;
    --build-only) build_only="1"; shift ;;
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

# One measurement per output directory at a time. Two pipelines sharing an
# output directory interleave their raw files and, worse, compete for the same
# CPUs, so every sample either side collects is contaminated. Non-blocking on
# purpose: a second run should fail immediately and visibly rather than queue up
# and start measuring later under conditions nobody recorded.
exec 9>"$out_dir/.bench-pipeline.lock"
if ! flock -n 9; then
  echo "[bench] another bench-pipeline run holds $out_dir/.bench-pipeline.lock; refusing to measure" >&2
  exit 1
fi
echo "[bench] holding exclusive lock on $out_dir (pid $$)"

# Raw samples from an earlier run are never mixed with this one's. Either the
# directory is empty, or it is emptied completely, or the run refuses: a partial
# overwrite would leave one side's pairs from an older binary in place.
raw_dir="$out_dir/raw"
if [[ -d "$raw_dir" ]] && [[ -n "$(ls -A "$raw_dir" 2>/dev/null)" ]]; then
  if [[ "$clear_raw" == "1" ]]; then
    echo "[bench] clearing existing raw samples in $raw_dir"
    rm -rf "$raw_dir"
  else
    echo "[bench] $raw_dir already holds raw samples; pass --clear to discard them" >&2
    exit 1
  fi
fi
mkdir -p "$raw_dir"

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

# The canonical build path is shared state across runs, so it takes its own
# exclusive lock. Two pipelines with different output directories would
# otherwise take turns replacing each other's worktree mid-build.
mkdir -p "$build_root"
build_root="$(cd "$build_root" && pwd)"
exec 8>"$build_root/.bench-build.lock"
if ! flock -n 8; then
  echo "[bench] another bench-pipeline run holds $build_root/.bench-build.lock; refusing to build" >&2
  exit 1
fi
echo "[bench] holding exclusive lock on canonical build path $build_root (pid $$)"

# The harness is not a workspace member, so it is built through its own manifest
# and lands in its own target directory. Building it with `-p` from the workspace
# root would not resolve it at all, and making it a member would turn
# `bench-spans` on for every product build in the workspace.
bench_manifest="crates/flutterdec-bench/Cargo.toml"

# One side at a time, always in the same directory, always from a fresh
# worktree, and the finished binary copied out before the next side moves in.
# The worktree is removed straight after the copy so a later run starts from the
# same empty path this one did, and so no worktree stays registered afterwards.
# `git apply` fails loudly on any drift, which is the check that the two sides
# really are running the same harness.
#
# Both slot directories are named at the same length and hold the same file
# name, so the two binaries are launched through paths that differ only in
# content. The bytes are identical by construction; the gate below proves it.
build_tree="$build_root/tree"
bin_root="$out_dir/bin"
rm -rf "$bin_root"
drop_tree() {
  git worktree remove --force "$build_tree" >/dev/null 2>&1 || true
  rm -rf "$build_tree"
  git worktree prune
}
build_side() {
  local side="$1" revision="$2"
  echo "[bench] building $side at $revision in $build_tree"
  drop_tree
  git worktree add --detach "$build_tree" "$revision" >/dev/null
  git -C "$build_tree" apply --whitespace=nowarn "$patch_file"
  local head
  head="$(git -C "$build_tree" rev-parse HEAD)"
  ( cd "$build_tree" && nix develop "${nix_flags[@]}" -c \
      cargo build --manifest-path "$bench_manifest" --release >/dev/null )
  local built="$build_tree/crates/flutterdec-bench/target/release/flutterdec-bench"
  if [[ ! -x "$built" ]]; then
    echo "[bench] expected harness binary missing: $built" >&2
    exit 1
  fi
  mkdir -p "$bin_root/$side"
  cp "$built" "$bin_root/$side/flutterdec-bench"
  printf '%s\n' "$head" > "$bin_root/$side/product-ref"
  drop_tree
  echo "[bench] $side product revision: $head"
}

build_side reference "$reference"
build_side candidate "$candidate"

reference_bin="$bin_root/reference/flutterdec-bench"
candidate_bin="$bin_root/candidate/flutterdec-bench"
reference_binary_digest="$(sha256sum "$reference_bin" | cut -d' ' -f1)"
candidate_binary_digest="$(sha256sum "$candidate_bin" | cut -d' ' -f1)"
reference_head="$(cat "$bin_root/reference/product-ref")"
candidate_head="$(cat "$bin_root/candidate/product-ref")"
harness_head="$(git rev-parse HEAD)"

# Hard gate, before any warmup. One product revision under one harness patch has
# to build to one binary; if it does not, every number the run would go on to
# print is build layout rather than product code.
"$script_dir/bench-identity-gate.sh" \
  "$reference_head" "$candidate_head" \
  "$reference_binary_digest" "$candidate_binary_digest"
identity_gate="not applicable: product commits differ"
if [[ "$reference_head" == "$candidate_head" ]]; then
  identity_gate="passed: equal product commits and equal binary sha256"
fi

if [[ "$build_only" == "1" ]]; then
  echo "[bench] build-only, stopping before warmup"
  echo "[bench] reference $reference_head $reference_binary_digest"
  echo "[bench] candidate $candidate_head $candidate_binary_digest"
  exit 0
fi

# The matrix each side will run, before any timing. Different digests here mean
# the two binaries would not be measured on the same work.
"$reference_bin" manifest "${matrix_args[@]}" --out "$out_dir/manifest-reference.json"
"$candidate_bin" manifest "${matrix_args[@]}" --out "$out_dir/manifest-candidate.json"
if ! diff -q "$out_dir/manifest-reference.json" "$out_dir/manifest-candidate.json" >/dev/null; then
  echo "[bench] workload manifests differ between revisions; comparison is void" >&2
  exit 1
fi

# Warmups are preliminary and per binary, not per pair. Re-warming before every
# one of the 30 measured passes spent four times the wall clock on warmups alone,
# which is what made a run long enough to overlap the next one. The measured
# pairs stay interleaved and their internal order alternates, so drift that does
# survive is charged to each side about equally rather than always to the same
# position.
#
# The correctness pass runs here and only here. It is a property of the binary,
# not of a pass: it emits every case twice to prove determinism, costs about
# twice a measured pass, and repeating it identically before each of the 30
# measured passes both quadrupled the run and put unrelated graph work in the
# cache between pairs. A failure exits non-zero, so no pair is ever measured
# against a binary whose cases did not check out.
warmup() {
  local side="$1" binary="$2" product="$3" binary_digest="$4"
  echo "[bench] warming $side with $warmups unmeasured pass(es) and the correctness pass"
  "$binary" run \
    "${matrix_args[@]}" \
    --warmups "$warmups" \
    --runs 0 \
    --correctness on \
    --product-ref "$product" \
    --harness-ref "$harness_head" \
    --patch-sha256 "$patch_digest" \
    --binary-sha256 "$binary_digest" \
    --label "${label}${label:+ }${side} warmup" \
    --out "$out_dir/warmup-${side}.json"
}

# `position` is `first` or `second` within the pair. It is appended to the order
# log here rather than by the loop, so the log records the order that actually
# executed, and it is carried in the run label so every raw document says where
# in its pair it was measured.
order_log="$raw_dir/pair-order.tsv"
measure() {
  local side="$1" binary="$2" product="$3" binary_digest="$4" pair="$5" position="$6"
  printf '%s\t%s\t%s\n' "$pair" "$position" "$side" >> "$order_log"
  "$binary" run \
    "${matrix_args[@]}" \
    --warmups 0 \
    --runs 1 \
    --correctness off \
    --product-ref "$product" \
    --harness-ref "$harness_head" \
    --patch-sha256 "$patch_digest" \
    --binary-sha256 "$binary_digest" \
    --label "${label}${label:+ }${side} pair ${pair} ${position}" \
    --out "$out_dir/raw/${side}-${pair}.json" \
    --samples "$out_dir/raw/${side}-${pair}.tsv"
}

warmup reference "$reference_bin" "$reference_head" "$reference_binary_digest"
warmup candidate "$candidate_bin" "$candidate_head" "$candidate_binary_digest"

# Execution order alternates inside every pair. Running the same side second
# every time is not a neutral schedule: an A/A run of two identical binaries at
# 1371e42 put 20 of the 24 cells whose absolute paired delta reached 2 percent in
# favour of whichever side went second, which is a position effect and not a
# product difference. Alternating charges that effect to reference on odd pairs
# and to candidate on even ones, so it cancels in the median paired delta instead
# of masquerading as one.
echo "[bench] $pairs interleaved pairs at zero per-pair warmups, alternating order"
for (( pair = 0; pair < pairs; pair++ )); do
  if (( pair % 2 == 0 )); then
    measure reference "$reference_bin" "$reference_head" "$reference_binary_digest" "$pair" first
    measure candidate "$candidate_bin" "$candidate_head" "$candidate_binary_digest" "$pair" second
  else
    measure candidate "$candidate_bin" "$candidate_head" "$candidate_binary_digest" "$pair" first
    measure reference "$reference_bin" "$reference_head" "$reference_binary_digest" "$pair" second
  fi
  echo "[bench] pair $((pair + 1))/$pairs done"
done

# The schedule restated independently and checked against what ran. Duplicating
# the rule is the point: if the loop above ever stops alternating, every delta
# silently carries the position effect again, so the run fails here rather than
# publishing numbers that look like a product difference.
expected_order=""
for (( pair = 0; pair < pairs; pair++ )); do
  if (( pair % 2 == 0 )); then
    expected_order+="${pair}"$'\t'"first"$'\t'"reference"$'\n'
    expected_order+="${pair}"$'\t'"second"$'\t'"candidate"$'\n'
  else
    expected_order+="${pair}"$'\t'"first"$'\t'"candidate"$'\n'
    expected_order+="${pair}"$'\t'"second"$'\t'"reference"$'\n'
  fi
done
if ! diff -u <(printf '%s' "$expected_order") "$order_log"; then
  echo "[bench] measured pair order is not the alternating schedule; comparison is void" >&2
  exit 1
fi
echo "[bench] pair order verified alternating across $pairs pairs"

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
canonical_build_path       $build_root/tree
build_order                sequential, one side at a time in the canonical path
identity_gate              $identity_gate
matrix                     $matrix
pairs                      $pairs
preliminary_warmups        $warmups
warmups_per_measured_pair  0
pair_order                 alternating: reference first on even pairs, candidate first on odd
pair_order_log             raw/pair-order.tsv
pair_order_verified        yes
correctness_documents      warmup-reference.json warmup-candidate.json
EOF

echo "[bench] wrote $out_dir/analysis.json"
cat "$out_dir/binding.txt"
