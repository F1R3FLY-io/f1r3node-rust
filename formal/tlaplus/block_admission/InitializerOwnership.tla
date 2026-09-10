------------------------- MODULE InitializerOwnership -------------------------
EXTENDS FiniteSets, TLC

CONSTANTS Jobs, Initializer, TakeBeforeAwait, AbortOnDrop, WaitForRetirement
VARIABLES parent, owned, task, aborts, waiting, observed
vars == <<parent, owned, task, aborts, waiting, observed>>
Live == {job \in Jobs : task[job] = "live"}
Complete == {job \in Jobs : task[job] \in {"ok", "error", "panic", "cancelled"}}
Temporary == IF waiting /\ TakeBeforeAwait THEN {Initializer} ELSE {}

Init ==
    /\ parent = "running"
    /\ owned = Jobs
    /\ task = [job \in Jobs |-> "live"]
    /\ aborts = {}
    /\ waiting = FALSE
    /\ observed = [job \in Jobs |-> "none"]

PollInitializer ==
    /\ parent = "running" /\ ~waiting /\ Initializer \in owned
    /\ waiting' = TRUE
    /\ owned' = IF TakeBeforeAwait THEN owned \ {Initializer} ELSE owned
    /\ UNCHANGED <<parent, task, aborts, observed>>

CompetingSelectBranch ==
    /\ parent = "running" /\ waiting
    /\ waiting' = FALSE
    /\ UNCHANGED <<parent, owned, task, aborts, observed>>

Finish(job, result) ==
    /\ job \in Live /\ result \in {"ok", "error", "panic"}
    /\ task' = [task EXCEPT ![job] = result]
    /\ UNCHANGED <<parent, owned, aborts, waiting, observed>>

Observe(job) ==
    /\ parent \in {"running", "draining"}
    /\ job \in Complete /\ job \in owned \cup Temporary
    /\ observed' = [observed EXCEPT ![job] = task[job]]
    /\ owned' = owned \ {job}
    /\ waiting' = IF job = Initializer THEN FALSE ELSE waiting
    /\ UNCHANGED <<parent, task, aborts>>

Stop ==
    /\ parent = "running"
    /\ parent' = "draining"
    /\ aborts' = aborts \cup owned
    /\ waiting' = FALSE
    /\ UNCHANGED <<owned, task, observed>>

ChildCancellation(job) ==
    /\ job \in Live /\ job \in aborts
    /\ task' = [task EXCEPT ![job] = "cancelled"]
    /\ UNCHANGED <<parent, owned, aborts, waiting, observed>>

ShutdownComplete ==
    /\ parent = "draining"
    /\ (~WaitForRetirement \/ owned = {})
    /\ parent' = "complete"
    /\ UNCHANGED <<owned, task, aborts, waiting, observed>>

DropSupervisor ==
    /\ parent \in {"running", "draining"}
    /\ parent' = "gone"
    /\ aborts' = IF AbortOnDrop THEN aborts \cup owned ELSE aborts
    /\ owned' = {}
    /\ waiting' = FALSE
    /\ UNCHANGED <<task, observed>>

Next == PollInitializer \/ CompetingSelectBranch \/ Stop \/ ShutdownComplete \/ DropSupervisor
        \/ (\E job \in Jobs : Observe(job) \/ ChildCancellation(job)
            \/ (\E result \in {"ok", "error", "panic"} : Finish(job, result)))

TypeOK ==
    /\ Initializer \in Jobs
    /\ parent \in {"running", "draining", "complete", "gone"}
    /\ owned \subseteq Jobs /\ aborts \subseteq Jobs /\ waiting \in BOOLEAN
    /\ task \in [Jobs -> {"live", "ok", "error", "panic", "cancelled"}]
    /\ observed \in [Jobs -> {"none", "ok", "error", "panic", "cancelled"}]
Inv_NoDetachedChild == Live \subseteq owned \cup Temporary \cup aborts
Inv_PendingResultOwned == parent = "running" =>
    {job \in Jobs : observed[job] = "none"} \subseteq owned \cup Temporary
Inv_ExactObservation == \A job \in Jobs : observed[job] # "none" => observed[job] = task[job]
Inv_CleanupAfterRetirement == parent = "complete" => owned = {} /\ Live = {}
Live_StopRetiresChildren == parent # "running" ~> Live = {}

Spec == Init /\ [][Next]_vars
        /\ (\A job \in Jobs : WF_vars(ChildCancellation(job)))
=============================================================================
