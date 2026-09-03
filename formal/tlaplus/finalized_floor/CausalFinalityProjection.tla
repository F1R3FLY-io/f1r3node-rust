-------------------------- MODULE CausalFinalityProjection --------------------------
EXTENDS Naturals, Integers, FiniteSets, TLC

CONSTANTS
  \* @type: Set(Int);
  Replicas,
  \* @type: Bool;
  ParentContainsEvidence,
  \* @type: Bool;
  UseAmbientEvidenceForCandidate,
  \* @type: Bool;
  UseOutgoingEvidenceForOwnProjection,
  \* @type: Bool;
  UseUnfilteredFinalityVotes,
  \* @type: Bool;
  MutateExactWireState,
  \* @type: Bool;
  PropagateInvalidContext,
  \* @type: Bool;
  RequireBothDeltaDependencies,
  \* @type: Bool;
  OmitSecondEvidenceDependency

ASSUME Replicas /= {}

\* @type: Int;
ValidatorOne == 1
\* @type: Int;
ValidatorTwo == 2
\* @type: Int;
Equivocator == 3
\* @type: Set(Int);
Validators == {ValidatorOne, ValidatorTwo, Equivocator}
\* @type: Int;
InvalidVoter == ValidatorTwo

\* @type: Str;
ObjectiveEvidence == "ObjectiveEvidence"
\* @type: Set(Str);
EvidenceSet == {ObjectiveEvidence}
\* @type: Str;
LeftEvidenceBlock == "EvidenceLeft"
\* @type: Str;
RightEvidenceBlock == "EvidenceRight"
\* @type: Set(Str);
EvidenceBlocks == {LeftEvidenceBlock, RightEvidenceBlock}

\* @type: Str;
BaseFloor == "BaseFloor"
\* @type: Str;
PromotedFloor == "PromotedFloor"
\* @type: Set(Str);
Floors == {BaseFloor, PromotedFloor}

\* @type: Set(Int);
ExactJustificationValue == Validators
\* @type: Int -> Int;
ExactMaxSequenceValue == [validator \in Validators |-> 9]

\* @type: Set(Str);
ParentOutgoingEvidence == IF ParentContainsEvidence THEN EvidenceSet ELSE {}
\* @type: Set(Str);
CandidateDelta == EvidenceSet
\* @type: Set(Str);
CandidateOutgoingEvidence == ParentOutgoingEvidence \cup CandidateDelta

\* @type: Set(Str);
CandidateDependencies ==
  IF OmitSecondEvidenceDependency
  THEN {LeftEvidenceBlock}
  ELSE EvidenceBlocks

\* @type: (Int) => Int;
CertifiedLatestGeneration(validator) == IF validator \in Validators THEN 0 ELSE -1
\* @type: (Int) => Int;
ParentAuthorityGeneration(validator) == IF validator \in Validators THEN 0 ELSE -1
\* @type: (Int) => Int;
ParentAuthorityStake(validator) == IF validator \in Validators THEN 1 ELSE 0

\* @type: (Int, Set(Str)) => Bool;
EvidenceDisqualifies(validator, evidence) ==
  validator = Equivocator /\
  ObjectiveEvidence \in evidence /\
  CertifiedLatestGeneration(validator) = ParentAuthorityGeneration(validator)

\* @type: (Set(Str)) => Set(Int);
FilteredProjection(evidence) ==
  {validator \in ExactJustificationValue :
     /\ ParentAuthorityStake(validator) > 0
     /\ CertifiedLatestGeneration(validator) = ParentAuthorityGeneration(validator)
     /\ validator /= InvalidVoter
     /\ ~EvidenceDisqualifies(validator, evidence)}

\* @type: (Set(Str)) => Set(Int);
ProjectionFor(evidence) ==
  IF UseUnfilteredFinalityVotes
  THEN ExactJustificationValue
  ELSE FilteredProjection(evidence)

\* @type: (Set(Int)) => Bool;
HasQuorum(voters) == Cardinality(voters) >= 2
\* @type: (Set(Int)) => Str;
FloorFor(voters) == IF HasQuorum(voters) THEN PromotedFloor ELSE BaseFloor

VARIABLES
  \* @type: Int -> Set(Str);
  ambientEvidence,
  \* @type: Int -> Bool;
  candidateCertified,
  \* @type: Int -> Set(Str);
  incomingEvidence,
  \* @type: Int -> Set(Str);
  outgoingEvidence,
  \* @type: Int -> Set(Int);
  finalityProjection,
  \* @type: Int -> Str;
  certifiedFloor,
  \* @type: Int -> Str;
  certifiedPreState,
  \* @type: Int -> Bool;
  slashAuthorized,
  \* @type: Int -> Set(Int);
  exactJustifications,
  \* @type: Int -> (Int -> Int);
  exactMaxSequence,
  \* @type: Int -> Set(Str);
  invalidOutgoingEvidence,
  \* @type: Int -> Int;
  restartCount

vars ==
  <<ambientEvidence, candidateCertified, incomingEvidence, outgoingEvidence,
    finalityProjection, certifiedFloor, certifiedPreState, slashAuthorized,
    exactJustifications, exactMaxSequence, invalidOutgoingEvidence,
    restartCount>>

Init ==
  /\ ambientEvidence = [replica \in Replicas |-> {}]
  /\ candidateCertified = [replica \in Replicas |-> FALSE]
  /\ incomingEvidence = [replica \in Replicas |-> {}]
  /\ outgoingEvidence = [replica \in Replicas |-> {}]
  /\ finalityProjection = [replica \in Replicas |-> {}]
  /\ certifiedFloor = [replica \in Replicas |-> BaseFloor]
  /\ certifiedPreState = [replica \in Replicas |-> BaseFloor]
  /\ slashAuthorized = [replica \in Replicas |-> FALSE]
  /\ exactJustifications =
       [replica \in Replicas |-> ExactJustificationValue]
  /\ exactMaxSequence =
       [replica \in Replicas |-> ExactMaxSequenceValue]
  /\ invalidOutgoingEvidence = [replica \in Replicas |-> {}]
  /\ restartCount = [replica \in Replicas |-> 0]

ObserveAmbient(replica) ==
  /\ ObjectiveEvidence \notin ambientEvidence[replica]
  /\ ambientEvidence' =
       [ambientEvidence EXCEPT ![replica] = @ \cup {ObjectiveEvidence}]
  /\ UNCHANGED <<candidateCertified, incomingEvidence, outgoingEvidence,
                  finalityProjection, certifiedFloor, certifiedPreState,
                  slashAuthorized, exactJustifications, exactMaxSequence,
                  invalidOutgoingEvidence, restartCount>>

\* @type: (Int) => Set(Str);
CertificationIncoming(replica) ==
  IF UseAmbientEvidenceForCandidate
  THEN ParentOutgoingEvidence \cup ambientEvidence[replica]
  ELSE ParentOutgoingEvidence

\* @type: (Int) => Set(Str);
CertificationProjectionEvidence(replica) ==
  IF UseOutgoingEvidenceForOwnProjection
  THEN CertificationIncoming(replica) \cup CandidateDelta
  ELSE CertificationIncoming(replica)

CertifyCandidate(replica) ==
  /\ ~candidateCertified[replica]
  /\ ~RequireBothDeltaDependencies \/ EvidenceBlocks \subseteq CandidateDependencies
  /\ LET incoming == CertificationIncoming(replica) IN
     LET outgoing == incoming \cup CandidateDelta IN
     LET projected == ProjectionFor(CertificationProjectionEvidence(replica)) IN
       /\ candidateCertified' = [candidateCertified EXCEPT ![replica] = TRUE]
       /\ incomingEvidence' = [incomingEvidence EXCEPT ![replica] = incoming]
       /\ outgoingEvidence' = [outgoingEvidence EXCEPT ![replica] = outgoing]
       /\ finalityProjection' = [finalityProjection EXCEPT ![replica] = projected]
       /\ certifiedFloor' = [certifiedFloor EXCEPT ![replica] = FloorFor(projected)]
       /\ certifiedPreState' = [certifiedPreState EXCEPT ![replica] = FloorFor(projected)]
       /\ slashAuthorized' =
            [slashAuthorized EXCEPT
               ![replica] = ObjectiveEvidence \in outgoing]
       /\ exactJustifications' =
            IF MutateExactWireState
            THEN [exactJustifications EXCEPT ![replica] = projected]
            ELSE exactJustifications
       /\ exactMaxSequence' =
            IF MutateExactWireState
            THEN [exactMaxSequence EXCEPT
                    ![replica] =
                      [validator \in projected |-> ExactMaxSequenceValue[validator]]]
            ELSE exactMaxSequence
  /\ UNCHANGED <<ambientEvidence, invalidOutgoingEvidence, restartCount>>

CertifyInvalidBlock(replica) ==
  /\ invalidOutgoingEvidence[replica] = {}
  /\ invalidOutgoingEvidence' =
       IF PropagateInvalidContext
       THEN [invalidOutgoingEvidence EXCEPT ![replica] = CandidateOutgoingEvidence]
       ELSE invalidOutgoingEvidence
  /\ UNCHANGED <<ambientEvidence, candidateCertified, incomingEvidence,
                  outgoingEvidence, finalityProjection, certifiedFloor,
                  certifiedPreState, slashAuthorized, exactJustifications,
                  exactMaxSequence, restartCount>>

Restart(replica) ==
  /\ restartCount[replica] < 1
  /\ ambientEvidence' = [ambientEvidence EXCEPT ![replica] = {}]
  /\ restartCount' = [restartCount EXCEPT ![replica] = @ + 1]
  /\ UNCHANGED <<candidateCertified, incomingEvidence, outgoingEvidence,
                  finalityProjection, certifiedFloor, certifiedPreState,
                  slashAuthorized, exactJustifications, exactMaxSequence,
                  invalidOutgoingEvidence>>

Next ==
  \/ \E replica \in Replicas : ObserveAmbient(replica)
  \/ \E replica \in Replicas : CertifyCandidate(replica)
  \/ \E replica \in Replicas : CertifyInvalidBlock(replica)
  \/ \E replica \in Replicas : Restart(replica)

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ ambientEvidence \in [Replicas -> SUBSET EvidenceSet]
  /\ candidateCertified \in [Replicas -> BOOLEAN]
  /\ incomingEvidence \in [Replicas -> SUBSET EvidenceSet]
  /\ outgoingEvidence \in [Replicas -> SUBSET EvidenceSet]
  /\ finalityProjection \in [Replicas -> SUBSET Validators]
  /\ certifiedFloor \in [Replicas -> Floors]
  /\ certifiedPreState \in [Replicas -> Floors]
  /\ slashAuthorized \in [Replicas -> BOOLEAN]
  /\ exactJustifications \in [Replicas -> SUBSET Validators]
  /\ \A replica \in Replicas :
       /\ DOMAIN exactMaxSequence[replica] \subseteq Validators
       /\ \A validator \in DOMAIN exactMaxSequence[replica] :
            exactMaxSequence[replica][validator] \in 0..9
  /\ invalidOutgoingEvidence \in [Replicas -> SUBSET EvidenceSet]
  /\ restartCount \in [Replicas -> 0..1]

Inv_IncomingEvidenceComesOnlyFromParents ==
  \A replica \in Replicas :
    candidateCertified[replica] =>
      incomingEvidence[replica] = ParentOutgoingEvidence

Inv_OutgoingIsIncomingPlusValidatedDelta ==
  \A replica \in Replicas :
    candidateCertified[replica] =>
      outgoingEvidence[replica] = incomingEvidence[replica] \cup CandidateDelta

Inv_CandidateDeltaCannotAffectOwnProjection ==
  \A replica \in Replicas :
    candidateCertified[replica] =>
      finalityProjection[replica] = FilteredProjection(incomingEvidence[replica])

Inv_ExactJustificationsPreserved ==
  \A replica \in Replicas :
    exactJustifications[replica] = ExactJustificationValue

Inv_ExactMaxSequencePreserved ==
  \A replica \in Replicas :
    exactMaxSequence[replica] = ExactMaxSequenceValue

Inv_FloorAndPreStateUseFrozenProjection ==
  \A replica \in Replicas :
    candidateCertified[replica] =>
      /\ certifiedFloor[replica] = FloorFor(FilteredProjection(incomingEvidence[replica]))
      /\ certifiedPreState[replica] = certifiedFloor[replica]

Inv_InvalidAndCausallyEquivocatingVotesExcluded ==
  \A replica \in Replicas :
    candidateCertified[replica] =>
      /\ InvalidVoter \notin finalityProjection[replica]
      /\ (ObjectiveEvidence \in incomingEvidence[replica] =>
            Equivocator \notin finalityProjection[replica])

Inv_ValidatedDeltaMayAuthorizeSlash ==
  \A replica \in Replicas :
    candidateCertified[replica] =>
      slashAuthorized[replica] =
        (ObjectiveEvidence \in outgoingEvidence[replica])

Inv_DeltaCarriesBothCertifiedDependencies ==
  \A replica \in Replicas :
    candidateCertified[replica] => EvidenceBlocks \subseteq CandidateDependencies

Inv_InvalidBlocksDoNotPropagateEvidence ==
  \A replica \in Replicas : invalidOutgoingEvidence[replica] = {}

Inv_AmbientEvidenceCannotChangeCertifiedResult ==
  \A replica \in Replicas :
    candidateCertified[replica] =>
      /\ incomingEvidence[replica] = ParentOutgoingEvidence
      /\ finalityProjection[replica] = FilteredProjection(ParentOutgoingEvidence)
      /\ certifiedFloor[replica] = FloorFor(FilteredProjection(ParentOutgoingEvidence))

Inv_EquivalentReplicasConverge ==
  \A leftReplica \in Replicas, rightReplica \in Replicas :
    (candidateCertified[leftReplica] /\ candidateCertified[rightReplica]) =>
      /\ incomingEvidence[leftReplica] = incomingEvidence[rightReplica]
      /\ outgoingEvidence[leftReplica] = outgoingEvidence[rightReplica]
      /\ finalityProjection[leftReplica] = finalityProjection[rightReplica]
      /\ certifiedFloor[leftReplica] = certifiedFloor[rightReplica]
      /\ certifiedPreState[leftReplica] = certifiedPreState[rightReplica]
      /\ slashAuthorized[leftReplica] = slashAuthorized[rightReplica]

=============================================================================
