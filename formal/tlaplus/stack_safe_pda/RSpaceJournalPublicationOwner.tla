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

VARIABLE state
vars == <<state>>

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

Spec == Init /\ [][Next]_vars
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
=============================================================================
