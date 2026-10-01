-------------------------- MODULE ReplaySupplySnapshot --------------------------
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS
  \* @type: Int;
  InitialSupply,
  \* @type: Seq(Int);
  Costs,
  \* @type: Int;
  AdmittedCount,
  \* @type: Seq(Int);
  RecordedEvents,
  \* @type: Int;
  QueryEvent,
  \* @type: Seq(Int);
  Roots,
  \* @type: Str;
  CertifiedProposer,
  \* @type: Str;
  Defect

DeployCount == Len(Costs)

PrefixCost(count) ==
  CASE count = 0 -> 0
    [] count = 1 -> Costs[1]
    [] count = 2 -> Costs[1] + Costs[2]
    [] OTHER -> Costs[1] + Costs[2] + Costs[3]

CandidateEvent(index) ==
  CASE index = 1 -> RecordedEvents[1]
    [] index = 2 -> RecordedEvents[2]
    [] OTHER -> QueryEvent

ASSUME
  /\ InitialSupply >= 0
  /\ DeployCount = 3
  /\ Costs[1] >= 0
  /\ Costs[2] >= 0
  /\ Costs[3] >= 0
  /\ AdmittedCount = 2
  /\ PrefixCost(AdmittedCount) <= InitialSupply
  /\ Costs[AdmittedCount + 1] > InitialSupply - PrefixCost(AdmittedCount)
  /\ Len(RecordedEvents) = AdmittedCount
  /\ RecordedEvents[1] >= 0
  /\ RecordedEvents[2] >= 0
  /\ QueryEvent >= 0
  /\ QueryEvent \notin {RecordedEvents[1], RecordedEvents[2]}
  /\ Len(Roots) = AdmittedCount + 1
  /\ Roots[1] >= 0
  /\ Roots[2] >= 0
  /\ Roots[3] >= 0
  /\ Cardinality({Roots[1], Roots[2], Roots[3]}) = 3
  /\ Defect \in {
       "None",
       "QueryAfterRig",
       "ReplayRuntimeSnapshot",
       "CaptureAfterRig",
       "WrongRoot",
       "WrongProposer",
       "RecordedBalance",
       "SnapshotOnlySettlement",
       "DeferredBurn"
     }

VARIABLES
  \* @type: Str;
  phase,
  \* @type: Int;
  playCursor,
  \* @type: Int;
  playSupply,
  \* @type: Int;
  admitted,
  \* @type: Int;
  deferred,
  \* @type: Seq(Int);
  committedCosts,
  \* @type: Seq(Int);
  committedEvents,
  \* @type: Int;
  replayCursor,
  \* @type: Int;
  replaySupply,
  \* @type: Int;
  replayBurned,
  \* @type: Int;
  replayRoot,
  \* @type: Seq(Int);
  replayTrace,
  \* @type: Bool;
  rigged,
  \* @type: Bool;
  snapshotPresent,
  \* @type: Int;
  snapshotBalance,
  \* @type: Int;
  snapshotRoot,
  \* @type: Str;
  snapshotProposer,
  \* @type: Bool;
  snapshotAuthenticated,
  \* @type: Bool;
  snapshotCapturedBeforeRig,
  \* @type: Str;
  snapshotRuntime,
  \* @type: Bool;
  settlementBypassed,
  \* @type: Int;
  deferredBurn

vars == <<
  phase, playCursor, playSupply, admitted, deferred, committedCosts,
  committedEvents, replayCursor, replaySupply, replayBurned, replayRoot,
  replayTrace, rigged, snapshotPresent, snapshotBalance, snapshotRoot,
  snapshotProposer, snapshotAuthenticated, snapshotCapturedBeforeRig,
  snapshotRuntime, settlementBypassed, deferredBurn
>>

ExpectedReplayCount ==
  IF phase = "Done" THEN AdmittedCount ELSE replayCursor - 1

ExpectedReplayTrace ==
  IF ExpectedReplayCount = 0
    THEN <<>>
    ELSE SubSeq(RecordedEvents, 1, ExpectedReplayCount)

Init ==
  /\ phase = "Play"
  /\ playCursor = 1
  /\ playSupply = InitialSupply
  /\ admitted = 0
  /\ deferred = 0
  /\ committedCosts = <<>>
  /\ committedEvents = <<>>
  /\ replayCursor = 1
  /\ replaySupply = InitialSupply
  /\ replayBurned = 0
  /\ replayRoot = Roots[1]
  /\ replayTrace = <<>>
  /\ rigged = FALSE
  /\ snapshotPresent = FALSE
  /\ snapshotBalance = 0
  /\ snapshotRoot = Roots[1]
  /\ snapshotProposer = CertifiedProposer
  /\ snapshotAuthenticated = FALSE
  /\ snapshotCapturedBeforeRig = FALSE
  /\ snapshotRuntime = "None"
  /\ settlementBypassed = FALSE
  /\ deferredBurn = 0

PlayCandidate ==
  /\ phase = "Play"
  /\ playCursor <= DeployCount
  /\ IF playSupply >= Costs[playCursor]
        THEN
          /\ playSupply' = playSupply - Costs[playCursor]
          /\ admitted' = admitted + 1
          /\ deferred' = deferred
          /\ committedCosts' = Append(committedCosts, Costs[playCursor])
          /\ committedEvents' = Append(committedEvents, CandidateEvent(playCursor))
          /\ playCursor' = playCursor + 1
          /\ phase' = "Play"
          /\ deferredBurn' = deferredBurn
        ELSE
          /\ playSupply' = playSupply
          /\ admitted' = admitted
          /\ deferred' = deferred + DeployCount - playCursor + 1
          /\ committedCosts' = committedCosts
          /\ committedEvents' = committedEvents
          /\ playCursor' = DeployCount + 1
          /\ phase' = "Snapshot"
          /\ deferredBurn' =
               IF Defect = "DeferredBurn"
                 THEN deferredBurn + Costs[playCursor]
                 ELSE deferredBurn
  /\ UNCHANGED <<
       replayCursor, replaySupply, replayBurned, replayRoot, replayTrace,
       rigged, snapshotPresent, snapshotBalance, snapshotRoot,
       snapshotProposer, snapshotAuthenticated, snapshotCapturedBeforeRig,
       snapshotRuntime, settlementBypassed
     >>

CaptureSnapshot ==
  /\ phase = "Snapshot"
  /\ replayCursor <= admitted
  /\ ~rigged
  /\ Defect # "CaptureAfterRig"
  /\ phase' = "Rig"
  /\ snapshotPresent' = TRUE
  /\ snapshotBalance' =
       IF Defect = "RecordedBalance" THEN InitialSupply ELSE replaySupply
  /\ snapshotRoot' =
       IF Defect = "WrongRoot" THEN Roots[Len(Roots)] ELSE replayRoot
  /\ snapshotProposer' =
       IF Defect = "WrongProposer" THEN "other-validator" ELSE CertifiedProposer
  /\ snapshotAuthenticated' = TRUE
  /\ snapshotCapturedBeforeRig' = TRUE
  /\ snapshotRuntime' =
       IF Defect = "ReplayRuntimeSnapshot" THEN "Replay" ELSE "Ordinary"
  /\ UNCHANGED <<
       playCursor, playSupply, admitted, deferred, committedCosts,
       committedEvents, replayCursor, replaySupply, replayBurned, replayRoot,
       replayTrace, rigged, settlementBypassed, deferredBurn
     >>

RigBeforeSnapshot ==
  /\ phase = "Snapshot"
  /\ replayCursor <= admitted
  /\ ~rigged
  /\ Defect = "CaptureAfterRig"
  /\ phase' = "RigLate"
  /\ rigged' = TRUE
  /\ UNCHANGED <<
       playCursor, playSupply, admitted, deferred, committedCosts,
       committedEvents, replayCursor, replaySupply, replayBurned, replayRoot,
       replayTrace, snapshotPresent, snapshotBalance, snapshotRoot,
       snapshotProposer, snapshotAuthenticated, snapshotCapturedBeforeRig,
       snapshotRuntime, settlementBypassed, deferredBurn
     >>

CaptureLateSnapshot ==
  /\ phase = "RigLate"
  /\ rigged
  /\ ~snapshotPresent
  /\ phase' = "Replay"
  /\ snapshotPresent' = TRUE
  /\ snapshotBalance' = replaySupply
  /\ snapshotRoot' = replayRoot
  /\ snapshotProposer' = CertifiedProposer
  /\ snapshotAuthenticated' = TRUE
  /\ snapshotCapturedBeforeRig' = FALSE
  /\ snapshotRuntime' = "Ordinary"
  /\ UNCHANGED <<
       playCursor, playSupply, admitted, deferred, committedCosts,
       committedEvents, replayCursor, replaySupply, replayBurned, replayRoot,
       replayTrace, rigged, settlementBypassed, deferredBurn
     >>

RigCertificate ==
  /\ phase = "Rig"
  /\ snapshotPresent
  /\ phase' = "Replay"
  /\ rigged' = TRUE
  /\ UNCHANGED <<
       playCursor, playSupply, admitted, deferred, committedCosts,
       committedEvents, replayCursor, replaySupply, replayBurned, replayRoot,
       replayTrace, snapshotPresent, snapshotBalance, snapshotRoot,
       snapshotProposer, snapshotAuthenticated, snapshotCapturedBeforeRig,
       snapshotRuntime, settlementBypassed, deferredBurn
     >>

ReplayCommitted ==
  /\ phase = "Replay"
  /\ replayCursor <= admitted
  /\ rigged
  /\ snapshotPresent
  /\ snapshotAuthenticated
  /\ snapshotCapturedBeforeRig
  /\ snapshotRuntime = "Ordinary"
  /\ snapshotRoot = replayRoot
  /\ snapshotProposer = CertifiedProposer
  /\ snapshotBalance >= committedCosts[replayCursor]
  /\ replaySupply >= committedCosts[replayCursor]
  /\ replaySupply' =
       IF Defect = "SnapshotOnlySettlement"
         THEN replaySupply
         ELSE replaySupply - committedCosts[replayCursor]
  /\ replayBurned' =
       IF Defect = "SnapshotOnlySettlement"
         THEN replayBurned
         ELSE replayBurned + committedCosts[replayCursor]
  /\ replayRoot' = Roots[replayCursor + 1]
  /\ replayTrace' = replayTrace \o
       IF Defect = "QueryAfterRig"
         THEN <<QueryEvent, committedEvents[replayCursor]>>
         ELSE <<committedEvents[replayCursor]>>
  /\ settlementBypassed' =
       (settlementBypassed \/ (Defect = "SnapshotOnlySettlement"))
  /\ rigged' = FALSE
  /\ snapshotPresent' = FALSE
  /\ snapshotAuthenticated' = FALSE
  /\ snapshotCapturedBeforeRig' = FALSE
  /\ snapshotRuntime' = "None"
  /\ IF replayCursor = admitted
        THEN
          /\ phase' = "Done"
          /\ replayCursor' = replayCursor + 1
        ELSE
          /\ phase' = "Snapshot"
          /\ replayCursor' = replayCursor + 1
  /\ UNCHANGED <<
       playCursor, playSupply, admitted, deferred, committedCosts,
       committedEvents, snapshotBalance, snapshotRoot, snapshotProposer,
       deferredBurn
     >>

Terminal ==
  /\ phase = "Done"
  /\ UNCHANGED vars

Next ==
  PlayCandidate \/ CaptureSnapshot \/ RigBeforeSnapshot \/ CaptureLateSnapshot
    \/ RigCertificate \/ ReplayCommitted \/ Terminal

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(PlayCandidate)
  /\ WF_vars(CaptureSnapshot)
  /\ WF_vars(RigBeforeSnapshot)
  /\ WF_vars(CaptureLateSnapshot)
  /\ WF_vars(RigCertificate)
  /\ WF_vars(ReplayCommitted)

TypeOK ==
  /\ phase \in {"Play", "Snapshot", "Rig", "RigLate", "Replay", "Done"}
  /\ playCursor \in 1..(DeployCount + 1)
  /\ playSupply >= 0
  /\ admitted \in 0..AdmittedCount
  /\ deferred \in 0..DeployCount
  /\ Len(committedCosts) \in 0..AdmittedCount
  /\ Len(committedEvents) \in 0..AdmittedCount
  /\ replayCursor \in 1..(AdmittedCount + 1)
  /\ replaySupply >= 0
  /\ replayBurned >= 0
  /\ replayRoot \in {Roots[1], Roots[2], Roots[3]}
  /\ Len(replayTrace) \in 0..(2 * AdmittedCount)
  /\ rigged \in BOOLEAN
  /\ snapshotPresent \in BOOLEAN
  /\ snapshotBalance >= 0
  /\ snapshotRoot \in {Roots[1], Roots[2], Roots[3]}
  /\ snapshotProposer \in {CertifiedProposer, "other-validator"}
  /\ snapshotAuthenticated \in BOOLEAN
  /\ snapshotCapturedBeforeRig \in BOOLEAN
  /\ snapshotRuntime \in {"None", "Ordinary", "Replay"}
  /\ settlementBypassed \in BOOLEAN
  /\ deferredBurn >= 0

PlayPrefixIsExact ==
  /\ Len(committedCosts) = admitted
  /\ Len(committedEvents) = admitted
  /\ committedCosts = IF admitted = 0 THEN <<>> ELSE SubSeq(Costs, 1, admitted)
  /\ committedEvents =
       IF admitted = 0 THEN <<>> ELSE SubSeq(RecordedEvents, 1, admitted)
  /\ playSupply = InitialSupply - PrefixCost(admitted)

MaximalPrefixAgreement ==
  phase # "Play" =>
    /\ admitted = AdmittedCount
    /\ deferred = DeployCount - AdmittedCount
    /\ committedCosts = SubSeq(Costs, 1, AdmittedCount)
    /\ committedEvents = RecordedEvents

SnapshotsAreAuthenticated == snapshotPresent => snapshotAuthenticated

SnapshotsUseCurrentRoot == snapshotPresent => snapshotRoot = replayRoot

SnapshotsUseCertifiedProposer ==
  snapshotPresent => snapshotProposer = CertifiedProposer

SnapshotsMatchActualFuel == snapshotPresent => snapshotBalance = replaySupply

SnapshotsPrecedeRigging == snapshotPresent => snapshotCapturedBeforeRig

SnapshotsUseOrdinaryRuntime == snapshotPresent => snapshotRuntime = "Ordinary"

ReplayUsesAuthenticatedSnapshots ==
  phase = "Replay" =>
    /\ rigged
    /\ snapshotPresent
    /\ snapshotAuthenticated
    /\ snapshotCapturedBeforeRig
    /\ snapshotRuntime = "Ordinary"
    /\ snapshotRoot = replayRoot
    /\ snapshotProposer = CertifiedProposer
    /\ snapshotBalance = replaySupply

ExactRecordedReplayTrace == replayTrace = ExpectedReplayTrace

ReplayConservesSupply == replaySupply + replayBurned = InitialSupply

ReplaySettlementIsExact ==
  replaySupply = InitialSupply - PrefixCost(ExpectedReplayCount)

ActualSettlementRequired == ~settlementBypassed

DeferredCandidatesDoNotBurn == deferredBurn = 0

EventuallyReplayCompletes == <>(phase = "Done")

=============================================================================
