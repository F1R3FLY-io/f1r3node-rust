------------------- MODULE MCStateImportCheckpointPublication -------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS TransferActors, CheckpointActorSet, MaxGenerations, NonemptyBase, FreshCheckpoint,
          OperationDefect, CheckpointDefect
VARIABLES history, cold, tags, currentRoot, owner, phase, checked, frontier,
          pending, staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked,
          issued, settled, operations, handle, callerCancelled, scanReceipt,
          markerRead, retirement, executions, terminalExecutions, unmatchedCommit,
          cpIssued, cpSettled, cpHandle, cpCaptured, cpBuilt, cpSealed,
          cpStage, cpOutcome, cpWrites, cpTerminalWrites, cpCancelled,
          cpTagSuccess, cpPublished, cpFinalResult, cpReported, cpLocalRoot

Actors == TransferActors
CheckpointActors == CheckpointActorSet
Roots == {"initial", "left", "right"}
HistoryRefs == {"h0", "h1", "h2", "shared"}
ColdRefs == {"shared-leaf", "left-data", "right-data"}
RootRef == [root \in Roots |-> CASE root = "initial" -> "h0"
    [] root = "left" -> "h1" [] OTHER -> "h2"]
Children == [ref \in HistoryRefs \cup ColdRefs |->
    CASE ref = "h0" -> IF NonemptyBase THEN {"shared"} ELSE {}
    [] ref = "h1" -> {"shared", "left-data"}
    [] ref = "h2" -> {"shared", "right-data"}
    [] ref = "shared" -> {"shared-leaf"}
    [] OTHER -> {}]
ActorRoot == [actor \in Actors |-> IF actor = "a" THEN "left" ELSE "right"]
InitialRoot == "initial"
Generations == MaxGenerations
MaxOperations == 1
Effects == {"left-root", "right-root", "shared-node", "shared-cold", "left-cold", "right-cold"}
EffectRefs == [effect \in Effects |-> CASE effect = "left-root" -> {"h1"}
    [] effect = "right-root" -> {"h2"} [] effect = "shared-node" -> {"shared"}
    [] effect = "shared-cold" -> {"shared-leaf"} [] effect = "left-cold" -> {"left-data"}
    [] OTHER -> {"right-data"}]
EffectDomain == [effect \in Effects |->
    IF effect \in {"left-root", "right-root", "shared-node"} THEN "History" ELSE "Cold"]
AllowedEffects == [root \in Roots |-> IF root = "initial" THEN {}
    ELSE {"shared-node", "shared-cold"} \cup
        IF root = "left" THEN {"left-root", "left-cold"} ELSE {"right-root", "right-cold"}]
PhysicalKey == [ref \in HistoryRefs \cup ColdRefs |-> ref]
CheckpointTarget == [actor \in CheckpointActors |-> IF actor = "c" THEN "left" ELSE "right"]
CheckpointBase == [actor \in CheckpointActors |-> IF FreshCheckpoint THEN "none" ELSE "initial"]
CheckpointHistory == [actor \in CheckpointActors |->
    {RootRef[CheckpointTarget[actor]]} \cup IF NonemptyBase /\ ~FreshCheckpoint THEN {} ELSE {"shared"}]
CheckpointCold == [actor \in CheckpointActors |->
    {IF actor = "c" THEN "left-data" ELSE "right-data"} \cup
        IF NonemptyBase /\ ~FreshCheckpoint THEN {} ELSE {"shared-leaf"}]
CheckpointPlan == [actor \in CheckpointActors |-> CheckpointHistory[actor] \cup CheckpointCold[actor]]

INSTANCE StateImportCheckpointPublication
=============================================================================
