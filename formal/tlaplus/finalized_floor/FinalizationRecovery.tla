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
    UnsafeSkipEffectsCursor,
    \* @type: Bool;
    UnsafeEarlyCompaction,
    \* @type: Bool;
    UnsafeCompactionCursor

ASSUME /\ MaxRounds \in Nat \ {0}
       /\ MaxCrashes \in Nat
       /\ UnsafeSkipProjection \in BOOLEAN
       /\ UnsafeEffectBeforeProjection \in BOOLEAN
       /\ UnsafeSkipEffectsCursor \in BOOLEAN
       /\ UnsafeEarlyCompaction \in BOOLEAN
       /\ UnsafeCompactionCursor \in BOOLEAN

Rounds == 1..MaxRounds
ReceiptSlots == 1..2
\* @type: (Int) => Set(<<Int, Int>>);
RoundKeys(round) == {<<round, slot>> : slot \in ReceiptSlots}

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
    effectsCompactionCursor,
    \* @type: Set(<<Int, Int>>);
    storedReceipts,
    \* @type: Set(Int);
    completedHistory

vars == <<running, crashes, manifests, records, durableHead, projected,
          projectionCursor, effects, receipts, effectsComplete, effectsCursor,
          effectsCompactionCursor, storedReceipts, completedHistory>>

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
    /\ storedReceipts = {}
    /\ completedHistory = {}

Prepare(round) ==
    /\ running
    /\ round \in Rounds
    /\ round <= durableHead + 1
    /\ manifests' = manifests \cup {round}
    /\ UNCHANGED <<running, crashes, records, durableHead, projected,
                    projectionCursor, effects, receipts, effectsComplete,
                    effectsCursor, effectsCompactionCursor, storedReceipts, completedHistory>>

CommitRound(round) ==
    /\ running
    /\ round = durableHead + 1
    /\ round \in manifests
    /\ records' = records \cup {round}
    /\ durableHead' = round
    /\ UNCHANGED <<running, crashes, manifests, projected, projectionCursor,
                    effects, receipts, effectsComplete, effectsCursor,
                    effectsCompactionCursor, storedReceipts, completedHistory>>

ProjectNext ==
    /\ running
    /\ projectionCursor < durableHead
    /\ projectionCursor + 1 \in records
    /\ projectionCursor' = projectionCursor + 1
    /\ projected' = projected \cup {projectionCursor'}
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    effects, receipts, effectsComplete, effectsCursor,
                    effectsCompactionCursor, storedReceipts, completedHistory>>

ApplyEffect(round) ==
    /\ running
    /\ round \in projected
    /\ effects' = effects \cup {round}
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    projected, projectionCursor, receipts, effectsComplete,
                    effectsCursor, effectsCompactionCursor, storedReceipts, completedHistory>>

RecordReceipt(round) ==
    /\ running
    /\ round \in effects
    /\ round > effectsCursor
    /\ receipts' = receipts \cup {round}
    /\ storedReceipts' = storedReceipts \cup RoundKeys(round)
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    projected, projectionCursor, effects, effectsComplete,
                    effectsCursor, effectsCompactionCursor, completedHistory>>

CompleteEffects(round) ==
    /\ running
    /\ round \in receipts
    /\ round > effectsCursor
    /\ RoundKeys(round) \subseteq storedReceipts
    /\ effectsComplete' = effectsComplete \cup {round}
    /\ completedHistory' = completedHistory \cup {round}
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    projected, projectionCursor, effects, receipts,
                    effectsCursor, effectsCompactionCursor, storedReceipts>>

AdvanceEffectsCursor ==
    /\ running
    /\ effectsCursor < durableHead
    /\ effectsCursor + 1 \in effectsComplete
    /\ effectsCursor' = effectsCursor + 1
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    projected, projectionCursor, effects, receipts,
                    effectsComplete, effectsCompactionCursor, storedReceipts, completedHistory>>

DeleteReceiptPage(round, page) ==
    /\ running
    /\ round = effectsCompactionCursor + 1
    /\ round <= effectsCursor \/ UnsafeEarlyCompaction
    /\ page \subseteq (RoundKeys(round) \cap storedReceipts)
    /\ Cardinality(page) \in 1..2
    /\ storedReceipts' = storedReceipts \ page
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    projected, projectionCursor, effects, receipts, effectsComplete,
                    effectsCursor, effectsCompactionCursor, completedHistory>>

CompactEffects ==
    /\ running
    /\ effectsCompactionCursor < effectsCursor
    /\ RoundKeys(effectsCompactionCursor + 1) \cap storedReceipts = {}
        \/ UnsafeCompactionCursor
    /\ effectsComplete' = effectsComplete \ {effectsCompactionCursor + 1}
    /\ effectsCompactionCursor' = effectsCompactionCursor + 1
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    projected, projectionCursor, effects, receipts,
                    effectsCursor, storedReceipts, completedHistory>>

Crash ==
    /\ running
    /\ crashes < MaxCrashes
    /\ running' = FALSE
    /\ crashes' = crashes + 1
    /\ UNCHANGED <<manifests, records, durableHead, projected,
                    projectionCursor, effects, receipts, effectsComplete,
                    effectsCursor, effectsCompactionCursor, storedReceipts, completedHistory>>

Restart ==
    /\ ~running
    /\ running' = TRUE
    /\ UNCHANGED <<crashes, manifests, records, durableHead, projected,
                    projectionCursor, effects, receipts, effectsComplete,
                    effectsCursor, effectsCompactionCursor, storedReceipts, completedHistory>>

UnsafeProjectGap(round) ==
    /\ UnsafeSkipProjection
    /\ running
    /\ round \in records
    /\ round > projectionCursor + 1
    /\ projectionCursor' = round
    /\ projected' = projected \cup {round}
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    effects, receipts, effectsComplete, effectsCursor,
                    effectsCompactionCursor, storedReceipts, completedHistory>>

UnsafeEarlyEffect(round) ==
    /\ UnsafeEffectBeforeProjection
    /\ running
    /\ round \in records \ projected
    /\ effects' = effects \cup {round}
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    projected, projectionCursor, receipts, effectsComplete,
                    effectsCursor, effectsCompactionCursor, storedReceipts, completedHistory>>

UnsafeAdvanceEffectsGap(round) ==
    /\ UnsafeSkipEffectsCursor
    /\ running
    /\ round \in effectsComplete
    /\ round > effectsCursor + 1
    /\ effectsCursor' = round
    /\ UNCHANGED <<running, crashes, manifests, records, durableHead,
                    projected, projectionCursor, effects, receipts,
                    effectsComplete, effectsCompactionCursor, storedReceipts, completedHistory>>

Next ==
    \/ \E round \in Rounds : Prepare(round)
    \/ \E round \in Rounds : CommitRound(round)
    \/ ProjectNext
    \/ \E round \in Rounds : ApplyEffect(round)
    \/ \E round \in Rounds : RecordReceipt(round)
    \/ \E round \in Rounds : CompleteEffects(round)
    \/ AdvanceEffectsCursor
    \/ \E round \in Rounds, page \in SUBSET (Rounds \X ReceiptSlots) :
           DeleteReceiptPage(round, page)
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
    /\ storedReceipts \in SUBSET (Rounds \X ReceiptSlots)
    /\ completedHistory \in SUBSET Rounds

Inv_RecordPrefix == records = Prefix(durableHead)
Inv_ProjectionPrefix == projected = Prefix(projectionCursor)
Inv_ProjectionBounded == projectionCursor <= durableHead
Inv_EffectsAfterProjection == effects \subseteq projected
Inv_ReceiptsAfterEffects == receipts \subseteq effects
Inv_CompletionAfterReceipts == effectsComplete \subseteq receipts
Inv_EffectsCursorPrefix == Prefix(effectsCursor) \subseteq completedHistory
Inv_EffectsCursorBounded == effectsCursor <= projectionCursor
Inv_EffectsCompactionBounded == effectsCompactionCursor <= effectsCursor

Inv_StoredReceiptsAreAuthentic == storedReceipts \subseteq (receipts \X ReceiptSlots)
Inv_CompletionHistory == effectsComplete \subseteq completedHistory /\ completedHistory \subseteq receipts
Inv_RequiredReceiptsPresent == \A round \in receipts :
    round > effectsCursor => RoundKeys(round) \subseteq storedReceipts
Inv_CompactionCursorClean == storedReceipts \cap (Prefix(effectsCompactionCursor) \X ReceiptSlots) = {}

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
    /\ Inv_StoredReceiptsAreAuthentic
    /\ Inv_CompletionHistory
    /\ Inv_RequiredReceiptsPresent
    /\ Inv_CompactionCursorClean

=============================================================================
