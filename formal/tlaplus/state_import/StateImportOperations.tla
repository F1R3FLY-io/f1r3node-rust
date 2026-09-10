-------------------------- MODULE StateImportOperations --------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS Actors, Roots, HistoryRefs, ColdRefs, RootRef, Children,
          ActorRoot, InitialRoot, Generations, Effects, EffectRefs, EffectDomain,
          AllowedEffects, PhysicalKey, MaxOperations, OperationDefect
VARIABLES history, cold, tags, currentRoot, owner, phase, checked, frontier,
          pending, staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked,
          issued, settled, operations, handle, callerCancelled, scanReceipt,
          markerRead, retirement, executions, terminalExecutions, unmatchedCommit

Base == INSTANCE StateImportPublication WITH Defect <- "Safe"
BaseVars == <<history, cold, tags, currentRoot, owner, phase, checked, frontier,
    pending, staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>
GhostVars == <<issued, settled, operations, handle, callerCancelled, scanReceipt,
    markerRead, retirement, executions, terminalExecutions, unmatchedCommit>>
vars == <<BaseVars, GhostVars>>
None == "none"
NoAttempt == <<None, 0>>
NoOp == <<NoAttempt, 0>>
Attempts == Actors \X (1..Generations)
Ops == Attempts \X (1..MaxOperations)
Refs == HistoryRefs \cup ColdRefs
Stages == {"History", "Cold", "Tag", "Pointer"}
Kinds == {"History", "Cold", "Publication"}
Results == {"Unread", "Success", "Failure", "Unknown"}
NoCounts == [stage \in Stages |-> 0]
EmptyOperation == [kind |-> None, effect |-> None, stage |-> None,
    status |-> "Unused", result |-> "Unread", authorized |-> FALSE,
    tagReturnedSuccess |-> FALSE]
Owned(attempt) == Base!Owned(attempt)
Active(attempt) == Owned(attempt) /\ pending[attempt[1]] /\ ~callerCancelled[attempt]
AttemptRoot(attempt) == Base!AttemptRoot(attempt)
Outstanding(op) == op \in issued \ settled
HasReceipt(attempt) == scanReceipt[attempt] = AttemptRoot(attempt)

ASSUME /\ IsFiniteSet(Effects) /\ MaxOperations \in Nat \ {0}
       /\ None \notin Effects /\ None \notin Refs
       /\ EffectRefs \in [Effects -> SUBSET Refs]
       /\ EffectDomain \in [Effects -> {"History", "Cold"}]
       /\ AllowedEffects \in [Roots -> SUBSET Effects]
       /\ DOMAIN PhysicalKey = Refs
       /\ \A effect \in Effects :
           EffectRefs[effect] \subseteq
               IF EffectDomain[effect] = "History" THEN HistoryRefs ELSE ColdRefs
       /\ OperationDefect \in {"Safe", "DropRunningOperation", "UnmatchedBatchCommit",
           "AuthorizeAfterCancel", "CommitAfterTerminalUnknown", "PointerAfterTagError",
           "MarkerOnlyCompletion", "StaleResultRetiresReplacement",
           "UnknownResultRetiresRecovery", "DeduplicateByPhysicalKey",
           "CheckpointClobbersCheckedReference"}

Init ==
    /\ Base!Init
    /\ issued = {} /\ settled = {}
    /\ operations = [op \in Ops |-> EmptyOperation]
    /\ handle = [attempt \in Attempts |-> NoOp]
    /\ callerCancelled = [attempt \in Attempts |-> FALSE]
    /\ scanReceipt = [attempt \in Attempts |-> None]
    /\ markerRead = [attempt \in Attempts |-> "Unread"]
    /\ retirement = [actor \in Actors |-> NoAttempt]
    /\ executions = [op \in Ops |-> NoCounts]
    /\ terminalExecutions = [op \in Ops |-> NoCounts]
    /\ unmatchedCommit = FALSE

CompatibleEffect(effect) ==
    IF EffectDomain[effect] = "History"
    THEN Base!CompatibleHistoryBatch(EffectRefs[effect])
    ELSE Base!CompatibleColdBatch(EffectRefs[effect])

StartOperation(attempt, op, kind, effect) ==
    /\ op \in Ops \ issued /\ op[1] = attempt /\ handle[attempt] = NoOp
    /\ Active(attempt) \/
        (OperationDefect = "AuthorizeAfterCancel" /\ Owned(attempt) /\ pending[attempt[1]])
    /\ IF kind = "Publication"
       THEN /\ effect = None /\ HasReceipt(attempt)
            /\ Base!AuthorizeTag(attempt)
       ELSE /\ phase[attempt] = "Load" /\ effect \in AllowedEffects[AttemptRoot(attempt)]
            /\ EffectDomain[effect] = kind /\ UNCHANGED BaseVars
    /\ issued' = issued \cup {op}
    /\ handle' = [handle EXCEPT ![attempt] = op]
    /\ operations' = [operations EXCEPT ![op] =
        [kind |-> kind, effect |-> effect,
         stage |-> IF kind = "Publication" THEN "Tag" ELSE kind,
         status |-> "Running", result |-> "Unread", authorized |-> Active(attempt),
         tagReturnedSuccess |-> FALSE]]
    /\ UNCHANGED <<settled, callerCancelled, scanReceipt, markerRead, retirement,
        executions, terminalExecutions, unmatchedCommit>>

CommitOperation(op, effect) ==
    /\ Outstanding(op) /\ operations[op].status = "Running"
    /\ IF operations[op].kind = "Publication"
       THEN /\ effect = None
            /\ IF operations[op].stage = "Tag"
               THEN Base!WriteTag(op[1]) ELSE Base!WriteCurrentRoot(op[1])
       ELSE /\ effect \in Effects
            /\ effect = operations[op].effect \/ OperationDefect = "UnmatchedBatchCommit"
            /\ CompatibleEffect(effect)
    /\ operations' = [operations EXCEPT ![op].status = "Committed"]
    /\ executions' = [executions EXCEPT ![op][operations[op].stage] = @ + 1]
    /\ unmatchedCommit' = (unmatchedCommit \/ effect # operations[op].effect)
    /\ UNCHANGED <<issued, settled, handle, callerCancelled, scanReceipt,
        markerRead, retirement, terminalExecutions>>

AbortOperation(op) ==
    /\ Outstanding(op) /\ operations[op].status = "Running"
    /\ operations' = [operations EXCEPT ![op].status = "Aborted"]
    /\ UNCHANGED <<BaseVars, issued, settled, handle, callerCancelled, scanReceipt,
        markerRead, retirement, executions, terminalExecutions, unmatchedCommit>>

ObserveTransaction(op, result) ==
    /\ Outstanding(op) /\ operations[op].status \in {"Committed", "Aborted"}
    /\ result \in Results \ {"Unread"}
    /\ result = "Unknown" \/
        (result = "Success" /\ operations[op].status = "Committed") \/
        (result = "Failure" /\ operations[op].status = "Aborted")
    /\ IF operations[op].stage = "Tag" /\
            (result = "Success" \/ OperationDefect = "PointerAfterTagError")
       THEN /\ operations' = [operations EXCEPT ![op].stage = "Pointer",
                   ![op].status = "Running", ![op].tagReturnedSuccess = (result = "Success")]
            /\ UNCHANGED <<BaseVars, settled, handle, terminalExecutions>>
       ELSE /\ operations' = [operations EXCEPT ![op].result = result]
            /\ settled' = settled \cup {op}
            /\ handle' = [handle EXCEPT ![op[1]] = NoOp]
            /\ terminalExecutions' = [terminalExecutions EXCEPT ![op] = executions[op]]
            /\ IF result = "Success" THEN UNCHANGED BaseVars
               ELSE IF OperationDefect = "UnknownResultRetiresRecovery" /\ result = "Unknown"
               THEN /\ pending' = [pending EXCEPT ![op[1][1]] = FALSE]
                    /\ UNCHANGED <<history, cold, tags, currentRoot, owner, phase, checked,
                        frontier, staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>
               ELSE Base!FailAttempt(op[1])
    /\ UNCHANGED <<issued, callerCancelled, scanReceipt, markerRead, retirement,
        executions, unmatchedCommit>>

BeginScan(attempt) ==
    /\ Active(attempt) /\ handle[attempt] = NoOp /\ Base!BeginScan(attempt)
    /\ UNCHANGED GhostVars

CheckReference(attempt, ref) ==
    /\ Active(attempt)
    /\ IF OperationDefect = "DeduplicateByPhysicalKey"
       THEN /\ phase[attempt] = "Scan" /\ ref \in frontier[attempt] \cap Base!Available
            /\ checked' = [checked EXCEPT ![attempt] = @ \cup {ref}]
            /\ frontier' = [frontier EXCEPT ![attempt] =
                {next \in (@ \cup Children[ref]) \ (checked[attempt] \cup {ref}) :
                    PhysicalKey[next] # PhysicalKey[ref]}]
            /\ UNCHANGED <<history, cold, tags, currentRoot, owner, phase, pending,
                staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>
       ELSE Base!CheckReference(attempt, ref)
    /\ UNCHANGED GhostVars

FinishScan(attempt) ==
    /\ Active(attempt) /\ phase[attempt] = "Scan" /\ frontier[attempt] = {}
    /\ scanReceipt[attempt] = None
    /\ scanReceipt' = [scanReceipt EXCEPT ![attempt] = AttemptRoot(attempt)]
    /\ UNCHANGED <<BaseVars, issued, settled, operations, handle, callerCancelled,
        markerRead, retirement, executions, terminalExecutions, unmatchedCommit>>

ReadMarker(attempt, outcome) ==
    /\ Active(attempt) /\ handle[attempt] = NoOp /\ markerRead[attempt] = "Unread"
    /\ HasReceipt(attempt) \/ OperationDefect = "MarkerOnlyCompletion"
    /\ outcome \in {"Found", "Absent", "Failure"}
    /\ outcome = "Failure" \/
        (outcome = "Found" /\ AttemptRoot(attempt) \in tags) \/
        (outcome = "Absent" /\ AttemptRoot(attempt) \notin tags)
    /\ markerRead' = [markerRead EXCEPT ![attempt] = outcome]
    /\ UNCHANGED <<BaseVars, issued, settled, operations, handle, callerCancelled,
        scanReceipt, retirement, executions, terminalExecutions, unmatchedCommit>>

RetireRequest(attempt) ==
    /\ Active(attempt) \/
        (OperationDefect = "StaleResultRetiresReplacement" /\ pending[attempt[1]])
    /\ markerRead[attempt] = "Found" /\ handle[attempt] = NoOp
    /\ HasReceipt(attempt) \/ OperationDefect = "MarkerOnlyCompletion"
    /\ pending' = [pending EXCEPT ![attempt[1]] = FALSE]
    /\ phase' = IF phase[attempt] = "Pointed"
        THEN [phase EXCEPT ![attempt] = "Complete"] ELSE phase
    /\ retirement' = [retirement EXCEPT ![attempt[1]] = attempt]
    /\ UNCHANGED <<history, cold, tags, currentRoot, owner, checked, frontier,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked,
        issued, settled, operations, handle, callerCancelled, scanReceipt, markerRead,
        executions, terminalExecutions, unmatchedCommit>>

CancelCaller(attempt) ==
    /\ Active(attempt)
    /\ callerCancelled' = [callerCancelled EXCEPT ![attempt] = TRUE]
    /\ UNCHANGED <<BaseVars, issued, settled, operations, handle, scanReceipt,
        markerRead, retirement, executions, terminalExecutions, unmatchedCommit>>

FailIdleAttempt(attempt) ==
    /\ handle[attempt] = NoOp /\ pending[attempt[1]]
    /\ Base!FailAttempt(attempt)
    /\ UNCHANGED GhostVars

RetryAttempt(attempt) ==
    /\ handle[attempt] = NoOp /\ Base!RetryAttempt(attempt)
    /\ callerCancelled' = [callerCancelled EXCEPT ![attempt] = FALSE]
    /\ scanReceipt' = [scanReceipt EXCEPT ![attempt] = None]
    /\ markerRead' = [markerRead EXCEPT ![attempt] = "Unread"]
    /\ UNCHANGED <<issued, settled, operations, handle, retirement,
        executions, terminalExecutions, unmatchedCommit>>

ReplaceRequest(actor) ==
    /\ Base!ReplaceRequest(actor)
    /\ retirement' = [retirement EXCEPT ![actor] = NoAttempt]
    /\ UNCHANGED <<issued, settled, operations, handle, callerCancelled, scanReceipt,
        markerRead, executions, terminalExecutions, unmatchedCommit>>

CheckpointCommit(effect) == CompatibleEffect(effect) /\ UNCHANGED GhostVars
CheckpointSelect(root) == Base!CheckpointSelect(root) /\ UNCHANGED GhostVars
ReaderCapture == Base!ReaderCapture /\ UNCHANGED GhostVars
ReaderCheck(ref) == Base!ReaderCheck(ref) /\ UNCHANGED GhostVars
ReaderComplete == Base!ReaderComplete /\ UNCHANGED GhostVars

DropRunningOperation(op) ==
    /\ OperationDefect = "DropRunningOperation"
    /\ Outstanding(op) /\ operations[op].status = "Running" /\ handle[op[1]] = op
    /\ handle' = [handle EXCEPT ![op[1]] = NoOp]
    /\ UNCHANGED <<BaseVars, issued, settled, operations, callerCancelled, scanReceipt,
        markerRead, retirement, executions, terminalExecutions, unmatchedCommit>>

CommitAfterTerminalUnknown(op) ==
    /\ OperationDefect = "CommitAfterTerminalUnknown"
    /\ op \in settled /\ operations[op].result = "Unknown"
    /\ executions' = [executions EXCEPT ![op][operations[op].stage] = @ + 1]
    /\ UNCHANGED <<BaseVars, issued, settled, operations, handle, callerCancelled,
        scanReceipt, markerRead, retirement, terminalExecutions, unmatchedCommit>>

CheckpointClobbers(ref) ==
    /\ OperationDefect = "CheckpointClobbersCheckedReference"
    /\ \E attempt \in Attempts : ref \in checked[attempt]
    /\ history' = history \ {ref} /\ cold' = cold \ {ref}
    /\ UNCHANGED <<tags, currentRoot, owner, phase, checked, frontier, pending,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked, GhostVars>>

Next ==
    \/ \E attempt \in Attempts :
        \/ \E op \in Ops, kind \in Kinds, effect \in Effects \cup {None} :
            StartOperation(attempt, op, kind, effect)
        \/ \E ref \in Refs : CheckReference(attempt, ref)
        \/ \E outcome \in {"Found", "Absent", "Failure"} : ReadMarker(attempt, outcome)
        \/ BeginScan(attempt) \/ FinishScan(attempt) \/ RetireRequest(attempt)
        \/ CancelCaller(attempt) \/ FailIdleAttempt(attempt) \/ RetryAttempt(attempt)
    \/ \E op \in Ops :
        \/ \E effect \in Effects \cup {None} : CommitOperation(op, effect)
        \/ \E result \in Results : ObserveTransaction(op, result)
        \/ AbortOperation(op) \/ DropRunningOperation(op) \/ CommitAfterTerminalUnknown(op)
    \/ \E actor \in Actors : ReplaceRequest(actor)
    \/ \E effect \in Effects : CheckpointCommit(effect)
    \/ \E root \in Roots : CheckpointSelect(root)
    \/ ReaderCapture \/ ReaderComplete
    \/ \E ref \in Refs : ReaderCheck(ref) \/ CheckpointClobbers(ref)

TypeOK ==
    /\ Base!TypeOK /\ issued \subseteq Ops /\ settled \subseteq issued
    /\ operations \in [Ops ->
        [kind : Kinds \cup {None}, effect : Effects \cup {None}, stage : Stages \cup {None},
         status : {"Unused", "Running", "Committed", "Aborted"}, result : Results,
         authorized : BOOLEAN, tagReturnedSuccess : BOOLEAN]]
    /\ handle \in [Attempts -> Ops \cup {NoOp}]
    /\ callerCancelled \in [Attempts -> BOOLEAN]
    /\ scanReceipt \in [Attempts -> Roots \cup {None}]
    /\ markerRead \in [Attempts -> {"Unread", "Found", "Absent", "Failure"}]
    /\ retirement \in [Actors -> Attempts \cup {NoAttempt}]
    /\ executions \in [Ops -> [Stages -> Nat]]
    /\ terminalExecutions \in [Ops -> [Stages -> Nat]]
    /\ unmatchedCommit \in BOOLEAN

UnsettledOperationsOwned == \A op \in issued \ settled : handle[op[1]] = op
HandlesMatchIssuedOperations == \A attempt \in Attempts : handle[attempt] # NoOp =>
    /\ handle[attempt] \in issued \ settled /\ handle[attempt][1] = attempt
OperationAuthorizationCurrent == \A op \in issued : operations[op].authorized
CommitMatchesAuthorization == ~unmatchedCommit
TerminalOperationsCannotExecuteAgain == \A op \in settled :
    /\ operations[op].status \in {"Committed", "Aborted"}
    /\ executions[op] = terminalExecutions[op]
PointerRequiresTagSuccess == \A op \in issued : operations[op].stage = "Pointer" =>
    /\ operations[op].tagReturnedSuccess /\ executions[op]["Tag"] = 1
ScanReceiptsEstablishClosure == \A attempt \in Attempts : scanReceipt[attempt] # None =>
    /\ scanReceipt[attempt] = AttemptRoot(attempt) /\ Base!Closed(AttemptRoot(attempt))
MarkerObservationsMatchRoot == \A attempt \in Attempts : markerRead[attempt] = "Found" =>
    AttemptRoot(attempt) \in tags
FrontierPreservesContextualChildren == \A attempt \in Attempts :
    phase[attempt] \in Base!ScanningPhases =>
        \A ref \in checked[attempt] : Children[ref] \subseteq checked[attempt] \cup frontier[attempt]
CheckedReferencesRemainAvailable == \A attempt \in Attempts :
    phase[attempt] \in Base!ScanningPhases => checked[attempt] \subseteq Base!Available
RetirementHasCurrentEvidence == \A actor \in Actors : ~pending[actor] =>
    LET attempt == <<actor, owner[actor]>>
    IN /\ retirement[actor] = attempt /\ scanReceipt[attempt] = ActorRoot[actor]
       /\ markerRead[attempt] = "Found" /\ ActorRoot[actor] \in tags
       /\ Base!Closed(ActorRoot[actor])
AllInvariants ==
    /\ TypeOK /\ UnsettledOperationsOwned /\ HandlesMatchIssuedOperations
    /\ OperationAuthorizationCurrent /\ CommitMatchesAuthorization
    /\ TerminalOperationsCannotExecuteAgain /\ PointerRequiresTagSuccess
    /\ ScanReceiptsEstablishClosure /\ MarkerObservationsMatchRoot /\ RetirementHasCurrentEvidence
    /\ FrontierPreservesContextualChildren /\ CheckedReferencesRemainAvailable
    /\ Base!FrontierCut /\ Base!PublishedRootsClosed /\ Base!CurrentRootClosed
    /\ Base!UnresolvedRecoveryOwned /\ Base!CapturedReaderClosed /\ Base!ReaderValuesPreserved
    /\ Base!NoStaleCompletion /\ Base!CompletedAttemptsClosed
RefinesBase == Base!Spec
Spec == Init /\ [][Next]_vars
=============================================================================
