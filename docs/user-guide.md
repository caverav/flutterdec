# User Guide

## Scope

Current supported target scope:

- Flutter AOT Android ARM64 binaries
- static analysis only

Inputs:

- APK
- `libapp.so`

## Install And Run

If you want the shortest path:

- run without installing: `nix run`
- install persistently with Nix: `nix profile install`
- install a release binary

### Run Without Installing

From GitHub:

```bash
nix run github:caverav/flutterdec -- --help
nix run github:caverav/flutterdec -- info ./sample.apk --json
nix run github:caverav/flutterdec -- decompile ./sample.apk -o ./out
```

From a local checkout:

```bash
nix run . -- --help
nix run . -- info ./sample.apk --json
nix run . -- decompile ./sample.apk -o ./out
```

### Install Persistently With Nix

Install from GitHub:

```bash
nix profile install github:caverav/flutterdec
flutterdec --help
```

Install from a local checkout:

```bash
nix profile install .
flutterdec --help
```

Update later:

```bash
nix profile upgrade flutterdec
```

### Install From A Release Binary

Current prerelease: [`v0.1.0-alpha.4`](https://github.com/caverav/flutterdec/releases/tag/v0.1.0-alpha.4)

Linux x64:

```bash
curl -fLO https://github.com/caverav/flutterdec/releases/download/v0.1.0-alpha.4/flutterdec-v0.1.0-alpha.4-Linux-X64.tar.gz
tar -xzf flutterdec-v0.1.0-alpha.4-Linux-X64.tar.gz
sudo install -m 0755 flutterdec /usr/local/bin/flutterdec
flutterdec --help
```

macOS arm64:

```bash
curl -fLO https://github.com/caverav/flutterdec/releases/download/v0.1.0-alpha.4/flutterdec-v0.1.0-alpha.4-macOS-ARM64.tar.gz
tar -xzf flutterdec-v0.1.0-alpha.4-macOS-ARM64.tar.gz
sudo install -m 0755 flutterdec /usr/local/bin/flutterdec
flutterdec --help
```

Other platforms and future tags:

[Releases page](https://github.com/caverav/flutterdec/releases)

### Other Ways To Run It

Install into the user Cargo bin:

```bash
nix develop -c cargo install --path crates/flutterdec-cli
~/.cargo/bin/flutterdec --help
```

Run from source without installing:

```bash
nix develop -c cargo run -p flutterdec-cli -- --help
nix develop -c cargo run -p flutterdec-cli -- info ./sample.apk --json
nix develop -c cargo run -p flutterdec-cli -- decompile ./sample.apk -o ./out
```

Build a local release binary:

```bash
nix develop -c cargo build -p flutterdec-cli --release
./target/release/flutterdec --help
```

### Where Adapters Live, And Why A Run Can Fail Outside A Checkout

`decompile` needs a Python adapter installed for the target's Dart snapshot hash. The adapter store is found
by **walking up from your current directory** for a folder containing *both* `Cargo.toml` and
`adapters/manifest.json`; if no such folder is found, the current directory is used as-is. So the binary's
own location is irrelevant - what matters is where you `cd`.

Two consequences that produce the same confusing error:

- **Running from outside a checkout finds no store.** `nix run`, a `nix profile` install, or a release binary
  invoked from, say, `~/work` will report:

  ```
  Error: adapter not installed for hash 80a49c7111088100a233b2ae788e1f48.
  run: flutterdec adapter install --dart-hash 80a49c7111088100a233b2ae788e1f48
  ```

  The message is correct and the fix is to run that command - but run it from **inside** the checkout, or the
  adapter lands somewhere the next invocation will not look.
- **A fresh clone or `git worktree` has the manifest but no adapters.** `adapters/installed/` is gitignored,
  so a second working tree of the same repository starts with zero installed adapters even though your main
  checkout has them. This has already caused a real misdiagnosis in this project's own research: a run in a
  fresh worktree hit the error above and it was recorded as "the quality gate is unrunnable" before the true
  cause was found. If a command works in one tree and not another, check this first.

Install once per snapshot hash, from within the checkout:

```bash
flutterdec info ./sample.apk --json      # read snapshot_hash
flutterdec adapter install --dart-hash <HASH>
flutterdec adapter list
```

Two things to know about that install step. `adapter install` writes the built adapter into
`adapters/installed/`, which is gitignored, **but it also registers the hash in `adapters/manifest.json`,
which is tracked** - so installing an adapter leaves your working tree dirty with a one-entry diff. That is
expected, not a mistake, and the entry is worth keeping if you intend to share support for that snapshot
hash. Second, the manifest and the installed adapters can disagree: a fresh clone has a manifest listing
adapters whose files are absent, and you get the same "adapter not installed" message as if the manifest were
empty. `flutterdec adapter list` shows both sides, which is the quickest way to tell the two apart.

## First Use

If this is your first run, this is the shortest useful path.

1. Inspect the target:

```bash
flutterdec info ./sample.apk --json
```

For APK inputs, `info` also reports Android startup summary fields:

- `android_startup_present`
- `android_startup_confidence`
- `android_startup_entrypoint_count`
- `android_startup_flutter_activity_count`

If adapter metadata is available, `info` also reports:

- `app_package_counts_top`
- `adapter_kind`
- `manifest_entry_present`
- `adapter_snapshot_hash_match`
- `compatibility_warnings`

2. Install the adapter for the detected Dart hash:

```bash
flutterdec adapter install --dart-hash <HASH>
flutterdec adapter list
```

3. Decompile:

```bash
flutterdec decompile ./sample.apk -o ./out
```

4. Start with:

- `out/pseudocode/*.dartpseudo`
- `out/report.json`
- `out/quality.json`

## Basic Commands

Inspect target metadata:

```bash
flutterdec info ./sample.apk --json
```

Decompile with the default app-focused scope:

```bash
flutterdec decompile ./sample.apk -o ./out
```

Decompile with extra artifacts:

```bash
flutterdec decompile ./sample.apk -o ./out --emit-asm --emit-ir
```

Compare two builds at recovered-function level:

```bash
flutterdec diff --old ./old.apk --new ./new.apk -o ./out-diff --json
```

Generate a Ghidra import script with recovered symbols:

```bash
flutterdec decompile ./sample.apk -o ./out --emit-ghidra-script
```

Generate an IDA import script with recovered symbols:

```bash
flutterdec decompile ./sample.apk -o ./out --emit-ida-script
```

## Function Scope

Default scope is app-focused:

- `app-unknown` (default): app (`package:*`) plus unknown ownership functions
- `app`: only app (`package:*`) functions
- `all`: include Flutter, Dart runtime, and framework internals too

Include everything:

```bash
flutterdec decompile ./sample.apk -o ./out --function-scope all
```

Limit output to selected app packages:

```bash
flutterdec decompile ./sample.apk -o ./out \
  --function-scope app-unknown \
  --app-package my_app
```

Tip: if you are not sure about package names, check `report.json` under `function_scope.app_package_counts_top`.

## Single-Function Targeting

Decompile and disassemble one specific function:

```bash
flutterdec decompile ./sample.apk -o ./out \
  --target id:42 \
  --emit-asm
```

Target by entry address:

```bash
flutterdec decompile ./sample.apk -o ./out \
  --target va:0x613468 \
  --emit-asm
```

`--target` accepts `id:<N>`, `va:0x<ADDR>`, `0x<ADDR>`, or `<N>`.
When `<N>` matches multiple functions, the command fails and asks for explicit `id:` or `va:`.
Target mode emits only the matched function artifacts and reports selection details in `report.json.target_selection`.
If the match is outside current scope filters, target mode can override scope automatically for that function.

## Analysis Engine Profiles

`decompile` profile controls analysis depth vs throughput.

Profile options:

- `balanced` (default): best readability and semantic recovery
- `light`: reduced analysis for faster large-scale runs

Examples:

```bash
flutterdec decompile ./sample.apk -o ./out --analysis-profile balanced
flutterdec decompile ./sample.apk -o ./out --analysis-profile light
```

Adapter backend options:

- `--adapter-backend auto` (default): try r2flutter, then the Blutter bridge, then the internal adapter
- `--adapter-backend internal`: force internal adapter only
- `--adapter-backend blutter`: require Blutter bridge backend with no fallback
- `--adapter-backend r2-flutter`: require the r2flutter backend with no fallback
- `--require-snapshot-hash-match`: fail if adapter snapshot hash does not match loader snapshot hash

Backend choice decides how much is actually recovered. The internal adapter carves
strings and scans prologues: every function comes out as `sub_<addr>` and its
`object_pool` is a list of carved strings, not real pool slots. `r2flutter` and
`blutter` parse the snapshot, so they return exact Dart names and a real `ObjectPool`.

Only a backend that reports `pool_geometry` lets `flutterdec` turn a `pool[N]`
reference in the disassembly into a value. Without it, pool references are left
unresolved on purpose, and `report.json.pool_metadata.hints_suppressed_reason`
explains why. Check `pool_metadata.index_space_authoritative` if pseudocode has fewer
string literals than you expected.

r2flutter backend environment variables:

- `FLUTTERDEC_R2FLUTTER_BIN`: path to the `r2flutter` binary
- `FLUTTERDEC_R2FLUTTER_CMD`: full command to launch it, when a wrapper is needed
- `FLUTTERDEC_R2FLUTTER_TIMEOUT`: per-invocation timeout in seconds (default 900)
- otherwise `r2flutter` is resolved from `PATH`

Blutter bridge environment variables:

- `FLUTTERDEC_BLUTTER_CMD`: full command used to launch Blutter, for example `python3 /opt/blutter/blutter.py`
- `FLUTTERDEC_BLUTTER_PY`: direct path to `blutter.py`

Nix setup note:

- in `nix develop`, `FLUTTERDEC_BLUTTER_CMD` is exported automatically to a Nix-managed `flutterdec-blutter` wrapper
- direct wrapper invocation is available with `nix run .#blutter-bridge -- --help`

Per-feature toggles:

- `--with-canonical-model-symbols` / `--no-canonical-model-symbols`
- `--with-pool-value-hints` / `--no-pool-value-hints`
- `--with-pool-semantic-hints` / `--no-pool-semantic-hints`
- `--with-semantic-reporting` / `--no-semantic-reporting`
- `--with-bootflow-category-seeds` / `--no-bootflow-category-seeds`
- `--with-apk-startup-analysis` / `--no-apk-startup-analysis`

## Quality Gates

Decompile quality checks are controlled by:

- `--max-placeholder-ifs`
- `--max-unresolved-cf`
- `--max-indirect-call-ratio`
- `--min-disassembly-ratio`

Example for exploratory runs:

```bash
flutterdec decompile ./sample.apk -o ./out \
  --max-functions 250 \
  --max-placeholder-ifs 999999 \
  --max-unresolved-cf 999999 \
  --max-indirect-call-ratio 1.0 \
  --min-disassembly-ratio 0.0
```

## Name Recovery With Engine Symbols

Generate direct-call target mapping from a stripped/unstripped engine pair:

```bash
flutterdec map-symbols \
  --stripped ./libflutter.stripped.so \
  --unstripped ./libflutter.unstripped.so \
  -o ./out/symbol-map \
  --register-local-cache
```

Use the cached mapping in later APK decompile runs:

```bash
flutterdec decompile ./sample.apk -o ./out \
  --extra-symbol-elf ./libflutter.unstripped.so
```

When the cached build id matches the APK's embedded `libflutter.so`, `decompile` auto-loads the registered target summary and reports the match under `report.json.engine_symbol_ingestion`.

## Outputs

Main output artifacts:

- `pseudocode/*.dartpseudo`
- `report.json`
- `quality.json`
- `asm/*.s` if `--emit-asm`
- `ir/*.json` if `--emit-ir`
- `ghidra_apply_symbols.py` if `--emit-ghidra-script`
- `ida_apply_symbols.py` if `--emit-ida-script`
- `diff_report.json` if you run `flutterdec diff`

Most users should start by reading:

- `pseudocode/*.dartpseudo`
- `report.json.android_startup`
- `quality.json`

The [CLI reference](cli-reference.md#emission-diagnostics-in-qualityjson)
defines every `quality.json.emission` counter and unit, explains the block
ledger and omitted-path marker scopes, and documents
`report.json.record_split.rejected_invalid_ir`.


## Reading Value Annotations In The Pseudocode

A register whose binding the decompiler could not carry through a control-flow merge renders as a bare
`regN`. Where the value it *held* is known, `flutterdec` appends it as a comment rather than substituting it,
so the comment never changes what the code says. Four forms appear, and the distinction between the first two
is the one worth learning:

| form | meaning |
|---|---|
| `regN /* = a \| b */` | **exhaustive.** Every path reaching this merge contributed a value, and they are all listed. `regN` is one of these. |
| `regN /* possible (non-exhaustive): a \| b */` | **incomplete.** At least one incoming path contributed no usable value, so the real value may be something not listed. |
| `regN /* loop-entry value: a \| b */` | the value held on **entry** to the loop, one per entry arm - several when the loop has more than one entry arm and they disagree. The value on the back edge is never shown, and is usually different. |
| `regN /* value before this call: a */` | the value held **immediately before** the adjacent call, which clobbered the register. Not the value after it. Always a single value. |

Expect the incomplete form to dominate, and expect the call form to be rare. Measured on LocalSend 1.17 at
default scope: 917 non-exhaustive, 246 exhaustive, 148 loop-entry, **3** pre-call. Values are listed in
ascending predecessor order, so the order is stable across runs.

Two properties are worth relying on:

- **The annotations are comments and nothing else.** Strip every one and you get byte-identical output to a
  build without them. They do not participate in naming, aliasing or type inference, so they cannot make the
  surrounding code wrong.
- **A listed value was actually observed on the path it is attributed to**, not guessed from context. Values
  the analysis could not attribute are omitted rather than approximated, which is why the non-exhaustive form
  exists and is the more common of the two.

Absence of an annotation means the value was not recovered, not that the register is unimportant. Most bare
`regN` occurrences carry no annotation: see `docs/research-pseudocode-quality.md` R29 for the measured
coverage per site and its limits.
