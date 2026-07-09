# Slashing — Bug-Hunting & Property-Discovery Methodology

> *“What is now proved was once only imagin'd.”* — William Blake,
> *The Marriage of Heaven and Hell*, 1790.

This document set is the **pedagogical companion to the slashing
verification effort**. It does not describe *what* the slashing
subsystem is (see [`../design/`](../design/) for that), nor *which*
properties were proven (see [`../slashing-verification.md`](../slashing-verification.md)
for that), nor *what* attacks were defended against (see
[`../slashing-threat-model.md`](../slashing-threat-model.md) for that).
It describes **how the bugs, security vulnerabilities, and interesting
properties were *found***, and **how a reader can apply the same
methodology to a new component, a new property, or a new threat
class**.

Eleven bugs in the inherited Scala source, five regressions introduced
during the Rust port, sixteen normative theorems, and a closed
bisimilarity claim between the Rust and Scala implementations did not
arrive by intuition alone. They are the output of a deliberate,
multi-method search program whose layers are described below.

---

## 1 · How to read this directory

| Layer            | If you are …                                                         | Start here                                                                                                                                                                                         |
|------------------|----------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **Foundations**  | a curious engineer who wants the *why* before the *how*              | [`01-philosophy.md`](./01-philosophy.md) → [`02-glossary-and-notation.md`](./02-glossary-and-notation.md)                                                                                          |
| **Techniques**   | an engineer evaluating which method fits a new problem               | [`formal-methods/`](./formal-methods/), [`randomized-search/`](./randomized-search/), [`differential-and-metamorphic/`](./differential-and-metamorphic/), [`attack-modeling/`](./attack-modeling/) |
| **Pipeline**     | an auditor verifying that every witness becomes a classified outcome | [`pipeline/`](./pipeline/)                                                                                                                                                                         |
| **Models**       | a researcher reading the Sage corpus                                 | [`sage-models/`](./sage-models/)                                                                                                                                                                   |
| **Case studies** | an engineer who wants concrete examples of methodology in action     | [`case-studies/`](./case-studies/) — one chapter per discovered bug                                                                                                                                |
| **How-to**       | a contributor extending the search program                           | [`tutorials/`](./tutorials/), [`extending.md`](./extending.md)                                                                                                                                     |
| **References**   | a researcher chasing citations                                       | [`references.md`](./references.md), and `../design/13-references.md` for the full upstream bibliography                                                                                            |
| **Diagrams**     | a visual reader                                                      | [`diagrams/`](./diagrams/) — methodology stack, witness→promotion pipeline, scientific-method loop, tool↔theorem coverage                                                                          |

The full index is in [§4 — File map](#4--file-map) below.

---

## 2 · Why a methodology document?

A reader can extract the **theorems** from
`slashing-verification.md`, the **threats** from
`slashing-threat-model.md`, the **command runbook** from
`slashing-search-horizon.md`, the **traceability ledger** from
`slashing-traceability.md`, and the **architecture** from
`design/`. None of those documents answer the *next* question an
engineer asks once they have finished reading:

> *“If a new component arrives — say, a graduated-slashing penalty
> module, or a checkpoint protocol — how do I know I have done a
> thorough job of looking for bugs in it?”*

The slashing effort answered that question pragmatically across nine
techniques, three artifact languages (Rocq, TLA⁺, Sage), six adversary
classes, and a witness-to-source promotion pipeline that converts
machine-generated counterexamples into either source fixes,
specification strengthening, model boundaries, or assumption
counterexamples — with deliberate rules about when *not* to act on
a finding. This directory writes that program down in a form that
generalizes.

The contract is informal but firm:

> *Every load-bearing claim in the slashing specification is backed by
> evidence from at least one mechanized technique (Rocq or TLA⁺) and
> at least one randomized technique (proptest, Hypothesis, libFuzzer,
> Loom, or Sage exhaustive search). No single technique is the proof.*

This is **evidence stacking** — see
[`pipeline/03-evidence-stacking.md`](./pipeline/03-evidence-stacking.md).

---

## 3 · The methodology stack

Nine techniques are deployed against the slashing subsystem. Each has
distinct epistemic strength, a distinct cost model, and a distinct
class of bug it is most likely to find. The decision tree for *which*
technique to apply to a *given* candidate property lives in
[`02-glossary-and-notation.md §1`](./02-glossary-and-notation.md). The
techniques themselves are documented in detail in the chapters listed
below.

| # | Technique                                    | Epistemic strength                                                | Cost        | Best at finding                                                                    |
|---|----------------------------------------------|-------------------------------------------------------------------|-------------|------------------------------------------------------------------------------------|
| ① | **Rocq mechanized proof**                    | Strongest: kernel-checked, unbounded-state, semantic              | Very high   | Mathematical errors; missing preconditions; latent bisimilarity gaps               |
| ② | **TLA⁺ + TLC (model checking)**              | Exhaustive on finite bounds; complete on the bounded instance     | Medium      | Concurrency races; reorderings; liveness; abstraction-level message-passing errors |
| ③ | **TLA⁺ + Apalache (symbolic)**               | SMT-backed bounded; finds counterexamples TLC cannot enumerate    | Medium      | Wider validator/epoch domains; numerical envelopes                                 |
| ④ | **Kani (symbolic Rust)**                     | Bounded model checking of *real* Rust source                      | Medium      | Overflow, panic-on-invalid-input, predicate refactor regressions                   |
| ⑤ | **Sage (exact finite modeling)**             | Exact integer / graph / combinatorial enumeration                 | Low to high | Quorum bounds, closure shapes, stake-weighted edge cases                           |
| ⑥ | **Hypothesis (stateful, shrinking)**         | Generates and *minimizes* multi-step lifecycle traces             | Low         | DAG topologies, multi-epoch behaviors, attack campaigns                            |
| ⑦ | **proptest (property-based)**                | Randomized in Rust against the production-shaped harness          | Low         | Monotonicity, idempotence, soundness, completeness on small inputs                 |
| ⑧ | **cargo-fuzz / libFuzzer (coverage-guided)** | Bit-level mutation guided by edge coverage                        | Low         | Proto parsers, arithmetic boundaries, structure-aware authorization paths          |
| ⑨ | **Loom (concurrency interleaving)**          | Exhausts permitted memory-model orderings under a thread schedule | Low         | Lost-update races; missing happens-before; atomicity violations                    |

A tenth technique — **system adversarial testing** at the multi-node
level — is documented in the architecture spec but is out of scope
for this methodology directory; see
[`../slashing-search-horizon.md §2`](../slashing-search-horizon.md).

Diagram 01 in [`diagrams/`](./diagrams/01-methodology-stack.svg) shows
the layer stack and its coverage of the verification hierarchy. The
authoritative artifact-to-theorem map is Diagram 04 in the same
directory.

---

## 4 · File map

```
methodology/
├── README.md                                    ← this file
├── 01-philosophy.md                             ← scientific method, witness rule
├── 02-glossary-and-notation.md                  ← every symbol/acronym/term
├── formal-methods/
│   ├── 01-mechanized-proof-rocq.md              ← Rocq, dependent types, why kernel-checked
│   ├── 02-model-checking-tla.md                 ← TLA⁺, TLC, Apalache
│   ├── 03-symbolic-rust-kani.md                 ← bounded model checking of real Rust
│   └── 04-finite-modeling-sage.md               ← exact integers, graphs, combinatorics
├── randomized-search/
│   ├── 01-property-testing-proptest.md          ← proptest in Rust
│   ├── 02-stateful-hypothesis.md                ← Hypothesis state-machine search
│   ├── 03-coverage-guided-fuzzing.md            ← cargo-fuzz / libFuzzer
│   └── 04-concurrency-interleaving-loom.md      ← loom permutation search
├── differential-and-metamorphic/
│   ├── 01-differential-rust-vs-scala.md         ← cross-implementation oracle
│   ├── 02-metamorphic-relations.md              ← permutation, idempotence, monotonicity
│   └── 03-triple-bisimilarity.md                ← Rocq oracle ↔ harness ↔ production
├── attack-modeling/
│   ├── 01-stride-and-attack-trees.md            ← threat-modeling pedagogy
│   ├── 02-adversarial-search.md                 ← damage optimizer, deep-threat models
│   └── 03-economic-game-theoretic.md            ← rational adversary, bribery, censorship
├── pipeline/
│   ├── 01-witness-to-source-rule.md             ← witness → traceability → action
│   ├── 02-classification-taxonomy.md            ← 8 statuses, decision tree
│   └── 03-evidence-stacking.md                  ← how techniques compose / triangulate
├── sage-models/                                 ← per-family chapters (closure, adversarial, …)
├── case-studies/                                ← one chapter per discovered bug (#1–#16)
├── tutorials/                                   ← per-tool how-to
├── extending.md                                 ← adding a new property/target/spec
├── references.md                                ← verified DOIs for methodology citations
└── diagrams/
    ├── 01-methodology-stack.{puml,svg}          ← layer stack of techniques
    ├── 02-witness-to-promotion.{puml,svg}       ← sequence: model → witness → action
    ├── 03-scientific-method-loop.{puml,svg}     ← activity: hypothesis → experiment
    ├── 04-tool-theorem-coverage.{puml,svg}      ← which tools cover which theorems
    ├── 05-classification-decision-tree.{puml,svg} ← decision tree for the 8 statuses
    └── 06-differential-divergence-classification.{puml,svg} ← Rust↔Scala δ classification
```

---

## 5 · Relationship to the existing documentation

The methodology directory complements but does not replace:

| Document                                                                                                 | Role                                             | Relationship to `methodology/`                                                                                     |
|----------------------------------------------------------------------------------------------------------|--------------------------------------------------|--------------------------------------------------------------------------------------------------------------------|
| [`../design/`](../design/)                                                                               | Pedagogical architecture and bug-fix rationale   | `methodology/` cites design chapters when illustrating *why* a given technique was applied to a specific component |
| [`../slashing-specification.md`](../slashing-specification.md)                                           | Normative contract                               | `methodology/` references theorem labels (`T-1`, `T-9.10`, …) but does not restate proofs                          |
| [`../slashing-verification.md`](../slashing-verification.md)                                             | Mathematical proof artifact                      | `methodology/` describes the *process* that produced theorems; the verification doc is the *product*               |
| [`../slashing-threat-model.md`](../slashing-threat-model.md)                                             | Defensive threat catalog (STRIDE + attack trees) | `methodology/attack-modeling/` is the pedagogical companion that teaches *how* to use those catalogs               |
| [`../slashing-traceability.md`](../slashing-traceability.md)                                             | Ledger of findings and their classifications     | `methodology/pipeline/` describes the *rules* the ledger enforces                                                  |
| [`../slashing-search-horizon.md`](../slashing-search-horizon.md)                                         | Operator-facing command runbook                  | `methodology/` is the *why* and *how to think about it*; search-horizon is the *what command to run*               |
| [`../../../formal/`](../../../../formal/)                                                                   | Mechanized artifacts (Rocq, TLA⁺, Sage)          | `methodology/formal-methods/` and `methodology/sage-models/` are the reader's guide                                |
| [`../../../fuzz/`](../../../../fuzz/), [`../../../casper/tests/slashing/`](../../../../casper/tests/slashing/) | Concrete test artifacts                          | `methodology/case-studies/` walks through how each artifact was motivated                                          |

---

## 6 · The promise this directory makes to its reader

By the time you have read [`01-philosophy.md`](./01-philosophy.md),
[`02-glossary-and-notation.md`](./02-glossary-and-notation.md), and
the four index chapters at the head of the four technique
subdirectories (`formal-methods/01`, `randomized-search/01`,
`differential-and-metamorphic/01`, `attack-modeling/01`), you should
be able to:

1. Read a candidate property — say, *“a slashed validator's bond is
   zero in every subsequent block”* — and pick the right technique(s)
   to verify it.
2. Distinguish a *bug witness* from a *model boundary* from an
   *assumption counterexample*.
3. Choose whether to write a Rocq theorem, a TLA⁺ invariant, a
   Hypothesis state machine, a proptest property, a fuzz target, or
   a Loom test — *and explain why your choice is the right one*.
4. Reproduce any of the sixteen discovered bugs from the
   [`case-studies/`](./case-studies/) directory in under thirty
   minutes given a clean clone.
5. Add a new search target without introducing the most common
   anti-pattern (a randomized test that asserts a false property of
   the model rather than a true property of the system).

The next chapter, [`01-philosophy.md`](./01-philosophy.md), starts
from the question *“what is a bug?”* and works up from there.
