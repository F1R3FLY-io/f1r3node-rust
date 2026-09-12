------------------------ MODULE DiskStopDeadline ------------------------
EXTENDS Naturals, TLC

CONSTANT EnforceDeadline
VARIABLES phase, elapsed, termSent, killSent
vars == <<phase, elapsed, termSent, killSent>>

Init ==
    /\ phase = "stopping"
    /\ elapsed = 0
    /\ termSent = FALSE
    /\ killSent = FALSE

Tick ==
    /\ phase = "stopping"
    /\ elapsed < 3
    /\ elapsed' = elapsed + 1
    /\ termSent' = (EnforceDeadline /\ elapsed' >= 1)
    /\ killSent' = (EnforceDeadline /\ elapsed' = 2)
    /\ phase' = IF killSent' THEN "returned" ELSE "stopping"

CommandsReturn ==
    /\ phase = "stopping"
    /\ phase' = "returned"
    /\ UNCHANGED <<elapsed, termSent, killSent>>

Next == Tick \/ CommandsReturn
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"stopping", "returned"}
    /\ elapsed \in 0..3
    /\ termSent \in BOOLEAN
    /\ killSent \in BOOLEAN

StopWithinBudget == phase = "stopping" => elapsed < 2
KillFollowsTerm == killSent => termSent
=============================================================================
