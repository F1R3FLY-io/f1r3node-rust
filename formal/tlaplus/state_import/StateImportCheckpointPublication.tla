-------------------- MODULE StateImportCheckpointPublication --------------------
EXTENDS StateImportOperations

CONSTANTS CheckpointActors, CheckpointTarget, CheckpointBase,
          CheckpointHistory, CheckpointCold, CheckpointPlan, CheckpointDefect
VARIABLES cpIssued, cpSettled, cpHandle, cpCaptured, cpBuilt, cpSealed,
          cpStage, cpOutcome, cpWrites, cpTerminalWrites, cpCancelled,
          cpTagSuccess, cpPublished, cpFinalResult, cpReported, cpLocalRoot

CpVars == <<cpIssued, cpSettled, cpHandle, cpCaptured, cpBuilt, cpSealed,
    cpStage, cpOutcome, cpWrites, cpTerminalWrites, cpCancelled,
    cpTagSuccess, cpPublished, cpFinalResult, cpReported, cpLocalRoot>>
CheckpointVars == <<vars, CpVars>>
CpStages == {"Idle", "Cold", "History", "Build", "Tag", "Pointer", "Done", "Failed"}
CpWriteStages == {"Cold", "History", "Tag", "Pointer"}
CpOutcomes == {"Unstarted", "Running", "Committed", "Aborted", "Observed"}
CpBaseRefs(actor) == IF cpCaptured[actor] = None THEN {} ELSE Base!Reach(cpCaptured[actor])
CpCovered(actor) == CpBaseRefs(actor) \cup cpBuilt[actor]

ASSUME /\ CheckpointActors # {} /\ IsFiniteSet(CheckpointActors)
       /\ CheckpointTarget \in [CheckpointActors -> Roots]
       /\ CheckpointBase \in [CheckpointActors -> Roots \cup {None}]
       /\ CheckpointHistory \in [CheckpointActors -> SUBSET HistoryRefs]
       /\ CheckpointCold \in [CheckpointActors -> SUBSET ColdRefs]
       /\ CheckpointPlan \in [CheckpointActors -> SUBSET Refs]
       /\ OperationDefect = "Safe"
       /\ CheckpointDefect \in {"Safe", "FollowCurrentRoot", "OmitConstructedChild",
           "SealBeforeConstruction", "PublishOtherRoot", "PointerAfterTagError",
           "DropRunningCheckpoint", "ReportUnknownPointerSuccess", "ReportAfterCancellation",
           "TerminalLatePointer"}

CheckpointInit ==
    /\ Init
    /\ cpIssued = [actor \in CheckpointActors |-> FALSE]
    /\ cpSettled = [actor \in CheckpointActors |-> FALSE]
    /\ cpHandle = [actor \in CheckpointActors |-> FALSE]
    /\ cpCaptured = [actor \in CheckpointActors |-> None]
    /\ cpBuilt = [actor \in CheckpointActors |-> {}]
    /\ cpSealed = [actor \in CheckpointActors |-> FALSE]
    /\ cpStage = [actor \in CheckpointActors |-> "Idle"]
    /\ cpOutcome = [actor \in CheckpointActors |-> "Unstarted"]
    /\ cpWrites = [actor \in CheckpointActors |-> [stage \in CpWriteStages |-> 0]]
    /\ cpTerminalWrites = [actor \in CheckpointActors |-> [stage \in CpWriteStages |-> 0]]
    /\ cpCancelled = [actor \in CheckpointActors |-> FALSE]
    /\ cpTagSuccess = [actor \in CheckpointActors |-> FALSE]
    /\ cpPublished = [actor \in CheckpointActors |-> None]
    /\ cpFinalResult = [actor \in CheckpointActors |-> "Unread"]
    /\ cpReported = [actor \in CheckpointActors |-> FALSE]
    /\ cpLocalRoot = [actor \in CheckpointActors |->
        IF CheckpointBase[actor] = None THEN InitialRoot ELSE CheckpointBase[actor]]

CpStart(actor) ==
    /\ ~cpIssued[actor] /\ ~cpCancelled[actor]
    /\ CheckpointBase[actor] = None \/ CheckpointBase[actor] \in tags
    /\ cpIssued' = [cpIssued EXCEPT ![actor] = TRUE]
    /\ cpHandle' = [cpHandle EXCEPT ![actor] = TRUE]
    /\ cpCaptured' = [cpCaptured EXCEPT ![actor] =
        IF CheckpointDefect = "FollowCurrentRoot" THEN currentRoot ELSE CheckpointBase[actor]]
    /\ cpStage' = [cpStage EXCEPT ![actor] = "Cold"]
    /\ cpOutcome' = [cpOutcome EXCEPT ![actor] = "Running"]
    /\ UNCHANGED <<vars, cpSettled, cpBuilt, cpSealed, cpWrites, cpTerminalWrites,
        cpCancelled, cpTagSuccess, cpPublished, cpFinalResult, cpReported, cpLocalRoot>>

CpCommit(actor, publicationRoot) ==
    /\ cpIssued[actor] /\ ~cpSettled[actor]
    /\ cpStage[actor] \in CpWriteStages /\ cpOutcome[actor] = "Running"
    /\ CASE cpStage[actor] = "Cold" ->
            /\ publicationRoot = None /\ Base!CompatibleColdBatch(CheckpointCold[actor])
       [] cpStage[actor] = "History" ->
            /\ publicationRoot = None /\ Base!CompatibleHistoryBatch(CheckpointHistory[actor])
       [] cpStage[actor] = "Tag" ->
            /\ cpSealed[actor]
            /\ publicationRoot \in Roots
            /\ publicationRoot = CheckpointTarget[actor] \/ CheckpointDefect = "PublishOtherRoot"
            /\ tags' = tags \cup {publicationRoot}
            /\ UNCHANGED <<history, cold, currentRoot, owner, phase, checked, frontier,
                pending, staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>
       [] OTHER ->
            /\ publicationRoot = CheckpointTarget[actor]
            /\ currentRoot' = publicationRoot
            /\ UNCHANGED <<history, cold, tags, owner, phase, checked, frontier,
                pending, staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>
    /\ cpOutcome' = [cpOutcome EXCEPT ![actor] = "Committed"]
    /\ cpWrites' = [cpWrites EXCEPT ![actor][cpStage[actor]] = @ + 1]
    /\ cpPublished' = IF cpStage[actor] = "Tag"
        THEN [cpPublished EXCEPT ![actor] = publicationRoot] ELSE cpPublished
    /\ UNCHANGED <<GhostVars, cpIssued, cpSettled, cpHandle, cpCaptured, cpBuilt,
        cpSealed, cpStage, cpTerminalWrites, cpCancelled, cpTagSuccess,
        cpFinalResult, cpReported, cpLocalRoot>>

CpAbort(actor) ==
    /\ cpIssued[actor] /\ ~cpSettled[actor]
    /\ cpStage[actor] \in CpWriteStages /\ cpOutcome[actor] = "Running"
    /\ cpOutcome' = [cpOutcome EXCEPT ![actor] = "Aborted"]
    /\ UNCHANGED <<vars, cpIssued, cpSettled, cpHandle, cpCaptured, cpBuilt, cpSealed,
        cpStage, cpWrites, cpTerminalWrites, cpCancelled, cpTagSuccess, cpPublished,
        cpFinalResult, cpReported, cpLocalRoot>>

CpObserve(actor, result) ==
    /\ cpIssued[actor] /\ ~cpSettled[actor]
    /\ cpStage[actor] \in CpWriteStages /\ cpOutcome[actor] \in {"Committed", "Aborted"}
    /\ result \in {"Success", "Failure", "Unknown"}
    /\ result = "Unknown" \/
        (result = "Success" /\ cpOutcome[actor] = "Committed") \/
        (result = "Failure" /\ cpOutcome[actor] = "Aborted")
    /\ IF (result = "Success" /\ cpStage[actor] # "Pointer") \/
            (CheckpointDefect = "PointerAfterTagError" /\ cpStage[actor] = "Tag")
       THEN /\ cpStage' = [cpStage EXCEPT ![actor] = CASE @ = "Cold" -> "History"
                    [] @ = "History" -> "Build" [] OTHER -> "Pointer"]
            /\ cpOutcome' = [cpOutcome EXCEPT ![actor] =
                IF cpStage[actor] = "History" THEN "Observed" ELSE "Running"]
            /\ cpTagSuccess' = IF cpStage[actor] = "Tag"
                THEN [cpTagSuccess EXCEPT ![actor] = (result = "Success")] ELSE cpTagSuccess
            /\ UNCHANGED <<cpSettled, cpHandle, cpTerminalWrites, cpFinalResult,
                cpReported, cpLocalRoot>>
       ELSE LET report == (~cpCancelled[actor] \/ CheckpointDefect = "ReportAfterCancellation") /\
                    cpStage[actor] = "Pointer" /\
                    (result = "Success" \/
                        (CheckpointDefect = "ReportUnknownPointerSuccess" /\ result = "Unknown"))
            IN /\ cpStage' = [cpStage EXCEPT ![actor] =
                    IF result = "Success" THEN "Done" ELSE "Failed"]
               /\ cpOutcome' = [cpOutcome EXCEPT ![actor] = "Observed"]
               /\ cpSettled' = [cpSettled EXCEPT ![actor] = TRUE]
               /\ cpHandle' = [cpHandle EXCEPT ![actor] = FALSE]
               /\ cpTerminalWrites' = [cpTerminalWrites EXCEPT ![actor] = cpWrites[actor]]
               /\ cpFinalResult' = [cpFinalResult EXCEPT ![actor] = result]
               /\ cpReported' = [cpReported EXCEPT ![actor] = report]
               /\ cpLocalRoot' = IF report
                    THEN [cpLocalRoot EXCEPT ![actor] = CheckpointTarget[actor]] ELSE cpLocalRoot
               /\ UNCHANGED cpTagSuccess
    /\ UNCHANGED <<vars, cpIssued, cpCaptured, cpBuilt, cpSealed, cpWrites,
        cpCancelled, cpPublished>>

CpConstruct(actor, ref) ==
    /\ cpStage[actor] = "Build"
    /\ ref \in (CheckpointPlan[actor] \cap Base!Available) \ cpBuilt[actor]
    /\ Children[ref] \subseteq CpCovered(actor) \/ CheckpointDefect = "OmitConstructedChild"
    /\ cpBuilt' = [cpBuilt EXCEPT ![actor] = @ \cup {ref}]
    /\ UNCHANGED <<vars, cpIssued, cpSettled, cpHandle, cpCaptured, cpSealed,
        cpStage, cpOutcome, cpWrites, cpTerminalWrites, cpCancelled, cpTagSuccess,
        cpPublished, cpFinalResult, cpReported, cpLocalRoot>>

CpSeal(actor) ==
    /\ cpStage[actor] = "Build"
    /\ RootRef[CheckpointTarget[actor]] \in CpCovered(actor) \/
        CheckpointDefect = "SealBeforeConstruction"
    /\ cpSealed' = [cpSealed EXCEPT ![actor] = TRUE]
    /\ cpStage' = [cpStage EXCEPT ![actor] = "Tag"]
    /\ cpOutcome' = [cpOutcome EXCEPT ![actor] = "Running"]
    /\ UNCHANGED <<vars, cpIssued, cpSettled, cpHandle, cpCaptured, cpBuilt,
        cpWrites, cpTerminalWrites, cpCancelled, cpTagSuccess, cpPublished,
        cpFinalResult, cpReported, cpLocalRoot>>

CpCancel(actor) ==
    /\ ~cpCancelled[actor] /\ ~cpSettled[actor]
    /\ cpCancelled' = [cpCancelled EXCEPT ![actor] = TRUE]
    /\ UNCHANGED <<vars, cpIssued, cpSettled, cpHandle, cpCaptured, cpBuilt, cpSealed,
        cpStage, cpOutcome, cpWrites, cpTerminalWrites, cpTagSuccess, cpPublished,
        cpFinalResult, cpReported, cpLocalRoot>>

CpDropHandle(actor) ==
    /\ CheckpointDefect = "DropRunningCheckpoint"
    /\ cpIssued[actor] /\ ~cpSettled[actor] /\ cpHandle[actor]
    /\ cpHandle' = [cpHandle EXCEPT ![actor] = FALSE]
    /\ UNCHANGED <<vars, cpIssued, cpSettled, cpCaptured, cpBuilt, cpSealed,
        cpStage, cpOutcome, cpWrites, cpTerminalWrites, cpCancelled, cpTagSuccess,
        cpPublished, cpFinalResult, cpReported, cpLocalRoot>>

CpTerminalLatePointer(actor) ==
    /\ CheckpointDefect = "TerminalLatePointer"
    /\ cpSettled[actor] /\ cpWrites[actor]["Pointer"] = 1
    /\ currentRoot' = CheckpointTarget[actor]
    /\ cpWrites' = [cpWrites EXCEPT ![actor]["Pointer"] = @ + 1]
    /\ UNCHANGED <<history, cold, tags, owner, phase, checked, frontier, pending,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked, GhostVars,
        cpIssued, cpSettled, cpHandle, cpCaptured, cpBuilt, cpSealed, cpStage, cpOutcome,
        cpTerminalWrites, cpCancelled, cpTagSuccess, cpPublished, cpFinalResult,
        cpReported, cpLocalRoot>>

CheckpointNext ==
    \/ Next /\ UNCHANGED CpVars
    \/ \E actor \in CheckpointActors :
        \/ CpStart(actor) \/ CpAbort(actor) \/ CpSeal(actor) \/ CpCancel(actor) \/ CpDropHandle(actor)
        \/ CpTerminalLatePointer(actor)
        \/ \E root \in Roots \cup {None} : CpCommit(actor, root)
        \/ \E result \in {"Success", "Failure", "Unknown"} : CpObserve(actor, result)
        \/ \E ref \in Refs : CpConstruct(actor, ref)

CpTypeOK ==
    /\ cpIssued \in [CheckpointActors -> BOOLEAN] /\ cpSettled \in [CheckpointActors -> BOOLEAN]
    /\ cpHandle \in [CheckpointActors -> BOOLEAN] /\ cpCaptured \in [CheckpointActors -> Roots \cup {None}]
    /\ cpBuilt \in [CheckpointActors -> SUBSET Refs] /\ cpSealed \in [CheckpointActors -> BOOLEAN]
    /\ cpStage \in [CheckpointActors -> CpStages] /\ cpOutcome \in [CheckpointActors -> CpOutcomes]
    /\ cpWrites \in [CheckpointActors -> [CpWriteStages -> Nat]]
    /\ cpTerminalWrites \in [CheckpointActors -> [CpWriteStages -> Nat]]
    /\ cpCancelled \in [CheckpointActors -> BOOLEAN] /\ cpTagSuccess \in [CheckpointActors -> BOOLEAN]
    /\ cpPublished \in [CheckpointActors -> Roots \cup {None}]
    /\ cpFinalResult \in [CheckpointActors -> Results] /\ cpReported \in [CheckpointActors -> BOOLEAN]
    /\ cpLocalRoot \in [CheckpointActors -> Roots]
CpCapturedBaseIsLocal == \A actor \in CheckpointActors : cpIssued[actor] =>
    cpCaptured[actor] = CheckpointBase[actor]
CpCapturedBaseClosed == \A actor \in CheckpointActors : cpCaptured[actor] # None =>
    Base!Closed(cpCaptured[actor])
CpConstructionCut == \A actor \in CheckpointActors :
    /\ cpBuilt[actor] \subseteq CheckpointPlan[actor] \cap Base!Available
    /\ \A ref \in cpBuilt[actor] : Children[ref] \subseteq CpCovered(actor)
CpCertificateCoversRoot == \A actor \in CheckpointActors : cpSealed[actor] =>
    /\ \A stage \in {"Cold", "History"} : cpWrites[actor][stage] = 1
    /\ RootRef[CheckpointTarget[actor]] \in CpCovered(actor)
    /\ Base!Closed(CheckpointTarget[actor])
CpPublishedRootAuthorized == \A actor \in CheckpointActors : cpPublished[actor] # None =>
    /\ cpSealed[actor] /\ cpPublished[actor] = CheckpointTarget[actor]
CpPointerRequiresTagSuccess == \A actor \in CheckpointActors : cpStage[actor] = "Pointer" =>
    /\ cpTagSuccess[actor] /\ cpWrites[actor]["Tag"] = 1
CpOutstandingWorkOwned == \A actor \in CheckpointActors : cpIssued[actor] /\ ~cpSettled[actor] => cpHandle[actor]
CpTerminalCannotExecute == \A actor \in CheckpointActors : cpSettled[actor] =>
    /\ cpOutcome[actor] = "Observed" /\ cpWrites[actor] = cpTerminalWrites[actor]
CpReportedRequiresSuccess == \A actor \in CheckpointActors :
    /\ (cpReported[actor] => cpSettled[actor] /\ cpFinalResult[actor] = "Success" /\
        cpStage[actor] = "Done" /\ cpWrites[actor]["Pointer"] = 1)
    /\ (cpLocalRoot[actor] = IF cpReported[actor] THEN CheckpointTarget[actor]
        ELSE IF CheckpointBase[actor] = None THEN InitialRoot ELSE CheckpointBase[actor])
CpCancelledResultSuppressed == \A actor \in CheckpointActors : cpReported[actor] => ~cpCancelled[actor]

CheckpointInvariants == AllInvariants /\ CpTypeOK /\ CpCapturedBaseIsLocal /\ CpCapturedBaseClosed
    /\ CpConstructionCut /\ CpCertificateCoversRoot /\ CpPublishedRootAuthorized
    /\ CpPointerRequiresTagSuccess /\ CpOutstandingWorkOwned /\ CpTerminalCannotExecute
    /\ CpReportedRequiresSuccess /\ CpCancelledResultSuppressed
CheckpointSpec == CheckpointInit /\ [][CheckpointNext]_CheckpointVars
=============================================================================
