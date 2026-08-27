---------------------- MODULE MergeRecoveryCoherence ----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
    BaseDominatesTombstone,
    FilterScopeTombstones,
    RejectBaseDuplicates,
    RequireMergeMetadata,
    ValidateTombstoneAuthority,
    FilterWholeChains,
    FilterOrdinaryOnReject,
    FilterMergeableOnReject,
    ValidateEffectIdentity

BaseCommittedSource == "base-committed-source"
BaseRejectedSource == "base-rejected-source"
ScopeTombstonedSource == "scope-tombstoned-source"
ScopeDuplicateSource == "scope-duplicate-source"
NumericUntaggedSource == "numeric-untagged-source"
NumericTaggedSource == "numeric-tagged-source"
RecoveryAfterBaseRejectionSource == "base-rejected-recovery-source"
InvalidTombstoneTargetSource == "invalid-tombstone-target-source"
ChainPrimarySource == "chain-primary-source"
ChainCollateralSource == "chain-collateral-source"
EffectMismatchSource == "effect-mismatch-source"
BaseDuplicateChainPrimarySource == "base-duplicate-chain-primary-source"
BaseDuplicateChainCollateralSource == "base-duplicate-chain-collateral-source"
RetrySource == "retry-source"

BaseClosureSources == {BaseCommittedSource, BaseRejectedSource}
BaseCommittedSources == {BaseCommittedSource}
BaseRejectedSources == {BaseRejectedSource}
ScopeSources ==
    {ScopeTombstonedSource,
     ScopeDuplicateSource,
     NumericUntaggedSource,
     NumericTaggedSource,
     RecoveryAfterBaseRejectionSource,
     InvalidTombstoneTargetSource,
     ChainPrimarySource,
     ChainCollateralSource,
     EffectMismatchSource,
     BaseDuplicateChainPrimarySource,
     BaseDuplicateChainCollateralSource}
AllSources == BaseClosureSources \union ScopeSources \union {RetrySource}
RawTombstoneTargets ==
    {BaseCommittedSource,
     ScopeTombstonedSource,
     InvalidTombstoneTargetSource,
     ChainPrimarySource}
ValidTombstoneTargets ==
    {BaseCommittedSource, ScopeTombstonedSource, ChainPrimarySource}
Signatures == {"A", "B", "C", "D", "E", "F", "G", "H", "I", "J"}

Signature(source) ==
    CASE source = BaseCommittedSource -> "A"
      [] source = ScopeDuplicateSource -> "A"
      [] source = RetrySource -> "A"
      [] source = ScopeTombstonedSource -> "B"
      [] source = NumericUntaggedSource -> "C"
      [] source = NumericTaggedSource -> "D"
      [] source = BaseRejectedSource -> "E"
      [] source = RecoveryAfterBaseRejectionSource -> "E"
      [] source = InvalidTombstoneTargetSource -> "F"
      [] source = ChainPrimarySource -> "G"
      [] source = ChainCollateralSource -> "H"
      [] source = EffectMismatchSource -> "I"
      [] source = BaseDuplicateChainPrimarySource -> "A"
      [] source = BaseDuplicateChainCollateralSource -> "J"

ChainOf(source) ==
    IF source \in {ChainPrimarySource, ChainCollateralSource}
    THEN {ChainPrimarySource, ChainCollateralSource}
    ELSE IF source \in
              {BaseDuplicateChainPrimarySource,
               BaseDuplicateChainCollateralSource}
         THEN {BaseDuplicateChainPrimarySource,
               BaseDuplicateChainCollateralSource}
    ELSE {source}

HasRequiredMergeMetadata(source) == source /= NumericUntaggedSource
HasConsistentEffectIdentity(source) == source /= EffectMismatchSource

VARIABLES
    phase,
    activeSources,
    rejectedSources,
    ordinaryEffectSources,
    mergeMetadataSources,
    numericValue,
    numericDatumCount

vars ==
    <<phase,
      activeSources,
      rejectedSources,
      ordinaryEffectSources,
      mergeMetadataSources,
      numericValue,
      numericDatumCount>>

Init ==
    /\ phase = "ready"
    /\ activeSources = BaseCommittedSources
    /\ rejectedSources = BaseRejectedSources
    /\ ordinaryEffectSources = BaseCommittedSources
    /\ mergeMetadataSources = BaseCommittedSources
    /\ numericValue = 5
    /\ numericDatumCount = 1

BaseActive ==
    IF BaseDominatesTombstone
    THEN BaseCommittedSources
    ELSE BaseCommittedSources \ RawTombstoneTargets

ValidatedTombstoneTargets ==
    IF ValidateTombstoneAuthority
    THEN ValidTombstoneTargets
    ELSE RawTombstoneTargets

ApplicableTombstoneTargets ==
    IF FilterScopeTombstones
    THEN ValidatedTombstoneTargets \intersect ScopeSources
    ELSE {}

TombstonedScope ==
    IF FilterWholeChains
    THEN UNION {ChainOf(source) : source \in ApplicableTombstoneTargets}
    ELSE ApplicableTombstoneTargets

BaseDuplicateSources ==
    {source \in ScopeSources :
        Signature(source) \in
            {Signature(base) : base \in BaseCommittedSources}}

BaseDuplicateScope ==
    IF RejectBaseDuplicates
    THEN IF FilterWholeChains
         THEN UNION {ChainOf(source) : source \in BaseDuplicateSources}
         ELSE BaseDuplicateSources
    ELSE {}

ScopeAccepted(source) ==
    /\ source \in ScopeSources
    /\ source \notin TombstonedScope
    /\ source \notin BaseDuplicateScope
    /\ IF RequireMergeMetadata
       THEN HasRequiredMergeMetadata(source)
       ELSE TRUE
    /\ IF ValidateEffectIdentity
       THEN HasConsistentEffectIdentity(source)
       ELSE TRUE

AcceptedScope == {source \in ScopeSources : ScopeAccepted(source)}

MergeScope ==
    LET accepted == AcceptedScope
        rejected == ScopeSources \ accepted
        retainedOrdinary ==
            IF FilterOrdinaryOnReject THEN {} ELSE {ScopeTombstonedSource}
        retainedMergeable ==
            IF FilterMergeableOnReject THEN {} ELSE {ScopeTombstonedSource}
        acceptedMetadata ==
            {source \in accepted : HasRequiredMergeMetadata(source)}
        taggedAccepted == NumericTaggedSource \in accepted
        untaggedAccepted == NumericUntaggedSource \in accepted
    IN
    /\ phase = "ready"
    /\ phase' = "merged"
    /\ activeSources' = BaseActive \union accepted
    /\ rejectedSources' = BaseRejectedSources \union rejected
    /\ ordinaryEffectSources' =
       BaseCommittedSources \union accepted \union retainedOrdinary
    /\ mergeMetadataSources' =
       BaseCommittedSources \union acceptedMetadata \union retainedMergeable
    /\ numericValue' =
       IF taggedAccepted THEN numericValue + 3 ELSE numericValue
    /\ numericDatumCount' =
       IF untaggedAccepted THEN numericDatumCount + 1 ELSE numericDatumCount

RetryEligible ==
    {source \in activeSources : Signature(source) = "A"} = {}

PublishRetry ==
    /\ phase = "merged"
    /\ RetryEligible
    /\ phase' = "retried"
    /\ activeSources' = activeSources \union {RetrySource}
    /\ rejectedSources' = rejectedSources
    /\ ordinaryEffectSources' = ordinaryEffectSources \union {RetrySource}
    /\ mergeMetadataSources' = mergeMetadataSources \union {RetrySource}
    /\ UNCHANGED <<numericValue, numericDatumCount>>

Next == MergeScope \/ PublishRetry

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"ready", "merged", "retried"}
    /\ activeSources \subseteq AllSources
    /\ rejectedSources \subseteq AllSources
    /\ ordinaryEffectSources \subseteq AllSources
    /\ mergeMetadataSources \subseteq AllSources
    /\ numericValue \in Nat
    /\ numericDatumCount \in Nat

Inv_FinalizedBaseCommittedActive ==
    BaseCommittedSources \subseteq activeSources

Inv_BaseRejectedHasNoEffect ==
    /\ BaseRejectedSources \intersect activeSources = {}
    /\ BaseRejectedSources \intersect ordinaryEffectSources = {}
    /\ BaseRejectedSources \intersect mergeMetadataSources = {}

Inv_BaseRejectedRecoveryAllowed ==
    phase /= "ready" =>
        /\ RecoveryAfterBaseRejectionSource \in activeSources
        /\ RecoveryAfterBaseRejectionSource \in ordinaryEffectSources
        /\ RecoveryAfterBaseRejectionSource \in mergeMetadataSources

Inv_TombstonedScopeNotApplied ==
    /\ ScopeTombstonedSource \notin activeSources
    /\ ScopeTombstonedSource \notin ordinaryEffectSources
    /\ ScopeTombstonedSource \notin mergeMetadataSources

Inv_InvalidTombstoneCannotErase ==
    phase /= "ready" =>
        /\ InvalidTombstoneTargetSource \in activeSources
        /\ InvalidTombstoneTargetSource \in ordinaryEffectSources
        /\ InvalidTombstoneTargetSource \in mergeMetadataSources

Inv_ChainAtomic ==
    phase /= "ready" =>
        /\ {ChainPrimarySource, ChainCollateralSource}
             \intersect activeSources = {}
        /\ {ChainPrimarySource, ChainCollateralSource}
             \intersect ordinaryEffectSources = {}
        /\ {ChainPrimarySource, ChainCollateralSource}
             \intersect mergeMetadataSources = {}
        /\ {BaseDuplicateChainPrimarySource,
             BaseDuplicateChainCollateralSource}
             \intersect activeSources = {}
        /\ {BaseDuplicateChainPrimarySource,
             BaseDuplicateChainCollateralSource}
             \intersect ordinaryEffectSources = {}
        /\ {BaseDuplicateChainPrimarySource,
             BaseDuplicateChainCollateralSource}
             \intersect mergeMetadataSources = {}

Inv_AtMostOneActivePerSignature ==
    \A signature \in Signatures :
        Cardinality(
            {source \in activeSources : Signature(source) = signature}
        ) <= 1

Inv_AtMostOneEffectPerSignature ==
    \A signature \in Signatures :
        Cardinality(
            {source \in ordinaryEffectSources : Signature(source) = signature}
        ) <= 1

Inv_StateRecordCoherence ==
    /\ activeSources = ordinaryEffectSources
    /\ activeSources = mergeMetadataSources
    /\ rejectedSources \intersect ordinaryEffectSources = {}
    /\ rejectedSources \intersect mergeMetadataSources = {}

Inv_RetryDoesNotDuplicateBaseEffect ==
    RetrySource \in ordinaryEffectSources =>
        BaseCommittedSource \notin ordinaryEffectSources

Inv_EffectIdentityConsistency ==
    EffectMismatchSource \notin ordinaryEffectSources

Inv_TaggedNumberSingleDatum == numericDatumCount = 1

Inv_TaggedNumberFold ==
    IF NumericTaggedSource \in ordinaryEffectSources
    THEN numericValue = 8
    ELSE numericValue = 5
=============================================================================
