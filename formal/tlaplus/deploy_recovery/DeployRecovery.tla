-------------------------- MODULE DeployRecovery --------------------------
EXTENDS Naturals, FiniteSets, Sequences

CONSTANTS
    NumValidators,
    OnlineValidators,
    MaxHeight,
    Lifespan,
    ValidAfter,
    OccurrenceAware,
    EnforceExpiry,
    SingleLeader,
    HeartbeatsContinue,
    PreserveSelectedRecovery

ValidatorSet == 1..NumValidators
Sources == [height : 0..MaxHeight, validator : ValidatorSet]
InitialSources ==
    {[height |-> 0, validator |-> v] : v \in ValidatorSet}
RetryRecords ==
    [validator : ValidatorSet,
     finalizedView : 0..MaxHeight,
     proposalView : 0..(MaxHeight - 1),
     blockHeight : 1..MaxHeight,
     sourceFree : BOOLEAN,
     unexpired : BOOLEAN,
     survivesSelfChainFilter : BOOLEAN]

VARIABLES
    proposalHeight,
    finalizedHeight,
    observedProposalHeights,
    observedFinalizedHeights,
    occurrences,
    tombstones,
    visibleOccurrences,
    visibleTombstones,
    pendingRetries

vars ==
    <<proposalHeight,
      finalizedHeight,
      observedProposalHeights,
      observedFinalizedHeights,
      occurrences,
      tombstones,
      visibleOccurrences,
      visibleTombstones,
      pendingRetries>>

Init ==
    /\ proposalHeight = 0
    /\ finalizedHeight = 0
    /\ observedProposalHeights = [v \in ValidatorSet |-> 0]
    /\ observedFinalizedHeights = [v \in ValidatorSet |-> 0]
    /\ occurrences = InitialSources
    /\ tombstones = {}
    /\ visibleOccurrences = [v \in ValidatorSet |-> InitialSources]
    /\ visibleTombstones = [v \in ValidatorSet |-> {}]
    /\ pendingRetries = {}

ActiveSources == occurrences \ tombstones
LocalActiveSources(v) == visibleOccurrences[v] \ visibleTombstones[v]

InWindowAt(blockHeight) ==
    /\ ValidAfter < blockHeight
    /\ blockHeight < ValidAfter + Lifespan
    /\ blockHeight <= MaxHeight

LeaderAt(finalizedView) == (finalizedView % NumValidators) + 1

DispositionAllowsRetry(v) ==
    IF OccurrenceAware
    THEN LocalActiveSources(v) = {}
    ELSE visibleTombstones[v] /= {}

CandidateBlockHeight(v) == observedProposalHeights[v] + 1

RetryEligible(v) ==
    /\ CandidateBlockHeight(v) <= MaxHeight
    /\ DispositionAllowsRetry(v)
    /\ IF EnforceExpiry
       THEN InWindowAt(CandidateBlockHeight(v))
       ELSE TRUE

PrepareRetry(v) ==
    LET record ==
        [validator |-> v,
         finalizedView |-> observedFinalizedHeights[v],
         proposalView |-> observedProposalHeights[v],
         blockHeight |-> CandidateBlockHeight(v),
         sourceFree |-> LocalActiveSources(v) = {},
         unexpired |-> InWindowAt(CandidateBlockHeight(v)),
         survivesSelfChainFilter |-> PreserveSelectedRecovery]
    IN
    /\ v \in OnlineValidators
    /\ RetryEligible(v)
    /\ ~\E pending \in pendingRetries : pending.validator = v
    /\ IF SingleLeader
       THEN v = LeaderAt(observedFinalizedHeights[v])
       ELSE TRUE
    /\ pendingRetries' = pendingRetries \union {record}
    /\ UNCHANGED
       <<proposalHeight,
         finalizedHeight,
         observedProposalHeights,
         observedFinalizedHeights,
         occurrences,
         tombstones,
         visibleOccurrences,
         visibleTombstones>>

PublishRetry(pending) ==
    LET source ==
        [height |-> pending.blockHeight, validator |-> pending.validator]
        nextProposalHeight ==
            IF pending.blockHeight > proposalHeight
            THEN pending.blockHeight
            ELSE proposalHeight
        nextObservedProposalHeight ==
            IF pending.blockHeight > observedProposalHeights[pending.validator]
            THEN pending.blockHeight
            ELSE observedProposalHeights[pending.validator]
    IN
    /\ pending \in pendingRetries
    /\ pending.survivesSelfChainFilter
    /\ source \notin occurrences
    /\ occurrences' = occurrences \union {source}
    /\ visibleOccurrences' =
       [visibleOccurrences EXCEPT
          ![pending.validator] = @ \union {source}]
    /\ pendingRetries' = pendingRetries \ {pending}
    /\ proposalHeight' = nextProposalHeight
    /\ observedProposalHeights' =
       [observedProposalHeights EXCEPT
          ![pending.validator] = nextObservedProposalHeight]
    /\ UNCHANGED
       <<finalizedHeight,
         observedFinalizedHeights,
         tombstones,
         visibleTombstones>>

PublishRejection(v, source) ==
    /\ v \in OnlineValidators
    /\ source \in LocalActiveSources(v)
    /\ source \notin tombstones
    /\ tombstones' = tombstones \union {source}
    /\ visibleTombstones' =
       [visibleTombstones EXCEPT ![v] = @ \union {source}]
    /\ UNCHANGED
       <<proposalHeight,
         finalizedHeight,
         observedProposalHeights,
         observedFinalizedHeights,
         occurrences,
         visibleOccurrences,
         pendingRetries>>

ObserveOccurrence(v, source) ==
    /\ v \in OnlineValidators
    /\ source \in occurrences \ visibleOccurrences[v]
    /\ visibleOccurrences' =
       [visibleOccurrences EXCEPT ![v] = @ \union {source}]
    /\ UNCHANGED
       <<proposalHeight,
         finalizedHeight,
         observedProposalHeights,
         observedFinalizedHeights,
         occurrences,
         tombstones,
         visibleTombstones,
         pendingRetries>>

ObserveTombstone(v, source) ==
    /\ v \in OnlineValidators
    /\ source \in tombstones \ visibleTombstones[v]
    /\ visibleOccurrences' =
       [visibleOccurrences EXCEPT ![v] = @ \union {source}]
    /\ visibleTombstones' =
       [visibleTombstones EXCEPT ![v] = @ \union {source}]
    /\ UNCHANGED
       <<proposalHeight,
         finalizedHeight,
         observedProposalHeights,
         observedFinalizedHeights,
         occurrences,
         tombstones,
         pendingRetries>>

ObserveProposalHeight(v) ==
    /\ v \in OnlineValidators
    /\ observedProposalHeights[v] < proposalHeight
    /\ observedProposalHeights' =
       [observedProposalHeights EXCEPT ![v] = @ + 1]
    /\ UNCHANGED
       <<proposalHeight,
         finalizedHeight,
         observedFinalizedHeights,
         occurrences,
         tombstones,
         visibleOccurrences,
         visibleTombstones,
         pendingRetries>>

ObserveFinalizedHeight(v) ==
    LET nextFinalizedView == observedFinalizedHeights[v] + 1
    IN
    /\ v \in OnlineValidators
    /\ observedFinalizedHeights[v] < finalizedHeight
    /\ observedFinalizedHeights' =
       [observedFinalizedHeights EXCEPT ![v] = nextFinalizedView]
    /\ observedProposalHeights' =
       [observedProposalHeights EXCEPT
          ![v] = IF @ < nextFinalizedView THEN nextFinalizedView ELSE @]
    /\ UNCHANGED
       <<proposalHeight,
         finalizedHeight,
         occurrences,
         tombstones,
         visibleOccurrences,
         visibleTombstones,
         pendingRetries>>

Advance(v) ==
    LET nextFinalizedHeight == finalizedHeight + 1
        nextProposalHeight ==
            IF finalizedHeight < proposalHeight
            THEN proposalHeight
            ELSE proposalHeight + 1
    IN
    /\ v \in OnlineValidators
    /\ observedProposalHeights[v] = proposalHeight
    /\ observedFinalizedHeights[v] = finalizedHeight
    /\ finalizedHeight < proposalHeight \/ proposalHeight < MaxHeight
    /\ IF HeartbeatsContinue
       THEN TRUE
       ELSE v = LeaderAt(finalizedHeight)
    /\ finalizedHeight' = nextFinalizedHeight
    /\ proposalHeight' = nextProposalHeight
    /\ observedFinalizedHeights' =
       [observedFinalizedHeights EXCEPT ![v] = nextFinalizedHeight]
    /\ observedProposalHeights' =
       [observedProposalHeights EXCEPT ![v] = nextProposalHeight]
    /\ UNCHANGED
       <<occurrences,
         tombstones,
         visibleOccurrences,
         visibleTombstones,
         pendingRetries>>

PrepareAny == \E v \in ValidatorSet : PrepareRetry(v)
PublishAny == \E pending \in RetryRecords : PublishRetry(pending)
RejectAny ==
    \E v \in ValidatorSet, source \in Sources : PublishRejection(v, source)
ObserveOccurrenceAny ==
    \E v \in ValidatorSet, source \in Sources : ObserveOccurrence(v, source)
ObserveTombstoneAny ==
    \E v \in ValidatorSet, source \in Sources : ObserveTombstone(v, source)
ObserveProposalHeightAny ==
    \E v \in ValidatorSet : ObserveProposalHeight(v)
ObserveFinalizedHeightAny ==
    \E v \in ValidatorSet : ObserveFinalizedHeight(v)
AdvanceAny == \E v \in ValidatorSet : Advance(v)

Next ==
    \/ PrepareAny
    \/ PublishAny
    \/ RejectAny
    \/ ObserveOccurrenceAny
    \/ ObserveTombstoneAny
    \/ ObserveProposalHeightAny
    \/ ObserveFinalizedHeightAny
    \/ AdvanceAny

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(PrepareAny)
    /\ WF_vars(PublishAny)
    /\ WF_vars(ObserveOccurrenceAny)
    /\ WF_vars(ObserveTombstoneAny)
    /\ WF_vars(ObserveProposalHeightAny)
    /\ WF_vars(ObserveFinalizedHeightAny)
    /\ WF_vars(AdvanceAny)

TypeOK ==
    /\ NumValidators > 0
    /\ MaxHeight > 0
    /\ OnlineValidators \subseteq ValidatorSet
    /\ proposalHeight \in 0..MaxHeight
    /\ finalizedHeight \in 0..proposalHeight
    /\ observedProposalHeights \in [ValidatorSet -> 0..proposalHeight]
    /\ observedFinalizedHeights \in [ValidatorSet -> 0..finalizedHeight]
    /\ \A v \in ValidatorSet :
          observedFinalizedHeights[v] <= observedProposalHeights[v]
    /\ occurrences \subseteq Sources
    /\ tombstones \subseteq occurrences
    /\ visibleOccurrences \in [ValidatorSet -> SUBSET occurrences]
    /\ visibleTombstones \in [ValidatorSet -> SUBSET tombstones]
    /\ \A v \in ValidatorSet :
          visibleTombstones[v] \subseteq visibleOccurrences[v]
    /\ pendingRetries \subseteq RetryRecords

Inv_ExactTombstones == tombstones \subseteq occurrences

Inv_LocalViewsAreSound ==
    \A v \in ValidatorSet :
        /\ visibleOccurrences[v] \subseteq occurrences
        /\ visibleTombstones[v] \subseteq tombstones
        /\ visibleTombstones[v] \subseteq visibleOccurrences[v]

Inv_RetryRequiresNoActiveSource ==
    \A pending \in pendingRetries : pending.sourceFree

Inv_NoExpiredRetry ==
    \A pending \in pendingRetries : pending.unexpired

Inv_SelectedRetrySurvivesSelfChainFilter ==
    \A pending \in pendingRetries : pending.survivesSelfChainFilter

Inv_OneRecoveryProposerPerFinalizedView ==
    \A view \in 0..MaxHeight :
        Cardinality(
            {pending \in pendingRetries : pending.finalizedView = view}
        ) <= 1

Inv_RecoveryLeaderIsCommittedViewDerived ==
    \A pending \in pendingRetries :
        pending.validator = LeaderAt(pending.finalizedView)

Inv_CrossViewRetriesAreBounded ==
    Cardinality(pendingRetries) =
        Cardinality(
            {pending.finalizedView : pending \in pendingRetries}
        )

Inv_OnePendingRetryPerValidator ==
    \A v \in ValidatorSet :
        Cardinality(
            {pending \in pendingRetries : pending.validator = v}
        ) <= 1

Inv_RecoveryHeightUsesCommittedDagView ==
    \A pending \in pendingRetries :
        pending.blockHeight = pending.proposalView + 1

NeedsRecovery ==
    /\ ActiveSources = {}
    /\ \E v \in OnlineValidators :
          /\ LocalActiveSources(v) = {}
          /\ InWindowAt(CandidateBlockHeight(v))

RecoveryCommittedOrExpired ==
    \/ ActiveSources /= {}
    \/ \A v \in OnlineValidators :
          ~InWindowAt(CandidateBlockHeight(v))

Live_RecoveryOrExpiry ==
    NeedsRecovery ~> RecoveryCommittedOrExpired
=============================================================================
