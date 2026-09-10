------------------------- MODULE LocalRequestActivation -------------------------
EXTENDS Integers, FiniteSets
CONSTANTS Hashes, Unsafe
Capacity == 1
VARIABLES active, deadline, now, phase, target, facts, result,
          budget, provenance, preserved, capacityCount, expectedCount, lease
vars == <<active, deadline, now, phase, target, facts, result,
          budget, provenance, preserved, capacityCount, expectedCount, lease>>
Classify(f) == IF f.owned THEN "tracked"
              ELSE IF f.failed THEN "error"
              ELSE IF f.quarantined THEN "quarantined"
              ELSE IF f.full THEN "capacity" ELSE "tracked"
Facts(h, failed) == [owned |-> h \in active, failed |-> failed,
                    quarantined |-> deadline[h] > now,
                    full |-> Cardinality(active) >= Capacity]
Refusal(r) == r \in {"quarantined", "capacity"}
Init == /\ active \in SUBSET Hashes /\ Cardinality(active) <= Capacity
        /\ deadline \in [Hashes -> 0..2] /\ now = 0
        /\ phase = "idle" /\ target \in Hashes
        /\ facts = Facts(target, FALSE) /\ result = "none"
        /\ budget \in [Hashes -> 0..2] /\ provenance \in SUBSET Hashes
        /\ preserved = TRUE /\ capacityCount = 0 /\ expectedCount = 0 /\ lease = TRUE

Tick == /\ now < 2 /\ now' = now + 1
        /\ UNCHANGED <<active, deadline, phase, target, facts, result,
              budget, provenance, preserved, capacityCount, expectedCount, lease>>
Other(h) == /\ h # target
            /\ (h \in active \/ Cardinality(active) < Capacity)
            /\ active' = IF h \in active THEN active \ {h} ELSE active \cup {h}
            /\ UNCHANGED <<deadline, now, phase, target, facts, result,
                  budget, provenance, preserved, capacityCount, expectedCount, lease>>
Decide(failed) ==
    /\ phase = "idle" /\ facts' = Facts(target, failed)
    /\ LET expected == Classify(facts')
           chosen == IF Unsafe = "quarantine-as-capacity" /\ expected = "quarantined" THEN "capacity"
                     ELSE IF Unsafe = "error-as-refusal" /\ expected = "error" THEN "capacity"
                     ELSE expected
       IN /\ result' = chosen
          /\ active' = IF chosen = "tracked" THEN active \cup {target} ELSE active
          /\ budget' = IF Unsafe = "mutate-refusal" /\ Refusal(chosen)
                        THEN [budget EXCEPT ![target] = (@ + 1) % 3] ELSE budget
          /\ preserved' = (~Refusal(chosen) \/ (active' = active /\ budget' = budget))
          /\ expectedCount' = IF expected = "capacity" THEN 1 ELSE 0
    /\ phase' = "decided"
    /\ UNCHANGED <<deadline, now, target, provenance, capacityCount, lease>>
Report ==
    /\ phase = "decided"
    /\ result' = IF Unsafe = "late-reason" /\ Refusal(result)
                  THEN Classify(Facts(target, facts.failed)) ELSE result
    /\ capacityCount' = IF result' = "capacity" \/ (Unsafe = "count-all-refusals" /\ Refusal(result')) THEN 1 ELSE 0
    /\ lease' = ~(result' = "tracked" \/ (Unsafe = "release-refused" /\ Refusal(result')))
    /\ phase' = "reported"
    /\ UNCHANGED <<active, deadline, now, target, facts, budget, provenance, preserved, expectedCount>>

Next == Tick \/ (\E h \in Hashes : Other(h)) \/ (\E failed \in BOOLEAN : Decide(failed)) \/ Report
Inv_Reason == phase = "idle" \/ result = Classify(facts)
Inv_Preserved == preserved
Inv_Metric == phase # "reported" \/ capacityCount = expectedCount
Inv_Lease == phase # "reported" \/ result = "tracked" \/ lease
Inv_Capacity == Cardinality(active) <= Capacity
TypeOK == /\ active \subseteq Hashes /\ deadline \in [Hashes -> 0..2] /\ now \in 0..2
          /\ phase \in {"idle", "decided", "reported"} /\ target \in Hashes
          /\ result \in {"none", "tracked", "error", "quarantined", "capacity"}
          /\ budget \in [Hashes -> 0..2] /\ provenance \subseteq Hashes
          /\ capacityCount \in 0..1 /\ expectedCount \in 0..1 /\ lease \in BOOLEAN /\ preserved \in BOOLEAN
Spec == Init /\ [][Next]_vars
=============================================================================
