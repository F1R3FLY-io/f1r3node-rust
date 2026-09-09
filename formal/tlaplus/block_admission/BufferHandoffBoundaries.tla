------------------------ MODULE BufferHandoffBoundaries ------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS KeyCount, RequestCap, Unsafe
Keys == 1..KeyCount
Deps == {"block", "certificate", "unverified"}
Required(k) == IF k = 1 THEN {} ELSE {"block", "certificate"}
Seeded == IF RequestCap = 0 THEN {} ELSE {1}

VARIABLES owned, tracked, received, published, rows, snapshot, attempts, age, wake
vars == <<owned, tracked, received, published, rows, snapshot, attempts, age, wake>>

Init ==
    /\ owned = Keys
    /\ tracked = Seeded
    /\ received = Seeded
    /\ published = {}
    /\ rows = [k \in Keys |-> IF k = 1 THEN {} ELSE {"block"}]
    /\ snapshot = {}
    /\ attempts = [k \in Keys |-> 1]
    /\ age = [k \in Keys |-> 1]
    /\ wake = [k \in Keys |-> 0]

Publish(k) ==
    /\ k \in owned \ published
    /\ published' = published \cup {k}
    /\ rows' = [rows EXCEPT ![k] =
        CASE Unsafe = "partial-row" /\ Required(k) # {} -> {"block"}
          [] Unsafe = "incoming-body" -> @ \cup Required(k) \cup {"unverified"}
          [] OTHER -> @ \cup Required(k)]
    /\ UNCHANGED <<owned, tracked, received, snapshot, attempts, age, wake>>

Observe(k) ==
    /\ k \in received
    /\ snapshot' = snapshot \cup {k}
    /\ UNCHANGED <<owned, tracked, received, published, rows, attempts, age, wake>>

Reopen(k) ==
    /\ k \in owned
    /\ k \in tracked \/ Cardinality(tracked) < RequestCap
    /\ tracked' = tracked \cup {k}
    /\ received' = received \ {k}
    /\ wake' = [wake EXCEPT ![k] = 1]
    /\ UNCHANGED <<owned, published, rows, snapshot, attempts, age>>

Maintain(k) ==
    /\ k \in snapshot
    /\ snapshot' = snapshot \ {k}
    /\ LET act == IF Unsafe = "stale-maintenance" THEN TRUE ELSE k \in received
       IN /\ received' = IF act THEN received \ {k} ELSE received
          /\ wake' = IF act /\ Unsafe = "stale-maintenance"
                     THEN [wake EXCEPT ![k] = 0] ELSE wake
          /\ attempts' = IF act /\ Unsafe = "stale-maintenance"
                         THEN [attempts EXCEPT ![k] = 0] ELSE attempts
          /\ age' = IF act /\ Unsafe = "stale-maintenance"
                    THEN [age EXCEPT ![k] = 0] ELSE age
    /\ UNCHANGED <<owned, tracked, published, rows>>

Durable(k) == (k \in published \/ Unsafe = "trust-old-row") /\
    (Unsafe # "empty-row-invisible" \/ rows[k] # {})

ReleaseDurable(k) ==
    /\ k \in owned
    /\ Durable(k)
    /\ owned' = owned \ {k}
    /\ tracked' = tracked \ {k}
    /\ received' = received \ {k}
    /\ UNCHANGED <<published, rows, snapshot, attempts, age, wake>>

ReleaseReady(k) ==
    /\ k \in owned
    /\ k \in tracked \ received
    /\ owned' = owned \ {k}
    /\ UNCHANGED <<tracked, received, published, rows, snapshot, attempts, age, wake>>

Next == \E k \in Keys : Publish(k) \/ Observe(k) \/ Reopen(k) \/ Maintain(k)
                              \/ ReleaseDurable(k) \/ ReleaseReady(k)

TypeOK ==
    /\ owned \subseteq Keys
    /\ tracked \subseteq Keys
    /\ received \subseteq tracked
    /\ published \subseteq Keys
    /\ rows \in [Keys -> SUBSET Deps]
    /\ snapshot \subseteq Keys
    /\ attempts \in [Keys -> 0..1]
    /\ age \in [Keys -> 0..1]
    /\ wake \in [Keys -> 0..1]

Inv_CompleteRows == \A k \in published : rows[k] = Required(k)
Inv_CanonicalDependencies == \A k \in published : rows[k] \subseteq Required(k)
Inv_LiveOwner == Keys \subseteq owned \cup published \cup (tracked \ received)
Inv_Capacity == Cardinality(tracked) <= RequestCap
Inv_Policy == /\ attempts = [k \in Keys |-> 1] /\ age = [k \in Keys |-> 1]
Inv_DurableReleaseEnabled == \A k \in owned \cap published : ENABLED ReleaseDurable(k)

Spec == Init /\ [][Next]_vars
=============================================================================
