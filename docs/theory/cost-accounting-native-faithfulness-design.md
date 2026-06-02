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
- [ ] **L5** gate-unwrap (atomic + SAnd) — old `rule1_unit_after_gate` shape.
- [ ] `se_implies_bisim`, `bisim_par_l`/`bisim_par_r` congruence (small cofix).

### `CATranslationFaithfulness.v`  (Section)  — **Section + foundation committed (89737ab4)**
- [x] Section over the audited hash/ground hypotheses; Local Notations Nt/Tt/Pt/Ct/St.
- [x] in-Section closedness (Nt_closed/Tt_closed) + lift/subst invariance
  (Nt_lift_inv/Nt_subst_inv/Tt_lift_inv/Tt_subst_inv) — closed images inert under COMM.
- [ ] **PREREQUISITE (newly identified):** a proc-level lift/lift composition lemma
  `lift_proc d 1 (lift_proc 1 0 Q) = lift_proc (S d) 0 Q` (general `lift_lift` on proc).
  **RhoSyntax does NOT provide it** (only `lift_zero_proc`); must be proved (mutual
  induction on proc/name, ~60 lines) before the depth-indexed bridge.
- [x] **st_trd (d,c)** : depth-indexed mutual translation (`p_trd`/`cn_trd`/`st_trd`,
  threading a lift at cutoff c) + **bridge** `st_trd d c X = lift_proc d c (St X)`
  (`trd_bridge`, via `lift_lift_comm_proc` at gates) + `st_trd_zero` (`St = st_trd 0 0`).
  Committed 54701083. Also: `lift_lift_comm_proc`, `lift_lift_compose_proc`,
  `lift_proc_S_compose` (committed cbe527d6/54701083).
- [ ] **L3 / per-rule commutation — KEY FINDING (verified by hand on
  `T = STSigned (CPDeref (CNVar 0)) SUnit`):** the commutation is **NOT a syntactic
  equality** even at index 0. `subst_proc (St T) 0 (Quote (St U))` yields
  `PDeref(Quote(St U))` (the *gated* translation, derefed) whereas
  `St (subst_st T 0 (CQuote U))` yields `Pt(st_to_proc U)` (gates **stripped** by the
  dequote-collapse). So there is no clean equational L3; the per-rule simulations are
  **operational + coinductive**, reducing `PDeref(Quote(St U)) →/≡ St U` (deref-quote)
  then `St U ~bisim~ Pt(st_to_proc U)` (L4). This confirms L4 is load-bearing for every
  rule, and the per-rule proofs need RhoReduction's reduction rules + the `bisim`
  coinductive — they are NOT discharged by the lift/subst equational layer (now complete).
- [ ] **L4** dequote-collapse bisim (ports `multi_stuck_residue_bisim`; ~150 lines).
- [ ] `rule1..5` per-rule simulations (Thm B). Step counts: r1 atomic **2** / SAnd **3**;
  r2 **3** (nested two-gate, no Split); r3 **5** (Split needed — combined token); r4 **5**
  (Split, **no Join** — GAP-2); r5 **3** (no Split, no Join).
- [ ] `ca_par_l`/`ca_par_r` congruence.
- [ ] **Thm A** `ca_translation_forward` (headline): `∀ S S', ca_step S S' → ∃ W,
  rho_reachable (tr S) W ∧ bisim W (tr S')`.

**Progress note (this session):** the entire `CATranslationLemmas` module (L1/L2/
lift_lift_comm) and the `CATranslationFaithfulness` Section + invariance foundation are
committed and gate-green. The remaining items above form a layered development (proc
lift/lift → st_tr_d + bridge → L3 → L4 → per-rule → Thm A → Thm C) whose crux (L3) is the
genuinely-novel research step; each layer must compile axiom-free before the next, so it
proceeds as a sequence of gate-green checkpoints, not one landing.

### `CABisimulation.v`  (Section)
- [ ] port `bisim` usage + `post_gate_bisim`/`multi_stuck_residue_bisim` native.
- [ ] `ca_sim` cofixpoint; **Thm C** `ca_translation_strong_bisimilar` (headline).

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
