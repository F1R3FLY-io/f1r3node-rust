------------------------- MODULE DeferredRetryCompletion -------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS OwnerCount, OperationCount, OperationCap, Unsafe
Owners == 1..OwnerCount
Operations == 1..OperationCount

VARIABLES latest, active, terminal, operationOwner, stage, seen, committed,
          effective, disk, writer, sent, transmissions, restarted, maintenance
vars == <<latest, active, terminal, operationOwner, stage, seen, committed,
          effective, disk, writer, sent, transmissions, restarted, maintenance>>

Held == {p \in Operations : stage[p] \in {"sending", "success", "deferred"}}
Unwritten(o) == seen[o] \ committed[o]
Retained(o) == active = o \/ \E p \in Held : operationOwner[p] = o
Live(o) == o = latest /\ o \notin terminal

Init ==
    /\ latest = 1 /\ active = 1 /\ terminal = {}
    /\ operationOwner = [p \in Operations |-> 0]
    /\ stage = [p \in Operations |-> "idle"]
    /\ seen = [o \in Owners |-> {}] /\ committed = seen
    /\ effective = [o \in Owners |-> 0] /\ disk = effective
    /\ writer = 1 /\ sent = {} /\ transmissions = 0
    /\ restarted = FALSE /\ maintenance = FALSE

Start(p) ==
    /\ active = latest /\ Live(latest) /\ stage[p] = "idle"
    /\ Cardinality(Held) < OperationCap \/ Unsafe = "unbounded-permits"
    /\ operationOwner' = [operationOwner EXCEPT ![p] = latest]
    /\ stage' = [stage EXCEPT ![p] = "sending"]
    /\ sent' = sent \cup {p} /\ transmissions' = transmissions + 1
    /\ UNCHANGED <<latest, active, terminal, seen, committed, effective, disk,
                    writer, restarted, maintenance>>

ObserveCompletion(p, returnedOk) ==
    /\ stage[p] = "sending"
    /\ LET o == operationOwner[p] IN
       /\ seen' = IF Live(o) THEN [seen EXCEPT ![o] = @ \cup {p}] ELSE seen
       /\ effective' = IF Live(o) /\ (Unsafe # "success-only" \/ returnedOk)
                       THEN [effective EXCEPT ![o] = @ + 1] ELSE effective
    /\ stage' = [stage EXCEPT ![p] = "success"]
    /\ UNCHANGED <<latest, active, terminal, operationOwner, committed, disk,
                    writer, sent, transmissions, restarted, maintenance>>

CancelTransport(p) ==
    /\ stage[p] = "sending"
    /\ stage' = [stage EXCEPT ![p] = "done"]
    /\ UNCHANGED <<latest, active, terminal, operationOwner, seen, committed,
                    effective, disk, writer, sent, transmissions, restarted, maintenance>>

FailedPersistence(p) ==
    /\ stage[p] \in {"success", "deferred"}
    /\ Live(operationOwner[p]) /\ p \in Unwritten(operationOwner[p])
    /\ stage' = [stage EXCEPT ![p] = IF Unsafe = "lost-completion" THEN "done" ELSE "deferred"]
    /\ UNCHANGED <<latest, active, terminal, operationOwner, seen, committed,
                    effective, disk, writer, sent, transmissions, restarted, maintenance>>

Persist(p) ==
    /\ stage[p] \in {"success", "deferred"}
    /\ Live(operationOwner[p]) \/ Unsafe = "terminal-write"
    /\ LET o == operationOwner[p]
           charge == IF Unsafe = "double-charge" THEN effective[o] + 1 ELSE effective[o]
       IN
       /\ disk' = [disk EXCEPT ![latest] = charge]
       /\ committed' = [committed EXCEPT ![o] = seen[o]]
       /\ effective' = [effective EXCEPT ![o] = charge]
       /\ writer' = o
    /\ transmissions' = transmissions + IF Unsafe = "network-redrain" THEN 1 ELSE 0
    /\ UNCHANGED <<latest, active, terminal, operationOwner, stage, seen,
                    sent, restarted, maintenance>>

Release(p) ==
    /\ stage[p] \in {"success", "deferred"}
    /\ ~Live(operationOwner[p]) \/ p \in committed[operationOwner[p]]
    /\ stage' = [stage EXCEPT ![p] = "done"]
    /\ UNCHANGED <<latest, active, terminal, operationOwner, seen, committed,
                    effective, disk, writer, sent, transmissions, restarted, maintenance>>

Passivate ==
    /\ active # 0 /\ active' = 0
    /\ UNCHANGED <<latest, terminal, operationOwner, stage, seen, committed,
                    effective, disk, writer, sent, transmissions, restarted, maintenance>>

Activate ==
    /\ active = 0 /\ Live(latest) /\ active' = latest
    /\ effective' = IF ~Retained(latest) \/ Unsafe = "stale-reload"
                    THEN [effective EXCEPT ![latest] = disk[latest]] ELSE effective
    /\ UNCHANGED <<latest, terminal, operationOwner, stage, seen, committed,
                    disk, writer, sent, transmissions, restarted, maintenance>>

Finish ==
    /\ Live(latest) /\ terminal' = terminal \cup {latest}
    /\ active' = 0
    /\ UNCHANGED <<latest, operationOwner, stage, seen, committed, effective,
                    disk, writer, sent, transmissions, restarted, maintenance>>

Replace ==
    /\ latest \in terminal /\ latest < OwnerCount
    /\ latest' = latest + 1 /\ active' = latest + 1 /\ writer' = latest + 1
    /\ UNCHANGED <<terminal, operationOwner, stage, seen, committed, effective,
                    disk, sent, transmissions, restarted, maintenance>>

Restart ==
    /\ ~restarted /\ restarted' = TRUE /\ active' = 0
    /\ stage' = [p \in Operations |-> "done"]
    /\ seen' = committed /\ effective' = disk
    /\ UNCHANGED <<latest, terminal, operationOwner, committed, disk, writer,
                    sent, transmissions, maintenance>>

MaintainCertificate ==
    /\ maintenance' = ~maintenance
    /\ UNCHANGED <<latest, active, terminal, operationOwner, stage, seen, committed,
                    effective, disk, writer, sent, transmissions, restarted>>

Next == (\E p \in Operations : Start(p) \/ ObserveCompletion(p, TRUE)
                              \/ ObserveCompletion(p, FALSE) \/ CancelTransport(p)
                              \/ FailedPersistence(p) \/ Persist(p) \/ Release(p))
        \/ Passivate \/ Activate \/ Finish \/ Replace \/ Restart \/ MaintainCertificate

TypeOK ==
    /\ latest \in Owners /\ active \in Owners \cup {0} /\ terminal \subseteq Owners
    /\ operationOwner \in [Operations -> Owners \cup {0}]
    /\ stage \in [Operations -> {"idle", "sending", "success", "deferred", "done"}]
    /\ seen \in [Owners -> SUBSET Operations] /\ committed \in [Owners -> SUBSET Operations]
    /\ effective \in [Owners -> Nat] /\ disk \in [Owners -> Nat]
    /\ writer \in Owners /\ sent \subseteq Operations /\ transmissions \in Nat
    /\ restarted \in BOOLEAN /\ maintenance \in BOOLEAN

Inv_ExactEffective == \A o \in Owners : Live(o) => effective[o] = Cardinality(seen[o])
Inv_CommittedPolicy == \A o \in Owners : Live(o) => disk[o] = Cardinality(committed[o])
Inv_CompletionRetained == \A o \in Owners : Live(o) =>
    \A p \in Unwritten(o) : p \in Held /\ operationOwner[p] = o
Inv_PermitBound == Cardinality(Held) <= OperationCap
Inv_CurrentWriter == writer = latest
Inv_NoNetworkOnDrain == transmissions = Cardinality(sent)
Inv_IndependentMaintenance == ENABLED MaintainCertificate
Spec == Init /\ [][Next]_vars
=============================================================================
