--------------------- MODULE RecoveryDispatcherComposition ---------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS Jobs, Requests, Offers, Captures, CountCap, WorkerCap, PassVisits, PageSize, Defect
\* @typeAlias: dispatcherState = {
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
module_typedefs == TRUE
VARIABLE
    \* @type: $dispatcherState;
    s
\* @type: <<$dispatcherState>>;
vars == <<s>>
Services == {"recovery", "proposal"}
None == "none"
JobStages == {"fresh", "queued", "active", "payload-dropped", "bytes-released",
              "identity-released", "woken", "retired", "joined"}
ServiceStages == {"fresh", "idle", "borrow", "release", "waiting", "busy",
                  "retired", "joined"}
CaptureStages == {"fresh", "capturing", "pending", "driver-presence",
                  "driver-processing", "driver-complete", "driver-dropping",
                  "external-dropping", "retired"}
DriverCaptureStages == {"driver-presence", "driver-processing",
                        "driver-complete", "driver-dropping"}
OfferStages == {"fresh", "pending", "active", "authorized", "returned",
                "coalesced", "invalidated", "cancelled"}
Events == ({"external"} \X Requests) \cup ({"dequeue", "release", "rejected"} \X Jobs)
\* @type: ($dispatcherState) => Set(Str);
Queued(t) == {j \in Jobs : t.jobPC[j] = "queued"}
\* @type: ($dispatcherState) => Bool;
StrongEndpoint(t) == t.externalSenders \/ t.senderBorrows # {}
Merge(a, b) == IF a = 2 \/ b = 2 THEN 2 ELSE IF a > 0 \/ b > 0 THEN 1 ELSE 0
\* @type: ($dispatcherState, <<Str, Str>>) => Bool;
Kind(t, e) == e[1] = "external" /\ e[2] \in t.proposalRequests
\* @type: ($dispatcherState, Set(<<Str, Str>>)) => Int;
Expected(t, ids) == IF ids = {} THEN 0 ELSE IF \E e \in ids : Kind(t, e) THEN 2 ELSE 1
\* @type: ($dispatcherState, <<Str, Str>>) => $dispatcherState;
WithDemand(t, event) ==
    IF t.controlsStopped THEN t ELSE
    [t EXCEPT !.shared = Merge(@, IF Kind(t, event) THEN 2 ELSE 1),
              !.sharedIDs = @ \cup {event},
              !.notice = (@ \/ t.shared = 0)]

Init ==
    \E proposalRequests \in SUBSET Requests :
    s = [supervisor |-> "running", parentAlive |-> TRUE,
         receiverOpen |-> TRUE, controlsStopped |-> FALSE,
         receiverOwner |-> TRUE, dispatched |-> {},
         externalSenders |-> TRUE,
         jobPC |-> [j \in Jobs |-> "fresh"], jobContext |-> [j \in Jobs |-> 0],
         handles |-> {}, liveJobs |-> {}, workerAborts |-> {},
         payloads |-> {}, bytes |-> {}, identities |-> {}, armed |-> {},
         releaseWakes |-> {}, rejectedWake |-> FALSE,
         servicePC |-> [a \in Services |-> "fresh"], serviceOwners |-> {},
         liveServices |-> {}, serviceAborts |-> {}, senderBorrows |-> {},
         metricBorrow |-> FALSE,
         recoveryBody |-> None, scannerBodies |-> {},
         loadCounts |-> [j \in Jobs |-> 0],
         loadDuringPresence |-> FALSE, loadWithoutVisit |-> FALSE,
         waitReason |-> "none", notice |-> FALSE,
         proposalRequests |-> proposalRequests, issued |-> {},
         shared |-> 0, sharedIDs |-> {}, successor |-> 0, successorIDs |-> {},
         passPC |-> "idle", passOrigin |-> 0, remaining |-> 0,
         passProposal |-> FALSE, failed |-> FALSE,
         pendingCandidate |-> None, candidateWitness |-> None, successorMaintenance |-> FALSE,
         pageAllowance |-> PageSize, pageSpent |-> 0, pageSuspended |-> FALSE,
         attempt |-> FALSE, loadPermit |-> FALSE, loadWithoutAttempt |-> FALSE,
         ackPending |-> FALSE, passErrorWitness |-> FALSE,
         context |-> 1,
         pendingOffer |-> None, activeOffer |-> None,
         offerPC |-> [o \in Offers |-> "fresh"],
         offerOrigin |-> [o \in Offers |-> 0], authorizedAt |-> [o \in Offers |-> 0],
         proposalSnapshot |-> FALSE, proposalReady |-> FALSE,
         capturePC |-> [k \in Captures |-> "fresh"],
         captureOrigin |-> [k \in Captures |-> 0],
         captureRoots |-> {}, cancelledCaptures |-> {}, driverCapture |-> None]

Enqueue(j) ==
    /\ s.receiverOpen /\ s.externalSenders /\ s.jobPC[j] = "fresh"
    /\ j \notin s.scannerBodies
    /\ Cardinality(Queued(s)) < CountCap
    /\ s' = [s EXCEPT !.jobPC[j] = "queued", !.jobContext[j] = s.context,
                      !.payloads = @ \cup {j}, !.bytes = @ \cup {j},
                      !.identities = @ \cup {j}, !.armed = @ \cup {j}]

Dispatch(j) ==
    /\ s.supervisor = "running" /\ s.jobPC[j] = "queued"
    /\ Cardinality(IF Defect = "WorkerCap" THEN s.liveJobs ELSE s.handles) < WorkerCap
    /\ s' = WithDemand(
        [s EXCEPT !.jobPC[j] = "active", !.liveJobs = @ \cup {j},
                  !.dispatched = @ \cup {j},
                  !.handles = IF Defect = "DetachedWorker" THEN @ ELSE @ \cup {j}],
        <<"dequeue", j>>)

DropPayload(j) ==
    /\ s.jobPC[j] = "active" \/ (s.jobPC[j] = "queued" /\ ~s.receiverOpen)
    /\ s' = [s EXCEPT !.jobPC[j] = "payload-dropped", !.payloads = @ \ {j}]

ReleaseBytes(j) ==
    /\ s.jobPC[j] = "payload-dropped"
    /\ s' = [s EXCEPT !.jobPC[j] = "bytes-released", !.bytes = @ \ {j}]

ReleaseIdentity(j) ==
    /\ s.jobPC[j] = "bytes-released"
    /\ s' = [s EXCEPT !.jobPC[j] = "identity-released", !.identities = @ \ {j}]

PublishRelease(j) ==
    /\ j \in s.armed
    /\ s.jobPC[j] = "identity-released"
       \/ (Defect = "EarlyWake" /\ j \in s.payloads /\ j \in s.liveJobs)
    /\ s' = WithDemand(
        [s EXCEPT !.jobPC[j] = "woken", !.armed = @ \ {j}, !.releaseWakes = @ \cup {j}],
        <<"release", j>>)

WorkerRetires(j) ==
    /\ s.jobPC[j] = "woken"
    /\ s' = [s EXCEPT !.jobPC[j] = "retired", !.liveJobs = @ \ {j},
                      !.workerAborts = @ \ {j}]

JoinWorker(j) ==
    /\ s.jobPC[j] = "retired"
    /\ s' = [s EXCEPT !.jobPC[j] = "joined", !.handles = @ \ {j}]

EarlyBytes(j) ==
    /\ Defect = "EarlyBytes" /\ j \in s.payloads /\ j \in s.bytes
    /\ s' = [s EXCEPT !.bytes = @ \ {j}]

EarlyIdentity(j) ==
    /\ Defect = "EarlyIdentity" /\ j \in s.bytes /\ j \in s.identities
    /\ s' = [s EXCEPT !.identities = @ \ {j}]

RejectedReservation ==
    /\ s.supervisor = "running" /\ Jobs # {}
    /\ Cardinality(Queued(s)) = CountCap
    /\ \E j \in Jobs :
        s' = IF Defect = "RejectedWake"
             THEN WithDemand([s EXCEPT !.rejectedWake = TRUE], <<"rejected", j>>)
             ELSE s

StartService(a) ==
    /\ s.supervisor = "running" /\ s.servicePC[a] = "fresh"
    /\ s' = [s EXCEPT !.servicePC[a] = "idle", !.liveServices = @ \cup {a},
             !.serviceOwners = IF Defect = "DetachedService" THEN @ ELSE @ \cup {a}]

ExternalRequest(r) ==
    /\ s.supervisor = "running" /\ r \notin s.issued
    /\ s' = WithDemand([s EXCEPT !.issued = @ \cup {r}], <<"external", r>>)

BeginPass ==
    /\ s.supervisor = "running" /\ s.servicePC["recovery"] = "idle"
    /\ s.passPC = "idle" /\ Merge(s.shared, s.successor) > 0
    /\ s' = [s EXCEPT !.passPC = "scanning", !.passOrigin = s.context,
             !.remaining = PassVisits, !.passProposal = (Merge(s.shared, s.successor) = 2),
             !.failed = FALSE, !.passErrorWitness = FALSE, !.shared = 0, !.sharedIDs = {},
             !.successor = 0, !.successorIDs = {}, !.successorMaintenance = FALSE]

TakeIntoNext ==
    /\ s.supervisor = "running" /\ s.servicePC["recovery"] \in {"idle", "waiting"}
    /\ s.shared > 0 /\ s.waitReason # "ack"
    /\ s' = [s EXCEPT !.successor = IF Defect = "DroppedWake" THEN @ ELSE Merge(@, s.shared),
             !.successorIDs = @ \cup s.sharedIDs, !.shared = 0, !.sharedIDs = {},
             !.notice = TRUE]

BeginAttempt ==
    /\ s.supervisor = "running" /\ s.servicePC["recovery"] = "idle"
    /\ ~s.attempt /\ ~s.pageSuspended /\ s.pageAllowance > 0
    /\ s' = [s EXCEPT !.pageAllowance = @ - 1, !.pageSpent = @ + 1,
                      !.attempt = TRUE, !.loadPermit = TRUE]

SelectCandidate(j) ==
    /\ s.supervisor = "running" /\ s.servicePC["recovery"] = "borrow"
    /\ Defect = "LoadWithoutAttempt" \/ (s.attempt /\ s.loadPermit)
    /\ s.pendingCandidate = None
    /\ s.passPC = "scanning" /\ s.passOrigin = s.context /\ s.remaining > 0
    /\ Cardinality(Queued(s)) < CountCap
    /\ s.driverCapture = None
    /\ s' = [s EXCEPT !.remaining = @ - 1,
                      !.pendingCandidate = j, !.candidateWitness = j]

VisitPass(error) ==
    /\ s.supervisor = "running" /\ s.servicePC["recovery"] = "idle"
    /\ s.passPC = "scanning" /\ s.passOrigin = s.context /\ s.remaining > 0
    /\ ~s.attempt /\ ~s.pageSuspended /\ s.pageAllowance > 0
    /\ s.pendingCandidate = None /\ s.driverCapture = None
    /\ Cardinality(Queued(s)) < CountCap
    /\ s' = [s EXCEPT !.remaining = @ - 1, !.failed = (@ \/ error), !.passErrorWitness = (@ \/ error),
                      !.pageAllowance = @ - 1, !.pageSpent = @ + 1]

ResolveCandidate(error) ==
    /\ s.supervisor = "running" /\ s.servicePC["recovery"] = "borrow"
    /\ s.attempt /\ s.pendingCandidate # None
    /\ s' = [s EXCEPT !.pendingCandidate = None, !.candidateWitness = None,
                      !.recoveryBody = None, !.scannerBodies = {},
                      !.loadPermit = FALSE, !.failed = (@ \/ error), !.passErrorWitness = (@ \/ error)]

FinishPass ==
    /\ s.supervisor = "running" /\ s.passPC = "scanning" /\ s.remaining = 0
    /\ s.servicePC["recovery"] = "idle" /\ ~s.attempt
    /\ Defect = "CompleteWithPending" \/ s.pendingCandidate = None
    /\ s' = [s EXCEPT !.passPC = "complete"]

EndPass ==
    /\ s.supervisor = "running" /\ s.passPC = "complete"
    /\ s.servicePC["recovery"] = "idle"
    /\ (~s.passProposal \/ s.failed \/ s.passOrigin # s.context)
    /\ s' = [s EXCEPT !.passPC = "idle", !.passOrigin = 0, !.passProposal = FALSE]

DiscardStalePass ==
    /\ s.supervisor = "running" /\ s.passPC # "idle" /\ s.passOrigin # s.context
    /\ s.servicePC["recovery"] = "idle"
    /\ s' = [s EXCEPT !.passPC = "idle", !.remaining = 0,
                      !.passOrigin = 0, !.passProposal = FALSE,
                      !.pendingCandidate = None, !.candidateWitness = None]

UpgradeEndpoint ==
    /\ s.supervisor = "running" /\ s.servicePC["recovery"] = "idle"
    /\ StrongEndpoint(s) /\ (s.attempt \/ Defect = "LoadWithoutAttempt")
    /\ s' = [s EXCEPT !.servicePC["recovery"] = "borrow",
                      !.senderBorrows = @ \cup {"recovery"}]

EndpointOperation(reason) ==
    /\ s.supervisor = "running" /\ s.servicePC["recovery"] = "borrow"
    /\ (reason = "count" => Cardinality(Queued(s)) = CountCap)
    /\ Defect = "BodyAcrossWait" \/ s.recoveryBody = None
    /\ (s.ackPending <=> reason = "ack")
    /\ s' = [s EXCEPT !.servicePC["recovery"] = "release", !.waitReason = reason,
                      !.attempt = FALSE, !.loadPermit = FALSE]

DropEndpoint ==
    /\ s.servicePC["recovery"] = "release"
    /\ s' = [s EXCEPT !.servicePC["recovery"] = IF s.waitReason = "none" THEN "idle" ELSE "waiting",
             !.pageSuspended = s.waitReason \in {"count", "bytes", "yield", "signal"},
             !.senderBorrows = IF Defect = "SenderAwait" THEN @ ELSE @ \ {"recovery"}]

PageYield ==
    /\ s.supervisor = "running" /\ s.servicePC["recovery"] = "idle"
    /\ ~s.attempt /\ ~s.pageSuspended
    /\ s' = [s EXCEPT !.servicePC["recovery"] = "waiting", !.waitReason = "yield",
                      !.pageSuspended = TRUE]

RetryDeadline ==
    /\ s.supervisor = "running" /\ s.servicePC["recovery"] = "waiting"
    /\ LET maintenance == s.passPC = "idle" /\ s.driverCapture = None
       IN s' = [s EXCEPT !.notice = TRUE,
                !.successor = IF maintenance \/ Defect = "ActiveRetryDemand" THEN Merge(@, 1) ELSE @,
                !.successorMaintenance = @ \/ maintenance]

WakeRecovery ==
    /\ s.supervisor = "running" /\ s.servicePC["recovery"] = "waiting"
    /\ s.waitReason # "ack"
    /\ s.notice \/ s.waitReason = "yield"
    /\ s.shared = 0
    /\ s' = [s EXCEPT !.servicePC["recovery"] = "idle", !.waitReason = "none",
                      !.pageAllowance = IF s.pageSuspended THEN PageSize ELSE @,
                      !.pageSpent = IF s.pageSuspended THEN 0 ELSE @,
                      !.pageSuspended = FALSE, !.notice = FALSE]

Acknowledge(error) ==
    /\ s.supervisor = "running" /\ s.servicePC["recovery"] = "waiting"
    /\ s.waitReason = "ack" /\ s.ackPending
    /\ s' = [s EXCEPT !.ackPending = FALSE, !.servicePC["recovery"] = "idle",
                      !.waitReason = "none",
                      !.failed = IF Defect = "IgnoreAcknowledgmentError" THEN @ ELSE @ \/ error,
                      !.passErrorWitness = @ \/ error]

RefuelWithoutSuspension ==
    /\ Defect = "RefuelWithoutSuspension"
    /\ s.supervisor = "running" /\ s.servicePC["recovery"] = "idle"
    /\ ~s.attempt /\ ~s.pageSuspended /\ s.pageSpent > 0
    /\ s' = [s EXCEPT !.pageAllowance = PageSize]

UpgradeMetricEndpoint ==
    /\ s.supervisor = "running" /\ ~s.metricBorrow /\ StrongEndpoint(s)
    /\ s' = [s EXCEPT !.metricBorrow = TRUE,
                      !.senderBorrows = @ \cup {"dispatcher"}]

DropMetricEndpoint ==
    /\ s.metricBorrow
    /\ s' = [s EXCEPT !.metricBorrow = FALSE,
                      !.senderBorrows = @ \ {"dispatcher"}]

LoadRecoveryBody(j) ==
    /\ s.supervisor = "running" /\ s.servicePC["recovery"] = "borrow"
    /\ s.passPC = "scanning" /\ s.passOrigin = s.context
    /\ s.recoveryBody = None /\ s.jobPC[j] = "fresh"
    /\ Defect = "CountBeforeLoad" \/ Cardinality(Queued(s)) < CountCap
    /\ Defect = "LoadWithoutVisit" \/ s.pendingCandidate = j
    /\ Defect = "LoadWithoutAttempt" \/ s.loadPermit
    /\ IF s.driverCapture = None THEN TRUE
       ELSE Defect = "PresenceBypass" \/ s.capturePC[s.driverCapture] # "driver-presence"
    /\ s' = [s EXCEPT !.recoveryBody = j, !.scannerBodies = @ \cup {j},
                      !.loadCounts[j] = Cardinality(Queued(s)),
                      !.loadDuringPresence = IF s.driverCapture = None THEN @
                         ELSE @ \/ s.capturePC[s.driverCapture] = "driver-presence",
                      !.loadWithoutVisit = @ \/ s.pendingCandidate # j,
                      !.loadWithoutAttempt = @ \/ ~s.loadPermit,
                      !.loadPermit = FALSE,
                      !.jobContext[j] = s.context]

AdmitRecoveryBody ==
    /\ s.supervisor = "running" /\ s.receiverOpen /\ s.recoveryBody # None
    /\ s.servicePC["recovery"] = "borrow" /\ Cardinality(Queued(s)) < CountCap
    /\ LET j == s.recoveryBody
       IN s' = [s EXCEPT !.jobPC[j] = "queued", !.recoveryBody = None,
                !.pendingCandidate = None, !.candidateWitness = None, !.ackPending = TRUE,
                !.scannerBodies = @ \ {j}, !.payloads = @ \cup {j},
                !.bytes = @ \cup {j}, !.identities = @ \cup {j},
                !.armed = @ \cup {j}]

TemporaryRejection ==
    /\ s.supervisor = "running" /\ s.servicePC["recovery"] = "borrow"
    /\ s.recoveryBody # None
    /\ s' = [s EXCEPT !.recoveryBody = None, !.scannerBodies = {},
                      !.pendingCandidate = IF Defect = "DropPendingOnTemporaryRejection" THEN None ELSE @,
                      !.servicePC["recovery"] = "release", !.waitReason = "bytes",
                      !.attempt = FALSE, !.loadPermit = FALSE]

DropRecoveryBody ==
    /\ s.supervisor # "running"
    /\ s.recoveryBody # None
    /\ s' = [s EXCEPT !.recoveryBody = None,
                      !.scannerBodies = @ \ {s.recoveryBody}]

CloseExternal ==
    /\ s.externalSenders
    /\ s' = [s EXCEPT !.externalSenders = FALSE]

ObserveInputClosed ==
    /\ s.supervisor = "running" /\ ~s.externalSenders
    /\ s.senderBorrows = {} /\ Queued(s) = {}
    /\ s' = [s EXCEPT !.supervisor = "requested"]

ReplaceContext ==
    /\ s.supervisor = "running" /\ s.context = 1
    /\ s' = [s EXCEPT !.context = 2, !.notice = TRUE]

Offer(o) ==
    /\ s.supervisor = "running" /\ s.passPC = "complete"
    /\ s.servicePC["recovery"] = "idle"
    /\ ~s.failed /\ s.passProposal /\ s.passOrigin = s.context
    /\ s.offerPC[o] = "fresh"
    /\ LET old == s.pendingOffer
       IN s' = [s EXCEPT !.pendingOffer = o, !.offerOrigin[o] = s.passOrigin,
                  !.offerPC = [candidate \in Offers |->
                      IF candidate = o THEN "pending"
                      ELSE IF candidate = old THEN "coalesced" ELSE s.offerPC[candidate]],
                  !.passPC = "idle", !.passOrigin = 0, !.passProposal = FALSE]

TakeOffer ==
    /\ s.supervisor = "running" /\ s.servicePC["proposal"] = "idle"
    /\ s.activeOffer = None /\ s.pendingOffer # None
    /\ LET o == s.pendingOffer
       IN s' = [s EXCEPT !.pendingOffer = None, !.activeOffer = o,
                         !.offerPC[o] = "active", !.servicePC["proposal"] = "busy",
                         !.proposalReady = FALSE]

ReadProposalSnapshot ==
    /\ s.supervisor = "running" /\ s.activeOffer # None
    /\ s.offerPC[s.activeOffer] = "active" /\ ~s.proposalReady
    /\ s' = [s EXCEPT !.proposalSnapshot = TRUE]

ReleaseProposalSnapshot ==
    /\ s.proposalSnapshot
    /\ s' = [s EXCEPT !.proposalSnapshot = FALSE, !.proposalReady = TRUE]

AuthorizeOffer ==
    /\ s.supervisor = "running" /\ s.activeOffer # None /\ s.proposalReady
    /\ ~s.proposalSnapshot /\ s.offerPC[s.activeOffer] = "active"
    /\ Defect = "StaleAuthorization" \/ s.offerOrigin[s.activeOffer] = s.context
    /\ LET o == s.activeOffer
       IN s' = [s EXCEPT !.offerPC[o] = "authorized", !.authorizedAt[o] = s.context]

RejectStaleOffer ==
    /\ s.supervisor = "running" /\ s.activeOffer # None
    /\ s.offerPC[s.activeOffer] = "active" /\ s.offerOrigin[s.activeOffer] # s.context
    /\ LET o == s.activeOffer
       IN s' = [s EXCEPT !.offerPC[o] = "invalidated", !.activeOffer = None,
                         !.servicePC["proposal"] = "idle",
                         !.proposalSnapshot = FALSE, !.proposalReady = FALSE]

CompleteOffer ==
    /\ s.activeOffer # None /\ s.offerPC[s.activeOffer] = "authorized"
    /\ LET o == s.activeOffer
       IN s' = [s EXCEPT !.offerPC[o] = "returned", !.activeOffer = None,
                  !.pendingOffer = IF Defect = "LostPendingOffer" THEN None ELSE @,
                  !.servicePC["proposal"] = "idle", !.proposalReady = FALSE]

GrantCapture(k) ==
    /\ s.supervisor = "running" /\ s.capturePC[k] = "fresh"
    /\ ~(\E other \in Captures : s.capturePC[other] \in
          IF Defect = "CaptureReuse" THEN {"capturing", "pending"}
          ELSE {"capturing", "pending", "external-dropping"})
    /\ s' = [s EXCEPT !.capturePC[k] = "capturing", !.captureOrigin[k] = s.context,
                      !.captureRoots = @ \cup {k}]

CaptureReturns(k) ==
    /\ s.capturePC[k] = "capturing"
    /\ s' = [s EXCEPT !.capturePC[k] =
        IF s.controlsStopped \/ s.captureOrigin[k] # s.context \/ k \in s.cancelledCaptures
        THEN "external-dropping" ELSE "pending"]

TakeCapture(k) ==
    /\ s.supervisor = "running" /\ s.servicePC["recovery"] = "idle"
    /\ s.capturePC[k] = "pending" /\ s.captureOrigin[k] = s.context
    /\ k \notin s.cancelledCaptures
    /\ s.driverCapture = None
    /\ s' = [s EXCEPT !.capturePC[k] = "driver-presence", !.driverCapture = k]

AdvanceCapture(k) ==
    /\ ~s.attempt /\ ~s.pageSuspended /\ s.pageAllowance > 0
    /\ s.supervisor = "running" /\ s.captureOrigin[k] = s.context
    /\ s.servicePC["recovery"] = "idle" /\ k \notin s.cancelledCaptures
    /\ s.driverCapture = k
    /\ s.capturePC[k] \in {"driver-presence", "driver-processing", "driver-complete"}
    /\ s' = [s EXCEPT !.pageAllowance = @ - 1, !.pageSpent = @ + 1, !.capturePC[k] =
        CASE @ = "driver-presence" -> "driver-processing"
          [] @ = "driver-processing" -> "driver-complete"
          [] OTHER -> "driver-dropping"]

CancelCapture(k) ==
    /\ s.controlsStopped \/ s.captureOrigin[k] # s.context \/ k \in s.cancelledCaptures
    /\ s.capturePC[k] = "pending" \/ s.capturePC[k] \in DriverCaptureStages \ {"driver-dropping"}
    /\ s' = [s EXCEPT !.capturePC[k] =
        IF s.driverCapture = k THEN "driver-dropping" ELSE "external-dropping"]

CancelTicket(k) ==
    /\ s.capturePC[k] \notin {"fresh", "retired"}
    /\ s' = [s EXCEPT !.cancelledCaptures = @ \cup {k}]

DropCapture(k) ==
    /\ s.capturePC[k] \in {"driver-dropping", "external-dropping"}
    /\ s' = [s EXCEPT !.capturePC[k] = "retired", !.captureRoots = @ \ {k},
                      !.driverCapture = IF @ = k THEN None ELSE @]

RequestStop ==
    /\ s.supervisor = "running"
    /\ s' = [s EXCEPT !.supervisor = "requested"]

CloseInput ==
    /\ s.supervisor = "requested"
    /\ s' = [s EXCEPT !.supervisor = "closed", !.receiverOpen = FALSE]

StopControls ==
    /\ s.supervisor = "closed"
    /\ s' = [s EXCEPT !.supervisor = "signaled", !.controlsStopped = TRUE,
             !.shared = 0, !.sharedIDs = {}, !.successor = 0, !.successorIDs = {},
             !.successorMaintenance = FALSE, !.notice = TRUE]

RequestAborts ==
    /\ s.supervisor = "signaled"
    /\ s' = [s EXCEPT !.supervisor = "aborting",
             !.workerAborts = IF Defect = "AbortRetires" THEN {} ELSE s.liveJobs \cap s.handles,
             !.serviceAborts = IF Defect = "AbortRetires" THEN {} ELSE s.liveServices \cap s.serviceOwners,
             !.handles = IF Defect = "AbortRetires" THEN {} ELSE @,
             !.serviceOwners = IF Defect = "AbortRetires" THEN {} ELSE @]

ParentDrop ==
    /\ s.parentAlive /\ s.supervisor # "stopped"
    /\ s' = [s EXCEPT !.parentAlive = FALSE, !.supervisor = "aborting",
             !.receiverOpen = FALSE, !.controlsStopped = TRUE,
             !.shared = 0, !.sharedIDs = {}, !.successor = 0, !.successorIDs = {},
             !.successorMaintenance = FALSE,
             !.workerAborts = IF Defect = "ParentAbortLost" THEN {} ELSE s.liveJobs,
             !.serviceAborts = s.liveServices,
             !.handles = {}, !.serviceOwners = {},
             !.metricBorrow = FALSE, !.senderBorrows = @ \ {"dispatcher"}]

RetireReceiver ==
    /\ ~s.receiverOpen /\ s.receiverOwner
    /\ \A j \in Jobs \ s.dispatched : s.jobPC[j] \in {"fresh", "joined"}
    /\ s' = [s EXCEPT !.receiverOwner = FALSE]

ServiceRetires(a) ==
    /\ s.supervisor = "aborting" /\ s.servicePC[a] \notin {"retired", "joined"}
    /\ (a = "recovery" =>
         ~(\E k \in Captures : s.capturePC[k] \in DriverCaptureStages) /\ s.recoveryBody = None)
    /\ s' = [s EXCEPT !.servicePC[a] = "retired", !.liveServices = @ \ {a},
             !.senderBorrows = @ \ {a}, !.serviceAborts = @ \ {a},
             !.ackPending = IF a = "recovery" THEN FALSE ELSE @,
             !.pendingCandidate = IF a = "recovery" THEN None ELSE @,
             !.candidateWitness = IF a = "recovery" THEN None ELSE @,
             !.attempt = IF a = "recovery" THEN FALSE ELSE @,
             !.loadPermit = IF a = "recovery" THEN FALSE ELSE @,
             !.pageSuspended = IF a = "recovery" THEN FALSE ELSE @,
             !.pendingOffer = IF a = "proposal" THEN None ELSE @,
             !.activeOffer = IF a = "proposal" THEN None ELSE @,
             !.proposalSnapshot = IF a = "proposal" THEN FALSE ELSE @,
             !.proposalReady = IF a = "proposal" THEN FALSE ELSE @,
             !.offerPC = [o \in Offers |->
                 IF a = "proposal" /\ s.offerPC[o] \in {"pending", "active", "authorized"}
                 THEN "cancelled" ELSE s.offerPC[o]]]

ObserveServiceRetirement(a) ==
    /\ s.servicePC[a] = "retired"
    /\ s' = [s EXCEPT !.servicePC[a] = "joined", !.serviceOwners = @ \ {a}]

StopComplete ==
    /\ s.supervisor = "aborting"
    /\ \A j \in Jobs : s.jobPC[j] \in {"fresh", "joined"}
    /\ \A a \in Services : s.servicePC[a] = "joined"
    /\ ~s.metricBorrow
    /\ ~s.receiverOwner
    /\ s' = [s EXCEPT !.supervisor = IF s.parentAlive THEN "stopped" ELSE "quiesced"]

Next ==
    \/ \E j \in Jobs : Enqueue(j) \/ Dispatch(j) \/ DropPayload(j) \/ ReleaseBytes(j)
                       \/ ReleaseIdentity(j) \/ PublishRelease(j) \/ WorkerRetires(j)
                       \/ JoinWorker(j) \/ EarlyBytes(j) \/ EarlyIdentity(j) \/ LoadRecoveryBody(j)
                       \/ SelectCandidate(j)
    \/ \E a \in Services : StartService(a) \/ ServiceRetires(a) \/ ObserveServiceRetirement(a)
    \/ \E r \in Requests : ExternalRequest(r)
    \/ \E o \in Offers : Offer(o)
    \/ \E k \in Captures : GrantCapture(k) \/ CaptureReturns(k) \/ TakeCapture(k)
                           \/ AdvanceCapture(k) \/ CancelCapture(k) \/ CancelTicket(k) \/ DropCapture(k)
    \/ \E error \in BOOLEAN : VisitPass(error) \/ ResolveCandidate(error) \/ Acknowledge(error)
    \/ \E reason \in {"none", "count", "bytes", "yield", "ack", "signal"} : EndpointOperation(reason)
    \/ RejectedReservation \/ BeginPass \/ TakeIntoNext \/ FinishPass \/ EndPass
    \/ DiscardStalePass \/ UpgradeEndpoint \/ DropEndpoint \/ RetryDeadline \/ WakeRecovery
    \/ UpgradeMetricEndpoint \/ DropMetricEndpoint \/ AdmitRecoveryBody \/ DropRecoveryBody
    \/ CloseExternal \/ ObserveInputClosed \/ ReplaceContext
    \/ TakeOffer \/ ReadProposalSnapshot \/ ReleaseProposalSnapshot
    \/ AuthorizeOffer \/ RejectStaleOffer \/ CompleteOffer
    \/ RequestStop \/ CloseInput \/ StopControls \/ RequestAborts \/ StopComplete \/ ParentDrop
    \/ RetireReceiver \/ BeginAttempt \/ PageYield \/ RefuelWithoutSuspension \/ TemporaryRejection

TypeOK ==
    /\ s.supervisor \in {"running", "requested", "closed", "signaled", "aborting", "stopped", "quiesced"}
    /\ s.parentAlive \in BOOLEAN /\ s.metricBorrow \in BOOLEAN
    /\ s.receiverOwner \in BOOLEAN /\ s.dispatched \subseteq Jobs
    /\ s.receiverOpen \in BOOLEAN /\ s.externalSenders \in BOOLEAN /\ s.controlsStopped \in BOOLEAN
    /\ s.jobPC \in [Jobs -> JobStages] /\ s.jobContext \in [Jobs -> 0..2]
    /\ s.handles \subseteq Jobs /\ s.liveJobs \subseteq Jobs /\ s.workerAborts \subseteq Jobs
    /\ s.payloads \subseteq Jobs /\ s.bytes \subseteq Jobs /\ s.identities \subseteq Jobs
    /\ s.armed \subseteq Jobs /\ s.releaseWakes \subseteq Jobs /\ s.rejectedWake \in BOOLEAN
    /\ s.servicePC \in [Services -> ServiceStages] /\ s.serviceOwners \subseteq Services
    /\ s.liveServices \subseteq Services /\ s.serviceAborts \subseteq Services
    /\ s.senderBorrows \subseteq {"recovery", "dispatcher"} /\ s.notice \in BOOLEAN
    /\ s.recoveryBody \in Jobs \cup {None} /\ s.scannerBodies \subseteq Jobs
    /\ s.loadCounts \in [Jobs -> 0..CountCap]
    /\ s.loadDuringPresence \in BOOLEAN /\ s.loadWithoutVisit \in BOOLEAN
    /\ s.proposalRequests \subseteq Requests /\ s.issued \subseteq Requests
    /\ s.shared \in 0..2 /\ s.successor \in 0..2
    /\ s.sharedIDs \subseteq Events /\ s.successorIDs \subseteq Events
    /\ s.passPC \in {"idle", "scanning", "complete"} /\ s.passOrigin \in 0..2
    /\ s.remaining \in 0..PassVisits /\ s.passProposal \in BOOLEAN /\ s.failed \in BOOLEAN
    /\ s.pendingCandidate \in Jobs \cup {None} /\ s.candidateWitness \in Jobs \cup {None}
    /\ s.successorMaintenance \in BOOLEAN
    /\ s.pageAllowance \in 0..PageSize /\ s.pageSpent \in 0..PageSize
    /\ s.pageSuspended \in BOOLEAN /\ s.attempt \in BOOLEAN /\ s.loadPermit \in BOOLEAN
    /\ s.loadWithoutAttempt \in BOOLEAN /\ s.ackPending \in BOOLEAN /\ s.passErrorWitness \in BOOLEAN
    /\ s.context \in 1..2
    /\ s.pendingOffer \in Offers \cup {None} /\ s.activeOffer \in Offers \cup {None}
    /\ s.offerPC \in [Offers -> OfferStages] /\ s.offerOrigin \in [Offers -> 0..2]
    /\ s.authorizedAt \in [Offers -> 0..2]
    /\ s.proposalSnapshot \in BOOLEAN /\ s.proposalReady \in BOOLEAN
    /\ s.capturePC \in [Captures -> CaptureStages] /\ s.captureOrigin \in [Captures -> 0..2]
    /\ s.captureRoots \subseteq Captures /\ s.driverCapture \in Captures \cup {None}
    /\ s.cancelledCaptures \subseteq Captures

Inv_WorkersOwned == s.liveJobs \subseteq s.handles \cup s.workerAborts
Inv_ServicesOwned == s.liveServices \subseteq s.serviceOwners \cup s.serviceAborts
Inv_AbortRetirement == s.supervisor = "aborting" => Inv_WorkersOwned /\ Inv_ServicesOwned
Inv_WorkerLimit == Cardinality(s.handles \cup s.workerAborts) <= WorkerCap
Inv_QueueLimit == Cardinality(Queued(s)) <= CountCap
Inv_PayloadCharged == s.payloads \subseteq s.bytes
Inv_IdentityRetained == s.bytes \subseteq s.identities
Inv_PayloadOwned == s.payloads \subseteq (Queued(s) \cup s.handles \cup s.workerAborts)
Inv_AdmissionOwned == \A j \in s.bytes \cup s.identities :
    IF j \in s.dispatched THEN j \in s.handles \cup s.workerAborts ELSE s.receiverOwner
Inv_ReleaseOrder == s.releaseWakes \cap (s.payloads \cup s.bytes \cup s.identities) = {}
Inv_RejectionCannotSelfWake == ~s.rejectedWake
Inv_NoSenderAcrossWait == s.servicePC["recovery"] = "waiting" => "recovery" \notin s.senderBorrows
Inv_MetricBorrow == s.metricBorrow = ("dispatcher" \in s.senderBorrows)
Inv_ScannerWitness == s.scannerBodies = IF s.recoveryBody = None THEN {} ELSE {s.recoveryBody}
Inv_ScannerOwned == s.recoveryBody # None =>
                    "recovery" \in s.liveServices \cap (s.serviceOwners \cup s.serviceAborts)
Inv_CountBeforeLoad == \A j \in Jobs : s.loadCounts[j] < CountCap
Inv_NoBodyAcrossWait == s.servicePC["recovery"] \in {"release", "waiting"} =>
                       s.scannerBodies = {}
Inv_NoPresenceBypass == ~s.loadDuringPresence
Inv_NoLoadWithoutVisit == ~s.loadWithoutVisit
Inv_CandidateRetained == s.pendingCandidate = s.candidateWitness
Inv_NoPendingCompletion == s.passPC = "complete" => s.candidateWitness = None
Inv_ErrorPreventsProposal == s.passErrorWitness => s.failed
Inv_PageConservation == s.pageSpent + s.pageAllowance = PageSize
Inv_NoLoadWithoutAttempt == ~s.loadWithoutAttempt
Inv_SharedDemand == s.shared = Expected(s, s.sharedIDs)
Inv_SuccessorDemand == s.successor = Merge(Expected(s, s.successorIDs), IF s.successorMaintenance THEN 1 ELSE 0)
Inv_PendingOffer == {o \in Offers : s.offerPC[o] = "pending"} =
                    IF s.pendingOffer = None THEN {} ELSE {s.pendingOffer}
Inv_ActiveOffer == {o \in Offers : s.offerPC[o] \in {"active", "authorized"}} =
                   IF s.activeOffer = None THEN {} ELSE {s.activeOffer}
Inv_ProposalOwner == s.activeOffer # None =>
    "proposal" \in s.liveServices \cap (s.serviceOwners \cup s.serviceAborts)
Inv_AuthorizationWitness == \A o \in Offers : s.authorizedAt[o] # 0 =>
                            s.authorizedAt[o] = s.offerOrigin[o]
Inv_NoSnapshotDuringCallback == s.activeOffer # None /\ s.offerPC[s.activeOffer] = "authorized"
                               => ~s.proposalSnapshot
Inv_CaptureRootWitness == s.captureRoots = {k \in Captures : s.capturePC[k] \notin {"fresh", "retired"}}
Inv_DriverCapture == {k \in Captures : s.capturePC[k] \in DriverCaptureStages} =
                    IF s.driverCapture = None THEN {} ELSE {s.driverCapture}
Inv_DriverCaptureOwned == s.driverCapture # None =>
    "recovery" \in s.liveServices \cap (s.serviceOwners \cup s.serviceAborts)
Inv_PendingCaptureBound ==
    Cardinality({k \in Captures : s.capturePC[k] \in {"capturing", "pending", "external-dropping"}}) <= 1
Inv_StopIsTerminal == s.controlsStopped => s.shared = 0 /\ s.successor = 0
Inv_DispatcherRetired == s.supervisor \in {"stopped", "quiesced"} =>
    /\ ~s.receiverOpen /\ s.controlsStopped
    /\ s.liveJobs = {} /\ s.handles = {} /\ s.liveServices = {} /\ s.serviceOwners = {}
    /\ s.payloads = {} /\ s.bytes = {} /\ s.identities = {} /\ s.senderBorrows = {}
    /\ s.pendingOffer = None /\ s.activeOffer = None /\ ~s.proposalSnapshot
    /\ s.driverCapture = None
    /\ s.scannerBodies = {} /\ s.recoveryBody = None
    /\ ~s.receiverOwner

Safety == TypeOK /\ Inv_AbortRetirement /\ Inv_WorkersOwned /\ Inv_ServicesOwned /\ Inv_WorkerLimit
          /\ Inv_QueueLimit /\ Inv_PayloadCharged /\ Inv_PayloadOwned /\ Inv_ReleaseOrder
          /\ Inv_IdentityRetained /\ Inv_MetricBorrow /\ Inv_ScannerWitness /\ Inv_ScannerOwned
          /\ Inv_CountBeforeLoad /\ Inv_PendingCaptureBound
          /\ Inv_AdmissionOwned /\ Inv_NoBodyAcrossWait /\ Inv_NoPresenceBypass /\ Inv_NoLoadWithoutVisit
          /\ Inv_RejectionCannotSelfWake /\ Inv_NoSenderAcrossWait
          /\ Inv_SharedDemand /\ Inv_SuccessorDemand /\ Inv_PendingOffer /\ Inv_ActiveOffer
          /\ Inv_ProposalOwner /\ Inv_AuthorizationWitness /\ Inv_NoSnapshotDuringCallback
          /\ Inv_CaptureRootWitness /\ Inv_DriverCapture /\ Inv_DriverCaptureOwned
          /\ Inv_StopIsTerminal /\ Inv_DispatcherRetired
          /\ Inv_CandidateRetained /\ Inv_NoPendingCompletion /\ Inv_PageConservation /\ Inv_NoLoadWithoutAttempt
          /\ Inv_ErrorPreventsProposal
ModelSymmetry == Permutations(Jobs) \cup Permutations(Requests) \cup Permutations(Offers) \cup Permutations(Captures)
Spec == Init /\ [][Next]_vars
ControlNext ==
    CASE Defect \in {"DropPendingOnTemporaryRejection", "CompleteWithPending",
                    "RefuelWithoutSuspension", "LoadWithoutAttempt", "ActiveRetryDemand",
                    "IgnoreAcknowledgmentError"} ->
        \/ StartService("recovery")
        \/ \E r \in Requests : ExternalRequest(r)
        \/ \E j \in Jobs : SelectCandidate(j) \/ LoadRecoveryBody(j)
        \/ BeginPass \/ VisitPass(FALSE) \/ FinishPass \/ BeginAttempt
        \/ ResolveCandidate(FALSE) \/ UpgradeEndpoint \/ DropEndpoint
        \/ EndpointOperation("none") \/ EndpointOperation("bytes") \/ EndpointOperation("ack")
        \/ AdmitRecoveryBody \/ Acknowledge(TRUE)
        \/ TemporaryRejection \/ PageYield \/ RetryDeadline \/ WakeRecovery
        \/ RefuelWithoutSuspension
      [] Defect = "WorkerCap" ->
        \E j \in Jobs : Enqueue(j) \/ Dispatch(j) \/ DropPayload(j) \/ ReleaseBytes(j)
                        \/ ReleaseIdentity(j) \/ PublishRelease(j) \/ WorkerRetires(j)
      [] Defect \in {"LostPendingOffer", "StaleAuthorization"} ->
        \/ \E a \in Services : StartService(a)
        \/ \E r \in Requests : ExternalRequest(r)
        \/ BeginPass \/ VisitPass(FALSE) \/ FinishPass \/ PageYield \/ WakeRecovery
        \/ \E o \in Offers : Offer(o)
        \/ TakeOffer \/ ReadProposalSnapshot \/ ReleaseProposalSnapshot
        \/ AuthorizeOffer \/ CompleteOffer
        \/ IF Defect = "StaleAuthorization" THEN ReplaceContext ELSE FALSE
      [] OTHER -> Next
ControlSpec == Init /\ [][ControlNext]_vars
=============================================================================
