------------------------ MODULE BufferPendingProvenance ------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS KeyCount, RequestCap, DeletePending, AllowRestart
Keys == 1..KeyCount
ASSUME /\ KeyCount > 0 /\ RequestCap > 0
       /\ DeletePending \in BOOLEAN /\ AllowRestart \in BOOLEAN

VARIABLES tracked, received, pending, stored, solicited, everSolicited, done, restarted
vars == <<tracked, received, pending, stored, solicited, everSolicited, done, restarted>>

Init ==
    /\ tracked = {1}
    /\ received = {}
    /\ pending = {}
    /\ stored = {}
    /\ solicited = {1}
    /\ everSolicited = {1}
    /\ done = {}
    /\ restarted = FALSE

Required == {k + 1 : k \in pending \ {KeyCount}} \ done
Eligible(k) == k \in solicited
Ready(k) == k = KeyCount \/ k + 1 \in done

Request(k) ==
    /\ k \in Required \ (tracked \cup stored)
    /\ Cardinality(tracked) < RequestCap
    /\ tracked' = tracked \cup {k}
    /\ solicited' = solicited \cup {k}
    /\ everSolicited' = everSolicited \cup {k}
    /\ UNCHANGED <<received, pending, stored, done, restarted>>

Receive(k) ==
    /\ k \in tracked \ received
    /\ received' = received \cup {k}
    /\ stored' = stored \cup {k}
    /\ UNCHANGED <<tracked, pending, solicited, everSolicited, done, restarted>>

PublishPending(k) ==
    /\ k \in received \ pending
    /\ Eligible(k)
    /\ ~Ready(k)
    /\ pending' = pending \cup {k}
    /\ tracked' = IF DeletePending THEN tracked \ {k} ELSE tracked
    /\ received' = IF DeletePending THEN received \ {k} ELSE received
    /\ solicited' = IF DeletePending THEN solicited \ {k} ELSE solicited
    /\ UNCHANGED <<stored, everSolicited, done, restarted>>

Complete(k) ==
    /\ k \in stored \ done
    /\ Eligible(k)
    /\ Ready(k)
    /\ done' = done \cup {k}
    /\ pending' = pending \ {k}
    /\ tracked' = tracked \ {k}
    /\ received' = received \ {k}
    /\ solicited' = solicited \ {k}
    /\ UNCHANGED <<stored, everSolicited, restarted>>

Restart ==
    /\ AllowRestart /\ ~restarted /\ pending # {}
    /\ restarted' = TRUE
    /\ tracked' = {}
    /\ received' = {}
    /\ solicited' = {}
    /\ UNCHANGED <<pending, stored, everSolicited, done>>

Work == \E k \in Keys : Request(k) \/ Receive(k) \/ PublishPending(k) \/ Complete(k)
Next == Work \/ Restart

TypeOK ==
    /\ tracked \subseteq Keys
    /\ received \subseteq tracked
    /\ pending \subseteq stored
    /\ stored \subseteq Keys
    /\ solicited \subseteq tracked
    /\ everSolicited \subseteq Keys
    /\ done \subseteq stored
    /\ pending \cap done = {}
    /\ restarted \in BOOLEAN

Inv_Capacity == Cardinality(tracked) <= RequestCap
Inv_NoInventedAuthority == solicited \subseteq everSolicited
Inv_PendingEligibility == pending \subseteq solicited
Inv_DependencyProgressAvailable == done = Keys \/ ENABLED Work

Spec == Init /\ [][Next]_vars
=============================================================================
