-------------------- MODULE NumericMergeRngBoundary --------------------
EXTENDS Naturals
CONSTANT CheckedBoundary
VARIABLES datumCount, randomCount, stage, result
vars == <<datumCount, randomCount, stage, result>>

Init == /\ datumCount \in 0..3
        /\ randomCount \in 0..datumCount
        /\ (datumCount > 0 => randomCount > 0)
        /\ stage = "reconstruct"
        /\ result = "none"

Reconstruct == /\ stage = "reconstruct"
               /\ IF datumCount = 1
                  THEN /\ stage' = "complete" /\ result' = "original"
                  ELSE IF randomCount >= 2
                       THEN /\ stage' = "complete" /\ result' = "upstream-merge"
                       ELSE /\ stage' = IF CheckedBoundary THEN "rejected" ELSE "panic"
                            /\ result' = "none"
               /\ UNCHANGED <<datumCount, randomCount>>

Spec == Init /\ [][Reconstruct]_vars
NoBoundaryPanic == stage # "panic"
SingletonUnchanged == stage # "reconstruct" /\ datumCount = 1 => result = "original"
ValidMergeUnchanged == stage # "reconstruct" /\ datumCount > 1 /\ randomCount >= 2 => result = "upstream-merge"
InvalidInputPublishesNothing == datumCount # 1 /\ randomCount < 2 => result = "none"
=====================================================================
