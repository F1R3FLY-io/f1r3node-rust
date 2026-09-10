---------------- MODULE MC_RecoveryDispatcherComposition ----------------
EXTENDS Naturals, FiniteSets

VARIABLE
    \* @type: {
    \* supervisor: Str, parentAlive: Bool, receiverOpen: Bool, controlsStopped: Bool,
    \* receiverOwner: Bool, dispatched: Set(Str), externalSenders: Bool,
    \* jobPC: Str -> Str, jobContext: Str -> Int,
    \* handles: Set(Str), liveJobs: Set(Str), workerAborts: Set(Str),
    \* payloads: Set(Str), bytes: Set(Str), identities: Set(Str), armed: Set(Str),
    \* releaseWakes: Set(Str), rejectedWake: Bool,
    \* servicePC: Str -> Str, serviceOwners: Set(Str), liveServices: Set(Str),
    \* serviceAborts: Set(Str), senderBorrows: Set(Str), metricBorrow: Bool,
    \* recoveryBody: Str, scannerBodies: Set(Str), loadCounts: Str -> Int,
    \* loadDuringPresence: Bool, loadWithoutVisit: Bool, waitReason: Str, notice: Bool,
    \* proposalRequests: Set(Str), issued: Set(Str), shared: Int,
    \* sharedIDs: Set(<<Str, Str>>), successor: Int, successorIDs: Set(<<Str, Str>>),
    \* passPC: Str, passOrigin: Int, remaining: Int, passProposal: Bool, failed: Bool,
    \* pendingCandidate: Str, candidateWitness: Str, successorMaintenance: Bool,
    \* pageAllowance: Int, pageSpent: Int, pageSuspended: Bool, attempt: Bool,
    \* loadPermit: Bool, loadWithoutAttempt: Bool, ackPending: Bool, passErrorWitness: Bool,
    \* context: Int, pendingOffer: Str, activeOffer: Str, offerPC: Str -> Str,
    \* offerOrigin: Str -> Int, authorizedAt: Str -> Int,
    \* proposalSnapshot: Bool, proposalReady: Bool, capturePC: Str -> Str,
    \* captureOrigin: Str -> Int, captureRoots: Set(Str), cancelledCaptures: Set(Str),
    \* driverCapture: Str};
    composed

Dispatcher == INSTANCE RecoveryDispatcherComposition WITH
    s <- composed,
    Jobs <- {"J1", "J2", "J3"},
    Requests <- {"R1", "R2", "R3"},
    Offers <- {"O1", "O2", "O3"},
    Captures <- {"K1", "K2"},
    CountCap <- 2,
    WorkerCap <- 2,
    PassVisits <- 2,
    PageSize <- 2,
    Defect <- "Safe"

Init == Dispatcher!Init
Next == Dispatcher!Next
Safety == Dispatcher!Safety
Jobs == {"J1", "J2", "J3"}
Services == {"recovery", "proposal"}
Requests == {"R1", "R2", "R3"}
Offers == {"O1", "O2", "O3"}
Captures == {"K1", "K2"}
None == "none"
J(stages) == {j \in Jobs : composed.jobPC[j] \in stages}

StateDomain ==
    composed \in [
      supervisor: {"running", "requested", "closed", "signaled", "aborting", "stopped", "quiesced"},
      parentAlive: BOOLEAN, receiverOpen: BOOLEAN, controlsStopped: BOOLEAN,
      receiverOwner: BOOLEAN, dispatched: SUBSET Jobs, externalSenders: BOOLEAN,
      jobPC: [Jobs -> Dispatcher!JobStages], jobContext: [Jobs -> 0..2],
      handles: SUBSET Jobs, liveJobs: SUBSET Jobs, workerAborts: SUBSET Jobs,
      payloads: SUBSET Jobs, bytes: SUBSET Jobs, identities: SUBSET Jobs, armed: SUBSET Jobs,
      releaseWakes: SUBSET Jobs, rejectedWake: BOOLEAN,
      servicePC: [Services -> Dispatcher!ServiceStages], serviceOwners: SUBSET Services,
      liveServices: SUBSET Services, serviceAborts: SUBSET Services,
      senderBorrows: SUBSET {"recovery", "dispatcher"}, metricBorrow: BOOLEAN,
      recoveryBody: Jobs \cup {None}, scannerBodies: SUBSET Jobs, loadCounts: [Jobs -> 0..2],
      loadDuringPresence: BOOLEAN, loadWithoutVisit: BOOLEAN,
      waitReason: {"none", "count", "bytes", "yield", "ack", "signal"}, notice: BOOLEAN,
      proposalRequests: SUBSET Requests, issued: SUBSET Requests, shared: 0..2,
      sharedIDs: SUBSET Dispatcher!Events, successor: 0..2, successorIDs: SUBSET Dispatcher!Events,
      passPC: {"idle", "scanning", "complete"}, passOrigin: 0..2, remaining: 0..2,
      passProposal: BOOLEAN, failed: BOOLEAN, context: 1..2,
      pendingCandidate: Jobs \cup {None}, candidateWitness: Jobs \cup {None},
      successorMaintenance: BOOLEAN, pageAllowance: 0..2, pageSpent: 0..2,
      pageSuspended: BOOLEAN, attempt: BOOLEAN, loadPermit: BOOLEAN, loadWithoutAttempt: BOOLEAN,
      ackPending: BOOLEAN, passErrorWitness: BOOLEAN,
      pendingOffer: Offers \cup {None}, activeOffer: Offers \cup {None},
      offerPC: [Offers -> Dispatcher!OfferStages], offerOrigin: [Offers -> 0..2],
      authorizedAt: [Offers -> 0..2], proposalSnapshot: BOOLEAN, proposalReady: BOOLEAN,
      capturePC: [Captures -> Dispatcher!CaptureStages], captureOrigin: [Captures -> 0..2],
      captureRoots: SUBSET Captures, cancelledCaptures: SUBSET Captures,
      driverCapture: Captures \cup {None}]

WorkerState ==
    /\ composed.payloads = J({"queued", "active"})
    /\ composed.bytes = J({"queued", "active", "payload-dropped"})
    /\ composed.identities = J({"queued", "active", "payload-dropped", "bytes-released"})
    /\ composed.armed = J({"queued", "active", "payload-dropped", "bytes-released", "identity-released"})
    /\ composed.releaseWakes = J({"woken", "retired", "joined"})
    /\ composed.dispatched \subseteq
       J({"active", "payload-dropped", "bytes-released", "identity-released", "woken", "retired", "joined"})
    /\ J({"active"}) \subseteq composed.dispatched
    /\ composed.liveJobs = composed.dispatched \cap
       J({"active", "payload-dropped", "bytes-released", "identity-released", "woken"})
    /\ composed.handles = IF composed.parentAlive THEN composed.dispatched \cap
       J({"active", "payload-dropped", "bytes-released", "identity-released", "woken", "retired"})
       ELSE {}
    /\ composed.workerAborts = IF composed.supervisor = "aborting" THEN composed.liveJobs ELSE {}

SupervisorState ==
    /\ composed.receiverOpen = (composed.supervisor \in {"running", "requested"})
    /\ composed.controlsStopped = (composed.supervisor \in {"signaled", "aborting", "stopped", "quiesced"})
    /\ (~composed.parentAlive => composed.supervisor \in {"aborting", "quiesced"})
    /\ (~composed.receiverOwner =>
         ~composed.receiverOpen /\
         \A j \in Jobs \ composed.dispatched : composed.jobPC[j] \in {"fresh", "joined"})
    /\ (composed.metricBorrow => composed.parentAlive)

ServiceState ==
    /\ ((\E a \in Services : composed.servicePC[a] \in {"retired", "joined"}) =>
         composed.supervisor \in {"aborting", "stopped", "quiesced"})
    /\ composed.liveServices =
       {a \in Services : composed.servicePC[a] \in {"idle", "borrow", "release", "waiting", "busy"}}
    /\ composed.serviceOwners \subseteq {a \in Services : composed.servicePC[a] \notin {"fresh", "joined"}}
    /\ (composed.parentAlive => composed.liveServices \subseteq composed.serviceOwners)
    /\ (~composed.parentAlive => composed.serviceOwners = {})
    /\ composed.serviceAborts = IF composed.supervisor = "aborting" THEN composed.liveServices ELSE {}
    /\ ("recovery" \in composed.senderBorrows) =
       (composed.servicePC["recovery"] \in {"borrow", "release"})
    /\ (IF composed.recoveryBody = None THEN TRUE ELSE
         composed.servicePC["recovery"] = "borrow" /\ composed.passPC = "scanning" /\
         composed.jobPC[composed.recoveryBody] = "fresh")

ProposalState ==
    /\ (composed.servicePC["proposal"] = "busy") = (composed.activeOffer # None)
    /\ (composed.servicePC["proposal"] \in {"retired", "joined"} => composed.pendingOffer = None)
    /\ (IF composed.proposalSnapshot THEN
         IF composed.activeOffer = None THEN FALSE
         ELSE composed.offerPC[composed.activeOffer] = "active" /\ ~composed.proposalReady
         ELSE TRUE)
    /\ (composed.proposalReady => composed.activeOffer # None)
    /\ \A o \in Offers :
         /\ (composed.offerOrigin[o] = 0) = (composed.offerPC[o] = "fresh")
         /\ (composed.authorizedAt[o] # 0 =>
              composed.offerPC[o] \in {"authorized", "returned", "cancelled"})

PassState ==
    /\ (composed.passPC = "idle") = (composed.passOrigin = 0)
    /\ (composed.passPC \in {"idle", "complete"} => composed.remaining = 0)
    /\ (composed.passPC = "idle" => ~composed.passProposal /\ composed.pendingCandidate = None)
    /\ (composed.pendingCandidate # None => composed.passPC = "scanning")
    /\ (composed.recoveryBody # None => composed.pendingCandidate = composed.recoveryBody)
    /\ (composed.loadPermit => composed.attempt)
    /\ (composed.attempt => composed.pageSpent > 0)
    /\ (composed.recoveryBody # None => ~composed.loadPermit)
    /\ (composed.attempt => composed.servicePC["recovery"] \in {"idle", "borrow"})
    /\ (composed.pageSuspended => composed.servicePC["recovery"] = "waiting")
    /\ (composed.servicePC["recovery"] \in {"release", "waiting"} => ~composed.attempt)
    /\ (composed.controlsStopped => ~composed.successorMaintenance)
    /\ (composed.ackPending =>
         composed.passPC = "scanning" /\ composed.pendingCandidate = None /\ ~composed.loadPermit /\
         composed.servicePC["recovery"] \in {"borrow", "release", "waiting"} /\
         (composed.servicePC["recovery"] \in {"release", "waiting"} => composed.waitReason = "ack"))
    /\ (composed.waitReason = "ack" /\ composed.servicePC["recovery"] \in {"release", "waiting"} =>
         composed.ackPending /\ ~composed.pageSuspended)
    /\ LET published ==
         ({"external"} \X composed.issued) \cup
         ({"dequeue"} \X composed.dispatched) \cup
         ({"release"} \X composed.releaseWakes)
       IN composed.sharedIDs \cup composed.successorIDs \subseteq published
    /\ composed.sharedIDs \cap composed.successorIDs = {}

CaptureState ==
    /\ \A k \in Captures : (composed.captureOrigin[k] = 0) = (composed.capturePC[k] = "fresh")
    /\ composed.cancelledCaptures \subseteq {k \in Captures : composed.capturePC[k] # "fresh"}

IndInv == StateDomain /\ Safety /\ WorkerState /\ SupervisorState /\
          ServiceState /\ ProposalState /\ PassState /\ CaptureState
=============================================================================
