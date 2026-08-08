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

Every counter there is a count of **emitted lines**
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

```
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

```
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
final t3 = dispatch.sel25768(); // dispatch table, selector_offset: 25768, args: unknown
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
  ```
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
- Field offsets render as raw byte displacements (`obj.f7`, `obj.f15`, `obj.f23`),
  hiding that they are all `8k - 1`: tag-adjusted word slots 0, 1, 2. Dividing
  through makes them readable now and directly joinable to a class field table
  later.
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

Loops, over the 830 functions that have one:

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
| pseudocode lines | 628,929 | **485,455** |
| emitted call statements | 109,729 | **83,245** |
| inflation | 3.03x | **2.28x** |
| duplicate-line fraction | 67.9% | 62.0% |
| `omitted_path_markers` | 1,026 | 804 |
| `loop_backedge_markers` | 448 | 296 |
| functions where emitted equals emittable | 45.0% | **72.8%** |
| out-of-scope temporary references | 0 | 0 |
| unresolved register references | 2,093 | 10,446 |

The last row is the cost, and it is the honest kind: a value that genuinely
differs per path is now named as an unresolved register rather than given one
path's expression. The old golden for `goldenSimpleLoop` asserted
`return (receiver + 1);` inside a loop that increments `receiver`, which was
wrong on every iteration after the first.

1,448 functions still duplicate, all of them on the DFS fallback. Remaining work,
in order: labelled `break`/`continue` so a back edge or exit to a non-innermost
loop does not force a fallback; then partial structuring, where an edge the region
tree does not describe becomes a marked, singly-emitted tail rather than
discarding the whole function's structure. Both would let the DFS emitter be
deleted rather than maintained alongside. Rendering the header test as
`while (cond)` rather than `while (true)` plus `break` is only sound when the
header writes no register the condition reads, which excludes the common
increment-in-header shape, so it is not a general simplification.

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

Landed, `cargo test --workspace` green (276 tests), `fmt` and `clippy` clean:

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
distinct selectors, zero negative offsets, receiver resolved on 66.1%, arity
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
