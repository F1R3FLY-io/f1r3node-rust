----------------------- MODULE RecoveryInputClosure -----------------------
EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS Jobs, Borrowers, WorkerCap, QueueCap, Defect
VARIABLES running, external, borrows, queue, workers, retired, payloads,
          due, probeBody, noise
vars == <<running, external, borrows, queue, workers, retired, payloads,
          due, probeBody, noise>>
None == "none"
Queued == {queue[i] : i \in 1..Len(queue)}
Strong == external \/ borrows # {}
ClosedEmpty == ~Strong /\ queue = <<>>

Init ==
    /\ running = TRUE /\ external = TRUE /\ borrows = {}
    /\ queue = <<>> /\ workers = {} /\ retired = {} /\ payloads = {}
    /\ due = FALSE /\ probeBody = None /\ noise = FALSE

Publish(j) ==
    /\ running /\ Strong /\ Len(queue) < QueueCap
    /\ j \notin Queued \cup workers \cup retired
    /\ queue' = Append(queue, j) /\ payloads' = payloads \cup {j}
    /\ UNCHANGED <<running, external, borrows, workers, retired, due, probeBody, noise>>

Dispatch ==
    /\ running /\ Len(queue) > 0 /\ Cardinality(workers) < WorkerCap
    /\ workers' = workers \cup {Head(queue)} /\ queue' = Tail(queue)
    /\ UNCHANGED <<running, external, borrows, retired, payloads, due, probeBody, noise>>

WorkerReturns(j) ==
    /\ j \in workers
    /\ workers' = workers \ {j} /\ retired' = retired \cup {j}
    /\ payloads' = payloads \ {j}
    /\ UNCHANGED <<running, external, borrows, queue, due, probeBody, noise>>

Borrow(b) ==
    /\ running /\ Strong /\ b \notin borrows
    /\ borrows' = borrows \cup {b}
    /\ UNCHANGED <<running, external, queue, workers, retired, payloads, due, probeBody, noise>>

Release(b) ==
    /\ b \in borrows /\ borrows' = borrows \ {b}
    /\ UNCHANGED <<running, external, queue, workers, retired, payloads, due, probeBody, noise>>

CloseExternal ==
    /\ external /\ external' = FALSE
    /\ UNCHANGED <<running, borrows, queue, workers, retired, payloads, due, probeBody, noise>>

ClockTick ==
    /\ running /\ ~due /\ due' = TRUE
    /\ UNCHANGED <<running, external, borrows, queue, workers, retired, payloads, probeBody, noise>>

Probe ==
    /\ running /\ due
    /\ Defect # "ClosureProbeRequiresWorkerSlot" \/ Cardinality(workers) < WorkerCap
    /\ IF Defect = "ProbeDequeues" /\ Len(queue) > 0 /\ Cardinality(workers) = WorkerCap
       THEN /\ probeBody' = Head(queue) /\ queue' = Tail(queue)
       ELSE UNCHANGED <<probeBody, queue>>
    /\ running' = ~ClosedEmpty /\ due' = FALSE
    /\ UNCHANGED <<external, borrows, workers, retired, payloads, noise>>

UnrelatedEvent ==
    /\ running /\ noise' = ~noise
    /\ due' = IF Defect = "ResetDeadlineOnEvent" THEN FALSE ELSE due
    /\ UNCHANGED <<running, external, borrows, queue, workers, retired, payloads, probeBody>>

Next ==
    \/ \E j \in Jobs : Publish(j) \/ WorkerReturns(j)
    \/ \E b \in Borrowers : Borrow(b) \/ Release(b)
    \/ Dispatch \/ CloseExternal \/ ClockTick \/ Probe \/ UnrelatedEvent

TypeOK ==
    /\ running \in BOOLEAN /\ external \in BOOLEAN /\ borrows \subseteq Borrowers
    /\ queue \in Seq(Jobs) /\ Len(queue) <= QueueCap
    /\ workers \subseteq Jobs /\ retired \subseteq Jobs /\ payloads \subseteq Jobs
    /\ due \in BOOLEAN /\ noise \in BOOLEAN /\ probeBody \in Jobs \cup {None}

Inv_NoExtraBody == probeBody = None
Inv_ParallelBound == Cardinality(workers) <= WorkerCap
Inv_PayloadOwnership == payloads = Queued \cup workers
Inv_UniqueOwnership ==
    /\ Len(queue) = Cardinality(Queued)
    /\ Queued \cap workers = {} /\ Queued \cap retired = {} /\ workers \cap retired = {}
Inv_NoEarlyStop == ~running => ClosedEmpty
ClosureObserved == (running /\ ClosedEmpty) ~> ~running
Spec == Init /\ [][Next]_vars /\ WF_vars(ClockTick) /\ WF_vars(Probe)
Symmetry == Permutations(Jobs) \cup Permutations(Borrowers)
=============================================================================
