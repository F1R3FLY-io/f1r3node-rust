From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import PathMapPhysicalOwnership
  PathMapReversePruneTeardown.

Import ListNotations.
Import PathMapReversePruneTeardown.

(** Each negative control changes one ownership, traversal, pruning, or
    publication decision.  The witnesses are intentionally small; the main
    theory separately quantifies over arbitrary finite tries and provides
    unbounded line, dense, and comb topology families. *)
Module PathMapReversePruneTeardownMutants.

Definition valued_line : trie :=
  Node (Some (SetMember 10))
    (FCons (Node (Some (BagMultiplicity 20 3)) FNil) FNil).

Definition root_par_trie : trie :=
  Node (Some (MapPar 7 [11; 12]))
    (FCons (Node (Some (SetMember 30)) FNil) FNil).

Definition topology_only_dense : trie := topology_dense 4.
Definition topology_only_comb : trie := topology_comb 3.

Definition compressed_internal_values : trie :=
  Node (Some (SetMember 1))
    (FCons
      (Node (Some (BagMultiplicity 2 5))
        (FCons (Node (Some (SetMember 3)) FNil) FNil))
      FNil).

Definition private_writer_authority : terminal_root_authority :=
  verified_terminal_authority PrivateWriterEarlyDrop.

Definition failed_candidate_authority : terminal_root_authority :=
  verified_terminal_authority FailedFreshCandidate.

Definition final_anchor_authority : terminal_root_authority :=
  verified_terminal_authority FinalAnchorRootTeardown.

Theorem adversarial_fixtures_are_non_vacuous :
  tree_edges topology_only_dense = 4 /\
  tree_edges topology_only_comb = 6 /\
  reverse_payloads_tree topology_only_dense = [] /\
  reverse_payloads_tree topology_only_comb = [].
Proof. vm_compute. auto. Qed.

(** Run fixtures of barrier-free Phase B.  The frames list the ancestors of the
    leaf being removed, from its parent up to the zipper root (the last frame),
    with the children each one holds to the left of the removed path.
    [forked_spine]: the parent forks (a line of eight edges to the left).
    [valued_spine]: the parent holds a real value.
    [unary_spine]: two value-free single-child parents below a fork. *)
Definition forked_spine : list run_frame :=
  [RunFrame None (FCons (topology_line 8) FNil); RunFrame None FNil].

Definition valued_spine : list run_frame :=
  [RunFrame (Some (SetMember 40)) FNil; RunFrame None FNil; RunFrame None FNil].

Definition unary_spine : list run_frame :=
  [RunFrame None FNil; RunFrame None FNil;
   RunFrame None (FCons topology_leaf FNil); RunFrame None FNil].

Theorem barrier_free_run_fixtures_are_non_vacuous :
  barrier_free_run None forked_spine = [RunStep OneBytePrune 1 []] /\
  barrier_free_run None valued_spine = [RunStep OneBytePrune 1 []] /\
  barrier_free_run None unary_spine =
    [RunStep BranchCut 1 []; RunStep BranchCut 1 []; RunStep OneBytePrune 1 []].
Proof. vm_compute. auto. Qed.

Lemma a_failing_step_refutes_the_run : forall steps step,
  In step steps -> ~ run_step_is_one_edge_without_payload_loss step ->
  ~ Forall run_step_is_one_edge_without_payload_loss steps.
Proof.
  intros steps step member failing all_steps.
  apply failing. exact (proj1 (Forall_forall _ _) all_steps step member).
Qed.

(** Mutant: the branch cut is also chosen below a fork.  [remove_branches]
    at the fork then detaches every left sibling subtree with the child's
    edge: ten logical edges in one step on [forked_spine]. *)
Definition mutant_fork_cut_policy (view : step_view) : phase_b_step_kind :=
  if frame_is_root (view_parent view) ||
     holds_payload (frame_cell (view_parent view))
  then OneBytePrune else BranchCut.

Theorem kill_cut_at_forking_parent :
  barrier_free_run_with mutant_fork_cut_policy None forked_spine =
    [RunStep BranchCut 10 []; RunStep OneBytePrune 1 []] /\
  ~ Forall run_step_is_one_edge_without_payload_loss
      (barrier_free_run_with mutant_fork_cut_policy None forked_spine).
Proof.
  assert (computed :
    barrier_free_run_with mutant_fork_cut_policy None forked_spine =
      [RunStep BranchCut 10 []; RunStep OneBytePrune 1 []])
    by (vm_compute; reflexivity).
  split; [exact computed|]. rewrite computed.
  apply (a_failing_step_refutes_the_run _ (RunStep BranchCut 10 []));
    [left; reflexivity|].
  intros [edges _]. discriminate edges.
Qed.

(** Mutant: the branch cut is also chosen below a valued parent.  The run then
    goes on above that parent, now a valued leaf, and the next cut drops the
    real value [SetMember 40] instead of leaving it to Phase A's disposer. *)
Definition mutant_valued_cut_policy (view : step_view) : phase_b_step_kind :=
  if frame_is_root (view_parent view) ||
     match view_left_children view with FNil => false | FCons _ _ => true end
  then OneBytePrune else BranchCut.

Theorem kill_cut_at_valued_parent :
  barrier_free_run_with mutant_valued_cut_policy None valued_spine =
    [RunStep BranchCut 1 []; RunStep BranchCut 1 [SetMember 40];
     RunStep OneBytePrune 1 []] /\
  ~ Forall run_step_is_one_edge_without_payload_loss
      (barrier_free_run_with mutant_valued_cut_policy None valued_spine).
Proof.
  assert (computed :
    barrier_free_run_with mutant_valued_cut_policy None valued_spine =
      [RunStep BranchCut 1 []; RunStep BranchCut 1 [SetMember 40];
       RunStep OneBytePrune 1 []])
    by (vm_compute; reflexivity).
  split; [exact computed|]. rewrite computed.
  apply (a_failing_step_refutes_the_run _
    (RunStep BranchCut 1 [SetMember 40])); [right; left; reflexivity|].
  intros [_ dropped]. discriminate dropped.
Qed.

(** Mutant: the stopping prune is not bounded to one byte.  It climbs past
    the stopping parent up to the zipper root and detaches every branch it
    passes: on [forked_spine] the one step below the fork removes all eleven
    logical edges of the trie. *)
Fixpoint unbounded_prune_edges (frames : list run_frame) : nat :=
  match frames with
  | [] => 0
  | frame :: higher =>
      S (match higher with
         | [] => 0
         | _ :: _ =>
             forest_edges (run_frame_left frame) + unbounded_prune_edges higher
         end)
  end.

Fixpoint mutant_unbounded_prune_run (current : option payload)
    (frames : list run_frame) : list run_step :=
  match frames with
  | [] => []
  | frame :: higher =>
      match barrier_free_step_kind (run_view frame higher) with
      | BranchCut =>
          RunStep BranchCut
            (step_detached_edges BranchCut (run_view frame higher))
            (option_list current ++
              reverse_payloads_forest (run_frame_left frame)) ::
          mutant_unbounded_prune_run (run_frame_cell frame) higher
      | OneBytePrune => [RunStep OneBytePrune (unbounded_prune_edges frames) []]
      end
  end.

Theorem kill_unbounded_stopping_prune :
  mutant_unbounded_prune_run None forked_spine =
    [RunStep OneBytePrune 11 []] /\
  ~ Forall run_step_is_one_edge_without_payload_loss
      (mutant_unbounded_prune_run None forked_spine).
Proof.
  assert (computed :
    mutant_unbounded_prune_run None forked_spine =
      [RunStep OneBytePrune 11 []])
    by (vm_compute; reflexivity).
  split; [exact computed|]. rewrite computed.
  apply (a_failing_step_refutes_the_run _ (RunStep OneBytePrune 11 []));
    [left; reflexivity|].
  intros [edges _]. discriminate edges.
Qed.

(** Mutant: enumerate only stored values.  Topology-only leaves and tails are
    then invisible and remain owned by recursive PathMap Drop. *)
Definition mutant_value_only_prune_widths (subject : trie) : list nat :=
  repeat 1 (length (reverse_payloads_tree subject)).

Theorem kill_value_only_walk_on_topology_only_comb :
  mutant_value_only_prune_widths topology_only_comb <>
  prune_widths (drain_tree topology_only_comb).
Proof. vm_compute. discriminate. Qed.

(** Controls of the live removal path's barrier step [prune_segment], which
    keeps its barriers.  Until the barrier-free remedy they were Phase B
    controls; Phase B now installs no barrier, and its own controls are the
    three mutants above.

    Mutant: an absent parent is not guarded, so one detach spans two logical
    edges instead of the exact immediate child edge. *)
Definition mutant_missing_barrier : list teardown_event := [PruneSpan 2].

Theorem kill_missing_immediate_parent_barrier :
  ~ branch_safe_segment false mutant_missing_barrier.
Proof. unfold branch_safe_segment, mutant_missing_barrier. discriminate. Qed.

(** Mutant: the barrier is installed after pruning rather than before it. *)
Definition mutant_wrong_level_barrier : list teardown_event :=
  [PruneSpan 1; InstallBarrier; DetachBarrier].

Theorem kill_wrong_level_or_late_barrier :
  ~ branch_safe_segment false mutant_wrong_level_barrier.
Proof. unfold branch_safe_segment, mutant_wrong_level_barrier. discriminate. Qed.

(** Mutant: a destruction-only barrier overwrites an existing real value. *)
Definition mutant_overwrite_real_parent : list teardown_event :=
  [InstallBarrier; PruneSpan 1; DetachBarrier].

Theorem kill_real_parent_value_overwrite :
  ~ branch_safe_segment true mutant_overwrite_real_parent.
Proof. unfold branch_safe_segment, mutant_overwrite_real_parent. discriminate. Qed.

(** Controls of the floor-bounded live removal ([live_prune_run]), the loop
    of [remove_leaf_with_floor] that the in-place exclusive removal reuses.
    Each mutant changes one decision of that loop, or drops the premise of
    the external-image theorem, and is refuted on a fixture of the main
    theory: [live_chain_frames], [live_floor_frames] or [live_fork_frames].

    Mutant: no barrier at a value-free parent.  Nothing then stops PathMap's
    own prune at that parent, so the first step detaches the whole dangling
    chain up to the next real value or fork: three logical edges in one
    prune on [live_chain_frames], where the model detaches one edge per
    step. *)
Fixpoint mutant_unguarded_reach (frames : list live_frame) : nat :=
  match frames with
  | [] => 0
  | frame :: higher =>
      S (if holds_payload (live_frame_cell frame) || live_frame_forks frame
         then 0 else mutant_unguarded_reach higher)
  end.

Definition mutant_unguarded_live_run (frames : list live_frame)
    : list teardown_event :=
  [PruneSpan (mutant_unguarded_reach frames)].

Theorem kill_live_run_missing_barrier :
  prune_widths (live_prune_run 5 live_chain_frames) = [1; 1; 1] /\
  prune_widths (mutant_unguarded_live_run live_chain_frames) = [3] /\
  exists width,
    In width (prune_widths (mutant_unguarded_live_run live_chain_frames)) /\
    width <> 1.
Proof.
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  exists 3. split.
  - vm_compute. left. reflexivity.
  - discriminate.
Qed.

(** Mutant: the floor is ignored.  The run stops only at a real value or a
    fork, so on [live_floor_frames] with [above_floor = 1] it goes on past
    the floor and detaches the floor's own edge: three steps, where
    [live_prune_run_respects_the_floor] allows at most two. *)
Fixpoint mutant_floorless_live_run (frames : list live_frame)
    : list teardown_event :=
  match frames with
  | [] => []
  | frame :: higher =>
      live_frame_segment frame ++
      (if holds_payload (live_frame_cell frame) || live_frame_forks frame
       then [] else mutant_floorless_live_run higher)
  end.

Theorem kill_live_run_ignored_floor :
  length (prune_widths (live_prune_run 1 live_floor_frames)) = 2 /\
  length (prune_widths (mutant_floorless_live_run live_floor_frames)) = 3 /\
  S 1 < length (prune_widths (mutant_floorless_live_run live_floor_frames)).
Proof.
  assert (floorless :
    length (prune_widths (mutant_floorless_live_run live_floor_frames)) = 3)
    by (vm_compute; reflexivity).
  split; [vm_compute; reflexivity|].
  split; [exact floorless|].
  rewrite floorless. lia.
Qed.

(** Mutant: the barrier installed at a value-free stopping parent stays
    installed (the retirement at the stopping frame is skipped).  On
    [live_fork_frames] the run stops at once at the fork and leaves one
    barrier in the live trie, where
    [live_removal_retires_every_installed_barrier] retires every barrier the
    removal installs. *)
Definition mutant_retained_barrier_segment (stops : bool) (frame : live_frame)
    : list teardown_event :=
  if holds_payload (live_frame_cell frame) then [PruneSpan 1]
  else if stops then [InstallBarrier; PruneSpan 1]
  else [InstallBarrier; PruneSpan 1; DetachBarrier].

Fixpoint mutant_retained_barrier_live_run (above_floor : nat)
    (frames : list live_frame) : list teardown_event :=
  match frames with
  | [] => []
  | frame :: higher =>
      mutant_retained_barrier_segment
        (live_frame_stops (live_at_floor above_floor) frame) frame ++
      (if live_frame_stops (live_at_floor above_floor) frame then []
       else mutant_retained_barrier_live_run (pred above_floor) higher)
  end.

Theorem kill_live_run_barrier_left_installed :
  mutant_retained_barrier_live_run 4 live_fork_frames =
    [InstallBarrier; PruneSpan 1] /\
  installed_barrier_count
    (mutant_retained_barrier_live_run 4 live_fork_frames) <>
  retired_barrier_count
    (mutant_retained_barrier_live_run 4 live_fork_frames).
Proof.
  split; [vm_compute; reflexivity|].
  vm_compute. discriminate.
Qed.

(** Mutant: the removal runs on an anchor whose footprint overlaps an
    external anchor's, the aliasing of bug 14492: after a header clone both
    anchors hold the nodes [(0, 0)] and [(0, 1)].  Removing the endpoint at
    [(0, 1)] below the forking root [(0, 0)], which is the floor, frees
    [(0, 1)] and destroys part of the external image.  The overlap is
    exactly the premise that [live_removal_run_preserves_external_images]
    needs and this run lacks. *)
Definition live_overlap_subject : PathMapPhysicalOwnership.physical_anchor :=
  PathMapPhysicalOwnership.PhysicalAnchor 0
    (PathMapPhysicalOwnership.RootAllocated (0, 0)) [(0, 0); (0, 1)]
    [[1]; [2]] 1 1 PathMapPhysicalOwnership.WholeRootOwner.

Definition live_overlap_sibling : PathMapPhysicalOwnership.physical_anchor :=
  PathMapPhysicalOwnership.PhysicalAnchor 1
    (PathMapPhysicalOwnership.RootAllocated (0, 0)) [(0, 0); (0, 1)]
    [[1]; [2]] 1 1 PathMapPhysicalOwnership.WholeRootOwner.

Definition live_overlap_owned : PathMapPhysicalOwnership.allocation_region :=
  PathMapPhysicalOwnership.AllocationRegion 0 [(0, 0); (0, 1)] [].

Definition live_overlap_sibling_region
    : PathMapPhysicalOwnership.allocation_region :=
  PathMapPhysicalOwnership.AllocationRegion 0 [(0, 0); (0, 1)] [].

Definition live_overlap_frames : list live_frame :=
  [LiveFrame (0, 0) None true].

Theorem kill_live_run_on_overlapping_footprint :
  live_run_released 0 (0, 1) live_overlap_frames = [(0, 1)] /\
  ~ PathMapPhysicalOwnership.physical_image_intact_after_release
      (live_released_part live_overlap_subject
        (live_run_released 0 (0, 1) live_overlap_frames))
      live_overlap_sibling /\
  ~ exclusive_terminal_footprint live_overlap_owned
      [live_overlap_sibling_region].
Proof.
  assert (lost :
    ~ PathMapPhysicalOwnership.physical_image_intact_after_release
        (live_released_part live_overlap_subject
          (live_run_released 0 (0, 1) live_overlap_frames))
        live_overlap_sibling) by (vm_compute; discriminate).
  split; [vm_compute; reflexivity|].
  split; [exact lost|].
  intro footprint. apply lost.
  apply (@live_removal_run_preserves_external_images live_overlap_owned
    [live_overlap_sibling_region] live_overlap_subject live_overlap_sibling
    live_overlap_sibling_region 0 (0, 1) live_overlap_frames footprint).
  - left. reflexivity.
  - reflexivity.
  - right. left. reflexivity.
  - intros frame member. destruct member as [same|[]].
    subst frame. left. reflexivity.
Qed.

(** Mutant: [remove_val(true)] is used at a terminal and recursively prunes a
    unary suffix. *)
Definition mutant_remove_value_with_recursive_prune : list teardown_event :=
  [PruneSpan 2].

Theorem kill_remove_value_with_recursive_prune :
  prune_widths mutant_remove_value_with_recursive_prune <> [1].
Proof. vm_compute. discriminate. Qed.

(** Mutants: a Phase-A value endpoint is skipped or destroyed twice. *)
Definition mutant_skip_first_payload : list payload :=
  tl (preorder_payloads_tree valued_line).

Definition mutant_duplicate_first_payload : list payload :=
  match preorder_payloads_tree valued_line with
  | [] => []
  | first :: rest => first :: first :: rest
  end.

Theorem kill_skipped_payload :
  mutant_skip_first_payload <>
  removed_without_prune (terminal_two_phase_trace valued_line).
Proof. vm_compute. discriminate. Qed.

Theorem kill_duplicated_payload :
  mutant_duplicate_first_payload <>
  disposed_values (terminal_two_phase_trace valued_line).
Proof. vm_compute. discriminate. Qed.

(** Mutant: root-value handling assumes all values have nonempty paths. *)
Definition mutant_omit_root_payload : list payload :=
  preorder_payloads_forest
    (match valued_line with Node _ children => children end).

Theorem kill_root_payload_omission :
  mutant_omit_root_payload <>
  removed_without_prune (terminal_two_phase_trace valued_line).
Proof. vm_compute. discriminate. Qed.

Definition mutant_root_and_leaf_only : list payload :=
  [SetMember 1; SetMember 3].

Theorem kill_compressed_internal_value_omission :
  mutant_root_and_leaf_only <>
  disposed_values (terminal_two_phase_trace compressed_internal_values).
Proof. vm_compute. discriminate. Qed.

Definition mutant_delayed_phase_a_disposal (value : payload) :
    list teardown_event :=
  [RemoveValueNoPrune value; CloseValueCursor; DisposeValue value].

Theorem kill_delayed_phase_a_disposal :
  firstn 2 (mutant_delayed_phase_a_disposal (SetMember 10)) <>
  firstn 2 (phase_a_dispose_value (SetMember 10)).
Proof. discriminate. Qed.

(** Mutant: retain every extracted value until traversal completes.  Unlike
    delayed disposal, this directly recreates the O(N) pending-value slope. *)
Definition mutant_retained_phase_a_values (subject : trie) : nat :=
  logical_value_count subject.

Theorem kill_retain_all_phase_a_values_memory_slope :
  phase_a_retained_values iterative_phase_a_resources <
  mutant_retained_phase_a_values compressed_internal_values.
Proof. vm_compute. lia. Qed.

Definition mutant_omit_close_and_zero_validation (subject : trie) :
    list teardown_event :=
  flat_map phase_a_dispose_value (preorder_payloads_tree subject).

Theorem kill_omitted_close_and_zero_validation :
  mutant_omit_close_and_zero_validation valued_line <>
  phase_a_value_trace valued_line.
Proof. vm_compute. discriminate. Qed.

Definition mutant_topology_before_zero_proof (subject : trie) :
    list teardown_event := BeginTopologyPhase :: drain_tree subject.

Theorem kill_topology_before_zero_value_proof :
  detached_payloads (mutant_topology_before_zero_proof valued_line) <> [].
Proof. vm_compute. discriminate. Qed.

Theorem kill_phase_b_implicit_real_value_drop :
  detached_payloads
    (run_phase_b (run_phase_a valued_line)
      (proj1 erase_values_produces_a_value_free_root valued_line)) = [] /\
  detached_payloads (mutant_topology_before_zero_proof valued_line) <> [].
Proof. vm_compute. split; [reflexivity|discriminate]. Qed.

(** Mutant: canonical empty authority is reopened and sent through both
    teardown phases instead of ordinary empty Drop. *)
Definition mutant_reopen_canonical_empty : list teardown_event :=
  terminal_two_phase_trace canonical_empty_root.

Theorem kill_reopened_canonical_empty_final_anchor :
  mutant_reopen_canonical_empty <>
  release_trace
    (release_final_anchor final_anchor_authority canonical_empty_root).
Proof. vm_compute. discriminate. Qed.

(** Mutant: topology teardown is entered despite the canonical root owning no
    logical edge. *)
Definition mutant_drain_canonical_empty : list teardown_event :=
  [BeginTopologyPhase].

Theorem kill_canonical_empty_topology_drain :
  mutant_drain_canonical_empty <>
  release_trace
    (release_final_anchor final_anchor_authority canonical_empty_root).
Proof. discriminate. Qed.

(** Mutant: the last owner hands a nonempty trie directly to ordinary Drop. *)
Definition mutant_raw_unique_release (subject : trie) : release_result :=
  ReleaseResult [] (tree_edges subject) (tree_edges subject) None.

Theorem kill_raw_unique_release :
  residual_campaign_edges (mutant_raw_unique_release topology_only_comb) <> 0 /\
  recursively_dropped_unique_edges
    (mutant_raw_unique_release topology_only_comb) <> 0.
Proof. vm_compute. split; discriminate. Qed.

(** Mutant: two wrappers independently snapshot [strong_count = 2], both
    choose the nonlast path, and therefore neither executes anchor teardown. *)
Definition mutant_strong_count_snapshot_race : list anchor_event :=
  [DropNonlastWrapperArc; DropNonlastWrapperArc].

Definition safe_two_wrapper_drop_trace : list anchor_event :=
  anchor_transition_trace (nonlast_wrapper_drop 0) ++
  anchor_transition_trace final_wrapper_drop.

Theorem kill_snapshot_only_strong_count_race :
  mutant_strong_count_snapshot_race <> safe_two_wrapper_drop_trace.
Proof. vm_compute. discriminate. Qed.

(** Mutant: wrapper Clone also clones the raw PathMap header and its root
    value.  The anchor design permits exactly one of each immutable version. *)
Definition mutant_per_wrapper_raw_header_clone : anchor_state :=
  AnchorState 2 2 2.

Theorem kill_per_wrapper_raw_header_and_root_value_clone :
  mutant_per_wrapper_raw_header_clone <>
  anchor_transition_final (immutable_wrapper_clone 0).
Proof. discriminate. Qed.

(** Mutant: a manual family counter retains per-wrapper raw-root lifecycle
    work on a nonlast Drop.  The Arc-only wrapper must touch no raw header. *)
Definition mutant_manual_family_nonlast_trace : list anchor_event :=
  [CloneRawHeaderForFreshAnchor; DropNonlastWrapperArc].

Theorem kill_manual_family_count_protocol :
  mutant_manual_family_nonlast_trace <>
  anchor_transition_trace (nonlast_wrapper_drop 0).
Proof. discriminate. Qed.

(** Mutant: a failed or torn make_mut publishes a fresh raw header without
    registering the corresponding anchor root payload. *)
Definition mutant_unregistered_make_mut_anchor : anchor_state :=
  AnchorState 1 1 0.

Theorem kill_unregistered_make_mut_raw_header :
  ~ anchor_well_formed mutant_unregistered_make_mut_anchor.
Proof. simpl. intros [_ impossible]. discriminate. Qed.

(** Mutant: Drop of a private writer/iterator bypasses terminal closure. *)
Definition mutant_early_owner_drop
    (_ : campaign_owned_exit) (subject : trie) : release_result :=
  mutant_raw_unique_release subject.

Theorem kill_early_pending_writer_or_iterator_drop :
  residual_campaign_edges
    (mutant_early_owner_drop PrivateWriterEarlyDrop topology_only_comb) <>
  residual_campaign_edges
    (close_campaign_owned_exit private_writer_authority topology_only_comb).
Proof. vm_compute. discriminate. Qed.

(** Mutant: a failed fresh whole-root candidate is handed to raw Drop. *)
Theorem kill_failed_fresh_candidate_raw_drop :
  residual_campaign_edges (mutant_raw_unique_release topology_only_dense) <>
  residual_campaign_edges
    (close_campaign_owned_exit failed_candidate_authority topology_only_dense).
Proof. vm_compute. discriminate. Qed.

(** Mutant: a [Par] value is detached and recursively dropped instead of being
    transferred to the explicit Par teardown PDA. *)
Definition mutant_recursive_par_disposal : list teardown_event :=
  [RemoveValueNoPrune (MapPar 7 [11; 12]);
   DisposeValue (MapPar 7 [11; 12]);
   DetachOriginal (MapPar 7 [11; 12])].

Theorem kill_recursive_par_disposal :
  queued_par_jobs mutant_recursive_par_disposal <>
  queued_par_jobs (phase_a_dispose_value (MapPar 7 [11; 12])).
Proof. vm_compute. discriminate. Qed.

(** Mutants: reject publishes a private partial map, or accept publishes twice. *)
Definition mutant_early_publication (failure : build_failure) : builder_result :=
  BuilderResult [PublishComplete] 1 0 0 (Some failure).

Definition mutant_double_publication : builder_result :=
  BuilderResult [PublishComplete; PublishComplete] 2 0 0 None.

Theorem kill_early_partial_publication :
  publication_count (mutant_early_publication FramingFailure) <>
  publication_count
    (finalize_private_builder
      failed_candidate_authority
      (RejectPrivate FramingFailure) topology_only_dense).
Proof. discriminate. Qed.

Theorem kill_double_complete_publication :
  publication_count mutant_double_publication <>
  publication_count
    (finalize_private_builder failed_candidate_authority
      AcceptComplete topology_only_dense).
Proof. discriminate. Qed.

(** Mutant: deferred ACT failure is reported before an earlier child-PDA
    failure. *)
Definition mutant_deferred_act_first : option build_failure :=
  Some DeferredActFailure.

Theorem kill_error_precedence_inversion :
  mutant_deferred_act_first <>
  select_failure false true true true true.
Proof. discriminate. Qed.

(** Failure-strengthening controls for the named-policy/quarantine extension. *)
Definition mutant_accept_caller_closure
    (request : teardown_policy_request) : option named_teardown_policy :=
  match request with
  | RequestNamedPolicy policy => Some policy
  | RequestCallerClosure => Some UnitSetPolicy
  end.

Theorem kill_unsealed_caller_closure_policy :
  mutant_accept_caller_closure RequestCallerClosure <>
  resolve_teardown_policy RequestCallerClosure.
Proof. discriminate. Qed.

Definition mutant_panicking_policy_dispatch
    (_ : payload) : policy_dispatch_outcome := PolicyUnwound.

Theorem kill_panicking_production_policy_dispatch :
  mutant_panicking_policy_dispatch (CandidatePosition 7) <>
  dispatch_named_policy (CandidatePosition 7).
Proof. discriminate. Qed.

(** Mutant: the concrete candidate-position disposer family is silently
    omitted from the authoritative production registry. *)
Definition mutant_omit_candidate_position_policy :
    list named_teardown_policy :=
  [BagMultiplicityPolicy].

Theorem kill_candidate_position_policy_omission :
  ~ In CandidatePositionPolicy mutant_omit_candidate_position_policy /\
  In CandidatePositionPolicy production_guarded_teardown_policies.
Proof. split; simpl; intuition discriminate. Qed.

(** Mutant: a candidate-position payload is routed through the multiplicity
    disposer policy even though the two concrete payload families have
    distinct typed policy identities. *)
Definition mutant_candidate_position_policy_misrouting :
    named_teardown_policy := BagMultiplicityPolicy.

Theorem kill_candidate_position_policy_misrouting :
  mutant_candidate_position_policy_misrouting <>
  policy_for_payload (CandidatePosition 7).
Proof. discriminate. Qed.

Definition mutant_unquarantined_fail_stop (subject : trie)
    : quarantined_teardown_result :=
  QuarantinedTeardownResult ProcessFailStopped []
    (tree_edges subject) (payload_count subject) false 0 0 0 0.

Theorem kill_raw_header_outside_fail_stop_quarantine :
  quarantine_retains_raw_header
    (mutant_unquarantined_fail_stop topology_only_comb) <>
  quarantine_retains_raw_header
    (run_quarantined_teardown private_writer_authority
      NoAllocationRequired true topology_only_comb).
Proof. discriminate. Qed.

Definition mutant_explicit_drop_with_live_topology (subject : trie)
    : quarantined_teardown_result :=
  QuarantinedTeardownResult LiveCompleted []
    (tree_edges subject) (payload_count subject) false 1 0 1 0.

Theorem kill_explicit_raw_drop_with_live_edges :
  ~ explicit_raw_drop_is_empty
      (mutant_explicit_drop_with_live_topology topology_only_comb).
Proof. unfold explicit_raw_drop_is_empty. vm_compute. intuition discriminate. Qed.

Definition mutant_recover_after_unexpected_panic (subject : trie)
    : quarantined_teardown_result :=
  QuarantinedTeardownResult LiveCompleted []
    (tree_edges subject) (payload_count subject) true 0 0 1 0.

Theorem kill_recoverable_continuation_after_unexpected_panic :
  quarantine_process
    (mutant_recover_after_unexpected_panic topology_only_comb) <>
  quarantine_process
    (run_quarantined_teardown private_writer_authority
      NoAllocationRequired true topology_only_comb).
Proof. discriminate. Qed.

Theorem kill_silent_live_process_quarantine_leak :
  ~ no_live_process_quarantine_leak
      (mutant_recover_after_unexpected_panic topology_only_comb).
Proof.
  unfold no_live_process_quarantine_leak. simpl.
  intro claimed. specialize (claimed eq_refl). discriminate.
Qed.

Definition mutant_recursive_drop_on_panic (subject : trie)
    : quarantined_teardown_result :=
  QuarantinedTeardownResult ProcessFailStopped []
    (tree_edges subject) (payload_count subject) false 0
    (tree_edges subject) 0 0.

Theorem kill_recursive_raw_drop_during_fail_stop :
  quarantine_recursive_raw_drops
    (mutant_recursive_drop_on_panic topology_only_comb) <>
  quarantine_recursive_raw_drops
    (run_quarantined_teardown private_writer_authority
      NoAllocationRequired true topology_only_comb).
Proof. vm_compute. discriminate. Qed.

Definition mutant_skip_par_handoff : list teardown_event :=
  [RemoveValueNoPrune (MapPar 7 [11; 12]);
   DisposeValue (MapPar 7 [11; 12])].

Definition mutant_duplicate_par_handoff : list teardown_event :=
  [RemoveValueNoPrune (MapPar 7 [11; 12]);
   DisposeValue (MapPar 7 [11; 12]);
   QueueParPda 7 [11; 12]; QueueParPda 7 [11; 12]].

Theorem kill_skipped_par_handoff :
  queued_par_jobs mutant_skip_par_handoff <>
  queued_par_jobs (phase_a_dispose_value (MapPar 7 [11; 12])).
Proof. vm_compute. discriminate. Qed.

Theorem kill_duplicated_par_handoff :
  queued_par_jobs mutant_duplicate_par_handoff <>
  queued_par_jobs (phase_a_dispose_value (MapPar 7 [11; 12])).
Proof. vm_compute. discriminate. Qed.

(** Mutant: final-wrapper election is confused with internal-node uniqueness,
    so a real public-zipper allocation is reported as allocation-free. *)
Definition mutant_final_wrapper_assumes_no_allocation (subject : trie)
    : quarantined_teardown_result :=
  completed_quarantined_teardown subject 0.

Theorem kill_final_wrapper_as_internal_node_uniqueness :
  quarantine_allocation_attempts
    (mutant_final_wrapper_assumes_no_allocation topology_only_comb) <>
  quarantine_allocation_attempts
    (run_quarantined_teardown private_writer_authority
      AllocationSucceeded false topology_only_comb).
Proof. discriminate. Qed.

Definition mutant_continue_after_cross_anchor_allocation_failure
    (subject : trie) : quarantined_teardown_result :=
  completed_quarantined_teardown subject 1.

Theorem kill_cross_anchor_allocation_failure_continuation :
  quarantine_process
    (mutant_continue_after_cross_anchor_allocation_failure
      topology_only_comb) <>
  quarantine_process
    (run_quarantined_teardown private_writer_authority
      AllocationFailed false topology_only_comb).
Proof. discriminate. Qed.

Inductive quarantine_mutant : Type :=
| UnsealedCallerClosurePolicy
| PanickingProductionPolicyDispatch
| CandidatePositionPolicyOmission
| CandidatePositionPolicyMisrouting
| RawHeaderOutsideFailStopQuarantine
| ExplicitDropWithLiveTopology
| RecoverAfterUnexpectedPanic
| SilentLiveQuarantineLeak
| RecursiveRawDropDuringFailStop
| SkippedParHandoff
| DuplicatedParHandoff
| FinalWrapperInternalNodeUniqueness
| TerminalAllocationContinuation.

Definition all_quarantine_mutants : list quarantine_mutant :=
  [UnsealedCallerClosurePolicy;
   PanickingProductionPolicyDispatch;
   CandidatePositionPolicyOmission;
   CandidatePositionPolicyMisrouting;
   RawHeaderOutsideFailStopQuarantine;
   ExplicitDropWithLiveTopology;
   RecoverAfterUnexpectedPanic;
   SilentLiveQuarantineLeak;
   RecursiveRawDropDuringFailStop;
   SkippedParHandoff;
   DuplicatedParHandoff;
   FinalWrapperInternalNodeUniqueness;
   TerminalAllocationContinuation].

Definition quarantine_mutant_killed (mutant : quarantine_mutant) : Prop :=
  match mutant with
  | UnsealedCallerClosurePolicy =>
      mutant_accept_caller_closure RequestCallerClosure <>
      resolve_teardown_policy RequestCallerClosure
  | PanickingProductionPolicyDispatch =>
      mutant_panicking_policy_dispatch (CandidatePosition 7) <>
      dispatch_named_policy (CandidatePosition 7)
  | CandidatePositionPolicyOmission =>
      ~ In CandidatePositionPolicy mutant_omit_candidate_position_policy /\
      In CandidatePositionPolicy production_guarded_teardown_policies
  | CandidatePositionPolicyMisrouting =>
      mutant_candidate_position_policy_misrouting <>
      policy_for_payload (CandidatePosition 7)
  | RawHeaderOutsideFailStopQuarantine =>
      quarantine_retains_raw_header
        (mutant_unquarantined_fail_stop topology_only_comb) <>
      quarantine_retains_raw_header
        (run_quarantined_teardown private_writer_authority
          NoAllocationRequired true topology_only_comb)
  | ExplicitDropWithLiveTopology =>
      ~ explicit_raw_drop_is_empty
          (mutant_explicit_drop_with_live_topology topology_only_comb)
  | RecoverAfterUnexpectedPanic =>
      quarantine_process
        (mutant_recover_after_unexpected_panic topology_only_comb) <>
      quarantine_process
        (run_quarantined_teardown private_writer_authority
          NoAllocationRequired true topology_only_comb)
  | SilentLiveQuarantineLeak =>
      ~ no_live_process_quarantine_leak
          (mutant_recover_after_unexpected_panic topology_only_comb)
  | RecursiveRawDropDuringFailStop =>
      quarantine_recursive_raw_drops
        (mutant_recursive_drop_on_panic topology_only_comb) <>
      quarantine_recursive_raw_drops
        (run_quarantined_teardown private_writer_authority
          NoAllocationRequired true topology_only_comb)
  | SkippedParHandoff =>
      queued_par_jobs mutant_skip_par_handoff <>
      queued_par_jobs (phase_a_dispose_value (MapPar 7 [11; 12]))
  | DuplicatedParHandoff =>
      queued_par_jobs mutant_duplicate_par_handoff <>
      queued_par_jobs (phase_a_dispose_value (MapPar 7 [11; 12]))
  | FinalWrapperInternalNodeUniqueness =>
      quarantine_allocation_attempts
        (mutant_final_wrapper_assumes_no_allocation topology_only_comb) <>
      quarantine_allocation_attempts
        (run_quarantined_teardown private_writer_authority
          AllocationSucceeded false topology_only_comb)
  | TerminalAllocationContinuation =>
      quarantine_process
        (mutant_continue_after_cross_anchor_allocation_failure
          topology_only_comb) <>
      quarantine_process
        (run_quarantined_teardown private_writer_authority
          AllocationFailed false topology_only_comb)
  end.

Theorem every_quarantine_mutant_has_a_causal_counterexample : forall mutant,
  quarantine_mutant_killed mutant.
Proof.
  intro mutant. destruct mutant; cbn.
  - apply kill_unsealed_caller_closure_policy.
  - apply kill_panicking_production_policy_dispatch.
  - apply kill_candidate_position_policy_omission.
  - apply kill_candidate_position_policy_misrouting.
  - apply kill_raw_header_outside_fail_stop_quarantine.
  - apply kill_explicit_raw_drop_with_live_edges.
  - apply kill_recoverable_continuation_after_unexpected_panic.
  - apply kill_silent_live_process_quarantine_leak.
  - apply kill_recursive_raw_drop_during_fail_stop.
  - apply kill_skipped_par_handoff.
  - apply kill_duplicated_par_handoff.
  - apply kill_final_wrapper_as_internal_node_uniqueness.
  - apply kill_cross_anchor_allocation_failure_continuation.
Qed.

Theorem quarantine_mutant_registry_is_unique_complete_and_exact :
  NoDup all_quarantine_mutants /\
  Forall quarantine_mutant_killed all_quarantine_mutants /\
  length all_quarantine_mutants = 13.
Proof.
  repeat split.
  - repeat constructor; simpl; intuition discriminate.
  - apply Forall_forall. intros mutant _.
    apply every_quarantine_mutant_has_a_causal_counterexample.
Qed.

(** Three entries were retired when Phase B became barrier-free: they were
    wrong barrier segments of Phase B, which no longer has a barrier.  Their
    kill theorems [kill_missing_immediate_parent_barrier],
    [kill_wrong_level_or_late_barrier] and [kill_real_parent_value_overwrite]
    stay as controls of the live removal path's step [prune_segment].
    [CutAtForkingParent], [CutAtValuedParent] and [UnboundedStoppingPrune]
    take their registry positions.  The retired bindings were:

    | MissingBarrier => ~ branch_safe_segment false mutant_missing_barrier
    | WrongLevelBarrier =>
        ~ branch_safe_segment false mutant_wrong_level_barrier
    | OverwriteRealParent =>
        ~ branch_safe_segment true mutant_overwrite_real_parent
*)
Inductive teardown_mutant : Type :=
| ValueOnlyWalk
| CutAtForkingParent
| CutAtValuedParent
| UnboundedStoppingPrune
| RecursiveRemoveValuePrune
| SkippedPayload
| DuplicatedPayload
| RootPayloadOmission
| CompressedInternalValueOmission
| DelayedPhaseADisposal
| RetainAllPhaseAValues
| MissingCloseZeroValidation
| TopologyBeforeZeroProof
| PhaseBImplicitRealValueDrop
| ReopenCanonicalEmpty
| DrainCanonicalEmpty
| RawUniqueRelease
| StrongCountSnapshotRace
| PerWrapperRawHeaderClone
| ManualFamilyCounter
| UnregisteredMakeMutAnchor
| EarlyOwnerDrop
| FailedFreshCandidateRawDrop
| RecursiveParDisposal
| EarlyPartialPublication
| DoubleCompletePublication
| ErrorPrecedenceInversion
| LiveRunMissingBarrier
| LiveRunIgnoredFloor
| LiveRunBarrierLeftInstalled
| LiveRunOverlappingFootprint.

Definition all_teardown_mutants : list teardown_mutant :=
  [ValueOnlyWalk; CutAtForkingParent; CutAtValuedParent; UnboundedStoppingPrune;
   RecursiveRemoveValuePrune; SkippedPayload; DuplicatedPayload;
   RootPayloadOmission; CompressedInternalValueOmission;
   DelayedPhaseADisposal; RetainAllPhaseAValues;
   MissingCloseZeroValidation;
   TopologyBeforeZeroProof; PhaseBImplicitRealValueDrop;
   ReopenCanonicalEmpty; DrainCanonicalEmpty;
   RawUniqueRelease; StrongCountSnapshotRace;
   PerWrapperRawHeaderClone; ManualFamilyCounter;
   UnregisteredMakeMutAnchor; EarlyOwnerDrop;
   FailedFreshCandidateRawDrop; RecursiveParDisposal; EarlyPartialPublication;
   DoubleCompletePublication; ErrorPrecedenceInversion;
   LiveRunMissingBarrier; LiveRunIgnoredFloor; LiveRunBarrierLeftInstalled;
   LiveRunOverlappingFootprint].

Definition teardown_mutant_killed (mutant : teardown_mutant) : Prop :=
  match mutant with
  | ValueOnlyWalk =>
      mutant_value_only_prune_widths topology_only_comb <>
      prune_widths (drain_tree topology_only_comb)
  | CutAtForkingParent =>
      ~ Forall run_step_is_one_edge_without_payload_loss
          (barrier_free_run_with mutant_fork_cut_policy None forked_spine)
  | CutAtValuedParent =>
      ~ Forall run_step_is_one_edge_without_payload_loss
          (barrier_free_run_with mutant_valued_cut_policy None valued_spine)
  | UnboundedStoppingPrune =>
      ~ Forall run_step_is_one_edge_without_payload_loss
          (mutant_unbounded_prune_run None forked_spine)
  | RecursiveRemoveValuePrune =>
      prune_widths mutant_remove_value_with_recursive_prune <> [1]
  | SkippedPayload =>
      mutant_skip_first_payload <>
      removed_without_prune (terminal_two_phase_trace valued_line)
  | DuplicatedPayload =>
      mutant_duplicate_first_payload <>
      disposed_values (terminal_two_phase_trace valued_line)
  | RootPayloadOmission =>
      mutant_omit_root_payload <>
      removed_without_prune (terminal_two_phase_trace valued_line)
  | CompressedInternalValueOmission =>
      mutant_root_and_leaf_only <>
      disposed_values (terminal_two_phase_trace compressed_internal_values)
  | DelayedPhaseADisposal =>
      firstn 2 (mutant_delayed_phase_a_disposal (SetMember 10)) <>
      firstn 2 (phase_a_dispose_value (SetMember 10))
  | RetainAllPhaseAValues =>
      phase_a_retained_values iterative_phase_a_resources <
      mutant_retained_phase_a_values compressed_internal_values
  | MissingCloseZeroValidation =>
      mutant_omit_close_and_zero_validation valued_line <>
      phase_a_value_trace valued_line
  | TopologyBeforeZeroProof =>
      detached_payloads (mutant_topology_before_zero_proof valued_line) <> []
  | PhaseBImplicitRealValueDrop =>
      detached_payloads
        (run_phase_b (run_phase_a valued_line)
          (proj1 erase_values_produces_a_value_free_root valued_line)) = [] /\
      detached_payloads (mutant_topology_before_zero_proof valued_line) <> []
  | ReopenCanonicalEmpty =>
      mutant_reopen_canonical_empty <>
      release_trace
        (release_final_anchor final_anchor_authority canonical_empty_root)
  | DrainCanonicalEmpty =>
      mutant_drain_canonical_empty <>
      release_trace
        (release_final_anchor final_anchor_authority canonical_empty_root)
  | RawUniqueRelease =>
      residual_campaign_edges (mutant_raw_unique_release topology_only_comb) <> 0
  | StrongCountSnapshotRace =>
      mutant_strong_count_snapshot_race <> safe_two_wrapper_drop_trace
  | PerWrapperRawHeaderClone =>
      mutant_per_wrapper_raw_header_clone <>
      anchor_transition_final (immutable_wrapper_clone 0)
  | ManualFamilyCounter =>
      mutant_manual_family_nonlast_trace <>
      anchor_transition_trace (nonlast_wrapper_drop 0)
  | UnregisteredMakeMutAnchor =>
      ~ anchor_well_formed mutant_unregistered_make_mut_anchor
  | EarlyOwnerDrop =>
      residual_campaign_edges
        (mutant_early_owner_drop PrivateWriterEarlyDrop topology_only_comb) <>
      residual_campaign_edges
        (close_campaign_owned_exit private_writer_authority topology_only_comb)
  | FailedFreshCandidateRawDrop =>
      residual_campaign_edges (mutant_raw_unique_release topology_only_dense) <>
      residual_campaign_edges
        (close_campaign_owned_exit failed_candidate_authority topology_only_dense)
  | RecursiveParDisposal =>
      queued_par_jobs mutant_recursive_par_disposal <>
      queued_par_jobs (phase_a_dispose_value (MapPar 7 [11; 12]))
  | EarlyPartialPublication =>
      publication_count (mutant_early_publication FramingFailure) <>
      publication_count
        (finalize_private_builder failed_candidate_authority
          (RejectPrivate FramingFailure) topology_only_dense)
  | DoubleCompletePublication =>
      publication_count mutant_double_publication <>
      publication_count
        (finalize_private_builder failed_candidate_authority
          AcceptComplete topology_only_dense)
  | ErrorPrecedenceInversion =>
      mutant_deferred_act_first <>
      select_failure false true true true true
  | LiveRunMissingBarrier =>
      exists width,
        In width (prune_widths (mutant_unguarded_live_run live_chain_frames)) /\
        width <> 1
  | LiveRunIgnoredFloor =>
      S 1 < length (prune_widths (mutant_floorless_live_run live_floor_frames))
  | LiveRunBarrierLeftInstalled =>
      installed_barrier_count
        (mutant_retained_barrier_live_run 4 live_fork_frames) <>
      retired_barrier_count
        (mutant_retained_barrier_live_run 4 live_fork_frames)
  | LiveRunOverlappingFootprint =>
      ~ PathMapPhysicalOwnership.physical_image_intact_after_release
          (live_released_part live_overlap_subject
            (live_run_released 0 (0, 1) live_overlap_frames))
          live_overlap_sibling /\
      ~ exclusive_terminal_footprint live_overlap_owned
          [live_overlap_sibling_region]
  end.

Theorem every_teardown_mutant_has_a_causal_counterexample : forall mutant,
  teardown_mutant_killed mutant.
Proof.
  intro mutant. destruct mutant; cbn.
  - apply kill_value_only_walk_on_topology_only_comb.
  - exact (proj2 kill_cut_at_forking_parent).
  - exact (proj2 kill_cut_at_valued_parent).
  - exact (proj2 kill_unbounded_stopping_prune).
  - apply kill_remove_value_with_recursive_prune.
  - apply kill_skipped_payload.
  - apply kill_duplicated_payload.
  - apply kill_root_payload_omission.
  - apply kill_compressed_internal_value_omission.
  - apply kill_delayed_phase_a_disposal.
  - apply kill_retain_all_phase_a_values_memory_slope.
  - apply kill_omitted_close_and_zero_validation.
  - apply kill_topology_before_zero_value_proof.
  - apply kill_phase_b_implicit_real_value_drop.
  - apply kill_reopened_canonical_empty_final_anchor.
  - apply kill_canonical_empty_topology_drain.
  - exact (proj1 kill_raw_unique_release).
  - apply kill_snapshot_only_strong_count_race.
  - apply kill_per_wrapper_raw_header_and_root_value_clone.
  - apply kill_manual_family_count_protocol.
  - apply kill_unregistered_make_mut_raw_header.
  - apply kill_early_pending_writer_or_iterator_drop.
  - apply kill_failed_fresh_candidate_raw_drop.
  - apply kill_recursive_par_disposal.
  - apply kill_early_partial_publication.
  - apply kill_double_complete_publication.
  - apply kill_error_precedence_inversion.
  - exact (proj2 (proj2 kill_live_run_missing_barrier)).
  - exact (proj2 (proj2 kill_live_run_ignored_floor)).
  - exact (proj2 kill_live_run_barrier_left_installed).
  - exact (proj2 kill_live_run_on_overlapping_footprint).
Qed.

Theorem teardown_mutant_registry_is_unique_and_complete :
  NoDup all_teardown_mutants /\
  Forall teardown_mutant_killed all_teardown_mutants.
Proof.
  split.
  - repeat constructor; simpl; intuition discriminate.
  - apply Forall_forall. intros mutant _.
    apply every_teardown_mutant_has_a_causal_counterexample.
Qed.

Theorem teardown_mutant_registry_has_exactly_thirty_one_entries :
  length all_teardown_mutants = 31.
Proof. reflexivity. Qed.

Theorem teardown_mutant_registry_is_unique_complete_and_exact :
  NoDup all_teardown_mutants /\
  Forall teardown_mutant_killed all_teardown_mutants /\
  length all_teardown_mutants = 31.
Proof.
  split.
  - exact (proj1 teardown_mutant_registry_is_unique_and_complete).
  - split.
    + exact (proj2 teardown_mutant_registry_is_unique_and_complete).
    + apply teardown_mutant_registry_has_exactly_thirty_one_entries.
Qed.

Print Assumptions adversarial_fixtures_are_non_vacuous.
Print Assumptions kill_snapshot_only_strong_count_race.
Print Assumptions kill_per_wrapper_raw_header_and_root_value_clone.
Print Assumptions kill_early_pending_writer_or_iterator_drop.
Print Assumptions kill_failed_fresh_candidate_raw_drop.
Print Assumptions kill_candidate_position_policy_omission.
Print Assumptions kill_candidate_position_policy_misrouting.
Print Assumptions quarantine_mutant_registry_is_unique_complete_and_exact.
Print Assumptions every_teardown_mutant_has_a_causal_counterexample.
Print Assumptions teardown_mutant_registry_is_unique_and_complete.
Print Assumptions kill_retain_all_phase_a_values_memory_slope.
Print Assumptions teardown_mutant_registry_has_exactly_thirty_one_entries.
Print Assumptions teardown_mutant_registry_is_unique_complete_and_exact.
Print Assumptions barrier_free_run_fixtures_are_non_vacuous.
Print Assumptions a_failing_step_refutes_the_run.
Print Assumptions kill_cut_at_forking_parent.
Print Assumptions kill_cut_at_valued_parent.
Print Assumptions kill_unbounded_stopping_prune.
Print Assumptions kill_live_run_missing_barrier.
Print Assumptions kill_live_run_ignored_floor.
Print Assumptions kill_live_run_barrier_left_installed.
Print Assumptions kill_live_run_on_overlapping_footprint.

End PathMapReversePruneTeardownMutants.
