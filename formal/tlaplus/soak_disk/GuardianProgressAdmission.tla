---------------------- MODULE GuardianProgressAdmission ----------------------
EXTENDS Naturals, TLC
CONSTANTS CheckProgress, SilenceTicks
VARIABLES age, phase, admitted, failureRecorded
vars == <<age, phase, admitted, failureRecorded>>

Init ==
    /\ age \in {0, SilenceTicks, SilenceTicks + 1}
    /\ phase = "boundary"
    /\ admitted = FALSE
    /\ failureRecorded = FALSE

Decide ==
    /\ phase = "boundary"
    /\ admitted' = (~CheckProgress \/ age <= SilenceTicks)
    /\ failureRecorded' = ~admitted'
    /\ phase' = "checked"
    /\ UNCHANGED age

Spec == Init /\ [][Decide]_vars /\ WF_vars(Decide)
TypeOK ==
    /\ age \in {0, SilenceTicks, SilenceTicks + 1}
    /\ phase \in {"boundary", "checked"}
    /\ admitted \in BOOLEAN
    /\ failureRecorded \in BOOLEAN
StaleProgressPreventsAdmission == admitted => age <= SilenceTicks
RefusalRecorded == (phase = "checked" /\ ~admitted) => failureRecorded
Completes == <>(phase = "checked")
=============================================================================
