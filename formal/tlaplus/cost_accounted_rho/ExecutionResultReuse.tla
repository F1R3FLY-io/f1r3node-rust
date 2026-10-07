---------------- MODULE ExecutionResultReuse ----------------
EXTENDS Naturals, TLC

CONSTANT
    \* @type: Str;
    Defect

ASSUME Defect \in {
    "None",
    "OmitDeployIdentity",
    "OmitPreState",
    "OmitSchedule",
    "OmitWitness",
    "OmitCostSurface",
    "OmitGrade",
    "OmitContext",
    "OmitUserPostState",
    "OmitMergeable",
    "CacheBeforeValidation",
    "DuplicateUserReplay",
    "RechargePersistentIntroduction",
    "CollapsePersistentFirings"
}

Validators == {"V1", "V2"}
Phases == {"Idle", "Certified", "Validated", "Rejected", "Published", "Cached", "Recovered"}

\* @typeAlias: executionArtifact = { deployIdentity: Int, preState: Int, schedule: Int, witness: Int, costSurface: Int, grade: Int, context: Int, userPostState: Int, mergeable: Int };
module_typedefs == TRUE

\* @type: $executionArtifact;
CanonicalArtifact == [
    deployIdentity |-> 1,
    preState |-> 1,
    schedule |-> 1,
    witness |-> 1,
    costSurface |-> 1,
    grade |-> 1,
    context |-> 1,
    userPostState |-> 1,
    mergeable |-> 1
]

\* @type: Set($executionArtifact);
Artifacts == {
    CanonicalArtifact,
    [CanonicalArtifact EXCEPT !.deployIdentity = 2],
    [CanonicalArtifact EXCEPT !.preState = 2],
    [CanonicalArtifact EXCEPT !.schedule = 2],
    [CanonicalArtifact EXCEPT !.witness = 2],
    [CanonicalArtifact EXCEPT !.costSurface = 2],
    [CanonicalArtifact EXCEPT !.grade = 2],
    [CanonicalArtifact EXCEPT !.context = 2],
    [CanonicalArtifact EXCEPT !.userPostState = 2],
    [CanonicalArtifact EXCEPT !.mergeable = 2]
}

\* @type: ($executionArtifact, $executionArtifact) => Bool;
ExactMatch(left, right) == left = right

\* @type: ($executionArtifact, $executionArtifact) => Bool;
ReuseMatch(left, right) ==
    /\ (Defect = "OmitDeployIdentity" \/ left.deployIdentity = right.deployIdentity)
    /\ (Defect = "OmitPreState" \/ left.preState = right.preState)
    /\ (Defect = "OmitSchedule" \/ left.schedule = right.schedule)
    /\ (Defect = "OmitWitness" \/ left.witness = right.witness)
    /\ (Defect = "OmitCostSurface" \/ left.costSurface = right.costSurface)
    /\ (Defect = "OmitGrade" \/ left.grade = right.grade)
    /\ (Defect = "OmitContext" \/ left.context = right.context)
    /\ (Defect = "OmitUserPostState" \/ left.userPostState = right.userPostState)
    /\ (Defect = "OmitMergeable" \/ left.mergeable = right.mergeable)

\* @type: $executionArtifact;
NoArtifact == [
    deployIdentity |-> 0,
    preState |-> 1,
    schedule |-> 1,
    witness |-> 1,
    costSurface |-> 1,
    grade |-> 1,
    context |-> 1,
    userPostState |-> 1,
    mergeable |-> 1
]

VARIABLES
    \* @type: Str -> Str;
    phase,
    \* @type: Str -> $executionArtifact;
    observed,
    \* @type: Str -> Int;
    userExecutions,
    \* @type: Str -> $executionArtifact;
    durable,
    \* @type: Str -> $executionArtifact;
    cache,
    \* @type: Str -> Int;
    introductionAttempts,
    \* @type: Str -> Int;
    introductionCharges,
    \* @type: Str -> Int;
    firingAttempts,
    \* @type: Str -> Int;
    firingCharges

vars == <<phase, observed, userExecutions, durable, cache,
          introductionAttempts, introductionCharges, firingAttempts, firingCharges>>

Init ==
    /\ phase = [validator \in Validators |-> "Idle"]
    /\ observed = [validator \in Validators |-> NoArtifact]
    /\ userExecutions = [validator \in Validators |-> 0]
    /\ durable = [validator \in Validators |-> NoArtifact]
    /\ cache = [validator \in Validators |-> NoArtifact]
    /\ introductionAttempts = [validator \in Validators |-> 0]
    /\ introductionCharges = [validator \in Validators |-> 0]
    /\ firingAttempts = [validator \in Validators |-> 0]
    /\ firingCharges = [validator \in Validators |-> 0]

ComputeAdmission(validator, artifact) ==
    /\ phase[validator] = "Idle"
    /\ artifact \in Artifacts
    /\ observed' = [observed EXCEPT ![validator] = artifact]
    /\ userExecutions' = [userExecutions EXCEPT ![validator] = @ + 1]
    /\ phase' = [phase EXCEPT ![validator] = "Certified"]
    /\ UNCHANGED <<durable, cache, introductionAttempts, introductionCharges,
                    firingAttempts, firingCharges>>

ValidateAdmission(validator) ==
    /\ phase[validator] = "Certified"
    /\ IF ReuseMatch(observed[validator], CanonicalArtifact)
       THEN phase' = [phase EXCEPT ![validator] = "Validated"]
       ELSE phase' = [phase EXCEPT ![validator] = "Rejected"]
    /\ IF Defect = "DuplicateUserReplay" /\ ReuseMatch(observed[validator], CanonicalArtifact)
       THEN userExecutions' = [userExecutions EXCEPT ![validator] = @ + 1]
       ELSE UNCHANGED userExecutions
    /\ UNCHANGED <<observed, durable, cache, introductionAttempts,
                    introductionCharges, firingAttempts, firingCharges>>

Publish(validator) ==
    /\ phase[validator] = "Validated"
    /\ durable' = [durable EXCEPT ![validator] = observed[validator]]
    /\ phase' = [phase EXCEPT ![validator] = "Published"]
    /\ UNCHANGED <<observed, userExecutions, cache, introductionAttempts,
                    introductionCharges, firingAttempts, firingCharges>>

CacheResult(validator) ==
    /\ IF Defect = "CacheBeforeValidation"
       THEN phase[validator] \in {"Certified", "Published"}
       ELSE phase[validator] = "Published"
    /\ cache' = [cache EXCEPT ![validator] = observed[validator]]
    /\ phase' = [phase EXCEPT ![validator] = "Cached"]
    /\ UNCHANGED <<observed, userExecutions, durable, introductionAttempts,
                    introductionCharges, firingAttempts, firingCharges>>

Recover(validator) ==
    /\ phase[validator] = "Cached"
    /\ ExactMatch(cache[validator], CanonicalArtifact)
    /\ phase' = [phase EXCEPT ![validator] = "Recovered"]
    /\ UNCHANGED <<observed, userExecutions, durable, cache,
                    introductionAttempts, introductionCharges,
                    firingAttempts, firingCharges>>

InstallPersistent(validator) ==
    /\ introductionAttempts[validator] < 2
    /\ introductionAttempts' = [introductionAttempts EXCEPT ![validator] = @ + 1]
    /\ IF Defect = "RechargePersistentIntroduction" \/ introductionAttempts[validator] = 0
       THEN introductionCharges' = [introductionCharges EXCEPT ![validator] = @ + 1]
       ELSE UNCHANGED introductionCharges
    /\ UNCHANGED <<phase, observed, userExecutions, durable, cache,
                    firingAttempts, firingCharges>>

FirePersistent(validator) ==
    /\ introductionAttempts[validator] > 0
    /\ firingAttempts[validator] < 2
    /\ firingAttempts' = [firingAttempts EXCEPT ![validator] = @ + 1]
    /\ IF Defect = "CollapsePersistentFirings" /\ firingAttempts[validator] > 0
       THEN UNCHANGED firingCharges
       ELSE firingCharges' = [firingCharges EXCEPT ![validator] = @ + 1]
    /\ UNCHANGED <<phase, observed, userExecutions, durable, cache,
                    introductionAttempts, introductionCharges>>

Quiesce ==
    /\ \A validator \in Validators :
          /\ phase[validator] \in {"Rejected", "Recovered"}
          /\ introductionAttempts[validator] = 2
          /\ firingAttempts[validator] = 2
    /\ UNCHANGED vars

ProtocolStep(validator) ==
    \/ \E artifact \in Artifacts : ComputeAdmission(validator, artifact)
    \/ ValidateAdmission(validator)
    \/ Publish(validator)
    \/ CacheResult(validator)
    \/ Recover(validator)

Next ==
    \/ \E validator \in Validators : ProtocolStep(validator)
    \/ \E validator \in Validators : InstallPersistent(validator)
    \/ \E validator \in Validators : FirePersistent(validator)
    \/ Quiesce

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ \A validator \in Validators : WF_vars(ProtocolStep(validator))

TypeOK ==
    /\ phase \in [Validators -> Phases]
    /\ observed \in [Validators -> Artifacts \cup {NoArtifact}]
    /\ userExecutions \in [Validators -> 0..2]
    /\ durable \in [Validators -> Artifacts \cup {NoArtifact}]
    /\ cache \in [Validators -> Artifacts \cup {NoArtifact}]
    /\ introductionAttempts \in [Validators -> 0..2]
    /\ introductionCharges \in [Validators -> 0..2]
    /\ firingAttempts \in [Validators -> 0..2]
    /\ firingCharges \in [Validators -> 0..2]

ValidatedReuseIsExact ==
    \A validator \in Validators :
        phase[validator] \in {"Validated", "Published", "Cached", "Recovered"}
        => ExactMatch(observed[validator], CanonicalArtifact)

UserExecutionOccursOnce ==
    \A validator \in Validators : userExecutions[validator] <= 1

DurableEvidenceIsExact ==
    \A validator \in Validators :
        durable[validator] # NoArtifact
        => /\ phase[validator] \in {"Published", "Cached", "Recovered"}
           /\ ExactMatch(durable[validator], CanonicalArtifact)

CacheFollowsExactDurability ==
    \A validator \in Validators :
        cache[validator] # NoArtifact
        => /\ durable[validator] = cache[validator]
           /\ ExactMatch(cache[validator], CanonicalArtifact)

ValidatorsPublishEqualResults ==
    \A left, right \in Validators :
        durable[left] # NoArtifact /\ durable[right] # NoArtifact
        => durable[left] = durable[right]

PersistentIntroductionChargedOnce ==
    \A validator \in Validators :
        introductionCharges[validator] = IF introductionAttempts[validator] = 0 THEN 0 ELSE 1

PersistentFiringsChargedSeparately ==
    \A validator \in Validators : firingCharges[validator] = firingAttempts[validator]

PersistentFiringRequiresInstallation ==
    \A validator \in Validators : firingAttempts[validator] > 0 => introductionAttempts[validator] > 0

EventuallyTerminal ==
    \A validator \in Validators : <>(phase[validator] \in {"Rejected", "Recovered"})

=============================================================================
