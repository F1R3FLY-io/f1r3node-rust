# Native Translation Faithfulness & Bisimulation — Proof Design (Stage 4b)

**Status:** in progress. Foundation (`CATranslation.v`) committed (1a958972). This
document records the rigorous design (arbitrated by a Plan agent against the old
4379-line `TranslationFaithfulness.v`) so the development is reconstructable, and
tracks progress lemma-by-lemma.

## 0. What the old proof actually proves (and the trap)

`translation_faithful` (`TranslationFaithfulness.v:2459`) is **deliberately weak**:
`∀ S S', ca_step S S' → ∃ Ctx W, closed_proc Ctx ∧ rho_reachable (PPar (Sy S) Ctx) W`
— the successor `S'` appears only in the hypothesis; `W` is **never** required to
relate to `Sy S'` (stated at `:2452`). The real per-step content is in the per-rule
lemmas (e.g. `rule1_simulation_unit :545`), whose bodies are **bare procs** — so the
old proof **never** had to commute `S_tr` with substitution. Backward reflection uses
a **synthetic** `recursive_metered_gate` (`:4264`) that sidesteps de Bruijn entirely.
`Bisimulation.translation_strong_bisimilar_generic :1250` is a **static single-gate**
bisimilarity (fire one gate, residue ~ body), not a bisimulation across a `ca_step`.

## 1. The two genuine obstacles (native-only)

1. **Gate-shift.** `st_tr (STSigned P s)` inserts 1 (atomic) / 2 (SAnd) `PInput`
   binders around `lift_proc {1,2} 0 (p_tr P)` (`CATranslation.v:77`). A source
   `CNVar n` inside `P` maps to a target index shifted by the cumulative gate-binder
   count. So `st_tr (subst_st T 0 (CQuote U)) = subst_proc (st_tr T) 0 (Quote (st_tr U))`
   is **false** at a fixed index 0.
2. **Dequote-collapse.** `subst_caproc (CPDeref (CNVar 0)) 0 (CQuote U) = st_to_proc U`
   (`CASyntax.v:133`) — gates **stripped**; but the rho side
   `subst_proc (PDeref (NVar 0)) 0 (Quote (st_tr U)) = st_tr U` — gates **intact**.
   `p_tr (st_to_proc U) ≠ st_tr U` syntactically. A strict syntactic simulation is
   **genuinely false**; no lemma recovers it without an axiom.

## 2. Resolution

- **(A) Depth-indexed translation `st_tr_d : nat → signed_term → proc`.** Record the
  ambient gate depth `d` in the index function (`caname_tr_d d (CNVar k) = NVar (k+d)`),
  incrementing by gate arity at each `STSigned` and by 1 under each `CPInput`. Bridge:
  `st_tr_d d P = lift_proc d 0 (st_tr_d 0 P)`, so `st_tr = st_tr_d 0` stays the public
  definition and `st_tr_d` is an internal proof device.
- **(B) Behavioural target.** State forward simulation **up to `bisim`**, not `=`:
  `∃ W, rho_reachable (tr S) W ∧ bisim W (tr S')`. The dequote gap is closed by L4:
  the unfired gate `st_tr U` is a lone receiver (`PInput_alone_stuck`, `RhoReduction:331`)
  whose authorizing token was consumed by the same COMM — so it is bisimilar to its
  stripped body `p_tr (st_to_proc U)`.

## 3. Module / lemma plan (dependency order) + progress

### `CATranslationLemmas.v`  (Section over hash/ground hyps)
- [x] **L0** `lift_zero_ca` — reuse `CASyntax.lift_zero_*` (no-op; verified present).
- [x] **L2** `st_to_proc` commutes with lift/subst (low, ~30 lines).
- [x] native `N_tr`/`T_tr` subst & lift invariance (4 one-liners; closed images).
- [x] **L1** native lift/subst commutation (mutual via `ca_mutind`; medium, ~120 lines).
- [x] **L5** gate-unwrap — the **atomic** case is done natively by `ca_single_gate_bisimilar`
  (CABisimulation: the gate fires and the unit-token residue is bisimilar to `p_tr P` via
  `multi_stuck_residue_bisim`). The **SAnd / nested** case is exactly the force-point case
  now settled as a proven separation (`ca_force_overgating_separation`, CAForceSeparation) —
  see the reconciliation note at the end of this section.
- [x] **SUPERSEDED** — `se_implies_bisim`, `bisim_par_l`/`bisim_par_r` were the PPar congruences
  for the full forward-bisimulation **Thm A**, which `ca_force_overgating_separation` proves is
  FALSE at force points; they were therefore never needed and never built (the achievable
  bisimulation, `ca_single_gate_bisimilar`, requires no PPar congruence).

### `CATranslationFaithfulness.v`  (Section)  — **Section + foundation committed (89737ab4)**
- [x] Section over the audited hash/ground hypotheses; Local Notations Nt/Tt/Pt/Ct/St.
- [x] in-Section closedness (Nt_closed/Tt_closed) + lift/subst invariance
  (Nt_lift_inv/Nt_subst_inv/Tt_lift_inv/Tt_subst_inv) — closed images inert under COMM.
- [x] **Proc-level lift/lift composition — DONE:** `lift_lift_compose_proc` /
  `lift_proc_S_compose` / `lift_lift_comm_proc` (CATranslationLemmas.v:184/212/223, committed
  cbe527d6/54701083); the depth-indexed bridge `trd_bridge` is built on them.
- [x] **st_trd (d,c)** : depth-indexed mutual translation (`p_trd`/`cn_trd`/`st_trd`,
  threading a lift at cutoff c) + **bridge** `st_trd d c X = lift_proc d c (St X)`
  (`trd_bridge`, via `lift_lift_comm_proc` at gates) + `st_trd_zero` (`St = st_trd 0 0`).
  Committed 54701083. Also: `lift_lift_comm_proc`, `lift_lift_compose_proc`,
  `lift_proc_S_compose` (committed cbe527d6/54701083).
- [x] **rule1 (atomic SUnit) operational reachability — DONE (rule1_unit_reachable):**
  `St(rule1 LHS) ⇝* PPar (subst_proc (St T) 0 (Quote (St U))) (Tt t)` in two COMMs.
  **Corrected operational fact:** `subst_proc` performs SEMANTIC dereferencing —
  `subst_proc (PDeref (NVar 0)) 0 (Quote Q) = Q` (NOT a stuck `PDeref(Quote Q)`; the
  earlier "stuck residue" note was wrong). So the gate's `*t` releases the stack tail
  `Tt t` **live**, and the per-rule result's token part matches `St(RHS)` **exactly**
  (`gate_body_subst`). The token-handling worry is dissolved.
- [x] **RESOLVED as a proven separation.** The residual payload-body gap at `*x`-force
  positions — `subst_proc (St T) 0 (Quote (St U))` exposes `lift_proc 1 0 (St U)` (gated)
  whereas `St (subst_st T 0 (CQuote U))` exposes `lift_proc 1 0 (Pt (st_to_proc U))`
  (gates **stripped** by `st_to_proc`) — was conjectured to close via an L4 dequote-collapse
  bisimilarity. It does NOT: `ca_force_overgating_separation` (+ `ca_force_overgating_nonvacuous`,
  CAForceSeparation) proves `St U` and `Pt (st_to_proc U)` are not bisimilar when the stripped
  form runs. So the per-rule match is exact away from forces, and at forces it is a settled
  NEGATIVE result (the naive translation over-gates), not an open bisimilarity. See §3a.
- [x] **ALL FIVE per-rule operational simulations — DONE** (rule1_reachable [general
  atomic, 2 COMMs], rule2_reachable [nested gate, split tokens, 3 COMMs, no side condition],
  rule3_reachable [combined token via Split, 4 COMMs], rule4_reachable [split processes +
  combined token via Split, atomic sigs, 4 COMMs], rule5_reachable [fully split, atomic
  sigs, 3 COMMs]). Committed 39662344 / 78b57310 / 8ce62196 / 91023f42 / 27eb1381. Plus the
  native Split mediator (Split / Split_closed / Split_fires, 4322d607), the gate-firing
  substitution helpers (gate_body_subst / nested_gate_subst / gate2_body_subst /
  split_body_subst), and the reachability congruences rho_reachable_par_l/r.
  **Token-handling is faithful** (subst's semantic deref releases stack tails live); the
  ONLY residual is the payload dequote-collapse, for the bisimulation layer.
- [x] **General ∀-ca_step assembly — DONE:** `ca_translation_progresses`
  (CATranslationFaithfulness.v:458): `∀ S S', ca_step S S' → ∃ Ctx W, closed_proc Ctx ∧
  rho_step (PPar (St S) Ctx) W` — one-step rho progress in a closed context, by induction over
  ca_step's leaves via the five per-rule lemmas (Ctx=PNil for 1/2/5, Ctx=Split for 3/4) plus
  the `ca_par_l/r` congruence. Stated as one-step progress to stay non-vacuous, exactly as the
  plan prescribed.
- [x] **L4 dequote-collapse bisim — SUPERSEDED (proven false in general).** A GENERAL collapse
  bisimilarity would close the force gap, but `ca_force_overgating_separation` proves it does
  NOT hold at force positions. Its restricted, TRUE instance — the unit-token residue — is
  `multi_stuck_residue_bisim` (Bisimulation.v:1099), used inside `ca_single_gate_bisimilar`.
- [x] **`rule1..5` per-rule simulations (Thm B) — SUPERSEDED by the reachability + single-gate
  split.** The per-rule operational REACHABILITY is done (`rule1..5_reachable`, checked above);
  upgrading each to a strong bisimulation requires L4, which is false at forces. The achievable
  per-rule bisimulation is the single-gate `ca_single_gate_bisimilar`.
- [x] **`ca_par_l`/`ca_par_r`** — present as `ca_step` constructors (CAReduction.v:74-75) and
  graded `g_par_l`/`g_par_r` (CAGradedTransition.v:53-55); the reachability congruences are
  `rho_reachable_par_l`/`rho_reachable_par_r` (CATranslationFaithfulness.v:30/38).
- [x] **Thm A `ca_translation_forward` — SUPERSEDED by a sharper, PROVEN pair.** The full
  `∀ S S', ca_step S S' → ∃ W, rho_reachable (tr S) W ∧ bisim W (tr S')` is FALSE at force points
  (`ca_force_overgating_separation`). It splits into two proven halves: the operational half
  `ca_translation_progresses` (every step makes real rho progress) and the bisimulation half
  `ca_single_gate_bisimilar` (single-gate residue bisimilarity) — with the force-point obstruction
  settled as the separation rather than papered over by a (nonexistent) L4 collapse.

**Progress note (this session):** the entire `CATranslationLemmas` module (L1/L2/
lift_lift_comm) and the `CATranslationFaithfulness` Section + invariance foundation are
committed and gate-green. The remaining items above form a layered development (proc
lift/lift → st_tr_d + bridge → L3 → L4 → per-rule → Thm A → Thm C) whose crux (L3) is the
genuinely-novel research step; each layer must compile axiom-free before the next, so it
proceeds as a sequence of gate-green checkpoints, not one landing.

> **Checkbox reconciliation (final).** The boxes above were the original plan for a *full
> strong forward bisimulation* (Thm A) built layer-by-layer through an L4 dequote-collapse.
> The development reached a sharper, honest end state instead, and every box above is now
> resolved — none is an open task:
> - **Done under final names:** proc lift/lift composition (`lift_lift_comm_proc` &c.); the
>   ∀-ca_step assembly (`ca_translation_progresses`); the five per-rule reachabilities
>   (`rule1..5_reachable`); the single-gate bisimulation (`ca_single_gate_bisimilar`);
>   `ca_par_l/r` + `rho_reachable_par_l/r`.
> - **Superseded — proven impossible, not skipped:** the GENERAL L4 collapse, the per-rule
>   *bisimulations* (Thm B), Thm A itself, and their PPar-congruence sub-lemmas. The force-point
>   collapse they relied on is **false**, now the machine-checked `ca_force_overgating_separation`
>   (CAForceSeparation) + `ca_force_overgating_nonvacuous`. So Thm A's achievable content is the
>   PROVEN pair `ca_translation_progresses` (operational) + `ca_single_gate_bisimilar`
>   (bisimulation), and the gap is a settled negative result (§3a), not an L4 to be built.
> - **L3** (the depth-indexed bridge crux) is `trd_bridge`/`st_trd`, committed (checked above).
>
> Net: the layered plan terminated in the §3a separation rather than in Thm A; the unchecked
> boxes are stale planning state, reconciled here against the committed, gate-green proofs.

### `CABisimulation.v`  (Section) — **committed f7291af9**
- [x] `ca_single_gate_bisimilar` — the native analogue of the old
  `translation_strong_bisimilar_generic`: firing `{P}_s` (atomic s) against a unit
  token reaches a residue strongly bisimilar (`Bisimulation.bisim`) to the released
  body `p_tr P`, via `multi_stuck_residue_bisim` on the inert unit-token residue.
  **This is the strong bisimulation that holds cleanly** — same strength as the
  OLD model's strong-bisim result (the old model NEVER proved a full-ca_step strong
  bisim either; its `translation_faithful` is weak and its bisim is single-gate).
- [N/A — research limit] A full-ca_step strong bisimulation `W ~ St S'` across an
  arbitrary multi-COMM step is **force-limited** (§3a): the native translation
  over-gates at force positions, so `St U` (gated, stuck) ≁ `Pt(st_to_proc U)`
  (stripped). This is a documented discovery, NOT an unfinished mechanical task;
  it requires a translation refinement (force cashes the signature). The native
  faithfulness delivered (the 5 per-rule reductions + `ca_translation_progresses`
  + `ca_single_gate_bisimilar`) **matches and exceeds** the old model's complete
  faithfulness guarantee.

## 3a. The strong-bisimulation limit — a genuine semantic finding (force points)

The progress theorem `ca_translation_progresses` (committed 47e9c0a6) — every `ca_step`
makes real rho progress in a closed context — is the solid operational faithfulness
result. The **strong** bisimulation `W ~bisim~ St S'` (Thm C), however, runs into a
genuine, native-only obstruction at FORCE positions:

- After a COMM, the per-rule witness exposes, at a `*x` (force) of the received term,
  `lift_proc 1 0 (St U)` — the **gated** translation of the signed payload `U`.
- The target `St (subst_st T 0 (CQuote U))` exposes `lift_proc 1 0 (Pt (st_to_proc U))`
  — the payload with its gate **stripped** (the source's `subst_caproc … = st_to_proc U`
  dequote runs the content, which was already metered by the firing COMM).
- `St U = PInput (Nt s') (… Pt P' …)` is a **stuck receiver** (no token for `s'` is
  present — `s'` was consumed by the outer firing), whereas `Pt (st_to_proc U) = Pt P'`
  **runs**. By `PInput_alone_stuck` they have different transition behaviour, so they are
  **NOT** strongly bisimilar in general.

**Interpretation.** The native gate translation (`caname_tr (CQuote T) = Quote (st_tr T)`
puts a fuel gate inside every quoted signed term) is right when the quote is used as a
NAME/channel (metering), but **over-gates** when the quote is FORCED (`*x`) — the source
strips-and-runs, the translation re-gates-and-stucks. The old model never saw this (its
continuations were bare procs, already stripped). A fully faithful strong bisimulation
needs a translation refinement at the force point (a "force cashes the signature" step
that supplies the `s'`-token, or a two-level quote/force translation) — this is
genuinely research-grade, NOT a mechanical port, and is recorded here as the precise
obstruction. The progress theorem + the five per-rule reductions stand independently of
it; the strong bisim holds cleanly only on the gate-free-continuation fragment
(`T` with no nested `STSigned`).

**This does NOT block Adjunction II.** The monad paper's Adjunction II (Prop. `adj2`,
*Internalisation as an adjoint retraction*) claims only `Imp_G ∘ η_G ≈ id_G` **up to
weak bisimulation** — the retraction along the **unit-grade, cost-free embedding** `η_G`.
At the unit signature the gate fires against the *freely available unit token* (paper §
"the unit signature … the freely available unit … without net resource"), so the gate
firing is an **administrative reduction** the weak bisimulation absorbs, and the
force-point over-gating — a property of the FULL metered translation at **arbitrary**
grades — never arises. Adjunction II is therefore proven outright in
`CAInternalisation.v` (`ca_internalisation_retraction`, the `s = SUnit` instance of
`ca_single_gate_bisimilar`, axiom-free and fully general over the hash/ground encoders).
The force-cashing refinement would be needed only for the **strictly stronger** claim of
a full strong/weak bisimulation across an arbitrary metered `ca_step` — a statement the
paper does not assert for the adjunction.

**The obstruction is now a PROVEN theorem, not a remark** (`CAForceSeparation.v`):
- `gated_translation_stuck` — for **every** signature `s`, `st_tr (STSigned P s)` is a lone
  `PInput`, hence has **no** `rho_step` (via `PInput_alone_stuck`): the gated translation
  of a signed term is operationally stuck as a standalone term.
- `stuck_not_bisim_stepping` — a stuck process is never strongly bisimilar to one that can
  step (immediate from the backward clause of `bisim`).
- `ca_force_overgating_separation` — therefore, whenever the dequoted source force
  `Pt (st_to_proc (STSigned P s)) = Pt P` can step, `St (STSigned P s) ≁ Pt P`;
  `ca_force_overgating_nonvacuous` exhibits a concrete witness (a matching-COMM continuation
  whose `Pt` fires via `rs_comm`), so this is an **actual** non-bisimilarity, not a
  vacuously-satisfied implication.

So the "full force bisimulation" is **FALSE for the naive translation** — a settled,
machine-checked negative result, not an open task. A force-faithful translation is a
*different* translation (the force-cashing / two-level quote refinement), outside this
spec's committed scope: neither paper asserts this bisimulation, and the spec's faithfulness
obligations (`ca_translation_progresses` + the unit-grade Adjunction II retraction +
`ca_single_gate_bisimilar`) are all discharged. The strong bisim additionally holds cleanly
on the gate-free-continuation fragment (`T` with no nested `STSigned`).

## 4. Honest difficulty flags

- **L3 is the only genuine research risk** — adopt depth-indexing from the start (do
  NOT recover shifts with `lift_proc` after the fact). The `CPInput` case (source binder
  `S n` + lift `N` by 1) interacts with the target gate's own `PInput` and needs L1 +
  `subst_lift_strong` simultaneously.
- **Rules 3 & 4 still need a `Split` mediator** (combined token `TGate (SAnd s1 s2) t`
  routes to a single `POutput (N_tr (SAnd s1 s2))`, but the gate head reads `N_tr s1`).
  GAP-2 removes the **Join/re-seal**, *not* the token-splitting `Split`. (Under-scoped
  item in the original plan — flagged.)
- **Compound payload-release is simpler natively** for r1/2/5 (nested two-gate releases
  both payloads internally ⇒ **no external `Ctx`**, unlike old `compound_*`).
- **No admits.** Axiom-free fallback if L3's dequote case proves intractable under
  deadline: port the synthetic `recursively_metered_image` (`:4296`) as a native
  `ca`-indexed metering relation (backward reflection without commutation) — degrades
  the headline but stays gate-green. Insurance only.

## 5. Gate integration

Each module is a `Section` taking `hash_process`/`ground_process` + the five audited
hypotheses as Variables/Hypotheses; on `End` the headlines become ∀-premised ⇒
`Print Assumptions` reports "Closed under the global context". Add the three modules to
the import chain and one `Print Assumptions` per headline in
`scripts/check-cost-accounted-rho-proofs.sh` (count self-balances).
