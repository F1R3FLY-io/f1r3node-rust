-------------------- MODULE AdmissionEffectAlignment --------------------
EXTENDS FiniteSets

CONSTANT
  \* @type: Bool;
  CountStatusRecordsAsEffects

ASSUME CountStatusRecordsAsEffects \in BOOLEAN

Validators == {"v1", "v2", "v3"}
BlockRecords == {"funding-rejection", "close-block"}
EffectRecords == {"close-block"}
MergeMetadata == {"close-block"}
ValidatorStates == {"Ready", "Indexed", "Blocked", "Proposed"}
DeployStates == {"Pending", "Finalized"}

CountedRecords ==
  IF CountStatusRecordsAsEffects
  THEN BlockRecords
  ELSE EffectRecords

MetadataCardinalityMatches ==
  Cardinality(CountedRecords) = Cardinality(MergeMetadata)

VARIABLES
  \* @type: Str -> Str;
  validatorState,
  \* @type: Str -> Set(Str);
  indexedRecords,
  \* @type: Set(Str);
  proposed,
  \* @type: Str;
  laterDeploy

vars == <<validatorState, indexedRecords, proposed, laterDeploy>>

Init ==
  /\ validatorState = [validator \in Validators |-> "Ready"]
  /\ indexedRecords = [validator \in Validators |-> {}]
  /\ proposed = {}
  /\ laterDeploy = "Pending"

IndexParent(validator) ==
  /\ validator \in Validators
  /\ validatorState[validator] = "Ready"
  /\ IF MetadataCardinalityMatches
     THEN /\ validatorState' =
                [validatorState EXCEPT ![validator] = "Indexed"]
          /\ indexedRecords' =
                [indexedRecords EXCEPT ![validator] = CountedRecords]
     ELSE /\ validatorState' =
                [validatorState EXCEPT ![validator] = "Blocked"]
          /\ indexedRecords' = indexedRecords
  /\ UNCHANGED <<proposed, laterDeploy>>

ProposeSuccessor(validator) ==
  /\ validator \in Validators
  /\ validatorState[validator] = "Indexed"
  /\ indexedRecords[validator] = EffectRecords
  /\ validatorState' =
        [validatorState EXCEPT ![validator] = "Proposed"]
  /\ proposed' = proposed \union {validator}
  /\ UNCHANGED <<indexedRecords, laterDeploy>>

FinalizeLaterDeploy ==
  /\ proposed = Validators
  /\ laterDeploy = "Pending"
  /\ laterDeploy' = "Finalized"
  /\ UNCHANGED <<validatorState, indexedRecords, proposed>>

Idle == UNCHANGED vars

Next ==
  \/ \E validator \in Validators : IndexParent(validator)
  \/ \E validator \in Validators : ProposeSuccessor(validator)
  \/ FinalizeLaterDeploy
  \/ Idle

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A validator \in Validators : WF_vars(IndexParent(validator))
  /\ \A validator \in Validators : WF_vars(ProposeSuccessor(validator))
  /\ WF_vars(FinalizeLaterDeploy)

TypeOK ==
  /\ validatorState \in [Validators -> ValidatorStates]
  /\ indexedRecords \in [Validators -> SUBSET BlockRecords]
  /\ proposed \subseteq Validators
  /\ laterDeploy \in DeployStates

Inv_EffectMetadataAligned ==
  Cardinality(EffectRecords) = Cardinality(MergeMetadata)

Inv_StatusOnlyRecordHasNoMetadata ==
  "funding-rejection" \notin MergeMetadata

Inv_IndexedDomainExact ==
  \A validator \in Validators :
    validatorState[validator] \in {"Indexed", "Proposed"} =>
      indexedRecords[validator] = EffectRecords

Inv_StatusOnlyRecordCannotBlock ==
  \A validator \in Validators : validatorState[validator] /= "Blocked"

Inv_FinalizationRequiresAllProposals ==
  laterDeploy = "Finalized" => proposed = Validators

Live_AllValidatorsPropose == <>(proposed = Validators)
Live_LaterDeployFinalizes == <>(laterDeploy = "Finalized")
=============================================================================
