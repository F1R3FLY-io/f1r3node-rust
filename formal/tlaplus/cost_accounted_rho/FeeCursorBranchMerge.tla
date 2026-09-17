-------------------- MODULE FeeCursorBranchMerge --------------------
EXTENDS Naturals, FiniteSets, Sequences

CONSTANTS GuardEmptyCells, KeepRejectedMoney, CanonicalOrder
Effects == {1, 2}
Validators == {1, 2}
Scopes == {1, 2}

VARIABLES existed, selected, nextPosition, priority, prepared, pending, kept, charged
vars == <<existed, selected, nextPosition, priority, prepared, pending, kept, charged>>

Init == /\ existed \in [Scopes -> BOOLEAN]
        /\ selected \in [Effects -> Scopes]
        /\ nextPosition \in [Effects -> 0..1]
        /\ priority \in {<<1, 2>>, <<2, 1>>}
        /\ prepared = {}
        /\ pending = [v \in Validators |-> Effects]
        /\ kept = [v \in Validators |-> {}]
        /\ charged = [v \in Validators |-> {}]

Prepare(e) == /\ e \notin prepared
              /\ prepared' = prepared \cup {e}
              /\ UNCHANGED <<existed, selected, nextPosition, priority, pending, kept, charged>>

Rank(e) == IF priority[1] = e THEN 0 ELSE 1
Writers(v, s) == {e \in kept[v] : selected[e] = s}
Done(v) == pending[v] = {}

MergeOne(v, e) ==
    /\ prepared = Effects
    /\ e \in pending[v]
    /\ ~CanonicalOrder \/ (\A other \in pending[v] : Rank(e) <= Rank(other))
    /\ LET fresh == Writers(v, selected[e]) = {}
           accept == fresh \/ (~existed[selected[e]] /\ ~GuardEmptyCells)
       IN /\ kept' = [kept EXCEPT ![v] = IF accept THEN @ \cup {e} ELSE @]
          /\ charged' = [charged EXCEPT ![v] =
               IF accept \/ KeepRejectedMoney THEN @ \cup {e} ELSE @]
    /\ pending' = [pending EXCEPT ![v] = @ \ {e}]
    /\ UNCHANGED <<existed, selected, nextPosition, priority, prepared>>

Next == (\E e \in Effects : Prepare(e))
     \/ (\E v \in Validators, e \in Effects : MergeOne(v, e))
Spec == Init /\ [][Next]_vars

RevisionCells(v, s) ==
    {[origin |-> e, value |-> IF existed[s] THEN 2 ELSE 1] : e \in Writers(v, s)}
PositionCells(v, s) ==
    {[origin |-> e, value |-> nextPosition[e]] : e \in Writers(v, s)}

CursorCellUnique == \A v \in Validators, s \in Scopes :
    /\ Cardinality(RevisionCells(v, s)) <= 1
    /\ Cardinality(PositionCells(v, s)) <= 1

WholeEffectAccounting == \A v \in Validators : charged[v] = kept[v]

IndependentScopesSurvive == selected[1] # selected[2] =>
    (\A v \in Validators : Done(v) => kept[v] = Effects)

SameScopeKeepsOne == selected[1] = selected[2] =>
    (\A v \in Validators : Done(v) => Cardinality(kept[v]) = 1)

ValidatorAgreement == Done(1) /\ Done(2) =>
    /\ kept[1] = kept[2]
    /\ charged[1] = charged[2]
    /\ \A s \in Scopes :
         /\ RevisionCells(1, s) = RevisionCells(2, s)
         /\ PositionCells(1, s) = PositionCells(2, s)

NoPhantomEffects == \A v \in Validators :
    /\ kept[v] \subseteq prepared
    /\ kept[v] \cap pending[v] = {}

=====================================================================
