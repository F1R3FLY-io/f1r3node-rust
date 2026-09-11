------------------------ MODULE HostStopOwnership ------------------------
EXTENDS TLC
CONSTANTS SelectOwned, UnsafeSelection
VARIABLES phase, running
vars == <<phase, running>>
Unrelated == {"unrelated-node", "unrelated-client"}
Writers == Unrelated \cup {"owned-node"}
Init == phase = "active" /\ running = Writers
Stop ==
    /\ phase = "active"
    /\ running' = running \ (IF SelectOwned THEN {"owned-node"} ELSE UnsafeSelection)
    /\ phase' = "exited"
Next == Stop
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
TypeOK == phase \in {"active", "exited"} /\ running \subseteq Writers
UnownedHostWritersPreserved == Unrelated \subseteq running
OwnedHostWriterStopped == phase = "exited" => "owned-node" \notin running
Completes == <>(phase = "exited")
=============================================================================
