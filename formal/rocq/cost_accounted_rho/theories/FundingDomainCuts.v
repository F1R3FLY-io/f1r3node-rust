From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import EligibleFundingAssignment FundingDeficitCertificate FundingResidualCut.
From CostAccountedRho Require Import FundingFifoSearch FundingResidualGraph FundingDiscovery.
Import ListNotations.

Definition source_subset_capacity sources selected capacity :=
  funding_sum sources (selected_funding_demand selected capacity).

Definition source_neighborhood sources (eligible : nat -> nat -> bool) selected :=
  funding_neighbor sources (fun obligation source => eligible source obligation) selected.

Definition source_neighborhood_demand sources obligations eligible selected demand :=
  funding_sum obligations
    (selected_funding_demand (source_neighborhood sources eligible selected) demand).

Definition contribution_cut_domain sources obligations eligible capacity demand :=
  forall selected,
    Nat.min (funding_sum obligations demand) (source_subset_capacity sources selected capacity) <=
    source_neighborhood_demand sources obligations eligible selected demand.

Definition excluded_source_cut_domain sources obligations eligible capacity demand :=
  forall obligation, obligation < obligations -> 0 < demand obligation ->
    forall selected,
      (forall source, source < sources -> selected source = true -> eligible source obligation = false) ->
      source_subset_capacity sources selected capacity <=
      source_neighborhood_demand sources obligations eligible selected demand.

Definition transposed_cut_domain sources obligations eligible capacity demand :=
  forall obligation, obligation < obligations -> 0 < demand obligation ->
    forall selected,
      funding_deficit_check obligations sources
        (fun target source => eligible source target) demand
        (fun source => if eligible source obligation then 0 else capacity source) selected = false.

Lemma funding_sum_strict_entry : forall count lhs rhs index,
  (forall position, position < count -> lhs position <= rhs position) ->
  index < count -> lhs index < rhs index ->
  funding_sum count lhs < funding_sum count rhs.
Proof.
  induction count; intros lhs rhs index bounded inside strict; simpl; [lia|].
  destruct (Nat.eq_dec index count) as [->|different].
  - assert (funding_sum count lhs <= funding_sum count rhs).
    { apply funding_sum_monotone. intros. apply bounded. lia. }
    lia.
  - assert (funding_sum count lhs < funding_sum count rhs).
    { apply (IHcount lhs rhs index).
      - intros. apply bounded. lia.
      - lia.
      - exact strict. }
    specialize (bounded count ltac:(lia)). lia.
Qed.

Lemma funding_sum_strict_has_entry : forall count lhs rhs,
  funding_sum count lhs < funding_sum count rhs ->
  exists index, index < count /\ lhs index < rhs index.
Proof.
  induction count; intros lhs rhs strict; simpl in strict; [lia|].
  destruct (Nat.lt_ge_cases (lhs count) (rhs count)) as [last|last].
  - exists count. auto.
  - destruct (IHcount lhs rhs ltac:(lia)) as [index [inside smaller]].
    exists index. auto.
Qed.

Lemma omitted_positive_obligation_makes_cut_proper : forall sources obligations eligible selected demand obligation,
  obligation < obligations -> 0 < demand obligation ->
  (forall source, source < sources -> selected source = true -> eligible source obligation = false) ->
  source_neighborhood_demand sources obligations eligible selected demand < funding_sum obligations demand.
Proof.
  intros sources obligations eligible selected demand obligation inside positive excluded.
  assert (absent : source_neighborhood sources eligible selected obligation = false).
  { unfold source_neighborhood. destruct (funding_neighbor sources
      (fun target source => eligible source target) selected obligation) eqn:neighbor; [|reflexivity].
    apply funding_neighbor_exact in neighbor. destruct neighbor as [source [bounded [chosen permitted]]].
    specialize (excluded source bounded chosen). congruence. }
  unfold source_neighborhood_demand. eapply funding_sum_strict_entry with (index := obligation).
  - intros. unfold selected_funding_demand. destruct (source_neighborhood sources eligible selected position); lia.
  - exact inside.
  - unfold selected_funding_demand. now rewrite absent.
Qed.

Theorem contribution_cut_domain_iff_excluded_source_cuts : forall sources obligations eligible capacity demand,
  contribution_cut_domain sources obligations eligible capacity demand <->
  excluded_source_cut_domain sources obligations eligible capacity demand.
Proof.
  intros sources obligations eligible capacity demand. split.
  - intros all_cuts obligation inside positive selected excluded.
    specialize (all_cuts selected).
    pose proof (omitted_positive_obligation_makes_cut_proper sources obligations eligible selected demand
      obligation inside positive excluded) as proper.
    destruct (Nat.le_ge_cases (funding_sum obligations demand)
      (source_subset_capacity sources selected capacity)) as [large|small].
    + rewrite Nat.min_l in all_cuts by exact large. lia.
    + rewrite Nat.min_r in all_cuts by exact small. exact all_cuts.
  - intros excluded selected.
    destruct (Nat.le_gt_cases (funding_sum obligations demand)
      (source_neighborhood_demand sources obligations eligible selected demand)) as [whole|proper].
    + eapply Nat.le_trans; [apply Nat.le_min_l|exact whole].
    + destruct (funding_sum_strict_has_entry obligations
        (selected_funding_demand (source_neighborhood sources eligible selected) demand) demand proper)
        as [obligation [inside smaller]].
      unfold selected_funding_demand in smaller.
      destruct (source_neighborhood sources eligible selected obligation) eqn:neighbor; [lia|].
      eapply Nat.le_trans; [apply Nat.le_min_r|].
      apply (excluded obligation inside smaller selected).
      intros source bounded chosen. destruct (eligible source obligation) eqn:permitted; [|reflexivity].
      assert (source_neighborhood sources eligible selected obligation = true).
      { apply funding_neighbor_exact. exists source. auto. }
      congruence.
Qed.

Lemma source_neighborhood_monotone : forall sources eligible left right obligation,
  (forall source, source < sources -> left source = true -> right source = true) ->
  source_neighborhood sources eligible left obligation = true ->
  source_neighborhood sources eligible right obligation = true.
Proof.
  intros sources eligible left right obligation included neighbor.
  apply funding_neighbor_exact in neighbor. destruct neighbor as [source [inside [chosen permitted]]].
  apply funding_neighbor_exact. exists source. repeat split; auto.
Qed.

Theorem excluded_source_cuts_iff_transposed_cuts : forall sources obligations eligible capacity demand,
  excluded_source_cut_domain sources obligations eligible capacity demand <->
  transposed_cut_domain sources obligations eligible capacity demand.
Proof.
  intros sources obligations eligible capacity demand. split.
  - intros excluded obligation inside positive selected.
    set (filtered := fun source => selected source && negb (eligible source obligation)).
    assert (selection : forall source, source < sources -> filtered source = true ->
      eligible source obligation = false).
    { intros source bounded chosen. unfold filtered in chosen.
      apply andb_true_iff in chosen. apply negb_true_iff. tauto. }
    pose proof (excluded obligation inside positive filtered selection) as bound.
    unfold funding_deficit_check. apply Nat.ltb_ge.
    assert (same : funding_sum sources
        (selected_funding_demand selected (fun source => if eligible source obligation then 0 else capacity source)) =
      source_subset_capacity sources filtered capacity).
    { apply funding_sum_ext. intros source bounded. unfold selected_funding_demand, filtered.
      destruct (selected source), (eligible source obligation); reflexivity. }
    rewrite same. eapply Nat.le_trans; [exact bound|].
    apply funding_sum_monotone. intros target bounded.
    unfold selected_funding_demand, neighbor_funding_capacity.
    destruct (source_neighborhood sources eligible filtered target) eqn:neighbor; [|lia].
    assert (source_neighborhood sources eligible selected target = true).
    { eapply source_neighborhood_monotone; [|exact neighbor].
      intros source source_inside chosen. unfold filtered in chosen. now apply andb_true_iff in chosen as [chosen _]. }
    unfold source_neighborhood in H. now rewrite H.
  - intros transposed obligation inside positive selected excluded.
    specialize (transposed obligation inside positive selected).
    unfold funding_deficit_check in transposed. apply Nat.ltb_ge in transposed.
    assert (same : funding_sum sources
        (selected_funding_demand selected (fun source => if eligible source obligation then 0 else capacity source)) =
      source_subset_capacity sources selected capacity).
    { apply funding_sum_ext. intros source bounded. unfold selected_funding_demand.
      destruct (selected source) eqn:chosen; [rewrite excluded by assumption|]; reflexivity. }
    rewrite same in transposed. exact transposed.
Qed.

Theorem contribution_cuts_iff_transposed_cuts : forall sources obligations eligible capacity demand,
  contribution_cut_domain sources obligations eligible capacity demand <->
  transposed_cut_domain sources obligations eligible capacity demand.
Proof.
  intros. rewrite contribution_cut_domain_iff_excluded_source_cuts.
  apply excluded_source_cuts_iff_transposed_cuts.
Qed.

Theorem transposed_assignments_certify_contribution_cuts : forall sources obligations eligible capacity demand,
  (forall obligation, obligation < obligations -> 0 < demand obligation ->
    exists flow, assignment_valid obligations sources
      (fun target source => eligible source target) demand
      (fun source => if eligible source obligation then 0 else capacity source) flow) ->
  contribution_cut_domain sources obligations eligible capacity demand.
Proof.
  intros sources obligations eligible capacity demand assignments.
  apply contribution_cuts_iff_transposed_cuts. intros obligation inside positive selected.
  destruct (assignments obligation inside positive) as [flow valid].
  eapply valid_assignment_never_has_deficit. exact valid.
Qed.

Theorem zero_demand_has_unrestricted_contribution_cuts : forall sources obligations eligible capacity demand,
  funding_sum obligations demand = 0 ->
  contribution_cut_domain sources obligations eligible capacity demand.
Proof.
  intros sources obligations eligible capacity demand zero selected. rewrite zero. simpl. lia.
Qed.

Definition capped_contribution sources capacity total contribution :=
  funding_sum sources contribution = total /\
  forall source, source < sources -> contribution source <= capacity source.

Theorem contribution_cuts_cover_every_capped_vector : forall sources obligations eligible capacity demand contribution,
  contribution_cut_domain sources obligations eligible capacity demand ->
  capped_contribution sources capacity (funding_sum obligations demand) contribution ->
  forall selected, funding_deficit_check sources obligations eligible contribution demand selected = false.
Proof.
  intros sources obligations eligible capacity demand contribution cuts [total bounded] selected.
  set (excluded := fun source => negb (funding_neighbor obligations eligible selected source)).
  assert (excluded_capacity : source_subset_capacity sources excluded contribution <=
      source_subset_capacity sources excluded capacity).
  { apply funding_sum_monotone. intros source inside. unfold selected_funding_demand.
    destruct (excluded source); [apply bounded; assumption|lia]. }
  pose proof (funding_sum_partition sources
    (funding_neighbor obligations eligible selected) contribution) as source_partition.
  pose proof (funding_sum_partition obligations selected demand) as demand_partition.
  assert (excluded_total : source_subset_capacity sources excluded contribution <= funding_sum obligations demand).
  { unfold source_subset_capacity, excluded. lia. }
  assert (excluded_cut : source_subset_capacity sources excluded contribution <=
      source_neighborhood_demand sources obligations eligible excluded demand).
  { eapply Nat.le_trans; [|apply cuts]. apply Nat.min_glb; assumption. }
  assert (disjoint : source_neighborhood_demand sources obligations eligible excluded demand <=
      funding_sum obligations (selected_funding_demand (fun index => negb (selected index)) demand)).
  { apply funding_sum_monotone. intros obligation inside. unfold selected_funding_demand.
    destruct (source_neighborhood sources eligible excluded obligation) eqn:neighbor; [|lia].
    assert (selected obligation = false).
    { apply funding_neighbor_exact in neighbor.
      destruct neighbor as [source [source_inside [chosen permitted]]].
      unfold excluded in chosen. apply negb_true_iff in chosen.
      destruct (selected obligation) eqn:selected_at; [|reflexivity].
      assert (funding_neighbor obligations eligible selected source = true).
      { apply funding_neighbor_exact. exists obligation. auto. }
      congruence. }
    rewrite H. simpl. lia. }
  unfold funding_deficit_check. apply Nat.ltb_ge.
  change (funding_sum obligations (selected_funding_demand selected demand) <=
    funding_sum sources (selected_funding_demand (funding_neighbor obligations eligible selected) contribution)).
  unfold source_subset_capacity, excluded in excluded_cut. unfold excluded in disjoint. lia.
Qed.

Theorem certified_transposed_flows_exclude_every_contribution_deficit : forall sources obligations eligible capacity demand contribution,
  (forall obligation, obligation < obligations -> 0 < demand obligation ->
    exists flow, assignment_valid obligations sources
      (fun target source => eligible source target) demand
      (fun source => if eligible source obligation then 0 else capacity source) flow) ->
  capped_contribution sources capacity (funding_sum obligations demand) contribution ->
  forall selected, funding_deficit_check sources obligations eligible contribution demand selected = false.
Proof.
  intros sources obligations eligible capacity demand contribution transposed capped selected.
  eapply contribution_cuts_cover_every_capped_vector; [|exact capped].
  now apply transposed_assignments_certify_contribution_cuts.
Qed.

Theorem certified_capped_vector_search_reaches_sink : forall sources obligations eligible capacity demand contribution flow neighbors,
  contribution_cut_domain sources obligations eligible capacity demand ->
  capped_contribution sources capacity (funding_sum obligations demand) contribution ->
  partial_funding_valid sources obligations eligible contribution demand flow ->
  funding_sum obligations (obligation_draw sources flow) < funding_sum obligations demand ->
  fifo_neighbors_valid (funding_vertex_count sources obligations)
    (funding_residual_graph sources obligations eligible contribution demand flow) neighbors ->
  exists state cursor,
    fifo_search_reference (funding_vertex_count sources obligations) 0 (funding_sink sources obligations)
      (funding_residual_graph sources obligations eligible contribution demand flow) neighbors
      (discovery_initial 0) 0 = Some (state, cursor) /\
    discovery_known state (funding_sink sources obligations) = true.
Proof.
  intros sources obligations eligible capacity demand contribution flow neighbors cuts capped partial short covered.
  destruct (initial_funding_fifo_returns_path_or_checked_deficit sources obligations eligible contribution demand flow
    neighbors partial short covered) as [state [cursor [result [found|[_ [deficit _]]]]]].
  - exists state, cursor. split; [exact result|tauto].
  - pose proof (contribution_cuts_cover_every_capped_vector sources obligations eligible capacity demand contribution
      cuts capped (fun obligation => negb (discovery_known state (funding_obligation_vertex sources obligation)))).
    congruence.
Qed.

Definition combined_restriction_edge source obligation :=
  negb (Nat.eqb source 0) || Nat.eqb obligation 2.

Definition combined_restriction_capacity source := if Nat.eqb source 0 then 3 else 6.

Example combined_restriction_passes_single_obligation_capacity_checks :
  forall obligation, obligation < 3 ->
    source_subset_capacity 2 (fun source => negb (combined_restriction_edge source obligation))
      combined_restriction_capacity <= 6 - 2.
Proof.
  intros obligation inside. destruct obligation as [|[|[|obligation]]]; try lia; vm_compute; lia.
Qed.

Example combined_restriction_has_a_transposed_deficit :
  funding_deficit_check 3 2 (fun obligation source => combined_restriction_edge source obligation)
    (fun _ => 2)
    (fun source => if combined_restriction_edge source 0 then 0 else combined_restriction_capacity source)
    (fun source => Nat.eqb source 0) = true.
Proof. reflexivity. Qed.

Example combined_restriction_has_a_capped_counterexample :
  capped_contribution 2 combined_restriction_capacity 6 (fun _ => 3) /\
  funding_deficit_check 2 3 combined_restriction_edge (fun _ => 3) (fun _ => 2)
    (fun obligation => obligation <? 2) = true.
Proof.
  split; [split|reflexivity]; [reflexivity|].
  intros source inside. unfold combined_restriction_capacity.
  destruct (Nat.eqb source 0); lia.
Qed.

Print Assumptions contribution_cut_domain_iff_excluded_source_cuts.
Print Assumptions excluded_source_cuts_iff_transposed_cuts.
Print Assumptions contribution_cuts_iff_transposed_cuts.
Print Assumptions transposed_assignments_certify_contribution_cuts.
Print Assumptions zero_demand_has_unrestricted_contribution_cuts.
Print Assumptions contribution_cuts_cover_every_capped_vector.
Print Assumptions certified_transposed_flows_exclude_every_contribution_deficit.
Print Assumptions certified_capped_vector_search_reaches_sink.
