-------------------- MODULE ContributionCursor --------------------
EXTENDS Naturals, FiniteSets, Sequences

CONSTANTS Workers, Scopes, PositionCount, MaxRevision, ZeroWrite
ASSUME /\ Workers # {} /\ IsFiniteSet(Workers)
       /\ Scopes # {} /\ IsFiniteSet(Scopes)
       /\ PositionCount \in Nat \ {0} /\ MaxRevision \in Nat \ {0}
       /\ ZeroWrite \in {"none", "snapshot", "successor"}

Positions == 0..(PositionCount - 1)
VARIABLES cursors, initial, pending, receipts
vars == <<cursors, initial, pending, receipts>>
Empty == [phase |-> "idle", scope |-> CHOOSE s \in Scopes : TRUE,
          expected |-> [revision |-> 0, position |-> 0], nextPosition |-> 0, zero |-> TRUE]

Init ==
    /\ cursors \in [Scopes -> [revision : 0..MaxRevision, position : Positions]]
    /\ initial = cursors
    /\ pending = [w \in Workers |-> Empty]
    /\ receipts = <<>>

Prepare(w, scope, position, zero) ==
    /\ pending[w].phase = "idle"
    /\ pending' = [pending EXCEPT ![w] = [phase |-> "planned", scope |-> scope,
         expected |-> cursors[scope], nextPosition |-> position, zero |-> zero]]
    /\ UNCHANGED <<cursors, initial, receipts>>

Commit(w) ==
    /\ pending[w].phase = "planned"
    /\ LET plan == pending[w]
           current == cursors[plan.scope]
           successor == [revision |-> plan.expected.revision + 1, position |-> plan.nextPosition]
           result == IF ~plan.zero THEN successor
                     ELSE IF ZeroWrite = "snapshot" THEN plan.expected
                     ELSE IF ZeroWrite = "successor" THEN successor ELSE current
       IN /\ plan.zero \/ (plan.expected = current /\ current.revision < MaxRevision)
          /\ cursors' = [cursors EXCEPT ![plan.scope] = result]
          /\ receipts' = Append(receipts, [worker |-> w, scope |-> plan.scope,
               zero |-> plan.zero, expected |-> plan.expected, before |-> current, after |-> result])
    /\ pending' = [pending EXCEPT ![w].phase = "done"]
    /\ UNCHANGED initial

Abort(w) ==
    /\ pending[w].phase = "planned"
    /\ pending' = [pending EXCEPT ![w].phase = "done"]
    /\ UNCHANGED <<cursors, initial, receipts>>

Next == (\E w \in Workers, s \in Scopes, p \in Positions, zero \in BOOLEAN : Prepare(w, s, p, zero))
     \/ (\E w \in Workers : Commit(w) \/ Abort(w))
Spec == Init /\ [][Next]_vars

ZeroHasNoWrite ==
    \A i \in 1..Len(receipts) : receipts[i].zero => receipts[i].after = receipts[i].before
RevisionCountsPositiveCommits ==
    \A s \in Scopes : cursors[s].revision = initial[s].revision
        + Cardinality({i \in 1..Len(receipts) : receipts[i].scope = s /\ ~receipts[i].zero})
PositiveUsesCurrentCursor ==
    \A i \in 1..Len(receipts) : ~receipts[i].zero => receipts[i].expected = receipts[i].before
CursorWithinRepresentation ==
    cursors \in [Scopes -> [revision : 0..MaxRevision, position : Positions]]
OneReceiptPerWorker ==
    \A i, j \in 1..Len(receipts) : receipts[i].worker = receipts[j].worker => i = j

=============================================================================
