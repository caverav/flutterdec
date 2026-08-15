# Research: why the pseudocode does not look like Dart, measured

Status: research finding plus a validated prototype. Companion to
[#23](https://github.com/caverav/flutterdec/issues/23) (startup flow) and
[#42](https://github.com/caverav/flutterdec/issues/42) (snapshot
deserialization), which attack metadata recovery. This one attacks the other
half: what the decompiler does with the bytes once it has them.

## Summary

Three defects account for nearly all of the noise, and none of them needs
snapshot metadata to fix:

1. **The emitter duplicates and drops control flow at the same time.** Half of
   every emitted function is literally repeated text, while 55% of the machine
   calls never appear at all. Both are the same bug: `emit_block` expands a DAG
   into a tree.
2. **Dart AOT's runtime bookkeeping is emitted as program logic.** One
   three-instruction guard, present in 82.7% of functions, fabricates a
   conditional, a call and a back edge each time it appears. Removing it deletes
   a third of all emitted text.
3. **Method names are invented from unrelated strings.** The tool named calls
   after Finnish weekdays and Icelandic UI labels because it treats a pool
   string in an argument register as the callee's name. Dart's calling
   convention makes that impossible in principle.

Against this, the binary hands us a precise, unused semantic signal: **48% of
indirect calls are dispatch-table calls whose selector identity is encoded in
the instruction stream.** Recovering it is exact, needs no metadata, and is the
natural join point for the snapshot work.

Prototype in this branch implements (2) and (3) and the dispatch-table recovery.
(1) is specified below but deliberately not attempted, see
[Recommendations](#recommendations).

## Method

All figures come from one real target, measured end to end:

| | |
|---|---|
| target | LocalSend 1.17.0, `lib/arm64-v8a/libapp.so` (14 MB) |
| snapshot hash | `80a49c7111088100a233b2ae788e1f48`, Dart 3.5.0 |
| backend | internal (`dynamic_snapshot_string_model_v1`), no blutter, no r2flutter |
| function records | 5,800 |
| decoded instructions | 1,720,627 |
| machine calls (`bl` + `blr`) | 157,462 |

```bash
flutterdec adapter install --dart-hash 80a49c7111088100a233b2ae788e1f48
flutterdec decompile LocalSend-1.17.0-android-arm64v8.apk -o out \
  --emit-asm --emit-ir --function-scope all \
  --adapter-backend internal \
  --max-placeholder-ifs 999999 --max-unresolved-cf 999999 \
  --max-indirect-call-ratio 1.0 --min-disassembly-ratio 0.0
```

`--adapter-backend internal` is load-bearing rather than decorative. The flag defaults to
`auto`, which tries r2flutter first, then blutter, then internal, so the backend named in
the table above is selected here only because neither alternative is on `PATH`. Reproducing
this recipe on a machine that has one installed would silently measure a different
substrate: the blutter table yields 39,343 function records against internal's 5,800 and
drops `shared_stub_naming` from 14 names to 0, because it does not cover the runtime stub
range, so there is no prologue to read. Every figure in this document is valid under
`internal` only.

Figures are from the merged branch tip, rebuilt in a clean clone. ABI claims are
checked against the Dart SDK source, never inferred from output alone. Every constant used here was re-verified against tag `3.11.0`
(`runtime/vm/constants_arm64.h`, `dispatch_table.h`, `raw_object.h`,
`stack_frame_arm64.h`, `compiler/runtime_offsets_extracted.h`,
`compiler/backend/flow_graph_compiler_arm64.cc`,
`compiler/backend/dart_calling_conventions.cc`). The register assignments, the
dispatch-table codegen and `kOriginElement` are unchanged from 3.9.

One constant is not stable, and it matters. `Thread::stack_limit_offset` is
`0x38` up to Dart 3.5 and `0x48` by 3.11 (AOT, ARM64, compressed pointers);
`top` moves `0x50` to `0x60`, `end` `0x58` to `0x68`, `dispatch_table_array`
`0x60` to `0x70`. Any recogniser keyed on a literal `THR` displacement therefore
silently stops working on a version bump, which is why the guard recogniser below
matches on instruction shape and leaves the displacement free.

### Cross-version check

Every claim here was re-measured on a second, independent binary from a different
SDK generation. Versions are resolved from the vendored snapshot-hash table added
by #85.

| | sample A | sample B |
|---|---|---|
| app | LocalSend 1.17.0 | Immich 3.1.0 |
| snapshot hash | `80a49c7111088100a233b2ae788e1f48` | `ace654289f5abc240509fc941453ebc5` |
| Dart | 3.5.0 | 3.12.1 |
| function records | 5,800 | 8,329 |
| decoded instructions | 1,720,627 | 2,458,430 |
| `Thread::stack_limit_offset` | `0x38` | `0x48` |
| guard occurrences, shape variants | 20,941, one | 30,557, one |
| class-id `ubfx` (pos, width) | `(0xc, 0x14)` | `(0xc, 0x14)` |
| dispatch sites with a recovered offset | 10,244 | 16,353 |
| distinct selectors, negative offsets | 1,279, zero | 1,015, zero |
| receiver resolved | 63.5% | 68.8% |
| two-way branches, case analysis exhaustive | 40,154, yes | 49,709, yes |
| structured without fallback | 73.2% | 74.6% |
| out-of-scope value references | 0 | 0 |

The guard offset differs between the two samples and the recogniser is unaffected,
because it matches `ldr TMP, [THR, #any]` then `cmp SP, TMP` then `b.ls`, and that
shape is the only thing in either corpus that compares the Dart stack pointer
against a register. An offset-keyed matcher would have recovered nothing on sample
B. The class-id bitfield has not moved between these two releases, but recognition
does not depend on it either.

Zero negative selector offsets on two binaries built by different SDKs, 2,294
independently derived values in total, is the strongest available check on the
offset arithmetic without a ground-truth selector table.

### The counters in `quality.json` cannot measure this

Every counter that existed when this was written is a count of **emitted lines**
(`crates/flutterdec-core/src/pipeline/quality.rs:26-45`), so it cannot
distinguish *recovered more* from *duplicated more*. `semantic_direct_calls`
doubling is equally consistent with twice the recovery and twice the
duplication. Worse, `indirect_call_ratio` is a ratio, so inflating the
denominator turns the strict gate green on its own: the baseline run **fails**
the gate at 387 placeholder ifs, and a run emitting 76% more calls **passes** at
512.

Figures below therefore use two duplication-immune measures derived from the
`--emit-ir` artifacts:

- **emittable calls**, `Call` instructions in blocks reachable from the entry,
  counted once each, stopping at each block's first terminator (what the emitter
  can possibly print).
- **inflation**, emitted call statements / emittable calls. `1.0` is faithful;
  above is duplication, below is loss.

## Finding 1: the emitter expands a DAG into a tree

`emit_block` (`crates/flutterdec-decompiler/src/control_flow/emit.rs:603`) is a
recursive inliner. Each branch inlines *both* successors as nested blocks, and
saves/restores the register map around each arm so the two paths get independent
state. Since 33.1% of basic blocks in this binary are join points, every join is
re-emitted once per incoming path, bounded only by a visit budget and
`depth >= 12`. When the budget runs out the remaining code is not emitted, it is
replaced by `return _block_N();` and later collapsed to `return null;` with an
`// omitted complex paths:` comment.

Measured on the shipped baseline:

| | |
|---|---|
| exact-duplicate lines | **50.0%** of all emitted pseudocode |
| functions with >=3 duplicated statements | 3,778 / 5,800 (65.1%) |
| functions emitting **more** calls than the machine code contains | 1,998 (34.4%) |
| functions emitting **fewer** | 2,895 (49.9%) |
| emitted call statements | 69,100 of 151,988 emittable, **inflation 0.45** |

Both failure modes at once, in half the corpus each. This is not a tuning
problem: a tree expansion of a DAG has no visit budget that is both faithful and
finite.

A second, independent bug in the same function: the arms only recurse on
`Branch`, `Jump` and `Return`, so a block ending in a non-terminator, which
happens whenever the next instruction is a branch target, silently drops the
rest of the function. That alone explains most of the 55% call loss.

Adding the missing fall-through recursion confirms the diagnosis by making it
worse in the opposite direction: retention rises from 45% to 81% of machine
calls, and inflation goes from 0.45 to **4.37**. Recovering the dropped code
without single-emission structuring just routes more blocks through the
duplicating expansion.

### The CFGs are structurable

This is worth stating because it bounds the work. After Finding 2's elision:

| | |
|---|---|
| functions with a loop | 911 / 5,799 (15.7%) |
| **irreducible** functions | 247 (**4.26%**) |
| median blocks per function | 15 (p95: 122) |
| median join blocks per function | 4 (p95: 41) |

95.7% of functions are reducible, and Dart has labelled `break`/`continue`,
which is exactly the escape hatch goto-free structuring needs for multi-level
exits. Dart has no `goto` at all, so goto-free output is a hard language
constraint here, not a stylistic preference.

## Finding 2: a runtime guard is emitted as program logic

Dart AOT emits a stack-overflow check before the body of any function that can
loop or recurse:

```text
ldr x16, [x26, #0x38]   ; TMP = THR->stack_limit_   (x26 = THR, offset is version-dependent)
cmp x15, x16            ; Dart SP (x15) vs the limit
b.ls <slow>             ; slow path: call StackOverflowStub, then jump back in
```

The taken edge is a guard, and its slow path re-enters the body, so leaving it
in the CFG manufactures a conditional, a call and a **backward edge that is not
a loop**, precisely what a naive structurer turns into a `while`.

The pattern is not a heuristic. Across 1.72 M instructions, **every** `cmp x15, xN`
in the corpus is this check: 20,941 occurrences, one encoding, no variants, so
there is no false-positive risk.

| | |
|---|---|
| occurrences | 20,941 |
| functions containing >=1 | 4,796 / 5,800 (**82.7%**) |
| functions containing >=6 | 958 (one per inlined callee) |

Removing the guard edge, measured on the recovered CFGs:

| | before | after | |
|---|---|---|---|
| join blocks | 64,352 | 34,175 | **-47%** |
| irreducible functions | 706 (12.17%) | 247 (4.26%) | **-65%** |

Nearly half of all join points and two thirds of all irreducibility in this
binary are artefacts of one unrecognised guard.

Worked example, `sub_606d3c`. Baseline emitted the entire body **twice**, 
once under a fabricated `if (sp[-0x18] <= thread.f56)` (the guard, with `x26`
already cosmetically named `thread`), then again as the fall-through:

```dart
if (sp[-0x18] <= thread.f56) {
  final t1 = sub_d51c4c(obj2, obj1, obj2, obj1);   // the StackOverflowStub
  if (obj1.f15 != 0) { ... 20 lines ... }
  return t1;
}
if (obj1.f15 != 0) { ... the same 20 lines again ... }
return obj2;
```

With the guard elided, the body is emitted once and the fabricated call is gone.

### Ablation

The elision requires two supporting fixes (`ret` and `b` must end a basic block;
`emit_block` must follow fall-through), so it is measured against a baseline that
already has them. `A` = shipped, `B` = A + terminator and fall-through fixes,
`C` = B + elision.

| | A | B | C | C - B |
|---|---|---|---|---|
| pseudocode lines | 368,327 | 948,938 | 628,929 | **-33.7%** |
| emitted call statements | 69,100 | 205,665 | 127,508 | **-38.0%** |
| inflation | 0.45 | 4.37 | 3.03 | -1.34 |
| duplicate-line fraction | 50.0% | 71.0% | 67.2% | -3.8 pp |
| `placeholder_ifs` | 387 | 770 | 512 | -33.5% |
| `omitted_path_markers` | 503 | 1,223 | 1,026 | -16.1% |

A third of all emitted text and 38% of all emitted call statements were the
guard. Residual inflation of 3.03 is Finding 1, untouched.

Line counts here are logical lines. `wc -l` reports 624,493 for the same
output because 4,436 of the 5,800 files end without a trailing newline.

## Finding 3: dispatch-table selectors are recoverable exactly

`FlowGraphCompiler::EmitDispatchTableCall` (ARM64) emits:

```text
ldur wC, [recv, #-1]          ; object header (receiver is tag-adjusted)
ubfx xC, xC, #0xc, #0x14      ; class id = header bits 12..31
add  x30, xC, #K              ; K = selector_offset - DispatchTable::kOriginElement
ldr  x30, [x21, x30, lsl #3]  ; x21 = DISPATCH_TABLE_REG
blr  x30
```

`kOriginElement` is 4096 on ARM64 (`runtime/vm/dispatch_table.h`), so

> **selector_offset = K_signed + 4096**

is the *canonical* integer the Dart compiler assigned the selector, the same key
a recovered selector table is indexed by. `AddImmediate` materialises `K` four
ways, a bare `mov` when it is zero, a 12-bit `sub`/`add`, a shifted `#k, lsl #12`
for multiples of 4096, and `movz`(+`movk`)+`add` for wide values. All four must be
folded or the recovered ids are wrong while still clustering, which is the worst
case, since clustering makes them look right.

Recovery over the whole decoded body, tracking constants and class ids with
kill-on-definition so a stale register cannot fabricate a selector:

| | |
|---|---|
| dispatch-table call sites (idiom present) | 11,536 (**48%** of 23,998 `blr`) |
| functions containing one | 2,213 (38.2%) |
| selector offset statically recovered | 10,244 (**88.8%**) |
| distinct selectors | 1,279, range 0..109,443, **zero negative** |
| receiver register recovered | 63.5% |

Zero negative offsets across 1,279 independently derived values is the
correctness check: a wrong origin constant, a dropped `movk` half or a
misdecoded `lsl #12` would produce them immediately.

The 11.2% residual is dominated by the idiom straddling a basic-block boundary,
since the prototype resets its tracking per block rather than carrying state
across a join.

Prototype output, before and after, same call site:

```dart
// before, name taken from whatever string was in an argument register
final t1 = dispatch.torstai(null, null, (objTmp1.f7), poolStr8768);
//         ^ "Thursday", Finnish

// after, recovered selector, recovered receiver, argument list not overclaimed
final t2 = objTmp1.sel1814(t1); // dispatch table, selector_offset: 1814, args: lower bound
final t3 = sel25768(); // dispatch table, selector_offset: 25768, args: unknown, receiver: unrecovered
```

Measured in the reachable CFG, where duplication cannot inflate the count:

| | |
|---|---|
| distinct reachable indirect call sites (`blr`) | 6,307 |
| of which dispatch-table calls | 4,168 (66.1%) |
| **named** | 3,767 (**90.4%** of those visible) |
| distinct selectors, reachable | 643 |
| distinct selectors in emitted output | 544 |
| emitted dispatch call statements | 7,721, inflation **2.05x** (Finding 1) |
| dispatch sites the IR covers, reachable vs total | 3,767 / 10,133, only **37.2%** reachable |

`sel<N>` is a placeholder, but a *sound* one: identical at every call site of the
same method binary-wide, so it is greppable now and becomes a pure rename when
the snapshot work lands a selector table. Nothing in the lifter changes then.

`quality.json` reports these separately from metadata-named calls, as
`dispatch_table_calls` rather than `dispatch_selector_calls`, because the two have
different warrants: one is provable from the instruction stream, the other depends
on pool metadata being both present and correct. On the sampled binary with the
internal backend the split is 6,832 and 0, which is the honest picture: no
selector name here comes from metadata.

## Finding 4: invented names, and why they cannot be right

Observed in shipped output on a real APK:

```dart
final t1 = dispatch.torstai(null, null, (objTmp1.f7), poolStr8768);
final t2 = dispatch.jz(intTmp1, null, null, "y. MMM d., EEE" /* pool[2328] */);
final t1 = LoadingUnit.new(tmp7, null, (objTmp1.f23.f15.f7), "Til baka" /* pool[6464] */);
final t2 = AQaqaq.new(0xa0, ..., tmp1, ...);   // "constructor-like selector"
```

`torstai` is Thursday in Finnish, `Til baka` is Icelandic, `Sejarah` (also
observed) is Indonesian. These are localisation strings from the object pool
being used as method names, because
`infer_selector_name_from_context` accepted any identifier-shaped pool string
reachable from **x0-x3** as the selector, and
`looks_constructor_like_selector` then promoted capitalised ones to `X.new(...)`.

This cannot be correct, and the reason is structural rather than statistical.
Dart AOT ARM64:

- returns in **x0** and passes arguments from **x1** upward, so x0-x3 are the
  *previous call's result plus arguments*, never a selector;
- for a switchable call, puts the selector-bearing `ICData`/`MegamorphicCache` in
  **x5** (`IC_DATA_REG`) and the callee's `Code` in a separate pool slot:
```text
  mov  x1, #2                ; argument count
  ldr  x5, [x27, #0x2250]    ; pool -> ICData / MegamorphicCache
  ldr  x30, [x27, #0x2258]   ; pool -> Code
  ldur x30, [x30, #7]        ; Code::entry_point
  blr  x30
  ```
  (136+ functions in this binary);
- for a dispatch-table call, encodes the selector as the offset immediate of
  Finding 3.

In every construction the selector lives somewhere the old heuristic did not
look. It was reading argument values as method names. 1,636 call sites in this
binary got such a name.

The prototype keeps the inference but demotes it from an identifier to
`// selector candidate, unverified: <name>`, so the evidence survives and the
false claim does not. Metadata-derived selectors are unaffected and still name
calls.

## Finding 5: the internal adapter merges functions, which caps everything

Independent of the decompiler, and it bounds every figure above.

| | |
|---|---|
| adapter function records | 5,800 |
| frame prologues (`stp x29, x30, [x15, #-0x10]!`) inside them | **24,880** |
| records containing >1 prologue | 3,158 (**54.4%**) |
| `ret` instructions | 39,159 |
| instructions reachable from the declared entry | **~30%** |

`_recover_functions` scans for prologues but emits one record per *region*, so a
record averages 4.3 real functions. `sub_607700` is a three-instruction trampoline
whose record spans 307 instructions, four prologues and five returns.

Consequences: the inner functions are only entered through external calls, so
they are genuinely unreachable from the declared entry and cannot be emitted;
`app_package_counts_top` reports a single scraped package (`markdown` for this
binary); every name is `sub_<addr>`, so only 1.7% of direct calls carry any
semantic name. This is exactly what the snapshot work in #42 fixes, and it is why
this report's recoveries are specified to *join* onto that work rather than
duplicate it.

Also worth recording: `target_va_symbol_calls` is **0**. The `map-symbols`
ingestion path contributes nothing on a stripped release APK, which is the entire
target population.

## Cosmetic rewrites standing in for semantic modelling

Consequences of the pipeline stringifying capstone's structured operands and
re-lexing them downstream (`helpers/instruction_parse.rs`), which destroys
register class, shift kind and amount, condition codes and flag effects:

- `passes/expr_cleanup.rs:279` deletes `" + x28 /* lsl #32 */"` **as text**. That
  is compressed-pointer decompression (`x28` = `HEAP_BITS`), and it is one of the
  strongest type oracles in the binary: it proves the 32-bit slot just loaded is
  an object reference rather than a Smi or an unboxed field. 65,759 occurrences,
  all discarded. On any modern Flutter build compressed pointers are the default,
  so every field load is a 32-bit `ldur w`, which is also why `canonical_reg`
  folding `w0` into `x0` is not a cosmetic loss.
- `passes/naming.rs:322` names any repeated `(value_N - 1)` **`codePoint`**. In
  Dart AOT ARM64 `- 1` is almost always `kHeapObjectTag` removal, essentially
  never a Unicode code point. Same failure class as Finding 4: a fabricated name
  is worse than none.
- `expr_cleanup.rs:142` already rewrites the class-id extraction to
  `classId(obj)`, recognised, then never *used*. 18,816 extractions, 1,545 class-id
  equality checks and 1,227 interval checks (`ubfx; sub; cmp #k`, i.e. Dart's
  `obj is T` over a cid range) are all available and none becomes an `is` test.
- Field offsets rendered as raw tagged displacements (`obj.f7`, `obj.f15`), one
  less than the offset any field table is keyed by, because object pointers carry
  `kHeapObjectTag`. **Fixed.** The displacement identifies itself: field offsets
  are 4-aligned, so 3 mod 4 is exactly a tag-adjusted one, and untagged bases
  cannot match. Measured on sample B, 262,439 of 272,805 object-base
  displacements are 3 or 7 mod 8, while every `THR` displacement is 0 mod 8.
- Register-offset operands, which is how Dart reaches list and array elements,
  were folded to displacement zero by `parse_mem_operand` and rendered `base.f0`,
  indistinguishable from a real field-zero read. 2,094 sites on sample B once
  dispatch-table loads and frame accesses are excluded. **Fixed**, they render as
  `base[index]`.
- `x21` is `DISPATCH_TABLE_REG` and was the one reserved register `init_state` did
  not name, so the dispatch calls whose selector offset is not statically
  recoverable rendered as `reg21[reg0](...)`. **Fixed**, they read
  `dispatchTable[cid](...)`, which states what the call is. That also made the
  `dispatchTargetFn` alias pass redundant, so it is gone.
- `(x >> 0x3f) & 1) != 0` is a sign test (`tbnz xN, #0x3f`), i.e. `x < 0`.
- `cond_from_cmp` handles 10 conditions and only from `cmp`; `tst`, `cmn`,
  `ands`, `subs`, `ccmp` and `fcmp` set no condition, which is where the
  `/* cond */` placeholders come from.
- Signatures are always eight `dynamic` parameters and calls always four
  arguments. `emit_call` reads x0-x3, which is the wrong set on both ends.
  `DartCallingConvention::kCpuRegistersForArgs` for ARM64 is
  `{R1, R2, R3, R5, R6, R7}` (`runtime/vm/constants_arm64.h:645`): x0 is the
  return register, and **x4 is skipped** because it is `ARGS_DESC_REG`. So the
  emitter passes the previous call's return value as argument 0, drops x5 to x7
  entirely, and cannot represent more than four arguments at all.
  Overflow arguments go on the stack, and `ComputeCallingConvention`
  (`compiler/backend/dart_calling_conventions.cc`) assigns them by counting
  **down** from the last argument starting at `param_end_from_fp + 1`, which is
  slot 2 on ARM64 (`kParamEndSlotFromFp = 1`, `stack_frame_arm64.h`), i.e.
  `[x29, #0x10]`. The last argument therefore sits at the **lowest** offset.
  Measured against that register set: 72.4% of functions take register
  arguments, mode 1, max 6; **33.6% also take stack arguments**, up to 26 slots.
  An arity fix that handles only the register half emits reversed argument lists
  for those 1,946 functions, which is worse than the current filler because it
  looks plausible.

## What the binary offers for free

Counts from this target, all recoverable with no snapshot metadata:

| signal | occurrences | recovers |
|---|---|---|
| dispatch-table call | 11,536 | selector identity, receiver |
| class-id extraction | 18,816 | receiver type site |
| compressed-pointer decompress | 65,759 | field is an object reference |
| object header load | 71,808 | type test / dispatch site |
| `NULL_REG` compare (`cmp wN, w22`) | 20,597 | `== null` |
| `NULL_REG` assign | 28,779 | `= null` |
| stack-overflow guard | 20,941 | *delete* |
| Smi tag test (`tbz/tbnz #0`) | 13,188 | `is int` fast path |
| Smi untag (`sbfx ..., #1`) | 12,314 | integer value, not a shift |
| sign test (`tbz/tbnz #0x3f`) | 845 | `x < 0` |
| switchable call (`IC_DATA_REG` in x5) | 136+ functions | selector identity |

`THR` field accesses resolve against the SDK's own offset table
(`runtime_offsets_extracted.h`, AOT/ARM64/compressed): `0x38` `stack_limit`
(20,941), `0x50` `top` and `0x58` `end` (inline allocation), `0x150`
`allocate_object_stub`, `0x190`/`0x198` `stack_overflow_shared`, `0x200`
`write_barrier_entry`, `0x60` `dispatch_table_array`. Each names a runtime
operation the pseudocode currently prints as `thread.fN`.

## Recommendations

Ordered by measured payoff per unit of risk.

**R1, Single-emission structuring. Implemented, partially.** Emission is now
driven by the dominator and post-dominator trees rather than by inlining
successors. Every conditional in this binary falls into one of three shapes the
follow-node rule covers, with no fourth case:

| branch shape | count | share | structure |
|---|---|---|---|
| immediate post-dominator is one of the successors | 16,407 | 40.9% | `if`, then the continuation |
| no post-dominator, the arms never rejoin | 12,416 | 30.9% | `if/else`, no continuation |
| immediate post-dominator is a join block | 11,331 | 28.2% | `if/else`, then the follow node once |

The three are exhaustive by construction and sum to 40,154. An earlier draft of
this table reported 56.2 / 30.9 / 12.9, because both the analysis and the first
implementation selected the immediate post-dominator with the smallest
post-dominator set, which is the *farthest* strict post-dominator rather than the
nearest. That systematically attributed if-then branches to the if-then-else row.
The nearest post-dominator is the one with the largest post-dominator set.

Loops, over the 830 functions that have one. That is fewer than the 911 counted
in Finding 2, and 250 irreducible rather than 247, because the two are different
corpora: Finding 2 analyses the CFGs an external rebuild derives from the
disassembly, this analyses the CFGs the tool itself emits, where the
runtime-check elision and the reachability rules have already applied.

| | count | needs |
|---|---|---|
| loop nests with a single exit block | 500 | `while` plus `break` |
| loop nests with more than one exit block | 805 | `break`, labelled where nested |
| functions with more than one loop head | 272 | labelled `break`/`continue` |
| loops with more than one latch | 15 | latch merge or duplication |
| irreducible functions | 250 (4.3%) | declined, DFS fallback |

Two findings came out of implementing it.

Dart AOT shares one non-returning slow path per function for null checks, bounds
checks and type checks: a handful of instructions ending in a throw or deopt stub,
with many predecessors and no successors. It post-dominates nothing, so it is
never a follow node, and refusing to repeat it forced **84% of the structuring
fallbacks**. Small terminal blocks are therefore allowed to repeat, which also
reads better: Dart cannot express a shared tail without hoisting it into a label.

A block emitted once cannot carry a per-path register state, and getting that
wrong is silent. The first cut let a value defined in an arm that returns be
referenced afterwards: **1,055 out-of-scope temporary references** across the
corpus, where both the baseline and the pre-structuring branch had zero. Each arm
now starts from the state at its branch, and at a merge a binding survives only
if no path into it redefines that register. Verified by scanning every emitted
function for a temporary referenced before any declaration: zero.

Result, against this branch before the change:

| | before R1 | with R1 |
|---|---|---|
| pseudocode lines | 628,929 | **396,581** |
| emitted call statements | 109,729 | **83,245** |
| inflation | 3.03x | **2.28x** |
| duplicate-line fraction | 67.9% | 62.0% |
| `omitted_path_markers` | 1,026 | 804 |
| `loop_backedge_markers` | 448 | 296 |
| functions where emitted equals emittable | 45.0% | **72.8%** |
| out-of-scope temporary references | 0 | 0 |
| unresolved register references | 2,093 | 17,548 |

The last row is the cost, and it is the honest kind: a value that genuinely
differs per path is now named as an unresolved register rather than given one
path's expression. The old golden for `goldenSimpleLoop` asserted
`return (receiver + 1);` inside a loop that increments `receiver`, which was
wrong on every iteration after the first.

### What forced the remaining fallbacks

Measured rather than assumed, by replaying the region walk over every recovered
CFG and recording the first edge it could not place:

| cause | LocalSend | share |
|---|---|---|
| structured | 4,244 | 73.2% |
| shared continuation that is not the follow node | 1,236 | 21.3% |
| irreducible | 250 | 4.3% |
| back edge or exit to a non-innermost loop | 70 | 1.2% |

Labelled `break`/`continue`, the obvious next feature, would have addressed 1.2%.
The real cause is a shared continuation that the follow-node rule cannot place,
and Dart has no `goto`, so such a block cannot be named at all. The choices are
to repeat it, hoist it into a helper, or give up on structuring the function.
Giving up means the DFS emitter, whose duplication is unbounded, so repeating a
bounded region is strictly the smaller cost. A region containing a loop is never
repeated.

The budget comes from the measured distribution. Sweeping it:

| blocks | instructions | structured | duplication inside structured functions |
|---|---|---|---|
| 1 | 16 | 74.8% | 1.02x |
| 4 | 24 | 80.5% | 1.04x |
| 8 | 48 | **85.0%** | **1.09x** |
| 16 | 96 | 88.7% | 1.20x |
| 32 | 256 | 91.7% | 1.45x |

8 blocks and 48 instructions is the knee. `quality.json` reports
`repeated_blocks` so budgeted repetition stays visible rather than being absorbed
into the structuring rate.

Structuring also made empty `if` bodies common: when both successors are the
join, neither arm emits a statement. 4,117 of them on sample A against 321 on
`main`. Arms are now rendered into buffers and emptiness is decided on emitted
content, which drops the branch or inverts the condition. 4,117 to 234.

That change surfaced a defect worth its own note, because it is the same class
of silent loss the targeted prune in `build_function_ir` exists to avoid.
"Emitted nothing" does not mean "does nothing": `apply_other_lift` discards any
mnemonic it does not model, with no statement, no state change and no counter,
so floating point, vector work and load/store pairs vanish. Eliding those arms
deleted real computation invisibly. An empty arm carrying unmodelled work now
reports how many instructions it holds, and `quality.json` reports
`unlifted_instructions`: 1,430 across 1,292 sites on sample A and 1,535 across
1,363 on sample B, every one of which the ungated version dropped.

That count is also the first measurement of the lifting gap itself, which is R3
in this list, and it is small: about 0.08% of decoded instructions on either
sample sit on a branch arm the lifter cannot express.

### Negative result: naive phi materialisation

The one measure that moved the wrong way under structuring is unresolved register
references, because a value that genuinely differs per path is dropped rather than
given one path's expression. The obvious fix is to name it: assign it to a merge
variable at the end of each arm and read that at the join.

Implemented and reverted. Lines went from 400,073 to 520,559 and unresolved
register references rose from 18,589 to 31,184 rather than falling, because 30,736
of 40,186 generated variables were never read and the assignments themselves
reference registers that are still unbound. The read set came from an
over-approximating scan of up to 64 successor blocks, which marks almost
everything live.

The lesson is specific: this needs real liveness at the join, and candidates
ranked by how often the continuation reads them, not a reachability scan. Sorting
candidate registers by name also silently spends the budget on the wrong ones,
since `x10` sorts before `x2` and the Dart argument registers are x1, x2, x3, x5,
x6, x7.

### Remaining, in order

Real liveness for the merge variables above; partial structuring, where an edge
the region tree cannot describe becomes a marked, singly-emitted tail rather than
discarding the whole function's structure; then labelled `break`/`continue` for
the 1.2%. The first two would let the DFS emitter be deleted rather than kept.

Rendering the header test as `while (cond)` rather than `while (true)` plus
`break` is only sound when the header writes no register the condition reads,
which excludes the common increment-in-header shape, so it is not a general
simplification.

Prior art, in order of fit: Yakdan et al., *No More Gotos* (NDSS 2015) for
provably goto-free structuring; Relooper (Emscripten) for a simpler
node-splitting baseline; Muchnick's interval analysis for the classic hammock
formulation; LLVM's `WebAssemblyCFGStackify` for a production implementation.

**R2, Structured operands from the disassembler, once.**
`build_capstone` sets `detail(false)`, so capstone's structured operands are
thrown away and re-lexed from text downstream. Every remaining recommendation
needs shift kind, register class and condition codes, which is exactly what the
round-trip destroys. Emit them once; delete `helpers/instruction_parse.rs`.

**R3, Recognise the rest of the ABI.** With R2 in place, each item is a small
pattern over typed operands: THR offsets to named runtime operations, the inline
allocation sequence to `new`, Smi tag algebra to integer values, the sign test to
`< 0`, class-id compares and cid intervals to `is` tests, and the decompression
sequence kept as a type fact rather than deleted as text.

**R4, Real arity, both conventions.** Backward liveness on x1-x7 at entry
**and** the maximum positive frame offset, un-reversing the stack case. Validate
by consensus: every call site of one selector must agree. A first attempt with a
naive "registers written before the call" proxy achieved only 41% unanimity
across 671 multi-site selectors, which is evidence the naive version is not good
enough rather than evidence the check is wrong.

**R5, Switchable-call recovery.** Second zero-metadata call family: the pool
index loaded into `IC_DATA_REG` is as stable a selector identity as the
dispatch-table offset, and the `ldur x30, [x30, #7]` + `blr x30` shape is
unambiguous. Same join mechanism as Finding 3.

**R6, Metrics that can fail.** Add `semantic_direct_call_ratio` (1.7% today),
distinct-VA call retention, emitted-lines-per-source-VA (duplication), and a
count of instructions unreachable from the declared entry. The strict gate
currently keys only on placeholder counts, which are an order of magnitude
smaller than the naming and fidelity gaps, and being ratio-based it can be turned
green by emitting more text.

## Relationship to #42 / the serwalker work

Complementary, not overlapping. Everything here is a property of the instruction
stream; #42 recovers the object graph. The seam is deliberate:

| this report recovers | #42 supplies | result |
|---|---|---|
| `selector_offset` (canonical) | selector  ->  name | `recv.getInstance(...)` |
| class id at a dispatch site | cid  ->  class | receiver type, `is T` |
| field byte offset  ->  word slot | class field table | `obj.controller` |
| argument descriptor pool slot | descriptor contents | exact arity and named arguments |

Each is an integer key joined by equality. When the table lands, `sel1814`
becomes a real name with no change to the lifter. Finding 5 quantifies the
ceiling until then: of the 10,133 dispatch sites the IR covers, only 3,767
(37.2%) are reachable from a declared entry, so **62.8% are lost**; of the 1,279
distinct selectors in the binary, 643 are reachable and 528 reach the output, so
roughly **half the method vocabulary is invisible**, purely because of adapter
function boundaries, not because of anything in the lifter.

## Prototype in this branch

Landed, `cargo test --workspace` green, `fmt` and `clippy` clean:

- `flutterdec-ir`: `IROp::RuntimeCheck`; the Dart stack-overflow guard is
  recognised by instruction shape, with the `THR` displacement left free so a
  version bump cannot silently disable it, and contributes no CFG edge. Its
  stranded slow path is pruned, targeted rather than by blanket reachability,
  which would delete the merged functions of Finding 5. `ret` and `b` end a
  basic block.
- `flutterdec-decompiler`: `helpers/dispatch_table.rs` recovers canonical
  selector offsets from all four immediate encodings, with kill-on-definition on
  both keys and receiver names so a stale register cannot fabricate a selector or
  a receiver. `emit_call` prefers it over every heuristic. Arguments are the
  registers of `DartCallingConvention::kCpuRegistersForArgs` with a definition
  reaching the call, labelled `args: lower bound`, or `args: unknown` when none,
  so an empty list is never read as a recovered zero-arity method.
  Pool-string selector guesses are demoted from identifiers to
  `selector candidate, unverified` comments and still propagate through spills
  and field round-trips, so the evidence survives without the false claim.
- `flutterdec-decompiler`: `control_flow/regions.rs` computes dominators,
  post-dominators, follow nodes and natural loops on the reachable CFG;
  `control_flow/structured.rs` walks that structure and emits every block once,
  with per-arm state isolation, bounded repetition of small shared slow paths, a
  checked emit-once invariant, and the DFS emitter retained as the fallback for
  irreducible functions.

Emitted, on the sampled binary: 7,721 dispatch call statements over 544
distinct selectors, zero negative offsets, receiver resolved on two-thirds, arity
reported as a lower bound on 70.0% and honestly unknown on the rest. Structuring
holds emit-once on 72.8% of functions, takes inflation from 3.03x to 2.28x and
emitted lines down 23%, and removes 1,055 references to values defined in a branch
that returned, verified as zero across all 5,800 functions.

Not attempted: R2 to R6, and the two remaining structuring steps (labelled
`break`/`continue`, then partial structuring) that would let the DFS emitter be
deleted rather than kept as a fallback.

## Sources

Dart SDK, tag `3.11.0` (BSD-3-Clause), all paths under `runtime/vm`:

- `constants_arm64.h`: register assignments (`THR` R26, `PP` R27, `NULL_REG` R22,
  `HEAP_BITS` R28, `DISPATCH_TABLE_REG` R21, `SPREG` R15, `IC_DATA_REG` R5,
  `ARGS_DESC_REG` R4), and `DartCallingConvention::kCpuRegistersForArgs`.
- `dispatch_table.h`: `kOriginElement`, `kLargestSmallOffset`.
- `raw_object.h`: `ClassIdTag` bitfield, header layout.
- `stack_frame_arm64.h`: `kParamEndSlotFromFp`.
- `compiler/runtime_offsets_extracted.h`: `Thread` field offsets per
  arch/mode/pointer-width, which is how the version drift in
  `stack_limit_offset` was established.
- `compiler/backend/flow_graph_compiler_arm64.cc`:
  `FlowGraphCompiler::EmitDispatchTableCall`.
- `compiler/backend/dart_calling_conventions.cc`: `ComputeCallingConvention`,
  including the reverse stack-argument assignment.
- `compiler/aot/dispatch_table_generator.h`: `SelectorMap`, `TableSelector`,
  which is what a recovered selector table would be keyed by.

Structuring prior art:

- Yakdan, Eschweiler, Gerhards-Padilla, Smith, *No More Gotos: Decompilation
  Using Pattern-Independent Control-Flow Structuring and Semantics-Preserving
  Transformations*, NDSS 2015.
- Zakai, *Emscripten: an LLVM-to-JavaScript compiler*, OOPSLA 2011 (Relooper).
- Muchnick, *Advanced Compiler Design and Implementation*, chapter 7
  (interval and structural analysis).
- LLVM `llvm/lib/Target/WebAssembly/WebAssemblyCFGStackify.cpp`.
- Cifuentes, *Reverse Compilation Techniques*, 1994, chapter 6.

Existing Flutter AOT tooling consulted for comparison:

- `worawit/blutter`: snapshot-driven recovery of classes, functions and the
  object pool.
- `radareorg/r2flutter`: object pool reconstruction and the snapshot-hash to
  SDK-version table vendored by #85.

Sample: LocalSend 1.17.0 `android-arm64v8` release APK
(`github.com/localsend/localsend`, AGPL-3.0), used read-only as a measurement
target.

## R7. The lifter was asserting values it did not have

Everything above concerns structure. This concerns the expressions inside it,
and it is the largest single source of wrong output found so far.

`apply_other_lift` matched a list of sixteen mnemonics and ended in `_ => {}`.
An unmodelled instruction wrote its destination register, but the emitter's
`reg_values` still held whatever the last modelled instruction had put there, so
every later read rendered the stale value as that register's value. There was no
counter, no marker, and nothing in the output to distinguish it from a recovered
expression.

The instruction that made this visible is `csel`. Dart materialises a comparison
into a value by loading both canonical bools and selecting between them:

```text
add  x16, x22, #0x20      ; kTrueOffsetFromNull
add  x17, x22, #0x30      ; kFalseOffsetFromNull
csel x0,  x16, x17, ne
ret
```

`csel` was unmodelled, so x0 kept its entry value and the function emitted
`return receiver;`. The truth is `return (a != b);`. 174 and 270 functions on the
two samples return a value produced this way.

Scale of the class: 459,250 instructions across both binaries write a register
and reach the fallback. Ranked, the top are `ldp` (79,645), `ldurb` (42,542),
`sbfx` (25,899), `movk` (21,649), `sbfiz` (8,262) and the conditional-select
family (2,993).

Invalidating unmodelled destinations changes 28,064 emitted lines on LocalSend,
7.08% of 396,381, across 1,402 of 5,800 files; 30,907 on Immich across 1,849.
Those are changed lines, positionally diffed. The register-reference counters
move much further, but invalidation changes expression *shape* as well as leaf
spelling, so that delta is a proxy with unknown multiplicity rather than a count
of wrong reads. One example, from a Smi overflow check:

```text
before   if (objTmp2.f8 == objTmp1)
after    if (objTmp2.f8 == signedBitFieldInsert(objTmp2.f8, 1, 0x1f))
```

`sbfiz x0, x2, #1, #0x1f` was unmodelled, so x0 held a value from two
instructions earlier and the comparison read as two unrelated fields.

### Flags were worse than registers

`last_cmp` held the operands of the most recent `cmp`, and was cleared only when
state merged at a join. Every `b.<cc>` rendered its condition from it. Nothing
else that writes NZCV touched it: `tst`, `cmn`, `ccmp`, `fcmp`, `adds`, `subs`,
`ands` and `bics` all set flags and all fell through to `_ => {}`.

So `tst x0, #1; b.ne L`, the Smi tag test and one of the most common shapes in
Dart AOT output, rendered as whatever the previous `cmp` compared. Measured over
247,555 condition consumers, 33,705 (13.6%) take their flags from something
other than `cmp`:

| flag source | conditions | share |
|---|---|---|
| `cmp` | 213,850 | 86.4% |
| `tst` | 22,141 | 8.9% |
| `fcmp` | 9,721 | 3.9% |
| none in block | 714 | 0.3% |
| `subs`, `ands`, `cmn`, `adds` | 1,129 | 0.5% |

Conditions are what the structurer branches on, so this fed the region analysis
as well as the text. `tst`, `cmn`, `ands`, `adds`, `subs` and `fcmp` are now
modelled, and any unmodelled flag writer clears `last_cmp` so the condition
degrades to `flags.b_<cc>`. `placeholder_ifs` therefore rises, 278 to 612 and
297 to 442: a plausible fabrication became an honest placeholder. `fcmp` carries
one imprecision worth naming, since everything else here is exact: a NaN operand
leaves the comparison unordered, so `b.gt` and `b.le` after it are not exact
negations.

### What the bools are

`kTrueOffsetFromNull` is `kObjectAlignment * 2` and `kFalseOffsetFromNull` is
`* 3`, so 0x20 and 0x30 on a 64-bit target (`runtime/vm/pointer_tagging.h`). An
add of either off NULL_REG materialises a canonical bool. 32,901 sites across
both samples, and every one of them uses one of those two offsets: zero other
displacements exist, so the `null + N` collapse that existed for the general
case was unreachable on real input. Only the two defined offsets are
intercepted; anything else stays plain arithmetic, which is a true statement
about a canonical object rather than a claim about an integer.

The same encoding gives a bool *test*: `kBoolValueBitPosition` is
`kObjectAlignmentLog2`, so bit 4, and `object.cc` asserts
`(true_ & kBoolValueMask) == 0`, `(false_ & kBoolValueMask) != 0` and
`false_ == (true_ | kBoolValueMask)`. `kObjectStartAlignment` is 64, with
`COMPILE_ASSERT(kObjectStartAlignment >= 2 * kBoolValueMask)`, so bits 4 and 5 of
null's address are structurally zero rather than incidentally so. Bit 4 clear is
`true`, set is `false`; bit 5 distinguishes bool from null and is a different
predicate.

That accounts for 9,337 and 15,988 bit-4 tests, and **it is deliberately not
folded**. `TestIntInstr::IsSupported(kTagged)` is true, and the tagged path does
`bit_index = min(kSmiBits, bit_index) + kSmiTagShift`, so a Dart-level test of
bit 3 on a tagged Smi lands on machine bit 4 as well. The two readings are
indistinguishable by bit index. Provenance does not settle it either: 54% of
tested registers are call returns, and a call can return a Smi. Rendering `if
(x)` where the truth is `if (x & 8)` is exactly the class of error this document
is about, so the raw shape stays. The same clamp means machine bit 63 is
ambiguous for the sign test, and lossy on top, since every Dart bit index at or
above 62 clamps there. Machine bit 0 is the one unambiguous case: the clamp
guarantees a tagged `TestInt` lands at bit 1 or higher, so bit 0 can only be the
Smi tag test.

### The argument registers were wrong on both ends

`DartCallingConvention::kCpuRegistersForArgs` is `{R1, R2, R3, R5, R6, R7}` and
`kReturnReg` is `R0` (`runtime/vm/constants_arm64.h`). x4 is `ARGS_DESC_REG`.
Four places independently encoded something else: the state seeding, the direct
call argument list, the emitted signature, and a naming pass that rewrote line 0
and so silently overrode the emitter.

The consequence is that x0, the return register, was emitted as argument 0 of
every direct call and named `receiver` in every signature. Confirmed without
reference to the SDK, on 14,129 functions, by asking which registers an entry
block reads before writing:

| register | functions | role |
|---|---|---|
| x1 | 55.0% | first argument |
| x2 | 29.5% | second |
| x3 | 9.5% | third |
| x4 | 10.3% | `ARGS_DESC_REG`, live-in but not an argument |
| x5 | 3.7% | fourth |
| x6 | 1.6% | fifth |
| x7 | 0.8% | sixth |
| x0 | 3.4% | return register |

The monotone decline through x1, x2, x3, x5, x6, x7 is the convention's order.
Prologues corroborate it: they spill x1 and x2 and never read x0.

Arity is now a lower bound rather than a fixed count, matching what
`DispatchCall::argument_registers` already reported. A trailing position whose
register still holds its entry seed was never written, and one that was
invalidated says nothing either, because x5-x7 are general scratch in
`kDartAvailableCpuRegs`. Over 325,376 direct calls, 44.7% define no argument
register at all and 96% define at most three. Emitted arity was a flat 4 for
88.2% of calls; it is now 0 for 22.9%, 1 for 12.7%, 2 for 17.4%, 3 for 31.1%.

`emit_call` separately fell back to `arg{r}` when a register had no binding,
which rendered as `receiver` or `paramN` and claimed the caller's value had
survived to the call site. It falls back to the register name now.

### Cost

Two numbers moved the wrong way for the right reason, and one is unexplained.
`placeholder_ifs` rises, as above. `poolOff` references fall 5.7% and 2.5%; the
loss is not attributable to a single mechanism, being spread across statements
that changed shape, and the unnormalised-pool-page count is zero, so no pool
reference renders wrongly. Widening the load arms did briefly produce a shape
the normaliser rejected, `((pool + page /* lsl #n */) + disp)).fN`, leaving 834
references as raw arithmetic; both displacements are known, so they now fold
into one entry.

### Still open

- The Smi untag idiom `sbfx rd, rs, #1, #0x1f` renders as
  `signedBitField(x, 1, 0x1f)`. Exact, but verbose at this frequency.
- 160 sp-relative references are renamed to `stackSlotN` without a declaration.
  Pre-existing, and reduced from 292 by this cycle.
- Bit-4 and bit-63 tests stay raw, for the reasons above. Resolving them needs a
  value-type model, not a better pattern.

## R8. Values do not survive a call, and these binaries are compressed

### The call boundary

`emit_call` bound x0 to the call's temporary and left every other volatile
register holding its pre-call value. `kDartVolatileCpuRegs`
(`runtime/vm/constants_arm64.h:556-560`) is R0 through R14; the assembler uses TMP
and TMP2 freely, and the call writes LR. R18 is the platform's scratch on some
targets and callee-saved on others, but that does not matter here: the VM lists it
in `kReservedCpuRegisters` unconditionally, with the comment that it is marked
reserved on every OS "to avoid adding another dimension for OS into the extracted
runtime offsets" (`constants_arm64.h:539-547`), so no Dart value ever occupies it.
Of the registers that can hold one, only R19 through R28 and SPREG survive a call.

The proof is not statistical. Instrumenting the base expression of every field
read found 2,497 reads whose base rendered as `null` on one sample, against
exactly **one** genuine load off NULL_REG in the entire binary. The shape is
`mov x0, x22`, then a call, then `[x0, #-1]`: a read of the object header off
null, which cannot happen. It rendered as `null._tag`, and the same mechanism
produced `null.f24 == thread.f160` and `if (null == null)`.

This matters more than an unresolved register would, because `regN` reads as a
gap and `null.f24` reads as a fact.

NZCV is caller-saved as well, so the comparison is dropped at a call for the
same reason an unmodelled flag writer drops it.

The cost is real and worth stating: `raw_register_name_refs` rises 37,240 to
55,735 and 41,991 to 66,199, and `smiUntag` occurrences fall by about 7,000 per
sample. Both are one effect seen twice: an expression is no longer inlined across
a call boundary. Inlining it there *was* the claim that the value survived.

### Compressed pointers, established rather than assumed

| evidence | LocalSend | Immich |
|---|---|---|
| `add rD, rS, x28, lsl #32` | 65,759 | 103,731 |
| 32-bit load displacements 3 or 7 mod 8 | 74,982 | 115,682 |
| `sbfx #1, #0x1f` | 12,314 | 13,585 |

x28 is HEAP_BITS, whose low half is `heap_base >> 32`, so that add reconstructs a
pointer and an uncompressed build emits none. The displacements are 4-byte-spaced
reference fields with the `kHeapObjectTag` adjustment; 87% of every 32-bit load
feeds a decompression. Flutter's `tools/gn` sets
`dart_use_compressed_pointers = True` for Android arm64, which is why, given the
Dart default is false. A first reading of the Thread offset tables suggested
Immich was uncompressed; the instruction stream settled it, and an offset that
appears in both the compressed and uncompressed blocks is not evidence either
way.

Decompression changes no Dart-level value, so it is transparent now. That removed
`+ (reg28 << 0x20)` from a quarter of all field reads.

### Reserved is not the same as invariant

`kReservedCpuRegisters` excludes x21, x22, x26, x27 and x28 from
`kDartAvailableCpuRegs`. HEAP_BITS was unseeded and read as `reg28` 5,334 and
7,012 times, always in the write-barrier idiom. It is pinned now, because AOT
re-derives it in-body, 639 instructions across 157 functions, restoring the same
constant from THR.

SPREG is reserved and deliberately **not** pinned. 47,412 instructions across
5,149 functions write it. Pinning rendered `[x15, #8]` after a frame allocation as
`sp[8]` when the address is `sp - frame + 8`, and collapsed three distinct frames
in one function onto one name. It keeps its merge exemption for an unrelated
reason: frames are balanced, so every path into a join leaves the same stack
pointer, and dropping the exemption cost 11,717 slot references for no
correctness gain. The two ideas are separate in the code now.

TMP, TMP2, LR and CODE_REG are reserved but hold no fixed value, and stay
unnamed. `CODE_REG` is explicitly "not passed in AOT".

### Named idioms

`SmiUntag` is `sbfm(dst, src, kSmiTagSize, kSmiBits + kSmiTagSize)`. The
disassembler prints that as the alias `sbfx rd, rs, #lsb, #width` with
`lsb = immr = kSmiTagSize` and `width = imms - immr + 1 = kSmiBits + 1`, so the
condition the lifter matches is exactly `lsb == 1 && width in {31, 63}`
(`expression_lift.rs:552-556`) -- 31 with compressed pointers, 63 without. Both are
accepted, so the rule encodes no build configuration, and any other operand pair
keeps the generic `signedBitField` name.

The rule is safe even where the compiler's intent was not an untag, because at
those operands the two are the same operation: a signed extract of the bits above
the tag bit, across the full remaining width, *is* an untag of a Smi-shaped value.
What would be unsafe is widening the match, since `sbfiz rd, rs, #l, #w`
sign-extends from bit `l + w` rather than from `w`, so an arithmetic rendering that
got that wrong would read as resolved. 25,899 sites read `smiUntag(x)` rather than
`signedBitField(x, 1, 0x1f)`. The insert form
at the same position is the tag, emitted by `BoxInteger32` and `BoxInt64`, which
is why all 8,262 sites are followed by the round-trip compare.

Measured operand pairs, from the IR of both samples:

| `ubfx`/`sbfx` pair | LocalSend | Immich | meaning |
|---|---|---|---|
| `ubfx #0xc, #0x14` | 18,816 | 31,953 | class id, position stable 3.5 to 3.12 |
| `sbfx #1, #0x1f` | 12,314 | 13,585 | Smi untag |
| `sbfiz #1, #0x1f` | 3,789 | 4,473 | Smi tag, all followed by the overflow compare |
| `ubfx #0, #0x20` | 3,679 | 2,610 | zero extension |
| `ubfx #8, #4` | 2 | 2 | size tag |

### The remaining leak, narrowed

165 and 277 `null.fN` references survive, with 701 and 1,611 bit tests on a
literal operand and 930 and 1,809 fully-constant `if` conditions. These are one
bug with three faces, and they are deliberately **not** constant-folded: Dart AOT
does not emit a bit test on a value it can fold, so a literal reaching one means
the binding is wrong, and folding would upgrade a false claim to a confident one.
They serve as canaries instead.

Two things are ruled out by experiment rather than argument. It is not a missing
merge: forcing `merge_state_at_join` at every block, not only at joins, moved the
count from 165 to 161. It is not the DFS fallback: the worst offender is
structured-emitted.

What does correlate is the omitted-path mechanism. Of 95 leaking functions on one
sample, 77.9% carry an `omitted complex paths` marker against 10.3% of the 5,705
clean ones, a 7.6-fold enrichment. That is the next place to look.

### Ready for the next cycle

Two tables were derived and verified but not yet used, both requiring a key on
(Dart version, pointer mode) because an offset table alone would mislabel one of
the two samples:

- **Thread offsets.** 8,930 and 22,216 direct THR loads. The top 18 data offsets
  cover 56.9% and 81.8%; the top 10 stub entry points cover 99.7%. Between 3.5
  and 3.12, 110 of 111 common offsets moved, and only 3 offsets agree between the
  compressed and uncompressed blocks, so version alone is not a sufficient key.
  Static AOT must read the `AOT_Thread_*` block, per `runtime_offsets_list.h`.
- **Fixed class field offsets.** Exact per-class tables exist and did not drift
  between 3.5 and 3.12 for the core classes. But naming a field needs the
  receiver's class, and no offset means the same thing across classes: `length` is
  0xc on `Array`, 0x8 on `String`, 0x14 on `TypedDataBase`. Measured reachability
  is the blocker: of 95,964 and 135,995 field loads, the fraction whose receiver
  class is identifiable from a nearby class-id check is 0% and 0.0007% on an exact
  match, and 0.6% and 1.6% under a deliberately loose upper bound. Class-directed
  field naming is therefore **not** viable without the snapshot field tables, and
  the dominant `obj.fN` noise stays until that work lands.

## R9. The fallback emitter had no join merge

### What was wrong

`emit_block` emits a block once, guarded by `emitted`, under whichever path
reached it first. Every other path then reads that path's register state. Arm
isolation was already present and is a different thing: restoring the pre-branch
state around an arm asserts that state still holds afterwards.

The output was therefore not merely unresolved but impossible. `mov x0, x22`
before a branch left x0 bound to `null`, and a shared successor rendered
`null._tag`, a read of the object header off null. Instrumenting the base
expression of every field read found 1,922 such reads across 71 functions.

Three symptoms, one cause, all visible in `sub_619e18`:

| symptom | mechanism |
|---|---|
| `if (null == null)` | block 3 sets x0 from NULL_REG, block 10 compares x7 against it |
| `null.f24` | block 6 sets x4 from NULL_REG, block 13 reads a field off it |
| `((true >> 4) & 1)` | block 24 materialises `true`, block 25 tests bit 4 of it |

### Two hypotheses killed by experiment

Both were plausible and both were wrong, which is why they were tested rather
than argued.

- **A missing structured merge.** Forcing `merge_state_at_join` at every
  structured block rather than only at joins moved the count from 165 to 161. The
  leaking functions are on the fallback path, so the structured merge never runs
  for them at all.
- **An under-computed write set.** `registers_written_between` roots its forward
  walk at a join's immediate predecessors, so a write in an earlier block of the
  same arm is an ancestor of a root rather than a successor. Extending the set to
  every ancestor changed nothing, 165 to 165. The branch-level merge already roots
  at the arms and covers the whole arm subgraph.

The `omitted complex paths` marker, which correlated at 77.9% against 10.3% of
clean functions, is a confounder rather than a cause: it marks the complex
functions that land on the fallback, and omission emits a `return`, so no
same-path read follows it.

### The fix and its price

The merge drops registers written by any block that can reach the join, by
backward reachability over `ir.blocks[..].succs`. Not the whole function, which
would wipe state at every join. Not `Regions`, which is absent precisely here,
because `Regions::build` returns `None` for an irreducible CFG. The join's own
writes count when it sits in a cycle, since they are live on re-entry, and
predecessor lists are deduplicated so a branch whose two targets are the same
block still has one predecessor.

| measure | LocalSend | Immich |
|---|---|---|
| `null.fN` / `null._tag` | 165 -> 8 | 277 -> 7 |
| bit test on a literal operand | 701 -> 7 | 1,611 -> 2 |
| fully-constant `if` conditions | 930 -> 164 | 1,809 -> 241 |
| `raw_register_name_refs` | 55,735 -> 95,659 | 66,199 -> 114,348 |

The cost is concentrated where the defect was. 723 functions carry a fallback
marker on LocalSend, 12.5% of the count but 58% of the emitted lines, and their
unresolved-register density goes from 0.133 to 0.293 per line. That is the honest
price of emitting a shared block once without knowing which path reached it.

Worth being precise about what the tightening bought, since it is easy to assume
it was the point. Dropping every register the function writes, rather than only
those written by blocks that can reach the join, gives 96,839 against 95,659: the
backward-reachable set saves 1,180 references, 1.2%. In a CFG complex enough to
reach the fallback, nearly every block can reach nearly every join, so the
narrower set is structural insurance rather than a measured saving. It is still
the right rule, because the looser one would wipe state at joins in a small
fallback function where it is not warranted, but it did not buy the readability
back.

### Shifted operands changed the value

ARM64 lets the last source operand carry a shift or an extend, and both were
rendered as a trailing comment. So `cmp x3, x0, asr #1` read `a == b` where the
truth is `a == (b >> 1)`. That is the Smi round-trip check Dart emits after
tagging, so it appears wherever an integer is boxed, and it is a condition, so the
structurer read it too.

Every flag-setting arm now applies the modifier. An extend narrows and then
shifts, and the shift amount is kept: `sxtw #3` scales by the machine word, and
dropping the `#3` would render a scaled index as unscaled, a wrong value rather
than a missing one. `lsr` is `>>>` because Dart's `>>` is arithmetic, and a
modifier the lifter does not model reports itself rather than vanishing.

Still deferred and still coupled: the non-flag arithmetic arms keep the comment
form, 5,938 and 8,055 sites, because `try_parse_shifted_pool_field` parses that
exact form to fold a pool page into an entry index.

### Pointer mode is a fact, not an inference

Compression decides the width of a reference field and the value of `kSmiBits`,
so it selects which offset table describes the runtime. It had been inferred from
instruction shape, which is sound where evidence exists but unbounded where it
does not: a binary with no compressed reference loads produces no evidence either
way.

It does not need inferring. `runtime/vm/snapshot.h` fixes the header as magic
`0xdcdcf5f5`, an `int64` length, an `int64` kind, 20 bytes.
`WriteVersionAndFeatures` then writes `Version::SnapshotString()`, the
32-character snapshot hash with no separator, followed by the NUL-terminated
features string, and `Dart::FeaturesString` appends exactly one of
`compressed-pointers` or `no-compressed-pointers`.

Both samples read as compressed, which confirms the instruction stream
independently and matches Flutter's `tools/gn` forcing it for Android arm64.
Reported as `compressed_pointers` in `info`. The negative spelling has to be
tested first, since `compressed-pointers` is a substring of it.

### Why Thread offsets are still not named

The research is complete and the tables are derived, but the gate is not met.
Every observed offset was mapped for both versions: 108 rows for 3.5, 117 for
3.12, from the AOT ARM64 compressed blocks. Direct THR loads number 8,930 and
22,216, and ten stub entry points cover 99.7% of the 3,187 and 3,456 THR-indirect
calls.

What blocks it is that the same raw offset means different things across blocks:

| offset | Dart 3.5 | Dart 3.12 |
|---|---|---|
| `0x68` | `field_table_values` | `end` |
| `0x78` | `object_null` | `field_table_values` |
| `0x90` | `empty_array` | `object_sentinel` |
| `0x208` | `call_to_runtime_entry_point` | `array_write_barrier_entry_point` |
| `0x238` | `stack_overflow_shared_without_fpu_regs` | `allocate_object_slow_entry_point` |

110 of 111 common offsets moved between the two versions, and only 3 agree between
the compressed and uncompressed blocks of a single version. So no partial match is
safe: naming from the nearest table would produce `thread.stackLimit` pointing at
the wrong field, which is a claim where `thread.f56` is a gap. The rule has to be
an exact (hash, mode) match against a vendored table, and an unknown hash keeps
the numeric form.

### Why field naming is still not viable

Exact per-class offset tables exist and did not drift between 3.5 and 3.12 for the
core classes, and the tag adjustment is confirmed exact: logical offset N is
emitted as displacement N-1, from `FieldAddress(base, disp - kHeapObjectTag)`.

But no offset means the same thing across classes. `length` is `0xc` on `Array`
and `GrowableObjectArray`, `0x8` on `String`, `0x14` on `TypedDataBase`; `0x8`
alone is `type_arguments`, `parent`, `first_field` or `data` depending on the
class. So naming a field requires the receiver's class, and the measured
reachability is the blocker: of 95,964 and 135,995 field loads, the fraction whose
receiver class is identifiable from a nearby class-id check is 0% and 0.0007% on an
exact adjacent triple, and 0.6% and 1.6% under a deliberately loose upper bound
that overcounts. The dominant `obj.fN` noise waits on the snapshot field tables.

## R10. Multi-instruction idioms, measured

Single instructions are largely modelled now, so the remaining readability is in
sequences. Each candidate below was located in the SDK and then counted in the
disassembly of both samples, so the ranking is by evidence rather than by how
appealing the rendering would be. Counts are IR instruction or window counts, not
emitted lines.

| idiom | LocalSend | Immich | status |
|---|---|---|---|
| write barrier check | 8,846 | 12,355 | **named** |
| null check shared stub calls | 7,378 | 10,777 | gated on stub identity |
| bounds check shared stub calls | 5,729 | 3,477 | gated on stub identity |
| inline allocation fast path | 1,989 | 2,401 | gated on cid recovery |
| type test stub calls | 967 | 1,605 | type not recoverable |
| one-byte code unit read | 462 | 352 | below the bar |
| two-byte code unit read | 0 | 0 | absent |

### What landed

The write barrier. `StoreBarrier` ends with `tst(scratch, HEAP_BITS LSR #32)`,
whose high half is the barrier mask, and HEAP_BITS is reserved, so the recognition
is exact by construction. See R9 for the two limits taken deliberately: only the
comparison's left side is named, so the branch keeps its own polarity, and the
argument is not reconstructed into `(object, value)`.

### What is gated, and on what

- **Null and bounds checks** are the largest remaining group, 13,107 and 14,254
  calls between them. They are *direct calls to a handful of shared stub
  addresses*, which is what makes them countable, and also what blocks naming
  them: the mapping from address to stub kind was inferred from context and call
  frequency, not derived. Naming `sub_d521f4` as a null-check throw on that
  evidence would be a per-binary guess. What would make it sound is deriving the
  address from the snapshot's stub table rather than recognising it by shape.

  Two denominators, and they differ slightly, so the basis matters. Counting
  direct calls to the six null-family stub addresses gives 7,378 and 10,777.
  Counting the guards instead, `b.eq` blocks whose successor is one of those call
  blocks, gives 7,376 and 10,775. The two disagree by two on each sample because
  some stub blocks are shared or reached through a different entry layout, so
  neither number is the count of "null checks" without saying which it is.
- **Inline allocation** is `ldp result, end, [THR, top_offset]`, then add size,
  compare, branch to the slow path, initialise the header. The sequence is exact
  and spans a branch, so recognising it means matching across blocks. `allocate(n)`
  is honest; `new <cid>()` needs the class id, which is in the tags word being
  stored and would have to be tracked to the store.
- **Type tests** call `slow_type_test_entry_point` through THR with the instance
  in R0 and the destination type in R8 (`TypeTestABI`). The checked type is *not*
  in that call: the two pool entries it reserves are the subtype cache and the
  destination name. So the honest rendering names the operation and not the type,
  and naming the operation at all needs the THR stub table, which is gated in R9.
- **Element reads** distinguish width but not class. `Array` data sits at logical
  `0x18` and both string kinds at `0x10`, so machine displacement `0xf` is shared
  by one-byte and two-byte strings and only the load width separates them. A
  displacement alone cannot name the class.

### The shape of what is left

Every remaining item is blocked on the same two things, and neither is a pattern
problem. Naming a runtime entry point needs the (hash, mode) stub table. Naming a
field or a class needs the snapshot's own tables. Both are data the binary
contains and the decompiler does not yet read, which is why the answer to the
dominant `obj.fN` noise is not a better heuristic.


## R11. What the register gap actually costs, measured

`regN` in the output is a register whose value the emitter did not have. The
question was whether a smarter merge could recover them. Two independent
measurements say mostly no.

Symbolic CFG replay over the marker subset (663 LocalSend functions, 830 Immich),
comparing all non-tainted incoming edge states at every join with a 16-state cap
and downstream taint marking, so a conservative lower bound:

| sample | agreeing | candidate dropped-binding sites | rate |
|---|---|---|---|
| LocalSend | 5,567 | 49,119 | 11.33% |
| Immich | 6,445 | 54,446 | 11.84% |

Including `x29` lifts it to 23.65%/24.51%, but `x29` is the frame pointer and is
never emitted as `regN`, so that number is an artifact. Per-register the
agreement concentrates in `x0`-`x3` (LocalSend `x0` 1,304 of 8,514).

Raw `regN` references: 94,923 and 113,046, density 0.290 per line. Of those,
3,136 (3.30%) and 5,113 (4.52%) are single-use within their function.

So an exact join merge recovers roughly one binding in nine. The remainder needs
real dataflow, not a better merge heuristic. Recorded so the next attempt starts
from the measurement instead of the intuition.

## R12. Width, and where the mask belongs

`canonical_reg` folds `w1` and `x1` onto one key because they are one machine
register. The width is still semantic: a `w` form computes in 32 bits and
zero-extends. Where that matters is not uniform:

- `and`/`orr`/`eor` of two 32-bit values: result already fits, nothing to say.
- `add`/`sub`/`mul`: halves agree except on overflow.
- `neg`/`mvn`: **always** differ. `~x` sets every high bit where the machine
  clears all of them. Fixed, but only 6 sites on LocalSend and 0 on Immich.
- `lsr`/`asr`/`sdiv`/`udiv`: the *operand* width decides the answer.
  `lsr w0, w1, #4` is `(x1 & 0xffffffff) >>> 4`, which differs from
  `(x1 >>> 4) & 0xffffffff` in bits 28-31 whenever the high half is live --
  exactly the case the `w`/`x` fold creates. `asr w` sign-extends from bit 31,
  so it needs `signExtend(x1, 32) >> n`, not a mask at all.

Deliberately not landed: a trailing mask on the shift and division forms. It
would read as fixed while still being wrong. The asymmetry is pinned by test.


## R13. Stub identity, derived from the callee's own prologue (landed)

Null and bounds checks are the biggest unnamed call group. They were blocked on
"deriving stub identity rather than inferring it from call frequency". They are
derivable, and the derivation is exact.

An ARM64 generic shared stub loads its own `Code` object from a fixed `Thread`
slot before the runtime call: `GenerateSharedStubGeneric` does
`ldr CODE_REG, [THR, #self_offset]` after the canonical pushes
(`stub_code_compiler_arm64.cc:287-337`), and each error generator passes its own
self offset plus runtime entry (`stub_code_compiler.cc:1666-1727`, RangeError at
`stub_code_compiler_arm64.cc:617-668`). So the callee's *own prologue* names it.

Measured on the IR, mapping each direct call target to the THR self-offset in its
prologue:

| sample | null-check calls | distinct targets | bounds calls | distinct targets | mapped |
|---|---|---|---|---|---|
| LocalSend | 7,378 | 5 | 5,729 | 2 | 100% |
| Immich | 10,777 | 4 | 3,477 | 2 | 100% |

Across all generic shared stubs: 11 distinct target addresses per sample, 29,922
and 39,811 calls, every self-offset mapping to an exact SDK name.

Why the names are not in the binary: `VM_STUB_CODE_LIST` and `StubNames[]` are
compile-time (`stub_code.h:57-68,111-119`); `NameOfStub` is a runtime
entrypoint->name search (`stub_code.cc:350-378`). The VM snapshot serializes stub
roots in list order (`app_snapshot.cc:6997-7015`, restored at `7110-7114`) but
`WriteRootRef` emits a name only when a profile writer is attached
(`app_snapshot.cc:439-446`). Names do exist at write time:
`SnapshotTextObjectNamer::AddNonUniqueNameFor` calls
`StubCode::NameOfStub(insns.EntryPoint())` and prefixes `stub ` for stub code
(`image_snapshot.cc:1300-1332`), fed to `AddCodeSymbol`
(`image_snapshot.cc:886-944`). But those are *local static symbols*, not payload:
`ELF::InitializeSymbolTables` omits `.symtab`/`.strtab` when stripped
(`elf.cc:1244-1272`), so they survive only in an unstripped or separate-debug
artifact.

In the release artifact the bare `InstructionsSection` header carries only
payload length, BSS offset, relocated address and build id
(`object.h:6078-6110`, `raw_object.h:2217-2241`), and `WriteText` concatenates
payloads with no per-stub name table (`image_snapshot.cc:743-890,1020-1063`).
What *is* present is the RO-data `InstructionsTable`, whose `DataEntry` holds
`{pc_offset, stack_map_offset}` alongside the code-object sequence
(`raw_object.h:2459-2488`, `app_snapshot.cc:8302-8387,8421-8434`), and the
deserializer associates cluster order with `EntryPointAt`
(`app_snapshot.cc:1853-1864,9618-9639`). So the two exact routes are the
prologue self-offset above, and ordered stub roots joined to that table.

The gate is the same as for thread offsets: the self-offset is version and mode
dependent, so it needs the `(hash, mode)` table, which is now readable from the
snapshot header and vendored for 3.5 and 3.12 under `docs/research-data/`. Not
frequency, not address hardcoding.

### What landed

`shared_stub_names` scans each function's prologue for `ldr rD, [x26, #imm]` --
`THR` is `R26`, confirmed in the stream where `x26` is the base of 2,971 of the
sampled loads -- and accepts the displacement only if it is a member of the
vendored stub-slot set for the binary's SDK **and** at least one push onto the
Dart stack precedes it. Ordinary functions are not at risk: their prologue loads
the stack limit (`0x48` on 3.12, 1,537 sampled loads), a thread field that is not
in the set.

Two shapes cost coverage before the push requirement and the window were
measured rather than assumed:

- A **trampoline that tail-calls a stub** reads the same slot the stub reads from
  itself: `ldr x24, [x26, #0x1d8]; ldur x16, [x24, #7]; br x16`. Two per sample
  matched and were given the stub's own name, and one guards its tail call with
  `cmp w0, w22`, so that name hid a null check.
  `GenerateSharedStubGeneric` saves the register set first and the trampolines
  push nothing, which separates them exactly.

  Refusing the stub's name would have made 520 and 696 call sites anonymous
  again, so they are named `<stub>Thunk` instead: the whole body after the load
  is `ldur` the entry point then `br`, which never returns, so the wrapper is
  derivable rather than guessed. An ordinary caller uses `blr` and continues
  afterwards; naming that after the stub it calls would be the same false claim,
  so it stays anonymous. Both directions are pinned by test.
- The **mint allocators** save the FPU set too, putting their self-load at
  instruction 37 on 3.5 and 38 on 3.12. A 32-instruction window dropped one stub
  from every binary silently, because a stub that goes unnamed looks identical to
  a stub that is not there. The window is 48.

Result, with the derived names fed through the existing `symbol_names` channel at
`Exact` quality:

| sample | SDK | named | call sites named |
|---|---|---|---|
| LocalSend | 3.5.0 | 12 stubs + 2 trampolines | 21,920 |
| Immich | 3.12.1 | 12 stubs + 2 trampolines | 26,663 |

The output corroborates itself. `if (reg0 == null) { nullCastErrorSharedWithoutFpuRegs(); }`
and `if (index >= smiUntag(reg2.f28.f20)) { rangeErrorSharedWithoutFpuRegs(index, ...); }`
-- the recovered guard independently matches the stub the name claims.

### What the coverage depends on

The stub code has to be in the disassembled function set, which comes from the
model's function table. That is not automatic:

| model | functions | stubs in range | named |
|---|---|---|---|
| `dynamic_snapshot_string_model_v1` (LocalSend, Immich) | 5,800 / 8,329 | yes | 14 |
| `blutter_bridge_model_v1` (LocalSend) | 39,343 | no | 0 |

More functions and fewer names: the blutter table lists far more app functions but
does not cover the runtime stub range, so `0xd51c4c` and its siblings are never
disassembled and there is no prologue to read. The figures above are from the
string model on both samples.

This is the third model-gated subsystem in this document, after the selector
annotations and `dispatch.<name>`. Unlike those two it fails loudly:
`report.json.shared_stub_naming.status` reads `no_stub_prologues` with the version
and pointer mode still populated, which distinguishes "the table was selected and
matched nothing" from "the SDK is unknown". Naming shared stubs arguably should
not depend on the app-function table at all, since they are runtime code reached
by `bl`; closing that means disassembling call targets outside the model's list,
which is a loader change rather than a naming one.

### The version hazard, and two guards

**All eleven slots present in both vendored tables disagree on the name.** `0x118`
is `nullCastErrorSharedWithoutFpuRegs` on 3.12 and
`writeErrorSharedWithoutFpuRegs` on 3.5; `0x128` is `rangeError...` on 3.12 and
`allocateMint...` on 3.5. A version confusion therefore mislabels *every* call
site, not some.

So the header is not trusted alone. The binary's own offset set fingerprints the
SDK: the correct table matches 14 prologues on each sample and the other matches
7 and 8. If the header's version is not the best-scoring table, naming is
refused. On an equal score the names themselves are compared, because an equal
count is not agreement -- a binary with one shared slot separates nothing, and
that case refuses too.

A semantic cross-check was measured and found **insufficient** as a validator:
call sites of the stub named `nullCastError` are 100% preceded by `cmp` against
NULL_REG on both samples, but so are the sites of `nullError` when the wrong
table renames them, because both are null-related. It distinguishes null from
range, not null-cast from null-error. Kept as a measurement, not as a gate.

`report.json.shared_stub_naming` carries `status`, `named`, and the two keys the
gate used, so a zero is diagnosable rather than indistinguishable from a feature
that never ran: `unknown_key`, `no_stub_prologues`, `table_disagreement`, `named`.


## R14. Two naming subsystems are dormant on real input

Measured zero output on both binaries:

- `[selector]`, `// stdlib:`, `// framework:`, `// package:` intent annotations.
- `dispatch.<recovered-name>(` from `helpers/call_intent/intent.rs:145`.

Both need adapter selector metadata or symbol names the current adapter does not
manifest. The `dispatch.selN` table path, which does fire, is the one fixed in
this cycle. Worth knowing before anyone extends these: the code is implemented and
tested, and the input is missing, so the tests pass while the output is empty.

Do **not** copy the `selN` fix to `intent.rs:145`. The two cases are asymmetric.
`sel25768` is self-evidently a placeholder, so a bare `sel25768(...)` cannot be
misread. A recovered human-readable selector is the opposite: bare
`minWidth(...)` reads as a resolved top-level function call, and Dart has
top-level functions, so that is a *stronger* claim than `dispatch.minWidth(...)`,
not a weaker one. If that path is ever touched, keep a prefix that cannot be
mistaken for a resolved call and move the admission into the comment
(`// selector: minWidth, receiver: unrecovered`).

Blast radius if anyone tries: about ten assertions in
`tests/cfg_and_stack/call_and_loops.rs`, plus `docs/how-it-works.md:119` and
`:626`, where `call_fallback` diagnostics classify on the `dispatch.invoke(...)`
string.

Consequence for the showcase: `docs/assets/readme-src/` shows `dispatch.minWidth`,
which the pipeline cannot currently produce on a stripped APK. The `/* lsl #2 */`
in the same asset was impossible for a different reason and is corrected.


## R15. A stub call is not a Dart call, and the difference was 24% of the output

Naming the shared stubs (R13) made a second defect visible: every call to one was
still modelled as an ordinary Dart call. Three SDK facts contradict that, and each
was measurably wrong in emitted output.

### The fall-through after a raising call does not exist

`GenerateSharedStub` takes `allow_return`, and when it is false the generator
emits `Breakpoint()` in place of the epilogue
(`stub_code_compiler_arm64.cc:303-307`). Independently visible in both binaries:
every non-returning stub body reaches `brk` before any `ret`, every returning one
reaches `ret` first. The disassembler records a fall-through edge after any
non-terminator, so the edge after such a call was fiction.

45.7% of LocalSend functions and 42.5% of Immich functions contain one. Cutting
those edges before either emitter sees the CFG:

| | LocalSend | Immich |
|---|---|---|
| emitted lines | 387,070 -> **295,465** (-23.7%) | 530,129 -> **408,442** (-23.0%) |
| `omitted complex paths` markers | 663 -> **439** | 830 -> **520** |

Nearly a quarter of all output was unreachable code rendered as live. That also
confirms a note already in `structured.rs:431-436`: the shared non-returning slow
path, with many predecessors and no successors, post-dominates nothing, is never a
follow node, and "alone accounted for 84% of the fallbacks". Removing its fake
edges is why the fallback marker count fell by a third.

Blocks are cut, not removed. `regions.rs:38` rejects a CFG whose block ids are not
dense and `structured.rs` iterates `0..blocks.len()` as ids, so dropping a block
without renumbering would make region recovery fail and push the function onto the
very fallback emitter this is meant to feed less. Clearing the edge is enough:
both emitters walk from the entry, so an orphan is never visited.

### Returning is not the same as defining a value

`store_runtime_result_in_result_register` is a separate SDK flag, defaulted false
and set only for the mint allocator (`stub_code_compiler_arm64.cc:1481,1501`). So:

| stub | returns | defines a value |
|---|---|---|
| null / nullArg / nullCast / range / write / fieldAccess / lateInit errors | no | no |
| `stackOverflow` | yes | no |
| `allocateMint` | yes | yes, in `R0` |
| `slowTypeTest` | yes | no -- `TypeTestABI::kInstanceReg` is `R0` and *preserved*; the answer goes to `R7` (`constants_arm64.h:237-258`) |

`final tN = stackOverflowSharedWithoutFpuRegs()` was therefore as false as binding
a throw, and `stackOverflow` is the largest family by call volume. Bindings on a
call that defines nothing: **19,328 -> 0** and **23,646 -> 0**. `allocateMint`
stays bound, 1,800 and 2,074 sites.

### A shared stub clobbers nothing

It pushes and pops `AddAllNonReservedRegisters` around the runtime call
(`stub_code_compiler_arm64.cc:300,309`; `locations.h:692-703`), so every
caller-saved binding survives it. Treating it as a normal call was dropping them
at the commonest call site in the binary:

| | LocalSend | Immich |
|---|---|---|
| `raw_register_name_refs` | 94,923 -> **65,835** (-30.6%) | 113,046 -> **84,822** (-25.0%) |

This is the largest single reduction in register noise this project has measured,
and it came from an ABI fact rather than from any dataflow work.

The figure moved *up* later in the cycle, from 62,066 and 77,641, and the direction
is correct. `written_registers` reported no destination at all for a store and only
the loaded register for a load, so it missed the base register that a pre- or
post-indexed access writes back. 2,346 and 1,394 such instructions per sample have
a base outside the pinned set, and each one left that register's binding alive
across a join that had in fact redefined it. Reporting the base makes the merge drop
those bindings, which turns 5,045 and 9,141 stale values into admitted unknowns.
A `regN` that replaces a wrong value is a gain, not a regression.

### Most "irreducible control flow" was the fake edge

The biggest surprise. `Regions::build` gives up on three conditions only: an empty
CFG (`regions.rs:33-34`), a block id outside the block count (`:38-39`), and a
retreating edge whose target does not dominate its source (`:75-76`, leaf at
`:184`). Replaying that predicate exactly over both samples, before and after the
fake edges were cut:

| | LocalSend | Immich |
|---|---|---|
| irreducible | 250 -> **7** | 232 -> **7** |
| region recovery succeeds | 95.69% -> **99.88%** | 97.21% -> **99.92%** |

**97% of the irreducible control flow was an artifact of modelling a throw as a
call that returns.** A raising stub call with a fabricated fall-through creates a
retreating edge into a shared slow path that nothing dominates. The graph was
never irreducible; the model of it was wrong. Every earlier count in this document
that treated 250 irreducible functions as a structural floor was measuring that
defect.

Files carrying a DFS-only marker (`omitted complex paths`, `depth-limited block`,
`loop back-edges`) fell from 12.47% to 8.53% and 11.11% to 7.40%. That is a lower
bound on the fallback rate, not an exact one: `helper_flow/inlining.rs:151-180` can
inline a helper and erase the marker, so a fallback can leave no trace in the final
artifact. An exact figure needs a replay of `try_emit_structured`, whose remaining
decline gates are `depth > 64` (`structured.rs:135`), a repeated region rejected by
`is_repeatable_region` (`:157-160`, `:457-462`), a failed `render_loop`
(`:170-175`), and a coverage mismatch (`:55-67`).

### Corrections to earlier figures in this document

- R13 reported 21,920 and 26,663 named stub call sites. That grep counted name
  substring occurrences, including inside bindings and comments, over pre-prune
  output. Counted as call sites on current output: **10,054** and **11,613**.
- A stub's inputs are not in `DartCallingConvention`, so the inferred argument
  list described the wrong thing entirely and is dropped. The 409 and 457 that
  remain belong to the two trampolines, deliberately left as unknown callees.
- The last 1,180 sites resolved through `helper_flow/summary.rs:57`, which builds
  a second emitter for helper bodies and did not carry the table. Helper bodies
  are where the shared slow paths land, so that was most of the residue. Three
  wrong hypotheses were tried first (the indirect-call path, branch precedence,
  alternate entry points); only the last-mentioned was checked by measurement, and
  interior-offset stub calls turned out to be exactly zero in both samples.


## R16. What is left, measured on a correct CFG

Every figure below is from a replay over the post-prune artifacts, with controls.

### The structurer now succeeds on 89-90%, and the residue is mostly one shape

Faithful replay of `try_emit_structured`, first leaf gate per function:

| gate | LocalSend | Immich |
|---|---|---|
| structured | **5,173 (89.19%)** | **7,522 (90.31%)** |
| repeated region contains a loop header | 323 (5.57%) | 434 (5.21%) |
| repeated region over 8 blocks | 191 (3.29%) | 214 (2.57%) |
| repeated region over 48 instructions | 105 (1.81%) | 149 (1.79%) |
| `Regions::build` irreducible | 7 (0.12%) | 7 (0.08%) |
| depth over 64 | 1 (0.02%) | 3 (0.04%) |

Cross-checked against the artifacts: 495 and 616 pseudocode files carry a DFS-only
marker and **zero** of them replay as structured, so the replay never contradicts
the output, and the marker count is confirmed as a lower bound (627 and 807 actual
declines).

The two budget gates are tunable. Rerunning only those declines at 2x
(8->16 blocks, 48->96 instructions) clears 121 of 296 and 169 of 363, costing
23,519 and 28,145 duplicated instructions, median 91. At 4x it clears 193 and 269
but the tail is bad: one Immich function duplicates 45,863 instructions. 2x is the
defensible point if any.

`repeated:loop` is the largest gate and is **not** simply structural. Dart has
labelled `break` and `continue`, and `structured.rs:151-152` says so itself: "An
outer loop would need a labelled `continue`, which is declined for now". Splitting
those declines by loop count, 121 of 323 (37.5%) and 173 of 434 (39.9%) are in
functions with exactly **one** loop header, where no label is needed at all -- the
region's only loop content is a back edge to the enclosing loop, which
`structured.rs:143-146` already renders as `continue`. So `is_repeatable_region`
rejecting on `regions.is_loop_header` is too coarse for at least that share.

### The remaining register noise is dataflow, not missing instruction models

Taxonomy of every `regN` occurrence by the writer set of that register within its
function, controls 5,800 and 8,329 files and occurrence totals matching
`quality.json` exactly:

| class | LocalSend | Immich |
|---|---|---|
| has at least one modelled writer | 61,566 (**99.2%**) | 77,266 (**99.5%**) |
| only unmodelled writers | 284 (0.5%) | 191 (0.2%) |
| only a call writes it | 195 (0.3%) | 160 (0.2%) |
| live-in, never written | 21 (0.0%) | 24 (0.0%) |

The unmodelled class is overstated: the destination parser reads `tbz w0, #0, #...`
as writing `w0`, and `tbz` is a test-and-branch that writes nothing, as are the
`stur`/`stp`/`str` entries. So under half a percent, and the real figure is smaller.

So the value behind a `regN` is almost always computable from instructions the
lifter already models, and the loss happens at joins, loop headers and ordinary
call clobbers. That reframes the earlier 11.33% and 11.84%: those measured the
subset where every incoming edge already carries an *identical* expression, which
bounds what a naive merge recovers, not what dataflow could. The ceiling is near
total; the naive-merge recovery is small. Adding instruction models is not the
lever.

### A `bl` into the middle of a function means the extents are wrong

6.38% and 6.17% of direct call sites target an address that is not any
disassembled function's entry: 8,124 and 12,095 distinct targets. Two facts kill
the obvious response and reveal a better one.

**There is no head to exploit.** Every single outside-model target is called
exactly once -- maximum frequency 1, so p50, p90 and p99 are all 1. A naming-only
side table would therefore name at most 6.4% of call sites, one site each.

**79% and 83% of them are interior addresses of functions already disassembled**
(6,428 and 10,094). The offsets are not an entry-point convention. Dart AOT does
have alternate entry points -- `kMonomorphicEntryOffsetAOT = 8` and
`kPolymorphicEntryOffsetAOT = 24` on ARM64 (`object.h:5922-5923`), plus a variable
unchecked entry (`object.h:7041-7065`) -- but the measured distribution is:

| offset from containing entry | LocalSend | Immich |
|---|---|---|
| +0 to +8 | 0 (0.0%) | 2 (0.0%) |
| +9 to +24 | 248 (3.9%) | 361 (3.6%) |
| +25 to +64 | 131 (2.0%) | 195 (1.9%) |
| +65 to +256 | 1,092 (17.0%) | 1,563 (15.5%) |
| +257 to +1024 | 2,260 (35.2%) | 3,622 (35.9%) |
| +1025 and up | 2,697 (42.0%) | 4,351 (43.1%) |

As a fraction of the containing function's size the offsets are uniform: p10 0.08,
p50 **0.49**, p90 0.91 on both samples. A median at half the function is the
signature of a random position inside a record that spans several real functions,
not of a fixed entry-point offset. Only the 3.6-3.9% in the +9 to +24 bucket look
like alternate entries.

So the model's function extents are inflated. `_recover_functions` in the adapter
sizes a function as the gap to the next recovered start, capped at 0x8000, so any
real function it fails to recover is swallowed by its predecessor. The same defect
was visible earlier from the other side: `allocateMintWithFpuRegs` disassembles as
133 and 190 instructions against 45 and 46 for the without-FPU variant, because it
had absorbed its neighbours.

**Consequence for naming: snapping an interior call target to its containing
function would name it after the wrong function.** Deliberately not done.

A decisive discriminator confirms it. A real function entry is preceded by a
terminator; a branch target inside live code is preceded by an instruction that
falls through to it. Measured over the interior targets:

| predecessor of the target | LocalSend | Immich |
|---|---|---|
| a terminator (`ret`, `b`, `brk`, `br`) | 5,446 (84.7%) | 8,627 (85.5%) |
| falls through into it | **1 (0.0%)** | **3 (0.0%)** |
| nothing disassembled at target-4 | 981 (15.3%) | 1,464 (14.5%) |

One and three fall-throughs out of 6,428 and 10,094. These are function entries.

And the cost is larger than naming. Blocks unreachable from their own record's
entry: **210,355 of 290,636** on LocalSend and **284,242 of 388,402** on Immich,
72% and 73%. Both emitters walk from the entry, so none of that code is emitted,
which is why the output looks plausible while a great deal of the binary is absent
from it.

I first attributed all of that to swallowed neighbours. Partitioning it refutes
that, so the claim is corrected here rather than left standing:

| root of the unreachable component | LocalSend | Immich |
|---|---|---|
| an interior `bl` target -- splitting recovers this | 58,484 (27.8%) | 85,544 (30.1%) |
| downstream of a raising call -- correctly dead, this pass created it | 10,387 (4.9%) | 11,392 (4.0%) |
| **rooted at neither** | **141,484 (67.3%)** | **187,306 (65.9%)** |

So splitting at interior call targets recovers under a third. Two thirds is a third
cause I had not named: decoded blocks that no direct call and no fall-through
reaches. The likely mechanism is an entry reachable only through an *indirect*
call -- a dispatch-table slot, a closure, or a pool-held target -- which no `bl`
names, so direct-call evidence cannot discover it. That is a separate recovery
problem from extents, and it is the larger half.

The residual has a cause, and it is bigger than the split. Applying the same
terminator predicate to every *unreachable component root*, independent of whether
anything calls it:

| unreachable component root | LocalSend | Immich |
|---|---|---|
| roots total | 43,822 | 53,799 |
| follows a terminator | 40,222 (91.8%) | 48,501 (90.2%) |
| has a frame-setup prologue | 16,372 (37.4%) | 20,541 (38.2%) |
| is a direct `bl` target | 6,185 (14.1%) | 9,713 (18.1%) |
| **entry-shaped but uncalled** | **35,018 (79.9%)** | **40,252 (74.8%)** |

An unreachable block that follows a terminator is reached by nothing and preceded
by a return or an unconditional branch, so it is a function entry or alignment
padding, not a branch target. Most of them have no direct caller because in Dart
AOT most calls go through the dispatch table or an IC, leaving no `bl` to split on.

So the split predicate should be "looks like an entry", with a direct call target as
corroboration rather than the trigger. The conservative subset -- terminator-preceded
*and* carrying a frame prologue -- is 16,372 and 20,541. The permissive one is
40,222 and 48,501.

A terminator predecessor alone is not enough to call a root a function, and I
published an inflated count before checking. Dart catch-block entries are reached
only through the runtime's exception dispatch, so they are unreachable roots
preceded by a terminator, and they belong to the *enclosing* function: splitting
there would tear one function apart, the inverse of the extent defect. The
discriminator is the frame: a function entry *pushes* one with writeback
(`stp x29, x30, [x15, #-0x10]!`), a catch entry *restores* one from `x29`.

| unreachable component root | LocalSend | Immich |
|---|---|---|
| pushes a new frame -- a function entry | **16,021 (36.6%)** | **19,861 (36.9%)** |
| ... and also follows a terminator | 14,231 (32.5%) | 16,828 (31.3%) |
| ... and is also a direct `bl` target | 4,246 (9.7%) | 6,650 (12.4%) |
| restores a frame -- catch-like, do not split | 95 (0.2%) | 365 (0.7%) |
| neither | 27,706 (63.2%) | 33,573 (62.4%) |

So catch entries are a real hazard but a small population. The frame-push count is
a **lower** bound on missed entries, because a leaf function needs no frame at all,
and the terminator count of 40,222 and 48,501 is an upper bound. The truth is
between them.

Stated conservatively: the adapter declares 5,800 and 8,329 functions, and the
instruction stream carries frame-push entry evidence for at least 16,021 and 19,861
more, so roughly **3.8x and 3.4x** as many functions exist as are emitted. Not the
eightfold figure I first recorded from the permissive predicate. Either way the code
is not missing from the disassembly: it is decoded, sitting inside inflated records,
unreachable from the declared entry, and never walked.

The apparent function counts of 5,800 and 8,329 are records, not functions. Direct
call evidence alone raises the floor to about 12,200 and 18,400; the residual says
the true figure is higher still and not discoverable this way.

When the split is implemented it should be a post-pass partitioning the already
decoded `instructions` of an inflated record, needing no re-disassembly and no
adapter change. Pieces after the first must NOT inherit the record's
`function_name` or `owner_class`: that name belongs to the declared entry, and
copying it onto a swallowed neighbour would give thousands of functions a
confidently wrong name, which is the same defect class as a fabricated receiver.

**Consequence for coverage: each of those 6,428 and 10,094 addresses is exact
evidence of a function entry the model missed**, because a `bl` sets the link
register and expects to return, so its target is a callee entry rather than
intra-function control flow, which uses `b`. Splitting the containing record at
those addresses is derived, not heuristic. It is a loader and disassembler change
rather than a naming one, and it would move function counts, the disassembly ratio
and prioritization, so it is recorded here rather than attempted late in a cycle.


## R17. Allocation stubs name their own class, exactly

A per-class allocation stub materialises `MakeTagWordForNewSpaceObject(cid,
instance_size)` before tail-calling the shared allocate entry
(`stub_code_compiler_arm64.cc:2389-2451`). The tag layout is fixed: `SizeTagBits`
occupies bits 8 to 11 and `ClassIdTag` the 20 bits above it
(`raw_object.h:258-303`), so the class id is `(tag >> 12) & 0xfffff`, read off the
callee rather than inferred.

Validated by the shift being wrong any other way:

| | LocalSend | Immich |
|---|---|---|
| allocation stubs recognised | 1,059 | 1,353 |
| ids in 1..30,000 at shift 12 | **1,059 (100%)** | **1,353 (100%)** |
| ids in range at shift 8 | 187 (17.7%) | 285 (21.1%) |
| distinct ids | 1,059 | 1,353 |

One distinct id per stub, which is what per-class stubs must produce. Result:
4,623 and 5,413 call sites named across 399 and 587 distinct classes, and `sub_`
call references fall to 34,048 and 52,043.

**The id stays a number.** `CLASS_LIST` gained an entry before `FunctionType`
between the two SDKs -- `RecordType` sits at `class_id.h:77` in 3.5 and `:78` in
3.12, and `Finalizer`, `Record` and `SuspendState` shift with it -- so a single
vendored cid-to-name table would name one of these two samples confidently wrong.
Resolving `cid` to a class name needs the snapshot's class table
(`ClassTable::At(cid)`), which this pipeline does not read.

The pass runs ahead of every shared-stub gate on purpose. It needs neither the
vendored slot table nor the version, since the tag layout is identical in both
SDKs, and gating it would have discarded these names wherever the shared-stub
table refuses -- not hypothetical, since one model yields `no_stub_prologues` on a
binary whose allocation stubs are perfectly nameable. `allocation_named` is
reported separately so `status` still describes what actually failed.

### What the mint allocator does and does not say

The shared mint allocator only allocates. `BoxInt64Instr` on ARM64 binds its input
to an arbitrary register and issues the payload store itself *after* the call:
`StoreToOffset(in, out, Mint::value_offset() - kHeapObjectTag)`
(`il_arm64.cc:3800-3852`), and the runtime fallback returns
`Integer::New(kMaxInt64)`, a placeholder. So the returned object is empty at the
call and the value arrives from a following store. The current rendering -- bind
the call, then store into the binding -- is exactly that, and claims nothing more.

### Type tests carry a name, but not the type

`GenerateTTSCall` reserves two pool entries: the first a patchable null subtype
test cache, the second a patchable `dst_name` String, which is the *variable* name
used in the error message (`flow_graph_compiler.cc:2838-2873`,
`runtime_entry.cc:1877-1902`). The destination type itself is in `R8` at the call
site, not in the pool (`constants_arm64.h:234-243`). So a type test can yield the
name being assigned to, and the type only through the register.


## R18. A split predicate that needs no CFG, validated four ways

R16 established that the model's function records swallow their neighbours, and that
at least 16,021 and 19,861 missed entries are visible as unreachable component roots
that push a frame. That derivation needs the CFG. A split has to happen earlier than
that, on `FunctionDisassembly` before `build_program_ir`, so the useful question is
whether the same points are visible in the instruction list alone.

They are. Scanning each record's instructions in address order, a candidate is an
index whose predecessor is a terminator (`ret`, an unconditional `b`, `brk`, `br`) and
whose own instruction pushes a frame through x15 with writeback:

| | LocalSend | Immich |
|---|---|---|
| candidates from the instruction stream | 17,988 | 22,553 |
| frame-push unreachable roots from the CFG | 16,021 | 19,861 |
| CFG roots that are also stream candidates | 15,641 (**97.6%**) | 19,333 (**97.3%**) |

Two filters bring the stream figure down to something safe to cut at.

**Reject a candidate any branch inside the record reaches.** A conditional branch into
a frame push means the push is intra-function control flow, not an entry. That removes
1,729 and 2,320 candidates, 9.6% and 10.3%. Unconditional branches, which would be
tail calls to a genuinely separate function, hit 1 candidate and 0.

**Reject a candidate that sits below the record's own reachable code.** Every check so
far establishes that a root *looks like* an entry; none establishes that cutting there
does not amputate the function that currently emits. A layout of entry, code, an
unreachable root, then more code reachable from the first part would truncate a
function that works today, which is a regression on the part of the output that is
already correct. Taking the highest address reached from the record's entry and
requiring the candidate to lie above it rejects 95 and 78 candidates.

The containment clause has to be applied **sequentially**, not once against the record's
entry. Reachability from block 0 says nothing about a swallowed function: with two
candidates K1 below K2, cutting at K2 can still amputate the function whose entry is K1,
if that function branches forward past K2. Validating each candidate against the reach
of the *previous piece* rather than the record entry rejects 157 and 155 candidates, of
which 62 and 77 are visible only that way. The recursion is not free.

Two weaker forms of the check were measured and rejected. Requiring simply that no
branch below K targets at or above K over-rejects badly, accepting only 8,338 and 12,764,
because a branch inside a swallowed neighbour routinely crosses a later candidate without
saying anything about containment; and it still misses 90 and 73 real amputations that
the reach-based form catches. Reachability is the right relation, applied per piece.

Final predicate, four clauses, all evaluable from the instruction list plus intra-record
reachability:

| | LocalSend | Immich |
|---|---|---|
| terminator predecessor and frame push | 17,988 | 22,553 |
| minus branch targets | 16,258 | 20,233 |
| minus not contained in the preceding piece | **16,101** | **20,078** |

That lands within 1% of the CFG-derived count from R16, from the other direction, which
is the corroboration worth having: two independent derivations of the same set.

Those three figures are from a simulation over the emitted IR, and the implementation
measures slightly more: 18,487 and 23,087 candidates, 16,302 and 20,424 accepted. The
difference is provenance rather than logic, and it is worth naming because it took a
measurement to explain. The IR the simulation read is post-prune, so it is missing the
32,811 and 37,162 instructions the raising-call prune truncates. The split runs earlier,
on the unpruned `FunctionDisassembly` list, so it sees adjacent pairs the simulation
could not. Checked directly: the terminator and frame-push predicates agree on every
instruction in the corpus, zero disagreements, so the instruction set is the only
difference. The implementation's numbers are the correct ones.

### There is no other evidence for an entry point today

Shape is all there is, which is why the predicate has to carry its own precision. Over
the current artifacts, with controls of 1,677,866 and 2,406,966 instructions:

| evidence source | LocalSend | Immich |
|---|---|---|
| `LoadPool` instructions | 96,790 | 140,657 |
| distinct `LoadPool` target forms | **1** (`poolOff[N]`) | **1** (`poolOff[N]`) |
| pool entries that are addresses | 0 | 0 |
| dispatch-table loads | 11,536 | 18,716 |
| dispatch-table code addresses readable | 0 | 0 |
| indirect register calls | 23,967 | 27,888 |
| indirect targets resolved to a constant | 0 | 0 |
| unreachable roots corroborated by any of the above | **0 of 39,633** | **0 of 47,444** |

Not one pool load in either binary resolves to an address: every one of 96,790 and
140,657 renders as a byte displacement, which follows from the geometry being unset. The
evidence exists in the snapshot -- a dispatch-table slot is a `uword` entry point
(`dispatch_table.h:16-31`, filled by `GetEntryPointByCodeIndex` in
`app_snapshot.cc:9390-9464`), and the object pool holds `Code` and `Function` references
that `TracePool` treats as indirect call targets (`app_snapshot.cc:2723-2740`) -- but the
model has no dispatch-table field at all and the adapter sets every pool entry's
`target_va` to `None`.

The zeros above are floors rather than proofs. The indirect-call scan resolves only
`mov`/`movz`/`movk` chains within a block and invalidates on any other write, so a
constant built across a block boundary would be missed.

### What the output is made of now

For deciding where the next lever is, per emitted line across both samples:

| shape | LocalSend | per line | Immich | per line |
|---|---|---|---|---|
| local `tmpN`/`objTmpN` | 117,143 | 0.40 | 170,043 | 0.42 |
| field read `obj.fN` | 91,571 | 0.31 | 130,230 | 0.32 |
| call temporary `tN` | 88,113 | 0.30 | 131,121 | 0.32 |
| unresolved `regN` | 65,835 | 0.22 | 84,822 | 0.21 |
| anonymous `sub_` call | 34,048 | 0.12 | 52,043 | 0.13 |
| `poolOff[N]` displacement | 14,567 | 0.05 | 23,178 | 0.06 |
| named runtime stub call | 10,054 | 0.03 | 11,613 | 0.03 |
| `smiUntag`/`smiTag` | 7,301 | 0.02 | 9,217 | 0.02 |
| `selN` dispatch | 5,173 | 0.02 | 8,187 | 0.02 |
| `allocateClassId` | 4,623 | 0.02 | 5,413 | 0.01 |
| omitted-path marker | 439 | 0.00 | 520 | 0.00 |

The two largest are not defects: a local name and a call temporary are how any
decompiler names an intermediate. `obj.fN` at a third of a line is the field-naming
problem waiting on the snapshot's class and field tables. `poolOff[N]` waits on pool
geometry. So of the shapes this pipeline can act on alone, `regN` and the anonymous
call remain the two, and the anonymous call is what a record split addresses.


## R19. The split, landed, and the latent defect it uncovered

`--split-records` implements the R18 predicate as a pass over `FunctionDisassembly`
before `build_program_ir`, so each piece gets dense block ids and an entry at block
zero, which is what `Regions::build` requires. Measured on both samples:

| | LocalSend | Immich |
|---|---|---|
| records declared by the adapter | 5,800 | 8,329 |
| records that held more than one function | 2,875 | 4,576 |
| functions recovered | **16,302** | **20,424** |
| functions emitted after the split | **22,102** | **28,753** |
| emitted lines | 295,450 -> 846,661 | 408,421 -> 1,162,933 |
| wall time | 102s | 137s |
| candidates rejected: branch target | 1,906 | 2,390 |
| candidates rejected: not contained | 279 | 268 |
| candidates rejected: no block | 0 | 5 |

The gates hold. `disassembly_ratio` stays 1.0 because its numerator is the pre-split
record count, the quality report passes, and `shared_stub_naming.status` is still
`named` on both, which was the risk worth checking: the split changes the population
the vendored-table fingerprint scores. Simulated beforehand and confirmed after, the
correct table gains a prologue as buried stubs become their own records, and the
margin moves from 1.71x to 1.62x on LocalSend and from 1.50x to 1.62x on Immich
rather than flipping.

`rejected_no_block` was added purely so a silent abandonment would be visible, and it
immediately earned it: 5 candidates on Immich.

The flag is opt-in, and the reason is not runtime. It multiplies the emitted function
count, so every absolute quality counter grows, and `--max-functions` and
`--function-scope` continue to apply to records rather than to what is emitted.

### The defect the split uncovered

The first full run died after 26 minutes, killed by the kernel at 23.8GB resident.
Bisecting by function found `sub_964fc8` in LocalSend, and inside it the block at
`0x969aa0`: one block, 217 straight-line instructions, and a one-gigabyte allocation
request. No branch, so no visit budget or depth limit applied.

It is a mixing routine, and the constants say so: `0x1fffffff`, `0x7ffff`,
`0x3ffffff`. Three registers feed each other through `add`, `and` and `lsl`, and
because every modelled instruction builds its value out of the *text* of the values
it reads, the string doubles every few instructions.

**This was latent, not introduced.** Any binary whose reachable code contains a hash
of that shape would have hit it; the split only made this block reachable. The cap is
on the read: past 512 characters a register reads as itself and renders as `regN`,
which is the gap this emitter prefers to a value nobody can read. Eight reads in the
lifter and four more that put a stored value directly into emitted text now route
through one helper, including the `ldp` second-destination path, which stores its
result and so compounded outside the first version of the fix.

With the cap, both samples complete in 102 and 137 seconds, and the longest emitted
line across either is 2,660 characters. Without it the test fixture reaches 175,787
characters at six iterations of the real shape and 109,863,287 at ten.

## R20. What the reader actually sees after the split, measured

> **Scope note, added at round-2 close. R20's absolute figures describe `2619ec7`, not HEAD, and the
> *ranking* is what survives.** Two later changes moved the population underneath it. R22's structurer cut
> rendered lines by roughly 10% on both samples - the census below reports 867,551 / 1,190,169, HEAD reports
> **777,937 / 1,074,372** - and R29 added value annotations that did not exist when this was measured. Emitted
> file counts (22,102 / 28,753) and the per-file `splitlines()` convention are unchanged and still correct.
>
> So read this section for **which shapes dominate and why**, which is what it was written to establish, and
> read R22 and R29 for current magnitudes. Re-measuring the full ranking at HEAD would be its own round; it
> has not been done, and this note exists so nobody quotes a stale absolute as current. The trailing-newline
> subsection below is the one part that is fully superseded - that defect is fixed, as its own closing
> paragraph records.

Every shape figure above predates `--split-records`, which took emitted functions from
5,800 to 22,102 and 8,329 to 28,753. That changed both the population and the
denominator, so the ranking of what a reader encounters was unknown. This is the first
census of the post-split corpus.

All figures come from the pinned baseline binary built at `2619ec7`, run with
`--adapter-backend internal --emit-ir --function-scope all --split-records` and the
permissive gate flags. Both samples: LocalSend snapshot Dart 3.5.0, Immich 3.12.1.

### The denominator convention has to be stated

Two counting conventions differ on the same corpus by exactly the number of files that
end without a newline:

| convention | LocalSend | Immich |
|---|---|---|
| per-file `splitlines()` | **867,551** | **1,190,169** |
| `cat corpus` then `wc -l` | 846,661 | 1,162,933 |
| difference = files lacking a trailing newline | 20,890 of 22,102 | 27,236 of 28,753 |

This document already recorded the effect at the pre-split scale (line 258: `wc -l`
"reports 624,493 for the same output because 4,436 of the 5,800 files end without a
trailing newline"). Post-split it is 94.5% and 94.7% of files. The consequence is
sharper than a counting nicety: concatenating the corpus splices the last line of one
file onto the first of the next, so a `cat`-based scan destroys ~20,890 and ~27,236 line
boundaries. **Every rate below uses per-file `splitlines()`.**

**That defect is now fixed** (`pipeline/helpers.rs::terminated`, applied to pseudocode, asm
and IR artifact writes). Verified on a 2,343-file sample: 0 files lack a terminator in either
artifact kind, and `cat corpus | wc -l` now returns 76,831, exactly equal to per-file
`splitlines()`. The two conventions converge, so the ambiguity that produced this subsection
cannot recur.

What that does and does not supersede. **Corpus digests are superseded** on the far side of
that commit, since every emitted file gained a byte. **The per-line rates in this section are
not affected at all**, because `splitlines()` counts an unterminated final line and a
terminated one identically - which is precisely why the record standardised on it rather than
on `wc -l`. Had the rates been published on the concatenated convention, this fix would have
moved every one of them by 2.4%.

### Ranked by what a reader encounters

One classification per physical line, by parsing the line rather than counting substring
hits. Rows overlap, a single line can be a pool reference, a field access, a call and a
guard at once, so this is a census, not a partition, and the rates are not additive.

| rank | line shape | LocalSend | per line | Immich | per line |
|---:|---|---:|---:|---:|---:|
| 1 | synthetic local (`tmpN`/`objTmpN`/`intTmpN`/`resultTmpN`) | 306,126 | 0.3529 | 453,062 | 0.3807 |
| 2 | call temporary `tN` | 291,321 | 0.3358 | 411,387 | 0.3457 |
| 3 | **anonymous field access `obj.fN`** | 199,937 | **0.2305** | 271,425 | **0.2281** |
| 4 | unresolved `regN` | 112,485 | 0.1297 | 160,643 | 0.1350 |
| 5 | **anonymous direct call `sub_<hex>(...)`** | 94,921 | **0.1094** | 149,422 | **0.1255** |
| 6 | **raw pool displacement `poolOff[N]`** | 70,635 | **0.0814** | 84,205 | **0.0708** |
| 7 | guard/null family (null, tag-bit, class-id) | 38,325 | 0.0442 | 56,237 | 0.0473 |
| 8 | generated six-argument `dynamic sub_<hex>` header | 22,102 | 0.0255 | 28,753 | 0.0242 |
| 9 | `smiTag`/`smiUntag` | 15,866 | 0.0183 | 19,789 | 0.0166 |
| 10 | annotated dispatch selector `selN(...)` | 15,111 | 0.0174 | 21,889 | 0.0184 |
| 11 | `receiver: unrecovered` (subset of row 10) | 6,170 | 0.0071 | 7,438 | 0.0062 |
| 12 | `tailCall_<hex>` | 3,070 | 0.0035 | 4,848 | 0.0041 |
| 13 | DFS-only marker union | 1,367 | 0.0016 | 1,819 | 0.0015 |
| 14 | omitted-path marker (subset of row 13) | 1,020 | 0.0012 | 1,266 | 0.0011 |
| 15 | placeholder condition `/* cond */` | 997 | 0.0011 | 1,099 | 0.0009 |
| 16 | unlifted-instruction comment | 727 | 0.0008 | 824 | 0.0007 |

Measured as exact `0/0` floors, explicitly not findings: depth-limited marker,
`// selector:` semantic fallback, unresolved branch/jump marker, `pool[?]`, `poolValN`.
The predicate was exercised across every validated file and matched nothing.

### The scan was checked against the pipeline, not trusted

R13's stub-call figures were wrong because they came from a substring grep rather than a
replay, and that correction is recorded above. So each shape with a counter was replayed
against it:

| replay vs pipeline counter | LocalSend | Immich |
|---|---|---|
| `raw_register_name_refs` | 152,595 vs 152,595 | 213,394 vs 213,394 |
| omitted complex path | 1,020 vs 1,020 | 1,266 vs 1,266 |
| loop back-edges | 347 vs 347 | 553 vs 553 |
| `/* cond */` | 1,098 vs 1,098 | 1,220 vs 1,220 |

Two deliberate disagreements are retained rather than reconciled, because each measures
something different and substituting either would hide the gap:

- Annotated selector **lines** 15,111/21,889 against `dispatch_table_calls`
  14,561/20,938, a **+550/+951** difference. The counter reports pipeline events; the
  line count reports what the reader sees. Of the selector lines, 8,941/14,451 are
  `receiver.selN(...)` and 6,170/7,438 are bare `selN(...)` each carrying
  `receiver: unrecovered`, which is why a `.selN`-only scan would have undercounted.
- Emitted `sub_<hex>(...)` occurrences 94,921/149,422 against reachable IR direct calls
  whose target is an emitted `sub_<hex>` entry, 90,049/143,035, **+4,872/+6,387**, with
  target-level differences in both directions.

A separate control matters more than either: **0 of 94,921 and 0 of 149,422** emitted
`sub_<hex>` targets lack a corresponding output header. They are opaque names, not
missing targets. Malformed IR successor references while replaying reachability: 0 and 0.

The two largest shapes have no pipeline counter at all, so they were bounded structurally
instead of given a fabricated one: 53,271/83,481 explicit synthetic-local declarations
against 306,126/453,062 bearing lines, and 150,039/219,092 `final tN =` declarations
against 160,046/231,698 pipeline `total_calls`. That second pair is deliberately not
presented as equality, a call can return directly or tail-call and need no binding.

### Reading sixteen functions

Counters cannot answer whether output reads as Dart, so sixteen complete functions were
read across the size distribution, eight per sample at the p00 through p99 line-count
quantiles. The judgement is not that the lines contain placeholders; it is that their
combination defeats normal Dart reading.

One line carries three of the top six losses at once:
`sub_d50e6c(poolOff[146216], tmp1)`: the reader can identify neither the method, the
pool value, nor the local's meaning. Another mixes `tmp2.f112`, `poolOff[74088].f8(...)`
and `reg0` within seven executable lines, which reads as a memory-layout trace rather
than object code. The dispatch problem is visible rather than theoretical:
`sel3776(classId(resultTmp3))` states `receiver: unrecovered` on the same line, while the
next call is `t2.sel942(t2)`. At the p99 end, one function opens with
`// omitted complex paths: block 47` and then repeats near-identical call sequences.

The guard family is the clearest case for restraint. Nested field null checks followed by
named error stubs are unreadable as source, but they may implement real Dart error
semantics, and no syntactic predicate distinguishes compiler scaffolding from required
behaviour. It stays a candidate, not a defect.

### The split's residue, quantified

A blob is an emitted record over 200 instructions whose reachable fraction is under 0.20,
by explicit DFS from block 0. R19 attributed these to a leaf-function blind spot.

| | LocalSend | Immich |
|---|---:|---:|
| blobs / emitted functions | 179 / 22,102 (0.81%) | 242 / 28,753 (0.84%) |
| unreachable instructions inside blobs | **117,063** | **122,068** |
| share of all remaining unreachable IR | 37.7% | 32.1% |
| rendered lines belonging to blobs | 8,588 (0.99%) | 9,855 (0.83%) |

So blobs are a substantial **coverage** loss and a negligible **reader-line** cost, which
is why they are not a readability lever despite hiding over 117,000 instructions. Using
block reachability instead of instruction reachability gives 183/246 rather than 179/242,
so the denominator is stated rather than implied.

The concentration is at unreachable-component level, not file level: 5,844/6,359 (91.9%)
and 5,348/5,980 (89.4%) of component heads follow a terminator without the frame-push
predicate, and 5,431/4,762 of those begin with `add`. Only 63/179 (35.2%) and 82/242
(33.9%) of blob files contain even one such component, because one inflated record can
carry several unrelated components. [INFERENCE] This is compatible with the no-frame leaf
blind spot, but it does not prove each component is a leaf function or that splitting
them is safe.

### What this reorders

Frequency is not reachability. The corpus's largest shape is `obj.fN` at 0.23 per line,
and R9 already measured that naming a field requires the receiver's class, which is
identifiable for 0% and 0.0007% of field loads exactly. R21 below closes that lever with
a stronger cause. So the top three shapes by frequency are, today, the three with the
smallest addressable prize, and the two levers that *are* actionable from inside this
PR are rows 4 and 13: register dataflow at 0.1297/0.1350 per line, and structuring at
0.0016/0.0015 per line whose corpus share is small but whose local severity is high,
since an omitted path means emitted code that is not what the function does.

Rejected here with the number that killed each: deleting `tmpN`/`tN` scaffolding
(0.353/0.381 and 0.336/0.346 per line), the `tN` declarations cover 150,039/219,092 of
160,046/231,698 calls, so blind inlining risks duplicating side effects or losing
evaluation order; guard canonicalisation (0.0442/0.0473), below the pool prize and
indistinguishable from real Dart error paths by syntax; blob splitting, 0.99%/0.83%
reader exposure; and the five `0/0` floors, which justify no investment from this corpus.

## R21. The three largest shapes are zero-addressable, and the cause is the model contract

R20 ranked what a reader sees. This section asks what can be *fixed from inside this PR*,
and the answer for the top three shapes is nothing, for a more specific reason than
"metadata is missing".

All figures replay the retained census artifacts from the pinned `2619ec7` baseline.
Model JSON was captured by running `info --json` on both APKs while `which r2flutter`,
`which radare2` and `which blutter` each returned status 1, so no external backend was
installed or on `PATH`.

### What the installed adapter actually carries

The distinction that matters is schema-permits versus adapter-emits, R14's lesson, where
two naming subsystems were dormant because the schema allowed what the real input never
carried. Here the schema is the tighter constraint:

| model property | schema / Rust permits | LocalSend | Immich |
|---|---|---:|---:|
| classes | identity only | **1**: `Global` | **1**: `Global` |
| class field names / offsets | **no field-table key or nested shape is defined** | 0 of 1 | 0 of 1 |
| class method names / entries | no declared contract | 0 of 1 | 0 of 1 |
| function names | required, optional quality kind | 5,800 `name_kind=placeholder`, **0 exact** | 8,329 placeholder, **0 exact** |
| function entry addresses | required | 5,800 unique | 8,329 unique |
| function owner class | required | `Global` for 5,800/5,800 | `Global` for 8,329/8,329 |
| pool entries | base required, semantic optional | 19,872 carved strings, `source=internal`, confidence 0.4; target VA **0**, owner class **0** | 19,156; target VA **0**, owner **0** |
| pool geometry | optional; required to claim hardware indices | **absent** | **absent** |

`ClassInfo` in `schemas/adapter.schema.json:36-48` defines exactly `id`, `name`, `super`,
`lib`, and the Rust deserializer mirrors that surface with no class-field, field-offset or
method-table member. An exhaustive recursive raw-JSON audit of both captured models found
only `classes[].{id,lib,name,super}`: no differently-spelled or nested field list, so this
is not a named-key false negative.

That gives the cause a precise shape. Field naming is not unavailable on this input; it is
**unrepresentable in the current adapter contract**. There is nothing to populate. No
adapter work short of extending the model can reach it, which is exactly why it belongs to
the snapshot-deserialization effort (#42, PRs #74/#86) and not to this seam.

It also supplies the root cause for R14, which measured two naming subsystems emitting
zero on real input and recorded the fact without explaining it. Standard-name
canonicalization derives names from adapter class and library metadata; with one synthetic
class and every function name a placeholder, it cannot fire. R14's dormancy was never a
bug in those subsystems.

### The `this`-relative path, tested and closed

R9 measured receiver class from a *nearby class-id check*: 0% and 0.0007% exact,
0.6%/1.6% under a loose overcounting bound. That leaves a different mechanism untested, a
load off the incoming receiver of a function whose own `owner_class` supplies the class,
needing no check at all.

| `receiver.fN` replay | LocalSend | Immich |
|---|---:|---:|
| all emitted `receiver.fN` lines | 15,949 | 14,911 |
| lines inside model-declared first pieces | **4,863** | **6,400** |
| share of all `.fN` lines | **2.43%** | **2.36%** |
| classes carrying a field table | 0 of 1 | 0 of 1 |
| **actually nameable today** | **0** | **0** |

The scan traversed IR rather than reporting a textual zero: it found 5,104/6,815 `x1`
memory operations in those receiver-bearing declared records, and sampled provenance ties
`receiver.f16`/`receiver.f8` in emitted output to reads from incoming `x1` in the matching
IR.

Two cautions the numbers force. `receiver` is the emitter's naming convention for `arg0`,
**not** proof of an instance receiver, the 2.43%/2.36% is an upper-bound *shape*, not
evidence that a real declaring class exists. And 11,086/8,511 of those receiver lines sit
in split tails, whose owner metadata is intentionally empty: copying a record's owner onto
a swallowed neighbour is the confidently-wrong-class defect R18 warned about, enforced at
`runners/split.rs:201-252`. So the path dead-ends at the contract boundary, not at class
identification. 2.43%/2.36% is worth recording as the forward-looking prize for whoever
lands the field tables.

### Function identity: the addresses overlap, the names are the addresses

| direct-call identity | LocalSend | Immich |
|---|---:|---:|
| visible anonymous calls | 94,921 | 149,422 |
| occurrences whose target equals a model `entry_va` | 90,176 (4,390 distinct) | 142,345 (6,506 distinct) |
| model entry names usable as identity | **0 of 5,800** | **0 of 8,329** |
| visible calls outside the model entry set | 4,745 (3,425 distinct) | 7,077 (5,288 distinct) |

The high overlap is not a naming path. Every model name is a placeholder constructed from
the entry address itself, so substituting it prints the same `sub_<hex>` already there.
The residual out-of-model targets are not an alias opportunity either: R16 measured that
79%/83% of them are interiors of inflated records, and snapping them to a containing entry
would name the wrong function.

`map-symbols` does not cover app code. It reads a stripped/unstripped **ELF pair**, and
automatic ingestion fingerprints `libflutter.so` to resolve an engine cache. Both census
runs had engine ingestion enabled and applied **0 of 0** targets from **0 of 0** loaded
paths while all 5,800/8,329 adapter functions stayed placeholders: a loud zero against a
non-empty engine context, not evidence that app symbols are quietly mapped.

### Pool geometry: the machinery landed; the substrate is absent

Both reports state the boundary directly: 19,875/19,159 pool records, `geometry: null`,
`index_space_authoritative: false`, and an explicit suppression reason that the indices are
not hardware-space.

PR #85, "resolve object pool references in the real index space, add r2flutter backend",
is **already in the evaluated baseline**: its commit `36b4bc5` passes
`git merge-base --is-ancestor 36b4bc5 2619ec7`. The named branch is an 11-commit
continuation beyond it. The implementation emits `pool_geometry` only when r2flutter
reconstructs it and otherwise empties the semantic hints rather than guessing, which is why
geometry is null here: **the mechanism is landed but unfed.** With radare2 and r2flutter
absent from this host, the addressable prize is 0 of 70,635 and 0 of 84,205 `poolOff`
lines, against a conditional ceiling of the full category at 0.0814/0.0708 per line once an
authoritative backend runs.

That ceiling is a maximum, not a forecast: no post-backend hit rate can be claimed without
running it. And it must never be evaluated by installing the backend mid-mission, the
`--adapter-backend` default is `auto`, which resolves r2flutter first, so installing it
silently changes the substrate under every comparison. The measured precedent is blutter:
39,343 function records against internal's 5,800, with `shared_stub_naming` falling from 14
names to 0. A backend is a separate research arm with its own scoring reference.

### The boundary, stated

| lever | addressable today | owner |
|---|---:|---|
| field names (`obj.fN`, 199,937/271,425) | **0** | #42 / PRs #74, #86, needs the contract to gain field tables at all |
| function identity (`sub_<hex>`, 94,921/149,422) | **0** | #42 / PRs #74, #86, needs snapshot function metadata |
| pool displacements (`poolOff[N]`, 70,635/84,205) | **0** | landed in #85; conditional on an r2flutter substrate, evaluated as a separate arm |

So the actionable surface for pseudocode quality inside this PR is emitter and dataflow
work: register recovery at 0.1297/0.1350 per line, and structuring. Everything larger is
gated on metadata this seam cannot produce, and the correct response is to say so with
numbers rather than to ship a plausible-looking name. A wrong field or method name is
strictly worse than `fN`, because `fN` is an honest admission and a wrong name is a claim.

## R22. Repeated regions may end at the enclosing loop (landed)

R16 measured that the structurer's largest decline class is `repeated region contains a
loop header`, and that 37.5%/39.9% of those declines are functions with exactly one loop
header where Dart needs no label at all: `structured.rs` said so in its own comment. That
is now implemented, together with a 2x repeat budget.

### The predicate

A repeated region is admitted when its reachable content, stopped at the region follow,
reaches the **innermost active loop header** and contains no other loop header. That header
is not duplicated; `render_sequence` already renders the encounter as the enclosing
`continue;`. Precisely:

1. there is an active innermost loop header `H`;
2. from the repeated block, successors excluding `follow` reach `H`;
3. traversal stops at `H` and does not count beyond it;
4. every other visited block is not a loop header;
5. the visited region stays within 16 blocks and 96 instructions.

Each clause earns its place by the output a violation produces: repeating a different or
nested header duplicates that loop's body; treating an outer header as the inner `continue`
targets the wrong loop; traversing *through* a header instead of stopping can duplicate the
body or drop an iteration. Every case the predicate does not cover still declines.

The independent replay reproduced R16's single-loop share exactly on the original
unsplit records, 121/323 (37.5%) and 173/434 (39.9%), by mirroring `Regions::build`,
`render_sequence`, `render_loop` and `is_repeatable_region` rather than grepping. On the
post-split corpus the exact traversal predicate admits 290 of 721 and 438 of 1,075 former
loop declines, of which 213/332 finish structuring because the independent budget gates
still apply. So R16's loop-count figure was a proxy, and the exact predicate is narrower.

### Measured on both samples, integrated

Re-measured against the pinned `2619ec7` baseline, both runs
`--adapter-backend internal --split-records --function-scope all`. The "after" column is
reproducible at branch HEAD: a direct run at `750be0c` returns `raw_register_name_refs` 136,378
and 189,696, rendered lines 777,937 and 1,074,372, and file counts 22,102 and 28,753 - identical
to the figures below, so the recipe and the numbers agree at a named commit rather than at "the
integrated tree". Note the counters are computed in-pipeline from the emitted source, so the
trailing-newline fix landed at `33d73e9` does not move them; it changes bytes on disk only.

| | LocalSend | Immich |
|---|---:|---:|
| structured share (replay) | 93.3762% -> **95.8103%** (+538 fn) | 93.0512% -> **95.6631%** (+751 fn) |
| emitted functions | 22,102 -> 22,102 | 28,753 -> 28,753 |
| rendered lines | 867,551 -> **777,937** (-10.33%) | 1,190,169 -> **1,074,372** (-9.73%) |
| `raw_register_name_refs` | 152,595 -> 136,378 (-10.6%) | 213,394 -> 189,696 (-11.1%) |
| per line | 0.175892 -> 0.175307 (-0.33%) | 0.179297 -> 0.176565 (-1.52%) |
| `omitted_path_markers` | 1,020 -> 782 (-23.3%) | 1,266 -> 942 (-25.6%) |
| per line | 0.001176 -> 0.001005 (-14.50%) | 0.001064 -> 0.000877 (-17.57%) |
| `repeated_blocks` | 7,956 -> 21,492 (+170%) | 12,898 -> 30,191 (+134%) |
| strict gate / `disassembly_ratio` | passed / 1.0 | passed / 1.0 |

Emitted function count is unchanged on both, which is the control that matters most: no
function stopped being emitted, so the 10% line reduction is not dropped content.

### `repeated_blocks` tripling is a ruler artifact, not a cost

Read naively the table says duplication rose 170% while emitted text fell 10%, which cannot
both be true. The counter is emitter-asymmetric: `structured.rs` is its only increment site
and `lib.rs` states the asymmetry in its own doc comment: "Bounded by budget; the DFS
fallback's duplication is not." Finding 1 is that the fallback expands a DAG into a tree, and
none of that duplication is counted. Moving functions into the structured path therefore
raises the counter while lowering real duplication.

The duplication-immune measure defined in the Method section settles it. Emittable calls are
byte-identical before and after: the IR did not change, so this is purely emission policy:

| | LocalSend | Immich |
|---|---:|---:|
| emittable calls (denominator) | 112,737 -> 112,737 | 172,256 -> 172,256 |
| emitted call statements | 160,046 -> 146,842 | 231,698 -> 212,916 |
| **inflation** | **1.4196x -> 1.3025x** | **1.3451x -> 1.2360x** |

Inflation moves toward 1.0 by 8.25% and 8.11%. Real duplication fell. Judging this change on
`repeated_blocks` alone would have rejected it.

### Cost, and why 4x was rejected

Bounded duplicate emission among newly structured functions: 77,983 and 92,178 instructions,
median 72/66, worst 3,234/1,648. On the R16 baseline the budget alone structures 121 and 169
functions for 23,519/28,145 duplicated instructions at median 91. 4x reaches 193/269
functions but costs 60,234/132,202 with a worst case of 6,698 and **45,863** instructions in
one Immich function. That tail is pathological, so 2x is the landed point, as a named
constant.

Two tests were added, each mutation-checked by hand: removing the enclosing-header stop
fails the loop test, and restoring the old 8/48 budget fails the budget test.

## R23. Where register loss actually happens, partitioned

R16 established that 98.0%/98.1% of `regN` occurrences have a register with a modelled
writer, so the loss is dataflow. It did not say *which* dataflow. Instrumenting the three
invalidation sites and running full scope on both samples gives the partition:

| cause | LocalSend | Immich |
|---|---:|---:|
| **join merge drops the binding** | **112,904** | **148,849** |
| loop-header merge drops the binding | 5,539 | 6,965 |
| ordinary call clobber | 11,263 | 19,997 |
| other invalidation / pre-existing gap | 23,160 | 37,859 |

Joins dominate by an order of magnitude. The instrumentation perturbs its own output: the
marked totals are +271/+276 against the clean baseline counters, a disclosed 0.18%/0.13%
shift, and the category ordering survives that uncertainty by two orders of magnitude.
Source sites: `structured.rs` clears bindings at joins and around loop headers, `emit.rs`
clears volatile registers at ordinary calls, grounded in
`runtime/vm/constants_arm64.h:520-530,556-560` where ABI-preserved registers are R19-R28.

**What this does not say.** It would be wrong to call 112,904/148,849 recoverable. The mark
records that the state map was conservatively cleared at a join, not that both arms define a
useful value, that the continuation reads it before redefining it, or that the branch is
structured. R11 measured that *exact-agreement* merging recovers one binding in nine
(11.33%/11.84%) and concluded the remainder needs real dataflow rather than a better merge
heuristic, so R11 bounds the merge approach, not a phi, which handles disagreeing arms by
construction. The binding constraint on a phi is liveness, and the negative result recorded
above is precisely what happens when liveness is approximated badly: naive materialisation
added 120,486 lines and *increased* raw register references because its reachability proxy
marked mostly-dead values.

The design that follows is a demand-driven direct-join phi: only where a structured branch's
join is actually read later does each arm assign a synthesised local, with the post-join state
bound to that local. Backward recovery over dominators loses, because it cannot soundly
choose between two non-identical reaching definitions without recreating a phi. Any
implementation must bind a local name and never inline arm expressions across an edge: the
512-character substitution cap exists because a self-feeding expression once reached 110MB on
one line.

### A second failure mode, structural, verified in current code

The design above is not safe at the obvious insertion point, and the reason is in the control
flow of the emitter rather than in any liveness estimate.

`structured.rs:304` sets `cursor = region_follow`, so the join block becomes the **next
iteration's** `id` in the `while let Some(id) = cursor` loop at `:140`. `:305` then resets
`self.state = state_at_branch`, discarding every binding the arms established, before the arm
merge at `:309-310` runs at all. The next iteration reaches `:179`, finds `is_join`, and at
`:185-189` performs a **full-predecessor** merge on the same block the arm merge just handled.
So every if/else join is merged twice, back to back.

Follow a register that needs a phi. It is written on at least one arm by definition, the arms
are predecessors of the join, so it appears in `written` at `:188` and `merge_state_at_join`
drops its binding at `:189` - before `render_block_body` renders the join body at `:192`. A phi
installed at `:309` is therefore destroyed before anything reads it: the arm assignments are
emitted, so lines grow, and the join still prints `regN`.

This is **additive to** the recorded cause of the earlier negative result, not a rediagnosis of
it. That section attributes its failure to an over-approximating read set, 30,736 of 40,186
generated locals never read. Those numbers rule out a clean substitution: 23.5% *were* read, so
destruction cannot have been universal, which means the earlier attempt either installed at a
different point or was not uniformly affected. Its insertion point is not in the record, so no
claim is made about it. What is verified here is that the current code destroys a binding
installed at `:309`, and that this would survive perfect liveness.

Three shapes avoid it, and an implementation must choose one deliberately: install at the
`:181-190` join-block site, where the destroying merge is the one being replaced; record pending
phi bindings keyed by join block and re-establish after `:189`; or make that merge phi-aware
with an exemption in the shape of the existing `pinned_value`/`x15` carve-out at `:555`.

Three further constraints, each verified: a phi must fire only where the join's **complete**
predecessor set equals the arms emitted into, since `:309` sees only `&arms` while `:185-189`
sees every predecessor, and a third incoming path would leave the local unassigned; loop headers
are ineligible because `render_loop` at `:327-337` merges twice around the body and the back-edge
value is not rendered at the header; and a phi must restore `reg_values` only, because
`merge_state_at_join` also clears `last_cmp` and `selector_hints` at `:553-560`, both
path-sensitive, and resurrecting them yields wrong conditions or wrong dispatch selectors.

The negative result also records a ranking trap worth repeating: do not order candidate
registers by name. `x10` sorts before `x2`, and the Dart argument registers are x1, x2, x3, x5,
x6, x7, so alphabetical order spends the budget on the least valuable registers first.

No prototype landed, so no before/after is claimed. The partition is the result: it says the
next attempt belongs at joins, and it says what would make it fail.

## R24. The reproducibility gate caught a real defect

Every figure in this document depends on the pipeline being deterministic, and that premise
had never been tested at full scope. Testing it found a genuine bug.

Two cold processes over LocalSend at `--function-scope all` produced **different emitted
text** with a **byte-identical `quality.json`**. It is intermittent: three observed runs gave
two distinct corpora, 2-1. At `--max-functions 200` it never reproduced, which is why earlier
spot checks passed.

The cause is a partial order. `renames` is a `HashMap`, so `into_iter()` yields a
seed-dependent order; the sort key was identifier **length only**; and Rust's `sort_by` is
stable, so equal-length names kept that seeded order. The renames are then applied as
sequential textual substitutions, where order matters whenever two overlapping renames have
the same key length. Counters were unaffected because none of them depends on which rename
won, which is exactly why this survived unnoticed.

The fix is a total order: length descending, then the key itself. Keys come from a map and
are therefore unique, so length-then-key admits **exactly one permutation** regardless of what
`into_iter()` produced. That is the argument the fix rests on: the re-run of the full-scope
gate on both samples is corroboration, not proof, because an unfixed build passes a single
A/B comparison about two thirds of the time.

The fix is output-neutral in the common case: the post-fix LocalSend corpus hashes identical
to the pre-fix majority permutation, so it removes the divergence without churning output. A
test constructs both insertion orders directly and asserts the sorted result is identical, so
it fails deterministically without the tie-break rather than flaking.

### The class had a second instance, and here is the audit

Fixing one site did not close the class. A second partial order in the same file was found later,
by a front comparing two builds carefully: `extract_minus_one_aliases` collected candidates from a
`HashMap` and sorted them with `sort_unstable_by` on **frequency alone**. Worse than the first
case - an unstable sort does not even preserve input order for equal keys - so two identifiers
sharing a count were emitted in an arbitrary order. It surfaced as `reg8Minus1` and `reg9Minus1`
swapping declarations between runs in one function, with every counter identical. Fixed the same
way, frequency then lexicographic, with a test that constructs both insertion orders and fails
deterministically without the tie-break.

Then the whole class was audited rather than waiting for a sixth discovery, and the rule that
emerged is worth more than the fix:

> **A partial-order sort is a nondeterminism hazard only when its input order is itself
> nondeterministic.** `sort_unstable` is deterministic for a fixed input sequence; it merely fails
> to preserve the relative order of equal elements. So the hazard is precisely a partial comparator
> applied to a sequence derived from `HashMap`/`HashSet` iteration.

Every sort on an output-affecting path, judged by that rule:

| site | input source | verdict |
|---|---|---|
| `naming.rs` rename pairs | `HashMap` | fixed earlier, total order |
| `naming.rs` alias candidates | `HashMap` | **fixed here**, total order |
| `naming.rs` stack-slot and pool-literal candidates | `HashMap`, then full `.sort()` on `Vec<String>` | total, safe |
| `lib.rs` symbol ranking | `HashMap`, comparator ends in `a_name.cmp(b_name)` | total, safe |
| `helper_flow/inlining.rs` removal ranges | `scan_helpers(&self.lines)`, line order | deterministic input, safe |
| `symbol_map/elf.rs` section table | ELF headers in file order | deterministic input, safe |
| `control_flow/regions.rs` branch targets | full `sort_unstable()` | total, safe |

Two defects, both in the same file, both from the same habit of sorting a hash-derived list by one
key. Nothing else in the audit needs changing, and the rule above is what makes that a conclusion
rather than an assumption.

Consequence for the figures above, stated narrowly. Everything derived from `quality.json` is
unaffected, because those values were byte-identical across the divergent pair, and inflation
is unaffected because identifier renaming changes no call count.

Exactly two rows of R20's census are conditional on the pre-fix permutation: **row 1**
(synthetic locals, 306,126/453,062) and **row 2** (call temporaries `tN`,
291,321/411,387). Those are the shapes the rename pass rewrites. Rows 3 through 6,
`obj.fN`, `sub_<hex>`, `poolOff[N]` and `regN`, are emitter-generated tokens that never
enter the rename map, so they are unaffected, and they are the rows R21's zero-addressable
conclusion and the actionable-surface ranking actually rest on. Rows 1 and 2 are also the two
already rejected as levers, with the reason recorded. So no conclusion in R20 or R21 depends
on a figure the defect could have moved.

## R25. The phi prize, bounded by topology rather than liveness

> **PROVISIONAL. Every count below is unverified and must not be cited yet.** Named exactly, so
> the scope of the retraction is not left vague: the raw live population 32,850/58,316; the
> eligible 7,519/12,466 across 5,351/9,241 sites; the declined-incomplete 25,326/45,846; the
> loop-header 5/4; the arm-write bounds 5,493/9,133 and 2,026/3,333; and the 1.39-1.41 event-to-site
> multiplicity. The front that produced them has since stated they came from a broader IR proxy
> rather than a trace of the actual emitter, and a second front instrumenting the real renderer
> reports conflicting numbers. The instrumented measurement is authoritative, because every one of
> these quantities is about what the render walk does.
>
> **Two controls quoted below do not do the work they appear to.** "The partitions close exactly"
> is a tautology: the three categories are defined as a partition of the raw set, so they sum by
> construction regardless of whether any classification is right. And the structural gate matching
> the published structured share confirms only that the replay identifies *which functions are
> structured* - not that it identifies joins, liveness or predecessor sets. Neither could have
> failed, so neither is evidence.
>
> **What does not depend on any count.** The claim that an arm-site binding is always destroyed is
> deductive, not statistical: a register needing a phi is by definition written on an arm, arms are
> predecessors of the join, so it is in the full-predecessor `written` set at
> `structured.rs:185-189` and is dropped at `:189` before the join body renders at `:192`. That
> holds whatever the population turns out to be, and it is the finding that changed the design.
> The identification of predecessor completeness as the dominant gate is also directional rather
> than magnitude-dependent. The units, denominator and text-budget reasoning stand on their own.
>
> **Superseded by R26 for the population**, which gives the emitter-true net figures of 48,099
> and 73,307 candidate sites. The criterion itself was never evaluated: only an upper bound proved
> obtainable, and it was inconclusive. Retained rather than deleted because the correction had to
> be measured against this reasoning, and because the topology constraints it establishes are what
> still bound the design.

### Why the proxy diverged, from the front that built it

Recorded because a retraction that says only "the numbers were wrong" teaches nothing, and
because each item names a trap the replacement measurement has to avoid.

1. **It skipped the emitter state, which biases up.** The proxy defined a candidate as the raw
   definitions reachable from the arms intersected with conventional CFG live-in at the join. The
   real loss is `state.reg_values` intersected with `written` at `structured.rs:553-556`, after the
   branch snapshot and reset discipline at `:214-251` and `:304-310`. Neither `state_at_branch` nor
   either arm's ending `LiftState` was reconstructed, so a raw arm write counted even where the
   lifter left no usable binding - an unmodelled or invalidating operation, a later clobber, an
   over-cap expression, a stub-specific effect.
2. **Liveness was static may-live, not emitted-read liveness.** A block-CFG fixed point over parsed
   machine-register operands, rather than actual `lookup_reg`/`capped_reg_value` calls on rendered
   paths. Any-path continuation reads and parser false reads bias high; incomplete mnemonic and
   operand recognition biases low. The net magnitude is not defensible in either direction.
3. **Arm availability was never measured.** "Every arm has a modelled write" is a syntactic subset:
   an unwritten arm can legitimately retain the branch-entry binding, which is a false negative,
   and a write can be invalidated before the arm ends, which is a false positive. Only arm-end
   `reg_values` before the reset at `:305` resolves it.
4. **The event unit was wrong.** It enumerated `region_follow` branch events - the `:309` view -
   rather than each generic join-block iteration at `:179-190`. A join can be missed when it is not
   a branch follow, and can be entered from several branch or repeated-region contexts, so the unit
   is not denominator-compatible with R23's emitted-reference provenance.
5. **What survives is a source invariant, not a calibrated count.** For a phi binding installed at
   `:309`, the arm write is included in the complete-predecessor traversal at `:185-189` and
   `:514-539`, and `merge_state_at_join` then removes the matching `reg_values` entry before `:192`.
   That is an argument from the code, so it holds independently; the *number* killed still needs
   renderer instrumentation.

The authoritative unit follows from item 4: trace the actual arm-end state and any pending phi,
then the actual generic merge before and after, then the actual post-join resolved reads. That is
what the replacement measurement traces.

R23 named joins as the dominant register loss. R24's addendum showed the obvious insertion
point cannot work. This sizes what a correct one would recover, by replaying the actual
`render_sequence` branch-follow events over HEAD's IR on both samples. Controls: 22,102/22,102
and 28,753/28,753 IR files processed, and the replay's structural gate accepted
21,176/22,102 (95.810%) and 27,505/28,753 (95.660%), matching the published post-split
structured shares rather than passing a silent zero.

A candidate is a `(structured branch follow, canonical register)` binding event, live when the
register is read in the join block or a reachable continuation before a modelled redefinition.
It uses the same canonical-register convention and write set as `structured.rs:514-539` and
excludes pinned registers and `x15` exactly as `merge_state_at_join` does.

| replayed binding events | LocalSend | Immich |
|---|---:|---:|
| raw structured branch-follow live candidates | 32,850 | 58,316 |
| declined: incomplete predecessor set | 25,326 | 45,846 |
| declined: loop header | 5 | 4 |
| **eligible under both preconditions** | **7,519** | **12,466** |
| distinct eligible `(join, register)` sites | 5,351 | 9,241 |
| strict policy: every arm contains a modelled write | 5,493 | 9,133 |

The partitions close exactly on both samples: 7,519 + 25,326 + 5 = 32,850 and
12,466 + 45,846 + 4 = 58,316.

So the eligible population is **7,519/12,466 binding events** across **5,351/9,241 distinct
`(join, register)` sites**, and under the sound arm policy - decline the whole phi unless every
incoming arm can assign, because an unassigned predecessor reaches code that reads the local -
**5,493/9,133 events**.

**Those are binding events, not emitted `regN` occurrences, and the two must not be divided into
each other.** The replay's own method string says so: "binding events, not emitted regN
occurrences". A dropped binding causes a `regN` wherever that register is subsequently read, so
one event may correspond to zero, one, or several emitted occurrences. Setting 7,519 against
HEAD's 136,378 references therefore yields 5.51% as a ratio of *different units*, which bounds
nothing: the recoverable-occurrence count could be larger or smaller. It is quoted here only as
an order-of-magnitude locator, and the honest statement is that **the occurrence-level prize is
unmeasured** - attributing occurrences to the event that caused them needs the emitter, not an IR
replay.

Events exceed sites by roughly 1.39-1.41x throughout (raw 32,850/23,603 and 58,316/42,936;
eligible 7,519/5,351 and 12,466/9,241). That multiplicity is a property of the render walk, which
can visit a block more than once, so it is not evidence that a phi local would be read 1.4 times.

R23's join-drop marks are deliberately **not** used as a denominator here. They were measured at
`2619ec7`, before the structurer moved 538 and 751 functions into the structured path and took
`raw_register_name_refs` from 152,595/213,394 to 136,378/189,696. Dividing a HEAD numerator by
that pre-structurer total would mix commits in exactly the direction that moves join counts, and
this document's own rule is that a result whose denominator moved without explanation is not
evidence. A HEAD join-drop total would require re-running R23's instrumentation, which was not
done.

### The gate is predecessor completeness, not liveness or agreement

Liveness removes far less than expected: 32,850/58,316 of the branch-follow candidates are live.
The reduction to 7,519/12,466 comes almost entirely from one topology rule, which
declines **25,326/45,846 events, 77.1%/78.6%** of the live population, because the join has a
third-or-more predecessor that is not one of the two emitted arms. Loop headers remove 5 and 4.

This also settles the relationship with R11, which measured *exact agreement* at 11.33%/11.84%.
Agreement is not the gate. A phi reconciles disagreeing arms by construction, and the live
population is larger than R11's rate; what bounds it is emitter topology.

### The double merge, confirmed at 100%

**Every** raw branch-follow live event - 32,850 of 32,850 and 58,316 of 58,316 - is in the
generic full-predecessor `written` set at `structured.rs:185-190`. So a binding installed at the
arm site `:309-310` is killed before `render_block_body` runs at `:192`, without exception. The
structural failure mode is not a tendency, it is total.

### What this implies for the design, and the open question

The complete-predecessor rule is an artifact of *where* the phi was conceived. The arm site at
`:309` knows only two arms, so any third predecessor forces a decline. But the join-block site at
`:185-190` - which the 100% kill result already forces the implementation to move to - computes
the **full predecessor set** itself.

So the same change that is mandatory for correctness may dissolve the dominant gate. If a phi at
the join-block site can assign on every predecessor rather than two arms, the addressable
population rises from 7,519/12,466 binding events toward the raw 32,850/58,316, a factor of
4.4x and 4.7x. Stated as a factor rather than a share of raw register references, because those
are occurrences and these are events. That factor is the difference between a marginal change and
the largest single quality win available in this seam.

That upper bound will not survive contact, and "is there room for an assignment" is too loose to
measure. The computable predicate is **emitted text order**: for each declined event, is every
extra predecessor rendered *before* the join? The emitter walks a cursor and marks
`structured_emitted`, so a predecessor rendered after the join - a back-edge source, or a block
the region tree orders later - can never assign before the join body reads the local. That is the
loop-header exclusion generalized, and it is the real limit.

Two wrinkles will invalidate the measurement if left unmodelled, and both are verified in code:

- **A block can render more than once.** `structured.rs:150-163` re-renders an already-emitted
  block when `is_repeatable_region` admits it, which is where `repeated_blocks` increments. An
  assignment placed in a repeated predecessor is emitted in *every* copy, so its line cost
  multiplies, and if the copies feed different joins the binding has to be per-copy rather than
  per-block.
- **This is edge placement, not block placement.** A predecessor with more than one successor
  cannot carry an unconditional `tN = ...` intended for only one of them. In SSA terms it is a
  critical edge and needs splitting, which in structured output means duplicating a tail or
  introducing a flag - a strictly harder construction than the two-arm case.

So the generalized population must be reported in three parts, not folded into one headline:
extra predecessors rendered before the join and single-successor (the tractable set); rendered
before but multi-successor (needs edge splitting); and rendered after the join or repeated (out of
reach without a different emitter shape).

No prototype was built and no gain is claimed. Cost, if built: one synthesized local per
eligible site plus one assignment per participating arm, so roughly 5,351 and 9,241 declarations
with at least twice that many assignments - line growth of order 2%, moving text from the `regN`
category into the synthetic-local category. That trade needs the absolute counter, not a per-line
ratio, because a phi adds lines by construction and the ratio would flatter it.

### The phi converts noise rather than removing it, and that is the decisive axis

Scoring this on `raw_register_name_refs` alone hides what the change actually does to a reader.
A phi emits `final tN = ...` on each participating arm. That is R20 row 2, `tN` temporaries, the
corpus's **second largest** shape at 0.336/0.346 per line. `regN` is row 4 at 0.1297/0.1350. So
the change grows the second-biggest noise class in order to shrink the fourth.

The asymmetry is what matters. A `regN` is an opaque token **inside an existing line**. A `tN`
binding is a **whole new line** of scaffolding. Removing one token by adding two lines is more
total text for the reader, and the added text is itself a shape this census already classifies as
noise.

The cost side is arithmetic: at least two arm assignments per site, so at least two added lines
per site, across 5,351 and 9,241 sites. The **benefit** side is not yet measured. It requires the
number of emitted `regN` occurrences each site would remove, and the only figure available -
events exceeding sites by 1.39-1.41x - is render-walk multiplicity rather than reads per register,
as stated above. So it cannot stand in for occurrences recovered.

This does not automatically kill it. A `tN` bound to a traceable expression carries information an
opaque `regN` does not, so it is an information gain rather than a relabel. But it must be scored
on that axis, with the budget declared before the result is seen:

> **Pre-registered kill criterion.** The phi must remove more opaque `regN` tokens than the number
> of lines it adds - a recovered-per-added-line ratio of at least 1.0 - or it is recorded as a
> negative result and not landed. The criterion is registered now, before the input to it is
> known, precisely so the threshold cannot be moved once the result is in. **Its numerator is
> unmeasured**: no prediction of pass or fail is made here, because the only ratio in hand
> measures something else.

[INFERENCE] The generalized join-block design is not obviously better on this axis and may be
worse: more predecessors means more assignments per site, so unless multi-predecessor joins carry
proportionally more reads, the ratio degrades as the population grows. The references-per-site and
arms-per-site distributions for the raw 32,850/58,316 population are unmeasured and decide it.

So the next measurement is not an implementation. It is the text budget: recovered references per
added line, for the narrow and generalized designs separately. If both come in below 1.0, the
honest outcome is a recorded negative with numbers, which the stop rule explicitly permits. It
would be the second negative this lever has produced, and that pattern is itself the finding:
register loss at joins is real, dominant, and may simply not be worth paying for in text.

## R26. The phi is dropped: five defects across three attempts

R25 registered a kill criterion for the join phi - recovered `regN` occurrences per added line
must reach 1.0 - and left its numerator unmeasured. Three fronts attempted to measure it, hitting
five unrelated defects between them, and
the outcome is a **recorded negative: the prize is unmeasurable at acceptable cost, and the phi is
not built.**

### The one solid number

Instrumenting the real emitter at the generic join site gives the first emitter-true candidate
population, which supersedes R25's retracted figures:

| candidate sites at the generic join | LocalSend | Immich |
|---|---:|---:|
| raw, as captured | 52,772 | 78,118 |
| x29 (frame pointer) leaked in | 4,673 | 4,811 |
| **net** | **48,099** | **73,307** |

The leak is worth its own note. `merge_state_at_join` exempts pinned registers and `x15` from
clearing, but the arm-end capture applied a different exclusion set, so the frame pointer entered
the population at 8.9% and 6.2%. R11 recorded the identical artifact inflating its own agreement
rate to 23.65%/24.51%, and observed that `x29` is never emitted as `regN`. That holds here and is
refined: canonical `x29` and `indirectTarget29` reads are exactly zero, while the `framePointer`
spelling does appear 350 and 590 times. So the register is emitted, just never under a name that
claims it is an unrecovered value.

### The criterion could not be evaluated

Only an upper bound was obtainable, because the read hook counts occurrences across the whole
function with no line provenance, so it cannot attribute a read to the join that dropped the
binding. Arm text before the join and unrelated later positions all count:

| | LocalSend | Immich |
|---|---:|---:|
| function-wide occurrences (upper bound on numerator) | 353,881 | 395,259 |
| added arm lines (denominator) | 117,172 | 192,706 |
| **recovered-per-added-line** | **<= 3.020** | **<= 2.051** |

An upper bound is decision-relevant in one direction only. Below 1.0 it would have killed the
design outright, since the true ratio cannot exceed the bound. At or above 1.0 it proves nothing,
and both samples land above. So the criterion is **inconclusive**, which under the pre-registered
rule is the same outcome as a failure to measure: the phi is dropped rather than built on a number
nobody has.

Controls, each capable of failing: total reads nonzero and not equal to the site count
(353,881 against 48,099; 395,259 against 73,307); `x0` reads nonzero at 121,596 and 158,995; reads
broken down per rendered spelling so a zero for any one form is visible rather than folded into a
total; and instrumented against clean `quality.json` side by side.

### Five defects, five different reasons

The durable finding of this front is not a ratio. It is that one quantity resisted three
measurement attempts for five unrelated reasons, and that every failure produced a number that
looked plausible until it was checked:

1. **Wrong unit.** An IR proxy counted raw arm-reachable definitions intersected with static
   CFG live-in, never reconstructing `state_at_branch` or arm-end `LiftState`. Its counts were
   published and retracted.
2. **Never run.** The corrected instrumentation was designed and compiled but the pass ended
   before it executed.
3. **Wrong token spelling.** The read hook searched the canonical `xN` key, but it runs at
   `lib.rs:282`, after `apply_name_and_type_hints` at `:280`, so every one of 52,772 sites came
   back `unread` - a clean all-zero result from a hook that could not observe the thing.
4. **Incomplete spelling set.** `named_register_alias` renders 30 as `returnAddress` and 29 as
   `framePointer`, and `named_indirect_target` renders 30 as `dispatchTarget` and 2 as
   `cachedTarget`. Searching `reg{n}` alone would have produced a low but nonzero ratio that read
   as a measurement rather than a miss.
5. **No line provenance.** The hook scans every final line of the function, so even with correct
   spellings it yields function-wide occurrences rather than post-join attributable reads. This is
   the one that stopped the measurement, and closing it needs a new hook rather than a fix.

Defect 3 is the instructive one for method: an all-zero result passed every control in place at
the time, because none of those controls could fail. The nonzero-total control that would have
caught it was added only after the fact.

### What would change this

Per-join read attribution needs the emitter to record, for each rendered `regN`, which join
dropped its binding - line provenance the current hook does not carry. That is a fourth
instrumentation design, and three rounds on one counterfactual is already past the point of
diminishing returns for a prize whose eligible population is gated by the topology constraints in
R25.

The alternative is to stop measuring the counterfactual and build the cheaper design instead. An
annotation adds no lines, so the criterion that could not be evaluated here does not apply to it
at all, and its coverage is directly observable on real output rather than by proxy. That is the
path taken next.

## R27. An open anomaly in `placeholder_cond_markers`

Recorded rather than resolved, because it concerns the ruler and a counter nobody should trust
silently.

A full-counter reference measured at branch HEAD, main-tree binary, both samples with
`--adapter-backend internal --split-records --function-scope all`:

| counter | LocalSend | Immich |
|---|---:|---:|
| `raw_register_name_refs` | 136,378 | 189,696 |
| `raw_arg_name_refs` | 0 | 0 |
| `placeholder_cond_markers` | **716** | **716** |
| `omitted_path_markers` | 782 | 942 |
| `loop_backedge_markers` | 266 | 460 |
| `block_helper_refs` | 0 | 0 |
| `placeholder_ifs` | 1,178 | 840 |
| `unresolved_cf` | 0 | 0 |
| `repeated_blocks` | 21,492 | 30,191 |
| `total_calls` | 146,842 | 212,916 |

Every other counter differs between the two samples, as it should - the corpora are 22,102 and
28,753 functions from different apps built by different SDK generations. `placeholder_cond_markers`
is **identical at 716**, and that is implausible as a coincidence.

It is also a change. R20's census measured the `/* cond */` shape at 1,098 and 1,220 through the
same counter at `2619ec7`, before the structurer landed - different values, as expected. After the
structurer they converge to the same number on both samples.

**Three readings**, and the third is the one this framing initially missed:

1. Coincidence: the structurer reduced the shape to app-specific populations that happen to
   coincide at 716.
2. Counter defect: it has become insensitive to the corpus, saturating or reading one input twice.
3. **Shared code.** Both APKs embed the same Dart runtime and core libraries. If the structurer
   resolved the *app-specific* placeholder conditions, the residue would be SDK-resident code -
   identical in both binaries **by construction**, neither coincidence nor defect.

Reading 3 fits the data best. R20's pre-structurer figures of 1,098 and 1,220 differed exactly as
app-specific code would, and nothing else measured here produces exact cross-sample equality.

**The discriminator is not a count.** Recounting the literal and comparing against the counter
validates counting *fidelity* only, and every reading above predicts that agreement, so it is a
control that cannot fail for the question it is attached to - the defect class this document
codifies elsewhere. The check that discriminates is comparing the 716 **emitted bodies** across the
two samples, normalised for names and addresses which necessarily differ. Same shapes means reading
3, and it converts an anomaly into a ceiling statement in R21's style: residual placeholder
conditions are SDK-resident, so no app-side emitter work reduces them and the lever is closed with
a reason. Different shapes means readings 1 or 2, and a counter returning the same value for two
dissimilar inputs would then be worth chasing properly.

**The discriminator was run.** Both corpora regenerated at HEAD, every function containing the
marker collected, and its lines and whole bodies normalised for the things that must differ between
two apps - addresses, `sub_` names, synthesised numbering, pool indices, field offsets, literals:

| | LocalSend | Immich |
|---|---:|---:|
| marker occurrences | **716** | **716** |
| functions containing one | 193 | **214** |
| distinct normalised marker lines | 89 | 37 |
| distinct normalised function bodies | 179 | 191 |
| shared normalised marker lines | 26 | 26 |
| shared normalised bodies | 29 | 29 |

What this settles:

- **Counting fidelity is fine.** A direct scan returns 716 on both, matching the counter exactly, so
  reading 2 has no support at the counting layer.
- **Reading 3 is refuted in its strong form.** Only 29 of 179 and 191 normalised bodies are shared,
  about 15%, so the residue is not one identical block of SDK-resident code appearing in both
  binaries. The functions carrying these conditions are app-specific.
- **But it is partly right at the shape level.** 26 of Immich's 37 distinct marker lines - 70% - also
  occur in LocalSend. So the *conditions* are largely shared idioms while the *functions containing
  them* are not. That is a meaningful finding on its own: the residual placeholder conditions are a
  small vocabulary of recurring shapes, which is what a targeted fix would have to address rather
  than a long tail.
- **The exact equality stays unexplained.** 716 arises from 193 functions on one sample and 214 on
  the other, with largely different bodies, so it is not equality by construction. The remaining
  candidate is coincidence, which is unsatisfying but is what the evidence supports once counting
  fidelity is confirmed and shared-code is refuted.

Left open deliberately, with the measurement recorded so nobody repeats it. The counter gates
nothing and no result here rests on it. The 70% line-shape overlap is the part worth acting on if
`/* cond */` ever becomes a lever.

## R28. Annotate instead of materialise (landed)

R26 dropped the join phi as unmeasurable. This is the cheaper design that replaced it, and it
landed because it is measurable directly: building it *is* the measurement, so no counterfactual
was needed.

### The design, and why it sidesteps every obstacle

Where a join drops a binding, the register keeps its `regN` spelling and the candidate values are
appended as a comment on the same line:

```dart
final t1 = sub_6c1b28(reg1 /* = -1 | 1 */, reg2, poolOff[55384]);
```

Arm-end values are captured into a side table keyed by `(join, canonical register)` and inserted
**after every analysis and rewrite has run**. That ordering is the whole design. Nothing is bound,
so the generic full-predecessor merge has nothing to destroy - the 100% kill in R25 is irrelevant
rather than something to work around. No assignment is placed in any predecessor, so predecessor
completeness, loop headers and critical edges do not apply. And because insertion happens last,
the annotation cannot influence naming, aliasing, compaction or type inference; it is invisible to
analysis and visible only to a reader.

### Honesty is enforced, not assumed

A list renders `= ` only when the captured arms are exactly the join's predecessors. Otherwise it
renders `possible (non-exhaustive):`, which is the overwhelming majority:

| | LocalSend | Immich |
|---|---:|---:|
| complete (`= `) | 158 | 253 |
| explicitly non-exhaustive | 4,911 | 7,854 |
| **total annotations** | **5,069** | **8,107** |
| share of raw register references | **3.72%** | **4.27%** |

Three percent complete is the honest shape of this problem, and printing `= ` on the other 97%
would have been the confidently-wrong-claim defect R18 and R21 both refuse.

Candidates that are themselves unrecovered are rejected outright, so a reader never gets one
unknown explained by two more. Enforcing that required a shared helper covering **every** spelling
a bare register renders as: canonical `xN`, the `named_register_alias` form (`regN`, or
`framePointer` for 29 and `returnAddress` for 30), and the `named_indirect_target` form
(`dispatchTarget` for 30, `cachedTarget` for 2, `indirectTarget{n}` otherwise).

`argN` is deliberately **not** in that set, and the distinction is worth recording because an
earlier draft of this section said it was. `argN` is rewritten to a named form by
`apply_name_and_type_hints` for every argument register, so it does not survive into final output -
`raw_arg_name_refs` measures 0 on both samples. The reason `argN` came up at all was comment
contamination before the counters were taught to read the code span, which is a different problem
and is fixed separately. Four separate defects on
this branch came from consumers hand-rolling partial subsets of that list, so it now exists once.

### Neutrality, measured independently

Verified by the parent on the integrated tree, not taken from the implementing front's report:

| contract | LocalSend | Immich |
|---|---|---|
| rendered lines | 777,937 = reference | 1,074,372 = reference |
| emitted files | 22,102 = reference | 28,753 = reference |
| `raw_register_name_refs` | 136,378 = reference | 189,696 = reference |
| `raw_arg_name_refs` | 0 = reference | 0 = reference |
| unbalanced comment lines | 0 | 0 |
| longest physical line | 2,660, unchanged | 2,490, unchanged |
| strict gate / `disassembly_ratio` | passed / 1.0 | passed / 1.0 |
| two cold processes | byte-identical | byte-identical |

**One divergence was observed and is not fully explained.** The first post-integration gate run
showed the two Immich processes producing different corpora. Every subsequent attempt to reproduce
it failed: **17 full-scope Immich runs since are byte-identical**, including the clean gate above.

The leading explanation is not a code defect. That gate run had `/tmp` at 81% capacity with stderr
redirected to `/dev/null`, so an `ENOSPC` truncation during the second write would have produced a
differing hash silently. The re-run with headroom and stderr captured returned `rc=0`, empty
stderr, and full file counts on both processes. But the divergent artifact was deleted before it
could be diffed, so this is the best available explanation rather than a proven one.

What is independent of that: every path in the annotation feature was verified ordered by
inspection - `regs.sort()` before candidate capture, provenance ordered over an arm slice,
`join_candidate_regs` sorted and deduped, anchors iterated as a `Vec` in render order, and the
insert sort total because dedupe makes `(line, token_start)` unique. Two hash-order defects found
earlier on this branch were both in `naming.rs` and both are fixed.

Recorded here rather than left in a summary, because R24 makes reproducibility this branch's
headline claim, and the next person to see a divergence should start from this evidence rather
than from zero. If it recurs: **retain both corpora immediately** - losing the artifact is what
turned a five-minute diff into an hour of unsuccessful hunting.

Zero physical lines added, because an annotation is characters on a line that already exists. The
text cost is 0.47 and 0.59 bytes per rendered line.

The counters read the code span so annotation text cannot inflate them. That is a **deliberate
ruler change**, justified because the counters are meant to measure emitted code and because it is
a bit-for-bit no-op on all pre-feature output, which contains no annotation spans - a property
proven against a pre-annotation build rather than argued.

### Corrections to the implementing front's figures

The front's report quotes 7,124 and 12,602 annotations at 5.22% and 6.64%, with the longest line
growing to 2,998 and 3,000. Those figures predate its own same-token de-duplication fix, which
limits one annotation per rendered register site; before it, a single `regN` could take several
comments in sequence. The parent's independent measurement on the integrated tree gives 5,069 and
8,107 at 3.72% and 4.27%, with the longest line unchanged - because the stacked annotations were
what had pushed those dense lines to the cap. The lower figures are the landed behaviour.

Two hypotheses were tested and rejected before that cause was identified: that the alias-order fix
ate the difference - only about 300 lines carry a `Minus1` alias at all, worth roughly a dozen
annotations - and that annotations were failing to attach, which `raw_register_name_refs` being
exact rules out, since the register population is unchanged.

### What this is not

It does not recover a value, materialise a phi, or bind a local. **96.3% and 95.7% of raw register
references remain bare.** The residual is the liveness problem R26 could not size. "100% useful"
is conditional on a stated syntactic rule - a candidate must be a field access, call-shaped
expression, or literal - and measures candidate *shape*, not the semantic correctness of the
lifted expression behind it. Six sites were traced by hand to the arm writers in the emitted IR
and all checked out, which is a spot-check rather than a proof.
## R29. Round 2 annotation coverage, published per loss site

> ## SUPERSEDED. Every count in this section describes a corpus whose annotations named identifiers that did not exist.
>
> **Do not cite a number from R29.** These counts were measured before R33 found that 3,022 annotation
> values on LocalSend referenced identifiers appearing nowhere in their file - `argN`, `local_mN`,
> `local_pN`, all rewritten by the naming pass after the candidate was captured. So this section counted
> annotations a reader could not resolve, and its headline gain was substantially an artifact of that.
>
> Corrected figures, both samples, full scope, after the fix:
>
> | | LocalSend | Immich |
> |---|---:|---:|
> | exhaustive `= ` | 197 | 283 |
> | non-exhaustive | 1,850 | 3,135 |
> | loop-entry | 321 | 387 |
> | pre-call | 6 | 12 |
> | **total** | **2,374** | **3,817** |
> | exhaustive share of join annotations | **9.6%** | **8.3%** |
> | dangling identifier references | **0** | **0** |
>
> Against R29's published 4,369 and 7,246 that is a second, larger fall, and the two have different causes
> which must not be merged into one narrative: the first was the usefulness whitelist tightening, the
> second is rejecting annotations that named nothing.
>
> **R29's headline claim inverts.** It reported the exhaustive share rising 3.1% to 19.2% / 21.0%; the
> honest figures are 9.6% and 8.3%, because exhaustive annotations were hit hardest - down 74% against
> 42% for non-exhaustive. The "complete" claims this section celebrated were disproportionately the ones
> naming dead identifiers.
>
> The two ledger tables below have **stale counts**, not stale integrity assertions, and the distinction
> matters. Their `reconciled true` and `audit_rows_vs_corpus_scan 0` rows are **re-established**: both
> validators were re-run against a fresh audit after the fix and report zero across all seven rules and
> all seven counts. Only the volume columns await regeneration, and they are deliberately not hand-edited
> because their columns derive from uncapped and control builds - hand-editing a provenance artifact is
> what left a stale `site_key` in a fixture earlier in this round.
>
> R33 carries the method, the per-class attribution and the verification. R34 changes which name a local
> is given and **does not move these counts** - measured, both totals identical at 2,374 with the
> exhaustive count identical at 197. So the 2,368 against 2,374 delta is entirely the pre-call literal the
> earlier scan's pattern omitted, six occurrences, and 197 + 1,850 + 321 = 2,368 exactly.
> `raw_register_name_refs` is unchanged at 136,378 and 189,696 throughout - which is exactly
> why the ruler could not see this defect, and why citing its zero as evidence that no `argN` reaches
> output was circular.

R28 annotated one loss site. Round 2 extended the same annotate-do-not-materialise design to two
more - loop-header entry and pre-call clobber - and, at the site R28 already covered, replaced a
containment test with a whole-value whitelist. This section publishes what came out, per sample and
per loss site.

**The headline number went down.** Total annotations fell from 5,069 to 4,369 on LocalSend and from
8,107 to 7,246 on Immich, and their share of raw register references fell from 3.72% to 3.20% and
from 4.27% to 3.82%. Two new sites contributed 397 and 510 annotations; tightening the existing site
removed 1,097 and 1,371. The second number is larger, so the round is a **net negative on annotation
volume** and is recorded as one. What moved in the other direction is the honest-form share, below.

### Identity of the two builds

**Reference: commit `ff07207`, identified by commit plus the output it produces, not by a binary
digest.** The digest this record once pinned for it is retracted as an identity test. The release
binary embeds its build directory, `/tmp` is tmpfs and was erased by a power cut mid-round, the
worktree that produced the pinned binary is gone, and a rebuild at the same path still hashed
differently - so the digest was never reproducible and a mismatch on it is not evidence of a stale
binary. A build is the reference iff it reproduces all of: 22,102 and 28,753 emitted files, 777,937
and 1,074,372 rendered lines, longest physical line 2,660 and 2,490, and `raw_register_name_refs`
136,378 and 189,696. The build used here reproduced every one.

**Candidate: commit `a7119ab`**, release binary
`b12271063c572646e4a3c68c3ab49c622ab1a546267e3b6278945e11e1bec5d2`. Unlike the reference, the
candidate binary is reproducible in this worktree, so that digest is a usable pin and is published as
one.

The coverage ledger was first built by an earlier candidate build of the same feature set,
`470334075c69fa30ad3fe35da1f971ef8f0c8962d000603556b7fef3d42b523d`. Every figure below was
**re-derived at `a7119ab`** by a fresh full-scope run of both samples, and the resulting annotation
span inventories are byte-identical to that earlier build's. Both artifact sets are retained and
listed at the end.

The ledger's schema has a `binaries.reference_sha256` field, and the re-derived ledger carries the
string `ff07207-output-anchored-no-digest` in it rather than a hash. That is deliberate: the field
predates the retraction, and filling it with any digest would republish an identity test this record
no longer stands behind.

### Method and units

Corpus runs are full scope on both samples - `decompile <apk> --function-scope all
--adapter-backend internal --split-records` plus the four permissive gate flags, no
`--max-functions`. Serial, one sample at a time, `df -h /tmp` recorded before and after each run,
stderr captured to its own file, `rc=0` asserted, and the manifest asserted at exactly 22,102 and
28,753 files before anything downstream ran. A truncated run is void rather than weak, so the count
is a precondition and not a reported figure. The permissive flags were separately shown to leave
every emitted counter unchanged - they move the gate verdict only - so a permissive corpus is a valid
source for these counts.

The units are:

- an **annotation** is one rendered comment span at one loss site on one output line. At most one
  annotation survives per rendered register spelling, so a `regN` never carries a chain;
- a **candidate element** is one rendered value inside such a span. A three-predecessor join whose
  arms disagree three ways is **one** annotation and **three** elements, so the two columns are not
  interchangeable and neither is a restatement of the other;
- **omitted by cap** is one annotation dropped **whole**. There is no partial annotation: the
  insertion path decides whole-or-nothing in one place per site.

Two independent counting paths are published side by side rather than reconciled into one number.
The audit stream (`FLUTTERDEC_PROV_AUDIT`, JSONL, one record per annotation plus per-function
omission summaries) is the emitter's own account. The corpus scan
(`scripts/scan-annotation-safety.py`) is a black-box parse of the emitted text that takes the four
annotation literals from `helpers/annotation.rs` rather than restating them. The ledger reports both
and their difference.

Figures are **per sample, never averaged**. LocalSend is Dart 3.5 and Immich is Dart 3.12; a mean
over two SDK generations describes neither.

### The ledger schema, so a missing row reads differently from an absent measurement

`coverage-ledger.json` is `schema_version` 1 and carries its own `fields` block. The fields this
section reports, all counts of annotations except where stated:

| field | meaning |
|---|---|
| `annotations` | annotations emitted at this loss site, from the audit stream |
| `candidate_elements` | rendered candidate values across those annotations |
| `omitted_by_cap` | annotations dropped whole by a budget |
| `omitted_by_cap_annotation_budget` | of those, dropped by `MAX_JOIN_ANNOTATION` |
| `omitted_by_cap_line_budget` | of those, dropped by the aggregate line budget `MAX_JOIN_ANNOTATED_LINE` |
| `omitted_by_unsafe_span` | dropped by the structural safety gate, not by a budget |
| `omitted_at_insertion` | the emitter's own count of every drop, counted at the drop |
| `annotations_in_corpus` | annotations the corpus scan found for this site's literals |

The distinction the schema is built to make:

- a site that **was** measured and found empty publishes a full row of zeros. A site that was **not**
  measured is absent from `loss_sites` entirely. The three site keys are fixed - `join`,
  `loop_entry`, `call` - so an absent key is a gap in the measurement, never a zero;
- an audit row whose site tag is none of those three lands in `unknown_loss_sites`, which is a
  separate map and is `{}` on both samples. A mislabelled row therefore cannot hide inside a site
  total;
- `annotations` and `annotations_in_corpus` come from different sides of the emitter, and
  `reconciliation.audit_rows_vs_corpus_scan` is their disagreement. A record that was emitted but
  never placed would show up there as a nonzero, rather than as a smaller total that still looks
  plausible;
- `reconciliation.rows_vs_counter` compares the summed drop rows against `omitted_at_insertion`,
  which the emitter counts at the drop itself and publishes per function. A row lost between emitter
  and audit file surfaces here;
- `reconciled` is true only when both reconciliation counts are zero and `unknown_loss_sites` is
  empty. It is true on both samples.

Whether a drop was whole rather than a truncation is deliberately **not** asked of the ledger. It is
a property of the artifact, and the corpus scan answers it: a span cut short is either left without
its terminator, reported as `unclosed`, or longer than a budget, reported as `over_span` and
`over_cap`. All three are zero on both samples.

### LocalSend (Dart 3.5), candidate `a7119ab`

| loss site | annotations | candidate elements | `omitted_by_cap` | by annotation budget | by line budget | `omitted_by_unsafe_span` | `omitted_at_insertion` | `annotations_in_corpus` |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `join` | 3,972 | 4,839 | **0** | 0 | 0 | 0 | 0 | 3,972 |
| `loop_entry` | 390 | 390 | **0** | 0 | 0 | 0 | 0 | 390 |
| `call` | 7 | 7 | **0** | 0 | 0 | 0 | 0 | 7 |
| **total** | **4,369** | **5,236** | **0** | 0 | 0 | 0 | 0 | **4,369** |

`unknown_loss_sites` `{}`, `audit_rows_vs_corpus_scan` 0, `rows_vs_counter` 0, `reconciled` true.
Against `raw_register_name_refs` of 136,378: join 2.91%, loop entry 0.29%, call 0.005%, all sites
3.20%.

### Immich (Dart 3.12), candidate `a7119ab`

| loss site | annotations | candidate elements | `omitted_by_cap` | by annotation budget | by line budget | `omitted_by_unsafe_span` | `omitted_at_insertion` | `annotations_in_corpus` |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `join` | 6,736 | 8,340 | **0** | 0 | 0 | 0 | 0 | 6,736 |
| `loop_entry` | 488 | 488 | **0** | 0 | 0 | 0 | 0 | 488 |
| `call` | 22 | 22 | **0** | 0 | 0 | 0 | 0 | 22 |
| **total** | **7,246** | **8,850** | **0** | 0 | 0 | 0 | 0 | **7,246** |

`unknown_loss_sites` `{}`, `audit_rows_vs_corpus_scan` 0, `rows_vs_counter` 0, `reconciled` true.
Against `raw_register_name_refs` of 189,696: join 3.55%, loop entry 0.26%, call 0.012%, all sites
3.82%.

Elements per annotation are 1.22 and 1.24 at the join site and exactly 1.00 at the other two, which
is a property of those sites rather than a measurement: a loop header annotates its single entry
value and a call annotates the one pre-call value.

### Against the reference, per site

The reference corpus is **not** annotation-free - R28's join annotation landed before `ff07207` - so
the reference column is a real count, taken from the same corpus scan run against a `ff07207` build.

| loss site | LocalSend `ff07207` | LocalSend `a7119ab` | Immich `ff07207` | Immich `a7119ab` |
|---|---:|---:|---:|---:|
| `join`, exhaustive (`= `) | 158 | 762 | 253 | 1,412 |
| `join`, non-exhaustive | 4,911 | 3,210 | 7,854 | 5,324 |
| `join`, total | 5,069 | 3,972 | 8,107 | 6,736 |
| `loop_entry` | 0 | 390 | 0 | 488 |
| `call` | 0 | 7 | 0 | 22 |
| **all sites** | **5,069** | **4,369** | **8,107** | **7,246** |

Two things happened at the join site and they pull opposite ways.

**The population shrank, because the usefulness filter was wrong.** R28's filter was a containment
test: `value.contains(".f")` accepted `(thread.f80 + 1)`, which is an arithmetic expression over a
field, not a field access. It is now a whole-value whitelist of exactly three forms - literal, field
access, call-shaped expression - decided by one function, `candidate_form` in
`control_flow/structured.rs`, which all three loss sites classify through. An old-filter control
built from the same tree isolates the cost of that change to the join site at 5,284 down to 3,972
and 8,543 down to 6,736. *That intermediate pair is a worker-reported control figure whose corpus was
not retained off tmpfs; the two end points are both traceable to retained scans, the middle is not,
and it is labelled here rather than presented as if it were.*

**The honest form got much more common.** An annotation renders `= ` only when the captured arms are
exactly the join's own predecessors, and `possible (non-exhaustive):` otherwise. Capture now
enumerates the join's predecessors and reads each candidate from that predecessor's retained
block-end snapshot, so joins with three and four predecessors are covered at all and a complete one
is recognised as complete. The exhaustive share of join annotations rose from 3.1% to 19.2% on
LocalSend and from 3.1% to 21.0% on Immich.

> **This conclusion reversed.** See the banner at the head of R29. The exhaustive share did not rise to
> 19.2%/21.0%; corrected, it is 9.6% and 8.3%. The honest-form gain claimed here was substantially an
> artifact of counting annotations whose values named identifiers absent from their own file, and the
> exhaustive class was the one most inflated - fitting, since the exhaustive form is precisely the one
> that asserts completeness. What survives is the second half: coverage of two loss sites that previously
> had none.

So the round bought a smaller set of annotations, a much larger fraction of which make a complete
claim, plus coverage of two loss sites that previously had none. Whether that trade is worth 700 and
861 annotations is not something these figures decide, and this section does not claim they do.

### `omitted_by_cap` is a measured zero, not an unexercised counter

A zero in a drop counter is the easiest number in this record to obtain dishonestly, so it carries
two controls.

**An uncapped control proves the zero.** The same tree rebuilt with both budgets at `usize::MAX`
(binary `0ac03e7bebf0a160da489801c7f05b50f048f58ccbd82e0dc91745218298cc4a`) produced span inventories
**byte-identical** to the candidate on both samples - re-checked here by diffing that control's
retained inventory against the `a7119ab` re-derivation, not inherited from a report. If any
annotation had been dropped by a budget, raising the budgets would have made it appear.

**A lowered-cap pair proves the drop path executes end to end through the CLI.** Rebuilding with the
budgets at 40 and 200 (candidate `be4b8bdc03b03eafb98a5c5d85ec6533a9f08f15e300077fac26c2dca27ec2a8`,
its uncapped twin `46603e77ff70cddeff4c60203977597963f557c42739d5832ce08fa3f8aff744`):

| | LocalSend annotations | LocalSend `omitted_by_cap` | Immich annotations | Immich `omitted_by_cap` |
|---|---:|---:|---:|---:|
| `join` | 582 | 5,238 (5,231 annotation / 7 line) | 1,144 | 8,611 (8,581 / 30) |
| `loop_entry` | 388 | 4 (4 / 0) | 484 | 4 (4 / 0) |
| `call` | 3 | 4 (4 / 0) | 7 | 15 (15 / 0) |
| **total** | **973** | **5,246** (5,239 / 7) | **1,635** | **8,630** (8,600 / 30) |

Rows and the emitter's own `omitted_at_insertion` agree exactly on both samples, and `unclosed` stays
at zero, so lowering a budget drops whole annotations rather than truncating them.

At the shipped budgets the margin is wide: the longest physical line is 2,660 and 2,490 against a
3,000 line budget, and the longest annotation span is 124 and 101 against 512.

Two counts in the lowered-cap ledger are reported and deliberately **not** failed, under
`reported_not_failed`. `dropped_span_not_in_uncapped_corpus` is 643 and 772: every one of those comes
from a function and register with a second dropped row, and the cause is the
one-annotation-per-register-spelling rule - raising the budgets lets the first anchor take a
coordinate the second would otherwise have taken. `dropped_span_also_emitted_elsewhere` is 6 and 29,
which is not evidence of a false drop either, because spans are not unique: identical values at two
joins render identical bytes. Truncation is answered by the scan, not by looking a span up in the
corpus.

### The counters did not move, which is the required result rather than a good one

Annotation recovers nothing. It appends a comment after every analysis and rewrite have run, and the
quality counters read the code span before it. So a delta in **either** direction in any
`quality.json` counter would be contamination, and the target was zero deltas rather than a lower
number.

On both samples, candidate `a7119ab` and reference `ff07207` produce **byte-identical**
`quality.json` files: sha256 `5ea32677b160e94633704bb9f95939dc5d63386602444fcefc240529d15ea56c` on
LocalSend and `bd3255abb0eb1e461c2f0d45b4d541a0fa98ff2a6f3fe784a751242e17284cb0` on Immich, all keys
equal, `raw_register_name_refs` 136,378 and 189,696 with a delta of 0 in both directions and
`raw_arg_name_refs` 0 on both. Because the gate verdict is a total function of those four thresholded
values, byte-identical `quality.json` files mean **no candidate-introduced gate failure at any
threshold set**, not merely at the ones that were run.

### `argN` is not a register spelling this feature has to cover

Recorded because an earlier draft of R28 said it was, and a record that quietly drops a corrected
claim teaches nothing.

Candidates that are themselves unrecovered are rejected, and the authority for what counts as
unrecovered is one function: `unrecovered_value_spellings` in `crates/flutterdec-decompiler/src/helpers/naming.rs`.
It returns the canonical `xN`, the `named_register_alias` form (`regN`, with `framePointer` for 29
and `returnAddress` for 30), and the `named_indirect_target` form (`dispatchTarget` for 30,
`cachedTarget` for 2, `indirectTarget{n}` otherwise). **`argN` is not a member of that set and the
record must not describe it as one.** `apply_name_and_type_hints` rewrites every argument register to
a named form, so `argN` does not survive into final output and `raw_arg_name_refs` measures 0 on both
samples. The reason it came up at all was comment contamination, before the counters were taught to
read the code span, which is a different problem and is fixed separately.

### A delimiter drift that never fired, and was one edit from six counters

Worth an entry precisely because it produced no observable defect.

At `ff07207` the join emitter built its span as `format!(" /*{}{} */", prefix, values.join(" | "))`,
where `prefix` held only the label. The strip parser that hides annotations from the quality counters
matched byte strings that included the delimiters. That is two independent spellings of one literal
with no constant tying them together - they agreed only because the emitter's concatenation happened
to produce the parser's bytes.

The blast radius if they had ever disagreed: the strip parser stops recognising the span, and the
whole annotation - label, every candidate value, both delimiters - is counted as emitted code by all
six counters in `source_text_counters` (`crates/flutterdec-core/src/pipeline/quality.rs`). One reword
of a label on either side would have done it, and the resulting counter movement would have looked
like a change in the emitted program rather than like a parser miss.

Removed by giving each literal a single definition that owns **every** delimiter - opener, the ` | `
separator and the ` */` terminator, the latter two shared across all four literals in
`helpers/annotation.rs`. The emitter renders through `render()` and both parsers recognise through
`annotation_at()`, so no consumer holds its own copy. A constant holding only the label would not
have fixed it, because both sides would still have hand-rolled the delimiters.

Two hazards found while closing it, both cheap to re-trip:

- a drift check that counts occurrences of a literal also counts the literal **quoted in a comment**,
  so the file defining a protected literal must not spell it again in prose beside it;
- the one-definition check counts occurrences of the *current* opener, so a hand-written copy of a
  **superseded** spelling is invisible to it. A separate check for consumers hand-rolling a delimiter
  is what catches that case; the two are not redundant.

### The two-sample limitation, restated

Every figure above is one sample's. There are two samples - LocalSend, Dart 3.5, and Immich, Dart
3.12 - chosen as a deliberate two-generation control, and nothing here is averaged across them. A
mechanism that holds on both is evidence about the SDK; one that holds on a single sample is a
version artifact until a third disagrees.

Neither sample is a random draw from any population of Flutter applications, so none of these counts
is an estimate of anything beyond these two binaries. In particular the `call` site's 7 and 22
annotations are small enough that a third sample could move them by an order of magnitude in either
direction without contradicting anything published here.

### Retained raw artifacts

Every cell above comes from one of these. Digests are of the artifact, not of a corpus - the emitted
corpora are hundreds of megabytes on tmpfs and were deleted after scanning, which is why the scans
and the audit streams are what is kept.

Re-derived at `a7119ab` for this section, under `~/.zenith/evidence/record-016/`:

| artifact | what it carries |
|---|---|
| `run.sh` | the exact driver: flags, serial order, `df` preflight, `rc` and manifest assertions |
| `A-{localsend,immich}-scan.json` | files, lines, longest line, longest span, annotations by literal, all four violation counts |
| `A-{localsend,immich}-scan-spans.txt` | the full span inventory, one line per distinct span with its count |
| `A-{localsend,immich}-quality.json` | every quality counter for the candidate |
| `audit-{localsend,immich}.jsonl` | one record per annotation, plus the per-function omission summaries |
| `coverage-ledger.json` | the two per-sample ledgers rebuilt from the above |
| `A-{localsend,immich}.stderr` | both 0 bytes, which is the assertion the runs were clean |
| `df.log` | `/tmp` free space before and after each run |
| `evidence.sha256`, `candidate-binary.sha256` | digests of all of the above, and of the candidate binary |

Retained from the round's own measurement, under `~/.zenith/evidence/cap-and-ledger/`:

| artifact | what it carries |
|---|---|
| `R-{localsend,immich}-scan.json` | the `ff07207` reference scan - the reference column of the per-site table |
| `R-{localsend,immich}-quality.json` | the reference quality counters the candidate is compared against |
| `A2-{localsend,immich}-scan*.txt/json` | the wave-2 candidate scan, span inventories byte-identical to the `a7119ab` re-derivation |
| `B-{localsend,immich}-scan-spans.txt` | the uncapped control's span inventory |
| `C-{localsend,immich}-scan*.json` | the lowered-cap corpus, including every over-cap line |
| `Bp-{localsend,immich}-scan*.json` | the lowered-cap pair's uncapped twin |
| `audit-c-{localsend,immich}.jsonl` | the lowered-cap audit stream, source of the 5,246 and 8,630 drop rows |
| `coverage-ledger-lowered-caps.json` | the lowered-cap ledger |
| `binaries.sha256`, `evidence.sha256`, `run.sh`, `runc.sh`, `rescan.sh`, `df.log` | build identities, artifact digests and the drivers |

The `quality.json` comparison and the gate derivation also have retained copies under
`~/.zenith/evidence/gate-counters/` as `q-{cand,ref}-{ls,im}.quality.json`.

## R30. The fabricated signature, and why trimming it is the wrong fix

> **Resolved in R32, from a direction this section did not consider.** The analysis below holds - trimming
> really is wrong, and the over-pass measurement stands - but it framed the honest options as "keep six slots
> and add a marker" or "leave it and document". There was a third: keep all six slots and change their
> **spelling**, so the names stop claiming a role they cannot support. `receiver, param1..param5` became
> `slot0..slot5`. The six-slot envelope survives, so a declaration is still at least as wide as any call and
> the over-pass this section warned about cannot occur. No comment was added, no counter moved, and the line
> is shorter than before. Read this section for why the obvious fix is wrong; read R32 for what landed.

Not a result. This records an investigation that stopped short of a change, because the obvious fix is
measurably worse than the defect, and the next person to look at this should not have to re-derive that.

### The defect

Every emitted function declares the same signature:

```dart
dynamic sub_7781e4(dynamic receiver, dynamic param1, dynamic param2, dynamic param3, dynamic param4, dynamic param5)
```

Six identifiers on every function, whatever it does. `arg_ids` is built as
`(0..DART_ARGUMENT_REGISTERS.len())` (`passes/naming.rs:349`) over
`["x1","x2","x3","x5","x6","x7"]` (`helpers/dispatch_table.rs:30`), unconditionally. The code already carries
a comment warning that this "silently widens every signature regardless of what the emitter wrote". It is
**six**, not eight - the `arg0..arg7` strings elsewhere in the tree are hand-built test fixtures, not
generated output.

This is the same class as the register loss R28 and R29 address: output asserted with more confidence than the
analysis earns. A reader has no way to tell a real two-argument function from this template.

### Trimming to body-referenced parameters would produce self-contradictory Dart

The tempting fix is to declare only the parameters the body actually reads. Measured on 400 LocalSend
functions, the distribution of body-referenced parameters is:

| referenced params | functions |
|---|---|
| 0 | 116 |
| 1 | 10 |
| 2 | 2 |
| 6 | 272 |

So 116 of 400 would be declared `f()`. But call sites build their argument list from the **same** six-register
file (`control_flow/emit.rs:352`) and only truncate from the end (`:366-368`), so a rendered call carries **at
most six** positional arguments - six by construction, not by observation. Measured on the same 400 functions,
counting commas at paren depth 1 so nested calls and string literals do not inflate the count, the call-site
arity distribution is 0:2,472, 1:1,844, 2:2,251, 3:1,342, 4:223, 5:77, 6:85, maximum **6**, which agrees with
the constructed bound.

Today every declaration is six wide, so a declaration is always at least as wide as any call to it and no call
can over-pass. Trimming inverts that invariant, and this was **measured by joining each callee to its own
callers** rather than inferred from the two distributions above - which would have been exactly the kind of
composed claim this section argues against. Parsing all 400 declarations and every call site in the same
corpus, then matching on callee name:

| | |
|---|---:|
| declarations parsed | 400 |
| callees with at least one visible caller | 36 |
| of those, **over-passed after trimming** | **14 (38%)** |
| worst gap | 3 extra arguments |

Concretely, `sub_9a0f0c` reads one parameter and is called with four; `sub_ab51e8` reads none and is called
with two. After trimming, that renders as a function declared `f(receiver)` invoked as `f(a, b, c, d)` in the
same corpus.

Only 36 of 400 callees have a caller inside a 400-function slice, so 38% is a rate over the joinable subset and
the absolute count is a lower bound - a full-scope run would join far more. That is enough to decide the
design: emitted Dart that contradicts itself is worse for a reader than a uniform template, so the trim fails
on its own terms.

*Method note, because it nearly became a false figure in this record: a first pass reported a maximum of 13,
which was an artifact of splitting on `(`, `)` and `,` together, so nested calls such as
`f(a, smiUntag(b.f8))` inflated the field count. The same corpus counted at depth 1 gives 6. A 6-entry
register file that only truncates cannot emit 13, and that contradiction is what exposed the error - the
structural bound was the better argument all along, and the measurement is now only corroboration.*

### Arity from body reads is not recoverable anyway

Even without the call-site problem, "the body reads two parameters" is not "the function takes two
parameters". A callback that ignores its arguments still receives them. Trimming would replace one fabrication
with a quieter one that is harder to notice.

Two further hazards, recorded because each is a subtler bug than the one being fixed:

- **`paramN` is positional; survivors must keep their original index.** If a body reads `arg0` and `arg3`, the
  signature must read `receiver, param3`, never `receiver, param1`. Re-enumerating a filtered list silently
  relabels which ABI register a reader believes they are looking at.
- **The change is counter-neutral but not byte-neutral.** `raw_arg_name_refs` is 0 on both samples, because
  every `argN` is renamed to `receiver`/`objN`/`valueN`/`paramN` before emission - a scan of a real corpus for
  `\barg[0-7]\b` finds zero occurrences - and the signature line carries no `xN`, `regN` or `_block_` either.
  So no quality counter moves and this is **not** a ruler change. It does change emitted bytes, needs its own
  reference comparison, and would move the longest-line figures downward.

### What an honest signature would look like

Since arity is not recoverable, the honest options are to keep the six slots and **mark the declaration as a
template rather than a signature**, or to leave it and document the convention. A comment marker is
counter-neutral, because comments carry none of the counted tokens. Its cost is one comment on each of 22,102
and 28,753 functions - roughly five times the entire annotation volume R29 added - so whether that is worth it
is a real trade and belongs in a contract with a falsifiable acceptance clause.

Implementation cost, for whoever takes it: seven test files hardcode a literal signature string
(`golden_and_parser.rs`, `control_flow_compaction.rs`, `alias_and_expr_cleanup.rs`, `helper_inlining.rs`,
`readability_and_naming.rs` and two others), 44 occurrences in total.

## R31. Heuristic names are indistinguishable from recovered ones

> **Resolved in R32.** The classification below is unchanged and the measurement stands. What landed is the
> rule that separates the families: a name may describe an observed **source**, never an inferred type or
> role. So `objN`, `valueN`, `receiver`, `paramN`, `objTmpN` and `intTmpN` are gone, while `poolValN`,
> `resultTmpN`, `tN` and `local_mN` stayed, because each states something observed. The per-declaration
> marker this section costed and rejected was never needed.
>
> The residual this banner previously called "still open" - reserving a `source_` prefix so that absence of a
> synthetic marker cannot imply recovery - turned out to be **vacuous**, because no synthetic marker was
> introduced. Nothing in the output claims recovery, so there is no absence to misread. See the closing
> subsection of R32 for why building the unused prefix would have been worse than leaving it out.

Also not a result. Same shape as R30: a defect located, sized, and left unimplemented because the fix is a
design decision rather than an edit.

### The defect

An emitted local or parameter carries a name that looks derived from the program. **None of them is.** Every
such name is synthesized from the emitted text itself: `apply_name_and_type_hints` receives only the function
name and reads `self.lines`, and `infer_declared_types_from_context` likewise reads lines. No adapter class,
field or symbol metadata reaches a local or parameter name. Three families produce them:

- **Call results**, `tN`, from a per-function counter at `control_flow/emit.rs:338` - sequential, carrying no
  claim beyond "the Nth call in this body".
- **Parameters**, from usage counts at `passes/naming.rs:371-379`: index 0 becomes `receiver`,
  `field_access >= 1` becomes `objN`, `arith_ops >= 2 && field_access == 0` becomes `valueN`, otherwise
  `paramN`.
- **Locals**, on the same principle at `:402-424`: `pool_assign > 0` becomes `poolValN`,
  `field_access >= 2` becomes `objTmpN`, `arith_ops >= 2 && field_access == 0` becomes `intTmpN`, then
  `resultTmpN`, then `tmpN`. Separately, `helpers/naming.rs:61` derives `local_mN` / `local_pN` from the
  **stack offset** - the one family tied to a program fact, and even that is an offset rather than a name.

One field access is enough to call something `obj`. Two arithmetic operations are enough to call it `value` or
`intTmp`. These are reasonable reading aids, not recovered facts, and nothing in the output says so. The
request's definition of noise is "constructs that admit the decompiler did not recover something"; these
quietly assert the opposite.

**Scope of that claim, stated so it does not contradict R28 and R29.** It is about **local and parameter
names only**. Other identifier classes in the same output *are* recovered where the evidence exists: callee
names from runtime-stub identity and symbol maps render as real names - `allocateClassId5637`, `classId`,
`cachedTarget` all appear in the corpus measured here - and pool-derived string and selector literals are
resolved where the pool entry is known, which is exactly what R28 and R29 rely on for the candidate values
they annotate. What has no recovered source is the name of a local or a parameter.

Distribution over 400 LocalSend functions, by identifier occurrence: `t` 20,248, `objTmp` 9,997, `tmp` 8,669,
`resultTmp` 3,435, `param` 2,345, `receiver` 1,549, `intTmp` 896, `obj` 148, `local_m` 48, `value` 25. Of
9,613 emitted `final` declarations, **9,609 are `tN`** - so the dominant family is the call-result counter, not
the heuristics, which is worth stating because the heuristics are the ones that read like recovered names.

**A convention already exists and covers only half of them.** `is_opaque_temporary`
(`control_flow/structured.rs:314`) treats `t`, `tmp`, `objTmp`, `intTmp` and `resultTmp` as opaque. It does
**not** cover `receiver`, `objN`, `valueN`, `paramN`, `poolValN` or `local_mN`. So publishing "these prefixes
are synthesized, everything else is recovered" would certify the heuristic *parameter* names as recovered -
precisely the overreach this section is about. Any documented convention needs three buckets, and the third
one, genuinely metadata-derived local and parameter names, is **empty** on current output.

### Why this is not a one-line fix, and the trap waiting for whoever tries

The obvious fix is a marker distinguishing heuristic names from recovered ones. Two constraints make it a
contract-sized decision:

**Volume.** It applies to essentially every declaration - tens of thousands per sample, far more than R29's
entire annotation output. A per-declaration comment is a large readability cost paid on every line to flag
something a convention could carry instead (for example, reserving a spelling that means "synthesized" and
documenting it once, which is what `tmpN` already half-does by accident).

**The counters are not comment-blind.** `pipeline/quality.rs:12` strips **only** the four annotation literals
before counting, deliberately, so that pre-annotation reports stay bit-comparable. Everything else in a line -
including any new comment form - is still counted. And `:18-24` counts `x0..x30` **and** `reg0..reg30` as
identifier tokens. So the natural marker wording, something like `/* name synthesized from x8 */` or
`/* unrecovered reg12 */`, would **inflate `raw_register_name_refs`**, the one counter this round requires to
be byte-equal in both directions. Any marker must either avoid the `xN`, `regN`, `_block_` and `/* cond */`
token spellings entirely, or be added to the strip span like the four annotation literals were. Verify with a
counter diff, never by eye.

That second constraint is the reusable lesson, and it generalises beyond this gap: on this project a comment
is not automatically free. Adding one is a ruler change unless its span is stripped or its wording avoids every
counted token.

## R32. Names may say where a value came from, not what it is

R31 recorded that no local or parameter name is metadata-derived, and left the fix open because the
obvious one - a marker comment per declaration - costs roughly ten times the project's entire annotation
budget and risks the ruler. This is the fix that avoids both, and the measurements that chose it.

### The rule

**A name may describe an observed source. It may not assert an inferred type or role.**

Applied to the two families that broke it:

| old | new | what it asserted, and on what evidence |
|---|---|---|
| `receiver` | `slot0` | a role, from position alone |
| `objN` | `slotN` | an object, from **one** field access |
| `valueN` | `slotN` | a value, from **two** arithmetic operations |
| `paramN` | `slotN` | a parameter, when arity is unrecoverable (R30) |
| `objTmpN` | `tmpN` | a type, from two field accesses |
| `intTmpN` | `tmpN` | a type, from two arithmetic operations |

Kept, because each states something observed rather than inferred: `poolValN` (assigned from the object
pool), `resultTmpN` (assigned from a call result), `tN` (the Nth call in this body), `local_mN` / `local_pN`
(the stack offset).

The index on `slotN` is the one earned fact about a parameter - its position in the argument-register file -
and it survives verbatim. It is **never renumbered**, because callers render arguments from the same file, so
renumbering would relabel which register a reader is looking at.

### Why this and not a marker

Three designs were costed on the real LocalSend corpus, 22,102 files and 777,937 rendered lines, by applying
each renaming to actual output and measuring:

| design | characters | per line | longest line | honest about |
|---|---:|---:|---:|---|
| baseline | 25,651,247 | - | 2,660 | nothing |
| `synthetic_param_N` / `synthetic_local_N` prefixes | 35,227,813 | **+12.31** | 2,660 | everything, including the earned names |
| this change | 24,460,315 | **-1.53** | 2,660 | only the unearned |

The verbose prefix is **+37.3%** of all emitted characters. For scale, the entire annotation feature of R28
and R29 added 362,383 bytes at 0.47 bytes per rendered line, and that was treated as a cost worth measuring
carefully; a `synthetic_` prefix is roughly **26 times more per line**. For a tool whose purpose is
readability that is disqualifying, and it also relabels `poolValN` and `local_mN`, discarding facts.

This change instead makes output **smaller**: measured end to end against the reference binary, LocalSend
25,711,643 to 25,301,455 characters (**-1.60%**) and Immich 35,027,028 to 34,444,860 (**-1.66%**). Honesty and
brevity aligned here rather than trading off, because the names that were removed were the long ones.

### Neutrality

Verified by running the candidate and the `ff07207` reference over both samples at full scope, then comparing
`quality.json` field by field. **All fourteen counters are equal**, `raw_register_name_refs` included at
136,378 and 189,696. Rendered line counts are identical, +0 on both samples. Longest physical line is
unchanged at 2,660 and 2,490 against the 3,000 cap. Emitted file manifests are 22,102 and 28,753 on both
sides. So this is not a ruler change, and `slotN` / `tmpN` contain none of the `argN`, `xN`, `regN`,
`_block_` or `/* cond */` spellings the counters look for - no stripper change was needed.

### The information is relocated, not lost

The type guesses that used to live in identifiers still exist where they can be checked. `intTmp1` became
`int tmp1` - the inference is unchanged, it simply moved from the name to the declared type, which is
`dynamic` when unproven. A reader can disagree with a declared type by reading the code; they cannot
disagree with a name.

### The other half of R31 is closed too, by not building anything

R31 left a residual: absence of a synthetic marker does not certify that a name **was** recovered, and
reserving a `source_` prefix for the metadata-derived bucket would close it "cheaply, because it applies to
nothing today". Re-examined after this change, that residual is **vacuous**, and the phrase "cheap because it
applies to nothing" was the tell.

It assumed the design that was not built. The proposal it came from paired a `synthetic_` prefix with a
`source_` prefix, so absence of the first could mislead. What landed has **no** synthetic marker: names are
either neutral (`slotN`, `tmpN`) or state an observed source (`poolValN`, `resultTmpN`, `tN`, `local_mN`).
Nothing claims recovery, so there is no absence to misread. A `source_` prefix would be an identifier
namespace with no producer, no consumer and no caller - a speculative abstraction whose only justification is
a future that may not arrive, and which would be dead on arrival by construction.

If metadata-derived local names are ever recovered, whoever adds them will need a way to distinguish them,
and will design it against a real case rather than a hypothetical one. That is strictly better than a prefix
reserved today by someone who cannot see what it must distinguish.

Recorded because "cheap and it applies to nothing" is an argument that sounds like thrift and is actually its
opposite: the cost of unused machinery is not the code, it is that a later reader must work out whether the
empty case is a gap or a decision.

## R33. The annotations named identifiers that did not exist

The feature's only job is telling a reader which value a register held. On LocalSend at full scope,
**3,022 of the identifiers inside annotation spans appeared nowhere else in their file**, across 1,902
of 22,102 files. `/* = local_m32.f8 */` beside a body that calls that local `tmp7`.

### Why the ruler could not see it, and why my evidence was circular

`raw_register_name_refs` reads 0, and I cited that as proof no `argN` reaches emitted output - in R28,
then again in R31 and R32. The counter strips annotation spans **before** counting
(`pipeline/quality.rs`), so it is structurally blind to anything inside one. The measurement I used
could not observe the thing I claimed. That is the second published falsehood of this round traced to
trusting a number without checking what it can see.

The cause is an ordering the design always had: candidates are captured while a line is rendered, which
is before `apply_name_and_type_hints`, and inserted after it. So a candidate spells its locals `argN`,
`local_mN`, `local_pN` while the body has moved to `slotN`, `tmpN`, `poolValN`, `resultTmpN`.

### Two mechanisms, because one class is rescuable and the other is not

| class | occurrences | outcome |
|---|---:|---|
| `argN` | 834 | **rescued.** Replaying the rename map gives `slot0.f8` - a field access on an identifier live in the signature. 815 `slotN` appear in spans afterwards. |
| `local_mN` | 1,215 | **dropped.** Renames to `tmpN`, which the existing filter rejects as one gap decorating another. Correct, and previously hidden by the stale spelling. |
| `local_pN` | 974 | **dropped**, same reason. |

The candidate is re-judged *after* the replay, on the text that will actually be emitted. That is what
`candidate_form` promises - "the value is classified exactly as it will be rendered" - and what an
independent scan of the output checks, so renaming without re-judging would have emitted text the filter
itself would refuse.

What survives both is checked against the identifiers actually present in the body. That rule is
**structural, not a list of naming families**: a list is fail-open, passing silently the next time the
naming pass gains a family, which is how this defect and a stale provenance fixture both arrived. So
every identifier must be present unless its position proves it is not a local - followed by `(` it is a
callee, preceded by `.` it is a field - with the reserved globals read from the single definition the
naming pass already seeds from.

The identifier set is snapshotted before insertion rather than scanned per token, because `self.lines`
is mutated by the insertion loop: otherwise a token could count as live because an *earlier annotation*
mentioned it. Annotations vouching for each other.

### Snapshots had to move too

Renaming candidates while leaving snapshots raw made a sound emitter report violations, because
`check_snapshot` is audit-internal - "every candidate's value is in the snapshot its own id names".
Renaming both is sound precisely because that rule compares the audit against itself; the rules that
reach outside it, `ir` and `loop_ir`, check site keys, path keys and binding loss, never value
spellings.

### Verification

Dangling references **3,022 to 0**. `argN` in spans **834 to 0**. Every `quality.json` counter unchanged
on both samples, `raw_register_name_refs` at 136,378 and 189,696. The audit is byte-inert: enabling it
produces an identical corpus (`diff -rq`, 0 differing files). Both validators report zero on a real
corpus - the honest checker across all seven rules unfiltered, and the cross-audit reconciler across all
seven counts, 2,374 spans against 2,374 records.

Cost: annotation spans 4,363 to 2,374 on LocalSend and 7,246 to 3,817 on Immich. R29's figures are
superseded, and its headline inverts - see the banner there.

### A negative control that had rotted

`testdata/provenance/join-audit-sample.jsonl` carried snapshot `site_key`s naming the join rather than
the predecessor path, left stale when the join snapshot key was corrected earlier in this round. So the
planted-violation test could not pass: the clean fixture scored 5 violations and each plant scored 5
instead of 1. The control that proves the checker detects a real violation was silently dead, and
nothing in CI would have said so - `scripts/lint-shell.sh` globs `*.sh`, so none of the round's eleven
Python scripts is guarded by anything. Regenerated from each row's own `snapshot_id`.

## R34. `resultTmpN` claimed a call result it never observed

Found while auditing the naming rule R32 introduced, and it is a defect in that rule's enforcement
rather than in the rule.

`resultTmpN` survived R32 because it states an observed source: assignment from a call result. The
selector was a `"{id} = t"` prefix test, so `{id} = thread.f104.f1968` and `{id} = true` matched too,
and both are ubiquitous in real output. The name asserted a source nothing had verified - the exact
class of unsupported claim R32 exists to remove, sitting inside one of the two names R32 kept.

### The obvious fix was itself incomplete, in the same class

Requiring `t` followed by a digit still accepted `{id} = t8.f12` - a field *of* a call result, which is
not a call result. And that was the **dominant** case, not an edge: 456 files, against the 2 spellings
the first fix addressed. Often both forms sit side by side on different locals, as in
`00115_sub_613bb8`:

```
final t8 = sub_8a0018(t7);
resultTmp2 = t8;        // earned - t8 is the call result
resultTmp3 = t8.f12;    // a different local, named on field-load evidence alone
```

So the predicate is now the whole temporary and nothing appended: digits, then `;` or end of line. Same
digits-only discipline as `is_opaque_temporary`. Field-load names fell **456 to 136**, and the residual
136 are legitimate - checked exhaustively, not sampled: in every one, the same local also has a bare
`resultTmpN = tN;` elsewhere in the file, because `collect_ident_stats` aggregates per local. Zero
unearned remain.

Checked the other survivor at the same time: `pool_assign` requires a literal `{id} = pool[`, `= (pool[`
or `= ((pool[`, so `poolValN` does state an observed source. **R32's rule stands as written; exactly one
survivor was misclassified.**

This does not move annotation counts - measured, identical at 2,374 spans and 197 exhaustive before and
after, `raw_register_name_refs` unchanged at 136,378. It changes which name a local is given, nothing
about which values are annotated.

### The fixture, and two ways it nearly tested nothing

Both halves of the predicate are pinned by mutation: removing the terminator check fails on `= t8.f12`,
removing the digit check fails on `= t`. The first version of this fixture pinned **neither** - its three
cases all passed under the weaker predicate, so it would have shipped green against the defect it was
written for.

It also twice registered a local in `lines` but not in `emitter.locals`, which is silent here: an
unregistered local keeps its `local_mN` spelling, contains no `resultTmp`, and every negative assertion
about it passes vacuously. So the fixture now asserts no `local_m` spelling survives the naming pass,
which makes that omission loud once for all cases. Worth stating because it is the same failure mode as
the rotted provenance fixture in R33: a check that cannot fail is indistinguishable from a check that
passes.

R32's own figures are deliberately **not** retro-edited: they were measured against R32's commit, and
folding a later change into them would make them irreproducible from that commit, which is the property
that makes this record citable. Same treatment as R25's retracted counts and R20's baseline.
