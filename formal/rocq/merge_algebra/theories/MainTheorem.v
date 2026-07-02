(* ===========================================================================
   MainTheorem.v - Capstone: the F1R3FLY block merger is DETERMINISTIC.

   Four end-to-end statements, each `exact`-discharged against already-proven,
   axiom-free module lemmas, so the capstones introduce NO new assumptions
   (verify with `Print Assumptions` - all "Closed under the global context").
   The four axes and what each rules out (all are no-fork / merge-soundness):

     merge_algebra_keeporder_correct  (P3 - node-identical merge winner)
       (a) keep_one_total_order       - the 5-key comparator is a STRICT TOTAL
                                        order (trichotomy), UNCONDITIONAL (no
                                        NoDup / collision premise, unlike
                                        fork_choice/TieBreak.v);
       (b) keep_one_equal_impl_eq     - cmp a b = Eq -> a = b (the Equal-class is
                                        exactly the injective terminal key);
       (c) output_indep_of_input_perm - permuted merge inputs sort IDENTICALLY;
       (d) sort_argmax_unique         - the head is the unique min_by winner.

     merge_algebra_netting_correct    (GAP-1 - deterministic channel netting)
       (a) combine_max_comm           - the shipped combine is commutative;
       (b) combine_not_assoc_exhibit  - but NON-associative (Finding A, exhibited);
       (c) channel_netting_fixed_...  - the sum-union FIX is a commutative monoid
                                        whose fold is permutation-invariant.

     merge_algebra_conflict_correct   (GAP-3 - sound conflict-detector removal)
       the removed single-value-cell predicate is SUBSUMED by the retained
       double-consume race detector, except on intrinsically-mergeable number
       channels (conflict_removal_sound).

     merge_algebra_split_correct      (P2 - the user/system split hides no conflict)
       combine(fold user, fold system) = fold all, so conflict detection on the
       split index equals detection on the monolithic index (event_log_split_sound).
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Import ZArith.
Import ListNotations.

From MergeAlgebra Require Import KeepOneOrder.
From MergeAlgebra Require Import ChannelNetting.
From MergeAlgebra Require Import ConflictSoundness.
From MergeAlgebra Require Import EventLogSplit.

(* ===========================================================================
   Capstone 1 - P3 KEEP-ONE ORDER (node-identical merge winner)
   =========================================================================== *)

Theorem merge_algebra_keeporder_correct :
  (* (a) the 5-key comparator is a STRICT TOTAL order (trichotomy), UNCONDITIONAL *)
  (forall g1 g2 g3 g4,
     (forall a, ~ ord g1 g2 g3 g4 a a)
     /\ (forall a b c, ord g1 g2 g3 g4 a b -> ord g1 g2 g3 g4 b c -> ord g1 g2 g3 g4 a c)
     /\ (forall a b, ord g1 g2 g3 g4 a b \/ a = b \/ ord g1 g2 g3 g4 b a))
  /\
  (* (b) the Equal-class is exactly the injective key: cmp a b = Eq -> a = b *)
  (forall g1 g2 g3 g4 a b, cmp g1 g2 g3 g4 a b = Eq -> a = b)
  /\
  (* (c) permuted merge inputs sort to the IDENTICAL list (no fork) *)
  (forall g1 g2 g3 g4 l l', Permutation l l' -> sort g1 g2 g3 g4 l = sort g1 g2 g3 g4 l')
  /\
  (* (d) the head is the unique argmax winner (the min_by result) *)
  (forall g1 g2 g3 g4 l,
     match sort g1 g2 g3 g4 l with
     | [] => l = []
     | w :: _ => In w l /\ forall e, In e l -> e = w \/ ord g1 g2 g3 g4 w e
     end).
Proof.
  exact (conj keep_one_total_order
          (conj keep_one_equal_impl_eq
            (conj output_indep_of_input_perm sort_argmax_unique))).
Qed.

(* ===========================================================================
   Capstone 2 - GAP-1 CHANNEL NETTING (deterministic per-channel combine)
   =========================================================================== *)

Theorem merge_algebra_netting_correct :
  (* (a) the shipped max-union combine is COMMUTATIVE *)
  (forall x y, combine_max x y = combine_max y x)
  /\
  (* (b) but NON-associative -- Finding A, exhibited as a theorem *)
  (combine_max (combine_max mk_add mk_add) mk_rem = empty_cc
   /\ combine_max mk_add (combine_max mk_add mk_rem) = mk_add
   /\ empty_cc <> mk_add)
  /\
  (* (c) the sum-union FIX is a commutative monoid with a permutation-invariant
     fold, and cancel_common preserves the netted multiplicity *)
  ((forall x y, combine_sum x y = combine_sum y x)
   /\ (forall x y z, combine_sum x (combine_sum y z) = combine_sum (combine_sum x y) z)
   /\ (forall x, combine_sum empty_cc x = x)
   /\ (forall l1 l2, Permutation l1 l2 -> netting_fold l1 = netting_fold l2)
   /\ (forall c, net (cancel c) = net c)).
Proof.
  exact (conj combine_max_comm
          (conj combine_not_assoc_exhibit
            channel_netting_fixed_deterministic)).
Qed.

(* ===========================================================================
   Capstone 3 - GAP-3 CONFLICT-DETECTOR REMOVAL SOUNDNESS
   =========================================================================== *)

Theorem merge_algebra_conflict_correct :
  forall is_number_ch,
    (* the removed single-value-cell predicate is subsumed by the retained
       double-consume race, except on number channels (intrinsically mergeable) *)
    (forall a b, removed_fires a b ->
       retained_conflict is_number_ch a b \/ is_number_channel is_number_ch a b)
    /\
    (* with the number-channel exemption the code applies, it is FULLY subsumed *)
    (forall a b, removed_fires_exempt is_number_ch a b -> retained_conflict is_number_ch a b).
Proof.
  exact conflict_removal_sound.
Qed.

(* ===========================================================================
   Capstone 4 - P2 EVENT-LOG SPLIT SOUNDNESS
   =========================================================================== *)

Theorem merge_algebra_split_correct :
  (* combine(fold user, fold system) = fold all *)
  (forall p l, combine_split p l = foldi l)
  /\
  (* so conflict detection on the split index = detection on the monolithic index *)
  (forall (conflicts : Idx -> Idx -> bool) p l other,
     conflicts (combine_split p l) other = conflicts (foldi l) other).
Proof.
  exact event_log_split_sound.
Qed.
