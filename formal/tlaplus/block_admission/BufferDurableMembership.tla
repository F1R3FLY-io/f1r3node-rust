------------------------ MODULE BufferDurableMembership ------------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Blocks, Certificates, Clients, PreserveReadyRows, PreserveOrphanRows,
          RestoreEmptyRows, PublishAfterCommit, AtomicRemoval

Keys == Blocks \cup Certificates
EmptyRow == [present |-> FALSE, parents |-> {}]
EmptyRows == [key \in Keys |-> EmptyRow]

VARIABLES disk, memory, expected, plan, owner, phase, operation, target

vars == <<disk, memory, expected, plan, owner, phase, operation, target>>

Explicit(rows) == {key \in Keys : rows[key].present}
References(rows) == UNION {rows[key].parents : key \in Explicit(rows)}
Candidates(rows) == (Explicit(rows) \cup References(rows)) \ Certificates
Ready(rows) == {key \in Explicit(rows) \cup References(rows) : rows[key].parents = {}}

Ensure(rows, child) ==
    [rows EXCEPT ![child] = [present |-> TRUE, parents |-> @.parents]]

Add(rows, parent, child) ==
    [rows EXCEPT ![child] = [present |-> TRUE, parents |-> @.parents \cup {parent}]]

Remove(rows, key) ==
    [child \in Keys |-> IF child = key THEN EmptyRow
                       ELSE [present |-> rows[child].present,
                             parents |-> rows[child].parents \ {key}]]

Init ==
    /\ disk = EmptyRows
    /\ memory = EmptyRows
    /\ expected = EmptyRows
    /\ plan = EmptyRows
    /\ owner = "none"
    /\ phase = "idle"
    /\ operation = "none"
    /\ target \in Keys

Prepare(client, nextRows, kind, key) ==
    /\ phase = "idle"
    /\ owner' = client
    /\ phase' = "prepared"
    /\ plan' = nextRows
    /\ operation' = kind
    /\ target' = key
    /\ memory' = IF PublishAfterCommit THEN memory ELSE nextRows
    /\ UNCHANGED <<disk, expected>>

RemovedOrphan(key) ==
    /\ key \in disk[target].parents
    /\ ~\E child \in Keys : key \in plan[child].parents

CommittedRows ==
    [key \in Keys |->
        IF operation = "remove" /\ key # target /\
           ((~PreserveReadyRows /\ target \in disk[key].parents /\ plan[key].parents = {})
            \/ (~PreserveOrphanRows /\ RemovedOrphan(key)))
        THEN EmptyRow ELSE plan[key]]

Commit ==
    /\ phase = "prepared"
    /\ AtomicRemoval \/ operation # "remove"
    /\ disk' = CommittedRows
    /\ expected' = plan
    /\ phase' = "committed"
    /\ UNCHANGED <<memory, plan, owner, operation, target>>

PartialRemove ==
    /\ phase = "prepared"
    /\ operation = "remove"
    /\ ~AtomicRemoval
    /\ disk' = [key \in Keys |-> IF key = target THEN disk[key] ELSE plan[key]]
    /\ phase' = "partial"
    /\ UNCHANGED <<memory, expected, plan, owner, operation, target>>

FinishPartialRemove ==
    /\ phase = "partial"
    /\ disk' = plan
    /\ expected' = plan
    /\ phase' = "committed"
    /\ UNCHANGED <<memory, plan, owner, operation, target>>

Publish ==
    /\ phase = "committed"
    /\ memory' = plan
    /\ phase' = "idle"
    /\ owner' = "none"
    /\ UNCHANGED <<disk, expected, plan, operation, target>>

Abort ==
    /\ phase = "prepared"
    /\ phase' = "idle"
    /\ owner' = "none"
    /\ UNCHANGED <<disk, memory, expected, plan, operation, target>>

CrashRestart ==
    /\ memory' = [key \in Keys |->
          IF RestoreEmptyRows \/ disk[key].parents # {} THEN disk[key] ELSE EmptyRow]
    /\ phase' = "idle"
    /\ owner' = "none"
    /\ UNCHANGED <<disk, expected, plan, operation, target>>

Next ==
    \/ \E client \in Clients, child \in Blocks :
         Prepare(client, Ensure(memory, child), "ensure", child)
    \/ \E client \in Clients, child \in Blocks, parent \in Keys :
         Prepare(client, Add(memory, parent, child), "add", child)
    \/ \E client \in Clients, key \in Keys :
         Prepare(client, Remove(memory, key), "remove", key)
    \/ Commit
    \/ PartialRemove
    \/ FinishPartialRemove
    \/ Publish
    \/ Abort
    \/ CrashRestart

RowType(rows) ==
    /\ rows \in [Keys -> [present : BOOLEAN, parents : SUBSET Keys]]
    /\ \A key \in Keys : ~rows[key].present => rows[key].parents = {}
    /\ Explicit(rows) \subseteq Blocks

TypeOK ==
    /\ RowType(disk) /\ RowType(memory) /\ RowType(expected) /\ RowType(plan)
    /\ phase \in {"idle", "prepared", "committed", "partial"}
    /\ owner \in Clients \cup {"none"}
    /\ (phase = "idle") = (owner = "none")
    /\ operation \in {"none", "ensure", "add", "remove"}
    /\ target \in Keys

Inv_DurableMembership == disk = expected
Inv_PublishedProjection == phase = "idle" => memory = disk
Inv_RestartCandidates == phase = "idle" => Candidates(memory) = Candidates(expected)
Inv_CertificateSeparation == Candidates(memory) \cap Certificates = {}
Safety == TypeOK /\ Inv_DurableMembership /\ Inv_PublishedProjection
          /\ Inv_RestartCandidates /\ Inv_CertificateSeparation
Spec == Init /\ [][Next]_vars
=============================================================================
