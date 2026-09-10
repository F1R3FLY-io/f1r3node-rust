----------------------- MODULE RecoveryBudgetEpisodes -----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
    \* @type: Set(Str);
    Keys,
    \* @type: Set(Str);
    Owners,
    \* @type: Set(Str);
    Episodes,
    \* @type: Str;
    InitialEpisode,
    \* @type: Str -> Int;
    EpisodeRank,
    \* @type: Str -> Int;
    QueueRank,
    \* @type: Str -> Int;
    Generation,
    \* @type: Set(Int);
    DispatchIds,
    \* @type: Int;
    MaxOutstanding,
    \* @type: Int;
    MaxTracked,
    \* @type: Int;
    Capacity,
    \* @type: Int;
    BatchLimit,
    \* @type: Int;
    MaxAttempts,
    \* @type: Int;
    MaxDispatchCount,
    \* @type: Int;
    NoToken,
    \* @type: Bool;
    EnableLedger,
    \* @type: Bool;
    EnableWindow,
    \* @type: Bool;
    AllowUncertifiedAdvance,
    \* @type: Bool;
    DropChargesOnRestart,
    \* @type: Bool;
    ChargeWithoutUsage,
    \* @type: Bool;
    UsageWithoutCharge,
    \* @type: Bool;
    AllowOvercapacity,
    \* @type: Bool;
    ReuseToken,
    \* @type: Bool;
    AllowUnboundedBatch,
    \* @type: Bool;
    EvictActiveAtCapacity,
    \* @type: Bool;
    AcceptStaleCompletion,
    \* @type: Bool;
    ResetAttemptsWithoutProgress,
    \* @type: Bool;
    AllowOwnerlessTracked,
    \* @type: Bool;
    ResolveWrongKey,
    \* @type: Bool;
    DropBudgetOnRestart,
    \* @type: Bool;
    UseBoundedCounter,
    \* @type: Bool;
    RetainExpiredOwnership

Phases == {"absent", "ready", "inflight", "backoff", "resolved"}
TrackedPhases == {"ready", "inflight", "backoff"}
Generations == {Generation[owner] : owner \in Owners}
ChargeType ==
    [episode : Episodes, target : Keys, citer : Owners, generation : Generations]
DispatchHandleType == [key : Keys, token : DispatchIds]

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

vars ==
    <<currentEpisode, certifiedEpisodes, phase, owners, attempts, inFlight,
      obligations, dispatchHandles, dispatchCount, charges, usage, migratedCharges,
      legacyCharges, startupComplete, lastBatchSize, uncertifiedAdvance,
      staleMutation, badReset, wrongResolve, lostDurability, lostBudget,
      identityReuse, cursor, restartAvailable>>

Tracked == {key \in Keys : phase[key] \in TrackedPhases}
Ready == {key \in Keys : phase[key] = "ready"}
InFlightKeys == {key \in Keys : phase[key] = "inflight"}
ActiveTokens == {inFlight[key] : key \in InFlightKeys}
HandleTokens == {handle.token : handle \in dispatchHandles}
ReservedTokens == ActiveTokens \cup HandleTokens
ChargesFor(episode) == {charge \in charges : charge.episode = episode}

CyclicDistance(key) ==
    IF QueueRank[key] >= cursor
    THEN QueueRank[key] - cursor
    ELSE Cardinality(Keys) + QueueRank[key] - cursor

SelectedReady ==
    {key \in Ready :
        \A other \in Ready : CyclicDistance(key) <= CyclicDistance(other)}

ChargeFor(key, owner, episode) ==
    [episode |-> episode,
     target |-> key,
     citer |-> owner,
     generation |-> Generation[owner]]

Init ==
    /\ InitialEpisode \in Episodes
    /\ currentEpisode = InitialEpisode
    /\ certifiedEpisodes = {InitialEpisode}
    /\ phase = [key \in Keys |-> "absent"]
    /\ owners = [key \in Keys |-> {}]
    /\ attempts = [key \in Keys |-> 0]
    /\ inFlight = [key \in Keys |-> NoToken]
    /\ obligations = {}
    /\ dispatchHandles = {}
    /\ dispatchCount = 0
    /\ charges = {}
    /\ usage = [episode \in Episodes |-> 0]
    /\ migratedCharges = {}
    /\ legacyCharges = {}
    /\ startupComplete = FALSE
    /\ lastBatchSize = 0
    /\ uncertifiedAdvance = FALSE
    /\ staleMutation = FALSE
    /\ badReset = FALSE
    /\ wrongResolve = FALSE
    /\ lostDurability = FALSE
    /\ lostBudget = FALSE
    /\ identityReuse = FALSE
    /\ cursor = 1
    /\ restartAvailable = TRUE

CertifyEpisode(episode) ==
    /\ episode \in Episodes \ certifiedEpisodes
    /\ certifiedEpisodes' = certifiedEpisodes \cup {episode}
    /\ UNCHANGED
        <<currentEpisode, phase, owners, attempts, inFlight, obligations,
          dispatchHandles, dispatchCount, charges, usage, migratedCharges,
          legacyCharges, startupComplete, lastBatchSize, uncertifiedAdvance,
          staleMutation, badReset, wrongResolve, lostDurability, lostBudget,
          identityReuse, restartAvailable>>

AdvanceEpisode(episode) ==
    /\ episode \in Episodes \ {currentEpisode}
    /\ EpisodeRank[currentEpisode] < EpisodeRank[episode]
    /\ episode \in certifiedEpisodes \/ AllowUncertifiedAdvance
    /\ currentEpisode' = episode
    /\ uncertifiedAdvance' =
        (uncertifiedAdvance \/ (episode \notin certifiedEpisodes))
    /\ UNCHANGED
        <<certifiedEpisodes, phase, owners, attempts, inFlight, obligations,
          dispatchHandles, dispatchCount, charges, usage, migratedCharges,
          legacyCharges, startupComplete, lastBatchSize, staleMutation,
          badReset, wrongResolve, lostDurability, lostBudget,
          identityReuse, restartAvailable>>

StageLegacyCharge(key, owner) ==
    LET charge == ChargeFor(key, owner, currentEpisode)
    IN
    /\ ~startupComplete
    /\ charge \notin legacyCharges
    /\ ~\E existing \in legacyCharges : existing.target = key
    /\ legacyCharges' = legacyCharges \cup {charge}
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, phase, owners, attempts,
          inFlight, obligations, dispatchHandles, dispatchCount, charges, usage,
          migratedCharges, startupComplete, lastBatchSize,
          uncertifiedAdvance, staleMutation, badReset, wrongResolve,
          lostDurability, lostBudget, identityReuse, restartAvailable>>

MigrateLegacy ==
    /\ ~startupComplete
    /\ charges' = charges \cup legacyCharges
    /\ migratedCharges' = migratedCharges \cup legacyCharges
    /\ usage' =
        [episode \in Episodes |->
            Cardinality({charge \in charges \cup legacyCharges :
                charge.episode = episode})]
    /\ legacyCharges' = {}
    /\ startupComplete' = TRUE
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, phase, owners, attempts,
          inFlight, obligations, dispatchHandles, dispatchCount, lastBatchSize,
          uncertifiedAdvance, staleMutation, badReset, wrongResolve,
          lostDurability, lostBudget, identityReuse, restartAvailable>>

RegisterOne(key, owner) ==
    /\ owner \in Owners
    /\ IF phase[key] \in TrackedPhases
       THEN
         /\ owners' = [owners EXCEPT ![key] = @ \cup {owner}]
         /\ UNCHANGED <<phase, obligations, attempts, inFlight>>
       ELSE
         /\ phase[key] \in {"absent", "resolved"}
         /\ Cardinality(Tracked) < MaxTracked
         /\ phase' = [phase EXCEPT ![key] = "ready"]
         /\ owners' = [owners EXCEPT ![key] = {owner}]
         /\ obligations' = obligations \cup {key}
         /\ attempts' = [attempts EXCEPT ![key] = 0]
         /\ inFlight' = [inFlight EXCEPT ![key] = NoToken]
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, dispatchHandles, dispatchCount,
          charges, usage, migratedCharges, legacyCharges, startupComplete,
          lastBatchSize, uncertifiedAdvance, staleMutation, badReset,
          wrongResolve, lostDurability, lostBudget, identityReuse,
          restartAvailable>>

EvictAndRegister(victim, key, owner) ==
    /\ EvictActiveAtCapacity
    /\ Cardinality(Tracked) = MaxTracked
    /\ victim \in Tracked
    /\ key \in Keys \ Tracked
    /\ owner \in Owners
    /\ phase' = [phase EXCEPT ![victim] = "absent", ![key] = "ready"]
    /\ owners' = [owners EXCEPT ![victim] = {}, ![key] = {owner}]
    /\ attempts' = [attempts EXCEPT ![victim] = 0, ![key] = 0]
    /\ inFlight' = [inFlight EXCEPT ![victim] = NoToken, ![key] = NoToken]
    /\ obligations' = obligations \cup {key}
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, dispatchHandles, dispatchCount,
          charges, usage, migratedCharges, legacyCharges, startupComplete,
          lastBatchSize, uncertifiedAdvance, staleMutation, badReset,
          wrongResolve, lostDurability, lostBudget, identityReuse,
          restartAvailable>>

ReleaseOwner(key, owner) ==
    /\ key \in Tracked
    /\ owner \in owners[key]
    /\ IF owners[key] = {owner}
       THEN
         /\ phase' = [phase EXCEPT ![key] = "resolved"]
         /\ owners' = [owners EXCEPT ![key] = {}]
         /\ obligations' = obligations \ {key}
         /\ attempts' = [attempts EXCEPT ![key] = 0]
         /\ inFlight' = [inFlight EXCEPT ![key] = NoToken]
       ELSE
         /\ owners' = [owners EXCEPT ![key] = @ \ {owner}]
         /\ UNCHANGED <<phase, obligations, attempts, inFlight>>
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, dispatchHandles, dispatchCount,
          charges, usage, migratedCharges, legacyCharges, startupComplete,
          lastBatchSize, uncertifiedAdvance, staleMutation, badReset,
          wrongResolve, lostDurability, lostBudget, identityReuse,
          restartAvailable>>

ReleaseLastOwnerUnsafely(key, owner) ==
    /\ AllowOwnerlessTracked
    /\ key \in Tracked
    /\ owners[key] = {owner}
    /\ owners' = [owners EXCEPT ![key] = {}]
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, phase, attempts, inFlight,
          obligations, dispatchHandles, dispatchCount, charges, usage,
          migratedCharges, legacyCharges, startupComplete, lastBatchSize,
          uncertifiedAdvance, staleMutation, badReset, wrongResolve,
          lostDurability, lostBudget, identityReuse, restartAvailable>>

\* @type: (Set(Str), Str -> Int) => Set(Int);
AssignedTokens(batch, assignment) ==
    {assignment[key] : key \in batch}

\* @type: (Set(Str), Str -> Int) => Bool;
DispatchBatch(batch, assignment) ==
    /\ batch \in SUBSET Ready
    /\ batch # {}
    /\ Cardinality(batch) <= BatchLimit \/ AllowUnboundedBatch
    /\ AllowUnboundedBatch \/ batch = SelectedReady
    /\ assignment \in [batch -> DispatchIds]
    /\ Cardinality(AssignedTokens(batch, assignment)) = Cardinality(batch)
    /\ IF ReuseToken /\ ReservedTokens # {}
       THEN AssignedTokens(batch, assignment) \cap ReservedTokens # {}
       ELSE AssignedTokens(batch, assignment) \cap ReservedTokens = {}
    /\ Cardinality(dispatchHandles) + Cardinality(batch) <= MaxOutstanding
    /\ ~UseBoundedCounter \/
       dispatchCount + Cardinality(batch) <= MaxDispatchCount
    /\ phase' =
        [key \in Keys |-> IF key \in batch THEN "inflight" ELSE phase[key]]
    /\ attempts' =
        [key \in Keys |->
            IF key \in batch
            THEN IF attempts[key] < MaxAttempts THEN attempts[key] + 1 ELSE MaxAttempts
            ELSE attempts[key]]
    /\ inFlight' =
        [key \in Keys |->
            IF key \in batch THEN assignment[key] ELSE inFlight[key]]
    /\ dispatchHandles' =
        dispatchHandles \cup
        {[key |-> key, token |-> assignment[key]] : key \in batch}
    /\ dispatchCount' =
        IF UseBoundedCounter
        THEN dispatchCount + Cardinality(batch)
        ELSE dispatchCount
    /\ identityReuse' =
        (identityReuse \/
         (AssignedTokens(batch, assignment) \cap ReservedTokens # {}))
    /\ lastBatchSize' = Cardinality(batch)
    /\ cursor' =
        LET selected == CHOOSE key \in batch : TRUE
        IN IF QueueRank[selected] = Cardinality(Keys)
           THEN 1
           ELSE QueueRank[selected] + 1
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, owners, obligations,
          charges, usage, migratedCharges, legacyCharges, startupComplete,
          uncertifiedAdvance, staleMutation, badReset, wrongResolve,
          lostDurability, lostBudget, restartAvailable>>

CompleteDispatch(handle, madeProgress) ==
    /\ handle \in dispatchHandles
    /\ dispatchHandles' = dispatchHandles \ {handle}
    /\ IF phase[handle.key] = "inflight" /\
          inFlight[handle.key] = handle.token
       THEN
         /\ phase' =
             [phase EXCEPT
                 ![handle.key] = IF madeProgress THEN "ready" ELSE "backoff"]
         /\ attempts' =
             [attempts EXCEPT
                 ![handle.key] = IF madeProgress THEN 0 ELSE @]
         /\ inFlight' = [inFlight EXCEPT ![handle.key] = NoToken]
         /\ UNCHANGED <<staleMutation>>
       ELSE IF AcceptStaleCompletion /\ phase[handle.key] = "inflight"
       THEN
         /\ phase' = [phase EXCEPT ![handle.key] = "ready"]
         /\ attempts' = [attempts EXCEPT ![handle.key] = 0]
         /\ inFlight' = [inFlight EXCEPT ![handle.key] = NoToken]
         /\ staleMutation' = TRUE
       ELSE
         /\ UNCHANGED <<phase, attempts, inFlight, staleMutation>>
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, owners, obligations,
          dispatchCount, charges, usage, migratedCharges, legacyCharges,
          startupComplete, lastBatchSize, uncertifiedAdvance, badReset,
          wrongResolve, lostDurability, lostBudget, identityReuse,
          restartAvailable>>

DropDispatch(handle) ==
    /\ handle \in dispatchHandles
    /\ dispatchHandles' = dispatchHandles \ {handle}
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, phase, owners, attempts,
          inFlight, obligations, dispatchCount, charges, usage,
          migratedCharges, legacyCharges, startupComplete, lastBatchSize,
          uncertifiedAdvance, staleMutation, badReset, wrongResolve,
          lostDurability, lostBudget, identityReuse, restartAvailable>>

ReclaimExpired(key) ==
    /\ phase[key] = "inflight"
    /\ ~\E handle \in dispatchHandles :
         handle.key = key /\ handle.token = inFlight[key]
    /\ ~RetainExpiredOwnership
    /\ phase' = [phase EXCEPT ![key] = "backoff"]
    /\ inFlight' = [inFlight EXCEPT ![key] = NoToken]
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, owners, attempts, obligations,
          dispatchHandles, dispatchCount, charges, usage, migratedCharges,
          legacyCharges, startupComplete, lastBatchSize,
          uncertifiedAdvance, staleMutation, badReset, wrongResolve,
          lostDurability, lostBudget, identityReuse, restartAvailable>>

TimeoutDispatch(handle) ==
    /\ handle \in dispatchHandles
    /\ phase[handle.key] = "inflight"
    /\ inFlight[handle.key] = handle.token
    /\ dispatchHandles' = dispatchHandles \ {handle}
    /\ phase' = [phase EXCEPT ![handle.key] = "backoff"]
    /\ inFlight' = [inFlight EXCEPT ![handle.key] = NoToken]
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, owners, attempts, obligations,
          dispatchCount, charges, usage, migratedCharges, legacyCharges,
          startupComplete, lastBatchSize, uncertifiedAdvance, staleMutation,
          badReset, wrongResolve, lostDurability, lostBudget, identityReuse,
          restartAvailable>>

BackoffElapsed(key) ==
    /\ phase[key] = "backoff"
    /\ phase' = [phase EXCEPT ![key] = "ready"]
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, owners, attempts, inFlight,
          obligations, dispatchHandles, dispatchCount, charges, usage,
          migratedCharges, legacyCharges, startupComplete, lastBatchSize,
          uncertifiedAdvance, staleMutation, badReset, wrongResolve,
          lostDurability, lostBudget, identityReuse, restartAvailable>>

ResetWithoutProgress(key) ==
    /\ ResetAttemptsWithoutProgress
    /\ key \in Tracked
    /\ attempts[key] > 0
    /\ attempts' = [attempts EXCEPT ![key] = 0]
    /\ badReset' = TRUE
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, phase, owners, inFlight,
          obligations, dispatchHandles, dispatchCount, charges, usage,
          migratedCharges, legacyCharges, startupComplete, lastBatchSize,
          uncertifiedAdvance, staleMutation, wrongResolve, lostDurability,
          lostBudget, identityReuse, restartAvailable>>

ResolveOne(key) ==
    /\ key \in Tracked
    /\ phase' = [phase EXCEPT ![key] = "resolved"]
    /\ owners' = [owners EXCEPT ![key] = {}]
    /\ attempts' = [attempts EXCEPT ![key] = 0]
    /\ inFlight' = [inFlight EXCEPT ![key] = NoToken]
    /\ obligations' = obligations \ {key}
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, dispatchHandles, dispatchCount,
          charges, usage, migratedCharges, legacyCharges, startupComplete,
          lastBatchSize, uncertifiedAdvance, staleMutation, badReset,
          wrongResolve, lostDurability, lostBudget, identityReuse,
          restartAvailable>>

ResolveAnotherKey(requested, removed) ==
    /\ ResolveWrongKey
    /\ requested \in Tracked
    /\ removed \in Tracked \ {requested}
    /\ phase' = [phase EXCEPT ![removed] = "resolved"]
    /\ owners' = [owners EXCEPT ![removed] = {}]
    /\ attempts' = [attempts EXCEPT ![removed] = 0]
    /\ inFlight' = [inFlight EXCEPT ![removed] = NoToken]
    /\ obligations' = obligations \ {removed}
    /\ wrongResolve' = TRUE
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, dispatchHandles, dispatchCount,
          charges, usage, migratedCharges, legacyCharges, startupComplete,
          lastBatchSize, uncertifiedAdvance, staleMutation, badReset,
          lostDurability, lostBudget, identityReuse, restartAvailable>>

CommitCharge(key, owner) ==
    LET charge == ChargeFor(key, owner, currentEpisode)
    IN
    /\ startupComplete
    /\ charge \notin charges
    /\ ~\E existing \in charges :
        existing.episode = currentEpisode /\ existing.target = key
    /\ usage[currentEpisode] < Capacity \/ AllowOvercapacity
    /\ charges' = charges \cup {charge}
    /\ usage' =
        IF ChargeWithoutUsage
        THEN usage
        ELSE [usage EXCEPT ![currentEpisode] = @ + 1]
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, phase, owners, attempts,
          inFlight, obligations, dispatchHandles, dispatchCount, migratedCharges,
          legacyCharges, startupComplete, lastBatchSize,
          uncertifiedAdvance, staleMutation, badReset, wrongResolve,
          lostDurability, lostBudget, identityReuse, restartAvailable>>

IncrementUsageWithoutCharge ==
    /\ UsageWithoutCharge
    /\ startupComplete
    /\ usage' = [usage EXCEPT ![currentEpisode] = @ + 1]
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, phase, owners, attempts,
          inFlight, obligations, dispatchHandles, dispatchCount, charges,
          migratedCharges, legacyCharges, startupComplete, lastBatchSize,
          uncertifiedAdvance, staleMutation, badReset, wrongResolve,
          lostDurability, lostBudget, identityReuse, restartAvailable>>

Restart ==
    /\ restartAvailable
    /\ charges' = IF DropChargesOnRestart THEN {} ELSE charges
    /\ usage' =
        IF DropBudgetOnRestart
        THEN [episode \in Episodes |-> 0]
        ELSE usage
    /\ migratedCharges' = migratedCharges \cap charges'
    /\ phase' =
        [key \in Keys |-> IF key \in obligations THEN "ready" ELSE "absent"]
    /\ owners' =
        [key \in Keys |-> IF key \in obligations THEN owners[key] ELSE {}]
    /\ attempts' =
        [key \in Keys |-> 0]
    /\ inFlight' = [key \in Keys |-> NoToken]
    /\ obligations' = obligations
    /\ dispatchHandles' = {}
    /\ dispatchCount' = IF UseBoundedCounter THEN 0 ELSE dispatchCount
    /\ cursor' = 1
    /\ legacyCharges' = {}
    /\ startupComplete' = TRUE
    /\ lastBatchSize' = 0
    /\ lostDurability' =
        (lostDurability \/ (DropChargesOnRestart /\ charges # {}))
    /\ lostBudget' =
        (lostBudget \/
         (DropBudgetOnRestart /\ \E episode \in Episodes : usage[episode] > 0))
    /\ restartAvailable' = FALSE
    /\ UNCHANGED
        <<currentEpisode, certifiedEpisodes, uncertifiedAdvance,
          staleMutation, badReset, wrongResolve, identityReuse>>

Certify == \E episode \in Episodes : CertifyEpisode(episode)
Advance == \E episode \in Episodes : AdvanceEpisode(episode)
StageLegacy == \E key \in Keys, owner \in Owners : StageLegacyCharge(key, owner)
Register == \E key \in Keys, owner \in Owners : RegisterOne(key, owner)
Evict ==
    \E victim \in Keys, key \in Keys, owner \in Owners :
        EvictAndRegister(victim, key, owner)
Release == \E key \in Keys, owner \in Owners : ReleaseOwner(key, owner)
UnsafeRelease ==
    \E key \in Keys, owner \in Owners : ReleaseLastOwnerUnsafely(key, owner)
Dispatch ==
    \E batch \in SUBSET Keys :
        \E assignment \in [batch -> DispatchIds] :
            DispatchBatch(batch, assignment)
Finish ==
    \E handle \in DispatchHandleType, progress \in BOOLEAN :
        CompleteDispatch(handle, progress)
Drop == \E handle \in DispatchHandleType : DropDispatch(handle)
Timeout == \E handle \in DispatchHandleType : TimeoutDispatch(handle)
Reclaim == \E key \in Keys : ReclaimExpired(key)
Elapsed == \E key \in Keys : BackoffElapsed(key)
UnsafeReset == \E key \in Keys : ResetWithoutProgress(key)
Resolve == \E key \in Keys : ResolveOne(key)
UnsafeResolve ==
    \E requested \in Keys, removed \in Keys :
        ResolveAnotherKey(requested, removed)
Charge == \E key \in Keys, owner \in Owners : CommitCharge(key, owner)

LedgerNext ==
    /\ (Certify \/ Advance \/ StageLegacy \/ MigrateLegacy \/ Charge
        \/ IncrementUsageWithoutCharge)
    /\ cursor' = cursor

WindowNext ==
    Dispatch \/
    ( /\ (Register \/ Evict \/ Release \/ UnsafeRelease \/ Finish \/ Drop
          \/ Timeout \/ Reclaim \/ Elapsed \/ UnsafeReset \/ Resolve
          \/ UnsafeResolve)
      /\ cursor' = cursor )

Next ==
    (EnableLedger /\ LedgerNext) \/ (EnableWindow /\ WindowNext) \/ Restart

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ currentEpisode \in Episodes
    /\ certifiedEpisodes \in SUBSET Episodes
    /\ phase \in [Keys -> Phases]
    /\ owners \in [Keys -> SUBSET Owners]
    /\ attempts \in [Keys -> 0..MaxAttempts]
    /\ inFlight \in [Keys -> DispatchIds \cup {NoToken}]
    /\ obligations \in SUBSET Keys
    /\ dispatchHandles \in SUBSET DispatchHandleType
    /\ Cardinality(dispatchHandles) <= MaxOutstanding
    /\ dispatchCount \in 0..MaxDispatchCount
    /\ charges \in SUBSET ChargeType
    /\ usage \in [Episodes -> Nat]
    /\ migratedCharges \in SUBSET charges
    /\ legacyCharges \in SUBSET ChargeType
    /\ startupComplete \in BOOLEAN
    /\ lastBatchSize \in 0..Cardinality(Keys)
    /\ uncertifiedAdvance \in BOOLEAN
    /\ staleMutation \in BOOLEAN
    /\ badReset \in BOOLEAN
    /\ wrongResolve \in BOOLEAN
    /\ lostDurability \in BOOLEAN
    /\ lostBudget \in BOOLEAN
    /\ identityReuse \in BOOLEAN
    /\ cursor \in 1..Cardinality(Keys)
    /\ restartAvailable \in BOOLEAN
    /\ DispatchIds # {}
    /\ NoToken \notin DispatchIds
    /\ 0 < MaxOutstanding
    /\ MaxOutstanding < Cardinality(DispatchIds)
    /\ EpisodeRank \in [Episodes -> 0..Cardinality(Episodes)]
    /\ \A left, right \in Episodes :
        EpisodeRank[left] = EpisodeRank[right] => left = right
    /\ Generation \in [Owners -> Nat]
    /\ QueueRank \in [Keys -> 1..Cardinality(Keys)]
    /\ \A left, right \in Keys :
        QueueRank[left] = QueueRank[right] => left = right

Inv_CurrentEpisodeCertified == currentEpisode \in certifiedEpisodes

Inv_UsageMatchesCharges ==
    \A episode \in Episodes : usage[episode] = Cardinality(ChargesFor(episode))

Inv_OverCapacityIsLegacyOnly ==
    \A episode \in Episodes :
        usage[episode] > Capacity => ChargesFor(episode) \subseteq migratedCharges

Inv_TrackingIsBounded == Cardinality(Tracked) <= MaxTracked

Inv_ObligationsStayTracked == obligations \subseteq Tracked

Inv_TrackedHasOwner == \A key \in Tracked : owners[key] # {}

Inv_InFlightTokensAreUnique ==
    Cardinality(ActiveTokens) = Cardinality(InFlightKeys)

Inv_LiveIdentityIsNotReused == ~identityReuse

Inv_BatchIsBounded == lastBatchSize <= BatchLimit

Inv_StaleCompletionIsEffectFree == ~staleMutation

Inv_AttemptsResetOnlyAfterProgress == ~badReset

Inv_ResolutionUsesRequestedKey == ~wrongResolve

Inv_DurableChargesSurviveRestart == ~lostDurability

Inv_DurableUsageSurvivesRestart == ~lostBudget

TimeoutOne(key) ==
    \E handle \in DispatchHandleType :
        handle.key = key /\ TimeoutDispatch(handle) /\ cursor' = cursor

DropOne(handle) == DropDispatch(handle) /\ cursor' = cursor

ElapsedOne(key) == BackoffElapsed(key) /\ cursor' = cursor

ReclaimOne(key) == ReclaimExpired(key) /\ cursor' = cursor

LiveSpec ==
    Spec
    /\ WF_vars(Dispatch)
    /\ \A elapsedKey \in Keys : WF_vars(ElapsedOne(elapsedKey))
    /\ \A timeoutKey \in Keys : WF_vars(TimeoutOne(timeoutKey))
    /\ \A expiredKey \in Keys : WF_vars(ReclaimOne(expiredKey))
    /\ \A handle \in DispatchHandleType : WF_vars(DropOne(handle))

EventuallyDispatchesOrResolves ==
    \A key \in Keys :
        (phase[key] = "ready" /\ key \in obligations)
        ~> (phase[key] \in {"inflight", "resolved"})

EventuallyLeavesInFlight ==
    \A key \in Keys :
        (phase[key] = "inflight" /\ key \in obligations)
        ~> (phase[key] \in {"ready", "backoff", "resolved"})

=============================================================================
