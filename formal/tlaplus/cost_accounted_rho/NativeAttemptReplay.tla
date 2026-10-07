-------------------- MODULE NativeAttemptReplay --------------------
EXTENDS Naturals, FiniteSets, Sequences

CONSTANTS Workers, Limit, RecordDenied, UseRecordedDecision

VARIABLES phase, costs, prepared, play, trace, used, replay, replayUsed
vars == <<phase, costs, prepared, play, trace, used, replay, replayUsed>>
Outcomes == {"Unseen", "Accepted", "Denied"}
TraceWorkers == {trace[i] : i \in 1..Len(trace)}

RECURSIVE Charge(_, _)
Charge(workers, outcomes) ==
  IF workers = {} THEN 0
  ELSE LET w == CHOOSE actor \in workers : TRUE
       IN (IF outcomes[w] = "Accepted" THEN costs[w] ELSE 0)
          + Charge(workers \ {w}, outcomes)

Init ==
  /\ phase = "Play"
  /\ costs \in [Workers -> 0..Limit]
  /\ prepared = {}
  /\ play = [w \in Workers |-> "Unseen"]
  /\ trace = <<>>
  /\ used = 0
  /\ replay = [w \in Workers |-> "Unseen"]
  /\ replayUsed = 0

Prepare(w) ==
  /\ phase = "Play"
  /\ w \notin prepared
  /\ prepared' = prepared \cup {w}
  /\ UNCHANGED <<phase, costs, play, trace, used, replay, replayUsed>>

Reserve(w) ==
  /\ phase = "Play"
  /\ w \in prepared
  /\ play[w] = "Unseen"
  /\ LET accept == used + costs[w] <= Limit
     IN /\ play' = [play EXCEPT ![w] = IF accept THEN "Accepted" ELSE "Denied"]
        /\ used' = IF accept THEN used + costs[w] ELSE used
        /\ trace' = IF accept \/ RecordDenied THEN Append(trace, w) ELSE trace
  /\ UNCHANGED <<phase, costs, prepared, replay, replayUsed>>

Seal ==
  /\ phase = "Play"
  /\ \A w \in Workers : play[w] # "Unseen"
  /\ phase' = "Replay"
  /\ UNCHANGED <<costs, prepared, play, trace, used, replay, replayUsed>>

Replay(w) ==
  /\ phase = "Replay"
  /\ w \in TraceWorkers
  /\ replay[w] = "Unseen"
  /\ LET accept == IF UseRecordedDecision THEN play[w] = "Accepted"
                   ELSE replayUsed + costs[w] <= Limit
     IN /\ replay' = [replay EXCEPT ![w] = IF accept THEN "Accepted" ELSE "Denied"]
        /\ replayUsed' = IF accept THEN replayUsed + costs[w] ELSE replayUsed
  /\ UNCHANGED <<phase, costs, prepared, play, trace, used>>

Finish ==
  /\ phase = "Replay"
  /\ \A w \in TraceWorkers : replay[w] # "Unseen"
  /\ phase' = "Done"
  /\ UNCHANGED <<costs, prepared, play, trace, used, replay, replayUsed>>

Next == (\E w \in Workers : Prepare(w) \/ Reserve(w) \/ Replay(w)) \/ Seal \/ Finish
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in {"Play", "Replay", "Done"}
  /\ costs \in [Workers -> 0..Limit]
  /\ prepared \subseteq Workers
  /\ play \in [Workers -> Outcomes]
  /\ replay \in [Workers -> Outcomes]
  /\ trace \in Seq(Workers)
  /\ Len(trace) = Cardinality(TraceWorkers)
  /\ used \in Nat
  /\ replayUsed \in Nat

ExactUsage == used = Charge(Workers, play) /\ replayUsed = Charge(Workers, replay)
HardCeiling == used <= Limit /\ replayUsed <= Limit
RecordedAttemptCoverage == phase # "Play" => TraceWorkers = Workers
ReplayDecisionAgreement == phase = "Done" => replay = play
ReplayUsageAgreement == phase = "Done" => replayUsed = used
IndependentPreparationEnabled == phase = "Play" =>
  \A w \in Workers \ prepared : ENABLED Prepare(w)
IndependentReplayEnabled == phase = "Replay" =>
  \A w \in TraceWorkers : replay[w] = "Unseen" => ENABLED Replay(w)
=============================================================================
