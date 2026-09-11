--------------------------- MODULE MonitorAdmission ---------------------------
EXTENDS Naturals

CONSTANTS Work, ObserveMonitorDeath
VARIABLES phase, monitorRunning, benchmarks, iterations, failures, restarts

vars == <<phase, monitorRunning, benchmarks, iterations, failures, restarts>>

Init ==
    /\ phase = "probe"
    /\ monitorRunning = TRUE
    /\ benchmarks = 0
    /\ iterations = 0
    /\ failures = 0
    /\ restarts = 0

MonitorDiesDuringProbe ==
    /\ phase = "probe"
    /\ phase' = "checked"
    /\ monitorRunning' = FALSE
    /\ UNCHANGED <<benchmarks, iterations, failures, restarts>>

Admission ==
    /\ phase = "checked"
    /\ phase' = IF ObserveMonitorDeath THEN "refused" ELSE "admitted"
    /\ benchmarks' = IF ~ObserveMonitorDeath /\ Work = "benchmark" THEN 1 ELSE 0
    /\ iterations' = IF ~ObserveMonitorDeath /\ Work = "iteration" THEN 1 ELSE 0
    /\ failures' = IF ObserveMonitorDeath THEN 1 ELSE 0
    /\ UNCHANGED <<monitorRunning, restarts>>

Restart ==
    /\ phase = "refused"
    /\ restarts < 2
    /\ restarts' = restarts + 1
    /\ UNCHANGED <<phase, monitorRunning, benchmarks, iterations, failures>>

Next == MonitorDiesDuringProbe \/ Admission \/ Restart
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ Work \in {"benchmark", "iteration"}
    /\ phase \in {"probe", "checked", "refused", "admitted"}
    /\ monitorRunning \in BOOLEAN
    /\ benchmarks \in 0..1
    /\ iterations \in 0..1
    /\ failures \in 0..1
    /\ restarts \in 0..2

MonitorDeathPreventsAdmission == ~monitorRunning => (benchmarks = 0 /\ iterations = 0)
RefusalRetainsFailure == phase = "refused" => failures = 1
Completes == <>(restarts = 2)
=============================================================================
