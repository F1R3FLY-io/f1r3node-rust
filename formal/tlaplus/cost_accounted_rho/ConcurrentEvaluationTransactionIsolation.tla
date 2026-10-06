------------- MODULE ConcurrentEvaluationTransactionIsolation -------------
EXTENDS Integers, FiniteSets

CONSTANT
    \* @type: Str;
    Defect

Transactions == {1, 2}
BaseRoot == 0

ASSUME Defect \in {
    "None",
    "ParserReusesPriorWitness",
    "ReducerErasesAttempt",
    "PlayValidationNoRollback",
    "ReplayValidationNoRollback",
    "ReplayEvidenceBeforeFinalValidation",
    "SharedCurrentRootAuthority",
    "SharedCurrentRootPublication",
    "RollbackDeletesForeignRoot"
}

CandidateRoot(transaction) == transaction
IsReplay(transaction) == transaction = 2

VARIABLES
    \* @type: Int -> Str;
    phase,
    \* @type: Int -> Int;
    baseRoot,
    \* @type: Int -> Int;
    localRoot,
    \* @type: Int -> Int;
    budgetWitness,
    \* @type: Int -> Int;
    resultWitness,
    \* @type: Int -> Int;
    attemptedWork,
    \* @type: Int -> Int;
    hotState,
    \* @type: Set(Int);
    validated,
    \* @type: Set(Int);
    evidence,
    \* @type: Int -> Int;
    publishedRoot,
    \* @type: Set(Int);
    recordedRoots,
    \* @type: Int;
    currentPointer

\* @type: <<
\*   Int -> Str,
\*   Int -> Int,
\*   Int -> Int,
\*   Int -> Int,
\*   Int -> Int,
\*   Int -> Int,
\*   Int -> Int,
\*   Set(Int),
\*   Set(Int),
\*   Int -> Int,
\*   Set(Int),
\*   Int
\* >>;
vars == <<
    phase,
    baseRoot,
    localRoot,
    budgetWitness,
    resultWitness,
    attemptedWork,
    hotState,
    validated,
    evidence,
    publishedRoot,
    recordedRoots,
    currentPointer
>>

Init ==
    /\ phase = [transaction \in Transactions |-> "Idle"]
    /\ baseRoot = [transaction \in Transactions |-> BaseRoot]
    /\ localRoot = [transaction \in Transactions |-> BaseRoot]
    /\ budgetWitness = [transaction \in Transactions |-> 3]
    /\ resultWitness = [transaction \in Transactions |-> 0]
    /\ attemptedWork = [transaction \in Transactions |-> 0]
    /\ hotState = [transaction \in Transactions |-> 0]
    /\ validated = {}
    /\ evidence = {}
    /\ publishedRoot = [transaction \in Transactions |-> BaseRoot]
    /\ recordedRoots = {BaseRoot}
    /\ currentPointer = BaseRoot

ParserFail(transaction) ==
    /\ transaction \in Transactions
    /\ phase[transaction] = "Idle"
    /\ phase' = [phase EXCEPT ![transaction] = "ParserRejected"]
    /\ resultWitness' = [resultWitness EXCEPT
        ![transaction] =
            IF Defect = "ParserReusesPriorWitness"
            THEN budgetWitness[transaction]
            ELSE 0]
    /\ UNCHANGED <<
        baseRoot,
        localRoot,
        budgetWitness,
        attemptedWork,
        hotState,
        validated,
        evidence,
        publishedRoot,
        recordedRoots,
        currentPointer
        >>

CaptureBase(transaction) ==
    /\ transaction \in Transactions
    /\ phase[transaction] = "Idle"
    /\ phase' = [phase EXCEPT ![transaction] = "Captured"]
    /\ baseRoot' = [baseRoot EXCEPT
        ![transaction] =
            IF Defect = "SharedCurrentRootAuthority"
            THEN currentPointer
            ELSE BaseRoot]
    /\ budgetWitness' = [budgetWitness EXCEPT ![transaction] = 0]
    /\ resultWitness' = [resultWitness EXCEPT ![transaction] = 0]
    /\ attemptedWork' = [attemptedWork EXCEPT ![transaction] = 0]
    /\ hotState' = [hotState EXCEPT ![transaction] = 0]
    /\ validated' = validated \ {transaction}
    /\ evidence' = evidence \ {transaction}
    /\ publishedRoot' = [publishedRoot EXCEPT ![transaction] = BaseRoot]
    /\ UNCHANGED <<localRoot, recordedRoots, currentPointer>>

ResetToCapturedBase(transaction) ==
    /\ transaction \in Transactions
    /\ phase[transaction] = "Captured"
    /\ phase' = [phase EXCEPT ![transaction] = "Reset"]
    /\ localRoot' = [localRoot EXCEPT ![transaction] = baseRoot[transaction]]
    /\ currentPointer' = baseRoot[transaction]
    /\ UNCHANGED <<
        baseRoot,
        budgetWitness,
        resultWitness,
        attemptedWork,
        hotState,
        validated,
        evidence,
        publishedRoot,
        recordedRoots
        >>

Execute(transaction) ==
    /\ transaction \in Transactions
    /\ phase[transaction] = "Reset"
    /\ phase' = [phase EXCEPT ![transaction] = "Worked"]
    /\ budgetWitness' = [budgetWitness EXCEPT ![transaction] = 2]
    /\ attemptedWork' = [attemptedWork EXCEPT ![transaction] = 2]
    /\ hotState' = [hotState EXCEPT ![transaction] = 1]
    /\ UNCHANGED <<
        baseRoot,
        localRoot,
        resultWitness,
        validated,
        evidence,
        publishedRoot,
        recordedRoots,
        currentPointer
        >>

ReducerFail(transaction) ==
    /\ transaction \in Transactions
    /\ phase[transaction] = "Worked"
    /\ phase' = [phase EXCEPT ![transaction] = "ReducerRejected"]
    /\ resultWitness' = [resultWitness EXCEPT
        ![transaction] =
            IF Defect = "ReducerErasesAttempt"
            THEN 0
            ELSE attemptedWork[transaction]]
    /\ localRoot' = [localRoot EXCEPT ![transaction] = baseRoot[transaction]]
    /\ hotState' = [hotState EXCEPT ![transaction] = 0]
    /\ currentPointer' = baseRoot[transaction]
    /\ UNCHANGED <<
        baseRoot,
        budgetWitness,
        attemptedWork,
        validated,
        evidence,
        publishedRoot,
        recordedRoots
        >>

Checkpoint(transaction) ==
    /\ transaction \in Transactions
    /\ phase[transaction] = "Worked"
    /\ phase' = [phase EXCEPT ![transaction] = "Checkpointed"]
    /\ localRoot' = [localRoot EXCEPT ![transaction] = CandidateRoot(transaction)]
    /\ recordedRoots' = recordedRoots \union {CandidateRoot(transaction)}
    /\ currentPointer' = CandidateRoot(transaction)
    /\ UNCHANGED <<
        baseRoot,
        budgetWitness,
        resultWitness,
        attemptedWork,
        hotState,
        validated,
        evidence,
        publishedRoot
        >>

ValidateCandidate(transaction) ==
    /\ transaction \in Transactions
    /\ phase[transaction] = "Checkpointed"
    /\ localRoot[transaction] = CandidateRoot(transaction)
    /\ phase' = [phase EXCEPT ![transaction] = "Validated"]
    /\ validated' = validated \union {transaction}
    /\ evidence' =
        IF /\ Defect = "ReplayEvidenceBeforeFinalValidation"
           /\ IsReplay(transaction)
        THEN evidence \union {transaction}
        ELSE evidence
    /\ UNCHANGED <<
        baseRoot,
        localRoot,
        budgetWitness,
        resultWitness,
        attemptedWork,
        hotState,
        publishedRoot,
        recordedRoots,
        currentPointer
        >>

RollbackRoots(transaction) ==
    IF Defect = "RollbackDeletesForeignRoot"
    THEN {BaseRoot, CandidateRoot(transaction)}
    ELSE recordedRoots

RejectCheckpoint(transaction) ==
    /\ transaction \in Transactions
    /\ phase[transaction] = "Checkpointed"
    /\ phase' = [phase EXCEPT
        ![transaction] = IF IsReplay(transaction) THEN "ReplayRejected" ELSE "PlayRejected"]
    /\ resultWitness' = [resultWitness EXCEPT
        ![transaction] = budgetWitness[transaction]]
    /\ IF \/ /\ Defect = "PlayValidationNoRollback"
              /\ ~IsReplay(transaction)
          \/ /\ Defect = "ReplayValidationNoRollback"
              /\ IsReplay(transaction)
       THEN UNCHANGED <<localRoot, hotState, recordedRoots, currentPointer>>
       ELSE /\ localRoot' = [localRoot EXCEPT ![transaction] = baseRoot[transaction]]
            /\ hotState' = [hotState EXCEPT ![transaction] = 0]
            /\ recordedRoots' = RollbackRoots(transaction)
            /\ currentPointer' = baseRoot[transaction]
    /\ validated' = validated \ {transaction}
    /\ evidence' = evidence \ {transaction}
    /\ UNCHANGED <<baseRoot, budgetWitness, attemptedWork, publishedRoot>>

RejectFinalValidation(transaction) ==
    /\ transaction \in Transactions
    /\ phase[transaction] = "Validated"
    /\ phase' = [phase EXCEPT
        ![transaction] = IF IsReplay(transaction) THEN "ReplayRejected" ELSE "PlayRejected"]
    /\ resultWitness' = [resultWitness EXCEPT
        ![transaction] = budgetWitness[transaction]]
    /\ localRoot' = [localRoot EXCEPT ![transaction] = baseRoot[transaction]]
    /\ hotState' = [hotState EXCEPT ![transaction] = 0]
    /\ validated' = validated \ {transaction}
    /\ evidence' =
        IF /\ Defect = "ReplayEvidenceBeforeFinalValidation"
           /\ IsReplay(transaction)
        THEN evidence
        ELSE evidence \ {transaction}
    /\ recordedRoots' = RollbackRoots(transaction)
    /\ currentPointer' = baseRoot[transaction]
    /\ UNCHANGED <<baseRoot, budgetWitness, attemptedWork, publishedRoot>>

Accept(transaction) ==
    /\ transaction \in Transactions
    /\ phase[transaction] = "Validated"
    /\ phase' = [phase EXCEPT ![transaction] = "Accepted"]
    /\ resultWitness' = [resultWitness EXCEPT
        ![transaction] = budgetWitness[transaction]]
    /\ evidence' = evidence \union {transaction}
    /\ publishedRoot' = [publishedRoot EXCEPT
        ![transaction] =
            IF Defect = "SharedCurrentRootPublication"
            THEN currentPointer
            ELSE localRoot[transaction]]
    /\ UNCHANGED <<
        baseRoot,
        localRoot,
        budgetWitness,
        attemptedWork,
        hotState,
        validated,
        recordedRoots,
        currentPointer
        >>

Crash(transaction) ==
    /\ transaction \in Transactions
    /\ phase[transaction] \in {"Captured", "Reset", "Worked", "Checkpointed", "Validated"}
    /\ phase' = [phase EXCEPT ![transaction] = "Crashed"]
    /\ localRoot' = [localRoot EXCEPT ![transaction] = baseRoot[transaction]]
    /\ hotState' = [hotState EXCEPT ![transaction] = 0]
    /\ validated' = validated \ {transaction}
    /\ evidence' = evidence \ {transaction}
    /\ currentPointer' = baseRoot[transaction]
    /\ UNCHANGED <<
        baseRoot,
        budgetWitness,
        resultWitness,
        attemptedWork,
        publishedRoot,
        recordedRoots
        >>

Restart(transaction) ==
    /\ transaction \in Transactions
    /\ phase[transaction] = "Crashed"
    /\ phase' = [phase EXCEPT ![transaction] = "Idle"]
    /\ budgetWitness' = [budgetWitness EXCEPT ![transaction] = 3]
    /\ resultWitness' = [resultWitness EXCEPT ![transaction] = 0]
    /\ attemptedWork' = [attemptedWork EXCEPT ![transaction] = 0]
    /\ UNCHANGED <<
        baseRoot,
        localRoot,
        hotState,
        validated,
        evidence,
        publishedRoot,
        recordedRoots,
        currentPointer
        >>

TerminalStutter ==
    /\ \A transaction \in Transactions :
        phase[transaction] \in {
            "ParserRejected",
            "ReducerRejected",
            "PlayRejected",
            "ReplayRejected",
            "Accepted"
        }
    /\ UNCHANGED vars

Next ==
    \/ \E transaction \in Transactions : ParserFail(transaction)
    \/ \E transaction \in Transactions : CaptureBase(transaction)
    \/ \E transaction \in Transactions : ResetToCapturedBase(transaction)
    \/ \E transaction \in Transactions : Execute(transaction)
    \/ \E transaction \in Transactions : ReducerFail(transaction)
    \/ \E transaction \in Transactions : Checkpoint(transaction)
    \/ \E transaction \in Transactions : ValidateCandidate(transaction)
    \/ \E transaction \in Transactions : RejectCheckpoint(transaction)
    \/ \E transaction \in Transactions : RejectFinalValidation(transaction)
    \/ \E transaction \in Transactions : Accept(transaction)
    \/ \E transaction \in Transactions : Crash(transaction)
    \/ \E transaction \in Transactions : Restart(transaction)
    \/ TerminalStutter

TypeOK ==
    /\ phase \in [Transactions -> {
        "Idle",
        "Captured",
        "Reset",
        "Worked",
        "Checkpointed",
        "Validated",
        "ParserRejected",
        "ReducerRejected",
        "PlayRejected",
        "ReplayRejected",
        "Accepted",
        "Crashed"
        }]
    /\ baseRoot \in [Transactions -> {BaseRoot, 1, 2}]
    /\ localRoot \in [Transactions -> {BaseRoot, 1, 2}]
    /\ budgetWitness \in [Transactions -> Nat]
    /\ resultWitness \in [Transactions -> Nat]
    /\ attemptedWork \in [Transactions -> Nat]
    /\ hotState \in [Transactions -> 0..1]
    /\ validated \subseteq Transactions
    /\ evidence \subseteq Transactions
    /\ publishedRoot \in [Transactions -> {BaseRoot, 1, 2}]
    /\ recordedRoots \subseteq {BaseRoot, 1, 2}
    /\ currentPointer \in {BaseRoot, 1, 2}

ExplicitBaseAuthority ==
    \A transaction \in Transactions :
        phase[transaction] # "Idle" => baseRoot[transaction] = BaseRoot

CurrentPointerNamesRecordedRoot == currentPointer \in recordedRoots

CheckpointedRootsRemainRecorded ==
    \A transaction \in Transactions :
        phase[transaction] \in {"Checkpointed", "Validated", "Accepted"} =>
            CandidateRoot(transaction) \in recordedRoots

ParserFailureHasNoWitness ==
    \A transaction \in Transactions :
        phase[transaction] = "ParserRejected" =>
            /\ resultWitness[transaction] = 0
            /\ attemptedWork[transaction] = 0
            /\ transaction \notin evidence

ReducerFailureRetainsAttemptedWork ==
    \A transaction \in Transactions :
        phase[transaction] = "ReducerRejected" =>
            /\ attemptedWork[transaction] > 0
            /\ resultWitness[transaction] = attemptedWork[transaction]
            /\ localRoot[transaction] = baseRoot[transaction]
            /\ hotState[transaction] = 0
            /\ transaction \notin evidence

RejectedTransactionsAreStateAtomic ==
    \A transaction \in Transactions :
        phase[transaction] \in {"PlayRejected", "ReplayRejected"} =>
            /\ localRoot[transaction] = baseRoot[transaction]
            /\ hotState[transaction] = 0
            /\ transaction \notin evidence

EvidenceRequiresAcceptance ==
    \A transaction \in evidence : phase[transaction] = "Accepted"

AcceptedRootsAreOwnedAndRecorded ==
    \A transaction \in Transactions :
        phase[transaction] = "Accepted" =>
            /\ transaction \in validated
            /\ transaction \in evidence
            /\ publishedRoot[transaction] = CandidateRoot(transaction)
            /\ publishedRoot[transaction] \in recordedRoots

CrashedTransactionsPublishNothing ==
    \A transaction \in Transactions :
        phase[transaction] = "Crashed" =>
            /\ transaction \notin validated
            /\ transaction \notin evidence
            /\ localRoot[transaction] = baseRoot[transaction]
            /\ hotState[transaction] = 0

FreshWitnessPerTransaction ==
    \A transaction \in Transactions :
        phase[transaction] \in {"Captured", "Reset"} =>
            /\ budgetWitness[transaction] = 0
            /\ attemptedWork[transaction] = 0
            /\ resultWitness[transaction] = 0

Spec == Init /\ [][Next]_vars

=============================================================================
