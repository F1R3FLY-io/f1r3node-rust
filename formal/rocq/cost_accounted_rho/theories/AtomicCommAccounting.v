From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

Inductive trigger_side : Type :=
  | ProducerTriggered
  | ConsumerTriggered.

Inductive io_introduction : Type :=
  | IntroduceSend (channel : nat)
  | IntroduceReceive (channels : list nat).

Record comm_observation := {
  comm_identity : nat;
  comm_arity : nat;
  comm_trigger : trigger_side
}.

Definition comm_charge (_ : comm_observation) : nat := 1.

Definition comm_trace_cost (trace : list comm_observation) : nat :=
  fold_right (fun observation total => comm_charge observation + total) 0 trace.

Definition introduce_io
  (trace : list comm_observation)
  (_ : io_introduction)
  : list comm_observation := trace.

Definition reserve_then_commit {State : Type}
  (commit : State -> comm_observation -> State)
  (budget consumed : nat)
  (state : State)
  (observation : comm_observation)
  : State * nat * bool :=
  if consumed <? budget
  then (commit state observation, S consumed, true)
  else (state, consumed, false).

Theorem unmatched_introduction_costs_zero : forall trace introduction,
  comm_trace_cost (introduce_io trace introduction) = comm_trace_cost trace.
Proof.
  reflexivity.
Qed.

Theorem committed_comm_costs_exactly_one : forall identity arity trigger,
  comm_trace_cost
    [{| comm_identity := identity;
        comm_arity := arity;
        comm_trigger := trigger |}] = 1.
Proof.
  reflexivity.
Qed.

Theorem trigger_side_does_not_change_cost : forall identity arity,
  comm_trace_cost
    [{| comm_identity := identity;
        comm_arity := arity;
        comm_trigger := ProducerTriggered |}] =
  comm_trace_cost
    [{| comm_identity := identity;
        comm_arity := arity;
        comm_trigger := ConsumerTriggered |}].
Proof.
  reflexivity.
Qed.

Theorem join_arity_does_not_multiply_cost : forall identity left_arity right_arity trigger,
  comm_trace_cost
    [{| comm_identity := identity;
        comm_arity := left_arity;
        comm_trigger := trigger |}] =
  comm_trace_cost
    [{| comm_identity := identity;
        comm_arity := right_arity;
        comm_trigger := trigger |}].
Proof.
  reflexivity.
Qed.

Theorem comm_trace_cost_is_comm_count : forall trace,
  comm_trace_cost trace = length trace.
Proof.
  induction trace as [| observation rest IH]; simpl; lia.
Qed.

Theorem comm_trace_cost_permutation_invariant : forall left right,
  Permutation left right ->
  comm_trace_cost left = comm_trace_cost right.
Proof.
  intros left right permutation.
  repeat rewrite comm_trace_cost_is_comm_count.
  now apply Permutation_length.
Qed.

Theorem replaying_same_comm_trace_has_same_cost : forall play replay,
  play = replay ->
  comm_trace_cost play = comm_trace_cost replay.
Proof.
  intros play replay same_trace.
  now subst replay.
Qed.

Theorem rejected_comm_is_atomic : forall State
  (commit : State -> comm_observation -> State)
  budget consumed state observation,
  budget <= consumed ->
  reserve_then_commit commit budget consumed state observation =
    (state, consumed, false).
Proof.
  intros State commit budget consumed state observation exhausted.
  unfold reserve_then_commit.
  apply Nat.ltb_ge in exhausted.
  now rewrite exhausted.
Qed.

Theorem funded_comm_debits_before_commit : forall State
  (commit : State -> comm_observation -> State)
  budget consumed state observation,
  consumed < budget ->
  reserve_then_commit commit budget consumed state observation =
    (commit state observation, S consumed, true).
Proof.
  intros State commit budget consumed state observation funded.
  unfold reserve_then_commit.
  apply Nat.ltb_lt in funded.
  now rewrite funded.
Qed.

Theorem admitted_trace_fits_budget : forall trace budget,
  length trace <= budget ->
  comm_trace_cost trace <= budget.
Proof.
  intros trace budget admitted.
  rewrite comm_trace_cost_is_comm_count.
  exact admitted.
Qed.
