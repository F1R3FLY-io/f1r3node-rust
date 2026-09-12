-------------------------- MODULE RunDomainAdmission --------------------------
EXTENDS Naturals

CONSTANTS Work, VerifyPlacement
VARIABLES phase, recordMatches, benchmarks, iterations, failures, restarts

vars == <<phase, recordMatches, benchmarks, iterations, failures, restarts>>

Init ==
    /\ phase = "probe"
    /\ recordMatches \in BOOLEAN
    /\ benchmarks = 0
    /\ iterations = 0
    /\ failures = 0
    /\ restarts = 0

Admission ==
    /\ phase = "probe"
    /\ LET refused == VerifyPlacement /\ ~recordMatches IN
        /\ phase' = IF refused THEN "refused" ELSE "admitted"
        /\ benchmarks' = IF ~refused /\ Work = "benchmark" THEN 1 ELSE 0
        /\ iterations' = IF ~refused /\ Work = "iteration" THEN 1 ELSE 0
        /\ failures' = IF refused THEN 1 ELSE 0
    /\ UNCHANGED <<recordMatches, restarts>>

Restart ==
    /\ phase = "refused"
    /\ restarts < 2
    /\ restarts' = restarts + 1
    /\ UNCHANGED <<phase, recordMatches, benchmarks, iterations, failures>>

Next == Admission \/ Restart
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ Work \in {"benchmark", "iteration"}
    /\ phase \in {"probe", "refused", "admitted"}
    /\ recordMatches \in BOOLEAN
    /\ benchmarks \in 0..1
    /\ iterations \in 0..1
    /\ failures \in 0..1
    /\ restarts \in 0..2

UnverifiedPlacementPreventsAdmission == ~recordMatches => (benchmarks = 0 /\ iterations = 0)
MatchingRecordAdmits == phase = "refused" => ~recordMatches
RefusalRetainsFailure == phase = "refused" => failures = 1
Completes == <>(phase = "admitted" \/ restarts = 2)
=============================================================================
