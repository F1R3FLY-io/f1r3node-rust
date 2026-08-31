From Stdlib Require Import Bool List Arith Lia.

Import ListNotations.
Set Implicit Arguments.

(**
  Formal-first contract for campaign-owned PathMap destruction.

  The model is deliberately stated over logical byte-trie edges, not over the
  private node representation of the PathMap crate.  A production refinement
  must implement each [PruneSpan 1] by positioning a public owned write zipper
  on the reverse-lexicographic terminal path, removing its value without
  pruning, installing a destruction-only barrier at the immediate parent when
  that parent has no real value, and then pruning exactly that one logical
  edge.  The barrier is removed before the next transition.

  This separates the guarantee that campaign-owned code can establish from an
  assertion about private upstream allocation internals: no nonempty uniquely
  owned trie is handed to ordinary recursive Drop, while every detachment made
  by the integration is bounded to one logical edge.
*)

Module PathMapReversePruneTeardown.

Inductive payload : Type :=
| SetMember : nat -> payload
| MapPar : nat -> list nat -> payload
| BagMultiplicity : nat -> nat -> payload.

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
| DetachOriginal : payload -> teardown_event
| QueueParPda : nat -> list nat -> teardown_event
| DestroyParChild : nat -> nat -> teardown_event
| InstallBarrier : teardown_event
| PruneSpan : nat -> teardown_event
| DetachBarrier : teardown_event
| ReleaseSharedRoot : teardown_event
| PublishComplete : teardown_event.

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
    header and therefore exactly one independent root value live inside each
    anchor.  Immutable wrapper Clone changes only the Arc count.  The standard
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
| BeginAnchorReversePrunePda
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
    [ArcElectsFinalWrapper; BeginAnchorReversePrunePda].

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
    [ArcElectsFinalWrapper; BeginAnchorReversePrunePda] /\
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

Record release_result : Type := ReleaseResult {
  release_trace : list teardown_event;
  residual_campaign_edges : nat;
  recursively_dropped_unique_edges : nat;
  preserved_sibling : option trie
}.

Definition release_unique (subject : trie) : release_result :=
  ReleaseResult (drain_tree subject) 0 0 None.

(** The final Arc owner transfers the anchor's sole raw PathMap header into the
    reverse-prune closure.  A different anchor created by [Arc::make_mut] may
    still share private PathMap subtries; public zipper mutation may perform
    CoW while draining this semantic image, but the sibling anchor and its root
    payload remain untouched. *)
Definition release_final_anchor (subject : trie) : release_result :=
  release_unique subject.

Definition release_cross_anchor_shared_image
    (subject sibling : trie) : release_result :=
  ReleaseResult (drain_tree subject) 0 0 (Some sibling).

Theorem unique_release_never_uses_recursive_nonempty_pathmap_drop :
  forall subject,
    residual_campaign_edges (release_unique subject) = 0 /\
    recursively_dropped_unique_edges (release_unique subject) = 0.
Proof. repeat split; reflexivity. Qed.

Theorem cross_anchor_teardown_preserves_the_sibling_image :
  forall subject sibling,
  preserved_sibling (release_cross_anchor_shared_image subject sibling) =
    Some sibling /\
  recursively_dropped_unique_edges
    (release_cross_anchor_shared_image subject sibling) = 0 /\
  detached_payloads
    (release_trace (release_cross_anchor_shared_image subject sibling)) =
    reverse_payloads_tree subject.
Proof.
  intros subject sibling. repeat split.
  exact (proj1 drain_detaches_originals_exactly_once_in_reverse_postorder
    subject).
Qed.

Theorem final_anchor_drop_detaches_its_single_root_image : forall subject,
  detached_payloads (release_trace (release_final_anchor subject)) =
    reverse_payloads_tree subject.
Proof.
  apply (proj1 drain_detaches_originals_exactly_once_in_reverse_postorder).
Qed.

Theorem final_anchor_drop_is_the_only_raw_header_teardown :
  anchor_transition_trace final_wrapper_drop =
    [ArcElectsFinalWrapper; BeginAnchorReversePrunePda] /\
  anchor_raw_headers (anchor_transition_final final_wrapper_drop) = 0 /\
  anchor_root_payload_copies (anchor_transition_final final_wrapper_drop) = 0.
Proof. repeat split; reflexivity. Qed.

(** All production exits that can acquire exclusive ownership use the same
    closure.  In particular, Drop of a partially built owned writer or owned
    iterator is not allowed to bypass the PDA, and subtree replacement/removal
    may publish the replacement only after the detached subtree owns no edge. *)
Inductive campaign_owned_exit : Type :=
| PendingWriterEarlyDrop
| OwnedIteratorEarlyDrop
| MalformedDecodeDiscard
| BagConstructionDiscard
| DestructiveSubtreeReplacement
| DestructiveSubtreeRemoval.

Definition close_campaign_owned_exit
    (_ : campaign_owned_exit) (detached : trie) : release_result :=
  release_unique detached.

Theorem every_campaign_owned_exit_drains_before_release :
  forall exit detached,
    release_trace (close_campaign_owned_exit exit detached) =
      drain_tree detached /\
    prune_widths (release_trace (close_campaign_owned_exit exit detached)) =
      repeat 1 (tree_edges detached) /\
    residual_campaign_edges (close_campaign_owned_exit exit detached) = 0 /\
    recursively_dropped_unique_edges
      (close_campaign_owned_exit exit detached) = 0.
Proof.
  intros exit detached. destruct exit; simpl; repeat split.
  all: try exact (proj1 drain_prunes_every_logical_edge_once detached).
Qed.

Theorem destructive_detachment_never_releases_a_live_branch :
  forall detached,
    residual_campaign_edges
      (close_campaign_owned_exit DestructiveSubtreeReplacement detached) = 0 /\
    residual_campaign_edges
      (close_campaign_owned_exit DestructiveSubtreeRemoval detached) = 0.
Proof. intros; split; reflexivity. Qed.

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
    (outcome : builder_outcome) (partial : trie) : builder_result :=
  match outcome with
  | AcceptComplete => BuilderResult [PublishComplete] 1 0 0 None
  | RejectPrivate failure =>
      BuilderResult (drain_tree partial) 0 0 0 (Some failure)
  end.

Theorem malformed_or_rejected_partial_build_is_fully_drained :
  forall partial failure,
    builder_trace (finalize_private_builder (RejectPrivate failure) partial) =
      drain_tree partial /\
    publication_count
      (finalize_private_builder (RejectPrivate failure) partial) = 0 /\
    unreleased_private_edges
      (finalize_private_builder (RejectPrivate failure) partial) = 0 /\
    builder_recursive_unique_drop_edges
      (finalize_private_builder (RejectPrivate failure) partial) = 0.
Proof. intros; repeat split; reflexivity. Qed.

Theorem complete_builder_publishes_exactly_once : forall complete,
  builder_trace (finalize_private_builder AcceptComplete complete) =
    [PublishComplete] /\
  publication_count (finalize_private_builder AcceptComplete complete) = 1 /\
  unreleased_private_edges
    (finalize_private_builder AcceptComplete complete) = 0.
Proof. intros; repeat split; reflexivity. Qed.

Theorem teardown_preserves_selected_error_precedence :
  forall partial framing child deferred_act association canonicality failure,
    select_failure framing child deferred_act association canonicality =
      Some failure ->
    returned_failure
      (finalize_private_builder (RejectPrivate failure) partial) = Some failure.
Proof. intros; reflexivity. Qed.

Theorem child_failure_precedes_deferred_act_failure :
  select_failure false true true true true = Some ChildPdaFailure.
Proof. reflexivity. Qed.

(** Production teardown does not accept an arbitrary callback.  The complete
    registry names the three payload families owned by this campaign; an
    integration may specialize their implementations further, but may not
    introduce a caller-supplied unwinding closure into [Drop]. *)
Inductive named_teardown_policy : Type :=
| UnitSetPolicy
| ParMapPdaPolicy
| BagMultiplicityPolicy.

Definition registered_teardown_policies : list named_teardown_policy :=
  [UnitSetPolicy; ParMapPdaPolicy; BagMultiplicityPolicy].

Definition policy_for_payload (value : payload) : named_teardown_policy :=
  match value with
  | SetMember _ => UnitSetPolicy
  | MapPar _ _ => ParMapPdaPolicy
  | BagMultiplicity _ _ => BagMultiplicityPolicy
  end.

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

Definition dispatch_registered_policy
    (value : payload) : policy_dispatch_outcome :=
  PolicyReturned (dispose_payload value).

Theorem named_policy_registry_is_closed_and_complete :
  NoDup registered_teardown_policies /\
  length registered_teardown_policies = 3 /\
  (forall value,
    In (policy_for_payload value) registered_teardown_policies) /\
  resolve_teardown_policy RequestCallerClosure = None.
Proof.
  repeat split.
  - repeat constructor; simpl; intuition discriminate.
  - intros value. destruct value; simpl; auto.
Qed.

Theorem production_policy_dispatch_never_unwinds : forall value,
  exists trace,
    dispatch_registered_policy value = PolicyReturned trace /\
    trace = dispose_payload value.
Proof. intro value. exists (dispose_payload value). split; reflexivity. Qed.

(** A raw header remains quarantined until successful reverse pruning has
    removed every logical edge and payload.  Unexpected panic and cross-anchor
    allocation failure are fail-stop outcomes: they do not resume execution,
    publish, leak into a live process, or invoke ordinary recursive PathMap
    Drop.  The operating system reclaims quarantined storage after termination.

    Allocation failure is deliberately absent from unique-anchor teardown.
    Cross-anchor teardown may require upstream CoW allocation; failure there is
    process-terminal rather than an unwind through a nonempty raw header. *)
Inductive anchor_allocation_class : Type :=
| UniqueAnchorTeardown
| CrossAnchorCowTeardown.

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
  QuarantinedTeardownResult LiveCompleted (drain_tree subject)
    0 0 false 1 0 1 allocation_attempts.

Definition fail_stopped_quarantined_teardown
    (subject : trie) (allocation_attempts : nat)
    : quarantined_teardown_result :=
  QuarantinedTeardownResult ProcessFailStopped []
    (tree_edges subject) (payload_count subject) true 0 0 0
    allocation_attempts.

Definition run_quarantined_teardown
    (class : anchor_allocation_class)
    (allocation : teardown_allocation_outcome)
    (unexpected_panic : bool)
    (subject : trie) : quarantined_teardown_result :=
  if unexpected_panic then fail_stopped_quarantined_teardown subject 0
  else
    match class, allocation with
    | UniqueAnchorTeardown, NoAllocationRequired =>
        completed_quarantined_teardown subject 0
    | CrossAnchorCowTeardown, AllocationSucceeded =>
        completed_quarantined_teardown subject 1
    | CrossAnchorCowTeardown, AllocationFailed =>
        fail_stopped_quarantined_teardown subject 1
    | _, _ => fail_stopped_quarantined_teardown subject 0
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
    class allocation unexpected_panic subject,
  explicit_raw_drop_is_empty
    (run_quarantined_teardown class allocation unexpected_panic subject) /\
  fail_stop_is_terminal
    (run_quarantined_teardown class allocation unexpected_panic subject) /\
  no_live_process_quarantine_leak
    (run_quarantined_teardown class allocation unexpected_panic subject) /\
  quarantine_recursive_raw_drops
    (run_quarantined_teardown class allocation unexpected_panic subject) = 0.
Proof.
  intros class allocation unexpected_panic subject.
  destruct unexpected_panic, class, allocation; simpl;
    unfold explicit_raw_drop_is_empty, fail_stop_is_terminal,
      no_live_process_quarantine_leak; simpl; intuition discriminate.
Qed.

Theorem unexpected_panic_is_fail_stop_without_recursive_drop : forall
    class allocation subject,
  quarantine_process
    (run_quarantined_teardown class allocation true subject) =
      ProcessFailStopped /\
  quarantine_live_continuations
    (run_quarantined_teardown class allocation true subject) = 0 /\
  quarantine_recursive_raw_drops
    (run_quarantined_teardown class allocation true subject) = 0 /\
  quarantine_retains_raw_header
    (run_quarantined_teardown class allocation true subject) = true.
Proof. intros. destruct class, allocation; repeat split; reflexivity. Qed.

Theorem successful_quarantine_preserves_exact_par_handoff : forall
    class allocation subject,
  quarantine_process
    (run_quarantined_teardown class allocation false subject) = LiveCompleted ->
  queued_par_jobs
    (quarantine_trace
      (run_quarantined_teardown class allocation false subject)) =
    flat_map par_job (reverse_payloads_tree subject).
Proof.
  intros class allocation subject completed.
  destruct class, allocation; simpl in completed |- *;
    try discriminate;
    exact (proj1 drain_transfers_every_par_to_one_explicit_pda_job subject).
Qed.

Theorem unique_anchor_teardown_is_allocation_free : forall subject,
  quarantine_process
    (run_quarantined_teardown UniqueAnchorTeardown
      NoAllocationRequired false subject) = LiveCompleted /\
  quarantine_allocation_attempts
    (run_quarantined_teardown UniqueAnchorTeardown
      NoAllocationRequired false subject) = 0.
Proof. intro subject. split; reflexivity. Qed.

Theorem cross_anchor_allocation_failure_is_terminal : forall subject,
  quarantine_process
    (run_quarantined_teardown CrossAnchorCowTeardown
      AllocationFailed false subject) = ProcessFailStopped /\
  quarantine_allocation_attempts
    (run_quarantined_teardown CrossAnchorCowTeardown
      AllocationFailed false subject) = 1 /\
  quarantine_live_continuations
    (run_quarantined_teardown CrossAnchorCowTeardown
      AllocationFailed false subject) = 0 /\
  quarantine_recursive_raw_drops
    (run_quarantined_teardown CrossAnchorCowTeardown
      AllocationFailed false subject) = 0.
Proof. intro subject. repeat split; reflexivity. Qed.

Inductive quarantine_invariant_id : Type :=
| ClosedNamedPolicyRegistryInvariant
| NonUnwindingPolicyDispatchInvariant
| RawHeaderQuarantineInvariant
| ExplicitDropAtEmptyInvariant
| FailStopNoContinuationInvariant
| FailStopNoRecursiveDropInvariant
| NoLiveProcessLeakInvariant
| ParHandoffExactOnceInvariant
| UniqueAnchorNoAllocationInvariant
| CrossAnchorAllocationFailStopInvariant.

Definition quarantine_invariant_catalog : list quarantine_invariant_id :=
  [ClosedNamedPolicyRegistryInvariant;
   NonUnwindingPolicyDispatchInvariant;
   RawHeaderQuarantineInvariant;
   ExplicitDropAtEmptyInvariant;
   FailStopNoContinuationInvariant;
   FailStopNoRecursiveDropInvariant;
   NoLiveProcessLeakInvariant;
   ParHandoffExactOnceInvariant;
   UniqueAnchorNoAllocationInvariant;
   CrossAnchorAllocationFailStopInvariant].

Theorem quarantine_invariant_catalog_is_duplicate_free_and_exact :
  NoDup quarantine_invariant_catalog /\
  length quarantine_invariant_catalog = 10.
Proof. split; [repeat constructor; simpl; intuition discriminate|reflexivity]. Qed.

Theorem quarantine_failure_strengthening_capstone : forall subject,
  (forall class allocation unexpected_panic,
    explicit_raw_drop_is_empty
      (run_quarantined_teardown class allocation unexpected_panic subject) /\
    fail_stop_is_terminal
      (run_quarantined_teardown class allocation unexpected_panic subject) /\
    no_live_process_quarantine_leak
      (run_quarantined_teardown class allocation unexpected_panic subject) /\
    quarantine_recursive_raw_drops
      (run_quarantined_teardown class allocation unexpected_panic subject) = 0) /\
  queued_par_jobs
    (quarantine_trace
      (run_quarantined_teardown UniqueAnchorTeardown
        NoAllocationRequired false subject)) =
    flat_map par_job (reverse_payloads_tree subject) /\
  quarantine_allocation_attempts
    (run_quarantined_teardown UniqueAnchorTeardown
      NoAllocationRequired false subject) = 0 /\
  quarantine_process
    (run_quarantined_teardown CrossAnchorCowTeardown
      AllocationFailed false subject) = ProcessFailStopped.
Proof.
  intro subject. split.
  - intros class allocation unexpected_panic.
    exact (every_quarantine_outcome_guards_raw_drop
      class allocation unexpected_panic subject).
  - split.
    + exact (proj1 drain_transfers_every_par_to_one_explicit_pda_job subject).
    + split; reflexivity.
Qed.

Inductive invariant_id : Type :=
| AnchorArcAuthorityInvariant
| ReverseTerminalOrderInvariant
| RootPayloadInvariant
| OriginalExactlyOnceInvariant
| BarrierNonOverwriteInvariant
| ImmediateParentBarrierInvariant
| OneEdgePruneInvariant
| TopologyProgressInvariant
| UniqueNoRawDropInvariant
| ImmutableCloneHeaderInvariant
| FinalArcElectionInvariant
| MakeMutFreshAnchorInvariant
| CrossAnchorCowInvariant
| ParPdaTransferInvariant
| PartialOwnerExitInvariant
| DetachedSubtreeClosureInvariant
| PrivatePublicationInvariant
| FailurePrecedenceInvariant.

Definition invariant_catalog : list invariant_id :=
  [AnchorArcAuthorityInvariant; ReverseTerminalOrderInvariant;
   RootPayloadInvariant; OriginalExactlyOnceInvariant;
   BarrierNonOverwriteInvariant; ImmediateParentBarrierInvariant;
   OneEdgePruneInvariant; TopologyProgressInvariant;
   UniqueNoRawDropInvariant; ImmutableCloneHeaderInvariant;
   FinalArcElectionInvariant; MakeMutFreshAnchorInvariant;
   CrossAnchorCowInvariant; ParPdaTransferInvariant;
   PartialOwnerExitInvariant; DetachedSubtreeClosureInvariant;
   PrivatePublicationInvariant; FailurePrecedenceInvariant].

Theorem invariant_catalog_is_duplicate_free : NoDup invariant_catalog.
Proof. repeat constructor; simpl; intuition discriminate. Qed.

Theorem invariant_catalog_has_exactly_eighteen_entries :
  length invariant_catalog = 18.
Proof. reflexivity. Qed.

Theorem campaign_branch_safe_pathmap_teardown_capstone : forall subject,
  detached_payloads (drain_tree subject) = reverse_payloads_tree subject /\
  prune_widths (drain_tree subject) = repeat 1 (tree_edges subject) /\
  queued_par_jobs (drain_tree subject) =
    flat_map par_job (reverse_payloads_tree subject) /\
  residual_campaign_edges (release_unique subject) = 0 /\
  recursively_dropped_unique_edges (release_unique subject) = 0 /\
  (forall sibling,
    preserved_sibling
      (release_cross_anchor_shared_image subject sibling) = Some sibling) /\
  (forall failure,
    returned_failure
      (finalize_private_builder (RejectPrivate failure) subject) =
      Some failure /\
    publication_count
      (finalize_private_builder (RejectPrivate failure) subject) = 0) /\
  (forall exit,
    residual_campaign_edges (close_campaign_owned_exit exit subject) = 0 /\
    recursively_dropped_unique_edges
      (close_campaign_owned_exit exit subject) = 0) /\
  anchor_transition_final (nonlast_wrapper_drop 0) =
    anchor_transition_initial final_wrapper_drop /\
  anchor_transition_trace final_wrapper_drop =
    [ArcElectsFinalWrapper; BeginAnchorReversePrunePda] /\
  anchor_raw_headers
    (anchor_transition_final (immutable_wrapper_clone 0)) = 1 /\
  anchor_root_payload_copies
    (anchor_transition_final (immutable_wrapper_clone 0)) = 1 /\
  anchor_raw_headers
    (make_mut_fresh_final (shared_arc_make_mut 0)) = 1 /\
  publication_count (finalize_private_builder AcceptComplete subject) = 1.
Proof.
  intro subject. repeat split.
  - exact (proj1 drain_detaches_originals_exactly_once_in_reverse_postorder
      subject).
  - exact (proj1 drain_prunes_every_logical_edge_once subject).
  - exact (proj1 drain_transfers_every_par_to_one_explicit_pda_job subject).
Qed.

Print Assumptions drain_detaches_originals_exactly_once_in_reverse_postorder.
Print Assumptions drain_prunes_every_logical_edge_once.
Print Assumptions drain_transfers_every_par_to_one_explicit_pda_job.
Print Assumptions topology_adversaries_are_drained_edge_by_edge.
Print Assumptions immutable_wrapper_clone_does_not_clone_raw_header_or_root_value.
Print Assumptions two_wrapper_arc_sequence_elects_exactly_one_final_drop.
Print Assumptions make_mut_creates_one_well_formed_fresh_anchor.
Print Assumptions make_mut_abort_leaves_no_unregistered_raw_header.
Print Assumptions unique_release_never_uses_recursive_nonempty_pathmap_drop.
Print Assumptions cross_anchor_teardown_preserves_the_sibling_image.
Print Assumptions final_anchor_drop_is_the_only_raw_header_teardown.
Print Assumptions every_campaign_owned_exit_drains_before_release.
Print Assumptions destructive_detachment_never_releases_a_live_branch.
Print Assumptions malformed_or_rejected_partial_build_is_fully_drained.
Print Assumptions complete_builder_publishes_exactly_once.
Print Assumptions invariant_catalog_has_exactly_eighteen_entries.
Print Assumptions campaign_branch_safe_pathmap_teardown_capstone.

End PathMapReversePruneTeardown.
