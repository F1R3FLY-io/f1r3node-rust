------------------------ MODULE BenchmarkMonitorDeath ------------------------
EXTENDS Naturals

CONSTANT ObserveMonitorDeath
VARIABLES phase, monitorRunning, ownedRunning, unownedRunning, failures,
          benchmarkFailures, benchmarks, iterations, restarts

vars == <<phase, monitorRunning, ownedRunning, unownedRunning, failures,
          benchmarkFailures, benchmarks, iterations, restarts>>

Init ==
    /\ phase = "benchmark"
    /\ monitorRunning = TRUE
    /\ ownedRunning = TRUE
    /\ unownedRunning = TRUE
    /\ failures = 0
    /\ benchmarkFailures = 0
    /\ benchmarks = 1
    /\ iterations = 0
    /\ restarts = 0

MonitorDies ==
    /\ phase = "benchmark"
    /\ phase' = "failed"
    /\ monitorRunning' = FALSE
    /\ UNCHANGED <<ownedRunning, unownedRunning, failures, benchmarkFailures,
                    benchmarks, iterations, restarts>>

Respond ==
    /\ phase = "failed"
    /\ phase' = "checked"
    /\ ownedRunning' = ~ObserveMonitorDeath
    /\ failures' = IF ObserveMonitorDeath THEN 1 ELSE 0
    /\ benchmarkFailures' = IF ObserveMonitorDeath THEN 1 ELSE 0
    /\ UNCHANGED <<monitorRunning, unownedRunning, benchmarks, iterations, restarts>>

Restart ==
    /\ phase = "checked"
    /\ restarts < 2
    /\ restarts' = restarts + 1
    /\ benchmarks' = IF failures = 1 THEN benchmarks ELSE benchmarks + 1
    /\ iterations' = IF failures = 1 THEN iterations ELSE iterations + 1
    /\ UNCHANGED <<phase, monitorRunning, ownedRunning, unownedRunning,
                    failures, benchmarkFailures>>

Next == MonitorDies \/ Respond \/ Restart
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"benchmark", "failed", "checked"}
    /\ monitorRunning \in BOOLEAN
    /\ ownedRunning \in BOOLEAN
    /\ unownedRunning \in BOOLEAN
    /\ failures \in 0..1
    /\ benchmarkFailures \in 0..1
    /\ benchmarks \in 1..3
    /\ iterations \in 0..2
    /\ restarts \in 0..2

BenchmarkMonitorDeathStopsOwnedWriter == phase = "checked" => ~ownedRunning
UnownedWriterPreserved == unownedRunning
BenchmarkMonitorFailureRetained ==
    phase = "checked" =>
        (failures = 1 /\ benchmarkFailures = 1 /\ benchmarks = 1 /\ iterations = 0)
Completes == <>(restarts = 2)
=============================================================================
