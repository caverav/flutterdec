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
| Candidate revision | `22146970e5d2fd1fd223297aec0b2b06826dfbd9` |
| Reference product tree | `23165413ab8e29b08ac71bd712aaf607154aea090ae1680170472f05d3a8e6f3` (106 files) |
| Candidate product tree | `9dae6d8258663b359fb960e3546ec84cc26a672289c4d25f0293f7213edbbc75` (142 files) |
| Reference binary sha256 | `a8eef0988e6f25c0da07ffe0dfdadad217272625c523bdaf87c95787f5ac604c` |
| Candidate binary sha256 | `7fe12b2f592159f102616722e266ee0f399a59f9a37568a3ef86f69a8f2e61f3` |
| CLI version | `flutterdec 0.1.0-alpha.4` on both sides |
| Toolchain | `nix develop`, rustc 1.92.0, cargo 1.92.0, profile `release` |
| Environment | `LC_ALL=C`, `TZ=UTC`, identical absolute input path on every run |

Both sides were built clean, in separate worktrees, into separate
workspace-backed target directories, from `rust-toolchain.toml` and
`flake.lock` that are byte-identical at the two revisions.

**What the candidate revision is, and what it is not.** `2214697` is the
*product-state* revision: the state of the code that decides the emitted bytes,
which is what these artifacts were built from. It is deliberately never advanced
to the docs commit that carries this evidence. Advancing it would change what the
artifacts are claimed to come from, and no commit can name itself anyway. The
property that has to keep holding is that the product tree has not moved since,
so it is pinned by digest: sha256 over one `<path>\t<git blob object id>\n` line
per tracked file under `Cargo.lock`, `Cargo.toml`, `adapters/`, `crates/`,
`flake.lock`, `flake.nix`, `rust-toolchain.toml` and `symbols/`, paths in
ascending byte order, no header. `check-compat-baseline.py verify` recomputes it
at `2214697`, at `1371e42`, and at the current `HEAD`, and fails if `HEAD` has any
product-path delta from the recorded candidate tree. The evidence and prose live
outside those paths on purpose, which is what lets the record keep moving while
the pin stays still.

The revision that *delivers* this evidence is therefore not recorded in
`input-recipe.json` - it cannot be, without a self-referential hash. Read it from
the repository: `git log --oneline -- docs/compat-baseline-real-binary.md
docs/compat-evidence/`. Every one of those commits touches zero product paths,
which is exactly the property `verify` checks.

## 2. Fetch the input

```bash
python3 scripts/check-compat-baseline.py fetch --dest /tmp/localsend-1.17.0-arm64v8.apk
```

The fetch fails unless the downloaded asset matches both the recorded byte
size and the recorded SHA-256, so a moved or re-cut release cannot silently
become the baseline input. It also never writes or replaces `--dest` with
unverified newly downloaded bytes: the download goes to the temporary sibling
`<dest>.part`, size and digest are checked there, and `--dest` is replaced only
once both match. The temporary file is removed on every failure path, including a
`curl` that dies mid-transfer, so a bad download cannot be picked up by the next
run. An existing `--dest` that already verifies is reused and not re-downloaded;
one that does not is re-fetched and replaced rather than trusted.

That is the whole claim, and the one case it does not cover is worth stating: a
file that was already invalid at `--dest` before the run is removed only by a
successful replace, so if the re-fetch itself fails it is still there afterwards.
What holds unconditionally is that every invocation revalidates whatever is at
`--dest` against the recipe and never silently reuses it, so a surviving invalid
file cannot be mistaken for the baseline input by any later `fetch`, and the
decompile in section 8 runs on a path `fetch` exited 0 on. Manual equivalent:

```bash
curl -fsSL -o /tmp/localsend-1.17.0-arm64v8.apk \
  https://github.com/localsend/localsend/releases/download/v1.17.0/LocalSend-1.17.0-android-arm64v8.apk
sha256sum /tmp/localsend-1.17.0-arm64v8.apk
```

## 3. Install the adapter, then run

`adapters/installed/*` is gitignored (`.gitignore:11`), so a cold checkout has
no adapter and the decompile aborts with `adapter not installed for hash
80a49c7111088100a233b2ae788e1f48` **before writing a single artifact**. One
offline step fixes that, from the repository root, on both sides:

```bash
target/release/flutterdec adapter install --dart-hash 80a49c7111088100a233b2ae788e1f48
```

It needs no network: it generates a 329-byte shim,
`adapters/installed/dart_adapter_80a49c7111088100a233b2ae788e1f48`
(sha256 `a9cd861143bf91c46f484e79dd5cd3180fd329883d3dd3b02cf8dda16af45f95`),
from the tracked template `adapters/python/adapter_template.py`. It leaves
`adapters/manifest.json` untouched, because the pinned snapshot hash is already
an entry there, so the tracked tree stays clean. The machine-readable form is
`input-recipe.json.adapter`, and `check-compat-baseline.py verify` fails if this
step is missing from this document.

The adapter is resolved from the current working directory upwards, so the run
has to happen with the checkout as the working directory; the built binary can
live in any target directory.

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
| candidate `2214697` | 1 | placeholder if-count exceeded; unresolved control-flow count exceeded | 133 s |
| candidate, second process | 1 | same two reasons | 133 s |
| candidate, third process | 1 | same two reasons | 133 s |
| candidate, fourth process, input copied to a different absolute path | 1 | same two reasons | 133 s |

`placeholder_ifs=501 unresolved_cf=0 indirect_call_ratio=0.156
disassembly_ratio=1.000` on the reference; `placeholder_ifs=530
unresolved_cf=521 indirect_call_ratio=0.158 disassembly_ratio=1.000` on the
candidate.

## 4. Artifact sets

Each run wrote 17402 files: 5800 `pseudocode/*.dartpseudo`, 5800 `ir/*.json`,
5800 `asm/*.s`, plus `quality.json` and `report.json`. All five runs have the
**same 17402 relative paths**; no file was added, renamed, or lost, and
`quality.json.function_count` is 5800 on both sides.

| Manifest | sha256 of the per-artifact manifest |
| --- | --- |
| reference | `cb156a879817b9db2e2ed416ca4715df2b7a754cf11032f968b6416c96d4cdc0` |
| candidate | `3879e3728baffe0ec7ea58b73ddf3439dc2bf2f97d5857a5cc9af65862319a8a` |
| candidate, second process | `3879e3728baffe0ec7ea58b73ddf3439dc2bf2f97d5857a5cc9af65862319a8a` |
| candidate, third process | `3879e3728baffe0ec7ea58b73ddf3439dc2bf2f97d5857a5cc9af65862319a8a` |

Each of those four values is the sha256 of a one-run manifest: one
`<path>\t<bytes>\t<sha256>\n` line per emitted artifact, paths in ascending byte
order, no header, trailing newline included. Nothing else is in the hashed text.
Two independent recomputations therefore reproduce all four:

```bash
# offline, from the committed per-artifact rows: reference and candidate
python3 scripts/check-compat-baseline.py verify

# from a fresh output tree, printed at the end of a replay
python3 scripts/check-compat-baseline.py replay --out /tmp/compat-out
```

`verify` resolves the `=` shorthand in the candidate columns of
`artifact-manifest.tsv`, rebuilds each side's manifest text, and fails unless
the digest equals the recorded one; the three candidate process fields are the
same bytes, so the candidate derivation covers all three. The derivation is
recorded as `input-recipe.json.artifacts.manifest_digest_derivation` and
implemented by `side_manifest_text` and `tree_manifest_digest` in
`scripts/check-compat-baseline.py`. One caveat, measured rather than assumed:
the digest of a *fresh* tree includes `report.json`, which carries the three
volatile workspace strings below, so it equals the recorded value only for a
replay in the recorded workspace. The per-artifact comparison that `replay`
performs is the check that holds from any checkout. Measured here, from a fresh
run of the section 8 recipe in this workspace: `replay` reproduced the recorded
candidate digest
`3879e3728baffe0ec7ea58b73ddf3439dc2bf2f97d5857a5cc9af65862319a8a` exactly, all
17402 lines included. The earlier slice measured the other half of the same
claim from a cold clone outside this workspace, against the digest it recorded:
exactly one of the 17402 lines differed, `report.json`, and substituting the
recorded line reproduced that digest. Both halves say the same thing - the one
line that follows the workspace is `report.json` - and the per-artifact
comparison is what a different checkout should rely on.

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

3672 files differ: 103986 removed lines against 1930418 added lines. Those two
totals, and every per-file `removed_lines`/`added_lines` in section 6.4, are
counted from Python `difflib.unified_diff(reference_lines, candidate_lines, n=0)`
over `splitlines(True)`, skipping the `---`/`+++`/`@@` lines. The implementation
is part of the definition: GNU `diff -U0` and `git diff -U0` group hunks
differently and reproduce neither total. The
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
| `// indirect branch through regN: target not recovered` | 0 | 520 | Unrecovered indirect branches are now stated; the reference emitted no such marker and counted `unresolved_cf = 0`. It did emit text at those sites - the fallthrough it invented past `br Xn` - and section 6.4 accounts for that text. |
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
(`difflib.unified_diff(n=0)` hunks that replace exactly one line with exactly one
line - the same implementation caveat applies, `SequenceMatcher` opcodes and GNU
`diff -U0` both give different splits):
9060 gain `regN` tokens, 1984 keep the same number, and 78 lose them. All 78 are
enumerated in
[compat-evidence/operand-direction-losses.tsv](compat-evidence/operand-direction-losses.tsv),
one row per replacement, and every row carries its own `adjudication_class` from a
fixed four-value vocabulary:

| `adjudication_class` | Rows | Example |
| --- | --- | --- |
| `expression_replaced_by_the_register_holding_it` | 63 | `if (((reg4 + (reg8 << 2))).f16 == reg4)` becomes `if (reg9 == reg4)` |
| `register_named_as_parameter_slot` | 11 | `smiUntag(reg5.f12)` becomes `smiUntag(slot3.f12)` |
| `line_replaced_by_structural_or_comment_line` | 1 | `if (!(reg0 != 6)) {` becomes `else {` |
| `other` | 3 | `t16.f8 = reg2;` becomes `t16.f8 = (tmp3 + (tmp1 - 1));` |

This column is a declared reading and not a measurement, and the record says so in
both places it is recorded:
the four-way split of the fewer_registers rows is committed human adjudication of
the two rendered lines, carried per row in the operand-direction-losses.tsv
`adjudication_class` column; it is not a syntax metric inferred from the rows, so
`verify` recounts the column against the recorded totals rather than re-deriving
the class. What that buys is auditability rather than derivation: the counts used
to stand only in this table, over a three-column file a reader could not recount,
and they are now the sum of 78 individually stated readings. The vocabulary is
closed - `verify` fails on a fifth value, on a row carrying none, and on any
per-class count that disagrees with
`difference-classes.json.operand_naming_fewer_register_classes`.

The largest class is the same conservative direction as the rest of the section:
the reference's compound expression is replaced by the single name the candidate
stands behind for that value, which is the holding register on 58 of those 63
rows and a temporary derived from one (`reg4Minus1`, `reg1Minus1`) on the other 5.
`register_named_as_parameter_slot` is a recovery,
`line_replaced_by_structural_or_comment_line` is restructuring, and `other` is
the three rows that are none of the above: `01058_sub_6b17b4.dartpseudo` renders
`reg2` as the temporary `tmp3`, `01551_sub_7781e4.dartpseudo` renders `reg2` as
the expression `(tmp3 + (tmp1 - 1))`, and `05149_sub_bdd098.dartpseudo` is the one
case in the 3672 differing files where the candidate prints strictly less at a
call site: the reference emits `sub_90b144(reg0)` and the candidate emits
`sub_90b144()`, declining the register-argument claim. Section 6.5 adjudicates
that row from the call site's own instructions.

**Guard polarity.** Three of the 78 rows carry a second difference that cuts
across the four classes above and that none of them describes, so the table
carries a fifth column, defined once and recorded here: `guard_polarity` is
the operand-direction-losses.tsv column, derived from the two rendered lines and
not adjudicated there: reference_eq_zero_candidate_ne_zero on a row whose
reference_line and candidate_line both start with `'if ('` while the reference
ends with `'== 0) {'` and the candidate with `'!= 0) {'`, and none on every other
row. It cuts across adjudication_class, which the flagged rows keep, and verify
re-derives the column from the row bytes instead of trusting it. Which polarity
is correct is decided outside this column, by semantic-adjudication.json over the
ARM64 spans: the candidate is right and the reference inverted the guard.
The three sites are exactly these, with the reference line above the candidate
line that replaces it:

```
pseudocode/03119_sub_936128.dartpseudo
- if ((((reg4 + reg3) + smiUntag((((((reg3 == (reg1 & 0xffffffff)) & 0x10) == 0) ? 1 : 0) << 1))) & 0xc0000000) == 0) {
+ if ((((reg4 + reg3) + reg3) & 0xc0000000) != 0) {
pseudocode/05530_sub_c8b9b8.dartpseudo
- if ((((reg2 + reg4) + smiUntag((((((reg4 == (reg3 & 0xffffffff)) & 0x10) == 0) ? 1 : 0) << 1))) & 0xc0000000) == 0) {
+ if ((((reg2 + reg4) + reg2) & 0xc0000000) != 0) {
pseudocode/05548_sub_c93e0c.dartpseudo
- if ((((((((reg5 & 0xffffffff) >>> 0x10) + (reg4 & 0xffffffff)) + (((reg2 & 0xffffffff) & reg3) << 0x10)) + smiUntag((((((reg7 == (reg6 & 0xffffffff)) & 0x10) == 0) ? 1 : 0) << 1))) + smiUntag((((((((reg5 & 0xffffffff) & 0xffffffff) == reg5) & 0x10) != 0) ? 1 : 0) << 0x11))) & 0xc0000000) == 0) {
+ if (((((((reg7 >>> 0x10) + reg4) + (reg4 << 0x10)) + reg2) + reg2) & 0xc0000000) != 0) {
```

In all three the guarded body is structurally identical on the two sides -
`if (thread.f88 <= (thread.f80 + 0x10)) {` and one `sub_d51af0(...)` call, which
is the row immediately below each guard in the table and differs only in the
operand spellings that class already covers - so the guard's own terminal
comparison is the whole difference. All three keep the
`expression_replaced_by_the_register_holding_it` class they already carried. The
vocabulary is closed at those two values and `verify` fails when a row carries a
third, when the column and the direction re-derived from the row bytes disagree,
when the flagged set is not exactly these three files, when a flagged row's
`adjudication_class` moves, or when this document stops naming a site or renders
one of its six lines differently. The reverse flip - a reference `!= 0` guard
becoming `== 0` - does not occur in the 78 rows, and
`difference-classes.json.operand_naming_guard_polarity.reverse_direction_rows`
records that 0 and is recounted the same way. Section 6.5 settles which side is
right, from the machine code.

**Helper resolution.** All 27097 `_block_N` occurrences resolve: every
referenced helper has a definition in the same file, in all 5800 candidate
files, with zero dangling references. The reference has no helpers at all.

**Unknown semantics.** Unknown values keep the same vocabulary on both sides
(`receiver: unrecovered`, `reg<N>`, `poolOff[...]`, `/* cond */`); the
candidate adds the two unknown *control* effects - trap and unrecovered
indirect branch - that the reference left unstated. No value the reference
resolved is restated as unknown: every changed operand pair is in the four
classes above, and none of them replaces a resolved value with an unknown
marker. Resolved pseudocode did disappear, though - whole call renderings, not
just operand spellings - wherever the reference's invented control flow was the
only thing that reached it. That is a separate class and section 6.4 enumerates
it file by file.

### 6.4 Pseudocode the candidate no longer emits

The candidate emits 7.7x more text overall, but 103986 lines are removed, and
some of those lines are call renderings that vanish from a file entirely. Every
differing file is accounted for
([compat-evidence/pseudocode-callee-removals.tsv](compat-evidence/pseudocode-callee-removals.tsv),
215 rows, aggregates in
[compat-evidence/callee-removals-aggregate.json](compat-evidence/callee-removals-aggregate.json)).
A *callee rendering* is `<callee>(` for the three emitted callee spellings
(`sub_<hex>`, `fn_0x<hex>`, `sel<N>`); a callee has *wholly vanished* from a file
when the reference rendered it at least once and the candidate renders it
nowhere in that file.

4045 callee renderings are lost against 139285 gained. 215 files lose at least
one; in 85 of them at least one callee wholly vanishes, 170 distinct callees
across 1975 renderings. The 103986 removed lines partition over the classes with
nothing left over:

| Class | Files | Removed lines | Adjudication |
| --- | --- | --- | --- |
| `no_callee_rendering_lost` | 3457 | 65656 | Restructuring and operand renaming only, sections 6.1 and 6.2. Every callee the reference rendered is still rendered. |
| `fewer_renderings_same_callees` | 130 | 26667 | The same callees, rendered fewer times: a block the reference repeated inline is now emitted once as a `_block_N` helper and called, so duplicate renderings collapse. No callee is lost. |
| `vanished_behind_indirect_branch` | 58 | 7964 | The removed text sat behind a `br Xn`. Adjudicated below. |
| `vanished_behind_indirect_branch_and_trap` | 11 | 2359 | Both effect ops appear among the lost edges bounding that file's vanished calls: in 5 of the 11 every such region is bounded by both ops, in the other 6 the vanished calls sit in two to four regions that are not all bounded by both. Same adjudication as the two single-effect classes. |
| `vanished_behind_trap` | 14 | 1180 | The removed text sat behind a `brk`, which is the section 6.1 class rendered at the pseudocode surface. |
| `dispatch_selector_rendering_only` | 2 | 160 | Two files lose only `sel<N>(` renderings, whose selector is recovered from a dispatch table rather than from a call target, so no IR call instruction carries the name. Declared open in section 9. |

**What the table's columns mean.** Three of them are easy to read as something
they are not, so each states its own scope:

- `ir_reference_only_instructions` is per file: instruction addresses present in
  that function's reference `ir/*.json` and absent from its candidate
  `ir/*.json`. It is measured on **every** row, not only on the rows with a
  wholly vanished callee. 104 rows are nonzero, for 244 instructions: 238 on 101
  `fewer_renderings_same_callees` rows (97 rows of 2, two of 4, one of 6, one of
  30) and 6 on 3 `vanished_behind_trap` rows of 2 each. The value histogram over
  all 104 nonzero rows - 100 twos, two fours, one six, one thirty - is recorded in
  the aggregate and `verify` checks the column against it.
  The other 690 of the run's 934 sit outside the table, 678 in differing files
  that lose no callee rendering and 12 in files whose pseudocode is byte-identical.
  244 + 690 = 934, which is the whole `after_trap:brk` class section 6.1
  adjudicates, so every reference-only instruction in the run is placed and none
  is a real loss.
- `candidate_marker` is whole-file marker presence in the candidate pseudocode -
  `both`, `indirect_branch_only`, `trap_only` or `none` - and is recorded on every
  row. It is *not* the class: `00465_sub_6516f8.dartpseudo` is bounded by a trap
  edge yet carries no `// trap:` marker, because the marker census counts
  *emitted* markers and an unemitted region emits none. Over all 215 rows it is
  121 `trap_only`, 71 `indirect_branch_only`, 23 `none`, 0 `both`; over the 85
  files with a wholly vanished callee it is 70 `indirect_branch_only` and 15
  `trap_only`.
- `lost_edge_effects` counts effect **occurrences**. Its unit is
  the `pseudocode-callee-removals.tsv` column, an effect-occurrence count and
  not a count of distinct control-flow edges: its keys are keyed on the tuples in
  `lost_edge_effects_algorithm`, so one lost edge bounding a region that holds
  three vanished callees is counted three times. Distinct address-level lost edges
  are counted separately in `callee-removals-aggregate.json distinct_lost_edges`.
  That unit is why the column sums to 385, 303 and 68 rather than to an edge
  count.

**The algorithm the `lost_edge_effects` column is derived by.** "That block's
unreachable region" has neighbouring readings that answer differently on the same
trees, so the six clauses below are the definition; they are carried verbatim in
`difference-classes.json.definitions.lost_edge_effects_algorithm` and in
`callee-removals-aggregate.json.lost_edge_effects.algorithm`, and `verify` fails
if any of the three copies is dropped or reworded.

1. The entry block is the candidate block whose `start_va` equals the function's
   `entry_va`, and a candidate block is unreachable when no directed path of
   successor edges reaches it from that entry block.
2. The region of a candidate block is its weakly connected component in the
   subgraph induced by the unreachable blocks, taking a successor edge between two
   unreachable blocks as undirected.
3. A lost reference edge bounds that region when its head address falls inside
   the component: the head resolves to the candidate block that starts at that
   address, or failing that to the candidate block holding an instruction at it.
4. `lost_edge_after_<op>` is one occurrence per (file, wholly vanished callee,
   candidate block holding a `Call` to that callee, distinct candidate tail op
   among the lost edges bounding that block's region).
5. `disposition_<D>` is one occurrence per (file, wholly vanished callee,
   unreachable candidate block holding a `Call` to that callee).
6. `dispatch_selector_rendering` is one occurrence per (file, wholly vanished
   `sel<N>` callee), not one per lost rendering.

A file's `vanished_behind_<ops>` class name is that file's `lost_edge_after_<op>`
tail ops, each op in snake_case, in ascending op order, joined by `_and_`; the ops
of one file need not all bound the same region.

Only that reading reproduces the recorded 303 and 68. Taking the region as every
unreachable block in the file instead gives 350 and 90, re-derived from the same
two output trees. Clause 3's second half never decides a count in this run -
every bounding head is a block start - and clauses 1 and 2 are in code as
`unreachable_regions` in `scripts/check-compat-baseline.py`, which `--self-test`
runs on a graph where the whole-file and per-component readings disagree, and
again with one successor edge reversed, which is what pins the undirected half of
clause 2. No count is claimed here for any other reading of that clause.
Clause 6 is the one unit the
committed rows can settle on their own: the 85 rows carry 34 wholly vanished
`sel<N>` callees over 14 files against 370 lost `sel<N>` renderings, and `verify`
recounts both from `vanished_callee_names` and `lost_rendering_detail` and fails
if the recorded 34 is the rendering figure instead. Three further keys the
derivation can emit - `callee_absent_from_candidate_ir`,
`lost_edge_after_absent`, and `reachable_block_not_rendered` - do not occur in
this run, which `verify` also holds.

**Why the removed text was unreachable.** Attribution is the same
nearest-earlier-instruction rule the `definitions` object already uses for
exclusive instructions, applied to edges. Address-level edges are built for both
sides - sequential adjacency inside a block, plus one edge from a block's last
instruction address to each successor block's start address. An edge the
reference had and the candidate does not is a lost edge, and the control effect
that removed it is the candidate op at the edge's tail address. For all 83 files
where an address-carrying callee vanished, the block holding the call is
unreachable from the function entry in the candidate, its ledger disposition is
`RetainedUnreachable`, and every lost edge bounding its unreachable region has a
tail whose candidate op is `IndirectBranch` or `Trap`. There is no other tail op
in the whole class.

Counted as distinct address-level edges instead - the same `lost_edge`
definition, deduplicated, over the 85 files with a wholly vanished callee - the
numbers are smaller and are recorded separately in
`callee-removals-aggregate.json.distinct_lost_edges`:

| Distinct lost edges, 85 files | `IndirectBranch` | `Trap` | tail absent from the candidate IR | total |
| --- | --- | --- | --- | --- |
| anywhere in the file | 142 | 59 | 6 | 207 |
| bounding an unreachable region | 142 | 52 | 0 | 194 |

The two rows differ because seven `Trap`-tailed lost edges in these files do not
land in an unreachable block at all, and because the 6 edges whose tail
instruction is not in the candidate IR are exactly the 6 reference-only
instructions of the trap class - the same 6 counted in the column above. No
distinct lost edge in the class has any other tail op, which is the claim; 303
and 68 are the occurrence sums, and the two units are not interchangeable.

`br Xn` is a register-indirect jump and `brk` traps: neither has a fallthrough.
The reference classified both as `Other`, which is not a terminator, so the IR
builder kept the block open and pulled the following bytes into it - which is
how the reference reached that code at all. Removing the invented edge makes
those blocks unreachable, so the emitter no longer renders them. The IR keeps
them: across the 58 indirect-branch files the reference has **0** instructions
the candidate lacks, and the only reference-only instructions anywhere in the 85
wholly-vanished files are 6, in 3 files of the trap class. They are part of the
934 `after_trap:brk` instructions section 6.1 already adjudicates, as are the 238
on the `fewer_renderings_same_callees` rows.

One representative file per class, all of them rows of the removal table:

| Class | Representative | What it loses |
| --- | --- | --- |
| `no_callee_rendering_lost` | not in the table by construction | nothing |
| `fewer_renderings_same_callees` | `00054_sub_60b138.dartpseudo` | 2 duplicate `sub_d4fcf4(` renderings, callee still present |
| `vanished_behind_indirect_branch` | `00033_sub_60963c.dartpseudo` | `sub_d50264`, 4 renderings, 2 `RetainedUnreachable` blocks behind one `IndirectBranch` lost edge |
| `vanished_behind_indirect_branch_and_trap` | `00465_sub_6516f8.dartpseudo` | 2 callees, 10 renderings, 4 lost edges of each effect |
| `vanished_behind_trap` | `00249_sub_62f4d8.dartpseudo` | `fn_0x62f620`, 3 renderings behind one `Trap` lost edge |
| `dispatch_selector_rendering_only` | `01540_sub_772c00.dartpseudo` | `sel4096`, 18 renderings; the second file of this class is `01723_sub_7cf2b0.dartpseudo` |

Worked example, in the committed diff:
`pseudocode/00096_sub_60f780.dartpseudo` renders `sub_bbe8a0(` 8 times in the
reference and nowhere in the candidate. In `ir/00096_sub_60f780.json` the
reference has 14 blocks. Its block 9 starts at `0x60f7e8` with
`ldr x16, [x26, #0x278]`; `br x16` is the block's *second* instruction, at
`0x60f7ec`, and because `br x16` was `Other` the block does not end there - it
runs on for ten more instructions, through `b.ls #0x60f8b4` at `0x60f814`, twelve
in all.

The candidate has 16 blocks. Block 9 is those first two instructions and ends at
`br x16`, now an `IndirectBranch` with no successors, and `0x60f7f0` onward
becomes blocks 10 to 15. All six are unreachable from `entry_va` and all six are
`RetainedUnreachable`; only blocks 10 and 15 have an empty predecessor set, while
11 to 14 have predecessors inside that same unreachable region (11 from 10 and
15, 12 from 11, 13 from 12, 14 from 11, 12 and 13). Unreachable from the entry is
the property that matters here, not an empty predecessor list. `bl #0xbbe8a0`
sits in blocks 11 and 13, twice in each. No instruction is missing from the
candidate IR: this file's `ir_reference_only_instructions` is 0.

So this class is the pseudocode-surface consequence of the same correction as
section 6.1: text the reference reached only through an edge the program does
not have. It is a real reduction in emitted text, and it is not a loss of
recovered code - the instructions are in the byte-identical `asm/*.s` and in the
candidate IR, which retains and accounts for them as `RetainedUnreachable`.

### 6.3 Quality and report values

Nineteen `report.json` values change, fifteen of them inside the `quality`
block it embeds and four outside it (`call_fallback.dispatch_target_invoke`,
`call_fallback.generic_invoke`, and the two `shared_stub_naming` noreturn
counters). All of them follow from 6.1 and 6.2. The ones worth
naming: `unresolved_cf` 0 -> 521 (previously unstated indirect branches),
`block_helper_refs` 0 -> 27097, `total_calls` 45955 -> 41883 and
`indirect_calls` 7149 -> 6627 (calls behind an invented post-trap fallthrough
are no longer counted), `unlifted_instructions` 689 -> 481,
`omitted_path_markers` 355 -> 50, and the new `emission` object.

### 6.5 Machine-level adjudication of the behaviour-affecting differences

Four of the differences above change what the artifact says the program does, and
none of them can be settled by counting text. Each one is derived here from the
ARM64 instruction spans, the destinations those instructions encode, and the two
emitted files - never from a prior rendering, a prior report, or the emitter's own
account of itself. The derivations, with every span they rest on, are in
[compat-evidence/semantic-adjudication.json](compat-evidence/semantic-adjudication.json),
and `scripts/check-compat-semantics.py` re-derives all four from the two output
trees and fails on any disagreement:

```bash
python3 scripts/check-compat-semantics.py --self-test     # the rules themselves
python3 scripts/check-compat-semantics.py verify          # offline: the record
python3 scripts/check-compat-semantics.py check \
  --reference /tmp/ref-out --candidate /tmp/compat-out    # the derivation
```

**Guard polarity: the candidate is right and the reference inverted it.** The
seven files that render this guard shape hold 15 mask-test sites, and every one of
them is the same three-instruction span: `tst Xn, #0xc0000000`, then `b.eq` to an
address past the guarded body, then the body itself. `tst` is `ANDS xzr, Xn, #imm`
(Arm ARM DDI 0487, C6.2), so it sets Z exactly when the masked bits are zero, and
`b.eq` is taken exactly then. Every one of those sites is `tst Xn, #0xc0000000`
followed immediately by `b.eq`, which branches past the guarded body exactly when
the masked bits are zero, so the body runs when they are not and `!= 0` is the
guard the machine code carries. The candidate renders all 14 emitted guards that
way and the reference inverted three of them. The body is identified by what it
contains rather than by position: the `ldp` of the Thread stack-limit pair at
`[x26, #0x50]`, the `+ 0x10`, the `cmp`, and a `b.ls` whose destination calls the
shared stack-overflow stub at `0xd51af0`. In `03119_sub_936128` the first site is
`tst x3, #0xc0000000` at `0x9361a4`, `b.eq #0x9361d8` at `0x9361a8`, and the body
from `0x9361ac` (`ldp x0, x4, [x26, #0x50]`) through `b.ls #0x936430` at
`0x9361b8`, whose target reaches `bl #0xd51af0` at `0x936438`. No site in the
seven files is followed by `b.ne`, so no emitted guard there derives `== 0`, and
the checker's self-test proves the rule reads the condition code by deriving the
opposite answer from a planted `b.ne`.

The emitted side of that shape is counted the same way on both trees: an emitted
line whose stripped text ends with `'& 0xc0000000) == 0) {'` or
`'& 0xc0000000) != 0) {'` and whose next emitted line contains `'thread.f88 <='`,
counted over `pseudocode/*.dartpseudo` on each side, occurs 14 times in the same 7
files. The reference renders 11 of them != 0 and only these 3 == 0, and the
candidate renders all 14 != 0. Five of the reference's 11 sit inside the three
flagged files - three in `03119_sub_936128.dartpseudo` and one each in the other
two - so each of those functions renders that shape both ways in the reference
itself, which is what first made the flip look like a correction. The machine code
is what settles it, and the 15 sites say the same thing at every one of them. The
one file whose site count does not match its emitted count is
`04626_sub_b14cf4`, with 2 sites and 1 emitted guard: the second guard's block is
not emitted, so there is no line to compare, and the derivation covers the sites
rather than the lines.

All 15 sites, each one a `tst`/`b.eq` pair whose fall-through is the checked body,
so each one derives `!= 0`:

| Function | Mask test | Branch | Destination | Body from | Slow path | Stub call |
| --- | --- | --- | --- | --- | --- | --- |
| `03097_sub_9264c4` | `tst x1, #0xc0000000` at `0x926548` | `b.eq #0x92657c` at `0x92654c` | `0x92657c` | `0x926550` | `0x92655c` | `0x926674` |
| `03119_sub_936128` | `tst x3, #0xc0000000` at `0x9361a4` | `b.eq #0x9361d8` at `0x9361a8` | `0x9361d8` | `0x9361ac` | `0x9361b8` | `0x936438` |
| `03119_sub_936128` | `tst x0, #0xc0000000` at `0x936230` | `b.eq #0x936264` at `0x936234` | `0x936264` | `0x936238` | `0x936244` | `0x936458` |
| `03119_sub_936128` | `tst x1, #0xc0000000` at `0x9362ec` | `b.eq #0x936320` at `0x9362f0` | `0x936320` | `0x9362f4` | `0x936300` | `0x936478` |
| `03119_sub_936128` | `tst x1, #0xc0000000` at `0x9363b8` | `b.eq #0x9363ec` at `0x9363bc` | `0x9363ec` | `0x9363c0` | `0x9363cc` | `0x9364a0` |
| `03124_sub_936bd0` | `tst x2, #0xc0000000` at `0x936c00` | `b.eq #0x936c34` at `0x936c04` | `0x936c34` | `0x936c08` | `0x936c14` | `0x936cf0` |
| `03124_sub_936bd0` | `tst x2, #0xc0000000` at `0x936c7c` | `b.eq #0x936cb0` at `0x936c80` | `0x936cb0` | `0x936c84` | `0x936c90` | `0x936d0c` |
| `04626_sub_b14cf4` | `tst x2, #0xc0000000` at `0xb14d28` | `b.eq #0xb14d5c` at `0xb14d2c` | `0xb14d5c` | `0xb14d30` | `0xb14d3c` | `0xb14d80` |
| `04626_sub_b14cf4` | `tst x0, #0xc0000000` at `0xb14db8` | `b.eq #0xb14dec` at `0xb14dbc` | `0xb14dec` | `0xb14dc0` | `0xb14dcc` | `0xb14e90` |
| `05530_sub_c8b9b8` | `tst x3, #0xc0000000` at `0xc8ba10` | `b.eq #0xc8ba44` at `0xc8ba14` | `0xc8ba44` | `0xc8ba18` | `0xc8ba24` | `0xc8bb58` |
| `05530_sub_c8b9b8` | `tst x3, #0xc0000000` at `0xc8bad4` | `b.eq #0xc8bb08` at `0xc8bad8` | `0xc8bb08` | `0xc8badc` | `0xc8bae8` | `0xc8bb84` |
| `05548_sub_c93e0c` | `tst x6, #0xc0000000` at `0xc93f64` | `b.eq #0xc93f98` at `0xc93f68` | `0xc93f98` | `0xc93f6c` | `0xc93f78` | `0xc940c0` |
| `05548_sub_c93e0c` | `tst x3, #0xc0000000` at `0xc94034` | `b.eq #0xc94068` at `0xc94038` | `0xc94068` | `0xc9403c` | `0xc94048` | `0xc940dc` |
| `05557_sub_cf0300` | `tst x4, #0xc0000000` at `0xcf04c0` | `b.eq #0xcf04f4` at `0xcf04c4` | `0xcf04f4` | `0xcf04c8` | `0xcf04d4` | `0xcf0610` |
| `05557_sub_cf0300` | `tst x3, #0xc0000000` at `0xcf054c` | `b.eq #0xcf0580` at `0xcf0550` | `0xcf0580` | `0xcf0554` | `0xcf0560` | `0xcf063c` |

**Selector losses: unreachable code, not lost recovery.** The address-level graph
of `01540_sub_772c00` and `01723_sub_7cf2b0` is rebuilt from the emitted asm
alone, giving each instruction the control effect its text has: `br Xn`, `ret`,
`brk` and an unconditional `b` have no fall-through, a conditional branch has both
edges, and a `bl`/`blr` falls through to its return address while its destination
leaves the function. Under those effects **every** dispatch call site in both
files is unreachable from the entry address. Inventing a fall-through after
`br Xn` - which is exactly what classifying `br` as a non-terminator did - makes
two of them reachable in each file, and they are the only ones: `0x772cf8` and
`0x772d28` in `01540_sub_772c00`, `0x7cf32c` and `0x7cf350` in
`01723_sub_7cf2b0`. Those are the sites whose 18 and 2 `sel<N>(` renderings the
reference emitted and the candidate does not. In `01540_sub_772c00` the invented
edge is the one after `br x16` at `0x772ca8`: the reference's block runs from
`0x772ca4` to `0x772cd0` and falls into `0x772cd4`, while the candidate's ends at
the `br`. So the class is the same correction as section 6.1 seen at the selector
surface, and no recovered call was lost.

**The dropped call argument: no recovered value disappeared.** At
`0xbdd0c0` in `05149_sub_bdd098` the instruction is `bl #0x90b144`. The emitter
claims an argument position only for a register the call site defined, in
`DartCallingConvention` order - `x1`, `x2`, `x3`, `x5`, `x6`, `x7` - and the only
writes to any of those, or to `x0`, between the function entry and that `bl` are
`mov x0, x1` at `0xbdd0a4` and `mov x1, x0` at `0xbdd0bc`. Following that chain,
the value in `x1` at the call is the function's own incoming `x1`: a pass-through,
which the stated lower bound does not claim as an argument. No argument position
carries a defined value, so `sub_90b144()` is the whole claim the call site
supports. The token the reference printed, `reg0`, is the unrecovered-value
spelling of `x0`, which is not an argument position at all, so what disappeared
was an arity claim over an unrecovered value and not a recovered argument.

**Control-flow accounting: reconciled by fixing the counter.** The candidate used
to emit 521 unresolved-control-flow statements while `quality.json.unresolved_cf`
reported 517. The cause was not a text difference: a block the DFS walk refuses to
inline is rendered by a nested emitter in `append_helper_functions` whose counters
are discarded when its lines are copied into the body, and `inline_helper_calls`
then copies that body to every call site, so both paths put statements into the
artifact that the walk never counted. The whole residue was one function:
`00860_sub_696734.dartpseudo` carries four `// indirect branch through reg2`
statements over a single reachable `br x2` at `0x6968fc` - its other two
`br x16` blocks are `NoreturnPruned` and `RetainedUnreachable` - and its counter
read 0. `2214697` recounts the statement shapes from the finished body, so the
counter is the number the artifact carries whatever path emitted it: 521 against
521 on the candidate, 0 against 0 on the reference, over 138 statement-bearing
functions. The regression test is
`crates/flutterdec-decompiler/tests/unresolved_cf_accounting.rs`, which fails on
the previous counter for both the helper-body case and the inlined-copy case.

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
- The five text-derived quality counters recount exactly from the emitted
  files on **both** sides: `block_helper_refs` (0 / 27097),
  `placeholder_cond_markers` (215 / 2617), `omitted_path_markers` (355 / 50),
  `loop_backedge_markers` (130 / 77), and `unresolved_cf` (0 / 521), which
  joined them in `2214697` and is the subject of section 6.5.

**`raw_register_name_refs` and its two counter scopes.** The structural census
counts `\breg\d+\b` over the whole emitted text and reports 54062 on the
reference and 692215 on the candidate. `quality.json.raw_register_name_refs` is
54062 and **690963**. The 1252 difference is a scope change in the counter, not
an unaccounted 1252 tokens, and both scopes are recounted in
[compat-evidence/register-counter-scopes.json](compat-evidence/register-counter-scopes.json):

| Scope | What it counts | Reference text | Candidate text |
| --- | --- | --- | --- |
| whole line | `count_ident_token` over `strip_join_annotation_span(line)`, for `x0..x30` and `reg0..reg30`, with no code-span filter - what `1371e42` ran | 54062 | 692215 |
| code span | `count_code_identifier_tokens` over the same stripped line, which visits emitter-owned code spans only - what the candidate runs | 53784 | 690963 |
| difference | tokens inside a non-code span | 278 | 1252 |

The reference revision computed the counter in the whole-line scope
(`crates/flutterdec-core/src/pipeline/helpers.rs` at `1371e42`), so its 54062
equals the census exactly. The candidate computes it in the code-span scope
(`crates/flutterdec-decompiler/src/lib.rs`, `count_code_identifier_tokens` over
`for_each_code_span`), which skips every string literal and every comment,
because a recovered pool string is program data and a comment is the emitter's
own prose. The whole 1252 is tokens the code-span filter excludes, and every one
of them sits in a line comment - 0 in block comments, 0 in string literals. They
split two ways with nothing left over: **731** in the combined `// target: ...`,
`// indirect via: ...` trailing comments and **521** in the
`// indirect branch through regN: ...` markers themselves, recorded as
`excluded_by_comment_shape`. Applying the candidate's scope to the reference text
gives 53784, which is 278 fewer than the reference reported (all 278 in `//
target:` comments; the reference emits no indirect-branch markers), so the scope
change is worth exactly this difference and nothing else moved.

**The `0..=30` boundary drops nothing here.**
`crates/flutterdec-core/src/pipeline/quality.rs:18` iterates `for n in 0..=30`,
so the quality counter only ever looks for `x0..x30` and `reg0..reg30`, while the
census regex `\breg\d+\b` has no upper bound. On this baseline the two ranges see
the same tokens: `reg31` occurs **0** times and `x31` occurs **0** times on both
sides, no token of either spelling carries an index above 30, and the highest
emitted register index is **`reg25`**. `xN` identifier tokens are 0 in the
pseudocode altogether - the emitter renders an unresolved register through
`named_register_alias`, which yields `regN` - and the handful of literal `x31`
substrings in the text are inside hex constants such as `0x31e`. That is why
`census_regN_over_whole_text` equals `whole_line_scope_total` exactly on both
sides rather than by coincidence, and it is why the entire 1252 is the code-span
filter and not the index bound. `verify` fails if any of those four counts stops
being zero.

`strip_join_annotation_span` removes 0 `regN` tokens on either side: the
annotation spans in this baseline carry none, and the code-span filter would
skip them regardless because an annotation is a block comment. The two counters
are both correct in their own scope; the census number is the one to quote for
emitted text, and the quality counter is the one to quote for emitted code. The
0.329-per-line figure in section 6.2 is the code-span counter over 2099918
emitted lines.

**Unresolved control flow, statement by statement.** The counter is the number
of unresolved-control-flow statements the artifacts carry, counted by the prefix
each one starts with - `// indirect branch`, `// unresolved branch target`,
`// unresolved jump`, and `// invalid CFG`, which is the whole body of a function
whose CFG did not validate and is one unresolved site (0 of them here) - and it
reconciles exactly on both sides: **521** statements
against `quality.json.unresolved_cf` 521 on the candidate, over 138 functions, and
0 against 0 on the reference. Section 6.5 records what that took: until `2214697`
the counter was the walk's own total, it read 517, and the four-statement
difference was one function whose statements were rendered into a helper body the
walk did not count.

Counting by prefix is part of the definition, and the census shows why.
`structural-census-candidate.json.unrecovered_indirect_branch` is 520 because it
matches `// indirect branch through \w+: target not recovered`, and in
`pseudocode/01224_sub_6cd918.dartpseudo` one statement carries an inline value
annotation between the register and the tail:
`// indirect branch through reg2 /* = slot0.f8 */: target not recovered`. A
whole-marker regex loses that line; the prefix keeps it, which is why the census
carries both numbers - the 520 it measured and
`unrecovered_indirect_branch_statement_prefix` 521 - and why the counter is
prefix-keyed. The regression test in
`crates/flutterdec-decompiler/tests/unresolved_cf_accounting.rs` pins that case
directly.

## 8. Re-running this baseline

```bash
# offline, no input needed: the committed baseline is internally consistent,
# drops no public schema key, and adjudicates every difference class it records
python3 scripts/check-compat-baseline.py verify
python3 scripts/check-compat-baseline.py --self-test   # the parser's own check

# with the real input: replay the candidate side and compare every artifact
python3 scripts/check-compat-baseline.py fetch --dest /tmp/localsend.apk
# required unless this host's nix.conf already enables both features: without it
# every `nix develop` line below, and scripts/ci-check.sh, fail with
# "experimental Nix feature 'nix-command' is disabled" before building anything
export NIX_CONFIG='extra-experimental-features = nix-command flakes'
nix develop -c cargo build -p flutterdec-cli --release
# offline, and required: a cold checkout has no adapter (section 3)
target/release/flutterdec adapter install --dart-hash 80a49c7111088100a233b2ae788e1f48
LC_ALL=C TZ=UTC target/release/flutterdec decompile /tmp/localsend.apk \
  --out /tmp/compat-out --adapter-backend internal --function-scope all \
  --emit-ir --emit-asm   # exits 1 on the quality gate, after writing artifacts
python3 scripts/check-compat-baseline.py replay --out /tmp/compat-out
```

Every command above runs from the checkout root, in that order, with nothing
else prepared by hand. Without the `adapter install` line the decompile exits 1
having written **0** artifacts, which is why `verify` fails when this document
stops naming it.

`replay` compares 17401 artifacts against the committed candidate digests and
`report.json` against the committed snapshot with the three volatile fields
normalized, so it fails on any lost file, any added file, and any changed byte
outside those fields. Both modes were plant-tested, each exiting 1: deleting two
artifacts and changing `counts.functions` in the replayed `report.json` is
reported as two missing artifacts plus a `report.json` difference in `counts`;
injecting a dropped `blocks.preds` key into `schema-comparison.json` and putting
an absolute path back into `report-candidate.json.input` each fail `verify` with
the matching message.

The four areas this document had to repair are plant-tested the same way, each
exiting 1 with its own message:

| Plant | `verify` failure |
| --- | --- |
| delete the `adapter install` line from section 3 and section 8 | `the adapter install step is not in compat-baseline-real-binary.md` |
| flip one digit of one `cand_sha256` in `artifact-manifest.tsv` | `artifacts.candidate_manifest_sha256 does not recompute from the per-artifact manifest: <digest>` |
| drop a row from `pseudocode-callee-removals.tsv` | `pseudocode-callee-removals.tsv has 214 rows, 215 files are claimed` |
| change `code_span_scope_total` in `register-counter-scopes.json` | `the code_span scope recounts <n> register tokens on the candidate, quality-candidate.json reports 690963` |

The precision repair on top of it is plant-tested the same way, each exiting 1:

| Plant | `verify` failure |
| --- | --- |
| change one row's `ir_reference_only_instructions` | `the per-row ir_reference_only_instructions do not sum to ir_reference_only_instructions_by_class: {...}` plus the histogram and total failures |
| blank one row's `candidate_marker` | `<file> carries the candidate_marker '', which is not one of ('both', 'indirect_branch_only', 'trap_only', 'none')` plus the all-rows count failure |
| change one row's `lost_edge_after_IndirectBranch` count | `the lost_edge_effects column does not sum to the recorded totals: {...}` |
| set `reg31_tokens` in `register-counter-scopes.json` to a nonzero value | `the candidate text carries <n> reg31_tokens, so the quality.rs 0..=30 boundary drops tokens the census counts and the two scopes are not comparable` |
| commit any change under `crates/` | `HEAD has a product-path delta from revisions.candidate (2214697): the product tree hashes <digest>, so the recorded artifacts were not produced by the product state at HEAD` |
| delete the `NIX_CONFIG` export from section 8 | `the NIX_CONFIG export the build line needs is not in compat-baseline-real-binary.md` |
| drop the exact `difflib` spelling, `n=0` argument included, from section 6.2 | `the diff implementation is not named in compat-baseline-real-binary.md` |

That last row is worded the long way round on purpose. A plant row that quotes a
required anchor verbatim satisfies the anchor itself, so the plant it describes
stops failing: the row has to name the anchor without spelling it. The same rule
applies to every row below.

The definition repair on top of that is plant-tested the same way. The first four
are the point of it: the recorded totals sum to themselves under a wrong reading
of the region rule, so the only defence is that the definition cannot be dropped
or reworded in one copy without failing here.

| Plant | Failure |
| --- | --- |
| reword clause 2 in section 6.2 | `verify`: `clause 2 of the lost_edge_effects algorithm is missing or altered in compat-baseline-real-binary.md` |
| edit `lost_edge_effects.algorithm` in `callee-removals-aggregate.json` | `verify`: `the lost_edge_effects algorithm in callee-removals-aggregate.json is missing or altered` |
| delete `definitions.lost_edge_effects_algorithm` from `difference-classes.json` | `verify`: `the lost_edge_effects algorithm in difference-classes.json is missing or altered` |
| reword `definitions.lost_edge_effects` in `difference-classes.json` | `verify`: `the lost_edge_effects unit in difference-classes.json is missing or altered` |
| record the rendering figure 370 as `vanished_sel_callees` | `verify`: `the dispatch_selector_rendering cross-check does not match the rows: {'vanished_sel_callees': 34, ...}` |
| drop `weakly connected component` from the script's clause 2 | `--self-test`: `AssertionError: weakly connected component` |

The clarity repair on top of that is plant-tested the same way. The first three
close the operand-direction split, which used to stand only as a prose table over
a file that could not be recounted; the last two are the `fetch` destination:

| Plant | Failure |
| --- | --- |
| blank one row's `adjudication_class` in `operand-direction-losses.tsv` | `verify`: `operand-direction-losses.tsv row <file> carries the adjudication_class '', which is not one of ('expression_replaced_by_the_register_holding_it', 'line_replaced_by_structural_or_comment_line', 'other', 'register_named_as_parameter_slot')` |
| move one row from `other` to `expression_replaced_by_the_register_holding_it` | `verify`: `the adjudication_class column does not sum to operand_naming_fewer_register_classes: {...}` |
| reword `definitions.operand_naming_fewer_register_classes` in `difference-classes.json` | `verify`: `the operand_naming_fewer_register_classes definition in difference-classes.json does not state that the split is a committed adjudication recounted from the per-row column` |
| perturb `input.bytes` or `input.sha256` in `input-recipe.json`, then `fetch` | `fetch`: `fetched bytes do not match the recipe: <size> <digest>`, with no file left at `--dest` and no `<dest>.part` residue |
| leave a stale file at `--dest`, then `fetch` | `fetch`: `<dest> does not match the recipe (<size> bytes, sha256 <digest>); re-fetching`, then exit 0 with the verified asset in place |

The polarity disclosure on top of that is plant-tested the same way. The column
is derived rather than adjudicated, so the first three plants are the ones that
matter: the recorded three sites cannot be widened, narrowed, or silently
detached from the direction the rows themselves state.

| Plant | Failure |
| --- | --- |
| blank one row's `guard_polarity` in `operand-direction-losses.tsv` | `verify`: `operand-direction-losses.tsv row <file> carries the guard_polarity '', which is not one of ('none', 'reference_eq_zero_candidate_ne_zero')` plus the counts failure |
| set one flagged row's `guard_polarity` to `none` | `verify`: `the guard_polarity column does not match the direction re-derived from the rendered lines: column [2 files], rows [3 files]`, plus the pinned-sites, `2 rows carry ...` and counts failures |
| flag a fourth row | the same derivation, pinned-sites and count failures, plus `the guard-polarity site <file> is not named in compat-baseline-real-binary.md` and both `is not rendered verbatim` failures |
| move a flagged row's `adjudication_class` to `other` | `verify`: `the guard-polarity row 03119_sub_936128.dartpseudo carries the adjudication_class 'other'; the flag is orthogonal and the class must stay expression_replaced_by_the_register_holding_it` |
| reword `definitions.operand_naming_guard_polarity` in `difference-classes.json` | `verify`: `the operand_naming_guard_polarity definition in difference-classes.json does not state that the column is re-derived and the flip unaccepted` |
| set `operand_naming_guard_polarity.adjudication.conclusion` to anything else | `verify`: `operand_naming_guard_polarity.adjudication does not conclude that the candidate is correct` |
| drop a clause of that definition from section 6.2 | `verify`: `the guard_polarity definition is missing or altered in compat-baseline-real-binary.md` |
| change one rendered guard line in section 6.2 | `verify`: `the guard-polarity row for 05530_sub_c8b9b8.dartpseudo is not rendered verbatim in compat-baseline-real-binary.md: <line>...` |
| drop a clause of the machine-level guard adjudication from section 6.5 | `verify`: `the machine-level adjudication of the guard-polarity flip is missing or altered in compat-baseline-real-binary.md` |
| change `sibling_guards.reference_ne_zero` in `difference-classes.json` to 10 | `verify`: `sibling_guards does not split guards_per_side on the reference side`, plus a `the sibling-guard count ... is not stated in compat-baseline-real-binary.md` failure quoting the section 6.5 sentence the wrong count builds |
| drop a clause of `sibling_guards.reading` | `verify`: `the sibling-guard reading behind the 14/7 figures is missing or altered in compat-baseline-real-binary.md` |
| let `guard_polarity_of` accept either direction | `--self-test`: `AssertionError` on `guard_polarity_of(guard_ne, guard_eq) == "none"` |

The section 6.5 adjudication is plant-tested against its own oracle, each exiting
1. The first three are the point of it: a conclusion the machine code does not
support has to fail here rather than read as settled.

| Plant | Failure |
| --- | --- |
| replace one site's guard branch with the opposite condition code, in both trees | `check`: `the machine code derives ['!= 0', '== 0'], the record claims != 0`, plus `guard site 0x9361a4 does not match the record` |
| do the same in one tree only | `check`: `asm/03119_sub_936128.s is not identical on the two sides`, which is checked before any guard is read |
| replace the register branch in a selector file's asm with a non-terminator | `check`: `01540_sub_772c00.dartpseudo: dispatch calls ['0x772cf8', '0x772d28'] are reachable without an invented fall-through` |
| give an argument position a defined value, by writing `x1` from a register the call site defines | `check`: `argument positions ['x1'] carry a defined value, so the candidate dropped a recovered argument`, plus the pass-through mismatch |
| perturb `quality.json.unresolved_cf` in the tree being checked | `check`: `candidate.quality_unresolved_cf derives 520, the record claims 521`, plus `candidate: 521 statements against quality.json.unresolved_cf 520` |
| drop a site from `semantic-adjudication.json` | `check`: `guard site 0x9363b8 is not in the record`; `verify`: `the per-file guard sites sum to 14, the record claims 15` |
| stop naming a derived site in this document | `verify`: `the guard site <va> is not named in compat-baseline-real-binary.md` |
| revert the counter to the walk's own total | `cargo test`: two failures in `unresolved_cf_accounting.rs`, the helper-body case and the inlined-copy case |
| let the derivation rule accept either polarity | `--self-test`: `AssertionError` on the planted opposite-condition span |

Neither `verify` nor `scripts/check-compat-semantics.py` is wired into
`scripts/ci-check.sh` yet. Both that script and `scripts/lint-python.sh` are
protected paths in section 7 of `oracle-protocol-ir-cfg-emitter.md`, so adding a
step to them is a ruler change and needs its own adjudication commit, which this
evidence slice deliberately does not bundle. The emitter side of the accounting
does run in CI: `crates/flutterdec-decompiler/tests/unresolved_cf_accounting.rs`
is a `cargo test --workspace` target.

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
- Section 6.5 adjudicates the four behaviour-affecting differences from the
  machine code, and what it proves is bounded by what it reads. The guard
  derivation covers the 15 mask-test sites of this one guard shape in these seven
  files, not every emitted guard in the run; the selector derivation is an
  address-level reachability argument over one function's own instructions, so it
  says nothing about a target reached from another function; and the argument
  derivation follows `mov` chains only, so a value assembled by arithmetic would
  be reported as an opaque write rather than followed.
- `scripts/check-compat-semantics.py check` needs both output trees, so like
  `replay` it is the independent-rerun path rather than an offline one. Its
  `verify` and `--self-test` modes are offline.
- The input is fetched, never vendored, so an independent rerun needs network
  access to the pinned GitHub release asset.
- `check-compat-baseline.py verify` is not a CI step, for the reason in
  section 8, so nothing re-runs it automatically.
- The product-tree check in section 1 needs `git` and needs `2214697` and
  `1371e42` to be reachable objects. In a shallow or exported checkout that has
  neither, `verify` prints that it skipped the check rather than passing it
  silently; every other check still runs.
