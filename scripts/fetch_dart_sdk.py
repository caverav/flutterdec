#!/usr/bin/env python3
"""Resolve snapshot hash to Dart SDK source checkout (v1 bootstrap)."""

from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "src/core/dartvm/versions/manifest.json"
OUT_BASE = ROOT / "build/dart_sdks"


def run(cmd: list[str], cwd: Path | None = None) -> None:
    subprocess.run(cmd, cwd=str(cwd) if cwd else None, check=True)


def resolve_version(dart_hash: str) -> str:
    if MANIFEST.exists():
        data = json.loads(MANIFEST.read_text())
        for entry in data.get("entries", []):
            if entry.get("snapshot_hash") == dart_hash:
                return entry.get("version", "unknown")
    return "unknown"


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--dart-hash", required=True)
    args = p.parse_args()

    version = resolve_version(args.dart_hash)
    if version == "unknown":
        print(
            f"no pinned Dart SDK version found for hash={args.dart_hash}; "
            "skipping SDK fetch (dynamic adapter can still be built)"
        )
        return 0
    out = OUT_BASE / version
    out.parent.mkdir(parents=True, exist_ok=True)

    if out.exists():
        print(f"dart sdk already present: {out}")
        return 0

    # Source-first workflow; this pins a checkout location for adapter builds.
    run(["git", "clone", "https://github.com/dart-lang/sdk.git", str(out)])
    print(f"fetched dart sdk for hash={args.dart_hash} version={version} -> {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
