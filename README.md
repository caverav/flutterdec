# flutterdec

`flutterdec` is a Rust-first research CLI for static decompilation of Flutter AOT Android ARM64 binaries.

## Scope

- Android ARM64 only
- Static analysis only
- Adapter-backed snapshot model ingest
- Disassembly -> IR/CFG -> pseudo-Dart output

## Dev environment

```bash
nix develop
cargo test
cargo run -p flutterdec-cli -- info path/to/libapp.so --json
```

## CLI

```bash
flutterdec info <apk|so> [--json]
flutterdec decompile <apk|so> -o out/ [--emit-asm] [--emit-ir]
flutterdec adapter install --dart-hash <hash>
flutterdec adapter list
```

## Ethics

Use only on binaries you are legally allowed to analyze. This project is for security research and interoperability study.

## Context

Project research context and architecture notes are in `context.md`.
