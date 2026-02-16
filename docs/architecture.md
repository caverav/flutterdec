# Architecture

## Pipeline

1. Loader: input APK/ELF normalization into `BinaryImage`
2. Snapshot locator: symbol-based lookup + stripped fallback scan
3. Dart VM adapter: version/hash detection and adapter JSON ingestion
4. Disassembly: ARM64 function decoding and ABI-aware annotation
5. IR: LLIR + CFG build
6. Decompiler: structuring + expression folding + pseudo-Dart emission
7. Naming: deterministic heuristic pass + optional user map override
8. Export: report JSON, IDA Python, Ghidra JSON

## Module boundaries

- `src/core/loader`: input parsing and memory region resolution
- `src/core/dartvm`: adapter lifecycle and normalized schema ingestion
- `src/core/model`: stable program/object model
- `src/core/disasm`: ARM64 decoding and Dart ABI semantics
- `src/core/ir`: LLIR and CFG representation/building
- `src/core/decompiler`: structured control flow + pseudo emitter
- `src/core/naming`: obfuscation-aware display name inference
- `src/core/export`: downstream artifacts and reverse-engineering exports
