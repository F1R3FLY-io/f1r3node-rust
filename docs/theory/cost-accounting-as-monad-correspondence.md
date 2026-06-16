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
| Cost endofunctor on concrete ciGSLT | `CACostFunctorCI.v` — `CostCI`, `cost_ci_preserves_step`, `cost_ci_preserves_bisim`, `cost_ci_preserves_quote_faithful` | — |
| Cost-endofunctor genericity: untyped-λ instance (rigid K ⇒ R1 only) | `CAUntypedLambda.v` — `lca_only_beta_r1`, `lca_funded_run_bounded`, `lca_SN_funded` (+ the Ω seam `omega_pure_diverges` / `lca_omega_funded_one_step`), `lca_beta_r1_erasure`; `CAUntypedLambdaCI.v` — `Lambda_ciGSLT` a 2nd object under `CostCI` (`Lambda_ciGSLT_nonvacuous`) | mettail `cost-decoration` emits one `Beta_R1` for λ vs five for comm (DR-25) |
| Section of the ≡-quotient (`# = digest ∘ cf`) | `SystemStructEquiv.v` — `proc_encode` / `hash_preimage_encode` (canonical-form section); `crypto_quote` | runtime hash (DR-16 G-parametric; DR-20 (i)) |
| **GAP-2 dissolved** (split-process COMM keeps the continuation's own seal) | `CAReduction.v` `ca_rule4`/`ca_rule5` (no `SAnd` re-seal); `WrappingSubjectReduction.v` `gap2_split_{combined,split}_keeps_own_seal`; capstone `GAP2_Dissolved` | DR-21 (b) |
| **Cost determinism** (terminal cost unique) | `CACostDeterminism.v` — `newman_funded` → `ca_normal_form_unique_funded` → `ca_cost_deterministic_funded` (on the funded fragment) | runtime per-COMM cost (DR-9) |
| **Stack consumption is the modulus** | `CAModulus.v` — `funded_run_bounded` (run length ≤ consumed stack) | **TLA+** `LocatedPurse` `Inv_Conservation`; **Sage** modulus rows |
| Strong normalization (conditional on funding) | `CAStrongNormalization.v` — `ca_SN_funded`; the divergence witness `st_total_fuel_can_increase_off_funded` (SN is genuinely conditional) | DR-21 (c) |
| Located resource stacks / purses; nearness `near(I,J)` | `ChannelSeparation.v` `lane_pool_disjoint` (disjoint per-signature pools); `near` = name-equality `≡_N` (DR-20 (ii)) | **TLA+** `LocatedPurse` (`Inv_NoUnderflow`, `Inv_LocalSufficiencyComposes`); runtime `DashMap<Sig,…>` lane pool |
| The calculus IS a continued interactive GSLT with the cost structure | **`ContinuedGSLTCapstone.v` `continued_gslt_cost_capstone`** (axiom-free, "Closed under the global context") | — |

## Token-source model — clean-slate realignment (DR-27, 2026-06-15; CORRECTED same day by Greg's authoritative answers)

A re-reading of the source `.tex` re-derived the token model; **Greg's 2026-06-15 SOLUTIONs then made it
authoritative and OVERTURNED the pre-answer "off-model" verdict on REV / `wallets.txt`** (those were a misreading).
The authoritative model:
- **ONE system token (canonical).** ONE consumable — the system token; phlogiston is the degenerate single-token
  case (the old homogeneous phlo = the `s₀`-collapse, cost-accounted-rho.tex:1481). It has NO consistent name —
  *token / Phlogiston / REV / Rock / F1r3caps* all denote it (Greg: "pegged" = identical); **avoid `F1r3caps`**
  (collides with F1R3FLY.io *Capabilities*); canonical = **phlogiston**. So **REV is a NAME for the one token, NOT
  a separate species** (and NOT off-model). **`Pay(τ)`** (typed_value.tex) is a **TYPE on that one token, NOT a
  second token** (Greg P9/P13); **stake** is a distinct locked-token ROLE (slashable, same denomination).
- **`wallets.txt` IS the genesis trust-root (Greg P12), NOT off-model.** One entry per wallet (balance = its
  available system tokens); the seed source of the per-signer pools `Σ⟦s⟧`. The impl's `client_fuel_allocations`
  IS that `wallets.txt` seeding mechanism (on-model). User-provided cons-notation tokens (signed) **desugar to
  system tokens**, the signature tracking origin for cost attribution. The legacy `SystemVault`/`MakeMint`
  value-transfer layer is a SEPARATE concern migrating to the one-token + `Pay(τ)`-type model (Greg P13, deferred —
  blocked on OSLF). Minting drops the `sysAuthToken` MONOPOLY → capability + (forward) type-gated, uniform protocol
  minting; genesis supply = `wallets.txt`.
- **`spacetime-functor.tex` is geometry, NOT storage.** It maps spent phlogiston to spacetime *volume*
  (Number = Volume); it does **not** model a storage/rent charge (the rent model is `rent_and_shard_splitting.tex`,
  rebased off the legacy `phloLimit×phloPrice` escrow per DR-27). Do not read it as the rent resolution.
- **Funding `Sig` is `g | #P | s∘s`.** The LL connectives `⊕/&/!/?/⊸` are the *value* type-logic (typed_value.tex) /
  the OSLF *type* layer — NOT funding-signature formers; the 6 extra `Sig` variants on the consensus wire are an
  undocumented extension (DR-27 F-A). This corrects an earlier reading that conflated the funding `Sig` algebra
  with the type-connective set.

See [DR-26](cost-accounting-decision-records.md) (verification posture: shapes, not certificates) and
[DR-27](cost-accounting-decision-records.md) (full findings + remediations).

## Runtime correspondence (zero behavioral change)

The native migration adds **no new runtime behavior** (verified); the existing runtime already realizes the
monad-paper concepts:

| Paper concept | Runtime artifact |
|---|---|
| Unit η(P) = {P}_∅ (cost-free fragment) | `accounting/mod.rs` `RuntimeBudget::unmetered` / s₀-collapse |
| Lazy metering (charge when forced, not exposed) | per-COMM charge in `reduce.rs` / `metering.rs` (DR-9) |
| Located purses / disjoint per-surface pools | `accounting/mod.rs` per-signature `DashMap<Sig,…>` lane pool |
| Graded transitions (step labelled by consumed signature) | `BillableTokenEvent.sig_hash` |
| Generic GSLT/OSLF funding boundary | `accounting/resource_logic.rs` `GsltPresentation`, `ResourceSignature`, `OslfResourceLogic<G>`; native specialization `RhoGslt` |
| Two monoids (spatial vs temporal) | spatial `Par` (unordered) vs temporal `SourcePath` (ordered) |

MeTTaIL is not a Rust runtime dependency in this design. When `mettail-rust` is ready, integration should be
an adapter that implements the generic `GsltPresentation`/`ResourceSignature`/`OslfResourceLogic<G>` surface
and plugs into the injected acceptance/replay entry points. The native node remains coupled to the
specification-level GSLT/OSLF interface, not to a specific MeTTaIL implementation.

## Honestly Rocq-primary (now mechanized)

Per the multi-prover allocation (DR-21 (d), and the design table): some claims are equational/logical and
Rocq-primary, with TLA+/Sage/Lean carrying genuine content only where there is operational/algebraic substance
(monad laws → Sage + Lean; located purses + modulus → TLA+). The two categorical claims that rest on the
**native translation / bisimulation** (the source-to-source erasure into pure rho) are now discharged
axiom-free in the native four-sort grammar:

- **Graded Hennessy–Milner adequacy** (graded-HML equivalence = graded bisimulation over the
  signature-labelled `graded_step`):
  - *soundness* — `CAGradedAdequacy.graded_adequacy_sound` (graded-bisimilar ⇒ same graded-HML);
  - *image-finiteness* — `CAGradedImageFinite.graded_image_finite` and `CAGradedSuccPairs.graded_image_finite_pairs`
    (the explicit finite successor enumerations `graded_succ` / `graded_succ_all`);
  - *completeness* — `CAGradedCompleteness.graded_finitary_adequacy`: at every finite modal depth `n`,
    depth-`n` graded bisimilarity ⟺ agreement on all graded-HML formulae of modal depth ≤ `n`, via the
    constructive `graded_dichotomy` (distinguishing-formula extraction). **No Classical / funext / Choice** —
    image-finiteness removes the only non-constructive obstacle.
  - *full (non-stratified) HM theorem* — `CAGradedLimit.graded_limit_adequacy`: `(∀n, graded_bisim_n n S T)
    ⟺ (∀φ, gsat S φ ↔ gsat T φ)` — approximant-limit graded bisimilarity = graded-HML equivalence, with no
    depth bound. `graded_bisim_refines_approximants` bridges the coinductive gfp (`CAGradedAdequacy.graded_bisim`)
    into the limit (`graded_bisim_implies_hml`). The one implication NOT proven — approximant-limit ⇒ coinductive
    gfp — is exactly the image-finite infinite pigeonhole (a weak omniscience principle); it is stated and assumed
    **nowhere**, so the whole stack stays axiom-free. This is the precise constructive ceiling, exhibited as theorems.
  - *force-point obstruction, proven* — `CAForceSeparation.ca_force_overgating_separation` (+ `_nonvacuous`):
    the gated translation `St (STSigned P s)` is stuck (`gated_translation_stuck`), so it is **not** strongly
    bisimilar to the dequoted-and-running source force `Pt (st_to_proc (STSigned P s))`. The "full metered
    bisimulation at force points" is thus a machine-checked **FALSE-for-the-naive-translation** result, not an
    open task; a force-faithful translation is a different (out-of-scope) translation.
- **The two adjunctions** — Free ⊣ Forget (structural) in `CAAdjunctions.v`
  (`cost_forget_install`, `cost_install_forget_alters`, naturality), and the internalisation
  ℐ_G ≡ `Imp_G : Cost(G) → G` over Turing-complete bases in `CAInternalisation.v`. The latter is the paper's
  Prop. `adj2` (*internalisation as an adjoint retraction*): `ca_internalisation_retraction` proves
  `Imp_G ∘ η_G ≈ id_G` **up to weak bisimulation** — the retraction along the cost-free unit embedding `η_G`,
  where the freely-available unit token fires the gate as an administrative reduction (so the §3a force-point
  over-gating, a property of the *full metered* translation at arbitrary grades, is not in scope of the claim).
  Axiom-free and fully general over the hash/ground encoders.

The central structural claims (wrapping, the cost monad, GAP-2 dissolution, cost determinism, the modulus) are
discharged axiom-free by `continued_gslt_cost_capstone`; the graded adequacy and both adjunctions above complete
the categorical layer (CL5–CL6) in the native grammar.
