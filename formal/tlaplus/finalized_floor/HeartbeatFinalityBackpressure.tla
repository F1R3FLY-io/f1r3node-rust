------------------- MODULE HeartbeatFinalityBackpressure -------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
  \* @type: Set(Int);
  Validators,
  \* @type: Int;
  NumValidators,
  \* @type: Set(Int);
  OnlineValidators,
  \* @type: Int;
  SourceValidator,
  \* @type: Int;
  FinalizedHeight,
  \* @type: Int;
  MaxTick,
  \* @type: Int;
  StallTimeout,
  \* @type: Int;
  RecoveryInterval,
  \* @type: Int;
  QueueBound,
  \* @type: Int;
  ThresholdNum,
  \* @type: Int;
  ThresholdDen,
  \* @type: Int;
  Stake1,
  \* @type: Int;
  Stake2,
  \* @type: Int;
  Stake3,
  \* @type: Int;
  StateDroppingValidator,
  \* @type: Bool;
  InitialCandidate,
  \* @type: Bool;
  InitialLayeredSupport,
  \* @type: Bool;
  RotateRecovery,
  \* @type: Bool;
  EagerHeartbeat,
  \* @type: Bool;
  EnforceBackpressure,
  \* @type: Bool;
  ReplayPreservesState,
  \* @type: Bool;
  RequireStateCertificate,
  \* @type: Bool;
  DeliveryWithinRound,
  \* @type: Bool;
  BoundedRecoveryScheduling

ValidatorCount == NumValidators
NoValidator == 0
NoRound == MaxTick + 1
RoundRefs == 0..MaxTick \union {NoRound}
MaxBlocks == NumValidators * (MaxTick + 1) + 3
BlockIds == 0..MaxBlocks
NoBlock == MaxBlocks + 1
BlockRefs == BlockIds \union {NoBlock}

Stake ==
  [validator \in Validators |->
    CASE validator = 1 -> Stake1
      [] validator = 2 -> Stake2
      [] OTHER -> Stake3]

WeightOf(validators) ==
  (IF 1 \in validators THEN Stake1 ELSE 0) +
  (IF 2 \in validators THEN Stake2 ELSE 0) +
  (IF 3 \in validators THEN Stake3 ELSE 0)

TotalStake == WeightOf(Validators)

ASSUME /\ ValidatorCount = 3
       /\ Cardinality(Validators) = NumValidators
       /\ \A validator \in Validators :
             /\ validator >= 1
             /\ validator <= NumValidators
       /\ OnlineValidators \subseteq Validators
       /\ OnlineValidators # {}
       /\ SourceValidator \in Validators
       /\ FinalizedHeight >= 0
       /\ MaxTick > StallTimeout
       /\ StallTimeout > 0
       /\ RecoveryInterval > 0
       /\ QueueBound > 0
       /\ ThresholdDen > 0
       /\ ThresholdNum >= 0
       /\ Stake1 > 0
       /\ Stake2 > 0
       /\ Stake3 > 0
       /\ StateDroppingValidator \in Validators \union {NoValidator}
       /\ InitialCandidate \in BOOLEAN
       /\ InitialLayeredSupport \in BOOLEAN
       /\ ~InitialCandidate \/ ~InitialLayeredSupport
       /\ InitialLayeredSupport => {2, 3} \subseteq OnlineValidators
       /\ RotateRecovery \in BOOLEAN
       /\ EagerHeartbeat \in BOOLEAN
       /\ EnforceBackpressure \in BOOLEAN
       /\ ReplayPreservesState \in BOOLEAN
       /\ RequireStateCertificate \in BOOLEAN
       /\ DeliveryWithinRound \in BOOLEAN
       /\ BoundedRecoveryScheduling \in BOOLEAN

ExactCertificate(supporters) ==
  LET weight == WeightOf(supporters)
  IN /\ 2 * weight > TotalStake
     /\ 2 * weight * ThresholdDen
          > TotalStake * (ThresholdDen + ThresholdNum)

RecoveryRoundAt(duration) ==
  IF duration < StallTimeout
  THEN NoRound
  ELSE (duration - StallTimeout) \div RecoveryInterval

RecoveryLeaderAt(round) ==
  IF RotateRecovery
  THEN ((FinalizedHeight + round) % ValidatorCount) + 1
  ELSE SourceValidator

VARIABLES
  \* @type: Int;
  tick,
  \* @type: Int -> Int;
  elapsed,
  \* @type: Int -> Int;
  nextRecoveryRound,
  \* @type: Int;
  nextBlock,
  \* @type: Int -> Set(Int);
  networkPending,
  \* @type: Int -> Set(Int);
  validationQueue,
  \* @type: Int -> Int;
  creator,
  \* @type: Int -> Set(Int);
  ancestors,
  \* @type: Int -> (Int -> Int);
  views,
  \* @type: Int -> Bool;
  replayPreserving,
  \* @type: Int -> (Int -> Int);
  localLatest,
  \* @type: Set(<<Int, Int>>);
  attemptedRecovery,
  \* @type: Set(<<Int, Int>>);
  eagerEpochs,
  \* @type: Int -> Int;
  floor,
  \* @type: Int -> Int;
  promotedCandidate,
  \* @type: Int -> Set(Int);
  promotedCausalSupport,
  \* @type: Int -> Set(Int);
  promotedStateSupport

vars ==
  <<tick,
    elapsed,
    nextRecoveryRound,
    nextBlock,
    networkPending,
    validationQueue,
    creator,
    ancestors,
    views,
    replayPreserving,
    localLatest,
    attemptedRecovery,
    eagerEpochs,
    floor,
    promotedCandidate,
    promotedCausalSupport,
    promotedStateSupport>>

EmptyView == [validator \in Validators |-> NoBlock]
LayerOneView == [validator \in Validators |-> IF validator = 2 THEN 0 ELSE NoBlock]
LayerTwoView ==
  [validator \in Validators |->
    CASE validator = 2 -> 0
      [] validator = 3 -> 1
      [] OTHER -> NoBlock]

InitialCreator ==
  [block \in BlockIds |->
    IF InitialLayeredSupport
    THEN
      CASE block = 0 -> 2
        [] block = 1 -> 3
        [] block = 2 -> 2
        [] OTHER -> NoValidator
    ELSE
      IF InitialCandidate /\ block = 0
      THEN SourceValidator
      ELSE NoValidator]

InitialAncestors ==
  [block \in BlockIds |->
    IF InitialLayeredSupport
    THEN
      CASE block = 1 -> {0}
        [] block = 2 -> {0, 1}
        [] OTHER -> {}
    ELSE {}]

InitialViews ==
  [block \in BlockIds |->
    IF InitialLayeredSupport
    THEN
      CASE block = 1 -> LayerOneView
        [] block = 2 -> LayerTwoView
        [] OTHER -> EmptyView
    ELSE EmptyView]

BlockReplayPreserves(validator) ==
  ReplayPreservesState /\ validator # StateDroppingValidator

InitialReplay ==
  [block \in BlockIds |->
    IF InitialCreator[block] \in Validators
    THEN BlockReplayPreserves(InitialCreator[block])
    ELSE FALSE]

InitialLatestFor(observer, validator) ==
  IF observer \notin OnlineValidators
  THEN NoBlock
  ELSE
    IF InitialLayeredSupport
    THEN
      CASE validator = 2 -> 2
        [] validator = 3 -> 1
        [] OTHER -> NoBlock
    ELSE
      IF InitialCandidate /\ validator = SourceValidator
      THEN 0
      ELSE NoBlock

Init ==
  /\ tick = 0
  /\ elapsed = [validator \in Validators |-> 0]
  /\ nextRecoveryRound = [validator \in Validators |-> 0]
  /\ nextBlock =
       IF InitialLayeredSupport THEN 3
       ELSE IF InitialCandidate THEN 1
       ELSE 0
  /\ networkPending = [validator \in Validators |-> {}]
  /\ validationQueue = [validator \in Validators |-> {}]
  /\ creator = InitialCreator
  /\ ancestors = InitialAncestors
  /\ views = InitialViews
  /\ replayPreserving = InitialReplay
  /\ localLatest =
       [observer \in Validators |->
         [validator \in Validators |->
           InitialLatestFor(observer, validator)]]
  /\ attemptedRecovery = {}
  /\ eagerEpochs = {}
  /\ floor = [validator \in Validators |-> 0]
  /\ promotedCandidate = [validator \in Validators |-> NoBlock]
  /\ promotedCausalSupport = [validator \in Validators |-> {}]
  /\ promotedStateSupport = [validator \in Validators |-> {}]

ProducedBlocks == {block \in BlockIds : creator[block] \in Validators}

ObservedBlocks(observer) ==
  {block \in BlockIds :
    \E validator \in Validators :
      localLatest[observer][validator] = block}

ObservedClosure(observer) ==
  UNION
    {ancestors[block] \union {block} :
      block \in ObservedBlocks(observer)}

DescendsFrom(block, target) ==
  /\ block \in ProducedBlocks
  /\ target \in ProducedBlocks
  /\ target \in ancestors[block] \union {block}

StateDescendsFrom(block, target) ==
  /\ DescendsFrom(block, target)
  /\ \A predecessor \in ancestors[block] \union {block} :
       DescendsFrom(predecessor, target)
       => replayPreserving[predecessor]

LatestSupports(finalizer, target, validator) ==
  LET latest == localLatest[finalizer][validator]
  IN latest # NoBlock /\ DescendsFrom(latest, target)

LatestStateSupports(finalizer, target, validator) ==
  LET latest == localLatest[finalizer][validator]
  IN latest # NoBlock /\ StateDescendsFrom(latest, target)

ViewSupports(finalizer, target, observer, subject) ==
  IF observer = subject
  THEN LatestSupports(finalizer, target, observer)
  ELSE
    LET observerLatest == localLatest[finalizer][observer]
    IN IF observerLatest = NoBlock
       THEN FALSE
       ELSE /\ views[observerLatest][subject] # NoBlock
            /\ DescendsFrom(views[observerLatest][subject], target)

ViewStateSupports(finalizer, target, observer, subject) ==
  IF observer = subject
  THEN LatestStateSupports(finalizer, target, observer)
  ELSE
    LET observerLatest == localLatest[finalizer][observer]
    IN IF observerLatest = NoBlock
       THEN FALSE
       ELSE
         LET observedLatest == views[observerLatest][subject]
         IN /\ observedLatest # NoBlock
            /\ StateDescendsFrom(observedLatest, target)

MutualCausalClique(finalizer, target, supporters) ==
  /\ supporters \subseteq Validators
  /\ supporters # {}
  /\ \A observer \in supporters :
       \A subject \in supporters :
         ViewSupports(finalizer, target, observer, subject)

MutualStateClique(finalizer, target, supporters) ==
  /\ supporters \subseteq Validators
  /\ supporters # {}
  /\ \A observer \in supporters :
       \A subject \in supporters :
         ViewStateSupports(finalizer, target, observer, subject)

CausalSupport(finalizer, target) ==
  {validator \in Validators :
    LatestSupports(finalizer, target, validator)}

StateSupport(finalizer, target) ==
  {validator \in Validators :
    LatestStateSupports(finalizer, target, validator)}

Backlog(observer) ==
  networkPending[observer] \union validationQueue[observer]

CanEnqueue ==
  ~EnforceBackpressure
  \/ \A observer \in OnlineValidators :
       Cardinality(Backlog(observer)) < QueueBound

AllPendingEmpty ==
  \A observer \in OnlineValidators :
    /\ networkPending[observer] = {}
    /\ validationQueue[observer] = {}

RecoveryDue(validator) ==
  LET highestAvailable == RecoveryRoundAt(elapsed[validator])
      nextRound == nextRecoveryRound[validator]
  IN /\ highestAvailable # NoRound
     /\ nextRound <= highestAvailable

RecoverySchedulingAllows(validator) ==
  ~BoundedRecoveryScheduling
  \/ \A peer \in OnlineValidators :
       nextRecoveryRound[validator] <= nextRecoveryRound[peer]

RecoveryEnabled(validator) ==
  LET round == nextRecoveryRound[validator]
  IN /\ floor[validator] = 0
     /\ RecoveryDue(validator)
     /\ validator = RecoveryLeaderAt(round)
     /\ validator \in OnlineValidators
     /\ RecoverySchedulingAllows(validator)
     /\ <<validator, round>> \notin attemptedRecovery
     /\ nextBlock <= MaxBlocks
     /\ Backlog(validator) = {}
     /\ (~DeliveryWithinRound \/ AllPendingEmpty)
     /\ CanEnqueue

CreateRecoveryBlock(validator) ==
  /\ RecoveryEnabled(validator)
  /\ LET round == nextRecoveryRound[validator]
     IN /\ creator' = [creator EXCEPT ![nextBlock] = validator]
        /\ ancestors' =
             [ancestors EXCEPT ![nextBlock] = ObservedClosure(validator)]
        /\ views' = [views EXCEPT ![nextBlock] = localLatest[validator]]
        /\ replayPreserving' =
             [replayPreserving EXCEPT
               ![nextBlock] = BlockReplayPreserves(validator)]
        /\ networkPending' =
             [observer \in Validators |->
               IF observer \in OnlineValidators /\ observer # validator
               THEN networkPending[observer] \union {nextBlock}
               ELSE networkPending[observer]]
        /\ validationQueue' =
             [observer \in Validators |->
               IF observer = validator
               THEN validationQueue[observer] \union {nextBlock}
               ELSE validationQueue[observer]]
        /\ nextBlock' = nextBlock + 1
        /\ attemptedRecovery' =
             attemptedRecovery \union {<<validator, round>>}
        /\ nextRecoveryRound' =
             [nextRecoveryRound EXCEPT ![validator] = @ + 1]
  /\ UNCHANGED
       <<tick,
         elapsed,
         localLatest,
         eagerEpochs,
         floor,
         promotedCandidate,
         promotedCausalSupport,
         promotedStateSupport>>

EmitEagerHeartbeat(validator) ==
  /\ EagerHeartbeat
  /\ ProducedBlocks # {}
  /\ floor[validator] = 0
  /\ validator \in OnlineValidators
  /\ <<validator, tick>> \notin eagerEpochs
  /\ nextBlock <= MaxBlocks
  /\ CanEnqueue
  /\ creator' = [creator EXCEPT ![nextBlock] = validator]
  /\ ancestors' =
       [ancestors EXCEPT ![nextBlock] = ObservedClosure(validator)]
  /\ views' = [views EXCEPT ![nextBlock] = localLatest[validator]]
  /\ replayPreserving' =
       [replayPreserving EXCEPT
         ![nextBlock] = BlockReplayPreserves(validator)]
  /\ networkPending' =
       [observer \in Validators |->
         IF observer \in OnlineValidators /\ observer # validator
         THEN networkPending[observer] \union {nextBlock}
         ELSE networkPending[observer]]
  /\ validationQueue' =
       [observer \in Validators |->
         IF observer = validator
         THEN validationQueue[observer] \union {nextBlock}
         ELSE validationQueue[observer]]
  /\ nextBlock' = nextBlock + 1
  /\ eagerEpochs' = eagerEpochs \union {<<validator, tick>>}
  /\ UNCHANGED
       <<tick,
         elapsed,
         nextRecoveryRound,
         localLatest,
         attemptedRecovery,
         floor,
         promotedCandidate,
         promotedCausalSupport,
         promotedStateSupport>>

Deliver(observer, block) ==
  /\ observer \in OnlineValidators
  /\ block \in networkPending[observer]
  /\ Cardinality(validationQueue[observer]) < QueueBound
  /\ networkPending' =
       [networkPending EXCEPT ![observer] = @ \ {block}]
  /\ validationQueue' =
       [validationQueue EXCEPT ![observer] = @ \union {block}]
  /\ UNCHANGED
       <<tick,
         elapsed,
         nextRecoveryRound,
         nextBlock,
         creator,
         ancestors,
         views,
         replayPreserving,
         localLatest,
         attemptedRecovery,
         eagerEpochs,
         floor,
         promotedCandidate,
         promotedCausalSupport,
         promotedStateSupport>>

CanValidate(observer, block) ==
  LET validator == creator[block]
      current == localLatest[observer][validator]
  IN /\ block \in validationQueue[observer]
     /\ ancestors[block] \subseteq ObservedClosure(observer)
     /\ current = NoBlock \/ DescendsFrom(block, current)

Validate(observer, block) ==
  /\ observer \in OnlineValidators
  /\ CanValidate(observer, block)
  /\ validationQueue' =
       [validationQueue EXCEPT ![observer] = @ \ {block}]
  /\ localLatest' =
       [localLatest EXCEPT
         ![observer][creator[block]] = block]
  /\ UNCHANGED
       <<tick,
         elapsed,
         nextRecoveryRound,
         nextBlock,
         networkPending,
         creator,
         ancestors,
         views,
         replayPreserving,
         attemptedRecovery,
         eagerEpochs,
         floor,
         promotedCandidate,
         promotedCausalSupport,
         promotedStateSupport>>

SkipRecoveryRound(validator) ==
  /\ floor[validator] = 0
  /\ validator \in OnlineValidators
  /\ RecoveryDue(validator)
  /\ RecoverySchedulingAllows(validator)
  /\ RecoveryLeaderAt(nextRecoveryRound[validator]) # validator
  /\ nextRecoveryRound' =
       [nextRecoveryRound EXCEPT ![validator] = @ + 1]
  /\ UNCHANGED
       <<tick,
         elapsed,
         nextBlock,
         networkPending,
         validationQueue,
         creator,
         ancestors,
         views,
         replayPreserving,
         localLatest,
         attemptedRecovery,
         eagerEpochs,
         floor,
         promotedCandidate,
         promotedCausalSupport,
         promotedStateSupport>>

AdvanceElapsed(validator, nextElapsed) ==
  /\ validator \in OnlineValidators
  /\ elapsed[validator] < nextElapsed
  /\ nextElapsed <= MaxTick
  /\ elapsed' = [elapsed EXCEPT ![validator] = nextElapsed]
  /\ UNCHANGED
       <<tick,
         nextRecoveryRound,
         nextBlock,
         networkPending,
         validationQueue,
         creator,
         ancestors,
         views,
         replayPreserving,
         localLatest,
         attemptedRecovery,
         eagerEpochs,
         floor,
         promotedCandidate,
         promotedCausalSupport,
         promotedStateSupport>>

AdvanceClock ==
  /\ tick < MaxTick
  /\ tick' = tick + 1
  /\ UNCHANGED
       <<elapsed,
         nextRecoveryRound,
         nextBlock,
         networkPending,
         validationQueue,
         creator,
         ancestors,
         views,
         replayPreserving,
         localLatest,
         attemptedRecovery,
         eagerEpochs,
         floor,
         promotedCandidate,
         promotedCausalSupport,
         promotedStateSupport>>

PromoteFloor(finalizer, target, supporters) ==
  /\ finalizer \in OnlineValidators
  /\ floor[finalizer] = 0
  /\ target \in ProducedBlocks
  /\ ExactCertificate(supporters)
  /\ MutualCausalClique(finalizer, target, supporters)
  /\ (~RequireStateCertificate
       \/ MutualStateClique(finalizer, target, supporters))
  /\ floor' = [floor EXCEPT ![finalizer] = 1]
  /\ promotedCandidate' =
       [promotedCandidate EXCEPT ![finalizer] = target]
  /\ promotedCausalSupport' =
       [promotedCausalSupport EXCEPT ![finalizer] = supporters]
  /\ promotedStateSupport' =
       [promotedStateSupport EXCEPT
         ![finalizer] = StateSupport(finalizer, target) \cap supporters]
  /\ UNCHANGED
       <<tick,
         elapsed,
         nextRecoveryRound,
         nextBlock,
         networkPending,
         validationQueue,
         creator,
         ancestors,
         views,
         replayPreserving,
         localLatest,
         attemptedRecovery,
         eagerEpochs>>

Idle ==
  (\A validator \in OnlineValidators : floor[validator] = 1)
  /\ UNCHANGED vars

Next ==
  \/ \E validator \in Validators : CreateRecoveryBlock(validator)
  \/ \E validator \in Validators : EmitEagerHeartbeat(validator)
  \/ \E observer \in Validators :
       \E block \in BlockIds : Deliver(observer, block)
  \/ \E observer \in Validators :
       \E block \in BlockIds : Validate(observer, block)
  \/ \E validator \in Validators : SkipRecoveryRound(validator)
  \/ \E validator \in Validators :
       \E nextElapsed \in 0..MaxTick :
         AdvanceElapsed(validator, nextElapsed)
  \/ AdvanceClock
  \/ \E finalizer \in Validators :
       \E target \in BlockIds :
         \E supporters \in SUBSET Validators :
           PromoteFloor(finalizer, target, supporters)
  \/ Idle

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A validator \in Validators :
       WF_vars(CreateRecoveryBlock(validator))
  /\ \A observer \in Validators :
       \A block \in BlockIds :
         /\ WF_vars(Deliver(observer, block))
         /\ WF_vars(Validate(observer, block))
  /\ \A validator \in Validators :
       WF_vars(SkipRecoveryRound(validator))
  /\ \A validator \in Validators :
       \A nextElapsed \in 0..MaxTick :
         WF_vars(AdvanceElapsed(validator, nextElapsed))
  /\ \A finalizer \in Validators :
       \A target \in BlockIds :
         \A supporters \in SUBSET Validators :
           WF_vars(PromoteFloor(finalizer, target, supporters))

TypeOK ==
  /\ tick \in 0..MaxTick
  /\ elapsed \in [Validators -> 0..MaxTick]
  /\ nextRecoveryRound \in [Validators -> RoundRefs]
  /\ nextBlock \in 0..(MaxBlocks + 1)
  /\ networkPending \in [Validators -> SUBSET BlockIds]
  /\ validationQueue \in [Validators -> SUBSET BlockIds]
  /\ creator \in [BlockIds -> (Validators \union {NoValidator})]
  /\ ancestors \in [BlockIds -> SUBSET BlockIds]
  /\ views \in [BlockIds -> [Validators -> BlockRefs]]
  /\ replayPreserving \in [BlockIds -> BOOLEAN]
  /\ localLatest \in [Validators -> [Validators -> BlockRefs]]
  /\ attemptedRecovery \subseteq Validators \X (0..MaxTick)
  /\ eagerEpochs \subseteq Validators \X (0..MaxTick)
  /\ floor \in [Validators -> {0, 1}]
  /\ promotedCandidate \in [Validators -> BlockRefs]
  /\ promotedCausalSupport \in [Validators -> SUBSET Validators]
  /\ promotedStateSupport \in [Validators -> SUBSET Validators]

Inv_ValidationBacklogBounded ==
  \A observer \in Validators :
    Cardinality(Backlog(observer)) <= QueueBound

Inv_BoundedSupportIsSingleShot ==
  ~EagerHeartbeat =>
    Cardinality(ProducedBlocks)
      <= Cardinality(attemptedRecovery)
         + IF InitialLayeredSupport THEN 3
           ELSE IF InitialCandidate THEN 1
           ELSE 0

Inv_OneRecoveryLeaderPerRound ==
  \A round \in 0..MaxTick :
    Cardinality(
      {validator \in Validators :
         <<validator, round>> \in attemptedRecovery}
    ) <= 1

Inv_RecoveryAttemptMatchesLeader ==
  \A attempt \in attemptedRecovery :
    attempt[1] = RecoveryLeaderAt(attempt[2])

Inv_BlockViewsReferenceAncestry ==
  \A block \in ProducedBlocks :
    \A validator \in Validators :
      views[block][validator] = NoBlock
      \/ views[block][validator] \in ancestors[block]

Inv_LocalLatestIsProduced ==
  \A observer \in Validators :
    \A validator \in Validators :
      localLatest[observer][validator] = NoBlock
      \/ localLatest[observer][validator] \in ProducedBlocks

Inv_StateSupportRefinesCausalSupport ==
  \A finalizer \in Validators :
    \A target \in ProducedBlocks :
      StateSupport(finalizer, target)
        \subseteq CausalSupport(finalizer, target)

Inv_PromotionUsesMutualCausalClique ==
  \A finalizer \in Validators :
    floor[finalizer] = 0
    \/ MutualCausalClique(
         finalizer,
         promotedCandidate[finalizer],
         promotedCausalSupport[finalizer])

Inv_PromotionUsesExactCausalMajority ==
  \A finalizer \in Validators :
    floor[finalizer] = 0
    \/ ExactCertificate(promotedCausalSupport[finalizer])

Inv_PromotionUsesExactStateMajority ==
  \A finalizer \in Validators :
    floor[finalizer] = 0
    \/ ExactCertificate(promotedStateSupport[finalizer])

Inv_PromotionThresholdIsUnchanged ==
  \A finalizer \in Validators :
    floor[finalizer] = 0
    \/ LET causalWeight == WeightOf(promotedCausalSupport[finalizer])
           stateWeight == WeightOf(promotedStateSupport[finalizer])
       IN /\ 2 * causalWeight > TotalStake
          /\ 2 * causalWeight * ThresholdDen
               > TotalStake * (ThresholdDen + ThresholdNum)
          /\ 2 * stateWeight > TotalStake
          /\ 2 * stateWeight * ThresholdDen
               > TotalStake * (ThresholdDen + ThresholdNum)

Inv_OnlinePromotionsAreCompatible ==
  \A left \in OnlineValidators :
    \A right \in OnlineValidators :
      floor[left] = 0
      \/ floor[right] = 0
      \/ promotedCandidate[left] = promotedCandidate[right]
      \/ StateDescendsFrom(
           promotedCandidate[left],
           promotedCandidate[right])
      \/ StateDescendsFrom(
           promotedCandidate[right],
           promotedCandidate[left])

Inv_NoPromotion ==
  \A validator \in OnlineValidators : floor[validator] = 0

Live_RecoveryRotatesPastOfflineLeader ==
  \A validator \in OnlineValidators : <> (floor[validator] = 1)
=============================================================================
