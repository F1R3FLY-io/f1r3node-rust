From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia Classes.RelationClasses Sorting.Permutation Sorting.Sorted Sorting.Mergesort.
Import ListNotations.

Module DescendingFundingOrder.
  Definition t := nat.
  Definition leb (left right : nat) := Nat.leb right left.
  Theorem leb_total : forall left right, leb left right = true \/ leb right left = true.
  Proof. intros. unfold leb. rewrite !Nat.leb_le. lia. Qed.
End DescendingFundingOrder.

Module FundingSort := Mergesort.Sort DescendingFundingOrder.
Definition burden_rank := FundingSort.sort.

Theorem burden_rank_permutation : forall values, Permutation values (burden_rank values).
Proof. apply FundingSort.Permuted_sort. Qed.

Theorem burden_rank_descending : forall values,
  StronglySorted (fun left right : nat => right <= left) (burden_rank values).
Proof.
  intro values. unfold burden_rank.
  assert (transitive : Transitive (fun left right => DescendingFundingOrder.leb left right = true)).
  { intros x y z. unfold DescendingFundingOrder.leb. rewrite !Nat.leb_le. lia. }
  pose proof (FundingSort.StronglySorted_sort values transitive) as ordered.
  induction ordered; constructor; auto.
  eapply Forall_impl; [|exact H]. intros. now apply Nat.leb_le.
Qed.

Lemma descending_permutation_unique : forall left right,
  StronglySorted (fun x y : nat => y <= x) left ->
  StronglySorted (fun x y : nat => y <= x) right ->
  Permutation left right -> left = right.
Proof.
  intros left right ordered. revert right.
  induction ordered as [|x xs tail_order IH tail_bound]; intros right right_order same.
  - symmetry. now apply Permutation_nil.
  - destruct right as [|y ys]; [apply Permutation_length in same; simpl in same; lia|].
    inversion right_order as [|? ? ys_order ys_bound]; subst.
    assert (x_in : In x (y :: ys)) by (eapply Permutation_in; [exact same|simpl; auto]).
    assert (y_in : In y (x :: xs)) by (eapply Permutation_in; [apply Permutation_sym; exact same|simpl; auto]).
    assert (equal : x = y).
    { simpl in x_in, y_in. rewrite Forall_forall in tail_bound, ys_bound.
      destruct x_in as [same_head|x_in]; [lia|].
      destruct y_in as [same_head|y_in]; [lia|].
      specialize (tail_bound y y_in). specialize (ys_bound x x_in). lia. }
    subst y. f_equal. apply IH; [exact ys_order|]. now apply Permutation_cons_inv in same.
Qed.

Theorem burden_rank_ignores_source_permutation : forall left right,
  Permutation left right -> burden_rank left = burden_rank right.
Proof.
  intros left right same. apply descending_permutation_unique;
    try apply burden_rank_descending.
  eapply Permutation_trans; [apply Permutation_sym; apply burden_rank_permutation|].
  eapply Permutation_trans; [exact same|apply burden_rank_permutation].
Qed.

Theorem burden_rank_preserves_source_count : forall values,
  length (burden_rank values) = length values.
Proof. intros. symmetry. apply Permutation_length. apply burden_rank_permutation. Qed.

Fixpoint funding_lex_le (left right : list nat) : Prop :=
  match left, right with
  | [], _ => True
  | _ :: _, [] => False
  | x :: xs, y :: ys => x < y \/ (x = y /\ funding_lex_le xs ys)
  end.

Fixpoint funding_lex_leb (left right : list nat) : bool :=
  match left, right with
  | [], _ => true
  | _ :: _, [] => false
  | x :: xs, y :: ys => Nat.ltb x y || (Nat.eqb x y && funding_lex_leb xs ys)
  end.

Theorem funding_lex_check_exact : forall left right,
  funding_lex_leb left right = true <-> funding_lex_le left right.
Proof.
  induction left; intros [|y ys]; simpl.
  - tauto.
  - tauto.
  - split; [discriminate|contradiction].
  - rewrite orb_true_iff, andb_true_iff, Nat.ltb_lt, Nat.eqb_eq, IHleft. tauto.
Qed.

Theorem funding_lex_reflexive : forall values, funding_lex_le values values.
Proof. induction values; simpl; auto. Qed.

Theorem funding_lex_total : forall left right,
  funding_lex_le left right \/ funding_lex_le right left.
Proof.
  induction left as [|x xs IH]; intros [|y ys]; simpl; auto.
  destruct (Nat.lt_trichotomy x y) as [less|[equal|greater]]; auto.
  subst y. destruct (IH ys); auto.
Qed.

Theorem funding_lex_transitive : forall left middle right,
  funding_lex_le left middle -> funding_lex_le middle right -> funding_lex_le left right.
Proof.
  induction left as [|x xs IH]; intros [|y ys] [|z zs]; simpl; try tauto.
  intros [xy|[xy tail_xy]] [yz|[yz tail_yz]]; try (left; lia).
  right. split; [lia|eapply IH; eassumption].
Qed.

Theorem funding_lex_antisymmetric : forall left right,
  funding_lex_le left right -> funding_lex_le right left -> left = right.
Proof.
  induction left as [|x xs IH]; intros [|y ys]; simpl; try tauto.
  intros [xy|[xy tail_xy]] [yx|[yx tail_yx]]; try lia.
  subst y. f_equal. now apply IH.
Qed.

Definition burden_le left right := funding_lex_le (burden_rank left) (burden_rank right).
Definition burden_leb left right := funding_lex_leb (burden_rank left) (burden_rank right).

Definition choose_burden (left right : list nat) : list nat :=
  if burden_leb left right then left else right.

Theorem choose_burden_member : forall left right,
  choose_burden left right = left \/ choose_burden left right = right.
Proof. intros. unfold choose_burden. destruct (burden_leb left right); auto. Qed.

Theorem choose_burden_least : forall left right,
  burden_le (choose_burden left right) left /\ burden_le (choose_burden left right) right.
Proof.
  intros left right. unfold choose_burden, burden_leb, burden_le.
  destruct (funding_lex_leb (burden_rank left) (burden_rank right)) eqn:checked.
  - apply funding_lex_check_exact in checked. split; auto using funding_lex_reflexive.
  - split; [|apply funding_lex_reflexive].
    destruct (funding_lex_total (burden_rank left) (burden_rank right)); auto.
    apply funding_lex_check_exact in H. congruence.
Qed.

Fixpoint least_burden (seed : list nat) (candidates : list (list nat)) : list nat :=
  match candidates with
  | [] => seed
  | next :: rest => choose_burden next (least_burden seed rest)
  end.

Theorem finite_minimax_is_a_supplied_candidate : forall candidates seed,
  In (least_burden seed candidates) (seed :: candidates).
Proof.
  induction candidates as [|next rest IH]; intros seed; simpl; auto.
  destruct (choose_burden_member next (least_burden seed rest)) as [selected|selected]; rewrite selected; auto.
  specialize (IH seed). simpl in IH. tauto.
Qed.

Theorem finite_minimax_is_optimal : forall candidates seed candidate,
  In candidate (seed :: candidates) -> burden_le (least_burden seed candidates) candidate.
Proof.
  induction candidates as [|next rest IH]; intros seed candidate included; simpl in *.
  - destruct included as [->|[]]. apply funding_lex_reflexive.
  - destruct (choose_burden_least next (least_burden seed rest)) as [next_bound rest_bound].
    destruct included as [same|[same|included]].
    + subst candidate. eapply funding_lex_transitive; [exact rest_bound|]. apply IH. simpl; auto.
    + subst candidate. exact next_bound.
    + eapply funding_lex_transitive; [exact rest_bound|]. apply IH. simpl; auto.
Qed.

Theorem equal_optima_have_identical_rank : forall left right,
  burden_le left right -> burden_le right left -> burden_rank left = burden_rank right.
Proof. intros. now apply funding_lex_antisymmetric. Qed.

Theorem candidate_order_preserves_optimal_rank : forall seed left right,
  Permutation left right ->
  burden_rank (least_burden seed left) = burden_rank (least_burden seed right).
Proof.
  intros seed left right same. apply equal_optima_have_identical_rank;
    apply finite_minimax_is_optimal.
  - eapply Permutation_in; [apply perm_skip; apply Permutation_sym; exact same|].
    apply finite_minimax_is_a_supplied_candidate.
  - eapply Permutation_in; [apply perm_skip; exact same|].
    apply finite_minimax_is_a_supplied_candidate.
Qed.

Theorem complete_candidate_cover_establishes_global_optimality : forall feasible seed candidates,
  feasible seed -> (forall candidate, In candidate candidates -> feasible candidate) ->
  (forall candidate, feasible candidate -> In candidate (seed :: candidates)) ->
  feasible (least_burden seed candidates) /\
  forall candidate, feasible candidate -> burden_le (least_burden seed candidates) candidate.
Proof.
  intros feasible seed candidates seed_valid candidates_valid complete. split.
  - pose proof (finite_minimax_is_a_supplied_candidate candidates seed) as included.
    destruct included as [same|included]; [now rewrite <- same|now apply candidates_valid].
  - intros. apply finite_minimax_is_optimal. now apply complete.
Qed.

Example minimax_is_not_leximin_contribution :
  burden_leb [0; 5; 5] [1; 1; 8] = true /\ burden_leb [1; 1; 8] [0; 5; 5] = false.
Proof. vm_compute. auto. Qed.

Example minimizing_only_the_maximum_is_insufficient :
  burden_leb [5; 3; 2] [5; 4; 1] = true /\ burden_leb [5; 4; 1] [5; 3; 2] = false.
Proof. vm_compute. auto. Qed.

Example equal_rank_does_not_choose_a_purse :
  burden_rank [3; 3; 2] = burden_rank [2; 3; 3] /\ [3; 3; 2] <> [2; 3; 3].
Proof. vm_compute. split; [reflexivity|discriminate]. Qed.

Example incomplete_candidates_cannot_establish_global_optimality :
  least_burden [1; 1; 8] [] = [1; 1; 8] /\ burden_leb [1; 1; 8] [0; 5; 5] = false.
Proof. vm_compute. auto. Qed.

Definition minimax_samples : list (list nat) :=
  flat_map (fun x => flat_map (fun y => map (fun z => [x; y; z]) (seq 0 5)) (seq 0 5)) (seq 0 5).

Example generated_minimax_order_regression :
  forallb (fun left => forallb (fun right =>
    (burden_leb left right || burden_leb right left) &&
    (burden_leb (choose_burden left right) left && burden_leb (choose_burden left right) right))
    minimax_samples) minimax_samples = true.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions burden_rank_descending.
Print Assumptions burden_rank_ignores_source_permutation.
Print Assumptions funding_lex_check_exact.
Print Assumptions funding_lex_transitive.
Print Assumptions finite_minimax_is_a_supplied_candidate.
Print Assumptions finite_minimax_is_optimal.
Print Assumptions equal_optima_have_identical_rank.
Print Assumptions candidate_order_preserves_optimal_rank.
Print Assumptions complete_candidate_cover_establishes_global_optimality.
