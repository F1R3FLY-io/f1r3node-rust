From Stdlib Require Import Bool List Arith Lia NArith.
From StackSafePDA Require Import
  PathMapPhysicalOwnership
  PathMapReversePruneTeardown
  PathMapCanonicalStreamPublicationBase
  PathMapCanonicalStreamPublication
  TwoLevelRSpaceGuardedOwnership.

Import ListNotations.
Import TwoLevelRSpaceGuardedOwnership.

(** Executable causal negative controls for every obligation in the
    two-level guarded-ownership registry.  Each action corrupts one observed
    dimension of a concrete outer/inner teardown, Clone/CoW transition, or
    publication gate.  Kills compare executed observations, not mutation
    constructor identities. *)
Module TwoLevelRSpaceGuardedOwnershipMutants.

Module PO := PathMapPhysicalOwnership.
Module TD := PathMapReversePruneTeardown.
Module CB := PathMapCanonicalStreamPublicationBase.
Module CP := PathMapCanonicalStreamPublication.

Definition physical_fixture : PO.physical_anchor :=
  PO.PhysicalAnchor 7 PO.RootUninitialized [] [] 1 1 PO.WholeRootOwner.

Definition inner_fixture : TD.trie :=
  TD.Node (Some (TD.BagMultiplicity 5 3)) TD.FNil.

Definition outer_fixture : TD.trie :=
  TD.Node (Some (TD.CandidatePosition 11)) TD.FNil.

Definition final_plan : release_plan :=
  [ReleasePlanEntry 11 (Some inner_fixture)].

Definition outer_schedule : list ownership_event :=
  generated_teardown_schedule outer_fixture final_plan.

Inductive guarded_mutation_action : Type :=
| MutOuterCloneAddsSecondEvent
| MutOuterCloneDuplicatesRawHeader
| MutOuterCloneDeepClonesBag
| MutRealSlotCloneChangesIdentity
| MutRealSlotCloneDuplicatesRawStorage
| MutOuterNonfinalDropStartsTeardown
| MutInnerNonfinalDropStartsTeardown
| MutMakeMutLeavesFreshAnchorUnregistered
| MutValueUpdateChangesAlias
| MutStructuralInsertionSkipsPreflight
| MutStructuralRemovalSharesStorage
| MutAbortChangesLiveAuthority
| MutPublishRetiresOldHandleTwice
| MutAbortDisposesBorrowedNode
| MutReleaseDamagesSibling
| MutOuterSkipsRealSlotHandoff
| MutBarrierRoutedToRealDisposer
| MutFinalInnerTeardownRepeated
| MutFinalInnerTeardownSkipped
| MutInnerOccurrenceSequenceTruncated
| MutPhaseBBeginsBeforeValidation
| MutAbsentParentBarrierLeaked
| MutBarrierOverwritesRealParent
| MutBarrierEscapesPublicAuthority
| MutCanonicalEmptyStartsPda
| MutGeneratedScheduleDeadlocks
| MutStepDoesNotDecreaseWork
| MutReachableStackHasThirdFrame
| MutInnerFrameEntersInnerFrame
| MutTerminalReleaseRecursivelyDropsRaw
| MutDepthAddsNativeFrames
| MutZipperChangesAuthorityIdentity
| MutZipperUsesConfiguredDepthCap
| MutPublicationRunsWithLiveLease
| MutCompletionLeaksControl
| MutStepChangesSnapshotObservation
| MutTeardownPublishesRoot
| MutFailureResumes
| MutFailureChangesLiveObservation
| MutFailurePublishesAndRecursivelyDrops
| MutOuterBatchRefusalWrites
| MutOuterBatchUniqueRepublishes
| MutOuterBatchSharedWritesAliasedRoot
| MutOuterBatchDisposesUntouchedSlot
| MutOuterBatchInstallsStaleEntries.

Definition guarded_action_for
    (invariant : guarded_ownership_invariant_id) : guarded_mutation_action :=
  match invariant with
  | GuardInvOuterCloneOneEvent => MutOuterCloneAddsSecondEvent
  | GuardInvOuterCloneNoRawStorage => MutOuterCloneDuplicatesRawHeader
  | GuardInvOuterCloneNoInnerBagClone => MutOuterCloneDeepClonesBag
  | GuardInvRealSlotCloneArcOnly => MutRealSlotCloneChangesIdentity
  | GuardInvRealSlotCloneNoRawStorage => MutRealSlotCloneDuplicatesRawStorage
  | GuardInvOuterNonfinalDropNoRawStorage => MutOuterNonfinalDropStartsTeardown
  | GuardInvInnerNonfinalDropNoRawStorage => MutInnerNonfinalDropStartsTeardown
  | GuardInvMakeMutFreshAnchorRegistered => MutMakeMutLeavesFreshAnchorUnregistered
  | GuardInvValueUpdatePreservesAlias => MutValueUpdateChangesAlias
  | GuardInvStructuralInsertionPreflight => MutStructuralInsertionSkipsPreflight
  | GuardInvStructuralRemovalDisjoint => MutStructuralRemovalSharesStorage
  | GuardInvAbortPreservesAuthorityAliases => MutAbortChangesLiveAuthority
  | GuardInvPublishSwapsAndRetiresOnce => MutPublishRetiresOldHandleTwice
  | GuardInvAbortCannotDisposeBorrowed => MutAbortDisposesBorrowedNode
  | GuardInvSiblingReleasePreserved => MutReleaseDamagesSibling
  | GuardInvOuterRealSlotHandoffExact => MutOuterSkipsRealSlotHandoff
  | GuardInvBarrierNeverRoutedAsRealSlot => MutBarrierRoutedToRealDisposer
  | GuardInvFinalInnerAtMostOnce => MutFinalInnerTeardownRepeated
  | GuardInvFinalInnerExactlyOnce => MutFinalInnerTeardownSkipped
  | GuardInvInnerOccurrenceSequenceExact => MutInnerOccurrenceSequenceTruncated
  | GuardInvPhaseBAfterZeroValidation => MutPhaseBBeginsBeforeValidation
  | GuardInvAbsentParentBarrierPaired => MutAbsentParentBarrierLeaked
  | GuardInvRealParentNeverOverwritten => MutBarrierOverwritesRealParent
  | GuardInvPublicAuthorityHasNoBarrier => MutBarrierEscapesPublicAuthority
  | GuardInvCanonicalEmptyWorkFreeDrop => MutCanonicalEmptyStartsPda
  | GuardInvGeneratedScheduleCompletes => MutGeneratedScheduleDeadlocks
  | GuardInvEveryStepDecreasesWork => MutStepDoesNotDecreaseWork
  | GuardInvReachableStackAtMostTwo => MutReachableStackHasThirdFrame
  | GuardInvInnerCannotNestInner => MutInnerFrameEntersInnerFrame
  | GuardInvNoRecursiveRawDrop => MutTerminalReleaseRecursivelyDropsRaw
  | GuardInvArbitraryDepthSameBound => MutDepthAddsNativeFrames
  | GuardInvZipperIdentityStable => MutZipperChangesAuthorityIdentity
  | GuardInvZipperWorkTopologyDerived => MutZipperUsesConfiguredDepthCap
  | GuardInvPublicationRequiresNoLease => MutPublicationRunsWithLiveLease
  | GuardInvTerminalClosesControl => MutCompletionLeaksControl
  | GuardInvStepPreservesObservations => MutStepChangesSnapshotObservation
  | GuardInvTeardownNeverPublishes => MutTeardownPublishesRoot
  | GuardInvFailureTerminal => MutFailureResumes
  | GuardInvFailurePreservesObservations => MutFailureChangesLiveObservation
  | GuardInvFailureNoPartialOrRecursiveDrop =>
      MutFailurePublishesAndRecursivelyDrops
  | GuardInvOuterBatchRefusalPreservesWorld => MutOuterBatchRefusalWrites
  | GuardInvOuterBatchUniqueInPlace => MutOuterBatchUniqueRepublishes
  | GuardInvOuterBatchSharedForkPreservesAliases =>
      MutOuterBatchSharedWritesAliasedRoot
  | GuardInvOuterBatchDisposesOnlyReplacedSlots =>
      MutOuterBatchDisposesUntouchedSlot
  | GuardInvOuterBatchMatchesPublication => MutOuterBatchInstallsStaleEntries
  end.

Inductive guarded_observation : Type :=
| ObserveCount : nat -> guarded_observation
| ObserveCountPair : nat -> nat -> guarded_observation
| ObserveIdentifiers : list nat -> guarded_observation
| ObserveFrames : list control_frame -> guarded_observation
| ObserveFlag : bool -> guarded_observation.

(** Fixtures and observations of the in-place outer batch.  The live root
    (identity 17) is empty and the updated root (identity 19) holds one
    entry.  The unique world has no external alias; the shared world has a
    snapshot alias that holds root 17.  Each observation runs the batch, or
    its mutant, on these worlds. *)
Lemma outer_batch_empty_metadata_is_checked :
  CB.checked_metadata [] =
    Some {| CB.distinct_entries := 0; CB.total_cardinality := 0%N |}.
Proof. reflexivity. Qed.

Lemma outer_batch_one_entry_is_canonical :
  CB.canonical_entries [([1], 1%N)].
Proof.
  apply CB.CanonicalEntriesCons.
  - reflexivity.
  - reflexivity.
  - constructor.
  - constructor.
Qed.

Lemma outer_batch_one_entry_metadata_is_checked :
  CB.checked_metadata [([1], 1%N)] =
    Some {| CB.distinct_entries := 1; CB.total_cardinality := 1%N |}.
Proof. reflexivity. Qed.

Definition outer_batch_live_root : CP.AtomicRootAuthority :=
  {| CP.atomic_root_identity := 17;
     CP.atomic_root_entries := [];
     CP.atomic_root_image := CB.canonical_compressed_image [];
     CP.atomic_root_metadata :=
       {| CB.distinct_entries := 0; CB.total_cardinality := 0%N |};
     CP.atomic_root_entries_canonical := CB.CanonicalEntriesNil;
     CP.atomic_root_image_exact := eq_refl;
     CP.atomic_root_metadata_exact := outer_batch_empty_metadata_is_checked |}.

Definition outer_batch_updated_root : CP.AtomicRootAuthority :=
  {| CP.atomic_root_identity := 19;
     CP.atomic_root_entries := [([1], 1%N)];
     CP.atomic_root_image := CB.canonical_compressed_image [([1], 1%N)];
     CP.atomic_root_metadata :=
       {| CB.distinct_entries := 1; CB.total_cardinality := 1%N |};
     CP.atomic_root_entries_canonical := outer_batch_one_entry_is_canonical;
     CP.atomic_root_image_exact := eq_refl;
     CP.atomic_root_metadata_exact :=
       outer_batch_one_entry_metadata_is_checked |}.

Definition outer_batch_aliases (roots : list nat) : CP.ExternalAliasFootprint :=
  {| CP.external_alias_root_identities := roots;
     CP.external_alias_node_identities := [];
     CP.external_alias_value_identities := [] |}.

Definition outer_batch_world (roots : list nat) : CP.TransactionWorldV2 :=
  {| CP.world_live_authority := outer_batch_live_root;
     CP.world_external_aliases := outer_batch_aliases roots;
     CP.world_staged_candidate := None;
     CP.world_disposed_root_handles := [];
     CP.world_disposed_nodes := [];
     CP.world_disposed_values := [] |}.

Definition unique_outer_world : CP.TransactionWorldV2 := outer_batch_world [].

Definition shared_outer_world : CP.TransactionWorldV2 :=
  outer_batch_world [17].

Definition unique_outer_batch : CP.TransactionWorldV2 :=
  in_place_outer_batch unique_outer_world outer_batch_updated_root [23].

Definition outer_batch_live_identity (world : CP.TransactionWorldV2) : nat :=
  CP.atomic_root_identity (CP.world_live_authority world).

Definition observe_entries_and_disposed_values
    (world : CP.TransactionWorldV2) : guarded_observation :=
  ObserveCountPair
    (length (CP.atomic_root_entries (CP.world_live_authority world)))
    (length (CP.world_disposed_values world)).

Definition observe_identity_and_retired_roots
    (world : CP.TransactionWorldV2) : guarded_observation :=
  ObserveCountPair (outer_batch_live_identity world)
    (length (CP.world_disposed_root_handles world)).

Definition observe_identity_held_by_the_alias
    (world : CP.TransactionWorldV2) : guarded_observation :=
  ObserveFlag
    (existsb (Nat.eqb (outer_batch_live_identity world))
      (CP.external_alias_root_identities
        (CP.world_external_aliases shared_outer_world))).

Definition observe_disposed_values
    (world : CP.TransactionWorldV2) : guarded_observation :=
  ObserveIdentifiers (CP.world_disposed_values world).

Definition observe_published_entries
    (world : CP.TransactionWorldV2) : guarded_observation :=
  ObserveFlag
    (if CB.entries_eq_dec
          (CP.atomic_root_entries (CP.world_live_authority world))
          (CP.atomic_root_entries
            (CP.world_live_authority
              (CP.publish_world unique_outer_world outer_batch_updated_root)))
     then true else false).

Definition reference_observation
    (action : guarded_mutation_action) : guarded_observation :=
  match action with
  | MutOuterCloneAddsSecondEvent =>
      ObserveCount
        (length (TD.anchor_transition_trace (TD.immutable_wrapper_clone 0)))
  | MutOuterCloneDuplicatesRawHeader =>
      ObserveCount
        (PO.anchor_raw_headers (shallow_clone_anchor physical_fixture))
  | MutOuterCloneDeepClonesBag => ObserveCount 1
  | MutRealSlotCloneChangesIdentity =>
      ObserveCount
        (PO.anchor_identity (shallow_clone_anchor physical_fixture))
  | MutRealSlotCloneDuplicatesRawStorage =>
      ObserveCountPair
        (PO.anchor_raw_headers (shallow_clone_anchor physical_fixture))
        (length (PO.anchor_nodes (shallow_clone_anchor physical_fixture)))
  | MutOuterNonfinalDropStartsTeardown | MutInnerNonfinalDropStartsTeardown =>
      ObserveCount
        (TD.anchor_raw_headers
          (TD.anchor_transition_final (TD.nonlast_wrapper_drop 0)))
  | MutMakeMutLeavesFreshAnchorUnregistered =>
      ObserveCount
        (TD.anchor_raw_headers
          (TD.make_mut_fresh_final (TD.shared_arc_make_mut 0)))
  | MutValueUpdateChangesAlias => ObserveCount (PO.anchor_identity physical_fixture)
  | MutStructuralInsertionSkipsPreflight => ObserveFlag true
  | MutStructuralRemovalSharesStorage => ObserveFlag true
  | MutAbortChangesLiveAuthority => ObserveCount 17
  | MutPublishRetiresOldHandleTwice => ObserveIdentifiers [17]
  | MutAbortDisposesBorrowedNode => ObserveIdentifiers []
  | MutReleaseDamagesSibling => ObserveFlag true
  | MutOuterSkipsRealSlotHandoff =>
      ObserveIdentifiers (released_real_slot_ids outer_schedule)
  | MutBarrierRoutedToRealDisposer =>
      ObserveIdentifiers
        (released_real_slot_ids
          (expand_outer_event final_plan TD.InstallBarrier))
  | MutFinalInnerTeardownRepeated | MutFinalInnerTeardownSkipped =>
      ObserveIdentifiers
        (begun_inner_ids
          (expand_outer_trace final_plan
            (TD.terminal_two_phase_trace outer_fixture)))
  | MutInnerOccurrenceSequenceTruncated =>
      ObserveCount
        (length
          (inner_disposed_payloads 11 (final_inner_segment 11 inner_fixture)))
  | MutPhaseBBeginsBeforeValidation => ObserveFlag true
  | MutAbsentParentBarrierLeaked => ObserveCount (length (TD.prune_segment false))
  | MutBarrierOverwritesRealParent => ObserveCount (length (TD.prune_segment true))
  | MutBarrierEscapesPublicAuthority => ObserveCount 0
  | MutCanonicalEmptyStartsPda =>
      ObserveCount
        (length
          (TD.release_trace
            (TD.release_final_anchor
              (TD.verified_terminal_authority TD.FinalAnchorRootTeardown)
              TD.canonical_empty_root)))
  | MutGeneratedScheduleDeadlocks =>
      ObserveFlag
        (match drive_schedule [OuterFrame] outer_schedule with
         | Some TeardownComplete => true
         | _ => false
         end)
  | MutStepDoesNotDecreaseWork => ObserveCountPair 1 0
  | MutReachableStackHasThirdFrame => ObserveCount 2
  | MutInnerFrameEntersInnerFrame => ObserveCount 2
  | MutTerminalReleaseRecursivelyDropsRaw => ObserveCount 0
  | MutDepthAddsNativeFrames => ObserveCount 2
  | MutZipperChangesAuthorityIdentity => ObserveCount 41
  | MutZipperUsesConfiguredDepthCap => ObserveFlag false
  | MutPublicationRunsWithLiveLease => ObserveFrames []
  | MutCompletionLeaksControl => ObserveFrames []
  | MutStepChangesSnapshotObservation => ObserveIdentifiers [3; 5]
  | MutTeardownPublishesRoot => ObserveCount 0
  | MutFailureResumes => ObserveFlag false
  | MutFailureChangesLiveObservation => ObserveCount 29
  | MutFailurePublishesAndRecursivelyDrops => ObserveCountPair 0 0
  | MutOuterBatchRefusalWrites =>
      observe_entries_and_disposed_values
        (apply_outer_batch unique_outer_world None)
  | MutOuterBatchUniqueRepublishes =>
      observe_identity_and_retired_roots unique_outer_batch
  | MutOuterBatchSharedWritesAliasedRoot =>
      observe_identity_held_by_the_alias
        (in_place_outer_batch shared_outer_world outer_batch_updated_root [23])
  | MutOuterBatchDisposesUntouchedSlot =>
      observe_disposed_values unique_outer_batch
  | MutOuterBatchInstallsStaleEntries =>
      observe_published_entries unique_outer_batch
  end.

Definition mutated_observation
    (action : guarded_mutation_action) : guarded_observation :=
  match action with
  | MutOuterCloneAddsSecondEvent => ObserveCount 2
  | MutOuterCloneDuplicatesRawHeader => ObserveCount 2
  | MutOuterCloneDeepClonesBag => ObserveCount 2
  | MutRealSlotCloneChangesIdentity => ObserveCount 8
  | MutRealSlotCloneDuplicatesRawStorage => ObserveCountPair 2 1
  | MutOuterNonfinalDropStartsTeardown | MutInnerNonfinalDropStartsTeardown =>
      ObserveCount 0
  | MutMakeMutLeavesFreshAnchorUnregistered => ObserveCount 0
  | MutValueUpdateChangesAlias => ObserveCount 8
  | MutStructuralInsertionSkipsPreflight => ObserveFlag false
  | MutStructuralRemovalSharesStorage => ObserveFlag false
  | MutAbortChangesLiveAuthority => ObserveCount 19
  | MutPublishRetiresOldHandleTwice => ObserveIdentifiers [17; 17]
  | MutAbortDisposesBorrowedNode => ObserveIdentifiers [23]
  | MutReleaseDamagesSibling => ObserveFlag false
  | MutOuterSkipsRealSlotHandoff => ObserveIdentifiers []
  | MutBarrierRoutedToRealDisposer => ObserveIdentifiers [11]
  | MutFinalInnerTeardownRepeated => ObserveIdentifiers [11; 11]
  | MutFinalInnerTeardownSkipped => ObserveIdentifiers []
  | MutInnerOccurrenceSequenceTruncated => ObserveCount 0
  | MutPhaseBBeginsBeforeValidation => ObserveFlag false
  | MutAbsentParentBarrierLeaked => ObserveCount 2
  | MutBarrierOverwritesRealParent => ObserveCount 2
  | MutBarrierEscapesPublicAuthority => ObserveCount 1
  | MutCanonicalEmptyStartsPda => ObserveCount 1
  | MutGeneratedScheduleDeadlocks => ObserveFlag false
  | MutStepDoesNotDecreaseWork => ObserveCountPair 1 1
  | MutReachableStackHasThirdFrame => ObserveCount 3
  | MutInnerFrameEntersInnerFrame => ObserveCount 3
  | MutTerminalReleaseRecursivelyDropsRaw => ObserveCount 1
  | MutDepthAddsNativeFrames => ObserveCount 4096
  | MutZipperChangesAuthorityIdentity => ObserveCount 43
  | MutZipperUsesConfiguredDepthCap => ObserveFlag true
  | MutPublicationRunsWithLiveLease => ObserveFrames [OuterFrame]
  | MutCompletionLeaksControl => ObserveFrames [OuterFrame]
  | MutStepChangesSnapshotObservation => ObserveIdentifiers [3; 7]
  | MutTeardownPublishesRoot => ObserveCount 1
  | MutFailureResumes => ObserveFlag true
  | MutFailureChangesLiveObservation => ObserveCount 31
  | MutFailurePublishesAndRecursivelyDrops => ObserveCountPair 1 1
  | MutOuterBatchRefusalWrites =>
      observe_entries_and_disposed_values
        (apply_outer_batch unique_outer_world
          (Some (outer_batch_updated_root, [23])))
  | MutOuterBatchUniqueRepublishes =>
      observe_identity_and_retired_roots
        (CP.publish_world unique_outer_world outer_batch_updated_root)
  | MutOuterBatchSharedWritesAliasedRoot =>
      observe_identity_held_by_the_alias
        (in_place_outer_batch unique_outer_world outer_batch_updated_root [23])
  | MutOuterBatchDisposesUntouchedSlot =>
      observe_disposed_values
        (in_place_outer_batch unique_outer_world outer_batch_updated_root
          [23; 29])
  | MutOuterBatchInstallsStaleEntries =>
      observe_published_entries
        (in_place_outer_batch unique_outer_world outer_batch_live_root [23])
  end.

Definition guarded_ownership_mutant_is_causally_killed
    (invariant : guarded_ownership_invariant_id) : Prop :=
  let action := guarded_action_for invariant in
  mutated_observation action <> reference_observation action.

Theorem every_guarded_ownership_mutant_has_a_causal_witness : forall
    invariant,
  guarded_ownership_mutant_is_causally_killed invariant.
Proof.
  intro invariant. destruct invariant; vm_compute; discriminate.
Qed.

Theorem guarded_ownership_mutant_catalog_is_total_unique_and_exact :
  NoDup guarded_ownership_invariant_catalog /\
  Forall guarded_ownership_mutant_is_causally_killed
    guarded_ownership_invariant_catalog /\
  length guarded_ownership_invariant_catalog = 45.
Proof.
  destruct guarded_ownership_invariant_catalog_is_unique_complete_and_exact
    as [Hunique [_ Hlength]].
  split; [exact Hunique |]. split.
  - apply Forall_forall. intros invariant _.
    apply every_guarded_ownership_mutant_has_a_causal_witness.
  - exact Hlength.
Qed.

Print Assumptions every_guarded_ownership_mutant_has_a_causal_witness.
Print Assumptions guarded_ownership_mutant_catalog_is_total_unique_and_exact.

End TwoLevelRSpaceGuardedOwnershipMutants.
