#!/usr/bin/env bash
# Prove a release archive is usable off a checkout.
#
# Assembles (or accepts) the archive the release workflow publishes, extracts it
# into a fresh directory, and drives the extracted binary from an unrelated
# empty working directory with an isolated HOME and a scrubbed environment. The
# CLI therefore has no repository root, no `FLUTTERDEC_*` override, and no
# ambient store to fall back on: everything it finds must come from the archive
# and from the writable store under the isolated HOME.
#
# Usage:
#   scripts/release-layout-smoke.sh <binary>          # stage and pack, then check
#   scripts/release-layout-smoke.sh <archive.tar.gz>  # check a published archive
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: scripts/release-layout-smoke.sh <binary|archive.tar.gz>" >&2
  exit 1
fi

subject="$1"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

work="$(mktemp -d)"
cleanup() { rm -rf "$work"; }
trap cleanup EXIT

fail() {
  echo "[release-layout-smoke] FAIL: $*" >&2
  exit 1
}

# Anything the CLI writes into the checkout is newer than this marker.
marker="$work/marker"
touch "$marker"
before_status="$(git -C "$repo_root" status --porcelain)"

archive="$work/archive.tar.gz"
case "$subject" in
  *.tar.gz)
    [[ -f "$subject" ]] || fail "no such archive: $subject"
    cp "$subject" "$archive"
    ;;
  *)
    "$script_dir/stage-release-prefix.sh" "$subject" "$work/prefix"
    # Byte-identical to the release workflow's packaging step.
    tar -C "$work/prefix" -czf "$archive" bin share
    ;;
esac

members="$(tar -tzf "$archive")"
for required in \
  bin/flutterdec \
  share/flutterdec/adapters/registry.json \
  share/flutterdec/adapters/python/adapter_template.py \
  share/flutterdec/artifacts/flutterdec-serwalker \
  share/flutterdec/data/dart-profiles.json; do
  grep -qx "$required" <<<"$members" || fail "archive has no $required"
done
echo "[release-layout-smoke] archive members ok"
grep -Eq '^share/flutterdec/data/serwalker_profiles/profile_[[:alnum:]_-]+\.json$' <<<"$members" ||
  fail "archive has no Serwalker data profiles"

extracted="$work/extracted"
mkdir -p "$extracted"
tar -xzf "$archive" -C "$extracted"

home="$work/home"
empty_cwd="$work/empty-cwd"
mkdir -p "$home" "$empty_cwd"
cli="$extracted/bin/flutterdec"
[[ -x "$cli" ]] || fail "extracted binary is not executable"

run_cli() {
  # No PWD-derived state, no inherited FLUTTERDEC_* override, no real HOME.
  (cd "$empty_cwd" && env -i HOME="$home" PATH="/usr/bin:/bin" "$cli" "$@")
}

if ! before="$(run_cli adapter list --json)"; then
  fail "adapter list failed on the extracted archive"
fi
grep -q '"state": "unavailable"' <<<"$before" ||
  fail "expected the bundled records to start unavailable"
if ! listing="$(run_cli adapter list)"; then
  fail "adapter list failed on the extracted archive"
fi

# The hashes come out of the CLI, so each can only be a record the CLI actually
# read out of the extracted registry. One record per adapter: Serwalker, the
# native parser the archive bundles, and the reference producer.
record_for() {
  local adapter="$1"
  grep " adapter=$adapter " <<<"$listing" | head -1 |
    sed -n 's/^hash=\([0-9a-f]\{32\}\) .*/\1/p'
}
for adapter in flutterdec-serwalker flutterdec-local-python; do
  hash="$(record_for "$adapter")"
  [[ -n "$hash" ]] || fail "adapter list reported no $adapter record"
  grep -q "\"$hash\"" "$extracted/share/flutterdec/adapters/registry.json" ||
    fail "record $hash is not in the archived registry"
  echo "[release-layout-smoke] adapter list loaded the bundled $adapter record ($hash)"

  if ! installed="$(run_cli adapter install --dart-hash "$hash" --json)"; then
    fail "adapter install failed for the $adapter record on the extracted archive"
  fi
  grep -q '"source": "packaged-producer"' <<<"$installed" ||
    fail "install did not publish the packaged $adapter artifact"
  grep -q "$home/.local/share/flutterdec/adapters" <<<"$installed" ||
    fail "install did not use the store under the isolated HOME"
  echo "[release-layout-smoke] install resolved the packaged $adapter artifact and profile"
done

if ! after="$(run_cli adapter list)"; then
  fail "adapter list failed after install"
fi
for adapter in flutterdec-serwalker flutterdec-local-python; do
  grep -q "state=verified adapter=$adapter " <<<"$after" ||
    fail "the installed $adapter record is not verified"
done
echo "[release-layout-smoke] list reports both installed adapters verified"

store="$home/.local/share/flutterdec/adapters"
[[ -d "$store" ]] || fail "no writable store under the isolated HOME"
[[ -n "$(find "$store/artifacts" -type f -print -quit 2>/dev/null)" ]] ||
  fail "the store holds no published artifact"

# Nothing above may have touched the checkout.
after_status="$(git -C "$repo_root" status --porcelain)"
[[ "$before_status" == "$after_status" ]] ||
  fail "git status changed during the smoke run"
touched="$(find "$repo_root" -newer "$marker" \
  -not -path "$repo_root/.git/*" \
  -not -path "$repo_root/target/*" \
  -not -path "$repo_root/.git" \
  -not -path "$repo_root/target" \
  -not -path "$repo_root" -print 2>/dev/null || true)"
[[ -z "$touched" ]] || fail "the checkout was written to: $touched"

echo "[release-layout-smoke] checkout untouched, store isolated under HOME"
echo "[release-layout-smoke] ok"
