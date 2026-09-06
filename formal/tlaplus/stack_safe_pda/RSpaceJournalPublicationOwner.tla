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
Terminal(store, version, transaction) ==
    [store |-> store, profile |-> "selected", version |-> version, transaction |-> transaction]
Old == Terminal("local", 0, "previous")
New == Terminal("local", 1, "this")
Reports == {Old, New, Terminal("local", 1, "other"),
            Terminal("foreign", 1, "this"), Terminal("local", 2, "other"),
            [New EXCEPT !.profile = "foreign"]}
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
       acceptedRecovery |-> FALSE, recoveryReport |-> Old]

Capture(receipt) ==
    /\ state.phase = "Ready"
    /\ receipt \in Receipts
    /\ IF Bound(receipt) \/ Fault = "foreign-receipt"
       THEN state' = [state EXCEPT !.phase = "Captured", !.gate = TRUE,
                       !.receipt = receipt, !.acceptedRecovery = FALSE]
       ELSE state' = [state EXCEPT !.rejectedBinding = TRUE]

Seal ==
    /\ state.phase = "Captured"
    /\ state' = [state EXCEPT !.phase = "Publishing"]

Publish(outcome) ==
    /\ state.phase = "Publishing"
    /\ outcome \in {"old", "unknown-old", "new", "unknown-new"}
    /\ LET committed == outcome \in {"new", "unknown-new"}
       IN state' = [state EXCEPT
           !.phase = IF outcome = "new" THEN "AwaitingAck" ELSE "RecoveryRequired",
           !.durable = IF committed THEN New ELSE Old,
           !.admitted = IF outcome = "new" THEN New ELSE Old,
           !.publications = IF committed THEN @ + 1 ELSE @,
           !.acceptedRecovery = FALSE]

Recover(report) ==
    /\ state.phase = "RecoveryRequired"
    /\ report \in Reports
    /\ LET exact == report = state.durable /\ report \in {Old, New}
           weak == Fault = "wrong-recovery" /\ report.version = 1
       IN IF exact \/ weak
          THEN state' = [state EXCEPT
              !.phase = IF report = Old THEN "Ready" ELSE "AwaitingAck",
              !.gate = report # Old,
              !.admitted = report,
              !.acceptedRecovery = TRUE,
              !.recoveryReport = report,
              !.recoveredOld = @ \/ report = Old,
              !.recoveredNew = @ \/ report = New]
          ELSE state' = [state EXCEPT !.acceptedRecovery = FALSE,
                          !.recoveryReport = report]

PinSuccessor ==
    /\ state.phase = "AwaitingAck"
    /\ ~state.newPin
    /\ state' = [state EXCEPT !.newPin = TRUE]

Acknowledge(receipt) ==
    /\ state.phase = "AwaitingAck"
    /\ state.newPin
    /\ receipt \in Receipts
    /\ IF receipt = state.receipt /\ Bound(receipt) /\ state.admitted = New
       THEN state' = [state EXCEPT !.phase = "Done", !.hotBase = 1,
                       !.journal = Empty, !.acknowledged = TRUE,
                       !.ackReceipt = receipt, !.gate = FALSE, !.oldPin = FALSE]
       ELSE state' = [state EXCEPT !.rejectedBinding = TRUE]

CrashAndReopen ==
    /\ state.phase \in OpenPhases \union {"Done"}
    /\ state' = [state EXCEPT !.phase = "Reopened", !.gate = FALSE,
        !.hotBase = state.durable.version, !.journal = Empty,
        !.incarnation = 1 - @, !.epoch = 0,
        !.admitted = state.durable,
        !.oldPin = state.durable = Old, !.newPin = state.durable = New]

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
    \/ Seal
    \/ \E outcome \in {"old", "unknown-old", "new", "unknown-new"}: Publish(outcome)
    \/ \E report \in Reports: Recover(report)
    \/ PinSuccessor
    \/ \E receipt \in Receipts: Acknowledge(receipt)
    \/ CrashAndReopen
    \/ EarlyAck \/ PartialClear \/ LosePin \/ UnresolvedRebase
    \/ DuplicatePublish \/ MutationDuringGate

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
GateHeldUntilResolution == state.phase \in OpenPhases => state.gate
ContinuousAuthority == state.writer /\ (state.oldPin \/ state.newPin)
ExactPendingReceipt == state.phase \in OpenPhases => Bound(state.receipt)
WholeJournalAcknowledgement == state.journal \in {Full, Empty}
AcknowledgementRequiresExactSuccessor ==
    state.acknowledged => state.durable = New /\ state.admitted = New /\ state.newPin
AcknowledgementRequiresExactReceipt ==
    state.acknowledged => state.ackReceipt = state.receipt
ReadyHasCoherentBase ==
    state.phase \in {"Ready", "Done", "Reopened"} =>
        state.hotBase = state.admitted.version /\ state.admitted = state.durable
RecoveryAcceptsOnlyExactTerminal ==
    state.acceptedRecovery => state.recoveryReport = state.durable
AtMostOnePublication == state.publications <= 1
ReopenDiscardsVolatileJournal ==
    state.phase = "Reopened" => state.journal = Empty /\ state.hotBase = state.durable.version
NeverAcknowledged == ~state.acknowledged
NeverRecoveredOld == ~state.recoveredOld
NeverRecoveredNew == ~state.recoveredNew
NeverRejectedBinding == ~state.rejectedBinding
=============================================================================
