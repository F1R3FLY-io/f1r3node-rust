From Stdlib Require Import Arith.PeanoNat Bool.Bool Lia.

Record fee_cursor := {
  cursor_revision : nat;
  cursor_position : nat
}.

Record fee_cursor_plan := {
  cursor_scope : nat;
  cursor_expected : fee_cursor;
  cursor_next : fee_cursor
}.

Definition check_fee_cursor
  (maximum count target : nat) (current : fee_cursor) (plan : fee_cursor_plan)
  : option fee_cursor :=
  if (cursor_scope plan =? target) &&
     (cursor_revision (cursor_expected plan) =? cursor_revision current) &&
     (cursor_position (cursor_expected plan) =? cursor_position current) &&
     (cursor_revision (cursor_expected plan) <? maximum) &&
     (cursor_position (cursor_expected plan) <? count) &&
     (cursor_position (cursor_next plan) <? count) &&
     (cursor_revision (cursor_next plan) =? S (cursor_revision (cursor_expected plan)))
  then Some (cursor_next plan)
  else None.

Theorem checked_fee_cursor_is_scoped_bounded_successor :
  forall maximum count target current plan result,
    check_fee_cursor maximum count target current plan = Some result ->
    cursor_scope plan = target /\
    cursor_revision (cursor_expected plan) = cursor_revision current /\
    cursor_position (cursor_expected plan) = cursor_position current /\
    cursor_position current < count /\
    cursor_position result < count /\
    cursor_revision result = S (cursor_revision current) /\
    cursor_revision result <= maximum /\
    result = cursor_next plan.
Proof.
  intros maximum count target current plan result accepted.
  unfold check_fee_cursor in accepted.
  destruct (_ && _) eqn:guards; [|discriminate].
  injection accepted as same. subst result.
  repeat rewrite andb_true_iff in guards.
  repeat rewrite Nat.eqb_eq in guards.
  repeat rewrite Nat.ltb_lt in guards.
  destruct guards as [[[[[[scope revision] position] bounded] before] after] successor].
  repeat split; try assumption; lia.
Qed.

Theorem fee_cursor_rejects_reuse_after_success :
  forall maximum count target current plan result,
    check_fee_cursor maximum count target current plan = Some result ->
    check_fee_cursor maximum count target result plan = None.
Proof.
  intros maximum count target current plan result accepted.
  pose proof (checked_fee_cursor_is_scoped_bounded_successor
    maximum count target current plan result accepted) as facts.
  destruct (check_fee_cursor maximum count target result plan) as [again|] eqn:retry;
    [|reflexivity].
  pose proof (checked_fee_cursor_is_scoped_bounded_successor
    maximum count target result plan again retry) as retry_facts.
  decompose [and] facts. decompose [and] retry_facts. lia.
Qed.

Theorem fee_cursor_rejects_exhausted_revision :
  forall maximum count target current plan,
    maximum <= cursor_revision current ->
    check_fee_cursor maximum count target current plan = None.
Proof.
  intros maximum count target current plan exhausted.
  destruct (check_fee_cursor maximum count target current plan) as [result|] eqn:checked;
    [|reflexivity].
  pose proof (checked_fee_cursor_is_scoped_bounded_successor
    maximum count target current plan result checked) as facts.
  decompose [and] facts. lia.
Qed.

Theorem fee_cursor_rejects_foreign_scope :
  forall maximum count target current plan,
    cursor_scope plan <> target ->
    check_fee_cursor maximum count target current plan = None.
Proof.
  intros maximum count target current plan different.
  destruct (check_fee_cursor maximum count target current plan) as [result|] eqn:checked;
    [|reflexivity].
  pose proof (checked_fee_cursor_is_scoped_bounded_successor
    maximum count target current plan result checked) as [same _].
  contradiction.
Qed.

Inductive fee_cursor_history (maximum count target : nat) : fee_cursor -> nat -> fee_cursor -> Prop :=
| fee_cursor_history_empty : forall cursor,
    fee_cursor_history maximum count target cursor 0 cursor
| fee_cursor_history_step : forall first before after steps plan,
    fee_cursor_history maximum count target first steps before ->
    check_fee_cursor maximum count target before plan = Some after ->
    fee_cursor_history maximum count target first (S steps) after.

Theorem fee_cursor_history_counts_successful_settlements :
  forall maximum count target first steps last,
    fee_cursor_history maximum count target first steps last ->
    cursor_revision last = cursor_revision first + steps.
Proof.
  intros maximum count target first steps last history.
  induction history as [cursor|first before after steps plan history IH checked].
  - lia.
  - pose proof (checked_fee_cursor_is_scoped_bounded_successor
      maximum count target before plan after checked) as facts.
    decompose [and] facts. lia.
Qed.

Theorem fee_cursor_old_plan_stays_stale_after_any_positive_history :
  forall maximum count target first steps last plan,
    fee_cursor_history maximum count target first steps last ->
    0 < steps ->
    cursor_revision (cursor_expected plan) = cursor_revision first ->
    check_fee_cursor maximum count target last plan = None.
Proof.
  intros maximum count target first steps last plan history positive original.
  pose proof (fee_cursor_history_counts_successful_settlements
    maximum count target first steps last history) as progression.
  destruct (check_fee_cursor maximum count target last plan) as [result|] eqn:checked;
    [|reflexivity].
  pose proof (checked_fee_cursor_is_scoped_bounded_successor
    maximum count target last plan result checked) as facts.
  decompose [and] facts. lia.
Qed.

Print Assumptions checked_fee_cursor_is_scoped_bounded_successor.
Print Assumptions fee_cursor_rejects_reuse_after_success.
Print Assumptions fee_cursor_rejects_exhausted_revision.
Print Assumptions fee_cursor_rejects_foreign_scope.
Print Assumptions fee_cursor_history_counts_successful_settlements.
Print Assumptions fee_cursor_old_plan_stays_stale_after_any_positive_history.
