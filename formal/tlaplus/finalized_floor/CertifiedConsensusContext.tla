-------------------- MODULE CertifiedConsensusContext --------------------
EXTENDS Integers, FiniteSets, Sequences

CONSTANT
  \* @type: Str;
  Defect

ASSUME Defect \in {
  "None",
  "LocalLmmAdmission",
  "LocalTrackerAdmission",
  "LocalFinalizedAdmission",
  "ParentOrderAdmission",
  "CandidatePrestateAuthority",
  "SnapshotPrefilter",
  "EstimatorRefilter",
  "HeadWeightForkChoice",
  "LocalTopForkChoice",
  "OutsideFloorVote",
  "IncompleteLatestSlots",
  "FinalizerReprojection",
  "GenerationBlindLmm",
  "StaleFinalizerCommit"
}

\* @type: Set(Int);
Nodes == {1, 2, 3}
\* @type: Set(Int);
Validators == {1, 2, 3}
\* @type: Set(Int);
Workers == {1, 2}
\* @type: Set(Str);
Blocks == {"G", "P", "E0", "E1", "C", "U", "R", "O"}
\* @type: Set(Str);
CandidateClosure == {"G", "P", "E0", "E1", "C"}
\* @type: Set(Str);
AmbientBlocks == {"U", "R", "O"}
\* @type: Str;
Fault == "2@0#1"
\* @type: Str;
Candidate == "C"
\* @type: Str;
NoBlock == "-"
\* @type: Set(Str);
WorkerPhases == {"Idle", "Snapshot", "Evaluated", "Manifest", "Committed", "Stale"}
\* @type: Int;
MaxHeadRevision == 2
\* @type: Int;
MaxRequestSequence == 4
\* @type: Int;
MaxDagRevision == 3
\* @type: Int;
MaxAdmissionRevision == 3

\* @type: (Str) => Int;
Sender(block) ==
  CASE block \in {"G", "P", "U", "O"} -> 1
    [] block \in {"E0", "E1"} -> 2
    [] OTHER -> 3

\* @type: (Str) => Int;
Generation(block) == IF block = "U" THEN 1 ELSE 0

\* @type: (Str) => Int;
SequenceNumber(block) ==
  CASE block = "G" -> 0
    [] block \in {"P", "E0", "E1"} -> 1
    [] block \in {"C", "U"} -> 2
    [] OTHER -> 3

\* @type: (Str) => Bool;
DescendsFromFloor(block) == block /= "O"

\* @type: (Int) => Int;
FloorStake(validator) ==
  CASE validator = 1 -> 5
    [] validator = 2 -> 3
    [] OTHER -> 2

\* @type: (Int) => Int;
HeadStake(validator) ==
  CASE validator = 1 -> 1
    [] validator = 2 -> 7
    [] OTHER -> 2

\* @type: (Int) => Int;
FloorGeneration(validator) == IF validator \in Validators THEN 0 ELSE -1

\* @type: (Int) => Str;
CandidateWire(validator) ==
  CASE validator = 1 -> "P"
    [] validator = 2 -> "E0"
    [] OTHER -> "G"

\* @typeAlias: localview = {
\*   ambientLatest: Str,
\*   trackerKnows: Bool,
\*   finalizedParent: Bool,
\*   reverseParents: Bool,
\*   cacheFiltered: Bool,
\*   headStakeShift: Bool,
\*   preGeneration: Int,
\*   ambientTop: Int,
\*   missingSlot: Bool
\* };

\* Every element is a receiver-local state surrounding the same complete,
\* certified causal closure. None of these fields is consensus evidence.
LocalViews ==
  [ambientLatest : {"P", "U"},
   trackerKnows : BOOLEAN,
   finalizedParent : BOOLEAN,
   reverseParents : BOOLEAN,
   cacheFiltered : BOOLEAN,
   headStakeShift : BOOLEAN,
   preGeneration : {0, 1},
   ambientTop : {3, 7},
   missingSlot : BOOLEAN]

ReferenceView ==
  [ambientLatest |-> "P",
   trackerKnows |-> FALSE,
   finalizedParent |-> FALSE,
   reverseParents |-> FALSE,
   cacheFiltered |-> FALSE,
   headStakeShift |-> FALSE,
   preGeneration |-> 0,
   ambientTop |-> 3,
   missingSlot |-> FALSE]

\* @type: ($localview) => (Int -> Int);
AuthorityGenerations(view) ==
  [validator \in Validators |->
    IF Defect = "CandidatePrestateAuthority" /\ validator = 1
    THEN view.preGeneration
    ELSE FloorGeneration(validator)]

\* @type: ($localview) => (Int -> Int);
AuthorityStakes(view) ==
  [validator \in Validators |-> FloorStake(validator)]

\* @type: ($localview) => Set(Str);
EffectiveEvidence(view) ==
  IF Defect = "LocalTrackerAdmission"
  THEN IF view.trackerKnows THEN {Fault} ELSE {}
  ELSE {Fault}

\* @type: ($localview) => (Int -> Str);
ExactLatestMessages(view) ==
  [validator \in Validators |->
    IF Defect = "IncompleteLatestSlots" /\ validator = 3 /\ view.missingSlot
    THEN NoBlock
    ELSE IF Defect = "OutsideFloorVote" /\ validator = 1
    THEN "O"
    ELSE IF Defect = "LocalLmmAdmission" /\ validator = 1
    THEN view.ambientLatest
    ELSE IF Defect = "SnapshotPrefilter" /\
            validator = 2 /\ view.trackerKnows
         THEN NoBlock
         ELSE IF Defect = "GenerationBlindLmm" /\ validator = 1
              THEN view.ambientLatest
              ELSE CandidateWire(validator)]

\* @type: ($localview, Int) => Bool;
GenerationEligible(view, validator) ==
  LET block == ExactLatestMessages(view)[validator] IN
  block /= NoBlock /\
  (Defect = "GenerationBlindLmm" \/
   Generation(block) = AuthorityGenerations(view)[validator])

\* @type: ($localview, Int) => Bool;
FloorDescendantEligible(view, validator) ==
  Defect = "OutsideFloorVote" \/
  DescendsFromFloor(ExactLatestMessages(view)[validator])

\* @type: ($localview) => Set(Int);
EligibleLatestMessages(view) ==
  {validator \in Validators :
    GenerationEligible(view, validator) /\
    FloorDescendantEligible(view, validator) /\
    ~(validator = 2 /\ Fault \in EffectiveEvidence(view))}

\* @typeAlias: context = {
\*   floor: Str,
\*   stakes: Int -> Int,
\*   generations: Int -> Int,
\*   exactLatest: Int -> Str,
\*   evidence: Set(Str),
\*   eligible: Set(Int)
\* };

\* @type: ($localview) => $context;
CertifiedContext(view) ==
  [floor |-> "G",
   stakes |-> AuthorityStakes(view),
   generations |-> AuthorityGenerations(view),
   exactLatest |-> ExactLatestMessages(view),
   evidence |-> EffectiveEvidence(view),
   eligible |-> EligibleLatestMessages(view)]

\* @type: ($localview) => Bool;
AdmissionVerdict(view) ==
  CASE Defect = "LocalLmmAdmission" -> view.ambientLatest = "P"
    [] Defect = "LocalTrackerAdmission" -> view.trackerKnows
    [] Defect = "LocalFinalizedAdmission" -> ~view.finalizedParent
    [] Defect = "ParentOrderAdmission" -> ~view.reverseParents
    [] OTHER -> TRUE

\* @type: (Set(Int), Int -> Int) => Int;
SupportWeight(supporters, stakes) ==
  (IF 1 \in supporters THEN stakes[1] ELSE 0) +
  (IF 2 \in supporters THEN stakes[2] ELSE 0) +
  (IF 3 \in supporters THEN stakes[3] ELSE 0)

\* @type: (Int -> Int) => Int;
CommitteeWeight(stakes) == stakes[1] + stakes[2] + stakes[3]

\* @type: (Set(Int), Int -> Int) => Bool;
WeightedMajority(supporters, stakes) ==
  2 * SupportWeight(supporters, stakes) > CommitteeWeight(stakes)

\* @type: ($localview) => Set(Int);
EstimatorProjection(view) ==
  IF Defect = "EstimatorRefilter" /\ view.cacheFiltered
  THEN EligibleLatestMessages(view) \ {1}
  ELSE EligibleLatestMessages(view)

\* @type: ($localview) => (Int -> Int);
EstimatorStakes(view) ==
  IF Defect = "HeadWeightForkChoice" /\ view.headStakeShift
  THEN [validator \in Validators |-> HeadStake(validator)]
  ELSE AuthorityStakes(view)

\* @type: ($localview) => Str;
EstimatorChoice(view) ==
  IF WeightedMajority(EstimatorProjection(view), EstimatorStakes(view))
  THEN "C"
  ELSE "P"

\* @type: ($localview) => Str;
EstimatorLca(view) ==
  IF Defect = "LocalTopForkChoice" /\ view.ambientTop = 7
  THEN "G"
  ELSE "P"

\* @type: ($localview) => Set(Int);
FinalizerProjection(view) ==
  IF Defect = "FinalizerReprojection" /\ view.trackerKnows
  THEN EligibleLatestMessages(view) \ {1}
  ELSE EligibleLatestMessages(view)

\* @type: ($localview) => Str;
FinalizerChoice(view) ==
  IF WeightedMajority(FinalizerProjection(view), AuthorityStakes(view))
  THEN "C"
  ELSE "P"

\* @type: (Set(Str)) => Set(Str);
EvidenceGroup(blocks) == blocks \cap {"E0", "E1"}

\* @type: (Set(Str)) => Set(Str);
CanonicalEvidence(blocks) ==
  IF {"E0", "E1"} \subseteq EvidenceGroup(blocks) THEN {Fault} ELSE {}

AdmissionClosureAgreement ==
  \A view \in LocalViews : AdmissionVerdict(view) = AdmissionVerdict(ReferenceView)

ConsensusContextExtensional ==
  \A view \in LocalViews : CertifiedContext(view) = CertifiedContext(ReferenceView)

LocalLmmIrrelevantToAdmission ==
  \A view \in LocalViews :
    AdmissionVerdict(view) =
      AdmissionVerdict([view EXCEPT
        !.ambientLatest = IF @ = "P" THEN "U" ELSE "P"])

LocalTrackerIrrelevantToAdmission ==
  \A view \in LocalViews :
    AdmissionVerdict(view) =
      AdmissionVerdict([view EXCEPT !.trackerKnows = ~@])

LocalFinalizedFlagsIrrelevantToAdmission ==
  \A view \in LocalViews :
    AdmissionVerdict(view) =
      AdmissionVerdict([view EXCEPT !.finalizedParent = ~@])

ParentOrderInvariant ==
  \A view \in LocalViews :
    AdmissionVerdict(view) =
      AdmissionVerdict([view EXCEPT !.reverseParents = ~@])

EvidenceMergeACI ==
  \A left, middle, right \in SUBSET CandidateClosure :
    /\ EvidenceGroup(left \cup middle) = EvidenceGroup(middle \cup left)
    /\ EvidenceGroup((left \cup middle) \cup right) =
       EvidenceGroup(left \cup (middle \cup right))
    /\ EvidenceGroup(left \cup left) = EvidenceGroup(left)

EvidenceSoundAndViewComplete ==
  \A view \in LocalViews :
    /\ Fault \in EffectiveEvidence(view)
    /\ CanonicalEvidence(CandidateClosure) = {Fault}
    /\ Sender("E0") = Sender("E1")
    /\ Generation("E0") = Generation("E1")
    /\ SequenceNumber("E0") = SequenceNumber("E1")

FrozenFloorAuthority ==
  \A view \in LocalViews, validator \in Validators :
    /\ AuthorityStakes(view)[validator] = FloorStake(validator)
    /\ AuthorityGenerations(view)[validator] = FloorGeneration(validator)

CandidatePrestateAuthorityNoninterference == FrozenFloorAuthority

GenerationScopedVotes ==
  \A view \in LocalViews :
    \A validator \in EligibleLatestMessages(view) :
      Generation(ExactLatestMessages(view)[validator]) =
        AuthorityGenerations(view)[validator]

CompleteLatestMessageSlots ==
  \A view \in LocalViews :
    \A validator \in Validators : ExactLatestMessages(view)[validator] /= NoBlock

EligibleVotesDescendFromFloor ==
  \A view \in LocalViews :
    \A validator \in EligibleLatestMessages(view) :
      DescendsFromFloor(ExactLatestMessages(view)[validator])

EstimatorConsumesOneProjection ==
  \A view \in LocalViews :
    EstimatorProjection(view) = EligibleLatestMessages(view)

EstimatorUsesFrozenAuthority ==
  \A view \in LocalViews : EstimatorStakes(view) = AuthorityStakes(view)

EstimatorContextExtensional ==
  \A view \in LocalViews : EstimatorChoice(view) = EstimatorChoice(ReferenceView)

EstimatorLcaContextExtensional ==
  \A view \in LocalViews : EstimatorLca(view) = EstimatorLca(ReferenceView)

FinalizerConsumesOneProjection ==
  \A view \in LocalViews :
    FinalizerProjection(view) = EligibleLatestMessages(view)

FinalizerContextExtensional ==
  \A view \in LocalViews : FinalizerChoice(view) = FinalizerChoice(ReferenceView)

\* @type: (Int) => Str;
CandidateForRevision(revision) == IF revision = 0 THEN "C" ELSE "R"

\* @typeAlias: pipelinestate = {
\*   requestSequence: Int,
\*   requestedThrough: Int,
\*   launchedThrough: Int,
\*   completedThrough: Int,
\*   dagRevision: Int,
\*   admissionRevision: Int,
\*   headRevision: Int,
\*   ledger: Int -> Str,
\*   workerPhase: Int -> Str,
\*   workerCovered: Int -> Int,
\*   workerExpected: Int -> Int,
\*   workerCandidate: Int -> Str
\* };
module_typedefs == TRUE

VARIABLE
  \* @type: $pipelinestate;
  state

\* @type: <<$pipelinestate>>;
vars == <<state>>

Init ==
  state =
    [requestSequence |-> 1,
     requestedThrough |-> 1,
     launchedThrough |-> 0,
     completedThrough |-> 0,
     dagRevision |-> 0,
     admissionRevision |-> 0,
     headRevision |-> 0,
     ledger |-> [revision \in 1..MaxHeadRevision |-> NoBlock],
     workerPhase |-> [worker \in Workers |-> "Idle"],
     workerCovered |-> [worker \in Workers |-> 0],
     workerExpected |-> [worker \in Workers |-> 0],
     workerCandidate |-> [worker \in Workers |-> NoBlock]]

PublishRequest ==
  /\ state.requestSequence < MaxRequestSequence
  /\ state.dagRevision < MaxDagRevision
  /\ state' = [state EXCEPT
       !.requestSequence = @ + 1,
       !.requestedThrough = state.requestSequence + 1,
       !.dagRevision = @ + 1]

AdvanceAdmission ==
  /\ state.admissionRevision < MaxAdmissionRevision
  /\ \E worker \in Workers :
       state.workerPhase[worker] \in {"Snapshot", "Evaluated", "Manifest"}
  /\ state' = [state EXCEPT !.admissionRevision = @ + 1]

\* @type: (Int) => Bool;
Launch(worker) ==
  /\ state.workerPhase[worker] = "Idle"
  /\ state.launchedThrough < state.requestedThrough
  /\ state.headRevision < MaxHeadRevision
  /\ state' = [state EXCEPT
       !.launchedThrough = state.requestedThrough,
       !.workerPhase[worker] = "Snapshot",
       !.workerCovered[worker] = state.requestedThrough,
       !.workerExpected[worker] = state.headRevision,
       !.workerCandidate[worker] = CandidateForRevision(state.headRevision)]

\* @type: (Int) => Bool;
Evaluate(worker) ==
  /\ state.workerPhase[worker] = "Snapshot"
  /\ state' = [state EXCEPT !.workerPhase[worker] = "Evaluated"]

\* @type: (Int) => Bool;
PrepareManifest(worker) ==
  /\ state.workerPhase[worker] = "Evaluated"
  /\ state' = [state EXCEPT !.workerPhase[worker] = "Manifest"]

\* @type: (Int) => Bool;
TryAppend(worker) ==
  LET current == state.headRevision IN
  LET expected == state.workerExpected[worker] IN
  LET fresh == expected = current IN
  LET unsafeStale == Defect = "StaleFinalizerCommit" /\ expected < current IN
  LET mayCommit == (fresh \/ unsafeStale) /\ current < MaxHeadRevision IN
  /\ state.workerPhase[worker] = "Manifest"
  /\ state.headRevision < MaxHeadRevision
  /\ state' = [state EXCEPT
       !.headRevision = IF mayCommit THEN @ + 1 ELSE @,
       !.ledger[current + 1] =
         IF mayCommit THEN state.workerCandidate[worker] ELSE @,
       !.workerPhase[worker] =
         IF mayCommit THEN "Committed" ELSE "Stale"]

\* @type: (Int) => Bool;
RetireCommitted(worker) ==
  /\ state.workerPhase[worker] = "Committed"
  /\ state' = [state EXCEPT
       !.completedThrough =
         IF state.workerCovered[worker] > @
         THEN state.workerCovered[worker]
         ELSE @,
       !.workerPhase[worker] = "Idle",
       !.workerCandidate[worker] = NoBlock]

\* @type: (Int) => Bool;
RetireStale(worker) ==
  /\ state.workerPhase[worker] = "Stale"
  /\ state.requestSequence < MaxRequestSequence
  /\ state' = [state EXCEPT
       !.requestSequence = @ + 1,
       !.requestedThrough = state.requestSequence + 1,
       !.workerPhase[worker] = "Idle",
       !.workerCandidate[worker] = NoBlock]

Next ==
  \/ PublishRequest
  \/ AdvanceAdmission
  \/ \E worker \in Workers : Launch(worker)
  \/ \E worker \in Workers : Evaluate(worker)
  \/ \E worker \in Workers : PrepareManifest(worker)
  \/ \E worker \in Workers : TryAppend(worker)
  \/ \E worker \in Workers : RetireCommitted(worker)
  \/ \E worker \in Workers : RetireStale(worker)

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ state.requestSequence \in 0..MaxRequestSequence
  /\ state.requestedThrough \in 0..MaxRequestSequence
  /\ state.launchedThrough \in 0..MaxRequestSequence
  /\ state.completedThrough \in 0..MaxRequestSequence
  /\ state.dagRevision \in 0..MaxDagRevision
  /\ state.admissionRevision \in 0..MaxAdmissionRevision
  /\ state.headRevision \in 0..MaxHeadRevision
  /\ state.ledger \in [1..MaxHeadRevision -> {NoBlock, "C", "R"}]
  /\ state.workerPhase \in [Workers -> WorkerPhases]
  /\ state.workerCovered \in [Workers -> 0..MaxRequestSequence]
  /\ state.workerExpected \in [Workers -> 0..MaxHeadRevision]
  /\ state.workerCandidate \in [Workers -> {NoBlock, "C", "R"}]

RequestCoverageOrder ==
  state.completedThrough <= state.launchedThrough /\
  state.launchedThrough <= state.requestedThrough /\
  state.requestedThrough <= state.requestSequence

HeadRevisionMatchesLedger ==
  \A revision \in 1..MaxHeadRevision :
    (revision <= state.headRevision) <=> state.ledger[revision] /= NoBlock

FinalizedFloorMonotoneCompatible ==
  /\ state.headRevision >= 1 => state.ledger[1] = "C"
  /\ state.headRevision >= 2 => state.ledger[2] = "R"

StaleFinalizerCannotCommit == FinalizedFloorMonotoneCompatible

OnlyPreparedWorkersReachAppendOutcome ==
  \A worker \in Workers :
    state.workerPhase[worker] \in {"Committed", "Stale"} =>
      state.workerCandidate[worker] \in {"C", "R"}

ParallelEvaluationDoesNotBlockAdmission ==
  state.admissionRevision \in 0..MaxAdmissionRevision

=============================================================================
