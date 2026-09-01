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
| FinalWrapperInternalNodeUniquenessMutant.

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
   FinalWrapperInternalNodeUniquenessMutant].

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
Qed.

Theorem physical_ownership_mutant_catalog_is_unique_complete_and_exact :
  NoDup physical_ownership_mutant_catalog /\
  Forall physical_ownership_mutant_killed physical_ownership_mutant_catalog /\
  length physical_ownership_mutant_catalog = 11.
Proof.
  repeat split.
  - repeat constructor; simpl; intuition discriminate.
  - apply Forall_forall. intros mutant _.
    apply every_physical_ownership_mutant_has_a_causal_counterexample.
Qed.

Print Assumptions every_physical_ownership_mutant_has_a_causal_counterexample.
Print Assumptions physical_ownership_mutant_catalog_is_unique_complete_and_exact.

End PathMapPhysicalOwnershipMutants.
