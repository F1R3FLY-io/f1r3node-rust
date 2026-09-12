--------------------- MODULE IterationCrashRecovery ---------------------
EXTENDS Naturals, TLC
CONSTANT RememberInFlight
VARIABLES phase, inFlight, refused, failures, admitted
vars == <<phase, inFlight, refused, failures, admitted>>
Init ==
    /\ phase = "active"
    /\ inFlight = RememberInFlight
    /\ refused = FALSE
    /\ failures = 0
    /\ admitted = FALSE
Crash ==
    /\ phase = "active"
    /\ phase' = "crashed"
    /\ UNCHANGED <<inFlight, refused, failures, admitted>>
Recover ==
    /\ phase \in {"crashed", "recovered"}
    /\ phase' = IF phase = "crashed" THEN "recovered" ELSE "checked"
    /\ failures' = IF inFlight THEN failures + 1 ELSE failures
    /\ refused' = (refused \/ inFlight)
    /\ inFlight' = FALSE
    /\ admitted' = ~(refused \/ inFlight)
Next == Crash \/ Recover
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
TypeOK ==
    /\ phase \in {"active", "crashed", "recovered", "checked"}
    /\ inFlight \in BOOLEAN
    /\ refused \in BOOLEAN
    /\ failures \in 0..1
    /\ admitted \in BOOLEAN
CrashRequiresRefusal == phase \in {"recovered", "checked"} => ~admitted /\ failures = 1
Completes == <>(phase = "checked")
=============================================================================
