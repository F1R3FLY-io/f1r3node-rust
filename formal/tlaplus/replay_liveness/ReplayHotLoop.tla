--------------------------- MODULE ReplayHotLoop ---------------------------
EXTENDS Naturals

CONSTANTS EventCount, IndexedReplay

VARIABLES processed, commClones, matcherRuns

vars == <<processed, commClones, matcherRuns>>

Init == /\ processed = 0
        /\ commClones = 0
        /\ matcherRuns = 0

Process ==
    /\ processed < EventCount
    /\ processed' = processed + 1
    /\ IF IndexedReplay
          THEN /\ commClones' = commClones
               /\ matcherRuns' = matcherRuns
          ELSE /\ commClones' = commClones + EventCount
               /\ matcherRuns' = matcherRuns + EventCount

Done == /\ processed = EventCount
        /\ UNCHANGED vars

Next == Process \/ Done
Spec == Init /\ [][Next]_vars /\ WF_vars(Process)

TypeOK == /\ processed \in 0..EventCount
          /\ commClones \in Nat
          /\ matcherRuns \in Nat

Inv_LinearCloneWork == commClones <= processed
Inv_LinearMatcherWork == matcherRuns <= processed
Live_ReplayCompletes == <> (processed = EventCount)
=============================================================================
