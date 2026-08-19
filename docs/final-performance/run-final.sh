#!/usr/bin/env bash
# Frozen final timing and additive resource disposition for 630ec44..9b82e07.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 OUT_DIR" >&2
  exit 2
fi

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out="$(mkdir -p "$1" && cd "$1" && pwd)"
build_root="${TMPDIR:-/tmp}/flutterdec-final-performance-build"
tree="$build_root/tree"
reference="630ec442d951aac5704ae80287367912bfbfc388"
candidate="9b82e07fa62f97654aea5153d9fb6a2ef57a377a"
timing_harness="4c127aba4e74fb6f8d486c4cb066586bb0d74846"
resource_harness="b0e615785b28e7e58aa06dd1b929dd58acf06e53"
resource_guard="f4eb2d87d0bc8649addce2a2522ee8a56e856ca1"
patch="$repo/docs/baseline/harness-8e7f080.patch"
timing_tree="$(git -C "$repo" rev-parse "$timing_harness:crates/flutterdec-bench")"
overlay_paths=(crates/flutterdec-bench/src/main.rs crates/flutterdec-bench/src/measure.rs crates/flutterdec-decompiler/src/lib.rs crates/flutterdec-decompiler/src/control_flow/structured.rs)
nix_flags=(--extra-experimental-features 'nix-command flakes')

exec 9>"$out/.lock"
flock -n 9 || { echo "final run already active in $out" >&2; exit 1; }
rm -rf "${out:?}/disclosed" "${out:?}/held-out" "${out:?}/bin"
rm -f "$out"/{SHA256SUMS,binding.txt,final-audit.json,final-audit.txt,seed.private,seed.txt}
mkdir -p "$out/bin/timing/reference" "$out/bin/timing/candidate" "$out/bin/resource/reference" "$out/bin/resource/candidate" "$build_root"
started_at="$(date --iso-8601=seconds)"

drop_tree() {
  git -C "$repo" worktree remove --force "$tree" >/dev/null 2>&1 || true
  rm -rf "$tree"
  git -C "$repo" worktree prune
}
trap drop_tree EXIT

run_nix() {
  (cd "$repo" && env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS CARGO_INCREMENTAL=0 \
    nix develop "${nix_flags[@]}" -c "$@")
}

build() {
  local kind="$1" side="$2" revision="$3" staged actual path blob index
  drop_tree
  git -C "$repo" worktree add --detach "$tree" "$revision" >/dev/null
  rm -rf "$tree/crates/flutterdec-bench"
  git -C "$repo" archive "$timing_harness" crates/flutterdec-bench | tar -x -C "$tree"
  git -C "$tree" add -f crates/flutterdec-bench
  staged="$(git -C "$tree" write-tree)"
  actual="$(git -C "$tree" rev-parse "$staged:crates/flutterdec-bench")"
  [[ "$actual" == "$timing_tree" ]] || { echo "accepted timing tree drift" >&2; exit 1; }
  if [[ "$kind" == resource ]]; then
    git -C "$repo" archive "$resource_harness" "${overlay_paths[@]}" | tar -x -C "$tree"
    for index in "${!overlay_paths[@]}"; do
      path="${overlay_paths[$index]}"
      blob="$(git -C "$repo" rev-parse "$resource_harness:$path")"
      [[ "$(git -C "$tree" hash-object "$path")" == "$blob" ]] || exit 1
    done
  fi
  (cd "$tree" && env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS CARGO_INCREMENTAL=0 \
    nix develop "${nix_flags[@]}" -c cargo build --manifest-path crates/flutterdec-bench/Cargo.toml --release >/dev/null)
  cp "$tree/crates/flutterdec-bench/target/release/flutterdec-bench" "$out/bin/$kind/$side/flutterdec-bench"
  drop_tree
}

# All product revisions and both rulers are immutable and built before entropy is drawn.
build timing reference "$reference"
build timing candidate "$candidate"
build resource reference "$reference"
build resource candidate "$candidate"
bound_at="$(date --iso-8601=seconds)"

timing_patch_sha="$(sha256sum "$patch" | cut -d' ' -f1)"
overlay_digest="$(for path in "${overlay_paths[@]}"; do git -C "$repo" rev-parse "$resource_harness:$path"; done | sha256sum | cut -d' ' -f1)"
for kind in timing resource; do
  for side in reference candidate; do
    sha256sum "$out/bin/$kind/$side/flutterdec-bench" | cut -d' ' -f1 >"$out/bin/$kind/$side/sha256"
  done
done

# The private seed is not printed or exposed to an implementation worker. It is
# disclosed only after both held-out product revisions have completed.
umask 077
seed_created_at="$(date --iso-8601=seconds)"
run_nix python3 -c 'import secrets,sys; open(sys.argv[1],"x",encoding="ascii").write(secrets.token_hex(16)+"\n")' "$out/seed.private"
seed="$(<"$out/seed.private")"

schedule() {
  local target="$1" pair
  for ((pair=0; pair<15; pair++)); do
    if ((pair % 2 == 0)); then
      printf '%s\t%s\t%s\n' "$pair" first reference
      printf '%s\t%s\t%s\n' "$pair" second candidate
    else
      printf '%s\t%s\t%s\n' "$pair" first candidate
      printf '%s\t%s\t%s\n' "$pair" second reference
    fi
  done >"$target"
}

run_matrix() {
  local matrix="$1" pair side position binary product binary_sha
  local root="$out/$matrix"
  local args=(--matrix "$matrix")
  [[ "$matrix" == held-out ]] && args+=(--held-out-seed "$seed")
  mkdir -p "$root/timing/raw" "$root/resource"
  schedule "$root/timing/planned-pair-order.tsv"
  : >"$root/timing/pair-order.tsv"
  for side in reference candidate; do
    [[ "$side" == reference ]] && product="$reference" || product="$candidate"
    binary="$out/bin/timing/$side/flutterdec-bench"
    binary_sha="$(<"$out/bin/timing/$side/sha256")"
    run_nix "$binary" manifest "${args[@]}" --out "$root/timing/manifest-$side.json"
    run_nix "$binary" run "${args[@]}" --warmups 3 --runs 0 --correctness on \
      --product-ref "$product" --harness-ref "$timing_harness" --patch-sha256 "$timing_patch_sha" \
      --binary-sha256 "$binary_sha" --label "final $matrix $side warmup" --out "$root/timing/warmup-$side.json"
  done
  diff -u "$root/timing/manifest-reference.json" "$root/timing/manifest-candidate.json"
  for ((pair=0; pair<15; pair++)); do
    if ((pair % 2 == 0)); then sides=(reference candidate); else sides=(candidate reference); fi
    for index in 0 1; do
      side="${sides[$index]}"; [[ "$index" == 0 ]] && position=first || position=second
      [[ "$side" == reference ]] && product="$reference" || product="$candidate"
      binary="$out/bin/timing/$side/flutterdec-bench"; binary_sha="$(<"$out/bin/timing/$side/sha256")"
      printf '%s\t%s\t%s\n' "$pair" "$position" "$side" >>"$root/timing/pair-order.tsv"
      run_nix "$binary" run "${args[@]}" --warmups 0 --runs 1 --correctness off \
        --product-ref "$product" --harness-ref "$timing_harness" --patch-sha256 "$timing_patch_sha" \
        --binary-sha256 "$binary_sha" --label "final $matrix $side pair $pair $position" \
        --out "$root/timing/raw/$side-$pair.json" --samples "$root/timing/raw/$side-$pair.tsv"
    done
    echo "[final] $matrix pair $((pair+1))/15"
  done
  diff -u "$root/timing/planned-pair-order.tsv" "$root/timing/pair-order.tsv"
  for side in reference candidate; do
    head -n 1 "$root/timing/raw/$side-0.tsv" >"$root/timing/samples-$side.tsv"
    for ((pair=0; pair<15; pair++)); do
      tail -n +2 "$root/timing/raw/$side-$pair.tsv" | awk -v run="$pair" 'BEGIN{OFS="\t"}{$1=run;print}'
    done >>"$root/timing/samples-$side.tsv"
    [[ "$side" == reference ]] && product="$reference" || product="$candidate"
    binary="$out/bin/resource/$side/flutterdec-bench"; binary_sha="$(<"$out/bin/resource/$side/sha256")"
    run_nix "$binary" resource "${args[@]}" --warmups 3 --plant none \
      --product-ref "$product" --harness-ref "$resource_harness" --patch-sha256 "$overlay_digest" \
      --binary-sha256 "$binary_sha" --label "final $matrix resource $side" \
      --out "$root/resource/$side.json" --samples "$root/resource/$side.tsv"
  done
  run_nix "$out/bin/timing/reference/flutterdec-bench" aggregate \
    --reference "$root/timing/samples-reference.tsv" --candidate "$root/timing/samples-candidate.tsv" \
    --out "$root/timing/analysis-all-cells.json"
}

run_matrix disclosed
heldout_first_run_at="$(date --iso-8601=seconds)"
run_matrix held-out
heldout_completed_at="$(date --iso-8601=seconds)"

run_nix python3 "$repo/docs/final-performance/analyze-final.py" "$out" | tee "$out/final-audit.txt"
# Disclosure happens only after both frozen revisions and all held-out pairs are complete.
run_nix python3 -c 'import os,sys; os.replace(sys.argv[1],sys.argv[2])' "$out/seed.private" "$out/seed.txt"
chmod 0644 "$out/seed.txt"
ended_at="$(date --iso-8601=seconds)"

rustc_vv="$(run_nix rustc -Vv | tr '\n' ';')"
cargo_vv="$(run_nix cargo -Vv | tr '\n' ';')"
rustc_path="$(run_nix sh -c 'command -v rustc')"
cargo_path="$(run_nix sh -c 'command -v cargo')"
rustc_sha="$(run_nix sha256sum "$rustc_path" | cut -d' ' -f1)"
cargo_sha="$(run_nix sha256sum "$cargo_path" | cut -d' ' -f1)"
nix_sha="$(sha256sum "$(command -v nix)" | cut -d' ' -f1)"
cat >"$out/binding.txt" <<EOF
started_at $started_at
bound_at $bound_at
seed_created_at $seed_created_at
heldout_first_run_at $heldout_first_run_at
heldout_completed_at $heldout_completed_at
ended_at $ended_at
seed_non_disclosure seed and manifest not printed or read by implementation workers before both product revisions were bound; seed disclosed after both held-out runs completed
reference_product_ref $reference
reference_commit_time $(git -C "$repo" show -s --format=%cI "$reference")
reference_tree $(git -C "$repo" rev-parse "$reference^{tree}")
candidate_product_ref $candidate
candidate_commit_time $(git -C "$repo" show -s --format=%cI "$candidate")
candidate_tree $(git -C "$repo" rev-parse "$candidate^{tree}")
accepted_timing_harness_ref $timing_harness
accepted_timing_harness_tree $timing_tree
timing_patch_sha256 $timing_patch_sha
accepted_resource_guard_ref $resource_guard
resource_harness_ref $resource_harness
resource_overlay_sha256 $overlay_digest
timing_reference_binary_sha256 $(<"$out/bin/timing/reference/sha256")
timing_candidate_binary_sha256 $(<"$out/bin/timing/candidate/sha256")
resource_reference_binary_sha256 $(<"$out/bin/resource/reference/sha256")
resource_candidate_binary_sha256 $(<"$out/bin/resource/candidate/sha256")
rustc_vv $rustc_vv
rustc_binary_sha256 $rustc_sha
cargo_vv $cargo_vv
cargo_binary_sha256 $cargo_sha
nix_version $(nix --version)
nix_binary_sha256 $nix_sha
flake_lock_sha256 $(sha256sum "$repo/flake.lock" | cut -d' ' -f1)
rust_toolchain_sha256 $(sha256sum "$repo/rust-toolchain.toml" | cut -d' ' -f1)
CARGO_INCREMENTAL 0
RUSTFLAGS unset
CARGO_ENCODED_RUSTFLAGS unset
warmups 3
pairs 15
pair_order alternating
timeout_seconds 120
memory_limit_bytes 2147483648
EOF

rm -rf "${out:?}/bin" "${out:?}/disclosed/timing/raw" "${out:?}/held-out/timing/raw"
checksum_tmp="$(mktemp)"
(cd "$out" && find . -type f ! -name SHA256SUMS ! -name .lock -print0 | sort -z | xargs -0 sha256sum) >"$checksum_tmp"
mv "$checksum_tmp" "$out/SHA256SUMS"
rm -f "$out/.lock"
echo "[final] wrote $out"
