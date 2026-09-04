----------------------- MODULE EvaluationRootIsolation -----------------------
EXTENDS FiniteSets, Naturals

CONSTANT Defect

Evaluations == {1, 2}
BaseRoot == 0
CandidateRoot(evaluation) == evaluation

ASSUME Defect \in {
    "None",
    "SharedBaseAuthority",
    "SharedRootPublication",
    "RollbackRetainsCandidate",
    "RollbackDeletesForeignRoot",
    "EvidenceBeforeAcceptance"
}

VARIABLES
    phase,
    baseRoot,
    localRoot,
    validated,
    evidence,
    publishedRoot,
    recordedRoots,
    globalRoot

vars == <<
    phase,
    baseRoot,
    localRoot,
    validated,
    evidence,
    publishedRoot,
    recordedRoots,
    globalRoot
>>

Init ==
    /\ phase = [evaluation \in Evaluations |-> "Idle"]
    /\ baseRoot = [evaluation \in Evaluations |-> BaseRoot]
    /\ localRoot = [evaluation \in Evaluations |-> BaseRoot]
    /\ validated = {}
    /\ evidence = {}
    /\ publishedRoot = [evaluation \in Evaluations |-> BaseRoot]
    /\ recordedRoots = {BaseRoot}
    /\ globalRoot = BaseRoot

Capture(evaluation) ==
    /\ evaluation \in Evaluations
    /\ phase[evaluation] = "Idle"
    /\ phase' = [phase EXCEPT ![evaluation] = "Captured"]
    /\ baseRoot' = [baseRoot EXCEPT
        ![evaluation] =
            IF Defect = "SharedBaseAuthority" THEN globalRoot ELSE BaseRoot]
    /\ localRoot' = [localRoot EXCEPT ![evaluation] = baseRoot'[evaluation]]
    /\ UNCHANGED <<validated, evidence, publishedRoot, recordedRoots, globalRoot>>

Execute(evaluation) ==
    /\ evaluation \in Evaluations
    /\ phase[evaluation] = "Captured"
    /\ phase' = [phase EXCEPT ![evaluation] = "Executed"]
    /\ UNCHANGED <<
        baseRoot,
        localRoot,
        validated,
        evidence,
        publishedRoot,
        recordedRoots,
        globalRoot
        >>

Checkpoint(evaluation) ==
    /\ evaluation \in Evaluations
    /\ phase[evaluation] = "Executed"
    /\ phase' = [phase EXCEPT ![evaluation] = "Checkpointed"]
    /\ localRoot' = [localRoot EXCEPT ![evaluation] = CandidateRoot(evaluation)]
    /\ recordedRoots' = recordedRoots \union {CandidateRoot(evaluation)}
    /\ globalRoot' = CandidateRoot(evaluation)
    /\ UNCHANGED <<baseRoot, validated, evidence, publishedRoot>>

Validate(evaluation) ==
    /\ evaluation \in Evaluations
    /\ phase[evaluation] = "Checkpointed"
    /\ phase' = [phase EXCEPT ![evaluation] = "Validated"]
    /\ validated' = validated \union {evaluation}
    /\ evidence' =
        IF Defect = "EvidenceBeforeAcceptance"
        THEN evidence \union {evaluation}
        ELSE evidence
    /\ UNCHANGED <<
        baseRoot,
        localRoot,
        publishedRoot,
        recordedRoots,
        globalRoot
        >>

SafeRecordedRootsAfterRollback(evaluation) ==
    recordedRoots \ {CandidateRoot(evaluation)}

Reject(evaluation) ==
    /\ evaluation \in Evaluations
    /\ phase[evaluation] \in {"Checkpointed", "Validated"}
    /\ phase' = [phase EXCEPT ![evaluation] = "Rejected"]
    /\ localRoot' = [localRoot EXCEPT
        ![evaluation] =
            IF Defect = "RollbackRetainsCandidate"
            THEN localRoot[evaluation]
            ELSE baseRoot[evaluation]]
    /\ validated' = validated \ {evaluation}
    /\ evidence' = evidence \ {evaluation}
    /\ recordedRoots' =
        IF Defect = "RollbackDeletesForeignRoot"
        THEN {BaseRoot, CandidateRoot(evaluation)}
        ELSE SafeRecordedRootsAfterRollback(evaluation)
    /\ globalRoot' = baseRoot[evaluation]
    /\ UNCHANGED <<baseRoot, publishedRoot>>

Accept(evaluation) ==
    /\ evaluation \in Evaluations
    /\ phase[evaluation] = "Validated"
    /\ phase' = [phase EXCEPT ![evaluation] = "Accepted"]
    /\ evidence' = evidence \union {evaluation}
    /\ publishedRoot' = [publishedRoot EXCEPT
        ![evaluation] =
            IF Defect = "SharedRootPublication"
            THEN globalRoot
            ELSE localRoot[evaluation]]
    /\ UNCHANGED <<
        baseRoot,
        localRoot,
        validated,
        recordedRoots,
        globalRoot
        >>

Crash(evaluation) ==
    /\ evaluation \in Evaluations
    /\ phase[evaluation] \in {"Captured", "Executed", "Checkpointed", "Validated"}
    /\ phase' = [phase EXCEPT ![evaluation] = "Crashed"]
    /\ localRoot' = [localRoot EXCEPT ![evaluation] = baseRoot[evaluation]]
    /\ validated' = validated \ {evaluation}
    /\ evidence' = evidence \ {evaluation}
    /\ recordedRoots' = SafeRecordedRootsAfterRollback(evaluation)
    /\ globalRoot' = baseRoot[evaluation]
    /\ UNCHANGED <<baseRoot, publishedRoot>>

Restart(evaluation) ==
    /\ evaluation \in Evaluations
    /\ phase[evaluation] = "Crashed"
    /\ phase' = [phase EXCEPT ![evaluation] = "Idle"]
    /\ UNCHANGED <<
        baseRoot,
        localRoot,
        validated,
        evidence,
        publishedRoot,
        recordedRoots,
        globalRoot
        >>

TerminalStutter ==
    /\ \A evaluation \in Evaluations :
        phase[evaluation] \in {"Rejected", "Accepted"}
    /\ UNCHANGED vars

Next ==
    \/ \E evaluation \in Evaluations : Capture(evaluation)
    \/ \E evaluation \in Evaluations : Execute(evaluation)
    \/ \E evaluation \in Evaluations : Checkpoint(evaluation)
    \/ \E evaluation \in Evaluations : Validate(evaluation)
    \/ \E evaluation \in Evaluations : Reject(evaluation)
    \/ \E evaluation \in Evaluations : Accept(evaluation)
    \/ \E evaluation \in Evaluations : Crash(evaluation)
    \/ \E evaluation \in Evaluations : Restart(evaluation)
    \/ TerminalStutter

TypeOK ==
    /\ phase \in [Evaluations -> {
        "Idle", "Captured", "Executed", "Checkpointed", "Validated",
        "Rejected", "Accepted", "Crashed"
        }]
    /\ baseRoot \in [Evaluations -> {BaseRoot, 1, 2}]
    /\ localRoot \in [Evaluations -> {BaseRoot, 1, 2}]
    /\ validated \subseteq Evaluations
    /\ evidence \subseteq Evaluations
    /\ publishedRoot \in [Evaluations -> {BaseRoot, 1, 2}]
    /\ recordedRoots \subseteq {BaseRoot, 1, 2}
    /\ globalRoot \in {BaseRoot, 1, 2}

ExplicitBaseAuthority ==
    \A evaluation \in Evaluations :
        phase[evaluation] # "Idle" => baseRoot[evaluation] = BaseRoot

CheckpointedRootsRemainRecorded ==
    \A evaluation \in Evaluations :
        phase[evaluation] \in {"Checkpointed", "Validated", "Accepted"} =>
            CandidateRoot(evaluation) \in recordedRoots

RejectedEvaluationsAreStateAtomic ==
    \A evaluation \in Evaluations :
        phase[evaluation] = "Rejected" =>
            /\ localRoot[evaluation] = baseRoot[evaluation]
            /\ evaluation \notin validated
            /\ evaluation \notin evidence

EvidenceRequiresAcceptance ==
    \A evaluation \in evidence : phase[evaluation] = "Accepted"

AcceptedRootsAreOwnedAndRecorded ==
    \A evaluation \in Evaluations :
        phase[evaluation] = "Accepted" =>
            /\ evaluation \in validated
            /\ evaluation \in evidence
            /\ publishedRoot[evaluation] = CandidateRoot(evaluation)
            /\ publishedRoot[evaluation] \in recordedRoots

CrashedEvaluationsPublishNothing ==
    \A evaluation \in Evaluations :
        phase[evaluation] = "Crashed" =>
            /\ evaluation \notin validated
            /\ evaluation \notin evidence
            /\ localRoot[evaluation] = baseRoot[evaluation]

=============================================================================
