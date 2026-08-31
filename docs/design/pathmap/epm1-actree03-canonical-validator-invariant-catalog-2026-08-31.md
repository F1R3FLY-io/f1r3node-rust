# EPM1 ACTree03 canonical-validator invariant catalog

Date: 2026-08-31

Status: kernel-checked formal authority and focused executable correspondence verified

Scope: canonical PathMap ACTree03 writer images admitted by the stack-safe direct validator and
consumed by Bag and EPathMap/EPM1

## Purpose

This report is the traceability authority between the concrete ACTree03 writer/parser proof and
the optimized Rust validator. It answers one exact completeness question: does the direct,
stack-safe validator accept every physical byte image emitted by the canonical PathMap writer while
retaining the independently tested rejection language for malformed or alternate encodings?

The required refinement order is:

1. model the physical writer, parser, line cache, semantic discovery, and replay;
2. prove writer-to-validator completeness and run negative controls;
3. extract every formal obligation into stable executable properties;
4. bind every property to the optimized Rust implementation, generators, and independent oracles;
5. accept the implementation only after the focused properties and repository proof gate pass.

The proof does not authorize an alternate EPathMap representation. Production retains the native
`Empty | Set(PathMap<()>) | Map(PathMap<Par>)` model, and Bag retains
`PathMap<Multiplicity>`. ACT bytes remain PathMap's prefix-compressed trie image; no property below
projects keys into `Vec<Par>`, sorts trie entries, or constructs a hash-map shadow.

## Formal and executable evidence

- Rocq project: `formal/rocq/actree03_completeness/`
- final proof authority: `formal/rocq/actree03_completeness/theories/Epm1ActConcreteV5.v`
- structural mutation controls:
  `formal/rocq/actree03_completeness/theories/Epm1ActConcreteV5Mutants.v`
- cursor mutation controls:
  `formal/rocq/actree03_completeness/theories/Epm1CursorPositiveV4Mutants.v`
- production validator: `rspace++/src/rspace/pathmap_codec.rs`
- executable properties: `rspace++/tests/support/pathmap_codec_validated_plan.rs`
- mechanical theorem/test gate: `models/tests/formal_equivalence_manifest.rs`
- integrated proof gate: `scripts/check-stack-safe-pda-proofs.sh`

All six Rocq files compile in dependency order under a 1.5 GiB resident-memory ceiling with zero
swap. The final capstone exposes each release theorem through `Print Assumptions`; each reports
`Closed under the global context`. The tracked sources contain no `Admitted`, `admit`, `Axiom`, or
`Parameter` declaration. Both mutation suites compile counterexamples against deliberately altered
decisions rather than weakening the accepted theorem.

## Vocabulary

- **Physical node identity**: the byte offset of an ACT object in the arena.
- **Semantic node**: a logical trie node after folding any physical line-wrapper object.
- **Canonical producer position**: the next arena offset at which the modeled PathMap writer emits
  a child block, line, wrapper, or root.
- **Validated plan**: one immutable forward-preorder record per admitted semantic node plus folded
  value-order facts.
- **Writer completeness**: every canonical writer output is accepted; this does not claim that every
  accepted arbitrary byte sequence was writer-produced without the separate malformed differential.
- **Fuel-free runtime PDA**: the production walk terminates from finite input, unique/backward
  physical identities, child counts, and explicit heap frames. The proof's token list is a
  well-founded reasoning device, not a production depth limit.

## Principled implementation refinements

The proof deliberately uses simple mathematical structures. Production applies two semantics-
preserving optimizations whose correspondence is executable rather than definitional:

1. Rocq discovers reverse preorder and performs exact identifier lookup. Rust writes one compact
   record per node in forward preorder and resolves descendants with `next_record` plus child counts.
   This removes repeated linear lookup.
2. Rocq separates semantic discovery from recursive replay and then proves an enter/finish PDA.
   Rust performs canonical producer replay as validation frames finish. This avoids retaining a
   second input-sized finish-order structure.

`ACT03-INDEX-001`, `ACT03-CHILD-001`, and `ACT03-REPLAY-001` are the mandatory refinement witnesses
for these differences. Neither optimization changes canonical bytes, logical topology, endpoints,
value ordinals, error admission, or source provenance.

## Exhaustive property catalog

| ID | Required property | Formal authority | Executable strategy |
|---|---|---|---|
| ACT03-CAN-001 | Every canonical PathMap writer image is accepted. | `validate_accepts_every_canonical_writer_output` | Generate arbitrary PathMaps, emit with `ArenaCompactTree::from_zipper`, and require both validators to accept. |
| ACT03-FRM-001 | Magic, root slot, arena end, and zero trailer are exact. | `semantic_certificate_image_boundaries` | Exhaustive truncation and single-bit mutation differential plus explicit gap/trailer controls. |
| ACT03-ROOT-001 | The root begins in bounds, is emitted last, and closes the arena exactly. | `semantic_certificate_root_closes_arena`; `canonical_semantic_certificate_header_and_root_bounds` | Root displacement, gap, overlap, and trailing-byte rejection against the legacy oracle. |
| ACT03-DISC-001 | Discovery contains every and only reachable semantic node. | `canonical_writer_semantic_certificate_discovery_exact` | Compare complete topology and endpoint transcripts; require one parse per plan record. |
| ACT03-ID-001 | Physical identities are unique; aliases cannot enter the plan. | `semantic_certificate_preorder_physical_ids_nodup` | Assert unique record offsets and reject alias/forward-edge mutants. |
| ACT03-ORDER-001 | Children precede parents, branch labels increase strictly, and children, lines, wrappers, and root follow producer order. | `validate_accepts_semantic_certificate` | Mutation differential plus named alias, sibling-reentry, gap, and root-order controls. |
| ACT03-INDEX-001 | Every discovered record has one exact forward-preorder index and the implicit child schedule consumes each record once. | `reversed_source_nodes_index` | Reparse every recorded physical ID, compare value/line/child metadata, and execute the child-count schedule without lookup. |
| ACT03-CHILD-001 | The summed encoded lengths of direct children equal their exact producer extent. | `direct_children_encoded_len_source_forest_exact`; `direct_children_encoded_len_source_exact` | Target unequal child lengths, descendants, wrappers, reused lines, and a value-free endpoint. |
| ACT03-LINE-001 | A new canonical line is allocated at the current producer position. | `replay_canonical_line_matches_add_or_reuse` | Forced-hash line-cache sequence. |
| ACT03-LINE-002 | An equal cached line reuses its latest physical identity without allocation. | same | Exact byte-range equality and unchanged producer position. |
| ACT03-LINE-003 | A hash collision reuses nothing unless line bytes are equal. | same | Forced collision between unequal A and B. |
| ACT03-LINE-004 | An unequal collision overwrites the bucket, so A/B/A re-emits A and subsequent A reuses the new identity. | same | Exact A/B/A/A position and equality-work assertions. |
| ACT03-REPLAY-001 | Finish-order producer replay equals certified recursive writer construction. | `certified_source_writer_recursive_replay_exact`; `semantic_certificate_recursive_replay_exact` | Compare direct-plan topology/endpoints with legacy reconstruction across arbitrary and targeted writer images. |
| ACT03-PDA-001 | The explicit enter/finish machine is extensionally equal to recursive replay and uses constant native stack. | `replay_plan_executes_source_semantics`; `semantic_certificate_pda_replay_exact` | Depth and width tests on a fixed 64 KiB native stack. |
| ACT03-FUEL-001 | The formal schedule needs exactly two transitions per semantic node; production has no fuel or traversal cap. | `semantic_certificate_pda_replay_exact` | Require one physical parse per record, zero reparses, finite unique identities, and complete frame exhaustion. |
| ACT03-EPM1-001 | The ordinary ACT writer bridge remains valid inside Empty, Set, and Map EPM1 envelopes. | `validate_accepts_every_canonical_writer_output` | EPM1 Set/Map/Empty, compact-line, branch, dense, deep, value-table, and golden-byte properties. |
| ACT03-ORACLE-001 | The optimized validator has the same admitted canonical writer language as independent reconstruction. | `validate_accepts_semantic_certificate` | Legacy parse/visit and reconstruct/re-encode differentials over writer images and mutations. |
| ACT03-SOURCE-001 | Contiguous and fragmented indexed sources produce identical admission, events, errors, and source offsets. | `semantic_certificate_image_boundaries` | Deterministic fragmentation schedules over canonical and malformed images. |

## Generator catalog

| ID | Domain and required shapes |
|---|---|
| G-ACT03-WRITER | Arbitrary `PathMap<u64>` mutation sequences using value insertion and value-free `create_path`; empty, duplicate update, line, shared prefix, sparse/dense branch, nested wrapper, and varied `u64` value widths. |
| G-ACT03-MUTATION | All truncations and single-bit mutations of selected images plus typed root, arena, trailer, alias, label, sibling, wrapper, and line-placement mutations. |
| G-ACT03-LINE-CACHE | Forced-hash A, A/A, A/B, A/B/A, and A/B/A/A sequences, including equal and unequal byte ranges. |
| G-EPM1-WRITER | Empty, Set, and Map EPM1 envelopes over canonical ACT writer images with canonical `Par` bodies and exact sequential ordinals. |
| G-ACT03-DEPTH-WIDTH | Deep combs, value-at-every-prefix combs, sparse branches, and complete 256-way dense branches. |

The arbitrary writer generator uses a bounded case size only to keep a focused unit test
deterministic in resources. It is not a semantic depth limit; separate deep tests reach depth 4,096
and the validator contains no fuel parameter.

## Oracle catalog

| ID | Independent authority and use |
|---|---|
| O-ACT03-LEGACY | `CompactTrie::parse_canonical` plus legacy `visit`/`visit_topology`; retained test oracle, never a production fallback. |
| O-ACT03-RECONSTRUCT | Decode/reconstruct/re-encode equality for exact canonical images. |
| O-ACT03-PATHMAP | Live PathMap zipper topology and endpoint/value observations. |
| O-ACT03-SOURCE | Contiguous, fragmented, owned, and indexed cursor agreement. |
| O-EPM1-GOLDEN | Frozen EPM1/protobuf/bincode/event-hash fixtures and mode-specific round trips. |
| O-ROCQ-ACT03 | Named theorem and assumption-print inventory in `Epm1ActConcreteV5.v`. |

## Negative-control and non-vacuity matrix

| Boundary | Negative control | Required witness |
|---|---|---|
| root/header/trailer | truncated root slot, displaced root, gap, nonzero trailer, trailing bytes | empty and nonempty writer images accepted |
| physical identity | aliased node, forward/equal child, sibling reentry | every canonical physical ID appears exactly once |
| producer order | reordered branch labels, displaced child block/line/wrapper/root | unequal child extents and nested wrappers accepted |
| line cache | equal-line allocation mutant, unequal-collision reuse mutant, stale A reuse after A/B | first allocation, equal reuse, collision overwrite, and A/B/A re-emission reached |
| replay schedule | finish-before-descendants and missing-finish mutation suites | enter and finish reached for every semantic node |
| source abstraction | fragmented truncation and mutation corpus | contiguous and fragmented canonical transcripts agree |
| stack safety | recursive control is excluded from production; no artificial fuel is accepted | deep and wide traversals finish on a 64 KiB native stack |

## Formal-to-executable derivation matrix

Coverage classes are **D** (direct property), **P** (property/differential), and **E** (exact
implementation refinement). Release closure requires every row to include D or P and every E row to
name its semantic differential.

| Formal family | Property IDs | Coverage |
|---|---|---|
| exact physical writer/parser and boundaries | ACT03-CAN-001, ACT03-FRM-001, ACT03-ROOT-001 | D/P |
| semantic discovery and unique identity | ACT03-DISC-001, ACT03-ID-001 | D/P |
| reverse-preorder mathematical index to forward-preorder Rust records | ACT03-INDEX-001 | D/E |
| exact child extents and producer order | ACT03-CHILD-001, ACT03-ORDER-001 | D/P/E |
| line allocation/reuse/collision state machine | ACT03-LINE-001..004 | D |
| recursive writer replay to fused finish-order replay | ACT03-REPLAY-001 | P/E |
| recursive semantics to stack-safe PDA | ACT03-PDA-001, ACT03-FUEL-001 | D/E |
| ordinary writer to EPM1 and independent oracles | ACT03-EPM1-001, ACT03-ORACLE-001, ACT03-SOURCE-001 | D/P |

## Complexity and consensus boundary

For source bytes $`a`$, semantic nodes $`S`$, logical edge bytes $`e`$, maximum endpoint length
$`K`$, and explicit depth $`D`$, the selected implementation retains the established bounds

```math
T(a,S,e)=\Theta(a+e),\qquad H(S,K,D)=O(S+K+D),\qquad N=O(1).
```

The proof-only reverse-preorder search is not production complexity. Rust's forward-preorder record
index and fused finish replay remove that quadratic lookup and a duplicate input-sized schedule.
The test-only record reparse is an oracle assertion and is absent from production.

This checkpoint proves and tests existing canonical bytes; it changes no Rholang value, EPathMap
mode, Bag multiplicity, protobuf/bincode/EPM1 byte, hash, COMM selection/order, charge, post-state,
or persistent authority. It therefore adds no consensus-change-register entry. Any future change to
the writer image or admitted byte language must be reviewed there before implementation.

## Completion rule

This catalog may be marked verified only when:

1. all six Rocq sources and both mutation suites compile under the repository memory cap;
2. the assumption-free source gate and all 18 mechanical bindings pass;
3. arbitrary writer, exact record-index, unequal-child/wrapper/reuse, mutation, deep, and wide
   properties pass;
4. focused EPM1 writer-image properties pass; and
5. the living PathMap journal and stack-safety report record the evidence without presenting it as
   a wall-clock or whole-RSpace performance result.

## Verification record

All completion conditions above passed on 2026-08-31 under systemd user scopes with zero swap:

| Gate | Resource ceiling | Result |
|---|---:|---:|
| six-file Rocq proof lineage and both mutation suites | 1.5 GiB RSS | pass; every printed capstone closed |
| complete repository Rocq/Verus/Z3/TLC/mCRL2 harness | 8 GiB outer scope; 1.5 GiB ACT Rocq; 1 GiB other subjects | pass; captured-log SHA-256 `9121875b6024f0ca70e0fd47bc847c34ac23ac504d95e94deb41379a44892e8f` |
| focused `rspace_plus_plus` validated-plan module | 8 GiB RSS, eight-core quota | 10/10 |
| complete formal-equivalence manifest binary | 8 GiB RSS, eight-core quota | 17/17 |
| focused EPM1 snapshot/writer-image binary | 8 GiB RSS, eight-core quota | 16/16 |

The validated-plan result includes 128 property cases per run over bounded arbitrary PathMap
operation sequences. These results close the named correctness and stack-safety obligations. They
do not supersede the separately tracked allocator, wall-clock, cache, persistence, or full release
workspace gates.
