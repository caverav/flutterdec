# Architecture

## Pipeline

1. Loader: normalize APK/ELF and locate snapshot blobs/symbol VAs
2. Adapter: run hash-specific Python adapter and load model JSON
3. Disassembly: ARM64 lightweight decode and Dart ABI annotations
4. IR/CFG: LLIR generation and basic-block graph recovery
5. Decompiler: structured pseudo-Dart emission
6. Quality/reporting: strict metrics and artifact generation

## Module boundaries

- `crates/flutterdec-loader`: file input and snapshot extraction
- `crates/flutterdec-adapter`: adapter install/run + model contract
- `crates/flutterdec-disasm-arm64`: function-level ARM64 decode
- `crates/flutterdec-ir`: LLIR + CFG build
- `crates/flutterdec-decompiler`: pseudocode and quality counters
- `crates/flutterdec-core`: orchestration and artifact emission
- `crates/flutterdec-cli`: command interface
