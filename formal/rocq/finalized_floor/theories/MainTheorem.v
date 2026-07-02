(* ===========================================================================
   MainTheorem.v - Capstone: the finalized-floor multi-parent merge is correct.

   Bundles the module-level theorems into one end-to-end statement. Each conjunct
   is discharged by `exact` against the already-proven, axiom-free lemma, so the
   capstone itself introduces no new assumptions (verify with
   `Print Assumptions finalized_floor_merge_correct`).

   The conjuncts, and what each rules out (safety S-labels from the spec):

     T-TERM      spine walk terminates          -- floor derivation always halts
     T-MONO/L-ANC ancestor-monotone finalization-- floor cannot regress (¬S2);
                                                    downward-closed finalized cut
     L-SNAP      snapshot-monotone finalization -- larger justification snapshot
                                                    only ever finalizes more
     T-CACHE     frontier cache transparent     -- warm up-walk == cold down-walk,
                                                    so caching cannot fork (¬S1)
     T-DETMERGE  merge order-independent        -- no fork from parent fold order (¬S6)
     T-K1        no mergeable write lost         -- the ~400-block write-loss (¬S5)
     T-NDA       recovery not double-applied     -- effects applied at most once

   The H1 deterministic backstop (over-Δ merges refuse rather than substitute a
   lossy state) and its liveness are verified in TLA+ (SpecFixed:
   Inv_NoLostParentWrite + Inv_DeltaWithinCap + Liveness_Progress); this Rocq
   development supplies the determinism and algebra that keep every honest node
   in lockstep.
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

From FinalizedFloor Require Import Foundation.
From FinalizedFloor Require Import CliqueOracle.
From FinalizedFloor Require Import Floor.
From FinalizedFloor Require Import Merge.
From FinalizedFloor Require Import Recovery.

Theorem finalized_floor_merge_correct :
  (* T-TERM: the main-parent spine walk always reaches genesis. *)
  (forall d, wf_spine d ->
     forall b, In b d ->
       exists g, walk_spine d b (blk_num b) = Some g /\ blk_main_parent g = None)
  /\
  (* T-MONO / L-ANC: finalization is downward-closed along ancestry. *)
  (forall d c J b b', anc_of d b' b -> Finalized d c J b -> Finalized d c J b')
  /\
  (* L-SNAP: finalization is monotone under snapshot growth. *)
  (forall d c J J' b, snap_extends J' J -> Finalized d c J b -> Finalized d c J' b)
  /\
  (* T-CACHE: the warm frontier up-walk equals the cold down-walk (no fork). *)
  (forall pivot band, AdjDC band ->
     lastTrue ((pivot, true) :: band) = Some (upgo pivot band))
  /\
  (* T-DETMERGE / T-CONV: the mergeable-channel merge is order-independent. *)
  (forall l1 l2, Permutation l1 l2 -> merge_or l1 = merge_or l2)
  /\
  (* T-K1: no mergeable write is lost (every set bit survives the merge). *)
  (forall l x i, In x l -> Nat.testbit x i = true -> Nat.testbit (merge_or l) i = true)
  /\
  (* T-NDA: recovery never double-applies an effect. *)
  (forall s d, apply_effect (apply_effect s d) d = apply_effect s d).
Proof.
  repeat split.
  - exact spine_walk_terminates.
  - exact L_ANC.
  - exact L_SNAP.
  - exact frontier_cache_transparent.
  - exact merge_or_perm.
  - exact merge_or_no_lost_bit.
  - exact apply_idem.
Qed.
