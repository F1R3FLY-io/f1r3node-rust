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
                                        whose fold is permutation-invariant
                                        (documented context: the DEFERRED-cancel
                                        operator giving genuine order-independence);
       (d) channel_netting_deployed_  - THE DEPLOYED operator's determinism: the
           deterministic                 shipped combine_max is a semilattice JOIN
                                        (vunion) with a DEFERRED single cancel, and
                                        its fold over a CANONICAL order (KeepOneOrder
                                        DeployChainIndex::cmp / compare_branches) is
                                        node-identical — the actual no-fork property.

     merge_algebra_conflict_correct   (GAP-3 - sound conflict-detector removal +
                                       §3c produce-only over-fill guard)
       PART A: the removed single-value-cell predicate is SUBSUMED by the retained
         double-consume race detector, except on intrinsically-mergeable number
         channels (conflict_removal_sound).
       PART B: the §3c guard closes the produce-only gap Part A's model cannot see
         (a produce that does NOT consume the base over-fills a single-value NUMBER
         cell): svc_guard_catches_overfill (the guard fires on every over-fill),
         overfill_not_retained (it is NOT redundant with the retained detector),
         and svc_invariant_iff_both_detectors (the two detectors together are
         exactly complete). Extends this capstone into a CONJUNCTION; still ONE
         Theorem (the gate requires exactly 4 axiom-free capstones).

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
     fold, and cancel_common preserves the netted multiplicity (documented context:
     the DEFERRED-cancel operator that would give genuine order-independence) *)
  ((forall x y, combine_sum x y = combine_sum y x)
   /\ (forall x y z, combine_sum x (combine_sum y z) = combine_sum (combine_sum x y) z)
   /\ (forall x, combine_sum empty_cc x = x)
   /\ (forall l1 l2, Permutation l1 l2 -> netting_fold l1 = netting_fold l2)
   /\ (forall c, net (cancel c) = net c))
  /\
  (* (d) THE DEPLOYED operator's determinism (the shipped no-fork guarantee, the
     property this capstone now points at). The shipped `combine_max` is a
     SEMILATTICE JOIN with a DEFERRED single cancel (combine_max = cancel ∘ vunion);
     the max-union `vunion` join is idempotent/commutative/ASSOCIATIVE with an
     order-independent fold; and folding the (non-associative) `combine_max` over a
     CANONICAL order (compare_branches / DeployChainIndex::cmp, KeepOneOrder) is a
     function of the input SET — node-identical, no fork. Finding A (b) is the
     inline-vs-deferred cancel gap, rendered BENIGN by the canonical order. *)
  ((forall x y, combine_max x y = cancel (vunion x y))
   /\ (forall x y, vunion x y = vunion y x)
   /\ (forall x y z, vunion x (vunion y z) = vunion (vunion x y) z)
   /\ (forall x, vunion x x = x)
   /\ (forall l1 l2, Permutation l1 l2 -> vunion_fold l1 = vunion_fold l2)
   /\ (forall canon, (forall l l', Permutation l l' -> canon l = canon l') ->
        forall l l', Permutation l l' -> deployed_fold canon l = deployed_fold canon l')
   /\ (forall c, net (cancel c) = net c))
  /\
  (* (e) GENUINE order-independence under the OrderDependenceGuard (the new
     runtime guard in casper/src/rust/merging/conflict_set_merger.rs). On a
     NON-mergeable channel where the guard does NOT trip -- i.e. no datum is
     contributed to a side (added or removed) by >= 2 distinct survivors, so the
     per-datum contribution list has `adds l <= 1 /\ rems l <= 1` -- the shipped
     `combine_max` fold is order-independent over EVERY permutation, STRONGER than
     the canonical-order determinism of (d). Lemma 1: the identity-canon fold
     equals the single DEFERRED cancel of the sum-net; Lemma 2 (the headline):
     hence the fold is permutation-invariant. Finding A (b) is rendered benign
     here not by the canonical order but by the guard's no-duplicate precondition. *)
  ((forall l, adds l <= 1 -> rems l <= 1 ->
       deployed_fold (fun x => x) l = cancel (netting_fold l))
   /\ (forall l l', Permutation l l' -> adds l <= 1 -> rems l <= 1 ->
       deployed_fold (fun x => x) l = deployed_fold (fun x => x) l')).
Proof.
  exact (conj combine_max_comm
          (conj combine_not_assoc_exhibit
            (conj channel_netting_fixed_deterministic
              (conj channel_netting_deployed_deterministic
                (conj deployed_fold_id_eq_cancel_netting
                  combine_max_order_independent_under_no_dup))))).
Qed.

(* ===========================================================================
   Capstone 3 - GAP-3 CONFLICT-DETECTOR REMOVAL SOUNDNESS
   =========================================================================== *)

Theorem merge_algebra_conflict_correct :
  (* PART A - the REMOVED consume-then-produce predicate is subsumed by the
     retained double-consume race detector (Section Conflict / GAP-3). *)
  (forall is_number_ch,
    (* the removed single-value-cell predicate is subsumed by the retained
       double-consume race, except on number channels (intrinsically mergeable) *)
    (forall a b, removed_fires a b ->
       retained_conflict is_number_ch a b \/ is_number_channel is_number_ch a b)
    /\
    (* with the number-channel exemption the code applies, it is FULLY subsumed *)
    (forall a b, removed_fires_exempt is_number_ch a b -> retained_conflict is_number_ch a b))
  /\
  (* PART B - the §3c produce-only over-fill guard (Section Overfill). A produce
     that does NOT consume the base over-fills a single-value NUMBER cell -- a case
     Part A's `svc_update = consumes && produces` model is VACUOUS for. For ANY
     number-classifier / numeric-base / cardinality parameters: (1) the guard fires
     on every over-fill, (2) the over-fill is NOT caught by the retained detector
     (given the explicit consume->removed bridge), so the guard is a genuinely
     separate detector, and (3) the two detectors together are EXACTLY complete. *)
  (forall is_number_ch numeric_base base_card added removed,
    (forall ch,
       svc_overfill is_number_ch numeric_base base_card added removed ch = true ->
       svc_guard_active is_number_ch numeric_base base_card added removed ch = true)
    /\
    (forall a b ch,
       (consumes a ch = true -> consumed_by_merge removed ch = true) ->
       svc_overfill is_number_ch numeric_base base_card added removed ch = true ->
       retained_ch is_number_ch a b ch = false)
    /\
    (forall a b ch,
       svc_broken is_number_ch numeric_base base_card added removed a b ch <->
       svc_flagged is_number_ch numeric_base base_card added removed a b ch = true)).
Proof.
  split.
  - exact conflict_removal_sound.
  - intros is_number_ch numeric_base base_card added removed.
    split; [ | split ].
    + exact (svc_guard_catches_overfill is_number_ch numeric_base base_card added removed).
    + exact (overfill_not_retained is_number_ch numeric_base base_card added removed).
    + exact (svc_invariant_iff_both_detectors is_number_ch numeric_base base_card added removed).
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
