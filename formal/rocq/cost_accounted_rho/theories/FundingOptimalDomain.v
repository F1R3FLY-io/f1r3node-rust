From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia Sorting.Permutation Sorting.Sorted.
From CostAccountedRho Require Import EligibleFundingAssignment FundingDeficitCertificate FundingResidualCut FundingBox LexicographicMinimax FundingMinimaxCertificate.
Import ListNotations.

Theorem worse_sorted_rank_has_reference_breakpoint : forall left right,
  StronglySorted (fun x y : nat => y <= x) right -> length left = length right ->
  ~ funding_lex_le left right ->
  exists value, In value right /\ contribution_excess value right < contribution_excess value left.
Proof.
  induction left as [|x xs IH]; intros [|y ys] ordered same not_le; simpl in same; try discriminate.
  - exfalso. apply not_le. simpl. trivial.
  - inversion ordered as [|? ? tail_order tail_bound]; subst.
    destruct (Nat.lt_trichotomy x y) as [less|[equal|greater]].
    + exfalso. apply not_le. simpl. now left.
    + subst y. assert (tail_not_le : ~ funding_lex_le xs ys).
      { intro tail. apply not_le. simpl. right. auto. }
      destruct (IH ys tail_order ltac:(lia) tail_not_le) as [value [included deficit]].
      exists value. split; simpl; auto; lia.
    + exists y. split; [now left|]. simpl.
      rewrite contribution_excess_zero by exact tail_bound. lia.
Qed.

Theorem reference_breakpoints_bound_alternative_rank : forall reference alternative,
  length reference = length alternative ->
  contribution_excess 0 reference = contribution_excess 0 alternative ->
  (forall value, In value reference -> 0 < value -> contribution_excess value alternative <= contribution_excess value reference) ->
  burden_le alternative reference.
Proof.
  intros reference alternative size total bounded. unfold burden_le.
  destruct (funding_lex_leb (burden_rank alternative) (burden_rank reference)) eqn:checked.
  - now apply funding_lex_check_exact.
  - exfalso.
    assert (not_le : ~ funding_lex_le (burden_rank alternative) (burden_rank reference)).
    { intro accepted. apply funding_lex_check_exact in accepted. congruence. }
    destruct (worse_sorted_rank_has_reference_breakpoint (burden_rank alternative) (burden_rank reference)
      (burden_rank_descending reference) ltac:(rewrite !burden_rank_preserves_source_count; lia) not_le)
      as [value [included deficit]].
    assert (original : In value reference).
    { eapply Permutation_in; [apply Permutation_sym; apply burden_rank_permutation|exact included]. }
    rewrite <- (contribution_excess_permutation value reference (burden_rank reference) (burden_rank_permutation reference)) in deficit.
    rewrite <- (contribution_excess_permutation value alternative (burden_rank alternative) (burden_rank_permutation alternative)) in deficit.
    destruct value; [lia|]. specialize (bounded (S value) original ltac:(lia)). lia.
Qed.

Theorem paired_breakpoints_characterize_equal_rank : forall reference alternative,
  length reference = length alternative ->
  contribution_excess 0 reference = contribution_excess 0 alternative ->
  (forall value, In value reference -> 0 < value ->
    contribution_excess (value - 1) reference = contribution_excess (value - 1) alternative /\
    contribution_excess value reference = contribution_excess value alternative) ->
  burden_rank reference = burden_rank alternative.
Proof.
  intros reference alternative size total same. apply equal_optima_have_identical_rank.
  - apply breakpoint_excess_bounds_prove_minimax_rank; [exact size|].
    intros value included positive. destruct (same value included positive). lia.
  - apply reference_breakpoints_bound_alternative_rank; [exact size|exact total|].
    intros value included positive. destruct (same value included positive). lia.
Qed.

Theorem equal_rank_preserves_every_excess : forall left right,
  burden_rank left = burden_rank right ->
  forall threshold, contribution_excess threshold left = contribution_excess threshold right.
Proof.
  intros left right same threshold.
  rewrite (contribution_excess_permutation threshold left (burden_rank left) (burden_rank_permutation left)).
  rewrite (contribution_excess_permutation threshold right (burden_rank right) (burden_rank_permutation right)).
  now rewrite same.
Qed.

Definition tight_funding_excess_cut sources obligations eligible capacity demand flow threshold selected :=
  flow_contribution_excess sources obligations threshold flow +
  funding_sum sources (neighbor_funding_capacity obligations eligible selected (fun i => Nat.min (capacity i) threshold)) =
  funding_sum obligations (selected_funding_demand selected demand).

Definition funding_cut_lower obligations eligible capacity threshold selected i :=
  if funding_neighbor obligations eligible selected i then Nat.min (capacity i) threshold else 0.

Definition funding_cut_upper obligations eligible capacity threshold selected i :=
  if funding_neighbor obligations eligible selected i then capacity i else Nat.min (capacity i) threshold.

Definition funding_cut_eligible obligations eligible selected i j :=
  eligible i j && (negb (funding_neighbor obligations eligible selected i) || selected j).

Lemma tight_cut_selected_rows_exact : forall sources obligations eligible capacity demand flow threshold selected,
  assignment_valid sources obligations eligible capacity demand flow ->
  tight_funding_excess_cut sources obligations eligible capacity demand flow threshold selected ->
  forall i, i < sources ->
    source_draw obligations (selected_funding_flow selected flow) i =
    neighbor_funding_capacity obligations eligible selected (fun i => Nat.min (capacity i) threshold) i +
      (source_draw obligations flow i - threshold).
Proof.
  intros sources obligations eligible capacity demand flow threshold selected valid tight.
  apply funding_sum_equal_bounded_entries.
  - intros i inside.
    pose proof (selected_funding_rows_bounded sources obligations eligible capacity demand flow selected i valid inside) as bounded.
    assert (selected_below : source_draw obligations (selected_funding_flow selected flow) i <= source_draw obligations flow i).
    { unfold source_draw. apply funding_sum_monotone. intros j _. unfold selected_funding_flow. destruct (selected j); lia. }
    unfold neighbor_funding_capacity in *. destruct (funding_neighbor obligations eligible selected i).
    + pose proof (proj1 (proj1 valid i inside)). destruct (Nat.le_ge_cases (capacity i) threshold).
      * rewrite Nat.min_l by assumption. lia.
      * rewrite Nat.min_r by assumption. lia.
    + lia.
  - rewrite funding_sum_add. unfold tight_funding_excess_cut, flow_contribution_excess in tight.
    assert (selected_total : funding_sum sources (source_draw obligations (selected_funding_flow selected flow)) =
      funding_sum obligations (selected_funding_demand selected demand)).
    { rewrite funding_assignment_rows_equal_columns. apply funding_sum_ext.
      intros j inside. eapply selected_funding_columns_exact; eauto. }
    rewrite selected_total. lia.
Qed.

Theorem tight_cut_implies_box_restriction : forall sources obligations eligible capacity demand flow threshold selected,
  assignment_valid sources obligations eligible capacity demand flow ->
  tight_funding_excess_cut sources obligations eligible capacity demand flow threshold selected ->
  box_funding_valid sources obligations (funding_cut_eligible obligations eligible selected)
    (funding_cut_lower obligations eligible capacity threshold selected)
    (funding_cut_upper obligations eligible capacity threshold selected) demand flow.
Proof.
  intros sources obligations eligible capacity demand flow threshold selected valid tight.
  pose proof (tight_cut_selected_rows_exact _ _ _ _ _ _ _ _ valid tight) as exact_rows.
  assert (row_facts : forall i, i < sources ->
    source_draw obligations flow i <= funding_cut_upper obligations eligible capacity threshold selected i /\
    funding_cut_lower obligations eligible capacity threshold selected i <= source_draw obligations flow i /\
    (funding_neighbor obligations eligible selected i = true ->
      source_draw obligations (selected_funding_flow selected flow) i = source_draw obligations flow i)).
  { intros i inside. specialize (exact_rows i inside).
    pose proof (proj1 (proj1 valid i inside)) as cap.
    assert (subset : source_draw obligations (selected_funding_flow selected flow) i <= source_draw obligations flow i).
    { unfold source_draw. apply funding_sum_monotone. intros j _. unfold selected_funding_flow. destruct (selected j); lia. }
    unfold funding_cut_upper, funding_cut_lower, neighbor_funding_capacity in *.
    destruct (funding_neighbor obligations eligible selected i) eqn:neighbor.
    - destruct (Nat.le_ge_cases (capacity i) threshold).
      + rewrite Nat.min_l in * by assumption. repeat split; auto; lia.
      + rewrite Nat.min_r in * by assumption. repeat split; auto; lia.
    - pose proof (nonneighbor_selected_flow_zero _ _ _ _ _ _ _ _ valid inside neighbor).
      split; [apply Nat.min_glb; lia|]. split; [lia|discriminate]. }
  split.
  - split; [|exact (proj2 valid)]. intros i inside. split; [apply (proj1 (row_facts i inside))|].
    intros j bounded excluded. unfold funding_cut_eligible in excluded.
    destruct (eligible i j) eqn:permitted; [|apply (proj2 (proj1 valid i inside) j bounded permitted)].
    simpl in excluded. apply orb_false_iff in excluded. destruct excluded as [neighbor unselected].
    apply negb_false_iff in neighbor.
    pose proof (proj2 (proj2 (row_facts i inside)) neighbor) as equal.
    assert (entry : selected_funding_flow selected flow i j = flow i j).
    { apply (funding_sum_equal_bounded_entries obligations (selected_funding_flow selected flow i) (flow i)).
      - intros k _. unfold selected_funding_flow. destruct (selected k); lia.
      - exact equal.
      - exact bounded. }
    unfold selected_funding_flow in entry. rewrite unselected in entry. lia.
  - intros i inside. apply (proj1 (proj2 (row_facts i inside))).
Qed.

Theorem box_restriction_implies_original_and_tight_cut : forall sources obligations eligible capacity demand flow threshold selected,
  box_funding_valid sources obligations (funding_cut_eligible obligations eligible selected)
    (funding_cut_lower obligations eligible capacity threshold selected)
    (funding_cut_upper obligations eligible capacity threshold selected) demand flow ->
  assignment_valid sources obligations eligible capacity demand flow /\
  tight_funding_excess_cut sources obligations eligible capacity demand flow threshold selected.
Proof.
  intros sources obligations eligible capacity demand flow threshold selected [[rows columns] lowers].
  assert (valid : assignment_valid sources obligations eligible capacity demand flow).
  { split; [|exact columns]. intros i inside. destruct (rows i inside) as [upper edges]. split.
    - unfold funding_cut_upper in upper. destruct (funding_neighbor obligations eligible selected i); [exact upper|].
      eapply Nat.le_trans; [exact upper|apply Nat.le_min_l].
    - intros j bounded excluded. apply edges; [exact bounded|]. unfold funding_cut_eligible. now rewrite excluded. }
  split; [exact valid|].
  assert (equal : forall i, i < sources ->
    neighbor_funding_capacity obligations eligible selected (fun i => Nat.min (capacity i) threshold) i +
      (source_draw obligations flow i - threshold) = source_draw obligations (selected_funding_flow selected flow) i).
  { intros i inside. pose proof (rows i inside) as [upper edges]. specialize (lowers i inside).
    unfold funding_cut_upper in upper. unfold funding_cut_lower in lowers. unfold neighbor_funding_capacity.
    destruct (funding_neighbor obligations eligible selected i) eqn:neighbor.
    - assert (all_selected : source_draw obligations (selected_funding_flow selected flow) i = source_draw obligations flow i).
      { unfold source_draw. apply funding_sum_ext. intros j bounded. unfold selected_funding_flow.
        destruct (selected j) eqn:chosen; [reflexivity|].
        assert (flow i j = 0).
        { apply edges; [exact bounded|]. unfold funding_cut_eligible. rewrite neighbor, chosen. now destruct (eligible i j). }
        lia. }
      rewrite all_selected. destruct (Nat.le_ge_cases (capacity i) threshold).
      + rewrite Nat.min_l in * by assumption. lia.
      + rewrite Nat.min_r in * by assumption. lia.
    - rewrite (nonneighbor_selected_flow_zero _ _ _ _ _ _ _ _ valid inside neighbor).
      pose proof (Nat.le_min_r (capacity i) threshold). lia. }
  pose proof (funding_sum_ext sources _ _ equal) as summed.
  rewrite funding_sum_add, funding_assignment_rows_equal_columns in summed.
  assert (selected_total : funding_sum obligations (obligation_draw sources (selected_funding_flow selected flow)) =
    funding_sum obligations (selected_funding_demand selected demand)).
  { apply funding_sum_ext. intros j inside. eapply selected_funding_columns_exact; eauto. }
  rewrite selected_total in summed. unfold tight_funding_excess_cut, flow_contribution_excess. lia.
Qed.

Theorem tight_cut_iff_box_restriction : forall sources obligations eligible capacity demand flow threshold selected,
  (assignment_valid sources obligations eligible capacity demand flow /\
   tight_funding_excess_cut sources obligations eligible capacity demand flow threshold selected) <->
  box_funding_valid sources obligations (funding_cut_eligible obligations eligible selected)
    (funding_cut_lower obligations eligible capacity threshold selected)
    (funding_cut_upper obligations eligible capacity threshold selected) demand flow.
Proof.
  intros. split; [intros [valid tight]; now apply tight_cut_implies_box_restriction|].
  apply box_restriction_implies_original_and_tight_cut.
Qed.

Lemma complete_assignment_zero_excess_total : forall sources obligations eligible capacity demand flow,
  assignment_valid sources obligations eligible capacity demand flow ->
  flow_contribution_excess sources obligations 0 flow = funding_sum obligations demand.
Proof.
  intros. unfold flow_contribution_excess.
  transitivity (funding_sum sources (source_draw obligations flow)).
  - apply funding_sum_ext. intros. lia.
  - eapply accepted_assignment_conserves_obligation; eauto.
Qed.

Definition paired_funding_cut_family sources obligations eligible capacity demand reference low_cut high_cut :=
  forall i, i < sources -> 0 < source_draw obligations reference i ->
  tight_funding_excess_cut sources obligations eligible capacity demand reference
    (source_draw obligations reference i - 1) (low_cut i) /\
  tight_funding_excess_cut sources obligations eligible capacity demand reference
    (source_draw obligations reference i) (high_cut i).

Definition paired_funding_box_family sources obligations eligible capacity demand reference low_cut high_cut flow :=
  assignment_valid sources obligations eligible capacity demand flow /\
  forall i, i < sources -> 0 < source_draw obligations reference i ->
    box_funding_valid sources obligations (funding_cut_eligible obligations eligible (low_cut i))
      (funding_cut_lower obligations eligible capacity (source_draw obligations reference i - 1) (low_cut i))
      (funding_cut_upper obligations eligible capacity (source_draw obligations reference i - 1) (low_cut i)) demand flow /\
    box_funding_valid sources obligations (funding_cut_eligible obligations eligible (high_cut i))
      (funding_cut_lower obligations eligible capacity (source_draw obligations reference i) (high_cut i))
      (funding_cut_upper obligations eligible capacity (source_draw obligations reference i) (high_cut i)) demand flow.

Theorem paired_cut_boxes_capture_exact_equal_rank_domain : forall sources obligations eligible capacity demand reference low_cut high_cut alternative,
  assignment_valid sources obligations eligible capacity demand reference ->
  paired_funding_cut_family sources obligations eligible capacity demand reference low_cut high_cut ->
  (paired_funding_box_family sources obligations eligible capacity demand reference low_cut high_cut alternative <->
   assignment_valid sources obligations eligible capacity demand alternative /\
   burden_rank (map (source_draw obligations reference) (seq 0 sources)) =
   burden_rank (map (source_draw obligations alternative) (seq 0 sources))).
Proof.
  intros sources obligations eligible capacity demand reference low_cut high_cut alternative reference_valid certificate.
  split.
  - intros [valid boxes]. split; [exact valid|].
    apply paired_breakpoints_characterize_equal_rank; [now rewrite !length_map| |].
    + rewrite !contribution_excess_source_list.
      rewrite (complete_assignment_zero_excess_total _ _ _ _ _ _ valid).
      now apply complete_assignment_zero_excess_total with (eligible := eligible) (capacity := capacity).
    + intros value included positive. apply in_map_iff in included. destruct included as [i [<- inside]].
      apply in_seq in inside. specialize (certificate i ltac:(lia) positive).
      specialize (boxes i ltac:(lia) positive). destruct boxes as [low high].
      apply box_restriction_implies_original_and_tight_cut in low, high.
      rewrite !contribution_excess_source_list.
      unfold tight_funding_excess_cut in *. split; lia.
  - intros [valid same]. split; [exact valid|]. intros i inside positive.
    specialize (certificate i inside positive). destruct certificate as [low high].
    assert (excess : forall threshold, flow_contribution_excess sources obligations threshold reference =
      flow_contribution_excess sources obligations threshold alternative).
    { intro threshold. rewrite <- !contribution_excess_source_list.
      now apply equal_rank_preserves_every_excess. }
    unfold tight_funding_excess_cut in low, high. rewrite excess in low, high.
    split; apply tight_cut_implies_box_restriction; assumption.
Qed.

Record funding_box_spec := {
  box_lower : nat -> nat;
  box_upper : nat -> nat;
  box_edges : nat -> nat -> bool
}.

Definition funding_spec_valid sources obligations demand spec flow :=
  box_funding_valid sources obligations (box_edges spec) (box_lower spec) (box_upper spec) demand flow.

Definition intersect_funding_specs left right := {|
  box_lower := fun i => Nat.max (box_lower left i) (box_lower right i);
  box_upper := fun i => Nat.min (box_upper left i) (box_upper right i);
  box_edges := fun i j => box_edges left i j && box_edges right i j
|}.

Theorem funding_spec_intersection_exact : forall sources obligations demand left right flow,
  funding_spec_valid sources obligations demand (intersect_funding_specs left right) flow <->
  funding_spec_valid sources obligations demand left flow /\ funding_spec_valid sources obligations demand right flow.
Proof.
  intros sources obligations demand left right flow. unfold funding_spec_valid, box_funding_valid, assignment_valid, source_valid, intersect_funding_specs. simpl.
  split.
  - intros [[rows columns] lower].
    assert (each : forall i, i < sources ->
      source_draw obligations flow i <= box_upper left i /\ source_draw obligations flow i <= box_upper right i /\
      box_lower left i <= source_draw obligations flow i /\ box_lower right i <= source_draw obligations flow i).
    { intros i inside. destruct (rows i inside) as [upper _]. specialize (lower i inside).
      pose proof (Nat.le_min_l (box_upper left i) (box_upper right i)).
      pose proof (Nat.le_min_r (box_upper left i) (box_upper right i)).
      pose proof (Nat.le_max_l (box_lower left i) (box_lower right i)).
      pose proof (Nat.le_max_r (box_lower left i) (box_lower right i)). repeat split; lia. }
    split; split; try (intros i inside; specialize (each i inside); tauto);
      split; [|exact columns| |exact columns]; intros i inside;
      split; try (specialize (each i inside); tauto).
    + intros j bounded excluded. apply (proj2 (rows i inside) j bounded).
      now rewrite excluded.
    + intros j bounded excluded. apply (proj2 (rows i inside) j bounded).
      now rewrite excluded, andb_false_r.
  - intros [[[left_rows left_columns] left_lower] [[right_rows _] right_lower]].
    split.
    + split; [|exact left_columns]. intros i inside. split.
      * apply Nat.min_glb; [apply (proj1 (left_rows i inside))|apply (proj1 (right_rows i inside))].
      * intros j bounded excluded. apply andb_false_iff in excluded. destruct excluded;
          [apply (proj2 (left_rows i inside) j bounded)|apply (proj2 (right_rows i inside) j bounded)]; assumption.
    + intros i inside. apply Nat.max_lub; [apply left_lower|apply right_lower]; assumption.
Qed.

Fixpoint intersect_funding_family base (specs : list funding_box_spec) :=
  match specs with
  | [] => base
  | spec :: rest => intersect_funding_family (intersect_funding_specs base spec) rest
  end.

Theorem funding_family_intersection_exact : forall specs base sources obligations demand flow,
  funding_spec_valid sources obligations demand (intersect_funding_family base specs) flow <->
  funding_spec_valid sources obligations demand base flow /\
  Forall (fun spec => funding_spec_valid sources obligations demand spec flow) specs.
Proof.
  induction specs as [|spec rest IH]; intros; simpl.
  - split; [intro valid; split; [exact valid|constructor]|tauto].
  - rewrite IH, funding_spec_intersection_exact. split.
    + intros [[base_valid spec_valid] rest_valid]. split; [exact base_valid|now constructor].
    + intros [base_valid all_valid]. inversion all_valid; subst. auto.
Qed.
