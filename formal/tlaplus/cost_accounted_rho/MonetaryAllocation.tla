-------------------- MODULE MonetaryAllocation --------------------
EXTENDS Naturals, FiniteSets, Sequences, Apalache

CONSTANTS PayerCount, CapacityBound
ASSUME /\ PayerCount \in Nat \ {0}
       /\ CapacityBound \in Nat

Payers == 0..(PayerCount - 1)
Sum(values) ==
    LET Add(total, key) == total + values[key]
    IN ApaFoldSet(Add, 0, DOMAIN values)
Minimum(left, right) == IF left < right THEN left ELSE right
Base(caps, level) == [p \in Payers |-> Minimum(caps[p], level)]
WaterLevel(caps, amount) ==
    CHOOSE level \in 0..amount :
        /\ Sum(Base(caps, level)) <= amount
        /\ \A higher \in (level + 1)..amount :
               Sum(Base(caps, higher)) > amount
Distance(payer, start) == (payer + PayerCount - start) % PayerCount
Eligible(caps, level) == {p \in Payers : caps[p] > level}
ResidualPayers(caps, amount, start) ==
    LET level == WaterLevel(caps, amount)
        residual == amount - Sum(Base(caps, level))
        eligible == Eligible(caps, level)
    IN {p \in eligible :
         Cardinality({q \in eligible : Distance(q, start) < Distance(p, start)})
           < residual}
Allocation(caps, amount, start) ==
    LET base == Base(caps, WaterLevel(caps, amount))
        extras == ResidualPayers(caps, amount, start)
    IN [p \in Payers |-> base[p] + IF p \in extras THEN 1 ELSE 0]
NextCursor(caps, amount, start) ==
    LET extras == ResidualPayers(caps, amount, start)
    IN IF extras = {} THEN start
       ELSE LET last == CHOOSE p \in extras :
                       \A q \in extras : Distance(q, start) <= Distance(p, start)
            IN (last + 1) % PayerCount

=============================================================================
