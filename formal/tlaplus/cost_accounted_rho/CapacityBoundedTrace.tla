------------------------- MODULE CapacityBoundedTrace -------------------------
EXTENDS FiniteSets, Integers, Sequences, TLC

CONSTANTS Events, Rank, Capacity, FixedLimit, UseFixedLimit

ASSUME /\ Events # {}
       /\ Capacity \in Nat \ {0}
       /\ FixedLimit \in Nat \ {0}
       /\ UseFixedLimit \in BOOLEAN
       /\ Rank \in [Events -> Nat]
       /\ \A left, right \in Events : Rank[left] = Rank[right] => left = right

VARIABLES active, pending, retained

vars == <<active, pending, retained>>

Limit == IF UseFixedLimit THEN FixedLimit ELSE Capacity + 1

RECURSIVE RankSort(_)
RankSort(S) ==
    IF S = {} THEN <<>>
    ELSE LET least == CHOOSE event \in S :
                         \A other \in S : Rank[event] <= Rank[other]
         IN <<least>> \o RankSort(S \ {least})

Prefix(sequence, count) ==
    IF Len(sequence) <= count THEN sequence
    ELSE SubSeq(sequence, 1, count)

SequenceSet(sequence) == {sequence[index] : index \in DOMAIN sequence}

CanonicalWindow(S) == SequenceSet(Prefix(RankSort(S), Limit))

Init ==
    /\ active \in SUBSET Events
    /\ pending = active
    /\ retained = {}

Observe(event) ==
    /\ event \in pending
    /\ pending' = pending \ {event}
    /\ retained' = CanonicalWindow(retained \cup {event})
    /\ UNCHANGED active

Next == \E event \in Events : Observe(event)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ active \subseteq Events
    /\ pending \subseteq active
    /\ retained \subseteq active \ pending

WindowIsBounded == Cardinality(retained) <= Capacity + 1

WindowIsCanonical == retained = CanonicalWindow(active \ pending)

AcceptedTraceIsExact ==
    pending = {} /\ Cardinality(active) <= Capacity => retained = active

OutOfFuelWitnessIsPreserved ==
    pending = {} /\ Cardinality(active) > Capacity =>
        Cardinality(retained) = Capacity + 1

=============================================================================
