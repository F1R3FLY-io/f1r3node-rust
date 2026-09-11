--------------------- MODULE BenchmarkCrashRecovery ---------------------
EXTENDS Naturals
CONSTANT RememberBenchmark
VARIABLES phase, pending, refused, failures, benchFailures, benchSegments, admitted
vars == <<phase, pending, refused, failures, benchFailures, benchSegments, admitted>>
Init == /\ phase = "active"
        /\ pending = RememberBenchmark
        /\ refused = FALSE
        /\ failures = 0
        /\ benchFailures = 0
        /\ benchSegments = IF RememberBenchmark THEN 1 ELSE 0
        /\ admitted = FALSE
Crash == /\ phase = "active"
         /\ phase' = "crashed"
         /\ UNCHANGED <<pending, refused, failures, benchFailures, benchSegments, admitted>>
Recover == /\ phase \in {"crashed", "recovered"}
           /\ phase' = IF phase = "crashed" THEN "recovered" ELSE "checked"
           /\ failures' = IF pending THEN failures + 1 ELSE failures
           /\ benchFailures' = IF pending THEN benchFailures + 1 ELSE benchFailures
           /\ refused' = (refused \/ pending)
           /\ pending' = FALSE
           /\ admitted' = ~(refused \/ pending)
           /\ UNCHANGED benchSegments
Next == Crash \/ Recover
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
TypeOK == /\ phase \in {"active", "crashed", "recovered", "checked"}
          /\ pending \in BOOLEAN
          /\ refused \in BOOLEAN
          /\ failures \in 0..1
          /\ benchFailures \in 0..1
          /\ benchSegments \in 0..1
          /\ admitted \in BOOLEAN
BenchmarkCrashRequiresRefusal ==
    phase \in {"recovered", "checked"} =>
        refused /\ ~admitted /\ failures = 1 /\ benchFailures = 1 /\ benchSegments = 1
Completes == <>(phase = "checked")
=============================================================================
