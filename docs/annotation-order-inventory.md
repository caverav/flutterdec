# Order inventory for the value-annotation work

Every ordering the annotation work (shared literals, join site, loop-entry site, pre-call site)
introduced or touched on a path that reaches emitted output, with the input each one sorts and the
argument that its order is total.

Determinism here is argued **by construction**. A byte-identity pair over two cold processes is
corroboration and not proof: an unfixed partial order over a hash-derived sequence was measured on
this branch to pass a single A/B comparison about two thirds of the time. The rule this inventory
applies is R24's - a partial comparator is a hazard only when the sequence feeding it is
hash-derived - so every row names its input.

Three sequences below are hash-derived (**H**). Everything else is a `Vec` whose order comes from
block ids, line indices or a `const` slice, and is therefore already fixed before the sort runs.

Line numbers are as of the tree that produced candidate binary
`470334075c69fa30ad3fe35da1f971ef8f0c8962d000603556b7fef3d42b523d`; the function names are the
durable reference.

## Sorts

| # | site | input | hash-derived | why the order is total |
|---|---|---|---|---|
| O1 | `structured.rs:128`, `ordered_join_candidate_provenance` — `sort_by(pred, value)` | `filter_map` over `Regions::predecessors(join)` at the join site and over its non-back-edge subset at the loop site. `Regions::preds` is built by scanning block ids ascending. | no | Key `(pred, value)`. Over the real input `pred` alone is already unique, because one predecessor contributes at most one candidate; `value` makes the comparator total for *any* input, including a repeated predecessor id. Records equal on both fields are equal on the third as well - `snapshot_id` is a function of `(site, pred, capture)` - so a surviving tie is between indistinguishable records. |
| O2 | `structured.rs:1078`, `record_join_candidates` — `regs.sort()` | the `HashSet<String>` `registers_written_between` returns | **H** | `HashSet` elements are unique, so a plain `String` sort is total. This is the visit order of the dropped registers, and it decides the order the audit records its snapshots in. |
| O3 | `structured.rs:1219`, `record_loop_entry_candidates` — `regs.sort()` | the same write set, taken at the loop header | **H** | as O2 |
| O4 | `structured.rs:1038`, `record_join_snapshots` — `registers.sort()` | `snapshot.reg_values.iter()`, a `HashMap<String, String>` | **H** | map keys are unique, so the first element of the pair totally orders it. This is the register list written into each audit snapshot row. |
| O5 | `structured.rs:1160`, `record_loop_entry_snapshots` — `registers.sort()` | the same map at the loop site | **H** | as O4 |
| O6 | `structured.rs:1117-1118`, `record_join_candidates` — `regs.sort(); regs.dedup()` | the `Vec<String>` pushed in O2's order | no | sorted then deduped, so the result is a strictly increasing sequence |
| O7 | `structured.rs:1259-1260`, `record_loop_entry_candidates` | the `Vec<String>` pushed in O3's order | no | as O6 |
| O8 | `structured.rs:1389`, `append_join_annotations` — `sort_unstable_by(line desc, at desc)` | `Vec<PlannedJoinAnnotation>`, built by walking the anchors in push order and each anchor's lines and token offsets ascending | no | `(line, at)` is unique by construction: a planned insert whose pair already exists is rejected before it is pushed. With no ties left, the comparator is total and `sort_unstable` is deterministic. The offset component is not decoration - inserts are planned in ascending offset and applied in descending offset, so dropping it would shift every later offset by the length of the annotation already inserted. |
| O9 | `structured.rs:1446` and `structured.rs:1506`, the two annotation-provenance recorders — `sort_by(line asc, at asc)` | the same `inserts` slice | no | same uniqueness as O8; this is the ascending output order the audit's monotonic span search depends on |
| O10 | `emit.rs:1086`, `append_call_annotations` — `sort_unstable_by(line desc, at desc)` | `Vec` built from `call_annotation_anchors` (push order is render order) mapped through `align_rendered_lines` | no | as O8, with the duplicate `(line, at)` rejected before the push |
| O11 | `provenance.rs:318-319`, `write_function_provenance` — `referenced.sort_unstable(); dedup()` | `Vec<&str>` collected from `placed`, itself in record order | no | order never reaches the output: `referenced` is consumed only by `contains`, so this is a lookup narrowing and not an ordering. The emitted snapshot rows keep `provenance.snapshots` order. |
| O12 | `structured.rs:383-384`, `register_tokens` — `tokens.sort(); tokens.dedup()` | `Vec<String>` from a left-to-right scan of one line | no | pre-existing; touched by this work only through the `canonical_join_register_spelling` -> `canonical_register_spelling` rename. Sorted and deduped, so strictly increasing. |

## `HashMap` / `HashSet` iteration on an output-affecting path

| # | site | what it iterates | why the output does not depend on the order |
|---|---|---|---|
| I1 | `structured.rs:1033` and `1155` | `snapshot.reg_values` | collected and immediately sorted, O4 / O5 |
| I2 | `structured.rs:1073` and `1214` | `written` (`HashSet<String>`) | collected and immediately sorted, O2 / O3 |
| I3 | `structured.rs:1564-1572`, `merge_state_at_join` | `state.reg_values.retain`, `state.call_clobbers.retain` | `retain` applies a per-element predicate and yields a map; the surviving set does not depend on the visit order |
| I4 | `expression_lift.rs`, `apply_other_lift` | `for reg in written_registers(..)` then `call_clobbers.remove` | `written_registers` returns a `Vec`; and a set of removals is order-independent regardless |
| I5 | `emit.rs:812`, `record_pre_call_snapshot` | `for reg in CALL_CLOBBERED_REGISTERS` | a `const &[&str]`, so the pre-call snapshot and the clobber table are built in a fixed order |
| I6 | `loop_annotation_sites`, `join_candidates`, `join_candidate_regs`, `call_clobbers`, `state.reg_values` | — | never iterated on an output path: insert, point lookup, remove and clear only |

Not iterated and not touched: `Regions::preds`/`succs` are `Vec`s, and the pre-existing sorts at
`regions.rs` (`targets`), `lib.rs` (`ranked`) and `helpers/naming.rs` (`spellings`) are outside this
work's diff.

## Mutation tests

`src/tests/cfg_and_stack/order_totality.rs`. Each test varies the input order and asserts the output
does not move; each was then confirmed to **fail** with its tie-break deleted, because a test that
passes without the tie-break is not testing the tie-break.

The hash-derived rows are varied through the hash itself rather than through a simulated
permutation. `RandomState::new` advances a thread-local counter per instance, so every fresh
`HashSet` in one process has its own seed - the same variation two cold processes see. The
repetition tests assert that premise (`assert_hash_order_varies`) before relying on it, so they
cannot pass vacuously over a sequence that happened not to vary.

| mutation | removed | test that fails |
|---|---|---|
| M1 | O1's `.then_with(\|\| left.value.cmp(&right.value))` | `candidate_order_is_total_over_every_permutation_of_its_input` (and `join_candidate_order_ignores_source_insertion_order`) |
| M2 | O2's `regs.sort()` | `a_join_emits_one_fingerprint_under_every_hash_seed` |
| M3 | O3's `regs.sort()` | `a_loop_header_emits_one_fingerprint_under_every_hash_seed` |
| M4 | O4's and O5's `registers.sort()` | `a_recorded_snapshot_lists_its_registers_in_sorted_order`, plus both fingerprint tests |
| M5 | O8's `.then_with(\|\| right.at.cmp(&left.at))` | `two_annotations_on_one_line_land_on_their_own_registers` |
| M6 | O10's `.then_with(\|\| right.1.cmp(&left.1))` | `two_call_annotations_on_one_line_land_on_their_own_registers` |

Two fixture properties are load-bearing and were found by watching a mutation *survive*:

- **Disjoint predecessor coverage.** If every predecessor carried every register, the first register
  visited would record all the snapshots and the rest would find them already present, so any visit
  order would produce the same audit. The join fixture gives block 1 x0 and x2 and block 2 x3 and x5
  for that reason.
- **A loop header needs two entry arms.** The first loop fixture had one entry predecessor, so every
  candidate cited the same snapshot and M3 survived. A header reached from two arms - which is also
  a join, and the join capture declines it - is what makes the loop site's register order
  observable.

Register order is invisible in the pseudocode: annotations are planned by walking the rendered line,
not the register list. It is visible in the order the audit records its snapshots, which is a file a
validator diffs across runs, so the fingerprint the repetition tests compare is the rendered lines
**and** the recorded provenance. Verified rather than assumed: with O2's sort deleted the two
emissions' rendered lines are byte-equal and the snapshot lists differ (`join:3:pred:1:1` against
`join:3:pred:2:0`).

That is also why the corpus pair below was run twice more with `FLUTTERDEC_PROV_AUDIT` set. The
plain corpus cannot observe O2-O5 at all, so a byte-identical corpus pair is not evidence about
them; the audit pair is.

## Cold-process evidence

Candidate binary `/tmp/flutterdec-detcand`,
sha256 `470334075c69fa30ad3fe35da1f971ef8f0c8962d000603556b7fef3d42b523d`. Serial runs from the repo
root, `--function-scope all --adapter-backend internal --split-records` plus the four permissive gate
flags, no `--max-functions`, stderr captured to a file.

| pair | files | rc | stderr | wall | sha256 of the corpus, both runs | `diff -qr` |
|---|---|---|---|---|---|---|
| localsend A/B | 22,102 | 0 | 0 bytes | 154 s / 153 s | `42b3a79974aa8b33d27f1669da905a319504f0f0bf77e6d6bfd2a5a9a452c350` | rc 0 |
| immich A/B | 28,753 | 0 | 0 bytes | 206 s / 208 s | `f59802410a0f08b6086a64b87e7280ed0909ad0773dbecefc89f191e00ac6593` | rc 0 |
| localsend A/B, audit on | 22,102 | 0 | 0 bytes | 121 s / 129 s | audit `6fab171e2f510ae6b8221c3a97f7fca8f7e08107127ce632047934205edccd62`, 8,815 rows each | rc 0, corpus and audit |

The corpus carries all four literals - 762 exhaustive, 3,210 non-exhaustive, 390 loop-entry and 7
pre-call on localsend - so the comparison is over annotated output rather than over a build in which
the feature never fired.
