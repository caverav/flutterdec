# Final performance disposition

## Decision

The final decision is **honest no-win**, and the bound final signoff does not
satisfy the frozen performance acceptance target. Candidate
`9b82e07fa62f97654aea5153d9fb6a2ef57a377a` remains the immutable E1 evidence
object, but no speed candidate was accepted over frozen post-correctness
reference `630ec442d951aac5704ae80287367912bfbfc388`.

The disclosed four-case target repeated E1's strong result: emission-exclusive
improved 15.0589 percent and combined improved 11.7943 percent. Both estimates
cleared their comparison-specific 5 percent MDE. The independently generated
six-case held-out comparison did not generalize: emission-exclusive improved
only 1.1321 percent and combined improved only 0.6329 percent. Both estimates
were inside their own 5 percent MDE and therefore miss the promotion rule. Six
disclosed serialization cells also exceeded their positive 10 percent per-cell
guard. These are failures of the frozen performance acceptance target, not merely
reasons to withhold a speed claim.

The E1 allocation measurements remain historical evidence only. All disclosed
and held-out artifacts are byte-identical between reference and candidate, all
correctness cases pass, and no allocation count, total-byte, or per-phase
peak-live cell regresses. The frozen final protocol did not accept E1, so its
product change was removed by forward commit `ecca9e6`. The historical E1, E2,
and E3 objects and ledgers remain available for audit. The allocation result is
not shipped and is not an accepted product win.

## Frozen performance acceptance failures

The bound held-out emission-exclusive estimate is `-0.0113209874` and the bound
held-out combined estimate is `-0.0063290226`. Each has MDE `0.05`; neither
clears MDE. The disclosed final audit also records these six serialization
cells above their positive `0.10` bounds:

| Case | Estimate | Bound |
| --- | ---: | ---: |
| `linear/64/base` | +0.1003255880 | +0.10 |
| `linear/256/heavy` | +0.1047375370 | +0.10 |
| `diamond-chain/64/heavy` | +0.1051061363 | +0.10 |
| `diamond-chain/256/heavy` | +0.1071999275 | +0.10 |
| `diamond-chain/1024/light` | +0.1025175223 | +0.10 |
| `multi-exit/64/base` | +0.1008626704 | +0.10 |

A later fresh run reported no per-case violation, but it is not the sealed final
draw and cannot replace or repair this checksum-bound result. The frozen
samples, seed, thresholds, rulers, audits, and checksums remain unchanged.

## Frozen bindings and chronology

The scoring reference was frozen before candidate work at
`630ec442d951aac5704ae80287367912bfbfc388` (commit time
`2026-08-19T00:14:00-04:00`). The immutable candidate is
`9b82e07fa62f97654aea5153d9fb6a2ef57a377a` (commit time
`2026-08-19T02:47:20-04:00`). Timing uses accepted harness
`4c127aba4e74fb6f8d486c4cb066586bb0d74846`, tree
`83e06014b368736c1921a0da7949c7b6a0b76e97`, and patch SHA-256
`14413796ca8a89cc1328497b5c87629b1c55f945ec58e73eebb3838df0700460`.
The additive resource ruler uses `b0e615785b28e7e58aa06dd1b929dd58acf06e53`
and is accepted/protected through
`f4eb2d87d0bc8649addce2a2522ee8a56e856ca1`; its overlay digest is
`eda7c8bff8207fac64d3b9b9f4ce88e10e1805fb5a9a327453b94b079b437e0e`.

All four timing/resource binaries were built and bound by
`2026-08-19T04:21:14-04:00`. Only then did the sealed runner draw 128-bit seed
`522fd89869ed6662b16def9f47e45194`. The seed and manifest were neither printed
nor inspected before both product revisions were bound. The first held-out run
started at `2026-08-19T04:31:55-04:00`; all held-out timing and resource work
finished at `2026-08-19T04:34:19-04:00`; disclosure occurred only afterward.
The held-out manifest SHA-256 is
`490e84dafdc8e9a4fe80fe8ddab4bfbe115e141924ab9dfd8fc05f5cfc822830`
and matrix SHA-256 is
`797fa5817a5cdb025b2ab869b1ad32215c648c6ec3144d6ca94a7668891f1af3`.
Both revisions use that exact manifest and the same six workload digests.

Full commit/tree, binary, compiler, Cargo, Nix, flake, toolchain, timing, and
resource digests are in `evidence/binding.txt`. The four binary SHA-256 values
are `3d336d00...` and `14225324...` for timing, and `bd3798a0...` and
`878e1c86...` for resource reference/candidate respectively.

## Protocol and results

Each comparison used three preliminary warmups per side followed by 15
warm-cache paired runs. Pair order alternated reference-first and
candidate-first. For a pair, target score is the sum of target-case
nanoseconds; delta is `(candidate - reference) / reference`. The estimate is
the median paired delta, noise is MAD of those 15 deltas, and MDE is
`max(0.05, 3 * MAD)` separately for each comparison. The disclosed target is
the frozen four-case emission prefix. The held-out target is all six generated
mixed-topology cases.

| Matrix | Score | Estimate | MAD | MDE | Result |
| --- | --- | ---: | ---: | ---: | --- |
| disclosed | emission-exclusive | -0.15058869 | 0.00401237 | 0.05000000 | clears MDE |
| disclosed | combined | -0.11794302 | 0.00441203 | 0.05000000 | clears MDE |
| held-out | emission-exclusive | -0.01132099 | 0.00452143 | 0.05000000 | no win |
| held-out | combined | -0.00632902 | 0.00369699 | 0.05000000 | no win |

Every one of 33 disclosed and six held-out correctness cases passed on both
revisions, with exact artifact SHA-256 equality. Every case has its own paired
phase results and MDE in `evidence/final-audit.json`. No held-out case exceeded
its positive `max(0.10, case MDE)` regression bound. The six disclosed
serialization failures are recorded there without suppression.

The additive single-thread resource pass records allocation count, total
allocated bytes, peak live bytes, and process RSS for each case and disjoint
phase. Candidate maximum delta is 0.0 for all three allocation metrics on both
matrices. Peak process RSS was 85,962,752 bytes disclosed and 96,722,944 bytes
held-out, below 2 GiB. Worst timing-span residue was 0.000594 disclosed and
0.000007 held-out, below 0.02. No timeout, correctness, artifact, resource, or
protected-ruler failure occurred.

For the historical E1 object, the disclosed candidate emission share is
0.794612, giving a remove-all-emission Amdahl ceiling of 4.869x, down from the
frozen post-correctness 5.483x ceiling. The held-out workload-specific share is
0.880360 and its analogous ceiling is 8.358x. Both are historical
characterization, not shipped results and not used to move the frozen target.

## Candidate-family disposition

Exactly the frozen first-round families were attempted, in order:

- E1 preserved immutable evidence object `9b82e07`: disclosed emission/combined
  estimates were -16.074/-14.757 percent in its ledger, all target cases
  cleared MDE, artifacts and guards passed, and allocations fell. Final
  held-out evidence failed MDE, and the final protocol did not accept the
  product change. Forward commit `ecca9e6` removes it from the shipped branch.
- E2 rejected immutable comparison object
  `b2f6b503cc7351fce2cf1820b67081688eac16fa`: pooled disclosed scores
  improved, but `irreducible/64/base` emission was +1.458 percent and failed
  the required negative 5 percent MDE. No E2 product commit was retained.
- E3 rejected immutable comparison object
  `63390e154ea0a21b09fd520b163865a6d1d0b6bb`: pooled CFG was +3.439
  percent and three 1024-block linear cells regressed 54.189 through 58.106
  percent. No E3 product commit was retained.

The residual disclosed emission opportunity is still material, but E2 and E3
exhausted the frozen first round and E1 failed held-out MDE. The stop rule is
therefore met: stop without inventing a win; a later mission would need a new
predeclared family and new independent held-out draw.

## Correctness cost kept separate

The earlier `1371e42549472ec388f58bc1fd5dbdf96e8dcdd1` to post-correctness
`630ec44` comparison remains solely in `docs/post-correctness/`. Its
time-weighted combined cost was +61.425 percent, with emission +45.225 percent
and serialization +458.069 percent; its paired medians were +21.388 percent
combined, +12.943 percent emission, and +60.794 percent serialization. None of
those values is included in E1 scoring or described as a candidate regression.

## Reproduction and evidence

The final command was:

```text
export FLUTTERDEC_RUN_ROOT="${FLUTTERDEC_RUN_ROOT:?set a writable run root}"
TMPDIR="$FLUTTERDEC_RUN_ROOT/final-tmp" \
  docs/final-performance/run-final.sh \
  "$FLUTTERDEC_RUN_ROOT/final-performance-run"
```

An initial pre-measurement execution exposed a Bash local-declaration error and
stopped after builds, before any manifest or run. Its private seed was never
disclosed and its output was deleted. The corrected command rebuilt and rebound
all four binaries, drew fresh entropy, and completed every disclosed and
held-out pair. Its first audit intentionally stopped on the disclosed per-cell
failures before disclosure; `finalize-existing.sh` then recorded those failures
as honest no-win rather than asserting them away, disclosed the same seed only
after both runs were complete, and pruned raw/binary staging.

`evidence/SHA256SUMS` binds all 30 retained files. Retained evidence includes
both manifests, warmup correctness documents, 15-pair sample streams, pair
orders, all-cell aggregates, separate resource documents and streams, the
fail-closed final audit, chronology/bindings, and the reproducible seed. No raw
process document, executable, build tree, or mutable cache is committed.
