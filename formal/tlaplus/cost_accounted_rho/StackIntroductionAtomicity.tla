-------------------- MODULE StackIntroductionAtomicity --------------------
EXTENDS Integers, FiniteSets

CONSTANTS
    \* @type: Str;
    left,
    \* @type: Str;
    right,
    \* @type: Set(Str);
    Operations,
    \* @type: Str -> Int;
    Cells,
    \* @type: Str -> Bool;
    ByteFits,
    \* @type: Str -> Bool;
    ContinuationSucceeds,
    \* @type: Int;
    InitialCells,
    \* @type: Str;
    Defect

ASSUME Operations = {left, right}
ASSUME left # right
ASSUME Cells \in [Operations -> Nat \ {0}]
ASSUME ByteFits \in [Operations -> BOOLEAN]
ASSUME ContinuationSucceeds \in [Operations -> BOOLEAN]
ASSUME InitialCells = Cells[left] + Cells[right]
ASSUME Defect \in {
    "None",
    "ExposePrepared",
    "OmitAbort",
    "FallibleBirth",
    "OmitNestedProduce",
    "OmitDeployRollback",
    "ReplayOmission"
}

ZeroVector == [operation \in Operations |-> 0]

VectorTotal(vector) == vector[left] + vector[right]

CommittedOperations(vector) ==
    {operation \in Operations : vector[operation] > 0}

Terminal(status) == status \in {"Committed", "Rejected"}

VARIABLES
    \* @type: Str -> Str;
    phase,
    \* @type: Int;
    available,
    \* @type: Str -> Int;
    pending,
    \* @type: Str -> Int;
    committed,
    \* @type: Set(Str);
    rspace,
    \* @type: Set(Str);
    births,
    \* @type: Set(Str);
    rejected,
    \* @type: Str -> Int;
    byteCharged,
    \* @type: Bool;
    deployFailed,
    \* @type: Set(Str);
    rolledBack,
    \* @type: Bool;
    replayDone,
    \* @type: Str -> Int;
    replayCommitted,
    \* @type: Set(Str);
    replayRSpace,
    \* @type: Set(Str);
    replayBirths,
    \* @type: Str -> Int;
    replayByteCharged,
    \* @type: Bool;
    replayDeployFailed

vars == <<
    phase,
    available,
    pending,
    committed,
    rspace,
    births,
    rejected,
    byteCharged,
    deployFailed,
    rolledBack,
    replayDone,
    replayCommitted,
    replayRSpace,
    replayBirths,
    replayByteCharged,
    replayDeployFailed
>>

Init ==
    /\ phase = [operation \in Operations |-> "Idle"]
    /\ available = InitialCells
    /\ pending = ZeroVector
    /\ committed = ZeroVector
    /\ rspace = {}
    /\ births = {}
    /\ rejected = {}
    /\ byteCharged = ZeroVector
    /\ deployFailed = FALSE
    /\ rolledBack = {}
    /\ replayDone = FALSE
    /\ replayCommitted = ZeroVector
    /\ replayRSpace = {}
    /\ replayBirths = {}
    /\ replayByteCharged = ZeroVector
    /\ replayDeployFailed = FALSE

Prepare(operation) ==
    /\ phase[operation] = "Idle"
    /\ available >= Cells[operation]
    /\ phase' = [phase EXCEPT ![operation] = "Prepared"]
    /\ available' = available - Cells[operation]
    /\ IF Defect = "ExposePrepared"
       THEN /\ committed' = [committed EXCEPT ![operation] = Cells[operation]]
            /\ UNCHANGED pending
       ELSE /\ pending' = [pending EXCEPT ![operation] = Cells[operation]]
            /\ UNCHANGED committed
    /\ UNCHANGED <<
        rspace, births, rejected, byteCharged, deployFailed, rolledBack,
        replayDone, replayCommitted, replayRSpace, replayBirths,
        replayByteCharged, replayDeployFailed
       >>

AcceptBytes(operation) ==
    /\ phase[operation] = "Prepared"
    /\ ByteFits[operation]
    /\ phase' = [phase EXCEPT ![operation] = "ByteAccepted"]
    /\ byteCharged' = [byteCharged EXCEPT ![operation] = 1]
    /\ UNCHANGED <<
        available, pending, committed, rspace, births, rejected,
        deployFailed, rolledBack, replayDone, replayCommitted, replayRSpace,
        replayBirths, replayByteCharged, replayDeployFailed
       >>

RejectBytes(operation) ==
    /\ phase[operation] = "Prepared"
    /\ ~ByteFits[operation]
    /\ phase' = [phase EXCEPT ![operation] = "Rejected"]
    /\ rejected' = rejected \cup {operation}
    /\ IF Defect = "OmitAbort"
       THEN /\ UNCHANGED <<available, pending, committed>>
       ELSE /\ available' = available + pending[operation] + committed[operation]
            /\ pending' = [pending EXCEPT ![operation] = 0]
            /\ committed' = [committed EXCEPT ![operation] = 0]
    /\ UNCHANGED <<
        rspace, births, byteCharged, deployFailed, rolledBack, replayDone,
        replayCommitted, replayRSpace, replayBirths, replayByteCharged,
        replayDeployFailed
       >>

MutateRSpace(operation) ==
    /\ phase[operation] = "ByteAccepted"
    /\ phase' = [phase EXCEPT ![operation] = "Mutated"]
    /\ rspace' = rspace \cup {operation}
    /\ UNCHANGED <<
        available, pending, committed, births, rejected, byteCharged,
        deployFailed, rolledBack, replayDone, replayCommitted, replayRSpace,
        replayBirths, replayByteCharged, replayDeployFailed
       >>

Commit(operation) ==
    /\ phase[operation] = "Mutated"
    /\ ContinuationSucceeds[operation]
    /\ phase' = [phase EXCEPT ![operation] = "Committed"]
    /\ committed' = [committed EXCEPT ![operation] = pending[operation]]
    /\ pending' = [pending EXCEPT ![operation] = 0]
    /\ IF Defect = "FallibleBirth"
       THEN UNCHANGED births
       ELSE births' = births \cup {operation}
    /\ UNCHANGED <<
        available, rspace, rejected, byteCharged, deployFailed, rolledBack,
        replayDone, replayCommitted, replayRSpace, replayBirths,
        replayByteCharged, replayDeployFailed
       >>

AbortAfterMutation(operation) ==
    /\ phase[operation] = "Mutated"
    /\ ~ContinuationSucceeds[operation]
    /\ phase' = [phase EXCEPT ![operation] = "Rejected"]
    /\ available' = available + pending[operation]
    /\ pending' = [pending EXCEPT ![operation] = 0]
    /\ rspace' = rspace \ {operation}
    /\ rejected' = rejected \cup {operation}
    /\ UNCHANGED <<
        committed, births, byteCharged, deployFailed, rolledBack, replayDone,
        replayCommitted, replayRSpace, replayBirths, replayByteCharged,
        replayDeployFailed
       >>

FailDeploy ==
    /\ ~deployFailed
    /\ ~replayDone
    /\ \A operation \in Operations : Terminal(phase[operation])
    /\ CommittedOperations(committed) # {}
    /\ deployFailed' = TRUE
    /\ rolledBack' = CommittedOperations(committed)
    /\ IF Defect = "OmitDeployRollback"
       THEN UNCHANGED <<available, pending, committed, rspace, births>>
       ELSE /\ available' = available + VectorTotal(committed)
            /\ pending' = ZeroVector
            /\ committed' = ZeroVector
            /\ rspace' = {}
            /\ births' = {}
    /\ UNCHANGED <<
        phase, rejected, byteCharged, replayDone, replayCommitted,
        replayRSpace, replayBirths, replayByteCharged, replayDeployFailed
       >>

Replay ==
    /\ ~replayDone
    /\ \A operation \in Operations : Terminal(phase[operation])
    /\ replayDone' = TRUE
    /\ IF Defect = "ReplayOmission"
       THEN /\ replayCommitted' = ZeroVector
            /\ replayRSpace' = {}
            /\ replayBirths' = {}
            /\ replayByteCharged' = ZeroVector
            /\ replayDeployFailed' = FALSE
       ELSE /\ replayCommitted' = committed
            /\ replayRSpace' = rspace
            /\ replayBirths' = births
            /\ replayByteCharged' = byteCharged
            /\ replayDeployFailed' = deployFailed
    /\ UNCHANGED <<
        phase, available, pending, committed, rspace, births, rejected,
        byteCharged, deployFailed, rolledBack
       >>

TerminalStutter ==
    /\ replayDone
    /\ UNCHANGED vars

Next ==
    \/ \E operation \in Operations : Prepare(operation)
    \/ \E operation \in Operations : AcceptBytes(operation)
    \/ \E operation \in Operations : RejectBytes(operation)
    \/ \E operation \in Operations : MutateRSpace(operation)
    \/ \E operation \in Operations : Commit(operation)
    \/ \E operation \in Operations : AbortAfterMutation(operation)
    \/ FailDeploy
    \/ Replay
    \/ TerminalStutter

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ \A operation \in Operations : WF_vars(Prepare(operation))
    /\ \A operation \in Operations : WF_vars(AcceptBytes(operation))
    /\ \A operation \in Operations : WF_vars(RejectBytes(operation))
    /\ \A operation \in Operations : WF_vars(MutateRSpace(operation))
    /\ \A operation \in Operations : WF_vars(Commit(operation))
    /\ \A operation \in Operations : WF_vars(AbortAfterMutation(operation))
    /\ WF_vars(FailDeploy)
    /\ WF_vars(Replay)

TypeOK ==
    /\ phase \in [Operations -> {
        "Idle", "Prepared", "ByteAccepted", "Mutated", "Committed", "Rejected"
       }]
    /\ available \in Nat
    /\ pending \in [Operations -> Nat]
    /\ committed \in [Operations -> Nat]
    /\ rspace \subseteq Operations
    /\ births \subseteq Operations
    /\ rejected \subseteq Operations
    /\ byteCharged \in [Operations -> Nat]
    /\ deployFailed \in BOOLEAN
    /\ rolledBack \subseteq Operations
    /\ replayDone \in BOOLEAN
    /\ replayCommitted \in [Operations -> Nat]
    /\ replayRSpace \subseteq Operations
    /\ replayBirths \subseteq Operations
    /\ replayByteCharged \in [Operations -> Nat]
    /\ replayDeployFailed \in BOOLEAN

PhysicalCapacityIsConserved ==
    available + VectorTotal(pending) + VectorTotal(committed) = InitialCells

PreparedAuthorityIsNotWitnessVisible ==
    CommittedOperations(committed) \subseteq rspace

EveryCommittedStackHasOneBirth ==
    CommittedOperations(committed) = births

RejectedOperationIsEffectFree ==
    \A operation \in rejected :
        /\ pending[operation] = 0
        /\ committed[operation] = 0
        /\ operation \notin rspace
        /\ operation \notin births

CommittedStackIsComplete ==
    \A operation \in CommittedOperations(committed) :
        committed[operation] = Cells[operation]

CausallyExtractedOperations ==
    IF Defect = "OmitNestedProduce"
    THEN rspace \ {right}
    ELSE rspace

EveryCommittedStackIsCausallyExtracted ==
    CommittedOperations(committed) \subseteq CausallyExtractedOperations

FailedDeployHasNoLinearEffects ==
    deployFailed =>
        /\ pending = ZeroVector
        /\ committed = ZeroVector
        /\ rspace = {}
        /\ births = {}

RolledBackWorkRetainsByteCharge ==
    \A operation \in rolledBack : byteCharged[operation] = 1

ReplayMatchesCommit ==
    replayDone =>
        /\ replayCommitted = committed
        /\ replayRSpace = rspace
        /\ replayBirths = births
        /\ replayByteCharged = byteCharged
        /\ replayDeployFailed = deployFailed

EventuallyReplayed == <>replayDone

OperationsDef == {left, right}
CellsDef == [operation \in Operations |-> IF operation = left THEN 2 ELSE 1]
ByteFitsDef == [operation \in Operations |-> operation = right]
ContinuationSucceedsDef == [operation \in Operations |-> TRUE]

=============================================================================
