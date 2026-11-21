#!/usr/bin/env bash
# Stage the prefix a packaged flutterdec expects, so a release archive carries
# the read-only data the CLI resolves relative to its own executable.
#
# The layout below is not cosmetic: `flutterdec_loader::layout::Layout` looks
# for `<exe dir>/../share/flutterdec/adapters/registry.json`, reads the profile
# each compatibility record names under the same data directory, and publishes
# `adapters/python/adapter_template.py` from it on `adapter install`. An archive
# holding only the binary extracts into a CLI that cannot find any of them.
#
# Serwalker, the native snapshot parser, is bundled the same way: its
# executable is copied to `share/flutterdec/artifacts/flutterdec-serwalker`
# and its digest is written into every Serwalker record of the staged registry.
# The checked-in registry cannot carry that digest, because the bytes only exist
# once this build does. Both come from the directory holding `<binary>`, so
# build them together:
#
#   cargo build -p flutterdec-cli -p flutterdec-serwalker --release --bins \
#     --example bundle_registry
#
# `install -D` is GNU-only, so this uses mkdir/cp/chmod to stay usable on the
# macOS release runner.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: scripts/stage-release-prefix.sh <binary> <prefix-dir>" >&2
  exit 1
fi

binary="$1"
prefix="$2"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
share="$prefix/share/flutterdec"

if [[ ! -f "$binary" ]]; then
  echo "[stage-release-prefix] no such binary: $binary" >&2
  exit 1
fi
bin_dir="$(dirname "$binary")"
serwalker="$bin_dir/flutterdec-serwalker"
bundler="$bin_dir/examples/bundle_registry"
for tool in "$serwalker" "$bundler"; do
  if [[ ! -x "$tool" ]]; then
    echo "[stage-release-prefix] no such executable: $tool" >&2
    echo "[stage-release-prefix] build it with: cargo build -p flutterdec-cli -p flutterdec-serwalker --release --bins --example bundle_registry" >&2
    exit 1
  fi
done

mkdir -p "$prefix/bin" "$share/adapters/python" "$share/data/serwalker_profiles"
cp "$binary" "$prefix/bin/flutterdec"
chmod 0755 "$prefix/bin/flutterdec"
cp "$repo_root/adapters/registry.json" "$share/adapters/registry.json"
cp "$repo_root/adapters/python/adapter_template.py" \
  "$share/adapters/python/adapter_template.py"
for profile in "$repo_root"/data/*.json; do
  cp "$profile" "$share/data/$(basename "$profile")"
done
for profile in "$repo_root"/data/serwalker_profiles/*.json; do
  cp "$profile" "$share/data/serwalker_profiles/$(basename "$profile")"
done
"$bundler" --data-dir "$share" --artifact "$serwalker"
chmod 0644 "$share/adapters/registry.json" \
  "$share/adapters/python/adapter_template.py" \
  "$share"/data/*.json
chmod 0644 "$share"/data/serwalker_profiles/*.json

missing=0
for required in \
  bin/flutterdec \
  share/flutterdec/adapters/registry.json \
  share/flutterdec/adapters/python/adapter_template.py \
  share/flutterdec/artifacts/flutterdec-serwalker \
  share/flutterdec/data/dart-profiles.json; do
  if [[ ! -f "$prefix/$required" ]]; then
    echo "[stage-release-prefix] missing $required" >&2
    missing=1
  fi
done
if [[ "$missing" -ne 0 ]]; then
  exit 1
fi

echo "[stage-release-prefix] staged $prefix"
