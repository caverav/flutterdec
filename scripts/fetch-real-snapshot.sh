#!/usr/bin/env bash
set -euo pipefail

# LocalSend 1.17.0 arm64 is the fixture used by the ignored real-snapshot tests.
# Pin both the APK and the extracted libapp.so so a changed release cannot pass
# as the same compatibility record.
readonly APK_URL="https://github.com/localsend/localsend/releases/download/v1.17.0/LocalSend-1.17.0-android-arm64v8.apk"
readonly APK_SHA256="2c7f5fd4872da25115bb8e5e62f92de94dda47b0f249ff387ac667b13871dc3e"
readonly LIBAPP_SHA256="3df9e9983424fc1f5b08345b19e6ce912291692e60a86e63c0608ff350512023"

usage() {
  cat <<'USAGE'
Usage:
  scripts/fetch-real-snapshot.sh [OUTPUT_DIR]

Downloads the pinned LocalSend 1.17.0 arm64 APK, verifies its SHA-256, extracts
lib/arm64-v8a/libapp.so, and verifies the extracted digest. Prints the path to
the extracted libapp.so.

The default OUTPUT_DIR is target/serwalker-real-snapshot.
USAGE
}

if [[ ${1:-} == "-h" || ${1:-} == "--help" ]]; then
  usage
  exit 0
fi

verify_sha256() {
  local file="$1"
  local expected="$2"
  local actual

  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$file" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$file" | awk '{print $1}')"
  else
    echo "a SHA-256 tool is required (sha256sum or shasum)" >&2
    return 1
  fi

  if [[ "$actual" != "$expected" ]]; then
    echo "sha256 mismatch for $file: expected $expected, got $actual" >&2
    return 1
  fi
}

out_dir="${1:-target/serwalker-real-snapshot}"
mkdir -p "$out_dir"

apk="$out_dir/LocalSend-1.17.0-android-arm64v8.apk"
libapp="$out_dir/libapp.so"

if [[ ! -f "$apk" ]]; then
  part="$apk.part"
  rm -f "$part"
  echo "downloading LocalSend 1.17.0 arm64 APK" >&2
  curl -fsSL --retry 3 --retry-delay 2 -o "$part" "$APK_URL"
  mv "$part" "$apk"
fi
verify_sha256 "$apk" "$APK_SHA256"

part="$libapp.part"
rm -f "$part"
unzip -p "$apk" lib/arm64-v8a/libapp.so >"$part"
mv "$part" "$libapp"
verify_sha256 "$libapp" "$LIBAPP_SHA256"

printf '%s\n' "$libapp"
