---------------------------- MODULE ControllerLoss ----------------------------
VARIABLES phase, driverRunning, monitorRunning, ownedRunning, unownedRunning

vars == <<phase, driverRunning, monitorRunning, ownedRunning, unownedRunning>>

Init ==
    /\ phase = "active"
    /\ driverRunning = TRUE
    /\ monitorRunning = TRUE
    /\ ownedRunning = TRUE
    /\ unownedRunning = TRUE

ControllersCrash ==
    /\ phase = "active"
    /\ phase' = "lost"
    /\ driverRunning /\ monitorRunning
    /\ driverRunning' = FALSE
    /\ monitorRunning' = FALSE
    /\ UNCHANGED <<ownedRunning, unownedRunning>>

Observe ==
    /\ phase = "lost"
    /\ phase' = "checked"
    /\ UNCHANGED <<driverRunning, monitorRunning, ownedRunning, unownedRunning>>

Spec == Init /\ [][ControllersCrash \/ Observe]_vars
TypeOK ==
    /\ phase \in {"active", "lost", "checked"}
    /\ driverRunning \in BOOLEAN
    /\ monitorRunning \in BOOLEAN
    /\ ownedRunning \in BOOLEAN
    /\ unownedRunning \in BOOLEAN

ControllerLossStopsOwnedWriter == phase = "checked" => ~ownedRunning
UnownedWriterPreserved == unownedRunning
=============================================================================
