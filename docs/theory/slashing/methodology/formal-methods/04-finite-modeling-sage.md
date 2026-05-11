# 04 · Finite modeling with Sage

> *“In theory, theory and practice are the same. In practice, they
> are not.”* — Aphorism, often attributed to Yogi Berra.
>
> *“A picture is worth a thousand words, but a counterexample is
> worth a thousand pictures.”* — Adapted from
> Halbwachs *et al.* [Hal91].

This chapter explains the role of Sage [SageDev] — the open-source
mathematics software system — in the slashing methodology. Sage is
neither a theorem prover nor a model checker; it is an **exact
finite-computation engine** with first-class graph, combinatorial,
and integer-arithmetic libraries. The slashing methodology uses
Sage to *generate* witnesses that the rest of the stack then
classifies and either promotes or dismisses.

Organization:

- [§1 — Why Sage, not Python](#1--why-sage-not-python)
- [§2 — The slashing Sage corpus](#2--the-slashing-sage-corpus)
- [§3 — A literate walkthrough of `closure_model.sage`](#3--a-literate-walkthrough-of-closure_modelsage)
- [§4 — From Sage finding to traceability entry](#4--from-sage-finding-to-traceability-entry)
- [§5 — Pitfalls](#5--pitfalls)
- [§6 — When Sage is the wrong tool](#6--when-sage-is-the-wrong-tool)

---

## 1 · Why Sage, not Python

Three properties make Sage [SageDev] uniquely well-suited to the
slashing search program; none is satisfied by raw Python alone.

### 1.1 Exact integer arithmetic

Floating-point arithmetic is forbidden in this work — every quantity
of interest (validator counts, stake bonds, sequence numbers, block
numbers, epoch indices, BFT bounds) is an integer. Sage's `Integer`
type is unbounded-precision, so an arithmetic boundary in the model
exactly reflects an arithmetic boundary in the production system
when the production system uses bounded primitives.

For example, the model
[`bounded_arithmetic_model.sage`](../../../../../formal/sage/slashing/bounded_arithmetic_model.sage)
compares exact Sage arithmetic against checked, wrapping, and
saturating bounded integer projections; the witness for Sage
finding #8 (overflow at `i64::MAX + 1`) emerges directly from the
exact-vs-bounded comparison.

### 1.2 First-class graph and combinatorial libraries

Sage's `DiGraph`, `Subsets`, `Permutations`, `Partitions`, and
related libraries make algorithms on small-to-moderate graphs
expressible in handfuls of lines. The slashing models exploit this
heavily:

- `closure_model.sage` uses `DiGraph.transitive_closure()` to
  cross-check the iterative slash closure.
- `tracker_race_model.sage` uses `Permutations(n)` to enumerate
  thread schedules.
- `graph_edge_cases_model.sage` uses `DiGraph.is_simple()`,
  `is_acyclic()` to classify edge cases.
- `damage_optimizer.sage` uses `Subsets` over edge sets to enumerate
  adversarial graph shapes within a stake budget.

In raw Python, each of these would require either a hand-written
implementation (with its own bugs to audit) or an external
dependency (NetworkX, sympy) — Sage bundles them.

### 1.3 Mathematical pedigree

Sage is a verified-by-use mathematics platform with a peer-reviewed
codebase used by the academic mathematics community. The slashing
methodology's confidence in a Sage finding is bounded by Sage's
correctness, not by a hand-rolled implementation's correctness — a
weaker assumption.

---

## 2 · The slashing Sage corpus

Thirty Sage scripts and one Hypothesis-driven scenario search engine
live in [`formal/sage/slashing/`](../../../../../formal/sage/slashing/).
They are grouped into fourteen families documented in
[`../sage-models/`](../sage-models/); a brief one-liner per family:

| Family                                                                                            | What it searches                                                                                |
|---------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------|
| [Closure & graph](../sage-models/01-closure-and-graph.md)                                          | Two-level closure, neglect edges, BFT bound                                                     |
| [Adversarial & damage](../sage-models/02-adversarial-and-damage.md)                                | Adversarial timing, damage optimization, deep-threat                                            |
| [Arithmetic & projection](../sage-models/03-arithmetic-and-projection.md)                          | Bounded vs. exact arithmetic envelopes                                                          |
| [Differential & bisimilarity](../sage-models/04-differential-and-bisimilarity.md)                  | Rust/Scala/Rocq-oracle divergence                                                                |
| [Epoch & lifecycle](../sage-models/05-epoch-and-lifecycle.md)                                      | Epoch boundaries, churn, rebond                                                                 |
| [Evidence visibility & timing](../sage-models/06-evidence-visibility-and-timing.md)                | Partial visibility, late reports                                                                |
| [Horizon & objective-frontier](../sage-models/07-horizon-and-objective-frontier.md)                | Long-range adversarial sweeps                                                                   |
| [Hypothesis stateful search](../sage-models/08-hypothesis-stateful-search.md)                      | Multi-step lifecycle traces with shrinking                                                       |
| [Pipeline & accounting](../sage-models/09-pipeline-and-accounting.md)                              | Bond accounting, slash idempotence, record normalization                                         |
| [Quorum intersection](../sage-models/10-quorum-intersection.md)                                    | Weighted quorum intersection                                                                     |
| [Tracker race](../sage-models/11-tracker-race.md)                                                  | Concurrent tracker schedules                                                                     |
| [Theorem-assumption counterexamples](../sage-models/12-theorem-assumption-counterexamples.md)      | Witnesses for theorem preconditions                                                              |
| [Scenario corpus generation](../sage-models/13-scenario-corpus-generation.md)                      | Deterministic fixture corpus                                                                     |
| [Weighted stake optimization](../sage-models/14-weighted-stake-optimization.md)                    | Stake-weighted attack optimization                                                               |

Each Sage script writes a JSON-serializable witness; the witness is
consumed by:

1. The Rust trace-replay test
   ([`casper/tests/slashing/hypothesis_rust_replay_fixtures.rs`](../../../../../casper/tests/slashing/hypothesis_rust_replay_fixtures.rs))
2. The traceability ledger
   ([`../slashing-traceability.md`](../../slashing-traceability.md))
3. The findings file ([`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md))

---

## 3 · A literate walkthrough of `closure_model.sage`

The two-level closure model is the canonical example: small Sage
program, exact graph computation, witness emission, downstream
promotion.

### 3.1 The problem

A validator can be slashed for two reasons:

1. **Direct equivocation** — signing two distinct blocks at the same
   sequence number.
2. **Neglect** — proposing a block whose justifications fail to
   acknowledge a previously-recorded direct offender.

The **closure** is the transitive set: direct offenders, plus
neglecters, plus neglecters of neglecters, …, plus all `n − 1`
levels. The closure terminates by a Knaster–Tarski-style fixed-point
argument: the slash set is monotone non-decreasing in a domain
bounded by the number of validators.

### 3.2 The closure in pseudocode

```
algorithm slash_closure(n : ℕ, equivocators : Set(Validator),
                          edges : Set(NeglectEdge)) → ClosureResult:
    let graph ← DiGraph(vertices = 0..n, edges = edges)
    let slashed ← copy(equivocators)
    let rounds ← [sorted(slashed)]
    loop:
        let next_slashed ← slashed
        for each validator v in sorted(slashed):
            next_slashed ← next_slashed ∪ in_neighbors(graph, v)
        if next_slashed = slashed:
            return ClosureResult(closure = sorted(slashed), rounds = rounds)
        slashed ← next_slashed
        append rounds with sorted(slashed)
        if length(rounds) > n + 1:
            (* invariant: closure must converge in ≤ n iterations *)
            raise AssertionError("closure did not converge within n rounds")
```

The Sage implementation mirrors this almost line-for-line; see
[`formal/sage/slashing/closure_model.sage`](../../../../../formal/sage/slashing/closure_model.sage)
lines 25–45.

### 3.3 The cross-check

```
algorithm closure_via_transitive_closure(n, equivocators, edges) → Set(Validator):
    let graph ← DiGraph(vertices = 0..n, edges = edges)
    let transitive ← graph.transitive_closure()
    let closure ← copy(equivocators)
    for each validator v in 0..n:
        if v ∈ closure: continue
        if ∃ offender ∈ equivocators. transitive.has_edge(v, offender):
            closure ← closure ∪ { v }
    return sorted(closure)
```

The Sage model runs **both** the iterative algorithm and the
transitive-closure cross-check, then asserts the two outputs agree.
If they disagree, the model has a bug; if they agree, the model
emits a witness file.

This **dual implementation** is a methodology pattern (also known as
*N-version programming* in safety-critical engineering [AvLi77]):
two implementations of the same predicate, divergence treated as
evidence of a bug in at least one of them.

### 3.4 The witness format

A typical witness from `closure_model.sage`:

```json
{
  "kind": "two_level_closure_witness",
  "n": 4,
  "equivocators": [1],
  "edges": [[0, 1]],
  "closure": [0, 1],
  "rounds": [[1], [0, 1]],
  "bft_bound": 1,
  "fault": 1,
  "active_after": 2,
  "quorum_required": 3,
  "quorum_violated": true,
  "shortest_neglect_paths": {"0": [0, 1]}
}
```

Each field is purposeful:

- `n`, `equivocators`, `edges` are the **input** that reproduces the
  witness.
- `closure`, `rounds`, `shortest_neglect_paths` are the **output**
  that the production code must agree with.
- `bft_bound`, `fault`, `active_after`, `quorum_required`,
  `quorum_violated` are the **classification metadata** the
  traceability ledger uses.

### 3.5 What this becomes downstream

A witness with `quorum_violated = true` becomes:

1. **A Rocq theorem about the BFT bound**:
   `formal/rocq/slashing/theories/TwoLevelSlashing.v` Theorem
   `two_level_closure_bft_bound` proves that the BFT bound is
   preserved if and only if the direct-offender stake is ≤ ⅓ of
   total stake; the Sage witness corroborates the theorem.

2. **A TLA⁺ invariant**:
   `formal/tlaplus/slashing/TwoLevelSlashing.tla` `Inv_BFTBound`
   exhaustively checks the same property on finite instances.

3. **A Rust regression fixture**:
   [`casper/tests/slashing/prop_t_12_quorum_preservation.rs`](../../../../../casper/tests/slashing/prop_t_12_quorum_preservation.rs)
   reproduces the closure on the harness state machine.

4. **A traceability ledger entry** with status
   `proof_or_model_strengthening` (the witness was promoted to
   Rocq theorem and TLA⁺ invariant).

This is **evidence stacking** in concrete form — see
[`../pipeline/03-evidence-stacking.md`](../pipeline/03-evidence-stacking.md).

---

## 4 · From Sage finding to traceability entry

The promotion of a Sage witness to a production artifact follows a
strict procedure, encoded in
[`../pipeline/01-witness-to-source-rule.md`](../pipeline/01-witness-to-source-rule.md).
For Sage specifically, the procedure is:

```
algorithm promote_sage_finding(w : SageWitness) → LedgerEntry:
    ▸ 1. classify w under threat-model vocabulary:
         ┌──────────────────────────────────────────────────────────────┐
         │ • bisimilar — w demonstrates expected Rust/Scala agreement   │
         │ • permitted_bug_fix — w shows Rust deliberately diverges     │
         │ • candidate_boundary — w exercises a theorem precondition    │
         │ • projection_risk — w shows a model could project differently│
         │ • assumption_counterexample — w shows a precondition is hard │
         │ • unexpected — must be reclassified before commit            │
         └──────────────────────────────────────────────────────────────┘
    ▸ 2. trace w into Rust production path:
         (a) write a deterministic reproduction harness that drives the
             Rust slashing pipeline through the same DAG / stake / epoch
             configuration that w describes;
         (b) observe the production behavior:
              | reproduces w     → confirmed_current_bug
              | rejects w-input  → not_reproduced_in_rust
              | guarded by check → projection_risk_guarded
              | requires theorem precondition to hold → assumption_counterexample
              | suggests stronger theorem            → proof_or_model_strengthening
              | unclear                              → needs_source_audit
    ▸ 3. append to formal/sage/slashing/FINDINGS.md with finding number
    ▸ 4. append to slashing-traceability.md with classification and status
    ▸ 5. if status = confirmed_current_bug:
            (a) fix Rust source
            (b) add pre_fix_bug_N.rs regression
            (c) add Rocq theorem if normative
            (d) update threat model / spec / design docs
       else if status = proof_or_model_strengthening:
            (a) add Rocq theorem and/or TLA+ invariant
            (b) add prop_t_*.rs regression
       else: record-only (no source change)
    ▸ 6. (always) write a deterministic JSON fixture file under
         casper/tests/slashing/<corpus>/ so the witness is replayable
```

This is the **witness rule** of
[`../01-philosophy.md §4`](../01-philosophy.md) in operational form.
The Sage witness is the *input* to the rule; the ledger entry is the
*output*.

### 4.1 The bookkeeping invariant

Every Sage witness must eventually have a finding number, a
classification, a status, and at least one of: source fix, formal
artifact, regression fixture. A witness without any of these is a
documentation bug; the methodology forbids leaving witnesses
classification-free.

---

## 5 · Pitfalls

### 5.1 Pitfall: the witness is a model artifact

A Sage witness can describe a configuration the production Rust path
**cannot construct**. Promoting such a witness to a Rust bug wastes
review time and erodes ledger quality.

**Mitigation**: every Sage script in this development includes a
trace-replay test that drives the production Rust path through the
same configuration. If the Rust path rejects the input, the witness
is classified `not_reproduced_in_rust` and recorded as a model
artifact rather than a bug.

### 5.2 Pitfall: the cross-check passes too

Two implementations of the same predicate may both have the same bug.
The dual-implementation cross-check inside Sage models reduces but
does not eliminate this risk; the **third** check is the production
Rust path itself.

**Mitigation**: every Sage model uses three implementations of any
load-bearing computation: the iterative version, the Sage-library
version (e.g. `transitive_closure()`), and the Rust trace-replay
version. Agreement among all three is strong evidence; agreement
among any two with disagreement of the third surfaces a bug in
exactly one of the implementations.

### 5.3 Pitfall: state-space explosion in the search

Sage searches that enumerate over `Subsets(E)` where `|E|` grows
quadratically with `n` become intractable for `n > 6`. A naively
written model can run for hours without producing useful witnesses.

**Mitigation**: every search in this development is **objective-guided**
(see [`07-horizon-and-objective-frontier.md`](../sage-models/07-horizon-and-objective-frontier.md))
— it has a scoring function (closure depth, slashed stake, accountability
gap) and prunes by score. The `objective_frontier_model.sage` model
formalizes the objective-guided search.

### 5.4 Pitfall: forgetting to seed deterministically

Sage's randomized methods seed from the system clock by default. A
finding that reproduces on one run may not reproduce on the next.

**Mitigation**: every Sage model in this development takes a `--seed`
argument and writes the seed into the witness JSON. Replays use the
same seed.

---

## 6 · When Sage is the wrong tool

| Situation                                                       | Right tool                                            |
|-----------------------------------------------------------------|-------------------------------------------------------|
| The property is over unbounded validator counts                 | Rocq                                                  |
| The property requires precise concurrency interleavings         | TLA⁺ + TLC; or Loom                                   |
| The property is about Rust source code at all                   | Kani for exhaustion; proptest for shrinking            |
| The property is about a multi-step lifecycle trace              | Hypothesis                                            |
| The property is about byte-level encoding                       | libFuzzer                                              |
| The property is about cross-implementation equivalence          | Triple-bisimilarity driver                            |

Sage's niche is **structured exact search over small spaces**. When
the space gets too large, the search becomes random sampling without
the shrinking infrastructure that Hypothesis or proptest provide;
when the space gets too small, the cost of writing the Sage model
exceeds the benefit of the witness.

---

## 7 · Related work

- **Sage**: SageMath developers [SageDev]; see also Stein & Joyner
  [SJ05].
- **Exact-arithmetic / computer-algebra correctness**: see Wiedijk
  [Wie03] for a comparison of computer-algebra systems and theorem
  provers.
- **N-version programming**: Avizienis & Lyu [AvLi77].
- **Objective-guided search**: Goldberg [Gol89] (genetic algorithms);
  Lehman & Stanley [LS11] (novelty search).
- **Property-based test corpora**: Claessen & Hughes [CH00]
  (QuickCheck).

DOIs in [`../references.md`](../references.md).

---

## 8 · Next chapter

[`../randomized-search/01-property-testing-proptest.md`](../randomized-search/01-property-testing-proptest.md)
— the *randomized* arm of the methodology, beginning with the lowest-
cost, highest-throughput layer.
