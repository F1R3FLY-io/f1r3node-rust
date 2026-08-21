-------------------------- MODULE ReplaySupplySnapshot --------------------------
EXTENDS Naturals, Sequences, TLC

CONSTANTS InitialSupply, Costs, RecordedEvents, QueryEvent, ReplayReadsLiveState

RECURSIVE SeqSum(_)
SeqSum(sequence) ==
  IF Len(sequence) = 0
    THEN 0
    ELSE Head(sequence) + SeqSum(Tail(sequence))

DeployCount == Len(Costs)

ASSUME /\ InitialSupply \in Nat
       /\ Costs \in Seq(Nat)
       /\ Len(Costs) > 0
       /\ SeqSum(Costs) <= InitialSupply
       /\ RecordedEvents \in Seq(Nat)
       /\ Len(RecordedEvents) = DeployCount
       /\ QueryEvent \in Nat
       /\ QueryEvent \notin {RecordedEvents[index] : index \in 1..DeployCount}
       /\ ReplayReadsLiveState \in BOOLEAN

ExpectedPre(index) ==
  InitialSupply - IF index = 1 THEN 0 ELSE SeqSum(SubSeq(Costs, 1, index - 1))

ExpectedTrace(cursor) ==
  IF cursor = 1 THEN <<>> ELSE SubSeq(RecordedEvents, 1, cursor - 1)

VARIABLES phase, captureCursor, captureSupply, snapshots,
          replayCursor, replaySupply, replayTrace

vars == <<phase, captureCursor, captureSupply, snapshots,
          replayCursor, replaySupply, replayTrace>>

Init ==
  /\ phase = "Capture"
  /\ captureCursor = 1
  /\ captureSupply = InitialSupply
  /\ snapshots = <<>>
  /\ replayCursor = 1
  /\ replaySupply = InitialSupply
  /\ replayTrace = <<>>

Capture ==
  /\ phase = "Capture"
  /\ captureCursor <= DeployCount
  /\ snapshots' = Append(snapshots, captureSupply)
  /\ captureSupply' = captureSupply - Costs[captureCursor]
  /\ IF captureCursor = DeployCount
        THEN /\ phase' = "Replay"
             /\ captureCursor' = captureCursor
        ELSE /\ phase' = "Capture"
             /\ captureCursor' = captureCursor + 1
  /\ UNCHANGED <<replayCursor, replaySupply, replayTrace>>

Replay ==
  /\ phase = "Replay"
  /\ replayCursor <= DeployCount
  /\ replaySupply = snapshots[replayCursor]
  /\ replaySupply' = replaySupply - Costs[replayCursor]
  /\ replayTrace' = replayTrace \o
       IF ReplayReadsLiveState
         THEN <<QueryEvent, RecordedEvents[replayCursor]>>
         ELSE <<RecordedEvents[replayCursor]>>
  /\ IF replayCursor = DeployCount
        THEN /\ phase' = "Done"
             /\ replayCursor' = replayCursor + 1
        ELSE /\ phase' = "Replay"
             /\ replayCursor' = replayCursor + 1
  /\ UNCHANGED <<captureCursor, captureSupply, snapshots>>

Next == Capture \/ Replay

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(Capture)
  /\ WF_vars(Replay)

TypeOK ==
  /\ phase \in {"Capture", "Replay", "Done"}
  /\ captureCursor \in 1..DeployCount
  /\ captureSupply \in Nat
  /\ snapshots \in Seq(Nat)
  /\ replayCursor \in 1..(DeployCount + 1)
  /\ replaySupply \in Nat
  /\ replayTrace \in Seq(Nat)

SnapshotsAreAuthenticated ==
  \A index \in 1..Len(snapshots) : snapshots[index] = ExpectedPre(index)

ReplayUsesAuthenticatedSnapshots ==
  phase = "Replay" => replaySupply = snapshots[replayCursor]

ExactRecordedReplayTrace == replayTrace = ExpectedTrace(replayCursor)

ReplayConservesSupply == replaySupply = ExpectedPre(replayCursor)

EventuallyReplayCompletes == <>(phase = "Done")

=============================================================================
