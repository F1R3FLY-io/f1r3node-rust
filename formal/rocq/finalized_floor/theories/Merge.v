(* ===========================================================================
   Merge.v - The merge write-algebra: determinism (no fork) and no-lost-write.

   Once the finalized floor fixes the merge base, the multi-parent merge combines
   the parents' per-channel writes. For the merge to be a sound consensus step it
   must be:

     * DETERMINISTIC (T-DETMERGE / T-CONV): the combined result must not depend
       on the order in which parents are folded, else two nodes folding the same
       parent set in different orders derive different post-states -> a FORK.

     * LOSSLESS on mergeable channels (T-K1, additive/bitmask case): every
       parent's contribution must survive the merge; a merge that silently
       dropped a write is exactly the ~400-block bug's failure mode (safety S5).

   Both mergeable-channel algebras the implementation uses are JOIN-SEMILATTICES
   -- a commutative, associative, idempotent binary operation:

     * BitmaskOr        (rspace++/merging_logic.rs)     : bitwise OR   (Nat.lor)
     * keep-one / max   (single-value cell winner pick) : maximum      (Nat.max)

   For any semilattice, folding over a list is invariant under permutation (hence
   deterministic and convergent) and absorbs every element (hence lossless). We
   prove this once, generically, then instantiate for `Nat.lor` and `Nat.max`.
   (The `IntegerAdd` channel is a wrapping commutative GROUP rather than a
   semilattice; its determinism likewise follows from commutativity+associativity
   -- the permutation lemma below uses only those two, so it covers additive
   channels too; idempotence is only needed for the no-lost-write absorption.)

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                    | Spec / Property          | Rust Implementation
   ------------------------+--------------------------+-------------------------
   merge (fold_right op e) | fold of per-channel diffs| conflict_set_merger.rs
   merge_perm              | T-DETMERGE / T-CONV      | deterministic merge output
   merge_absorbs           | T-K1 (join >= each input)| merging_logic.rs combine
   merge_or_no_lost_bit    | no mergeable write lost  | BitmaskOr channel merge
   merge_max_*             | keep-one winner pick     | single-value-cell merge
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Import Lia.
Import ListNotations.

(* ===========================================================================
   Section 1 - Generic join-semilattice fold
   =========================================================================== *)

Section Semilattice.
  Context {A : Type}.
  Variable op : A -> A -> A.
  Variable e : A.
  Hypothesis op_comm  : forall a b, op a b = op b a.
  Hypothesis op_assoc : forall a b c, op a (op b c) = op (op a b) c.
  Hypothesis op_idem  : forall a, op a a = a.

  Definition merge (l : list A) : A := fold_right op e l.

  Lemma merge_cons : forall x l, merge (x :: l) = op x (merge l).
  Proof. reflexivity. Qed.

  (* T-DETMERGE / T-CONV: the fold is order-independent. Uses only commutativity
     and associativity (so it holds for additive channels too). *)
  Lemma merge_perm : forall l1 l2, Permutation l1 l2 -> merge l1 = merge l2.
  Proof.
    intros l1 l2 H. induction H as [| x l1 l2 Hp IH | x y l | l1 l2 l3 Hp1 IH1 Hp2 IH2].
    - reflexivity.
    - simpl. rewrite IH. reflexivity.
    - (* swap x y: op y (op x m) = op x (op y m) *)
      simpl. rewrite op_assoc. rewrite op_assoc. rewrite (op_comm y x). reflexivity.
    - rewrite IH1, IH2. reflexivity.
  Qed.

  (* T-K1: every element is absorbed by the merge (it lies at or below the join),
     so no input is ever lost. *)
  Lemma merge_absorbs : forall l x, In x l -> op x (merge l) = merge l.
  Proof.
    induction l as [| h t IH]; intros x Hin; simpl in *.
    - contradiction.
    - destruct Hin as [Heq | Hin].
      + subst x. rewrite op_assoc. rewrite op_idem. reflexivity.
      + rewrite op_assoc. rewrite (op_comm x h). rewrite <- op_assoc.
        rewrite (IH x Hin). reflexivity.
  Qed.

End Semilattice.

(* ===========================================================================
   Section 2 - BitmaskOr instance (bitwise OR) : lossless mergeable channel
   =========================================================================== *)

Definition merge_or : list nat -> nat := merge Nat.lor 0.

(* Determinism of the bitmask merge (no fork). *)
Theorem merge_or_perm :
  forall l1 l2, Permutation l1 l2 -> merge_or l1 = merge_or l2.
Proof.
  apply (merge_perm Nat.lor 0 Nat.lor_comm Nat.lor_assoc).
Qed.

(* Absorption of the bitmask merge. *)
Theorem merge_or_absorbs :
  forall l x, In x l -> Nat.lor x (merge_or l) = merge_or l.
Proof.
  apply (merge_absorbs Nat.lor 0 Nat.lor_comm Nat.lor_assoc Nat.lor_diag).
Qed.

(* NO-LOST-WRITE (T-K1 for bitmask channels): every bit set by any input is set
   in the merge. A "write" is a set bit; the merge loses none of them. *)
Theorem merge_or_no_lost_bit :
  forall l x i, In x l -> Nat.testbit x i = true -> Nat.testbit (merge_or l) i = true.
Proof.
  intros l x i Hin Hbit.
  (* merge_or l = Nat.lor x (merge_or l) by absorption; then testbit distributes. *)
  rewrite <- (merge_or_absorbs l x Hin).
  rewrite Nat.lor_spec. rewrite Hbit. reflexivity.
Qed.

(* ===========================================================================
   Section 3 - keep-one instance (max) : deterministic single-value winner pick
   =========================================================================== *)

Definition merge_max : list nat -> nat := merge Nat.max 0.

(* Determinism of the keep-one winner (no fork): the surviving value does not
   depend on parent fold order. *)
Theorem merge_max_perm :
  forall l1 l2, Permutation l1 l2 -> merge_max l1 = merge_max l2.
Proof.
  apply (merge_perm Nat.max 0 Nat.max_comm Nat.max_assoc).
Qed.

(* The kept value dominates every candidate (the winner is >= each write). *)
Theorem merge_max_absorbs :
  forall l x, In x l -> Nat.max x (merge_max l) = merge_max l.
Proof.
  apply (merge_absorbs Nat.max 0 Nat.max_comm Nat.max_assoc Nat.max_id).
Qed.

Theorem merge_max_ge :
  forall l x, In x l -> x <= merge_max l.
Proof.
  intros l x Hin.
  pose proof (merge_max_absorbs l x Hin) as H.
  (* x <= max x (merge_max l) = merge_max l *)
  assert (Hle : x <= Nat.max x (merge_max l)) by apply Nat.le_max_l.
  rewrite H in Hle. exact Hle.
Qed.
