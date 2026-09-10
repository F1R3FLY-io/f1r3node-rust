----------------------- MODULE AdmissionIdentityOwnership -----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Keys, Attempts, CountCap, Workers, ClaimOnlyVacant, CleanupQueued, ExactRelease,
          GuardProducerPayloadRelease

VARIABLES key, phase, owner, receiverOpen, payloads

vars == <<key, phase, owner, receiverOpen, payloads>>
Live == {a \in Attempts : phase[a] \in {"producer", "queued", "active", "finished", "rejected"}}
Queued == {a \in Attempts : phase[a] = "queued"}
Active == {a \in Attempts : phase[a] = "active"}

Init ==
    /\ key \in [Attempts -> Keys]
    /\ phase = [a \in Attempts |-> "unused"]
    /\ owner = [k \in Keys |-> "none"]
    /\ receiverOpen = TRUE
    /\ payloads = {}

Claim(a) ==
    /\ phase[a] = "unused"
    /\ ClaimOnlyVacant => owner[key[a]] = "none"
    /\ phase' = [phase EXCEPT ![a] = "producer"]
    /\ payloads' = payloads \cup {a}
    /\ owner' = [owner EXCEPT ![key[a]] = a]
    /\ UNCHANGED <<key, receiverOpen>>

Enqueue(a) ==
    /\ phase[a] = "producer"
    /\ receiverOpen
    /\ Cardinality(Queued) < CountCap
    /\ phase' = [phase EXCEPT ![a] = "queued"]
    /\ UNCHANGED <<key, owner, receiverOpen, payloads>>

Start(a) ==
    /\ phase[a] = "queued"
    /\ Cardinality(Active) < Workers
    /\ phase' = [phase EXCEPT ![a] = "active"]
    /\ UNCHANGED <<key, owner, receiverOpen, payloads>>

Finish(a) ==
    /\ phase[a] = "active"
    /\ phase' = [phase EXCEPT ![a] = "finished"]
    /\ payloads' = payloads \ {a}
    /\ UNCHANGED <<key, owner, receiverOpen>>

ReleaseOwner(a) ==
    [owner EXCEPT ![key[a]] = IF ExactRelease /\ @ # a THEN @ ELSE "none"]

Drop(a) ==
    /\ a \in Live
    /\ phase[a] # "rejected"
    /\ phase' = [phase EXCEPT ![a] = "done"]
    /\ payloads' = payloads \ {a}
    /\ owner' = IF phase[a] = "queued" /\ ~CleanupQueued THEN owner ELSE ReleaseOwner(a)
    /\ UNCHANGED <<key, receiverOpen>>

StaleCompletion(a) ==
    /\ phase[a] = "done"
    /\ owner' = ReleaseOwner(a)
    /\ UNCHANGED <<key, phase, receiverOpen, payloads>>

RejectProducer(a) ==
    /\ phase[a] = "producer"
    /\ phase' = [phase EXCEPT ![a] = "rejected"]
    /\ UNCHANGED <<key, owner, receiverOpen, payloads>>

DropRejectedPayload(a) ==
    /\ phase[a] \in {"rejected", "done"}
    /\ a \in payloads
    /\ payloads' = payloads \ {a}
    /\ UNCHANGED <<key, phase, owner, receiverOpen>>

ReleaseRejected(a) ==
    /\ phase[a] = "rejected"
    /\ GuardProducerPayloadRelease => a \notin payloads
    /\ phase' = [phase EXCEPT ![a] = "done"]
    /\ owner' = ReleaseOwner(a)
    /\ UNCHANGED <<key, receiverOpen, payloads>>

CloseReceiver ==
    /\ receiverOpen
    /\ receiverOpen' = FALSE
    /\ phase' = [a \in Attempts |-> IF a \in Queued THEN "done" ELSE phase[a]]
    /\ payloads' = payloads \ Queued
    /\ owner' = IF CleanupQueued
                THEN [k \in Keys |-> IF owner[k] \in Queued THEN "none" ELSE owner[k]]
                ELSE owner
    /\ UNCHANGED key

Next ==
    \/ \E a \in Attempts : Claim(a) \/ Enqueue(a) \/ Start(a) \/ Finish(a)
                           \/ Drop(a) \/ StaleCompletion(a) \/ RejectProducer(a)
                           \/ DropRejectedPayload(a) \/ ReleaseRejected(a)
    \/ CloseReceiver

TypeOK ==
    /\ key \in [Attempts -> Keys]
    /\ phase \in [Attempts -> {"unused", "producer", "queued", "active", "finished", "done", "rejected"}]
    /\ owner \in [Keys -> Attempts \cup {"none"}]
    /\ receiverOpen \in BOOLEAN
    /\ payloads \subseteq Attempts

Inv_ExactLiveOwnership ==
    /\ \A a \in Live : owner[key[a]] = a
    /\ \A k \in Keys : owner[k] # "none" => owner[k] \in Live /\ key[owner[k]] = k

Inv_NoDuplicateOwner == \A a, b \in Live : key[a] = key[b] => a = b
Inv_LocalCapacity == Cardinality(Queued) <= CountCap /\ Cardinality(Active) <= Workers
Inv_PayloadCovered == \A a \in payloads : a \in Live /\ owner[key[a]] = a

Spec == Init /\ [][Next]_vars
=============================================================================
