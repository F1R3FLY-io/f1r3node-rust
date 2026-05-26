# Cost Accounting under Linear Logic: the DILL Substructural Layer and the Multiplicative Unit `1`

**Version:** 1.0
**Date:** 2026-05-25
**Authors:** Dylon Edwards, with formal-verification contributions from L. Gregory Meredith
**Status:** Implementation-aligned formal design document
**Scope:** The linear-logic additions to cost accounting — the runtime signature algebra `sig_algebra`, the Dual-Intuitionistic-Linear-Logic (DILL) two-zone fragment in `LinearLogicResources.v`, the channel-layer algebraic identities in `LLIdentities.v`, the multiplicative unit `1`, and their cross-checks in Sage, TLA⁺, and the Rust runtime.

The authority chain is: repo-local Rocq / TLA⁺ / Sage formal models, then this design document, then the `f1r3node-rust` implementation. Where this document and any publication draft disagree, the repo-local models and this document are authoritative. This document is a *companion* to the flagship proof, [*Formal Verification of Cost-Accounted Rho Calculus*](cost-accounted-rho-verification.md): that document proves that phlogiston accounting is faithfully encodable in the pure rho calculus; **this** document explains the linear-logic structure of the *authorization* algebra that decides how much fuel a deploy costs, and is the dedicated home for the linear-logic / DILL / unit-`1` work that the other cost-accounting design docs do not cover.

---

## Table of Contents

1. [Introduction and Motivation](#1-introduction-and-motivation)
2. [Glossary of Symbols and Terms](#2-glossary-of-symbols-and-terms)
3. [Linear Logic from First Principles](#3-linear-logic-from-first-principles)
4. [Dual Intuitionistic Linear Logic and the Two-Zone Sequent](#4-dual-intuitionistic-linear-logic-and-the-two-zone-sequent)
5. [The Runtime Signature Algebra and the Reflection Pipeline](#5-the-runtime-signature-algebra-and-the-reflection-pipeline)
6. [The Cost-Accounting Interpretation: resource = cost](#6-the-cost-accounting-interpretation-resource--cost)
7. [Channel-Layer Algebraic Identities](#7-channel-layer-algebraic-identities)
8. [Substructural Guarantees: No Double-Spend, No Free Weakening](#8-substructural-guarantees-no-double-spend-no-free-weakening)
9. [Multi-Modal Corroboration](#9-multi-modal-corroboration)
10. [Scope, Limitations, and Honesty](#10-scope-limitations-and-honesty)
11. [Cross-References and Further Reading](#11-cross-references-and-further-reading)
12. [References](#12-references)

---

## 1. Introduction and Motivation

### 1.1 The cost-accounting problem, in one paragraph

In the F1R3FLY blockchain a *deploy* is a unit of work submitted to the network, and **phlogiston** (informally "fuel", "phlo", or "gas") is the resource it spends as it reduces. Before a deploy may spend fuel, it must be *authorized* — historically by a single digital signature. The cost-accounted rho calculus [[8](#ref-8)] internalizes this: a deploy carries a *compound signature* describing exactly which signing authorities must contribute, and the runtime must compute, from that compound signature alone, **how many signature/fuel witnesses the deploy is obligated to supply**. The migration that introduces compound signatures, the `Sig`/`Token`/`SignedProcess` types, and the recursive metering kernel is documented in the companion [cost-accounting migration design](cost-accounting-migration.md); the proof that the whole scheme is faithfully encodable in the pure rho calculus is in the [verification companion](cost-accounted-rho-verification.md). This document concerns a single, sharp question that sits underneath both: **what algebra do compound signatures form, and what does that algebra say about cost?**

The answer is the subject of this document: compound signatures form a fragment of **intuitionistic linear logic with exponentials (ILLE)**, the obligation count is exactly a linear-logic *resource count*, and the single-use discipline of linear logic is exactly the *no-double-spend* guarantee the accounting system needs.

### 1.2 Why linear logic, for a process-calculus reader

A reader fluent in the π-calculus [[7](#ref-7)] or the reflective rho calculus [[6](#ref-6)] already accepts a resource discipline without calling it that: when a process `for(y ← x) P` receives a name on channel `x`, the message is *consumed* by that communication (COMM) — it is not silently duplicated, and it does not vanish unused. **Linear logic** [[1](#ref-1)] is the logic of exactly this discipline: a hypothesis must be used *exactly once*. The connection between linear-logic propositions and concurrent processes is not a loose analogy; Caires and Pfenning [[4](#ref-4)] showed that intuitionistic linear propositions correspond precisely to session types for π-calculus channels, with proof reduction matching process reduction. We exploit the same correspondence one level up: a compound *signature* is a linear-logic proposition, an individual cryptographic witness is a linear *hypothesis*, and "this deploy is authorized" is a linear *entailment* that consumes those witnesses.

### 1.3 The "resource = cost" through-line

The thesis that organizes this entire document is a single identification, made precise in [§5.6](#56-the-runtime-bridge-theorems) and [§6](#6-the-cost-accounting-interpretation-resource--cost) and mechanically proved in Rocq:

> A compound signature is a linear-logic formula. The **number of atomic witnesses that formula obligates** — its linear-logic *required-units* count — **is** the deploy's authorization cost. The single-use (linear) witnesses model spendable fuel that cannot be double-spent or conjured; the reusable (`!`-marked) witnesses model standing capabilities that are not consumed; and the **multiplicative unit `1`** is the zero-cost, zero-witness authorization that acts as the neutral element of the whole algebra.

### 1.4 What this document covers, and what it does not

This document covers five concrete artifacts, all on the `feature/cost-accounted-rho` branch:

| Artifact                       | File(s)                                                                                   | Role                                                                        |
|--------------------------------|-------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------|
| Runtime signature algebra      | `formal/rocq/cost_accounted_rho/theories/CostAccountedSyntax.v` (the `sig_algebra` block) | The 9-connective ILLE algebra + the cost function                           |
| DILL two-zone fragment         | `formal/rocq/cost_accounted_rho/theories/LinearLogicResources.v`                          | The `dill` sequent calculus, resource measures, no-double-spend theorems    |
| Channel-layer identities       | `formal/rocq/cost_accounted_rho/theories/LLIdentities.v`                                  | Equational laws incl. the unit laws and Mac Lane coherence                  |
| Bounded-exhaustive cross-check | `formal/sage/cost_accounting/ll_identity_search.sage`                                     | An independent reference model searched over hundreds of thousands of cases |
| Per-connective protocol models | `formal/tlaplus/cost_accounted_rho/{Plus,With,Bang,WhyNot,Lolly,Threshold}Protocol.tla`   | Finite-state model-checking of each connective's runtime protocol           |

It does **not** cover the operational *reduction* of authorized deploys (that lives in `RhoReduction.v` / `Bisimulation.v`), nor the on-chain `rho:system:capabilities` registry that implements bounded reuse and the lollipop transformer at runtime. Those are dynamics; this document is the *static* resource accounting that the dynamics presuppose.

### 1.5 Document roadmap

[§2](#2-glossary-of-symbols-and-terms) defines every symbol and term used below. [§3](#3-linear-logic-from-first-principles) teaches linear logic from first principles; [§4](#4-dual-intuitionistic-linear-logic-and-the-two-zone-sequent) introduces DILL and the `dill` relation. [§5](#5-the-runtime-signature-algebra-and-the-reflection-pipeline) presents the runtime `sig_algebra` and the reflection pipeline that turns it into a linear-logic formula and then a cost. [§6](#6-the-cost-accounting-interpretation-resource--cost) ties connectives to cost with a worked example. [§7](#7-channel-layer-algebraic-identities) and [§8](#8-substructural-guarantees-no-double-spend-no-free-weakening) present the two families of Rocq theorems (algebraic identities and substructural guarantees). [§9](#9-multi-modal-corroboration) shows how Sage, TLA⁺, and Rust corroborate the Rocq proofs — including a real bug the corroboration caught. [§10](#10-scope-limitations-and-honesty) states the limitations precisely.

---

## 2. Glossary of Symbols and Terms

*Every symbol, connective, and term used in this document appears in the tables below before its first use; later sections assume these definitions.* Throughout, `σ`, `τ`, `ρ` range over signature formulas and `a`, `b`, `c` over atomic signatures.

### 2.1 Linear-logic connectives and the unit

The fourth column — the **cost reading** — is this document's central contribution and is justified formally in [§5](#5-the-runtime-signature-algebra-and-the-reflection-pipeline)–[§6](#6-the-cost-accounting-interpretation-resource--cost). "Witness" means one atomic signature / one fuel unit.

| Symbol | Name | Informal reading | Cost (witnesses required) |
|:------:|------|------------------|---------------------------|
| `1` | multiplicative **unit** | the trivial requirement; "no authorization needed" | `0` |
| `⊗` | **tensor** (multiplicative conjunction) | *both* `σ` and `τ`, using disjoint witnesses | `cost(σ) + cost(τ)` |
| `⊸` | **lollipop** (linear implication) | consume a `σ` to obtain a `τ` | `cost(σ) + cost(τ)` |
| `&` | **with** (additive conjunction) | the *verifier* may project either branch; both must be available | `cost(σ) + cost(τ)` |
| `⊕` | **plus** (additive disjunction) | the *signer* commits to exactly one branch | `cost(chosen branch)` |
| `!` | **of-course** / **bang** (exponential) | a *reusable* `σ` (unbounded uses) | `cost(σ)` |
| `?` | **why-not** (exponential) | an *optional* `σ` (zero-or-more uses) | `0` |
| `⊢` | **turnstile** (entailment) | "… derives / authorizes …" | — |
| `≡` | channel **equivalence** | equality of the reflected witness multiset (see §2.5) | — |
| `∎` | QED | end of a proof | — |

The **threshold** (`k`-of-`N` quorum) connective, written informally `Threshold(k, [σ₁ … σₙ])`, is not a textbook linear-logic connective; it is a primitive of this development (see [§5.1](#51-the-sig_algebra-type)) with `cost = k`.

### 2.2 Sequent and context notation

| Symbol | Name | Meaning |
|:------:|------|---------|
| `Γ` (Gamma) | **unrestricted** context / **zone** | a list of *reusable* hypotheses; admits weakening and contraction (defined in §2.6) |
| `Δ` (Delta) | **linear** context / **zone** | a list of *single-use* hypotheses; admits neither weakening nor contraction |
| `Γ ; Δ ⊢ A` | **DILL sequent** | "with reusable capabilities `Γ` and spendable witnesses `Δ`, the goal `A` is derivable" |
| `Δ₁, Δ₂` (also `Δ₁ ⊎ Δ₂`) | context **split** | multiset union; the linear zone partitioned across premises |
| `·` | the **empty** context | no hypotheses (the linear analogue of the unit `1`) |

### 2.3 The runtime signature algebra: Rocq ↔ Rust ↔ cost

The Rocq inductive `sig_algebra` (`CostAccountedSyntax.v:229`), the runtime Rust enum `Sig` (`accounting/mod.rs:821`), and the wire-format `SigCompound` proto are three views of the same algebra. The cost column is the Rocq fixpoint `sig_algebra_min_required` (`CostAccountedSyntax.v:253`).

| Rocq `sig_algebra` | Rust `Sig` | Wire `Connective` | LL connective | `sig_algebra_min_required` |
|--------------------|-----------|-------------------|:-------------:|----------------------------|
| `ASUnit` | `Sig::Unit` | `Atom` (empty) | `1` | `0` |
| `ASHash a` | `Sig::Hash(b)` | `Atom` | atom | `1` |
| `ASAnd s₁ s₂` | `Sig::And` | `Tensor` | `⊗` | `min(s₁) + min(s₂)` |
| `ASWith s₁ s₂` | `Sig::With` | `With` | `&` | `min(s₁) + min(s₂)` |
| `ASPlus c s₁ s₂` | `Sig::Plus` | `Plus` | `⊕` | `min(chosen branch)` |
| `ASBang s` | `Sig::Bang` | `Bang` | `!` | `min(s)` |
| `ASWhyNot s` | `Sig::WhyNot` | `Whynot` | `?` | `0` |
| `ASLolly s₁ s₂` | `Sig::Lolly` | `Lolly` | `⊸` | `min(s₁) + min(s₂)` |
| `ASThreshold k ms` | `Sig::Threshold{…}` | `Threshold` | `k-of-N` | `k` |

> **Naming note.** The Rust variant `Sig::And` is the linear-logic tensor `⊗`; the name `And` is retained for backward compatibility with the Phase-1 substrate, and the doc comment at `accounting/mod.rs:829` records that the `Tensor` rename is intentionally postponed to a separate coordinated PR. We write `⊗` throughout and treat `ASAnd` / `Sig::And` / `Tensor` as synonyms.

### 2.4 Object-level linear-logic formulas and their measures

These live in `LinearLogicResources.v`. `ll_formula` (`line 7`) is the object-level syntax mirroring the nine connectives (`LLUnit`, `LLAtom`, `LLTensor`, `LLPlus`, `LLWith`, `LLBang`, `LLWhyNot`, `LLLolly`, `LLThreshold`).

| Definition | Type | Meaning |
|------------|------|---------|
| `ll_required_units` | `ll_formula → ℕ` | the **cost**: minimum witnesses the formula obligates |
| `ll_available_slots` | `ll_formula → ℕ` | the **capacity**: how many witness positions exist |
| `ll_consumed_atoms` | `ll_formula → list ℕ` | the atoms *actually spent* (`⊕` takes only the chosen branch; `?` spends nothing) |
| `ll_atoms` | `ll_formula → list ℕ` | every atom appearing anywhere in the formula |
| `ll_valid` | `ll_formula → bool` | well-formedness (threshold bounds `1 ≤ k ≤ n`) |

### 2.5 The channel model

`LLIdentities.v` models a signature's *reflection* as a multiset of atomic-proposition identifiers.

| Definition | Meaning |
|------------|---------|
| `channel` = `list ℕ` | a multiset of atom ids (the reflected shape of a signature) |
| `channel_equiv c₁ c₂ ≝ Permutation c₁ c₂` | two channels are `≡` iff their multisets agree |
| `tensor_channel`, `plus_channel`, `with_channel`, `lolly_channel` | all = list concatenation `++` at the channel layer |
| `bang_channel`, `whynot_channel` | both = the identity on channels |
| `threshold_channel` | concatenation of all member channels |

### 2.6 Key terms

| Term | Definition |
|------|------------|
| **Structural rules** | the logical rules that let a hypothesis be duplicated, discarded, or reordered. The three are *weakening*, *contraction*, and *exchange*. |
| **Weakening** | discarding an unused hypothesis: from `Γ ⊢ B` infer `Γ, A ⊢ B`. Linear logic *rejects* it — a required witness may not be thrown away for free. |
| **Contraction** | duplicating a hypothesis: from `Γ, A, A ⊢ B` infer `Γ, A ⊢ B`. Linear logic *rejects* it — one witness may not be used twice. |
| **Exchange** | reordering hypotheses. Linear logic *keeps* it (our channels are multisets, so order is irrelevant). |
| **ILLE** | Intuitionistic Linear Logic with Exponentials — the single-conclusion linear logic with `!` and `?`. The fragment this development mechanizes. |
| **DILL** | Dual Intuitionistic Linear Logic [[2](#ref-2)] — a two-zone presentation of ILLE with a reusable zone `Γ` and a linear zone `Δ`. |
| **Dereliction** | the rule `!σ ⊢ σ`: a reusable resource may be used once. |
| **Signer's choice vs. verifier's choice** | `⊕` is decided by the party constructing the deploy (which branch they signed); `&` leaves the choice to the verifier (block proposer), so both branches must be available. |
| **Quorum / threshold** | a k-of-N requirement: any k of the N listed signers suffice. |
| **No-double-spend** | a single linear witness authorizes exactly one obligation; it cannot be consumed twice. |
| **No-free-weakening** | a presented-but-invalid witness cannot be silently ignored; the cost it claims to fund must actually be funded. |
| **Reflection layer** | the substrate step (`SignatureChannel::from_sig`, then `ParSortMatcher::sort_match`) that turns a `Sig` into a permutation-invariant rho-calculus channel. |
| **Monoidal coherence** | the pentagon and triangle diagrams [[5](#ref-5)] guaranteeing that all ways of reassociating `⊗` and cancelling `1` agree. |

---

## 3. Linear Logic from First Principles

This section assumes no prior linear logic. It builds the intuition that the cost interpretation rests on.

### 3.1 Formulas as resources

Classical and intuitionistic logic treat hypotheses as *facts*: once you know `A`, you know it forever and may use it as many times as you like. Linear logic [[1](#ref-1)] instead treats each hypothesis as a *resource* that is consumed by use. Girard's own illustration is a vending machine: from "one dollar" and "one dollar `⊸` one candy" you may obtain one candy, **and then you no longer have the dollar**. You cannot derive two candies, because you cannot duplicate the dollar; and you are not allowed to keep the dollar and walk away with it unused while still "owning" the candy.

For cost accounting this is precisely the right discipline. A cryptographic signature authorizing a fuel payment is a resource: presenting it pays for exactly one obligation, it cannot be copied to pay twice, and a deploy may not claim a payment without actually supplying the witness.

### 3.2 Structural rules, and what linearity removes

A *sequent* `Γ ⊢ B` reads "from the hypotheses `Γ`, conclusion `B` follows." The **structural rules** govern how the hypothesis list may be manipulated independently of the connectives:

```
              Γ ⊢ B                         Γ, A, A ⊢ B
  ─────────────────────  weakening      ─────────────────  contraction
        Γ, A ⊢ B                            Γ, A ⊢ B
```

- **Weakening** lets you add (or, read upward, discard) an unused hypothesis `A`.
- **Contraction** lets you collapse two copies of `A` into one — equivalently, to use one `A` twice.

Linear logic **removes both** (keeping only exchange). The cost-accounting readings are immediate and are exactly the two security properties we want:

| Removed rule | Cost-accounting meaning | Mechanized as |
|--------------|-------------------------|---------------|
| contraction | you cannot use one witness twice → **no double-spend** | `ll_linear_no_contraction` ([§8.2](#82-no-contraction-no-weakening)) |
| weakening | you cannot discard a required witness for free → **no free weakening** | `ll_linear_no_weakening` ([§8.2](#82-no-contraction-no-weakening)) |

### 3.3 The multiplicative connectives: `⊗` and `⊸`

The **tensor** `σ ⊗ τ` means "I have both `σ` and `τ`, and the witnesses for `σ` are disjoint from those for `τ`." Because the witnesses are disjoint, the cost is *additive*: `cost(σ ⊗ τ) = cost(σ) + cost(τ)`. In cost-accounting terms, `σ ⊗ τ` is the multi-signer requirement "every one of these signers must contribute." A list of `N` cosigners folds to a left-nested tensor `((σ₁ ⊗ σ₂) ⊗ …) ⊗ σₙ`, which is exactly what the runtime does at `accounting/mod.rs:605` (see [§9.4](#94-rust-runtime-grounding)).

The **lollipop** `σ ⊸ τ` (linear implication) means "consume a `σ` to produce a `τ`." It is the type of a *capability*: a delegation that, when fed an authorization `σ`, yields an authorization `τ`, spending `σ` in the process. Its cost is again additive, `cost(σ ⊸ τ) = cost(σ) + cost(τ)`, because exercising the capability requires both the input witness and whatever the output obligates.

### 3.4 The additive connectives: `&` and `⊕`

Where the multiplicatives are about having *both* of two resources at once, the additives are about a *choice* between two — and they differ in *who* chooses.

- **With**, `σ & τ`, is the **verifier's** choice. The party authorizing must be ready to satisfy *either* branch, because the verifier decides at validation time which branch's fuel flows. Both branches must therefore be available, so `cost(σ & τ) = cost(σ) + cost(τ)`.
- **Plus**, `σ ⊕ τ`, is the **signer's** choice. The party constructing the deploy commits, at signing time, to exactly one branch (recorded as a left/right witness). Only that branch's witnesses are required, so `cost(σ ⊕ τ) = cost(chosen branch)`.

This signer-vs-verifier asymmetry is exactly the linear-logic distinction between internal (`⊕`) and external (`&`) choice, and the Rust enum encodes it verbatim (`accounting/mod.rs:845` for `Plus` "signer's choice", `:852` for `With` "verifier's choice").

### 3.5 The exponentials: ! and ?

The exponentials are the controlled re-entry of the structural rules. `!σ` ("of-course `σ`", "bang `σ`") marks a resource that *may* be weakened and contracted — i.e., used any number of times, including zero. In cost-accounting terms `!σ` is a **reusable capability**: a standing authorization that funds many reductions from a single registration. Crucially, **registering it costs only what `σ` costs once**: `cost(!σ) = cost(σ)`; the unbounded *reuse* is accounted at the capability registry, not here.

`?σ` ("why-not `σ`") is the dual: an **optional** authorization, present zero-or-more times. A deploy whose authorization is `?σ` is accepted whether or not `σ` is actually supplied, so it obligates nothing: `cost(?σ) = 0`.

### 3.6 The multiplicative unit `1`

The tensor `⊗` is a monoid-like operation, and like every such operation it has a neutral element. That neutral element is the **multiplicative unit `1`**: the "empty bundle of resources," the authorization that demands nothing.

Three facts pin `1` down, and all three are mechanized:

1. **It is the `⊗`-identity.** `1 ⊗ σ ≡ σ` and `σ ⊗ 1 ≡ σ` (the *unit laws*), proved at the channel layer as `tensor_unit_left` and `tensor_unit_right` ([§7.2](#72-multiplicative-laws-incl-the-unit)). The Rust enum states the same: "*`1` — multiplicative unit. Identity for `And` / `Tensor`: `σ ⊗ 1 ≡ σ`*" (`accounting/mod.rs:822`).
2. **It costs nothing.** `ll_required_units LLUnit = 0` and `sig_algebra_min_required ASUnit = 0`. Adding `1` to any bundle leaves the bundle's cost unchanged — definitionally, since `cost(1 ⊗ σ) = 0 + cost(σ) = cost(σ)`.
3. **It is derivable from the empty linear zone.** In the sequent calculus, `Γ ; · ⊢ 1` for any `Γ` (the rule `dill_unit`, [§4.3](#43-the-repos-dill-relation)): you can always "authorize nothing" without spending any witness.

`1` is the backbone of the algebra: it is the base case of the recursive cost function, the unit object in the monoidal-coherence theorems ([§7.6](#76-monoidal-coherence-mac-lane)), and the cost-accounting motif of a *free authorization*.

### 3.7 The nine connectives at a glance

```
 multiplicative │ ⊗  tensor      both, disjoint witnesses     cost = c(σ)+c(τ)
                │ 1  unit         the empty requirement         cost = 0
                │ ⊸  lollipop     consume σ to yield τ          cost = c(σ)+c(τ)
 ───────────────┼──────────────────────────────────────────────────────────────
 additive       │ &  with         verifier's choice (both)      cost = c(σ)+c(τ)
                │ ⊕  plus         signer's choice (one)         cost = c(chosen)
 ───────────────┼──────────────────────────────────────────────────────────────
 exponential    │ !  of-course    reusable (unbounded)          cost = c(σ)
                │ ?  why-not       optional (zero-or-more)       cost = 0
 ───────────────┼──────────────────────────────────────────────────────────────
 derived        │ Threshold k     any k of N members            cost = k
                │ atom            one concrete signer            cost = 1
```

---

## 4. Dual Intuitionistic Linear Logic and the Two-Zone Sequent

### 4.1 Why two zones

Linear logic's exponential `!` does real work — it re-admits weakening and contraction — but a sequent calculus with `!` rules sprinkled through it is awkward to reason about. Benton's *mixed linear / non-linear* logic [[3](#ref-3)] and Barber's **Dual Intuitionistic Linear Logic (DILL)** [[2](#ref-2)] reorganize this by *splitting the context into two zones*:

- an **unrestricted zone `Γ`** of hypotheses that behave intuitionistically (freely weakened and contracted — i.e., reusable), and
- a **linear zone `Δ`** of hypotheses that must be used exactly once.

A DILL sequent is written `Γ ; Δ ⊢ A`. The modality `!` is precisely what mediates the two zones: a `!`-formula is one that may be moved from the disciplined linear zone into the free unrestricted zone. This two-zone presentation maps cleanly onto cost accounting: **`Γ` holds the reusable capabilities (the `!`-resources), `Δ` holds the spendable fuel witnesses, and the whole accounting question is how `Δ` is partitioned and consumed.**

### 4.2 Reading the judgment

Read `Γ ; Δ ⊢ A` as:

> "Given the reusable capabilities listed in `Γ` and the single-use witnesses listed in `Δ`, the authorization `A` is derivable — consuming exactly the witnesses in `Δ` and none of `Γ`."

Two structural facts make this a *linear* discipline rather than an ordinary one, and both are visible in the rules below:

- `Γ` is **copied** into every premise of a multi-premise rule (it is reusable, so sharing it costs nothing).
- `Δ` is **partitioned** across premises (it is single-use, so each witness goes to exactly one premise — never duplicated, never dropped).

The diagram below contrasts the two zones and shows three representative rules.

![Two-zone DILL sequent Γ ; Δ ⊢ A. The unrestricted zone Γ (reusable, admitting weakening and contraction) is copied to every premise, while the linear zone Δ (single-use, admitting neither) is partitioned across premises. Three representative dill rules are shown: dill_tensor splits Δ into Δ₁ and Δ₂ for the two premises while sharing Γ; dill_lolly_intro pushes the antecedent A onto the linear zone; dill_unrestricted draws a reusable hypothesis from Γ and derives the bang of it with an empty linear zone.](diagrams/dill-two-zone-flow.svg)

(*Source: [`diagrams/dill-two-zone-flow.puml`](diagrams/dill-two-zone-flow.puml) — render with `plantuml -tsvg docs/theory/diagrams/dill-two-zone-flow.puml`.*)

### 4.3 The repo's `dill` relation

`LinearLogicResources.v:133` defines the two-zone judgment as an inductive relation:

```coq
Inductive dill : unrestricted_ctx -> linear_ctx -> ll_formula -> Prop := …
```

where `unrestricted_ctx` and `linear_ctx` are both `list ll_formula` (`lines 100–101`). Its ten constructors, presented in literate sequent-rule style (premises above the bar, conclusion below; `·` is the empty linear zone), are:

```
─────────────────  dill_ax              one linear witness proves itself
 Γ ; [A] ⊢ A

─────────────────  dill_unit            the unit needs no witness
 Γ ; · ⊢ 1

   f ∈ Γ
─────────────────  dill_unrestricted    a reusable hypothesis yields !f,
 Γ ; · ⊢ ! f                            spending nothing linear

 Γ ; Δ₁ ⊢ A      Γ ; Δ₂ ⊢ B
────────────────────────────  dill_tensor    ⊗ splits the linear zone (Δ₁ ⊎ Δ₂),
 Γ ; Δ₁,Δ₂ ⊢ A ⊗ B                            shares Γ

 Γ ; Δ ⊢ A                       Γ ; Δ ⊢ B
──────────────────────  dill_plus_left   ────────────────────── dill_plus_right
 Γ ; Δ ⊢ A ⊕ B                   Γ ; Δ ⊢ A ⊕ B
   (signer injects the chosen branch — left or right)

 Γ ; Δ ⊢ A      Γ ; Δ ⊢ B
──────────────────────────  dill_with    & shares the SAME Δ across both premises
 Γ ; Δ ⊢ A & B                            (the verifier will project one)

 Γ ; A,Δ ⊢ B
──────────────────  dill_lolly_intro     ⊸ moves the antecedent into Δ
 Γ ; Δ ⊢ A ⊸ B

 Γ ; Δ₁ ⊢ A ⊸ B      Γ ; Δ₂ ⊢ A
──────────────────────────────────  dill_lolly_elim   linear modus ponens:
 Γ ; Δ₁,Δ₂ ⊢ B                                         consume the argument's Δ₂

─────────────────  dill_whynot_intro     ?f needs no witness
 Γ ; · ⊢ ? f
```

Read against [§2](#2-glossary-of-symbols-and-terms), these rules are simply the cost discipline written as inference rules. `dill_tensor` *splits* the witness store (each witness funds one conjunct — no contraction); `dill_with` *shares* it (both branches are costed because the verifier chooses); `dill_lolly_elim` is linear modus ponens, the rule that consumes a capability's argument; and `dill_unit`, `dill_unrestricted`, `dill_whynot_intro` all derive their conclusion from the *empty* linear zone `·`, formalizing "this costs no fuel."

### 4.4 A scope caveat, stated up front

The `dill` relation is a faithful but **deliberately small fragment** of full DILL. It is single-conclusion and *introduction-flavored*: it has no explicit cut constructor (`dill_cut`), its `!`-rule (`dill_unrestricted`) fuses dereliction-from-`Γ` with `!`-introduction, and `dill_whynot_intro` is unconditional. We document exactly what is and is not included in [§10.1](#101-the-dill-fragment-caveat); cut is studied separately, and admissibly, at the channel layer ([§7.8](#78-cut-admissibly)). What the fragment *does* establish rigorously is the resource behaviour of every connective and the two substructural prohibitions — which is exactly what cost accounting needs.

---

## 5. The Runtime Signature Algebra and the Reflection Pipeline

### 5.1 The `sig_algebra` type

The earlier cost-accounted syntax modeled a signature with only three constructors — `SUnit`, `SHash`, `SAnd` (unit, atom, tensor) — at `CostAccountedSyntax.v:76`. The linear-logic work generalizes this to the full ILLE connective set in a *new* inductive, `sig_algebra` (`CostAccountedSyntax.v:229`):

```coq
Inductive sig_choice : Type := ChooseLeft | ChooseRight.

Inductive sig_algebra : Type :=
  | ASUnit      : sig_algebra                                   (* 1 *)
  | ASHash      : nat -> sig_algebra                            (* atom *)
  | ASAnd       : sig_algebra -> sig_algebra -> sig_algebra     (* ⊗ *)
  | ASThreshold : nat -> list sig_algebra -> sig_algebra        (* k-of-N *)
  | ASPlus      : sig_choice -> sig_algebra -> sig_algebra -> sig_algebra  (* ⊕ *)
  | ASWith      : sig_algebra -> sig_algebra -> sig_algebra     (* & *)
  | ASBang      : sig_algebra -> sig_algebra                    (* ! *)
  | ASWhyNot    : sig_algebra -> sig_algebra                    (* ? *)
  | ASLolly     : sig_algebra -> sig_algebra -> sig_algebra.    (* ⊸ *)
```

Note that `ASPlus` carries a `sig_choice` — the signer's committed branch — directly in the term, because (per [§3.4](#34-the-additive-connectives--and-)) the cost of a `⊕` depends on which branch was chosen. The `ASThreshold` constructor is primitive rather than derived: a k-of-N quorum is not cheaply expressible via `⊕`/`⊗` without an `O(C(n,k))` blow-up (`accounting/mod.rs:839`).

### 5.2 The cost function `sig_algebra_min_required`

The cost of an authorization is the fixpoint `sig_algebra_min_required` (`CostAccountedSyntax.v:253`). In literate form, the minimum number of valid signatures a `sig_algebra` obligates is computed by structural recursion:

> The unit obligates **nothing** (0). An atom obligates **one** signature (1). Tensor, with, and lollipop are **additive** — their cost is the sum of their parts' costs, because every part must be funded. A plus costs only its **chosen** branch (`ChooseLeft → left`, `ChooseRight → right`). A bang costs exactly what its **inner** formula costs (reuse adds nothing). A why-not costs **nothing** (it is optional). A threshold of k members costs exactly **k**.

```coq
Fixpoint sig_algebra_min_required (s : sig_algebra) : nat :=
  match s with
  | ASUnit                  => 0
  | ASHash _                => 1
  | ASAnd s1 s2             => sig_algebra_min_required s1 + sig_algebra_min_required s2
  | ASThreshold k _         => k
  | ASPlus ChooseLeft  s1 _ => sig_algebra_min_required s1
  | ASPlus ChooseRight _ s2 => sig_algebra_min_required s2
  | ASWith s1 s2            => sig_algebra_min_required s1 + sig_algebra_min_required s2
  | ASBang s'               => sig_algebra_min_required s'
  | ASWhyNot _              => 0
  | ASLolly s1 s2           => sig_algebra_min_required s1 + sig_algebra_min_required s2
  end.
```

Each clause is anchored by a one-line lemma so downstream proofs can cite a named fact rather than unfold the fixpoint: `sig_algebra_plus_left_min_required` (`:308`), `…_plus_right_…` (`:314`), `…_with_…` (`:320`), `…_bang_…` (`:326`), `…_whynot_min_required_zero` (`:331`), `…_lolly_…` (`:335`), and `…_threshold_min_required` (`:341`).

### 5.3 Companion measures

Three further fixpoints round out the runtime model:

- `sig_algebra_atoms` (`:240`) — the list of every atom id in the formula.
- `sig_algebra_valid` (`:280`) — well-formedness; for `ASThreshold k members` it requires `(1 ≤ k) ∧ (k ≤ |members|) ∧` every member valid. The bound is extracted by `sig_algebra_threshold_valid_bounds` (`:346`).
- `sig_algebra_all_required` (`:267`) — true iff *every* atom is mandatory (false for threshold, plus, why-not). When it holds, cost equals the atom count, a fact proved by induction over all nine constructors in `sig_algebra_all_required_min_required_atoms` (`:358`). This is the N-of-N special case: a pure tensor of atoms costs exactly as many witnesses as it contains.

### 5.4 Object-level `ll_formula` and the reflection `ll_of_sig_algebra`

To connect the runtime algebra to *linear logic proper*, `LinearLogicResources.v` defines the object-level syntax `ll_formula` (`line 7`) — one constructor per connective — and a reflection `ll_of_sig_algebra : sig_algebra → ll_formula` (`line 18`) that maps each runtime constructor to its linear-logic counterpart (`ASUnit ↦ LLUnit`, `ASAnd ↦ LLTensor`, `ASWith ↦ LLWith`, `ASBang ↦ LLBang`, `ASLolly ↦ LLLolly`, and so on). The runtime signature *is* a linear-logic formula; `ll_of_sig_algebra` is the witness of that fact.

The full pipeline — from the Rust wire `Sig`, through the Rocq runtime `sig_algebra`, through reflection into `ll_formula`, to the cost — is shown below. The crucial feature is that it **commutes**: the two ways of computing a cost (directly via `sig_algebra_min_required`, or by reflecting and then taking `ll_required_units`) give the same answer.

![Reflection and cost pipeline. The Rust Sig enum models the Rocq sig_algebra; sig_algebra reflects via ll_of_sig_algebra into the object-level ll_formula; sig_algebra projects to a cost via sig_algebra_min_required, and ll_formula projects to a cost via ll_required_units. The two cost projections are equal, an equality closed by the Rocq theorem ll_sig_algebra_required_complete; the Rust min_required_for mirrors sig_algebra_min_required clause-for-clause.](diagrams/ll-reflection-pipeline.svg)

(*Source: [`diagrams/ll-reflection-pipeline.puml`](diagrams/ll-reflection-pipeline.puml) — render with `plantuml -tsvg docs/theory/diagrams/ll-reflection-pipeline.puml`.*)

### 5.5 Object-level measures: cost vs. capacity vs. consumed

Three measures on `ll_formula` distinguish notions that are easy to conflate (all from `LinearLogicResources.v`):

- `ll_required_units` (`:45`) — the **cost**, mirroring `sig_algebra_min_required` clause-for-clause.
- `ll_available_slots` (`:59`) — the **capacity**: how many witness positions exist. For a threshold it is the *number of members* (N), not the quorum (k) — capacity ≥ cost.
- `ll_consumed_atoms` (`:72`) — the atoms **actually spent**: like `ll_atoms` but `⊕` takes only the chosen branch and `?` consumes nothing.

The distinction matters for the threshold connective: a 2-of-3 quorum has capacity 3 (three members are available) but cost 2 (only two are required).

### 5.6 The runtime-bridge theorems

The keystone results prove the pipeline of [§5.4](#54-object-level-ll_formula-and-the-reflection-ll_of_sig_algebra) commutes. They are what turn "the runtime cost *is* a linear-logic resource count" from a slogan into a theorem.

> **Theorem** (`ll_sig_algebra_required_complete`, `LinearLogicResources.v:206`). For every signature algebra `s`,
> `ll_required_units (ll_of_sig_algebra s) = sig_algebra_min_required s`.
>
> *Proof.* Structural induction on `s`; the tensor/with/lollipop cases rewrite by the two inductive hypotheses, the plus case case-splits on the `sig_choice`, and every other case is definitional. ∎

> **Theorem** (`ll_sig_algebra_consumed_matches_presented`, `:225`). For every `s`, `ll_consumed_atoms (ll_of_sig_algebra s) = sig_algebra_presented_atoms s` — the atoms the linear-logic reading spends are exactly the atoms the runtime presents.

> **Theorem** (`ll_sig_algebra_threshold_valid_bounds_bridge`, `:247`). A valid threshold reflects to a formula whose quorum is in range: `1 ≤ k ≤ |members|`.

These three say the object-level linear-logic semantics and the runtime semantics agree on *cost*, on *which witnesses are spent*, and on *quorum well-formedness* — the complete static interface between the two layers.

---

## 6. The Cost-Accounting Interpretation: resource = cost

### 6.1 The master correspondence

Collecting [§3](#3-linear-logic-from-first-principles)–[§5](#5-the-runtime-signature-algebra-and-the-reflection-pipeline), each connective carries a single, consistent meaning across logic, cost, and runtime:

| Connective | Linear-logic meaning | Cost clause | Cost-accounting reading | Rust `Sig` |
|:----------:|----------------------|-------------|-------------------------|------------|
| `1` | multiplicative unit | `0` | free authorization; neutral element | `Unit` |
| atom | atomic proposition | `1` | one concrete signer | `Hash` |
| `⊗` | multiplicative `∧` | `c(σ)+c(τ)` | all cosigners must contribute | `And` |
| `&` | additive `∧` | `c(σ)+c(τ)` | both available; verifier projects one | `With` |
| `⊕` | additive `∨` | `c(chosen)` | signer commits to one branch | `Plus` |
| `!` | of-course | `c(σ)` | reusable standing capability | `Bang` |
| `?` | why-not | `0` | optional witness | `WhyNot` |
| `⊸` | linear implication | `c(σ)+c(τ)` | capability: spend `σ`, obtain `τ` | `Lolly` |
| Threshold `k` | (derived) | `k` | any `k` of `N` signers | `Threshold` |

The recursion bottoms out at `1` (cost 0) and atoms (cost 1); every composite folds these by sum, by chosen-branch, or by quorum. The following inline derivation shows the cost of a small authorization computed structurally — note how `1` contributes 0 and drops out:

```
authorization:  ((a ⊗ b) ⊗ c)  ⊗  (1 ⊕ d)        a,b,c,d atomic; signer chose RIGHT
                       │                  │
        ┌──────────────┴───────┐     ┌────┴─────┐
   (a ⊗ b) ⊗ c               (1 ⊕ d)            (chosen branch = d)
        │                        │
   ┌────┴────┐              cost = c(d) = 1     ← ⊕ pays only the chosen branch
 a ⊗ b       c
   │
 ┌─┴─┐
 a   b

   ll_required_units = ((1 + 1) + 1) + 1  =  4
```

### 6.2 Linear zone = spendable fuel

The single-use witnesses in `Δ` model fuel that is paid out exactly once. The two prohibitions of [§3.2](#32-structural-rules-and-what-linearity-removes) become the two core security properties, both proved in [§8](#8-substructural-guarantees-no-double-spend-no-free-weakening): no contraction → **no double-spend** (`ll_linear_no_contraction`), no weakening → **no fuel discarded or required-witness skipped** (`ll_linear_no_weakening`).

### 6.3 `!`/unrestricted zone = reusable capabilities

A `Sig::Bang` is a replicable authorization — one registration funds many invocations (`accounting/mod.rs:857`). In the DILL model this is exactly the unrestricted zone `Γ`: using a `Γ`-hypothesis leaves `Γ` unchanged (`reuse_unrestricted γ = γ`, `LinearLogicResources.v:130`), and using it twice still leaves `Γ` unchanged (`ll_unrestricted_can_be_reused`, [§8.4](#84-unrestricted-reuse-is-free-and-idempotent)). Registration cost is just `c(σ)` once; *bounded* reuse (a capability usable at most n times) is enforced by the `rho:system:capabilities` registry, outside this static model.

### 6.4 Unit `1` = zero-cost neutral element

By [§3.6](#36-the-multiplicative-unit-1), `1` costs `0`, is the `⊗`-identity up to channel equivalence, and is derivable from the empty linear zone. It is the recurring "free authorization" of the algebra and the base case that makes the recursive cost function total.

### 6.5 Worked example: a 2-of-3 threshold deploy, end-to-end

Consider a deploy authorized by *any two of three* cosigners with public keys yielding atoms `a₁`, `a₂`, `a₃`, each funding a `phlo_share` of 100 against a `phlo_limit` of 300.

1. **Authorization term.** `s = ASThreshold 2 [ASHash a₁; ASHash a₂; ASHash a₃]`. It is well-formed: `1 ≤ 2 ≤ 3`, so `sig_algebra_valid s = true` ([§5.3](#53-companion-measures)).
2. **Reflection.** `ll_of_sig_algebra s = LLThreshold 2 [LLAtom a₁; LLAtom a₂; LLAtom a₃]`.
3. **Cost.** `sig_algebra_min_required s = 2`, and by `ll_sig_algebra_required_complete` the reflected cost agrees: `ll_required_units (ll_of_sig_algebra s) = 2`. The capacity is 3 (`ll_available_slots`), so cost (2) < capacity (3) — the hallmark of a quorum. The result is packaged by `ll_threshold_quorum_sound` ([§8.5](#85-per-connective-resource-laws)): validity gives `1 ≤ 2 ∧ 2 ≤ 3 ∧ required = 2`.
4. **Runtime dispatch.** The wire `SigThreshold { threshold: 2, members: […] }` yields `min_required_for = 2` (`casper_message.rs:1524`), and since this is neither an all-required N-of-N nor a cost-0 optional, the dispatcher routes to `from_signed_data_threshold(data, signers, phlo_limit, 2)` (`casper_message.rs:1373`).
5. **No-free-weakening in action.** Suppose signers 1 and 2 present valid signatures (already meeting the quorum) but signer 3 presents a *non-empty but invalid* signature. The verifier still rejects with `SignatureVerifyFailed`, because every non-empty signature is verified *before* the quorum count is checked (`signed.rs:293`). A presented witness cannot be silently dropped while its `phlo_share` still participates in the envelope total — this is the no-weakening rule enforced at runtime, and it is the exact defect [§9.5](#95-the-bug-the-corroboration-caught) describes.
6. **Model-checking mirror.** The TLA⁺ `ThresholdProtocol` checks `QuorumThresholdConstraint` (k ∈ [1,n]), `QuorumExactness` (an accepting set has ≥ k members), and `QuorumNoOverCount` (≤ N) — the finite-state shadow of steps 1 and 3.

---

## 7. Channel-Layer Algebraic Identities

`LLIdentities.v` proves the *equational* theory of the connectives — commutativity, associativity, the unit laws, distributivity, coherence — at the **reflection (channel) layer**. This complements [§5](#5-the-runtime-signature-algebra-and-the-reflection-pipeline)/[§8](#8-substructural-guarantees-no-double-spend-no-free-weakening), which are about *cost* and *consumption*; here we ask which authorizations are *interchangeable in shape*.

### 7.1 The channel model and why it is faithful

A `channel` is `list ℕ` — a multiset of atom ids — and `channel_equiv` is `Permutation` (`LLIdentities.v:45`). Every connective reflects to either concatenation (`tensor_channel`, `plus_channel`, `with_channel`, `lolly_channel`, `threshold_channel`) or the identity (`bang_channel`, `whynot_channel`) (`:75–86`). This is faithful because the substrate's reflection step `SignatureChannel::from_sig` post-composes with `ParSortMatcher::sort_match` (`accounting/mod.rs:1097`), which canonicalizes the resulting channel and makes it invariant under permutation of constituents — so a list quotiented by `Permutation` captures exactly the reflection-layer equivalence. The semantic *distinctions* (signer- vs verifier-choice, replicability, branch selection) are deliberately *not* in the channel shape; they are enforced at the verifier-dispatch and capability-registry layers, as the file's own comments record (`:121–125`, `:230–243`).

### 7.2 Multiplicative laws (incl. the unit)

> `tensor_commutative` (`:92`): `σ ⊗ τ ≡ τ ⊗ σ`. `tensor_associative` (`:99`): `(σ ⊗ τ) ⊗ ρ ≡ σ ⊗ (τ ⊗ ρ)`.
> **The unit laws** — `tensor_unit_left` (`:107`): `1 ⊗ σ ≡ σ`, and `tensor_unit_right` (`:114`): `σ ⊗ 1 ≡ σ` — with the empty channel `[]` serving as `1`.

These four make `(channels, ⊗, 1)` a commutative monoid up to channel equivalence; the unit laws are the formal content of [§3.6](#36-the-multiplicative-unit-1).

### 7.3 Additive laws, projections, and injections

`plus_commutative`/`plus_associative` and `with_commutative`/`with_associative` (`:128–156`) give the same monoid shape for `⊕` and `&`. The choice structure appears as containments: `with_projection_left`/`with_projection_right` (`:388`, `:395`) model `σ & τ ⊢ σ` and `⊢ τ` (the verifier may take *either* branch), while `plus_injection_left`/`plus_injection_right` (`:410`, `:417`) model `σ ⊢ σ ⊕ τ` and `τ ⊢ σ ⊕ τ` (the signer injects *one* branch).

### 7.4 Exponential laws and admissible structural rules

`bang_idempotent` (`!!σ ≡ !σ`) and `whynot_idempotent` (`:163`, `:167`) hold; `bang_unit`/`whynot_unit` (`:171`, `:175`) record that `!` and `?` are the identity at the channel layer. The three structural rules that `!` *re-admits* are proved admissible as channel relationships: `bang_dereliction_admissible` (`!σ ⊢ σ`, `:326`), `bang_weakening_admissible` (`!σ ⊢ 1`, `:333`), `bang_contraction_admissible` (`!σ ⊢ !σ ⊗ !σ`, `:343`); duals for `?` at `:358–379`. These are the channel-layer counterpart to the controlled re-entry of weakening/contraction described in [§3.5](#35-the-exponentials--and-).

### 7.5 Implication and currying

`lolly_to_tensor_channel` (`:187`) records that at the channel layer `σ ⊸ τ` reflects like `σ ⊗ τ`. `lolly_curry_isomorphism` (`:291`) proves the closed-monoidal adjunction `(σ ⊗ τ) ⊸ ρ ≡ σ ⊸ (τ ⊸ ρ)`. The modus-ponens *channel decomposition* `σ ⊗ (σ ⊸ τ) ≡ σ ⊗ σ ⊗ τ` is `lolly_modus_ponens_channel_decomposition` (`:308`) — with the explicit caveat that the genuine reduction `σ ⊗ (σ ⊸ τ) ⊢ τ` (which *consumes* `σ`) lives in the reduction relation, not here. The capability reading is shown below.

![Sequence diagram of linear modus ponens. The deployer presents a witness σ to the linear zone Δ, then invokes a capability σ ⊸ τ; the capability consumes σ from Δ via consume_linear_atom, leaving Δ empty, and yields τ. A note records the conservation law ll_lolly_resource_flow_conservative: the cost of σ ⊸ τ equals the cost of σ plus the cost of τ, so no resource is created from nothing.](diagrams/lolly-modus-ponens-sequence.svg)

(*Source: [`diagrams/lolly-modus-ponens-sequence.puml`](diagrams/lolly-modus-ponens-sequence.puml) — render with `plantuml -tsvg docs/theory/diagrams/lolly-modus-ponens-sequence.puml`.*)

### 7.6 Monoidal coherence (Mac Lane)

That *all* the reassociations and unit-cancellations above are mutually consistent is the content of Mac Lane's coherence theorem for monoidal categories [[5](#ref-5)]. Both coherence diagrams are proved at the channel layer: the **pentagon** `tensor_associator_pentagon_coherent` (`:460`) for the associator over a four-fold tensor, and the **triangle** `tensor_unitor_triangle_coherent` (`:470`) for the unitor, with `[]` as the unit object. Coherence is what licenses us to write `σ ⊗ τ ⊗ ρ` without parentheses and to drop `1` freely.

### 7.7 Threshold/quorum identities

Beyond `threshold_permutation_invariant` (`:210`, reordering members preserves the channel), the file proves `threshold_singleton_collapse` (a 1-of-1 threshold is its member, `:486`), `threshold_empty_members` (a vacuous quorum is the unit `[]`, `:494`), and `threshold_associative_at_channel` (`:501`).

### 7.8 Cut, admissibly

The classical cut rule "from `Γ ⊢ σ` and `Δ, σ ⊢ τ` derive `Γ, Δ ⊢ τ`" is proved at the channel layer as a *containment* composition, `cut_admissible` (`:434`). This is the channel-layer stand-in for the cut that the `dill` fragment of [§4](#4-dual-intuitionistic-linear-logic-and-the-two-zone-sequent) omits as a constructor; see [§10.1](#101-the-dill-fragment-caveat).

### 7.9 Runtime-bridge equalities

Finally, a family of one-line theorems (`:528–587`) re-states the cost clauses of [§5.2](#52-the-cost-function-sig_algebra_min_required) as equalities on `sig_algebra_min_required` itself (e.g. `ll_tensor_min_required_matches_runtime`, `ll_whynot_min_required_matches_runtime`), and `ll_all_required_uses_all_atoms` (`:573`) ties the N-of-N case to the atom count. These close the loop between the equational layer and the cost layer.

---

## 8. Substructural Guarantees: No Double-Spend, No Free Weakening

This section presents the security-critical theorems — the formal statements that the linear zone genuinely behaves linearly. All are in `LinearLogicResources.v`, all `Qed`-closed, no axioms.

### 8.1 The single-use operation

Spending a witness is the function `consume_linear_atom : ℕ → linear_ctx → option linear_ctx` (`:109`). In literate form:

> To consume atom *target* from a linear zone, walk the zone front to back. At the first `LLAtom` equal to *target*, remove it and return the rest. If the head is some other formula, keep it and recurse into the tail. If the zone is exhausted without a match, fail with `None`.

The `option` result is the whole point: consumption can **fail**, and a failed consume is how double-spend is detected.

### 8.2 No contraction, no weakening

> **Theorem** (`ll_linear_no_contraction`, `:316`). For every atom *a*, the linear zone `[LLAtom a]` is **not** channel-equivalent to `[LLTensor (LLAtom a) (LLAtom a)]`.
> *Proof.* A permutation preserves length; the left zone presents one atom, the right presents two, and `1 ≠ 2`. ∎

> **Theorem** (`ll_linear_no_weakening`, `:327`). The empty linear zone is not equivalent to `[LLAtom a]` — you cannot conjure a witness from nothing. *(Same length argument: `0 ≠ 1`.)* ∎

`ll_linear_atom_contraction_changes_count` (`:336`) makes the contraction failure quantitative: the single zone has atom-count 1, the duplicated zone has count 2.

### 8.3 No double-spend

> **Theorem** (`ll_consume_linear_once_atom_exhausts`, `:349`). `consume_linear_atom a [LLAtom a] = Some []` — spending the sole witness empties the zone.

> **Theorem** (`ll_no_double_spend_single_witness`, `:359`). After consuming *a* from `[LLAtom a]`, a *second* consume of *a* returns `None`. A single witness funds exactly one obligation.

> **Theorem** (`ll_double_spend_requires_duplicate_witness`, `:371`). From `[LLAtom a; LLAtom a]`, two consecutive consumes of *a* both succeed (ending in `Some []`). You can only "spend twice" if you genuinely held two witnesses.

The statechart below traces both cases side by side.

![Statechart of no-double-spend. In the single-witness branch, a zone holding one witness a transitions on consume to an exhausted state, and a second consume transitions to a rejected state returning None. In the duplicate-witness branch, a zone holding two copies of a transitions through one-remaining to empty across two successful consumes. Notes attach the theorems ll_no_double_spend_single_witness and ll_double_spend_requires_duplicate_witness to the two outcomes.](diagrams/no-double-spend-statechart.svg)

(*Source: [`diagrams/no-double-spend-statechart.mmd`](diagrams/no-double-spend-statechart.mmd) — render with `mmdc -i docs/theory/diagrams/no-double-spend-statechart.mmd -o docs/theory/diagrams/no-double-spend-statechart.svg`.*)

`ll_linear_cut_consumes_cut_witness` (`:405`) records the companion fact `consume_linear_atom a (LLAtom a :: Δ) = Some Δ`: a cut consumes precisely its cut witness, leaving the rest of the zone intact.

### 8.4 Unrestricted reuse is free and idempotent

Dual to the linear zone, the unrestricted zone `Γ` is reusable. `ll_unrestricted_reuse_preserves_context` (`:387`) proves `reuse_unrestricted f γ = γ`, and `ll_unrestricted_can_be_reused` (`:396`) proves reuse is idempotent (using twice still returns `γ`). `ll_unrestricted_cut_preserves_linear_zone` (`:415`) records that drawing on `Γ` leaves `Δ` untouched. The contrast between [§8.3](#83-no-double-spend) (linear, depletes) and this subsection (unrestricted, persists) *is* the cost-accounting meaning of the two zones.

### 8.5 Per-connective resource laws

A final group tabulates each connective's resource behaviour. Each is `Qed`-closed in `LinearLogicResources.v`:

| Theorem | Statement | Line |
|---------|-----------|:----:|
| `ll_plus_left_consumes_chosen_branch` | `⊕`(left) costs/consumes only the left branch | `:258` |
| `ll_plus_right_consumes_chosen_branch` | `⊕`(right) costs/consumes only the right branch | `:264` |
| `ll_with_requires_both_branches_available` | `&` costs the sum and concatenates consumed atoms | `:270` |
| `ll_bang_reuse_no_extra_linear_cost` | `!σ` costs exactly what `σ` costs | `:278` |
| `ll_whynot_consumes_no_linear_witness` | `?σ` costs `0` and consumes `[]` | `:285` |
| `ll_lolly_resource_flow_conservative` | `σ ⊸ τ` costs `c(σ)+c(τ)`; no resource ex nihilo | `:291` |
| `ll_threshold_quorum_sound` | valid `Threshold(k,…)` gives `1 ≤ k ≤ n ∧ cost = k` | `:299` |

---

## 9. Multi-Modal Corroboration

### 9.1 Four models that must agree

The linear-logic cost laws are checked in four independent ways, on the principle that any disagreement localizes a bug. The Sage reference header states the contract directly: *"Any counterexample found here is a bug in either the Rocq proof, the Rust substrate, or this Python reference implementation — they must agree"* (`ll_identity_search.sage:9`). This is the scientific-method ledger applied to a proof obligation: the same law, modeled four ways, with discrepancies treated as falsified hypotheses.

![Architecture of the multi-modal corroboration. Four components — Rocq proofs (LinearLogicResources.v, LLIdentities.v; machine-checked and unbounded), Sage bounded-exhaustive search, TLA+ per-connective model checking, and the executable Rust runtime with regression tests — are connected by bidirectional agreement edges: Rocq and Sage share identical cost laws, Rocq and TLA+ share conservation and per-connective protocols, Rocq and Rust agree that min_required equals sig_algebra_min_required, and Sage and Rust agree that reflect mirrors SignatureChannel::from_sig. A dashed feedback arrow from Rust to Rocq records that the no-free-weakening obligation caught a real threshold-verifier bug.](diagrams/ll-corroboration-architecture.svg)

(*Source: [`diagrams/ll-corroboration-architecture.puml`](diagrams/ll-corroboration-architecture.puml) — render with `plantuml -tsvg docs/theory/diagrams/ll-corroboration-architecture.puml`.*)

### 9.2 Sage bounded-exhaustive search

`ll_identity_search.sage` re-implements the `Sig` algebra in Python, with `reflect()` mirroring `SignatureChannel::from_sig` (`:97`), and `required_units` / `consumed_atoms` / `consume_atom_once` mirroring their Rocq namesakes (`:119`, `:142`, `:163`). It checks two suites:

- **16 identities** (`:336`) — the channel-equivalence laws of [§7](#7-channel-layer-algebraic-identities), *including two anti-identities that must FAIL*: `anti_contraction` (`σ ⊗ σ ≢ σ`) and `anti_weakening` (`σ ⊗ τ ≢ σ`), which encode the linear prohibitions of [§3.2](#32-structural-rules-and-what-linearity-removes) as tests whose *failure* is the expected, passing outcome.
- **11 resource obligations** (`:547`) — the cost laws of [§8.5](#85-per-connective-resource-laws), e.g. `resource_tensor_required_additive`, `resource_whynot_requires_zero`, `resource_lolly_conservative`, plus the two substructural ones `resource_single_witness_no_double_spend` and `resource_duplicate_witness_allows_two_spends`.

The recorded run (see the testing-completeness discoveries note dated 2026-05-25) covers **643 827 bounded cases with zero counterexamples**, writing a JSON report and exiting non-zero on any failure. The Plus cost in Sage deserves a precise note, given in [§10.3](#103-the-plus-cost-semantics-across-models).

### 9.3 TLA⁺ protocol tier

Two layers of TLA⁺ models complement the Rocq proofs. At the *protocol* tier, `MultiSignerProtocol.tla` checks the multi-signer pre-charge/refund machine, including the conservation invariant `TotalRefundConservation` (Σ refunds + total cost = Σ charged) and `NoRefundCrossAttribution`. At the *connective* tier, six per-connective specs each check a characteristic property:

| Spec | Characteristic invariant | Meaning |
|------|--------------------------|---------|
| `PlusProtocol.tla` | `AdditiveChoiceDeterminism` | the chosen branch is fixed at wire-decode time |
| `WithProtocol.tla` | `AdditiveCoConservation` | only one branch's fuel is consumed |
| `BangProtocol.tla` | `BangPersistence` | a registered `!`-capability survives across invocations |
| `WhyNotProtocol.tla` | `WhyNotNoChargeWhenAbsent` | an absent optional witness consumes no fuel |
| `LollyProtocol.tla` | `LollyNoCreationExNihilo` | `σ_to` never appears without `σ_from` |
| `ThresholdProtocol.tla` | `QuorumExactness`/`QuorumThresholdConstraint` | an accepting set has ≥ k of N members, k ∈ [1,N] |

Per repository policy these model-checks are run locally, not in CI.

### 9.4 Rust runtime grounding

The runtime makes the algebra executable:

- `accounting/mod.rs` defines the `Sig` enum (`:821`) with per-variant linear-logic doc comments, `SignatureChannel::from_sig` (`:1097`) implementing the channel reflection of [§7.1](#71-the-channel-model-and-why-it-is-faithful), and `set_deploy_signatures` (`:605`) which folds N cosigner hashes into a **left-associated `Sig::And` tree** — the multi-signer `⊗` of [§3.3](#33-the-multiplicative-connectives--and-).
- `casper_message.rs` defines `min_required_for` (`:1471`), which mirrors `sig_algebra_min_required` clause-for-clause (Atom→1, Tensor|With→l+r, Plus→chosen branch, Bang→inner, Whynot→0, Lolly→from+to, Threshold→k), and `from_proto_cosigned_with_sig_algebra` (`:1302`), the dispatcher that routes an algebra to the right envelope constructor. Its three branches are: all-required → N-of-N `from_signed_data`; cost-0 (the `?`/`1` case, `min_required == 0` at `:1355`) → accept any *presented* signer; otherwise → `from_signed_data_threshold`.

### 9.5 The bug the corroboration caught

The corroboration is not a formality: the no-free-weakening obligation revealed a real defect. The threshold verifier `from_signed_data_threshold` (`signed.rs:241`) had, in an earlier form, tallied valid signatures in a loop that *skipped* empty-signature entries and counted only valid ones — so a signer presenting a **non-empty but invalid** signature could be silently ignored once the quorum was otherwise met, *while its `phlo_share` still counted toward the envelope total* `Σ phlo_share = phlo_limit`. That is precisely a *weakening*: discarding a presented (required) witness for free.

The fix merges verification into the share-tallying loop so that **every non-empty signature is verified, and any invalid one is rejected, before the quorum is checked** (`signed.rs:293`):

```rust
let hash = Signed::<A>::signature_hash(&signer.sig_algorithm.name(), serialized_data.clone());
if !signer.sig_algorithm.verify(&hash, &signer.sig, &signer.pk.bytes) {
    return Err(CosignedError::SignatureVerifyFailed { index: i, pk_hex: hex::encode(&signer.pk.bytes) });
}
valid_signers = valid_signers.saturating_add(1);
```

The regression test `cosigned_threshold_rejects_non_empty_invalid_signature_even_when_quorum_met` (`signed.rs:789`) locks it in: with three signers (two valid, one non-empty-but-invalid) and threshold 2, the envelope is rejected with `SignatureVerifyFailed` even though the quorum is otherwise satisfied. The linear-logic "no weakening" theorem had no runtime counterpart until this fix; the multi-modal discipline is what surfaced the gap.

---

## 10. Scope, Limitations, and Honesty

This section states precisely what is and is not established, so no claim is overread.

### 10.1 The `dill` fragment caveat

The `dill` relation ([§4.3](#43-the-repos-dill-relation)) is a **single-conclusion, introduction-flavored fragment** of DILL, not full DILL:

- there is **no `dill_cut` constructor** (cut is studied admissibly and only at the channel layer, `cut_admissible`, [§7.8](#78-cut-admissibly));
- there are **no left rules** for `⊕`, `&`, `⊗`;
- the `!`-rule `dill_unrestricted` **fuses** dereliction-from-`Γ` with `!`-introduction rather than providing the standard separate promotion/dereliction/contraction/weakening rules;
- `dill_whynot_intro` is **unconditional** (`Γ ; · ⊢ ?f` for any `f`), modeling "optional" by outright discard rather than threading a `?`-context.

What the fragment proves rigorously is the *resource behaviour* of each connective and the two substructural prohibitions ([§8](#8-substructural-guarantees-no-double-spend-no-free-weakening)) — sufficient for the cost-accounting claims of this document, but not a metatheory (e.g. cut-elimination, normalization) of full DILL.

### 10.2 The channel-layer distributivity caveat

The canonical linear-logic distributivity `σ ⊗ (τ ⊕ ρ) ≡ (σ ⊗ τ) ⊕ (σ ⊗ ρ)` does **not** hold under the channel-as-multiset semantics, because the right-hand side duplicates `σ`. The counterexample (from the source comment at `LLIdentities.v:230`) is `σ = [1]`, `τ = [2]`, `ρ = [3]`: the left side reflects to `[1,2,3]` but the right side to `[1,2,1,3]`. Only the weaker *containment* `tensor_over_plus_subset_lhs_in_rhs` (`:244`) — every atom of the left appears in the right — is provable at this layer. Genuine distributivity is enforced at the verifier-dispatch layer, where presenting `σ ⊗ (τ ⊕ ρ)` consumes `σ` once and exactly one of `{τ, ρ}`.

### 10.3 The Plus cost semantics across models

`⊕` is the one connective whose cost model differs *intentionally* between the artifacts, and the difference is sound:

- Rocq `sig_algebra_min_required` and `ll_required_units`, and Rust `min_required_for`, all use the **`sig_choice`-tagged / `chosen_branch` committed branch** (`CostAccountedSyntax.v:259`, `casper_message.rs:1499`). This is the cost the signer actually commits to.
- The runtime `Sig::Plus(left, right)` carries **no** branch tag — its channel *reflection* is the branch-agnostic union of both branches (`accounting/mod.rs:1135`), because the verifier reads the branch witness from the wire envelope.
- The Sage general-purpose `required_units` models `⊕` as **`min(left, right)`** (`ll_identity_search.sage:127`) — the *cheapest* achievable branch, a sound lower bound on what a signer could commit to — whereas the branch-specific Sage `plus_required_units(·, branch)` (`:137`) selects the committed branch and is what the dedicated obligation `resource_plus_branch_required_units` checks against Rocq/Rust.

These agree on the branch-specific obligation and are jointly sound: the committed-branch cost (Rocq/Rust) is the *actual* cost, and the `min` (Sage) is a *lower bound* on it, useful for adversarial reasoning ("what is the least a signer could pay").

### 10.4 Operational reduction is elsewhere

This document is *static* resource accounting. The operational reduction of authorized deploys (the genuine `σ ⊗ (σ ⊸ τ) ⊢ τ` that consumes `σ`; bisimulation; confluence) lives in `RhoReduction.v` / `Bisimulation.v` and is the subject of the [verification companion](cost-accounted-rho-verification.md). The runtime mechanisms for *bounded* `!`-reuse and the `⊸` transformer live in the on-chain `rho:system:capabilities` registry.

### 10.5 Trust base

The Rocq development depends only on the Rocq 9.1.1 kernel and standard library; the linear-logic modules contain **no `Axiom` and no `Admitted`**, a property the proof-hygiene gate `scripts/check-cost-accounted-rho-proofs.sh` enforces (it greps for incompletion markers and runs `Print Assumptions` on the headline theorems, including all the `dill_*` and `ll_*` results). The Sage reference and the Rust runtime are *corroborating* models, not part of the proof's trust base; the TLA⁺ models establish finite-state reachability properties only.

---

## 11. Cross-References and Further Reading

| Topic | Where |
|-------|-------|
| Encoding cost accounting in pure rho; bisimulation, confluence, token conservation | [cost-accounted-rho-verification.md](cost-accounted-rho-verification.md) |
| The `Sig`/`Token`/`SignedProcess` types, the metering kernel, the deploy path | [cost-accounting-migration.md](cost-accounting-migration.md) |
| Adversary model; where no-free-weakening sits as a security vector | [cost-accounting-threat-model.md](cost-accounting-threat-model.md) |
| Operational scenarios (cosigner/threshold use cases) with formal + test anchors | [cost-accounting-use-cases.md](cost-accounting-use-cases.md) |
| The bounded-exhaustive search program of which `ll_identity_search.sage` is a part | [cost-accounting-search-horizon.md](cost-accounting-search-horizon.md) |
| The LL identity R/P/E/S coverage matrix (Rocq / proptest / example / Sage) | `docs/discoveries/2026-05-25-phase-4-testing-completeness.md` |

Source files: `formal/rocq/cost_accounted_rho/theories/{CostAccountedSyntax,LinearLogicResources,LLIdentities}.v`; `formal/sage/cost_accounting/ll_identity_search.sage`; `formal/tlaplus/cost_accounted_rho/*Protocol.tla`; `rholang/src/rust/interpreter/accounting/mod.rs`; `models/src/rust/casper/protocol/casper_message.rs`; `crypto/src/rust/signatures/signed.rs`.

---

## 12. References

<a id="ref-1"></a>[1] J.-Y. Girard, "Linear logic," *Theoretical Computer Science*, vol. 50, no. 1, pp. 1–101, 1987. [doi:10.1016/0304-3975(87)90045-4](https://doi.org/10.1016/0304-3975(87)90045-4)

<a id="ref-2"></a>[2] A. Barber, "Dual Intuitionistic Linear Logic," Technical Report ECS-LFCS-96-347, Laboratory for Foundations of Computer Science, University of Edinburgh, 1996. Available: <http://www.lfcs.inf.ed.ac.uk/reports/96/ECS-LFCS-96-347/>

<a id="ref-3"></a>[3] P. N. Benton, "A mixed linear and non-linear logic: Proofs, terms and models," in *Computer Science Logic (CSL 1994)*, Lecture Notes in Computer Science, vol. 933, Springer, pp. 121–135, 1995. [doi:10.1007/BFb0022251](https://doi.org/10.1007/BFb0022251)

<a id="ref-4"></a>[4] L. Caires and F. Pfenning, "Session Types as Intuitionistic Linear Propositions," in *CONCUR 2010 — Concurrency Theory*, Lecture Notes in Computer Science, vol. 6269, Springer, pp. 222–236, 2010. [doi:10.1007/978-3-642-15375-4_16](https://doi.org/10.1007/978-3-642-15375-4_16)

<a id="ref-5"></a>[5] S. Mac Lane, *Categories for the Working Mathematician*, Graduate Texts in Mathematics, vol. 5, Springer New York, 1978. [doi:10.1007/978-1-4757-4721-8](https://doi.org/10.1007/978-1-4757-4721-8)

<a id="ref-6"></a>[6] L. G. Meredith and M. Radestock, "A reflective higher-order calculus," *Electronic Notes in Theoretical Computer Science*, vol. 141, no. 5, pp. 49–67, 2005. [doi:10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016)

<a id="ref-7"></a>[7] R. Milner, *Communicating and Mobile Systems: the π-Calculus*, Cambridge University Press, 1999. ISBN 978-0-521-65869-0.

<a id="ref-8"></a>[8] L. G. Meredith, "Translating Cost-Accounted Rho Calculus Back to the Pure Rho Calculus: Toward Rearchitecting Phlogiston Accounting in Rholang," F1R3FLY.io, April 2026. Mechanized in Rocq at `formal/rocq/cost_accounted_rho/`; see the verification companion, [*Formal Verification of Cost-Accounted Rho Calculus*](cost-accounted-rho-verification.md).
