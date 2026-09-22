From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import PathMapReversePruneTeardown.

Import ListNotations.
Import PathMapReversePruneTeardown.PathMapReversePruneTeardown.
Set Implicit Arguments.

Module EpmOwnedRetirement.

Section Provider.
Context {Arena State Value : Type}.
Variable arena : Arena.
Variable valid : Arena -> Value -> Prop.
Variable invariant : State -> Prop.
Variable provide : State -> Value * State.
Hypothesis provide_valid : forall state,
  invariant state ->
  valid arena (fst (provide state)) /\ invariant (snd (provide state)).

Definition annotate (event : teardown_event) (state : State)
    : (teardown_event * option Value) * State :=
  match event with
  | InstallBarrier =>
      let '(value, next) := provide state in
      ((event, Some value), next)
  | _ => ((event, None), state)
  end.

Fixpoint elaborate (trace : list teardown_event) (state : State)
    : list (teardown_event * option Value) * State :=
  match trace with
  | [] => ([], state)
  | event :: rest =>
      let '(annotated, next) := annotate event state in
      let '(suffix, final) := elaborate rest next in
      (annotated :: suffix, final)
  end.

Definition erase (trace : list (teardown_event * option Value)) :=
  map (@fst teardown_event (option Value)) trace.

Definition annotation_valid (entry : teardown_event * option Value) : Prop :=
  match snd entry with
  | None => fst entry <> InstallBarrier
  | Some value => fst entry = InstallBarrier /\ valid arena value
  end.

Lemma annotate_erases : forall event state,
  fst (fst (annotate event state)) = event.
Proof.
  intros event state. destruct event; simpl; try reflexivity.
  destruct (provide state). reflexivity.
Qed.

Lemma annotate_valid : forall event state,
  invariant state ->
  annotation_valid (fst (annotate event state)) /\
  invariant (snd (annotate event state)).
Proof.
  intros event state good. destruct event; simpl;
    try (split; [discriminate|exact good]).
  pose proof (@provide_valid state good) as supplied.
  destruct (provide state) as [value next]. simpl in *.
  unfold annotation_valid. simpl. tauto.
Qed.

Theorem stateful_provider_preserves_every_event : forall trace state,
  erase (fst (elaborate trace state)) = trace.
Proof.
  induction trace as [|event rest IH]; intro state; simpl; [reflexivity|].
  pose proof (annotate_erases event state) as same.
  destruct (annotate event state) as [entry next] eqn:step.
  specialize (IH next).
  destruct (elaborate rest next) as [suffix final]. simpl in *.
  unfold erase in *. simpl. rewrite same, IH. reflexivity.
Qed.

Theorem stateful_provider_preserves_valid_same_arena_values : forall trace state,
  invariant state ->
  Forall annotation_valid (fst (elaborate trace state)) /\
  invariant (snd (elaborate trace state)).
Proof.
  induction trace as [|event rest IH]; intros state good; simpl.
  - split; [constructor|exact good].
  - pose proof (@annotate_valid event state good) as step_good.
    destruct (annotate event state) as [entry next]. simpl in step_good.
    destruct step_good as [entry_good next_good].
    specialize (IH next next_good).
    destruct (elaborate rest next) as [suffix final]. simpl in *.
    destruct IH as [suffix_good final_good]. split; [constructor|]; assumption.
Qed.

Theorem stateful_terminal_drain_preserves_existing_semantics : forall subject state,
  let trace := erase (fst (elaborate (terminal_two_phase_trace subject) state)) in
  removed_without_prune trace = preorder_payloads_tree subject /\
  disposed_values trace = preorder_payloads_tree subject /\
  queued_par_jobs trace = flat_map par_job (preorder_payloads_tree subject) /\
  prune_widths trace = repeat 1 (tree_edges subject).
Proof.
  intros subject state. simpl.
  rewrite stateful_provider_preserves_every_event.
  pose proof (terminal_two_phase_trace_is_exact_and_topology_complete subject).
  tauto.
Qed.

Theorem repeat_scaffold_needs_only_validity : forall value,
  valid arena value ->
  forall state : unit,
    valid arena (fst (value, state)) /\ True.
Proof. intros. simpl. tauto. Qed.

End Provider.

Inductive ownership_form := OwnedRoot | OwnedZipper.

Record owned : Type := Owned {
  owned_subject : trie;
  owned_form : ownership_form;
  owned_authority : terminal_root_authority
}.

Definition as_root (subject : owned) : owned :=
  Owned (owned_subject subject) OwnedRoot (owned_authority subject).

Inductive retirement_reason :=
| Refusal | ProjectionFailure | Replacement | FrameDisposal | ArenaDisposal.

Inductive ownership_event :=
| GuardedRetirement : retirement_reason -> owned -> list teardown_event -> ownership_event
| GuardedTransfer : owned -> owned -> ownership_event.

Definition retire (reason : retirement_reason) (slot : option owned)
    : option owned * list ownership_event :=
  match slot with
  | None => (None, [])
  | Some subject =>
      (None, [GuardedRetirement reason subject
        (terminal_two_phase_trace (owned_subject subject))])
  end.

Definition transfer (slot : option owned)
    : option owned * option owned * list ownership_event :=
  match slot with
  | None => (None, None, [])
  | Some subject =>
      (None, Some (as_root subject), [GuardedTransfer subject (as_root subject)])
  end.

Inductive disposition := RetireTo : retirement_reason -> disposition | TransferToGuard.

Definition settle (action : disposition) (slot : option owned)
    : option owned * option owned * list ownership_event :=
  match action with
  | RetireTo reason => let '(source, events) := retire reason slot in
      (source, None, events)
  | TransferToGuard => transfer slot
  end.

Definition source_slot (result : option owned * option owned * list ownership_event) :=
  fst (fst result).
Definition recipient_slot (result : option owned * option owned * list ownership_event) :=
  snd (fst result).
Definition ownership_events (result : option owned * option owned * list ownership_event) :=
  snd result.

Theorem every_settlement_consumes_source : forall action slot,
  source_slot (settle action slot) = None.
Proof. intros action slot. destruct action, slot; reflexivity. Qed.

Theorem live_source_has_exactly_one_terminal_event : forall action subject,
  length (ownership_events (settle action (Some subject))) = 1.
Proof. intros action subject. destruct action; reflexivity. Qed.

Theorem consumed_source_cannot_retire_or_transfer_again : forall first second slot,
  ownership_events (settle second (source_slot (settle first slot))) = [].
Proof.
  intros first second slot. rewrite every_settlement_consumes_source.
  destruct second; reflexivity.
Qed.

Theorem retirement_and_transfer_are_exclusive : forall action subject,
  (recipient_slot (settle action (Some subject)) = None /\
    exists reason, ownership_events (settle action (Some subject)) =
      [GuardedRetirement reason subject (terminal_two_phase_trace (owned_subject subject))]) \/
  (recipient_slot (settle action (Some subject)) = Some (as_root subject) /\
    ownership_events (settle action (Some subject)) =
      [GuardedTransfer subject (as_root subject)]).
Proof.
  intros action subject. destruct action as [reason|].
  - left. split; [reflexivity|exists reason; reflexivity].
  - right. split; reflexivity.
Qed.

Theorem zipper_adoption_preserves_subject_and_authority : forall subject authority,
  transfer (Some (Owned subject OwnedZipper authority)) =
    (None, Some (Owned subject OwnedRoot authority),
      [GuardedTransfer (Owned subject OwnedZipper authority)
        (Owned subject OwnedRoot authority)]).
Proof. reflexivity. Qed.

Definition replace_guarded (previous incoming : option owned)
    : option owned * option owned * list ownership_event :=
  let '(_, retired) := retire Replacement previous in
  let '(source, destination, moved) := transfer incoming in
  (source, destination, retired ++ moved).

Theorem replacement_retires_old_before_adopting_new : forall previous incoming,
  replace_guarded (Some previous) (Some incoming) =
    (None, Some (as_root incoming),
      [GuardedRetirement Replacement previous
        (terminal_two_phase_trace (owned_subject previous));
       GuardedTransfer incoming (as_root incoming)]).
Proof. reflexivity. Qed.

Theorem replacing_with_absence_retires_without_publication : forall previous,
  replace_guarded (Some previous) None =
    (None, None, [GuardedRetirement Replacement previous
      (terminal_two_phase_trace (owned_subject previous))]).
Proof. reflexivity. Qed.

Definition dispose_scope (reason : retirement_reason) (subjects : list owned)
    : list owned * list ownership_event :=
  ([], flat_map (fun subject => snd (retire reason (Some subject))) subjects).

Theorem scope_disposal_consumes_each_owned_slot_once : forall reason subjects,
  fst (dispose_scope reason subjects) = [] /\
  length (snd (dispose_scope reason subjects)) = length subjects.
Proof.
  intros reason subjects. split; [reflexivity|].
  unfold dispose_scope. simpl.
  induction subjects; simpl; congruence.
Qed.

Theorem scope_disposal_preserves_order_and_drain_traces : forall reason subjects,
  snd (dispose_scope reason subjects) =
    map (fun subject => GuardedRetirement reason subject
      (terminal_two_phase_trace (owned_subject subject))) subjects.
Proof.
  intros reason subjects. unfold dispose_scope. simpl.
  induction subjects; simpl; congruence.
Qed.

Section Failure.
Context {Error : Type}.

Inductive cleanup_result :=
| CleanupReturned : Error -> option owned -> list ownership_event -> cleanup_result
| CleanupFailStopped : cleanup_result.

Definition reject_owned (selected : Error) (reason : retirement_reason)
    (subject : owned) (allocation : teardown_allocation_outcome) (panic : bool)
    : cleanup_result :=
  match quarantine_process (run_quarantined_teardown
    (owned_authority subject) allocation panic (owned_subject subject)) with
  | LiveCompleted => CleanupReturned selected None
      (snd (retire reason (Some subject)))
  | ProcessFailStopped => CleanupFailStopped
  end.

Theorem returning_failure_preserves_selected_error : forall selected reason subject
    allocation panic returned remaining events,
  reject_owned selected reason subject allocation panic =
    CleanupReturned returned remaining events ->
  returned = selected /\ remaining = None /\
  events = [GuardedRetirement reason subject
    (terminal_two_phase_trace (owned_subject subject))].
Proof.
  intros selected reason subject allocation panic returned remaining events same.
  unfold reject_owned in same.
  destruct (quarantine_process (run_quarantined_teardown
    (owned_authority subject) allocation panic (owned_subject subject)));
    inversion same; subst; repeat split; reflexivity.
Qed.

Theorem refusal_and_projection_failure_do_not_publish : forall selected reason subject
    allocation panic returned remaining events,
  (reason = Refusal \/ reason = ProjectionFailure) ->
  reject_owned selected reason subject allocation panic =
    CleanupReturned returned remaining events -> remaining = None.
Proof.
  intros selected reason subject allocation panic returned remaining events why same.
  exact (proj1 (proj2 (@returning_failure_preserves_selected_error
    selected reason subject allocation panic returned remaining events same))).
Qed.

Theorem cleanup_panic_is_terminal : forall selected reason subject allocation,
  reject_owned selected reason subject allocation true = CleanupFailStopped.
Proof. intros. destruct allocation; reflexivity. Qed.

Theorem cleanup_allocation_failure_is_terminal : forall selected reason subject panic,
  reject_owned selected reason subject AllocationFailed panic = CleanupFailStopped.
Proof. intros. destruct panic; reflexivity. Qed.

Theorem retirement_reuses_existing_raw_drop_quarantine : forall subject allocation panic,
  explicit_raw_drop_is_empty (run_quarantined_teardown
    (owned_authority subject) allocation panic (owned_subject subject)) /\
  fail_stop_is_terminal (run_quarantined_teardown
    (owned_authority subject) allocation panic (owned_subject subject)) /\
  no_live_process_quarantine_leak (run_quarantined_teardown
    (owned_authority subject) allocation panic (owned_subject subject)) /\
  quarantine_recursive_raw_drops (run_quarantined_teardown
    (owned_authority subject) allocation panic (owned_subject subject)) = 0.
Proof. intros. apply every_quarantine_outcome_guards_raw_drop. Qed.

End Failure.

Print Assumptions stateful_provider_preserves_every_event.
Print Assumptions stateful_provider_preserves_valid_same_arena_values.
Print Assumptions stateful_terminal_drain_preserves_existing_semantics.
Print Assumptions repeat_scaffold_needs_only_validity.
Print Assumptions every_settlement_consumes_source.
Print Assumptions live_source_has_exactly_one_terminal_event.
Print Assumptions consumed_source_cannot_retire_or_transfer_again.
Print Assumptions retirement_and_transfer_are_exclusive.
Print Assumptions zipper_adoption_preserves_subject_and_authority.
Print Assumptions replacement_retires_old_before_adopting_new.
Print Assumptions replacing_with_absence_retires_without_publication.
Print Assumptions scope_disposal_consumes_each_owned_slot_once.
Print Assumptions scope_disposal_preserves_order_and_drain_traces.
Print Assumptions returning_failure_preserves_selected_error.
Print Assumptions refusal_and_projection_failure_do_not_publish.
Print Assumptions cleanup_panic_is_terminal.
Print Assumptions cleanup_allocation_failure_is_terminal.
Print Assumptions retirement_reuses_existing_raw_drop_quarantine.

End EpmOwnedRetirement.
