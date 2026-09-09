--------------------- MODULE DependencyRequestProvenance ---------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS KeyCount, RequestCap, Unsafe
Keys == 1..KeyCount
VARIABLES tracked, authority, required, received, budget, snapshot, readSet
vars == <<tracked, authority, required, received, budget, snapshot, readSet>>

Init ==
    /\ tracked = {}
    /\ authority = {}
    /\ required = {}
    /\ received = {}
    /\ budget = [k \in Keys |-> 1]
    /\ snapshot = {}
    /\ readSet = {}

Request(k, dependency) ==
    /\ k \in tracked \/ Cardinality(tracked) < RequestCap
    /\ tracked' = tracked \cup {k}
    /\ required' = IF dependency THEN required \cup {k} ELSE required
    /\ authority' =
        CASE Unsafe = "new-only" /\ k \in tracked -> authority
          [] Unsafe = "overwrite" -> IF dependency THEN authority \cup {k} ELSE authority \ {k}
          [] OTHER -> IF dependency THEN authority \cup {k} ELSE authority
    /\ budget' = IF Unsafe = "reset-budget" /\ dependency
                  THEN [budget EXCEPT ![k] = 0] ELSE budget
    /\ UNCHANGED <<received, snapshot, readSet>>

Receive(k) ==
    /\ k \in tracked
    /\ received' = received \cup {k}
    /\ UNCHANGED <<tracked, authority, required, budget, snapshot, readSet>>

Read(k) ==
    /\ k \in tracked
    /\ readSet' = readSet \cup {k}
    /\ snapshot' = IF k \in authority THEN snapshot \cup {k} ELSE snapshot \ {k}
    /\ UNCHANGED <<tracked, authority, required, received, budget>>

StaleAnnouncement(k) ==
    /\ Unsafe = "stale-update"
    /\ k \in readSet
    /\ authority' = IF k \in snapshot THEN authority \cup {k} ELSE authority \ {k}
    /\ readSet' = readSet \ {k}
    /\ UNCHANGED <<tracked, required, received, budget, snapshot>>

Next == \E k \in Keys : Request(k, TRUE) \/ Request(k, FALSE) \/ Receive(k)
                             \/ Read(k) \/ StaleAnnouncement(k)

TypeOK == /\ tracked \subseteq Keys /\ authority \subseteq tracked
          /\ required \subseteq tracked /\ received \subseteq tracked
          /\ budget \in [Keys -> 0..1] /\ snapshot \subseteq tracked
          /\ readSet \subseteq tracked
Inv_ExactAuthority == authority = required
Inv_Capacity == Cardinality(tracked) <= RequestCap
Inv_BudgetPreserved == budget = [k \in Keys |-> 1]
Spec == Init /\ [][Next]_vars
=============================================================================
