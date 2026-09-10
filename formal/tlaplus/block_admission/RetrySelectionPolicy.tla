------------------------- MODULE RetrySelectionPolicy -------------------------
EXTENDS Integers, FiniteSets, TLC

CONSTANTS OperationCount, PeerCount, Persistent, Unsafe
Operations == 1..OperationCount
Now == 10
Cooldown == 2
Kinds == {"none", "peer_request", "peer_requery", "broadcast_only"}

VARIABLES epoch, revision, policy, stored, expected, initial, received, operations,
          actionMetric, suppressionMetric, completionMetric
vars == <<epoch, revision, policy, stored, expected, initial, received, operations,
          actionMetric, suppressionMetric, completionMetric>>

NextCursor(p) == IF PeerCount = 0 THEN p.cursor ELSE (p.cursor + 1) % PeerCount
Kind(p) == IF received THEN "none"
           ELSE IF p.waiting > 0 THEN "peer_request"
           ELSE IF PeerCount > 0 /\ p.peers < PeerCount THEN "peer_requery"
           ELSE "broadcast_only"
Suppressed(p) == CASE Kind(p) = "peer_requery" -> Now - p.peerClock < Cooldown
                  [] Kind(p) = "broadcast_only" -> Now - p.broadcastClock < Cooldown
                  [] OTHER -> FALSE
Reference(p) ==
    IF Kind(p) = "none" THEN p
    ELSE [p EXCEPT !.timestamp = Now,
          !.cursor = IF p.waiting > 0 THEN p.cursor ELSE NextCursor(p),
          !.waiting = IF p.waiting > 0 THEN p.waiting - 1 ELSE 0,
          !.peerClock = IF Kind(p) = "peer_requery" /\ ~Suppressed(p) THEN Now ELSE @,
          !.broadcastClock = IF Kind(p) = "broadcast_only" /\ ~Suppressed(p) THEN Now ELSE @]
Candidate(p) ==
    IF Unsafe = "drop-suppressed" /\ Suppressed(p) THEN p
    ELSE IF Unsafe = "skip-budget-cursor" /\ p.waiting = 0 /\ p.peers >= PeerCount
         THEN [Reference(p) EXCEPT !.cursor = p.cursor]
    ELSE Reference(p)
Due(age, expiry) == age > 1
CandidateDue(age, expiry) == IF Unsafe = "expiry-clock" THEN age > expiry
                            ELSE IF Unsafe = "inclusive-clock" THEN age >= 1
                            ELSE age > 1
ClockEquivalent == \A age \in 0..4, expiry \in 1..4 : CandidateDue(age, expiry) = Due(age, expiry)

EmptyOperation == [stage |-> "idle", epoch |-> 0, revision |-> 0, snapshot |-> initial,
                   candidate |-> initial, kind |-> "none", suppressed |-> FALSE,
                   published |-> FALSE, finished |-> FALSE, permit |-> FALSE, freshAtPublish |-> TRUE]

Init ==
    /\ epoch = 1 /\ revision = 0 /\ received \in BOOLEAN
    /\ initial \in {[timestamp |-> 0, cursor |-> 0, waiting |-> w, total |-> used,
                     peers |-> used, peerClock |-> pc, broadcastClock |-> bc] :
                     w \in 0..1, used \in {0, PeerCount}, pc \in {0, Now}, bc \in {0, Now}}
    /\ policy = initial /\ stored = initial /\ expected = initial
    /\ operations = [p \in Operations |-> EmptyOperation]
    /\ actionMetric = 0 /\ suppressionMetric = 0 /\ completionMetric = 0

Prepare(p) ==
    /\ operations[p].stage = "idle"
    /\ operations' = [operations EXCEPT ![p] =
         [stage |-> "prepared", epoch |-> epoch, revision |-> revision, snapshot |-> policy,
          candidate |-> Candidate(policy), kind |-> Kind(policy), suppressed |-> Suppressed(policy),
          published |-> FALSE, finished |-> FALSE, permit |-> FALSE, freshAtPublish |-> TRUE]]
    /\ UNCHANGED <<epoch, revision, policy, stored, expected, initial, received,
                    actionMetric, suppressionMetric, completionMetric>>

Publish(p, writeOK) ==
    /\ operations[p].stage = "prepared"
    /\ LET op == operations[p]
           current == (op.epoch = epoch \/ Unsafe = "stale-epoch")
                      /\ (op.revision = revision \/ Unsafe = "stale-revision")
           publish == current /\ writeOK /\ op.kind # "none"
           corrupt == current /\ ~writeOK /\ Persistent /\ Unsafe = "partial-failure"
           dispatch == publish /\ ~op.suppressed
       IN
       /\ policy' = IF publish \/ corrupt THEN op.candidate ELSE policy
       /\ expected' = IF publish THEN Reference(expected) ELSE expected
       /\ stored' = IF publish /\ Persistent THEN op.candidate ELSE stored
       /\ revision' = IF publish THEN revision + 1 ELSE revision
       /\ operations' = [operations EXCEPT ![p].stage = IF dispatch THEN "sending" ELSE "done",
            ![p].published = publish,
            ![p].freshAtPublish = ~publish \/ (op.epoch = epoch /\ op.revision = revision),
            ![p].permit = dispatch \/ (publish /\ op.suppressed /\ Unsafe = "suppressed-permit")]
       /\ actionMetric' = actionMetric + IF publish /\ Unsafe # "late-action-metric" THEN 1 ELSE 0
       /\ suppressionMetric' = suppressionMetric + IF publish /\ op.suppressed THEN 1 ELSE 0
    /\ UNCHANGED <<epoch, initial, received, completionMetric>>

Complete(p, returnedOK) ==
    /\ operations[p].stage = "sending"
    /\ LET op == operations[p]
           current == op.epoch = epoch
       IN
       /\ policy' = IF current THEN [policy EXCEPT !.total = @ + 1,
                        !.peers = @ + IF op.kind = "peer_requery" THEN 1 ELSE 0] ELSE policy
       /\ expected' = IF current THEN [expected EXCEPT !.total = @ + 1,
                          !.peers = @ + IF op.kind = "peer_requery" THEN 1 ELSE 0] ELSE expected
       /\ revision' = IF current THEN revision + 1 ELSE revision
       /\ operations' = [operations EXCEPT ![p].stage = "observed", ![p].finished = TRUE]
    /\ completionMetric' = completionMetric + 1
    /\ UNCHANGED <<epoch, stored, initial, received, actionMetric, suppressionMetric>>

Persist(p, writeOK) ==
    /\ operations[p].stage = "observed"
    /\ stored' = IF writeOK /\ operations[p].epoch = epoch /\ Persistent THEN policy ELSE stored
    /\ operations' = IF writeOK THEN [operations EXCEPT ![p].stage = "done", ![p].permit = FALSE] ELSE operations
    /\ completionMetric' = completionMetric + IF Unsafe = "duplicate-completion-metric" THEN 1 ELSE 0
    /\ UNCHANGED <<epoch, revision, policy, expected, initial, received, actionMetric, suppressionMetric>>

Cancel(p) ==
    /\ operations[p].stage = "sending"
    /\ operations' = [operations EXCEPT ![p].stage = "done", ![p].permit = FALSE]
    /\ UNCHANGED <<epoch, revision, policy, stored, expected, initial, received,
                    actionMetric, suppressionMetric, completionMetric>>

Replace ==
    /\ epoch = 1 /\ epoch' = 2 /\ revision' = 0
    /\ policy' = initial /\ stored' = initial /\ expected' = initial
    /\ UNCHANGED <<initial, received, operations, actionMetric, suppressionMetric, completionMetric>>

Next == Replace \/ (\E p \in Operations : Prepare(p) \/ Cancel(p)
              \/ (\E result \in BOOLEAN : Publish(p, result) \/ Complete(p, result) \/ Persist(p, result)))

Inv_ClockEquivalent == epoch \in {1, 2} /\ ClockEquivalent
Inv_NoStalePublication == \A p \in Operations : operations[p].published => operations[p].freshAtPublish
Inv_PolicyCorrespondence == policy = expected
Inv_PairedControl == ~Persistent \/
    <<policy.timestamp, policy.cursor, policy.waiting, policy.peerClock, policy.broadcastClock>> =
    <<stored.timestamp, stored.cursor, stored.waiting, stored.peerClock, stored.broadcastClock>>
Inv_OperationOwnership == \A p \in Operations : operations[p].permit = (operations[p].stage \in {"sending", "observed"})
Inv_MetricBoundary ==
    /\ actionMetric = Cardinality({p \in Operations : operations[p].published})
    /\ suppressionMetric = Cardinality({p \in Operations : operations[p].published /\ operations[p].suppressed})
    /\ completionMetric = Cardinality({p \in Operations : operations[p].finished})

Spec == Init /\ [][Next]_vars
=============================================================================
