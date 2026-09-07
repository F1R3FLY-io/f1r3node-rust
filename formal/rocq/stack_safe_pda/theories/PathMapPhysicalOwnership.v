From Stdlib Require Import Bool List Arith Lia.

Import ListNotations.
Set Implicit Arguments.

(** Physical ownership refinement for guarded PathMap integration.

    Existing trie models describe logical endpoints.  They cannot establish
    that two raw PathMap headers own disjoint compressed nodes.  This model
    makes allocation identities, raw-header authority, root representation,
    and destructive footprints explicit. *)
Module PathMapPhysicalOwnership.

Definition byte := nat.
Definition path := list byte.
Definition allocation_id := nat.
Definition node_offset := nat.
Definition node_id := (allocation_id * node_offset)%type.
Definition value_id := (allocation_id * node_offset)%type.

Definition node_id_eq_dec : forall left right : node_id,
    {left = right} + {left <> right}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition allocated_nodes (allocation : allocation_id) (width : nat)
    : list node_id :=
  map (fun offset => (allocation, offset)) (seq 0 width).

Definition physically_disjoint (left right : list node_id) : Prop :=
  forall node, In node left -> ~ In node right.

(** Structural publication owns an allocation region, not merely a root id.
    Nodes and stored values are tracked separately because a fresh root id is
    insufficient evidence that no compressed child or value is shared. *)
Record allocation_region : Type := AllocationRegion {
  region_allocation : allocation_id;
  region_nodes : list node_id;
  region_values : list value_id
}.

Definition allocation_region_well_formed
    (region : allocation_region) : Prop :=
  (forall node, In node (region_nodes region) ->
    fst node = region_allocation region) /\
  (forall value, In value (region_values region) ->
    fst value = region_allocation region).

Definition allocation_regions_disjoint
    (left right : allocation_region) : Prop :=
  physically_disjoint (region_nodes left) (region_nodes right) /\
  physically_disjoint (region_values left) (region_values right).

Definition structural_candidate_disjoint := allocation_regions_disjoint.

Theorem distinct_well_formed_regions_are_structurally_disjoint : forall
    source candidate,
  allocation_region_well_formed source ->
  allocation_region_well_formed candidate ->
  region_allocation source <> region_allocation candidate ->
  structural_candidate_disjoint source candidate.
Proof.
  intros source candidate [source_nodes source_values]
    [candidate_nodes candidate_values] different.
  split; unfold physically_disjoint;
    intros identifier in_source in_candidate.
  - pose proof (source_nodes identifier in_source) as source_owner.
    pose proof (candidate_nodes identifier in_candidate) as candidate_owner.
    congruence.
  - pose proof (source_values identifier in_source) as source_owner.
    pose proof (candidate_values identifier in_candidate) as candidate_owner.
    congruence.
Qed.

Record value_write_footprint : Type := ValueWriteFootprint {
  value_write_nodes : list node_id;
  value_write_values : list value_id
}.

(** [write] is the post-[Arc::make_mut] write set.  Immutable nodes outside
    this set may remain shared; only storage that will actually be mutated must
    be detached from an external alias. *)
Definition value_update_write_isolated
    (write : value_write_footprint) (external_alias : allocation_region) : Prop :=
  physically_disjoint (value_write_nodes write) (region_nodes external_alias) /\
  physically_disjoint (value_write_values write) (region_values external_alias).

Lemma allocated_node_has_its_allocation : forall allocation width node,
  In node (allocated_nodes allocation width) -> fst node = allocation.
Proof.
  intros allocation width [node_allocation node_offset] present.
  unfold allocated_nodes in present.
  apply in_map_iff in present as [offset [same _]].
  inversion same. reflexivity.
Qed.

Theorem independent_allocations_are_physically_disjoint : forall
    left_allocation right_allocation left_width right_width,
  left_allocation <> right_allocation ->
  physically_disjoint
    (allocated_nodes left_allocation left_width)
    (allocated_nodes right_allocation right_width).
Proof.
  intros left_allocation right_allocation left_width right_width different.
  unfold physically_disjoint. intros node in_left in_right.
  pose proof (@allocated_node_has_its_allocation
    left_allocation left_width node in_left) as left_owner.
  pose proof (@allocated_node_has_its_allocation
    right_allocation right_width node in_right) as right_owner.
  congruence.
Qed.

Inductive root_mode : Type :=
| RootUninitialized
| RootAllocated (root : node_id)
| RootEmptySentinel.

Inductive destructive_capability : Type :=
| WholeRootOwner
| InteriorFocusOwner (absolute_focus : path).

Record physical_anchor : Type := PhysicalAnchor {
  anchor_identity : nat;
  anchor_root_mode : root_mode;
  anchor_nodes : list node_id;
  anchor_logical_paths : list path;
  anchor_raw_headers : nat;
  anchor_arc_wrappers : nat;
  anchor_destructive_capability : destructive_capability
}.

Definition anchor_observation (anchor : physical_anchor) :=
  (anchor_root_mode anchor, anchor_nodes anchor, anchor_logical_paths anchor).

Definition external_alias_observation_preserved
    (before after : physical_anchor) : Prop :=
  anchor_observation after = anchor_observation before.

Definition write_ready (anchor : physical_anchor) : Prop :=
  match anchor_root_mode anchor with
  | RootEmptySentinel => False
  | _ => True
  end.

Definition shared_arc_clone (anchor : physical_anchor) : physical_anchor :=
  PhysicalAnchor
    (anchor_identity anchor)
    (anchor_root_mode anchor)
    (anchor_nodes anchor)
    (anchor_logical_paths anchor)
    (anchor_raw_headers anchor)
    (S (anchor_arc_wrappers anchor))
    (anchor_destructive_capability anchor).

Theorem shared_arc_clone_creates_no_raw_owner : forall anchor,
  anchor_raw_headers (shared_arc_clone anchor) = anchor_raw_headers anchor /\
  anchor_nodes (shared_arc_clone anchor) = anchor_nodes anchor /\
  anchor_logical_paths (shared_arc_clone anchor) = anchor_logical_paths anchor.
Proof. intros. repeat split; reflexivity. Qed.

Definition independent_rebuild
    (anchor allocation : nat) (paths : list path) : physical_anchor :=
  PhysicalAnchor anchor
    (match paths with
     | [] => RootUninitialized
     | _ :: _ => RootAllocated (allocation, 0)
     end)
    (allocated_nodes allocation (length paths))
    paths 1 1 WholeRootOwner.

(** [independent_rebuild] remains a compact logical fixture for older proofs.
    Production refinement uses an explicit region so prefix compression may
    allocate any number of physical nodes and values. *)
Definition independent_rebuild_from_region
    (anchor : nat) (paths : list path) (region : allocation_region)
    : physical_anchor :=
  PhysicalAnchor anchor
    (match paths with
     | [] => RootUninitialized
     | _ :: _ => RootAllocated (region_allocation region, 0)
     end)
    (region_nodes region) paths 1 1 WholeRootOwner.

Definition region_covers_rebuild_root
    (paths : list path) (region : allocation_region) : Prop :=
  match paths with
  | [] => True
  | _ :: _ => In (region_allocation region, 0) (region_nodes region)
  end.

Theorem independent_region_rebuild_uses_actual_node_footprint : forall
    anchor paths region,
  anchor_nodes (independent_rebuild_from_region anchor paths region) =
    region_nodes region.
Proof. reflexivity. Qed.

Theorem independent_region_rebuilds_are_structurally_disjoint : forall
    source_anchor source_paths source_region
    candidate_anchor candidate_paths candidate_region,
  allocation_region_well_formed source_region ->
  allocation_region_well_formed candidate_region ->
  region_covers_rebuild_root source_paths source_region ->
  region_covers_rebuild_root candidate_paths candidate_region ->
  region_allocation source_region <> region_allocation candidate_region ->
  structural_candidate_disjoint source_region candidate_region /\
  physically_disjoint
    (anchor_nodes
      (independent_rebuild_from_region source_anchor source_paths source_region))
    (anchor_nodes
      (independent_rebuild_from_region
        candidate_anchor candidate_paths candidate_region)).
Proof.
  intros source_anchor source_paths source_region
    candidate_anchor candidate_paths candidate_region
    source_well_formed candidate_well_formed _ _ different.
  pose proof (distinct_well_formed_regions_are_structurally_disjoint
    (source := source_region) (candidate := candidate_region)
    source_well_formed candidate_well_formed different) as disjoint.
  split; [exact disjoint|].
  exact (proj1 disjoint).
Qed.

Theorem independent_rebuild_is_physically_disjoint : forall
    source_anchor source_allocation source_paths
    rebuilt_anchor rebuilt_allocation rebuilt_paths,
  source_allocation <> rebuilt_allocation ->
  physically_disjoint
    (anchor_nodes
      (independent_rebuild source_anchor source_allocation source_paths))
    (anchor_nodes
      (independent_rebuild rebuilt_anchor rebuilt_allocation rebuilt_paths)).
Proof.
  intros. unfold independent_rebuild. simpl.
  now apply independent_allocations_are_physically_disjoint.
Qed.

Definition release_survivors
    (subject sibling : physical_anchor) : list node_id :=
  filter
    (fun node => if in_dec node_id_eq_dec node (anchor_nodes subject)
                 then false else true)
    (anchor_nodes sibling).

Definition physical_image_intact_after_release
    (subject sibling : physical_anchor) : Prop :=
  release_survivors subject sibling = anchor_nodes sibling.

Theorem release_preserves_disjoint_anchor_images : forall subject sibling,
  physically_disjoint (anchor_nodes subject) (anchor_nodes sibling) ->
  physical_image_intact_after_release subject sibling.
Proof.
  intros subject sibling disjoint.
  unfold physical_image_intact_after_release, release_survivors.
  induction (anchor_nodes sibling) as [|node rest IH]; simpl.
  - reflexivity.
  - destruct (in_dec node_id_eq_dec node (anchor_nodes subject)) as
        [in_subject|not_in_subject].
    + exfalso. exact (disjoint node in_subject (or_introl eq_refl)).
    + simpl. f_equal. apply IH.
      intros candidate in_subject in_rest.
      exact (disjoint candidate in_subject (or_intror in_rest)).
Qed.

Theorem physical_image_intact_requires_disjointness : forall subject sibling,
  physical_image_intact_after_release subject sibling ->
  physically_disjoint (anchor_nodes subject) (anchor_nodes sibling).
Proof.
  intros subject sibling intact node in_subject in_sibling.
  unfold physical_image_intact_after_release, release_survivors in intact.
  rewrite <- intact in in_sibling.
  apply filter_In in in_sibling as [_ retained].
  destruct (in_dec node_id_eq_dec node (anchor_nodes subject));
    [discriminate|contradiction].
Qed.

Theorem physical_image_intact_iff_disjoint : forall subject sibling,
  physical_image_intact_after_release subject sibling <->
  physically_disjoint (anchor_nodes subject) (anchor_nodes sibling).
Proof.
  split; [apply physical_image_intact_requires_disjointness|
          apply release_preserves_disjoint_anchor_images].
Qed.

(** Value-only CoW and terminal release have different authority boundaries.
    A guarded [set_val] needs only an isolated post-CoW write footprint and an
    unchanged external alias observation.  It neither requires nor implies
    complete disjointness of immutable off-path nodes. *)
Definition cow_value_update_preserves_alias
    (write : value_write_footprint) (alias_region : allocation_region)
    (alias_before alias_after : physical_anchor) : Prop :=
  value_update_write_isolated write alias_region /\
  external_alias_observation_preserved alias_before alias_after.

Theorem isolated_cow_value_update_preserves_external_alias : forall
    write alias_region alias,
  value_update_write_isolated write alias_region ->
  cow_value_update_preserves_alias write alias_region alias alias.
Proof. intros. split; [assumption|reflexivity]. Qed.

Definition unique_value_only_update (anchor : physical_anchor) : physical_anchor :=
  anchor.

Theorem unique_value_only_update_retains_anchor_identity : forall anchor,
  anchor_identity (unique_value_only_update anchor) = anchor_identity anchor.
Proof. reflexivity. Qed.

Definition shared_value_only_update
    (updated_authority external_alias : physical_anchor)
    : physical_anchor * physical_anchor :=
  (updated_authority, external_alias).

Theorem shared_value_only_update_preserves_external_alias : forall
    updated_authority external_alias,
  snd (shared_value_only_update updated_authority external_alias) =
    external_alias /\
  external_alias_observation_preserved external_alias
    (snd (shared_value_only_update updated_authority external_alias)).
Proof. intros. split; reflexivity. Qed.

Definition off_path_sharing
    (left right : allocation_region) : Prop :=
  exists node, In node (region_nodes left) /\ In node (region_nodes right).

Definition isolated_write_fixture : value_write_footprint :=
  ValueWriteFootprint [(1, 0)] [(1, 0)].

Definition updated_region_fixture : allocation_region :=
  AllocationRegion 1 [(1, 0); (0, 1)] [(1, 0); (0, 2)].

Definition immutable_alias_region_fixture : allocation_region :=
  AllocationRegion 0 [(0, 1)] [(0, 2)].

Theorem isolated_value_write_allows_immutable_off_path_sharing :
  value_update_write_isolated
    isolated_write_fixture immutable_alias_region_fixture /\
  off_path_sharing updated_region_fixture immutable_alias_region_fixture.
Proof.
  split.
  - split; unfold physically_disjoint; intros [allocation offset] in_write;
      simpl in in_write; destruct in_write as [same|[]]; inversion same;
      simpl; intuition congruence.
  - exists (0, 1). simpl. auto.
Qed.

Definition compressed_store_fixture : physical_anchor :=
  PhysicalAnchor 0 (RootAllocated (0, 0)) [(0, 0); (0, 1)]
    [[7; 1]; [7; 2]] 1 1 WholeRootOwner.

(** Both logical branches occupy one compressed physical node.  An interior
    focus can therefore have exact prefix semantics without owning a detached
    physical node. *)
Definition interior_focus_fixture : physical_anchor :=
  PhysicalAnchor 1 (RootAllocated (0, 1)) [(0, 1)]
    [[1]] 1 1 (InteriorFocusOwner [7; 1]).

Definition shared_prefix_sibling_fixture : physical_anchor :=
  PhysicalAnchor 0 (RootAllocated (0, 0)) [(0, 1)]
    [[7; 2]] 1 1 WholeRootOwner.

Theorem semantic_focus_does_not_imply_physical_isolation :
  anchor_logical_paths interior_focus_fixture = [[1]] /\
  In [7; 1] (anchor_logical_paths compressed_store_fixture) /\
  In [7; 2] (anchor_logical_paths compressed_store_fixture) /\
  ~ physically_disjoint
      (anchor_nodes interior_focus_fixture)
      (anchor_nodes shared_prefix_sibling_fixture).
Proof.
  repeat split; simpl; auto.
  unfold physically_disjoint. intro isolated.
  specialize (isolated (0, 1)). simpl in isolated. tauto.
Qed.

Theorem focused_raw_owner_release_can_destroy_shared_prefix_sibling :
  ~ physical_image_intact_after_release
      interior_focus_fixture shared_prefix_sibling_fixture.
Proof. vm_compute. discriminate. Qed.

Theorem interior_focus_owner_requires_physical_detachment : forall subject sibling,
  (exists focus,
      anchor_destructive_capability subject = InteriorFocusOwner focus) ->
  physical_image_intact_after_release subject sibling ->
  physically_disjoint (anchor_nodes subject) (anchor_nodes sibling).
Proof.
  intros subject sibling _ intact.
  now apply physical_image_intact_requires_disjointness.
Qed.

Definition independently_rebuilt_focus_fixture : physical_anchor :=
  independent_rebuild 2 1 [[1]].

Theorem independently_rebuilt_focus_preserves_store_image :
  physically_disjoint
    (anchor_nodes independently_rebuilt_focus_fixture)
    (anchor_nodes compressed_store_fixture) /\
  physical_image_intact_after_release
    independently_rebuilt_focus_fixture compressed_store_fixture.
Proof.
  split.
  - unfold physically_disjoint. intros [allocation offset] rebuilt store.
    simpl in rebuilt, store. destruct rebuilt as [rebuilt|[]].
    inversion rebuilt. subst. simpl in store. intuition congruence.
  - apply release_preserves_disjoint_anchor_images.
    unfold physically_disjoint. intros [allocation offset] rebuilt store.
    simpl in rebuilt, store. destruct rebuilt as [rebuilt|[]].
    inversion rebuilt. subst. simpl in store. intuition congruence.
Qed.

Inductive physical_ownership_invariant_id : Type :=
| AllocatedRootWriteReadyInvariant
| SharedArcNoRawOwnerInvariant
| IndependentRegionRebuildDisjointnessInvariant
| DisjointReleasePreservationInvariant
| ValueUpdateWriteIsolationInvariant
| UniqueValueOnlyAnchorIdentityInvariant
| ExternalAliasObservationInvariant
| ImmutableOffPathSharingInvariant
| SemanticFocusNotPhysicalIsolationInvariant
| InteriorFocusDetachmentInvariant
| FocusedRawOwnerSiblingLossInvariant.

Definition physical_ownership_invariant_catalog :
    list physical_ownership_invariant_id :=
  [AllocatedRootWriteReadyInvariant;
   SharedArcNoRawOwnerInvariant;
   IndependentRegionRebuildDisjointnessInvariant;
   DisjointReleasePreservationInvariant;
   ValueUpdateWriteIsolationInvariant;
   UniqueValueOnlyAnchorIdentityInvariant;
   ExternalAliasObservationInvariant;
   ImmutableOffPathSharingInvariant;
   SemanticFocusNotPhysicalIsolationInvariant;
   InteriorFocusDetachmentInvariant;
   FocusedRawOwnerSiblingLossInvariant].

Definition physical_ownership_invariant_binding
    (invariant : physical_ownership_invariant_id) : Prop :=
  match invariant with
  | AllocatedRootWriteReadyInvariant =>
      forall anchor root,
        anchor_root_mode anchor = RootAllocated root -> write_ready anchor
  | SharedArcNoRawOwnerInvariant =>
      forall anchor,
        anchor_raw_headers (shared_arc_clone anchor) = anchor_raw_headers anchor
  | IndependentRegionRebuildDisjointnessInvariant =>
      forall source candidate,
        allocation_region_well_formed source ->
        allocation_region_well_formed candidate ->
        region_allocation source <> region_allocation candidate ->
        structural_candidate_disjoint source candidate
  | DisjointReleasePreservationInvariant =>
      forall subject sibling,
        physically_disjoint (anchor_nodes subject) (anchor_nodes sibling) ->
        physical_image_intact_after_release subject sibling
  | ValueUpdateWriteIsolationInvariant =>
      value_update_write_isolated
        isolated_write_fixture immutable_alias_region_fixture
  | UniqueValueOnlyAnchorIdentityInvariant =>
      forall anchor,
        anchor_identity (unique_value_only_update anchor) =
        anchor_identity anchor
  | ExternalAliasObservationInvariant =>
      forall updated_authority external_alias,
        external_alias_observation_preserved external_alias
          (snd (shared_value_only_update updated_authority external_alias))
  | ImmutableOffPathSharingInvariant =>
      off_path_sharing updated_region_fixture immutable_alias_region_fixture
  | SemanticFocusNotPhysicalIsolationInvariant =>
      ~ physically_disjoint
          (anchor_nodes interior_focus_fixture)
          (anchor_nodes shared_prefix_sibling_fixture)
  | InteriorFocusDetachmentInvariant =>
      forall subject sibling,
        (exists focus,
          anchor_destructive_capability subject = InteriorFocusOwner focus) ->
        physical_image_intact_after_release subject sibling ->
        physically_disjoint (anchor_nodes subject) (anchor_nodes sibling)
  | FocusedRawOwnerSiblingLossInvariant =>
      ~ physical_image_intact_after_release
          interior_focus_fixture shared_prefix_sibling_fixture
  end.

Theorem every_physical_ownership_invariant_is_bound : forall invariant,
  physical_ownership_invariant_binding invariant.
Proof.
  intro invariant. destruct invariant; cbn.
  - intros anchor root allocated. unfold write_ready. now rewrite allocated.
  - intro anchor. exact (proj1 (shared_arc_clone_creates_no_raw_owner anchor)).
  - apply distinct_well_formed_regions_are_structurally_disjoint.
  - apply release_preserves_disjoint_anchor_images.
  - exact (proj1 isolated_value_write_allows_immutable_off_path_sharing).
  - apply unique_value_only_update_retains_anchor_identity.
  - intros. exact (proj2
      (@shared_value_only_update_preserves_external_alias
        updated_authority external_alias)).
  - exact (proj2 isolated_value_write_allows_immutable_off_path_sharing).
  - exact (proj2 (proj2 (proj2 semantic_focus_does_not_imply_physical_isolation))).
  - apply interior_focus_owner_requires_physical_detachment.
  - apply focused_raw_owner_release_can_destroy_shared_prefix_sibling.
Qed.

Theorem physical_ownership_invariant_catalog_is_unique_complete_and_exact :
  NoDup physical_ownership_invariant_catalog /\
  Forall physical_ownership_invariant_binding
    physical_ownership_invariant_catalog /\
  length physical_ownership_invariant_catalog = 11.
Proof.
  repeat split.
  - repeat constructor; simpl; intuition discriminate.
  - apply Forall_forall. intros invariant _.
    apply every_physical_ownership_invariant_is_bound.
Qed.

Print Assumptions independent_rebuild_is_physically_disjoint.
Print Assumptions independent_region_rebuilds_are_structurally_disjoint.
Print Assumptions isolated_value_write_allows_immutable_off_path_sharing.
Print Assumptions unique_value_only_update_retains_anchor_identity.
Print Assumptions release_preserves_disjoint_anchor_images.
Print Assumptions semantic_focus_does_not_imply_physical_isolation.
Print Assumptions focused_raw_owner_release_can_destroy_shared_prefix_sibling.
Print Assumptions physical_ownership_invariant_catalog_is_unique_complete_and_exact.

Record live_write_state : Type := LiveWriteState {
  live_anchor : physical_anchor;
  live_empty : bool;
  live_value_count : nat;
  live_cursor_open : bool
}.

Definition live_write_well_formed (state : live_write_state) : Prop :=
  (live_empty state = true ->
    anchor_logical_paths (live_anchor state) = [] /\ live_value_count state = 0) /\
  (anchor_root_mode (live_anchor state) = RootEmptySentinel -> live_empty state = true).

Definition fresh_live_anchor (anchor : physical_anchor) : physical_anchor :=
  PhysicalAnchor (anchor_identity anchor) RootUninitialized [] []
    (anchor_raw_headers anchor) (anchor_arc_wrappers anchor)
    (anchor_destructive_capability anchor).

Definition close_live_cursor (state : live_write_state) : live_write_state :=
  LiveWriteState (live_anchor state) (live_empty state) (live_value_count state) false.

Definition normalized_live_image (state : live_write_state) : live_write_state :=
  if live_empty state then
    LiveWriteState (fresh_live_anchor (live_anchor state)) true 0 (live_cursor_open state)
  else state.

Definition normalize_closed_live_write (state : live_write_state)
    : option live_write_state :=
  if live_cursor_open state then None else Some (normalized_live_image state).

Definition finish_live_write {Outcome : Type}
    (state : live_write_state) (outcome : Outcome) : live_write_state * Outcome :=
  (normalized_live_image (close_live_cursor state), outcome).

Theorem live_normalization_requires_closed_cursor : forall state,
  normalize_closed_live_write state = None <-> live_cursor_open state = true.
Proof.
  intros state. unfold normalize_closed_live_write.
  destruct (live_cursor_open state); split; congruence.
Qed.

Theorem live_normalization_is_idempotent : forall state,
  normalized_live_image (normalized_live_image state) = normalized_live_image state.
Proof.
  intros [anchor empty count open]. unfold normalized_live_image; simpl.
  destruct empty; [destruct anchor|]; reflexivity.
Qed.

Theorem closed_live_image_is_write_ready : forall state,
  live_write_well_formed state ->
  write_ready (live_anchor (normalized_live_image state)).
Proof.
  intros state [_ sentinel_empty]. unfold normalized_live_image.
  destruct (live_empty state) eqn:empty; [exact I|].
  unfold write_ready.
  destruct (anchor_root_mode (live_anchor state)) eqn:mode; try exact I.
  specialize (sentinel_empty eq_refl). congruence.
Qed.

Theorem live_write_gateway_capstone : forall Outcome state (outcome : Outcome),
  live_write_well_formed state ->
  let after := fst (finish_live_write state outcome) in
  live_cursor_open after = false /\
  normalize_closed_live_write (close_live_cursor state) = Some after /\
  write_ready (live_anchor after) /\
  anchor_logical_paths (live_anchor after) = anchor_logical_paths (live_anchor state) /\
  live_value_count after = live_value_count state /\
  anchor_identity (live_anchor after) = anchor_identity (live_anchor state) /\
  anchor_raw_headers (live_anchor after) = anchor_raw_headers (live_anchor state) /\
  anchor_arc_wrappers (live_anchor after) = anchor_arc_wrappers (live_anchor state) /\
  snd (finish_live_write state outcome) = outcome.
Proof.
  intros Outcome state outcome well_formed.
  assert (closed_well_formed : live_write_well_formed (close_live_cursor state)).
  { exact well_formed. }
  pose proof (closed_live_image_is_write_ready closed_well_formed) as ready.
  destruct state as [anchor empty count open].
  destruct well_formed as [empty_sound sentinel_empty].
  unfold finish_live_write, close_live_cursor, normalize_closed_live_write,
    normalized_live_image in *; simpl in *.
  destruct empty; simpl in *.
  - destruct (empty_sound eq_refl) as [paths count_zero].
    repeat split; try reflexivity; try exact I; congruence.
  - repeat split; try reflexivity; exact ready.
Qed.

Theorem live_write_empty_and_nonempty_images_are_exact : forall Outcome state
    (outcome : Outcome),
  (live_empty state = true ->
    live_anchor (fst (finish_live_write state outcome)) =
      fresh_live_anchor (live_anchor state)) /\
  (live_empty state = false ->
    live_anchor (fst (finish_live_write state outcome)) = live_anchor state).
Proof.
  intros Outcome [anchor empty count open] outcome.
  unfold finish_live_write, close_live_cursor, normalized_live_image; simpl.
  destruct empty; simpl; split; intros; congruence.
Qed.

Print Assumptions live_normalization_requires_closed_cursor.
Print Assumptions live_normalization_is_idempotent.
Print Assumptions live_write_gateway_capstone.
Print Assumptions live_write_empty_and_nonempty_images_are_exact.

End PathMapPhysicalOwnership.
