------------------------- MODULE AuthorityFinality -------------------------
EXTENDS Naturals, TLC

CONSTANTS Scenarios, MaxObservations,
          PairDifferentDags, AcceptMissingFinality, SuppressHeadMismatch

VARIABLES phase, inputsMatch, finalityPresent, headsMatch, observations, verdict

vars == <<phase, inputsMatch, finalityPresent, headsMatch, observations, verdict>>
Cases == 1..Scenarios

Init ==
    /\ phase = [s \in Cases |-> "request"]
    /\ inputsMatch = [s \in Cases |-> TRUE]
    /\ finalityPresent = [s \in Cases |-> TRUE]
    /\ headsMatch = [s \in Cases |-> TRUE]
    /\ observations = [s \in Cases |-> 0]
    /\ verdict = [s \in Cases |-> "pending"]

Choose(s) ==
    /\ phase[s] = "request"
    /\ \E same \in BOOLEAN, present \in BOOLEAN, heads \in BOOLEAN:
        /\ inputsMatch' = [inputsMatch EXCEPT ![s] = same]
        /\ finalityPresent' = [finalityPresent EXCEPT ![s] = present]
        /\ headsMatch' = [headsMatch EXCEPT ![s] = heads]
    /\ phase' = [phase EXCEPT ![s] = "collect"]
    /\ UNCHANGED <<observations, verdict>>

Collect(s) ==
    /\ phase[s] = "collect"
    /\ observations[s] < MaxObservations
    /\ observations' = [observations EXCEPT ![s] = @ + 1]
    /\ UNCHANGED <<phase, inputsMatch, finalityPresent, headsMatch, verdict>>

Classify(s) ==
    /\ phase[s] = "collect"
    /\ observations[s] = MaxObservations
    /\ verdict' = [verdict EXCEPT ![s] =
        IF ~inputsMatch[s] /\ ~PairDifferentDags THEN "invalid_input"
        ELSE IF ~headsMatch[s] /\ ~SuppressHeadMismatch THEN "product_failure"
        ELSE IF ~finalityPresent[s] /\ ~AcceptMissingFinality THEN "incomplete"
        ELSE "passed"]
    /\ phase' = [phase EXCEPT ![s] = "classified"]
    /\ UNCHANGED <<inputsMatch, finalityPresent, headsMatch, observations>>

Next == \E s \in Cases: Choose(s) \/ Collect(s) \/ Classify(s)
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in [Cases -> {"request", "collect", "classified"}]
    /\ inputsMatch \in [Cases -> BOOLEAN]
    /\ finalityPresent \in [Cases -> BOOLEAN]
    /\ headsMatch \in [Cases -> BOOLEAN]
    /\ observations \in [Cases -> 0..MaxObservations]
    /\ verdict \in [Cases -> {"pending", "invalid_input", "product_failure", "incomplete", "passed"}]

MismatchedInputDetected ==
    \A s \in Cases:
        phase[s] = "classified" /\ ~inputsMatch[s] => verdict[s] = "invalid_input"

MissingFinalityDetected ==
    \A s \in Cases:
        phase[s] = "classified" /\ inputsMatch[s] /\ headsMatch[s] /\ ~finalityPresent[s]
        => verdict[s] = "incomplete"

HeadMismatchReported ==
    \A s \in Cases:
        phase[s] = "classified" /\ inputsMatch[s] /\ ~headsMatch[s]
        => verdict[s] = "product_failure"
=============================================================================
