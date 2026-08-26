------------------ MODULE ObjectiveEvidenceAuthorization ------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
  \* @type: Set(Int);
  Replicas,
  \* @type: Bool;
  GroupByEpochBeforeCanonicalization,
  \* @type: Bool;
  RequireCurrentActivationEpoch,
  \* @type: Bool;
  UseCanonicalPreStateGeneration,
  \* @type: Bool;
  UseCanonicalPreStateBond,
  \* @type: Bool;
  ScopeUnarySuppressionToFaultKey,
  \* @type: Bool;
  ActivateOnObjectiveGroups,
  \* @type: Bool;
  ShareProposerReceiverPredicate

OldGenerationCurrentEpoch == "A"
CurrentGenerationOldEpoch == "B"
CurrentEpochLow == "C"
CurrentEpochHigh == "D"
Hashes ==
  {OldGenerationCurrentEpoch, CurrentGenerationOldEpoch,
   CurrentEpochLow, CurrentEpochHigh}
CurrentGenerationGroup ==
  {CurrentGenerationOldEpoch, CurrentEpochLow, CurrentEpochHigh}
CurrentEpochGroup == {CurrentEpochLow, CurrentEpochHigh}
NoEvidence == "None"
EvidenceAB == "AB"
EvidenceAC == "AC"
EvidenceAD == "AD"
EvidenceBC == "BC"
EvidenceBD == "BD"
EvidenceCD == "CD"
EvidenceValues ==
  {NoEvidence, EvidenceAB, EvidenceAC, EvidenceAD,
   EvidenceBC, EvidenceBD, EvidenceCD}
TargetGeneration == 1
SnapshotGeneration == 0
TargetEpoch == 1
TargetSequence == 7

BondGeneration(hash) ==
  IF hash = OldGenerationCurrentEpoch THEN 0 ELSE TargetGeneration

ActivationEpoch(hash) ==
  IF hash = CurrentGenerationOldEpoch THEN 0 ELSE TargetEpoch

Sequence(hash) == TargetSequence

CanonicalPair(hashes) ==
  IF {OldGenerationCurrentEpoch, CurrentGenerationOldEpoch} \subseteq hashes
  THEN EvidenceAB
  ELSE IF {OldGenerationCurrentEpoch, CurrentEpochLow} \subseteq hashes
  THEN EvidenceAC
  ELSE IF {OldGenerationCurrentEpoch, CurrentEpochHigh} \subseteq hashes
  THEN EvidenceAD
  ELSE IF {CurrentGenerationOldEpoch, CurrentEpochLow} \subseteq hashes
  THEN EvidenceBC
  ELSE IF {CurrentGenerationOldEpoch, CurrentEpochHigh} \subseteq hashes
  THEN EvidenceBD
  ELSE IF CurrentEpochGroup \subseteq hashes
  THEN EvidenceCD
  ELSE NoEvidence

PairMembers(evidence) ==
  IF evidence = EvidenceAB
  THEN {OldGenerationCurrentEpoch, CurrentGenerationOldEpoch}
  ELSE IF evidence = EvidenceAC
  THEN {OldGenerationCurrentEpoch, CurrentEpochLow}
  ELSE IF evidence = EvidenceAD
  THEN {OldGenerationCurrentEpoch, CurrentEpochHigh}
  ELSE IF evidence = EvidenceBC
  THEN {CurrentGenerationOldEpoch, CurrentEpochLow}
  ELSE IF evidence = EvidenceBD
  THEN {CurrentGenerationOldEpoch, CurrentEpochHigh}
  ELSE IF evidence = EvidenceCD
  THEN CurrentEpochGroup
  ELSE {}

AuthorityGeneration ==
  IF UseCanonicalPreStateGeneration THEN TargetGeneration ELSE SnapshotGeneration

AuthorityBond == IF UseCanonicalPreStateBond THEN 100 ELSE 0

GenerationGroup(hashes, authorityGeneration) ==
  {hash \in hashes :
    BondGeneration(hash) = authorityGeneration /\
    Sequence(hash) = TargetSequence}

EpochGroup(hashes) ==
  {hash \in hashes : ActivationEpoch(hash) = TargetEpoch}

SelectEvidence(hashes, authorityGeneration) ==
  LET generationHashes == GenerationGroup(hashes, authorityGeneration) IN
    IF GroupByEpochBeforeCanonicalization
    THEN CanonicalPair(EpochGroup(generationHashes))
    ELSE CanonicalPair(generationHashes)

PairAuthorized(evidence, authorityGeneration, requireEpoch) ==
  LET members == PairMembers(evidence) IN
    /\ evidence /= NoEvidence
    /\ Cardinality(members) = 2
    /\ AuthorityBond > 0
    /\ \A hash \in members :
         /\ BondGeneration(hash) = authorityGeneration
         /\ Sequence(hash) = TargetSequence
         /\ (requireEpoch => ActivationEpoch(hash) = TargetEpoch)

ReceiverAuthorized(evidence) ==
  IF ShareProposerReceiverPredicate
  THEN PairAuthorized(
         evidence,
         AuthorityGeneration,
         RequireCurrentActivationEpoch)
  ELSE PairAuthorized(evidence, SnapshotGeneration, FALSE)

StructuralObjectiveGroupPresent(hashes) ==
  Cardinality(GenerationGroup(hashes, TargetGeneration)) >= 2

Arrival(replica, index) ==
  IF replica = 1
  THEN IF index = 1 THEN OldGenerationCurrentEpoch
       ELSE IF index = 2 THEN CurrentGenerationOldEpoch
       ELSE IF index = 3 THEN CurrentEpochLow ELSE CurrentEpochHigh
  ELSE IF index = 1 THEN CurrentEpochHigh
       ELSE IF index = 2 THEN CurrentEpochLow
       ELSE IF index = 3 THEN CurrentGenerationOldEpoch
       ELSE OldGenerationCurrentEpoch

\* @typeAlias: objectiveAuthorizationState = {
\*   received: Int -> Set(Str),
\*   delivered: Int -> Int,
\*   proposerEvaluated: Int -> Bool,
\*   proposedEvidence: Int -> Str,
\*   receiverEvaluated: Int -> Bool,
\*   receiverAccepted: Int -> Bool,
\*   tamperedReceiverEvaluated: Int -> Bool,
\*   tamperedReceiverAccepted: Int -> Bool,
\*   unaryEvaluated: Int -> Bool,
\*   sameKeyUnaryAccepted: Int -> Bool,
\*   independentUnaryAccepted: Int -> Bool
\* };
module_typedefs == TRUE

VARIABLE
  \* @type: $objectiveAuthorizationState;
  state
\* @type: <<$objectiveAuthorizationState>>;
vars == <<state>>

Init ==
  state =
    [received |-> [replica \in Replicas |-> {}],
     delivered |-> [replica \in Replicas |-> 0],
     proposerEvaluated |-> [replica \in Replicas |-> FALSE],
     proposedEvidence |-> [replica \in Replicas |-> NoEvidence],
     receiverEvaluated |-> [replica \in Replicas |-> FALSE],
     receiverAccepted |-> [replica \in Replicas |-> FALSE],
     tamperedReceiverEvaluated |-> [replica \in Replicas |-> FALSE],
     tamperedReceiverAccepted |-> [replica \in Replicas |-> FALSE],
     unaryEvaluated |-> [replica \in Replicas |-> FALSE],
     sameKeyUnaryAccepted |-> [replica \in Replicas |-> FALSE],
     independentUnaryAccepted |-> [replica \in Replicas |-> FALSE]]

Deliver(replica) ==
  /\ replica \in Replicas
  /\ state.delivered[replica] < 4
  /\ LET incoming == Arrival(replica, state.delivered[replica] + 1) IN
       state' =
         [state EXCEPT
           !.received[replica] = @ \union {incoming},
           !.delivered[replica] = @ + 1]

EvaluateProposer(replica) ==
  /\ replica \in Replicas
  /\ state.delivered[replica] = 4
  /\ ~state.proposerEvaluated[replica]
  /\ LET structural == StructuralObjectiveGroupPresent(state.received[replica]) IN
     LET activated == ActivateOnObjectiveGroups /\ structural IN
     LET selected == SelectEvidence(state.received[replica], AuthorityGeneration) IN
     LET authorized ==
       activated /\ PairAuthorized(
         selected,
         AuthorityGeneration,
         RequireCurrentActivationEpoch) IN
       state' =
         [state EXCEPT
           !.proposerEvaluated[replica] = TRUE,
           !.proposedEvidence[replica] =
             IF authorized THEN selected ELSE NoEvidence]

EvaluateReceiver(replica) ==
  /\ replica \in Replicas
  /\ state.proposerEvaluated[replica]
  /\ ~state.receiverEvaluated[replica]
  /\ state' =
       [state EXCEPT
         !.receiverEvaluated[replica] = TRUE,
         !.receiverAccepted[replica] =
           ReceiverAuthorized(state.proposedEvidence[replica])]

EvaluateTamperedCrossEpochPair(replica) ==
  /\ replica \in Replicas
  /\ state.delivered[replica] = 4
  /\ ~state.tamperedReceiverEvaluated[replica]
  /\ state' =
       [state EXCEPT
         !.tamperedReceiverEvaluated[replica] = TRUE,
         !.tamperedReceiverAccepted[replica] = ReceiverAuthorized(EvidenceBC)]

EvaluateUnarySuppression(replica) ==
  /\ replica \in Replicas
  /\ state.delivered[replica] = 4
  /\ ~state.unaryEvaluated[replica]
  /\ LET structural == StructuralObjectiveGroupPresent(state.received[replica]) IN
       state' =
         [state EXCEPT
           !.unaryEvaluated[replica] = TRUE,
           !.sameKeyUnaryAccepted[replica] = ~structural,
           !.independentUnaryAccepted[replica] =
             IF structural /\ ~ScopeUnarySuppressionToFaultKey
             THEN FALSE
             ELSE TRUE]

Next ==
  \/ \E replica \in Replicas : Deliver(replica)
  \/ \E replica \in Replicas : EvaluateProposer(replica)
  \/ \E replica \in Replicas : EvaluateReceiver(replica)
  \/ \E replica \in Replicas : EvaluateTamperedCrossEpochPair(replica)
  \/ \E replica \in Replicas : EvaluateUnarySuppression(replica)

Spec == /\ Init /\ [][Next]_vars

TypeOK ==
  /\ state.received \in [Replicas -> SUBSET Hashes]
  /\ state.delivered \in [Replicas -> 0..4]
  /\ state.proposerEvaluated \in [Replicas -> BOOLEAN]
  /\ state.proposedEvidence \in [Replicas -> EvidenceValues]
  /\ state.receiverEvaluated \in [Replicas -> BOOLEAN]
  /\ state.receiverAccepted \in [Replicas -> BOOLEAN]
  /\ state.tamperedReceiverEvaluated \in [Replicas -> BOOLEAN]
  /\ state.tamperedReceiverAccepted \in [Replicas -> BOOLEAN]
  /\ state.unaryEvaluated \in [Replicas -> BOOLEAN]
  /\ state.sameKeyUnaryAccepted \in [Replicas -> BOOLEAN]
  /\ state.independentUnaryAccepted \in [Replicas -> BOOLEAN]

Inv_EpochGroupingPrecedesCanonicalization ==
  \A replica \in Replicas :
    state.proposerEvaluated[replica] =>
      state.proposedEvidence[replica] = EvidenceCD

Inv_CrossEpochPairCannotAuthorize ==
  \A replica \in Replicas :
    state.tamperedReceiverEvaluated[replica] =>
      ~state.tamperedReceiverAccepted[replica]

Inv_ProposerReceiverParity ==
  \A replica \in Replicas :
    state.receiverEvaluated[replica] =>
      state.receiverAccepted[replica] =
        (state.proposedEvidence[replica] /= NoEvidence)

Inv_SameKeyUnarySuppressed ==
  \A replica \in Replicas :
    state.unaryEvaluated[replica] => ~state.sameKeyUnaryAccepted[replica]

Inv_IndependentUnaryPreserved ==
  \A replica \in Replicas :
    state.unaryEvaluated[replica] => state.independentUnaryAccepted[replica]

Inv_ArrivalOrderAgreement ==
  (\A replica \in Replicas : state.proposerEvaluated[replica]) =>
    state.proposedEvidence[1] = state.proposedEvidence[2]

Inv_CanonicalAuthorityRoot ==
  \A replica \in Replicas :
    state.proposerEvaluated[replica] =>
      /\ AuthorityGeneration = TargetGeneration
      /\ AuthorityBond > 0

=============================================================================
