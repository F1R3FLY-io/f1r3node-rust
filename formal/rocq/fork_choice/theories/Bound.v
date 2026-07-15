(* ===========================================================================
   Bound.v - Parent-count truncation (B2) keeps the ghost main parent; the
   empty-tips case is a typed error (P2-8).

   After ranking, the estimator caps the number of parents (estimator.rs:120):

       if max_number_of_parents < 0 || == UNLIMITED_PARENTS (i32::MAX)
         then take ALL ranked tips
         else take(max_number_of_parents)

   The B2 fix treats BOTH "unlimited" sentinels EXPLICITLY (no `-1 as usize`
   wraparound). Whatever the cap, tips[0] - the GHOST main parent (the unique
   argmax from Rank/TieBreak) - MUST survive, else the block's main parent would
   change under a cap => a different chain. And `filter_deep_parents` on an EMPTY
   ranked list is a typed error, never a panic (P2-8, estimator.rs:156).

   Proved:
     * `head_preserved`      - a positive/unlimited cap keeps the head (tips[0]);
     * `take_never_drops_head`- firstn (S k) never drops the head;
     * `cast_usize_safe`     - the two unlimited sentinels take ALL; a positive
                               cap truncates (the B2 cast-safety, no wraparound);
     * `empty_tips_typed_err`- capping [] yields [] and flags the P2-8 error.

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                    | Rust (casper/src/rust/estimator.rs)
   ------------------------+-----------------------------------------------------
   cap_tips                | tips = if unlimited then all else take(n) (:120-129)
   head_preserved          | tips[0] (ghost main parent) survives the cap
   cast_usize_safe         | B2 explicit sentinel handling (no -1 as usize)
   empty_tips_typed_err    | filter_deep_parents empty -> typed Err (:156-162)
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
Import ListNotations.

From ForkChoice Require Import Foundation.
From ForkChoice Require Import Rank.

(* The parent-count cap: all tips when `unlimited` (either sentinel), else the
   first `n`. `firstn` = the Rust `.take(n)`. *)
Definition cap_tips {A : Type} (n : nat) (unlimited : bool) (l : list A) : list A :=
  if unlimited then l else firstn n l.

(* firstn never drops the head of a nonempty list (the base fact behind B2). *)
Theorem take_never_drops_head :
  forall (A : Type) (k : nat) (x : A) (xs : list A),
    firstn (S k) (x :: xs) = x :: firstn k xs.
Proof. intros; reflexivity. Qed.

(* The ghost main parent (head) survives any admissible cap: unlimited, or a
   positive count (n >= 1). *)
Theorem head_preserved :
  forall (A : Type) (n : nat) (unlimited : bool) (l : list A),
    (unlimited = true \/ 1 <= n) ->
    hd_error (cap_tips n unlimited l) = hd_error l.
Proof.
  intros A n unlimited l Hadm. unfold cap_tips.
  destruct unlimited eqn:U.
  - reflexivity.
  - destruct Hadm as [Habs | Hn]; [discriminate Habs |].
    destruct l as [| x xs].
    + rewrite firstn_nil. reflexivity.
    + destruct n as [| k]; [inversion Hn |].
      rewrite take_never_drops_head. reflexivity.
Qed.

(* B2 cast-safety: the two unlimited sentinels take ALL tips; a genuine positive
   cap truncates via firstn - the two conventions are never conflated. *)
Theorem cast_usize_safe :
  forall (A : Type) (n : nat) (unlimited : bool) (l : list A),
    (unlimited = true -> cap_tips n unlimited l = l)
    /\ (unlimited = false -> cap_tips n unlimited l = firstn n l).
Proof.
  intros A n unlimited l. split; intro H; unfold cap_tips; rewrite H; reflexivity.
Qed.

(* P2-8: capping an empty ranked-tip list yields [], flagged as the typed error
   (`filter_deep_parents` on empty input -> KvStoreError::InvalidArgument). *)
Definition tips_error {A : Type} (l : list A) : bool :=
  match l with [] => true | _ => false end.

Theorem empty_tips_typed_err :
  forall (A : Type) (n : nat) (unlimited : bool),
    cap_tips n unlimited (@nil A) = []
    /\ tips_error (cap_tips n unlimited (@nil A)) = true.
Proof.
  intros A n unlimited. unfold cap_tips.
  destruct unlimited; simpl.
  - split; reflexivity.
  - rewrite firstn_nil. split; reflexivity.
Qed.

(* ===========================================================================
   The composition with Rank: the ghost main parent computed by the fork-choice
   descent (Rank.rank) sits at tips[0] and SURVIVES the cap - so capping cannot
   change the block's main parent (snapshot.rs orders it first, estimator.rs
   truncates after). Exercises the Rank dependency.
   =========================================================================== *)

Corollary ghost_main_parent_survives_cap :
  forall (d : DAG) (score : BlockHash -> nat) (is_scored : BlockHash -> bool)
         (fuel : nat) (h : BlockHash) (n : nat) (unlimited : bool)
         (rest : list BlockHash),
    (unlimited = true \/ 1 <= n) ->
    hd_error (cap_tips n unlimited (rank d score is_scored fuel h :: rest))
      = Some (rank d score is_scored fuel h).
Proof.
  intros d score is_scored fuel h n unlimited rest Hadm.
  rewrite (head_preserved BlockHash n unlimited
             (rank d score is_scored fuel h :: rest) Hadm).
  reflexivity.
Qed.
