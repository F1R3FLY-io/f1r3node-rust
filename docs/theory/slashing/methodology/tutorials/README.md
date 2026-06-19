# Tutorials — index

This directory contains **per-tool tutorials** for adding new
search targets to the slashing methodology. Each tutorial walks
through the prerequisites, the skeleton, an example from this
repository, and the verification step.

The chapters here are *how-to* guides; the *why-to* and the
*epistemics* live in the framework chapters
([`../formal-methods/`](../formal-methods/),
[`../randomized-search/`](../randomized-search/),
[`../differential-and-metamorphic/`](../differential-and-metamorphic/)).

## Index

| #  | Tutorial                                                                           | When to use                                           |
|----|------------------------------------------------------------------------------------|-------------------------------------------------------|
| 01 | [Write a new proptest property](./01-write-new-proptest.md)                        | Single-step properties; sub-second feedback           |
| 02 | [Write a new TLA⁺ spec + TLC model](./02-write-new-tla-spec.md)                    | Bounded finite-state exhaustion                       |
| 03 | [Write a new libFuzzer / cargo-fuzz target](./03-write-new-fuzz-target.md)         | Byte-level / structure-aware coverage-guided search   |
| 04 | [Write a new Kani harness](./04-write-new-kani-harness.md)                         | Per-function exhaustion on bounded primitives         |
| 05 | [Write a new Sage model](./05-write-new-sage-model.md)                             | Exact finite enumeration with combinatorial structure |
| 06 | [Write a new Hypothesis state machine](./06-write-new-hypothesis-state-machine.md) | Multi-step lifecycle traces with shrinking            |
| 07 | [Write a new Loom test](./07-write-new-loom-test.md)                               | Concurrency interleaving permutation search           |
| 08 | [Write a new Rocq theorem](./08-write-new-rocq-theorem.md)                         | Unbounded-domain proofs of load-bearing properties    |

## How to choose

See the decision tree in
[`../01-philosophy.md §3.2`](../01-philosophy.md#32--picking-the-tool--the-decision-tree)
for the methodology's guidance on which tool to reach for first.
