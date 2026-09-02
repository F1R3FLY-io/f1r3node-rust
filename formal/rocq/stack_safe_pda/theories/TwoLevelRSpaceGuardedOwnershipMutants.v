From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import
  PathMapPhysicalOwnership
  PathMapReversePruneTeardown
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
| MutFailurePublishesAndRecursivelyDrops.

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
  end.

Inductive guarded_observation : Type :=
| ObserveCount : nat -> guarded_observation
| ObserveCountPair : nat -> nat -> guarded_observation
| ObserveIdentifiers : list nat -> guarded_observation
| ObserveFrames : list control_frame -> guarded_observation
| ObserveFlag : bool -> guarded_observation.

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
  length guarded_ownership_invariant_catalog = 40.
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
