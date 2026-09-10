---------------------- MODULE CleanupOutcome ----------------------
EXTENDS Naturals, TLC
CONSTANT EnforceFailures
VARIABLES faultAt, command, failed, phase, admitted
vars == <<faultAt, command, failed, phase, admitted>>
Init ==
    /\ faultAt \in 0..5
    /\ command = 1
    /\ failed = FALSE
    /\ phase = "commands"
    /\ admitted = FALSE
ObserveCommand ==
    /\ phase = "commands"
    /\ failed' = (failed \/ command = faultAt)
    /\ command' = command + 1
    /\ phase' = IF command = 5 THEN "admission" ELSE "commands"
    /\ UNCHANGED <<faultAt, admitted>>
Decide ==
    /\ phase = "admission"
    /\ admitted' = (~EnforceFailures \/ ~failed)
    /\ phase' = "checked"
    /\ UNCHANGED <<faultAt, command, failed>>
Next == ObserveCommand \/ Decide
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
TypeOK ==
    /\ faultAt \in 0..5
    /\ command \in 1..6
    /\ failed \in BOOLEAN
    /\ phase \in {"commands", "admission", "checked"}
    /\ admitted \in BOOLEAN
CleanupFailurePreventsAdmission == admitted => ~failed
Completes == <>(phase = "checked")
=============================================================================
