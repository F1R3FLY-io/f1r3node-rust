From Stdlib Require Import Arith.PeanoNat Bool.Bool Lia.

Inductive reserve_result : Type :=
  | ReserveGranted (new_used : nat)
  | ReserveLimitRejected
  | ReserveOverflowRejected.

Definition checked_reserve
  (counter_max limit used request : nat)
  : reserve_result :=
  if counter_max <? used then ReserveOverflowRejected
  else if counter_max - used <? request then ReserveOverflowRejected
  else if limit <? used then ReserveLimitRejected
  else if limit - used <? request then ReserveLimitRejected
  else ReserveGranted (used + request).

Definition reserve_used (old_used : nat) (result : reserve_result) : nat :=
  match result with
  | ReserveGranted new_used => new_used
  | ReserveLimitRejected => old_used
  | ReserveOverflowRejected => old_used
  end.

Record budget_state := {
  budget_counter_max : nat;
  budget_phase_limit : nat;
  budget_used : nat
}.

Definition budget_with_used (state : budget_state) (used : nat) : budget_state :=
  {|
    budget_counter_max := budget_counter_max state;
    budget_phase_limit := budget_phase_limit state;
    budget_used := used
  |}.

Definition reserve_transition
  (state : budget_state)
  (request : nat)
  : budget_state * reserve_result :=
  let result := checked_reserve
    (budget_counter_max state)
    (budget_phase_limit state)
    (budget_used state)
    request in
  match result with
  | ReserveGranted next_used => (budget_with_used state next_used, result)
  | ReserveLimitRejected => (state, result)
  | ReserveOverflowRejected => (state, result)
  end.

Theorem checked_reserve_granted_bounds :
  forall counter_max limit used request next_used,
    checked_reserve counter_max limit used request =
      ReserveGranted next_used ->
    next_used = used + request /\
    next_used <= counter_max /\
    next_used <= limit.
Proof.
  intros counter_max limit used request next_used granted.
  unfold checked_reserve in granted.
  destruct (counter_max <? used) eqn:counter_before; try discriminate.
  destruct (counter_max - used <? request) eqn:counter_after; try discriminate.
  destruct (limit <? used) eqn:limit_before; try discriminate.
  destruct (limit - used <? request) eqn:limit_after; try discriminate.
  inversion granted; subst next_used.
  apply Nat.ltb_ge in counter_before.
  apply Nat.ltb_ge in counter_after.
  apply Nat.ltb_ge in limit_before.
  apply Nat.ltb_ge in limit_after.
  repeat split; lia.
Qed.

Theorem successful_reserve_transition_is_bounded :
  forall state request next_state next_used,
    reserve_transition state request =
      (next_state, ReserveGranted next_used) ->
    budget_used next_state = next_used /\
    next_used <= budget_counter_max state /\
    next_used <= budget_phase_limit state.
Proof.
  intros state request next_state next_used transition.
  unfold reserve_transition in transition.
  remember (checked_reserve
    (budget_counter_max state)
    (budget_phase_limit state)
    (budget_used state)
    request) as result eqn:reserved.
  destruct result as [granted_used | |]; try discriminate.
  inversion transition; subst next_state next_used.
  simpl.
  pose proof (checked_reserve_granted_bounds
    (budget_counter_max state)
    (budget_phase_limit state)
    (budget_used state)
    request granted_used (eq_sym reserved)) as bounds.
  tauto.
Qed.

Theorem rejected_reserve_does_not_mutate :
  forall state request rejection,
    (rejection = ReserveLimitRejected \/
     rejection = ReserveOverflowRejected) ->
    checked_reserve
      (budget_counter_max state)
      (budget_phase_limit state)
      (budget_used state)
      request = rejection ->
    reserve_transition state request = (state, rejection).
Proof.
  intros state request rejection rejected decision.
  destruct rejected as [limit_rejected | overflow_rejected];
    subst rejection; unfold reserve_transition; rewrite decision; reflexivity.
Qed.

Theorem counter_overflow_is_rejected :
  forall counter_max limit used request,
    counter_max < used \/ counter_max - used < request ->
    checked_reserve counter_max limit used request =
      ReserveOverflowRejected.
Proof.
  intros counter_max limit used request overflow.
  unfold checked_reserve.
  destruct (counter_max <? used) eqn:before.
  - reflexivity.
  - apply Nat.ltb_ge in before.
    destruct overflow as [already_overflowed | addition_overflows].
    + lia.
    + assert ((counter_max - used <? request) = true) as comparison.
      { apply Nat.ltb_lt. exact addition_overflows. }
      rewrite comparison.
      reflexivity.
Qed.

Theorem phase_limit_exhaustion_is_rejected :
  forall counter_max limit used request,
    used <= counter_max ->
    request <= counter_max - used ->
    (limit < used \/ limit - used < request) ->
    checked_reserve counter_max limit used request = ReserveLimitRejected.
Proof.
  intros counter_max limit used request used_bounded request_bounded exhausted.
  unfold checked_reserve.
  assert ((counter_max <? used) = false) as counter_before.
  { apply Nat.ltb_ge. exact used_bounded. }
  assert ((counter_max - used <? request) = false) as counter_after.
  { apply Nat.ltb_ge. exact request_bounded. }
  rewrite counter_before, counter_after.
  destruct (limit <? used) eqn:before.
  - reflexivity.
  - apply Nat.ltb_ge in before.
    destruct exhausted as [already_exhausted | addition_exhausts].
    + lia.
    + assert ((limit - used <? request) = true) as comparison.
      { apply Nat.ltb_lt. exact addition_exhausts. }
      rewrite comparison.
      reflexivity.
Qed.

Record execution_state := {
  execution_budget : budget_state;
  execution_economic_balance : nat;
  execution_semantic_state : nat
}.

Definition host_reserve
  (state : execution_state)
  (request : nat)
  : execution_state * reserve_result :=
  let '(next_budget, result) := reserve_transition
    (execution_budget state) request in
  ({|
    execution_budget := next_budget;
    execution_economic_balance := execution_economic_balance state;
    execution_semantic_state := execution_semantic_state state
  |}, result).

Theorem host_reserve_preserves_economic_balance :
  forall state request,
    execution_economic_balance (fst (host_reserve state request)) =
      execution_economic_balance state.
Proof.
  intros state request.
  unfold host_reserve.
  destruct (reserve_transition (execution_budget state) request).
  reflexivity.
Qed.

Theorem host_reserve_preserves_semantic_state :
  forall state request,
    execution_semantic_state (fst (host_reserve state request)) =
      execution_semantic_state state.
Proof.
  intros state request.
  unfold host_reserve.
  destruct (reserve_transition (execution_budget state) request).
  reflexivity.
Qed.
