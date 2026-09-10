--------------------- MODULE FinalizationEffectSelection ---------------------
EXTENDS Integers, FiniteSets

CONSTANTS
    \* @type: Int;
    MaxRounds,
    \* @type: Int;
    MaxWorkers,
    \* @type: Int;
    MaxCrashes,
    \* @type: Str;
    Bug

ASSUME /\ MaxRounds > 0
       /\ MaxWorkers > 0
       /\ MaxCrashes >= 0
       /\ Bug \in {"none", "fresh-head", "late-receipt-guard"}

Workers == 1..MaxWorkers
Rounds == 1..MaxRounds
Prefix(revision) == {round \in Rounds : round <= revision}

VARIABLES
    \* @type: Int;
    head,
    \* @type: Int;
    projected,
    \* @type: Int -> Str;
    phase,
    \* @type: Int -> Int;
    projectionTarget,
    \* @type: Int -> Int;
    effectsTarget,
    \* @type: Set(Int);
    applied,
    \* @type: Set(Int);
    receipts,
    \* @type: Int;
    crashes

vars == <<head, projected, phase, projectionTarget, effectsTarget,
          applied, receipts, crashes>>

Init ==
    /\ head = 0
    /\ projected = 0
    /\ phase = [worker \in Workers |-> "idle"]
    /\ projectionTarget = [worker \in Workers |-> 0]
    /\ effectsTarget = [worker \in Workers |-> 0]
    /\ applied = {}
    /\ receipts = {}
    /\ crashes = 0

AppendRound ==
    /\ head < MaxRounds
    /\ head' = head + 1
    /\ UNCHANGED <<projected, phase, projectionTarget, effectsTarget,
                    applied, receipts, crashes>>

BeginProjection(worker) ==
    /\ phase[worker] = "idle"
    /\ projectionTarget' = [projectionTarget EXCEPT ![worker] = head]
    /\ phase' = [phase EXCEPT ![worker] = "project"]
    /\ UNCHANGED <<head, projected, effectsTarget, applied, receipts, crashes>>

ProjectNext(worker) ==
    /\ phase[worker] = "project"
    /\ projected < projectionTarget[worker]
    /\ projected' = projected + 1
    /\ UNCHANGED <<head, phase, projectionTarget, effectsTarget,
                    applied, receipts, crashes>>

FinishProjection(worker) ==
    /\ phase[worker] = "project"
    /\ projectionTarget[worker] <= projected
    /\ phase' = [phase EXCEPT ![worker] = "select"]
    /\ UNCHANGED <<head, projected, projectionTarget, effectsTarget,
                    applied, receipts, crashes>>

SelectEffects(worker) ==
    /\ phase[worker] = "select"
    /\ effectsTarget' = [effectsTarget EXCEPT ![worker] =
                           IF Bug = "fresh-head" THEN head ELSE projected]
    /\ phase' = [phase EXCEPT ![worker] = "effects"]
    /\ UNCHANGED <<head, projected, projectionTarget, applied, receipts, crashes>>

ApplySelected(worker, round) ==
    /\ phase[worker] = "effects"
    /\ round \in Prefix(effectsTarget[worker])
    /\ applied' = applied \cup {round}
    /\ UNCHANGED <<head, projected, phase, projectionTarget, effectsTarget,
                    receipts, crashes>>

ApplyDirect(round) ==
    /\ round <= head
    /\ round <= projected \/ Bug = "late-receipt-guard"
    /\ applied' = applied \cup {round}
    /\ UNCHANGED <<head, projected, phase, projectionTarget, effectsTarget,
                    receipts, crashes>>

Receipt(round) ==
    /\ round \in applied
    /\ round <= projected
    /\ receipts' = receipts \cup {round}
    /\ UNCHANGED <<head, projected, phase, projectionTarget, effectsTarget,
                    applied, crashes>>

FinishPass(worker) ==
    /\ phase[worker] = "effects"
    /\ Prefix(effectsTarget[worker]) \subseteq receipts
    /\ phase' = [phase EXCEPT ![worker] = "idle"]
    /\ UNCHANGED <<head, projected, projectionTarget, effectsTarget,
                    applied, receipts, crashes>>

Crash ==
    /\ crashes < MaxCrashes
    /\ phase' = [worker \in Workers |-> "idle"]
    /\ projectionTarget' = [worker \in Workers |-> 0]
    /\ effectsTarget' = [worker \in Workers |-> 0]
    /\ crashes' = crashes + 1
    /\ UNCHANGED <<head, projected, applied, receipts>>

Next ==
    \/ AppendRound
    \/ \E worker \in Workers : BeginProjection(worker)
    \/ \E worker \in Workers : ProjectNext(worker)
    \/ \E worker \in Workers : FinishProjection(worker)
    \/ \E worker \in Workers : SelectEffects(worker)
    \/ \E worker \in Workers, round \in Rounds : ApplySelected(worker, round)
    \/ \E round \in Rounds : ApplyDirect(round)
    \/ \E round \in Rounds : Receipt(round)
    \/ \E worker \in Workers : FinishPass(worker)
    \/ Crash

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ head \in 0..MaxRounds
    /\ projected \in 0..MaxRounds
    /\ phase \in [Workers -> {"idle", "project", "select", "effects"}]
    /\ projectionTarget \in [Workers -> 0..MaxRounds]
    /\ effectsTarget \in [Workers -> 0..MaxRounds]
    /\ applied \in SUBSET Rounds
    /\ receipts \in SUBSET Rounds
    /\ crashes \in 0..MaxCrashes

Inv_ProjectionBound == projected <= head
Inv_CapturedProjectionBound == \A worker \in Workers : projectionTarget[worker] <= head
Inv_SelectedTargetProjected == \A worker \in Workers : effectsTarget[worker] <= projected
Inv_EffectsAfterProjection == applied \subseteq Prefix(projected)
Inv_ReceiptsAfterEffects == receipts \subseteq applied
Safety == TypeOK /\ Inv_ProjectionBound /\ Inv_SelectedTargetProjected
          /\ Inv_CapturedProjectionBound
          /\ Inv_EffectsAfterProjection /\ Inv_ReceiptsAfterEffects
=============================================================================
