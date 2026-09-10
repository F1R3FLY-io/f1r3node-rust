---------------------- MODULE MC_RecoveryBudgetEpisodes ---------------------
EXTENDS FiniteSets, Naturals

CONSTANTS
    \* @type: Str;
    FaultMode,
    \* @type: Str;
    Scope

ModelKeys ==
    IF Scope \in {"composition", "liveness"}
    THEN {"k1", "k2"}
    ELSE {"k1", "k2", "k3"}

ModelDispatchIds ==
    IF Scope \in {"composition", "liveness"}
    THEN {1, 2, 3}
    ELSE {1, 2, 3, 4}

VARIABLES
    \* @type: Str;
    currentEpisode,
    \* @type: Set(Str);
    certifiedEpisodes,
    \* @type: Str -> Str;
    phase,
    \* @type: Str -> Set(Str);
    owners,
    \* @type: Str -> Int;
    attempts,
    \* @type: Str -> Int;
    inFlight,
    \* @type: Set(Str);
    obligations,
    \* @type: Set({key: Str, token: Int});
    dispatchHandles,
    \* @type: Int;
    dispatchCount,
    \* @type: Set({episode: Str, target: Str, citer: Str, generation: Int});
    charges,
    \* @type: Str -> Int;
    usage,
    \* @type: Set({episode: Str, target: Str, citer: Str, generation: Int});
    migratedCharges,
    \* @type: Set({episode: Str, target: Str, citer: Str, generation: Int});
    legacyCharges,
    \* @type: Bool;
    startupComplete,
    \* @type: Int;
    lastBatchSize,
    \* @type: Bool;
    uncertifiedAdvance,
    \* @type: Bool;
    staleMutation,
    \* @type: Bool;
    badReset,
    \* @type: Bool;
    wrongResolve,
    \* @type: Bool;
    lostDurability,
    \* @type: Bool;
    lostBudget,
    \* @type: Bool;
    identityReuse,
    \* @type: Int;
    cursor,
    \* @type: Bool;
    restartAvailable

R == INSTANCE RecoveryBudgetEpisodes WITH
    Keys <- ModelKeys,
    Owners <- {"o1", "o2"},
    Episodes <- {"e0", "e1"},
    InitialEpisode <- "e0",
    EpisodeRank <- [episode \in {"e0", "e1"} |-> IF episode = "e0" THEN 0 ELSE 1],
    QueueRank <-
        [key \in ModelKeys |->
            CASE key = "k1" -> 1
              [] key = "k2" -> 2
              [] OTHER -> 3],
    Generation <- [owner \in {"o1", "o2"} |-> IF owner = "o1" THEN 0 ELSE 1],
    DispatchIds <- ModelDispatchIds,
    MaxOutstanding <- Cardinality(ModelDispatchIds) - 1,
    MaxTracked <-
        IF FaultMode = "active-eviction"
        THEN Cardinality(ModelKeys) - 1
        ELSE Cardinality(ModelKeys),
    Capacity <- 2,
    BatchLimit <- 1,
    MaxAttempts <- 2,
    MaxDispatchCount <- 2,
    NoToken <- 0,
    EnableLedger <- Scope \in {"ledger", "composition"},
    EnableWindow <- Scope \in {"window", "composition", "liveness"},
    AllowUncertifiedAdvance <- FaultMode = "uncertified-advance",
    DropChargesOnRestart <- FaultMode = "drop-charges",
    ChargeWithoutUsage <- FaultMode = "charge-without-usage",
    UsageWithoutCharge <- FaultMode = "usage-without-charge",
    AllowOvercapacity <- FaultMode = "overcapacity",
    ReuseToken <- FaultMode = "duplicate-token",
    AllowUnboundedBatch <- FaultMode = "unbounded-batch",
    EvictActiveAtCapacity <- FaultMode = "active-eviction",
    AcceptStaleCompletion <- FaultMode = "stale-completion",
    ResetAttemptsWithoutProgress <- FaultMode = "attempt-reset",
    AllowOwnerlessTracked <- FaultMode = "ownerless-tracked",
    ResolveWrongKey <- FaultMode = "wrong-key-resolve",
    DropBudgetOnRestart <- FaultMode = "drop-budget",
    UseBoundedCounter <- FaultMode = "bounded-counter",
    RetainExpiredOwnership <- FaultMode = "abandoned-owner",
    currentEpisode <- currentEpisode,
    certifiedEpisodes <- certifiedEpisodes,
    phase <- phase,
    owners <- owners,
    attempts <- attempts,
    inFlight <- inFlight,
    obligations <- obligations,
    dispatchHandles <- dispatchHandles,
    dispatchCount <- dispatchCount,
    charges <- charges,
    usage <- usage,
    migratedCharges <- migratedCharges,
    legacyCharges <- legacyCharges,
    startupComplete <- startupComplete,
    lastBatchSize <- lastBatchSize,
    uncertifiedAdvance <- uncertifiedAdvance,
    staleMutation <- staleMutation,
    badReset <- badReset,
    wrongResolve <- wrongResolve,
    lostDurability <- lostDurability,
    lostBudget <- lostBudget,
    identityReuse <- identityReuse,
    cursor <- cursor,
    restartAvailable <- restartAvailable

Spec == R!Spec
LiveSpec == R!LiveSpec
Init == R!Init
Next == R!Next
TypeOK == R!TypeOK
Inv_CurrentEpisodeCertified == R!Inv_CurrentEpisodeCertified
Inv_UsageMatchesCharges == R!Inv_UsageMatchesCharges
Inv_OverCapacityIsLegacyOnly == R!Inv_OverCapacityIsLegacyOnly
Inv_TrackingIsBounded == R!Inv_TrackingIsBounded
Inv_ObligationsStayTracked == R!Inv_ObligationsStayTracked
Inv_TrackedHasOwner == R!Inv_TrackedHasOwner
Inv_InFlightTokensAreUnique == R!Inv_InFlightTokensAreUnique
Inv_LiveIdentityIsNotReused == R!Inv_LiveIdentityIsNotReused
Inv_BatchIsBounded == R!Inv_BatchIsBounded
Inv_StaleCompletionIsEffectFree == R!Inv_StaleCompletionIsEffectFree
Inv_AttemptsResetOnlyAfterProgress == R!Inv_AttemptsResetOnlyAfterProgress
Inv_ResolutionUsesRequestedKey == R!Inv_ResolutionUsesRequestedKey
Inv_DurableChargesSurviveRestart == R!Inv_DurableChargesSurviveRestart
Inv_DurableUsageSurvivesRestart == R!Inv_DurableUsageSurvivesRestart
EventuallyDispatchesOrResolves == R!EventuallyDispatchesOrResolves
EventuallyLeavesInFlight == R!EventuallyLeavesInFlight

Inv_AllSafety ==
    /\ TypeOK
    /\ Inv_CurrentEpisodeCertified
    /\ Inv_UsageMatchesCharges
    /\ Inv_OverCapacityIsLegacyOnly
    /\ Inv_TrackingIsBounded
    /\ Inv_ObligationsStayTracked
    /\ Inv_TrackedHasOwner
    /\ Inv_InFlightTokensAreUnique
    /\ Inv_LiveIdentityIsNotReused
    /\ Inv_BatchIsBounded
    /\ Inv_StaleCompletionIsEffectFree
    /\ Inv_AttemptsResetOnlyAfterProgress
    /\ Inv_ResolutionUsesRequestedKey
    /\ Inv_DurableChargesSurviveRestart
    /\ Inv_DurableUsageSurvivesRestart

=============================================================================
