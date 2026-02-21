# Architecture

## Pipeline

1. Loader: normalize APK/ELF and locate snapshot blobs/symbol VAs
2. Adapter: run hash-specific Python adapter and load model JSON
3. Disassembly: ARM64 lightweight decode and Dart ABI annotations
4. IR/CFG: LLIR generation and basic-block graph recovery
5. Decompiler: structured pseudo-Dart emission
6. Quality/reporting: strict metrics and artifact generation

Auxiliary path:

7. Symbol mapping: stripped vs unstripped ARM64 ELF direct-call target mapping
8. Engine fingerprinting: ELF metadata and marker extraction for build/version hints
9. Symbol ingestion: feed `map-symbols` target summaries into `decompile` for pseudocode call naming
10. Call intent tagging: emit stdlib/runtime/native intent comments for recognized call targets

## Module boundaries

- `crates/flutterdec-loader`: file input and snapshot extraction
- `crates/flutterdec-adapter`: adapter install/run + model contract
- `crates/flutterdec-disasm-arm64`: function-level ARM64 decode
- `crates/flutterdec-ir`: LLIR + CFG build
- `crates/flutterdec-decompiler`: pseudocode and quality counters
- `crates/flutterdec-core`: orchestration and artifact emission
- `crates/flutterdec-cli`: command interface
