From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia Sorting.Permutation Sorting.Sorted.
From CostAccountedRho Require Import EligibleFundingAssignment FundingDeficitCertificate LexicographicMinimax FundingResidualCut.
Import ListNotations.

Fixpoint contribution_excess threshold (values : list nat) :=
  match values with
  | [] => 0
  | value :: rest => (value - threshold) + contribution_excess threshold rest
  end.

Lemma contribution_excess_permutation : forall threshold left right,
  Permutation left right -> contribution_excess threshold left = contribution_excess threshold right.
Proof. intros threshold left right same. induction same; simpl; lia. Qed.

Lemma contribution_excess_zero : forall threshold values,
  Forall (fun value => value <= threshold) values -> contribution_excess threshold values = 0.
Proof. intros threshold values bounded. induction bounded; simpl; lia. Qed.

Theorem smaller_sorted_rank_has_excess_witness : forall left right,
  StronglySorted (fun x y : nat => y <= x) right ->
  length left = length right ->
  ~ funding_lex_le left right ->
  exists value, In value left /\ 0 < value /\
    contribution_excess (value - 1) right < contribution_excess (value - 1) left.
Proof.
  induction left as [|x xs IH]; intros [|y ys] ordered same not_le; simpl in same; try discriminate.
  - exfalso. apply not_le. simpl. trivial.
  - inversion ordered as [|? ? tail_order tail_bound]; subst.
    destruct (Nat.lt_trichotomy x y) as [less|[equal|greater]].
    + exfalso. apply not_le. simpl. now left.
    + subst y. assert (tail_not_le : ~ funding_lex_le xs ys).
      { intro tail. apply not_le. simpl. right. auto. }
      destruct (IH ys tail_order ltac:(lia) tail_not_le) as [value [included [positive deficit]]].
      exists value. repeat split; simpl; auto; lia.
    + exists x. repeat split; simpl; auto; try lia.
      assert (zero : contribution_excess (x - 1) ys = 0).
      { apply contribution_excess_zero. rewrite Forall_forall in *.
        intros value included. specialize (tail_bound value included). lia. }
      rewrite zero. lia.
Qed.

Theorem breakpoint_excess_bounds_prove_minimax_rank : forall left right,
  length left = length right ->
  (forall value, In value left -> 0 < value ->
    contribution_excess (value - 1) left <= contribution_excess (value - 1) right) ->
  burden_le left right.
Proof.
  intros left right same bounded. unfold burden_le.
  destruct (funding_lex_leb (burden_rank left) (burden_rank right)) eqn:checked.
  - now apply funding_lex_check_exact.
  - exfalso.
    assert (not_le : ~ funding_lex_le (burden_rank left) (burden_rank right)).
    { intro accepted. apply funding_lex_check_exact in accepted. congruence. }
    destruct (smaller_sorted_rank_has_excess_witness (burden_rank left) (burden_rank right)
      (burden_rank_descending right) ltac:(rewrite !burden_rank_preserves_source_count; exact same) not_le)
      as [value [included [positive deficit]]].
    assert (original : In value left).
    { eapply Permutation_in; [apply Permutation_sym; apply burden_rank_permutation|exact included]. }
    specialize (bounded value original positive).
    rewrite <- (contribution_excess_permutation (value - 1) left (burden_rank left) (burden_rank_permutation left)) in deficit.
    rewrite <- (contribution_excess_permutation (value - 1) right (burden_rank right) (burden_rank_permutation right)) in deficit.
    lia.
Qed.

Definition flow_contribution_excess sources obligations threshold flow :=
  funding_sum sources (fun i => source_draw obligations flow i - threshold).

Definition clipped_funding_cut_bound sources obligations eligible capacity demand threshold selected :=
  funding_sum obligations (selected_funding_demand selected demand) -
  funding_sum sources (neighbor_funding_capacity obligations eligible selected (fun i => Nat.min (capacity i) threshold)).

Theorem clipped_cut_bounds_every_assignment_excess : forall sources obligations eligible capacity demand flow threshold selected,
  assignment_valid sources obligations eligible capacity demand flow ->
  clipped_funding_cut_bound sources obligations eligible capacity demand threshold selected <=
    flow_contribution_excess sources obligations threshold flow.
Proof.
  intros sources obligations eligible capacity demand flow threshold selected valid.
  assert (actual : assignment_valid sources obligations eligible (source_draw obligations flow) demand flow).
  { eapply accepted_assignment_respects_reserved_sources; [exact valid|auto]. }
  pose proof (valid_assignment_covers_every_selected_cut _ _ _ _ _ _ selected actual) as covered.
  assert (bounded : funding_sum sources (neighbor_funding_capacity obligations eligible selected (source_draw obligations flow)) <=
    funding_sum sources (neighbor_funding_capacity obligations eligible selected (fun i => Nat.min (capacity i) threshold)) +
    flow_contribution_excess sources obligations threshold flow).
  { unfold flow_contribution_excess. rewrite <- funding_sum_add.
    apply funding_sum_monotone. intros i inside.
    pose proof (proj1 (proj1 valid i inside)) as cap.
    unfold neighbor_funding_capacity. destruct (funding_neighbor obligations eligible selected i); [|lia].
    destruct (Nat.le_ge_cases (capacity i) threshold).
    - rewrite Nat.min_l by assumption. lia.
    - rewrite Nat.min_r by assumption. lia. }
  unfold clipped_funding_cut_bound. lia.
Qed.

Lemma contribution_excess_append : forall threshold left right,
  contribution_excess threshold (left ++ right) =
  contribution_excess threshold left + contribution_excess threshold right.
Proof. intros threshold left. induction left; intros; simpl; [reflexivity|rewrite IHleft; lia]. Qed.

Lemma contribution_excess_source_list : forall sources obligations threshold flow,
  contribution_excess threshold (map (source_draw obligations flow) (seq 0 sources)) =
  flow_contribution_excess sources obligations threshold flow.
Proof.
  induction sources; intros; [reflexivity|].
  rewrite seq_S, map_app, contribution_excess_append, IHsources.
  unfold flow_contribution_excess. simpl. lia.
Qed.

Definition funding_minimax_cut_certificate sources obligations eligible capacity demand flow cuts :=
  forall i, i < sources -> 0 < source_draw obligations flow i ->
  flow_contribution_excess sources obligations (source_draw obligations flow i - 1) flow =
  clipped_funding_cut_bound sources obligations eligible capacity demand
    (source_draw obligations flow i - 1) (cuts i).

Theorem capacity_total_forces_every_source_exact : forall sources obligations eligible capacity demand flow,
  assignment_valid sources obligations eligible capacity demand flow ->
  funding_sum sources capacity = funding_sum obligations demand ->
  forall i, i < sources -> source_draw obligations flow i = capacity i.
Proof.
  intros sources obligations eligible capacity demand flow valid equal.
  apply funding_sum_equal_bounded_entries.
  - intros i inside. apply (proj1 (proj1 valid i inside)).
  - rewrite accepted_assignment_conserves_obligation with (eligible := eligible) (capacity := capacity) (demand := demand) by exact valid.
    symmetry. exact equal.
Qed.

Theorem checked_excess_cuts_prove_global_funding_minimax : forall sources obligations eligible capacity demand candidate cuts,
  assignment_valid sources obligations eligible capacity demand candidate ->
  funding_minimax_cut_certificate sources obligations eligible capacity demand candidate cuts ->
  forall alternative, assignment_valid sources obligations eligible capacity demand alternative ->
    burden_le (map (source_draw obligations candidate) (seq 0 sources))
      (map (source_draw obligations alternative) (seq 0 sources)).
Proof.
  intros sources obligations eligible capacity demand candidate cuts valid certificate alternative feasible.
  apply breakpoint_excess_bounds_prove_minimax_rank; [now rewrite !length_map|].
  intros value included positive. apply in_map_iff in included.
  destruct included as [i [<- inside]]. apply in_seq in inside.
  rewrite !contribution_excess_source_list.
  rewrite (certificate i ltac:(lia) positive).
  apply clipped_cut_bounds_every_assignment_excess. exact feasible.
Qed.

Example maximum_only_witness_misses_lower_rank_failure :
  contribution_excess 4 [5;4;1] = contribution_excess 4 [5;3;2] /\
  contribution_excess 3 [5;3;2] < contribution_excess 3 [5;4;1].
Proof. simpl. auto. Qed.
