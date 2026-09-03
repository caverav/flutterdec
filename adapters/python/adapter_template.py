#!/usr/bin/env python3
"""Checked-in reference producer for the flutterdec adapter boundary.

One invocation reads an adapter protocol v1 request, runs exactly one backend,
and writes a ProgramModel v4 plus a protocol v1 result. There is no v2/v3 path.

The rule every backend here follows is that an unrecovered fact is absent, not
invented. A function whose name was not recovered has no name; a class whose
library was not recovered has no library; a producer that carved strings out of
the data image says its pool indexes are ordinal and its index space is
unavailable, rather than handing the host positions that look like `ObjectPool`
entries. Every domain that came back empty carries a diagnostic saying so, so
"nothing was there" and "we did not look" stay distinguishable.
"""
from __future__ import annotations

import argparse
import fcntl
import hashlib
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

PROTOCOL_MAJOR = 1
MODEL_VERSION = 4

VM_DATA = "vm_data"
ISOLATE_DATA = "isolate_data"
VM_INSTRUCTIONS = "vm_instructions"
ISOLATE_INSTRUCTIONS = "isolate_instructions"

DOMAINS = (
    "libraries",
    "classes",
    "class_relationships",
    "functions",
    "function_names",
    "object_pool",
    "pool_index_space",
)

COMPLETE = "complete"
PARTIAL = "partial"
UNAVAILABLE = "unavailable"

EXACT = "exact"
DERIVED = "derived"
HEURISTIC = "heuristic"

# Mirrors `validate::PLACEHOLDER_NAMES`. A carved string that is one of these is
# an admission of ignorance wearing a value's clothes, and the host rejects the
# whole model over one of them, so they are filtered at the source.
PLACEHOLDER_NAMES = frozenset(
    [
        "",
        "-",
        "?",
        "??",
        "???",
        "n/a",
        "na",
        "none",
        "null",
        "nil",
        "todo",
        "tbd",
        "unknown",
        "<unknown>",
        "unnamed",
        "anonymous",
        "placeholder",
        "undefined",
    ]
)


class BackendUnavailable(Exception):
    """The backend's tooling is not installed. Distinct from it failing."""


class BackendFailed(Exception):
    """The backend ran and could not produce a model."""


def _is_placeholder(text: str) -> bool:
    return text.strip().lower() in PLACEHOLDER_NAMES


def _usable_value(text: str) -> bool:
    return bool(text) and not _is_placeholder(text)


# --------------------------------------------------------------------------
# protocol v1
# --------------------------------------------------------------------------


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _load_request(path: Path) -> dict:
    try:
        request = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f"request is not readable JSON: {exc}") from exc
    if request.get("protocol_major") != PROTOCOL_MAJOR:
        raise ValueError(
            f"unsupported protocol major {request.get('protocol_major')!r}; this adapter implements {PROTOCOL_MAJOR}"
        )
    if request.get("model_major") != MODEL_VERSION:
        raise ValueError(
            f"unsupported model major {request.get('model_major')!r}; this adapter emits {MODEL_VERSION}"
        )
    for key in ("identity", "compatibility", "producer", "inputs", "output", "requested_backend"):
        if key not in request:
            raise ValueError(f"request is missing {key}")
    return request


def _handle(request: dict, region: str) -> dict:
    for handle in request["inputs"]:
        if handle.get("region") == region:
            return handle
    raise ValueError(f"request omits input region {region}")


def _read_region(request: dict, region: str) -> bytes:
    handle = _handle(request, region)
    path = Path(handle["path"])
    try:
        data = path.read_bytes()
    except OSError as exc:
        raise FileNotFoundError(f"input {region} is not readable: {exc}") from exc
    # The digest is the point of the handle: without it "the adapter read the
    # snapshot" only means "the adapter read a file".
    if len(data) != handle["size"]:
        raise ValueError(
            f"input {region} is {len(data)} bytes, request declared {handle['size']}"
        )
    actual = _sha256(data)
    if actual != handle["sha256"]:
        raise ValueError(
            f"input {region} digest {actual} does not match declared {handle['sha256']}"
        )
    return data


def _model_regions(request: dict) -> List[dict]:
    """Echo the host's region table back verbatim.

    Anything else here is the adapter disagreeing with the host about what it
    was given, which the host rejects rather than reconciles.
    """
    return [
        {
            "region": h["region"],
            "size": h["size"],
            "sha256": h["sha256"],
            "virtual_address": h.get("virtual_address"),
            "executable": h["executable"],
        }
        for h in request["inputs"]
    ]


def _region_va(request: dict, region: str) -> int:
    return int(_handle(request, region).get("virtual_address") or 0)


def _write_result(
    path: Path,
    status: str,
    model: Optional[str] = None,
    error: Optional[dict] = None,
    resolved_backend: Optional[str] = None,
    fallback_reason: Optional[str] = None,
    diagnostics: Optional[List[dict]] = None,
) -> None:
    payload = {
        "protocol_major": PROTOCOL_MAJOR,
        "model_major": MODEL_VERSION,
        "status": status,
        "model": model,
        "error": error,
        "resolved_backend": resolved_backend,
        "fallback_reason": fallback_reason,
        "diagnostics": diagnostics or [],
    }
    path.write_text(json.dumps(payload, indent=2), encoding="utf-8")


# --------------------------------------------------------------------------
# model v4 assembly
# --------------------------------------------------------------------------


def _diagnostic(code: str, message: str, subject: Optional[str] = None,
                severity: str = "warning") -> dict:
    return {"code": code, "severity": severity, "subject": subject, "message": message}


def _name(text: str, provenance: str) -> dict:
    # `confidence` stays null everywhere in this file. A number here would look
    # calibrated, and none of these backends has anything to calibrate against.
    return {"text": text, "provenance": provenance, "confidence": None}


def _build_model(
    request: dict,
    capabilities: Dict[str, str],
    libraries: List[dict],
    classes: List[dict],
    functions: List[dict],
    object_pool: dict,
    diagnostics: List[dict],
) -> dict:
    """Assemble a v4 model and add the diagnostic every unavailable domain owes.

    Validation rejects an unavailable domain with nothing said about it, which
    is deliberate: silence is indistinguishable from a producer that forgot to
    look, and this is the one place that can tell the difference.
    """
    explained = {d.get("subject") for d in diagnostics}
    for domain in DOMAINS:
        if capabilities[domain] == UNAVAILABLE and domain not in explained:
            diagnostics.append(
                _diagnostic(
                    "domain_not_recovered",
                    f"this backend recovered no {domain.replace('_', ' ')}",
                    subject=domain,
                )
            )
    return {
        "model_version": MODEL_VERSION,
        "producer": request["producer"],
        "input": {
            "identity": request["identity"],
            "regions": _model_regions(request),
        },
        "compatibility": request["compatibility"],
        "capabilities": capabilities,
        "libraries": sorted(libraries, key=lambda v: v["id"]),
        "classes": sorted(classes, key=lambda v: v["id"]),
        "functions": sorted(functions, key=lambda v: v["id"]),
        "object_pool": object_pool,
        "diagnostics": diagnostics,
        "extensions": {},
    }


def _ordinal_pool(entries: List[dict]) -> dict:
    """A pool whose indexes are positions in a list, not hardware slots.

    `geometry` is absent and the index space is separately reported unavailable,
    so a `ldr xN, [x27, #disp]` cannot be resolved through these.
    """
    return {"index_space": "ordinal", "geometry": None, "entries": entries}


def _empty_pool() -> dict:
    return _ordinal_pool([])


# --------------------------------------------------------------------------
# shared extraction
# --------------------------------------------------------------------------


def _extract_strings(data: bytes, min_len: int = 5, max_items: int = 20000) -> List[str]:
    out: List[str] = []
    for m in re.finditer(rb"[ -~]{%d,}" % min_len, data):
        s = m.group(0).decode("utf-8", errors="ignore")
        if len(s) > 220:
            continue
        if not _usable_value(s):
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


def _library_uris(strings: List[str]) -> List[str]:
    """Library URIs that appear as strings in the image.

    There is no fallback. A snapshot whose data image contains no `package:` URI
    yields no libraries, because the alternative is emitting one library named
    after an app that may not exist.
    """
    out: List[str] = []
    seen: Set[str] = set()
    for s in strings:
        if s.startswith("package:") and ".dart" in s and len(s) < 200 and s not in seen:
            seen.add(s)
            out.append(s)
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


def _normalize_library_uri(raw: str) -> Optional[str]:
    t = raw.strip()
    if not t:
        return None
    if t.startswith("package:") or t.startswith("dart:") or t.startswith("file:"):
        return t
    return None


def _sanitize_class_name(raw: str) -> Optional[str]:
    t = raw.strip()
    if not t:
        return None
    t = re.sub(r"<.*?>", "", t)
    t = "".join(ch for ch in t if ch.isalnum() or ch in "_$")
    if not t or _is_placeholder(t):
        return None
    return t


# --------------------------------------------------------------------------
# internal backend: string carving plus prologue scanning
# --------------------------------------------------------------------------


def _decode_bl_target(pc: int, word: int) -> Optional[int]:
    if ((word >> 26) & 0x3F) != 0b100101:
        return None
    imm26 = word & 0x03FFFFFF
    if imm26 & (1 << 25):
        imm26 -= 1 << 26
    return pc + (imm26 << 2)


def _is_frame_prologue(word: int) -> bool:
    rt = word & 0x1F
    rn = (word >> 5) & 0x1F
    rt2 = (word >> 10) & 0x1F
    is_store_pair = ((word >> 30) & 0x3) == 0b10
    return is_store_pair and rt == 29 and rt2 == 30 and rn == 31


def _recover_code_ranges(instr: bytes, base_va: int) -> List[dict]:
    """Code ranges from frame prologues and call targets.

    Every range here is a guess about where a function begins, so each one is
    `heuristic` and none of them carries a name: there is no name evidence in a
    prologue, and `sub_1234` was never one.
    """
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

    sorted_starts = sorted(s for s in starts if base_va <= s < hi)
    out: List[dict] = []
    for i, start in enumerate(sorted_starts):
        nxt = sorted_starts[i + 1] if i + 1 < len(sorted_starts) else hi
        size = min(nxt - start, 0x8000)
        if size <= 0:
            continue
        out.append(
            {
                "id": len(out),
                "name": None,
                "owner": None,
                "code": {"start_va": int(start), "size": int(size)},
                "code_section_va": int(base_va),
                "provenance": HEURISTIC,
            }
        )
    return out


def _build_internal_model(request: dict) -> dict:
    vm_data = _read_region(request, VM_DATA)
    iso_data = _read_region(request, ISOLATE_DATA)
    iso_instr = _read_region(request, ISOLATE_INSTRUCTIONS)
    iso_va = _region_va(request, ISOLATE_INSTRUCTIONS)

    strings = _extract_strings(vm_data + iso_data)
    uris = _library_uris(strings)
    functions = _recover_code_ranges(iso_instr, iso_va)
    entries = [
        {
            "index": i,
            "kind": "string",
            "value": s,
            "target_va": None,
            "provenance": HEURISTIC,
            "confidence": None,
        }
        for i, s in enumerate(strings)
    ]

    capabilities = {
        "libraries": PARTIAL if uris else UNAVAILABLE,
        "classes": UNAVAILABLE,
        "class_relationships": UNAVAILABLE,
        "functions": PARTIAL if functions else UNAVAILABLE,
        "function_names": UNAVAILABLE,
        "object_pool": PARTIAL if entries else UNAVAILABLE,
        "pool_index_space": UNAVAILABLE,
    }
    diagnostics = [
        _diagnostic(
            "domain_not_recovered",
            "this backend does not deserialize the snapshot, so no class table is reachable",
            subject="classes",
        ),
        _diagnostic(
            "domain_not_recovered",
            "no function names are recoverable from instruction bytes alone",
            subject="function_names",
        ),
        _diagnostic(
            "domain_not_recovered",
            "entries are carved strings in carve order, so no ObjectPool index space was established",
            subject="pool_index_space",
        ),
    ]
    if functions:
        diagnostics.insert(
            0,
            _diagnostic(
                "domain_heuristic_only",
                "code ranges come from frame-prologue and call-target scanning, not from a snapshot parser",
                subject="functions",
            ),
        )

    return _build_model(
        request,
        capabilities,
        libraries=[
            {"id": i, "uri": uri, "display_name": None, "provenance": HEURISTIC}
            for i, uri in enumerate(uris)
        ],
        classes=[],
        functions=functions,
        object_pool=_ordinal_pool(entries),
        diagnostics=diagnostics,
    )


# --------------------------------------------------------------------------
# blutter backend
# --------------------------------------------------------------------------


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

    if text.startswith("set ") or text.startswith("get "):
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
    if not _usable_value(token):
        return None
    return token


def _parse_blutter_class_decl(line: str) -> Optional[Tuple[Optional[str], Optional[str]]]:
    """`(class, super)` for a class declaration, or `None` for a non-declaration.

    `class :: {` is blutter's header for a library's top-level members. It is not
    a class, so it returns `(None, None)`: the functions under it have no owner
    rather than an owner called `Global`.
    """
    t = line.strip()
    if t == "class :: {":
        return (None, None)
    m = re.match(
        r"^(?:abstract class|class|enum)\s+([^\s<{]+)(?:<[^>]*>)?(?:\s+extends\s+([^\s<{]+))?",
        t,
    )
    if not m:
        return None
    cls = _sanitize_class_name(m.group(1) or "")
    if not cls:
        return None
    return (cls, _sanitize_class_name(m.group(2) or ""))


def _parse_blutter_pp(pool_path: Path) -> List[str]:
    """Pool slot text in the order blutter printed it.

    Blutter prints `[pp+0xNN]`, so a hardware index is arguably derivable from
    the documented ARM64 AOT layout. It is not derived here: this producer has
    no way to confirm those displacements are PP-relative for the snapshot in
    hand, and a wrong index space silently mis-resolves every pool reference.
    The entries stay ordinal and the index space stays unavailable.
    """
    if not pool_path.exists():
        return []
    line_re = re.compile(r"^\[pp\+0x[0-9a-fA-F]+\]\s+(.*)$")
    out: List[str] = []
    for raw_line in pool_path.read_text(encoding="utf-8", errors="ignore").splitlines():
        m = line_re.match(raw_line.strip())
        if not m:
            continue
        value = (m.group(1) or "").strip()
        if _usable_value(value):
            out.append(value)
    return out


def _parse_blutter_asm(asm_dir: Path) -> Tuple[List[dict], List[dict], List[dict]]:
    if not asm_dir.exists() or not asm_dir.is_dir():
        raise BackendFailed(f"blutter asm directory not found: {asm_dir}")

    header_re = re.compile(r"^//\s*lib:\s*.*?,\s*url:\s*(\S+)\s*$")
    addr_re = re.compile(r"//\s*\*\*\s*addr:\s*(0x[0-9a-fA-F]+),\s*size:\s*(0x[0-9a-fA-F]+)")

    library_ids: Dict[str, int] = {}
    class_ids: Dict[Tuple[Optional[int], str], int] = {}
    class_supers: Dict[int, str] = {}
    class_names: Dict[str, int] = {}
    functions: List[dict] = []
    seen_entry: Set[int] = set()

    def ensure_library(uri: str) -> Optional[int]:
        normalized = _normalize_library_uri(uri)
        if normalized is None:
            return None
        if normalized not in library_ids:
            library_ids[normalized] = len(library_ids)
        return library_ids[normalized]

    def ensure_class(library: Optional[int], name: str, super_name: Optional[str]) -> int:
        key = (library, name)
        if key not in class_ids:
            class_ids[key] = len(class_ids)
            class_names.setdefault(name, class_ids[key])
        if super_name:
            class_supers[class_ids[key]] = super_name
        return class_ids[key]

    for dart_file in sorted(asm_dir.rglob("*.dart")):
        current_lib: Optional[int] = None
        current_class: Optional[int] = None
        pending_name: Optional[str] = None
        pending_owner: Optional[int] = None

        for line in dart_file.read_text(encoding="utf-8", errors="ignore").splitlines():
            m_header = header_re.match(line.strip())
            if m_header:
                current_lib = ensure_library(m_header.group(1).strip())
                current_class = None
                pending_name = None
                continue

            decl = _parse_blutter_class_decl(line)
            if decl is not None:
                cls_name, super_name = decl
                current_class = (
                    ensure_class(current_lib, cls_name, super_name) if cls_name else None
                )
                pending_name = None
                continue

            stripped = line.strip()
            if line.startswith("  ") and stripped.endswith("{") and not stripped.startswith("//"):
                head_name = _extract_blutter_function_name(line)
                if head_name:
                    pending_name = head_name
                    pending_owner = current_class

            m_addr = addr_re.search(line)
            if not m_addr:
                continue
            try:
                entry = int(m_addr.group(1), 16)
                size = int(m_addr.group(2), 16)
            except ValueError:
                continue
            if entry in seen_entry:
                pending_name = None
                continue
            seen_entry.add(entry)

            functions.append(
                {
                    "id": len(functions),
                    # Scraped out of blutter's rendered source, so a guess about
                    # the text, never `exact`.
                    "name": _name(pending_name, HEURISTIC) if pending_name else None,
                    "owner": pending_owner if pending_name else current_class,
                    "code": {"start_va": entry, "size": max(size, 1)},
                    "code_section_va": 0,
                    "provenance": DERIVED,
                }
            )
            pending_name = None
            pending_owner = None

    libraries = [
        {"id": lid, "uri": uri, "display_name": None, "provenance": DERIVED}
        for uri, lid in library_ids.items()
    ]
    classes = []
    for (library, name), cid in class_ids.items():
        super_name = class_supers.get(cid)
        super_id = class_names.get(super_name) if super_name else None
        classes.append(
            {
                "id": cid,
                "name": name,
                "library": library,
                # Only an edge whose target is a class we actually recovered.
                "super_class": super_id if super_id is not None and super_id != cid else None,
                "provenance": DERIVED,
            }
        )
    return libraries, classes, functions


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
        raise BackendUnavailable(
            "blutter runner not found. set FLUTTERDEC_BLUTTER_CMD or FLUTTERDEC_BLUTTER_PY, or install blutter.py"
        )

    out_dir = Path(tempfile.mkdtemp(prefix="flutterdec-blutter-"))
    mode = _runner_mode(runner)

    if mode == "blutter_py":
        if not input_path:
            raise BackendFailed("blutter.py backend needs --input-path")
        cmd = runner + [input_path, str(out_dir), "--no-analysis"]
    elif mode == "blutter_bin":
        if not libapp_path:
            raise BackendFailed("blutter binary backend needs --libapp-path")
        cmd = runner + ["-i", libapp_path, "-o", str(out_dir)]
    else:
        if not input_path:
            raise BackendFailed("custom blutter backend needs --input-path")
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
        raise BackendFailed(
            "blutter failed with status {}\nstdout:\n{}\nstderr:\n{}".format(
                proc.returncode, proc.stdout, proc.stderr
            )
        )
    return out_dir


def _build_blutter_model(request: dict, input_path: Optional[str],
                         libapp_path: Optional[str]) -> dict:
    blutter_out = _run_blutter_dump(input_path, libapp_path)
    libraries, classes, functions = _parse_blutter_asm(blutter_out / "asm")
    if not functions:
        raise BackendFailed("blutter output did not contain recoverable code ranges")

    # Blutter reports addresses in the isolate instruction image, which is the
    # region the host declared; the model has to name the region base it means.
    iso_va = _region_va(request, ISOLATE_INSTRUCTIONS)
    iso_size = _handle(request, ISOLATE_INSTRUCTIONS)["size"]
    kept: List[dict] = []
    dropped = 0
    for f in functions:
        start = f["code"]["start_va"]
        if not (iso_va <= start < iso_va + iso_size):
            dropped += 1
            continue
        f["code"]["size"] = min(f["code"]["size"], iso_va + iso_size - start)
        f["code_section_va"] = iso_va
        f["id"] = len(kept)
        kept.append(f)
    if not kept:
        raise BackendFailed(
            "no blutter code range fell inside the isolate instruction region the host declared"
        )

    pool_values = _parse_blutter_pp(blutter_out / "pp.txt")
    entries = [
        {
            "index": i,
            "kind": "string",
            "value": value,
            "target_va": None,
            "provenance": HEURISTIC,
            "confidence": None,
        }
        for i, value in enumerate(pool_values)
    ]

    named = sum(1 for f in kept if f["name"] is not None)
    has_supers = any(c["super_class"] is not None for c in classes)
    diagnostics = [
        _diagnostic(
            "domain_heuristic_only",
            "function names are scraped from blutter's rendered source, not read from the snapshot",
            subject="function_names",
        )
    ]
    if dropped:
        diagnostics.append(
            _diagnostic(
                "record_discarded",
                f"{dropped} blutter code ranges fell outside the declared isolate instruction region",
                subject="functions",
            )
        )
    if entries:
        diagnostics.append(
            _diagnostic(
                "domain_not_recovered",
                "blutter prints pool slots in dump order; no ObjectPool index space was established",
                subject="pool_index_space",
            )
        )

    capabilities = {
        "libraries": PARTIAL if libraries else UNAVAILABLE,
        "classes": PARTIAL if classes else UNAVAILABLE,
        "class_relationships": PARTIAL if (classes and has_supers) else UNAVAILABLE,
        "functions": PARTIAL,
        "function_names": PARTIAL if named else UNAVAILABLE,
        "object_pool": PARTIAL if entries else UNAVAILABLE,
        "pool_index_space": UNAVAILABLE,
    }
    if capabilities["function_names"] == UNAVAILABLE:
        diagnostics = [d for d in diagnostics if d.get("subject") != "function_names"]
    if not classes:
        classes = []
    return _build_model(
        request,
        capabilities,
        libraries=libraries,
        classes=classes,
        functions=kept,
        object_pool=_ordinal_pool(entries),
        diagnostics=diagnostics,
    )


# --------------------------------------------------------------------------
# r2flutter backend
# --------------------------------------------------------------------------


def _resolve_r2flutter_runner() -> Optional[List[str]]:
    env_cmd = os.getenv("FLUTTERDEC_R2FLUTTER_CMD", "").strip()
    if env_cmd:
        return shlex.split(env_cmd)
    env_bin = os.getenv("FLUTTERDEC_R2FLUTTER_BIN", "").strip()
    if env_bin:
        return [env_bin]
    found = shutil.which("r2flutter")
    if found:
        return [found]
    return None


def _r2flutter_timeout() -> int:
    raw = os.getenv("FLUTTERDEC_R2FLUTTER_TIMEOUT", "").strip()
    try:
        return max(1, int(raw)) if raw else 900
    except ValueError:
        return 900


def _r2flutter_json(runner: List[str], target: str, flag: str):
    """Run one r2flutter action and parse its JSON.

    r2flutter emits one action per invocation and writes radare2 loader warnings
    to stderr, so stdout is parsed on its own. Several invocations happen per
    model build and each one loads the whole binary, so a wedged radare2 would
    otherwise hang the adapter, and the core waiting on it, indefinitely.
    """
    try:
        proc = subprocess.run(
            [*runner, flag, target],
            capture_output=True,
            text=True,
            shell=False,
            check=False,
            timeout=_r2flutter_timeout(),
        )
    except OSError as exc:
        raise BackendUnavailable(f"could not launch r2flutter ({' '.join(runner)}): {exc}") from exc
    except subprocess.TimeoutExpired as exc:
        raise BackendFailed(f"r2flutter {flag} timed out after {exc.timeout}s") from exc
    if proc.returncode != 0:
        raise BackendFailed(
            f"r2flutter {flag} failed ({proc.returncode}): {proc.stderr.strip()[:400]}"
        )
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        raise BackendFailed(f"r2flutter {flag} did not emit JSON: {exc}") from exc


_R2F_POOL_ENTRY_RE = re.compile(r"\bentry=(\d+)\b")
_R2F_IT_METHOD_RE = re.compile(r"^method\.(?:(?P<owner>.+)\.)?(?P<name>[^.]+)$")


def _r2flutter_functions(instruction_table: dict, iso_va: int, iso_size: int,
                         class_ids: Dict[str, int]) -> Tuple[List[dict], int]:
    """Map the AOT instruction table onto v4 functions.

    Entry addresses and names come out of the snapshot, so a name that is there
    is `exact`. Sizes are not serialized: the gap to the next entry is the usual
    approximation, which is why the record's own provenance is `derived` even
    when its name is not.
    """
    entries = sorted(
        (e for e in instruction_table.get("entries", []) if e.get("address")),
        key=lambda e: e["address"],
    )
    out: List[dict] = []
    dropped = 0
    for i, e in enumerate(entries):
        start = int(e["address"])
        if not (iso_va <= start < iso_va + iso_size):
            dropped += 1
            continue
        nxt = int(entries[i + 1]["address"]) if i + 1 < len(entries) else iso_va + iso_size
        size = min(max(nxt - start, 4), 0x8000, iso_va + iso_size - start)
        raw_name = (e.get("name") or "").strip()
        owner = None
        name = None
        if _usable_value(raw_name):
            m = _R2F_IT_METHOD_RE.match(raw_name)
            if m and m.group("owner"):
                owner = class_ids.get(m.group("owner"))
            name = _name(raw_name, EXACT)
        out.append(
            {
                "id": len(out),
                "name": name,
                "owner": owner,
                "code": {"start_va": start, "size": int(size)},
                "code_section_va": iso_va,
                "provenance": DERIVED,
            }
        )
    return out, dropped


def _r2flutter_super_name(value) -> Optional[str]:
    """Superclass name from r2flutter metadata, or None when it is unresolved.

    r2flutter emits `super` as an object that may carry `ref`, `type_ref` and
    `name`. Only `name` is a recovered name: r2flutter fills it in itself when
    the reference resolves, so a bare `ref`/`type_ref` means its own lookup
    failed. v4 reports that as no superclass rather than inventing `Object`.
    """
    if isinstance(value, str):
        return _sanitize_class_name(value)
    if isinstance(value, dict):
        name = value.get("name")
        if isinstance(name, str):
            return _sanitize_class_name(name)
    return None


def _r2flutter_classes(classes: List[dict]) -> Tuple[List[dict], Dict[str, int]]:
    """Project r2flutter classes onto v4 classes.

    r2flutter does not attribute classes to libraries, so `library` stays null
    rather than pointing at a URI recovered from somewhere else entirely.
    """
    named: List[str] = []
    seen: Set[str] = set()
    for c in classes:
        name = _sanitize_class_name(c.get("name") or "")
        if not name or name in seen:
            continue
        seen.add(name)
        named.append(name)
    ids = {name: i for i, name in enumerate(named)}
    out = []
    for name in named:
        raw_super = next(
            (c.get("super") for c in classes if _sanitize_class_name(c.get("name") or "") == name),
            None,
        )
        super_name = _r2flutter_super_name(raw_super)
        super_id = ids.get(super_name) if super_name else None
        out.append(
            {
                "id": ids[name],
                "name": name,
                "library": None,
                "super_class": super_id if super_id is not None and super_id != ids[name] else None,
                "provenance": EXACT,
            }
        )
    return out, ids


def _r2flutter_pool(strings: List[dict]) -> List[dict]:
    """ObjectPool entries keyed by the real entry index.

    r2flutter reports, per string, the pool slots that reference it as
    `pool=<ref> index=<n> entry=<E> pp_off=<disp>`. `entry` is the authoritative
    index a `ldr xN, [x27, #pp_off]` resolves to, which is exactly the key the
    decompiler joins on. Nothing here is positional or guessed.
    """
    by_index: Dict[int, dict] = {}
    for s in strings:
        value = s.get("value")
        if not isinstance(value, str) or not _usable_value(value):
            continue
        kind = "selector" if _selector_from_string(value) else "string"
        for ref in s.get("refs", []):
            if ref.get("kind") != "object_pool.entry":
                continue
            m = _R2F_POOL_ENTRY_RE.search(ref.get("name") or "")
            if not m:
                continue
            index = int(m.group(1))
            # One slot holds one object. A second claim on the same index is a
            # contradiction, and the host rejects duplicates outright.
            by_index.setdefault(
                index,
                {
                    "index": index,
                    "kind": kind,
                    "value": value,
                    "target_va": None,
                    "provenance": EXACT,
                    "confidence": None,
                },
            )
    return [by_index[i] for i in sorted(by_index)]


def _build_r2flutter_model(request: dict, input_path: Optional[str],
                           libapp_path: Optional[str]) -> dict:
    runner = _resolve_r2flutter_runner()
    if runner is None:
        raise BackendUnavailable(
            "r2flutter not found; set FLUTTERDEC_R2FLUTTER_CMD or FLUTTERDEC_R2FLUTTER_BIN, "
            "or put r2flutter on PATH"
        )
    target = libapp_path or input_path
    if not target:
        raise BackendFailed("r2flutter backend needs --libapp-path or --input-path")

    iso_va = _region_va(request, ISOLATE_INSTRUCTIONS)
    iso_size = _handle(request, ISOLATE_INSTRUCTIONS)["size"]

    classes, class_ids = _r2flutter_classes(_r2flutter_json(runner, target, "-jc"))
    instruction_table = _r2flutter_json(runner, target, "-ji")
    functions, dropped = _r2flutter_functions(instruction_table, iso_va, iso_size, class_ids)
    if not functions:
        raise BackendFailed("r2flutter recovered no instruction-table entries in the declared region")

    # `-jxz` is the reliable pool-referenced string set with its slot
    # back-references; that is what the pool index space needs.
    pool_strings = _r2flutter_json(runner, target, "-jxz")
    pool_entries = _r2flutter_pool(pool_strings)

    # Library URIs mostly live in the data image rather than the pool, so the
    # wider carved set is the only place to find them. They drive
    # `--function-scope` and package prioritisation, not naming, which is why a
    # carved URI is `heuristic` while an instruction-table name is not.
    try:
        all_strings = _r2flutter_json(runner, target, "-jzz")
    except BackendFailed:
        all_strings = pool_strings
    uris = _library_uris(
        [s.get("value", "") for s in all_strings if isinstance(s.get("value"), str)]
    )
    libraries = [
        {"id": i, "uri": uri, "display_name": None, "provenance": HEURISTIC}
        for i, uri in enumerate(uris)
    ]

    # Only claim an authoritative pool index space when the ObjectPool image was
    # actually reconstructed. r2flutter reports an error for snapshots whose pool
    # fill payload it cannot decode, and a guessed geometry there would silently
    # mis-resolve every pool reference.
    geometry = None
    try:
        pp = _r2flutter_json(runner, target, "-jp")
        if isinstance(pp, dict) and "entries_offset" in pp and "word_size" in pp:
            geometry = {
                "entries_offset": int(pp["entries_offset"]),
                "word_size": int(pp["word_size"]),
            }
    except BackendFailed:
        geometry = None

    diagnostics: List[dict] = []
    if dropped:
        diagnostics.append(
            _diagnostic(
                "record_discarded",
                f"{dropped} instruction-table entries fell outside the declared isolate instruction region",
                subject="functions",
            )
        )
    if geometry is None:
        pool_entries = []
        diagnostics.append(
            _diagnostic(
                "domain_not_recovered",
                "r2flutter could not reconstruct the ObjectPool image, so no entry is addressable",
                subject="object_pool",
            )
        )
        object_pool = _empty_pool()
    elif not pool_entries:
        # Geometry without a single entry describes an index space nothing
        # occupies; claiming `hardware` there would be a claim about no data.
        object_pool = _empty_pool()
        diagnostics.append(
            _diagnostic(
                "domain_not_recovered",
                "r2flutter reconstructed the pool image but no slot referenced a usable string",
                subject="object_pool",
            )
        )
    else:
        object_pool = {
            "index_space": "hardware",
            "geometry": geometry,
            "entries": pool_entries,
        }
    if uris:
        diagnostics.append(
            _diagnostic(
                "domain_heuristic_only",
                "library URIs are carved from the data image, not read from a library table",
                subject="libraries",
            )
        )

    unnamed = sum(1 for f in functions if f["name"] is None)
    has_supers = any(c["super_class"] is not None for c in classes)
    capabilities = {
        "libraries": PARTIAL if libraries else UNAVAILABLE,
        "classes": PARTIAL if classes else UNAVAILABLE,
        "class_relationships": PARTIAL if (classes and has_supers) else UNAVAILABLE,
        # Sizes are the gap to the next entry, so the domain is never complete.
        "functions": PARTIAL,
        "function_names": UNAVAILABLE
        if unnamed == len(functions)
        else PARTIAL,
        "object_pool": PARTIAL if pool_entries else UNAVAILABLE,
        "pool_index_space": COMPLETE if geometry is not None and pool_entries else UNAVAILABLE,
    }
    return _build_model(
        request,
        capabilities,
        libraries=libraries,
        classes=classes,
        functions=functions,
        object_pool=object_pool,
        diagnostics=diagnostics,
    )


# --------------------------------------------------------------------------
# entrypoint
# --------------------------------------------------------------------------

BACKEND_ORDER = ("r2flutter", "blutter", "internal")

BUILDERS = {
    "r2flutter": _build_r2flutter_model,
    "blutter": _build_blutter_model,
    "internal": lambda request, _input_path, _libapp_path: _build_internal_model(request),
}


def _run_backend(name: str, request: dict, input_path: Optional[str],
                 libapp_path: Optional[str]) -> dict:
    return BUILDERS[name](request, input_path, libapp_path)


def entrypoint() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--request", required=True)
    p.add_argument("--result", required=True)
    p.add_argument("--input-path")
    p.add_argument("--libapp-path")
    args = p.parse_args()

    result_path = Path(args.result)
    try:
        request = _load_request(Path(args.request))
    except ValueError as exc:
        _write_result(
            result_path,
            "unsupported",
            error={"code": "unsupported_protocol", "message": str(exc)},
        )
        return 1

    requested = request["requested_backend"]
    output = request["output"]

    if requested == "auto":
        order = list(BACKEND_ORDER)
    elif requested in BUILDERS:
        order = [requested]
    else:
        _write_result(
            result_path,
            "unsupported",
            error={
                "code": "unsupported_protocol",
                "message": f"unknown requested backend {requested!r}",
            },
        )
        return 1

    fallback_reason: Optional[str] = None
    notes: List[dict] = []
    last_error: Optional[Tuple[str, str]] = None

    for name in order:
        try:
            model = _run_backend(name, request, args.input_path, args.libapp_path)
        except BackendUnavailable as exc:
            last_error = ("unsupported_snapshot", f"{name}: {exc}")
            if requested == "auto" and fallback_reason is None:
                fallback_reason = "backend_unavailable"
            notes.append(
                _diagnostic("domain_unsupported", f"{name} backend unavailable: {exc}",
                            subject=name, severity="info")
            )
            continue
        except (BackendFailed, FileNotFoundError, ValueError, OSError) as exc:
            last_error = ("parse_failed", f"{name}: {exc}")
            if requested == "auto" and fallback_reason is None:
                fallback_reason = "backend_failed"
            notes.append(
                _diagnostic("domain_not_recovered", f"{name} backend failed: {exc}",
                            subject=name, severity="warning")
            )
            continue

        try:
            Path(output).write_text(json.dumps(model), encoding="utf-8")
        except OSError as exc:
            _write_result(
                result_path,
                "failed",
                error={"code": "output_write_failed", "message": str(exc)},
            )
            return 1

        _write_result(
            result_path,
            "ok",
            model=output,
            resolved_backend=name,
            fallback_reason=fallback_reason,
            diagnostics=notes + model["diagnostics"],
        )
        return 0

    code, message = last_error or ("internal", "no backend ran")
    _write_result(result_path, "failed", error={"code": code, "message": message},
                  diagnostics=notes)
    return 1


if __name__ == "__main__":
    raise SystemExit(entrypoint())
