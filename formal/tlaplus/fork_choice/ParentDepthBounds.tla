---- MODULE ParentDepthBounds ----
EXTENDS Integers, Naturals, Sequences

CONSTANTS Ranked, Height, MaxCandidateHeight, Depth, Buffer, CountCap, DropHead

RankedModel == <<1, 2, 3, 4>>
HeightModel == [i \in 1..4 |-> CASE i = 1 -> 10
                                  [] i = 2 -> 100
                                  [] i = 3 -> 95
                                  [] OTHER -> 20]
NegativeOne == -1

Blocks == {Ranked[i] : i \in 1..Len(Ranked)}

ASSUME /\ Len(Ranked) > 0
       /\ MaxCandidateHeight \in Int
       /\ Height \in [Blocks -> Int]
       /\ \A i \in 1..Len(Ranked) : Height[Ranked[i]] <= MaxCandidateHeight
       /\ \E i \in 1..Len(Ranked) : Height[Ranked[i]] = MaxCandidateHeight
       /\ DropHead \in BOOLEAN

WithinDepth(i, allowance) ==
  MaxCandidateHeight - Height[Ranked[i]] <= allowance

RetainedIndices ==
  IF DropHead
  THEN {i \in 1..Len(Ranked) : WithinDepth(i, Depth)}
  ELSE {1} \cup {i \in 2..Len(Ranked) : WithinDepth(i, Depth)}

VARIABLE turn

Init == turn = 0
Next == turn' = 1 - turn
Spec == Init /\ [][Next]_<<turn>>

Inv_HeadPreserved == 1 \in RetainedIndices

Inv_EveryRetainedTailWithinDepth ==
  \A i \in RetainedIndices \ {1} : WithinDepth(i, Depth)

Inv_FreshestCandidateRetained ==
  \E i \in RetainedIndices : Height[Ranked[i]] = MaxCandidateHeight

Inv_ReceiverAcceptsHonestParents ==
  \A i \in RetainedIndices \ {1} : WithinDepth(i, Depth + Buffer)

Inv_ConfigAdmissible ==
  /\ Depth \in Nat
  /\ Buffer \in Nat
  /\ CountCap \in Int
  /\ CountCap = -1 \/ CountCap >= 1

====
