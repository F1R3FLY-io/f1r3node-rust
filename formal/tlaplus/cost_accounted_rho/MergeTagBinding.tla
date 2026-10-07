------------------------ MODULE MergeTagBinding ------------------------
EXTENDS FiniteSets, TLC

CONSTANTS
  \* @type: Set(Str);
  Validators,
  \* @type: Str;
  LegacyValidator,
  \* @type: Str;
  ChangedValidator,
  \* @type: Str;
  IntegerAddTag,
  \* @type: Str;
  BitmaskOrTag,
  \* @type: Str;
  ChangedEnvelopeTag,
  \* @type: Bool;
  UseEnvelopeDerivedTag

ASSUME /\ Validators # {}
       /\ LegacyValidator \in Validators
       /\ ChangedValidator \in Validators
       /\ LegacyValidator # ChangedValidator
       /\ IntegerAddTag # BitmaskOrTag
       /\ ChangedEnvelopeTag # IntegerAddTag
       /\ ChangedEnvelopeTag # BitmaskOrTag
       /\ UseEnvelopeDerivedTag \in BOOLEAN

Unset == "unset"
IntegerAddKind == "IntegerAdd"
BitmaskOrKind == "BitmaskOr"
OrdinaryKind == "Ordinary"

EnvelopeTag(validator) ==
  IF validator = LegacyValidator THEN IntegerAddTag ELSE ChangedEnvelopeTag

ResolvedIntegerAddUri == IntegerAddTag

ContractTag(validator) ==
  IF UseEnvelopeDerivedTag
  THEN EnvelopeTag(validator)
  ELSE ResolvedIntegerAddUri

Classify(tag) ==
  IF tag = IntegerAddTag THEN IntegerAddKind
  ELSE IF tag = BitmaskOrTag THEN BitmaskOrKind
  ELSE OrdinaryKind

VARIABLES
  \* @type: Set(Str);
  pending,
  \* @type: Str -> Str;
  eventTag,
  \* @type: Str -> Str;
  mergeKind

vars == <<pending, eventTag, mergeKind>>

Init ==
  /\ pending = Validators
  /\ eventTag = [validator \in Validators |-> Unset]
  /\ mergeKind = [validator \in Validators |-> Unset]

Evaluate(validator) ==
  /\ validator \in pending
  /\ pending' = pending \ {validator}
  /\ eventTag' = [eventTag EXCEPT ![validator] = ContractTag(validator)]
  /\ mergeKind' = [mergeKind EXCEPT ![validator] = Classify(ContractTag(validator))]

Idle ==
  /\ pending = {}
  /\ UNCHANGED vars

Next == (\E validator \in pending : Evaluate(validator)) \/ Idle

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A validator \in Validators : WF_vars(Evaluate(validator))

Processed == Validators \ pending

TypeOK ==
  /\ pending \subseteq Validators
  /\ eventTag \in [Validators -> {Unset, IntegerAddTag, BitmaskOrTag, ChangedEnvelopeTag}]
  /\ mergeKind \in [Validators -> {Unset, IntegerAddKind, BitmaskOrKind, OrdinaryKind}]

Inv_SystemTagsSeparated == IntegerAddTag # BitmaskOrTag

Inv_UriRegistryAgreement ==
  \A validator \in Processed : eventTag[validator] = ResolvedIntegerAddUri

Inv_NumericEventsAreIntegerAdd ==
  \A validator \in Processed : mergeKind[validator] = IntegerAddKind

Inv_ValidatorAgreement ==
  \A left, right \in Processed :
    /\ eventTag[left] = eventTag[right]
    /\ mergeKind[left] = mergeKind[right]

Inv_ClassifierAuthenticatesExactTag ==
  \A validator \in Processed :
    (mergeKind[validator] = IntegerAddKind) = (eventTag[validator] = IntegerAddTag)

Live_AllValidatorsEvaluate == <>(pending = {})

=======================================================================
