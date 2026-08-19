#!/usr/bin/env bash
# Measure the fixed pre-correctness scoring reference against the exact
# post-correctness product reference. The former needs the accepted harness
# patch; the latter already contains that harness in its history.
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
candidate="630ec442d951aac5704ae80287367912bfbfc388"
harness="4c127aba4e74fb6f8d486c4cb066586bb0d74846"
nix_flags=(--extra-experimental-features 'nix-command flakes')

exec 9>"$out/.lock"
flock -n 9 || { echo "refresh already running in $out" >&2; exit 1; }
rm -rf "${out:?}/raw" "${out:?}/bin"
rm -f "$out"/{analysis.json,attribution.txt,binding.txt,manifest-candidate.json,manifest-reference.json,pair-order.tsv,planned-pair-order.tsv,samples-candidate.tsv,samples-reference.tsv,warmup-candidate.json,warmup-reference.json}
mkdir -p "$out/raw" "$out/bin/reference" "$out/bin/candidate" "$build_root"
started_at="$(date --iso-8601=seconds)"

drop_tree() {
  git -C "$repo" worktree remove --force "$tree" >/dev/null 2>&1 || true
  rm -rf "$tree"
  git -C "$repo" worktree prune
}
trap drop_tree EXIT

build() {
  local side="$1" revision="$2" mode="$3"
  drop_tree
  git -C "$repo" worktree add --detach "$tree" "$revision" >/dev/null
  if [[ "$mode" == patch ]]; then
    git -C "$tree" apply --whitespace=nowarn "$patch"
  fi
  (cd "$tree" && env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS \
    CARGO_INCREMENTAL=0 nix develop "${nix_flags[@]}" -c \
    cargo build --manifest-path crates/flutterdec-bench/Cargo.toml --release >/dev/null)
  cp "$tree/crates/flutterdec-bench/target/release/flutterdec-bench" \
    "$out/bin/$side/flutterdec-bench"
  drop_tree
}

build reference "$reference" patch
build candidate "$candidate" embedded

ref_bin="$out/bin/reference/flutterdec-bench"
cand_bin="$out/bin/candidate/flutterdec-bench"
patch_sha="$(sha256sum "$patch" | cut -d' ' -f1)"
ref_sha="$(sha256sum "$ref_bin" | cut -d' ' -f1)"
cand_sha="$(sha256sum "$cand_bin" | cut -d' ' -f1)"
matrix_args=(--matrix disclosed)

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
  run_nix "$binary" run "${matrix_args[@]}" --warmups 3 --runs 0 --correctness on \
    --product-ref "$product" --harness-ref "$harness" \
    --patch-sha256 "$patch_sha" --binary-sha256 "$digest" \
    --label "post-correctness refresh $side warmup" \
    --out "$out/warmup-$side.json"
}

measure() {
  local side="$1" binary="$2" product="$3" digest="$4" pair="$5" position="$6"
  printf '%s\t%s\t%s\n' "$pair" "$position" "$side" >> "$out/pair-order.tsv"
  run_nix "$binary" run "${matrix_args[@]}" --warmups 0 --runs 1 --correctness off \
    --product-ref "$product" --harness-ref "$harness" \
    --patch-sha256 "$patch_sha" --binary-sha256 "$digest" \
    --label "post-correctness refresh $side pair $pair $position" \
    --out "$out/raw/$side-$pair.json" --samples "$out/raw/$side-$pair.tsv"
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

collect() {
  local side="$1" target="$2"
  head -n 1 "$out/raw/$side-0.tsv" > "$target"
  for ((pair = 0; pair < 15; pair++)); do
    tail -n +2 "$out/raw/$side-$pair.tsv" | \
      awk -v run="$pair" 'BEGIN{OFS="\t"} {$1=run; print}'
  done >> "$target"
}

collect reference "$out/samples-reference.tsv"
collect candidate "$out/samples-candidate.tsv"
run_nix "$ref_bin" aggregate --reference "$out/samples-reference.tsv" \
  --candidate "$out/samples-candidate.tsv" --out "$out/analysis.json"

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
candidate_harness_mode     embedded in exact candidate HEAD
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
EOF

run_nix python3 "$repo/docs/post-correctness/refresh-attribution.py" "$out" \
  > "$out/attribution.txt"
echo "[refresh] wrote $out"
