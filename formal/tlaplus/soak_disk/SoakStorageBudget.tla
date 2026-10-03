-------------------------- MODULE SoakStorageBudget --------------------------
(* The consumers that fill the soak VM's one volume, each as an event source *)
(* with a per-event byte cap and a per-unit rate: blocks stored, deploys     *)
(* executed, history checkpoints, node logs, and container logs. Consumption *)
(* is the sum of what the events retain. RateBound is the derived worst case *)
(* per clock unit; it is the value SoakDiskGuardian takes as WriteRateMax.   *)
(* The deploy cap is not derived here: deploy_storage/DeployStorageBound     *)
(* proves it from phlo accounting, and this module only names it. Three      *)
(* Boolean constants mark caps the node does not enforce today; each control  *)
(* drops one and shows the budget fails without it.                          *)
EXTENDS Naturals, TLC

CONSTANTS BlockBytesCap,      \* bytes one stored block may add
          BlocksPerUnit,      \* blocks stored per clock unit
          DeployStorageCap,   \* bytes one deploy may retain (DeployStorageBound)
          DeploysPerUnit,     \* deploys executed per clock unit
          CheckpointBytesCap, \* bytes one history checkpoint may add
          CheckpointsPerUnit, \* checkpoints per clock unit
          LogBytesPerUnit,    \* node log bytes per clock unit
          ContainerLogBytesPerUnit, \* container log bytes per clock unit
          Excess,             \* how far an uncapped event may exceed its cap
          Horizon,            \* clock units explored
          CapBlocks,          \* the node enforces a block byte cap
          CapLogs,            \* the node enforces a log byte cap
          CapHistory          \* the node bounds checkpoint growth

ASSUME /\ {BlockBytesCap, BlocksPerUnit, DeployStorageCap, DeploysPerUnit,
           CheckpointBytesCap, CheckpointsPerUnit, LogBytesPerUnit,
           ContainerLogBytesPerUnit, Excess} \subseteq Nat
       /\ Horizon \in Nat \ {0}
       /\ {CapBlocks, CapLogs, CapHistory} \subseteq BOOLEAN

RateBound == BlockBytesCap * BlocksPerUnit
             + DeployStorageCap * DeploysPerUnit
             + CheckpointBytesCap * CheckpointsPerUnit
             + LogBytesPerUnit
             + ContainerLogBytesPerUnit

VARIABLES elapsed,   \* clock units completed
          consumed,  \* bytes retained so far
          blocks, deploys, checkpoints, \* events in the current unit
          logged, containerLogged        \* bytes logged in the current unit

vars == <<elapsed, consumed, blocks, deploys, checkpoints, logged, containerLogged>>

Init ==
    /\ elapsed = 0
    /\ consumed = 0
    /\ blocks = 0
    /\ deploys = 0
    /\ checkpoints = 0
    /\ logged = 0
    /\ containerLogged = 0

Bytes(cap, capped) == IF capped THEN 0..cap ELSE 0..(cap + Excess)

StoreBlock ==
    /\ blocks < BlocksPerUnit
    /\ \E b \in Bytes(BlockBytesCap, CapBlocks) :
         consumed' = consumed + b
    /\ blocks' = blocks + 1
    /\ UNCHANGED <<elapsed, deploys, checkpoints, logged, containerLogged>>

ExecuteDeploy ==
    /\ deploys < DeploysPerUnit
    /\ \E b \in 0..DeployStorageCap : consumed' = consumed + b
    /\ deploys' = deploys + 1
    /\ UNCHANGED <<elapsed, blocks, checkpoints, logged, containerLogged>>

Checkpoint ==
    /\ checkpoints < CheckpointsPerUnit
    /\ \E b \in Bytes(CheckpointBytesCap, CapHistory) :
         consumed' = consumed + b
    /\ checkpoints' = checkpoints + 1
    /\ UNCHANGED <<elapsed, blocks, deploys, logged, containerLogged>>

\* Logging is a byte stream; one action spends the whole unit's allowance.
Log ==
    /\ logged = 0
    /\ \E b \in Bytes(LogBytesPerUnit, CapLogs) :
         /\ consumed' = consumed + b
         /\ logged' = b
    /\ UNCHANGED <<elapsed, blocks, deploys, checkpoints, containerLogged>>

ContainerLog ==
    /\ containerLogged = 0
    /\ \E b \in 0..ContainerLogBytesPerUnit :
         /\ consumed' = consumed + b
         /\ containerLogged' = b
    /\ UNCHANGED <<elapsed, blocks, deploys, checkpoints, logged>>

Tick ==
    /\ elapsed < Horizon
    /\ elapsed' = elapsed + 1
    /\ blocks' = 0
    /\ deploys' = 0
    /\ checkpoints' = 0
    /\ logged' = 0
    /\ containerLogged' = 0
    /\ UNCHANGED consumed

Next == StoreBlock \/ ExecuteDeploy \/ Checkpoint \/ Log \/ ContainerLog \/ Tick

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ elapsed \in 0..Horizon
    /\ consumed \in Nat
    /\ blocks \in 0..BlocksPerUnit
    /\ deploys \in 0..DeploysPerUnit
    /\ checkpoints \in 0..CheckpointsPerUnit
    /\ logged \in Nat
    /\ containerLogged \in Nat

\* The theorem: with every cap enforced, consumption over any prefix of the
\* run, including the unit in progress, stays within RateBound per unit.
WithinBudget == consumed <= RateBound * (elapsed + 1)
Completes == <>(elapsed = Horizon)
=============================================================================
