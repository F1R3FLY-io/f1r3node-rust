----------------------------- MODULE Recovery -----------------------------
EXTENDS Naturals, TLC
CONSTANTS Scenarios, MaxObservations,
          ConflateRecoveryLanes, CollapseOccurrenceIdentity, AssumePauseApplied
VARIABLES phase, laneMatched, pauseObserved, observations, occurrences, duplicates, verdict
vars == <<phase, laneMatched, pauseObserved, observations, occurrences, duplicates, verdict>>
Cases == 1..Scenarios
Init ==
    /\ phase = [s \in Cases |-> "request"]
    /\ laneMatched = [s \in Cases |-> TRUE]
    /\ pauseObserved = [s \in Cases |-> TRUE]
    /\ observations = [s \in Cases |-> 0]
    /\ occurrences = [s \in Cases |-> 0]
    /\ duplicates = [s \in Cases |-> 0]
    /\ verdict = [s \in Cases |-> "pending"]
Choose(s) ==
    /\ phase[s] = "request"
    /\ \E lane \in BOOLEAN, ack \in BOOLEAN:
        /\ laneMatched' = [laneMatched EXCEPT ![s] = lane]
        /\ pauseObserved' = [pauseObserved EXCEPT ![s] = ack]
    /\ phase' = [phase EXCEPT ![s] = "collect"]
    /\ UNCHANGED <<observations, occurrences, duplicates, verdict>>
Collect(s) ==
    /\ phase[s] = "collect"
    /\ observations[s] < MaxObservations
    /\ observations' = [observations EXCEPT ![s] = @ + 1]
    /\ UNCHANGED <<phase, laneMatched, pauseObserved, occurrences, duplicates, verdict>>
Classify(s) ==
    /\ phase[s] = "collect"
    /\ observations[s] = MaxObservations
    /\ occurrences' = [occurrences EXCEPT ![s] = IF CollapseOccurrenceIdentity THEN 1 ELSE 2]
    /\ duplicates' = [duplicates EXCEPT ![s] = IF CollapseOccurrenceIdentity THEN 2 ELSE 1]
    /\ verdict' = [verdict EXCEPT ![s] =
        IF ~laneMatched[s] /\ ~ConflateRecoveryLanes THEN "incomplete"
        ELSE IF ~pauseObserved[s] /\ ~AssumePauseApplied THEN "incomplete"
        ELSE "passed"]
    /\ phase' = [phase EXCEPT ![s] = "classified"]
    /\ UNCHANGED <<laneMatched, pauseObserved, observations>>
Next == \E s \in Cases: Choose(s) \/ Collect(s) \/ Classify(s)
Spec == Init /\ [][Next]_vars
TypeOK ==
    /\ phase \in [Cases -> {"request", "collect", "classified"}]
    /\ laneMatched \in [Cases -> BOOLEAN]
    /\ pauseObserved \in [Cases -> BOOLEAN]
    /\ observations \in [Cases -> 0..MaxObservations]
    /\ occurrences \in [Cases -> 0..2]
    /\ duplicates \in [Cases -> 0..2]
    /\ verdict \in [Cases -> {"pending", "incomplete", "passed"}]
LaneLabelsPreserved == \A s \in Cases: verdict[s] = "passed" => laneMatched[s]
OccurrenceCountsPreserved ==
    \A s \in Cases: phase[s] = "classified" /\ laneMatched[s]
        => occurrences[s] = 2 /\ duplicates[s] = 1
PauseCoverageAcknowledged == \A s \in Cases: verdict[s] = "passed" => pauseObserved[s]
=============================================================================
