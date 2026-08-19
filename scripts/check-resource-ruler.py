#!/usr/bin/env python3
"""Verify the auxiliary resource ruler digest inventory and loader chain."""

from __future__ import annotations

import argparse
import hashlib
import re
import sys
import tempfile
from pathlib import Path

PROTOCOL = "docs/resource-ruler-protocol.md"
HEADING = "## Protected digest inventory"
PROTECTED_PATHS = (
    "crates/flutterdec-bench/Cargo.toml",
    "crates/flutterdec-bench/src/main.rs",
    "crates/flutterdec-bench/src/measure.rs",
    "crates/flutterdec-decompiler/src/lib.rs",
    "crates/flutterdec-decompiler/src/control_flow/structured.rs",
    "scripts/bench-resource.sh",
    "scripts/audit-resource-evidence.py",
    "scripts/check-resource-ruler.py",
    "scripts/ci-check.sh",
)
ROW = re.compile(r"^\| `([^`]+)` \| `([0-9a-f]{64})` \|$")
CI_LOADER = "nix develop -c python3 scripts/check-resource-ruler.py"


def workspace_root() -> Path:
    return Path(__file__).resolve().parent.parent


def parse_rows(text: str):
    try:
        section = text.split(HEADING, 1)[1].split("\n## ", 1)[0]
    except IndexError as error:
        raise ValueError("protected digest inventory section is missing") from error
    rows = []
    for line in section.splitlines():
        match = ROW.match(line)
        if match:
            rows.append(match.groups())
    if not rows:
        raise ValueError("protected digest inventory has no rows")
    return rows


def digest_failures(root: Path, rows):
    failures = []
    paths = [path for path, _ in rows]
    if tuple(paths) != PROTECTED_PATHS:
        failures.append("digest paths differ from the hardcoded inventory or order")
    if len(paths) != len(set(paths)):
        failures.append("digest inventory contains duplicate paths")
    for path, expected in rows:
        target = root / path
        if not target.is_file():
            failures.append(f"{path}: protected file deleted or not regular")
            continue
        actual = hashlib.sha256(target.read_bytes()).hexdigest()
        if actual != expected:
            failures.append(f"{path}: stale digest {expected}, actual {actual}")
    return failures


def loader_failures(root: Path):
    failures = []
    ci = (root / "scripts/ci-check.sh").read_text(encoding="utf-8")
    if ci.count(CI_LOADER) != 1:
        failures.append("resource checker CI loader is absent, duplicated, or bypassed")
    main = (root / "crates/flutterdec-bench/src/main.rs").read_text(encoding="utf-8")
    measure = (root / "crates/flutterdec-bench/src/measure.rs").read_text(encoding="utf-8")
    lib = (root / "crates/flutterdec-decompiler/src/lib.rs").read_text(encoding="utf-8")
    structured = (root / "crates/flutterdec-decompiler/src/control_flow/structured.rs").read_text(encoding="utf-8")
    needles = {
        "resource subcommand": 'Some("resource") => resource(&args[1..])',
        "allocator lifecycle test": "resource_allocator_covers_full_lifecycle_without_misattribution",
        "allocator recursion gate": "instrumentation_recursions",
        "phase panic cleanup test": "nested_phase_exit_and_panic_cleanup_restore_the_parent",
        "fixed phase stack": "static RESOURCE_PHASES: Cell<PhaseStack>",
        "CFG phase entry": "ResourcePhase::Cfg",
        "CFG clone plant": "ResourcePlant::CfgGraphClone",
        "emitter clone plant": "ResourcePlant::EmitterBlockClone",
    }
    carriers = {
        "resource subcommand": main,
        "allocator lifecycle test": measure,
        "allocator recursion gate": measure,
        "phase panic cleanup test": measure,
        "fixed phase stack": lib,
        "CFG phase entry": structured,
        "CFG clone plant": structured,
        "emitter clone plant": structured,
    }
    for name, needle in needles.items():
        if needle not in carriers[name]:
            failures.append(f"{name} loader is absent or bypassed")
    return failures


def self_test():
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        rows = []
        for path in PROTECTED_PATHS:
            target = root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(path, encoding="ascii")
            rows.append((path, hashlib.sha256(path.encode("ascii")).hexdigest()))
        assert not digest_failures(root, rows)
        (root / PROTECTED_PATHS[0]).unlink()
        assert "deleted" in digest_failures(root, rows)[0]
        (root / PROTECTED_PATHS[0]).write_text("changed", encoding="ascii")
        assert "stale digest" in digest_failures(root, rows)[0]
    sample = f"{HEADING}\n\n| Path | sha256 |\n| --- | --- |\n"
    sample += "\n".join(f"| `{path}` | `{'0' * 64}` |" for path in PROTECTED_PATHS)
    sample += "\n\n## Next\n"
    assert tuple(path for path, _ in parse_rows(sample)) == PROTECTED_PATHS
    print("[resource-ruler] self-test ok: deletion, stale digest, and section bounds")


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    self_test()
    if args.self_test:
        return 0
    root = workspace_root()
    rows = parse_rows((root / PROTOCOL).read_text(encoding="utf-8"))
    failures = digest_failures(root, rows)
    if not failures:
        failures = loader_failures(root)
    if failures:
        print(f"[resource-ruler] FAILED, {len(failures)} problem(s)")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print(f"[resource-ruler] ok, {len(rows)} digests and all loaders match")
    return 0


if __name__ == "__main__":
    sys.exit(main())
