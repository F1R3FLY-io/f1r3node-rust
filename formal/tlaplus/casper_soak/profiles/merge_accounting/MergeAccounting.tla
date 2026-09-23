------------------------- MODULE MergeAccounting --------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS Scenarios, MaxObservations,
          DeduplicateByEffectValue, TreatMissingSettlementAsZero, MixProtocolProfiles
ASSUME MaxObservations = 3
VARIABLES phase, compatible, settlementKnown, observations, seen, measured, duplicates, verdict
vars == <<phase, compatible, settlementKnown, observations, seen, measured, duplicates, verdict>>
Cases == 1..Scenarios
IdentityAt(i) == IF i = 2 THEN 2 ELSE 1
Init ==
    /\ phase = [s \in Cases |-> "request"]
    /\ compatible = [s \in Cases |-> TRUE]
    /\ settlementKnown = [s \in Cases |-> TRUE]
    /\ observations = [s \in Cases |-> 0]
    /\ seen = [s \in Cases |-> {}]
    /\ measured = [s \in Cases |-> 0]
    /\ duplicates = [s \in Cases |-> 0]
    /\ verdict = [s \in Cases |-> "pending"]
Choose(s) ==
    /\ phase[s] = "request"
    /\ \E c \in BOOLEAN, known \in BOOLEAN:
        /\ compatible' = [compatible EXCEPT ![s] = c]
        /\ settlementKnown' = [settlementKnown EXCEPT ![s] = known]
    /\ phase' = [phase EXCEPT ![s] = "collect"]
    /\ UNCHANGED <<observations, seen, measured, duplicates, verdict>>
Collect(s) ==
    /\ phase[s] = "collect"
    /\ observations[s] < MaxObservations
    /\ LET key == IF DeduplicateByEffectValue THEN 0 ELSE IdentityAt(observations[s] + 1)
       IN /\ seen' = [seen EXCEPT ![s] = @ \cup {key}]
          /\ measured' = [measured EXCEPT ![s] = @ + IF key \in seen[s] THEN 0 ELSE 1]
          /\ duplicates' = [duplicates EXCEPT ![s] = @ + IF key \in seen[s] THEN 1 ELSE 0]
    /\ observations' = [observations EXCEPT ![s] = @ + 1]
    /\ UNCHANGED <<phase, compatible, settlementKnown, verdict>>
Classify(s) ==
    /\ phase[s] = "collect"
    /\ observations[s] = MaxObservations
    /\ verdict' = [verdict EXCEPT ![s] =
        IF ~compatible[s] /\ ~MixProtocolProfiles THEN "invalid_input"
        ELSE IF ~settlementKnown[s] /\ ~TreatMissingSettlementAsZero THEN "incomplete"
        ELSE "passed"]
    /\ phase' = [phase EXCEPT ![s] = "classified"]
    /\ UNCHANGED <<compatible, settlementKnown, observations, seen, measured, duplicates>>
Next == \E s \in Cases: Choose(s) \/ Collect(s) \/ Classify(s)
Spec == Init /\ [][Next]_vars
TypeOK ==
    /\ phase \in [Cases -> {"request", "collect", "classified"}]
    /\ compatible \in [Cases -> BOOLEAN]
    /\ settlementKnown \in [Cases -> BOOLEAN]
    /\ observations \in [Cases -> 0..MaxObservations]
    /\ seen \in [Cases -> SUBSET {0, 1, 2}]
    /\ measured \in [Cases -> 0..MaxObservations]
    /\ duplicates \in [Cases -> 0..MaxObservations]
    /\ verdict \in [Cases -> {"pending", "invalid_input", "incomplete", "passed"}]
MultiplicityMeasured ==
    \A s \in Cases: phase[s] = "classified" /\ compatible[s]
        => measured[s] = 2 /\ duplicates[s] = 1
SettlementCoverageRequired ==
    \A s \in Cases: phase[s] = "classified" /\ compatible[s] /\ ~settlementKnown[s]
        => verdict[s] = "incomplete"
CompatibilityLabeled ==
    \A s \in Cases: phase[s] = "classified" /\ ~compatible[s]
        => verdict[s] = "invalid_input"
=============================================================================
