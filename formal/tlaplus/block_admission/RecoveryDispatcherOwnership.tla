---------------------- MODULE RecoveryDispatcherOwnership ----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Jobs, CountCap, WorkerCap, OwnWorkers
VARIABLES supervisor, receiverOpen, phase, handles, aborts, leases
vars == <<supervisor, receiverOpen, phase, handles, aborts, leases>>
Queued == {job \in Jobs : phase[job] = "queued"}
Active == {job \in Jobs : phase[job] = "active"}

Init ==
    /\ supervisor = "running"
    /\ receiverOpen = TRUE
    /\ phase = [job \in Jobs |-> "fresh"]
    /\ handles = {}
    /\ aborts = {}
    /\ leases = {}

Enqueue(job) ==
    /\ receiverOpen /\ phase[job] = "fresh" /\ Cardinality(Queued) < CountCap
    /\ phase' = [phase EXCEPT ![job] = "queued"]
    /\ leases' = leases \cup {job}
    /\ UNCHANGED <<supervisor, receiverOpen, handles, aborts>>

Dispatch(job) ==
    /\ supervisor = "running" /\ job \in Queued /\ Cardinality(handles) < WorkerCap
    /\ phase' = [phase EXCEPT ![job] = "active"]
    /\ handles' = IF OwnWorkers THEN handles \cup {job} ELSE handles
    /\ UNCHANGED <<supervisor, receiverOpen, aborts, leases>>

Finish(job) ==
    /\ job \in Active
    /\ phase' = [phase EXCEPT ![job] = "retired"]
    /\ leases' = leases \ {job}
    /\ aborts' = aborts \ {job}
    /\ UNCHANGED <<supervisor, receiverOpen, handles>>

Join(job) ==
    /\ job \in handles /\ phase[job] = "retired"
    /\ handles' = handles \ {job}
    /\ UNCHANGED <<supervisor, receiverOpen, phase, aborts, leases>>

RequestStop ==
    /\ supervisor = "running"
    /\ supervisor' = "requested"
    /\ UNCHANGED <<receiverOpen, phase, handles, aborts, leases>>

CloseInput ==
    /\ supervisor = "requested"
    /\ supervisor' = "closing"
    /\ receiverOpen' = FALSE
    /\ aborts' = Active \cap handles
    /\ UNCHANGED <<phase, handles, leases>>

DiscardQueued(job) ==
    /\ supervisor = "closing" /\ job \in Queued
    /\ phase' = [phase EXCEPT ![job] = "retired"]
    /\ leases' = leases \ {job}
    /\ UNCHANGED <<supervisor, receiverOpen, handles, aborts>>

StopComplete ==
    /\ supervisor = "closing" /\ Queued = {} /\ handles = {}
    /\ supervisor' = "stopped"
    /\ UNCHANGED <<receiverOpen, phase, handles, aborts, leases>>

Next ==
    \/ \E job \in Jobs : Enqueue(job) \/ Dispatch(job) \/ Finish(job) \/ Join(job) \/ DiscardQueued(job)
    \/ RequestStop \/ CloseInput \/ StopComplete
TypeOK ==
    /\ supervisor \in {"running", "requested", "closing", "stopped"}
    /\ receiverOpen \in BOOLEAN
    /\ phase \in [Jobs -> {"fresh", "queued", "active", "retired"}]
    /\ handles \subseteq Jobs /\ aborts \subseteq Jobs /\ leases \subseteq Jobs
Inv_ExactOwnership == leases = Queued \cup Active
Inv_ChildrenOwned == Active \subseteq handles
Inv_LocalBounds == Cardinality(Queued) <= CountCap /\ Cardinality(handles) <= WorkerCap
Inv_StopReleasesOwnership == supervisor = "stopped" => leases = {} /\ handles = {} /\ ~receiverOpen
Live_StopRetiresChildren == (supervisor # "running") ~> (supervisor = "stopped")
Spec == Init /\ [][Next]_vars /\ WF_vars(CloseInput) /\ WF_vars(StopComplete)
        /\ (\A job \in Jobs : WF_vars(Finish(job)) /\ WF_vars(Join(job)) /\ WF_vars(DiscardQueued(job)))
=============================================================================
