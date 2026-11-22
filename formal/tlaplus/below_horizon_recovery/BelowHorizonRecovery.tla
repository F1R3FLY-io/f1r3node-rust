--------------------- MODULE BelowHorizonRecovery ---------------------
\* Protocol-level model of the #1059 recovery flow. Covers the two
\* correctness invariants that make the bug fix complete:
\*
\*   (a) Signal-delivery liveness: an observer below peers'
\*       effective_anchor that stays in Running eventually sees a
\*       BlockPrunedResponse and records belowHorizonFlag. This path
\*       landed in PR #1078 and the spec verifies it under both the
\*       pre-fix and post-fix launch-routing configurations.
\*
\*   (b) Catch-up liveness: an observer whose stored state is below
\*       peers' GC horizon eventually reaches a state at or above
\*       peersAnchor. The post-fix model (LaunchRoutesLFS = TRUE)
\*       proves this. The pre-fix model (LaunchRoutesLFS = FALSE,
\*       i.e. the state immediately after PR #1078) violates it —
\*       the counterexample is the loop observed in the soak: the
\*       signal reaches the observer, the operator restarts, launch
\*       reconnects as Running at the same below-horizon height, and
\*       the loop repeats forever.
\*
\* The within-session pending-area pressure (soak's AreaFull log) is
\* a transient resource effect, bounded by the restart cycle that
\* item (b) delivers, and is verified at the Rust level (see
\* IngestState::block_pruned tests). It is not modeled here.
EXTENDS Naturals

CONSTANTS
    MaxHeight,          \* upper bound on peer finalization progression
    HorizonDepth,       \* peersFloor - peersAnchor gap
    LaunchRoutesLFS     \* the follow-up fix: launch-time routing to Initializing when below horizon

ASSUME
    /\ MaxHeight \in Nat
    /\ HorizonDepth \in Nat
    /\ HorizonDepth > 0
    /\ MaxHeight > HorizonDepth
    /\ LaunchRoutesLFS \in BOOLEAN

Phase == {"Running", "Restarting", "Initializing"}

VARIABLES
    peersFloor,       \* peers' latest finalized height; monotone
    peersAnchor,      \* peers' effective_anchor (body-prune floor); monotone
    observerHeight,   \* observer's latest validated height; monotone within a launch
    observerPhase,    \* "Running" | "Restarting" | "Initializing"
    belowHorizonFlag  \* set true when observer sees a BlockPrunedResponse this launch

vars == <<peersFloor, peersAnchor, observerHeight, observerPhase, belowHorizonFlag>>

TypeOK ==
    /\ peersFloor       \in 0..MaxHeight
    /\ peersAnchor      \in 0..MaxHeight
    /\ observerHeight   \in 0..MaxHeight
    /\ observerPhase    \in Phase
    /\ belowHorizonFlag \in BOOLEAN

Init ==
    /\ peersFloor       = 0
    /\ peersAnchor      = 0
    /\ observerHeight   = 0
    /\ observerPhase    = "Running"
    /\ belowHorizonFlag = FALSE

\* ---- Peer actions ---------------------------------------------------

\* Peers advance their finalized tip and, after HorizonDepth, their
\* body-prune anchor. Finalization alone leaves anchor behind until
\* floor >= HorizonDepth (warm-up on a fresh shard).
PeersAdvance ==
    /\ peersFloor < MaxHeight
    /\ peersFloor' = peersFloor + 1
    /\ peersAnchor' = IF peersFloor + 1 > HorizonDepth
                        THEN peersFloor + 1 - HorizonDepth
                        ELSE 0
    /\ UNCHANGED <<observerHeight, observerPhase, belowHorizonFlag>>

\* ---- Observer actions, Running phase -------------------------------

\* Observer validates a block from peers if the block is above peers'
\* prune anchor (peers still have the body). Happy-path intake.
ObserverCatchUp ==
    /\ observerPhase = "Running"
    /\ observerHeight < peersFloor
    /\ observerHeight + 1 > peersAnchor
    /\ observerHeight' = observerHeight + 1
    /\ UNCHANGED <<peersFloor, peersAnchor, observerPhase, belowHorizonFlag>>

\* Observer requests a block whose body peers have pruned. Peer
\* responds with BlockPrunedResponse, which sets belowHorizonFlag.
\* This is the path PR #1078 delivered.
ObserverReceivesPrunedResponse ==
    /\ observerPhase = "Running"
    /\ observerHeight < peersAnchor
    /\ belowHorizonFlag' = TRUE
    /\ UNCHANGED <<peersFloor, peersAnchor, observerHeight, observerPhase>>

\* ---- Restart + launch decision --------------------------------------

\* Operator (or supervisor) restarts the observer. Conditional on the
\* below-horizon flag having been set this launch: in practice the
\* operator restarts in response to the warning log emitted by
\* `IngestState::block_pruned`, not spontaneously. This condition is
\* what makes `Live_SignalDelivered` a prerequisite for recovery
\* rather than a parallel property.
ObserverRestart ==
    /\ observerPhase = "Running"
    /\ belowHorizonFlag
    /\ observerPhase' = "Restarting"
    /\ UNCHANGED <<peersFloor, peersAnchor, observerHeight, belowHorizonFlag>>

\* At launch with stored data, the fix routes to Initializing when the
\* observer's stored height is below peers' GC horizon; otherwise it
\* reconnects as before. The pre-fix configuration disables the LFS
\* branch so every restart resumes Running regardless — the stuck state
\* the soak observed.
LaunchDecisionLFS ==
    /\ observerPhase = "Restarting"
    /\ LaunchRoutesLFS
    /\ observerHeight < peersAnchor
    /\ observerPhase' = "Initializing"
    /\ belowHorizonFlag' = FALSE
    /\ UNCHANGED <<peersFloor, peersAnchor, observerHeight>>

LaunchDecisionRunning ==
    /\ observerPhase = "Restarting"
    /\ \/ ~LaunchRoutesLFS
       \/ observerHeight >= peersAnchor
    /\ observerPhase' = "Running"
    /\ belowHorizonFlag' = FALSE
    /\ UNCHANGED <<peersFloor, peersAnchor, observerHeight>>

\* Initializing runs the LFS flow and brings observerHeight up to
\* peersAnchor, then transitions to Running.
InitializingCompletes ==
    /\ observerPhase = "Initializing"
    /\ observerHeight' = IF peersAnchor > observerHeight THEN peersAnchor ELSE observerHeight
    /\ observerPhase' = "Running"
    /\ UNCHANGED <<peersFloor, peersAnchor, belowHorizonFlag>>

Next ==
    \/ PeersAdvance
    \/ ObserverCatchUp
    \/ ObserverReceivesPrunedResponse
    \/ ObserverRestart
    \/ LaunchDecisionLFS
    \/ LaunchDecisionRunning
    \/ InitializingCompletes

\* Fairness: the signal reaches the observer, the operator restarts,
\* launch takes the correct route, LFS runs to completion, intake
\* catches up when peers serve.
Fairness ==
    /\ WF_vars(ObserverReceivesPrunedResponse)
    /\ WF_vars(ObserverRestart)
    /\ WF_vars(LaunchDecisionLFS)
    /\ WF_vars(LaunchDecisionRunning)
    /\ WF_vars(InitializingCompletes)
    /\ WF_vars(ObserverCatchUp)

Spec == Init /\ [][Next]_vars /\ Fairness

\* ---- Safety invariants ---------------------------------------------

\* Peer anchor never leads peer floor by more than HorizonDepth.
Inv_AnchorTrailsFloor ==
    peersAnchor + HorizonDepth >= peersFloor \/ peersFloor < HorizonDepth

\* Observer never validates past peers' latest finalized height.
Inv_HeightBounded == observerHeight <= peersFloor

\* ---- Liveness ------------------------------------------------------

\* Core recovery property. Whenever observerHeight falls below peers'
\* prune anchor, the system eventually reaches a state where the
\* observer is at or above peersAnchor again. The post-fix model
\* proves this under fairness. The pre-fix model (LaunchRoutesLFS =
\* FALSE) violates it: every restart resumes Running at the same
\* below-horizon height, matching the soak evidence.
Live_ObserverCatchesUp ==
    [](observerHeight < peersAnchor => <>(observerHeight >= peersAnchor))

\* Signal delivery landed in PR #1078. Verified in both the pre-fix
\* and post-fix configurations to confirm the probe path still fires.
Live_SignalDelivered ==
    [](
        /\ observerPhase = "Running"
        /\ observerHeight < peersAnchor
     => <>belowHorizonFlag
    )

=============================================================================
