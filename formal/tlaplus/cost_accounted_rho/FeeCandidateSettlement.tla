-------------------- MODULE FeeCandidateSettlement --------------------
EXTENDS MonetaryAllocation

CONSTANTS Workers, Transactions, RepriceAfterUser, RequireUnchangedUserBalance
ASSUME /\ Workers # {} /\ IsFiniteSet(Workers)
       /\ Transactions # {} /\ IsFiniteSet(Transactions)
       /\ RepriceAfterUser \in BOOLEAN
       /\ RequireUnchangedUserBalance \in BOOLEAN

VARIABLES visible, cursor, external, pending, completed, receipts, fees, initialTotal
candidateVars == <<visible, cursor, external, pending, completed, receipts, fees, initialTotal>>
Empty == [active |-> FALSE, evaluated |-> FALSE,
          tx |-> CHOOSE tx \in Transactions : TRUE,
          before |-> [p \in Payers |-> 0], afterUser |-> [p \in Payers |-> 0],
          beforeExternal |-> 0, afterExternal |-> 0, beforeCursor |-> 0,
          allocation |-> [p \in Payers |-> 0], nextCursor |-> 0]

Init ==
    /\ visible \in [Payers -> 0..CapacityBound]
    /\ cursor \in Payers
    /\ external = 2
    /\ initialTotal = Sum(visible) + external
    /\ pending = [w \in Workers |-> Empty]
    /\ completed = {}
    /\ receipts = <<>>
    /\ fees = 0

Prepare(w, tx) ==
    /\ ~pending[w].active
    /\ tx \notin completed
    /\ Sum(visible) >= 1
    /\ pending' = [pending EXCEPT ![w] =
         [active |-> TRUE, evaluated |-> FALSE, tx |-> tx,
          before |-> visible, afterUser |-> visible,
          beforeExternal |-> external, afterExternal |-> external,
          beforeCursor |-> cursor, allocation |-> Allocation(visible, 1, cursor),
          nextCursor |-> NextCursor(visible, 1, cursor)]]
    /\ UNCHANGED <<visible, cursor, external, completed, receipts, fees, initialTotal>>

Evaluate(w, payer, topUp) ==
    /\ pending[w].active /\ ~pending[w].evaluated
    /\ topUp \in 0..1
    /\ topUp <= pending[w].beforeExternal
    /\ pending' = [pending EXCEPT
         ![w].evaluated = TRUE,
         ![w].afterUser[payer] = @ + topUp,
         ![w].afterExternal = @ - topUp]
    /\ UNCHANGED <<visible, cursor, external, completed, receipts, fees, initialTotal>>

Fresh(plan) ==
    /\ plan.active /\ plan.evaluated /\ plan.tx \notin completed
    /\ plan.before = visible /\ plan.beforeCursor = cursor
    /\ plan.beforeExternal = external
Feasible(plan) == \A p \in Payers : plan.allocation[p] <= plan.afterUser[p]

Commit(w) ==
    /\ Fresh(pending[w]) /\ Feasible(pending[w])
    /\ ~RequireUnchangedUserBalance \/ pending[w].afterUser = pending[w].before
    /\ LET plan == pending[w]
           debit == IF RepriceAfterUser
                    THEN Allocation(plan.afterUser, 1, plan.beforeCursor)
                    ELSE plan.allocation
           next == IF RepriceAfterUser
                   THEN NextCursor(plan.afterUser, 1, plan.beforeCursor)
                   ELSE plan.nextCursor
       IN /\ visible' = [p \in Payers |-> plan.afterUser[p] - debit[p]]
          /\ external' = plan.afterExternal
          /\ cursor' = next
          /\ completed' = completed \cup {plan.tx}
          /\ receipts' = Append(receipts,
               [before |-> plan.before, afterUser |-> plan.afterUser,
                beforeCursor |-> plan.beforeCursor, allocation |-> debit, nextCursor |-> next])
    /\ pending' = [pending EXCEPT ![w] = Empty]
    /\ fees' = fees + 1
    /\ UNCHANGED initialTotal

Abort(w) ==
    /\ pending[w].active
    /\ pending' = [pending EXCEPT ![w] = Empty]
    /\ UNCHANGED <<visible, cursor, external, completed, receipts, fees, initialTotal>>
Next == (\E w \in Workers, tx \in Transactions : Prepare(w, tx))
     \/ (\E w \in Workers, payer \in Payers, topUp \in 0..1 : Evaluate(w, payer, topUp))
     \/ (\E w \in Workers : Commit(w) \/ Abort(w))
Spec == Init /\ [][Next]_candidateVars

MoneyConserved == Sum(visible) + external + fees = initialTotal
OneFeePerCandidate == fees = Cardinality(completed) /\ fees = Len(receipts)
CertifiedPrestateDeterminesFee ==
    \A index \in 1..Len(receipts) :
      LET receipt == receipts[index]
      IN /\ receipt.allocation = Allocation(receipt.before, 1, receipt.beforeCursor)
         /\ receipt.nextCursor = NextCursor(receipt.before, 1, receipt.beforeCursor)
FundedTopUpCannotDisableCommit ==
    \A w \in Workers : Fresh(pending[w]) /\ Feasible(pending[w]) => ENABLED Commit(w)

=============================================================================
