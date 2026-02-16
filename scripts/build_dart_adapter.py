#!/usr/bin/env python3
"""Build/install adapter executable for snapshot hash.

This installs a dynamic adapter that parses snapshot blobs provided by flutterdec
and emits schema_version=1 program JSON for that hash/version.
"""

from __future__ import annotations

import argparse
import json
import stat
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
        "version": "unknown",
        "adapter": f"dart_adapter_{dart_hash}",
    }


ADAPTER_TEMPLATE = r'''#!/usr/bin/env python3
import argparse
import json
import re
import struct
from typing import List, Set

VERSION = __VERSION__
SNAPSHOT_HASH = __SNAPSHOT_HASH__


def read_bytes(path: str) -> bytes:
    with open(path, "rb") as f:
        return f.read()


def extract_strings(data: bytes, min_len: int = 5, max_items: int = 20000) -> List[str]:
    out = []
    for m in re.finditer(rb"[ -~]{%d,}" % min_len, data):
        try:
            s = m.group(0).decode("utf-8", errors="ignore")
        except Exception:
            continue
        if len(s) > 200:
            continue
        out.append(s)
        if len(out) >= max_items:
            break

    seen = set()
    uniq = []
    for s in out:
        if s in seen:
            continue
        seen.add(s)
        uniq.append(s)
    return uniq


def decode_bl_target(pc: int, word: int) -> int | None:
    if ((word >> 26) & 0x3F) != 0b100101:
        return None
    imm26 = word & 0x03FFFFFF
    if imm26 & (1 << 25):
        imm26 -= (1 << 26)
    return pc + (imm26 << 2)


def is_frame_prologue(word: int) -> bool:
    rt = word & 0x1F
    rn = (word >> 5) & 0x1F
    rt2 = (word >> 10) & 0x1F
    is_store_pair = ((word >> 30) & 0x3) == 0b10
    return is_store_pair and rt == 29 and rt2 == 30 and rn == 31


def recover_functions(instr: bytes, base_va: int) -> List[dict]:
    starts: Set[int] = set()
    if base_va:
        starts.add(base_va)

    instr_len = len(instr)
    hi = base_va + instr_len

    for off in range(0, instr_len - 3, 4):
        word = struct.unpack_from("<I", instr, off)[0]
        pc = base_va + off
        tgt = decode_bl_target(pc, word)
        if tgt is not None and base_va <= tgt < hi:
            starts.add(tgt)

    for off in range(0, instr_len - 3, 4):
        word = struct.unpack_from("<I", instr, off)[0]
        if is_frame_prologue(word):
            starts.add(base_va + off)

    sorted_starts = sorted(starts)
    funcs = []
    for i, start in enumerate(sorted_starts):
        nxt = sorted_starts[i + 1] if i + 1 < len(sorted_starts) else hi
        size = max(4, min(nxt - start, 0x8000)) if nxt > start else 128
        funcs.append({
            "id": i,
            "name": f"sub_{start}",
            "owner_class": "Global",
            "entry_va": int(start),
            "size": int(size),
            "code_section_va": int(base_va),
        })
    return funcs


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

    vm_data = read_bytes(args.vm_data)
    iso_data = read_bytes(args.isolate_data)
    iso_instr = read_bytes(args.isolate_instr)

    strings = extract_strings(vm_data + iso_data)

    object_pool = []
    for i, s in enumerate(strings):
        object_pool.append({"i": i, "kind": "String", "s": s})

    lib_uri = next((s for s in strings if s.startswith("package:")), "package:app/main.dart")
    classes = [{"id": 0, "name": "Global", "super": "Object", "lib": lib_uri}]

    functions = recover_functions(iso_instr, args.isolate_instr_va)
    if not functions:
        functions = [{
            "id": 0,
            "name": "entry",
            "owner_class": "Global",
            "entry_va": int(args.isolate_instr_va or 4096),
            "size": 128,
            "code_section_va": int(args.isolate_instr_va or 4096),
        }]

    payload = {
        "schema_version": 1,
        "adapter_kind": "native_snapshot_adapter",
        "dart_version": VERSION,
        "snapshot_hash": SNAPSHOT_HASH,
        "arch": "arm64",
        "object_pool": object_pool,
        "classes": classes,
        "functions": functions,
    }

    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(payload, f, indent=2)


if __name__ == "__main__":
    main()
'''


def build_adapter_source(version: str, snapshot_hash: str) -> str:
    src = ADAPTER_TEMPLATE.replace("__VERSION__", json.dumps(version))
    src = src.replace("__SNAPSHOT_HASH__", json.dumps(snapshot_hash))
    return src


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

    adapter.write_text(build_adapter_source(version, snapshot_hash), encoding="utf-8")
    adapter.chmod(adapter.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

    print(f"installed dynamic adapter: {adapter}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
