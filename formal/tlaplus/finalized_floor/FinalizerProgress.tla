------------------------- MODULE FinalizerProgress -------------------------
EXTENDS Naturals, FiniteSets

CONSTANTS CandidateCount, CandidateCap, BudgetRestartAt, TimeoutCandidates, Finalizable

VARIABLES phase, cursor, examined, selected

vars == <<phase, cursor, examined, selected>>
Candidates == 1..CandidateCount
FirstFinalizable == CHOOSE c \in Finalizable : \A d \in Finalizable : c <= d

ASSUME /\ CandidateCount > 0
       /\ CandidateCap \in 0..CandidateCount
       /\ BudgetRestartAt \in 0..CandidateCount
       /\ TimeoutCandidates \subseteq Candidates
       /\ Finalizable \subseteq Candidates
       /\ Finalizable # {}

Init ==
    /\ phase = "Idle"
    /\ cursor = 1
    /\ examined = {}
    /\ selected = 0

Start ==
    /\ phase = "Idle"
    /\ phase' = "Scanning"
    /\ cursor' = 1
    /\ examined' = {}
    /\ selected' = 0

Evaluate ==
    /\ phase = "Scanning"
    /\ cursor \in Candidates
    /\ (CandidateCap = 0 \/ cursor <= CandidateCap)
    /\ (BudgetRestartAt = 0 \/ cursor # BudgetRestartAt)
    /\ cursor \notin TimeoutCandidates
    /\ examined' = examined \union {cursor}
    /\ IF cursor \in Finalizable
          THEN /\ phase' = "Done"
               /\ cursor' = cursor
               /\ selected' = cursor
          ELSE /\ phase' = "Scanning"
               /\ cursor' = cursor + 1
               /\ selected' = 0

Exhaust ==
    /\ phase = "Scanning"
    /\ cursor = CandidateCount + 1
    /\ CandidateCap = 0
    /\ phase' = "Done"
    /\ cursor' = cursor
    /\ examined' = examined
    /\ selected' = 0

RestartAtCap ==
    /\ phase = "Scanning"
    /\ CandidateCap # 0
    /\ cursor > CandidateCap
    /\ phase' = "Idle"
    /\ cursor' = 1
    /\ examined' = {}
    /\ selected' = 0

RestartAtBudget ==
    /\ phase = "Scanning"
    /\ BudgetRestartAt # 0
    /\ cursor = BudgetRestartAt
    /\ phase' = "Idle"
    /\ cursor' = 1
    /\ examined' = {}
    /\ selected' = 0

RestartAtTimeout ==
    /\ phase = "Scanning"
    /\ cursor \in TimeoutCandidates
    /\ phase' = "Idle"
    /\ cursor' = 1
    /\ examined' = {}
    /\ selected' = 0

Done == phase = "Done" /\ UNCHANGED vars

Next == Evaluate \/ Exhaust \/ RestartAtCap \/ RestartAtBudget \/ RestartAtTimeout \/ Start \/ Done

Spec == Init /\ [][Next]_vars
             /\ WF_vars(Start)
             /\ WF_vars(Evaluate)
             /\ WF_vars(Exhaust)

TypeOK ==
    /\ phase \in {"Idle", "Scanning", "Done"}
    /\ cursor \in 1..(CandidateCount + 1)
    /\ examined \subseteq Candidates
    /\ selected \in {0} \union Candidates

Inv_SelectedIsHighestFinalizable == selected = 0 \/ selected = FirstFinalizable
Inv_ExhaustionMeansCompleteCoverage == phase # "Done" \/ selected # 0 \/ examined = Candidates
Live_HighestFinalizableEventuallySelected == <> (selected = FirstFinalizable)
=============================================================================
