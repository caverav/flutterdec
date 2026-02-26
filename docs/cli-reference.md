# CLI Reference

## `flutterdec info`

Usage:

```bash
flutterdec info <INPUT> [--json]
```

Arguments:

- `<INPUT>`: APK or `libapp.so`
- `--json`: print JSON output

If adapter metadata is available, JSON output also includes app-package hints:

- `app_package_count_total`
- `app_package_counts_top`

## `flutterdec decompile`

Usage:

```bash
flutterdec decompile <INPUT> -o <OUT_DIR> [OPTIONS]
```

Required:

- `<INPUT>`: APK or `libapp.so`
- `-o, --out <OUT_DIR>`

General options:

- `--emit-asm`
- `--emit-asm-opcodes` (requires `--emit-asm`; prepends raw 32-bit opcode words in `asm/*.s`)
- `--emit-ir`
- `--focus <FOCUS>`
- `--max-functions <N>`
- `--function-scope <app-unknown|app|all>` (default `app-unknown`)
- `--app-package <NAME>` (repeatable; restricts to selected `package:<NAME>/...` libraries)
- `--adapter-backend <auto|internal|blutter>` (default `auto`)

Symbol ingestion:

- `--extra-symbol-elf <PATH>` (repeatable)
- `--extra-symbol-map-targets <PATH>` (repeatable)
- `--include-nearest-symbol-map`

Quality-gate options:

- `--max-placeholder-ifs <N>` (default `0`)
- `--max-unresolved-cf <N>` (default `0`)
- `--max-indirect-call-ratio <R>` (default `0.30`)
- `--min-disassembly-ratio <R>` (default `0.80`)

Analysis-engine profile:

- `--analysis-profile <light|balanced>` (default `balanced`)

Analysis-engine feature toggles:

- `--with-canonical-model-symbols`
- `--no-canonical-model-symbols`
- `--with-pool-value-hints`
- `--no-pool-value-hints`
- `--with-pool-semantic-hints`
- `--no-pool-semantic-hints`
- `--with-semantic-reporting`
- `--no-semantic-reporting`

Conflict rule:

- each `--with-*` conflicts with its matching `--no-*`

Adapter backend environment:

- `FLUTTERDEC_BLUTTER_CMD`: full command to execute Blutter bridge backend
- `FLUTTERDEC_BLUTTER_PY`: path to `blutter.py` (uses current Python interpreter)

## `flutterdec engine-fingerprint`

Usage:

```bash
flutterdec engine-fingerprint <INPUT> [--json] [-o <OUT_DIR>] [--max-markers <N>]
```

Arguments:

- `<INPUT>`: ELF file (usually `libflutter.so`)
- `-o, --out <OUT_DIR>`
- `--max-markers <N>` (default `24`)
- `--json`

## `flutterdec map-symbols`

Usage:

```bash
flutterdec map-symbols --stripped <PATH> --unstripped <PATH> -o <OUT_DIR> [OPTIONS]
```

Arguments:

- `--stripped <PATH>`
- `--unstripped <PATH>`
- `-o, --out <OUT_DIR>`
- `--include-branches`
- `--nearest-max-distance <N>` (default `8192`)
- `--require-exec-match`
- `--json`

## `flutterdec adapter`

Install:

```bash
flutterdec adapter install --dart-hash <HASH>
```

List:

```bash
flutterdec adapter list
```
