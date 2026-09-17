------------------------- MODULE ShardRuntimeIsolation -------------------------
EXTENDS FiniteSets, Naturals

CONSTANTS Defect, EnableCrashes

Shards == {1, 2}
Tasks == {1, 2, 3, 4}
MaxWorkers == 2

ASSUME
    /\ Defect \in {
        "None",
        "BlindStaleCommit",
        "CrossShardStateWrite",
        "CrossShardRootPublication",
        "ResourceLeak"
            }
    /\ EnableCrashes \in BOOLEAN

Owner(task) == IF task \in {1, 2} THEN 1 ELSE 2
OtherShard(shard) == IF shard = 1 THEN 2 ELSE 1

VARIABLES
    phase,
    observedVersion,
    running,
    allocated,
    committed,
    ledger,
    recordedRoots,
    version

vars == <<
    phase,
    observedVersion,
    running,
    allocated,
    committed,
    ledger,
    recordedRoots,
    version
>>

Init ==
    /\ phase = [task \in Tasks |-> "Queued"]
    /\ observedVersion = [task \in Tasks |-> 0]
    /\ running = {}
    /\ allocated = {}
    /\ committed = {}
    /\ ledger = [shard \in Shards |-> 0]
    /\ recordedRoots = [shard \in Shards |-> 0]
    /\ version = [shard \in Shards |-> 0]

Capture(task) ==
    /\ task \in Tasks
    /\ phase[task] \in {"Queued", "Retry"}
    /\ phase' = [phase EXCEPT ![task] = "Captured"]
    /\ observedVersion' = [observedVersion EXCEPT ![task] = version[Owner(task)]]
    /\ UNCHANGED <<running, allocated, committed, ledger, recordedRoots, version>>

Acquire(task) ==
    /\ task \in Tasks
    /\ phase[task] = "Captured"
    /\ Cardinality(running) < MaxWorkers
    /\ phase' = [phase EXCEPT ![task] = "Running"]
    /\ running' = running \union {task}
    /\ allocated' = allocated \union {task}
    /\ UNCHANGED <<observedVersion, committed, ledger, recordedRoots, version>>

Compute(task) ==
    /\ task \in Tasks
    /\ phase[task] = "Running"
    /\ phase' = [phase EXCEPT ![task] = "Computed"]
    /\ UNCHANGED <<
        observedVersion,
        running,
        allocated,
        committed,
        ledger,
        recordedRoots,
        version
        >>

CommitEnabled(task) ==
    /\ phase[task] = "Computed"
    /\ \/ Defect = "BlindStaleCommit"
       \/ observedVersion[task] = version[Owner(task)]

Commit(task) ==
    /\ task \in Tasks
    /\ CommitEnabled(task)
    /\ LET owner == Owner(task)
           stateTarget ==
               IF Defect = "CrossShardStateWrite" THEN OtherShard(owner) ELSE owner
           rootTarget ==
               IF Defect = "CrossShardRootPublication"
               THEN OtherShard(stateTarget)
               ELSE stateTarget
       IN /\ phase' = [phase EXCEPT ![task] = "Committed"]
          /\ committed' = committed \union {task}
          /\ ledger' = [ledger EXCEPT
              ![stateTarget] =
                  IF Defect = "BlindStaleCommit"
                  THEN observedVersion[task] + 1
                  ELSE @ + 1]
          /\ recordedRoots' = [recordedRoots EXCEPT ![rootTarget] = @ + 1]
          /\ version' = [version EXCEPT ![stateTarget] = @ + 1]
          /\ running' = running \ {task}
          /\ allocated' =
              IF Defect = "ResourceLeak" THEN allocated ELSE allocated \ {task}
    /\ UNCHANGED observedVersion

RetryStale(task) ==
    /\ task \in Tasks
    /\ phase[task] = "Computed"
    /\ observedVersion[task] # version[Owner(task)]
    /\ phase' = [phase EXCEPT ![task] = "Retry"]
    /\ running' = running \ {task}
    /\ allocated' =
        IF Defect = "ResourceLeak" THEN allocated ELSE allocated \ {task}
    /\ UNCHANGED <<observedVersion, committed, ledger, recordedRoots, version>>

Crash(task) ==
    /\ EnableCrashes
    /\ task \in Tasks
    /\ phase[task] \in {"Captured", "Running", "Computed"}
    /\ phase' = [phase EXCEPT ![task] = "Crashed"]
    /\ running' = running \ {task}
    /\ allocated' =
        IF Defect = "ResourceLeak" THEN allocated ELSE allocated \ {task}
    /\ UNCHANGED <<observedVersion, committed, ledger, recordedRoots, version>>

Restart(task) ==
    /\ task \in Tasks
    /\ phase[task] = "Crashed"
    /\ phase' = [phase EXCEPT ![task] = "Retry"]
    /\ UNCHANGED <<
        observedVersion,
        running,
        allocated,
        committed,
        ledger,
        recordedRoots,
        version
        >>

TerminalStutter ==
    /\ committed = Tasks
    /\ UNCHANGED vars

Next ==
    \/ \E task \in Tasks : Capture(task)
    \/ \E task \in Tasks : Acquire(task)
    \/ \E task \in Tasks : Compute(task)
    \/ \E task \in Tasks : Commit(task)
    \/ \E task \in Tasks : RetryStale(task)
    \/ \E task \in Tasks : Crash(task)
    \/ \E task \in Tasks : Restart(task)
    \/ TerminalStutter

OwnedCommitted(shard) == {task \in committed : Owner(task) = shard}

TypeOK ==
    /\ phase \in [Tasks -> {
        "Queued", "Captured", "Running", "Computed", "Retry", "Crashed", "Committed"
        }]
    /\ observedVersion \in [Tasks -> Nat]
    /\ running \subseteq Tasks
    /\ allocated \subseteq Tasks
    /\ committed \subseteq Tasks
    /\ ledger \in [Shards -> Nat]
    /\ recordedRoots \in [Shards -> Nat]
    /\ version \in [Shards -> Nat]

ResourceCapacityBounded == Cardinality(running) <= MaxWorkers

ResourceOwnershipExact == allocated = running

RunningPhaseOwnsResource ==
    \A task \in Tasks :
        (task \in running) <=> phase[task] \in {"Running", "Computed"}

LedgerMatchesOwnedCommits ==
    \A shard \in Shards : ledger[shard] = Cardinality(OwnedCommitted(shard))

RecordedRootsMatchLedger ==
    \A shard \in Shards : recordedRoots[shard] = ledger[shard]

VersionsMatchLedger ==
    \A shard \in Shards : version[shard] = ledger[shard]

CommittedTasksReleaseResources ==
    \A task \in committed : task \notin running /\ task \notin allocated

Liveness_AllTasksCommit == <> (committed = Tasks)

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ \A task \in Tasks :
        /\ WF_vars(Capture(task))
        /\ WF_vars(Acquire(task))
        /\ WF_vars(Compute(task))
        /\ WF_vars(Commit(task))
        /\ WF_vars(RetryStale(task))

=============================================================================
