# VAL-METRIC-002 resource evidence

This directory is the durable output of:

```text
TMPDIR=/home/camilo/flutterdec/.post-correctness-tmp/resource-tmp \
  scripts/bench-resource.sh \
  /home/camilo/flutterdec/.post-correctness-tmp/val-metric-002
```

The run completed with exit 0. It measured all 33 disclosed cases for frozen
post-correctness reference `630ec442d951aac5704ae80287367912bfbfc388`
and immutable final candidate `9b82e07fa62f97654aea5153d9fb6a2ef57a377a`.
The accepted timing harness remains `4c127aba4e74fb6f8d486c4cb066586bb0d74846`
with tree `83e06014b368736c1921a0da7949c7b6a0b76e97`; timing selection was not
rerun. Auxiliary resource scoring used `b0e615785b28e7e58aa06dd1b929dd58acf06e53`
and overlay digest `eda7c8bff8207fac64d3b9b9f4ce88e10e1805fb5a9a327453b94b079b437e0e`.

Reference binary SHA-256 is
`75110be536aebaefe0c12339e0872d2f6ceab03da85920f2f468905d34d7d5d6`;
candidate binary SHA-256 is
`fb820370952f0da1a613024a4bd05b8b28140922dfd2dfffc8af046657af5665`.
The two manifests are byte-identical at
`bfb167600ee186d4e360958348cc8892e3dee2620f9dcdeaf9fcd60c20fd3bc7`.

Each binding has exactly 165 machine-readable rows: 33 cases times IR, CFG,
emission-exclusive, serialization, and derived combined. Every row has positive
allocation count, total allocated bytes, peak live bytes, and process peak RSS.
Reference RSS ranged from 85,839,872 to 85,925,888 bytes; candidate RSS ranged
from 85,831,680 to 85,975,040 bytes.

No candidate cell regressed. Maximum candidate relative delta was 0.0 for each
of count, total bytes, and peak live bytes. The largest reductions were 63.6866
percent in emission-exclusive count and 14.2461 percent in emission-exclusive
total bytes, both on `irreducible/1024/base`; peak live bytes were unchanged.
Source review of the sole candidate diff confirms it adds no graph or block-set
clone: it replaces eager cloning before duplicate rejection with contains-first
insertion in `control_flow/emit.rs`.

The no-op lifecycle control reproduced all 165 resource metric triples exactly.
The CFG graph-wide clone raised its own peak by 104.6317 percent and failed the
5 percent guard in two cells. The emitter block-vector clone raised its own peak
by 134.1554 percent and failed in 18 cells. Neither plant changed another
exclusive phase, so nested CFG work cannot pass while charged to emission.
`audit.json` is the fail-closed result; `SHA256SUMS` binds every retained
machine output plus `binding.txt`.

Four additional disposable plants were run after the retained measurement and
all exited 1 as required. Deleting `measure.rs` produced `protected file deleted
or not regular`; changing one byte in the protected audit produced `stale
digest`; replacing the CI checker invocation produced both a stale CI digest
and `resource checker CI loader is absent`; changing one CFG-plant IR peak
produced `cfg plant changed wrong phase ('linear/64/base', 'ir')`. The disposable
worktree was removed after each protection plant.

Allocator semantics, reset and lifetime rules, test coverage, protection, and
ruler-change adjudication are in `docs/resource-ruler-protocol.md`.
