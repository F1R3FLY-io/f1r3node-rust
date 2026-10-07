---------------- MODULE CostChannelValidationMeter ----------------
EXTENDS Naturals, Sequences

\* Finite refinement obligation for the canonical Par visitor's native
\* Route-A cost detector. Each item is one charged visit in the composed
\* operation: original borrowed Par visits are already covered by the native
\* prepare_* CloneBacking inspection, while decoded-key Pars and key bytes
\* require NEW charges in the canonical visitor. Thus ordinary no-key Legacy
\* operations retain their old native meter totals. Key items must be charged
\* BEFORE positional key decode, including a malformed key. The Rust tests
\* compare the concrete SourceMeter trace and accepted hash bytes.
Items == {"root", "set-key", "map-key", "map-value"}
Keys == {"set-key", "map-key"}
Program == <<"root", "set-key", "map-key", "map-value">>

VARIABLES todo, charge, visited, decoded, found, costSites, badKeys,
          budget, phase, published
vars == <<todo, charge, visited, decoded, found, costSites, badKeys,
          budget, phase, published>>

Init ==
    /\ todo = Program
    /\ charge = 0
    /\ visited = 0
    /\ decoded = 0
    /\ found = FALSE
    /\ costSites \in SUBSET Items
    /\ badKeys \in SUBSET Keys
    /\ budget \in 0..4
    /\ phase = "NeedCharge"
    /\ published = FALSE

Charge ==
    /\ phase = "NeedCharge" /\ Len(todo) > 0
    /\ IF charge < budget
       THEN /\ charge' = charge + 1 /\ phase' = "Charged"
       ELSE /\ charge' = charge /\ phase' = "Rejected"
    /\ UNCHANGED <<todo, visited, decoded, found, costSites,
                   badKeys, budget, published>>

Visit ==
    /\ phase = "Charged" /\ Len(todo) > 0
    /\ LET item == Head(todo) IN
       /\ visited' = visited + 1
       /\ decoded' = decoded + IF item \in Keys THEN 1 ELSE 0
       /\ found' = (found \/ item \in costSites)
       /\ phase' = IF item \in badKeys THEN "Rejected" ELSE "NeedCharge"
    /\ todo' = Tail(todo)
    /\ UNCHANGED <<charge, costSites, badKeys, budget, published>>

Publish ==
    /\ phase = "NeedCharge" /\ Len(todo) = 0
    /\ phase' = "Accepted"
    /\ published' = TRUE
    /\ UNCHANGED <<todo, charge, visited, decoded, found,
                   costSites, badKeys, budget>>

Stop ==
    /\ phase \in {"Accepted", "Rejected"}
    /\ UNCHANGED vars

Next == Charge \/ Visit \/ Publish \/ Stop
Spec == Init /\ [][Next]_vars

Inv_ChargeBeforeVisit == visited <= charge
Inv_ChargeBeforeDecode == decoded <= charge
Inv_OneChargedTask == charge <= visited + 1
Inv_RejectBeforePublish == phase = "Rejected" => ~published
Inv_PublishOnlyAfterComplete == published => phase = "Accepted" /\ Len(todo) = 0
Inv_AcceptedCostFold == phase = "Accepted" => found = (costSites # {})
Inv_MalformedFailsClosed == phase = "Accepted" => badKeys = {}
=====================================================================
