From Stdlib Require Import List Arith Lia.
Import ListNotations.
Set Implicit Arguments.

Section Comparison.
Context {Atom : Type}.
Variable atom_compare : Atom -> Atom -> comparison.

Inductive score :=
| Leaf : Atom -> score
| Branch : scores -> score
with scores :=
| Empty : scores
| Cons : score -> scores -> scores.

Definition first_difference (head tail : comparison) : comparison :=
  match head with Eq => tail | _ => head end.

Fixpoint compare_recursive (lhs rhs : score) {struct lhs} : comparison :=
  match lhs, rhs with
  | Leaf x, Leaf y => atom_compare x y
  | Leaf _, Branch _ => Lt
  | Branch _, Leaf _ => Gt
  | Branch xs, Branch ys => compare_nodes_recursive xs ys
  end
with compare_nodes_recursive (lhs rhs : scores) {struct lhs} : comparison :=
  match lhs, rhs with
  | Empty, Empty => Eq
  | Empty, Cons _ _ => Lt
  | Cons _ _, Empty => Gt
  | Cons x xs, Cons y ys =>
      first_difference (compare_recursive x y) (compare_nodes_recursive xs ys)
  end.

Inductive job :=
| Pair : score -> score -> job
| Nodes : scores -> scores -> job.

Definition job_result (item : job) : comparison :=
  match item with
  | Pair lhs rhs => compare_recursive lhs rhs
  | Nodes lhs rhs => compare_nodes_recursive lhs rhs
  end.

Fixpoint pending_result (work : list job) : comparison :=
  match work with
  | [] => Eq
  | item :: rest => first_difference (job_result item) (pending_result rest)
  end.

Fixpoint score_weight (lhs : score) : nat :=
  match lhs with Leaf _ => 1 | Branch xs => 2 + nodes_weight xs end
with nodes_weight (lhs : scores) : nat :=
  match lhs with Empty => 1 | Cons x xs => 1 + score_weight x + nodes_weight xs end.

Definition job_weight (item : job) : nat :=
  match item with Pair lhs _ => score_weight lhs | Nodes lhs _ => nodes_weight lhs end.

Fixpoint work_weight (work : list job) : nat :=
  match work with [] => 0 | item :: rest => job_weight item + work_weight rest end.

Definition continue_or_return (result : comparison) (rest : list job)
  : comparison + list job :=
  match result with Eq => inr rest | _ => inl result end.

Definition advance (item : job) (rest : list job) : comparison + list job :=
  match item with
  | Pair (Leaf x) (Leaf y) => continue_or_return (atom_compare x y) rest
  | Pair (Leaf _) (Branch _) => inl Lt
  | Pair (Branch _) (Leaf _) => inl Gt
  | Pair (Branch xs) (Branch ys) => inr (Nodes xs ys :: rest)
  | Nodes Empty Empty => inr rest
  | Nodes Empty (Cons _ _) => inl Lt
  | Nodes (Cons _ _) Empty => inl Gt
  | Nodes (Cons x xs) (Cons y ys) => inr (Pair x y :: Nodes xs ys :: rest)
  end.

Lemma first_difference_associative : forall a b c,
  first_difference (first_difference a b) c =
  first_difference a (first_difference b c).
Proof. destruct a; reflexivity. Qed.

Theorem advance_preserves_comparison : forall item rest,
  match advance item rest with
  | inl result => pending_result (item :: rest) = result
  | inr next => pending_result (item :: rest) = pending_result next
  end.
Proof.
  intros [lhs rhs|lhs rhs] rest;
    destruct lhs; destruct rhs; simpl; try reflexivity.
  - unfold continue_or_return. destruct (atom_compare a a0); reflexivity.
  - apply first_difference_associative.
Qed.

Lemma score_weight_positive : forall lhs, 0 < score_weight lhs.
Proof. destruct lhs; simpl; lia. Qed.

Lemma nodes_weight_positive : forall lhs, 0 < nodes_weight lhs.
Proof. destruct lhs; simpl; lia. Qed.

Theorem advance_decreases_weight : forall item rest next,
  advance item rest = inr next -> work_weight next < work_weight (item :: rest).
Proof.
  intros [lhs rhs|lhs rhs] rest next;
    destruct lhs; destruct rhs; simpl; try discriminate.
  - unfold continue_or_return. destruct (atom_compare a a0); intro H;
      inversion H; subst; simpl; lia.
  - intro H. inversion H; subst. simpl. lia.
  - intro H. inversion H; subst. simpl. lia.
  - intro H. inversion H; subst. simpl. lia.
Qed.

Fixpoint compare_machine (fuel : nat) (work : list job) : option comparison :=
  match work with
  | [] => Some Eq
  | item :: rest =>
      match fuel with
      | 0 => None
      | S remaining =>
          match advance item rest with
          | inl result => Some result
          | inr next => compare_machine remaining next
          end
      end
  end.

Theorem machine_matches_pending_recursion : forall fuel work,
  work_weight work <= fuel -> compare_machine fuel work = Some (pending_result work).
Proof.
  induction fuel as [|fuel IH]; intros [|item rest] Hweight; simpl; auto.
  - destruct item; simpl in Hweight.
    + pose proof (score_weight_positive s). lia.
    + pose proof (nodes_weight_positive s). lia.
  - pose proof (advance_preserves_comparison item rest) as Hresult.
    destruct (advance item rest) as [result|next] eqn:Hadvance.
    + change (Some result = Some (pending_result (item :: rest))). now rewrite Hresult.
    + rewrite IH.
      * change (Some (pending_result next) = Some (pending_result (item :: rest))).
        now rewrite Hresult.
      * pose proof (advance_decreases_weight item rest Hadvance). lia.
Qed.

Theorem iterative_score_comparison_equals_recursive : forall lhs rhs,
  compare_machine (score_weight lhs) [Pair lhs rhs] =
  Some (compare_recursive lhs rhs).
Proof.
  intros. rewrite machine_matches_pending_recursion by (simpl; lia).
  simpl. unfold first_difference. destruct (compare_recursive lhs rhs); reflexivity.
Qed.
End Comparison.

Print Assumptions advance_preserves_comparison.
Print Assumptions advance_decreases_weight.
Print Assumptions iterative_score_comparison_equals_recursive.

Example tail_before_head_changes_comparison :
  pending_result Nat.compare
    [Nodes (Cons (Leaf 0) Empty) (Cons (Leaf 1) Empty); Pair (Leaf 1) (Leaf 0)] <>
  compare_nodes_recursive Nat.compare
    (Cons (Leaf 1) (Cons (Leaf 0) Empty))
    (Cons (Leaf 0) (Cons (Leaf 1) Empty)).
Proof. vm_compute. discriminate. Qed.
