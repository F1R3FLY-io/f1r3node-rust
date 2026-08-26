From Stdlib Require Import List.
Import ListNotations.

From CostAccountedRho Require Import MintingInjection.
From CostAccountedRho Require Import WalletNaming.

Definition remove_validator (v : pubkey) (validators : list pubkey) : list pubkey :=
  filter (fun candidate => negb (pubkey_eqb v candidate)) validators.

Definition redeem_unhalt (state : pos_state) (v : pubkey) : pos_state :=
  {| pb_balance := pb_balance state;
     pb_minted := pb_minted state;
     pb_halted := remove_validator v (pb_halted state) |}.

Lemma pubkey_eqb_refl :
  forall v, pubkey_eqb v v = true.
Proof.
  intro v. apply pubkey_eqb_true_iff. reflexivity.
Qed.

Lemma remove_validator_excludes_target :
  forall validators v,
    ~ In v (remove_validator v validators).
Proof.
  intros validators v Hin.
  unfold remove_validator in Hin.
  apply filter_In in Hin as [_ Hkept].
  rewrite pubkey_eqb_refl in Hkept.
  discriminate.
Qed.

Lemma pubkey_inb_remove_validator :
  forall validators v,
    pubkey_inb v (remove_validator v validators) = false.
Proof.
  intros validators v.
  destruct (pubkey_inb v (remove_validator v validators)) eqn:Hmembership.
  - apply pubkey_inb_true_iff in Hmembership.
    exfalso.
    apply (remove_validator_excludes_target validators v).
    exact Hmembership.
  - reflexivity.
Qed.

Lemma mint_key_inb_false_of_absence :
  forall key ledger,
    ~ In key ledger -> mint_key_inb key ledger = false.
Proof.
  intros key ledger Habsent.
  destruct (mint_key_inb key ledger) eqn:Hmembership.
  - apply mint_key_inb_true_iff in Hmembership.
    contradiction.
  - reflexivity.
Qed.

Theorem redemption_preserves_mint_ledger :
  forall state v,
    pb_minted (redeem_unhalt state v) = pb_minted state.
Proof.
  reflexivity.
Qed.

Theorem redemption_preserves_vault_balance :
  forall state v owner,
    balance_of (redeem_unhalt state v) owner = balance_of state owner.
Proof.
  reflexivity.
Qed.

Theorem redemption_removes_only_the_target_halt :
  forall state v,
    ~ In v (pb_halted (redeem_unhalt state v)).
Proof.
  intros state v.
  apply remove_validator_excludes_target.
Qed.

Theorem recorded_epoch_mint_is_identity :
  forall state v epoch amount,
    In (v, epoch) (pb_minted state) ->
    epoch_mint state v epoch amount = state.
Proof.
  intros state v epoch amount Hrecorded.
  unfold epoch_mint, mint_eligible.
  assert (Hmembership : mint_key_inb (v, epoch) (pb_minted state) = true).
  { apply mint_key_inb_true_iff. exact Hrecorded. }
  rewrite Hmembership, Bool.andb_false_r.
  reflexivity.
Qed.

Theorem redemption_cannot_remint_recorded_epoch :
  forall state v epoch amount,
    In (v, epoch) (pb_minted state) ->
    epoch_mint (redeem_unhalt state v) v epoch amount
      = redeem_unhalt state v.
Proof.
  intros state v epoch amount Hrecorded.
  apply recorded_epoch_mint_is_identity.
  exact Hrecorded.
Qed.

Theorem redemption_enables_exactly_one_fresh_epoch_credit :
  forall state v epoch amount,
    ~ In (v, epoch) (pb_minted state) ->
    balance_of (epoch_mint (redeem_unhalt state v) v epoch amount) v
      = balance_of state v + amount.
Proof.
  intros state v epoch amount Habsent.
  unfold epoch_mint, mint_eligible, redeem_unhalt.
  simpl.
  rewrite pubkey_inb_remove_validator.
  rewrite (mint_key_inb_false_of_absence (v, epoch) (pb_minted state) Habsent).
  simpl.
  unfold balance_of, credit.
  simpl.
  rewrite pubkey_eqb_refl.
  reflexivity.
Qed.

Theorem redemption_fresh_epoch_replay_is_idempotent :
  forall state v epoch amount,
    ~ In (v, epoch) (pb_minted state) ->
    let minted := epoch_mint (redeem_unhalt state v) v epoch amount in
    epoch_mint minted v epoch amount = minted.
Proof.
  intros state v epoch amount Habsent.
  simpl.
  apply recorded_epoch_mint_is_identity.
  unfold epoch_mint, mint_eligible, redeem_unhalt.
  simpl.
  rewrite pubkey_inb_remove_validator.
  rewrite (mint_key_inb_false_of_absence (v, epoch) (pb_minted state) Habsent).
  simpl.
  left. reflexivity.
Qed.

Print Assumptions redemption_preserves_mint_ledger.
Print Assumptions redemption_preserves_vault_balance.
Print Assumptions redemption_removes_only_the_target_halt.
Print Assumptions redemption_cannot_remint_recorded_epoch.
Print Assumptions redemption_enables_exactly_one_fresh_epoch_credit.
Print Assumptions redemption_fresh_epoch_replay_is_idempotent.
