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

End RSpaceCallerAuthorityMutants.
