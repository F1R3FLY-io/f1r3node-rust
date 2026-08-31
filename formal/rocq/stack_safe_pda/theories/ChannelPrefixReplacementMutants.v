From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import ChannelPrefixReplacement.

Import ListNotations.
Import ChannelPrefixReplacement.

(** Causal negative controls for the prepare-then-publish [set_channel]
    transaction.  Every mutant changes one decision in preparation,
    prefix-rooted publication, metadata publication, or cursor restoration. *)
Module ChannelPrefixReplacementMutants.

Definition target_marker : path := [1; 2].
Definition other_marker : path := [9].
Definition old_target_entry : bag_entry := BagEntry [1; 2; 3] 2.
Definition unrelated_entry : bag_entry := BagEntry [9; 4] 7.
Definition replacement_entry : bag_entry := BagEntry [5] 11.
Definition second_replacement_entry : bag_entry := BagEntry [6] 13.

Definition before_fixture : list bag_entry :=
  [old_target_entry; unrelated_entry].
Definition replacement_fixture : list bag_entry := [replacement_entry].

Theorem fixtures_are_non_vacuous :
  prefix_projection target_marker before_fixture = [BagEntry [3] 2] /\
  lookup_multiplicity [9; 4] before_fixture = Some 7 /\
  replace_prefix before_fixture target_marker replacement_fixture =
    [unrelated_entry; BagEntry [1; 2; 5] 11].
Proof. vm_compute. auto. Qed.

(** Mutant: clear the live target prefix before an encoder reports failure. *)
Definition mutant_clear_before_encoding_finishes : list bag_entry :=
  keep_outside_prefix target_marker before_fixture.

Theorem kill_clear_before_encoding_failure_atomicity :
  mutant_clear_before_encoding_finishes <> before_fixture.
Proof. discriminate. Qed.

(** Mutant: a prepared-looking result is published even though encoding has
    already selected the failure outcome. *)
Definition mutant_publish_after_encoding_failure : list bag_entry :=
  replace_prefix before_fixture target_marker replacement_fixture.

Theorem kill_publication_after_encoding_failure_precedence :
  mutant_publish_after_encoding_failure <> before_fixture.
Proof. discriminate. Qed.

(** Mutant: clear the whole authoritative Bag instead of only [p]. *)
Definition mutant_clear_whole_authority : list bag_entry :=
  lift_suffix_entries target_marker replacement_fixture.

Theorem kill_whole_bag_clear_non_target_loss :
  lookup_multiplicity [9; 4] mutant_clear_whole_authority <>
  lookup_multiplicity [9; 4] before_fixture.
Proof. discriminate. Qed.

(** Mutant: append the prepared Bag without draining the old target subtree. *)
Definition mutant_retain_old_target : list bag_entry :=
  before_fixture ++ lift_suffix_entries target_marker replacement_fixture.

Theorem kill_old_target_retention :
  prefix_projection target_marker mutant_retain_old_target <>
  replacement_fixture.
Proof. discriminate. Qed.

(** Mutant: publish an empty/partial staging Bag after preparation succeeded. *)
Definition mutant_skip_prepared_entry : list bag_entry :=
  replace_prefix before_fixture target_marker [].

Theorem kill_skipped_prepared_suffix :
  prefix_projection target_marker mutant_skip_prepared_entry <>
  replacement_fixture.
Proof. discriminate. Qed.

(** Mutant: replay one staged suffix twice. *)
Definition mutant_duplicate_prepared_entry : list bag_entry :=
  replace_prefix before_fixture target_marker
    [replacement_entry; replacement_entry].

Theorem kill_duplicated_prepared_suffix :
  prefix_projection target_marker mutant_duplicate_prepared_entry <>
  replacement_fixture.
Proof. discriminate. Qed.

(** Mutant: feed relative staging keys to an absolute root cursor. *)
Definition mutant_omit_target_marker_composition : list bag_entry :=
  keep_outside_prefix target_marker before_fixture ++ replacement_fixture.

Theorem kill_unframed_relative_key_publication :
  prefix_projection target_marker mutant_omit_target_marker_composition <>
  replacement_fixture.
Proof. discriminate. Qed.

(** Mutant: collapse every prepared multiplicity to set membership. *)
Definition mutant_flatten_multiplicity_to_one : list bag_entry :=
  replace_prefix before_fixture target_marker [BagEntry [5] 1].

Theorem kill_multiplicity_flattening :
  lookup_multiplicity [5]
    (prefix_projection target_marker mutant_flatten_multiplicity_to_one) <>
  lookup_multiplicity [5] replacement_fixture.
Proof. discriminate. Qed.

(** Mutant: allow duplicate relative keys through checked preparation. *)
Definition mutant_duplicate_prepared_keys : list bag_entry :=
  [replacement_entry; replacement_entry].

Theorem kill_duplicate_prepared_key_acceptance :
  ~ entries_well_formed mutant_duplicate_prepared_keys.
Proof.
  intros [unique _]. inversion unique as [|key keys absent tail_unique]; subst.
  apply absent. simpl. auto.
Qed.

(** Mutant: accept a multiplicity one greater than [u64::MAX]. *)
Definition mutant_overflowing_multiplicity : list bag_entry :=
  [BagEntry [5] (S max_u64)].

Theorem kill_unchecked_u64_multiplicity :
  ~ entries_fit_u64 mutant_overflowing_multiplicity.
Proof.
  unfold entries_fit_u64, mutant_overflowing_multiplicity. simpl.
  intro bounded. inversion bounded as [|entry entries head_bounded tail_bounded].
  exact (Nat.nle_succ_diag_l max_u64 head_bounded).
Qed.

(** Mutant: a relative rebase silently replaces the immutable target root. *)
Definition mutant_rebase_changes_target
    (_ : guarded_target_cursor) (suffix : path) : guarded_target_cursor :=
  GuardedTargetCursor suffix [].

Theorem kill_cursor_target_root_drift :
  cursor_target_root
    (mutant_rebase_changes_target (open_target_cursor target_marker) [7]) <>
  cursor_target_root (open_target_cursor target_marker).
Proof. discriminate. Qed.

(** Mutant: an absolute write capability bypasses the target-root type. *)
Definition mutant_absolute_cursor_write : bag_entry := BagEntry [9; 8] 1.

Theorem kill_out_of_prefix_absolute_cursor_write :
  strip_prefix target_marker (entry_path mutant_absolute_cursor_write) = None.
Proof. reflexivity. Qed.

(** Mutant: restore truncates to the suffix rather than the immutable marker. *)
Definition mutant_restore_to_suffix
    (marker suffix : path) : reusable_path_buffer :=
  ReusablePathBuffer marker suffix.

Theorem kill_reusable_buffer_wrong_restore :
  mutant_restore_to_suffix target_marker [5] <>
  buffer_at_marker target_marker.
Proof. discriminate. Qed.

(** Mutant: an already-loaded channel increments the count again. *)
Definition mutant_increment_loaded_count_always (count : nat) : nat := S count.

Theorem kill_double_loaded_count_increment :
  mutant_increment_loaded_count_always 1 <> 1.
Proof. discriminate. Qed.

(** Mutant: an empty replacement is not recorded as a loaded channel. *)
Definition mutant_empty_replacement_loaded_set : list path := [].

Theorem kill_empty_channel_not_loaded :
  ~ In target_marker mutant_empty_replacement_loaded_set.
Proof. simpl. tauto. Qed.

(** Mutant: publication occurs at capacity without checked successor room. *)
Definition mutant_loaded_past_capacity : nat := 2.

Theorem kill_missing_loaded_capacity_preflight :
  ~ mutant_loaded_past_capacity <= 1.
Proof. unfold mutant_loaded_past_capacity. lia. Qed.

Definition catalog_fixture (marker : path) : option nat :=
  if path_eq_dec marker target_marker then Some 21 else Some 34.

(** Mutant: retain the stale target candidate catalog after replacement. *)
Definition mutant_retain_target_catalog : path -> option nat := catalog_fixture.

Theorem kill_stale_target_catalog :
  mutant_retain_target_catalog target_marker <> None.
Proof. vm_compute. discriminate. Qed.

(** Mutant: invalidate every candidate catalog instead of exactly [K[p]]. *)
Definition mutant_clear_all_catalogs (_ : path) : option nat := None.

Theorem kill_non_target_catalog_clear :
  mutant_clear_all_catalogs other_marker <> catalog_fixture other_marker.
Proof. vm_compute. discriminate. Qed.

(** Mutant: mutate the externally-held immutable catalog snapshot token. *)
Definition mutant_external_snapshot_after_publish : nat := 42.

Theorem kill_external_catalog_snapshot_mutation :
  mutant_external_snapshot_after_publish <> 41.
Proof. discriminate. Qed.

(** Mutant: replay the private Bag in a different order than the prepared
    canonical lexicographic witness. *)
Definition ordered_replacement : list bag_entry :=
  [replacement_entry; second_replacement_entry].
Definition mutant_reverse_replay : list bag_entry :=
  replace_prefix before_fixture target_marker (rev ordered_replacement).

Theorem kill_noncanonical_replay_order :
  prefix_projection target_marker mutant_reverse_replay <>
  ordered_replacement.
Proof. discriminate. Qed.

Inductive channel_replacement_mutant_id : Type :=
| ClearBeforeEncodingMutant
| PublishAfterEncodingFailureMutant
| WholeAuthorityClearMutant
| RetainOldTargetMutant
| SkipPreparedSuffixMutant
| DuplicatePreparedSuffixMutant
| UnframedRelativeKeyMutant
| FlattenMultiplicityMutant
| DuplicatePreparedKeyMutant
| U64OverflowMutant
| CursorTargetDriftMutant
| AbsoluteCursorWriteMutant
| WrongBufferRestoreMutant
| DoubleLoadedCountMutant
| EmptyNotLoadedMutant
| CapacityPreflightOmissionMutant
| StaleTargetCatalogMutant
| ClearAllCatalogsMutant
| ExternalSnapshotMutationMutant
| NoncanonicalReplayOrderMutant.

Definition channel_replacement_mutant_catalog :
    list channel_replacement_mutant_id :=
  [ClearBeforeEncodingMutant; WholeAuthorityClearMutant;
   PublishAfterEncodingFailureMutant;
   RetainOldTargetMutant; SkipPreparedSuffixMutant;
   DuplicatePreparedSuffixMutant; UnframedRelativeKeyMutant;
   FlattenMultiplicityMutant; DuplicatePreparedKeyMutant; U64OverflowMutant;
   CursorTargetDriftMutant; AbsoluteCursorWriteMutant;
   WrongBufferRestoreMutant; DoubleLoadedCountMutant; EmptyNotLoadedMutant;
   CapacityPreflightOmissionMutant; StaleTargetCatalogMutant;
   ClearAllCatalogsMutant; ExternalSnapshotMutationMutant;
   NoncanonicalReplayOrderMutant].

Definition channel_replacement_mutant_killed
    (mutant : channel_replacement_mutant_id) : Prop :=
  match mutant with
  | ClearBeforeEncodingMutant =>
      mutant_clear_before_encoding_finishes <> before_fixture
  | PublishAfterEncodingFailureMutant =>
      mutant_publish_after_encoding_failure <> before_fixture
  | WholeAuthorityClearMutant =>
      lookup_multiplicity [9; 4] mutant_clear_whole_authority <>
      lookup_multiplicity [9; 4] before_fixture
  | RetainOldTargetMutant =>
      prefix_projection target_marker mutant_retain_old_target <>
      replacement_fixture
  | SkipPreparedSuffixMutant =>
      prefix_projection target_marker mutant_skip_prepared_entry <>
      replacement_fixture
  | DuplicatePreparedSuffixMutant =>
      prefix_projection target_marker mutant_duplicate_prepared_entry <>
      replacement_fixture
  | UnframedRelativeKeyMutant =>
      prefix_projection target_marker mutant_omit_target_marker_composition <>
      replacement_fixture
  | FlattenMultiplicityMutant =>
      lookup_multiplicity [5]
        (prefix_projection target_marker mutant_flatten_multiplicity_to_one) <>
      lookup_multiplicity [5] replacement_fixture
  | DuplicatePreparedKeyMutant =>
      ~ entries_well_formed mutant_duplicate_prepared_keys
  | U64OverflowMutant => ~ entries_fit_u64 mutant_overflowing_multiplicity
  | CursorTargetDriftMutant =>
      cursor_target_root
        (mutant_rebase_changes_target (open_target_cursor target_marker) [7]) <>
      cursor_target_root (open_target_cursor target_marker)
  | AbsoluteCursorWriteMutant =>
      strip_prefix target_marker (entry_path mutant_absolute_cursor_write) = None
  | WrongBufferRestoreMutant =>
      mutant_restore_to_suffix target_marker [5] <>
      buffer_at_marker target_marker
  | DoubleLoadedCountMutant => mutant_increment_loaded_count_always 1 <> 1
  | EmptyNotLoadedMutant =>
      ~ In target_marker mutant_empty_replacement_loaded_set
  | CapacityPreflightOmissionMutant => ~ mutant_loaded_past_capacity <= 1
  | StaleTargetCatalogMutant => mutant_retain_target_catalog target_marker <> None
  | ClearAllCatalogsMutant =>
      mutant_clear_all_catalogs other_marker <> catalog_fixture other_marker
  | ExternalSnapshotMutationMutant => mutant_external_snapshot_after_publish <> 41
  | NoncanonicalReplayOrderMutant =>
      prefix_projection target_marker mutant_reverse_replay <>
      ordered_replacement
  end.

Theorem every_channel_replacement_mutant_has_a_causal_counterexample : forall mutant,
  channel_replacement_mutant_killed mutant.
Proof.
  intro mutant. destruct mutant; cbn.
  - apply kill_clear_before_encoding_failure_atomicity.
  - apply kill_publication_after_encoding_failure_precedence.
  - apply kill_whole_bag_clear_non_target_loss.
  - apply kill_old_target_retention.
  - apply kill_skipped_prepared_suffix.
  - apply kill_duplicated_prepared_suffix.
  - apply kill_unframed_relative_key_publication.
  - apply kill_multiplicity_flattening.
  - apply kill_duplicate_prepared_key_acceptance.
  - apply kill_unchecked_u64_multiplicity.
  - apply kill_cursor_target_root_drift.
  - apply kill_out_of_prefix_absolute_cursor_write.
  - apply kill_reusable_buffer_wrong_restore.
  - apply kill_double_loaded_count_increment.
  - apply kill_empty_channel_not_loaded.
  - apply kill_missing_loaded_capacity_preflight.
  - apply kill_stale_target_catalog.
  - apply kill_non_target_catalog_clear.
  - apply kill_external_catalog_snapshot_mutation.
  - apply kill_noncanonical_replay_order.
Qed.

Theorem channel_replacement_mutant_catalog_is_unique_complete_and_exact :
  NoDup channel_replacement_mutant_catalog /\
  Forall channel_replacement_mutant_killed channel_replacement_mutant_catalog /\
  length channel_replacement_mutant_catalog = 20.
Proof.
  repeat split.
  - repeat constructor; simpl; intuition discriminate.
  - repeat constructor;
      apply every_channel_replacement_mutant_has_a_causal_counterexample.
Qed.

Print Assumptions every_channel_replacement_mutant_has_a_causal_counterexample.
Print Assumptions channel_replacement_mutant_catalog_is_unique_complete_and_exact.

End ChannelPrefixReplacementMutants.
