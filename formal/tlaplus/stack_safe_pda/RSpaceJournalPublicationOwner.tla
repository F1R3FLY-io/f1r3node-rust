---------------- MODULE RSpaceJournalPublicationOwner ----------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANT Fault

Stores == {"local", "foreign"}
Incarnations == 0..1
Epochs == 0..1
Domains == {"data", "continuations", "content", "sources", "joins"}
Full == [domain \in Domains |-> 1]
Empty == [domain \in Domains |-> 0]
Receipt(store, incarnation, epoch) ==
    [store |-> store, profile |-> "selected", incarnation |-> incarnation, epoch |-> epoch,
     base |-> 0, target |-> 1, transaction |-> "this", journal |-> Full]
SelectedReceipts == {Receipt(s, i, e) : s \in Stores, i \in Incarnations, e \in Epochs}
Receipts == SelectedReceipts \union
    {[receipt EXCEPT !.profile = "foreign"] : receipt \in SelectedReceipts}
AckReceipts == Receipts \union
    {[receipt EXCEPT !.base = 1] : receipt \in SelectedReceipts} \union
    {[receipt EXCEPT !.target = 0] : receipt \in SelectedReceipts} \union
    {[receipt EXCEPT !.transaction = "other"] : receipt \in SelectedReceipts} \union
    {[receipt EXCEPT !.journal = Empty] : receipt \in SelectedReceipts}
Terminal(store, version, transaction) ==
    [store |-> store, profile |-> "selected", version |-> version,
     transaction |-> transaction, sequence |-> version, packTail |-> version + 1,
     indexTail |-> version + 1]
ExpectedTerminal(receipt) ==
    [Terminal(receipt.store, receipt.target, receipt.transaction)
     EXCEPT !.profile = receipt.profile]
Old == Terminal("local", 0, "previous")
New == Terminal("local", 1, "this")
Reports == {Old, New, Terminal("local", 1, "other"),
            Terminal("foreign", 1, "this"), Terminal("local", 2, "other"),
            [New EXCEPT !.profile = "foreign"], [New EXCEPT !.sequence = 2],
            [New EXCEPT !.packTail = 3], [New EXCEPT !.indexTail = 3]}
OpenPhases == {"Captured", "Publishing", "RecoveryRequired", "AwaitingAck"}
Phases == OpenPhases \union {"Ready", "Done", "Reopened"}
Views == {"a", "b"}
Roots == {0, 1}
MaxSession == 2
Sessions == 0..MaxSession
OwnerIncarnations == 0..(2 * MaxSession)
Requests == {"checkpoint", "reset", "spawn"}
ViewOutcomes == {"new", "unknown-new", "unknown-old"}
ViewStages == {"idle", "wait", "retake", "run", "publish", "ack", "apply", "done"}
ViewResults == {"none", "refused-permit", "refused-incarnation", "refused-base",
                "uncertain", "acknowledged", "reset", "spawned"}
PendingPhases == {"Captured", "RecoveryRequired", "AwaitingAck"}
Other(view) == CHOOSE other \in Views : other # view
Idle == [stage |-> "idle", op |-> "none", permit |-> "none", root |-> 0, result |-> "none"]
NoPending == [view |-> "none", permit |-> "none", base |-> 0, phase |-> "none", acks |-> 0]
Request(op, permit, root) ==
    [stage |-> "wait", op |-> op, permit |-> permit, root |-> root, result |-> "none"]

VARIABLE state
VARIABLES owner, attachment, viewGate, caller, ownerMutex, pending
viewVars == <<owner, attachment, viewGate, caller, ownerMutex, pending>>
vars == <<state, owner, attachment, viewGate, caller, ownerMutex, pending>>

ViewInit ==
    /\ owner = [incarnation |-> 0, live |-> TRUE, root |-> 0, session |-> 0]
    /\ attachment = [view \in Views |-> [incarnation |-> 0, base |-> 0, session |-> 0]]
    /\ viewGate = [view \in Views |-> "free"]
    /\ caller = [view \in Views |-> Idle]
    /\ ownerMutex = "free"
    /\ pending = NoPending

Bound(receipt) ==
    /\ receipt.store = "local"
    /\ receipt.profile = "selected"
    /\ receipt.incarnation = state.incarnation
    /\ receipt.epoch = state.epoch
    /\ receipt.base = state.hotBase
    /\ receipt.journal = state.journal

ExactAckReplay(receipt) ==
    /\ state.ackCommitted
    /\ receipt = state.receipt
    /\ receipt = state.ackReceipt
    /\ state.ackTerminal = state.expected
    /\ state.admitted = state.expected
    /\ state.durable = state.expected
    /\ state.hotBase = receipt.target
    /\ state.journal = Empty
    /\ state.incarnation = receipt.incarnation
    /\ state.epoch = 1 - receipt.epoch

Init ==
    \E incarnation \in Incarnations, epoch \in Epochs:
    state =
      [phase |-> "Ready", gate |-> FALSE, writer |-> TRUE,
       oldPin |-> TRUE, newPin |-> FALSE,
       incarnation |-> incarnation, epoch |-> epoch, hotBase |-> 0,
       journal |-> Full, receipt |-> Receipt("local", incarnation, epoch),
       durable |-> Old, admitted |-> Old, publications |-> 0,
       acknowledged |-> FALSE, ackReceipt |-> Receipt("local", incarnation, epoch),
       rejectedBinding |-> FALSE, recoveredOld |-> FALSE, recoveredNew |-> FALSE,
       acceptedRecovery |-> FALSE, recoveryReport |-> Old,
       sealed |-> FALSE, expected |-> Old,
       ackCommitted |-> FALSE, ackTerminal |-> Old, ackCommits |-> 0,
       acceptedAck |-> FALSE,
       lastAcceptedAck |-> [receipt |-> Receipt("local", incarnation, epoch), terminal |-> Old],
       aborted |-> FALSE, unwound |-> FALSE, cancelled |-> FALSE,
       unwoundAfterAck |-> FALSE, ackRetriedAfterUnwind |-> FALSE,
       coldLimit |-> "loose", coldEvidence |-> "admitted",
       emptyCheckpointUnderTightLimits |-> FALSE,
       acknowledgedUnderTightLimits |-> FALSE, decoy |-> FALSE]

Capture(receipt) ==
    /\ state.phase = "Ready"
    /\ receipt \in Receipts
    /\ IF Bound(receipt) \/ Fault = "foreign-receipt"
       THEN state' = [state EXCEPT !.phase = "Captured", !.gate = TRUE,
                       !.receipt = receipt, !.acceptedRecovery = FALSE,
                       !.sealed = FALSE, !.expected = Old]
       ELSE state' = [state EXCEPT !.rejectedBinding = TRUE]

Seal ==
    /\ state.phase = "Captured"
    /\ state' = [state EXCEPT !.phase = "Publishing", !.sealed = TRUE,
                  !.expected = ExpectedTerminal(state.receipt)]

AbortCapture ==
    /\ state.phase = "Captured"
    /\ state' = [state EXCEPT !.phase = "Ready", !.gate = FALSE, !.aborted = TRUE]

Publish(outcome) ==
    /\ state.phase = "Publishing"
    /\ outcome \in {"old", "unknown-old", "new", "unknown-new"}
    /\ LET committed == outcome \in {"new", "unknown-new"}
       IN state' = [state EXCEPT
           !.phase = IF outcome = "new" THEN "AwaitingAck" ELSE "RecoveryRequired",
           !.durable = IF committed THEN state.expected ELSE Old,
           !.admitted = IF outcome = "new" THEN state.expected ELSE Old,
           !.publications = IF committed THEN @ + 1 ELSE @,
           !.acceptedRecovery = FALSE,
           !.coldEvidence = IF outcome = "new"
                            THEN IF Fault = "fabricated-cold" THEN "fabricated" ELSE "none"
                            ELSE @]

Recover(report) ==
    /\ state.phase = "RecoveryRequired"
    /\ report \in Reports
    /\ LET exact == report = state.durable /\ report \in {Old, state.expected}
           weak == Fault = "wrong-recovery" /\ report.version = 1
           pathname == Fault = "pathname-recovery" /\ state.decoy /\ report = Old
           withinLimits == state.coldLimit = "loose" \/ Fault = "cold-limit-ignored"
       IN IF (exact \/ weak \/ pathname) /\ withinLimits
          THEN state' = [state EXCEPT
              !.phase = IF report = Old THEN "Ready" ELSE "AwaitingAck",
              !.gate = report # Old,
              !.admitted = report,
              !.acceptedRecovery = TRUE,
              !.recoveryReport = report,
              !.recoveredOld = @ \/ report = Old,
              !.recoveredNew = @ \/ report = New,
              !.coldEvidence = IF state.coldLimit = "loose" THEN "admitted" ELSE "exceeds-limits"]
          ELSE state' = [state EXCEPT !.acceptedRecovery = FALSE,
                          !.recoveryReport = report]

PinSuccessor ==
    /\ state.phase = "AwaitingAck"
    /\ ~state.newPin
    /\ state' = [state EXCEPT !.newPin = TRUE]

Acknowledge(receipt, terminal) ==
    /\ state.phase = "AwaitingAck"
    /\ state.newPin
    /\ receipt \in AckReceipts
    /\ terminal \in Reports
    /\ IF state.ackCommitted
       THEN IF (ExactAckReplay(receipt) /\ terminal = state.ackTerminal)
               \/ Fault = "wrong-ack-input"
            THEN state' = [state EXCEPT
                 !.ackRetriedAfterUnwind = @ \/ state.unwoundAfterAck,
                 !.acceptedAck = TRUE,
                 !.lastAcceptedAck = [receipt |-> receipt, terminal |-> terminal],
                 !.ackCommits = IF Fault = "double-ack" THEN @ + 1 ELSE @,
                 !.acknowledgedUnderTightLimits = @ \/ state.coldLimit = "tight"]
            ELSE state' = [state EXCEPT !.rejectedBinding = TRUE]
       ELSE IF receipt = state.receipt /\ Bound(receipt) /\ state.sealed
               /\ state.admitted = state.expected /\ state.durable = state.expected
               /\ terminal = state.expected
            THEN state' = [state EXCEPT !.hotBase = receipt.target,
                 !.journal = Empty, !.epoch = 1 - @, !.ackCommitted = TRUE,
                 !.ackReceipt = receipt, !.ackTerminal = state.expected, !.ackCommits = @ + 1,
                 !.acceptedAck = TRUE,
                 !.lastAcceptedAck = [receipt |-> receipt, terminal |-> terminal],
                 !.acknowledgedUnderTightLimits = @ \/ state.coldLimit = "tight"]
            ELSE state' = [state EXCEPT !.rejectedBinding = TRUE]

TightenColdLimits ==
    /\ state.coldLimit = "loose"
    /\ state' = [state EXCEPT !.coldLimit = "tight"]

EmptyCheckpoint ==
    /\ state.phase \in {"Ready", "Done", "Reopened"}
    /\ state.journal = Empty
    /\ state' = [state EXCEPT
        !.emptyCheckpointUnderTightLimits = @ \/ state.coldLimit = "tight"]

FinishAcknowledgement ==
    /\ state.phase = "AwaitingAck"
    /\ ExactAckReplay(state.receipt)
    /\ state' = [state EXCEPT !.phase = "Done", !.acknowledged = TRUE,
                  !.gate = FALSE, !.oldPin = FALSE]

Unwind ==
    /\ state.phase \in OpenPhases
    /\ state' = [state EXCEPT !.phase = "RecoveryRequired", !.unwound = TRUE,
                  !.unwoundAfterAck = @ \/ state.ackCommitted,
                  !.gate = IF Fault = "unwind-gate-release" THEN FALSE ELSE @]

CancelCall ==
    /\ state.phase \in OpenPhases
    /\ state' = [state EXCEPT !.cancelled = TRUE,
                  !.gate = IF Fault = "cancel-gate-release" THEN FALSE ELSE @]

PlaceDecoy ==
    /\ state.phase \in OpenPhases
    /\ ~state.decoy
    /\ state' = [state EXCEPT !.decoy = TRUE]

RemoveDecoy ==
    /\ state.phase \in OpenPhases
    /\ state.decoy
    /\ state' = [state EXCEPT !.decoy = FALSE]

CrashAndReopen ==
    /\ state.phase \in OpenPhases \union {"Done"}
    /\ ~state.decoy
    /\ state' = [state EXCEPT !.phase = "Reopened", !.gate = FALSE,
        !.hotBase = state.durable.version,
        !.journal = IF Fault = "journal-replay" THEN @ ELSE Empty,
        !.incarnation = 1 - @, !.epoch = 0,
        !.admitted = state.durable,
        !.ackCommitted = FALSE,
        !.oldPin = state.durable = Old, !.newPin = state.durable = New]

LoseAcknowledgementMarker ==
    /\ Fault = "lost-ack-marker"
    /\ state.phase = "AwaitingAck" /\ state.ackCommitted
    /\ state' = [state EXCEPT !.ackCommitted = FALSE]

CorruptAcknowledgementMarker ==
    /\ Fault = "ack-marker-corruption"
    /\ state.phase = "AwaitingAck" /\ state.ackCommitted
    /\ state' = [state EXCEPT !.ackReceipt =
                  Receipt("foreign", state.receipt.incarnation, state.receipt.epoch)]

WrongPreparedWitness ==
    /\ Fault = "wrong-prepared-witness"
    /\ state.phase = "Publishing"
    /\ state' = [state EXCEPT !.expected.sequence = 2]

EarlyAck ==
    /\ Fault = "early-ack"
    /\ state.phase = "Captured"
    /\ state' = [state EXCEPT !.phase = "Done", !.acknowledged = TRUE,
                  !.journal = Empty, !.gate = FALSE]

PartialClear ==
    /\ Fault = "partial-clear"
    /\ state.phase = "AwaitingAck"
    /\ state' = [state EXCEPT !.journal["data"] = 0]

LosePin ==
    /\ Fault = "lease-gap"
    /\ state.phase = "Publishing"
    /\ state' = [state EXCEPT !.oldPin = FALSE]

UnresolvedRebase ==
    /\ Fault = "unresolved-rebase"
    /\ state.phase = "AwaitingAck"
    /\ state' = [state EXCEPT !.phase = "Ready", !.gate = FALSE]

DuplicatePublish ==
    /\ Fault = "double-publish"
    /\ state.phase = "AwaitingAck"
    /\ state' = [state EXCEPT !.publications = @ + 1]

MutationDuringGate ==
    /\ Fault = "gate-release"
    /\ state.phase \in OpenPhases
    /\ state' = [state EXCEPT !.gate = FALSE]

Next ==
    \/ \E receipt \in Receipts: Capture(receipt)
    \/ Seal \/ AbortCapture
    \/ \E outcome \in {"old", "unknown-old", "new", "unknown-new"}: Publish(outcome)
    \/ \E report \in Reports: Recover(report)
    \/ PinSuccessor
    \/ \E receipt \in AckReceipts, terminal \in Reports: Acknowledge(receipt, terminal)
    \/ FinishAcknowledgement \/ Unwind \/ CancelCall
    \/ TightenColdLimits \/ EmptyCheckpoint
    \/ PlaceDecoy \/ RemoveDecoy
    \/ CrashAndReopen
    \/ EarlyAck \/ PartialClear \/ LosePin \/ UnresolvedRebase
    \/ DuplicatePublish \/ MutationDuringGate
    \/ LoseAcknowledgementMarker \/ CorruptAcknowledgementMarker \/ WrongPreparedWitness

Spec == Init /\ ViewInit /\ [][Next /\ UNCHANGED viewVars]_vars
TypeInvariant ==
    /\ state.phase \in Phases
    /\ state.incarnation \in Incarnations
    /\ state.epoch \in Epochs
    /\ state.hotBase \in 0..1
    /\ state.journal \in [Domains -> 0..1]
    /\ state.receipt \in Receipts
    /\ state.durable \in {Old, New}
    /\ state.admitted \in Reports
    /\ state.publications \in Nat
    /\ state.ackCommits \in Nat
    /\ state.ackCommitted \in BOOLEAN
    /\ state.sealed \in BOOLEAN
    /\ state.expected \in Reports
    /\ state.acceptedAck \in BOOLEAN
    /\ state.lastAcceptedAck \in [receipt : AckReceipts, terminal : Reports]
    /\ state.coldLimit \in {"loose", "tight"}
    /\ state.coldEvidence \in {"none", "admitted", "exceeds-limits", "fabricated"}
    /\ state.emptyCheckpointUnderTightLimits \in BOOLEAN
    /\ state.acknowledgedUnderTightLimits \in BOOLEAN
    /\ state.decoy \in BOOLEAN
GateHeldUntilResolution == state.phase \in OpenPhases => state.gate
ContinuousAuthority == state.writer /\ (state.oldPin \/ state.newPin)
ExactPendingReceipt ==
    state.phase \in OpenPhases =>
        IF state.ackCommitted THEN ExactAckReplay(state.receipt) ELSE Bound(state.receipt)
WholeJournalAcknowledgement == state.journal \in {Full, Empty}
AcknowledgementRequiresExactSuccessor ==
    state.acknowledged \/ state.ackCommitted =>
        state.durable = New /\ state.admitted = New /\ state.newPin
AcknowledgementRequiresExactReceipt ==
    state.acknowledged \/ state.ackCommitted => state.ackReceipt = state.receipt
ReadyHasCoherentBase ==
    state.phase \in {"Ready", "Done", "Reopened"} =>
        state.hotBase = state.admitted.version /\ state.admitted = state.durable
RecoveryAcceptsOnlyExactTerminal ==
    state.acceptedRecovery => state.recoveryReport = state.durable
        /\ state.recoveryReport \in {Old, state.expected}
AtMostOnePublication == state.publications <= 1
PreparedWitnessBound == state.sealed => state.expected = ExpectedTerminal(state.receipt)
AtMostOneAcknowledgement == state.ackCommits <= 1
ClearedPendingJournalHasExactMarker ==
    state.phase \in OpenPhases /\ state.journal = Empty => ExactAckReplay(state.receipt)
AcknowledgementAcceptsOnlyExactInput ==
    state.acceptedAck => state.lastAcceptedAck.receipt = state.ackReceipt
        /\ state.lastAcceptedAck.terminal = state.ackTerminal
ReopenDiscardsVolatileJournal ==
    state.phase = "Reopened" => state.journal = Empty /\ state.hotBase = state.durable.version
ColdRecoveryHonorsIncomingLimits ==
    state.acceptedRecovery => state.coldEvidence = "admitted"
TrustedSuccessorHasNoColdEvidence ==
    state.phase = "AwaitingAck" /\ ~state.acceptedRecovery => state.coldEvidence = "none"
NeverAcknowledged == ~state.acknowledged
NeverRecoveredOld == ~state.recoveredOld
NeverRecoveredNew == ~state.recoveredNew
NeverRejectedBinding == ~state.rejectedBinding
NeverAbortedCapture == ~state.aborted
NeverUnwound == ~state.unwound
NeverCancelled == ~state.cancelled
NeverRetriedAcknowledgementAfterUnwind == ~state.ackRetriedAfterUnwind
NeverEmptyCheckpointUnderTightenedColdLimits == ~state.emptyCheckpointUnderTightLimits
NeverAwaitingAckRetryUnderTightenedColdLimits == ~state.acknowledgedUnderTightLimits
NeverReconciledUnderDecoy == ~(state.decoy /\ state.acceptedRecovery)

FrozenState == Init /\ state.incarnation = 0 /\ state.epoch = 0
Quiescent ==
    /\ ownerMutex = "free"
    /\ pending = NoPending
    /\ \A view \in Views: caller[view].stage = "idle"

CallCheckpoint(view, permit) ==
    /\ caller[view].stage = "idle"
    /\ viewGate[permit] = "free"
    /\ viewGate' = [viewGate EXCEPT ![permit] = view]
    /\ caller' = [caller EXCEPT ![view] = Request("checkpoint", permit, 0)]
    /\ UNCHANGED <<owner, attachment, ownerMutex, pending>>

CallReset(view, target) ==
    /\ caller[view].stage = "idle"
    /\ viewGate[view] = "free"
    /\ viewGate' = [viewGate EXCEPT ![view] = view]
    /\ caller' = [caller EXCEPT ![view] = Request("reset", view, target)]
    /\ UNCHANGED <<owner, attachment, ownerMutex, pending>>

CallSpawn(view) ==
    /\ caller[view].stage = "idle"
    /\ caller[Other(view)].stage = "idle"
    /\ \A gate \in Views: viewGate[gate] = "free"
    /\ viewGate' = [gate \in Views |-> view]
    /\ caller' = [caller EXCEPT ![view] = Request("spawn", view, 0)]
    /\ UNCHANGED <<owner, attachment, ownerMutex, pending>>

TakeMutex(view) ==
    /\ caller[view].stage = "wait"
    /\ ownerMutex = "free"
    /\ ownerMutex' = view
    /\ caller' = [caller EXCEPT ![view].stage =
           IF Fault = "retake-gate" /\ caller[view].op = "checkpoint" THEN "retake" ELSE "run"]
    /\ UNCHANGED <<owner, attachment, viewGate, pending>>

OwnerAcquiresGate(view) ==
    /\ ownerMutex = view
    /\ caller[view].stage = "retake"
    /\ viewGate[view] = "free"
    /\ caller' = [caller EXCEPT ![view].stage = "run"]
    /\ UNCHANGED <<owner, attachment, viewGate, ownerMutex, pending>>

Resolve ==
    \/ /\ pending.phase = "RecoveryRequired"
       /\ owner.root = pending.base
       /\ viewGate' = [viewGate EXCEPT ![pending.permit] = "free"]
       /\ pending' = NoPending
       /\ UNCHANGED attachment
    \/ /\ pending.phase = "RecoveryRequired"
       /\ owner.root # pending.base
       /\ pending' = [pending EXCEPT !.phase = "AwaitingAck"]
       /\ UNCHANGED <<attachment, viewGate>>
    \/ /\ pending.phase = "AwaitingAck"
       /\ attachment' = IF pending.acks = 0
                        THEN [attachment EXCEPT ![pending.view].base = owner.root]
                        ELSE attachment
       /\ viewGate' = [viewGate EXCEPT ![pending.permit] = "free"]
       /\ pending' = NoPending

ReconcileFirst(view) ==
    /\ ownerMutex = view
    /\ caller[view].stage = "run"
    /\ pending # NoPending
    /\ Resolve
    /\ UNCHANGED <<owner, caller, ownerMutex>>

ReconcilePending ==
    /\ ownerMutex = "free"
    /\ pending.phase \in {"RecoveryRequired", "AwaitingAck"}
    /\ Resolve
    /\ UNCHANGED <<owner, caller, ownerMutex>>

Refuse(view, result) ==
    /\ caller' = [caller EXCEPT ![view].stage = "done", ![view].result = result]
    /\ UNCHANGED <<owner, attachment, viewGate, ownerMutex, pending>>

CheckRequest(view) ==
    /\ ownerMutex = view
    /\ caller[view].stage = "run"
    /\ pending = NoPending
    /\ LET request == caller[view]
           permitOk == request.permit = view \/ Fault = "foreign-permit"
           incarnationOk == attachment[view].incarnation = owner.incarnation
           baseOk == attachment[view].base = owner.root \/ Fault = "skip-base"
       IN  IF request.op = "checkpoint" /\ ~permitOk THEN Refuse(view, "refused-permit")
           ELSE IF ~incarnationOk THEN Refuse(view, "refused-incarnation")
           ELSE IF request.op = "checkpoint" /\ ~baseOk THEN Refuse(view, "refused-base")
           ELSE IF request.op = "checkpoint"
           THEN /\ pending' = [view |-> view, permit |-> request.permit,
                               base |-> attachment[view].base, phase |-> "Captured",
                               acks |-> 0]
                /\ viewGate' = [viewGate EXCEPT ![request.permit] = "pending"]
                /\ caller' = [caller EXCEPT ![view].stage = "publish"]
                /\ UNCHANGED <<owner, attachment, ownerMutex>>
           ELSE /\ caller' = [caller EXCEPT ![view].stage = "apply"]
                /\ UNCHANGED <<owner, attachment, viewGate, ownerMutex, pending>>

PublishView(view, outcome) ==
    /\ ownerMutex = view
    /\ caller[view].stage = "publish"
    /\ pending.phase = "Captured"
    /\ owner' = IF outcome = "unknown-old" THEN owner ELSE [owner EXCEPT !.root = 1 - @]
    /\ IF outcome = "new"
       THEN /\ pending' = [pending EXCEPT !.phase = "AwaitingAck"]
            /\ caller' = [caller EXCEPT ![view].stage = "ack"]
       ELSE /\ pending' = [pending EXCEPT !.phase = "RecoveryRequired"]
            /\ caller' = [caller EXCEPT ![view].stage = "done", ![view].result = "uncertain"]
    /\ UNCHANGED <<attachment, viewGate, ownerMutex>>

AcknowledgeView(view, completes) ==
    /\ ownerMutex = view
    /\ caller[view].stage = "ack"
    /\ pending.phase = "AwaitingAck"
    /\ pending.acks = 0
    /\ attachment' = [attachment EXCEPT ![pending.view].base = owner.root]
    /\ IF completes
       THEN /\ viewGate' = [viewGate EXCEPT ![pending.permit] = "free"]
            /\ pending' = NoPending
            /\ caller' = [caller EXCEPT ![view].stage = "done", ![view].result = "acknowledged"]
       ELSE /\ pending' = [pending EXCEPT !.acks = 1]
            /\ caller' = [caller EXCEPT ![view].stage = "done", ![view].result = "uncertain"]
            /\ UNCHANGED viewGate
    /\ UNCHANGED <<owner, ownerMutex>>

ApplyActivation(view) ==
    /\ ownerMutex = view
    /\ caller[view].stage = "apply"
    /\ IF caller[view].op = "reset"
       THEN /\ owner' = [owner EXCEPT !.root = caller[view].root]
            /\ attachment' = [attachment EXCEPT ![view] =
                   [incarnation |-> owner.incarnation, base |-> caller[view].root,
                    session |-> owner.session]]
            /\ caller' = [caller EXCEPT ![view].stage = "done", ![view].result = "reset"]
       ELSE /\ owner' = [owner EXCEPT !.root = attachment[view].base]
            /\ attachment' = [attachment EXCEPT ![Other(view)] =
                   [incarnation |-> owner.incarnation, base |-> attachment[view].base,
                    session |-> owner.session]]
            /\ caller' = [caller EXCEPT ![view].stage = "done", ![view].result = "spawned"]
    /\ UNCHANGED <<viewGate, ownerMutex, pending>>

ReturnCall(view) ==
    /\ ownerMutex = view
    /\ caller[view].stage = "done"
    /\ ownerMutex' = "free"
    /\ viewGate' = [gate \in Views |-> IF viewGate[gate] = view THEN "free" ELSE viewGate[gate]]
    /\ caller' = [caller EXCEPT ![view] = Idle]
    /\ UNCHANGED <<owner, attachment, pending>>

CloseOwner ==
    /\ owner.live
    /\ owner.session < MaxSession
    /\ Quiescent
    /\ owner' = [owner EXCEPT !.live = FALSE,
                 !.incarnation = IF Fault = "close-no-bump" THEN @ ELSE @ + 1]
    /\ UNCHANGED <<attachment, viewGate, caller, ownerMutex, pending>>

FreshIncarnation(incarnation) ==
    IF Fault = "reopen-reuses-incarnation"
    THEN incarnation # owner.incarnation
    ELSE incarnation = owner.incarnation + 1

ReopenOwner(view, incarnation) ==
    /\ ~owner.live
    /\ Quiescent
    /\ FreshIncarnation(incarnation)
    /\ owner' = [owner EXCEPT !.live = TRUE, !.incarnation = incarnation, !.session = @ + 1]
    /\ attachment' = [attachment EXCEPT ![view] =
           [incarnation |-> incarnation, base |-> owner.root, session |-> owner.session + 1]]
    /\ UNCHANGED <<viewGate, caller, ownerMutex, pending>>

OwnerAction ==
    \E view \in Views:
        \/ TakeMutex(view)
        \/ OwnerAcquiresGate(view)
        \/ ReconcileFirst(view)
        \/ CheckRequest(view)
        \/ \E outcome \in ViewOutcomes: PublishView(view, outcome)
        \/ \E completes \in BOOLEAN: AcknowledgeView(view, completes)
        \/ ApplyActivation(view)
        \/ ReturnCall(view)

ViewNext ==
    /\ \/ \E view \in Views, permit \in Views: CallCheckpoint(view, permit)
       \/ \E view \in Views, target \in Roots: CallReset(view, target)
       \/ \E view \in Views: CallSpawn(view)
       \/ OwnerAction
       \/ ReconcilePending
       \/ CloseOwner
       \/ \E view \in Views, incarnation \in OwnerIncarnations: ReopenOwner(view, incarnation)
    /\ UNCHANGED state

ViewSpec ==
    /\ FrozenState
    /\ ViewInit
    /\ [][ViewNext]_vars
    /\ WF_vars(OwnerAction /\ UNCHANGED state)
    /\ WF_vars(ReconcilePending /\ UNCHANGED state)

ViewTypeInvariant ==
    /\ owner \in [incarnation : OwnerIncarnations, live : BOOLEAN, root : Roots,
                  session : Sessions]
    /\ attachment \in [Views -> [incarnation : OwnerIncarnations, base : Roots,
                                 session : Sessions]]
    /\ viewGate \in [Views -> Views \union {"free", "pending"}]
    /\ ownerMutex \in Views \union {"free"}
    /\ caller \in [Views -> [stage : ViewStages, op : Requests \union {"none"},
                             permit : Views \union {"none"}, root : Roots,
                             result : ViewResults]]
    /\ \/ pending = NoPending
       \/ pending \in [view : Views, permit : Views, base : Roots, phase : PendingPhases,
                       acks : 0..1]
ExactBaseOrRefuse ==
    /\ pending.phase = "Captured" => pending.base = owner.root
    /\ \A view \in Views:
           caller[view].result = "refused-base" => attachment[view].base # owner.root
NoViewMutationWhilePending ==
    pending # NoPending => viewGate[pending.view] = "pending"
AckOnce ==
    /\ pending.acks <= 1
    /\ pending.acks = 1 =>
           /\ pending.phase = "AwaitingAck"
           /\ attachment[pending.view].base = owner.root
StaleIncarnationRefused ==
    \A view \in Views:
        (ownerMutex = view /\ caller[view].stage \in {"publish", "ack", "apply"})
            => owner.live /\ attachment[view].session = owner.session
PermitProvenance ==
    pending # NoPending => pending.permit = pending.view
LockOrder ==
    \A view \in Views: ~(ownerMutex = view /\ caller[view].stage = "retake")
SpawnSharesOwner ==
    \A view \in Views:
        caller[view].result = "spawned" =>
            /\ owner.incarnation = attachment[view].incarnation
            /\ attachment[Other(view)].incarnation = attachment[view].incarnation
            /\ attachment[Other(view)].session = attachment[view].session
            /\ attachment[Other(view)].base = attachment[view].base
            /\ owner.root = attachment[view].base
PendingResolves == pending # NoPending ~> pending = NoPending
NeverRefusedStaleBase == \A view \in Views: caller[view].result # "refused-base"
NeverRefusedRetainedIncarnation ==
    ~\E view \in Views:
        /\ caller[view].result = "refused-incarnation"
        /\ owner.live
        /\ attachment[view].session < owner.session
NeverReconciledByOtherView ==
    ~\E view \in Views:
        /\ ownerMutex = view
        /\ caller[view].stage = "run"
        /\ pending # NoPending
        /\ pending.view # view
=============================================================================
