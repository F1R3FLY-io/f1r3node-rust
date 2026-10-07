From Stdlib Require Import Bool.Bool Lists.List PeanoNat.
Import ListNotations.

Record removal_event : Type := {
  removal_channel : nat;
  removal_source : nat;
  removal_instance : nat
}.

Record settlement_state : Type := {
  remaining_stack : list nat;
  settlement_trace : list removal_event
}.

Definition settlement_removal
  (channel source instance : nat)
  : removal_event :=
  {| removal_channel := channel;
     removal_source := source;
     removal_instance := instance |}.

Definition settle_pop
  (state : settlement_state)
  (channel source instance : nat)
  : settlement_state :=
  {| remaining_stack := tl (remaining_stack state);
     settlement_trace :=
       settlement_trace state ++ [settlement_removal channel source instance] |}.

Definition replay_pop
  (state : settlement_state)
  (event : removal_event)
  : settlement_state :=
  {| remaining_stack := tl (remaining_stack state);
     settlement_trace := settlement_trace state ++ [event] |}.

Definition removal_conflicts
  (left right : removal_event)
  : bool :=
  Nat.eqb (removal_channel left) (removal_channel right) &&
  Nat.eqb (removal_instance left) (removal_instance right).

Definition soft_checkpoint
  (trace : list removal_event)
  : list removal_event * list removal_event :=
  (trace, []).

Theorem settlement_pop_is_event_visible :
  forall state channel source instance,
    settlement_trace (settle_pop state channel source instance) =
    settlement_trace state ++ [settlement_removal channel source instance].
Proof.
  reflexivity.
Qed.

Theorem replay_reproduces_settlement_removal :
  forall state channel source instance,
    replay_pop state (settlement_removal channel source instance) =
    settle_pop state channel source instance.
Proof.
  reflexivity.
Qed.

Theorem same_linear_instance_conflicts :
  forall channel left_source right_source instance,
    removal_conflicts
      (settlement_removal channel left_source instance)
      (settlement_removal channel right_source instance) = true.
Proof.
  intros.
  unfold removal_conflicts, settlement_removal.
  simpl.
  now rewrite !Nat.eqb_refl.
Qed.

Theorem distinct_linear_instances_do_not_conflict :
  forall channel left_source right_source left_instance right_instance,
    left_instance <> right_instance ->
    removal_conflicts
      (settlement_removal channel left_source left_instance)
      (settlement_removal channel right_source right_instance) = false.
Proof.
  intros channel left_source right_source left_instance right_instance Hneq.
  unfold removal_conflicts, settlement_removal.
  simpl.
  rewrite Nat.eqb_refl.
  apply Nat.eqb_neq in Hneq.
  exact Hneq.
Qed.

Theorem soft_checkpoint_returns_current_segment :
  forall trace,
    fst (soft_checkpoint trace) = trace.
Proof.
  reflexivity.
Qed.

Theorem soft_checkpoint_clears_active_segment :
  forall trace,
    snd (soft_checkpoint trace) = [].
Proof.
  reflexivity.
Qed.

Theorem consecutive_soft_checkpoints_are_disjoint :
  forall first second,
    fst (soft_checkpoint first) = first /\
    fst (soft_checkpoint (snd (soft_checkpoint first) ++ second)) = second.
Proof.
  intros.
  split; reflexivity.
Qed.

Theorem checkpoint_segments_reconstruct_execution_trace :
  forall first second,
    fst (soft_checkpoint first) ++
    fst (soft_checkpoint (snd (soft_checkpoint first) ++ second)) =
    first ++ second.
Proof.
  reflexivity.
Qed.

Theorem settlement_extends_trace_prefix :
  forall state channel source instance,
    exists suffix,
      settlement_trace (settle_pop state channel source instance) =
      settlement_trace state ++ suffix.
Proof.
  intros.
  exists [settlement_removal channel source instance].
  reflexivity.
Qed.

Print Assumptions settlement_pop_is_event_visible.
Print Assumptions replay_reproduces_settlement_removal.
Print Assumptions same_linear_instance_conflicts.
Print Assumptions distinct_linear_instances_do_not_conflict.
Print Assumptions soft_checkpoint_returns_current_segment.
Print Assumptions soft_checkpoint_clears_active_segment.
Print Assumptions consecutive_soft_checkpoints_are_disjoint.
Print Assumptions checkpoint_segments_reconstruct_execution_trace.
Print Assumptions settlement_extends_trace_prefix.
