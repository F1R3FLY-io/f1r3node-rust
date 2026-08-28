From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Lia.
Import ListNotations.

Definition rotating_recovery_leader
  (validator_count finalized_height recovery_round : nat)
  : nat :=
  (finalized_height + recovery_round) mod validator_count.

Definition recovery_round_authorized
  (validator_count finalized_height recovery_round proposer : nat)
  : Prop :=
  validator_count > 0 /\
  proposer = rotating_recovery_leader
    validator_count finalized_height recovery_round.

Theorem rotating_recovery_leader_in_committee :
  forall validator_count finalized_height recovery_round,
    validator_count > 0 ->
    rotating_recovery_leader
      validator_count finalized_height recovery_round < validator_count.
Proof.
  intros validator_count finalized_height recovery_round Hcount.
  unfold rotating_recovery_leader.
  apply Nat.mod_upper_bound.
  lia.
Qed.

Theorem recovery_round_authorization_unique :
  forall validator_count finalized_height recovery_round proposer_a proposer_b,
    recovery_round_authorized
      validator_count finalized_height recovery_round proposer_a ->
    recovery_round_authorized
      validator_count finalized_height recovery_round proposer_b ->
    proposer_a = proposer_b.
Proof.
  intros validator_count finalized_height recovery_round proposer_a proposer_b
    [_ Ha] [_ Hb].
  rewrite Ha, Hb.
  reflexivity.
Qed.

Record finality_progress := {
  observed_floor : nat;
  attempted_round : option nat
}.

Definition recovery_round_due
  (progress : finality_progress)
  (recovery_round : nat)
  : bool :=
  match attempted_round progress with
  | None => true
  | Some attempted => negb (Nat.eqb attempted recovery_round)
  end.

Definition record_recovery_attempt
  (progress : finality_progress)
  (recovery_round : nat)
  : finality_progress :=
  {| observed_floor := observed_floor progress;
     attempted_round := Some recovery_round |}.

Definition observe_floor
  (progress : finality_progress)
  (current_floor : nat)
  : finality_progress :=
  if Nat.eqb (observed_floor progress) current_floor
  then progress
  else {| observed_floor := current_floor; attempted_round := None |}.

Theorem recorded_recovery_round_is_not_due_twice :
  forall progress recovery_round,
    recovery_round_due
      (record_recovery_attempt progress recovery_round)
      recovery_round = false.
Proof.
  intros progress recovery_round.
  unfold recovery_round_due, record_recovery_attempt.
  simpl.
  rewrite Nat.eqb_refl.
  reflexivity.
Qed.

Theorem unchanged_floor_preserves_recovery_history :
  forall progress,
    observe_floor progress (observed_floor progress) = progress.
Proof.
  intros progress.
  unfold observe_floor.
  rewrite Nat.eqb_refl.
  reflexivity.
Qed.

Theorem advanced_floor_resets_recovery_history :
  forall progress current_floor,
    observed_floor progress <> current_floor ->
    attempted_round (observe_floor progress current_floor) = None.
Proof.
  intros progress current_floor Hchanged.
  unfold observe_floor.
  apply Nat.eqb_neq in Hchanged.
  rewrite Hchanged.
  reflexivity.
Qed.

Definition enqueue_bounded {A : Type}
  (capacity : nat)
  (queue : list A)
  (item : A)
  : list A :=
  if Nat.ltb (length queue) capacity
  then queue ++ [item]
  else queue.

Theorem bounded_enqueue_preserves_capacity :
  forall (A : Type) capacity (queue : list A) item,
    length queue <= capacity ->
    length (enqueue_bounded capacity queue item) <= capacity.
Proof.
  intros A capacity queue item Hbounded.
  unfold enqueue_bounded.
  destruct (Nat.ltb (length queue) capacity) eqn:Hspace.
  - apply Nat.ltb_lt in Hspace.
    rewrite length_app.
    simpl.
    lia.
  - exact Hbounded.
Qed.

Definition heartbeat_backpressure_contract : Prop :=
  (forall validator_count finalized_height recovery_round,
    validator_count > 0 ->
    rotating_recovery_leader
      validator_count finalized_height recovery_round < validator_count)
  /\
  (forall validator_count finalized_height recovery_round proposer_a proposer_b,
    recovery_round_authorized
      validator_count finalized_height recovery_round proposer_a ->
    recovery_round_authorized
      validator_count finalized_height recovery_round proposer_b ->
    proposer_a = proposer_b)
  /\
  (forall progress recovery_round,
    recovery_round_due
      (record_recovery_attempt progress recovery_round)
      recovery_round = false)
  /\
  (forall progress,
    observe_floor progress (observed_floor progress) = progress)
  /\
  (forall progress current_floor,
    observed_floor progress <> current_floor ->
    attempted_round (observe_floor progress current_floor) = None)
  /\
  (forall (A : Type) capacity (queue : list A) item,
    length queue <= capacity ->
    length (enqueue_bounded capacity queue item) <= capacity).

Theorem heartbeat_backpressure_end_to_end :
  heartbeat_backpressure_contract.
Proof.
  unfold heartbeat_backpressure_contract.
  repeat split.
  - exact rotating_recovery_leader_in_committee.
  - exact recovery_round_authorization_unique.
  - exact recorded_recovery_round_is_not_due_twice.
  - exact unchanged_floor_preserves_recovery_history.
  - exact advanced_floor_resets_recovery_history.
  - exact bounded_enqueue_preserves_capacity.
Qed.

Print Assumptions heartbeat_backpressure_end_to_end.
