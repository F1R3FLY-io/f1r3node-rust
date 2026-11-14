From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import PathMapPhysicalOwnership.

Import ListNotations.
Import PathMapPhysicalOwnership.

(** Causal negative controls for physical PathMap ownership. *)
Module PathMapPhysicalOwnershipMutants.

Definition mutant_semantic_focus_implies_isolation : Prop :=
  physically_disjoint
    (anchor_nodes interior_focus_fixture)
    (anchor_nodes shared_prefix_sibling_fixture).

Theorem kill_semantic_prefix_as_physical_isolation :
  ~ mutant_semantic_focus_implies_isolation.
Proof.
  exact (proj2 (proj2 (proj2 semantic_focus_does_not_imply_physical_isolation))).
Qed.

Definition mutant_owned_focused_make_map_is_release_safe : Prop :=
  physical_image_intact_after_release
    interior_focus_fixture shared_prefix_sibling_fixture.

Theorem kill_owned_focused_make_map :
  ~ mutant_owned_focused_make_map_is_release_safe.
Proof. apply focused_raw_owner_release_can_destroy_shared_prefix_sibling. Qed.

Definition mutant_shared_prefix_sibling_after_release : list node_id :=
  release_survivors interior_focus_fixture shared_prefix_sibling_fixture.

Theorem kill_shared_compressed_node_sibling_loss :
  mutant_shared_prefix_sibling_after_release <>
  anchor_nodes shared_prefix_sibling_fixture.
Proof. vm_compute. discriminate. Qed.

Definition mutant_release_ignores_physical_owners
    (_ subject sibling : physical_anchor) : list node_id :=
  anchor_nodes sibling.

Theorem kill_destructive_release_that_ignores_other_owners :
  mutant_release_ignores_physical_owners
      compressed_store_fixture interior_focus_fixture
      shared_prefix_sibling_fixture <>
  release_survivors interior_focus_fixture shared_prefix_sibling_fixture.
Proof. vm_compute. discriminate. Qed.

Definition mutant_arc_clone_creates_raw_header
    (anchor : physical_anchor) : physical_anchor :=
  PhysicalAnchor
    (anchor_identity anchor)
    (anchor_root_mode anchor)
    (anchor_nodes anchor)
    (anchor_logical_paths anchor)
    (S (anchor_raw_headers anchor))
    (S (anchor_arc_wrappers anchor))
    (anchor_destructive_capability anchor).

Theorem kill_arc_clone_raw_header_duplication :
  anchor_raw_headers
      (mutant_arc_clone_creates_raw_header compressed_store_fixture) <>
  anchor_raw_headers
      (shared_arc_clone compressed_store_fixture).
Proof. discriminate. Qed.

Definition mutant_rebuild_reuses_focused_node : physical_anchor :=
  PhysicalAnchor 2 (RootAllocated (0, 1)) [(0, 1)] [[1]] 1 1 WholeRootOwner.

Theorem kill_independent_rebuild_node_reuse :
  ~ physically_disjoint
      (anchor_nodes mutant_rebuild_reuses_focused_node)
      (anchor_nodes compressed_store_fixture).
Proof.
  unfold physically_disjoint. intro isolated.
  specialize (isolated (0, 1)). simpl in isolated. tauto.
Qed.

(** A topology-preserving update of a uniquely owned anchor must not invent a
    fresh anchor merely to replace an existing value. *)
Definition mutant_unique_value_only_update
    (anchor : physical_anchor) : physical_anchor :=
  PhysicalAnchor
    (S (anchor_identity anchor))
    (anchor_root_mode anchor)
    (anchor_nodes anchor)
    (anchor_logical_paths anchor)
    (anchor_raw_headers anchor)
    (anchor_arc_wrappers anchor)
    (anchor_destructive_capability anchor).

Theorem kill_unique_value_only_anchor_replacement :
  anchor_identity
      (mutant_unique_value_only_update compressed_store_fixture) <>
  anchor_identity
      (unique_value_only_update compressed_store_fixture).
Proof. discriminate. Qed.

(** A shared guarded update may detach the updated authority, but it must
    return the pre-existing external alias unchanged. *)
Definition mutant_shared_value_only_update
    (updated_authority _external_alias : physical_anchor)
    : physical_anchor * physical_anchor :=
  (updated_authority, updated_authority).

Theorem kill_shared_value_only_alias_mutation :
  anchor_observation
      (snd (mutant_shared_value_only_update
        independently_rebuilt_focus_fixture compressed_store_fixture)) <>
  anchor_observation
      (snd (shared_value_only_update
        independently_rebuilt_focus_fixture compressed_store_fixture)).
Proof. discriminate. Qed.

(** Requiring whole-root disjointness for a value-only update rejects legal
    immutable off-path sharing even though the detached write set is safe. *)
Definition mutant_forbids_immutable_off_path_sharing : Prop :=
  ~ off_path_sharing updated_region_fixture immutable_alias_region_fixture.

Theorem kill_whole_root_disjointness_for_value_only_update :
  ~ mutant_forbids_immutable_off_path_sharing.
Proof.
  unfold mutant_forbids_immutable_off_path_sharing.
  intro forbids_sharing. apply forbids_sharing.
  exact (proj2 isolated_value_write_allows_immutable_off_path_sharing).
Qed.

Definition mutant_structural_region_reuses_source_footprint : allocation_region :=
  AllocationRegion 0 [(0, 1)] [(0, 2)].

Theorem kill_structural_node_and_value_footprint_sharing :
  ~ structural_candidate_disjoint
      immutable_alias_region_fixture
      mutant_structural_region_reuses_source_footprint.
Proof.
  unfold structural_candidate_disjoint, allocation_regions_disjoint,
    physically_disjoint. intro disjoint.
  destruct disjoint as [nodes values].
  specialize (nodes (0, 1)). simpl in nodes. tauto.
Qed.

(** Election of the final wrapper owner says nothing about whether a different
    anchor still shares internal PathMap nodes. *)
Definition final_wrapper_internal_sharing_fixture : physical_anchor :=
  PhysicalAnchor 9 (RootAllocated (0, 1)) [(0, 1)] [[7; 1]]
    1 1 WholeRootOwner.

Definition mutant_final_wrapper_implies_internal_node_uniqueness : Prop :=
  physically_disjoint
    (anchor_nodes final_wrapper_internal_sharing_fixture)
    (anchor_nodes shared_prefix_sibling_fixture).

Theorem kill_final_wrapper_as_internal_node_uniqueness :
  ~ mutant_final_wrapper_implies_internal_node_uniqueness.
Proof.
  unfold mutant_final_wrapper_implies_internal_node_uniqueness,
    physically_disjoint. intro disjoint.
  specialize (disjoint (0, 1)). simpl in disjoint. tauto.
Qed.

(** Exclusive provenance controls (bug 14492, option A').  [mutant_gate_admits]
    evaluates a policy's removal gate at one position of a world, and
    [mutant_release_destroys] states that releasing the anchor at the first
    position would free part of the physical image of the anchor at the
    second.  Each mutant changes one decision of [exclusive_provenance_policy].
    Its kill theorem runs concrete commands under the mutant policy, shows
    that the mutant gate accepts an anchor whose release destroys another
    live anchor's image, and shows that the production policy rejects that
    anchor. *)
Definition mutant_gate_admits (policy : provenance_policy)
    (world : provenance_world) (position : nat) : bool :=
  match nth_error (provenance_anchors world) position with
  | Some anchor =>
      provenance_removal_gate policy (provenance_anchors world) position anchor
  | None => false
  end.

Definition mutant_release_destroys (world : provenance_world)
    (subject_position sibling_position : nat) : Prop :=
  match nth_error (provenance_anchors world) subject_position,
        nth_error (provenance_anchors world) sibling_position with
  | Some subject, Some sibling =>
      ~ physical_image_intact_after_release
          (provenance_physical subject) (provenance_physical sibling)
  | _, _ => False
  end.

(** [AnchorRoot::clone] clears only the fresh copy's flag.  After a wrapper
    clone and a header clone the source keeps a stale bit, holds one wrapper
    and shares every node with the copy. *)
Definition mutant_fresh_bit_only_policy : provenance_policy :=
  ProvenancePolicy (fun bit => bit)
    (fun _ _ anchor => exclusive_removal_gate anchor).

Definition mutant_fresh_bit_only_clone_is_refuted : Prop :=
  mutant_gate_admits mutant_fresh_bit_only_policy
      (provenance_run_with mutant_fresh_bit_only_policy
        provenance_header_clone_run empty_provenance_world) 1 = true /\
  mutant_release_destroys
      (provenance_run_with mutant_fresh_bit_only_policy
        provenance_header_clone_run empty_provenance_world) 1 0 /\
  mutant_gate_admits exclusive_provenance_policy
      (provenance_run provenance_header_clone_run) 1 = false.

Theorem kill_clone_that_clears_only_the_fresh_bit :
  mutant_fresh_bit_only_clone_is_refuted.
Proof. vm_compute. repeat split; try reflexivity; try discriminate. Qed.

(** The gate elects the final wrapper only.  Both halves of a header clone
    hold one wrapper and share every node; so does
    [final_wrapper_internal_sharing_fixture], whose provenance bit a header
    clone has cleared. *)
Definition mutant_wrapper_only_policy : provenance_policy :=
  ProvenancePolicy (fun _ => false)
    (fun _ _ anchor => Nat.eqb (provenance_wrappers anchor) 1).

Definition final_wrapper_provenance_fixture : provenance_anchor :=
  ProvenanceAnchor final_wrapper_internal_sharing_fixture false.

Definition mutant_wrapper_only_gate_is_refuted : Prop :=
  mutant_gate_admits mutant_wrapper_only_policy
      (provenance_run_with mutant_wrapper_only_policy
        provenance_header_clone_run empty_provenance_world) 0 = true /\
  mutant_release_destroys
      (provenance_run_with mutant_wrapper_only_policy
        provenance_header_clone_run empty_provenance_world) 0 1 /\
  mutant_gate_admits exclusive_provenance_policy
      (provenance_run provenance_header_clone_run) 0 = false /\
  provenance_removal_gate mutant_wrapper_only_policy []
      0 final_wrapper_provenance_fixture = true /\
  exclusive_removal_gate final_wrapper_provenance_fixture = false /\
  ~ physical_image_intact_after_release
      final_wrapper_internal_sharing_fixture shared_prefix_sibling_fixture.

Theorem kill_wrapper_only_removal_gate : mutant_wrapper_only_gate_is_refuted.
Proof. vm_compute. repeat split; try reflexivity; try discriminate. Qed.

(** Exclusivity comes from a root-only sharing oracle, which accepts an
    anchor of one wrapper whose root node no anchor at another position
    holds.  A unique path-copy write of the root of one half of a header clone
    gives that half a private root, while its descendant (0, 1) stays shared
    with the other half. *)
Definition mutant_root_held_elsewhere (anchors : list provenance_anchor)
    (position : nat) (root : node_id) : bool :=
  existsb
    (fun indexed : nat * provenance_anchor =>
      negb (Nat.eqb (fst indexed) position) &&
      negb (provenance_absent_from (provenance_nodes (snd indexed)) root))
    (combine (seq 0 (length anchors)) anchors).

Definition mutant_root_only_gate (anchors : list provenance_anchor)
    (position : nat) (anchor : provenance_anchor) : bool :=
  Nat.eqb (provenance_wrappers anchor) 1 &&
  match anchor_root_mode (provenance_physical anchor) with
  | RootAllocated root => negb (mutant_root_held_elsewhere anchors position root)
  | _ => true
  end.

Definition mutant_root_only_policy : provenance_policy :=
  ProvenancePolicy (fun _ => false) mutant_root_only_gate.

Definition provenance_private_root_run : list provenance_command :=
  [ProvenanceConstruct 2; ProvenanceWrapperClone 0; ProvenanceHeaderClone 0;
   ProvenanceUniqueWrite 0 [(0, 0)] 0].

Definition mutant_root_only_oracle_is_refuted : Prop :=
  mutant_gate_admits mutant_root_only_policy
      (provenance_run_with mutant_root_only_policy
        provenance_private_root_run empty_provenance_world) 0 = true /\
  mutant_release_destroys
      (provenance_run_with mutant_root_only_policy
        provenance_private_root_run empty_provenance_world) 0 1 /\
  mutant_gate_admits exclusive_provenance_policy
      (provenance_run provenance_private_root_run) 0 = false.

Theorem kill_root_only_sharing_oracle : mutant_root_only_oracle_is_refuted.
Proof. vm_compute. repeat split; try reflexivity; try discriminate. Qed.

Inductive physical_ownership_mutant_id : Type :=
| SemanticPrefixIsolationMutant
| OwnedFocusedMakeMapMutant
| SharedCompressedSiblingMutant
| ReleaseIgnoresOwnersMutant
| ArcCloneRawHeaderMutant
| IndependentRebuildNodeReuseMutant
| UniqueValueOnlyAnchorReplacementMutant
| SharedValueOnlyAliasMutationMutant
| ForbidImmutableOffPathSharingMutant
| StructuralFootprintSharingMutant
| FinalWrapperInternalNodeUniquenessMutant
| CloneClearsOnlyFreshBitMutant
| WrapperOnlyRemovalGateMutant
| RootOnlySharingOracleMutant.

Definition physical_ownership_mutant_catalog :
    list physical_ownership_mutant_id :=
  [SemanticPrefixIsolationMutant;
   OwnedFocusedMakeMapMutant;
   SharedCompressedSiblingMutant;
   ReleaseIgnoresOwnersMutant;
   ArcCloneRawHeaderMutant;
   IndependentRebuildNodeReuseMutant;
   UniqueValueOnlyAnchorReplacementMutant;
   SharedValueOnlyAliasMutationMutant;
   ForbidImmutableOffPathSharingMutant;
   StructuralFootprintSharingMutant;
   FinalWrapperInternalNodeUniquenessMutant;
   CloneClearsOnlyFreshBitMutant;
   WrapperOnlyRemovalGateMutant;
   RootOnlySharingOracleMutant].

Definition physical_ownership_mutant_killed
    (mutant : physical_ownership_mutant_id) : Prop :=
  match mutant with
  | SemanticPrefixIsolationMutant =>
      ~ mutant_semantic_focus_implies_isolation
  | OwnedFocusedMakeMapMutant =>
      ~ mutant_owned_focused_make_map_is_release_safe
  | SharedCompressedSiblingMutant =>
      mutant_shared_prefix_sibling_after_release <>
      anchor_nodes shared_prefix_sibling_fixture
  | ReleaseIgnoresOwnersMutant =>
      mutant_release_ignores_physical_owners
          compressed_store_fixture interior_focus_fixture
          shared_prefix_sibling_fixture <>
      release_survivors interior_focus_fixture shared_prefix_sibling_fixture
  | ArcCloneRawHeaderMutant =>
      anchor_raw_headers
          (mutant_arc_clone_creates_raw_header compressed_store_fixture) <>
      anchor_raw_headers (shared_arc_clone compressed_store_fixture)
  | IndependentRebuildNodeReuseMutant =>
      ~ physically_disjoint
          (anchor_nodes mutant_rebuild_reuses_focused_node)
          (anchor_nodes compressed_store_fixture)
  | UniqueValueOnlyAnchorReplacementMutant =>
      anchor_identity
          (mutant_unique_value_only_update compressed_store_fixture) <>
      anchor_identity
          (unique_value_only_update compressed_store_fixture)
  | SharedValueOnlyAliasMutationMutant =>
      anchor_observation
          (snd (mutant_shared_value_only_update
            independently_rebuilt_focus_fixture compressed_store_fixture)) <>
      anchor_observation
          (snd (shared_value_only_update
            independently_rebuilt_focus_fixture compressed_store_fixture))
  | ForbidImmutableOffPathSharingMutant =>
      ~ mutant_forbids_immutable_off_path_sharing
  | StructuralFootprintSharingMutant =>
      ~ structural_candidate_disjoint
          immutable_alias_region_fixture
          mutant_structural_region_reuses_source_footprint
  | FinalWrapperInternalNodeUniquenessMutant =>
      ~ mutant_final_wrapper_implies_internal_node_uniqueness
  | CloneClearsOnlyFreshBitMutant => mutant_fresh_bit_only_clone_is_refuted
  | WrapperOnlyRemovalGateMutant => mutant_wrapper_only_gate_is_refuted
  | RootOnlySharingOracleMutant => mutant_root_only_oracle_is_refuted
  end.

Theorem every_physical_ownership_mutant_has_a_causal_counterexample :
  forall mutant, physical_ownership_mutant_killed mutant.
Proof.
  intro mutant. destruct mutant; cbn.
  - apply kill_semantic_prefix_as_physical_isolation.
  - apply kill_owned_focused_make_map.
  - apply kill_shared_compressed_node_sibling_loss.
  - apply kill_destructive_release_that_ignores_other_owners.
  - apply kill_arc_clone_raw_header_duplication.
  - apply kill_independent_rebuild_node_reuse.
  - apply kill_unique_value_only_anchor_replacement.
  - apply kill_shared_value_only_alias_mutation.
  - apply kill_whole_root_disjointness_for_value_only_update.
  - apply kill_structural_node_and_value_footprint_sharing.
  - apply kill_final_wrapper_as_internal_node_uniqueness.
  - exact kill_clone_that_clears_only_the_fresh_bit.
  - exact kill_wrapper_only_removal_gate.
  - exact kill_root_only_sharing_oracle.
Qed.

Theorem physical_ownership_mutant_catalog_is_unique_complete_and_exact :
  NoDup physical_ownership_mutant_catalog /\
  Forall physical_ownership_mutant_killed physical_ownership_mutant_catalog /\
  length physical_ownership_mutant_catalog = 14.
Proof.
  repeat split.
  - repeat constructor; simpl; intuition discriminate.
  - apply Forall_forall. intros mutant _.
    apply every_physical_ownership_mutant_has_a_causal_counterexample.
Qed.

Print Assumptions every_physical_ownership_mutant_has_a_causal_counterexample.
Print Assumptions physical_ownership_mutant_catalog_is_unique_complete_and_exact.
Print Assumptions kill_clone_that_clears_only_the_fresh_bit.
Print Assumptions kill_wrapper_only_removal_gate.
Print Assumptions kill_root_only_sharing_oracle.

Definition empty_sentinel_fixture : live_write_state :=
  LiveWriteState (PhysicalAnchor 0 RootEmptySentinel [] [] 1 1 WholeRootOwner)
    true 0 false.

Definition open_empty_sentinel_fixture : live_write_state :=
  LiveWriteState (live_anchor empty_sentinel_fixture) true 0 true.

Definition topology_only_fixture : live_write_state :=
  LiveWriteState (PhysicalAnchor 0 (RootAllocated (0, 0)) [(0, 0)] []
    1 1 WholeRootOwner) false 0 false.

Definition prefix_sibling_fixture : live_write_state :=
  LiveWriteState shared_prefix_sibling_fixture false 1 false.

Definition mutant_normalize_by_value_count (state : live_write_state) :=
  if Nat.eqb (live_value_count state) 0 then
    fresh_live_anchor (live_anchor state)
  else live_anchor state.

Definition mutant_normalize_by_focus_empty
    (state : live_write_state) (focus_empty : bool) :=
  if focus_empty then fresh_live_anchor (live_anchor state) else live_anchor state.

Definition mutant_success_only_normalization (state : live_write_state)
    (outcome : option nat) :=
  match outcome with
  | Some _ => finish_live_write state outcome
  | None => (close_live_cursor state, outcome)
  end.

Theorem live_write_negative_controls_are_nonvacuous :
  live_write_well_formed empty_sentinel_fixture /\
  live_write_well_formed topology_only_fixture /\
  live_write_well_formed prefix_sibling_fixture /\
  ~ write_ready (live_anchor empty_sentinel_fixture) /\
  write_ready (live_anchor (fst (finish_live_write empty_sentinel_fixture 7))) /\
  normalize_closed_live_write open_empty_sentinel_fixture = None /\
  ~ write_ready (live_anchor
      (fst (mutant_success_only_normalization empty_sentinel_fixture None))) /\
  mutant_normalize_by_value_count topology_only_fixture <>
      live_anchor (fst (finish_live_write topology_only_fixture 7)) /\
  anchor_logical_paths (mutant_normalize_by_focus_empty prefix_sibling_fixture true) <>
      anchor_logical_paths (live_anchor (fst (finish_live_write prefix_sibling_fixture 7))).
Proof.
  cbv; intuition congruence.
Qed.

Print Assumptions live_write_negative_controls_are_nonvacuous.

End PathMapPhysicalOwnershipMutants.
