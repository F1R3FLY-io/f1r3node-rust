---------------------------- MODULE ControllerLoss ----------------------------
VARIABLES driverRunning, monitorRunning, ownedRunning, unownedRunning

vars == <<driverRunning, monitorRunning, ownedRunning, unownedRunning>>

Init ==
    /\ driverRunning = TRUE
    /\ monitorRunning = TRUE
    /\ ownedRunning = TRUE
    /\ unownedRunning = TRUE

ControllersCrash ==
    /\ driverRunning /\ monitorRunning
    /\ driverRunning' = FALSE
    /\ monitorRunning' = FALSE
    /\ UNCHANGED <<ownedRunning, unownedRunning>>

Spec == Init /\ [][ControllersCrash]_vars
TypeOK ==
    /\ driverRunning \in BOOLEAN
    /\ monitorRunning \in BOOLEAN
    /\ ownedRunning \in BOOLEAN
    /\ unownedRunning \in BOOLEAN

ControllerLossStopsOwnedWriter == ~driverRunning /\ ~monitorRunning => ~ownedRunning
UnownedWriterPreserved == unownedRunning
=============================================================================
