------------------------- MODULE AuthorityPresentation -------------------------
EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS
    OmitIntermediatePresentation,
    AllowWeakening,
    AllowNonAtomicDebit,
    ReplaySubstitutesPresentation,
    OmitStackReservationFromCertificate

ASSUME /\ OmitIntermediatePresentation \in BOOLEAN
       /\ AllowWeakening \in BOOLEAN
       /\ AllowNonAtomicDebit \in BOOLEAN
       /\ ReplaySubstitutesPresentation \in BOOLEAN
       /\ OmitStackReservationFromCertificate \in BOOLEAN

Atoms == {"a", "b", "c", "d", "e"}
Demand == {"a", "b", "c", "d"}
Stacks == {"ab", "cd", "a", "b", "c", "d", "abcde"}
Available == Stacks
InitialPosition == [stack \in Stacks |-> 1]

Cells ==
    [stack \in Stacks |->
      CASE stack = "ab" -> <<{"a", "b"}, {"a"}>>
        [] stack = "cd" -> <<{"c", "d"}, {"d"}>>
        [] stack = "a" -> <<{"a"}, {"a"}>>
        [] stack = "b" -> <<{"b"}, {"b"}>>
        [] stack = "c" -> <<{"c"}, {"c"}>>
        [] stack = "d" -> <<{"d"}, {"d"}>>
        [] OTHER -> <<Atoms, {"e"}>>]

Manifest ==
    IF OmitIntermediatePresentation
    THEN {"a", "b", "c", "d", "abcde"}
    ELSE Available

CanonicalDraw == IF AllowWeakening THEN {"abcde"} ELSE {"ab", "cd"}
ReplayDraw(certified) ==
    IF ReplaySubstitutesPresentation
    THEN {"a", "b", "c", "d"}
    ELSE certified

HeadAtoms(stack, position) == Cells[stack][position[stack]]

RECURSIVE UnionHeads(_, _, _)

UnionHeads(draw, position, remaining) ==
    IF remaining = {}
    THEN {}
    ELSE LET stack == CHOOSE candidate \in remaining : TRUE
         IN HeadAtoms(stack, position)
              \cup UnionHeads(draw, position, remaining \ {stack})

PresentedAtoms(draw, position) == UnionHeads(draw, position, draw)

RECURSIVE HeadCount(_, _, _)

HeadCount(draw, position, remaining) ==
    IF remaining = {}
    THEN 0
    ELSE LET stack == CHOOSE candidate \in remaining : TRUE
         IN Cardinality(HeadAtoms(stack, position))
              + HeadCount(draw, position, remaining \ {stack})

ExactCover(draw, position) ==
    /\ PresentedAtoms(draw, position) = Demand
    /\ HeadCount(draw, position, draw) = Cardinality(Demand)

FundingValid(draw, position) ==
    /\ draw \subseteq Available
    /\ draw \subseteq Manifest
    /\ IF AllowWeakening
       THEN Demand \subseteq PresentedAtoms(draw, position)
       ELSE ExactCover(draw, position)

Pop(position, draw) ==
    [stack \in Stacks |->
      position[stack] + IF stack \in draw THEN 1 ELSE 0]

VARIABLES
    phase,
    selected,
    certificateDraw,
    positions,
    popped,
    committed,
    rejected,
    consumedAtoms,
    replayPositions,
    replayed

vars == <<phase, selected, certificateDraw, positions, popped, committed, rejected,
          consumedAtoms, replayPositions, replayed>>

Init ==
    /\ phase = "Admission"
    /\ selected = {}
    /\ certificateDraw = {}
    /\ positions = InitialPosition
    /\ popped = {}
    /\ committed = FALSE
    /\ rejected = FALSE
    /\ consumedAtoms = {}
    /\ replayPositions = InitialPosition
    /\ replayed = FALSE

Admit ==
    /\ phase = "Admission"
    /\ FundingValid(CanonicalDraw, positions)
    /\ selected' = CanonicalDraw
    /\ certificateDraw' =
         IF OmitStackReservationFromCertificate THEN {} ELSE CanonicalDraw
    /\ phase' = "Execution"
    /\ UNCHANGED <<positions, popped, committed, rejected, consumedAtoms,
                    replayPositions, replayed>>

Reject ==
    /\ phase = "Admission"
    /\ ~FundingValid(CanonicalDraw, positions)
    /\ rejected' = TRUE
    /\ phase' = "Done"
    /\ UNCHANGED <<selected, certificateDraw, positions, popped, committed, consumedAtoms,
                    replayPositions, replayed>>

Commit ==
    /\ phase = "Execution"
    /\ IF AllowNonAtomicDebit /\ Cardinality(selected) > 1
       THEN LET stack == CHOOSE candidate \in selected : TRUE
            IN /\ positions' = Pop(positions, {stack})
               /\ popped' = {stack}
               /\ consumedAtoms' = HeadAtoms(stack, positions)
               /\ phase' = "Partial"
               /\ UNCHANGED committed
       ELSE /\ positions' = Pop(positions, selected)
            /\ popped' = selected
            /\ consumedAtoms' = PresentedAtoms(selected, positions)
            /\ committed' = TRUE
            /\ phase' = "Replay"
    /\ UNCHANGED <<selected, certificateDraw, rejected, replayPositions, replayed>>

FinishPartial ==
    /\ phase = "Partial"
    /\ positions' = Pop(positions, selected \ popped)
    /\ popped' = selected
    /\ consumedAtoms' = PresentedAtoms(selected, InitialPosition)
    /\ committed' = TRUE
    /\ phase' = "Replay"
    /\ UNCHANGED <<selected, certificateDraw, rejected, replayPositions, replayed>>

Replay ==
    /\ phase = "Replay"
    /\ FundingValid(ReplayDraw(certificateDraw), replayPositions)
    /\ replayPositions' = Pop(replayPositions, ReplayDraw(certificateDraw))
    /\ replayed' = TRUE
    /\ phase' = "Done"
    /\ UNCHANGED <<selected, certificateDraw, positions, popped, committed, rejected,
                    consumedAtoms>>

Next == Admit \/ Reject \/ Commit \/ FinishPartial \/ Replay
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"Admission", "Execution", "Partial", "Replay", "Done"}
    /\ selected \subseteq Stacks
    /\ certificateDraw \subseteq Stacks
    /\ positions \in [Stacks -> 1..3]
    /\ popped \subseteq Stacks
    /\ committed \in BOOLEAN
    /\ rejected \in BOOLEAN
    /\ consumedAtoms \subseteq Atoms
    /\ replayPositions \in [Stacks -> 1..3]
    /\ replayed \in BOOLEAN

IntermediatePartitionAdmitted ==
    phase = "Done" => ~rejected

NoWeakening ==
    committed => consumedAtoms = Demand

NoPartialEventDebit ==
    ~committed => positions = InitialPosition

TemporalStacksPopOnlyTheirHead ==
    \A stack \in Stacks : positions[stack] \in {1, 2}

ReplayUsesCertifiedPresentation ==
    replayed => ReplayDraw(certificateDraw) = certificateDraw

CertificateBindsPhysicalReservation ==
    committed => popped \subseteq certificateDraw

ReplayMatchesSettlement ==
    replayed => replayPositions = positions

IntermediatePartitionIsExact ==
    ~AllowWeakening => ExactCover({"ab", "cd"}, InitialPosition)

=============================================================================
