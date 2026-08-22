# Current post-correctness performance freeze

This directory freezes the accepted performance reference before any new
candidate work. It compares fixed product `1371e42549472ec388f58bc1fd5dbdf96e8dcdd1`
with correctness head `5ba4b6d30604606c04b5b742eaf9469adc1c729d`.
The evidence commit changes benchmark evidence and audit tooling only. It does
not change product code, workloads, goldens, checkers, thresholds, or scoring.

## Reproduce

Run from a clean checkout with enough space for four release builds:

```text
TMPDIR=/home/camilo/flutterdec/.post-correctness-tmp/val-metric-005-tmp \
docs/post-correctness/run-refresh.sh \
  /home/camilo/flutterdec/.post-correctness-tmp/val-metric-005-run
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

`evidence/` is 9,798,051 bytes before Git compression. Its `SHA256SUMS` binds
83 files and has SHA-256 `0b51d8cd...`. The directory retains:

- four release binaries and their binding digests;
- 30 raw timing JSON documents and 30 raw TSV sample streams;
- two warmup and correctness documents, two manifests, and both pair-order logs;
- two resource JSON documents and two 165-row resource TSV streams;
- 34 start/end chronology rows proving sequential non-overlap;
- collected sample streams, independent aggregation, audit output, environment,
  exact commands, console transcript, harness patches, and checksums.

The four binary SHA-256 values are:

| Binary | SHA-256 |
| --- | --- |
| timing reference | `dd1e86fb83880b61f0a0111bc9c303f24d1a1d1d0773b95fc26f20d0e524eb38` |
| timing correctness head | `056921b335af90da4f7b0d27556c0ba7136f8ed75841dca345f521072af9457c` |
| resource reference | `9c11ac759ca7704ebf034a36a32f82defedc7337bd1d513610c8cda8acda0ebc` |
| resource correctness head | `248a02e16ef61706efbbb53dd39ee9ce96ad438c925e6cccc00da11038f014c2` |

The measured window was `2026-08-22T10:04:39-04:00` through
`2026-08-22T10:16:33-04:00`. All 33 correctness artifacts matched on both
sides, no timeout fired, the worst phase residue was 0.00066139, and maximum
RSS across timing and resource lanes was 88,846,336 bytes.

## Metrics

The aggregate is descriptive correctness cost, not a candidate result.

| Phase | median paired delta | MAD | MDE |
| --- | ---: | ---: | ---: |
| IR | +0.059337 | 0.029575 | 0.088724 |
| CFG | +0.023608 | 0.029967 | 0.089901 |
| emission exclusive | +0.184112 | 0.038873 | 0.116618 |
| serialization | +0.541826 | 0.075146 | 0.225439 |
| combined | +0.243944 | 0.058289 | 0.174867 |

Time-weighted combined cost rose 87.223 percent. At the correctness head,
emission is 84.217 percent and serialization is 13.429 percent of combined
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
checksums=83/83
PASS live raw audit before aggregation
raw_documents=30 sample_streams=30 measured_passes=990
pair_order=30/30 alternating correctness=33/33_both workloads=33_unique
chronology=34/34 non_overlapping resource_rows=330 raw_lanes_skipped=0
```

The first post-run audit stopped because its new resource parser omitted the
JSON document's separate `combined` object. The measurement processes had
already finished and all raw documents were retained. The parser was repaired,
the completed evidence was audited without rerunning or editing measurements,
and the console transcript preserves that sequence.
