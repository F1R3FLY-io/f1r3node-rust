# Cost-Accounting Specification Alignment Audit

> **Implementation-status correction.** This historical audit often classified
> an obligation as covered when an abstract theorem or surface recognizer
> existed. That is insufficient for consensus implementation conformance.
> [The executable conformance matrix](./cost-accounting-executable-conformance-matrix.md)
> is authoritative for completion status and requires wire, storage, runtime,
> replay, settlement, and test evidence. Native `CostSignedTerm`, `CostStack`,
> `CostAuthority`, certificate, and physical-draw representations now preserve
> the formerly erased semantics. Gap statements below are retained as the
> historical audit trail and must not be read as current implementation status.
> The papers assume the existing F1R3node architecture; their wallet, purse,
> mint, and fee notation is refined through SystemVault, RSpace, PoS, replay,
> and merge rather than implemented as parallel stores.

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

**Update (DR-23, a deeper cross-validation pass).** (1) The DR-22 "scope-boundary" items in §3.3
below are now CLOSED: the Adjunction-II / simulation-bicategory **2-truncation is dissolved in core
Lean** (axiom-free, via definitional `Prop` proof-irrelevance — NOT Mathlib/AFP, both still absent);
Tamarin + Verus now verify (8/8 provers); the Monad/Adjunction **records are instantiated**; Iris
proves the **logically-atomic** triple; Prop 6.1 not-eso gained a general image-finiteness witness.
(2) A careful re-read + two arbitration passes surfaced new, finer misalignments — most in the
DR-22/this-session additions themselves: an Adjunction-I **mislabel** (the file named for it proves
`Forget∘Free=id`, a section-retraction, not the Cost-generating resolution `cost_kleisli_adjunction`);
the Cost endofunctor/monad are mechanized on **types/setoids, not the concrete `CICat`**; Adjunction II
**omits the Turing-completeness/interpreter** content; the join rules carry a **closed-payload
restriction** narrower than Def 4.6; and the general not-eso witness uses image-finiteness, **not** the
paper's stated reasons. Full findings + remediation in **DR-23**; the rows below are annotated where a
verdict changed.

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

> **Fine-grained catalog.** This matrix is the *coarse* (per-spec-section) view. The
> **exhaustive, per-property** conformance catalog — every `CA-P-###` obligation with its spec
> source, assertion modality, covering artifact, and COVERED/PARTIAL/GAP/DEFERRED/SCOPE-BOUNDARY/
> EXCEEDS status — lives in
> [`cost-accounting-conformance-properties.md`](./cost-accounting-conformance-properties.md)
> (also mirrored as a pgmcp work-item tree, *"Cost-Accounting Conformance Property Catalog v1"*,
> linked to register root 87). Use it to assert an implementation against the *entire*
> specification.

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
| §4.6/§4.7 | **Per-actor authority keyed by the signer's public key** — the payer is stable across deploys and never keyed by the wire signature | `WalletNaming.system_vault_name_injective` | Rust `accounting::funding_sig`, `vault_payer`, and state-bound admission | A |
| Thm 1 (`tex:2060`) | Funding check decidable (linear-time) | `LinearLogicResources.funding_decidable` (decidability; linear-time is an impl property) | Rust `admit_by_funding` | A |
| Def 7 (`tex:2138`) | Conservative demand + refund | `Settlement.{debit_plus_refund_eq_reservation,refund_le_reservation}`; `evaluation_cannot_receive_refund_fuel` | Sage `settlement_model` | A (DR-5) |
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
| Thm 7.1 (`tex:763`) | `Cost` is an endofunctor on ciGSLT | **`CACostFunctor.cost_is_endofunctor`** + `CostEndofunctor` for the writer skeleton; **`CACostFunctorCI.CostCI`** + `cost_ci_preserves_step`/`cost_ci_preserves_bisim`/`cost_ci_preserves_quote_faithful` for the concrete `CICat` lift | Lean/Mathlib; Isabelle | A |
| Prop 6.1 (`tex:659`) | `U` faithful, not full, not eso | **`CAProperSubcategory.proper_subcategory`** — faithful + not-full (key-collapse, matches paper) + not-eso via two model-specific witnesses (W1 stack-inertness, W2 image-finiteness) | Lean/Mathlib; Isabelle | A-minus (**DR-23 (F)**: the not-eso witnesses are model-specific; image-finiteness is NOT the paper's stated reason (undecidable ≡ / unfactorable / non-wrappable), so it is a sufficient witness, honestly scoped, not the paper's general statement) |
| Prop 6.2 (`tex:740`) | Closure: `Cost(G) ∈ ciGSLT` | **`CACostFunctor.cost_obj_closure`** (landed DR-22) | — | A |
| Prop 9.2 (`tex:1113`) | Adjunction I (Free ⊣ Forget) | `CAAdjunctions.cost_forget_install`/`cost_install_forget_alters`; **`CAAdjunctionI.free_forget_adjunction`** | Lean/Mathlib; Isabelle | A |
| Thm 7.2 (`tex:792`) | Graded HM adequacy (sound **and** complete) | `CAGradedAdequacy.graded_adequacy_sound`; `CAGradedCompleteness.graded_finitary_adequacy`; `CAGradedLimit.graded_limit_adequacy` | mCRL2 modal-μ | A (exceeds) |
| Prop 9.3 (`tex:1143`) | Adjunction II (internalisation as adjoint retraction) | `CAInternalisation.ca_internalisation_retraction` (unit-grade retraction, cross-sort `st_tr` + real COMM); `CAAdjunctionII.internalisation_adjoint_retraction` (counit-dissolution); full bicat coherence **delivered in core Lean** (DR-23, no longer a ceiling) | mCRL2 | A-minus (coherence delivered; **DR-23 (E)**: the Turing-complete/`ciGSLTtc` interpreter conditioning is a residual — Phase 2) |
| Prop 12.1 (`tex:1434`) | Local sufficiency composes | `CALocatedPurses.local_sufficiency_composes`; `draw_disjoint` | TLA+ `LocatedPurse` | A |
| §10.4 (`tex:1285`) | Located resource stacks | `CALocatedPurses`; `ChannelSeparation.lane_pool_disjoint` | Persisted `CostAuthority`, authenticated purse inventory, physical draws, and SystemVault settlement; Sage `producer_routing` | A |
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

The 33 decision records (`docs/casper/theory/cost-accounting-decision-records.md`, DR-1…DR-33) and the pgmcp ambiguity register (root id 87) record the
judgement calls. Load-bearing examples:

- **DR-5** — runtime precharge/refund messages are removed. Admission reserves a certified finite
  upper bound without mutating supply; close-block settlement debits replay-checked realized cost.
  The §8 refund is the unused reservation retained at the **settlement boundary**, with
  `EndToEndAuthority.refund_is_unused_reservation` and
  `evaluation_cannot_receive_refund_fuel` proving the boundary and no-mid-run-refund properties.
- **DR-9** — the calculus execution unit is one token per COMM;
  reducer-operation gas remains diagnostic.
- **DR-32** — the Rust refinement linearizes that execution unit at the locked
  successful RSpace match: unmatched I/O consumes no COMM unit, binary and join
  matches each consume one, trigger side cannot affect identity, and
  reservation failure precedes all event-log and tuplespace mutation.
- **DR-47** — protocol 4 adds canonical introduction, payload-transfer, and
  committed-trace bytes to the same fixed RevVault ceiling. Consequently,
  unmatched stored I/O has positive quantitative cost even though its calculus
  COMM projection is zero.
- **DR-11/DR-13/DR-31** — per-signature linear-proof admission, state-bound dependent evidence for ambient continuations, and supply on `Σ⟦s⟧ = from_sig(s)`.
- **DR-16** — OQS removed; §4.5 G-parametricity realized by the `SignaturesAlg` trait.
- **DR-20/DR-21** — Rule-4/5 re-seal proved cost-benign (GAP-2 dissolved); native four-sort
  grammar executed; native SN conditional on the linearly-funded fragment.
- Register epics A–H cover syntax/quotation, reduction, sugar, signatures, acceptance,
  pure-rho-vs-impl, economic/supply, governance/validator — 38 resolved + 2 reclassified
  resolved after a normative re-reading.

### 3.3 (C) Still-open / scope-boundary (with the precise reason)

| Item | Why it is a boundary, not a defect |
|---|---|
| **CCS / λ / ambient / interaction-category instances** | The monad paper presents them as *foils* illustrating the general construction; only the **rho** instance is an implementation target. Out of scope by construction. |
| **Full bicategorical coherence of Adjunction II** (interchange, both triangle 2-cell equalities, associator/unitor) | **CLOSED (DR-23).** Outside Rocq's axiom-free/no-funext fragment (a setoid bicategory's 2-cell coherence needs funext/UIP), so Rocq ships the 2-truncation — but the **full** coherence is now delivered in **core Lean** (`formal/lean/CostAccountedRho/SimulationBicategory.lean`, interchange/pentagon/triangle by definitional `Prop` proof-irrelevance, `#print axioms` = none), NOT Mathlib/AFP (both absent). The result is delivered, not bounded. |
| **Coinductive graded HM completeness** (the gfp, not the approximant limit) | Provably needs the infinite-pigeonhole / fan principle, isolated as the **named** hypothesis `image_finite_stabilization` (`CAGradedLimit`); the reduction *to* it is mechanized and the principle is assumed nowhere, so the development stays axiom-free. A metamathematical ceiling, not a missing proof. |
| **Full metered-translation strong bisimulation at force points** | Proven **FALSE** for the naive translation: `CAForceSeparation.ca_force_overgating_separation` (+ `_nonvacuous`). A force-faithful translation is a different translation; neither spec asserts this bisimulation. Settled negative result. |
| **Higher-order dequotation "configurable safety margin"** (`tex:2075`) | A finite margin over a lower bound cannot prove a worst-case bound. The structural analyzer returns `DemandBound::Unprovable`; production invokes the publication's dependent-proof alternative by binding exact finite evidence to authenticated state and inputs. A GSLT/MeTTaIL producer may instead supply a checked conservative upper bound through the same generic interface. `min_phlo_price` remains economic configuration, not a semantic proof. |
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
   (`tex:2075`) is not a proof rule. DR-31 realizes the companion paper's dependent-proof
   alternative with authenticated state-bound evidence and finite capacity. The
   **canonical-form `cf` serialization** remains an implementation/backend parameter
   (DR-2/16).

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

## 8. Publications-alignment crosswalk (F-A…F-D, REV, §D2.9)

The corrections that aligned the implementation with the calculus papers, each mapped spec →
prover → Rust. All have landed.

The crosswalk was checked directly against the read-only sources under `../publications/`,
including `cost-accounting/cost-accounted-rho.tex` and `TypedCurrency/typed_value.tex`.

| Correction | Normative basis | Rocq / formal anchor | Rust anchor | Status |
|---|---|---|---|---|
| **F-A** funding vs capability: the funding `Sig` grammar is `g \| #P \| s∘s`; `⊕`/`&`/`!`/`?`/`⊸` are value/type-logic, while `Threshold` is an admission-only quorum; none is a funding former | signature grammar `g \| #P \| s∘s` | `is_funding_former` (`Sig`); `admission_sig_algebra_valid` and its soundness theorems (`CostAccountedSyntax.v`) | ingress `analyze_funding_algebra` (`casper_message.rs`); gate `is_funding_former` (`acceptance.rs`) | landed |
| **F-B** certified-bound gate: Definition 19 is `Σ ≥ Δ` for a proved finite demand; unresolved structural demand cannot use a margin over a lower bound. Production supplies dependent state-bound evidence for authenticated ambient continuations; generic GSLT evidence may supply a conservative bound. | Def 19 / Thm 20 plus the dependent-or-conservative alternatives | `EndToEndAuthority.{certified_reservation,state_bound_certificate_funds_committed_cost,exhausted_execution_cannot_be_certified}`; `StateBoundAdmission.tla` | `DemandBound::{FiniteUpperBound,Unprovable,Exact}`; `RuntimeManager::certify_state_bound_admission`; finite-capacity commit/replay | landed (DR-31) |
| **F-C / F-D** conserving FeeExtract: a flat one-token-per-admitted-deploy ownership transfer, never an additive mint | FeeExtract (`tex:3637`) plus the existing native vault architecture | `TokenConservation.fee_transfer_conserves`; `BoundedLedger.bounded_fee_transfer_conserved_or_rejected`; `AtomicVaultSettlementRefinement` | `FlatFeeApportionment`; `SystemVault.applyCost` | landed as native atomic transfer |
| **REV / phlogiston denomination mapping**: canonical persistent custody is the existing SystemVault; located stacks are prepaid linear execution authority, not a second spendable balance | the scoped papers plus the assumed F1R3node architecture and Greg's confirmation | `WalletNaming`, `MintingInjection`, `VaultBackedCostLifecycle`, `AtomicVaultSettlementRefinement`, `EndToEndAuthority` | canonical genesis vault allocations; lexical `applyCost`; authenticated stack transfers | landed |
| **Section D2.9** funding identity derives from verified signer public keys, never the wire-signature envelope | per-actor signature-indexed authority (§4.6/§4.7); `g` = a public key | `WalletNaming.system_vault_name_injective` | `accounting::funding_sig`; `vault_payer`; state-bound admission | landed (`3a4e03eb`, then native-vault refinement) |

F-A is enforced by two independent guards — the load-bearing gRPC-ingress reject and the
belt-and-suspenders funding-former gate predicate:

![F-A ingress + gate flow — the wire DeployDataProto.sig_algebra is checked at ingress: if it contains a capability connective (⊕/&/!/?/⊸) from_proto_cosigned_with_sig_algebra calls reject_capability_connectives and returns an error (the load-bearing guard). Otherwise the Cosigned is built, funding_sig is derived, and the gate asserts funding_sig.is_funding_former() (the regression lock, unreachable unless funding_sig is made non-total) before admitting. Both guards ensure no capability-formed channel ever keys a funding pool.](diagrams/f-a-ingress-gate-flow.svg)

(*Source: [`diagrams/f-a-ingress-gate-flow.puml`](diagrams/f-a-ingress-gate-flow.puml) — render with `plantuml -tsvg docs/casper/theory/diagrams/f-a-ingress-gate-flow.puml`.*)

The F-C/F-D fee is a flat, conserving transfer distinct from the balanced
execution-cost allocation of a multi-signature deploy. The native transaction
reserves both obligations together, burns only realized execution cost, refunds
unused cost reservation, and transfers the flat fee directly to the proposer.
The payer debit equals the proposer credit; neither an intermediate `F_v` ledger
nor an epoch conversion is observable. This linearizes the worked pure-rho trace
while retaining the blessed Exchange for genuine two-sided swaps. The complete
refinement and its invariants are documented in
[End-to-End Authority Settlement](cost-accounting-impl/end-to-end-authority-settlement.md).
