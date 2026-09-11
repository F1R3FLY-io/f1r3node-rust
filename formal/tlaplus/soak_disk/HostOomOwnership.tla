-------------------------- MODULE HostOomOwnership --------------------------
EXTENDS Naturals, FiniteSets
CONSTANT SelectOwned
VARIABLES phase, preferred
vars == <<phase, preferred>>
Unrelated == {"unrelated-node", "unrelated-client"}
Writers == Unrelated \cup {"owned-node"}
Init == /\ phase = "active"
        /\ preferred = {}
Mark == /\ phase = "active"
        /\ preferred' = IF SelectOwned THEN {"owned-node"}
                        ELSE {"owned-node", "unrelated-node"}
        /\ phase' = "marked"
Next == Mark
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
TypeOK == /\ phase \in {"active", "marked"}
          /\ preferred \subseteq Writers
UnownedPreferencesPreserved == preferred \cap Unrelated = {}
OwnedPreferenceApplied == phase = "marked" => "owned-node" \in preferred
Completes == <>(phase = "marked")
=============================================================================
