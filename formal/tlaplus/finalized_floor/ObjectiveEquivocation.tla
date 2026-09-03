----------------------- MODULE ObjectiveEquivocation -----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
  \* @type: Set(Int);
  Replicas,
  \* @type: Set(Int);
  Validators,
  \* @type: Int;
  Equivocator,
  \* @type: Bool;
  UseCanonicalPairEvidence,
  \* @type: Bool;
  AcceptanceIgnoresLocalInvalidFlags,
  \* @type: Bool;
  RequireBothEvidenceDependencies,
  \* @type: Bool;
  ExcludeEquivocatorFromVoting,
  \* @type: Bool;
  PersistObjectiveEvidence,
  \* @type: Bool;
  SuppressUnaryFallbackForObjectivePair,
  \* @type: Bool;
  ScopeVoterExclusionToActiveIncarnation,
  \* @type: Bool;
  GroupByIncarnationBeforeCanonicalization,
  \* @type: Bool;
  ScopeUnarySuppressionToSiblingGroup,
  \* @type: Bool;
  UseBlockEpochAsBondIncarnation,
  \* @type: Bool;
  UseDeterministicUnaryMinimum,
  \* @type: Bool;
  UseCanonicalPreStateBondAuthority,
  \* @type: Bool;
  RepairEvidenceIndexOnDuplicateRetry,
  \* @type: Bool;
  UseFilteredFinalityVoteProjection

SiblingA == "A"
SiblingB == "B"
SiblingC == "C"
SiblingHashes == {SiblingA, SiblingB, SiblingC}
CurrentIncarnationHashes == {SiblingB, SiblingC}
NoHash == "None"
HashIdentities == SiblingHashes \union {NoHash}
NoEvidence == "NoEvidence"
EvidenceA == "A"
EvidenceB == "B"
EvidenceC == "C"
EvidenceAB == "AB"
EvidenceAC == "AC"
EvidenceBC == "BC"
EvidenceValues ==
  {NoEvidence, EvidenceA, EvidenceB, EvidenceC,
   EvidenceAB, EvidenceAC, EvidenceBC}
TargetIncarnation == 1
InvalidLmmVoter == 2
UnaryLow == "D"
UnaryHigh == "E"
UnaryHashes == {UnaryLow, UnaryHigh}
VerdictUnevaluated == "Unevaluated"
VerdictAccepted == "Accepted"
VerdictRejected == "Rejected"
VerdictValues == {VerdictUnevaluated, VerdictAccepted, VerdictRejected}

\* @type: (Str) => Int;
BondIncarnation(hash) == IF hash = SiblingA THEN 0 ELSE 1

\* Block epochs are attacker-authored and deliberately disagree with the
\* immutable PoS bond incarnation in this witness.
\* @type: (Str) => Int;
BlockEpoch(hash) == IF hash = SiblingB THEN 0 ELSE 1

\* @type: Set(Str);
CurrentGroupingHashes ==
  IF UseBlockEpochAsBondIncarnation
  THEN {SiblingA, SiblingC}
  ELSE CurrentIncarnationHashes

\* @type: Str;
CurrentGroupingEvidence ==
  IF UseBlockEpochAsBondIncarnation THEN EvidenceAC ELSE EvidenceBC

\* @type: (Str) => Str;
UnaryEvidence(hash) ==
  IF hash = SiblingA THEN EvidenceA
  ELSE IF hash = SiblingB THEN EvidenceB
  ELSE EvidenceC

\* @type: (Set(Str)) => Str;
RawCanonicalPair(hashes) ==
  IF {SiblingA, SiblingB} \subseteq hashes THEN EvidenceAB
  ELSE IF {SiblingA, SiblingC} \subseteq hashes THEN EvidenceAC
  ELSE IF {SiblingB, SiblingC} \subseteq hashes THEN EvidenceBC
  ELSE NoEvidence

\* @type: (Set(Str), Str) => Str;
EvidenceFor(hashes, incoming) ==
  LET withIncoming == hashes \union {incoming} IN
    IF GroupByIncarnationBeforeCanonicalization
    THEN
      IF CurrentGroupingHashes \subseteq withIncoming
      THEN IF UseCanonicalPairEvidence
           THEN CurrentGroupingEvidence
           ELSE UnaryEvidence(incoming)
      ELSE NoEvidence
    ELSE RawCanonicalPair(withIncoming)

\* @type: (Str) => Bool;
ActiveIncarnationPair(evidence) == evidence = EvidenceBC

\* @type: (Str) => Set(Str);
PairDependencies(evidence) ==
  IF evidence = EvidenceBC THEN CurrentIncarnationHashes
  ELSE IF evidence = EvidenceAB THEN {SiblingA, SiblingB}
  ELSE IF evidence = EvidenceAC THEN {SiblingA, SiblingC}
  ELSE {}

ASSUME /\ Replicas = {1, 2}
       /\ Validators = {1, 2, 3}
       /\ Equivocator = 3
       /\ Replicas \subseteq Validators
       /\ Equivocator \in Validators
       /\ Equivocator \notin Replicas
       /\ UseCanonicalPairEvidence \in BOOLEAN
       /\ AcceptanceIgnoresLocalInvalidFlags \in BOOLEAN
       /\ RequireBothEvidenceDependencies \in BOOLEAN
       /\ ExcludeEquivocatorFromVoting \in BOOLEAN
       /\ PersistObjectiveEvidence \in BOOLEAN
       /\ SuppressUnaryFallbackForObjectivePair \in BOOLEAN
       /\ ScopeVoterExclusionToActiveIncarnation \in BOOLEAN
       /\ GroupByIncarnationBeforeCanonicalization \in BOOLEAN
       /\ ScopeUnarySuppressionToSiblingGroup \in BOOLEAN
       /\ UseBlockEpochAsBondIncarnation \in BOOLEAN
       /\ UseDeterministicUnaryMinimum \in BOOLEAN
       /\ UseCanonicalPreStateBondAuthority \in BOOLEAN
       /\ RepairEvidenceIndexOnDuplicateRetry \in BOOLEAN
       /\ UseFilteredFinalityVoteProjection \in BOOLEAN

\* @typeAlias: equivocationState = {
\*   receivedCount: Int -> Int,
\*   pendingValidated: Int -> Str,
\*   receivedHashes: Int -> Set(Str),
\*   locallyAcceptedBlocks: Int -> Set(Str),
\*   localInvalid: Int -> Set(Str),
\*   structuralPairSeen: Int -> Bool,
\*   evidence: Int -> Str,
\*   durableEvidence: Int -> Str,
\*   evidenceAccepted: Int -> Bool,
\*   slashAccepted: Int -> Bool,
\*   unaryFallbackSuppressed: Int -> Bool,
\*   otherSequenceUnarySlashAccepted: Int -> Bool,
\*   unaryObserved: Int -> Set(Str),
\*   unarySelected: Int -> Str,
\*   candidateVerdict: Int -> Str,
\*   metadataPresent: Int -> Bool,
\*   evidenceIndexPresent: Int -> Bool,
\*   repairPending: Int -> Bool,
\*   crashedAfterMetadata: Int -> Bool,
\*   duplicateRetryCompleted: Int -> Bool,
\*   dependencies: Int -> Set(Str),
\*   voters: Int -> Set(Int),
\*   exactJustificationKeys: Int -> Set(Int),
\*   restarted: Set(Int),
\*   currentIncarnation: Int -> Int
\* };
module_typedefs == TRUE

VARIABLE
  \* @type: $equivocationState;
  state

\* @type: <<$equivocationState>>;
vars == <<state>>

\* Replica 1 sees a cross-incarnation pair first. Replica 2 sees the two
\* current-incarnation siblings first. Both eventually receive the same set.
\* @type: (Int, Int) => Str;
Arrival(replica, index) ==
  IF replica = 1
  THEN IF index = 1 THEN SiblingA
       ELSE IF index = 2 THEN SiblingB ELSE SiblingC
  ELSE IF index = 1 THEN SiblingC
       ELSE IF index = 2 THEN SiblingB ELSE SiblingA

\* @type: (Int, Str) => Bool;
EvidenceAcceptFor(replica, evidence) ==
  /\ ActiveIncarnationPair(evidence)
  /\ (AcceptanceIgnoresLocalInvalidFlags
       \/ state.localInvalid[replica] \intersect CurrentIncarnationHashes = {})

\* @type: (Set(Str), Str) => Set(Str);
DependenciesFor(hashes, incoming) ==
  IF RequireBothEvidenceDependencies
  THEN CurrentIncarnationHashes
  ELSE {incoming}

\* @type: Set(Int);
BaseFinalityVoters ==
  IF UseFilteredFinalityVoteProjection
  THEN Validators \ {InvalidLmmVoter}
  ELSE Validators

\* @type: (Set(Int), Bool, Bool) => Set(Int);
VotersAfterInsert(oldVoters, structuralPair, accepted) ==
  IF ScopeVoterExclusionToActiveIncarnation
  THEN
    IF accepted /\ ExcludeEquivocatorFromVoting
    THEN BaseFinalityVoters \ {Equivocator}
    ELSE BaseFinalityVoters
  ELSE
    IF structuralPair
    THEN oldVoters \ {Equivocator}
    ELSE oldVoters

BothCurrentIncarnationHashesReceived(replica) ==
  CurrentIncarnationHashes \subseteq state.receivedHashes[replica]

StructuralCrossIncarnationOnly(replica) ==
  /\ state.structuralPairSeen[replica]
  /\ ~BothCurrentIncarnationHashesReceived(replica)

AllDelivered(replica) == state.receivedCount[replica] = 3

Init ==
  state =
    [receivedCount |-> [replica \in Replicas |-> 0],
     pendingValidated |-> [replica \in Replicas |-> NoHash],
     receivedHashes |-> [replica \in Replicas |-> {}],
     locallyAcceptedBlocks |-> [replica \in Replicas |-> {}],
     localInvalid |->
       [replica \in Replicas |-> IF replica = 1 THEN {SiblingB} ELSE {}],
     structuralPairSeen |-> [replica \in Replicas |-> FALSE],
     evidence |-> [replica \in Replicas |-> NoEvidence],
     durableEvidence |-> [replica \in Replicas |-> NoEvidence],
     evidenceAccepted |-> [replica \in Replicas |-> FALSE],
     slashAccepted |-> [replica \in Replicas |-> FALSE],
     unaryFallbackSuppressed |-> [replica \in Replicas |-> FALSE],
     otherSequenceUnarySlashAccepted |->
       [replica \in Replicas |-> TRUE],
     unaryObserved |-> [replica \in Replicas |-> {}],
     unarySelected |-> [replica \in Replicas |-> NoHash],
     candidateVerdict |-> [replica \in Replicas |-> VerdictUnevaluated],
     metadataPresent |-> [replica \in Replicas |-> FALSE],
     evidenceIndexPresent |-> [replica \in Replicas |-> FALSE],
     repairPending |-> [replica \in Replicas |-> FALSE],
     crashedAfterMetadata |-> [replica \in Replicas |-> FALSE],
     duplicateRetryCompleted |-> [replica \in Replicas |-> FALSE],
     dependencies |-> [replica \in Replicas |-> {}],
     voters |-> [replica \in Replicas |-> BaseFinalityVoters],
     exactJustificationKeys |-> [replica \in Replicas |-> Validators],
     restarted |-> {},
     currentIncarnation |-> [replica \in Replicas |-> 1]]

ValidateNext(replica) ==
  /\ replica \in Replicas
  /\ state.receivedCount[replica] < 3
  /\ state.pendingValidated[replica] = NoHash
  /\ LET incoming == Arrival(replica, state.receivedCount[replica] + 1) IN
       state' =
         [state EXCEPT
           !.pendingValidated[replica] = incoming,
           !.locallyAcceptedBlocks[replica] = @ \union {incoming}]

ValidateAny == \E replica \in Replicas : ValidateNext(replica)

DurableInsertNext(replica) ==
  /\ replica \in Replicas
  /\ state.pendingValidated[replica] \in SiblingHashes
  /\ LET nextIndex == state.receivedCount[replica] + 1 IN
     LET incoming == state.pendingValidated[replica] IN
     LET withIncoming == state.receivedHashes[replica] \union {incoming} IN
     LET structuralPair == Cardinality(withIncoming) >= 2 IN
     LET selectedEvidence == EvidenceFor(state.receivedHashes[replica], incoming) IN
     LET accepted == EvidenceAcceptFor(replica, selectedEvidence) IN
     LET needsIndex == accepted /\ ~state.evidenceIndexPresent[replica] IN
     LET slash ==
       accepted \/
         (structuralPair
          /\ ~SuppressUnaryFallbackForObjectivePair
          /\ BondIncarnation(incoming) = TargetIncarnation) IN
       state' =
         [state EXCEPT
           !.receivedCount[replica] = nextIndex,
           !.pendingValidated[replica] = NoHash,
           !.receivedHashes[replica] = withIncoming,
           !.structuralPairSeen[replica] = structuralPair,
           !.evidence[replica] = selectedEvidence,
           !.metadataPresent[replica] = @ \/ accepted,
           !.repairPending[replica] = @ \/ needsIndex,
           !.evidenceAccepted[replica] = accepted,
           !.slashAccepted[replica] = slash,
           !.unaryFallbackSuppressed[replica] =
             structuralPair /\ SuppressUnaryFallbackForObjectivePair,
           !.otherSequenceUnarySlashAccepted[replica] =
             IF structuralPair /\ ~ScopeUnarySuppressionToSiblingGroup
             THEN FALSE
             ELSE @,
           !.dependencies[replica] =
             IF accepted
             THEN DependenciesFor(state.receivedHashes[replica], incoming)
             ELSE {},
           !.voters[replica] =
             VotersAfterInsert(@, structuralPair, accepted)]

DurableInsertAny == \E replica \in Replicas : DurableInsertNext(replica)

CompleteEvidenceIndex(replica) ==
  /\ replica \in Replicas
  /\ state.repairPending[replica]
  /\ ~state.crashedAfterMetadata[replica]
  /\ state' =
       [state EXCEPT
         !.durableEvidence[replica] =
           IF PersistObjectiveEvidence THEN state.evidence[replica]
           ELSE NoEvidence,
         !.evidenceIndexPresent[replica] = TRUE,
         !.repairPending[replica] = FALSE]

CompleteEvidenceIndexAny ==
  \E replica \in Replicas : CompleteEvidenceIndex(replica)

CrashAfterMetadata(replica) ==
  /\ replica \in Replicas
  /\ state.metadataPresent[replica]
  /\ state.repairPending[replica]
  /\ ~state.crashedAfterMetadata[replica]
  /\ state' =
       [state EXCEPT !.crashedAfterMetadata[replica] = TRUE]

CrashAfterMetadataAny ==
  \E replica \in Replicas : CrashAfterMetadata(replica)

RetryDuplicateInsert(replica) ==
  /\ replica \in Replicas
  /\ state.crashedAfterMetadata[replica]
  /\ state.repairPending[replica]
  /\ ~state.duplicateRetryCompleted[replica]
  /\ state' =
       [state EXCEPT
         !.durableEvidence[replica] =
           IF RepairEvidenceIndexOnDuplicateRetry
           THEN IF PersistObjectiveEvidence THEN state.evidence[replica]
                ELSE NoEvidence
           ELSE @,
         !.evidenceIndexPresent[replica] =
           IF RepairEvidenceIndexOnDuplicateRetry THEN TRUE ELSE @,
         !.repairPending[replica] =
           IF RepairEvidenceIndexOnDuplicateRetry THEN FALSE ELSE @,
         !.duplicateRetryCompleted[replica] = TRUE]

RetryDuplicateInsertAny ==
  \E replica \in Replicas : RetryDuplicateInsert(replica)

\* @type: (Int, Int) => Str;
UnaryArrival(replica, index) ==
  IF replica = 1
  THEN IF index = 1 THEN UnaryHigh ELSE UnaryLow
  ELSE IF index = 1 THEN UnaryLow ELSE UnaryHigh

ObserveUnaryEvidence(replica) ==
  /\ replica \in Replicas
  /\ Cardinality(state.unaryObserved[replica]) < 2
  /\ LET incoming ==
       UnaryArrival(replica, Cardinality(state.unaryObserved[replica]) + 1) IN
     LET withIncoming == state.unaryObserved[replica] \union {incoming} IN
       state' =
         [state EXCEPT
           !.unaryObserved[replica] = withIncoming,
           !.unarySelected[replica] =
             IF UseDeterministicUnaryMinimum
             THEN IF UnaryLow \in withIncoming THEN UnaryLow ELSE UnaryHigh
             ELSE IF @ = NoHash THEN incoming ELSE @]

ObserveUnaryEvidenceAny ==
  \E replica \in Replicas : ObserveUnaryEvidence(replica)

EvaluateSameBlockUnbondCandidate(replica) ==
  /\ replica \in Replicas
  /\ BothCurrentIncarnationHashesReceived(replica)
  /\ state.candidateVerdict[replica] = VerdictUnevaluated
  /\ state' =
       [state EXCEPT
         !.candidateVerdict[replica] =
           IF UseCanonicalPreStateBondAuthority
           THEN VerdictRejected
           ELSE IF state.localInvalid[replica]
                     \intersect CurrentIncarnationHashes = {}
                THEN VerdictAccepted
                ELSE VerdictRejected]

EvaluateSameBlockUnbondCandidateAny ==
  \E replica \in Replicas : EvaluateSameBlockUnbondCandidate(replica)

Restart(replica) ==
  /\ replica \in Replicas
  /\ AllDelivered(replica)
  /\ ~state.repairPending[replica]
  /\ replica \notin state.restarted
  /\ LET restored == state.durableEvidence[replica] IN
     LET accepted ==
       /\ state.currentIncarnation[replica] = 1
       /\ ActiveIncarnationPair(restored) IN
       state' =
         [state EXCEPT
           !.evidence[replica] = restored,
           !.evidenceAccepted[replica] = accepted,
           !.slashAccepted[replica] = accepted,
           !.unaryFallbackSuppressed[replica] =
             state.structuralPairSeen[replica]
               /\ SuppressUnaryFallbackForObjectivePair,
           !.dependencies[replica] =
             IF accepted THEN PairDependencies(restored) ELSE {},
           !.voters[replica] =
             IF ScopeVoterExclusionToActiveIncarnation
             THEN IF accepted /\ ExcludeEquivocatorFromVoting
                  THEN BaseFinalityVoters \ {Equivocator}
                  ELSE BaseFinalityVoters
             ELSE @,
           !.restarted = @ \union {replica}]

RestartAny == \E replica \in Replicas : Restart(replica)

AdvanceIncarnation(replica) ==
  /\ replica \in Replicas
  /\ state.currentIncarnation[replica] = 1
  /\ AllDelivered(replica)
  /\ state' =
       [state EXCEPT
         !.currentIncarnation[replica] = 2,
         !.evidenceAccepted[replica] = FALSE,
         !.slashAccepted[replica] = FALSE,
         !.dependencies[replica] = {},
         !.voters[replica] =
           IF ScopeVoterExclusionToActiveIncarnation
           THEN BaseFinalityVoters
           ELSE @]

AdvanceIncarnationAny == \E replica \in Replicas : AdvanceIncarnation(replica)

Next ==
  \/ ValidateAny
  \/ DurableInsertAny
  \/ CompleteEvidenceIndexAny
  \/ CrashAfterMetadataAny
  \/ RetryDuplicateInsertAny
  \/ ObserveUnaryEvidenceAny
  \/ EvaluateSameBlockUnbondCandidateAny
  \/ RestartAny
  \/ AdvanceIncarnationAny

Spec ==
  /\ Init
  /\ [][Next]_vars

TypeOK ==
  /\ state.receivedCount \in [Replicas -> 0..3]
  /\ state.pendingValidated \in [Replicas -> HashIdentities]
  /\ state.receivedHashes \in [Replicas -> SUBSET SiblingHashes]
  /\ state.locallyAcceptedBlocks \in [Replicas -> SUBSET SiblingHashes]
  /\ state.localInvalid \in [Replicas -> SUBSET SiblingHashes]
  /\ state.structuralPairSeen \in [Replicas -> BOOLEAN]
  /\ state.evidence \in [Replicas -> EvidenceValues]
  /\ state.durableEvidence \in [Replicas -> EvidenceValues]
  /\ state.evidenceAccepted \in [Replicas -> BOOLEAN]
  /\ state.slashAccepted \in [Replicas -> BOOLEAN]
  /\ state.unaryFallbackSuppressed \in [Replicas -> BOOLEAN]
  /\ state.otherSequenceUnarySlashAccepted \in [Replicas -> BOOLEAN]
  /\ state.unaryObserved \in [Replicas -> SUBSET UnaryHashes]
  /\ state.unarySelected \in [Replicas -> UnaryHashes \union {NoHash}]
  /\ state.candidateVerdict \in [Replicas -> VerdictValues]
  /\ state.metadataPresent \in [Replicas -> BOOLEAN]
  /\ state.evidenceIndexPresent \in [Replicas -> BOOLEAN]
  /\ state.repairPending \in [Replicas -> BOOLEAN]
  /\ state.crashedAfterMetadata \in [Replicas -> BOOLEAN]
  /\ state.duplicateRetryCompleted \in [Replicas -> BOOLEAN]
  /\ state.dependencies \in [Replicas -> SUBSET SiblingHashes]
  /\ state.voters \in [Replicas -> SUBSET Validators]
  /\ state.exactJustificationKeys \in [Replicas -> SUBSET Validators]
  /\ state.restarted \subseteq Replicas
  /\ state.currentIncarnation \in [Replicas -> 1..2]

Inv_OppositeOrderDeliversSameSiblings ==
  \A replica \in Replicas :
    AllDelivered(replica) => state.receivedHashes[replica] = SiblingHashes

Inv_StaleAcceptedSiblingsStillCreateObjectiveEvidence ==
  \A replica \in Replicas :
    (AllDelivered(replica) /\ state.currentIncarnation[replica] = 1) =>
      /\ state.locallyAcceptedBlocks[replica] = SiblingHashes
      /\ state.evidenceAccepted[replica]

Inv_ConflictDiscoveryOccursAtDurableInsertion ==
  \A replica \in Replicas :
    (~BothCurrentIncarnationHashesReceived(replica)) =>
      ~state.evidenceAccepted[replica]

Inv_GroupByIncarnationBeforeCanonicalization ==
  \A replica \in Replicas :
    (BothCurrentIncarnationHashesReceived(replica)
      /\ state.currentIncarnation[replica] = 1) =>
      /\ state.evidence[replica] = EvidenceBC
      /\ state.evidenceAccepted[replica]

Inv_AdversarialBlockEpochDoesNotDefineBondIncarnation ==
  \A replica \in Replicas :
    (BothCurrentIncarnationHashesReceived(replica)
      /\ state.currentIncarnation[replica] = 1) =>
      state.evidence[replica] = EvidenceBC

Inv_AcceptanceIsObjective ==
  \A replica \in Replicas :
    state.evidenceAccepted[replica] =>
      /\ state.currentIncarnation[replica] = 1
      /\ state.evidence[replica] = EvidenceBC
      /\ BothCurrentIncarnationHashesReceived(replica)

Inv_BothHashesAreDependencies ==
  \A replica \in Replicas :
    state.evidenceAccepted[replica] =>
      CurrentIncarnationHashes \subseteq state.dependencies[replica]

Inv_ActiveIncarnationEquivocatorCannotVote ==
  \A replica \in Replicas :
    state.evidenceAccepted[replica] =>
      Equivocator \notin state.voters[replica]

Inv_StructuralPairSuppressesUnaryFallback ==
  \A replica \in Replicas :
    state.structuralPairSeen[replica] =>
      state.unaryFallbackSuppressed[replica]

Inv_IndependentUnaryFaultAtOtherSequenceRemainsEligible ==
  \A replica \in Replicas :
    state.otherSequenceUnarySlashAccepted[replica]

Inv_UnaryEvidenceUsesDeterministicMinimum ==
  \A replica \in Replicas :
    state.unaryObserved[replica] = UnaryHashes =>
      state.unarySelected[replica] = UnaryLow

Inv_UnaryEvidenceConvergesAcrossArrivalOrders ==
  (state.unaryObserved[1] = UnaryHashes
    /\ state.unaryObserved[2] = UnaryHashes) =>
    state.unarySelected[1] = state.unarySelected[2]

Inv_SameBlockUnbondUsesCanonicalPreStateAuthority ==
  \A replica \in Replicas :
    state.candidateVerdict[replica] /= VerdictUnevaluated =>
      state.candidateVerdict[replica] = VerdictRejected

Inv_SameBlockUnbondVerdictsConverge ==
  (state.candidateVerdict[1] /= VerdictUnevaluated
    /\ state.candidateVerdict[2] /= VerdictUnevaluated) =>
    state.candidateVerdict[1] = state.candidateVerdict[2]

Inv_DuplicateRetryRepairsEvidenceIndex ==
  \A replica \in Replicas :
    state.duplicateRetryCompleted[replica] =>
      /\ state.evidenceIndexPresent[replica]
      /\ ~state.repairPending[replica]
      /\ state.durableEvidence[replica] = EvidenceBC

Inv_DurableEvidenceConvergesAfterRepair ==
  (state.evidenceIndexPresent[1]
    /\ state.evidenceIndexPresent[2]) =>
    state.durableEvidence[1] = state.durableEvidence[2]

Inv_ExactJustificationsUseFilteredFinalityVotes ==
  \A replica \in Replicas :
    /\ state.exactJustificationKeys[replica] = Validators
    /\ InvalidLmmVoter \notin state.voters[replica]

Inv_CrossIncarnationPairIsConsistentlyNonSlashable ==
  \A replica \in Replicas :
    StructuralCrossIncarnationOnly(replica) =>
      /\ ~state.evidenceAccepted[replica]
      /\ ~state.slashAccepted[replica]
      /\ Equivocator \in state.voters[replica]

Inv_IncarnationTransitionRestoresRawKey ==
  \A replica \in Replicas :
    state.currentIncarnation[replica] = 2 =>
      /\ ~state.evidenceAccepted[replica]
      /\ ~state.slashAccepted[replica]
      /\ Equivocator \in state.voters[replica]

Inv_RestartPreservesObjectiveEvidence ==
  \A replica \in state.restarted :
    /\ state.evidence[replica] = EvidenceBC
    /\ (state.currentIncarnation[replica] = 1 =>
          /\ state.evidenceAccepted[replica]
          /\ state.slashAccepted[replica]
          /\ Equivocator \notin state.voters[replica])
    /\ (state.currentIncarnation[replica] = 2 =>
          /\ ~state.evidenceAccepted[replica]
          /\ ~state.slashAccepted[replica]
          /\ Equivocator \in state.voters[replica])

Inv_ReplicaAgreementAfterAllDeliveries ==
  ((\A replica \in Replicas : AllDelivered(replica))
    /\ state.currentIncarnation[1] = state.currentIncarnation[2]) =>
    /\ state.evidence[1] = state.evidence[2]
    /\ state.evidenceAccepted[1] = state.evidenceAccepted[2]
    /\ state.slashAccepted[1] = state.slashAccepted[2]
    /\ state.dependencies[1] = state.dependencies[2]
    /\ state.voters[1] = state.voters[2]
    /\ (state.unaryObserved[1] = UnaryHashes
          /\ state.unaryObserved[2] = UnaryHashes =>
          state.unarySelected[1] = state.unarySelected[2])
    /\ (state.candidateVerdict[1] /= VerdictUnevaluated
          /\ state.candidateVerdict[2] /= VerdictUnevaluated =>
          state.candidateVerdict[1] = state.candidateVerdict[2])

=============================================================================
