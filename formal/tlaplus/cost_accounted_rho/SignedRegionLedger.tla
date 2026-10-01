-------------------------- MODULE SignedRegionLedger --------------------------
EXTENDS Naturals

CONSTANT Fault

ASSUME Fault \in {"safe", "wrong_lollipop_payer", "scheduler_local",
                  "double_credit", "stale_root", "replay_drift",
                  "merge_order"}

VARIABLES fired, outerBalance, innerBalance, continuationSeal, scheduler,
          attributedLane, issued, creditCount, creditApplications, committed,
          committedRoot, previousRoot,
          restored, restoredRoot, merged, mergedCost, replayed, replayCost

vars == <<fired, outerBalance, innerBalance, continuationSeal, scheduler,
          attributedLane, issued, creditCount, creditApplications, committed,
          committedRoot, previousRoot,
          restored, restoredRoot, merged, mergedCost, replayed, replayCost>>

Init ==
    /\ fired = FALSE
    /\ outerBalance = 2
    /\ innerBalance = 2
    /\ continuationSeal = "none"
    /\ scheduler \in {"outer", "inner"}
    /\ attributedLane = "none"
    /\ issued = FALSE
    /\ creditCount = 0
    /\ creditApplications = 0
    /\ committed = FALSE
    /\ committedRoot = 0
    /\ previousRoot = 0
    /\ restored = FALSE
    /\ restoredRoot = 0
    /\ merged = {}
    /\ mergedCost = 0
    /\ replayed = FALSE
    /\ replayCost = 0

FireLollipop ==
    /\ ~fired
    /\ fired' = TRUE
    /\ outerBalance' = IF Fault = "wrong_lollipop_payer"
          THEN outerBalance ELSE outerBalance - 1
    /\ innerBalance' = IF Fault = "wrong_lollipop_payer"
          THEN innerBalance - 1 ELSE innerBalance
    /\ continuationSeal' = "inner"
    /\ attributedLane' = IF Fault = "scheduler_local"
          THEN scheduler ELSE "outer"
    /\ UNCHANGED <<scheduler, issued, creditCount, creditApplications,
                  committed, committedRoot, previousRoot,
                  restored, restoredRoot, merged, mergedCost, replayed, replayCost>>

IssueCredit ==
    /\ fired
    /\ ~issued
    /\ issued' = TRUE
    /\ creditCount' = 1
    /\ creditApplications' = 1
    /\ UNCHANGED <<fired, outerBalance, innerBalance, continuationSeal,
                  scheduler, attributedLane, committed, committedRoot, previousRoot,
                  restored, restoredRoot, merged, mergedCost, replayed, replayCost>>

ReplayCredit ==
    /\ Fault = "double_credit"
    /\ issued
    /\ ~committed
    /\ creditApplications = 1
    /\ creditApplications' = 2
    /\ creditCount' = creditCount + 1
    /\ UNCHANGED <<fired, outerBalance, innerBalance, continuationSeal,
                  scheduler, attributedLane, issued, committed, committedRoot,
                  previousRoot, restored, restoredRoot, merged, mergedCost,
                  replayed, replayCost>>

CommitRoot ==
    /\ issued
    /\ ~committed
    /\ committed' = TRUE
    /\ committedRoot' = 16 * outerBalance + 4 * innerBalance + creditCount
    /\ previousRoot' = committedRoot
    /\ UNCHANGED <<fired, outerBalance, innerBalance, continuationSeal,
                  scheduler, attributedLane, issued, creditCount, creditApplications,
                  restored, restoredRoot, merged, mergedCost, replayed, replayCost>>

RestoreRoot ==
    /\ committed
    /\ ~restored
    /\ merged = {1, 2}
    /\ restored' = TRUE
    /\ restoredRoot' = IF Fault = "stale_root"
          THEN previousRoot ELSE committedRoot
    /\ UNCHANGED <<fired, outerBalance, innerBalance, continuationSeal,
                  scheduler, attributedLane, issued, creditCount,
                  creditApplications, committed, committedRoot, previousRoot,
                  merged, mergedCost, replayed, replayCost>>

Merge(e) ==
    /\ committed
    /\ ~replayed
    /\ e \in {1, 2} \ merged
    /\ merged' = merged \cup {e}
    /\ mergedCost' = IF Fault = "merge_order" THEN e ELSE mergedCost + e
    /\ previousRoot' = committedRoot
    /\ committedRoot' = committedRoot + e
    /\ UNCHANGED <<fired, outerBalance, innerBalance, continuationSeal,
                  scheduler, attributedLane, issued, creditCount,
                  creditApplications, committed, restored, restoredRoot,
                  replayed, replayCost>>

Replay ==
    /\ merged = {1, 2}
    /\ ~replayed
    /\ replayed' = TRUE
    /\ replayCost' = IF Fault = "replay_drift"
          THEN mergedCost + 1 ELSE mergedCost
    /\ UNCHANGED <<fired, outerBalance, innerBalance, continuationSeal,
                  scheduler, attributedLane, issued, creditCount,
                  creditApplications, committed, committedRoot, previousRoot,
                  restored, restoredRoot, merged, mergedCost>>

Next ==
    \/ FireLollipop
    \/ IssueCredit
    \/ ReplayCredit
    \/ CommitRoot
    \/ RestoreRoot
    \/ (\E e \in {1, 2} : Merge(e))
    \/ Replay

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ fired \in BOOLEAN
    /\ outerBalance \in 0..2
    /\ innerBalance \in 0..2
    /\ continuationSeal \in {"none", "inner"}
    /\ scheduler \in {"outer", "inner"}
    /\ attributedLane \in {"none", "outer", "inner"}
    /\ issued \in BOOLEAN
    /\ creditCount \in 0..2
    /\ creditApplications \in 0..2
    /\ committed \in BOOLEAN
    /\ committedRoot \in 0..100
    /\ previousRoot \in 0..100
    /\ restored \in BOOLEAN
    /\ restoredRoot \in 0..100
    /\ merged \subseteq {1, 2}
    /\ mergedCost \in 0..30
    /\ replayed \in BOOLEAN
    /\ replayCost \in 0..31

Inv_OuterPaysLollipop ==
    fired => outerBalance = 1 /\ innerBalance = 2

Inv_InnerOwnsContinuation ==
    fired => continuationSeal = "inner"

Inv_SchedulerIndependentAttribution ==
    fired => attributedLane = "outer"

Inv_CreditExactlyOnce ==
    creditCount = IF issued THEN 1 ELSE 0

Inv_FreshRestoredRoot ==
    restored => restoredRoot = committedRoot

MergeSum == (IF 1 \in merged THEN 1 ELSE 0) +
            (IF 2 \in merged THEN 2 ELSE 0)

Inv_MergeOrderIndependent == mergedCost = MergeSum

Inv_RootRepresentsLedger ==
    committed =>
      committedRoot = 16 * outerBalance + 4 * innerBalance + creditCount + MergeSum

Inv_ReplayDeterministic ==
    replayed => replayCost = mergedCost
=============================================================================
