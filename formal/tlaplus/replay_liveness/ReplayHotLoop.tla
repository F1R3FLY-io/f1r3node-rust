--------------------------- MODULE ReplayHotLoop ---------------------------
EXTENDS Naturals

CONSTANTS EventCount, IndexedReplay, InlineSingleton, YieldInterval

VARIABLES processed, commClones, matcherRuns, liveTasks, schedulerYields

vars == <<processed, commClones, matcherRuns, liveTasks, schedulerYields>>

Init == /\ processed = 0
        /\ commClones = 0
        /\ matcherRuns = 0
        /\ liveTasks = 0
        /\ schedulerYields = 0

Process ==
    /\ processed < EventCount
    /\ processed' = processed + 1
    /\ IF IndexedReplay
          THEN /\ commClones' = commClones
               /\ matcherRuns' = matcherRuns
          ELSE /\ commClones' = commClones + EventCount
               /\ matcherRuns' = matcherRuns + EventCount
    /\ liveTasks' = IF InlineSingleton THEN liveTasks ELSE liveTasks + 1
    /\ schedulerYields' =
          IF InlineSingleton /\ ((processed + 1) % YieldInterval = 0)
             THEN schedulerYields + 1
             ELSE schedulerYields

Done == /\ processed = EventCount
        /\ UNCHANGED vars

Next == Process \/ Done
Spec == Init /\ [][Next]_vars /\ WF_vars(Process)

TypeOK == /\ processed \in 0..EventCount
          /\ commClones \in Nat
          /\ matcherRuns \in Nat
          /\ liveTasks \in Nat
          /\ schedulerYields \in Nat

Inv_LinearCloneWork == commClones <= processed
Inv_LinearMatcherWork == matcherRuns <= processed
Inv_NoRecursiveTaskChain == liveTasks = 0
Inv_BoundedYieldWork == /\ schedulerYields * YieldInterval <= processed
                        /\ processed < (schedulerYields + 1) * YieldInterval
Live_ReplayCompletes == <> (processed = EventCount)
=============================================================================
