----------------------- MODULE GenesisResourcePolicy -----------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Nodes, Genesis, Later, None, Original, Changed, UseLatest, MixMinimum,
          GenesisMinimum, LaterMinimum, CheckRules
VARIABLES current, pending, loaded, available, supported
vars == <<current, pending, loaded, available, supported>>
Roots == {Genesis, Later}
Policy(root) == IF root = Genesis THEN Original ELSE Changed
Minimum(root) == IF root = Genesis THEN GenesisMinimum ELSE LaterMinimum
Init == /\ current = [n \in Nodes |-> Genesis]
        /\ pending = [n \in Nodes |-> None]
        /\ loaded = [n \in Nodes |-> None]
        /\ available = Roots
        /\ supported \in [Nodes -> SUBSET {Original, Changed}]
Request(n) == /\ pending[n] = None
              /\ pending' = [pending EXCEPT ![n] = IF UseLatest THEN current[n] ELSE Genesis]
              /\ UNCHANGED <<current, loaded, available, supported>>
Complete(n) == /\ pending[n] # None
               /\ loaded' = [loaded EXCEPT ![n] =
                    IF pending[n] \in available /\ (~CheckRules \/ Policy(pending[n]) \in supported[n])
                    THEN <<Policy(pending[n]), Minimum(IF MixMinimum THEN current[n] ELSE pending[n])>>
                    ELSE None]
               /\ pending' = [pending EXCEPT ![n] = None]
               /\ UNCHANGED <<current, available, supported>>
Advance(n) == /\ current' = [current EXCEPT ![n] = Later]
              /\ UNCHANGED <<pending, loaded, available, supported>>
Restart(n) == /\ pending' = [pending EXCEPT ![n] = None]
              /\ loaded' = [loaded EXCEPT ![n] = None]
              /\ \E rules \in SUBSET {Original, Changed} : supported' = [supported EXCEPT ![n] = rules]
              /\ UNCHANGED <<current, available>>
StorageAvailability == /\ available' \in SUBSET Roots
                       /\ UNCHANGED <<current, pending, loaded, supported>>
Next == (\E n \in Nodes : Request(n) \/ Complete(n) \/ Advance(n) \/ Restart(n))
        \/ StorageAvailability
TypeOK == /\ current \in [Nodes -> Roots]
          /\ pending \in [Nodes -> Roots \cup {None}]
          /\ loaded \in [Nodes -> {None} \cup ({Original, Changed} \X {GenesisMinimum, LaterMinimum})]
          /\ available \subseteq Roots
          /\ supported \in [Nodes -> SUBSET {Original, Changed}]
GenesisAuthority == \A n \in Nodes : loaded[n] \in {None, <<Original, GenesisMinimum>>}
ValidatorAgreement == \A a, b \in Nodes :
    (loaded[a] # None /\ loaded[b] # None) => loaded[a] = loaded[b]
SupportedInterpretation == \A n \in Nodes : loaded[n] # None => loaded[n][1] \in supported[n]
Spec == Init /\ [][Next]_vars
=============================================================================
