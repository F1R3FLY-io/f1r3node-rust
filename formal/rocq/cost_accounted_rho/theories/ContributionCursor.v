From Stdlib Require Import Arith.PeanoNat Bool.Bool Lia.
From CostAccountedRho Require Import FeeCursorTransition.

Definition check_contribution_cursor maximum count target amount current plan :=
  match amount, plan with
  | 0, None =>
      if (cursor_revision current <=? maximum) && (cursor_position current <? count)
      then Some current else None
  | S _, Some transition => check_fee_cursor maximum count target current transition
  | _, _ => None
  end.

Theorem zero_contribution_has_no_cursor_write : forall maximum count target current plan result,
  check_contribution_cursor maximum count target 0 current plan = Some result ->
  plan = None /\ result = current.
Proof.
  intros maximum count target current [transition|] result checked; cbn in checked; [discriminate|].
  destruct (_ && _) eqn:valid; [|discriminate]. inversion checked. auto.
Qed.

Theorem zero_contribution_accepts_every_valid_revision : forall maximum count target current,
  cursor_revision current <= maximum -> cursor_position current < count ->
  check_contribution_cursor maximum count target 0 current None = Some current.
Proof.
  intros maximum count target current bounded position.
  change ((if (cursor_revision current <=? maximum) && (cursor_position current <? count)
    then Some current else None) = Some current).
  apply Nat.leb_le in bounded. apply Nat.ltb_lt in position. rewrite bounded, position. reflexivity.
Qed.

Theorem positive_contribution_uses_existing_cursor_check : forall maximum count target amount current plan,
  0 < amount ->
  check_contribution_cursor maximum count target amount current (Some plan) =
    check_fee_cursor maximum count target current plan.
Proof. intros maximum count target [|amount] current plan positive; [lia|reflexivity]. Qed.

Theorem positive_contribution_requires_a_transition : forall maximum count target amount current,
  0 < amount -> check_contribution_cursor maximum count target amount current None = None.
Proof. intros maximum count target [|amount] current positive; [lia|reflexivity]. Qed.

Theorem checked_contribution_counts_positive_amounts : forall maximum count target amount current plan result,
  check_contribution_cursor maximum count target amount current plan = Some result ->
  cursor_revision result = cursor_revision current + (if amount =? 0 then 0 else 1).
Proof.
  intros maximum count target [|amount] current plan result checked.
  - apply zero_contribution_has_no_cursor_write in checked. destruct checked as [_ same]. subst. cbn. lia.
  - destruct plan as [transition|]; cbn in checked; [|discriminate].
    pose proof (checked_fee_cursor_is_scoped_bounded_successor maximum count target current transition result checked) as facts.
    decompose [and] facts. cbn. lia.
Qed.

Inductive contribution_cursor_history maximum count target : fee_cursor -> nat -> fee_cursor -> Prop :=
| contribution_history_empty : forall current,
    contribution_cursor_history maximum count target current 0 current
| contribution_history_step : forall first before after positives amount plan,
    contribution_cursor_history maximum count target first positives before ->
    check_contribution_cursor maximum count target amount before plan = Some after ->
    contribution_cursor_history maximum count target first
      (positives + if amount =? 0 then 0 else 1) after.

Theorem contribution_history_counts_only_positive_settlements : forall maximum count target first positives last,
  contribution_cursor_history maximum count target first positives last ->
  cursor_revision last = cursor_revision first + positives.
Proof.
  intros maximum count target first positives last history.
  induction history as [current|first before after positives amount plan history IH checked].
  - lia.
  - pose proof (checked_contribution_counts_positive_amounts maximum count target amount before plan after checked).
    lia.
Qed.

Theorem zero_contribution_cannot_overwrite_a_positive_successor :
  forall maximum count target current plan next,
  check_fee_cursor maximum count target current plan = Some next ->
  check_contribution_cursor maximum count target 0 next None = Some next.
Proof.
  intros maximum count target current plan next checked.
  pose proof (checked_fee_cursor_is_scoped_bounded_successor maximum count target current plan next checked) as facts.
  apply zero_contribution_accepts_every_valid_revision; decompose [and] facts; assumption.
Qed.

Theorem zero_contribution_does_not_make_an_old_positive_plan_current :
  forall maximum count target current plan next amount,
  check_fee_cursor maximum count target current plan = Some next ->
  0 < amount ->
  check_contribution_cursor maximum count target amount next (Some plan) = None.
Proof.
  intros maximum count target current plan next amount checked positive.
  rewrite positive_contribution_uses_existing_cursor_check by assumption.
  eapply fee_cursor_rejects_reuse_after_success; eauto.
Qed.

Definition check_funding_cursor_pair maximum count resource_scope fee_scope
  resource_amount fee_amount resource_current fee_current resource_plan fee_plan :=
  if resource_scope =? fee_scope then None else
  match check_contribution_cursor maximum count resource_scope resource_amount resource_current resource_plan,
        check_contribution_cursor maximum count fee_scope fee_amount fee_current fee_plan with
  | Some resource_next, Some fee_next => Some (resource_next, fee_next)
  | _, _ => None
  end.

Theorem funding_cursor_pair_requires_both_checks :
  forall maximum count resource_scope fee_scope resource_amount fee_amount
    resource_current fee_current resource_plan fee_plan resource_next fee_next,
  check_funding_cursor_pair maximum count resource_scope fee_scope resource_amount fee_amount
    resource_current fee_current resource_plan fee_plan = Some (resource_next, fee_next) ->
  resource_scope <> fee_scope /\
  check_contribution_cursor maximum count resource_scope resource_amount resource_current resource_plan = Some resource_next /\
  check_contribution_cursor maximum count fee_scope fee_amount fee_current fee_plan = Some fee_next.
Proof.
  intros maximum count resource_scope fee_scope resource_amount fee_amount
    resource_current fee_current resource_plan fee_plan resource_next fee_next checked.
  unfold check_funding_cursor_pair in checked.
  destruct (resource_scope =? fee_scope) eqn:distinct; [discriminate|].
  apply Nat.eqb_neq in distinct.
  destruct (check_contribution_cursor maximum count resource_scope resource_amount resource_current resource_plan) eqn:resource; [|discriminate].
  destruct (check_contribution_cursor maximum count fee_scope fee_amount fee_current fee_plan) eqn:fee; [|discriminate].
  inversion checked; subst. auto.
Qed.

Theorem funding_cursor_pair_counts_each_positive_role :
  forall maximum count resource_scope fee_scope resource_amount fee_amount
    resource_current fee_current resource_plan fee_plan resource_next fee_next,
  check_funding_cursor_pair maximum count resource_scope fee_scope resource_amount fee_amount
    resource_current fee_current resource_plan fee_plan = Some (resource_next, fee_next) ->
  cursor_revision resource_next = cursor_revision resource_current + (if resource_amount =? 0 then 0 else 1) /\
  cursor_revision fee_next = cursor_revision fee_current + (if fee_amount =? 0 then 0 else 1).
Proof.
  intros maximum count resource_scope fee_scope resource_amount fee_amount
    resource_current fee_current resource_plan fee_plan resource_next fee_next checked.
  apply funding_cursor_pair_requires_both_checks in checked.
  destruct checked as [_ [resource fee]]. split;
    eapply checked_contribution_counts_positive_amounts; eauto.
Qed.

Theorem funding_cursor_pair_rejects_reuse_of_any_positive_role :
  forall maximum count resource_scope fee_scope resource_amount fee_amount
    resource_current fee_current resource_plan fee_plan resource_next fee_next,
  check_funding_cursor_pair maximum count resource_scope fee_scope resource_amount fee_amount
    resource_current fee_current resource_plan fee_plan = Some (resource_next, fee_next) ->
  0 < resource_amount + fee_amount ->
  check_funding_cursor_pair maximum count resource_scope fee_scope resource_amount fee_amount
    resource_next fee_next resource_plan fee_plan = None.
Proof.
  intros maximum count resource_scope fee_scope resource_amount fee_amount
    resource_current fee_current resource_plan fee_plan resource_next fee_next checked positive.
  apply funding_cursor_pair_requires_both_checks in checked.
  destruct checked as [distinct [resource fee]].
  unfold check_funding_cursor_pair.
  destruct (resource_scope =? fee_scope); [reflexivity|].
  destruct resource_amount as [|resource_amount].
  - destruct fee_amount as [|fee_amount]; [lia|].
    destruct fee_plan as [fee_plan|]; [|discriminate].
    cbn in fee.
    pose proof (zero_contribution_does_not_make_an_old_positive_plan_current
      maximum count fee_scope fee_current fee_plan fee_next (S fee_amount) fee ltac:(lia)) as stale.
    rewrite stale. destruct (check_contribution_cursor maximum count resource_scope 0 resource_next resource_plan); reflexivity.
  - destruct resource_plan as [resource_plan|]; [|discriminate].
    cbn in resource.
    pose proof (zero_contribution_does_not_make_an_old_positive_plan_current
      maximum count resource_scope resource_current resource_plan resource_next (S resource_amount) resource ltac:(lia)) as stale.
    rewrite stale. reflexivity.
Qed.

Print Assumptions funding_cursor_pair_requires_both_checks.
Print Assumptions funding_cursor_pair_counts_each_positive_role.
Print Assumptions funding_cursor_pair_rejects_reuse_of_any_positive_role.
Print Assumptions zero_contribution_has_no_cursor_write.
Print Assumptions zero_contribution_accepts_every_valid_revision.
Print Assumptions positive_contribution_uses_existing_cursor_check.
Print Assumptions positive_contribution_requires_a_transition.
Print Assumptions checked_contribution_counts_positive_amounts.
Print Assumptions contribution_history_counts_only_positive_settlements.
Print Assumptions zero_contribution_cannot_overwrite_a_positive_successor.
Print Assumptions zero_contribution_does_not_make_an_old_positive_plan_current.
