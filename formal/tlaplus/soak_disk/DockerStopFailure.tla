------------------------ MODULE DockerStopFailure ------------------------
EXTENDS Naturals, TLC
CONSTANT RetainStopFailure
VARIABLES phase, writerAlive, failures, refused, terminationUnconfirmed
vars == <<phase, writerAlive, failures, refused, terminationUnconfirmed>>
Init ==
    /\ phase = "active"
    /\ writerAlive = TRUE
    /\ failures = 0
    /\ refused = FALSE
    /\ terminationUnconfirmed = FALSE
RejectStop ==
    /\ phase = "active"
    /\ phase' = "exited"
    /\ UNCHANGED writerAlive
    /\ failures' = IF RetainStopFailure THEN 1 ELSE 0
    /\ refused' = RetainStopFailure
    /\ terminationUnconfirmed' = RetainStopFailure
Next == RejectStop
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
TypeOK == phase \in {"active", "exited"} /\ failures \in 0..1 /\ writerAlive \in BOOLEAN /\ refused \in BOOLEAN /\ terminationUnconfirmed \in BOOLEAN
FailedStopRetained == phase = "exited" => failures = 1 /\ refused /\ terminationUnconfirmed
RejectedStopDoesNotTerminate == writerAlive
Completes == <>(phase = "exited")
=============================================================================
