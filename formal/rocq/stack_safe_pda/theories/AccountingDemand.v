From Stdlib Require Import List Arith Bool Lia Wf_nat.

Import ListNotations.
Set Implicit Arguments.

(** Abstract events retain precisely the observables of the Rust analyzer:
    present-channel COMMs invoke the callback, missing-channel COMMs still
    count but do not invoke it, and Unknown is sticky. *)
Inductive demand_event : Type :=
| CommSome : nat -> demand_event
| CommMissing : demand_event
| Unknown : demand_event.

(** The process-position relation after protobuf recognition. A forest keeps
    sibling order explicit, matching each repeated-field cursor. *)
Inductive demand_tree : Type :=
| DemandNode : list demand_event -> demand_forest -> demand_tree
with demand_forest : Type :=
| DemandNil : demand_forest
| DemandCons : demand_tree -> demand_forest -> demand_forest.

Fixpoint recursive_trace (subject : demand_tree) : list demand_event :=
  match subject with
  | DemandNode local children => local ++ recursive_forest_trace children
  end
with recursive_forest_trace (subjects : demand_forest) : list demand_event :=
  match subjects with
  | DemandNil => []
  | DemandCons subject rest =>
      recursive_trace subject ++ recursive_forest_trace rest
  end.

Inductive demand_task : Type :=
| VisitTree : demand_tree -> demand_task
| VisitForest : demand_forest -> demand_task
| Emit : demand_event -> demand_task.

Definition expand_tree (subject : demand_tree) : list demand_task :=
  match subject with
  | DemandNode local children => map Emit local ++ [VisitForest children]
  end.

Definition expand_forest (subjects : demand_forest) : list demand_task :=
  match subjects with
  | DemandNil => []
  | DemandCons subject rest => [VisitTree subject; VisitForest rest]
  end.

Definition task_trace (task : demand_task) : list demand_event :=
  match task with
  | VisitTree subject => recursive_trace subject
  | VisitForest subjects => recursive_forest_trace subjects
  | Emit event => [event]
  end.

Fixpoint work_trace (work : list demand_task) : list demand_event :=
  match work with
  | [] => []
  | task :: rest => task_trace task ++ work_trace rest
  end.

Lemma work_trace_app :
  forall left right,
    work_trace (left ++ right) = work_trace left ++ work_trace right.
Proof.
  induction left; intros right; simpl; auto.
  now rewrite IHleft, app_assoc.
Qed.

Lemma work_trace_emit_map :
  forall events, work_trace (map Emit events) = events.
Proof.
  induction events; simpl; auto. now rewrite IHevents.
Qed.

Lemma expand_tree_preserves_trace :
  forall subject, work_trace (expand_tree subject) = recursive_trace subject.
Proof.
  intros [local children]. unfold expand_tree. simpl.
  rewrite work_trace_app, work_trace_emit_map. simpl.
  now rewrite app_nil_r.
Qed.

Lemma expand_forest_preserves_trace :
  forall subjects,
    work_trace (expand_forest subjects) = recursive_forest_trace subjects.
Proof.
  intros subjects. destruct subjects; simpl; auto.
  now rewrite app_nil_r.
Qed.

(** One transition of the explicit LIFO worklist, presented with its top at the
    list head. Output is accumulated in semantic order. *)
Inductive demand_step :
    list demand_task -> list demand_event ->
    list demand_task -> list demand_event -> Prop :=
| StepEmit : forall event rest output,
    demand_step (Emit event :: rest) output rest (output ++ [event])
| StepTree : forall subject rest output,
    demand_step (VisitTree subject :: rest) output
      (expand_tree subject ++ rest) output
| StepForest : forall subjects rest output,
    demand_step (VisitForest subjects :: rest) output
      (expand_forest subjects ++ rest) output.

Definition trace_invariant
    (expected : list demand_event)
    (work : list demand_task)
    (output : list demand_event) : Prop :=
  output ++ work_trace work = expected.

Theorem accounting_step_preserves_recursive_trace :
  forall expected work output work' output',
    trace_invariant expected work output ->
    demand_step work output work' output' ->
    trace_invariant expected work' output'.
Proof.
  intros expected work output work' output' invariant step.
  inversion step; subst; unfold trace_invariant in *; simpl in *.
  - now rewrite <- app_assoc.
  - rewrite work_trace_app, expand_tree_preserves_trace.
    exact invariant.
  - rewrite work_trace_app, expand_forest_preserves_trace.
    exact invariant.
Qed.

Inductive demand_steps :
    list demand_task -> list demand_event ->
    list demand_task -> list demand_event -> Prop :=
| StepsRefl : forall work output, demand_steps work output work output
| StepsNext : forall work output middle middle_output final final_output,
    demand_step work output middle middle_output ->
    demand_steps middle middle_output final final_output ->
    demand_steps work output final final_output.

Lemma accounting_steps_preserve_recursive_trace :
  forall expected work output work' output',
    trace_invariant expected work output ->
    demand_steps work output work' output' ->
    trace_invariant expected work' output'.
Proof.
  intros expected work output work' output' invariant steps.
  induction steps.
  - exact invariant.
  - apply IHsteps.
    eapply accounting_step_preserves_recursive_trace; eauto.
Qed.

(** Exact transition count of a finite recognized subject.  This is separate
    from the semantic trace length because empty forests still require one
    [VisitForest] transition. *)
Fixpoint tree_steps (subject : demand_tree) : nat :=
  match subject with
  | DemandNode local children => S (length local + forest_steps children)
  end
with forest_steps (subjects : demand_forest) : nat :=
  match subjects with
  | DemandNil => 1
  | DemandCons subject rest =>
      S (tree_steps subject + forest_steps rest)
  end.

Definition task_steps (task : demand_task) : nat :=
  match task with
  | VisitTree subject => tree_steps subject
  | VisitForest subjects => forest_steps subjects
  | Emit _ => 1
  end.

Fixpoint work_steps (work : list demand_task) : nat :=
  match work with
  | [] => 0
  | task :: rest => task_steps task + work_steps rest
  end.

Lemma work_steps_app :
  forall left right,
    work_steps (left ++ right) = work_steps left + work_steps right.
Proof.
  induction left; intros right; simpl; auto. rewrite IHleft. lia.
Qed.

Lemma work_steps_emit_map :
  forall events, work_steps (map Emit events) = length events.
Proof.
  induction events; simpl; auto.
Qed.

Lemma expand_tree_decreases_one :
  forall subject,
    tree_steps subject = S (work_steps (expand_tree subject)).
Proof.
  intros [local children]. unfold expand_tree. simpl.
  rewrite work_steps_app, work_steps_emit_map. simpl. lia.
Qed.

Lemma expand_forest_decreases_one :
  forall subjects,
    forest_steps subjects = S (work_steps (expand_forest subjects)).
Proof.
  intros subjects. destruct subjects; simpl; lia.
Qed.

Lemma accounting_step_decreases_work :
  forall work output work' output',
    demand_step work output work' output' ->
    work_steps work = S (work_steps work').
Proof.
  intros work output work' output' step. inversion step; subst; simpl.
  - reflexivity.
  - rewrite work_steps_app, expand_tree_decreases_one. lia.
  - rewrite work_steps_app, expand_forest_decreases_one. lia.
Qed.

Theorem accounting_pda_terminates :
  forall work output,
    exists final_output, demand_steps work output [] final_output.
Proof.
  intro work.
  induction work using
    (well_founded_induction_type (well_founded_ltof _ work_steps)).
  intros output. destruct work as [| task rest].
  - exists output. constructor.
  - destruct task as [subject | subjects | event].
    + assert (decrease :
        work_steps (expand_tree subject ++ rest) <
        work_steps (VisitTree subject :: rest)).
      { rewrite work_steps_app. cbn [work_steps task_steps].
        pose proof (expand_tree_decreases_one subject). lia. }
      destruct (H (expand_tree subject ++ rest) decrease output)
        as [final_output execution].
      exists final_output. econstructor; [constructor | exact execution].
    + assert (decrease :
        work_steps (expand_forest subjects ++ rest) <
        work_steps (VisitForest subjects :: rest)).
      { rewrite work_steps_app. cbn [work_steps task_steps].
        pose proof (expand_forest_decreases_one subjects). lia. }
      destruct (H (expand_forest subjects ++ rest) decrease output)
        as [final_output execution].
      exists final_output. econstructor; [constructor | exact execution].
    + assert (decrease :
        work_steps rest < work_steps (Emit event :: rest)) by (simpl; lia).
      destruct (H rest decrease (output ++ [event]))
        as [final_output execution].
      exists final_output. econstructor; [constructor | exact execution].
Qed.

Theorem accounting_pda_refines_recursive_trace :
  forall subject output,
    demand_steps [VisitTree subject] [] [] output ->
    output = recursive_trace subject.
Proof.
  intros subject output execution.
  pose proof (accounting_steps_preserve_recursive_trace
    (expected := recursive_trace subject)
    (work := [VisitTree subject]) (output := [])
    (work' := []) (output' := output)) as preservation.
  assert (initial : trace_invariant
    (recursive_trace subject) [VisitTree subject] []).
  { unfold trace_invariant. simpl. now rewrite app_nil_r. }
  specialize (preservation initial execution).
  unfold trace_invariant in preservation. simpl in preservation.
  now rewrite app_nil_r in preservation.
Qed.

Fixpoint repeat_trace (multiplicity : nat) (trace : list demand_event)
    : list demand_event :=
  match multiplicity with
  | 0 => []
  | S rest => trace ++ repeat_trace rest trace
  end.

Fixpoint comm_count (trace : list demand_event) : nat :=
  match trace with
  | [] => 0
  | CommSome _ :: rest | CommMissing :: rest => S (comm_count rest)
  | Unknown :: rest => comm_count rest
  end.

Lemma comm_count_app :
  forall left right,
    comm_count (left ++ right) = comm_count left + comm_count right.
Proof.
  induction left; intros right; simpl; auto.
  destruct a; simpl; rewrite IHleft; lia.
Qed.

Theorem compressed_multiplicity_replays_the_whole_trace :
  forall multiplicity trace,
    comm_count (repeat_trace multiplicity trace) =
      multiplicity * comm_count trace.
Proof.
  induction multiplicity; intros trace; simpl; auto.
  rewrite comm_count_app, IHmultiplicity. lia.
Qed.

Definition saturated_count (cap : nat) (trace : list demand_event) : nat :=
  Nat.min cap (comm_count trace).

Corollary compressed_multiplicity_saturates_after_scaling :
  forall cap multiplicity trace,
    saturated_count cap (repeat_trace multiplicity trace) =
      Nat.min cap (multiplicity * comm_count trace).
Proof.
  intros. unfold saturated_count.
  now rewrite compressed_multiplicity_replays_the_whole_trace.
Qed.

Fixpoint has_unknown (trace : list demand_event) : bool :=
  match trace with
  | [] => false
  | Unknown :: _ => true
  | _ :: rest => has_unknown rest
  end.

Lemma has_unknown_app :
  forall left right,
    has_unknown (left ++ right) = has_unknown left || has_unknown right.
Proof.
  induction left as [| event left IH]; intros right; simpl.
  - reflexivity.
  - destruct event; simpl; try rewrite IH; reflexivity.
Qed.

Theorem positive_multiplicity_preserves_unknown :
  forall multiplicity trace,
    has_unknown (repeat_trace (S multiplicity) trace) = has_unknown trace.
Proof.
  induction multiplicity as [| multiplicity IH]; intros trace.
  - cbn [repeat_trace]. rewrite has_unknown_app. simpl.
    now rewrite Bool.orb_false_r.
  - change
      (has_unknown (trace ++ repeat_trace (S multiplicity) trace) =
        has_unknown trace).
    rewrite has_unknown_app, IH.
    destruct (has_unknown trace); reflexivity.
Qed.

Section StatefulCallback.

Context {State : Type}.
Variable callback : State -> nat -> State.

Fixpoint run_callbacks (trace : list demand_event) (state : State) : State :=
  match trace with
  | [] => state
  | CommSome channel :: rest => run_callbacks rest (callback state channel)
  | _ :: rest => run_callbacks rest state
  end.

Theorem equal_trace_preserves_every_stateful_callback :
  forall left right state,
    left = right -> run_callbacks left state = run_callbacks right state.
Proof. intros. now subst. Qed.

Theorem accounting_pda_preserves_stateful_callback :
  forall subject output state,
    demand_steps [VisitTree subject] [] [] output ->
    run_callbacks output state = run_callbacks (recursive_trace subject) state.
Proof.
  intros subject output state execution.
  apply equal_trace_preserves_every_stateful_callback.
  now apply accounting_pda_refines_recursive_trace.
Qed.

End StatefulCallback.

Print Assumptions accounting_pda_refines_recursive_trace.
Print Assumptions accounting_pda_terminates.
Print Assumptions compressed_multiplicity_saturates_after_scaling.
Print Assumptions accounting_pda_preserves_stateful_callback.
