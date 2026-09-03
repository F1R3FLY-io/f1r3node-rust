------------------------- MODULE FinalizationAtomicity -------------------------
EXTENDS FiniteSets, Integers, TLC

CONSTANT
    \* @type: Int;
    MaxRequests,
    \* @type: Int;
    MaxRounds,
    \* @type: Int;
    MaxAdmissions,
    \* @type: Bool;
    UnsafeAdvanceHeadBeforeRecord,
    \* @type: Bool;
    UnsafeEffectBeforeCommit,
    \* @type: Bool;
    UnsafeStaleOverwrite,
    \* @type: Bool;
    UnsafeRegressivePublication,
    \* @type: Bool;
    UnsafeLostWake

ASSUME /\ MaxRequests \in Nat \ {0}
       /\ MaxRounds \in Nat \ {0}
       /\ MaxAdmissions \in Nat \ {0}
       /\ UnsafeAdvanceHeadBeforeRecord \in BOOLEAN
       /\ UnsafeEffectBeforeCommit \in BOOLEAN
       /\ UnsafeStaleOverwrite \in BOOLEAN
       /\ UnsafeRegressivePublication \in BOOLEAN
       /\ UnsafeLostWake \in BOOLEAN

Workers == {1, 2}
Rounds == 1..MaxRounds
Phases == {"Idle", "Evaluate", "Manifest", "Append", "Effect", "Complete"}

VARIABLES
    \* @type: Int;
    requestedThrough,
    \* @type: Int;
    launchedThrough,
    \* @type: Int;
    completedThrough,
    \* @type: Bool;
    dispatcherRunning,
    \* @type: Int -> Str;
    workerPhase,
    \* @type: Int -> Int;
    workerExpected,
    \* @type: Int -> Int;
    workerCandidate,
    \* @type: Int -> Int;
    workerCovered,
    \* @type: Set(Int);
    manifests,
    \* @type: Set(Int);
    records,
    \* @type: Int;
    durableHead,
    \* @type: Set(Int);
    effects,
    \* @type: Set(Int);
    receipts,
    \* @type: Int;
    publishedHead,
    \* @type: Int;
    publicationHighWater,
    \* @type: Int;
    admissions

vars == <<requestedThrough, launchedThrough, completedThrough,
          dispatcherRunning, workerPhase, workerExpected, workerCandidate,
          workerCovered, manifests, records, durableHead, effects, receipts,
          publishedHead, publicationHighWater, admissions>>

\* @type: (Int, Int) => Int;
Max(left, right) == IF left >= right THEN left ELSE right

\* @type: Set(Int);
ActiveWorkers == {worker \in Workers : workerPhase[worker] # "Idle"}

\* @type: Set(Int);
CommittedPrefix == {round \in Rounds : round <= durableHead}

Init ==
    /\ requestedThrough = 0
    /\ launchedThrough = 0
    /\ completedThrough = 0
    /\ dispatcherRunning = FALSE
    /\ workerPhase = [worker \in Workers |-> "Idle"]
    /\ workerExpected = [worker \in Workers |-> 0]
    /\ workerCandidate = [worker \in Workers |-> 0]
    /\ workerCovered = [worker \in Workers |-> 0]
    /\ manifests = {}
    /\ records = {}
    /\ durableHead = 0
    /\ effects = {}
    /\ receipts = {}
    /\ publishedHead = 0
    /\ publicationHighWater = 0
    /\ admissions = 0

Request ==
    /\ requestedThrough < MaxRequests
    /\ requestedThrough' = requestedThrough + 1
    /\ dispatcherRunning' = TRUE
    /\ UNCHANGED <<launchedThrough, completedThrough, workerPhase,
                    workerExpected, workerCandidate, workerCovered, manifests,
                    records, durableHead, effects, receipts, publishedHead,
                    publicationHighWater, admissions>>

AdmitBlock ==
    /\ admissions < MaxAdmissions
    /\ admissions' = admissions + 1
    /\ IF requestedThrough < MaxRequests
       THEN
         /\ requestedThrough' = requestedThrough + 1
         /\ dispatcherRunning' = TRUE
       ELSE
         /\ UNCHANGED <<requestedThrough, dispatcherRunning>>
    /\ UNCHANGED <<launchedThrough, completedThrough, workerPhase,
                    workerExpected, workerCandidate, workerCovered, manifests,
                    records, durableHead, effects, receipts, publishedHead,
                    publicationHighWater>>

Launch(worker) ==
    /\ dispatcherRunning
    /\ workerPhase[worker] = "Idle"
    /\ requestedThrough > launchedThrough
    /\ durableHead < MaxRounds
    /\ workerPhase' = [workerPhase EXCEPT ![worker] = "Evaluate"]
    /\ workerExpected' = [workerExpected EXCEPT ![worker] = durableHead]
    /\ workerCandidate' = [workerCandidate EXCEPT ![worker] = durableHead + 1]
    /\ workerCovered' = [workerCovered EXCEPT ![worker] = requestedThrough]
    /\ launchedThrough' = requestedThrough
    /\ UNCHANGED <<requestedThrough, completedThrough, dispatcherRunning,
                    manifests, records, durableHead, effects, receipts,
                    publishedHead, publicationHighWater, admissions>>

Evaluate(worker) ==
    /\ workerPhase[worker] = "Evaluate"
    /\ workerPhase' = [workerPhase EXCEPT ![worker] = "Manifest"]
    /\ UNCHANGED <<requestedThrough, launchedThrough, completedThrough,
                    dispatcherRunning, workerExpected, workerCandidate,
                    workerCovered, manifests, records, durableHead, effects,
                    receipts, publishedHead, publicationHighWater, admissions>>

PrepareManifest(worker) ==
    /\ workerPhase[worker] = "Manifest"
    /\ workerCandidate[worker] \in Rounds
    /\ manifests' = manifests \cup {workerCandidate[worker]}
    /\ workerPhase' = [workerPhase EXCEPT ![worker] = "Append"]
    /\ UNCHANGED <<requestedThrough, launchedThrough, completedThrough,
                    dispatcherRunning, workerExpected, workerCandidate,
                    workerCovered, records, durableHead, effects, receipts,
                    publishedHead, publicationHighWater, admissions>>

CommitRecord(worker) ==
    /\ workerPhase[worker] = "Append"
    /\ workerCandidate[worker] \in manifests
    /\ workerExpected[worker] = durableHead
    /\ workerCandidate[worker] = durableHead + 1
    /\ records' = records \cup {workerCandidate[worker]}
    /\ durableHead' = workerCandidate[worker]
    /\ workerPhase' = [workerPhase EXCEPT ![worker] = "Effect"]
    /\ UNCHANGED <<requestedThrough, launchedThrough, completedThrough,
                    dispatcherRunning, workerExpected, workerCandidate,
                    workerCovered, manifests, effects, receipts, publishedHead,
                    publicationHighWater, admissions>>

AlreadyCommitted(worker) ==
    /\ workerPhase[worker] = "Append"
    /\ workerCandidate[worker] \in records
    /\ workerCandidate[worker] <= durableHead
    /\ workerPhase' = [workerPhase EXCEPT
         ![worker] = IF durableHead < MaxRounds THEN "Evaluate" ELSE "Complete"]
    /\ workerExpected' = [workerExpected EXCEPT ![worker] = durableHead]
    /\ workerCandidate' = [workerCandidate EXCEPT
         ![worker] = IF durableHead < MaxRounds THEN durableHead + 1 ELSE durableHead]
    /\ UNCHANGED <<requestedThrough, launchedThrough, completedThrough,
                    dispatcherRunning, workerCovered, manifests, records, durableHead, effects,
                    receipts, publishedHead, publicationHighWater, admissions>>

Stale(worker) ==
    /\ workerPhase[worker] = "Append"
    /\ workerExpected[worker] # durableHead
    /\ workerPhase' = [workerPhase EXCEPT
         ![worker] = IF durableHead < MaxRounds THEN "Evaluate" ELSE "Complete"]
    /\ workerExpected' = [workerExpected EXCEPT ![worker] = durableHead]
    /\ workerCandidate' = [workerCandidate EXCEPT
         ![worker] = IF durableHead < MaxRounds THEN durableHead + 1 ELSE durableHead]
    /\ UNCHANGED <<requestedThrough, launchedThrough, completedThrough,
                    dispatcherRunning, workerCovered, manifests, records, durableHead, effects,
                    receipts, publishedHead, publicationHighWater, admissions>>

ApplyEffect(worker) ==
    /\ workerPhase[worker] = "Effect"
    /\ workerCandidate[worker] \in records
    /\ effects' = effects \cup {workerCandidate[worker]}
    /\ workerPhase' = [workerPhase EXCEPT ![worker] = "Complete"]
    /\ UNCHANGED <<requestedThrough, launchedThrough, completedThrough,
                    dispatcherRunning, workerExpected, workerCandidate,
                    workerCovered, manifests, records, durableHead, receipts,
                    publishedHead, publicationHighWater, admissions>>

RecordReceipt(round) ==
    /\ round \in effects
    /\ receipts' = receipts \cup {round}
    /\ UNCHANGED <<requestedThrough, launchedThrough, completedThrough,
                    dispatcherRunning, workerPhase, workerExpected,
                    workerCandidate, workerCovered, manifests, records,
                    durableHead, effects, publishedHead, publicationHighWater,
                    admissions>>

Publish ==
    /\ publishedHead < durableHead
    /\ publishedHead' = durableHead
    /\ publicationHighWater' = durableHead
    /\ UNCHANGED <<requestedThrough, launchedThrough, completedThrough,
                    dispatcherRunning, workerPhase, workerExpected,
                    workerCandidate, workerCovered, manifests, records, effects,
                    receipts, durableHead, admissions>>

Complete(worker) ==
    /\ workerPhase[worker] = "Complete"
    /\ completedThrough' = Max(completedThrough, workerCovered[worker])
    /\ workerPhase' = [workerPhase EXCEPT ![worker] = "Idle"]
    /\ UNCHANGED <<requestedThrough, launchedThrough, dispatcherRunning,
                    workerExpected, workerCandidate, workerCovered, manifests,
                    records, durableHead, effects, receipts, publishedHead,
                    publicationHighWater, admissions>>

Park ==
    /\ dispatcherRunning
    /\ ActiveWorkers = {}
    /\ requestedThrough <= completedThrough
    /\ dispatcherRunning' = FALSE
    /\ UNCHANGED <<requestedThrough, launchedThrough, completedThrough,
                    workerPhase, workerExpected, workerCandidate, workerCovered,
                    manifests, records, durableHead, effects, receipts,
                    publishedHead, publicationHighWater, admissions>>

Restart ==
    /\ ActiveWorkers # {}
    /\ workerPhase' = [worker \in Workers |-> "Idle"]
    /\ launchedThrough' = completedThrough
    /\ dispatcherRunning' = (requestedThrough > completedThrough)
    /\ publishedHead' = durableHead
    /\ publicationHighWater' = durableHead
    /\ UNCHANGED <<requestedThrough, completedThrough, workerExpected,
                    workerCandidate, workerCovered, manifests, records,
                    durableHead, effects, receipts, admissions>>

UnsafeHeadBeforeRecord(worker) ==
    /\ UnsafeAdvanceHeadBeforeRecord
    /\ workerPhase[worker] = "Append"
    /\ workerExpected[worker] = durableHead
    /\ workerCandidate[worker] = durableHead + 1
    /\ durableHead' = workerCandidate[worker]
    /\ workerPhase' = [workerPhase EXCEPT ![worker] = "Effect"]
    /\ UNCHANGED <<requestedThrough, launchedThrough, completedThrough,
                    dispatcherRunning, workerExpected, workerCandidate,
                    workerCovered, manifests, records, effects, receipts,
                    publishedHead, publicationHighWater, admissions>>

UnsafeEarlyEffect(worker) ==
    /\ UnsafeEffectBeforeCommit
    /\ workerPhase[worker] = "Append"
    /\ effects' = effects \cup {workerCandidate[worker]}
    /\ workerPhase' = [workerPhase EXCEPT ![worker] = "Complete"]
    /\ UNCHANGED <<requestedThrough, launchedThrough, completedThrough,
                    dispatcherRunning, workerExpected, workerCandidate,
                    workerCovered, manifests, records, durableHead, receipts,
                    publishedHead, publicationHighWater, admissions>>

UnsafeOverwrite(worker) ==
    /\ UnsafeStaleOverwrite
    /\ workerPhase[worker] = "Append"
    /\ workerExpected[worker] # durableHead
    /\ workerCandidate[worker] \in Rounds
    /\ records' = records \cup {workerCandidate[worker]}
    /\ durableHead' = workerCandidate[worker]
    /\ workerPhase' = [workerPhase EXCEPT ![worker] = "Effect"]
    /\ UNCHANGED <<requestedThrough, launchedThrough, completedThrough,
                    dispatcherRunning, workerExpected, workerCandidate,
                    workerCovered, manifests, effects, receipts, publishedHead,
                    publicationHighWater, admissions>>

UnsafePublishRegression ==
    /\ UnsafeRegressivePublication
    /\ publicationHighWater > 0
    /\ publishedHead' = publicationHighWater - 1
    /\ UNCHANGED <<requestedThrough, launchedThrough, completedThrough,
                    dispatcherRunning, workerPhase, workerExpected,
                    workerCandidate, workerCovered, manifests, records,
                    durableHead, effects, receipts, publicationHighWater,
                    admissions>>

UnsafeParkWithConcurrentRequest ==
    /\ UnsafeLostWake
    /\ dispatcherRunning
    /\ ActiveWorkers = {}
    /\ requestedThrough < MaxRequests
    /\ requestedThrough' = requestedThrough + 1
    /\ dispatcherRunning' = FALSE
    /\ UNCHANGED <<launchedThrough, completedThrough, workerPhase,
                    workerExpected, workerCandidate, workerCovered, manifests,
                    records, durableHead, effects, receipts, publishedHead,
                    publicationHighWater, admissions>>

Next ==
    \/ Request
    \/ AdmitBlock
    \/ \E worker \in Workers : Launch(worker)
    \/ \E worker \in Workers : Evaluate(worker)
    \/ \E worker \in Workers : PrepareManifest(worker)
    \/ \E worker \in Workers : CommitRecord(worker)
    \/ \E worker \in Workers : AlreadyCommitted(worker)
    \/ \E worker \in Workers : Stale(worker)
    \/ \E worker \in Workers : ApplyEffect(worker)
    \/ \E round \in Rounds : RecordReceipt(round)
    \/ Publish
    \/ \E worker \in Workers : Complete(worker)
    \/ Park
    \/ Restart
    \/ \E worker \in Workers : UnsafeHeadBeforeRecord(worker)
    \/ \E worker \in Workers : UnsafeEarlyEffect(worker)
    \/ \E worker \in Workers : UnsafeOverwrite(worker)
    \/ UnsafePublishRegression
    \/ UnsafeParkWithConcurrentRequest

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ requestedThrough \in 0..MaxRequests
    /\ launchedThrough \in 0..MaxRequests
    /\ completedThrough \in 0..MaxRequests
    /\ dispatcherRunning \in BOOLEAN
    /\ workerPhase \in [Workers -> Phases]
    /\ workerExpected \in [Workers -> 0..MaxRounds]
    /\ workerCandidate \in [Workers -> 0..MaxRounds]
    /\ workerCovered \in [Workers -> 0..MaxRequests]
    /\ manifests \in SUBSET Rounds
    /\ records \in SUBSET Rounds
    /\ durableHead \in 0..MaxRounds
    /\ effects \in SUBSET Rounds
    /\ receipts \in SUBSET Rounds
    /\ publishedHead \in 0..MaxRounds
    /\ publicationHighWater \in 0..MaxRounds
    /\ admissions \in 0..MaxAdmissions

Inv_HeadHasRecord == durableHead = 0 \/ durableHead \in records
Inv_RecordPrefix == records = CommittedPrefix
Inv_EffectsRequireCommit == effects \subseteq records
Inv_ReceiptsFollowEffects == receipts \subseteq effects
Inv_ManifestPrecedesCommit == records \subseteq manifests
Inv_PublicationBounded == publishedHead <= durableHead
Inv_PublicationMonotonic == publishedHead = publicationHighWater
Inv_RequestCountersMonotonic ==
    /\ completedThrough <= launchedThrough
    /\ launchedThrough <= requestedThrough
Inv_NoLostWake ==
    requestedThrough > completedThrough =>
        dispatcherRunning \/ ActiveWorkers # {}

Safety ==
    /\ TypeOK
    /\ Inv_HeadHasRecord
    /\ Inv_RecordPrefix
    /\ Inv_EffectsRequireCommit
    /\ Inv_ReceiptsFollowEffects
    /\ Inv_ManifestPrecedesCommit
    /\ Inv_PublicationBounded
    /\ Inv_PublicationMonotonic
    /\ Inv_RequestCountersMonotonic
    /\ Inv_NoLostWake

=============================================================================
