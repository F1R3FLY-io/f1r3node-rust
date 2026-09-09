------------------------- MODULE DurablePendingHandoff -------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS KeyCount, WorkerCount, RequestCap, RevisionLimit, Unsafe
Keys == 1..KeyCount
Workers == 1..WorkerCount
Idle == [stage |-> "idle", key |-> 0, revision |-> 0, fact |-> FALSE]

VARIABLES active, rows, metadata, resident, terminal, obligations,
          revision, diskRevision, fact, diskFact, issued, workers, staleCleanup,
          restarted
vars == <<active, rows, metadata, resident, terminal, obligations,
          revision, diskRevision, fact, diskFact, issued, workers, staleCleanup,
          restarted>>

Init ==
    /\ active = {} /\ rows = {} /\ metadata = {} /\ resident = {}
    /\ terminal = {} /\ obligations = {} /\ issued = {}
    /\ revision = [k \in Keys |-> 0]
    /\ diskRevision = [k \in Keys |-> 0]
    /\ fact = [k \in Keys |-> FALSE]
    /\ diskFact = [k \in Keys |-> FALSE]
    /\ workers = [w \in Workers |-> Idle]
    /\ staleCleanup = FALSE /\ restarted = FALSE

Admit(k, dependency) ==
    /\ k \notin terminal \cup active \cup metadata
    /\ Cardinality(active) < RequestCap
    /\ active' = active \cup {k}
    /\ obligations' = obligations \cup {k}
    /\ revision' = [revision EXCEPT ![k] = 1]
    /\ fact' = [fact EXCEPT ![k] = dependency]
    /\ issued' = IF dependency THEN issued \cup {k} ELSE issued
    /\ UNCHANGED <<rows, metadata, resident, terminal, diskRevision, diskFact,
                    workers, staleCleanup, restarted>>

UpdatePolicy(k, dependency) ==
    /\ k \in (active \cup metadata) \ terminal
    /\ revision[k] < RevisionLimit
    /\ revision' = [revision EXCEPT ![k] = @ + 1]
    /\ fact' = [fact EXCEPT ![k] = @ \/ dependency]
    /\ issued' = IF dependency THEN issued \cup {k} ELSE issued
    /\ diskRevision' = IF k \in metadata
                        THEN [diskRevision EXCEPT ![k] = revision[k] + 1]
                        ELSE diskRevision
    /\ diskFact' = IF k \in metadata THEN [diskFact EXCEPT ![k] = fact[k] \/ dependency]
                    ELSE diskFact
    /\ UNCHANGED <<active, rows, metadata, resident, terminal, obligations,
                    workers, staleCleanup, restarted>>

Capture(w, k) ==
    /\ workers[w].stage = "idle"
    /\ k \in (active \cup metadata) \ terminal
    /\ workers' = [workers EXCEPT ![w] =
         [stage |-> "captured", key |-> k, revision |-> revision[k], fact |-> fact[k]]]
    /\ UNCHANGED <<active, rows, metadata, resident, terminal, obligations,
                    revision, diskRevision, fact, diskFact, issued, staleCleanup, restarted>>

Commit(w) ==
    /\ workers[w].stage = "captured"
    /\ LET k == workers[w].key IN
       /\ k \notin terminal \/ Unsafe = "terminal-revival"
       /\ workers[w].revision = revision[k] \/ Unsafe = "stale-commit"
       /\ rows' = rows \cup {k}
       /\ metadata' = IF Unsafe = "partial-publication" THEN metadata ELSE metadata \cup {k}
       /\ diskRevision' = [diskRevision EXCEPT ![k] = workers[w].revision]
       /\ diskFact' = [diskFact EXCEPT ![k] = workers[w].fact]
       /\ resident' = resident \cup {k}
    /\ workers' = [workers EXCEPT ![w].stage = "committed"]
    /\ UNCHANGED <<active, terminal, obligations, revision, fact, issued,
                    staleCleanup, restarted>>

Retire(w) ==
    /\ workers[w].stage = "committed"
    /\ LET k == workers[w].key IN
       /\ k \notin terminal /\ k \in metadata
       /\ workers[w].revision = revision[k] \/ Unsafe = "stale-cleanup"
       /\ active' = active \ {k}
       /\ staleCleanup' = (staleCleanup \/ workers[w].revision # revision[k])
    /\ workers' = [workers EXCEPT ![w] = Idle]
    /\ UNCHANGED <<rows, metadata, resident, terminal, obligations, revision,
                    diskRevision, fact, diskFact, issued, restarted>>

Retry(w) ==
    /\ workers[w].stage # "idle"
    /\ workers' = [workers EXCEPT ![w] = Idle]
    /\ UNCHANGED <<active, rows, metadata, resident, terminal, obligations,
                    revision, diskRevision, fact, diskFact, issued, staleCleanup, restarted>>

Finish(k) ==
    /\ k \in obligations \ terminal
    /\ terminal' = terminal \cup {k}
    /\ active' = active \ {k}
    /\ rows' = rows \ {k}
    /\ metadata' = metadata \ {k}
    /\ resident' = resident \ {k}
    /\ UNCHANGED <<obligations, revision, diskRevision, fact, diskFact,
                    issued, workers, staleCleanup, restarted>>

Evict(k) ==
    /\ k \in resident
    /\ resident' = resident \ {k}
    /\ metadata' = IF Unsafe = "eviction-loss" THEN metadata \ {k} ELSE metadata
    /\ UNCHANGED <<active, rows, terminal, obligations, revision, diskRevision,
                    fact, diskFact, issued, workers, staleCleanup, restarted>>

Restart ==
    /\ ~restarted
    /\ active' = {}
    /\ resident' = {}
    /\ obligations' = rows \cup terminal
    /\ revision' = [k \in Keys |-> IF k \in metadata THEN diskRevision[k] ELSE 0]
    /\ fact' = [k \in Keys |-> IF k \in metadata /\ Unsafe # "restart-loss"
                              THEN diskFact[k] ELSE FALSE]
    /\ workers' = [w \in Workers |-> Idle]
    /\ restarted' = TRUE
    /\ UNCHANGED <<rows, metadata, terminal, diskRevision, diskFact, issued, staleCleanup>>

Next == (\E k \in Keys : Admit(k, TRUE) \/ Admit(k, FALSE)
           \/ UpdatePolicy(k, TRUE) \/ UpdatePolicy(k, FALSE) \/ Finish(k) \/ Evict(k))
        \/ (\E w \in Workers : (\E k \in Keys : Capture(w, k))
           \/ Commit(w) \/ Retire(w) \/ Retry(w))
        \/ Restart

TypeOK ==
    /\ active \subseteq Keys /\ rows \subseteq Keys /\ metadata \subseteq Keys
    /\ resident \subseteq Keys /\ terminal \subseteq Keys /\ obligations \subseteq Keys
    /\ issued \subseteq Keys
    /\ revision \in [Keys -> 0..RevisionLimit]
    /\ diskRevision \in [Keys -> 0..RevisionLimit]
    /\ fact \in [Keys -> BOOLEAN] /\ diskFact \in [Keys -> BOOLEAN]
    /\ workers \in [Workers -> [stage : {"idle", "captured", "committed"},
         key : 0..KeyCount, revision : 0..RevisionLimit, fact : BOOLEAN]]
    /\ staleCleanup \in BOOLEAN /\ restarted \in BOOLEAN

Inv_CompletePublication == rows = metadata
Inv_Ownership == obligations \subseteq active \cup metadata \cup terminal
Inv_Capacity == Cardinality(active) <= RequestCap
Inv_DurablePolicy == \A k \in metadata : diskRevision[k] = revision[k] /\ diskFact[k] = fact[k]
Inv_NoInventedAuthority == \A k \in Keys : (fact[k] \/ diskFact[k]) => k \in issued
Inv_NoStaleCleanup == ~staleCleanup
Inv_TerminalDominance == terminal \cap (rows \cup metadata \cup active) = {}
Spec == Init /\ [][Next]_vars
=============================================================================
