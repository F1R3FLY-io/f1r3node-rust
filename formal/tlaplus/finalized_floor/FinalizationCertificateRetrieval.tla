---------------- MODULE FinalizationCertificateRetrieval ----------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANT
  \* @type: Int;
  MaxTracked,
  \* @type: Bool;
  TypedDependencies,
  \* @type: Bool;
  ValidateResponses,
  \* @type: Bool;
  RequireExpectedResponse,
  \* @type: Bool;
  RetainFailedRequests,
  \* @type: Bool;
  RebuildAfterRestart,
  \* @type: Bool;
  DeduplicateQueue

ASSUME /\ MaxTracked \in Nat \ {0}
       /\ TypedDependencies \in BOOLEAN
       /\ ValidateResponses \in BOOLEAN
       /\ RequireExpectedResponse \in BOOLEAN
       /\ RetainFailedRequests \in BOOLEAN
       /\ RebuildAfterRestart \in BOOLEAN
       /\ DeduplicateQueue \in BOOLEAN

Blocks == {"x", "y"}
Digests == {"x", "y"}
ResponseIds == {"x1", "x2", "y1", "y2", "unsolicited", "badShape", "mismatch"}

\* @type: Str -> Str;
CertificateOf == [block \in Blocks |-> block]

\* @type: Str => Str;
ResponseDigest(response) ==
  CASE response \in {"x1", "x2", "unsolicited", "badShape", "mismatch"} -> "x"
    [] OTHER -> "y"

\* @type: Str => Str;
ResponseContentDigest(response) ==
  IF response = "mismatch" THEN "y" ELSE ResponseDigest(response)

\* @type: Str => Bool;
ResponseShapeValid(response) == response # "badShape"

\* @type: Str => Bool;
ValidResponse(response) ==
  /\ ResponseShapeValid(response)
  /\ ResponseContentDigest(response) = ResponseDigest(response)

\* @type: Str => Set(Str);
ValidCopies(digest) ==
  {response \in ResponseIds :
    /\ ResponseDigest(response) = digest
    /\ response \in {"x1", "x2", "y1", "y2"}}

\* @type: Str => Seq(Str);
BlockKey(block) == <<"block", block>>

\* @type: Str => Seq(Str);
CertificateKey(digest) ==
  IF TypedDependencies
  THEN <<"certificate", digest>>
  ELSE <<"block", digest>>

VARIABLES
  \* @type: Set(Str);
  detachedBlocks,
  \* @type: Set(Str);
  waitingBlocks,
  \* @type: Set(Str);
  certificateStore,
  \* @type: Set(Str);
  trackedDigests,
  \* @type: Set(Str);
  pendingResponses,
  \* @type: Set(Str);
  queuedBlocks,
  \* @type: Str -> Int;
  enqueueCount,
  \* @type: Str -> Int;
  attempts,
  \* @type: Bool;
  running,
  \* @type: Int;
  crashCount,
  \* @type: Set(Str);
  acceptedResponses,
  \* @type: Set(Str);
  lostOnFailure,
  \* @type: Set(Str);
  unsolicitedMutations,
  \* @type: Set(Str);
  restartStranding

vars == <<detachedBlocks, waitingBlocks, certificateStore, trackedDigests,
  pendingResponses, queuedBlocks, enqueueCount, attempts, running, crashCount,
  acceptedResponses, lostOnFailure, unsolicitedMutations, restartStranding>>

\* @type: Set(Seq(Str));
StoredKeys ==
  {BlockKey(block) : block \in detachedBlocks} \union
  {CertificateKey(digest) : digest \in certificateStore}

\* @type: Str => Bool;
DependencyResolved(block) ==
  CertificateKey(CertificateOf[block]) \in StoredKeys

\* @type: Str => Bool;
OutstandingDigest(digest) ==
  \E block \in waitingBlocks : CertificateOf[block] = digest

\* @type: Str => Bool;
ResponseExpected(response) ==
  ResponseDigest(response) \in trackedDigests

\* @type: Str => Bool;
ResponseAccepted(response) ==
  /\ (~RequireExpectedResponse \/ ResponseExpected(response))
  /\ (~ValidateResponses \/ ValidResponse(response))

\* @type: Int => Int;
NextAttempt(attempt) == IF attempt < 2 THEN attempt + 1 ELSE 2

Init ==
  /\ detachedBlocks = Blocks
  /\ waitingBlocks = Blocks
  /\ certificateStore = {}
  /\ trackedDigests = {}
  /\ pendingResponses = {"unsolicited", "badShape", "mismatch"}
  /\ queuedBlocks = {}
  /\ enqueueCount = [block \in Blocks |-> 0]
  /\ attempts = [digest \in Digests |-> 0]
  /\ running = TRUE
  /\ crashCount = 0
  /\ acceptedResponses = {}
  /\ lostOnFailure = {}
  /\ unsolicitedMutations = {}
  /\ restartStranding = {}

TrackDigest(digest) ==
  /\ running
  /\ digest \in Digests
  /\ OutstandingDigest(digest)
  /\ digest \notin certificateStore
  /\ digest \notin trackedDigests
  /\ Cardinality(trackedDigests) < MaxTracked
  /\ crashCount = 0 \/ RebuildAfterRestart
  /\ trackedDigests' = trackedDigests \union {digest}
  /\ UNCHANGED <<detachedBlocks, waitingBlocks, certificateStore,
       pendingResponses, queuedBlocks, enqueueCount, attempts, running,
       crashCount, acceptedResponses, lostOnFailure, unsolicitedMutations,
       restartStranding>>

SendFailure(digest) ==
  /\ running
  /\ digest \in trackedDigests
  /\ attempts' = [attempts EXCEPT ![digest] = NextAttempt(@)]
  /\ trackedDigests' =
       IF RetainFailedRequests THEN trackedDigests ELSE trackedDigests \ {digest}
  /\ lostOnFailure' =
       IF RetainFailedRequests THEN lostOnFailure ELSE lostOnFailure \union {digest}
  /\ UNCHANGED <<detachedBlocks, waitingBlocks, certificateStore,
       pendingResponses, queuedBlocks, enqueueCount, running, crashCount,
       acceptedResponses, unsolicitedMutations, restartStranding>>

SendSuccess(digest) ==
  /\ running
  /\ digest \in trackedDigests
  /\ attempts' = [attempts EXCEPT ![digest] = NextAttempt(@)]
  /\ pendingResponses' = pendingResponses \union ValidCopies(digest)
  /\ UNCHANGED <<detachedBlocks, waitingBlocks, certificateStore,
       trackedDigests, queuedBlocks, enqueueCount, running, crashCount,
       acceptedResponses, lostOnFailure, unsolicitedMutations,
       restartStranding>>

ReceiveResponse(response) ==
  /\ running
  /\ response \in pendingResponses
  /\ pendingResponses' = pendingResponses \ {response}
  /\ IF ResponseAccepted(response)
       THEN /\ certificateStore' = certificateStore \union {ResponseDigest(response)}
            /\ trackedDigests' = trackedDigests \ {ResponseDigest(response)}
            /\ acceptedResponses' = acceptedResponses \union {response}
            /\ unsolicitedMutations' =
                 IF ResponseExpected(response)
                 THEN unsolicitedMutations
                 ELSE unsolicitedMutations \union {response}
       ELSE /\ UNCHANGED <<certificateStore, trackedDigests,
                 acceptedResponses, unsolicitedMutations>>
  /\ UNCHANGED <<detachedBlocks, waitingBlocks, queuedBlocks, enqueueCount,
       attempts, crashCount, running, lostOnFailure, restartStranding>>

ResolveStoredDependency(block) ==
  /\ running
  /\ block \in waitingBlocks
  /\ DependencyResolved(block)
  /\ waitingBlocks' = waitingBlocks \ {block}
  /\ UNCHANGED <<detachedBlocks, certificateStore, trackedDigests,
       pendingResponses, queuedBlocks, enqueueCount, attempts, running,
       crashCount, acceptedResponses, lostOnFailure, unsolicitedMutations,
       restartStranding>>

WakeBlock(block) ==
  /\ running
  /\ block \in detachedBlocks
  /\ block \notin waitingBlocks
  /\ CertificateOf[block] \in certificateStore
  /\ ~DeduplicateQueue \/ block \notin queuedBlocks
  /\ queuedBlocks' = queuedBlocks \union {block}
  /\ enqueueCount' = [enqueueCount EXCEPT ![block] = @ + 1]
  /\ UNCHANGED <<detachedBlocks, waitingBlocks, certificateStore,
       trackedDigests, pendingResponses, attempts, running, crashCount,
       acceptedResponses, lostOnFailure, unsolicitedMutations,
       restartStranding>>

Crash ==
  /\ running
  /\ crashCount = 0
  /\ running' = FALSE
  /\ crashCount' = 1
  /\ trackedDigests' = {}
  /\ pendingResponses' = {}
  /\ UNCHANGED <<detachedBlocks, waitingBlocks, certificateStore,
       queuedBlocks, enqueueCount, attempts, acceptedResponses,
       lostOnFailure, unsolicitedMutations, restartStranding>>

Restart ==
  /\ ~running
  /\ running' = TRUE
  /\ restartStranding' =
       IF RebuildAfterRestart THEN {} ELSE waitingBlocks
  /\ UNCHANGED <<detachedBlocks, waitingBlocks, certificateStore,
       trackedDigests, pendingResponses, queuedBlocks, enqueueCount, attempts,
       crashCount, acceptedResponses, lostOnFailure, unsolicitedMutations>>

Idle ==
  /\ queuedBlocks = detachedBlocks
  /\ UNCHANGED vars

Next ==
  \/ \E digest \in Digests : TrackDigest(digest)
  \/ \E digest \in Digests : SendFailure(digest)
  \/ \E digest \in Digests : SendSuccess(digest)
  \/ \E response \in ResponseIds : ReceiveResponse(response)
  \/ \E block \in Blocks : ResolveStoredDependency(block)
  \/ \E block \in Blocks : WakeBlock(block)
  \/ Crash
  \/ Restart
  \/ Idle

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A digest \in Digests : WF_vars(TrackDigest(digest))
  /\ \A digest \in Digests : WF_vars(SendSuccess(digest))
  /\ \A response \in ResponseIds : WF_vars(ReceiveResponse(response))
  /\ \A block \in Blocks : WF_vars(ResolveStoredDependency(block))
  /\ \A block \in Blocks : WF_vars(WakeBlock(block))
  /\ WF_vars(Restart)

TypeOK ==
  /\ detachedBlocks \subseteq Blocks
  /\ waitingBlocks \subseteq Blocks
  /\ certificateStore \subseteq Digests
  /\ trackedDigests \subseteq Digests
  /\ pendingResponses \subseteq ResponseIds
  /\ queuedBlocks \subseteq Blocks
  /\ enqueueCount \in [Blocks -> Nat]
  /\ attempts \in [Digests -> 0..2]
  /\ running \in BOOLEAN
  /\ crashCount \in 0..1
  /\ acceptedResponses \subseteq ResponseIds
  /\ lostOnFailure \subseteq Digests
  /\ unsolicitedMutations \subseteq ResponseIds
  /\ restartStranding \subseteq Blocks

RequestTrackerIsBounded == Cardinality(trackedDigests) <= MaxTracked

TypedDependencyNamespaceIsDisjoint ==
  \A block \in Blocks, digest \in Digests :
    BlockKey(block) # CertificateKey(digest)

ResolvedOnlyAfterCertificatePersistence ==
  \A block \in detachedBlocks \ waitingBlocks :
    CertificateOf[block] \in certificateStore

OnlyValidResponsesPersist ==
  \A response \in acceptedResponses : ValidResponse(response)

UnsolicitedResponsesDoNotMutate == unsolicitedMutations = {}

FailedSendsRetainObligations == lostOnFailure = {}

RestartNeverStrandsPersistentObligations == restartStranding = {}

BufferedBlocksAreNotQueued == waitingBlocks \cap queuedBlocks = {}

EveryBlockIsQueuedAtMostOnce ==
  \A block \in Blocks : enqueueCount[block] <= 1

QueueSetMatchesCounts ==
  \A block \in Blocks : (block \in queuedBlocks) = (enqueueCount[block] = 1)

AllDetachedBlocksEventuallyQueue == <> (queuedBlocks = detachedBlocks)

=============================================================================
