------------------------------ MODULE Slashing ------------------------------
EXTENDS Naturals, Sequences, TLC
CONSTANTS Scenarios, MaxObservations,
          ReuseRequestedOrder, DropEpochIdentity, SuppressSlashMismatch
ASSUME MaxObservations = 3
VARIABLES phase, actualOrder, epochMatches, authorizationMatches,
          observations, recordedOrder, verdict
vars == <<phase, actualOrder, epochMatches, authorizationMatches,
          observations, recordedOrder, verdict>>
Cases == 1..Scenarios
RequestedOrder == <<1, 2>>
Orders == {<<1, 2>>, <<2, 1>>}
Init ==
    /\ phase = [s \in Cases |-> "request"]
    /\ actualOrder = [s \in Cases |-> RequestedOrder]
    /\ epochMatches = [s \in Cases |-> TRUE]
    /\ authorizationMatches = [s \in Cases |-> TRUE]
    /\ observations = [s \in Cases |-> 0]
    /\ recordedOrder = [s \in Cases |-> <<>>]
    /\ verdict = [s \in Cases |-> "pending"]
Choose(s) ==
    /\ phase[s] = "request"
    /\ \E order \in Orders, epoch \in BOOLEAN, authorization \in BOOLEAN:
        /\ actualOrder' = [actualOrder EXCEPT ![s] = order]
        /\ epochMatches' = [epochMatches EXCEPT ![s] = epoch]
        /\ authorizationMatches' = [authorizationMatches EXCEPT ![s] = authorization]
    /\ phase' = [phase EXCEPT ![s] = "collect"]
    /\ UNCHANGED <<observations, recordedOrder, verdict>>
Collect(s) ==
    /\ phase[s] = "collect"
    /\ observations[s] < MaxObservations
    /\ recordedOrder' = [recordedOrder EXCEPT ![s] =
        IF observations[s] < 2
        THEN Append(@, IF ReuseRequestedOrder
                       THEN RequestedOrder[observations[s] + 1]
                       ELSE actualOrder[s][observations[s] + 1])
        ELSE @]
    /\ observations' = [observations EXCEPT ![s] = @ + 1]
    /\ UNCHANGED <<phase, actualOrder, epochMatches, authorizationMatches, verdict>>
Classify(s) ==
    /\ phase[s] = "collect"
    /\ observations[s] = MaxObservations
    /\ verdict' = [verdict EXCEPT ![s] =
        IF ~epochMatches[s] /\ ~DropEpochIdentity THEN "incomplete"
        ELSE IF ~authorizationMatches[s] /\ ~SuppressSlashMismatch THEN "product_failure"
        ELSE IF recordedOrder[s] # RequestedOrder THEN "incomplete"
        ELSE "passed"]
    /\ phase' = [phase EXCEPT ![s] = "classified"]
    /\ UNCHANGED <<actualOrder, epochMatches, authorizationMatches, observations, recordedOrder>>
Next == \E s \in Cases: Choose(s) \/ Collect(s) \/ Classify(s)
Spec == Init /\ [][Next]_vars
TypeOK ==
    /\ phase \in [Cases -> {"request", "collect", "classified"}]
    /\ actualOrder \in [Cases -> Orders]
    /\ epochMatches \in [Cases -> BOOLEAN]
    /\ authorizationMatches \in [Cases -> BOOLEAN]
    /\ observations \in [Cases -> 0..MaxObservations]
    /\ recordedOrder \in [Cases -> {<<>>, <<1>>, <<2>>, <<1, 2>>, <<2, 1>>}]
    /\ verdict \in [Cases -> {"pending", "incomplete", "product_failure", "passed"}]
EvidenceOrderRecorded ==
    \A s \in Cases: phase[s] = "classified" => recordedOrder[s] = actualOrder[s]
EpochCorrelationRequired ==
    \A s \in Cases: phase[s] = "classified" /\ ~epochMatches[s]
        => verdict[s] = "incomplete"
AuthorizationMismatchReported ==
    \A s \in Cases: phase[s] = "classified" /\ epochMatches[s] /\ ~authorizationMatches[s]
        => verdict[s] = "product_failure"
=============================================================================
