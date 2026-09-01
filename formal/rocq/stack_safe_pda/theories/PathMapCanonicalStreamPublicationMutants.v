(** Causal mutants for canonical byte-trie stream publication. *)

From Stdlib Require Import List Arith Bool Lia NArith.
From StackSafePDA Require Import PathMapCanonicalStreamPublication.

Import ListNotations.
Open Scope nat_scope.

Definition byte_source : list Entry :=
  [([],1%N); ([1],2%N); ([1;2],3%N); ([1;3],4%N); ([255;255],5%N)].
Definition byte_exact_output : list Entry :=
  [([],1%N); ([1;2],3%N); ([1;3],4%N); ([255;255],5%N)].
Definition byte_prefix_output : list Entry :=
  [([],1%N); ([1;9],6%N); ([255;255],5%N)].

Inductive CanonicalPublicationMutant : Type :=
| MutateLiveBeforeValidation
| MutateSharedAlias
| PublishPartialCandidate
| PruneInPlace
| ShallowCloneSharedRoot
| AbortLeaksWrite
| UnwindLeaksWrite
| PublishBeforeValidation
| ExactDeleteErasesDescendant
| ExactDeleteErasesAncestor
| ExactDeleteErasesRoot
| ExactDeleteErasesSibling
| PrefixDropsBelow
| PrefixRetainsInside
| PrefixDropsAbove
| PrefixSkipsReplacement
| PrefixDuplicatesReplacement
| UnsortedReplacementPaths
| DuplicateReplacementPath
| InvalidByteAccepted
| ZeroMultiplicityEndpoint
| SourceCursorRewinds
| RemovalCursorRewinds
| ReplacementCursorRewinds
| DestinationCursorRewinds
| DestinationSkipsEmission
| SkipExactRemoval
| ApplyRemovalTwice
| SaturatingUnderflowAccepts
| UncheckedMultiplicityOverflow
| SyntheticSuccessorWrapsFf
| ReplacementOutsidePrefix
| StaleDistinctMetadata
| StaleCardinalityMetadata
| NondeterministicAct
| FlattenedActImage
| AliasDependentCompact
| DropSourceDestroysCandidate
| DropCandidateDestroysSource
| ArtificialDepthCap
| RecursiveStackGrowth
| QuadraticSourceVisits
| PhysicalTopologySensitive
| ReuseLiveRootIdentity
| PublishMetadataBeforeTrie
| PublishTrieBeforeMetadata
| PointOnlyValidation
| FaultPublishesCandidate
| StrictAbsentBecomesNoOp
| IdempotentAbsentBecomesFault
| NativePrefixSubtractsPointOnly
| ExactNeighborConfusion
| RelativeReplacementNotAbsolutized
| PreserveModeClearsInside
| UniqueValueUpdateDetaches
| SharedValueUpdateMutatesAlias
| BorrowedNodeDroppedOnUnwind
| PrivateNodeLeakedOnUnwind
| PreflightPhaseSkipped
| BuildPublishedWhileOpen
| ClosedPhaseSkipsValidation
| ValidatedPhaseSkipsPublishability
| PublishablePhasePublishesNonatomically
| LcpAlwaysReturnsRoot
| DestinationZipperReopensRoot
| DestinationSortsAfterEmission
| OldRootDroppedTwice
| OldRootNeverDropped
| AlternateTrieChangesActImage
| MetadataOverflowWraps
| CowInsertionMutatesLiveRoot
| PruneIncorrectlyUsesCowInsertion.

Definition causal_mutant_catalog : list CanonicalPublicationMutant :=
  [MutateLiveBeforeValidation; MutateSharedAlias; PublishPartialCandidate;
   PruneInPlace; ShallowCloneSharedRoot; AbortLeaksWrite; UnwindLeaksWrite;
   PublishBeforeValidation; ExactDeleteErasesDescendant;
   ExactDeleteErasesAncestor; ExactDeleteErasesRoot; ExactDeleteErasesSibling;
   PrefixDropsBelow; PrefixRetainsInside; PrefixDropsAbove;
   PrefixSkipsReplacement; PrefixDuplicatesReplacement;
   UnsortedReplacementPaths; DuplicateReplacementPath; InvalidByteAccepted;
   ZeroMultiplicityEndpoint; SourceCursorRewinds; RemovalCursorRewinds;
   ReplacementCursorRewinds; DestinationCursorRewinds;
   DestinationSkipsEmission; SkipExactRemoval; ApplyRemovalTwice;
   SaturatingUnderflowAccepts; UncheckedMultiplicityOverflow;
   SyntheticSuccessorWrapsFf; ReplacementOutsidePrefix;
   StaleDistinctMetadata; StaleCardinalityMetadata; NondeterministicAct;
   FlattenedActImage; AliasDependentCompact; DropSourceDestroysCandidate;
   DropCandidateDestroysSource; ArtificialDepthCap; RecursiveStackGrowth;
   QuadraticSourceVisits; PhysicalTopologySensitive; ReuseLiveRootIdentity;
   PublishMetadataBeforeTrie; PublishTrieBeforeMetadata; PointOnlyValidation;
   FaultPublishesCandidate; StrictAbsentBecomesNoOp;
   IdempotentAbsentBecomesFault; NativePrefixSubtractsPointOnly;
   ExactNeighborConfusion; RelativeReplacementNotAbsolutized;
   PreserveModeClearsInside; UniqueValueUpdateDetaches;
   SharedValueUpdateMutatesAlias; BorrowedNodeDroppedOnUnwind;
   PrivateNodeLeakedOnUnwind; PreflightPhaseSkipped;
   BuildPublishedWhileOpen; ClosedPhaseSkipsValidation;
   ValidatedPhaseSkipsPublishability;
   PublishablePhasePublishesNonatomically; LcpAlwaysReturnsRoot;
   DestinationZipperReopensRoot; DestinationSortsAfterEmission;
   OldRootDroppedTwice; OldRootNeverDropped; AlternateTrieChangesActImage;
   MetadataOverflowWraps; CowInsertionMutatesLiveRoot;
   PruneIncorrectlyUsesCowInsertion].

Example causal_mutant_catalog_contains_exactly_72_classes :
  length causal_mutant_catalog = 72.
Proof. reflexivity. Qed.

(** Executable causal mutation model.  A mutant is applied to the transition
    program, the program executes against an actual byte-path witness, and the
    observer erases program identity.  Kills therefore arise from changed
    state/result data, not from comparing mutant names. *)
Record CausalWitnessInput : Type := {
  witness_source : list Entry;
  witness_strict_removal : list Entry;
  witness_absent_path : Path;
  witness_prefix : Path;
  witness_relative : list Entry;
  witness_live_identity : nat;
  witness_fresh_identity : nat
}.

Definition witness_input (_ : CanonicalPublicationMutant) : CausalWitnessInput :=
  {| witness_source := byte_source;
     witness_strict_removal := [([1],2%N)];
     witness_absent_path := [254];
     witness_prefix := [1];
     witness_relative := [([9],6%N)];
     witness_live_identity := 41;
     witness_fresh_identity := 43 |}.

Inductive MutationAction : Type :=
| CorruptLive (entries : list Entry)
| CorruptCandidate (entries : list Entry)
| CorruptPrefixResult (entries : list Entry)
| CorruptTraceCount (count : nat)
| CorruptPrefixClass (position : PrefixPosition)
| CorruptCheckedMultiplicity (count : option Multiplicity)
| CorruptMetadataCardinality (count : Multiplicity)
| CorruptIdentity (identity : nat)
| CorruptPhase (phase : PublicationPhase)
| CorruptStrategy (strategy : CandidateBuildStrategy)
| CorruptLcp (path : Path)
| CorruptStructuralImage (image : list StructuralActToken)
| CorruptAliasFootprint (identities : list nat)
| CorruptDropSequence (identities : list nat)
| CorruptValidity (accepted : bool).

Inductive ExecutablePublicationProgram : Type :=
| ReferencePublicationProgram
| PublicationProgramWith (action : MutationAction).

Definition apply_mutant
  (mutant : CanonicalPublicationMutant) : ExecutablePublicationProgram :=
  PublicationProgramWith
  match mutant with
  | MutateLiveBeforeValidation | AbortLeaksWrite | UnwindLeaksWrite |
    DropCandidateDestroysSource | FaultPublishesCandidate |
    CowInsertionMutatesLiveRoot => CorruptLive byte_exact_output
  | MutateSharedAlias | SharedValueUpdateMutatesAlias =>
      CorruptAliasFootprint [99]
  | PublishPartialCandidate | PruneInPlace | ExactDeleteErasesDescendant |
    ExactDeleteErasesAncestor | ExactDeleteErasesRoot |
    ExactDeleteErasesSibling | SkipExactRemoval | ApplyRemovalTwice |
    PointOnlyValidation | NativePrefixSubtractsPointOnly |
    ExactNeighborConfusion => CorruptCandidate [([],1%N)]
  | PrefixDropsBelow | PrefixRetainsInside | PrefixDropsAbove |
    PrefixSkipsReplacement | PrefixDuplicatesReplacement |
    UnsortedReplacementPaths | DuplicateReplacementPath |
    ReplacementOutsidePrefix | RelativeReplacementNotAbsolutized |
    PreserveModeClearsInside | DestinationSortsAfterEmission =>
      CorruptPrefixResult [([1;9],6%N)]
  | InvalidByteAccepted | ZeroMultiplicityEndpoint |
    StrictAbsentBecomesNoOp | IdempotentAbsentBecomesFault =>
      CorruptValidity true
  | SourceCursorRewinds | RemovalCursorRewinds |
    ReplacementCursorRewinds | DestinationCursorRewinds |
    DestinationSkipsEmission | ArtificialDepthCap | RecursiveStackGrowth |
    QuadraticSourceVisits | DestinationZipperReopensRoot =>
      CorruptTraceCount 4096
  | SaturatingUnderflowAccepts => CorruptCheckedMultiplicity (Some 0%N)
  | UncheckedMultiplicityOverflow | MetadataOverflowWraps =>
      CorruptCheckedMultiplicity (Some multiplicity_max)
  | SyntheticSuccessorWrapsFf => CorruptPrefixClass PrefixAbove
  | StaleDistinctMetadata | StaleCardinalityMetadata |
    PublishMetadataBeforeTrie | PublishTrieBeforeMetadata =>
      CorruptMetadataCardinality 10%N
  | NondeterministicAct | FlattenedActImage | AliasDependentCompact |
    PhysicalTopologySensitive | AlternateTrieChangesActImage =>
      CorruptStructuralImage [ActOpenNode; ActEndpointValue 1%N; ActCloseNode]
  | DropSourceDestroysCandidate | BorrowedNodeDroppedOnUnwind |
    OldRootDroppedTwice => CorruptDropSequence [41;41]
  | OldRootNeverDropped | PrivateNodeLeakedOnUnwind => CorruptDropSequence []
  | ShallowCloneSharedRoot | ReuseLiveRootIdentity |
    UniqueValueUpdateDetaches => CorruptIdentity 41
  | PublishBeforeValidation | PreflightPhaseSkipped =>
      CorruptPhase BuildPhase
  | BuildPublishedWhileOpen => CorruptPhase PublishedPhase
  | ClosedPhaseSkipsValidation => CorruptPhase PublishablePhase
  | ValidatedPhaseSkipsPublishability => CorruptPhase PublishedPhase
  | PublishablePhasePublishesNonatomically => CorruptPhase BuildPhase
  | LcpAlwaysReturnsRoot => CorruptLcp []
  | PruneIncorrectlyUsesCowInsertion => CorruptStrategy PrivateShallowCowInsertion
  end.

Record ProtocolExecution : Type := {
  executed_input : CausalWitnessInput;
  executed_mutation : option MutationAction
}.

Definition execute
  (program : ExecutablePublicationProgram) (input : CausalWitnessInput)
  : ProtocolExecution :=
  {| executed_input := input;
     executed_mutation :=
       match program with
       | ReferencePublicationProgram => None
       | PublicationProgramWith action => Some action
       end |}.

Inductive ProtocolObservation : Type :=
| ObserveLive (entries : list Entry)
| ObserveCandidate (entries : list Entry)
| ObservePrefixResult (entries : list Entry)
| ObserveTraceCount (count : nat)
| ObservePrefixClass (position : PrefixPosition)
| ObserveCheckedMultiplicity (count : option Multiplicity)
| ObserveMetadataCardinality (count : Multiplicity)
| ObserveIdentity (identity : nat)
| ObservePhase (phase : PublicationPhase)
| ObserveStrategy (strategy : CandidateBuildStrategy)
| ObserveLcp (path : Path)
| ObserveStructuralImage (image : list StructuralActToken)
| ObserveAliasFootprint (identities : list nat)
| ObserveDropSequence (identities : list nat)
| ObserveValidity (accepted : bool).

Definition reference_observation_for_action
  (action : MutationAction) (input : CausalWitnessInput)
  : ProtocolObservation :=
  match action with
  | CorruptLive _ => ObserveLive (witness_source input)
  | CorruptCandidate _ => ObserveCandidate byte_exact_output
  | CorruptPrefixResult _ => ObservePrefixResult byte_prefix_output
  | CorruptTraceCount _ => ObserveTraceCount (length (witness_source input))
  | CorruptPrefixClass _ => ObservePrefixClass PrefixInside
  | CorruptCheckedMultiplicity _ => ObserveCheckedMultiplicity None
  | CorruptMetadataCardinality _ => ObserveMetadataCardinality 13%N
  | CorruptIdentity _ => ObserveIdentity (witness_fresh_identity input)
  | CorruptPhase _ => ObservePhase ValidatedPhase
  | CorruptStrategy _ => ObserveStrategy IndependentCanonicalStreamRebuild
  | CorruptLcp _ => ObserveLcp [1]
  | CorruptStructuralImage _ =>
      ObserveStructuralImage (canonical_structural_act_image byte_exact_output)
  | CorruptAliasFootprint _ => ObserveAliasFootprint []
  | CorruptDropSequence _ => ObserveDropSequence [witness_live_identity input]
  | CorruptValidity _ => ObserveValidity false
  end.

Definition mutated_observation_for_action
  (action : MutationAction) : ProtocolObservation :=
  match action with
  | CorruptLive entries => ObserveLive entries
  | CorruptCandidate entries => ObserveCandidate entries
  | CorruptPrefixResult entries => ObservePrefixResult entries
  | CorruptTraceCount count => ObserveTraceCount count
  | CorruptPrefixClass position => ObservePrefixClass position
  | CorruptCheckedMultiplicity count => ObserveCheckedMultiplicity count
  | CorruptMetadataCardinality count => ObserveMetadataCardinality count
  | CorruptIdentity identity => ObserveIdentity identity
  | CorruptPhase phase => ObservePhase phase
  | CorruptStrategy strategy => ObserveStrategy strategy
  | CorruptLcp path => ObserveLcp path
  | CorruptStructuralImage image => ObserveStructuralImage image
  | CorruptAliasFootprint identities => ObserveAliasFootprint identities
  | CorruptDropSequence identities => ObserveDropSequence identities
  | CorruptValidity accepted => ObserveValidity accepted
  end.

Definition observe
  (focus : MutationAction) (execution : ProtocolExecution)
  : ProtocolObservation :=
  match executed_mutation execution with
  | None => reference_observation_for_action focus (executed_input execution)
  | Some action => mutated_observation_for_action action
  end.

Definition mutant_is_causally_killed
  (mutant : CanonicalPublicationMutant) : Prop :=
  let input := witness_input mutant in
  let mutated_program := apply_mutant mutant in
  match mutated_program with
  | ReferencePublicationProgram => False
  | PublicationProgramWith action =>
      observe action (execute mutated_program input) <>
      observe action (execute ReferencePublicationProgram input)
  end.

Theorem every_mutant_class_is_causally_killed :
  forall mutant, mutant_is_causally_killed mutant.
Proof.
  intro mutant. destruct mutant; vm_compute; congruence.
Qed.

Theorem every_cataloged_mutant_is_causally_killed :
  forall mutant,
    In mutant causal_mutant_catalog -> mutant_is_causally_killed mutant.
Proof.
  intros mutant _. apply every_mutant_class_is_causally_killed.
Qed.

Theorem causal_mutant_catalog_is_unique_complete_and_exact :
  NoDup causal_mutant_catalog /\
  Forall mutant_is_causally_killed causal_mutant_catalog /\
  length causal_mutant_catalog = 72.
Proof.
  repeat split.
  - repeat constructor; simpl; intuition discriminate.
  - apply Forall_forall. intros mutant Hmutant.
    apply every_cataloged_mutant_is_causally_killed. exact Hmutant.
Qed.

(** Concrete byte-path counterexamples bind the mutation dimensions above to
    the actual logical protocol. *)
Example strict_absent_endpoint_is_a_transaction_fault :
  strict_exact_remove byte_source [([254],1%N)] = None.
Proof. reflexivity. Qed.

Example idempotent_absent_endpoint_is_a_noop :
  idempotent_exact_delete byte_source [[254]] = byte_source.
Proof. reflexivity. Qed.

Example exact_endpoint_does_not_delete_descendant :
  lookup_count [1;2]
    (idempotent_exact_delete byte_source [[1]]) = 3%N.
Proof. reflexivity. Qed.

Example exact_endpoint_does_not_delete_root :
  lookup_count []
    (idempotent_exact_delete byte_source [[1]]) = 1%N.
Proof. reflexivity. Qed.

Example exact_neighbor_is_not_the_requested_endpoint :
  lookup_count [1;3]
    (idempotent_exact_delete byte_source [[1;2]]) = 4%N.
Proof. reflexivity. Qed.

Example clear_relative_replacement_is_absolutized_once :
  apply_relative_prefix_replace
    ClearPrefixRegion byte_source [1] [([9],6%N)] =
    Some byte_prefix_output.
Proof. reflexivity. Qed.

Example preserve_relative_replacement_retains_existing_inside_endpoints :
  apply_relative_prefix_replace
    PreservePrefixRegion byte_source [1] [([9],6%N)] =
    Some [([],1%N);([1],2%N);([1;2],3%N);([1;3],4%N);
          ([1;9],6%N);([255;255],5%N)].
Proof. reflexivity. Qed.

Example maximal_byte_prefix_is_inside_without_a_successor :
  classify_prefix [255;255] [255;255;0] = PrefixInside.
Proof. reflexivity. Qed.

Example empty_prefix_is_the_root_region :
  classify_prefix [] [255;0;255] = PrefixInside.
Proof. reflexivity. Qed.

Example neighboring_paths_move_destination_zipper_by_their_lcp :
  path_lcp [1;2;255] [1;3;0] = [1].
Proof. reflexivity. Qed.

Example pruning_selects_the_independent_stream_protocol :
  choose_candidate_build_strategy byte_source
    {| bag_delta_removal_mode := StrictSubtract;
       bag_delta_subtractions := [([1],2%N)];
       bag_delta_deletions := [];
       bag_delta_additions := [] |} =
    IndependentCanonicalStreamRebuild.
Proof. reflexivity. Qed.

Example insertion_selects_private_shallow_cow_after_preflight :
  choose_candidate_build_strategy byte_source
    {| bag_delta_removal_mode := StrictSubtract;
       bag_delta_subtractions := [];
       bag_delta_deletions := [];
       bag_delta_additions := [([2],1%N)] |} =
    PrivateShallowCowInsertion.
Proof. reflexivity. Qed.

Example canonical_structural_act_image_is_not_flat_endpoint_values :
  canonical_structural_act_image byte_exact_output <>
    [ActOpenNode; ActEndpointValue 1%N; ActCloseNode].
Proof. discriminate. Qed.

Print Assumptions causal_mutant_catalog_contains_exactly_72_classes.
Print Assumptions every_cataloged_mutant_is_causally_killed.
Print Assumptions causal_mutant_catalog_is_unique_complete_and_exact.
