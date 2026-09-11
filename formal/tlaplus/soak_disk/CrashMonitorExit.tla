--------------------------- MODULE CrashMonitorExit ---------------------------
EXTENDS Naturals

CONSTANT RememberHandledExit
VARIABLES phase, handled, stopRequests

vars == <<phase, handled, stopRequests>>

Init ==
    /\ phase = "active"
    /\ handled = FALSE
    /\ stopRequests = 0

HandleExit ==
    /\ phase = "active"
    /\ phase' = "exited"
    /\ handled' = RememberHandledExit
    /\ stopRequests' = 1

ObserveExit ==
    /\ phase = "exited"
    /\ phase' = "checked"
    /\ stopRequests' = IF handled THEN stopRequests ELSE stopRequests + 1
    /\ UNCHANGED handled

Next == HandleExit \/ ObserveExit
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
TypeOK ==
    /\ phase \in {"active", "exited", "checked"}
    /\ handled \in BOOLEAN
    /\ stopRequests \in 0..2
HandledExitHasNoExtraStop == phase = "checked" => stopRequests = 1
Completes == <>(phase = "checked")
=============================================================================
