#!/usr/bin/env python3
from __future__ import annotations

import argparse
import fcntl
import json
import os
import re
import shlex
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple


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


def _decode_bl_target(pc: int, word: int) -> Optional[int]:
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
                "name_kind": "placeholder",
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


def _selector_from_string(s: str) -> Optional[str]:
    t = s.strip()
    if not t:
        return None
    lower = t.lower()
    if ".dart" in lower or "/" in t or "\\" in t or "://" in t:
        return None
    if " " in t:
        return None
    if "@" in t:
        t = t.split("@", 1)[0]
    if ":" in t:
        t = t.split(":", 1)[1]
    t = t.strip()
    if not t:
        return None
    cleaned = "".join(ch for ch in t if ch.isalnum() or ch in "._$")
    if not cleaned:
        return None
    if len(cleaned) > 96:
        return None
    if not (cleaned[0].isalpha() or cleaned[0] in "_$"):
        return None
    return cleaned


def _entrypoint_selector_from_name(name: str) -> Optional[str]:
    selector = _selector_from_string(name or "")
    if not selector:
        return None
    lower = selector.strip().lower()
    if (
        lower == "main"
        or lower.endswith(".main")
        or lower.endswith("::main")
        or lower.endswith("_main")
        or lower == "runapp"
        or lower.endswith(".runapp")
    ):
        return selector
    return None


def _bootflow_candidates_from_name(
    name: str, owner_class: str, library_uri: str
) -> List[Tuple[str, str, str]]:
    selector = _selector_from_string(name or "")
    if not selector:
        return []
    lower = selector.strip().lower()
    owner_lower = (owner_class or "").strip().lower()
    library_lower = (library_uri or "").strip().lower()
    out: List[Tuple[str, str, str]] = []

    if (
        lower == "main"
        or lower.endswith(".main")
        or lower.endswith("::main")
        or lower.endswith("_main")
    ):
        out.append(("EntryPointCandidate", selector, f"entrypoint:{selector}"))
        out.append(("BootMainCandidate", selector, f"bootflow:main:{selector}"))

    if lower == "runapp" or lower.endswith(".runapp"):
        out.append(("EntryPointCandidate", selector, f"entrypoint:{selector}"))
        out.append(("BootRunAppCandidate", selector, f"bootflow:runapp:{selector}"))

    if lower in {
        "didpushrouteinformation",
        "didpushroute",
        "didpoproute",
        "setnewroutepath",
        "parserouteinformation",
        "ongenerateroute",
        "onunknownroute",
        "onnewintent",
        "handleintent",
    }:
        out.append(("DeepLinkHandlerCandidate", selector, f"bootflow:deeplink:{selector}"))

    looks_activity_owner = "activity" in owner_lower or "flutterjni" in owner_lower
    looks_activity_lib = (
        "activity" in library_lower
        or "android" in library_lower
        or library_lower.startswith("package:flutter/src/embedding")
    )
    if lower in {"onnewintent", "handleintent"}:
        out.append(("ActivityHandlerCandidate", selector, f"bootflow:activity:{selector}"))
    elif lower in {"oncreate", "onstart", "onresume", "onpause", "onstop"} and (
        looks_activity_owner or looks_activity_lib
    ):
        out.append(("ActivityHandlerCandidate", selector, f"bootflow:activity:{selector}"))

    bootstrap_like_owner = (
        "binding" in owner_lower
        or "engine" in owner_lower
        or "jni" in owner_lower
        or "bootstrap" in owner_lower
        or owner_lower == "global"
        or owner_lower.endswith("binding")
    )
    bootstrap_like_library = (
        library_lower.endswith("/main.dart")
        or library_lower.startswith("package:flutter/")
        or "/bootstrap" in library_lower
        or "/engine" in library_lower
    )
    if lower in {
        "ensureinitialized",
        "nativeensureinitialized",
        "startinitialization",
        "ensureinitializationcomplete",
    } and (
        lower != "ensureinitialized"
        or bootstrap_like_owner
        or bootstrap_like_library
    ):
        out.append(("BootstrapInitCandidate", selector, f"bootflow:init:{selector}"))

    deduped: List[Tuple[str, str, str]] = []
    seen: Set[Tuple[str, str]] = set()
    for decoded_kind, sel, value in out:
        key = (decoded_kind, sel.lower())
        if key in seen:
            continue
        seen.add(key)
        deduped.append((decoded_kind, sel, value))
    return deduped


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
                "confidence": 0.4,
                "source": "internal",
            }
        )
    return entries


def _append_synthetic_pool_entries(base: List[dict], synthetic: List[dict]) -> List[dict]:
    out = list(base)
    next_index = len(out)
    seen: Set[Tuple[Optional[str], Optional[str], Optional[int], Optional[str], Optional[str]]] = set()
    for e in out:
        seen.add(
            (
                e.get("decoded_kind"),
                e.get("selector"),
                e.get("target_va"),
                e.get("owner_class"),
                e.get("library_uri"),
            )
        )

    for e in synthetic:
        key = (
            e.get("decoded_kind"),
            e.get("selector"),
            e.get("target_va"),
            e.get("owner_class"),
            e.get("library_uri"),
        )
        if key in seen:
            continue
        seen.add(key)
        entry = dict(e)
        entry["index"] = next_index
        next_index += 1
        out.append(entry)
    return out


def _normalize_library_uri(raw: str) -> Optional[str]:
    t = raw.strip()
    if not t:
        return None
    if t.startswith("package:") or t.startswith("dart:") or t.startswith("file:"):
        return t
    return None


def _sanitize_owner_class(raw: str) -> Optional[str]:
    t = raw.strip()
    if not t:
        return None
    t = re.sub(r"<.*?>", "", t)
    t = "".join(ch for ch in t if ch.isalnum() or ch in "_$")
    if not t:
        return None
    return t


def _extract_blutter_function_name(head_line: str) -> Optional[str]:
    text = head_line.strip()
    if not text.endswith("{"):
        return None
    text = text[:-1].strip()
    if not text or text.startswith("//"):
        return None

    for tag in ("[closure] ", "[ffi] "):
        if text.startswith(tag):
            text = text[len(tag) :].strip()

    changed = True
    while changed:
        changed = False
        for prefix in ("const ", "abstract ", "static ", "factory "):
            if text.startswith(prefix):
                text = text[len(prefix) :].strip()
                changed = True

    if text.startswith("set "):
        text = text[4:].strip()
    elif text.startswith("get "):
        text = text[4:].strip()

    left = text.split("(", 1)[0].strip()
    if not left:
        return None

    if left.startswith("_ "):
        left = left[2:].strip()

    if left.startswith("operator "):
        op = left.split("operator ", 1)[1].strip()
        return op or None

    token = left.split()[-1].strip()
    if not token:
        return None
    token = re.sub(r"<.*?>", "", token)
    token = token.rstrip("{")
    return token or None


def _parse_blutter_class_decl(line: str) -> Optional[Tuple[str, str]]:
    t = line.strip()
    if t == "class :: {":
        return ("Global", "Object")
    m = re.match(
        r"^(?:abstract class|class|enum)\s+([^\s<{]+)(?:<[^>]*>)?(?:\s+extends\s+([^\s<{]+))?",
        t,
    )
    if not m:
        return None
    cls = _sanitize_owner_class(m.group(1) or "")
    if not cls:
        return None
    sup = _sanitize_owner_class(m.group(2) or "Object") or "Object"
    return (cls, sup)


def _parse_blutter_function_ref(text: str) -> Optional[Tuple[str, str, str]]:
    m = re.search(r"\[([^\]]+)\]\s+([^:]+)::([^\s(]+)", text)
    if not m:
        return None
    lib = _normalize_library_uri(m.group(1) or "")
    owner = _sanitize_owner_class(m.group(2) or "")
    sel = _selector_from_string(m.group(3) or "")
    if not lib or not owner or not sel:
        return None
    return (lib, owner, sel)


def _parse_blutter_pp(pool_path: Path) -> List[dict]:
    if not pool_path.exists():
        return []

    entries: List[dict] = []
    line_re = re.compile(r"^\[pp\+0x([0-9a-fA-F]+)\]\s+(.*)$")
    unlink_re = re.compile(r"UnlinkedCall:\s*0x([0-9a-fA-F]+)\s*-\s*(.*)$")

    for raw_line in pool_path.read_text(encoding="utf-8", errors="ignore").splitlines():
        m = line_re.match(raw_line.strip())
        if not m:
            continue
        value = (m.group(2) or "").strip()
        decoded_kind = "BlutterPoolEntry"
        selector = None
        target_va = None
        owner_class = None
        library_uri = None

        m_unlink = unlink_re.search(value)
        if m_unlink:
            decoded_kind = "BlutterUnlinkedCall"
            try:
                target_va = int(m_unlink.group(1), 16)
            except Exception:
                target_va = None
            ref = _parse_blutter_function_ref(m_unlink.group(2) or "")
            if ref is not None:
                library_uri, owner_class, selector = ref
        else:
            ref = _parse_blutter_function_ref(value)
            if ref is not None:
                decoded_kind = "BlutterFunctionRef"
                library_uri, owner_class, selector = ref

        entries.append(
            {
                "index": len(entries),
                "kind": "String",
                "value": value,
                "decoded_kind": decoded_kind,
                "selector": selector,
                "target_va": target_va,
                "owner_class": owner_class,
                "library_uri": library_uri,
                "confidence": 0.8 if decoded_kind in ("BlutterUnlinkedCall", "BlutterFunctionRef") else 0.6,
                "source": "blutter",
            }
        )
    return entries


def _parse_blutter_asm(asm_dir: Path) -> Tuple[List[dict], List[dict], List[dict], List[dict]]:
    if not asm_dir.exists() or not asm_dir.is_dir():
        raise RuntimeError(f"blutter asm directory not found: {asm_dir}")

    header_re = re.compile(r"^//\s*lib:\s*.*?,\s*url:\s*(\S+)\s*$")
    addr_re = re.compile(r"//\s*\*\*\s*addr:\s*(0x[0-9a-fA-F]+),\s*size:\s*(0x[0-9a-fA-F]+)")

    libraries: Dict[str, dict] = {}
    classes: Dict[Tuple[str, str], dict] = {}
    functions: List[dict] = []
    seen_entry: Set[int] = set()
    entrypoint_candidates: List[dict] = []
    seen_bootflow_target: Set[Tuple[int, str, str]] = set()

    def ensure_library(uri: str) -> str:
        normalized = _normalize_library_uri(uri) or uri
        if normalized not in libraries:
            libraries[normalized] = {
                "id": len(libraries),
                "uri": normalized,
                "name_display": normalized,
            }
        return normalized

    def ensure_class(uri: str, cls: str, super_name: str = "Object") -> str:
        owner = _sanitize_owner_class(cls) or "Global"
        key = (uri, owner)
        if key not in classes:
            classes[key] = {
                "id": len(classes),
                "name": owner,
                "super": _sanitize_owner_class(super_name) or "Object",
                "lib": uri,
            }
        return owner

    for dart_file in sorted(asm_dir.rglob("*.dart")):
        current_lib = ensure_library("package:app/main.dart")
        current_class = ensure_class(current_lib, "Global", "Object")
        pending_name: Optional[str] = None
        pending_owner: str = current_class
        pending_lib: str = current_lib

        for line in dart_file.read_text(encoding="utf-8", errors="ignore").splitlines():
            m_header = header_re.match(line.strip())
            if m_header:
                current_lib = ensure_library(m_header.group(1).strip())
                current_class = ensure_class(current_lib, "Global", "Object")
                pending_name = None
                continue

            decl = _parse_blutter_class_decl(line)
            if decl is not None:
                cls_name, super_name = decl
                current_class = ensure_class(current_lib, cls_name, super_name)
                pending_name = None
                continue

            stripped = line.strip()
            if line.startswith("  ") and stripped.endswith("{") and not stripped.startswith("//"):
                head_name = _extract_blutter_function_name(line)
                if head_name:
                    pending_name = head_name
                    pending_owner = current_class
                    pending_lib = current_lib

            m_addr = addr_re.search(line)
            if not m_addr:
                continue
            try:
                entry = int(m_addr.group(1), 16)
                size = int(m_addr.group(2), 16)
            except Exception:
                continue
            if entry in seen_entry:
                pending_name = None
                continue
            seen_entry.add(entry)

            owner = ensure_class(pending_lib, pending_owner, "Object")
            name = pending_name or f"sub_{entry:x}"
            functions.append(
                {
                    "id": len(functions),
                    "name": name,
                    "owner_class": owner,
                    "entry_va": entry,
                    "size": max(size, 4),
                    "code_section_va": 0,
                    "name_kind": "placeholder" if name.startswith("sub_") else "heuristic",
                }
            )
            for decoded_kind, selector, value in _bootflow_candidates_from_name(
                name, owner, pending_lib
            ):
                key = (entry, decoded_kind, selector.lower())
                if key in seen_bootflow_target:
                    continue
                seen_bootflow_target.add(key)
                entrypoint_candidates.append(
                    {
                        "index": 0,
                        "kind": "String",
                        "value": value,
                        "decoded_kind": decoded_kind,
                        "selector": selector,
                        "target_va": int(entry),
                        "owner_class": owner,
                        "library_uri": pending_lib,
                        "confidence": 0.85,
                        "source": "synthetic",
                    }
                )
            pending_name = None

    libs = sorted(libraries.values(), key=lambda v: int(v["id"]))
    clss = sorted(classes.values(), key=lambda v: int(v["id"]))
    if not libs:
        libs = [{"id": 0, "uri": "package:app/main.dart", "name_display": "package:app/main.dart"}]
    if not clss:
        clss = [{"id": 0, "name": "Global", "super": "Object", "lib": libs[0]["uri"]}]
    return libs, clss, functions, entrypoint_candidates


def _runner_mode(cmd: List[str]) -> str:
    if not cmd:
        return ""
    exe = os.path.basename(cmd[0]).lower()
    if exe.endswith("blutter.py"):
        return "blutter_py"
    if exe == "blutter":
        return "blutter_bin"
    return "custom"


def _resolve_blutter_runner() -> Optional[List[str]]:
    env_cmd = os.getenv("FLUTTERDEC_BLUTTER_CMD", "").strip()
    if env_cmd:
        return shlex.split(env_cmd)

    env_py = os.getenv("FLUTTERDEC_BLUTTER_PY", "").strip()
    if env_py:
        py_exec = os.getenv("PYTHON", sys.executable or "python3")
        return [py_exec, env_py]

    found_py = shutil.which("blutter.py")
    if found_py:
        return [found_py]

    found_bin = shutil.which("blutter")
    if found_bin:
        return [found_bin]

    return None


def _run_blutter_dump(input_path: Optional[str], libapp_path: Optional[str]) -> Path:
    runner = _resolve_blutter_runner()
    if not runner:
        raise RuntimeError(
            "blutter runner not found. set FLUTTERDEC_BLUTTER_CMD or FLUTTERDEC_BLUTTER_PY, or install blutter.py"
        )

    out_dir = Path(tempfile.mkdtemp(prefix="flutterdec-blutter-"))
    mode = _runner_mode(runner)

    if mode == "blutter_py":
        if not input_path:
            raise RuntimeError("blutter.py backend needs --input-path")
        indir = input_path
        cmd = runner + [indir, str(out_dir), "--no-analysis"]
    elif mode == "blutter_bin":
        if not libapp_path:
            raise RuntimeError("blutter binary backend needs --libapp-path")
        cmd = runner + ["-i", libapp_path, "-o", str(out_dir)]
    else:
        if not input_path:
            raise RuntimeError("custom blutter backend needs --input-path")
        cmd = runner + [input_path, str(out_dir)]

    lock_dir = Path.home() / ".cache" / "flutterdec"
    lock_dir.mkdir(parents=True, exist_ok=True)
    lock_file = lock_dir / "blutter-run.lock"
    with lock_file.open("w") as lock_fp:
        fcntl.flock(lock_fp.fileno(), fcntl.LOCK_EX)
        proc = subprocess.run(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            shell=False,
        )
        fcntl.flock(lock_fp.fileno(), fcntl.LOCK_UN)
    if proc.returncode != 0:
        raise RuntimeError(
            "blutter failed with status {}\nstdout:\n{}\nstderr:\n{}".format(
                proc.returncode, proc.stdout, proc.stderr
            )
        )
    return out_dir


def _build_blutter_model(
    vm_data: bytes,
    iso_data: bytes,
    default_snapshot_hash: str,
    default_version: str,
    input_path: Optional[str],
    libapp_path: Optional[str],
) -> dict:
    blutter_out = _run_blutter_dump(input_path, libapp_path)
    asm_dir = blutter_out / "asm"
    libs, classes, funcs, entrypoint_candidates = _parse_blutter_asm(asm_dir)
    if not funcs:
        raise RuntimeError("blutter output did not contain recoverable functions")

    pool_entries = _parse_blutter_pp(blutter_out / "pp.txt")
    if not pool_entries:
        pool_entries = _pool_entries(_extract_strings(vm_data + iso_data))
    if entrypoint_candidates:
        pool_entries = _append_synthetic_pool_entries(pool_entries, entrypoint_candidates)

    snapshot_hash = _detect_snapshot_hash(vm_data, iso_data, default_snapshot_hash)
    return {
        "schema_version": 3,
        "adapter_kind": "blutter_bridge_model_v1",
        "dart_version": default_version,
        "snapshot_hash": snapshot_hash,
        "arch": "arm64",
        "libraries": libs,
        "classes": classes,
        "functions": funcs,
        "object_pool": pool_entries,
    }


def _normalize_backend(raw: str) -> str:
    t = (raw or "auto").strip().lower()
    if t in ("auto", "internal", "blutter"):
        return t
    return "auto"


def entrypoint(default_snapshot_hash: str = "unknown", default_version: str = "unknown") -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--vm-data", required=True)
    p.add_argument("--isolate-data", required=True)
    p.add_argument("--vm-instr", required=True)
    p.add_argument("--isolate-instr", required=True)
    p.add_argument("--vm-instr-va", type=int, default=0)
    p.add_argument("--isolate-instr-va", type=int, default=0)
    p.add_argument("--input-path")
    p.add_argument("--libapp-path")
    p.add_argument("--out", required=True)
    args = p.parse_args()

    vm_data = _read_bytes(args.vm_data)
    iso_data = _read_bytes(args.isolate_data)
    iso_instr = _read_bytes(args.isolate_instr)

    backend = _normalize_backend(os.getenv("FLUTTERDEC_ADAPTER_BACKEND", "auto"))
    if backend in ("auto", "blutter"):
        try:
            payload = _build_blutter_model(
                vm_data,
                iso_data,
                default_snapshot_hash,
                default_version,
                args.input_path,
                args.libapp_path,
            )
            with open(args.out, "w", encoding="utf-8") as f:
                json.dump(payload, f, indent=2)
            return 0
        except Exception as exc:
            if backend == "blutter":
                raise
            print(
                f"[adapter] blutter backend unavailable, fallback to internal: {exc}",
                file=sys.stderr,
            )

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
                "name_kind": "heuristic",
            }
        ]

    payload = {
        "schema_version": 3,
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
