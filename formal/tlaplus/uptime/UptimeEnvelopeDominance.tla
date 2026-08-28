---- MODULE UptimeEnvelopeDominance ----
EXTENDS Integers, Naturals, TLC

CONSTANTS Validators, Quorum, QueueCap, LagCap, LagSlo, MemoryCap,
          AllowBestExtraFailure

VARIABLES eligibleWorst, eligibleBest,
          queueWorst, queueBest,
          lagWorst, lagBest,
          commonWorst, commonBest,
          storageWorst, storageBest,
          memoryWorst, memoryBest

vars == <<eligibleWorst, eligibleBest,
          queueWorst, queueBest,
          lagWorst, lagBest,
          commonWorst, commonBest,
          storageWorst, storageBest,
          memoryWorst, memoryBest>>

BoolLeq(a, b) == (~a) \/ b
Inc(value, cap) == IF value < cap THEN value + 1 ELSE value
Dec(value) == IF value > 0 THEN value - 1 ELSE value

ServiceUp(eligible, queue, lag, commonDown, storageWritable, memory) ==
    /\ eligible >= Quorum
    /\ queue < QueueCap
    /\ lag <= LagSlo
    /\ ~commonDown
    /\ storageWritable
    /\ memory < MemoryCap

Init ==
    /\ eligibleWorst = Validators
    /\ eligibleBest = Validators
    /\ queueWorst = 0
    /\ queueBest = 0
    /\ lagWorst = 0
    /\ lagBest = 0
    /\ commonWorst = FALSE
    /\ commonBest = FALSE
    /\ storageWorst = TRUE
    /\ storageBest = TRUE
    /\ memoryWorst = 0
    /\ memoryBest = 0

SharedValidatorFailure ==
    /\ eligibleWorst > 0
    /\ eligibleBest > 0
    /\ eligibleWorst' = eligibleWorst - 1
    /\ eligibleBest' = eligibleBest - 1
    /\ UNCHANGED <<queueWorst, queueBest, lagWorst, lagBest,
                    commonWorst, commonBest, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

WorstExtraValidatorFailure ==
    /\ eligibleWorst > 0
    /\ eligibleWorst' = eligibleWorst - 1
    /\ UNCHANGED <<eligibleBest, queueWorst, queueBest, lagWorst, lagBest,
                    commonWorst, commonBest, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

BestExtraValidatorFailure ==
    /\ AllowBestExtraFailure
    /\ eligibleBest > 0
    /\ eligibleBest' = eligibleBest - 1
    /\ UNCHANGED <<eligibleWorst, queueWorst, queueBest, lagWorst, lagBest,
                    commonWorst, commonBest, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

SharedValidatorRepair ==
    /\ eligibleWorst < Validators
    /\ eligibleBest < Validators
    /\ eligibleWorst' = eligibleWorst + 1
    /\ eligibleBest' = eligibleBest + 1
    /\ UNCHANGED <<queueWorst, queueBest, lagWorst, lagBest,
                    commonWorst, commonBest, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

BestExtraValidatorRepair ==
    /\ eligibleBest < Validators
    /\ eligibleBest' = eligibleBest + 1
    /\ UNCHANGED <<eligibleWorst, queueWorst, queueBest, lagWorst, lagBest,
                    commonWorst, commonBest, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

SharedArrival ==
    /\ queueWorst' = Inc(queueWorst, QueueCap)
    /\ queueBest' = Inc(queueBest, QueueCap)
    /\ UNCHANGED <<eligibleWorst, eligibleBest, lagWorst, lagBest,
                    commonWorst, commonBest, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

WorstExtraArrival ==
    /\ queueWorst' = Inc(queueWorst, QueueCap)
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueBest, lagWorst, lagBest,
                    commonWorst, commonBest, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

SharedRelief ==
    /\ queueWorst' = Dec(queueWorst)
    /\ queueBest' = Dec(queueBest)
    /\ UNCHANGED <<eligibleWorst, eligibleBest, lagWorst, lagBest,
                    commonWorst, commonBest, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

BestExtraRelief ==
    /\ queueBest' = Dec(queueBest)
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, lagWorst, lagBest,
                    commonWorst, commonBest, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

SharedLagGrowth ==
    /\ lagWorst' = Inc(lagWorst, LagCap)
    /\ lagBest' = Inc(lagBest, LagCap)
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, queueBest,
                    commonWorst, commonBest, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

WorstExtraLagGrowth ==
    /\ lagWorst' = Inc(lagWorst, LagCap)
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, queueBest, lagBest,
                    commonWorst, commonBest, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

SharedLagDrain ==
    /\ lagWorst' = Dec(lagWorst)
    /\ lagBest' = Dec(lagBest)
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, queueBest,
                    commonWorst, commonBest, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

BestExtraLagDrain ==
    /\ lagBest' = Dec(lagBest)
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, queueBest, lagWorst,
                    commonWorst, commonBest, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

SharedCommonFailure ==
    /\ commonWorst' = TRUE
    /\ commonBest' = TRUE
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, queueBest,
                    lagWorst, lagBest, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

WorstExtraCommonFailure ==
    /\ commonWorst' = TRUE
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, queueBest,
                    lagWorst, lagBest, commonBest, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

SharedCommonRepair ==
    /\ commonWorst' = FALSE
    /\ commonBest' = FALSE
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, queueBest,
                    lagWorst, lagBest, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

BestExtraCommonRepair ==
    /\ commonBest' = FALSE
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, queueBest,
                    lagWorst, lagBest, commonWorst, storageWorst, storageBest,
                    memoryWorst, memoryBest>>

SharedStorageFailure ==
    /\ storageWorst' = FALSE
    /\ storageBest' = FALSE
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, queueBest,
                    lagWorst, lagBest, commonWorst, commonBest,
                    memoryWorst, memoryBest>>

WorstExtraStorageFailure ==
    /\ storageWorst' = FALSE
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, queueBest,
                    lagWorst, lagBest, commonWorst, commonBest, storageBest,
                    memoryWorst, memoryBest>>

SharedStorageRepair ==
    /\ storageWorst' = TRUE
    /\ storageBest' = TRUE
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, queueBest,
                    lagWorst, lagBest, commonWorst, commonBest,
                    memoryWorst, memoryBest>>

BestExtraStorageRepair ==
    /\ storageBest' = TRUE
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, queueBest,
                    lagWorst, lagBest, commonWorst, commonBest, storageWorst,
                    memoryWorst, memoryBest>>

SharedMemoryGrowth ==
    /\ memoryWorst' = Inc(memoryWorst, MemoryCap)
    /\ memoryBest' = Inc(memoryBest, MemoryCap)
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, queueBest,
                    lagWorst, lagBest, commonWorst, commonBest,
                    storageWorst, storageBest>>

WorstExtraMemoryGrowth ==
    /\ memoryWorst' = Inc(memoryWorst, MemoryCap)
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, queueBest,
                    lagWorst, lagBest, commonWorst, commonBest,
                    storageWorst, storageBest, memoryBest>>

SharedMemoryReclaim ==
    /\ memoryWorst' = Dec(memoryWorst)
    /\ memoryBest' = Dec(memoryBest)
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, queueBest,
                    lagWorst, lagBest, commonWorst, commonBest,
                    storageWorst, storageBest>>

BestExtraMemoryReclaim ==
    /\ memoryBest' = Dec(memoryBest)
    /\ UNCHANGED <<eligibleWorst, eligibleBest, queueWorst, queueBest,
                    lagWorst, lagBest, commonWorst, commonBest,
                    storageWorst, storageBest, memoryWorst>>

Next ==
    \/ SharedValidatorFailure
    \/ WorstExtraValidatorFailure
    \/ BestExtraValidatorFailure
    \/ SharedValidatorRepair
    \/ BestExtraValidatorRepair
    \/ SharedArrival
    \/ WorstExtraArrival
    \/ SharedRelief
    \/ BestExtraRelief
    \/ SharedLagGrowth
    \/ WorstExtraLagGrowth
    \/ SharedLagDrain
    \/ BestExtraLagDrain
    \/ SharedCommonFailure
    \/ WorstExtraCommonFailure
    \/ SharedCommonRepair
    \/ BestExtraCommonRepair
    \/ SharedStorageFailure
    \/ WorstExtraStorageFailure
    \/ SharedStorageRepair
    \/ BestExtraStorageRepair
    \/ SharedMemoryGrowth
    \/ WorstExtraMemoryGrowth
    \/ SharedMemoryReclaim
    \/ BestExtraMemoryReclaim

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ eligibleWorst \in 0..Validators
    /\ eligibleBest \in 0..Validators
    /\ queueWorst \in 0..QueueCap
    /\ queueBest \in 0..QueueCap
    /\ lagWorst \in 0..LagCap
    /\ lagBest \in 0..LagCap
    /\ commonWorst \in BOOLEAN
    /\ commonBest \in BOOLEAN
    /\ storageWorst \in BOOLEAN
    /\ storageBest \in BOOLEAN
    /\ memoryWorst \in 0..MemoryCap
    /\ memoryBest \in 0..MemoryCap

Dominance ==
    /\ eligibleWorst <= eligibleBest
    /\ queueWorst >= queueBest
    /\ lagWorst >= lagBest
    /\ memoryWorst >= memoryBest
    /\ BoolLeq(commonBest, commonWorst)
    /\ BoolLeq(storageWorst, storageBest)

ServiceOrder ==
    ServiceUp(eligibleWorst, queueWorst, lagWorst, commonWorst, storageWorst, memoryWorst)
    => ServiceUp(eligibleBest, queueBest, lagBest, commonBest, storageBest, memoryBest)

====
