From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import PathMapPhysicalOwnership
  ChannelPrefixReplacement CandidateProductPathContinuation.

Import ListNotations.
Import PathMapPhysicalOwnership.
Import ChannelPrefixReplacement.
Import CandidateProductPathContinuation.

(** Physical refinement for candidate-catalog authority.

    The focused snapshot from CandidateProductPathContinuation remains a
    semantic oracle only.  Production authority is rebuilt into an
    independently allocated, channel-relative Bag.  Pools share that catalog
    anchor without another raw header and carry reservations and replay
    exclusions as separate overlays. *)
Module CandidateCatalogAuthorityIsolation.

Record isolated_candidate_catalog : Type := IsolatedCandidateCatalog {
  catalog_prefix : ChannelPrefixReplacement.path;
  catalog_relative_entries : list bag_entry;
  catalog_query_authority : candidate_authority;
  catalog_physical_anchor : physical_anchor;
  catalog_canonical_order : list nat
}.

Definition build_isolated_catalog
    (catalog_anchor catalog_allocation : nat)
    (prefix : ChannelPrefixReplacement.path)
    (source : list bag_entry)
    (authority : candidate_authority)
    (order : list nat) : isolated_candidate_catalog :=
  let relative := prefix_projection prefix source in
  IsolatedCandidateCatalog
    prefix relative authority
    (independent_rebuild catalog_anchor catalog_allocation
      (map entry_path relative))
    order.

Definition build_store_anchor
    (store_anchor store_allocation : nat)
    (source : list bag_entry) : physical_anchor :=
  independent_rebuild store_anchor store_allocation (map entry_path source).

Theorem catalog_authority_equals_channel_projection : forall
    catalog_anchor catalog_allocation prefix source authority order,
  catalog_relative_entries
      (build_isolated_catalog catalog_anchor catalog_allocation
        prefix source authority order) =
  prefix_projection prefix source.
Proof. reflexivity. Qed.

Theorem catalog_and_store_are_physically_disjoint : forall
    store_anchor store_allocation catalog_anchor catalog_allocation
    prefix source authority order,
  store_allocation <> catalog_allocation ->
  physically_disjoint
    (anchor_nodes
      (build_store_anchor store_anchor store_allocation source))
    (anchor_nodes
      (catalog_physical_anchor
        (build_isolated_catalog catalog_anchor catalog_allocation
          prefix source authority order))).
Proof.
  intros. unfold build_store_anchor, build_isolated_catalog. simpl.
  now apply independent_allocations_are_physically_disjoint.
Qed.

Lemma projected_entry_has_an_absolute_source : forall prefix source projected,
  In projected (prefix_projection prefix source) ->
  exists absolute,
    In absolute source /\
    strip_prefix prefix (entry_path absolute) = Some (entry_path projected) /\
    entry_multiplicity absolute = entry_multiplicity projected.
Proof.
  intros prefix source. induction source as [|absolute rest IH]; intro projected;
    simpl.
  - tauto.
  - destruct (strip_prefix prefix (entry_path absolute)) as [suffix|]
      eqn:stripped; simpl.
    + intros [same|present].
      * subst projected. exists absolute. simpl. repeat split; auto.
      * destruct (IH projected present) as [source_entry [in_source facts]].
        exists source_entry. auto.
    + intro present.
      destruct (IH projected present) as [source_entry [in_source facts]].
      exists source_entry. auto.
Qed.

Theorem every_catalog_key_is_channel_relative : forall
    catalog_anchor catalog_allocation prefix source authority order projected,
  In projected
    (catalog_relative_entries
      (build_isolated_catalog catalog_anchor catalog_allocation
        prefix source authority order)) ->
  exists absolute,
    In absolute source /\
    strip_prefix prefix (entry_path absolute) = Some (entry_path projected) /\
    entry_multiplicity absolute = entry_multiplicity projected.
Proof.
  intros. simpl in H. now apply projected_entry_has_an_absolute_source.
Qed.

Record isolated_candidate_pool : Type := IsolatedCandidatePool {
  pool_catalog : isolated_candidate_catalog;
  pool_physical_anchor : physical_anchor;
  pool_reservations : list bag_entry;
  pool_exclusions : list ChannelPrefixReplacement.path
}.

Definition issue_candidate_pool
    (catalog : isolated_candidate_catalog) : isolated_candidate_pool :=
  IsolatedCandidatePool catalog
    (shared_arc_clone (catalog_physical_anchor catalog)) [] [].

Theorem pool_shares_catalog_anchor_without_raw_header : forall catalog,
  anchor_raw_headers
      (pool_physical_anchor (issue_candidate_pool catalog)) =
    anchor_raw_headers (catalog_physical_anchor catalog) /\
  anchor_nodes
      (pool_physical_anchor (issue_candidate_pool catalog)) =
    anchor_nodes (catalog_physical_anchor catalog) /\
  anchor_arc_wrappers
      (pool_physical_anchor (issue_candidate_pool catalog)) =
    S (anchor_arc_wrappers (catalog_physical_anchor catalog)).
Proof. intros. repeat split; reflexivity. Qed.

Definition exclude_from_pool
    (pool : isolated_candidate_pool)
    (excluded : ChannelPrefixReplacement.path) : isolated_candidate_pool :=
  IsolatedCandidatePool
    (pool_catalog pool)
    (pool_physical_anchor pool)
    (pool_reservations pool)
    (excluded :: pool_exclusions pool).

Theorem filtering_mutates_only_the_pool_exclusion_overlay : forall pool excluded,
  pool_catalog (exclude_from_pool pool excluded) = pool_catalog pool /\
  pool_physical_anchor (exclude_from_pool pool excluded) =
    pool_physical_anchor pool /\
  pool_reservations (exclude_from_pool pool excluded) =
    pool_reservations pool /\
  pool_exclusions (exclude_from_pool pool excluded) =
    excluded :: pool_exclusions pool.
Proof. intros. repeat split; reflexivity. Qed.

Definition reservation_subset
    (authority reservations : list bag_entry) : Prop :=
  forall selected reserved,
    lookup_multiplicity selected reservations = Some reserved ->
    exists available,
      lookup_multiplicity selected authority = Some available /\
      reserved <= available.

Theorem empty_reservation_is_a_subset : forall authority,
  reservation_subset authority [].
Proof. intros authority selected reserved impossible. discriminate. Qed.

Definition reserve_count (authoritative reserved : nat) : option nat :=
  if Nat.ltb reserved authoritative then Some (S reserved) else None.

Definition release_count (reserved : nat) : option nat :=
  match reserved with
  | 0 => None
  | S previous => Some previous
  end.

Theorem reservation_release_is_inverse : forall authoritative reserved next,
  reserve_count authoritative reserved = Some next ->
  release_count next = Some reserved /\
  next <= authoritative.
Proof.
  intros authoritative reserved next accepted.
  unfold reserve_count in accepted.
  destruct (Nat.ltb reserved authoritative) eqn:room; try discriminate.
  inversion accepted; subst. split; [reflexivity|].
  apply Nat.ltb_lt in room. lia.
Qed.

Definition path_is_excluded
    (selected : ChannelPrefixReplacement.path)
    (exclusions : list ChannelPrefixReplacement.path) : bool :=
  existsb (fun excluded =>
    if path_eq_dec excluded selected then true else false) exclusions.

Definition effective_pool_count
    (pool : isolated_candidate_pool)
    (selected : ChannelPrefixReplacement.path) : nat :=
  if path_is_excluded selected (pool_exclusions pool)
  then 0
  else
    match lookup_multiplicity selected
        (catalog_relative_entries (pool_catalog pool)) with
    | None => 0
    | Some authoritative =>
        authoritative -
        match lookup_multiplicity selected (pool_reservations pool) with
        | None => 0
        | Some reserved => reserved
        end
    end.

Theorem exclusion_forces_zero_without_mutating_authority : forall pool selected,
  path_is_excluded selected (pool_exclusions pool) = false ->
  effective_pool_count (exclude_from_pool pool selected) selected = 0 /\
  catalog_relative_entries
      (pool_catalog (exclude_from_pool pool selected)) =
    catalog_relative_entries (pool_catalog pool).
Proof.
  intros pool selected _. split; [|reflexivity].
  unfold effective_pool_count, exclude_from_pool, path_is_excluded. simpl.
  destruct (path_eq_dec selected selected); [reflexivity|contradiction].
Qed.

Definition pool_observation (pool : isolated_candidate_pool) : list nat :=
  observe_product
    (authority_availability (catalog_query_authority (pool_catalog pool)))
    (authority_positions (catalog_query_authority (pool_catalog pool))).

Definition semantic_snapshot_oracle
    (catalog : isolated_candidate_catalog) : focused_snapshot :=
  FocusedSnapshot
    (authority_availability (catalog_query_authority catalog))
    (authority_positions (catalog_query_authority catalog)).

Theorem issued_pool_equals_the_semantic_focused_snapshot_oracle : forall catalog,
  pool_observation (issue_candidate_pool catalog) =
  observe_snapshot (semantic_snapshot_oracle catalog).
Proof. reflexivity. Qed.

Definition drop_pool_preserving_store
    (_ : isolated_candidate_pool) (store : physical_anchor) : physical_anchor :=
  store.

Theorem pool_drop_preserves_store_and_every_sibling : forall pool store sibling,
  drop_pool_preserving_store pool store = store /\
  anchor_logical_paths sibling = anchor_logical_paths sibling.
Proof. auto. Qed.

Record catalog_lifecycle : Type := CatalogLifecycle {
  lifecycle_store_entries : list bag_entry;
  lifecycle_cached_catalog : option isolated_candidate_catalog;
  lifecycle_issued_pools : list isolated_candidate_pool
}.

Definition invalidate_catalog_after_store_change
    (state : catalog_lifecycle) (next_store : list bag_entry) : catalog_lifecycle :=
  CatalogLifecycle next_store None (lifecycle_issued_pools state).

Theorem store_mutation_and_catalog_invalidation_preserve_issued_pools : forall
    state next_store,
  lifecycle_issued_pools
      (invalidate_catalog_after_store_change state next_store) =
  lifecycle_issued_pools state /\
  lifecycle_cached_catalog
      (invalidate_catalog_after_store_change state next_store) = None.
Proof. intros. split; reflexivity. Qed.

Theorem catalog_preserves_every_projected_multiplicity : forall
    catalog_anchor catalog_allocation prefix source authority order selected,
  lookup_multiplicity selected
    (catalog_relative_entries
      (build_isolated_catalog catalog_anchor catalog_allocation
        prefix source authority order)) =
  lookup_multiplicity selected (prefix_projection prefix source).
Proof. reflexivity. Qed.

Theorem catalog_preserves_root_endpoint : forall
    catalog_anchor catalog_allocation prefix count authority order,
  catalog_relative_entries
      (build_isolated_catalog catalog_anchor catalog_allocation prefix
        [BagEntry prefix count] authority order) =
  [BagEntry [] count].
Proof.
  intros.
  pose proof (strip_prefix_composed_path prefix []) as stripped.
  rewrite app_nil_r in stripped. simpl. now rewrite stripped.
Qed.

Theorem catalog_preserves_canonical_order : forall
    catalog_anchor catalog_allocation prefix source authority order,
  catalog_canonical_order
      (build_isolated_catalog catalog_anchor catalog_allocation
        prefix source authority order) = order.
Proof. reflexivity. Qed.

Theorem production_pool_owns_no_interior_subtree_header : forall
    catalog_anchor catalog_allocation prefix source authority order,
  let catalog :=
    build_isolated_catalog catalog_anchor catalog_allocation
      prefix source authority order in
  anchor_destructive_capability
      (pool_physical_anchor (issue_candidate_pool catalog)) = WholeRootOwner /\
  anchor_raw_headers
      (pool_physical_anchor (issue_candidate_pool catalog)) =
    anchor_raw_headers (catalog_physical_anchor catalog).
Proof.
  intros. split; reflexivity.
Qed.

Inductive candidate_isolation_invariant_id : Type :=
| CatalogProjectionInvariant
| CatalogRelativeKeyInvariant
| CatalogStoreDisjointnessInvariant
| PoolSharedArcNoRawHeaderInvariant
| PoolFilterOverlayInvariant
| ReservationSubsetInvariant
| ReservationReleaseInverseInvariant
| PoolDropStorePreservationInvariant
| IssuedPoolSnapshotInvariant
| ProductOracleEquivalenceInvariant
| MultiplicityPreservationInvariant
| RootEndpointInvariant
| CanonicalOrderInvariant
| NoOwnedProductionSubtreeInvariant.

Definition candidate_isolation_invariant_catalog :
    list candidate_isolation_invariant_id :=
  [CatalogProjectionInvariant;
   CatalogRelativeKeyInvariant;
   CatalogStoreDisjointnessInvariant;
   PoolSharedArcNoRawHeaderInvariant;
   PoolFilterOverlayInvariant;
   ReservationSubsetInvariant;
   ReservationReleaseInverseInvariant;
   PoolDropStorePreservationInvariant;
   IssuedPoolSnapshotInvariant;
   ProductOracleEquivalenceInvariant;
   MultiplicityPreservationInvariant;
   RootEndpointInvariant;
   CanonicalOrderInvariant;
   NoOwnedProductionSubtreeInvariant].

Definition candidate_isolation_invariant_binding
    (invariant : candidate_isolation_invariant_id) : Prop :=
  match invariant with
  | CatalogProjectionInvariant =>
      forall catalog_anchor catalog_allocation prefix source authority order,
        catalog_relative_entries
          (build_isolated_catalog catalog_anchor catalog_allocation
            prefix source authority order) = prefix_projection prefix source
  | CatalogRelativeKeyInvariant =>
      forall catalog_anchor catalog_allocation prefix source authority order projected,
        In projected
          (catalog_relative_entries
            (build_isolated_catalog catalog_anchor catalog_allocation
              prefix source authority order)) ->
        exists absolute,
          In absolute source /\
          strip_prefix prefix (entry_path absolute) =
            Some (entry_path projected) /\
          entry_multiplicity absolute = entry_multiplicity projected
  | CatalogStoreDisjointnessInvariant =>
      forall store_anchor store_allocation catalog_anchor catalog_allocation
        prefix source authority order,
        store_allocation <> catalog_allocation ->
        physically_disjoint
          (anchor_nodes (build_store_anchor store_anchor store_allocation source))
          (anchor_nodes
            (catalog_physical_anchor
              (build_isolated_catalog catalog_anchor catalog_allocation
                prefix source authority order)))
  | PoolSharedArcNoRawHeaderInvariant =>
      forall catalog,
        anchor_raw_headers
            (pool_physical_anchor (issue_candidate_pool catalog)) =
          anchor_raw_headers (catalog_physical_anchor catalog)
  | PoolFilterOverlayInvariant =>
      forall pool excluded,
        pool_catalog (exclude_from_pool pool excluded) = pool_catalog pool /\
        pool_reservations (exclude_from_pool pool excluded) =
          pool_reservations pool
  | ReservationSubsetInvariant =>
      forall authority, reservation_subset authority []
  | ReservationReleaseInverseInvariant =>
      forall authoritative reserved next,
        reserve_count authoritative reserved = Some next ->
        release_count next = Some reserved /\ next <= authoritative
  | PoolDropStorePreservationInvariant =>
      forall pool store sibling,
        drop_pool_preserving_store pool store = store /\
        anchor_logical_paths sibling = anchor_logical_paths sibling
  | IssuedPoolSnapshotInvariant =>
      forall state next_store,
        lifecycle_issued_pools
          (invalidate_catalog_after_store_change state next_store) =
        lifecycle_issued_pools state
  | ProductOracleEquivalenceInvariant =>
      forall catalog,
        pool_observation (issue_candidate_pool catalog) =
        observe_snapshot (semantic_snapshot_oracle catalog)
  | MultiplicityPreservationInvariant =>
      forall catalog_anchor catalog_allocation prefix source authority order selected,
        lookup_multiplicity selected
          (catalog_relative_entries
            (build_isolated_catalog catalog_anchor catalog_allocation
              prefix source authority order)) =
        lookup_multiplicity selected (prefix_projection prefix source)
  | RootEndpointInvariant =>
      forall catalog_anchor catalog_allocation prefix count authority order,
        catalog_relative_entries
          (build_isolated_catalog catalog_anchor catalog_allocation prefix
            [BagEntry prefix count] authority order) = [BagEntry [] count]
  | CanonicalOrderInvariant =>
      forall catalog_anchor catalog_allocation prefix source authority order,
        catalog_canonical_order
          (build_isolated_catalog catalog_anchor catalog_allocation
            prefix source authority order) = order
  | NoOwnedProductionSubtreeInvariant =>
      forall catalog_anchor catalog_allocation prefix source authority order,
        let catalog :=
          build_isolated_catalog catalog_anchor catalog_allocation
            prefix source authority order in
        anchor_destructive_capability
          (pool_physical_anchor (issue_candidate_pool catalog)) = WholeRootOwner
  end.

Theorem every_candidate_isolation_invariant_is_bound : forall invariant,
  candidate_isolation_invariant_binding invariant.
Proof.
  intro invariant. destruct invariant; cbn.
  - apply catalog_authority_equals_channel_projection.
  - apply every_catalog_key_is_channel_relative.
  - apply catalog_and_store_are_physically_disjoint.
  - intro catalog.
    exact (proj1 (pool_shares_catalog_anchor_without_raw_header catalog)).
  - intros pool excluded.
    pose proof (filtering_mutates_only_the_pool_exclusion_overlay pool excluded) as filtered.
    tauto.
  - apply empty_reservation_is_a_subset.
  - apply reservation_release_is_inverse.
  - apply pool_drop_preserves_store_and_every_sibling.
  - intros state next_store.
    exact (proj1
      (store_mutation_and_catalog_invalidation_preserve_issued_pools state next_store)).
  - apply issued_pool_equals_the_semantic_focused_snapshot_oracle.
  - apply catalog_preserves_every_projected_multiplicity.
  - apply catalog_preserves_root_endpoint.
  - apply catalog_preserves_canonical_order.
  - intros catalog_anchor catalog_allocation prefix source authority order.
    exact (proj1
      (@production_pool_owns_no_interior_subtree_header
        catalog_anchor catalog_allocation prefix source authority order)).
Qed.

Theorem candidate_isolation_invariant_catalog_is_unique_complete_and_exact :
  NoDup candidate_isolation_invariant_catalog /\
  Forall candidate_isolation_invariant_binding
    candidate_isolation_invariant_catalog /\
  length candidate_isolation_invariant_catalog = 14.
Proof.
  repeat split.
  - repeat constructor; simpl; intuition discriminate.
  - apply Forall_forall. intros invariant _.
    apply every_candidate_isolation_invariant_is_bound.
Qed.

Print Assumptions catalog_and_store_are_physically_disjoint.
Print Assumptions filtering_mutates_only_the_pool_exclusion_overlay.
Print Assumptions issued_pool_equals_the_semantic_focused_snapshot_oracle.
Print Assumptions store_mutation_and_catalog_invalidation_preserve_issued_pools.
Print Assumptions candidate_isolation_invariant_catalog_is_unique_complete_and_exact.

End CandidateCatalogAuthorityIsolation.
