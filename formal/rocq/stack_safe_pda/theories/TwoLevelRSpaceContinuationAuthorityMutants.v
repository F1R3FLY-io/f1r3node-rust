From Stdlib Require Import Bool List Arith Lia ZArith.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority
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

End TwoLevelRSpaceContinuationAuthorityMutants.
