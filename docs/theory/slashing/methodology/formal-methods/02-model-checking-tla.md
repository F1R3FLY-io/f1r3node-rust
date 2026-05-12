# 02 · Model checking with TLA⁺ (TLC and Apalache)

> *“Writing is nature's way of letting you know how sloppy your
> thinking is.”* — Leslie Lamport, attributed.
>
> *“If you don't write it down, you don't know what you're doing.”* —
> Leslie Lamport, *Specifying Systems* [Lam02].

This chapter explains the role of TLA⁺ in the slashing methodology.
TLA⁺ sits between Rocq (unbounded, expensive) and randomized testing
(bounded, cheap, fast) in the epistemic stack. It is the **finite,
exhaustive, complete** arm: every finite state in the bounded model is
visited; every invariant is checked at every state.

Organization:

- [§1 — Why TLA⁺ at all](#1--why-tla-at-all)
- [§2 — TLC vs. Apalache](#2--tlc-vs-apalache)
- [§3 — The seven slashing models](#3--the-seven-slashing-models-overview)
- [§4 — Anatomy of a model-checked invariant](#4--anatomy-of-a-model-checked-invariant)
- [§5 — Concurrency and the `ConcurrentTracker` toggle](#5--concurrency-and-the-concurrenttracker-toggle)
- [§6 — Pitfalls](#6--pitfalls)
- [§7 — Promotion to Rocq and back-promotion to Rust](#7--promotion-to-rocq-and-back-promotion-to-rust)

---

## 1 · Why TLA⁺ at all

A reader who has read [`01-mechanized-proof-rocq.md`](./01-mechanized-proof-rocq.md)
may reasonably ask: *“If Rocq gives unbounded proof, why also TLA⁺?”*

Three answers:

### 1.1 Different cost surface

A Rocq theorem about a state machine costs **proof effort**
proportional to the structure of the *proof term*. A TLC model check
costs **CPU time** proportional to the size of the *state space*.
For small but adversarially-shaped state spaces (the slashing
two-level closure on `n ≤ 5`, or the equivocation detector on `n ≤ 4`
with `b ≤ 2` blocks per seq), TLC delivers an exhaustion result in
seconds. A Rocq theorem of the same shape — *“for `n ≤ 5`, no
adversary can drive a particular invariant to false”* — would
require thousands of lines of case analysis.

### 1.2 Specification as documentation

A TLA⁺ specification is **executable English**. The
`EquivocationDetector.tla` model reads top-to-bottom as a description
of the protocol, with each `\E v ∈ Validators` corresponding to a
sentence in the architecture document. It serves as a third
checkable artifact alongside the Rocq theorems and the Rust code —
when the three disagree, the disagreement itself is informative.

### 1.3 Concurrency is TLA⁺'s home turf

The slashing subsystem has one known concurrency hazard: the
lock-free tracker access that caused Bug #2. Concurrency models in
Rocq are possible (e.g. [HKR12]) but require encoding the scheduler.
TLA⁺ has a scheduler natively: every action is independently fairly
or unfairly schedulable, and TLC explores every permitted ordering.
The `ConcurrentTracker.tla` model demonstrates the bug with the
literal three-line invariant the lock-free version violates.

---

## 2 · TLC vs. Apalache

The slashing development uses two TLA⁺ engines: TLC for explicit-state
enumeration, Apalache for SMT-backed symbolic checking.

| Property                        | TLC (explicit-state)                               | Apalache (symbolic)                                  |
|---------------------------------|----------------------------------------------------|------------------------------------------------------|
| **State representation**        | One concrete state per reachable configuration     | SMT formula over symbolic state                      |
| **Explores**                    | Every reachable state, breadth-first by default    | Every state up to a depth bound, by SMT search       |
| **State-space cost**            | Exponential in protocol parameters                 | Sub-exponential in many cases (SMT prunes)           |
| **Best at**                     | Liveness via Büchi conversion; tight finite bounds | Wider validator/epoch domains; numerical envelopes   |
| **Counterexample form**         | Concrete state sequence                            | Concrete state sequence reconstructed from SMT model |
| **Liveness checking**           | Yes (via `WF_` / `SF_` fairness, Büchi product)    | Limited; primarily safety                            |
| **Practical limit (this work)** | `n ≤ 4, d ≤ 6, b ≤ 2 ⇒ |𝒮| ≤ 10⁶`                  | `n ≤ 8, d ≤ 12, b ≤ 4 ⇒ tractable by SMT`            |
| **Tool reference**              | [Lam02, Yu99]                                      | Apalache: [KKT19, KKT20]                             |

The default choice in the slashing development is TLC; Apalache is
*nominated* as the fallthrough for searches where the state space
is too large for TLC but narrow enough that the SMT solver can
prune effectively. The runner
`scripts/ci/slashing-search-horizon.sh` enables Apalache when the
operator sets `RUN_APALACHE=1` (it invokes `apalache-mc check`
against `MC_AuthorizedSlashFlow.tla` and `MC_TwoLevelSlashing.tla`).

> **Status note.** The Apalache hook exists in the CI script but
> has *not* been exercised against a specific finding as of this
> writing: no Sage/Hypothesis/Rocq finding in
> `formal/sage/slashing/FINDINGS.md` or
> [`../../slashing-traceability.md`](../../slashing-traceability.md)
> is attributable to an Apalache run. The technique is documented
> as the prescribed fallthrough for future work when TLC's bound
> proves too tight; the table below describes the configurations
> where that pathway *would* be invoked, not configurations where
> Apalache was actually run.

### 2.1 When TLC explodes

Three configurations have been observed to push TLC into prohibitive
runtime in this development:

| Configuration                                                      | Reachable states | TLC behavior                                                            |
|--------------------------------------------------------------------|------------------|-------------------------------------------------------------------------|
| `EquivocationDetector` with `n = 5, d = 8, b = 3`                  | ~10⁸             | OOM at 32 GB RAM                                                        |
| `TwoLevelSlashing` with `n = 6, neglect edges allowed = unbounded` | ~10⁷             | 4 h wall time on 12-core; tight bound used                              |
| `AuthorizedSlashFlow` with epoch length unconstrained              | unbounded        | TLC cannot terminate; the prescribed fallthrough is Apalache (see note) |

In each case the bound was tightened so finite TLC exhaustion is
preserved at the operative bound. The Apalache pathway is the
prescribed extension when wider domains are required in future
work.

---

## 3 · The seven slashing models — overview

The TLA⁺ models in `formal/tlaplus/slashing/` are:

| File                          | Models                                                                               | Headline invariant                                                                 |
|-------------------------------|--------------------------------------------------------------------------------------|------------------------------------------------------------------------------------|
| `EquivocationDetector.tla`    | The detector LTS                                                                     | `Inv_DetectionSound ∧ Inv_DetectionComplete ∧ Inv_TaxonomyCorrect`                 |
| `ConcurrentTracker.tla`       | The lock-free vs. locked tracker, parameterized by `Locked ∈ BOOLEAN`                | `Inv_NoOverwrite` (violated when `Locked = FALSE`; satisfied when `Locked = TRUE`) |
| `SlashFlow.tla`               | End-to-end pipeline (detection → record → propose → SlashDeploy → PoS → fork-choice) | `Inv_Pipeline_Reaches_Effect ∧ Inv_NoSlashWithoutRecord`                           |
| `TwoLevelSlashing.tla`        | Closure of direct offenders + neglecters                                             | `Inv_ClosureTermination ∧ Inv_BFTBound ∧ Inv_QuorumIntersect`                      |
| `AuthorizedSlashFlow.tla`     | Slash authorization for current-epoch invalid-block evidence                         | `Inv_SlashOnlyIfAuthorized ∧ Inv_RebondRejectsStaleEvidence`                       |
| `JustificationProjection.tla` | Justification-validator-projection model                                             | `Inv_DuplicateValidatorsRejected`                                                  |
| `WithdrawFlow.tla`            | Post-quarantine withdrawal flow modelling Bug #10                                    | `Inv_TotalFundsConserved ∧ Inv_WithdrawalRetryable`                                |

The full list of invariants per model is in the
[`../slashing-verification.md §10`](../../slashing-verification.md) and
in [`formal/tlaplus/slashing/README.md`](../../../../../formal/tlaplus/slashing/README.md).

### 3.1 Why seven, not one big model?

A single TLA⁺ specification that combined all seven concerns would
have a state space approaching `10¹⁵` and would never terminate
under TLC. The slashing approach factors the system into
**single-responsibility** specifications, each small enough to
exhaustively check. The factoring is the same factoring used by the
Rocq modules (see
[`01-mechanized-proof-rocq.md §3`](./01-mechanized-proof-rocq.md));
it is reused here for the same reasons: cognitive load,
recompile/recheck cost, and trust factoring.

### 3.2 Why deliberately keep `ConcurrentTracker.tla` parameterized?

`MC_ConcurrentTracker.tla` is parameterized by `Locked ∈ BOOLEAN`,
and **both** runs must execute and be recorded:

- `Locked = FALSE` → TLC must **fail** `Inv_NoOverwrite` with a
  concrete two-thread witness. This is **proof that the bug exists**
  in the lock-free version (Scala behavior).
- `Locked = TRUE` → TLC must **pass** every invariant. This is
  **proof that the fix works** (Rust post-Bug-#2 behavior).

A model that only ever passes provides weak evidence; a model that
*also fails on the unfixed version* corroborates that the model has
contact with the real defect, not just with a sanitized abstraction.

---

## 4 · Anatomy of a model-checked invariant

This section walks through one invariant from `EquivocationDetector.tla`
to make the methodology concrete.

### 4.1 The Rocq theorem

```rocq
Theorem t_1_detection_sound : ∀ s v b,
  is_honest_in s v →
  ¬ is_slashable (classify s b).
```

### 4.2 The TLA⁺ equivalent

```tla
Inv_DetectionSound ==
    \A v \in Validators :
      \A s \in 1..MaxSeqNum :
        \A b \in 1..MaxBlocksPerSeq :
          detectedStatus[<<v, s, b>>] \in {"valid", "none"}
          \/ IsRealEquivocation(v, s)
```

Read aloud:

> *“For every validator `v`, every sequence number `s`, and every
> block `b`, the detector classifies `(v, s, b)` as `valid` or
> `none` (= not yet observed), unless `v` actually equivocated at
> `s`.”*

### 4.3 The TLC run

```bash
tlc -workers 12 MC_EquivocationDetector.tla
```

TLC enumerates breadth-first from `Init`. At every reachable state, it
evaluates `Inv_DetectionSound`. If a state violates it, TLC prints the
trace from `Init` to that state. The slashing development's expected
output for the post-fix code is:

```
Finished computing initial states: 1 distinct state generated at ...
Model checking completed. No error has been found.
...
States generated: 547823
Distinct states: 89441
Queue size: 0
```

### 4.4 What this gives you that Rocq does not

The TLC run is **executable** in CI: every push re-runs the model.
A future regression that breaks the property surfaces *immediately*
rather than waiting for someone to re-execute the Rocq proof. The
Rocq proof is *correct forever*; the TLC run *defends the model from
specification drift*.

### 4.5 What this gives you that proptest does not

TLC visits *every* state up to the bound; proptest samples uniformly
at random. For state spaces with sparse witnesses, TLC will surface
the witness on iteration `n`; proptest might never find it in a
budget of `n` samples. The Bug #2 witness specifically lives at the
intersection of `Thread_A.read = Thread_B.read` and a particular
hash insertion order; the probability of finding it under uniform
random sampling is below the noise floor.

---

## 5 · Concurrency and the `ConcurrentTracker` toggle

The lock-free tracker bug (Bug #2,
[`../design/09-bug-fixes-and-rationale.md §9.3`](../../design/09-bug-fixes-and-rationale.md))
is the canonical example of *“you cannot find this with single-threaded
tests”*. The TLA⁺ model `ConcurrentTracker.tla` captures the
essence:

```tla
\* Pre-fix: lock-free read-modify-write
LockFreeUpdate(t, v, h) ==
    /\ ~ Locked
    /\ existing == IF v \in DOMAIN tracker THEN tracker[v] ELSE {}
    /\ tracker' = [tracker EXCEPT ![v] = @ \cup {h}]
    \* race: another thread can interleave between the read above
    \*       and the write here.

\* Post-fix: locked update (atomic RMW)
LockedUpdate(t, v, h) ==
    /\ Locked
    /\ tracker' = AtomicRMW(tracker, v, h)
```

The invariant the lock-free version violates is:

```tla
Inv_NoOverwrite ==
    \A v \in DOMAIN tracker :
      \A h \in HashesInserted(v) :
        h \in tracker[v]
```

The violation TLC produces is a 4-step trace:

```
Step 1: Thread_A reads tracker[v] = ∅
Step 2: Thread_B reads tracker[v] = ∅
Step 3: Thread_A writes tracker[v] = {h_A}
Step 4: Thread_B writes tracker[v] = {h_B}    (* lost: h_A *)
```

This trace is the **direct cause** of the Rust regression test
[`casper/tests/slashing/loom_t_9_2_atomic_record.rs`](../../../../../casper/tests/slashing/loom_t_9_2_atomic_record.rs)
which reproduces the same race in actual Rust under Loom; see
[`../randomized-search/04-concurrency-interleaving-loom.md`](../randomized-search/04-concurrency-interleaving-loom.md)
for the Loom side of the story.

### 5.1 Diagrammatic form

Diagram 02 in [`../diagrams/`](../diagrams/) shows the
witness-to-promotion flow for this bug. The lock-free trace becomes
a Loom test becomes a Rocq theorem becomes a Rust regression — all
four artifacts grounded in the same four-step TLC counterexample.

---

## 6 · Pitfalls

### 6.1 Pitfall: the abstraction lies

A TLA⁺ model is a faithful description of the protocol *as the author
understood it*. If the abstraction omits a relevant aspect of the
implementation, the model can verify properties that the
implementation violates.

**Mitigation**: every TLA⁺ model in this development has a Rust
**trace replay** test
([`casper/tests/slashing/tla_trace_replay.rs`](../../../../../casper/tests/slashing/tla_trace_replay.rs))
that drives the production code through a sequence of TLC-emitted
states. If the production code rejects the trace, the model is wrong
about the abstraction; the model is then refined. The replay tests
re-execute on every CI run.

### 6.2 Pitfall: forgetting fairness

Liveness invariants of the form `P ~> Q` require fairness assumptions
on the relevant actions. A model without `WF_vars(Action)` will
admit traces where the action is enabled forever but never taken,
which spuriously falsifies `P ~> Q`.

**Mitigation**: every liveness invariant in the slashing models is
qualified by a `WF` or `SF` clause; the relationship is documented
inline in the `.tla` source.

### 6.3 Pitfall: state-space explosion goes silent

TLC reports state counts but does not warn when the runtime exceeds
the operator's expectation. A check that should take 30 seconds but
is running for 30 minutes likely has a parameter set too high.

**Mitigation**: each `MC_*.cfg` file in
[`formal/tlaplus/slashing/`](../../../../../formal/tlaplus/slashing/) has
calibrated parameters such that the state space is `≤ 10⁵` and TLC
runs in `≤ 60 s` on a 12-core machine. CI fails the check if the run
exceeds a configurable wall-clock budget.

### 6.4 Pitfall: weak `Init`

A `Init` predicate that overconstrains the starting state can hide
bugs that are reachable from a more permissive starting state.

**Mitigation**: every `Init` in the slashing models permits the
**most general** starting configuration consistent with the
protocol's well-formedness rules — empty DAGs, no records, no
detections. Any test of "what if the system has already done X?"
appears as a `Next` action sequence, not as a hand-crafted `Init`.

---

## 7 · Promotion to Rocq and back-promotion to Rust

A TLC pass on a bounded instance is *not* a Rocq theorem; the bound
is the difference. The slashing methodology has explicit rules for
when a TLC result earns its way upstream.

### 7.1 The promotion ladder

```
                       ┌────────────────────────┐
                       │ TLC: exhaust bounded   │
                       │ instance with B states │
                       └───────────┬────────────┘
                                   │
                ┌──────────────────┼──────────────────┐
                │                  │                  │
                ▼                  ▼                  ▼
        ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐
        │ Apalache     │  │ Rocq theorem │  │ Rust regression  │
        │ widen domain │  │ unbounded    │  │ trace replay     │
        └──────────────┘  └──────────────┘  └──────────────────┘
```

The methodology rule is:

> A property holding under TLC on bound `B` is admissible as a
> **specification clause** but not as a theorem. Promotion to
> a Rocq theorem requires either an Apalache pass on a domain large
> enough to subsume any *plausible production parameter* or an
> unconditional Rocq proof.

For the slashing development, the BFT bound, two-level closure
termination, and detector soundness are all Rocq theorems (the
TLC pass is a corroborating sanity check); the rebond-stale-evidence
property is a TLC-checked invariant promoted to Rocq via
`BugFixSlashAuthorization.v` on the strength of the Rocq proof
alone — Apalache is the prescribed domain-widening fallthrough
but has not yet been exercised against this or any other property
(see the status note in §2 above).

### 7.2 Back-promotion to Rust

Every TLC counterexample that survives the witness-rule pipeline
becomes a regression test in
[`casper/tests/slashing/tla_trace_replay.rs`](../../../../../casper/tests/slashing/tla_trace_replay.rs).
The replay file contains JSON traces dumped by `scripts/ci/dump-tla-traces.sh`
from TLC runs. The replay then drives the production Rust path through
the trace; any divergence between the TLA⁺ model and the production
behavior triggers a test failure that the engineer can either
classify as a model-refinement task or a Rust source bug.

This **back-promotion** is what closes the loop between the
abstract TLA⁺ model and the concrete Rust system; without it the
TLA⁺ work risks drifting from the implementation.

---

## 8 · Related work

- Lamport's foundational papers [Lam94, Lam02] introduce TLA⁺ and its
  semantics.
- Yu *et al.* [Yu99] describe TLC.
- Konnov *et al.* [KKT19, KKT20] describe Apalache.
- For distributed-system TLA⁺ verification practice, see
  AWS's published case studies [Newcombe14] and Microsoft Cosmos DB's
  TLA⁺ work [Cha18]; both inspired the testing-as-engineering culture
  used in this development.

DOIs in [`../references.md`](../references.md).

---

## 9 · Next chapter

[`03-symbolic-rust-kani.md`](./03-symbolic-rust-kani.md) — bounded
model checking of **actual Rust code**, not an abstract model. Kani
covers the boundary-arithmetic and authorization-predicate properties
where neither Rocq nor TLA⁺ is the natural fit.
