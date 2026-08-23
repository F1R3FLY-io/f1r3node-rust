# PathMap engineering journal

Status: living scientific and engineering report

Campaign: `campaign-root-4131`

Started: 2026-08-23

Last updated: 2026-08-23

## Purpose

This journal records what the campaign learns about using PathMap effectively in
`f1r3node-rust-mettail`. It is intentionally chronological and falsifiable. An entry preserves the
question, source coordinate, experimental method, observation, interpretation, rejected
interpretations, and resulting engineering rule. Later evidence amends an entry explicitly rather
than silently rewriting its history.

The journal complements, rather than duplicates, three living reports:

- [PathMap-native EPathMap report](pathmap-report-2026-08-03.md), the historical EPathMap evidence
  ledger;
- [PathMap Bag and RSpace integration](../rspace/pathmap-bag-rspace-integration.md), the RSpace
  contract and experiment report; and
- [Stack-safety report](../stack-safety/stack-safety-report-2026-07-29.md), the recursion, allocation,
  and profiling ledger.

## Terms, symbols, and evidence classes

| Term or symbol | Definition |
|---|---|
| PathMap | The unmodified sibling crate implementing a prefix-compressed byte trie with copy-on-write roots, read/write zippers, algebra, lattice operations, merkleization, and compact arena encodings. PathMap claims in this report are pinned to `f62b6f62580cd6bb6db9ce5385bb886fd7183397`. |
| Par | Rholang's canonical parallel-composition value. Its repeated component families are represented by `ParBag`; root `locally_free` and `connective_used` metadata remain separate. |
| RSpace | The tuple-space authority that stores data and continuations and derives search paths for candidate pruning. |
| EPathMap | Rholang's homogeneous `Empty | Set(PathMap<()>) | Map(PathMap<Par>)` value. |
| Bag | RSpace's `PathMap<Multiplicity>` multiset; one unique byte path stores one strictly positive `u64` multiplicity. |
| RMP | Rholang Match Path, the versioned, derived structural search image used by the RSpace candidate index. |
| RPB3 | The version-3 authoritative ParBag key grammar: component family, canonical score, canonical protobuf body, and score-length trailer. |
| RMP1 | The first RMP token grammar, whose ParBag token fingerprints the RPB3/ACT representation. |
| RMP2 | The provisional second RMP token grammar described in J-005; it fingerprints semantic ParBag endpoint records and is not accepted until H-001 closes. |
| EPM1 | The version-1 compact EPathMap format: mode and cardinality metadata, ACTree03 topology, and direct map-value protobuf regions. |
| ACT | Arena Compact Tree, PathMap's compact trie snapshot family. |
| ACTree03 | The ACT encoding used as the topology portion of EPM1. |
| Canonical PathMap writer image | ACTree03 bytes emitted from an in-memory PathMap by the pinned PathMap writer and then framed by the repository codec. A validator must admit every such byte image while refusing alternate physical encodings that the writer would not emit. |
| Explicit value-free endpoint | An observable trie path created without a value. In an ACT branch it may be represented by a reached childless semantic node whose value is absent; its parent edge and node presence distinguish it from an absent path. |
| Value-free unary branch | A semantic ACT node with no value and exactly one child. The canonical writer line-compresses this shape, so an uncompressed occurrence is an alternate encoding and remains invalid. |
| RhoPathV1 | The version-1 RSpace key framing that combines structural RMP bytes with canonical bincode identity bytes. |
| Provenance-admitted canonical region | A borrowed protobuf region reached only from an authoritative, RPB3-validated ParBag endpoint and retained without accepting arbitrary caller bytes. Its field order and bodies therefore inherit the generated canonical emitter's invariants. |
| Oracle-equivalence gate | A differential test in which the optimized result must equal the independent semantic implementation for the complete registered corpus. |
| COMM | A RSpace communication event: one successful data/continuation match whose result and ordering are consensus-relevant. |
| Canonical datum identity | The canonical bincode bytes of one stored `Datum`; the same suffix is used by encoded and decoded candidate APIs. |
| Canonical datum candidate order | The total order by BLAKE2b-256 of canonical datum identity, then by the identity bytes themselves to resolve a hash collision. Derived RMP bytes never participate. |
| BLAKE2b-256 | BLAKE2b with a 256-bit digest, specified by [RFC 7693](https://www.rfc-editor.org/rfc/rfc7693). |
| CoW | Copy on write: immutable roots are shared until a writer requires a private root. |
| [`OnceLock<T>`](https://doc.rust-lang.org/std/sync/struct.OnceLock.html) | Rust's thread-safe write-once cell. After one successful initialization, readers borrow the same stored value without recomputing it. |
| [`Arc<T>`](https://doc.rust-lang.org/std/sync/struct.Arc.html) | Rust's atomically reference-counted shared-ownership pointer. The final owner drops the shared value. |
| Clone family | ParBag instances that share one immutable `Arc<ParBagAux>` auxiliary root until a component mutation detaches the writer under CoW. |
| Candidate product cursor | The synchronized pair of PathMap read zippers over candidate availability and candidate positions used to visit only paths present in both tries. |
| AC | Associative-commutative matching, where grouping and order do not distinguish equivalent terms. |
| Sound candidate query | A derived RMP query that admits every candidate accepted by the authoritative spatial matcher. It may admit extras; rejecting one authoritative success is a false negative. |
| AST | Abstract syntax tree, the decoded recursive rhoapi object graph avoided by the byte-native routes. |
| DFS | Depth-first search: visit a child branch to completion before resuming its sibling. |
| PDA | Pushdown automaton: an iterative traversal whose explicit frames live on the heap. |
| CPU | Central processing unit. CPU time sums the execution time consumed by the process's threads. |
| RSS | Resident set size: the process memory currently resident in physical memory. |
| CBR | Consensus-Breaking Register: the historical stable identifier prefix retained by the living consensus-change register. |
| RAII | Resource acquisition is initialization: ownership binds resource lifetime to construction and destruction. |
| SHA | Secure Hash Algorithm: used here in the standard `SHA-256` artifact-integrity label. |
| KiB and MiB | Binary size units of `$`2^{10}`$` and `$`2^{20}`$` bytes, respectively. |
| Heaptrack | A heap-allocation profiler. “Headless” use records and analyzes traces through command-line tools without launching its graphical interface. |
| `$`n`$` | The input's logical number of trie endpoints, AST levels, or bytes, as specified per experiment. |
| DERIVED | Established from source, generated tables, a commit, or a proof artifact. |
| MEASURED | Observed by a recorded command under a stated resource envelope. |
| HYPOTHESIS | Registered before the decisive measurement. |
| DECISION | Engineering rule accepted from the available evidence. |

## Journal protocol

Each performance investigation follows this literate procedure:

**Algorithm 1 (Isolate a suspected PathMap cost).** This procedure separates a trie operation from
its surrounding codecs and lifecycle work before assigning a complexity class.

```pseudocode
ALGORITHM IsolatePathMapCost(operation, geometric_sizes)

Input:
    a production operation and a geometric size ladder
Output:
    an attributed complexity class and an actionable design rule

1. Freeze the source coordinate and resource envelope.
2. Split the operation at representation boundaries: key preparation, trie mutation, traversal,
   serialization, hashing, and teardown.
3. Measure every split independently on the same logical fixture.
4. Fit or inspect geometric ratios before assigning a complexity class.
5. Use a native profiler to confirm that sampled work belongs to the attributed phase.
6. Reject explanations contradicted by the split measurements.
7. Change the narrowest authoritative boundary, preserving PathMap as the trie.
8. Prove semantic equivalence, rerun the ladder, and record the treatment beside control.
```

The procedure is intentionally representation-aware: a result attributed to PathMap must survive
the phase split rather than inherit the cost of a decoder, sorter, or destructor around it.

For observations `$`T(n)`$` at geometric depths, the first diagnostic is the doubling ratio:

```math
r_n = \frac{T(2n)}{T(n)}.
```

A stable ratio near two suggests linear time; a ratio near four suggests quadratic time. This is a
diagnostic, not a confidence interval. Accepted benchmark claims still use preregistered sampling,
paired controls, and the statistical rules in the companion reports.

## Current PathMap use rules

These rules summarize accepted evidence. Their entry column points to the chronological record that
justifies each rule.

| Rule | Rationale | Entry |
|---|---|---|
| Keep the trie authoritative. Never turn an EPathMap or Bag into `Vec<Par>` for storage, serialization, hashing, or routine traversal. | Flattening loses prefix sharing, value-free endpoints, CoW roots, algebra, and zipper locality while adding a full materialization. | J-001 |
| Specialize the value domain: `PathMap<()>`, `PathMap<Par>`, and `PathMap<Multiplicity>` are distinct semantic algebras. | A set endpoint, map value, and bag count have different identity, join, meet, subtraction, and overflow laws. | J-001, J-002 |
| Serialize the native compact representation directly and keep recursive values in the generated PDA. | EPM1 preserves topology; direct component bytes avoid entry projection and recursive decoding. | J-001 |
| Use one retained zipper for ordered batches and deltas. | Moving between lexicographically adjacent paths reuses their longest common prefix and publishes one CoW root. | J-002 |
| Use native algebra and lattice operations only when their value algebra matches the Rholang operation exactly. | Join, meet, subtraction, and restriction are powerful, but an attractive algebraic name is not a proof of semantic equivalence. | J-003 |
| Attribute the bytes before blaming PathMap. | A trie mutation may be linear or negligible while preparing its byte path is quadratic. Measure the two phases separately. | J-004 |
| Do not decode an authoritative component merely to derive another byte index. | Reconstructing every recursive suffix can turn a constant-size derived key into quadratic work. Use a generated protobuf-region cursor over the resident body. | J-004 |
| Keep upstream PathMap unmodified unless an invariant cannot be enforced locally. | Local adapters can validate ACT input, avoid unsupported reverse moves, and preserve upgradeability without maintaining a fork. | J-001, J-003 |
| Validate canonical writer images by logical trie semantics, not by assuming that every value-free childless node is absent. | A reached childless node may be an explicit observable endpoint. Redundant value-free unary nodes and malformed ACT graph/framing shapes remain invalid. | J-008 |
| Exact-encode only fields that the authoritative matcher compares. | Exact constraints on matcher-ignored fields can reject a legal match. A derived query must project those fields semantically or decline pruning and scan the authoritative domain. | J-011 |

Provisional rule—not accepted evidence: J-005 proposes fingerprinting a Bag as self-delimiting
endpoint records plus multiplicities rather than an occurrence-expanded digest stream. H-001 must
close before this rule moves into the accepted table. J-006's forced-collision play/replay gate now
proves that derived RMP paths prune candidates but never break replay-visible datum-order ties;
stable commit and release evidence remain before promotion. J-007 proposes lazy nested-Par
summarization after cheap dynamic
classification; its implementation and geometric slope gate must close before promotion.
J-009 proposes a clone-family-shared, version-specific RMP2 digest cache; its byte, invalidation,
concurrency, and preregistered resource gates remain open under pgmcp tasks 5983 and 5984. J-010
attributes a possible quadratic allocation slope to retained zipper clones. Its first treatment is
now a focused-green bounded CoW suffix checkpoint, while a single-moving-zipper PDA remains the
escalation path; no complexity claim is accepted before the geometric gates in pgmcp bug 5985.
J-011's focused semantic repair is accepted, but its COMM, replay, and release gates remain open.
J-012 records the fail-closed generated-schema and protobuf-boundary checks. J-013 states exactly
what the current formal bridge proves and, importantly, what it does not yet prove.

## Chronological findings

### J-001 — 2026-08-03 through 2026-08-12: EPathMap must remain a homogeneous trie

Question: Can compatibility code treat EPathMap as a list of entries without losing the benefits of
PathMap?

Audit coordinate—not the original experiment coordinate:
`f1r3node-rust-mettail@a6b3ad09032c22bb10fafd6f7a890d7871b36609`, tree
`3388f0e80310d9794857d14650138bb45231a2e4`, with PathMap at
`f62b6f62580cd6bb6db9ce5385bb886fd7183397`. The linked historical ledger retains the individual
experiment coordinates.

Evidence: DERIVED and MEASURED. The complete lineage, fixed-scale measurements, negative results,
and proofs are recorded in the [EPathMap evidence ledger](pathmap-report-2026-08-03.md). EPM1 set
serialization measured 6,252 bytes against 491,528 bytes for the list projection at 1,024 entries;
native set lookup measured 32.00 ns per key against approximately 258 microseconds for the projected
linear scan. The campaign did not modify the PathMap crate.

Finding: The correct representation is:

```math
\mathit{EPathMap} = \mathit{Empty}
\mid \mathit{Set}(\mathit{PathMap}\langle()\rangle)
\mid \mathit{Map}(\mathit{PathMap}\langle\mathit{Par}\rangle).
```

The neutral empty value delays mode selection. Once selected, set and map modes remain homogeneous;
mixed value presence is rejected rather than represented by an option-valued trie.

Rejected interpretation: EPM1's advantage is not that ACTree03 compresses every serialized key
stream. That compression hypothesis was measured false. The accepted benefits are a single native
authority, topology preservation, direct accessors, and removal of repeated projections.

Decision: Every EPathMap boundary must operate on the trie, its zippers, or its compact bytes.

### J-002 — 2026-08-15 through 2026-08-17: Bag deltas should publish one CoW root

Question: How should repeated RSpace additions and removals mutate
`PathMap<Multiplicity>` efficiently and atomically?

Audit coordinate—not the original experiment coordinate:
`f1r3node-rust-mettail@a6b3ad09032c22bb10fafd6f7a890d7871b36609`, tree
`3388f0e80310d9794857d14650138bb45231a2e4`, with PathMap at
`f62b6f62580cd6bb6db9ce5385bb886fd7183397`. The linked integration report retains the individual
experiment coordinates.

Evidence: DERIVED and MEASURED. `BagDelta::apply_to` sorts exact paths once, forks one private CoW
root, retains one write zipper, and moves between adjacent paths with `move_to_path`. Underflow or
overflow discards the private root, while success publishes it once. Raw measurements are linked
from [the RSpace integration report](../rspace/pathmap-bag-rspace-integration.md) and the
[`rspace-bag-delta-cursor-2026-08-16.tsv`](../stack-safety/measurements/rspace-bag-delta-cursor-2026-08-16.tsv)
measurement artifact.

Finding: Reopening a zipper at the root for every update repeats the shared prefix. A retained
zipper reuses the longest common prefix of consecutive sorted paths. CoW makes the transactional
shape natural: mutate a private root, validate every count transition, then replace the authority.

Decision: Batch mutations use one ordered zipper traversal. Single-path operations may use
PathMap's ordinary `insert`, `get`, and `remove`: at the pinned PathMap coordinate, `insert` and
`remove` delegate to write zippers while `get` delegates to a borrowed-path read zipper.

### J-003 — 2026-08-17: algebraic operations require semantic eligibility proofs

Question: Should every Rholang set, map, and bag operation be expressed as a PathMap lattice or
algebra operation?

Audit coordinate—not the original experiment coordinate:
`f1r3node-rust-mettail@a6b3ad09032c22bb10fafd6f7a890d7871b36609`, tree
`3388f0e80310d9794857d14650138bb45231a2e4`, with PathMap at
`f62b6f62580cd6bb6db9ce5385bb886fd7183397`. The linked integration report retains the individual
experiment coordinates.

Evidence: DERIVED and MEASURED. Join, meet, subtraction, prefix restriction, subtree extraction,
and zipper grafting were evaluated in the EPathMap and RSpace ladders. Some operations remove a
complete traversal; others require value-side conflict or multiplicity rules that the bare topology
operation cannot decide. The detailed eligibility matrix is in
[PathMap Bag and RSpace integration](../rspace/pathmap-bag-rspace-integration.md).

Finding: Native algebra is optimal when the PathMap value algebra is the language algebra. It is a
conservative prefilter or ineligible when Rholang still needs directional matching, capture
bindings, guard backtracking, overflow refusal, or deterministic candidate order.

Rejected interpretation: `restrict` is not universally beneficial merely because it can discard a
large subtree. If the derived key has no safe nonempty prefix, building a mask costs work without
pruning candidates.

Decision: Every algebraic use carries an eligibility argument and an oracle-equivalence gate.

### J-004 — 2026-08-23: the deep-send slope is before PathMap insertion

Question: Why did the exact `plain_deploy` depth-131,072 release gate exceed its 120-second cap after
the stack-safe traversals passed independently?

Source coordinate: stable repository base
`f1r3node-rust-mettail@a6b3ad09032c22bb10fafd6f7a890d7871b36609`, tree
`3388f0e80310d9794857d14650138bb45231a2e4`. The diagnostic probe also contained uncommitted
working-tree changes whose exact tree and invocation were not preserved. The measurements are
therefore valid for attribution at that checkpoint but are not independently reproducible
acceptance evidence.

Method: A release-only ignored probe split `RSpaceDataIndex::datum_path` into structural RMP
projection, canonical bincode identity encoding, RhoPathV1 framing, and the final Bag insertion.
Every process was pinned to one CPU inside an 8 GiB `MemoryMax`/zero-swap systemd scope. The raw
observations are stored in
[`rspace-structural-key-preparation-2026-08-23.tsv`](../stack-safety/measurements/rspace-structural-key-preparation-2026-08-23.tsv).
Its [adjacent metadata record](../stack-safety/measurements/rspace-structural-key-preparation-2026-08-23.metadata.md)
states the reproducibility limitation without reconstructing an unrecorded command.

| Unary-list depth | Complete key bytes | Structural bytes | Structural time | Identity time | Framing time |
|---:|---:|---:|---:|---:|---:|
| 1,024 | 66,005 | 225 | 39.610 ms | 6.112 µs | 2.384 µs |
| 2,048 | 132,756 | 225 | 152.178 ms | 4.138 µs | 4.038 µs |
| 4,096 | 269,974 | 226 | 604.595 ms | 9.418 µs | 10.570 µs |
| 8,192 | 544,404 | 225 | 2,512.954 ms | 27.051 µs | 16.652 µs |

The structural doubling ratios are 3.842, 3.973, and 4.157. Although the complete key grows
linearly and the structural output remains 225–226 bytes, structural projection grows
quadratically. A second arm called `RSpaceDataIndex::insert`; at depth 8,192 it completed in
2,531.876 ms against 2,512.998 ms for key preparation in the recorded control row. The single
unpaired observation cannot attribute the apparent difference, so no positive insertion cost is
inferred.

Finding: The slope is not evidence that PathMap insertion is quadratic. The structural encoder
first calls `single_expr_match_component`, which decodes the sole authoritative Expr protobuf body
out of ParBag. On a recursive value, that reconstruction repeatedly rebuilds shorter suffix
ParBags before the generated RMP cursor emits a constant-size opaque nested-Par token. The final
PathMap sees only the already-prepared linear byte key.

Figure 1 shows the attribution boundary. Red denotes the measured quadratic control; green denotes
the treatment under implementation.

![RSpace structural-key attribution](figures/rspace-structural-key-attribution.svg)

The figure source is
[`rspace-structural-key-attribution.puml`](figures/rspace-structural-key-attribution.puml).

Rejected interpretations:

1. The test's old depth observer was quadratic, but replacing it did not remove this production
   slope.
2. Parsing and normalization are not the dominant phase: the isolated depth-32,768 normalizer
   completed in 0.24 seconds.
3. Canonical bincode identity is not the dominant phase in this probe: it remained in the
   microsecond range because the generated encoder streams the resident component bytes.
4. Replacing `Bag::insert` with another PathMap write zipper cannot repair work completed before
   the Bag receives its path; PathMap's public insertion already uses a write zipper.

Hypothesis under test: A generated, stack-safe protobuf-region-to-RMP cursor can consume the
canonical Expr body already stored in the ParBag endpoint, cut nested Par regions without decoding
them, and emit the same structural semantics in time linear in bytes visited. The treatment must be
fully differential over the generated schema, preserve candidate-pruning soundness, and version any
derived RMP byte change explicitly.

Decision: Repair the structural projection boundary, not PathMap. Preserve the Bag as
`PathMap<Multiplicity>` and retain ordinary PathMap insertion unless post-repair measurements
attribute a separate cost to it.

### J-005 — 2026-08-23: RMP2 fingerprints semantic Bag records, not ACT bytes

Question: Can the region-native treatment preserve RMP1, and if it can, is doing so the cleanest
long-term representation?

Status: HYPOTHESIS and PROVISIONAL DECISION. This entry is a design candidate, not accepted
performance or equivalence evidence.

Source coordinate: the accepted baseline is
`f1r3node-rust-mettail@a6b3ad09032c22bb10fafd6f7a890d7871b36609`, tree
`3388f0e80310d9794857d14650138bb45231a2e4`. The RMP2 prototype reviewed here was an uncommitted
successor to that baseline, so its final source coordinate remains an explicit closure requirement.

Evidence: DERIVED but incomplete. Two source reviews traced both candidate designs through
`ParConstruction`, `ParBag::visit_canonical_key_parts`, RPB3 key formation, the generated canonical
protobuf emitter, `par_bag_match_summary`, `canonical_par_view`, and PathMap's ACT snapshot. The
review conclusions were:

1. RMP1 can be reproduced in linear time for a deep unary value by building a shared bottom-up
   score/protobuf plan and materializing only the requested root ParBag key and transient ACT.
2. A schema-complete implementation must nevertheless reproduce every sorter equation, malformed
   nonsemantic score, unordered collection rule, RPB3 framing rule, and ACT byte rule merely to
   obtain an opaque derived digest.
3. The candidate design assumes that score and ACT bytes add no pruning information beyond the
   canonical family, component body, and multiplicity already present in the Bag. H-001 must prove
   this through no-false-negative query equivalence and representation-independent candidate order.
4. Hashing the ordinary canonical protobuf component stream literally is also wrong for this
   purpose: protobuf repeats a field once per occurrence, so one endpoint of multiplicity
   `$`m`$` would require `$`m`$` body feeds and would discard the Bag's compressed count.

Provisional finding: RMP2 retains the existing constant-size ParBag token shape—distinct count,
saturated-`u64` cardinality, and one BLAKE2b-256 digest—but changes the digest preimage to a
canonical, multiplicity-compressed endpoint transcript. For each distinct endpoint `$`i`$`, define:

```math
R_i = \operatorname{be}_{32}(f_i)
\mathbin{\|} \operatorname{be}_{64}(m_i)
\mathbin{\|} \operatorname{be}_{64}(|b_i|)
\mathbin{\|} b_i,
```

where `$`f_i`$` is the protobuf component-family tag, `$`m_i`$` is its strictly positive
`Multiplicity(u64)`, and `$`b_i`$` is its canonical component protobuf body. Records occur in the
same family/score/body order used by the resident ParBag. The fixed-width header and explicit body
length make each record self-delimiting and make the record sequence uniquely decodable. They do
not make the set of complete transcripts prefix-free: a one-record transcript is a byte prefix of
the same transcript followed by another record. Prefix-freedom is not required here because the
streaming hash is finalized at the end of the complete record sequence. The digest is:

```math
h = \operatorname{BLAKE2b256}\!\left(
  \mathtt{RhoMatchPath/ParBag/components/v2}
  \mathbin{\|} R_1 \mathbin{\|} \cdots \mathbin{\|} R_d
\right).
```

The enclosing token emits `$`d`$` and the saturated cardinality `$`c`$` exactly once beside `$`h`$`:

```math
c = \min\!\left(2^{64}-1, \sum_i m_i\right).
```

Those counts are not duplicated inside the digest preimage. The digest still commits to each exact
`u64` multiplicity, so saturation of `$`c`$` does not erase the multiplicity distribution from
`$`h`$`. Root `Par.locally_free` and `Par.connective_used` fields are excluded from the endpoint
transcript because the generated Par program emits them separately. The component families are
exactly protobuf tags 1, 2, 4, 5, 6, 7, 8, 11, and 12. Metadata nested inside a component body
remains included in `$`b_i`$`.

```text
Algorithm: emit one RMP2 ParBag token from either authority

Input:
    a validated resident ParBag OR a provenance-admitted canonical Par region
Output:
    PAR_BAG || be64(distinct) || be64(saturated_cardinality) || digest,
    or a validation/size error before any token is emitted

Preconditions:
    resident keys have passed RPB3 validation; a region is borrowed only through that
    provenance and retains canonical family/score/body order

1. Initialize distinct and cardinality to zero and the digest with the exact domain bytes
   `RhoMatchPath/ParBag/components/v2`.
2. Visit component families in canonical family order.
3. For a resident Bag, read `(body, multiplicity)` once per PathMap endpoint.
4. For a canonical region, validate the shell and field order, then coalesce only adjacent equal
   `(family, body)` occurrences. Refuse malformed input, order regression, or checked-counter
   overflow before exposing a token.
5. Feed one self-delimiting record per distinct endpoint into the domain-separated digest.
6. Saturate the externally emitted cardinality at `u64::MAX`; retain exact per-endpoint `u64`
   multiplicities in the digest records.
7. For an empty Bag, hash the domain alone and emit zero for both counts.
8. Finalize and emit the counts and digest. Never construct a score, RPB3 key, ACT, AST, or
   `Vec<Par>`.
```

Resident-route time is `$`\Theta(d + \sum_i |b_i|)`$`, where `$`d`$` is the distinct endpoint count;
digest state is constant-sized and the zipper reuses its path buffer. A canonical protobuf region
is occurrence-expanded, so its route is `$`\Theta(B)`$` in the encoded region size `$`B`$`, which
can equal `$`\Theta(\sum_i m_i |b_i|)`$`. Coalescing reduces digest feeds and retained state; it
cannot remove the cost of scanning and comparing the already-expanded source bytes.

Figure 2 shows why both resident and region-native routes have the same transcript.

![RMP2 compressed ParBag transcript](figures/rmp2-parbag-transcript.svg)

The figure source is [`rmp2-parbag-transcript.puml`](figures/rmp2-parbag-transcript.puml).

Rejected alternatives:

- Preserve RMP1 by reconstructing a transient score-aware ParBag and ACT. This is feasible, but it
  couples a reconstructible matcher index to RPB3 and ACT and creates a large equivalence surface
  with no additional selectivity.
- Hash expanded protobuf component fields. This makes the digest consume
  `$`\Theta(\sum_i m_i |b_i|)`$` body bytes rather than `$`\Theta(d + \sum_i |b_i|)`$` on the
  resident route. The region route must still scan its occurrence-expanded `$`\Theta(B)`$` input.
- Hash component bodies without their family tags or multiplicities. Equal cardinality does not
  identify the multiplicity distribution, and the same body bytes in different families have
  different Rholang meaning.

Provisional decision: Evaluate RMP2 as a versioned derived-index format rebuilt from the
authoritative Bag. Before acceptance, pin the prototype's source coordinate and prove
resident/region transcript equality, saturated-cardinality boundary behavior, no-false-negative
query equivalence, representation-independent candidate order, replay/post-state equivalence, and
linear depth/width resource slopes. Record any accepted cutover under CBR-053 because candidate
pruning is verdict-adjacent even though RMP bytes are not a wire or cold-store identity.

Amendment — 2026-08-23 documentation audit: J-005 remains provisional. This amendment replaces the
incorrect transcript-level prefix-free claim with the exact self-delimiting/uniquely-decodable
property, records the existing saturated-`u64` cardinality projection, separates resident and
region route complexity, and makes the missing immutable prototype coordinate and equivalence
evidence explicit. No RMP2 acceptance or performance conclusion is claimed by this entry.

### J-006 — 2026-08-23: derived paths prune; identity orders datum candidates

Question: May a derived RMP path break a tie between datum candidates when candidate selection is
visible to replay?

Status: DERIVED design invariant with PROVISIONAL implementation evidence. The invariant is
consensus-preserving; the current encoded implementation is not accepted until the focused and
play/replay gates below pass.

Source coordinate: the accepted baseline is
`f1r3node-rust-mettail@a6b3ad09032c22bb10fafd6f7a890d7871b36609`, tree
`3388f0e80310d9794857d14650138bb45231a2e4`. The candidate-order implementation reviewed here is an
uncommitted successor. Its relevant functions are
[`compare_candidate_identity`, `sort_encoded_candidates`, and
`validate_unique_candidate_identities`](../../../rspace++/src/rspace/data_index.rs),
`RSpaceDataIndex::ordered_encoded_channel_entries` and
`RSpaceDataIndex::ordered_channel_entries` in the same file, and
[`deterministic_candidate_hash`](../../../rspace++/src/rspace/candidate_order.rs) for the shared
BLAKE2b-256 primitive.

Finding: RMP is a reconstructible pruning index. Changing its version may change which PathMap
branch finds a candidate, but must never change which admissible datum is first. For canonical datum
identity bytes `$`I(x)`$`, datum candidates use the total order:

```math
K(x) = \left(\operatorname{BLAKE2b256}(I(x)), I(x)\right).
```

The encoded API parses the already-stored identity suffix, hashes that suffix once, sorts by
`$`K(x)`$`, and retains the suffix offset. The decoded API consumes that same ordered encoded list
and decodes the same suffix lazily or on first access. It does not re-encode a second byte authority.

```text
Algorithm: order one channel's stored datum candidates

Input:
    validated RhoPathV1 datum paths below one channel prefix
Output:
    candidates in canonical identity order, or an error before selection

1. Parse each path and borrow its canonical datum-identity suffix.
2. Decorate the entry with BLAKE2b-256 of that suffix exactly once.
3. Sort by digest and then by suffix bytes; never compare the derived RMP prefix.
4. Refuse an identical suffix found under distinct derived paths.
5. Retain one exact storage path per distinct identity for removal and lazy decode.
6. Preserve exact repetitions as the one Bag endpoint's positive multiplicity.
```

The duplicate rule distinguishes two cases. An exact repeated datum has one identical complete Bag
path and increases its `Multiplicity`; it is not a second catalog entry. The same identity suffix
under distinct derived paths contradicts the single-authority invariant and is malformed, so the
catalog refuses it rather than allowing an RMP version to choose an order.

Rejected interpretations:

1. Full trie-path order is not canonical candidate order: it would make an RMP version or encoding
   change replay-visible.
2. Hash-only order is not total unless collision behavior is specified. Canonical identity bytes
   are the collision tie-breaker.
3. Decode then re-encode for ordering creates a second identity authority and adds avoidable work.

Provisional decision: Keep candidate pruning and candidate ordering separate. Before implementation
acceptance, pass focused opposing-RMP/hash-collision/duplicate-identity/multiplicity tests, prove the
encoded and decoded APIs return byte-identical ordering keys, and run play/replay COMM-selection and
post-state equivalence gates. Append a dated acceptance or rejection amendment; do not silently
promote this entry into the accepted-rule table.

Checkpoint amendment — 2026-08-23: accepted local comparator rule; end-to-end replay gate open. The
vector-backed continuation order now uses the same collision rule as the PathMap-backed datum
catalog: `(BLAKE2b-256 digest, complete canonical identity bytes)`. Store index is consulted only
for byte-identical repeated continuation entries because those entries require distinct removal
addresses. A test-only digest function returned the same 32-byte value for every candidate. Both
forward and reverse insertion orders produced `a`, `same`, `same`, `z`; the two equal identities
retained their original distinct store indices. The focused unit passed 1/1 in systemd scope
`run-p2360232-i10336581`, under CPUs 0–7, `MemoryHigh=8G`, `MemoryMax=12G`, and zero swap.

This evidence proves the local total-order rule under an induced digest collision. It does not yet
prove that removal, reinsertion, COMM selection, and replay preserve the same result. Those gates
remain reachable through pgmcp task 5988 and must pass before J-006 becomes an accepted global
PathMap use rule.

Checkpoint amendment — 2026-08-23: ACCEPTED END-TO-END GATE; full release OPEN. A `cfg(test)`-only
thread-local RAII injector now replaces only the datum-catalog identity digest. Its guard is
`!Send`, the test uses Tokio's current-thread runtime, and each Rust test thread has independent
state. Production builds compile no injector state, conditional branch, field, layout change, or
API. The gate forces every identity digest to the same 32-byte value and first proves that its two
datums have opposing orders: derived RMP bytes prefer one datum, while canonical identity bytes
prefer the other.

The complete play/replay gate passed 1/1 in systemd scope `run-p2600563-i11051253`, under CPUs
0–7, `MemoryHigh=6G`, `MemoryMax=8G`, and zero swap. It proved:

- canonical-identity selection and bind order under the forced collision;
- atomic removal of multiplicity two from one Bag endpoint;
- reinsertion followed by the same canonical selection;
- five Produce input/output events, two Consume input/output events, and two COMM events;
- `times_repeated` values two and four across the two COMMs;
- consumption of every rigged replay COMM;
- equality of the final expanded state map; and
- equality of the play and replay post-state root.

This closes the collision/removal/reinsertion/replay obligation in pgmcp task 5988 at the focused
system coordinate. Stable commit and full-release evidence remain before lifecycle completion.

### J-007 — 2026-08-23: classify nested Par shells before summarizing them

Question: When may the protobuf-region route compute a nested ParBag summary without recreating a
quadratic suffix walk?

Status: DERIVED root-cause hypothesis and PROVISIONAL design rule. No treatment or geometric result
is claimed yet.

Source coordinate: the accepted baseline is
`f1r3node-rust-mettail@a6b3ad09032c22bb10fafd6f7a890d7871b36609`, tree
`3388f0e80310d9794857d14650138bb45231a2e4`. The reviewed RMP2 prototype is an uncommitted successor.
The relevant current functions are `CanonicalRhoPathRegionCursor::next`'s `Work::Par` arm,
`canonical_par_view`, `AdmittedParRegion::classify`, and `AdmittedParRegion::summary` in
[`match_region_path.rs`](../../../models/src/rust/rholang/match_region_path.rs).

| Evidence class | Current artifact or gap |
|---|---|
| Source derivation | The named functions above at an uncommitted successor to the exact baseline. This supports the root-cause hypothesis but is not an immutable implementation coordinate. |
| Executable bridge | None yet. The exact probe marker, command, resource envelope, and raw output must be registered and linked before execution. |
| Formal bridge | None yet. Add the exact theorem/model marker and checked artifact only after it exists; no formal verification is claimed here. |
| Measurement | None yet. No timing, allocation, or slope observation supports this entry. |
| Equivalence | None yet. Emitted-token, malformed-shell, candidate, COMM, and replay gates remain open. |

Derived hypothesis: Eager summarization has two different cost contexts. The opaque datum route reaches one
nested Par, emits one constant-size ParBag token, and hashes that selected Par region once. That is
safe. A dynamic query compiler may instead classify the nested Par, descend through its sole Expr,
then repeat at the next nested Par. If classification eagerly hashes the complete remaining Par
region before each descent, a depth-`$`n`$` unary carrier performs:

```math
\sum_{k=0}^{n-1} \Theta(n-k) = \Theta(n^2)
```

byte work even though each individual cursor step is stack-safe.

The required separation is:

```text
Algorithm: classify and emit a nested Par region lazily

Input:
    a provenance-admitted nested Par region
Output:
    a dynamic structural descent or one opaque ParBag token

1. Validate only the current Par shell needed for classification: field framing, canonical order,
   root metadata, component count, and the sole-Expr distinction.
2. Skip nested length-delimited payloads by their validated extents; do not hash their bodies.
3. If the dynamic classifier descends through a sole Expr, pass its borrowed region onward without
   computing the enclosing ParBag digest.
4. If the compiler emits an opaque or metadata-bearing whole-value token, compute exactly that
   selected region's summary once and emit it.
5. Refuse a malformed shell before descent or token emission; arbitrary bytes never acquire
   provenance through this classifier.
```

This design does not weaken canonical validation. The cheap classifier is available only through
the private provenance-bearing region handle minted from an authoritative RPB3 endpoint. It avoids
revalidating and rehashing already-admitted nested bodies while retaining exact shell errors at the
decision boundary.

Rejected interpretations:

1. Stack-safe iteration alone does not imply linear time; an explicit PDA can still visit nested
   suffixes quadratically.
2. Memoizing every suffix digest retains memory proportional to depth and computes digests the
   dynamic route may never emit.
3. Removing ParBag summaries globally would weaken the opaque datum token. The repair is lazy
   placement, not removal.

Provisional decision: Separate provenance/shell classification from opaque-token summarization.
Before acceptance, implement the lazy boundary, prove emitted-token equality and malformed-shell
refusal, count exactly one summary for the selected opaque token, and run a geometric deep-unary
query ladder demonstrating a linear time/allocation slope. Append the result as a dated amendment.

Supersession: none. A later result must append an explicitly dated acceptance, rejection, or
supersession amendment, link the immutable implementation coordinate and evidence artifacts, and
update H-005. It must not rewrite this hypothesis into an observed result.

### J-008 — 2026-08-23: admit canonical explicit value-free endpoints

Question: Is every non-root ACT semantic node without a value, child, or compressed line a
redundant empty branch?

Status: DERIVED, MEASURED, and DECISION for the repository validator rule. The focused repair is
accepted at its test checkpoint. The implementation coordinate remains an uncommitted successor to
`f1r3node-rust-mettail@a6b3ad09032c22bb10fafd6f7a890d7871b36609`, so immutable-coordinate and
broader campaign acceptance remain open until a stable commit and the EPM1 malformed-input,
protobuf/bincode, replay, and full-release gates pass. This distinction preserves the successful
focused evidence without presenting an uncommitted tree as a release result.

Tracking coordinate: pgmcp bug 5981,
`admit-canonical-pathmap-writer-images-with-explicit-value-free-epathmap-endpoints-e060a6`.
Implementation coordinates in the uncommitted successor are
[`semantic_act_node`](../../../rspace++/src/rspace/pathmap_codec.rs), the writer-image corpus in
[`epathmap_trie_codec.rs`](../../../models/src/rust/epathmap_trie_codec.rs), and the public EPM1
round-trip gate in
[`epathmap_epm1_snapshot.rs`](../../../models/tests/epathmap_epm1_snapshot.rs).

Source derivation: PathMap's public topology operation can create a path without installing a
value. If that path's final edge diverges from another root child, the pinned canonical ACT writer
may encode the endpoint as a reached childless branch node with no value. The node is not absence:
`CompactTrie::visit` yields the accumulated path, `CompactTrie::visit_topology` yields its logical
node, and reconstruction calls `create_path` for it. The former validator rejected this writer
image as an “uncompressed empty non-root branch.” That predicate confused physical value absence
with logical path absence.

```text
Algorithm: validate one semantic ACT node without erasing an explicit endpoint

Input:
    one physical ACT node after graph, offset, branch-order, and line-framing validation
Output:
    one canonical semantic node, or a precise alternate/malformed-encoding error

1. Resolve a branch node into its optional value and strictly ordered children, or resolve a line
   node into its nonempty suffix, optional value, optional branch wrapper, and wrapper children.
2. Refuse a line that has both its own value and a child; refuse a line child that is not a branch.
3. Refuse a value-free semantic node with exactly one child because the canonical writer would
   line-compress that unary path.
4. Refuse a line branch wrapper with no children because it cannot represent a branch.
5. Admit a reached childless semantic node with no value and no line. Preserve it as an explicit
   path endpoint; do not manufacture a value.
6. Replay values with `set_value`, value-free endpoints with `create_path`, and require the
   canonical writer image to be a byte fixed point after reconstruction.
```

The repair removes only the obsolete rejection in step 5. Its invariants are:

1. Logical topology is authoritative: a parent edge reaching a semantic node establishes the path
   even when the node has no value.
2. Set/map values remain distinct from value-free endpoints. EPM1 value ordinals are still
   validated before topology events become observable.
3. For every pinned-writer image `$`B`$`, decoding and re-encoding must satisfy:

```math
\operatorname{encode}(\operatorname{decode}(B)) = B.
```

4. A live zipper and direct ACT topology visitor must emit the same root/value, edge/value, and
   postorder-leave event sequence.
5. The repository changes no PathMap source and introduces no second trie authority.

Negative classes remain negative. In particular, the validator still refuses value-free unary
branches; branch bytes that are not strictly increasing; inconsistent sparse/dense child metadata;
shared child references; overflowing or out-of-arena offsets; reserved line header bits; empty or
overlapping line data; a line node with both value and child; a non-branch line child; and a branch
wrapper without children. Admitting one canonical endpoint shape therefore does not admit arbitrary
empty subgraphs or alternate encodings.

Focused evidence, all from the same uncommitted successor:

| Gate | Result |
|---|---:|
| `compact_trie_topology_stream_matches_native_zipper_for_all_endpoint_shapes` | 1/1 passed |
| `root_divergent_value_free_endpoint_round_trips_the_canonical_epm1_image` | 1/1 passed |
| `rust::epathmap_trie_codec::tests::direct_act_replay_matches_reconstruction_oracle_on_writer_images_and_mutations` | 1/1 passed |
| Complete `models/tests/match_path_structure.rs` suite | 7/7 passed |

The complete suite ran in systemd scope `run-p2191610-i10594105` with zero swap: 112.393 seconds
wall time, 109.920 seconds CPU time, and 99.7 MiB peak resident memory. These figures are resource
evidence for the focused gate, not a throughput benchmark.

Rejected interpretation: weakening all value-free-node checks is neither necessary nor correct.
Only a childless reached endpoint has the newly established writer provenance; a value-free unary
node remains redundant because canonical line compression represents it differently.

Decision: Repository ACT validators recognize the logical PathMap endpoint represented by the
canonical writer's value-free childless branch. Preserve all other canonicality and malformed-input
checks. Amend this entry with the stable commit and broader gates rather than rewriting the focused
checkpoint.

### J-009 — 2026-08-23: cache only the versioned RMP2 digest across a CoW clone family

Question: After the RMP2 transcript becomes linear, should repeated derived-path construction walk
and hash an unchanged resident ParBag every time?

Status: HYPOTHESIS and PROVISIONAL DESIGN. No implementation or performance improvement is claimed.
The design task is pgmcp 5983,
`memoize-the-versioned-rmp2-parbag-digest-across-copy-on-write-clone-families-c92231`; its
preregistered experiment is pgmcp 5984,
`measure-cold-warm-clone-shared-and-post-mutation-rmp2-digest-reuse-aea03e`.

Source derivation: the resident RMP2 route hashes the same versioned endpoint transcript whenever
it builds a derived RhoPath. `ParBag` already shares immutable auxiliary cells through
`Arc<ParBagAux>`, so an unchanged clone family has one natural cache owner. Distinct count and
cardinality are already constant-time Bag summaries; caching them would add invalidation surface
without removing a trie walk. The only proposed payload is the 32-byte RMP2 BLAKE2b-256 digest in
one `OnceLock<[u8; 32]>` whose field name and type are explicitly tied to RMP2.

```text
Algorithm: read or initialize the clone-family RMP2 digest

Input:
    one immutable resident ParBag and its shared ParBagAux root
Output:
    the exact 32-byte RMP2 endpoint-transcript digest

1. If the RMP2 OnceLock is initialized, borrow its 32-byte value and visit zero endpoints.
2. Otherwise ask the OnceLock to initialize exactly once across concurrent readers.
3. In the initializer, visit canonical ParBag endpoint records once, feed the exact J-005 domain
   and record grammar into BLAKE2b-256, and store only the finalized 32-byte digest.
4. Read distinct count and saturated cardinality from the Bag's existing constant-time summaries;
   do not duplicate them in the cache.
5. Never cache a protobuf-region summary because a borrowed region has no stable owning clone
   family.
```

```text
Algorithm: preserve or invalidate the RMP2 digest on mutation

Input:
    an operation on a ParBag whose auxiliary root may be shared and warm
Output:
    a semantically correct cache state for every surviving clone

1. Validate the requested component transition and its checked multiplicity before publishing a
   mutation.
2. For a failed transition or semantic no-op, preserve the existing auxiliary root and digest.
3. For a metadata-only Par mutation, preserve the digest because J-005 excludes root metadata.
4. For a successful component mutation on a uniquely owned auxiliary root, clear the RMP2 cell
   together with the other component-derived cells.
5. For a successful component mutation on a shared root, detach the writer to a new auxiliary root
   with an empty RMP2 cell; leave every unmodified clone on the original warm root.
6. Recompute lazily on the first subsequent RMP2 request. Never retain transcript bytes, install a
   global cache, or substitute an ACT/PathMap merkle hash for the versioned RMP2 digest.
```

Hypothesis: one cold request retains the current
`$`\Theta(d + \sum_i |b_i|)`$` endpoint work, while a warm request becomes `$`\Theta(1)`$` with
fixed auxiliary storage independent of endpoint count and body size. The hypothesis is rejected if
the cold lane regresses by more than 5%, if a warm request visits an endpoint, if retained cache
memory slopes with transcript size, or if any mutation/concurrency/byte-equivalence gate fails.

The preregistered experiment crosses distinct widths 1, 16, 64, 256, and 1,024; component-body
sizes 16, 256, and 4,096 bytes; and repeat counts 1, 2, 4, 8, and 32. Its lanes are no-cache control,
cold cache, warm cache, prewarmed CoW clones, first post-mutation request, and second post-mutation
request. It records wall time, instructions, data reads, allocations, and headless Heaptrack
retained memory under pinned CPU, zero swap, capped RSS, and an idle-host preflight.

Acceptance additionally requires exact RMP2 bytes and oracle equivalence; one initialization per
clone family under concurrent readers; the complete successful/failed/no-op/metadata/component
invalidation matrix; zero warm endpoint visits; no warm transcript-size slope; fixed retained
growth per auxiliary root; candidate, COMM, and replay neutrality; and a checked formal cache
invariant. For transcripts of at least 256 KiB, the warm lane must improve by at least five times
over no-cache. A small singleton need only satisfy the cold-regression limit. Negative results stay
in this journal.

Provisional decision: exploit existing CoW ownership, but cache only the immutable,
version-specific digest whose recomputation is expensive. Do not generalize this into a shadow
representation or unbounded memoization scheme.

Checkpoint amendment — 2026-08-23: ACCEPTED FOCUSED IMPLEMENTATION; performance conclusion OPEN.
The cache is one `OnceLock<[u8; 32]>` in the existing `Arc<ParBagAux>` CoW root. Production
encoding and the measurement-only uncached control call the same private transcript function, so
the experiment does not compare two independently maintained algorithms. Seven focused tests
passed 7/7 in systemd scope `run-p2372458-i10775331`, under a 7 GiB memory maximum and zero swap:

- cold then warm access initialized exactly once;
- untouched CoW clones shared that one initialization;
- mutation detached and invalidated only the changed clone;
- every component mutation route invalidated lazily;
- metadata-only changes, no-ops, overflow failures, and underflow failures retained the digest;
- eight concurrent readers initialized once; and
- cached digest and token bytes equaled a contiguous transcript oracle.

The auxiliary-root budget gate passed in scope `run-p2387397-i10640371`; the observed 64-bit
`ParBagAux` size was 672 bytes. The test enforces 672 bytes as an upper budget, allowing a future
compiler or layout improvement to reduce the size without failing. This measurement does not show
that warm hashing is faster or that retained memory is flat across transcript sizes. Those claims
remain preregistered under experiment 5984, together with instructions, allocations, headless
Heaptrack, mutation-lane, and cold-regression evidence.

Measurement amendment — 2026-08-23: FULL MATRIX MEASURED; PROFILE CLOSURE OPEN. The complete raw
matrix is
[`rmp2-par-bag-digest-cache-2026-08-23.tsv`](../stack-safety/measurements/rmp2-par-bag-digest-cache-2026-08-23.tsv),
SHA-256 `2fa2d445ed410116ed9a0b1beadaeb878edbf8e808d813866a690ae2b619c65b`.
It contains 13,500 observations over 75 cells, six lanes, and 30 retained samples per lane. All
lane checksums agree, and warm, prewarmed-CoW, and second-post-mutation accesses allocate zero
bytes. Across the matrix, median speedups were 762.66 times for uncached over warm, 762.67 times
for cold over warm, and 772.44 times for first over second post-mutation access. At the largest
cell—1,024 distinct 4,096-byte bodies with multiplicity 32—the uncached, cold, warm, prewarmed-CoW,
first-post-mutation, and second-post-mutation medians were respectively 7,221,289; 7,176,509;
175.5; 155; 7,220,527; and 151 ns. Multiplicity did not create occurrence-expanded allocation or
time work because each distinct trie endpoint contributes one transcript record containing its
checked multiplicity.

One 609-byte cell initially violated the 5% cold-regression guard: its 30-pair cold median was
7.36% above the uncached median. The negative cell remains in the full matrix. A selected-cell
replication reused the identical six-lane harness with 30 warmups and 300 retained pairs. Its raw
record and analysis are
[`rmp2-par-bag-digest-cache-tiny-replication-2026-08-23.metadata.md`](../stack-safety/measurements/rmp2-par-bag-digest-cache-tiny-replication-2026-08-23.metadata.md).
Cold and uncached medians were 1,704 and 1,703 ns. A 200,000-replicate paired bootstrap placed the
ratio-of-means 99% interval at [0.934846, 1.049629], below the preregistered 1.05 ceiling. The
larger replication therefore attributes the first result to short-sample noise; production gained
no transcript-size threshold, bypass, or other overfit special case.

The deterministic and retained-memory profiles are
[`rmp2-par-bag-digest-cache-resource-profiles-2026-08-23.md`](../stack-safety/measurements/rmp2-par-bag-digest-cache-resource-profiles-2026-08-23.md).
After matched fixture subtraction, a warm lookup requires approximately 29 instructions, 13 data
reads, and 9 data writes at all three registered sentinels from 69 through 4,214,817 transcript
bytes. Headless Heaptrack records exactly equal allocation-call totals for warm and matched
warm-null processes across 10,000 requests at every sentinel. Cold initialization adds temporary,
transcript-linear work but no retained heap allocation: the digest is inline in the existing
fixed-budget auxiliary root. The wall, allocation, instruction, data-access, RSS, mutation,
concurrency, and semantic evidence accepts H-006's resource hypothesis. Broader COMM/replay and
full-release gates remain open lifecycle checks.

### J-010 — 2026-08-23: retained candidate zipper clones may copy quadratic path state

Question: Does retaining `CandidateProductCursor` at every completed query frame defeat the
locality of PathMap zipper traversal?

Status: DERIVED HYPOTHESIS. No timing, allocation, or asymptotic treatment result is claimed. The
reachable tracking item is pgmcp bug 5985,
`remove-depth-quadratic-pathmap-zipper-cloning-from-retained-candidate-product-matching-49a707`.

Source coordinate: the uncommitted successor's
[`CandidateProductCursor` and `visit_sequence_frames_with_prefix`](../../../rspace++/src/rspace/data_index.rs),
audited against PathMap 0.2.2 at `f62b6f62580cd6bb6db9ce5385bb886fd7183397`.
`CandidateProductCursor` derives `Clone` over two `ReadZipperUntracked` values: one for candidate
availability and one for candidate positions. At the pinned PathMap coordinate,
`ReadZipperCore::clone` copies both its complete `prefix_buf` and its `ancestors` vector.

The candidate scanner itself moves its paired zippers down and back up efficiently. At every
complete stored sequence frame, however, `visit_sequence_frames_with_prefix` calls
`visitor(cursor.clone(), &frame)`. Wildcard, first-capture, and nested-match callbacks retain that
clone in `CandidateQueryState`; a terminal state clones once more before the end marker; and an
`accepts_remainder` route enumerates another frame and re-enqueues the same atom index. If a
depth-`$`n`$` chain retains a clone at each depth `$`k`$`, copied path state has the hypothesis:

```math
\sum_{k=1}^{n} \Theta(k) = \Theta(n^2).
```

This equation is conditional on the registered adversarial shape actually reaching the named
clone sites. The required clone/allocation counters are anti-vacuity evidence; source inspection
alone does not establish the production slope.

```text
Algorithm candidate: match with one moving candidate-product zipper PDA

Input:
    the availability and position PathMaps plus one compiled RMP query
Output:
    the same candidate positions and capture bindings as the current matcher

1. Open one synchronized availability/position zipper pair at the candidate prefix.
2. Store DFS continuation state on an explicit heap stack: next child mask position, sequence-frame
   scanner state, query atom, remainder state, and capture rollback/persistent-root state.
3. Descend and ascend the same zipper pair in place. Evaluate a complete frame at the current
   focus before moving again; do not retain a full zipper clone in a work item.
4. On a branch, retain only the compact continuation needed to resume sibling enumeration. Restore
   captures through rollbackable deltas or persistent roots rather than copying descendant paths.
5. At a terminal marker, read the candidate position at the current focus and continue the same
   DFS. Preserve deterministic output sorting and deduplication after traversal.
6. If the unmodified PathMap exposes a demonstrably constant-cost subtree checkpoint with the same
   semantics, measure it as an alternative. Do not fork PathMap.
```

Acceptance requires a preregistered geometric depth/width resource ladder; counters proving that
the control exercised zipper clones and the treatment does not; allocation, time, and RSS slopes;
exact old/new candidate and binding differentials; malformed-input refusal; COMM, play/replay, and
post-state equivalence; and a small-native-stack deep gate. An optimization that merely moves the
same copied `$`prefix_buf`$` or ancestor history into another retained object is rejected.

Provisional decision: preserve PathMap as both candidate tries and preserve moving-zipper locality.
Replace retained zipper snapshots only after the executable ladder confirms the source-derived
slope and the single-moving-zipper PDA proves identical search/backtracking semantics.

Checkpoint amendment — 2026-08-23: DESIGN DECISION, implementation and measurement still open.
Source audit of the pinned PathMap API identified a smaller first treatment than the full
single-moving-zipper PDA: construct a bounded suffix `PathMap` from the current subtree through
`ZipperSubtries::make_map`, then open a shallow read zipper at that suffix root. The suffix map
shares immutable trie structure through PathMap's CoW representation; it does not copy the complete
ancestor vector or depth-growing prefix buffer. A radix frame has at most 256 children, so each
transition retains a depth-independent number of suffix roots and opens at most two shallow zipper
roots. The encoded sequence terminator `[0, 0]` remains a transient scan state because a checkpoint
after the terminator would change frame-boundary semantics.

The executable control must remain available in the same tree so the experiment can compare the
old retained-zipper clone and the CoW suffix checkpoint on identical inputs. Escalate to the
single-moving-zipper PDA if the checkpoint treatment either leaves a fitted exponent above 1.2 or
regresses a representative or dense-width case by more than 10 percent. This selection does not
accept H-007: pgmcp bug 5985 and experiment 5990 retain the old/new semantic differential,
anti-vacuity counters, geometric resource ladder, COMM/replay, and small-stack requirements.

Checkpoint amendment — 2026-08-23: ACCEPTED FOCUSED IMPLEMENTATION; resource conclusion OPEN. The
production matcher now stores `CandidateProductCheckpoint` values containing CoW suffix maps and
reopens a shallow paired cursor for each continuation. A source search finds no production clone of
the paired read zipper; the former algorithm remains only as the same-tree test control. Four tests
passed 4/4 in systemd scope `run-p2383353-i10776044`, with a 6 GiB memory maximum and zero swap:

- checkpoint traversal equaled both the former PDA and the byte oracle;
- terminal endpoints were scanned transiently without losing a root value;
- compressed-prefix and dense-radix shapes agreed; and
- the legacy control proved that the adversarial depth shape reached the clone sites.

At depth 128, the control visited 128 frames, cloned 129 zipper pairs, and attributed more than
500,000 cumulative prefix bytes to those clones. This is anti-vacuity evidence, not an allocation
measurement: allocator traffic, timing, resident-set size, and the fitted exponent remain open in
pgmcp experiment 5990. The candidate-pruning integration subsequently passed 10/10 in systemd
scope `run-p2394525-i10842339`, under CPUs 0–7, `MemoryHigh=6G`, `MemoryMax=8G`, and zero swap. It
covered direct scan, prefix false positives, retained-automaton Bag product, datum-first
continuation pruning, framed-product trace and post-state equivalence, indexed/full-scan
equivalence, pre-decode pruning, nested `Match`, and wildcard-frame pruning. COMM/replay,
small-stack, geometric resource, and release gates remain open.

Measurement amendment — 2026-08-23: NEGATIVE TREATMENT PRESERVED, REVISED TREATMENT ACCEPTED. The
first complete paired matrix measured the bounded CoW suffix checkpoint at every accepted query
atom. Its raw record is
[`rspace-candidate-suffix-checkpoint-v1-2026-08-23.tsv`](../stack-safety/measurements/rspace-candidate-suffix-checkpoint-v1-2026-08-23.tsv),
SHA-256 `3d6f3e2d67808bbb45a37c6cb2b4440d651eda1350cfc5e98cce37e4a9d6ec34`.
That treatment removed the adversarial clone slope and accelerated branching queries, but exact
and repeated-capture queries were approximately twice as slow as the frozen control. The result
rejected checkpoint-per-atom as a production design; it was not deleted or relabeled as a win.

The revised pushdown automaton consumes maximal runs of exact atoms and already-bound captures on
one moving paired cursor. It creates a CoW suffix checkpoint only after a branching frame passes
its predicate. Terminal inspection borrows and restores the same cursor, and a failed asymmetric
descent restores both product zippers independently. This keeps the prefix-compressed
availability and position PathMaps authoritative while allocating persistent suffix roots only
where ownership crosses a branch.

The second complete paired matrix is
[`rspace-candidate-suffix-checkpoint-v2-2026-08-23.tsv`](../stack-safety/measurements/rspace-candidate-suffix-checkpoint-v2-2026-08-23.tsv),
SHA-256 `0bc9cf4a55d10c32b7e0b1faee5b34eb051334c7cdf1400336773146625e5cf8`.
It contains 3,000 observations across 50 cells and 30 retained pairs per cell. Every result equals
the frozen control and byte oracle. The smallest paired median speedup was 1.387 times; median
speedups by query were 1.422 for exact, 1.419 for repeated capture, 2.330 for wildcard, 3.647 for
remainder, and 6.111 for nested match. At depth 4,096, the nested-match speedup was 95.522 times on
the compressed topology and 57.604 times on the dense topology. The capped whole run used no swap
and reached 659,604 KiB peak resident set size. It incurred 2,717 major faults, so only its paired
within-run comparisons support the latency conclusion.

The headless Heaptrack record
[`rspace-candidate-suffix-checkpoint-v2-heaptrack-2026-08-23.md`](../stack-safety/measurements/rspace-candidate-suffix-checkpoint-v2-heaptrack-2026-08-23.md)
attributes 7 fewer allocation calls per exact operation and 14 fewer per repeated-capture
operation at depth 4,096. The Callgrind record
[`rspace-candidate-suffix-checkpoint-v2-callgrind-2026-08-23.md`](../stack-safety/measurements/rspace-candidate-suffix-checkpoint-v2-callgrind-2026-08-23.md)
finds a lower process instruction count for the hybrid at every depth from 256 through 4,096,
with linear growth in both arms. Finally, exact, repeated-capture, and wildcard queries completed
at depth 16,384 on a 256 KiB native thread stack, with no stack-size environment override,
`stacker`, or traversal cap. The focused semantic, resource, and stack gates accept the
deterministic-run hybrid. Broader COMM/replay and full-release gates remain open.

### J-011 — 2026-08-23: dynamic AC queries must not constrain matcher-ignored fields

Question: Can the RMP2 query compiler exact-encode every nonmetadata protobuf field of a dynamic
`New`, `ReceiveBind`, or `MatchCase` value?

Status: DERIVED correctness finding; repair OPEN. This entry establishes the soundness obligation
and does not claim a treatment, test pass, or release result. The reachable tracking item is pgmcp
bug 5982,
`prevent-dynamic-ac-rmp2-queries-from-exact-encoding-matcher-ignored-fields-f8309e`.

Source coordinate: the uncommitted successor's `MatchValue::rho_path_query` in
[`spatial_matcher_pda.rs`](../../../rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs)
routes `New`, `Match`, and `ReceiveBind` into the generated query compiler.
[`compile_ordered_nested_query`](../../../rholang/src/rust/interpreter/matcher/match.rs) exact-encodes
nonmetadata fields unless a recognized dynamic carrier replaces a region with a wildcard or the
compiler declines the query. The authoritative matcher equations in `evaluate_new`,
`evaluate_receive_bind`, and `evaluate_match_case` compare fewer fields:

| Value | Authoritative spatial relation | Ignored nonmetadata fields exact-encoded by the affected dynamic lane |
|---|---|---|
| `New` | equal `bind_count`, then match `p` | `uri`, `injections` |
| `ReceiveBind` | equal `patterns`, then match `source` | `remainder`, `free_count` |
| `MatchCase` | equal `pattern`, then match `source` | `free_count`, `guard` |

Root metadata such as `New.locally_free` is already handled by the compiler's metadata wildcard
route and is not the defect identified here. The defect is exact byte comparison of ignored
nonmetadata fields in an otherwise dynamic AC query. Two values can differ only in one listed field,
be accepted by the authoritative relation, yet be separated by the derived query. That violates
sound-candidate inclusion:

```math
\operatorname{authoritative\_match}(p, d)
\Longrightarrow
\operatorname{query\_accepts}(Q(p), \operatorname{RMP2}(d)).
```

```text
Algorithm requirement: compile only an authoritative semantic projection

Input:
    one dynamic AC pattern and the authoritative relation for its constructor
Output:
    a sound derived query, or no pruning query

1. Enumerate the constructor fields compared by the authoritative relation.
2. Compile exact structure and dynamic holes only for those compared fields.
3. Replace each matcher-ignored field with a whole-field wildcard whose framing cannot constrain
   its contents.
4. If the generated query grammar cannot express that projection without constraining ignored
   bytes, return no query (`ListCandidatePlan::All`) and scan the authoritative candidate domain.
5. Never return a narrower approximation. The authoritative spatial PDA remains the verdict
   authority after candidate admission.
```

Two treatments remain eligible: a schema-complete semantic projection of the ignored fields, or
the fail-closed `All` fallback for the affected dynamic roots. The first may retain more pruning;
the second is the conservative reference because it cannot introduce a false negative. Selection
requires explicit cost and complexity evidence after correctness.

Acceptance requires constructor-specific counterexamples in which only each ignored field differs;
an oracle-success-implies-query-accept property across the registered corpus; unchanged ground
exact behavior; malformed-query refusal; candidate-set and binding equivalence; and COMM,
play/replay, and post-state equivalence. Until those gates arrive, the implementation must not be
reported repaired, and H-008 remains open.

Checkpoint amendment — 2026-08-23: ACCEPTED FOCUSED REPAIR; system gates remain open. The dynamic
query compiler now declines direct `MatchValue` pruning for `New`, `Match`, and `ReceiveBind` when
the value is not ground. Returning `ListCandidatePlan::All` is the sound fail-closed projection:
the authoritative spatial matcher sees every resident candidate and remains the sole verdict
authority. Ground values retain the exact byte lane.

Four direct-row controls passed: one counterexample each for `New`, `ReceiveBind`, and `MatchCase`,
plus the ground exact lane. The complete registered property
`compiled_rho_path_query_is_a_superset_of_every_recursive_oracle_success` also executed and passed
1/1 in 0.40 seconds in systemd scope `run-p2315545-i10756324`, under CPUs 0–7,
`MemoryHigh=8G`, `MemoryMax=12G`, and zero swap. These results close the constructor-level
false-negative demonstrated by J-011. They do not establish COMM selection, play/replay post-state
equivalence, or the full release suite, so pgmcp bug 5982 remains in progress rather than complete.

### J-012 — 2026-08-23: protobuf projection must fail closed across schema and oneof boundaries

Question: Can a generated protobuf-region projector safely assume field positions and oneof
well-formedness when RMP2 bytes participate in candidate pruning?

Status: ACCEPTED FOCUSED REPAIR; release gate OPEN. The reachable records are pgmcp bug 5986,
`return-a-schema-incompatibility-error-instead-of-indexing-generated-par-fields-c781be`, and task
5989, `cover-absent-expr-oneof-defaults-replacement-and-canonical-width-boundaries-d471da`.

Source derivation found release-path indexing of generated `Par` fields by positions zero through
two. A schema reorder or omission could therefore panic rather than return an incompatibility
error. The repair introduces checked field discovery that validates each expected protobuf tag,
name, and field kind before constructing the generated match program. The focused drift test
`generated_par_program_drift_is_a_schema_error_not_an_indexing_panic` passed 1/1.

The byte cursor also gained direct resident-versus-region controls for:

- an absent Expr oneof;
- a present default `bool`;
- a present empty `EList` message;
- signed integers at -65 and 64;
- string lengths 127 and 128;
- byte-array length 16,384; and
- a deliberately conflicting `GBool` plus `GInt` oneof.

Resident and canonical-region RMP2 projections agree for every valid boundary. The conflicting
oneof is rejected before an endpoint event can complete. Existing canonicality controls continue
to reject explicitly encoded empty `locally_free`, false `connective_used`, and duplicate metadata
fields. This is a fail-closed security and consensus property: malformed or schema-incompatible
bytes may reduce pruning by causing an error, but must never admit a different semantic transcript
or trigger unchecked indexing.

### J-013 — 2026-08-23: formal transcript lemmas are a bridge, not a runtime equivalence proof

Question: What is the exact assurance boundary of the current RMP2 Rocq and Verus artifacts?

Status: DERIVED ASSURANCE BOUNDARY. Runtime-projection closure remains tracked by pgmcp task 5987,
`bridge-rmp2-formal-lemmas-to-actual-spatialmatcher-semantic-projections-c38663`.

The Rocq development proves four capstone properties over an abstract semantic record transcript.
The Verus development verifies eight obligations for framed-record correspondence, multiplicity,
coalescing, and digest input equality. The executable resident/region differentials connect both
implementations to the same test corpus. These artifacts establish useful bridge lemmas, but their
current correspondence premise assumes equality of the semantic record sequences supplied to the
model. They do not yet prove that every generated protobuf cursor branch and every resident
`SpatialMatcher` projection produces those sequences for all valid inputs.

Acceptance requires formal definitions of the actual field projection, occurrence coalescing, and
opaque-nested-`Par` rule; checked lemmas from each runtime event source into that model; and
executable extraction or proof-carrying correspondence at the source boundary. Until then, report
the formal result as transcript-level equivalence with executable runtime bridge tests, never as a
whole-runtime proof.

Checkpoint amendment — 2026-08-23: ACCEPTED SCOPED FORMAL BRIDGE; lifecycle and release gates
remain open. The Rocq development now closes seven capstone theorems over the common semantic-field
projection used by the resident and canonical-region event sources. Those theorems cover field
correspondence, adjacent-only occurrence coalescing with exact cardinality, the nonadjacent-repeat
counterexample, and opaque nested `Par` values. The Verus executable model verifies twelve
obligations with `--no-cheating`, including pending-run accumulation, final-run flushing, and the
same opacity rule. Three focused Rust gates connect the model to the actual resident and
canonical-region cursors: all maintained Expr oneofs, two- and three-run adjacency cases with
transcript/digest equality, and nested-`Par` opacity. The formal manifest also passes.

This closes the earlier assumed-record-sequence gap at the maintained semantic event boundary. It
does **not** prove arbitrary protobuf parsing, BLAKE2b correctness or collision resistance,
allocation behavior, generated-code extraction, malformed arbitrary byte streams, or equivalence
of the whole RSpace matcher and unifier. Those exclusions are explicit assurance boundaries, not
deferred premises silently treated as proved. Task 5987 therefore remains in progress until the
broader lifecycle and release gates pass.

### J-014 — 2026-08-23: do not open a datum catalog before a rendezvous can inspect it

Question: Should `produce` build the current channel's hash-ordered PathMap candidate catalog when
no waiting continuation exists?

Status: MEASURED DEFECT and ACCEPTED FOCUSED REPAIR. The reachable record is pgmcp bug 5993,
`avoid-eager-datum-catalog-hashing-when-produce-has-no-rendezvous-candidate-4e5df4`.

The first forced-collision run failed its anti-vacuity count: three identity digests had been
computed before the first consume, although only two distinct identities existed. Source tracing
explained the exact count. `extract_produce_candidate` opened `bat_channel_pool` before fetching
waiting continuations. Producing the first new identity therefore hashed the one resting identity;
producing the second distinct identity rebuilt and hashed both. None of those hashes could affect a
COMM because the continuation set was empty.

```text
Algorithm: open the produced channel's candidate pool only on demand

Input:
    one produce attempt, grouped waiting-continuation channels, and the hot store
Output:
    the same rendezvous result with no unused datum-catalog work

1. Allocate one local, empty OnceLock for the produced channel's immutable CandidatePool.
2. Fetch and canonically order waiting continuations for the current channel group.
3. If that continuation set is empty, advance to the next group before fetching any datum pool.
4. If retained pruning needs resting availability, initialize the OnceLock from the hot store.
5. If spatial matching later requests the produced channel, clone that same initialized pool and
   prepend the fresh datum to the clone.
6. Never publish the local pool as another authority; it dies with the produce attempt.
```

The repair uses a local `OnceLock<CandidatePool<A>>`, shared by retained pruning and spatial data
fetch, plus an early empty-continuation transition in the matcher driver. The successful collision
gate now observes zero identity hashes across three unmatched produces in both play and replay,
then exactly two hashes when the first consume actually needs the two-identity catalog. The same
gate preserves bindings, multiplicity, COMM events, replay, and the post-state root. Broader
matcher regression passed 49 library tests, 10 candidate-pruning integration tests, and 17 guarded
matching tests. The full-release suite remains open; no throughput improvement is claimed from one
exact work-count control.

### J-015 — 2026-08-23: collision authority and suffix roots must survive their real boundaries

Question: Do the continuation-order collision rule and CoW suffix checkpoints remain correct at
their production boundaries, rather than only under helper-level assumptions?

Status: ACCEPTED FOCUSED HARDENING; exact release workspace remains open.

The final review found two gaps. First, continuation ordering already used the correct total tuple
`(BLAKE2b-256(identity), identity bytes, store index)`, but its forced-collision test injected a
digest only into a private comparator helper. Second, suffix extraction asserted only in debug
builds that a checkpoint focus could not itself contain an endpoint. Neither gap demonstrated a
runtime failure; both left the release claim weaker than the implementation contract.

One shared, thread-local, test-only digest injector now sits at the actual datum and continuation
ordering boundaries. It compiles out of production. The continuation lifecycle gate deliberately
stores two waiting continuations so the hot store's newest-first vector order is `z-cont`, then
`a-cont`, while canonical identity order is the reverse. Its first attempt expected insertion
order and failed before the COMM; that negative result established that the anti-vacuity witness
was observing the real store. The corrected gate forces both digests equal and proves all of the
following in play and replay:

- `a-cont` is selected from vector store index one;
- `z-cont` at index zero is the sole survivor, so sorted position was not mistaken for the removal
  address;
- the COMM names `a-cont`'s exact `Consume` identity;
- both production-boundary identities are hashed exactly once; and
- the replay map and non-empty post-state root equal play.

Checkpoint extraction now snapshots a focus value before `ZipperSubtries::make_map`, restores it
at the suffix root, and refuses only a genuinely empty synchronized side. This is preferable to a
false-negative fallback, a release panic, or the depth-growing legacy zipper clone. A direct
root-plus-child control proves both endpoints survive. A separate counter proves canonical RhoPath
component branches take the no-restoration fast path, so the defensive correctness rule does not
change the measured normal shape. The capped release library gate passed 58 of 58 tests, including
both forced-collision lifecycles, eleven focused checkpoint controls, and the separate depth-16,384
small-stack gate. The byte-native datum catalog/read API also drops the obsolete `Serialize` and
`CandidateOrderingBytes` requirements left by the decoded-hash route; a decode-only generic compile
witness prevents their accidental return. No PathMap source or feature was changed.

### J-016 — 2026-08-23: preserve admitted regions and CoW roots across reducer boundaries

Question: Why did the exact depth-131,072 deploy gate still exceed its watchdog after RMP2's
structural PathMap query became byte-native, and which PathMap properties should cross the two
remaining abstraction boundaries?

Status: MEASURED DEFECT, ACCEPTED REPAIR, and EXACT WORKSPACE ACCEPTANCE. The implementation is
`f1r3node-rust-mettail@1afd4791e`; the reachable records are pgmcp bugs 5994 and 5995, experiment
311, and artifacts 380–381. The durable resource record is
[`resident-send-reducer-join-2026-08-23.md`](../stack-safety/measurements/resident-send-reducer-join-2026-08-23.md).

The first diagnosis rejected the tempting hypothesis that PathMap lookup or final RSpace insertion
was responsible. At depth 8,192, post-RMP2 structural projection took 590,939 ns and final insertion
contributed about 12,976 allocation calls. Headless Heaptrack instead attributed 52,712,929 of
53,716,424 whole-process allocation calls to the generated nested `Send` decoder: each successively
shorter child `Par` suffix was attached to a new `ParBag`, which repeatedly rebuilt PathMap payloads
before evaluation began. The canonical top-level `Par` importer could not help because nested
`Send` children did not cross that boundary.

The selected projection retains PathMap's authoritative byte representation. It derives the
optional-channel and repeated-data roles from `SEND_RHO_MATCH_PROGRAM`, applies the existing
stack-safe `cut_at_par` region PDA to the admitted RPB3 endpoint, decodes only the compact scalar
shell, and decodes each immediate child exactly once through the established top-level importer.
It does not enumerate the ParBag, manufacture a `Vec<Par>`, sort an occurrence list, or introduce a
shadow map. Decreasing fields, duplicate singular fields, and schema near misses take the complete
generated decoder, so the optimization remains a proof-carrying fast path rather than a new wire
authority.

That repair made `plain_deploy` linear but exposed an independent `env_get_deploy` slope. A second
headless profile attributed 52,639,873 of 53,747,932 allocation calls to
`computed_locally_free_par -> generated Expr decoder -> attach_par_exprs -> ParBag insertion`.
`descend_eval` had correctly returned the environment binding as a resident CoW `Par`, but erased
the stronger fact that canonical-path-stable ground data has an empty structural locally-free
fold. Its parent `EvKont::Join` then semantically decoded the complete recursive value merely to
reprove that empty summary, and subsequently imported the same value into `ParConstruction`.

The final design makes the proof explicit. `StableGround` certifies evaluator identity *and* an
empty structural fold; `ExpressionFree` certifies only evaluator identity for reflected process
families. Only the former receives the empty-fold certificate. At a one-child `Join` whose
expression-free shell has the additive-zero component Bag, the evaluator moves the resident child
root and applies the existing `Par::parallel` metadata equations directly:

```math
B_{\mathrm{out}} = B_{\mathrm{child}} \oplus \varnothing = B_{\mathrm{child}},
```

```math
L_{\mathrm{out}} = L_{\mathrm{child}} \mathbin{\mathrm{union}} L_{\mathrm{shell}},
\qquad
C_{\mathrm{out}} = C_{\mathrm{child}} \lor C_{\mathrm{shell}}.
```

Here `B` is the authoritative `PathMap<Multiplicity>` component Bag, `L` is the wire-visible
locally-free byte vector, and `C` is `connective_used`. The historical child-then-shell `union` is
retained exactly, including trailing zero bytes; the separately carried structural fold remains
the child's proof because an empty component shell contributes no structural bits. Every near miss
uses the complete construction PDA.

MEASURED results:

- `plain_deploy` control time was 2.91, 10.79, and 44.21 seconds at depths 8,192, 16,384, and
  32,768; the region projection measured 0.06, 0.13, and 0.26 seconds and reached depth 131,072 in
  1.06 seconds;
- `env_get_deploy` before the neutral Join measured 2.59 and 9.94 seconds at depths 8,192 and
  16,384 and exceeded 146.9 seconds at depth 131,072; after the repair it measured 0.13, 0.25,
  0.48, and 2.03 seconds through depth 131,072;
- the post-Join depth-8,192 profile records 963,802 allocation calls, a 98.21% reduction from the
  immediate predecessor, and no dominant full semantic decoder/import stack;
- the unchanged 2 MiB-worker gate accepts both deploy subjects at depth 131,072 in 3.14 seconds;
  and
- the exact release workspace run passes 4,495 of 4,495 executed tests, with 53 declared skips and
  zero failures.

The tests preserve optional-field presence, repeated-child order, unknown metadata, malformed
EPathMap refusal, fallback reachability, exact protobuf and bincode values, recursive-oracle result
and charge traces, and the original PathMap storage root. No PathMap source or feature changed.

The timing ladders remain PRELIMINARY one-observation attribution evidence. Experiment 311's
registered 30-sample statistical magnitude decision remains pending and must not be inferred from
these rows. That pending magnitude estimate does not reopen the repaired correctness/resource gate.

## Open hypotheses and required closure evidence

| ID | Hypothesis | Required evidence before acceptance | Status |
|---|---|---|---|
| H-001 | Direct canonical protobuf-region RMP2 projection changes the deep unary-list structural path from quadratic to linear time. | Generated-schema differential corpus; resident/region transcript equality; malformed-region refusal; geometric timing/allocation ladder; exact candidate and COMM equivalence. | **Accepted for mechanism and gates at `1afd4791e`**; experiment 311's n=30 magnitude verdict remains pending |
| H-002 | A narrow substitution identity certificate plus the additive-zero resident Join removes redundant traversal for already-stable ground send data. | Byte, score, accounting, event, RSpace identity, near-miss, CoW-root, and zero-work controls. | **Accepted at `1afd4791e`**; 98.21% fewer profiled allocation calls and exact recursive/charge differentials |
| H-003 | After H-001 and H-002, the exact depth-131,072 deploy completes under the unchanged 2 MiB worker stack and 120-second watchdog. | Exact release depth gate under capped RSS and zero swap. | **Accepted**; both subjects reach 131,072 and the exact release workspace passes 4,495/4,495 |
| H-004 | Datum and continuation candidate order is independent of derived RMP bytes and digest collisions in encoded, decoded, play, and replay routes. | Opposing-RMP and forced-hash-collision controls; duplicate-identity refusal; multiplicity preservation; original-address removal; encoded/decoded key identity; play/replay COMM and post-state equivalence. | Both focused end-to-end collision/replay gates passed; release open |
| H-005 | Lazy nested-Par summarization changes the dynamic deep-unary query route from repeated suffix hashing to one selected-token hash. | Shell-classifier differential; malformed-shell refusal; exact summary-count probe; geometric time/allocation ladder; emitted-token and candidate equivalence. | Open |
| H-006 | A clone-family-shared `OnceLock<[u8; 32]>` removes repeated resident RMP2 transcript walks with fixed retained space and no material cold regression. | Exact bytes/oracle; one initialization per clone family; mutation/no-op/metadata/concurrency matrix; zero warm endpoint visits; preregistered cold/warm/clone/post-mutation time, allocation, instructions, and headless Heaptrack RSS gates from pgmcp 5983/5984. | Resource and focused semantic hypotheses accepted; COMM/replay/full-release lifecycle gates open |
| H-007 | Retained `CandidateProductCursor` clones copy depth-growing zipper path/ancestor state and, if that slope is confirmed, a bounded CoW suffix checkpoint removes it without changing candidate or binding semantics. | Clone/allocation anti-vacuity; geometric depth/width resource ladder; old/new candidate and binding differential; focus-endpoint preservation; malformed, COMM, replay/post-state, and small-stack gates from pgmcp 5985 and experiment 5990. | Deterministic-run hybrid passed semantic, focus-root, paired resource, allocation, instruction, and 256 KiB small-stack gates; full release open |
| H-008 | Semantic projection or a fail-closed `All` fallback restores dynamic AC candidate-query soundness for matcher-ignored `New`, `ReceiveBind`, and `MatchCase` fields. | Per-field counterexamples; oracle-success-implies-query-accept property; ground exact, malformed, candidate/binding, COMM, play/replay, and post-state gates from pgmcp 5982. | Focused semantic gates passed; COMM/replay/release open |

## Threats to validity

- The J-004 ladder uses one observation per size to attribute phases, not to make a throughput
  claim. Treatment acceptance requires a preregistered repeated-sample experiment.
- The Bag insertion arm repeats key preparation internally. Its near-equality with the isolated key
  arm establishes that insertion is not the observed multi-second phase; it does not establish a
  nanosecond insertion bound.
- CPU pinning prevents migration but not frequency variation. Ratios near four across three
  doublings are sufficiently separated from the linear hypothesis for root-cause triage, while
  final performance claims will use paired controls.
- The current evidence covers a deep unary EList carrier. The repair may not specialize to that
  shape; it must be generated-schema complete and differentially verified across every recursive
  field kind.
- J-008's four focused invocations share an uncommitted implementation coordinate. They establish
  the corrected writer-image invariant and its focused resource envelope, but they do not replace
  the pending immutable commit, malformed corpus, replay, or full-release evidence.
- J-009's full matrix, powered replication, headless Heaptrack, and Cachegrind profiles pass the
  preregistered speedup, cold-regression, warm-zero-allocation, non-occurrence-expanded, constant
  warm-instruction/data-access, and fixed-retained-storage gates. The first 30-pair tiny negative
  remains preserved. The remaining COMM/replay/full-release checks are lifecycle equivalence gates,
  not missing resource evidence.
- J-010's quadratic sum is conditional on an adversarial query reaching the retained-clone sites at
  geometrically increasing depths. The v1 counter ladder established and removed that mechanism,
  while the v2 paired matrix, allocation profiles, and instruction ladder accept the revised
  deterministic-run hybrid. Cross-run absolute v1/v2 latency comparisons remain secondary because
  the v2 command incurred major page faults; paired comparisons within each run are primary.
- J-011's constructor-specific counterexamples and oracle-superset gate passed at one uncommitted
  implementation coordinate. They establish focused query inclusion, not concurrent COMM order,
  replay/post-state equivalence, an immutable commit, or the full release result.
- J-012's boundary corpus is deliberately selected, not exhaustive over every protobuf wire image.
  Generated-schema enumeration, malformed fuzzing, and the release suite remain necessary.
- J-013's scoped bridge now reaches the maintained resident and canonical-region semantic event
  sources. It still does not establish parser, cryptographic, allocation, arbitrary-malformed-byte,
  generated-code-extraction, or whole-runtime matcher correctness; extending the claim beyond that
  boundary would remain unsound.
- J-014's zero-hash result is an exact work counter for one unmatched-produce shape, not a
  throughput distribution. Retained-pruning, matched-produce, and concurrent workloads remain in
  the broader resource gates.

## Update rules

1. Append a dated entry whenever a PathMap operation, representation, compact format, cursor,
   algebra, lattice operation, CoW behavior, or persistence strategy affects a technical decision.
2. Store raw measurements under `docs/design/stack-safety/measurements/` and link them from the
   entry.
3. Mark conclusions as DERIVED, MEASURED, HYPOTHESIS, or DECISION.
4. Preserve negative results and superseded interpretations.
5. Link consensus-visible byte changes to the consensus register and stack/RSS results to the
   stack-safety report.
6. Update pgmcp progress, experiments, and reachable child tasks in the same stable checkpoint.
