# Invalidated baseline runs, in order

Every A/A run of the phase harness that was measured and then thrown away, why
it was thrown away, and what was left behind. None of these numbers may be used
as a noise floor. The valid baseline is
[baseline-ir-cfg-emitter.md](../baseline-ir-cfg-emitter.md) at harness
`8e7f08096434b614a1e8dc6d3092ff6a67bb44c9`.

The list is chronological. Each entry was invalidated by a defect the next
harness revision fixed, so the same 33-case matrix was re-measured from scratch
each time rather than patched.

## 1. Harness `1b11f7e`, fixed pair order

| Field | Value |
| --- | --- |
| Harness revision | `1b11f7e` |
| Patch sha256 | `e9faab161fe53a67b8375df493d08d42b5912f2f374d3ce41f6b0e53b1b1403d` |
| Output directories | `/tmp/bench-A`, `/tmp/bench-B` (second run partial) |
| Schedule | reference always first in the pair, candidate always second |
| Invalidated by | `b4b1d8c` |

Position was confounded with side. Of the 24 cells whose absolute paired delta
reached 2 percent, 20 favoured the candidate, which held the second position in
all 15 pairs. Second position is measurably the faster one, so the run reported
a schedule artefact as a product-scale difference.

Recorded numbers kept for comparison only:
[invalidated-2026-08-18-fixed-order.txt](invalidated-2026-08-18-fixed-order.txt).
The output directories were deleted at the `b4b1d8c` milestone. The second run
never completed and was never analysed.

## 2. Harness `b4b1d8c`, per-side worktrees

| Field | Value |
| --- | --- |
| Harness revision | `b4b1d8c` |
| Patch sha256 | `4d8e3682a5a1e600a49df5dbd111ac592152fa25ec06824dafda81b642e9b49f` |
| Output directories | `/tmp/zbench/aa-1`, `/tmp/zbench/aa-2` |
| Published in | commit `3aa2fe4`, under `docs/baseline/aa-1` and `docs/baseline/aa-2` |
| Schedule | alternating, verified |
| Invalidated by | `5aa4b4e` |

Alternation fixed defect 1: the direct position effect fell to about 0.2 percent,
symmetric across both sides. It exposed a larger one underneath. Each side was
built in its own worktree, and the absolute worktree path enters the crate
metadata hash, so two builds of one source revision produced two different
binaries:

| Build | sha256 | Bytes |
| --- | --- | --- |
| `pathtest/reference`, built then target deleted and rebuilt at the same path | `42e92daa321e7f59...` | 1869920 |
| `pathtest/candidate`, same source | `75303789a789d37f...` | 1869984 |

The result was a reproducible A/A skew of 1 to 2 percent on
`emission_exclusive` and about 1 percent on `combined`, in a direction fixed by
which path each side occupied. It survived holding position fixed, so it was not
the schedule, and it reproduced across both independent runs, so averaging more
pairs would not have removed it. That skew was published as a noise floor, which
is what makes the run void rather than merely imprecise.

The committed artifacts of both runs, and the `harness-b4b1d8c.patch` they were
bound to, are removed by the commit that adds this file. `/tmp/zbench` was
deleted.

## 3. Partial directory left after harness `5aa4b4e`

| Field | Value |
| --- | --- |
| Harness revision | `5aa4b4e` |
| Output directory | `/tmp/bench-A` |
| State when found | `raw/` present and empty, `.bench-pipeline.lock` 0 bytes, 0 samples, no `analysis.json`, no `binding.txt`, 0 bytes total |

Nothing was measured: the directory holds no sample file, no manifest and no
analysis, so there are no numbers to invalidate or retain. It was removed, along
with the scratch directory `/tmp/bench-run` and the console logs
`/tmp/bench-A-console.txt` and `/tmp/bench-B-console.txt`, before the valid runs
were started. No bench process was live at that point.

## What each fix cost

Every entry above was a bias the harness could not see in its own output, which
is why the gates now fail the run instead of reporting the number:

- Fixed order (1) is caught by restating the alternating schedule after the loop
  and diffing it against the order log the measuring function itself appended.
- Path-dependent builds (2) are caught by building both sides sequentially at one
  canonical path and by `scripts/bench-identity-gate.sh`, which refuses equal
  product refs whose binaries differ.
- The same gate now also refuses different product refs whose binaries match,
  which is the A/B direction of the same defect: a candidate whose change never
  reached the machine code would otherwise be measured, and the noise reported as
  its effect.
