------------------------- MODULE DockerOomOwnership -------------------------
EXTENDS Naturals, FiniteSets
CONSTANT ConfigureAtCreation
VARIABLES phase, running, preferred
vars == <<phase, running, preferred>>
Writers == {"owned", "unrelated"}
Init == /\ phase = "before"
        /\ running = {"unrelated"}
        /\ preferred = {}
Launch == /\ phase = "before"
          /\ phase' = "active"
          /\ running' = Writers
          /\ preferred' = IF ConfigureAtCreation THEN {"owned"} ELSE {}
Observe == /\ phase = "active"
           /\ phase' = "observed"
           /\ preferred' = IF ConfigureAtCreation THEN preferred ELSE Writers
           /\ UNCHANGED running
Next == Launch \/ Observe
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
TypeOK == /\ phase \in {"before", "active", "observed"}
          /\ running \subseteq Writers
          /\ preferred \subseteq running
UnownedContainerPreferencesPreserved == "unrelated" \notin preferred
OwnedContainerPreferenceApplied == phase = "observed" => "owned" \in preferred
Completes == <>(phase = "observed")
=============================================================================
