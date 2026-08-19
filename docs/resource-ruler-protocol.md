# Auxiliary phase resource ruler

This ruler closes VAL-METRIC-002 without changing the accepted timing ruler,
candidate order, target selection, or scores. Timing remains bound to
`4c127aba4e74fb6f8d486c4cb066586bb0d74846`. Resource scoring is a separate
command over frozen reference `630ec442d951aac5704ae80287367912bfbfc388`
and immutable final candidate `9b82e07fa62f97654aea5153d9fb6a2ef57a377a`.

## Allocator semantics

The resource allocator prefixes every system allocation with an aligned,
fixed-size header holding magic, reset epoch, requested size, and owner phase.
The prefix preserves the caller's alignment and is not visible to the caller.
Instrumentation uses only constant-initialized thread-local cells, fixed arrays,
integer operations, and the system allocator call already required by the
program. It performs no allocation, lock, formatting, IO, or recursion. Every
case reports an instrumentation recursion count and fails if it is nonzero.

- `alloc` and successful `alloc_zeroed` each add one event and the requested
  bytes to the current leaf phase; live bytes rise by requested size.
- `alloc_zeroed` zeroes the caller-visible region. The private header is written
  after the system call and does not change those bytes.
- Successful `realloc` ends the old owner lifetime and records one allocation
  event of `new_size` owned by the current leaf phase. Failure preserves the old
  allocation and records nothing. System `realloc` preserves caller bytes.
- `dealloc` records no allocation event. It subtracts the header's requested
  size from the allocation's original owner and current combined live value,
  even if deallocation occurs in another phase.
- Peak live bytes are the maximum outstanding requested bytes owned by a phase
  during the current reset epoch. Combined peak is the maximum sum of all four
  owners, not the sum of phase peaks.
- Reset increments an epoch and zeroes all metrics. Later destruction of an
  allocation from an older epoch is ignored, so setup and previous cases cannot
  subtract from a new case. Epoch wrapping skips zero.
- State is thread-local. The runner records one thread; another thread has its
  own phase stack, epoch, metrics, and recursion counter.
- Phase entry pushes onto a fixed eight-entry stack. The active leaf alone owns
  allocations. CFG entry temporarily replaces emission-exclusive ownership;
  RAII exit restores its parent during normal return and panic unwinding.

The four disjoint owners are IR, CFG, emission-exclusive, and serialization.
Every allocation made while a protected phase is active belongs to exactly one
owner. The combined row is derived concurrently from those owners; it is not a
fifth allocation owner. Allocations outside a protected phase are deliberately
unscored and cannot be charged to a case.

## Execution and guards

`scripts/bench-resource.sh OUT_DIR` reconstructs the accepted `4c127ab` timing
harness tree first, verifies its Git tree object, overlays the four fixed
resource files by Git blob identity, and builds both products sequentially at
one canonical path. It runs every disclosed case after three warmups, separately
from timing, and emits JSON plus TSV count, total bytes, peak live bytes, and
process peak RSS for each case and phase. Workload manifests must match.

The audit rejects a missing/zero/duplicate cell, non-positive RSS, a candidate
count/bytes/peak regression above 5 percent, or a non-repeatable no-op control.
The CFG graph clone and emitter block-vector clone must each add count and bytes,
raise their own phase peak above 5 percent on at least one case, and leave every
other exclusive phase byte-identical. This last rule makes a nested CFG charge
to emission, a dropped charge, or any other phase misattribution fail.

## Protected digest inventory

The checker requires this exact ordered set. Digests are filled by the atomic
protocol adjudication commit and recomputed before Cargo work. Deletion, stale
digest, extra/missing row, duplicate row, and loader bypass all fail closed.

| Path | sha256 |
| --- | --- |
| `crates/flutterdec-bench/Cargo.toml` | `98dbc4b430302d76c4cf4716dfdd781ea354f26195514d1f9b844e79f97a7040` |
| `crates/flutterdec-bench/src/main.rs` | `c8fefa460ecc3dd7a919f6577367d5e967386c277630fd3e1493d4dd53b6ac34` |
| `crates/flutterdec-bench/src/measure.rs` | `49dfc3fcb2a33fa2903f9f19ec0c02915fb15cc2cba86a9d4f8e6d72535570b8` |
| `crates/flutterdec-decompiler/src/lib.rs` | `bfaec5e3c54488f60cc6057fb8743847bed1fb0c8f1db11743a400d9c76d3cd7` |
| `crates/flutterdec-decompiler/src/control_flow/structured.rs` | `5d748ff24c73049402db3511c9346bfeba8730002757c228da6bc4bb809b4d02` |
| `scripts/bench-resource.sh` | `93a6932301bb37452c41149b237d3c46b4244d2b3ee2d76a60ca11dd35c101db` |
| `scripts/audit-resource-evidence.py` | `4cea4c88f15144a5cdf34a724751a173d11e91f7d7cda5641c156cd48faaf220` |
| `scripts/check-resource-ruler.py` | `c99345bea216e0c8f42eb854d826d5d040230f37bf571b81eda81c80d6fe2716` |
| `scripts/ci-check.sh` | `9dac174b7a2a4e3a0d14d182d292dac0dbc8b6c63679e859da7cc8dad21ea45e` |

## Ruler-change adjudication

This is a protected auxiliary measurement, not a new timing selector. It does
not alter the disclosed matrix, seed, timing spans, estimator, MDE, candidate
order, accepted timing harness, frozen product refs, or any product behavior
without `bench-spans`. The only product-source additions are feature-gated
phase ownership and explicit clone plants. The resource command is never called
by timing selection. Any future byte change in the inventory requires an atomic
adjudication with old/new digests, refreshed lifecycle and plant evidence, and
an explicit statement that timing selection was not rerun.

The 2026-08-19 block-ledger adjudication changes
`crates/flutterdec-decompiler/src/lib.rs` from
`1b5c12d8cd0669e73867b1115986a11d191bc416d1fdbd3a393e394aa2611df5` to
`e90521c07a1af522e546a93a37b4932a668c3c16cd6a04594b18e71aef79f0bb`.
The change adds emission reconciliation and invalid-CFG identity reporting;
the feature-gated resource allocator, phase ownership, CFG clone plant, and
emitter clone plant are byte-for-byte unchanged. Their lifecycle, nesting,
panic cleanup, and plant checks were refreshed in Nix with the atomic product
commit. No timing selection, candidate order, score, threshold, sample, seed,
accepted harness, frozen reference, or immutable candidate was rerun or changed.

The 2026-08-19 fail-closed ledger adjudication changes
`crates/flutterdec-decompiler/src/lib.rs` from
`e90521c07a1af522e546a93a37b4932a668c3c16cd6a04594b18e71aef79f0bb` to
`92c27aab0358d37bb098cbbba6f8ffac03e0a44e9f34781cbf81902da44d135f`.
The change stores immutable valid-graph topology and concrete traversal paths,
and validates the invalid-CFG outcome before returning it. The feature-gated
resource allocator, phase ownership, CFG clone plant, and emitter clone plant
are byte-for-byte unchanged. No timing selection, candidate order, score,
threshold, sample, seed, accepted harness, frozen reference, or immutable
candidate was rerun or changed.

The 2026-08-19 raw-graph evidence binding changes
`crates/flutterdec-decompiler/src/lib.rs` from
`92c27aab0358d37bb098cbbba6f8ffac03e0a44e9f34781cbf81902da44d135f` to
`038b55cf34dd8b4236500c5e852f7fb2f48ef1b3b940f4219c4df0904f514440`.
The change retains a typed raw-graph witness beside an invalid outcome so its
existing digest can be recomputed during public ledger validation. The
feature-gated resource allocator, phase ownership, CFG clone plant, and emitter
clone plant are byte-for-byte unchanged. No timing selection, candidate order,
score, threshold, sample, seed, accepted harness, frozen reference, or
immutable candidate was rerun or changed.

The 2026-08-19 rejected-graph witness verification changes
`crates/flutterdec-decompiler/src/lib.rs` from
`038b55cf34dd8b4236500c5e852f7fb2f48ef1b3b940f4219c4df0904f514440` to
`7c30a534afa55197e87e220d05baed1807008cf7d512789fd4c22ac6529c154b`.
The change centralizes the existing FNV digest over the typed raw-graph witness
whose graph validity is checked by the same admission routine as production.
The feature-gated resource allocator, phase ownership, CFG clone plant, and
emitter clone plant are byte-for-byte unchanged. Their lifecycle, nesting,
panic cleanup, retained no-op, and both phase-isolated clone-plant checks were
refreshed in Nix. No timing selection, candidate order, score, threshold,
sample, seed, accepted harness, frozen reference, or immutable candidate was
rerun or changed.

The 2026-08-19 branch-target oracle adjudication changes
`scripts/ci-check.sh` from
`b1600c29ccbda98b751e8a337c6aa875dfc56eef3dc66efb9edb00952c78188c` to
`354a21e6ecdfef30e9bc8ea91dbdfd7a33ca8062c4d537c81648328c7e5aeb43`.
The only change names the new protected `flutterdec-ir` integration target in
the existing oracle-loader phase. The resource checker invocation, resource
files, feature-gated allocator, phase ownership, clone plants, thresholds,
timing selection, candidate order, scores, samples, seed, accepted harness,
frozen reference, and immutable candidate are unchanged. The resource ruler
self-test and clean loader check were refreshed in Nix; timing selection was not
rerun.

The 2026-08-19 DFS loop-relation adjudication changes
`crates/flutterdec-decompiler/src/lib.rs` from
`7c30a534afa55197e87e220d05baed1807008cf7d512789fd4c22ac6529c154b` to
`bfaec5e3c54488f60cc6057fb8743847bed1fb0c8f1db11743a400d9c76d3cd7`, and
`scripts/ci-check.sh` from
`354a21e6ecdfef30e9bc8ea91dbdfd7a33ca8062c4d537c81648328c7e5aeb43` to
`6cb19f223bde0510e2c70eac4c7b759b6fe04d57e74dce9e17ffcb1ce89c6389`.
The product change adds only the DFS dominance cache; the CI change names the
new protected public fixture target. Resource allocator code, phase ownership,
clone plants, thresholds, timing selection, candidate order, scores, samples,
seed, accepted harness, frozen reference, and immutable candidate are unchanged.
The resource ruler and full CI were refreshed; timing selection was not rerun.

The 2026-08-19 entry-loop merge adjudication changes
`scripts/ci-check.sh` from
`6cb19f223bde0510e2c70eac4c7b759b6fe04d57e74dce9e17ffcb1ce89c6389` to
`94cc9b90e935bb1ecdce4280a31315337d5345b5c74b165e395c74de6dd608f5`.
The only change names the new protected decompiler integration target in the
existing oracle-loader phase. The resource checker invocation, resource files,
feature-gated allocator, phase ownership, clone plants, thresholds, timing
selection, candidate order, scores, samples, seed, accepted harness, frozen
reference, and immutable candidate are unchanged. The resource ruler and full CI
were refreshed; timing selection was not rerun.

The 2026-08-19 block-ledger contract protection adjudication changes
`scripts/ci-check.sh` from
`94cc9b90e935bb1ecdce4280a31315337d5345b5c74b165e395c74de6dd608f5` to
`9dac174b7a2a4e3a0d14d182d292dac0dbc8b6c63679e859da7cc8dad21ea45e`.
The only change names the new protected `block_ledger_contract` decompiler
integration target in the existing oracle-loader phase. The resource checker
invocation, resource files, feature-gated allocator, phase ownership, clone
plants, thresholds, timing selection, candidate order, scores, samples, seed,
accepted harness, frozen reference, and immutable candidate are unchanged. The
resource ruler and full CI were refreshed; timing selection was not rerun.

The 2026-08-19 CI guard parity change moves no digest in this inventory. Every
protected path above, including `scripts/ci-check.sh`, is byte-unchanged: the
change is additive on the GitHub side only, where
`.github/workflows/ci.yml` gains `nix develop -c python3
scripts/check-resource-ruler.py` as a step of its own, identical to the local
invocation this checker already requires in `scripts/ci-check.sh`. That command
is now also matched by value in both CI files by
`the_protected_oracle_loader_chain_is_intact`, so a lane cannot drop the resource
inventory while the other lane still runs it. The resource files, feature-gated
allocator, phase ownership, clone plants, thresholds, timing selection, candidate
order, scores, samples, seed, accepted harness, frozen reference, and immutable
candidate are unchanged. The resource ruler self-test and clean loader check were
refreshed in Nix, and the checker was proved fail-closed through the workflow's
own command line by deleting `scripts/bench-resource.sh` in a disposable
worktree; timing selection was not rerun.
