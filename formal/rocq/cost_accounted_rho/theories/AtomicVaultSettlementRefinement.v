From Stdlib Require Import Arith.PeanoNat Bool.Bool Lia.
From CostAccountedRho Require Import VaultBackedCostLifecycle.

Definition native_apply
  (bound actual fee : nat)
  (ledger : vault_ledger)
  : option vault_ledger :=
  if Nat.leb bound (liquid ledger) &&
     Nat.leb (actual + fee) bound then
    Some {|
      liquid := liquid ledger - (actual + fee);
      held := held ledger;
      consumed := consumed ledger + actual;
      fee_paid := fee_paid ledger + fee;
      protocol_minted := protocol_minted ledger
    |}
  else None.

Theorem native_apply_refines_reserve_then_settle :
  forall ledger bound actual fee,
    native_apply bound actual fee ledger =
    lifecycle bound actual fee ledger.
Proof.
  intros [liquid0 held0 consumed0 fee0 minted0] bound actual fee.
  simpl.
  unfold native_apply, lifecycle, reserve.
  simpl.
  destruct (Nat.leb bound liquid0) eqn:Hbound.
  - apply Nat.leb_le in Hbound.
    unfold settle.
    simpl.
    assert (Hreserved : Nat.leb bound (held0 + bound) = true).
    { apply Nat.leb_le. lia. }
    rewrite Hreserved.
    destruct (Nat.leb (actual + fee) bound) eqn:Hwithin.
    + apply Nat.leb_le in Hwithin.
      simpl.
      replace (liquid0 - bound + (bound - (actual + fee)))
        with (liquid0 - (actual + fee)) by lia.
      replace (held0 + bound - bound) with held0 by lia.
      reflexivity.
    + reflexivity.
  - reflexivity.
Qed.

Theorem native_apply_success_is_visible_and_conserving :
  forall ledger bound actual fee applied,
    native_apply bound actual fee ledger = Some applied ->
    held applied = held ledger /\
    liquid applied = liquid ledger - (actual + fee) /\
    consumed applied = consumed ledger + actual /\
    fee_paid applied = fee_paid ledger + fee /\
    canonical_value applied = canonical_value ledger.
Proof.
  intros ledger bound actual fee applied Happly.
  unfold native_apply in Happly.
  destruct (Nat.leb bound (liquid ledger) &&
            Nat.leb (actual + fee) bound) eqn:Hvalid;
    try discriminate.
  apply andb_true_iff in Hvalid as [Hbound Hwithin].
  apply Nat.leb_le in Hbound.
  apply Nat.leb_le in Hwithin.
  inversion Happly; subst; clear Happly.
  repeat split; unfold canonical_value; simpl; lia.
Qed.

Theorem native_apply_insufficient_bound_has_no_result :
  forall ledger bound actual fee,
    liquid ledger < bound ->
    native_apply bound actual fee ledger = None.
Proof.
  intros ledger bound actual fee Hinsufficient.
  unfold native_apply.
  apply Nat.leb_gt in Hinsufficient.
  now rewrite Hinsufficient.
Qed.

Theorem native_apply_rejects_realized_over_bound :
  forall ledger bound actual fee,
    bound < actual + fee ->
    native_apply bound actual fee ledger = None.
Proof.
  intros ledger bound actual fee Hover.
  unfold native_apply.
  apply Nat.leb_gt in Hover.
  now rewrite Hover, andb_false_r.
Qed.

Definition aggregate_native_apply
  (actual_left fee_left actual_right fee_right : nat)
  (ledger : vault_ledger)
  : option vault_ledger :=
  let total := actual_left + fee_left + (actual_right + fee_right) in
  if Nat.leb total (liquid ledger) then
    Some {|
      liquid := liquid ledger - total;
      held := held ledger;
      consumed := consumed ledger + actual_left + actual_right;
      fee_paid := fee_paid ledger + fee_left + fee_right;
      protocol_minted := protocol_minted ledger
    |}
  else None.

Theorem aggregate_native_apply_is_order_independent :
  forall ledger actual_left fee_left actual_right fee_right,
    aggregate_native_apply actual_left fee_left actual_right fee_right ledger =
    aggregate_native_apply actual_right fee_right actual_left fee_left ledger.
Proof.
  intros ledger actual_left fee_left actual_right fee_right.
  unfold aggregate_native_apply.
  replace (actual_left + fee_left + (actual_right + fee_right))
    with (actual_right + fee_right + (actual_left + fee_left)) by lia.
  destruct (Nat.leb
    (actual_right + fee_right + (actual_left + fee_left))
    (liquid ledger)); try reflexivity.
  f_equal.
  destruct ledger; simpl.
  f_equal; lia.
Qed.

Theorem aggregate_native_apply_rejects_overdraw :
  forall ledger actual_left fee_left actual_right fee_right,
    liquid ledger < actual_left + fee_left + (actual_right + fee_right) ->
    aggregate_native_apply actual_left fee_left actual_right fee_right ledger = None.
Proof.
  intros ledger actual_left fee_left actual_right fee_right Hoverdraw.
  unfold aggregate_native_apply.
  apply Nat.leb_gt in Hoverdraw.
  now rewrite Hoverdraw.
Qed.

Print Assumptions native_apply_refines_reserve_then_settle.
Print Assumptions native_apply_success_is_visible_and_conserving.
Print Assumptions native_apply_insufficient_bound_has_no_result.
Print Assumptions native_apply_rejects_realized_over_bound.
Print Assumptions aggregate_native_apply_is_order_independent.
Print Assumptions aggregate_native_apply_rejects_overdraw.
