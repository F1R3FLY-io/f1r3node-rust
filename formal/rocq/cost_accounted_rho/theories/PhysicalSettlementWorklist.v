From Stdlib Require Import List Arith.PeanoNat Lia.

Import ListNotations.

Inductive search_tree (A : Type) : Type :=
| SearchFailure
| SearchSuccess (value : A)
| SearchChoice (left_tree right_tree : search_tree A).

Arguments SearchFailure {A}.
Arguments SearchSuccess {A} _.
Arguments SearchChoice {A} _ _.

Fixpoint recursive_solutions {A : Type} (tree : search_tree A) : list A :=
  match tree with
  | SearchFailure => []
  | SearchSuccess value => [value]
  | SearchChoice left_tree right_tree =>
      recursive_solutions left_tree ++ recursive_solutions right_tree
  end.

Fixpoint search_tree_size {A : Type} (tree : search_tree A) : nat :=
  match tree with
  | SearchFailure | SearchSuccess _ => 1
  | SearchChoice left_tree right_tree =>
      1 + search_tree_size left_tree + search_tree_size right_tree
  end.

Fixpoint search_forest_size {A : Type} (forest : list (search_tree A)) : nat :=
  match forest with
  | [] => 0
  | tree :: rest => search_tree_size tree + search_forest_size rest
  end.

Fixpoint recursive_forest_solutions {A : Type}
  (forest : list (search_tree A)) : list A :=
  match forest with
  | [] => []
  | tree :: rest =>
      recursive_solutions tree ++ recursive_forest_solutions rest
  end.

Fixpoint worklist_solutions {A : Type}
  (fuel : nat)
  (work : list (search_tree A)) : list A :=
  match fuel with
  | 0 => []
  | S remaining_fuel =>
      match work with
      | [] => []
      | SearchFailure :: rest =>
          worklist_solutions remaining_fuel rest
      | SearchSuccess value :: rest =>
          value :: worklist_solutions remaining_fuel rest
      | SearchChoice left_tree right_tree :: rest =>
          worklist_solutions remaining_fuel (left_tree :: right_tree :: rest)
      end
  end.

Theorem worklist_solutions_refine_recursive_forest :
  forall (A : Type) (fuel : nat) (forest : list (search_tree A)),
    search_forest_size forest = fuel ->
    worklist_solutions fuel forest = recursive_forest_solutions forest.
Proof.
  intros A fuel.
  induction fuel as [|fuel IH]; intros forest Hsize.
  - destruct forest as [|tree rest].
    + reflexivity.
    + destruct tree; simpl in Hsize; lia.
  - destruct forest as [|tree rest].
    + simpl in Hsize; lia.
    + destruct tree as [|value|left_tree right_tree].
      * simpl in Hsize |- *.
        apply IH.
        lia.
      * simpl in Hsize |- *.
        f_equal.
        apply IH.
        lia.
      * simpl in Hsize |- *.
        rewrite IH.
        -- cbn.
           now rewrite app_assoc.
        -- cbn.
           lia.
Qed.

Theorem worklist_solutions_refine_recursive :
  forall (A : Type) (tree : search_tree A),
    worklist_solutions (search_tree_size tree) [tree] =
    recursive_solutions tree.
Proof.
  intros A tree.
  pose proof
    (worklist_solutions_refine_recursive_forest
      A (search_tree_size tree) [tree]) as Hrefine.
  assert (Hsize : search_forest_size [tree] = search_tree_size tree).
  { simpl. lia. }
  specialize (Hrefine Hsize).
  simpl in Hrefine.
  now rewrite app_nil_r in Hrefine.
Qed.

Definition recursive_first {A : Type} (tree : search_tree A) : option A :=
  hd_error (recursive_solutions tree).

Definition worklist_first {A : Type} (tree : search_tree A) : option A :=
  hd_error (worklist_solutions (search_tree_size tree) [tree]).

Theorem worklist_first_preserves_canonical_candidate_order :
  forall (A : Type) (tree : search_tree A),
    worklist_first tree = recursive_first tree.
Proof.
  intros A tree.
  unfold worklist_first, recursive_first.
  now rewrite worklist_solutions_refine_recursive.
Qed.

Theorem worklist_success_is_recursive_success :
  forall (A : Type) (tree : search_tree A) (value : A),
    worklist_first tree = Some value ->
    recursive_first tree = Some value.
Proof.
  intros A tree value Hworklist.
  rewrite worklist_first_preserves_canonical_candidate_order in Hworklist.
  exact Hworklist.
Qed.

Theorem worklist_failure_is_recursive_failure :
  forall (A : Type) (tree : search_tree A),
    worklist_first tree = None ->
    recursive_first tree = None.
Proof.
  intros A tree Hworklist.
  rewrite worklist_first_preserves_canonical_candidate_order in Hworklist.
  exact Hworklist.
Qed.

Print Assumptions worklist_solutions_refine_recursive.
Print Assumptions worklist_first_preserves_canonical_candidate_order.
Print Assumptions worklist_success_is_recursive_success.
Print Assumptions worklist_failure_is_recursive_failure.
