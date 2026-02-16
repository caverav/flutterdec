#!/usr/bin/env python3
"""Build/install adapter executable for snapshot hash.

v1 bootstrap installs a deterministic adapter shim in ~/.cache/flutterdec/adapters/
that emits schema_version=1 JSON and keeps core contracts stable.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "src/core/dartvm/versions/manifest.json"


def resolve_entry(dart_hash: str) -> dict:
    if MANIFEST.exists():
        data = json.loads(MANIFEST.read_text())
        for entry in data.get("entries", []):
            if entry.get("snapshot_hash") == dart_hash:
                return entry
    return {
        "snapshot_hash": dart_hash,
        "version": dart_hash,
        "adapter": f"dart_adapter_{dart_hash}",
    }


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--dart-hash", required=True)
    args = p.parse_args()

    entry = resolve_entry(args.dart_hash)
    adapter_name = entry.get("adapter", f"dart_adapter_{args.dart_hash}")
    version = entry.get("version", "unknown")
    snapshot_hash = entry.get("snapshot_hash", args.dart_hash)

    out_dir = Path.home() / ".cache/flutterdec/adapters"
    out_dir.mkdir(parents=True, exist_ok=True)
    adapter = out_dir / adapter_name

    shim = f'''#!/usr/bin/env python3
import argparse
import json

def main():
    p = argparse.ArgumentParser()
    p.add_argument("--vm-data", required=True)
    p.add_argument("--isolate-data", required=True)
    p.add_argument("--vm-instr", required=True)
    p.add_argument("--isolate-instr", required=True)
    p.add_argument("--vm-instr-va", type=int, default=0)
    p.add_argument("--isolate-instr-va", type=int, default=0)
    p.add_argument("--out", required=True)
    args = p.parse_args()

    payload = {{
      "schema_version": 1,
      "dart_version": {json.dumps(version)},
      "snapshot_hash": {json.dumps(snapshot_hash)},
      "arch": "arm64",
      "object_pool": [
        {{"i": 0, "kind": "String", "s": "package:app/main.dart"}},
        {{"i": 1, "kind": "Type", "s": "void?"}},
        {{"i": 2, "kind": "Int", "n": 42}}
      ],
      "classes": [
        {{"id": 1, "name": "A", "super": "Object", "lib": "package:app/main.dart"}}
      ],
      "functions": [
        {{"id": 1, "name": "a", "owner_class": "A", "entry_va": args.isolate_instr_va or 4096, "size": 128}}
      ]
    }}

    with open(args.out, "w", encoding="utf-8") as f:
      json.dump(payload, f, indent=2)

if __name__ == "__main__":
    main()
'''

    adapter.write_text(shim)
    adapter.chmod(0o755)
    print(f"installed adapter: {adapter}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
