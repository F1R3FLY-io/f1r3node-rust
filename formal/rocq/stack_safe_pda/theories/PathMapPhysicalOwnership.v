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

(** * Exclusive provenance of anchor roots

    Bug 14492 (option A') removes an exhausted Bag endpoint in place only
    when the anchor is provably exclusive: [Arc::get_mut] on the guarded
    wrapper succeeds and the anchor root's [exclusive] flag is set.  This
    section models the flag's life cycle as a provenance bit paired with each
    [physical_anchor]; the record [physical_anchor] itself is unchanged.

    A world lists anchors by position.  Every creation prepends, so the
    positions of existing anchors shift by one.  An anchor whose last wrapper
    drops stays in the list as a tombstone with no wrapper, no node and a
    cleared bit.  Fresh nodes come from an allocation that no anchor has used,
    recorded by [provenance_next_allocation].  Logical paths are not tracked
    by this section.

    Steps and their Rust counterparts:
    - [ProvenanceConstruct]: [AnchorRoot::from_map] on an empty map, and the
      rebuilt map of a fallback removal; fresh nodes, bit set, one wrapper.
    - [ProvenanceWrapperClone] and [ProvenanceWrapperDrop]: [Arc::clone] and
      drop of a [GuardedPathMap].  The wrappers share one anchor root and its
      bit, which neither step changes (ruling R5); the last drop retires the
      anchor.
    - [ProvenanceHeaderClone]: [Arc::make_mut] on a shared wrapper runs
      [AnchorRoot::clone], which clears the source's flag; the copy starts
      cleared, holds one wrapper and shares every node of the source.
    - [ProvenanceUniqueWrite]: a write through a unique wrapper.  The write
      zipper path-copies the nodes in [copied] into a fresh allocation and
      may add fresh nodes.  [materialize_missing_suffix] drops its donor
      graft before it returns, so grafted nodes are modelled as fresh.  The
      bit is unchanged: no write sets the flag again (ruling R6).
    - [ProvenanceExhaustingRemoval]: the policy's removal gate decides.  When
      it holds, the anchor releases [released] in place and keeps its
      identity.  When it fails, the remaining entries are rebuilt into a fresh
      exclusive anchor and the remover's wrapper of the old anchor drops.  The
      barrier node splits of an in-place removal are a unique write followed
      by the release.

    [provenance_policy] isolates the two decisions that the negative controls
    of PathMapPhysicalOwnershipMutants.v change: the source bit after a header
    clone and the removal gate. *)

Record provenance_anchor : Type := ProvenanceAnchor {
  provenance_physical : physical_anchor;
  provenance_exclusive : bool
}.

Record provenance_world : Type := ProvenanceWorld {
  provenance_anchors : list provenance_anchor;
  provenance_next_allocation : allocation_id;
  provenance_next_identity : nat
}.

Definition empty_provenance_world : provenance_world :=
  ProvenanceWorld [] 0 0.

Definition provenance_nodes (anchor : provenance_anchor) : list node_id :=
  anchor_nodes (provenance_physical anchor).

Definition provenance_wrappers (anchor : provenance_anchor) : nat :=
  anchor_arc_wrappers (provenance_physical anchor).

Definition provenance_absent_from (nodes : list node_id) (node : node_id)
    : bool :=
  if in_dec node_id_eq_dec node nodes then false else true.

Inductive provenance_command : Type :=
| ProvenanceConstruct (width : nat)
| ProvenanceWrapperClone (position : nat)
| ProvenanceWrapperDrop (position : nat)
| ProvenanceHeaderClone (position : nat)
| ProvenanceUniqueWrite (position : nat) (copied : list node_id) (added : nat)
| ProvenanceExhaustingRemoval (position : nat) (released : list node_id)
    (rebuilt_width : nat).

Record provenance_policy : Type := ProvenancePolicy {
  provenance_clone_source_bit : bool -> bool;
  provenance_removal_gate :
    list provenance_anchor -> nat -> provenance_anchor -> bool
}.

(** The production gate: the provenance bit and a single wrapper, the
    model's [Arc::get_mut]. *)
Definition exclusive_removal_gate (anchor : provenance_anchor) : bool :=
  provenance_exclusive anchor && Nat.eqb (provenance_wrappers anchor) 1.

Definition exclusive_provenance_policy : provenance_policy :=
  ProvenancePolicy (fun _ => false)
    (fun _ _ anchor => exclusive_removal_gate anchor).

Definition provenance_fresh_anchor
    (identity : nat) (allocation : allocation_id) (width : nat)
    : provenance_anchor :=
  ProvenanceAnchor
    (PhysicalAnchor identity
      (match width with
       | 0 => RootUninitialized
       | S _ => RootAllocated (allocation, 0)
       end)
      (allocated_nodes allocation width) [] 1 1 WholeRootOwner)
    true.

Definition provenance_wrapper_clone (anchor : provenance_anchor)
    : provenance_anchor :=
  match provenance_wrappers anchor with
  | 0 => anchor
  | S _ =>
      ProvenanceAnchor (shared_arc_clone (provenance_physical anchor))
        (provenance_exclusive anchor)
  end.

Definition provenance_retired (anchor : provenance_anchor)
    : provenance_anchor :=
  ProvenanceAnchor
    (PhysicalAnchor (anchor_identity (provenance_physical anchor))
      RootUninitialized [] [] 0 0
      (anchor_destructive_capability (provenance_physical anchor)))
    false.

Definition provenance_wrapper_drop (anchor : provenance_anchor)
    : provenance_anchor :=
  match provenance_wrappers anchor with
  | 0 => anchor
  | 1 => provenance_retired anchor
  | S remaining =>
      ProvenanceAnchor
        (PhysicalAnchor
          (anchor_identity (provenance_physical anchor))
          (anchor_root_mode (provenance_physical anchor))
          (anchor_nodes (provenance_physical anchor))
          (anchor_logical_paths (provenance_physical anchor))
          (anchor_raw_headers (provenance_physical anchor))
          remaining
          (anchor_destructive_capability (provenance_physical anchor)))
        (provenance_exclusive anchor)
  end.

Definition provenance_header_clone_source (policy : provenance_policy)
    (anchor : provenance_anchor) : provenance_anchor :=
  ProvenanceAnchor
    (PhysicalAnchor
      (anchor_identity (provenance_physical anchor))
      (anchor_root_mode (provenance_physical anchor))
      (anchor_nodes (provenance_physical anchor))
      (anchor_logical_paths (provenance_physical anchor))
      (anchor_raw_headers (provenance_physical anchor))
      (pred (anchor_arc_wrappers (provenance_physical anchor)))
      (anchor_destructive_capability (provenance_physical anchor)))
    (provenance_clone_source_bit policy (provenance_exclusive anchor)).

Definition provenance_header_clone_copy (identity : nat)
    (anchor : provenance_anchor) : provenance_anchor :=
  ProvenanceAnchor
    (PhysicalAnchor identity
      (anchor_root_mode (provenance_physical anchor))
      (anchor_nodes (provenance_physical anchor))
      (anchor_logical_paths (provenance_physical anchor))
      1 1
      (anchor_destructive_capability (provenance_physical anchor)))
    false.

Definition provenance_written_root (allocation : allocation_id)
    (copied : list node_id) (width : nat) (mode : root_mode) : root_mode :=
  match mode with
  | RootAllocated root =>
      if in_dec node_id_eq_dec root copied
      then RootAllocated (allocation, 0) else RootAllocated root
  | _ =>
      match width with
      | 0 => mode
      | S _ => RootAllocated (allocation, 0)
      end
  end.

Definition provenance_unique_write (allocation : allocation_id)
    (copied : list node_id) (added : nat) (anchor : provenance_anchor)
    : provenance_anchor :=
  ProvenanceAnchor
    (PhysicalAnchor
      (anchor_identity (provenance_physical anchor))
      (provenance_written_root allocation copied (length copied + added)
        (anchor_root_mode (provenance_physical anchor)))
      (allocated_nodes allocation (length copied + added) ++
        filter (provenance_absent_from copied)
          (anchor_nodes (provenance_physical anchor)))
      (anchor_logical_paths (provenance_physical anchor))
      (anchor_raw_headers (provenance_physical anchor))
      (anchor_arc_wrappers (provenance_physical anchor))
      (anchor_destructive_capability (provenance_physical anchor)))
    (provenance_exclusive anchor).

(** A removal that releases the root leaves the map empty, which
    [normalize_live_empty] represents as an uninitialized root. *)
Definition provenance_released_root (released : list node_id)
    (mode : root_mode) : root_mode :=
  match mode with
  | RootAllocated root =>
      if in_dec node_id_eq_dec root released
      then RootUninitialized else RootAllocated root
  | _ => mode
  end.

Definition provenance_partial_release (released : list node_id)
    (anchor : provenance_anchor) : provenance_anchor :=
  ProvenanceAnchor
    (PhysicalAnchor
      (anchor_identity (provenance_physical anchor))
      (provenance_released_root released
        (anchor_root_mode (provenance_physical anchor)))
      (filter (provenance_absent_from released)
        (anchor_nodes (provenance_physical anchor)))
      (anchor_logical_paths (provenance_physical anchor))
      (anchor_raw_headers (provenance_physical anchor))
      (anchor_arc_wrappers (provenance_physical anchor))
      (anchor_destructive_capability (provenance_physical anchor)))
    (provenance_exclusive anchor).

(** The nodes that an in-place release actually frees, as a physical anchor
    for [release_preserves_disjoint_anchor_images]. *)
Definition provenance_released_part (released : list node_id)
    (anchor : provenance_anchor) : physical_anchor :=
  PhysicalAnchor
    (anchor_identity (provenance_physical anchor))
    (anchor_root_mode (provenance_physical anchor))
    (filter (fun candidate => negb (provenance_absent_from released candidate))
      (anchor_nodes (provenance_physical anchor)))
    (anchor_logical_paths (provenance_physical anchor))
    (anchor_raw_headers (provenance_physical anchor))
    (anchor_arc_wrappers (provenance_physical anchor))
    (anchor_destructive_capability (provenance_physical anchor)).

Fixpoint provenance_update (position : nat)
    (change : provenance_anchor -> provenance_anchor)
    (anchors : list provenance_anchor) {struct anchors}
    : list provenance_anchor :=
  match anchors, position with
  | [], _ => []
  | anchor :: rest, 0 => change anchor :: rest
  | anchor :: rest, S later => anchor :: provenance_update later change rest
  end.

Definition provenance_step_with (policy : provenance_policy)
    (command : provenance_command) (world : provenance_world)
    : provenance_world :=
  match world with
  | ProvenanceWorld anchors allocation identity =>
      match command with
      | ProvenanceConstruct width =>
          ProvenanceWorld
            (provenance_fresh_anchor identity allocation width :: anchors)
            (S allocation) (S identity)
      | ProvenanceWrapperClone position =>
          ProvenanceWorld
            (provenance_update position provenance_wrapper_clone anchors)
            allocation identity
      | ProvenanceWrapperDrop position =>
          ProvenanceWorld
            (provenance_update position provenance_wrapper_drop anchors)
            allocation identity
      | ProvenanceHeaderClone position =>
          match nth_error anchors position with
          | Some anchor =>
              match provenance_wrappers anchor with
              | S (S _) =>
                  ProvenanceWorld
                    (provenance_header_clone_copy identity anchor ::
                      provenance_update position
                        (provenance_header_clone_source policy) anchors)
                    allocation (S identity)
              | _ => world
              end
          | None => world
          end
      | ProvenanceUniqueWrite position copied added =>
          match nth_error anchors position with
          | Some anchor =>
              match provenance_wrappers anchor with
              | 1 =>
                  ProvenanceWorld
                    (provenance_update position
                      (provenance_unique_write allocation copied added)
                      anchors)
                    (S allocation) identity
              | _ => world
              end
          | None => world
          end
      | ProvenanceExhaustingRemoval position released rebuilt_width =>
          match nth_error anchors position with
          | Some anchor =>
              match provenance_wrappers anchor with
              | 0 => world
              | S _ =>
                  if provenance_removal_gate policy anchors position anchor
                  then
                    ProvenanceWorld
                      (provenance_update position
                        (provenance_partial_release released) anchors)
                      allocation identity
                  else
                    ProvenanceWorld
                      (provenance_fresh_anchor identity allocation
                          rebuilt_width ::
                        provenance_update position provenance_wrapper_drop
                          anchors)
                      (S allocation) (S identity)
              end
          | None => world
          end
      end
  end.

Definition provenance_step (command : provenance_command)
    (world : provenance_world) : provenance_world :=
  provenance_step_with exclusive_provenance_policy command world.

Fixpoint provenance_run_with (policy : provenance_policy)
    (commands : list provenance_command) (world : provenance_world)
    : provenance_world :=
  match commands with
  | [] => world
  | command :: rest =>
      provenance_run_with policy rest (provenance_step_with policy command world)
  end.

Definition provenance_run (commands : list provenance_command)
    : provenance_world :=
  provenance_run_with exclusive_provenance_policy commands
    empty_provenance_world.

(** The inductive invariant: every node lies below the next fresh allocation,
    and an anchor whose bit is set shares no node with the anchor at any other
    position. *)
Definition provenance_allocations_below (bound : allocation_id)
    (anchors : list provenance_anchor) : Prop :=
  forall position anchor node,
    nth_error anchors position = Some anchor ->
    In node (provenance_nodes anchor) -> fst node < bound.

Definition exclusive_isolation (anchors : list provenance_anchor) : Prop :=
  forall position anchor other_position other,
    nth_error anchors position = Some anchor ->
    nth_error anchors other_position = Some other ->
    other_position <> position ->
    provenance_exclusive anchor = true ->
    physically_disjoint (provenance_nodes anchor) (provenance_nodes other).

Definition provenance_invariant (world : provenance_world) : Prop :=
  provenance_allocations_below (provenance_next_allocation world)
    (provenance_anchors world) /\
  exclusive_isolation (provenance_anchors world).

Lemma provenance_option_map_some : forall
    (change : provenance_anchor -> provenance_anchor) found result,
  option_map change found = Some result ->
  exists original, found = Some original /\ result = change original.
Proof.
  intros change found result mapped.
  destruct found as [original|]; simpl in mapped.
  - injection mapped as same.
    exists original. split; [reflexivity|symmetry; exact same].
  - discriminate mapped.
Qed.

Lemma provenance_update_nth_here : forall position change anchors,
  nth_error (provenance_update position change anchors) position =
    option_map change (nth_error anchors position).
Proof.
  induction position as [|position IH]; intros change anchors;
    destruct anchors as [|anchor rest]; simpl; try reflexivity.
  apply IH.
Qed.

Lemma provenance_update_nth_other : forall position change anchors other,
  other <> position ->
  nth_error (provenance_update position change anchors) other =
    nth_error anchors other.
Proof.
  induction position as [|position IH]; intros change anchors other different;
    destruct anchors as [|anchor rest]; destruct other as [|other];
    simpl; try reflexivity.
  - exfalso. apply different. reflexivity.
  - apply IH. intro same. apply different. rewrite same. reflexivity.
Qed.

Lemma provenance_allocations_below_weaken : forall bound anchors,
  provenance_allocations_below bound anchors ->
  provenance_allocations_below (S bound) anchors.
Proof.
  intros bound anchors below position anchor node at_position present.
  pose proof (below position anchor node at_position present) as bounded.
  lia.
Qed.

Lemma provenance_update_preserves_below : forall bound position change anchors,
  provenance_allocations_below bound anchors ->
  (forall anchor node,
    In node (provenance_nodes (change anchor)) ->
    In node (provenance_nodes anchor) \/ fst node < bound) ->
  provenance_allocations_below bound
    (provenance_update position change anchors).
Proof.
  intros bound position change anchors below footprint
    found_position found node at_found in_found.
  destruct (Nat.eq_dec found_position position) as [here|elsewhere].
  - subst found_position.
    rewrite provenance_update_nth_here in at_found.
    destruct (@provenance_option_map_some change (nth_error anchors position)
        found at_found) as [original [original_at found_is_change]].
    subst found.
    destruct (footprint original node in_found) as [in_original|bounded].
    + exact (below position original node original_at in_original).
    + exact bounded.
  - rewrite (@provenance_update_nth_other position change anchors
      found_position elsewhere) in at_found.
    exact (below found_position found node at_found in_found).
Qed.

Lemma provenance_update_preserves_isolation : forall bound position change
    anchors,
  provenance_allocations_below bound anchors ->
  exclusive_isolation anchors ->
  (forall anchor,
    provenance_exclusive (change anchor) = true ->
    provenance_exclusive anchor = true) ->
  (forall anchor node,
    In node (provenance_nodes (change anchor)) ->
    In node (provenance_nodes anchor) \/ fst node = bound) ->
  exclusive_isolation (provenance_update position change anchors).
Proof.
  intros bound position change anchors below isolated monotone footprint
    first_position first second_position second at_first at_second different
    exclusive node in_first in_second.
  destruct (Nat.eq_dec first_position position)
    as [first_here|first_elsewhere].
  - subst first_position.
    rewrite provenance_update_nth_here in at_first.
    destruct (@provenance_option_map_some change (nth_error anchors position)
        first at_first) as [original [original_at first_is_change]].
    subst first.
    rewrite (@provenance_update_nth_other position change anchors
      second_position different) in at_second.
    destruct (footprint original node in_first) as [in_original|fresh].
    + exact (isolated position original second_position second original_at
        at_second different (monotone original exclusive) node in_original
        in_second).
    + pose proof (below second_position second node at_second in_second)
        as bounded.
      lia.
  - rewrite (@provenance_update_nth_other position change anchors
      first_position first_elsewhere) in at_first.
    destruct (Nat.eq_dec second_position position)
      as [second_here|second_elsewhere].
    + subst second_position.
      rewrite provenance_update_nth_here in at_second.
      destruct (@provenance_option_map_some change (nth_error anchors position)
          second at_second) as [original [original_at second_is_change]].
      subst second.
      destruct (footprint original node in_second) as [in_original|fresh].
      * exact (isolated first_position first position original at_first
          original_at different exclusive node in_first in_original).
      * pose proof (below first_position first node at_first in_first)
          as bounded.
        lia.
    + rewrite (@provenance_update_nth_other position change anchors
        second_position second_elsewhere) in at_second.
      exact (isolated first_position first second_position second at_first
        at_second different exclusive node in_first in_second).
Qed.

Lemma provenance_prepend_preserves_below : forall bound anchor anchors,
  provenance_allocations_below bound anchors ->
  (forall node, In node (provenance_nodes anchor) -> fst node < bound) ->
  provenance_allocations_below bound (anchor :: anchors).
Proof.
  intros bound anchor anchors below anchor_below
    found_position found node at_found in_found.
  destruct found_position as [|found_position]; simpl in at_found.
  - injection at_found as found_is_anchor. subst found.
    exact (anchor_below node in_found).
  - exact (below found_position found node at_found in_found).
Qed.

Lemma provenance_prepend_preserves_isolation : forall anchor anchors,
  exclusive_isolation anchors ->
  (provenance_exclusive anchor = true ->
    forall position other,
      nth_error anchors position = Some other ->
      physically_disjoint (provenance_nodes anchor) (provenance_nodes other)) ->
  (forall position other,
    nth_error anchors position = Some other ->
    provenance_exclusive other = true ->
    physically_disjoint (provenance_nodes other) (provenance_nodes anchor)) ->
  exclusive_isolation (anchor :: anchors).
Proof.
  intros anchor anchors isolated anchor_isolated others_isolated
    first_position first second_position second at_first at_second different
    exclusive.
  destruct first_position as [|first_position];
    destruct second_position as [|second_position];
    simpl in at_first, at_second.
  - exfalso. apply different. reflexivity.
  - injection at_first as first_is_anchor. subst first.
    exact (anchor_isolated exclusive second_position second at_second).
  - injection at_second as second_is_anchor. subst second.
    exact (others_isolated first_position first at_first exclusive).
  - assert (later_different : second_position <> first_position) by lia.
    exact (isolated first_position first second_position second at_first
      at_second later_different exclusive).
Qed.

Lemma provenance_fresh_anchor_nodes : forall identity allocation width node,
  In node (provenance_nodes (provenance_fresh_anchor identity allocation width)) ->
  fst node = allocation.
Proof.
  intros identity allocation width node present.
  exact (@allocated_node_has_its_allocation allocation width node present).
Qed.

Lemma provenance_fresh_prepend_preserves_invariant : forall anchors allocation
    identity width,
  provenance_allocations_below allocation anchors ->
  exclusive_isolation anchors ->
  provenance_allocations_below (S allocation)
      (provenance_fresh_anchor identity allocation width :: anchors) /\
    exclusive_isolation
      (provenance_fresh_anchor identity allocation width :: anchors).
Proof.
  intros anchors allocation identity width below isolated.
  split.
  - apply provenance_prepend_preserves_below.
    + exact (@provenance_allocations_below_weaken allocation anchors below).
    + intros node present.
      pose proof (@provenance_fresh_anchor_nodes identity allocation width node
        present) as owner.
      lia.
  - apply provenance_prepend_preserves_isolation.
    + exact isolated.
    + intros _ position other at_other node present other_present.
      pose proof (@provenance_fresh_anchor_nodes identity allocation width node
        present) as owner.
      pose proof (below position other node at_other other_present) as bounded.
      lia.
    + intros position other at_other _ node other_present present.
      pose proof (@provenance_fresh_anchor_nodes identity allocation width node
        present) as owner.
      pose proof (below position other node at_other other_present) as bounded.
      lia.
Qed.

Lemma provenance_shrinking_update_preserves_invariant : forall allocation
    position change anchors,
  provenance_allocations_below allocation anchors ->
  exclusive_isolation anchors ->
  (forall anchor,
    provenance_exclusive (change anchor) = true ->
    provenance_exclusive anchor = true) ->
  (forall anchor node,
    In node (provenance_nodes (change anchor)) ->
    In node (provenance_nodes anchor)) ->
  provenance_allocations_below allocation
      (provenance_update position change anchors) /\
    exclusive_isolation (provenance_update position change anchors).
Proof.
  intros allocation position change anchors below isolated monotone footprint.
  split.
  - apply provenance_update_preserves_below.
    + exact below.
    + intros anchor node present. left. exact (footprint anchor node present).
  - apply (@provenance_update_preserves_isolation allocation).
    + exact below.
    + exact isolated.
    + exact monotone.
    + intros anchor node present. left. exact (footprint anchor node present).
Qed.

Lemma provenance_wrapper_clone_keeps_exclusive : forall anchor,
  provenance_exclusive (provenance_wrapper_clone anchor) = true ->
  provenance_exclusive anchor = true.
Proof.
  intros [[identity root nodes paths headers wrappers capability] exclusive]
    clone_exclusive.
  destruct wrappers as [|wrappers]; exact clone_exclusive.
Qed.

Lemma provenance_wrapper_clone_keeps_nodes : forall anchor node,
  In node (provenance_nodes (provenance_wrapper_clone anchor)) ->
  In node (provenance_nodes anchor).
Proof.
  intros [[identity root nodes paths headers wrappers capability] exclusive]
    node present.
  destruct wrappers as [|wrappers]; exact present.
Qed.

Lemma provenance_wrapper_drop_keeps_exclusive : forall anchor,
  provenance_exclusive (provenance_wrapper_drop anchor) = true ->
  provenance_exclusive anchor = true.
Proof.
  intros [[identity root nodes paths headers wrappers capability] exclusive]
    drop_exclusive.
  destruct wrappers as [|[|wrappers]].
  - exact drop_exclusive.
  - discriminate drop_exclusive.
  - exact drop_exclusive.
Qed.

Lemma provenance_wrapper_drop_keeps_nodes : forall anchor node,
  In node (provenance_nodes (provenance_wrapper_drop anchor)) ->
  In node (provenance_nodes anchor).
Proof.
  intros [[identity root nodes paths headers wrappers capability] exclusive]
    node present.
  destruct wrappers as [|[|wrappers]].
  - exact present.
  - destruct present.
  - exact present.
Qed.

Lemma provenance_partial_release_keeps_exclusive : forall released anchor,
  provenance_exclusive (provenance_partial_release released anchor) = true ->
  provenance_exclusive anchor = true.
Proof.
  intros released anchor released_exclusive. exact released_exclusive.
Qed.

Lemma provenance_partial_release_keeps_nodes : forall released anchor node,
  In node (provenance_nodes (provenance_partial_release released anchor)) ->
  In node (provenance_nodes anchor).
Proof.
  intros released anchor node present.
  change (In node (filter (provenance_absent_from released)
    (provenance_nodes anchor))) in present.
  apply filter_In in present as [kept _].
  exact kept.
Qed.

Lemma provenance_unique_write_nodes : forall allocation copied added anchor
    node,
  In node
      (provenance_nodes (provenance_unique_write allocation copied added anchor)) ->
  In node (provenance_nodes anchor) \/ fst node = allocation.
Proof.
  intros allocation copied added anchor node present.
  change (In node (allocated_nodes allocation (length copied + added) ++
    filter (provenance_absent_from copied) (provenance_nodes anchor)))
    in present.
  apply in_app_or in present as [fresh|kept].
  - right.
    exact (@allocated_node_has_its_allocation allocation
      (length copied + added) node fresh).
  - left. apply filter_In in kept as [owned _]. exact owned.
Qed.

Lemma provenance_unique_write_preserves_invariant : forall allocation position
    copied added anchors,
  provenance_allocations_below allocation anchors ->
  exclusive_isolation anchors ->
  provenance_allocations_below (S allocation)
      (provenance_update position
        (provenance_unique_write allocation copied added) anchors) /\
    exclusive_isolation
      (provenance_update position
        (provenance_unique_write allocation copied added) anchors).
Proof.
  intros allocation position copied added anchors below isolated.
  split.
  - apply provenance_update_preserves_below.
    + exact (@provenance_allocations_below_weaken allocation anchors below).
    + intros anchor node present.
      destruct (@provenance_unique_write_nodes allocation copied added anchor
          node present) as [kept|fresh].
      * left. exact kept.
      * right. lia.
  - apply (@provenance_update_preserves_isolation allocation).
    + exact below.
    + exact isolated.
    + intros anchor written_exclusive. exact written_exclusive.
    + exact (@provenance_unique_write_nodes allocation copied added).
Qed.

(** A header clone keeps the invariant exactly when the source's bit is
    cleared: the copy shares every node of the source. *)
Lemma provenance_header_clone_preserves_invariant : forall policy allocation
    identity position anchors source,
  (forall bit, provenance_clone_source_bit policy bit = false) ->
  provenance_allocations_below allocation anchors ->
  exclusive_isolation anchors ->
  nth_error anchors position = Some source ->
  provenance_allocations_below allocation
      (provenance_header_clone_copy identity source ::
        provenance_update position (provenance_header_clone_source policy)
          anchors) /\
    exclusive_isolation
      (provenance_header_clone_copy identity source ::
        provenance_update position (provenance_header_clone_source policy)
          anchors).
Proof.
  intros policy allocation identity position anchors source clears below
    isolated at_source.
  assert (source_cleared : forall anchor,
    provenance_exclusive (provenance_header_clone_source policy anchor) =
      false).
  { intro anchor. exact (clears (provenance_exclusive anchor)). }
  assert (source_monotone : forall anchor,
    provenance_exclusive (provenance_header_clone_source policy anchor) =
      true ->
    provenance_exclusive anchor = true).
  { intros anchor source_exclusive.
    rewrite source_cleared in source_exclusive.
    discriminate source_exclusive. }
  assert (source_nodes : forall anchor node,
    In node (provenance_nodes (provenance_header_clone_source policy anchor)) ->
    In node (provenance_nodes anchor)).
  { intros anchor node present. exact present. }
  destruct (@provenance_shrinking_update_preserves_invariant allocation position
      (provenance_header_clone_source policy) anchors below isolated
      source_monotone source_nodes) as [updated_below updated_isolated].
  split.
  - apply provenance_prepend_preserves_below.
    + exact updated_below.
    + intros node present.
      exact (below position source node at_source present).
  - apply provenance_prepend_preserves_isolation.
    + exact updated_isolated.
    + intro copy_exclusive. discriminate copy_exclusive.
    + intros other_position other at_other other_exclusive node other_present
        copy_present.
      destruct (Nat.eq_dec other_position position) as [here|elsewhere].
      * subst other_position.
        rewrite provenance_update_nth_here, at_source in at_other.
        change (Some (provenance_header_clone_source policy source) =
          Some other) in at_other.
        injection at_other as other_is_source.
        subst other.
        rewrite (source_cleared source) in other_exclusive.
        discriminate other_exclusive.
      * rewrite (@provenance_update_nth_other position
          (provenance_header_clone_source policy) anchors other_position
          elsewhere) in at_other.
        assert (source_different : position <> other_position).
        { intro same. apply elsewhere. symmetry. exact same. }
        exact (isolated other_position other position source at_other
          at_source source_different other_exclusive node other_present
          copy_present).
Qed.

Lemma provenance_step_preserves_invariant : forall policy command world,
  (forall bit, provenance_clone_source_bit policy bit = false) ->
  provenance_invariant world ->
  provenance_invariant (provenance_step_with policy command world).
Proof.
  intros policy command [anchors allocation identity] clears
    [below isolated].
  change (provenance_allocations_below allocation anchors) in below.
  change (exclusive_isolation anchors) in isolated.
  destruct command as [width|position|position|position
      |position copied added|position released rebuilt_width];
    unfold provenance_step_with; cbv beta iota zeta.
  - exact (@provenance_fresh_prepend_preserves_invariant anchors allocation
      identity width below isolated).
  - exact (@provenance_shrinking_update_preserves_invariant allocation position
      provenance_wrapper_clone anchors below isolated
      (@provenance_wrapper_clone_keeps_exclusive)
      (@provenance_wrapper_clone_keeps_nodes)).
  - exact (@provenance_shrinking_update_preserves_invariant allocation position
      provenance_wrapper_drop anchors below isolated
      (@provenance_wrapper_drop_keeps_exclusive)
      (@provenance_wrapper_drop_keeps_nodes)).
  - destruct (nth_error anchors position) as [source|] eqn:at_source;
      cbv beta iota zeta.
    + destruct (provenance_wrappers source) as [|[|wrappers]];
        cbv beta iota zeta.
      * exact (conj below isolated).
      * exact (conj below isolated).
      * exact (@provenance_header_clone_preserves_invariant policy allocation
          identity position anchors source clears below isolated at_source).
    + exact (conj below isolated).
  - destruct (nth_error anchors position) as [target|] eqn:at_target;
      cbv beta iota zeta.
    + destruct (provenance_wrappers target) as [|[|wrappers]];
        cbv beta iota zeta.
      * exact (conj below isolated).
      * exact (@provenance_unique_write_preserves_invariant allocation position
          copied added anchors below isolated).
      * exact (conj below isolated).
    + exact (conj below isolated).
  - destruct (nth_error anchors position) as [subject|] eqn:at_subject;
      cbv beta iota zeta.
    + destruct (provenance_wrappers subject) as [|wrappers];
        cbv beta iota zeta.
      * exact (conj below isolated).
      * destruct (provenance_removal_gate policy anchors position subject);
          cbv beta iota zeta.
        -- exact (@provenance_shrinking_update_preserves_invariant allocation
             position (provenance_partial_release released) anchors below
             isolated (@provenance_partial_release_keeps_exclusive released)
             (@provenance_partial_release_keeps_nodes released)).
        -- destruct (@provenance_shrinking_update_preserves_invariant
               allocation position provenance_wrapper_drop anchors below
               isolated (@provenance_wrapper_drop_keeps_exclusive)
               (@provenance_wrapper_drop_keeps_nodes))
             as [dropped_below dropped_isolated].
           exact (@provenance_fresh_prepend_preserves_invariant
             (provenance_update position provenance_wrapper_drop anchors)
             allocation identity rebuilt_width dropped_below
             dropped_isolated).
    + exact (conj below isolated).
Qed.

(** The invariant holds for every policy whose header clone clears the source
    bit; the removal gate does not matter, because a release only removes
    nodes and a rebuild only adds fresh ones. *)
Theorem provenance_run_preserves_invariant : forall policy commands world,
  (forall bit, provenance_clone_source_bit policy bit = false) ->
  provenance_invariant world ->
  provenance_invariant (provenance_run_with policy commands world).
Proof.
  intros policy commands.
  induction commands as [|command rest IH];
    intros world clears invariant; simpl.
  - exact invariant.
  - apply IH.
    + exact clears.
    + exact (@provenance_step_preserves_invariant policy command world clears
        invariant).
Qed.

Lemma empty_provenance_world_satisfies_invariant :
  provenance_invariant empty_provenance_world.
Proof.
  split.
  - intros position anchor node at_position.
    destruct position; discriminate at_position.
  - intros position anchor other_position other at_position.
    destruct position; discriminate at_position.
Qed.

(** Main theorem: on every reachable world, an anchor that passes the
    production gate (bit set, one wrapper) is physically disjoint from the
    anchor at every other position. *)
Theorem exclusive_gate_implies_physical_isolation : forall commands position
    anchor,
  nth_error (provenance_anchors (provenance_run commands)) position =
    Some anchor ->
  exclusive_removal_gate anchor = true ->
  forall other_position other,
    nth_error (provenance_anchors (provenance_run commands)) other_position =
      Some other ->
    other_position <> position ->
    physically_disjoint (provenance_nodes anchor) (provenance_nodes other).
Proof.
  intros commands position anchor at_position gate other_position other
    at_other different.
  assert (clears : forall bit,
    provenance_clone_source_bit exclusive_provenance_policy bit = false).
  { intro bit. reflexivity. }
  destruct (@provenance_run_preserves_invariant exclusive_provenance_policy
      commands empty_provenance_world clears
      empty_provenance_world_satisfies_invariant) as [_ isolated].
  unfold exclusive_removal_gate in gate.
  apply andb_true_iff in gate as [exclusive _].
  exact (isolated position anchor other_position other at_position at_other
    different exclusive).
Qed.

Lemma provenance_gated_removal_step : forall anchors allocation identity
    position subject released rebuilt_width,
  nth_error anchors position = Some subject ->
  exclusive_removal_gate subject = true ->
  provenance_step (ProvenanceExhaustingRemoval position released rebuilt_width)
      (ProvenanceWorld anchors allocation identity) =
    ProvenanceWorld
      (provenance_update position (provenance_partial_release released)
        anchors)
      allocation identity.
Proof.
  intros anchors allocation identity position subject released rebuilt_width
    at_subject gate.
  assert (single : provenance_wrappers subject = 1).
  { unfold exclusive_removal_gate in gate.
    apply andb_true_iff in gate as [_ single].
    apply Nat.eqb_eq in single.
    exact single. }
  unfold provenance_step, provenance_step_with; cbv beta iota zeta.
  rewrite at_subject; cbv beta iota zeta.
  rewrite single; cbv beta iota zeta.
  destruct (provenance_removal_gate exclusive_provenance_policy anchors position
      subject) eqn:gate_value.
  - reflexivity.
  - change (exclusive_removal_gate subject = false) in gate_value.
    rewrite gate in gate_value.
    discriminate gate_value.
Qed.

Lemma provenance_released_part_is_owned : forall released anchor node,
  In node (anchor_nodes (provenance_released_part released anchor)) ->
  In node (provenance_nodes anchor).
Proof.
  intros released anchor node present.
  change (In node
    (filter (fun candidate => negb (provenance_absent_from released candidate))
      (provenance_nodes anchor))) in present.
  apply filter_In in present as [owned _].
  exact owned.
Qed.

Lemma provenance_subset_preserves_disjointness : forall part whole other,
  (forall node, In node part -> In node whole) ->
  physically_disjoint whole other ->
  physically_disjoint part other.
Proof.
  intros part whole other subset disjoint node in_part in_other.
  exact (disjoint node (subset node in_part) in_other).
Qed.

(** Corollary: an exclusive in-place removal keeps the anchor at its position
    with its identity, leaves the anchor at every other position unchanged,
    and frees no node of any other anchor's physical image. *)
Theorem exclusive_partial_release_preserves_external_images : forall commands
    position subject released rebuilt_width,
  nth_error (provenance_anchors (provenance_run commands)) position =
    Some subject ->
  exclusive_removal_gate subject = true ->
  nth_error
      (provenance_anchors
        (provenance_step
          (ProvenanceExhaustingRemoval position released rebuilt_width)
          (provenance_run commands)))
      position = Some (provenance_partial_release released subject) /\
  forall other_position other,
    nth_error (provenance_anchors (provenance_run commands)) other_position =
      Some other ->
    other_position <> position ->
    nth_error
        (provenance_anchors
          (provenance_step
            (ProvenanceExhaustingRemoval position released rebuilt_width)
            (provenance_run commands)))
        other_position = Some other /\
    physical_image_intact_after_release
      (provenance_released_part released subject) (provenance_physical other).
Proof.
  intros commands position subject released rebuilt_width at_subject gate.
  pose proof (@exclusive_gate_implies_physical_isolation commands position
    subject at_subject gate) as isolated.
  remember (provenance_run commands) as world eqn:run.
  destruct world as [anchors allocation identity].
  change (nth_error anchors position = Some subject) in at_subject.
  rewrite (@provenance_gated_removal_step anchors allocation identity position
    subject released rebuilt_width at_subject gate).
  split.
  - change (nth_error
      (provenance_update position (provenance_partial_release released)
        anchors) position =
      Some (provenance_partial_release released subject)).
    rewrite provenance_update_nth_here, at_subject.
    reflexivity.
  - intros other_position other at_other different.
    split.
    + change (nth_error
        (provenance_update position (provenance_partial_release released)
          anchors) other_position = Some other).
      rewrite (@provenance_update_nth_other position
        (provenance_partial_release released) anchors other_position
        different).
      exact at_other.
    + apply release_preserves_disjoint_anchor_images.
      exact (@provenance_subset_preserves_disjointness
        (anchor_nodes (provenance_released_part released subject))
        (provenance_nodes subject) (provenance_nodes other)
        (@provenance_released_part_is_owned released subject)
        (isolated other_position other at_other different)).
Qed.

Definition provenance_gate_at (world : provenance_world) (position : nat)
    : bool :=
  match nth_error (provenance_anchors world) position with
  | Some anchor => exclusive_removal_gate anchor
  | None => false
  end.

Definition provenance_nodes_at (world : provenance_world) (position : nat)
    : option (list node_id) :=
  option_map provenance_nodes (nth_error (provenance_anchors world) position).

Definition provenance_fresh_run : list provenance_command :=
  [ProvenanceConstruct 2].

Definition provenance_wrapper_clone_run : list provenance_command :=
  [ProvenanceConstruct 2; ProvenanceWrapperClone 0].

Definition provenance_wrapper_drop_run : list provenance_command :=
  [ProvenanceConstruct 2; ProvenanceWrapperClone 0; ProvenanceWrapperDrop 0].

Definition provenance_header_clone_run : list provenance_command :=
  [ProvenanceConstruct 2; ProvenanceWrapperClone 0; ProvenanceHeaderClone 0].

Definition provenance_rebuild_run : list provenance_command :=
  [ProvenanceConstruct 2; ProvenanceWrapperClone 0; ProvenanceHeaderClone 0;
   ProvenanceExhaustingRemoval 1 [(0, 1)] 1].

Definition provenance_in_place_run : list provenance_command :=
  [ProvenanceConstruct 2; ProvenanceExhaustingRemoval 0 [(0, 1)] 0].

(** Non-vacuity: a fresh anchor passes the gate; a live wrapper clone blocks
    it and its drop restores it (ruling R5); a header clone blocks both
    halves, which hold the same nodes; a failed gate rebuilds into a fresh
    anchor that passes while the surviving copy stays blocked (ruling R6); an
    exclusive removal stays in place with the same single anchor. *)
Theorem exclusive_provenance_gate_is_nonvacuous :
  provenance_gate_at (provenance_run provenance_fresh_run) 0 = true /\
  provenance_gate_at (provenance_run provenance_wrapper_clone_run) 0 = false /\
  provenance_gate_at (provenance_run provenance_wrapper_drop_run) 0 = true /\
  provenance_gate_at (provenance_run provenance_header_clone_run) 0 = false /\
  provenance_gate_at (provenance_run provenance_header_clone_run) 1 = false /\
  provenance_nodes_at (provenance_run provenance_header_clone_run) 0 =
    Some [(0, 0); (0, 1)] /\
  provenance_nodes_at (provenance_run provenance_header_clone_run) 1 =
    Some [(0, 0); (0, 1)] /\
  provenance_gate_at (provenance_run provenance_rebuild_run) 0 = true /\
  provenance_nodes_at (provenance_run provenance_rebuild_run) 0 =
    Some [(1, 0)] /\
  provenance_gate_at (provenance_run provenance_rebuild_run) 1 = false /\
  length (provenance_anchors (provenance_run provenance_in_place_run)) = 1 /\
  provenance_gate_at (provenance_run provenance_in_place_run) 0 = true /\
  provenance_nodes_at (provenance_run provenance_in_place_run) 0 =
    Some [(0, 0)].
Proof. vm_compute. repeat split; reflexivity. Qed.

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
| FocusedRawOwnerSiblingLossInvariant
| ExclusiveProvenanceIsolationInvariant.

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
   FocusedRawOwnerSiblingLossInvariant;
   ExclusiveProvenanceIsolationInvariant].

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
  | ExclusiveProvenanceIsolationInvariant =>
      forall commands position anchor,
        nth_error (provenance_anchors (provenance_run commands)) position =
          Some anchor ->
        exclusive_removal_gate anchor = true ->
        forall other_position other,
          nth_error (provenance_anchors (provenance_run commands))
            other_position = Some other ->
          other_position <> position ->
          physically_disjoint (provenance_nodes anchor) (provenance_nodes other)
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
  - exact @exclusive_gate_implies_physical_isolation.
Qed.

Theorem physical_ownership_invariant_catalog_is_unique_complete_and_exact :
  NoDup physical_ownership_invariant_catalog /\
  Forall physical_ownership_invariant_binding
    physical_ownership_invariant_catalog /\
  length physical_ownership_invariant_catalog = 12.
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
Print Assumptions provenance_run_preserves_invariant.
Print Assumptions exclusive_gate_implies_physical_isolation.
Print Assumptions exclusive_partial_release_preserves_external_images.
Print Assumptions exclusive_provenance_gate_is_nonvacuous.

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
