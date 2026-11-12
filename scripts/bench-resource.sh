#!/usr/bin/env bash
# Separate resource scoring for the frozen post-correctness reference and E1.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 OUT_DIR" >&2
  exit 2
fi

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="$(mkdir -p "$1" && cd "$1" && pwd)"
build_root="${TMPDIR:-/tmp}/flutterdec-resource-build"
tree="$build_root/tree"
reference="630ec442d951aac5704ae80287367912bfbfc388"
candidate="9b82e07fa62f97654aea5153d9fb6a2ef57a377a"
timing_harness="4c127aba4e74fb6f8d486c4cb066586bb0d74846"
resource_harness="b0e615785b28e7e58aa06dd1b929dd58acf06e53"
timing_tree="83e06014b368736c1921a0da7949c7b6a0b76e97"
overlay_paths=(
  crates/flutterdec-bench/src/main.rs
  crates/flutterdec-bench/src/measure.rs
  crates/flutterdec-decompiler/src/lib.rs
  crates/flutterdec-decompiler/src/control_flow/structured.rs
)
overlay_blobs=(
  c4cf741746b32ac55747180764ff10361c7999d1
  a658a769d8f2a8a4491a93870355cf98ac77aa7a
  542b0826dff120548ad9e166de8132669cab243f
  78d6c8b59f6844c5342eab6eed09f5b19c200922
)
nix_flags=(--extra-experimental-features 'nix-command flakes')

exec 9>"$out/.lock"
flock -n 9 || { echo "resource run already active in $out" >&2; exit 1; }
rm -rf "${out:?}/bin"
rm -f "$out"/*.json "$out"/*.tsv "$out"/*.txt
mkdir -p "$out/bin/reference" "$out/bin/candidate" "$build_root"

drop_tree() {
  git -C "$repo" worktree remove --force "$tree" >/dev/null 2>&1 || true
  rm -rf "$tree"
  git -C "$repo" worktree prune
}
trap drop_tree EXIT

build() {
  local side="$1" revision="$2" staged actual
  drop_tree
  git -C "$repo" worktree add --detach "$tree" "$revision" >/dev/null
  rm -rf "$tree/crates/flutterdec-bench"
  git -C "$repo" archive "$timing_harness" crates/flutterdec-bench | tar -x -C "$tree"
  staged="$(git -C "$tree" add -f crates/flutterdec-bench && git -C "$tree" write-tree)"
  actual="$(git -C "$tree" rev-parse "$staged:crates/flutterdec-bench")"
  [[ "$actual" == "$timing_tree" ]] || { echo "accepted timing harness drift: $actual" >&2; exit 1; }
  git -C "$repo" archive "$resource_harness" "${overlay_paths[@]}" | tar -x -C "$tree"
  for index in "${!overlay_paths[@]}"; do
    actual="$(git -C "$tree" hash-object "${overlay_paths[$index]}")"
    [[ "$actual" == "${overlay_blobs[$index]}" ]] || {
      echo "resource overlay drift: ${overlay_paths[$index]} $actual" >&2
      exit 1
    }
  done
  (cd "$tree" && env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS CARGO_INCREMENTAL=0 \
    nix develop "${nix_flags[@]}" -c cargo build \
      --manifest-path crates/flutterdec-bench/Cargo.toml --release >/dev/null)
  cp "$tree/crates/flutterdec-bench/target/release/flutterdec-bench" \
    "$out/bin/$side/flutterdec-bench"
  drop_tree
}

build reference "$reference"
build candidate "$candidate"
ref_bin="$out/bin/reference/flutterdec-bench"
cand_bin="$out/bin/candidate/flutterdec-bench"
ref_sha="$(sha256sum "$ref_bin" | cut -d' ' -f1)"
cand_sha="$(sha256sum "$cand_bin" | cut -d' ' -f1)"
[[ "$ref_sha" != "$cand_sha" ]] || { echo "different product refs built identical binaries" >&2; exit 1; }
overlay_digest="$(printf '%s\n' "${overlay_blobs[@]}" | sha256sum | cut -d' ' -f1)"

run_nix() {
  (cd "$repo" && env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS CARGO_INCREMENTAL=0 \
    nix develop "${nix_flags[@]}" -c "$@")
}
run_resource() {
  local binary="$1" product="$2" digest="$3" name="$4" plant="$5" warmups="$6"
  run_nix "$binary" resource --matrix disclosed --warmups "$warmups" --plant "$plant" \
    --product-ref "$product" --harness-ref "$resource_harness" \
    --patch-sha256 "$overlay_digest" --binary-sha256 "$digest" \
    --label "resource scoring $name" --out "$out/$name.json" --samples "$out/$name.tsv"
}

run_nix "$ref_bin" manifest --matrix disclosed --out "$out/manifest-reference.json"
run_nix "$cand_bin" manifest --matrix disclosed --out "$out/manifest-candidate.json"
diff -u "$out/manifest-reference.json" "$out/manifest-candidate.json"
run_resource "$ref_bin" "$reference" "$ref_sha" reference none 3
run_resource "$cand_bin" "$candidate" "$cand_sha" candidate none 3
run_resource "$cand_bin" "$candidate" "$cand_sha" noop none 0
run_resource "$cand_bin" "$candidate" "$cand_sha" cfg-plant cfg-graph-clone 0
run_resource "$cand_bin" "$candidate" "$cand_sha" emitter-plant emitter-block-clone 0
run_nix python3 "$repo/scripts/audit-resource-evidence.py" \
  --reference "$out/reference.tsv" --candidate "$out/candidate.tsv" \
  --noop "$out/noop.tsv" --cfg-plant "$out/cfg-plant.tsv" \
  --emitter-plant "$out/emitter-plant.tsv" --out "$out/audit.json"

cat >"$out/binding.txt" <<EOF
reference_product_ref $reference
candidate_product_ref $candidate
accepted_timing_harness_ref $timing_harness
accepted_timing_harness_tree $timing_tree
resource_harness_ref $resource_harness
resource_overlay_sha256 $overlay_digest
reference_binary_sha256 $ref_sha
candidate_binary_sha256 $cand_sha
matrix disclosed
warmups 3
threads 1
timing_selection_rerun no
resource_scoring separate
EOF
rm -rf "${out:?}/bin"
(cd "$out" && sha256sum -- ./*.json ./*.tsv binding.txt >SHA256SUMS)
rm -f "$out/.lock"
echo "[resource] wrote $out"
