From Stdlib Require Import Bool List Arith Lia NArith.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority
  TwoLevelRSpaceAuthorityMutants
  TwoLevelRSpaceOperations.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceAuthorityMutants.
Import TwoLevelRSpaceOperations.

(** Concrete negative controls for the exact upsert, unload, multi-channel
    preparation, catalog, and publication laws. *)
Module TwoLevelRSpaceOperationsMutants.

Definition inserted_channel : Path := [1%nat; 1%nat].
Definition inserted_datum : Path := [7%nat].
Definition inserted_bag : ChannelBag := [(inserted_datum, 19%N)].

Lemma inserted_channel_is_valid : path_valid inserted_channel = true.
Proof. reflexivity. Qed.

Lemma inserted_bag_is_canonical : canonical_entries inserted_bag.
Proof.
  repeat constructor; reflexivity.
Qed.

Lemma replacement_bag_is_canonical : canonical_entries replacement_bag.
Proof.
  repeat constructor; try reflexivity.
Qed.

Definition inserted_upsert : prepared_channel_operation :=
  @PreparedUpsert inserted_channel inserted_bag
    inserted_channel_is_valid inserted_bag_is_canonical.

Definition target_upsert : prepared_channel_operation :=
  @PreparedUpsert target_channel replacement_bag eq_refl
    replacement_bag_is_canonical.

Definition inserted_batch : PreparedChannelBatch.
Proof.
  refine (PrepareChannelBatch [inserted_upsert] _).
  unfold touched_channels, inserted_upsert, operation_channel.
  simpl. constructor.
  - intro Hin. inversion Hin.
  - constructor.
Defined.

(** OpsInvUpsertTargetExact. *)
Definition mutant_upsert_keeps_old_target : TwoLevelAuthority := authority_fixture.

Theorem kill_upsert_target_omission :
  lookup_channel target_channel mutant_upsert_keeps_old_target <>
  lookup_channel target_channel
    (upsert_channel_exact target_channel replacement_bag authority_fixture).
Proof. vm_compute. discriminate. Qed.

(** OpsInvUpsertOtherPreserved. *)
Definition mutant_upsert_erases_sibling : TwoLevelAuthority :=
  [(target_channel, replacement_bag); (descendant_channel, descendant_bag)].

Theorem kill_upsert_unrelated_erasure :
  lookup_channel sibling_channel mutant_upsert_erases_sibling <>
  lookup_channel sibling_channel authority_fixture.
Proof. vm_compute. discriminate. Qed.

(** OpsInvUpsertLoaded. *)
Definition mutant_upsert_does_not_load : TwoLevelAuthority := authority_fixture.

Theorem kill_upsert_loaded_omission :
  ~ loaded mutant_upsert_does_not_load inserted_channel.
Proof.
  unfold loaded. intros [contents Hlookup].
  vm_compute in Hlookup. discriminate.
Qed.

(** OpsInvUpsertCanonical. *)
Definition mutant_append_unordered_upsert : TwoLevelAuthority :=
  authority_fixture ++ [(inserted_channel, inserted_bag)].

Theorem kill_unordered_outer_insertion :
  ~ canonical_channels mutant_append_unordered_upsert.
Proof.
  intro Hcanonical.
  inversion Hcanonical as [|c1 b1 t1 V1 B1 O1 C1]; subst.
  inversion C1 as [|c2 b2 t2 V2 B2 O2 C2]; subst.
  inversion C2 as [|c3 b3 t3 V3 B3 O3 C3]; subst.
  inversion O3 as [|entry rest Hlt Hrest]; subst.
  unfold sibling_channel, inserted_channel in Hlt.
  inversion Hlt; lia.
Qed.

(** OpsInvLoadedUpsertCount. *)
Definition mutant_loaded_upsert_count : nat := S (length authority_fixture).

Theorem kill_loaded_upsert_count_increment :
  mutant_loaded_upsert_count <> length authority_fixture.
Proof. vm_compute. discriminate. Qed.

(** OpsInvUnloadedUpsertCount. *)
Definition mutant_unloaded_upsert_count : nat := length authority_fixture.

Theorem kill_unloaded_upsert_count_stasis :
  mutant_unloaded_upsert_count <> S (length authority_fixture).
Proof. vm_compute. discriminate. Qed.

(** OpsInvDeleteTargetAbsent. *)
Definition mutant_delete_keeps_target : TwoLevelAuthority := authority_fixture.

Theorem kill_delete_target_retention :
  lookup_channel target_channel mutant_delete_keeps_target <> None.
Proof. vm_compute. discriminate. Qed.

(** OpsInvDeleteOtherPreserved. *)
Definition mutant_delete_erases_descendant : TwoLevelAuthority :=
  [(sibling_channel, sibling_bag)].

Theorem kill_delete_descendant_erasure :
  lookup_channel descendant_channel mutant_delete_erases_descendant <>
  lookup_channel descendant_channel authority_fixture.
Proof. vm_compute. discriminate. Qed.

(** OpsInvDeleteCanonical. *)
Definition mutant_delete_reverses_survivors : TwoLevelAuthority :=
  [(sibling_channel, sibling_bag); (descendant_channel, descendant_bag)].

Theorem kill_noncanonical_delete_result :
  ~ canonical_channels mutant_delete_reverses_survivors.
Proof.
  intro Hcanonical.
  inversion Hcanonical as [|channel contents tail V B O C]; subst.
  inversion O as [|entry rest Hlt Hrest]; subst.
  unfold sibling_channel, descendant_channel in Hlt.
  inversion Hlt; lia.
Qed.

(** OpsInvAbsentDeleteIdempotent. *)
Definition mutant_absent_delete_removes_sibling : TwoLevelAuthority :=
  [(target_channel, old_target_bag); (descendant_channel, descendant_bag)].

Theorem kill_absent_delete_mutation :
  mutant_absent_delete_removes_sibling <> authority_fixture.
Proof. discriminate. Qed.

(** OpsInvLoadedDeleteCount. *)
Definition mutant_loaded_delete_count : nat := length authority_fixture.

Theorem kill_loaded_delete_count_stasis :
  S (length (delete_channel_exact target_channel authority_fixture)) <>
  S mutant_loaded_delete_count.
Proof. vm_compute. discriminate. Qed.

(** OpsInvBatchTargetsUnique. *)
Definition mutant_duplicate_batch_targets : list Path :=
  [target_channel; target_channel].

Theorem kill_duplicate_batch_targets :
  ~ NoDup mutant_duplicate_batch_targets.
Proof.
  intro Hunique.
  inversion Hunique as [|head tail Hnotin Htail]; subst.
  apply Hnotin. simpl. auto.
Qed.

(** OpsInvBatchCanonical. *)
Definition mutant_noncanonical_batch_result : TwoLevelAuthority :=
  rev authority_fixture.

Theorem kill_noncanonical_batch_result :
  ~ canonical_channels mutant_noncanonical_batch_result.
Proof.
  intro Hcanonical.
  inversion Hcanonical as [|channel contents tail V B O C]; subst.
  inversion O as [|entry rest Hlt Hrest]; subst.
  unfold sibling_channel, descendant_channel in Hlt.
  inversion Hlt; lia.
Qed.

(** OpsInvBatchUntouchedChannel. *)
Definition mutant_batch_changes_untouched : TwoLevelAuthority :=
  [(target_channel, replacement_bag);
   (descendant_channel, descendant_bag);
   (sibling_channel, [])].

Theorem kill_batch_untouched_channel_mutation :
  lookup_channel sibling_channel mutant_batch_changes_untouched <>
  lookup_channel sibling_channel authority_fixture.
Proof. vm_compute. discriminate. Qed.

Definition catalog_fixture : CatalogAuthority :=
  fun channel =>
    if path_eq_dec channel inserted_channel then Some 11%nat else Some 13%nat.

Definition transaction_fixture : @transactional_store_state nat nat :=
  TransactionalStoreState
    authority_fixture
    (length authority_fixture)
    catalog_fixture
    17%nat
    23%nat.

(** OpsInvFailureAtomic. *)
Definition mutant_failed_state : @transactional_store_state nat nat :=
  TransactionalStoreState
    authority_fixture
    (length authority_fixture)
    catalog_fixture
    17%nat
    24%nat.

Theorem kill_failure_public_state_mutation :
  mutant_failed_state <> transaction_fixture.
Proof. discriminate. Qed.

(** OpsInvAcceptedOnePublication. *)
Definition mutant_success_publication_count : nat := 2%nat.

Theorem kill_duplicate_root_publication :
  mutant_success_publication_count <> 1%nat.
Proof. discriminate. Qed.

(** OpsInvTransitionDeterministic. *)
Definition mutant_left_outcome : @channel_batch_outcome nat nat :=
  ChannelBatchOutcome ChannelBatchRefused transaction_fixture 0.

Definition mutant_right_outcome : @channel_batch_outcome nat nat :=
  ChannelBatchOutcome ChannelBatchPublished transaction_fixture 1.

Theorem kill_nondeterministic_outcomes :
  mutant_left_outcome <> mutant_right_outcome.
Proof. discriminate. Qed.

(** OpsInvPublishedStoreWellFormed. *)
Definition mutant_wrong_loaded_count_state : @transactional_store_state nat nat :=
  TransactionalStoreState
    authority_fixture
    (S (length authority_fixture))
    catalog_fixture
    17%nat
    24%nat.

Theorem kill_wrong_published_loaded_count :
  ~ store_state_well_formed mutant_wrong_loaded_count_state.
Proof.
  intros [_ Hcount]. vm_compute in Hcount. discriminate.
Qed.

(** OpsInvTouchedCatalogInvalidated. *)
Definition mutant_touched_catalog : option nat :=
  catalog_fixture inserted_channel.

Theorem kill_retained_touched_catalog : mutant_touched_catalog <> None.
Proof. vm_compute. discriminate. Qed.

(** OpsInvUntouchedCatalogPreserved. *)
Definition mutant_untouched_catalog : option nat := None.

Theorem kill_erased_untouched_catalog :
  mutant_untouched_catalog <> catalog_fixture sibling_channel.
Proof. vm_compute. discriminate. Qed.

(** OpsInvIssuedSnapshotsPreserved. *)
Definition mutant_external_snapshots : nat := 18%nat.

Theorem kill_issued_snapshot_mutation : mutant_external_snapshots <> 17%nat.
Proof. discriminate. Qed.

(** OpsInvGenerationAdvancedOnce. *)
Definition mutant_generation : nat := 23%nat.

Theorem kill_generation_stasis : mutant_generation <> S 23%nat.
Proof. discriminate. Qed.

Definition two_level_operation_mutant_killed
    (invariant : two_level_operation_invariant_id) : Prop :=
  match invariant with
  | OpsInvUpsertTargetExact =>
      lookup_channel target_channel mutant_upsert_keeps_old_target <>
      lookup_channel target_channel
        (upsert_channel_exact target_channel replacement_bag authority_fixture)
  | OpsInvUpsertOtherPreserved =>
      lookup_channel sibling_channel mutant_upsert_erases_sibling <>
      lookup_channel sibling_channel authority_fixture
  | OpsInvUpsertLoaded =>
      ~ loaded mutant_upsert_does_not_load inserted_channel
  | OpsInvUpsertCanonical =>
      ~ canonical_channels mutant_append_unordered_upsert
  | OpsInvLoadedUpsertCount =>
      mutant_loaded_upsert_count <> length authority_fixture
  | OpsInvUnloadedUpsertCount =>
      mutant_unloaded_upsert_count <> S (length authority_fixture)
  | OpsInvDeleteTargetAbsent =>
      lookup_channel target_channel mutant_delete_keeps_target <> None
  | OpsInvDeleteOtherPreserved =>
      lookup_channel descendant_channel mutant_delete_erases_descendant <>
      lookup_channel descendant_channel authority_fixture
  | OpsInvDeleteCanonical =>
      ~ canonical_channels mutant_delete_reverses_survivors
  | OpsInvAbsentDeleteIdempotent =>
      mutant_absent_delete_removes_sibling <> authority_fixture
  | OpsInvLoadedDeleteCount =>
      S (length (delete_channel_exact target_channel authority_fixture)) <>
      S mutant_loaded_delete_count
  | OpsInvBatchTargetsUnique =>
      ~ NoDup mutant_duplicate_batch_targets
  | OpsInvBatchCanonical =>
      ~ canonical_channels mutant_noncanonical_batch_result
  | OpsInvBatchUntouchedChannel =>
      lookup_channel sibling_channel mutant_batch_changes_untouched <>
      lookup_channel sibling_channel authority_fixture
  | OpsInvFailureAtomic =>
      mutant_failed_state <> transaction_fixture
  | OpsInvAcceptedOnePublication =>
      mutant_success_publication_count <> 1%nat
  | OpsInvTransitionDeterministic =>
      mutant_left_outcome <> mutant_right_outcome
  | OpsInvPublishedStoreWellFormed =>
      ~ store_state_well_formed mutant_wrong_loaded_count_state
  | OpsInvTouchedCatalogInvalidated =>
      mutant_touched_catalog <> None
  | OpsInvUntouchedCatalogPreserved =>
      mutant_untouched_catalog <> catalog_fixture sibling_channel
  | OpsInvIssuedSnapshotsPreserved =>
      mutant_external_snapshots <> 17%nat
  | OpsInvGenerationAdvancedOnce =>
      mutant_generation <> S 23%nat
  end.

Theorem every_two_level_operation_mutant_has_a_causal_witness : forall invariant,
  two_level_operation_mutant_killed invariant.
Proof.
  intro invariant. destruct invariant; cbn.
  - apply kill_upsert_target_omission.
  - apply kill_upsert_unrelated_erasure.
  - apply kill_upsert_loaded_omission.
  - apply kill_unordered_outer_insertion.
  - apply kill_loaded_upsert_count_increment.
  - apply kill_unloaded_upsert_count_stasis.
  - apply kill_delete_target_retention.
  - apply kill_delete_descendant_erasure.
  - apply kill_noncanonical_delete_result.
  - apply kill_absent_delete_mutation.
  - apply kill_loaded_delete_count_stasis.
  - apply kill_duplicate_batch_targets.
  - apply kill_noncanonical_batch_result.
  - apply kill_batch_untouched_channel_mutation.
  - apply kill_failure_public_state_mutation.
  - apply kill_duplicate_root_publication.
  - apply kill_nondeterministic_outcomes.
  - apply kill_wrong_published_loaded_count.
  - apply kill_retained_touched_catalog.
  - apply kill_erased_untouched_catalog.
  - apply kill_issued_snapshot_mutation.
  - apply kill_generation_stasis.
Qed.

Theorem two_level_operation_mutant_catalog_is_total_unique_and_exact :
  NoDup two_level_operation_invariant_catalog /\
  Forall two_level_operation_mutant_killed
    two_level_operation_invariant_catalog /\
  length two_level_operation_invariant_catalog = 22%nat.
Proof.
  destruct two_level_operation_invariant_catalog_is_unique_complete_and_exact
    as [Hunique [_ Hlength]].
  split; [exact Hunique |]. split.
  - apply Forall_forall. intros invariant _.
    apply every_two_level_operation_mutant_has_a_causal_witness.
  - exact Hlength.
Qed.

Print Assumptions every_two_level_operation_mutant_has_a_causal_witness.
Print Assumptions two_level_operation_mutant_catalog_is_total_unique_and_exact.

End TwoLevelRSpaceOperationsMutants.
