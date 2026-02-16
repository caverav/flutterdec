# flutterdec

`flutterdec` is a standalone CLI and reusable C++20 library for decompiling Flutter Android ARM64 AOT (`libapp.so`) into Dart-like pseudocode.

## Status

v1 CLI implementation includes these pipeline stages:
- input loading (`apk` or `libapp.so`)
- snapshot location and version hashing
- adapter-based program model import (when available)
- heuristic model recovery fallback (when adapter is missing/fails)
- ARM64 disassembly + Dart ABI annotations
- LLIR/CFG construction
- pseudo-Dart emission
- naming heuristics
- IDA/Ghidra exports

## Build

```bash
cmake -S . -B build -G Ninja
cmake --build build
```

## CLI

```bash
./build/src/flutterdec info <libapp.so|apk>
./build/src/flutterdec decompile <libapp.so|apk> -o out/ --emit-asm --emit-ir
./build/src/flutterdec export ida <libapp.so|apk> -o ida.py
./build/src/flutterdec export ghidra <libapp.so|apk> -o ghidra.json
./build/src/flutterdec setup --dart-hash <hash>
```

`decompile` runs in strict mode by default and requires adapter-backed metadata for correctness.
To inspect unsupported hashes, use experimental fallback mode:

```bash
./build/src/flutterdec decompile <libapp.so|apk> -o out/ --experimental-heuristic
```

This emits `/out/quality.json` with quality-gate metrics.

## Test

```bash
ctest --test-dir build --output-on-failure
```
