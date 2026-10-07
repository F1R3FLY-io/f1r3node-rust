-------------------- MODULE CheckpointLocalHandoff --------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Workers, PublishBeforeRead, DrainBeforeRead

Phases == {"Ready", "Written", "Prepared", "Failed", "Done"}
Snapshot(root, log, count) == [root |-> root, store |-> root, log |-> log, count |-> count]
Before == Snapshot(0, 2, 2)
After(worker) == Snapshot(worker, 0, 0)

VARIABLES phase, local, durable, failures, returned
vars == <<phase, local, durable, failures, returned>>

Init ==
  /\ phase = [w \in Workers |-> "Ready"]
  /\ local = [w \in Workers |-> Before]
  /\ durable = {0}
  /\ failures = [w \in Workers |-> 0]
  /\ returned = [w \in Workers |-> 0]

Persist(w) ==
  /\ phase[w] = "Ready"
  /\ phase' = [phase EXCEPT ![w] = "Written"]
  /\ durable' = durable \cup {w}
  /\ local' = [local EXCEPT
       ![w].root = IF PublishBeforeRead THEN w ELSE @,
       ![w].log = IF DrainBeforeRead THEN 0 ELSE @,
       ![w].count = IF DrainBeforeRead THEN 0 ELSE @]
  /\ UNCHANGED <<failures, returned>>

Prepare(w) ==
  /\ phase[w] = "Written"
  /\ w \in durable
  /\ phase' = [phase EXCEPT ![w] = "Prepared"]
  /\ UNCHANGED <<local, durable, failures, returned>>

ReadFail(w) ==
  /\ phase[w] = "Written"
  /\ failures[w] = 0
  /\ phase' = [phase EXCEPT ![w] = "Failed"]
  /\ failures' = [failures EXCEPT ![w] = 1]
  /\ UNCHANGED <<local, durable, returned>>

Retry(w) ==
  /\ phase[w] = "Failed"
  /\ phase' = [phase EXCEPT ![w] = "Ready"]
  /\ UNCHANGED <<local, durable, failures, returned>>

Publish(w) ==
  /\ phase[w] = "Prepared"
  /\ phase' = [phase EXCEPT ![w] = "Done"]
  /\ returned' = [returned EXCEPT ![w] = local[w].log]
  /\ local' = [local EXCEPT ![w] = After(w)]
  /\ UNCHANGED <<durable, failures>>

Next == \E w \in Workers : Persist(w) \/ Prepare(w) \/ ReadFail(w) \/ Retry(w) \/ Publish(w)
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in [Workers -> Phases]
  /\ local \in [Workers -> [root : Workers \cup {0}, store : Workers \cup {0}, log : 0..2, count : 0..2]]
  /\ durable \subseteq Workers \cup {0}
  /\ failures \in [Workers -> 0..1]
  /\ returned \in [Workers -> 0..2]
FailedSnapshotPreserved == \A w \in Workers : phase[w] = "Failed" => local[w] = Before
UnpublishedSnapshotPreserved == \A w \in Workers : phase[w] # "Done" => local[w] = Before
PublishedSnapshotExact == \A w \in Workers : phase[w] = "Done" => local[w] = After(w)
RetryTracePreserved == \A w \in Workers : phase[w] = "Done" => returned[w] = 2
PublicationUsesDurableRoot == \A w \in Workers : phase[w] = "Done" => w \in durable
IndependentPreparation == \A w \in Workers : phase[w] = "Written" => ENABLED Prepare(w)
=============================================================================
