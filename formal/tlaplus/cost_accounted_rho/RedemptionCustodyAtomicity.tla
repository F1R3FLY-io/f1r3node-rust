------------------ MODULE RedemptionCustodyAtomicity ------------------
EXTENDS Integers

CONSTANT Defect

ASSUME Defect \in {
    "None",
    "CheckpointAfterStake",
    "CheckpointAfterFuel",
    "PublishReceiptBeforePoS",
    "LostReceiptOnRetry"
}

Outcomes == {"Vindicated", "Guilty", "Burned"}
PartialPenalty == 3
InitialStake == 10
InitialFuel == 8

VARIABLES
    phase,
    outcome,
    stakeSource,
    stakePenalty,
    stakeBurned,
    fuelSource,
    fuelPenalty,
    fuelBurned,
    posLifecycle,
    receipt,
    workStakeSource,
    workStakePenalty,
    workStakeBurned,
    workFuelSource,
    workFuelPenalty,
    workFuelBurned,
    workPosLifecycle

vars == <<
    phase,
    outcome,
    stakeSource,
    stakePenalty,
    stakeBurned,
    fuelSource,
    fuelPenalty,
    fuelBurned,
    posLifecycle,
    receipt,
    workStakeSource,
    workStakePenalty,
    workStakeBurned,
    workFuelSource,
    workFuelPenalty,
    workFuelBurned,
    workPosLifecycle
>>

Init ==
    /\ phase = "Idle"
    /\ outcome = "None"
    /\ stakeSource = InitialStake
    /\ stakePenalty = 0
    /\ stakeBurned = 0
    /\ fuelSource = InitialFuel
    /\ fuelPenalty = 0
    /\ fuelBurned = 0
    /\ posLifecycle = "Quarantined"
    /\ receipt = "None"
    /\ workStakeSource = InitialStake
    /\ workStakePenalty = 0
    /\ workStakeBurned = 0
    /\ workFuelSource = InitialFuel
    /\ workFuelPenalty = 0
    /\ workFuelBurned = 0
    /\ workPosLifecycle = "Quarantined"

Begin(o) ==
    /\ phase = "Idle"
    /\ o \in Outcomes
    /\ phase' = "Begun"
    /\ outcome' = o
    /\ workStakeSource' = stakeSource
    /\ workStakePenalty' = stakePenalty
    /\ workStakeBurned' = stakeBurned
    /\ workFuelSource' = fuelSource
    /\ workFuelPenalty' = fuelPenalty
    /\ workFuelBurned' = fuelBurned
    /\ workPosLifecycle' = posLifecycle
    /\ UNCHANGED <<stakeSource, stakePenalty, stakeBurned,
                    fuelSource, fuelPenalty, fuelBurned,
                    posLifecycle, receipt>>

StageStake ==
    /\ phase = "Begun"
    /\ phase' = "StakeStaged"
    /\ CASE outcome = "Vindicated" ->
                /\ UNCHANGED <<workStakeSource, workStakePenalty, workStakeBurned>>
            [] outcome = "Guilty" ->
                /\ workStakeSource' = workStakeSource - PartialPenalty
                /\ workStakePenalty' = workStakePenalty + PartialPenalty
                /\ UNCHANGED workStakeBurned
            [] outcome = "Burned" ->
                /\ workStakeSource' = 0
                /\ UNCHANGED workStakePenalty
                /\ workStakeBurned' = workStakeBurned + workStakeSource
    /\ UNCHANGED <<outcome, stakeSource, stakePenalty, stakeBurned,
                    fuelSource, fuelPenalty, fuelBurned, posLifecycle, receipt,
                    workFuelSource, workFuelPenalty, workFuelBurned,
                    workPosLifecycle>>

StageFuel ==
    /\ phase = "StakeStaged"
    /\ phase' = "FuelStaged"
    /\ CASE outcome = "Vindicated" ->
                /\ UNCHANGED <<workFuelSource, workFuelPenalty, workFuelBurned>>
            [] outcome = "Guilty" ->
                /\ workFuelSource' = workFuelSource - PartialPenalty
                /\ workFuelPenalty' = workFuelPenalty + PartialPenalty
                /\ UNCHANGED workFuelBurned
            [] outcome = "Burned" ->
                /\ workFuelSource' = 0
                /\ UNCHANGED workFuelPenalty
                /\ workFuelBurned' = workFuelBurned + workFuelSource
    /\ UNCHANGED <<outcome, stakeSource, stakePenalty, stakeBurned,
                    fuelSource, fuelPenalty, fuelBurned, posLifecycle, receipt,
                    workStakeSource, workStakePenalty, workStakeBurned,
                    workPosLifecycle>>

StagePoS ==
    /\ phase = "FuelStaged"
    /\ phase' = "PoSStaged"
    /\ workPosLifecycle' = IF outcome = "Burned" THEN "Burned" ELSE "Restored"
    /\ UNCHANGED <<outcome, stakeSource, stakePenalty, stakeBurned,
                    fuelSource, fuelPenalty, fuelBurned, posLifecycle, receipt,
                    workStakeSource, workStakePenalty, workStakeBurned,
                    workFuelSource, workFuelPenalty, workFuelBurned>>

Accept ==
    /\ phase = "PoSStaged"
    /\ phase' = "Accepted"
    /\ stakeSource' = workStakeSource
    /\ stakePenalty' = workStakePenalty
    /\ stakeBurned' = workStakeBurned
    /\ fuelSource' = workFuelSource
    /\ fuelPenalty' = workFuelPenalty
    /\ fuelBurned' = workFuelBurned
    /\ posLifecycle' = workPosLifecycle
    /\ receipt' = outcome
    /\ UNCHANGED <<outcome, workStakeSource, workStakePenalty,
                    workStakeBurned, workFuelSource, workFuelPenalty,
                    workFuelBurned, workPosLifecycle>>

StakeWasStaged == phase \in {"StakeStaged", "FuelStaged", "PoSStaged"}
FuelWasStaged == phase \in {"FuelStaged", "PoSStaged"}

Abort(nextPhase) ==
    /\ phase \in {"Begun", "StakeStaged", "FuelStaged", "PoSStaged"}
    /\ phase' = nextPhase
    /\ stakeSource' =
        IF Defect = "CheckpointAfterStake" /\ StakeWasStaged
        THEN workStakeSource
        ELSE stakeSource
    /\ stakePenalty' =
        IF Defect = "CheckpointAfterStake" /\ StakeWasStaged
        THEN workStakePenalty
        ELSE stakePenalty
    /\ stakeBurned' =
        IF Defect = "CheckpointAfterStake" /\ StakeWasStaged
        THEN workStakeBurned
        ELSE stakeBurned
    /\ fuelSource' =
        IF Defect = "CheckpointAfterFuel" /\ FuelWasStaged
        THEN workFuelSource
        ELSE fuelSource
    /\ fuelPenalty' =
        IF Defect = "CheckpointAfterFuel" /\ FuelWasStaged
        THEN workFuelPenalty
        ELSE fuelPenalty
    /\ fuelBurned' =
        IF Defect = "CheckpointAfterFuel" /\ FuelWasStaged
        THEN workFuelBurned
        ELSE fuelBurned
    /\ receipt' =
        IF Defect = "PublishReceiptBeforePoS" /\ phase = "FuelStaged"
        THEN outcome
        ELSE receipt
    /\ UNCHANGED <<outcome, posLifecycle, workStakeSource,
                    workStakePenalty, workStakeBurned, workFuelSource,
                    workFuelPenalty, workFuelBurned, workPosLifecycle>>

RetryAccepted ==
    /\ phase = "Accepted"
    /\ phase' = "Retried"
    /\ IF Defect = "LostReceiptOnRetry" /\ outcome = "Guilty"
       THEN /\ stakeSource' = stakeSource - PartialPenalty
            /\ stakePenalty' = stakePenalty + PartialPenalty
            /\ fuelSource' = fuelSource - PartialPenalty
            /\ fuelPenalty' = fuelPenalty + PartialPenalty
       ELSE UNCHANGED <<stakeSource, stakePenalty, fuelSource, fuelPenalty>>
    /\ UNCHANGED <<outcome, stakeBurned, fuelBurned, posLifecycle, receipt,
                    workStakeSource, workStakePenalty, workStakeBurned,
                    workFuelSource, workFuelPenalty, workFuelBurned,
                    workPosLifecycle>>

TerminalStutter ==
    /\ phase \in {"Rejected", "Crashed", "Retried"}
    /\ UNCHANGED vars

Next ==
    \/ \E o \in Outcomes : Begin(o)
    \/ StageStake
    \/ StageFuel
    \/ StagePoS
    \/ Accept
    \/ Abort("Rejected")
    \/ Abort("Crashed")
    \/ RetryAccepted
    \/ TerminalStutter

TypeOK ==
    /\ phase \in {"Idle", "Begun", "StakeStaged", "FuelStaged",
                    "PoSStaged", "Accepted", "Rejected", "Crashed", "Retried"}
    /\ outcome \in Outcomes \cup {"None"}
    /\ receipt \in Outcomes \cup {"None"}
    /\ posLifecycle \in {"Quarantined", "Restored", "Burned"}
    /\ workPosLifecycle \in {"Quarantined", "Restored", "Burned"}
    /\ stakeSource \in 0..InitialStake
    /\ stakePenalty \in 0..InitialStake
    /\ stakeBurned \in 0..InitialStake
    /\ fuelSource \in 0..InitialFuel
    /\ fuelPenalty \in 0..InitialFuel
    /\ fuelBurned \in 0..InitialFuel

StakeConserved == stakeSource + stakePenalty + stakeBurned = InitialStake
FuelConserved == fuelSource + fuelPenalty + fuelBurned = InitialFuel

AbortedExecutionPublishesNothing ==
    phase \in {"Rejected", "Crashed"} =>
        /\ stakeSource = InitialStake
        /\ stakePenalty = 0
        /\ stakeBurned = 0
        /\ fuelSource = InitialFuel
        /\ fuelPenalty = 0
        /\ fuelBurned = 0
        /\ posLifecycle = "Quarantined"
        /\ receipt = "None"

ReceiptCommitsWholeResolution ==
    receipt # "None" =>
        /\ phase \in {"Accepted", "Retried"}
        /\ receipt = outcome
        /\ posLifecycle = IF outcome = "Burned" THEN "Burned" ELSE "Restored"

GuiltyPenaltyAppliedExactlyOnce ==
    outcome = "Guilty" /\ phase \in {"Accepted", "Retried"} =>
        /\ stakePenalty = PartialPenalty
        /\ fuelPenalty = PartialPenalty

=============================================================================
