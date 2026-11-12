#!/usr/bin/env bash
# Finalize a fully measured run whose fail-closed audit selected honest no-win.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 RUN_DIR" >&2
  exit 2
fi

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out="$(cd "$1" && pwd)"
reference="630ec442d951aac5704ae80287367912bfbfc388"
candidate="9b82e07fa62f97654aea5153d9fb6a2ef57a377a"
timing_harness="4c127aba4e74fb6f8d486c4cb066586bb0d74846"
resource_harness="b0e615785b28e7e58aa06dd1b929dd58acf06e53"
resource_guard="f4eb2d87d0bc8649addce2a2522ee8a56e856ca1"
timing_tree="$(git -C "$repo" rev-parse "$timing_harness:crates/flutterdec-bench")"
patch="$repo/docs/baseline/harness-8e7f080.patch"
overlay_paths=(crates/flutterdec-bench/src/main.rs crates/flutterdec-bench/src/measure.rs crates/flutterdec-decompiler/src/lib.rs crates/flutterdec-decompiler/src/control_flow/structured.rs)
nix_flags=(--extra-experimental-features 'nix-command flakes')

[[ -f "$out/seed.private" && ! -e "$out/seed.txt" ]]
for matrix in disclosed held-out; do
  [[ "$(wc -l <"$out/$matrix/timing/pair-order.tsv")" == 30 ]]
  [[ -f "$out/$matrix/resource/reference.tsv" && -f "$out/$matrix/resource/candidate.tsv" ]]
done

run_nix() {
  (cd "$repo" && env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS CARGO_INCREMENTAL=0 \
    nix develop "${nix_flags[@]}" -c "$@")
}

run_nix python3 "$repo/docs/final-performance/analyze-final.py" "$out" | tee "$out/final-audit.txt"
run_nix python3 -c 'import os,sys; os.replace(sys.argv[1],sys.argv[2])' "$out/seed.private" "$out/seed.txt"
chmod 0644 "$out/seed.txt"

timing_patch_sha="$(sha256sum "$patch" | cut -d' ' -f1)"
overlay_digest="$(for path in "${overlay_paths[@]}"; do git -C "$repo" rev-parse "$resource_harness:$path"; done | sha256sum | cut -d' ' -f1)"
rustc_vv="$(run_nix rustc -Vv | tr '\n' ';')"
cargo_vv="$(run_nix cargo -Vv | tr '\n' ';')"
rustc_path="$(run_nix sh -c 'command -v rustc')"
cargo_path="$(run_nix sh -c 'command -v cargo')"
cat >"$out/binding.txt" <<EOF
started_at $(date --iso-8601=seconds -d "$(stat -c %y "$out/bin/timing/reference/flutterdec-bench")")
bound_at $(date --iso-8601=seconds -d "$(stat -c %y "$out/bin/resource/candidate/sha256")")
seed_created_at $(date --iso-8601=seconds -d "$(stat -c %y "$out/seed.txt")")
heldout_first_run_at $(date --iso-8601=seconds -d "$(stat -c %y "$out/held-out/timing/raw/reference-0.json")")
heldout_completed_at $(date --iso-8601=seconds -d "$(stat -c %y "$out/held-out/resource/candidate.json")")
ended_at $(date --iso-8601=seconds)
seed_non_disclosure seed and manifest were not printed or inspected before both product revisions were bound; seed disclosed only after all held-out pairs and resource runs completed
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
rustc_binary_sha256 $(run_nix sha256sum "$rustc_path" | cut -d' ' -f1)
cargo_vv $cargo_vv
cargo_binary_sha256 $(run_nix sha256sum "$cargo_path" | cut -d' ' -f1)
nix_version $(nix --version)
nix_binary_sha256 $(sha256sum "$(command -v nix)" | cut -d' ' -f1)
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
echo "[finalize] wrote $out"
