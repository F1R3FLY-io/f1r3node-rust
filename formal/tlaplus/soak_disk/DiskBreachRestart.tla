------------------------ MODULE DiskBreachRestart ------------------------
EXTENDS Naturals, TLC

CONSTANT PreserveBreach
VARIABLES phase, priorFailures, failures, marker, admitted
vars == <<phase, priorFailures, failures, marker, admitted>>

Init ==
    /\ phase = "resume"
    /\ priorFailures \in {0, 2}
    /\ failures = priorFailures
    /\ marker = TRUE
    /\ admitted = FALSE

Recover ==
    /\ phase = "resume"
    /\ marker' = PreserveBreach
    /\ failures' = IF PreserveBreach /\ failures = 0 THEN 1 ELSE failures
    /\ phase' = IF PreserveBreach THEN "stopped" ELSE "ready"
    /\ UNCHANGED <<priorFailures, admitted>>

Finish ==
    /\ phase \in {"stopped", "ready"}
    /\ admitted' = (phase = "ready")
    /\ phase' = "done"
    /\ UNCHANGED <<priorFailures, failures, marker>>

Next == Recover \/ Finish
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"resume", "stopped", "ready", "done"}
    /\ priorFailures \in {0, 2}
    /\ failures \in 0..2
    /\ marker \in BOOLEAN
    /\ admitted \in BOOLEAN

RetainedBreachStopsRestart == phase = "done" => (marker /\ ~admitted /\ failures > 0)
PriorFailuresPreserved == failures >= priorFailures
Completes == <>(phase = "done")
=============================================================================
