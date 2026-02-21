#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import struct
from typing import Dict, List, Set


def _read_bytes(path: str) -> bytes:
    with open(path, "rb") as f:
        return f.read()


def _extract_strings(data: bytes, min_len: int = 5, max_items: int = 20000) -> List[str]:
    out: List[str] = []
    for m in re.finditer(rb"[ -~]{%d,}" % min_len, data):
        s = m.group(0).decode("utf-8", errors="ignore")
        if len(s) > 220:
            continue
        out.append(s)
        if len(out) >= max_items:
            break

    seen: Set[str] = set()
    uniq: List[str] = []
    for s in out:
        if s in seen:
            continue
        seen.add(s)
        uniq.append(s)
    return uniq


def _detect_snapshot_hash(vm_data: bytes, iso_data: bytes, fallback: str) -> str:
    probe = (vm_data + iso_data)[:65536]
    m = re.search(rb"([0-9a-f]{32})product\\s+no-code_comments", probe)
    if m:
        return m.group(1).decode("ascii", errors="ignore")
    m2 = re.search(rb"\b([0-9a-f]{32})\b", probe)
    if m2:
        return m2.group(1).decode("ascii", errors="ignore")
    return fallback


def _decode_bl_target(pc: int, word: int) -> int | None:
    if ((word >> 26) & 0x3F) != 0b100101:
        return None
    imm26 = word & 0x03FFFFFF
    if imm26 & (1 << 25):
        imm26 -= (1 << 26)
    return pc + (imm26 << 2)


def _is_frame_prologue(word: int) -> bool:
    rt = word & 0x1F
    rn = (word >> 5) & 0x1F
    rt2 = (word >> 10) & 0x1F
    is_store_pair = ((word >> 30) & 0x3) == 0b10
    return is_store_pair and rt == 29 and rt2 == 30 and rn == 31


def _recover_functions(instr: bytes, base_va: int) -> List[dict]:
    starts: Set[int] = set()
    if base_va:
        starts.add(base_va)

    instr_len = len(instr)
    hi = base_va + instr_len

    prologues: Set[int] = set()
    call_target_counts: Dict[int, int] = {}
    for off in range(0, max(0, instr_len - 3), 4):
        word = struct.unpack_from("<I", instr, off)[0]
        pc = base_va + off
        tgt = _decode_bl_target(pc, word)
        if tgt is not None and base_va <= tgt < hi and (tgt - base_va) % 4 == 0:
            call_target_counts[tgt] = call_target_counts.get(tgt, 0) + 1

        if _is_frame_prologue(word):
            prologues.add(base_va + off)

    starts.update(prologues)
    for tgt, count in call_target_counts.items():
        if tgt in prologues or count >= 2:
            starts.add(tgt)

    sorted_starts = sorted(starts)
    funcs = []
    for i, start in enumerate(sorted_starts):
        nxt = sorted_starts[i + 1] if i + 1 < len(sorted_starts) else hi
        size = max(4, min(nxt - start, 0x8000)) if nxt > start else 128
        funcs.append(
            {
                "id": i,
                "name": f"sub_{start:x}",
                "owner_class": "Global",
                "entry_va": int(start),
                "size": int(size),
                "code_section_va": int(base_va),
            }
        )
    return funcs


def _collect_libraries(strings: List[str]) -> List[str]:
    out: List[str] = []
    seen: Set[str] = set()
    for s in strings:
        if s.startswith("package:") and ".dart" in s and len(s) < 200 and s not in seen:
            seen.add(s)
            out.append(s)
    if not out:
        out = ["package:app/main.dart"]
    return out[:512]


def _selector_from_string(s: str) -> str | None:
    t = s.strip()
    if not t:
        return None
    if "@" in t:
        t = t.split("@", 1)[0]
    if ":" in t:
        t = t.split(":", 1)[1]
    t = t.strip()
    if not t:
        return None
    # Keep plausible selector chars, drop noisy payload.
    cleaned = "".join(ch for ch in t if ch.isalnum() or ch in "._$")
    if not cleaned:
        return None
    if len(cleaned) > 96:
        return None
    if not (cleaned[0].isalpha() or cleaned[0] in "_$"):
        return None
    return cleaned


def _pool_entries(strings: List[str]) -> List[dict]:
    entries = []
    for i, s in enumerate(strings):
        decoded_kind = "String"
        selector = _selector_from_string(s)
        library_uri = None
        if s.startswith("package:") and ".dart" in s:
            decoded_kind = "LibraryUri"
            library_uri = s
        elif selector is not None:
            decoded_kind = "SelectorString"

        entries.append(
            {
                "index": i,
                "kind": "String",
                "value": s,
                "decoded_kind": decoded_kind,
                "selector": selector,
                "target_va": None,
                "owner_class": None,
                "library_uri": library_uri,
            }
        )
    return entries


def entrypoint(default_snapshot_hash: str = "unknown", default_version: str = "unknown") -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--vm-data", required=True)
    p.add_argument("--isolate-data", required=True)
    p.add_argument("--vm-instr", required=True)
    p.add_argument("--isolate-instr", required=True)
    p.add_argument("--vm-instr-va", type=int, default=0)
    p.add_argument("--isolate-instr-va", type=int, default=0)
    p.add_argument("--out", required=True)
    args = p.parse_args()

    vm_data = _read_bytes(args.vm_data)
    iso_data = _read_bytes(args.isolate_data)
    iso_instr = _read_bytes(args.isolate_instr)

    strings = _extract_strings(vm_data + iso_data)
    snapshot_hash = _detect_snapshot_hash(vm_data, iso_data, default_snapshot_hash)

    libs = _collect_libraries(strings)
    funcs = _recover_functions(iso_instr, args.isolate_instr_va)
    if not funcs:
        funcs = [
            {
                "id": 0,
                "name": "entry",
                "owner_class": "Global",
                "entry_va": int(args.isolate_instr_va or 0x1000),
                "size": 128,
                "code_section_va": int(args.isolate_instr_va or 0x1000),
            }
        ]

    payload = {
        "schema_version": 2,
        "adapter_kind": "dynamic_snapshot_string_model_v1",
        "dart_version": default_version,
        "snapshot_hash": snapshot_hash,
        "arch": "arm64",
        "libraries": [{"id": i, "uri": lib, "name_display": lib} for i, lib in enumerate(libs)],
        "classes": [{"id": 0, "name": "Global", "super": "Object", "lib": libs[0]}],
        "functions": funcs,
        "object_pool": _pool_entries(strings),
    }

    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(payload, f, indent=2)

    return 0


if __name__ == "__main__":
    raise SystemExit(entrypoint())
