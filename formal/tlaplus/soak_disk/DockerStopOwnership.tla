------------------------ MODULE DockerStopOwnership ------------------------
EXTENDS TLC
CONSTANT SelectOwned
VARIABLES phase, running
vars == <<phase, running>>
Init == phase = "active" /\ running = {"workload", "unrelated"}
Stop ==
    /\ phase = "active"
    /\ running' = IF SelectOwned THEN running \ {"workload"} ELSE {}
    /\ phase' = "exited"
Next == Stop
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
TypeOK == phase \in {"active", "exited"} /\ running \subseteq {"workload", "unrelated"}
UnownedWritersPreserved == "unrelated" \in running
OwnedWriterStopped == phase = "exited" => "workload" \notin running
Completes == <>(phase = "exited")
=============================================================================
