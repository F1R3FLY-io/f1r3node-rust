------------------ MODULE EvaluationTransactionIsolation ------------------
EXTENDS Integers

CONSTANT
    \* @type: Str;
    Defect

ASSUME Defect \in {
    "None",
    "ParserReusesPriorWitness",
    "ReducerErasesAttempt",
    "PlayValidationNoRollback",
    "ReplayValidationNoRollback",
    "ReplayEvidenceBeforeFinalValidation"
}

VARIABLES
    \* @type: Str;
    phase,
    \* @type: Int;
    budgetWitness,
    \* @type: Int;
    resultWitness,
    \* @type: Int;
    attemptedWork,
    \* @type: Int;
    rspaceState,
    \* @type: Int;
    durableState,
    \* @type: Int;
    mergeableEvidence

vars == <<
    phase,
    budgetWitness,
    resultWitness,
    attemptedWork,
    rspaceState,
    durableState,
    mergeableEvidence
>>

Init ==
    /\ phase = "Idle"
    /\ budgetWitness = 3
    /\ resultWitness = 0
    /\ attemptedWork = 0
    /\ rspaceState = 0
    /\ durableState = 0
    /\ mergeableEvidence = 0

WorkedPhase(kind) ==
    CASE kind = "Reducer" -> "ReducerWorked"
    [] kind = "Play" -> "PlayWorked"
    [] OTHER -> "ReplayWorked"

AcceptedPhase(kind) ==
    IF kind = "Play" THEN "PlayAccepted" ELSE "ReplayAccepted"

AcceptablePhase(kind) ==
    IF kind = "Play" THEN "PlayWorked" ELSE "ReplayEffectsValidated"

ParseFail ==
    /\ phase = "Idle"
    /\ phase' = "ParserRejected"
    /\ resultWitness' = IF Defect = "ParserReusesPriorWitness" THEN budgetWitness ELSE 0
    /\ UNCHANGED <<budgetWitness, attemptedWork, rspaceState, durableState, mergeableEvidence>>

Begin(kind) ==
    /\ phase = "Idle"
    /\ kind \in {"Reducer", "Play", "Replay"}
    /\ phase' = kind
    /\ budgetWitness' = 0
    /\ resultWitness' = 0
    /\ attemptedWork' = 0
    /\ rspaceState' = 0
    /\ mergeableEvidence' = 0
    /\ UNCHANGED durableState

DoWork(kind) ==
    /\ phase = kind
    /\ kind \in {"Reducer", "Play", "Replay"}
    /\ phase' = WorkedPhase(kind)
    /\ budgetWitness' = 2
    /\ attemptedWork' = 2
    /\ rspaceState' = 1
    /\ UNCHANGED <<resultWitness, durableState, mergeableEvidence>>

ReducerFail ==
    /\ phase = "ReducerWorked"
    /\ phase' = "ReducerRejected"
    /\ resultWitness' = IF Defect = "ReducerErasesAttempt" THEN 0 ELSE attemptedWork
    /\ rspaceState' = 0
    /\ UNCHANGED <<budgetWitness, attemptedWork, durableState, mergeableEvidence>>

PlayValidationFail ==
    /\ phase = "PlayWorked"
    /\ phase' = "PlayRejected"
    /\ resultWitness' = budgetWitness
    /\ IF Defect = "PlayValidationNoRollback"
       THEN UNCHANGED rspaceState
       ELSE rspaceState' = 0
    /\ UNCHANGED <<budgetWitness, attemptedWork, durableState, mergeableEvidence>>

ReplayValidationFail ==
    /\ phase = "ReplayCheckpointed"
    /\ phase' = "ReplayRejected"
    /\ resultWitness' = budgetWitness
    /\ IF Defect = "ReplayValidationNoRollback"
       THEN UNCHANGED <<rspaceState, durableState, mergeableEvidence>>
       ELSE /\ rspaceState' = 0
            /\ durableState' = 0
            /\ mergeableEvidence' = 0
    /\ UNCHANGED <<budgetWitness, attemptedWork>>

PublishReplayCheckpoint ==
    /\ phase = "ReplayWorked"
    /\ phase' = "ReplayCheckpointed"
    /\ durableState' = rspaceState
    /\ UNCHANGED <<budgetWitness, resultWitness, attemptedWork, rspaceState, mergeableEvidence>>

ValidateReplayEffects ==
    /\ phase = "ReplayCheckpointed"
    /\ phase' = "ReplayEffectsValidated"
    /\ mergeableEvidence' =
        IF Defect = "ReplayEvidenceBeforeFinalValidation" THEN 1 ELSE 0
    /\ UNCHANGED <<budgetWitness, resultWitness, attemptedWork, rspaceState, durableState>>

ReplayFinalValidationFail ==
    /\ phase = "ReplayEffectsValidated"
    /\ phase' = "ReplayRejected"
    /\ resultWitness' = budgetWitness
    /\ rspaceState' = 0
    /\ durableState' = 0
    /\ UNCHANGED <<budgetWitness, attemptedWork, mergeableEvidence>>

Accept(kind) ==
    /\ phase = AcceptablePhase(kind)
    /\ kind \in {"Play", "Replay"}
    /\ phase' = AcceptedPhase(kind)
    /\ resultWitness' = budgetWitness
    /\ durableState' = rspaceState
    /\ IF kind = "Replay"
       THEN mergeableEvidence' = 1
       ELSE UNCHANGED mergeableEvidence
    /\ UNCHANGED <<budgetWitness, attemptedWork, rspaceState>>

TerminalStutter ==
    /\ phase \in {
        "ParserRejected", "ReducerRejected", "PlayRejected", "ReplayRejected",
        "PlayAccepted", "ReplayAccepted"
       }
    /\ UNCHANGED vars

Next ==
    \/ ParseFail
    \/ \E kind \in {"Reducer", "Play", "Replay"} : Begin(kind)
    \/ \E kind \in {"Reducer", "Play", "Replay"} : DoWork(kind)
    \/ ReducerFail
    \/ PlayValidationFail
    \/ PublishReplayCheckpoint
    \/ ReplayValidationFail
    \/ ValidateReplayEffects
    \/ ReplayFinalValidationFail
    \/ \E kind \in {"Play", "Replay"} : Accept(kind)
    \/ TerminalStutter

TypeOK ==
    /\ phase \in {
        "Idle", "Reducer", "Play", "Replay", "ReducerWorked", "PlayWorked",
        "ReplayWorked", "ReplayCheckpointed", "ReplayEffectsValidated",
        "ParserRejected", "ReducerRejected", "PlayRejected",
        "ReplayRejected", "PlayAccepted", "ReplayAccepted"
       }
    /\ budgetWitness \in Nat
    /\ resultWitness \in Nat
    /\ attemptedWork \in Nat
    /\ rspaceState \in 0..1
    /\ durableState \in 0..1
    /\ mergeableEvidence \in 0..1

ParserFailureHasNoWitness ==
    phase = "ParserRejected" =>
        /\ resultWitness = 0
        /\ attemptedWork = 0
        /\ rspaceState = 0
        /\ durableState = 0
        /\ mergeableEvidence = 0

ReducerFailureRetainsAttemptedWork ==
    phase = "ReducerRejected" =>
        /\ attemptedWork > 0
        /\ resultWitness = attemptedWork
        /\ rspaceState = 0
        /\ durableState = 0
        /\ mergeableEvidence = 0

RejectedPlayIsStateAtomic ==
    phase = "PlayRejected" =>
        /\ rspaceState = 0
        /\ durableState = 0
        /\ mergeableEvidence = 0

RejectedReplayIsStateAtomic ==
    phase = "ReplayRejected" =>
        /\ rspaceState = 0
        /\ durableState = 0
        /\ mergeableEvidence = 0

RejectedReplayPublishesNoEvidence ==
    phase = "ReplayRejected" => mergeableEvidence = 0

AcceptedReplayPublishesValidatedEvidence ==
    phase = "ReplayAccepted" => mergeableEvidence = 1

ExecutionStartsWithFreshWitness ==
    phase \in {"Reducer", "Play", "Replay"} =>
        /\ budgetWitness = 0
        /\ attemptedWork = 0
        /\ resultWitness = 0

=============================================================================
