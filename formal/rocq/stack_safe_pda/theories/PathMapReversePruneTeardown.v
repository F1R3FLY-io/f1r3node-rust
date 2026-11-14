From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import PathMapPhysicalOwnership.

Import ListNotations.
Set Implicit Arguments.

(**
  Formal-first contract for campaign-owned PathMap destruction.

  The trie-level model is deliberately stated over logical byte-trie edges, not
  over the private node representation of the PathMap crate.  A production
  refinement
  refines to two explicit passes over one whole private root.  Phase A uses one
  public owned write zipper in logical-byte preorder, removes every real value
  without logical pruning, and immediately transfers it to its named disposer.
  Only after closing the cursor and validating a zero-value root may Phase B
  remove topology in reverse postorder, one logical edge at a time, without
  writing any value: below a value-free parent that has one child and is not
  the zipper root it cuts the child's branch, and at the root, at a valued
  parent or at a fork it prunes one byte.  The live removal path, which removes
  one valued leaf from a live trie, keeps its synthetic barriers.

  This separates the guarantee that campaign-owned code can establish from an
  assertion about private upstream allocation internals: no nonempty private
  trie is handed to ordinary recursive Drop, while every topological
  detachment made by the integration is bounded to one logical edge.  Final
  wrapper election alone never establishes internal-node uniqueness; terminal
  teardown additionally requires an exclusive physical footprint or a
  verified public-drain refinement.

  The last module, [BarrierFreeNodeRefinement], is the one place that speaks
  about the private node representation.  It models the line-list and dense
  nodes that hold the write zipper's focus as far as the two calls of the
  Phase B step read them, states the premises of the remedy study as one
  predicate on those nodes, and proves under that predicate that each cut
  removes one path byte and detaches a part whose destructors nest at most
  two deep.  The premises are hypotheses of its theorems; nothing about the
  PathMap crate is assumed globally.
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

(** The per-edge step of the live removal path, which removes one valued leaf
    from a live trie and keeps its barriers.  A real parent value is already a
    one-edge pruning barrier.  An absent parent receives a distinct temporary
    barrier immediately before pruning; the original value is never
    overwritten.  Phase B of the final teardown used this step before the
    barrier-free remedy; it now uses [barrier_free_segment] below. *)
Definition prune_segment (parent_has_value : bool) : list teardown_event :=
  if parent_has_value
  then [PruneSpan 1]
  else [InstallBarrier; PruneSpan 1; DetachBarrier].

(** Barrier-free Phase B (Algorithm 2 of the remedy study).  Reverse postorder
    removes the edge to a child after every subtree to its right and below it,
    so the parent then holds exactly the children to the child's left (earlier
    in the forest, which is byte order) and the child itself.  The parent
    stops a run when it is the zipper root, holds a value, or still holds a
    left sibling of the child (a fork). *)
Definition holds_payload (cell : option payload) : bool :=
  match cell with Some _ => true | None => false end.

(** [frame_ascent] counts the logical edges that PathMap's own [prune_path]
    would remove from the parent's own edge upwards: it climbs through
    value-free single-child ancestors until the zipper root, a value or a
    fork.  It only states what a prune at a non-stopping parent would detach;
    the barrier-free step never prunes there. *)
Record parent_frame : Type := ParentFrame {
  frame_is_root : bool;
  frame_cell : option payload;
  frame_ascent : nat
}.

Record step_view : Type := StepView {
  view_parent : parent_frame;
  view_left_children : forest
}.

Definition parent_stops_the_run (view : step_view) : bool :=
  frame_is_root (view_parent view) ||
  holds_payload (frame_cell (view_parent view)) ||
  match view_left_children view with FNil => false | FCons _ _ => true end.

Inductive phase_b_step_kind : Type :=
| BranchCut
| OneBytePrune.

Definition barrier_free_step_kind (view : step_view) : phase_b_step_kind :=
  if parent_stops_the_run view then OneBytePrune else BranchCut.

(** A branch cut is [remove_branches(false)] at the parent: it detaches the
    child's edge together with every left sibling subtree still held by the
    parent.  A prune is [prune_ascend] from the child: it detaches one edge
    below a stopping parent and climbs otherwise. *)
Definition step_detached_edges
    (kind : phase_b_step_kind) (view : step_view) : nat :=
  match kind with
  | BranchCut => S (forest_edges (view_left_children view))
  | OneBytePrune =>
      if parent_stops_the_run view
      then 1
      else S (frame_ascent (view_parent view))
  end.

Definition upward_prune_reach (view : step_view) : nat :=
  step_detached_edges OneBytePrune view.

(** The observer reports one prune per step, whose width is the number of
    logical edges the step detached; Phase B emits no barrier event. *)
Definition barrier_free_segment (view : step_view) : list teardown_event :=
  [PruneSpan (step_detached_edges (barrier_free_step_kind view) view)].

Fixpoint drain_node (is_root : bool) (ascent : nat) (subject : trie)
    {struct subject} : list teardown_event :=
  match subject with
  | Node cell children =>
      drain_forest children (ParentFrame is_root cell ascent) FNil ++
      dispose_cell cell
  end
with drain_forest
    (subjects : forest) (parent : parent_frame) (left : forest)
    {struct subjects} : list teardown_event :=
  match subjects with
  | FNil => []
  | FCons subject rest =>
      drain_forest rest parent (FCons subject left) ++
      drain_node false (upward_prune_reach (StepView parent left)) subject ++
      barrier_free_segment (StepView parent left)
  end.

Definition drain_tree (subject : trie) : list teardown_event :=
  drain_node true 0 subject.

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

(** Every barrier-free Phase B step detaches exactly one logical edge: a cut
    is chosen only below a parent without left siblings, and a prune only
    below a stopping parent. *)
Theorem every_barrier_free_step_detaches_exactly_one_logical_edge :
  forall view,
    step_detached_edges (barrier_free_step_kind view) view = 1.
Proof.
  intros [[is_root cell ascent] left].
  unfold barrier_free_step_kind, step_detached_edges, parent_stops_the_run.
  cbn [view_parent view_left_children frame_is_root frame_cell frame_ascent].
  destruct is_root, (holds_payload cell), left; reflexivity.
Qed.

(** A non-stopping step is a branch cut below a value-free parent that is not
    the zipper root and has no other child; a stopping step is a one-byte
    prune below the root, a valued parent or a fork. *)
Theorem barrier_free_step_classification : forall view,
  (barrier_free_step_kind view = BranchCut <->
     frame_is_root (view_parent view) = false /\
     frame_cell (view_parent view) = None /\
     view_left_children view = FNil) /\
  (barrier_free_step_kind view = OneBytePrune <->
     frame_is_root (view_parent view) = true \/
     (exists value, frame_cell (view_parent view) = Some value) \/
     (exists sibling rest, view_left_children view = FCons sibling rest)).
Proof.
  intros [[is_root cell ascent] left].
  unfold barrier_free_step_kind, parent_stops_the_run, holds_payload.
  cbn [view_parent view_left_children frame_is_root frame_cell frame_ascent].
  destruct is_root, cell as [value|], left as [|sibling rest]; simpl;
    split; split; intro claim;
    repeat match goal with
           | found : _ /\ _ |- _ => destruct found
           | found : _ \/ _ |- _ => destruct found
           | found : exists _, _ |- _ => destruct found
           end;
    try discriminate; try reflexivity;
    try (repeat split; reflexivity);
    try (left; reflexivity);
    try (right; left; eexists; reflexivity);
    try (right; right; do 2 eexists; reflexivity).
Qed.

Lemma barrier_free_segment_is_one_logical_edge : forall view,
  barrier_free_segment view = [PruneSpan 1].
Proof.
  intro view. unfold barrier_free_segment.
  now rewrite every_barrier_free_step_detaches_exactly_one_logical_edge.
Qed.

Lemma barrier_free_segment_prunes_one_logical_edge : forall view,
  prune_widths (barrier_free_segment view) = [1].
Proof. intro view. now rewrite barrier_free_segment_is_one_logical_edge. Qed.

Lemma barrier_free_segment_detaches_no_original : forall view,
  detached_payloads (barrier_free_segment view) = [].
Proof. intro view. now rewrite barrier_free_segment_is_one_logical_edge. Qed.

Lemma barrier_free_segment_queues_no_par_job : forall view,
  queued_par_jobs (barrier_free_segment view) = [].
Proof. intro view. now rewrite barrier_free_segment_is_one_logical_edge. Qed.

Theorem barrier_free_segment_installs_no_barrier : forall view,
  ~ In InstallBarrier (barrier_free_segment view) /\
  ~ In DetachBarrier (barrier_free_segment view).
Proof.
  intro view. rewrite barrier_free_segment_is_one_logical_edge.
  simpl. intuition discriminate.
Qed.

Lemma drain_node_detaches_originals_in_reverse_postorder :
  (forall subject is_root ascent,
      detached_payloads (drain_node is_root ascent subject) =
      reverse_payloads_tree subject) /\
  (forall subjects parent left,
      detached_payloads (drain_forest subjects parent left) =
      reverse_payloads_forest subjects).
Proof.
  apply trie_forest_induction.
  - intros cell children children_equivalence is_root ascent.
    cbn [drain_node reverse_payloads_tree].
    rewrite detached_payloads_app, children_equivalence.
    rewrite dispose_cell_detaches_exactly. reflexivity.
  - intros parent left. reflexivity.
  - intros subject subject_equivalence rest rest_equivalence parent left.
    cbn [drain_forest reverse_payloads_forest].
    repeat rewrite detached_payloads_app.
    rewrite rest_equivalence, subject_equivalence.
    rewrite barrier_free_segment_detaches_no_original, app_nil_r. reflexivity.
Qed.

Theorem drain_detaches_originals_exactly_once_in_reverse_postorder :
  (forall subject,
      detached_payloads (drain_tree subject) =
      reverse_payloads_tree subject) /\
  (forall subjects parent left,
      detached_payloads (drain_forest subjects parent left) =
      reverse_payloads_forest subjects).
Proof.
  split.
  - intro subject. unfold drain_tree.
    apply (proj1 drain_node_detaches_originals_in_reverse_postorder).
  - exact (proj2 drain_node_detaches_originals_in_reverse_postorder).
Qed.

Lemma drain_node_prunes_every_logical_edge_once :
  (forall subject is_root ascent,
      prune_widths (drain_node is_root ascent subject) =
      repeat 1 (tree_edges subject)) /\
  (forall subjects parent left,
      prune_widths (drain_forest subjects parent left) =
      repeat 1 (forest_edges subjects)).
Proof.
  apply trie_forest_induction.
  - intros cell children children_equivalence is_root ascent.
    cbn [drain_node tree_edges].
    rewrite prune_widths_app, children_equivalence.
    rewrite dispose_cell_has_no_prune, app_nil_r. reflexivity.
  - intros parent left. reflexivity.
  - intros subject subject_equivalence rest rest_equivalence parent left.
    cbn [drain_forest forest_edges].
    repeat rewrite prune_widths_app.
    rewrite rest_equivalence, subject_equivalence,
      barrier_free_segment_prunes_one_logical_edge.
    repeat rewrite repeat_add. now rewrite app_assoc.
Qed.

Theorem drain_prunes_every_logical_edge_once :
  (forall subject,
      prune_widths (drain_tree subject) = repeat 1 (tree_edges subject)) /\
  (forall subjects parent left,
      prune_widths (drain_forest subjects parent left) =
      repeat 1 (forest_edges subjects)).
Proof.
  split.
  - intro subject. unfold drain_tree.
    apply (proj1 drain_node_prunes_every_logical_edge_once).
  - exact (proj2 drain_node_prunes_every_logical_edge_once).
Qed.

Lemma drain_node_transfers_every_par_to_one_explicit_pda_job :
  (forall subject is_root ascent,
      queued_par_jobs (drain_node is_root ascent subject) =
      flat_map par_job (reverse_payloads_tree subject)) /\
  (forall subjects parent left,
      queued_par_jobs (drain_forest subjects parent left) =
      flat_map par_job (reverse_payloads_forest subjects)).
Proof.
  apply trie_forest_induction.
  - intros cell children children_equivalence is_root ascent.
    cbn [drain_node reverse_payloads_tree].
    rewrite queued_par_jobs_app, children_equivalence.
    rewrite dispose_cell_queues_exact_par_job, flat_map_app. reflexivity.
  - intros parent left. reflexivity.
  - intros subject subject_equivalence rest rest_equivalence parent left.
    cbn [drain_forest reverse_payloads_forest].
    repeat rewrite queued_par_jobs_app.
    rewrite rest_equivalence, subject_equivalence.
    rewrite barrier_free_segment_queues_no_par_job, app_nil_r.
    rewrite flat_map_app. reflexivity.
Qed.

Theorem drain_transfers_every_par_to_one_explicit_pda_job :
  (forall subject,
      queued_par_jobs (drain_tree subject) =
      flat_map par_job (reverse_payloads_tree subject)) /\
  (forall subjects parent left,
      queued_par_jobs (drain_forest subjects parent left) =
      flat_map par_job (reverse_payloads_forest subjects)).
Proof.
  split.
  - intro subject. unfold drain_tree.
    apply (proj1 drain_node_transfers_every_par_to_one_explicit_pda_job).
  - exact (proj2 drain_node_transfers_every_par_to_one_explicit_pda_job).
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

Lemma barrier_free_segment_has_no_phase_a_value_event : forall view,
  removed_without_prune (barrier_free_segment view) = [] /\
  disposed_values (barrier_free_segment view) = [].
Proof.
  intro view. rewrite barrier_free_segment_is_one_logical_edge. simpl. tauto.
Qed.

Lemma drain_node_contains_no_real_value_operation :
  (forall subject is_root ascent,
      removed_without_prune (drain_node is_root ascent subject) = [] /\
      disposed_values (drain_node is_root ascent subject) = []) /\
  (forall subjects parent left,
      removed_without_prune (drain_forest subjects parent left) = [] /\
      disposed_values (drain_forest subjects parent left) = []).
Proof.
  apply trie_forest_induction.
  - intros cell children children_empty is_root ascent.
    specialize (children_empty (ParentFrame is_root cell ascent) FNil).
    destruct children_empty as [children_removed children_disposed].
    cbn [drain_node].
    repeat rewrite removed_without_prune_app.
    repeat rewrite disposed_values_app.
    destruct (dispose_cell_has_no_phase_a_value_event cell)
      as [cell_removed cell_disposed].
    rewrite children_removed, children_disposed, cell_removed, cell_disposed.
    tauto.
  - intros parent left. simpl. tauto.
  - intros subject subject_empty rest rest_empty parent left.
    specialize (rest_empty parent (FCons subject left)).
    specialize (subject_empty false (upward_prune_reach (StepView parent left))).
    destruct rest_empty as [rest_removed rest_disposed].
    destruct subject_empty as [subject_removed subject_disposed].
    cbn [drain_forest].
    repeat rewrite removed_without_prune_app.
    repeat rewrite disposed_values_app.
    destruct (barrier_free_segment_has_no_phase_a_value_event (StepView parent left))
      as [segment_removed segment_disposed].
    rewrite rest_removed, rest_disposed, subject_removed, subject_disposed,
      segment_removed, segment_disposed. tauto.
Qed.

Theorem topology_phase_contains_no_real_value_operation :
  (forall subject,
      removed_without_prune (drain_tree subject) = [] /\
      disposed_values (drain_tree subject) = []) /\
  (forall subjects parent left,
      removed_without_prune (drain_forest subjects parent left) = [] /\
      disposed_values (drain_forest subjects parent left) = []).
Proof.
  split.
  - intro subject. unfold drain_tree.
    apply (proj1 drain_node_contains_no_real_value_operation).
  - exact (proj2 drain_node_contains_no_real_value_operation).
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

Lemma value_free_drain_is_one_prune_event_per_logical_edge :
  (forall subject is_root ascent,
      preorder_payloads_tree subject = [] ->
      drain_node is_root ascent subject =
        repeat (PruneSpan 1) (tree_edges subject)) /\
  (forall subjects parent left,
      preorder_payloads_forest subjects = [] ->
      drain_forest subjects parent left =
        repeat (PruneSpan 1) (forest_edges subjects)).
Proof.
  apply trie_forest_induction.
  - intros cell children children_drain is_root ascent no_values.
    destruct cell as [value|]; [simpl in no_values; discriminate no_values|].
    cbn [drain_node tree_edges dispose_cell].
    cbn [preorder_payloads_tree option_list app] in no_values.
    rewrite (children_drain _ _ no_values). apply app_nil_r.
  - intros parent left _. reflexivity.
  - intros subject subject_drain rest rest_drain parent left no_values.
    cbn [preorder_payloads_forest] in no_values.
    apply app_eq_nil in no_values as [subject_free rest_free].
    cbn [drain_forest forest_edges].
    rewrite (rest_drain _ _ rest_free), (subject_drain _ _ subject_free),
      barrier_free_segment_is_one_logical_edge.
    repeat rewrite repeat_add. simpl. now rewrite app_assoc.
Qed.

(** Phase B of a value-free root of [T] logical edges is the topology marker
    followed by exactly [T] one-edge prunes, in the drain's order, and nothing
    else: no barrier, no value event, no detached original. *)
Theorem phase_b_emits_one_prune_event_per_logical_edge_and_nothing_else :
  forall phase_a zero_values,
    run_phase_b phase_a zero_values =
      BeginTopologyPhase ::
        repeat (PruneSpan 1) (tree_edges (phase_a_value_free_root phase_a)).
Proof.
  intros phase_a zero_values. unfold run_phase_b, drain_tree.
  rewrite (proj1 value_free_drain_is_one_prune_event_per_logical_edge
    _ true 0 zero_values).
  reflexivity.
Qed.

Theorem phase_b_events_are_the_begin_marker_and_one_edge_prunes :
  forall phase_a zero_values event,
    In event (run_phase_b phase_a zero_values) ->
    event = BeginTopologyPhase \/ event = PruneSpan 1.
Proof.
  intros phase_a zero_values event member.
  rewrite phase_b_emits_one_prune_event_per_logical_edge_and_nothing_else
    in member.
  destruct member as [is_begin|is_prune].
  - left. symmetry. exact is_begin.
  - right. apply repeat_spec in is_prune. exact is_prune.
Qed.

Theorem phase_b_writes_no_value : forall phase_a zero_values,
  ~ In InstallBarrier (run_phase_b phase_a zero_values) /\
  ~ In DetachBarrier (run_phase_b phase_a zero_values) /\
  (forall value,
    ~ In (RemoveValueNoPrune value) (run_phase_b phase_a zero_values)) /\
  (forall value,
    ~ In (DisposeValue value) (run_phase_b phase_a zero_values)) /\
  (forall value,
    ~ In (DetachOriginal value) (run_phase_b phase_a zero_values)).
Proof.
  intros phase_a zero_values.
  pose proof (@phase_b_events_are_the_begin_marker_and_one_edge_prunes
    phase_a zero_values) as only.
  split; [|split; [|split; [|split]]];
    [intro member | intro member | intros value member
    | intros value member | intros value member];
    destruct (only _ member) as [same|same]; discriminate same.
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

(** One run of Algorithm 2, that is one call of its inner loop.  It starts at
    a childless leaf and looks at the leaf's parent: a branch cut there
    detaches the edge and leaves the parent as the new childless leaf, so the
    run goes on one level higher; a one-byte prune ends the run.  The frames
    list the ancestors from the leaf's parent up to the zipper root, which is
    the last frame; [run_frame_left] holds the children of that ancestor to
    the left of the path being removed (those to its right are already gone,
    since the run starts at the rightmost leaf). *)
Record run_frame : Type := RunFrame {
  run_frame_cell : option payload;
  run_frame_left : forest
}.

Definition is_last_frame (higher : list run_frame) : bool :=
  match higher with [] => true | _ :: _ => false end.

Fixpoint run_prune_reach (frames : list run_frame) : nat :=
  match frames with
  | [] => 0
  | frame :: higher =>
      upward_prune_reach
        (StepView
          (ParentFrame (is_last_frame higher) (run_frame_cell frame)
            (run_prune_reach higher))
          (run_frame_left frame))
  end.

Definition run_view (frame : run_frame) (higher : list run_frame)
    : step_view :=
  StepView
    (ParentFrame (is_last_frame higher) (run_frame_cell frame)
      (run_prune_reach higher))
    (run_frame_left frame).

Record run_step : Type := RunStep {
  run_step_kind : phase_b_step_kind;
  run_step_edges : nat;
  run_step_dropped_payloads : list payload
}.

(** [current] is the cell of the leaf whose edge the step removes.  A cut
    drops everything below the parent: the leaf's cell and the left subtrees.
    A prune removes a node only when it holds no value (PathMap's dangling
    removal keeps a value), so below a valued leaf it detaches nothing. *)
Fixpoint barrier_free_run_with (choose : step_view -> phase_b_step_kind)
    (current : option payload) (frames : list run_frame) : list run_step :=
  match frames with
  | [] => []
  | frame :: higher =>
      match choose (run_view frame higher) with
      | BranchCut =>
          RunStep BranchCut
            (step_detached_edges BranchCut (run_view frame higher))
            (option_list current ++
              reverse_payloads_forest (run_frame_left frame)) ::
          barrier_free_run_with choose (run_frame_cell frame) higher
      | OneBytePrune =>
          [RunStep OneBytePrune
            (match current with
             | None => step_detached_edges OneBytePrune (run_view frame higher)
             | Some _ => 0
             end) []]
      end
  end.

Definition barrier_free_run
    : option payload -> list run_frame -> list run_step :=
  barrier_free_run_with barrier_free_step_kind.

Definition run_step_is_one_edge_without_payload_loss (step : run_step) : Prop :=
  run_step_edges step = 1 /\ run_step_dropped_payloads step = [].

Definition unary_value_free_frame (frame : run_frame) : Prop :=
  run_frame_cell frame = None /\ run_frame_left frame = FNil.

Lemma barrier_free_run_cut_step : forall frame higher,
  barrier_free_step_kind (run_view frame higher) = BranchCut ->
  barrier_free_run None (frame :: higher) =
    RunStep BranchCut 1 [] :: barrier_free_run None higher.
Proof.
  intros frame higher cut.
  pose proof (proj1 (proj1 (barrier_free_step_classification
    (run_view frame higher))) cut) as [_ [no_value no_left]].
  cbn [run_view view_parent view_left_children frame_cell]
    in no_value, no_left.
  unfold barrier_free_run. cbn [barrier_free_run_with]. rewrite cut.
  unfold step_detached_edges, run_view. cbn [view_left_children].
  rewrite no_left, no_value. reflexivity.
Qed.

Lemma barrier_free_run_prune_step : forall frame higher,
  barrier_free_step_kind (run_view frame higher) = OneBytePrune ->
  barrier_free_run None (frame :: higher) = [RunStep OneBytePrune 1 []].
Proof.
  intros frame higher prune.
  pose proof (every_barrier_free_step_detaches_exactly_one_logical_edge
    (run_view frame higher)) as one_edge.
  rewrite prune in one_edge.
  unfold barrier_free_run. cbn [barrier_free_run_with]. rewrite prune.
  rewrite one_edge. reflexivity.
Qed.

Theorem barrier_free_run_detaches_one_edge_per_step_and_drops_no_payload :
  forall frames,
    Forall run_step_is_one_edge_without_payload_loss
      (barrier_free_run None frames).
Proof.
  induction frames as [|frame higher IH]; [apply Forall_nil|].
  destruct (barrier_free_step_kind (run_view frame higher)) eqn:kind.
  - rewrite (barrier_free_run_cut_step _ _ kind).
    apply Forall_cons; [split; reflexivity|exact IH].
  - rewrite (barrier_free_run_prune_step _ _ kind).
    apply Forall_cons; [split; reflexivity|apply Forall_nil].
Qed.

Theorem every_run_cuts_up_to_its_first_stopping_parent_and_prunes_there :
  forall frames,
    frames <> [] ->
    exists cuts stop higher,
      frames = cuts ++ stop :: higher /\
      Forall unary_value_free_frame cuts /\
      barrier_free_step_kind (run_view stop higher) = OneBytePrune /\
      barrier_free_run None frames =
        repeat (RunStep BranchCut 1 []) (length cuts) ++
        [RunStep OneBytePrune 1 []].
Proof.
  induction frames as [|frame higher IH]; intro nonempty.
  - exfalso. apply nonempty. reflexivity.
  - destruct (barrier_free_step_kind (run_view frame higher)) eqn:kind.
    + pose proof (proj1 (proj1 (barrier_free_step_classification
        (run_view frame higher))) kind) as [not_root [no_value no_left]].
      cbn [run_view view_parent view_left_children frame_is_root frame_cell]
        in not_root, no_value, no_left.
      destruct higher as [|next rest].
      * simpl in not_root. discriminate not_root.
      * assert (rest_nonempty : next :: rest <> []) by discriminate.
        destruct (IH rest_nonempty)
          as [cuts [stop [top [split_frames [unary [stop_kind run]]]]]].
        exists (frame :: cuts), stop, top.
        split; [rewrite split_frames; reflexivity|].
        split; [apply Forall_cons; [split; assumption|exact unary]|].
        split; [exact stop_kind|].
        rewrite (barrier_free_run_cut_step _ _ kind), run. reflexivity.
    + exists [], frame, higher.
      split; [reflexivity|].
      split; [apply Forall_nil|].
      split; [exact kind|].
      exact (barrier_free_run_prune_step _ _ kind).
Qed.

(** The barrier protocol of the live removal path's step [prune_segment].
    The barrier-free Phase B step has no barrier at all
    ([barrier_free_segment_installs_no_barrier]). *)
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

(** Barrier-free counterparts of the three barrier theorems above for Phase B:
    below a value-free parent the step writes nothing and is one prune event of
    width one (a cut when the parent is not the root and has no other child),
    and a real parent value ends the run without being overwritten. *)
Theorem absent_parent_is_cut_or_pruned_without_a_barrier :
  forall is_root ascent left,
    barrier_free_segment (StepView (ParentFrame is_root None ascent) left) =
      [PruneSpan 1] /\
    (is_root = false -> left = FNil ->
      barrier_free_step_kind
        (StepView (ParentFrame is_root None ascent) left) = BranchCut).
Proof.
  intros is_root ascent left. split.
  - apply barrier_free_segment_is_one_logical_edge.
  - intros not_root no_left. subst is_root left. reflexivity.
Qed.

Theorem real_parent_value_ends_the_run_without_a_barrier :
  forall is_root value ascent left,
    barrier_free_step_kind
      (StepView (ParentFrame is_root (Some value) ascent) left) =
      OneBytePrune /\
    barrier_free_segment
      (StepView (ParentFrame is_root (Some value) ascent) left) =
      [PruneSpan 1].
Proof.
  intros is_root value ascent left. split.
  - unfold barrier_free_step_kind, parent_stops_the_run. simpl.
    rewrite orb_true_r. reflexivity.
  - apply barrier_free_segment_is_one_logical_edge.
Qed.

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

(** * The floor-bounded live removal

    The in-place removal of an exhausted endpoint on an exclusive anchor
    calls [NativePathCursor::remove_val_and_prune].  The endpoint's value is
    taken and handed to the named disposer.  An endpoint at the zipper root
    or with children keeps its topology.  A leaf goes through
    [remove_leaf_with_floor] with the cursor's prefix length as the floor,
    and its loop runs once per byte of the dangling chain:

    - ascend one byte to the parent and read three facts about it before
      changing anything: whether it is the floor, whether it holds a real
      value, and whether it has another child (a surviving fork);
    - if it holds no real value, install a synthetic barrier there;
    - descend and remove the child: the real value at the first step (taken
      counted and disposed), the previous step's barrier afterwards;
    - remove exactly one edge ([prune_ascend]);
    - stop at the floor, at a real value or at a fork, and retire the
      barrier installed at the stopping parent; otherwise carry on from the
      parent, whose barrier is now the valued leaf.

    [live_frame] is what one step reads about the parent: its node, its cell
    and whether it forks.  [live_prune_run above_floor frames] lists the
    events of the loop over the frames from the endpoint's parent upwards,
    where [above_floor] is the number of bytes between that parent and the
    floor.  Each step is one [prune_segment]: a real parent value is already
    a one-edge barrier; a value-free parent gets a barrier immediately
    before the prune, and the barrier goes after it.

    Two differences of form, neither of substance.  The model takes the
    counted value as [RemoveValueNoPrune] and [DisposeValue] and counts the
    edge of that first step separately, as [PruneSpan 1]: the code's
    [take_counted_value] with [prune = true] may remove that edge itself, and
    then only ascends; either way one edge goes.  The code retires the
    barrier of a parent that does not stop the run one step later, when it
    removes that parent as the next child, and installs the next barrier
    first; the model retires it in the same segment.  Both orders have the
    same events and counts, and in both every prune is below a real value
    or a barrier.

    The barrier-free cut of the final teardown is not this step and is not
    modeled here: its premises assume a value-free trie that no other handle
    observes, which a live anchor is not. *)
Record live_frame : Type := LiveFrame {
  live_frame_node : PathMapPhysicalOwnership.node_id;
  live_frame_cell : option payload;
  live_frame_forks : bool
}.

(** [above_floor] counts down by one per frame; the frame read at zero is
    the floor. *)
Definition live_at_floor (above_floor : nat) : bool :=
  match above_floor with
  | 0 => true
  | S _ => false
  end.

(** The loop's stopping test, in the code's order: the floor, a real parent
    value, a surviving fork. *)
Definition live_frame_stops (at_floor : bool) (frame : live_frame) : bool :=
  at_floor || holds_payload (live_frame_cell frame) || live_frame_forks frame.

Definition live_frame_segment (frame : live_frame) : list teardown_event :=
  prune_segment (holds_payload (live_frame_cell frame)).

Fixpoint live_prune_run (above_floor : nat) (frames : list live_frame)
    : list teardown_event :=
  match frames with
  | [] => []
  | frame :: higher =>
      live_frame_segment frame ++
      (if live_frame_stops (live_at_floor above_floor) frame then []
       else live_prune_run (pred above_floor) higher)
  end.

Fixpoint live_run_steps (above_floor : nat) (frames : list live_frame)
    : nat :=
  match frames with
  | [] => 0
  | frame :: higher =>
      S (if live_frame_stops (live_at_floor above_floor) frame then 0
         else live_run_steps (pred above_floor) higher)
  end.

(** The nodes that the run frees: the endpoint, then each parent that does
    not stop the run and so becomes the next valued leaf.  The stopping
    parent keeps its node. *)
Fixpoint live_run_released (above_floor : nat)
    (child : PathMapPhysicalOwnership.node_id) (frames : list live_frame)
    : list PathMapPhysicalOwnership.node_id :=
  match frames with
  | [] => []
  | frame :: higher =>
      child ::
      (if live_frame_stops (live_at_floor above_floor) frame then []
       else live_run_released (pred above_floor) (live_frame_node frame)
         higher)
  end.

(** The two cases of [remove_val_and_prune]: an endpoint at the zipper root
    or with children keeps its topology; a leaf runs the loop. *)
Inductive live_endpoint : Type :=
| LiveValueOnlyEndpoint
| LiveLeafEndpoint (above_floor : nat) (frames : list live_frame).

Definition live_endpoint_prune (endpoint : live_endpoint)
    : list teardown_event :=
  match endpoint with
  | LiveValueOnlyEndpoint => []
  | LiveLeafEndpoint above_floor frames => live_prune_run above_floor frames
  end.

Definition live_removal_trace (value : payload) (endpoint : live_endpoint)
    : list teardown_event :=
  phase_a_dispose_value value ++ live_endpoint_prune endpoint.

(** The counters of the code's [BarrierPruneStats]. *)
Fixpoint installed_barrier_count (trace : list teardown_event) : nat :=
  match trace with
  | [] => 0
  | InstallBarrier :: rest => S (installed_barrier_count rest)
  | _ :: rest => installed_barrier_count rest
  end.

Fixpoint retired_barrier_count (trace : list teardown_event) : nat :=
  match trace with
  | [] => 0
  | DetachBarrier :: rest => S (retired_barrier_count rest)
  | _ :: rest => retired_barrier_count rest
  end.

(** A frame that the run passes: no real value and no other child. *)
Definition live_frame_passes (frame : live_frame) : Prop :=
  holds_payload (live_frame_cell frame) = false /\
  live_frame_forks frame = false.

Lemma installed_barrier_count_app : forall left right,
  installed_barrier_count (left ++ right) =
    installed_barrier_count left + installed_barrier_count right.
Proof.
  induction left as [|event rest IH]; intros right; simpl; auto.
  destruct event; simpl; rewrite IH; reflexivity.
Qed.

Lemma retired_barrier_count_app : forall left right,
  retired_barrier_count (left ++ right) =
    retired_barrier_count left + retired_barrier_count right.
Proof.
  induction left as [|event rest IH]; intros right; simpl; auto.
  destruct event; simpl; rewrite IH; reflexivity.
Qed.

Lemma prune_segment_balances_barriers : forall parent_has_value,
  installed_barrier_count (prune_segment parent_has_value) =
    retired_barrier_count (prune_segment parent_has_value).
Proof. destruct parent_has_value; reflexivity. Qed.

Lemma phase_a_dispose_value_has_no_barrier : forall value,
  installed_barrier_count (phase_a_dispose_value value) = 0 /\
  retired_barrier_count (phase_a_dispose_value value) = 0.
Proof.
  destruct value as [member|par_id children|key multiplicity|position];
    simpl; try (split; reflexivity).
  induction children as [|child rest IH]; simpl.
  - split; reflexivity.
  - exact IH.
Qed.

Lemma phase_a_dispose_value_detaches_no_original : forall value,
  detached_payloads (phase_a_dispose_value value) = [].
Proof.
  destruct value as [member|par_id children|key multiplicity|position];
    simpl; try reflexivity.
  apply detached_par_child_events_are_empty.
Qed.

Lemma live_frame_stops_at_the_floor : forall frame,
  live_frame_stops (live_at_floor 0) frame = true.
Proof. intro frame. reflexivity. Qed.

Lemma live_frame_passing_is_above_the_floor : forall above_floor frame,
  live_frame_stops (live_at_floor above_floor) frame = false ->
  exists below, above_floor = S below.
Proof.
  intros [|below] frame passes.
  - rewrite live_frame_stops_at_the_floor in passes. discriminate passes.
  - exists below. reflexivity.
Qed.

Lemma live_frame_stop_reason : forall level frame,
  live_frame_stops (live_at_floor level) frame = true ->
  level = 0 \/ holds_payload (live_frame_cell frame) = true \/
  live_frame_forks frame = true.
Proof.
  intros level frame stops.
  unfold live_frame_stops in stops.
  apply orb_true_iff in stops as [floor_or_value|forks].
  - apply orb_true_iff in floor_or_value as [floor|value].
    + left. destruct level as [|level].
      * reflexivity.
      * simpl in floor. discriminate floor.
    + right. left. exact value.
  - right. right. exact forks.
Qed.

Lemma live_prune_run_stopping_frame : forall above_floor frame higher,
  live_frame_stops (live_at_floor above_floor) frame = true ->
  live_prune_run above_floor (frame :: higher) = live_frame_segment frame.
Proof.
  intros above_floor frame higher stops.
  change (live_frame_segment frame ++
    (if live_frame_stops (live_at_floor above_floor) frame then []
     else live_prune_run (pred above_floor) higher) =
    live_frame_segment frame).
  rewrite stops. apply app_nil_r.
Qed.

Lemma live_prune_run_passing_frame : forall below frame higher,
  live_frame_stops (live_at_floor (S below)) frame = false ->
  live_prune_run (S below) (frame :: higher) =
    live_frame_segment frame ++ live_prune_run below higher.
Proof.
  intros below frame higher passes.
  change (live_frame_segment frame ++
    (if live_frame_stops (live_at_floor (S below)) frame then []
     else live_prune_run (pred (S below)) higher) =
    live_frame_segment frame ++ live_prune_run below higher).
  rewrite passes. reflexivity.
Qed.

Lemma live_run_steps_stopping_frame : forall above_floor frame higher,
  live_frame_stops (live_at_floor above_floor) frame = true ->
  live_run_steps above_floor (frame :: higher) = 1.
Proof.
  intros above_floor frame higher stops.
  change (S (if live_frame_stops (live_at_floor above_floor) frame then 0
    else live_run_steps (pred above_floor) higher) = 1).
  rewrite stops. reflexivity.
Qed.

Lemma live_run_steps_passing_frame : forall below frame higher,
  live_frame_stops (live_at_floor (S below)) frame = false ->
  live_run_steps (S below) (frame :: higher) =
    S (live_run_steps below higher).
Proof.
  intros below frame higher passes.
  change (S (if live_frame_stops (live_at_floor (S below)) frame then 0
    else live_run_steps (pred (S below)) higher) =
    S (live_run_steps below higher)).
  rewrite passes. reflexivity.
Qed.

Lemma live_run_released_stopping_frame : forall above_floor child frame
    higher,
  live_frame_stops (live_at_floor above_floor) frame = true ->
  live_run_released above_floor child (frame :: higher) = [child].
Proof.
  intros above_floor child frame higher stops.
  change (child ::
    (if live_frame_stops (live_at_floor above_floor) frame then []
     else live_run_released (pred above_floor) (live_frame_node frame)
       higher) =
    [child]).
  rewrite stops. reflexivity.
Qed.

Lemma live_run_released_passing_frame : forall below child frame higher,
  live_frame_stops (live_at_floor (S below)) frame = false ->
  live_run_released (S below) child (frame :: higher) =
    child :: live_run_released below (live_frame_node frame) higher.
Proof.
  intros below child frame higher passes.
  change (child ::
    (if live_frame_stops (live_at_floor (S below)) frame then []
     else live_run_released (pred (S below)) (live_frame_node frame)
       higher) =
    child :: live_run_released below (live_frame_node frame) higher).
  rewrite passes. reflexivity.
Qed.

(** One logical edge per step: the prune widths of a run are exactly one
    [1] per step. *)
Theorem live_prune_run_detaches_one_logical_edge_per_step :
  forall frames above_floor,
  prune_widths (live_prune_run above_floor frames) =
    repeat 1 (live_run_steps above_floor frames).
Proof.
  induction frames as [|frame higher IH]; intros above_floor.
  - reflexivity.
  - destruct (live_frame_stops (live_at_floor above_floor) frame) eqn:stops.
    + rewrite (@live_prune_run_stopping_frame above_floor frame higher stops),
        (@live_run_steps_stopping_frame above_floor frame higher stops).
      unfold live_frame_segment.
      rewrite prune_segment_is_one_logical_edge.
      reflexivity.
    + destruct (@live_frame_passing_is_above_the_floor above_floor frame stops)
        as [below ->].
      rewrite (@live_prune_run_passing_frame below frame higher stops),
        (@live_run_steps_passing_frame below frame higher stops),
        prune_widths_app, IH.
      unfold live_frame_segment.
      rewrite prune_segment_is_one_logical_edge.
      reflexivity.
Qed.

(** The floor bounds the run: at most [above_floor + 1] steps, the last one
    below the floor itself. *)
Theorem live_prune_run_respects_the_floor : forall frames above_floor,
  live_run_steps above_floor frames <= S above_floor.
Proof.
  induction frames as [|frame higher IH]; intros above_floor.
  - simpl. lia.
  - destruct (live_frame_stops (live_at_floor above_floor) frame) eqn:stops.
    + rewrite (@live_run_steps_stopping_frame above_floor frame higher stops).
      lia.
    + destruct (@live_frame_passing_is_above_the_floor above_floor frame stops)
        as [below ->].
      rewrite (@live_run_steps_passing_frame below frame higher stops).
      specialize (IH below). lia.
Qed.

(** Where the run stops.  When the floor lies within the frames, the run
    passes value-free single-child parents below the floor, each guarded
    by a barrier that it retires; it stops at the first parent that is the
    floor, holds a real value or forks, with that parent's [prune_segment];
    and it frees the endpoint and the parents it passes, never the
    stopping parent. *)
Theorem live_removal_run_stops_at_the_floor_a_real_value_or_a_fork :
  forall frames above_floor child,
  above_floor < length frames ->
  exists passed stop higher level,
    frames = passed ++ stop :: higher /\
    Forall live_frame_passes passed /\
    level + length passed = above_floor /\
    (level = 0 \/ holds_payload (live_frame_cell stop) = true \/
     live_frame_forks stop = true) /\
    live_prune_run above_floor frames =
      flat_map (fun _ => [InstallBarrier; PruneSpan 1; DetachBarrier])
        passed ++
      prune_segment (holds_payload (live_frame_cell stop)) /\
    live_run_released above_floor child frames =
      child :: map live_frame_node passed.
Proof.
  induction frames as [|frame higher IH]; intros above_floor child bound.
  - simpl in bound. exfalso. lia.
  - destruct (live_frame_stops (live_at_floor above_floor) frame) eqn:stops.
    + exists [], frame, higher, above_floor.
      split; [reflexivity|].
      split; [constructor|].
      split; [simpl; lia|].
      split; [exact (@live_frame_stop_reason above_floor frame stops)|].
      split.
      * rewrite (@live_prune_run_stopping_frame above_floor frame higher stops).
        reflexivity.
      * rewrite (@live_run_released_stopping_frame above_floor child frame
          higher stops).
        reflexivity.
    + destruct (@live_frame_passing_is_above_the_floor above_floor frame stops)
        as [below ->].
      assert (higher_bound : below < length higher).
      { simpl in bound. lia. }
      destruct (IH below (live_frame_node frame) higher_bound) as
        [passed [stop [rest [level
          [shape [passing [levels [reason [trace released]]]]]]]]].
      assert (frame_passes : live_frame_passes frame).
      { unfold live_frame_stops in stops.
        apply orb_false_iff in stops as [floor_or_value forks].
        apply orb_false_iff in floor_or_value as [_ value].
        split; [exact value|exact forks]. }
      exists (frame :: passed), stop, rest, level.
      split; [rewrite shape; reflexivity|].
      split; [constructor; [exact frame_passes|exact passing]|].
      split; [simpl; lia|].
      split; [exact reason|].
      split.
      * rewrite (@live_prune_run_passing_frame below frame higher stops), trace,
          app_assoc.
        unfold live_frame_segment.
        rewrite (proj1 frame_passes).
        reflexivity.
      * rewrite (@live_run_released_passing_frame below child frame higher
          stops), released.
        reflexivity.
Qed.

Lemma live_prune_run_balances_barriers : forall frames above_floor,
  installed_barrier_count (live_prune_run above_floor frames) =
    retired_barrier_count (live_prune_run above_floor frames).
Proof.
  induction frames as [|frame higher IH]; intros above_floor.
  - reflexivity.
  - destruct (live_frame_stops (live_at_floor above_floor) frame) eqn:stops.
    + rewrite (@live_prune_run_stopping_frame above_floor frame higher stops).
      unfold live_frame_segment.
      apply prune_segment_balances_barriers.
    + destruct (@live_frame_passing_is_above_the_floor above_floor frame stops)
        as [below ->].
      rewrite (@live_prune_run_passing_frame below frame higher stops),
        installed_barrier_count_app, retired_barrier_count_app, IH.
      unfold live_frame_segment.
      rewrite prune_segment_balances_barriers.
      reflexivity.
Qed.

(** Every barrier the removal installs is retired: no synthetic barrier is
    left in the live trie. *)
Theorem live_removal_retires_every_installed_barrier : forall value endpoint,
  installed_barrier_count (live_removal_trace value endpoint) =
    retired_barrier_count (live_removal_trace value endpoint).
Proof.
  intros value endpoint. unfold live_removal_trace.
  rewrite installed_barrier_count_app, retired_barrier_count_app.
  destruct (@phase_a_dispose_value_has_no_barrier value)
    as [installed retired].
  rewrite installed, retired.
  destruct endpoint as [|above_floor frames].
  - reflexivity.
  - exact (@live_prune_run_balances_barriers frames above_floor).
Qed.

Lemma live_prune_run_moves_no_real_value : forall frames above_floor,
  removed_without_prune (live_prune_run above_floor frames) = [] /\
  disposed_values (live_prune_run above_floor frames) = [] /\
  detached_payloads (live_prune_run above_floor frames) = [] /\
  queued_par_jobs (live_prune_run above_floor frames) = [].
Proof.
  induction frames as [|frame higher IH]; intros above_floor.
  - repeat split; reflexivity.
  - destruct (live_frame_stops (live_at_floor above_floor) frame) eqn:stops.
    + rewrite (@live_prune_run_stopping_frame above_floor frame higher stops).
      unfold live_frame_segment.
      destruct (holds_payload (live_frame_cell frame));
        repeat split; reflexivity.
    + destruct (@live_frame_passing_is_above_the_floor above_floor frame stops)
        as [below ->].
      rewrite (@live_prune_run_passing_frame below frame higher stops).
      destruct (IH below) as [removed [disposed [detached queued]]].
      rewrite removed_without_prune_app, disposed_values_app,
        detached_payloads_app, queued_par_jobs_app, removed, disposed,
        detached, queued.
      unfold live_frame_segment.
      destruct (holds_payload (live_frame_cell frame));
        repeat split; reflexivity.
Qed.

Lemma live_endpoint_prune_moves_no_real_value : forall endpoint,
  removed_without_prune (live_endpoint_prune endpoint) = [] /\
  disposed_values (live_endpoint_prune endpoint) = [] /\
  detached_payloads (live_endpoint_prune endpoint) = [] /\
  queued_par_jobs (live_endpoint_prune endpoint) = [].
Proof.
  intros [|above_floor frames].
  - repeat split; reflexivity.
  - exact (@live_prune_run_moves_no_real_value frames above_floor).
Qed.

(** Exactly one real value is taken and disposed, the endpoint's own, and
    no original payload is detached by a prune. *)
Theorem live_removal_disposes_exactly_one_real_value : forall value endpoint,
  removed_without_prune (live_removal_trace value endpoint) = [value] /\
  disposed_values (live_removal_trace value endpoint) = [value] /\
  detached_payloads (live_removal_trace value endpoint) = [].
Proof.
  intros value endpoint. unfold live_removal_trace.
  destruct (phase_a_dispose_value_removes_and_disposes_once value)
    as [removed disposed].
  destruct (@live_endpoint_prune_moves_no_real_value endpoint)
    as [removed_prune [disposed_prune [detached_prune _]]].
  rewrite removed_without_prune_app, disposed_values_app,
    detached_payloads_app, removed, disposed, removed_prune, disposed_prune,
    detached_prune, phase_a_dispose_value_detaches_no_original.
  repeat split; reflexivity.
Qed.

(** No Par job: the prune run queues none, the whole removal queues only
    the endpoint value's own job, and a Bag endpoint has none. *)
Theorem live_removal_queues_no_par_job : forall value endpoint,
  queued_par_jobs (live_endpoint_prune endpoint) = [] /\
  queued_par_jobs (live_removal_trace value endpoint) = par_job value /\
  (forall key multiplicity,
    value = BagMultiplicity key multiplicity ->
    queued_par_jobs (live_removal_trace value endpoint) = []).
Proof.
  intros value endpoint.
  destruct (@live_endpoint_prune_moves_no_real_value endpoint)
    as [_ [_ [_ queued_prune]]].
  assert (whole :
    queued_par_jobs (live_removal_trace value endpoint) = par_job value).
  { unfold live_removal_trace.
    rewrite queued_par_jobs_app, queued_prune,
      phase_a_dispose_value_queues_exact_par_job.
    apply app_nil_r. }
  split; [exact queued_prune|].
  split; [exact whole|].
  intros key multiplicity bag.
  rewrite whole, bag. reflexivity.
Qed.

(** A release confined to an exclusive footprint frees no node of any
    external region, so every external anchor keeps its physical image. *)
Theorem exclusive_footprint_release_preserves_external_images : forall
    (owned : PathMapPhysicalOwnership.allocation_region)
    (external : list PathMapPhysicalOwnership.allocation_region)
    (part sibling : PathMapPhysicalOwnership.physical_anchor)
    (sibling_region : PathMapPhysicalOwnership.allocation_region),
  exclusive_terminal_footprint owned external ->
  In sibling_region external ->
  PathMapPhysicalOwnership.anchor_nodes sibling =
    PathMapPhysicalOwnership.region_nodes sibling_region ->
  (forall node, In node (PathMapPhysicalOwnership.anchor_nodes part) ->
    In node (PathMapPhysicalOwnership.region_nodes owned)) ->
  PathMapPhysicalOwnership.physical_image_intact_after_release part sibling.
Proof.
  intros owned external part sibling sibling_region exclusive member
    sibling_nodes within.
  unfold exclusive_terminal_footprint in exclusive.
  rewrite Forall_forall in exclusive.
  destruct (exclusive sibling_region member) as [node_disjoint _].
  apply PathMapPhysicalOwnership.release_preserves_disjoint_anchor_images.
  intros node in_part in_sibling.
  rewrite sibling_nodes in in_sibling.
  exact (node_disjoint node (within node in_part) in_sibling).
Qed.

(** The part of [subject] that a removal frees, as a physical anchor for
    [physical_image_intact_after_release]. *)
Definition live_released_part
    (subject : PathMapPhysicalOwnership.physical_anchor)
    (released : list PathMapPhysicalOwnership.node_id)
    : PathMapPhysicalOwnership.physical_anchor :=
  PathMapPhysicalOwnership.PhysicalAnchor
    (PathMapPhysicalOwnership.anchor_identity subject)
    (PathMapPhysicalOwnership.anchor_root_mode subject)
    released
    (PathMapPhysicalOwnership.anchor_logical_paths subject)
    (PathMapPhysicalOwnership.anchor_raw_headers subject)
    (PathMapPhysicalOwnership.anchor_arc_wrappers subject)
    (PathMapPhysicalOwnership.anchor_destructive_capability subject).

Lemma live_run_released_within_the_chain : forall frames above_floor child
    node,
  In node (live_run_released above_floor child frames) ->
  node = child \/
  exists frame, In frame frames /\ node = live_frame_node frame.
Proof.
  induction frames as [|frame higher IH]; intros above_floor child node
    member.
  - destruct member.
  - destruct (live_frame_stops (live_at_floor above_floor) frame) eqn:stops.
    + rewrite (@live_run_released_stopping_frame above_floor child frame
        higher stops) in member.
      destruct member as [same|member].
      * left. symmetry. exact same.
      * destruct member.
    + destruct (@live_frame_passing_is_above_the_floor above_floor frame stops)
        as [below ->].
      rewrite (@live_run_released_passing_frame below child frame higher
        stops) in member.
      destruct member as [same|member].
      * left. symmetry. exact same.
      * destruct (IH below (live_frame_node frame) node member)
          as [same|[upper [inside same]]].
        -- right. exists frame. split; [left; reflexivity|exact same].
        -- right. exists upper. split; [right; exact inside|exact same].
Qed.

(** External images survive the live removal: when the endpoint and the
    parents of the chain lie in a footprint that is exclusive of the
    external regions, the nodes the run frees leave every external anchor's
    image intact. *)
Theorem live_removal_run_preserves_external_images : forall
    (owned : PathMapPhysicalOwnership.allocation_region)
    (external : list PathMapPhysicalOwnership.allocation_region)
    (subject sibling : PathMapPhysicalOwnership.physical_anchor)
    (sibling_region : PathMapPhysicalOwnership.allocation_region)
    (above_floor : nat) (child : PathMapPhysicalOwnership.node_id)
    (frames : list live_frame),
  exclusive_terminal_footprint owned external ->
  In sibling_region external ->
  PathMapPhysicalOwnership.anchor_nodes sibling =
    PathMapPhysicalOwnership.region_nodes sibling_region ->
  In child (PathMapPhysicalOwnership.region_nodes owned) ->
  (forall frame, In frame frames ->
    In (live_frame_node frame) (PathMapPhysicalOwnership.region_nodes owned)) ->
  PathMapPhysicalOwnership.physical_image_intact_after_release
    (live_released_part subject (live_run_released above_floor child frames))
    sibling.
Proof.
  intros owned external subject sibling sibling_region above_floor child frames
    exclusive member sibling_nodes child_owned frames_owned.
  apply (@exclusive_footprint_release_preserves_external_images owned external
    (live_released_part subject (live_run_released above_floor child frames))
    sibling sibling_region exclusive member sibling_nodes).
  intros node released.
  destruct (@live_run_released_within_the_chain frames above_floor child node
      released) as [same|[frame [inside same]]].
  - rewrite same. exact child_owned.
  - rewrite same. exact (frames_owned frame inside).
Qed.

(** The footprint of a provenance anchor: its nodes, and the values stored
    in them (a value is identified with its node here, so value disjointness
    follows from node disjointness).  [structural_candidate_disjoint] does
    not read the allocation field. *)
Definition live_node_region
    (anchor : PathMapPhysicalOwnership.provenance_anchor)
    : PathMapPhysicalOwnership.allocation_region :=
  PathMapPhysicalOwnership.AllocationRegion 0
    (PathMapPhysicalOwnership.provenance_nodes anchor)
    (PathMapPhysicalOwnership.provenance_nodes anchor).

(** Composition with the provenance model of [PathMapPhysicalOwnership]: on
    every reachable provenance world, an anchor that passes the production
    gate (provenance bit set, one wrapper) has a footprint exclusive of the
    anchors at all other positions, and so its live removal, whose chain
    lies in its own nodes, leaves every one of their images intact. *)
Theorem exclusive_provenance_licenses_live_removal : forall commands position
    anchor externals,
  nth_error
      (PathMapPhysicalOwnership.provenance_anchors
        (PathMapPhysicalOwnership.provenance_run commands)) position =
    Some anchor ->
  PathMapPhysicalOwnership.exclusive_removal_gate anchor = true ->
  (forall other, In other externals ->
    exists other_position,
      nth_error
          (PathMapPhysicalOwnership.provenance_anchors
            (PathMapPhysicalOwnership.provenance_run commands))
          other_position = Some other /\
      other_position <> position) ->
  exclusive_terminal_footprint (live_node_region anchor)
    (map live_node_region externals) /\
  forall above_floor child frames other,
    In child (PathMapPhysicalOwnership.provenance_nodes anchor) ->
    (forall frame, In frame frames ->
      In (live_frame_node frame)
        (PathMapPhysicalOwnership.provenance_nodes anchor)) ->
    In other externals ->
    PathMapPhysicalOwnership.physical_image_intact_after_release
      (live_released_part (PathMapPhysicalOwnership.provenance_physical anchor)
        (live_run_released above_floor child frames))
      (PathMapPhysicalOwnership.provenance_physical other).
Proof.
  intros commands position anchor externals at_position gate positioned.
  assert (footprint :
    exclusive_terminal_footprint (live_node_region anchor)
      (map live_node_region externals)).
  { unfold exclusive_terminal_footprint.
    apply Forall_forall.
    intros region member.
    apply in_map_iff in member as [other [region_of_other other_member]].
    subst region.
    destruct (positioned other other_member)
      as [other_position [at_other different]].
    pose proof
      (@PathMapPhysicalOwnership.exclusive_gate_implies_physical_isolation
        commands position anchor at_position gate other_position other at_other
        different) as disjoint.
    split; exact disjoint. }
  split; [exact footprint|].
  intros above_floor child frames other child_owned frames_owned other_member.
  assert (region_member :
    In (live_node_region other) (map live_node_region externals)).
  { apply in_map. exact other_member. }
  exact (@live_removal_run_preserves_external_images
    (live_node_region anchor) (map live_node_region externals)
    (PathMapPhysicalOwnership.provenance_physical anchor)
    (PathMapPhysicalOwnership.provenance_physical other)
    (live_node_region other) above_floor child frames footprint region_member
    eq_refl child_owned frames_owned).
Qed.

(** Fixtures, also used by the controls in
    PathMapReversePruneTeardownMutants.v.  [live_chain_frames]: two
    value-free single-child parents below a valued one, far below the floor.
    [live_floor_frames]: value-free single-child parents; with
    [above_floor = 1] the second is the floor.  [live_fork_frames]: a
    value-free parent that keeps another child. *)
Definition live_bag_value : payload := BagMultiplicity 7 1.

Definition live_chain_frames : list live_frame :=
  [LiveFrame (0, 2) None false; LiveFrame (0, 1) None false;
   LiveFrame (0, 0) (Some (SetMember 9)) false].

Definition live_floor_frames : list live_frame :=
  [LiveFrame (1, 3) None false; LiveFrame (1, 2) None false;
   LiveFrame (1, 1) None false].

Definition live_fork_frames : list live_frame :=
  [LiveFrame (2, 1) None true; LiveFrame (2, 0) None false].

(** Non-vacuity: a chain run that stops at a real value, a run that stops
    at the floor, a run that stops at a fork, and a value-only removal. *)
Theorem live_removal_model_is_nonvacuous :
  live_removal_trace live_bag_value (LiveLeafEndpoint 5 live_chain_frames) =
    [RemoveValueNoPrune live_bag_value; DisposeValue live_bag_value;
     InstallBarrier; PruneSpan 1; DetachBarrier;
     InstallBarrier; PruneSpan 1; DetachBarrier; PruneSpan 1] /\
  live_run_released 5 (0, 3) live_chain_frames = [(0, 3); (0, 2); (0, 1)] /\
  live_prune_run 1 live_floor_frames =
    [InstallBarrier; PruneSpan 1; DetachBarrier;
     InstallBarrier; PruneSpan 1; DetachBarrier] /\
  live_run_released 1 (1, 4) live_floor_frames = [(1, 4); (1, 3)] /\
  live_prune_run 4 live_fork_frames =
    [InstallBarrier; PruneSpan 1; DetachBarrier] /\
  live_removal_trace live_bag_value LiveValueOnlyEndpoint =
    [RemoveValueNoPrune live_bag_value; DisposeValue live_bag_value].
Proof. vm_compute. repeat split; reflexivity. Qed.

(** Two entries were retired when Phase B became barrier-free, because they
    stated Phase B's barrier protocol and Phase B no longer installs a barrier.
    Their statements stay proved, as theorems about the live removal path's
    step [prune_segment] ([barrier_never_overwrites_a_real_parent_value] and
    [immediate_parent_barrier_protocol]), which [TwoLevelRSpaceGuardedOwnership]
    still uses.  [BarrierFreePhaseBTraceInvariant] and
    [BarrierFreeStepClassificationInvariant] take their catalog positions, and
    [OneEdgePruneInvariant] is rebound from [prune_segment] to the barrier-free
    step.  The retired bindings were:

    | BarrierNonOverwriteInvariant => ~ In InstallBarrier (prune_segment true)
    | ImmediateParentBarrierInvariant => forall parent_has_value,
        branch_safe_segment parent_has_value (prune_segment parent_has_value)
    | OneEdgePruneInvariant => forall parent_has_value,
        prune_widths (prune_segment parent_has_value) = [1]
*)
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
| BarrierFreePhaseBTraceInvariant
| BarrierFreeStepClassificationInvariant
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
| CanonicalEmptyFinalAnchorFastPathInvariant
| LiveRemovalOneEdgePerStepInvariant
| LiveRemovalFloorBoundInvariant
| LiveRemovalStopInvariant
| LiveRemovalBarrierBalanceInvariant
| LiveRemovalSingleDisposalInvariant
| LiveRemovalNoParJobInvariant
| LiveRemovalExternalImageInvariant
| ExclusiveProvenanceLiveRemovalInvariant.

Definition invariant_catalog : list invariant_id :=
  [AnchorArcAuthorityInvariant; PhaseAPreorderInvariant;
   RootPayloadInvariant; PhaseAExactOnceInvariant;
   PhaseAImmediateDisposalInvariant; PhaseACloseZeroValidationInvariant;
   PhaseANoTopologyCloseOrderInvariant;
   PhaseAConstantControlNoDepthCapInvariant;
   PhaseBValueFreeProvenanceInvariant;
   BarrierFreePhaseBTraceInvariant; BarrierFreeStepClassificationInvariant;
   OneEdgePruneInvariant; TopologyProgressInvariant;
   NoRecursiveRawDropInvariant; ImmutableCloneHeaderInvariant;
   FinalArcElectionInvariant; MakeMutFreshAnchorInvariant;
   MakeMutAbortRegistrationInvariant;
   TerminalStorageSafetyEvidenceInvariant; ParPdaTransferInvariant;
   WholePrivateRootExitInvariant; PrivatePublicationInvariant;
   FailurePrecedenceInvariant; TerminalAllocationOutcomeInvariant;
   CanonicalEmptyFinalAnchorFastPathInvariant;
   LiveRemovalOneEdgePerStepInvariant; LiveRemovalFloorBoundInvariant;
   LiveRemovalStopInvariant; LiveRemovalBarrierBalanceInvariant;
   LiveRemovalSingleDisposalInvariant; LiveRemovalNoParJobInvariant;
   LiveRemovalExternalImageInvariant;
   ExclusiveProvenanceLiveRemovalInvariant].

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
  | BarrierFreePhaseBTraceInvariant => forall phase_a zero_values,
      run_phase_b phase_a zero_values =
        BeginTopologyPhase ::
          repeat (PruneSpan 1) (tree_edges (phase_a_value_free_root phase_a))
  | BarrierFreeStepClassificationInvariant => forall view,
      (barrier_free_step_kind view = BranchCut <->
         frame_is_root (view_parent view) = false /\
         frame_cell (view_parent view) = None /\
         view_left_children view = FNil) /\
      (barrier_free_step_kind view = OneBytePrune <->
         frame_is_root (view_parent view) = true \/
         (exists value, frame_cell (view_parent view) = Some value) \/
         (exists sibling rest, view_left_children view = FCons sibling rest))
  | OneEdgePruneInvariant => forall view,
      step_detached_edges (barrier_free_step_kind view) view = 1 /\
      prune_widths (barrier_free_segment view) = [1]
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
  | LiveRemovalOneEdgePerStepInvariant =>
      forall frames above_floor,
      prune_widths (live_prune_run above_floor frames) =
        repeat 1 (live_run_steps above_floor frames)
  | LiveRemovalFloorBoundInvariant =>
      forall frames above_floor,
      live_run_steps above_floor frames <= S above_floor
  | LiveRemovalStopInvariant =>
      forall frames above_floor child,
      above_floor < length frames ->
      exists passed stop higher level,
        frames = passed ++ stop :: higher /\
        Forall live_frame_passes passed /\
        level + length passed = above_floor /\
        (level = 0 \/ holds_payload (live_frame_cell stop) = true \/
         live_frame_forks stop = true) /\
        live_prune_run above_floor frames =
          flat_map (fun _ => [InstallBarrier; PruneSpan 1; DetachBarrier])
            passed ++
          prune_segment (holds_payload (live_frame_cell stop)) /\
        live_run_released above_floor child frames =
          child :: map live_frame_node passed
  | LiveRemovalBarrierBalanceInvariant =>
      forall value endpoint,
      installed_barrier_count (live_removal_trace value endpoint) =
        retired_barrier_count (live_removal_trace value endpoint)
  | LiveRemovalSingleDisposalInvariant =>
      forall value endpoint,
      removed_without_prune (live_removal_trace value endpoint) = [value] /\
      disposed_values (live_removal_trace value endpoint) = [value] /\
      detached_payloads (live_removal_trace value endpoint) = []
  | LiveRemovalNoParJobInvariant =>
      forall value endpoint,
      queued_par_jobs (live_endpoint_prune endpoint) = [] /\
      queued_par_jobs (live_removal_trace value endpoint) = par_job value /\
      (forall key multiplicity,
        value = BagMultiplicity key multiplicity ->
        queued_par_jobs (live_removal_trace value endpoint) = [])
  | LiveRemovalExternalImageInvariant =>
      forall
        (owned : PathMapPhysicalOwnership.allocation_region)
        (external : list PathMapPhysicalOwnership.allocation_region)
        (subject sibling : PathMapPhysicalOwnership.physical_anchor)
        (sibling_region : PathMapPhysicalOwnership.allocation_region)
        (above_floor : nat) (child : PathMapPhysicalOwnership.node_id)
        (frames : list live_frame),
      exclusive_terminal_footprint owned external ->
      In sibling_region external ->
      PathMapPhysicalOwnership.anchor_nodes sibling =
        PathMapPhysicalOwnership.region_nodes sibling_region ->
      In child (PathMapPhysicalOwnership.region_nodes owned) ->
      (forall frame, In frame frames ->
        In (live_frame_node frame) (PathMapPhysicalOwnership.region_nodes owned)) ->
      PathMapPhysicalOwnership.physical_image_intact_after_release
        (live_released_part subject (live_run_released above_floor child frames))
        sibling
  | ExclusiveProvenanceLiveRemovalInvariant =>
      forall commands position
        anchor externals,
      nth_error
          (PathMapPhysicalOwnership.provenance_anchors
            (PathMapPhysicalOwnership.provenance_run commands)) position =
        Some anchor ->
      PathMapPhysicalOwnership.exclusive_removal_gate anchor = true ->
      (forall other, In other externals ->
        exists other_position,
          nth_error
              (PathMapPhysicalOwnership.provenance_anchors
                (PathMapPhysicalOwnership.provenance_run commands))
              other_position = Some other /\
          other_position <> position) ->
      exclusive_terminal_footprint (live_node_region anchor)
        (map live_node_region externals) /\
      forall above_floor child frames other,
        In child (PathMapPhysicalOwnership.provenance_nodes anchor) ->
        (forall frame, In frame frames ->
          In (live_frame_node frame)
            (PathMapPhysicalOwnership.provenance_nodes anchor)) ->
        In other externals ->
        PathMapPhysicalOwnership.physical_image_intact_after_release
          (live_released_part (PathMapPhysicalOwnership.provenance_physical anchor)
            (live_run_released above_floor child frames))
          (PathMapPhysicalOwnership.provenance_physical other)
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
  - apply phase_b_emits_one_prune_event_per_logical_edge_and_nothing_else.
  - apply barrier_free_step_classification.
  - intro view. split.
    + apply every_barrier_free_step_detaches_exactly_one_logical_edge.
    + apply barrier_free_segment_prunes_one_logical_edge.
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
  - exact @live_prune_run_detaches_one_logical_edge_per_step.
  - exact @live_prune_run_respects_the_floor.
  - exact @live_removal_run_stops_at_the_floor_a_real_value_or_a_fork.
  - exact @live_removal_retires_every_installed_barrier.
  - exact @live_removal_disposes_exactly_one_real_value.
  - exact @live_removal_queues_no_par_job.
  - exact @live_removal_run_preserves_external_images.
  - exact @exclusive_provenance_licenses_live_removal.
Qed.

Theorem invariant_catalog_is_duplicate_free : NoDup invariant_catalog.
Proof. repeat constructor; simpl; intuition discriminate. Qed.

Theorem invariant_catalog_has_exactly_thirty_three_entries :
  length invariant_catalog = 33.
Proof. reflexivity. Qed.

Theorem invariant_catalog_is_unique_complete_and_exact :
  NoDup invariant_catalog /\
  Forall invariant_binding invariant_catalog /\
  length invariant_catalog = 33.
Proof.
  split.
  - apply invariant_catalog_is_duplicate_free.
  - split.
    + apply Forall_forall. intros invariant _.
      apply every_primary_invariant_is_bound.
    + apply invariant_catalog_has_exactly_thirty_three_entries.
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

Module NavigationRefinement.

Record navigation_plan (Route : Type) := NavigationPlan {
  plan_route : Route;
  plan_leaf : option payload;
  plan_resume : list teardown_event;
  plan_unary : bool
}.

Arguments NavigationPlan {Route}.

Definition has_payload (cell : option payload) : bool :=
  match cell with Some _ => true | None => false end.

Definition unary_root (cell : option payload) (children : forest) : bool :=
  match cell, children with
  | None, FCons _ FNil => true
  | _, _ => false
  end.

Definition append_resume {Route} (plan : navigation_plan Route)
    (events : list teardown_event) : navigation_plan Route :=
  NavigationPlan (plan_route plan) (plan_leaf plan)
    (plan_resume plan ++ events) (plan_unary plan).

Definition finish_plan {Route} (plan : navigation_plan Route)
    : list teardown_event :=
  dispose_cell (plan_leaf plan) ++ plan_resume plan.

Definition finish_forest {Route}
    (selected : option (nat * navigation_plan Route)) : list teardown_event :=
  match selected with None => [] | Some (_, plan) => finish_plan plan end.

Fixpoint scalar_node (is_root : bool) (ascent : nat) (subject : trie)
    {struct subject} : navigation_plan (list nat) :=
  match subject with
  | Node cell children =>
      match scalar_forest children (ParentFrame is_root cell ascent) FNil with
      | None => NavigationPlan [] cell [] false
      | Some (index, plan) =>
          NavigationPlan (index :: plan_route plan) (plan_leaf plan)
            (plan_resume plan ++ dispose_cell cell) (unary_root cell children)
      end
  end
with scalar_forest (subjects : forest) (parent : parent_frame) (left : forest)
    {struct subjects} : option (nat * navigation_plan (list nat)) :=
  match subjects with
  | FNil => None
  | FCons subject rest =>
      match scalar_forest rest parent (FCons subject left) with
      | None =>
          Some (0, append_resume
            (scalar_node false (upward_prune_reach (StepView parent left)) subject)
            (barrier_free_segment (StepView parent left)))
      | Some (index, plan) =>
          Some (S index, append_resume plan
            (drain_node false (upward_prune_reach (StepView parent left)) subject ++
             barrier_free_segment (StepView parent left)))
      end
  end.

Definition scalar_tree (subject : trie) : navigation_plan (list nat) :=
  scalar_node true 0 subject.

Definition add_compressed_edge (unary : bool) (index : nat)
    (chunks : list (list nat)) : list (list nat) :=
  if unary then
    match chunks with
    | [] => [[index]]
    | chunk :: rest => (index :: chunk) :: rest
    end
  else [index] :: chunks.

Fixpoint compressed_node (is_root : bool) (ascent : nat) (subject : trie)
    {struct subject} : navigation_plan (list (list nat)) :=
  match subject with
  | Node cell children =>
      match compressed_forest children (ParentFrame is_root cell ascent) FNil with
      | None => NavigationPlan [] cell [] false
      | Some (index, plan) =>
          NavigationPlan
            (add_compressed_edge (plan_unary plan) index (plan_route plan))
            (plan_leaf plan) (plan_resume plan ++ dispose_cell cell)
            (unary_root cell children)
      end
  end
with compressed_forest (subjects : forest) (parent : parent_frame) (left : forest)
    {struct subjects} : option (nat * navigation_plan (list (list nat))) :=
  match subjects with
  | FNil => None
  | FCons subject rest =>
      match compressed_forest rest parent (FCons subject left) with
      | None =>
          Some (0, append_resume
            (compressed_node false (upward_prune_reach (StepView parent left)) subject)
            (barrier_free_segment (StepView parent left)))
      | Some (index, plan) =>
          Some (S index, append_resume plan
            (drain_node false (upward_prune_reach (StepView parent left)) subject ++
             barrier_free_segment (StepView parent left)))
      end
  end.

Definition compressed_tree (subject : trie) : navigation_plan (list (list nat)) :=
  compressed_node true 0 subject.

Definition expand_plan (plan : navigation_plan (list (list nat)))
    : navigation_plan (list nat) :=
  NavigationPlan (concat (plan_route plan)) (plan_leaf plan)
    (plan_resume plan) (plan_unary plan).

Definition expand_forest
    (selected : option (nat * navigation_plan (list (list nat))))
    : option (nat * navigation_plan (list nat)) :=
  match selected with
  | None => None
  | Some (index, plan) => Some (index, expand_plan plan)
  end.

Lemma compressed_edge_expands_exactly : forall unary index chunks,
  concat (add_compressed_edge unary index chunks) = index :: concat chunks.
Proof. intros [] index [|chunk rest]; reflexivity. Qed.

Lemma expansion_preserves_resume : forall plan events,
  expand_plan (append_resume plan events) = append_resume (expand_plan plan) events.
Proof. reflexivity. Qed.

Lemma compressed_and_scalar_node_descent_agree :
  (forall subject is_root ascent,
    expand_plan (compressed_node is_root ascent subject) =
      scalar_node is_root ascent subject) /\
  (forall subjects parent left,
    expand_forest (compressed_forest subjects parent left) =
      scalar_forest subjects parent left).
Proof.
  apply trie_forest_induction.
  - intros cell children IH is_root ascent. cbn [compressed_node scalar_node].
    specialize (IH (ParentFrame is_root cell ascent) FNil).
    destruct (compressed_forest children (ParentFrame is_root cell ascent) FNil)
      as [[index plan]|] eqn:E;
      simpl in IH; rewrite <- IH; simpl; try reflexivity.
    unfold expand_plan. simpl. rewrite compressed_edge_expands_exactly. reflexivity.
  - reflexivity.
  - intros subject IHsubject rest IHrest parent left.
    cbn [compressed_forest scalar_forest].
    specialize (IHrest parent (FCons subject left)).
    destruct (compressed_forest rest parent (FCons subject left))
      as [[index plan]|] eqn:E;
      simpl in IHrest; rewrite <- IHrest; simpl.
    + now rewrite expansion_preserves_resume.
    + now rewrite expansion_preserves_resume, IHsubject.
Qed.

Theorem compressed_and_scalar_descent_agree :
  (forall subject, expand_plan (compressed_tree subject) = scalar_tree subject) /\
  (forall subjects parent left,
    expand_forest (compressed_forest subjects parent left) =
      scalar_forest subjects parent left).
Proof.
  split.
  - intro subject. unfold compressed_tree, scalar_tree.
    apply (proj1 compressed_and_scalar_node_descent_agree).
  - exact (proj2 compressed_and_scalar_node_descent_agree).
Qed.

Lemma finish_append_resume : forall Route (plan : navigation_plan Route) events,
  finish_plan (append_resume plan events) = finish_plan plan ++ events.
Proof. intros. unfold finish_plan, append_resume. simpl. now rewrite app_assoc. Qed.

Lemma scalar_node_descent_reconstructs_exact_drain :
  (forall subject is_root ascent,
    finish_plan (scalar_node is_root ascent subject) =
      drain_node is_root ascent subject) /\
  (forall subjects parent left,
    finish_forest (scalar_forest subjects parent left) =
      drain_forest subjects parent left).
Proof.
  apply trie_forest_induction.
  - intros cell children IH is_root ascent. cbn [scalar_node drain_node].
    specialize (IH (ParentFrame is_root cell ascent) FNil).
    destruct (scalar_forest children (ParentFrame is_root cell ascent) FNil)
      as [[index plan]|] eqn:E;
      simpl in IH.
    + unfold finish_plan. simpl.
      rewrite app_assoc. rewrite <- IH. reflexivity.
    + unfold finish_plan. simpl.
      rewrite <- IH. simpl. apply app_nil_r.
  - reflexivity.
  - intros subject IHsubject rest IHrest parent left.
    cbn [scalar_forest drain_forest].
    specialize (IHrest parent (FCons subject left)).
    destruct (scalar_forest rest parent (FCons subject left))
      as [[index plan]|] eqn:E;
      simpl in IHrest; cbn [finish_forest].
    + rewrite finish_append_resume, IHrest. reflexivity.
    + rewrite finish_append_resume, IHsubject, <- IHrest. reflexivity.
Qed.

Theorem scalar_descent_reconstructs_exact_drain :
  (forall subject, finish_plan (scalar_tree subject) = drain_tree subject) /\
  (forall subjects parent left,
    finish_forest (scalar_forest subjects parent left) =
      drain_forest subjects parent left).
Proof.
  split.
  - intro subject. unfold scalar_tree, drain_tree.
    apply (proj1 scalar_node_descent_reconstructs_exact_drain).
  - exact (proj2 scalar_node_descent_reconstructs_exact_drain).
Qed.

Fixpoint select_child (index : nat) (subjects : forest) : option trie :=
  match index, subjects with
  | _, FNil => None
  | 0, FCons subject _ => Some subject
  | S index, FCons _ rest => select_child index rest
  end.

Fixpoint follow_route (route : list nat) (subject : trie) : option trie :=
  match route with
  | [] => Some subject
  | index :: rest =>
      match subject with Node _ children =>
        match select_child index children with
        | None => None
        | Some child => follow_route rest child
        end
      end
  end.

Lemma scalar_forest_none_is_empty : forall subjects parent left,
  scalar_forest subjects parent left = None -> subjects = FNil.
Proof.
  intros [|subject rest] parent left E; auto.
  cbn [scalar_forest] in E.
  destruct (scalar_forest rest parent (FCons subject left)) as [[index plan]|];
    discriminate.
Qed.

Lemma scalar_node_descent_reaches_a_leaf :
  (forall subject is_root ascent,
    follow_route (plan_route (scalar_node is_root ascent subject)) subject =
      Some (Node (plan_leaf (scalar_node is_root ascent subject)) FNil)) /\
  (forall subjects parent left index plan,
    scalar_forest subjects parent left = Some (index, plan) ->
    match select_child index subjects with
    | None => False
    | Some child => follow_route (plan_route plan) child = Some (Node (plan_leaf plan) FNil)
    end).
Proof.
  apply trie_forest_induction.
  - intros cell children IH is_root ascent. cbn [scalar_node].
    destruct (scalar_forest children (ParentFrame is_root cell ascent) FNil)
      as [[index plan]|] eqn:E.
    + specialize (IH _ _ _ _ E). simpl.
      destruct (select_child index children); simpl in *; auto; contradiction.
    + apply scalar_forest_none_is_empty in E. subst. reflexivity.
  - intros parent left index plan E. discriminate.
  - intros subject IHsubject rest IHrest parent left index plan E.
    cbn [scalar_forest] in E.
    destruct (scalar_forest rest parent (FCons subject left))
      as [[selected tail]|] eqn:F;
      inversion E; subst; clear E; simpl.
    + exact (IHrest _ _ _ _ F).
    + apply IHsubject.
Qed.

Theorem scalar_descent_reaches_a_leaf :
  (forall subject,
    follow_route (plan_route (scalar_tree subject)) subject =
      Some (Node (plan_leaf (scalar_tree subject)) FNil)) /\
  (forall subjects parent left index plan,
    scalar_forest subjects parent left = Some (index, plan) ->
    match select_child index subjects with
    | None => False
    | Some child => follow_route (plan_route plan) child = Some (Node (plan_leaf plan) FNil)
    end).
Proof.
  split.
  - intro subject. unfold scalar_tree.
    apply (proj1 scalar_node_descent_reaches_a_leaf).
  - exact (proj2 scalar_node_descent_reaches_a_leaf).
Qed.

Fixpoint forest_width (subjects : forest) : nat :=
  match subjects with FNil => 0 | FCons _ rest => S (forest_width rest) end.

Theorem scalar_forest_selects_greatest_child : forall subjects parent left index plan,
  scalar_forest subjects parent left = Some (index, plan) ->
  S index = forest_width subjects.
Proof.
  induction subjects as [|subject rest IH]; intros parent left index plan E;
    cbn [scalar_forest] in E.
  - discriminate.
  - destruct (scalar_forest rest parent (FCons subject left))
      as [[selected tail]|] eqn:F;
      inversion E; subst; clear E; simpl.
    + f_equal. exact (IH _ _ _ _ F).
    + apply scalar_forest_none_is_empty in F. subst. reflexivity.
Qed.

Theorem compressed_descent_reaches_the_same_leaf : forall subject,
  follow_route (concat (plan_route (compressed_tree subject))) subject =
    Some (Node (plan_leaf (compressed_tree subject)) FNil).
Proof.
  intro subject. pose proof (proj1 scalar_descent_reaches_a_leaf subject) as H.
  rewrite <- (proj1 compressed_and_scalar_descent_agree subject) in H.
  exact H.
Qed.

Theorem compressed_descent_reconstructs_exact_drain : forall subject,
  finish_plan (compressed_tree subject) = drain_tree subject.
Proof.
  intro subject. pose proof (proj1 scalar_descent_reconstructs_exact_drain subject) as H.
  rewrite <- (proj1 compressed_and_scalar_descent_agree subject) in H.
  exact H.
Qed.

Definition compressed_terminal_trace (subject : trie) : list teardown_event :=
  phase_a_value_trace subject ++ BeginTopologyPhase ::
    finish_plan (compressed_tree (erase_values_tree subject)).

Theorem compressed_navigation_preserves_terminal_trace : forall subject,
  compressed_terminal_trace subject = terminal_two_phase_trace subject.
Proof.
  intro subject. unfold compressed_terminal_trace.
  rewrite compressed_descent_reconstructs_exact_drain. reflexivity.
Qed.

Theorem compressed_navigation_preserves_disposal_and_one_edge_pruning : forall subject,
  removed_without_prune (compressed_terminal_trace subject) = preorder_payloads_tree subject /\
  disposed_values (compressed_terminal_trace subject) = preorder_payloads_tree subject /\
  prune_widths (compressed_terminal_trace subject) = repeat 1 (tree_edges subject).
Proof.
  intro subject. rewrite compressed_navigation_preserves_terminal_trace.
  pose proof (terminal_two_phase_trace_is_exact_and_topology_complete subject)
    as [removed [disposed [_ [_ pruned]]]]. auto.
Qed.

Example compressed_line_nonvacuity :
  plan_route (compressed_tree (topology_line 8)) = [repeat 0 8] /\
  plan_route (scalar_tree (topology_line 8)) = repeat 0 8.
Proof. vm_compute. auto. Qed.

Example compressed_branch_nonvacuity :
  plan_route (compressed_tree
    (Node None (FCons (topology_line 2) (FCons (topology_line 3) FNil)))) =
    [[1; 0; 0; 0]].
Proof. reflexivity. Qed.

Inductive navigation_invariant :=
| ExpandedRouteInvariant
| TerminalLeafInvariant
| GreatestChildInvariant
| ExactContinuationInvariant
| TerminalTraceInvariant
| DisposalAndPruneInvariant
| CompressedLineWitnessInvariant
| GreatestBranchWitnessInvariant.

Definition navigation_catalog : list navigation_invariant :=
  [ExpandedRouteInvariant; TerminalLeafInvariant; GreatestChildInvariant;
   ExactContinuationInvariant; TerminalTraceInvariant; DisposalAndPruneInvariant;
   CompressedLineWitnessInvariant; GreatestBranchWitnessInvariant].

Definition navigation_binding (invariant : navigation_invariant) : Prop :=
  match invariant with
  | ExpandedRouteInvariant => forall subject,
      expand_plan (compressed_tree subject) = scalar_tree subject
  | TerminalLeafInvariant => forall subject,
      follow_route (concat (plan_route (compressed_tree subject))) subject =
        Some (Node (plan_leaf (compressed_tree subject)) FNil)
  | GreatestChildInvariant => forall subjects parent left index plan,
      scalar_forest subjects parent left = Some (index, plan) ->
      S index = forest_width subjects
  | ExactContinuationInvariant => forall subject,
      finish_plan (compressed_tree subject) = drain_tree subject
  | TerminalTraceInvariant => forall subject,
      compressed_terminal_trace subject = terminal_two_phase_trace subject
  | DisposalAndPruneInvariant => forall subject,
      removed_without_prune (compressed_terminal_trace subject) = preorder_payloads_tree subject /\
      disposed_values (compressed_terminal_trace subject) = preorder_payloads_tree subject /\
      prune_widths (compressed_terminal_trace subject) = repeat 1 (tree_edges subject)
  | CompressedLineWitnessInvariant =>
      plan_route (compressed_tree (topology_line 8)) = [repeat 0 8] /\
      plan_route (scalar_tree (topology_line 8)) = repeat 0 8
  | GreatestBranchWitnessInvariant =>
      plan_route (compressed_tree
        (Node None (FCons (topology_line 2) (FCons (topology_line 3) FNil)))) =
        [[1; 0; 0; 0]]
  end.

Theorem every_navigation_invariant_is_bound : forall invariant,
  navigation_binding invariant.
Proof.
  intros []; cbn [navigation_binding].
  - exact (proj1 compressed_and_scalar_descent_agree).
  - exact compressed_descent_reaches_the_same_leaf.
  - exact scalar_forest_selects_greatest_child.
  - exact compressed_descent_reconstructs_exact_drain.
  - exact compressed_navigation_preserves_terminal_trace.
  - exact compressed_navigation_preserves_disposal_and_one_edge_pruning.
  - exact compressed_line_nonvacuity.
  - exact compressed_branch_nonvacuity.
Qed.

Theorem navigation_catalog_is_unique_complete_and_exact :
  NoDup navigation_catalog /\
  Forall navigation_binding navigation_catalog /\
  length navigation_catalog = 8 /\
  (forall invariant, In invariant navigation_catalog).
Proof.
  split.
  - repeat constructor; simpl; intuition discriminate.
  - split.
    + apply Forall_forall. intros invariant _. apply every_navigation_invariant_is_bound.
    + split; [reflexivity|]. intros []; simpl; tauto.
Qed.

Print Assumptions compressed_and_scalar_descent_agree.
Print Assumptions scalar_descent_reconstructs_exact_drain.
Print Assumptions scalar_descent_reaches_a_leaf.
Print Assumptions scalar_forest_selects_greatest_child.
Print Assumptions compressed_descent_reaches_the_same_leaf.
Print Assumptions compressed_descent_reconstructs_exact_drain.
Print Assumptions compressed_navigation_preserves_terminal_trace.
Print Assumptions compressed_navigation_preserves_disposal_and_one_edge_pruning.
Print Assumptions navigation_catalog_is_unique_complete_and_exact.
Print Assumptions compressed_and_scalar_node_descent_agree.
Print Assumptions scalar_node_descent_reconstructs_exact_drain.
Print Assumptions scalar_forest_none_is_empty.
Print Assumptions scalar_node_descent_reaches_a_leaf.

End NavigationRefinement.

(** Node-level refinement of the barrier-free Phase B step (remedy study,
    section 2.5).  The trie-level model above states what one step detaches
    in logical edges.  This module models the physical nodes of PathMap 0.3.4
    that hold the zipper's focus, transcribes the two calls of the step
    ([remove_branches(false)] and [prune_path]) from the crate sources, and
    proves the study's Lemmas 1 to 3 and the stack bound of its Theorem 1.
    The study's premises P1 to P6 are the conjuncts of the one predicate
    [pathmap_premises]; none of them is assumed globally. *)
Module BarrierFreeNodeRefinement.

#[local] Unset Implicit Arguments.

(** ** Keys

    A byte is a natural number; keys and paths are byte lists. *)

Definition prefix (shorter longer : list nat) : Prop :=
  exists rest, longer = shorter ++ rest.

Definition proper_prefix (shorter longer : list nat) : Prop :=
  exists rest, rest <> [] /\ longer = shorter ++ rest.

Lemma prefix_reflexive : forall key, prefix key key.
Proof. intro key. exists []. symmetry. apply app_nil_r. Qed.

Lemma prefix_of_extension : forall start rest key,
  prefix (start ++ rest) key -> prefix start key.
Proof.
  intros start rest key [more extended]. exists (rest ++ more).
  rewrite app_assoc. exact extended.
Qed.

Lemma self_extension_is_empty : forall (start rest : list nat),
  start = start ++ rest -> rest = [].
Proof.
  intros start rest extended.
  assert (padded : start ++ [] = start ++ rest).
  { rewrite app_nil_r. exact extended. }
  apply app_inv_head in padded. symmetry. exact padded.
Qed.

Lemma extension_is_not_empty : forall (key : list nat) byte,
  key ++ [byte] <> [].
Proof.
  intros key byte empty. apply app_eq_nil in empty.
  destruct empty as [_ empty]. discriminate empty.
Qed.

Lemma extension_is_not_a_prefix : forall (key : list nat) byte,
  ~ prefix (key ++ [byte]) key.
Proof.
  intros key byte [more extended]. rewrite <- app_assoc in extended.
  apply self_extension_is_empty in extended. discriminate extended.
Qed.

(** Two equal concatenations split at the shorter first part. *)
Lemma app_split_cases : forall (first1 first2 rest1 rest2 : list nat),
  first1 ++ rest1 = first2 ++ rest2 ->
  (exists middle, first2 = first1 ++ middle /\ rest1 = middle ++ rest2) \/
  (exists middle, first1 = first2 ++ middle /\ rest2 = middle ++ rest1).
Proof.
  induction first1 as [|byte first1 IH]; intros first2 rest1 rest2 equal.
  - left. exists first2. split; [reflexivity | exact equal].
  - destruct first2 as [|byte2 first2].
    + right. exists (byte :: first1). split; [reflexivity|].
      symmetry. exact equal.
    + simpl in equal. injection equal as same tail. subst byte2.
      destruct (IH first2 rest1 rest2 tail)
        as [[middle [longer split_rest]]|[middle [longer split_rest]]].
      * left. exists middle. split; [|exact split_rest].
        simpl. f_equal. exact longer.
      * right. exists middle. split; [|exact split_rest].
        simpl. f_equal. exact longer.
Qed.

Lemma prefix_of_one_byte_extension : forall path key byte,
  prefix path (key ++ [byte]) <-> prefix path key \/ path = key ++ [byte].
Proof.
  intros path key byte. split.
  - intros [more extended].
    destruct more as [|first more_rest].
    + right. rewrite app_nil_r in extended. symmetry. exact extended.
    + left.
      assert (nonempty : first :: more_rest <> []) by discriminate.
      destruct (exists_last nonempty) as [front [last_byte split_more]].
      rewrite split_more in extended. rewrite app_assoc in extended.
      apply app_inj_tail in extended. destruct extended as [extended _].
      exists front. exact extended.
  - intros [[more extended]|same].
    + exists (more ++ [byte]). rewrite extended. rewrite <- app_assoc.
      reflexivity.
    + subst path. apply prefix_reflexive.
Qed.

(** [starts_with key start] is Rust's [key.starts_with(start)]. *)
Fixpoint starts_with (key start : list nat) {struct start} : bool :=
  match start, key with
  | [], _ => true
  | _ :: _, [] => false
  | expected :: start_rest, byte :: key_rest =>
      Nat.eqb expected byte && starts_with key_rest start_rest
  end.

Lemma starts_with_spec : forall start key,
  starts_with key start = true <-> prefix start key.
Proof.
  induction start as [|expected start_rest IH]; intros key.
  - simpl. split; [intros _; exists key; reflexivity | reflexivity].
  - destruct key as [|byte key_rest]; simpl.
    + split; [discriminate|]. intros [rest extended]. discriminate extended.
    + split.
      * intros matched. apply andb_true_iff in matched.
        destruct matched as [same tail]. apply Nat.eqb_eq in same.
        apply IH in tail. destruct tail as [rest tail]. subst.
        exists rest. reflexivity.
      * intros [rest extended]. simpl in extended.
        injection extended as same tail. subst byte key_rest.
        apply andb_true_iff. split; [apply Nat.eqb_refl|].
        apply IH. exists rest. reflexivity.
Qed.

Lemma starts_with_false : forall start key,
  ~ prefix start key -> starts_with key start = false.
Proof.
  intros start key off.
  destruct (starts_with key start) eqn:matched; [|reflexivity].
  apply starts_with_spec in matched. contradiction.
Qed.

Lemma starts_with_extension : forall start rest,
  starts_with (start ++ rest) start = true.
Proof. intros start rest. apply starts_with_spec. exists rest. reflexivity. Qed.

Fixpoint key_eqb (left right : list nat) {struct left} : bool :=
  match left, right with
  | [], [] => true
  | left_byte :: left_rest, right_byte :: right_rest =>
      Nat.eqb left_byte right_byte && key_eqb left_rest right_rest
  | _, _ => false
  end.

Lemma key_eqb_spec : forall left right,
  key_eqb left right = true <-> left = right.
Proof.
  induction left as [|left_byte left_rest IH];
    intros [|right_byte right_rest]; simpl.
  - split; reflexivity.
  - split; discriminate.
  - split; discriminate.
  - split.
    + intros matched. apply andb_true_iff in matched.
      destruct matched as [same tail]. apply Nat.eqb_eq in same.
      apply IH in tail. subst. reflexivity.
    + intros matched. injection matched as same tail. subst.
      apply andb_true_iff. split; [apply Nat.eqb_refl|].
      apply IH. reflexivity.
Qed.

Lemma key_eqb_refl : forall key, key_eqb key key = true.
Proof. intro key. apply key_eqb_spec. reflexivity. Qed.

Lemma key_eqb_false : forall left right,
  left <> right -> key_eqb left right = false.
Proof.
  intros left right different.
  destruct (key_eqb left right) eqn:matched; [|reflexivity].
  apply key_eqb_spec in matched. contradiction.
Qed.

Definition key_is_empty (key : list nat) : bool :=
  match key with
  | [] => true
  | _ :: _ => false
  end.

Lemma nonempty_key_is_not_empty : forall key,
  key <> [] -> key_is_empty key = false.
Proof.
  intros [|byte rest] nonempty; [|reflexivity].
  exfalso. apply nonempty. reflexivity.
Qed.

Definition same_first_byte (left right : list nat) : bool :=
  match left, right with
  | left_byte :: _, right_byte :: _ => Nat.eqb left_byte right_byte
  | _, _ => false
  end.

Lemma other_first_byte : forall byte key,
  ~ prefix [byte] key -> same_first_byte [byte] key = false.
Proof.
  intros byte [|first rest] off; [reflexivity|]. simpl.
  apply Nat.eqb_neq. intro same. subst first. apply off.
  exists rest. reflexivity.
Qed.

Lemma firstn_extension : forall (key suffix : list nat),
  firstn (length key) (key ++ suffix) = key.
Proof.
  induction key as [|byte key IH]; intros suffix; simpl;
    [reflexivity | f_equal; apply IH].
Qed.

(** ** Nodes

    [EmptyNode] is the sentinel of [trie_node.rs::new_empty]: it is not
    allocated and has no destructor.  A line-list node has two slots, each
    unused or holding a key with a value or a child pointer.  A dense node
    has one cell per byte; the cells are listed by position, the head being
    byte 0, so that a byte has at most one cell by construction.
    [UnmodelledNode] stands for the tiny reference and bridge nodes, whose
    layout this model does not describe; premise P2 excludes it. *)
Inductive node : Type :=
| EmptyNode : node
| LineListNode : slot -> slot -> node
| DenseByteNode : cells -> node
| UnmodelledNode : node
with slot : Type :=
| UnusedSlot : slot
| ValueSlot : list nat -> payload -> slot
| ChildSlot : list nat -> node -> slot
with cells : Type :=
| NoCells : cells
| VacantCell : cells -> cells
| LeafCell : option payload -> cells -> cells
| BranchCell : option payload -> node -> cells -> cells.

Scheme node_induction := Induction for node Sort Prop
with slot_induction := Induction for slot Sort Prop
with cells_induction := Induction for cells Sort Prop.
Combined Scheme node_slot_cells_induction
  from node_induction, slot_induction, cells_induction.

Definition slot_key (subject : slot) : list nat :=
  match subject with
  | UnusedSlot => []
  | ValueSlot key _ | ChildSlot key _ => key
  end.

Definition slot_is_used (subject : slot) : bool :=
  match subject with
  | UnusedSlot => false
  | ValueSlot _ _ | ChildSlot _ _ => true
  end.

Definition slot_is_child (subject : slot) : bool :=
  match subject with
  | ChildSlot _ _ => true
  | UnusedSlot | ValueSlot _ _ => false
  end.

(** The cell of one byte: absent, or present with an optional value and an
    optional child pointer.  A present cell with neither is a dangling byte. *)
Inductive dense_cell : Type :=
| AbsentCell : dense_cell
| PresentCell : option payload -> option node -> dense_cell.

Definition head_cell (dense : cells) : dense_cell :=
  match dense with
  | NoCells | VacantCell _ => AbsentCell
  | LeafCell stored _ => PresentCell stored None
  | BranchCell stored child _ => PresentCell stored (Some child)
  end.

Definition higher_cells (dense : cells) : cells :=
  match dense with
  | NoCells => NoCells
  | VacantCell higher | LeafCell _ higher | BranchCell _ _ higher => higher
  end.

Fixpoint cell_at (dense : cells) (byte : nat) {struct byte} : dense_cell :=
  match byte with
  | 0 => head_cell dense
  | S lower => cell_at (higher_cells dense) lower
  end.

Definition with_head_cell (head : dense_cell) (higher : cells) : cells :=
  match head with
  | AbsentCell => VacantCell higher
  | PresentCell stored None => LeafCell stored higher
  | PresentCell stored (Some child) => BranchCell stored child higher
  end.

Fixpoint set_cell (dense : cells) (byte : nat) (replacement : dense_cell)
    {struct byte} : cells :=
  match byte with
  | 0 => with_head_cell replacement (higher_cells dense)
  | S lower =>
      with_head_cell (head_cell dense)
        (set_cell (higher_cells dense) lower replacement)
  end.

Lemma head_of_with_head_cell : forall head higher,
  head_cell (with_head_cell head higher) = head.
Proof. intros [|stored [child|]] higher; reflexivity. Qed.

Lemma higher_of_with_head_cell : forall head higher,
  higher_cells (with_head_cell head higher) = higher.
Proof. intros [|stored [child|]] higher; reflexivity. Qed.

Lemma cell_at_no_cells : forall byte, cell_at NoCells byte = AbsentCell.
Proof. induction byte as [|lower IH]; [reflexivity | exact IH]. Qed.

Lemma cell_at_set_cell_same : forall byte dense replacement,
  cell_at (set_cell dense byte replacement) byte = replacement.
Proof.
  induction byte as [|lower IH]; intros dense replacement;
    cbn [cell_at set_cell].
  - apply head_of_with_head_cell.
  - rewrite higher_of_with_head_cell. apply IH.
Qed.

Lemma cell_at_set_cell_other : forall byte other dense replacement,
  other <> byte ->
  cell_at (set_cell dense byte replacement) other = cell_at dense other.
Proof.
  induction byte as [|lower IH]; intros [|other_lower] dense replacement
    different; cbn [cell_at set_cell].
  - exfalso. apply different. reflexivity.
  - rewrite higher_of_with_head_cell. reflexivity.
  - apply head_of_with_head_cell.
  - rewrite higher_of_with_head_cell. apply IH. intro same.
    apply different. f_equal. exact same.
Qed.

(** ** Paths and values

    [node_has_path subject path]: the path, read from the node's own root,
    exists.  A value slot provides the prefixes of its key; a child slot
    provides the prefixes of its key and its key followed by a path of the
    child; a present cell provides its byte, and with a child pointer the
    byte followed by a path of the child. *)
Fixpoint node_has_path (subject : node) (path : list nat) {struct subject}
    : Prop :=
  path = [] \/
  match subject with
  | EmptyNode => False
  | LineListNode slot0 slot1 =>
      slot_has_path slot0 path \/ slot_has_path slot1 path
  | DenseByteNode dense => cells_have_path dense path
  | UnmodelledNode => False
  end
with slot_has_path (subject : slot) (path : list nat) {struct subject}
    : Prop :=
  match subject with
  | UnusedSlot => False
  | ValueSlot key _ => prefix path key
  | ChildSlot key child =>
      prefix path key \/
      exists rest, path = key ++ rest /\ node_has_path child rest
  end
with cells_have_path (subject : cells) (path : list nat) {struct subject}
    : Prop :=
  match subject with
  | NoCells => False
  | VacantCell higher =>
      match path with
      | S lower :: rest => cells_have_path higher (lower :: rest)
      | _ => False
      end
  | LeafCell _ higher =>
      match path with
      | [] => False
      | 0 :: rest => rest = []
      | S lower :: rest => cells_have_path higher (lower :: rest)
      end
  | BranchCell _ child higher =>
      match path with
      | [] => False
      | 0 :: rest => node_has_path child rest
      | S lower :: rest => cells_have_path higher (lower :: rest)
      end
  end.

(** [node_holds_value subject path value]: the value is read only from the
    slot or the cell whose key ends at the path: a value slot whose key is
    the path, or the value of the cell of a one-byte path.  Below a child
    pointer the remainder of the path must not be empty.  Hence
    [node_holds_value child [] value] is a value hidden at the root of the
    node below a pointer, which no parent reads; premise P5 states that
    there is none. *)
Fixpoint node_holds_value (subject : node) (path : list nat)
    (value : payload) {struct subject} : Prop :=
  match subject with
  | EmptyNode => False
  | LineListNode slot0 slot1 =>
      slot_holds_value slot0 path value \/ slot_holds_value slot1 path value
  | DenseByteNode dense => cells_hold_value dense path value
  | UnmodelledNode => False
  end
with slot_holds_value (subject : slot) (path : list nat) (value : payload)
    {struct subject} : Prop :=
  match subject with
  | UnusedSlot => False
  | ValueSlot key stored => key = path /\ stored = value
  | ChildSlot key child =>
      exists rest,
        path = key ++ rest /\ rest <> [] /\ node_holds_value child rest value
  end
with cells_hold_value (subject : cells) (path : list nat) (value : payload)
    {struct subject} : Prop :=
  match subject with
  | NoCells => False
  | VacantCell higher =>
      match path with
      | S lower :: rest => cells_hold_value higher (lower :: rest) value
      | _ => False
      end
  | LeafCell stored higher =>
      match path with
      | [] => False
      | 0 :: rest => rest = [] /\ stored = Some value
      | S lower :: rest => cells_hold_value higher (lower :: rest) value
      end
  | BranchCell stored child higher =>
      match path with
      | [] => False
      | 0 :: rest =>
          (rest = [] /\ stored = Some value) \/
          (rest <> [] /\ node_holds_value child rest value)
      | S lower :: rest => cells_hold_value higher (lower :: rest) value
      end
  end.

Definition cell_has_path (cell : dense_cell) (rest : list nat) : Prop :=
  match cell with
  | AbsentCell => False
  | PresentCell _ None => rest = []
  | PresentCell _ (Some child) => node_has_path child rest
  end.

Definition cell_holds_value (cell : dense_cell) (rest : list nat)
    (value : payload) : Prop :=
  match cell with
  | AbsentCell => False
  | PresentCell stored None => rest = [] /\ stored = Some value
  | PresentCell stored (Some child) =>
      (rest = [] /\ stored = Some value) \/
      (rest <> [] /\ node_holds_value child rest value)
  end.

Lemma root_path_exists : forall subject, node_has_path subject [].
Proof. intros [|slot0 slot1|dense|]; left; reflexivity. Qed.

Lemma cells_have_no_root_path : forall dense, ~ cells_have_path dense [].
Proof.
  intros [|higher|stored higher|stored child higher] provided; exact provided.
Qed.

Lemma cells_hold_no_root_value : forall dense value,
  ~ cells_hold_value dense [] value.
Proof.
  intros [|higher|stored higher|stored child higher] value held; exact held.
Qed.

Lemma cells_have_path_at : forall dense byte rest,
  cells_have_path dense (byte :: rest) <->
  cell_has_path (cell_at dense byte) rest.
Proof.
  induction dense as [|higher IH|stored higher IH|stored child higher IH];
    intros [|lower] rest; cbn.
  - tauto.
  - rewrite cell_at_no_cells. cbn. tauto.
  - tauto.
  - apply IH.
  - tauto.
  - apply IH.
  - tauto.
  - apply IH.
Qed.

Lemma cells_hold_value_at : forall dense byte rest value,
  cells_hold_value dense (byte :: rest) value <->
  cell_holds_value (cell_at dense byte) rest value.
Proof.
  induction dense as [|higher IH|stored higher IH|stored child higher IH];
    intros [|lower] rest value; cbn.
  - tauto.
  - rewrite cell_at_no_cells. cbn. tauto.
  - tauto.
  - apply IH.
  - tauto.
  - apply IH.
  - tauto.
  - apply IH.
Qed.

Lemma paths_are_closed_under_prefixes :
  (forall subject path rest,
     node_has_path subject (path ++ rest) -> node_has_path subject path) /\
  (forall subject path rest,
     slot_has_path subject (path ++ rest) -> slot_has_path subject path) /\
  (forall subject path rest,
     cells_have_path subject (path ++ rest) -> path <> [] ->
     cells_have_path subject path).
Proof.
  apply node_slot_cells_induction.
  - intros path rest [empty|[]]. left. apply app_eq_nil in empty.
    exact (proj1 empty).
  - intros slot0 IH0 slot1 IH1 path rest [empty|[provided|provided]].
    + left. apply app_eq_nil in empty. exact (proj1 empty).
    + right. left. exact (IH0 path rest provided).
    + right. right. exact (IH1 path rest provided).
  - intros dense IH path rest [empty|provided].
    + left. apply app_eq_nil in empty. exact (proj1 empty).
    + destruct path as [|byte path_rest]; [left; reflexivity|]. right.
      apply (IH (byte :: path_rest) rest provided). discriminate.
  - intros path rest [empty|[]]. left. apply app_eq_nil in empty.
    exact (proj1 empty).
  - intros path rest [].
  - intros key stored path rest provided.
    exact (prefix_of_extension path rest key provided).
  - intros key child IH path rest [under|[below [split_here child_path]]].
    + left. exact (prefix_of_extension path rest key under).
    + destruct (app_split_cases path key rest below split_here)
        as [[middle [key_longer _]]|[middle [path_longer below_split]]].
      * left. exists middle. exact key_longer.
      * right. exists middle. split; [exact path_longer|].
        apply (IH middle rest). rewrite <- below_split. exact child_path.
  - intros path rest [].
  - intros higher IH [|byte path_rest] rest provided nonempty.
    + exfalso. apply nonempty. reflexivity.
    + simpl in *. destruct byte as [|lower]; [contradiction|].
      apply (IH (lower :: path_rest) rest provided). discriminate.
  - intros stored higher IH [|byte path_rest] rest provided nonempty.
    + exfalso. apply nonempty. reflexivity.
    + simpl in *. destruct byte as [|lower].
      * apply app_eq_nil in provided. exact (proj1 provided).
      * apply (IH (lower :: path_rest) rest provided). discriminate.
  - intros stored child IH_child higher IH [|byte path_rest] rest provided
      nonempty.
    + exfalso. apply nonempty. reflexivity.
    + simpl in *. destruct byte as [|lower].
      * exact (IH_child path_rest rest provided).
      * apply (IH (lower :: path_rest) rest provided). discriminate.
Qed.

Lemma a_value_lies_on_a_path :
  (forall subject path value,
     node_holds_value subject path value -> node_has_path subject path) /\
  (forall subject path value,
     slot_holds_value subject path value -> slot_has_path subject path) /\
  (forall subject path value,
     cells_hold_value subject path value -> cells_have_path subject path).
Proof.
  apply node_slot_cells_induction.
  - intros path value [].
  - intros slot0 IH0 slot1 IH1 path value [held|held]; right.
    + left. exact (IH0 path value held).
    + right. exact (IH1 path value held).
  - intros dense IH path value held. right. exact (IH path value held).
  - intros path value [].
  - intros path value [].
  - intros key stored path value [same _]. subst path.
    apply prefix_reflexive.
  - intros key child IH path value [rest [split_here [_ held]]]. right.
    exists rest. split; [exact split_here | exact (IH rest value held)].
  - intros path value [].
  - intros higher IH [|[|lower] path_rest] value held; simpl in *;
      try contradiction.
    exact (IH (lower :: path_rest) value held).
  - intros stored higher IH [|[|lower] path_rest] value held; simpl in *.
    + contradiction.
    + exact (proj1 held).
    + exact (IH (lower :: path_rest) value held).
  - intros stored child IH_child higher IH [|[|lower] path_rest] value held;
      simpl in *.
    + contradiction.
    + destruct held as [[empty _]|[_ held]].
      * subst path_rest. apply root_path_exists.
      * exact (IH_child path_rest value held).
    + exact (IH (lower :: path_rest) value held).
Qed.

(** ** Emptiness and destructor nesting *)

Fixpoint cells_are_vacant (dense : cells) : bool :=
  match dense with
  | NoCells => true
  | VacantCell higher => cells_are_vacant higher
  | LeafCell _ _ | BranchCell _ _ _ => false
  end.

(** [node_is_empty] as the sources compute it: the sentinel is empty
    ([empty_node.rs]), a line-list node is empty when slot 0 is unused
    ([line_list_node.rs]), a dense node when it has no cell
    ([dense_byte_node.rs]). *)
Definition node_is_empty (subject : node) : bool :=
  match subject with
  | EmptyNode => true
  | LineListNode slot0 _ => negb (slot_is_used slot0)
  | DenseByteNode dense => cells_are_vacant dense
  | UnmodelledNode => false
  end.

(** [node_drop_nesting subject]: the number of node destructors that can be
    active at the same time while the node is dropped.  The sentinel has no
    destructor.  Every other node counts one more than its deepest child,
    because the destructors that are active together belong to the nodes of
    one chain of child pointers.  It is an upper bound of the study's
    nesting depth D. *)
Fixpoint node_drop_nesting (subject : node) {struct subject} : nat :=
  match subject with
  | EmptyNode => 0
  | LineListNode slot0 slot1 =>
      S (Nat.max (slot_drop_nesting slot0) (slot_drop_nesting slot1))
  | DenseByteNode dense => S (cells_drop_nesting dense)
  | UnmodelledNode => 1
  end
with slot_drop_nesting (subject : slot) {struct subject} : nat :=
  match subject with
  | UnusedSlot | ValueSlot _ _ => 0
  | ChildSlot _ child => node_drop_nesting child
  end
with cells_drop_nesting (subject : cells) {struct subject} : nat :=
  match subject with
  | NoCells => 0
  | VacantCell higher | LeafCell _ higher => cells_drop_nesting higher
  | BranchCell _ child higher =>
      Nat.max (node_drop_nesting child) (cells_drop_nesting higher)
  end.

Lemma vacant_cells_are_absent : forall dense,
  cells_are_vacant dense = true ->
  forall byte, cell_at dense byte = AbsentCell.
Proof.
  induction dense as [|higher IH|stored higher IH|stored child higher IH];
    intros vacant byte; simpl in vacant.
  - apply cell_at_no_cells.
  - destruct byte as [|lower]; [reflexivity|]. exact (IH vacant lower).
  - discriminate vacant.
  - discriminate vacant.
Qed.

Lemma absent_cells_are_vacant : forall dense,
  (forall byte, cell_at dense byte = AbsentCell) ->
  cells_are_vacant dense = true.
Proof.
  induction dense as [|higher IH|stored higher IH|stored child higher IH];
    intros absent; simpl.
  - reflexivity.
  - apply IH. intro byte. exact (absent (S byte)).
  - specialize (absent 0). simpl in absent. discriminate absent.
  - specialize (absent 0). simpl in absent. discriminate absent.
Qed.

Lemma cells_drop_nesting_bound : forall bound dense,
  (forall byte stored child,
     cell_at dense byte = PresentCell stored (Some child) ->
     node_drop_nesting child <= bound) ->
  cells_drop_nesting dense <= bound.
Proof.
  intros bound dense.
  induction dense as [|higher IH|held higher IH|held below higher IH];
    intros shallow; simpl.
  - apply Nat.le_0_l.
  - apply IH. intros byte stored child at_byte.
    exact (shallow (S byte) stored child at_byte).
  - apply IH. intros byte stored child at_byte.
    exact (shallow (S byte) stored child at_byte).
  - apply Nat.max_lub.
    + exact (shallow 0 held below eq_refl).
    + apply IH. intros byte stored child at_byte.
      exact (shallow (S byte) stored child at_byte).
Qed.

(** ** Node well-formedness

    The premises P2, P3 and P5 of the study speak about single nodes.  The
    proofs of its lemmas apply them to the focus node and to the nodes X and
    Y below it, so this module states them for every node of the focus
    node's subtree: [subtree_forall property subject] says that the property
    holds for the node and for every node below it. *)
Fixpoint subtree_forall (property : node -> Prop) (subject : node)
    {struct subject} : Prop :=
  property subject /\
  match subject with
  | EmptyNode | UnmodelledNode => True
  | LineListNode slot0 slot1 =>
      slot_subtree_forall property slot0 /\
      slot_subtree_forall property slot1
  | DenseByteNode dense => cells_subtree_forall property dense
  end
with slot_subtree_forall (property : node -> Prop) (subject : slot)
    {struct subject} : Prop :=
  match subject with
  | UnusedSlot | ValueSlot _ _ => True
  | ChildSlot _ child => subtree_forall property child
  end
with cells_subtree_forall (property : node -> Prop) (subject : cells)
    {struct subject} : Prop :=
  match subject with
  | NoCells => True
  | VacantCell higher | LeafCell _ higher =>
      cells_subtree_forall property higher
  | BranchCell _ child higher =>
      subtree_forall property child /\ cells_subtree_forall property higher
  end.

(** P2: the node is a line-list node, a dense node or the empty node. *)
Definition node_kind_is_modelled (subject : node) : Prop :=
  subject <> UnmodelledNode.

(** P3: the six clauses of [line_list_node.rs::validate_node] that the study
    lists, over the keys of the two slots (an unused slot has the empty
    key); the first clause is stated once per slot.

    - a used slot has a non-empty key;
    - slot 0 has no child pointer when the key of slot 1 strictly extends
      the key of slot 0;
    - the key of a used slot 1 is never a proper prefix of the key of
      slot 0;
    - two child pointers never share a key;
    - with slot 1 used, two keys that share their first byte are related by
      prefix;
    - the two keys never share more than one leading byte. *)
Definition line_list_slots_are_valid (slot0 slot1 : slot) : Prop :=
  (slot_is_used slot0 = true -> slot_key slot0 <> []) /\
  (slot_is_used slot1 = true -> slot_key slot1 <> []) /\
  (slot_is_child slot0 = true ->
   ~ proper_prefix (slot_key slot0) (slot_key slot1)) /\
  (slot_is_used slot1 = true ->
   ~ proper_prefix (slot_key slot1) (slot_key slot0)) /\
  (slot_is_child slot0 = true -> slot_is_child slot1 = true ->
   slot_key slot0 <> slot_key slot1) /\
  (slot_is_used slot1 = true ->
   forall byte rest0 rest1,
     slot_key slot0 = byte :: rest0 -> slot_key slot1 = byte :: rest1 ->
     prefix (slot_key slot0) (slot_key slot1) \/
     prefix (slot_key slot1) (slot_key slot0)) /\
  (forall first second rest0 rest1,
     slot_key slot0 = first :: second :: rest0 ->
     slot_key slot1 = first :: second :: rest1 -> False).

Definition line_list_node_is_valid (subject : node) : Prop :=
  match subject with
  | LineListNode slot0 slot1 => line_list_slots_are_valid slot0 slot1
  | EmptyNode | DenseByteNode _ | UnmodelledNode => True
  end.

(** P5: a value at the end of a path is stored in the slot or cell whose key
    ends there, never inside the node below it: no node holds a value at
    its own root. *)
Definition node_holds_no_root_value (subject : node) : Prop :=
  forall value, ~ node_holds_value subject [] value.

Lemma subtree_forall_here : forall property subject,
  subtree_forall property subject -> property subject.
Proof. intros property [|slot0 slot1|dense|] [here _]; exact here. Qed.

Lemma cells_subtree_forall_at : forall property dense byte stored child,
  cells_subtree_forall property dense ->
  cell_at dense byte = PresentCell stored (Some child) ->
  subtree_forall property child.
Proof.
  intros property dense.
  induction dense as [|higher IH|held higher IH|held below higher IH];
    intros byte stored child hereditary at_byte.
  - rewrite cell_at_no_cells in at_byte. discriminate at_byte.
  - destruct byte as [|lower]; [simpl in at_byte; discriminate at_byte|].
    exact (IH lower stored child hereditary at_byte).
  - destruct byte as [|lower]; [simpl in at_byte; discriminate at_byte|].
    exact (IH lower stored child hereditary at_byte).
  - destruct hereditary as [here above].
    destruct byte as [|lower].
    + simpl in at_byte. injection at_byte as same_stored same_child.
      subst below. exact here.
    + exact (IH lower stored child above at_byte).
Qed.

(** In this model a line-list node can hold a value at its own root only in
    a value slot with the empty key, which the first clause of P3 excludes,
    and a dense node cannot hold one at all.  Hence P5 follows from P3 here;
    it stays a conjunct of the predicate. *)
Lemma used_slot_with_a_key_holds_no_root_value : forall subject value,
  (slot_is_used subject = true -> slot_key subject <> []) ->
  ~ slot_holds_value subject [] value.
Proof.
  intros [|key stored|key child] value key_used held; simpl in held.
  - exact held.
  - destruct held as [empty _]. exact (key_used eq_refl empty).
  - destruct held as [rest [split_here [nonempty _]]].
    symmetry in split_here. apply app_eq_nil in split_here.
    exact (nonempty (proj2 split_here)).
Qed.

Lemma valid_subtrees_hold_no_root_value :
  (forall subject,
     subtree_forall line_list_node_is_valid subject ->
     subtree_forall node_holds_no_root_value subject) /\
  (forall subject,
     slot_subtree_forall line_list_node_is_valid subject ->
     slot_subtree_forall node_holds_no_root_value subject) /\
  (forall subject,
     cells_subtree_forall line_list_node_is_valid subject ->
     cells_subtree_forall node_holds_no_root_value subject).
Proof.
  apply node_slot_cells_induction.
  - intros _. split; [intros value [] | exact I].
  - intros slot0 IH0 slot1 IH1 [valid [below0 below1]].
    destruct valid as [key0_used [key1_used _]]. split.
    + intros value [held|held].
      * exact (used_slot_with_a_key_holds_no_root_value slot0 value
                 key0_used held).
      * exact (used_slot_with_a_key_holds_no_root_value slot1 value
                 key1_used held).
    + split; [exact (IH0 below0) | exact (IH1 below1)].
  - intros dense IH [_ below]. split.
    + intros value held. exact (cells_hold_no_root_value dense value held).
    + exact (IH below).
  - intros _. split; [intros value [] | exact I].
  - intros _. exact I.
  - intros key stored _. exact I.
  - intros key child IH below. exact (IH below).
  - intros _. exact I.
  - intros higher IH below. exact (IH below).
  - intros stored higher IH below. exact (IH below).
  - intros stored child IH_child higher IH [below_child below_higher].
    split; [exact (IH_child below_child) | exact (IH below_higher)].
Qed.

(** ** The slots of a line-list node

    The lemmas of the study single out one slot of a line-list node (the
    slot that carries the cut) and speak about the other one.
    [slot_roles slot0 slot1 carrying other] says which is which. *)
Definition slot_roles (slot0 slot1 carrying other : slot) : Prop :=
  (carrying = slot0 /\ other = slot1) \/ (carrying = slot1 /\ other = slot0).

Lemma slot_roles_are_members : forall slot0 slot1 carrying other,
  slot_roles slot0 slot1 carrying other ->
  (carrying = slot0 \/ carrying = slot1) /\ (other = slot0 \/ other = slot1).
Proof.
  intros slot0 slot1 carrying other [[same0 same1]|[same1 same0]]; split;
    [left | right | right | left]; assumption.
Qed.

Lemma node_path_by_slot_roles : forall slot0 slot1 carrying other path,
  slot_roles slot0 slot1 carrying other ->
  (node_has_path (LineListNode slot0 slot1) path <->
   path = [] \/ slot_has_path carrying path \/ slot_has_path other path).
Proof.
  intros slot0 slot1 carrying other path [[same0 same1]|[same1 same0]];
    rewrite same0, same1.
  - split.
    + intros [empty|[provided|provided]];
        [left; exact empty | right; left; exact provided
        | right; right; exact provided].
    + intros [empty|[provided|provided]];
        [left; exact empty | right; left; exact provided
        | right; right; exact provided].
  - split.
    + intros [empty|[provided|provided]];
        [left; exact empty | right; right; exact provided
        | right; left; exact provided].
    + intros [empty|[provided|provided]];
        [left; exact empty | right; right; exact provided
        | right; left; exact provided].
Qed.

Lemma node_value_by_slot_roles : forall slot0 slot1 carrying other path value,
  slot_roles slot0 slot1 carrying other ->
  (node_holds_value (LineListNode slot0 slot1) path value <->
   slot_holds_value carrying path value \/ slot_holds_value other path value).
Proof.
  intros slot0 slot1 carrying other path value
    [[same0 same1]|[same1 same0]]; rewrite same0, same1.
  - split.
    + intros [held|held]; [left; exact held | right; exact held].
    + intros [held|held]; [left; exact held | right; exact held].
  - split.
    + intros [held|held]; [right; exact held | left; exact held].
    + intros [held|held]; [right; exact held | left; exact held].
Qed.

Lemma member_slot_path : forall slot0 slot1 member path,
  member = slot0 \/ member = slot1 ->
  slot_has_path member path -> node_has_path (LineListNode slot0 slot1) path.
Proof.
  intros slot0 slot1 member path [same|same] provided; rewrite <- same; right;
    [left | right]; exact provided.
Qed.

Lemma member_slot_value : forall slot0 slot1 member path value,
  member = slot0 \/ member = slot1 ->
  slot_holds_value member path value ->
  node_holds_value (LineListNode slot0 slot1) path value.
Proof.
  intros slot0 slot1 member path value [same|same] held; rewrite <- same;
    [left | right]; exact held.
Qed.

Lemma member_slot_key_is_not_empty : forall slot0 slot1 member,
  member = slot0 \/ member = slot1 ->
  line_list_slots_are_valid slot0 slot1 ->
  slot_is_used member = true -> slot_key member <> [].
Proof.
  intros slot0 slot1 member [same|same] [key0_used [key1_used _]] used;
    subst member; [exact (key0_used used) | exact (key1_used used)].
Qed.

Lemma member_slot_subtree : forall property slot0 slot1 member key child,
  member = slot0 \/ member = slot1 ->
  member = ChildSlot key child ->
  subtree_forall property (LineListNode slot0 slot1) ->
  subtree_forall property child.
Proof.
  intros property slot0 slot1 member key child [same|same] shape
    [_ [below0 below1]]; rewrite same in shape.
  - rewrite shape in below0. exact below0.
  - rewrite shape in below1. exact below1.
Qed.

Lemma slot_without_use_is_unused : forall subject,
  slot_is_used subject = false -> subject = UnusedSlot.
Proof.
  intros [|key stored|key child] unused;
    [reflexivity | discriminate unused | discriminate unused].
Qed.

Lemma slot_key_prefix_is_slot_path : forall subject path,
  slot_is_used subject = true -> prefix path (slot_key subject) ->
  slot_has_path subject path.
Proof.
  intros [|key stored|key child] path used under; simpl in *.
  - discriminate used.
  - exact under.
  - left. exact under.
Qed.

(** A path of a slot lies under the slot's key, or it passes through the
    slot's child pointer and continues in the node below. *)
Lemma slot_path_cases : forall subject path,
  slot_has_path subject path ->
  prefix path (slot_key subject) \/
  exists key child rest,
    subject = ChildSlot key child /\ path = key ++ rest /\ rest <> [] /\
    node_has_path child rest.
Proof.
  intros [|key stored|key child] path provided; simpl in provided.
  - destruct provided.
  - left. exact provided.
  - destruct provided as [under|[rest [split_here child_path]]].
    + left. exact under.
    + destruct rest as [|byte rest_tail].
      * left. rewrite app_nil_r in split_here. rewrite split_here.
        apply prefix_reflexive.
      * right. exists key, child, (byte :: rest_tail).
        split; [reflexivity|]. split; [exact split_here|].
        split; [discriminate | exact child_path].
Qed.

(** ** Pathless nodes and nodes that hold one dangling byte *)

Definition node_is_pathless (subject : node) : Prop :=
  forall path, node_has_path subject path -> path = [].

(** The node's paths are its root and the one byte, and it holds no value:
    its only path is the dangling byte. *)
Definition node_holds_only_dangling_byte (subject : node) (byte : nat)
    : Prop :=
  (forall path, node_has_path subject path <-> path = [] \/ path = [byte]) /\
  (forall path value, ~ node_holds_value subject path value).

(** A node of a modelled kind that satisfies P3 and has no path below its
    root is empty as the sources compute it, and dropping it runs at most
    its own destructor. *)
Lemma pathless_node_is_empty_and_shallow : forall subject,
  node_kind_is_modelled subject ->
  line_list_node_is_valid subject ->
  node_is_pathless subject ->
  node_is_empty subject = true /\ node_drop_nesting subject <= 1.
Proof.
  intros [|slot0 slot1|dense|] modelled valid pathless.
  - split; [reflexivity | apply Nat.le_0_l].
  - destruct valid as [key0_used [key1_used _]].
    assert (unused0 : slot_is_used slot0 = false).
    { apply not_true_is_false. intro used.
      apply (key0_used used). apply (pathless (slot_key slot0)).
      right. left.
      apply slot_key_prefix_is_slot_path;
        [exact used | apply prefix_reflexive]. }
    assert (unused1 : slot_is_used slot1 = false).
    { apply not_true_is_false. intro used.
      apply (key1_used used). apply (pathless (slot_key slot1)).
      right. right.
      apply slot_key_prefix_is_slot_path;
        [exact used | apply prefix_reflexive]. }
    apply slot_without_use_is_unused in unused0.
    apply slot_without_use_is_unused in unused1.
    rewrite unused0, unused1.
    split; [reflexivity | apply le_n].
  - assert (absent : forall byte, cell_at dense byte = AbsentCell).
    { intro byte.
      destruct (cell_at dense byte) as [|stored pointer] eqn:at_byte;
        [reflexivity|]. exfalso.
      assert (provided : node_has_path (DenseByteNode dense) [byte]).
      { right. apply cells_have_path_at. rewrite at_byte.
        destruct pointer as [child|];
          [exact (root_path_exists child) | exact eq_refl]. }
      pose proof (pathless [byte] provided) as impossible.
      discriminate impossible. }
    split.
    + simpl. apply absent_cells_are_vacant. exact absent.
    + simpl. apply le_n_S. apply cells_drop_nesting_bound.
      intros byte stored child at_byte. rewrite absent in at_byte.
      discriminate at_byte.
  - exfalso. apply modelled. reflexivity.
Qed.

Lemma pathless_node_is_empty : forall subject,
  node_kind_is_modelled subject ->
  line_list_node_is_valid subject ->
  node_is_pathless subject ->
  node_is_empty subject = true.
Proof.
  intros subject modelled valid pathless.
  exact (proj1 (pathless_node_is_empty_and_shallow subject modelled valid
                  pathless)).
Qed.

Lemma pathless_node_is_shallow : forall subject,
  node_kind_is_modelled subject ->
  line_list_node_is_valid subject ->
  node_is_pathless subject ->
  node_drop_nesting subject <= 1.
Proof.
  intros subject modelled valid pathless.
  exact (proj2 (pathless_node_is_empty_and_shallow subject modelled valid
                  pathless)).
Qed.

Lemma extension_within_one_byte : forall (key rest : list nat) byte,
  key <> [] -> key ++ rest = [] \/ key ++ rest = [byte] -> rest = [].
Proof.
  intros [|first key_rest] rest byte nonempty [empty|single].
  - exfalso. apply nonempty. reflexivity.
  - exfalso. apply nonempty. reflexivity.
  - discriminate empty.
  - simpl in single. injection single as same tail.
    apply app_eq_nil in tail. exact (proj2 tail).
Qed.

(** A node whose only path is one dangling byte has at most one empty node
    below it: dropping it nests at most two destructors. *)
Lemma dangling_byte_node_is_shallow : forall subject byte,
  subtree_forall node_kind_is_modelled subject ->
  subtree_forall line_list_node_is_valid subject ->
  node_holds_only_dangling_byte subject byte ->
  node_drop_nesting subject <= 2.
Proof.
  intros [|slot0 slot1|dense|] byte modelled valid [paths _].
  - apply Nat.le_0_l.
  - assert (shallow : forall member,
              member = slot0 \/ member = slot1 ->
              slot_drop_nesting member <= 1).
    { intros [|key stored|key child] is_member.
      - apply Nat.le_0_l.
      - apply Nat.le_0_l.
      - simpl. apply pathless_node_is_shallow.
        + exact (subtree_forall_here _ _
                   (member_slot_subtree _ slot0 slot1 (ChildSlot key child)
                      key child is_member eq_refl modelled)).
        + exact (subtree_forall_here _ _
                   (member_slot_subtree _ slot0 slot1 (ChildSlot key child)
                      key child is_member eq_refl valid)).
        + intros rest child_path.
          apply (extension_within_one_byte key rest byte).
          * exact (member_slot_key_is_not_empty slot0 slot1
                     (ChildSlot key child) is_member
                     (subtree_forall_here _ _ valid) eq_refl).
          * apply paths.
            apply (member_slot_path slot0 slot1 (ChildSlot key child)
                     (key ++ rest) is_member).
            right. exists rest. split; [reflexivity | exact child_path]. }
    simpl. apply le_n_S. apply Nat.max_lub.
    + apply shallow. left. reflexivity.
    + apply shallow. right. reflexivity.
  - destruct modelled as [_ cells_modelled]. destruct valid as [_ cells_valid].
    simpl. apply le_n_S. apply cells_drop_nesting_bound.
    intros at_byte stored child present.
    apply pathless_node_is_shallow.
    + exact (subtree_forall_here _ _
               (cells_subtree_forall_at _ dense at_byte stored child
                  cells_modelled present)).
    + exact (subtree_forall_here _ _
               (cells_subtree_forall_at _ dense at_byte stored child
                  cells_valid present)).
    + intros rest child_path.
      assert (provided :
        node_has_path (DenseByteNode dense) (at_byte :: rest)).
      { right. apply cells_have_path_at. rewrite present. exact child_path. }
      apply paths in provided. destruct provided as [empty|single].
      * discriminate empty.
      * injection single as same tail. exact tail.
  - simpl. apply le_S. apply le_n.
Qed.

(** ** The transcribed calls

    What a call removes from a node is listed as detached parts, in the
    order in which the sources drop them. *)
Inductive detached_part : Type :=
| DetachedValue : payload -> detached_part
| DetachedChild : node -> detached_part.

Definition slot_payload (subject : slot) : list detached_part :=
  match subject with
  | UnusedSlot => []
  | ValueSlot _ stored => [DetachedValue stored]
  | ChildSlot _ child => [DetachedChild child]
  end.

Definition detached_part_drop_nesting (part : detached_part) : nat :=
  match part with
  | DetachedValue _ => 0
  | DetachedChild child => node_drop_nesting child
  end.

Fixpoint detached_drop_nesting (parts : list detached_part) : nat :=
  match parts with
  | [] => 0
  | part :: others =>
      Nat.max (detached_part_drop_nesting part) (detached_drop_nesting others)
  end.

Lemma detached_child_drop_nesting : forall child,
  detached_drop_nesting [DetachedChild child] = node_drop_nesting child.
Proof. intro child. simpl. apply Nat.max_0_r. Qed.

(** The result of [node_remove_all_branches]: the node afterwards, the
    returned flag, and what was detached. *)
Record node_removal : Type := NodeRemoval {
  removal_node : node;
  removal_removed : bool;
  removal_detached : list detached_part
}.

(** The two prune limits that the calls of Phase B pass: [usize::MAX] from
    [remove_branches(false)], and 0 from the climb of [prune_path] in a
    zipper rooted at the map's root.  [preserve_prune_limit] runs for
    neither, because it requires a limit above 0 and below the key
    length. *)
Inductive prune_limit : Type :=
| PruneLimitZero : prune_limit
| PruneLimitUnbounded : prune_limit.

(** The non-pruning branch of [remove_subtries]: the key is shortened to
    the length of the removal key and the payload is swapped for a pointer
    to the empty sentinel. *)
Definition cut_slot (key_length : nat) (subject : slot) : slot :=
  ChildSlot (firstn key_length (slot_key subject)) EmptyNode.

(** [remove_0] and [remove_1] of
    [line_list_node.rs::node_remove_all_branches]: the slot's key starts
    with the removal key and is longer than it or holds a child pointer. *)
Definition slot_is_removed (subject : slot) (key : list nat) : bool :=
  starts_with (slot_key subject) key &&
  (Nat.ltb (length key) (length (slot_key subject)) || slot_is_child subject).

(** [line_list_node.rs::node_remove_all_branches] with [remove_subtries]:
    slot 1 is handled first; a taken slot 0 shifts slot 1 down
    ([take_payload]). *)
Definition line_list_remove_all_branches (limit : prune_limit)
    (slot0 slot1 : slot) (key : list nat) : node_removal :=
  let key0_starts_with := starts_with (slot_key slot0) key in
  let remove_0 := slot_is_removed slot0 key in
  let remove_1 := slot_is_removed slot1 key in
  let prune :=
    match limit with
    | PruneLimitZero => negb (key_is_empty key)
    | PruneLimitUnbounded => false
    end in
  let slot1_after :=
    if remove_1 then
      if prune || key0_starts_with || key_is_empty key
      then UnusedSlot
      else cut_slot (length key) slot1
    else slot1 in
  let node_after :=
    if remove_0 then
      if prune || key_is_empty key
      then LineListNode slot1_after UnusedSlot
      else LineListNode (cut_slot (length key) slot0) slot1_after
    else LineListNode slot0 slot1_after in
  NodeRemoval node_after (remove_0 || remove_1)
    ((if remove_1 then slot_payload slot1 else []) ++
     (if remove_0 then slot_payload slot0 else [])).

(** [dense_byte_node.rs::node_remove_all_branches]: only a one-byte key
    finds a cell; a cell with a value keeps the value and loses the child
    pointer; a cell without a value is removed at limit 0 and otherwise
    keeps the cell without the child pointer. *)
Definition dense_remove_all_branches (limit : prune_limit) (dense : cells)
    (key : list nat) : node_removal :=
  match key with
  | [byte] =>
      match cell_at dense byte with
      | PresentCell (Some value) (Some child) =>
          NodeRemoval
            (DenseByteNode
               (set_cell dense byte (PresentCell (Some value) None)))
            true [DetachedChild child]
      | PresentCell None (Some child) =>
          NodeRemoval
            (DenseByteNode
               (set_cell dense byte
                  (match limit with
                   | PruneLimitZero => AbsentCell
                   | PruneLimitUnbounded => PresentCell None None
                   end)))
            true [DetachedChild child]
      | PresentCell _ None | AbsentCell =>
          NodeRemoval (DenseByteNode dense) false []
      end
  | _ => NodeRemoval (DenseByteNode dense) false []
  end.

(** The empty node returns [false] ([empty_node.rs]); the kinds that the
    model does not describe are left unchanged (P2 excludes them). *)
Definition node_remove_all_branches (limit : prune_limit) (subject : node)
    (key : list nat) : node_removal :=
  match subject with
  | LineListNode slot0 slot1 =>
      line_list_remove_all_branches limit slot0 slot1 key
  | DenseByteNode dense => dense_remove_all_branches limit dense key
  | EmptyNode | UnmodelledNode => NodeRemoval subject false []
  end.

(** The outcome of [write_zipper.rs::remove_branches(false)] at a focus:
    the focus node afterwards, the returned flag, what was detached, and
    the number of nodes the call allocates (a copy-on-write clone of a
    shared node is not counted, as in the study). *)
Record cut_outcome : Type := CutOutcome {
  cut_node : node;
  cut_removed : bool;
  cut_detached : list detached_part;
  cut_allocated_nodes : nat
}.

(** With a non-empty node key the call is [node_remove_all_branches] with
    the limit [usize::MAX] and no prune.  With an empty node key the focus
    is the zipper root: an empty root node is left alone, any other root
    node is replaced by one newly allocated empty line-list node
    ([new_allocated_in]). *)
Definition remove_branches_without_prune (subject : node)
    (node_key : list nat) : cut_outcome :=
  match node_key with
  | [] =>
      if node_is_empty subject
      then CutOutcome subject false [] 0
      else CutOutcome (LineListNode UnusedSlot UnusedSlot) true
             [DetachedChild subject] 1
  | _ :: _ =>
      let removal :=
        node_remove_all_branches PruneLimitUnbounded subject node_key in
      CutOutcome (removal_node removal) (removal_removed removal)
        (removal_detached removal) 0
  end.

(** The result of [node_remove_dangling] with keep length 0. *)
Record dangling_removal : Type := DanglingRemoval {
  dangling_node : node;
  dangling_pruned_bytes : nat;
  dangling_detached : list detached_part
}.

(** The count that the line-list call returns: the key length, or one less
    when the other slot's key starts with the same byte. *)
Definition pruned_key_bytes (key other_key : list nat) : nat :=
  if same_first_byte key other_key then length key - 1 else length key.

Definition line_list_remove_dangling_in_slot1 (slot0 slot1 : slot)
    (key : list nat) : dangling_removal :=
  match slot1 with
  | ChildSlot key1 child1 =>
      if key_eqb key1 key && node_is_empty child1
      then DanglingRemoval (LineListNode slot0 UnusedSlot)
             (pruned_key_bytes key (slot_key slot0)) [DetachedChild child1]
      else DanglingRemoval (LineListNode slot0 slot1) 0 []
  | UnusedSlot | ValueSlot _ _ =>
      DanglingRemoval (LineListNode slot0 slot1) 0 []
  end.

(** [line_list_node.rs::node_remove_dangling] with keep length 0: slot 0 is
    tried first, then slot 1; a slot is taken when it holds a child pointer
    under exactly the key and the node it points to is empty. *)
Definition line_list_remove_dangling (slot0 slot1 : slot) (key : list nat)
    : dangling_removal :=
  match slot0 with
  | ChildSlot key0 child0 =>
      if key_eqb key0 key && node_is_empty child0
      then DanglingRemoval (LineListNode slot1 UnusedSlot)
             (pruned_key_bytes key (slot_key slot1)) [DetachedChild child0]
      else line_list_remove_dangling_in_slot1 slot0 slot1 key
  | UnusedSlot | ValueSlot _ _ =>
      line_list_remove_dangling_in_slot1 slot0 slot1 key
  end.

(** [dense_byte_node.rs::node_remove_dangling] with keep length 0: only a
    one-byte key finds a cell; a cell without value and child pointer is
    removed; a cell that points to an empty node is removed when it has no
    value and otherwise only loses the pointer, which returns 0. *)
Definition dense_remove_dangling (dense : cells) (key : list nat)
    : dangling_removal :=
  match key with
  | [byte] =>
      match cell_at dense byte with
      | PresentCell None None =>
          DanglingRemoval (DenseByteNode (set_cell dense byte AbsentCell)) 1 []
      | PresentCell stored (Some child) =>
          if node_is_empty child
          then
            match stored with
            | Some _ =>
                DanglingRemoval
                  (DenseByteNode
                     (set_cell dense byte (PresentCell stored None)))
                  0 [DetachedChild child]
            | None =>
                DanglingRemoval
                  (DenseByteNode (set_cell dense byte AbsentCell))
                  1 [DetachedChild child]
            end
          else DanglingRemoval (DenseByteNode dense) 0 []
      | PresentCell (Some _) None | AbsentCell =>
          DanglingRemoval (DenseByteNode dense) 0 []
      end
  | _ => DanglingRemoval (DenseByteNode dense) 0 []
  end.

(** The sources never call [node_remove_dangling] on the empty node; the
    model leaves it and the kinds it does not describe unchanged. *)
Definition node_remove_dangling (subject : node) (key : list nat)
    : dangling_removal :=
  match subject with
  | LineListNode slot0 slot1 => line_list_remove_dangling slot0 slot1 key
  | DenseByteNode dense => dense_remove_dangling dense key
  | EmptyNode | UnmodelledNode => DanglingRemoval subject 0 []
  end.

(** The ancestors of the focus node are abstracted to one flag per path
    byte above the focus node, nearest first: whether the position reached
    by that byte holds a value or has more than one branch.  The zipper
    root has no flag; the climb always stops there.
    [climb_above_node] counts the bytes the climb of
    [prune_path_internal] ascends above the focus node. *)
Fixpoint climb_above_node (outer_stops : list bool) : nat :=
  match outer_stops with
  | [] => 0
  | true :: _ => 0
  | false :: higher => S (climb_above_node higher)
  end.

(** What [write_zipper.rs::prune_path] does at a focus: the returned byte
    count, the focus node afterwards, what was detached from the focus
    node, and whether the climb removed a branch in a node above the focus
    node (which this model does not describe further). *)
Record prune_report : Type := PruneReport {
  report_pruned_bytes : nat;
  report_node : node;
  report_detached : list detached_part;
  report_removed_above_node : bool
}.

(** P6 as a function.  [prune_path] returns 0 for an empty node key.
    Otherwise it calls [node_remove_dangling] on the focus node with the
    node key and keep length 0.  Only after a positive count it calls
    [prune_path_internal], which returns 0 unless the focus node is now
    empty.  For an emptied focus node the climb ascends the whole node key
    and then up to the first position that stops it.  When the focus node
    is the zipper's root node, the climb ends at the zipper root and calls
    [node_remove_all_branches] with limit 0 and the first path byte on that
    node.  [prune_path] returns the larger count. *)
Definition prune_path_outcome (subject : node) (node_key : list nat)
    (outer_stops : list bool) : prune_report :=
  match node_key with
  | [] => PruneReport 0 subject [] false
  | first :: _ =>
      let dangling := node_remove_dangling subject node_key in
      match dangling_pruned_bytes dangling with
      | 0 =>
          PruneReport 0 (dangling_node dangling) (dangling_detached dangling)
            false
      | S _ =>
          if node_is_empty (dangling_node dangling)
          then
            match outer_stops with
            | [] =>
                let removal :=
                  node_remove_all_branches PruneLimitZero
                    (dangling_node dangling) [first] in
                PruneReport
                  (Nat.max (dangling_pruned_bytes dangling) (length node_key))
                  (removal_node removal)
                  (dangling_detached dangling ++ removal_detached removal)
                  false
            | _ :: _ =>
                PruneReport
                  (Nat.max (dangling_pruned_bytes dangling)
                     (length node_key + climb_above_node outer_stops))
                  (dangling_node dangling) (dangling_detached dangling) true
            end
          else
            PruneReport (dangling_pruned_bytes dangling)
              (dangling_node dangling) (dangling_detached dangling) false
      end
  end.

(** ** The focus and the premises

    A write zipper's focus: the node that holds it, the node key (the part
    of the path inside that node), the stop flags of the positions above
    the node, and what [prune_path] does there. *)
Record zipper_focus : Type := ZipperFocus {
  focus_node : node;
  focus_node_key : list nat;
  focus_outer_stops : list bool;
  focus_prune_outcome : prune_report
}.

Definition focus_depth (focus : zipper_focus) : nat :=
  length (focus_outer_stops focus) + length (focus_node_key focus).

(** A child pointer of the node stored under exactly this key. *)
Definition node_has_child_pointer_at (subject : node) (key : list nat)
    : Prop :=
  match subject with
  | EmptyNode | UnmodelledNode => False
  | LineListNode slot0 slot1 =>
      (exists child, slot0 = ChildSlot key child) \/
      (exists child, slot1 = ChildSlot key child)
  | DenseByteNode dense =>
      exists byte stored child,
        key = [byte] /\ cell_at dense byte = PresentCell stored (Some child)
  end.

(** P1: the focus node is the deepest node on the path: no child pointer
    of the focus node lies under a proper prefix of the node key (the
    zipper would have descended below it), and a focus that is not the
    zipper root has a non-empty node key. *)
Definition focus_node_is_deepest (focus : zipper_focus) : Prop :=
  (forall key,
     proper_prefix key (focus_node_key focus) ->
     ~ node_has_child_pointer_at (focus_node focus) key) /\
  (focus_node_key focus = [] -> focus_outer_stops focus = []).

(** P4: when the focus is an existing path inside a dense node, its node
    key is one byte. *)
Definition dense_focus_key_is_one_byte (focus : zipper_focus) : Prop :=
  forall dense,
    focus_node focus = DenseByteNode dense ->
    focus_node_key focus <> [] ->
    node_has_path (focus_node focus) (focus_node_key focus) ->
    length (focus_node_key focus) = 1.

(** P6: [prune_path] behaves at the focus as transcribed above. *)
Definition prune_path_is_as_transcribed (focus : zipper_focus) : Prop :=
  focus_prune_outcome focus =
  prune_path_outcome (focus_node focus) (focus_node_key focus)
    (focus_outer_stops focus).

(** The premises P1 to P6 of the study, as one predicate on the modelled
    focus.  P2, P3 and P5 are stated for the subtree of the focus node.
    P7 is not a property of nodes: the constant navigation stack is a
    field of the cost record below. *)
Definition pathmap_premises (focus : zipper_focus) : Prop :=
  focus_node_is_deepest focus /\
  subtree_forall node_kind_is_modelled (focus_node focus) /\
  subtree_forall line_list_node_is_valid (focus_node focus) /\
  dense_focus_key_is_one_byte focus /\
  subtree_forall node_holds_no_root_value (focus_node focus) /\
  prune_path_is_as_transcribed focus.

Lemma focus_below_the_root_has_a_node_key : forall focus,
  focus_node_is_deepest focus -> focus_depth focus <> 0 ->
  focus_node_key focus <> [].
Proof.
  intros [subject node_key outer_stops outcome] [_ root_only] below_root
    empty.
  cbv [focus_depth focus_node_key focus_outer_stops] in *.
  apply below_root. rewrite (root_only empty). rewrite empty. reflexivity.
Qed.

(** In this model P4 follows from P1: a longer node key that exists inside
    a dense node passes through the child pointer of its first byte.  P4
    stays a conjunct of the predicate. *)
Lemma deepest_focus_in_a_dense_node_has_a_one_byte_key : forall focus,
  focus_node_is_deepest focus -> dense_focus_key_is_one_byte focus.
Proof.
  intros [subject node_key outer_stops outcome] [deepest _] dense same
    nonempty provided.
  cbv [focus_node focus_node_key] in *. subst subject.
  destruct node_key as [|first [|second more]].
  - exfalso. apply nonempty. reflexivity.
  - reflexivity.
  - exfalso.
    destruct provided as [empty|provided]; [discriminate empty|].
    apply cells_have_path_at in provided.
    revert provided.
    destruct (cell_at dense first) as [|stored [child|]] eqn:at_first;
      intro provided.
    + exact provided.
    + apply (deepest [first]).
      * exists (second :: more). split; [discriminate | reflexivity].
      * exists first, stored, child. split; [reflexivity | exact at_first].
    + simpl in provided. discriminate provided.
Qed.

(** The predicate from its four independent conjuncts: in this model P4
    follows from P1 and P5 from P3. *)
Lemma pathmap_premises_from_p1_p2_p3_p6 : forall focus,
  focus_node_is_deepest focus ->
  subtree_forall node_kind_is_modelled (focus_node focus) ->
  subtree_forall line_list_node_is_valid (focus_node focus) ->
  prune_path_is_as_transcribed focus ->
  pathmap_premises focus.
Proof.
  intros focus deepest modelled valid transcribed.
  split; [exact deepest|]. split; [exact modelled|]. split; [exact valid|].
  split.
  { exact (deepest_focus_in_a_dense_node_has_a_one_byte_key focus deepest). }
  split; [|exact transcribed].
  exact (proj1 valid_subtrees_hold_no_root_value (focus_node focus) valid).
Qed.

(** ** The hypotheses of the lemmas

    The paths of the trie below the focus are the paths of the focus node
    that extend the node key.  [single_child_parent_at] is the hypothesis
    of Lemmas 1 and 2 on the focus node: the parent P at the node key has
    no value and exactly one child byte, and its child L has no value and
    no child. *)
Definition single_child_parent_at (subject : node) (node_key : list nat)
    (child_byte : nat) : Prop :=
  (forall value, ~ node_holds_value subject node_key value) /\
  (forall byte,
     node_has_path subject (node_key ++ [byte]) <-> byte = child_byte) /\
  (forall value, ~ node_holds_value subject (node_key ++ [child_byte]) value) /\
  (forall byte, ~ node_has_path subject (node_key ++ [child_byte; byte])).

(** The focus is a non-root parent as in Lemmas 1 and 2. *)
Definition focus_is_single_child_parent (focus : zipper_focus)
    (child_byte : nat) : Prop :=
  focus_depth focus <> 0 /\
  single_child_parent_at (focus_node focus) (focus_node_key focus) child_byte.

(** ** Lemma 1: one slot carries the cut *)

Lemma key_under_the_parent_key :
  forall slot0 slot1 member node_key child_byte,
    member = slot0 \/ member = slot1 ->
    slot_is_used member = true ->
    single_child_parent_at (LineListNode slot0 slot1) node_key child_byte ->
    prefix node_key (slot_key member) ->
    slot_key member = node_key \/ slot_key member = node_key ++ [child_byte].
Proof.
  intros slot0 slot1 member node_key child_byte is_member used
    [_ [children [_ no_grandchild]]] [suffix key_shape].
  destruct suffix as [|first suffix_rest].
  - left. rewrite key_shape. apply app_nil_r.
  - right.
    assert (same : first = child_byte).
    { apply children.
      apply (member_slot_path slot0 slot1 member (node_key ++ [first])
               is_member).
      apply slot_key_prefix_is_slot_path; [exact used|].
      exists suffix_rest. rewrite key_shape. rewrite <- app_assoc.
      reflexivity. }
    subst first.
    destruct suffix_rest as [|second more]; [exact key_shape|].
    exfalso. apply (no_grandchild second).
    apply (member_slot_path slot0 slot1 member
             (node_key ++ [child_byte; second]) is_member).
    apply slot_key_prefix_is_slot_path; [exact used|].
    exists more. rewrite key_shape. rewrite <- app_assoc. reflexivity.
Qed.

(** A slot whose key starts with the parent's node key holds a child
    pointer, under the node key itself or under the node key followed by
    the child byte. *)
Lemma slot_under_the_parent_key_carries_the_child :
  forall slot0 slot1 member node_key child_byte,
    member = slot0 \/ member = slot1 ->
    node_key <> [] ->
    single_child_parent_at (LineListNode slot0 slot1) node_key child_byte ->
    prefix node_key (slot_key member) ->
    (exists child, member = ChildSlot node_key child) \/
    (exists child, member = ChildSlot (node_key ++ [child_byte]) child).
Proof.
  intros slot0 slot1 member node_key child_byte is_member nonempty parent
    under.
  assert (used : slot_is_used member = true).
  { destruct member as [|key stored|key child]; [|reflexivity|reflexivity].
    exfalso. destruct under as [suffix key_shape]. simpl in key_shape.
    symmetry in key_shape. apply app_eq_nil in key_shape.
    apply nonempty. exact (proj1 key_shape). }
  destruct (key_under_the_parent_key slot0 slot1 member node_key child_byte
              is_member used parent under) as [key_shape|key_shape];
    destruct parent as [no_value [_ [no_leaf_value _]]];
    destruct member as [|key stored|key child]; simpl in key_shape.
  - discriminate used.
  - exfalso. subst key. apply (no_value stored).
    apply (member_slot_value slot0 slot1 (ValueSlot node_key stored) node_key
             stored is_member).
    split; reflexivity.
  - left. exists child. subst key. reflexivity.
  - discriminate used.
  - exfalso. subst key. apply (no_leaf_value stored).
    apply (member_slot_value slot0 slot1
             (ValueSlot (node_key ++ [child_byte]) stored)
             (node_key ++ [child_byte]) stored is_member).
    split; reflexivity.
  - right. exists child. subst key. reflexivity.
Qed.

(** P3 excludes two slots under the parent's node key. *)
Lemma two_slots_under_the_parent_key_are_invalid :
  forall slot0 slot1 node_key child_byte,
    line_list_slots_are_valid slot0 slot1 ->
    ((exists child, slot0 = ChildSlot node_key child) \/
     (exists child, slot0 = ChildSlot (node_key ++ [child_byte]) child)) ->
    ((exists child, slot1 = ChildSlot node_key child) \/
     (exists child, slot1 = ChildSlot (node_key ++ [child_byte]) child)) ->
    False.
Proof.
  intros slot0 slot1 node_key child_byte
    [_ [_ [no_extension [no_shorter [no_shared _]]]]]
    [[child0 shape0]|[child0 shape0]] [[child1 shape1]|[child1 shape1]];
    subst slot0 slot1.
  - exact (no_shared eq_refl eq_refl eq_refl).
  - apply (no_extension eq_refl). exists [child_byte].
    split; [discriminate | reflexivity].
  - apply (no_shorter eq_refl). exists [child_byte].
    split; [discriminate | reflexivity].
  - exact (no_shared eq_refl eq_refl eq_refl).
Qed.

Lemma member_child_pointer : forall slot0 slot1 member key child,
  member = slot0 \/ member = slot1 ->
  member = ChildSlot key child ->
  node_has_child_pointer_at (LineListNode slot0 slot1) key.
Proof.
  intros slot0 slot1 member key child [same|same] shape; rewrite same in shape;
    [left | right]; exists child; exact shape.
Qed.

Lemma member_has_no_pointer_above : forall slot0 slot1 member node_key,
  member = slot0 \/ member = slot1 ->
  (forall key,
     proper_prefix key node_key ->
     ~ node_has_child_pointer_at (LineListNode slot0 slot1) key) ->
  forall key child,
    member = ChildSlot key child -> ~ proper_prefix key node_key.
Proof.
  intros slot0 slot1 member node_key is_member deepest key child shape above.
  exact (deepest key above
           (member_child_pointer slot0 slot1 member key child is_member shape)).
Qed.

(** A slot whose key does not start with the node key, and that holds no
    child pointer above the node key, has no path at or below the node
    key. *)
Lemma slot_off_the_key_has_no_path_below : forall subject node_key rest,
  ~ prefix node_key (slot_key subject) ->
  (forall key child,
     subject = ChildSlot key child -> ~ proper_prefix key node_key) ->
  ~ slot_has_path subject (node_key ++ rest).
Proof.
  intros subject node_key rest off no_pointer provided.
  destruct (slot_path_cases subject (node_key ++ rest) provided)
    as [under|[key [child [below [shape [split_here [nonempty
                                                     child_path]]]]]]].
  - apply off.
    exact (prefix_of_extension node_key rest (slot_key subject) under).
  - destruct (app_split_cases node_key key rest below split_here)
      as [[middle [key_longer _]]|[middle [key_shorter _]]].
    + apply off. rewrite shape. simpl. exists middle. exact key_longer.
    + destruct middle as [|first middle_rest].
      * apply off. rewrite shape. simpl. exists [].
        rewrite app_nil_r in key_shorter. rewrite app_nil_r. symmetry.
        exact key_shorter.
      * apply (no_pointer key child shape). exists (first :: middle_rest).
        split; [discriminate | exact key_shorter].
Qed.

Lemma two_byte_extension : forall (key : list nat) first second rest,
  (key ++ [first]) ++ second :: rest = (key ++ [first; second]) ++ rest.
Proof. intros key first second rest. rewrite <- !app_assoc. reflexivity. Qed.

(** The node below the parent, whether a line-list slot or a dense cell
    points to it, holds exactly the dangling child byte. *)
Lemma node_below_the_parent_holds_only_the_dangling_byte :
  forall subject node_key child_byte below,
    single_child_parent_at subject node_key child_byte ->
    (forall rest,
       node_has_path below rest -> node_has_path subject (node_key ++ rest)) ->
    (forall rest value,
       rest <> [] -> node_holds_value below rest value ->
       node_holds_value subject (node_key ++ rest) value) ->
    node_has_path below [child_byte] ->
    node_holds_no_root_value below ->
    node_holds_only_dangling_byte below child_byte.
Proof.
  intros subject node_key child_byte below
    [_ [children [no_leaf_value no_grandchild]]] lifts_paths lifts_values
    has_byte no_root_value.
  assert (paths : forall path,
            node_has_path below path -> path = [] \/ path = [child_byte]).
  { intros [|first [|second more]] provided.
    - left. reflexivity.
    - right. f_equal. apply children. exact (lifts_paths [first] provided).
    - exfalso.
      assert (first_two : node_has_path below [first; second]).
      { apply (proj1 paths_are_closed_under_prefixes below [first; second]
                 more).
        exact provided. }
      assert (same : first = child_byte).
      { apply children. apply lifts_paths.
        apply (proj1 paths_are_closed_under_prefixes below [first] [second]).
        exact first_two. }
      subst first.
      exact (no_grandchild second
               (lifts_paths [child_byte; second] first_two)). }
  split.
  - intro path. split; [apply paths|].
    intros [empty|single]; subst path;
      [apply root_path_exists | exact has_byte].
  - intros path value held.
    destruct path as [|first rest].
    + exact (no_root_value value held).
    + pose proof (proj1 a_value_lies_on_a_path below (first :: rest) value
                    held) as provided.
      destruct (paths (first :: rest) provided) as [empty|single];
        [discriminate empty|].
      rewrite single in held.
      apply (no_leaf_value value).
      apply (lifts_values [child_byte] value); [discriminate | exact held].
Qed.

(** The slot that provides the path to the child L is the carrying slot of
    Lemma 1, in case (i) or in case (ii). *)
Lemma member_with_the_child_path_carries_the_cut :
  forall slot0 slot1 member node_key child_byte,
    member = slot0 \/ member = slot1 ->
    node_key <> [] ->
    (forall key,
       proper_prefix key node_key ->
       ~ node_has_child_pointer_at (LineListNode slot0 slot1) key) ->
    subtree_forall node_holds_no_root_value (LineListNode slot0 slot1) ->
    single_child_parent_at (LineListNode slot0 slot1) node_key child_byte ->
    slot_has_path member (node_key ++ [child_byte]) ->
    prefix node_key (slot_key member) /\
    ((exists emptied,
        member = ChildSlot (node_key ++ [child_byte]) emptied /\
        node_is_pathless emptied) \/
     (exists below,
        member = ChildSlot node_key below /\
        node_holds_only_dangling_byte below child_byte)).
Proof.
  intros slot0 slot1 member node_key child_byte is_member nonempty deepest
    no_root_values parent provided.
  destruct (slot_path_cases member (node_key ++ [child_byte]) provided)
    as [under|[key [child [rest [shape [split_here [rest_nonempty
                                                    child_path]]]]]]].
  - assert (starts : prefix node_key (slot_key member)).
    { exact (prefix_of_extension node_key [child_byte] (slot_key member)
               under). }
    split; [exact starts|].
    destruct (slot_under_the_parent_key_carries_the_child slot0 slot1 member
                node_key child_byte is_member nonempty parent starts)
      as [[child shape]|[child shape]].
    + exfalso. rewrite shape in under. simpl in under.
      exact (extension_is_not_a_prefix node_key child_byte under).
    + left. exists child. split; [exact shape|].
      intros [|first rest] child_path; [reflexivity|]. exfalso.
      destruct parent as [_ [_ [_ no_grandchild]]].
      apply (no_grandchild first).
      apply (proj1 paths_are_closed_under_prefixes (LineListNode slot0 slot1)
               (node_key ++ [child_byte; first]) rest).
      rewrite <- two_byte_extension.
      apply (member_slot_path slot0 slot1 member
               ((node_key ++ [child_byte]) ++ first :: rest) is_member).
      rewrite shape. right. exists (first :: rest).
      split; [reflexivity | exact child_path].
  - destruct (exists_last rest_nonempty) as [front [last_byte rest_shape]].
    rewrite rest_shape in split_here. rewrite app_assoc in split_here.
    apply app_inj_tail in split_here.
    destruct split_here as [key_shape same_byte]. subst last_byte.
    destruct front as [|first front_rest].
    + rewrite app_nil_r in key_shape. subst key.
      simpl in rest_shape. subst rest.
      split.
      * rewrite shape. simpl. apply prefix_reflexive.
      * right. exists child. split; [exact shape|].
        apply (node_below_the_parent_holds_only_the_dangling_byte
                 (LineListNode slot0 slot1) node_key child_byte child parent).
        -- intros below_rest below_path.
           apply (member_slot_path slot0 slot1 member (node_key ++ below_rest)
                    is_member).
           rewrite shape. right. exists below_rest.
           split; [reflexivity | exact below_path].
        -- intros below_rest value below_nonempty held.
           apply (member_slot_value slot0 slot1 member
                    (node_key ++ below_rest) value is_member).
           rewrite shape. exists below_rest.
           split; [reflexivity|]. split; [exact below_nonempty | exact held].
        -- exact child_path.
        -- exact (subtree_forall_here _ _
                    (member_slot_subtree _ slot0 slot1 member node_key child
                       is_member shape no_root_values)).
    + exfalso. apply (deepest key).
      * exists (first :: front_rest). split; [discriminate | exact key_shape].
      * exact (member_child_pointer slot0 slot1 member key child is_member
                 shape).
Qed.

(** The slots of the focus node under the hypotheses of Lemma 1: one slot
    carries the cut, the other one does not start with the node key. *)
Definition slot_carries_the_cut (carrying : slot) (node_key : list nat)
    (child_byte : nat) : Prop :=
  (exists emptied,
     carrying = ChildSlot (node_key ++ [child_byte]) emptied /\
     node_is_pathless emptied) \/
  (exists below,
     carrying = ChildSlot node_key below /\
     node_holds_only_dangling_byte below child_byte).

Lemma line_list_cut_shape : forall slot0 slot1 node_key child_byte,
  node_key <> [] ->
  (forall key,
     proper_prefix key node_key ->
     ~ node_has_child_pointer_at (LineListNode slot0 slot1) key) ->
  subtree_forall line_list_node_is_valid (LineListNode slot0 slot1) ->
  subtree_forall node_holds_no_root_value (LineListNode slot0 slot1) ->
  single_child_parent_at (LineListNode slot0 slot1) node_key child_byte ->
  exists carrying other,
    slot_roles slot0 slot1 carrying other /\
    ~ prefix node_key (slot_key other) /\
    slot_carries_the_cut carrying node_key child_byte.
Proof.
  intros slot0 slot1 node_key child_byte nonempty deepest valid no_root_values
    parent.
  assert (provided :
    node_has_path (LineListNode slot0 slot1) (node_key ++ [child_byte])).
  { destruct parent as [_ [children _]]. apply children. reflexivity. }
  destruct provided as [empty|[provided|provided]].
  - exfalso. exact (extension_is_not_empty node_key child_byte empty).
  - destruct (member_with_the_child_path_carries_the_cut slot0 slot1 slot0
                node_key child_byte (or_introl eq_refl) nonempty deepest
                no_root_values parent provided) as [starts carries].
    exists slot0, slot1. split; [left; split; reflexivity|].
    split; [|exact carries].
    intro other_starts.
    apply (two_slots_under_the_parent_key_are_invalid slot0 slot1 node_key
             child_byte (subtree_forall_here _ _ valid)).
    + exact (slot_under_the_parent_key_carries_the_child slot0 slot1 slot0
               node_key child_byte (or_introl eq_refl) nonempty parent starts).
    + exact (slot_under_the_parent_key_carries_the_child slot0 slot1 slot1
               node_key child_byte (or_intror eq_refl) nonempty parent
               other_starts).
  - destruct (member_with_the_child_path_carries_the_cut slot0 slot1 slot1
                node_key child_byte (or_intror eq_refl) nonempty deepest
                no_root_values parent provided) as [starts carries].
    exists slot1, slot0. split; [right; split; reflexivity|].
    split; [|exact carries].
    intro other_starts.
    apply (two_slots_under_the_parent_key_are_invalid slot0 slot1 node_key
             child_byte (subtree_forall_here _ _ valid)).
    + exact (slot_under_the_parent_key_carries_the_child slot0 slot1 slot0
               node_key child_byte (or_introl eq_refl) nonempty parent
               other_starts).
    + exact (slot_under_the_parent_key_carries_the_child slot0 slot1 slot1
               node_key child_byte (or_intror eq_refl) nonempty parent starts).
Qed.

(** Lemma 1 of the study.  Let the focus be a non-root parent without a
    value and with exactly one child byte, whose child has no value and no
    child.  If the focus node is a line-list node, exactly one of its slots
    has a key that starts with the node key, and either (i) that key is the
    node key followed by the child byte and its payload is a child pointer
    to an empty node, or (ii) that key is the node key and its payload is a
    child pointer to a node whose only path is the dangling child byte. *)
Theorem one_slot_carries_the_cut : forall focus child_byte slot0 slot1,
  pathmap_premises focus ->
  focus_is_single_child_parent focus child_byte ->
  focus_node focus = LineListNode slot0 slot1 ->
  exists carrying other,
    slot_roles slot0 slot1 carrying other /\
    prefix (focus_node_key focus) (slot_key carrying) /\
    ~ prefix (focus_node_key focus) (slot_key other) /\
    ((exists emptied,
        carrying = ChildSlot (focus_node_key focus ++ [child_byte]) emptied /\
        node_is_empty emptied = true) \/
     (exists below,
        carrying = ChildSlot (focus_node_key focus) below /\
        node_holds_only_dangling_byte below child_byte)).
Proof.
  intros [subject node_key outer_stops outcome] child_byte slot0 slot1
    [deepest [modelled [valid [_ [no_root_values _]]]]] [below_root parent]
    shape.
  pose proof (focus_below_the_root_has_a_node_key _ deepest below_root)
    as nonempty.
  destruct deepest as [deepest _].
  cbv [focus_node focus_node_key focus_outer_stops focus_prune_outcome] in *.
  subst subject.
  destruct (line_list_cut_shape slot0 slot1 node_key child_byte nonempty
              deepest valid no_root_values parent)
    as [carrying [other [roles [other_off carries]]]].
  exists carrying, other. split; [exact roles|].
  destruct (slot_roles_are_members slot0 slot1 carrying other roles)
    as [carrying_member _].
  destruct carries
    as [[emptied [carrying_shape pathless]]|[below [carrying_shape dangling]]].
  - split.
    { rewrite carrying_shape. simpl. exists [child_byte]. reflexivity. }
    split; [exact other_off|].
    left. exists emptied. split; [exact carrying_shape|].
    apply pathless_node_is_empty.
    + exact (subtree_forall_here _ _
               (member_slot_subtree _ slot0 slot1 carrying
                  (node_key ++ [child_byte]) emptied carrying_member
                  carrying_shape modelled)).
    + exact (subtree_forall_here _ _
               (member_slot_subtree _ slot0 slot1 carrying
                  (node_key ++ [child_byte]) emptied carrying_member
                  carrying_shape valid)).
    + exact pathless.
  - split.
    { rewrite carrying_shape. simpl. apply prefix_reflexive. }
    split; [exact other_off|].
    right. exists below. split; [exact carrying_shape | exact dangling].
Qed.

(** ** Lemma 2: the cut is bounded *)

(** The call on a line-list node when only slot 0 is selected: the
    non-pruning branch of [remove_subtries] cuts slot 0 and leaves slot 1
    alone. *)
Lemma cut_in_slot0_computes : forall slot0 slot1 key,
  key_is_empty key = false ->
  slot_is_removed slot0 key = true ->
  starts_with (slot_key slot1) key = false ->
  line_list_remove_all_branches PruneLimitUnbounded slot0 slot1 key =
  NodeRemoval (LineListNode (cut_slot (length key) slot0) slot1) true
    (slot_payload slot0).
Proof.
  intros slot0 slot1 key nonempty removed0 off1.
  assert (kept1 : slot_is_removed slot1 key = false).
  { unfold slot_is_removed. rewrite off1. reflexivity. }
  unfold line_list_remove_all_branches. cbv zeta.
  rewrite removed0, kept1, nonempty. reflexivity.
Qed.

(** The call when only slot 1 is selected and the key of slot 0 does not
    start with the removal key: slot 1 is cut and slot 0 is left alone. *)
Lemma cut_in_slot1_computes : forall slot0 slot1 key,
  key_is_empty key = false ->
  starts_with (slot_key slot0) key = false ->
  slot_is_removed slot1 key = true ->
  line_list_remove_all_branches PruneLimitUnbounded slot0 slot1 key =
  NodeRemoval (LineListNode slot0 (cut_slot (length key) slot1)) true
    (slot_payload slot1).
Proof.
  intros slot0 slot1 key nonempty off0 removed1.
  assert (kept0 : slot_is_removed slot0 key = false).
  { unfold slot_is_removed. rewrite off0. reflexivity. }
  unfold line_list_remove_all_branches. cbv zeta.
  rewrite kept0, removed1, off0, nonempty. simpl.
  rewrite app_nil_r. reflexivity.
Qed.

(** What the carrying slot of Lemma 1 provides, in both of its cases: the
    call selects it, cutting it leaves the node key with a pointer to the
    sentinel, its paths are the prefixes of the node key and the path to
    the child, it holds no value, and its payload is one child pointer. *)
Lemma carrying_slot_summary : forall carrying node_key child_byte,
  slot_carries_the_cut carrying node_key child_byte ->
  slot_is_removed carrying node_key = true /\
  cut_slot (length node_key) carrying = ChildSlot node_key EmptyNode /\
  (forall path,
     slot_has_path carrying path <->
     prefix path node_key \/ path = node_key ++ [child_byte]) /\
  (forall path value, ~ slot_holds_value carrying path value) /\
  exists key detached,
    carrying = ChildSlot key detached /\
    (node_is_pathless detached \/
     node_holds_only_dangling_byte detached child_byte).
Proof.
  intros carrying node_key child_byte
    [[emptied [shape pathless]]|[below [shape [below_paths below_values]]]];
    subst carrying.
  - split.
    { unfold slot_is_removed. cbn [slot_key slot_is_child].
      apply andb_true_iff.
      split; [apply starts_with_extension | apply orb_true_r]. }
    split.
    { unfold cut_slot. cbn [slot_key]. rewrite firstn_extension.
      reflexivity. }
    split.
    { intro path. split.
      - intros [under|[rest [split_here child_path]]].
        + apply prefix_of_one_byte_extension. exact under.
        + right. rewrite (pathless rest child_path) in split_here.
          rewrite app_nil_r in split_here. exact split_here.
      - intros [under|same].
        + left. apply prefix_of_one_byte_extension. left. exact under.
        + left. apply prefix_of_one_byte_extension. right. exact same. }
    split.
    { intros path value [rest [_ [nonempty held]]]. apply nonempty.
      apply pathless.
      exact (proj1 a_value_lies_on_a_path emptied rest value held). }
    exists (node_key ++ [child_byte]), emptied.
    split; [reflexivity | left; exact pathless].
  - split.
    { unfold slot_is_removed. cbn [slot_key slot_is_child].
      apply andb_true_iff.
      split; [apply starts_with_spec; apply prefix_reflexive
             | apply orb_true_r]. }
    split.
    { unfold cut_slot. cbn [slot_key]. rewrite firstn_all. reflexivity. }
    split.
    { intro path. split.
      - intros [under|[rest [split_here child_path]]].
        + left. exact under.
        + apply below_paths in child_path.
          destruct child_path as [empty|single].
          * left. subst rest. rewrite app_nil_r in split_here. subst path.
            apply prefix_reflexive.
          * right. subst rest. exact split_here.
      - intros [under|same].
        + left. exact under.
        + right. exists [child_byte]. split; [exact same|].
          apply below_paths. right. reflexivity. }
    split.
    { intros path value [rest [_ [_ held]]].
      exact (below_values rest value held). }
    exists node_key, below.
    split; [reflexivity | right; split; assumption].
Qed.

(** The slot that the cut leaves behind ends a dangling path at the node
    key: its paths are the prefixes of the node key and it holds no
    value. *)
Lemma cut_slot_summary : forall node_key,
  (forall path,
     slot_has_path (ChildSlot node_key EmptyNode) path <->
     prefix path node_key) /\
  (forall path value,
     ~ slot_holds_value (ChildSlot node_key EmptyNode) path value).
Proof.
  intro node_key. split.
  - intro path. split.
    + intros [under|[rest [split_here [empty|[]]]]]; [exact under|].
      subst rest. rewrite app_nil_r in split_here. subst path.
      apply prefix_reflexive.
    + intro under. left. exact under.
  - intros path value [rest [_ [_ []]]].
Qed.

(** What Lemma 2 states about the node call of [remove_branches(false)]:
    it removes something; the parent's path stays, without a child; the
    paths afterwards are the paths before without the path to the child;
    the values are the same; and exactly one child pointer is detached,
    whose destructors nest at most two deep. *)
Definition removal_is_the_bounded_cut (subject : node) (node_key : list nat)
    (child_byte : nat) (removal : node_removal) : Prop :=
  removal_removed removal = true /\
  node_has_path (removal_node removal) node_key /\
  (forall byte, ~ node_has_path (removal_node removal) (node_key ++ [byte])) /\
  (forall path,
     node_has_path (removal_node removal) path <->
     node_has_path subject path /\ path <> node_key ++ [child_byte]) /\
  (forall path value,
     node_holds_value (removal_node removal) path value <->
     node_holds_value subject path value) /\
  exists detached,
    removal_detached removal = [DetachedChild detached] /\
    node_drop_nesting detached <= 2.

(** Cases (i) and (ii): the focus node is a line-list node. *)
Lemma line_list_cut_is_bounded : forall slot0 slot1 node_key child_byte,
  node_key <> [] ->
  (forall key,
     proper_prefix key node_key ->
     ~ node_has_child_pointer_at (LineListNode slot0 slot1) key) ->
  subtree_forall node_kind_is_modelled (LineListNode slot0 slot1) ->
  subtree_forall line_list_node_is_valid (LineListNode slot0 slot1) ->
  subtree_forall node_holds_no_root_value (LineListNode slot0 slot1) ->
  single_child_parent_at (LineListNode slot0 slot1) node_key child_byte ->
  removal_is_the_bounded_cut (LineListNode slot0 slot1) node_key child_byte
    (node_remove_all_branches PruneLimitUnbounded (LineListNode slot0 slot1)
       node_key).
Proof.
  intros slot0 slot1 node_key child_byte nonempty deepest modelled valid
    no_root_values parent.
  destruct (line_list_cut_shape slot0 slot1 node_key child_byte nonempty
              deepest valid no_root_values parent)
    as [carrying [other [roles [other_off carries]]]].
  destruct (slot_roles_are_members slot0 slot1 carrying other roles)
    as [carrying_member other_member].
  destruct (carrying_slot_summary carrying node_key child_byte carries)
    as [removed [cut_shape [carrying_paths [carrying_values
         [carrying_key [detached [carrying_shape detached_shape]]]]]]].
  destruct (cut_slot_summary node_key) as [cut_paths cut_values].
  assert (other_paths :
    forall rest, ~ slot_has_path other (node_key ++ rest)).
  { intro rest.
    apply (slot_off_the_key_has_no_path_below other node_key rest other_off).
    exact (member_has_no_pointer_above slot0 slot1 other node_key
             other_member deepest). }
  assert (shallow : node_drop_nesting detached <= 2).
  { pose proof (member_slot_subtree _ slot0 slot1 carrying carrying_key
                  detached carrying_member carrying_shape modelled)
      as detached_modelled.
    pose proof (member_slot_subtree _ slot0 slot1 carrying carrying_key
                  detached carrying_member carrying_shape valid)
      as detached_valid.
    destruct detached_shape as [pathless|dangling].
    - apply le_S.
      exact (pathless_node_is_shallow detached
               (subtree_forall_here _ _ detached_modelled)
               (subtree_forall_here _ _ detached_valid) pathless).
    - exact (dangling_byte_node_is_shallow detached child_byte
               detached_modelled detached_valid dangling). }
  assert (computed :
    exists after0 after1,
      node_remove_all_branches PruneLimitUnbounded (LineListNode slot0 slot1)
        node_key =
      NodeRemoval (LineListNode after0 after1) true [DetachedChild detached] /\
      slot_roles after0 after1 (ChildSlot node_key EmptyNode) other).
  { assert (payload_shape :
      slot_payload carrying = [DetachedChild detached]).
    { rewrite carrying_shape. reflexivity. }
    unfold node_remove_all_branches.
    destruct roles as [[same0 same1]|[same1 same0]].
    - exists (ChildSlot node_key EmptyNode), other. split.
      + rewrite <- same0, <- same1.
        rewrite cut_in_slot0_computes.
        * rewrite cut_shape, payload_shape. reflexivity.
        * apply nonempty_key_is_not_empty. exact nonempty.
        * exact removed.
        * apply starts_with_false. exact other_off.
      + left. split; reflexivity.
    - exists other, (ChildSlot node_key EmptyNode). split.
      + rewrite <- same1, <- same0.
        rewrite cut_in_slot1_computes.
        * rewrite cut_shape, payload_shape. reflexivity.
        * apply nonempty_key_is_not_empty. exact nonempty.
        * apply starts_with_false. exact other_off.
        * exact removed.
      + right. split; reflexivity. }
  destruct computed as [after0 [after1 [computed roles_after]]].
  unfold removal_is_the_bounded_cut. rewrite computed.
  cbv [removal_node removal_removed removal_detached].
  split; [reflexivity|].
  split.
  { apply (node_path_by_slot_roles after0 after1
             (ChildSlot node_key EmptyNode) other node_key roles_after).
    right. left. apply cut_paths. apply prefix_reflexive. }
  split.
  { intros byte provided.
    apply (node_path_by_slot_roles after0 after1
             (ChildSlot node_key EmptyNode) other (node_key ++ [byte])
             roles_after) in provided.
    destruct provided as [empty|[under|beside]].
    - exact (extension_is_not_empty node_key byte empty).
    - apply cut_paths in under.
      exact (extension_is_not_a_prefix node_key byte under).
    - exact (other_paths [byte] beside). }
  split.
  { intro path. split.
    - intro provided.
      apply (node_path_by_slot_roles after0 after1
               (ChildSlot node_key EmptyNode) other path roles_after)
        in provided.
      destruct provided as [empty|[under|beside]].
      + subst path. split; [apply root_path_exists|].
        intro same. symmetry in same.
        exact (extension_is_not_empty node_key child_byte same).
      + apply cut_paths in under. split.
        * apply (node_path_by_slot_roles slot0 slot1 carrying other path
                   roles).
          right. left. apply carrying_paths. left. exact under.
        * intro same. subst path.
          exact (extension_is_not_a_prefix node_key child_byte under).
      + split.
        * apply (node_path_by_slot_roles slot0 slot1 carrying other path
                   roles).
          right. right. exact beside.
        * intro same. subst path. exact (other_paths [child_byte] beside).
    - intros [provided different].
      apply (node_path_by_slot_roles slot0 slot1 carrying other path roles)
        in provided.
      apply (node_path_by_slot_roles after0 after1
               (ChildSlot node_key EmptyNode) other path roles_after).
      destruct provided as [empty|[carried|beside]].
      + left. exact empty.
      + right. left. apply cut_paths. apply carrying_paths in carried.
        destruct carried as [under|same]; [exact under|].
        exfalso. exact (different same).
      + right. right. exact beside. }
  split.
  { intros path value. split.
    - intro held.
      apply (node_value_by_slot_roles after0 after1
               (ChildSlot node_key EmptyNode) other path value roles_after)
        in held.
      apply (node_value_by_slot_roles slot0 slot1 carrying other path value
               roles).
      destruct held as [cut_held|other_held].
      + exfalso. exact (cut_values path value cut_held).
      + right. exact other_held.
    - intro held.
      apply (node_value_by_slot_roles slot0 slot1 carrying other path value
               roles) in held.
      apply (node_value_by_slot_roles after0 after1
               (ChildSlot node_key EmptyNode) other path value roles_after).
      destruct held as [carried_held|other_held].
      + exfalso. exact (carrying_values path value carried_held).
      + right. exact other_held. }
  exists detached. split; [reflexivity | exact shallow].
Qed.

(** Case (iii): the focus node is a dense node and the node key is one
    byte.  The cell of that byte has no value and a child pointer to the
    node that holds the dangling child byte; with a limit other than 0 the
    call clears the pointer and keeps the cell. *)
Lemma dense_cut_is_bounded : forall dense node_key child_byte,
  subtree_forall node_kind_is_modelled (DenseByteNode dense) ->
  subtree_forall line_list_node_is_valid (DenseByteNode dense) ->
  subtree_forall node_holds_no_root_value (DenseByteNode dense) ->
  length node_key = 1 ->
  single_child_parent_at (DenseByteNode dense) node_key child_byte ->
  removal_is_the_bounded_cut (DenseByteNode dense) node_key child_byte
    (node_remove_all_branches PruneLimitUnbounded (DenseByteNode dense)
       node_key).
Proof.
  intros dense node_key child_byte [_ cells_modelled] [_ cells_valid]
    [_ cells_no_root_values] one_byte parent.
  destruct node_key as [|parent_byte [|second more]]; simpl in one_byte;
    try discriminate one_byte.
  clear one_byte.
  pose proof parent as [no_value [children [no_leaf_value no_grandchild]]].
  cbn [app] in no_value, children, no_leaf_value, no_grandchild.
  assert (provided :
    node_has_path (DenseByteNode dense) [parent_byte; child_byte]).
  { apply children. reflexivity. }
  destruct provided as [empty|provided]; [discriminate empty|].
  apply cells_have_path_at in provided.
  revert provided.
  destruct (cell_at dense parent_byte) as [|stored [below|]] eqn:at_parent;
    intro provided.
  - destruct provided.
  - destruct stored as [value|].
    { exfalso. apply (no_value value).
      apply (proj2 (cells_hold_value_at dense parent_byte [] value)).
      rewrite at_parent. left. split; reflexivity. }
    assert (dangling : node_holds_only_dangling_byte below child_byte).
    { apply (node_below_the_parent_holds_only_the_dangling_byte
               (DenseByteNode dense) [parent_byte] child_byte below parent).
      - intros rest below_path. cbn [app]. right.
        apply (proj2 (cells_have_path_at dense parent_byte rest)).
        rewrite at_parent. exact below_path.
      - intros rest value rest_nonempty held. cbn [app].
        apply (proj2 (cells_hold_value_at dense parent_byte rest value)).
        rewrite at_parent. right.
        split; [exact rest_nonempty | exact held].
      - exact provided.
      - exact (subtree_forall_here _ _
                 (cells_subtree_forall_at _ dense parent_byte None below
                    cells_no_root_values at_parent)). }
    destruct dangling as [below_paths below_values].
    assert (shallow : node_drop_nesting below <= 2).
    { apply (dangling_byte_node_is_shallow below child_byte).
      - exact (cells_subtree_forall_at _ dense parent_byte None below
                 cells_modelled at_parent).
      - exact (cells_subtree_forall_at _ dense parent_byte None below
                 cells_valid at_parent).
      - split; [exact below_paths | exact below_values]. }
    assert (at_parent_after :
      cell_at (set_cell dense parent_byte (PresentCell None None)) parent_byte =
      PresentCell None None).
    { apply cell_at_set_cell_same. }
    assert (elsewhere : forall byte,
      byte <> parent_byte ->
      cell_at (set_cell dense parent_byte (PresentCell None None)) byte =
      cell_at dense byte).
    { intros byte different. apply cell_at_set_cell_other. exact different. }
    unfold removal_is_the_bounded_cut, node_remove_all_branches,
      dense_remove_all_branches.
    rewrite at_parent.
    cbv [removal_node removal_removed removal_detached]. cbn [app].
    split; [reflexivity|].
    split.
    { right. apply (proj2 (cells_have_path_at _ parent_byte [])).
      rewrite at_parent_after. exact eq_refl. }
    split.
    { intros byte [empty|after_path]; [discriminate empty|].
      apply cells_have_path_at in after_path.
      rewrite at_parent_after in after_path. simpl in after_path.
      discriminate after_path. }
    split.
    { intros [|first rest].
      - split.
        + intros _. split; [apply root_path_exists | discriminate].
        + intros _. apply root_path_exists.
      - destruct (Nat.eq_dec first parent_byte) as [same|different].
        + subst first. split.
          * intros [empty|after_path]; [discriminate empty|].
            apply cells_have_path_at in after_path.
            rewrite at_parent_after in after_path. simpl in after_path.
            subst rest. split; [|discriminate].
            right. apply (proj2 (cells_have_path_at dense parent_byte [])).
            rewrite at_parent. exact (root_path_exists below).
          * intros [[empty|before_path] other_path]; [discriminate empty|].
            apply cells_have_path_at in before_path.
            rewrite at_parent in before_path. simpl in before_path.
            apply below_paths in before_path.
            destruct before_path as [empty|single].
            -- subst rest. right.
               apply (proj2 (cells_have_path_at _ parent_byte [])).
               rewrite at_parent_after. exact eq_refl.
            -- exfalso. apply other_path. rewrite single. reflexivity.
        + split.
          * intros [empty|after_path]; [discriminate empty|].
            apply cells_have_path_at in after_path.
            rewrite (elsewhere first different) in after_path.
            split.
            -- right. apply cells_have_path_at. exact after_path.
            -- intro same. injection same as same_first same_rest.
               exact (different same_first).
          * intros [[empty|before_path] _]; [discriminate empty|].
            apply cells_have_path_at in before_path.
            right. apply cells_have_path_at.
            rewrite (elsewhere first different). exact before_path. }
    split.
    { intros [|first rest] value.
      - split; intro held; exfalso.
        + exact (cells_hold_no_root_value
                   (set_cell dense parent_byte (PresentCell None None)) value
                   held).
        + exact (cells_hold_no_root_value dense value held).
      - destruct (Nat.eq_dec first parent_byte) as [same|different].
        + subst first. split; intro held; exfalso.
          * pose proof (proj1 (cells_hold_value_at
                                 (set_cell dense parent_byte
                                    (PresentCell None None))
                                 parent_byte rest value) held) as at_cell.
            rewrite at_parent_after in at_cell.
            destruct at_cell as [_ impossible]. discriminate impossible.
          * pose proof (proj1 (cells_hold_value_at dense parent_byte rest
                                 value) held) as at_cell.
            rewrite at_parent in at_cell.
            destruct at_cell as [[_ impossible]|[_ below_held]];
              [discriminate impossible|].
            exact (below_values rest value below_held).
        + split; intro held.
          * pose proof (proj1 (cells_hold_value_at
                                 (set_cell dense parent_byte
                                    (PresentCell None None))
                                 first rest value) held) as at_cell.
            rewrite (elsewhere first different) in at_cell.
            exact (proj2 (cells_hold_value_at dense first rest value)
                     at_cell).
          * pose proof (proj1 (cells_hold_value_at dense first rest value)
                          held) as at_cell.
            apply (proj2 (cells_hold_value_at
                            (set_cell dense parent_byte
                               (PresentCell None None))
                            first rest value)).
            rewrite (elsewhere first different). exact at_cell. }
    exists below. split; [reflexivity | exact shallow].
  - simpl in provided. discriminate provided.
Qed.

(** Lemma 2 of the study.  Under the hypotheses of Lemma 1,
    [remove_branches(false)] at the parent (a) allocates no node (beyond a
    copy-on-write clone, which the model does not count), (b) leaves the
    parent in the trie without a value and without a child and changes no
    other path or value, and (c) detaches one child pointer whose
    destructors nest at most two deep. *)
Theorem the_cut_is_bounded : forall focus child_byte,
  pathmap_premises focus ->
  focus_is_single_child_parent focus child_byte ->
  let cut :=
    remove_branches_without_prune (focus_node focus) (focus_node_key focus) in
  cut_allocated_nodes cut = 0 /\
  cut_removed cut = true /\
  node_has_path (cut_node cut) (focus_node_key focus) /\
  (forall value,
     ~ node_holds_value (cut_node cut) (focus_node_key focus) value) /\
  (forall byte,
     ~ node_has_path (cut_node cut) (focus_node_key focus ++ [byte])) /\
  (forall path,
     node_has_path (cut_node cut) path <->
     node_has_path (focus_node focus) path /\
     path <> focus_node_key focus ++ [child_byte]) /\
  (forall path value,
     node_holds_value (cut_node cut) path value <->
     node_holds_value (focus_node focus) path value) /\
  exists detached,
    cut_detached cut = [DetachedChild detached] /\
    node_drop_nesting detached <= 2.
Proof.
  intros [subject node_key outer_stops outcome] child_byte
    [deepest [modelled [valid [dense_key [no_root_values _]]]]]
    [below_root parent].
  pose proof (focus_below_the_root_has_a_node_key _ deepest below_root)
    as nonempty.
  destruct deepest as [deepest _].
  cbv [focus_node focus_node_key focus_outer_stops focus_prune_outcome
       dense_focus_key_is_one_byte] in *.
  assert (bounded :
    removal_is_the_bounded_cut subject node_key child_byte
      (node_remove_all_branches PruneLimitUnbounded subject node_key)).
  { destruct subject as [|slot0 slot1|dense|].
    - exfalso. destruct parent as [_ [children _]].
      destruct (proj2 (children child_byte) eq_refl) as [empty|[]].
      exact (extension_is_not_empty node_key child_byte empty).
    - exact (line_list_cut_is_bounded slot0 slot1 node_key child_byte
               nonempty deepest modelled valid no_root_values parent).
    - apply (dense_cut_is_bounded dense node_key child_byte modelled valid
               no_root_values); [|exact parent].
      apply (dense_key dense eq_refl nonempty).
      destruct parent as [_ [children _]].
      apply (proj1 paths_are_closed_under_prefixes (DenseByteNode dense)
               node_key [child_byte]).
      apply children. reflexivity.
    - exfalso. apply (subtree_forall_here _ _ modelled). reflexivity. }
  destruct node_key as [|first key_rest];
    [exfalso; apply nonempty; reflexivity|].
  destruct bounded as [removed [kept [childless [paths [values shallow]]]]].
  cbv zeta. unfold remove_branches_without_prune.
  cbv [cut_node cut_removed cut_detached cut_allocated_nodes].
  split; [reflexivity|].
  split; [exact removed|].
  split; [exact kept|].
  split.
  { intros value held. destruct parent as [no_value _].
    exact (no_value value (proj1 (values (first :: key_rest) value) held)). }
  split; [exact childless|].
  split; [exact paths|].
  split; [exact values | exact shallow].
Qed.

(** ** Lemma 3: the stopping cut is one byte *)

Lemma prefix_transitive : forall start middle key,
  prefix start middle -> prefix middle key -> prefix start key.
Proof.
  intros start middle key [rest1 shape1] [rest2 shape2].
  subst middle key. exists (rest1 ++ rest2). symmetry. apply app_assoc.
Qed.

(** A key continues a position by at most one byte. *)
Lemma one_key_extends_one_child :
  forall (parent_key : list nat) byte other key,
    prefix (parent_key ++ [byte]) key -> prefix (parent_key ++ [other]) key ->
    byte = other.
Proof.
  intros parent_key byte other key [rest shape] [other_rest other_shape].
  rewrite shape in other_shape. rewrite <- !app_assoc in other_shape.
  apply app_inv_head in other_shape. simpl in other_shape.
  injection other_shape as same tail. exact same.
Qed.

(** The proper prefixes of a path do not depend on its last byte. *)
Lemma proper_prefix_of_sibling :
  forall key (parent_key : list nat) byte other,
    proper_prefix key (parent_key ++ [byte]) ->
    proper_prefix key (parent_key ++ [other]).
Proof.
  intros key parent_key byte other [rest [nonempty shape]].
  destruct (exists_last nonempty) as [front [last_byte rest_shape]].
  rewrite rest_shape in shape. rewrite app_assoc in shape.
  apply app_inj_tail in shape. destruct shape as [parent_shape _].
  exists (front ++ [other]). split; [apply extension_is_not_empty|].
  rewrite parent_shape. rewrite <- app_assoc. reflexivity.
Qed.

(** Two keys that continue a position by different bytes are not related
    by prefix. *)
Lemma keys_of_two_children_are_unrelated :
  forall (parent_key : list nat) byte other key other_key,
    byte <> other ->
    prefix (parent_key ++ [byte]) key ->
    prefix (parent_key ++ [other]) other_key ->
    ~ (prefix key other_key \/ prefix other_key key).
Proof.
  intros parent_key byte other key other_key different under other_under
    [shorter|shorter]; apply different.
  - exact (one_key_extends_one_child parent_key byte other other_key
             (prefix_transitive _ _ _ under shorter) other_under).
  - exact (one_key_extends_one_child parent_key byte other key under
             (prefix_transitive _ _ _ other_under shorter)).
Qed.

Lemma slot_under_a_nonempty_key_is_used : forall subject key,
  key <> [] -> prefix key (slot_key subject) -> slot_is_used subject = true.
Proof.
  intros [|stored_key stored|stored_key child] key nonempty under;
    [|reflexivity|reflexivity].
  exfalso. destruct under as [suffix key_shape]. simpl in key_shape.
  symmetry in key_shape. apply app_eq_nil in key_shape.
  apply nonempty. exact (proj1 key_shape).
Qed.

(** A path of a slot with no child pointer above it lies under the slot's
    key. *)
Lemma member_path_without_pointer_above_lies_under_its_key :
  forall slot0 slot1 member path,
    member = slot0 \/ member = slot1 ->
    slot_has_path member path ->
    (forall key,
       proper_prefix key path ->
       ~ node_has_child_pointer_at (LineListNode slot0 slot1) key) ->
    slot_is_used member = true /\ prefix path (slot_key member).
Proof.
  intros slot0 slot1 member path is_member provided no_pointer.
  assert (used : slot_is_used member = true).
  { destruct member as [|key stored|key child];
      [destruct provided | reflexivity | reflexivity]. }
  split; [exact used|].
  destruct (slot_path_cases member path provided)
    as [under|[key [child [rest [shape [split_here [rest_nonempty _]]]]]]];
    [exact under|].
  exfalso. apply (no_pointer key).
  - exists rest. split; [exact rest_nonempty | exact split_here].
  - exact (member_child_pointer slot0 slot1 member key child is_member shape).
Qed.

Lemma path_without_pointer_above_lies_under_a_slot_key :
  forall slot0 slot1 path,
    path <> [] ->
    node_has_path (LineListNode slot0 slot1) path ->
    (forall key,
       proper_prefix key path ->
       ~ node_has_child_pointer_at (LineListNode slot0 slot1) key) ->
    exists member,
      (member = slot0 \/ member = slot1) /\
      slot_is_used member = true /\ prefix path (slot_key member).
Proof.
  intros slot0 slot1 path nonempty [empty|[provided|provided]] no_pointer.
  - exfalso. exact (nonempty empty).
  - exists slot0. split; [left; reflexivity|].
    exact (member_path_without_pointer_above_lies_under_its_key slot0 slot1
             slot0 path (or_introl eq_refl) provided no_pointer).
  - exists slot1. split; [right; reflexivity|].
    exact (member_path_without_pointer_above_lies_under_its_key slot0 slot1
             slot1 path (or_intror eq_refl) provided no_pointer).
Qed.

(** P3 excludes two children of a position that lies inside a line-list
    node: their keys would share the position's non-empty node key without
    being related by prefix. *)
Lemma two_children_inside_a_node_are_invalid :
  forall slot0 slot1 parent_key byte other member other_member,
    parent_key <> [] ->
    byte <> other ->
    line_list_slots_are_valid slot0 slot1 ->
    member = slot0 \/ member = slot1 ->
    other_member = slot0 \/ other_member = slot1 ->
    slot_is_used member = true ->
    slot_is_used other_member = true ->
    prefix (parent_key ++ [byte]) (slot_key member) ->
    prefix (parent_key ++ [other]) (slot_key other_member) ->
    False.
Proof.
  intros slot0 slot1 parent_key byte other member other_member nonempty
    different valid is_member is_other_member used other_used under
    other_under.
  destruct parent_key as [|first parent_rest];
    [apply nonempty; reflexivity|].
  pose proof under as [rest shape].
  pose proof other_under as [other_rest other_shape].
  destruct valid as [_ [_ [_ [_ [_ [related _]]]]]].
  destruct is_member as [same|same];
    destruct is_other_member as [other_same|other_same];
    subst member other_member.
  - apply different.
    exact (one_key_extends_one_child (first :: parent_rest) byte other
             (slot_key slot0) under other_under).
  - apply (keys_of_two_children_are_unrelated (first :: parent_rest) byte
             other (slot_key slot0) (slot_key slot1) different under
             other_under).
    exact (related other_used first ((parent_rest ++ [byte]) ++ rest)
             ((parent_rest ++ [other]) ++ other_rest) shape other_shape).
  - apply (keys_of_two_children_are_unrelated (first :: parent_rest) byte
             other (slot_key slot1) (slot_key slot0) different under
             other_under).
    destruct (related used first ((parent_rest ++ [other]) ++ other_rest)
                ((parent_rest ++ [byte]) ++ rest) other_shape shape)
      as [shorter|shorter]; [right | left]; exact shorter.
  - apply different.
    exact (one_key_extends_one_child (first :: parent_rest) byte other
             (slot_key slot1) under other_under).
Qed.

(** The first step of the study's proof: the node key of a leaf whose
    parent stops the run is one byte.  The parent is the zipper root when
    the leaf's depth is 1; it has more than one child when the node key
    continues a parent key by the leaf byte and the same parent key
    continued by another byte is a path too. *)
Lemma stopping_leaf_key_is_one_byte :
  forall subject node_key (outer_stops : list bool),
  (forall key,
     proper_prefix key node_key -> ~ node_has_child_pointer_at subject key) ->
  (node_key = [] -> outer_stops = []) ->
  line_list_node_is_valid subject ->
  (forall dense,
     subject = DenseByteNode dense -> node_key <> [] ->
     node_has_path subject node_key -> length node_key = 1) ->
  node_has_path subject node_key ->
  (length outer_stops + length node_key = 1 \/
   exists parent_key leaf_byte sibling_byte,
     node_key = parent_key ++ [leaf_byte] /\
     sibling_byte <> leaf_byte /\
     node_has_path subject (parent_key ++ [sibling_byte])) ->
  exists leaf_byte,
    node_key = [leaf_byte] /\
    (outer_stops = [] \/
     exists sibling_byte,
       sibling_byte <> leaf_byte /\ node_has_path subject [sibling_byte]).
Proof.
  intros subject node_key outer_stops deepest root_only valid dense_key
    provided
    [at_root|[parent_key [leaf_byte [sibling_byte
                                      [key_shape [different sibling]]]]]].
  - destruct node_key as [|leaf_byte [|second more]].
    + exfalso. rewrite (root_only eq_refl) in at_root.
      simpl in at_root. discriminate at_root.
    + exists leaf_byte. split; [reflexivity|]. left.
      destruct outer_stops as [|stop higher]; [reflexivity|].
      exfalso. simpl in at_root. lia.
    + exfalso. simpl in at_root. lia.
  - subst node_key. destruct parent_key as [|first parent_rest].
    + exists leaf_byte. split; [reflexivity|]. right.
      exists sibling_byte. split; [exact different | exact sibling].
    + exfalso.
      destruct subject as [|slot0 slot1|dense|].
      * destruct provided as [empty|[]]. simpl in empty.
        discriminate empty.
      * assert (parent_nonempty : first :: parent_rest <> [])
          by discriminate.
        assert (leaf_differs : leaf_byte <> sibling_byte).
        { intro same. apply different. symmetry. exact same. }
        destruct (path_without_pointer_above_lies_under_a_slot_key slot0
                    slot1 ((first :: parent_rest) ++ [leaf_byte])
                    (extension_is_not_empty (first :: parent_rest) leaf_byte)
                    provided deepest)
          as [leaf_member [leaf_is_member [leaf_used leaf_under]]].
        assert (sibling_deepest : forall key,
                  proper_prefix key
                    ((first :: parent_rest) ++ [sibling_byte]) ->
                  ~ node_has_child_pointer_at (LineListNode slot0 slot1) key).
        { intros key above. apply deepest.
          exact (proper_prefix_of_sibling key (first :: parent_rest)
                   sibling_byte leaf_byte above). }
        destruct (path_without_pointer_above_lies_under_a_slot_key slot0
                    slot1 ((first :: parent_rest) ++ [sibling_byte])
                    (extension_is_not_empty (first :: parent_rest)
                       sibling_byte)
                    sibling sibling_deepest)
          as [sibling_member
                [sibling_is_member [sibling_used sibling_under]]].
        exact (two_children_inside_a_node_are_invalid slot0 slot1
                 (first :: parent_rest) leaf_byte sibling_byte leaf_member
                 sibling_member parent_nonempty leaf_differs valid
                 leaf_is_member sibling_is_member leaf_used sibling_used
                 leaf_under sibling_under).
      * assert (one_byte :
          length ((first :: parent_rest) ++ [leaf_byte]) = 1).
        { apply (dense_key dense eq_refl); [simpl; discriminate|].
          exact provided. }
        simpl in one_byte. injection one_byte as impossible.
        apply length_zero_iff_nil in impossible.
        exact (extension_is_not_empty parent_rest leaf_byte impossible).
      * destruct provided as [empty|[]]. simpl in empty.
        discriminate empty.
Qed.

(** The second step: a used slot whose key starts with the node key of a
    childless value-free leaf holds exactly that key and a child pointer to
    a node without a path. *)
Lemma slot_under_a_leaf_key_is_its_child_pointer :
  forall slot0 slot1 member leaf_key,
    member = slot0 \/ member = slot1 ->
    slot_is_used member = true ->
    (forall value,
       ~ node_holds_value (LineListNode slot0 slot1) leaf_key value) ->
    (forall byte,
       ~ node_has_path (LineListNode slot0 slot1) (leaf_key ++ [byte])) ->
    prefix leaf_key (slot_key member) ->
    exists emptied,
      member = ChildSlot leaf_key emptied /\ node_is_pathless emptied.
Proof.
  intros slot0 slot1 member leaf_key is_member used no_value childless
    [suffix key_shape].
  destruct suffix as [|first suffix_rest].
  - rewrite app_nil_r in key_shape.
    destruct member as [|key stored|key child]; simpl in key_shape.
    + discriminate used.
    + exfalso. subst key. apply (no_value stored).
      apply (member_slot_value slot0 slot1 (ValueSlot leaf_key stored)
               leaf_key stored is_member).
      split; reflexivity.
    + subst key. exists child. split; [reflexivity|].
      intros [|first rest] child_path; [reflexivity|]. exfalso.
      apply (childless first).
      apply (proj1 paths_are_closed_under_prefixes (LineListNode slot0 slot1)
               (leaf_key ++ [first]) rest).
      rewrite <- app_assoc.
      apply (member_slot_path slot0 slot1 (ChildSlot leaf_key child)
               (leaf_key ++ [first] ++ rest) is_member).
      right. exists (first :: rest).
      split; [reflexivity | exact child_path].
  - exfalso. apply (childless first).
    apply (member_slot_path slot0 slot1 member (leaf_key ++ [first])
             is_member).
    apply slot_key_prefix_is_slot_path; [exact used|].
    exists suffix_rest. rewrite key_shape. rewrite <- app_assoc.
    reflexivity.
Qed.

(** The slots of the focus node at a childless value-free leaf: one slot
    holds the leaf's key with a child pointer to a node without a path,
    and the key of the other slot does not start with the leaf's key. *)
Lemma line_list_leaf_shape : forall slot0 slot1 leaf_key,
  leaf_key <> [] ->
  (forall key,
     proper_prefix key leaf_key ->
     ~ node_has_child_pointer_at (LineListNode slot0 slot1) key) ->
  line_list_slots_are_valid slot0 slot1 ->
  node_has_path (LineListNode slot0 slot1) leaf_key ->
  (forall value,
     ~ node_holds_value (LineListNode slot0 slot1) leaf_key value) ->
  (forall byte,
     ~ node_has_path (LineListNode slot0 slot1) (leaf_key ++ [byte])) ->
  exists carrying other emptied,
    slot_roles slot0 slot1 carrying other /\
    carrying = ChildSlot leaf_key emptied /\
    node_is_pathless emptied /\
    ~ prefix leaf_key (slot_key other).
Proof.
  intros slot0 slot1 leaf_key nonempty deepest valid provided no_value
    childless.
  assert (pointer : forall member,
            member = slot0 \/ member = slot1 ->
            prefix leaf_key (slot_key member) ->
            exists emptied,
              member = ChildSlot leaf_key emptied /\
              node_is_pathless emptied).
  { intros member is_member under.
    exact (slot_under_a_leaf_key_is_its_child_pointer slot0 slot1 member
             leaf_key is_member
             (slot_under_a_nonempty_key_is_used member leaf_key nonempty
                under)
             no_value childless under). }
  destruct (path_without_pointer_above_lies_under_a_slot_key slot0 slot1
              leaf_key nonempty provided deepest)
    as [member [is_member [_ under]]].
  destruct valid as [_ [_ [_ [_ [no_shared _]]]]].
  destruct is_member as [same|same]; subst member.
  - destruct (pointer slot0 (or_introl eq_refl) under)
      as [emptied [shape pathless]].
    exists slot0, slot1, emptied.
    split; [left; split; reflexivity|].
    split; [exact shape|]. split; [exact pathless|].
    intro other_under.
    destruct (pointer slot1 (or_intror eq_refl) other_under)
      as [other_emptied [other_shape _]].
    apply no_shared.
    + rewrite shape. reflexivity.
    + rewrite other_shape. reflexivity.
    + rewrite shape, other_shape. reflexivity.
  - destruct (pointer slot1 (or_intror eq_refl) under)
      as [emptied [shape pathless]].
    exists slot1, slot0, emptied.
    split; [right; split; reflexivity|].
    split; [exact shape|]. split; [exact pathless|].
    intro other_under.
    destruct (pointer slot0 (or_introl eq_refl) other_under)
      as [other_emptied [other_shape _]].
    apply no_shared.
    + rewrite other_shape. reflexivity.
    + rewrite shape. reflexivity.
    + rewrite shape, other_shape. reflexivity.
Qed.

(** [node_remove_dangling] takes a child pointer to an empty node in
    slot 0 under exactly the key. *)
Lemma dangling_in_slot0_computes : forall slot1 key emptied,
  node_is_empty emptied = true ->
  line_list_remove_dangling (ChildSlot key emptied) slot1 key =
  DanglingRemoval (LineListNode slot1 UnusedSlot)
    (pruned_key_bytes key (slot_key slot1)) [DetachedChild emptied].
Proof.
  intros slot1 key emptied empty. unfold line_list_remove_dangling.
  rewrite key_eqb_refl, empty. reflexivity.
Qed.

(** It takes such a pointer in slot 1 when the key of slot 0 does not
    start with the key. *)
Lemma dangling_in_slot1_computes : forall slot0 key emptied,
  ~ prefix key (slot_key slot0) ->
  node_is_empty emptied = true ->
  line_list_remove_dangling slot0 (ChildSlot key emptied) key =
  DanglingRemoval (LineListNode slot0 UnusedSlot)
    (pruned_key_bytes key (slot_key slot0)) [DetachedChild emptied].
Proof.
  intros slot0 key emptied off empty.
  assert (in_slot1 :
    line_list_remove_dangling_in_slot1 slot0 (ChildSlot key emptied) key =
    DanglingRemoval (LineListNode slot0 UnusedSlot)
      (pruned_key_bytes key (slot_key slot0)) [DetachedChild emptied]).
  { unfold line_list_remove_dangling_in_slot1.
    rewrite key_eqb_refl, empty. reflexivity. }
  destruct slot0 as [|key0 stored0|key0 child0].
  - exact in_slot1.
  - exact in_slot1.
  - unfold line_list_remove_dangling.
    rewrite (key_eqb_false key0 key).
    + exact in_slot1.
    + intro same. apply off. simpl. rewrite same. apply prefix_reflexive.
Qed.

(** The paths and values of a slot that points to a node without a path:
    its paths are the prefixes of its key and it holds no value. *)
Lemma leaf_pointer_summary : forall leaf_key emptied,
  node_is_pathless emptied ->
  (forall path,
     slot_has_path (ChildSlot leaf_key emptied) path <->
     prefix path leaf_key) /\
  (forall path value,
     ~ slot_holds_value (ChildSlot leaf_key emptied) path value).
Proof.
  intros leaf_key emptied pathless. split.
  - intro path. split.
    + intros [under|[rest [split_here child_path]]]; [exact under|].
      rewrite (pathless rest child_path) in split_here.
      rewrite app_nil_r in split_here. subst path. apply prefix_reflexive.
    + intro under. left. exact under.
  - intros path value [rest [_ [nonempty held]]]. apply nonempty.
    apply pathless.
    exact (proj1 a_value_lies_on_a_path emptied rest value held).
Qed.

Lemma prefix_of_one_byte : forall path byte,
  prefix path [byte] <-> path = [] \/ path = [byte].
Proof.
  intros path byte. split.
  - intros [rest shape]. destruct path as [|first path_rest].
    + left. reflexivity.
    + right. simpl in shape. injection shape as same tail. subst first.
      destruct path_rest as [|second more]; [reflexivity|].
      simpl in tail. discriminate tail.
  - intros [empty|single]; subst path.
    + exists [byte]. reflexivity.
    + apply prefix_reflexive.
Qed.

(** What Lemma 3 states about [prune_path] at the leaf: it returns 1 and
    removes nothing above the focus node; the paths afterwards are the
    paths before without the leaf byte; the values are the same; and what
    it detaches is nothing with a destructor (an empty cell) or one child
    pointer to an empty node, of nesting at most 1. *)
Definition report_is_the_one_byte_prune (subject : node) (leaf_byte : nat)
    (report : prune_report) : Prop :=
  report_pruned_bytes report = 1 /\
  report_removed_above_node report = false /\
  (forall path,
     node_has_path (report_node report) path <->
     node_has_path subject path /\ path <> [leaf_byte]) /\
  (forall path value,
     node_holds_value (report_node report) path value <->
     node_holds_value subject path value) /\
  (report_detached report = [] \/
   exists emptied,
     report_detached report = [DetachedChild emptied] /\
     node_is_empty emptied = true) /\
  detached_drop_nesting (report_detached report) <= 1.

(** The focus node is a line-list node.  The slot with key [leaf_byte]
    is taken and the count is 1, because the key of the other slot does
    not start with that byte.  If the other slot is used, the node is not
    empty and the climb does not run.  Otherwise the parent is the zipper
    root: the climb ends at the root, where
    [node_remove_all_branches] with limit 0 finds nothing in the emptied
    root node. *)
Lemma line_list_stopping_prune : forall slot0 slot1 leaf_byte outer_stops,
  (forall key,
     proper_prefix key [leaf_byte] ->
     ~ node_has_child_pointer_at (LineListNode slot0 slot1) key) ->
  subtree_forall node_kind_is_modelled (LineListNode slot0 slot1) ->
  subtree_forall line_list_node_is_valid (LineListNode slot0 slot1) ->
  node_has_path (LineListNode slot0 slot1) [leaf_byte] ->
  (forall value,
     ~ node_holds_value (LineListNode slot0 slot1) [leaf_byte] value) ->
  (forall byte,
     ~ node_has_path (LineListNode slot0 slot1) [leaf_byte; byte]) ->
  (outer_stops = [] \/
   exists sibling_byte,
     sibling_byte <> leaf_byte /\
     node_has_path (LineListNode slot0 slot1) [sibling_byte]) ->
  report_is_the_one_byte_prune (LineListNode slot0 slot1) leaf_byte
    (prune_path_outcome (LineListNode slot0 slot1) [leaf_byte] outer_stops).
Proof.
  intros slot0 slot1 leaf_byte outer_stops deepest modelled valid provided
    no_value childless stop.
  assert (nonempty : [leaf_byte] <> []) by discriminate.
  destruct (line_list_leaf_shape slot0 slot1 [leaf_byte] nonempty deepest
              (subtree_forall_here _ _ valid) provided no_value childless)
    as [carrying [other [emptied [roles [carrying_shape [pathless
                                                          other_off]]]]]].
  subst carrying.
  destruct (slot_roles_are_members slot0 slot1
              (ChildSlot [leaf_byte] emptied) other roles)
    as [carrying_member other_member].
  assert (emptied_modelled : node_kind_is_modelled emptied).
  { exact (subtree_forall_here _ _
             (member_slot_subtree _ slot0 slot1
                (ChildSlot [leaf_byte] emptied) [leaf_byte] emptied
                carrying_member eq_refl modelled)). }
  assert (emptied_valid : line_list_node_is_valid emptied).
  { exact (subtree_forall_here _ _
             (member_slot_subtree _ slot0 slot1
                (ChildSlot [leaf_byte] emptied) [leaf_byte] emptied
                carrying_member eq_refl valid)). }
  pose proof (pathless_node_is_empty emptied emptied_modelled emptied_valid
                pathless) as emptied_is_empty.
  destruct (leaf_pointer_summary [leaf_byte] emptied pathless)
    as [leaf_paths leaf_values].
  assert (other_paths : ~ slot_has_path other [leaf_byte]).
  { apply (slot_off_the_key_has_no_path_below other [leaf_byte] []
             other_off).
    exact (member_has_no_pointer_above slot0 slot1 other [leaf_byte]
             other_member deepest). }
  assert (other_is_used_or_root :
    slot_is_used other = true \/ outer_stops = []).
  { destruct stop as [at_root|[sibling_byte [different sibling]]];
      [right; exact at_root|].
    left.
    apply (node_path_by_slot_roles slot0 slot1
             (ChildSlot [leaf_byte] emptied) other [sibling_byte] roles)
      in sibling.
    destruct sibling as [empty|[carried|beside]].
    - discriminate empty.
    - exfalso. apply leaf_paths in carried.
      apply prefix_of_one_byte in carried.
      destruct carried as [empty|single]; [discriminate empty|].
      injection single as same. exact (different same).
    - destruct other as [|key stored|key child];
        [destruct beside | reflexivity | reflexivity]. }
  assert (computed :
    prune_path_outcome (LineListNode slot0 slot1) [leaf_byte] outer_stops =
    PruneReport 1 (LineListNode other UnusedSlot) [DetachedChild emptied]
      false).
  { assert (one_byte : pruned_key_bytes [leaf_byte] (slot_key other) = 1).
    { unfold pruned_key_bytes.
      rewrite (other_first_byte leaf_byte (slot_key other) other_off).
      reflexivity. }
    assert (dangling :
      node_remove_dangling (LineListNode slot0 slot1) [leaf_byte] =
      DanglingRemoval (LineListNode other UnusedSlot) 1
        [DetachedChild emptied]).
    { unfold node_remove_dangling.
      destruct roles as [[same0 same1]|[same1 same0]].
      - rewrite <- same0, <- same1.
        rewrite (dangling_in_slot0_computes other [leaf_byte] emptied
                   emptied_is_empty).
        rewrite one_byte. reflexivity.
      - rewrite <- same1, <- same0.
        rewrite (dangling_in_slot1_computes other [leaf_byte] emptied
                   other_off emptied_is_empty).
        rewrite one_byte. reflexivity. }
    unfold prune_path_outcome. rewrite dangling.
    cbv [dangling_pruned_bytes dangling_node dangling_detached].
    destruct other as [|key stored|key child].
    - destruct other_is_used_or_root as [other_used|at_root];
        [discriminate other_used|].
      subst outer_stops. reflexivity.
    - reflexivity.
    - reflexivity. }
  unfold report_is_the_one_byte_prune. rewrite computed.
  cbv [report_pruned_bytes report_node report_detached
       report_removed_above_node].
  split; [reflexivity|]. split; [reflexivity|].
  split.
  { intro path. split.
    - intros [empty|[beside|[]]].
      + subst path. split; [apply root_path_exists | discriminate].
      + split.
        * exact (member_slot_path slot0 slot1 other path other_member
                   beside).
        * intro same. subst path. exact (other_paths beside).
    - intros [provided_path different].
      apply (node_path_by_slot_roles slot0 slot1
               (ChildSlot [leaf_byte] emptied) other path roles)
        in provided_path.
      destruct provided_path as [empty|[carried|beside]].
      + left. exact empty.
      + apply leaf_paths in carried. apply prefix_of_one_byte in carried.
        destruct carried as [empty|single].
        * left. exact empty.
        * exfalso. exact (different single).
      + right. left. exact beside. }
  split.
  { intros path value. split.
    - intros [held|[]].
      exact (member_slot_value slot0 slot1 other path value other_member
               held).
    - intro held.
      apply (node_value_by_slot_roles slot0 slot1
               (ChildSlot [leaf_byte] emptied) other path value roles)
        in held.
      destruct held as [carried|beside].
      + exfalso. exact (leaf_values path value carried).
      + left. exact beside. }
  split.
  { right. exists emptied. split; [reflexivity | exact emptied_is_empty]. }
  rewrite detached_child_drop_nesting.
  exact (pathless_node_is_shallow emptied emptied_modelled emptied_valid
           pathless).
Qed.

(** [prune_path] on a dense focus node once [node_remove_dangling] has
    removed the cell of the leaf byte with count 1.  If cells remain, the
    climb does not run.  If none remains, the parent is the zipper root
    (a sibling would be a remaining cell), and the climb finds nothing in
    the emptied root node. *)
Lemma dense_prune_after_dangling :
  forall dense leaf_byte outer_stops detached,
    node_remove_dangling (DenseByteNode dense) [leaf_byte] =
    DanglingRemoval (DenseByteNode (set_cell dense leaf_byte AbsentCell)) 1
      detached ->
    (outer_stops = [] \/
     exists sibling_byte,
       sibling_byte <> leaf_byte /\
       node_has_path (DenseByteNode dense) [sibling_byte]) ->
    prune_path_outcome (DenseByteNode dense) [leaf_byte] outer_stops =
    PruneReport 1 (DenseByteNode (set_cell dense leaf_byte AbsentCell))
      detached false.
Proof.
  intros dense leaf_byte outer_stops detached dangling stop.
  unfold prune_path_outcome. rewrite dangling.
  cbv [dangling_pruned_bytes dangling_node dangling_detached].
  destruct (node_is_empty
              (DenseByteNode (set_cell dense leaf_byte AbsentCell)))
    eqn:emptied_node; [|reflexivity].
  destruct stop as [at_root|[sibling_byte [different sibling]]].
  - subst outer_stops.
    unfold node_remove_all_branches, dense_remove_all_branches.
    rewrite cell_at_set_cell_same.
    cbv [removal_node removal_detached].
    rewrite app_nil_r. reflexivity.
  - exfalso.
    pose proof (vacant_cells_are_absent
                  (set_cell dense leaf_byte AbsentCell) emptied_node
                  sibling_byte) as absent.
    rewrite (cell_at_set_cell_other leaf_byte sibling_byte dense AbsentCell
               different) in absent.
    destruct sibling as [empty|sibling]; [discriminate empty|].
    apply cells_have_path_at in sibling. rewrite absent in sibling.
    exact sibling.
Qed.

(** Removing the cell of the leaf byte from a dense node removes exactly
    that byte when the cell has no path below it and no value. *)
Lemma dense_cell_removal_is_the_one_byte_prune :
  forall dense leaf_byte detached,
    (forall rest,
       cell_has_path (cell_at dense leaf_byte) rest -> rest = []) ->
    (forall rest value,
       ~ cell_holds_value (cell_at dense leaf_byte) rest value) ->
    (detached = [] \/
     exists emptied,
       detached = [DetachedChild emptied] /\ node_is_empty emptied = true) ->
    detached_drop_nesting detached <= 1 ->
    report_is_the_one_byte_prune (DenseByteNode dense) leaf_byte
      (PruneReport 1 (DenseByteNode (set_cell dense leaf_byte AbsentCell))
         detached false).
Proof.
  intros dense leaf_byte detached cell_paths cell_values detached_shape
    shallow.
  assert (at_leaf_after :
    cell_at (set_cell dense leaf_byte AbsentCell) leaf_byte = AbsentCell).
  { apply cell_at_set_cell_same. }
  assert (elsewhere : forall byte,
    byte <> leaf_byte ->
    cell_at (set_cell dense leaf_byte AbsentCell) byte = cell_at dense byte).
  { intros byte different. apply cell_at_set_cell_other. exact different. }
  unfold report_is_the_one_byte_prune.
  cbv [report_pruned_bytes report_node report_detached
       report_removed_above_node].
  split; [reflexivity|]. split; [reflexivity|].
  split.
  { intros [|first rest].
    - split.
      + intros _. split; [apply root_path_exists | discriminate].
      + intros _. apply root_path_exists.
    - destruct (Nat.eq_dec first leaf_byte) as [same|different].
      + subst first. split.
        * intros [empty|after_path]; [discriminate empty|].
          apply cells_have_path_at in after_path.
          rewrite at_leaf_after in after_path. destruct after_path.
        * intros [[empty|before_path] other_path]; [discriminate empty|].
          apply cells_have_path_at in before_path.
          exfalso. apply other_path.
          rewrite (cell_paths rest before_path). reflexivity.
      + split.
        * intros [empty|after_path]; [discriminate empty|].
          apply cells_have_path_at in after_path.
          rewrite (elsewhere first different) in after_path.
          split.
          -- right. apply cells_have_path_at. exact after_path.
          -- intro same. injection same as same_first same_rest.
             exact (different same_first).
        * intros [[empty|before_path] _]; [discriminate empty|].
          apply cells_have_path_at in before_path.
          right. apply cells_have_path_at.
          rewrite (elsewhere first different). exact before_path. }
  split.
  { intros [|first rest] value.
    - split; intro held; exfalso.
      + exact (cells_hold_no_root_value
                 (set_cell dense leaf_byte AbsentCell) value held).
      + exact (cells_hold_no_root_value dense value held).
    - destruct (Nat.eq_dec first leaf_byte) as [same|different].
      + subst first. split; intro held; exfalso.
        * pose proof (proj1 (cells_hold_value_at
                               (set_cell dense leaf_byte AbsentCell)
                               leaf_byte rest value) held) as at_cell.
          rewrite at_leaf_after in at_cell. exact at_cell.
        * exact (cell_values rest value
                   (proj1 (cells_hold_value_at dense leaf_byte rest value)
                      held)).
      + split; intro held.
        * pose proof (proj1 (cells_hold_value_at
                               (set_cell dense leaf_byte AbsentCell)
                               first rest value) held) as at_cell.
          rewrite (elsewhere first different) in at_cell.
          exact (proj2 (cells_hold_value_at dense first rest value)
                   at_cell).
        * pose proof (proj1 (cells_hold_value_at dense first rest value)
                        held) as at_cell.
          apply (proj2 (cells_hold_value_at
                          (set_cell dense leaf_byte AbsentCell)
                          first rest value)).
          rewrite (elsewhere first different). exact at_cell. }
  split; [exact detached_shape | exact shallow].
Qed.

(** The focus node is a dense node.  The cell of the leaf byte has no
    value and no child pointer, or a pointer to a node without a path;
    [node_remove_dangling] removes the cell and returns 1. *)
Lemma dense_stopping_prune : forall dense leaf_byte outer_stops,
  subtree_forall node_kind_is_modelled (DenseByteNode dense) ->
  subtree_forall line_list_node_is_valid (DenseByteNode dense) ->
  node_has_path (DenseByteNode dense) [leaf_byte] ->
  (forall value,
     ~ node_holds_value (DenseByteNode dense) [leaf_byte] value) ->
  (forall byte, ~ node_has_path (DenseByteNode dense) [leaf_byte; byte]) ->
  (outer_stops = [] \/
   exists sibling_byte,
     sibling_byte <> leaf_byte /\
     node_has_path (DenseByteNode dense) [sibling_byte]) ->
  report_is_the_one_byte_prune (DenseByteNode dense) leaf_byte
    (prune_path_outcome (DenseByteNode dense) [leaf_byte] outer_stops).
Proof.
  intros dense leaf_byte outer_stops [_ cells_modelled] [_ cells_valid]
    provided no_value childless stop.
  destruct provided as [empty|provided]; [discriminate empty|].
  apply cells_have_path_at in provided.
  assert (cell_facts :
    exists detached,
      node_remove_dangling (DenseByteNode dense) [leaf_byte] =
      DanglingRemoval (DenseByteNode (set_cell dense leaf_byte AbsentCell))
        1 detached /\
      (forall rest,
         cell_has_path (cell_at dense leaf_byte) rest -> rest = []) /\
      (forall rest value,
         ~ cell_holds_value (cell_at dense leaf_byte) rest value) /\
      (detached = [] \/
       exists emptied,
         detached = [DetachedChild emptied] /\
         node_is_empty emptied = true) /\
      detached_drop_nesting detached <= 1).
  { unfold node_remove_dangling, dense_remove_dangling.
    revert provided.
    destruct (cell_at dense leaf_byte) as [|stored [emptied|]] eqn:at_leaf;
      intro provided.
    - destruct provided.
    - destruct stored as [value|].
      { exfalso. apply (no_value value).
        apply (proj2 (cells_hold_value_at dense leaf_byte [] value)).
        rewrite at_leaf. left. split; reflexivity. }
      assert (pathless : node_is_pathless emptied).
      { intros [|first rest] child_path; [reflexivity|]. exfalso.
        apply (childless first).
        apply (proj1 paths_are_closed_under_prefixes (DenseByteNode dense)
                 [leaf_byte; first] rest).
        right.
        apply (proj2 (cells_have_path_at dense leaf_byte (first :: rest))).
        rewrite at_leaf. exact child_path. }
      assert (emptied_modelled : node_kind_is_modelled emptied).
      { exact (subtree_forall_here _ _
                 (cells_subtree_forall_at _ dense leaf_byte None emptied
                    cells_modelled at_leaf)). }
      assert (emptied_valid : line_list_node_is_valid emptied).
      { exact (subtree_forall_here _ _
                 (cells_subtree_forall_at _ dense leaf_byte None emptied
                    cells_valid at_leaf)). }
      pose proof (pathless_node_is_empty emptied emptied_modelled
                    emptied_valid pathless) as emptied_is_empty.
      exists [DetachedChild emptied].
      split.
      { cbv beta iota. rewrite emptied_is_empty. reflexivity. }
      split.
      { intros rest cell_path. exact (pathless rest cell_path). }
      split.
      { intros rest value [[_ impossible]|[nonempty held]].
        - discriminate impossible.
        - apply nonempty. apply pathless.
          exact (proj1 a_value_lies_on_a_path emptied rest value held). }
      split.
      { right. exists emptied.
        split; [reflexivity | exact emptied_is_empty]. }
      rewrite detached_child_drop_nesting.
      exact (pathless_node_is_shallow emptied emptied_modelled
               emptied_valid pathless).
    - destruct stored as [value|].
      { exfalso. apply (no_value value).
        apply (proj2 (cells_hold_value_at dense leaf_byte [] value)).
        rewrite at_leaf. split; reflexivity. }
      exists [].
      split; [reflexivity|].
      split.
      { intros rest cell_path. exact cell_path. }
      split.
      { intros rest value [_ impossible]. discriminate impossible. }
      split; [left; reflexivity | simpl; apply Nat.le_0_l]. }
  destruct cell_facts
    as [detached [dangling [cell_paths [cell_values [detached_shape
                                                      shallow]]]]].
  rewrite (dense_prune_after_dangling dense leaf_byte outer_stops detached
             dangling stop).
  exact (dense_cell_removal_is_the_one_byte_prune dense leaf_byte detached
           cell_paths cell_values detached_shape shallow).
Qed.

(** The focus of Lemma 3: a leaf that exists, has no value and has no
    child. *)
Definition focus_is_childless_value_free_leaf (focus : zipper_focus)
    : Prop :=
  node_has_path (focus_node focus) (focus_node_key focus) /\
  (forall value,
     ~ node_holds_value (focus_node focus) (focus_node_key focus) value) /\
  (forall byte,
     ~ node_has_path (focus_node focus) (focus_node_key focus ++ [byte])).

(** The leaf's parent stops the run in Phase B: it is the zipper root (the
    leaf's depth is 1), or it has more than one child (the leaf's node key
    continues a parent key by the leaf byte, and the same parent key
    continued by another byte is a path of the focus node). *)
Definition focus_parent_stops_the_run (focus : zipper_focus) : Prop :=
  focus_depth focus = 1 \/
  exists parent_key leaf_byte sibling_byte,
    focus_node_key focus = parent_key ++ [leaf_byte] /\
    sibling_byte <> leaf_byte /\
    node_has_path (focus_node focus) (parent_key ++ [sibling_byte]).

(** Lemma 3 of the study.  Let the write zipper be rooted at the map's
    root and let its focus be a childless value-free leaf whose parent is
    the zipper root or has more than one child.  Then the node key of the
    leaf is its last byte alone, and [prune_path] removes exactly that
    byte: it returns 1, removes nothing above the focus node, leaves every
    other path and every value of the focus node in place, and detaches
    only the payload that represents the leaf, whose destructors nest at
    most one deep. *)
Theorem the_stopping_cut_is_one_byte : forall focus,
  pathmap_premises focus ->
  focus_is_childless_value_free_leaf focus ->
  focus_parent_stops_the_run focus ->
  exists leaf_byte,
    focus_node_key focus = [leaf_byte] /\
    report_pruned_bytes (focus_prune_outcome focus) = 1 /\
    report_removed_above_node (focus_prune_outcome focus) = false /\
    (forall path,
       node_has_path (report_node (focus_prune_outcome focus)) path <->
       node_has_path (focus_node focus) path /\ path <> [leaf_byte]) /\
    (forall path value,
       node_holds_value (report_node (focus_prune_outcome focus)) path
         value <->
       node_holds_value (focus_node focus) path value) /\
    (report_detached (focus_prune_outcome focus) = [] \/
     exists emptied,
       report_detached (focus_prune_outcome focus) =
       [DetachedChild emptied] /\
       node_is_empty emptied = true) /\
    detached_drop_nesting (report_detached (focus_prune_outcome focus)) <= 1.
Proof.
  intros [subject node_key outer_stops outcome]
    [deepest [modelled [valid [dense_key [_ transcribed]]]]]
    [provided [no_value childless]] stops.
  destruct deepest as [deepest root_only].
  cbv [focus_node focus_node_key focus_outer_stops focus_prune_outcome
       focus_depth dense_focus_key_is_one_byte prune_path_is_as_transcribed
       focus_parent_stops_the_run] in *.
  destruct (stopping_leaf_key_is_one_byte subject node_key outer_stops
              deepest root_only (subtree_forall_here _ _ valid) dense_key
              provided stops)
    as [leaf_byte [key_shape stop]].
  subst node_key outcome.
  exists leaf_byte. split; [reflexivity|].
  assert (report :
    report_is_the_one_byte_prune subject leaf_byte
      (prune_path_outcome subject [leaf_byte] outer_stops)).
  { destruct subject as [|slot0 slot1|dense|].
    - exfalso. destruct provided as [empty|[]]. discriminate empty.
    - exact (line_list_stopping_prune slot0 slot1 leaf_byte outer_stops
               deepest modelled valid provided no_value childless stop).
    - exact (dense_stopping_prune dense leaf_byte outer_stops modelled valid
               provided no_value childless stop).
    - exfalso. destruct provided as [empty|[]]. discriminate empty. }
  exact report.
Qed.

(** ** The stack bound of Theorem 1

    The study bounds the native stack of one cut by the constant of the
    navigation (premise P7) plus the destructor of the detached part: a
    recursive drop of nesting depth D needs at most f * D + c0, and the
    iterative path of the line-list destructor needs the constant S_iter
    (study, section 1.3).  The four costs are the fields of one record;
    the bound holds for every value of them. *)
Record frame_costs : Type := FrameCosts {
  navigation_stack : nat;
  destructor_frame : nat;
  destructor_constant : nat;
  iterative_drop_stack : nat
}.

(** The path that the destructor of a detached part takes. *)
Inductive destructor_path : Type :=
| RecursiveDrop : destructor_path
| IterativeDrop : destructor_path.

Definition drop_stack (costs : frame_costs) (path : destructor_path)
    (nesting : nat) : nat :=
  match path with
  | RecursiveDrop =>
      destructor_frame costs * nesting + destructor_constant costs
  | IterativeDrop => iterative_drop_stack costs
  end.

(** The stack of one cut: the navigation and the drop of what the cut
    detached. *)
Definition cut_stack (costs : frame_costs) (path : destructor_path)
    (detached : list detached_part) : nat :=
  navigation_stack costs +
  drop_stack costs path (detached_drop_nesting detached).

(** The bound of Theorem 1: S_nav + max (2 f + c0) S_iter. *)
Definition barrier_free_stack_bound (costs : frame_costs) : nat :=
  navigation_stack costs +
  Nat.max (2 * destructor_frame costs + destructor_constant costs)
    (iterative_drop_stack costs).

Lemma drop_stack_fits : forall costs path nesting,
  nesting <= 2 ->
  navigation_stack costs + drop_stack costs path nesting <=
  barrier_free_stack_bound costs.
Proof.
  intros costs [|] nesting shallow;
    unfold barrier_free_stack_bound, drop_stack.
  - pose proof (Nat.mul_le_mono_l nesting 2 (destructor_frame costs) shallow)
      as scaled.
    lia.
  - lia.
Qed.

(** The stack bound of Theorem 1 of the study, over the modelled frame
    costs.  Under the premises, whichever path the destructor takes, the
    cut at a parent that does not stop the run (Lemma 2) and the cut at a
    leaf whose parent stops it (Lemma 3) each need at most
    S_nav + max (2 f + c0) S_iter. *)
Theorem every_barrier_free_cut_fits_the_stack_bound :
  forall costs path focus,
    pathmap_premises focus ->
    (forall child_byte,
       focus_is_single_child_parent focus child_byte ->
       cut_stack costs path
         (cut_detached
            (remove_branches_without_prune (focus_node focus)
               (focus_node_key focus))) <=
       barrier_free_stack_bound costs) /\
    (focus_is_childless_value_free_leaf focus ->
     focus_parent_stops_the_run focus ->
     cut_stack costs path (report_detached (focus_prune_outcome focus)) <=
     barrier_free_stack_bound costs).
Proof.
  intros costs path focus premises. split.
  - intros child_byte parent.
    pose proof (the_cut_is_bounded focus child_byte premises parent)
      as bounded.
    cbv zeta in bounded.
    destruct bounded
      as [_ [_ [_ [_ [_ [_ [_ [detached [detached_shape shallow]]]]]]]]].
    unfold cut_stack. rewrite detached_shape.
    rewrite detached_child_drop_nesting.
    exact (drop_stack_fits costs path (node_drop_nesting detached) shallow).
  - intros leaf stops.
    destruct (the_stopping_cut_is_one_byte focus premises leaf stops)
      as [leaf_byte [_ [_ [_ [_ [_ [_ shallow]]]]]]].
    unfold cut_stack. apply (drop_stack_fits costs path).
    apply le_S. exact shallow.
Qed.

(** With the constants that the study measured (f = 80, c0 = 72 and
    S_iter = 144 bytes in the release build; 128, 64 and 263 in the debug
    build) the maximum is the recursive term, as the study states:
    S_nav + 232 and S_nav + 320 bytes. *)
Theorem measured_frame_costs_give_the_recursive_bound : forall navigation,
  barrier_free_stack_bound (FrameCosts navigation 80 72 144) =
  navigation + 232 /\
  barrier_free_stack_bound (FrameCosts navigation 128 64 263) =
  navigation + 320.
Proof. intro navigation. split; reflexivity. Qed.

(** ** Witnesses

    The hypotheses of the lemmas are satisfiable, and on small nodes the
    transcribed calls compute what the figures of the study show.
    [observed_focus] is a focus whose [prune_path] outcome is the
    transcribed one. *)
Definition observed_focus (subject : node) (node_key : list nat)
    (outer_stops : list bool) : zipper_focus :=
  ZipperFocus subject node_key outer_stops
    (prune_path_outcome subject node_key outer_stops).

Lemma proper_prefix_of_one_byte : forall key byte,
  proper_prefix key [byte] -> key = [].
Proof.
  intros [|first key_rest] byte [rest [nonempty shape]]; [reflexivity|].
  exfalso. simpl in shape. injection shape as same tail.
  symmetry in tail. apply app_eq_nil in tail.
  exact (nonempty (proj2 tail)).
Qed.

(** A node that satisfies P3 has no child pointer under the empty key. *)
Lemma valid_node_has_no_pointer_at_its_root : forall subject,
  line_list_node_is_valid subject -> ~ node_has_child_pointer_at subject [].
Proof.
  intros [|slot0 slot1|dense|] valid pointer.
  - exact pointer.
  - destruct valid as [key0_used [key1_used _]].
    destruct pointer as [[child shape]|[child shape]].
    + apply key0_used; rewrite shape; reflexivity.
    + apply key1_used; rewrite shape; reflexivity.
  - destruct pointer as [byte [stored [child [impossible _]]]].
    discriminate impossible.
  - exact pointer.
Qed.

(** With a one-byte node key the predicate follows from P2 and P3 on the
    subtree of the focus node and from the transcribed outcome. *)
Lemma observed_one_byte_focus_satisfies_the_premises :
  forall subject byte outer_stops,
    subtree_forall node_kind_is_modelled subject ->
    subtree_forall line_list_node_is_valid subject ->
    pathmap_premises (observed_focus subject [byte] outer_stops).
Proof.
  intros subject byte outer_stops modelled valid.
  apply pathmap_premises_from_p1_p2_p3_p6.
  - split.
    + intros key above.
      rewrite (proper_prefix_of_one_byte key byte above).
      exact (valid_node_has_no_pointer_at_its_root subject
               (subtree_forall_here _ _ valid)).
    + intro impossible. discriminate impossible.
  - exact modelled.
  - exact valid.
  - exact eq_refl.
Qed.

Lemma single_slot_node_is_valid : forall subject,
  (slot_is_used subject = true -> slot_key subject <> []) ->
  line_list_node_is_valid (LineListNode subject UnusedSlot).
Proof.
  intros subject key_used.
  split; [exact key_used|].
  split; [discriminate|].
  split.
  { intros _ [rest [nonempty shape]]. simpl in shape.
    symmetry in shape. apply app_eq_nil in shape.
    exact (nonempty (proj2 shape)). }
  split; [discriminate|].
  split; [discriminate|].
  split; [discriminate|].
  intros first second rest0 rest1 _ impossible. discriminate impossible.
Qed.

Lemma slots_with_different_first_bytes_are_valid :
  forall slot0 slot1 first0 rest0 first1 rest1,
    slot_key slot0 = first0 :: rest0 ->
    slot_key slot1 = first1 :: rest1 ->
    first0 <> first1 ->
    line_list_node_is_valid (LineListNode slot0 slot1).
Proof.
  intros slot0 slot1 first0 rest0 first1 rest1 key0 key1 different.
  unfold line_list_node_is_valid, line_list_slots_are_valid.
  rewrite key0, key1.
  split; [intros _; discriminate|].
  split; [intros _; discriminate|].
  split.
  { intros _ [rest [_ shape]]. simpl in shape.
    injection shape as same tail. apply different. symmetry. exact same. }
  split.
  { intros _ [rest [_ shape]]. simpl in shape.
    injection shape as same tail. exact (different same). }
  split.
  { intros _ _ same. injection same as same_first same_rest.
    exact (different same_first). }
  split.
  { intros _ byte tail0 tail1 shape0 shape1. exfalso.
    injection shape0 as same0 same_rest0.
    injection shape1 as same1 same_rest1.
    apply different. rewrite same0, same1. reflexivity. }
  intros first second tail0 tail1 shape0 shape1.
  injection shape0 as same0 same_rest0.
  injection shape1 as same1 same_rest1.
  apply different. rewrite same0, same1. reflexivity.
Qed.

(** Lemma 2, case (i): the key of the slot continues the node key by the
    child byte.  The focus is the path [1] in a root node whose one slot
    has the key [1; 2].  The cut shortens the key to [1] and swaps the
    pointer for the sentinel. *)
Definition node_with_the_child_in_the_slot_key : node :=
  LineListNode (ChildSlot [1; 2] EmptyNode) UnusedSlot.

Example the_cut_shortens_a_slot_key :
  pathmap_premises
    (observed_focus node_with_the_child_in_the_slot_key [1] []) /\
  focus_is_single_child_parent
    (observed_focus node_with_the_child_in_the_slot_key [1] []) 2 /\
  remove_branches_without_prune node_with_the_child_in_the_slot_key [1] =
  CutOutcome (LineListNode (ChildSlot [1] EmptyNode) UnusedSlot) true
    [DetachedChild EmptyNode] 0.
Proof.
  unfold node_with_the_child_in_the_slot_key.
  split; [|split; [|reflexivity]].
  - apply observed_one_byte_focus_satisfies_the_premises.
    + repeat split; discriminate.
    + split; [|repeat split].
      apply single_slot_node_is_valid. discriminate.
  - cbv [observed_focus focus_is_single_child_parent focus_depth focus_node
         focus_node_key focus_outer_stops single_child_parent_at].
    cbn [app].
    split; [discriminate|].
    split; [|split; [|split]].
    + intros value [[rest [_ [_ []]]]|[]].
    + intro byte. split.
      * intros [empty|[[under|[rest [split_here [none|[]]]]]|[]]].
        -- discriminate empty.
        -- destruct under as [more shape]. simpl in shape. congruence.
        -- subst rest. simpl in split_here. congruence.
      * intro same. subst byte. right. left. left. exists [].
        reflexivity.
    + intros value [[rest [_ [_ []]]]|[]].
    + intros byte [empty|[[under|[rest [split_here [none|[]]]]]|[]]].
      * discriminate empty.
      * destruct under as [more shape]. simpl in shape.
        discriminate shape.
      * subst rest. simpl in split_here. discriminate split_here.
Qed.

(** Lemma 2, case (ii): a slot holds a child pointer under exactly the
    node key, and the node below it holds the dangling child byte.  The
    cut swaps the pointer for the sentinel and leaves the other slot. *)
Definition node_with_the_child_below_a_pointer : node :=
  LineListNode
    (ChildSlot [1] (LineListNode (ChildSlot [2] EmptyNode) UnusedSlot))
    (ChildSlot [3] EmptyNode).

Example the_cut_replaces_a_pointer_at_the_node_key :
  pathmap_premises
    (observed_focus node_with_the_child_below_a_pointer [1] []) /\
  focus_is_single_child_parent
    (observed_focus node_with_the_child_below_a_pointer [1] []) 2 /\
  remove_branches_without_prune node_with_the_child_below_a_pointer [1] =
  CutOutcome
    (LineListNode (ChildSlot [1] EmptyNode) (ChildSlot [3] EmptyNode)) true
    [DetachedChild (LineListNode (ChildSlot [2] EmptyNode) UnusedSlot)] 0.
Proof.
  unfold node_with_the_child_below_a_pointer.
  split; [|split; [|reflexivity]].
  - apply observed_one_byte_focus_satisfies_the_premises.
    + repeat split; discriminate.
    + split; [|split].
      * apply (slots_with_different_first_bytes_are_valid _ _ 1 [] 3 []);
          [reflexivity | reflexivity | discriminate].
      * split; [|repeat split].
        apply single_slot_node_is_valid. discriminate.
      * repeat split.
  - cbv [observed_focus focus_is_single_child_parent focus_depth focus_node
         focus_node_key focus_outer_stops single_child_parent_at].
    cbn [app].
    split; [discriminate|].
    split; [|split; [|split]].
    + intros value
        [[rest [_ [_ [[inner [_ [_ []]]]|[]]]]]|[rest [_ [_ []]]]].
    + intro byte. split.
      * intros [empty|[[under|[rest [split_here child_path]]]
                      |[under|[rest [split_here _]]]]].
        -- discriminate empty.
        -- destruct under as [more shape]. simpl in shape.
           discriminate shape.
        -- simpl in split_here.
           assert (rest_is : rest = [byte]) by congruence. subst rest.
           destruct child_path
             as [inner_empty
                |[[inner_under|[inner_rest [inner_split _]]]|[]]].
           ++ discriminate inner_empty.
           ++ destruct inner_under as [more shape]. simpl in shape.
              congruence.
           ++ simpl in inner_split. congruence.
        -- destruct under as [more shape]. simpl in shape.
           discriminate shape.
        -- simpl in split_here. discriminate split_here.
      * intro same. subst byte. right. left. right. exists [2].
        split; [reflexivity|]. right. left. left. exists [].
        reflexivity.
    + intros value
        [[rest [_ [_ [[inner [_ [_ []]]]|[]]]]]|[rest [_ [_ []]]]].
    + intros byte [empty|[[under|[rest [split_here child_path]]]
                         |[under|[rest [split_here _]]]]].
      * discriminate empty.
      * destruct under as [more shape]. simpl in shape.
        discriminate shape.
      * simpl in split_here.
        assert (rest_is : rest = [2; byte]) by congruence. subst rest.
        destruct child_path
          as [inner_empty
             |[[inner_under|[inner_rest [inner_split [inner_none|[]]]]]
              |[]]].
        -- discriminate inner_empty.
        -- destruct inner_under as [more shape]. simpl in shape.
           discriminate shape.
        -- subst inner_rest. simpl in inner_split.
           discriminate inner_split.
      * destruct under as [more shape]. simpl in shape.
        discriminate shape.
      * simpl in split_here. discriminate split_here.
Qed.

(** Lemma 2, case (iii): the focus node is a dense node, the node key is
    the byte 1, and its cell points to the node that holds the dangling
    child byte.  The cut clears the pointer and keeps the cell. *)
Definition dense_node_with_the_child_below_a_cell : node :=
  DenseByteNode
    (VacantCell
       (BranchCell None (LineListNode (ChildSlot [2] EmptyNode) UnusedSlot)
          NoCells)).

Example the_cut_clears_a_dense_child_pointer :
  pathmap_premises
    (observed_focus dense_node_with_the_child_below_a_cell [1] []) /\
  focus_is_single_child_parent
    (observed_focus dense_node_with_the_child_below_a_cell [1] []) 2 /\
  remove_branches_without_prune dense_node_with_the_child_below_a_cell [1] =
  CutOutcome (DenseByteNode (VacantCell (LeafCell None NoCells))) true
    [DetachedChild (LineListNode (ChildSlot [2] EmptyNode) UnusedSlot)] 0.
Proof.
  unfold dense_node_with_the_child_below_a_cell.
  split; [|split; [|reflexivity]].
  - apply observed_one_byte_focus_satisfies_the_premises.
    + repeat split; discriminate.
    + split; [exact I|]. split; [|exact I]. split; [|repeat split].
      apply single_slot_node_is_valid. discriminate.
  - cbv [observed_focus focus_is_single_child_parent focus_depth focus_node
         focus_node_key focus_outer_stops single_child_parent_at].
    cbn [app].
    split; [discriminate|].
    split; [|split; [|split]].
    + intros value [[_ impossible]|[nonempty _]].
      * discriminate impossible.
      * apply nonempty. reflexivity.
    + intro byte. split.
      * intros [empty|[empty|[[under|[rest [split_here _]]]|[]]]].
        -- discriminate empty.
        -- discriminate empty.
        -- destruct under as [more shape]. simpl in shape. congruence.
        -- simpl in split_here. congruence.
      * intro same. subst byte. right. right. left. left. exists [].
        reflexivity.
    + intros value [[impossible _]|[_ [[inner [_ [_ []]]]|[]]]].
      discriminate impossible.
    + intros byte
        [empty|[empty|[[under|[rest [split_here [none|[]]]]]|[]]]].
      * discriminate empty.
      * discriminate empty.
      * destruct under as [more shape]. simpl in shape.
        discriminate shape.
      * subst rest. simpl in split_here. discriminate split_here.
Qed.

(** Lemma 3 at the zipper root: the leaf 1 is the only child of the root.
    [prune_path] takes the slot, the root node is empty afterwards, and
    the climb finds nothing more to remove. *)
Definition root_node_with_one_leaf : node :=
  LineListNode (ChildSlot [1] EmptyNode) UnusedSlot.

Example the_stopping_cut_at_the_root_empties_the_root_node :
  pathmap_premises (observed_focus root_node_with_one_leaf [1] []) /\
  focus_is_childless_value_free_leaf
    (observed_focus root_node_with_one_leaf [1] []) /\
  focus_parent_stops_the_run
    (observed_focus root_node_with_one_leaf [1] []) /\
  prune_path_outcome root_node_with_one_leaf [1] [] =
  PruneReport 1 (LineListNode UnusedSlot UnusedSlot)
    [DetachedChild EmptyNode] false.
Proof.
  unfold root_node_with_one_leaf.
  split; [|split; [|split; [|reflexivity]]].
  - apply observed_one_byte_focus_satisfies_the_premises.
    + repeat split; discriminate.
    + split; [|repeat split].
      apply single_slot_node_is_valid. discriminate.
  - cbv [observed_focus focus_is_childless_value_free_leaf focus_node
         focus_node_key].
    cbn [app].
    split; [|split].
    + right. left. left. exists []. reflexivity.
    + intros value [[rest [_ [_ []]]]|[]].
    + intros byte [empty|[[under|[rest [split_here [none|[]]]]]|[]]].
      * discriminate empty.
      * destruct under as [more shape]. simpl in shape.
        discriminate shape.
      * subst rest. simpl in split_here. discriminate split_here.
  - left. reflexivity.
Qed.

(** Lemma 3 at a fork: the leaf 1 has the sibling 3 in the same node.
    [prune_path] takes the slot of the leaf; the sibling keeps the node
    non-empty, so the climb does not run. *)
Definition node_with_a_fork_at_its_root : node :=
  LineListNode (ChildSlot [1] EmptyNode) (ChildSlot [3] EmptyNode).

Example the_stopping_cut_at_a_fork_keeps_the_sibling :
  pathmap_premises
    (observed_focus node_with_a_fork_at_its_root [1] [true]) /\
  focus_is_childless_value_free_leaf
    (observed_focus node_with_a_fork_at_its_root [1] [true]) /\
  focus_parent_stops_the_run
    (observed_focus node_with_a_fork_at_its_root [1] [true]) /\
  prune_path_outcome node_with_a_fork_at_its_root [1] [true] =
  PruneReport 1 (LineListNode (ChildSlot [3] EmptyNode) UnusedSlot)
    [DetachedChild EmptyNode] false.
Proof.
  unfold node_with_a_fork_at_its_root.
  split; [|split; [|split; [|reflexivity]]].
  - apply observed_one_byte_focus_satisfies_the_premises.
    + repeat split; discriminate.
    + split; [|repeat split].
      apply (slots_with_different_first_bytes_are_valid _ _ 1 [] 3 []);
        [reflexivity | reflexivity | discriminate].
  - cbv [observed_focus focus_is_childless_value_free_leaf focus_node
         focus_node_key].
    cbn [app].
    split; [|split].
    + right. left. left. exists []. reflexivity.
    + intros value [[rest [_ [_ []]]]|[rest [_ [_ []]]]].
    + intros byte [empty|[[under|[rest [split_here [none|[]]]]]
                         |[under|[rest [split_here _]]]]].
      * discriminate empty.
      * destruct under as [more shape]. simpl in shape.
        discriminate shape.
      * subst rest. simpl in split_here. discriminate split_here.
      * destruct under as [more shape]. simpl in shape.
        discriminate shape.
      * simpl in split_here. discriminate split_here.
  - right. exists [], 1, 3. split; [reflexivity|].
    split; [discriminate|].
    right. right. left. exists []. reflexivity.
Qed.

(** Lemma 3 in a dense node: the leaf 1 has the sibling 2, both cells
    without a value and without a child pointer.  [prune_path] removes
    the cell of the leaf and detaches nothing that has a destructor. *)
Definition dense_node_with_a_fork_at_its_root : node :=
  DenseByteNode (VacantCell (LeafCell None (LeafCell None NoCells))).

Example the_stopping_cut_in_a_dense_node_removes_one_cell :
  pathmap_premises
    (observed_focus dense_node_with_a_fork_at_its_root [1] [true]) /\
  focus_is_childless_value_free_leaf
    (observed_focus dense_node_with_a_fork_at_its_root [1] [true]) /\
  focus_parent_stops_the_run
    (observed_focus dense_node_with_a_fork_at_its_root [1] [true]) /\
  prune_path_outcome dense_node_with_a_fork_at_its_root [1] [true] =
  PruneReport 1
    (DenseByteNode (VacantCell (VacantCell (LeafCell None NoCells)))) []
    false.
Proof.
  unfold dense_node_with_a_fork_at_its_root.
  split; [|split; [|split; [|reflexivity]]].
  - apply observed_one_byte_focus_satisfies_the_premises.
    + repeat split; discriminate.
    + split; exact I.
  - cbv [observed_focus focus_is_childless_value_free_leaf focus_node
         focus_node_key].
    cbn [app].
    split; [|split].
    + right. exact eq_refl.
    + intros value [_ impossible]. discriminate impossible.
    + intros byte [empty|empty]; discriminate empty.
  - right. exists [], 1, 2. split; [reflexivity|].
    split; [discriminate|].
    right. exact eq_refl.
Qed.

Print Assumptions prefix_reflexive.
Print Assumptions prefix_of_extension.
Print Assumptions self_extension_is_empty.
Print Assumptions extension_is_not_empty.
Print Assumptions extension_is_not_a_prefix.
Print Assumptions app_split_cases.
Print Assumptions prefix_of_one_byte_extension.
Print Assumptions starts_with_spec.
Print Assumptions starts_with_false.
Print Assumptions starts_with_extension.
Print Assumptions key_eqb_spec.
Print Assumptions key_eqb_refl.
Print Assumptions key_eqb_false.
Print Assumptions nonempty_key_is_not_empty.
Print Assumptions other_first_byte.
Print Assumptions firstn_extension.
Print Assumptions head_of_with_head_cell.
Print Assumptions higher_of_with_head_cell.
Print Assumptions cell_at_no_cells.
Print Assumptions cell_at_set_cell_same.
Print Assumptions cell_at_set_cell_other.
Print Assumptions root_path_exists.
Print Assumptions cells_have_no_root_path.
Print Assumptions cells_hold_no_root_value.
Print Assumptions cells_have_path_at.
Print Assumptions cells_hold_value_at.
Print Assumptions paths_are_closed_under_prefixes.
Print Assumptions a_value_lies_on_a_path.
Print Assumptions vacant_cells_are_absent.
Print Assumptions absent_cells_are_vacant.
Print Assumptions cells_drop_nesting_bound.
Print Assumptions subtree_forall_here.
Print Assumptions cells_subtree_forall_at.
Print Assumptions used_slot_with_a_key_holds_no_root_value.
Print Assumptions valid_subtrees_hold_no_root_value.
Print Assumptions slot_roles_are_members.
Print Assumptions node_path_by_slot_roles.
Print Assumptions node_value_by_slot_roles.
Print Assumptions member_slot_path.
Print Assumptions member_slot_value.
Print Assumptions member_slot_key_is_not_empty.
Print Assumptions member_slot_subtree.
Print Assumptions slot_without_use_is_unused.
Print Assumptions slot_key_prefix_is_slot_path.
Print Assumptions slot_path_cases.
Print Assumptions pathless_node_is_empty_and_shallow.
Print Assumptions pathless_node_is_empty.
Print Assumptions pathless_node_is_shallow.
Print Assumptions extension_within_one_byte.
Print Assumptions dangling_byte_node_is_shallow.
Print Assumptions detached_child_drop_nesting.
Print Assumptions focus_below_the_root_has_a_node_key.
Print Assumptions deepest_focus_in_a_dense_node_has_a_one_byte_key.
Print Assumptions pathmap_premises_from_p1_p2_p3_p6.
Print Assumptions key_under_the_parent_key.
Print Assumptions slot_under_the_parent_key_carries_the_child.
Print Assumptions two_slots_under_the_parent_key_are_invalid.
Print Assumptions member_child_pointer.
Print Assumptions member_has_no_pointer_above.
Print Assumptions slot_off_the_key_has_no_path_below.
Print Assumptions two_byte_extension.
Print Assumptions node_below_the_parent_holds_only_the_dangling_byte.
Print Assumptions member_with_the_child_path_carries_the_cut.
Print Assumptions line_list_cut_shape.
Print Assumptions one_slot_carries_the_cut.
Print Assumptions cut_in_slot0_computes.
Print Assumptions cut_in_slot1_computes.
Print Assumptions carrying_slot_summary.
Print Assumptions cut_slot_summary.
Print Assumptions line_list_cut_is_bounded.
Print Assumptions dense_cut_is_bounded.
Print Assumptions the_cut_is_bounded.
Print Assumptions prefix_transitive.
Print Assumptions one_key_extends_one_child.
Print Assumptions proper_prefix_of_sibling.
Print Assumptions keys_of_two_children_are_unrelated.
Print Assumptions slot_under_a_nonempty_key_is_used.
Print Assumptions member_path_without_pointer_above_lies_under_its_key.
Print Assumptions path_without_pointer_above_lies_under_a_slot_key.
Print Assumptions two_children_inside_a_node_are_invalid.
Print Assumptions stopping_leaf_key_is_one_byte.
Print Assumptions slot_under_a_leaf_key_is_its_child_pointer.
Print Assumptions line_list_leaf_shape.
Print Assumptions dangling_in_slot0_computes.
Print Assumptions dangling_in_slot1_computes.
Print Assumptions leaf_pointer_summary.
Print Assumptions prefix_of_one_byte.
Print Assumptions line_list_stopping_prune.
Print Assumptions dense_prune_after_dangling.
Print Assumptions dense_cell_removal_is_the_one_byte_prune.
Print Assumptions dense_stopping_prune.
Print Assumptions the_stopping_cut_is_one_byte.
Print Assumptions drop_stack_fits.
Print Assumptions every_barrier_free_cut_fits_the_stack_bound.
Print Assumptions measured_frame_costs_give_the_recursive_bound.
Print Assumptions proper_prefix_of_one_byte.
Print Assumptions valid_node_has_no_pointer_at_its_root.
Print Assumptions observed_one_byte_focus_satisfies_the_premises.
Print Assumptions single_slot_node_is_valid.
Print Assumptions slots_with_different_first_bytes_are_valid.
Print Assumptions the_cut_shortens_a_slot_key.
Print Assumptions the_cut_replaces_a_pointer_at_the_node_key.
Print Assumptions the_cut_clears_a_dense_child_pointer.
Print Assumptions the_stopping_cut_at_the_root_empties_the_root_node.
Print Assumptions the_stopping_cut_at_a_fork_keeps_the_sibling.
Print Assumptions the_stopping_cut_in_a_dense_node_removes_one_cell.

End BarrierFreeNodeRefinement.

Print Assumptions every_barrier_free_step_detaches_exactly_one_logical_edge.
Print Assumptions barrier_free_step_classification.
Print Assumptions barrier_free_segment_is_one_logical_edge.
Print Assumptions barrier_free_segment_prunes_one_logical_edge.
Print Assumptions barrier_free_segment_detaches_no_original.
Print Assumptions barrier_free_segment_queues_no_par_job.
Print Assumptions barrier_free_segment_installs_no_barrier.
Print Assumptions drain_node_detaches_originals_in_reverse_postorder.
Print Assumptions drain_node_prunes_every_logical_edge_once.
Print Assumptions drain_node_transfers_every_par_to_one_explicit_pda_job.
Print Assumptions barrier_free_segment_has_no_phase_a_value_event.
Print Assumptions drain_node_contains_no_real_value_operation.
Print Assumptions topology_phase_contains_no_real_value_operation.
Print Assumptions value_free_drain_is_one_prune_event_per_logical_edge.
Print Assumptions phase_b_emits_one_prune_event_per_logical_edge_and_nothing_else.
Print Assumptions phase_b_events_are_the_begin_marker_and_one_edge_prunes.
Print Assumptions phase_b_writes_no_value.
Print Assumptions barrier_free_run_cut_step.
Print Assumptions barrier_free_run_prune_step.
Print Assumptions barrier_free_run_detaches_one_edge_per_step_and_drops_no_payload.
Print Assumptions every_run_cuts_up_to_its_first_stopping_parent_and_prunes_there.
Print Assumptions absent_parent_is_cut_or_pruned_without_a_barrier.
Print Assumptions real_parent_value_ends_the_run_without_a_barrier.
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
Print Assumptions invariant_catalog_has_exactly_thirty_three_entries.
Print Assumptions invariant_catalog_is_unique_complete_and_exact.
Print Assumptions campaign_branch_safe_pathmap_teardown_capstone.
Print Assumptions live_prune_run_detaches_one_logical_edge_per_step.
Print Assumptions live_prune_run_respects_the_floor.
Print Assumptions live_removal_run_stops_at_the_floor_a_real_value_or_a_fork.
Print Assumptions live_removal_retires_every_installed_barrier.
Print Assumptions live_removal_disposes_exactly_one_real_value.
Print Assumptions live_removal_queues_no_par_job.
Print Assumptions exclusive_footprint_release_preserves_external_images.
Print Assumptions live_removal_run_preserves_external_images.
Print Assumptions exclusive_provenance_licenses_live_removal.
Print Assumptions live_removal_model_is_nonvacuous.

End PathMapReversePruneTeardown.
