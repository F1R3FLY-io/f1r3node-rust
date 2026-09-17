-------------------- MODULE JointFundingSettlement --------------------
EXTENDS MonetaryAllocation

CONSTANTS Workers, Transactions, ResourceDemand, ResourceEligible, FeeEligible,
          CheckFreshness, CommitResourceCursor, CommitFeeCursor
ASSUME /\ Workers # {} /\ IsFiniteSet(Workers)
       /\ Transactions # {} /\ IsFiniteSet(Transactions)
       /\ ResourceDemand \in Nat
       /\ ResourceEligible \subseteq Payers
       /\ FeeEligible \subseteq Payers
       /\ CheckFreshness \in BOOLEAN
       /\ CommitResourceCursor \in BOOLEAN
       /\ CommitFeeCursor \in BOOLEAN

CappedDomain(caps) ==
    {draw \in [Payers -> 0..CapacityBound] :
       Sum(draw) = ResourceDemand /\ \A p \in Payers : draw[p] <= caps[p]}
JointDomain(caps) ==
    {draw \in CappedDomain(caps) :
       /\ \A p \in Payers \ ResourceEligible : draw[p] = 0
       /\ \E p \in FeeEligible : draw[p] < caps[p]}
CountAt(draw, level) == Cardinality({p \in Payers : draw[p] = level})
BetterRank(left, right) ==
    \E level \in 0..CapacityBound :
       /\ CountAt(left, level) < CountAt(right, level)
       /\ \A higher \in (level + 1)..CapacityBound : CountAt(left, higher) = CountAt(right, higher)
EqualRank(left, right) ==
    \A level \in 0..CapacityBound : CountAt(left, level) = CountAt(right, level)
CyclicGreater(left, right, start) ==
    \E p \in Payers :
       /\ left[p] > right[p]
       /\ \A q \in Payers : Distance(q, start) < Distance(p, start) => left[q] = right[q]
ResourceAllocation(caps, start) ==
    CHOOSE draw \in JointDomain(caps) :
      \A alternative \in JointDomain(caps) :
        /\ ~BetterRank(alternative, draw)
        /\ EqualRank(alternative, draw) => ~CyclicGreater(alternative, draw, start)
ResourceNext(caps, start) ==
    IF ResourceDemand = 0 THEN start
    ELSE IF JointDomain(caps) = CappedDomain(caps)
         THEN NextCursor(caps, ResourceDemand, start)
         ELSE (start + 1) % PayerCount
FeeCaps(caps, resource) ==
    [p \in Payers |-> IF p \in FeeEligible THEN caps[p] - resource[p] ELSE 0]
Plan(caps, resourceStart, feeStart) ==
    LET resource == ResourceAllocation(caps, resourceStart)
        feeCaps == FeeCaps(caps, resource)
    IN [resource |-> resource, fee |-> Allocation(feeCaps, 1, feeStart),
        resourceAfter |-> ResourceNext(caps, resourceStart),
        feeAfter |-> NextCursor(feeCaps, 1, feeStart)]

VARIABLES balances, resourcePosition, feePosition, pending, completed,
          receipts, initialBalances, initialResourcePosition, initialFeePosition
vars == <<balances, resourcePosition, feePosition, pending, completed,
          receipts, initialBalances, initialResourcePosition, initialFeePosition>>
Empty == [active |-> FALSE]
Init ==
    /\ balances \in [Payers -> 0..CapacityBound]
    /\ resourcePosition \in Payers
    /\ feePosition \in Payers
    /\ initialBalances = balances
    /\ initialResourcePosition = resourcePosition
    /\ initialFeePosition = feePosition
    /\ pending = [w \in Workers |-> Empty]
    /\ completed = {}
    /\ receipts = <<>>

Prepare(w, tx) ==
    /\ ~pending[w].active
    /\ tx \notin completed
    /\ JointDomain(balances) # {}
    /\ pending' = [pending EXCEPT ![w] =
         [active |-> TRUE, transaction |-> tx, before |-> balances,
          resourceStart |-> resourcePosition, feeStart |-> feePosition,
          plan |-> Plan(balances, resourcePosition, feePosition)]]
    /\ UNCHANGED <<balances, resourcePosition, feePosition, completed, receipts,
                    initialBalances, initialResourcePosition, initialFeePosition>>

Commit(w) ==
    /\ pending[w].active
    /\ LET captured == pending[w]
           plan == captured.plan
       IN /\ captured.transaction \notin completed
          /\ (~CheckFreshness \/
               (captured.before = balances /\ captured.resourceStart = resourcePosition /\ captured.feeStart = feePosition))
          /\ \A p \in Payers : plan.resource[p] + plan.fee[p] <= balances[p]
          /\ balances' = [p \in Payers |-> balances[p] - plan.resource[p] - plan.fee[p]]
          /\ resourcePosition' = IF CommitResourceCursor THEN plan.resourceAfter ELSE resourcePosition
          /\ feePosition' = IF CommitFeeCursor THEN plan.feeAfter ELSE feePosition
          /\ completed' = completed \cup {captured.transaction}
          /\ receipts' = Append(receipts,
               [before |-> balances, resourceStart |-> resourcePosition, feeStart |-> feePosition,
                plan |-> plan, resourceAfter |-> resourcePosition', feeAfter |-> feePosition',
                transaction |-> captured.transaction])
    /\ pending' = [pending EXCEPT ![w] = Empty]
    /\ UNCHANGED <<initialBalances, initialResourcePosition, initialFeePosition>>

Abort(w) ==
    /\ pending[w].active
    /\ pending' = [pending EXCEPT ![w] = Empty]
    /\ UNCHANGED <<balances, resourcePosition, feePosition, completed, receipts,
                    initialBalances, initialResourcePosition, initialFeePosition>>

Next == (\E w \in Workers, tx \in Transactions : Prepare(w, tx))
        \/ (\E w \in Workers : Commit(w) \/ Abort(w))
Spec == Init /\ [][Next]_vars

MoneyConserved == Sum(balances) + Len(receipts) * (ResourceDemand + 1) = Sum(initialBalances)
BalancesStayNatural == balances \in [Payers -> Nat]
NoDuplicateSettlement == Cardinality(completed) = Len(receipts)
EveryReceiptUsesCurrentPlan ==
    \A index \in 1..Len(receipts) :
      LET receipt == receipts[index]
      IN /\ JointDomain(receipt.before) # {}
         /\ receipt.plan = Plan(receipt.before, receipt.resourceStart, receipt.feeStart)
         /\ receipt.resourceAfter = receipt.plan.resourceAfter
         /\ receipt.feeAfter = receipt.plan.feeAfter
CursorsChangeOnlyWithSettlement ==
    /\ resourcePosition = IF receipts = <<>> THEN initialResourcePosition ELSE receipts[Len(receipts)].resourceAfter
    /\ feePosition = IF receipts = <<>> THEN initialFeePosition ELSE receipts[Len(receipts)].feeAfter

=============================================================================
