--------------------------- MODULE Publication ----------------------------
EXTENDS Naturals, TLC
CONSTANTS Scenarios, MaxObservations,
          AssumeCrashApplied, MixRestartObservations, HideTupleMismatch
VARIABLES phase, acknowledged, restartMatched, tupleComplete, observations, verdict
vars == <<phase, acknowledged, restartMatched, tupleComplete, observations, verdict>>
Cases == 1..Scenarios
Init ==
    /\ phase = [s \in Cases |-> "request"]
    /\ acknowledged = [s \in Cases |-> TRUE]
    /\ restartMatched = [s \in Cases |-> TRUE]
    /\ tupleComplete = [s \in Cases |-> TRUE]
    /\ observations = [s \in Cases |-> 0]
    /\ verdict = [s \in Cases |-> "pending"]
Choose(s) ==
    /\ phase[s] = "request"
    /\ \E ack \in BOOLEAN, linked \in BOOLEAN, complete \in BOOLEAN:
        /\ acknowledged' = [acknowledged EXCEPT ![s] = ack]
        /\ restartMatched' = [restartMatched EXCEPT ![s] = linked]
        /\ tupleComplete' = [tupleComplete EXCEPT ![s] = complete]
    /\ phase' = [phase EXCEPT ![s] = "collect"]
    /\ UNCHANGED <<observations, verdict>>
Collect(s) ==
    /\ phase[s] = "collect"
    /\ observations[s] < MaxObservations
    /\ observations' = [observations EXCEPT ![s] = @ + 1]
    /\ UNCHANGED <<phase, acknowledged, restartMatched, tupleComplete, verdict>>
Classify(s) ==
    /\ phase[s] = "collect"
    /\ observations[s] = MaxObservations
    /\ verdict' = [verdict EXCEPT ![s] =
        IF ~restartMatched[s] /\ ~MixRestartObservations THEN "incomplete"
        ELSE IF ~tupleComplete[s] /\ ~HideTupleMismatch THEN "product_failure"
        ELSE IF ~acknowledged[s] /\ ~AssumeCrashApplied THEN "incomplete"
        ELSE "passed"]
    /\ phase' = [phase EXCEPT ![s] = "classified"]
    /\ UNCHANGED <<acknowledged, restartMatched, tupleComplete, observations>>
Next == \E s \in Cases: Choose(s) \/ Collect(s) \/ Classify(s)
Spec == Init /\ [][Next]_vars
TypeOK ==
    /\ phase \in [Cases -> {"request", "collect", "classified"}]
    /\ acknowledged \in [Cases -> BOOLEAN]
    /\ restartMatched \in [Cases -> BOOLEAN]
    /\ tupleComplete \in [Cases -> BOOLEAN]
    /\ observations \in [Cases -> 0..MaxObservations]
    /\ verdict \in [Cases -> {"pending", "incomplete", "product_failure", "passed"}]
FaultAcknowledged ==
    \A s \in Cases: verdict[s] = "passed" => acknowledged[s]
RestartIdentityMatched ==
    \A s \in Cases:
        phase[s] = "classified" /\ ~restartMatched[s] => verdict[s] = "incomplete"
TornTupleReported ==
    \A s \in Cases:
        phase[s] = "classified" /\ restartMatched[s] /\ ~tupleComplete[s]
        => verdict[s] = "product_failure"
=============================================================================
