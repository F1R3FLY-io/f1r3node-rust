(* ═══════════════════════════════════════════════════════════════════════════
   BugFixWithdrawTransferFailure.v — Proof for Bug Fix #10 (T-9.10)

   Bug #10. PoS.rhox:619 has "FIXME fix transfer in failure case" inside
   `removeQuarantinedWithdrawers`. The pre-fix `payWithdraw` contract calls
   `payWithdrawer!(...)` and the surrounding flow proceeds to remove the
   validator from `withdrawers` and `committedRewards` regardless of
   whether the underlying `posVault.transfer` succeeded. If the transfer
   fails the validator is removed from state without receiving funds — a
   fund-loss bug that breaks vault conservation.

   Fix. Pattern-match on the transfer result: only validators whose
   transfer succeeded are removed from `withdrawers` / `committedRewards`.
   Failed withdrawals leave the per-validator state intact for retry on a
   later block. This mirrors the Bug #4 fix already applied to the slash
   arm.

   Theorems proven (no admits):
     T-9.10  (withdraw_transfer_failure_safety) — Per-validator safety:
              the post-fix transition either succeeds (validator removed)
              or fails (state unchanged for that validator).
     T-9.10' (failure_preserves_total_funds)    — Total funds invariant:
              a failed withdrawal does not lose funds (the fund-loss bug
              is structurally ruled out).
     T-9.10″ (withdraw_independence)            — Order-independence of
              the parallel `unorderedParMap` flow.

   Dependencies: PoSContract.v (for PoSState), Validator.v.
   Companion doc: docs/theory/slashing/design/09-bug-fixes-and-rationale.md
                  §9.13.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Stdlib Require Import PeanoNat.
From Slashing Require Import Validator PoSContract.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Withdrawer / reward maps
   ═══════════════════════════════════════════════════════════════════════════

   The PoS contract tracks two auxiliary maps alongside `allBonds`:
   - `withdrawers : V → (bond, quarantine_block)` — validators who unbonded
     and are awaiting end-of-quarantine before payout.
   - `committedRewards : V → nat` — accumulated epoch rewards.
   We model both as association lists, mirroring `BondMap`. *)

Definition WithdrawerEntry := (nat * nat)%type.   (* (bond, quarantine) *)
Definition WithdrawerMap   := list (Validator * WithdrawerEntry).
Definition RewardMap       := list (Validator * nat).

Fixpoint wm_lookup (wm : WithdrawerMap) (v : Validator) : option WithdrawerEntry :=
  match wm with
  | []         => None
  | (k, e) :: rest =>
      if validator_eq_dec k v then Some e else wm_lookup rest v
  end.

Fixpoint wm_remove (wm : WithdrawerMap) (v : Validator) : WithdrawerMap :=
  match wm with
  | []         => []
  | (k, e) :: rest =>
      if validator_eq_dec k v
      then wm_remove rest v
      else (k, e) :: wm_remove rest v
  end.

Fixpoint rm_lookup (rm : RewardMap) (v : Validator) : nat :=
  match rm with
  | []         => 0
  | (k, n) :: rest =>
      if validator_eq_dec k v then n else rm_lookup rest v
  end.

Fixpoint rm_remove (rm : RewardMap) (v : Validator) : RewardMap :=
  match rm with
  | []         => []
  | (k, n) :: rest =>
      if validator_eq_dec k v
      then rm_remove rest v
      else (k, n) :: rm_remove rest v
  end.

Definition wm_contains (wm : WithdrawerMap) (v : Validator) : bool :=
  match wm_lookup wm v with
  | Some _ => true
  | None   => false
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Extended PoS state
   ═══════════════════════════════════════════════════════════════════════════

   PoSStateW additively embeds the base PoSState. The bisimilarity proof in
   Bisimulation.v projects PoSStateW.psw_pos to recover the base state.
   Vault conservation is stated over the extended state because withdrawal
   payouts move funds from the PoS vault to the validator's own vault. *)

Record PoSStateW : Type := mkPoSStateW {
  psw_pos         : PoSState;
  psw_withdrawers : WithdrawerMap;
  psw_rewards     : RewardMap;
  psw_pos_balance : nat
}.

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — Withdraw with transfer-success oracle
   ═══════════════════════════════════════════════════════════════════════════

   The post-fix `payWithdraw` is parameterised by a Boolean transfer-success
   oracle. The oracle abstracts the underlying `posVault.transfer` outcome.
   - On success: the validator is removed from `withdrawers` and
     `committedRewards`, and the PoS balance is decremented by their full
     payout (bond + accumulated rewards).
   - On failure: state is unchanged. The validator remains in `withdrawers`
     for a future block to retry. *)

Definition payout (psw : PoSStateW) (v : Validator) : nat :=
  match wm_lookup (psw_withdrawers psw) v with
  | Some (bond, _) => bond + rm_lookup (psw_rewards psw) v
  | None           => 0
  end.

Definition withdraw_with_transfer_oracle
    (psw : PoSStateW) (v : Validator) (transfer_ok : bool) : PoSStateW :=
  if transfer_ok
  then
    mkPoSStateW
      (psw_pos psw)
      (wm_remove (psw_withdrawers psw) v)
      (rm_remove (psw_rewards psw) v)
      (psw_pos_balance psw - payout psw v)
  else psw.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — Helper lemmas on map operations
   ═══════════════════════════════════════════════════════════════════════════ *)

Lemma wm_lookup_remove_self :
  forall wm v,
    wm_lookup (wm_remove wm v) v = None.
Proof.
  induction wm as [| [k e] rest IH]; intros v; simpl.
  - reflexivity.
  - destruct (validator_eq_dec k v) as [Heq | Hneq].
    + apply IH.
    + simpl. destruct (validator_eq_dec k v) as [E | _]; [contradiction | apply IH].
Qed.

Lemma rm_lookup_remove_self :
  forall rm v,
    rm_lookup (rm_remove rm v) v = 0.
Proof.
  induction rm as [| [k n] rest IH]; intros v; simpl.
  - reflexivity.
  - destruct (validator_eq_dec k v) as [Heq | Hneq].
    + apply IH.
    + simpl. destruct (validator_eq_dec k v) as [E | _]; [contradiction | apply IH].
Qed.

Lemma wm_lookup_remove_other :
  forall wm v u,
    u <> v ->
    wm_lookup (wm_remove wm v) u = wm_lookup wm u.
Proof.
  induction wm as [| [k e] rest IH]; intros v u Hneq; simpl.
  - reflexivity.
  - destruct (validator_eq_dec k v) as [Hkv | Hkv].
    + destruct (validator_eq_dec k u) as [Hku | Hku].
      * subst k. symmetry in Hku. contradiction (Hneq Hku).
      * apply IH; assumption.
    + simpl. destruct (validator_eq_dec k u) as [Hku | Hku].
      * reflexivity.
      * apply IH; assumption.
Qed.

Lemma rm_lookup_remove_other :
  forall rm v u,
    u <> v ->
    rm_lookup (rm_remove rm v) u = rm_lookup rm u.
Proof.
  induction rm as [| [k n] rest IH]; intros v u Hneq; simpl.
  - reflexivity.
  - destruct (validator_eq_dec k v) as [Hkv | Hkv].
    + destruct (validator_eq_dec k u) as [Hku | Hku].
      * subst k. symmetry in Hku. contradiction (Hneq Hku).
      * apply IH; assumption.
    + simpl. destruct (validator_eq_dec k u) as [Hku | Hku].
      * reflexivity.
      * apply IH; assumption.
Qed.

Lemma wm_remove_commutative :
  forall wm v u,
    wm_remove (wm_remove wm v) u = wm_remove (wm_remove wm u) v.
Proof.
  induction wm as [| [k e] rest IH]; intros v u; simpl.
  - reflexivity.
  - destruct (validator_eq_dec k v) as [Hkv | Hkv].
    + destruct (validator_eq_dec k u) as [Hku | Hku].
      * apply IH.
      * simpl. destruct (validator_eq_dec k v) as [_ | NE]; [apply IH | contradiction].
    + destruct (validator_eq_dec k u) as [Hku | Hku].
      * simpl. destruct (validator_eq_dec k u) as [_ | NE]; [apply IH | contradiction].
      * simpl.
        destruct (validator_eq_dec k v) as [E | _]; [contradiction |].
        destruct (validator_eq_dec k u) as [E | _]; [contradiction |].
        f_equal. apply IH.
Qed.

Lemma rm_remove_commutative :
  forall rm v u,
    rm_remove (rm_remove rm v) u = rm_remove (rm_remove rm u) v.
Proof.
  induction rm as [| [k n] rest IH]; intros v u; simpl.
  - reflexivity.
  - destruct (validator_eq_dec k v) as [Hkv | Hkv].
    + destruct (validator_eq_dec k u) as [Hku | Hku].
      * apply IH.
      * simpl. destruct (validator_eq_dec k v) as [_ | NE]; [apply IH | contradiction].
    + destruct (validator_eq_dec k u) as [Hku | Hku].
      * simpl. destruct (validator_eq_dec k u) as [_ | NE]; [apply IH | contradiction].
      * simpl.
        destruct (validator_eq_dec k v) as [E | _]; [contradiction |].
        destruct (validator_eq_dec k u) as [E | _]; [contradiction |].
        f_equal. apply IH.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — T-9.10: Per-validator transfer-failure safety
   ═══════════════════════════════════════════════════════════════════════════

   The post-fix withdraw transition either succeeds AND removes the validator
   from `withdrawers`, or fails AND leaves the entire state unchanged. The
   bug — losing funds by removing the validator without paying — is
   structurally precluded. *)

Theorem t_9_10_withdraw_transfer_failure_safety :
  forall psw v transfer_ok,
    let psw' := withdraw_with_transfer_oracle psw v transfer_ok in
    (transfer_ok = true /\ wm_contains (psw_withdrawers psw') v = false)
    \/ (transfer_ok = false /\ psw' = psw).
Proof.
  intros psw v transfer_ok.
  unfold withdraw_with_transfer_oracle.
  destruct transfer_ok.
  - left. split; [reflexivity |].
    unfold wm_contains. simpl.
    rewrite wm_lookup_remove_self. reflexivity.
  - right. split; reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §6 — T-9.10': A failed withdrawal preserves the total-funds invariant
   ═══════════════════════════════════════════════════════════════════════════

   The total funds tracked by the contract for the pending-withdrawal flow:
     total_w(psw) = pos_balance + Σ payouts_for_withdrawers.

   A failed withdrawal leaves the state unchanged, so the invariant is
   trivially preserved. This rules out the pre-fix fund-loss bug:
   pre-fix, a failed transfer would shrink Σ (validator removed) without
   correspondingly debiting pos_balance, so funds would vanish. The
   post-fix's failure-preserving branch directly guarantees total_w
   stays constant on failure. *)

Fixpoint sum_withdrawer_payouts (wm : WithdrawerMap) (rm : RewardMap) : nat :=
  match wm with
  | []         => 0
  | (k, (bond, _)) :: rest =>
      bond + rm_lookup rm k + sum_withdrawer_payouts rest rm
  end.

Definition total_funds (psw : PoSStateW) : nat :=
  psw_pos_balance psw + sum_withdrawer_payouts (psw_withdrawers psw) (psw_rewards psw).

Theorem t_9_10_failure_preserves_total_funds :
  forall psw v,
    total_funds (withdraw_with_transfer_oracle psw v false) = total_funds psw.
Proof.
  intros psw v.
  unfold withdraw_with_transfer_oracle. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §7 — T-9.10″: Order-independence of parallel withdrawals
   ═══════════════════════════════════════════════════════════════════════════

   The Rholang flow uses `unorderedParMap` to drive withdrawals in parallel.
   Per-validator independence requires that withdrawing v then u produces
   the same withdrawer / reward maps as withdrawing u then v, when v ≠ u.
   This is the formal-verification anchor for parallel `unorderedParMap`
   correctness in the production flow. *)

Theorem t_9_10_withdraw_independence :
  forall psw v u ok_v ok_u,
    v <> u ->
    let psw1 := withdraw_with_transfer_oracle
                  (withdraw_with_transfer_oracle psw v ok_v) u ok_u in
    let psw2 := withdraw_with_transfer_oracle
                  (withdraw_with_transfer_oracle psw u ok_u) v ok_v in
    psw_withdrawers psw1 = psw_withdrawers psw2
    /\ psw_rewards psw1 = psw_rewards psw2.
Proof.
  intros psw v u ok_v ok_u Hneq.
  unfold withdraw_with_transfer_oracle.
  destruct ok_v, ok_u; simpl; split.
  - apply wm_remove_commutative.
  - apply rm_remove_commutative.
  - reflexivity.
  - reflexivity.
  - reflexivity.
  - reflexivity.
  - reflexivity.
  - reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §8 — Closing remarks
   ═══════════════════════════════════════════════════════════════════════════

   Three theorems proven without admits:
   - T-9.10  (per-validator safety)
   - T-9.10' (failure preserves total_funds)
   - T-9.10″ (parallel order-independence)

   Together these establish that the post-fix `payWithdraw` cannot lose
   funds via the bug-#10 scenario: a failed transfer either leaves the
   validator's withdrawer entry intact (T-9.10 right disjunct) and the
   total funds invariant intact (T-9.10') for retry on a later block, or
   the transfer succeeds and the validator is correctly removed
   (T-9.10 left disjunct). The parallel-fold safety (T-9.10″) certifies
   that the production `unorderedParMap`-based withdrawal flow is
   schedule-agnostic at the state-projection level used by the Rocq model.
   ═══════════════════════════════════════════════════════════════════════════ *)
