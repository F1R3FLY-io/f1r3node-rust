# 02 · Stateful property testing with Hypothesis

> *“Things that have never happened before happen all the time.”*
> — Scott D. Sagan, *The Limits of Safety*, 1993 [Sag93].

This chapter explains the role of Hypothesis [MMM19] in the
slashing methodology. Hypothesis is a Python property-based testing
framework with two features absent from proptest that make it the
right tool for multi-step lifecycle search:

1. **Stateful machines** — a sequence of actions is a first-class
   citizen; the framework generates, executes, and **shrinks** the
   sequence as a whole.
2. **Persistent failing-example database** — a counterexample found
   in one run is replayed on the next, regardless of seed.

Organization:

- [§1 — When Hypothesis is the right tool](#1--when-hypothesis-is-the-right-tool)
- [§2 — The 12 slashing Hypothesis state machines](#2--the-12-slashing-hypothesis-state-machines)
- [§3 — Literate walkthrough of a stateful trace](#3--literate-walkthrough-of-a-stateful-trace)
- [§4 — From Python witness to Rust replay fixture](#4--from-python-witness-to-rust-replay-fixture)
- [§5 — Pitfalls](#5--pitfalls)
- [§6 — Related work](#6--related-work)

---

## 1 · When Hypothesis is the right tool

The decision tree from
[`../01-philosophy.md §3.2`](../01-philosophy.md#32--picking-the-tool--the-decision-tree)
routes the engineer to Hypothesis when the candidate property:

- describes a **multi-step lifecycle** (bonding → equivocating →
  detection → recording → propose → slash → withdraw → rebond);
- exhibits **emergent behavior** only visible after many actions
  (e.g. *“after 50 random actions, can the closure depth exceed `n − 1`?”*);
- benefits from **trace minimization** — the most informative
  counterexample is the **shortest** action sequence that produces
  the bug.

The slashing methodology uses Hypothesis specifically to:

1. Search adversarial **campaigns** — sequences of actions chosen
   to maximize an objective (e.g. *“maximize slashed honest stake”*).
2. Explore **multi-epoch** behaviors — actions that span epoch
   boundaries reveal stale-evidence and rebond-identity bugs.
3. Search **partition / gossip** schedules — actions where messages
   are delayed or dropped between subsets of validators.
4. Generate **persistent corpora** — the persistent database is
   committed to Git as
   [`casper/tests/slashing/hypothesis_persistent_corpus.rs`](../../../../../casper/tests/slashing/hypothesis_persistent_corpus.rs).

It is **not** used for:

- Per-function arithmetic exhaustion — use Kani.
- Single-step properties — use proptest.
- Concurrency interleavings — use Loom or TLA⁺.
- Static graph properties on small `n` — use Sage.

---

## 2 · The 12 slashing Hypothesis state machines

The Hypothesis search engines live in
[`formal/sage/slashing/hypothesis_search/`](../../../../../formal/sage/slashing/hypothesis_search/);
the Rust-side replay harnesses live in
[`casper/tests/slashing/hypothesis_*.rs`](../../../../../casper/tests/slashing/).
The twelve machines are:

| Machine                                        | Searches for                                                             |
|------------------------------------------------|--------------------------------------------------------------------------|
| `hypothesis_adversarial_scheduler.rs`          | Adversarial schedules driving worst-case detector behavior               |
| `hypothesis_arithmetic_projection_stress.rs`   | Stress for the checked-arithmetic projections                            |
| `hypothesis_assumption_minimization.rs`        | Minimization of inputs that establish theorem-precondition necessity     |
| `hypothesis_assumption_weakening.rs`           | Search for inputs where a theorem's precondition could be weakened       |
| `hypothesis_bundle_evidence_state_machine.rs`  | Action sequences mixing bonding, equivocating, slashing, and withdrawing |
| `hypothesis_feature_combination_coverage.rs`   | Combinatorial coverage of feature flags / parameter combinations         |
| `hypothesis_liveness_as_safety.rs`             | Liveness properties encoded as bounded-depth safety properties           |
| `hypothesis_multi_epoch_state_machine.rs`      | Sequences spanning epoch boundaries (rebond, stale evidence, churn)      |
| `hypothesis_objective_guided_frontier.rs`      | Objective-guided frontier search for adversarial damage                  |
| `hypothesis_partition_gossip_state_machine.rs` | Partition + gossip-delay schedules                                       |
| `hypothesis_persistent_corpus.rs`              | Replay of the persistent failing-example corpus                          |
| `hypothesis_precondition_fuzzing.rs`           | Fuzzing of theorem preconditions                                         |
| `hypothesis_reduced_scenarios.rs`              | Minimization of complex scenarios into deterministic Rust fixtures       |
| `hypothesis_rust_differential_corpus.rs`       | Differential search between Rust harness and Rocq oracle                 |
| `hypothesis_rust_metamorphic_checks.rs`        | Metamorphic relations (permutation invariance, idempotence)              |
| `hypothesis_rust_replay_fixtures.rs`           | Replay of all Sage/Hypothesis JSON fixtures on the Rust path             |

The naming convention `hypothesis_*` makes these distinguishable
from the proptest-driven `prop_t_*` files.

---

## 3 · Literate walkthrough of a stateful trace

The `hypothesis_multi_epoch_state_machine.rs` engine is the most
intricate; the simpler `hypothesis_adversarial_scheduler.rs` machine
illustrates the methodology without the multi-epoch bookkeeping.

### 3.1 The state machine in literate pseudocode

```
machine AdversarialScheduler:
    state:
        harness       : SlashingTestHarness
        action_log    : List(Action)
        equivocations : Set(Validator)
        slashed       : Set(Validator)

    initial:
        harness       ← SlashingTestHarness(n = 4, initial_bond = 100)
        action_log    ← []
        equivocations ← ∅
        slashed       ← ∅

    actions:
        ┌────────────────────────────────────────────────────────────┐
        │ bond_new_validator(v, stake)                               │
        │   pre:  v ∉ harness.validators ∧ stake > 0                 │
        │   step: harness.try_bond(v, stake)                         │
        │   log:  ("bond", v, stake)                                 │
        ├────────────────────────────────────────────────────────────┤
        │ sign_honest_block(v, seq)                                  │
        │   pre:  v ∈ harness.validators ∧ seq = harness.next_seq(v) │
        │   step: harness.sign_block(v, seq)                         │
        │   log:  ("sign", v, seq)                                   │
        ├────────────────────────────────────────────────────────────┤
        │ equivocate(v, seq)                                         │
        │   pre:  v ∈ harness.validators                             │
        │   step: harness.equivocate_block(v, seq)                   │
        │         equivocations.add(v)                               │
        │   log:  ("equivocate", v, seq)                             │
        ├────────────────────────────────────────────────────────────┤
        │ slash(v)                                                   │
        │   pre:  v ∈ equivocations ∧ v ∉ slashed                    │
        │   step: harness.simulate_slash_proposal(v)                 │
        │         slashed.add(v)                                     │
        │   log:  ("slash", v)                                       │
        ├────────────────────────────────────────────────────────────┤
        │ propose_neglect(v, seq)                                    │
        │   pre:  v ∉ slashed                                        │
        │   step: harness.sign_block_omitting_records(v, seq)        │
        │   log:  ("neglect", v, seq)                                │
        └────────────────────────────────────────────────────────────┘

    invariants (checked after every step):
        I₁  ∀ v ∈ slashed: harness.bond(v) = 0
        I₂  ∀ v ∈ harness.active_set: harness.bond(v) > 0
        I₃  closure_depth(harness) ≤ harness.validator_count − 1
        I₄  ∀ v ∈ harness.equivocators ⇒ ∃ EquivocationRecord(v, _)

    objective (for shrinking):
        |slashed| − |equivocations|       (* honest validators slashed *)
```

The framework picks actions according to a built-in policy
(weighted random, biased toward novel state). It executes the
action, re-checks the invariants, and records any violation as a
counterexample. On a violation, the framework shrinks the action
sequence — removing actions, simplifying argument values — until
no smaller failing sequence exists.

### 3.2 Why the invariants are checked **after every step**, not just at the end

A common mistake is to check invariants only at the end of the
sequence. Doing so:

1. **Hides intermediate violations** that get "fixed" later by chance.
2. **Confuses shrinking** — the minimization tries to delete actions
   the violation depends on but doesn't know which.

Per-step checks ensure the framework can localize the failure to
the smallest *prefix* that violates the invariant, which is then a
better starting point for shrinking.

### 3.3 Why the objective is *“honest validators slashed”*

Hypothesis shrinks toward smaller-and-still-failing inputs by
default; with an explicit objective function, the framework can be
biased toward "more interesting" failures even when the property
holds. The objective `|slashed| − |equivocations|` measures the
gap between the slashed set and the actual equivocator set —
positive values mean honest validators are being slashed, which is
exactly the failure mode T-1 forbids.

---

## 4 · From Python witness to Rust replay fixture

The Python-to-Rust bridge is the load-bearing part of the
Hypothesis integration. The bridge has three pieces:

### 4.1 JSON dump

The Hypothesis engine writes every interesting trace to a JSON file:

```json
{
  "fixture_kind": "hypothesis_adversarial_scheduler",
  "seed": "0xdeadbeef",
  "actions": [
    {"op": "bond", "validator": "v0", "stake": 100},
    {"op": "bond", "validator": "v1", "stake": 50},
    {"op": "sign", "validator": "v0", "seq": 0},
    {"op": "equivocate", "validator": "v0", "seq": 0},
    {"op": "slash", "validator": "v0"}
  ],
  "expected_invariants_at_end": {
    "I_1": true, "I_2": true, "I_3": true, "I_4": true
  }
}
```

The schema is defined in
[`formal/sage/slashing/scenario_schema.sage`](../../../../../formal/sage/slashing/scenario_schema.sage).

### 4.2 Rust replay harness

The Rust file
[`casper/tests/slashing/hypothesis_rust_replay_fixtures.rs`](../../../../../casper/tests/slashing/hypothesis_rust_replay_fixtures.rs)
reads the JSON fixture and replays it through the production-shaped
harness:

```
algorithm replay_fixture(fixture : JSON) → ReplayResult:
    let h ← new SlashingTestHarness(initial state from fixture)
    for each action in fixture.actions:
        match action.op:
            "bond"        → h.try_bond(action.validator, action.stake)
            "sign"        → h.sign_block(action.validator, action.seq)
            "equivocate"  → h.equivocate_block(action.validator, action.seq)
            "slash"       → h.simulate_slash_proposal(action.validator)
            … (etc)
    for each (key, expected) in fixture.expected_invariants_at_end:
        let actual ← evaluate_invariant(h, key)
        if actual ≠ expected:
            return ReplayResult::Divergence(key, expected, actual)
    return ReplayResult::Match
```

Divergence between the Python expectation and the Rust observation
is **always** an error — either the Python model is wrong about the
production behavior (model bug; refine), or the Rust path is wrong
about its own semantics (Rust bug; fix and add regression).

### 4.3 Promotion to deterministic Rust fixture

When a Hypothesis-generated trace surfaces a bug, the methodology
**does not** simply replay the JSON forever. The trace is shrunk
inside Python, then translated into a hand-written Rust test:

```
algorithm promote_hypothesis_witness_to_rust_fixture(w : ShrunkenTrace):
    ▸ 1. write Rust test under casper/tests/slashing/uc_NN_*.rs or
         pre_fix_bug_N.rs as appropriate
    ▸ 2. inline the action sequence literally (no JSON dependency)
    ▸ 3. assert each invariant explicitly
    ▸ 4. add a comment citing the source Hypothesis machine and seed
    ▸ 5. delete the JSON fixture from the corpus
        (replaced by the deterministic Rust regression)
    ▸ 6. record promotion in slashing-traceability.md
```

This is the methodology's **promotion principle for stateful
witnesses**: once a witness is interesting enough to be a regression,
it should become a first-class Rust test, not a JSON file. JSON
fixtures are *intermediate*; deterministic Rust tests are *permanent*.

---

## 5 · Pitfalls

### 5.1 Pitfall: forgetting `precondition`

Without preconditions, Hypothesis happily generates malformed action
sequences (e.g. `slash(v)` before `bond(v)`). The framework counts
these as failures even though they don't represent a real defect.

**Mitigation**: every action in this development declares its
preconditions via Hypothesis's `precondition` decorator; the engine
filters infeasible actions automatically.

### 5.2 Pitfall: non-deterministic harness

If the harness has non-deterministic behavior (e.g. iteration order
of a `HashMap`), a replayed trace may produce different results on
different runs. Hypothesis cannot shrink reliably under
non-determinism.

**Mitigation**: every harness in this development uses `BTreeMap` /
`BTreeSet` (sorted-order iteration) and accepts an explicit RNG seed
so randomization is reproducible.

### 5.3 Pitfall: invariant that depends on internal state

An invariant that reads an internal harness field (rather than going
through the observation API) breaks when the harness is refactored.

**Mitigation**: same rule as proptest — invariants go through the
documented observation API only.

### 5.4 Pitfall: testing the model, not the system

If the Hypothesis engine and the Rust replay harness both have the
same model bug, the replay agrees with the engine and the test
passes spuriously.

**Mitigation**: the **Rocq oracle** is the third implementation;
[`prop_t_triple_bisim_*.rs`](../../../../../casper/tests/slashing/) drives
all three through the same trace and asserts triple agreement. See
[`../differential-and-metamorphic/03-triple-bisimilarity.md`](../differential-and-metamorphic/03-triple-bisimilarity.md).

---

## 6 · Related work

- **Hypothesis**: MacIver *et al.* [MMM19].
- **Stateful property testing**: Claessen & Hughes [CH02] extend
  QuickCheck with stateful machines; this is the model Hypothesis's
  `RuleBasedStateMachine` builds on.
- **Trace minimization**: Zeller [Zel02] (delta debugging) is the
  intellectual ancestor of shrinking algorithms.
- **Differential testing of model and implementation**: McKeeman
  [McK98] is the original differential-testing paper.

DOIs in [`../references.md`](../references.md).

---

## 7 · Next chapter

[`03-coverage-guided-fuzzing.md`](./03-coverage-guided-fuzzing.md)
— the byte-level, structure-aware fuzzing layer. Where proptest and
Hypothesis sample inputs randomly with strategies, libFuzzer
samples inputs *adaptively* — mutating inputs to maximize edge
coverage in the binary under test.
