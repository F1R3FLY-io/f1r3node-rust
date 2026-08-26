-------------------------- MODULE FinalizationRecovery --------------------------
EXTENDS FiniteSets, Integers, TLC

CONSTANT
    \* @type: Int;
    MaxRounds,
    \* @type: Int;
    MaxCrashes,
    \* @type: Bool;
    UnsafeSkipProjection,
    \* @type: Bool;
    UnsafeEffectBeforeProjection,
    \* @type: Bool;
    UnsafeSkipEffectsCursor

ASSUME /\ MaxRounds \in Nat \ {0}
       /\ MaxCrashes \in Nat
       /\ UnsafeSkipProjection \in BOOLEAN
       /\ UnsafeEffectBeforeProjection \in BOOLEAN
       /\ UnsafeSkipEffectsCursor \in BOOLEAN

Rounds == 1..MaxRounds

VARIABLES
    \* @type: Bool;
    running,
    \* @type: Int;
    crashes,
    \* @type: Set(Int);
    manifests,
    \* @type: Set(Int);
    records,
    \* @type: Int;
    durableHead,
    \* @type: Set(Int);
    projected,
    \* @type: Int;
    projectionCursor,
    \* @type: Set(Int);
    effects,
    \* @type: Set(Int);
    receipts,
    \* @type: Set(Int);
    effectsComplete,
    \* @type: Int;
    effectsCursor,
    \* @type: Int;
    effectsCompactionCursor

vars == <<running, crashes, manifests, records, durableHead, projected,
          projectionCursor, effects, receipts, effectsComplete, effectsCursor,
          effectsCompactionCursor>>

\* @type: (Int) => Set(Int);
Prefix(revision) == {round \in Rounds : round <= revision}

Init ==
    /\ running = TRUE
    /\ crashes = 0
    /\ manifests = {}
    /\ records = {}
    /\ durableHead = 0
    /\ projected = {}
    /\ projectionCursor = 0
    /\ effects = {}
    /\ receipts = {}
    /\ effectsComplete = {}
    /\ effectsCursor = 0
    /\ effectsCompactionCursor = 0

Prepare(round) ==
    /\ running
    /\ round \in Rounds
    /\ round <= durableHead + 1
    /\ manifests' = manifests \cup {round}
    /\ UNCHANGED <<running, crashes, records, durableHead, projected,
                    projectionCursor, effects, receipts, effectsComplete,
                    effectsCursor, effectsCompactionCursor>>

CommitRound(round) ==
    /\ running
    /\ round = durableHead + 1
    /\ round \in manifests
    /\ records' = records \cup {round}
    /\ durableHead' = round
    /\ UNCHANGED <<running, crashes, manifests, projected, projectionCursor,
                    effects, receipts, effectsComplete, effectsCursor,
                    effectsCompactionCursor>>

ProjectNext ==
    /\ running
    /\ projectionCursor < durableHead
    /\ projectionCursor + 1 \in records
    /\ projectionCursor' = projectionCursor + 1
    /\ projected' = projected \cup {projectionCursor'}
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    effects, receipts, effectsComplete, effectsCursor,
                    effectsCompactionCursor>>

ApplyEffect(round) ==
    /\ running
    /\ round \in projected
    /\ effects' = effects \cup {round}
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    projected, projectionCursor, receipts, effectsComplete,
                    effectsCursor, effectsCompactionCursor>>

RecordReceipt(round) ==
    /\ running
    /\ round \in effects
    /\ receipts' = receipts \cup {round}
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    projected, projectionCursor, effects, effectsComplete,
                    effectsCursor, effectsCompactionCursor>>

CompleteEffects(round) ==
    /\ running
    /\ round \in receipts
    /\ effectsComplete' = effectsComplete \cup {round}
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    projected, projectionCursor, effects, receipts,
                    effectsCursor, effectsCompactionCursor>>

AdvanceEffectsCursor ==
    /\ running
    /\ effectsCursor < durableHead
    /\ effectsCursor + 1 \in effectsComplete
    /\ effectsCursor' = effectsCursor + 1
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    projected, projectionCursor, effects, receipts,
                    effectsComplete, effectsCompactionCursor>>

CompactEffects ==
    /\ running
    /\ effectsCompactionCursor < effectsCursor
    /\ effectsCompactionCursor' = effectsCursor
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    projected, projectionCursor, effects, receipts,
                    effectsComplete, effectsCursor>>

Crash ==
    /\ running
    /\ crashes < MaxCrashes
    /\ running' = FALSE
    /\ crashes' = crashes + 1
    /\ UNCHANGED <<manifests, records, durableHead, projected,
                    projectionCursor, effects, receipts, effectsComplete,
                    effectsCursor, effectsCompactionCursor>>

Restart ==
    /\ ~running
    /\ running' = TRUE
    /\ UNCHANGED <<crashes, manifests, records, durableHead, projected,
                    projectionCursor, effects, receipts, effectsComplete,
                    effectsCursor, effectsCompactionCursor>>

UnsafeProjectGap(round) ==
    /\ UnsafeSkipProjection
    /\ running
    /\ round \in records
    /\ round > projectionCursor + 1
    /\ projectionCursor' = round
    /\ projected' = projected \cup {round}
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    effects, receipts, effectsComplete, effectsCursor,
                    effectsCompactionCursor>>

UnsafeEarlyEffect(round) ==
    /\ UnsafeEffectBeforeProjection
    /\ running
    /\ round \in records \ projected
    /\ effects' = effects \cup {round}
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    projected, projectionCursor, receipts, effectsComplete,
                    effectsCursor, effectsCompactionCursor>>

UnsafeAdvanceEffectsGap(round) ==
    /\ UnsafeSkipEffectsCursor
    /\ running
    /\ round \in effectsComplete
    /\ round > effectsCursor + 1
    /\ effectsCursor' = round
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    projected, projectionCursor, effects, receipts,
                    effectsComplete, effectsCompactionCursor>>

Next ==
    \/ \E round \in Rounds : Prepare(round)
    \/ \E round \in Rounds : CommitRound(round)
    \/ ProjectNext
    \/ \E round \in Rounds : ApplyEffect(round)
    \/ \E round \in Rounds : RecordReceipt(round)
    \/ \E round \in Rounds : CompleteEffects(round)
    \/ AdvanceEffectsCursor
    \/ CompactEffects
    \/ Crash
    \/ Restart
    \/ \E round \in Rounds : UnsafeProjectGap(round)
    \/ \E round \in Rounds : UnsafeEarlyEffect(round)
    \/ \E round \in Rounds : UnsafeAdvanceEffectsGap(round)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ running \in BOOLEAN
    /\ crashes \in 0..MaxCrashes
    /\ manifests \in SUBSET Rounds
    /\ records \in SUBSET Rounds
    /\ durableHead \in 0..MaxRounds
    /\ projected \in SUBSET Rounds
    /\ projectionCursor \in 0..MaxRounds
    /\ effects \in SUBSET Rounds
    /\ receipts \in SUBSET Rounds
    /\ effectsComplete \in SUBSET Rounds
    /\ effectsCursor \in 0..MaxRounds
    /\ effectsCompactionCursor \in 0..MaxRounds

Inv_RecordPrefix == records = Prefix(durableHead)
Inv_ProjectionPrefix == projected = Prefix(projectionCursor)
Inv_ProjectionBounded == projectionCursor <= durableHead
Inv_EffectsAfterProjection == effects \subseteq projected
Inv_ReceiptsAfterEffects == receipts \subseteq effects
Inv_CompletionAfterReceipts == effectsComplete \subseteq receipts
Inv_EffectsCursorPrefix == Prefix(effectsCursor) \subseteq effectsComplete
Inv_EffectsCursorBounded == effectsCursor <= projectionCursor
Inv_EffectsCompactionBounded == effectsCompactionCursor <= effectsCursor

Safety ==
    /\ TypeOK
    /\ Inv_RecordPrefix
    /\ Inv_ProjectionPrefix
    /\ Inv_ProjectionBounded
    /\ Inv_EffectsAfterProjection
    /\ Inv_ReceiptsAfterEffects
    /\ Inv_CompletionAfterReceipts
    /\ Inv_EffectsCursorPrefix
    /\ Inv_EffectsCursorBounded
    /\ Inv_EffectsCompactionBounded

=============================================================================
