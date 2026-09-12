------------------------- MODULE NativeControlPath -------------------------
EXTENDS Naturals

CONSTANTS ValidateAncestors, AncestorTrusted
VARIABLES phase, workloadStarted, unrelatedRunning

vars == <<phase, workloadStarted, unrelatedRunning>>

Init ==
    /\ phase = "check"
    /\ workloadStarted = FALSE
    /\ unrelatedRunning = TRUE

Validate ==
    /\ phase = "check"
    /\ phase' = IF ValidateAncestors /\ ~AncestorTrusted THEN "refused" ELSE "ready"
    /\ UNCHANGED <<workloadStarted, unrelatedRunning>>

Launch ==
    /\ phase = "ready"
    /\ phase' = "executed"
    /\ workloadStarted' = TRUE
    /\ UNCHANGED unrelatedRunning

Next == Validate \/ Launch
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ ValidateAncestors \in BOOLEAN
    /\ AncestorTrusted \in BOOLEAN
    /\ phase \in {"check", "refused", "ready", "executed"}
    /\ workloadStarted \in BOOLEAN
    /\ unrelatedRunning \in BOOLEAN

UntrustedControlPreventsWorkload == ~AncestorTrusted => ~workloadStarted
UnrelatedWriterPreserved == unrelatedRunning
Completes == <>(phase \in {"refused", "executed"})
=============================================================================
