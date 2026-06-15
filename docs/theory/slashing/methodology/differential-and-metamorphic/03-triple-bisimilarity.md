# 03 · Triple-bisimilarity testing

> *“The whole is greater than the sum of its parts.”*
> — Aristotle, *Metaphysics*, c. 350 BCE.

This chapter explains the **triple-bisimilarity driver**: a single
test framework that runs the **same trace** through three
implementations of the slashing protocol — the Rust test harness,
the Rocq-derived oracle, and the production-shaped adapter — and
asserts that all three agree on every observable.

The triple-bisim pattern is the methodology's strongest *operational*
evidence (the Rocq bisimilarity theorem T-15a/b is the strongest
*mathematical* evidence; the two are complementary).

Organization:

- [§1 — Why three implementations](#1--why-three-implementations-and-not-two)
- [§2 — The triple-bisim architecture](#2--the-triple-bisim-architecture)
- [§3 — The three prop_t_triple_bisim_* properties](#3--the-three-prop_t_triple_bisim_-properties)
- [§4 — Failure-classification table](#4--failure-classification-table)
- [§5 — Pitfalls](#5--pitfalls)
- [§6 — Related work](#6--related-work)

---

## 1 · Why three implementations and not two?

A pairwise comparison (Rust vs. Rocq oracle, or Rust vs. Scala) is
informative, but the absence of disagreement on a pairwise comparison
can have two causes:

1. Both implementations are correct.
2. Both implementations share the same bug.

A triple-bisim comparison eliminates the second cause for a
non-trivial class of bugs: the three implementations have been
*independently constructed* — the Rocq oracle from mathematical
specification, the Rust harness from the Rust source, the production
adapter from the wire-level production code path — so a shared bug
must be a *triple* coincidence, an event of negligible probability.

The methodology adopts this from the **N-version programming**
tradition [AvLi77]: independent implementations of the same
specification, divergence treated as evidence of a defect in at most
`N − 1` of them.

### 1.1 Why three, not five?

Each additional implementation reduces the residual probability of
*“all share the same bug”* exponentially, but each costs additional
maintenance. Three is the methodology's chosen balance:

- **Cost** — three implementations is feasible (Rocq + Rust ×
  2 adapters).
- **Coverage** — the three are sufficiently different in origin
  (mathematical specification, test harness, production code) that
  shared-bug events are rare.
- **Diagnosability** — disagreement among three is naturally
  classifiable (which **two** agree?), giving the engineer
  immediate insight into which implementation likely contains the
  defect.

---

## 2 · The triple-bisim architecture

The triple-bisim driver lives in
[`casper/tests/slashing/triple_bisim_driver.rs`](../../../../../casper/tests/slashing/triple_bisim_driver.rs)
and the three observers live in
[`casper/tests/slashing/observer.rs`](../../../../../casper/tests/slashing/observer.rs),
[`casper/tests/slashing/oracle.rs`](../../../../../casper/tests/slashing/oracle.rs),
[`casper/tests/slashing/oracle_adapter.rs`](../../../../../casper/tests/slashing/oracle_adapter.rs),
and
[`casper/tests/slashing/production_adapter.rs`](../../../../../casper/tests/slashing/production_adapter.rs).

The architecture is:

```
                        ┌─────────────────┐
                        │  Trace driver   │
                        │ (proptest /     │
                        │  hypothesis)    │
                        └────────┬────────┘
                                 │ (same action sequence)
              ┌──────────────────┼──────────────────┐
              │                  │                  │
              ▼                  ▼                  ▼
      ┌────────────┐     ┌────────────┐     ┌────────────────┐
      │ Harness    │     │ Rocq       │     │ Production     │
      │ (Rust test │     │ oracle     │     │ adapter        │
      │  surface)  │     │ (mirror)   │     │ (production    │
      │            │     │            │     │  code path)    │
      └─────┬──────┘     └─────┬──────┘     └────────┬───────┘
            │                  │                     │
            └──────────────────┼─────────────────────┘
                               │ (same observation)
                               ▼
                  ┌────────────────────────┐
                  │  SlashingObserver      │
                  │  trait checks parity   │
                  └────────────────────────┘
```

The `SlashingObserver` trait
([`casper/tests/slashing/observer.rs`](../../../../../casper/tests/slashing/observer.rs))
defines the **agreed observable contract**: a set of pure queries
each of the three implementations must answer. The driver calls each
query on each implementation after every action and asserts agreement.

### 2.1 The observable contract

The observable contract is intentionally minimal — only what an
external observer of the protocol can see:

| Observable                | Type signature                            |
|---------------------------|-------------------------------------------|
| `bond(v)`                 | `Validator → i64`                         |
| `is_active(v)`            | `Validator → bool`                        |
| `has_record(v, base_seq)` | `(Validator, i64) → bool`                 |
| `dispatch(h)`             | `BlockHash → Status`                      |
| `slashed_set()`           | `→ Set(Validator)`                        |
| `record_set()`            | `→ Set((Validator, i64, Set(BlockHash)))` |
| `fork_choice_weights()`   | `→ Map(BlockHash → i64)`                  |

Implementation-internal state (heap layout, iteration order, internal
hash maps) is **deliberately excluded**. Two implementations are
permitted to differ on internal state as long as their observable
behavior agrees.

### 2.2 Why this contract is the right one

The contract corresponds to the **labels** of the labeled transition
system (LTS) used in the Rocq bisimilarity definition; see
[`../../slashing-verification.md §3`](../../slashing-verification.md).
A bisimilar pair must agree on every label observable to an external
observer; the contract is the operational form of that requirement.

---

## 3 · The three `prop_t_triple_bisim_*` properties

The slashing test suite includes three triple-bisim properties:

| File                                | Property                                                                  |
|-------------------------------------|---------------------------------------------------------------------------|
| `prop_t_triple_bisim_dispatch.rs`   | The detector's `dispatch(h)` agrees across all three implementations      |
| `prop_t_triple_bisim_records.rs`    | The record set agrees across all three implementations after every action |
| `prop_t_triple_bisim_forkchoice.rs` | The fork-choice weights agree across all three implementations            |

Each property follows the same template:

### 3.1 Template

```
property prop_t_triple_bisim_dispatch:
    sample n   ← uniform(2, 8)
    sample seq ← uniform(1, 10)
    sample actions ← random_action_sequence(n, seq)

    let H ← new SlashingTestHarness(n, …)
    let O ← new RocqOracle(n, …)
    let P ← new ProductionAdapter(n, …)

    for each action in actions:
        H.apply(action)
        O.apply(action)
        P.apply(action)
        for each h in interesting_hashes(action):
            assert H.dispatch(h) = O.dispatch(h) = P.dispatch(h)
```

The pre- and post-conditions of every action are validated through
the `SlashingObserver` trait, so the three implementations are
treated symmetrically.

---

## 4 · Failure-classification table

When the triple-bisim driver reports disagreement, the methodology
classifies the failure using the **majority rule** — the two that
agree are presumed correct, the dissenter is the prime suspect.

| H | O | P | Likely defect                              | First-pass action                                                   |
|---|---|---|--------------------------------------------|---------------------------------------------------------------------|
| A | A | B | Production adapter or production code path | Audit `production_adapter.rs` and the production Rust path it wraps |
| A | B | A | Rocq oracle mirror                         | Audit `oracle.rs` against the Rocq theorem it mirrors               |
| B | A | A | Harness                                    | Audit `harness.rs` against the test-plan contract                   |
| A | B | C | Multiple defects                           | Halt and bisect: which action introduced the third dissent?         |

The methodology requires every triple-bisim failure to result in
either:

1. A source fix (when the defect is in production code), or
2. A harness/oracle refactor (when the defect is in test scaffolding), or
3. A test-plan amendment (when the test specification is ambiguous).

A "flake" classification is **not permitted** — the triple-bisim
driver uses deterministic seeds and is replayable; a divergence is
either a defect or a bug in the driver itself.

---

## 5 · Pitfalls

### 5.1 Pitfall: the observers diverge on initialization

If the three implementations start from different initial states,
every observation diverges. The driver enforces a `Init` synchronization
step that verifies all three observers report the same initial state
before any action is applied.

**Mitigation**: the `Init` check is the first assertion in every
triple-bisim test; failure halts the test before the action sequence
runs.

### 5.2 Pitfall: the Rocq oracle mirror drifts from the Rocq theorem

If the `oracle.rs` mirror is updated without re-verifying against the
Rocq theorem, the mirror can become a stale or incorrect oracle.

**Mitigation**: the methodology requires every change to `oracle.rs`
to cite a Rocq theorem and to be paired with a re-execution of the
corresponding `prop_t_*` property. The CI fails if the citation is
absent or stale.

### 5.3 Pitfall: the production adapter wraps the wrong function

The production adapter is supposed to wrap the **same** function the
production code path invokes. If it wraps a stale or test-only
function, the triple-bisim measures the wrong thing.

**Mitigation**: the production adapter calls the production functions
*directly* (no test-only intermediaries), and the function calls are
spot-checked in code review.

### 5.4 Pitfall: action sequences not representative of production

Random action sequences may exercise unusual configurations that
diverge cosmetically without representing real production risk.

**Mitigation**: the proptest strategy `random_action_sequence` is
weighted toward production-realistic transitions; the weighting is
documented in
[`casper/tests/slashing/generators.rs`](../../../../../casper/tests/slashing/generators.rs).

---

## 6 · Related work

- **N-version programming**: Avizienis & Lyu [AvLi77].
- **Observational equivalence in process calculi**: Milner [Mil89],
  Sangiorgi [San12].
- **Refinement and bisimulation in distributed systems**: Lynch &
  Vaandrager [LV95], Abadi & Lamport [AL91].
- **Cross-validation in software engineering**: Knight & Leveson
  [KL86] critique the assumption of independent failures in N-version
  programming; the methodology addresses this by varying the
  implementations *origin* (mathematical specification vs. test
  harness vs. production code) as well as their code.

DOIs in [`../references.md`](../references.md).

---

## 7 · Next chapter

[`../attack-modeling/01-stride-and-attack-trees.md`](../attack-modeling/01-stride-and-attack-trees.md)
— the **threat-modeling** layer of the methodology. Where the
chapters so far were about *correctness*, this layer is about *what
the adversary is trying to do* and *which correctness violations
matter to them*.
