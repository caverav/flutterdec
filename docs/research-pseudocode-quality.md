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
  --max-placeholder-ifs 999999 --max-unresolved-cf 999999 \
  --max-indirect-call-ratio 1.0 --min-disassembly-ratio 0.0
```

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
distinct selectors, zero negative offsets, receiver resolved on two thirds, arity
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
(`runtime/vm/constants_arm64.h`) is R0 through R14; the assembler uses TMP and
TMP2 freely, R18 is volatile off Fuchsia, and the call writes LR. Only R19
through R28 and SPREG survive.

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

`SmiUntag` is `sbfm(dst, src, kSmiTagSize, kSmiBits + kSmiTagSize)`, so it is the
only producer of a signed extract at bit 1 of width `kSmiBits + 1`: 31 compressed,
63 not. Both are accepted, so the rule encodes no build configuration. 25,899
sites read `smiUntag(x)` rather than `signedBitField(x, 1, 0x1f)`. The insert form
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
