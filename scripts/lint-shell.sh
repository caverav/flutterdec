#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "$repo_root"

if ! command -v shellcheck >/dev/null 2>&1; then
  echo "shellcheck is required (use: nix develop)" >&2
  exit 1
fi

mapfile -t files < <(find scripts -maxdepth 1 -type f -name "*.sh" | sort)
if [[ ${#files[@]} -eq 0 ]]; then
  echo "[lint-shell] no shell scripts found under scripts/"
  exit 0
fi

echo "[lint-shell] checking ${#files[@]} script(s)"
shellcheck "${files[@]}"
echo "[lint-shell] ok"

