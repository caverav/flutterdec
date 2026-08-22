# Public real-binary compatibility baseline: LocalSend 1.17.0 (arm64)

One whole-APK decompile of a pinned, publicly downloadable Flutter release,
run on the same bytes with the same command at the fixed reference `1371e42`
and at the branch head, with every output difference classified and
adjudicated. The APK is **not** vendored: section 2 is a fetch-and-verify
recipe.

Everything below is reproducible from `docs/compat-evidence/` and
`scripts/check-compat-baseline.py`. The machine-readable form of section 1 is
[compat-evidence/input-recipe.json](compat-evidence/input-recipe.json); the
prose here exists to adjudicate the differences, which a JSON file cannot do.

## 1. Bindings

| Field | Value |
| --- | --- |
| Input project | LocalSend, release `v1.17.0` (2025-02-20) |
| Input asset | `LocalSend-1.17.0-android-arm64v8.apk` |
| Input URL | `https://github.com/localsend/localsend/releases/download/v1.17.0/LocalSend-1.17.0-android-arm64v8.apk` |
| Release page | `https://github.com/localsend/localsend/releases/tag/v1.17.0` |
| Input bytes | 17491108 |
| Input sha256 | `2c7f5fd4872da25115bb8e5e62f92de94dda47b0f249ff387ac667b13871dc3e` |
| Input license | Apache-2.0, `https://github.com/localsend/localsend/blob/main/LICENSE` |
| Redistribution | none; fetched and digest-verified, never committed |
| Snapshot hash | `80a49c7111088100a233b2ae788e1f48` (identical on both sides) |
| Adapter | `--adapter-backend internal`, kind `dynamic_snapshot_string_model_v1` |
| Reference revision | `1371e42549472ec388f58bc1fd5dbdf96e8dcdd1` |
| Candidate revision | `361c922ad418da67277aea911050fdf70fe10234` |
| Reference binary sha256 | `54981d32172f8ea9cc03331eb16d8e3cfbc8f300d7ebf17bbc0b25dece8c272b` |
| Candidate binary sha256 | `08110f952bc2c0a831aa9bb979b4f745b9607595cef6f47c1bf5bc36a4f67c31` |
| CLI version | `flutterdec 0.1.0-alpha.4` on both sides |
| Toolchain | `nix develop`, rustc 1.92.0, cargo 1.92.0, profile `release` |
| Environment | `LC_ALL=C`, `TZ=UTC`, identical absolute input path on every run |

Both sides were built clean, in separate worktrees, into separate
workspace-backed target directories, from `rust-toolchain.toml` and
`flake.lock` that are byte-identical at the two revisions.

## 2. Fetch the input

```bash
python3 scripts/check-compat-baseline.py fetch --dest /tmp/localsend-1.17.0-arm64v8.apk
```

The fetch fails unless the downloaded asset matches both the recorded byte
size and the recorded SHA-256, so a moved or re-cut release cannot silently
become the baseline input. Manual equivalent:

```bash
curl -fsSL -o /tmp/localsend-1.17.0-arm64v8.apk \
  https://github.com/localsend/localsend/releases/download/v1.17.0/LocalSend-1.17.0-android-arm64v8.apk
sha256sum /tmp/localsend-1.17.0-arm64v8.apk
```

## 3. Command

Run once per side, from a clean release build, into its own output directory:

```bash
LC_ALL=C TZ=UTC target/release/flutterdec decompile <input> \
  --out <out> \
  --adapter-backend internal \
  --function-scope all \
  --emit-ir \
  --emit-asm
```

Quality gates are the defaults (`--max-placeholder-ifs 0`,
`--max-unresolved-cf 0`, `--max-indirect-call-ratio 0.30`,
`--min-disassembly-ratio 0.80`). Both sides therefore exit 1 **after** writing
a complete artifact set, which is a truthful gate failure and not a run
failure:

| Run | Exit | Gate reasons | Wall clock |
| --- | --- | --- | --- |
| reference `1371e42` | 1 | placeholder if-count exceeded | 53 s |
| candidate `361c922` | 1 | placeholder if-count exceeded; unresolved control-flow count exceeded | 133 s |
| candidate, second process | 1 | same two reasons | 133 s |
| candidate, third process | 1 | same two reasons | 133 s |
| candidate, fourth process, input copied to a different absolute path | 1 | same two reasons | 133 s |

`placeholder_ifs=501 unresolved_cf=0 indirect_call_ratio=0.156
disassembly_ratio=1.000` on the reference; `placeholder_ifs=530
unresolved_cf=517 indirect_call_ratio=0.158 disassembly_ratio=1.000` on the
candidate.

## 4. Artifact sets

Each run wrote 17402 files: 5800 `pseudocode/*.dartpseudo`, 5800 `ir/*.json`,
5800 `asm/*.s`, plus `quality.json` and `report.json`. All five runs have the
**same 17402 relative paths**; no file was added, renamed, or lost, and
`quality.json.function_count` is 5800 on both sides.

| Manifest | sha256 of the per-artifact manifest |
| --- | --- |
| reference | `bc70190801414e18c6c68cf4a20f3037da1b086e47484880bc47fb85943ff39a` |
| candidate | `aa587824359452cd5af2e51b1ca18070b51ae297faf424199a1e0cc6d5b9f48b` |
| candidate, second process | `aa587824359452cd5af2e51b1ca18070b51ae297faf424199a1e0cc6d5b9f48b` |
| candidate, third process | `aa587824359452cd5af2e51b1ca18070b51ae297faf424199a1e0cc6d5b9f48b` |

The three candidate manifests are equal, so all 17402 artifacts are byte-stable
across three separate processes. Nothing in the emitted set carries a
timestamp, a duration, or a random value. Three strings in `report.json` follow
the workspace rather than the input bytes, and they are the whole volatile-field
allowlist for this baseline:

| Field | Committed as |
| --- | --- |
| `input` | `"<input>"` |
| `adapter_selection.adapter_exec_path` | `"<repo>/adapters/installed/dart_adapter_<snapshot hash>"` |
| `engine_symbol_ingestion.manifest_path` | `"<repo>/symbols/manifest.json"` |

The allowlist was measured, not assumed: a fourth process run from the same
bytes copied to a different absolute path reproduced 17401 of the 17402
artifacts byte-for-byte and differed only in `report.json.input`. The other two
fields are constant for any run inside one checkout, which is why
`check-compat-baseline.py replay` normalizes all three before comparing
`report.json` and compares every other artifact by digest.

Per-artifact digests for both sides are in
[compat-evidence/artifact-manifest.tsv](compat-evidence/artifact-manifest.tsv)
(`=` in the candidate columns means "identical to the reference"). The
function inventory is
[compat-evidence/function-inventory.tsv](compat-evidence/function-inventory.tsv);
the top-level function name of every one of the 5800 pseudocode files is the
same on both sides.

Reference against candidate: **7928 artifacts identical, 9474 differing**.

| Kind | Identical | Differing |
| --- | --- | --- |
| `asm/*.s` | 5800 | 0 |
| `pseudocode/*.dartpseudo` | 2128 | 3672 |
| `ir/*.json` | 0 | 5800 |
| `quality.json`, `report.json` | 0 | 2 |

Every assembly file is byte-identical, so instruction decoding did not move.
Every difference below is an IR, CFG, or emitter difference on top of the same
decode.

## 5. Public schemas and filenames

No public key was dropped anywhere
([compat-evidence/schema-comparison.json](compat-evidence/schema-comparison.json)):

- `ir/*.json`, all 5800 files compared: 13 reference key paths preserved,
  0 removed, and two additive top-level objects - `emission` (declines,
  traversal events) and `block_ledger` (per-block dispositions, stage
  identities, remaps, valid edges).
- `report.json`: 0 keys removed, 20 added, all under
  `quality.emission.*`, `record_split.rejected_invalid_ir`, and
  `shared_stub_naming.noreturn_skipped_invalid_ir`.
- `quality.json`: 0 keys removed, the additive `emission` object added.

Filenames, directory layout, and the `NNNNN_<name>` numbering are unchanged.

## 6. Difference adjudication

Counts are in
[compat-evidence/difference-classes.json](compat-evidence/difference-classes.json),
whose `definitions` object states how each one is measured - instruction
alignment, exclusive-instruction reasons, target shapes, operand pairing, field
paths, line tags - so every number below is recomputable from the two output
trees. Worked examples are in
[compat-evidence/representative-differences.diff](compat-evidence/representative-differences.diff).

### 6.1 IR, after removing the two added top-level objects

4107 of 5800 IR documents are then byte-identical; 1693 still differ. Those
1693 decompose into exactly three classes, with no others observed:

| Class | Count | Adjudication |
| --- | --- | --- |
| `Other->Trap` | 2456 instructions | `brk`/`hlt`/`udf` now carry the trap control effect instead of being unclassified. Accepted: R1 in `research-ir-cfg-emitter.md`, ARM64 control-effect table in `oracle-protocol-ir-cfg-emitter.md` section 3, ruler `crates/flutterdec-decompiler/tests/arm64_control_effects.rs`. |
| `Other->IndirectBranch` | 1304 instructions | `br Xn` is now an indirect branch with its register recorded in `target`. Same accepted class. |
| `empty->register` branch targets | 1304 | The only target-shape transition observed: an indirect branch that previously had an empty `target` now names the register it branches through - `x16` in 1284 of the 1304, then `x4` (11), `x2` (4), `x3` (3), `x1` (1), `x5` (1). No numeric target changed anywhere, so nothing in this baseline exercises the branch-target radix work (R8), and no target was removed. |

No reverse transition occurs: nothing that the reference classified became
`Other` in the candidate.

Block counts move in one direction only. The per-function delta histogram runs
from 0 to +55 with **no negative entry**: 4719 functions keep the same block
count and 1081 gain blocks. No function lost a block.

Instruction membership changes are fully accounted for:

- **934 instructions present only in the reference**, all in one class,
  `after_trap:brk`: walking back through the reference-only instructions from
  any of them lands on a `brk`, so every one is reached only through a trap.
  The reference gave `brk` an invented fallthrough edge,
  so the bytes after a trap were pulled into the CFG; the candidate stops
  there. Example, `ir/00008_sub_6071fc.json`: `bl #0xd51c4c` and
  `b #0x607214` at `0x607254`/`0x607258` sit directly behind `brk #0`.
  This is the removal of invented control flow, not a loss of recovered code:
  the same bytes are still in the byte-identical `asm/00008_sub_6071fc.s`.
  The reader-facing consequence is in the committed diff for
  `pseudocode/00008_sub_6071fc.dartpseudo`: the invented fallthrough closed a
  back edge, so the reference rendered a `while (true) { ... continue; }` loop
  that the program does not contain. The candidate renders the same code
  straight-line and ends it at `// trap: control does not continue`.
- **399 instructions present only in the candidate.** All 399 sit in blocks
  with an empty predecessor set - trailing unreachable stubs (typically the
  Dart stack-overflow slow path after a `ret` or a trap) that the candidate
  retains and accounts for as `RetainedUnreachable` instead of dropping
  silently.

### 6.2 Pseudocode

3672 files differ: 103986 removed lines against 1930418 added lines. The
candidate emits about 7.7x more pseudocode text for the same 5800 functions,
and the added text is paths the reference declined to emit rather than new
claims about the program:

| Marker | Reference | Candidate | Adjudication |
| --- | --- | --- | --- |
| `_block_N` helper occurrences | 0 | 27097 | Blocks the walk cannot inline are emitted as helpers instead of being dropped; 4781 helper definitions across 244 files. |
| `// control rejoins block N` | 0 | 202669 | An edge to an already-emitted block is now stated instead of falling through silently. |
| `// omitted complex paths:` | 355 | 50 | Fewer omissions, because the helper and rejoin renderings replace them. |
| `// loop back-edges:` | 130 | 77 | Same cause. |
| `// trap: ...` | 0 | 7262 | The trap control effect from 6.1, rendered. |
| `// indirect branch through regN: target not recovered` | 0 | 520 | Unrecovered indirect branches are now stated; the reference emitted nothing and counted `unresolved_cf = 0`. |
| `/* cond */` placeholder conditions | 215 | 2617 | Grows with the emitted volume; still declared as a placeholder rather than invented. |

Every row except the last is a text census over the emitted pseudocode, in
[compat-evidence/structural-census-reference.json](compat-evidence/structural-census-reference.json)
and
[compat-evidence/structural-census-candidate.json](compat-evidence/structural-census-candidate.json).
The last row is `quality.json.placeholder_cond_markers`, and the emitted text
carries exactly that many `/* cond */` occurrences on each side.

**Operand naming.** The second large class is a value the candidate refuses to
restate. `quality.json.raw_register_name_refs` rises from 54062 to 690963,
which is 0.198 per emitted line on the reference against 0.329 on the
candidate, so it is a real shift and not only the larger text. The committed diff for
`pseudocode/00001_sub_606bc0.dartpseudo` is the shape: across an intervening
call the reference kept rendering `if (tmp3.f8 != tmp2.f8)`, while the
candidate renders `if (reg0 != reg3)` and moves the `/* = -1 | 1 */` value
annotation onto the use that the annotation actually describes. The direction
is conservative - a register whose provenance is not proven past a
call-clobber boundary is now printed as a register instead of as a field
expression the emitter cannot stand behind. Accepted under the call-clobber
and annotation-anchor rules enforced by
`crates/flutterdec-decompiler/tests/register_width_provenance.rs`,
`annotation_anchor_identity.rs`, and `provenance_audit.rs`.

Counting the one-line-for-one-line replacements across all 3672 differing files
(zero-context diff hunks that replace exactly one line with exactly one line):
9060 gain `regN` tokens, 1984 keep the same number, and 78 lose them. All 78 are
enumerated in
[compat-evidence/operand-direction-losses.tsv](compat-evidence/operand-direction-losses.tsv)
and split four ways:

| Class | Count | Example |
| --- | --- | --- |
| an expression is replaced by the register that holds it | 63 | `if (((reg4 + (reg8 << 2))).f16 == reg4)` becomes `if (reg9 == reg4)` |
| a register is named as the parameter slot it held | 11 | `smiUntag(reg5.f12)` becomes `smiUntag(slot3.f12)` |
| the line becomes a structural line | 1 | `if (!(reg0 != 6)) {` becomes `else {` |
| other | 3 | `(reg1 - 1)` becomes the named temporary `reg1Minus1` |

The first class is the same conservative direction as the rest of the section,
the second is a recovery, and the third is restructuring. The `other` class
holds the one case where the candidate prints strictly less at a call site:
in `pseudocode/05149_sub_bdd098.dartpseudo` the reference emits
`sub_90b144(reg0)` and the candidate emits `sub_90b144()`, declining the
register-argument claim. It is listed in section 9 as an open item rather than
adjudicated as a correction.

**Helper resolution.** All 27097 `_block_N` occurrences resolve: every
referenced helper has a definition in the same file, in all 5800 candidate
files, with zero dangling references. The reference has no helpers at all.

**Unknown semantics.** Unknown values keep the same vocabulary on both sides
(`receiver: unrecovered`, `reg<N>`, `poolOff[...]`, `/* cond */`); the
candidate adds the two unknown *control* effects - trap and unrecovered
indirect branch - that the reference left unstated. Nothing that the reference
resolved became unknown.

### 6.3 Quality and report values

Nineteen `report.json` values change, fifteen of them inside the `quality`
block it embeds and four outside it (`call_fallback.dispatch_target_invoke`,
`call_fallback.generic_invoke`, and the two `shared_stub_naming` noreturn
counters). All of them follow from 6.1 and 6.2. The ones worth
naming: `unresolved_cf` 0 -> 517 (previously unstated indirect branches),
`block_helper_refs` 0 -> 27097, `total_calls` 45955 -> 41883 and
`indirect_calls` 7149 -> 6627 (calls behind an invented post-trap fallthrough
are no longer counted), `unlifted_instructions` 689 -> 481,
`omitted_path_markers` 355 -> 50, and the new `emission` object.

## 7. Accounting reconciliation

From [compat-evidence/accounting-reconciliation.json](compat-evidence/accounting-reconciliation.json):

- Summing the per-function `block_ledger` dispositions across all 5800
  candidate IR files reproduces `quality.json.emission` exactly:
  `StructuredEmitted` 55019, `DfsEmitted` 22277, `GuardPruned` 5302,
  `NoreturnPruned` 13016, `RetainedUnreachable` 201006,
  `ReachableUnemitted` 1499, `invalid_cfg_rejected` 0.
- Traversal events reconcile the same way: `DfsDepthOmission` 199441,
  `DfsVisitOmission` 25252, `HelperCapOmission` 3847. `structured_declines`
  303 = `repeat_budget` 301 + `structured_depth_budget` 2, and equals
  `structured_rollbacks`.
- `report.json.shared_stub_naming.noreturn_pruned_blocks` (13016) equals
  `quality.json.emission.noreturn_pruned_blocks` on the candidate; the
  reference reported 13696 with no ledger to check it against.
- The four text-derived quality counters recount exactly from the emitted
  files on **both** sides: `block_helper_refs` (0 / 27097),
  `placeholder_cond_markers` (215 / 2617), `omitted_path_markers` (355 / 50),
  `loop_backedge_markers` (130 / 77).

One residue, recorded rather than papered over: the candidate emits 520
unrecovered-indirect-branch markers while `quality.json.unresolved_cf` is 517.
The markers sit in 137 functions. Function-level replay with `--target id:<N>`
over all 137, in
[compat-evidence/marker-replay.tsv](compat-evidence/marker-replay.tsv),
reproduces 136 of them byte-for-byte, and in every one of those 136 the
replayed marker count equals the replayed counter: 516 and 516. The residue is
therefore exactly the one function whose targeted render differs,
`00860_sub_696734`, which carries 4 markers in the whole-program run against
the 1 remaining counter (517 - 516). Marker text and `unresolved_cf` are not a
product invariant - the counter is incremented on the counted emission path
while text can be re-rendered - so this is an emitter accounting question for
the emitter contracts, not a reference-to-candidate compatibility difference:
the reference has 0 of both.

## 8. Re-running this baseline

```bash
# offline, no input needed: the committed baseline is internally consistent,
# drops no public schema key, and adjudicates every difference class it records
python3 scripts/check-compat-baseline.py verify
python3 scripts/check-compat-baseline.py --self-test   # the parser's own check

# with the real input: replay the candidate side and compare every artifact
python3 scripts/check-compat-baseline.py fetch --dest /tmp/localsend.apk
nix develop -c cargo build -p flutterdec-cli --release
LC_ALL=C TZ=UTC target/release/flutterdec decompile /tmp/localsend.apk \
  --out /tmp/compat-out --adapter-backend internal --function-scope all \
  --emit-ir --emit-asm   # exits 1 on the quality gate, after writing artifacts
python3 scripts/check-compat-baseline.py replay --out /tmp/compat-out
```

`replay` compares 17401 artifacts against the committed candidate digests and
`report.json` against the committed snapshot with the three volatile fields
normalized, so it fails on any lost file, any added file, and any changed byte
outside those fields. Both modes were plant-tested, each exiting 1: deleting two
artifacts and changing `counts.functions` in the replayed `report.json` is
reported as two missing artifacts plus a `report.json` difference in `counts`;
injecting a dropped `blocks.preds` key into `schema-comparison.json`, putting an
absolute path back into `report-candidate.json.input`, and truncating
`marker-replay.tsv` each fail `verify` with the matching message.

`verify` is not wired into `scripts/ci-check.sh` yet. Both that script and
`scripts/lint-python.sh` are protected paths in section 7 of
`oracle-protocol-ir-cfg-emitter.md`, so adding a step to them is a ruler change
and needs its own section 9 adjudication commit, which this evidence slice
deliberately does not bundle.

Reproducing the reference side needs a second clean build at `1371e42`:

```bash
git worktree add --detach ../ref-1371e42 1371e42
cd ../ref-1371e42
CARGO_TARGET_DIR=../target-ref nix develop <repo> -c \
  cargo build -p flutterdec-cli --release
```

The two binary digests in section 1 are the ones this workspace produced;
crate metadata hashes depend on the build path, so treat them as a record of
what was run rather than as a value a different machine must reproduce. The
artifact manifest is the comparison that has to hold.

## 9. Limits

- One input, one architecture, one adapter backend. `arm32v7`, `x64`, and the
  Google Play build of the same release are untested here.
- Both sides exit 1 on the default quality gates. This baseline proves
  compatibility of the emitted artifacts, not that the run passes the gates.
- The 3-marker `unresolved_cf` residue in section 7 is open and belongs to the
  emitter contracts.
- One operand pair in section 6.2 is open: `sub_90b144(reg0)` becomes
  `sub_90b144()` in `pseudocode/05149_sub_bdd098.dartpseudo`. It is the only
  replacement in the 3672 differing files where the candidate prints fewer
  call arguments than the reference, and it is a declined register-argument
  claim rather than an adjudicated correction.
- The input is fetched, never vendored, so an independent rerun needs network
  access to the pinned GitHub release asset.
- `check-compat-baseline.py verify` is not a CI step, for the reason in
  section 8, so nothing re-runs it automatically.
