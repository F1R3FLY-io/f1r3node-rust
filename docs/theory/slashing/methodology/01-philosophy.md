# 01 · Philosophy — Why this methodology

> *“It is a capital mistake to theorize before one has data.”* —
> Sherlock Holmes, in A. Conan Doyle, *A Scandal in Bohemia*, 1891.
>
> *“The validation of a [scientific] hypothesis can be considered in
> terms of falsifiability.”* — Karl Popper, *The Logic of Scientific
> Discovery*, 1959 [Pop59].

This chapter answers four foundational questions:

1. [§1](#1--what-is-a-bug-here) — What is a bug, in a consensus-layer
   slashing subsystem?
2. [§2](#2--why-not-just-tests) — Why not just write tests?
3. [§3](#3--the-scientific-method-applied-to-bug-hunting) — How does
   the scientific method apply to bug-hunting in a distributed system?
4. [§4](#4--the-witness-rule) — Why the witness rule (the single most
   important rule of this methodology)?

Every other chapter in this directory rests on the answers below.

---

## 1 · What is a bug here?

In a distributed slashing subsystem we use a sharper definition than
the colloquial *“the code does the wrong thing.”* A **slashing bug**
is any of the following four classes:

| Class                 | Symbol | Definition                                                                                          | Example                                                                             |
|-----------------------|--------|-----------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------|
| **Soundness bug**     | 𝖡ₛ     | The slashing pipeline punishes an honest validator                                                  | Detector mis-classifies a duplicate-justification view as equivocation              |
| **Completeness bug**  | 𝖡𝖼     | A real equivocator is never slashed under fair scheduling                                           | Detector traversal silently returns ∅ on a missing latest-message pointer (Bug #11) |
| **Liveness bug**      | 𝖡ℓ     | The slashing pipeline halts or fails to make progress under permitted inputs                        | `epoch_for_block_number` panics when `epoch_length = 0` at startup                  |
| **Authorization bug** | 𝖡ₐ     | An attacker can cause a slash, or suppress a slash, without controlling the validator's signing key | Received `SlashDeploy` accepted without local re-validation (Bug #12)               |

Two derived classes appear in the threat model but are *not* in scope
for the source-code search:

- **Economic bug** (𝖡ₑ) — a rational adversary can profit by
  triggering the slashing protocol within its specification.
- **Implementation projection bug** (𝖡ᵢ) — a Rust function deviates
  from its Rocq/TLA⁺ model in a way that does not currently reproduce
  but could under bounded shifts of parameters.

The methodology distinguishes these classes because the **right tool
differs for each one**. A soundness bug is best found by a Rocq
theorem (*∀ honest validator v, ¬is_slashable(observed_status(v)*).
A liveness bug is best found by TLC under a weak-fairness hypothesis.
A concurrency bug is best found by Loom or TLA⁺. A boundary
arithmetic bug is best found by Kani or libFuzzer. The decision tree
appears in [`02-glossary-and-notation.md §1`](./02-glossary-and-notation.md).

---

## 2 · Why not just tests?

Unit and integration tests are necessary but provably insufficient for
this subsystem. Three observations together force the conclusion.

### 2.1 The state space is astronomical

Let `n` be the number of validators, `d` the maximum DAG depth, `b`
the maximum blocks per sequence number, and `r` the number of report
rounds. The state space of the abstract slashing model has
cardinality:

```
|𝒮| ≈ b^(n·d) · 2^(n²) · r^n
```

For modest parameters (`n=4, d=10, b=2, r=4`) we get
`|𝒮| ≈ 2⁴⁰ · 65 536 · 256 ≈ 1.85 × 10¹⁷` reachable abstract states.
No exhaustive unit test suite can cover even a millionth of this.

### 2.2 The interesting bugs hide in *interleavings*, not in *paths*

Bug #2 (the lock-free tracker race) does not require an unusual block
shape, an unusual signature, or an unusual proposer. It requires two
threads to read the same key at the same instant. A single-threaded
unit test will never see it, regardless of how many paths it executes.
The witness lives in the cross-product of `Threads × Schedules`, not
in the input space alone.

### 2.3 Bisimilarity is not testable; it is *provable*

The headline claim of the slashing port — *“the Rust implementation
is observationally equivalent to the Scala original, modulo a closed
set of sixteen documented bug fixes”* — is a statement about *all
possible* executions of both systems. A test can falsify it (by
exhibiting a divergent trace), but no finite test suite can establish
it. Bisimilarity (Theorem T-15a/b, see
[`../slashing-verification.md §8`](../slashing-verification.md))
demands a proof, not a test.

### 2.4 What tests *are* for

The slashing subsystem still has 200+ tests in
[`casper/tests/slashing/`](../../../../casper/tests/slashing/). Each one
plays one of four roles, and *only* those four roles:

1. **Anchor** — A use-case test (`uc_NN_*.rs`) freezes the production
   behavior of an interesting trace so refactors cannot silently drift.
2. **Regression** — A pre-fix bug test (`pre_fix_bug_N.rs`) replays a
   known historical witness so a regression cannot reintroduce the
   bug.
3. **Witness reproduction** — An integration test
   (`integration_t_*.rs`) replays a Sage/Hypothesis-generated witness
   on the production code path to confirm it does or does not
   reproduce.
4. **Property check** — A property-based test (`prop_t_*.rs`) samples
   the input space *to falsify the property*; it does not prove it.

Anything else — “coverage” numbers, “smoke tests” without
properties, golden-file comparisons of opaque blobs — is forbidden by
the methodology. Such tests pass without informing the reader about
the system's correctness.

---

## 3 · The scientific method applied to bug-hunting

The slashing search program follows the standard
hypothesize-experiment-falsify-revise loop. Diagram 03 in
[`diagrams/`](./diagrams/03-scientific-method-loop.svg) illustrates
the cycle.

### 3.1 The loop in literate form

```
algorithm SearchLoop:
    state ← (ledger : TraceabilityLedger,
              models : RocqModules ⊎ TLAModules,
              fixtures : DeterministicReplays)

    loop forever:
        ▸ 1.  Hypothesis ← derive_candidate_property(state.models)
        ▸ 2.  Tool       ← choose_tool(Hypothesis)                  # §3.2
        ▸ 3.  Result     ← Tool.search(Hypothesis)
        ▸ 4.  match Result:
              | Refuted(witness)    → § 3.3 — classify, ledger.append
              | NotRefuted(bound)   → § 3.4 — strengthen or retire
              | Inconclusive(cost)  → re-scope Hypothesis or pick another Tool
        ▸ 5.  if classification ∈ {confirmed_current_bug,
                                   confirmed_fixed_bug,
                                   projection_risk_guarded}:
                  fix source ∨ formalize guard ∨ add regression fixture
        ▸ 6.  promote witness → mechanized artifact (Rocq theorem or
                                 TLA⁺ invariant) when applicable
```

The loop is **monotone**: every iteration either expands `ledger`
(new finding), expands `models` (new theorem/invariant), or expands
`fixtures` (new replayable trace). The loop never deletes evidence;
it only re-classifies it.

### 3.2 Picking the tool — the decision tree

Given a candidate property `φ`, the choice of tool is constrained by
the shape of `φ`. The tree below is the heuristic the slashing team
applied; it is not normative, but it has been load-bearing for the
sixteen bug fixes.

The methodology has **two orthogonal arms**: a *formal-methods*
arm (mechanized proof or exhaustive model checking) and a
*randomized-search* arm (sampling-based falsification). For any
load-bearing property the two arms are run **in parallel** to
stack evidence (see
[`pipeline/03-evidence-stacking.md`](./pipeline/03-evidence-stacking.md));
neither arm is a fallthrough of the other.

**(a) Formal-methods arm** — pick one by φ's shape:

```
                                  φ
                                  │
            ┌─────────────────────┼─────────────────────┐
            │                     │                     │
       is φ semantic?       is φ over a            is φ about
       (∀ honest v …)       finite bounded         a single Rust
                            protocol?              function?
                            (≤ 10⁶ states)
            │                     │                     │
            ▼                     ▼                     ▼
        ┌───┴───┐             ┌───┴──┐              ┌───┴────┐
        │ Rocq  │             │ TLA⁺ │              │ Kani   │
        └───────┘             │ +TLC │              └────────┘
        prove                 └───┬──┘              prove
        unconditionally           │                 unconditionally
                                  │                 on bounded input
                                  ▼
                          is the bound
                          too tight for TLC?
                                  │
                                  ▼
                          ┌───────┴────────┐
                          │ Apalache       │
                          │ (symbolic TLA⁺)│
                          └────────────────┘
                          planned fallthrough;
                          not yet exercised
                          against a finding
                          (see formal-methods/02
                           §2 for status).
```

**(b) Randomized-search arm** — pick by φ's shape; run in parallel
with whichever formal-methods tool was chosen above:

```
                                  φ
                                  │
       ┌──────────────┬───────────┴───────────┬──────────────┐
       │              │                       │              │
   is φ a one-    is φ a multi-          is φ at a       is φ about a
   step data      step lifecycle?        byte-level      concurrency
   property?                             parser/proto?   interleaving?
       │              │                       │              │
       ▼              ▼                       ▼              ▼
   ┌───┴─────┐    ┌───┴────────┐         ┌────┴─────┐    ┌───┴─────┐
   │ proptest│    │ Hypothesis │         │libFuzzer │    │  Loom   │
   └─────────┘    └────────────┘         └──────────┘    └─────────┘
```

### 3.3 Refuted — the witness has work to do

When `Tool` produces a counterexample `w`, the witness is **not yet a
bug**. It must travel the classification pipeline of
[`pipeline/01-witness-to-source-rule.md`](./pipeline/01-witness-to-source-rule.md).
Eight statuses are possible (see
[`pipeline/02-classification-taxonomy.md`](./pipeline/02-classification-taxonomy.md));
only three of them require source changes.

### 3.4 Not refuted — when to strengthen, when to retire

Failure to find a counterexample is **not proof**. It is evidence of
absence only to the depth of the search. The methodology applies one
of three follow-ups:

1. **Strengthen the hypothesis** — narrow the precondition; assert a
   stronger consequence; this often produces a *better* theorem.
2. **Expand the search** — increase the bound (validator count, DAG
   depth, libFuzzer runs) until cost dominates.
3. **Retire the search** — record in the traceability ledger that
   the search returned no counterexample under bounds `B`; this is
   permanent evidence that informs future audits.

### 3.5 The bookkeeping requirement

> *“Records … must be kept so that any other person of similar
> training and skill, after consulting them, may continue the work or
> verify the results.”* — Joint Commission on Powers of Reference,
> 1893, quoted in Holmes, *On Beyond Zebra*, 1955.

The methodology mandates that **every** execution of the loop above
leaves a permanent artifact in one of:

- `formal/rocq/slashing/theories/*.v` — for promoted theorems,
- `formal/tlaplus/slashing/*.tla` — for promoted invariants,
- `formal/sage/slashing/FINDINGS.md` — for Sage exact witnesses,
- `casper/tests/slashing/pre_fix_bug_N.rs` — for replayable bug
  regressions,
- `casper/tests/slashing/uc_NN_*.rs` — for use-case anchors,
- [`../slashing-traceability.md`](../slashing-traceability.md) — for
  the finding's classification and source fate.

Re-running the loop without leaving a trace is **forbidden** by the
methodology; the cost of doing so is paid back tenfold the next time
the audit asks *“is this still true?”*.

---

## 4 · The witness rule

> **Witness rule**: A generated witness is **not** a Rust
> vulnerability unless it is reproduced on the production Rust path
> *or* contradicts a production-path invariant.

This single rule prevents the most common pathology in
property-driven verification — over-claiming.

### 4.1 The shape of the pathology

A Sage model of `n=4, b=2` validators with a hand-coded slash
function may exhibit an interesting behavior — say, a particular
quorum-loss witness. A naïve workflow concludes *“the slashing
protocol has a quorum-loss bug.”* But the witness lives in the
**model**, not in the **system**. The Rust path may filter that case
out, may never reach it, or may construct the relevant data
differently. Promoting the witness to a bug without the Rust
traceability step is at best wasted work and at worst a false alarm
that erodes auditor trust.

### 4.2 The shape of the rule

Every Sage / Hypothesis / proptest / Kani / Loom counterexample must
flow through:

```
   ┌──────────┐    1. generate w (Sage/Hypothesis/proptest/Kani/Loom)
   │ Witness  │
   │   w      │
   └────┬─────┘
        │ 2. classify w under threat-model vocabulary
        ▼
   ┌──────────────────────┐
   │ Threat-model class   │     (bisimilar | permitted_bug_fix |
   │ from §4 of           │      candidate_boundary | projection_risk |
   │ slashing-threat-model│      assumption_counterexample | unexpected)
   └─────────┬────────────┘
             │ 3. trace into the production Rust path
             ▼
   ┌──────────────────────────────┐
   │ Rust source traceability     │
   │ from slashing-traceability.md│
   └─────────┬────────────────────┘
             │ 4. assign one of 8 outcome statuses
             ▼
   ┌──────────────────────────────────────────────┐
   │ confirmed_current_bug | confirmed_fixed_bug  │
   │ not_reproduced_in_rust | model_boundary      │
   │ projection_risk_guarded | assumption_counter │
   │ proof_or_model_strengthening | needs_audit   │
   └─────────┬────────────────────────────────────┘
             │ 5. ledger.append((w, class, status, action))
             ▼
   ┌────────────┐
   │ permanent  │
   │ artifact   │
   └────────────┘
```

The full pipeline lives in [`pipeline/`](./pipeline/). The visual
form is Diagram 02 in [`diagrams/`](./diagrams/02-witness-to-promotion.svg).

### 4.3 The rule in adversarial form

The witness rule has a dual that is often more useful when arguing
**against** a proposed change:

> A finding that has not been classified under both the threat-model
> vocabulary *and* the traceability ledger may not motivate a
> source-code change.

This rule is what prevents *over*-correction. The slashing port
encountered several model-only findings (e.g. zero-stake direct
offender; see [Sage finding #3](../../../../formal/sage/slashing/FINDINGS.md))
that, on first glance, looked like Rust bugs. Tracing them through
the production path showed the Rust code *cannot construct* the
predicate state in question — the witness was a model artifact, not
a vulnerability. The rule kept those findings out of the source.

---

## 5 · Epistemic taxonomy (for the rest of the directory)

The chapters that follow use the vocabulary defined in this section
without further comment.

| Term               | Definition                                                                                                                    |
|--------------------|-------------------------------------------------------------------------------------------------------------------------------|
| **Proof**          | A kernel-checked Rocq derivation, or a TLC exhaustion to bound `B` with `0` counterexamples                                   |
| **Witness**        | A counterexample emitted by any tool — Sage, Hypothesis, proptest, libFuzzer, Kani, Loom, TLC                                 |
| **Property**       | A formal predicate `φ : State → Bool` that the methodology aims to establish or falsify                                       |
| **Invariant**      | A property of the *trajectory* of the system that must hold at every reachable state                                          |
| **Hypothesis**     | An informal candidate property the engineer is asking the tools to confirm or refute                                          |
| **Refutation**     | A concrete witness that falsifies a hypothesis                                                                                |
| **Bound**          | A finite parameter (validator count, DAG depth, libFuzzer run count, Loom thread count) within which the search is exhaustive |
| **Promotion**      | The act of moving a witness from Sage/Hypothesis into a Rust regression, Rocq theorem, or TLA⁺ invariant                      |
| **Bisimilarity**   | Observational equivalence between two systems — *every* externally visible move of one can be matched by the other            |
| **Counterexample** | A particular kind of witness that establishes the negation of a hypothesis                                                    |
| **Boundary**       | A region of input space where the property is permitted to fail by explicit theorem precondition                              |
| **Projection**     | A mapping from the model state to the production state; projection risk is a potential mismatch                               |

The full alphabetical glossary lives in
[`02-glossary-and-notation.md`](./02-glossary-and-notation.md).

---

## 6 · Theoretical bases

Three intellectual traditions converge in this methodology. Each is
cited where it is first applied; the consolidated reference list is in
[`references.md`](./references.md).

### 6.1 Falsificationism

Karl Popper's argument [Pop59] that scientific propositions are
distinguished by their *falsifiability* is the epistemic foundation
of all randomized testing in this work. A property the tools *cannot
falsify* is admissible as a hypothesis; one they *can* falsify
becomes a discovered bug or boundary.

### 6.2 Formal methods and mechanization

The mechanization tradition — Coq/Rocq [CoqArt04], Isabelle/HOL
[NPW02], TLA⁺ [Lam02] — supplies the *unbounded* arm of the program.
Where randomized testing finds counterexamples cheaply, mechanization
finds *the absence of counterexamples* — a stronger claim, paid for
with proof effort.

### 6.3 Property-based and randomized testing

The QuickCheck tradition [CH00] is the cheap, fast arm. Sage [SageDev]
adds exact integer / combinatorial precision; Hypothesis [MMM19]
adds *shrinking* — the ability to minimize a long, complex
counterexample to its smallest reproducer.

The methodology's contribution is not to choose between these
traditions but to **compose** them, with the witness rule as the
arbitration mechanism between *witness* (cheap) and *theorem* (final).

---

## 7 · Next steps

- [`02-glossary-and-notation.md`](./02-glossary-and-notation.md) —
  every term, symbol, and acronym used throughout this directory,
  defined before first use.
- [`formal-methods/01-mechanized-proof-rocq.md`](./formal-methods/01-mechanized-proof-rocq.md)
  — the strongest arm of the program.
- [`pipeline/01-witness-to-source-rule.md`](./pipeline/01-witness-to-source-rule.md)
  — the witness rule, fully unpacked.
