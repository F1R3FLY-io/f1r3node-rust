From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import PathMapPhysicalOwnership
  ChannelPrefixReplacement CandidateProductPathContinuation
  CandidateCatalogAuthorityIsolation.

Import ListNotations.
Import PathMapPhysicalOwnership.
Import ChannelPrefixReplacement.
Import CandidateProductPathContinuation.
Import CandidateCatalogAuthorityIsolation.

(** Causal negative controls for independently owned candidate authority. *)
Module CandidateCatalogAuthorityIsolationMutants.

Definition candidate_prefix : ChannelPrefixReplacement.path := [7].
Definition candidate_source : list bag_entry :=
  [BagEntry [7] 5; BagEntry [7; 1] 2; BagEntry [7; 2] 3; BagEntry [9] 11].

Definition availability_fixture : trie unit :=
  Node None [(1, Node (Some tt) []); (2, Node (Some tt) [])].

Definition positions_fixture : trie nat :=
  Node None [(1, Node (Some 0) []); (2, Node (Some 1) [])].

Definition authority_fixture : candidate_authority :=
  CandidateAuthority 41 availability_fixture positions_fixture.

Definition isolated_catalog_fixture : isolated_candidate_catalog :=
  build_isolated_catalog 2 1 candidate_prefix candidate_source
    authority_fixture [0; 1].

Definition isolated_pool_fixture : isolated_candidate_pool :=
  issue_candidate_pool isolated_catalog_fixture.

Definition mutant_owned_focused_authority : physical_anchor :=
  interior_focus_fixture.

Theorem kill_owned_focused_make_map_authority :
  ~ physical_image_intact_after_release
      mutant_owned_focused_authority shared_prefix_sibling_fixture.
Proof. apply focused_raw_owner_release_can_destroy_shared_prefix_sibling. Qed.

Definition mutant_catalog_leaks_sibling : list bag_entry := candidate_source.

Theorem kill_shared_prefix_sibling_leak :
  mutant_catalog_leaks_sibling <>
  catalog_relative_entries isolated_catalog_fixture.
Proof. vm_compute. discriminate. Qed.

Definition mutant_pool_drop_survivors : list node_id :=
  release_survivors interior_focus_fixture shared_prefix_sibling_fixture.

Theorem kill_shared_node_destructive_drop :
  mutant_pool_drop_survivors <>
  anchor_nodes shared_prefix_sibling_fixture.
Proof. vm_compute. discriminate. Qed.

Definition mutant_filter_mutates_catalog
    (pool : isolated_candidate_pool) : isolated_candidate_pool :=
  let catalog := pool_catalog pool in
  let entries := catalog_relative_entries catalog in
  let mutated_entries := match entries with [] => [] | _ :: rest => rest end in
  let mutated_catalog :=
    IsolatedCandidateCatalog
      (catalog_prefix catalog)
      mutated_entries
      (catalog_query_authority catalog)
      (catalog_physical_anchor catalog)
      (catalog_canonical_order catalog) in
  IsolatedCandidatePool mutated_catalog (pool_physical_anchor pool)
    (pool_reservations pool) (pool_exclusions pool).

Theorem kill_pool_filter_catalog_mutation :
  catalog_relative_entries
      (pool_catalog (mutant_filter_mutates_catalog isolated_pool_fixture)) <>
  catalog_relative_entries
      (pool_catalog (exclude_from_pool isolated_pool_fixture [1])).
Proof. vm_compute. discriminate. Qed.

Definition mutant_reservation_mutates_authority
    (pool : isolated_candidate_pool) : isolated_candidate_pool :=
  mutant_filter_mutates_catalog pool.

Theorem kill_reservation_authority_mutation :
  catalog_relative_entries
      (pool_catalog (mutant_reservation_mutates_authority isolated_pool_fixture)) <>
  catalog_relative_entries (pool_catalog isolated_pool_fixture).
Proof. vm_compute. discriminate. Qed.

Definition changed_source : list bag_entry :=
  [BagEntry [7] 5; BagEntry [7; 2] 3].

Definition mutant_store_change_reissues_pool : isolated_candidate_pool :=
  issue_candidate_pool
    (build_isolated_catalog 3 2 candidate_prefix changed_source
      authority_fixture [1]).

Theorem kill_store_mutation_that_changes_issued_pool :
  catalog_relative_entries (pool_catalog mutant_store_change_reissues_pool) <>
  catalog_relative_entries (pool_catalog isolated_pool_fixture).
Proof. vm_compute. discriminate. Qed.

Definition mutant_flattened_multiplicity : list bag_entry :=
  [BagEntry [] 1; BagEntry [1] 1; BagEntry [2] 1].

Theorem kill_catalog_multiplicity_flattening :
  mutant_flattened_multiplicity <>
  catalog_relative_entries isolated_catalog_fixture.
Proof. vm_compute. discriminate. Qed.

Definition mutant_absolute_catalog_keys : list bag_entry :=
  [BagEntry [7] 5; BagEntry [7; 1] 2; BagEntry [7; 2] 3].

Theorem kill_absolute_instead_of_relative_catalog_keys :
  mutant_absolute_catalog_keys <>
  catalog_relative_entries isolated_catalog_fixture.
Proof. vm_compute. discriminate. Qed.

Definition mutant_skip_root_endpoint : list bag_entry :=
  [BagEntry [1] 2; BagEntry [2] 3].

Theorem kill_catalog_root_endpoint_omission :
  mutant_skip_root_endpoint <>
  catalog_relative_entries isolated_catalog_fixture.
Proof. vm_compute. discriminate. Qed.

Definition mutant_reversed_catalog_order : list nat := [1; 0].

Theorem kill_catalog_order_mutation :
  mutant_reversed_catalog_order <>
  catalog_canonical_order isolated_catalog_fixture.
Proof. vm_compute. discriminate. Qed.

Inductive candidate_isolation_mutant_id : Type :=
| OwnedFocusedMakeMapMutant
| SharedPrefixSiblingLeakMutant
| SharedNodeDestructiveDropMutant
| PoolFilterMutatesCatalogMutant
| PoolReservationMutatesAuthorityMutant
| StoreMutationChangesIssuedPoolMutant
| FlattenMultiplicityMutant
| AbsoluteInsteadOfRelativeKeyMutant
| SkipRootEndpointMutant
| CatalogOrderMutationMutant.

Definition candidate_isolation_mutant_catalog :
    list candidate_isolation_mutant_id :=
  [OwnedFocusedMakeMapMutant;
   SharedPrefixSiblingLeakMutant;
   SharedNodeDestructiveDropMutant;
   PoolFilterMutatesCatalogMutant;
   PoolReservationMutatesAuthorityMutant;
   StoreMutationChangesIssuedPoolMutant;
   FlattenMultiplicityMutant;
   AbsoluteInsteadOfRelativeKeyMutant;
   SkipRootEndpointMutant;
   CatalogOrderMutationMutant].

Definition candidate_isolation_mutant_killed
    (mutant : candidate_isolation_mutant_id) : Prop :=
  match mutant with
  | OwnedFocusedMakeMapMutant =>
      ~ physical_image_intact_after_release
          mutant_owned_focused_authority shared_prefix_sibling_fixture
  | SharedPrefixSiblingLeakMutant =>
      mutant_catalog_leaks_sibling <>
      catalog_relative_entries isolated_catalog_fixture
  | SharedNodeDestructiveDropMutant =>
      mutant_pool_drop_survivors <> anchor_nodes shared_prefix_sibling_fixture
  | PoolFilterMutatesCatalogMutant =>
      catalog_relative_entries
          (pool_catalog (mutant_filter_mutates_catalog isolated_pool_fixture)) <>
      catalog_relative_entries
          (pool_catalog (exclude_from_pool isolated_pool_fixture [1]))
  | PoolReservationMutatesAuthorityMutant =>
      catalog_relative_entries
          (pool_catalog
            (mutant_reservation_mutates_authority isolated_pool_fixture)) <>
      catalog_relative_entries (pool_catalog isolated_pool_fixture)
  | StoreMutationChangesIssuedPoolMutant =>
      catalog_relative_entries (pool_catalog mutant_store_change_reissues_pool) <>
      catalog_relative_entries (pool_catalog isolated_pool_fixture)
  | FlattenMultiplicityMutant =>
      mutant_flattened_multiplicity <>
      catalog_relative_entries isolated_catalog_fixture
  | AbsoluteInsteadOfRelativeKeyMutant =>
      mutant_absolute_catalog_keys <>
      catalog_relative_entries isolated_catalog_fixture
  | SkipRootEndpointMutant =>
      mutant_skip_root_endpoint <>
      catalog_relative_entries isolated_catalog_fixture
  | CatalogOrderMutationMutant =>
      mutant_reversed_catalog_order <>
      catalog_canonical_order isolated_catalog_fixture
  end.

Theorem every_candidate_isolation_mutant_has_a_causal_counterexample :
  forall mutant, candidate_isolation_mutant_killed mutant.
Proof.
  intro mutant. destruct mutant; cbn.
  - apply kill_owned_focused_make_map_authority.
  - apply kill_shared_prefix_sibling_leak.
  - apply kill_shared_node_destructive_drop.
  - apply kill_pool_filter_catalog_mutation.
  - apply kill_reservation_authority_mutation.
  - apply kill_store_mutation_that_changes_issued_pool.
  - apply kill_catalog_multiplicity_flattening.
  - apply kill_absolute_instead_of_relative_catalog_keys.
  - apply kill_catalog_root_endpoint_omission.
  - apply kill_catalog_order_mutation.
Qed.

Theorem candidate_isolation_mutant_catalog_is_unique_complete_and_exact :
  NoDup candidate_isolation_mutant_catalog /\
  Forall candidate_isolation_mutant_killed candidate_isolation_mutant_catalog /\
  length candidate_isolation_mutant_catalog = 10.
Proof.
  repeat split.
  - repeat constructor; simpl; intuition discriminate.
  - repeat constructor;
      apply every_candidate_isolation_mutant_has_a_causal_counterexample.
Qed.

Print Assumptions every_candidate_isolation_mutant_has_a_causal_counterexample.
Print Assumptions candidate_isolation_mutant_catalog_is_unique_complete_and_exact.

End CandidateCatalogAuthorityIsolationMutants.
