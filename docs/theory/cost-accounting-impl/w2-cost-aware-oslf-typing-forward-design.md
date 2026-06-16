# W2: Cost-Aware OSLF Typing — Forward-Thinking Design (P4/P13)

Status: forward design (no code). Branch: `feature/cost-accounted-rho`. NOT yet implementable (blocked on the OSLF framework); this is the additive, forward-compatible blueprint Greg requested in P4 ("we cannot implement support for this yet but should implement a forward-thinking design"). Consensus-neutral by construction: nothing here touches the live linear funding path.

> Grounding mandate. This design is a forward EXTENSION of the existing, Qed-closed formalization, not a reinvention. Each construction below is tied to a concrete Rocq object or Rust seam that was read and verified. Paper citations: `typed_value.tex` line anchors were **confirmed** (read from `publications/TypedCurrency/typed_value.tex`, 2026-06-15 — see §4/§7-R2); `continued-gslt-cost-v2.tex` is cited **by name only** and its line anchors remain **unconfirmed (S0)** (not read).

## 0. Executive summary

P4 names a two-stage pipeline: you do not run OSLF on a naked GSLT — you first run **COST**, which yields the cost-decorated GSLT, and **then** run OSLF on that, producing a type system that is **cost-aware**. P13 says the **linear** half of this is already done (the funding gate / carve / supply / settlement), and the **behavioral** half is completed once OSLF lands. This document makes the pipeline concrete against the existing Rocq development and specifies the Rust seam the future checker plugs into.

The load-bearing facts that make this design "grounded, not invented":

- The **COST arrow is built.** `CACostFunctorCI.v` defines a genuine endofunctor `CostCI : Functor CICat CICat` on the concrete ciGSLT category, whose object map `CostObj G` adjoins to each state the **accumulated spatial signature** and whose transition appends the consumed signature via the `SAnd` tensor (`CACostFunctorCI.v:31-39`). That is, literally, "cost decorating the context." `CostMonad.v` gives the grade as `grade := (sig * token)` (`CostMonad.v:28`) — authority paired with the temporal stack — and proves the monad laws (`cost_left_unit`/`cost_right_unit`/`cost_assoc`, `CostMonad.v:125-139`).
- The **OSLF-over-COST arrow has a working finite fragment.** `CAGradedTransition.v` relabels each native `ca_step` by the signature it consumes (`graded_step : signed_term -> sig -> signed_term -> Prop`, `:24-75`), faithfully (`graded_step_sound`/`graded_step_complete`, `:78-104`), and equips it with a **graded Hennessy–Milner logic** `GForm` with the graded diamond `GDia : sig -> GForm -> GForm` = `⟨g⟩φ` (`:118-130`). Its **adequacy soundness is unconditional** (`graded_adequacy_sound`, `CAGradedAdequacy.v:48-68`); completeness holds **modulo image-finiteness** (`graded_finitary_adequacy`, `CAGradedCompleteness.v:174`; `graded_limit_adequacy`, `CAGradedLimit.v:54`, where image-finiteness is a hypothesis, never an axiom). This graded HML over `Cost(G)` is the existing skeleton of the cost-aware modal type system; W2 extends its formula language with spatial formers.
- The **linear `Δ`-side is done and proven.** `GSLTOSLFCapstone.v` assembles `OSLF_Funding_Logic_Sound` (`:104-126`): the funding judgment IS the resource inequality `Σ ≥ Δ`, it is decidable, the gate is a sound proof checker, an underfunded deploy is rejected, and the logic is **linear — no contraction** (`ll_linear_no_contraction`, `LinearLogicResources.v:324-333`). The Rust mirror is `delta_sigma.rs` (`demand`/`is_funded`, the s₀ collapse) + `resource_logic.rs` (`OslfResourceLogic`/`ApportionmentPolicy`).
- The **DILL dual-context judgment already exists in Rocq.** `dill : unrestricted_ctx -> linear_ctx -> ll_formula -> Prop` (`LinearLogicResources.v:139-167`) is the proven `Γ ; Δ ⊢ φ` with tensor/lolly/bang rules, and `ll_of_sig_algebra` maps the full `Sig` algebra — including `Lolly` and `Bang` — into `ll_formula` (`:23-36`). This is the skeleton of the behavioral typing's resource zone.
- The **funding/capability split is already enforced in Rust.** `Sig::is_funding_former()` (`accounting/mod.rs:1631-1642`) accepts exactly `{Unit,Ground,Quote,And}` (the funding grammar `g|#P|s∘s`) and rejects `{Threshold,Plus,With,Bang,WhyNot,Lolly}`, documenting the latter as capability/type-layer formers homed in `rho:system:capabilities`. `Sig::Lolly` is explicitly the **capability-delegation** connective (`mod.rs:1304-1309`). This is the seam type-constrained minting hangs on.

The design is therefore: **(COST) `CostObj`/`CostCI` is the decoration → (OSLF) extend `GForm`/`dill` over `CostObj`'s accumulated signature into a spatial+modal+linear type system → the linear zone `Δ` is the already-shipped funding side; the behavioral zone `φ` is the OSLF piece; type-constrained minting is a `Lolly`-gated mint judgment whose well-typed programs provably mint only sanctioned tokens.** All of it is opt-in, compile-time, and adds zero runtime/consensus surface.

## 1. The P4 pipeline made concrete

P4's sentence maps onto three existing arrows:

```
        COST  (the cost endofunctor)            OSLF  (apply OSLF to the graded LTS)
GSLT  ───────────────────────────────▶  Cost(GSLT)  ───────────────────────────────▶  cost-AWARE
(naked)        Rocq: CostCI                 (cost-decorated)    Rocq: graded_step +        spatial+modal+linear
 CICat       CACostFunctorCI.v               CostObj Rho_ciGSLT   GForm/gsat (skeleton)      type system
```

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

### 1.3 The forward extension W2 adds to Arrow 2

The existing `GForm` is purely modal. The cost-aware **type** system (Greg's "type") needs the **spatial** OSLF formers over the `Sig` algebra. W2 extends `GForm` with the two new formers the plan §W2 names (the only genuinely new pieces):

- **Spatial constructor `K(φ₁, φ₂)`** — a process/value whose shape is the constructor `K` applied to sub-shapes `φ₁,φ₂` (the OSLF spatial connective). For the cost calculus `K` ranges over the term constructors (`STPar`, `STStack`/`TGate`, `STSigned`) read spatially.
- **Modal `⟨K⟩φ`** — after exercising the `K`-shaped capability the residual is at `φ` (the OSLF modal connective; the graded refinement is the existing `GDia g φ` with `g` the signature `K` consumes).

These are decidable on the cost GSLT by the same two finiteness sources the plan cites: **token-stack depth** (the temporal modulus `token` is finite per term) and **location** (per-`Σ⟦s⟧` surface — each signature lane is a finite, content-addressed locus). The adequacy that makes "shapes give behavioral alignment" rigorous is the graded HM theorem already proved: soundness unconditionally (`graded_adequacy_sound`), completeness modulo image-finiteness (`graded_finitary_adequacy`/`graded_limit_adequacy`). Extending it to the spatial formers is the OSLF-adequacy obligation (§6, §7).

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

- **The funding gate `Σ ≥ Δ` is the linear-zone admissibility check.** `delta_s` (`LinearLogicResources.v:553-564`) counts the multiplicative-core layers of `Δ` (the per-`Σ` token demand); `funds n d := d ≤ n` (`:598`); decidable by `funding_decidable` (`:606`). In Rust this is `delta_sigma::demand` → `DemandEntry.known_lower_bound` and `delta_sigma::is_funded` (`delta_sigma.rs:174, 477`). The cost-aware judgment's linear zone is **funded** iff this existing check passes. No new linear machinery is built — the behavioral typing **reuses** it as the `Δ`-discharge.
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

```
            Γ ⊢ cap : ⟨C⟩ ⊸ Mint(τ)        Γ ; Δ ⊢ K : ⟨C⟩
   (T-Mint) ───────────────────────────────────────────────────────
            Γ ; Δ  ⊢  mint_K(τ)  :  Mint(τ)
```

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

`Pay(τ)` introduces **no second consumable**. The single system token remains the supply unit on `Σ⟦s⟧` (`delta_sigma.rs` module doc; W1 §3.3). `Pay(τ)` is a **typing discipline over that one token**: the `Δ`-zone atom is the same `Σ`-token `delta_s` already counts (`LinearLogicResources.v:627-652`, `sig_stack`/`sigma_s`: a depth-`n` stack of one signature reflects to an `n`-fold tensor of one atom, balance = count). Adding the `Pay(τ)` type to a value does not change its `Δ_s` demand or its `Σ⟦s⟧` settlement — those are computed structurally and are type-agnostic under the s₀ collapse (`demand` does not branch on signature shape, `delta_sigma.rs:174-180`). So `Pay(τ)` composes with the one-consumable model by being **purely additive metadata** on the consumable, checked at compile time, settled at runtime by the unchanged linear path.

## 5. Forward-compatibility (the load-bearing constraint)

The design is **additive**; the current native LINEAR funding path stays **byte-identical and consensus-stable**. Concretely:

- **No runtime/consensus surface.** The behavioral checker is a **compile-time** discipline. It runs over the normalized `Par` (or the `GsltPresentation` canonical form) **before** acceptance and reduction. It emits diagnostics; it does not alter the `Par`'s bytes, the demand `Δ_s`, the supply `Σ_s`, the settlement debits, or any RSpace event. The funding gate (`acceptance.rs::admit_by_funding`), the carve/settlement (`compute_settlement_debits`), the supply writes (`produce_balance`), and replay (`replay_cost_mismatch`) are untouched. `legacy_single_sig_byte_identical` (the W1 invariant) continues to hold: a non-cost deploy that opts out of behavioral typing takes the identical path.
- **Opt-in, per-term (P13: linear mandatory now, behavioral opt-in later).** The linear `Δ`-discharge (funding gate) remains **mandatory** for every deploy — it is consensus. The behavioral `φ`-typing is **opt-in**: a deploy carries it only if it declares OSLF types (e.g. via `{% P %}[s]` annotations from W1, whose per-layer signatures are the token *types* W2 reads — plan §W2 "preserving per-layer signatures"). A deploy with no annotations is well-typed vacuously (`φ = GTrue`), so the discipline is conservative over all existing traffic.
- **The Rust seam.** The cost-aware checker plugs in as a **`DiagnosticPass` over the abstract `GsltPresentation`/`OslfResourceLogic` trait** (`resource_logic.rs:46-67`), NOT inside the reducer or the gate. The natural shape (extending the existing trait family without disturbing it):

  ```
  trait CostAwareTyping<G: GsltPresentation> {
      // pure, compile-time; reads the canonicalized program + its signature types;
      // returns diagnostics; never mutates Par, demand, or supply.
      fn check(&self, canonical: &G::CanonicalProgram, types: &SigTypes<G>) -> Vec<TypeDiagnostic>;
  }
  ```

  It consumes the **same** `canonicalize_for_funding` output the funding analyzer uses (so the type and the demand see one program), and the same `Sig`-keyed lane basis (`ResourceSignature::key` = `lane_hash`, `resource_logic.rs:87`). The plan §W2 homes this in a rholang-rs `sem` `DiagnosticPass`; **note (verified):** `sem`/`DiagnosticPass`/`consumption.rs`/`numeric_types.rs` do **not** exist in this `f1r3node-rust` tree (grep found only `rholang/tests/...numeric_eval_spec.rs`), so that home is the **rholang-rs sibling crate**, and the f1r3node side exposes only the trait above. This keeps the checker out of consensus code entirely.

## 6. What is BLOCKED vs DESIGNABLE-NOW

| Piece | Status | Evidence |
|---|---|---|
| COST decoration (`Cost(·)`, grade `(sig*token)`, "context decoration") | **EXISTS** | `CACostFunctorCI.v` (`CostObj`/`CostMor`/`CostCI`), `CostMonad.v` (`grade`, monad laws) |
| The graded LTS + graded modal logic skeleton (`⟨g⟩φ`) | **EXISTS (finite fragment)** | `CAGradedTransition.v` (`graded_step`/`GForm`/`gsat`), `CAGradedAdequacy.v` (sound), `CAGradedCompleteness.v`/`CAGradedLimit.v` (complete modulo image-finiteness) |
| The DILL dual-context `Γ ; Δ ⊢ φ` judgment + full `ll_formula` algebra | **EXISTS** | `LinearLogicResources.v:139-167` (`dill`), `:23-36` (`ll_of_sig_algebra`) |
| The linear `Δ`-side (funding gate `Σ≥Δ`, no-contraction, apportionment) | **DONE (shipped, mandatory)** | `GSLTOSLFCapstone.v` (`OSLF_Funding_Logic_Sound`), `delta_sigma.rs`, `resource_logic.rs` |
| The funding/capability `Sig` split + `Lolly` mint hook | **EXISTS** | `accounting/mod.rs:1631` (`is_funding_former`), `:1304-1309` (`Sig::Lolly` = `rho:system:capabilities`) |
| OSLF **spatial** formers `K(φ₁,φ₂)` / `⟨K⟩φ` over `Cost(G)` (the type language) | **BLOCKED** (the unbuilt OSLF piece) | plan §W2: "the two NEW formers"; no Rocq object yet |
| The behavioral `φ`-checker (the `DiagnosticPass`) | **BLOCKED** on the above + the rholang-rs `sem` home | §5; `sem` not in this tree |
| `T-Mint` + `mint_authority_sound` | **BLOCKED** on the spatial-formula typing | §3.3, §7 R1 |
| `Pay(τ)` value typing | **BLOCKED** on the above | §4 |

**Prerequisites and migration path (linear-now → behavioral-once-OSLF):**

1. **Now (independent of OSLF):** the linear path is live. The F-A funding/capability separation guards (`is_funding_former` at the gate chokepoint — already coded, committed `e55769dd`) reserve the `Sig` capability connectives for the future type layer so they can never key a funding pool.
2. **Prerequisite P1 — OSLF spatial framework:** define the spatial formers `K(φ₁,φ₂)`/`⟨K⟩φ` over the graded LTS (extend `GForm`), with their satisfaction extending `gsat`. This is the MeTTaIL/OSLF functor work explicitly out of scope of the current development (`GSLTOSLFCapstone.v:18-23`).
3. **Prerequisite P2 — OSLF adequacy for the cost constructs:** extend `graded_adequacy_sound`/`graded_finitary_adequacy` to the spatial formers (the ONE assurance theorem that makes "shapes give alignment" rigorous; plan Q8/G-section). Soundness is the unconditional half; completeness carries the image-finiteness hypothesis already isolated in `CAGradedLimit.v`.
4. **Then — behavioral checker:** implement `CostAwareTyping` as a rholang-rs `sem` `DiagnosticPass` over `GsltPresentation`/`OslfResourceLogic`; the linear `Δ`-zone delegates to the existing `delta_sigma`. Opt-in per term; advisory diagnostics first (plan DR-26: alignment from shapes, certificates optional).
5. **Then — typed minting + `Pay(τ)`:** add `T-Mint` (gated on `Sig::Lolly` capabilities) and `Pay(τ)` value typing; mechanize `mint_authority_sound`.

At every step the linear path is unchanged, so consensus never moves; the behavioral layer is strictly additive.

## 7. Risks / open questions for Greg (genuine gaps only)

1. **The exact mint-authority TYPE judgment and its soundness theorem (R1).** §3.2 proposes `mint_cap(τ,C) := ⟨C⟩ ⊸ Mint(τ)` reusing `Sig::Lolly`, with `mint_authority_sound` = "well-typed ⇒ only-sanctioned tokens minted." Open: (a) is the behavioral contract `C` a single OSLF modal formula, or a richer interface (a conjunction of `⟨K⟩` obligations)? (b) Should `Mint(τ)` be a linear (`Δ`) or unrestricted (`Γ`) conclusion — i.e. is a mint authority single-use or replicable? The `Lolly` doc says "produces a `to` signature via the registered transformer," suggesting replicable (`Γ`/`!`-wrapped), but a single-use mint license (linear) is also coherent. (c) Confirm the soundness statement shape is the intended guarantee (no stronger "constructor uniqueness" requirement).

2. **`Pay(τ)` contraction-rejection — RESOLVED (confirmed against `typed_value.tex` §sec:linearity, read 2026-06-15): SUBSUMED by `ll_linear_no_contraction`, no separate rule.** The paper places `Pay(τ)` payment witnesses in the **linear zone `Δ`** of a DILL dual-context calculus — `Γ ; Δ₁ ⊢ v : Pay(τ)` (typed_value.tex:337) — and disallows weakening + contraction on the **whole** `Δ` zone: "Weakening and contraction are admissible on `Γ` and disallowed on `Δ`" (tex:327). The no-double-spend is the GENERAL linear-zone contraction-rejection, NOT a `Pay`-specific rule: "because `v` disappears from the hypotheses on consumption, it cannot be cut a second time … the calculus does not double-spend because it has no rule that lets it" (tex:345); "a payment witness `v : Pay(τ)` consumed … literally cannot be consumed again … because contraction is rejected and no second copy of `v` exists" (tex:351). So `Pay(τ)`'s no-duplication is EXACTLY the existing `ll_linear_no_contraction` (the `Δ`-zone no-contraction, `LinearLogicResources.v:324`); the design's §4.2 assumption holds and no dedicated `Pay`-linearity rule is needed. The paper also confirms the dual-context SHAPE itself (tex:321-327, "reputation in an unrestricted regime `Γ`, value in a linear one `Δ` … the dual-context (DILL-style) sequent calculus"), matching the Rocq `dill` — the funding and value disciplines share ONE linear zone. (R2 closed; no Greg input needed.)

3. **(Secondary, flagged not blocking) Image-finiteness of the cost LTS for full completeness.** `graded_coinductive_completeness_modulo` (`CAGradedLimit.v:116`) carries image-finiteness as a hypothesis. The cost calculus's `graded_step` is plausibly image-finite (finite redex set per term), but this is not yet a discharged lemma. Adequacy **soundness** (the direction the type checker relies on for "well-typed ⇒ behaves") is unconditional, so this does not block the design — but the full "types are complete for behavior" claim needs it. Worth confirming Greg wants the completeness direction mechanized, or whether soundness-only suffices for the alignment posture.

## Critical files

- `formal/rocq/cost_accounted_rho/theories/CACostFunctorCI.v` — the COST arrow (`CostObj`/`CostMor`/`CostCI`); the object to apply OSLF to.
- `formal/rocq/cost_accounted_rho/theories/CAGradedTransition.v` — the graded LTS + `GForm`/`gsat` modal skeleton to extend with spatial formers `K(φ₁,φ₂)`/`⟨K⟩φ`.
- `formal/rocq/cost_accounted_rho/theories/LinearLogicResources.v` — the `dill` dual-context judgment, `ll_of_sig_algebra`, `ll_linear_no_contraction`, `delta_s`/`funds` (the linear `Δ`-side and the home of the future `T-Mint`/`Pay(τ)` rules).
- `rholang/src/rust/interpreter/accounting/resource_logic.rs` — the `GsltPresentation`/`OslfResourceLogic`/`ApportionmentPolicy` trait family the `CostAwareTyping` `DiagnosticPass` plugs into (the opt-in compile-time seam).
- `rholang/src/rust/interpreter/accounting/mod.rs` — `Sig::is_funding_former()` (the funding/capability split) and `Sig::Lolly` (the `rho:system:capabilities` mint-authority connective) that type-constrained minting is gated on.
