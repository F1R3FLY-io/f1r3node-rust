From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import EligibleFundingAssignment FundingDeficitCertificate FundingResidualCut FundingDomainCuts.

Fixpoint fill_funding_capacity count (capacity : nat -> nat) total : nat -> nat :=
  match count with
  | 0 => fun _ => 0
  | S rest => fun source =>
      if Nat.eqb source rest then Nat.min total (capacity rest)
      else fill_funding_capacity rest capacity (total - Nat.min total (capacity rest)) source
  end.

Theorem fill_funding_capacity_respects_each_capacity : forall count capacity total source,
  fill_funding_capacity count capacity total source <= capacity source.
Proof.
  induction count; intros capacity total source; simpl; [lia|].
  destruct (Nat.eqb_spec source count) as [->|different].
  - apply Nat.le_min_r.
  - apply IHcount.
Qed.

Theorem fill_funding_capacity_has_exact_total : forall count capacity total,
  funding_sum count (fill_funding_capacity count capacity total) =
  Nat.min total (funding_sum count capacity).
Proof.
  induction count; intros capacity total; simpl; [lia|].
  rewrite Nat.eqb_refl.
  assert (prefix : funding_sum count
      (fun source => if Nat.eqb source count then Nat.min total (capacity count)
        else fill_funding_capacity count capacity (total - Nat.min total (capacity count)) source) =
    funding_sum count (fill_funding_capacity count capacity (total - Nat.min total (capacity count)))).
  { apply funding_sum_ext. intros source inside.
    destruct (Nat.eqb_spec source count); [lia|reflexivity]. }
  rewrite prefix, IHcount.
  destruct (Nat.le_ge_cases total (capacity count)) as [small|large].
  - rewrite (Nat.min_l total (capacity count) small). replace (total - total) with 0 by lia. simpl.
    rewrite Nat.min_l by lia. lia.
  - rewrite (Nat.min_r total (capacity count) large).
    destruct (Nat.le_ge_cases (total - capacity count) (funding_sum count capacity)) as [fits|exceeds].
    + rewrite Nat.min_l by exact fits. rewrite Nat.min_l by lia. lia.
    + rewrite Nat.min_r by exact exceeds. rewrite Nat.min_r by lia. lia.
Qed.

Definition priority_funding_fill sources selected capacity total source :=
  fill_funding_capacity sources (selected_funding_demand selected capacity) total source +
  fill_funding_capacity sources
    (selected_funding_demand (fun index => negb (selected index)) capacity)
    (total - source_subset_capacity sources selected capacity) source.

Theorem priority_funding_fill_respects_capacity : forall sources selected capacity total source,
  priority_funding_fill sources selected capacity total source <= capacity source.
Proof.
  intros sources selected capacity total source.
  pose proof (fill_funding_capacity_respects_each_capacity sources
    (selected_funding_demand selected capacity) total source) as first.
  pose proof (fill_funding_capacity_respects_each_capacity sources
    (selected_funding_demand (fun index => negb (selected index)) capacity)
    (total - source_subset_capacity sources selected capacity) source) as second.
  unfold priority_funding_fill. unfold selected_funding_demand in *.
  destruct (selected source); simpl in *; lia.
Qed.

Theorem priority_funding_fill_has_required_total : forall sources selected capacity total,
  total <= funding_sum sources capacity ->
  funding_sum sources (priority_funding_fill sources selected capacity total) = total.
Proof.
  intros sources selected capacity total enough.
  unfold priority_funding_fill. rewrite funding_sum_add.
  repeat rewrite fill_funding_capacity_has_exact_total.
  pose proof (funding_sum_partition sources selected capacity) as partition.
  unfold source_subset_capacity.
  destruct (Nat.le_ge_cases total (funding_sum sources (selected_funding_demand selected capacity))) as [small|large].
  - rewrite Nat.min_l by exact small.
    replace (total - funding_sum sources (selected_funding_demand selected capacity)) with 0 by lia.
    simpl. lia.
  - rewrite Nat.min_r by exact large. rewrite Nat.min_l by lia. lia.
Qed.

Theorem priority_funding_fill_maximizes_selected_contribution : forall sources selected capacity total,
  source_subset_capacity sources selected (priority_funding_fill sources selected capacity total) =
  Nat.min total (source_subset_capacity sources selected capacity).
Proof.
  intros sources selected capacity total.
  unfold source_subset_capacity at 1.
  transitivity (funding_sum sources
    (fill_funding_capacity sources (selected_funding_demand selected capacity) total)).
  - apply funding_sum_ext. intros source inside.
    pose proof (fill_funding_capacity_respects_each_capacity sources
      (selected_funding_demand selected capacity) total source) as first.
    pose proof (fill_funding_capacity_respects_each_capacity sources
      (selected_funding_demand (fun index => negb (selected index)) capacity)
      (total - source_subset_capacity sources selected capacity) source) as second.
    unfold selected_funding_demand in first, second.
    unfold priority_funding_fill, selected_funding_demand.
    destruct (selected source); simpl in *; lia.
  - apply fill_funding_capacity_has_exact_total.
Qed.

Theorem violated_source_cut_produces_checked_deficit : forall sources obligations eligible demand contribution selected,
  funding_sum sources contribution = funding_sum obligations demand ->
  source_neighborhood_demand sources obligations eligible selected demand <
    source_subset_capacity sources selected contribution ->
  funding_deficit_check sources obligations eligible contribution demand
    (fun obligation => negb (source_neighborhood sources eligible selected obligation)) = true.
Proof.
  intros sources obligations eligible demand contribution selected total violates.
  set (outside := fun obligation => negb (source_neighborhood sources eligible selected obligation)).
  assert (neighbor_bound : funding_sum sources (neighbor_funding_capacity obligations eligible outside contribution) <=
    funding_sum sources (selected_funding_demand (fun source => negb (selected source)) contribution)).
  { apply funding_sum_monotone. intros source inside.
    unfold neighbor_funding_capacity, selected_funding_demand.
    destruct (funding_neighbor obligations eligible outside source) eqn:neighbor; [|lia].
    assert (selected source = false).
    { apply funding_neighbor_exact in neighbor.
      destruct neighbor as [obligation [bounded [chosen permitted]]].
      unfold outside in chosen. apply negb_true_iff in chosen.
      destruct (selected source) eqn:selected_at; [|reflexivity].
      assert (source_neighborhood sources eligible selected obligation = true).
      { apply funding_neighbor_exact. exists source. auto. }
      congruence. }
    rewrite H. simpl. lia. }
  pose proof (funding_sum_partition sources selected contribution) as source_partition.
  pose proof (funding_sum_partition obligations (source_neighborhood sources eligible selected) demand) as demand_partition.
  unfold funding_deficit_check. apply Nat.ltb_lt.
  unfold source_subset_capacity, source_neighborhood_demand in violates.
  unfold outside in *. lia.
Qed.

Theorem violated_contribution_cut_has_capped_counterexample : forall sources obligations eligible capacity demand selected,
  funding_sum obligations demand <= funding_sum sources capacity ->
  source_neighborhood_demand sources obligations eligible selected demand <
    Nat.min (funding_sum obligations demand) (source_subset_capacity sources selected capacity) ->
  capped_contribution sources capacity (funding_sum obligations demand)
    (priority_funding_fill sources selected capacity (funding_sum obligations demand)) /\
  funding_deficit_check sources obligations eligible
    (priority_funding_fill sources selected capacity (funding_sum obligations demand)) demand
    (fun obligation => negb (source_neighborhood sources eligible selected obligation)) = true.
Proof.
  intros sources obligations eligible capacity demand selected enough violates.
  split.
  - split.
    + now apply priority_funding_fill_has_required_total.
    + intros. apply priority_funding_fill_respects_capacity.
  - apply violated_source_cut_produces_checked_deficit.
    + now apply priority_funding_fill_has_required_total.
    + rewrite priority_funding_fill_maximizes_selected_contribution. exact violates.
Qed.

Theorem contribution_domain_coverage_requires_all_cuts : forall sources obligations eligible capacity demand,
  funding_sum obligations demand <= funding_sum sources capacity ->
  (forall contribution,
    capped_contribution sources capacity (funding_sum obligations demand) contribution ->
    exists flow, assignment_valid sources obligations eligible contribution demand flow) ->
  contribution_cut_domain sources obligations eligible capacity demand.
Proof.
  intros sources obligations eligible capacity demand enough covers selected.
  destruct (Nat.le_gt_cases
    (Nat.min (funding_sum obligations demand) (source_subset_capacity sources selected capacity))
    (source_neighborhood_demand sources obligations eligible selected demand)) as [safe|unsafe]; [exact safe|].
  destruct (violated_contribution_cut_has_capped_counterexample sources obligations eligible capacity demand selected
    enough unsafe) as [capped deficit].
  exfalso. eapply funding_deficit_certificate_sound; [exact deficit|].
  apply covers. exact capped.
Qed.

Theorem transposed_deficit_produces_violated_contribution_cut : forall sources obligations eligible capacity demand obligation selected,
  obligation < obligations -> 0 < demand obligation ->
  funding_deficit_check obligations sources (fun target source => eligible source target) demand
    (fun source => if eligible source obligation then 0 else capacity source) selected = true ->
  source_neighborhood_demand sources obligations eligible
    (fun source => selected source && negb (eligible source obligation)) demand <
  Nat.min (funding_sum obligations demand)
    (source_subset_capacity sources (fun source => selected source && negb (eligible source obligation)) capacity).
Proof.
  intros sources obligations eligible capacity demand obligation selected inside positive deficit.
  set (filtered := fun source => selected source && negb (eligible source obligation)).
  assert (excluded : forall source, source < sources -> filtered source = true -> eligible source obligation = false).
  { intros source bounded chosen. unfold filtered in chosen. apply andb_true_iff in chosen.
    apply negb_true_iff. tauto. }
  pose proof (omitted_positive_obligation_makes_cut_proper sources obligations eligible filtered demand
    obligation inside positive excluded) as proper.
  assert (same : funding_sum sources
    (selected_funding_demand selected (fun source => if eligible source obligation then 0 else capacity source)) =
    source_subset_capacity sources filtered capacity).
  { apply funding_sum_ext. intros source bounded. unfold selected_funding_demand, filtered.
    destruct (selected source), (eligible source obligation); reflexivity. }
  unfold funding_deficit_check in deficit. apply Nat.ltb_lt in deficit. rewrite same in deficit.
  assert (neighborhood : source_neighborhood_demand sources obligations eligible filtered demand <=
    funding_sum obligations (neighbor_funding_capacity sources
      (fun target source => eligible source target) selected demand)).
  { apply funding_sum_monotone. intros target bounded.
    unfold selected_funding_demand, neighbor_funding_capacity.
    destruct (source_neighborhood sources eligible filtered target) eqn:neighbor; [|lia].
    assert (source_neighborhood sources eligible selected target = true).
    { eapply source_neighborhood_monotone; [|exact neighbor].
      intros source source_inside chosen. unfold filtered in chosen. apply andb_true_iff in chosen. tauto. }
    unfold source_neighborhood in H. now rewrite H. }
  destruct (Nat.le_ge_cases (funding_sum obligations demand) (source_subset_capacity sources filtered capacity)) as [small|large].
  - rewrite Nat.min_l by exact small. exact proper.
  - rewrite Nat.min_r by exact large. lia.
Qed.

Theorem transposed_deficit_has_capped_counterexample : forall sources obligations eligible capacity demand obligation selected,
  funding_sum obligations demand <= funding_sum sources capacity ->
  obligation < obligations -> 0 < demand obligation ->
  funding_deficit_check obligations sources (fun target source => eligible source target) demand
    (fun source => if eligible source obligation then 0 else capacity source) selected = true ->
  let filtered := fun source => selected source && negb (eligible source obligation) in
  let contribution := priority_funding_fill sources filtered capacity (funding_sum obligations demand) in
  capped_contribution sources capacity (funding_sum obligations demand) contribution /\
  funding_deficit_check sources obligations eligible contribution demand
    (fun target => negb (source_neighborhood sources eligible filtered target)) = true.
Proof.
  intros sources obligations eligible capacity demand obligation selected enough inside positive deficit.
  apply violated_contribution_cut_has_capped_counterexample; [exact enough|].
  eapply transposed_deficit_produces_violated_contribution_cut; eauto.
Qed.

Theorem omitted_obligation_bounds_neighborhood_demand : forall sources obligations eligible selected demand obligation,
  obligation < obligations ->
  (forall source, source < sources -> selected source = true -> eligible source obligation = false) ->
  source_neighborhood_demand sources obligations eligible selected demand + demand obligation <= funding_sum obligations demand.
Proof.
  intros sources obligations eligible selected demand obligation inside excluded.
  assert (absent : source_neighborhood sources eligible selected obligation = false).
  { destruct (source_neighborhood sources eligible selected obligation) eqn:neighbor; [|reflexivity].
    apply funding_neighbor_exact in neighbor. destruct neighbor as [source [bounded [chosen permitted]]].
    specialize (excluded source bounded chosen). congruence. }
  pose proof (CompleteFundingCandidates.funding_sum_contains_entry obligations
    (selected_funding_demand (fun target => negb (source_neighborhood sources eligible selected target)) demand)
    obligation inside) as remaining.
  unfold selected_funding_demand at 1 in remaining. rewrite absent in remaining. simpl in remaining.
  pose proof (funding_sum_partition obligations (source_neighborhood sources eligible selected) demand) as partition.
  unfold source_neighborhood_demand. lia.
Qed.

Theorem excluded_capacity_shortcut_has_violated_cut : forall sources obligations eligible capacity demand selected obligation,
  obligation < obligations -> 0 < demand obligation ->
  (forall source, source < sources -> selected source = true -> eligible source obligation = false) ->
  funding_sum obligations demand - demand obligation < source_subset_capacity sources selected capacity ->
  source_neighborhood_demand sources obligations eligible selected demand <
    Nat.min (funding_sum obligations demand) (source_subset_capacity sources selected capacity).
Proof.
  intros sources obligations eligible capacity demand selected obligation inside positive excluded large.
  pose proof (omitted_obligation_bounds_neighborhood_demand sources obligations eligible selected demand obligation inside excluded).
  destruct (Nat.le_ge_cases (funding_sum obligations demand) (source_subset_capacity sources selected capacity)) as [first|second].
  - rewrite Nat.min_l by exact first. lia.
  - rewrite Nat.min_r by exact second. lia.
Qed.

Print Assumptions fill_funding_capacity_respects_each_capacity.
Print Assumptions fill_funding_capacity_has_exact_total.
Print Assumptions priority_funding_fill_respects_capacity.
Print Assumptions priority_funding_fill_has_required_total.
Print Assumptions priority_funding_fill_maximizes_selected_contribution.
Print Assumptions violated_source_cut_produces_checked_deficit.
Print Assumptions violated_contribution_cut_has_capped_counterexample.
Print Assumptions contribution_domain_coverage_requires_all_cuts.
Print Assumptions transposed_deficit_produces_violated_contribution_cut.
Print Assumptions transposed_deficit_has_capped_counterexample.
Print Assumptions omitted_obligation_bounds_neighborhood_demand.
Print Assumptions excluded_capacity_shortcut_has_violated_cut.
