# 03 · Evidence stacking

> *“No theory survives contact with the data.”* — Adaptation of
> Helmuth von Moltke the Elder's military maxim, c. 1880 [VonM80].
>
> *“A single experiment can prove me wrong.”* — Albert Einstein, in
> response to *Hundert Autoren gegen Einstein*, 1931 [Ein31].

This chapter explains the **composition rule** for evidence. A
single tool's verdict is weak; *multiple* tools agreeing on a
property is the methodology's gold-standard evidence pattern. The
chapter formalizes the composition.

Organization:

- [§1 — The pattern in one paragraph](#1--the-pattern-in-one-paragraph)
- [§2 — The stacking taxonomy](#2--the-stacking-taxonomy)
- [§3 — Examples — three properties, three stacks](#3--examples--three-properties-three-stacks)
- [§4 — How much stacking is enough?](#4--how-much-stacking-is-enough)
- [§5 — Pitfalls](#5--pitfalls)
- [§6 — Related work](#6--related-work)

---

## 1 · The pattern in one paragraph

For every load-bearing property `φ` in the slashing specification,
the methodology requires evidence from:

1. **At least one mechanized verifier** — Rocq (preferred) or TLA⁺
   (for finite-state checks).
2. **At least one randomized layer** — proptest, Hypothesis,
   libFuzzer, Loom, or Sage exhaustive search.
3. **At least one differential or metamorphic comparison** — triple-
   bisim, differential Rust-vs-Scala, or a metamorphic relation.
4. **A traceability ledger entry** classifying any divergence among
   the above.

The four-layer requirement is *cumulative*: a property with all four
layers is `four-stacked` and is the strongest evidence the
methodology admits. A property with fewer layers is weaker; the
methodology marks it as such.

---

## 2 · The stacking taxonomy

The methodology defines five stacking depths, in increasing order of
strength:

| Stack depth     | Evidence layers                                                                                  | When acceptable                                                  |
|-----------------|--------------------------------------------------------------------------------------------------|------------------------------------------------------------------|
| **1-stacked**   | One layer only (e.g. a single proptest property)                                                 | Exploratory only; never load-bearing                              |
| **2-stacked**   | One mechanized + one randomized                                                                  | Internal helpers; non-load-bearing utilities                     |
| **3-stacked**   | One mechanized + one randomized + one of {differential, metamorphic, ledger}                     | Most load-bearing properties                                      |
| **4-stacked**   | One mechanized + one randomized + one differential/metamorphic + one ledger entry                | Headline properties (T-1, T-2, T-11, T-12, T-15)                  |
| **5-stacked**   | 4-stacked + a TLA⁺ trace-replay test that re-executes the property on production state            | Bisimilarity (T-15a/b); the headline theorem                      |

The slashing development's headline theorems — bisimilarity, BFT
bound, detector soundness/completeness, two-level closure
termination — are all **5-stacked**. Internal helpers (e.g.
arithmetic boundary functions) are typically **3-stacked** (Kani +
libFuzzer + Rocq).

### 2.1 Why stacking is not double-counting

A common objection: *“if Rocq proves `φ`, what does an additional
proptest tell us?”*

The answer is *engineering risk*, not *mathematical certainty*. The
Rocq theorem is correct *for all time* given its definitions; the
proptest is correct *now* given the running code. The two evidence
layers protect against different failure modes:

| Failure mode                                                            | Caught by                                       |
|-------------------------------------------------------------------------|-------------------------------------------------|
| The theorem is mathematically wrong                                     | Re-execution of the Rocq proof                  |
| The theorem's *definitions* drift from the running code                  | proptest (and triple-bisim)                     |
| The running code regresses                                              | proptest, libFuzzer, integration test           |
| The harness (which `prop_t_*` uses) drifts from the production path      | Triple-bisim driver                              |
| The proto encoding regresses                                             | libFuzzer round-trip                             |
| Concurrency interleaving regresses                                       | Loom + TLA⁺                                      |
| The bug recurs after a refactor                                          | Pre-fix regression test                          |

Each evidence layer catches a *different* failure mode; the stack
catches the *intersection*.

### 2.2 The single-tool fallacy

The methodology explicitly rejects the *single-tool fallacy* — the
belief that "if Rocq says so, it's true" or "if it passes the
fuzz tests, it's safe". Each tool is correct **only on its
domain** and **only modulo its trust base**:

| Tool       | Trust base                                                | Domain                                                       |
|------------|-----------------------------------------------------------|--------------------------------------------------------------|
| Rocq       | Kernel + stdlib                                            | Definitions inside the development                            |
| TLA⁺ TLC   | TLC binary + the model                                     | Finite bounded instance                                       |
| Kani       | CBMC + SMT solver                                          | Bounded primitive domain                                       |
| Sage       | Sage codebase                                              | Exact finite computation                                       |
| proptest   | Rust toolchain + the harness                                | Random samples within a strategy                              |
| Hypothesis | Python interpreter + the engine                            | Random samples of stateful traces                              |
| libFuzzer  | Compiler + libFuzzer                                       | Byte-level input space                                         |
| Loom       | Loom + Rust toolchain                                      | Schedule space at thread count `N`                            |

If the property holds in every domain on which it is checked, *and*
each tool's trust base is sound on its domain, *then* the property
holds in the union of domains. Stacking is the way to extend the
union to cover the *production* domain.

---

## 3 · Examples — three properties, three stacks

### 3.1 Detector soundness (T-1) — 5-stacked

```
Property φ₁ = ∀ honest v. ¬ is_slashable(classify(v))
   ├── Rocq theorem: t_1_detection_sound
   │   (EquivocationDetector.v, ~85 lines)
   ├── TLA+ invariant: Inv_DetectionSound
   │   (EquivocationDetector.tla)
   ├── proptest property: prop_t_1_detection_sound.rs
   ├── Triple-bisim: prop_t_triple_bisim_dispatch.rs
   ├── TLA+ trace replay: tla_trace_replay.rs
   └── Ledger entry: slashing-traceability.md "T-1"
```

5 layers; production-domain coverage is the union of the harness,
the oracle, and the production adapter, all checked against the
same Rocq theorem.

### 3.2 Arithmetic boundary `checked_base_seq` (Bug #15) — 3-stacked

```
Property φ₂ = ∀ s ≤ 0. checked_base_seq(s) = None
              ∀ s > 0. checked_base_seq(s) = Some(s − 1)
   ├── Kani harness: checked_base_seq_rejects_nonpositive
   │   + checked_base_seq_matches_positive_i32_predecessor
   ├── libFuzzer target: slashing_arithmetic
   ├── proptest: implicit through prop_t_9_7_seqnum_density.rs
   └── (no separate ledger entry; covered by Bug #15 in design/09)
```

3 layers. Lower depth than T-1 because this is a per-function
property (Kani exhausts the domain), not a system-wide property.

### 3.3 Bisimilarity (T-15) — 5-stacked

```
Property φ₃ = Rust LTS ≈_weak Scala LTS modulo {Bugs #1..#16}
   ├── Rocq theorem: main_bisimilarity_theorem (MainTheorem.v, ~617 lines)
   ├── TLA+ invariant: (per-projection invariants across all 7 models)
   ├── proptest properties: prop_t_14_weak_barbed_equiv.rs +
   │                          prop_t_15_bisim_under_workload.rs +
   │                          prop_t_triple_bisim_{records,dispatch,forkchoice}.rs
   ├── Differential corpus: hypothesis_rust_differential_corpus.rs
   ├── Sage differential model: differential_bisimilarity_model.sage
   └── Ledger entry: slashing-traceability.md "T-13a/b/c, T-14, T-15a/b"
```

5 layers, with the differential and metamorphic arms providing the
strongest possible operational corroboration of the unbounded Rocq
theorem.

---

## 4 · How much stacking is enough?

The methodology's rule:

> A property is stacked to depth `k` if `k` independent evidence
> layers all corroborate it. The depth required for a load-bearing
> property is at least 3; for a headline theorem, at least 5.

### 4.1 The marginal cost of stacking

The methodology pays for stacking explicitly. Each layer's cost is
small individually but additive:

| Layer       | Cost per property                          | When marginal                                                |
|-------------|---------------------------------------------|--------------------------------------------------------------|
| Rocq theorem| Hours to days                               | Property is unbounded and stable                              |
| TLA⁺ invariant| Hours                                    | Property is bounded finite-state                              |
| Kani harness| Minutes                                     | Property is per-function on primitives                        |
| Sage model  | Hours                                        | Property is exact finite-state with combinatorial structure   |
| proptest    | Minutes                                     | Property is a one-step property                                |
| Hypothesis  | Minutes to hours                            | Property is a multi-step lifecycle                            |
| libFuzzer   | Minutes (setup) + CI time                   | Property is byte-level / structure-aware                      |
| Loom        | Minutes                                     | Property is concurrency-related                                |
| Triple-bisim| Minutes (after harness exists)              | Property is observable across implementations                  |
| Differential| Minutes (after corpus exists)               | Property is cross-implementation                              |

For a headline theorem, the total stacking cost is **measured in
days** — small compared to the cost of a missed bug in production.

### 4.2 The discipline

Engineers writing a new theorem are required to *plan the stack
first*: which layers will corroborate this property, and which is
the primary (mechanized) layer? The plan is recorded in
[`../extending.md`](../extending.md) as a checklist; the PR adding
the theorem must satisfy the checklist.

---

## 5 · Pitfalls

### 5.1 Pitfall: counting "different proptests for the same property" as multiple layers

Two proptests are the same evidence layer; they don't compose. The
stack depth requires layers from *different* evidence kinds (Rocq vs.
TLA⁺ vs. Loom vs. proptest, etc.).

**Mitigation**: the stack-depth check in the methodology counts
*distinct evidence kinds*, not file counts.

### 5.2 Pitfall: stacking on top of a stale layer

If a Rocq theorem is broken or admitted, the stack collapses. The
methodology requires `Print Assumptions` to return *“Closed under
the global context”* for every theorem in the stack.

**Mitigation**: CI runs `Print Assumptions` on the headline theorems
and fails the build if any depends on an axiom or admitted lemma.

### 5.3 Pitfall: layers with overlapping trust bases

Two tools that share a trust base (e.g. two TLA⁺ models both checked
by the same TLC binary) provide weaker evidence than two tools with
distinct trust bases (e.g. one TLA⁺ and one Rocq theorem). The stack
requires *distinct* trust bases for full credit.

**Mitigation**: the stacking taxonomy in [§2](#2--the-stacking-taxonomy)
weights layers by trust-base independence; layers with overlapping
trust bases count fractionally.

### 5.4 Pitfall: forgetting to stack the *negative* evidence

A property may be `not_reproduced_in_rust` (negative finding); the
ledger entry is itself a piece of evidence. The methodology counts
the ledger entry as a layer.

**Mitigation**: every property's stack count includes the ledger
entry; this is the *audit-trail* layer.

---

## 6 · Related work

- **N-version programming**: Avizienis & Lyu [AvLi77].
- **Diversity in software engineering** (theoretical foundations of
  stacking): Knight & Leveson [KL86].
- **Layered assurance arguments** in safety-critical systems: Bishop
  & Bloomfield [BB98] (the Adelard ASCAD methodology).
- **Defense in depth** (originally a security concept; widely applied
  to assurance): Anderson [And08].

DOIs in [`../references.md`](../references.md).

---

## 7 · Next chapter

[`../case-studies/README.md`](../case-studies/README.md) — the
**case-study index**. Each of the 16 bug fixes is documented as a
worked example of the methodology in action: which tool emitted the
witness, which classification it received, which artifacts protect
against regression.
