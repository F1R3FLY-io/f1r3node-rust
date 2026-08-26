------------------- MODULE RecoveryCommitteeTransition -------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
  \* @type: Set(Int);
  Validators,
  \* @type: Int;
  ProspectiveValidator,
  \* @type: Int;
  InitialLfbHeight,
  \* @type: Int;
  StakeOne,
  \* @type: Int;
  StakeTwo,
  \* @type: Int;
  StakeThree,
  \* @type: Int;
  ProspectiveStake,
  \* @type: Int;
  ThresholdNum,
  \* @type: Int;
  ThresholdDen,
  \* @type: Bool;
  UseFloorAuthorization,
  \* @type: Bool;
  UseFloorJustifications,
  \* @type: Str;
  SynchronyWeightSource,
  \* @type: Bool;
  RequireRegistrationBeforePromotion,
  \* @type: Bool;
  RequireAcceptedForRegistration,
  \* @type: Bool;
  RequireSerializedCacheMatch,
  \* @type: Bool;
  UseUnfilteredSequenceContext,
  \* @type: Bool;
  RequireCanonicalRootAdmission,
  \* @type: Bool;
  RequireSenderKeyedJustifications,
  \* @type: Bool;
  UseCanonicalGenesisForSlots,
  \* @type: Bool;
  RequireRegisteredSenderForInvalidLmm,
  \* @type: Bool;
  RequirePositiveForRegistration,
  \* @type: Bool;
  FilterInvalidLmmForFinality,
  \* @type: Bool;
  BackfillLegacyGenesisOnDuplicate

Universe == Validators \union {ProspectiveValidator}
NoValidator == 0
Phases == {"Current", "BondStaged", "Registered", "NextFloor"}
SynchronyWeightSources == {"Floor", "Head", "PostState"}
CanonicalGenesis == "ApprovedGenesis"
InvalidRootIds == {"ParentlessOrdinary", "CounterfeitGenesis"}
InvalidHeightZeroGenesis == "InvalidHeightZero"
NoGenesis == "None"
BlockIdentities == {CanonicalGenesis} \union InvalidRootIds
GenesisIdentities == {CanonicalGenesis, InvalidHeightZeroGenesis, NoGenesis}
GenesisIndexIdentities == BlockIdentities \union {NoGenesis}

ASSUME /\ Validators = {1, 2, 3}
       /\ ProspectiveValidator = 4
       /\ NoValidator \notin Universe
       /\ InitialLfbHeight \in Nat
       /\ StakeOne > 0
       /\ StakeTwo > 0
       /\ StakeThree > 0
       /\ ProspectiveStake > 0
       /\ ThresholdNum > 0
       /\ ThresholdDen > ThresholdNum
       /\ UseFloorAuthorization \in BOOLEAN
       /\ UseFloorJustifications \in BOOLEAN
       /\ SynchronyWeightSource \in SynchronyWeightSources
       /\ RequireRegistrationBeforePromotion \in BOOLEAN
       /\ RequireAcceptedForRegistration \in BOOLEAN
       /\ RequireSerializedCacheMatch \in BOOLEAN
       /\ UseUnfilteredSequenceContext \in BOOLEAN
       /\ RequireCanonicalRootAdmission \in BOOLEAN
       /\ RequireSenderKeyedJustifications \in BOOLEAN
       /\ UseCanonicalGenesisForSlots \in BOOLEAN
       /\ RequireRegisteredSenderForInvalidLmm \in BOOLEAN
       /\ RequirePositiveForRegistration \in BOOLEAN
       /\ FilterInvalidLmmForFinality \in BOOLEAN
       /\ BackfillLegacyGenesisOnDuplicate \in BOOLEAN

\* @typeAlias: transitionState = {
\*   phase: Str,
\*   lfbHeight: Int,
\*   floorCommittee: Set(Int),
\*   postStateBonds: Set(Int),
\*   positivePostStateBonds: Set(Int),
\*   serializedBondsCache: Set(Int),
\*   postStateAccepted: Bool,
\*   registeredValidators: Set(Int),
\*   positiveSlotOrigins: Set(Int),
\*   validatorGenesis: Int -> Str,
\*   lmmSlots: Set(Int),
\*   invalidLmmSlots: Set(Int),
\*   headCommittee: Set(Int),
\*   support: Set(Int),
\*   queuedCreator: Int,
\*   queuedFloorHeight: Int,
\*   unfilteredCreatorSeq: Int,
\*   validCreatorSeq: Int,
\*   packagedJustificationSeq: Int,
\*   packagedNextSeq: Int,
\*   started: Bool,
\*   validated: Bool,
\*   startedWithExactContext: Bool,
\*   validatedWithExactContext: Bool,
\*   approvedGenesisAdmitted: Bool,
\*   canonicalGenesisIndex: Str,
\*   duplicateApprovedSeen: Bool,
\*   admittedRoots: Set(Str),
\*   rejectedRootAttempts: Set(Str),
\*   childAccepted: Bool,
\*   childJustificationKey: Int,
\*   childCitedSender: Int,
\*   invalidHeightZeroSeen: Bool,
\*   invalidUnregisteredSeen: Bool,
\*   nonPositiveAttempted: Bool
\* };
module_typedefs == TRUE

VARIABLE
  \* @type: $transitionState;
  state

\* @type: <<$transitionState>>;
vars == <<state>>

AuthorizationCommittee ==
  IF UseFloorAuthorization THEN state.floorCommittee ELSE state.postStateBonds

JustificationCommittee ==
  IF UseFloorJustifications THEN state.floorCommittee ELSE state.headCommittee

SynchronyCommittee ==
  IF SynchronyWeightSource = "Floor" THEN state.floorCommittee
  ELSE IF SynchronyWeightSource = "Head" THEN state.headCommittee
  ELSE state.postStateBonds

RegistrationGenesis ==
  IF UseCanonicalGenesisForSlots \/ ~state.invalidHeightZeroSeen
  THEN CanonicalGenesis
  ELSE InvalidHeightZeroGenesis

StakeOf(committee) ==
    (IF 1 \in committee THEN StakeOne ELSE 0)
  + (IF 2 \in committee THEN StakeTwo ELSE 0)
  + (IF 3 \in committee THEN StakeThree ELSE 0)
  + (IF ProspectiveValidator \in committee THEN ProspectiveStake ELSE 0)

SupportedStake(committee) == StakeOf(state.support \intersect committee)

SupportedStakeFrom(support, committee) == StakeOf(support \intersect committee)

AdmitsWith(committee) ==
  ThresholdDen * SupportedStake(committee) >= ThresholdNum * StakeOf(committee)

AdmitsSupport(support, committee) ==
  ThresholdDen * SupportedStakeFrom(support, committee) >=
    ThresholdNum * StakeOf(committee)

FinalitySupport ==
  IF FilterInvalidLmmForFinality
  THEN state.support \ state.invalidLmmSlots
  ELSE state.support

ValidOnlyFinalityAdmits ==
  AdmitsSupport(state.support \ state.invalidLmmSlots, state.floorCommittee)

ConfiguredFinalityAdmits ==
  AdmitsSupport(FinalitySupport, state.floorCommittee)

FloorSynchronyAdmits == AdmitsWith(state.floorCommittee)
ConfiguredSynchronyAdmits == AdmitsWith(SynchronyCommittee)

CreatorJustificationVisible ==
  /\ state.queuedCreator # NoValidator
  /\ state.queuedCreator \in JustificationCommittee

CreatorSequenceAvailable ==
  /\ CreatorJustificationVisible
  /\ state.packagedNextSeq = state.packagedJustificationSeq + 1

QueuedValidationContext ==
  /\ state.queuedCreator \in AuthorizationCommittee
  /\ CreatorJustificationVisible
  /\ CreatorSequenceAvailable
  /\ ConfiguredSynchronyAdmits

QueuedIsCurrent ==
  /\ state.queuedCreator # NoValidator
  /\ state.queuedFloorHeight = state.lfbHeight

Init ==
  state =
    [phase |-> "Current",
     lfbHeight |-> InitialLfbHeight,
     floorCommittee |-> Validators,
     postStateBonds |-> Validators,
     positivePostStateBonds |-> Validators,
     serializedBondsCache |-> Validators,
     postStateAccepted |-> TRUE,
     registeredValidators |-> Validators,
     positiveSlotOrigins |-> Validators,
     validatorGenesis |->
       [validator \in Universe |->
          IF validator \in Validators THEN CanonicalGenesis ELSE NoGenesis],
     lmmSlots |-> Validators,
     invalidLmmSlots |-> {},
     headCommittee |-> Validators,
     support |-> {2, 3},
     queuedCreator |-> NoValidator,
     queuedFloorHeight |-> InitialLfbHeight,
     unfilteredCreatorSeq |-> 0,
     validCreatorSeq |-> 0,
     packagedJustificationSeq |-> 0,
     packagedNextSeq |-> 0,
     started |-> FALSE,
     validated |-> FALSE,
     startedWithExactContext |-> FALSE,
     validatedWithExactContext |-> FALSE,
     approvedGenesisAdmitted |-> FALSE,
     canonicalGenesisIndex |-> NoGenesis,
     duplicateApprovedSeen |-> FALSE,
     admittedRoots |-> {},
     rejectedRootAttempts |-> {},
     childAccepted |-> FALSE,
     childJustificationKey |-> NoValidator,
     childCitedSender |-> NoValidator,
     invalidHeightZeroSeen |-> FALSE,
     invalidUnregisteredSeen |-> FALSE,
     nonPositiveAttempted |-> FALSE]

AdmitApprovedGenesis ==
  /\ ~state.approvedGenesisAdmitted
  /\ state' =
       [state EXCEPT
         !.approvedGenesisAdmitted = TRUE,
         !.admittedRoots = @ \union {CanonicalGenesis},
         !.canonicalGenesisIndex = CanonicalGenesis]

LoadLegacyApprovedGenesis ==
  /\ ~state.approvedGenesisAdmitted
  /\ state' =
       [state EXCEPT
         !.approvedGenesisAdmitted = TRUE,
         !.admittedRoots = @ \union {CanonicalGenesis}]

InsertDuplicateApprovedGenesis ==
  /\ state.approvedGenesisAdmitted
  /\ ~state.duplicateApprovedSeen
  /\ state' =
       [state EXCEPT
         !.duplicateApprovedSeen = TRUE,
         !.canonicalGenesisIndex =
           IF BackfillLegacyGenesisOnDuplicate THEN CanonicalGenesis ELSE @]

ReceiveInvalidRoot(root) ==
  /\ root \in InvalidRootIds
  /\ root \notin state.rejectedRootAttempts
  /\ state' =
       [state EXCEPT
         !.rejectedRootAttempts = @ \union {root},
         !.admittedRoots =
           IF RequireCanonicalRootAdmission THEN @ ELSE @ \union {root},
         !.canonicalGenesisIndex =
           IF RequireCanonicalRootAdmission THEN @ ELSE root]

ReceiveInvalidRootAny ==
  \E root \in InvalidRootIds : ReceiveInvalidRoot(root)

ReceiveOrdinaryChild ==
  /\ state.approvedGenesisAdmitted
  /\ ~state.childAccepted
  /\ state' =
       [state EXCEPT
         !.childAccepted = TRUE,
         !.childCitedSender = 1,
         !.childJustificationKey =
           IF RequireSenderKeyedJustifications THEN 1 ELSE ProspectiveValidator]

RecordInvalidHeightZeroJunk ==
  /\ ~state.invalidHeightZeroSeen
  /\ state' =
       [state EXCEPT
         !.invalidHeightZeroSeen = TRUE,
         !.validatorGenesis =
           IF UseCanonicalGenesisForSlots
           THEN @
           ELSE [validator \in Universe |->
                   IF validator = ProspectiveValidator
                      /\ validator \in state.registeredValidators
                   THEN InvalidHeightZeroGenesis
                   ELSE state.validatorGenesis[validator]]]

RecordInvalidUnregisteredLatest ==
  /\ ~state.invalidUnregisteredSeen
  /\ ProspectiveValidator \notin state.registeredValidators
  /\ state' =
       [state EXCEPT
         !.invalidUnregisteredSeen = TRUE,
         !.lmmSlots =
           IF RequireRegisteredSenderForInvalidLmm
           THEN @
           ELSE @ \union {ProspectiveValidator}]

RecordInvalidFinalityLatest ==
  /\ 2 \in state.lmmSlots
  /\ 2 \notin state.invalidLmmSlots
  /\ state' = [state EXCEPT !.invalidLmmSlots = @ \union {2}]

TryRegisterNonPositiveBond ==
  /\ ~state.nonPositiveAttempted
  /\ state' =
       [state EXCEPT
         !.nonPositiveAttempted = TRUE,
         !.registeredValidators =
           IF RequirePositiveForRegistration
           THEN @
           ELSE @ \union {ProspectiveValidator},
         !.validatorGenesis =
           IF RequirePositiveForRegistration
           THEN @
           ELSE [validator \in Universe |->
                   IF validator = ProspectiveValidator
                   THEN CanonicalGenesis
                   ELSE state.validatorGenesis[validator]]]

StageProspectiveBond ==
  /\ state.phase = "Current"
  /\ state' =
       [state EXCEPT
         !.phase = "BondStaged",
         !.postStateBonds = @ \union {ProspectiveValidator},
         !.positivePostStateBonds = @ \union {ProspectiveValidator},
         !.serializedBondsCache = @ \union {ProspectiveValidator},
         !.postStateAccepted = TRUE]

StageInvalidProspectiveBond ==
  /\ state.phase = "Current"
  /\ state' =
       [state EXCEPT
         !.phase = "BondStaged",
         !.postStateBonds = @ \union {ProspectiveValidator},
         !.positivePostStateBonds = @ \union {ProspectiveValidator},
         !.serializedBondsCache = @ \union {ProspectiveValidator},
         !.postStateAccepted = FALSE]

StageMismatchedSerializedCache ==
  /\ state.phase = "Current"
  /\ state' =
       [state EXCEPT
         !.phase = "BondStaged",
         !.serializedBondsCache = @ \union {ProspectiveValidator},
         !.postStateAccepted = TRUE]

RejectUnusableBondCache ==
  /\ state.phase = "BondStaged"
  /\ (~state.postStateAccepted
       \/ state.serializedBondsCache # state.postStateBonds)
  /\ state' =
       [state EXCEPT
         !.phase = "Current",
         !.postStateBonds = state.floorCommittee,
         !.positivePostStateBonds = state.floorCommittee,
         !.serializedBondsCache = state.floorCommittee,
         !.postStateAccepted = TRUE]

RegisterPostStateBonds ==
  /\ state.phase = "BondStaged"
  /\ (~RequireAcceptedForRegistration \/ state.postStateAccepted)
  /\ (~RequireSerializedCacheMatch
       \/ state.serializedBondsCache = state.postStateBonds)
  /\ state.approvedGenesisAdmitted
  /\ state' =
       [state EXCEPT
         !.phase = "Registered",
         !.registeredValidators = @ \union state.positivePostStateBonds,
         !.positiveSlotOrigins = @ \union state.positivePostStateBonds,
         !.validatorGenesis =
           [validator \in Universe |->
             IF validator \in state.positivePostStateBonds
             THEN RegistrationGenesis
             ELSE state.validatorGenesis[validator]],
         !.lmmSlots = @ \union state.positivePostStateBonds]

PromotePostStateToFloor ==
  /\ state.phase \in {"BondStaged", "Registered"}
  /\ ~RequireRegistrationBeforePromotion \/ state.phase = "Registered"
  /\ state' =
       [state EXCEPT
         !.phase = "NextFloor",
         !.lfbHeight = @ + 1,
         !.floorCommittee = state.positivePostStateBonds,
         !.headCommittee = state.positivePostStateBonds]

QueueRecovery(creator) ==
  /\ creator \in state.floorCommittee
  /\ ConfiguredSynchronyAdmits
  /\ state.queuedCreator = NoValidator
  /\ state' =
       [state EXCEPT
         !.queuedCreator = creator,
         !.queuedFloorHeight = state.lfbHeight,
         !.packagedJustificationSeq = state.unfilteredCreatorSeq,
         !.packagedNextSeq =
           (IF UseUnfilteredSequenceContext
            THEN state.unfilteredCreatorSeq
            ELSE state.validCreatorSeq) + 1]

QueueRecoveryAny == \E creator \in Universe : QueueRecovery(creator)

RecordInvalidCreatorLatest ==
  /\ state.queuedCreator = NoValidator
  /\ state.unfilteredCreatorSeq = state.validCreatorSeq
  /\ state' = [state EXCEPT !.unfilteredCreatorSeq = @ + 1]

DivergeHeadFromQueuedCreator ==
  /\ state.queuedCreator # NoValidator
  /\ state.queuedCreator \in state.headCommittee
  /\ state' =
       [state EXCEPT !.headCommittee = @ \ {state.queuedCreator}]

DriftHeadSynchronyWeights ==
  /\ state.headCommittee = Validators
  /\ state' = [state EXCEPT !.headCommittee = Validators \ {3}]

StartQueuedRecovery ==
  /\ state.queuedCreator # NoValidator
  /\ ~state.started
  /\ QueuedIsCurrent
  /\ QueuedValidationContext
  /\ state' =
       [state EXCEPT
         !.started = TRUE,
         !.startedWithExactContext = TRUE]

ValidateStartedRecovery ==
  /\ state.started
  /\ ~state.validated
  /\ QueuedIsCurrent
  /\ QueuedValidationContext
  /\ state' =
       [state EXCEPT
         !.validated = TRUE,
         !.validatedWithExactContext = TRUE]

Next ==
  \/ AdmitApprovedGenesis
  \/ LoadLegacyApprovedGenesis
  \/ InsertDuplicateApprovedGenesis
  \/ ReceiveInvalidRootAny
  \/ ReceiveOrdinaryChild
  \/ RecordInvalidHeightZeroJunk
  \/ RecordInvalidUnregisteredLatest
  \/ RecordInvalidFinalityLatest
  \/ TryRegisterNonPositiveBond
  \/ StageProspectiveBond
  \/ StageInvalidProspectiveBond
  \/ StageMismatchedSerializedCache
  \/ RejectUnusableBondCache
  \/ RegisterPostStateBonds
  \/ PromotePostStateToFloor
  \/ QueueRecoveryAny
  \/ RecordInvalidCreatorLatest
  \/ DivergeHeadFromQueuedCreator
  \/ DriftHeadSynchronyWeights
  \/ StartQueuedRecovery
  \/ ValidateStartedRecovery

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(AdmitApprovedGenesis)
  /\ SF_vars(StageProspectiveBond)
  /\ WF_vars(RejectUnusableBondCache)
  /\ WF_vars(RegisterPostStateBonds)
  /\ WF_vars(PromotePostStateToFloor)

TypeOK ==
  /\ state.phase \in Phases
  /\ state.lfbHeight \in {InitialLfbHeight, InitialLfbHeight + 1}
  /\ state.floorCommittee \subseteq Universe
  /\ state.postStateBonds \subseteq Universe
  /\ state.positivePostStateBonds \subseteq state.postStateBonds
  /\ state.serializedBondsCache \subseteq Universe
  /\ state.postStateAccepted \in BOOLEAN
  /\ state.registeredValidators \subseteq Universe
  /\ state.positiveSlotOrigins \subseteq Universe
  /\ state.validatorGenesis \in [Universe -> GenesisIdentities]
  /\ state.lmmSlots \subseteq Universe
  /\ state.invalidLmmSlots \subseteq state.lmmSlots
  /\ state.headCommittee \subseteq Universe
  /\ state.support \subseteq Universe
  /\ state.queuedCreator \in Universe \union {NoValidator}
  /\ state.queuedFloorHeight \in {InitialLfbHeight, InitialLfbHeight + 1}
  /\ state.unfilteredCreatorSeq \in 0..1
  /\ state.validCreatorSeq = 0
  /\ state.packagedJustificationSeq \in 0..1
  /\ state.packagedNextSeq \in 0..2
  /\ state.started \in BOOLEAN
  /\ state.validated \in BOOLEAN
  /\ state.startedWithExactContext \in BOOLEAN
  /\ state.validatedWithExactContext \in BOOLEAN
  /\ state.approvedGenesisAdmitted \in BOOLEAN
  /\ state.canonicalGenesisIndex \in GenesisIndexIdentities
  /\ state.duplicateApprovedSeen \in BOOLEAN
  /\ state.admittedRoots \subseteq BlockIdentities
  /\ state.rejectedRootAttempts \subseteq InvalidRootIds
  /\ state.childAccepted \in BOOLEAN
  /\ state.childJustificationKey \in Universe \union {NoValidator}
  /\ state.childCitedSender \in Universe \union {NoValidator}
  /\ state.invalidHeightZeroSeen \in BOOLEAN
  /\ state.invalidUnregisteredSeen \in BOOLEAN
  /\ state.nonPositiveAttempted \in BOOLEAN

Inv_ApprovedGenesisIsSoleRoot ==
  /\ state.admittedRoots \subseteq {CanonicalGenesis}
  /\ (CanonicalGenesis \in state.admittedRoots) = state.approvedGenesisAdmitted
  /\ state.canonicalGenesisIndex \in {NoGenesis, CanonicalGenesis}

Inv_DuplicateApprovedBackfillsLegacyIndex ==
  (state.approvedGenesisAdmitted /\ state.duplicateApprovedSeen) =>
    state.canonicalGenesisIndex = CanonicalGenesis

Inv_JustificationKeysMatchCitedSenders ==
  /\ (state.approvedGenesisAdmitted =>
        /\ NoValidator = NoValidator
        /\ CanonicalGenesis \in state.admittedRoots)
  /\ (state.childAccepted =>
        /\ state.childJustificationKey = state.childCitedSender
        /\ state.childJustificationKey # NoValidator)

Inv_RegisteredSlotsUseCanonicalGenesis ==
  \A validator \in state.registeredValidators :
    state.validatorGenesis[validator] = CanonicalGenesis

Inv_InvalidUnregisteredSendersHaveNoLmmSlot ==
  state.lmmSlots \subseteq state.registeredValidators

Inv_InvalidLmmDoesNotContributeToFinality ==
  /\ FinalitySupport \intersect state.invalidLmmSlots = {}
  /\ ConfiguredFinalityAdmits = ValidOnlyFinalityAdmits

Inv_OnlyPositivePostStateBondsCreateSlots ==
  state.registeredValidators \ Validators \subseteq state.positiveSlotOrigins

Inv_SerializedBondsArePostStateCache ==
  state.phase \in {"Registered", "NextFloor"} =>
    state.serializedBondsCache = state.postStateBonds

Inv_CurrentBlockAuthorizationIsFloor ==
  UseFloorAuthorization => AuthorizationCommittee = state.floorCommittee

Inv_ExactRecoveryJustificationsAreFloor ==
  UseFloorJustifications => JustificationCommittee = state.floorCommittee

Inv_ProspectiveAuthorizationDeferred ==
  (ProspectiveValidator \in state.postStateBonds
    /\ ProspectiveValidator \notin state.floorCommittee)
    => ProspectiveValidator \notin AuthorizationCommittee

Inv_FloorValidatorsRegistered ==
  state.floorCommittee \subseteq state.registeredValidators

Inv_PostStateBondsRegisterBeforeNextFloor ==
  state.phase \in {"Registered", "NextFloor"} =>
    state.positivePostStateBonds \subseteq state.registeredValidators

Inv_InvalidPostStateDoesNotRegister ==
  ~state.postStateAccepted =>
    ProspectiveValidator \notin state.registeredValidators

Inv_LfbHeightChangesOnlyWithFloorPromotion ==
  (state.phase = "NextFloor")
    <=> (state.lfbHeight = InitialLfbHeight + 1)

Inv_NewValidatorEligibleAtNextFloor ==
  (state.phase = "NextFloor") =>
    /\ ProspectiveValidator \in state.floorCommittee
    /\ ProspectiveValidator \in state.registeredValidators
    /\ ProspectiveValidator \in AuthorizationCommittee

Inv_FloorSynchronyWeightsAuthoritative ==
  SynchronyWeightSource = "Floor" =>
    ConfiguredSynchronyAdmits = FloorSynchronyAdmits

Inv_SynchronyAdmissionMatchesFloor ==
  ConfiguredSynchronyAdmits = FloorSynchronyAdmits

Inv_QueuedRecoveryHasExactContext ==
  QueuedIsCurrent => QueuedValidationContext

Inv_PackagedSequenceUsesUnfilteredLmm ==
  (state.queuedCreator # NoValidator) =>
    /\ state.packagedJustificationSeq = state.unfilteredCreatorSeq
    /\ state.packagedNextSeq = state.packagedJustificationSeq + 1

Inv_StartedAndValidatedHaveExactContext ==
  /\ state.started => state.startedWithExactContext
  /\ state.validated => state.validatedWithExactContext

Live_ProspectiveValidatorEventuallyEligible ==
  <> /\ state.phase = "NextFloor"
     /\ ProspectiveValidator \in state.floorCommittee
     /\ ProspectiveValidator \in state.registeredValidators

=============================================================================
