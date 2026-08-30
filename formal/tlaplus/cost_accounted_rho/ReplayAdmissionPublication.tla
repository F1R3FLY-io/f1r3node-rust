---------------- MODULE ReplayAdmissionPublication ----------------
EXTENDS FiniteSets, Naturals, Sequences, TLC

CONSTANT
    \* @type: Str;
    Defect

ASSUME Defect \in {
    "None",
    "AllAdmittedBypass",
    "CountOnlyAdmission",
    "PrimarySignatureIdentity",
    "LegacyWireFieldIndex",
    "RawEvidenceIdentity",
    "RawReservationIdentity",
    "RawFeeIdentity",
    "RawRngIdentity",
    "UnconsumedEvidence",
    "CallerInvalidContext",
    "EarlyPublication",
    "BareRowTrust",
    "PeerBytePublication",
    "ConflictOverwrite",
    "CacheBeforeStore"
}

Validators == {"V1", "V2"}
Blocks == {"Mixed", "All"}
Deploys == {"D1", "D2", "D3"}
Contexts == {"Authenticated", "Caller"}
Phases == {
    "Idle", "Certified", "Replayed", "Validated", "Published",
    "Rejected", "Conflict", "Crashed", "Reopened"
}

\* @type: Str => Seq(Str);
CandidateOrder(_block) == <<"D1", "D2", "D3">>

\* @type: Str => Seq(Str);
CanonicalAdmitted(block) ==
    IF block = "Mixed"
    THEN <<"D1", "D2">>
    ELSE <<"D1", "D2", "D3">>

\* @type: Str => Seq(Str);
CallerAdmitted(block) ==
    IF block = "Mixed"
    THEN <<"D1", "D3">>
    ELSE <<"D1", "D2">>

\* @type: (Str, Str) => Seq(Str);
AdmittedFor(block, context) ==
    IF context = "Authenticated"
    THEN CanonicalAdmitted(block)
    ELSE CallerAdmitted(block)

\* @type: Str => Seq(Str);
Tag(value) == Append(<<>>, value)
\* @type: Seq(Str);
EmptyEvidence == Tail(Tag("D1"))
\* @type: (Str, Str) => Seq(Str);
PairEvidence(first, second) == Append(Tag(first), second)
\* @type: (Str, Str, Str) => Seq(Str);
TripleEvidence(first, second, third) ==
    Append(PairEvidence(first, second), third)
\* @type: Set(Seq(Str));
EvidenceSequences ==
    {EmptyEvidence}
    \cup {Tag(first) : first \in Deploys}
    \cup {PairEvidence(first, second) : first, second \in Deploys}
    \cup {TripleEvidence(first, second, third) :
            first, second, third \in Deploys}
\* @type: Seq(Str);
NoEvidence == Tag("None")
\* @type: Seq(Str);
ForgedEvidence == Tag("Forged")
\* @type: Seq(Str);
PartialEvidence == Tag("Partial")
\* @type: Seq(Str) => Seq(Str);
EvidenceValue(evidence) == Tag("Complete") \o evidence
StoreValues ==
    {NoEvidence, ForgedEvidence, PartialEvidence} \cup
    {EvidenceValue(evidence) : evidence \in EvidenceSequences}
\* @type: Seq(Str);
PreStateRoot == Tag("Pre")
\* @type: Seq(Str) => Seq(Str);
PostStateRoot(evidence) == Tag("Post") \o evidence
StateRoots == {PreStateRoot} \cup {PostStateRoot(evidence) : evidence \in EvidenceSequences}
\* @type: Str => Str;
PrimaryWitnessIdentity(deploy) ==
    IF deploy \in {"D1", "D2"}
    THEN "same-primary-signature"
    ELSE deploy

\* @type: Str => Str;
ProtocolIdentity(deploy) == deploy

\* @type: Str => Str;
Identity(deploy) ==
    IF Defect = "PrimarySignatureIdentity"
    THEN PrimaryWitnessIdentity(deploy)
    ELSE ProtocolIdentity(deploy)

\* @type: Str => Str;
EvidenceIdentity(deploy) ==
    IF Defect = "RawEvidenceIdentity"
    THEN PrimaryWitnessIdentity(deploy)
    ELSE Identity(deploy)

\* @type: Str => Str;
ReservationIdentity(deploy) ==
    IF Defect = "RawReservationIdentity"
    THEN PrimaryWitnessIdentity(deploy)
    ELSE Identity(deploy)

\* @type: Str => Str;
FeeIdentity(deploy) ==
    IF Defect = "RawFeeIdentity"
    THEN PrimaryWitnessIdentity(deploy)
    ELSE Identity(deploy)

\* @type: Str => Str;
RngIdentity(deploy) ==
    IF Defect = "RawRngIdentity"
    THEN PrimaryWitnessIdentity(deploy)
    ELSE Identity(deploy)

\* @type: Str => Seq(Str);
BoundEvidence(block) ==
    IF Defect = "UnconsumedEvidence" /\ block = "Mixed"
    THEN Append(CanonicalAdmitted(block), "D3")
    ELSE CanonicalAdmitted(block)

\* @type: Str => Str;
StoredIdentity(deploy) ==
    IF Defect = "LegacyWireFieldIndex" /\ deploy \in {"D1", "D2"}
    THEN "empty-legacy-signature-field"
    ELSE Identity(deploy)

\* @type: (Seq(Str), Seq(Str)) => Bool;
SameLegacyIdentity(left, right) ==
    /\ Len(left) = Len(right)
    /\ \A index \in 1..Len(left) : Identity(left[index]) = Identity(right[index])

\* @type: (Str, Str, Seq(Str)) => Bool;
ReplayPermitted(block, context, evidence) ==
    /\ evidence \in EvidenceSequences
    /\ IF Defect = "CallerInvalidContext"
       THEN context \in Contexts
       ELSE context = "Authenticated"
    /\ CASE Defect = "AllAdmittedBypass" /\ block = "All" -> TRUE
          [] Defect = "CountOnlyAdmission" ->
               Len(evidence) = Len(AdmittedFor(block, context))
          [] Defect = "PrimarySignatureIdentity" ->
               SameLegacyIdentity(evidence, AdmittedFor(block, context))
          [] OTHER -> evidence = AdmittedFor(block, context)

\* @type: (Str, Str, Seq(Str)) => Bool;
ValidationAccepts(block, context, evidence) ==
    CASE Defect = "AllAdmittedBypass" /\ block = "All" -> TRUE
      [] Defect = "CountOnlyAdmission" ->
           Len(evidence) = Len(AdmittedFor(block, context))
      [] Defect = "PrimarySignatureIdentity" ->
           SameLegacyIdentity(evidence, AdmittedFor(block, context))
      [] OTHER -> evidence = AdmittedFor(block, context)

VARIABLES
    \* @type: (Str -> (Str -> Str));
    phase,
    \* @type: (Str -> (Str -> Seq(Str)));
    replayEvidence,
    \* @type: (Str -> (Str -> Str));
    replayContext,
    \* @type: (Str -> (Str -> Bool));
    validated,
    \* @type: (Str -> (Str -> Seq(Str)));
    durable,
    \* @type: (Str -> (Str -> Bool));
    durableCertified,
    \* @type: (Str -> (Str -> Seq(Str)));
    cache,
    \* @type: (Str -> (Str -> Seq(Str)));
    stateRoot,
    \* @type: (Str -> (Str -> Seq(Str)));
    firstWrite

vars == <<phase, replayEvidence, replayContext, validated, durable,
          durableCertified, cache, stateRoot, firstWrite>>

EmptyByBlock(value) == [block \in Blocks |-> value]
EmptyByValidator(value) == [validator \in Validators |-> EmptyByBlock(value)]

Init ==
    /\ phase = EmptyByValidator("Idle")
    /\ replayEvidence = EmptyByValidator(<<>>)
    /\ replayContext = EmptyByValidator("Authenticated")
    /\ validated = EmptyByValidator(FALSE)
    /\ durable = EmptyByValidator(NoEvidence)
    /\ durableCertified = EmptyByValidator(FALSE)
    /\ cache = EmptyByValidator(NoEvidence)
    /\ stateRoot = EmptyByValidator(PreStateRoot)
    /\ firstWrite = EmptyByValidator(NoEvidence)

Certify(validator, block) ==
    /\ phase[validator][block] \in {"Idle", "Reopened"}
    /\ phase' = [phase EXCEPT ![validator][block] = "Certified"]
    /\ UNCHANGED <<replayEvidence, replayContext, validated, durable,
                    durableCertified, cache, stateRoot, firstWrite>>

Replay(validator, block, context, evidence) ==
    /\ phase[validator][block] = "Certified"
    /\ context \in Contexts
    /\ ReplayPermitted(block, context, evidence)
    /\ replayEvidence' = [replayEvidence EXCEPT ![validator][block] = evidence]
    /\ replayContext' = [replayContext EXCEPT ![validator][block] = context]
    /\ phase' = [phase EXCEPT ![validator][block] = "Replayed"]
    /\ IF Defect = "EarlyPublication"
       THEN
           /\ durable' =
                [durable EXCEPT ![validator][block] = EvidenceValue(evidence)]
           /\ durableCertified' =
                [durableCertified EXCEPT ![validator][block] = FALSE]
           /\ firstWrite' =
                [firstWrite EXCEPT ![validator][block] = EvidenceValue(evidence)]
       ELSE UNCHANGED <<durable, durableCertified, firstWrite>>
    /\ IF Defect = "CacheBeforeStore"
       THEN cache' =
            [cache EXCEPT ![validator][block] = EvidenceValue(evidence)]
       ELSE UNCHANGED cache
    /\ UNCHANGED <<validated, stateRoot>>

Validate(validator, block) ==
    /\ phase[validator][block] = "Replayed"
    /\ ValidationAccepts(
         block,
         replayContext[validator][block],
         replayEvidence[validator][block]
       )
    /\ validated' = [validated EXCEPT ![validator][block] = TRUE]
    /\ stateRoot' = [stateRoot EXCEPT ![validator][block] =
         PostStateRoot(replayEvidence[validator][block])]
    /\ phase' = [phase EXCEPT ![validator][block] = "Validated"]
    /\ UNCHANGED <<replayEvidence, replayContext, durable,
                    durableCertified, cache, firstWrite>>

RejectReplay(validator, block) ==
    /\ phase[validator][block] = "Replayed"
    /\ ~ValidationAccepts(
         block,
         replayContext[validator][block],
         replayEvidence[validator][block]
       )
    /\ phase' = [phase EXCEPT ![validator][block] = "Rejected"]
    /\ UNCHANGED <<replayEvidence, replayContext, validated, durable,
                    durableCertified, cache, stateRoot, firstWrite>>

Publish(validator, block) ==
    /\ phase[validator][block] = "Validated"
    /\ LET evidence == replayEvidence[validator][block]
           stored == EvidenceValue(evidence)
       IN CASE durable[validator][block] = NoEvidence ->
              /\ durable' = [durable EXCEPT ![validator][block] = stored]
              /\ durableCertified' =
                   [durableCertified EXCEPT ![validator][block] = TRUE]
              /\ cache' = [cache EXCEPT ![validator][block] = stored]
              /\ firstWrite' =
                   [firstWrite EXCEPT ![validator][block] = stored]
              /\ phase' = [phase EXCEPT ![validator][block] = "Published"]
          [] durable[validator][block] = stored ->
              /\ durableCertified' =
                   [durableCertified EXCEPT ![validator][block] = TRUE]
              /\ cache' = [cache EXCEPT ![validator][block] = stored]
              /\ phase' = [phase EXCEPT ![validator][block] = "Published"]
              /\ UNCHANGED <<durable, firstWrite>>
          [] OTHER ->
              /\ phase' = [phase EXCEPT ![validator][block] = "Conflict"]
              /\ UNCHANGED <<durable, durableCertified, cache, firstWrite>>
    /\ UNCHANGED <<replayEvidence, replayContext, validated, stateRoot>>

InjectPeerBytes(validator, block) ==
    /\ Defect \in {"PeerBytePublication", "BareRowTrust"}
    /\ durable[validator][block] = NoEvidence
    /\ durable' = [durable EXCEPT ![validator][block] = ForgedEvidence]
    /\ durableCertified' =
         [durableCertified EXCEPT ![validator][block] = FALSE]
    /\ firstWrite' =
         [firstWrite EXCEPT ![validator][block] = ForgedEvidence]
    /\ UNCHANGED <<phase, replayEvidence, replayContext, validated,
                    cache, stateRoot>>

TrustBareRow(validator, block) ==
    /\ Defect = "BareRowTrust"
    /\ phase[validator][block] = "Certified"
    /\ durable[validator][block] # NoEvidence
    /\ phase' = [phase EXCEPT ![validator][block] = "Published"]
    /\ cache' = [cache EXCEPT ![validator][block] = durable[validator][block]]
    /\ UNCHANGED <<replayEvidence, replayContext, validated, durable,
                    durableCertified, stateRoot, firstWrite>>

OverwriteConflict(validator, block, evidence) ==
    /\ Defect = "ConflictOverwrite"
    /\ durable[validator][block] # NoEvidence
    /\ evidence \in EvidenceSequences
    /\ EvidenceValue(evidence) # durable[validator][block]
    /\ durable' =
         [durable EXCEPT ![validator][block] = EvidenceValue(evidence)]
    /\ durableCertified' =
         [durableCertified EXCEPT ![validator][block] = FALSE]
    /\ UNCHANGED firstWrite
    /\ UNCHANGED <<phase, replayEvidence, replayContext, validated,
                    cache, stateRoot>>

Crash(validator, block) ==
    /\ phase[validator][block] # "Crashed"
    /\ phase' = [phase EXCEPT ![validator][block] = "Crashed"]
    /\ cache' = [cache EXCEPT ![validator][block] = NoEvidence]
    /\ validated' = [validated EXCEPT ![validator][block] = FALSE]
    /\ UNCHANGED <<replayEvidence, replayContext, durable,
                    durableCertified, stateRoot, firstWrite>>

Reopen(validator, block) ==
    /\ phase[validator][block] = "Crashed"
    /\ phase' = [phase EXCEPT ![validator][block] = "Reopened"]
    /\ UNCHANGED <<replayEvidence, replayContext, validated, durable,
                    durableCertified, cache, stateRoot, firstWrite>>

Stutter == UNCHANGED vars

Next ==
    \/ \E validator \in Validators, block \in Blocks : Certify(validator, block)
    \/ \E validator \in Validators, block \in Blocks,
          context \in Contexts, evidence \in EvidenceSequences :
         Replay(validator, block, context, evidence)
    \/ \E validator \in Validators, block \in Blocks : Validate(validator, block)
    \/ \E validator \in Validators, block \in Blocks : RejectReplay(validator, block)
    \/ \E validator \in Validators, block \in Blocks : Publish(validator, block)
    \/ \E validator \in Validators, block \in Blocks : InjectPeerBytes(validator, block)
    \/ \E validator \in Validators, block \in Blocks : TrustBareRow(validator, block)
    \/ \E validator \in Validators, block \in Blocks,
          evidence \in EvidenceSequences : OverwriteConflict(validator, block, evidence)
    \/ \E validator \in Validators, block \in Blocks : Crash(validator, block)
    \/ \E validator \in Validators, block \in Blocks : Reopen(validator, block)
    \/ Stutter

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in [Validators -> [Blocks -> Phases]]
    /\ replayEvidence \in [Validators -> [Blocks -> EvidenceSequences]]
    /\ replayContext \in [Validators -> [Blocks -> Contexts]]
    /\ validated \in [Validators -> [Blocks -> BOOLEAN]]
    /\ durable \in [Validators -> [Blocks -> StoreValues]]
    /\ durableCertified \in [Validators -> [Blocks -> BOOLEAN]]
    /\ cache \in [Validators -> [Blocks -> StoreValues]]
    /\ stateRoot \in [Validators -> [Blocks -> StateRoots]]
    /\ firstWrite \in [Validators -> [Blocks -> StoreValues]]

DeployIdentityIsTypedAndInjective ==
    \A left, right \in Deploys : Identity(left) = Identity(right) => left = right

EvidenceIdentityIsTypedAndInjective ==
    \A left, right \in Deploys :
        EvidenceIdentity(left) = EvidenceIdentity(right) => left = right

ReservationIdentityIsTypedAndInjective ==
    \A left, right \in Deploys :
        ReservationIdentity(left) = ReservationIdentity(right) => left = right

FeeIdentityIsTypedAndInjective ==
    \A left, right \in Deploys : FeeIdentity(left) = FeeIdentity(right) => left = right

RngIdentityIsTypedAndInjective ==
    \A left, right \in Deploys : RngIdentity(left) = RngIdentity(right) => left = right

EvidenceConsumptionIsExact ==
    \A block \in Blocks : BoundEvidence(block) = CanonicalAdmitted(block)

StoredIdentityMatchesProtocolIdentity ==
    \A deploy \in Deploys : StoredIdentity(deploy) = Identity(deploy)

StoredIdentityIsInjective ==
    \A left, right \in Deploys :
        StoredIdentity(left) = StoredIdentity(right) => left = right

ValidatedReplayUsesExactPartition ==
    \A validator \in Validators, block \in Blocks :
        validated[validator][block] =>
            replayEvidence[validator][block] = CanonicalAdmitted(block)

ValidatedReplayUsesAuthenticatedContext ==
    \A validator \in Validators, block \in Blocks :
        validated[validator][block] =>
            replayContext[validator][block] = "Authenticated"

PersistentEvidenceRequiresValidatedReplay ==
    \A validator \in Validators, block \in Blocks :
        durable[validator][block] # NoEvidence => durableCertified[validator][block]

PersistentEvidenceIsExact ==
    \A validator \in Validators, block \in Blocks :
        durableCertified[validator][block] =>
            durable[validator][block] = EvidenceValue(CanonicalAdmitted(block))

CacheFollowsDurablePublication ==
    \A validator \in Validators, block \in Blocks :
        cache[validator][block] # NoEvidence =>
            /\ durableCertified[validator][block]
            /\ cache[validator][block] = durable[validator][block]

RejectedReplayPreservesAuthenticatedRoot ==
    \A validator \in Validators, block \in Blocks :
        phase[validator][block] = "Rejected" =>
            stateRoot[validator][block] = PreStateRoot

ConflictingWritesNeverOverwrite ==
    \A validator \in Validators, block \in Blocks :
        firstWrite[validator][block] # NoEvidence =>
            durable[validator][block] = firstWrite[validator][block]

CrashRecoveryExposesAbsentOrCompleteRow ==
    \A validator \in Validators, block \in Blocks :
        phase[validator][block] \in {"Crashed", "Reopened"} =>
            \/ durable[validator][block] = NoEvidence
            \/ /\ durableCertified[validator][block]
               /\ durable[validator][block] = EvidenceValue(CanonicalAdmitted(block))

ValidatorsPublishEqualEvidence ==
    \A block \in Blocks :
        /\ durableCertified["V1"][block]
        /\ durableCertified["V2"][block]
        => durable["V1"][block] = durable["V2"][block]

=============================================================================
