# ACTree03 canonical writer and validator completeness

Status: kernel-checked ACTree03 and positional EPM1 formal authority;
executable correspondence refinement in progress

This Rocq project proves that the stack-safe ACTree03 validator accepts every
canonical byte image emitted by the modeled PathMap writer. It closes the
previous gap between structural EPM1 envelope proofs and the concrete arena
writer/parser contract.

The development proves, without `Admitted`, `Axiom`, or caller-supplied fuel
assumptions:

- exact ACTree03 header, root, arena, and trailer boundaries;
- discovery of every reachable semantic node with unique physical identities;
- strict child order, exact child-root layout, and source-node indexing;
- canonical line allocation, equal-byte reuse, hash-collision overwrite, and
  A/B/A re-emission behavior;
- equivalence between recursive semantic replay and the explicit enter/finish
  pushdown automaton; and
- acceptance of every canonical ordinary-writer output by the direct
  validator;
- one zero ACT sentinel per structural value occurrence, with no sequential
  ordinal or allocation-address semantics;
- lexicographic lockstep between independently traversed source bodies and
  ACT value endpoints, including a root value;
- exact maintained/fallback endpoint-count agreement; and
- positional child-PDA suspension, owned-focus installation, error precedence,
  failure nonpublication, and exactly-once successful publication.

The numbered filenames preserve the audited proof lineage. The concrete
ACTree03 writer/parser authority is `Epm1ActConcreteV5.v` and the joined
physical/positional authority is `Epm1PositionalAssociationV6.v`; earlier files
provide definitions and lemmas imported by those capstones rather than
competing production specifications. The three `Mutants.v` files are negative
controls and must continue to compile their counterexamples against
deliberately altered decisions. V6 specifically kills pointer/address collapse,
sequential ordinals, nonzero sentinels, skipped/duplicated/wrong/root endpoints,
early/double publication, precedence inversion, and missing/extra bodies.

Run the project under a hard memory ceiling:

```bash
systemd-run --user --scope \
  -p MemoryMax=1536M -p MemorySwapMax=0 \
  coqc -Q theories '' theories/Epm1CursorAuthorityV2.v
# Continue in _CoqProject order through
# Epm1PositionalAssociationV6Mutants.v.
```

The repository gate `scripts/check-stack-safe-pda-proofs.sh` performs that
ordered compilation. Executable properties and their theorem bindings are
specified by the companion ACTree03 invariant catalog under
`docs/design/pathmap/`.

## Collision observations (2026-09-12; task 6023)

The V5 observational extension retains the existing `traced_add_or_reuse_line`
and its raw-writer erasure law. It counts a collision only when the selected
hash bucket is occupied by unequal bytes. Misses and equal-byte reuse contribute
zero. Observation does not modify IDs, arena bytes, cached entries or physical
writer objects, including across a sequence of calls.

| Checked invariant | Native binding |
|---|---|
| Unequal occupied bucket iff collision; each call contributes zero or one | Increment `line_hash_collisions` inside the existing unequal branch of `DirectCanonicalLineReplay::expected_line_id`, without another hash or equality operation. |
| Traced and raw writer results are unchanged | `forced_collision_histories_match_latest_content_and_physical_writer_order` compares each step with the independent latest-content/producer-order oracle. |
| A/B/A/A contributes exactly two collisions, allocates three line objects, and reuses the final A ID | `forced_line_hash_collision_preserves_range_equality_and_last_writer_semantics` checks counts 0, 1, 2, 2; `forced_collision_history_covers_reuse_aba_and_unequal_lengths` covers additional histories. |
| An equal occupied bucket is not a collision | `counting_every_occupied_bucket_is_not_collision_counting` is a causal negative control against counting every cache hit. |

The logical counter uses unbounded naturals. Native validation creates a fresh
local counter and replay cache per invocation; each uniquely registered physical
node reaches producer replay at most once, so collisions are bounded by the
number of nodes and hence source length. This native binding, lazy hash work and
source-range equality are not proved merely by decorating the writer model.
The decorator is ghost observation, not a proposal to repeat runtime comparisons.

V5 now reports 195 closed contexts (eight added). Compilation and independent
kernel checking passed in `target/verification/act-collision-formal-r1.log` and
`act-collision-kernel-r1.log`. Checks used MemoryHigh=3G, MemoryMax=4G, no swap,
one CPU quota, TasksMax=64 and a 900-second limit. The maintained gate already
uses this larger ceiling for V5 and its mutants; the smaller V2 command above
is not the V5 resource specification. Rebuild downstream V5 mutants and V6
importers in the existing project order after a V5 change.
