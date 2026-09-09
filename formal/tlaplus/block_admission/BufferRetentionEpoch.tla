-------------------------- MODULE BufferRetentionEpoch --------------------------
EXTENDS Integers, FiniteSets

CONSTANTS Identities, MaxTime, TTL, SeedRestoredAge, CountReadyPartition

VARIABLES live, incoming, outgoing, seen, born, now
vars == <<live, incoming, outgoing, seen, born, now>>

Init ==
    /\ live \in SUBSET Identities
    /\ incoming \in SUBSET live
    /\ outgoing \in SUBSET live
    /\ now = 0
    /\ born = [key \in Identities |-> 0]
    /\ seen = [key \in Identities |-> IF SeedRestoredAge /\ key \in live THEN 0 ELSE -1]

Restart ==
    /\ born' = [key \in Identities |-> now]
    /\ seen' = [key \in Identities |-> IF SeedRestoredAge /\ key \in live THEN now ELSE -1]
    /\ UNCHANGED <<live, incoming, outgoing, now>>

Insert(key) ==
    /\ key \notin live
    /\ live' = live \cup {key}
    /\ born' = [born EXCEPT ![key] = now]
    /\ seen' = [seen EXCEPT ![key] = now]
    /\ UNCHANGED <<incoming, outgoing, now>>

Tick ==
    /\ now < MaxTime
    /\ now' = now + 1
    /\ UNCHANGED <<live, incoming, outgoing, seen, born>>

Age(key) == IF seen[key] = -1 THEN 0 ELSE now - seen[key]
Count == Cardinality(incoming) +
         IF CountReadyPartition THEN Cardinality(live \ incoming) ELSE Cardinality(outgoing)

Next == Restart \/ Tick \/ (\E key \in Identities : Insert(key))
TypeOK ==
    /\ live \subseteq Identities
    /\ incoming \subseteq live /\ outgoing \subseteq live
    /\ now \in 0..MaxTime
    /\ seen \in [Identities -> (-1)..MaxTime]
    /\ born \in [Identities -> 0..MaxTime]
Inv_StableAgeEpoch == \A key \in live : Age(key) = now - born[key]
Inv_PositiveTTLCanExpire == \A key \in live : now >= born[key] + TTL => Age(key) >= TTL
Inv_CompletePressureCount == Count = Cardinality(live)
Spec == Init /\ [][Next]_vars
=============================================================================
