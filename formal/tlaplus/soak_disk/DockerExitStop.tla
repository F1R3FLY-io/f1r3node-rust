------------------------ MODULE DockerExitStop ------------------------
EXTENDS TLC
CONSTANT StopOnExit
VARIABLES phase, writerAlive
vars == <<phase, writerAlive>>
Init == phase = "work" /\ writerAlive = TRUE
RequestExit ==
    /\ phase = "work"
    /\ phase' = "exit-requested"
    /\ UNCHANGED writerAlive
StopWriter ==
    /\ StopOnExit
    /\ phase = "exit-requested"
    /\ writerAlive
    /\ writerAlive' = FALSE
    /\ UNCHANGED phase
FinishExit ==
    /\ phase = "exit-requested"
    /\ (~StopOnExit \/ ~writerAlive)
    /\ phase' = "exited"
    /\ UNCHANGED writerAlive
Next == RequestExit \/ StopWriter \/ FinishExit
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
TypeOK == phase \in {"work", "exit-requested", "exited"} /\ writerAlive \in BOOLEAN
ParentExitStopsFixtureWriter == phase = "exited" => ~writerAlive
Completes == <>(phase = "exited")
=============================================================================
