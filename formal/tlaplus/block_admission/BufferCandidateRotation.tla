------------------------- MODULE BufferCandidateRotation -------------------------
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS Identities, PageSize, AppendNew, KeepDuplicatePosition, RotateOnExamination

VARIABLES queue, members, allowance, selected

vars == <<queue, members, allowance, selected>>
Elements(sequence) == {sequence[index] : index \in DOMAIN sequence}
Without(sequence, key) == SelectSeq(sequence, LAMBDA element : element # key)
Position(sequence, key) == CHOOSE index \in DOMAIN sequence : sequence[index] = key
Rank(key) == Position(queue, key) - 1

Init ==
    /\ queue = <<>>
    /\ members = {}
    /\ allowance = [key \in Identities |-> 0]
    /\ selected = "none"

Insert(key) ==
    /\ queue' = IF key \in members
                 THEN IF KeepDuplicatePosition THEN queue ELSE Append(Without(queue, key), key)
                 ELSE IF AppendNew THEN Append(queue, key) ELSE <<key>> \o queue
    /\ members' = members \cup {key}
    /\ allowance' = IF key \in members THEN allowance
                     ELSE [allowance EXCEPT ![key] = Len(queue)]
    /\ selected' = "none"

Remove(key) ==
    /\ queue' = Without(queue, key)
    /\ members' = members \ {key}
    /\ allowance' = [allowance EXCEPT ![key] = 0]
    /\ selected' = "none"

Examine ==
    /\ queue # <<>>
    /\ LET count == IF RotateOnExamination THEN 1
                    ELSE IF Len(queue) < PageSize THEN Len(queue) ELSE PageSize
       IN queue' = SubSeq(queue, count + 1, Len(queue)) \o SubSeq(queue, 1, count)
    /\ selected' = Head(queue)
    /\ allowance' = [key \in Identities |->
          IF key = Head(queue) THEN Len(queue) - 1
          ELSE IF key \in members /\ allowance[key] > 0 THEN allowance[key] - 1
          ELSE allowance[key]]
    /\ UNCHANGED members

Next == (\E key \in Identities : Insert(key) \/ Remove(key)) \/ Examine

TypeOK ==
    /\ queue \in Seq(Identities)
    /\ Len(queue) <= Cardinality(Identities)
    /\ members \subseteq Identities
    /\ allowance \in [Identities -> 0..Cardinality(Identities)]
    /\ selected \in Identities \cup {"none"}

Inv_ExactMembership == Elements(queue) = members
Inv_NoDuplicates == Cardinality(Elements(queue)) = Len(queue)
Inv_NoOvertaking == \A key \in members : Rank(key) <= allowance[key]
Safety == TypeOK /\ Inv_ExactMembership /\ Inv_NoDuplicates /\ Inv_NoOvertaking
Live_PresentIdentityGetsExamined ==
    \A key \in Identities : (key \in members) ~> (selected = key \/ key \notin members)
Spec == Init /\ [][Next]_vars /\ WF_vars(Examine)
=============================================================================
