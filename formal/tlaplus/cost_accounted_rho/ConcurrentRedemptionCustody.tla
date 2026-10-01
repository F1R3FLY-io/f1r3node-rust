------------------ MODULE ConcurrentRedemptionCustody ------------------
EXTENDS Integers, FiniteSets

CONSTANT
  \* @type: Str;
  Defect

ASSUME Defect \in {
  "None",
  "NoTargetLock",
  "IgnoreGeneration",
  "AllowFullGuilty",
  "RestoreBonded",
  "CheckpointStake",
  "CheckpointFuel",
  "LostReceipt",
  "OverwriteConflict"
}

\* @type: Set(Str);
Validators == {"A", "B"}
\* @type: Set(Str);
Workers == {"A1", "A2", "B1", "AStale"}
\* @type: Set(Str);
Outcomes == {"Vindicated", "Guilty", "Burned"}
\* @type: Set(Str);
RestorablePhases == {"Bonded", "PendingWithdraw", "Withdrawing"}
\* @type: Set(Str);
Phases == RestorablePhases \cup {"Quarantined", "Withdrawn", "Burned"}
\* @type: Set(Str);
WorkerPhases == {
  "Idle", "Begun", "StakeStaged", "FuelStaged", "PoSStaged",
  "Accepted", "Rejected", "Crashed", "Retried", "ConflictRejected",
  "StaleRejected", "InvalidRejected"
}
\* @type: Str;
NoWorker == "NoWorker"
\* @type: Str;
NoOutcome == "NoOutcome"
\* @type: Str;
NoOrigin == "NoOrigin"
\* @type: Str;
NoPhase == "NoPhase"
\* @type: Int;
MaxGeneration == 2
\* @type: Int;
BondAmount == 2
\* @type: Set(Int);
PenaltyRange == 0..5
\* @type: Set(Int);
ValueRange == 0..12

\* @type: Str -> Str;
InitialPhase ==
  [v \in Validators |-> IF v = "A" THEN "PendingWithdraw" ELSE "Withdrawing"]
\* @type: Str -> Int;
InitialGeneration == [v \in Validators |-> IF v = "A" THEN 1 ELSE 0]
\* @type: Str -> Int;
InitialBond == [v \in Validators |-> IF v = "A" THEN 4 ELSE 3]
\* @type: Str -> Int;
InitialReward == [v \in Validators |-> IF v = "A" THEN 1 ELSE 2]
\* @type: Str -> Int;
InitialWallet == [v \in Validators |-> 0]
\* @type: Str -> Int;
InitialFuel == [v \in Validators |-> IF v = "A" THEN 3 ELSE 2]
\* @type: Str -> Str;
Target == [w \in Workers |-> IF w = "B1" THEN "B" ELSE "A"]
\* @type: Str -> Int;
TargetGeneration ==
  [w \in Workers |-> IF w = "B1" \/ w = "AStale" THEN 0 ELSE 1]

\* @typeAlias: receipt = {
\*   validator: Str,
\*   targetGeneration: Int,
\*   acceptedGeneration: Int,
\*   request: Str,
\*   outcome: Str,
\*   penalty: Int
\* };
module_typedefs == TRUE

VARIABLES
  \* @type: Str -> Str;
  phase,
  \* @type: Str -> Str;
  quarantineOrigin,
  \* @type: Str -> Int;
  generation,
  \* @type: Str -> Int;
  bond,
  \* @type: Str -> Int;
  reward,
  \* @type: Str -> Int;
  wallet,
  \* @type: Str -> Int;
  cooperativeStake,
  \* @type: Str -> Int;
  burnedStake,
  \* @type: Str -> Int;
  fuel,
  \* @type: Str -> Int;
  cooperativeFuel,
  \* @type: Str -> Int;
  burnedFuel,
  \* @type: Str -> Int;
  stateVersion,
  \* @type: Str -> Str;
  lockOwner,
  \* @type: Set(Str);
  mintingHalted,
  \* @type: Str -> Str;
  workerPhase,
  \* @type: Str -> Str;
  workerOutcome,
  \* @type: Str -> Int;
  workerPenalty,
  \* @type: Str -> Int;
  workerVersion,
  \* @type: Str -> Str;
  workOrigin,
  \* @type: Str -> Str;
  workPhase,
  \* @type: Str -> Int;
  workBond,
  \* @type: Str -> Int;
  workReward,
  \* @type: Str -> Int;
  workCooperativeStake,
  \* @type: Str -> Int;
  workBurnedStake,
  \* @type: Str -> Int;
  workFuel,
  \* @type: Str -> Int;
  workCooperativeFuel,
  \* @type: Str -> Int;
  workBurnedFuel,
  \* @type: Set($receipt);
  receipts,
  \* @type: Bool;
  abortPublished,
  \* @type: Bool;
  retryMutated,
  \* @type: Bool;
  unauthorizedAccepted,
  \* @type: Bool;
  fullGuiltyAccepted,
  \* @type: Bool;
  conflictMutated,
  \* @type: Str;
  lastRestoredOrigin,
  \* @type: Str;
  lastRestoredPhase

vars == <<
  phase, quarantineOrigin, generation, bond, reward, wallet,
  cooperativeStake, burnedStake, fuel, cooperativeFuel, burnedFuel,
  stateVersion, lockOwner, mintingHalted,
  workerPhase, workerOutcome, workerPenalty, workerVersion,
  workOrigin, workPhase, workBond, workReward, workCooperativeStake,
  workBurnedStake, workFuel, workCooperativeFuel, workBurnedFuel,
  receipts, abortPublished, retryMutated, unauthorizedAccepted,
  fullGuiltyAccepted, conflictMutated, lastRestoredOrigin,
  lastRestoredPhase
>>

\* @type: (Str) => $receipt;
Receipt(w) == [
  validator |-> Target[w],
  targetGeneration |-> TargetGeneration[w],
  acceptedGeneration |-> generation[Target[w]],
  request |-> w,
  outcome |-> workerOutcome[w],
  penalty |-> workerPenalty[w]
]

\* @type: (Str, Int) => Set($receipt);
ResolutionReceipts(v, g) ==
  {r \in receipts : r.validator = v /\ r.targetGeneration = g}

\* @type: (Str) => Int;
InitialStakeTotal(v) ==
  InitialBond[v] + InitialReward[v] + InitialWallet[v]

Init ==
  /\ phase = InitialPhase
  /\ quarantineOrigin = [v \in Validators |-> NoOrigin]
  /\ generation = InitialGeneration
  /\ bond = InitialBond
  /\ reward = InitialReward
  /\ wallet = InitialWallet
  /\ cooperativeStake = [v \in Validators |-> 0]
  /\ burnedStake = [v \in Validators |-> 0]
  /\ fuel = InitialFuel
  /\ cooperativeFuel = [v \in Validators |-> 0]
  /\ burnedFuel = [v \in Validators |-> 0]
  /\ stateVersion = [v \in Validators |-> 0]
  /\ lockOwner = [v \in Validators |-> NoWorker]
  /\ mintingHalted = {}
  /\ workerPhase = [w \in Workers |-> "Idle"]
  /\ workerOutcome = [w \in Workers |-> NoOutcome]
  /\ workerPenalty = [w \in Workers |-> 0]
  /\ workerVersion = [w \in Workers |-> 0]
  /\ workOrigin = [w \in Workers |-> NoOrigin]
  /\ workPhase = [w \in Workers |-> NoPhase]
  /\ workBond = [w \in Workers |-> 0]
  /\ workReward = [w \in Workers |-> 0]
  /\ workCooperativeStake = [w \in Workers |-> 0]
  /\ workBurnedStake = [w \in Workers |-> 0]
  /\ workFuel = [w \in Workers |-> 0]
  /\ workCooperativeFuel = [w \in Workers |-> 0]
  /\ workBurnedFuel = [w \in Workers |-> 0]
  /\ receipts = {}
  /\ abortPublished = FALSE
  /\ retryMutated = FALSE
  /\ unauthorizedAccepted = FALSE
  /\ fullGuiltyAccepted = FALSE
  /\ conflictMutated = FALSE
  /\ lastRestoredOrigin = NoOrigin
  /\ lastRestoredPhase = NoPhase

Slash(v) ==
  /\ phase[v] \in RestorablePhases
  /\ lockOwner[v] = NoWorker
  /\ quarantineOrigin' = [quarantineOrigin EXCEPT ![v] = phase[v]]
  /\ phase' = [phase EXCEPT ![v] = "Quarantined"]
  /\ mintingHalted' = mintingHalted \cup {v}
  /\ stateVersion' = [stateVersion EXCEPT ![v] = @ + 1]
  /\ lastRestoredOrigin' = NoOrigin
  /\ lastRestoredPhase' = NoPhase
  /\ UNCHANGED <<generation, bond, reward, wallet, cooperativeStake,
                  burnedStake, fuel, cooperativeFuel, burnedFuel, lockOwner,
                  workerPhase, workerOutcome, workerPenalty, workerVersion,
                  workOrigin, workPhase, workBond, workReward,
                  workCooperativeStake, workBurnedStake, workFuel,
                  workCooperativeFuel, workBurnedFuel, receipts,
                  abortPublished, retryMutated, unauthorizedAccepted,
                  fullGuiltyAccepted, conflictMutated>>

ValidOutcome(v, outcome, penalty) ==
  /\ outcome \in Outcomes
  /\ penalty \in PenaltyRange
  /\ IF outcome = "Guilty"
     THEN penalty < bond[v] \/ (Defect = "AllowFullGuilty" /\ penalty >= bond[v])
     ELSE penalty = 0

Begin(w, outcome, penalty) ==
  LET v == Target[w] IN
  /\ workerPhase[w] = "Idle"
  /\ phase[v] = "Quarantined"
  /\ ValidOutcome(v, outcome, penalty)
  /\ (TargetGeneration[w] = generation[v] \/ Defect = "IgnoreGeneration")
  /\ (lockOwner[v] = NoWorker \/ Defect = "NoTargetLock")
  /\ workerPhase' = [workerPhase EXCEPT ![w] = "Begun"]
  /\ workerOutcome' = [workerOutcome EXCEPT ![w] = outcome]
  /\ workerPenalty' = [workerPenalty EXCEPT ![w] = penalty]
  /\ workerVersion' = [workerVersion EXCEPT ![w] = stateVersion[v]]
  /\ workOrigin' = [workOrigin EXCEPT ![w] = quarantineOrigin[v]]
  /\ workPhase' = [workPhase EXCEPT ![w] = phase[v]]
  /\ workBond' = [workBond EXCEPT ![w] = bond[v]]
  /\ workReward' = [workReward EXCEPT ![w] = reward[v]]
  /\ workCooperativeStake' = [workCooperativeStake EXCEPT ![w] = cooperativeStake[v]]
  /\ workBurnedStake' = [workBurnedStake EXCEPT ![w] = burnedStake[v]]
  /\ workFuel' = [workFuel EXCEPT ![w] = fuel[v]]
  /\ workCooperativeFuel' = [workCooperativeFuel EXCEPT ![w] = cooperativeFuel[v]]
  /\ workBurnedFuel' = [workBurnedFuel EXCEPT ![w] = burnedFuel[v]]
  /\ lockOwner' =
       IF Defect = "NoTargetLock"
       THEN lockOwner
       ELSE [lockOwner EXCEPT ![v] = w]
  /\ UNCHANGED <<phase, quarantineOrigin, generation, bond, reward, wallet,
                  cooperativeStake, burnedStake, fuel, cooperativeFuel,
                  burnedFuel, stateVersion, mintingHalted, receipts,
                  abortPublished, retryMutated, unauthorizedAccepted,
                  fullGuiltyAccepted, conflictMutated,
                  lastRestoredOrigin, lastRestoredPhase>>

RejectStale(w) ==
  LET v == Target[w] IN
  /\ workerPhase[w] = "Idle"
  /\ phase[v] = "Quarantined"
  /\ TargetGeneration[w] /= generation[v]
  /\ Defect /= "IgnoreGeneration"
  /\ workerPhase' = [workerPhase EXCEPT ![w] = "StaleRejected"]
  /\ UNCHANGED <<phase, quarantineOrigin, generation, bond, reward, wallet,
                  cooperativeStake, burnedStake, fuel, cooperativeFuel,
                  burnedFuel, stateVersion, lockOwner, mintingHalted,
                  workerOutcome, workerPenalty, workerVersion, workOrigin,
                  workPhase, workBond, workReward, workCooperativeStake,
                  workBurnedStake, workFuel, workCooperativeFuel,
                  workBurnedFuel, receipts, abortPublished, retryMutated,
                  unauthorizedAccepted, fullGuiltyAccepted, conflictMutated,
                  lastRestoredOrigin, lastRestoredPhase>>

StageStake(w) ==
  /\ workerPhase[w] = "Begun"
  /\ workerPhase' = [workerPhase EXCEPT ![w] = "StakeStaged"]
  /\ CASE workerOutcome[w] = "Vindicated" ->
              UNCHANGED <<workBond, workReward, workCooperativeStake, workBurnedStake>>
          [] workerOutcome[w] = "Guilty" ->
              /\ workBond' = [workBond EXCEPT ![w] = @ - workerPenalty[w]]
              /\ workCooperativeStake' =
                   [workCooperativeStake EXCEPT ![w] = @ + workerPenalty[w]]
              /\ UNCHANGED <<workReward, workBurnedStake>>
          [] workerOutcome[w] = "Burned" ->
              /\ workBurnedStake' =
                   [workBurnedStake EXCEPT ![w] = @ + workBond[w] + workReward[w]]
              /\ workBond' = [workBond EXCEPT ![w] = 0]
              /\ workReward' = [workReward EXCEPT ![w] = 0]
              /\ UNCHANGED workCooperativeStake
  /\ UNCHANGED <<phase, quarantineOrigin, generation, bond, reward, wallet,
                  cooperativeStake, burnedStake, fuel, cooperativeFuel,
                  burnedFuel, stateVersion, lockOwner, mintingHalted,
                  workerOutcome, workerPenalty, workerVersion, workOrigin,
                  workPhase, workFuel, workCooperativeFuel, workBurnedFuel,
                  receipts, abortPublished, retryMutated,
                  unauthorizedAccepted, fullGuiltyAccepted, conflictMutated,
                  lastRestoredOrigin, lastRestoredPhase>>

StageFuel(w) ==
  /\ workerPhase[w] = "StakeStaged"
  /\ workerPhase' = [workerPhase EXCEPT ![w] = "FuelStaged"]
  /\ CASE workerOutcome[w] = "Vindicated" ->
              UNCHANGED <<workFuel, workCooperativeFuel, workBurnedFuel>>
          [] workerOutcome[w] = "Guilty" ->
              LET amount == IF workerPenalty[w] < workFuel[w]
                            THEN workerPenalty[w]
                            ELSE workFuel[w]
              IN /\ workFuel' = [workFuel EXCEPT ![w] = @ - amount]
                 /\ workCooperativeFuel' =
                      [workCooperativeFuel EXCEPT ![w] = @ + amount]
                 /\ UNCHANGED workBurnedFuel
          [] workerOutcome[w] = "Burned" ->
              /\ workBurnedFuel' =
                   [workBurnedFuel EXCEPT ![w] = @ + workFuel[w]]
              /\ workFuel' = [workFuel EXCEPT ![w] = 0]
              /\ UNCHANGED workCooperativeFuel
  /\ UNCHANGED <<phase, quarantineOrigin, generation, bond, reward, wallet,
                  cooperativeStake, burnedStake, fuel, cooperativeFuel,
                  burnedFuel, stateVersion, lockOwner, mintingHalted,
                  workerOutcome, workerPenalty, workerVersion, workOrigin,
                  workPhase, workBond, workReward, workCooperativeStake,
                  workBurnedStake, receipts, abortPublished, retryMutated,
                  unauthorizedAccepted, fullGuiltyAccepted, conflictMutated,
                  lastRestoredOrigin, lastRestoredPhase>>

StagePoS(w) ==
  /\ workerPhase[w] = "FuelStaged"
  /\ workerPhase' = [workerPhase EXCEPT ![w] = "PoSStaged"]
  /\ workPhase' = [workPhase EXCEPT ![w] =
       IF workerOutcome[w] = "Burned"
       THEN "Burned"
       ELSE IF Defect = "RestoreBonded"
            THEN "Bonded"
            ELSE workOrigin[w]]
  /\ UNCHANGED <<phase, quarantineOrigin, generation, bond, reward, wallet,
                  cooperativeStake, burnedStake, fuel, cooperativeFuel,
                  burnedFuel, stateVersion, lockOwner, mintingHalted,
                  workerOutcome, workerPenalty, workerVersion, workOrigin,
                  workBond, workReward, workCooperativeStake,
                  workBurnedStake, workFuel, workCooperativeFuel,
                  workBurnedFuel, receipts, abortPublished, retryMutated,
                  unauthorizedAccepted, fullGuiltyAccepted, conflictMutated,
                  lastRestoredOrigin, lastRestoredPhase>>

CanCommit(w) ==
  LET v == Target[w] IN
  /\ workerPhase[w] = "PoSStaged"
  /\ IF Defect = "NoTargetLock"
     THEN TRUE
     ELSE /\ lockOwner[v] = w
          /\ workerVersion[w] = stateVersion[v]
          /\ ResolutionReceipts(v, TargetGeneration[w]) = {}

Accept(w) ==
  LET v == Target[w] IN
  /\ CanCommit(w)
  /\ phase' = [phase EXCEPT ![v] = workPhase[w]]
  /\ quarantineOrigin' = [quarantineOrigin EXCEPT ![v] = NoOrigin]
  /\ bond' = [bond EXCEPT ![v] = workBond[w]]
  /\ reward' = [reward EXCEPT ![v] = workReward[w]]
  /\ cooperativeStake' =
       [cooperativeStake EXCEPT ![v] = workCooperativeStake[w]]
  /\ burnedStake' = [burnedStake EXCEPT ![v] = workBurnedStake[w]]
  /\ fuel' = [fuel EXCEPT ![v] = workFuel[w]]
  /\ cooperativeFuel' =
       [cooperativeFuel EXCEPT ![v] = workCooperativeFuel[w]]
  /\ burnedFuel' = [burnedFuel EXCEPT ![v] = workBurnedFuel[w]]
  /\ stateVersion' = [stateVersion EXCEPT ![v] = @ + 1]
  /\ lockOwner' = [lockOwner EXCEPT ![v] = NoWorker]
  /\ mintingHalted' =
       IF workerOutcome[w] = "Burned"
       THEN mintingHalted
       ELSE mintingHalted \ {v}
  /\ workerPhase' = [workerPhase EXCEPT ![w] = "Accepted"]
  /\ receipts' = receipts \cup {Receipt(w)}
  /\ unauthorizedAccepted' =
       (unauthorizedAccepted \/ (TargetGeneration[w] /= generation[v]))
  /\ fullGuiltyAccepted' =
       (fullGuiltyAccepted \/
        (workerOutcome[w] = "Guilty" /\ workerPenalty[w] >= bond[v]))
  /\ lastRestoredOrigin' =
       IF workerOutcome[w] = "Burned" THEN NoOrigin ELSE workOrigin[w]
  /\ lastRestoredPhase' = workPhase[w]
  /\ UNCHANGED <<generation, wallet, workerOutcome, workerPenalty,
                  workerVersion, workOrigin, workPhase, workBond, workReward,
                  workCooperativeStake, workBurnedStake, workFuel,
                  workCooperativeFuel, workBurnedFuel, abortPublished,
                  retryMutated, conflictMutated>>

RejectCommitConflict(w) ==
  LET v == Target[w] IN
  /\ workerPhase[w] = "PoSStaged"
  /\ ~CanCommit(w)
  /\ workerPhase' = [workerPhase EXCEPT ![w] = "ConflictRejected"]
  /\ lockOwner' =
       IF lockOwner[v] = w
       THEN [lockOwner EXCEPT ![v] = NoWorker]
       ELSE lockOwner
  /\ UNCHANGED <<phase, quarantineOrigin, generation, bond, reward, wallet,
                  cooperativeStake, burnedStake, fuel, cooperativeFuel,
                  burnedFuel, stateVersion, mintingHalted, workerOutcome,
                  workerPenalty, workerVersion, workOrigin, workPhase,
                  workBond, workReward, workCooperativeStake,
                  workBurnedStake, workFuel, workCooperativeFuel,
                  workBurnedFuel, receipts, abortPublished, retryMutated,
                  unauthorizedAccepted, fullGuiltyAccepted, conflictMutated,
                  lastRestoredOrigin, lastRestoredPhase>>

Abort(w, terminal) ==
  LET v == Target[w] IN
  LET publishStake ==
        Defect = "CheckpointStake" /\
        workerPhase[w] \in {"StakeStaged", "FuelStaged", "PoSStaged"}
      publishFuel ==
        Defect = "CheckpointFuel" /\
        workerPhase[w] \in {"FuelStaged", "PoSStaged"}
  IN
  /\ terminal \in {"Rejected", "Crashed"}
  /\ workerPhase[w] \in {"Begun", "StakeStaged", "FuelStaged", "PoSStaged"}
  /\ workerPhase' = [workerPhase EXCEPT ![w] = terminal]
  /\ bond' = IF publishStake THEN [bond EXCEPT ![v] = workBond[w]] ELSE bond
  /\ reward' = IF publishStake THEN [reward EXCEPT ![v] = workReward[w]] ELSE reward
  /\ cooperativeStake' =
       IF publishStake
       THEN [cooperativeStake EXCEPT ![v] = workCooperativeStake[w]]
       ELSE cooperativeStake
  /\ burnedStake' =
       IF publishStake
       THEN [burnedStake EXCEPT ![v] = workBurnedStake[w]]
       ELSE burnedStake
  /\ fuel' = IF publishFuel THEN [fuel EXCEPT ![v] = workFuel[w]] ELSE fuel
  /\ cooperativeFuel' =
       IF publishFuel
       THEN [cooperativeFuel EXCEPT ![v] = workCooperativeFuel[w]]
       ELSE cooperativeFuel
  /\ burnedFuel' =
       IF publishFuel
       THEN [burnedFuel EXCEPT ![v] = workBurnedFuel[w]]
       ELSE burnedFuel
  /\ stateVersion' =
       IF publishStake \/ publishFuel
       THEN [stateVersion EXCEPT ![v] = @ + 1]
       ELSE stateVersion
  /\ lockOwner' =
       IF lockOwner[v] = w
       THEN [lockOwner EXCEPT ![v] = NoWorker]
       ELSE lockOwner
  /\ abortPublished' = (abortPublished \/ publishStake \/ publishFuel)
  /\ UNCHANGED <<phase, quarantineOrigin, generation, wallet,
                  mintingHalted, workerOutcome, workerPenalty, workerVersion,
                  workOrigin, workPhase, workBond, workReward,
                  workCooperativeStake, workBurnedStake, workFuel,
                  workCooperativeFuel, workBurnedFuel, receipts,
                  retryMutated, unauthorizedAccepted, fullGuiltyAccepted,
                  conflictMutated, lastRestoredOrigin, lastRestoredPhase>>

Retry(w) ==
  LET v == Target[w] IN
  LET mutate ==
        Defect = "LostReceipt" /\ workerOutcome[w] = "Guilty" /\
        workerPenalty[w] < bond[v]
      fuelPenalty ==
        IF workerPenalty[w] < fuel[v] THEN workerPenalty[w] ELSE fuel[v]
  IN
  /\ workerPhase[w] = "Accepted"
  /\ Receipt(w) \in receipts
  /\ workerPhase' = [workerPhase EXCEPT ![w] = "Retried"]
  /\ bond' = IF mutate THEN [bond EXCEPT ![v] = @ - workerPenalty[w]] ELSE bond
  /\ cooperativeStake' =
       IF mutate
       THEN [cooperativeStake EXCEPT ![v] = @ + workerPenalty[w]]
       ELSE cooperativeStake
  /\ fuel' = IF mutate THEN [fuel EXCEPT ![v] = @ - fuelPenalty] ELSE fuel
  /\ cooperativeFuel' =
       IF mutate
       THEN [cooperativeFuel EXCEPT ![v] = @ + fuelPenalty]
       ELSE cooperativeFuel
  /\ stateVersion' =
       IF mutate THEN [stateVersion EXCEPT ![v] = @ + 1] ELSE stateVersion
  /\ retryMutated' = (retryMutated \/ mutate)
  /\ UNCHANGED <<phase, quarantineOrigin, generation, reward, wallet,
                  burnedStake, burnedFuel, lockOwner, mintingHalted,
                  workerOutcome, workerPenalty, workerVersion, workOrigin,
                  workPhase, workBond, workReward, workCooperativeStake,
                  workBurnedStake, workFuel, workCooperativeFuel,
                  workBurnedFuel, receipts, abortPublished,
                  unauthorizedAccepted, fullGuiltyAccepted, conflictMutated,
                  lastRestoredOrigin, lastRestoredPhase>>

ConflictRetry(w, outcome, penalty) ==
  /\ workerPhase[w] = "Accepted"
  /\ Receipt(w) \in receipts
  /\ outcome \in Outcomes
  /\ penalty \in PenaltyRange
  /\ (outcome /= workerOutcome[w] \/ penalty /= workerPenalty[w])
  /\ workerPhase' = [workerPhase EXCEPT ![w] = "ConflictRejected"]
  /\ receipts' =
       IF Defect = "OverwriteConflict"
       THEN receipts \cup {[
              validator |-> Target[w],
              targetGeneration |-> TargetGeneration[w],
              acceptedGeneration |-> generation[Target[w]],
              request |-> w,
              outcome |-> outcome,
              penalty |-> penalty
            ]}
       ELSE receipts
  /\ conflictMutated' = (conflictMutated \/ Defect = "OverwriteConflict")
  /\ UNCHANGED <<phase, quarantineOrigin, generation, bond, reward, wallet,
                  cooperativeStake, burnedStake, fuel, cooperativeFuel,
                  burnedFuel, stateVersion, lockOwner, mintingHalted,
                  workerOutcome, workerPenalty, workerVersion, workOrigin,
                  workPhase, workBond, workReward, workCooperativeStake,
                  workBurnedStake, workFuel, workCooperativeFuel,
                  workBurnedFuel, abortPublished, retryMutated,
                  unauthorizedAccepted, fullGuiltyAccepted,
                  lastRestoredOrigin, lastRestoredPhase>>

BeginWithdrawal(v) ==
  /\ phase[v] = "PendingWithdraw"
  /\ lockOwner[v] = NoWorker
  /\ phase' = [phase EXCEPT ![v] = "Withdrawing"]
  /\ stateVersion' = [stateVersion EXCEPT ![v] = @ + 1]
  /\ lastRestoredOrigin' = NoOrigin
  /\ lastRestoredPhase' = NoPhase
  /\ UNCHANGED <<quarantineOrigin, generation, bond, reward, wallet,
                  cooperativeStake, burnedStake, fuel, cooperativeFuel,
                  burnedFuel, lockOwner, mintingHalted, workerPhase,
                  workerOutcome, workerPenalty, workerVersion, workOrigin,
                  workPhase, workBond, workReward, workCooperativeStake,
                  workBurnedStake, workFuel, workCooperativeFuel,
                  workBurnedFuel, receipts, abortPublished, retryMutated,
                  unauthorizedAccepted, fullGuiltyAccepted, conflictMutated>>

CompleteWithdrawal(v) ==
  /\ phase[v] = "Withdrawing"
  /\ lockOwner[v] = NoWorker
  /\ phase' = [phase EXCEPT ![v] = "Withdrawn"]
  /\ wallet' = [wallet EXCEPT ![v] = @ + bond[v] + reward[v]]
  /\ bond' = [bond EXCEPT ![v] = 0]
  /\ reward' = [reward EXCEPT ![v] = 0]
  /\ stateVersion' = [stateVersion EXCEPT ![v] = @ + 1]
  /\ lastRestoredOrigin' = NoOrigin
  /\ lastRestoredPhase' = NoPhase
  /\ UNCHANGED <<quarantineOrigin, generation, cooperativeStake,
                  burnedStake, fuel, cooperativeFuel, burnedFuel, lockOwner,
                  mintingHalted, workerPhase, workerOutcome, workerPenalty,
                  workerVersion, workOrigin, workPhase, workBond, workReward,
                  workCooperativeStake, workBurnedStake, workFuel,
                  workCooperativeFuel, workBurnedFuel, receipts,
                  abortPublished, retryMutated, unauthorizedAccepted,
                  fullGuiltyAccepted, conflictMutated>>

FreshBond(v) ==
  /\ phase[v] = "Withdrawn"
  /\ generation[v] < MaxGeneration
  /\ wallet[v] >= BondAmount
  /\ phase' = [phase EXCEPT ![v] = "Bonded"]
  /\ generation' = [generation EXCEPT ![v] = @ + 1]
  /\ bond' = [bond EXCEPT ![v] = BondAmount]
  /\ reward' = [reward EXCEPT ![v] = 0]
  /\ wallet' = [wallet EXCEPT ![v] = @ - BondAmount]
  /\ stateVersion' = [stateVersion EXCEPT ![v] = @ + 1]
  /\ lastRestoredOrigin' = NoOrigin
  /\ lastRestoredPhase' = NoPhase
  /\ UNCHANGED <<quarantineOrigin, cooperativeStake, burnedStake, fuel,
                  cooperativeFuel, burnedFuel, lockOwner, mintingHalted,
                  workerPhase, workerOutcome, workerPenalty, workerVersion,
                  workOrigin, workPhase, workBond, workReward,
                  workCooperativeStake, workBurnedStake, workFuel,
                  workCooperativeFuel, workBurnedFuel, receipts,
                  abortPublished, retryMutated, unauthorizedAccepted,
                  fullGuiltyAccepted, conflictMutated>>

Next ==
  \/ \E v \in Validators : Slash(v)
  \/ \E w \in Workers, outcome \in Outcomes, penalty \in PenaltyRange :
       Begin(w, outcome, penalty)
  \/ \E w \in Workers : RejectStale(w)
  \/ \E w \in Workers : StageStake(w)
  \/ \E w \in Workers : StageFuel(w)
  \/ \E w \in Workers : StagePoS(w)
  \/ \E w \in Workers : Accept(w)
  \/ \E w \in Workers : RejectCommitConflict(w)
  \/ \E w \in Workers, terminal \in {"Rejected", "Crashed"} : Abort(w, terminal)
  \/ \E w \in Workers : Retry(w)
  \/ \E w \in Workers, outcome \in Outcomes, penalty \in PenaltyRange :
       ConflictRetry(w, outcome, penalty)
  \/ \E v \in Validators : BeginWithdrawal(v)
  \/ \E v \in Validators : CompleteWithdrawal(v)
  \/ \E v \in Validators : FreshBond(v)

Spec == Init /\ [][Next]_vars

ReceiptType == [
  validator : Validators,
  targetGeneration : 0..MaxGeneration,
  acceptedGeneration : 0..MaxGeneration,
  request : Workers,
  outcome : Outcomes,
  penalty : PenaltyRange
]

TypeOK ==
  /\ phase \in [Validators -> Phases]
  /\ quarantineOrigin \in [Validators -> RestorablePhases \cup {NoOrigin}]
  /\ generation \in [Validators -> 0..MaxGeneration]
  /\ bond \in [Validators -> ValueRange]
  /\ reward \in [Validators -> ValueRange]
  /\ wallet \in [Validators -> ValueRange]
  /\ cooperativeStake \in [Validators -> ValueRange]
  /\ burnedStake \in [Validators -> ValueRange]
  /\ fuel \in [Validators -> ValueRange]
  /\ cooperativeFuel \in [Validators -> ValueRange]
  /\ burnedFuel \in [Validators -> ValueRange]
  /\ stateVersion \in [Validators -> 0..20]
  /\ lockOwner \in [Validators -> Workers \cup {NoWorker}]
  /\ mintingHalted \subseteq Validators
  /\ workerPhase \in [Workers -> WorkerPhases]
  /\ workerOutcome \in [Workers -> Outcomes \cup {NoOutcome}]
  /\ workerPenalty \in [Workers -> PenaltyRange]
  /\ workerVersion \in [Workers -> 0..20]
  /\ workOrigin \in [Workers -> RestorablePhases \cup {NoOrigin}]
  /\ workPhase \in [Workers -> Phases \cup {NoPhase}]
  /\ workBond \in [Workers -> ValueRange]
  /\ workReward \in [Workers -> ValueRange]
  /\ workCooperativeStake \in [Workers -> ValueRange]
  /\ workBurnedStake \in [Workers -> ValueRange]
  /\ workFuel \in [Workers -> ValueRange]
  /\ workCooperativeFuel \in [Workers -> ValueRange]
  /\ workBurnedFuel \in [Workers -> ValueRange]
  /\ receipts \subseteq ReceiptType
  /\ abortPublished \in BOOLEAN
  /\ retryMutated \in BOOLEAN
  /\ unauthorizedAccepted \in BOOLEAN
  /\ fullGuiltyAccepted \in BOOLEAN
  /\ conflictMutated \in BOOLEAN
  /\ lastRestoredOrigin \in RestorablePhases \cup {NoOrigin}
  /\ lastRestoredPhase \in Phases \cup {NoPhase}

StakeConserved ==
  \A v \in Validators :
    bond[v] + reward[v] + wallet[v] + cooperativeStake[v] + burnedStake[v]
      = InitialStakeTotal(v)

FuelConserved ==
  \A v \in Validators :
    fuel[v] + cooperativeFuel[v] + burnedFuel[v] = InitialFuel[v]

LiveStakePositive ==
  \A v \in Validators :
    phase[v] \in RestorablePhases \cup {"Quarantined"} => bond[v] > 0

QuarantineCarriesExactOrigin ==
  \A v \in Validators :
    /\ (phase[v] = "Quarantined") <=> (quarantineOrigin[v] \in RestorablePhases)
    /\ (phase[v] = "Quarantined") => v \in mintingHalted

ReceiptsUseAuthorizedGeneration ==
  \A receipt \in receipts :
    receipt.targetGeneration = receipt.acceptedGeneration

AtMostOneResolutionPerIncarnation ==
  \A v \in Validators, g \in 0..MaxGeneration :
    Cardinality(ResolutionReceipts(v, g)) <= 1

RejectedTransactionsPublishNothing == ~abortPublished
ExactRetriesAreEffectFree == ~retryMutated
UnauthorizedGenerationsNeverCommit == ~unauthorizedAccepted
GuiltyIsStrictlyPartial == ~fullGuiltyAccepted
ConflictingRetriesAreEffectFree == ~conflictMutated

RestoresExactLifecycle ==
  lastRestoredOrigin = NoOrigin \/ lastRestoredPhase = lastRestoredOrigin

=============================================================================
