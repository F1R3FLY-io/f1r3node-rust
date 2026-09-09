------------------------ MODULE RetryQuarantineEntry ------------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Budget, OperationCount, Unsafe
Operations == 1..OperationCount
Fields == {"cursor", "dependency", "broadcast", "peer"}
EmptySchedule == [f \in Fields |-> 0]
VARIABLES attempts, reserved, done, cancelled, peers, peerAttempts, schedule,
          storedAttempts, storedPeers, quarantined, provenance, entryCorrect, failureAtomic
vars == <<attempts, reserved, done, cancelled, peers, peerAttempts, schedule,
          storedAttempts, storedPeers, quarantined, provenance, entryCorrect, failureAtomic>>

Init == /\ attempts = 0 /\ reserved = {} /\ done = {} /\ cancelled = {} /\ peers = {}
        /\ peerAttempts = 0 /\ schedule = EmptySchedule
        /\ storedAttempts = 0 /\ storedPeers = 0
        /\ quarantined = FALSE /\ provenance = TRUE
        /\ entryCorrect = TRUE /\ failureAtomic = TRUE

Reserve(o, peer) ==
    /\ ~quarantined /\ o \notin reserved \cup done \cup cancelled
    /\ attempts + (IF Unsafe = "overreserve" THEN 0 ELSE Cardinality(reserved)) < Budget
    /\ reserved' = reserved \cup {o}
    /\ peers' = IF peer THEN peers \cup {o} ELSE peers
    /\ UNCHANGED <<attempts, done, cancelled, peerAttempts, schedule,
          storedAttempts, storedPeers, quarantined, provenance, entryCorrect, failureAtomic>>

Complete(o) ==
    /\ o \in reserved /\ reserved' = reserved \ {o} /\ done' = done \cup {o}
    /\ attempts' = attempts + 1
    /\ peerAttempts' = peerAttempts + (IF o \in peers THEN 1 ELSE 0)
    /\ UNCHANGED <<cancelled, peers, schedule, storedAttempts, storedPeers,
          quarantined, provenance, entryCorrect, failureAtomic>>

Cancel(o) ==
    /\ o \in reserved /\ reserved' = reserved \ {o} /\ cancelled' = cancelled \cup {o}
    /\ UNCHANGED <<attempts, done, peers, peerAttempts, schedule,
          storedAttempts, storedPeers, quarantined, provenance, entryCorrect, failureAtomic>>

Touch(f) ==
    /\ ~quarantined /\ schedule' = [schedule EXCEPT ![f] = 1]
    /\ UNCHANGED <<attempts, reserved, done, cancelled, peers, peerAttempts,
          storedAttempts, storedPeers, quarantined, provenance, entryCorrect, failureAtomic>>

Flush ==
    /\ storedAttempts' = attempts /\ storedPeers' = peerAttempts
    /\ UNCHANGED <<attempts, reserved, done, cancelled, peers, peerAttempts, schedule,
          quarantined, provenance, entryCorrect, failureAtomic>>

Enter ==
    /\ ~quarantined
    /\ attempts >= Budget \/ (Unsafe = "early" /\ attempts + Cardinality(reserved) >= Budget)
    /\ attempts' = IF Unsafe = "total-reset" THEN 0 ELSE attempts
    /\ peerAttempts' = IF Unsafe = "keep-peer" THEN peerAttempts ELSE 0
    /\ schedule' = IF Unsafe = "keep-schedule" THEN schedule ELSE EmptySchedule
    /\ storedAttempts' = IF Unsafe = "underwrite" THEN attempts - 1 ELSE attempts
    /\ storedPeers' = peerAttempts'
    /\ quarantined' = TRUE
    /\ provenance' = IF Unsafe = "provenance" THEN FALSE ELSE provenance
    /\ entryCorrect' = (entryCorrect /\ attempts >= Budget /\ reserved = {}
          /\ attempts' = attempts /\ peerAttempts' = 0 /\ schedule' = EmptySchedule
          /\ storedAttempts' = attempts' /\ storedPeers' = peerAttempts' /\ provenance' = provenance)
    /\ UNCHANGED <<reserved, done, cancelled, peers, failureAtomic>>

FailedEntry ==
    /\ ~quarantined /\ attempts >= Budget
    /\ schedule' = IF Unsafe = "partial-failure" THEN EmptySchedule ELSE schedule
    /\ attempts' = IF Unsafe = "failed-count" THEN 0 ELSE attempts
    /\ peerAttempts' = IF Unsafe = "failed-peer" THEN 0 ELSE peerAttempts
    /\ storedAttempts' = IF Unsafe = "failed-disk" THEN attempts ELSE storedAttempts
    /\ done' = IF Unsafe = "failed-completion" THEN {} ELSE done
    /\ UNCHANGED <<reserved, cancelled, peers, storedPeers, quarantined, provenance, entryCorrect>>
    /\ failureAtomic' = (failureAtomic
          /\ <<attempts', reserved', done', cancelled', peers', peerAttempts', schedule',
                storedAttempts', storedPeers', quarantined', provenance'>>
           = <<attempts, reserved, done, cancelled, peers, peerAttempts, schedule,
                storedAttempts, storedPeers, quarantined, provenance>>)

Next == Flush \/ Enter \/ FailedEntry
     \/ (\E f \in Fields : Touch(f))
     \/ (\E o \in Operations : Complete(o) \/ Cancel(o)
                            \/ (\E peer \in BOOLEAN : Reserve(o, peer)))
Inv_ReservationBound == attempts + Cardinality(reserved) <= Budget
Inv_Accounting == attempts = Cardinality(done)
Inv_EntryProjection == entryCorrect
Inv_FailedEntry == failureAtomic
Inv_StoredBound == storedAttempts <= attempts /\ storedPeers <= peerAttempts
Spec == Init /\ [][Next]_vars
=============================================================================
