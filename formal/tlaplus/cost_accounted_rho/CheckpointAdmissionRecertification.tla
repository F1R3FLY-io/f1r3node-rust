---------------- MODULE CheckpointAdmissionRecertification ----------------
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS
  \* @type: Str;
  Defect

ASSUME Defect \in {
  "None",
  "ReuseOldCertificate",
  "PreserveOldRejections",
  "DropFinalRejections",
  "RecertifyAcceptedOnly",
  "ContextDrift",
  "ShrinkInitiallyAdmitted",
  "DrainOriginalCandidates",
  "PublishBeforeSuccess",
  "TreatDeferredAsRejected",
  "ArrivalOrder",
  "SharedMutableAttempt",
  "NonDecreasingRetry"
}

Validators == {"A", "B"}
UserCandidates == {1, 2, 4, 5}
DummyCandidate == 3
Candidates == UserCandidates \cup {DummyCandidate}
Phases == {"Ready", "Certified", "Published"}
InitialLimit == 4

\* @type: Int => Seq(Int);
CanonicalWindow(candidateLimit) ==
  CASE candidateLimit = 4 -> <<1, 2, DummyCandidate, 4, 5>>
    [] candidateLimit = 2 -> <<1, 2, DummyCandidate>>
    [] candidateLimit = 1 -> <<1, DummyCandidate>>
    [] OTHER -> <<DummyCandidate>>

\* @type: Int => Seq(Int);
ArrivalWindow(candidateLimit) ==
  CASE candidateLimit = 4 -> <<5, 4, DummyCandidate, 2, 1>>
    [] candidateLimit = 2 -> <<2, 1, DummyCandidate>>
    [] candidateLimit = 1 -> <<1, DummyCandidate>>
    [] OTHER -> <<DummyCandidate>>

\* @type: (Int, Int) => Seq(Int);
InitiallyAdmittedWindow(candidateLimit, attemptGeneration) ==
  IF attemptGeneration = 0
    THEN CanonicalWindow(candidateLimit)
    ELSE <<1, DummyCandidate>>

\* @type: (Int, Int) => Seq(Int);
AttemptWindow(candidateLimit, attemptGeneration) ==
  CASE Defect = "ArrivalOrder" -> ArrivalWindow(candidateLimit)
    [] Defect = "ShrinkInitiallyAdmitted" ->
         InitiallyAdmittedWindow(candidateLimit, attemptGeneration)
    [] OTHER -> CanonicalWindow(candidateLimit)

\* @type: Set(Seq(Int));
WindowDomain == {
  <<>>,
  <<1, 2, DummyCandidate, 4, 5>>,
  <<1, 2, DummyCandidate>>,
  <<1, DummyCandidate>>,
  <<DummyCandidate>>,
  <<5, 4, DummyCandidate, 2, 1>>,
  <<2, 1, DummyCandidate>>
}

\* @type: Seq(Int) => Set(Int);
SequenceSet(sequence) ==
  {sequence[index] : index \in DOMAIN sequence}

CandidateClass(candidate, candidateLimit) ==
  CASE candidate = 1 -> "Admitted"
    [] candidate = 2 -> "Rejected"
    [] candidate = 4 -> "Deferred"
    [] candidate = 5 -> "Rejected"
    [] candidate = DummyCandidate /\ candidateLimit = InitialLimit -> "Rejected"
    [] OTHER -> "Admitted"

ClassPartition(window, candidateLimit, class) ==
  {candidate \in SequenceSet(window) : CandidateClass(candidate, candidateLimit) = class}

ExpectedAdmitted(window, candidateLimit) ==
  ClassPartition(window, candidateLimit, "Admitted")

ExpectedRejected(window, candidateLimit) ==
  ClassPartition(window, candidateLimit, "Rejected")

ExpectedDeferred(window, candidateLimit) ==
  ClassPartition(window, candidateLimit, "Deferred")

NextLimit(candidateLimit) ==
  IF Defect = "NonDecreasingRetry"
    THEN candidateLimit
    ELSE candidateLimit \div 2

\* @type: Int => Set(Int);
RootChain(admittedCount) ==
  CASE admittedCount = 0 -> {0}
    [] admittedCount = 1 -> {0, 1}
    [] admittedCount = 2 -> {0, 1, 2}
    [] admittedCount = 3 -> {0, 1, 2, 3}
    [] admittedCount = 4 -> {0, 1, 2, 3, 4}
    [] OTHER -> {0, 1, 2, 3, 4, 5}

FrozenContext(validator) == validator

VARIABLES
  \* @type: Str -> Str;
  phase,
  \* @type: Str -> Int;
  candidateLimit,
  \* @type: Str -> Int;
  attemptGeneration,
  \* @type: Str -> Int;
  previousLimit,
  \* @type: Str -> Int;
  certificateGeneration,
  \* @type: Str -> Seq(Int);
  certificateWindow,
  \* @type: Str -> Set(Int);
  certificateAdmitted,
  \* @type: Str -> Set(Int);
  certificateRejected,
  \* @type: Str -> Set(Int);
  certificateDeferred,
  \* @type: Str -> Set(Int);
  certificateRoots,
  \* @type: Str -> Str;
  certificateContext,
  \* @type: Str -> Int;
  publishedGeneration,
  \* @type: Str -> Seq(Int);
  publishedWindow,
  \* @type: Str -> Set(Int);
  publishedAdmitted,
  \* @type: Str -> Set(Int);
  publishedRejected,
  \* @type: Str -> Set(Int);
  publishedDeferred,
  \* @type: Str -> Set(Int);
  publishedRoots,
  \* @type: Str -> Set(Int);
  storage,
  \* @type: Str -> Set(Int);
  settled,
  \* @type: Bool;
  failedPublication,
  \* @type: Bool;
  failedSettlement,
  \* @type: Bool;
  crossValidatorMutation

vars == <<
  phase,
  candidateLimit,
  attemptGeneration,
  previousLimit,
  certificateGeneration,
  certificateWindow,
  certificateAdmitted,
  certificateRejected,
  certificateDeferred,
  certificateRoots,
  certificateContext,
  publishedGeneration,
  publishedWindow,
  publishedAdmitted,
  publishedRejected,
  publishedDeferred,
  publishedRoots,
  storage,
  settled,
  failedPublication,
  failedSettlement,
  crossValidatorMutation
>>

Init ==
  /\ phase = [validator \in Validators |-> "Ready"]
  /\ candidateLimit = [validator \in Validators |-> InitialLimit]
  /\ attemptGeneration = [validator \in Validators |-> 0]
  /\ previousLimit = [validator \in Validators |-> 0]
  /\ certificateGeneration = [validator \in Validators |-> -1]
  /\ certificateWindow = [validator \in Validators |-> <<>>]
  /\ certificateAdmitted = [validator \in Validators |-> {}]
  /\ certificateRejected = [validator \in Validators |-> {}]
  /\ certificateDeferred = [validator \in Validators |-> {}]
  /\ certificateRoots = [validator \in Validators |-> {}]
  /\ certificateContext = [validator \in Validators |-> validator]
  /\ publishedGeneration = [validator \in Validators |-> -1]
  /\ publishedWindow = [validator \in Validators |-> <<>>]
  /\ publishedAdmitted = [validator \in Validators |-> {}]
  /\ publishedRejected = [validator \in Validators |-> {}]
  /\ publishedDeferred = [validator \in Validators |-> {}]
  /\ publishedRoots = [validator \in Validators |-> {}]
  /\ storage = [validator \in Validators |-> UserCandidates]
  /\ settled = [validator \in Validators |-> {}]
  /\ failedPublication = FALSE
  /\ failedSettlement = FALSE
  /\ crossValidatorMutation = FALSE

Certify(validator) ==
  /\ phase[validator] = "Ready"
  /\ LET window == AttemptWindow(candidateLimit[validator], attemptGeneration[validator])
         admitted == ExpectedAdmitted(window, candidateLimit[validator])
         rejected == ExpectedRejected(window, candidateLimit[validator])
         deferred == ExpectedDeferred(window, candidateLimit[validator])
     IN
       /\ phase' = [phase EXCEPT ![validator] = "Certified"]
       /\ certificateGeneration' =
            [certificateGeneration EXCEPT
              ![validator] =
                IF Defect = "ReuseOldCertificate" /\ attemptGeneration[validator] > 0
                  THEN attemptGeneration[validator] - 1
                  ELSE attemptGeneration[validator]]
       /\ certificateWindow' = [certificateWindow EXCEPT ![validator] = window]
       /\ certificateAdmitted' = [certificateAdmitted EXCEPT ![validator] = admitted]
       /\ certificateRejected' =
            [certificateRejected EXCEPT
              ![validator] =
                IF Defect = "PreserveOldRejections" /\ attemptGeneration[validator] > 0
                  THEN @
                  ELSE IF Defect = "RecertifyAcceptedOnly" /\ attemptGeneration[validator] > 0
                    THEN {}
                    ELSE rejected]
       /\ certificateDeferred' =
            [certificateDeferred EXCEPT
              ![validator] =
                IF Defect = "RecertifyAcceptedOnly" /\ attemptGeneration[validator] > 0
                  THEN {}
                  ELSE deferred]
       /\ certificateRoots' =
            [certificateRoots EXCEPT ![validator] = RootChain(Cardinality(admitted))]
       /\ certificateContext' =
            [certificateContext EXCEPT
              ![validator] =
                IF Defect = "ContextDrift" THEN "foreign-context" ELSE FrozenContext(validator)]
       /\ crossValidatorMutation' =
            (crossValidatorMutation \/ Defect = "SharedMutableAttempt")
  /\ UNCHANGED <<
       candidateLimit,
       attemptGeneration,
       previousLimit,
       publishedGeneration,
       publishedWindow,
       publishedAdmitted,
       publishedRejected,
       publishedDeferred,
       publishedRoots,
       storage,
       settled,
       failedPublication,
       failedSettlement
     >>

CheckpointRetry(validator) ==
  /\ phase[validator] = "Certified"
  /\ candidateLimit[validator] > 1
  /\ phase' = [phase EXCEPT ![validator] = "Ready"]
  /\ previousLimit' = [previousLimit EXCEPT ![validator] = candidateLimit[validator]]
  /\ candidateLimit' =
       [candidateLimit EXCEPT ![validator] = NextLimit(candidateLimit[validator])]
  /\ attemptGeneration' =
       [attemptGeneration EXCEPT ![validator] = @ + 1]
  /\ publishedGeneration' =
       IF Defect = "PublishBeforeSuccess"
         THEN [publishedGeneration EXCEPT ![validator] = certificateGeneration[validator]]
         ELSE publishedGeneration
  /\ publishedWindow' =
       IF Defect = "PublishBeforeSuccess"
         THEN [publishedWindow EXCEPT ![validator] = certificateWindow[validator]]
         ELSE publishedWindow
  /\ publishedAdmitted' =
       IF Defect = "PublishBeforeSuccess"
         THEN [publishedAdmitted EXCEPT ![validator] = certificateAdmitted[validator]]
         ELSE publishedAdmitted
  /\ publishedRejected' =
       IF Defect = "PublishBeforeSuccess"
         THEN [publishedRejected EXCEPT ![validator] = certificateRejected[validator]]
         ELSE publishedRejected
  /\ publishedDeferred' =
       IF Defect = "PublishBeforeSuccess"
         THEN [publishedDeferred EXCEPT ![validator] = certificateDeferred[validator]]
         ELSE publishedDeferred
  /\ publishedRoots' =
       IF Defect = "PublishBeforeSuccess"
         THEN [publishedRoots EXCEPT ![validator] = certificateRoots[validator]]
         ELSE publishedRoots
  /\ storage' =
       IF Defect = "DrainOriginalCandidates"
         THEN [storage EXCEPT ![validator] = @ \ UserCandidates]
         ELSE storage
  /\ settled' =
       IF Defect = "PublishBeforeSuccess"
         THEN [settled EXCEPT ![validator] = certificateAdmitted[validator]]
         ELSE settled
  /\ failedPublication' = (failedPublication \/ Defect = "PublishBeforeSuccess")
  /\ failedSettlement' = (failedSettlement \/ Defect = "PublishBeforeSuccess")
  /\ UNCHANGED <<
       certificateGeneration,
       certificateWindow,
       certificateAdmitted,
       certificateRejected,
       certificateDeferred,
       certificateRoots,
       certificateContext,
       crossValidatorMutation
     >>

CheckpointSuccess(validator) ==
  /\ phase[validator] = "Certified"
  /\ LET finalRejected ==
           IF Defect = "DropFinalRejections"
             THEN {}
             ELSE IF Defect = "TreatDeferredAsRejected"
               THEN certificateRejected[validator] \cup certificateDeferred[validator]
               ELSE certificateRejected[validator]
         finalDeferred ==
           IF Defect = "TreatDeferredAsRejected"
             THEN {}
             ELSE certificateDeferred[validator]
         terminalUsers ==
           (certificateAdmitted[validator] \cup finalRejected) \cap UserCandidates
     IN
       /\ phase' = [phase EXCEPT ![validator] = "Published"]
       /\ publishedGeneration' =
            [publishedGeneration EXCEPT ![validator] = certificateGeneration[validator]]
       /\ publishedWindow' =
            [publishedWindow EXCEPT ![validator] = certificateWindow[validator]]
       /\ publishedAdmitted' =
            [publishedAdmitted EXCEPT ![validator] = certificateAdmitted[validator]]
       /\ publishedRejected' =
            [publishedRejected EXCEPT ![validator] = finalRejected]
       /\ publishedDeferred' =
            [publishedDeferred EXCEPT ![validator] = finalDeferred]
       /\ publishedRoots' =
            [publishedRoots EXCEPT ![validator] = certificateRoots[validator]]
       /\ storage' = [storage EXCEPT ![validator] = @ \ terminalUsers]
       /\ settled' =
            [settled EXCEPT ![validator] = certificateAdmitted[validator]]
  /\ UNCHANGED <<
       candidateLimit,
       attemptGeneration,
       previousLimit,
       certificateGeneration,
       certificateWindow,
       certificateAdmitted,
       certificateRejected,
       certificateDeferred,
       certificateRoots,
       certificateContext,
       failedPublication,
       failedSettlement,
       crossValidatorMutation
     >>

Next ==
  \E validator \in Validators :
    Certify(validator) \/ CheckpointRetry(validator) \/ CheckpointSuccess(validator)

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A validator \in Validators : WF_vars(Certify(validator))
  /\ \A validator \in Validators : WF_vars(CheckpointSuccess(validator))

TypeOK ==
  /\ phase \in [Validators -> Phases]
  /\ candidateLimit \in [Validators -> 0..InitialLimit]
  /\ attemptGeneration \in [Validators -> 0..3]
  /\ previousLimit \in [Validators -> 0..InitialLimit]
  /\ certificateGeneration \in [Validators -> -1..3]
  /\ certificateWindow \in [Validators -> WindowDomain]
  /\ certificateAdmitted \in [Validators -> SUBSET Candidates]
  /\ certificateRejected \in [Validators -> SUBSET Candidates]
  /\ certificateDeferred \in [Validators -> SUBSET Candidates]
  /\ certificateRoots \in [Validators -> SUBSET (0..5)]
  /\ certificateContext \in [Validators -> {"A", "B", "foreign-context"}]
  /\ publishedGeneration \in [Validators -> -1..3]
  /\ publishedWindow \in [Validators -> WindowDomain]
  /\ publishedAdmitted \in [Validators -> SUBSET Candidates]
  /\ publishedRejected \in [Validators -> SUBSET Candidates]
  /\ publishedDeferred \in [Validators -> SUBSET Candidates]
  /\ publishedRoots \in [Validators -> SUBSET (0..5)]
  /\ storage \in [Validators -> SUBSET UserCandidates]
  /\ settled \in [Validators -> SUBSET Candidates]
  /\ failedPublication \in BOOLEAN
  /\ failedSettlement \in BOOLEAN
  /\ crossValidatorMutation \in BOOLEAN

PartitionCompleteAndDisjoint(window, admitted, rejected, deferred) ==
  /\ admitted \cup rejected \cup deferred = SequenceSet(window)
  /\ admitted \cap rejected = {}
  /\ admitted \cap deferred = {}
  /\ rejected \cap deferred = {}

CertifiedWindowIsCanonicalRawPrefix ==
  \A validator \in Validators :
    phase[validator] \in {"Certified", "Published"} =>
      certificateWindow[validator] = CanonicalWindow(candidateLimit[validator])

CertifiedPartitionIsCompleteAndDisjoint ==
  \A validator \in Validators :
    phase[validator] \in {"Certified", "Published"} =>
      PartitionCompleteAndDisjoint(
        certificateWindow[validator],
        certificateAdmitted[validator],
        certificateRejected[validator],
        certificateDeferred[validator])

CertifiedPartitionMatchesStateBoundExecution ==
  \A validator \in Validators :
    phase[validator] \in {"Certified", "Published"} =>
      /\ certificateAdmitted[validator] =
           ExpectedAdmitted(certificateWindow[validator], candidateLimit[validator])
      /\ certificateRejected[validator] =
           ExpectedRejected(certificateWindow[validator], candidateLimit[validator])
      /\ certificateDeferred[validator] =
           ExpectedDeferred(certificateWindow[validator], candidateLimit[validator])

CertificateUsesCurrentGeneration ==
  \A validator \in Validators :
    phase[validator] \in {"Certified", "Published"} =>
      certificateGeneration[validator] = attemptGeneration[validator]

CertificateUsesFrozenContext ==
  \A validator \in Validators :
    phase[validator] \in {"Certified", "Published"} =>
      certificateContext[validator] = FrozenContext(validator)

CertificateRootChainIsExact ==
  \A validator \in Validators :
    phase[validator] \in {"Certified", "Published"} =>
      certificateRoots[validator] = RootChain(Cardinality(certificateAdmitted[validator]))

PublishedPartitionIsCompleteAndDisjoint ==
  \A validator \in Validators :
    phase[validator] = "Published" =>
      PartitionCompleteAndDisjoint(
        publishedWindow[validator],
        publishedAdmitted[validator],
        publishedRejected[validator],
        publishedDeferred[validator])

PublishedAttemptIsFinal ==
  \A validator \in Validators :
    phase[validator] = "Published" =>
      /\ publishedGeneration[validator] = attemptGeneration[validator]
      /\ publishedWindow[validator] = certificateWindow[validator]
      /\ publishedAdmitted[validator] = certificateAdmitted[validator]
      /\ publishedRejected[validator] = certificateRejected[validator]
      /\ publishedDeferred[validator] = certificateDeferred[validator]
      /\ publishedRoots[validator] = certificateRoots[validator]

PeerRecomputationMatchesPublication ==
  \A validator \in Validators :
    phase[validator] = "Published" =>
      /\ publishedWindow[validator] = CanonicalWindow(candidateLimit[validator])
      /\ publishedAdmitted[validator] =
           ExpectedAdmitted(publishedWindow[validator], candidateLimit[validator])
      /\ publishedRejected[validator] =
           ExpectedRejected(publishedWindow[validator], candidateLimit[validator])
      /\ publishedDeferred[validator] =
           ExpectedDeferred(publishedWindow[validator], candidateLimit[validator])

RemovedCandidatesReceiveNoTerminalDecision ==
  \A validator \in Validators :
    phase[validator] = "Published" =>
      UserCandidates \ SequenceSet(publishedWindow[validator]) \subseteq storage[validator]

DeferredCandidatesRemainAvailable ==
  \A validator \in Validators :
    phase[validator] = "Published" =>
      publishedDeferred[validator] \cap UserCandidates \subseteq storage[validator]

StorageDrainsOnlyFinalTerminalUsers ==
  \A validator \in Validators :
    phase[validator] = "Published" =>
      storage[validator] =
        UserCandidates \
          ((publishedAdmitted[validator] \cup publishedRejected[validator]) \cap UserCandidates)

FailedAttemptsPublishNothing == ~failedPublication

FailedAttemptsSettleNothing == ~failedSettlement

FinalSettlementMatchesCertificate ==
  \A validator \in Validators :
    phase[validator] = "Published" => settled[validator] = publishedAdmitted[validator]

RetryLimitsStrictlyDecrease ==
  \A validator \in Validators :
    previousLimit[validator] = 0 \/ candidateLimit[validator] < previousLimit[validator]

ValidatorAttemptStateIsIndependent == ~crossValidatorMutation

EventuallyAllValidatorsPublish == <> (\A validator \in Validators : phase[validator] = "Published")

=============================================================================
