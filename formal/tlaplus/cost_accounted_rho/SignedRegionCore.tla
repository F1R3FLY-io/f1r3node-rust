--------------------------- MODULE SignedRegionCore ---------------------------
EXTENDS Naturals

CONSTANT Fault

ASSUME Fault \in {"safe", "erased_region", "span_alias", "partial_join"}

VARIABLES allocated, binderKeys, tokenA, tokenB, spentA, spentB, joinPhase,
          continuationSeal

vars == <<allocated, binderKeys, tokenA, tokenB, spentA, spentB, joinPhase,
          continuationSeal>>

SourceSpan(i) == 1

BinderKey(i) ==
    IF Fault = "span_alias" THEN <<SourceSpan(i), 0>>
    ELSE <<SourceSpan(i), i>>

Init ==
    /\ allocated = {}
    /\ binderKeys = [i \in 1..2 |-> <<0, 0>>]
    /\ tokenA = 1
    /\ tokenB \in {0, 1}
    /\ spentA = 0
    /\ spentB = 0
    /\ joinPhase = "ready"
    /\ continuationSeal = "none"

Allocate(i) ==
    /\ i \in 1..2
    /\ i \notin allocated
    /\ allocated' = allocated \cup {i}
    /\ binderKeys' = [binderKeys EXCEPT ![i] = BinderKey(i)]
    /\ UNCHANGED <<tokenA, tokenB, spentA, spentB, joinPhase, continuationSeal>>

FireJoin ==
    /\ joinPhase = "ready"
    /\ allocated = {1, 2}
    /\ tokenA = 1
    /\ tokenB = 1
    /\ tokenA' = 0
    /\ tokenB' = 0
    /\ spentA' = 1
    /\ spentB' = 1
    /\ joinPhase' = "done"
    /\ continuationSeal' =
          IF Fault = "erased_region" THEN "none" ELSE "continuation_owner"
    /\ UNCHANGED <<allocated, binderKeys>>

PartialDebit ==
    /\ Fault = "partial_join"
    /\ joinPhase = "ready"
    /\ allocated = {1, 2}
    /\ tokenA = 1
    /\ tokenB = 1
    /\ tokenA' = 0
    /\ spentA' = 1
    /\ joinPhase' = "partial"
    /\ UNCHANGED <<allocated, binderKeys, tokenB, spentB, continuationSeal>>

FinishPartial ==
    /\ Fault = "partial_join"
    /\ joinPhase = "partial"
    /\ tokenB = 1
    /\ tokenB' = 0
    /\ spentB' = 1
    /\ joinPhase' = "done"
    /\ continuationSeal' = "continuation_owner"
    /\ UNCHANGED <<allocated, binderKeys, tokenA, spentA>>

Next ==
    \/ (\E i \in 1..2 : Allocate(i))
    \/ FireJoin
    \/ PartialDebit
    \/ FinishPartial

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ allocated \subseteq {1, 2}
    /\ binderKeys \in [1..2 -> {<<0, 0>>, <<1, 0>>, <<1, 1>>, <<1, 2>>}]
    /\ tokenA \in 0..1
    /\ tokenB \in 0..1
    /\ spentA \in 0..1
    /\ spentB \in 0..1
    /\ joinPhase \in {"ready", "partial", "done"}
    /\ continuationSeal \in {"none", "continuation_owner"}

Inv_ContinuationSealPreserved ==
    joinPhase = "done" => continuationSeal = "continuation_owner"

Inv_FreshBinderIsolation ==
    {1, 2} \subseteq allocated => binderKeys[1] # binderKeys[2]

Inv_AtomicJoinDebit == spentA = spentB

Inv_NoUnfundedJoin == joinPhase = "done" => spentA = 1 /\ spentB = 1

Inv_TokenConservation == tokenA + spentA = 1 /\ tokenB + spentB \in {0, 1}
=============================================================================
