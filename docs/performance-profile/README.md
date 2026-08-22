# Post-correctness performance profile

## Scope and the two frozen epochs

This record profiles performance evidence only. It changes no product source,
benchmark workload, checker, golden, threshold, or candidate implementation.

Two accepted records exist and must not be conflated:

- The named +61.425 percent combined, +45.225 percent emission-exclusive, and
  +458.069 percent serialization costs compare fixed `1371e425...` with the
  historical correctness head `630ec442...`. That exact accepted run is frozen
  by evidence commit `341035a...`.
- Repaired VAL-METRIC-005 later froze correctness head `5ba4b6d...`. Its exact
  checksum-bound run reports +81.355 percent combined, +69.494 percent
  emission-exclusive, and +409.555 percent serialization, with a +20.444
  percent allocation-count and +371.207 percent allocated-byte change.

The historical numbers remain the requested regression target. Fresh profiling
uses the exact retained VAL-METRIC-005 binaries. A surviving `630ec442` release
binary supplies a source-and-harness-exact corroborating profile; its digest is
not the accepted historical binary digest because the old record pruned that
binary. Therefore no compiled-layout or exact-cycle claim is transferred from
that survivor to the accepted old timing run. Retained raw timing and allocation
streams, output counts, source boundaries, and the current exact-binary profiles
provide the attribution.

## Method and binding

The retained timing evidence remains the statistical ruler: three warmups, 15
alternating warm-cache pairs, 33 disclosed cases, seed 1592614637, one thread,
a 120 second timeout, and a 2 GiB limit. Historical values in
[`historical-phase.tsv`](evidence/historical-phase.tsv) and
[`historical-cases.tsv`](evidence/historical-cases.tsv) were recomputed from
`341035a:docs/post-correctness/evidence/samples-{reference,candidate}.tsv` and
the two retained correctness JSON documents. Current values come from the raw
evidence audited at `d995b72`.

Fresh sampling ran twice per executable with Linux perf 7.2.0, `cpu/cycles/Pu`
at 999 Hz, DWARF call graphs, no warmup inside the captured process, one full
matrix pass, and correctness off only because the retained correctness pass is
already checksum-bound. The command shape was:

```text
nix shell --extra-experimental-features 'nix-command flakes' \
  nixpkgs#linuxPackages.perf -c \
  perf record -F 999 --call-graph dwarf -o PROFILE.data -- \
  BINARY run --matrix disclosed --warmups 0 --runs 1 --correctness off \
  --product-ref PRODUCT --binary-sha256 BINARY_SHA --out RESULT.json \
  --samples SAMPLES.tsv
```

Fresh allocation instrumentation ran the exact resource binaries with three
warmups and `--plant none`:

```text
nix develop --extra-experimental-features 'nix-command flakes' -c \
  BINARY resource --matrix disclosed --warmups 3 --plant none \
  --product-ref PRODUCT --binary-sha256 BINARY_SHA \
  --out RESOURCE.json --samples RESOURCE.tsv
```

All 165 allocation count, allocated byte, and peak-live cells on both sides
matched the committed resource TSVs exactly. Process RSS differed, as expected:
fresh reference/candidate maxima were 85,872,640 and 88,850,432 bytes versus
retained 85,803,008 and 88,940,544. The profiler lost zero samples. Raw perf
captures are not committed; their hashes, approximate cycle counts, binary
digests, host, and evidence bindings are in
[`binding.txt`](evidence/binding.txt). Derived hotspot ranges are in
[`perf-summary.tsv`](evidence/perf-summary.tsv).

## What the historical regression pays for

The +4,999,344,702 ns time-weighted combined increment is almost entirely two
phases:

| Phase | Delta | Share of net regression | Required obligation and avoidable work |
| --- | ---: | ---: | --- |
| emission exclusive | +3,345,208,647 ns | 66.914 percent | Required: preserve and render formerly omitted reachable paths. Avoidable: recursive hash-table state and repeated whole-line analysis and rewriting. |
| serialization | +1,677,326,437 ns | 33.551 percent | Required: serialize the added source and accounting. Avoidable: rescan every output line for each counter and allocate complete pretty-JSON buffers. |
| CFG | -17,137,972 ns | -0.343 percent | Faster, not a source of the regression. |
| IR | -1,441,413 ns | -0.029 percent | Faster, not a source of the regression. |

The required output is concrete. Exactly seven of 33 pseudocode artifacts
change. Across the matrix, emitted source grows from 28,752 to 170,021 lines,
an increase of 141,269 lines, and helper definitions grow from zero to 338.
Those seven cases account for 87.79 percent of serialization growth and 95.42
percent of emission growth. The three irreducible cases alone account for
85.91 percent of the combined regression. Their 126,551 added lines and 169
helpers are output that correctness acceptance explicitly retained; deleting,
truncating, or no longer serializing them is not an optimization.

The remaining cost is implementation shape. At `630ec442`, `emit_with_plan`
owns 72.29-72.50 percent of sampled cycles, `emit_block` owns 39.89-40.06
percent, and `SipHasher::write` alone owns 22.11-22.27 percent self time.
`apply_name_and_type_hints` owns 15.43-15.55 percent, while its implementation
collects identifier statistics and then repeatedly rewrites every line in
`crates/flutterdec-decompiler/src/passes/naming.rs`. `compact_lines` owns
10.35-10.59 percent and may traverse and clone line strings across as many as
16 passes in `passes/compaction.rs`.

Serialization is not just copying required bytes. `serialize_artifacts` calls
`quality_from_artifacts`, which visits every source line and calls
`source_text_counters`. That function separately counts one helper token, eight
argument names, 62 register spellings, and three markers through repeated
code-span scans. The profiler assigns 15.37-15.39 percent to
`quality_from_artifacts` and 10.61-10.65 percent to `for_each_code_span` at
`630ec442`, versus 3.85-4.05 and 4.33-4.47 percent respectively at `1371e42`.
The added source is required; dozens of scans over each added line are not.

## Current VAL-METRIC-005 allocation shape

The fresh exact resource run reproduces combined counts of 279,135,331 versus
336,200,562 and allocated bytes of 4,733,423,691 versus 22,304,232,271.
Emission-exclusive owns 77.29 percent of the count increase and 99.77 percent
of the net byte increase. Serialization owns the other 22.70 percent of the
count increase but only 0.44 percent of net bytes. CFG allocated bytes fall.

Allocation size, not only frequency, explains the byte result. On the current
head, emission averages 216.8 bytes per allocation for `fan-in/1024/base` and
187.0 for `multi-exit/1024/base`, up from 17.9 and 18.0 on `1371e42`. Those two
cases alone contribute 79.02 percent of the allocated-byte regression.
`irreducible/1024/base` adds another 16.08 percent. The five rows in
[`current-resource.tsv`](evidence/current-resource.tsv) account for 98.81
percent of the byte increase and 94.09 percent of the allocation-count increase.

The source mechanism is `FuncEmitter::block_ledger` in
`crates/flutterdec-decompiler/src/lib.rs`. For every reachable-unemitted block
and every traversal event, it starts another graph search whose pending entries
carry a full `Vec<usize>` path; every successor clones that path before pushing
it. The block ledger and its explanation paths are required output. Repeatedly
copying prefixes during their construction is not. The large average allocation
size on fan-in and multi-exit graphs is the predicted signature of those path
clones, and the exact resource cells reproduce it.

## Compiled behavior

The retained binaries are unstripped. Symbol tables and sampled call stacks
show these are real compiled boundaries, not source-only guesses:

| Product | text bytes | emission entry | bytes | `emit_block` bytes | naming bytes | serialization bytes |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| `1371e42` | 1,516,257 | `emit_with_provenance` | 67,519 | 11,880 | 37,490 | 5,415 |
| `630ec442` profile binary | 1,644,013 | `emit_with_plan` | 86,351 | 12,566 | 40,594 | 5,415 |
| `5ba4b6d` | 1,752,597 | `emit_with_plan` | 92,965 | 12,889 | 40,594 | 5,480 |

`for_each_code_span`, `rewrite_spans`, `code_brace_counts`, `dominators`,
`quality_from_artifacts`, and `serialize_artifacts` also remain distinct symbols
in the current release executable. Across the two current candidate captures,
`emit_with_plan` is 82.06-82.15 percent children, `emit_block` 49.12-49.17,
`apply_name_and_type_hints` 13.76-13.83, `serialize_artifacts` 12.52-12.57,
and `quality_from_artifacts` 12.10-12.14. This stability supports family-level
localization; it does not claim instruction-level causality from sampled data.

## Shares and Amdahl ceilings

For the requested historical epoch, current phase shares are 81.762 percent
emission and 15.554 percent serialization. Making all emission free would cap
speedup at 5.483x; making all serialization free caps it at 1.184x. The four
largest historical emission cases consume 63.920 percent of combined time.
Even a perfect optimization outside those cases and outside serialization has
little leverage.

At the exact VAL-METRIC-005 head, phase shares are 84.927 percent emission and
12.680 percent serialization, with remove-all ceilings of 6.634x and 1.145x.
The four-case emission prefix (`irreducible/1024`, `irreducible/256`,
`multi-exit/1024`, `fan-in/1024`) holds 81.192 percent of emission and 76.635
percent of combined time. A family that misses those cases cannot clear a five
percent combined MDE unless it removes a large fraction of what remains.

These are hard upper bounds, not predictions. The candidate ledger below uses
the narrower sampled or resource signature of each family.

## Ranked independent candidate families

No candidate is implemented or promoted here. These families are independent
of rejected E1 (register-name clone avoidance), E2 (DFS join-write closure
cache), and E3 (dense dominance rows).

### 1. Parent-linked reachable-unemitted explanations

- Predicted ceiling: 79.02 percent of the current allocated-byte regression is
  in the two signature cases. Those cases own 20.38 percent of current combined
  time, which is the workload timing ceiling; a trial should predict only a
  5-15 percent combined gain until direct timing proves more.
- Smallest boundary: only the explanation search inside
  `FuncEmitter::block_ledger` in `crates/flutterdec-decompiler/src/lib.rs`.
  Store one parent per visited block and materialize the winning path once,
  instead of cloning every pending path prefix.
- Correctness oracle: byte-identical 33-case artifacts and serialized block
  ledgers; all block-ledger, emission-taxonomy, CFG identity, determinism, and
  protected-oracle lanes; full CI.
- Resource guard: no allocation-count, allocated-byte, peak-live, or RSS cell
  may rise; `fan-in/1024/base` and `multi-exit/1024/base` allocated bytes must
  fall by at least 50 percent and combined must clear its comparison MDE.
- Rollback: revert the whole candidate if any explanation path/order changes,
  any resource cell rises beyond its exact baseline, or two 15-pair comparisons
  fail the declared target.

### 2. Single lexical pass for quality counters

- Predicted ceiling: 10.61-10.65 percent sampled cycles in
  `for_each_code_span`, bounded by the 15.37-15.39 percent
  `quality_from_artifacts` subtree and the historical 15.554 percent
  serialization phase. A reasonable first prediction is 7-10 percent combined.
- Smallest boundary: `source_text_counters` and its call in
  `quality_from_artifacts` in `crates/flutterdec-core/src/pipeline/quality.rs`.
  Tokenize each source line once and update all counters in that pass.
- Correctness oracle: byte-identical quality/report JSON and pseudocode for all
  33 cases, the public compatibility replay, quality tests, protected digests,
  and full CI.
- Resource guard: serialization allocations and bytes must not increase in any
  case; the seven enlarged-output cases must each improve serialization beyond
  their own MDE or the family does not explain the target.
- Rollback: revert if any counter changes, any artifact/report byte changes, a
  serialization resource cell rises, or two comparisons fail pooled and
  enlarged-output targets.

### 3. Batched naming analysis and rewrite

- Predicted ceiling: 15.43-15.55 percent sampled cycles in
  `apply_name_and_type_hints`, bounded by the 81.762 percent historical emission
  share. The first trial should predict 8-12 percent combined, not the full
  subtree, because type inference and rename selection remain required.
- Smallest boundary: `apply_name_and_type_hints` plus its identifier-stat and
  rename loop in `crates/flutterdec-decompiler/src/passes/naming.rs`. Collect
  stats for all identifiers in one lexical walk, decide the same rename table,
  then rewrite each line once.
- Correctness oracle: byte-identical pseudocode and provenance audit rows on the
  disclosed matrix and public compatibility replay; naming, rewrite-boundary,
  annotation-anchor, line-identity, determinism, and full CI lanes.
- Resource guard: emission allocation count and allocated bytes must not rise in
  any cell; all seven enlarged-output cases must improve emission and pooled
  combined must clear MDE.
- Rollback: revert on any byte, annotation, provenance, or line-identity drift,
  any resource regression, or failure after two full comparisons.

Only one family should be tested at a time against the immutable VAL-METRIC-005
reference. No result may reuse these ceilings as its measured effect, combine
families, change workload selection, or trade required output away.
