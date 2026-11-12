# Atomic benchmark history repair

This record preserves the evidence for the accepted repair of pre-repair commit
`61e89fdf915f62e97a32df21385ee151a05690e6`. The repair started from remote
head `c2d514b595e05d0150ccc7a19c6454bb3443fb43`. It changes history only by
splitting that mixed commit and replaying its descendants over the split.

## Split boundary

The old commit is replaced by these two commits, in this order:

1. `4a9f681e404fe9e68dfc3e8c769a1000a0851214`,
   `perf(benchmark): isolate harness workspace`, changes only workspace
   membership, root and harness lockfiles and manifests, the ignored standalone
   target path, the excluded-harness CI lane, and the harness build and binary
   paths in `scripts/bench-pipeline.sh`.
2. `be38db284e3b57eadbc906d92a4533a9e22cdb6b`,
   `perf(benchmark): harden measurement scheduling`, changes only `--runs 0`,
   output locking, populated-raw refusal and `--clear`, and preliminary warmup
   scheduling.

The tree at the second commit is exactly the old `61e89fd` tree
`e33f1aace5bd46ec2fd98e7fa739f3d508ea8e16`. The stable patch ID for the old
parent-to-`61e89fd` diff and for the old parent-to-`be38db2` combined diff is
`768300a24b9545c3fc76a43e90f7f4b21f1eba1b`.

## Descendant mapping and equivalence

[`oid-map.tsv`](oid-map.tsv) maps all 107 rewritten old commits: the old split
commit plus every one of its 106 descendants through old head `c2d514b`. Each
row records both parents, both tree IDs, and both stable patch IDs. The first
row compares the old mixed commit with the combined two-commit replacement.
Every later row has equal old and new tree IDs and equal old and new stable
patch IDs. The rewritten pre-evidence head is
`c4eecd9f959a41d4f73a3b394b593f2a94ed0f7a`, whose tree is exactly the old
head tree `e9d2861ec56f8fe0f5f3f9860fc42e8e6519a52e`.

Author, committer, message, author timestamp, and committer timestamp were
copied from each old descendant. No merge, product edit, benchmark input,
correctness oracle, or accepted measurement output was introduced by the
rewrite.

## Immutable refs and accepted patch

[`immutable-refs.tsv`](immutable-refs.tsv) records the immutable product ref
`1371e42549472ec388f58bc1fd5dbdf96e8dcdd1`, immutable accepted harness ref
`8e7f08096434b614a1e8dc6d3092ff6a67bb44c9`, and old and rewritten heads with
their trees. The old mixed commit is an ancestor of neither immutable ref.

The patch re-derived with

```text
git diff 209a8fe 8e7f080 -- . ':!docs'
```

is byte-identical to `docs/baseline/harness-8e7f080.patch` before and after the
repair. Its SHA-256 remains
`14413796ca8a89cc1328497b5c87629b1c55f945ec58e73eebb3838df0700460`.

## Binding and artifact preservation

The pre-repair inventory covered all 241 tracked files under `docs/`. The
accepted-artifact subset contains all 188 tracked files under `docs/baseline/`,
`docs/post-correctness/evidence/`, `docs/final-performance/evidence/`,
`docs/performance-profile/evidence/`, `docs/resource-evidence/`,
`docs/research-data/`, and `docs/compat-evidence/`.

[`pre-repair-artifacts.tsv`](pre-repair-artifacts.tsv) and
[`post-repair-artifacts.tsv`](post-repair-artifacts.tsv) record path, byte count,
SHA-256, and Git blob ID for every accepted artifact. They are byte-identical,
including all 28 files under `docs/baseline/`. Their shared SHA-256 is
`a62dca86d16b29848bdbfaf252bef633917a555f535096f01799e1b2831f4ca6`.

[`pre-repair-bindings.tsv`](pre-repair-bindings.tsv) and
[`post-repair-bindings.tsv`](post-repair-bindings.tsv) contain the complete
579-line binding scan over those accepted artifacts. They are byte-identical at
SHA-256 `25a944e6f9c6d239ce627c9ebb9150d75a9c35290d0e640210e03335edea31ef`.
[`inventory-digests.txt`](inventory-digests.txt) records the paired inventory
digests.

All five committed `SHA256SUMS` manifests pass `sha256sum -c` before and after
the rewrite: post-correctness, final-performance, performance-profile,
resource-evidence, and research-data. The A/A audit over `docs/baseline/aa-1`
and `aa-2` also exits zero without changing an artifact.

## Product-tree digests

[`product-tree-digests.tsv`](product-tree-digests.tsv) recomputes the exact
derivation declared by `docs/compat-evidence/input-recipe.json`:

- immutable reference: `23165413ab8e29b08ac71bd712aaf607154aea090ae1680170472f05d3a8e6f3`
  over 106 files;
- accepted candidate ref `eabbe7e`: `1b49ad07ca604dee352f9166601d29bb7b14ae01ece20d0acb122c4e91e07061`
  over 142 files;
- old and rewritten heads: identical
  `fb8ed1493863440f0c1ee113334f00bdbe0089007003f84e9f237147ebf5ad9d`
  over 143 files.

The current head digest already differed from the accepted candidate digest
before this repair because the later test-only product path is included by the
declared derivation. This record preserves both values and does not relabel the
accepted candidate evidence as head evidence.

## Historical citations

[`provenance-citations.tsv`](provenance-citations.tsv) inventories all 291
tracked documentation occurrences of the 107 pre-repair commit IDs. Those
historical documents and accepted artifacts remain byte-identical. Each row
names the corresponding post-repair ID and marks the citation as retained
pre-repair provenance. This is deliberate: replacing IDs inside frozen evidence
would silently relabel what the original run or adjudication recorded.

The new IDs in `oid-map.tsv`, this record, and the PR disclosure are the
post-repair identities. The IDs in the citation census remain explicitly
pre-repair identities.

## Verification commands

The repair was checked with focused history and evidence commands only. No
formatter, linter, or project-wide test suite was run for this delivery repair.

```text
git diff 61e89fd be38db2
git diff 4e8a9b2 be38db2 | git patch-id --stable
git diff 4e8a9b2 61e89fd | git patch-id --stable
cmp pre-repair-artifacts.tsv post-repair-artifacts.tsv
cmp pre-repair-bindings.tsv post-repair-bindings.tsv
sha256sum -c SHA256SUMS
python3 docs/baseline/audit-aa-runs.py docs/baseline/aa-1 docs/baseline/aa-2
```
