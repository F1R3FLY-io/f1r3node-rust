From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import PathMapPhysicalOwnership.

Import ListNotations.
Set Implicit Arguments.

(**
  Formal-first contract for campaign-owned PathMap destruction.

  The model is deliberately stated over logical byte-trie edges, not over the
  private node representation of the PathMap crate.  A production refinement
  refines to two explicit passes over one whole private root.  Phase A uses one
  public owned write zipper in logical-byte preorder, removes every real value
  with [remove_val(false)], and immediately transfers it to its named disposer.
  Only after closing the cursor and validating a zero-value root may Phase B
  install synthetic barriers and prune topology in reverse postorder, one
  logical edge at a time.  A barrier is removed before the next transition.

  This separates the guarantee that campaign-owned code can establish from an
  assertion about private upstream allocation internals: no nonempty private
  trie is handed to ordinary recursive Drop, while every topological
  detachment made by the integration is bounded to one logical edge.  Final
  wrapper election alone never establishes internal-node uniqueness; terminal
  teardown additionally requires an exclusive physical footprint or a
  verified public-drain refinement.
*)

Module PathMapReversePruneTeardown.

Inductive payload : Type :=
| SetMember : nat -> payload
| MapPar : nat -> list nat -> payload
| BagMultiplicity : nat -> nat -> payload
| CandidatePosition : nat -> payload.

Inductive trie : Type :=
| Node : option payload -> forest -> trie
with forest : Type :=
| FNil : forest
| FCons : trie -> forest -> forest.

Scheme trie_induction := Induction for trie Sort Prop
with forest_induction := Induction for forest Sort Prop.
Combined Scheme trie_forest_induction from trie_induction, forest_induction.

Definition option_list {A : Type} (cell : option A) : list A :=
  match cell with
  | Some value => [value]
  | None => []
  end.

Fixpoint reverse_payloads_tree (subject : trie) : list payload :=
  match subject with
  | Node cell children =>
      reverse_payloads_forest children ++ option_list cell
  end
with reverse_payloads_forest (subjects : forest) : list payload :=
  match subjects with
  | FNil => []
  | FCons subject rest =>
      reverse_payloads_forest rest ++ reverse_payloads_tree subject
  end.

(** Phase A uses logical-byte preorder so every value, including a value on a
    compressed internal node or the empty/root path, is extracted before any
    topology is changed.  Forest order is the abstract byte order. *)
Fixpoint preorder_payloads_tree (subject : trie) : list payload :=
  match subject with
  | Node cell children => option_list cell ++ preorder_payloads_forest children
  end
with preorder_payloads_forest (subjects : forest) : list payload :=
  match subjects with
  | FNil => []
  | FCons subject rest =>
      preorder_payloads_tree subject ++ preorder_payloads_forest rest
  end.

Fixpoint erase_values_tree (subject : trie) : trie :=
  match subject with
  | Node _ children => Node None (erase_values_forest children)
  end
with erase_values_forest (subjects : forest) : forest :=
  match subjects with
  | FNil => FNil
  | FCons subject rest =>
      FCons (erase_values_tree subject) (erase_values_forest rest)
  end.

Definition value_free (subject : trie) : Prop :=
  preorder_payloads_tree subject = [].

Theorem erase_values_produces_a_value_free_root :
  (forall subject, value_free (erase_values_tree subject)) /\
  (forall subjects,
    preorder_payloads_forest (erase_values_forest subjects) = []).
Proof.
  apply trie_forest_induction.
  - intros cell children children_value_free.
    unfold value_free. simpl. exact children_value_free.
  - reflexivity.
  - intros subject subject_value_free rest rest_value_free.
    simpl. rewrite subject_value_free, rest_value_free. reflexivity.
Qed.

Fixpoint tree_edges (subject : trie) : nat :=
  match subject with
  | Node _ children => forest_edges children
  end
with forest_edges (subjects : forest) : nat :=
  match subjects with
  | FNil => 0
  | FCons subject rest =>
      forest_edges rest + tree_edges subject + 1
  end.

Definition root_payload (subject : trie) : option payload :=
  match subject with Node cell _ => cell end.

Inductive teardown_event : Type :=
| RemoveValueNoPrune : payload -> teardown_event
| DisposeValue : payload -> teardown_event
| CloseValueCursor : teardown_event
| ValidateValueFreeRoot : teardown_event
| BeginTopologyPhase : teardown_event
| DetachOriginal : payload -> teardown_event
| QueueParPda : nat -> list nat -> teardown_event
| DestroyParChild : nat -> nat -> teardown_event
| InstallBarrier : teardown_event
| PruneSpan : nat -> teardown_event
| DetachBarrier : teardown_event
| ReleaseSharedRoot : teardown_event
| PublishComplete : teardown_event.

Definition phase_a_dispose_value (value : payload) : list teardown_event :=
  match value with
  | MapPar par_id children =>
      [RemoveValueNoPrune value; DisposeValue value;
       QueueParPda par_id children] ++ map (DestroyParChild par_id) children
  | _ => [RemoveValueNoPrune value; DisposeValue value]
  end.

Definition phase_a_value_trace (subject : trie) : list teardown_event :=
  flat_map phase_a_dispose_value (preorder_payloads_tree subject) ++
  [CloseValueCursor; ValidateValueFreeRoot].

Fixpoint removed_without_prune (trace : list teardown_event) : list payload :=
  match trace with
  | [] => []
  | RemoveValueNoPrune value :: rest =>
      value :: removed_without_prune rest
  | _ :: rest => removed_without_prune rest
  end.

Fixpoint disposed_values (trace : list teardown_event) : list payload :=
  match trace with
  | [] => []
  | DisposeValue value :: rest => value :: disposed_values rest
  | _ :: rest => disposed_values rest
  end.

Record phase_a_result : Type := PhaseAResult {
  phase_a_source_root : trie;
  phase_a_value_free_root : trie;
  phase_a_events : list teardown_event
}.

Definition run_phase_a (subject : trie) : phase_a_result :=
  PhaseAResult subject (erase_values_tree subject) (phase_a_value_trace subject).

Definition dispose_payload (value : payload) : list teardown_event :=
  match value with
  | MapPar par_id children =>
      DetachOriginal value :: QueueParPda par_id children ::
        map (DestroyParChild par_id) children
  | _ => [DetachOriginal value]
  end.

Definition dispose_cell (cell : option payload) : list teardown_event :=
  match cell with
  | Some value => dispose_payload value
  | None => []
  end.

(** A real parent value is already a one-edge pruning barrier.  An absent
    parent receives a distinct temporary barrier immediately before pruning;
    the original value is never overwritten. *)
Definition prune_segment (parent_has_value : bool) : list teardown_event :=
  if parent_has_value
  then [PruneSpan 1]
  else [InstallBarrier; PruneSpan 1; DetachBarrier].

Fixpoint drain_tree (subject : trie) : list teardown_event :=
  match subject with
  | Node cell children =>
      drain_forest children
        (match cell with Some _ => true | None => false end) ++
      dispose_cell cell
  end
with drain_forest
    (subjects : forest) (parent_has_value : bool) : list teardown_event :=
  match subjects with
  | FNil => []
  | FCons subject rest =>
      drain_forest rest parent_has_value ++
      drain_tree subject ++
      prune_segment parent_has_value
  end.

(** Phase B is uncallable without an explicit proof that Phase A left a
    value-free root.  It therefore cannot implicitly prune or drop a real
    value under any compressed physical segmentation. *)
Definition run_phase_b
    (phase_a : phase_a_result)
    (_ : value_free (phase_a_value_free_root phase_a)) : list teardown_event :=
  BeginTopologyPhase :: drain_tree (phase_a_value_free_root phase_a).

Definition terminal_two_phase_trace (subject : trie) : list teardown_event :=
  let phase_a := run_phase_a subject in
  phase_a_events phase_a ++
  run_phase_b phase_a (proj1 erase_values_produces_a_value_free_root subject).

Fixpoint detached_payloads (trace : list teardown_event) : list payload :=
  match trace with
  | [] => []
  | DetachOriginal value :: rest => value :: detached_payloads rest
  | _ :: rest => detached_payloads rest
  end.

Fixpoint prune_widths (trace : list teardown_event) : list nat :=
  match trace with
  | [] => []
  | PruneSpan width :: rest => width :: prune_widths rest
  | _ :: rest => prune_widths rest
  end.

Fixpoint queued_par_jobs
    (trace : list teardown_event) : list (nat * list nat) :=
  match trace with
  | [] => []
  | QueueParPda par_id children :: rest =>
      (par_id, children) :: queued_par_jobs rest
  | _ :: rest => queued_par_jobs rest
  end.

Definition par_job (value : payload) : list (nat * list nat) :=
  match value with
  | MapPar par_id children => [(par_id, children)]
  | _ => []
  end.

Lemma removed_without_prune_app : forall left right,
  removed_without_prune (left ++ right) =
  removed_without_prune left ++ removed_without_prune right.
Proof.
  induction left as [|event rest IH]; intros right; simpl; auto.
  destruct event; simpl; now rewrite IH.
Qed.

Lemma disposed_values_app : forall left right,
  disposed_values (left ++ right) =
  disposed_values left ++ disposed_values right.
Proof.
  induction left as [|event rest IH]; intros right; simpl; auto.
  destruct event; simpl; now rewrite IH.
Qed.

Lemma removed_destroy_children_are_empty : forall par_id children,
  removed_without_prune (map (DestroyParChild par_id) children) = [].
Proof. intros par_id children. induction children; simpl; auto. Qed.

Lemma disposed_destroy_children_are_empty : forall par_id children,
  disposed_values (map (DestroyParChild par_id) children) = [].
Proof. intros par_id children. induction children; simpl; auto. Qed.

Lemma phase_a_dispose_value_is_immediate : forall value,
  firstn 2 (phase_a_dispose_value value) =
    [RemoveValueNoPrune value; DisposeValue value].
Proof. destruct value; reflexivity. Qed.

Lemma phase_a_dispose_value_removes_and_disposes_once : forall value,
  removed_without_prune (phase_a_dispose_value value) = [value] /\
  disposed_values (phase_a_dispose_value value) = [value].
Proof.
  destruct value; simpl; try tauto.
  rewrite removed_destroy_children_are_empty,
    disposed_destroy_children_are_empty. tauto.
Qed.

Lemma detached_payloads_app : forall left right,
  detached_payloads (left ++ right) =
  detached_payloads left ++ detached_payloads right.
Proof.
  induction left as [|event rest IH]; intros right; simpl; auto.
  destruct event; simpl; rewrite IH; reflexivity.
Qed.

Lemma prune_widths_app : forall left right,
  prune_widths (left ++ right) = prune_widths left ++ prune_widths right.
Proof.
  induction left as [|event rest IH]; intros right; simpl; auto.
  destruct event; simpl; rewrite IH; reflexivity.
Qed.

Lemma queued_par_jobs_app : forall left right,
  queued_par_jobs (left ++ right) =
  queued_par_jobs left ++ queued_par_jobs right.
Proof.
  induction left as [|event rest IH]; intros right; simpl; auto.
  destruct event; simpl; rewrite IH; reflexivity.
Qed.

Lemma phase_a_dispose_value_queues_exact_par_job : forall value,
  queued_par_jobs (phase_a_dispose_value value) = par_job value.
Proof.
  destruct value as [member|par_id children|key multiplicity|position];
    simpl; try reflexivity.
  assert (no_nested :
    queued_par_jobs (map (DestroyParChild par_id) children) = []).
  { induction children; simpl; auto. }
  now rewrite no_nested.
Qed.

Lemma phase_a_removed_flat_map : forall values,
  removed_without_prune (flat_map phase_a_dispose_value values) = values.
Proof.
  induction values as [|value rest IH]; [reflexivity|].
  change (removed_without_prune
    (phase_a_dispose_value value ++ flat_map phase_a_dispose_value rest) =
    value :: rest).
  rewrite removed_without_prune_app.
  rewrite (proj1 (phase_a_dispose_value_removes_and_disposes_once value)), IH.
  reflexivity.
Qed.

Lemma phase_a_disposed_flat_map : forall values,
  disposed_values (flat_map phase_a_dispose_value values) = values.
Proof.
  induction values as [|value rest IH]; [reflexivity|].
  change (disposed_values
    (phase_a_dispose_value value ++ flat_map phase_a_dispose_value rest) =
    value :: rest).
  rewrite disposed_values_app.
  rewrite (proj2 (phase_a_dispose_value_removes_and_disposes_once value)), IH.
  reflexivity.
Qed.

Lemma phase_a_par_jobs_flat_map : forall values,
  queued_par_jobs (flat_map phase_a_dispose_value values) =
  flat_map par_job values.
Proof.
  induction values as [|value rest IH]; [reflexivity|].
  change (queued_par_jobs
    (phase_a_dispose_value value ++ flat_map phase_a_dispose_value rest) =
    par_job value ++ flat_map par_job rest).
  rewrite queued_par_jobs_app.
  rewrite phase_a_dispose_value_queues_exact_par_job, IH. reflexivity.
Qed.

Theorem phase_a_extracts_and_immediately_disposes_every_real_value_once :
  forall subject,
    removed_without_prune (phase_a_value_trace subject) =
      preorder_payloads_tree subject /\
    disposed_values (phase_a_value_trace subject) =
      preorder_payloads_tree subject /\
    queued_par_jobs (phase_a_value_trace subject) =
      flat_map par_job (preorder_payloads_tree subject) /\
    exists extracted,
      phase_a_value_trace subject =
        extracted ++ [CloseValueCursor; ValidateValueFreeRoot] /\
      Forall
        (fun value => firstn 2 (phase_a_dispose_value value) =
          [RemoveValueNoPrune value; DisposeValue value])
        (preorder_payloads_tree subject).
Proof.
  intro subject. unfold phase_a_value_trace.
  remember (preorder_payloads_tree subject) as values.
  repeat split.
  - rewrite removed_without_prune_app, phase_a_removed_flat_map.
    simpl. apply app_nil_r.
  - rewrite disposed_values_app, phase_a_disposed_flat_map.
    simpl. apply app_nil_r.
  - rewrite queued_par_jobs_app, phase_a_par_jobs_flat_map.
    simpl. apply app_nil_r.
  - exists (flat_map phase_a_dispose_value values). split; [reflexivity|].
    subst values. apply Forall_forall. intros value _.
    apply phase_a_dispose_value_is_immediate.
Qed.

Lemma phase_a_dispose_value_has_no_prune : forall value,
  prune_widths (phase_a_dispose_value value) = [].
Proof.
  destruct value as [member|par_id children|key multiplicity|position];
    simpl; try reflexivity.
  induction children; simpl; auto.
Qed.

Lemma phase_a_prunes_flat_map_are_empty : forall values,
  prune_widths (flat_map phase_a_dispose_value values) = [].
Proof.
  induction values as [|value rest IH]; [reflexivity|].
  change (prune_widths
    (phase_a_dispose_value value ++ flat_map phase_a_dispose_value rest) = []).
  rewrite prune_widths_app, phase_a_dispose_value_has_no_prune, IH.
  reflexivity.
Qed.

Theorem phase_a_changes_no_topology_and_closes_before_validation :
  forall subject,
    prune_widths (phase_a_value_trace subject) = [] /\
    exists prefix,
      phase_a_value_trace subject =
        prefix ++ [CloseValueCursor; ValidateValueFreeRoot].
Proof.
  intro subject. unfold phase_a_value_trace.
  remember (preorder_payloads_tree subject) as values.
  split.
  - rewrite prune_widths_app, phase_a_prunes_flat_map_are_empty.
    reflexivity.
  - exists (flat_map phase_a_dispose_value values). reflexivity.
Qed.

Theorem phase_a_closes_with_a_value_free_root : forall subject,
  phase_a_source_root (run_phase_a subject) = subject /\
  phase_a_value_free_root (run_phase_a subject) = erase_values_tree subject /\
  value_free (phase_a_value_free_root (run_phase_a subject)).
Proof.
  intro subject. repeat split; try reflexivity.
  exact (proj1 erase_values_produces_a_value_free_root subject).
Qed.

Definition logical_value_count (subject : trie) : nat :=
  length (preorder_payloads_tree subject).

Definition logical_iteration_count := logical_value_count.

Theorem phase_a_zero_validation_covers_metadata_and_iteration :
  forall subject,
  logical_value_count (phase_a_value_free_root (run_phase_a subject)) = 0 /\
  logical_iteration_count
    (phase_a_value_free_root (run_phase_a subject)) = 0.
Proof.
  intro subject.
  change
    (length (preorder_payloads_tree (erase_values_tree subject)) = 0 /\
     length (preorder_payloads_tree (erase_values_tree subject)) = 0).
  rewrite (proj1 erase_values_produces_a_value_free_root subject).
  split; reflexivity.
Qed.

(** The Rust refinement carries one current value directly into its disposer;
    it does not retain an N-element pending-disposal vector.  Its cursor/PDA
    control state is constant-size and has no configured traversal-depth cap. *)
Record phase_a_resource_contract : Type := PhaseAResourceContract {
  phase_a_write_cursors : nat;
  phase_a_retained_values : nat;
  phase_a_native_stack_growth : nat;
  phase_a_depth_cap : option nat
}.

Definition iterative_phase_a_resources : phase_a_resource_contract :=
  PhaseAResourceContract 1 1 0 None.

Theorem phase_a_has_constant_control_and_no_artificial_depth_limit :
  phase_a_write_cursors iterative_phase_a_resources = 1 /\
  phase_a_retained_values iterative_phase_a_resources = 1 /\
  phase_a_native_stack_growth iterative_phase_a_resources = 0 /\
  phase_a_depth_cap iterative_phase_a_resources = None.
Proof. repeat split; reflexivity. Qed.

Lemma repeat_add : forall (A : Type) (value : A) left right,
  repeat value (left + right) = repeat value left ++ repeat value right.
Proof.
  intros A value left; induction left as [|left IH]; intro right; simpl; auto.
  now rewrite IH.
Qed.

Lemma detached_par_child_events_are_empty : forall par_id children,
  detached_payloads (map (DestroyParChild par_id) children) = [].
Proof.
  intros par_id children. induction children as [|child rest IH]; simpl; auto.
Qed.

Lemma par_child_events_have_no_prunes : forall par_id children,
  prune_widths (map (DestroyParChild par_id) children) = [].
Proof.
  intros par_id children. induction children as [|child rest IH]; simpl; auto.
Qed.

Lemma par_child_events_have_no_nested_queue : forall par_id children,
  queued_par_jobs (map (DestroyParChild par_id) children) = [].
Proof.
  intros par_id children. induction children as [|child rest IH]; simpl; auto.
Qed.

Lemma dispose_cell_detaches_exactly : forall cell,
  detached_payloads (dispose_cell cell) = option_list cell.
Proof.
  intros [value|]; simpl; auto. destruct value; simpl; try reflexivity.
  now rewrite detached_par_child_events_are_empty.
Qed.

Lemma dispose_cell_has_no_prune : forall cell,
  prune_widths (dispose_cell cell) = [].
Proof.
  intros [value|]; simpl; auto. destruct value; simpl; try reflexivity.
  now rewrite par_child_events_have_no_prunes.
Qed.

Lemma dispose_cell_queues_exact_par_job : forall cell,
  queued_par_jobs (dispose_cell cell) =
  flat_map par_job (option_list cell).
Proof.
  intros [value|]; simpl; auto. destruct value; simpl; try reflexivity.
  now rewrite par_child_events_have_no_nested_queue.
Qed.

Lemma prune_segment_is_one_logical_edge : forall parent_has_value,
  prune_widths (prune_segment parent_has_value) = [1].
Proof. destruct parent_has_value; reflexivity. Qed.

Lemma prune_segment_detaches_no_original : forall parent_has_value,
  detached_payloads (prune_segment parent_has_value) = [].
Proof. destruct parent_has_value; reflexivity. Qed.

Lemma prune_segment_queues_no_par_job : forall parent_has_value,
  queued_par_jobs (prune_segment parent_has_value) = [].
Proof. destruct parent_has_value; reflexivity. Qed.

Theorem drain_detaches_originals_exactly_once_in_reverse_postorder :
  (forall subject,
      detached_payloads (drain_tree subject) =
      reverse_payloads_tree subject) /\
  (forall subjects parent_has_value,
      detached_payloads (drain_forest subjects parent_has_value) =
      reverse_payloads_forest subjects).
Proof.
  apply trie_forest_induction.
  - intros cell children children_equivalence. simpl.
    rewrite detached_payloads_app, children_equivalence.
    rewrite dispose_cell_detaches_exactly. reflexivity.
  - intros parent_has_value. reflexivity.
  - intros subject subject_equivalence rest rest_equivalence parent_has_value.
    simpl. repeat rewrite detached_payloads_app.
    rewrite rest_equivalence, subject_equivalence.
    rewrite prune_segment_detaches_no_original, app_nil_r. reflexivity.
Qed.

Theorem drain_prunes_every_logical_edge_once :
  (forall subject,
      prune_widths (drain_tree subject) = repeat 1 (tree_edges subject)) /\
  (forall subjects parent_has_value,
      prune_widths (drain_forest subjects parent_has_value) =
      repeat 1 (forest_edges subjects)).
Proof.
  apply trie_forest_induction.
  - intros cell children children_equivalence. simpl.
    rewrite prune_widths_app, children_equivalence.
    rewrite dispose_cell_has_no_prune, app_nil_r. reflexivity.
  - intros parent_has_value. reflexivity.
  - intros subject subject_equivalence rest rest_equivalence parent_has_value.
    simpl. repeat rewrite prune_widths_app.
    rewrite rest_equivalence, subject_equivalence,
      prune_segment_is_one_logical_edge.
    repeat rewrite repeat_add. now rewrite app_assoc.
Qed.

Theorem drain_transfers_every_par_to_one_explicit_pda_job :
  (forall subject,
      queued_par_jobs (drain_tree subject) =
      flat_map par_job (reverse_payloads_tree subject)) /\
  (forall subjects parent_has_value,
      queued_par_jobs (drain_forest subjects parent_has_value) =
      flat_map par_job (reverse_payloads_forest subjects)).
Proof.
  apply trie_forest_induction.
  - intros cell children children_equivalence. simpl.
    rewrite queued_par_jobs_app, children_equivalence.
    rewrite dispose_cell_queues_exact_par_job, flat_map_app. reflexivity.
  - intros parent_has_value. reflexivity.
  - intros subject subject_equivalence rest rest_equivalence parent_has_value.
    simpl. repeat rewrite queued_par_jobs_app.
    rewrite rest_equivalence, subject_equivalence.
    rewrite prune_segment_queues_no_par_job, app_nil_r.
    rewrite flat_map_app. reflexivity.
Qed.

Theorem erase_values_preserves_topology :
  (forall subject,
    tree_edges (erase_values_tree subject) = tree_edges subject) /\
  (forall subjects,
    forest_edges (erase_values_forest subjects) = forest_edges subjects).
Proof.
  apply trie_forest_induction.
  - intros cell children children_edges. simpl. exact children_edges.
  - reflexivity.
  - intros subject subject_edges rest rest_edges.
    simpl. now rewrite subject_edges, rest_edges.
Qed.

Theorem erased_root_has_no_reverse_payloads :
  (forall subject,
    reverse_payloads_tree (erase_values_tree subject) = []) /\
  (forall subjects,
    reverse_payloads_forest (erase_values_forest subjects) = []).
Proof.
  apply trie_forest_induction.
  - intros cell children children_empty. simpl. now rewrite children_empty.
  - reflexivity.
  - intros subject subject_empty rest rest_empty.
    simpl. now rewrite subject_empty, rest_empty.
Qed.

Lemma dispose_cell_has_no_phase_a_value_event : forall cell,
  removed_without_prune (dispose_cell cell) = [] /\
  disposed_values (dispose_cell cell) = [].
Proof.
  intros [value|]; simpl; try tauto.
  destruct value as [member|par_id children|key multiplicity|position]; simpl;
    try tauto; induction children; simpl; auto.
Qed.

Lemma prune_segment_has_no_phase_a_value_event : forall parent_has_value,
  removed_without_prune (prune_segment parent_has_value) = [] /\
  disposed_values (prune_segment parent_has_value) = [].
Proof. destruct parent_has_value; simpl; tauto. Qed.

Theorem topology_phase_contains_no_real_value_operation :
  (forall subject,
      removed_without_prune (drain_tree subject) = [] /\
      disposed_values (drain_tree subject) = []) /\
  (forall subjects parent_has_value,
      removed_without_prune (drain_forest subjects parent_has_value) = [] /\
      disposed_values (drain_forest subjects parent_has_value) = []).
Proof.
  apply trie_forest_induction.
  - intros cell children children_empty.
    specialize (children_empty
      (match cell with Some _ => true | None => false end)).
    destruct children_empty as [children_removed children_disposed]. simpl.
    repeat rewrite removed_without_prune_app.
    repeat rewrite disposed_values_app.
    destruct (dispose_cell_has_no_phase_a_value_event cell)
      as [cell_removed cell_disposed].
    rewrite children_removed, children_disposed, cell_removed, cell_disposed.
    tauto.
  - intro parent_has_value. simpl. tauto.
  - intros subject [subject_removed subject_disposed]
      rest rest_empty parent_has_value.
    specialize (rest_empty parent_has_value).
    destruct rest_empty as [rest_removed rest_disposed]. simpl.
    repeat rewrite removed_without_prune_app.
    repeat rewrite disposed_values_app.
    destruct (prune_segment_has_no_phase_a_value_event parent_has_value)
      as [segment_removed segment_disposed].
    rewrite rest_removed, rest_disposed, subject_removed, subject_disposed,
      segment_removed, segment_disposed. tauto.
Qed.

Theorem phase_b_requires_zero_values_and_only_prunes_topology : forall
    phase_a zero_values,
  run_phase_b phase_a zero_values =
    BeginTopologyPhase :: drain_tree (phase_a_value_free_root phase_a) /\
  removed_without_prune (run_phase_b phase_a zero_values) = [] /\
  disposed_values (run_phase_b phase_a zero_values) = [].
Proof.
  intros phase_a zero_values. unfold run_phase_b. split; [reflexivity|].
  pose proof (proj1 topology_phase_contains_no_real_value_operation
    (phase_a_value_free_root phase_a)) as [removed disposed].
  simpl. tauto.
Qed.

Theorem terminal_two_phase_trace_is_exact_and_topology_complete :
  forall subject,
    removed_without_prune (terminal_two_phase_trace subject) =
      preorder_payloads_tree subject /\
    disposed_values (terminal_two_phase_trace subject) =
      preorder_payloads_tree subject /\
    queued_par_jobs (terminal_two_phase_trace subject) =
      flat_map par_job (preorder_payloads_tree subject) /\
    detached_payloads (run_phase_b (run_phase_a subject)
      (proj1 erase_values_produces_a_value_free_root subject)) = [] /\
    prune_widths (terminal_two_phase_trace subject) =
      repeat 1 (tree_edges subject).
Proof.
  intro subject.
  destruct (phase_a_extracts_and_immediately_disposes_every_real_value_once
    subject) as [removed [disposed [jobs immediate]]].
  destruct (phase_b_requires_zero_values_and_only_prunes_topology
    (run_phase_a subject)
    (proj1 erase_values_produces_a_value_free_root subject))
    as [_ [phase_b_removed phase_b_disposed]].
  repeat split.
  - change (removed_without_prune
      (phase_a_value_trace subject ++
       run_phase_b (run_phase_a subject)
         (proj1 erase_values_produces_a_value_free_root subject)) =
      preorder_payloads_tree subject).
    rewrite removed_without_prune_app, removed, phase_b_removed.
    apply app_nil_r.
  - change (disposed_values
      (phase_a_value_trace subject ++
       run_phase_b (run_phase_a subject)
         (proj1 erase_values_produces_a_value_free_root subject)) =
      preorder_payloads_tree subject).
    rewrite disposed_values_app, disposed, phase_b_disposed.
    apply app_nil_r.
  - change (queued_par_jobs
      (phase_a_value_trace subject ++
       BeginTopologyPhase :: drain_tree (erase_values_tree subject)) =
      flat_map par_job (preorder_payloads_tree subject)).
    rewrite queued_par_jobs_app, jobs. simpl.
    rewrite (proj1 drain_transfers_every_par_to_one_explicit_pda_job).
    rewrite (proj1 erased_root_has_no_reverse_payloads). apply app_nil_r.
  - unfold run_phase_b. simpl.
    rewrite (proj1 drain_detaches_originals_exactly_once_in_reverse_postorder).
    apply (proj1 erased_root_has_no_reverse_payloads).
  - change (prune_widths
      (phase_a_value_trace subject ++
       BeginTopologyPhase :: drain_tree (erase_values_tree subject)) =
      repeat 1 (tree_edges subject)).
    rewrite prune_widths_app.
    rewrite (proj1
      (phase_a_changes_no_topology_and_closes_before_validation subject)).
    simpl. rewrite (proj1 drain_prunes_every_logical_edge_once).
    rewrite (proj1 erase_values_preserves_topology). reflexivity.
Qed.

Definition branch_safe_segment
    (parent_has_value : bool) (segment : list teardown_event) : Prop :=
  match parent_has_value with
  | true => segment = [PruneSpan 1]
  | false => segment = [InstallBarrier; PruneSpan 1; DetachBarrier]
  end.

Theorem immediate_parent_barrier_protocol : forall parent_has_value,
  branch_safe_segment parent_has_value (prune_segment parent_has_value).
Proof. destruct parent_has_value; reflexivity. Qed.

Theorem barrier_never_overwrites_a_real_parent_value :
  ~ In InstallBarrier (prune_segment true).
Proof. simpl. intuition discriminate. Qed.

Theorem absent_parent_is_guarded_before_and_ungarded_after_prune :
  prune_segment false = [InstallBarrier; PruneSpan 1; DetachBarrier].
Proof. reflexivity. Qed.

Theorem one_edge_prune_strictly_decreases_remaining_topology : forall edges,
  0 < edges -> edges - 1 < edges.
Proof. intros edges positive. lia. Qed.

(** Value-free leaves are real topology.  The line, dense fan-out, and comb
    families provide unbounded adversarial witnesses without imposing an
    artificial depth or branching limit on the universal theorems above. *)
Definition topology_leaf : trie := Node None FNil.

Fixpoint topology_line (depth : nat) : trie :=
  match depth with
  | 0 => topology_leaf
  | S rest => Node None (FCons (topology_line rest) FNil)
  end.

Fixpoint topology_dense_forest (width : nat) : forest :=
  match width with
  | 0 => FNil
  | S rest => FCons topology_leaf (topology_dense_forest rest)
  end.

Definition topology_dense (width : nat) : trie :=
  Node None (topology_dense_forest width).

Fixpoint topology_comb (depth : nat) : trie :=
  match depth with
  | 0 => topology_leaf
  | S rest =>
      Node None
        (FCons (topology_comb rest) (FCons topology_leaf FNil))
  end.

Theorem topology_line_has_exact_depth_edges : forall depth,
  tree_edges (topology_line depth) = depth.
Proof. induction depth; simpl; lia. Qed.

Lemma topology_dense_forest_has_exact_width : forall width,
  forest_edges (topology_dense_forest width) = width.
Proof. induction width; simpl; lia. Qed.

Theorem topology_dense_has_exact_width_edges : forall width,
  tree_edges (topology_dense width) = width.
Proof. intro width. simpl. apply topology_dense_forest_has_exact_width. Qed.

Theorem topology_comb_has_twice_depth_edges : forall depth,
  tree_edges (topology_comb depth) = 2 * depth.
Proof. induction depth; simpl; lia. Qed.

Corollary topology_adversaries_are_drained_edge_by_edge : forall depth width,
  prune_widths (drain_tree (topology_line depth)) = repeat 1 depth /\
  prune_widths (drain_tree (topology_dense width)) = repeat 1 width /\
  prune_widths (drain_tree (topology_comb depth)) = repeat 1 (2 * depth).
Proof.
  intros depth width.
  repeat rewrite (proj1 drain_prunes_every_logical_edge_once).
  now rewrite topology_line_has_exact_depth_edges,
    topology_dense_has_exact_width_edges,
    topology_comb_has_twice_depth_edges.
Qed.

(** Wrapper ownership is one [Arc<AnchorRoot<V>>].  Exactly one raw PathMap
    header lives inside each anchor.  The abstract [anchor_root_payload_copies]
    field counts that wrapper-level root slot; it makes no claim that compressed
    internal nodes or values are physically unique.  Immutable wrapper Clone
    changes only the Arc count.  The standard
    library's final Arc destruction is the sole last-owner election and invokes
    [AnchorRoot::drop] exactly once.  No snapshot of [Arc::strong_count] is part
    of the refinement contract. *)
Record anchor_state : Type := AnchorState {
  anchor_wrapper_arcs : nat;
  anchor_raw_headers : nat;
  anchor_root_payload_copies : nat
}.

Definition live_anchor (wrapper_arcs : nat) : anchor_state :=
  AnchorState wrapper_arcs 1 1.

Definition dead_anchor : anchor_state := AnchorState 0 0 0.

Definition anchor_well_formed (anchor : anchor_state) : Prop :=
  match anchor_wrapper_arcs anchor with
  | 0 =>
      anchor_raw_headers anchor = 0 /\
      anchor_root_payload_copies anchor = 0
  | S _ =>
      anchor_raw_headers anchor = 1 /\
      anchor_root_payload_copies anchor = 1
  end.

Inductive anchor_event : Type :=
| CloneWrapperArc
| DropNonlastWrapperArc
| ArcElectsFinalWrapper
| BeginTerminalTwoPhasePda
| ArcMakeMutLinearization
| CloneRawHeaderForFreshAnchor
| RegisterFreshAnchor
| AbortMakeMutWithoutPublication.

Record anchor_transition : Type := AnchorTransition {
  anchor_transition_initial : anchor_state;
  anchor_transition_final : anchor_state;
  anchor_transition_trace : list anchor_event
}.

Definition immutable_wrapper_clone
    (existing_others : nat) : anchor_transition :=
  AnchorTransition
    (live_anchor (S existing_others))
    (live_anchor (S (S existing_others)))
    [CloneWrapperArc].

Definition nonlast_wrapper_drop
    (remaining_others : nat) : anchor_transition :=
  AnchorTransition
    (live_anchor (S (S remaining_others)))
    (live_anchor (S remaining_others))
    [DropNonlastWrapperArc].

Definition final_wrapper_drop : anchor_transition :=
  AnchorTransition
    (live_anchor 1)
    dead_anchor
    [ArcElectsFinalWrapper; BeginTerminalTwoPhasePda].

Record make_mut_transfer : Type := MakeMutTransfer {
  make_mut_old_initial : anchor_state;
  make_mut_old_final : anchor_state;
  make_mut_fresh_final : anchor_state;
  make_mut_trace : list anchor_event
}.

Definition shared_arc_make_mut (remaining_others : nat) : make_mut_transfer :=
  MakeMutTransfer
    (live_anchor (S (S remaining_others)))
    (live_anchor (S remaining_others))
    (live_anchor 1)
    [ArcMakeMutLinearization; CloneRawHeaderForFreshAnchor;
     RegisterFreshAnchor].

Definition aborted_arc_make_mut (existing_others : nat) : anchor_transition :=
  AnchorTransition
    (live_anchor (S (S existing_others)))
    (live_anchor (S (S existing_others)))
    [ArcMakeMutLinearization; AbortMakeMutWithoutPublication].

Theorem immutable_wrapper_clone_does_not_clone_raw_header_or_root_value :
  forall existing_others,
    anchor_raw_headers
      (anchor_transition_final (immutable_wrapper_clone existing_others)) = 1 /\
    anchor_root_payload_copies
      (anchor_transition_final (immutable_wrapper_clone existing_others)) = 1 /\
    anchor_well_formed
      (anchor_transition_final (immutable_wrapper_clone existing_others)).
Proof. intros; repeat split; reflexivity. Qed.

Theorem nonlast_wrapper_drop_cannot_touch_the_raw_pathmap :
  forall remaining_others,
    anchor_transition_trace (nonlast_wrapper_drop remaining_others) =
      [DropNonlastWrapperArc] /\
    anchor_raw_headers
      (anchor_transition_final (nonlast_wrapper_drop remaining_others)) = 1 /\
    anchor_root_payload_copies
      (anchor_transition_final (nonlast_wrapper_drop remaining_others)) = 1.
Proof. intros; repeat split; reflexivity. Qed.

Theorem two_wrapper_arc_sequence_elects_exactly_one_final_drop :
  anchor_transition_final (nonlast_wrapper_drop 0) =
    anchor_transition_initial final_wrapper_drop /\
  anchor_transition_trace final_wrapper_drop =
    [ArcElectsFinalWrapper; BeginTerminalTwoPhasePda] /\
  anchor_transition_final final_wrapper_drop = dead_anchor.
Proof. repeat split; reflexivity. Qed.

Theorem make_mut_creates_one_well_formed_fresh_anchor :
  forall remaining_others,
    anchor_well_formed
      (make_mut_old_final (shared_arc_make_mut remaining_others)) /\
    anchor_well_formed
      (make_mut_fresh_final (shared_arc_make_mut remaining_others)) /\
    anchor_raw_headers
      (make_mut_fresh_final (shared_arc_make_mut remaining_others)) = 1 /\
    anchor_root_payload_copies
      (make_mut_fresh_final (shared_arc_make_mut remaining_others)) = 1.
Proof. intros; repeat split; reflexivity. Qed.

Theorem make_mut_abort_leaves_no_unregistered_raw_header :
  forall existing_others,
    anchor_transition_final (aborted_arc_make_mut existing_others) =
      anchor_transition_initial (aborted_arc_make_mut existing_others) /\
    anchor_well_formed
      (anchor_transition_final (aborted_arc_make_mut existing_others)).
Proof. intros; split; [reflexivity|]. split; reflexivity. Qed.

Inductive campaign_owned_exit : Type :=
| PrivateWriterEarlyDrop
| PrivateIteratorEarlyDrop
| MalformedConstructionDiscard
| FailedFreshCandidate
| RetiredWholeAuthorityAfterPublication
| FinalAnchorRootTeardown.

Definition exclusive_terminal_footprint
    (owned : PathMapPhysicalOwnership.allocation_region)
    (external : list PathMapPhysicalOwnership.allocation_region) : Prop :=
  Forall
    (PathMapPhysicalOwnership.structural_candidate_disjoint owned)
    external.

(** A final wrapper is only authority over the raw header.  Terminal physical
    destruction additionally requires either an exclusive node-and-value
    allocation footprint or a separately verified public-drain refinement.
    No constructor denotes removal or replacement of a live subtree. *)
Record terminal_root_authority : Type := TerminalRootAuthority {
  terminal_exit : campaign_owned_exit;
  terminal_owned_region : PathMapPhysicalOwnership.allocation_region;
  terminal_external_regions :
    list PathMapPhysicalOwnership.allocation_region;
  terminal_verified_public_drain : bool;
  terminal_storage_safety :
    exclusive_terminal_footprint
      terminal_owned_region terminal_external_regions \/
    terminal_verified_public_drain = true
}.

Definition empty_terminal_region :
    PathMapPhysicalOwnership.allocation_region :=
  PathMapPhysicalOwnership.AllocationRegion 0 [] [].

Definition verified_terminal_authority
    (exit : campaign_owned_exit) : terminal_root_authority :=
  @TerminalRootAuthority exit empty_terminal_region [] true
    (or_intror eq_refl).

Definition exclusive_terminal_authority
    (exit : campaign_owned_exit)
    (owned : PathMapPhysicalOwnership.allocation_region)
    (external : list PathMapPhysicalOwnership.allocation_region)
    (exclusive : exclusive_terminal_footprint owned external)
    : terminal_root_authority :=
  @TerminalRootAuthority exit owned external false (or_introl exclusive).

Theorem terminal_authority_exposes_storage_safety : forall authority,
  exclusive_terminal_footprint
    (terminal_owned_region authority)
    (terminal_external_regions authority) \/
  terminal_verified_public_drain authority = true.
Proof. intro authority. exact (terminal_storage_safety authority). Qed.

Record release_result : Type := ReleaseResult {
  release_trace : list teardown_event;
  residual_campaign_edges : nat;
  recursively_dropped_unique_edges : nat;
  preserved_sibling : option trie
}.

Definition release_terminal_root
    (_ : terminal_root_authority) (subject : trie) : release_result :=
  ReleaseResult (terminal_two_phase_trace subject) 0 0 None.

Definition canonical_empty_root : trie := Node None FNil.

(** The canonical empty representation owns no value and no edge.  Its final
    anchor therefore takes ordinary empty Drop directly: no cursor is opened,
    and neither Phase A nor Phase B is entered.  Every nonempty or
    noncanonical root retains the two-phase protocol. *)
Definition release_final_anchor
    (authority : terminal_root_authority) (subject : trie) : release_result :=
  match subject with
  | Node None FNil => ReleaseResult [] 0 0 None
  | _ => release_terminal_root authority subject
  end.

Theorem canonical_empty_final_anchor_is_an_ordinary_work_free_drop :
  forall authority,
  release_trace (release_final_anchor authority canonical_empty_root) = [] /\
  removed_without_prune
    (release_trace (release_final_anchor authority canonical_empty_root)) = [] /\
  disposed_values
    (release_trace (release_final_anchor authority canonical_empty_root)) = [] /\
  prune_widths
    (release_trace (release_final_anchor authority canonical_empty_root)) = [] /\
  queued_par_jobs
    (release_trace (release_final_anchor authority canonical_empty_root)) = [].
Proof. intro authority. repeat split; reflexivity. Qed.

Definition release_cross_anchor_shared_image
    (_ : terminal_root_authority) (subject sibling : trie) : release_result :=
  ReleaseResult (terminal_two_phase_trace subject) 0 0 (Some sibling).

Theorem terminal_release_never_uses_recursive_nonempty_pathmap_drop :
  forall authority subject,
    residual_campaign_edges (release_terminal_root authority subject) = 0 /\
    recursively_dropped_unique_edges
      (release_terminal_root authority subject) = 0.
Proof. repeat split; reflexivity. Qed.

Theorem cross_anchor_teardown_preserves_the_sibling_image :
  forall authority subject sibling,
  preserved_sibling
    (release_cross_anchor_shared_image authority subject sibling) =
    Some sibling /\
  recursively_dropped_unique_edges
    (release_cross_anchor_shared_image authority subject sibling) = 0 /\
  removed_without_prune
    (release_trace
      (release_cross_anchor_shared_image authority subject sibling)) =
    preorder_payloads_tree subject /\
  disposed_values
    (release_trace
      (release_cross_anchor_shared_image authority subject sibling)) =
    preorder_payloads_tree subject.
Proof.
  intros authority subject sibling. repeat split; try reflexivity.
  - exact (proj1
      (terminal_two_phase_trace_is_exact_and_topology_complete subject)).
  - exact (proj1 (proj2
      (terminal_two_phase_trace_is_exact_and_topology_complete subject))).
Qed.

(** The production strengthening is stated over actual node-and-value regions.
    Logical prefix semantics and final-wrapper election cannot manufacture
    this premise. *)
Theorem terminal_region_release_preserves_disjoint_sibling : forall
    (subject sibling : PathMapPhysicalOwnership.physical_anchor)
    (owned_region sibling_region : PathMapPhysicalOwnership.allocation_region),
  PathMapPhysicalOwnership.anchor_nodes subject =
    PathMapPhysicalOwnership.region_nodes owned_region ->
  PathMapPhysicalOwnership.anchor_nodes sibling =
    PathMapPhysicalOwnership.region_nodes sibling_region ->
  PathMapPhysicalOwnership.structural_candidate_disjoint
    owned_region sibling_region ->
  PathMapPhysicalOwnership.physical_image_intact_after_release subject sibling.
Proof.
  intros subject sibling owned_region sibling_region subject_nodes sibling_nodes
    [node_disjoint value_disjoint].
  apply PathMapPhysicalOwnership.release_preserves_disjoint_anchor_images.
  now rewrite subject_nodes, sibling_nodes.
Qed.

Theorem final_anchor_drop_extracts_and_disposes_its_root_values_once :
  forall authority subject,
  removed_without_prune
    (release_trace (release_final_anchor authority subject)) =
    preorder_payloads_tree subject /\
  disposed_values
    (release_trace (release_final_anchor authority subject)) =
    preorder_payloads_tree subject.
Proof.
  intros authority [cell children].
  destruct cell as [value|], children as [|child rest]; simpl.
  - pose proof
      (terminal_two_phase_trace_is_exact_and_topology_complete
        (Node (Some value) FNil)) as [removed [disposed tail]].
    split; assumption.
  - pose proof
      (terminal_two_phase_trace_is_exact_and_topology_complete
        (Node (Some value) (FCons child rest)))
      as [removed [disposed tail]]. split; assumption.
  - split; reflexivity.
  - pose proof
      (terminal_two_phase_trace_is_exact_and_topology_complete
        (Node None (FCons child rest)))
      as [removed [disposed tail]]. split; assumption.
Qed.

Theorem final_anchor_drop_is_the_only_raw_header_teardown :
  anchor_transition_trace final_wrapper_drop =
    [ArcElectsFinalWrapper; BeginTerminalTwoPhasePda] /\
  anchor_raw_headers (anchor_transition_final final_wrapper_drop) = 0 /\
  anchor_root_payload_copies (anchor_transition_final final_wrapper_drop) = 0.
Proof. repeat split; reflexivity. Qed.

Definition close_campaign_owned_exit
    (authority : terminal_root_authority) (detached : trie) : release_result :=
  release_terminal_root authority detached.

Theorem every_campaign_owned_exit_drains_before_release :
  forall authority detached,
    release_trace (close_campaign_owned_exit authority detached) =
      terminal_two_phase_trace detached /\
    removed_without_prune
      (release_trace (close_campaign_owned_exit authority detached)) =
      preorder_payloads_tree detached /\
    disposed_values
      (release_trace (close_campaign_owned_exit authority detached)) =
      preorder_payloads_tree detached /\
    prune_widths
      (release_trace (close_campaign_owned_exit authority detached)) =
      repeat 1 (tree_edges detached) /\
    residual_campaign_edges
      (close_campaign_owned_exit authority detached) = 0 /\
    recursively_dropped_unique_edges
      (close_campaign_owned_exit authority detached) = 0.
Proof.
  intros authority detached. simpl. repeat split; try reflexivity.
  - exact (proj1
      (terminal_two_phase_trace_is_exact_and_topology_complete detached)).
  - exact (proj1 (proj2
      (terminal_two_phase_trace_is_exact_and_topology_complete detached))).
  - exact (proj2 (proj2 (proj2 (proj2
      (terminal_two_phase_trace_is_exact_and_topology_complete detached))))).
Qed.

Theorem terminal_exit_is_always_a_whole_private_root : forall exit,
  In exit
    [PrivateWriterEarlyDrop; PrivateIteratorEarlyDrop;
     MalformedConstructionDiscard; FailedFreshCandidate;
     RetiredWholeAuthorityAfterPublication; FinalAnchorRootTeardown].
Proof. intro exit. destruct exit; simpl; tauto. Qed.

Inductive build_failure : Type :=
| FramingFailure
| ChildPdaFailure
| DeferredActFailure
| AssociationFailure
| CanonicalityFailure.

Definition select_failure
    (framing child deferred_act association canonicality : bool)
    : option build_failure :=
  if framing then Some FramingFailure
  else if child then Some ChildPdaFailure
  else if deferred_act then Some DeferredActFailure
  else if association then Some AssociationFailure
  else if canonicality then Some CanonicalityFailure
  else None.

Inductive builder_outcome : Type :=
| AcceptComplete
| RejectPrivate : build_failure -> builder_outcome.

Record builder_result : Type := BuilderResult {
  builder_trace : list teardown_event;
  publication_count : nat;
  unreleased_private_edges : nat;
  builder_recursive_unique_drop_edges : nat;
  returned_failure : option build_failure
}.

Definition finalize_private_builder
    (_ : terminal_root_authority)
    (outcome : builder_outcome) (partial : trie) : builder_result :=
  match outcome with
  | AcceptComplete => BuilderResult [PublishComplete] 1 0 0 None
  | RejectPrivate failure =>
      BuilderResult (terminal_two_phase_trace partial) 0 0 0 (Some failure)
  end.

Theorem malformed_or_rejected_partial_build_is_fully_drained :
  forall authority partial failure,
    builder_trace
      (finalize_private_builder authority (RejectPrivate failure) partial) =
      terminal_two_phase_trace partial /\
    publication_count
      (finalize_private_builder authority (RejectPrivate failure) partial) = 0 /\
    unreleased_private_edges
      (finalize_private_builder authority (RejectPrivate failure) partial) = 0 /\
    builder_recursive_unique_drop_edges
      (finalize_private_builder authority (RejectPrivate failure) partial) = 0.
Proof. intros; repeat split; reflexivity. Qed.

Theorem complete_builder_publishes_exactly_once : forall authority complete,
  builder_trace (finalize_private_builder authority AcceptComplete complete) =
    [PublishComplete] /\
  publication_count
    (finalize_private_builder authority AcceptComplete complete) = 1 /\
  unreleased_private_edges
    (finalize_private_builder authority AcceptComplete complete) = 0.
Proof. intros; repeat split; reflexivity. Qed.

Theorem teardown_preserves_selected_error_precedence :
  forall authority partial framing child deferred_act association canonicality failure,
    select_failure framing child deferred_act association canonicality =
      Some failure ->
    returned_failure
      (finalize_private_builder authority (RejectPrivate failure) partial) =
      Some failure.
Proof. intros; reflexivity. Qed.

Theorem child_failure_precedes_deferred_act_failure :
  select_failure false true true true true = Some ChildPdaFailure.
Proof. reflexivity. Qed.

(** Production teardown does not accept an arbitrary callback.  Rust currently
    instantiates [GuardedPathMap] with exactly two payload/disposer families:
    [Multiplicity]/[MultiplicityDisposer] and
    [CandidatePosition]/[CandidatePositionDisposer].  Set and Par remain useful
    model/campaign payloads, but they are not advertised as production guarded
    instantiations. *)
Inductive named_teardown_policy : Type :=
| UnitSetPolicy
| ParMapPdaPolicy
| BagMultiplicityPolicy
| CandidatePositionPolicy.

Definition production_guarded_teardown_policies :
    list named_teardown_policy :=
  [BagMultiplicityPolicy; CandidatePositionPolicy].

Definition model_only_teardown_policies : list named_teardown_policy :=
  [UnitSetPolicy; ParMapPdaPolicy].

Definition all_named_teardown_policies : list named_teardown_policy :=
  production_guarded_teardown_policies ++ model_only_teardown_policies.

Definition policy_for_payload (value : payload) : named_teardown_policy :=
  match value with
  | SetMember _ => UnitSetPolicy
  | MapPar _ _ => ParMapPdaPolicy
  | BagMultiplicity _ _ => BagMultiplicityPolicy
  | CandidatePosition _ => CandidatePositionPolicy
  end.

Definition production_guarded_payload (value : payload) : Prop :=
  match value with
  | BagMultiplicity _ _ | CandidatePosition _ => True
  | _ => False
  end.

Definition model_only_payload (value : payload) : Prop :=
  match value with
  | SetMember _ | MapPar _ _ => True
  | _ => False
  end.

Inductive production_policy_id : Type :=
| MultiplicityProductionPolicy
| CandidatePositionProductionPolicy.

Definition production_policy_catalog : list production_policy_id :=
  [MultiplicityProductionPolicy; CandidatePositionProductionPolicy].

Definition production_policy_binding (policy : production_policy_id) : Prop :=
  match policy with
  | MultiplicityProductionPolicy =>
      In BagMultiplicityPolicy production_guarded_teardown_policies
  | CandidatePositionProductionPolicy =>
      In CandidatePositionPolicy production_guarded_teardown_policies
  end.

Theorem every_production_policy_is_bound : forall policy,
  production_policy_binding policy.
Proof. intro policy. destruct policy; cbn; auto. Qed.

Theorem production_policy_catalog_is_unique_complete_and_exact :
  NoDup production_policy_catalog /\
  Forall production_policy_binding production_policy_catalog /\
  length production_policy_catalog = 2.
Proof.
  split.
  - repeat constructor; simpl; intuition discriminate.
  - split.
    + apply Forall_forall. intros policy _.
      apply every_production_policy_is_bound.
    + reflexivity.
Qed.

Inductive teardown_policy_request : Type :=
| RequestNamedPolicy : named_teardown_policy -> teardown_policy_request
| RequestCallerClosure : teardown_policy_request.

Definition resolve_teardown_policy
    (request : teardown_policy_request) : option named_teardown_policy :=
  match request with
  | RequestNamedPolicy policy => Some policy
  | RequestCallerClosure => None
  end.

Inductive policy_dispatch_outcome : Type :=
| PolicyReturned : list teardown_event -> policy_dispatch_outcome
| PolicyUnwound : policy_dispatch_outcome.

Definition dispatch_named_policy
    (value : payload) : policy_dispatch_outcome :=
  PolicyReturned (phase_a_dispose_value value).

Theorem named_policy_registries_are_scoped_and_complete :
  NoDup production_guarded_teardown_policies /\
  length production_guarded_teardown_policies = 2 /\
  NoDup model_only_teardown_policies /\
  length model_only_teardown_policies = 2 /\
  (forall value, production_guarded_payload value ->
    In (policy_for_payload value) production_guarded_teardown_policies) /\
  (forall value, model_only_payload value ->
    In (policy_for_payload value) model_only_teardown_policies) /\
  (forall value,
    In (policy_for_payload value) production_guarded_teardown_policies ->
    production_guarded_payload value) /\
  resolve_teardown_policy RequestCallerClosure = None.
Proof.
  repeat split; try reflexivity.
  - repeat constructor; simpl; intuition discriminate.
  - repeat constructor; simpl; intuition discriminate.
  - intros value production. destruct value; simpl in production |- *;
      auto; contradiction.
  - intros value modeled. destruct value; simpl in modeled |- *;
      auto; contradiction.
  - intros value present. destruct value; simpl in present |- *;
      intuition discriminate.
Qed.

Theorem production_policy_dispatch_never_unwinds : forall value,
  exists trace,
    dispatch_named_policy value = PolicyReturned trace /\
    trace = phase_a_dispose_value value.
Proof. intro value. exists (phase_a_dispose_value value). split; reflexivity. Qed.

(** A raw header remains quarantined until both phases have completed.
    Unexpected panic and any allocation failure are fail-stop outcomes: they
    do not resume execution, publish, leak into a live process, or invoke
    ordinary recursive PathMap Drop.  Even a final wrapper is not an
    allocation-free promise: public zipper mutation may CoW shared internal
    nodes or values. *)
Inductive teardown_allocation_outcome : Type :=
| NoAllocationRequired
| AllocationSucceeded
| AllocationFailed.

Inductive teardown_process_outcome : Type :=
| LiveCompleted
| ProcessFailStopped.

Record quarantined_teardown_result : Type := QuarantinedTeardownResult {
  quarantine_process : teardown_process_outcome;
  quarantine_trace : list teardown_event;
  quarantine_remaining_edges : nat;
  quarantine_remaining_payloads : nat;
  quarantine_retains_raw_header : bool;
  quarantine_explicit_raw_drops : nat;
  quarantine_recursive_raw_drops : nat;
  quarantine_live_continuations : nat;
  quarantine_allocation_attempts : nat
}.

Definition payload_count (subject : trie) : nat :=
  length (reverse_payloads_tree subject).

Definition completed_quarantined_teardown
    (subject : trie) (allocation_attempts : nat)
    : quarantined_teardown_result :=
  QuarantinedTeardownResult LiveCompleted (terminal_two_phase_trace subject)
    0 0 false 1 0 1 allocation_attempts.

Definition fail_stopped_quarantined_teardown
    (subject : trie) (allocation_attempts : nat)
    : quarantined_teardown_result :=
  QuarantinedTeardownResult ProcessFailStopped []
    (tree_edges subject) (payload_count subject) true 0 0 0
    allocation_attempts.

Definition run_quarantined_teardown
    (_ : terminal_root_authority)
    (allocation : teardown_allocation_outcome)
    (unexpected_panic : bool)
    (subject : trie) : quarantined_teardown_result :=
  if unexpected_panic then fail_stopped_quarantined_teardown subject 0
  else
    match allocation with
    | NoAllocationRequired =>
        completed_quarantined_teardown subject 0
    | AllocationSucceeded =>
        completed_quarantined_teardown subject 1
    | AllocationFailed =>
        fail_stopped_quarantined_teardown subject 1
    end.

Definition explicit_raw_drop_is_empty
    (result : quarantined_teardown_result) : Prop :=
  quarantine_explicit_raw_drops result = 1 ->
    quarantine_remaining_edges result = 0 /\
    quarantine_remaining_payloads result = 0 /\
    quarantine_retains_raw_header result = false.

Definition fail_stop_is_terminal
    (result : quarantined_teardown_result) : Prop :=
  quarantine_process result = ProcessFailStopped ->
    quarantine_live_continuations result = 0 /\
    quarantine_explicit_raw_drops result = 0 /\
    quarantine_recursive_raw_drops result = 0 /\
    quarantine_retains_raw_header result = true.

Definition no_live_process_quarantine_leak
    (result : quarantined_teardown_result) : Prop :=
  quarantine_process result = LiveCompleted ->
    quarantine_retains_raw_header result = false.

Theorem every_quarantine_outcome_guards_raw_drop : forall
    authority allocation unexpected_panic subject,
  explicit_raw_drop_is_empty
    (run_quarantined_teardown authority allocation unexpected_panic subject) /\
  fail_stop_is_terminal
    (run_quarantined_teardown authority allocation unexpected_panic subject) /\
  no_live_process_quarantine_leak
    (run_quarantined_teardown authority allocation unexpected_panic subject) /\
  quarantine_recursive_raw_drops
    (run_quarantined_teardown authority allocation unexpected_panic subject) = 0.
Proof.
  intros authority allocation unexpected_panic subject.
  destruct unexpected_panic, allocation; simpl;
    unfold explicit_raw_drop_is_empty, fail_stop_is_terminal,
      no_live_process_quarantine_leak; simpl; intuition discriminate.
Qed.

Theorem unexpected_panic_is_fail_stop_without_recursive_drop : forall
    authority allocation subject,
  quarantine_process
    (run_quarantined_teardown authority allocation true subject) =
      ProcessFailStopped /\
  quarantine_live_continuations
    (run_quarantined_teardown authority allocation true subject) = 0 /\
  quarantine_recursive_raw_drops
    (run_quarantined_teardown authority allocation true subject) = 0 /\
  quarantine_retains_raw_header
    (run_quarantined_teardown authority allocation true subject) = true.
Proof. intros. destruct allocation; repeat split; reflexivity. Qed.

Theorem successful_quarantine_preserves_exact_par_handoff : forall
    authority allocation subject,
  quarantine_process
    (run_quarantined_teardown authority allocation false subject) =
      LiveCompleted ->
  queued_par_jobs
    (quarantine_trace
      (run_quarantined_teardown authority allocation false subject)) =
    flat_map par_job (preorder_payloads_tree subject).
Proof.
  intros authority allocation subject completed.
  destruct allocation; simpl in completed |- *;
    try discriminate;
    exact (proj1 (proj2 (proj2
      (terminal_two_phase_trace_is_exact_and_topology_complete subject)))).
Qed.

Theorem terminal_teardown_reports_actual_allocation_path :
  forall authority subject,
  quarantine_process
    (run_quarantined_teardown authority
      NoAllocationRequired false subject) = LiveCompleted /\
  quarantine_allocation_attempts
    (run_quarantined_teardown authority
      NoAllocationRequired false subject) = 0 /\
  quarantine_process
    (run_quarantined_teardown authority
      AllocationSucceeded false subject) = LiveCompleted /\
  quarantine_allocation_attempts
    (run_quarantined_teardown authority
      AllocationSucceeded false subject) = 1.
Proof. intros authority subject. repeat split; reflexivity. Qed.

Theorem terminal_allocation_failure_is_terminal : forall authority subject,
  quarantine_process
    (run_quarantined_teardown authority
      AllocationFailed false subject) = ProcessFailStopped /\
  quarantine_allocation_attempts
    (run_quarantined_teardown authority
      AllocationFailed false subject) = 1 /\
  quarantine_live_continuations
    (run_quarantined_teardown authority
      AllocationFailed false subject) = 0 /\
  quarantine_recursive_raw_drops
    (run_quarantined_teardown authority
      AllocationFailed false subject) = 0.
Proof. intros authority subject. repeat split; reflexivity. Qed.

Inductive quarantine_invariant_id : Type :=
| ScopedProductionPolicyRegistryInvariant
| NonUnwindingPolicyDispatchInvariant
| RawHeaderQuarantineInvariant
| ExplicitDropAtEmptyInvariant
| FailStopNoContinuationInvariant
| FailStopNoRecursiveDropInvariant
| NoLiveProcessLeakInvariant
| ParHandoffExactOnceInvariant
| TerminalAllocationPathExplicitInvariant
| TerminalAllocationFailStopInvariant.

Definition quarantine_invariant_catalog : list quarantine_invariant_id :=
  [ScopedProductionPolicyRegistryInvariant;
   NonUnwindingPolicyDispatchInvariant;
   RawHeaderQuarantineInvariant;
   ExplicitDropAtEmptyInvariant;
   FailStopNoContinuationInvariant;
   FailStopNoRecursiveDropInvariant;
   NoLiveProcessLeakInvariant;
   ParHandoffExactOnceInvariant;
   TerminalAllocationPathExplicitInvariant;
   TerminalAllocationFailStopInvariant].

Definition quarantine_invariant_binding
    (invariant : quarantine_invariant_id) : Prop :=
  match invariant with
  | ScopedProductionPolicyRegistryInvariant =>
      NoDup production_guarded_teardown_policies /\
      length production_guarded_teardown_policies = 2 /\
      NoDup model_only_teardown_policies /\
      length model_only_teardown_policies = 2 /\
      (forall value, production_guarded_payload value ->
        In (policy_for_payload value) production_guarded_teardown_policies) /\
      (forall value, model_only_payload value ->
        In (policy_for_payload value) model_only_teardown_policies) /\
      (forall value,
        In (policy_for_payload value) production_guarded_teardown_policies ->
        production_guarded_payload value) /\
      resolve_teardown_policy RequestCallerClosure = None
  | NonUnwindingPolicyDispatchInvariant =>
      forall value, exists trace,
        dispatch_named_policy value = PolicyReturned trace /\
        trace = phase_a_dispose_value value
  | RawHeaderQuarantineInvariant =>
      forall authority allocation panic subject,
        explicit_raw_drop_is_empty
          (run_quarantined_teardown authority allocation panic subject) /\
        fail_stop_is_terminal
          (run_quarantined_teardown authority allocation panic subject) /\
        no_live_process_quarantine_leak
          (run_quarantined_teardown authority allocation panic subject) /\
        quarantine_recursive_raw_drops
          (run_quarantined_teardown authority allocation panic subject) = 0
  | ExplicitDropAtEmptyInvariant =>
      forall authority allocation panic subject,
        explicit_raw_drop_is_empty
          (run_quarantined_teardown authority allocation panic subject)
  | FailStopNoContinuationInvariant =>
      forall authority allocation panic subject,
        fail_stop_is_terminal
          (run_quarantined_teardown authority allocation panic subject)
  | FailStopNoRecursiveDropInvariant =>
      forall authority allocation panic subject,
        quarantine_recursive_raw_drops
          (run_quarantined_teardown authority allocation panic subject) = 0
  | NoLiveProcessLeakInvariant =>
      forall authority allocation panic subject,
        no_live_process_quarantine_leak
          (run_quarantined_teardown authority allocation panic subject)
  | ParHandoffExactOnceInvariant =>
      forall authority allocation subject,
        quarantine_process
          (run_quarantined_teardown authority allocation false subject) =
          LiveCompleted ->
        queued_par_jobs
          (quarantine_trace
            (run_quarantined_teardown authority allocation false subject)) =
          flat_map par_job (preorder_payloads_tree subject)
  | TerminalAllocationPathExplicitInvariant =>
      forall authority subject,
        quarantine_allocation_attempts
          (run_quarantined_teardown authority
            NoAllocationRequired false subject) = 0 /\
        quarantine_allocation_attempts
          (run_quarantined_teardown authority
            AllocationSucceeded false subject) = 1
  | TerminalAllocationFailStopInvariant =>
      forall authority subject,
        quarantine_process
          (run_quarantined_teardown authority
            AllocationFailed false subject) = ProcessFailStopped /\
        quarantine_live_continuations
          (run_quarantined_teardown authority
            AllocationFailed false subject) = 0
  end.

Theorem every_quarantine_invariant_is_bound : forall invariant,
  quarantine_invariant_binding invariant.
Proof.
  intro invariant. destruct invariant; cbn.
  - exact named_policy_registries_are_scoped_and_complete.
  - apply production_policy_dispatch_never_unwinds.
  - apply every_quarantine_outcome_guards_raw_drop.
  - intros authority allocation panic subject. exact (proj1
      (every_quarantine_outcome_guards_raw_drop
        authority allocation panic subject)).
  - intros authority allocation panic subject. exact (proj1 (proj2
      (every_quarantine_outcome_guards_raw_drop
        authority allocation panic subject))).
  - intros authority allocation panic subject. exact (proj2 (proj2 (proj2
      (every_quarantine_outcome_guards_raw_drop
        authority allocation panic subject)))).
  - intros authority allocation panic subject. exact (proj1 (proj2 (proj2
      (every_quarantine_outcome_guards_raw_drop
        authority allocation panic subject)))).
  - apply successful_quarantine_preserves_exact_par_handoff.
  - intros authority subject. pose proof
      (terminal_teardown_reports_actual_allocation_path authority subject)
      as [_ [no_allocation [_ allocated]]]. split; assumption.
  - intros authority subject. pose proof
      (terminal_allocation_failure_is_terminal authority subject)
      as [failed [_ [no_continuation no_recursive]]]. split; assumption.
Qed.

Theorem quarantine_invariant_catalog_is_duplicate_free_and_exact :
  NoDup quarantine_invariant_catalog /\
  length quarantine_invariant_catalog = 10.
Proof. split; [repeat constructor; simpl; intuition discriminate|reflexivity]. Qed.

Theorem quarantine_invariant_catalog_is_unique_complete_and_exact :
  NoDup quarantine_invariant_catalog /\
  Forall quarantine_invariant_binding quarantine_invariant_catalog /\
  length quarantine_invariant_catalog = 10.
Proof.
  split.
  - exact (proj1 quarantine_invariant_catalog_is_duplicate_free_and_exact).
  - split.
    + apply Forall_forall. intros invariant _.
      apply every_quarantine_invariant_is_bound.
    + exact (proj2 quarantine_invariant_catalog_is_duplicate_free_and_exact).
Qed.

Theorem quarantine_failure_strengthening_capstone : forall authority subject,
  (forall allocation unexpected_panic,
    explicit_raw_drop_is_empty
      (run_quarantined_teardown authority allocation unexpected_panic subject) /\
    fail_stop_is_terminal
      (run_quarantined_teardown authority allocation unexpected_panic subject) /\
    no_live_process_quarantine_leak
      (run_quarantined_teardown authority allocation unexpected_panic subject) /\
    quarantine_recursive_raw_drops
      (run_quarantined_teardown authority allocation unexpected_panic subject) = 0) /\
  queued_par_jobs
    (quarantine_trace
      (run_quarantined_teardown authority
        NoAllocationRequired false subject)) =
    flat_map par_job (preorder_payloads_tree subject) /\
  quarantine_allocation_attempts
    (run_quarantined_teardown authority
      NoAllocationRequired false subject) = 0 /\
  quarantine_process
    (run_quarantined_teardown authority
      AllocationSucceeded false subject) = LiveCompleted /\
  quarantine_allocation_attempts
    (run_quarantined_teardown authority
      AllocationSucceeded false subject) = 1 /\
  quarantine_process
    (run_quarantined_teardown authority
      AllocationFailed false subject) = ProcessFailStopped.
Proof.
  intros authority subject. split.
  - intros allocation unexpected_panic.
    exact (every_quarantine_outcome_guards_raw_drop
      authority allocation unexpected_panic subject).
  - split.
    + exact (proj1 (proj2 (proj2
        (terminal_two_phase_trace_is_exact_and_topology_complete subject)))).
    + repeat split; reflexivity.
Qed.

Inductive invariant_id : Type :=
| AnchorArcAuthorityInvariant
| PhaseAPreorderInvariant
| RootPayloadInvariant
| PhaseAExactOnceInvariant
| PhaseAImmediateDisposalInvariant
| PhaseACloseZeroValidationInvariant
| PhaseANoTopologyCloseOrderInvariant
| PhaseAConstantControlNoDepthCapInvariant
| PhaseBValueFreeProvenanceInvariant
| BarrierNonOverwriteInvariant
| ImmediateParentBarrierInvariant
| OneEdgePruneInvariant
| TopologyProgressInvariant
| NoRecursiveRawDropInvariant
| ImmutableCloneHeaderInvariant
| FinalArcElectionInvariant
| MakeMutFreshAnchorInvariant
| MakeMutAbortRegistrationInvariant
| TerminalStorageSafetyEvidenceInvariant
| ParPdaTransferInvariant
| WholePrivateRootExitInvariant
| PrivatePublicationInvariant
| FailurePrecedenceInvariant
| TerminalAllocationOutcomeInvariant
| CanonicalEmptyFinalAnchorFastPathInvariant.

Definition invariant_catalog : list invariant_id :=
  [AnchorArcAuthorityInvariant; PhaseAPreorderInvariant;
   RootPayloadInvariant; PhaseAExactOnceInvariant;
   PhaseAImmediateDisposalInvariant; PhaseACloseZeroValidationInvariant;
   PhaseANoTopologyCloseOrderInvariant;
   PhaseAConstantControlNoDepthCapInvariant;
   PhaseBValueFreeProvenanceInvariant;
   BarrierNonOverwriteInvariant; ImmediateParentBarrierInvariant;
   OneEdgePruneInvariant; TopologyProgressInvariant;
   NoRecursiveRawDropInvariant; ImmutableCloneHeaderInvariant;
   FinalArcElectionInvariant; MakeMutFreshAnchorInvariant;
   MakeMutAbortRegistrationInvariant;
   TerminalStorageSafetyEvidenceInvariant; ParPdaTransferInvariant;
   WholePrivateRootExitInvariant; PrivatePublicationInvariant;
   FailurePrecedenceInvariant; TerminalAllocationOutcomeInvariant;
   CanonicalEmptyFinalAnchorFastPathInvariant].

Definition invariant_binding (invariant : invariant_id) : Prop :=
  match invariant with
  | AnchorArcAuthorityInvariant => anchor_well_formed (live_anchor 1)
  | PhaseAPreorderInvariant => forall subject,
      removed_without_prune (phase_a_value_trace subject) =
      preorder_payloads_tree subject
  | RootPayloadInvariant => forall cell children,
      preorder_payloads_tree (Node cell children) =
      option_list cell ++ preorder_payloads_forest children
  | PhaseAExactOnceInvariant => forall subject,
      removed_without_prune (phase_a_value_trace subject) =
        preorder_payloads_tree subject /\
      disposed_values (phase_a_value_trace subject) =
        preorder_payloads_tree subject
  | PhaseAImmediateDisposalInvariant => forall value,
      firstn 2 (phase_a_dispose_value value) =
      [RemoveValueNoPrune value; DisposeValue value]
  | PhaseACloseZeroValidationInvariant => forall subject,
      (exists prefix,
        phase_a_value_trace subject =
          prefix ++ [CloseValueCursor; ValidateValueFreeRoot]) /\
      logical_value_count (phase_a_value_free_root (run_phase_a subject)) = 0 /\
      logical_iteration_count
        (phase_a_value_free_root (run_phase_a subject)) = 0
  | PhaseANoTopologyCloseOrderInvariant => forall subject,
      prune_widths (phase_a_value_trace subject) = [] /\
      exists prefix,
        phase_a_value_trace subject =
          prefix ++ [CloseValueCursor; ValidateValueFreeRoot]
  | PhaseAConstantControlNoDepthCapInvariant =>
      phase_a_write_cursors iterative_phase_a_resources = 1 /\
      phase_a_retained_values iterative_phase_a_resources = 1 /\
      phase_a_native_stack_growth iterative_phase_a_resources = 0 /\
      phase_a_depth_cap iterative_phase_a_resources = None
  | PhaseBValueFreeProvenanceInvariant => forall subject,
      removed_without_prune
        (run_phase_b (run_phase_a subject)
          (proj1 erase_values_produces_a_value_free_root subject)) = [] /\
      disposed_values
        (run_phase_b (run_phase_a subject)
          (proj1 erase_values_produces_a_value_free_root subject)) = []
  | BarrierNonOverwriteInvariant => ~ In InstallBarrier (prune_segment true)
  | ImmediateParentBarrierInvariant => forall parent_has_value,
      branch_safe_segment parent_has_value
        (prune_segment parent_has_value)
  | OneEdgePruneInvariant => forall parent_has_value,
      prune_widths (prune_segment parent_has_value) = [1]
  | TopologyProgressInvariant => forall edges,
      0 < edges -> edges - 1 < edges
  | NoRecursiveRawDropInvariant => forall authority subject,
      recursively_dropped_unique_edges
        (release_terminal_root authority subject) = 0
  | ImmutableCloneHeaderInvariant => forall existing_others,
      anchor_raw_headers
        (anchor_transition_final
          (immutable_wrapper_clone existing_others)) = 1
  | FinalArcElectionInvariant =>
      anchor_transition_trace final_wrapper_drop =
        [ArcElectsFinalWrapper; BeginTerminalTwoPhasePda]
  | MakeMutFreshAnchorInvariant => forall remaining_others,
      anchor_well_formed
        (make_mut_fresh_final (shared_arc_make_mut remaining_others))
  | MakeMutAbortRegistrationInvariant => forall existing_others,
      anchor_transition_final (aborted_arc_make_mut existing_others) =
        anchor_transition_initial (aborted_arc_make_mut existing_others) /\
      anchor_well_formed
        (anchor_transition_final (aborted_arc_make_mut existing_others))
  | TerminalStorageSafetyEvidenceInvariant => forall authority,
      exclusive_terminal_footprint
        (terminal_owned_region authority)
        (terminal_external_regions authority) \/
      terminal_verified_public_drain authority = true
  | ParPdaTransferInvariant => forall subject,
      queued_par_jobs (terminal_two_phase_trace subject) =
      flat_map par_job (preorder_payloads_tree subject)
  | WholePrivateRootExitInvariant => forall exit,
      In exit
        [PrivateWriterEarlyDrop; PrivateIteratorEarlyDrop;
         MalformedConstructionDiscard; FailedFreshCandidate;
         RetiredWholeAuthorityAfterPublication; FinalAnchorRootTeardown]
  | PrivatePublicationInvariant => forall authority complete failure,
      publication_count
        (finalize_private_builder authority AcceptComplete complete) = 1 /\
      publication_count
        (finalize_private_builder authority (RejectPrivate failure) complete) = 0
  | FailurePrecedenceInvariant =>
      select_failure false true true true true = Some ChildPdaFailure
  | TerminalAllocationOutcomeInvariant => forall authority subject,
      quarantine_allocation_attempts
        (run_quarantined_teardown authority
          NoAllocationRequired false subject) = 0 /\
      quarantine_allocation_attempts
        (run_quarantined_teardown authority
          AllocationSucceeded false subject) = 1 /\
      quarantine_process
        (run_quarantined_teardown authority
          AllocationFailed false subject) = ProcessFailStopped
  | CanonicalEmptyFinalAnchorFastPathInvariant => forall authority,
      release_trace
        (release_final_anchor authority canonical_empty_root) = [] /\
      removed_without_prune
        (release_trace
          (release_final_anchor authority canonical_empty_root)) = [] /\
      disposed_values
        (release_trace
          (release_final_anchor authority canonical_empty_root)) = [] /\
      prune_widths
        (release_trace
          (release_final_anchor authority canonical_empty_root)) = [] /\
      queued_par_jobs
        (release_trace
          (release_final_anchor authority canonical_empty_root)) = []
  end.

Theorem every_primary_invariant_is_bound : forall invariant,
  invariant_binding invariant.
Proof.
  intro invariant. destruct invariant; cbn.
  - split; reflexivity.
  - intro subject. exact (proj1
      (phase_a_extracts_and_immediately_disposes_every_real_value_once subject)).
  - reflexivity.
  - intro subject. pose proof
      (phase_a_extracts_and_immediately_disposes_every_real_value_once subject)
      as [removed [disposed rest]]. split; assumption.
  - apply phase_a_dispose_value_is_immediate.
  - intro subject. split.
    + exact (proj2
        (phase_a_changes_no_topology_and_closes_before_validation subject)).
    + apply phase_a_zero_validation_covers_metadata_and_iteration.
  - apply phase_a_changes_no_topology_and_closes_before_validation.
  - apply phase_a_has_constant_control_and_no_artificial_depth_limit.
  - intro subject. pose proof
      (phase_b_requires_zero_values_and_only_prunes_topology
        (run_phase_a subject)
        (proj1 erase_values_produces_a_value_free_root subject))
      as [_ [removed disposed]]. split; assumption.
  - apply barrier_never_overwrites_a_real_parent_value.
  - apply immediate_parent_barrier_protocol.
  - apply prune_segment_is_one_logical_edge.
  - apply one_edge_prune_strictly_decreases_remaining_topology.
  - intros authority subject. exact (proj2
      (terminal_release_never_uses_recursive_nonempty_pathmap_drop
        authority subject)).
  - intro existing_others. exact (proj1
      (immutable_wrapper_clone_does_not_clone_raw_header_or_root_value
        existing_others)).
  - exact (proj1 (proj2 two_wrapper_arc_sequence_elects_exactly_one_final_drop)).
  - intro remaining_others. exact (proj1 (proj2
      (make_mut_creates_one_well_formed_fresh_anchor remaining_others))).
  - apply make_mut_abort_leaves_no_unregistered_raw_header.
  - apply terminal_authority_exposes_storage_safety.
  - intro subject. exact (proj1 (proj2 (proj2
      (terminal_two_phase_trace_is_exact_and_topology_complete subject)))).
  - apply terminal_exit_is_always_a_whole_private_root.
  - intros authority complete failure. split; reflexivity.
  - apply child_failure_precedes_deferred_act_failure.
  - intros authority subject. pose proof
      (terminal_teardown_reports_actual_allocation_path authority subject)
      as [_ [no_allocation [_ allocated]]].
    pose proof (terminal_allocation_failure_is_terminal authority subject)
      as [failed rest]. repeat split; assumption.
  - apply canonical_empty_final_anchor_is_an_ordinary_work_free_drop.
Qed.

Theorem invariant_catalog_is_duplicate_free : NoDup invariant_catalog.
Proof. repeat constructor; simpl; intuition discriminate. Qed.

Theorem invariant_catalog_has_exactly_twenty_five_entries :
  length invariant_catalog = 25.
Proof. reflexivity. Qed.

Theorem invariant_catalog_is_unique_complete_and_exact :
  NoDup invariant_catalog /\
  Forall invariant_binding invariant_catalog /\
  length invariant_catalog = 25.
Proof.
  split.
  - apply invariant_catalog_is_duplicate_free.
  - split.
    + apply Forall_forall. intros invariant _.
      apply every_primary_invariant_is_bound.
    + apply invariant_catalog_has_exactly_twenty_five_entries.
Qed.

Theorem campaign_branch_safe_pathmap_teardown_capstone :
  forall authority subject,
  removed_without_prune (terminal_two_phase_trace subject) =
    preorder_payloads_tree subject /\
  disposed_values (terminal_two_phase_trace subject) =
    preorder_payloads_tree subject /\
  queued_par_jobs (terminal_two_phase_trace subject) =
    flat_map par_job (preorder_payloads_tree subject) /\
  detached_payloads
    (run_phase_b (run_phase_a subject)
      (proj1 erase_values_produces_a_value_free_root subject)) = [] /\
  prune_widths (terminal_two_phase_trace subject) =
    repeat 1 (tree_edges subject) /\
  value_free (phase_a_value_free_root (run_phase_a subject)) /\
  residual_campaign_edges
    (release_terminal_root authority subject) = 0 /\
  recursively_dropped_unique_edges
    (release_terminal_root authority subject) = 0 /\
  (forall sibling,
    preserved_sibling
      (release_cross_anchor_shared_image authority subject sibling) =
      Some sibling) /\
  (forall failure,
    returned_failure
      (finalize_private_builder authority (RejectPrivate failure) subject) =
      Some failure /\
    publication_count
      (finalize_private_builder authority (RejectPrivate failure) subject) = 0) /\
  residual_campaign_edges
    (close_campaign_owned_exit authority subject) = 0 /\
    recursively_dropped_unique_edges
      (close_campaign_owned_exit authority subject) = 0 /\
  anchor_transition_final (nonlast_wrapper_drop 0) =
    anchor_transition_initial final_wrapper_drop /\
  anchor_transition_trace final_wrapper_drop =
    [ArcElectsFinalWrapper; BeginTerminalTwoPhasePda] /\
  anchor_raw_headers
    (anchor_transition_final (immutable_wrapper_clone 0)) = 1 /\
  anchor_root_payload_copies
    (anchor_transition_final (immutable_wrapper_clone 0)) = 1 /\
  anchor_raw_headers
    (make_mut_fresh_final (shared_arc_make_mut 0)) = 1 /\
  publication_count
    (finalize_private_builder authority AcceptComplete subject) = 1.
Proof.
  intros authority subject.
  pose proof (terminal_two_phase_trace_is_exact_and_topology_complete subject)
    as [removed [disposed [jobs [phase_b_empty prunes]]]].
  pose proof (proj2 (proj2 (phase_a_closes_with_a_value_free_root subject)))
    as zero_values.
  repeat split; try assumption; try reflexivity.
  all: intros; reflexivity.
Qed.

Print Assumptions drain_detaches_originals_exactly_once_in_reverse_postorder.
Print Assumptions drain_prunes_every_logical_edge_once.
Print Assumptions drain_transfers_every_par_to_one_explicit_pda_job.
Print Assumptions topology_adversaries_are_drained_edge_by_edge.
Print Assumptions immutable_wrapper_clone_does_not_clone_raw_header_or_root_value.
Print Assumptions two_wrapper_arc_sequence_elects_exactly_one_final_drop.
Print Assumptions make_mut_creates_one_well_formed_fresh_anchor.
Print Assumptions make_mut_abort_leaves_no_unregistered_raw_header.
Print Assumptions terminal_release_never_uses_recursive_nonempty_pathmap_drop.
Print Assumptions canonical_empty_final_anchor_is_an_ordinary_work_free_drop.
Print Assumptions cross_anchor_teardown_preserves_the_sibling_image.
Print Assumptions terminal_region_release_preserves_disjoint_sibling.
Print Assumptions final_anchor_drop_is_the_only_raw_header_teardown.
Print Assumptions every_campaign_owned_exit_drains_before_release.
Print Assumptions terminal_exit_is_always_a_whole_private_root.
Print Assumptions malformed_or_rejected_partial_build_is_fully_drained.
Print Assumptions complete_builder_publishes_exactly_once.
Print Assumptions production_policy_catalog_is_unique_complete_and_exact.
Print Assumptions named_policy_registries_are_scoped_and_complete.
Print Assumptions quarantine_invariant_catalog_is_unique_complete_and_exact.
Print Assumptions invariant_catalog_has_exactly_twenty_five_entries.
Print Assumptions invariant_catalog_is_unique_complete_and_exact.
Print Assumptions campaign_branch_safe_pathmap_teardown_capstone.

End PathMapReversePruneTeardown.
