--------------------------- MODULE DriverCrashStop ---------------------------
EXTENDS Naturals

CONSTANT SurvivesDriverCrash
VARIABLES phase, supervisorRunning, ownedRunning, unownedRunning

vars == <<phase, supervisorRunning, ownedRunning, unownedRunning>>

Init ==
    /\ phase = "active"
    /\ supervisorRunning = TRUE
    /\ ownedRunning = TRUE
    /\ unownedRunning = TRUE

Crash ==
    /\ phase = "active"
    /\ phase' = "crashed"
    /\ supervisorRunning' = SurvivesDriverCrash
    /\ UNCHANGED <<ownedRunning, unownedRunning>>

Respond ==
    /\ phase = "crashed"
    /\ phase' = "checked"
    /\ ownedRunning' = (ownedRunning /\ ~supervisorRunning)
    /\ supervisorRunning' = FALSE
    /\ UNCHANGED unownedRunning

Next == Crash \/ Respond
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"active", "crashed", "checked"}
    /\ supervisorRunning \in BOOLEAN
    /\ ownedRunning \in BOOLEAN
    /\ unownedRunning \in BOOLEAN

DriverCrashStopsOwnedWriter == phase = "checked" => ~ownedRunning
UnownedWriterPreserved == unownedRunning
Completes == <>(phase = "checked")
=============================================================================
