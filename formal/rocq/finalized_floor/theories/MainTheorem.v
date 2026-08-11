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
From FinalizedFloor Require Import OccurrenceDisposition.
From FinalizedFloor Require Import Recovery.
From FinalizedFloor Require Import Selection.
From FinalizedFloor Require Import IntegerAdd.
From FinalizedFloor Require Import FtExact.
From FinalizedFloor Require Import FtProvenance.

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

Theorem finalized_floor_occurrence_correct :
  (forall records rejected,
     tombstoned (reject_occurrence records rejected) rejected)
  /\
  (forall records rejected survivor,
     deploy_id rejected = deploy_id survivor ->
     source_id rejected <> source_id survivor ->
     active records survivor ->
     active (reject_occurrence records rejected) survivor)
  /\
  (forall records left right candidate,
     tombstoned (reject_occurrence (reject_occurrence records left) right) candidate <->
     tombstoned (reject_occurrence (reject_occurrence records right) left) candidate)
  /\
  (forall winner loser,
     deploy_id winner = deploy_id loser ->
     source_id winner <> source_id loser ->
     active (reject_occurrence [] loser) winner).
Proof.
  exact (conj rejection_is_source_exact
          (conj distinct_source_survives_rejection
            (conj rejection_order_independent one_winner_preserved))).
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

(* A9 exact-integer fault-tolerance DECISION (the f32 -> exact hardening). The exact
   test `2q·den ≥ S(den+num)` the node evaluates over i128 is bit-for-bit the rational
   test `(2q−S)/S ≥ num/den` cleared of its positive denominators, monotone in the
   clique weight (given den ≥ 0), and overflow-free in i128 for i64-bounded stake.
   The overflow envelope now covers the FULL validated ppm range num ∈ [-den, den]
   (G2 widening in FtExact.ft_exact_no_overflow), matching the runtime range-check
   and the negative-θ sentinels, not merely [0, den]. *)
Theorem finalized_floor_ftexact_correct :
  (forall q S num den : Z, ft_exact_ge q S num den <-> ft_ratio_ge q S num den)
  /\ (forall q S num den : Z, ft_exact_gt q S num den <-> ft_ratio_gt q S num den)
  /\ (forall q q' S num den : Z,
        0 <= den -> q <= q' -> ft_exact_ge q S num den -> ft_exact_ge q' S num den)
  /\ (forall q S num den : Z,
        0 <= q <= S -> 0 <= S <= 2^63 -> -den <= num <= den -> den = 1000000 ->
        Z.abs (2*q*den) < 2^127 /\ Z.abs (S*(den+num)) < 2^127).
Proof.
  exact (conj ft_exact_iff_ratio
          (conj ft_exact_iff_ratio_strict
            (conj ft_exact_mono_q ft_exact_no_overflow))).
Qed.

Close Scope Z_scope.

(* ===========================================================================
   G2 capstone — θ_ppm PROVENANCE determinism + the widened i128 overflow envelope.

   Strengthens the A9 (ftexact) capstone at its one un-modelled seam: the SOURCING
   of the threshold numerator θ_ppm. A9 proves the decision is exact GIVEN θ_ppm;
   this proves θ_ppm is a pure function of the on-chain value (the unconditional
   override at casper.rs:266), so local config cannot drive a fork,
   AND that the exact decision is i128-overflow-free across the node's FULL
   validated ppm range num ∈ [-den, den] (the token_metadata_check.rs:105 range,
   including the negative-θ sentinels) — not just the narrower [0, den].

   Each conjunct is discharged by `exact` against its FtProvenance lemma, so the
   capstone adds NO assumptions (verify with `Print Assumptions
   finalized_floor_ftprovenance_correct`).
   =========================================================================== *)
Open Scope Z_scope.

Theorem finalized_floor_ftprovenance_correct :
  (* G2 / provenance: the θ_ppm a node finalizes with is the on-chain value,
     independent of local config (the unconditional override), so two nodes on the
     same genesis agree on θ_ppm regardless of local config — not a fork input. *)
  (forall local onchain : Z, reconcile local onchain = onchain)
  /\
  (* G2 / provenance (agreement form): agreeing on-chain ppm forces agreeing
     reconciled ppm, for ANY local configs local, local'. *)
  (forall local local' onchain : Z, reconcile local onchain = reconcile local' onchain)
  /\
  (* G2 / widened envelope: the exact decision is i128-overflow-free over the FULL
     validated ppm range num ∈ [-den, den] (not merely [0, den]). *)
  (forall q S num den : Z,
     0 <= q <= S -> 0 <= S <= 2^63 -> -den <= num <= den -> den = 1000000 ->
     Z.abs (2*q*den) < 2^127 /\ Z.abs (S*(den+num)) < 2^127).
Proof.
  exact (conj reconcile_is_onchain
          (conj reconcile_agrees_on_onchain ppm_range_decision_no_overflow)).
Qed.

Close Scope Z_scope.

(* ===========================================================================
   Phase 7 capstone — the strengthened selection conjuncts (Case-B compatibility
   + selection maximality). The guard⇒AdjDC bridge and the frontier-is-finalized
   result live in GuardBridge.v (guard_constant_committee_transparent,
   upgo_finalized); they are checked axiom-free by the gate.
   =========================================================================== *)

Theorem finalized_floor_phase7_correct :
  (* Case-B compatibility: the precise guarantee of the all_compatible branch —
     every other candidate is `c`, in `c`'s past, or mergeable via a common
     descendant parent (no incompatible finalized fork). *)
  (forall d fuel parents cands c,
     case_b d fuel parents cands c = true ->
     forall o, In o cands ->
       o = c \/ anc_of d o c
       \/ (exists p, In p parents /\ anc_of d o p /\ anc_of d c p))
  /\
  (* Selection maximality: on descending-sorted candidates the chosen floor is the
     sound base of greatest block number — the canonical highest sound base. *)
  (forall d fuel parents cands f,
     DescSorted d cands ->
     select_floor d fuel parents cands = Some f ->
     is_sound d fuel parents cands f = true /\
     (forall c, In c cands -> is_sound d fuel parents cands c = true ->
        numof d c <= numof d f)).
Proof.
  exact (conj case_b_compatible select_highest_sound).
Qed.

(* ===========================================================================
   C1 + C5 capstone — the θ-exact finalization test and snapshot advancement.

   Strengthens the two "assumed/proxy" seams the earlier capstones rested on:

     C1  The node's REAL fault-tolerance decision is the exact-integer test
         `Finalized_ft` (2q·den ≥ S(den+num), θ = num/den), not merely strict
         majority. It is ancestor- and snapshot-monotone (L-ANC/L-SNAP for the
         exact test) and REFINES the strict-majority `Finalized` proxy for
         θ ∈ (0,1) over a positive-stake committee — so every θ-finalized block
         inherits T-CACHE (frontier_cache_transparent) and every capstone above.

     C5  Finalization is monotone under snapshot ADVANCEMENT (a validator's latest
         message moving forward to a DAG-descendant), which GENERALIZES the
         preservation-only L-SNAP: `snap_extends ⇒ snap_advances`, so the original
         L-SNAP is the reflexive-descendant corollary.

     C1' The num>0 refinement is VACUOUS at the DEFAULT θ = 0 and the negative-θ
         sentinels. The node's REAL decision additionally applies a θ-INDEPENDENT
         hard majority gate (clique_oracle.rs:79-81, `2·agreeing > S`), modelled as
         `Finalized_ft_hg`; the gate alone yields strict-majority `Finalized` for
         ALL num (`Finalized_ft_hg_refines_Finalized`), so θ ≤ 0 is covered too.
         (Cache transparency at θ ≤ 0 is independently secured by
         GuardBridge.BridgeFt over `Finalized_ft` directly, via `L_ANC_ft`.)

   Each conjunct is discharged by `exact` against its CliqueOracle lemma, so this
   capstone introduces NO new assumptions (verify with `Print Assumptions
   finalized_floor_thetaexact_advance_correct`). The pre-existing five capstones
   are unchanged; this only ADDS coverage of the real node test and the faithful
   advancement model. The num>0 refinement side-condition `0 < cweight c` (positive
   committee stake) is faithful and necessary — see the NOTE in CliqueOracle.v
   Section 7 (a zero-stake committee finalizes nothing); the C1' hard-gate
   refinement carries NO such side-condition (it holds for all num, all committees).
   =========================================================================== *)
Theorem finalized_floor_thetaexact_advance_correct :
  (* C1 / L-ANC: θ-exact finalization is downward-closed along ancestry. *)
  (forall d c J b b' num den,
     anc_of d b' b -> Finalized_ft d c J b num den -> Finalized_ft d c J b' num den)
  /\
  (* C1 / L-SNAP: θ-exact finalization is monotone under snapshot growth. *)
  (forall d c J J' b num den,
     snap_extends J' J -> Finalized_ft d c J b num den -> Finalized_ft d c J' b num den)
  /\
  (* C1 / refinement (θ > 0 ONLY): the θ-test (θ = num/den, num,den > 0, positive
     committee stake) implies the strict-majority proxy. θ CAVEAT: this conjunct is
     VACUOUS at the DEFAULT θ = 0 (num = 0 ⇒ the exact test is only the NON-strict
     2q ≥ S) and at the negative-θ sentinels (num < 0); the C1' conjunct below is
     what covers θ ≤ 0. *)
  (forall d c J b num den,
     (0 < num)%Z -> (0 < den)%Z -> 0 < cweight c ->
     Finalized_ft d c J b num den -> Finalized d c J b)
  /\
  (* C5 / advancement: finalization is monotone as latest messages advance to
     DAG-descendants (generalizes the preservation-only L-SNAP). *)
  (forall d c J J' b,
     snap_advances d J' J -> Finalized d c J b -> Finalized d c J' b)
  /\
  (* C5 / generalization: preservation ⇒ advancement, so the existing L-SNAP is
     the reflexive-descendant corollary of L_SNAP_advance. *)
  (forall d J' J, snap_extends J' J -> snap_advances d J' J)
  /\
  (* C1' / θ ≤ 0 COVERAGE: the node's REAL decision is the θ-test AND the
     θ-INDEPENDENT hard majority gate (clique_oracle.rs:79-81, `2·agreeing > S`).
     The hard gate ALONE yields the strict-majority `Finalized` for ALL num —
     including the default θ = 0 and the negative-θ sentinels — so θ-finalization
     refines `Finalized` WITHOUT the `0 < num` restriction the conjunct above
     carries. (Independently, T-CACHE holds directly over `Finalized_ft` for all
     num via GuardBridge.BridgeFt.guard_constant_committee_transparent_ft.) *)
  (forall d c J b num den,
     Finalized_ft_hg d c J b num den -> Finalized d c J b).
Proof.
  exact (conj L_ANC_ft
          (conj L_SNAP_ft
            (conj Finalized_ft_refines_Finalized
              (conj L_SNAP_advance
                (conj snap_extends_snap_advances Finalized_ft_hg_refines_Finalized))))).
Qed.
