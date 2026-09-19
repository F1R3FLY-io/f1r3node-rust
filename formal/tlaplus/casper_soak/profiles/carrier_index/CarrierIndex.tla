--------------------------- MODULE CarrierIndex ---------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS Scenarios, MaxObservations,
          AssumeIndexEngaged, CompareDifferentWindows, ZeroMissingCounters
ASSUME MaxObservations = 3
VARIABLES phase, matched, engaged, known, observations, verdict, counter, workReported
vars == <<phase, matched, engaged, known, observations, verdict, counter, workReported>>
Cases == 1..Scenarios
Init ==
    /\ phase = [s \in Cases |-> "request"]
    /\ matched = [s \in Cases |-> TRUE]
    /\ engaged = [s \in Cases |-> TRUE]
    /\ known = [s \in Cases |-> TRUE]
    /\ observations = [s \in Cases |-> 0]
    /\ verdict = [s \in Cases |-> "pending"]
    /\ counter = [s \in Cases |-> "unknown"]
    /\ workReported = [s \in Cases |-> FALSE]
Admit(s) ==
    /\ phase[s] = "request"
    /\ \E m \in BOOLEAN, e \in BOOLEAN, k \in BOOLEAN:
        /\ matched' = [matched EXCEPT ![s] = m]
        /\ engaged' = [engaged EXCEPT ![s] = e]
        /\ known' = [known EXCEPT ![s] = k]
        /\ phase' = [phase EXCEPT ![s] =
            IF ~m /\ ~CompareDifferentWindows THEN "classified" ELSE "collect"]
        /\ verdict' = [verdict EXCEPT ![s] =
            IF ~m /\ ~CompareDifferentWindows THEN "invalid_input" ELSE "pending"]
    /\ UNCHANGED <<observations, counter, workReported>>
Collect(s) ==
    /\ phase[s] = "collect"
    /\ observations[s] < MaxObservations
    /\ observations' = [observations EXCEPT ![s] = @ + 1]
    /\ UNCHANGED <<phase, matched, engaged, known, verdict, counter, workReported>>
Classify(s) ==
    /\ phase[s] = "collect"
    /\ observations[s] = MaxObservations
    /\ LET pathKnown == engaged[s] \/ AssumeIndexEngaged
           countKnown == known[s] \/ ZeroMissingCounters
       IN /\ verdict' = [verdict EXCEPT ![s] =
                 IF pathKnown /\ countKnown THEN "passed" ELSE "incomplete"]
          /\ counter' = [counter EXCEPT ![s] =
                 IF pathKnown /\ countKnown THEN "observed_zero" ELSE "unknown"]
          /\ workReported' = [workReported EXCEPT ![s] = pathKnown /\ countKnown]
    /\ phase' = [phase EXCEPT ![s] = "classified"]
    /\ UNCHANGED <<matched, engaged, known, observations>>
Next == \E s \in Cases: Admit(s) \/ Collect(s) \/ Classify(s)
Spec == Init /\ [][Next]_vars
TypeOK ==
    /\ phase \in [Cases -> {"request", "collect", "classified"}]
    /\ matched \in [Cases -> BOOLEAN]
    /\ engaged \in [Cases -> BOOLEAN]
    /\ known \in [Cases -> BOOLEAN]
    /\ observations \in [Cases -> 0..MaxObservations]
    /\ verdict \in [Cases -> {"pending", "invalid_input", "incomplete", "passed"}]
    /\ counter \in [Cases -> {"unknown", "observed_zero"}]
    /\ workReported \in [Cases -> BOOLEAN]
PathEngagementObserved ==
    \A s \in Cases: phase[s] = "classified" /\ matched[s] /\ ~engaged[s]
        => verdict[s] = "incomplete" /\ ~workReported[s]
CarrierInputsMatched ==
    \A s \in Cases: phase[s] = "classified" /\ ~matched[s]
        => verdict[s] = "invalid_input" /\ observations[s] = 0 /\ ~workReported[s]
MissingCountersUnknown ==
    \A s \in Cases: phase[s] = "classified" /\ matched[s] /\ ~known[s]
        => verdict[s] = "incomplete" /\ counter[s] = "unknown"
=============================================================================
