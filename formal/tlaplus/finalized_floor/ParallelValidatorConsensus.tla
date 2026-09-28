---------------------- MODULE ParallelValidatorConsensus ----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
  \* @type: Str;
  Defect,
  \* @type: Bool;
  EnableCrashes,
  \* @type: Int;
  MaxActions

Validators == {1, 2, 3}
Candidates == {1, 2}
Blocks == {0, 1, 2}
Effects == {0, 1, 2}
Roots == Blocks
\* @type: Set(<<Int, Int>>);
Pairs == Validators \X Candidates

\* @type: (Int, Int) => <<Int, Int>>;
Pair(validator, candidate) == <<validator, candidate>>

\* @type: (Int, Int, Int) => <<Int, Int, Int>>;
SupportMessage(observer, signer, candidate) == <<observer, signer, candidate>>

ASSUME /\ Defect \in {
           "None",
           "CausalOnlyAcceptance",
           "SupportBeforeValidation",
           "PromotionWithoutLocalReplay",
           "SharedCurrentRootAuthority",
           "SharedCurrentRootPublication",
           "NonAtomicFloorPromotion",
           "StaleFloorPromotion",
           "CrashDeletesRecordedRoot"
         }
       /\ EnableCrashes \in BOOLEAN
       /\ MaxActions \in Nat

Stake == [validator \in Validators |->
  CASE validator = 1 -> 60
    [] validator = 2 -> 20
    [] OTHER -> 15]

WeightOf(validators) ==
  (IF 1 \in validators THEN Stake[1] ELSE 0) +
  (IF 2 \in validators THEN Stake[2] ELSE 0) +
  (IF 3 \in validators THEN Stake[3] ELSE 0)

TotalStake == WeightOf(Validators)
ThresholdNum == 1
ThresholdDen == 10

ExactCertificate(signers) ==
  /\ 2 * WeightOf(signers) > TotalStake
  /\ 2 * WeightOf(signers) * ThresholdDen
       > TotalStake * (ThresholdDen + ThresholdNum)

StateOf(block) ==
  CASE block = 0 -> {0}
    [] block = 1 -> {0, 1}
    [] Defect = "CausalOnlyAcceptance" -> {2}
    [] OTHER -> {0, 1, 2}

OwnEffects(candidate) ==
  IF candidate = 1
  THEN {1}
  ELSE IF Defect = "CausalOnlyAcceptance" THEN {2} ELSE {1, 2}

CanonicalDeployOrigin(candidate) ==
  IF Defect = "CausalOnlyAcceptance" /\ candidate = 2 THEN 2 ELSE 1

VARIABLES
  \* @type: Int;
  actionCount,
  \* @type: Int -> Set(Int);
  knownCandidates,
  \* @type: <<Int, Int>> -> Str;
  phase,
  \* @type: <<Int, Int>> -> Int;
  capturedBlock,
  \* @type: <<Int, Int>> -> Int;
  capturedRoot,
  \* @type: <<Int, Int>> -> Set(Int);
  capturedState,
  \* @type: <<Int, Int>> -> Set(Int);
  replayedState,
  \* @type: Set(<<Int, Int>>);
  emittedSupport,
  \* @type: Set(<<Int, Int, Int>>);
  deliveredSupport,
  \* @type: Int -> Int;
  floorBlock,
  \* @type: Int -> Int;
  floorRoot,
  \* @type: Int -> Set(Int);
  floorState,
  \* @type: Int -> Set(Int);
  committedEffects,
  \* @type: Int -> Set(Int);
  recordedRoots,
  \* @type: Int -> Int;
  currentPointer,
  \* @type: Int -> Set(Int);
  promotionSupport,
  \* @type: Int -> Int;
  deployOrigin

vars == <<
  actionCount,
  knownCandidates,
  phase,
  capturedBlock,
  capturedRoot,
  capturedState,
  replayedState,
  emittedSupport,
  deliveredSupport,
  floorBlock,
  floorRoot,
  floorState,
  committedEffects,
  recordedRoots,
  currentPointer,
  promotionSupport,
  deployOrigin
>>

Init ==
  /\ actionCount = 0
  /\ knownCandidates = [validator \in Validators |-> {}]
  /\ phase = [pair \in Pairs |-> "Idle"]
  /\ capturedBlock = [pair \in Pairs |-> 0]
  /\ capturedRoot = [pair \in Pairs |-> 0]
  /\ capturedState = [pair \in Pairs |-> {}]
  /\ replayedState = [pair \in Pairs |-> {}]
  /\ emittedSupport = {}
  /\ deliveredSupport = {}
  /\ floorBlock = [validator \in Validators |-> 0]
  /\ floorRoot = [validator \in Validators |-> 0]
  /\ floorState = [validator \in Validators |-> StateOf(0)]
  /\ committedEffects = [validator \in Validators |-> StateOf(0)]
  /\ recordedRoots = [validator \in Validators |-> {0}]
  /\ currentPointer = [validator \in Validators |-> 0]
  /\ promotionSupport = [validator \in Validators |-> {}]
  /\ deployOrigin = [validator \in Validators |-> 0]

ReceiveCandidate(validator, candidate) ==
  LET pair == Pair(validator, candidate)
  IN /\ validator \in Validators
     /\ candidate \in Candidates
     /\ actionCount < MaxActions
     /\ phase[pair] = "Idle"
     /\ actionCount' = actionCount + 1
     /\ knownCandidates' =
          [knownCandidates EXCEPT ![validator] = @ \union {candidate}]
     /\ phase' = [phase EXCEPT ![pair] = "Received"]
     /\ UNCHANGED <<
          capturedBlock,
          capturedRoot,
          capturedState,
          replayedState,
          emittedSupport,
          deliveredSupport,
          floorBlock,
          floorRoot,
          floorState,
          committedEffects,
          recordedRoots,
          currentPointer,
          promotionSupport,
          deployOrigin
        >>

CaptureFloor(validator, candidate) ==
  LET pair == Pair(validator, candidate)
      authorityRoot ==
        IF Defect = "SharedCurrentRootAuthority"
        THEN currentPointer[validator]
        ELSE floorRoot[validator]
  IN /\ phase[pair] = "Received"
     /\ actionCount < MaxActions
     /\ actionCount' = actionCount + 1
     /\ phase' = [phase EXCEPT ![pair] = "Captured"]
     /\ capturedBlock' =
          [capturedBlock EXCEPT ![pair] = floorBlock[validator]]
     /\ capturedRoot' =
          [capturedRoot EXCEPT ![pair] = authorityRoot]
     /\ capturedState' =
          [capturedState EXCEPT ![pair] = StateOf(authorityRoot)]
     /\ UNCHANGED <<
          knownCandidates,
          replayedState,
          emittedSupport,
          deliveredSupport,
          floorBlock,
          floorRoot,
          floorState,
          committedEffects,
          recordedRoots,
          currentPointer,
          promotionSupport,
          deployOrigin
        >>

ReplayCandidate(validator, candidate) ==
  LET pair == Pair(validator, candidate)
  IN /\ phase[pair] = "Captured"
     /\ actionCount < MaxActions
     /\ actionCount' = actionCount + 1
     /\ phase' = [phase EXCEPT ![pair] = "Replayed"]
     /\ replayedState' =
          [replayedState EXCEPT
            ![pair] = capturedState[pair] \union OwnEffects(candidate)]
     /\ recordedRoots' =
          [recordedRoots EXCEPT ![validator] = @ \union {candidate}]
     /\ currentPointer' =
          [currentPointer EXCEPT ![validator] = candidate]
     /\ UNCHANGED <<
          knownCandidates,
          capturedBlock,
          capturedRoot,
          capturedState,
          emittedSupport,
          deliveredSupport,
          floorBlock,
          floorRoot,
          floorState,
          committedEffects,
          promotionSupport,
          deployOrigin
        >>

ReplayIsValid(validator, candidate) ==
  LET pair == Pair(validator, candidate)
  IN /\ replayedState[pair] = StateOf(candidate)
     /\ capturedState[pair] \subseteq replayedState[pair]
     /\ candidate \in recordedRoots[validator]

FinishValidation(validator, candidate) ==
  LET pair == Pair(validator, candidate)
      accepted ==
        IF Defect = "CausalOnlyAcceptance"
        THEN TRUE
        ELSE ReplayIsValid(validator, candidate)
  IN /\ phase[pair] = "Replayed"
     /\ actionCount < MaxActions
     /\ actionCount' = actionCount + 1
     /\ phase' =
          [phase EXCEPT ![pair] = IF accepted THEN "Accepted" ELSE "Rejected"]
     /\ UNCHANGED <<
          knownCandidates,
          capturedBlock,
          capturedRoot,
          capturedState,
          replayedState,
          emittedSupport,
          deliveredSupport,
          floorBlock,
          floorRoot,
          floorState,
          committedEffects,
          recordedRoots,
          currentPointer,
          promotionSupport,
          deployOrigin
        >>

EmitSupport(validator, candidate) ==
  LET pair == Pair(validator, candidate)
  IN /\ pair \notin emittedSupport
     /\ actionCount < MaxActions
     /\ IF Defect = "SupportBeforeValidation"
        THEN phase[pair] \in {"Received", "Captured", "Replayed", "Accepted"}
        ELSE phase[pair] = "Accepted"
     /\ actionCount' = actionCount + 1
     /\ emittedSupport' = emittedSupport \union {pair}
     /\ UNCHANGED <<
          knownCandidates,
          phase,
          capturedBlock,
          capturedRoot,
          capturedState,
          replayedState,
          deliveredSupport,
          floorBlock,
          floorRoot,
          floorState,
          committedEffects,
          recordedRoots,
          currentPointer,
          promotionSupport,
          deployOrigin
        >>

ReceiveSupport(observer, signer, candidate) ==
  /\ actionCount < MaxActions
  /\ Pair(signer, candidate) \in emittedSupport
  /\ SupportMessage(observer, signer, candidate) \notin deliveredSupport
  /\ actionCount' = actionCount + 1
  /\ deliveredSupport' =
       deliveredSupport \union {SupportMessage(observer, signer, candidate)}
  /\ UNCHANGED <<
       knownCandidates,
       phase,
       capturedBlock,
       capturedRoot,
       capturedState,
       replayedState,
       emittedSupport,
       floorBlock,
       floorRoot,
       floorState,
       committedEffects,
       recordedRoots,
       currentPointer,
       promotionSupport,
       deployOrigin
     >>

Supporters(observer, candidate) ==
  {signer \in Validators :
    SupportMessage(observer, signer, candidate) \in deliveredSupport}

LocallyCertified(observer, candidate) ==
  ExactCertificate(Supporters(observer, candidate))

PromoteFloor(validator, candidate) ==
  LET pair == Pair(validator, candidate)
      localReplay ==
        \/ phase[pair] = "Accepted"
        \/ Defect = "PromotionWithoutLocalReplay"
      preservesCurrent ==
        \/ floorState[validator] \subseteq StateOf(candidate)
        \/ Defect = "StaleFloorPromotion"
      promotedRoot ==
        IF Defect = "SharedCurrentRootPublication"
        THEN currentPointer[validator]
        ELSE candidate
  IN /\ candidate # floorBlock[validator]
     /\ actionCount < MaxActions
     /\ LocallyCertified(validator, candidate)
     /\ localReplay
     /\ preservesCurrent
     /\ actionCount' = actionCount + 1
     /\ floorBlock' = [floorBlock EXCEPT ![validator] = candidate]
     /\ floorRoot' = [floorRoot EXCEPT ![validator] = promotedRoot]
     /\ floorState' =
          [floorState EXCEPT
            ![validator] =
              IF Defect = "NonAtomicFloorPromotion"
              THEN @
              ELSE StateOf(candidate)]
     /\ committedEffects' =
          [committedEffects EXCEPT
            ![validator] = @ \union StateOf(candidate)]
     /\ promotionSupport' =
          [promotionSupport EXCEPT
            ![validator] = Supporters(validator, candidate)]
     /\ deployOrigin' =
          [deployOrigin EXCEPT
            ![validator] =
              IF 1 \in StateOf(candidate)
              THEN CanonicalDeployOrigin(candidate)
              ELSE @]
     /\ UNCHANGED <<
          knownCandidates,
          phase,
          capturedBlock,
          capturedRoot,
          capturedState,
          replayedState,
          emittedSupport,
          deliveredSupport,
          recordedRoots,
          currentPointer
        >>

CrashTask(validator, candidate) ==
  LET pair == Pair(validator, candidate)
  IN /\ EnableCrashes
     /\ actionCount < MaxActions
     /\ phase[pair] \in {"Captured", "Replayed"}
     /\ actionCount' = actionCount + 1
     /\ phase' = [phase EXCEPT ![pair] = "Crashed"]
     /\ recordedRoots' =
          [recordedRoots EXCEPT
            ![validator] =
              IF Defect = "CrashDeletesRecordedRoot"
                 /\ phase[pair] = "Replayed"
              THEN @ \ {candidate}
              ELSE @]
     /\ UNCHANGED <<
          knownCandidates,
          capturedBlock,
          capturedRoot,
          capturedState,
          replayedState,
          emittedSupport,
          deliveredSupport,
          floorBlock,
          floorRoot,
          floorState,
          committedEffects,
          currentPointer,
          promotionSupport,
          deployOrigin
        >>

RestartTask(validator, candidate) ==
  LET pair == Pair(validator, candidate)
  IN /\ phase[pair] = "Crashed"
     /\ actionCount < MaxActions
     /\ actionCount' = actionCount + 1
     /\ phase' = [phase EXCEPT ![pair] = "Received"]
     /\ capturedBlock' = [capturedBlock EXCEPT ![pair] = 0]
     /\ capturedRoot' = [capturedRoot EXCEPT ![pair] = 0]
     /\ capturedState' = [capturedState EXCEPT ![pair] = {}]
     /\ replayedState' = [replayedState EXCEPT ![pair] = {}]
     /\ UNCHANGED <<
          knownCandidates,
          emittedSupport,
          deliveredSupport,
          floorBlock,
          floorRoot,
          floorState,
          committedEffects,
          recordedRoots,
          currentPointer,
          promotionSupport,
          deployOrigin
        >>

Quiescent ==
  /\ \/ actionCount = MaxActions
     \/ \A validator \in Validators : floorBlock[validator] = 2
  /\ UNCHANGED vars

Next ==
  \/ \E validator \in Validators, candidate \in Candidates :
       ReceiveCandidate(validator, candidate)
  \/ \E validator \in Validators, candidate \in Candidates :
       CaptureFloor(validator, candidate)
  \/ \E validator \in Validators, candidate \in Candidates :
       ReplayCandidate(validator, candidate)
  \/ \E validator \in Validators, candidate \in Candidates :
       FinishValidation(validator, candidate)
  \/ \E validator \in Validators, candidate \in Candidates :
       EmitSupport(validator, candidate)
  \/ \E observer \in Validators,
        signer \in Validators,
        candidate \in Candidates :
       ReceiveSupport(observer, signer, candidate)
  \/ \E validator \in Validators, candidate \in Candidates :
       PromoteFloor(validator, candidate)
  \/ \E validator \in Validators, candidate \in Candidates :
       CrashTask(validator, candidate)
  \/ \E validator \in Validators, candidate \in Candidates :
       RestartTask(validator, candidate)
  \/ Quiescent

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A validator \in Validators, candidate \in Candidates :
       /\ WF_vars(ReceiveCandidate(validator, candidate))
       /\ WF_vars(CaptureFloor(validator, candidate))
       /\ WF_vars(ReplayCandidate(validator, candidate))
       /\ WF_vars(FinishValidation(validator, candidate))
       /\ WF_vars(EmitSupport(validator, candidate))
       /\ WF_vars(PromoteFloor(validator, candidate))
       /\ WF_vars(RestartTask(validator, candidate))
  /\ \A observer \in Validators,
        signer \in Validators,
        candidate \in Candidates :
       WF_vars(ReceiveSupport(observer, signer, candidate))

TypeOK ==
  /\ actionCount \in 0..MaxActions
  /\ knownCandidates \in [Validators -> SUBSET Candidates]
  /\ phase \in [Pairs -> {
       "Idle", "Received", "Captured", "Replayed",
       "Accepted", "Rejected", "Crashed"
     }]
  /\ capturedBlock \in [Pairs -> Blocks]
  /\ capturedRoot \in [Pairs -> Roots]
  /\ capturedState \in [Pairs -> SUBSET Effects]
  /\ replayedState \in [Pairs -> SUBSET Effects]
  /\ emittedSupport \subseteq Pairs
  /\ deliveredSupport \subseteq Validators \X Validators \X Candidates
  /\ floorBlock \in [Validators -> Blocks]
  /\ floorRoot \in [Validators -> Roots]
  /\ floorState \in [Validators -> SUBSET Effects]
  /\ committedEffects \in [Validators -> SUBSET Effects]
  /\ recordedRoots \in [Validators -> SUBSET Roots]
  /\ currentPointer \in [Validators -> Roots]
  /\ promotionSupport \in [Validators -> SUBSET Validators]
  /\ deployOrigin \in [Validators -> Blocks]

ExplicitFloorAuthority ==
  \A pair \in Pairs :
    phase[pair] \notin {"Idle", "Received"} =>
      /\ capturedRoot[pair] = capturedBlock[pair]
      /\ capturedState[pair] = StateOf(capturedBlock[pair])

ReplayRootsRemainLocallyRecorded ==
  \A pair \in Pairs :
    (\/ phase[pair] \in {"Replayed", "Accepted", "Rejected"}
     \/ phase[pair] = "Crashed" /\ replayedState[pair] # {})
    =>
      pair[2] \in recordedRoots[pair[1]]

AcceptedUsesExactReplay ==
  \A pair \in Pairs :
    phase[pair] = "Accepted" =>
      /\ replayedState[pair] = StateOf(pair[2])
      /\ capturedState[pair] \subseteq replayedState[pair]

SupportRequiresLocalAcceptance ==
  \A pair \in emittedSupport : phase[pair] = "Accepted"

DeliveredSupportWasEmitted ==
  \A message \in deliveredSupport :
    Pair(message[2], message[3]) \in emittedSupport

PromotedFloorUsesLocalReplay ==
  \A validator \in Validators :
    floorBlock[validator] # 0 =>
      /\ phase[Pair(validator, floorBlock[validator])] = "Accepted"
      /\ floorBlock[validator] \in recordedRoots[validator]

PromotedFloorUsesExactCertificate ==
  \A validator \in Validators :
    floorBlock[validator] = 0 \/ ExactCertificate(promotionSupport[validator])

FloorPublicationIsAtomic ==
  \A validator \in Validators :
    /\ floorRoot[validator] = floorBlock[validator]
    /\ floorState[validator] = StateOf(floorBlock[validator])

CommittedEffectsRemainInFloor ==
  \A validator \in Validators :
    committedEffects[validator] \subseteq floorState[validator]

HonestFloorsRemainStateCompatible ==
  \A left \in Validators, right \in Validators :
    \/ floorState[left] \subseteq floorState[right]
    \/ floorState[right] \subseteq floorState[left]

CanonicalDeployLookupAgrees ==
  \A left \in Validators, right \in Validators :
    /\ deployOrigin[left] # 0
    /\ deployOrigin[right] # 0
    => deployOrigin[left] = deployOrigin[right]

CurrentPointerNamesRecordedRoot ==
  \A validator \in Validators :
    currentPointer[validator] \in recordedRoots[validator]

AllValidatorsPromoteMergedFloor ==
  <> (\A validator \in Validators : floorBlock[validator] = 2)

=============================================================================
