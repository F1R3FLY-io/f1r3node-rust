--------------------------- MODULE VersionPhlo ---------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS Scenarios, MaxObservations,
          ConflateAuthorityVersions, OmitPhloPrice, IgnoreRefundMismatch
ASSUME MaxObservations = 3
VARIABLES phase, labelsSeparate, fieldsCaptured, refundMismatch,
          observations, verdict, failure, capture
vars == <<phase, labelsSeparate, fieldsCaptured, refundMismatch,
          observations, verdict, failure, capture>>
Cases == 1..Scenarios
Init ==
    /\ phase = [s \in Cases |-> "request"]
    /\ labelsSeparate = [s \in Cases |-> TRUE]
    /\ fieldsCaptured = [s \in Cases |-> TRUE]
    /\ refundMismatch = [s \in Cases |-> FALSE]
    /\ observations = [s \in Cases |-> 0]
    /\ verdict = [s \in Cases |-> "pending"]
    /\ failure = [s \in Cases |-> FALSE]
    /\ capture = [s \in Cases |-> "unknown"]
Admit(s) ==
    /\ phase[s] = "request"
    /\ \E l \in BOOLEAN, f \in BOOLEAN, r \in BOOLEAN:
        /\ labelsSeparate' = [labelsSeparate EXCEPT ![s] = l]
        /\ fieldsCaptured' = [fieldsCaptured EXCEPT ![s] = f]
        /\ refundMismatch' = [refundMismatch EXCEPT ![s] = r]
        /\ phase' = [phase EXCEPT ![s] =
            IF ~l /\ ~ConflateAuthorityVersions THEN "classified" ELSE "collect"]
        /\ verdict' = [verdict EXCEPT ![s] =
            IF ~l /\ ~ConflateAuthorityVersions THEN "invalid_input" ELSE "pending"]
    /\ UNCHANGED <<observations, failure, capture>>
Collect(s) ==
    /\ phase[s] = "collect"
    /\ observations[s] < MaxObservations
    /\ observations' = [observations EXCEPT ![s] = @ + 1]
    /\ UNCHANGED <<phase, labelsSeparate, fieldsCaptured, refundMismatch,
                    verdict, failure, capture>>
Classify(s) ==
    /\ phase[s] = "collect"
    /\ observations[s] = MaxObservations
    /\ LET known == fieldsCaptured[s] \/ OmitPhloPrice
           mismatch == refundMismatch[s] /\ ~IgnoreRefundMismatch
       IN /\ verdict' = [verdict EXCEPT ![s] =
                IF mismatch THEN "product_failure"
                ELSE IF known THEN "passed" ELSE "incomplete"]
          /\ failure' = [failure EXCEPT ![s] = mismatch]
          /\ capture' = [capture EXCEPT ![s] = IF known THEN "observed" ELSE "unknown"]
    /\ phase' = [phase EXCEPT ![s] = "classified"]
    /\ UNCHANGED <<labelsSeparate, fieldsCaptured, refundMismatch, observations>>
Next == \E s \in Cases: Admit(s) \/ Collect(s) \/ Classify(s)
Spec == Init /\ [][Next]_vars
TypeOK ==
    /\ phase \in [Cases -> {"request", "collect", "classified"}]
    /\ labelsSeparate \in [Cases -> BOOLEAN]
    /\ fieldsCaptured \in [Cases -> BOOLEAN]
    /\ refundMismatch \in [Cases -> BOOLEAN]
    /\ observations \in [Cases -> 0..MaxObservations]
    /\ verdict \in [Cases -> {"pending", "invalid_input", "incomplete", "passed", "product_failure"}]
    /\ failure \in [Cases -> BOOLEAN]
    /\ capture \in [Cases -> {"unknown", "observed"}]
VersionLabelsSeparate ==
    \A s \in Cases: phase[s] = "classified" /\ ~labelsSeparate[s]
        => verdict[s] = "invalid_input" /\ observations[s] = 0
BothPhloFieldsCaptured ==
    \A s \in Cases: phase[s] = "classified" /\ labelsSeparate[s] /\ ~fieldsCaptured[s]
        => verdict[s] # "passed" /\ capture[s] = "unknown"
SettlementOutcomeClassified ==
    \A s \in Cases: phase[s] = "classified" /\ labelsSeparate[s] /\ refundMismatch[s]
        => verdict[s] = "product_failure" /\ failure[s]
=============================================================================
