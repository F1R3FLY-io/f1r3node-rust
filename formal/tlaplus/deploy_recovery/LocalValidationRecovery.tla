------------------------ MODULE LocalValidationRecovery ------------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
    \* @type: Bool;
    DeferLocalFault,
    \* @type: Bool;
    DropLocalFault,
    \* @type: Bool;
    PreserveArtifactIdentity,
    \* @type: Bool;
    LocalAbsenceCreatesInvalidity

GenesisNode == "genesis"
RestoredNode == "restored"
Validators == {GenesisNode, RestoredNode}

Parent == "parent"
Sibling == "sibling"
Child == "child"
Blocks == {Parent, Sibling, Child}

BlockArtifact == "block:parent-dependency"
StateArtifact == "state:child-pre-state"
Artifacts == {BlockArtifact, StateArtifact}
NoArtifact == "none"

Phases == {"blocked", "ready", "in-flight", "deferred", "valid", "dropped"}
Classifications ==
    {"none", "missing-dependency", "local-fault", "objective-invalid"}

RequiredArtifact ==
    [block \in Blocks |->
        IF block = Child THEN StateArtifact ELSE BlockArtifact]

NodeClassification(validator) ==
    IF LocalAbsenceCreatesInvalidity
    THEN "objective-invalid"
    ELSE IF validator = GenesisNode THEN "local-fault" ELSE "missing-dependency"

NamedArtifact(block) ==
    IF PreserveArtifactIdentity
    THEN RequiredArtifact[block]
    ELSE BlockArtifact

VARIABLES
    \* @type: Str -> (Str -> Str);
    phase,
    \* @type: Str -> Set(Str);
    heldArtifacts,
    \* @type: Str -> (Str -> Str);
    waitingFor,
    \* @type: Str -> (Str -> Str);
    classification,
    \* @type: Str -> Set(Str);
    recoveryOutstanding,
    \* @type: Str -> (Str -> Int);
    recoveryFailuresRemaining,
    \* @type: Int;
    immediateSelfRequeues

vars == <<
    phase,
    heldArtifacts,
    waitingFor,
    classification,
    recoveryOutstanding,
    recoveryFailuresRemaining,
    immediateSelfRequeues
>>

Init ==
    /\ phase =
        [validator \in Validators |->
            [block \in Blocks |-> IF block = Child THEN "blocked" ELSE "ready"]]
    /\ heldArtifacts = [validator \in Validators |-> {}]
    /\ waitingFor =
        [validator \in Validators |-> [block \in Blocks |-> NoArtifact]]
    /\ classification =
        [validator \in Validators |-> [block \in Blocks |-> "none"]]
    /\ recoveryOutstanding = [validator \in Validators |-> {}]
    /\ recoveryFailuresRemaining =
        [validator \in Validators |-> [artifact \in Artifacts |-> 1]]
    /\ immediateSelfRequeues = 0

DispatchOne(validator, block) ==
    /\ phase[validator][block] = "ready"
    /\ phase' = [phase EXCEPT ![validator][block] = "in-flight"]
    /\ UNCHANGED <<heldArtifacts, waitingFor, classification,
                    recoveryOutstanding, recoveryFailuresRemaining,
                    immediateSelfRequeues>>

ObserveDeferralOne(validator, block) ==
    /\ phase[validator][block] = "in-flight"
    /\ RequiredArtifact[block] \notin heldArtifacts[validator]
    /\ classification' =
        [classification EXCEPT ![validator][block] = NodeClassification(validator)]
    /\ IF DropLocalFault
       THEN
           /\ phase' = [phase EXCEPT ![validator][block] = "dropped"]
           /\ UNCHANGED <<heldArtifacts, waitingFor, recoveryOutstanding,
                           recoveryFailuresRemaining, immediateSelfRequeues>>
       ELSE IF DeferLocalFault
       THEN
           LET artifact == NamedArtifact(block) IN
           /\ phase' = [phase EXCEPT ![validator][block] = "deferred"]
           /\ waitingFor' = [waitingFor EXCEPT ![validator][block] = artifact]
           /\ recoveryOutstanding' =
               [recoveryOutstanding EXCEPT ![validator] = @ \cup {artifact}]
           /\ UNCHANGED <<heldArtifacts, recoveryFailuresRemaining,
                           immediateSelfRequeues>>
       ELSE
           /\ phase' = [phase EXCEPT ![validator][block] = "ready"]
           /\ immediateSelfRequeues' = immediateSelfRequeues + 1
           /\ UNCHANGED <<heldArtifacts, waitingFor, recoveryOutstanding,
                           recoveryFailuresRemaining>>

RecoveryRequestFailsOne(validator, artifact) ==
    /\ artifact \in recoveryOutstanding[validator]
    /\ recoveryFailuresRemaining[validator][artifact] = 1
    /\ recoveryFailuresRemaining' =
        [recoveryFailuresRemaining EXCEPT ![validator][artifact] = 0]
    /\ UNCHANGED <<phase, heldArtifacts, waitingFor, classification,
                    recoveryOutstanding, immediateSelfRequeues>>

RecoveryRequestSucceedsOne(validator, artifact) ==
    /\ artifact \in recoveryOutstanding[validator]
    /\ recoveryFailuresRemaining[validator][artifact] = 0
    /\ heldArtifacts' =
        [heldArtifacts EXCEPT ![validator] = @ \cup {artifact}]
    /\ phase' =
        [phase EXCEPT ![validator] =
            [block \in Blocks |->
                IF phase[validator][block] = "deferred"
                   /\ waitingFor[validator][block] = artifact
                THEN "ready"
                ELSE phase[validator][block]]]
    /\ waitingFor' =
        [waitingFor EXCEPT ![validator] =
            [block \in Blocks |->
                IF phase[validator][block] = "deferred"
                   /\ waitingFor[validator][block] = artifact
                THEN NoArtifact
                ELSE waitingFor[validator][block]]]
    /\ recoveryOutstanding' =
        [recoveryOutstanding EXCEPT ![validator] = @ \ {artifact}]
    /\ UNCHANGED <<classification, recoveryFailuresRemaining,
                    immediateSelfRequeues>>

ValidateOne(validator, block) ==
    /\ phase[validator][block] = "in-flight"
    /\ RequiredArtifact[block] \in heldArtifacts[validator]
    /\ phase' =
        [phase EXCEPT ![validator] =
            [candidate \in Blocks |->
                IF candidate = block
                THEN "valid"
                ELSE IF block = Parent /\ candidate = Child
                THEN "ready"
                ELSE phase[validator][candidate]]]
    /\ UNCHANGED <<heldArtifacts, waitingFor, classification,
                    recoveryOutstanding, recoveryFailuresRemaining,
                    immediateSelfRequeues>>

Dispatch ==
    \E validator \in Validators, block \in Blocks : DispatchOne(validator, block)

ObserveDeferral ==
    \E validator \in Validators, block \in Blocks :
        ObserveDeferralOne(validator, block)

RecoveryRequestFails ==
    \E validator \in Validators, artifact \in Artifacts :
        RecoveryRequestFailsOne(validator, artifact)

RecoveryRequestSucceeds ==
    \E validator \in Validators, artifact \in Artifacts :
        RecoveryRequestSucceedsOne(validator, artifact)

Validate ==
    \E validator \in Validators, block \in Blocks : ValidateOne(validator, block)

Next ==
    Dispatch
    \/ ObserveDeferral
    \/ RecoveryRequestFails
    \/ RecoveryRequestSucceeds
    \/ Validate

Spec ==
    Init
    /\ [][Next]_vars
    /\ \A dispatchValidator \in Validators, dispatchBlock \in Blocks :
        WF_vars(DispatchOne(dispatchValidator, dispatchBlock))
    /\ \A deferralValidator \in Validators, deferralBlock \in Blocks :
        WF_vars(ObserveDeferralOne(deferralValidator, deferralBlock))
    /\ \A failingValidator \in Validators, failingArtifact \in Artifacts :
        WF_vars(RecoveryRequestFailsOne(failingValidator, failingArtifact))
    /\ \A recoveryValidator \in Validators, recoveryArtifact \in Artifacts :
        WF_vars(RecoveryRequestSucceedsOne(recoveryValidator, recoveryArtifact))
    /\ \A validationValidator \in Validators, validationBlock \in Blocks :
        WF_vars(ValidateOne(validationValidator, validationBlock))

TypeOK ==
    /\ phase \in [Validators -> [Blocks -> Phases]]
    /\ heldArtifacts \in [Validators -> SUBSET Artifacts]
    /\ waitingFor \in [Validators -> [Blocks -> Artifacts \cup {NoArtifact}]]
    /\ classification \in [Validators -> [Blocks -> Classifications]]
    /\ recoveryOutstanding \in [Validators -> SUBSET Artifacts]
    /\ recoveryFailuresRemaining \in
        [Validators -> [Artifacts -> 0..1]]
    /\ immediateSelfRequeues \in Nat

Inv_DeferredNamesRequiredArtifact ==
    \A validator \in Validators, block \in Blocks :
        phase[validator][block] = "deferred"
        => waitingFor[validator][block] = RequiredArtifact[block]

Inv_OutstandingExactlyMatchesWaiters ==
    \A validator \in Validators :
        recoveryOutstanding[validator]
        = {artifact \in Artifacts :
            \E block \in Blocks :
                phase[validator][block] = "deferred"
                /\ waitingFor[validator][block] = artifact}

Inv_RecoveryIsDeduplicatedPerArtifact ==
    \A validator \in Validators :
        Cardinality(recoveryOutstanding[validator]) <= Cardinality(Artifacts)

Inv_LocalAbsenceNeverCreatesInvalidity ==
    \A validator \in Validators, block \in Blocks :
        classification[validator][block] # "objective-invalid"

Inv_GenesisGuardIsLocal ==
    \A block \in Blocks :
        classification[GenesisNode][block] # "none"
        => classification[GenesisNode][block] = "local-fault"

Inv_RestoredNodeDefersMissingDependency ==
    \A block \in Blocks :
        classification[RestoredNode][block] # "none"
        => classification[RestoredNode][block] = "missing-dependency"

Inv_ChildWaitsForValidParent ==
    \A validator \in Validators :
        phase[validator][Child] \in {"ready", "in-flight", "deferred", "valid"}
        => phase[validator][Parent] = "valid"

Inv_ValidatedBlocksHeldTheirExactArtifact ==
    \A validator \in Validators, block \in Blocks :
        phase[validator][block] = "valid"
        => RequiredArtifact[block] \in heldArtifacts[validator]

Inv_NoImmediateSelfRequeue == immediateSelfRequeues = 0

Inv_NoDeferredBlockIsDropped ==
    \A validator \in Validators, block \in Blocks :
        phase[validator][block] # "dropped"

Live_AllValidatorsValidateAllBlocks ==
    <> (\A validator \in Validators, block \in Blocks :
            phase[validator][block] = "valid")
=============================================================================
