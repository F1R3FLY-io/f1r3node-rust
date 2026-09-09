---------------------- MODULE BenchmarkDiskMonitor ----------------------
EXTENDS Naturals, TLC

CONSTANTS MonitorOpening, InitialMiB, FaultMiB, HardFloorMiB
VARIABLES phase, guardianStarted, freeMiB, sampled, breachRecorded, stopRequested
vars == <<phase, guardianStarted, freeMiB, sampled, breachRecorded, stopRequested>>

Init ==
    /\ phase = "prepare"
    /\ guardianStarted = FALSE
    /\ freeMiB = InitialMiB
    /\ sampled = FALSE
    /\ breachRecorded = FALSE
    /\ stopRequested = FALSE

StartBenchmark ==
    /\ phase = "prepare"
    /\ guardianStarted' = MonitorOpening
    /\ phase' = "active"
    /\ UNCHANGED <<freeMiB, sampled, breachRecorded, stopRequested>>

DiskFalls ==
    /\ phase = "active"
    /\ freeMiB' = FaultMiB
    /\ phase' = "poll"
    /\ UNCHANGED <<guardianStarted, sampled, breachRecorded, stopRequested>>

Observe ==
    /\ phase = "poll"
    /\ sampled' = guardianStarted
    /\ breachRecorded' = (guardianStarted /\ freeMiB < HardFloorMiB)
    /\ stopRequested' = breachRecorded'
    /\ phase' = "observed"
    /\ UNCHANGED <<guardianStarted, freeMiB>>

ReturnBenchmark ==
    /\ phase = "observed"
    /\ phase' = "done"
    /\ guardianStarted' = TRUE
    /\ UNCHANGED <<freeMiB, sampled, breachRecorded, stopRequested>>

Next == StartBenchmark \/ DiskFalls \/ Observe \/ ReturnBenchmark
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"prepare", "active", "poll", "observed", "done"}
    /\ guardianStarted \in BOOLEAN
    /\ freeMiB \in {InitialMiB, FaultMiB}
    /\ sampled \in BOOLEAN
    /\ breachRecorded \in BOOLEAN
    /\ stopRequested \in BOOLEAN

BenchmarkBreachObserved ==
    (phase = "observed" /\ freeMiB < HardFloorMiB) =>
        (sampled /\ breachRecorded /\ stopRequested)

StopRequiresRecord == stopRequested => breachRecorded
Completes == <>(phase = "done")
=============================================================================
