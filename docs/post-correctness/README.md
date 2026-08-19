# Post-correctness performance reference

This freezes `630ec442d951aac5704ae80287367912bfbfc388` as the exact
accepted emitter HEAD after correctness acceptance and before E1, E2, or E3 code.
The commit timestamp is `2026-08-19T00:14:00-04:00`; the accepted measured
window was `2026-08-19T01:45:08-04:00` through `2026-08-19T01:53:53-04:00`.
The product source references are the full `1371e42549472ec388f58bc1fd5dbdf96e8dcdd1`
and `630ec442d951aac5704ae80287367912bfbfc388` object names recorded in the
binding.

This run follows the completed clean release characterization recorded in
mission evidence `CFG-REPEAT-CHARACTERIZATION-20260819/SUMMARY.md` (sha256
`03ea446cf64ef6dc3ea38b339302015ac2e76f92cfa4438fa33d88d4639ba776`).
Its `SHA256SUMS` file is
`001670600edaca57d6510d4b3bd110f221c633330ec39d7e78e67a148d1fef20`.
That evidence finished at `2026-08-19T01:14:56-04:00` and binds
rustc 1.92.0 commit `ded5c06c...`, LLVM 21.1.8, Cargo 1.92.0 commit
`344c4567...`, and Nix 2.34.3. Clean non-incremental release trials at both
`7b8628a` and `630ec44` report `repeated_blocks=2`; the earlier unbound
`repeated_blocks=0` observation is excluded as uncontrolled build evidence,
not attributed to the optimizer or compiler.

## Binding and reproduction

Run from a clean checkout:

```
docs/post-correctness/run-refresh.sh /tmp/flutterdec-post-correctness-run
```

The runner builds `1371e42` with accepted harness patch `14413796...` and uses
the product instrumentation already present at `630ec44`. Before either clean,
non-incremental build, it replaces the complete `crates/flutterdec-bench` tree
with revision `4c127aba4e74fb6f8d486c4cb066586bb0d74846` and verifies accepted Git
tree object `83e06014b368736c1921a0da7949c7b6a0b76e97`. Both builds use the same
canonical path. It then proves the manifests equal, performs three warmups and
one correctness pass per binary, and runs 15 warm-cache pairs with alternating
order. The disclosed matrix remains 33 cases, seed 1592614637, matrix sha256
`76b617c6...`, and manifest sha256 `bfb16760...`.

Exact bindings and digests are in `evidence/binding.txt`. The reference binary
is `70535430...`; the post-correctness binary is `b729b5d6...`. The committed
sample streams contain 2475 rows per side: 15 pairs by 33 cases by five spans.
`refresh-attribution.py --audit-live evidence` audited all 30 raw JSON documents
and all 30 raw TSV streams before aggregation. The runner retained the audit,
aggregates, sample streams, manifests, warmups, order logs, binding, and
checksums, then pruned `evidence/raw/` and `evidence/bin/`. The live audit
observed 33/33 correctness cases on both sides, zero timeouts, peak RSS
73007104 bytes against 2 GiB, and a worst span residue of 0.000655 against the
0.02 limit. The earlier run that used the candidate's later test-only harness
tree is invalid and remains only in mission history, not accepted evidence.

## Separate correctness cost

This comparison measures the cost from `1371e42` to the correctness reference;
it is not an E1, E2, or E3 candidate delta. The time-weighted sum of per-case
medians rose 61.425 percent overall: emission rose 45.225 percent and
serialization rose 458.069 percent because preserved helper bodies now reach
the artifact. The paired median deltas were +21.388 percent combined, +12.943
percent emission, and +60.794 percent serialization. The statistics differ
because the time-weighted sum gives dominant large cases their actual cost,
while the paired aggregate gives each of 495 case-pairs equal weight.

## Refreshed attribution

| Phase | pre-correctness share | post-correctness share |
| --- | ---: | ---: |
| IR | 0.014820 | 0.009071 |
| CFG | 0.031160 | 0.017998 |
| emission exclusive | 0.908828 | 0.817618 |
| serialization | 0.044990 | 0.155538 |

Emission remains dominant at 81.762 percent. Removing it entirely gives an
Amdahl speedup ceiling of 5.483x, down from 10.968x before correctness because
serialization now owns 15.554 percent. The full 33-case shares are in
`evidence/attribution.txt`.

Allocation shape also moved. Total median allocations rose from 279135331 to
332152221 and bytes from 4733423691 to 5802445103. Post-correctness emission
owns 94.991 percent of allocations and 95.274 percent of bytes; serialization
owns 4.710 percent and 3.255 percent. CFG still reads zero because its
allocations are charged inside the enclosing emission counter span.

## Mechanical targets and candidate leverage

The rule was declared before this refresh: rank by target-phase median
nanoseconds and take the shortest descending prefix reaching at least 75
percent. For the dominant emission target the result is:

| Order | Case | share of emission | cumulative |
| ---: | --- | ---: | ---: |
| 1 | `irreducible/1024/base` | 0.366835 | 0.366835 |
| 2 | `irreducible/256/base` | 0.214729 | 0.581564 |
| 3 | `irreducible/64/base` | 0.109246 | 0.690810 |
| 4 | `multi-exit/1024/base` | 0.090970 | 0.781781 |

E1 addresses shared emission churn, so its measured ceiling is 0.817618 of
workload time. E2 addresses the four-case declined-emission prefix, whose
emission is 0.639198 of workload time. E3 addresses CFG, 0.017998 of workload;
its own 75 percent CFG prefix is eight 1024-block cases ending with
`diamond-chain/1024/light`, cumulative 0.803591 of CFG and 0.014463 of workload.
The source sites remain the recursive DFS emission at `emit.rs:1253-1450`,
helper emission at `helper_flow/summary.rs:74`, and the set-based dominator and
post-dominator solvers at `regions.rs:209-365`.

## Frozen trial decisions

Trial order is E1, E2, E3. E1 is first because it has the broadest leverage and
can require byte-identical artifacts; E2 is second because it changes the
declined path closest to correctness; E3 is last because its workload leverage
is below the five percent combined floor and must be judged on CFG cells.

Accept, kill, and stop rules remain those in research section 15, with the
four-case emission prefix above replacing the stale five-case list and the
eight-case CFG prefix replacing the stale discretionary pair. Every candidate
uses its own 15 paired deltas and `MDE = max(0.05, 3 * MAD)` separately for the
pooled target phase and every decision cell. No MDE number in this refresh is a
candidate threshold. Accept requires target improvement beyond that MDE,
unchanged artifacts where byte identity is claimed, no cell regression beyond
`max(0.10, MDE_cell)`, full correctness and CI, non-increasing target
allocations, limits, and reconciliation. Kill on any oracle or digest failure,
artifact drift for a byte-identical claim, excess regression, resource or span
failure, or failure to clear MDE after two comparisons. Stop after E1-E3 if no
candidate clears its own MDE and no untried family has leverage above 0.05, or
when residual target emission falls below 0.05 of workload.

## Evidence digests

| File | sha256 |
| --- | --- |
| `run-refresh.sh` | `66c31da6950a1849bcd3ecff9ea91ed5759d8fd500235003b4b914cb945a98ab` |
| `refresh-attribution.py` | `f60286ed2968b1acc6345792ccdbd72808b67c118668b4b0c11bea95d4b050cb` |
| `evidence/SHA256SUMS` | `57dd390d522820e286d0056aeb568daac63ee429930bad447670b40e23410c07` |
| `evidence/analysis.json` | `964e1e94da004a7366ca773bb8f6838a859596b123bd00c5e68e4cddac8e3ef5` |
| `evidence/attribution.txt` | `fcaab6988b9152294056a229fdf2544e9427978a33a049d17a5f14a47bf411e0` |
| `evidence/audit-live.txt` | `62e688f8f89f921ca3443a7a3f555a54e1ca737ee84a182c042414eee78691dd` |
| `evidence/samples-reference.tsv` | `21fbd60f13c7e6dba7307561bbd63760ff4c2cb9f5cfb34dee837f2e665ea93d` |
| `evidence/samples-candidate.tsv` | `cf54c3ec08c904beb4cdf93dacc296b6d5d5b0f594a2201b01d2db8b56f9f31d` |
| `evidence/warmup-reference.json` | `8ff8ff68856ad12a16ad426d88c4ec11e5001ee2e5d148a898ac7515a83c3a5b` |
| `evidence/warmup-candidate.json` | `b49e6db26f09b0406defef48ec7ec36096c169e4feb88afff0c8a4698af79990` |
| `evidence/binding.txt` | `9327304879374cc45168b42108d569078fbf45359359fa0f2f2893119fa63547` |
| `evidence/planned-pair-order.tsv` | `cb6d84f1525e1f22bbb22d43d7eb14271b246fadbd45a9883bdc7406784de98f` |
| `evidence/pair-order.tsv` | `cb6d84f1525e1f22bbb22d43d7eb14271b246fadbd45a9883bdc7406784de98f` |
| both manifest files | `bfb167600ee186d4e360958348cc8892e3dee2620f9dcdeaf9fcd60c20fd3bc7` |
