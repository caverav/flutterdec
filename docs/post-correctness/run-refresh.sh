#!/usr/bin/env bash
# Measure the fixed pre-correctness scoring reference against the exact
# post-correctness product reference under the byte-identical accepted harness.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 OUT_DIR" >&2
  exit 2
fi

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out="$(mkdir -p "$1" && cd "$1" && pwd)"
build_root="${TMPDIR:-/tmp}/flutterdec-post-correctness-build"
tree="$build_root/tree"
patch="$repo/docs/baseline/harness-8e7f080.patch"
reference="1371e42549472ec388f58bc1fd5dbdf96e8dcdd1"
candidate="5ba4b6d30604606c04b5b742eaf9469adc1c729d"
harness="4c127aba4e74fb6f8d486c4cb066586bb0d74846"
resource_harness="b0e615785b28e7e58aa06dd1b929dd58acf06e53"
resource_guard="f4eb2d87d0bc8649addce2a2522ee8a56e856ca1"
resource_parent="$(git -C "$repo" rev-parse "$resource_harness^")"
harness_tree="$(git -C "$repo" rev-parse "$harness:crates/flutterdec-bench")"
resource_overlay_paths=(crates/flutterdec-bench/src/main.rs crates/flutterdec-bench/src/measure.rs crates/flutterdec-decompiler/src/lib.rs crates/flutterdec-decompiler/src/control_flow/structured.rs)
nix_flags=(--extra-experimental-features 'nix-command flakes')

exec 9>"$out/.lock"
flock -n 9 || { echo "refresh already running in $out" >&2; exit 1; }
rm -rf "${out:?}/raw" "${out:?}/bin" "${out:?}/resource"
rm -f "$out"/{SHA256SUMS,analysis.json,attribution.txt,audit-live.txt,binding.txt,chronology.tsv,manifest-candidate.json,manifest-reference.json,pair-order.tsv,planned-pair-order.tsv,samples-candidate.tsv,samples-reference.tsv,warmup-candidate.json,warmup-reference.json}
mkdir -p "$out/raw" "$out/resource" "$out/bin/timing/reference" "$out/bin/timing/candidate" "$out/bin/resource/reference" "$out/bin/resource/candidate" "$build_root"
resource_patch="$build_root/resource-overlay.patch"
git -C "$repo" diff "$resource_parent" "$resource_harness" -- "${resource_overlay_paths[@]}" >"$resource_patch"
cp "$resource_patch" "$out/resource-overlay.patch"
: >"$out/chronology.tsv"
printf 'sequence\tkind\tside\tpair\tposition\tstart_epoch_ns\tend_epoch_ns\tstart_iso\tend_iso\tartifact\n' >"$out/chronology.tsv"
started_at="$(date --iso-8601=seconds)"

drop_tree() {
  git -C "$repo" worktree remove --force "$tree" >/dev/null 2>&1 || true
  rm -rf "$tree"
  git -C "$repo" worktree prune
}
trap drop_tree EXIT

build() {
  local kind="$1" side="$2" revision="$3" mode="$4"
  local actual_harness_tree staged_root path blob
  drop_tree
  git -C "$repo" worktree add --detach "$tree" "$revision" >/dev/null
  if [[ "$mode" == patch ]]; then
    git -C "$tree" apply --whitespace=nowarn "$patch"
  fi
  rm -rf "$tree/crates/flutterdec-bench"
  git -C "$repo" archive "$harness" crates/flutterdec-bench | tar -x -C "$tree"
  git -C "$tree" add -f crates/flutterdec-bench
  staged_root="$(git -C "$tree" write-tree)"
  actual_harness_tree="$(git -C "$tree" rev-parse "$staged_root:crates/flutterdec-bench")"
  if [[ "$actual_harness_tree" != "$harness_tree" ]]; then
    echo "accepted harness tree mismatch: $actual_harness_tree != $harness_tree" >&2
    exit 1
  fi
  if [[ "$kind" == resource ]]; then
    if [[ "$mode" == patch ]]; then
      git -C "$tree" apply --whitespace=nowarn "$resource_patch"
    else
      git -C "$tree" apply --whitespace=nowarn \
        --include=crates/flutterdec-bench/src/main.rs \
        --include=crates/flutterdec-bench/src/measure.rs "$resource_patch"
    fi
    for path in crates/flutterdec-bench/src/main.rs crates/flutterdec-bench/src/measure.rs; do
      blob="$(git -C "$repo" rev-parse "$resource_harness:$path")"
      [[ "$(git -C "$tree" hash-object "$path")" == "$blob" ]] || exit 1
    done
  fi
  (cd "$tree" && env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS \
    CARGO_INCREMENTAL=0 nix develop "${nix_flags[@]}" -c \
    cargo build --manifest-path crates/flutterdec-bench/Cargo.toml --release >/dev/null)
  cp "$tree/crates/flutterdec-bench/target/release/flutterdec-bench" \
    "$out/bin/$kind/$side/flutterdec-bench"
  drop_tree
}

build timing reference "$reference" patch
build timing candidate "$candidate" materialized
build resource reference "$reference" patch
build resource candidate "$candidate" materialized

ref_bin="$out/bin/timing/reference/flutterdec-bench"
cand_bin="$out/bin/timing/candidate/flutterdec-bench"
patch_sha="$(sha256sum "$patch" | cut -d' ' -f1)"
ref_sha="$(sha256sum "$ref_bin" | cut -d' ' -f1)"
cand_sha="$(sha256sum "$cand_bin" | cut -d' ' -f1)"
resource_overlay_sha="$(sha256sum "$resource_patch" | cut -d' ' -f1)"
resource_ref_sha="$(sha256sum "$out/bin/resource/reference/flutterdec-bench" | cut -d' ' -f1)"
resource_cand_sha="$(sha256sum "$out/bin/resource/candidate/flutterdec-bench" | cut -d' ' -f1)"
matrix_args=(--matrix disclosed)
sequence=0

record_interval() {
  local kind="$1" side="$2" pair="$3" position="$4" start_ns="$5" start_iso="$6" artifact="$7"
  local end_ns end_iso
  end_ns="$(date +%s%N)"
  end_iso="$(date --iso-8601=ns)"
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$sequence" "$kind" "$side" "$pair" "$position" "$start_ns" "$end_ns" \
    "$start_iso" "$end_iso" "$artifact" >>"$out/chronology.tsv"
  sequence=$((sequence + 1))
}

run_nix() {
  (cd "$repo" && env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS \
    CARGO_INCREMENTAL=0 nix develop "${nix_flags[@]}" -c "$@")
}

run_nix "$ref_bin" manifest "${matrix_args[@]}" --out "$out/manifest-reference.json"
run_nix "$cand_bin" manifest "${matrix_args[@]}" --out "$out/manifest-candidate.json"
diff -u "$out/manifest-reference.json" "$out/manifest-candidate.json"

for ((pair = 0; pair < 15; pair++)); do
  if ((pair % 2 == 0)); then
    printf '%s\t%s\t%s\n' "$pair" first reference
    printf '%s\t%s\t%s\n' "$pair" second candidate
  else
    printf '%s\t%s\t%s\n' "$pair" first candidate
    printf '%s\t%s\t%s\n' "$pair" second reference
  fi
done > "$out/planned-pair-order.tsv"

warm() {
  local side="$1" binary="$2" product="$3" digest="$4"
  local start_ns start_iso artifact="warmup-$side.json"
  start_ns="$(date +%s%N)"; start_iso="$(date --iso-8601=ns)"
  run_nix "$binary" run "${matrix_args[@]}" --warmups 3 --runs 0 --correctness on \
    --product-ref "$product" --harness-ref "$harness" \
    --patch-sha256 "$patch_sha" --binary-sha256 "$digest" \
    --label "post-correctness refresh $side warmup" \
    --out "$out/$artifact"
  record_interval warmup "$side" - - "$start_ns" "$start_iso" "$artifact"
}

measure() {
  local side="$1" binary="$2" product="$3" digest="$4" pair="$5" position="$6"
  local start_ns start_iso artifact="raw/$side-$pair.json"
  printf '%s\t%s\t%s\n' "$pair" "$position" "$side" >> "$out/pair-order.tsv"
  start_ns="$(date +%s%N)"; start_iso="$(date --iso-8601=ns)"
  run_nix "$binary" run "${matrix_args[@]}" --warmups 0 --runs 1 --correctness off \
    --product-ref "$product" --harness-ref "$harness" \
    --patch-sha256 "$patch_sha" --binary-sha256 "$digest" \
    --label "post-correctness refresh $side pair $pair $position" \
    --out "$out/raw/$side-$pair.json" --samples "$out/raw/$side-$pair.tsv"
  record_interval measured "$side" "$pair" "$position" "$start_ns" "$start_iso" "$artifact"
}

warm reference "$ref_bin" "$reference" "$ref_sha"
warm candidate "$cand_bin" "$candidate" "$cand_sha"

for ((pair = 0; pair < 15; pair++)); do
  if ((pair % 2 == 0)); then
    measure reference "$ref_bin" "$reference" "$ref_sha" "$pair" first
    measure candidate "$cand_bin" "$candidate" "$cand_sha" "$pair" second
  else
    measure candidate "$cand_bin" "$candidate" "$cand_sha" "$pair" first
    measure reference "$ref_bin" "$reference" "$ref_sha" "$pair" second
  fi
  echo "[refresh] pair $((pair + 1))/15"
done
diff -u "$out/planned-pair-order.tsv" "$out/pair-order.tsv"

resource() {
  local side="$1" product="$2" digest="$3"
  local binary="$out/bin/resource/$side/flutterdec-bench"
  local start_ns start_iso artifact="resource/$side.json"
  start_ns="$(date +%s%N)"; start_iso="$(date --iso-8601=ns)"
  run_nix "$binary" resource "${matrix_args[@]}" --warmups 3 --plant none \
    --product-ref "$product" --harness-ref "$resource_harness" \
    --patch-sha256 "$resource_overlay_sha" --binary-sha256 "$digest" \
    --label "post-correctness refresh resource $side" \
    --out "$out/$artifact" --samples "$out/resource/$side.tsv"
  record_interval resource "$side" - - "$start_ns" "$start_iso" "$artifact"
}

resource reference "$reference" "$resource_ref_sha"
resource candidate "$candidate" "$resource_cand_sha"

collect() {
  local side="$1" target="$2"
  head -n 1 "$out/raw/$side-0.tsv" > "$target"
  for ((pair = 0; pair < 15; pair++)); do
    tail -n +2 "$out/raw/$side-$pair.tsv" | \
      awk -v run="$pair" 'BEGIN{OFS="\t"} {$1=run; print}'
  done >> "$target"
}

ended_at="$(date --iso-8601=seconds)"
rustc_vv="$(cd "$repo" && nix develop "${nix_flags[@]}" -c rustc -Vv | tr '\n' ';')"
cargo_vv="$(cd "$repo" && nix develop "${nix_flags[@]}" -c cargo -Vv | tr '\n' ';')"
nix_version="$(nix --version)"
manifest_sha="$(sha256sum "$out/manifest-reference.json" | cut -d' ' -f1)"
flake_lock_sha="$(sha256sum "$repo/flake.lock" | cut -d' ' -f1)"
cat > "$out/binding.txt" <<EOF
started_at                $started_at
ended_at                  $ended_at
reference_product_ref      $reference
candidate_product_ref      $candidate
harness_ref                $harness
patch_sha256               $patch_sha
reference_binary_sha256    $ref_sha
candidate_binary_sha256    $cand_sha
resource_reference_binary_sha256 $resource_ref_sha
resource_candidate_binary_sha256 $resource_cand_sha
harness_tree_oid            $harness_tree
resource_harness_ref        $resource_harness
resource_guard_ref          $resource_guard
resource_parent_ref         $resource_parent
resource_overlay_sha256     $resource_overlay_sha
resource_overlay_file       resource-overlay.patch
reference_resource_harness_mode accepted timing patch plus accepted resource patch
candidate_resource_harness_mode materialized product hooks plus accepted bench resource patch
reference_harness_mode      accepted patch plus materialized accepted bench tree
candidate_harness_mode      product instrumentation plus materialized accepted bench tree
canonical_build_path       $tree
rustc_vv                   $rustc_vv
cargo_vv                   $cargo_vv
nix_version                $nix_version
flake_lock_sha256          $flake_lock_sha
manifest_sha256            $manifest_sha
CARGO_INCREMENTAL          0
RUSTFLAGS                  unset
CARGO_ENCODED_RUSTFLAGS    unset
matrix                     disclosed
pairs                      15
preliminary_warmups        3
pair_order                 alternating
correctness                once per binary before measurement
timeout_seconds            120
memory_limit_bytes         2147483648
chronology_rows            34
EOF

collect reference "$out/samples-reference.tsv"
collect candidate "$out/samples-candidate.tsv"
run_nix "$ref_bin" aggregate --reference "$out/samples-reference.tsv" \
  --candidate "$out/samples-candidate.tsv" --out "$out/analysis.json"
run_nix python3 "$repo/docs/post-correctness/refresh-attribution.py" --audit-live "$out" \
  > "$out/audit-live.txt"
run_nix python3 "$repo/docs/post-correctness/refresh-attribution.py" "$out" \
  > "$out/attribution.txt"
checksum_tmp="$(mktemp)"
(cd "$out" && find . -type f ! -name SHA256SUMS ! -name .lock -print0 | sort -z | xargs -0 sha256sum) >"$checksum_tmp"
mv "$checksum_tmp" "$out/SHA256SUMS"
rm -f "$out/.lock"
echo "[refresh] wrote $out"
