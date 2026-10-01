-------------------- MODULE RotatingMonetaryAllocation --------------------
EXTENDS MonetaryAllocation

VARIABLES capacity, obligation, cursor, draw, nextCursor, done
vars == <<capacity, obligation, cursor, draw, nextCursor, done>>
Init ==
    /\ capacity \in [Payers -> 0..CapacityBound]
    /\ obligation \in 0..Sum(capacity)
    /\ cursor \in Payers
    /\ draw = [p \in Payers |-> 0]
    /\ nextCursor = cursor
    /\ done = FALSE
Plan ==
    /\ ~done
    /\ draw' = Allocation(capacity, obligation, cursor)
    /\ nextCursor' = NextCursor(capacity, obligation, cursor)
    /\ done' = TRUE
    /\ UNCHANGED <<capacity, obligation, cursor>>
Spec == Init /\ [][Plan]_vars

ConservedObligation == done => Sum(draw) = obligation
NoPayerOverdraw == \A p \in Payers : draw[p] <= capacity[p]
IntegerMaxMinFairness ==
    done => \A p, q \in Payers : draw[p] < capacity[p] => draw[q] <= draw[p] + 1
CursorIsValid == nextCursor \in Payers
ZeroChargePreservesCursor == obligation = 0 => nextCursor = cursor
SingleFeeRotates ==
    done /\ obligation = 1 /\ (\A p \in Payers : capacity[p] > 0)
      => /\ draw[cursor] = 1
         /\ nextCursor = (cursor + 1) % PayerCount

=============================================================================
