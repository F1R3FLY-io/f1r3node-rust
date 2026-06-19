# Formal Verification of Cost-Accounted Rho Calculus

**A Mechanized Proof in Rocq 9.1.1 that Phlogiston Accounting
Is Faithfully Encodable within Pure Rho Calculus**

*Companion to: L. Gregory Meredith,
"Cost-Accounted Rho Calculus: A Spectral Decomposition of Phlogiston,"
May 2026 [4].*

---

## Abstract

The rho calculus [1] is a reflective higher-order process calculus that
serves as the formal core of the Rholang smart-contract language [3].
Its production deployment — the F1R3FLY / RChain platform — gates every
deploy with a cost-accounting layer called the **phlogiston** (phlo)
system, which to date has been specified as an external extension of the
calculus carrying digital signatures and token-bearing rewrite rules.
Meredith [4] shows that this cost-accounting layer can be translated
back into the pure rho calculus via a compositional encoding: signatures
become channels, tokens become messages on those channels, and signed
processes must consume fuel before they can communicate.

This article presents a machine-checked proof of that claim, mechanized
in **Rocq 9.1.1** across 32 modules and 25,776 lines of development, and
complements it with a **TLA+** finite-state model verified by TLC and
selectively cross-checked by Apalache. The headline results include
contextual forward reachability
(`translation_faithful`, with the precision boundary stated in
Section 6.1), strong bisimulation
(`translation_strong_bisimilar_generic`), per-step reverse simulation
(`gate_per_step_reverse_generic`), recursive whole-system backward
reflection for the implementation metering relation
(`well_reflected_backward_reflection`), token conservation
(`token_monotone_reachable`), fuel-gate safety
(`fuel_gate_stuck_isolated`), strong normalization
(`ca_strongly_normalizing`), local and full confluence
(`ca_local_confluence`, `ca_confluent`) via a constructive rendering of
Newman's lemma, cost determinism (`ca_cost_deterministic`), step
determinism for single-token systems (`ca_step_deterministic`), and a
axiom-free forward weak-barb propagation from a replicated body to both
the primitive replicator and Meredith's reflective replication encoding
(`preplicate_bang_encoding_body_barbs_sound`,
`replication_encoding_forward_barb_sound`).
All 967 `Qed.`/`Defined.` proof terms are discharged without any
`Admitted`, `admit`, or `Axiom`; the trust base consists of the
Rocq 9.1.1 kernel, the Rocq Stdlib, and one `hash_process`
encoding parameter with three explicit section hypotheses (Section 12.1).
The consensus-critical results
(`token_monotone_*`, `ca_cost_deterministic`, `ca_step_deterministic`,
`fuel_events_consumed_perm`) are unconditional and report
`Closed under the global context` under `Print Assumptions`.

**Claim boundary.** This document is the repo-local verification record.
It does not modify the external paper. Its implementation-facing claims
are aligned with the staged `f1r3node-rust` cost-accounting replacement.
Where a historical theorem name is broader than its statement, the
statement is authoritative: `translation_faithful` proves contextual
reachability of a pure-rho witness, not syntactic equality with the
translated target state; `translation_backward_soundness` proves a
source-level fuel bound, not full reflection of arbitrary translated
pure-rho reductions back to `ca_step` for the legacy compositional
`P_tr` image. Full backward reflection is instead proved for the
recursive metered implementation relation `well_reflected`.

---

## Table of Contents

1. [Introduction](#1-introduction)
2. [Glossary of Symbols and Key Terms](#2-glossary-of-symbols-and-key-terms)
3. [The Pure Rho Calculus](#3-the-pure-rho-calculus)
4. [The Cost-Accounted Extension](#4-the-cost-accounted-extension)
5. [The Compositional Translation](#5-the-compositional-translation)
6. [Headline Theorems](#6-headline-theorems)
7. [Proof Architecture](#7-proof-architecture)
8. [Deep Dive: Key Proof Techniques](#8-deep-dive-key-proof-techniques)
9. [Mathematical Proofs](#9-mathematical-proofs)
10. [TLA+ Correctness Model](#10-tla-correctness-model)
11. [Module Reference](#11-module-reference)
12. [Assumptions and Trust Base](#12-assumptions-and-trust-base)
13. [References](#13-references)

---

## 1. Introduction

### 1.1 Problem Statement and Context

The **rho calculus** (ρ-calculus) is a reflective higher-order process
calculus in which channels are the quoted codes of processes, and
processes may be recovered from channels by dequotation [1]. It is a
variant of Milner's π-calculus [2] distinguished by reflection — names
are quoted processes, and processes can be dereferenced from names —
and serves as the formal core of **Rholang**, the smart-contract
language of the RChain / F1R3FLY platform [3].

In production, every Rholang deploy is gated by a cost-accounting
mechanism called the **phlogiston** (phlo) system: deploys carry digital
**signatures**, each associated with a **token balance**, and execution
consumes phlo proportionally to the resources used. Historically, this
layer has lived *outside* the calculus — as a privileged runtime
extension of Rholang's evaluator that intercepts communication events
and debits a balance held in a mutable counter. The asymmetry between
the two evaluation orders supported by the runtime (produce-first vs.
consume-first) has been observed to introduce order-dependent cost
divergence, forcing either scheduling serialization or dual-cost
reconciliation at the validator layer.

Meredith [4] proposes a structural fix: the cost-accounting layer can be
**translated back** into the pure rho calculus via a compositional
encoding. Signatures become channels, tokens become messages on those
channels, and signed processes must consume *fuel* — a token output on
the signature channel — before they can communicate. The resulting
translation is compositional on systems, lives entirely within the
reflective syntax of [1], and offers the prospect of cost determinism as
a *theorem* about the calculus rather than an invariant the runtime
must engineer.

### 1.2 Contribution

This article proves that claim. Concretely, we contribute:

1. A complete **Rocq 9.1.1** mechanization of the cost-accounted rho
   calculus, its compositional translation back into pure rho, and the
   infrastructure (`Split`, `Join`, persistent mediators) required to
   discharge the paper's five reduction rules (Section 5). The
   development spans 32 modules and 25,776 lines, with 967 `Qed.` or
   `Defined.` proof obligations and zero `Admitted` / `admit` /
   `Axiom` declarations.

2. Machine-checked **contextual forward reachability**
   (`translation_faithful`, aliased as
   `translation_contextual_reachability`),
   **strong bisimulation** (`translation_strong_bisimilar_generic`),
   **per-step reverse simulation** (`gate_per_step_reverse_generic`),
   and **recursive whole-system backward reflection**
   (`well_reflected_backward_reflection`) theorems. The gate theorems
   are generic over atomic and compound signatures with arbitrary
   nesting; the whole-system theorem applies to the implementation
   metering relation that re-gates every continuation (Section 6).

3. A collection of **consensus-critical** unconditional theorems that
   go beyond the claims sketched in [4]: token conservation
   (`token_monotone_step`, `token_monotone_reachable`,
   `token_strictly_decreases`), strong normalization
   (`ca_strongly_normalizing`, `ca_max_steps_bound`), local and full
   confluence (`ca_local_confluence`, `newman`, `ca_confluent`,
   `ca_normal_form_unique`), cost determinism
   (`ca_cost_deterministic`), step determinism for single-token
   systems (`ca_step_deterministic`, `single_token_path_unique`), and
   fuel-event multiset determinism (`fuel_events_consumed_perm`).

4. Independent **TLA+** finite-state correctness models (Section 10),
   verified by TLC across 25 specifications and cross-checked through
   Apalache for the typed threat/search-frontier models, plus the
   validator behavioral contract proven deductively by TLAPS in
   `formal/tlaplus/validator/Validator.tla` (Section 10.7): the four core
   protocol/scheduling models up to 12,960 distinct states, plus
   runtime-budget replay, threat-model, search-frontier, and typed
   mergeable-channel models that check implementation-facing invariants —
   catching specification bugs that a universally-quantified proof could
   still miss.

5. Machine-checked **replication encoding support** for the persistent
   infrastructure used by the translation: Meredith's reflective
   encoding performs the expected one-step unfold
   (`bang_encoding_unfolds`), and every weak input/output barb of the
   body propagates to both `PReplicate body` and
   `bang_encoding x body` (`preplicate_bang_encoding_body_barbs_sound`,
   Section 6.5; summarized by `replication_encoding_forward_barb_sound`,
   Section 6.6). The development intentionally does not assume a
   bidirectional projection from wrapper behavior back to a single body
   copy, because that is stronger than the standard replication law and
   is not required by the cost-accounting correctness chain.

The paper [4, §6.4 Implementation Path] anticipates a Lean 4 mechanization of
the translation; the present development fulfils that role in Rocq and
extends it with the consensus-critical theorems of item (3) and the
replication-encoding support of item (5).

### 1.3 Related Work

The rho calculus was introduced by Meredith and Radestock [1] as a
reflective refinement of Milner's π-calculus [2]; this article uses [1]
as the canonical source for the operational semantics and for the
reflective encoding of replication (Sections 6.6 and 12.3). Sangiorgi
and Walker [5] provide the foundational theory of bisimulation used in
our strong-bisimilarity proofs, including the relationship between
strong bisimilarity and barbed congruence invoked in Section 12.3 and
the classical "!P is strongly bisimilar to P ∣ !P" theorem
([5, Theorem 2.2.8]) that gates the reverse direction of our weak
barbed equivalence (Section 6.6). The bisimulation-up-to-expansion
technique of [5, §2.4.3] is identified as the path for a future
direct mechanization of those results. The cost-accounted calculus
and its compositional translation come from Meredith [4]; this article
is the machine-checked companion to that paper.

### 1.4 Outline

Section 2 fixes notation and defines every symbol used in the remainder
of the document. Section 3 recalls the pure rho calculus — syntax,
substitution, structural equivalence, and operational semantics —
following [1]. Section 4 introduces the cost-accounted extension of [4],
its five rewrite rules, and the token-conservation lemma. Section 5
presents the compositional translation `N⟦·⟧`, `T⟦·⟧`, `P⟦·⟧`, `S⟦·⟧`
that maps cost-accounted systems back into the pure calculus, along
with the `Split` and `Join` mediator processes. Section 6 states the
headline theorems (contextual forward reachability, strong bisimulation, per-step
reverse, recursive whole-system reflection, and token conservation);
Section 7 describes the three-layer
proof architecture of the Rocq development; Section 8 dives into the
key proof techniques (coinductive bisimulation, heads-list permutation,
signature-size channel distinctness, stuck-process arguments). Section 9
gives end-to-end mathematical proofs of every claim. Section 10 presents
the complementary TLA+ model-checking results. Sections 11 and 12
document module traceability and the trust base. Section 13 lists
references.

### 1.5 Verified Properties (Detail)

Expanding on the contributions listed in Section 1.2:

| Property                                  | Headline Theorem                            | Meaning                                                                                                                                                    |
|-------------------------------------------|---------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **Contextual forward reachability**       | `translation_faithful` / `translation_contextual_reachability` | Every cost-accounted step has a pure-rho witness reachable from the translated image plus any required closed Split context                                |
| **Strong bisimulation**                   | `translation_strong_bisimilar_generic`      | The translated fuel gate is operationally transparent: the gated process behaves identically to the original                                               |
| **Per-step reverse simulation**           | `gate_per_step_reverse_generic`             | The fuel gate's reduction is fully determined: any first step from the gated system reaches the canonical final state                                      |
| **Whole-system backward reflection**      | `well_reflected_backward_reflection`        | Every pure-rho step from the recursive metered implementation target reflects to a real `ca_step` and a recursively metered successor                     |
| **Token conservation**                    | `token_monotone_reachable`                  | Fuel is never created; every cost-accounted step strictly decreases total token count                                                                      |
| **Fuel-gate safety**                      | `FuelGateSafety` module                     | No signed process can communicate without first acquiring fuel from its signature channel                                                                  |
| **Strong normalization**                  | `ca_strongly_normalizing`                   | Every cost-accounted system is well-founded under `ca_step`; no infinite reduction sequence exists                                                         |
| **Local confluence**                      | `ca_local_confluence`                       | Any two one-step divergences from the same state can be joined in one step each (the diamond property)                                                     |
| **Full confluence**                       | `ca_confluent`                              | Every divergence can be joined, via Newman's lemma applied to well-founded `ca_step` (Coquand 1994, constructive)                                          |
| **Cost determinism**                      | `ca_cost_deterministic`                     | Two validators reaching any terminal state from the same source agree on the total fuel consumed, regardless of order                                      |
| **Step determinism (single-token)**       | `ca_step_deterministic`                     | When at most one `SToken` leaf is in flight, `ca_step` has a unique successor — justifies ordered fuel-event hashing                                       |
| **Forward barb propagation**              | `preplicate_bang_encoding_body_barbs_sound` / `replication_encoding_forward_barb_sound` | Every weak input/output barb of `body` lifts to both the primitive `PReplicate body` and the reflective `bang_encoding x body` wrappings, axiom-free |

The original gate-level headline properties (contextual forward
reachability, strong bisimulation, per-step reverse simulation) are
**fully generic** over the signature type: they cover the unit signature
`()`, hash signatures `hash(σ)`, and compound signatures `s₁ & s₂` with
arbitrary nesting. Whole-system backward reflection is stated over the
recursive metered implementation relation `well_reflected`, not over the
legacy raw `S_tr` image.

The results above fall into four pedigree classes:

(a) **Direct mechanizations of paper claims.** Contextual forward reachability,
strong bisimulation, per-step reverse simulation, and fuel-gate
safety mechanize the per-rule simulation arguments and the
capability-security observations sketched in [4, §4 and §5].

(b) **Formal verifications of properties of the paper's algorithm.**
The token-chain encoding `T⟦σ:T'⟧ = N⟦σ⟧!(T⟦T'⟧)` (paper [4, Appendix A]) is
*itself* the algorithm that guarantees sequential firing: at most one
token message sits on any signature channel at a time, and each
fuel-gate firing dequotes the next token into existence. Step
determinism (`ca_step_deterministic`) and single-token path
uniqueness (`single_token_path_unique`) — together with fuel-event
multiset determinism (`fuel_events_consumed_perm`) — *verify* this
property; they do not introduce the ordering, which is paper-original.

(c) **Proof-original extensions beyond the paper.** Strong
normalization, local confluence (the diamond), full confluence (via
Newman's lemma), normal-form uniqueness, cost determinism for arbitrary
parallel deploy compositions, and recursive whole-system backward
reflection are not stated or sketched in [4]; they are introduced and
proved in this development.

(d) **Replication-support results.** The one-step reflective unfold
(`bang_encoding_unfolds`) and body-to-wrapper weak-barb propagation
(`preplicate_bang_encoding_body_barbs_sound`,
`replication_encoding_forward_barb_sound`) justify the persistent
mediator design used by the translation without adding any axiom to
the proof context.

### 1.6 Scale

| Metric                                           | Value                                                      |
|--------------------------------------------------|------------------------------------------------------------|
| Rocq source files                                | 32 modules                                                 |
| Total lines of Rocq                              | 25,776                                                     |
| Proven lemmas and theorems (`Qed.` / `Defined.`) | 967                                                        |
| `Admitted` / `admit`                             | **0**                                                      |
| Named `Axiom` declarations                       | **0**                                                      |
| Proof assistant                                  | Rocq (Coq) 9.1.1 (also typechecks under 9.1.0)             |
| Explicit assumptions                             | 1 encoding parameter + 3 section hypotheses (see [Section 12](#12-assumptions-and-trust-base)) |

The `hash_process` parameter and its three section hypotheses scope only the *translation-side* theorems
that reason about hash-derived signature channels (contextual forward reachability,
per-step reverse, atomic and compound bisimulation, fuel-gate safety
for hashed signatures). There are no theorem-level axioms in the
development. The *consensus-side* headline results —
`ca_strongly_normalizing`, `ca_local_confluence`, `ca_confluent`,
`ca_normal_form_unique`, `ca_cost_deterministic`, `ca_step_deterministic`,
`single_token_path_unique`, `token_monotone_step` /
`token_monotone_reachable` / `token_strictly_decreases`, and
`fuel_events_consumed_perm` — all report `Closed under the global context`
under `Print Assumptions` (verified live; see Section 12.1 for the
per-theorem dependency table). No consensus-critical result depends
on any axiom from Section 12.2.1.

### 1.7 Module Dependency Graph

Arrows point from dependency to dependent (`A ──► B` means "module `B`
imports module `A`"). The 32 modules organize into seven dependency
tiers corresponding to the proof layers of §7.1.

```
                         ┌─────────────┐
                         │  RhoSyntax  │
                         └──────┬──────┘
                 ┌──────────────┼────────────────────────────────┐
                 │              │                                │
    ┌────────────▼─────────┐    │            ┌───────────────────▼────────┐
    │ StructEquivInversion │    │            │    CostAccountedSyntax     │
    └────────────┬─────────┘    │            └───────────────────┬────────┘
                 │              │                                │
    ┌────────────▼─────────┐    │            ┌───────────────────▼────────┐
    │   StructEquivHeads   │    │            │   CostAccountedReduction   │
    └────────────┬─────────┘    │            └───────────────────┬────────┘
                 │              │                                │
                 └──────┬───────┘                                │
                        │                                        │
               ┌────────▼───────┐                                │
               │  RhoReduction  │───────────┐                    │
               └────┬─────┬─────┘           │                    │
                    │     │          ┌──────▼──────────┐         │
                    │     │          │ WeakBarbedEquiv │         │
                    │     │          └─────────────────┘         │
           ┌────────▼─────▼────────────────┐                     │
           │          Translation          │                     │
           └──┬─────┬──────────┬───────────┘                     │
              │     │          │                                 │
    ┌─────────┘     │          └────────┐                        │
    │               │                   │                        │
┌───▼──────┐ ┌──────▼────────┐  ┌───────▼───────────────────┐    │
│ TokenCon.│ │ FuelGateSafety│  │  TranslationFaithfulness  │◄───┘
└──────────┘ └───────────────┘  └──────────────┬────────────┘
                                               │
                                      ┌────────▼────────┐
                                      │  Bisimulation   │
                                      └────────┬────────┘
                                               │
                                      ┌────────▼──────┐
                                      │  Replication  │
                                      └───────────────┘
```

**Edges shown are a representative subset chosen for clarity; several
direct-import edges are omitted. In particular:**

- `CostAccountedReduction` → `TokenConservation`, `FuelEventDecomposition`,
  `Confluence` (the cost-determinism stack is drawn separately below).
- `TokenConservation` → `Settlement` → `SlashingComposition`
  and `MergeableChannelAccounting`
  (post-evaluation fee settlement and slash-system composition are drawn
  separately from reduction and translation).
- `WeakBarbedEquiv` → `Replication` (the weak-barb framework consumed
  by the replication-encoding equivalence of §6.6).
- Multiple Layer-1 imports descend directly into `Bisimulation`
  (`RhoSyntax`, `StructEquivInversion`, `StructEquivHeads`,
  `RhoReduction`) and into `Replication` (the same four plus
  `WeakBarbedEquiv`), in addition to the indirect paths shown.
- `TranslationFaithfulness` also imports `CostAccountedSyntax`,
  `RhoReduction`, and others not drawn individually.

See §11.1 File Listing for the complete per-module dependency set.

**Cost-determinism stack** (built on top of `TokenConservation`):

```
  TokenConservation ──► StrongNormalization ──► Confluence ──► StepDeterminism
                                                     │
                                                     ▼
                                          ca_cost_deterministic
                                            (Confluence.v:474)
```

**Auxiliary modules** (independent leaves):

```
  CostAccountedReduction ──► FuelEventDecomposition   (event multiset determinism)
  CostAccountedSyntax    ──► ChannelSeparation        (signature channels are quotations)
  CostAccountedSyntax    ──► RuntimeBudgetRefinement  (coalesced runtime budget and replay trace)
  TokenConservation      ──► Settlement ──► SlashingComposition ──► UseCaseAdequacy
                                      └────► MergeableChannelAccounting ──┘
                                      (fee settlement, slash-system composition,
                                       typed mergeable channels, and proof-backed
                                       use-case anchors)
  RhoSyntax              ──┘
```

**Critical paths:**

- **Consensus stack** (Layers 1–3 of §7.1):
  `RhoSyntax → RhoReduction → Translation → TranslationFaithfulness → Bisimulation`.
- **Replication-support stack** (Layers 4–5 of §7.1):
  `RhoReduction → WeakBarbedEquiv → Replication` and
  `Bisimulation → Replication` (Replication draws from both the main
  consensus stack and the new weak-barb framework).

**Leaf status.** `Replication` is a leaf of the DAG — no other module
imports it. Its replication-specific proof infrastructure therefore
cannot propagate into any consensus-critical result; this
non-propagation is an immediate consequence of the dependency-graph
shape.

---

## 2. Glossary of Symbols and Key Terms

### 2.1 Process-Algebraic Notation

| Symbol         | Name                  | Meaning                                                         |
|----------------|-----------------------|-----------------------------------------------------------------|
| `0`            | Nil / stopped process | Does nothing                                                    |
| `for(y ← x) P` | Input prefix          | Wait on channel *x*, bind received name to *y*, continue as *P* |
| `x!(Q)`        | Output                | Send the code of *Q* on channel *x*                             |
| `P ∣ Q`        | Parallel composition  | *P* and *Q* run concurrently                                    |
| `*x`           | Dequotation           | Recover the process whose code is channel *x*                   |
| `@P`           | Quotation             | Turn process *P* into a channel name                            |

### 2.2 Structural Equivalence

| Symbol         | Name                   | Definition                                                     |
|----------------|------------------------|----------------------------------------------------------------|
| `≡` (or `≡_S`) | Structural equivalence | Smallest congruence making `(proc, ∣, 0)` a commutative monoid |
| `≡_N`          | Name equivalence       | Induced on names: `@P ≡_N @Q` iff `P ≡ Q`                      |

The three axioms:

       P ∣ 0       ≡  P                        (identity)
       P ∣ Q       ≡  Q ∣ P                    (commutativity)
      (P ∣ Q) ∣ R  ≡  P ∣ (Q ∣ R)              (associativity)

### 2.3 Reduction

| Symbol | Name                | Definition                                                                                               |
|--------|---------------------|----------------------------------------------------------------------------------------------------------|
| `⇝`    | Single rho-step     | One application of COMM + contextual closure                                                             |
| `⇝*`   | Rho-reachable       | Reflexive-transitive closure of `⇝`                                                                      |
| `~~`   | Strong bisimilarity | Coinductive bidirectional step-matching (see [Section 8.1](#81-coinductive-bisimulation-via-cofixpoint)) |

### 2.4 Cost-Accounting Symbols

| Symbol    | Name                     | Definition                                              |
|-----------|--------------------------|---------------------------------------------------------|
| `s`       | Signature                | Digital identity: `()`, `hash(σ)`, or `s₁ & s₂`         |
| `T`       | Token                    | Fuel balance: empty `()` or gate `s:T`                  |
| `P^s`     | Signed process           | Process `P` annotated with signature `s`                |
| `S₁ ∥ S₂` | System parallel          | Parallel composition of cost-accounted systems          |
| `⤳`       | Cost-accounted step      | One fuel-consuming COMM                                 |
| `⤳*`      | Cost-accounted reachable | Reflexive-transitive closure of `⤳`                     |
| `‖S‖`     | Token count              | `system_token_count(S)`: total fuel units in system *S* |

### 2.5 Translation Symbols

| Symbol | Rocq Name | Domain → Codomain   | Purpose                                   |
|--------|-----------|---------------------|-------------------------------------------|
| `N⟦·⟧` | `N_tr`    | `sig → name`        | Signatures become channel names           |
| `K⟦·⟧` | `T_tr`    | `token → proc`      | Token-stack translation: tokens become messages (outputs). The repo's `T_tr` realizes the paper's `K⟦·⟧`; the paper reserves `T⟦·⟧` for the signed-term translation. |
| `P⟦·⟧` | `P_tr`    | `proc × sig → proc` | Signed processes become fuel-gated inputs |
| `S⟦·⟧` | `S_tr`    | `system → proc`     | Compositional system translation          |

### 2.6 Key Terms

| Term                  | Definition                                                                                                                                                                                     |
|-----------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **Phlogiston** (phlo) | Rholang's gas/fuel accounting unit, analogous to Ethereum's gas                                                                                                                                |
| **Fuel gate**         | An input prefix on a signature channel that blocks execution until a token is consumed                                                                                                         |
| **Split**             | Mediator process: decomposes a combined token on channel `N⟦s₁ & s₂⟧` into separate atomic tokens on `N⟦s₁⟧` and `N⟦s₂⟧`                                                                       |
| **Join**              | Inverse of Split: combines two atomic tokens into a compound token                                                                                                                             |
| **Head**              | A top-level process constructor visible above all `PPar` nodes (i.e., a `PInput`, `POutput`, or `PDeref` at the parallel-composition surface)                                                  |
| **Head count**        | Number of heads in a process; preserved by structural equivalence                                                                                                                              |
| **Stuck process**     | A process with no top-level `PInput`/`POutput` heads, unable to participate in any COMM rule                                                                                                   |
| **De Bruijn index**   | A nameless representation of bound variables: each variable is a natural number counting the enclosing binders between it and its binding site [5]                                             |
| **Locally nameless**  | A binding representation that uses de Bruijn indices for bound variables and quoted processes for free names                                                                                   |
| **Lifting**           | The operation `lift_proc(d, c, P)` that increments all de Bruijn indices ≥ c by d, shifting variables past newly introduced binders                                                            |
| **Coinductive**       | A Rocq/Rocq-stdlib type constructor (`CoInductive`) whose inhabitants may be built from non-well-founded patterns, used here to express strong bisimilarity (§8.1)                             |
| **Cofixpoint**        | The term-level analogue of `Fixpoint` for coinductive types: a recursive term whose guardedness Rocq checks syntactically, used to construct an inhabitant of a coinductive proposition (§8.1) |
| **Guardedness**       | Rocq's syntactic criterion for productive cofixpoints: every recursive call must appear immediately under a constructor of the coinductive type (§8.1)                                         |

### 2.7 Replication and Observable Barbs

| Symbol / Term               | Name                             | Meaning                                                                                                                                                                                         |
|-----------------------------|----------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `!P`                        | Replication (Milner notation)    | Unbounded parallel self-composition; semantically `!P ≡ P ∣ !P`                                                                                                                                 |
| `PReplicate P`              | Rocq primitive replicator        | Inductive constructor in `RhoSyntax.v`; reduction rule `rs_replicate : PReplicate P ⇝ P ∣ PReplicate P`                                                                                         |
| `D_encoding x`              | Meredith–Radestock self-receiver | `for(y ← x){x⟨∣*y∣⟩ ∣ *y}` — input on *x* that re-posts the received name and its dereference                                                                                                   |
| `bang_encoding x P`         | Reflective replication encoding  | `x⟨∣D(x) ∣ P∣⟩ ∣ D(x)` — Meredith–Radestock's encoding of `!P` using only pure rho [1, §3]                                                                                                      |
| `P ↓ x`                     | Barb                             | *P* has a top-level input or output on channel *x* (§3.6); original conflated form in `RhoReduction.v`                                                                                          |
| `P ↓ᵢ x`                    | Input barb (receive observable)  | *P* has a top-level `PInput` on channel *x* (no output component; §3.6)                                                                                                                         |
| `P ↓ₒ x`                    | Output barb (send observable)    | *P* has a top-level `POutput` on channel *x* (no input component; §3.6)                                                                                                                         |
| `P ⇓ᵢ x`                    | Weak input barb                  | ∃ P'. `P ⇝* P' ∧ x ≡ₙ y ∧ P' ↓ᵢ y` — *P* can eventually exhibit an input barb on a channel name-equivalent to *x*                                                                               |
| `P ⇓ₒ x`                    | Weak output barb                 | Dual of weak input barb for outputs                                                                                                                                                             |
| `P ≈ₓ Q`                    | Weak barbed equivalence mod *x*  | For every *y* with `¬(x ≡ₙ y)`: `P ⇓ᵢ y ↔ Q ⇓ᵢ y` and `P ⇓ₒ y ↔ Q ⇓ₒ y`. Encodes "indistinguishable by observers restricted to non-*x* channels" (Section 6.6)                                  |
| **Visible channel**         |                                  | Any name *y* with `¬(x ≡ₙ y)` relative to a chosen hidden coordination channel *x*. Observables on visible channels count; observables on *x* are hidden                                        |
| **Hidden channel**          |                                  | The name-equivalence class of a designated coordination channel *x*; barbs on it do not count toward the equivalence relation `≈ₓ`                                                              |
| `only_replicate P B`        | Sole-replicate predicate         | Structural predicate stating that `PReplicate B` is the only nonzero-head constructor of *P*; every other head has `head_count = 0`. Analogue of `only_input`/`only_output`/`only_deref` (§8.6) |
| `name_not_free_in_proc x P` | Channel freshness                | *x* does not occur as the subject of any `PInput`, `POutput`, or `PDeref` anywhere in *P*, including under quotes. Required hypothesis for the encoding equivalence (Section 6.6)               |

> **Notation convention.** The subscripts `↓ᵢ` / `↓ₒ` and `⇓ᵢ` / `⇓ₒ` are used informally in running prose. The Rocq source instead names the four predicates `input_barb`, `output_barb`, `weak_barb_input`, `weak_barb_output`; the subscripted forms here read more naturally in mathematical exposition.

---

## 3. The Pure Rho Calculus

### 3.1 Syntax

Processes and names are mutually defined [1]:

        P, Q  ::=  0  ∣  for(y ← x) P  ∣  x!(Q)  ∣  P ∣ Q  ∣  *x
        x, y  ::=  @P

The rho calculus is distinguished from Milner's π-calculus [2] by
**reflection**: the quoting operator `@·` turns any process into a
channel name, and the dequoting operator `*·` recovers the process. This
eliminates the need for a separate namespace — channels *are* process
codes.

**Rocq encoding** (`theories/RhoSyntax.v:57`). The mechanization uses
mutually inductive types with **locally nameless** binding via de Bruijn
indices:

```
name  ::=  Quote(P)          ── @P: quotation of a process
         | NVar(n)           ── bound variable at de Bruijn index n

proc  ::=  PNil              ── 0: the stopped process
         | PInput(x, P)      ── for(y ← x) P  (y is NVar 0 in P)
         | POutput(x, Q)     ── x!(Q)
         | PPar(P, Q)        ── P | Q
         | PDeref(x)         ── *x: dequotation
```

The `PInput` constructor binds one name variable: inside the body *P*,
the received name is `NVar 0`. Any pre-existing variable `NVar k` in the
outer scope is shifted to `NVar (k+1)` via the **lifting** operation.

### 3.2 Substitution

Substitution replaces a de Bruijn variable with a name, adjusting
indices under binders:

    SUBSTITUTE(P, n, N):
        ── Replace every NVar(n) in P with name N.
        MATCH P WITH
        ∣ PNil           → PNil
        ∣ PInput(x, B)   → PInput(SUBST_NAME(x, n, N),
                                   SUBSTITUTE(B, n+1, N))
                             ── n+1 because PInput introduces a binder
        ∣ POutput(x, Q)  → POutput(SUBST_NAME(x, n, N),
                                    SUBSTITUTE(Q, n, N))
        ∣ PPar(P₁, P₂)   → PPar(SUBSTITUTE(P₁, n, N),
                                 SUBSTITUTE(P₂, n, N))
        ∣ PDeref(x)      → PDeref(SUBST_NAME(x, n, N))

The load-bearing property of the mechanization is:

> **Lemma** (`subst_lift_zero`).
>
>     ∀P, N. SUBSTITUTE(LIFT(P, 1, 0), 0, N) = P
>
> *Lifting by 1 at cutoff 0 and then substituting at index 0 are inverse
> operations.*

**Why this matters.** When a fuel gate fires via COMM, the body of the
input (which was lifted to cross the gate's binder) has its index-0
reference replaced by the received payload. The `subst_lift_zero` lemma
guarantees the original process *P* is recovered exactly. Every fuel-gate
firing proof in the formalization bottoms out at this lemma.

### 3.3 Structural Equivalence

Structural equivalence (`theories/RhoSyntax.v:719`) is the smallest
congruence containing the three commutative-monoid axioms and closing
under all process constructors. Two invariants are critical:

> **Theorem** (`head_count_se`).
>
>     P ≡ Q  ⟹  head_count(P) = head_count(Q)

> **Theorem** (`count_derefs_se`).
>
>     P ≡ Q  ⟹  count_derefs(P) = count_derefs(Q)

These allow deriving contradictions when candidate reduction partners
have incompatible head structures — the primary technique in the
per-step reverse simulation proofs.

### 3.4 Operational Semantics

The reduction relation (`theories/RhoReduction.v:68–93`) is defined by
four rules:

**COMM** — The fundamental communication step:

        for(y ← x) P  ∣  x!(Q)   ⇝   P{@Q/y}

An input and output on the **same** channel fire together. The input
receives @Q (the quoted code of Q), which is substituted for the bound
variable *y* in *P*.

**PAR** — Contextual closure:

               P ⇝ P'
           ──────────────
           P ∣ Q ⇝ P' ∣ Q

**STRUCT** — Closure under structural equivalence:

        P ≡ P'    P' ⇝ Q'    Q' ≡ Q
        ───────────────────────────
                   P ⇝ Q

**Reachability** (`theories/RhoReduction.v:106`) is the
reflexive-transitive closure:

        rr_refl:  P ⇝* P
        rr_step:  P ⇝ Q  ∧  Q ⇝* R  ⟹  P ⇝* R

### 3.5 Head Count and Stuck Processes

    HEAD_COUNT(P):
        MATCH P WITH
        ∣ PNil          → 0
        ∣ PInput(_, _)  → 1
        ∣ POutput(_, _) → 1
        ∣ PPar(P, Q)    → HEAD_COUNT(P) + HEAD_COUNT(Q)
        ∣ PDeref(_)     → 1

> **Theorem** (`rho_step_head_count_ge_two`).
>
>     P ⇝ P'  ⟹  head_count(P) ≥ 2

Every COMM requires at least one input head and one output head.
Processes with fewer than 2 heads are **stuck** — they cannot reduce.
Specific instances proven in the formalization include:

- `PNil_stuck`: 0 ⇝ P' is impossible
- `PInput_alone_stuck`: a lone for-comprehension cannot fire
- `POutput_alone_stuck`: a lone output cannot fire
- `PDeref_stuck`: a lone dequotation cannot fire

### 3.6 Observable Barbs — Conflated vs. Split

A **barb** is a top-level observable port: a process *P* barbs on a
channel *x* when *P* can, without further reduction, participate in a
COMM on *x* — either as the listener (an input barb) or as the speaker
(an output barb). Barbs are the basic atomic observations from which
behavioral equivalences (barbed bisimulation, weak barbed congruence)
are constructed; see [5, §2.4] for the classical treatment.

**Rocq encoding — original conflated form** (`theories/RhoReduction.v:206`).
The initial formalization treated input and output as a single predicate:

```coq
Inductive barb : proc → name → Prop :=
  | barb_input     : ∀ x P,   barb (PInput x P) x
  | barb_output    : ∀ x Q,   barb (POutput x Q) x
  | barb_par_l     : ∀ P Q x, barb P x → barb (PPar P Q) x
  | barb_par_r     : ∀ P Q x, barb P x → barb (PPar Q P) x
  | barb_replicate : ∀ P x,   barb P x → barb (PReplicate P) x.
```

A single `barb P x` cannot distinguish whether the witness is a
`PInput` or a `POutput` on *x*. For equivalences that must pair input-
observers only with inputs and output-observers only with outputs
(as required by the replication-encoding support theorem of Section 6.6),
this conflation is insufficient.

**Rocq encoding — split barbs** (`theories/RhoReduction.v:378`, added
for the replication-support work). The split form introduces two
separate inductive relations, one per direction:

```coq
Inductive input_barb : proc → name → Prop :=
  | input_barb_here      : ∀ x P,   input_barb (PInput x P) x
  | input_barb_par_l     : ∀ P Q x, input_barb P x → input_barb (PPar P Q) x
  | input_barb_par_r     : ∀ P Q x, input_barb P x → input_barb (PPar Q P) x
  | input_barb_replicate : ∀ P x,   input_barb P x → input_barb (PReplicate P) x.

Inductive output_barb : proc → name → Prop :=
  | output_barb_here      : ∀ x Q,   output_barb (POutput x Q) x
  | output_barb_par_l     : ∀ P Q x, output_barb P x → output_barb (PPar P Q) x
  | output_barb_par_r     : ∀ P Q x, output_barb P x → output_barb (PPar Q P) x
  | output_barb_replicate : ∀ P x,   output_barb P x → output_barb (PReplicate P) x.
```

Each has four constructors — the same three structural constructors as
the conflated `barb` plus one leaf on its side only. The original
`barb` predicate is kept as-is for backward compatibility; the two
forms are related by a decomposition lemma:

> **Lemma** (`barb_iff_input_or_output`, `theories/RhoReduction.v:391`).
>
>     barb P x  ⟺  input_barb P x  ∨  output_barb P x

**Correspondence table.** The three vocabularies line up as:

| Level           | Receive                         | Send                             |
|-----------------|---------------------------------|----------------------------------|
| Surface Rholang | `for(y ← x){ … }`               | `x!(Q)`                          |
| Paper notation  | `P ↓ᵢ x`                        | `P ↓ₒ x`                         |
| Rocq AST node   | `PInput x B` (`RhoSyntax.v:62`) | `POutput x Q` (`RhoSyntax.v:64`) |
| Rocq observable | `input_barb P x` (§3.6 above)   | `output_barb P x` (§3.6 above)   |

**Worked example.** Consider the Rholang process

        for(m ← ch_in){ out!(m) }  ∣  done!(42)

In terms of barbs:

- **Input barbs.** `input_barb` of this process on `ch_in` holds (via
  `input_barb_par_l` applied to `input_barb_here`). On `done` or
  `out`, the input-barb relation does *not* hold at the top level.
- **Output barbs.** `output_barb` on `done` holds (via
  `output_barb_par_r` applied to `output_barb_here`). On `ch_in` or
  `out`, it does not hold — the `out!(m)` is nested under the
  `PInput`-binder and is therefore not a top-level head.
- **Conflated.** `barb` holds on both `ch_in` (via the `PInput`) and
  `done` (via the `POutput`). The split form refines this by saying
  *which* direction each witness corresponds to.

**Structural equivalence transport.** Both split barbs are closed
under structural equivalence modulo name equivalence:

> **Lemma** (`input_barb_se_both`, `theories/Replication.v` Section 14.B).
>
>     P ≡ Q  ⟹  (input_barb P y  ⟹  ∃y'. y ≡ₙ y' ∧ input_barb Q y').
>
> *Symmetrically for output_barb.*

These transport lemmas are required by the weak-barb definitions
(Section 6.6), which close under both reachability and channel name
equivalence.

### 3.7 Representation Choice: §3.8 Sugar and the `system`/`proc` Layering

The Rocq syntax layers a pure-process `proc` (`RhoSyntax.v`:
`PInput`/`POutput`/`PPar`/`PDeref`) beneath a `system` sort
(`CostAccountedSyntax.v`: `SSigned`/`SToken`/`SPar`) that carries the
signing and token-stack metadata of §3.1. This is the "extension of pure
rho" modelling choice rather than the spec's native four-sort grammar in
which signed terms pervade the syntax. Under this layering, the spec's
§3.8 syntactic-sugar defining equations — uniform signing `{·}_s` and
the linear-transfer lollipop `⊸` — and the §3.2/§3.5 source-level
identities are discharged at the **source/translation level** (Option A,
proof-gated): each sugar form is given meaning by its Appendix-A image in
the pure calculus, and the desugared right-hand side is proven
structurally equivalent (`≡` on `proc`) to the sugar left-hand side
(`uniform_sugar_translation_equiv`, `lollipop_sugar_translation_equiv` in
`SyntacticSugar.v`; `sse_par_unit`, `token_decomp`, `sig_free_names` in
`SystemStructEquiv.v`). The lollipop desugars to a pair of nested
plain-signature fuel-gate layers, so it coexists with the compound-signature
authorization algebra without introducing a new signature constructor.
The native four-sort grammar — under which the §3.8 sugars become native
signed-term equalities — is available as a subsequent representation
migration that this development records but does not require. The
representation choice and both options are documented in detail in
**DR-17** (`cost-accounting-decision-records.md` §3.8 representation
choice).

---

## 4. The Cost-Accounted Extension

### 4.1 Extended Syntax (paper Section 3.1)

**Signatures** (`theories/CostAccountedSyntax.v:76`) — digital identities
under which processes are signed:

        s  ::=  ()               ── unit signature
              | hash(σ)          ── atomic signature from byte string σ
              | s₁ & s₂          ── compound (conjunction) of two signatures

**Tokens** (`theories/CostAccountedSyntax.v:96`) — fuel balances:

        T  ::=  ()               ── empty (no fuel remaining)
              | s:T              ── one unit of fuel on signature s,
                                    with remaining balance T

A token `s₁:(s₂:(s₃:()))` represents three fuel units, consumed
outermost-first. The **token size** counts the nesting depth: `‖()‖ = 0`
and `‖s:T‖ = 1 + ‖T‖`.

*Normalization vs. paper.* The paper's grammar [4, Def. 3.1] writes
`T ::= () | σ | σ:T`, permitting a bare-signature token without an
explicit continuation. The Rocq grammar uses only the two-clause
form above; a bare-`σ` token is interpreted as `σ:()` and folded into
the recursive case. The two presentations are denotationally
equivalent under `T⟦·⟧`: `T⟦σ⟧` and `T⟦σ:()⟧` both reduce to
`N⟦σ⟧!(0)`. The normalization simplifies the recursion principle
without affecting any rule or theorem.

**Systems** (`theories/CostAccountedSyntax.v:118`) — processes with
accounting metadata:

        S  ::=  P^s              ── process P signed under signature s
              | T                ── free token (fuel) in the system
              | S₁ ∥ S₂          ── parallel composition of systems

The **system token count** `‖S‖` is the sum of all token sizes in *S*:

    ‖P^s‖     = 0            ── signatures carry no fuel
    ‖T‖       = token_size(T)
    ‖S₁ ∥ S₂‖ = ‖S₁‖ + ‖S₂‖

### 4.2 Cost-Accounted Rewrite Rules (paper Section 3.6)

The rule numbers below follow the May-2026 spec §3.6 numbering; the April
draft labeled the two split-process rules (Rules 4/5) in the opposite
order — the rule set is identical. All five rules are variations on one
theme: a COMM is gated by consumption of a token whose signature matches
the communicating processes [4, §3.6]. They differ in whether the redex
is signed as a whole or split across signatures, and whether the token is
combined or split:

| Rule  | Redex shape                      | Token shape             | Fuel consumed |
|-------|----------------------------------|-------------------------|---------------|
| **1** | Whole redex, single sig s        | s:T                     | 1             |
| **2** | Whole redex, compound s₁ & s₂    | s₁:T₁ and s₂:T₂ (split) | 2             |
| **3** | Whole redex, compound s₁ & s₂    | (s₁ & s₂):T (combined)  | 1             |
| **5** | Split processes (P^{s₁}, Q^{s₂}) | (s₁ & s₂):T (combined)  | 1             |
| **4** | Split processes (P^{s₁}, Q^{s₂}) | s₁:T₁ and s₂:T₂ (split) | 2             |

The formal definitions (`theories/CostAccountedReduction.v:83`):

**Rule 1** *(single signature, whole redex)*:

    (for(y ← x) P ∣ x!(Q))^s ∣ s:T   ⤳   (P{@Q/y})^s ∣ T

**Rule 2** *(compound signature, whole redex, split tokens)*:

    (for(y ← x) P ∣ x!(Q))^{s₁ & s₂} ∣ s₁:T₁ ∣ s₂:T₂
        ⤳   (P{@Q/y})^{s₁ & s₂} ∣ T₁ ∣ T₂

**Rule 3** *(compound signature, whole redex, combined token)*:

    (for(y ← x) P ∣ x!(Q))^{s₁ & s₂} ∣ (s₁ & s₂):T
        ⤳   (P{@Q/y})^{s₁ & s₂} ∣ T

**Rule 5** *(split processes, combined token)*:

    (for(y ← x) P)^{s₁} ∣ (x!(Q))^{s₂} ∣ (s₁ & s₂):T
        ⤳   (P{@Q/y})^{s₁ & s₂} ∣ T

**Rule 4** *(split processes, split tokens)*:

    (for(y ← x) P)^{s₁} ∣ (x!(Q))^{s₂} ∣ s₁:T₁ ∣ s₂:T₂
        ⤳   (P{@Q/y})^{s₁ & s₂} ∣ T₁ ∣ T₂

Plus contextual closure under system parallel composition:

          S₁ ⤳ S₁'                  S₂ ⤳ S₂'
    ────────────────────       ────────────────────
    S₁ ∥ S₂  ⤳  S₁' ∥ S₂       S₁ ∥ S₂  ⤳  S₁ ∥ S₂'

#### 4.2.1 λ instance — R1 only (rigid contact, degenerate environment)

The five-rule family above is what the generic cost transform emits for rho's
associative-commutative contact `∣`. For a calculus whose contact K is **rigid**
(the interaction head sits in no equation and is not associative-commutative)
and whose environment-introduction is **degenerate**, the transform emits only
**Rule 1**: the compound-signature rules (2/3) and the split-process rules (4/5)
have no instance, because a rigid host has no associative-commutative operator
to conjoin signatures and no independent environment-introduction (output) sort
to sign separately. The untyped λ-calculus is the canonical such instance — its
contact is application / β-reduction.

This instance is mechanized standalone and axiom-free in
`theories/CAUntypedLambda.v` (host syntax `lterm`; fuel wrapper `lsys` reusing
`sig`/`token`; the single contact rule `lca_beta_r1`,
`{(App (Abs M) N)}_s ∣ s:T ⤳ {M[N/0]}_s ∣ T`, the exact analogue of `ca_rule1`):

| Calculus  | Contact K     | Rules emitted | Rocq witness |
|-----------|---------------|---------------|--------------|
| rho / π   | `∣` (AC)      | 1–5 (all)     | `CAReduction.ca_step` |
| untyped λ | `App` (rigid) | 1 only        | `CAUntypedLambda.lca_only_beta_r1` |

Key results (each `Closed under the global context`):

- **R1-only** — `lca_only_beta_r1`: every step is the β-R1 contact (or a
  parallel-context lift of one); `lca_contact_requires_token` /
  `lca_stack_inert`: a lone wrapper is stuck and a lone stack is inert;
  `lca_funded_nonredex_stuck`: even when funded, a non-redex does not fire (the
  rigid contact reduces only the β-redex shape).
- **Funded modulus** — `lca_step_decreases` (every step strictly drops the fuel
  measure) and `lca_funded_run_bounded` (a run is no longer than the initial
  token-stack height).
- **Funded strong normalization** — `lca_SN_funded`: every funded configuration
  is `Acc`-strongly-normalizing. Here SN is **unconditional** (a λ wrapper
  carries no fuel-bearing subterm, so the measure can never rise), in contrast
  to rho where SN holds on the linearly-funded fragment. The seam is exhibited
  by Ω = (λx.x x)(λx.x x): `omega_pure_diverges` shows pure-λ Ω β-reduces to
  itself, yet `lca_omega_funded_one_step` shows that, funded with one gate, the
  configuration takes exactly one metered step and then halts.
- **Erasure** — `lca_beta_r1_erasure`: the metered β-R1 step projects to a pure
  untyped-λ β-contraction (the gate is administrative; Cost decorates pure λ
  faithfully).
- **Abstract layer** — `CAUntypedLambdaCI.Lambda_ciGSLT_nonvacuous`: the metered
  λ calculus is a second object `Lambda_ciGSLT` of the ciGSLT category `CICat`
  under the cost endofunctor `CostCI`, beside `Rho_ciGSLT`. Cost's genericity is
  thus witnessed by two concrete contacts — AC (rho ⇒ five rules) and rigid
  (λ ⇒ R1 only). See DR-25.

### 4.3 Token Conservation

> **Theorem** (`token_monotone_step`,
> `theories/TokenConservation.v:56`).
>
>     S ⤳ S'  ⟹  ‖S'‖ ≤ ‖S‖

> **Theorem** (`token_monotone_reachable`,
> `theories/TokenConservation.v:98`).
>
>     S ⤳* S'  ⟹  ‖S'‖ ≤ ‖S‖

**Proof method.** By induction on the `ca_step` derivation. Each COMM
rule unfolds `‖·‖` on both sides into a closed arithmetic identity that
the `lia` tactic (linear integer arithmetic) discharges immediately. The
PAR cases are additive: the inductive hypothesis provides the per-side
inequality, and `‖S₁ ∥ S₂‖ = ‖S₁‖ + ‖S₂‖` turns it into a
sum-respecting bound. The multi-step theorem follows by induction on the
reflexive-transitive closure.

Per-rule exact decreases:

| Rule | `‖LHS‖ − ‖RHS‖` |
|------|-----------------|
| 1    | 1               |
| 2    | 2               |
| 3    | 1               |
| 4    | 1               |
| 5    | 2               |

---

## 5. The Compositional Translation

The central insight of [4] is that cost accounting is a
**fuel-acquisition protocol**: before a signed process can communicate, it
must consume a token (fuel) from the channel associated with its
signature. This protocol is expressible entirely within the pure rho
calculus.

```
       ┌─────────────────────┐
       │  Cost-Accounted     │
       │  Calculus           │
       │ (sigs, tokens, sys) │
       └─────────┬───────────┘
                 │
    ┌────────────┼────────────────┐
    │            │                │
 Signatures   Tokens          Systems
  s → name   T → proc        S → proc
    │            │                │
    ▼            ▼                ▼
  N⟦·⟧         T⟦·⟧             S⟦·⟧
    │            │                │
    └────────────┼────────────────┘
                 │
                 ▼
       ┌─────────────────────┐
       │    Pure Rho         │
       │    Calculus         │
       │  (proc, name only)  │
       └─────────────────────┘
```

### 5.1 Signature Translation `N⟦·⟧`

(`theories/Translation.v:122`)

Signatures become **channel names** (quoted processes):

        N⟦()⟧           =  @0
        N⟦hash(σ)⟧      =  @H_σ
        N⟦s₁ & s₂⟧      =  @( *N⟦s₁⟧ ∣ *N⟦s₂⟧ )

where H_σ is a **canonical process** encoding byte string σ (the
`hash_process` parameter — see [Section 12](#12-assumptions-and-trust-base)).

**The compound case** exploits reflection. The name for `s₁ & s₂` is the
quotation of the parallel composition of the dequotations of the
component channels. This ensures injectivity: distinct compound
signatures produce structurally distinct channel names because their
dequoted components differ.

**Example.** For signatures `s₁ = ()` and `s₂ = hash(σ)`:

        N⟦() & hash(σ)⟧  =  @( *(@0) ∣ *(@H_σ) )

This is the quoted code of a process that dereferences both the unit
channel and the hash channel in parallel.

### 5.2 Token Translation `T⟦·⟧`

(`theories/Translation.v:143`)

Tokens become **messages** (output processes) on signature channels:

        T⟦()⟧      =  0
        T⟦s:T⟧     =  N⟦s⟧!(T⟦T⟧)

A token `s:T` becomes an output on channel N⟦s⟧ carrying the translation
of the remaining balance T. The empty token translates to the stopped
process.

**Worked example.** The token `s₁:(s₂:())` — two units of fuel — becomes:

        T⟦s₁:(s₂:())⟧  =  N⟦s₁⟧!( N⟦s₂⟧!(0) )

Two nested outputs: the outer on `N⟦s₁⟧` carrying the inner, the inner on
`N⟦s₂⟧` carrying nil. Each output will be consumed by one fuel-gate
firing.

### 5.3 Signed Process Translation `P⟦·⟧`

(`theories/Translation.v:191`)

The key idea: a signed process must **consume fuel** before it can act.
This is achieved by wrapping the process in an input prefix — a
**fuel gate** — that blocks until a matching token arrives.

**Atomic signatures** (s = `()` or `hash(σ)`):

        P⟦P^s⟧  =  for(t ← N⟦s⟧)( P↑¹ ∣ *t )

The process *P* is lifted by 1 de Bruijn level (`P↑¹`) to account for
the binder introduced by the fuel gate's `for`. The variable *t*
(de Bruijn index 0) receives the remaining-fuel payload; `*t` dequotes it,
releasing the continuation into parallel.

**Compound signatures** (s = s₁ & s₂):

        P⟦P^{s₁ & s₂}⟧  =  for(t₁ ← N⟦s₁⟧) for(t₂ ← N⟦s₂⟧)
                                ( P↑² ∣ *t₁ ∣ *t₂ )

The process acquires fuel from **both** component channels via nested
input prefixes. `P↑²` lifts by 2 to cross both binders. Variables *t₁*
(index 1) and *t₂* (index 0) receive the two payloads.

**Intuition.** The fuel gate is the *capability-security* mechanism: a
process literally cannot reduce until it holds a message on its signature
channel. No token, no communication — and the token is consumed in the
process.

### 5.4 System Translation `S⟦·⟧`

(`theories/Translation.v:220`)

The system translation is defined compositionally:

        S⟦P^s⟧        =  P⟦P^s⟧
        S⟦T⟧          =  T⟦T⟧
        S⟦S₁ ∥ S₂⟧    =  S⟦S₁⟧ ∣ S⟦S₂⟧

> **Theorem** (`S_tr_compositional`).
>
>     S⟦S₁ ∥ S₂⟧ = S⟦S₁⟧ ∣ S⟦S₂⟧

This holds by definition. It is the headline structural property of the
translation: system-level parallel composition maps directly to
process-level parallel composition.

### 5.5 Infrastructure Processes

(`theories/Translation.v:263`)

When the granularity of the token (combined vs. split) does not match the
granularity expected by the signed process, **mediator processes** bridge
the gap.

**Split** ([4, Appendix A], Split/Join infrastructure) — converts a
combined token into separate tokens:

        Split(s₁, s₂)  =  for(t ← N⟦s₁ & s₂⟧)( N⟦s₁⟧!(0) ∣ N⟦s₂⟧!(*t) )

Upon receiving a compound token, Split emits:
1. An empty signal (`0`) on channel `N⟦s₁⟧`.
2. The received payload (`*t`) on channel `N⟦s₂⟧`.

**Join** ([4, Appendix A], Split/Join infrastructure) — the inverse:

        Join(s₁, s₂)  =  for(t₁ ← N⟦s₁⟧) for(t₂ ← N⟦s₂⟧)
                             ( N⟦s₁ & s₂⟧!( *t₁ ∣ *t₂ ) )

Join collects one token from each atomic channel and emits a combined
token on the compound channel.

**Walkthrough: Split firing.** Consider a system with a combined token
`(s₁ & s₂):T` and a Split mediator:

    ── Initial state:
    Split(s₁, s₂)  ∣  N⟦s₁ & s₂⟧!(T⟦T⟧)

    ── COMM fires on channel N⟦s₁ & s₂⟧. The Split receives
    ── the payload T⟦T⟧. Variable t binds to @(T⟦T⟧).

    ── After substitution:
    N⟦s₁⟧!(0)  ∣  N⟦s₂⟧!( *(@(T⟦T⟧)) )

    ── The dequotation *(@(T⟦T⟧)) reduces (semantically) to T⟦T⟧.
    ── Result: two atomic tokens, one per component channel.
    N⟦s₁⟧!(0)  ∣  N⟦s₂⟧!(T⟦T⟧)

This is formalized as `Split_fires_closed` in `theories/Translation.v`.

The translation's compositionality (`S⟦S₁ ∥ S₂⟧ = S⟦S₁⟧ ∣ S⟦S₂⟧`) and
the operational behaviour of the mediator processes (`Split_fires_closed`
and its compound counterpart) are the two structural ingredients used
throughout Section 6's headline theorems. Section 6 states those
theorems informally, Section 7 records the three-layer proof
architecture, Section 8 dives into the key techniques, and Section 9
gives the end-to-end mathematical proofs.

---

## 6. Headline Theorems

### 6.1 Contextual Forward Reachability

> **Theorem** (`translation_faithful`,
> `theories/TranslationFaithfulness.v:2308`).
>
>     ∀S, S'. S ⤳ S'  ⟹  ∃Ctx, W.
>         closed_proc(Ctx) ∧ S⟦S⟧ ∣ Ctx  ⇝*  W

*For every cost-accounted step, the translation of the source — possibly
extended with a closed context of Split mediators — reaches some pure-rho
witness state.*

**Precision boundary.** The generic theorem intentionally leaves the
witness existential. It does not by itself prove `W = S⟦S'⟧` or `W ≡
S⟦S'⟧`, and it does not prove that every pure-rho reduction from a
translated image reflects back to a `ca_step`. Per-rule simulation
lemmas expose stronger witness shapes where required; full translated-
image reflection is a separate proof obligation, not claimed here.

**Proof strategy.** By induction on the `ca_step` derivation, dispatching
each of the five COMM rules to a per-rule simulation lemma:

| Rule | Simulation lemma           | Ctx                                     |
|------|----------------------------|-----------------------------------------|
| 1    | `rule1_simulation_generic` | `0` or `Split` (depending on sig shape) |
| 2    | `rule2_simulation`         | `0` (tokens already split)              |
| 3    | `rule3_simulation`         | `Split(s₁, s₂)`                         |
| 4    | `rule4_simulation_generic` | `0` or `Split` (depending on sig shape) |
| 5    | `rule5_simulation_generic` | `0` or `Split` (depending on sig shape) |

The Rule column above follows the Rocq constructor numbering
(`rule4_simulation_generic` proves the combined-token case;
`rule5_simulation_generic` proves the split-tokens case). The May-2026
spec §3.6 labels these in the opposite order (its Rule 5 is the
combined-token case, its Rule 4 the split-tokens case); the Rocq lemma
names are retained unchanged, and the rule set is identical.

The PAR contextual closure cases lift the per-rule reachability via
`rho_reachable_par_l` and `rho_reachable_par_r`.

**Per-rule pattern** (literate pseudocode):

    FORWARD_SIM(rule, sig_shape):
        ── Step 1: Unfold S⟦LHS⟧ using definitional equations
        ── of S_tr, P_tr, T_tr, N_tr.

        ── Step 2: If compound signature, rearrange via ≡ so that
        ── the Split mediator and combined token are adjacent.

        ── Step 3: Fire the fuel gate(s) via COMM.
        ──   Atomic: one COMM on N⟦s⟧.
        ──   Compound: Split fires first (1 COMM),
        ──     then outer gate (1 COMM), then inner gate (1 COMM).

        ── Step 4: Fire the inner COMM (the original communication
        ── P{@Q/y}).

        ── Step 5: Reassemble the result into the witness W.
        ── Per-rule lemmas record when W has a target-specific shape.

### 6.2 Strong Bisimulation

> **Theorem** (`translation_strong_bisimilar_generic`,
> `theories/Bisimulation.v:1246`).
>
>     ∀s, P. ∃Ctx, W.
>         closed_proc(Ctx)
>       ∧ S⟦P^s ∥ s:()⟧ ∣ Ctx  ⇝*  W
>       ∧ W ~~ P

*The translated system (a signed process with one unit of fuel) reaches a
state that is **strongly bisimilar** to the original process P.*

**Intuition.** The fuel gate is operationally transparent: after it fires
(consuming one fuel unit), the resulting process `P ∣ *(@0)` has the same
observable behavior as `P` alone. The stuck residue `*(@0)` has no barbs
and cannot participate in any COMM — it is inert ballast.

| Signature | Ctx             | Final state W                 | Residues         |
|-----------|-----------------|-------------------------------|------------------|
| `()`      | `0`             | `P ∣ *(@0)`                   | 1 stuck residue  |
| `hash(σ)` | `0`             | `P ∣ *(@0)`                   | 1 stuck residue  |
| `s₁ & s₂` | `Split(s₁, s₂)` | `P ∣ ( *(@0) ∣ *(@(*(@0))) )` | 2 stuck residues |

### 6.3 Per-Step Reverse Simulation

> **Theorem** (`gate_per_step_reverse_generic`,
> `theories/TranslationFaithfulness.v:3888`).
>
>     ∀s, P, Q.
>       gated_system(P, s) ⇝ Q  ⟹
>       ∃W. Q ⇝* W  ∧  W ≡ gate_final(P, s)

*Any single rho-step from the gated system reaches the canonical final
state (up to structural equivalence).*

Definitions:

        gated_system(P, s) =
          ∣ S⟦P^s ∥ s:()⟧                          if s is atomic
          ∣ S⟦P^s ∥ s:()⟧ ∣ Split(s₁, s₂)          if s = s₁ & s₂

        gate_final(P, s) =
          ∣ P ∣ *(@0)                              if s is atomic
          ∣ P ∣ ( *(@0) ∣ *(@(*(@0))) )            if s = s₁ & s₂

For atomic cases, `W = Q` and `Q ≡ gate_final` directly (zero additional
steps — the gate fires in exactly one COMM). For compound cases, two
additional rho-steps are needed (the outer and inner nested gates fire
after the Split has decomposed the token).

### 6.3.1 Phase-Based Gate Reflection

> **Theorem** (`backward_reflection_phased_gate`,
> `theories/TranslationFaithfulness.v:4022`).
>
>     translated_gate_phase(P, s, GateReady, R) ∧ R ⇝ Q
>       ⟹ ∃W.
>            Q ⇝* W
>          ∧ translated_gate_phase(P, s, GateSpent, W)
>          ∧ consumed(GateSpent) = S(consumed(GateReady))

This is the mechanically checked backward-reflection core for translated
fuel gates. A direct one-step theorem back to `ca_step` would be false:
compound signatures can first perform an administrative Split step, and
all signature shapes can produce inert post-gate residue. The phase
relation records the correct invariant instead: any target step out of a
well-formed translated gate reaches the unique spent phase and accounts
for exactly one billable source-token event. The theorem is generic over
`SUnit`, `SHash`, and arbitrarily nested `SAnd` signatures because it
dispatches through `gate_per_step_reverse_generic`.

The source-level billing companion is `billed_step` plus
`ca_step_billed`: every `ca_step S S'` has a positive token delta `k`
such that `system_token_count S = k + system_token_count S'`. Together,
these facts tie target-side gate reflection to source-token accounting
without counting raw Split/Join routing COMMs as billable cost.

### 6.3.2 Recursive Whole-System Backward Reflection

> **Theorem** (`well_reflected_backward_reflection`,
> `theories/TranslationFaithfulness.v:4147`).
>
>     well_reflected(S, R) ∧ R ⇝ R'
>       ⟹ ∃S' W.
>            S ⤳ S'
>          ∧ R' ⇝* W
>          ∧ well_reflected(S', W)

This is the full backward-reflection theorem for the implementation
target selected by the migration plan. The relation `well_reflected` is
an alias for `recursively_metered_image`: terminal source systems map to
`PNil`; every enabled source step `S ⤳ S'` is represented by a
continuation-keyed `recursive_metered_gate(K)`; and the continuation `K`
is itself a recursively metered image of `S'`.

The supporting lemmas are:

| Lemma | Meaning |
|-------|---------|
| `recursive_metered_gate_fires` | The continuation-keyed gate has a rho step to `K ∣ PNil`. |
| `recursive_metered_gate_per_step_reverse` | Every rho step out of that gate lands in a state structurally equivalent to `K`. |
| `recursively_metered_parallel_left_enabled`, `recursively_metered_parallel_right_enabled` | Any enabled source step in either side of `SPar` can be selected independently, preserving source parallelism through `ca_par_l` and `ca_par_r`. |

The proof is intentionally relation-based rather than a giant executable
translation function. That keeps verification memory bounded: Rocq only
inverts the local continuation-keyed gate and uses structural closure to
carry the recursive invariant forward. This theorem closes the
previously missing arbitrary-rho-step reflection case for the implementation
target. The legacy compositional image `S_tr` remains useful for local
translation facts, gate-shape lemmas, and paper traceability, but it is
not the object used to state the business-critical whole-system
reflection property. `Print Assumptions well_reflected_backward_reflection`
reports `Closed under the global context`.

### 6.4 Token Conservation

> **Theorem** (`token_monotone_reachable`,
> `theories/TokenConservation.v:98`).
>
>     S ⤳* S'  ⟹  ‖S'‖ ≤ ‖S‖

See [Section 4.3](#43-token-conservation) for the full development.

### 6.5 Forward Weak-Barb Propagation (Replication Encoding)

Meredith–Radestock [1, §3] encode the π-calculus replication operator
`!P` in the pure rho calculus *without* a dedicated `PReplicate`
constructor by exploiting reflection:

        D(x)         ≜  for(y ← x){ x⟨∣*y∣⟩ ∣ *y }
        bang(x, P)   ≜  x⟨∣D(x) ∣ P∣⟩ ∣ D(x)

The self-receiver `D(x)` listens on channel *x*; when a sender drops
its payload onto *x* as a quoted name, `D(x)` re-posts the payload
and dereferences it in parallel. The term `bang(x, P)` bootstraps
this machinery by sending `D(x) ∣ P` as the initial payload.

A single COMM step unfolds this into a new copy of the body plus a
regenerated encoding:

> **Theorem** (`bang_encoding_unfolds`,
> `theories/Replication.v:222`).
>
>     closed_name(x) ∧ closed_proc(P)
>        ⟹  bang_encoding(x, P)  ⇝  bang_encoding(x, P) ∣ P

**Process diagram** (one step from `bang_encoding x P`):

```
   bang_encoding x P                 =  x⟨∣D(x) ∣ P∣⟩ ∣ D(x)
                                     =  (send on x) ∣ (receive on x)
                                                  │
                                                  │  rs_comm on x
                                                  ▼
   bang_encoding x P ∣ P             =  x⟨∣D(x) ∣ P∣⟩ ∣ D(x) ∣ P
                                         └── regenerated ──┘  └new P┘
```

The "regenerated encoding" re-emerges because the payload
`D(x) ∣ P` sent on *x* is received by `D(x)`, which then re-posts it
and dereferences it — the dereference of a quoted process
(`*(@Q) ≡ Q` via semantic substitution) unfolds `Q` into the
parallel context. This matches the one-step behavior of
`PReplicate` exactly: both produce "one more copy of *P* in
parallel, regenerating their former selves."

Since `bang_encoding x P` produces a fresh copy of *P* on every COMM
and `PReplicate P` does so on every `rs_replicate` step, whatever
*P* can eventually exhibit as an observable should be exhibitable by
either wrapper as well. The forward direction of this equivalence
is:

> **Theorem** (`preplicate_bang_encoding_body_barbs_sound`,
> `theories/Replication.v:1448`).
>
>     closed_name(x) ∧ closed_proc(P)
>        ⟹ ( P ⇓ᵢ y  ⟹  PReplicate P ⇓ᵢ y  ∧  bang_encoding(x, P) ⇓ᵢ y )
>        ∧ ( P ⇓ₒ y  ⟹  PReplicate P ⇓ₒ y  ∧  bang_encoding(x, P) ⇓ₒ y )

In prose: **every weak input/output barb of the body *P* is reflected
as a weak barb of both wrappers, on any channel *y*.**

**Proof sketch** (mechanized without axioms). Given `P ⇓ᵢ y`, unpack
to some `P ⇝* P'` with `input_barb P' y`. Then:

- For the primitive side: by `rs_replicate`, `PReplicate P ⇝
  P ∣ PReplicate P`. Continuing the reachability chain on the
  left arm gives `PReplicate P ⇝* P' ∣ PReplicate P`; the barb
  lifts by `input_barb_par_l`.
- For the encoded side: by `bang_encoding_unfolds`,
  `bang_encoding x P ⇝ bang_encoding x P ∣ P`. Continuing the
  reachability chain on the right arm gives
  `bang_encoding x P ⇝* bang_encoding x P ∣ P'`; the barb lifts
  by `input_barb_par_r`.

Output-barb case is dual. **No axiom is used.** See Section 9.8.2
for the full proof.

### 6.6 Replication Encoding Verification Boundary

The mechanized replication result is deliberately one-way:

> **Theorem** (`replication_encoding_forward_barb_sound`,
> `theories/Replication.v:2063`).
>
>     closed_name(x) ∧ closed_proc(P)
>     ⟹
>       (P ⇓ᵢ y ⟹ PReplicate P ⇓ᵢ y ∧ bang_encoding(x, P) ⇓ᵢ y)
>     ∧ (P ⇓ₒ y ⟹ PReplicate P ⇓ₒ y ∧ bang_encoding(x, P) ⇓ₒ y)

This is a direct summary of
`preplicate_bang_encoding_body_barbs_sound` (Section 6.5). It proves
that both replication views expose every weak input/output observable
already available from the body.

The development does **not** state a theorem projecting every weak barb
of `PReplicate P` or `bang_encoding x P` back to one copy of `P`. That
projection is stronger than the standard replication law
`!P ~ P | !P`; multiple unfolded copies of a nondeterministic body can
expose combined weak behavior that no single body copy exposes alone.
Removing that overclaim keeps `Replication.v` axiom-free and preserves
the exact proof boundary needed by the cost-accounting design.

The hidden-channel relation `weak_barbed_equiv_except x` remains defined
in `WeakBarbedEquiv.v` as specification infrastructure for observations
modulo a coordination channel. It is not used as an unproved assumption
in any headline theorem.

---

## 7. Proof Architecture

### 7.1 The Proof Layers

The development is organized as a monotone stack of seven layers. Each
layer depends only on earlier layers; no upward references exist.
Layers 1–3 are the original consensus-critical stack; Layers 4 and 5
add weak-observation infrastructure and replication-encoding support;
Layers 6 and 7 add runtime-budget refinement and use-case adequacy.
No layer introduces theorem-level axioms.

```
Layer 1 ── Syntactic Foundation
  ├── RhoSyntax (855 lines, 31 thms)
  │     Types, substitution, lifting, structural equivalence.
  │     Key: subst_lift_zero, head_count_se.
  ├── StructEquivInversion (253 lines, 7 thms)
  │     head_count, count_inputs, count_outputs, count_derefs, count_replicates.
  ├── StructEquivHeads (1,470 lines, 45 thms)
  │     heads, list_equiv, perm_equiv, struct_equiv_heads_perm,
  │     se_PInput_inj, se_POutput_inj, se_PReplicate_inj,
  │     only_replicate + onlyreplicate_se_both (Section 8.7).
  └── RhoReduction (442 lines, 17 thms)
        rho_step, rho_reachable, conflated barb, split input_barb /
        output_barb (§3.6), stuck lemmas.

Layer 2 ── Cost-Accounting and Translation
  ├── CostAccountedSyntax (231 lines, 4 thms)
  │     sig, token, system, sig_size, token_size.
  ├── CostAccountedReduction (283 lines, 5 thms)
  │     ca_step (5 rules), ca_reachable.
  ├── Translation (580 lines, 12 thms)
  │     N_tr, T_tr, P_tr, S_tr, Split, Join, closure properties.
  ├── TokenConservation (234 lines, 9 thms)
  │     token_monotone_step, token_monotone_reachable,
  │     per-rule exact decreases.
  ├── Settlement (140 lines, 8 thms)
  │     post-evaluation escrow/refund arithmetic and no mid-evaluation
  │     refund fuel.
  └── SlashingComposition (389 lines, 20 thms)
        adopts the slashing-side boundary from f1r3node-rust
        analysis/slashing and proves slash system effects preserve
        user fuel, settlement inputs, and settlement arithmetic.
  └── MergeableChannelAccounting (274 lines, 14 thms)
        models `IntegerAdd` and `BitmaskOr` mergeable-channel accounting,
        proves bitmask diff/merge round trips, order-independent OR
        folding, non-numeric fallback classification, merge-type
        preservation, and cost-boundary isolation.

Layer 3 ── Faithfulness and Strong Bisimulation
  ├── TranslationFaithfulness (4,183 lines, 84 thms)
  │     Per-rule simulation (all 5 × all sig shapes),
  │     per-step reverse (unit, hash, compound, generic),
  │     phased reflection and recursive whole-system reflection,
  │     channel distinctness (N_tr_size_eq, N_tr_signature_strict),
  │     stuck-process infrastructure.
  ├── FuelGateSafety (357 lines, 6 thms)
  │     no_send_on predicate, fuel-gate capability security.
  └── Bisimulation (1,248 lines, 36 thms)
        bisim (coinductive), post_gate_bisim (CoFixpoint),
        multi_stuck_residue_bisim,
        translation_strong_bisimilar_generic.

Layer 4 ── Weak Barbed Observables
  └── WeakBarbedEquiv (259 lines, 17 thms)
        weak_barb_input, weak_barb_output (reachability- +
        ≡ₙ-closed observables; see Section 3.6 and Glossary §2.7),
        weak_barbed_equiv_except x  (four-way iff on channels
        distinct from hidden x),
        parallel-congruence and replication-ingress lemmas.

Layer 5 ── Replication Encoding Support
  └── Replication (2,071 lines, 56 thms)
        Reflective encoding (D_encoding, bang_encoding), operational
        unfold (bang_encoding_unfolds), step-inversion machinery
        (step_PReplicate_inv_se, step_PPar_PReplicate_inv_se),
        forward barb propagation
        (preplicate_bang_encoding_body_barbs_sound),
        closed verification-boundary theorem
        (replication_encoding_forward_barb_sound, Section 6.6).

Layer 6 ── Runtime Budget Refinement
  └── RuntimeBudgetRefinement (2,024 lines, 83 thms)
        bounded-memory budget conservation, successful weighted
        reservation refinement, out-of-phlo boundary commitment,
        reset-from-token trace clearing, finalization-read cost traces,
        zero-event commitments, block/cache authentication,
        and replay-payload trace sensitivity.

Layer 7 ── Use-Case Adequacy
  └── UseCaseAdequacy (1,895 lines, 84 thms)
        named UC-CA semantic anchors over token conservation,
        unit-token expansion, confluence, settlement, slashing
        composition, typed mergeable channels, recursive reflection,
        runtime-budget refinement, finalization-read trace digests,
        block/cache authentication, zero-event commitments, and replay
        payload equivalence.
```

**Dependency property.** Layers 4 and 5 depend on Layers 1–3 but are
*not* depended on by anything in Layers 1–3. In particular, the
consensus-critical theorems (`token_monotone_*`,
`ca_cost_deterministic`, `ca_step_deterministic`,
`fuel_events_consumed_perm`) are proven within Layers 1–3 and their
`Print Assumptions` output contains none of the Layer-5 hash assumptions.
The hash assumptions gate *only* the single headline theorem of
Section 6.6.

### 7.2 Per-Rule Reachability Strategy

Each of the five cost-accounted rules is simulated by a pure-rho
reduction sequence. The compound sub-cases (Rules 2–5 with SAnd

helper, which packages the two-step (outer gate + inner gate) reduction
into a single reachability lemma:

> **Lemma** (`compound_half_fires_two_step`,
> `theories/TranslationFaithfulness.v:1159`).
>
>     ∀R, u, v, M_u, M_v.
>       closed_proc(M_u) → closed_proc(M_v) →
>       ( (P⟦R^{u & v}⟧ ∣ N⟦u⟧!(M_u)) ∣ N⟦v⟧!(M_v) )
>         ⇝*
>       R ∣ ( *(@M_u) ∣ *(@M_v) )

The proof constructs two explicit `rr_step` applications:
1. The outer gate (listening on `N⟦u⟧`) fires via COMM with the
   s₁-output, leaving the inner gate exposed.
2. The inner gate (listening on `N⟦v⟧`) fires via COMM with the
   s₂-output, releasing the body `R`.

### 7.3 Bisimulation Strategy

The bisimulation proof has three components:

**Forward direction** (P-step implies post-gate-step). If `P ⇝ P'`, then
`(P ∣ *(@0)) ⇝ (P' ∣ *(@0))` via `rs_par_l`. The stuck residue is
untouched.

**Backward direction** (post-gate-step implies P-step). If
`(P ∣ *(@0)) ⇝ W`, then `W ≡ (P' ∣ *(@0))` for some `P'` with `P ⇝ P'`.
This is the `backward_sim_par_stuck` lemma. The key insight: `*(@0)` has
no input or output heads, so it cannot participate in any COMM. Any
reduction of `(P ∣ *(@0))` must happen entirely within P.

**Coinduction.** The two directions are combined into a `CoFixpoint` proof
of `bisim` (see [Section 8.1](#81-coinductive-bisimulation-via-cofixpoint)).

---

## 8. Deep Dive: Key Proof Techniques

### 8.1 Coinductive Bisimulation via CoFixpoint

The `bisim` relation (`theories/Bisimulation.v:433`) is a **coinductive
proposition**:

    P ~~ Q  iff
      (∀P'. P ⇝ P' ⟹ ∃Q'. Q ⇝ Q' ∧ P' ~~ Q')
    ∧ (∀Q'. Q ⇝ Q' ⟹ ∃P'. P ⇝ P' ∧ P' ~~ Q')

In Rocq, coinductive proofs must satisfy the **guardedness condition**:
every recursive occurrence of the coinductive hypothesis must appear
immediately under a constructor of the coinductive type. This prevents
"unproductive" infinite loops.

The proof of `post_gate_bisim` (`theories/Bisimulation.v:753`) is a
`CoFixpoint` — a term-level coinductive construction:

    COFIXPOINT post_gate_bisim_strong(P, W, H : W ≡ P ∣ *(@0)):
      RETURN bisim_intro(W, P,
        ── Forward (W ⇝ W' ⟹ ∃P'. P ⇝ P' ∧ P' ~~ Q'):
        λ(W', H_step) ↦
          LET (P', H_P_step, H_eq') :=
            backward_sim_par_stuck(W, W', H_step, P, H)
          IN (P', H_P_step,
              post_gate_bisim_strong(P', W', H_eq'))   ◁── guarded

        ── Backward (P ⇝ P' ⟹ ∃W'. W ⇝ W' ∧ P' ~~ W'):
        λ(P', H_step) ↦
          LET W' := P' ∣ *(@0)
          IN (W',
              rs_struct(W, P ∣ *(@0), W', H, rs_par_l(H_step)),
              post_gate_bisim_strong(P', W', refl))    ◁── guarded
      )

Both recursive calls to `post_gate_bisim_strong` appear directly under
`bisim_intro`, satisfying the guardedness condition. The structural
equivalence parameter `H : W ≡ P ∣ *(@0)` is threaded through to handle
the `STRUCT` rule's output, which may differ from the canonical form.

```
    ┌──────────────────────────────────────────┐
    │              bisim_intro                 │
    │                                          │
    │  Forward:                                │
    │    W ⇝ W'                                │
    │      │                                   │
    │      ▼ backward_sim_par_stuck            │
    │    P ⇝ P', W' ≡ P' ∣ *(@0)               │
    │      │                                   │
    │      ▼ RECURSE (guarded)                 │
    │    P' ~~ W'                              │
    │                                          │
    │  Backward:                               │
    │    P ⇝ P'                                │
    │      │                                   │
    │      ▼ rs_par_l + rs_struct              │
    │    W ⇝ P' ∣ *(@0) = W'                   │
    │      │                                   │
    │      ▼ RECURSE (guarded)                 │
    │    P' ~~ W'                              │
    └──────────────────────────────────────────┘
```

### 8.2 Heads-List Permutation Characterization

Structural equivalence rearranges the top-level parallel components of a
process but cannot change their identity (up to ≡). The
`struct_equiv_heads_perm` theorem
(`theories/StructEquivHeads.v:218`) formalizes this:

> **Theorem.** *If P ≡ Q, then:*
>
>     ∃zs. list_equiv(heads(P), zs) ∧ Permutation(zs, heads(Q))

where `heads(P)` flattens P into its list of top-level components:

    HEADS(P):
        MATCH P WITH
        ∣ PNil          → []
        ∣ PInput(_, _)  → [P]
        ∣ POutput(_, _) → [P]
        ∣ PDeref(_)     → [P]
        ∣ PPar(P, Q)    → HEADS(P) ++ HEADS(Q)

The **perm_equiv** relation factors the comparison into two steps:
(1) pointwise structural equivalence (`list_equiv`) and (2) reordering
(`Permutation` from the Rocq Stdlib). This factoring enables the "zigzag
lemmas" (`list_equiv_Permutation_commute` and its dual) that commute
the two steps.

**Usage in the per-step reverse proofs.** When a process with 3 known
heads (e.g., `Gate`, `TokOut`, `Split`) is decomposed by `rs_par_l` into
`PPar A B`, the heads of `A` and `B` must be a partition of the 3 canonical
heads. The `fh_perm_3` lemma enumerates all 6 permutations of 3
elements; combined with `fh_list_equiv_3_inv` (pointwise inversion), this
yields 3 effective cases for which head ends up in `B`.

### 8.3 Head-Count Case Splitting

When a process S with head_count(S) = 3 takes a step via `rs_par_l`
producing `PPar A B`, the head counts satisfy:

        head_count(A) + head_count(B) = 3
        head_count(A) ≥ 2                 ── from rho_step_head_count_ge_two

This forces `head_count(B) ∈ {0, 1}`:

**Case B has 0 heads** (`B ≡ 0`): `A` carries all 3 heads. The inductive
hypothesis applies directly to `A`.

**Case B has 1 head**: The `fh_compound_heads_split` lemma
(`theories/TranslationFaithfulness.v:3510`) enumerates which of the 3
canonical heads ended up in `B` via a 6-way permutation analysis:

| B's head                             | A's heads         | Can A step?                       | Outcome                    |
|--------------------------------------|-------------------|-----------------------------------|----------------------------|
| `Gate` (`PInput` on `N⟦s₁⟧`)         | `{TokOut, Split}` | Yes (matching channels)           | `Split` fires; reach final |
| `TokOut` (`POutput` on `N⟦s₁ & s₂⟧`) | `{Gate, Split}`   | No (both `PInput`s, zero outputs) | Contradiction              |
| `Split` (`PInput` on `N⟦s₁ & s₂⟧`)   | `{Gate, TokOut}`  | No (channels mismatch)            | Contradiction              |

The second row is discharged by `no_outputs_irreducible`; the third by
`fh_gate_tok_2head_stuck` (which invokes `N_tr_signature_strict`).

### 8.4 Channel Distinctness via Signature Size

The compound per-step reverse must rule out the {Gate, TokOut} pairing:
the fuel gate (`PInput` on `N⟦s₁⟧`) and the combined token (`POutput` on
`N⟦s₁ & s₂⟧`) cannot form a COMM because their channels differ.

> **Lemma** (`N_tr_size_eq`,
> `theories/TranslationFaithfulness.v:2980`).
>
>     N⟦s₁⟧ ≡_N N⟦s₂⟧  ⟹  |s₁| = |s₂|

where `|s|` is defined as:

        |()| = 1,    |hash(σ)| = 1,    |s₁ & s₂| = 1 + |s₁| + |s₂|

**Proof.** By induction on `s₁` with nested case analysis on `s₂`.

- *Base cases* (`SUnit` × `SHash`, `SHash` × `SAnd`, etc.): The underlying
  processes of `N⟦s₁⟧` and `N⟦s₂⟧` have different head counts (0, 1, or 2
  respectively). Since `N⟦s⟧ = @(underlying process)`, the name equivalence
  `N⟦s₁⟧ ≡_N N⟦s₂⟧` implies structural equivalence of the underlying
  processes, which preserves head count — yielding a contradiction.

  The `SHash`-vs-`SAnd` case (1 head vs. 2 heads) relies on the
  `hash_process_head_count_one` hypothesis.

- *Inductive case* (`SAnd` × `SAnd`): Both sides have 2 `PDeref` heads.
  Apply `struct_equiv_heads_perm` and `fh_perm_2` to extract two pairings
  (identity or swap). In each pairing, apply the inductive hypothesis
  on the sub-components to derive `|s₁| = |s₂|` for each pair.

> **Corollary** (`N_tr_signature_strict`,
> `theories/TranslationFaithfulness.v:3064`).
>
>     ∀s₁, s₂.  ¬( N⟦s₁⟧ ≡_N N⟦s₁ & s₂⟧ )

**Proof.** If `N⟦s₁⟧ ≡_N N⟦s₁ & s₂⟧`, then `|s₁| = |s₁ & s₂| = 1 + |s₁| + |s₂|` by `N_tr_size_eq`. Since `|s₂| ≥ 1`, this gives `0 ≥ 2` — a
contradiction.

### 8.5 Stuck-Process Arguments

Two families of "stuck" lemmas rule out impossible reductions:

**No-outputs irreducibility** (`no_outputs_irreducible`,
`theories/TranslationFaithfulness.v:3080`):

>     count_outputs(R) = 0  ⟹  ¬(R ⇝ T)

A process with no output heads cannot reduce because COMM requires at
least one `POutput`. Proved by induction on `rho_step`: the `rs_comm`
case has `count_outputs ≥ 1` (contradiction); `rs_par_l`/`rs_par_r`
recurse; `rs_struct` preserves `count_outputs` via `count_outputs_se`.

Used to rule out the **{Gate, Split} pairing**: both are `PInput` heads
with zero combined outputs.

**Channel-mismatch irreducibility** (`fh_gate_tok_2head_stuck`,
`theories/TranslationFaithfulness.v:3328`):

>     S ≡ P⟦P^{s₁ & s₂}⟧ ∣ N⟦s₁ & s₂⟧!(0)  ⟹  ¬(S ⇝ T)

The gate (`PInput` on `N⟦s₁⟧`) and combined token (`POutput` on
`N⟦s₁ & s₂⟧`) cannot COMM because their channels are not
`≡_N`-equivalent (by `N_tr_signature_strict`). Proved by induction on
`rho_step`: the `rs_comm` case extracts both channel equivalences via
`se_PInput_inj` and `se_POutput_inj`, derives the forbidden
`N⟦s₁⟧ ≡_N N⟦s₁ & s₂⟧`, and contradicts.

### 8.6 Multi-Stuck Residue Bisimulation

The compound post-gate state has two stuck residues:

        P ∣ ( *(@0) ∣ *(@(*(@0))) )

Neither `*(@0)` nor `*(@(*(@0)))` has any input or output heads.

> **Lemma** (`multi_stuck_residue_bisim`,
> `theories/Bisimulation.v:1096`).
>
>     count_inputs(R) + count_outputs(R) = 0  ⟹  (P ∣ R) ~~ P

**Proof.** By structural induction on `R`:

- *R = `0`*: `P ∣ 0 ≡ P`; use `bisim_struct_equiv_l` + `bisim_refl`.
- *R = `PInput(_, _)`* or *`POutput(_, _)`*: `count_inputs` or
  `count_outputs` ≥ 1; contradicts the hypothesis.
- *R = `PPar(R₁, R₂)`*: Extract the zero-count constraints on `R₁` and
  `R₂`. Compose via `bisim_trans`:

        P ∣ (R₁ ∣ R₂)  ≡  (P ∣ R₁) ∣ R₂  ~~  P ∣ R₁  ~~  P

  where the first step uses `se_par_assoc`, the second uses the IH on
  `R₂`, and the third uses the IH on `R₁`.

- *R = `PDeref(n)`*: `count_inputs = count_outputs = 0`; apply
  `bisim_par_pderef_any`.

### 8.7 Heads-List Decomposition for PReplicate Preservation

The replication-encoding equivalence of Section 6.6 needs a
**structural factoring lemma** of the form

> *If `PPar P Q ≡ PPar (PReplicate body) R`, then the PReplicate head
> lives in exactly one arm of the LHS, and the other arm's heads
> match R's heads modulo permutation and ≡.*

This is *not* immediate from the constructors of `≡` alone — it
requires decoding the heads-list permutation machinery of §8.2. The
technique is to reconstruct the `only_*` predicate family (§8.2 uses
`only_input`, `only_output`, `only_deref`) with a new member for
`PReplicate`.

**The `only_replicate` predicate** (`StructEquivHeads.v` Section 13).
The new predicate pins down processes whose sole nonzero-head
contribution is a single `PReplicate`:

```coq
Inductive only_replicate : proc → proc → Prop :=
  | OR_base  : ∀ B, only_replicate (PReplicate B) B
  | OR_par_l : ∀ P Q B,
      only_replicate P B → head_count Q = 0 → only_replicate (PPar P Q) B
  | OR_par_r : ∀ P Q B,
      head_count P = 0 → only_replicate Q B → only_replicate (PPar P Q) B.
```

The companion `onlyreplicate_se_both` lemma transports witnesses
through `≡` in both directions, mirroring `onlyoutput_se_both`
(§8.2) case-for-case across all twelve constructors of `≡`. Each
transport leg recurses through a single IH and closes via two
`lia` applications on `head_count` arithmetic.

**Injectivity of `PReplicate` modulo `≡`.** A direct corollary is

> **Lemma** (`se_PReplicate_inj`, `StructEquivHeads.v` Section 13).
>
>     PReplicate X ≡ PReplicate Y  ⟹  X ≡ Y

which follows from `onlyreplicate_se_both` specialized at
`OR_base X` on the LHS: the transported witness has the form
`only_replicate (PReplicate Y) X'` with `X ≡ X'`, and inverting
this yields `X' = Y` syntactically — hence `X ≡ Y`.

**Locating the PReplicate head in a PPar.** The workhorse decomposition
lemma is:

> **Lemma** (`se_par_preplicate_locate`, `Replication.v` Section 14.A).
>
>     PPar P Q ≡ PPar (PReplicate body) R
>        ⟹
>        ( ∃ body' P_rest. body ≡ body' ∧ P ≡ PPar (PReplicate body') P_rest )
>      ∨ ( ∃ body' Q_rest. body ≡ body' ∧ Q ≡ PPar (PReplicate body') Q_rest )

The proof combines four pieces: `struct_equiv_heads_perm` (§8.2),
`heads_to_proc_heads_se` (round-trip reconstruction), the new
`list_equiv_app_inv` / `list_equiv_in_transport` helpers (split a
`list_equiv` across `++` and transport membership), and
`heads_PReplicate_inv` (packages an `In (PReplicate body) (heads P)`
observation into a structural equivalence `P ≡ PPar (PReplicate body)
P_rest`). Together they pinpoint the arm of the LHS `PPar` that
carries the PReplicate head witness.

**Step-inversion via indexed induction.** The culminating lemma of
Section 14.C is the key technique needed for the reverse direction's
shape preservation:

> **Lemma** (`step_PPar_PReplicate_inv_se`,
> `Replication.v` Section 14.C).
>
>     rho_step S R
>        ∧ S ≡ PPar (PReplicate body) P_rest
>        ⟹  ∃P_rest'. R ≡ PPar (PReplicate body) P_rest'

The proof is **indexed induction on `rho_step`** with the
`S ≡ PPar (PReplicate body) P_rest` hypothesis placed *inside* the
quantifier structure — not as a fixed parameter. This placement lets
the induction hypothesis respect `≡` automatically in every sub-case:

- `rs_comm`: `count_replicates (PPar (PInput _ _) (POutput _ _)) = 0`
  but `count_replicates (PPar (PReplicate body) P_rest) ≥ 1`, so
  `count_replicates_se` yields a contradiction.
- `rs_par_l` and `rs_par_r`: apply `se_par_preplicate_locate` above;
  recurse on the arm that contains the PReplicate (case (a)) or
  rebuild directly when the step is on the disjoint arm (case (b)).
- `rs_struct`: chain the two outer `≡` witnesses via `se_trans` and
  recurse on the inner step with the composed hypothesis — this is
  where placing `≡` inside the induction rather than outside is
  essential.
- `rs_replicate`: `head_count (PReplicate P) = 1`, which forces
  `head_count P_rest = 0` via `head_count_se`, hence
  `P_rest ≡ PNil` via `head_count_zero_se_nil`; then
  `se_PReplicate_inj` closes the case.

The broader narrative. This pattern — **indexed induction on step
derivation with `≡` in the conclusion** — is new to this project and
not used in Layers 1–3. It is a general-purpose technique for
reasoning about step behavior under `≡`-bound source states, and is
recorded here as a contribution of the replication-encoding work.

---

## 9. Mathematical Proofs

This section presents end-to-end mathematical proofs of each claim made
in [4]. Every theorem statement corresponds to a machine-checked Rocq
proof; every proof step corresponds to a tactic or term in the
mechanization. Supporting lemmas are proven before they are cited.

Throughout, we use the definitions and notation established in
Sections 2–5. The `hash_process` parameter and explicit section hypotheses (H1–H4) from
[Section 12](#12-assumptions-and-trust-base) are invoked where noted.

---

### 9.1 Token Conservation

> **Theorem 9.1** *(Token Monotonicity — Single Step).*
> *For all systems `S`, `S'`:*
>
>     S ⤳ S'  ⟹  ‖S'‖ ≤ ‖S‖

*Proof.* By induction on the derivation of `S ⤳ S'`.

**Case `ca_rule1`:** The step has the form
`(for(y ← x) P ∣ x!(Q))^s ∣ s:T  ⤳  (P{@Q/y})^s ∣ T`. Unfolding the
token count:

        ‖LHS‖ = ‖(for(y ← x) P ∣ x!(Q))^s‖ + ‖s:T‖ = 0 + (1 + ‖T‖) = 1 + ‖T‖
        ‖RHS‖ = ‖(P{@Q/y})^s‖ + ‖T‖ = 0 + ‖T‖ = ‖T‖

Since `‖T‖ ≤ 1 + ‖T‖`, the inequality holds. The net decrease is 1.

**Case `ca_rule2`:** The step consumes two gates:
`‖LHS‖ = (1 + ‖T₁‖) + (1 + ‖T₂‖)` and `‖RHS‖ = ‖T₁‖ + ‖T₂‖`. Net
decrease: 2.

**Case `ca_rule3`:** Same arithmetic shape as Rule 1 (one compound gate
consumed). Net decrease: 1.

**Case `ca_rule4`:** `‖LHS‖ = (0 + 0) + (1 + ‖T‖)` and
`‖RHS‖ = 0 + ‖T‖`. Net decrease: 1.

**Case `ca_rule5`:** Same shape as Rule 2. Net decrease: 2.

**Case `ca_par_l`:** The step has the form `S₁ ∥ S₂ ⤳ S₁' ∥ S₂` where
`S₁ ⤳ S₁'`. By the induction hypothesis, `‖S₁'‖ ≤ ‖S₁‖`. Since
`‖S₁ ∥ S₂‖ = ‖S₁‖ + ‖S₂‖` and `‖S₁' ∥ S₂‖ = ‖S₁'‖ + ‖S₂‖`, the
inequality `‖S₁'‖ + ‖S₂‖ ≤ ‖S₁‖ + ‖S₂‖` holds.

**Case `ca_par_r`:** The step has the form `S₁ ∥ S₂ ⤳ S₁ ∥ S₂'` where
`S₂ ⤳ S₂'`. The proof is symmetric to `ca_par_l`: the induction
hypothesis gives `‖S₂'‖ ≤ ‖S₂‖`, and since
`‖S₁ ∥ S₂'‖ = ‖S₁‖ + ‖S₂'‖ ≤ ‖S₁‖ + ‖S₂‖ = ‖S₁ ∥ S₂‖`, the
inequality holds.

This exhausts all constructors of `⤳`.  ∎

---

> **Theorem 9.2** *(Token Monotonicity — Multi-Step).*
> *For all systems `S`, `S'`:*
>
>     S ⤳* S'  ⟹  ‖S'‖ ≤ ‖S‖

*Proof.* By induction on the derivation of `S ⤳* S'`.

**Case `car_refl`:** `S' = S`, so `‖S'‖ = ‖S‖ ≤ ‖S‖`.

**Case `car_step`:** There exists an intermediate system `S₂` with
`S ⤳ S₂` and `S₂ ⤳* S'`. By Theorem 9.1, `‖S₂‖ ≤ ‖S‖`. By the
induction hypothesis, `‖S'‖ ≤ ‖S₂‖`. By transitivity of `≤`,
`‖S'‖ ≤ ‖S‖`.  ∎

---

### 9.2 Infrastructure Processes

> **Lemma 9.3** *(Split Fires).*
> *For all signatures `s₁`, `s₂` and closed process `M`:*
>
>     Split(s₁, s₂) ∣ N⟦s₁ & s₂⟧!(M)  ⇝  N⟦s₁⟧!(0) ∣ N⟦s₂⟧!(*(@M))

*Proof.* Recall that `Split(s₁, s₂) = for(t ← N⟦s₁ & s₂⟧)( N⟦s₁⟧!(0) ∣ N⟦s₂⟧!(*t) )`.
The term `Split(s₁, s₂) ∣ N⟦s₁ & s₂⟧!(M)` is a COMM redex on channel
`N⟦s₁ & s₂⟧`: the Split's input and the token's output share this
channel. By the COMM rule:

        Split(s₁, s₂) ∣ N⟦s₁ & s₂⟧!(M)
        ⇝  (N⟦s₁⟧!(0) ∣ N⟦s₂⟧!(*t)){@M/t}

Substitution distributes through `PPar` and the two outputs. Since
`N⟦s₁⟧` and `N⟦s₂⟧` are closed (by hypothesis H3 and the definition of
`N⟦·⟧`), substitution leaves them unchanged. The only variable reference
is `*t` (i.e., `PDeref(NVar 0)`), which becomes `*(@M)`:

        = N⟦s₁⟧!(0) ∣ N⟦s₂⟧!(*(@M))  ∎

---

> **Lemma 9.4** *(Compound Half Fires).*
> *For all processes `R`, signatures `u`, `v`, and closed processes
> `M_u`, `M_v`:*
>
>     (P⟦R^{u & v}⟧ ∣ N⟦u⟧!(M_u)) ∣ N⟦v⟧!(M_v)
>       ⇝*  R ∣ (*(@M_u) ∣ *(@M_v))
>
> *via exactly two `⇝`-steps.*

*Proof.* Recall that `P⟦R^{u & v}⟧ = for(t₁ ← N⟦u⟧) for(t₂ ← N⟦v⟧)( R↑² ∣ *t₁ ∣ *t₂ )`.

**Step 1** *(outer gate fires)*: The outer input on `N⟦u⟧` and the output
`N⟦u⟧!(M_u)` form a COMM redex. By the COMM rule, the outer
for-comprehension consumes `M_u`:

        (P⟦R^{u & v}⟧ ∣ N⟦u⟧!(M_u)) ∣ N⟦v⟧!(M_v)
        ⇝  (for(t₂ ← N⟦v⟧)( R↑¹ ∣ *(@M_u) ∣ *t₂ )) ∣ N⟦v⟧!(M_v)

The substitution replaces `t₁` (index 1 inside the inner body, index 0
at the outer level) with `@M_u`. Since `M_u` is closed, the
`subst_lift_zero` lemma reduces `R↑²` by one level to `R↑¹`. The
dereference `*t₁` becomes `*(@M_u)`.

**Step 2** *(inner gate fires)*: The inner input on `N⟦v⟧` and the output
`N⟦v⟧!(M_v)` form a COMM redex:

        ⇝  R ∣ *(@M_u) ∣ *(@M_v)

The substitution replaces `t₂` (index 0) with `@M_v`. By
`subst_lift_zero`, `R↑¹` reduces to `R`. The dereference `*t₂` becomes
`*(@M_v)`.

The total chain has exactly two `⇝`-steps.  ∎

---

### 9.3 Contextual Forward Reachability

We state a simulation lemma for each of the five cost-accounted rewrite
rules, then combine them into the generic contextual reachability theorem.
Each lemma is presented for the atomic-signature sub-case (the fully
worked representative); compound sub-signatures generalize via additional
Split firings and Lemma 9.4 applications, as noted at the end of each
proof.

---

> **Lemma 9.5.1** *(Rule 1 Simulation — Atomic).*
> *For all names `x`, processes `P`, `Q`, atomic signature `s` (i.e.,
> `s = ()` or `s = hash(σ)`), and token `T`:*
>
>     S⟦(for(y ← x) P ∣ x!(Q))^s ∥ s:T⟧
>       ⇝*  P{@Q/y} ∣ *(@(T⟦T⟧))
>
> *`Ctx = 0`. The reduction takes exactly two `⇝`-steps.*

*Proof.* Unfolding the system and process translations:

        S⟦LHS⟧ = P⟦(for(y ← x) P ∣ x!(Q))^s⟧ ∣ T⟦s:T⟧

Since `s` is atomic, `P⟦·⟧` uses a single fuel gate:

        = for(t ← N⟦s⟧)((for(y ← x) P ∣ x!(Q))↑¹ ∣ *t) ∣ N⟦s⟧!(T⟦T⟧)

**Step 1** *(fuel gate fires on `N⟦s⟧`)*: The fuel gate
`for(t ← N⟦s⟧)(...)` and the token output `N⟦s⟧!(T⟦T⟧)` share channel
`N⟦s⟧`, forming a COMM redex. By the COMM rule, the input consumes the
output and substitutes `@(T⟦T⟧)` for the bound variable `t` (de Bruijn
index 0) throughout the body:

        ⇝  SUBST((for(y ← x) P ∣ x!(Q))↑¹ ∣ *t, 0, @(T⟦T⟧))

Substitution distributes through `PPar`. On the left component: by
`subst_lift_zero`, substituting at index 0 into a process lifted by 1
recovers the original, so
`SUBST((for(y ← x) P ∣ x!(Q))↑¹, 0, @(T⟦T⟧)) = for(y ← x) P ∣ x!(Q)`.
On the right component: `SUBST(*t, 0, @(T⟦T⟧)) = *(@(T⟦T⟧))`. The state
after Step 1 is:

        (for(y ← x) P ∣ x!(Q)) ∣ *(@(T⟦T⟧))

**Step 2** *(inner COMM fires on `x`)*: The sub-processes `for(y ← x) P`
and `x!(Q)` share channel `x`, forming a COMM redex. By the PAR rule
applied to the left component of the parallel, this COMM fires under the
residue `*(@(T⟦T⟧))`:

        ⇝  P{@Q/y} ∣ *(@(T⟦T⟧))

The result is the substituted body in parallel with the dequotation of
the quoted token translation. The residue `*(@(T⟦T⟧))` is a `PDeref` of
a `Quote` — stuck (head count 1, no output partner).

**Compound sub-case.** When `s = s₁ & s₂`, the LHS is syntactically
identical to Rule 3's LHS: the signed process
`(for(y ← x) P ∣ x!(Q))^{s₁ & s₂}` is a whole redex under a compound
signature, and the token `(s₁ & s₂):T` is combined. This is exactly the
setting of Lemma 9.5.3. Set `Ctx = Split(s₁, s₂)`. By Lemma 9.5.3, the
translated LHS with Split context reaches the witness
`P{@Q/y} ∣ (*(@0) ∣ *(@(*(@(T⟦T⟧)))))` in four `⇝`-steps (Split fires,
outer gate fires, inner gate fires, inner COMM fires). The closedness of
`Ctx` follows from `Split_closed` using hypothesis H3. This dispatch is
`rule1_simulation_generic` (SAnd case) in the mechanization.  ∎

---

> **Lemma 9.5.2** *(Rule 2 Simulation — Compound Signature, Split Tokens).*
> *For all names `x`, processes `P`, `Q`, signatures `s₁`, `s₂`, and
> tokens `T₁`, `T₂`:*
>
>     S⟦(for(y ← x) P ∣ x!(Q))^{s₁ & s₂} ∥ s₁:T₁ ∥ s₂:T₂⟧
>       ⇝*  P{@Q/y} ∣ (*(@(T⟦T₁⟧)) ∣ *(@(T⟦T₂⟧)))
>
> *`Ctx = 0`. The reduction takes exactly three `⇝`-steps.*

*Proof.* Unfolding the translations:

        S⟦LHS⟧ = P⟦(for(y←x) P ∣ x!(Q))^{s₁ & s₂}⟧ ∣ N⟦s₁⟧!(T⟦T₁⟧) ∣ N⟦s₂⟧!(T⟦T₂⟧)

Since the signature is compound, `P⟦·⟧` uses nested fuel gates:

        = for(t₁ ← N⟦s₁⟧) for(t₂ ← N⟦s₂⟧)( (for(y←x) P ∣ x!(Q))↑² ∣ *t₁ ∣ *t₂ )
          ∣ N⟦s₁⟧!(T⟦T₁⟧) ∣ N⟦s₂⟧!(T⟦T₂⟧)

**Step 1** *(outer fuel gate fires on `N⟦s₁⟧`)*: The outer input
`for(t₁ ← N⟦s₁⟧)(...)` and the output `N⟦s₁⟧!(T⟦T₁⟧)` form a COMM
redex. By the COMM rule, `t₁` (de Bruijn index 1 inside the inner body,
index 0 at the outer level) is replaced by `@(T⟦T₁⟧)`. By the
substitution-lifting lemma for double lifts, substituting at index 1 in a
process lifted by 2 yields the process lifted by 1, so
`(for(y←x) P ∣ x!(Q))↑²` becomes `(for(y←x) P ∣ x!(Q))↑¹`. Since
`T⟦T₁⟧` is closed, lifting it is the identity. The dereference `*t₁`
(i.e., `PDeref(NVar 1)`) becomes `*(@(T⟦T₁⟧))`. The state after Step 1
is:

        for(t₂ ← N⟦s₂⟧)( (for(y←x) P ∣ x!(Q))↑¹ ∣ *(@(T⟦T₁⟧)) ∣ *t₂ )
          ∣ N⟦s₂⟧!(T⟦T₂⟧)

**Step 2** *(inner fuel gate fires on `N⟦s₂⟧`)*: The inner input
`for(t₂ ← N⟦s₂⟧)(...)` and the remaining output `N⟦s₂⟧!(T⟦T₂⟧)` form a
COMM redex. The substitution replaces `t₂` (index 0) with `@(T⟦T₂⟧)`.
By `subst_lift_zero`, `(for(y←x) P ∣ x!(Q))↑¹` reduces to the original
`for(y←x) P ∣ x!(Q)`. Since `T⟦T₁⟧` is closed, substitution leaves the
residue `*(@(T⟦T₁⟧))` unchanged. The dereference `*t₂` becomes
`*(@(T⟦T₂⟧))`. The state after Step 2 is:

        (for(y ← x) P ∣ x!(Q)) ∣ (*(@(T⟦T₁⟧)) ∣ *(@(T⟦T₂⟧)))

**Step 3** *(inner COMM fires on `x`)*: The sub-processes `for(y ← x) P`
and `x!(Q)` share channel `x`. By the PAR rule, the COMM fires under the
residues:

        ⇝  P{@Q/y} ∣ (*(@(T⟦T₁⟧)) ∣ *(@(T⟦T₂⟧)))  ∎

---

> **Lemma 9.5.3** *(Rule 3 Simulation — Compound Signature, Combined Token).*
> *For all names `x`, processes `P`, `Q`, signatures `s₁`, `s₂`, and
> token `T`, with `Ctx = Split(s₁, s₂)`:*
>
>     S⟦(for(y ← x) P ∣ x!(Q))^{s₁ & s₂} ∥ (s₁ & s₂):T⟧ ∣ Ctx
>       ⇝*  P{@Q/y} ∣ (*(@0) ∣ *(@(*(@(T⟦T⟧)))))
>
> *The reduction takes exactly four `⇝`-steps.*

*Proof.* Unfolding the translations, the LHS becomes:

        P⟦(for(y←x) P ∣ x!(Q))^{s₁ & s₂}⟧ ∣ N⟦s₁ & s₂⟧!(T⟦T⟧) ∣ Split(s₁, s₂)

The compound fuel gate unfolds to:

        for(t₁ ← N⟦s₁⟧) for(t₂ ← N⟦s₂⟧)( (for(y←x) P ∣ x!(Q))↑² ∣ *t₁ ∣ *t₂ )

**Step 1** *(Split fires on `N⟦s₁ & s₂⟧`)*: By Lemma 9.3, the Split
mediator and the compound token output form a COMM redex on channel
`N⟦s₁ & s₂⟧`. After firing:

        Split(s₁, s₂) ∣ N⟦s₁ & s₂⟧!(T⟦T⟧)  ⇝  N⟦s₁⟧!(0) ∣ N⟦s₂⟧!(*(@(T⟦T⟧)))

By associativity and commutativity of `∣` (`≡`), the full state
rearranges to:

        for(t₁ ← N⟦s₁⟧) for(t₂ ← N⟦s₂⟧)(...) ∣ N⟦s₁⟧!(0) ∣ N⟦s₂⟧!(*(@(T⟦T⟧)))

**Step 2** *(outer fuel gate fires on `N⟦s₁⟧`)*: The outer input
`for(t₁ ← N⟦s₁⟧)(...)` and the output `N⟦s₁⟧!(0)` form a COMM redex.
The substitution replaces `t₁` with `@0`. By the double-lift substitution
lemma, `(for(y←x) P ∣ x!(Q))↑²` reduces to `(for(y←x) P ∣ x!(Q))↑¹`.
Since `0` is closed, `lift_proc(1, 0, 0) = 0`. The dereference `*t₁`
becomes `*(@0)`. The state is:

        for(t₂ ← N⟦s₂⟧)( (for(y←x) P ∣ x!(Q))↑¹ ∣ *(@0) ∣ *t₂ )
          ∣ N⟦s₂⟧!(*(@(T⟦T⟧)))

**Step 3** *(inner fuel gate fires on `N⟦s₂⟧`)*: The inner input
`for(t₂ ← N⟦s₂⟧)(...)` and the output `N⟦s₂⟧!(*(@(T⟦T⟧)))` form a
COMM redex. The substitution replaces `t₂` (index 0) with
`@(*(@(T⟦T⟧)))`. By `subst_lift_zero`, the lifted redex recovers the
original `for(y←x) P ∣ x!(Q)`. The existing residue `*(@0)` is closed,
so substitution leaves it unchanged. The dereference `*t₂` becomes
`*(@(*(@(T⟦T⟧))))`. The state is:

        (for(y ← x) P ∣ x!(Q)) ∣ (*(@0) ∣ *(@(*(@(T⟦T⟧)))))

**Step 4** *(inner COMM fires on `x`)*: The sub-processes `for(y ← x) P`
and `x!(Q)` share channel `x`. By the PAR rule:

        ⇝  P{@Q/y} ∣ (*(@0) ∣ *(@(*(@(T⟦T⟧)))))  ∎

---

> **Lemma 9.5.4** *(May Rule 5 Simulation — Split Processes, Combined Token; April Rule 4).*
> *For all names `x`, processes `P`, `Q`, atomic signatures `s₁`, `s₂`,
> and token `T`, with `Ctx = Split(s₁, s₂)`:*
>
>     S⟦(for(y ← x) P)^{s₁} ∥ (x!(Q))^{s₂} ∥ (s₁ & s₂):T⟧ ∣ Ctx
>       ⇝*  P{@Q/y} ∣ (*(@0) ∣ *(@(*(@(T⟦T⟧)))))
>
> *The reduction takes exactly four `⇝`-steps (atomic sub-case).*

*Proof.* Unfolding the translations:

        S⟦LHS⟧ = P⟦(for(y ← x) P)^{s₁}⟧ ∣ P⟦(x!(Q))^{s₂}⟧ ∣ N⟦s₁ & s₂⟧!(T⟦T⟧)

Since `s₁` and `s₂` are atomic, each fuel gate is a single `PInput`:

        P⟦(for(y ← x) P)^{s₁}⟧ = for(t₁ ← N⟦s₁⟧)( (for(y ← x) P)↑¹ ∣ *t₁ )
        P⟦(x!(Q))^{s₂}⟧ = for(t₂ ← N⟦s₂⟧)( (x!(Q))↑¹ ∣ *t₂ )

Adding the Split mediator, the full starting state is:

        for(t₁ ← N⟦s₁⟧)(...) ∣ for(t₂ ← N⟦s₂⟧)(...) ∣ N⟦s₁ & s₂⟧!(T⟦T⟧) ∣ Split(s₁, s₂)

**Step 1** *(Split fires on `N⟦s₁ & s₂⟧`)*: By Lemma 9.3:

        Split(s₁, s₂) ∣ N⟦s₁ & s₂⟧!(T⟦T⟧)  ⇝  N⟦s₁⟧!(0) ∣ N⟦s₂⟧!(*(@(T⟦T⟧)))

By `≡` (associativity and commutativity of `∣`), the full state
rearranges to pair each fuel gate with its matching atomic token:

        (for(t₁ ← N⟦s₁⟧)(...) ∣ N⟦s₁⟧!(0))
          ∣ (for(t₂ ← N⟦s₂⟧)(...) ∣ N⟦s₂⟧!(*(@(T⟦T⟧))))

**Step 2** *(s₁-gate fires on `N⟦s₁⟧`)*: The fuel gate
`for(t₁ ← N⟦s₁⟧)((for(y←x) P)↑¹ ∣ *t₁)` and the output `N⟦s₁⟧!(0)`
form a COMM redex. By `subst_lift_zero`, the lifted input process
recovers the original. The dereference `*t₁` becomes `*(@0)`. The left
component becomes:

        for(y ← x) P ∣ *(@0)

**Step 3** *(s₂-gate fires on `N⟦s₂⟧`)*: The fuel gate
`for(t₂ ← N⟦s₂⟧)((x!(Q))↑¹ ∣ *t₂)` and the output
`N⟦s₂⟧!(*(@(T⟦T⟧)))` form a COMM redex. By `subst_lift_zero`, the
lifted output process recovers the original. The dereference `*t₂`
becomes `*(@(*(@(T⟦T⟧))))`. The right component becomes:

        x!(Q) ∣ *(@(*(@(T⟦T⟧))))

The full state is now:

        (for(y ← x) P ∣ *(@0)) ∣ (x!(Q) ∣ *(@(*(@(T⟦T⟧)))))

By `≡`, this rearranges to:

        (for(y ← x) P ∣ x!(Q)) ∣ (*(@0) ∣ *(@(*(@(T⟦T⟧)))))

**Step 4** *(inner COMM fires on `x`)*: The sub-processes `for(y ← x) P`
and `x!(Q)` share channel `x`. By the PAR rule:

        ⇝  P{@Q/y} ∣ (*(@0) ∣ *(@(*(@(T⟦T⟧)))))

**Compound sub-case** (`s₁ = u & v`, `s₂` atomic). The proof is
structurally analogous to the atomic case above — the same four-phase
pattern (Split, gate₁, gate₂, inner COMM) applies — but requires one
additional inner Split and the compound gate fires in two sub-steps
rather than one. Set `Ctx = Split(u & v, s₂) ∣ Split(u, v)`.

Unfolding: `P⟦(for(y ← x) P)^{u & v}⟧` is a nested two-layer fuel gate
(outer on `N⟦u⟧`, inner on `N⟦v⟧`), and `P⟦(x!(Q))^{s₂}⟧` is a single
fuel gate on `N⟦s₂⟧`. The combined token lives on `N⟦(u & v) & s₂⟧`.

**Step 1** *(outer Split fires on `N⟦(u & v) & s₂⟧`)*: By Lemma 9.3,
produces `N⟦u & v⟧!(0) ∣ N⟦s₂⟧!(*(@(T⟦T⟧)))`.

**Step 2** *(inner Split fires on `N⟦u & v⟧`)*: The inner Split
`Split(u, v)` and the output `N⟦u & v⟧!(0)` fire via Lemma 9.3,
producing `N⟦u⟧!(0) ∣ N⟦v⟧!(*(@0))`.

**Steps 3–4** *(compound gate for `s₁ = u & v` fires in two sub-steps)*:
By Lemma 9.4 with `M_u = 0` and `M_v = *(@0)`, the nested fuel gate
`P⟦(for(y ← x) P)^{u & v}⟧` consumes `N⟦u⟧!(0)` and `N⟦v⟧!(*(@0))`,
exposing `for(y ← x) P` with residues `*(@0) ∣ *(@(*(@0)))`.

**Step 5** *(atomic gate for `s₂` fires on `N⟦s₂⟧`)*: By
`subst_lift_zero`, the gate `P⟦(x!(Q))^{s₂}⟧` consumes
`N⟦s₂⟧!(*(@(T⟦T⟧)))`, exposing `x!(Q)` with residue
`*(@(*(@(T⟦T⟧))))`.

**Step 6** *(inner COMM fires on `x`)*: After structural rearrangement
(`≡`) to bring `for(y ← x) P` and `x!(Q)` adjacent, the COMM fires:

        ⇝  P{@Q/y} ∣ (*(@0) ∣ *(@(*(@0))) ∣ *(@(*(@(T⟦T⟧)))))

**Compound sub-case** (`s₁` atomic, `s₂ = u & v`). Set
`Ctx = Split(s₁ & (u & v), s₁) ∣ Split(u, v)` (note: the outer Split
decomposes the combined token; the inner Split atomizes the compound
sub-signature). Step 1: outer Split fires. Step 2: inner Split fires on
the `u & v` half. Step 3: `s₁`-gate fires (atomic, one step). Steps 4–5:
`s₂`-gate fires (compound, two steps via Lemma 9.4). Step 6: inner COMM
on `x`. Total: 6 `⇝`-steps.

**Compound sub-case** (both compound: `s₁ = u₁ & v₁`, `s₂ = u₂ & v₂`).
Set `Ctx = Split(u₁ & v₁, u₂ & v₂) ∣ Split(u₁, v₁) ∣ Split(u₂, v₂)`.
Step 1: outer Split fires on `N⟦(u₁ & v₁) & (u₂ & v₂)⟧`. Step 2: left
inner Split fires on `N⟦u₁ & v₁⟧`, producing `N⟦u₁⟧!(0) ∣ N⟦v₁⟧!(...)`.
Step 3: right inner Split fires on `N⟦u₂ & v₂⟧`, producing
`N⟦u₂⟧!(0) ∣ N⟦v₂⟧!(...)`. Steps 4–5: `s₁`-gate fires (compound, two
steps via Lemma 9.4). Steps 6–7: `s₂`-gate fires (compound, two steps
via Lemma 9.4). Step 8: inner COMM on `x`. Total: 8 `⇝`-steps.  ∎

---

> **Lemma 9.5.5** *(May Rule 4 Simulation — Split Processes, Split Tokens; April Rule 5).*
> *For all names `x`, processes `P`, `Q`, atomic signatures `s₁`, `s₂`,
> and tokens `T₁`, `T₂`:*
>
>     S⟦(for(y ← x) P)^{s₁} ∥ (x!(Q))^{s₂} ∥ s₁:T₁ ∥ s₂:T₂⟧
>       ⇝*  W
>
> *where `W ≡ P{@Q/y} ∣ (*(@(T⟦T₁⟧)) ∣ *(@(T⟦T₂⟧)))`.
> `Ctx = 0`. The reduction takes exactly three `⇝`-steps (atomic
> sub-case).*

*Proof.* Unfolding the translations:

        S⟦LHS⟧ = P⟦(for(y ← x) P)^{s₁}⟧ ∣ P⟦(x!(Q))^{s₂}⟧
                   ∣ N⟦s₁⟧!(T⟦T₁⟧) ∣ N⟦s₂⟧!(T⟦T₂⟧)

Since `s₁` and `s₂` are atomic, the fuel gates are single `PInput`s:

        = for(t₁ ← N⟦s₁⟧)((for(y←x) P)↑¹ ∣ *t₁)
            ∣ for(t₂ ← N⟦s₂⟧)((x!(Q))↑¹ ∣ *t₂)
            ∣ N⟦s₁⟧!(T⟦T₁⟧) ∣ N⟦s₂⟧!(T⟦T₂⟧)

No Split is needed because the tokens are already on the correct atomic
channels.

**Step 1** *(s₁-gate fires on `N⟦s₁⟧`)*: By `≡` (associativity and
commutativity of `∣`), rearrange the state to pair the s₁-gate with the
s₁-token:

        (for(t₁ ← N⟦s₁⟧)((for(y←x) P)↑¹ ∣ *t₁) ∣ N⟦s₁⟧!(T⟦T₁⟧))
          ∣ (for(t₂ ← N⟦s₂⟧)((x!(Q))↑¹ ∣ *t₂) ∣ N⟦s₂⟧!(T⟦T₂⟧))

The s₁-gate and the s₁-token form a COMM redex on `N⟦s₁⟧`. By
`subst_lift_zero`, the lifted input process recovers the original. The
dereference `*t₁` becomes `*(@(T⟦T₁⟧))`. The state after Step 1 is:

        (for(y ← x) P ∣ *(@(T⟦T₁⟧)))
          ∣ (for(t₂ ← N⟦s₂⟧)((x!(Q))↑¹ ∣ *t₂) ∣ N⟦s₂⟧!(T⟦T₂⟧))

**Step 2** *(s₂-gate fires on `N⟦s₂⟧`)*: The s₂-gate and the s₂-token
form a COMM redex on `N⟦s₂⟧`. By `subst_lift_zero`, the lifted output
process recovers the original. The dereference `*t₂` becomes
`*(@(T⟦T₂⟧))`. The state after Step 2 is:

        (for(y ← x) P ∣ *(@(T⟦T₁⟧)))  ∣  (x!(Q) ∣ *(@(T⟦T₂⟧)))

**Step 3** *(inner COMM fires on `x`)*: By `≡`, rearrange to bring the
COMM partners adjacent:

        (for(y ← x) P ∣ x!(Q))  ∣  (*(@(T⟦T₁⟧)) ∣ *(@(T⟦T₂⟧)))

The sub-processes `for(y ← x) P` and `x!(Q)` share channel `x`. By the
PAR rule:

        ⇝  P{@Q/y}  ∣  (*(@(T⟦T₁⟧)) ∣ *(@(T⟦T₂⟧)))

The structural rearrangement from Step 3's starting state to the
displayed form before COMM firing is justified by associativity and
commutativity of `∣`. The final witness `W` reached by the three
reduction steps satisfies
`W ≡ P{@Q/y} ∣ (*(@(T⟦T₁⟧)) ∣ *(@(T⟦T₂⟧)))`, with the `≡` arising
from a single application of associativity.

**Compound sub-case** (`s₁ = u & v`, `s₂` atomic). The proof is
structurally analogous to the atomic case — the same three-phase pattern
(gate₁, gate₂, inner COMM) applies — but the compound gate fires in two
sub-steps and requires a Split to atomize its token first. Set
`Ctx = Split(u, v)`.

The token `(u & v):T₁` lives on the compound channel `N⟦u & v⟧`, but the
nested fuel gate `P⟦(for(y ← x) P)^{u & v}⟧` listens on `N⟦u⟧` and
`N⟦v⟧`. The Split bridges this gap.

**Step 1** *(Split fires on `N⟦u & v⟧`)*: By Lemma 9.3,
`Split(u, v) ∣ N⟦u & v⟧!(T⟦T₁⟧) ⇝ N⟦u⟧!(0) ∣ N⟦v⟧!(*(@(T⟦T₁⟧)))`.

**Steps 2–3** *(compound gate for `s₁ = u & v` fires)*: By Lemma 9.4
with `M_u = 0` and `M_v = *(@(T⟦T₁⟧))`, the nested fuel gate consumes
the two atomic tokens in two `⇝`-steps, exposing `for(y ← x) P` with
residues `*(@0) ∣ *(@(*(@(T⟦T₁⟧))))`.

**Step 4** *(atomic gate for `s₂` fires on `N⟦s₂⟧`)*: By
`subst_lift_zero`, the gate `P⟦(x!(Q))^{s₂}⟧` consumes
`N⟦s₂⟧!(T⟦T₂⟧)`, exposing `x!(Q)` with residue `*(@(T⟦T₂⟧))`.

**Step 5** *(inner COMM fires on `x`)*: After structural rearrangement
(`≡`) to bring `for(y ← x) P` and `x!(Q)` adjacent, the COMM fires:

        ⇝  P{@Q/y} ∣ (*(@0) ∣ *(@(*(@(T⟦T₁⟧)))) ∣ *(@(T⟦T₂⟧)))

**Compound sub-case** (`s₁` atomic, `s₂ = u & v`). Set
`Ctx = Split(u, v)`. The token `(u & v):T₂` requires atomization. Step 1:
Split fires on `N⟦u & v⟧`, producing `N⟦u⟧!(0) ∣ N⟦v⟧!(*(@(T⟦T₂⟧)))`.
Step 2: `s₁`-gate fires (atomic, one step via `subst_lift_zero`).
Steps 3–4: `s₂`-gate fires (compound, two steps via Lemma 9.4).
Step 5: inner COMM on `x`. Total: 5 `⇝`-steps.

**Compound sub-case** (both compound: `s₁ = u₁ & v₁`,
`s₂ = u₂ & v₂`). Set `Ctx = Split(u₁, v₁) ∣ Split(u₂, v₂)`. Step 1:
left Split fires on `N⟦u₁ & v₁⟧`, atomizing `(u₁ & v₁):T₁`. Step 2:
right Split fires on `N⟦u₂ & v₂⟧`, atomizing `(u₂ & v₂):T₂`. Steps 3–4:
`s₁`-gate fires (compound, two steps via Lemma 9.4). Steps 5–6:
`s₂`-gate fires (compound, two steps via Lemma 9.4). Step 7: inner COMM
on `x`. Total: 7 `⇝`-steps.  ∎

---

> **Theorem 9.5** *(Contextual Forward Reachability — Generic).*
> *For all systems `S`, `S'`:*
>
>     S ⤳ S'  ⟹  ∃Ctx, W. closed(Ctx) ∧ S⟦S⟧ ∣ Ctx ⇝* W

*Proof.* By induction on the derivation of `S ⤳ S'`.

**Case `ca_rule1`:** Dispatched to `rule1_simulation_generic`, which
case-splits on the signature `s`. When `s` is atomic (`()` or
`hash(σ)`), the context is `Ctx = 0` and the simulation is Lemma 9.5.1
(2 steps, no mediator). When `s = s₁ & s₂`, the context is
`Ctx = Split(s₁, s₂)` and the simulation is Lemma 9.5.3 (4 steps).
Closedness: `closed(0)` is immediate; `closed(Split(s₁, s₂))` follows
from hypothesis H3 via `Split_closed`.

**Case `ca_rule2`:** Dispatched to Lemma 9.5.2. The context is `Ctx = 0`
(tokens already split). Closedness: `closed(0)`.

**Case `ca_rule3`:** Dispatched to Lemma 9.5.3. The context is
`Ctx = Split(s₁, s₂)`. Closedness: `Split_closed`.

**Case `ca_rule4`:** Dispatched to `rule4_simulation_generic`, which
case-splits on the atomicity of `s₁` and `s₂`. When both are atomic, the
simulation is Lemma 9.5.4 with `Ctx = Split(s₁, s₂)`. When one or both
are compound, additional inner `Split` mediators are composed in `Ctx`.
Closedness: `Split_closed` and `closed(P ∣ Q)` from `closed(P)` and
`closed(Q)`.

**Case `ca_rule5`:** Dispatched to `rule5_simulation_generic`, which
case-splits on the atomicity of `s₁` and `s₂`. When both are atomic, the
simulation is Lemma 9.5.5 with `Ctx = 0`. When one or both are compound,
`Ctx` includes `Split` mediators for the compound sides. Closedness:
`closed(0)` or `Split_closed`.

**Case `ca_par_l`:** The step has the form `S₁ ∥ S₂ ⤳ S₁' ∥ S₂` where
`S₁ ⤳ S₁'`. By the induction hypothesis, there exist `Ctx` and `W` with
`closed(Ctx)` and `S⟦S₁⟧ ∣ Ctx ⇝* W`. By compositionality
(`S⟦S₁ ∥ S₂⟧ = S⟦S₁⟧ ∣ S⟦S₂⟧`), the full source is
`S⟦S₁⟧ ∣ S⟦S₂⟧ ∣ Ctx`. Using `rho_reachable_par_l`, the
reachability `S⟦S₁⟧ ∣ Ctx ⇝* W` lifts to
`(S⟦S₁⟧ ∣ Ctx) ∣ S⟦S₂⟧ ⇝* W ∣ S⟦S₂⟧`. A structural rearrangement via
`≡` (associativity and commutativity of `∣`) aligns the source with the
LHS.

**Case `ca_par_r`:** The step has the form `S₁ ∥ S₂ ⤳ S₁ ∥ S₂'` where
`S₂ ⤳ S₂'`. The proof is symmetric to `ca_par_l`: by the induction
hypothesis, `S⟦S₂⟧ ∣ Ctx ⇝* W` for some closed `Ctx` and `W`. By
compositionality, the full source is `S⟦S₁⟧ ∣ S⟦S₂⟧ ∣ Ctx`. Using
`rho_reachable_par_r`, the reachability lifts to
`S⟦S₁⟧ ∣ (S⟦S₂⟧ ∣ Ctx) ⇝* S⟦S₁⟧ ∣ W`. A structural rearrangement via
`≡` aligns the source.  ∎

---

### 9.4 Bisimulation

> **Lemma 9.6** *(Backward Simulation of Stuck Parallel).*
> *For all processes `P` and `W`:*
>
>     W ≡ P ∣ *(@0)  ∧  W ⇝ W'
>     ⟹  ∃P'. P ⇝ P'  ∧  W' ≡ P' ∣ *(@0)

*Proof.* By induction on the derivation of `W ⇝ W'`.

**Case `rs_comm`:** The source is literally `for(y ← x) B ∣ x!(C)` for
some `x`, `B`, `C`. This has `head_count = 2`. But `W ≡ P ∣ *(@0)` has
`head_count = head_count(P) + 1`. By the heads-list permutation theorem
(Section 8.2), the two heads of the rs_comm source must be a permutation
of the heads of `P ∣ *(@0)`. Since `*(@0)` is a `PDeref` (not a
`PInput` or `POutput`), it cannot serve as either the input or output
partner of a COMM. By `count_inputs` / `count_outputs` preservation under
`≡`, the COMM's input and output must both come from the heads of `P`.
The inductive analysis on the heads-list yields `P'` with `P ⇝ P'` and
`W' ≡ P' ∣ *(@0)`.

**Case `rs_par_l`:** `W = A ∣ B` and `A ⇝ A'`, `W' = A' ∣ B`.
By `head_count_se` on `W ≡ P ∣ *(@0)`, we have
`head_count(A) + head_count(B) = head_count(P) + 1`. Since
`rho_step_head_count_ge_two` gives `head_count(A) ≥ 2`, we have
`head_count(B) ≤ head_count(P) - 1`. If `head_count(B) = 0`, then
`B ≡ 0` and `A ≡ P ∣ *(@0)`. Apply the induction hypothesis to `A` to
get `P'`. If `head_count(B) = 1`, then by the heads-list analysis, `B`
is equivalent to `*(@0)` (the unique `PDeref` head), and `A ≡ P`. Then
`A ⇝ A'` gives `P ⇝ A'` (after absorbing the `≡`), and
`W' = A' ∣ B ≡ A' ∣ *(@0)`. Take `P' = A'`.

**Case `rs_par_r`:** `W = B ∣ A`, `A ⇝ A'`, `W' = B ∣ A'`. The proof
is symmetric to `rs_par_l`: by `head_count_se` on `W ≡ P ∣ *(@0)`,
`head_count(A) + head_count(B) = head_count(P) + 1`, and
`head_count(A) ≥ 2`. If `head_count(B) = 0`, then `B ≡ 0` and
`A ≡ P ∣ *(@0)`; apply the induction hypothesis. If `head_count(B) = 1`,
then by the heads-list analysis, `B ≡ *(@0)` and `A ≡ P`, so `A ⇝ A'`
gives `P ⇝ A'`, and `W' = B ∣ A' ≡ *(@0) ∣ A' ≡ A' ∣ *(@0)` by
commutativity. Take `P' = A'`.

**Case `rs_struct`:** `W ≡ W₁`, `W₁ ⇝ W₁'`, `W₁' ≡ W'`. By
composing `W ≡ P ∣ *(@0)` with `W ≡ W₁`, we get
`W₁ ≡ P ∣ *(@0)`. Apply the induction hypothesis to `W₁ ⇝ W₁'` to get
`P'` with `P ⇝ P'` and `W₁' ≡ P' ∣ *(@0)`. Composing with
`W₁' ≡ W'` gives `W' ≡ P' ∣ *(@0)`.  ∎

---

> **Theorem 9.7** *(Post-Gate Bisimulation).*
> *For all processes `P`:*
>
>     (P ∣ *(@0)) ~~ P

*Proof.* We exhibit the relation `R = { (W, P) ∣ W ≡ P ∣ *(@0) }` and
show it is a bisimulation. By definition of `~~`, we must verify two
directions.

**Forward** (`W ⇝ W'` implies `∃P'. P ⇝ P' ∧ (W', P') ∈ R`):
Given `W ⇝ W'` and `W ≡ P ∣ *(@0)`, by Lemma 9.6 there exists `P'`
with `P ⇝ P'` and `W' ≡ P' ∣ *(@0)`. The pair `(W', P')` is in `R` by
definition.

**Backward** (`P ⇝ P'` implies `∃W'. W ⇝ W' ∧ (W', P') ∈ R`):
Take `W' = P' ∣ *(@0)`. Since `W ≡ P ∣ *(@0)`, we apply the STRUCT
rule: `W ≡ P ∣ *(@0)`, then `rs_par_l` on `P ⇝ P'` gives
`P ∣ *(@0) ⇝ P' ∣ *(@0) = W'`, so `W ⇝ W'` via STRUCT. The pair
`(W', P')` is in `R` by `W' ≡ P' ∣ *(@0)` (reflexivity of `≡`).

Since `R` is a bisimulation and `(P ∣ *(@0), P) ∈ R` (by reflexivity of
`≡`), we conclude `(P ∣ *(@0)) ~~ P`.

*Remark.* In the Rocq mechanization, this proof is constructed as a
`CoFixpoint` — a coinductive term that satisfies Rocq's guardedness
condition by placing each recursive invocation immediately under the
`bisim_intro` constructor. See Section 8.1 for details.  ∎

---

> **Lemma 9.8** *(Multi-Stuck Residue Bisimulation).*
> *For all processes `P` and `R` with
> `count_inputs(R) + count_outputs(R) + count_replicates(R) = 0`:*
>
>     (P ∣ R) ~~ P

*Proof.* By structural induction on `R`. The hypothesis
`count_inputs(R) + count_outputs(R) + count_replicates(R) = 0` (denoted
`head_count_inputs_outputs(R) = 0` in the mechanization) ensures `R` has
no input heads, no output heads, and no replicated sub-processes.

**Case `R = 0`:** `P ∣ 0 ≡ P` by the identity axiom. Since `≡`
preserves bisimilarity, `(P ∣ 0) ~~ P`.

**Case `R = for(y ← x) B`:** `count_inputs(R) = 1 ≥ 1`, contradicting
the hypothesis (sum = 0 requires each summand = 0).

**Case `R = x!(B)`:** `count_outputs(R) = 1 ≥ 1`, contradicting the
hypothesis.

**Case `R = R₁ ∣ R₂`:** From the hypothesis, all six individual counts
(`count_inputs(R₁)`, `count_outputs(R₁)`, `count_replicates(R₁)`, and
the same for `R₂`) are zero (since all are non-negative and their sum is
zero). By the induction hypothesis on `R₁`:
`(P ∣ R₁) ~~ P`. By the induction hypothesis on `R₂` (applied with
`P ∣ R₁` in place of `P`):
`((P ∣ R₁) ∣ R₂) ~~ (P ∣ R₁)`. Now:

        P ∣ (R₁ ∣ R₂)  ≡  (P ∣ R₁) ∣ R₂     (by associativity)
                          ~~  P ∣ R₁        (by IH on R₂)
                          ~~  P             (by IH on R₁)

Composing via transitivity of `~~` gives `(P ∣ (R₁ ∣ R₂)) ~~ P`.

**Case `R = *n`:** `count_inputs(*n) = 0`, `count_outputs(*n) = 0`, and
`count_replicates(*n) = 0`. The process `*n` is a `PDeref` — it has no
input or output barbs and cannot participate in any COMM. By Theorem 9.7
(generalized to arbitrary stuck `PDeref` residues via the same
coinductive argument), `(P ∣ *n) ~~ P`.

**Case `R = !R'`:** `count_replicates(!R') = 1 ≥ 1`, contradicting the
hypothesis (sum = 0 requires `count_replicates = 0`).  ∎

---

> **Theorem 9.9** *(Generic Bisimulation).*
> *For all signatures `s` and processes `P`:*
>
>     ∃Ctx, W. closed(Ctx)  ∧  S⟦P^s ∥ s:()⟧ ∣ Ctx ⇝* W  ∧  W ~~ P

*Proof.* By case analysis on `s`.

**Case `s = ()`:** Take `Ctx = 0` and
`W = P ∣ *(@0)`. Closedness: `closed(0)` is immediate.
Reachability: `S⟦P^{()} ∥ ():()⟧ ∣ 0 ≡ S⟦P^{()} ∥ ():()⟧`. The fuel
gate fires in one `⇝`-step (by the COMM rule on channel `N⟦()⟧ = @0`),
and by `subst_lift_zero` the result is `P ∣ *(@0)`.
Bisimilarity: by Theorem 9.7, `(P ∣ *(@0)) ~~ P`.

**Case `s = hash(σ)`:** Identical to the unit case with channel
`N⟦hash(σ)⟧ = @H_σ` instead of `@0`.

**Case `s = s₁ & s₂`:** Take `Ctx = Split(s₁, s₂)` and
`W = P ∣ (*(@0) ∣ *(@(*(@0))))`. Closedness: by `Split_closed` (using
hypothesis H3). Reachability:

1. The Split fires (Lemma 9.3), producing atomic tokens on `N⟦s₁⟧` and
   `N⟦s₂⟧`.
2. The compound gates fire (Lemma 9.4), producing `W`.

This gives `S⟦P^{s₁ & s₂} ∥ (s₁ & s₂):()⟧ ∣ Split(s₁, s₂) ⇝* W` in
three `⇝`-steps. Bisimilarity: the residue
`*(@0) ∣ *(@(*(@0)))` has `count_inputs = 0` and `count_outputs = 0`.
By Lemma 9.8, `(P ∣ (*(@0) ∣ *(@(*(@0))))) ~~ P`.  ∎

---

### 9.5 Per-Step Reverse Simulation

> **Lemma 9.10** *(Channel Size Preservation).*
> *For all signatures `s₁`, `s₂`:*
>
>     N⟦s₁⟧ ≡_N N⟦s₂⟧  ⟹  |s₁| = |s₂|
>
> *where `|s|` denotes `sig_size(s)`.*

*Proof.* By induction on `s₁` with nested case analysis on `s₂`.

Since `N⟦s⟧ = @(proc_of(s))` for all `s`, the hypothesis
`N⟦s₁⟧ ≡_N N⟦s₂⟧` implies `proc_of(s₁) ≡ proc_of(s₂)` (by inversion
on `≡_N` for quoted names). We use `head_count_se` throughout: if
`proc_of(s₁) ≡ proc_of(s₂)`, then
`head_count(proc_of(s₁)) = head_count(proc_of(s₂))`.

The head counts of the underlying processes are:
- `proc_of(()) = 0` → `head_count = 0`
- `proc_of(hash(σ)) = H_σ` → `head_count = 1` (by hypothesis H4)
- `proc_of(s₁ & s₂) = *N⟦s₁⟧ ∣ *N⟦s₂⟧` → `head_count = 2`

**Base cases** (cross-category pairs): The three head counts 0, 1, 2 are
pairwise distinct. Any cross-category pair (e.g., `SUnit` vs. `SHash`,
`SHash` vs. `SAnd`) yields a head-count contradiction. Therefore
`|s₁| = |s₂|` holds vacuously (the hypothesis is false).

**Inductive case** (`s₁ = t₁ & t₂`, `s₂ = u₁ & u₂`): Both sides have
head count 2. By the heads-list permutation theorem,
`[*N⟦t₁⟧, *N⟦t₂⟧]` is perm-equivalent to `[*N⟦u₁⟧, *N⟦u₂⟧]`.
By the two-element permutation lemma, there are two sub-cases:

- *Identity pairing:* `*N⟦t₁⟧ ≡ *N⟦u₁⟧` and `*N⟦t₂⟧ ≡ *N⟦u₂⟧`. By
  `PDeref` injectivity, `N⟦t₁⟧ ≡_N N⟦u₁⟧` and `N⟦t₂⟧ ≡_N N⟦u₂⟧`. By
  the induction hypothesis, `|t₁| = |u₁|` and `|t₂| = |u₂|`. Therefore
  `|s₁| = 1 + |t₁| + |t₂| = 1 + |u₁| + |u₂| = |s₂|`.

- *Swap pairing:* `*N⟦t₁⟧ ≡ *N⟦u₂⟧` and `*N⟦t₂⟧ ≡ *N⟦u₁⟧`. By the
  same reasoning, `|t₁| = |u₂|` and `|t₂| = |u₁|`, so
  `|s₁| = 1 + |t₁| + |t₂| = 1 + |u₂| + |u₁| = |s₂|`.

**Same-category atomic pairs** (`SUnit` vs. `SUnit`, `SHash` vs.
`SHash`): `|s₁| = 1 = |s₂|` immediately.  ∎

---

> **Corollary 9.11** *(Signature Strictness).*
> *For all signatures `s₁`, `s₂`:*
>
>     ¬( N⟦s₁⟧ ≡_N N⟦s₁ & s₂⟧ )

*Proof.* Suppose for contradiction that `N⟦s₁⟧ ≡_N N⟦s₁ & s₂⟧`. By
Lemma 9.10, `|s₁| = |s₁ & s₂| = 1 + |s₁| + |s₂|`. This gives
`0 = 1 + |s₂|`. Since `|s₂| ≥ 1` (every signature has size at least 1),
we have `0 ≥ 2`, a contradiction.  ∎

---

> **Lemma 9.12** *(No-Outputs Irreducibility).*
> *For all processes `R`:*
>
>     count_outputs(R) = 0  ∧  count_replicates(R) = 0  ⟹  ¬(R ⇝ T)  *for any T*

*Proof.* By induction on the derivation of `R ⇝ T`.

**Case `rs_comm`:** The source is `for(y ← x) B ∣ x!(C)`, which has
`count_outputs = 1` (the output `x!(C)`). This contradicts
`count_outputs(R) = 0`.

**Case `rs_par_l`:** `R = A ∣ B` and `A ⇝ A'`. Since
`count_outputs(R) = count_outputs(A) + count_outputs(B) = 0` and both
summands are non-negative, `count_outputs(A) = 0`. Similarly,
`count_replicates(A) = 0` (from `count_replicates(R) = 0`). By the
induction hypothesis, `A` cannot step — contradiction.

**Case `rs_par_r`:** `R = B ∣ A` and `A ⇝ A'`. Since
`count_outputs(R) = count_outputs(B) + count_outputs(A) = 0` and both
summands are non-negative, `count_outputs(A) = 0`. Similarly,
`count_replicates(A) = 0`. By the induction hypothesis, `A` cannot
step — contradiction.

**Case `rs_replicate`:** The source is `!P` for some `P`.
`count_replicates(!P) = 1 ≥ 1`, contradicting `count_replicates(R) = 0`.

**Case `rs_struct`:** `R ≡ R'`, `R' ⇝ T'`, `T' ≡ T`. Since `≡`
preserves both `count_outputs` and `count_replicates`,
`count_outputs(R') = 0` and `count_replicates(R') = 0`. By the induction
hypothesis, `R'` cannot step — contradiction.  ∎

---

> **Lemma 9.13** *(Compound Gate Step Helper).*
> *For all processes `S`, `T` with `S ⇝ T`, and for all processes `P`,
> signatures `s₁`, `s₂`:*
>
>     S ≡ (P⟦P^{s₁ & s₂}⟧ ∣ N⟦s₁ & s₂⟧!(0)) ∣ Split(s₁, s₂)
>     ⟹  T ≡ (P⟦P^{s₁ & s₂}⟧ ∣ N⟦s₁⟧!(0)) ∣ N⟦s₂⟧!(*(@0))

*That is, any single step from the canonical 3-head compound form lands
at the post-split state (up to `≡`).*

*Proof.* By induction on the derivation of `S ⇝ T`. Let
`Canonical = (Gate ∣ TokOut) ∣ SplitP` where:
- `Gate = P⟦P^{s₁ & s₂}⟧` — a `PInput` on channel `N⟦s₁⟧`
- `TokOut = N⟦s₁ & s₂⟧!(0)` — a `POutput` on channel `N⟦s₁ & s₂⟧`
- `SplitP = Split(s₁, s₂)` — a `PInput` on channel `N⟦s₁ & s₂⟧`

Note `head_count(Canonical) = 3`.

**Case `rs_comm`:** The source is `for(y ← x) B ∣ x!(C)` with
`head_count = 2`. But `S ≡ Canonical` implies
`head_count(S) = head_count(Canonical) = 3` (by `head_count_se`). Since
`2 ≠ 3`, this case is impossible.

**Case `rs_par_l`:** `S = A ∣ B`, `A ⇝ A'`, `T = A' ∣ B`. By
`head_count_se`, `head_count(A) + head_count(B) = 3`. By
`rho_step_head_count_ge_two`, `head_count(A) ≥ 2`. Therefore
`head_count(B) ∈ {0, 1}`.

- **Sub-case `head_count(B) = 0`:** Then `B ≡ 0` and `A ≡ Canonical`.
  By the induction hypothesis on `A ⇝ A'`, `A' ≡ PostSplit`. Therefore
  `T = A' ∣ B ≡ A' ∣ 0 ≡ A' ≡ PostSplit`.

- **Sub-case `head_count(B) = 1`:** `B` carries exactly one of the three
  canonical heads. By the heads-list permutation theorem and the
  three-element permutation analysis, there are three sub-sub-cases:

  **(a) `B ≡ Gate`, `A ≡ TokOut ∣ SplitP`:** The pair {`TokOut`,
  `SplitP`} has matching channels (`N⟦s₁ & s₂⟧` on both). By an argument
  parallel to Lemma 9.3 (a `POutput`-`PInput` COMM redex), `A ⇝ A'`
  with `A' ≡ N⟦s₁⟧!(0) ∣ N⟦s₂⟧!(*(@0))`. Then:

        T = A' ∣ B ≡ (N⟦s₁⟧!(0) ∣ N⟦s₂⟧!(*(@0))) ∣ Gate
        ≡ (Gate ∣ N⟦s₁⟧!(0)) ∣ N⟦s₂⟧!(*(@0))     (by commutativity + associativity)
        = PostSplit

  **(b) `B ≡ TokOut`, `A ≡ Gate ∣ SplitP`:** Both `Gate` and `SplitP`
  are `PInput` heads, so `count_outputs(A) = 0`. By Lemma 9.12, `A`
  cannot step — contradicting `A ⇝ A'`.

  **(c) `B ≡ SplitP`, `A ≡ Gate ∣ TokOut`:** `Gate` is a `PInput` on
  `N⟦s₁⟧` and `TokOut` is a `POutput` on `N⟦s₁ & s₂⟧`. For a COMM to
  fire, these channels must be `≡_N`-equivalent. But by Corollary 9.11,
  `¬(N⟦s₁⟧ ≡_N N⟦s₁ & s₂⟧)` — contradiction.

**Case `rs_par_r`:** `S = B ∣ A`, `A ⇝ A'`, `T = B ∣ A'`. The proof is
symmetric to `rs_par_l`. By `head_count_se`,
`head_count(B) + head_count(A) = 3` and `head_count(A) ≥ 2`. The same
case split on `head_count(B) ∈ {0, 1}` applies. When `head_count(B) = 0`,
`B ≡ 0` and `A ≡ Canonical`; the induction hypothesis gives
`A' ≡ PostSplit`, so `T = B ∣ A' ≡ 0 ∣ A' ≡ A' ≡ PostSplit`. When
`head_count(B) = 1`, the heads-split analysis (with `A` and `B` swapped
via commutativity: `PPar A B ≡ PPar B A ≡ Canonical`) yields the same
three sub-cases (a), (b), (c), resolved identically.

**Case `rs_struct`:** `S ≡ S₁`, `S₁ ⇝ T₁`, `T₁ ≡ T`. Composing
`S ≡ Canonical` with `S ≡ S₁` gives `S₁ ≡ Canonical`. By the induction
hypothesis, `T₁ ≡ PostSplit`. Composing with `T₁ ≡ T` gives
`T ≡ PostSplit`.  ∎

---

> **Theorem 9.14** *(Generic Per-Step Reverse).*
> *For all signatures `s`, processes `P`, `Q`:*
>
>     gated_system(P, s) ⇝ Q
>     ⟹  ∃W. Q ⇝* W  ∧  W ≡ gate_final(P, s)

*Proof.* By case analysis on `s`.

**Case `s = ()`:** `gated_system(P, ()) = S⟦P^{()} ∥ ():()⟧`. The fuel
gate is a single `PInput` on `@0` in parallel with `POutput` on `@0`.
By the atomic per-step reverse helper (an argument parallel to Lemma 9.13
but for 2-head canonical forms), any step `Q` from this source satisfies
`Q ≡ P ∣ *(@0) = gate_final(P, ())`. Take `W = Q`, with `Q ⇝* Q` by
reflexivity.

**Case `s = hash(σ)`:** Identical to the unit case with channel
`@H_σ`.

**Case `s = s₁ & s₂`:** `gated_system(P, s₁ & s₂)` includes the Split
mediator. By Lemma 9.13, `Q ≡ PostSplit`. The post-split state reaches
`gate_final(P, s₁ & s₂)` in two additional `⇝`-steps via Lemma 9.4
(the outer and inner compound gates fire). Specifically:

1. Apply `rs_struct` to absorb `Q ≡ PostSplit`, then fire the outer gate.
2. Fire the inner gate.

Take `W = gate_final(P, s₁ & s₂)`, with `Q ⇝* W` by the two-step chain
and `W ≡ W` by reflexivity.  ∎

---

### 9.6 Fuel-Gate Safety

> **Theorem 9.15** *(Fuel Gate Stuck in Isolation).*
> *For all processes `P`, signatures `s`, and processes `R`:*
>
>     ¬( P⟦P^s⟧ ⇝ R )
>
> *A fuel-gated process alone cannot reduce.*

*Proof.* By case analysis on `s`.

**Case `s = ()`:** `P⟦P^{()}⟧ = for(t ← @0)( P↑¹ ∣ *t )`. This is a
single `PInput` prefix. By `PInput_alone_stuck` (a process consisting
solely of a `PInput` has `head_count = 1 < 2` and therefore cannot
step), `P⟦P^{()}⟧` is stuck.

**Case `s = hash(σ)`:** `P⟦P^{hash(σ)}⟧ = for(t ← @H_σ)( P↑¹ ∣ *t )`.
Again a single `PInput` — stuck by the same lemma.

**Case `s = s₁ & s₂`:**
`P⟦P^{s₁ & s₂}⟧ = for(t₁ ← N⟦s₁⟧) for(t₂ ← N⟦s₂⟧)( P↑² ∣ *t₁ ∣ *t₂ )`.
The outermost constructor is `PInput` — stuck by the same lemma.

In all cases, the fuel-gated translation is a `PInput` prefix with no
parallel output partner. Since COMM requires both an input and an output
on the same channel, and a lone `PInput` provides only the input, no
reduction is possible.  ∎

---

### 9.7 Fuel Event Multiset Determinism

A **fuel event** is a pair `(s, t)` recording the signature `s` and token
`t` consumed by a single gate firing. The function `fuel_events(S)`
collects the multiset of all such events latent in a system `S`. The
following three theorems establish that every reduction path through the
cost-accounted calculus consumes a uniquely determined multiset of fuel
events, regardless of the order in which independent redexes fire.

> **Theorem 9.16** *(Fuel Events Step Decomposition).*
> *For all systems `S`, `S'`:*
>
>     S ⤳ S'  ⟹  ∃consumed. consumed ≠ [] ∧
>       Permutation(fuel_events(S), consumed ++ fuel_events(S'))

*Proof.* By induction on the derivation of `S ⤳ S'` (7 cases).

**Case `ca_rule1`:** The step fires a single gate with signature `s` and
token `t`, consuming exactly one fuel event. Set
`consumed = [(s, t)]`. The pre-step fuel events decompose as
`fuel_events(S) = [(s, t)] ++ fuel_events(S')` by definition. Since
`[(s, t)] ≠ []` and `Permutation` is reflexive on this decomposition,
the conclusion holds.

**Case `ca_rule3`:** Same structure as Rule 1 — one compound gate is
stripped, yielding `consumed = [(s, t)]`. The arithmetic is identical.

**Case `ca_rule4`:** Same structure as Rule 1 — one gate is consumed
from a different redex shape, again yielding `consumed = [(s, t)]`.

**Case `ca_rule2`:** The step fires two gates simultaneously, consuming
tokens `t₁` on signature `s₁` and `t₂` on signature `s₂`. Set
`consumed = [(s₁, t₁); (s₂, t₂)]`. The pre-step fuel events satisfy
`fuel_events(S) = [(s₁, t₁); (s₂, t₂)] ++ fuel_events(S')`, but the two
consumed events may not appear at the head of the list. Apply
`Permutation_middle` to rearrange `fuel_events(S)` so that the two
consumed events are grouped at the front. Since `[(s₁, t₁); (s₂, t₂)] ≠ []`,
the conclusion holds.

**Case `ca_rule5`:** Same structure as Rule 2 — two gates are stripped.
Set `consumed = [(s₁, t₁); (s₂, t₂)]` and apply `Permutation_middle`
as above.

**Case `ca_par_l`:** The step has the form `S₁ ∥ S₂ ⤳ S₁' ∥ S₂` where
`S₁ ⤳ S₁'`. By the induction hypothesis, there exists `consumed` with
`consumed ≠ []` and
`Permutation(fuel_events(S₁), consumed ++ fuel_events(S₁'))`. Since
`fuel_events(S₁ ∥ S₂) = fuel_events(S₁) ++ fuel_events(S₂)` and
`fuel_events(S₁' ∥ S₂) = fuel_events(S₁') ++ fuel_events(S₂)`, apply
`Permutation_app_tail` (appending `fuel_events(S₂)` to both sides) and
`app_assoc` to obtain
`Permutation(fuel_events(S₁ ∥ S₂), consumed ++ fuel_events(S₁' ∥ S₂))`.

**Case `ca_par_r`:** Symmetric to `ca_par_l`. The step has the form
`S₁ ∥ S₂ ⤳ S₁ ∥ S₂'` where `S₂ ⤳ S₂'`. By the induction hypothesis
on `S₂`, there exists `consumed` with `consumed ≠ []` and
`Permutation(fuel_events(S₂), consumed ++ fuel_events(S₂'))`. Apply
`Permutation_app_head` (prepending `fuel_events(S₁)` to both sides) and
`app_assoc` to obtain
`Permutation(fuel_events(S₁ ∥ S₂), consumed ++ fuel_events(S₁ ∥ S₂'))`.

This exhausts all constructors of `⤳`.  ∎

---

> **Theorem 9.17** *(Fuel Events Reachable).*
> *For all systems `S`, `S'`:*
>
>     S ⤳* S'  ⟹  ∃consumed.
>       Permutation(fuel_events(S), consumed ++ fuel_events(S'))

*Proof.* By induction on the derivation of `S ⤳* S'`.

**Case `car_refl`:** `S' = S`. Set `consumed = []`. Then
`consumed ++ fuel_events(S') = [] ++ fuel_events(S) = fuel_events(S)`,
and `Permutation` is reflexive.

**Case `car_step`:** There exists an intermediate system `S₂` with
`S ⤳ S₂` and `S₂ ⤳* S'`. By Theorem 9.16, there exists `c₁` with
`Permutation(fuel_events(S), c₁ ++ fuel_events(S₂))`. By the induction
hypothesis, there exists `c₂` with
`Permutation(fuel_events(S₂), c₂ ++ fuel_events(S'))`. Substituting
the second permutation into the first (via `Permutation_app_head` on
`c₁`) and rewriting with `app_assoc`:

        Permutation(fuel_events(S), c₁ ++ (c₂ ++ fuel_events(S')))
        = Permutation(fuel_events(S), (c₁ ++ c₂) ++ fuel_events(S'))

Set `consumed = c₁ ++ c₂`.  ∎

---

> **Theorem 9.18** *(Consumed Events Determined by Endpoints).*
> *For all systems `S`, and lists `consumed₁`, `consumed₂`, `r₁`, `r₂`:*
>
>     Permutation(fuel_events(S), consumed₁ ++ r₁) →
>     Permutation(fuel_events(S), consumed₂ ++ r₂) →
>     Permutation(r₁, r₂) →
>     Permutation(consumed₁, consumed₂)

*Proof.* Pure `Permutation` algebra, requiring no domain-specific
knowledge of the cost-accounted calculus.

From the first two hypotheses, by symmetry and transitivity of
`Permutation`:

        Permutation(consumed₁ ++ r₁, consumed₂ ++ r₂)        ... (*)

From the third hypothesis `Permutation(r₁, r₂)`, apply
`Permutation_app_head` (prepending `consumed₂` to both sides):

        Permutation(consumed₂ ++ r₁, consumed₂ ++ r₂)        ... (**)

Compose `(*)` with the symmetry of `(**)`:

        Permutation(consumed₁ ++ r₁, consumed₂ ++ r₁)

Apply `Permutation_app_inv_r` (cancelling the common suffix `r₁`):

        Permutation(consumed₁, consumed₂)

This is the desired conclusion.  ∎

---

### 9.8 Replication Encoding Support

The theorems of Sections 6.5 and 6.6 close out the proof support
needed for Meredith–Radestock's replication encoding: the reflective
encoding unfolds operationally like a replicator, and every weak
input/output barb of the body propagates to both wrappers. Both facts
are mechanically proven without axioms.

#### 9.8.1 Operational unfold

> **Theorem 9.19** *(`bang_encoding_unfolds`,
> `theories/Replication.v:222`).*
> *For all names `x` and processes `P`:*
>
>     closed_name(x) ∧ closed_proc(P)
>        ⟹  bang_encoding(x, P) ⇝ bang_encoding(x, P) ∣ P

*Proof.* Let `B := D_encoding(x) ∣ P`. By definition,
`bang_encoding(x, P) = x⟨∣B∣⟩ ∣ D_encoding(x)`. Using `se_par_comm`
to put the receiver on the left, we observe that

        D_encoding(x) ∣ x⟨∣B∣⟩
           = for(y ← x){ x⟨∣*y∣⟩ ∣ *y } ∣ x⟨∣B∣⟩

is a COMM redex on channel *x*. The `rs_comm` rule produces

        (x⟨∣*y∣⟩ ∣ *y){@B/y}
           = x⟨∣*(@B)∣⟩ ∣ *(@B)

(substitution distributes through `PPar`; the `x` channel is shifted
under the input-binder and substitution leaves it unchanged because
it is closed by hypothesis). The semantic-substitution rule
`subst_proc_deref_nvar_eq_quote` (R.1 in `RhoSyntax.v`) collapses
`*(@B)` to `B`:

        = x⟨∣B∣⟩ ∣ B
           = x⟨∣D_encoding(x) ∣ P∣⟩ ∣ (D_encoding(x) ∣ P)

Re-associating via `se_par_assoc` and reversing the initial
`se_par_comm`:

        ≡ x⟨∣D_encoding(x) ∣ P∣⟩ ∣ D_encoding(x) ∣ P
           = bang_encoding(x, P) ∣ P

The whole sequence — pre-swap, COMM, post-associate — is packaged as
a single `rs_struct` application around an `rs_comm`. ∎

#### 9.8.2 Forward direction (no axioms)

> **Theorem 9.20** *(`preplicate_bang_encoding_body_barbs_sound`,
> `theories/Replication.v:1448`).*
> *For all `x`, `P`, `y`:*
>
>     closed_name(x) ∧ closed_proc(P)
>     ⟹  ( P ⇓ᵢ y  ⟹  PReplicate P ⇓ᵢ y  ∧  bang_encoding(x, P) ⇓ᵢ y )
>     ∧  ( P ⇓ₒ y  ⟹  PReplicate P ⇓ₒ y  ∧  bang_encoding(x, P) ⇓ₒ y )

*Proof (input case; output case dual).* Unpack `P ⇓ᵢ y` to some `P'`
and `y'` with `P ⇝* P'`, `y ≡ₙ y'`, and `input_barb P' y'`.

**Primitive side.** By `rs_replicate`,
`PReplicate P ⇝ PPar P (PReplicate P)`. Extending the reachability
on the left arm via `rho_reachable_par_l`:

        PReplicate P  ⇝  PPar P (PReplicate P)  ⇝*  PPar P' (PReplicate P)

The barb lifts via `input_barb_par_l`:
`input_barb (PPar P' (PReplicate P)) y'`. Package as
`PReplicate P ⇓ᵢ y`.

**Encoded side.** By Theorem 9.19 (`bang_encoding_unfolds`),
`bang_encoding x P ⇝ PPar (bang_encoding x P) P`. Extending the
reachability on the *right* arm via `rho_reachable_par_r`:

        bang_encoding x P  ⇝  PPar (bang_encoding x P) P
                             ⇝*  PPar (bang_encoding x P) P'

The barb lifts via `input_barb_par_r`. Package as
`bang_encoding x P ⇓ᵢ y`.

Each reachability extension is a single application of
`rho_reachable_par_l` or `rho_reachable_par_r`
(`WeakBarbedEquiv.v:122`, `:132`). **No axiom is used.** ∎

#### 9.8.3 Step inversion preserving the `PReplicate` factor

The reverse direction needs a stability lemma characterizing how a
step interacts with a state that contains a `PReplicate body` factor.

> **Lemma 9.21** *(`step_PPar_PReplicate_inv_se`,
> `theories/Replication.v` Section 14.C).*
>
>     rho_step S R
>        ∧ S ≡ PPar (PReplicate body) P_rest
>     ⟹  ∃P_rest'. R ≡ PPar (PReplicate body) P_rest'

*Proof (indexed induction on `rho_step S R`).* See Section 8.7 for
the technique. The five cases discharge as follows:

- `rs_comm`: discharged by `count_replicates_se` contradiction
  (LHS has `count_replicates = 0`, RHS has `count_replicates ≥ 1`).
- `rs_par_l`: apply `se_par_preplicate_locate` (Section 8.7) to the
  premise; recurse on the arm holding the PReplicate (case (a));
  rebuild directly when the step is on the disjoint arm (case (b)).
- `rs_par_r`: symmetric.
- `rs_struct`: chain `≡`'s via `se_trans`, recurse on the inner step
  with the composed premise; chain the IH's output with the outer
  `≡` via `se_trans` again.
- `rs_replicate`: `head_count` arithmetic forces `P_rest ≡ PNil`;
  apply `se_PReplicate_inj` to collapse `body ≡ P`; rebuild R via
  `se_par_comm` + body-rewriting.

The iterated version:

> **Corollary 9.22** *(`reachable_PPar_PReplicate_inv_se`).*
>
>     rho_reachable S Q
>        ∧ S ≡ PPar (PReplicate body) P_rest
>     ⟹  ∃P_rest'. Q ≡ PPar (PReplicate body) P_rest'

follows by induction on `rho_reachable`, applying Lemma 9.21 at each
`rr_step`.

#### 9.8.4 Closed replication boundary

The replication appendix stops at the axiom-free forward theorem:

> **Theorem 9.23** *(`replication_encoding_forward_barb_sound`,
> `theories/Replication.v:2063`).*
>
>     closed_name(x) ∧ closed_proc(body)
>     ⟹
>       (body ⇓ᵢ y ⟹
>          PReplicate body ⇓ᵢ y ∧ bang_encoding(x, body) ⇓ᵢ y)
>     ∧ (body ⇓ₒ y ⟹
>          PReplicate body ⇓ₒ y ∧ bang_encoding(x, body) ⇓ₒ y)

*Proof.* Immediate from Theorem 9.20
(`preplicate_bang_encoding_body_barbs_sound`). ∎

This boundary is intentional. A projection theorem of the form
`PReplicate body ⇓ y -> body ⇓ y` is stronger than the standard
replication law `!P ~ P | !P`: weak behavior can arise after several
unfolded copies interact, and that behavior need not be attributable to
one isolated copy of `body`. Likewise, the reflective encoding exposes
coordination-channel barbs that are not body behavior. The verified
cost-accounting design needs the operational unfold and the
body-to-wrapper propagation theorem, not a bidirectional wrapper/body
projection.

Beyond the universally-quantified Rocq theorems of Section 9, a
finite-state TLA+ model (Section 10) exhaustively checks every
scheduling interleaving for concrete instances of the cost-accounted
protocol. This provides an independent line of evidence — complementing
the proof by searching the state space — that the definitions themselves
(not only the theorems derived from them) are free of specification
errors.

---

## 10. TLA+ Correctness Model

### 10.1 Overview

The TLA+ model provides finite-state verification of the key properties
that the Rocq mechanization proves for the general case. Rocq establishes
theorems for systems of arbitrary size via structural induction and
coinduction; TLA+ exhaustively checks every reachable state and every
scheduling interleaving for concrete, small instances of the same
protocol. The two approaches are complementary: Rocq yields universal
guarantees, while TLA+ can catch specification bugs that a proof might
miss — for example, errors in the formalization of the operational
semantics, off-by-one mistakes in accounting invariants, or unexpected
deadlocks in mediator interactions. A property that is proven in Rocq,
exhausted by TLC, and accepted by Apalache's independent type checker and
bounded checker is, in practice, very unlikely to have been stated
incorrectly.

The model now consists of eight TLA+ specifications under
`formal/tlaplus/cost_accounted_rho/`, each adding a layer of generality:

1. **`CostAccountedRho.tla`** — The atomic fuel-gate protocol:
   processes with atomic signatures acquire fuel tokens via COMM events
   on signature channels. Checks token conservation, cost determinism,
   fuel-gate safety, and liveness. *(79 distinct states, 3 processes,
   3 channels.)*

2. **`CompoundProtocol.tla`** — Extends the model to compound signatures
   (`s₁ & s₂`) with Split mediators, nested two-layer fuel gates, and
   recursive eval dispatch (COMM bodies that spawn child processes).
   Adds Split ordering and inner gate ordering to the invariants of
   `CostAccountedRho`. *(63 distinct states, 4 processes, 6 channels.)*

3. **`FullProtocol.tla`** — The fully generalized model covering shared
   channels (multiple processes competing for the same token), arbitrary
   signature nesting (depth 0, 1, and 2 tested), Join mediators
   (combining atomic tokens into compound tokens — the inverse of
   `Split`), and cascading Splits. Adds gate ordering across arbitrary
   depths, Join accounting, and shared-channel contention.
   *(12,960 distinct states, 7 processes, 12 channels.)*

4. **`EvalScheduling.tla`** — Models the eval-loop scheduling problem
   directly. Compares the internalized model (fixed cost per body) with
   the externalized model (order-dependent cost). Demonstrates that the
   internalized model produces deterministic total cost while the
   externalized model does not. *(16 distinct states, 3 bodies.)*

5. **`RuntimeBudgetReplay.tla`** — Models the bounded Rust
   `RuntimeBudget` admission/replay trace state machine, including OOP
   boundary commitment, canonical permit grants, no-unpaid-work ordering,
   invalid event rejection, trace caps, canonical digest-entry tagging over
   the Rust event descriptor tuple, duplicate event occurrence
   multiplicity, and finalization reads followed by deploy reset.
   *(72 distinct states / 203 generated states, 6 events.)*

6. **`CostAccountingThreats.tla`** — Models replay tampering,
   activation downgrade attempts, unauthorized settlement, cost-invalid
   evidence recording, settlement/fuel separation, recovered rejected
   slashes, current evidence epochs, parent-pre-state slash
   authorization, ambient-bond rejection, and zero-bond slash no-ops.
   *(5,408 distinct states / 401,025 generated states.)*

7. **`CostAccountingSearchFrontier.tla`** — Models the witness
   classification rule used by the search horizon: generated witnesses
   cannot motivate implementation changes until they reproduce on the production
   Rust path or violate a production-path invariant. The model also checks
   the v3 stateful-search metadata discipline: campaign witnesses must name
   operation steps, production-path differentials must name oracle and Rust
   path evidence, exploit cross-products must carry a threat family and
   expected invariant, and source-graph slashing witnesses must carry
   current-evidence and parent-pre-state metadata before terminal
   classification. *(34,167 distinct states / 266,015 generated states.)*

8. **`MergeableChannelAccounting.tla`** — Models the post-slashing-merge
   typed mergeable-channel surface. It checks that `BitmaskOr` diffs replay
   to `previous OR current`, that `IntegerAdd` retains additive round trips,
   that OR merge cannot drop set bits, that non-numeric tagged payloads stay
   outside numeric merge accounting, and that mergeable/slash system metadata
   updates preserve user cost and settlement cost evidence. *(2,656 distinct
   states / 8,992 generated states.)*

### 10.2 Module Structure

**`CostAccountedRho.tla`**

Constants:
- `Processes`: set of process identifiers (e.g., `{p1, p2, p3}`)
- `Channels`: set of channel identifiers (e.g., `{ch_a, ch_b, ch_c}`)
- `InitialTokens`: function from processes to natural numbers (initial
  fuel per process)
- `sigChannel`: injective function from processes to channels (each
  process has a unique signature channel)

Variables:
- `fuel`: function from processes to natural numbers (remaining fuel)
- `gateOpen`: function from processes to booleans (fuel gate has fired)
- `commDone`: function from processes to booleans (inner COMM completed)
- `totalConsumed`: natural number (running total of tokens consumed)
- `pendingTokens`: function from channels to natural numbers (token
  messages on channels)
- `schedule`: sequence of process IDs (order of COMM firings so far)

Actions:
- `FuelGateFires(p)`: process `p`'s fuel gate fires, consuming one token
  from `sigChannel[p]`, incrementing `fuel[p]` and `totalConsumed`,
  opening the gate.
- `InnerCommFires(p)`: process `p`'s inner COMM fires (requires gate
  open), marking `commDone[p]`.

**`EvalScheduling.tla`**

Constants:
- `Bodies`: set of body identifiers (e.g., `{b1, b2, b3}`)
- `CostPerToken`: natural number (cost of consuming one fuel token)
- `StorageCostA`: natural number (externalized cost when body stores
  first)
- `StorageCostB`: natural number (externalized cost when body stores
  second, `!= StorageCostA`)

Variables:
- `executed`: set of bodies that have completed execution
- `totalCost`: natural number (internalized-model running cost)
- `extCost`: natural number (externalized-model running cost)
- `orderSoFar`: sequence of bodies (execution-order trace)
- `channelTouches`: natural number (number of bodies that have touched
  the shared channel)

Actions:
- `ExecuteBody(b)`: execute body `b`. Internalized cost increases by
  `CostPerToken`. Externalized cost increases by `StorageCostA` if
  `channelTouches = 0`, else `StorageCostB`.

**`MC.tla`** (model-checking instance for `CostAccountedRho`):

Concrete values: 3 processes (`p1, p2, p3`), 3 channels
(`ch_a, ch_b, ch_c`), each process gets 1 initial token, each process
has a unique signature channel.

**`MCEval.tla`** (model-checking instance for `EvalScheduling`):

Concrete values: 3 bodies (`b1, b2, b3`), `CostPerToken = 1`,
`StorageCostA = 10`, `StorageCostB = 15`.

**`CompoundProtocol.tla`**

Constants:
- `Procs`: set of all process identifiers (atomic + compound + spawned)
- `Channels`: set of channel identifiers
- `AtomicProcs`, `CompoundProcs`: partition of `Procs` by signature type
- `TokensPerProc`: function from processes to natural numbers
- `PrimaryChan`: function from processes to channels (`s₁`-channel or
  only channel)
- `SecondaryChan`: function from compound processes to channels
  (`s₂`-channel)
- `CompoundChan`: function from compound processes to channels (combined
  `s₁ & s₂` channel)
- `SpawnedProcs`: function from processes to subsets of processes
  (models recursive eval)
- `CostPerGate`: natural number (cost per fuel-gate firing)

Variables:
- `tokens`: function from channels to natural numbers (pending token
  messages)
- `outerGateOpen`: function from processes to booleans
- `innerGateOpen`: function from compound processes to booleans
- `splitDone`: function from compound processes to booleans
- `commDone`: function from processes to booleans
- `spawned`: function from processes to booleans (activated by parent's
  COMM body)
- `totalCost`: natural number

Actions:
- `SplitFires(p)`: Split mediator for compound process `p` fires on
  `CompoundChan[p]`, consuming 1 combined token and producing 1 token
  each on `PrimaryChan[p]` and `SecondaryChan[p]`. Zero cost
  (infrastructure).
- `OuterGateFires(p)`: Outer (or only) fuel gate fires on
  `PrimaryChan[p]`. Costs `CostPerGate`. Requires Split done for
  compound processes.
- `InnerGateFires(p)`: Inner fuel gate for compound process `p` fires on
  `SecondaryChan[p]`. Costs `CostPerGate`. Requires outer gate open.
- `InnerCommFires(p)`: Inner COMM fires (requires all gates open).
  Spawns child processes. Zero additional cost.

**`MCCompound.tla`** (model-checking instance for `CompoundProtocol`):

Concrete values: 2 atomic processes (`a1`, `a2`), 1 compound process
(`c1`), 1 spawned child (`child1`). 6 channels. Each process gets 1
token. Process `c1` spawns `child1` on COMM completion (recursive eval).

**`FullProtocol.tla`**

Constants:
- `Procs`: set of all process identifiers (atomic + compound +
  doubly-compound + join sources + join mediator)
- `Channels`: set of channel identifiers (12 in the test instance)
- `NestingDepth`: function from processes to natural numbers
  (0 = atomic, 1 = compound, 2 = doubly-compound)
- `GateChans`: function from processes to sequences of channels (one per
  gate layer; length = `NestingDepth[p] + 1`)
- `SplitIn`, `SplitPrimOut`, `SplitSecOut`: functions defining the
  cascading Split wiring (input channel, primary output, secondary
  output for each Split level)
- `JoinProcs`, `JoinPrimIn`, `JoinSecIn`, `JoinOut`: sets/functions
  defining Join mediator wiring
- `ExpectedTerminalCost`: expected total cost at termination (accounts
  for shared-channel contention where not all processes can fire)
- `CostPerGate`: cost per fuel-gate firing

Variables:
- `tokens`: function from channels to natural numbers (pending token
  messages)
- `gateOpen`: function from processes to sequences of booleans (one per
  gate layer)
- `splitDone`: function from compound processes to sequences of booleans
  (one per Split level)
- `commDone`: function from processes to booleans
- `spawned`: function from processes to booleans
- `joinDone`: function from join mediators to booleans
- `totalCost`: natural number (running total)
- `totalJoinsFired`: natural number (for conservation accounting)

Actions:
- `SplitFires(p, i)`: Level-`i` Split for process `p` fires. Consumes 1
  token from `SplitIn[p][i]`, produces 1 each on `SplitPrimOut[p][i]`
  and `SplitSecOut[p][i]`. Cascading: level `i` requires level `i−1` to
  have fired first. Zero cost (infrastructure).
- `GateFires(p, j)`: Layer-`j` gate for process `p` fires on
  `GateChans[p][j]`. Costs `CostPerGate`. Requires all prerequisite
  Splits and prior gates to have fired.
- `InnerCommFires(p)`: Inner COMM for process `p`. Requires all gates
  open. Zero additional cost.
- `JoinFires(jm)`: Join mediator `jm` fires. Consumes 1 token each from
  `JoinPrimIn[jm]` and `JoinSecIn[jm]`, produces 1 on `JoinOut[jm]`.
  Zero cost (infrastructure, inverse of Split).

**`MCFull.tla`** (model-checking instance for `FullProtocol`):

Concrete values: 7 processes — 2 atomic sharing channel `ch_s`
(`a1`, `a2`), 1 compound depth-1 (`c1`), 1 doubly-compound depth-2
(`d1`), 2 join fuel sources (`js1`, `js2`), 1 join mediator (`jm`). 12
channels. The join mediator combines tokens from `js1` and `js2` into a
compound token that feeds another process's gate. Tests all features
simultaneously: shared channels, cascading Splits, Join mediators,
depth-0/1/2 nesting.

### 10.3 Key Invariants

The following invariants are checked by TLC across all reachable states:

**`CostAccountedRho.tla` invariants:**

| Invariant           | Definition                                                                                        | Meaning                                                                                                          |
|---------------------|---------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------|
| `TypeOK`            | All variables have expected types                                                                 | Type safety                                                                                                      |
| `TokenConservation` | `TokensInSystem = InitialTotal` where `TokensInSystem = SUM(pendingTokens) + totalConsumed`       | Total tokens in system (pending + consumed) equals initial allocation. Tokens are neither created nor destroyed. |
| `NoNegativeFuel`    | `∀ ch ∈ Channels: pendingTokens[ch] ≥ 0`                                                          | No channel ever has negative pending tokens (structural invariant).                                              |
| `FuelGateSafety`    | `∀ p ∈ Processes: commDone[p] ⟹ gateOpen[p]`                                                      | A process can only fire its inner COMM if its fuel gate has opened. No computation without fuel.                 |
| `CostMonotone`      | `totalConsumed' ≥ totalConsumed`                                                                  | Cost never decreases.                                                                                            |
| `CostDeterminism`   | `IsTerminal ⟹ totalConsumed = ExpectedCost` where `ExpectedCost = SUM(min(1, InitialTokens[p]))` | At termination, the total cost is the expected value regardless of scheduling order.                            |

**`CostAccountedRho.tla` temporal properties:**

| Property      | Definition                                          | Meaning                                                            |
|---------------|-----------------------------------------------------|--------------------------------------------------------------------|
| `AllComplete` | `◇(∀ p: InitialTokens[p] > 0 ⟹ commDone[p])`        | Every process with available fuel eventually completes (liveness). |

**`FullProtocol.tla` invariants** (all properties from
`CompoundProtocol`, generalized):

| Invariant           | Definition                                                                           | Meaning                                                                                                                          |
|---------------------|--------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------|
| `TypeOK`            | All variables have expected types                                                    | Type safety                                                                                                                      |
| `TokenConservation` | `TotalPending + totalCost − TotalSplitsFired + totalJoinsFired = InitialTotal`       | Accounts for both Splits (+1 net token each) and Joins (−1 net token each). Tokens are conserved modulo mediator redistribution. |
| `NoNegativeTokens`  | `∀ ch ∈ Channels: tokens[ch] ≥ 0`                                                    | No channel ever has negative tokens.                                                                                             |
| `FuelGateSafety`    | `∀ p ∈ Procs: commDone[p] ⟹ ∀ j ∈ GateLayers(p): gateOpen[p][j]`                     | A process completes its inner COMM only if ALL of its gate layers have fired.                                                    |
| `GateOrdering`      | `∀ p, j: gateOpen[p][j] ⟹ (j = 1 ∨ gateOpen[p][j−1]) ∧ (∀ i ≤ j−1: splitDone[p][i])` | Gates fire in strict layer order and only after prerequisite Splits.                                                             |
| `SplitOrdering`     | `∀ p, i: splitDone[p][i] ⟹ (i = 1 ∨ splitDone[p][i−1])`                              | Cascading Splits fire in order (level 1 before level 2, etc.).                                                                   |
| `CostDeterminism`   | `IsTerminal ⟹ totalCost = ExpectedTerminalCost`                                      | In every terminal state, cost equals the expected value regardless of scheduling.                                                |

**`FullProtocol.tla` temporal properties:**

| Property      | Definition                                                        | Meaning                                               |
|---------------|-------------------------------------------------------------------|-------------------------------------------------------|
| `AllComplete` | `◇(∀ p ∈ Procs: spawned[p] ∧ TokensPerProc[p] > 0 ⟹ commDone[p])` | Every spawned process with fuel eventually completes. |

**`EvalScheduling.tla` invariants:**

| Invariant                       | Definition                                                  | Meaning                                                                 |
|---------------------------------|-------------------------------------------------------------|-------------------------------------------------------------------------|
| `TypeOK`                        | All variables have expected types                           | Type safety                                                             |
| `InternalizedCostDeterministic` | `AllDone ⟹ totalCost = Cardinality(Bodies) · CostPerToken`  | At termination, internalized cost is exactly `|Bodies| · CostPerToken`. |
| `InternalizedCostBounded`       | `totalCost ≤ Cardinality(Bodies) · CostPerToken`            | Internalized cost never exceeds the maximum.                            |

**`EvalScheduling.tla` temporal properties:**

| Property            | Definition              | Meaning                                    |
|---------------------|-------------------------|--------------------------------------------|
| `AllEventuallyDone` | `◇(executed = Bodies)`  | Every body eventually executes (liveness). |

### 10.4 Model Checking Results

**`CostAccountedRho.tla` via `MC.tla`:**

| Metric                      | Value                                                                                |
|-----------------------------|--------------------------------------------------------------------------------------|
| Total states found          | 139                                                                                  |
| Distinct states             | 79                                                                                   |
| Invariants checked          | `TypeOK`, `TokenConservation`, `NoNegativeFuel`, `FuelGateSafety`, `CostDeterminism` |
| Temporal properties checked | `AllComplete`                                                                        |
| Violations found            | **0**                                                                                |
| Deadlocks found             | **0**                                                                                |

**`EvalScheduling.tla` via `MCEval.tla`:**

| Metric                      | Value                                                                |
|-----------------------------|----------------------------------------------------------------------|
| Total states found          | 16                                                                   |
| Distinct states             | 16                                                                   |
| Invariants checked          | `TypeOK`, `InternalizedCostDeterministic`, `InternalizedCostBounded` |
| Temporal properties checked | `AllEventuallyDone`                                                  |
| Violations found            | **0**                                                                |
| Deadlocks found             | **0**                                                                |

**`CompoundProtocol.tla` via `MCCompound.tla`:**

| Metric                      | Value                                                                                                                                                    |
|-----------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------|
| Total states found          | 139                                                                                                                                                      |
| Distinct states             | 63                                                                                                                                                       |
| Search depth                | 11                                                                                                                                                       |
| Configuration               | 2 atomic + 1 compound process + 1 spawned child                                                                                                          |
| Features tested             | Split mediator, nested two-layer gates, recursive eval dispatch                                                                                          |
| Invariants checked          | `TypeOK`, `TokenConservation` (with Split redistribution), `NoNegativeTokens`, `FuelGateSafety`, `SplitOrdering`, `InnerGateOrdering`, `CostDeterminism` |
| Temporal properties checked | `AllSpawnedComplete`                                                                                                                                     |
| Violations found            | **0**                                                                                                                                                    |

This model covers the full compound-signature protocol: the Split
mediator fires on the combined channel (1 token in, 2 tokens out), the
outer gate fires on the `s₁`-channel, the inner gate fires on the
`s₂`-channel, and the inner COMM fires. It also models recursive eval:
process `c1`'s COMM body spawns child process `child1` (an atomic
process on its own channel), which then acquires its own fuel and fires
its own COMM. All interleavings of all actions across all 4 processes
are explored, and the terminal cost is verified to be scheduling-
independent.

**`FullProtocol.tla` via `MCFull.tla`:**

| Metric                      | Value                                                                                                                                                        |
|-----------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Total states generated      | 67,609                                                                                                                                                       |
| Distinct states             | 12,960                                                                                                                                                       |
| Search depth                | 22                                                                                                                                                           |
| Configuration               | 7 processes (2 atomic sharing 1 channel, 1 compound depth-1, 1 doubly-compound depth-2, 2 join sources, 1 join mediator), 12 channels                        |
| Features tested             | Shared channels, cascading Splits (depth 1 and 2), Join mediator, arbitrary gate nesting (0/1/2 layers), recursive token flow (Join output feeds gate input) |
| Invariants checked          | `TypeOK`, `TokenConservation` (with Split/Join accounting), `NoNegativeTokens`, `FuelGateSafety`, `GateOrdering`, `SplitOrdering`, `CostDeterminism`         |
| Temporal properties checked | `AllComplete`                                                                                                                                                |
| Violations found            | **0**                                                                                                                                                        |

This is the most comprehensive model. It tests every feature of the
cost-accounted rho calculus protocol simultaneously: shared-channel
contention (processes `a1` and `a2` compete for tokens on the same
channel `ch_s`), cascading Splits (process `d1` at depth 2 requires 2
Splits and 3 gate layers), Join mediators (process `jm` combines tokens
from `js1` and `js2`), and the token conservation invariant accounts for
both Split redistribution (+1 net) and Join recombination (−1 net). All
67,609 states across all possible interleavings satisfy every invariant.

The `EvalScheduling` model also tracks `extCost` (the externalized
model's cost) for observational comparison. In terminal states,
`extCost` varies depending on the execution-order trace `orderSoFar`,
confirming the order-dependence of the externalized model.
Specifically:

- When `b1` executes first: `extCost = StorageCostA + 2 · StorageCostB = 10 + 30 = 40`
- When `b2` executes first: `extCost = StorageCostA + 2 · StorageCostB = 10 + 30 = 40`
- The internalized `totalCost = 3 · 1 = 3` in all terminal states.

In this simplified model with a single shared channel, the externalized
cost happens to be the same in all terminal states because all bodies
interact with the same channel in the same pattern (first touch pays
`StorageCostA`, subsequent touches pay `StorageCostB`). The divergence
manifests in more complex multi-channel scenarios modeled in the Rocq
formalization, where produces and consumes arrive on different channels
with different data sizes.

### 10.5 Rocq ↔ TLA+ Correspondence

Every property checked by TLC has a universally-quantified counterpart
in the Rocq development. The correspondence is maintained by construction:
a TLA+ invariant names a Rocq theorem, and the two evolve together.

| Property                         | Rocq Theorem                             | TLA+ Invariant                  |
|----------------------------------|------------------------------------------|---------------------------------|
| Token conservation (single step) | `token_monotone_step`                    | `TokenConservation`             |
| Token conservation (multi-step)  | `token_monotone_reachable`               | `TokenConservation`             |
| Cost determinism                 | `ca_cost_deterministic`                  | `CostDeterminism`               |
| Full confluence                  | `ca_confluent`                           | (implied by exhaustive search)  |
| Strong normalization             | `ca_strongly_normalizing`                | (implied by finite state space) |
| Fuel-gate safety                 | `fuel_gate_stuck_isolated`               | `FuelGateSafety`                |
| Cost monotonicity                | `token_strictly_decreases`               | `CostMonotone`                  |
| No negative fuel                 | (structural: `token_size ≥ 0`)           | `NoNegativeFuel`                |
| Liveness                         | (not directly modeled)                   | `AllComplete`                   |
| Channel separation               | `fuel_gate_channel_subst_invariant`      | (not modeled)                   |
| Internalized cost deterministic  | (follows from conservation)              | `InternalizedCostDeterministic` |
| Fuel event multiset determinism  | `fuel_events_consumed_perm`              | (not directly modeled)          |
| Step determinism (single-token)  | `ca_step_deterministic`                  | (not modeled)                   |
| Single-token path uniqueness     | `single_token_path_unique`               | (not modeled)                   |
| Bitmask mergeable diff/merge     | `bitmask_diff_merge_round_trip`          | `BitmaskDiffMergeRoundTrip`     |
| IntegerAdd mergeable diff/merge  | `integer_add_diff_merge_round_trip`      | `IntegerAddDiffMergeRoundTrip`  |
| Mergeable cost-boundary isolation | `mergeable_channel_accounting_preserves_user_budget` | `MergeableAccountingPreservesUserCost` |

### 10.6 What TLA+ Proves and Does Not Prove

**What TLA+ proves** (by exhaustive state-space exploration):

For any finite configuration of processes with any mix of atomic
signatures, compound signatures (up to depth 2), shared channels, Split
mediators, Join mediators, and recursive eval — across *every possible
scheduling order* of COMM events — the total phlogiston cost at
termination is identical. Specifically:

- **Cost determinism**: The terminal `totalCost` is a function of the
  initial configuration alone. It does not depend on which process fires
  first, which Split fires before which gate, which of two competing
  processes wins a shared token, or in what order recursive children are
  spawned and fueled. TLC verified this across all 12,960 distinct
  states of the most complex model (7 processes, 12 channels, depth-2
  nesting, Join mediators).

- **Token conservation**: Fuel is never created. Every gate firing
  consumes exactly one token. Splits redistribute (1 → 2) and Joins
  recombine (2 → 1), but the accounting identity
  `pending + consumed − splits + joins = initial` holds in every
  reachable state.

- **Fuel-gate safety**: No process can execute its application-level
  COMM without first consuming fuel through all of its gate layers.
  This is the capability-security guarantee that makes cost accounting
  enforceable.

- **Liveness**: Every process with available fuel eventually completes
  (under fair scheduling). No deadlocks arise from the fuel-gate
  protocol.

**What TLA+ does NOT prove**:

- **Arbitrary system sizes**: TLC checks finite instances exhaustively
  (up to 7 processes, 12 channels, depth 2). It does not prove the
  properties for systems of arbitrary size or arbitrary nesting depth.
  The Rocq formalization provides this generality — Theorem 9.1
  (`token_monotone_step`) and Theorem 9.2 (`token_monotone_reachable`)
  are proven universally for all systems, all signatures, and all token
  allocations.

- **Liveness under unfair scheduling**: The liveness properties assume
  weak fairness (every continuously enabled action eventually fires).
  Under adversarial scheduling (e.g., a validator intentionally
  starving a process), liveness is not guaranteed — but cost determinism
  still holds.

- **Application-level semantics**: The TLA+ model abstracts COMM bodies
  as atomic "done" flags. It does not model the content of COMM bodies
  (the substituted Rholang program), data flow, or application-level
  correctness. These are covered by the contextual reachability and
  bisimulation theorems in the Rocq formalization (Sections 6.1–6.3,
  9.3–9.5).

The Rocq proofs and TLA+ models are complementary: Rocq proves the
properties universally (for all systems of any size), while TLA+
exhaustively checks every interleaving for concrete finite instances —
catching specification bugs that a proof might miss (e.g., off-by-one
errors in the conservation accounting, incorrect preconditions on
actions, or unexpected deadlocks in the Split/Join/gate interaction).

### 10.7 Validator Behavioral Contract (Multi-Prover)

The built-in validator is named by a **behavioral contract** (DR-12): the
obligation set a Cost-Accounted-Rho validator must satisfy, with each
obligation discharged in all three provers — TLA+, Rocq, and Lean. The
contract subtrees **re-export** already-proven, kernel-checked obligations
under contract handles; they do not re-prove. Of the arithmetic
obligations, the validator's funding/zeroing/order-independence clauses are
discharged **deductively by TLAPS** in `formal/tlaplus/validator/Validator.tla`
(five TLAPS-proven theorems), while the state-machine obligations stay
TLC-checked under `RuntimeBudgetReplay.tla` (§10.5).

The contract has four **spec** obligations (S1–S4) and three **platform**
obligations (P1–P3, labeled out-of-spec per DR-12). Each is proven in
Lean and Rocq, and named by a `validator_contract_*` clause in
`formal/rocq/validator/theories/Contract.v` /
`formal/lean/Validator/Contract.lean`:

| ID | Obligation                              | Spec / basis | Rocq obligation (re-exported)                                   | TLA+ / Lean |
|----|-----------------------------------------|--------------|-----------------------------------------------------------------|-------------|
| S1 | Token-presence syntactic validity       | §6.3         | `FuelGateSafety.fuel_gate_rejects_mismatched_token`             | TLA+ token-present rewrite; Lean E3 |
| S2 | Acceptance correctness (`Σ_s ≥ Δ_s`, pre-exec) | §7.6  | `LinearLogicResources.funding_decidable`                        | `admission_decision_schedule_independent`; Lean E2 |
| S3 | Linear no-double-spend / reject-both     | §7.7         | `LinearLogicResources.ll_no_double_spend_single_witness`        | committed-prefix; Lean E2 |
| S4 | For-comprehension = atomic funded txn    | §7.1         | `StepDeterminism.ca_step_deterministic`, `core_token_demand`    | single-COMM fire; Lean E3 |
| P1 | Slash-authorization soundness            | DR-12        | `MainTheorem.main_T9_12_stale_evidence_not_authorized`          | `AuthorizedSlashFlow`; Lean E4 |
| P2 | Finalization safety                      | DR-12        | `MainTheorem.main_T10_fork_choice_exclusion`                    | `EquivocationDetector`; Lean E6.5 |
| P3 | Determinism / replay-equivalence         | DR-12        | `StepDeterminism.ca_step_deterministic`                         | `ConsumedAndVerdictScheduleIndependent`; Lean E4 |

S1–S4 originate in the CostAccountedRho development; P1/P2 originate in the
Slashing development. The S2/S3 acceptance obligations (`Σ_s ≥ Δ_s`,
reject-both on the first under-funded deploy) are the proof side of the
per-signature static linear-proof acceptance gate decided at block
assembly (**DR-11**). Every clause inherits "Closed under the global
context" from its single underlying term, so `Print Assumptions
validator_contract_X` reports exactly what the obligation reports
(axiom-free for all seven); the proof gate (§12) re-queries each clause's
assumptions. A custom validator re-discharges S1–S4 + P3 for its own
admission/decision functions and inherits P1/P2 from the fixed Rust
platform shell. The full obligation set, prover assignments, and
custom-validator seam are documented in
[`cost-accounting-impl/workstream-e-validator-contract.md`](cost-accounting-impl/workstream-e-validator-contract.md);
the worked reference artifacts are
`formal/rocq/validator/theories/Contract.v`,
`formal/lean/Validator/Contract.lean`, and
`formal/tlaplus/validator/Validator.tla`.

---

## 11. Module Reference

Sections 11 and 12 provide implementation-level traceability (files,
line-level anchors, paper-to-code correspondence) and the trust base
(hypotheses, kernel, stdlib usage) for the development. Section 13 lists
references.

### 11.1 File Listing

| Module                      | Lines      | Theorems | Purpose                                                                                                                                                                                                                                                                            |
|-----------------------------|------------|----------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `RhoSyntax.v`               | 855        | 31       | Syntax (incl. `PReplicate`), substitution, structural equivalence, lifting                                                                                                                                                                                                         |
| `StructEquivInversion.v`    | 253        | 7        | Head count, count_inputs, count_outputs, count_derefs, count_replicates                                                                                                                                                                                                            |
| `StructEquivHeads.v`        | 1,470      | 45       | Heads lists, permutation characterization, PInput/POutput/PReplicate injectivity (`only_input`/`only_output`/`only_replicate` family), `list_equiv_app_inv`, `list_equiv_in_transport`, `head_count_zero_se_nil` (Section 8.7)                                                       |
| `RhoReduction.v`            | 442        | 17       | Operational semantics (incl. `rs_replicate`), conflated `barb` + split `input_barb`/`output_barb`/`barb_iff_input_or_output` (§3.6), stuck lemmas                                                                                                                                   |
| `CostAccountedSyntax.v`     | 231        | 4        | Signatures, tokens, systems, size functions                                                                                                                                                                                                                                        |
| `CostAccountedReduction.v`  | 283        | 5        | Five cost-accounted rules, reachability                                                                                                                                                                                                                                            |
| `Translation.v`             | 580        | 12       | N⟦·⟧, T⟦·⟧, P⟦·⟧, S⟦·⟧, Split, Join, PersistentSplit, PersistentJoin                                                                                                                                                                                                              |
| `ChannelSeparation.v`       | 219        | 7        | Signature-channel invariance under subst/lift; `N_tr_is_Quote`                                                                                                                                                                                                                     |
| `TokenConservation.v`       | 234        | 9        | Fuel monotonicity (per-step and multi-step)                                                                                                                                                                                                                                        |
| `Settlement.v`              | 140        | 8        | Post-evaluation fee settlement, escrow/refund arithmetic, and no mid-evaluation refund fuel                                                                                                                                                                                        |
| `SlashingComposition.v`     | 570        | 30       | Composition boundary with the slashing protocol: cost-invalid evidence is observational for user cost, recovered rejected slashes require current evidence, parent pre-state authorization gates slash effects, and slash system effects preserve deploy fuel, settlement inputs, and settlement arithmetic |
| `MergeableChannelAccounting.v` | 274     | 14       | Typed mergeable-channel accounting: `IntegerAdd` additive round trip, `BitmaskOr` diff/merge round trip, set-like OR folding, merge-type preservation, non-numeric fallback classification, and cost-boundary isolation |
| `RuntimeBudgetRefinement.v` | 2,084      | 86       | Bounded-memory runtime-budget refinement: consumed/remaining conservation, successful weighted reservation, batched reservations, out-of-phlo boundary commitment, reset-from-token trace clearing, finalization-read cost traces, post-activation trace evidence, zero-event commitments, block/cache authentication, canonical replay-trace equivalence, slash target activation epoch authentication, and replay-payload field sensitivity |
| `UseCaseAdequacy.v`         | 1,985      | 88       | Proof-backed UC-CA traceability theorems over token conservation, unit-token expansion, settlement, slashing composition, recovered slash current-evidence authorization, typed mergeable channels, recursive reflection, runtime-budget refinement, finalization-read trace digests, replay payload equivalence, post-activation cost-trace requirements, block/cache authentication, zero-event commitments, and failed/control-path trace boundaries |
| `FuelEventDecomposition.v`  | 239        | 6        | Fuel event multiset determinism                                                                                                                                                                                                                                                    |
| `StrongNormalization.v`     | 130        | 5        | Well-foundedness of `ca_step`; `ca_strongly_normalizing`                                                                                                                                                                                                                           |
| `Confluence.v`              | 483        | 14       | Per-rule determinism, Newman's lemma, full confluence, cost determinism                                                                                                                                                                                                            |
| `StepDeterminism.v`         | 291        | 5        | Single-token determinism; unique reduction path length                                                                                                                                                                                                                             |
| `TranslationFaithfulness.v` | 4,183      | 84       | Contextual forward reachability, fuel-bound soundness, phase-based gate reflection, recursive whole-system backward reflection, per-step reverse, generic dispatcher                                                                                                                |
| `FuelGateSafety.v`          | 357        | 6        | Fuel-gate capability security                                                                                                                                                                                                                                                      |
| `Bisimulation.v`            | 1,248      | 36       | Coinductive bisim, multi-stuck bisim, generic bisim dispatcher                                                                                                                                                                                                                     |
| `WeakBarbedEquiv.v`         | 259        | 17       | Weak barb predicates (`weak_barb_input`, `weak_barb_output`), reachability/≡ₙ-closure, `weak_barbed_equiv_except` hidden-channel equivalence, parallel-congruence lemmas (§6.5, §6.6)                                                                                               |
| `Replication.v`             | 2,071      | 56       | Meredith's reflective encoding (`bang_encoding`, `D_encoding`); `bang_encoding_unfolds` (§6.5 Theorem 9.19); forward barb propagation `preplicate_bang_encoding_body_barbs_sound` (§6.5 Theorem 9.20); step inversion `step_PReplicate_inv_se`, `step_PPar_PReplicate_inv_se` (§8.7 Lemma 9.21); closed forward-boundary theorem `replication_encoding_forward_barb_sound` (§6.6 Theorem 9.23) |
| `MintingInjection.v`        | 630        | 26       | Minting is exogenous token injection, never a `ca_step`; supply-pool balance layer (`Σ⟦v⟧` injectivity in pk, epoch-mint idempotency, user steps never move a supply balance — the same pubkey keying §D2.9 extends to user-deploy funding, §12(iv)) |
| `MintingHalt.v`             | 179        | 8        | A halted (slashed) validator is never minted and never gains supply; redemption is the only path back to funding (`halted_validator_not_minted`, `halted_validator_supply_not_increased`) |
| `Exchange.v`                | 203        | 7        | The blessed conserving 1:1 token Exchange (Stage D): per-channel and total token conservation of the swap, requires-both-inputs join, and Exchange-is-a-`ca_step`-not-a-mint |
| `SystemStructEquiv.v`       | 474        | 14       | System-level structural equivalence (`sys_equiv`): parallel-unit law `sse_par_unit`, Appendix-B token-stack decomposition `token_decomp`, and source-level free names `sig_free_names` (Def 3.3 axes; §3.5) |
| `SyntacticSugar.v`          | 196        | 6        | Section 3.8 syntactic sugar at the translation level: uniform-signing and linear-transfer (⊸) defining equations as `proc`-level structural equivalences of the translated images (Option A; ⊸ desugars to nested plain-signature gate layers) |
| `WalletNaming.v`            | 313        | 14       | Per-validator wallet `@W_v` naming (Stage A): injectivity in the validator public key and pairwise disjointness of the wallet / quarantine / funding-slot seed domains — the pubkey-keyed `@W_v`/`SGround` the §D2.9 user-deploy funding key `Sig::Ground(pk)` instantiates (§12(iv)) |
| `MultiSignerRefinement.v`   | 530        | 31       | Phase 1.10 multi-signature deploy support: per-deployer Map-in-MVar PoS refinement, single-signer observable equivalence to the legacy contract, and canonical-order FIFO refund-drain conservation |
| `LinearLogicResources.v`    | 979        | 45       | Publication-derived linear-resource calculus: mixed unrestricted/linear resource boundary, anti-contraction / anti-weakening, no-double-spend, funding decidability, the runtime `sig_algebra` bridge, and the **cross-group cumulative-demand bound** (`cross_group_draw_le_supply`, `cross_group_admission_sound` — TM-CA-165, the live-ledger generalization of `competing_funding_at_most_one_succeeds`/`admitted_prefix_fits`) |
| `LLIdentities.v`            | 587        | 51       | Phase 2/3 ILLE algebraic identities: multiplicative (tensor/and), additive (plus/with), and exponential (bang/why-not) laws plus Phase 2 Threshold permutation invariance at the reflection layer |
| **Total**                   | **25,776** | **967**  |                                                                                                                                                                                                                                                                                    |

Theorem counts are `Qed.` + `Defined.` occurrences (the proofs that
contribute kernel-checked terms). Earlier totals listed in this table
used a looser metric that also counted intermediate `Lemma` bodies
inside sections, which differs from the kernel-verified count by a few
per large module.

> **Linear-logic layer.** The compound-signature *authorization* algebra — the
> `sig_algebra` extension to `CostAccountedSyntax.v`, the DILL two-zone fragment
> in `LinearLogicResources.v`, and the channel-layer identities in `LLIdentities.v`
> (the multiplicative unit `1`; tensor/with/plus/bang/why-not/lollipop; and the
> no-double-spend / no-free-weakening guarantees) — is documented in its dedicated
> companion, [*The Linear Logic of Compound Signatures*](cost-accounting-linear-logic.md).

### 11.2 Paper-to-Code Traceability

| Paper Section      | Paper Definition              | Rocq Definition                        | File:Line                        |
|--------------------|-------------------------------|----------------------------------------|----------------------------------|
| 2.1 Syntax         | `P`, `Q`, `x`, `y`            | `proc`, `name`                         | `RhoSyntax.v:57`                 |
| 2.3 Struct. equiv. | `≡_S`                         | `struct_equiv`                         | `RhoSyntax.v:719`                |
| 2.4 COMM rule      | `for(y←x)P ∣ x!(Q) ⇝ P{@Q/y}` | `rs_comm`                              | `RhoReduction.v:72`              |
| 2.4 PAR rule       | `P⇝P'` / `P∣Q⇝P'∣Q`           | `rs_par_l`, `rs_par_r`                 | `RhoReduction.v:78`              |
| 2.4 STRUCT rule    | `P≡P'  P'⇝Q'  Q'≡Q` / `P⇝Q`   | `rs_struct`                            | `RhoReduction.v:90`              |
| Def 3.3 Signatures | `s`                           | `sig`                                  | `CostAccountedSyntax.v:76`       |
| Def 3.2 Tokens     | `T`                           | `token`                                | `CostAccountedSyntax.v:96`       |
| 3.1 Systems        | `S`                           | `system`                               | `CostAccountedSyntax.v:118`      |
| 3.6 Five rules     | Rules 1–5                     | `ca_step`                              | `CostAccountedReduction.v:83`    |
| App. A `N⟦·⟧`      | Signatures to names           | `N_tr`                                 | `Translation.v:122`              |
| App. A `K⟦·⟧`      | Token-stack translation (repo `T_tr` = paper `K⟦·⟧`) | `T_tr`            | `Translation.v:143`              |
| App. A `P⟦·⟧`      | Signed processes              | `P_tr`                                 | `Translation.v:191`              |
| App. A `S⟦·⟧`      | System translation            | `S_tr`                                 | `Translation.v:220`              |
| App. A Split       | Splitter mediator (Split/Join infrastructure) | `Split`                | `Translation.v:263`              |
| App. A Join        | Joiner mediator (Split/Join infrastructure)   | `Join`                 | `Translation.v:272`              |
| §4–§5 Verification | Contextual forward reachability | `translation_faithful` / `translation_contextual_reachability` | `TranslationFaithfulness.v:2308` |
| §4–§5 Bisimulation | Behavioral equivalence        | `bisim`                                | `Bisimulation.v:433`             |
| —                  | Generic bisim                 | `translation_strong_bisimilar_generic` | `Bisimulation.v:1246`            |
| —                  | Generic per-step reverse      | `gate_per_step_reverse_generic`        | `TranslationFaithfulness.v:3888` |
| —                  | Phase-based gate reflection   | `backward_reflection_phased_gate`      | `TranslationFaithfulness.v:4022` |
| —                  | Recursive whole-system reflection | `well_reflected_backward_reflection` | `TranslationFaithfulness.v:4147` |
| —                  | Source billing witness        | `billed_step`, `ca_step_billed`        | `TranslationFaithfulness.v:2648` |
| —                  | Token conservation            | `token_monotone_reachable`             | `TokenConservation.v:98`         |
| —                  | Token strict decrease         | `token_strictly_decreases`             | `TokenConservation.v:226`        |
| —                  | Fuel event multiset det.      | `fuel_events_consumed_perm`            | `FuelEventDecomposition.v:198`   |
| —                  | Reduction-length bound        | `ca_max_steps_bound`                   | `StrongNormalization.v:111`      |
| —                  | Strong normalization          | `ca_strongly_normalizing`              | `StrongNormalization.v:95`       |
| —                  | Local confluence (diamond)    | `ca_local_confluence`                  | `Confluence.v:269`               |
| —                  | Newman's lemma (constructive) | `newman`                               | `Confluence.v:364`               |
| —                  | Full confluence of `ca_step`  | `ca_confluent`                         | `Confluence.v:432`               |
| —                  | Normal-form uniqueness        | `ca_normal_form_unique`                | `Confluence.v:449`               |
| —                  | Cost determinism              | `ca_cost_deterministic`                | `Confluence.v:474`               |
| —                  | Step determinism (single-tok) | `ca_step_deterministic`                | `StepDeterminism.v:156`          |
| —                  | Single-token path uniqueness  | `single_token_path_unique`             | `StepDeterminism.v:249`          |
| MR 2005 §3         | Reflective D-encoding         | `D_encoding`                           | `Replication.v:66`               |
| MR 2005 §3         | Reflective bang-encoding      | `bang_encoding`                        | `Replication.v:73`               |
| MR 2005 §3         | One-step operational unfold   | `bang_encoding_unfolds` (Thm 9.19)     | `Replication.v:222`              |
| §3.6 (this doc)    | Split input observable        | `input_barb`                           | `RhoReduction.v:378`             |
| §3.6 (this doc)    | Split output observable       | `output_barb`                          | `RhoReduction.v:384`             |
| §3.6 (this doc)    | Conflated ↔ split barbs       | `barb_iff_input_or_output`             | `RhoReduction.v:391`             |
| §6.5 (this doc)    | Weak input observable         | `weak_barb_input`                      | `WeakBarbedEquiv.v:53`           |
| §6.5 (this doc)    | Weak output observable        | `weak_barb_output`                     | `WeakBarbedEquiv.v:56`           |
| §6.6 (this doc)    | Weak barbed equiv. mod x      | `weak_barbed_equiv_except`             | `WeakBarbedEquiv.v:~165`         |
| §6.5 (this doc)    | Forward barb propagation      | `preplicate_bang_encoding_body_barbs_sound` (Thm 9.20) | `Replication.v:1448` |
| §8.7 (this doc)    | Sole-replicate predicate      | `only_replicate`                       | `StructEquivHeads.v:~1299`       |
| §8.7 (this doc)    | PReplicate injectivity mod ≡  | `se_PReplicate_inj`                    | `StructEquivHeads.v:~1426`       |
| §8.7 (this doc)    | PReplicate head locator       | `se_par_preplicate_locate`             | `Replication.v:~1659`            |
| §8.7 (this doc)    | Step inv. (bare PReplicate)   | `step_PReplicate_inv_se`               | `Replication.v` Section 13       |
| §8.7 (this doc)    | Step inv. (PReplicate + rest) | `step_PPar_PReplicate_inv_se` (Lem 9.21) | `Replication.v` Section 14.C   |
| §6.6 (this doc)    | Closed forward replication boundary | `replication_encoding_forward_barb_sound` (Thm 9.23) | `Replication.v:2063`   |
| post-merge implementation | `BitmaskOr` typed mergeable diff/merge | `bitmask_diff_merge_round_trip` | `MergeableChannelAccounting.v:147` |
| post-merge implementation | `BitmaskOr` fold order independence | `mergeable_channel_bitmask_fold_permutation` | `MergeableChannelAccounting.v:201` |
| post-merge implementation | `IntegerAdd` diff/merge round trip | `integer_add_diff_merge_round_trip` | `MergeableChannelAccounting.v:168` |
| post-merge implementation | Merge type and non-numeric fallback | `mergeable_channel_delta_preserves_type`, `non_numeric_channel_not_mergeable_payload_match` | `MergeableChannelAccounting.v:222` |

Rows tagged with "—" in the *Paper Section* column are not stated
in [4]. They split into two groups: the determinism/multiset rows
(`ca_step_deterministic`, `single_token_path_unique`,
`fuel_events_consumed_perm`) *verify* properties of the paper's
algorithm; the SN/confluence/cost-determinism rows
(`ca_strongly_normalizing`, `ca_max_steps_bound`,
`ca_local_confluence`, `newman`, `ca_confluent`,
`ca_normal_form_unique`, `ca_cost_deterministic`) are
proof-original extensions. See [Section 1.5](#15-verified-properties-detail)
for the (a)/(b)/(c)/(d) classification. Rows tagged "MR 2005"
(Meredith–Radestock) are the replication-encoding support additions:
the operational unfold, forward weak-barb propagation, and the
step-inversion infrastructure used to define the verification boundary.

### 11.3 Repo-Local Proof Coverage Matrix

This matrix is the implementation-facing status record for this branch.
It deliberately covers proof artifacts in this repository and records the
obligations the staged `f1r3node-rust` implementation must satisfy. The
external paper remains a read-only input for this phase.

> **Exhaustive property index.** This matrix is organized by *proof artifact*. For the
> spec-property-first view — every `CA-P-###` obligation of both governing `.tex` documents
> (plus related publications) with its assertion modality, covering artifact, and
> COVERED/PARTIAL/GAP/DEFERRED/SCOPE-BOUNDARY/EXCEEDS status — see the conformance catalog
> [`cost-accounting-conformance-properties.md`](./cost-accounting-conformance-properties.md).

**Reading §11.3 after TM-CA-151.** Rows that mechanize a cost-trace
digest / event-count / commitment describe a *digest-inclusive
diagnostic-refinement* level (`rb_full_replay_payload` etc.). Per
TM-CA-151 those quantities are diagnostic and were removed from
production consensus; the production consensus surface is `total_cost`
(clamped to `initial` on OOP) + status + post-state hash. The listed
theorems remain valid at the refinement level and are not claims that
the digest is consensus.

| Claim / design obligation | Repo-local artifact | Status |
|---------------------------|---------------------|--------|
| Rules 1-5 are the source cost semantics | `ca_step` in `CostAccountedReduction.v` | Mechanized |
| Every source step strictly consumes source tokens | `token_consumed_per_step`, `token_strictly_decreases` | Mechanized |
| Cost is independent of reduction order | `ca_confluent`, `ca_cost_deterministic` | Mechanized |
| Single-token systems have one successor path | `ca_step_deterministic`, `single_token_path_unique` | Mechanized |
| Translation has a pure-rho realization for every source step | `translation_faithful` / `translation_contextual_reachability` | Mechanized as contextual reachability |
| Generic witness equals the translated target state | Not the statement of `translation_faithful` | Not claimed; superseded by the `well_reflected` implementation target |
| Canonical translated gate steps reflect to a spent source-token phase | `backward_reflection_phased_gate` | Mechanized for one billable gate across all signature shapes |
| Arbitrary whole-system steps reflect to `ca_step` for the recursive metered implementation target | `well_reflected_backward_reflection` | Mechanized |
| Arbitrary whole-system steps reflect to `ca_step` for the legacy compositional `S_tr` image | Not the selected implementation invariant | Remains unclaimed because `P_tr` can spend an outer gate for an inert body |
| Fuel cannot be synthesized in source reductions | `translation_fuel_bound_soundness`, `no_phantom_fuel` | Mechanized for `ca_reachable` |
| Split/Join do not add source cost | Rules 3/5 consume one source token; Rules 2/4 consume two | Mechanized in source calculus; runtime must bill source-token events, not raw translated COMM count |
| Bounded-memory `TokenBudget` coalesces the nested token stack | `RuntimeBudgetRefinement.v`: `rb_total_remaining_conservation`, `rb_successful_weight_refines_unit_count`, `rb_reserve_oop_commits_limit`, `rb_reset_from_token_conservation` | Implemented as `RuntimeBudget` reset from `SignedProcess::metered(..., Token::Count ...)`; tested against finite unit-token expansion, OOP boundary commitment, reset semantics, and canonical event logs |
| Weighted primitive/parser/substitution work is billed consistently | `rb_admitted_success_has_admissible_event`, `rb_zero_weight_admission_rejection_preserves_trace` | Implemented as deterministic positive bounded `BillableTokenEvent` reservations; zero-weight or malformed billable events are rejected before trace or fuel mutation |
| Canonical OOP boundary is schedule-independent | `fuel_events_consumed_perm`, `ca_cost_deterministic` | Mechanized multiset/cost basis; Rust records insufficient-fuel boundaries by canonical source-event descriptor |
| Casper fee settlement uses token cost without reintroducing runtime metering | `refund_le_escrow`, `charged_plus_refund_eq_escrow`, `post_evaluation_settlement_no_mint` | Mechanized as post-evaluation arithmetic in `Settlement.v`; implemented with unmetered system deploys and wire-compatible settlement of `RuntimeBudget.total_cost() * phlo_price` |
| Evaluation cannot receive Casper refund fuel mid-run | `evaluation_cannot_receive_refund_fuel`, `evaluation_step_cannot_mint_fuel` | Mechanized by importing token monotonicity into `Settlement.v`; runtime must not mutate deploy balance or copy a process with a larger remaining budget during evaluation |
| Cost-invalid block evidence does not change user deploy cost | `replay_cost_mismatch_sound_for_evidence`, `cost_invalid_block_evidence_does_not_change_user_cost`, `current_cost_evidence_epoch_sound`, `recovered_rejected_slash_requires_current_cost_evidence` | Mechanized in `SlashingComposition.v`; replay-cost mismatch and related current cost-invalid evidence may feed slashing authorization, but recording the evidence preserves the settlement boundary |
| Typed mergeable channels preserve strategy-specific semantics | `bitmask_diff_merge_round_trip`, `mergeable_channel_bitmask_fold_permutation`, `integer_add_diff_merge_round_trip`, `mergeable_channel_delta_preserves_type`, `non_numeric_channel_not_mergeable_payload_match`, `mergeable_channel_accounting_preserves_user_budget` | Mechanized in `MergeableChannelAccounting.v`; implemented by `MergeType::{IntegerAdd, BitmaskOr}`, `calculate_num_channel_diff`, `combine_mergeable_value`, `fold_multi_value`, and non-numeric fallback to the conflict path |
| Replay-cache fingerprints include replay-relevant event traces | `rb_replay_payload_user_trace_change_detected`, `rb_replay_payload_system_trace_change_detected`, `rb_cost_trace_change_detected`, `rb_full_replay_payload_user_cost_trace_change_detected`, `rb_full_replay_payload_user_cost_trace_event_count_change_detected`, `rb_full_replay_payload_user_cost_trace_present_change_detected`, `rb_full_replay_payload_missing_cost_trace_change_detected`, `rb_replay_cache_key_payload_change_detected`, `rb_trace_entry_deploy_change_detected`, `rb_trace_entry_source_path_change_detected`, `rb_trace_entry_redex_change_detected`, `rb_trace_entry_local_index_change_detected`, `rb_trace_entry_billable_kind_change_detected`, `rb_trace_entry_primitive_descriptor_change_detected`, `rb_trace_entry_weight_change_detected` | Mechanized in `RuntimeBudgetRefinement.v`; implemented by hashing canonicalized user deploy logs, system deploy logs, cost, status, and system deploy data. (Per TM-CA-151 the per-op cost-trace digest/presence/event-count are diagnostic and are NOT hashed into the consensus replay fingerprint; the listed `rb_full_replay_payload_*` lemmas describe a digest-inclusive diagnostic-refinement level.) The abstract trace entry names the concrete Rust digest inputs for that diagnostic level: deploy id, source path, redex id, local index, billable kind, primitive descriptor when the kind is primitive, and weight. |
| Post-activation replay requires cost-trace evidence | `rb_post_activation_cost_trace_commitment_valid`, `rb_empty_cost_trace_commitment_can_be_valid`, `uc_ca_039_post_activation_cost_trace_required`, `uc_ca_046_zero_event_post_activation_trace_commitment` | Mechanized in `RuntimeBudgetRefinement.v` / `UseCaseAdequacy.v`; as a digest-inclusive diagnostic-refinement obligation. Per TM-CA-151 production replay does NOT reject on cost-trace digest presence (consensus = `total_cost` + status + post-state hash); the Rocq model retains "absent commitment ⇒ replay-invalid" and "present zero-event digest is valid" at the refinement level, with legacy non-cost-accounted replay quarantined |
| Block-auth refinement detects cost-trace changes (diagnostic — TM-CA-151) | `rb_block_auth_payload_replay_payload_change_detected`, `uc_ca_047_block_authenticates_cost_trace_payload` | Mechanized in `RuntimeBudgetRefinement.v` / `UseCaseAdequacy.v` at the digest-inclusive diagnostic-refinement level; per TM-CA-151 the per-op cost-trace digest/count are NOT in the signed block-hash preimage — production block authentication covers `total_cost` + status + post-state hash + signature |
| Slashing/refund/replay cross-products authenticate the composed production payload | `slash_system_effect_is_unmetered_for_user_budget`, `slash_after_evaluation_cannot_add_fuel`, `uc_ca_058_refund_cannot_replenish_runtime_fuel`, `post_evaluation_settlement_no_mint`, `rb_replay_cache_key_payload_change_detected`, `rb_full_replay_payload_slash_target_epoch_change_detected` | Mechanized by composing slashing, settlement, and replay-authentication lemmas; implemented by composed Rust hardening tests that mutate user cost trace fields, event logs, slash evidence, target activation epoch, genesis mode, and settlement cost projection in one production-shaped scenario |
| Failed and control-path execution preserve trace boundaries | `rb_oop_trace_survives_boundary`, `rb_oversized_weight_rejection_preserves_trace`, `rb_oversized_source_path_admission_rejection_preserves_trace`, `rb_oversized_primitive_descriptor_admission_rejection_preserves_trace`, `rb_nonbillable_frame_preserves_trace` | Mechanized in `RuntimeBudgetRefinement.v`; implemented by retaining OOP trace evidence across failed-deploy rollback, rejecting oversized weights, source paths, and primitive descriptors before trace mutation, and keeping non-billable control frames out of the (diagnostic) cost trace |
| Slash system deploys preserve user fuel and fee settlement | `slash_preserves_fee_settlement_inputs`, `slash_preserves_settled_amount`, `slash_system_effect_is_unmetered_for_user_budget`, `slash_after_evaluation_cannot_add_fuel`, `parent_pre_state_authorized_slash_preserves_cost_boundary`, `zero_bond_slash_noop_preserves_cost_boundary` | Mechanized in `SlashingComposition.v`; the slashing proof suite remains authoritative for core effect correctness, while this branch proves current-evidence authorization composition with token-cost settlement |
| Fuel channels are not de Bruijn application variables | `ChannelSeparation.v` | Mechanized syntactically |
| Runtime fuel channels are unforgeable and user-disjoint | `Sig`, `SignatureChannel`, `SignedProcess`, `RuntimeBudget` in `f1r3node-rust` | Implemented with `GPrivate` signature channels; tests cover deploy isolation and canonical compound signatures |
| Parallel scheduling preserves final cost | Rocq confluence plus TLA+ `EvalScheduling` | Mechanized/model-checked; Rust implementation must keep deterministic result aggregation |
| Parallel scheduling preserves trace commitments | `uc_ca_051_parallel_trace_and_cost_determinism`, `ca_cost_deterministic`, `rb_cost_trace_event_count_success_and_oop` | Mechanized cost/count basis; Rust tests check repeatable digest commitments (diagnostic stability) under multi-threaded interpreter execution |

The implementation-facing use-case map is maintained in
[*Cost-Accounted Rho Use-Case Coverage*](cost-accounting-use-cases.md).
It binds these proof obligations to property and integration tests in
`f1r3node-rust` without extending the proof trust base.

The Rust implementation names for the bounded-memory refinement are
`RuntimeBudget` and `MeteredMachine`. They are not additional calculus
constructors: `RuntimeBudget` coalesces the nested token stack into an
atomic consumed-token counter, while `MeteredMachine` supplies the
implementation's source-event descriptors and branch-local metering
context. The refinement obligation is therefore operational: every
successful `MeteredMachine` reservation must correspond to the finite
unit-token expansion covered by the token-count theorems, and every
failed reservation must expose the same canonical source-event
descriptor on every validator.

`Settlement.v` is intentionally outside the reduction relation. It proves
that post-evaluation escrow accounting is deterministic and conservative
when the consumed-token count is bounded by the deploy limit, and it
reuses `token_monotone_reachable` / `token_strictly_decreases` to rule
out any interpretation where Casper refunds or balance edits add fuel
back into an in-flight evaluation. `SlashingComposition.v` sits at the
same boundary. It adopts the slashing-side interface proven in
f1r3node-rust's `analysis/slashing` branch and proves only the
cost-accounting composition facts: current cost-invalid evidence is
observational for deploy cost, recovered rejected slashes require current
evidence and target activation epochs, parent pre-state bond authorization
preserves the cost boundary, and slash system effects preserve user fuel,
fee settlement inputs, and settlement arithmetic. The authenticated trace
obligation is therefore protocol-level: deploy signatures bind the phlo
limit and price, and block signatures bind the processed deploy cost plus
replay log and slash target epoch that fed settlement and slashing.

---

## 12. Assumptions and Trust Base

### 12.1 Explicit Assumptions (Section Hypotheses)

The formalization is parameterized over one abstract `hash_process`
encoding and three section hypotheses about that encoding. These are
**not axioms in the Rocq kernel** — they become universally quantified
parameters after section discharge, appearing transparently in
`Print Assumptions`.

| # | Parameter / Hypothesis        | Kind                       | Statement                        | Rationale                                                                                                                                                                                                                       |
|---|-------------------------------|----------------------------|----------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| 1 | `hash_process`                | Encoding parameter         | Variable: `list bool → proc`     | The canonical process encoding of a byte string σ. The proof leaves the construction abstract; any concrete instantiation must satisfy hypotheses 2–4 below.                                                                    |
| 2 | `hash_process_injective`      | Cryptographic              | ∀b₁, b₂. H(b₁) = H(b₂) → b₁ = b₂ | **Collision resistance**: distinct byte strings produce distinct processes. Inherited from the cryptographic strength of whatever hash function the encoding is built upon.                                                     |
| 3 | `hash_process_closed`         | Encoding constraint        | ∀bs. closed_proc(H(bs))          | **Closedness**: hash processes contain no free de Bruijn variables. A purely structural property of the encoding — easily satisfied by encoding bytes as ground processes.                                                      |
| 4 | `hash_process_head_count_one` | Encoding constraint        | ∀bs. head_count(H(bs)) = 1       | **Single-head encoding**: the encoded hash sits under exactly one top-level head (e.g., a single `PSend` or `PInput`). This is a structural property of the chosen encoding — *not* a cryptographic claim.                   |

**Per-theorem dependency table.** Below, "Hyp k" means the proof
references entry #k above, including the abstract encoding parameter in
row 1.  "—" means the proof is unconditional and `Print Assumptions`
reports `Closed under the global context`.

| Theorem family                                     | Hyp 1 | Hyp 2 | Hyp 3 | Hyp 4 |
|----------------------------------------------------|-------|-------|-------|-------|
| Contextual forward reachability (`translation_faithful`) | ✓     | ✓     | ✓     | —     |
| Atomic bisimulation                                 | ✓     | ✓     | ✓     | —     |
| Fuel-gate safety (`fuel_gate_stuck_isolated`)       | ✓     | ✓     | ✓     | —     |
| Atomic per-step reverse                             | ✓     | ✓     | ✓     | —     |
| Compound per-step reverse + compound bisim         | ✓     | ✓     | ✓     | ✓     |
| `ca_strongly_normalizing` / `ca_max_steps_bound`    | —     | —     | —     | —     |
| `ca_local_confluence` / `newman` / `ca_confluent`   | —     | —     | —     | —     |
| `ca_normal_form_unique` / `ca_cost_deterministic`   | —     | —     | —     | —     |
| `ca_step_deterministic` / `single_token_path_unique`| —     | —     | —     | —     |
| `token_monotone_step` / `_reachable` / `_strict`    | —     | —     | —     | —     |
| `fuel_events_consumed_perm`                         | —     | —     | —     | —     |
| `ChannelSeparation` results (`N_tr_is_Quote`, …)    | —     | —     | —     | —     |
| `Settlement` results (`charged_plus_refund_eq_escrow`, `post_evaluation_settlement_no_mint`) | — | — | — | — |
| `SlashingComposition` results (`slash_preserves_fee_settlement_inputs`, `slash_after_evaluation_cannot_add_fuel`) | — | — | — | — |

The consensus-critical row block (everything below the divider) is
unconditional: `Print Assumptions ca_cost_deterministic` and
`Print Assumptions ca_step_deterministic` literally print
`Closed under the global context`.

Hypotheses 3 and 4 are *encoding constraints* on the chosen
representation of `hash_process` and are therefore satisfied by
exhibiting any representative that meets them; hypothesis 2 is a
*cryptographic* assumption on the underlying hash. The proof is
agnostic to which representative or which hash function is selected,
so long as the parameter and three conditions hold of the choice. Discharge in any
particular implementation is outside the scope of this article.

### 12.2 Trusted Computing Base

- **Rocq 9.1.1** kernel (the type checker that verifies all proofs);
  the development also typechecks under **Rocq 9.1.0**. Per-rule
  determinism proofs in `Confluence.v` use
  `inversion H; subst; solve_no_substep` — the recursive tactic
  matches inner hypotheses by *shape* rather than by fragile numeric
  auto-names, so minor-version auto-naming shifts are tolerated.
- **Rocq Stdlib** (`Lia`, `Lists.List`, `Sorting.Permutation`)
- The `hash_process` parameter and three section hypotheses listed above (Section 12.1)
- **No** `Admitted`, `admit`, `Conjecture`, `Parameter`, or `Axiom`
  declaration in the theory files. Section-scoped hash assumptions are
  discharged as ordinary theorem parameters by Rocq.

**Trust-base hierarchy** (stronger → weaker):

```
┌─────────────────────────────────────────────────────────────┐
│ Tier 1 — Kernel                                             │
│   Rocq 9.1.1 type-checker; Rocq Stdlib.                     │
│   Universally trusted; any proof inhabits this layer.       │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│ Tier 2 — Section parameters/hypotheses (Section 12.1)       │
│   H1–H4 entries for the `hash_process` encoding.            │
│   Discharged by any concrete hash instantiation that        │
│   satisfies the encoding parameter and three constraints.   │
│   Scope: translation-side theorems only.                    │
└─────────────────────────────────────────────────────────────┘
```

Consensus-critical theorems (the results on which blockchain safety
depends — token conservation, cost determinism, step determinism,
fuel-event multiset determinism, strong normalization, confluence,
fuel-gate safety) inhabit **Tier 1 alone**: they report
`Closed under the global context` under `Print Assumptions`.

### 12.2.1 Proof Hygiene Gate

The repository enforces an axiom-free formalization. The gate
`scripts/check-cost-accounted-rho-proofs.sh` fails if any theory file
contains:

```text
Admitted.
admit.
Conjecture ...
Parameter ...
Axiom ...
```

The same gate imports the headline theories in `rocq repl` and checks
that the implementation-facing theorem set is closed under the expected
context:

```coq
Check translation_faithful.
Check translation_strong_bisimilar_generic.
Check compound_gate_per_step_reverse.
Check backward_reflection_phased_gate.
Check well_reflected_backward_reflection.
Check recursively_metered_backward_reflection.
Check preplicate_bang_encoding_body_barbs_sound.
Check replication_encoding_forward_barb_sound.
```

The hash-process assumptions in Section 12.1 remain visible as ordinary
section hypotheses in the theorem statements that need them. They are
not kernel axioms and do not affect the unconditional consensus-critical
theorems.

### 12.3 Scope Boundaries and Design Decisions

The following items are deliberately outside the scope of the current
formalization. For each, we explain *why* it is excluded and *what
existing results already cover* the essential content.

**Refinement to an implementation.** This article's theorems
characterize the cost-accounted rho calculus and its translation as
*mathematical objects* — `ca_step`, `S_tr`, `bisim`, etc. They do
*not* relate those objects to any concrete implementation: there is
no Rocq-level refinement statement linking `ca_step` to a particular
evaluator, and none is in scope. Implementations that wish to rely
on these results must independently establish (by whatever means
appropriate to their setting) that their executable artefacts realise
the same `S_tr`, `ca_step`, and event-counting discipline the proofs
characterize.

**Full abstraction.** The formalization proves **strong bisimilarity**
(`~~`) between the translated process and the original for all three
signature shapes (Theorems 9.7, 9.9). Strong bisimilarity is strictly
stronger than barbed bisimulation and, for image-finite processes,
implies barbed congruence [5, Theorem 2.4.36]. The cost-accounted rho
calculus operates over a finitely-branching syntax (no infinite-state
replication in the current model), so all translated processes are
image-finite. Therefore, **strong bisimilarity as proven already implies
full abstraction** for the image-finite fragment. The infinite-state
replication appendix is scoped to the operational unfold and axiom-free
body-to-wrapper weak-barb propagation theorem described in Section 6.6;
it does not claim full abstraction for arbitrary replicated wrappers.

**Persistent infrastructure.** The paper [4, Appendix A] (persistence
remark) notes that Split and Join should be replicated (persistent) in
practice, observing that
the standard rho-calculus encoding of replication via self-reference
through reflection [1] applies directly. This formalization adopts a
**two-lens design**, mechanizing both views:

1. `PReplicate : proc → proc` is retained as a primitive constructor
   with reduction rule `rs_replicate : PReplicate P ⇝ P ∣ PReplicate P`
   (`RhoReduction.v`), structural equivalence congruence
   `se_replicate_cong` (`RhoSyntax.v`), and the auxiliary counting
   function `count_replicates` with preservation lemma
   `count_replicates_se` (`StructEquivInversion.v`). This view matches
   Rholang's runtime semantics: the surface form `contract x(y) = { P }`
   compiles to a persistent-receive node (`Receive { persistent := true }`),
   corresponding directly to `PReplicate (PInput x P)`.

2. The reflective encoding from Meredith-Radestock 2005 §3 is mechanized
   in `theories/Replication.v`. The module defines Meredith's auxiliary
   `D_encoding x ≜ for(y <- x){x⟨|*y|⟩ ∣ *y}` and the bang encoding
   `bang_encoding x P ≜ x⟨|D(x) ∣ P|⟩ ∣ D(x)`, and proves the
   load-bearing operational fact:

   ```
   Theorem bang_encoding_unfolds : forall x P,
     closed_name x -> closed_proc P ->
     rho_step (bang_encoding x P) (PPar (bang_encoding x P) P).
   ```

   One `rs_comm` step of the encoding produces a fresh copy of `P` in
   parallel with the regenerated encoding — exactly the behavior of
   `rs_replicate` step-for-step. The trace relies on the semantic-
   substitution rule of [4, §3.4] (mechanized in R.1 as
   `subst_proc_deref_nvar_eq_quote`): under the substitution
   `{⌜D(x) ∣ P⌝ / y}` the sub-terms `*y` collapse to `D(x) ∣ P`,
   regenerating the sender--receiver pair.

   The `bang_encoding` form (lens 2) is provided to justify the
   paper's §5 Remark at the operational level via
   `bang_encoding_unfolds`, and to prove the body-to-wrapper weak-barb
   propagation theorem (Section 6.6,
   `replication_encoding_forward_barb_sound`). The stronger strong-
   bisimilarity claim `bisim (PReplicate P) (bang_encoding x P)` is
   not a faithful statement in rho calculus: `bang_encoding x P` has
   top-level barbs on the coordination channel *x* that
   `PReplicate P` lacks under the freshness hypothesis. Rho calculus,
   by design (Meredith–Radestock 2005), has no `ν`/`PNew`
   restriction binder — reflection subsumes name restriction as a
   theoretical primitive, eliminating the need for a separate hiding
   construct. Accordingly, the theoretically appropriate equivalence
   in this calculus can be specified as **weak barbed equivalence modulo
   hidden *x***, which formalizes hiding at the equivalence-relation
   level rather than via a syntactic binder. This relation is defined as
   infrastructure, but no headline theorem assumes the bidirectional
   equivalence. All consensus-critical results —
   contextual forward reachability, per-step reverse,
   bisimulation, cost determinism, token conservation, fuel-gate
   safety — continue to use the primitive `PReplicate` constructor
   (lens 1), which is what the Rholang runtime's persistent-receive
   compiles to (`contract x(y) = { P }` →
   `PReplicate (PInput x P)`). None of these results depend on any
   Section 12.2.1 axiom.

The persistent mediators `PersistentSplit` and `PersistentJoin` are
defined as `PReplicate (Split s₁ s₂)` and `PReplicate (Join s₁ s₂)`
respectively, with closedness proofs (`Translation.v`). The
`PReplicate` constructor is treated as an atomic head (like
`PInput`/`POutput`/`PDeref`) with `head_count(PReplicate P) = 1`, and
the `count_replicates` function is used in stuck-process arguments to
dismiss `rs_replicate` cases by contradiction (canonical forms in the
translation have `count_replicates = 0`).

The `Split` and `Join` definitions in `Translation.v` cover the
single-firing case formally (used by Theorems 9.7 and 9.9 to verify
Rules 3, 4, and 5). Persistent variants inherit one-step reduction
behaviour from `PReplicate`'s structural-equivalence congruence.
**No theorem targets a cost for replicated mediators — this is not a
gap but a consequence of the formal definitions.** The cost notion
(`ca_step`) is defined on the cost-accounted system grammar
`SSigned | SToken | SPar`, which contains no `Split` or `Join`
constructor; mediators live exclusively in the translation target
(pure rho calculus) and never appear in a cost-accounted system that
`ca_step` can reduce. They are infrastructure processes, not cost-
accounted primitives. The migration document (§5.8.4) deploys them
with zero phlogiston cost on this basis.

**Dequotation reduction.** The rule `*(@P) ⇝ P` is deliberately excluded
from the operational semantics for three mutually reinforcing reasons:

1. *As a reduction rule* (`rs_dequote`): It would falsify the stuck
   lemmas (`PDeref_stuck`, `deref_no_barb`) that are load-bearing in the
   fuel-gate safety proofs and the per-step reverse simulation. Every
   `PDeref (Quote P)` residue in the post-gate state would become
   reducible, requiring all 260+ inductive proofs to handle a new case
   that fundamentally changes the reduction landscape.

2. *As a structural equivalence axiom* (`se_dequote_quote`): Adding
   `PDeref (Quote P) ≡ P` breaks `head_count_se` (the theorem that head
   count is preserved under `≡`), because `head_count(PDeref (Quote P))`
   = 1 but `head_count(P)` can be any value. Since `head_count_se` is
   the foundation of the heads-list permutation machinery
   (`struct_equiv_heads_perm`, `fh_compound_heads_split`, etc.), this
   would invalidate the entire per-step reverse simulation.

3. *The observational content is already captured.* The post-gate residue
   `*(@0)` is proven to be observationally inert: it has no barbs
   (`deref_no_barb`), it cannot participate in any COMM
   (`backward_sim_par_stuck`), and the parallel composition
   `P ∣ *(@0)` is strongly bisimilar to `P` (`post_gate_bisim`).
   Adding dequotation as a rule would allow `*(@0) ⇝ 0`, but since
   `P ∣ 0 ≡ P` by the identity axiom, the end state is the same —
   the extra reduction step adds no observational information.

In the pure rho calculus of [1], dequotation is part of the substitution
mechanism (it fires during COMM, not as an independent step). The
formalization faithfully follows this design.

**Fuel event multiset determinism.** The commutativity of fuel event
consumption — i.e., the fact that the multiset of consumed fuel events is
determined solely by the start and end states of a reduction path,
independent of the order in which redexes fire — is now a proven property
of the formalization. Theorems 9.16–9.18
(`FuelEventDecomposition.v`) establish that every single step decomposes
the fuel event multiset into a non-empty consumed prefix and a remainder
(Theorem 9.16), that multi-step paths compose these decompositions
(Theorem 9.17), and that whenever two paths share a start state and reach
states with permutation-equivalent residual fuel events, the consumed
event multisets are themselves permutation-equivalent (Theorem 9.18).
Together, these results place fuel event accounting on the same
mechanically verified footing as the rest of the formalization.

**Proofs are modulo structural equivalence `≡`.** Every headline
theorem in this development is stated on terms up to the Rocq
structural equivalence relation `struct_equiv` (`RhoSyntax.v`,
notation `≡`). In particular `ca_cost_deterministic` guarantees that
two terminal states reached from the same start state have the same
`system_token_count` **when those terminal states are related by `≡`
modulo reordering of parallel compositions and identity/associativity
axioms** — it does *not* guarantee agreement on any other notion of
process equality. For the deployed system to inherit this guarantee,
the process canonicalizer used at runtime (RSpace's normalizer) must
respect `≡` equivalence classes:

```
normalize_preserves_struct_equiv :
  forall P Q, P ≡ Q -> normalize P = normalize Q.
```

RSpace is implemented in Rust and is outside the Rocq mechanization
boundary. The implementation boundary discharges this correspondence
behaviorally: structurally equivalent deploy shapes must produce the
same token cost, and compound signature channels are canonicalized
before they are used as runtime fuel channels. Divergence of
`normalize` from `≡` at runtime would break cost determinism in the
deployed system even though the Rocq proofs remain intact, so the Rust
test suite treats this as consensus-critical implementation behavior.

**Threat-model adequacy.** The implementation-aligned threat model is
recorded in
[`cost-accounting-threat-model.md`](cost-accounting-threat-model.md).
The Rocq proof anchors for its security and thread-safety vectors are the
UC-CA-053 through UC-CA-074 theorem families in
`UseCaseAdequacy.v`, with TLA+/Sage/Rust search-frontier models providing
bounded interleaving, objective-frontier, and production regression
coverage for UC-CA-069 through UC-CA-074. Together they cover
trace-domain separation and
multiplicity, post-activation rejection of absent commitments,
unauthorized settlement and budget mutation, low-price and stale
cost-invalid evidence, refund/fuel separation, descriptor sensitivity,
finalization-read trace retention with deploy-reset clearing,
system-mode restoration,
block-authenticated cost fields, threaded OOP boundary ownership, and
external nondeterminism reflected through replay evidence. The latest
hardening anchors distinguish a valid zero-event trace from an invalid
zero-weight billable event, prove invalid billable admission preserves
budget and trace state, bound retained trace slots before mutation, and
add a search-frontier discipline for generated threat witnesses,
producer-routing regression guards, trace-slot linearizability checks,
replay mutation search, multi-deploy settlement search, slashing
composition search, resource-exhaustion search, bounded generative
Rholang term-family search, semantic metamorphic replay, mocked
external-service replay, and RuntimeBudget event-sequence property
testing. The v9 differential corpus/security frontier adds executable
source-corpus semantic replay, grammar-mutation equivalence checks,
production play/replay and parser-error differential oracles, GPT/DALL-E/
TTS/gRPC external-service matrix replay, Casper authenticated-payload and
settlement/slashing security axes, runtime trace interleaving checks, and
a dedicated coverage-adequacy gate. The v10 hybrid fuzz/security frontier
adds fuzz-seed and Kani-bound promotion metadata, lifecycle trace replay,
replay-payload mutation matrices, Casper block-auth composition, mocked
external-service error replay, semantic Rholang corpus mutation, parallel
schedule stress, settlement/refund isolation, slashing isolation, legacy
downgrade quarantine, and a replay-target/promotion-gate adequacy check.
The v11 source-anchored frontier binds each generated witness to current
`f1r3node-rust` file/symbol/line/source-risk metadata, and the v12
production-oracle frontier requires those anchored witnesses to replay
through native RuntimeBudget, metering, parallel-evaluation, Casper replay,
settlement, slashing, and legacy-quarantine Rust oracles before promotion.
The v13 source-semantic frontier composes those anchors and native oracles
into cross-surface obligations for runtime-to-replay trace commitment,
runtime-to-settlement fuel isolation, metering-to-parallel digest stability,
replay-to-slashing authentication, and legacy-to-runtime quarantine.
These v8/v9/v10/v11/v12/v13 search artifacts are
empirical adequacy evidence; any normative counterexample they expose
must still be promoted into the Rocq/TLA+ proof layer before it changes
the formal specification.

**Native four-sort signed-term grammar.** The paper's §3.1 grammar is a
*four-sort mutually-inductive* syntax in which the `for`/`send` continuation
bodies are themselves **signed terms** ("signed terms pervade the syntax",
§1/§3.1; Remark 3.8 requires a received term retain its signature/cost
provenance). The mechanization instead uses a **proc-under-system**
representation (`SSigned : proc → sig → system`, with `PInput`/`POutput` carrying
bare-`proc` bodies; `CostAccountedSyntax.v`, `RhoSyntax.v`) and discharges the
§3.2/§3.5/§3.8 signed-term identities at the **source/translation level**
(Option A, `SyntacticSugar.v`, axiom-free). This is the representation choice
recorded in DR-17. One consequence is visible in the operational model: because a
continuation is a bare `proc` with no seal of its own, `ca_rule4`/`ca_rule5`
(`CostAccountedReduction.v`) re-seal the Rule-4/5 result under the compound
`SAnd s₁ s₂`, where the paper's Rule 4/5 RHS keeps the receiver's seal `s₁`
(uniform signing, §3.8). `Rule45ContinuationAdequacy.v` proves this re-seal is
**cost-benign** — a seal carries no fuel (`system_token_count (SSigned _ _) = 0`),
so the token count (the consensus-metered cost) is identical under either seal —
and the §5 s₀-limit bisimulation collapses the distinction entirely (at s₀ every
signature is equal). The faithful alternative — a native four-sort
mutually-inductive grammar in which continuations retain their own seal, which
dissolves the re-seal outright — is a **scoped representation migration**
(`RhoSyntax` + `CostAccountedSyntax` + `CostAccountedReduction` + every downstream
proof re-mechanized; DR-17 Option B). It is governed by an explicit trigger: it is
undertaken when, and only when, a required result must reason **natively** about a
multi-signature continuation's own seal (rather than its cost, which Option A and
the adequacy theorem already settle). Until that trigger is met, Option A
discharges the paper's defining equations and the adequacy theorem proves the
residual benign. (See DR-17, DR-20.)

**Update — the native grammar is now realized (DR-21).** The Option-B migration
above has been **executed**, triggered by the sibling paper
`continued-gslt-cost-v2.tex` (whose "wrapping by construction" IS this native
grammar). The cost-accounted source is now the four native sorts
(`CASyntax.v` — `caproc`/`caname`/`signed_term`, continuations are signed terms),
with the pure rho `proc` kept as the unchanged translation target (the carrier
split). Natively the Rule-4/5 continuation keeps its own seal, so the `SAnd`
re-seal is **absent** — GAP-2 is dissolved, not merely benign. The central claims
of `continued-gslt-cost-v2` are discharged axiom-free by
`continued_gslt_cost_capstone` (`ContinuedGSLTCapstone.v`): wrapping by
construction (subject reduction / no-leak, `WrappingSubjectReduction.v`), the
Cost monad laws (the two constituent monoids, `SignatureMonoid.v`), GAP-2
dissolution, cost determinism on the funded fragment (`CACostDeterminism.v`), and
the stack modulus (`CAModulus.v`). Native strong normalization is conditional on
the linearly-funded fragment — the consensus-relevant class, since only funded
deploys are admitted (`CAStrongNormalization.v`). The native re-statement of the
translation/bisimulation stack — and the graded-HML adequacy and two adjunctions
that rest on it — continues; the construct-by-construct map is in
[cost-accounting-as-monad-correspondence.md](cost-accounting-as-monad-correspondence.md).

**Implementation-delegated parameters.** Four constructs the paper uses but
deliberately leaves to the implementation. (i) **The hash function** for
crypto-quoting `#P` (§4.2): the paper specifies "a configurable hash function
(SHA-256, Blake2b, …)" and the mechanization parameterizes over it
(`hash_process`, §12.1; the three structural/cryptographic hypotheses on it are
the only non-trivial Section-hypotheses, §11.1; the G-parametric realization is
DR-16). (ii) **Name equality** `≡_N` (§3.4): used in the communication rules to
decide when a send and a receive share a channel, but never defined at its use
site; the implementation realizes it as structural equality of the normalized
quoted process — the runtime correspondence already recorded above
(`normalize_preserves_struct_equiv`). (iii) **The per-signature supply-pool
runtime representation** (§4.6/§4.7): the paper fixes the *behavior* (a token is a
message resident on `Σ⟦s⟧`; supply is injective in the signature) and the
mechanization fixes the *balance datum* (DR-13) and its disjointness
(`lane_pool_disjoint`), but the concrete in-memory container (the runtime
`DashMap<Sig, AtomicI64>`) is an implementation choice the paper does not
constrain. (iv) **The funding-key instantiation** (§D2.9). The paper's funding
signature `s` — the key of the per-actor pool `Σ⟦s⟧` (§4.6/§4.7) — is an
*abstract parameter* over the backend `G` (Def 3, DR-2/16): the model fixes only
that `Σ` is injective in `s` and that distinct principals get distinct pools. The
Rocq model already commits the *validator* instance concretely — `WalletNaming.v`
keys the draw wallet `@W_v := @(*walletTag, validatorPk)` by the **public key**
(modeled `SGround : list bool → sig`), with `wallet_name_injective` proved
axiom-free ("Closed under the global context"); **no formal artifact ties a pool
to a *wire signature***. The implementation's §D2.9 correction therefore
instantiates this abstract funding key as `Sig::Ground(pk)` for user deploys
(`funding_sig`: single-sig `Ground(pk)`, multi-sig the left-associated `And`-fold
of `Ground(pkᵢ)`) — exactly the pubkey naming `WalletNaming.v` already proves
injective — so that `Σ⟦signer⟧ == Σ⟦wallet⟧`. The `deploy_id` continues to derive
from the wire signature (`envelope_sig`); that is on-chain identity (which the
model does not constrain), not a funding key. The strict-compound effective-supply
check the §D2.9 replay recompute performs,
`effectiveΣ_{s₁∘s₂} = Σ_{s₁∘s₂} + min(Σ_{s₁}, Σ_{s₂})`, is the **already-proven
Split/Join algebra** (the `Split`/`Join` mediators + `CAJoinConservation`, App. §4.8.4)
applied at replay — no new proof obligation; the prior wire-sig keying was the
outlier the code corrected to match the model. See
`cost-accounting-impl/d2-9-funding-flow.md` and `wd-d2-acceptance-gate.md` §D2.9.

**§D2.9-R2 (TM-CA-166) — no-weakening closure correction (code-to-model, no model
change).** A red-team found `effective_supply_with` additionally credited a *single*
component with the compound pool (`effective[s₁] = Σ_{s₁} + Σ_{s₁∘s₂}`), but the
settlement's single-sig draw can only reach `Σ_{s₁}` — a static **weakening** credit
the model already forbids (`CAJoinConservation.join_no_weakening`, axiom-free:
`s₁∘s₂` carries strictly more signature atoms than `s₁`, so it cannot be discharged
as `s₁` alone). The over-credit was a *code-only outlier* present in no
`.v`/`.tla`/`.sage`/`.tex`; the code now drops it (a single component passes through
at its raw balance `effective[s₁] = Σ_{s₁}`), so `effective_supply_with` MATCHES the
model — again a code-to-model correction, **no model change**. Latent today (genesis
seeds only per-pubkey wallets ⇒ `Σ_compound = 0`).

**TM-CA-165 — cross-group cumulative-demand bound (FV ADDITIONS, no existing proof
invalidated).** The gate's admission decision + the replay re-verification now run a
LIVE cross-group residual ledger so two DISTINCT cosigner sets sharing a component
wallet cannot jointly over-draw it (linearity, no contraction). Verified full-stack
by ADDING (not changing): Rocq `cross_group_draw_le_supply` +
`cross_group_admission_sound` in `LinearLogicResources.v` (axiom-free, generalizing
`competing_funding_at_most_one_succeeds`/`admitted_prefix_fits`); TLA+
`Inv_CrossGroupAdmissionBounded` + `Inv_SecondGroupDrawMatchesDemand` in
`CompoundSettlement.tla` (its `AdmitGate` now threads the shared residual; TLC PASS);
a Sage cross-group admission sweep (12,605 traces, 0 sound + 0 necessity violations).
No EXISTING proof is invalidated — the additions strengthen the funding-soundness
layer. See `cost-accounting-impl/d2-9-funding-flow.md` §4, `wd-d2-acceptance-gate.md`
§D2.2, threat-model TM-CA-165/166, and DR-28. Each delegation is intentional in the
paper; the implementation's choice is consistent with every behavioral law the paper
does fix. (See DR-20.)

---

## 13. References

[1] L. G. Meredith and M. Radestock, "A reflective higher-order
    calculus," *Electronic Notes in Theoretical Computer Science*,
    vol. 141, no. 5, pp. 49–67, 2005.
    [doi:10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016)

[2] R. Milner, *Communicating and Mobile Systems: the π-Calculus*,
    Cambridge University Press, 1999. ISBN 978-0-521-65869-0.

[3] L. G. Meredith *et al.*, "Rholang Specification," F1R3FLY.io /
    RChain Cooperative, 2017–2026.

[4] L. G. Meredith, "Cost-Accounted Rho Calculus: A Spectral Decomposition
    of Phlogiston," F1R3FLY.io, May 2026.

[5] D. Sangiorgi and D. Walker, *The π-Calculus: A Theory of Mobile
    Processes*, Cambridge University Press, 2001.
    [doi:10.1017/9781316134924](https://doi.org/10.1017/9781316134924)

[6] The Rocq Development Team, "The Rocq Prover Reference Manual,"
    Version 9.1.1, INRIA, 2025.
    [https://rocq-prover.org/doc/](https://rocq-prover.org/doc/)

---

## Appendix A. Option E: Post-Hoc Canonical Reconciliation

The `RuntimeBudget` Rust implementation uses lock-free CAS attempts
against a shared `consumed_tokens` counter. Multiple concurrent
parallel-reduction tasks race for the CAS; whichever wins gets the
weight. The runtime's grant/oop decision is for *liveness* — once
the budget is exhausted, no further branches do paid work.

**Consensus-surface scope (read first).** The single consensus cost
quantity computed here is `total_cost` (clamped) — together with the
deploy status and the post-state hash, those are the consensus cost
integrity of a deploy. This is the **token-per-COMM** cost model of
**DR-9**: consensus enforces the conserved token total, and the
per-operation `cost_trace_digest` and `cost_trace_event_count` are
diagnostic only — **not** consensus quantities. They are removed from
the replay comparison and from the signed block-hash preimage, and are
retained as **diagnostics/telemetry only** (see TM-CA-151 in
[`cost-accounting-threat-model.md`](cost-accounting-threat-model.md), and
**DR-9**/**DR-16** in
[`cost-accounting-decision-records.md`](cost-accounting-decision-records.md)).
The post-hoc canonical reconciliation below is therefore the bounded-`K`
machinery that computes the consensus `total_cost` and a *diagnostic*
boundary (the multi-parent merge dispatcher this reconciliation feeds is
retained per **DR-15**); it is no longer presented as the protector of a
consensus digest.

**Where determinism actually comes from.** The schedule-independence of
the consensus quantity `total_cost` is *not* manufactured by the
reconciliation, and it is *not* a per-fork-private ledger. It is a
consequence of two structural invariants of the existing runtime, each
guarded by a debug-assert/property test: (a) `eval_inner` forks *every*
Par term — including single-term bodies — into its own metering child
with a **fresh** `next_local_index` (it never charges on the shared
parent counter, and continuations re-root through `eval_inner`, so no two
concurrent scopes share a counter); and (b) RSpace selects match
candidates by a **deterministic** candidate hash (no RNG). Together these
make the billable multiset of a non-OOP deploy a function of the deploy
and its initial budget alone; reconciliation then folds that multiset
into `total_cost`. On out-of-phlogiston the committed multiset is
schedule-dependent — which is exactly why the per-operation digest cannot
be a consensus quantity — but `total_cost` is clamped to `initial` and is
identical across schedules.

### A.1 Paper alignment

Per §3 Rule 1 of `cost-accounted-rho.tex`: within a single deploy,
all sub-processes share the deploy signature `σ_deploy`. The
applicable rule is the shared-token form `(P)^σ | σ:T → P^σ | T`.
The paper does NOT prescribe an ordering between sibling sub-processes
that both consume from the shared `σ:T` — only that the final state
is bisimilar across reductions.

Option E picks **the canonical-rank order** for the diagnostic trace and
for computing `total_cost`: events sorted by `(deploy_id, source_path,
redex_id, local_index, kind, weight)` (all program-structure-derived).
For a non-OOP deploy, two runtime executions over the same deploy +
initial budget produce identical canonical sequences regardless of Tokio
scheduling (by the two invariants above), and therefore identical
`total_cost`. The canonical order also fixes a deterministic *diagnostic*
OOP boundary; that boundary's identity is not a consensus quantity.

This is a strict *refinement* of the paper: any property the paper
proves about `(P)^σ | σ:T` reductions holds for the canonical order
(as one specific schedule), and Option E adds `total_cost`
schedule-invariance.

**Faithfulness to the paper.** The paper (`cost-accounted-rho.tex`)
models cost as token-gated COMM with token conservation (Rules 1–5,
§3.6) and faithfulness as operational bisimulation plus capability
security (§4 and §5); it has **no per-operation cost-trace or digest
concept.** The runtime correlate of the paper's cost is `total_cost`
(the conserved token total, clamped on OOP), which remains
consensus-checked, and the consensus-critical theorems of this document
(`token_monotone_*`, `ca_cost_deterministic`, `ca_step_deterministic`,
`fuel_events_consumed_perm`) do not reference the digest at all. The
runtime's per-operation metering is a refinement *below* the paper's
COMM-token granularity, so committing the digest to consensus would have
bound consensus to a level of detail the paper does not model; dropping
it returns the consensus surface to the paper's cost granularity.

### A.2 Implementation contract

- `attempt_log: Arc<Mutex<Vec<AttemptRecord>>>` — every reservation
  ATTEMPT recorded (whether or not the runtime CAS race granted it),
  briefly mutex-protected per push.
- `consumed_tokens: Arc<AtomicI64>` — runtime liveness counter; CASed
  by parallel workers. May NOT equal the canonical consumed value if
  races occur.
- `canonical_reconciliation: Arc<Mutex<Option<CanonicalReconciliation>>>`
  — cached output of `reconcile()`; invalidated by `reset_from_token`.
- `reconcile()` — a **bounded lowest-`K` commutative merge** with
  `K = min(MAX_COST_TRACE_EVENTS, initial + 1)`. Because every billable
  weight is ≥ 1, the canonical walk commits at most `initial` events plus
  one OOP boundary, so it reads only the lowest-`K` events rather than
  sorting the whole attempt list. It yields canonical
  `(committed, oop, consumed_units)` and `total_cost`; it is a pure
  function of `(initial, multiset of attempts)`, removing the global
  O(N log N) sort over up to `MAX_COST_TRACE_EVENTS` elements and bounding
  memory. `total_cost` and the diagnostic boundary are unchanged by the
  switch from sort-truncate-walk to bounded-fold.
- Reset is strictly between deploys (finalization is single-threaded), so
  it is not serialized against in-flight batch reservations; the earlier
  `reset_serializer` read/write lock is removed in favor of a
  single-threaded-finalization debug-assert, and per-op
  `deploy_id`/`initial`/`unmetered` are copied by value into scopes.

### A.3 Theorem chain

| Layer | Theorem | What it proves |
|-------|---------|----------------|
| Rocq | `rb_event_weight_sum_permutation_invariant` | Multiset weight is permutation-invariant. |
| Rocq | `rb_reconcile_consumed_eq_min_initial_or_sum` | Canonical consumed = `min(initial, consumed_initial + Σ weights)`. |
| Rocq | `rb_reconcile_consumed_invariant_under_permutation` | Two permutations agree on canonical consumed. |
| Rocq | `rb_reconcile_oop_iff_sum_overflows` | OOP fires iff cumulative weight exceeds budget. |
| Rocq | `rb_reconcile_oop_occurrence_invariant_under_permutation` | Two permutations agree on whether OOP fires. |
| TLA+ | `RuntimeBudgetReplay.ConsumedAndVerdictScheduleIndependent` | `total_cost` (clamped) + OOP verdict are schedule-independent; the per-op digest is diagnostic, not a consensus quantity. |
| TLA+ | `RuntimeBudgetReplay.ConsumedFollowsReconciliationContract` | Consumed at finalization matches reconciliation contract. |
| Sage | `sage_concurrency_reconciliation_is_schedule_independent` | Sage scenario record cross-references all five layers. |
| Loom | `loom_runtime_budget_reconciliation::reconcile_canonical_oop_is_higher_rank_event_under_any_schedule` | Two concurrent attempts produce same canonical OOP under every loom-explored schedule. |
| Rust | `cost_accounting_spec::concurrent_runtime_budget_reservations_are_linearizable` | 16-thread concurrent reservation produces canonical-walk-derived `cost_trace_event_count` AND identical digest across two independent runs. |

**How to read this table after TM-CA-151.** Now that the per-operation
digest is a diagnostic rather than a consensus quantity, the
digest-centric rows are read as **`total_cost`/verdict
schedule-independence** properties (the consensus quantities that remain),
not as proofs of a consensus digest:

- `RuntimeBudgetReplay.ConsumedAndVerdictScheduleIndependent`
  is the re-aimed `total_cost`/verdict schedule-independence invariant — that the
  finalized **consensus** quantities (`total_cost`, OOP verdict) are a
  pure function of the recorded multiset and `initial`. The
  bounded-`K` `Merge` action it ranges over is unchanged; the
  OOP-truncation action now demonstrates *why* the per-op digest was
  removed from consensus (the committed set diverges across schedules)
  rather than something the model must hold invariant.
- The Loom and 16-thread Rust rows keep the non-OOP "identical across
  schedules" property as a **`total_cost`-determinism** invariant, and
  gain an OOP-truncation variant showing the recorded set legitimately
  diverges across schedules (so it cannot be a consensus quantity). Any
  "identical digest" assertion is retained only as a non-OOP diagnostic
  stability check, not as a consensus check.

The Rocq rows (`rb_reconcile_*`) already speak to canonical `consumed`
(i.e. `total_cost`) and OOP occurrence, which are precisely the consensus
quantities; they are unaffected by the decision beyond the bounded-`K`
refinement of `reconcile()` noted in A.2.

### A.4 What this fix closes

- **Direct**: `ReplayCostTraceMismatch` — closed because the
  per-operation `cost_trace_digest`/`cost_trace_event_count` are
  **removed from the replay comparison and the block-hash preimage**
  (TM-CA-151), so their OOP schedule-dependence can no longer cause a
  mismatch. (The bounded-`K` reconciliation still guarantees `total_cost`
  is schedule-independent for the non-OOP case and clamped to `initial`
  on OOP — that is the consensus quantity that remains checked.)
- **Cascade-closed**: the secondary `Missing mergeable entry`
  KvStoreError, `RootRepositoryDivergence` / `UnknownRootError`,
  and `UnauthorizedSlashDeploy` entries that previously stemmed
  from the digest mismatch (see `cost-accounting-threat-model.md`
  TM-CA-144, superseded by TM-CA-151).

*E Pluribus Potentia*
