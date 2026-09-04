---------------------- MODULE StaleSiblingRecovery ----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
    \* @type: Int;
    NumValidators,
    \* @type: Int;
    CarrierOwner,
    \* @type: Bool;
    RetainAcceptedStale,
    \* @type: Bool;
    CompleteParentFrontier,
    \* @type: Bool;
    ExactSourceTombstone,
    \* @type: Bool;
    PropagateRejectedBuffer,
    \* @type: Bool;
    PreserveSelectedRecovery,
    \* @type: Bool;
    PreserveFloorEffect,
    \* @type: Bool;
    EnforceCarrierCustody

ASSUME /\ NumValidators \in Nat \ {0}
       /\ CarrierOwner \in 1..NumValidators
       /\ RetainAcceptedStale \in BOOLEAN
       /\ CompleteParentFrontier \in BOOLEAN
       /\ ExactSourceTombstone \in BOOLEAN
       /\ PropagateRejectedBuffer \in BOOLEAN
       /\ PreserveSelectedRecovery \in BOOLEAN
       /\ PreserveFloorEffect \in BOOLEAN
       /\ EnforceCarrierCustody \in BOOLEAN

Validators == 1..NumValidators
Effects == {"A", "B", "Fresh"}
Parents == {"A", "B"}
TombstoneSources == {"None", "A", "SignatureOnly"}

VARIABLES
    \* @type: Set(Int);
    seenA,
    \* @type: Set(Int);
    seenB,
    \* @type: Bool;
    bFinalized,
    \* @type: Set(Int);
    seenFloor,
    \* @type: Set(Int);
    causalA,
    \* @type: Set(Str);
    floorEffects,
    \* @type: Bool;
    settlementPublished,
    \* @type: Set(Str);
    settlementParents,
    \* @type: Set(Str);
    settlementEffects,
    \* @type: Str;
    tombstoneSource,
    \* @type: Set(Int);
    seenSettlement,
    \* @type: Set(Int);
    seenTombstone,
    \* @type: Set(Int);
    bufferedA,
    \* @type: Bool;
    recoveryPublished,
    \* @type: Int;
    recoveryPublisher,
    \* @type: Set(Str);
    recoveryEffects,
    \* @type: Set(Int);
    seenRecovery,
    \* @type: Int -> Set(Str);
    localEffects,
    \* @type: Bool;
    recoveryFinalized,
    \* @type: Set(Str);
    finalEffects

vars ==
    <<seenA,
      seenB,
      bFinalized,
      seenFloor,
      causalA,
      floorEffects,
      settlementPublished,
      settlementParents,
      settlementEffects,
      tombstoneSource,
      seenSettlement,
      seenTombstone,
      bufferedA,
      recoveryPublished,
      recoveryPublisher,
      recoveryEffects,
      seenRecovery,
      localEffects,
      recoveryFinalized,
      finalEffects>>

Init ==
    /\ seenA = {}
    /\ seenB = {}
    /\ bFinalized = FALSE
    /\ seenFloor = {}
    /\ causalA = {}
    /\ floorEffects = {}
    /\ settlementPublished = FALSE
    /\ settlementParents = {}
    /\ settlementEffects = {}
    /\ tombstoneSource = "None"
    /\ seenSettlement = {}
    /\ seenTombstone = {}
    /\ bufferedA = {}
    /\ recoveryPublished = FALSE
    /\ recoveryPublisher = 0
    /\ recoveryEffects = {}
    /\ seenRecovery = {}
    /\ localEffects = [v \in Validators |-> {}]
    /\ recoveryFinalized = FALSE
    /\ finalEffects = {}

ObserveA(v) ==
    /\ v \in Validators \ seenA
    /\ seenA' = seenA \union {v}
    /\ causalA' =
       IF v \in seenFloor /\ ~RetainAcceptedStale
       THEN causalA
       ELSE causalA \union {v}
    /\ UNCHANGED
       <<seenB,
         bFinalized,
         seenFloor,
         floorEffects,
         settlementPublished,
         settlementParents,
         settlementEffects,
         tombstoneSource,
         seenSettlement,
         seenTombstone,
         bufferedA,
         recoveryPublished,
         recoveryPublisher,
         recoveryEffects,
         seenRecovery,
         localEffects,
         recoveryFinalized,
         finalEffects>>

ObserveB(v) ==
    /\ v \in Validators \ seenB
    /\ seenB' = seenB \union {v}
    /\ UNCHANGED
       <<seenA,
         bFinalized,
         seenFloor,
         causalA,
         floorEffects,
         settlementPublished,
         settlementParents,
         settlementEffects,
         tombstoneSource,
         seenSettlement,
         seenTombstone,
         bufferedA,
         recoveryPublished,
         recoveryPublisher,
         recoveryEffects,
         seenRecovery,
         localEffects,
         recoveryFinalized,
         finalEffects>>

FinalizeB ==
    /\ ~bFinalized
    /\ seenB = Validators
    /\ bFinalized' = TRUE
    /\ floorEffects' = IF PreserveFloorEffect THEN {"B"} ELSE {}
    /\ UNCHANGED
       <<seenA,
         seenB,
         seenFloor,
         causalA,
         settlementPublished,
         settlementParents,
         settlementEffects,
         tombstoneSource,
         seenSettlement,
         seenTombstone,
         bufferedA,
         recoveryPublished,
         recoveryPublisher,
         recoveryEffects,
         seenRecovery,
         localEffects,
         recoveryFinalized,
         finalEffects>>

ObserveFloor(v) ==
    /\ bFinalized
    /\ v \in Validators \ seenFloor
    /\ seenFloor' = seenFloor \union {v}
    /\ causalA' = IF RetainAcceptedStale THEN causalA ELSE causalA \ {v}
    /\ UNCHANGED
       <<seenA,
         seenB,
         bFinalized,
         floorEffects,
         settlementPublished,
         settlementParents,
         settlementEffects,
         tombstoneSource,
         seenSettlement,
         seenTombstone,
         bufferedA,
         recoveryPublished,
         recoveryPublisher,
         recoveryEffects,
         seenRecovery,
         localEffects,
         recoveryFinalized,
         finalEffects>>

PublishSettlement(v) ==
    /\ ~settlementPublished
    /\ v \in seenA \intersect seenB \intersect seenFloor
    /\ settlementPublished' = TRUE
    /\ settlementParents' = IF CompleteParentFrontier THEN Parents ELSE {"B"}
    /\ settlementEffects' = floorEffects
    /\ tombstoneSource' =
       IF CompleteParentFrontier
       THEN IF ExactSourceTombstone THEN "A" ELSE "SignatureOnly"
       ELSE "None"
    /\ UNCHANGED
       <<seenA,
         seenB,
         bFinalized,
         seenFloor,
         causalA,
         floorEffects,
         seenSettlement,
         seenTombstone,
         bufferedA,
         recoveryPublished,
         recoveryPublisher,
         recoveryEffects,
         seenRecovery,
         localEffects,
         recoveryFinalized,
         finalEffects>>

ObserveSettlement(v) ==
    /\ settlementPublished
    /\ v \in seenA \intersect seenB \intersect seenFloor
    /\ v \in Validators \ seenSettlement
    /\ seenSettlement' = seenSettlement \union {v}
    /\ seenTombstone' =
       IF tombstoneSource = "None" THEN seenTombstone ELSE seenTombstone \union {v}
    /\ bufferedA' =
       IF PropagateRejectedBuffer
          /\ tombstoneSource /= "None"
          /\ (IF EnforceCarrierCustody THEN v = CarrierOwner ELSE TRUE)
       THEN bufferedA \union {v}
       ELSE bufferedA
    /\ causalA' = causalA \ {v}
    /\ UNCHANGED
       <<seenA,
         seenB,
         bFinalized,
         seenFloor,
         floorEffects,
         settlementPublished,
         settlementParents,
         settlementEffects,
         tombstoneSource,
         recoveryPublished,
         recoveryPublisher,
         recoveryEffects,
         seenRecovery,
         localEffects,
         recoveryFinalized,
         finalEffects>>

PublishRecovery(v) ==
    /\ ~recoveryPublished
    /\ v \in seenSettlement \intersect seenTombstone \intersect bufferedA
    /\ tombstoneSource = "A"
    /\ IF EnforceCarrierCustody THEN v = CarrierOwner ELSE TRUE
    /\ recoveryPublished' = TRUE
    /\ recoveryPublisher' = v
    /\ recoveryEffects' =
       settlementEffects \union {"Fresh"} \union
         (IF PreserveSelectedRecovery THEN {"A"} ELSE {})
    /\ UNCHANGED
       <<seenA,
         seenB,
         bFinalized,
         seenFloor,
         causalA,
         floorEffects,
         settlementPublished,
         settlementParents,
         settlementEffects,
         tombstoneSource,
         seenSettlement,
         seenTombstone,
         bufferedA,
         seenRecovery,
         localEffects,
         recoveryFinalized,
         finalEffects>>

ObserveRecovery(v) ==
    /\ recoveryPublished
    /\ v \in Validators \ seenRecovery
    /\ seenRecovery' = seenRecovery \union {v}
    /\ localEffects' = [localEffects EXCEPT ![v] = recoveryEffects]
    /\ UNCHANGED
       <<seenA,
         seenB,
         bFinalized,
         seenFloor,
         causalA,
         floorEffects,
         settlementPublished,
         settlementParents,
         settlementEffects,
         tombstoneSource,
         seenSettlement,
         seenTombstone,
         bufferedA,
         recoveryPublished,
         recoveryPublisher,
         recoveryEffects,
         recoveryFinalized,
         finalEffects>>

FinalizeRecovery ==
    /\ recoveryPublished
    /\ ~recoveryFinalized
    /\ seenRecovery = Validators
    /\ recoveryFinalized' = TRUE
    /\ finalEffects' = recoveryEffects
    /\ UNCHANGED
       <<seenA,
         seenB,
         bFinalized,
         seenFloor,
         causalA,
         floorEffects,
         settlementPublished,
         settlementParents,
         settlementEffects,
         tombstoneSource,
         seenSettlement,
         seenTombstone,
         bufferedA,
         recoveryPublished,
         recoveryPublisher,
         recoveryEffects,
         seenRecovery,
         localEffects>>

Next ==
    \/ \E v \in Validators : ObserveA(v)
    \/ \E v \in Validators : ObserveB(v)
    \/ FinalizeB
    \/ \E v \in Validators : ObserveFloor(v)
    \/ \E v \in Validators : PublishSettlement(v)
    \/ \E v \in Validators : ObserveSettlement(v)
    \/ \E v \in Validators : PublishRecovery(v)
    \/ \E v \in Validators : ObserveRecovery(v)
    \/ FinalizeRecovery

Fairness ==
    /\ \A v \in Validators : WF_vars(ObserveA(v))
    /\ \A v \in Validators : WF_vars(ObserveB(v))
    /\ WF_vars(FinalizeB)
    /\ \A v \in Validators : WF_vars(ObserveFloor(v))
    /\ \A v \in Validators : WF_vars(PublishSettlement(v))
    /\ \A v \in Validators : WF_vars(ObserveSettlement(v))
    /\ \A v \in Validators : WF_vars(PublishRecovery(v))
    /\ \A v \in Validators : WF_vars(ObserveRecovery(v))
    /\ WF_vars(FinalizeRecovery)

Spec == Init /\ [][Next]_vars /\ Fairness

TypeOK ==
    /\ seenA \subseteq Validators
    /\ seenB \subseteq Validators
    /\ bFinalized \in BOOLEAN
    /\ seenFloor \subseteq Validators
    /\ causalA \subseteq Validators
    /\ floorEffects \subseteq Effects
    /\ settlementPublished \in BOOLEAN
    /\ settlementParents \subseteq Parents
    /\ settlementEffects \subseteq Effects
    /\ tombstoneSource \in TombstoneSources
    /\ seenSettlement \subseteq Validators
    /\ seenTombstone \subseteq Validators
    /\ bufferedA \subseteq Validators
    /\ recoveryPublished \in BOOLEAN
    /\ recoveryPublisher \in 0..NumValidators
    /\ recoveryEffects \subseteq Effects
    /\ seenRecovery \subseteq Validators
    /\ localEffects \in [Validators -> SUBSET Effects]
    /\ recoveryFinalized \in BOOLEAN
    /\ finalEffects \subseteq Effects

Inv_AcceptedStaleRemainsCausal ==
    \A v \in Validators :
        v \in seenA /\ v \in seenFloor /\ v \notin seenSettlement => v \in causalA

Inv_SettlementUsesCompleteFrontier ==
    settlementPublished => settlementParents = Parents

Inv_TombstoneNamesExactSource ==
    settlementPublished => tombstoneSource = "A"

Inv_ObservedRejectionIsBuffered ==
    CarrierOwner \in seenSettlement /\ tombstoneSource = "A" =>
        CarrierOwner \in bufferedA

Inv_OnlyCarrierOwnerHasRetryCustody ==
    EnforceCarrierCustody => bufferedA \subseteq {CarrierOwner}

Inv_RetryRequiresLedgerAuthorization ==
    recoveryPublished =>
        /\ recoveryPublisher \in seenSettlement
        /\ recoveryPublisher \in seenTombstone
        /\ recoveryPublisher \in bufferedA
        /\ tombstoneSource = "A"

Inv_OnlyCarrierOwnerRetries ==
    recoveryPublished => recoveryPublisher = CarrierOwner

Inv_SelectedRecoveryIsNotSelfChainSuppressed ==
    recoveryPublished => "A" \in recoveryEffects

Inv_FinalizedEffectNeverRegresses ==
    /\ bFinalized => "B" \in floorEffects
    /\ settlementPublished => "B" \in settlementEffects
    /\ recoveryPublished => "B" \in recoveryEffects
    /\ recoveryFinalized => "B" \in finalEffects

Inv_ConvergedRecoveryState ==
    recoveryFinalized =>
        /\ finalEffects = Effects
        /\ \A v \in Validators : localEffects[v] = finalEffects

RecoveryCompletes == <>recoveryFinalized

=============================================================================
