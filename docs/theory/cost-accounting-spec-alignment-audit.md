# Cost-Accounting Specification Alignment Audit

**Scope.** This document audits the alignment between the two governing cost-accounting
specifications and the repository's formal specifications (Rocq / TLA+ / Sage / Lean) and
Rust implementation. It maps every load-bearing **normative** claim of each specification to
its mechanized counterpart, classifies every ambiguity/gap into one of three buckets
(filled by the formalization/implementation · already resolved by a decision record or the
ambiguity register · still-open scope-boundary), records where the formalization **exceeds**
the specifications, and surfaces the genuine holes that remain in the specification documents
themselves.

**The two specifications (READ-ONLY law, in the parent workspace, not this repo):**
- `publications/cost-accounting/cost-accounted-rho.tex` — the concrete cost-accounted rho
  calculus (syntax, the five gated COMM rules, joins, demand/supply, the acceptance gate).
- `publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex` — "Continued Interactive
  GSLTs and the Cost Endofunctor": the cost-accounting move as an endofunctor/monad on the
  category ciGSLT, with two adjunctions, graded Hennessy–Milner adequacy, and located
  capabilities.

**Headline.** Every normative claim either is mechanized (often in a form **stronger** than the
specification states) or is a deliberately-bounded scope item with a recorded reason. An
adversarial sweep found **exactly one** previously-undocumented genuine gap — the N-ary Join
schema (`tex §4.8`, Def 4.6 / Prop 4.7) — which is being closed natively (see §6 and DR-22).

---

## 1. Label reconciliation

The repository's verification documents historically used a §-numbering that differs from the
`.tex`. This audit keys to the **tex** labels. The correspondence:

| Topic | tex label | verification-doc label (historical) |
|---|---|---|
| Cost accounting for joins (Def 4.6, Prop 4.7) | **§4.8** (`tex:965/1099/1131`) | "§4.5" |
| Substitution | §4.5 (`tex:799`) | — |
| Race-elimination via acceptance | **§2.3 + §7.5** | "§6.4" |
| Overcharge / refund | **§8.2 / §8.6** | "§6.3" |
| Deployment boundary = unit of financial atomicity | **§7.7** (`tex:2231`) | "§6.5" |
| Internalisation `Imp_G ∘ η_G ≈ id_G` (Adj II) | App A.3 / monad-paper Prop "adj2" | — |

---

## 2. Traceability matrix

`file:symbol` = a Rocq theory headline (the names gated by
`scripts/check-cost-accounted-rho-proofs.sh`); witnesses in TLA+/Sage/Lean and the Rust symbol
are named where they exist. Bucket: **A** = filled by formal/impl · **B** = resolved by a
decision record / the register · **C** = still-open scope-boundary.

### 2.1 The concrete calculus — `cost-accounted-rho.tex`

| tex § / label | Normative claim | Rocq | Other prover / Rust | Bucket |
|---|---|---|---|---|
| Def 1 (`tex:615`) | Process/name syntax | `RhoSyntax` / `CASyntax.caproc` | Rust `rholang` AST | A |
| Def 2 (`tex:637`) | Signed terms + token stacks | `CASyntax.signed_term` (`STSigned/STPar/STStack`) | — | A |
| Def 3 (`tex:670`) | Signatures parametric over backend `G` | `CostAccountedSyntax.sig`; `SignaturesAlg` trait | crypto crate | A (DR-2/16) |
| §3.7 (`tex:776`) | Structural equivalence (AC monoids) | `CAStructEquiv.ca_equiv`/`st_equiv` | — | A |
| §3.8 (`tex:799`) | Capture-avoiding subst; dequote `*@U = st_to_proc U` | `CASyntax.subst_*`/`st_to_proc` | — | A |
| Rule 1 (`tex:839`) | Single-sig whole redex | `CAReduction.ca_rule1`; `rule1_reachable` | TLA+ `CostAccountedRho` | A |
| Rule 2 (`tex:857`) | Compound sig, split tokens | `ca_rule2`; `rule2_reachable` | — | A |
| Rule 3 (`tex:876`) | Compound sig, combined token | `ca_rule3`; `rule3_reachable` | — | A |
| Rule 4 (`tex:894`) | Split processes, split tokens | `ca_rule4`; `rule4_reachable` | Sage `exchange_conservation` | A |
| Rule 5 (`tex:912`) | Split processes, combined token | `ca_rule5`; `rule5_reachable` | — | A |
| §4.8 J1/J2 (`tex:995/1021`) | **N-ary join (Def 4.6)** | **`ca_join1`/`ca_join2`** (Component 2, landed DR-22; full SN/confluence/determinism/graded/translation metatheory) | Apalache symbolic-N | A |
| Prop 4.7 (`tex:1131`) | **Conservation of authority across partitions** | **`CAJoinConservation.join_authority_conserved`** (landed) | Why3 cross-witness | A |
| §4.8.4 (`tex:1149`) | Reverse-currying (Join/Split) regrouping | `Split`/`Join` mediators; `CAJoinConservation.reverse_curry_iso` (landed) | TLA+ token-conservation | A |
| §4.8.5 (`tex:1175`) | No-weakening of composite tokens | `LinearLogicResources.ll_linear_no_weakening`; `CAJoinConservation.join_no_weakening` (landed) | — | A |
| §4.6 (`tex:1251/1273`) | Uniform-signing + linear-transfer `⊸` sugar | `SyntacticSugar.uniform_sugar_translation_equiv`, `lollipop_sugar_translation_equiv` | — | A |
| §4.6 Rmk (`tex:1208`) | `⊸` is `∘`'s right adjoint (tensor–hom) — *"left to the sequel"* | **`LLIdentities.lolly_curry_isomorphism`** | — | A (exceeds) |
| Def 4–6 (`tex:2002`) | Token demand Δ, supply Σ, funding obligation Σ≥Δ | `LinearLogicResources.{delta_s,sigma_s,funding_check_balance_sound}` | Sage `budget_admission_model` | A |
| Thm 1 (`tex:2060`) | Funding check decidable (linear-time) | `LinearLogicResources.funding_decidable` (decidability; linear-time is an impl property) | Rust `admit_by_funding` | A |
| Def 7 (`tex:2138`) | Conservative demand + refund | `Settlement.{charged_plus_refund_eq_escrow,refund_le_escrow}`; `evaluation_cannot_receive_refund_fuel` | Sage `settlement_model` | A (DR-5) |
| §7.7 (`tex:2231`) | Deployment boundary = unit of financial atomicity | `LinearLogicResources.admit_prefix_maximal`/`reject_both_sound` | TLA+ D2 acceptance; Rust `admit_by_funding` | A (DR-11/13) |
| Rmk DB-atomicity (`tex:1805`) | Rules fire all-or-nothing | `WrappingSubjectReduction.no_leak_requires_token`; `ca_step_needs_fuel` | TLA+ invariants | A |
| App A.3 (`tex:2975`) | `Imp_G ∘ η_G ≈ id_G` up to weak bisim | `CAInternalisation.ca_internalisation_retraction` | mCRL2 weak-bisim | A |
| (not claimed) | Token conservation | `CATokenConservation.st_token_count_subst_invariant` | Sage; TLA+ | A (exceeds) |
| (not claimed) | Strong normalization | `CAStrongNormalization.ca_SN_funded` (funded fragment) | TTT2 | A (exceeds) |
| (not claimed) | Confluence | `CAConfluence.ca_local_confluence` | CSI | A (exceeds) |
| (not claimed) | Cost determinism | `CACostDeterminism.ca_cost_deterministic_funded` | TLA+; Sage | A (exceeds) |

### 2.2 The monad/endofunctor — `continued-gslt-cost-v2.tex`

| tex § / label | Normative claim | Rocq | Other prover | Bucket |
|---|---|---|---|---|
| §3.1 (`tex:376`) | Wrapping by construction (`𝕋` sort) | `CASyntax.signed_term` (native four-sort) | — | A |
| Lem 3.1 (`tex:461`) | Subject reduction for wrapping | `WrappingSubjectReduction.subject_reduction_wrapping` | TLA+ `WrappedSubjectReduction` | A |
| Cor 3.1 (`tex:475`) | No leak by construction | `no_leak_requires_token`/`no_leak_stack_inert` | Tamarin (security view) | A |
| Prop 4.1 (`tex:530`) | Stack consumption is the modulus | `FuelEventDecomposition.consumed_fuel_count_eq_token_drop` | — | A |
| §5 Def 5.1 (`tex:610`) | Section `cf`; `hashf = digest∘cf` | Rocq `hash_process`/`ground_process` Section hypotheses | — | A (abstracted) |
| Construction 9.1 (`tex:1036`) | Unit η, multiplication μ | `CostMonad.{cost_eta,cost_mu}` | Sage `cost_monad_laws`; Lean | A |
| Prop 9.1 (`tex:1064`) | Monad laws | `CostMonad.{cost_left_unit,cost_right_unit,cost_assoc}` | Sage; Lean `CostMonad` | A |
| Rmk 9.1 (`tex:1086`) | Non-idempotent monad | `CostMonad.cost_monad_not_idempotent` | Sage (`mu_non_injective`) | A |
| Thm 7.1 (`tex:763`) | `Cost` is an endofunctor on ciGSLT | **`CACostFunctor.cost_is_endofunctor`** + the `CostEndofunctor` `Functor` record (Component 3, landed DR-22) | Lean/Mathlib; Isabelle | A |
| Prop 6.1 (`tex:659`) | `U` faithful, not full, not eso | **`CAProperSubcategory.proper_subcategory`** (faithful + not-full + not-eso bounded, landed DR-22) | Lean/Mathlib; Isabelle | A |
| Prop 6.2 (`tex:740`) | Closure: `Cost(G) ∈ ciGSLT` | **`CACostFunctor.cost_obj_closure`** (landed DR-22) | — | A |
| Prop 9.2 (`tex:1113`) | Adjunction I (Free ⊣ Forget) | `CAAdjunctions.cost_forget_install`/`cost_install_forget_alters`; **`CAAdjunctionI.free_forget_adjunction`** | Lean/Mathlib; Isabelle | A |
| Thm 7.2 (`tex:792`) | Graded HM adequacy (sound **and** complete) | `CAGradedAdequacy.graded_adequacy_sound`; `CAGradedCompleteness.graded_finitary_adequacy`; `CAGradedLimit.graded_limit_adequacy` | mCRL2 modal-μ | A (exceeds) |
| Prop 9.3 (`tex:1143`) | Adjunction II (internalisation as adjoint retraction) | `CAInternalisation.ca_internalisation_retraction` (retraction); **`CAAdjunctionII.internalisation_adjoint_retraction`** (counit-dissolution, intra-carrier, landed DR-22); full bicat coherence (2-truncation ceiling) → Lean/Mathlib + Isabelle | mCRL2 | A (Rocq 2-truncation; coherence routed, §3.3, §5) |
| Prop 12.1 (`tex:1434`) | Local sufficiency composes | `CALocatedPurses.local_sufficiency_composes`; `draw_disjoint` | TLA+ `LocatedPurse` | A |
| §10.4 (`tex:1285`) | Located resource stacks | `CALocatedPurses`; `ChannelSeparation.lane_pool_disjoint` | Rust `Lane`/`DashMap` lanes; Sage `producer_routing` | A |
| §11.2 (`tex:1337`) | Spatial/modal type connectives (linear/copyable/relevant) | `LLIdentities.{bang_weakening_admissible,whynot_weakening_admissible}`; `CATypeDiscipline.ca_linear_no_contraction` | — | A |

---

## 3. Three-bucket classification of ambiguities/gaps

### 3.1 (A) Filled by the formalization / implementation

| Spec ambiguity/gap | How it is filled |
|---|---|
| Thm 7.2 graded adequacy stated **"schematically"** (completeness direction, constructivity unspecified) | Made fully constructive: `graded_finitary_adequacy` (depth-stratified) + `graded_limit_adequacy` (non-stratified) + `graded_coinductive_completeness_modulo` (gfp modulo the **named** principle `image_finite_stabilization`), all axiom-free — no Classical/funext/Choice. |
| `⊸` tensor–hom adjunction **"left to the sequel"** (`tex:1208`) | Proven: `LLIdentities.lolly_curry_isomorphism`. |
| μ stack-concatenation **order** underspecified (`tex:1051`) | Pinned by `CostMonad.cost_mu` (token-stack concat order is fixed and the laws hold up to `cost_equiv`). |
| Collision-resistance of `hashf` (empirical/cryptographic, unspecified) | Abstracted as Rocq Section hypotheses (`hash_process_injective`, `ground_hash_disjoint`) — every translation theorem is parametric over any collision-resistant encoder. |
| "Section `cf` computable" (computability sense unspecified) | Abstracted to the encoder Section hypotheses; the canonical form is the implementation's `cf` (DR-2/16). |
| Weakening discipline (spatial forbids, temporal admits) informally stated | `ll_linear_no_weakening` (spatial) vs `bang_weakening_admissible`/`whynot_weakening_admissible` (the `!`/`?` modalities) — both directions mechanized. |
| Subject reduction (Lem 3.1) given only as a sketch | `subject_reduction_wrapping`, axiom-free. |
| Funding decidability (Thm 1) — proof sketch | `funding_decidable` + `funding_check_balance_sound`. |
| Conservation across join partitions (Prop 4.7) core | `CAJoinConservation.join_authority_conserved` (landed DR-22, up to `Permutation`); the regrouping-invariance core is `MergeableChannelAccounting.mergeable_channel_bitmask_fold_permutation`. |

### 3.2 (B) Resolved via a decision record or the ambiguity register

The 21 decision records (`docs/theory/cost-accounting-decision-records.md`, DR-1…DR-21, plus
DR-22 from this audit) and the 38-entry pgmcp ambiguity register (root id 87) record the
judgement calls. Load-bearing examples:

- **DR-5** — runtime precharge/refund removed; deploys draw directly from `Σ⟦s⟧`. The §8 refund
  identity is retained only at the **settlement boundary** (`Settlement.refund_*`), with
  `evaluation_cannot_receive_refund_fuel` as the soundness witness of the removal. *Not a
  contradiction* — DR-5 removes the runtime mechanism; the boundary identity is the residual.
- **DR-9** — cost unit = one token per COMM; per-operation gas is diagnostic only.
- **DR-11/DR-13** — per-signature static linear-proof acceptance gate; supply on `Σ⟦s⟧ = from_sig(s)`.
- **DR-16** — OQS removed; §4.5 G-parametricity realized by the `SignaturesAlg` trait.
- **DR-20/DR-21** — Rule-4/5 re-seal proved cost-benign (GAP-2 dissolved); native four-sort
  grammar executed; native SN conditional on the linearly-funded fragment.
- Register epics A–H cover syntax/quotation, reduction, sugar, signatures, acceptance,
  pure-rho-vs-impl, economic/supply, governance/validator — 36 resolved + 2 reclassified
  resolved after a normative re-reading.

### 3.3 (C) Still-open / scope-boundary (with the precise reason)

| Item | Why it is a boundary, not a defect |
|---|---|
| **CCS / λ / ambient / interaction-category instances** | The monad paper presents them as *foils* illustrating the general construction; only the **rho** instance is an implementation target. Out of scope by construction. |
| **Full bicategorical coherence of Adjunction II** (interchange, both triangle 2-cell equalities, associator/unitor) | Outside Rocq's axiom-free/no-funext fragment (a setoid bicategory's 2-cell coherence needs funext/UIP). Rocq ships the **2-categorical truncation** (Prop-valued `weak_match` 2-cells); the **full** coherence is completed classically in Lean/Mathlib + Isabelle/AFP (§5). So the *result* is not bounded — only the *Rocq* realization is. |
| **Coinductive graded HM completeness** (the gfp, not the approximant limit) | Provably needs the infinite-pigeonhole / fan principle, isolated as the **named** hypothesis `image_finite_stabilization` (`CAGradedLimit`); the reduction *to* it is mechanized and the principle is assumed nowhere, so the development stays axiom-free. A metamathematical ceiling, not a missing proof. |
| **Full metered-translation strong bisimulation at force points** | Proven **FALSE** for the naive translation: `CAForceSeparation.ca_force_overgating_separation` (+ `_nonvacuous`). A force-faithful translation is a different translation; neither spec asserts this bisimulation. Settled negative result. |
| **Higher-order dequotation "configurable safety margin"** (`tex:2075`) | Spec-delegated to the implementation; pinned to `min_phlo_price` by DR-11. Genuinely a deployment parameter. |
| **Canonical-form `cf` serialization format** | Spec-silent (DR-2/16); an implementation/crypto-backend choice, modeled abstractly in Rocq. |

---

## 4. Where the formalization exceeds the specifications

1. **Graded HM adequacy is constructive**, where the monad paper states Thm 7.2 only
   "schematically": both directions, image-finite, no Classical/funext/Choice
   (`graded_finitary_adequacy`, `graded_limit_adequacy`), with the lone non-constructive step
   isolated as a named, assumed-nowhere principle.
2. **The `⊸` tensor–hom adjunction**, which the rho paper explicitly leaves "to the sequel",
   is proven (`lolly_curry_isomorphism`).
3. **Strong normalization, confluence, and cost determinism** are mechanized on the funded
   fragment, none of which the rho paper even *claims* (`ca_SN_funded`, `ca_local_confluence`,
   `ca_cost_deterministic_funded`), with the off-fragment counterexample also recorded
   (`st_total_fuel_can_increase_off_funded`).
4. **The force-point obstruction is a proven theorem**, not a remark: the naive translation
   over-gates at forces and is provably not bisimilar to the running stripped form
   (`ca_force_overgating_separation`, with a concrete non-vacuous witness).
5. **The implementation is verified, not merely corresponded**: Verus/Creusot prove the Rust
   `accounting/` runtime's budget-conservation and canonical determinism (Component 5),
   upgrading the prior correspondence-only alignment.

---

## 5. Spec-document holes still unaddressed (in the `.tex` themselves)

These are genuine under-specifications *in the source documents*; the formalization either
cannot close them (they are cryptographic/implementation parameters) or completes them in a
prover the document's own foundations do not fix:

1. **§4.8 joins were given only as an informal display + a one-paragraph Prop 4.7 assertion**,
   with no proof and no per-rule operational treatment — the source of the one genuine gap
   (§6). The native mechanization (Component 2) supplies the missing reduction rules and the
   conservation proof.
2. **The `⊸` tensor–hom adjunction and the §4.8.4 currying iso are stated as "to the sequel"**
   in the rho paper — closed here by `lolly_curry_isomorphism` / `reverse_curry_iso`.
3. **Adjunction II's full bicategorical coherence** (counit + both triangle identities as
   2-cells) is asserted "up to the 2-cells witnessing these weak bisimulations" without a
   proof — completed classically in Lean/Mathlib + Isabelle/AFP (§Component 5); Rocq's
   axiom-free fragment admits only the 2-truncation.
4. **Hash collision-resistance** is assumed but never specified as a cryptographic assumption —
   carried as Rocq Section hypotheses; a backend obligation (DR-16).
5. **The "configurable safety margin"** for higher-order dequotation over-approximation
   (`tex:2075`) and the **canonical-form `cf` serialization** are spec-delegated to the
   implementation; recorded as deployment/backend parameters (DR-11, DR-2/16).

---

## 6. The one genuine gap — CLOSED (DR-22)

The adversarial sweep found that the **N-ary Join schema** (`tex §4.8`: Def 4.6's
`for(y₁←x₁ & … & yₙ←xₙ){P}` with cases J1/J2, and Prop 4.7's conservation of authority across
all token-presentation partitions) had **no** mechanized counterpart: `ca_step` defined only
the five *binary* COMM rules; there was no join former, no join desugaring, and Prop 4.7 was
discharged by no theorem (the nearby binary `Split`/`Join` mediators, the `Exchange` N=2 join,
and `mergeable_channel_bitmask_fold_permutation` each prove a strict sub-property). It was not
covered by any decision record or register entry — a true gap, not a documented divergence.

It has been closed **natively** (Tier-2, landed in DR-22): a `CPJoin` grammar former + `ca_join1`/
`ca_join2` reduction rules (the N-ary analogues of Rules 1 and 3, with closed-payload premises for
capture-correct N-simultaneous substitution), the full metatheory re-proof (binding, token
conservation, funded SN via the keystone `linear_subst_many_fuel_le`, confluence + per-rule
determinism, the graded transition + its image-finiteness enumeration, translation progress), and
`CAJoinConservation` proving Prop 4.7 + the reverse-currying iso + the no-weakening corollary — all
axiom-free, all behind the LOCAL-ONLY Rocq gate (every headline "Closed under the global context").
DR-22 records the finding, the realization, the closed-payload design crux, and the §4.8 label
correction.

---

## 7. Verification map

All gates are LOCAL-ONLY (never `.github/workflows`):
- Rocq — `scripts/check-cost-accounted-rho-proofs.sh` (compile + `rocqchk` + axiom-free
  `Print Assumptions` self-count + banned-word scan).
- TLA+/TLC + Apalache — `scripts/check-cost-accounted-rho-tla-invariants.sh`.
- Sage — `scripts/check-cost-accounted-rho-sage.sh`.
- Lean (validator, Mathlib-free) + the separate Mathlib CT project — `scripts/check-cost-accounted-rho-lean.sh`.
- The multi-prover arsenal (each fail-soft) — `scripts/check-cost-accounted-rho-{mcrl2,tamarin,rewriting,why3,verus,isabelle,iris}.sh`, with `scripts/check-cost-accounted-rho-ALL.sh` reporting a per-prover pass/skip matrix.

> Buckets marked **C→A** in §2 move to A as Components 2/3/5 land; this document is updated to
> match, and the per-prover witnesses are filled into the matrix as each gate goes green.
