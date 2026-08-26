------------------------- MODULE FinalizationBoundHead ------------------------
EXTENDS Integers, TLC

CONSTANT
    \* @type: Bool;
    UnsafeLateBoundHead

ASSUME UnsafeLateBoundHead \in BOOLEAN

Workers == {1, 2}
Blocks == {"F0", "F1", "C", "D"}
Candidates == Blocks \ {"F0"}
Phases == {"Idle", "Evaluated", "Done"}

DAGDescends(base, candidate) ==
    \/ base = candidate
    \/ base = "F0"
    \/ /\ base = "F1"
       /\ candidate \in {"C", "D"}

StatePreserves(base, candidate) ==
    \/ base = candidate
    \/ /\ base = "F0"
       /\ candidate \in Candidates
    \/ /\ base = "F1"
       /\ candidate = "D"

VARIABLES
    \* @type: Int;
    revision,
    \* @type: Str;
    head,
    \* @type: Int -> Str;
    records,
    \* @type: Int -> Str;
    phase,
    \* @type: Int -> Int;
    expectedRevision,
    \* @type: Int -> Str;
    expectedHead,
    \* @type: Int -> Str;
    candidate

vars == <<revision, head, records, phase, expectedRevision, expectedHead,
          candidate>>

Init ==
    /\ revision = 0
    /\ head = "F0"
    /\ records = [index \in 0..2 |-> "F0"]
    /\ phase = [worker \in Workers |-> "Idle"]
    /\ expectedRevision = [worker \in Workers |-> 0]
    /\ expectedHead = [worker \in Workers |-> "F0"]
    /\ candidate = [worker \in Workers |-> "F0"]

Evaluate(worker, block) ==
    /\ phase[worker] = "Idle"
    /\ block \in Candidates
    /\ DAGDescends(head, block)
    /\ StatePreserves(head, block)
    /\ phase' = [phase EXCEPT ![worker] = "Evaluated"]
    /\ expectedRevision' = [expectedRevision EXCEPT ![worker] = revision]
    /\ expectedHead' = [expectedHead EXCEPT ![worker] = head]
    /\ candidate' = [candidate EXCEPT ![worker] = block]
    /\ UNCHANGED <<revision, head, records>>

CommitBound(worker) ==
    /\ phase[worker] = "Evaluated"
    /\ expectedRevision[worker] = revision
    /\ expectedHead[worker] = head
    /\ StatePreserves(expectedHead[worker], candidate[worker])
    /\ revision' = revision + 1
    /\ head' = candidate[worker]
    /\ records' = [records EXCEPT ![revision + 1] = candidate[worker]]
    /\ phase' = [phase EXCEPT ![worker] = "Done"]
    /\ UNCHANGED <<expectedRevision, expectedHead, candidate>>

RejectStale(worker) ==
    /\ phase[worker] = "Evaluated"
    /\ \/ expectedRevision[worker] # revision
       \/ expectedHead[worker] # head
    /\ phase' = [phase EXCEPT ![worker] = "Done"]
    /\ UNCHANGED <<revision, head, records, expectedRevision, expectedHead,
                    candidate>>

UnsafeCommitAgainstCurrentHead(worker) ==
    /\ UnsafeLateBoundHead
    /\ phase[worker] = "Evaluated"
    /\ \/ expectedRevision[worker] # revision
       \/ expectedHead[worker] # head
    /\ DAGDescends(head, candidate[worker])
    /\ revision' = revision + 1
    /\ head' = candidate[worker]
    /\ records' = [records EXCEPT ![revision + 1] = candidate[worker]]
    /\ phase' = [phase EXCEPT ![worker] = "Done"]
    /\ UNCHANGED <<expectedRevision, expectedHead, candidate>>

StayDone ==
    /\ \A worker \in Workers : phase[worker] = "Done"
    /\ UNCHANGED vars

Next ==
    \/ \E worker \in Workers, block \in Candidates : Evaluate(worker, block)
    \/ \E worker \in Workers : CommitBound(worker)
    \/ \E worker \in Workers : RejectStale(worker)
    \/ \E worker \in Workers : UnsafeCommitAgainstCurrentHead(worker)
    \/ StayDone

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ revision \in 0..2
    /\ head \in Blocks
    /\ records \in [0..2 -> Blocks]
    /\ phase \in [Workers -> Phases]
    /\ expectedRevision \in [Workers -> 0..2]
    /\ expectedHead \in [Workers -> Blocks]
    /\ candidate \in [Workers -> Blocks]

Inv_HeadMatchesLedger == head = records[revision]

Inv_AdjacentStatePreservation ==
    \A index \in 1..revision :
        StatePreserves(records[index - 1], records[index])

Safety ==
    /\ TypeOK
    /\ Inv_HeadMatchesLedger
    /\ Inv_AdjacentStatePreservation

=============================================================================
