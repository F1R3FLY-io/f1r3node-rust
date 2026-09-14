------------------------- MODULE NativeControllerLoss -------------------------
CONSTANT ManagedContainment
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
    /\ driverRunning' = FALSE
    /\ monitorRunning' = FALSE
    /\ UNCHANGED <<ownedRunning, unownedRunning>>

ManagerResponse ==
    /\ phase = "lost"
    /\ phase' = "checked"
    /\ ownedRunning' = IF ManagedContainment THEN FALSE ELSE ownedRunning
    /\ UNCHANGED <<driverRunning, monitorRunning, unownedRunning>>

Next == ControllersCrash \/ ManagerResponse
Spec == Init /\ [][Next]_vars /\ WF_vars(ControllersCrash) /\ WF_vars(ManagerResponse)
TypeOK ==
    /\ phase \in {"active", "lost", "checked"}
    /\ driverRunning \in BOOLEAN
    /\ monitorRunning \in BOOLEAN
    /\ ownedRunning \in BOOLEAN
    /\ unownedRunning \in BOOLEAN

NativeControllerLossStopsOwnedWriter == phase = "checked" => ~ownedRunning
UnownedWriterPreserved == unownedRunning
Completes == <>(phase = "checked")
=============================================================================
