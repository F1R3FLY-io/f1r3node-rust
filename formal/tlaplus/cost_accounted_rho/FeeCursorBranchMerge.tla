-------------------- MODULE FeeCursorBranchMerge --------------------
EXTENDS Naturals, FiniteSets, Sequences

\* Changed by DR-119 (bug 11004): the model also covers the creation-lock rule
\* of SystemVault. LockMode "global" is the lock that every settlement took
\* before DR-119. "bucket" is DR-119: an existing cursor takes no lock, and a
\* first use takes the lock of its bucket. "none" is a control without any
\* creation lock. BucketsRespectLeaves states that one TreeHashMap leaf has one
\* bucket; DR-119 derives both from the scope's keccak256 hash.
\* CONSTANTS GuardEmptyCells, KeepRejectedMoney, CanonicalOrder
CONSTANTS GuardEmptyCells, KeepRejectedMoney, CanonicalOrder,
          LockMode, BucketsRespectLeaves
ASSUME /\ LockMode \in {"global", "bucket", "none"}
       /\ BucketsRespectLeaves \in BOOLEAN
Effects == {1, 2}
Validators == {1, 2}
Scopes == {1, 2}
Leaves == {1, 2}
Buckets == {1, 2}

\* VARIABLES existed, selected, nextPosition, priority, prepared, pending, kept, charged
\* vars == <<existed, selected, nextPosition, priority, prepared, pending, kept, charged>>
VARIABLES existed, selected, nextPosition, priority, prepared, pending, kept, charged,
          leafOf, bucketOf
vars == <<existed, selected, nextPosition, priority, prepared, pending, kept, charged,
          leafOf, bucketOf>>

\* A bucket map respects the leaves when two scopes in one leaf share a bucket.
RespectsLeaves(leaves, buckets) ==
    \A s, t \in Scopes : leaves[s] = leaves[t] => buckets[s] = buckets[t]

Init == /\ existed \in [Scopes -> BOOLEAN]
        /\ selected \in [Effects -> Scopes]
        /\ nextPosition \in [Effects -> 0..1]
        /\ priority \in {<<1, 2>>, <<2, 1>>}
        /\ leafOf \in [Scopes -> Leaves]
        /\ bucketOf \in {b \in [Scopes -> Buckets] :
                           BucketsRespectLeaves => RespectsLeaves(leafOf, b)}
        /\ prepared = {}
        /\ pending = [v \in Validators |-> Effects]
        /\ kept = [v \in Validators |-> {}]
        /\ charged = [v \in Validators |-> {}]

Prepare(e) == /\ e \notin prepared
              /\ prepared' = prepared \cup {e}
              /\ UNCHANGED <<existed, selected, nextPosition, priority, pending, kept, charged,
                             leafOf, bucketOf>>

Rank(e) == IF priority[1] = e THEN 0 ELSE 1
Writers(v, s) == {e \in kept[v] : selected[e] = s}
Done(v) == pending[v] = {}

\* The base datums that an effect's settlement consumes. A first use creates
\* its cells and consumes them inside its own deployment.
Cells(s) == {<<"revision", s>>, <<"position", s>>}
Claims(e) ==
    LET s == selected[e]
        cells == IF existed[s] THEN Cells(s) ELSE {}
    IN CASE LockMode = "global" -> {<<"lock", 0>>} \cup cells
         [] LockMode = "bucket" -> IF existed[s] THEN cells ELSE {<<"bucket", bucketOf[s]>>}
         [] LockMode = "none" -> cells
KeptClaims(v) == UNION {Claims(k) : k \in kept[v]}

\* The race rule: a merge keeps an effect only when its claims are disjoint
\* from the claims of the effects that it already kept.
MergeOne(v, e) ==
    /\ prepared = Effects
    /\ e \in pending[v]
    /\ ~CanonicalOrder \/ (\A other \in pending[v] : Rank(e) <= Rank(other))
    /\ LET fresh == Writers(v, selected[e]) = {}
           cellsAccepted == fresh \/ (~existed[selected[e]] /\ ~GuardEmptyCells)
           claimsAccepted == Claims(e) \cap KeptClaims(v) = {}
           accept == cellsAccepted /\ claimsAccepted
       IN /\ kept' = [kept EXCEPT ![v] = IF accept THEN @ \cup {e} ELSE @]
          /\ charged' = [charged EXCEPT ![v] =
               IF accept \/ KeepRejectedMoney THEN @ \cup {e} ELSE @]
    /\ pending' = [pending EXCEPT ![v] = @ \ {e}]
    /\ UNCHANGED <<existed, selected, nextPosition, priority, prepared, leafOf, bucketOf>>

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

\* Changed by DR-119: this property never held for the code before DR-119,
\* because the global lock made every two settlements conflict. Under DR-119
\* two first uses that share a bucket still conflict, so the property holds
\* only without such a shared bucket (IndependentScopesSurviveRefined).
\* IndependentScopesSurvive == selected[1] # selected[2] =>
\*     (\A v \in Validators : Done(v) => kept[v] = Effects)
SharedCreationBucket ==
    /\ ~existed[selected[1]]
    /\ ~existed[selected[2]]
    /\ bucketOf[selected[1]] = bucketOf[selected[2]]
IndependentScopesSurviveRefined ==
    (selected[1] # selected[2] /\ ~SharedCreationBucket) =>
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

\* Added by DR-119: a merge never keeps two first uses of different scopes in
\* one TreeHashMap leaf. Their setters would leave two datums on the leaf,
\* because the merge does not see the race on a bitmask-tagged leaf.
CreatedScopes(v, l) ==
    {selected[e] : e \in {k \in kept[v] : ~existed[selected[k]] /\ leafOf[selected[k]] = l}}
MapLeafUnique == \A v \in Validators, l \in Leaves : Cardinality(CreatedScopes(v, l)) <= 1

=====================================================================
