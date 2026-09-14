------------------------ MODULE InterruptedOutputDrain ------------------------
CONSTANT StopBeforeDrain
VARIABLES phase, ownedRunning, unownedRunning, pipeOpen

vars == <<phase, ownedRunning, unownedRunning, pipeOpen>>

Init ==
    /\ phase = "client-exited"
    /\ ownedRunning = TRUE
    /\ unownedRunning = TRUE
    /\ pipeOpen = TRUE

ChooseOrder ==
    /\ phase = "client-exited"
    /\ phase' = IF StopBeforeDrain THEN "stopping" ELSE "draining"
    /\ UNCHANGED <<ownedRunning, unownedRunning, pipeOpen>>

Stop ==
    /\ phase = "stopping"
    /\ phase' = "draining"
    /\ ownedRunning' = FALSE
    /\ pipeOpen' = FALSE
    /\ UNCHANGED unownedRunning

Drain ==
    /\ phase = "draining"
    /\ ~pipeOpen
    /\ phase' = "complete"
    /\ UNCHANGED <<ownedRunning, unownedRunning, pipeOpen>>

Next == ChooseOrder \/ Stop \/ Drain
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"client-exited", "stopping", "draining", "complete"}
    /\ ownedRunning \in BOOLEAN
    /\ unownedRunning \in BOOLEAN
    /\ pipeOpen \in BOOLEAN

DrainRequiresOwnedStop == phase \in {"draining", "complete"} => ~ownedRunning
UnownedWriterPreserved == unownedRunning
Completes == <>(phase = "complete")
=============================================================================
