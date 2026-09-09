---------------------- MODULE GuardianSupervision ----------------------
EXTENDS TLC

CONSTANT DetectDeath
VARIABLES phase, alive, interruptRequested, breachRecorded
vars == <<phase, alive, interruptRequested, breachRecorded>>

Init ==
    /\ phase = "running"
    /\ alive = TRUE
    /\ interruptRequested = FALSE
    /\ breachRecorded = FALSE

Crash ==
    /\ phase = "running"
    /\ alive' = FALSE
    /\ UNCHANGED <<phase, interruptRequested, breachRecorded>>

Poll ==
    /\ phase = "running"
    /\ interruptRequested' = (DetectDeath /\ ~alive)
    /\ breachRecorded' = interruptRequested'
    /\ phase' = "decided"
    /\ UNCHANGED alive

Next == Crash \/ Poll
Spec == Init /\ [][Next]_vars /\ WF_vars(Poll)

TypeOK ==
    /\ phase \in {"running", "decided"}
    /\ alive \in BOOLEAN
    /\ interruptRequested \in BOOLEAN
    /\ breachRecorded \in BOOLEAN

DeadGuardianRequiresInterrupt ==
    (phase = "decided" /\ ~alive) => (interruptRequested /\ breachRecorded)

Completes == <>(phase = "decided")
=============================================================================
