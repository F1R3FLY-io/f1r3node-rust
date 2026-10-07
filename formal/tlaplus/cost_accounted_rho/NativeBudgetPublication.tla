-------------------- MODULE NativeBudgetPublication --------------------
EXTENDS Integers, FiniteSets, Sequences

CONSTANTS CheckGeneration, ChargeRetries, CheckRetryCut
Workers == {1, 2, 3}
Identity(w) == IF w = 3 THEN 2 ELSE 1
Cost(w) == IF w = 3 THEN 2 ELSE 1
Limit == 2

VARIABLES generation, prepared, completed, rows, receipts, used
vars == <<generation, prepared, completed, rows, receipts, used>>
FreshCount == Cardinality({i \in 1..Len(rows) : ~rows[i].retry})
Init ==
  /\ generation = 0
  /\ prepared = [w \in Workers |-> -1]
  /\ completed = {}
  /\ rows = <<>>
  /\ receipts = {}
  /\ used = 0

Prepare(w) ==
  /\ prepared[w] = -1
  /\ prepared' = [prepared EXCEPT ![w] = generation]
  /\ UNCHANGED <<generation, completed, rows, receipts, used>>
Reset ==
  /\ generation = 0
  /\ generation' = 1
  /\ rows' = <<>>
  /\ receipts' = {}
  /\ used' = 0
  /\ UNCHANGED <<prepared, completed>>
Publish(w) ==
  /\ prepared[w] # -1
  /\ w \notin completed
  /\ completed' = completed \cup {w}
  /\ IF CheckGeneration /\ prepared[w] # generation
     THEN UNCHANGED <<rows, receipts, used>>
     ELSE LET retry == Identity(w) \in receipts /\ ~ChargeRetries
              accepted == retry \/ used + Cost(w) <= Limit
          IN /\ rows' = Append(rows, [worker |-> w, generation |-> prepared[w],
                    retry |-> retry, accepted |-> accepted,
                    freshBefore |-> IF retry /\ ~CheckRetryCut THEN 0 ELSE FreshCount])
             /\ receipts' = IF accepted THEN receipts \cup {Identity(w)} ELSE receipts
             /\ used' = IF accepted /\ ~retry THEN used + Cost(w) ELSE used
  /\ UNCHANGED <<generation, prepared>>
Next == Reset \/ (\E w \in Workers : Prepare(w) \/ Publish(w))
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ generation \in {0, 1}
  /\ prepared \in [Workers -> {-1, 0, 1}]
  /\ completed \subseteq Workers
  /\ receipts \subseteq {1, 2}
  /\ used \in 0..Limit
  /\ rows \in Seq([worker : Workers, generation : {0, 1}, retry : BOOLEAN,
                  accepted : BOOLEAN, freshBefore : 0..3])
CurrentGenerationOnly == \A i \in 1..Len(rows) : rows[i].generation = generation
ReceiptUsage == used = (IF 1 \in receipts THEN 1 ELSE 0) + (IF 2 \in receipts THEN 2 ELSE 0)
RetryHasPriorAcceptance == \A i \in 1..Len(rows) : rows[i].retry =>
  \E j \in 1..(i-1) : rows[j].accepted /\ ~rows[j].retry
    /\ Identity(rows[j].worker) = Identity(rows[i].worker)
RetryCutFollowsAcceptance == \A i \in 1..Len(rows) : rows[i].retry =>
  \E j \in 1..(i-1) : rows[j].accepted /\ ~rows[j].retry
    /\ Identity(rows[j].worker) = Identity(rows[i].worker)
    /\ rows[j].freshBefore < rows[i].freshBefore
=============================================================================
