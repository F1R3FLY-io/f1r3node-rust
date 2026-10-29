From Stdlib Require Import Bool List Arith Lia ZArith.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority
  RhoPathV1Datum
  TwoLevelRSpaceContinuationAuthority.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceContinuationAuthority.
Set Implicit Arguments.
Open Scope nat_scope.

(** Executable causal controls for exact continuation authority.

    Each control changes one production choice and exposes a concrete bad
    observation.  The fixtures are logical observations of PathMap endpoints;
    they do not prescribe a list-backed production representation. *)
Module TwoLevelRSpaceContinuationAuthorityMutants.

Definition fixture_group : GroupKey := [[1]; [2]].
Definition fixture_prefix_group : GroupKey := [[1]].
Definition fixture_other_group : GroupKey := [[3]].
Definition fixture_first_path : Path := [10].
Definition fixture_second_path : Path := [20].
Definition fixture_join_channel : Path := [30].

Definition fixture_first_record : ContinuationRecord :=
  ContinuationRecordState [40] [41] false [].
Definition fixture_second_record : ContinuationRecord :=
  ContinuationRecordState [50] [51] false [].
Definition fixture_persistent_record : ContinuationRecord :=
  ContinuationRecordState [60] [61] true [].
Definition fixture_first_expectation : ContinuationExpectation :=
  expectation_of_record fixture_first_record.
Definition fixture_persistent_expectation : ContinuationExpectation :=
  expectation_of_record fixture_persistent_record.
Definition fixture_wrong_source_expectation : ContinuationExpectation :=
  ContinuationExpectationState [99] false [].
Definition fixture_wrong_persistence_expectation : ContinuationExpectation :=
  ContinuationExpectationState [41] true [].
Definition fixture_wrong_peeks_expectation : ContinuationExpectation :=
  ContinuationExpectationState [41] false [-1]%Z.

Definition fixture_ordinary (group : GroupKey) : list OrdinaryContinuation :=
  if group_eq_dec group fixture_group then
    [OrdinaryContinuationState fixture_first_path fixture_first_record;
     OrdinaryContinuationState fixture_second_path fixture_second_record]
  else [].

Definition fixture_one_ordinary (group : GroupKey)
    : list OrdinaryContinuation :=
  if group_eq_dec group fixture_group then
    [OrdinaryContinuationState fixture_first_path fixture_first_record]
  else [].

Definition fixture_persistent_ordinary (group : GroupKey)
    : list OrdinaryContinuation :=
  if group_eq_dec group fixture_group then
    [OrdinaryContinuationState fixture_first_path fixture_persistent_record]
  else [].

Definition fixture_reordered_ordinary (group : GroupKey)
    : list OrdinaryContinuation :=
  if group_eq_dec group fixture_group then
    [OrdinaryContinuationState fixture_second_path fixture_second_record;
     OrdinaryContinuationState fixture_first_path fixture_first_record]
  else [].

Definition fixture_no_installed (_ : GroupKey) : option ContinuationRecord := None.
Definition fixture_epoch (_ : GroupKey) : EqualityEpoch.t := EqualityEpoch.Token 7.
Definition fixture_resting_joins (channel : Path) : list GroupKey :=
  if path_eq_dec channel fixture_join_channel
  then [fixture_group; fixture_other_group]
  else [].
Definition fixture_installed_joins (channel : Path) : list GroupKey :=
  if path_eq_dec channel fixture_join_channel
  then [fixture_group]
  else [].

Definition fixture_state : ContinuationAuthority :=
  ContinuationAuthorityState fixture_no_installed fixture_ordinary
    fixture_epoch fixture_resting_joins fixture_installed_joins.
Definition fixture_one_state : ContinuationAuthority :=
  ContinuationAuthorityState fixture_no_installed fixture_one_ordinary
    fixture_epoch fixture_resting_joins fixture_installed_joins.
Definition fixture_persistent_state : ContinuationAuthority :=
  ContinuationAuthorityState fixture_no_installed fixture_persistent_ordinary
    fixture_epoch fixture_resting_joins fixture_installed_joins.
Definition fixture_reordered_state : ContinuationAuthority :=
  ContinuationAuthorityState fixture_no_installed fixture_reordered_ordinary
    fixture_epoch fixture_resting_joins fixture_installed_joins.

Definition fixture_capability : ContinuationCapability :=
  issue_ordinary_capability fixture_state fixture_group 0.
Definition fixture_one_capability : ContinuationCapability :=
  issue_ordinary_capability fixture_one_state fixture_group 0.
Definition fixture_persistent_capability : ContinuationCapability :=
  issue_ordinary_capability fixture_persistent_state fixture_group 0.

(** 1. Prefix-equal groups are not exact-equal continuation groups. *)
Definition mutant_resolve_capability_group
    (state : ContinuationAuthority) (_ : GroupKey)
    (capability : ContinuationCapability) : option ResolvedTarget :=
  resolve_capability state state (capability_group capability) capability.

Example exact_group_gate_rejects_prefix_alias_mutant :
  resolve_capability fixture_state fixture_state fixture_prefix_group
      fixture_capability = None /\
  mutant_resolve_capability_group
    fixture_state fixture_prefix_group fixture_capability =
      Some (OrdinaryTarget fixture_first_path fixture_first_record).
Proof. vm_compute. split; reflexivity. Qed.

(** 2. A capability from an earlier equality epoch cannot authorize a later
    group even when the same endpoint remains present. *)
Definition fixture_fresh_epoch (group : GroupKey) : EqualityEpoch.t :=
  if group_eq_dec group fixture_group
  then EqualityEpoch.fresh (fixture_epoch group)
  else fixture_epoch group.

Definition fixture_stale_commit_state : ContinuationAuthority :=
  ContinuationAuthorityState fixture_no_installed fixture_ordinary
    fixture_fresh_epoch fixture_resting_joins fixture_installed_joins.

Definition fixture_stale_capability : ContinuationCapability :=
  issue_ordinary_capability fixture_state fixture_group 0.

Definition mutant_ignore_epoch : option ResolvedTarget :=
  resolve_bound_ordinary fixture_state fixture_group
    (ordinary_paths (ordinary_continuations fixture_state fixture_group))
    (capability_index fixture_stale_capability).

Example exact_epoch_gate_rejects_stale_epoch_mutant :
  resolve_capability fixture_state fixture_stale_commit_state fixture_group
      fixture_stale_capability = None /\
  mutant_ignore_epoch =
    Some (OrdinaryTarget fixture_first_path fixture_first_record).
Proof. vm_compute. split; reflexivity. Qed.

(** 3. Resolving a live ordinal rather than its immutable bound path retargets
    an issued capability after an insertion/reordering. *)
Definition mutant_resolve_live_ordinal : option ResolvedTarget :=
  match nth_error (fixture_reordered_ordinary fixture_group) 0 with
  | Some ordinary =>
      Some (OrdinaryTarget (ordinary_path ordinary) (ordinary_record ordinary))
  | None => None
  end.

Example bound_path_rejects_live_ordinal_retarget_mutant :
  resolve_capability fixture_state fixture_reordered_state fixture_group
      fixture_capability =
    Some (OrdinaryTarget fixture_first_path fixture_first_record) /\
  mutant_resolve_live_ordinal =
    Some (OrdinaryTarget fixture_second_path fixture_second_record).
Proof. vm_compute. split; reflexivity. Qed.

(** 4. Converting a negative signed index to [nat] before rejecting it aliases
    the first ordinary endpoint. *)
Definition fixture_negative_capability : ContinuationCapability :=
  ContinuationCapabilityState fixture_group (-1)%Z.

Definition mutant_negative_to_nat : option ResolvedTarget :=
  resolve_bound_ordinary fixture_state fixture_group
    (ordinary_paths (ordinary_continuations fixture_state fixture_group))
    (Z.of_nat (Z.to_nat (capability_index fixture_negative_capability))).

Example signed_index_gate_rejects_negative_cast_mutant :
  resolve_capability fixture_state fixture_state fixture_group
      fixture_negative_capability = None /\
  mutant_negative_to_nat =
    Some (OrdinaryTarget fixture_first_path fixture_first_record).
Proof. vm_compute. split; reflexivity. Qed.

(** 5--7. Production authenticates source, persistence, and peeks in
    addition to the slot/path identity authority.  Omitting any one field
    admits a concrete substituted commit. *)
Definition mutant_ignore_source
    (expected : ContinuationExpectation) : option ResolvedTarget :=
  match resolve_capability fixture_state fixture_state fixture_group
      fixture_capability with
  | Some target =>
      if Bool.bool_dec
          (continuation_persistent (target_record target))
          (expected_persistent expected) then
        if list_eq_dec Z.eq_dec
            (continuation_peeks (target_record target)) (expected_peeks expected)
        then Some target else None
      else None
  | None => None
  end.

Definition mutant_ignore_persistence
    (expected : ContinuationExpectation) : option ResolvedTarget :=
  match resolve_capability fixture_state fixture_state fixture_group
      fixture_capability with
  | Some target =>
      if path_eq_dec
          (continuation_source (target_record target)) (expected_source expected)
      then
        if list_eq_dec Z.eq_dec
            (continuation_peeks (target_record target)) (expected_peeks expected)
        then Some target else None
      else None
  | None => None
  end.

Definition mutant_ignore_peeks
    (expected : ContinuationExpectation) : option ResolvedTarget :=
  match resolve_capability fixture_state fixture_state fixture_group
      fixture_capability with
  | Some target =>
      if path_eq_dec
          (continuation_source (target_record target)) (expected_source expected)
      then
        if Bool.bool_dec
            (continuation_persistent (target_record target))
            (expected_persistent expected)
        then Some target else None
      else None
  | None => None
  end.

Example exact_expectation_rejects_omitted_source_mutant :
  resolve_exact_continuation fixture_state fixture_state fixture_group fixture_capability
      fixture_wrong_source_expectation = None /\
  mutant_ignore_source fixture_wrong_source_expectation =
    Some (OrdinaryTarget fixture_first_path fixture_first_record).
Proof. vm_compute. split; reflexivity. Qed.

Example exact_expectation_rejects_omitted_persistence_mutant :
  resolve_exact_continuation fixture_state fixture_state fixture_group fixture_capability
      fixture_wrong_persistence_expectation = None /\
  mutant_ignore_persistence fixture_wrong_persistence_expectation =
    Some (OrdinaryTarget fixture_first_path fixture_first_record).
Proof. vm_compute. split; reflexivity. Qed.

Example exact_expectation_rejects_omitted_peeks_mutant :
  resolve_exact_continuation fixture_state fixture_state fixture_group fixture_capability
      fixture_wrong_peeks_expectation = None /\
  mutant_ignore_peeks fixture_wrong_peeks_expectation =
    Some (OrdinaryTarget fixture_first_path fixture_first_record).
Proof. vm_compute. split; reflexivity. Qed.

(** 8. Boundary validation rejects malformed source bytes before an exact
    commit expectation can authorize the target. *)
Definition fixture_malformed_source_record : ContinuationRecord :=
  ContinuationRecordState [70] [256] false [].
Definition fixture_malformed_source_ordinary (group : GroupKey) :=
  if group_eq_dec group fixture_group then
    [OrdinaryContinuationState fixture_first_path fixture_malformed_source_record]
  else [].
Definition fixture_malformed_source_state : ContinuationAuthority :=
  ContinuationAuthorityState fixture_no_installed fixture_malformed_source_ordinary
    fixture_epoch fixture_resting_joins fixture_installed_joins.
Definition fixture_malformed_source_capability :=
  issue_ordinary_capability fixture_malformed_source_state fixture_group 0.
Definition mutant_skip_boundary_validation
    (state : ContinuationAuthority) (capability : ContinuationCapability)
    (expected : ContinuationExpectation) : option ResolvedTarget :=
  match resolve_capability state state fixture_group capability with
  | Some target =>
      if expectation_matches_record_dec expected (target_record target)
      then Some target else None
  | None => None
  end.

Example canonical_source_path_rejects_skipped_boundary_mutant :
  resolve_exact_continuation fixture_malformed_source_state
      fixture_malformed_source_state fixture_group
      fixture_malformed_source_capability
      (expectation_of_record fixture_malformed_source_record) = None /\
  mutant_skip_boundary_validation fixture_malformed_source_state
      fixture_malformed_source_capability
      (expectation_of_record fixture_malformed_source_record) =
    Some (OrdinaryTarget fixture_first_path fixture_malformed_source_record).
Proof. vm_compute. split; reflexivity. Qed.

(** 9. The exact ordinary stored address itself is boundary checked. *)
Definition fixture_malformed_path : Path := [256].
Definition fixture_malformed_path_ordinary (group : GroupKey) :=
  if group_eq_dec group fixture_group then
    [OrdinaryContinuationState fixture_malformed_path fixture_first_record]
  else [].
Definition fixture_malformed_path_state : ContinuationAuthority :=
  ContinuationAuthorityState fixture_no_installed fixture_malformed_path_ordinary
    fixture_epoch fixture_resting_joins fixture_installed_joins.
Definition fixture_malformed_path_capability :=
  issue_ordinary_capability fixture_malformed_path_state fixture_group 0.

Example canonical_ordinary_path_rejects_skipped_boundary_mutant :
  resolve_exact_continuation fixture_malformed_path_state
      fixture_malformed_path_state fixture_group fixture_malformed_path_capability
      fixture_first_expectation = None /\
  mutant_skip_boundary_validation fixture_malformed_path_state
      fixture_malformed_path_capability fixture_first_expectation =
    Some (OrdinaryTarget fixture_malformed_path fixture_first_record).
Proof. vm_compute. split; reflexivity. Qed.

(** 10. A fabricated replacement catalog can retarget an index, while the
    production resolver derives its catalog only from the observed authority. *)
Definition mutant_fabricated_catalog : option ResolvedTarget :=
  resolve_bound_ordinary fixture_state fixture_group [fixture_second_path] 0.

Example observed_catalog_rejects_fabricated_retarget_mutant :
  resolve_capability fixture_state fixture_state fixture_group fixture_capability =
      Some (OrdinaryTarget fixture_first_path fixture_first_record) /\
  mutant_fabricated_catalog =
      Some (OrdinaryTarget fixture_second_path fixture_second_record).
Proof. vm_compute. split; reflexivity. Qed.

(** 11. Persistent ordinary continuations remain authoritative after a COMM. *)
Definition mutant_remove_persistent : ContinuationAuthority :=
  remove_nonpersistent_ordinary fixture_persistent_state fixture_group
    fixture_first_path [fixture_join_channel].

Example persistence_gate_rejects_persistent_removal_mutant :
  commit_exact_continuation fixture_persistent_state fixture_persistent_state
      fixture_group
      fixture_persistent_capability fixture_persistent_expectation
      [fixture_join_channel] = Some fixture_persistent_state /\
  lookup_ordinary fixture_first_path
      (ordinary_continuations mutant_remove_persistent fixture_group) = None.
Proof. vm_compute. split; reflexivity. Qed.

(** 10. A successful nonpersistent removal advances exactly its group epoch;
    retaining the old epoch would leave earlier capabilities live. *)
Definition fixture_after_one_removal : option ContinuationAuthority :=
  commit_exact_continuation fixture_one_state fixture_one_state fixture_group
    fixture_one_capability fixture_first_expectation [fixture_join_channel].
Definition mutant_epoch_without_bump : EqualityEpoch.t :=
  continuation_epochs fixture_one_state fixture_group.

Example epoch_invalidation_rejects_no_bump_mutant :
  match fixture_after_one_removal with
  | Some after =>
      continuation_epochs after fixture_group <>
        continuation_epochs fixture_one_state fixture_group /\
      mutant_epoch_without_bump =
        continuation_epochs fixture_one_state fixture_group
  | None => False
  end.
Proof. vm_compute. split; [discriminate | reflexivity]. Qed.

(** 11. Removing the final ordinary record deletes only the exact resting join
    membership and never the installed join namespace. *)
Definition mutant_delete_whole_resting_join_channel : list GroupKey := [].
Definition mutant_delete_installed_join_membership : list GroupKey :=
  remove_join_group_exact fixture_group
    (fixture_installed_joins fixture_join_channel).

Example final_cleanup_rejects_installed_or_whole_channel_deletion_mutant :
  match fixture_after_one_removal with
  | Some after =>
      resting_joins after fixture_join_channel = [fixture_other_group] /\
      mutant_delete_whole_resting_join_channel = [] /\
      installed_joins after fixture_join_channel = [fixture_group] /\
      mutant_delete_installed_join_membership = []
  | None => False
  end.
Proof. vm_compute. repeat split; reflexivity. Qed.

(** 12. A non-final removal must not delete the group's resting join. *)
Definition fixture_after_nonfinal_removal : option ContinuationAuthority :=
  commit_exact_continuation fixture_state fixture_state fixture_group fixture_capability
    fixture_first_expectation [fixture_join_channel].
Definition mutant_premature_join_cleanup : list GroupKey :=
  remove_join_group_exact fixture_group
    (fixture_resting_joins fixture_join_channel).

Example final_only_cleanup_rejects_premature_cleanup_mutant :
  match fixture_after_nonfinal_removal with
  | Some after =>
      resting_joins after fixture_join_channel =
        [fixture_group; fixture_other_group] /\
      mutant_premature_join_cleanup = [fixture_other_group]
  | None => False
  end.
Proof. vm_compute. split; reflexivity. Qed.

(** 13. Exact-group mutation preserves a proper-prefix sibling group. *)
Definition fixture_prefix_ordinary (group : GroupKey)
    : list OrdinaryContinuation :=
  if group_eq_dec group fixture_prefix_group
  then [OrdinaryContinuationState [70] fixture_second_record]
  else fixture_one_ordinary group.
Definition fixture_prefix_state : ContinuationAuthority :=
  ContinuationAuthorityState fixture_no_installed fixture_prefix_ordinary
    fixture_epoch fixture_resting_joins fixture_installed_joins.
Definition fixture_prefix_capability : ContinuationCapability :=
  issue_ordinary_capability fixture_prefix_state fixture_group 0.
Definition mutant_prefix_wide_ordinary : ContinuationMap :=
  set_group
    (set_group (ordinary_continuations fixture_prefix_state)
      fixture_group [])
    fixture_prefix_group [].

Example exact_group_mutation_rejects_prefix_wide_mutant :
  match commit_exact_continuation fixture_prefix_state fixture_prefix_state fixture_group
      fixture_prefix_capability fixture_first_expectation [fixture_join_channel] with
  | Some after =>
      lookup_ordinary [70]
        (ordinary_continuations after fixture_prefix_group) =
      Some fixture_second_record /\
      lookup_ordinary [70] (mutant_prefix_wide_ordinary fixture_prefix_group) =
      None
  | None => False
  end.
Proof. vm_compute. split; reflexivity. Qed.

Inductive continuation_authority_mutant_id : Type :=
| MutantPrefixGroupAlias
| MutantIgnoreEpoch
| MutantLiveOrdinalRetarget
| MutantNegativeIndexCast
| MutantOmitSource
| MutantOmitPersistence
| MutantOmitPeeks
| MutantMalformedSourcePath
| MutantMalformedOrdinaryPath
| MutantFabricatedCatalog
| MutantRemovePersistent
| MutantKeepOldEpoch
| MutantDeleteInstalledJoin
| MutantPrematureJoinCleanup
| MutantPrefixWideGroupMutation.

Definition continuation_authority_mutant_catalog :=
  [MutantPrefixGroupAlias; MutantIgnoreEpoch; MutantLiveOrdinalRetarget;
   MutantNegativeIndexCast; MutantOmitSource; MutantOmitPersistence;
   MutantOmitPeeks; MutantMalformedSourcePath;
   MutantMalformedOrdinaryPath; MutantFabricatedCatalog;
   MutantRemovePersistent; MutantKeepOldEpoch; MutantDeleteInstalledJoin;
   MutantPrematureJoinCleanup; MutantPrefixWideGroupMutation].

Theorem continuation_authority_mutant_catalog_is_duplicate_free_and_exact :
  NoDup continuation_authority_mutant_catalog /\
  length continuation_authority_mutant_catalog = 15%nat.
Proof.
  split.
  - repeat (constructor; [simpl; intuition discriminate |]).
    constructor.
  - reflexivity.
Qed.

Print Assumptions exact_group_gate_rejects_prefix_alias_mutant.
Print Assumptions exact_epoch_gate_rejects_stale_epoch_mutant.
Print Assumptions bound_path_rejects_live_ordinal_retarget_mutant.
Print Assumptions signed_index_gate_rejects_negative_cast_mutant.
Print Assumptions exact_expectation_rejects_omitted_source_mutant.
Print Assumptions exact_expectation_rejects_omitted_persistence_mutant.
Print Assumptions exact_expectation_rejects_omitted_peeks_mutant.
Print Assumptions canonical_source_path_rejects_skipped_boundary_mutant.
Print Assumptions canonical_ordinary_path_rejects_skipped_boundary_mutant.
Print Assumptions observed_catalog_rejects_fabricated_retarget_mutant.
Print Assumptions persistence_gate_rejects_persistent_removal_mutant.
Print Assumptions epoch_invalidation_rejects_no_bump_mutant.
Print Assumptions final_cleanup_rejects_installed_or_whole_channel_deletion_mutant.
Print Assumptions final_only_cleanup_rejects_premature_cleanup_mutant.
Print Assumptions exact_group_mutation_rejects_prefix_wide_mutant.
Print Assumptions continuation_authority_mutant_catalog_is_duplicate_free_and_exact.

Module OrderedReceivePositionMutants.
Import OrderedReceivePositions.

Definition a : Path := [0; 1].
Definition b : Path := [0; 1; 0].
Definition p0 := PatternFieldsState [[10]] (Some [1]) i32_min.
Definition p1 := PatternFieldsState [[20]; [21]] None 0%Z.
Definition p2 := PatternFieldsState [[30]] (Some [2]) i32_max.
Definition receive := ReceiveFieldsState [a; b; a] [p0; p1; p2]
  [40] true [-1; 2; 7]%Z [50].

Example dropped_repeated_position_is_detected :
  positions [a; b; a] a <> [0].
Proof. vm_compute. discriminate. Qed.

Example duplicated_position_is_detected :
  positions [a; b; a] a <> [0; 2; 2].
Proof. vm_compute. discriminate. Qed.

Example reordered_positions_are_detected :
  positions [a; b; a] a <> [2; 0].
Proof. vm_compute. discriminate. Qed.

Example channel_order_changes_occurrences :
  positions [a; b; a] a = [0; 2] /\
  positions [a; a; b] a = [0; 1].
Proof. vm_compute. split; reflexivity. Qed.

Example prefix_related_channel_is_not_an_alias :
  positions [a; b; a] b = [1] /\ positions [a; b; a] [0] = [].
Proof. vm_compute. split; reflexivity. Qed.

Example shifted_pattern_is_detected :
  bind_at receive 2 <> Some (BindFieldsState a p0 true).
Proof. vm_compute. discriminate. Qed.

Example erased_terms_are_detected :
  bind_at receive 2 <>
  Some (BindFieldsState a (PatternFieldsState [] (Some [2]) i32_max) true).
Proof. vm_compute. discriminate. Qed.

Example erased_remainder_is_detected :
  bind_at receive 2 <>
  Some (BindFieldsState a (PatternFieldsState [[30]] None i32_max) true).
Proof. vm_compute. discriminate. Qed.

Example erased_signed_count_is_detected :
  bind_at receive 0 <>
  Some (BindFieldsState a (PatternFieldsState [[10]] (Some [1]) 0%Z) false).
Proof. vm_compute. discriminate. Qed.

Example shifted_peek_is_detected :
  bind_at receive 0 <> Some (BindFieldsState a p0 true) /\
  bind_at receive 2 = Some (BindFieldsState a p2 true).
Proof. vm_compute. split; [discriminate|reflexivity]. Qed.

Example raw_peek_normalization_changes_coarse_observation :
  map (effective_peek [-1; 7]%Z) [0; 1; 2] =
    map (effective_peek []) [0; 1; 2] /\
  coarse_peek [-1; 7]%Z <> coarse_peek [].
Proof. vm_compute. split; [reflexivity|discriminate]. Qed.

Example mismatched_arity_cannot_truncate_a_pattern :
  bind_at (ReceiveFieldsState [a; b; a] [p0; p1] [40] true [] [50]) 0 = None.
Proof. vm_compute. reflexivity. Qed.

Example replacing_first_source_is_detected :
  let first := ContinuationRecordState [1] [2] true [-1]%Z in
  let alias := ContinuationRecordState [1] [3] true [-1]%Z in
  first_identity [1] [first; alias] = Some first /\
  first_identity [1] [first; alias] <> Some alias.
Proof. vm_compute. split; [reflexivity|discriminate]. Qed.

Example source_identity_erasure_does_not_erase_source :
  source_free_fields (replace_source receive [99]) = source_free_fields receive /\
  replace_source receive [99] <> receive.
Proof. vm_compute. split; [reflexivity|discriminate]. Qed.

Example equal_installed_and_resting_records_are_distinct_occurrences :
  let record := ContinuationRecordState [1] [2] true [-1]%Z in
  target_record (InstalledTarget record) = target_record (OrdinaryTarget [3] record) /\
  (InstalledTarget record, 2) <> (OrdinaryTarget [3] record, 2).
Proof. split; [reflexivity|discriminate]. Qed.

Print Assumptions dropped_repeated_position_is_detected.
Print Assumptions duplicated_position_is_detected.
Print Assumptions reordered_positions_are_detected.
Print Assumptions channel_order_changes_occurrences.
Print Assumptions prefix_related_channel_is_not_an_alias.
Print Assumptions shifted_pattern_is_detected.
Print Assumptions erased_terms_are_detected.
Print Assumptions erased_remainder_is_detected.
Print Assumptions erased_signed_count_is_detected.
Print Assumptions shifted_peek_is_detected.
Print Assumptions raw_peek_normalization_changes_coarse_observation.
Print Assumptions mismatched_arity_cannot_truncate_a_pattern.
Print Assumptions replacing_first_source_is_detected.
Print Assumptions source_identity_erasure_does_not_erase_source.
Print Assumptions equal_installed_and_resting_records_are_distinct_occurrences.

Example checked_position_does_not_narrow_or_saturate :
  checked_position (-1)%Z = None /\
  checked_position 18446744073709551616%Z = None /\
  checked_position position_max = Some position_max /\
  checked_position 0%Z = Some 0%Z.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example fixed_width_position_does_not_reuse_signed_peek_range :
  checked_position 2147483648%Z = Some 2147483648%Z.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions checked_position_does_not_narrow_or_saturate.
Print Assumptions fixed_width_position_does_not_reuse_signed_peek_range.

End OrderedReceivePositionMutants.

Module InstalledJoinRestorationMutants.
Import InstalledJoinRestoration.

(** Restoring a snapshot by replaying installation returns two distinct
    installed joins reversed, so the restored store reads them in the
    opposite order from the snapshot. *)
Example install_replay_reverses_restored_installed_joins :
  read_store (restore_by_install [fixture_prefix_group; fixture_other_group]) =
    [fixture_other_group; fixture_prefix_group] /\
  read_store (restore_by_install [fixture_prefix_group; fixture_other_group]) <>
    read_store (replace_group [fixture_prefix_group; fixture_other_group]).
Proof. vm_compute. split; [reflexivity|discriminate]. Qed.

Print Assumptions install_replay_reverses_restored_installed_joins.

End InstalledJoinRestorationMutants.

Module OrderedReceiveByteViewMutants.
Import OrderedReceivePositions.
Import OrderedReceiveByteView.

Record AdmissionChecks : Type := AdmissionChecksState {
  count_check : Path -> list nat -> bool;
  persist_check : nat -> bool;
  tail_check : Path -> list nat -> bool;
  peeks_check : list Z -> bool
}.

Definition admit_with (checks : AdmissionChecks) (record : Path) (ends : list nat)
    : option ByteView :=
  if count_check checks record ends then
    if ends_ordered 8 ends then
      match nth_error record (body_finish_of ends) with
      | Some persist =>
          if persist_check checks persist then
            if tail_check checks record ends then
              if peeks_check checks (decode_words (peek_slice record ends)) then
                if Nat.leb (identity_finish_of ends) (length record) then
                  Some (ByteViewState record ends (arity_of ends)
                    (decode_words (peek_slice record ends)))
                else None
              else None
            else None
          else None
      | None => None
      end
    else None
  else None.

Definition production_checks : AdmissionChecks :=
  AdmissionChecksState
    (fun record ends => Z.eqb (Z.of_nat (length ends)) (header_count record + 2)%Z)
    (fun persist => Nat.leb persist 1)
    (fun record ends => tail_is_exact (body_finish_of ends)
      (tail_count record (body_finish_of ends)) (identity_finish_of ends))
    strictly_ascending.

Lemma production_checks_are_admit_record : forall record ends,
  admit_with production_checks record ends = admit_record record ends.
Proof. intros. reflexivity. Qed.

Definition fixture_prefix (pattern : PatternFields) : Path := concat (pattern_terms pattern).
Definition fixture_pattern : PatternFields := PatternFieldsState [[5]] None 3%Z.

Definition receive_with_peeks (peeks : list Z) : ReceiveFields :=
  ReceiveFieldsState [[1]] [fixture_pattern] [7] false peeks [9].

Definition one_receive : ReceiveFields := receive_with_peeks [].
Definition two_receive : ReceiveFields :=
  ReceiveFieldsState [[1]; [2]] [fixture_pattern; fixture_pattern] [7] false [] [9].
Definition empty_receive : ReceiveFields := ReceiveFieldsState [] [] [7] false [] [9].
Definition three_group : ChannelGroup := ChannelGroupState LegacyV1 [[1]; [2]; [3]].

Definition zip_admit_receive (group : ChannelGroup) (record : Path) (ends : list nat)
    : option ReceiveView :=
  match admit_record record ends with
  | Some view => Some (ReceiveViewState group view)
  | None => None
  end.

Example zip_truncating_arity_is_detected :
  admit_receive three_group (record_image fixture_prefix two_receive)
    (field_bounds fixture_prefix two_receive) = None /\
  option_map (fun view => view_bind view 2)
    (zip_admit_receive three_group (record_image fixture_prefix two_receive)
      (field_bounds fixture_prefix two_receive)) = Some None.
Proof. vm_compute. split; reflexivity. Qed.

Definition wrapping_tail_checks : AdmissionChecks :=
  AdmissionChecksState (count_check production_checks) (persist_check production_checks)
    (fun record ends =>
      Z.eqb ((Z.of_nat (body_finish_of ends) + 9 +
        4 * tail_count record (body_finish_of ends)) mod 18446744073709551616)%Z
        (Z.of_nat (identity_finish_of ends)) &&
      Z.leb (Z.of_nat (identity_finish_of ends)) word_max)
    (peeks_check production_checks).

Definition wrapping_tail_record : Path := le_bytes 8 0%Z ++ 0 :: le_bytes 8 (2 ^ 62)%Z.

Example wrapping_peek_tail_is_detected :
  tail_count wrapping_tail_record 8 = (2 ^ 62)%Z /\
  admit_record wrapping_tail_record [8; 17] = None /\
  option_map (fun view => length (view_peeks view))
    (admit_with wrapping_tail_checks wrapping_tail_record [8; 17]) = Some 0.
Proof. vm_compute. repeat split; reflexivity. Qed.

Definition narrowed_count_checks : AdmissionChecks :=
  AdmissionChecksState
    (fun record ends =>
      Z.eqb (Z.of_nat (length ends)) (header_count record mod 4294967296 + 2)%Z)
    (persist_check production_checks) (tail_check production_checks)
    (peeks_check production_checks).

Definition narrowed_count_record : Path :=
  le_bytes 8 4294967297%Z ++ skipn 8 (record_image fixture_prefix one_receive).

Example narrowed_count_alias_is_detected :
  header_count narrowed_count_record = 4294967297%Z /\
  admit_record narrowed_count_record (field_bounds fixture_prefix one_receive) = None /\
  admit_with narrowed_count_checks narrowed_count_record
    (field_bounds fixture_prefix one_receive) <> None.
Proof. vm_compute. split; [reflexivity|split; [reflexivity|discriminate]]. Qed.

Definition shifted_first_bound (ends : list nat) : list nat :=
  match ends with
  | first :: rest => first - 1 :: rest
  | [] => []
  end.

Definition trailing_free_counts (view : ByteView) : list Z :=
  map (fun bytes => i32_value (trailing_word bytes)) (view_patterns view).

Example shifted_capture_bound_is_detected :
  option_map view_patterns (admit_record (record_image fixture_prefix one_receive)
    (field_bounds fixture_prefix one_receive)) =
    Some (map (pattern_image fixture_prefix) (receive_patterns one_receive)) /\
  option_map trailing_free_counts (admit_record (record_image fixture_prefix one_receive)
    (field_bounds fixture_prefix one_receive)) = Some [pattern_free_count fixture_pattern] /\
  option_map trailing_free_counts (admit_record (record_image fixture_prefix one_receive)
    (shifted_first_bound (field_bounds fixture_prefix one_receive))) = Some [773%Z].
Proof. vm_compute. repeat split; reflexivity. Qed.

Definition source_bearing_identity (view : ByteView) : Path := view_record view.

Example source_bearing_identity_is_detected :
  view_identity (encoded_view fixture_prefix one_receive) =
    view_identity (encoded_view fixture_prefix (replace_source one_receive [8])) /\
  source_bearing_identity (encoded_view fixture_prefix one_receive) <>
    source_bearing_identity (encoded_view fixture_prefix (replace_source one_receive [8])).
Proof. vm_compute. split; [reflexivity|discriminate]. Qed.

Definition body_split_source (view : ByteView) : Path :=
  skipn (body_end view) (view_record view).

Example misplaced_source_split_is_detected :
  view_source (encoded_view fixture_prefix one_receive) = receive_source one_receive /\
  body_split_source (encoded_view fixture_prefix one_receive) <> receive_source one_receive.
Proof. vm_compute. split; [reflexivity|discriminate]. Qed.

Definition normalizing_peek_checks : AdmissionChecks :=
  AdmissionChecksState (count_check production_checks) (persist_check production_checks)
    (tail_check production_checks) (fun _ => true).

Example normalized_peek_tail_is_detected :
  admit_record (record_image fixture_prefix (receive_with_peeks [5; 1]%Z))
    (field_bounds fixture_prefix (receive_with_peeks [5; 1]%Z)) = None /\
  admit_record (record_image fixture_prefix (receive_with_peeks [3; 3]%Z))
    (field_bounds fixture_prefix (receive_with_peeks [3; 3]%Z)) = None /\
  option_map (fun view => strictly_ascending (view_peeks view))
    (admit_with normalizing_peek_checks
      (record_image fixture_prefix (receive_with_peeks [5; 1]%Z))
      (field_bounds fixture_prefix (receive_with_peeks [5; 1]%Z))) = Some false /\
  option_map (fun view => strictly_ascending (view_peeks view))
    (admit_with normalizing_peek_checks
      (record_image fixture_prefix (receive_with_peeks [3; 3]%Z))
      (field_bounds fixture_prefix (receive_with_peeks [3; 3]%Z))) = Some false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Definition wrapping_effective_peek (peeks : list Z) (position : Z) : bool :=
  existsb (Z.eqb (signed_word (position mod word_modulus))) peeks.

Example wrapping_signed_position_is_detected :
  view_effective_peek [2%Z] 4294967298%Z = false /\
  wrapping_effective_peek [2%Z] 4294967298%Z = true.
Proof. vm_compute. split; reflexivity. Qed.

Definition unframed_channel_at_is (channels : GroupKey) (query : Path) (position : nat)
    : bool :=
  match nth_error channels position with
  | Some image => if path_eq_dec (firstn (length query) image) query then true else false
  | None => false
  end.

Definition unframed_positions (channels : GroupKey) (query : Path) : list nat :=
  filter (unframed_channel_at_is channels query) (seq 0 (length channels)).

Example unframed_prefix_match_is_detected :
  view_positions (ChannelGroupState LegacyV1 [[1]; [1; 0]]) LegacyV1 [1] = Some [0] /\
  unframed_positions [[1]; [1; 0]] [1] = [0; 1].
Proof. vm_compute. split; reflexivity. Qed.

Definition profile_blind_positions (group : ChannelGroup) (profile : ChannelProfile)
    (query : Path) : option (list nat) :=
  Some (positions (group_frames group) (RhoPathV1Datum.encode_frame query)).

Example profile_blind_query_is_detected :
  view_positions (ChannelGroupState Rci2V2 [[1]]) LegacyV1 [1] = None /\
  profile_blind_positions (ChannelGroupState Rci2V2 [[1]]) LegacyV1 [1] = Some [0].
Proof. vm_compute. split; reflexivity. Qed.

Definition nonempty_checks : AdmissionChecks :=
  AdmissionChecksState
    (fun record ends => count_check production_checks record ends &&
      Z.ltb 0 (header_count record))
    (persist_check production_checks) (tail_check production_checks)
    (peeks_check production_checks).

Example universal_nonempty_rule_is_detected :
  option_map view_arity (admit_record (record_image fixture_prefix empty_receive)
    (field_bounds fixture_prefix empty_receive)) = Some 0 /\
  admit_with nonempty_checks (record_image fixture_prefix empty_receive)
    (field_bounds fixture_prefix empty_receive) = None.
Proof. vm_compute. split; reflexivity. Qed.

Definition lenient_persist_checks : AdmissionChecks :=
  AdmissionChecksState (count_check production_checks) (fun _ => true)
    (tail_check production_checks) (peeks_check production_checks).

Definition persist_two_record : Path :=
  firstn (body_finish_at fixture_prefix one_receive) (record_image fixture_prefix one_receive) ++
  2 :: skipn (S (body_finish_at fixture_prefix one_receive))
    (record_image fixture_prefix one_receive).

Definition nonzero_persistent (view : ByteView) : bool :=
  match nth_error (view_record view) (body_end view) with
  | Some 0 | None => false
  | Some _ => true
  end.

Example noncanonical_persist_is_detected :
  admit_record persist_two_record (field_bounds fixture_prefix one_receive) = None /\
  option_map nonzero_persistent
    (admit_with lenient_persist_checks persist_two_record
      (field_bounds fixture_prefix one_receive)) = Some true.
Proof. vm_compute. split; reflexivity. Qed.

Print Assumptions production_checks_are_admit_record.
Print Assumptions zip_truncating_arity_is_detected.
Print Assumptions wrapping_peek_tail_is_detected.
Print Assumptions narrowed_count_alias_is_detected.
Print Assumptions shifted_capture_bound_is_detected.
Print Assumptions source_bearing_identity_is_detected.
Print Assumptions misplaced_source_split_is_detected.
Print Assumptions normalized_peek_tail_is_detected.
Print Assumptions wrapping_signed_position_is_detected.
Print Assumptions unframed_prefix_match_is_detected.
Print Assumptions profile_blind_query_is_detected.
Print Assumptions universal_nonempty_rule_is_detected.
Print Assumptions noncanonical_persist_is_detected.

End OrderedReceiveByteViewMutants.

End TwoLevelRSpaceContinuationAuthorityMutants.
