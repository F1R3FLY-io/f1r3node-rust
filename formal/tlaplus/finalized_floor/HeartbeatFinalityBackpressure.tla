------------------- MODULE HeartbeatFinalityBackpressure -------------------
EXTENDS Naturals, FiniteSets, Sequences

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
  MaxTick,
  \* @type: Int;
  QueueBound,
  \* @type: Int;
  ThresholdNum,
  \* @type: Int;
  ThresholdDen,
  \* @type: Bool;
  InitialCandidate,
  \* @type: Bool;
  RotateRecovery,
  \* @type: Bool;
  EagerHeartbeat,
  \* @type: Bool;
  EnforceBackpressure,
  \* @type: Bool;
  ReplayPreservesState,
  \* @type: Bool;
  RequireStateCertificate

ValidatorCount == NumValidators

Stake == [validator \in Validators |->
  CASE validator = 1 -> 15
    [] validator = 2 -> 60
    [] OTHER -> 20]

WeightOf(validators) ==
  (IF 1 \in validators THEN Stake[1] ELSE 0) +
  (IF 2 \in validators THEN Stake[2] ELSE 0) +
  (IF 3 \in validators THEN Stake[3] ELSE 0)

TotalStake == WeightOf(Validators)

ASSUME /\ ValidatorCount > 0
       /\ ValidatorCount = 3
       /\ Cardinality(Validators) = NumValidators
       /\ \A validator \in Validators :
             /\ validator >= 1
             /\ validator <= NumValidators
       /\ OnlineValidators \subseteq Validators
       /\ OnlineValidators # {}
       /\ SourceValidator \in Validators
       /\ MaxTick > 0
       /\ QueueBound > 0
       /\ ThresholdDen > 0
       /\ ThresholdNum >= 0
       /\ InitialCandidate \in BOOLEAN
       /\ RotateRecovery \in BOOLEAN
       /\ EagerHeartbeat \in BOOLEAN
       /\ EnforceBackpressure \in BOOLEAN
       /\ ReplayPreservesState \in BOOLEAN
       /\ RequireStateCertificate \in BOOLEAN

ExactCertificate(supporters) ==
  LET weight == WeightOf(supporters)
  IN /\ 2 * weight > TotalStake
     /\ 2 * weight * ThresholdDen
          > TotalStake * (ThresholdDen + ThresholdNum)

RecoveryLeaderAt(recoveryRound) ==
  IF RotateRecovery
  THEN (recoveryRound % ValidatorCount) + 1
  ELSE SourceValidator

VARIABLES
  \* @type: Int;
  tick,
  \* @type: Int -> Int;
  localRound,
  \* @type: Bool;
  candidateExists,
  \* @type: Seq(Int);
  queue,
  \* @type: Set(Int);
  emittedSupport,
  \* @type: Set(<<Int, Int>>);
  attemptedRecovery,
  \* @type: Set(<<Int, Int>>);
  eagerEpochs,
  \* @type: Set(Int);
  causalSupport,
  \* @type: Set(Int);
  stateSupport,
  \* @type: Int;
  floor,
  \* @type: Set(Int);
  promotedCausalSupport,
  \* @type: Set(Int);
  promotedStateSupport

vars ==
  <<tick,
    localRound,
    candidateExists,
    queue,
    emittedSupport,
    attemptedRecovery,
    eagerEpochs,
    causalSupport,
    stateSupport,
    floor,
    promotedCausalSupport,
    promotedStateSupport>>

InitialQueue == IF InitialCandidate THEN <<SourceValidator>> ELSE <<>>
InitialEmitted == IF InitialCandidate THEN {SourceValidator} ELSE {}

Init ==
  /\ tick = 0
  /\ localRound = [validator \in Validators |-> 0]
  /\ candidateExists = InitialCandidate
  /\ queue = InitialQueue
  /\ emittedSupport = InitialEmitted
  /\ attemptedRecovery = {}
  /\ eagerEpochs = {}
  /\ causalSupport = {}
  /\ stateSupport = {}
  /\ floor = 0
  /\ promotedCausalSupport = {}
  /\ promotedStateSupport = {}

CanEnqueue == ~EnforceBackpressure \/ Len(queue) < QueueBound

RecoveryEnabled(validator) ==
  /\ floor = 0
  /\ validator \in OnlineValidators
  /\ validator = RecoveryLeaderAt(localRound[validator])
  /\ <<validator, localRound[validator]>> \notin attemptedRecovery
  /\ CanEnqueue

CreateRecoveryCandidate(validator) ==
  /\ RecoveryEnabled(validator)
  /\ candidateExists' = TRUE
  /\ queue' = Append(queue, validator)
  /\ emittedSupport' = emittedSupport \union {validator}
  /\ attemptedRecovery' =
       attemptedRecovery \union {<<validator, localRound[validator]>>}
  /\ UNCHANGED
       <<tick,
         localRound,
         eagerEpochs,
         causalSupport,
         stateSupport,
         floor,
         promotedCausalSupport,
         promotedStateSupport>>

EmitEagerHeartbeat(validator) ==
  /\ EagerHeartbeat
  /\ candidateExists
  /\ floor = 0
  /\ validator \in OnlineValidators
  /\ <<validator, tick>> \notin eagerEpochs
  /\ CanEnqueue
  /\ queue' = Append(queue, validator)
  /\ eagerEpochs' = eagerEpochs \union {<<validator, tick>>}
  /\ UNCHANGED
       <<tick,
         localRound,
         candidateExists,
         emittedSupport,
         attemptedRecovery,
         causalSupport,
         stateSupport,
         floor,
         promotedCausalSupport,
         promotedStateSupport>>

ValidateHead ==
  /\ Len(queue) > 0
  /\ LET validator == Head(queue)
     IN /\ queue' = Tail(queue)
        /\ causalSupport' = causalSupport \union {validator}
        /\ stateSupport' =
             IF ReplayPreservesState
             THEN stateSupport \union {validator}
             ELSE stateSupport
  /\ UNCHANGED
       <<tick,
         localRound,
         candidateExists,
         emittedSupport,
         attemptedRecovery,
         eagerEpochs,
         floor,
         promotedCausalSupport,
         promotedStateSupport>>

PromoteFloor ==
  /\ floor = 0
  /\ candidateExists
  /\ ExactCertificate(causalSupport)
  /\ (~RequireStateCertificate \/ ExactCertificate(stateSupport))
  /\ floor' = 1
  /\ promotedCausalSupport' = causalSupport
  /\ promotedStateSupport' = stateSupport
  /\ UNCHANGED
       <<tick,
         localRound,
         candidateExists,
         queue,
         emittedSupport,
         attemptedRecovery,
         eagerEpochs,
         causalSupport,
         stateSupport>>

AdvanceClock ==
  /\ tick < MaxTick
  /\ tick' = tick + 1
  /\ UNCHANGED
       <<localRound,
         candidateExists,
         queue,
         emittedSupport,
         attemptedRecovery,
         eagerEpochs,
         causalSupport,
         stateSupport,
         floor,
         promotedCausalSupport,
         promotedStateSupport>>

AdvanceRecoveryView(validator) ==
  /\ floor = 0
  /\ validator \in Validators
  /\ localRound[validator] < MaxTick
  /\ ~RecoveryEnabled(validator)
  /\ localRound' =
       [localRound EXCEPT ![validator] = @ + 1]
  /\ UNCHANGED
       <<tick,
         candidateExists,
         queue,
         emittedSupport,
         attemptedRecovery,
         eagerEpochs,
         causalSupport,
         stateSupport,
         floor,
         promotedCausalSupport,
         promotedStateSupport>>

Idle == floor = 1 /\ UNCHANGED vars

Next ==
  \/ \E validator \in Validators : CreateRecoveryCandidate(validator)
  \/ \E validator \in Validators : EmitEagerHeartbeat(validator)
  \/ ValidateHead
  \/ PromoteFloor
  \/ AdvanceClock
  \/ \E validator \in Validators : AdvanceRecoveryView(validator)
  \/ Idle

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(ValidateHead)
  /\ WF_vars(PromoteFloor)
  /\ WF_vars(AdvanceClock)
  /\ \A validator \in Validators : WF_vars(CreateRecoveryCandidate(validator))
  /\ \A validator \in Validators : WF_vars(AdvanceRecoveryView(validator))

TypeOK ==
  /\ tick \in 0..MaxTick
  /\ localRound \in [Validators -> 0..MaxTick]
  /\ candidateExists \in BOOLEAN
  /\ queue \in Seq(Validators)
  /\ emittedSupport \subseteq Validators
  /\ attemptedRecovery \subseteq Validators \X (0..MaxTick)
  /\ eagerEpochs \subseteq Validators \X (0..MaxTick)
  /\ causalSupport \subseteq Validators
  /\ stateSupport \subseteq Validators
  /\ floor \in {0, 1}
  /\ promotedCausalSupport \subseteq Validators
  /\ promotedStateSupport \subseteq Validators

Inv_ValidationBacklogBounded == Len(queue) <= QueueBound

Inv_BoundedSupportIsSingleShot ==
  ~EagerHeartbeat => Cardinality(emittedSupport) <= ValidatorCount

Inv_OneRecoveryLeaderPerRound ==
  \A recoveryRound \in 0..MaxTick :
    Cardinality(
      {validator \in Validators :
         <<validator, recoveryRound>> \in attemptedRecovery}
    ) <= 1

Inv_RecoveryAttemptMatchesLeader ==
  \A attempt \in attemptedRecovery :
    attempt[1] = RecoveryLeaderAt(attempt[2])

Inv_StateSupportRefinesCausalSupport == stateSupport \subseteq causalSupport

Inv_PromotionUsesExactCausalMajority ==
  floor = 0 \/ ExactCertificate(promotedCausalSupport)

Inv_PromotionUsesExactStateMajority ==
  floor = 0 \/ ExactCertificate(promotedStateSupport)

Inv_PromotionThresholdIsUnchanged ==
  floor = 0
  \/ LET causalWeight == WeightOf(promotedCausalSupport)
         stateWeight == WeightOf(promotedStateSupport)
     IN /\ 2 * causalWeight > TotalStake
        /\ 2 * causalWeight * ThresholdDen
             > TotalStake * (ThresholdDen + ThresholdNum)
        /\ 2 * stateWeight > TotalStake
        /\ 2 * stateWeight * ThresholdDen
             > TotalStake * (ThresholdDen + ThresholdNum)

Live_RecoveryRotatesPastOfflineLeader == <> (floor = 1)
=============================================================================
