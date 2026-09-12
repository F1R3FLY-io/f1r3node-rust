---------------------- MODULE DiskDiagnosticDeadline ----------------------
EXTENDS Naturals, TLC

CONSTANT AggregateDeadline
VARIABLES phase, elapsed, rootsLeft, freeMiB
vars == <<phase, elapsed, rootsLeft, freeMiB>>

Init ==
    /\ phase = "attribution"
    /\ elapsed = 0
    /\ rootsLeft \in {1, 3, 32}
    /\ freeMiB = 2

Tick ==
    /\ phase = "attribution"
    /\ elapsed < 2
    /\ elapsed' = elapsed + 1
    /\ freeMiB' \in 0..freeMiB
    /\ phase' = IF AggregateDeadline /\ elapsed' = 1 THEN "done" ELSE "attribution"
    /\ UNCHANGED rootsLeft

CompleteRoot ==
    /\ phase = "attribution"
    /\ rootsLeft > 0
    /\ rootsLeft' = rootsLeft - 1
    /\ phase' = IF rootsLeft' = 0 THEN "done" ELSE "attribution"
    /\ UNCHANGED <<elapsed, freeMiB>>

Next == Tick \/ CompleteRoot
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"attribution", "done"}
    /\ elapsed \in 0..2
    /\ rootsLeft \in 0..32
    /\ freeMiB \in 0..2

AttributionWithinBudget == phase = "attribution" => elapsed < 1
=============================================================================
