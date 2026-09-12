---------------------- MODULE GuardianAdmission ----------------------
EXTENDS TLC

CONSTANT CheckBeforeAdmission
VARIABLES phase, alive, admitted, breachRecorded
vars == <<phase, alive, admitted, breachRecorded>>

Init ==
    /\ phase = "boundary"
    /\ alive = TRUE
    /\ admitted = FALSE
    /\ breachRecorded = FALSE

Crash ==
    /\ phase = "boundary"
    /\ alive' = FALSE
    /\ UNCHANGED <<phase, admitted, breachRecorded>>

Decide ==
    /\ phase = "boundary"
    /\ admitted' = (~CheckBeforeAdmission \/ alive)
    /\ breachRecorded' = ~admitted'
    /\ phase' = "decided"
    /\ UNCHANGED alive

Next == Crash \/ Decide
Spec == Init /\ [][Next]_vars /\ WF_vars(Decide)

TypeOK ==
    /\ phase \in {"boundary", "decided"}
    /\ alive \in BOOLEAN
    /\ admitted \in BOOLEAN
    /\ breachRecorded \in BOOLEAN

AdmissionRequiresGuardian == admitted => alive
RefusalRecorded == (phase = "decided" /\ ~admitted) => breachRecorded
Completes == <>(phase = "decided")
=============================================================================
