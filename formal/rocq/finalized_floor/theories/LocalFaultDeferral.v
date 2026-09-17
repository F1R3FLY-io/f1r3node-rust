From Stdlib Require Import Arith.Arith.

Inductive ValidationDisposition :=
| Pending
| Accepted
| ObjectiveInvalid.

Inductive QueueState :=
| Blocked
| Ready
| InFlight
| Deferred
| Terminal.

Record RecoveryState := {
  queue_state : QueueState;
  validation_disposition : ValidationDisposition;
  recovery_outstanding : bool
}.

Definition defer_local_fault (state : RecoveryState) : RecoveryState :=
  {| queue_state := Deferred;
     validation_disposition := validation_disposition state;
     recovery_outstanding := true |}.

Definition recovery_request_failed (state : RecoveryState) : RecoveryState := state.

Definition recovery_request_succeeded (state : RecoveryState) : RecoveryState :=
  {| queue_state := Ready;
     validation_disposition := validation_disposition state;
     recovery_outstanding := false |}.

Definition outstanding_count (state : RecoveryState) : nat :=
  if recovery_outstanding state then 1 else 0.

Definition regular_parent_satisfied (state : RecoveryState) : bool :=
  match validation_disposition state with
  | Accepted => true
  | Pending => false
  | ObjectiveInvalid => false
  end.

Theorem local_fault_preserves_consensus_disposition :
  forall state,
    validation_disposition (defer_local_fault state) =
    validation_disposition state.
Proof.
  reflexivity.
Qed.

Theorem local_fault_leaves_ready_queue :
  forall state,
    queue_state (defer_local_fault state) <> Ready.
Proof.
  intros state impossible.
  discriminate.
Qed.

Theorem local_fault_opens_exactly_one_recovery :
  forall state,
    outstanding_count (defer_local_fault state) = 1.
Proof.
  reflexivity.
Qed.

Theorem failed_recovery_does_not_restore_ready_state :
  forall state,
    queue_state state = Deferred ->
    queue_state (recovery_request_failed state) <> Ready.
Proof.
  intros state deferred.
  unfold recovery_request_failed.
  rewrite deferred.
  discriminate.
Qed.

Theorem successful_recovery_reopens_without_invalidating :
  forall state,
    queue_state (recovery_request_succeeded state) = Ready /\
    validation_disposition (recovery_request_succeeded state) =
      validation_disposition state /\
    recovery_outstanding (recovery_request_succeeded state) = false.
Proof.
  intros state.
  repeat split.
Qed.

Theorem regular_child_requires_valid_parent :
  forall state,
    regular_parent_satisfied state = true ->
    validation_disposition state = Accepted.
Proof.
  intros [queue disposition outstanding].
  destruct disposition; simpl; intros satisfied.
  - discriminate.
  - reflexivity.
  - discriminate.
Qed.

Theorem objective_invalid_parent_does_not_release_regular_child :
  forall queue outstanding,
    regular_parent_satisfied
      {| queue_state := queue;
         validation_disposition := ObjectiveInvalid;
         recovery_outstanding := outstanding |} = false.
Proof.
  reflexivity.
Qed.
