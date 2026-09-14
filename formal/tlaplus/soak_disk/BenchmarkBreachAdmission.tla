-------------------- MODULE BenchmarkBreachAdmission --------------------
EXTENDS Naturals, TLC

CONSTANT CheckRetainedBreach
VARIABLES phase, marker, admitted, failures
vars == <<phase, marker, admitted, failures>>

Init ==
    /\ phase = "benchmark"
    /\ marker \in BOOLEAN
    /\ admitted = FALSE
    /\ failures = 0

Benchmark ==
    /\ phase = "benchmark"
    /\ admitted' = (~CheckRetainedBreach \/ ~marker)
    /\ phase' = "recover"
    /\ UNCHANGED <<marker, failures>>

Recover ==
    /\ phase = "recover"
    /\ failures' = IF marker THEN 1 ELSE 0
    /\ phase' = "done"
    /\ UNCHANGED <<marker, admitted>>

Next == Benchmark \/ Recover
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"benchmark", "recover", "done"}
    /\ marker \in BOOLEAN
    /\ admitted \in BOOLEAN
    /\ failures \in 0..1

RetainedBreachPreventsBenchmark == marker => ~admitted
RecoveryRecordsFailure == (phase = "done" /\ marker) => failures = 1
Completes == <>(phase = "done")
=============================================================================
