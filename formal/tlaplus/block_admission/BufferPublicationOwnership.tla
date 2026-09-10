---------------------- MODULE BufferPublicationOwnership ----------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS BlockCount, WorkerCount, ResidentCap, Unsafe

Blocks == 1..BlockCount
Workers == 1..WorkerCount
Phases == {"idle", "writing", "committed", "cached", "failed", "acked"}

VARIABLES durable, cached, terminal, requested, seen, acknowledged, owner, phase

vars == <<durable, cached, terminal, requested, seen, acknowledged, owner, phase>>

Init ==
    /\ durable = {}
    /\ cached = {}
    /\ terminal = {}
    /\ requested = {}
    /\ seen = {}
    /\ acknowledged = {}
    /\ owner = [w \in Workers |-> 0]
    /\ phase = [w \in Workers |-> "idle"]

Begin(w, b) ==
    /\ phase[w] = "idle"
    /\ owner' = [owner EXCEPT ![w] = b]
    /\ phase' = [phase EXCEPT ![w] = "writing"]
    /\ requested' = requested \cup {b}
    /\ seen' = seen \cup {b}
    /\ UNCHANGED <<durable, cached, terminal, acknowledged>>

Commit(w) ==
    /\ phase[w] = "writing"
    /\ durable' = durable \cup {owner[w]}
    /\ phase' = [phase EXCEPT ![w] = "committed"]
    /\ UNCHANGED <<cached, terminal, requested, seen, acknowledged, owner>>

FailWrite(w) ==
    /\ phase[w] = "writing"
    /\ phase' = [phase EXCEPT ![w] = "failed"]
    /\ UNCHANGED <<durable, cached, terminal, requested, seen, acknowledged, owner>>

CacheChoices(b) ==
    IF ResidentCap = 0 THEN {{}}
    ELSE IF b \in cached \/ Cardinality(cached) < ResidentCap
        THEN {cached \cup {b}}
        ELSE {(cached \ {victim}) \cup {b} : victim \in cached}

PublishCache(w) ==
    /\ phase[w] = "committed"
    /\ phase' = [phase EXCEPT ![w] = "cached"]
    /\ cached' \in IF owner[w] \in durable THEN CacheChoices(owner[w]) ELSE {cached}
    /\ UNCHANGED <<durable, terminal, requested, seen, acknowledged, owner>>

SpeculativeCache(w) ==
    /\ Unsafe = "cache-before-commit"
    /\ phase[w] = "writing"
    /\ cached' \in CacheChoices(owner[w])
    /\ UNCHANGED <<durable, terminal, requested, seen, acknowledged, owner, phase>>

Acknowledge(w) ==
    /\ phase[w] \in {"committed", "cached", "failed"}
    /\ Unsafe = "acknowledge-unowned" \/ owner[w] \in durable \cup terminal
    /\ acknowledged' = acknowledged \cup {owner[w]}
    /\ requested' = requested \ {owner[w]}
    /\ phase' = [phase EXCEPT ![w] = "acked"]
    /\ UNCHANGED <<durable, cached, terminal, seen, owner>>

Release(w) ==
    /\ phase[w] \in {"failed", "acked"}
    /\ requested' =
        IF Unsafe = "forget-failed-request" /\ phase[w] = "failed"
        THEN requested \ {owner[w]}
        ELSE requested
    /\ phase' = [phase EXCEPT ![w] = "idle"]
    /\ owner' = [owner EXCEPT ![w] = 0]
    /\ UNCHANGED <<durable, cached, terminal, seen, acknowledged>>

Evict(b) ==
    /\ b \in cached
    /\ cached' = cached \ {b}
    /\ durable' = IF Unsafe = "evict-durable" THEN durable \ {b} ELSE durable
    /\ UNCHANGED <<terminal, requested, seen, acknowledged, owner, phase>>

Resolve(b) ==
    /\ b \in seen
    /\ b \notin terminal
    /\ terminal' = terminal \cup {b}
    /\ durable' = durable \ {b}
    /\ cached' = cached \ {b}
    /\ requested' = requested \ {b}
    /\ UNCHANGED <<seen, acknowledged, owner, phase>>

Restart ==
    /\ cached' = IF Unsafe = "restore-all-resident" THEN durable ELSE {}
    /\ requested' = {}
    /\ seen' = durable \cup terminal \cup acknowledged
    /\ owner' = [w \in Workers |-> 0]
    /\ phase' = [w \in Workers |-> "idle"]
    /\ UNCHANGED <<durable, terminal, acknowledged>>

Next ==
    \/ \E w \in Workers, b \in Blocks : Begin(w, b)
    \/ \E w \in Workers : Commit(w) \/ FailWrite(w) \/ PublishCache(w)
        \/ SpeculativeCache(w) \/ Acknowledge(w) \/ Release(w)
    \/ \E b \in Blocks : Evict(b) \/ Resolve(b)
    \/ Restart

TypeOK ==
    /\ durable \subseteq Blocks
    /\ cached \subseteq Blocks
    /\ terminal \subseteq Blocks
    /\ requested \subseteq Blocks
    /\ seen \subseteq Blocks
    /\ acknowledged \subseteq Blocks
    /\ owner \in [Workers -> 0..BlockCount]
    /\ phase \in [Workers -> Phases]
    /\ \A w \in Workers : (owner[w] = 0) <=> (phase[w] = "idle")

Inv_CacheOwnership == cached \subseteq durable
Inv_AcknowledgedDurability == acknowledged \subseteq durable \cup terminal
Inv_LiveRetry == seen \subseteq durable \cup terminal \cup requested
Inv_ResidentBound == Cardinality(cached) <= ResidentCap

Spec == Init /\ [][Next]_vars
=============================================================================
