-------------------- MODULE RotatingFeeSettlement --------------------
EXTENDS MonetaryAllocation

CONSTANTS Workers, Transactions, CheckFreshness, CommitCursor, AdvanceOnAbort
ASSUME /\ Workers # {} /\ IsFiniteSet(Workers)
       /\ Transactions # {} /\ IsFiniteSet(Transactions)
       /\ CheckFreshness \in BOOLEAN
       /\ CommitCursor \in BOOLEAN
       /\ AdvanceOnAbort \in BOOLEAN

VARIABLES balances, position, pending, completed, receipts, fees,
          initialBalances, initialPosition
settlementVars == <<balances, position, pending, completed, receipts, fees,
                    initialBalances, initialPosition>>
Empty == [active |-> FALSE, transaction |-> CHOOSE tx \in Transactions : TRUE,
          before |-> [p \in Payers |-> 0], start |-> 0,
          allocation |-> [p \in Payers |-> 0], after |-> 0]
SettlementInit ==
    /\ balances \in [Payers -> 0..CapacityBound]
    /\ initialBalances = balances
    /\ position \in Payers
    /\ initialPosition = position
    /\ pending = [w \in Workers |-> Empty]
    /\ completed = {}
    /\ receipts = <<>>
    /\ fees = 0

Prepare(w, tx) ==
    /\ ~pending[w].active
    /\ tx \notin completed
    /\ Sum(balances) >= 1
    /\ pending' = [pending EXCEPT ![w] =
         [active |-> TRUE, transaction |-> tx, before |-> balances, start |-> position,
          allocation |-> Allocation(balances, 1, position),
          after |-> NextCursor(balances, 1, position)]]
    /\ UNCHANGED <<balances, position, completed, receipts, fees,
                    initialBalances, initialPosition>>

Commit(w) ==
    /\ pending[w].active
    /\ LET plan == pending[w]
       IN /\ plan.transaction \notin completed
          /\ (~CheckFreshness \/ (plan.before = balances /\ plan.start = position))
          /\ \A p \in Payers : plan.allocation[p] <= balances[p]
          /\ balances' = [p \in Payers |-> balances[p] - plan.allocation[p]]
          /\ position' = IF CommitCursor THEN plan.after ELSE position
          /\ completed' = completed \cup {plan.transaction}
          /\ receipts' = Append(receipts,
               [before |-> balances, start |-> position, allocation |-> plan.allocation,
                after |-> position', transaction |-> plan.transaction])
    /\ pending' = [pending EXCEPT ![w] = Empty]
    /\ fees' = fees + 1
    /\ UNCHANGED <<initialBalances, initialPosition>>

Abort(w) ==
    /\ pending[w].active
    /\ position' = IF AdvanceOnAbort THEN pending[w].after ELSE position
    /\ pending' = [pending EXCEPT ![w] = Empty]
    /\ UNCHANGED <<balances, completed, receipts, fees,
                    initialBalances, initialPosition>>

SettlementNext ==
    (\E w \in Workers, tx \in Transactions : Prepare(w, tx))
    \/ (\E w \in Workers : Commit(w) \/ Abort(w))
SettlementSpec == SettlementInit /\ [][SettlementNext]_settlementVars

MoneyConserved == Sum(balances) + fees = Sum(initialBalances)
OneFeePerSettlement == fees = Cardinality(completed) /\ fees = Len(receipts)
EveryReceiptUsesCurrentPlan ==
    \A index \in 1..Len(receipts) :
        LET receipt == receipts[index]
        IN /\ receipt.allocation = Allocation(receipt.before, 1, receipt.start)
           /\ receipt.after = NextCursor(receipt.before, 1, receipt.start)
CursorChangesOnlyWithSettlement ==
    position = IF receipts = <<>> THEN initialPosition ELSE receipts[Len(receipts)].after
NoDuplicateSettlement ==
    \A i, j \in 1..Len(receipts) :
        receipts[i].transaction = receipts[j].transaction => i = j
BalancesStayNatural == balances \in [Payers -> Nat]

=============================================================================
