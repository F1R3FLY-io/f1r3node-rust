(* ═══════════════════════════════════════════════════════════════════════════
   TokenConservation.v — Token Count Invariant
   ═══════════════════════════════════════════════════════════════════════════

   Proves that the total number of fuel tokens in a system never increases
   under cost-accounted reduction. This is the fundamental conservation law
   of the cost-accounted rho calculus: fuel is neither minted out of thin
   air nor smuggled in through PAR contexts; it can only be consumed by the
   five COMM rules.

   Each of the five rules strips at least one outermost gate from a token
   that authorises the redex, replacing it with the token's suffix. The
   structural rules ca_par_l and ca_par_r are contextual closures that
   propagate the per-rule decrease through parallel composition without
   ever introducing new tokens. Adding the reflexive-transitive closure
   on top of single steps gives the multi-step monotone-decrease theorem.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Theorem                 │ Paper Property
   ─────────────────────────────┼────────────────────────────────────────────
   token_monotone_step          │ "Single step never creates fuel:
                                │   S ⤳ S' ⇒ ‖S'‖ ≤ ‖S‖"
   token_monotone_reachable     │ "Many steps never create fuel:
                                │   S ⤳* S' ⇒ ‖S'‖ ≤ ‖S‖"
   rule1_decreases_by_one       │ "Rule 1 consumes exactly one fuel unit"
   rule2_decreases_by_two       │ "Rule 2 consumes exactly two fuel units"
   rule3_decreases_by_one       │ "Rule 3 consumes exactly one fuel unit"
   rule4_decreases_by_one       │ "May Rule 5 consumes one fuel unit" (April Rule 4)
   rule5_decreases_by_two       │ "May Rule 4 consumes two fuel units" (April Rule 5)
   (Lemma suffixes track the ca_rule4/ca_rule5 constructors; the May-2026 spec
    Section 3.6 swaps the labels — see the canonical note in CostAccountedReduction.v.)
   ─────────────────────────────────────────────────────────────────────────

   Dependencies: Rocq 9.1.1 stdlib, CostAccountedSyntax,
                 CostAccountedReduction (this project)
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Lia Lists.List.
Import ListNotations.

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CostAccountedReduction.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Single-Step Conservation
   ═══════════════════════════════════════════════════════════════════════════

   The headline single-step lemma. By induction on the derivation of
   [ca_step S S'] each of the five COMM rules unfolds [system_token_count]
   on both sides into a closed arithmetic identity that [lia] discharges
   immediately. The PAR cases are dispatched the same way: the inductive
   hypothesis hands us the per-side inequality, and the additive shape of
   [system_token_count] on [SPar] turns it into a sum-respecting bound.
                                                                            *)

Theorem token_monotone_step : forall S S',
  ca_step S S' ->
  system_token_count S' <= system_token_count S.
Proof.
  intros S S' Hstep.
  induction Hstep; simpl.
  - (* ca_rule1: lhs = 0 + (1 + token_size t)
                 rhs = 0 + token_size t
       Net decrease: 1. *)
    lia.
  - (* ca_rule2: lhs = (0 + (1 + token_size t1)) + (1 + token_size t2)
                 rhs = (0 + token_size t1) + token_size t2
       Net decrease: 2. *)
    lia.
  - (* ca_rule3: same shape as ca_rule1 (decrease by 1). *)
    lia.
  - (* ca_rule4: lhs = ((0 + 0) + (1 + token_size t))
                 rhs = (0 + token_size t)
       Net decrease: 1. *)
    lia.
  - (* ca_rule5: same shape as ca_rule2 (decrease by 2). *)
    lia.
  - (* ca_par_l: contextual closure on the left subsystem.
                 IHHstep : count S1' <= count S1
       so       count (S1' ∥ S2) = count S1' + count S2
                                 <= count S1 + count S2
                                 = count (S1 ∥ S2). *)
    lia.
  - (* ca_par_r: symmetric to ca_par_l. *)
    lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Multi-Step Conservation (Reachability)
   ═══════════════════════════════════════════════════════════════════════════

   Lifts [token_monotone_step] across the reflexive-transitive closure
   [ca_reachable]. The base case [car_refl] gives [count S <= count S]
   trivially; the inductive [car_step] case chains the per-step decrease
   from [token_monotone_step] with the inductive hypothesis on the
   remainder of the reduction sequence.                                     *)

Theorem token_monotone_reachable : forall S S',
  ca_reachable S S' ->
  system_token_count S' <= system_token_count S.
Proof.
  intros S S' Hreach.
  induction Hreach as [S0 | S1 S2 S3 Hstep Hreach' IH].
  - (* car_refl: empty sequence, count is unchanged. *)
    lia.
  - (* car_step: S1 ⤳ S2 and S2 ⤳* S3.
                 IH : count S3 <= count S2
       Hstep gives count S2 <= count S1 via token_monotone_step,
       so by transitivity count S3 <= count S1. *)
    apply token_monotone_step in Hstep.
    lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Exact Per-Rule Decrease
   ═══════════════════════════════════════════════════════════════════════════

   The five lemmas below pin down the exact amount by which the token
   count drops on each individual rule. They are stronger than
   [token_monotone_step] in that they give an equality rather than an
   inequality, but each is dispatched by [simpl; lia] because the rule's
   source and target have closed-form token counts.                         *)

Lemma rule1_decreases_by_one : forall x P Q s t,
  system_token_count
    (SPar (SSigned (PPar (PInput x P) (POutput x Q)) s)
          (SToken (TGate s t)))
  = 1 + system_token_count
    (SPar (SSigned (subst_proc P 0 (Quote Q)) s)
          (SToken t)).
Proof.
  intros. simpl. lia.
Qed.

Lemma rule2_decreases_by_two : forall x P Q s1 s2 t1 t2,
  system_token_count
    (SPar (SPar (SSigned (PPar (PInput x P) (POutput x Q)) (SAnd s1 s2))
                (SToken (TGate s1 t1)))
          (SToken (TGate s2 t2)))
  = 2 + system_token_count
    (SPar (SPar (SSigned (subst_proc P 0 (Quote Q)) (SAnd s1 s2))
                (SToken t1))
          (SToken t2)).
Proof.
  intros. simpl. lia.
Qed.

Lemma rule3_decreases_by_one : forall x P Q s1 s2 t,
  system_token_count
    (SPar (SSigned (PPar (PInput x P) (POutput x Q)) (SAnd s1 s2))
          (SToken (TGate (SAnd s1 s2) t)))
  = 1 + system_token_count
    (SPar (SSigned (subst_proc P 0 (Quote Q)) (SAnd s1 s2))
          (SToken t)).
Proof.
  intros. simpl. lia.
Qed.

Lemma rule4_decreases_by_one : forall x P Q s1 s2 t,
  system_token_count
    (SPar (SPar (SSigned (PInput x P) s1)
                (SSigned (POutput x Q) s2))
          (SToken (TGate (SAnd s1 s2) t)))
  = 1 + system_token_count
    (SPar (SSigned (subst_proc P 0 (Quote Q)) (SAnd s1 s2))
          (SToken t)).
Proof.
  intros. simpl. lia.
Qed.

Lemma rule5_decreases_by_two : forall x P Q s1 s2 t1 t2,
  system_token_count
    (SPar (SPar (SPar (SSigned (PInput x P) s1)
                      (SSigned (POutput x Q) s2))
                (SToken (TGate s1 t1)))
          (SToken (TGate s2 t2)))
  = 2 + system_token_count
    (SPar (SPar (SSigned (subst_proc P 0 (Quote Q)) (SAnd s1 s2))
                (SToken t1))
          (SToken t2)).
Proof.
  intros. simpl. lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Exact-Decrease Theorem (the conservation invariant)
   ═══════════════════════════════════════════════════════════════════════════

   The headline theorem: every cost-accounted reduction step consumes a
   STRICTLY POSITIVE amount of fuel. Combined with non-negativity of
   token counts, this gives a termination measure for cost-accounted
   reductions: no infinite reduction sequence is possible from a
   finite-fuel system.

   The proof case-splits on the rule and uses the per-rule decrease
   lemmas. For the contextual closure cases (ca_par_l, ca_par_r), the
   inductive hypothesis carries the existence of the consumed quantum
   through the parallel composition.                                       *)

Theorem token_consumed_per_step : forall S S',
  ca_step S S' ->
  exists k, k > 0 /\ system_token_count S = k + system_token_count S'.
Proof.
  intros S S' Hstep.
  induction Hstep.
  - (* ca_rule1: decreases by 1 *)
    exists 1. split; [lia |]. simpl. lia.
  - (* ca_rule2: decreases by 2 *)
    exists 2. split; [lia |]. simpl. lia.
  - (* ca_rule3: decreases by 1 *)
    exists 1. split; [lia |]. simpl. lia.
  - (* ca_rule4: decreases by 1 *)
    exists 1. split; [lia |]. simpl. lia.
  - (* ca_rule5: decreases by 2 *)
    exists 2. split; [lia |]. simpl. lia.
  - (* ca_par_l: lift the existential through the parallel context *)
    destruct IHHstep as [k [Hk Heq]].
    exists k. split; [exact Hk |]. simpl. lia.
  - (* ca_par_r: symmetric *)
    destruct IHHstep as [k [Hk Heq]].
    exists k. split; [exact Hk |]. simpl. lia.
Qed.

(* Corollary: cost-accounted reduction is strictly decreasing on the
   token count, hence well-founded (no infinite reductions). *)
Corollary token_strictly_decreases : forall S S',
  ca_step S S' ->
  system_token_count S' < system_token_count S.
Proof.
  intros S S' Hstep.
  apply token_consumed_per_step in Hstep.
  destruct Hstep as [k [Hk Heq]].
  lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 5: WD-D2 Acceptance-Settlement Conservation (C↔D bridge)
   ═══════════════════════════════════════════════════════════════════════════

   The block-assembly acceptance gate admits a per-signature group of
   deployments and then SETTLES the per-signature supply pool [Σ⟦s⟧] by
   subtracting the SUM of the admitted deployments' demands [ΣΔ_admitted]
   (cost-accounted-rho §7.7; supply-realization handoff Decision 4c). The
   realized form (casper/.../costacc/close_block_deploy.rs::dual_write_supply)
   is the integer balance update [new = old.checked_sub(ΣΔ)], applied exactly
   once per block AFTER all admitted deployments have executed.

   Here we prove the conservation law for that settlement at the balance level:
   the post-settlement balance plus the debited amount equals the pre-settlement
   balance — no fuel is created or destroyed by the settlement; the tokens that
   leave the pool EXACTLY equal the admitted demand. This is the [TokenConservation]
   counterpart, at the supply-balance layer, of the [system_token_count]
   monotone-decrease theorem above (which conserves at the running-process layer):
   together they pin "consumed = Δ_s" (the per-step decrease) AND "post = pre − ΣΔ"
   (the settlement debit) as the two faces of one conserved quantity.            *)

(* The total admitted demand of a group: the sum of its per-deployment demands,
   in canonical order. (Demands are non-negative; modeled as [nat].) *)
Fixpoint admitted_demand_sum (ds : list nat) : nat :=
  match ds with
  | nil => 0
  | d :: ds' => d + admitted_demand_sum ds'
  end.

(* The post-settlement supply balance: pre-balance minus the total admitted
   demand. ([nat] truncated subtraction; the gate guarantees [ΣΔ ≤ pre], so under
   that hypothesis the subtraction is exact — see [settlement_conserves]). *)
Definition settle_balance (pre : nat) (ds : list nat) : nat :=
  pre - admitted_demand_sum ds.

(* [settlement_conserves]: under the gate's funding guarantee [ΣΔ_admitted ≤ pre]
   (which the acceptance gate enforces — it admits only a prefix whose cumulative
   demand fits the effective supply, and the underflow-guarded [checked_sub] would
   reject otherwise), the post-settlement balance plus the debited total EQUALS the
   pre-balance: [post + ΣΔ = pre], i.e. [post = pre − ΣΔ] with the subtraction
   exact. No fuel is created or destroyed by the acceptance settlement. *)
Theorem settlement_conserves :
  forall (pre : nat) (ds : list nat),
    admitted_demand_sum ds <= pre ->
    settle_balance pre ds + admitted_demand_sum ds = pre.
Proof.
  intros pre ds Hle. unfold settle_balance. lia.
Qed.

(* [accept_commit_conserves] (supply-realization handoff Decision 8,
   [TokenConservation.v] obligation "accept_commit_conserves"): the headline
   statement of the settlement law — the post-state supply balance is EXACTLY the
   pre-state balance minus the sum of the admitted deployments' demands, and that
   debited sum is EXACTLY the admitted demand total (which "= Σ reconcile.consumed"
   at the runtime, by the per-deployment "consumed = Δ_s" bridge that
   [replay_cost_mismatch] guards). Stated as the conjunction the C↔D bridge
   requires: [post = pre − ΣΔ_admitted] ∧ [ΣΔ_admitted = Σ of the admitted demands]. *)
Theorem accept_commit_conserves :
  forall (pre : nat) (ds : list nat),
    admitted_demand_sum ds <= pre ->
    settle_balance pre ds = pre - admitted_demand_sum ds
    /\ pre - settle_balance pre ds = admitted_demand_sum ds.
Proof.
  intros pre ds Hle. unfold settle_balance. split; lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 6: Stage-D fee collection + 1:1 conversion conserves (the FEE layer)
   ═══════════════════════════════════════════════════════════════════════════

   The COST lemmas above (settlement debit) are UNCHANGED. This section adds the
   independent FEE-layer conservation law (Stage D; spec "Fee conversion"
   tex:3061-3100). The fee is the spec's FeeExtract — a SEPARATE token,
   TRANSFERRED to the validator (never the burned settlement debit; cost ≠ fee).
   The realization holds two reducer-unwritable balances per validator: the fee
   pool [F_v] (`supply::fee_collection_channel`) and the gate pool [Σ⟦v⟧]. The
   economic loop is two writes:

     - COLLECTION: [F_v += f] (one token per processed deploy);
     - CONVERSION (1:1, at the epoch boundary): move the collected [f] from
       [F_v] into [Σ⟦v⟧] — [Σ⟦v⟧ += f], [F_v := 0].

   We prove that the conversion CONSERVES the validator's total fee+supply
   holding: what leaves [F_v] EXACTLY equals what enters [Σ⟦v⟧], so the combined
   total [F_v + Σ⟦v⟧] is invariant across the conversion. This is the balance-
   layer companion of [exchange_total_conserved] (Exchange.v) at the per-validator
   ledger, and the conservation half of [fee_convert_credit_is_backed]
   (MintingInjection.v). No tokens are minted or destroyed by the fee loop.       *)

(* The validator's fee + supply ledger: the collected fee pool [fee] = F_v and
   the gate pool [sigma] = Σ⟦v⟧. *)
Record fee_ledger : Type := {
  fee_pool   : nat;   (* F_v : collected, not-yet-converted fees *)
  supply_pool : nat   (* Σ⟦v⟧ : the gate pool *)
}.

(* COLLECTION: credit [f] tokens to the fee pool (the FeeExtract). *)
Definition fee_collect (l : fee_ledger) (f : nat) : fee_ledger :=
  {| fee_pool := fee_pool l + f; supply_pool := supply_pool l |}.

(* CONVERSION (1:1): move the ENTIRE fee pool [f = fee_pool l] into the supply
   pool and zero the fee pool. Mirrors the Rust post_eval convert (Σ⟦v⟧ += f,
   F_v := 0) for an eligible validator with f > 0; for f = 0 it is the identity
   (DR-4: no one-sided mint). *)
Definition fee_convert (l : fee_ledger) : fee_ledger :=
  {| fee_pool := 0; supply_pool := supply_pool l + fee_pool l |}.

(* The validator's total fee+supply holding. *)
Definition ledger_total (l : fee_ledger) : nat := fee_pool l + supply_pool l.

(* [fee_collection_conserves]: the 1:1 fee conversion CONSERVES the combined
   F_v + Σ⟦v⟧ total — exactly the [f] that leaves [F_v] enters [Σ⟦v⟧], so no
   token is created or destroyed by the fee loop. (The COLLECTION that precedes
   it added [f] from the client's transferred FeeExtract token, accounted at the
   block level; here we pin that the CONVERSION step itself is conserving, the
   property the multi-parent-merge / replay symmetry rests on.) *)
Theorem fee_collection_conserves : forall l,
  ledger_total (fee_convert l) = ledger_total l.
Proof.
  intros l. unfold ledger_total, fee_convert. simpl. lia.
Qed.

(* The convert credit to Σ⟦v⟧ is EXACTLY the fee pool that was drained (the
   1:1 peg with no remainder), and the fee pool ends at 0 — the realization'
   `Σ⟦v⟧ += f` / `F_v := 0`. *)
Theorem fee_convert_credit_eq_drained : forall l,
  supply_pool (fee_convert l) = supply_pool l + fee_pool l
  /\ fee_pool (fee_convert l) = 0.
Proof.
  intros l. unfold fee_convert. simpl. split; reflexivity.
Qed.

(* End-to-end (collect then convert) conserves the total relative to the
   pre-collection ledger PLUS the collected amount: the FeeExtract token [f] is
   neither lost nor duplicated as it flows F_v → Σ⟦v⟧. *)
Theorem fee_collect_then_convert_conserves : forall l f,
  ledger_total (fee_convert (fee_collect l f)) = ledger_total l + f.
Proof.
  intros l f. unfold ledger_total, fee_convert, fee_collect. simpl. lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 7: #12 — Compound (Split/Join) MULTI-POOL settlement conservation
   ═══════════════════════════════════════════════════════════════════════════

   The Section-5 settlement law [settlement_conserves] covers the SINGLE-pool
   debit ([post = pre − ΣΔ]) — the common single-signer shape. This section adds
   the conservation law for the COMPOUND ([Sig::And s₁ s₂]) debit, where one
   admitted compound group settles across THREE pools at once (spec §3.6 Rule 2 +
   Rule 4, tex 677-728: a compound-signed COMM consumes ONE token from each
   COMPONENT pool, OR one token from the combined pool; App. A Split/Join,
   tex 2020-2245). The realized form
   (casper/.../util/rholang/acceptance.rs::compute_settlement_debits) splits the
   group's cumulative admitted demand [k] COMBINED-POOL-FIRST:

     draw_compound = min(k, Σ⟦compound⟧)
     draw_pair     = k − draw_compound        (≤ min(Σ⟦s₁⟧, Σ⟦s₂⟧) by admission)
     Σ⟦compound⟧ −= draw_compound ; Σ⟦s₁⟧ −= draw_pair ; Σ⟦s₂⟧ −= draw_pair

   Two faces of the conserved quantity are proven here at the balance layer:
   (a) underflow-safety — each of the three post-balances stays [≥ 0]; and
   (b) conservation — the three post-balances plus the tokens drawn EXACTLY equal
   the three pre-balances. As with the [system_token_count] decrease theorems
   (Section 1), the BRIDGE is that the [draw_compound·1 + draw_pair·2] total tokens
   matches the two-token-per-compound-COMM semantics: drawing the matched
   COMPONENT pair costs two tokens (one per component, [rule2_decreases_by_two] /
   [rule5_decreases_by_two]), and drawing the COMBINED pool costs one
   ([rule4_decreases_by_one] / [rule3_decreases_by_one]).

   These are [Nat]/[lia]-provable; the only hypotheses are exactly the admission
   bounds the Rust gate enforces ([draw_compound ≤ Σ_comp] from
   [k.min(sigma_compound)], and [draw_pair ≤ min Σ₁ Σ₂] from the effective-supply
   admission [k ≤ Σ_compound + min(Σ₁,Σ₂)] combined with the combined-pool-first
   split). *)

(* The total tokens DRAWN by a compound settlement: one per combined-pool unit,
   TWO per component-pair unit (the matched pair debits BOTH [Σ⟦s₁⟧] and [Σ⟦s₂⟧]).
   This is the [draw_compound·1 + draw_pair·2] of the Rust debit and the
   Rule-4 (combined, 1 token) / Rule-2 (split pair, 2 tokens) bridge. *)
Definition compound_tokens_drawn (draw_compound draw_pair : nat) : nat :=
  draw_compound + 2 * draw_pair.

(* [compound_split_debit_conserves] (#12 core): under the admission bounds
   [draw_compound ≤ Σ_comp] and [draw_pair ≤ min Σ₁ Σ₂], the three-pool compound
   settlement is (a) UNDERFLOW-SAFE — every post-balance is [≥ 0] (trivially in
   [nat], so stated as the EXACT post-balance value, with the subtraction not
   truncating, which IS the [≥ 0]-with-exact-debit content the [checked_sub]
   backstop guarantees); and (b) CONSERVING — the three post-balances plus the
   tokens drawn equal the three pre-balances. No fuel is created or destroyed by
   the compound multi-pool debit. *)
Theorem compound_split_debit_conserves :
  forall (sigma_comp sigma1 sigma2 draw_compound draw_pair : nat),
    draw_compound <= sigma_comp ->
    draw_pair <= Nat.min sigma1 sigma2 ->
    (* (a) underflow-safety: each post-balance is the exact (non-truncating)
       difference, hence [≥ 0] and the debited amount is exactly the draw. *)
    (sigma_comp - draw_compound) + draw_compound = sigma_comp /\
    (sigma1 - draw_pair) + draw_pair = sigma1 /\
    (sigma2 - draw_pair) + draw_pair = sigma2 /\
    (* (b) conservation: post-balances + tokens-drawn = pre-balances. *)
    (sigma_comp - draw_compound) + (sigma1 - draw_pair) + (sigma2 - draw_pair)
      + compound_tokens_drawn draw_compound draw_pair
      = sigma_comp + sigma1 + sigma2.
Proof.
  intros sigma_comp sigma1 sigma2 draw_compound draw_pair Hc Hp.
  unfold compound_tokens_drawn.
  (* [draw_pair ≤ min σ₁ σ₂] gives [draw_pair ≤ σ₁] and [draw_pair ≤ σ₂]. *)
  assert (Hp1 : draw_pair <= sigma1) by lia.
  assert (Hp2 : draw_pair <= sigma2) by lia.
  repeat split; lia.
Qed.

(* The underflow-safety face stated directly as the three [≥ 0] inequalities the
   spec's funding obligation guarantees (every settled pool ends non-negative).
   In [nat] these hold unconditionally, but pairing them with the admission
   bounds documents that the EXACT (non-truncating) difference is what the Rust
   [checked_sub] computes — an over-draw would underflow and reject the block. *)
Corollary compound_split_debit_no_underflow :
  forall (sigma_comp sigma1 sigma2 draw_compound draw_pair : nat),
    draw_compound <= sigma_comp ->
    draw_pair <= Nat.min sigma1 sigma2 ->
    sigma_comp - draw_compound >= 0 /\
    sigma1 - draw_pair >= 0 /\
    sigma2 - draw_pair >= 0 /\
    (* and the exact debited amounts recover the pre-balances (no truncation): *)
    sigma_comp = (sigma_comp - draw_compound) + draw_compound /\
    sigma1 = (sigma1 - draw_pair) + draw_pair /\
    sigma2 = (sigma2 - draw_pair) + draw_pair.
Proof.
  intros sigma_comp sigma1 sigma2 draw_compound draw_pair Hc Hp.
  assert (Hp1 : draw_pair <= sigma1) by lia.
  assert (Hp2 : draw_pair <= sigma2) by lia.
  repeat split; lia.
Qed.

(* ─── Multi-pool generalization: a LIST of (pre, draw) settlements ────────────

   A block may settle MANY pools at once (every admitted group + every compound
   component). The cross-group residual ledger (acceptance.rs) keeps the SUMMED
   draw on each distinct pool [≤] its pre-state balance, so each pool's net
   (pre, draw) pair independently satisfies [draw ≤ pre]. Conservation then lifts
   to the whole block: the sum of post-balances plus the sum of draws equals the
   sum of pre-balances. The 3-pool [compound_split_debit_conserves] is the
   required core; this list form shows it composes across an arbitrary number of
   pools (the block-level settlement-conservation statement). *)

(* Sum of pre-state balances over a list of (pre, draw) pool settlements. *)
Fixpoint settlement_pre_sum (ps : list (nat * nat)) : nat :=
  match ps with
  | nil => 0
  | (pre, _) :: ps' => pre + settlement_pre_sum ps'
  end.

(* Sum of tokens drawn over the settlements. *)
Fixpoint settlement_draw_sum (ps : list (nat * nat)) : nat :=
  match ps with
  | nil => 0
  | (_, draw) :: ps' => draw + settlement_draw_sum ps'
  end.

(* Sum of post-state balances ([pre − draw] per pool). *)
Fixpoint settlement_post_sum (ps : list (nat * nat)) : nat :=
  match ps with
  | nil => 0
  | (pre, draw) :: ps' => (pre - draw) + settlement_post_sum ps'
  end.

(* The per-pool admission bound holds for EVERY pool in the list: each pool's
   drawn amount does not exceed its pre-state balance (the cross-group residual
   ledger's invariant). *)
Definition all_draws_within (ps : list (nat * nat)) : Prop :=
  Forall (fun p => snd p <= fst p) ps.

(* [multi_settlement_conserves]: under the per-pool admission bound, the
   block-level settlement is CONSERVING — [Σ post + Σ draws = Σ pre]. By list
   induction; each cons-cell discharges its own [pre − draw + draw = pre] via the
   head bound, and the tail by the inductive hypothesis. This is the list
   composition of [compound_split_debit_conserves] (whose three pools are three
   such (pre, draw) entries: [(Σ_comp, draw_compound)], [(Σ₁, draw_pair)],
   [(Σ₂, draw_pair)]). *)
Theorem multi_settlement_conserves :
  forall (ps : list (nat * nat)),
    all_draws_within ps ->
    settlement_post_sum ps + settlement_draw_sum ps = settlement_pre_sum ps.
Proof.
  intros ps Hwithin.
  induction ps as [| [pre draw] ps' IH]; cbn.
  - reflexivity.
  - (* head bound [draw ≤ pre] from the [Forall], tail by IH. *)
    inversion Hwithin as [| p ps'' Hhead Htail Heq]; subst.
    cbn in Hhead.
    specialize (IH Htail).
    lia.
Qed.

(* Bridge corollary: the 3-pool compound debit is an INSTANCE of the list-level
   block settlement conservation. Packaging the compound's three pools as the
   list [(Σ_comp, draw_compound); (Σ₁, draw_pair); (Σ₂, draw_pair)] and applying
   [multi_settlement_conserves] yields the same conserved identity (modulo the
   [2·draw_pair] regrouping, since the pair-draw appears in TWO list entries —
   one per component — which is exactly the [compound_tokens_drawn] two-token
   accounting). This ties the focused 3-pool core to the general block law. *)
Corollary compound_debit_is_block_settlement_instance :
  forall (sigma_comp sigma1 sigma2 draw_compound draw_pair : nat),
    draw_compound <= sigma_comp ->
    draw_pair <= Nat.min sigma1 sigma2 ->
    let ps := (sigma_comp, draw_compound)
                :: (sigma1, draw_pair)
                :: (sigma2, draw_pair) :: nil in
    settlement_post_sum ps + settlement_draw_sum ps = settlement_pre_sum ps /\
    settlement_draw_sum ps = compound_tokens_drawn draw_compound draw_pair.
Proof.
  intros sigma_comp sigma1 sigma2 draw_compound draw_pair Hc Hp ps.
  assert (Hwithin : all_draws_within ps).
  { unfold ps, all_draws_within.
    repeat constructor; cbn; lia. }
  split.
  - apply multi_settlement_conserves; exact Hwithin.
  - unfold ps, settlement_draw_sum, compound_tokens_drawn. cbn. lia.
Qed.
