From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Lia.
From CostAccountedRho Require Import DeterministicParallelReduction.
Import ListNotations.

Section CountedDetachedBisimulation.

Variable task : Type.
Variable offspring : task -> list task.
Variable task_charge : task -> nat.
Variable task_failure : task -> option nat.

Record observation := Observation {
  completed : list task;
  charged : nat;
  failures : list nat;
  canceled : bool;
  closed : bool
}.

Definition run_observation (t : task) (o : observation) : observation :=
  Observation
    (completed o ++ [t])
    (charged o + task_charge t)
    (match task_failure t with
     | Some failure => failures o ++ [failure]
     | None => failures o
     end)
    (canceled o)
    (closed o).

Definition cancel_observation (o : observation) : observation :=
  Observation (completed o) (charged o) (failures o) true (closed o).

Definition close_observation (o : observation) : observation :=
  Observation (completed o) (charged o) (failures o) (canceled o) true.

Record joined_state := Joined {
  joined_pending : list task;
  joined_observation : observation;
  joined_permit : bool
}.

Record detached_state := Detached {
  detached_pending : list task;
  detached_live : nat;
  detached_observation : observation;
  detached_permit : bool
}.

Inductive driver_event :=
| RunTask (t : task)
| CancelRoot
| AbortTask (t : task)
| CloseRoot.

Inductive joined_step : driver_event -> joined_state -> joined_state -> Prop :=
| JoinedRun : forall before after t o,
    canceled o = false -> closed o = false ->
    joined_step (RunTask t)
      (Joined (before ++ t :: after) o true)
      (Joined (before ++ after ++ offspring t) (run_observation t o) true)
| JoinedCancel : forall pending o,
    canceled o = false -> closed o = false ->
    joined_step CancelRoot
      (Joined pending o true)
      (Joined pending (cancel_observation o) true)
| JoinedAbort : forall before after t o,
    canceled o = true -> closed o = false ->
    joined_step (AbortTask t)
      (Joined (before ++ t :: after) o true)
      (Joined (before ++ after) o true)
| JoinedClose : forall o,
    closed o = false ->
    joined_step CloseRoot
      (Joined [] o true)
      (Joined [] (close_observation o) false).

Inductive detached_step : driver_event -> detached_state -> detached_state -> Prop :=
| DetachedRun : forall before after t o live,
    canceled o = false -> closed o = false ->
    detached_step (RunTask t)
      (Detached (before ++ t :: after) (S live) o true)
      (Detached (before ++ after ++ offspring t)
        (live + length (offspring t)) (run_observation t o) true)
| DetachedCancel : forall pending o live,
    canceled o = false -> closed o = false ->
    detached_step CancelRoot
      (Detached pending live o true)
      (Detached pending live (cancel_observation o) true)
| DetachedAbort : forall before after t o live,
    canceled o = true -> closed o = false ->
    detached_step (AbortTask t)
      (Detached (before ++ t :: after) (S live) o true)
      (Detached (before ++ after) live o true)
| DetachedClose : forall pending o,
    closed o = false ->
    detached_step CloseRoot
      (Detached pending 0 o true)
      (Detached pending 0 (close_observation o) false).

Inductive related : joined_state -> detached_state -> Prop :=
| Related : forall pending o permit,
    related (Joined pending o permit)
      (Detached pending (length pending) o permit).

Definition initial_observation : observation :=
  Observation [] 0 [] false false.

Definition initial_joined (root : task) : joined_state :=
  Joined [root] initial_observation true.

Definition initial_detached (root : task) : detached_state :=
  Detached [root] 1 initial_observation true.

Theorem initial_states_related : forall root,
  related (initial_joined root) (initial_detached root).
Proof.
  intro root. constructor.
Qed.

Lemma length_with_task : forall (before after : list task) (t : task),
  length (before ++ t :: after) = S (length (before ++ after)).
Proof.
  intros. repeat rewrite length_app. simpl. lia.
Qed.

Theorem joined_step_has_detached_match : forall event joined joined' detached,
  related joined detached ->
  joined_step event joined joined' ->
  exists detached',
    detached_step event detached detached' /\ related joined' detached'.
Proof.
  intros event joined joined' detached Hrelated Hstep.
  destruct Hrelated as [pending o permit].
  inversion Hstep; subst; clear Hstep.
  - rewrite length_with_task.
    exists (Detached (before ++ after ++ offspring t)
      (length (before ++ after) + length (offspring t))
      (run_observation t o) true).
    split.
    + apply DetachedRun; assumption.
    + replace (length (before ++ after) + length (offspring t))
        with (length (before ++ after ++ offspring t))
        by (repeat rewrite length_app; lia).
      constructor.
  - exists (Detached pending (length pending) (cancel_observation o) true).
    split.
    + apply DetachedCancel; assumption.
    + constructor.
  - rewrite length_with_task.
    exists (Detached (before ++ after) (length (before ++ after)) o true).
    split.
    + apply DetachedAbort; assumption.
    + constructor.
  - exists (Detached [] 0 (close_observation o) false).
    split.
    + apply DetachedClose. assumption.
    + constructor.
Qed.

Theorem detached_step_has_joined_match : forall event joined detached detached',
  related joined detached ->
  detached_step event detached detached' ->
  exists joined',
    joined_step event joined joined' /\ related joined' detached'.
Proof.
  intros event joined detached detached' Hrelated Hstep.
  destruct Hrelated as [pending o permit].
  inversion Hstep; subst; clear Hstep.
  - rewrite length_with_task in *.
    exists (Joined (before ++ after ++ offspring t) (run_observation t o) true).
    split.
    + apply JoinedRun; assumption.
    + assert (Hlive : live = length (before ++ after))
        by (repeat rewrite length_app in *; simpl in *; lia).
      rewrite Hlive.
      replace (length (before ++ after) + length (offspring t))
        with (length (before ++ after ++ offspring t))
        by (repeat rewrite length_app; lia).
      constructor.
  - exists (Joined pending (cancel_observation o) true).
    split.
    + apply JoinedCancel; assumption.
    + constructor.
  - rewrite length_with_task in *.
    exists (Joined (before ++ after) o true).
    split.
    + apply JoinedAbort; assumption.
    + assert (Hlive : live = length (before ++ after)) by lia.
      rewrite Hlive. constructor.
  - destruct pending as [| first rest].
    + exists (Joined [] (close_observation o) false).
      split.
      * apply JoinedClose. assumption.
      * constructor.
    + discriminate.
Qed.

Theorem related_observations_equal : forall joined detached,
  related joined detached ->
  joined_observation joined = detached_observation detached /\
  joined_permit joined = detached_permit detached /\
  detached_live detached = length (joined_pending joined).
Proof.
  intros joined detached Hrelated.
  destruct Hrelated. repeat split; reflexivity.
Qed.

End CountedDetachedBisimulation.

Arguments completed {task} _.
Arguments charged {task} _.
Arguments failures {task} _.
Arguments canceled {task} _.
Arguments closed {task} _.
Arguments joined_observation {task} _.
Arguments detached_observation {task} _.

Definition joined_selected_replay
  (state : joined_state reduction_intent) : reduction_state :=
  commit_frontier
    (completed (joined_observation state)) empty_reduction_state.

Definition detached_selected_replay
  (state : detached_state reduction_intent) : reduction_state :=
  commit_frontier
    (completed (detached_observation state)) empty_reduction_state.

Theorem related_cost_accounted_replay :
  forall (joined : joined_state reduction_intent)
         (detached : detached_state reduction_intent),
    related reduction_intent joined detached ->
    joined_selected_replay joined = detached_selected_replay detached /\
    charged (joined_observation joined) =
      charged (detached_observation detached) /\
    failures (joined_observation joined) =
      failures (detached_observation detached) /\
    canceled (joined_observation joined) =
      canceled (detached_observation detached) /\
    closed (joined_observation joined) =
      closed (detached_observation detached).
Proof.
  intros joined detached Hrelated.
  destruct Hrelated.
  repeat split; reflexivity.
Qed.
