# W2: Cost-Aware OSLF Typing — Forward-Thinking Design (P4/P13)

Status: implemented native reference design for the finite located resource
fragment required by the two governing papers. `GsltPresentation`,
`OslfResourceLogic`, `Formula`, the exact-versus-upper-bound evidence boundary,
the signature algebra, lollipop desugaring, and consensus funding/settlement
substrate now exist. Direct replacement by MeTTaIL-generated artifacts remains
the explicit integration exception. `Pay(τ)` and type-constrained minting are
requirements of other publications and remain separate work. Use
[`../cost-accounting-executable-conformance-matrix.md`](../cost-accounting-executable-conformance-matrix.md)
and [`end-to-end-authority-settlement.md`](end-to-end-authority-settlement.md)
for current implementation status.

> Grounding mandate. The implemented core is derived from
> `cost-accounted-rho.tex` §§Static Analysis/Data-Dependent Interaction and
> `continued-gslt-cost-v2.tex` §§A type discipline for token usage/Resource
> sufficiency. Both sources were re-read against the implementation. Sections 3
> and 4 retain related-publication design context but do not enlarge this epic's
> conformance scope.

## 0. Executive summary

P4 names a two-stage pipeline: you do not run OSLF on a naked GSLT — you first run **COST**, which yields the cost-decorated GSLT, and **then** run OSLF on that, producing a type system that is **cost-aware**. P13 says the **linear** half of this is already done (the funding gate / carve / supply / settlement), and the **behavioral** half is completed once OSLF lands. This document makes the pipeline concrete against the existing Rocq development and specifies the Rust seam the future checker plugs into.

The load-bearing facts that make this design "grounded, not invented":

- The **COST arrow is built.** `CACostFunctorCI.v` defines a genuine endofunctor `CostCI : Functor CICat CICat` on the concrete ciGSLT category, whose object map `CostObj G` adjoins to each state the **accumulated spatial signature** and whose transition appends the consumed signature via the `SAnd` tensor (`CACostFunctorCI.v:31-39`). That is, literally, "cost decorating the context." `CostMonad.v` gives the grade as `grade := (sig * token)` (`CostMonad.v:28`) — authority paired with the temporal stack — and proves the monad laws (`cost_left_unit`/`cost_right_unit`/`cost_assoc`, `CostMonad.v:125-139`).
- The **OSLF-over-COST arrow has an executable finite located resource fragment.** `CAGradedTransition.v` relabels each native `ca_step` by the signature it consumes and supplies the graded Hennessy–Milner foundation. `CAOSLFSpatialModal.v` adds exact modal spend, separating located composition, linear/copyable/relevant checks, conservative-bound soundness, and local-sufficiency composition. The Rust mirror is `accounting/oslf.rs`; `resource_logic.rs` makes it available through the generic GSLT seam.
- The **linear `Δ`-side is done and proven.** `GSLTOSLFCapstone.v` assembles `OSLF_Funding_Logic_Sound` (`:104-126`): the funding judgment IS the resource inequality `Σ ≥ Δ`, it is decidable, the gate is a sound proof checker, an underfunded deploy is rejected, and the logic is **linear — no contraction** (`ll_linear_no_contraction`, `LinearLogicResources.v:324-333`). The Rust mirror is `delta_sigma.rs` (`demand_bound`/`is_funded`, including native scoped authority) + `resource_logic.rs` (`OslfResourceLogic`/`ApportionmentPolicy`).
- The **DILL dual-context judgment already exists in Rocq.** `dill : unrestricted_ctx -> linear_ctx -> ll_formula -> Prop` (`LinearLogicResources.v:139-167`) is the proven `Γ ; Δ ⊢ φ` with tensor/lolly/bang rules, and `ll_of_sig_algebra` maps the full `Sig` algebra — including `Lolly` and `Bang` — into `ll_formula` (`:23-36`). This is the skeleton of the behavioral typing's resource zone.
- The **funding/capability split is already enforced in Rust.** `Sig::is_funding_former()` (`accounting/mod.rs:1631-1642`) accepts exactly `{Unit,Ground,Quote,And}` (the funding grammar `g|#P|s∘s`) and rejects `{Threshold,Plus,With,Bang,WhyNot,Lolly}`, documenting the latter as capability/type-layer formers homed in `rho:system:capabilities`. `Sig::Lolly` is explicitly the **capability-delegation** connective (`mod.rs:1304-1309`). This is the seam type-constrained minting hangs on.

The implemented two-paper path is therefore: **(COST) `CostObj`/`CostCI`
decorates the transition system → (OSLF) the native finite formula checker reads
authenticated per-surface observations → exact evidence checks modal usage and
post-state, while conservative evidence checks only sufficiency → consensus
settlement remains the source of exact realized draws.** The checker is pure and
does not add a new wire field, byte identity, RSpace event, or accounting mode.

## 1. The P4 pipeline made concrete

P4's sentence maps onto three existing arrows:

![The P4 pipeline as a two-arrow categorical chain. A naked GSLT object is decorated by the COST endofunctor (blue; Rocq CostObj in CACostFunctorCI.v) into a cost-decorated Cost(GSLT); OSLF is then applied over the graded LTS (blue; Rocq graded_step + GForm/gsat) to yield the cost-AWARE spatial+modal+linear type system.](../diagrams/gslt-cost-pipeline.svg)

(*Source: [`diagrams/gslt-cost-pipeline.tex`](../diagrams/gslt-cost-pipeline.tex) — render with `lualatex --output-format=dvi docs/casper/theory/diagrams/gslt-cost-pipeline.tex && dvisvgm --font-format=woff --exact docs/casper/theory/diagrams/gslt-cost-pipeline.dvi -o docs/casper/theory/diagrams/gslt-cost-pipeline.svg` (or `./render.sh gslt-cost-pipeline.tex`).*)

### 1.1 Arrow 1 — COST: what "cost decorates the context" means precisely

The naked GSLT object is `Rho_ciGSLT` (`CACategory.v:129-137`): carrier `signed_term`, transition `cstep = graded_step`, equivalence `cbisim = graded_bisim`. Running COST is applying `CostObj` (`CACostFunctorCI.v:31-39`):

```coq
CostObj G := {| carrier := (carrier G * sig);
                cstep   := fun p s p' => cstep G (fst p) s (fst p') /\ snd p' = SAnd (snd p) s;
                cbisim  := fun p q => cbisim G (fst p) (fst q); ... |}
```

The carrier gains a **second component `sig`** — the accumulated spatial signature — and every transition **appends its consumed signature** to that accumulator via `SAnd` (the `∘` tensor of the signature monoid). At the grade level this is `grade = (sig * token)` (`CostMonad.v:28`): the `sig` factor is the consumed authority (commutative, up to `≡sig`), the `token` factor is the temporal stack (free, the modulus). `cost_mu` flattens nested metering by `grade_op` = `SAnd` on the authority and `tok_concat` on the stack (`CostMonad.v:30, 111-112`); metering is **non-idempotent** (`cost_monad_not_idempotent`, `:152-158`).

So "cost decorates the context" = **the typing context gains a graded resource component carrying the accumulated/consumed signature `sig`** (the authority side of the grade). This is precisely a graded/linear annotation: it is monoidal (`grade_op_unit_l/r`, `grade_op_assoc`, `CostMonad.v:53-61`), it strictly accumulates along `→`, and it is invariant under `≡` (the grade is bookkeeping that the behavioral equivalence projects away — `cbisim (CostObj G) p q := cbisim G (fst p) (fst q)`).

### 1.2 Arrow 2 — OSLF on the cost-decorated GSLT

`CostCI` (`CACostFunctorCI.v:59-65`) is a genuine functor whose morphism action `CostMor f` is, by construction, a `CIMor` — it preserves the **gated transition** (`cost_ci_preserves_step`, `:76-82`), the **behavioral equivalence** (`cost_ci_preserves_bisim`, `:69-73`), and is **quote-faithful** (`cost_ci_preserves_quote_faithful`, `:84-90`). Because `CICat`'s `cstep` is already **signature-graded** (carrier → sig → carrier → Prop, `CACategory.v:43-68`), applying OSLF to `Cost(G)` yields a logic whose modalities `⟨a⟩_s` read the grade off each step. The existing finite witness of that logic is `GForm`/`gsat` over `graded_step` (`CAGradedTransition.v:118-130`):

```coq
Inductive GForm := GTrue | GAnd .. | GNot .. | GDia (g:sig) (φ:GForm).   (* ⟨g⟩φ *)
```

`gsat S (GDia g φ)` holds iff `S` can take a `g`-graded step to a state at `φ` — the modality is **indexed by the consumed authority**. This is why running OSLF on the **cost-decorated** GSLT gives cost-limited-transition reasoning the naked GSLT cannot express: the naked `ca_step` has no grade to quantify over, so a naked-OSLF modality `⟨a⟩φ` can only say "an a-transition exists." Over `Cost(G)` the modality `⟨a⟩_s φ` says "an a-transition exists **that consumes exactly the authority `s`**", and `gsat` reads that `s` directly off the `CostObj`/`graded_step` grade. A **cost-limited** property — "every reachable transition consumes authority drawn from a bounded multiset `Σ`" — becomes a modal formula over the accumulated-signature component, decidable in the finite (token-stack-depth-bounded) fragment.

### 1.3 The native finite located realization

The general `GForm` remains the behavioral graded-HML layer. The resource
discipline required by the two governing papers is implemented as a finite
observation quotient of the Rho `K = Par` structure:

- `Formula::Spatial(φ₁, φ₂)` requires disjoint named-surface footprints and
  evaluates each side against only its own observation. Because `Par` is AC and
  the native demand pass assigns every interaction to one `SigKey` surface, the
  surface partition is the resource-relevant separating split of the term.
- `Formula::Spend { grade, continuation }` requires exact supply and demand,
  consumes the finite grade from both, and checks the continuation against the
  exact post-state.
- `Formula::{Available,Required,Sufficient}` expose the finite local purse
  facts. `Required` and `Spend` preserve `Indeterminate` when only a conservative
  bound is known; `Sufficient` may soundly use an upper bound because
  $`\Sigma_I \ge \Delta_I^{\max} \ge \Delta_I^{\mathrm{actual}}`$.
- `Formula::{linear,copyable,relevant}` encode the paper's discipline lattice:
  linear means exactly one demand plus a mandatory spend, copyable admits
  weakening and multiplicity, and relevant admits multiplicity but requires a
  spend.

`CAOSLFSpatialModal.v` proves the corresponding exactness, no-contraction,
no-weakening, locality, spatial commutativity, conservative-sufficiency, and
composition theorems without axioms. `OslfLocatedTyping.tla` checks the
concurrent two-surface transition system with TLC and Apalache; five unsafe
configurations must exhibit contraction, weakening, aliasing, false modal
evidence, and candidate-supply credit respectively. This reference realization
is what a later MeTTaIL-generated implementation must refine.

![The native located OSLF evidence boundary. Authenticated pre-state supply and a conservative external reservation can prove sufficiency because actual demand is bounded above, but Required and Spend remain indeterminate because a maximum does not prove an interaction occurred. Bounded execution and independent replay then authenticate the exact causal authority events. Only that exact evidence can consume a grade from both supply and demand and check the continuation against the exact post-spend observation. Candidate-created stacks are explicitly excluded from authenticated supply.](../diagrams/oslf-evidence-boundary.svg)

*Source: [`oslf-evidence-boundary.puml`](../diagrams/oslf-evidence-boundary.puml),
rendered with `plantuml -tsvg docs/casper/theory/diagrams/oslf-evidence-boundary.puml`.*

## 2. The cost-aware type judgments

### 2.1 Judgment form

A DILL/graded **dual-context** judgment, exactly the shape the plan's Q4 answer fixes and the shape `dill` already realizes in Rocq:

```
Γ ; Δ  ⊢  P : φ
```

- `Γ` — the **unrestricted** context (`unrestricted_ctx`, `LinearLogicResources.v:107`): replicable capabilities, admits contraction/weakening (`dill_unrestricted`, `dill_whynot_intro`, `:142-144, 166-167`). Home of `!`/`?` capabilities and the mint-authority capability (§3).
- `Δ` — the **linear** context (`linear_ctx`, `:106`): the cost/funding resources. Carries the accumulated-signature grade from `CostObj` as a multiset of `ll_formula` atoms (one atom per `Σ`-token of authority, via `ll_of_sig_algebra`, `:23-36`). **Rejects contraction** — this is the no-double-spend zone (`ll_linear_no_contraction`, `:324`).
- `φ` — the **OSLF spatial+modal formula** over the `Sig` algebra (the extended `GForm` of §1.3): the behavioral type of `P`.

The connective inventory for `Δ`/`φ` is the existing `ll_formula` (`:7-16`): `LLTensor` (∘, parallel resource), `LLLolly` (⊸, capability transformer), `LLBang`/`LLWhyNot` (!/?, the `Γ`-movable exponentials), `LLWith`/`LLPlus` (&/⊕, verifier/prover choice), `LLThreshold` (k-of-N). `dill`'s rules already give the metatheory: `dill_tensor` splits `Δ` multiplicatively (`:145-148`), `dill_lolly_elim` is resource-consuming modus ponens (`:162-165`), `dill_unrestricted` draws from `Γ` with no linear witness (`:142-144`).

### 2.2 How it relates to the already-implemented LINEAR part (the `Δ`-side that is DONE)

The `Δ`-side of `Γ ; Δ ⊢ P : φ` is **exactly the funding judgment already shipped**:

- **The funding gate `Σ ≥ Δ` is the linear-zone admissibility check.** `delta_s` (`LinearLogicResources.v:553-564`) counts the multiplicative-core layers of `Δ` (the per-`Σ` token demand); `funds n d := d ≤ n` (`:598`); decidable by `funding_decidable` (`:606`). In Rust this is `delta_sigma::demand` → `DemandEntry.certified_upper_bound` and `delta_sigma::is_funded` (`delta_sigma.rs:174, 477`). The cost-aware judgment's linear zone is **funded** iff this existing check passes. No new linear machinery is built — the behavioral typing **reuses** it as the `Δ`-discharge.
- **`FlatFee`/`Default` apportionment is the settlement of the discharged `Δ`.** When the linear zone is consumed, `compute_settlement_debits` + `ApportionmentPolicy` (`resource_logic.rs:190-329`) decide which pools pay. `DefaultApportionment` realizes Greg P8 balanced multi-sig (the matched component pair is debited equally, `:219-263`); `FlatFeeApportionment` is the flat-one-token-per-deploy fee (`:289-329`). Conservation of Authority (the contract laws, `:170-196`) is the linear-zone's "exactly `k` units consumed" invariant. The behavioral typing does not alter any of this; it sits **above** the discharged linear zone.
- **`ll_linear_no_contraction` is the no-double-spend law of the `Δ`-zone.** `GSLTOSLFCapstone.v:115-116` and `LinearLogicResources.v:324` prove a single linear atom cannot be duplicated; `competing_funding_at_most_one_succeeds` (`:764-776`) is the Remark-21 "≤1 competitor wins." This is *already* the soundness the `Δ` context needs — the behavioral layer inherits it.

In one line: **the linear `Δ`-side is `delta_sigma` + the funding capstone, DONE and mandatory; the behavioral `φ`-side is what OSLF adds on top, opt-in.** Greg P13 exactly.

## 3. Type-constrained minting (the compile-time guarantee Greg wants)

### 3.1 The two minting notions — distinguish runtime object-capability from compile-time type-constrained

- **Runtime object-capability minting (already in the model).** Authority to mint = possession of the unforgeable channel `Σ⟦s⟧` = `from_sig(s).par` (`supply.rs::supply_channel`). Only Rust `produce_balance` on a `GSysAuthToken`-bearing system deploy writes a supply datum (DR-13; `stageb-minting-halt-interface.md` Decision 1/5). This is a **runtime** capability check: you either hold the channel at reduction time or you don't. It is shipped and stays byte-identical.
- **Compile-time type-constrained minting (Greg's P4 ask, the forward piece).** A **mint judgment** that statically guarantees, at COMPILE time, that "a token of type `τ` can only be minted by a constructor satisfying behavioral contract `C`." Certain tokens then **provably cannot be minted** (no well-typed constructor produces them), or only along sanctioned paths. This is what OSLF-over-COST buys that the runtime check alone cannot: a *type-level* prohibition, checked before any deploy runs.

### 3.2 The mint judgment

Reuse the **`Sig::Lolly`** capability connective, which is already defined as exactly this — "capability delegation: presenting a `from` signature produces a `to` signature via the registered transformer process, stored on-chain in `rho:system:capabilities`" (`accounting/mod.rs:1304-1309`). The mint-authority for token type `τ` is a `Lolly`-typed capability in `Γ`:

```
mint-authority(τ, C)  :=  ⟨C⟩ ⊸ Mint(τ)            ( an ll_formula: LLLolly (φ_C) (LLAtom τ) )
```

read "consuming a witness that the constructor satisfies behavioral contract `C` yields the authority to mint one `τ`-token." The mint judgment is then a derived rule over `Γ ; Δ ⊢ P : φ`:

![The (T-Mint) typing rule. From the premises Γ ⊢ cap : ⟨C⟩ ⊸ Mint(τ) (the reusable mint authority in the unrestricted zone Γ, blue) and Γ ; Δ ⊢ K : ⟨C⟩ (the constructor K satisfies behavioral contract C), conclude Γ ; Δ ⊢ mint_K(τ) : Mint(τ) (green). A token with no mint capability in scope is provably unmintable — there is no axiom introducing Mint(τ).](../diagrams/t-mint-rule.svg)

(*Source: [`diagrams/t-mint-rule.tex`](../diagrams/t-mint-rule.tex) — render with `lualatex --output-format=dvi docs/casper/theory/diagrams/t-mint-rule.tex && dvisvgm --font-format=woff --exact docs/casper/theory/diagrams/t-mint-rule.dvi -o docs/casper/theory/diagrams/t-mint-rule.svg` (or `./render.sh t-mint-rule.tex`).*)

- The mint capability `⟨C⟩ ⊸ Mint(τ)` lives in `Γ` (unrestricted: an authority may be reused), discharged by `dill_lolly_elim` (`LinearLogicResources.v:162-165`) — the existing resource-consuming `⊸`-elimination.
- The premise `Γ ; Δ ⊢ K : ⟨C⟩` requires the **constructor `K` to satisfy the behavioral contract `C`** as an OSLF spatial+modal formula `φ_C` (the §1.3 formers). A constructor that does not exhibit the `C`-shape is not derivable, so `mint_K(τ)` is **not typeable** — the token cannot be minted.
- Tokens with **no** mint capability in scope (`⟨C⟩ ⊸ Mint(τ)` absent from `Γ`) are provably unmintable: there is no axiom introducing `Mint(τ)` (it is not a `Δ`-atom you can assume; `dill_ax` only re-proves a hypothesis already in `Δ`, `:140`). This is the "tokens with types that guarantee certain tokens will not be minted" Greg asked for.

### 3.3 Soundness statement shape

```
Theorem mint_authority_sound (target shape):
  forall P, well_typed (Γ ; Δ ⊢ P : φ) ->
    forall τ, mints P τ ->
      exists C, In (mint_cap τ C) Γ  /\  (the C-witnessing constructor is the one that minted it).
```

In words: **well-typed ⇒ only-sanctioned tokens minted** — every token type a well-typed program mints has a corresponding mint capability in scope, discharged by a constructor that provably satisfies the capability's behavioral contract. The proof is by induction on the typing derivation: the only rule introducing `Mint(τ)` is `T-Mint`, which consumes `mint_cap τ C` from `Γ` and a `⟨C⟩`-witness from the derivation. This is the compile-time analogue of, and is layered strictly above, the runtime DR-13 unforgeable-channel guarantee. Its mechanization is the principal new proof obligation (§7, R1) and depends on the OSLF spatial-formula typing existing.

## 4. The `Pay(τ)` value-type layer (P13 behavioral piece)

### 4.1 What slots in once OSLF lands

`Pay(τ)` is the **value-transfer type** from `typed_value.tex` (§sec:linearity + the typing rules, confirmed 2026-06-15: a typed payment is `Γ ; Δ ⊢ v : Pay(τ)`, tex:337/457), and per Greg P9 it is a **TYPE on the one consumable, not a second token**. It is the behavioral `φ`-side specialization of §2 for value transfer:

```
Γ ; Δ  ⊢  transfer  :  Pay(τ)
```

where `Pay(τ)` is an `ll_formula` over the value's behavioral type `τ` (the same spatial+modal type of §1.3). A transfer is well-typed iff its sender-side resource sits in the **linear** zone `Δ` (so it cannot be duplicated) and its value behaves per `τ` (the behavioral shape, checked by the OSLF formula). The two readings the plan Q4 fixes: `Δ` prevents double-spend (the linear no-contraction), `τ`/`φ` gives behavioral alignment (an unlicensed `⟨K⟩` or failed shape fails the type).

### 4.2 Composition with the one-consumable model (P9)

`Pay(τ)` introduces **no second consumable**. The single system token remains the supply unit on `Σ⟦s⟧` (`delta_sigma.rs` module doc; W1 §3.3). `Pay(τ)` is a **typing discipline over that one token**: the `Δ`-zone atom is the same `Σ`-token `delta_s` already counts (`LinearLogicResources.v:627-652`, `sig_stack`/`sigma_s`: a depth-`n` stack of one signature reflects to an `n`-fold tensor of one atom, balance = count). Adding the `Pay(τ)` type to a value does not change its `Δ_s` demand or its `Σ⟦s⟧` settlement — native signed regions compute those structurally through `demand_bound`, and exact runtime evidence settles the realized draw. So `Pay(τ)` composes with the one-consumable model by being **purely additive metadata** on the consumable, checked at compile time, settled at runtime by the unchanged linear path.

## 5. Consensus and evidence boundary

The native checker is pure, but it is not merely diagnostic. It distinguishes
the two points at which the papers require different evidence strengths:

- **Admission:** `rho_observation` reads only authenticated pre-state supply and
  `static_authority_plan.external_reservation`. Candidate-created stacks are not
  credited. A conservative bound can prove local and global sufficiency, which
  is exactly the overcharge-and-refund acceptance argument.
- **Execution/replay:** exact `authority_realized` evidence can construct
  `ResourceObservation::exact`. Only exact evidence may prove `Required` or a
  graded `Spend` and its continuation post-state. Replay authenticates that
  evidence and consensus settles it against the reservation.
- **Representation:** formulas and verdicts are local checker inputs and outputs;
  they do not change `Par`, protobuf encoding, program hashes, RSpace traces, or
  the settlement witness. There is one accounting protocol, not an inactive
  legacy mode or an A/B switch.

The generic seam is executable:

```rust
trait OslfResourceLogic<G: GsltPresentation> {
    fn resource_observation(/* canonical program, signature, supply */)
        -> Result<ResourceObservation<_>, CheckError>;
    fn check_formula(/* canonical program, signature, supply, formula */)
        -> Result<(), CheckError>;
}
```

`DefaultResourceLogic` overrides the observation projection with the native Rho
causal-authority plan. Alternative GSLTs can provide exact or bounded evidence
through the same trait without changing the formula evaluator.

## 6. Completion and scope ledger

| Piece | Status | Evidence |
|---|---|---|
| COST decoration and grade | **IMPLEMENTED** | `CACostFunctorCI.v`, `CostMonad.v` |
| Graded LTS and modal adequacy | **IMPLEMENTED** | `CAGradedTransition.v`, `CAGradedAdequacy.v`, `CAGradedCompleteness.v`, `CAGradedLimit.v` |
| DILL resource judgment | **IMPLEMENTED** | `LinearLogicResources.v` |
| Linear funding, reservation, refund, and settlement | **IMPLEMENTED** | `delta_sigma.rs`, `resource_logic.rs`, `acceptance.rs`, `GSLTOSLFCapstone.v` |
| Native finite located spatial/modal resource checker | **IMPLEMENTED** | `accounting/oslf.rs`, `CAOSLFSpatialModal.v`, `OslfLocatedTyping.tla` |
| Linear/copyable/relevant opt-in formulas | **IMPLEMENTED** | Rust example/property tests; Rocq no-weakening/no-contraction theorems; TLC/Apalache controls |
| Conservative data-dependent sufficiency | **IMPLEMENTED** | `DemandKnowledge::UpperBound`, `conservative_sufficiency_is_sound` |
| Direct MeTTaIL code generation/adapter | **EXCLUDED BY USER** | Native traits and reference semantics are the conformance target for later integration |
| `Pay(τ)` and type-constrained minting | **OUTSIDE THIS TWO-PAPER EPIC** | Design context in §§3–4; governed by other publications |

## 7. Residual integration boundary

The only in-family integration boundary is replacing or cross-checking the native
reference evaluator with MeTTaIL-generated OSLF artifacts when MeTTaIL is ready.
That adapter must preserve all three-valued verdicts, surface footprints, exact
post-spend transitions, and the authenticated-supply boundary. It must pass the
same Rust conformance suite and reproduce the Rocq/TLA+ properties before it can
participate in validation.

Image-finiteness for the generic coinductive behavioral logic is already proved
for the concrete cost-accounted Rho transition system by
`CAGradedImageFinite.v`; `CAGradedLimit.v` retains the hypothesis only at the
framework-general theorem boundary.

## Critical files

- `formal/rocq/cost_accounted_rho/theories/CACostFunctorCI.v` — the COST arrow (`CostObj`/`CostMor`/`CostCI`); the object to apply OSLF to.
- `formal/rocq/cost_accounted_rho/theories/CAGradedTransition.v` — the graded LTS + `GForm`/`gsat` behavioral modal foundation.
- `formal/rocq/cost_accounted_rho/theories/CAOSLFSpatialModal.v` — the native finite located resource semantics and proofs.
- `formal/rocq/cost_accounted_rho/theories/LinearLogicResources.v` — the `dill` dual-context judgment, `ll_of_sig_algebra`, `ll_linear_no_contraction`, `delta_s`/`funds` (the linear `Δ`-side and the home of the future `T-Mint`/`Pay(τ)` rules).
- `rholang/src/rust/interpreter/accounting/oslf.rs` — executable formulas, three-valued evidence discipline, and the native Rho observation adapter.
- `rholang/src/rust/interpreter/accounting/resource_logic.rs` — the `GsltPresentation`/`OslfResourceLogic`/`ApportionmentPolicy` trait family.
- `formal/tlaplus/cost_accounted_rho/OslfLocatedTyping.tla` — concurrent spatial/modal state machine and unsafe controls.
- `rholang/src/rust/interpreter/accounting/mod.rs` — `Sig::is_funding_former()` (the funding/capability split) and `Sig::Lolly` (the `rho:system:capabilities` mint-authority connective) that type-constrained minting is gated on.
