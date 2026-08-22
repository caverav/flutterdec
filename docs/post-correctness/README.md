# Current post-correctness performance freeze

This directory freezes the accepted performance reference before any new
candidate work. It compares fixed product `1371e42549472ec388f58bc1fd5dbdf96e8dcdd1`
with correctness head `5ba4b6d30604606c04b5b742eaf9469adc1c729d`.
The preceding harness commit changes only the runner and auditor; the evidence
commit changes only this directory. Neither changes product code, workloads,
goldens, checkers, thresholds, or scoring.

## Reproduce

Run from a clean checkout with enough space for four release builds:

```text
TMPDIR=/home/camilo/flutterdec-metric-tmp \
docs/post-correctness/run-refresh.sh \
  /home/camilo/flutterdec-metric-run
```

The runner uses Nix, a single canonical build path, `CARGO_INCREMENTAL=0`,
unset Rust flags, a 120 second timeout, and a 2 GiB memory limit. It builds both
revisions with timing harness `4c127aba4e74fb6f8d486c4cb066586bb0d74846`
and accepted patch SHA-256 `14413796...`. The additive resource ruler is
`b0e615785b28e7e58aa06dd1b929dd58acf06e53`; its exact retained patch is
`evidence/resource-overlay.patch` with SHA-256 `7f1ef7a3...`.

The disclosed workload has 33 cases, seed 1592614637, matrix SHA-256
`76b617c6...`, and manifest SHA-256 `bfb16760...`. Each binary receives three
unmeasured warmups and one 33-case correctness pass. Measurement then runs 15
warm-cache interleaved pairs with alternating first position. Resource passes
are single-threaded and record allocation count, allocated bytes, peak live
bytes, and process peak RSS for every case and phase.

## Retained evidence

`evidence/` is 9,818,983 bytes before Git compression. Its `SHA256SUMS` binds
84 files and has SHA-256 `e5eb7aba...`. The directory retains:

- four release binaries and their binding digests;
- 30 raw timing JSON documents and 30 raw TSV sample streams;
- two warmup and correctness documents, two manifests, and both pair-order logs;
- two resource JSON documents and two 165-row resource TSV streams;
- 34 start/end chronology rows proving sequential non-overlap;
- collected sample streams, independent aggregation, audit output, environment,
  exact commands, console transcript, harness patches, and checksums;
- a preflight record binding the exact controlling HEAD, empty porcelain status,
  command, and timestamp before the runner created or replaced any output.

The four binary SHA-256 values are:

| Binary | SHA-256 |
| --- | --- |
| timing reference | `0676036bfcf58abb4f71c2e04111ef63c6506e5016e365b228886eee590d25cf` |
| timing correctness head | `ff696e5ed2857b17f30f7524aa57c246d959aa76eb9019d799820666f59532db` |
| resource reference | `f868a2e3b7f542f78b581a84b0405db81d4439d9873022afe182cf0b0eb23a86` |
| resource correctness head | `557c3b19b8618df341093af965d321d11b3e78a0601d075d3deb6cd83f4963e7` |

The clean-clone run was `2026-08-22T10:47:52-04:00` through
`2026-08-22T10:59:43-04:00`. All 33 correctness artifacts matched on both
sides, no timeout fired, the worst phase residue was 0.00064561, and maximum
RSS across timing and resource lanes was 88,940,544 bytes. The preflight binds
controlling revision `d5d1140bf68dc26f75f1f482469808b23d6143fb` and an empty
porcelain status.

## Metrics

The aggregate is descriptive correctness cost, not a candidate result.

| Phase | median paired delta | MAD | MDE |
| --- | ---: | ---: | ---: |
| IR | +0.063298 | 0.029494 | 0.088481 |
| CFG | +0.020819 | 0.031048 | 0.093144 |
| emission exclusive | +0.122142 | 0.036058 | 0.108175 |
| serialization | +0.486564 | 0.060299 | 0.180896 |
| combined | +0.185034 | 0.049835 | 0.149505 |

Time-weighted combined cost rose 81.355 percent. At the correctness head,
emission is 84.927 percent and serialization is 12.680 percent of combined
time. The resource ruler reports reference/correctness-head combined allocation
counts of 279,135,331 / 336,200,562 and allocated bytes of 4,733,423,691 /
22,304,232,271. Maximum per-cell peak-live bytes are 32,060,411 / 32,036,915.

## Independent audit

Run:

```text
nix develop --extra-experimental-features 'nix-command flakes' -c \
  python3 docs/post-correctness/refresh-attribution.py \
  docs/post-correctness/evidence
```

The audit independently reconstructs collected samples from every raw TSV,
matches every raw JSON binding and timing value, recomputes all medians, MADs,
MDEs, phase shares, allocation fields, peak-live fields, and the aggregate,
checks binary and workload digests, and proves chronology and pair order. The
final output begins:

```text
checksums=84/84
PASS live raw audit before aggregation
raw_documents=30 sample_streams=30 measured_passes=990
pair_order=30/30 alternating correctness=33/33_both workloads=33_unique
chronology=34/34 non_overlapping resource_rows=330 raw_lanes_skipped=0
```

The committed audit has no retained-summary or raw-pruning mode. Any missing raw
directory or document fails before attribution, and every successful audit runs
the same binding, chronology, overlap, sample, aggregate, resource, binary, and
checksum checks.
