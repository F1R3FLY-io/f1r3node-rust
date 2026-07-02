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
From Stdlib Require Import ZArith.
Import ListNotations.

From FinalizedFloor Require Import Foundation.
From FinalizedFloor Require Import CliqueOracle.
From FinalizedFloor Require Import Floor.
From FinalizedFloor Require Import Merge.
From FinalizedFloor Require Import Recovery.
From FinalizedFloor Require Import Selection.
From FinalizedFloor Require Import IntegerAdd.

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

(* ===========================================================================
   Phase 6 capstone extensions — the floor SELECTION (T-SOUND/T-FIN/T-LIN/
   T-PS/T-COMM/H3) and the IntegerAdd ALGEBRA (T-ALG c/d + launder-free + bound).
   Each conjunct is discharged by `exact` against its module lemma, so these add
   no assumptions (verify with `Print Assumptions`).
   =========================================================================== *)

Theorem finalized_floor_selection_correct :
  (* T-SOUND: the chosen merge base is sound. *)
  (forall d fuel parents cands f,
     select_floor d fuel parents cands = Some f ->
     is_sound d fuel parents cands f = true)
  /\
  (* T-SOUND (Err-correct) / T-PS: for ANY parent list, None ⇒ no candidate is
     sound (the incompatible-fork Err is correct, never a silent unsound base). *)
  (forall d fuel parents cands,
     select_floor d fuel parents cands = None ->
     forall c, In c cands -> is_sound d fuel parents cands c = false)
  /\
  (* T-SOUND-A / T-LIN: a Case-A base is a common DAG-ancestor of every parent. *)
  (forall d fuel parents c,
     case_a d fuel parents c = true ->
     forall p, In p parents -> c = p \/ anc_of d c p)
  /\
  (* T-FIN: the chosen base is drawn from the candidates, so it is finalized when
     they are. *)
  (forall (Fin : BlockHash -> Prop) d fuel parents cands f,
     Forall Fin cands -> select_floor d fuel parents cands = Some f -> Fin f)
  /\
  (* T-COMM: the committee is bonds_of(floor), a pure function of the floor. *)
  (forall bonds_of d fuel parents cands f,
     select_floor d fuel parents cands = Some f ->
     committee_used bonds_of d fuel parents cands = Some (bonds_of f))
  /\
  (* H3: the floor-bounded scan covers every parent write at or above the floor. *)
  (forall d parents fl p w,
     wf_dag_num d -> In p parents -> anc_of d fl w -> anc_of d w p ->
     in_scope d parents fl w).
Proof.
  (* build the tuple directly; `repeat split` would over-split `in_scope`. *)
  exact (conj select_sound
          (conj select_none_correct
            (conj case_a_common_ancestor
              (conj select_finalized
                (conj committee_is_floor_bonds scope_covers_band))))).
Qed.

Open Scope Z_scope.

Theorem finalized_floor_arithmetic_correct :
  (* T-ALG(c): wrapping-add group laws (associativity + commutativity). *)
  (forall a b c : Z, wadd (wadd a b) c = wadd a (wadd b c))
  /\ (forall a b : Z, wadd a b = wadd b a)
  /\
  (* T-ALG(d): the checked apply-to-base rejects on overflow OR a negative result. *)
  (forall base diff : Z, ~ in_range (base + diff) -> checked_apply base diff = None)
  /\ (forall base diff : Z, base + diff < 0 -> checked_apply base diff = None)
  /\
  (* The fail-loudly FIX is launder-free: if checked_combine accepts, it returns
     the TRUE sum, in range — a wrapped value can never be laundered through it. *)
  (forall (l : list Z) (c : Z), checked_combine l = Some c -> c = true_sum l /\ in_range c)
  /\
  (* Defense-in-depth: while every partial sum stays in range, wrapping = checked
     = true sum, so the launder cannot arise. *)
  (forall l : list Z, safe l -> checked_combine l = Some (true_sum l) /\ wsum l = true_sum l).
Proof.
  exact (conj wadd_assoc
          (conj wadd_comm
            (conj checked_apply_rejects_overflow
              (conj checked_apply_rejects_negative
                (conj checked_combine_sound supply_cap_no_launder))))).
Qed.

Close Scope Z_scope.
