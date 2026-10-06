-------------------- MODULE FeeCursorTransition --------------------
EXTENDS Naturals, FiniteSets, Sequences

CONSTANTS Workers, Scopes, PositionCount, MaxRevision,
          CheckRevision, CheckPosition, CheckScope, CheckOverflow, AdvanceOnAbort
ASSUME /\ Workers # {} /\ IsFiniteSet(Workers)
       /\ Scopes # {} /\ IsFiniteSet(Scopes)
       /\ PositionCount \in Nat \ {0} /\ MaxRevision \in Nat \ {0}
       /\ CheckRevision \in BOOLEAN /\ CheckPosition \in BOOLEAN
       /\ CheckScope \in BOOLEAN /\ CheckOverflow \in BOOLEAN
       /\ AdvanceOnAbort \in BOOLEAN

Positions == 0..(PositionCount - 1)
VARIABLES cursors, initial, pending, receipts
vars == <<cursors, initial, pending, receipts>>
Empty == [phase |-> "idle", scope |-> CHOOSE s \in Scopes : TRUE,
          expected |-> [revision |-> 0, position |-> 0], nextPosition |-> 0]

Init ==
    /\ cursors \in [Scopes -> [revision : 0..MaxRevision, position : Positions]]
    /\ initial = cursors
    /\ pending = [w \in Workers |-> Empty]
    /\ receipts = <<>>

Prepare(w, scope, nextPosition) ==
    /\ pending[w].phase = "idle"
    /\ pending' = [pending EXCEPT ![w] =
         [phase |-> "planned", scope |-> scope,
          expected |-> cursors[scope], nextPosition |-> nextPosition]]
    /\ UNCHANGED <<cursors, initial, receipts>>

AlterExpectedPosition(w, position) ==
    /\ pending[w].phase = "planned"
    /\ pending' = [pending EXCEPT ![w].expected.position = position]
    /\ UNCHANGED <<cursors, initial, receipts>>

Commit(w, target) ==
    /\ pending[w].phase = "planned"
    /\ LET plan == pending[w]
           current == cursors[target]
           successor == [revision |-> plan.expected.revision + 1,
                         position |-> plan.nextPosition]
       IN /\ (~CheckScope \/ plan.scope = target)
          /\ (~CheckRevision \/ plan.expected.revision = current.revision)
          /\ (~CheckPosition \/ plan.expected.position = current.position)
          /\ (~CheckOverflow \/ plan.expected.revision < MaxRevision)
          /\ cursors' = [cursors EXCEPT ![target] = successor]
          /\ receipts' = Append(receipts,
               [worker |-> w, scope |-> plan.scope, target |-> target,
                expected |-> plan.expected, before |-> current, after |-> successor])
    /\ pending' = [pending EXCEPT ![w].phase = "done"]
    /\ UNCHANGED initial

Abort(w) ==
    /\ pending[w].phase = "planned"
    /\ cursors' = IF AdvanceOnAbort
                   THEN [cursors EXCEPT ![pending[w].scope].revision = @ + 1]
                   ELSE cursors
    /\ pending' = [pending EXCEPT ![w].phase = "done"]
    /\ UNCHANGED <<initial, receipts>>

Next == (\E w \in Workers, s \in Scopes, p \in Positions : Prepare(w, s, p))
     \/ (\E w \in Workers, p \in Positions : AlterExpectedPosition(w, p))
     \/ (\E w \in Workers, s \in Scopes : Commit(w, s))
     \/ (\E w \in Workers : Abort(w))
Spec == Init /\ [][Next]_vars

ReceiptUsesExpectedRevision ==
    \A i \in 1..Len(receipts) : receipts[i].expected.revision = receipts[i].before.revision
ReceiptUsesExpectedPosition ==
    \A i \in 1..Len(receipts) : receipts[i].expected.position = receipts[i].before.position
ReceiptUsesExpectedScope ==
    \A i \in 1..Len(receipts) : receipts[i].scope = receipts[i].target
RevisionCountsCommits ==
    \A s \in Scopes : cursors[s].revision = initial[s].revision
        + Cardinality({i \in 1..Len(receipts) : receipts[i].target = s})
CursorWithinRepresentation ==
    cursors \in [Scopes -> [revision : 0..MaxRevision, position : Positions]]
OneReceiptPerWorker ==
    \A i, j \in 1..Len(receipts) : receipts[i].worker = receipts[j].worker => i = j

=============================================================================
