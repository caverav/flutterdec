# flutterdec

`flutterdec` is a standalone CLI and reusable C++20 library for decompiling Flutter Android ARM64 AOT (`libapp.so`) into Dart-like pseudocode.

## Status

v1 scaffold implemented with these pipeline stages:
- input loading (`apk` or `libapp.so`)
- snapshot location and version hashing
- adapter-based program model import
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

## Test

```bash
ctest --test-dir build --output-on-failure
```
