From Stdlib Require Import Arith Bool Lia.

Definition deploy_wake (accepted autopropose proposer_available signal_ready : bool) : bool :=
  accepted && negb (autopropose && proposer_available) && signal_ready.

Theorem deploy_wake_requires_admission :
  forall accepted autopropose proposer_available signal_ready,
    deploy_wake accepted autopropose proposer_available signal_ready = true <->
    accepted = true /\ (autopropose = false \/ proposer_available = false)
    /\ signal_ready = true.
Proof.
  unfold deploy_wake; intros [] [] [] []; simpl; intuition discriminate.
Qed.

Definition pending_proposal (has_work lag_allows cooling : bool) : bool :=
  (has_work && lag_allows) && negb cooling.

Theorem pending_decision_preserved :
  forall has_work lag_allows cooling,
    pending_proposal has_work lag_allows cooling =
    (has_work && negb cooling && lag_allows).
Proof.
  intros [] [] []; reflexivity.
Qed.

Theorem pending_decision_respects_cooldown :
  forall has_work lag_allows,
    pending_proposal has_work lag_allows true = false.
Proof.
  intros [] []; reflexivity.
Qed.

Definition recheck_delay
  (has_work lag_allows cooling other_lane_due : bool)
  (cooldown age duration_limit : nat) : option nat :=
  if has_work && lag_allows && cooling && negb other_lane_due
  then if age <? cooldown then Some (Nat.min (cooldown - age) duration_limit) else None
  else None.

Theorem recheck_requires_pending_cooldown :
  forall has_work lag_allows cooling other_lane_due cooldown age limit delay,
    recheck_delay has_work lag_allows cooling other_lane_due cooldown age limit = Some delay ->
    has_work = true /\ lag_allows = true /\ cooling = true /\ other_lane_due = false
    /\ age < cooldown /\ delay = Nat.min (cooldown - age) limit.
Proof.
  intros [] [] [] [] cooldown age limit delay H; cbn [recheck_delay andb negb] in H; try discriminate.
  destruct (age <? cooldown) eqn:Hage; try discriminate.
  apply Nat.ltb_lt in Hage.
  injection H as Hdelay.
  subst delay.
  repeat split; assumption || reflexivity.
Qed.

Definition next_sleep (check_interval minimum_recheck : nat) (recheck : option nat) : nat :=
  match recheck with
  | None => check_interval
  | Some delay => Nat.min (Nat.max minimum_recheck delay) check_interval
  end.

Theorem sleep_never_exceeds_interval :
  forall interval minimum_recheck recheck, next_sleep interval minimum_recheck recheck <= interval.
Proof.
  intros interval minimum_recheck [delay|]; simpl; lia.
Qed.

Theorem shortened_sleep_keeps_recheck_floor :
  forall interval minimum_recheck recheck,
    next_sleep interval minimum_recheck recheck < interval ->
    minimum_recheck <= next_sleep interval minimum_recheck recheck.
Proof.
  intros interval minimum_recheck [delay|]; simpl; lia.
Qed.

Theorem unsaturated_shortened_sleep_reaches_cooldown :
  forall interval minimum_recheck remaining limit,
    remaining <= limit ->
    next_sleep interval minimum_recheck (Some (Nat.min remaining limit)) < interval ->
    remaining <= next_sleep interval minimum_recheck (Some (Nat.min remaining limit)).
Proof.
  intros interval minimum_recheck remaining limit Hlimit.
  unfold next_sleep.
  rewrite (Nat.min_l remaining limit Hlimit).
  lia.
Qed.

Print Assumptions deploy_wake_requires_admission.
Print Assumptions pending_decision_preserved.
Print Assumptions pending_decision_respects_cooldown.
Print Assumptions recheck_requires_pending_cooldown.
Print Assumptions sleep_never_exceeds_interval.
Print Assumptions shortened_sleep_keeps_recheck_floor.
Print Assumptions unsaturated_shortened_sleep_reaches_cooldown.
