From Stdlib Require Import Arith Lia Lists.List.
From StackSafePDA Require Import RSpaceCallerAuthority.
Import ListNotations RSpaceCallerAuthority.RSpaceCallerAuthority.

Module RSpaceCallerAuthorityMutants.

Definition dag := Domain PathMapDag 1 2 3.
Definition initial := Owner dag 4 10 7 true true [10; 20] [(99, 10)].
Definition permit := view_permit initial.

Example generation_blind_aba_is_not_authorized :
  let restored := published (published initial 20) 10 in
  current_root restored = expected_root permit /\ ~ authorizes restored permit.
Proof. simpl; split; [reflexivity|]. apply root_aba_does_not_revive_a_permit; repeat split. Qed.

Example ready_bit_alone_cannot_revive_closed_permit :
  ~ authorizes (Owner dag 4 10 7 false true [10; 20] [(99, 10)]) permit.
Proof. unfold authorizes; simpl; intuition discriminate. Qed.

Example fresh_generation_does_not_replace_owner_identity :
  ~ authorizes (reopen initial 5) permit.
Proof. apply reopened_owner_rejects_old_incarnation; simpl; congruence. Qed.

Example matching_root_and_generation_do_not_replace_backend_identity :
  ~ authorizes (Owner (Domain Lmdb 1 2 3) 4 10 7 true true [10; 20] [(99, 10)]) permit.
Proof. unfold authorizes, permit, initial, view_permit, dag; simpl; intuition discriminate. Qed.

Example matching_backend_does_not_replace_store_profile_or_epoch :
  ~ authorizes (Owner (Domain PathMapDag 9 2 3) 4 10 7 true true [10; 20] [(99, 10)]) permit /\
  ~ authorizes (Owner (Domain PathMapDag 1 9 3) 4 10 7 true true [10; 20] [(99, 10)]) permit /\
  ~ authorizes (Owner (Domain PathMapDag 1 2 9) 4 10 7 true true [10; 20] [(99, 10)]) permit.
Proof. unfold authorizes, permit, initial, view_permit, dag; simpl; intuition discriminate. Qed.

Example awaiting_acknowledgement_is_not_ready :
  ~ authorizes (Owner dag 4 10 7 true false [10; 20] [(99, 10)]) permit.
Proof. apply not_ready_refuses_mutation; reflexivity. Qed.

Example retained_but_semantically_refused_root_has_no_capability :
  admit_historical_root (fun _ _ => false) initial 20 = None.
Proof. apply retained_membership_does_not_skip_semantic_admission; reflexivity. Qed.

Example semantically_valid_but_unknown_root_has_no_capability :
  admit_historical_root (fun _ _ => true) initial 30 = None.
Proof. apply unknown_target_cannot_be_admitted; simpl; intuition congruence. Qed.

Example duplicate_alias_is_not_a_single_resolution :
  ~ resolves_alias initial (AliasCatalog dag [(99, 10); (99, 20)]) 99 10.
Proof. apply missing_or_ambiguous_alias_refuses; simpl; lia. Qed.

Example provenance_does_not_retarget_an_inherited_alias :
  ~ resolve_manifest_alias initial (AliasCatalog dag [(99, 10)]) 99
      (StateManifest 20 (Some 99)).
Proof. unfold resolve_manifest_alias, resolves_alias; simpl; intuition discriminate. Qed.

Example absent_alias_does_not_fall_back_to_provenance :
  ~ resolve_manifest_alias initial (AliasCatalog dag []) 99 (StateManifest 20 (Some 99)).
Proof. unfold resolve_manifest_alias; apply missing_or_ambiguous_alias_refuses; simpl; lia. Qed.

Definition pinned := RetainedReader dag 10 6 100.

Example teardown_does_not_make_pinned_storage_reclaimable :
  ~ authorizes (close initial) permit /\ ~ reclaimable [pinned] dag 6.
Proof.
  split; [apply closed_owner_revokes_every_mutation_permit|].
  apply (pinned_generation_cannot_be_reclaimed [pinned] pinned); simpl; auto.
Qed.

Definition journals := FiveJournals nat nat nat nat nat 1 2 3 4 5.
Definition saved : @soft_payload nat nat (list nat) nat nat nat nat nat nat :=
  SoftPayload (fun k => if Nat.eqb k 0 then Some 0 else None) [8; 9] 2 journals.
Definition receiving_view := MutableView dag 4 10.
Definition receiving_history (v : mutable_view) (history : nat -> nat) : Prop :=
  v = receiving_view /\ forall key, history key = 3.
Definition fixture_journals (payload : @soft_payload nat nat (list nat) nat nat nat nat nat nat)
    (_ : nat -> nat) (candidate : five_journals nat nat nat nat nat) : Prop :=
  candidate = captured_journals payload.

Example captured_empty_is_not_an_uncaptured_cell :
  restored_values (restore saved (fun _ => 3) journals) 0 = 0 /\
  restored_values (restore saved (fun _ => 3) journals) 1 = 3.
Proof. split; reflexivity. Qed.

Example cross_base_restore_does_not_require_captured_generation :
  let receiver := published initial 20 in
  permitted_restore receiving_history fixture_journals
    receiver (view_permit receiver) receiving_view saved (fun _ => 3) journals
    receiver (restore saved (fun _ => 3) journals).
Proof. constructor; unfold receiving_history, fixture_journals, attached; repeat split. Qed.

Example cross_base_restore_does_not_revive_old_write_permit :
  ~ permitted_restore receiving_history fixture_journals
      (published initial 20) permit receiving_view saved (fun _ => 3) journals
      (published initial 20) (restore saved (fun _ => 3) journals).
Proof.
  intro restored; apply payload_transfer_never_transfers_mutation_authority in restored.
  eapply publication_invalidates_all_old_view_permits; [|exact restored]; repeat split.
Qed.

Example immediate_predecessor_freshness_allows_reopen_aba :
  authorizes (reopen (close (reopen (close initial) 5)) 4) permit /\
  ~ reopen_transition (close (reopen (close initial) 5)) [4; 5] 4
      (reopen (close (reopen (close initial) 5)) 4).
Proof.
  split; [repeat split|]. intro step; inversion step; subst.
  apply H1; simpl; auto.
Qed.

Example same_domain_forged_alias_catalog_is_not_appointed :
  ~ resolves_alias initial (AliasCatalog dag [(99, 20)]) 99 20.
Proof. apply unappointed_alias_catalog_cannot_resolve; simpl; discriminate. Qed.

Example foreign_history_is_not_a_receiving_view_history :
  ~ permitted_restore receiving_history fixture_journals
      initial permit receiving_view saved (fun _ => 4) journals
      initial (restore saved (fun _ => 4) journals).
Proof.
  intro step; apply soft_restore_requires_receiving_history_and_complete_journals in step.
  destruct step as [_ [[_ history] _]]; specialize (history 0); discriminate.
Qed.

Example dropping_one_of_five_journals_refuses_restore :
  let incomplete := FiveJournals nat nat nat nat nat 1 2 3 4 0 in
  ~ permitted_restore receiving_history fixture_journals
      initial permit receiving_view saved (fun _ => 3) incomplete
      initial (restore saved (fun _ => 3) incomplete).
Proof.
  intro incomplete; intro step.
  apply soft_restore_requires_receiving_history_and_complete_journals in step.
  destruct step as [_ [_ invalid]]. unfold fixture_journals in invalid.
  simpl in invalid; discriminate.
Qed.

Example old_journal_reuse_changes_the_restored_value :
  apply_count_delta 3 (rebased_addition 1 2) (rebased_removal 1 2) = 4 /\
  apply_count_delta 3 (rebased_addition 3 2) (rebased_removal 3 2) = 2.
Proof. split; reflexivity. Qed.

Example copying_old_base_count_journals_misses_the_restored_count :
  let captured : nat -> option nat := fun _ => Some 2 in
  apply_count_journal (fun _ => 3) (rebased_count_addition captured (fun _ => 1))
    (rebased_count_removal captured (fun _ => 1)) 0 = 4 /\
  apply_count_journal (fun _ => 3) (rebased_count_addition captured (fun _ => 3))
    (rebased_count_removal captured (fun _ => 3)) 0 = 2 /\
  count_overlay captured (fun _ => 3) 0 = 2.
Proof. vm_compute; repeat split. Qed.

Example omitting_count_removals_misses_a_lower_restored_count :
  let captured : nat -> option nat := fun _ => Some 2 in
  apply_count_journal (fun _ => 3) (rebased_count_addition captured (fun _ => 3))
    (fun _ => 0) 0 = 3 /\
  count_overlay captured (fun _ => 3) 0 = 2.
Proof. vm_compute; repeat split. Qed.

Example journal_key_only_rebase_misses_a_captured_cell_with_no_old_delta :
  let captured : nat -> option nat := fun _ => Some 1 in
  let journal_key key :=
    negb (Nat.eqb (rebased_count_addition captured (fun _ => 1) key +
      rebased_count_removal captured (fun _ => 1) key) 0) in
  let addition key :=
    if journal_key key then rebased_count_addition captured (fun _ => 3) key else 0 in
  let removal key :=
    if journal_key key then rebased_count_removal captured (fun _ => 3) key else 0 in
  journal_key 0 = false /\
  apply_count_journal (fun _ => 3) addition removal 0 = 3 /\
  count_overlay captured (fun _ => 3) 0 = 1 /\
  apply_count_journal (fun _ => 3) (rebased_count_addition captured (fun _ => 3))
    (rebased_count_removal captured (fun _ => 3)) 0 = 1.
Proof. vm_compute; repeat split. Qed.

Example keeping_the_captured_before_misses_the_receiving_base :
  ~ exact_content_transition (Some 3) (Some 2) (Some (Some 1, Some 2)) /\
  rebased_content Nat.eq_dec (Some 3) (Some 2) = Some (Some 3, Some 2) /\
  exact_content_transition (Some 3) (Some 2) (rebased_content Nat.eq_dec (Some 3) (Some 2)).
Proof.
  cbn; split; [intros [before _]; discriminate before|].
  split; [reflexivity|split; [reflexivity|split; [reflexivity|discriminate]]].
Qed.

Example copying_captured_sources_misses_bytes_after_a_cross_base_restore :
  rebased_content Nat.eq_dec (Some 2) (Some 2) = None /\
  source_of_transition (fun reference : nat => reference)
    (rebased_content Nat.eq_dec (Some 2) (Some 2)) = None /\
  source_of_transition (fun reference : nat => reference)
    (rebased_content Nat.eq_dec (Some 3) (Some 2)) = Some 2.
Proof. vm_compute; repeat split. Qed.

Example stale_source_bytes_for_a_removed_record_are_not_the_rebased_source :
  rebased_content Nat.eq_dec (Some 2) None = Some (Some 2, None) /\
  source_of_transition (fun reference : nat => reference)
    (rebased_content Nat.eq_dec (Some 2) None) = None /\
  source_of_transition (fun reference : nat => reference) (Some (Some 1, Some 2)) = Some 2.
Proof. vm_compute; repeat split. Qed.

Print Assumptions generation_blind_aba_is_not_authorized.
Print Assumptions ready_bit_alone_cannot_revive_closed_permit.
Print Assumptions fresh_generation_does_not_replace_owner_identity.
Print Assumptions matching_root_and_generation_do_not_replace_backend_identity.
Print Assumptions matching_backend_does_not_replace_store_profile_or_epoch.
Print Assumptions awaiting_acknowledgement_is_not_ready.
Print Assumptions retained_but_semantically_refused_root_has_no_capability.
Print Assumptions semantically_valid_but_unknown_root_has_no_capability.
Print Assumptions duplicate_alias_is_not_a_single_resolution.
Print Assumptions provenance_does_not_retarget_an_inherited_alias.
Print Assumptions absent_alias_does_not_fall_back_to_provenance.
Print Assumptions teardown_does_not_make_pinned_storage_reclaimable.
Print Assumptions captured_empty_is_not_an_uncaptured_cell.
Print Assumptions cross_base_restore_does_not_require_captured_generation.
Print Assumptions cross_base_restore_does_not_revive_old_write_permit.
Print Assumptions old_journal_reuse_changes_the_restored_value.
Print Assumptions immediate_predecessor_freshness_allows_reopen_aba.
Print Assumptions same_domain_forged_alias_catalog_is_not_appointed.
Print Assumptions foreign_history_is_not_a_receiving_view_history.
Print Assumptions dropping_one_of_five_journals_refuses_restore.
Print Assumptions copying_old_base_count_journals_misses_the_restored_count.
Print Assumptions omitting_count_removals_misses_a_lower_restored_count.
Print Assumptions journal_key_only_rebase_misses_a_captured_cell_with_no_old_delta.
Print Assumptions keeping_the_captured_before_misses_the_receiving_base.
Print Assumptions copying_captured_sources_misses_bytes_after_a_cross_base_restore.
Print Assumptions stale_source_bytes_for_a_removed_record_are_not_the_rebased_source.

Set Implicit Arguments.

Definition scope_config := LmdbConfiguration 1 64 8.
Definition scope_mapping prefix role name config :=
  DatabaseMapping (prefix ++ role_suffix role) (Some name) config.
Definition scope_selection prefix history roots cold cold_config :=
  AuthoritySelection prefix
    (scope_mapping prefix HistoryRole history scope_config)
    (scope_mapping prefix RootsRole roots scope_config)
    (scope_mapping prefix ColdRole cold cold_config).
Definition scope_a := scope_selection [7] [11] [12] [13] scope_config.
Definition scope_b := scope_selection [7] [21] [22] [23] scope_config.
Definition scope_overlap := scope_selection [7] [11] [22] [23] scope_config.
Definition scope_nul := scope_selection [7] [0; 11] [12] [13] scope_config.
Definition scope_collision := scope_selection [7] [11] [11] [13] scope_config.
Definition scope_mappings selection :=
  [selected_history selection; selected_roots selection; selected_cold selection].
Definition scope_default := AuthoritySelection [7]
  (DatabaseMapping ([7] ++ role_suffix HistoryRole) None scope_config)
  (DatabaseMapping ([7] ++ role_suffix RootsRole) None scope_config)
  (DatabaseMapping ([7] ++ role_suffix ColdRole) None scope_config).

Example resolved_scope_has_an_accepted_fixture :
  scope_admitted (scope_mappings scope_a) [] scope_a.
Proof.
  unfold scope_admitted, exact_role_selection, physical_names_have_no_nul,
    names_fit, field_fits, physical_roles_distinct, footprints_disjoint.
  vm_compute. repeat split; try (repeat constructor; simpl; intuition discriminate).
Qed.

Example ignoring_overrides_collapses_distinct_authority_names :
  (forall role, logical_database_id (selected_mapping scope_a role) =
     logical_database_id (selected_mapping scope_b role)) /\
  (forall role, resolve_database_name (selected_mapping scope_a role) <>
     resolve_database_name (selected_mapping scope_b role)).
Proof. split; intro role; destruct role; vm_compute; congruence. Qed.

Example missing_cold_role_is_rejected :
  ~ scope_admitted [selected_history scope_a; selected_roots scope_a] [] scope_a.
Proof.
  apply (missing_or_duplicate_logical_role_refuses ColdRole).
  vm_compute; discriminate.
Qed.

Example duplicated_history_role_is_rejected :
  ~ scope_admitted (selected_history scope_a :: scope_mappings scope_a) [] scope_a.
Proof.
  apply (missing_or_duplicate_logical_role_refuses HistoryRole).
  vm_compute; discriminate.
Qed.

Example history_roots_configuration_mismatch_is_rejected :
  let incompatible := AuthoritySelection [7] (selected_history scope_a)
    (scope_mapping [7] RootsRole [12] (LmdbConfiguration 1 65 8))
    (selected_cold scope_a) in
  ~ scope_admitted (scope_mappings incompatible) [] incompatible.
Proof.
  intro incompatible; apply incompatible_history_roots_configuration_refuses.
  discriminate.
Qed.

Example embedded_nul_is_rejected :
  ~ scope_admitted (scope_mappings scope_nul) [] scope_nul.
Proof.
  apply nul_physical_name_refuses. unfold physical_names_have_no_nul.
  simpl; intuition.
Qed.

Example dropping_the_u64_length_gate_admits_an_unencodable_field :
  ~ (forall length, ScopeBytes.fits_fixed_be 8 length = true).
Proof.
  intro unchecked. pose proof (unchecked (256 ^ 8)) as fits.
  apply ScopeBytes.fits_fixed_be_true_iff in fits.
  exact (Nat.lt_irrefl _ fits).
Qed.

Example colliding_role_names_are_rejected :
  ~ scope_admitted (scope_mappings scope_collision) [] scope_collision.
Proof.
  apply colliding_physical_roles_refuse. intro unique.
  inversion unique; subst. apply H1; simpl; auto.
Qed.

Example ordinary_writable_alias_is_rejected :
  ~ scope_admitted (scope_mappings scope_a) [live_slot scope_a HistoryRole] scope_a.
Proof.
  eapply writable_ordinary_alias_refuses with (slot := live_slot scope_a HistoryRole);
    unfold authority_footprint; simpl; auto.
Qed.

Example opening_before_nul_validation_is_not_an_admitted_attempt :
  ~ scope_open_attempt (scope_mappings scope_nul) [] scope_nul
      (authority_footprint scope_nul).
Proof.
  intro opened.
  pose proof (rejected_scope_performs_no_database_open embedded_nul_is_rejected opened) as empty.
  discriminate.
Qed.

Example unrelated_environment_accepts_reserved_looking_literal_names :
  footprints_disjoint (authority_footprint scope_a)
    [(2, rspace_prefix ++ role_suffix HistoryRole)].
Proof. apply unrelated_environment_does_not_reserve_literal_names; discriminate. Qed.

Definition unframed_scope_names names :=
  logical_prefix names ++ history_name names ++ roots_name names ++ cold_name names.

Example omitting_length_frames_collides_while_exact_encoding_separates :
  let left := ResolvedNames [1] [2; 3] [4] [5] in
  let right := ResolvedNames [1; 2] [3] [4] [5] in
  unframed_scope_names left = unframed_scope_names right /\
  migration_namespace left <> migration_namespace right.
Proof. vm_compute; split; congruence. Qed.

Example omitting_roots_field_collides_while_exact_encoding_separates :
  let left := selection_names scope_a in
  let right := selection_names (scope_selection [7] [11] [22] [13] scope_config) in
  (logical_prefix left, history_name left, cold_name left) =
    (logical_prefix right, history_name right, cold_name right) /\
  migration_namespace left <> migration_namespace right.
Proof. vm_compute; split; congruence. Qed.

Example omitting_logical_prefix_collides_while_exact_encoding_separates :
  let left := selection_names scope_a in
  let right := selection_names (scope_selection [8] [11] [12] [13] scope_config) in
  (history_name left, roots_name left, cold_name left) =
    (history_name right, roots_name right, cold_name right) /\
  migration_namespace left <> migration_namespace right.
Proof. vm_compute; split; congruence. Qed.

Example always_framing_changes_legacy_namespace_bytes :
  migration_namespace (selection_names scope_default) = [7] /\
  scope_namespace_tag ++ framed_names (selection_names scope_default) <> [7].
Proof. vm_compute; split; congruence. Qed.

Example unequal_triples_can_share_a_physical_write :
  selection_names scope_a <> selection_names scope_overlap /\
  ~ footprints_disjoint (authority_footprint scope_a) (authority_footprint scope_overlap) /\
  update_physical_cell (fun _ _ => None) (live_slot scope_a HistoryRole) 4 99
    (live_slot scope_overlap HistoryRole) 4 = Some 99.
Proof.
  split; [discriminate|]. split.
  - intro disjoint. apply (disjoint (live_slot scope_a HistoryRole)); simpl; auto.
  - vm_compute; reflexivity.
Qed.

Example namespace_bytes_are_not_the_literal_migration_cold_database :
  snd (migration_cold_source scope_a) = [13] /\
  snd (migration_cold_source scope_a) <> migration_namespace (selection_names scope_a).
Proof. vm_compute; split; congruence. Qed.

Example split_cold_source_is_not_the_live_destination_environment :
  let split_source := scope_selection [7] [11] [12] [13] (LmdbConfiguration 9 64 8) in
  scope_admitted (scope_mappings split_source) [] split_source /\
  migration_cold_source split_source = (9, [13]) /\
  live_slot split_source ColdRole = (1, [13]).
Proof.
  intro split_source; split.
  - unfold scope_admitted, exact_role_selection, physical_names_have_no_nul,
      names_fit, field_fits, physical_roles_distinct, footprints_disjoint.
    vm_compute. repeat split; try (repeat constructor; simpl; intuition discriminate).
  - split; reflexivity.
Qed.

Example scoped_cold_does_not_search_the_ambient_database :
  migration_cold_candidates scope_a = [[13]] /\
  ~ In legacy_db_name (migration_cold_candidates scope_a).
Proof. vm_compute; split; [reflexivity|intuition discriminate]. Qed.

Example logical_rspace_prefix_alone_does_not_authorize_legacy_v2 :
  let scoped := scope_selection rspace_prefix [11] [12] [13] scope_config in
  selected_prefix scoped = rspace_prefix /\ ~ legacy_v2_allowed scoped.
Proof. vm_compute; split; [reflexivity|intuition discriminate]. Qed.

Example exact_unscoped_defaults_keep_legacy_migration_behavior :
  let unscoped := AuthoritySelection rspace_prefix
    (DatabaseMapping (rspace_prefix ++ role_suffix HistoryRole) None scope_config)
    (DatabaseMapping (rspace_prefix ++ role_suffix RootsRole) None scope_config)
    (DatabaseMapping (rspace_prefix ++ role_suffix ColdRole) None scope_config) in
  legacy_v2_allowed unscoped /\
  migration_cold_candidates unscoped =
    [rspace_prefix ++ role_suffix ColdRole; legacy_db_name].
Proof. vm_compute; repeat split. Qed.

Example foreign_scope_migration_marker_is_rejected :
  ~ migration_marker_accepts (selection_names scope_b)
      (migration_namespace (selection_names scope_a)).
Proof. vm_compute; congruence. Qed.

Example reopening_the_exact_resolved_layout_preserves_content :
  forall (before : physical_contents) role key,
  before (live_slot scope_a role) key =
    before (live_slot (scope_selection [7] [11] [12] [13] scope_config) role) key.
Proof. reflexivity. Qed.

Print Assumptions resolved_scope_has_an_accepted_fixture.
Print Assumptions ignoring_overrides_collapses_distinct_authority_names.
Print Assumptions missing_cold_role_is_rejected.
Print Assumptions duplicated_history_role_is_rejected.
Print Assumptions history_roots_configuration_mismatch_is_rejected.
Print Assumptions embedded_nul_is_rejected.
Print Assumptions dropping_the_u64_length_gate_admits_an_unencodable_field.
Print Assumptions colliding_role_names_are_rejected.
Print Assumptions ordinary_writable_alias_is_rejected.
Print Assumptions opening_before_nul_validation_is_not_an_admitted_attempt.
Print Assumptions unrelated_environment_accepts_reserved_looking_literal_names.
Print Assumptions omitting_length_frames_collides_while_exact_encoding_separates.
Print Assumptions omitting_roots_field_collides_while_exact_encoding_separates.
Print Assumptions omitting_logical_prefix_collides_while_exact_encoding_separates.
Print Assumptions always_framing_changes_legacy_namespace_bytes.
Print Assumptions unequal_triples_can_share_a_physical_write.
Print Assumptions namespace_bytes_are_not_the_literal_migration_cold_database.
Print Assumptions split_cold_source_is_not_the_live_destination_environment.
Print Assumptions scoped_cold_does_not_search_the_ambient_database.
Print Assumptions logical_rspace_prefix_alone_does_not_authorize_legacy_v2.
Print Assumptions exact_unscoped_defaults_keep_legacy_migration_behavior.
Print Assumptions foreign_scope_migration_marker_is_rejected.
Print Assumptions reopening_the_exact_resolved_layout_preserves_content.

End RSpaceCallerAuthorityMutants.
