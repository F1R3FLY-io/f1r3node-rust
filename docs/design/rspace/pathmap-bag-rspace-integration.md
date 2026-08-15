# PathMap Bag and RSpace matching integration

Status: approved implementation design and living engineering report

Campaign: `campaign-4131-rspace-pathmap-integration`

Experiment: `rspace-pathmap-end-to-end-layout-2026-08-15`
Last updated: 2026-08-15

## Purpose and scope

This report specifies how RSpace can use a prefix-compressed `PathMap` bag without changing the observable contracts of Rholang communication, replay, persistence, or consensus. It is the implementation design for the `f1r3node-rust-mettail` campaign. The separate `reified-rspaces-mettatron-migration` epic consumes the interfaces produced here; it owns public Rholang Bag syntax and the later MeTTaTron-to-Rholang bridge.

The work has three separable layers:

| Layer | Authoritative representation | Responsibility |
|---|---|---|
| Bag algebra | `PathMap<Multiplicity>` | Unique byte paths, multiplicities, prefix compression, lattice operations, copy-on-write forks, and optional structural sharing |
| RSpace index | RhoPathV1 keys in one or more Bags | Candidate discovery, retained pattern indexes, deterministic reservation, and reversible projection to existing RSpace records |
| Matcher | Explicit pushdown automata over PathMap zippers | Directional spatial matching, ordered matching, associative-commutative matching, guard backtracking, and replay-equivalent selection |

Neither a `Vec<Par>` nor a hash table is an authoritative substitute for a trie layer. Compatibility vectors may be materialized only at legacy API boundaries, and their cost must be measured and removed from hot paths.

## Bag algebra

A multiplicity is a strictly positive integer bounded by `u64`:

```math
\operatorname{Multiplicity} = \{n \in \mathbb{N} \mid 1 \le n \le 2^{64}-1\}.
```

A bag over byte paths is a finite-support function `B`:

```math
B : \{0,\ldots,255\}^{*} \rightarrow \mathbb{N}.
```

A missing PathMap value denotes multiplicity zero; zero is never stored. The operations are intentionally distinct:

| Operation | Pointwise multiplicity | Algebraic role |
|---|---:|---|
| Sum, `B.sum(C)` | `B(p) + C(p)` | Commutative-monoid addition; detects `u64` overflow |
| Join, `B.join(C)` | `max(B(p), C(p))` | Idempotent lattice union |
| Meet, `B.meet(C)` | `min(B(p), C(p))` | Idempotent lattice intersection |
| Difference, `B.subtract(C)` | `max(B(p) - C(p), 0)` | Truncated multiset subtraction |

Treating additive sum as lattice join is rejected because `B + B != B` for a nonempty bag, violating lattice idempotence. This design consequently does not copy MeTTaTron's current additive `Lattice::pjoin` spelling.

The Bag caches distinct and total cardinality as derived scalars while `PathMap<Multiplicity>` remains the sole authoritative membership representation:

```math
\operatorname{distinct}(B)=|\operatorname{keys}(B)|,
\qquad
\operatorname{cardinality}(B)=\sum_{p\in\operatorname{keys}(B)} B(p).
```

## RhoPathV1 key model

RSpace needs structural prefixes for trie-pruned matching and a reversible identity for the complete stored record. RhoPathV1 therefore has two framed regions:

```text
RhoPathV1 := version || structural-length || structural-path || identity-bytes
```

The structural region is compiled from the existing canonical Rholang PathMap grammar. The identity region is the exact canonical record encoding required to reconstruct a `Datum`, including payload, persistence, and source. Pattern automata operate only on the structural region; exact membership and decoding use the complete key.

Identical records share one trie value whose `Multiplicity` counts occurrences. The current store-index tie breaker does not distinguish identical record bytes semantically: choosing and removing any occurrence leaves the same record and decremented multiplicity. Unequal candidates retain the existing ordering by the Blake2b-256 digest of canonical candidate bytes. Differential play/replay tests must prove this quotienting argument before the vector store is retired.

## Matching architecture

The optimized path is staged and directional:

1. Compile a `BindPattern` into a retained set automaton whose transitions consume RhoPathV1 structural bytes.
2. Product-walk that automaton with a Bag read zipper, pruning nonmatching prefixes before decoding complete records.
3. Use the current Rholang spatial semantics as the acceptance oracle while the byte automaton is established.
4. Enumerate multi-channel reservations with an explicit pushdown automaton whose frames contain zipper positions, residual multiplicities, bindings, and rollback state.
5. Evaluate the continuation guard only at complete leaves and backtrack iteratively after rejection.
6. Commit the least admissible reservation under the existing consensus order.

Production matching is directional: receive patterns match produced data. Symmetric Martelli-Montanari unification is an experiment for contexts that genuinely require variables on both sides; it is not silently substituted for receive semantics. Ordered children use linear PDA traversal. Associative-commutative collections use multiplicity-aware residual Bags rather than expanded vectors.

## Native PathMap operations

| Need | Native mechanism |
|---|---|
| Exact multiplicity | `get` or a zipper descended to the complete key |
| Increment/decrement | One write zipper; remove and prune at zero |
| Prefix discovery | Read zipper rooted at the structural prefix |
| Join, meet, difference | PathMap algebra with lawful multiplicity values |
| Copy-on-write snapshots | `PathMap::clone` structural sharing |
| Compact snapshots | `ArenaCompactTree` produced directly from a read zipper |
| Repeated-subtrie sharing | `merkleize`, only after end-to-end evidence justifies its construction cost |

No production operation may flatten the trie merely to perform an operation already supplied by PathMap. A decoded-object cache, if later justified experimentally, is derived and invalidatable rather than a second authoritative store.

## Consensus and compatibility invariants

- Candidate order remains a total order derived from canonical candidate bytes and existing tie semantics.
- Play and replay select the same COMM and leave byte-identical post-state roots.
- Persistent produces and consumes preserve reuse semantics.
- Peek reservations observe data without consuming multiplicity.
- Multi-channel joins reserve atomically and roll back without leaking decrements.
- Cold-store and event-hash bytes remain unchanged until a separately versioned migration is approved.
- Malformed bytes fail locally and deterministically without panics, partial mutation, native recursion, or unbounded allocation relative to input.
- There is no artificial term-depth, receive-arity, expression-width, or traversal-depth limit.
- Clone and Drop remain bounded in native-stack use for adversarially deep paths and terms.

The old and new implementations remain simultaneously executable in differential tests until these invariants are proved. The old implementation may remain only as a test oracle after the production switch.

## Scientific evaluation

The preregistered primary metric is completed COMM operations per second. The hypothesis requires at least a five-percent improvement at ninety-nine-percent confidence over baseline commit `dcc4600d4`, with every correctness gate passing and no critical latency or memory metric regressing by more than five percent.

The workload matrix crosses distinct cardinality, duplicate multiplicity, shared-prefix density, term depth, receive arity, join overlap, match density, guard rejection, backtracking width, persistence, peek, replay, and associative-commutative patterns. Secondary observations include p50, p95, and p99 latency; allocations per COMM; peak resident memory; cache and branch misses; snapshot size; and construction cost.

Benchmarks run only after a preflight confirms enough idle physical cores and no competing compiler or benchmark process. Each run follows the pgmcp-prescribed warm-up, sample count, CPU affinity, hardware record, and statistical test.

## Verification ladder

| Level | Evidence |
|---|---|
| Algebra | Property tests for identity, commutativity, associativity, idempotence, absorption, subtraction, and multiplicity conservation |
| Codec | Canonical round trips, malformed-frame rejection, tiny-stack deep inputs, and generated-versus-oracle differential tests |
| Matcher | Bounded exhaustive comparison of vector and PathMap candidate sets, bindings, selection order, and residual multiplicities |
| Replay | Play/replay trace equivalence and identical post-state roots over generated schedules |
| Model | TLA+/Apalache or Maude exploration of reservation, rollback, persistence, and multiplicity conservation |
| Rust | Verus proofs for arithmetic and PDA invariants where supported; SMT obligations otherwise |
| Robustness | Mutation tests demonstrating that equivalence and non-vacuity gates reject representative faults |

The semantic reservation procedure is:

```text
reserve(key, persistent, peek):
    require bag.count(key) > 0
    if not persistent and not peek:
        bag.decrement(key)
        rollback.push(key)
    return candidate(key)

rollback_all():
    while rollback is not empty:
        bag.increment(rollback.pop())
```

Procedure notation describes semantics only. Nested matching and multi-channel search are implemented by loop-driven state machines rather than native recursion.

## Migration and completion

The migration order is lawful Bag, RhoPathV1 codec, shadow index, retained automaton, reservation PDA, replay and cold-store integration, and removal of the vector hot path. Set, Map, and merkleization experiments are independent treatments and cannot be promoted merely because the Bag treatment succeeds.

The campaign completes only when the selected implementation is active end to end, old-path references are removed or test-only, every reachable pgmcp task has trusted evidence, the stack-safety report contains final profiles, the consensus register records the exact wire and state disposition, and the release workspace suite passes under a bounded resident-memory scope without `RUST_MIN_STACK`, `stacker`, or traversal caps.
