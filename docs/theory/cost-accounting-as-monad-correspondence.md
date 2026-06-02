# Continued GSLTs / Cost Endofunctor — Correspondence Map

**Status:** Implementation-aligned correspondence record
**Governing papers (READ-ONLY law):**
- `publications/cost-accounting/cost-accounted-rho.tex` — the concrete cost-accounted rho calculus.
- `publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex` — *"Continued Interactive GSLTs and the
  Cost Endofunctor"* (the categorical construction, *one level up*).

This document maps each construct of the monad paper to the artifact that realizes it in `f1r3node-rust`,
across Rocq, Rust, TLA+, Sage, and Lean. It is the alignment record for "fully support
`continued-gslt-cost-v2`." See [DR-21](cost-accounting-decision-records.md) (the executed native four-sort
migration + the conditional-SN finding) and [DR-20](cost-accounting-decision-records.md) (the GAP enumeration).

The decisive enabling move is the **native four-sort grammar** (DR-21): `for`/`send` continuations are signed
terms (the wrapped-term sort 𝕋), so "signed terms pervade the syntax" is native and "every redex sits inside a
wrapper" is a sorting invariant — the paper's *wrapping by construction*. The pure rho `proc`/`name` of
`RhoSyntax.v` is kept unchanged as the translation target (carrier split).

## Correspondence table

| Monad-paper construct | Rocq realization (`formal/rocq/cost_accounted_rho/theories/`) | Other provers / runtime |
|---|---|---|
| Wrapped-term sort 𝕋; continuation slots wrapped | `CASyntax.v` — `caproc`/`caname`/`signed_term`; wrapper `STSigned`; `CPInput`/`CPOutput` carry `signed_term` | — |
| Interaction cut; the gated rule family R1–R3 | `CAReduction.v` — `ca_step`, the five gated COMM rules | — |
| **Wrapping by construction** (no leak = subject reduction) | `WrappingSubjectReduction.v` — `subject_reduction_wrapping`, `no_leak_requires_token`, `no_leak_stack_inert` | — |
| **Cost monad** (η, μ); laws **descend from the two monoids** | `SignatureMonoid.v` — `sig_monoid_comm/assoc/unit_l/unit_r` (`(Sig,*,())` up to ≡sig), `tok_concat_assoc/unit_l/unit_r` (free monoid); assembled in `ContinuedGSLTCapstone.v` `Cost_Monad_Laws` | **Sage** `cost_monad_laws.sage` (bounded-exhaustive); **Lean** `CostAccountedRho/CostMonad.lean` `cost_monad_laws` |
| Two monoids: spatial `K` (AC) vs temporal cons (free, never commutative) | `SignatureMonoid.v` `tok_concat_not_commutative`; spatial monoid `CAStructEquiv.v`/`SystemStructEquiv.v` | **Sage** `stack_concat_commutative_FAILS`; **Lean** `stack_concat_not_commutative` |
| μ non-idempotent (flatten forgets the boundary) | (the merge is genuine; witnessed bounded-exhaustively) | **Sage** `mu_non_injective_forgets_boundary` |
| Section of the ≡-quotient (`# = digest ∘ cf`) | `SystemStructEquiv.v` — `proc_encode` / `hash_preimage_encode` (canonical-form section); `crypto_quote` | runtime hash (DR-16 G-parametric; DR-20 (i)) |
| **GAP-2 dissolved** (split-process COMM keeps the continuation's own seal) | `CAReduction.v` `ca_rule4`/`ca_rule5` (no `SAnd` re-seal); `WrappingSubjectReduction.v` `gap2_split_{combined,split}_keeps_own_seal`; capstone `GAP2_Dissolved` | DR-21 (b) |
| **Cost determinism** (terminal cost unique) | `CACostDeterminism.v` — `newman_funded` → `ca_normal_form_unique_funded` → `ca_cost_deterministic_funded` (on the funded fragment) | runtime per-COMM cost (DR-9) |
| **Stack consumption is the modulus** | `CAModulus.v` — `funded_run_bounded` (run length ≤ consumed stack) | **TLA+** `LocatedPurse` `Inv_Conservation`; **Sage** modulus rows |
| Strong normalization (conditional on funding) | `CAStrongNormalization.v` — `ca_SN_funded`; the divergence witness `st_total_fuel_can_increase_off_funded` (SN is genuinely conditional) | DR-21 (c) |
| Located resource stacks / purses; nearness `near(I,J)` | `ChannelSeparation.v` `lane_pool_disjoint` (disjoint per-signature pools); `near` = name-equality `≡_N` (DR-20 (ii)) | **TLA+** `LocatedPurse` (`Inv_NoUnderflow`, `Inv_LocalSufficiencyComposes`); runtime `DashMap<Sig,…>` lane pool |
| The calculus IS a continued interactive GSLT with the cost structure | **`ContinuedGSLTCapstone.v` `continued_gslt_cost_capstone`** (axiom-free, "Closed under the global context") | — |

## Runtime correspondence (zero behavioral change)

The native migration adds **no new runtime behavior** (verified); the existing runtime already realizes the
monad-paper concepts:

| Paper concept | Runtime artifact |
|---|---|
| Unit η(P) = {P}_∅ (cost-free fragment) | `accounting/mod.rs` `RuntimeBudget::unmetered` / s₀-collapse |
| Lazy metering (charge when forced, not exposed) | per-COMM charge in `reduce.rs` / `metering.rs` (DR-9) |
| Located purses / disjoint per-surface pools | `accounting/mod.rs` per-signature `DashMap<Sig,…>` lane pool |
| Graded transitions (step labelled by consumed signature) | `BillableTokenEvent.sig_hash` |
| Two monoids (spatial vs temporal) | spatial `Par` (unordered) vs temporal `SourcePath` (ordered) |

## Honestly Rocq-primary / still in progress

Per the multi-prover allocation (DR-21 (d), and the design table): some claims are equational/logical and
Rocq-primary, with TLA+/Sage/Lean carrying genuine content only where there is operational/algebraic substance
(monad laws → Sage + Lean; located purses + modulus → TLA+). Two categorical claims rest on the **native
translation / bisimulation** (the source-to-source erasure into pure rho), whose native re-mechanization is the
remaining migration work:

- **Graded Hennessy–Milner adequacy** (graded-HML equivalence = quote-faithful bisimulation): requires the
  graded LTS + the OSLF-generated graded logic over `Cost(G)`. Rocq-primary; the graded LTS is the
  signature-labelled `ca_step`.
- **The two adjunctions** — Free ⊣ Forget (structural) and the internalisation ℐ_G : Cost(G) → G over
  Turing-complete bases (up to weak bisimulation). For the rho instance, internalisation is realized by the
  source-to-source translation into pure rho (`Translation.v` / `TranslationFaithfulness.v` / `Bisimulation.v`),
  whose native (four-sort) re-statement composes with the native `ca_step`.

These are tracked as the continuation of the native migration; the central structural claims (wrapping,
the cost monad, GAP-2 dissolution, cost determinism, the modulus) are discharged axiom-free by
`continued_gslt_cost_capstone`.
