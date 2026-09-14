------------------- MODULE DockerCleanupOwnership -------------------
EXTENDS TLC, FiniteSets
CONSTANT DeleteUnowned
Resources == {"container", "network", "image"}
VARIABLES visited, present
vars == <<visited, present>>
Init ==
    /\ visited = {}
    /\ present = Resources
Inspect ==
    \E resource \in Resources \ visited:
        /\ visited' = visited \cup {resource}
        /\ present' = IF DeleteUnowned THEN present \ {resource} ELSE present
Spec == Init /\ [][Inspect]_vars /\ WF_vars(Inspect)
TypeOK == visited \subseteq Resources /\ present \subseteq Resources
UnownedDockerResourcesPreserved == present = Resources
Completes == <>(visited = Resources)
=============================================================================
