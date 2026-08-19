# Post-correctness performance reference

This freezes `630ec442d951aac5704ae80287367912bfbfc388` as the exact
accepted emitter HEAD after correctness acceptance and before E1, E2, or E3 code.
The commit timestamp is `2026-08-19T00:14:00-04:00`; the measured window was
`2026-08-19T01:18:47-04:00` through `2026-08-19T01:27:07-04:00`. At the start
and end, `HEAD` and `origin/research/ir-cfg-emitter` were both `630ec44`, and
`git log 630ec44..HEAD` plus the product-path diff were empty.

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

The runner builds `1371e42` with accepted harness patch `14413796...` and
builds exact `630ec44` directly, where the same harness is already embedded in
history. Both builds use the same canonical path. It then proves the manifest
equal, performs three warmups and one correctness pass per binary, and runs 15
warm-cache pairs with alternating order. The harness revision remains
`4c127aba4e74fb6f8d486c4cb066586bb0d74846`; the disclosed matrix remains 33
cases, seed 1592614637, matrix sha256 `76b617c6...`, and manifest sha256
`bfb16760...`.

Exact bindings and digests are in `evidence/binding.txt`. The reference binary
is `70535430...`; the post-correctness binary is `b729b5d6...`. The committed
sample streams contain 2475 rows per side: 15 pairs by 33 cases by five spans.
`refresh-attribution.py evidence` reproduces all tables below and audits the
raw run structure. The live audit observed 33/33 correctness cases on both
sides, zero timeouts, peak RSS 73007104 bytes against 2 GiB, and a worst span
residue of 0.000640 against the 0.02 limit.

## Separate correctness cost

This comparison measures the cost from `1371e42` to the correctness reference;
it is not an E1, E2, or E3 candidate delta. The time-weighted sum of per-case
medians rose 61.486 percent overall: emission rose 45.153 percent and
serialization rose 454.372 percent because preserved helper bodies now reach
the artifact. The paired median deltas were +20.642 percent combined, +12.795
percent emission, and +60.803 percent serialization. The statistics differ
because the time-weighted sum gives dominant large cases their actual cost,
while the paired aggregate gives each of 495 case-pairs equal weight.

## Refreshed attribution

| Phase | pre-correctness share | post-correctness share |
| --- | ---: | ---: |
| IR | 0.014999 | 0.009102 |
| CFG | 0.031644 | 0.018109 |
| emission exclusive | 0.908455 | 0.816574 |
| serialization | 0.045304 | 0.155526 |

Emission remains dominant at 81.657 percent. Removing it entirely gives an
Amdahl speedup ceiling of 5.452x, down from 10.925x before correctness because
serialization now owns 15.553 percent. The full 33-case shares are in
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
| 1 | `irreducible/1024/base` | 0.367286 | 0.367286 |
| 2 | `irreducible/256/base` | 0.216031 | 0.583317 |
| 3 | `irreducible/64/base` | 0.108711 | 0.692028 |
| 4 | `multi-exit/1024/base` | 0.090613 | 0.782641 |

E1 addresses shared emission churn, so its measured ceiling is 0.816574 of
workload time. E2 addresses the four-case declined-emission prefix, whose
emission is 0.639084 of workload time. E3 addresses CFG, 0.018109 of workload;
its own 75 percent CFG prefix is eight 1024-block cases through
`irreducible/1024/base`, cumulative 0.803477 of CFG and 0.014550 of workload.
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
| `run-refresh.sh` | `823b4accd54676c6056d48ae3fbf44828065c72e8ac43e3bc72475756db3b2cf` |
| `refresh-attribution.py` | `eb808c5ea881af0feeddf6695ebf07785db16250a9785b41d372a4a44b0f9d70` |
| `evidence/analysis.json` | `f2c6ec90de9f01013801773d4401ecf2ee87996717b299e7b8c16d3b4efe657a` |
| `evidence/attribution.txt` | `033b8a74012504e65e3d6cf958ac3f9ad9a22e009d10ff76346254b78b9c9ea5` |
| `evidence/samples-reference.tsv` | `f38abebf38706fc2afd0ac85136d8ee7d569f49dce7d72f26de396e64470ce90` |
| `evidence/samples-candidate.tsv` | `b7876566a68d5e6248925d43af8d5e9f35263664263123352bf8d3a1c21144b1` |
| `evidence/warmup-reference.json` | `2e09758199d136c0f562376f91cd4ae542379f2d724c995ed5b0b9182dac07a8` |
| `evidence/warmup-candidate.json` | `5c91b5b7671fc53494d807a42cc7404c0b9531a9acab34d93a288161b76b8c7a` |
| `evidence/binding.txt` | `5b76d7b9dea7fa0903b8148de5bc539d00f865d86815aca2b5fbf3f3a92d6d7d` |
| `evidence/planned-pair-order.tsv` | `cb6d84f1525e1f22bbb22d43d7eb14271b246fadbd45a9883bdc7406784de98f` |
| `evidence/pair-order.tsv` | `cb6d84f1525e1f22bbb22d43d7eb14271b246fadbd45a9883bdc7406784de98f` |
| both manifest files | `bfb167600ee186d4e360958348cc8892e3dee2620f9dcdeaf9fcd60c20fd3bc7` |
