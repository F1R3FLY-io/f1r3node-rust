----------------------- MODULE GenesisResourcePolicy -----------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Nodes, Genesis, Later, None, Original, Changed, UseLatest, MixMinimum,
          GenesisMinimum, LaterMinimum
VARIABLES current, pending, loaded, available
vars == <<current, pending, loaded, available>>
Roots == {Genesis, Later}
Policy(root) == IF root = Genesis THEN Original ELSE Changed
Minimum(root) == IF root = Genesis THEN GenesisMinimum ELSE LaterMinimum
Init == /\ current = [n \in Nodes |-> Genesis]
        /\ pending = [n \in Nodes |-> None]
        /\ loaded = [n \in Nodes |-> None]
        /\ available = Roots
Request(n) == /\ pending[n] = None
              /\ pending' = [pending EXCEPT ![n] = IF UseLatest THEN current[n] ELSE Genesis]
              /\ UNCHANGED <<current, loaded, available>>
Complete(n) == /\ pending[n] # None
               /\ loaded' = [loaded EXCEPT ![n] =
                    IF pending[n] \in available
                    THEN <<Policy(pending[n]), Minimum(IF MixMinimum THEN current[n] ELSE pending[n])>>
                    ELSE None]
               /\ pending' = [pending EXCEPT ![n] = None]
               /\ UNCHANGED <<current, available>>
Advance(n) == /\ current' = [current EXCEPT ![n] = Later]
              /\ UNCHANGED <<pending, loaded, available>>
Restart(n) == /\ pending' = [pending EXCEPT ![n] = None]
              /\ loaded' = [loaded EXCEPT ![n] = None]
              /\ UNCHANGED <<current, available>>
StorageAvailability == /\ available' \in SUBSET Roots
                       /\ UNCHANGED <<current, pending, loaded>>
Next == (\E n \in Nodes : Request(n) \/ Complete(n) \/ Advance(n) \/ Restart(n))
        \/ StorageAvailability
TypeOK == /\ current \in [Nodes -> Roots]
          /\ pending \in [Nodes -> Roots \cup {None}]
          /\ loaded \in [Nodes -> {None} \cup ({Original, Changed} \X {GenesisMinimum, LaterMinimum})]
          /\ available \subseteq Roots
GenesisAuthority == \A n \in Nodes : loaded[n] \in {None, <<Original, GenesisMinimum>>}
ValidatorAgreement == \A a, b \in Nodes :
    (loaded[a] # None /\ loaded[b] # None) => loaded[a] = loaded[b]
Spec == Init /\ [][Next]_vars
=============================================================================
