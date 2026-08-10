---
pgmcp_experiment: d-e4-lazy-relational-ac-edge-reuse-2026-08-09
experiment_id: 172
title: D-E4 lazy relational AC edge reuse
date: 2026-08-10
project: f1r3node-rust-mettail
kind: optimization
status: decided
structured_verdict: inconclusive
protocol_verdict: exact-criterion-satisfied
implementation_ref: f1r3node-rust-mettail@3a3db311690e4bc300b69514be1d2a2eb6773359
validation_ref: f1r3node-rust-mettail@0c5e297a3ba6118d614b75bb340b6c0b05703e01
proof_ref: mettail-rust@b6095533
plan: pgmcp work item d-e4-relational-ac-matching-for-par-multisets-bff1f9b3
---

# D-E4 lazy relational AC edge reuse

## Question and source correction

Can the production spatial matcher retain lazily discovered pattern/target edges across augmenting
paths, eliminating repeated structural associative-commutative (AC) match work without changing a
match, binding, carrier, traversal order, or atomic consume?

The original pgmcp question named `MaximumBipartiteMatch` as the production path. Source
re-derivation corrected that premise: production uses
`spatial_matcher_pda::ListMachine`. `MaximumBipartiteMatch` is a compatibility surface
that now shares the same `LazyRelation` utility. `sub_pars` remains the producer of
connective remainder candidates.

## Frozen protocol

The treatment stores successful edges for the evaluated prefix of each cacheable row and evaluates
only its unseen suffix on reuse. Failed pairs are summarized by the row frontier. The primary metric
is the exact `match_function`/production edge-evaluation count from the same binary and graph.
When all replicates have zero variance, the preregistered protocol decides by exact inequality rather
than a Welch statistic. The diagonal invariant must retain the exact assignment and exact call count
with zero cache reuse.

```text
pattern row
   ├── replay successful edges from the frozen cursor prefix
   └── evaluate each target in the unseen suffix at most once
          ├── failure: advance frontier only
          └── success: retain edge and binding delta
```

Retained relation storage is $`O(P+E)`$, where $`P`$ is the pattern count and
$`E`$ is the number of successful cacheable edges. It is not a dense
$`P \times T`$ table.

## Measurements

| Arm | Replicates | Exact calls per replicate | Assignment |
|---|---:|---:|---|
| nominal no-cache displacement schedule | 51 | 699,263 | reject |
| production sparse relation | 51 | 16,384 | reject |
| diagonal nominal control | 1 | 8,256 | accept |
| diagonal sparse relation | 1 | 8,256 | identical accept |

The treatment performs 682,879 fewer evaluations per displacement replicate, a
42.67962646484375× reduction. All control samples are the same constant and all treatment samples are
the same constant. Consequently both sample variances are zero and the generic Welch implementation
cannot produce a statistic or confidence interval. Pgmcp records its generic structured verdict as
`inconclusive`; the frozen protocol's exact-inequality criterion is nevertheless satisfied
without estimating or inventing variance.

The finalized sample digests are:

- control: `sha256:0d55a62b0d2a0a0e799a9f620b7635389ea34e3ec25aad2afdf27c5f9d916442`
- treatment: `sha256:7e988f7e5eee28a48cf60e0cbcdda12a996c4ea8007a33e2298e8e6de9cb52fe`

The durable tab-separated record is
[`relational-ac-edge-reuse-2026-08-10.tsv`](../design/stack-safety/measurements/relational-ac-edge-reuse-2026-08-10.tsv).
Raw samples remain in pgmcp experiment 172.

## Semantic and formal controls

The shared relation utility is compared with the nominal assignment schedule over every graph
through four patterns/four targets and 512 generated graphs through eight/eight. Production tests
cover permutations, duplicate values, nonlinear binders, pre-existing `FreeMap` bindings,
displacement, diagonal identity, and the historical impure `FnMut` schedule. The existing
Stage-AC carrier, shuffle, corrupted-report, atomicity, and bounded recursive-oracle suites remain
green.

The admission-free Rocq theory `LazyRelationalAcEdges.v` proves:

1. cached-prefix plus unseen-suffix rows equal a full scan;
2. membership is identical;
3. a duplicate-free prefix/suffix partition evaluates no pair twice;
4. every valid injective assignment is preserved; and
5. unseen-suffix length is a well-founded decreasing work measure.

All five `Print Assumptions` audits are closed under the global context.

## Resource and integration gates

The focused three-test production run, including all 102 deterministic primary samples, completed in
57.80 seconds; its systemd scope reported approximately 1.2 GiB peak memory. The final capped
`cargo test -p rholang` run passed 311 library tests, the 545-test aggregate integration
target with one intentional ignore, every standalone integration target, the stack-depth gate, and
documentation tests. That complete parallel gate reached its 12 GiB cgroup ceiling during
compilation and deliberate tripwire child runs; it is not a matcher-RSS measurement.

Every command used the ordinary Rust stack, `MemorySwapMax=0`, and no
`RUST_MIN_STACK`, `stacker`, artificial depth limit, PathMap fork, trie snapshot, or
collection projection.

## Reproduction

Run from the indicated repository, preserving parallel Cargo defaults:

```sh
# f1r3node-rust-mettail
systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 \
  cargo test -p rholang --lib production_list_pda -- --nocapture

systemd-run --user --scope -p MemoryMax=12G -p MemorySwapMax=0 \
  cargo test -p rholang

# mettail-rust
systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 \
  make -C formal check-capped \
    FORMAL_CAPPED_TARGET=rocq-advanced-automata \
    FORMAL_MEMORY_MAX_BYTES=2147483648 \
    FORMAL_MEMORY_HIGH_BYTES=1610612736 \
    FORMAL_TASKS_MAX=16384
```

`FORMAL_TASKS_MAX` is a nonbinding process-count guard required by the repository harness; it is
not a term-depth or traversal limit.

## Decision

Accept the sparse prefix-plus-delta relation as an exact, byte-neutral production optimization.
Consensus classification is `BYTE_NEUTRAL_MEASURED` within the equivalence-proven CBR-023
set. The implementation changes no accepted program, match verdict, binding, protobuf or bincode
byte, post-state/event hash, COMM schedule, charge, EPathMap mode, EPM1 snapshot, PathMap topology,
or zipper/algebra/lattice result.

---

This workspace-side ledger is rendered from and reconciled with pgmcp experiment 172. The pgmcp
structured record remains the source of truth for preregistration, samples, digests, and its generic
statistical-engine verdict.
