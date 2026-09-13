--------------------------- MODULE BoundedPublication ---------------------------
CONSTANT IndependentStop
VARIABLES visible, sync, stopped, confirmed, wedged

vars == <<visible, sync, stopped, confirmed, wedged>>

Init ==
    /\ visible = FALSE
    /\ sync = "pending"
    /\ stopped = FALSE
    /\ confirmed = FALSE
    /\ wedged = FALSE

MakeVisible ==
    /\ ~visible
    /\ visible' = TRUE
    /\ UNCHANGED <<sync, stopped, confirmed, wedged>>

SyncDone ==
    /\ visible
    /\ sync = "pending"
    /\ sync' = "done"
    /\ UNCHANGED <<visible, stopped, confirmed, wedged>>

SyncStall ==
    /\ visible
    /\ sync = "pending"
    /\ sync' = "stalled"
    /\ wedged' = (~IndependentStop /\ ~stopped)
    /\ UNCHANGED <<visible, stopped, confirmed>>

Stop ==
    /\ visible
    /\ ~stopped
    /\ (IndependentStop \/ sync = "done")
    /\ stopped' = TRUE
    /\ UNCHANGED <<visible, sync, confirmed, wedged>>

Confirm ==
    /\ sync = "done"
    /\ ~confirmed
    /\ confirmed' = TRUE
    /\ UNCHANGED <<visible, sync, stopped, wedged>>

Next == MakeVisible \/ SyncDone \/ SyncStall \/ Stop \/ Confirm
Spec == Init /\ [][Next]_vars /\ WF_vars(Stop) /\ WF_vars(MakeVisible)

TypeOK ==
    /\ visible \in BOOLEAN
    /\ sync \in {"pending", "done", "stalled"}
    /\ stopped \in BOOLEAN
    /\ confirmed \in BOOLEAN
    /\ wedged \in BOOLEAN

ShutdownIndependentOfSync == ~wedged
StalledNeverConfirmed == (sync = "stalled") => ~confirmed
StopEventuallyCompletes == <>stopped
=============================================================================
