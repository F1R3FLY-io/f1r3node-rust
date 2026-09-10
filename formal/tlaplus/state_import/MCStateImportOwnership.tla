-------------------- MODULE MCStateImportOwnership --------------------
EXTENDS Naturals, TLC

CONSTANTS OwnershipDefect, TransferActors, MaxGenerations
VARIABLES history, cold, tags, currentRoot, owner, phase, checked, frontier,
          pending, staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked,
          writePermit, tagPermit, retirement, capturedRoot, unauthorizedTag, unmatchedWrite
Actors == TransferActors
Roots == {"initial", "left", "right"}
HistoryRefs == {"h0", "h1", "h2", "shared"}
ColdRefs == {"leaf"}
RootRef == [root \in Roots |-> CASE root = "initial" -> "h0"
    [] root = "left" -> "h1" [] OTHER -> "h2"]
Children == [ref \in HistoryRefs \cup ColdRefs |->
    CASE ref \in {"h1", "h2"} -> {"shared"}
    [] ref = "shared" -> {"leaf"} [] OTHER -> {}]
ActorRoot == [actor \in Actors |-> IF actor = "a" THEN "left" ELSE "right"]
InitialRoot == "initial"
Generations == MaxGenerations
INSTANCE StateImportOwnership
ASSUME None \notin Refs
=============================================================================
