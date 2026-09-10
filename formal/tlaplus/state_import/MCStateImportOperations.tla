-------------------------- MODULE MCStateImportOperations --------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS TransferActors, MaxGenerations, OperationBound, SharedContexts, OperationDefect
VARIABLES history, cold, tags, currentRoot, owner, phase, checked, frontier,
          pending, staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked,
          issued, settled, operations, handle, callerCancelled, scanReceipt,
          markerRead, retirement, executions, terminalExecutions, unmatchedCommit

Actors == TransferActors
Roots == {"initial", "left", "right"}
HistoryRefs == {"h0", "h1", "h2"} \cup
    IF SharedContexts THEN {"data-parent", "shared-left", "shared-right"} ELSE {}
ColdRefs == {"data"}
RootRef == [root \in Roots |-> CASE root = "initial" -> "h0"
    [] root = "left" -> "h1" [] OTHER -> "h2"]
Children == [ref \in HistoryRefs \cup ColdRefs |->
    CASE ref = "h1" -> IF SharedContexts THEN {"data-parent"} ELSE {"data"}
    [] ref = "h2" -> IF SharedContexts THEN {"shared-left"} ELSE {"data"}
    [] ref = "data-parent" -> {"shared-left", "shared-right"}
    [] ref \in {"shared-left", "shared-right"} -> {"data"}
    [] OTHER -> {}]
ActorRoot == [actor \in Actors |-> IF actor = "a" THEN "left" ELSE "right"]
InitialRoot == "initial"
Generations == MaxGenerations
MaxOperations == OperationBound
Effects == {"left-root", "right-root", "cold"} \cup IF SharedContexts THEN {"shared"} ELSE {}
EffectRefs == [effect \in Effects |-> CASE effect = "left-root" ->
        IF SharedContexts THEN {"h1", "data-parent"} ELSE {"h1"}
    [] effect = "right-root" -> {"h2"}
    [] effect = "shared" -> {"shared-left", "shared-right"} [] OTHER -> ColdRefs]
EffectDomain == [effect \in Effects |-> IF effect = "cold" THEN "Cold" ELSE "History"]
AllowedEffects == [root \in Roots |->
    IF root = InitialRoot THEN {}
    ELSE {IF root = "left" THEN "left-root" ELSE "right-root", "cold"} \cup
        IF SharedContexts THEN {"shared"} ELSE {}]
PhysicalKey == [ref \in HistoryRefs \cup ColdRefs |->
    IF ref \in {"shared-left", "shared-right"} THEN "shared-key" ELSE ref]
INSTANCE StateImportOperations

WitnessAttempt == <<"a", 1>>
WitnessHistory == <<WitnessAttempt, 1>>
WitnessCold == <<WitnessAttempt, 2>>
WitnessPublication == <<WitnessAttempt, 3>>
SequenceWitnessNext(mode) ==
    \/ StartOperation(WitnessAttempt, WitnessHistory, "History", "left-root")
    \/ CommitOperation(WitnessHistory, "left-root")
    \/ ObserveTransaction(WitnessHistory, "Success")
    \/ (WitnessHistory \in settled /\
        StartOperation(WitnessAttempt, WitnessCold, "Cold", "cold"))
    \/ CommitOperation(WitnessCold, "cold")
    \/ ObserveTransaction(WitnessCold, "Success")
    \/ (WitnessCold \in settled /\ BeginScan(WitnessAttempt))
    \/ \E ref \in Refs : CheckReference(WitnessAttempt, ref)
    \/ FinishScan(WitnessAttempt)
    \/ StartOperation(WitnessAttempt, WitnessPublication, "Publication", None)
    \/ ((mode # "Cancelled" \/ callerCancelled[WitnessAttempt]) /\
        CommitOperation(WitnessPublication, None))
    \/ ObserveTransaction(WitnessPublication,
        IF (mode = "UnknownTag" /\ operations[WitnessPublication].stage = "Tag") \/
           (mode = "UnknownPointer" /\ operations[WitnessPublication].stage = "Pointer")
        THEN "Unknown" ELSE "Success")
    \/ (mode = "Cancelled" /\ WitnessPublication \in issued /\
        CancelCaller(WitnessAttempt))
    \/ (mode \in {"UnknownTag", "UnknownPointer"} /\ WitnessPublication \in settled /\
        RetryAttempt(WitnessAttempt))
    \/ (WitnessPublication \in settled /\ ReadMarker(WitnessAttempt, "Found"))
    \/ RetireRequest(WitnessAttempt)
OwnedSequenceSpec == Init /\ [][SequenceWitnessNext("Owned")]_vars /\ WF_vars(SequenceWitnessNext("Owned"))
CancelledSequenceSpec == Init /\ [][SequenceWitnessNext("Cancelled")]_vars /\ WF_vars(SequenceWitnessNext("Cancelled"))
UnknownTagSequenceSpec == Init /\ [][SequenceWitnessNext("UnknownTag")]_vars /\ WF_vars(SequenceWitnessNext("UnknownTag"))
UnknownPointerSequenceSpec == Init /\ [][SequenceWitnessNext("UnknownPointer")]_vars /\ WF_vars(SequenceWitnessNext("UnknownPointer"))
OwnedSequenceComplete == <>(
    {WitnessHistory, WitnessCold, WitnessPublication} \subseteq settled /\ ~pending["a"])
CancelledSequenceComplete == <>(
    {WitnessHistory, WitnessCold, WitnessPublication} \subseteq settled /\
    callerCancelled[WitnessAttempt] /\ pending["a"])
=============================================================================
