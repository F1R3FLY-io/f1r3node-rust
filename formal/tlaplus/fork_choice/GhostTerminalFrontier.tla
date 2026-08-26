----------------------- MODULE GhostTerminalFrontier -----------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANT UseGlobalTerminalHead

VARIABLES frontier, selectedHead, decreased

vars == <<frontier, selectedHead, decreased>>

Nodes == 0..6
Root == 0

Children(n) ==
    CASE n = 0 -> {1, 2, 6}
      [] n = 1 -> {3, 4}
      [] n = 2 -> {5}
      [] n = 6 -> {4}
      [] OTHER -> {}

Score == [n \in Nodes |->
    CASE n = 0 -> 110
      [] n = 1 -> 60
      [] n = 2 -> 40
      [] n = 3 -> 30
      [] n = 4 -> 30
      [] n = 5 -> 40
      [] n = 6 -> 30
      [] OTHER -> 0]

Outranks(a, b) ==
    \/ Score[a] > Score[b]
    \/ Score[a] = Score[b] /\ a < b

Best(candidates) ==
    CHOOSE winner \in candidates :
      \A candidate \in candidates :
        candidate = winner \/ Outranks(winner, candidate)

RECURSIVE Ghost(_)
Ghost(n) == IF Children(n) = {} THEN n ELSE Ghost(Best(Children(n)))

TerminalSet == {n \in Nodes : Children(n) = {}}
GlobalTerminalHead == Best(TerminalSet)
ExpectedGhostHead == Ghost(Root)

ChosenHead ==
    IF UseGlobalTerminalHead
    THEN GlobalTerminalHead
    ELSE ExpectedGhostHead

Expandable(current) == {n \in current : Children(n) # {}}

FrontierWork(current) ==
    IF Root \in current
    THEN 4
    ELSE Cardinality(current \cap {1, 2, 6})

Init ==
    /\ frontier = {Root}
    /\ selectedHead = ChosenHead
    /\ decreased = TRUE

Expand ==
    \E node \in Expandable(frontier) :
      LET nextFrontier == (frontier \ {node}) \cup Children(node) IN
      /\ frontier' = nextFrontier
      /\ UNCHANGED selectedHead
      /\ decreased' = (FrontierWork(nextFrontier) < FrontierWork(frontier))

Next == Expand
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ frontier \subseteq Nodes
    /\ selectedHead \in Nodes
    /\ decreased \in BOOLEAN

Inv_StrictExpansionProgress == decreased

Inv_HeadIsGreedyGhost == selectedHead = ExpectedGhostHead

Inv_ExactWhenTerminal ==
    Expandable(frontier) = {} => frontier = TerminalSet

Inv_GhostHeadRetained ==
    Expandable(frontier) = {} => selectedHead \in frontier

Inv_PinnedCounterexample ==
    /\ ExpectedGhostHead = 3
    /\ GlobalTerminalHead = 5
    /\ Score[1] = 60
    /\ Score[2] = 40

=============================================================================
