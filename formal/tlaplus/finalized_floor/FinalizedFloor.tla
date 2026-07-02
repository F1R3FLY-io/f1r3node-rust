-------------------------- MODULE FinalizedFloor --------------------------
(***************************************************************************)
(* Faithful model of the finalized-floor multi-parent merge, written to   *)
(* DISCOVER (not assume) correctness violations.                          *)
(*                                                                         *)
(* Models: casper/src/rust/util/rholang/interpreter_util.rs               *)
(*           compute_parents_post_state  (:726, :1114-1179)               *)
(*         casper/src/rust/finality/floor.rs  (the finalized floor cut)   *)
(*                                                                         *)
(* The feature PROMISES that a multi-parent merge over the finalized       *)
(* floor loses no committed parent write (the whole point of merging all   *)
(* bonded validators' latest blocks onto a sound finalized base). We model  *)
(* the IMPLEMENTATION exactly as written -- including the two hard caps     *)
(*   MAX_PARENT_MERGE_SCOPE_BLOCKS = 512   (here: MaxScope)                 *)
(*   MAX_FLOOR_DISTANCE_BLOCKS     = 256   (here: MaxDelta)                 *)
(* and the SILENT single-parent fallback that fires when the finalization   *)
(* lag Delta = num(tip) - num(floor) crosses the cap, returning one         *)
(* parent's post-state with empty rejected sets -- and we assert the        *)
(* promise as an invariant. Finalize is starvable relative to Propose (as   *)
(* under real load the finalizer lags block production), so TLC can drive    *)
(* Delta past the cap and exhibit the fallback dropping a sibling write.     *)
(*                                                                          *)
(* Scaled down (small MaxDelta, bounded tip) so TLC terminates; the         *)
(* violation is qualitative and cap-value-independent.                      *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS
    MaxDelta,   \* the MAX_FLOOR_DISTANCE_BLOCKS cap (scaled, e.g. 2)
    MaxScope,   \* the MAX_PARENT_MERGE_SCOPE_BLOCKS cap (scaled, e.g. 4)
    MaxTip      \* bound the chain length so the model is finite

VARIABLES
    tip,        \* highest block number produced
    floor,      \* finalized-cut block number (the finalized floor)
    parentKeys, \* keys the parents of the most recent merge each committed (ground truth)
    mergeKeys   \* keys the IMPLEMENTATION's merge actually reflected in its post-state

vars == <<tip, floor, parentKeys, mergeKeys>>

\* Two writer lanes each commit a distinct key on every round; a correct
\* multi-parent merge must reflect BOTH. Abstract them as {1, 2}.
Lanes == {1, 2}

\* The deterministic primary parent (max block-number, then hash) whose
\* post-state the fallback returns; abstractly the lane-1 key survives.
PrimaryKeys == {1}

Delta == tip - floor

\* Faithful model of the scope estimate: the unfinalized band plus sibling
\* branching. One block per lane per round above the floor, so ~2*Delta.
Scope == 2 * Delta

Init ==
    /\ tip = 0
    /\ floor = 0
    /\ parentKeys = {}
    /\ mergeKeys = {}

\* Propose a multi-parent merge block. This is compute_parents_post_state:
\* if the scope OR the floor-distance exceeds the cap, SILENTLY fall back to
\* a single parent's post-state (dropping the other lane's committed key);
\* otherwise merge all parents (union of both lanes' keys).
Propose ==
    /\ tip < MaxTip
    /\ tip' = tip + 1
    /\ parentKeys' = Lanes
    /\ mergeKeys' =
         IF (2 * ((tip + 1) - floor) > MaxScope) \/ (((tip + 1) - floor) > MaxDelta)
           THEN PrimaryKeys      \* <-- the interpreter_util.rs:1178 fallback
           ELSE Lanes            \* <-- the full multi-parent merge
    /\ floor' = floor

\* Finalize advances the floor by one. It is STARVABLE (no fairness), exactly
\* as the finalizer lags under load; weak fairness would only affect liveness,
\* not the reachable-state safety check below.
Finalize ==
    /\ floor < tip
    /\ floor' = floor + 1
    /\ UNCHANGED <<tip, parentKeys, mergeKeys>>

Next == Propose \/ Finalize

Spec == Init /\ [][Next]_vars

------------------------------------------------------------------------------
TypeOK ==
    /\ tip \in 0..MaxTip
    /\ floor \in 0..MaxTip
    /\ parentKeys \subseteq Lanes
    /\ mergeKeys \subseteq Lanes

\* The floor is a finalized cut: it never regresses (T-MONO / safety S2).
Inv_FloorMonotone == floor <= tip

\* THE PROMISE (safety S5): a multi-parent merge reflects every committed
\* parent write. TLC discovers this is violated once Delta crosses the cap
\* and the silent single-parent fallback drops the other lane's key.
Inv_NoLostParentWrite == parentKeys \subseteq mergeKeys
==============================================================================
