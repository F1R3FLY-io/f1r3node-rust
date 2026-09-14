-------------------------- MODULE CrashMonitorDeath --------------------------
EXTENDS Naturals

CONSTANT ObserveMonitorDeath
VARIABLES phase, monitorRunning, ownedRunning, unownedRunning, failures, admissions, restarts

vars == <<phase, monitorRunning, ownedRunning, unownedRunning, failures, admissions, restarts>>

Init ==
    /\ phase = "active"
    /\ monitorRunning = TRUE
    /\ ownedRunning = TRUE
    /\ unownedRunning = TRUE
    /\ failures = 0
    /\ admissions = 1
    /\ restarts = 0

MonitorDies ==
    /\ phase = "active"
    /\ phase' = "failed"
    /\ monitorRunning' = FALSE
    /\ UNCHANGED <<ownedRunning, unownedRunning, failures, admissions, restarts>>

Respond ==
    /\ phase = "failed"
    /\ phase' = "checked"
    /\ ownedRunning' = ~ObserveMonitorDeath
    /\ failures' = IF ObserveMonitorDeath THEN 1 ELSE 0
    /\ UNCHANGED <<monitorRunning, unownedRunning, admissions, restarts>>

Restart ==
    /\ phase = "checked"
    /\ restarts < 2
    /\ restarts' = restarts + 1
    /\ admissions' = IF failures = 1 THEN admissions ELSE admissions + 1
    /\ UNCHANGED <<phase, monitorRunning, ownedRunning, unownedRunning, failures>>

Next == MonitorDies \/ Respond \/ Restart
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"active", "failed", "checked"}
    /\ monitorRunning \in BOOLEAN
    /\ ownedRunning \in BOOLEAN
    /\ unownedRunning \in BOOLEAN
    /\ failures \in 0..1
    /\ admissions \in 1..3
    /\ restarts \in 0..2

MonitorDeathStopsOwnedWriter == phase = "checked" => ~ownedRunning
UnownedWriterPreserved == unownedRunning
MonitorFailureRetained == phase = "checked" => (failures = 1 /\ admissions = 1)
Completes == <>(restarts = 2)
=============================================================================
