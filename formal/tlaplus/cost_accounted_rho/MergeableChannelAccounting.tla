------------------ MODULE MergeableChannelAccounting ------------------

EXTENDS Naturals, Integers, TLC

VARIABLES
    baseValue,
    endValue,
    diffValue,
    mergeType,
    payloadKind,
    userCost,
    userCostSnapshot,
    settlementCost,
    settlementCostSnapshot,
    slashSystemEffect

vars ==
    <<baseValue, endValue, diffValue, mergeType, payloadKind,
      userCost, userCostSnapshot, settlementCost, settlementCostSnapshot,
      slashSystemEffect>>

ValueSet == 0..3
DiffSet == -3..3
MergeTypeSet == {"IntegerAdd", "BitmaskOr"}
PayloadKindSet == {"numeric", "non_numeric"}

Bits(v) ==
    CASE v = 0 -> {}
      [] v = 1 -> {0}
      [] v = 2 -> {1}
      [] v = 3 -> {0, 1}
      [] OTHER -> {}

Value(bits) ==
    IF bits = {} THEN 0
    ELSE IF bits = {0} THEN 1
    ELSE IF bits = {1} THEN 2
    ELSE 3

BitOr(a, b) == Value(Bits(a) \union Bits(b))

BitDiff(previous, current) == Value(Bits(current) \ Bits(previous))

TypedDiff(previous, current, ty) ==
    IF ty = "IntegerAdd" THEN current - previous
    ELSE BitDiff(previous, current)

TypedMerge(previous, diff, ty) ==
    IF ty = "IntegerAdd" THEN previous + diff
    ELSE BitOr(previous, diff)

Init ==
    /\ baseValue \in ValueSet
    /\ endValue \in ValueSet
    /\ diffValue = 0
    /\ mergeType \in MergeTypeSet
    /\ payloadKind \in PayloadKindSet
    /\ userCost \in ValueSet
    /\ userCostSnapshot = userCost
    /\ settlementCost \in ValueSet
    /\ settlementCostSnapshot = settlementCost
    /\ slashSystemEffect = FALSE

ComputeTypedDiff ==
    /\ payloadKind = "numeric"
    /\ diffValue' = TypedDiff(baseValue, endValue, mergeType)
    /\ UNCHANGED <<baseValue, endValue, mergeType, payloadKind,
        userCost, userCostSnapshot, settlementCost, settlementCostSnapshot,
        slashSystemEffect>>

SkipNonNumericPayload ==
    /\ payloadKind = "non_numeric"
    /\ diffValue' = 0
    /\ UNCHANGED <<baseValue, endValue, mergeType, payloadKind,
        userCost, userCostSnapshot, settlementCost, settlementCostSnapshot,
        slashSystemEffect>>

ApplySlashSystemEffect ==
    /\ slashSystemEffect' = TRUE
    /\ UNCHANGED <<baseValue, endValue, diffValue, mergeType, payloadKind,
        userCost, userCostSnapshot, settlementCost, settlementCostSnapshot>>

TerminalStutter ==
    UNCHANGED vars

Next ==
    ComputeTypedDiff \/ SkipNonNumericPayload \/
    ApplySlashSystemEffect \/ TerminalStutter

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ baseValue \in ValueSet
    /\ endValue \in ValueSet
    /\ diffValue \in DiffSet
    /\ mergeType \in MergeTypeSet
    /\ payloadKind \in PayloadKindSet
    /\ userCost \in ValueSet
    /\ userCostSnapshot \in ValueSet
    /\ settlementCost \in ValueSet
    /\ settlementCostSnapshot \in ValueSet
    /\ slashSystemEffect \in BOOLEAN

BitmaskDiffMergeRoundTrip ==
    payloadKind = "numeric" /\ mergeType = "BitmaskOr" =>
        TypedMerge(baseValue, TypedDiff(baseValue, endValue, mergeType), mergeType)
        = BitOr(baseValue, endValue)

IntegerAddDiffMergeRoundTrip ==
    payloadKind = "numeric" /\ mergeType = "IntegerAdd" =>
        TypedMerge(baseValue, TypedDiff(baseValue, endValue, mergeType), mergeType)
        = endValue

BitmaskMergeDoesNotDropBits ==
    payloadKind = "numeric" /\ mergeType = "BitmaskOr" =>
        /\ Bits(baseValue) \subseteq
            Bits(TypedMerge(baseValue, TypedDiff(baseValue, endValue, mergeType), mergeType))
        /\ Bits(endValue) \subseteq
            Bits(TypedMerge(baseValue, TypedDiff(baseValue, endValue, mergeType), mergeType))

NonNumericPayloadHasNoNumericDiff ==
    payloadKind = "non_numeric" => diffValue = 0

MergeTypeDomainPreserved ==
    mergeType \in MergeTypeSet

MergeableAccountingPreservesUserCost ==
    userCost = userCostSnapshot

MergeableAccountingPreservesSettlementCost ==
    settlementCost = settlementCostSnapshot

SlashSystemEffectPreservesCostBoundary ==
    slashSystemEffect =>
        /\ userCost = userCostSnapshot
        /\ settlementCost = settlementCostSnapshot

=============================================================================
