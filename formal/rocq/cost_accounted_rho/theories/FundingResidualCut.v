From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import EligibleFundingAssignment FundingDeficitCertificate CompleteFundingCandidates.

Definition partial_funding_valid sources obligations eligible capacity demand flow :=
  assignment_valid sources obligations eligible capacity (obligation_draw sources flow) flow /\
  forall obligation, obligation < obligations -> obligation_draw sources flow obligation <= demand obligation.

Definition residual_cut_closed sources obligations eligible capacity demand flow
    (reached_source reached_obligation : nat -> bool) :=
  (forall source, source < sources -> reached_source source = false ->
    source_draw obligations flow source = Nat.min (capacity source) (funding_sum obligations demand)) /\
  (forall source obligation, source < sources -> obligation < obligations ->
    reached_source source = true -> eligible source obligation = true ->
    flow source obligation < funding_sum obligations demand -> reached_obligation obligation = true) /\
  (forall source obligation, source < sources -> obligation < obligations ->
    reached_obligation obligation = true -> 0 < flow source obligation -> reached_source source = true) /\
  (forall obligation, obligation < obligations -> reached_obligation obligation = true ->
    obligation_draw sources flow obligation = demand obligation).

Lemma funding_sum_equal_bounded_entries : forall count lhs rhs,
  (forall index, index < count -> lhs index <= rhs index) ->
  funding_sum count lhs = funding_sum count rhs ->
  forall index, index < count -> lhs index = rhs index.
Proof.
  induction count; intros lhs rhs bounded equal index inside; [lia|].
  simpl in equal.
  assert (prefix : funding_sum count lhs <= funding_sum count rhs).
  { apply funding_sum_monotone. intros. apply bounded. lia. }
  pose proof (bounded count ltac:(lia)) as last.
  destruct (Nat.eq_dec index count) as [->|different]; [lia|].
  apply IHcount; try lia. intros. apply bounded. lia.
Qed.

Theorem complete_partial_assignment_is_valid : forall sources obligations eligible capacity demand flow,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  funding_sum obligations (obligation_draw sources flow) = funding_sum obligations demand ->
  assignment_valid sources obligations eligible capacity demand flow.
Proof.
  intros sources obligations eligible capacity demand flow [[rows _] bounded] equal.
  split; [exact rows|]. eapply funding_sum_equal_bounded_entries; eauto.
Qed.

Theorem total_clipped_capacity_preserves_assignments : forall sources obligations eligible capacity demand flow,
  assignment_valid sources obligations eligible capacity demand flow <->
  assignment_valid sources obligations eligible
    (fun source => Nat.min (capacity source) (funding_sum obligations demand)) demand flow.
Proof.
  intros sources obligations eligible capacity demand flow. split; intros valid.
  - pose proof (accepted_assignment_conserves_obligation _ _ _ _ _ _ valid) as total.
    destruct valid as [rows columns]. split; [|exact columns].
    intros source inside. destruct (rows source inside) as [bounded edges].
    split; [|exact edges]. apply Nat.min_glb; [exact bounded|].
    pose proof (funding_sum_contains_entry sources (source_draw obligations flow) source inside). lia.
  - destruct valid as [rows columns]. split; [|exact columns].
    intros source inside. destruct (rows source inside) as [bounded edges]. split; [|exact edges].
    eapply Nat.le_trans; [exact bounded|apply Nat.le_min_l].
Qed.

Lemma partial_funding_source_below_total : forall sources obligations demand flow source,
  funding_sum obligations (obligation_draw sources flow) < funding_sum obligations demand ->
  source < sources -> source_draw obligations flow source < funding_sum obligations demand.
Proof.
  intros sources obligations demand flow source short inside.
  pose proof (funding_sum_contains_entry sources (source_draw obligations flow) source inside) as bound.
  rewrite funding_assignment_rows_equal_columns in bound. lia.
Qed.

Lemma partial_funding_edge_below_total : forall sources obligations demand flow source obligation,
  funding_sum obligations (obligation_draw sources flow) < funding_sum obligations demand ->
  source < sources -> obligation < obligations -> flow source obligation < funding_sum obligations demand.
Proof.
  intros sources obligations demand flow source obligation short source_inside obligation_inside.
  pose proof (partial_funding_source_below_total sources obligations demand flow source short source_inside).
  pose proof (funding_sum_contains_entry obligations (flow source) obligation obligation_inside).
  unfold source_draw in *. lia.
Qed.

Theorem unreachable_source_uses_original_capacity : forall sources obligations eligible capacity demand flow reached_source reached_obligation source,
  residual_cut_closed sources obligations eligible capacity demand flow reached_source reached_obligation ->
  funding_sum obligations (obligation_draw sources flow) < funding_sum obligations demand ->
  source < sources -> reached_source source = false ->
  source_draw obligations flow source = capacity source.
Proof.
  intros sources obligations eligible capacity demand flow reached_source reached_obligation source [saturated _] short inside absent.
  specialize (saturated source inside absent).
  pose proof (partial_funding_source_below_total sources obligations demand flow source short inside).
  destruct (Nat.le_ge_cases (capacity source) (funding_sum obligations demand)) as [small|large].
  - rewrite Nat.min_l in saturated by exact small. exact saturated.
  - rewrite Nat.min_r in saturated by exact large. lia.
Qed.

Theorem selected_neighbor_is_unreachable : forall sources obligations eligible capacity demand flow reached_source reached_obligation source,
  residual_cut_closed sources obligations eligible capacity demand flow reached_source reached_obligation ->
  funding_sum obligations (obligation_draw sources flow) < funding_sum obligations demand ->
  source < sources ->
  funding_neighbor obligations eligible (fun obligation => negb (reached_obligation obligation)) source = true ->
  reached_source source = false.
Proof.
  intros sources obligations eligible capacity demand flow reached_source reached_obligation source [_ [forward _]] short inside neighbor.
  apply funding_neighbor_exact in neighbor.
  destruct neighbor as [obligation [bounded [selected permitted]]].
  apply negb_true_iff in selected.
  destruct (reached_source source) eqn:reached; [|reflexivity].
  assert (edge_small : flow source obligation < funding_sum obligations demand).
  { eapply partial_funding_edge_below_total; eauto. }
  specialize (forward source obligation inside bounded reached permitted edge_small).
  congruence.
Qed.

Theorem unreachable_source_only_funds_selected_obligations : forall sources obligations eligible capacity demand flow reached_source reached_obligation source,
  residual_cut_closed sources obligations eligible capacity demand flow reached_source reached_obligation ->
  source < sources -> reached_source source = false ->
  source_draw obligations
    (selected_funding_flow (fun obligation => negb (reached_obligation obligation)) flow) source =
  source_draw obligations flow source.
Proof.
  intros sources obligations eligible capacity demand flow reached_source reached_obligation source [_ [_ [reverse _]]] inside absent.
  unfold source_draw. apply funding_sum_ext. intros obligation bounded.
  unfold selected_funding_flow. destruct (reached_obligation obligation) eqn:reached; simpl; [|reflexivity].
  destruct (Nat.eq_dec (flow source obligation) 0) as [zero|positive]; [lia|].
  assert (reached_source source = true).
  { apply (reverse source obligation); auto; lia. }
  congruence.
Qed.

Theorem selected_partial_row_equals_neighbor_capacity : forall sources obligations eligible capacity demand flow reached_source reached_obligation source,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  residual_cut_closed sources obligations eligible capacity demand flow reached_source reached_obligation ->
  funding_sum obligations (obligation_draw sources flow) < funding_sum obligations demand -> source < sources ->
  source_draw obligations
    (selected_funding_flow (fun obligation => negb (reached_obligation obligation)) flow) source =
  neighbor_funding_capacity obligations eligible (fun obligation => negb (reached_obligation obligation)) capacity source.
Proof.
  intros sources obligations eligible capacity demand flow reached_source reached_obligation source [valid _] closed short inside.
  unfold neighbor_funding_capacity.
  destruct (funding_neighbor obligations eligible (fun obligation => negb (reached_obligation obligation)) source) eqn:neighbor.
  - assert (absent : reached_source source = false).
    { eapply selected_neighbor_is_unreachable; eauto. }
    erewrite unreachable_source_only_funds_selected_obligations; eauto.
    eapply unreachable_source_uses_original_capacity; eauto.
  - eapply nonneighbor_selected_flow_zero; eauto.
Qed.

Lemma funding_sum_partition : forall count selected amount,
  funding_sum count amount =
    funding_sum count (selected_funding_demand selected amount) +
    funding_sum count (selected_funding_demand (fun index => negb (selected index)) amount).
Proof.
  intros. rewrite <- funding_sum_add. apply funding_sum_ext.
  intros index _. unfold selected_funding_demand. destruct (selected index); simpl; lia.
Qed.

Theorem incomplete_residual_cut_has_checked_deficit : forall sources obligations eligible capacity demand flow reached_source reached_obligation,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  residual_cut_closed sources obligations eligible capacity demand flow reached_source reached_obligation ->
  funding_sum obligations (obligation_draw sources flow) < funding_sum obligations demand ->
  funding_deficit_check sources obligations eligible capacity demand (fun obligation => negb (reached_obligation obligation)) = true.
Proof.
  intros sources obligations eligible capacity demand flow reached_source reached_obligation partial closed short.
  set (selected := fun obligation => negb (reached_obligation obligation)).
  assert (rows : funding_sum sources (neighbor_funding_capacity obligations eligible selected capacity) =
    funding_sum sources (source_draw obligations (selected_funding_flow selected flow))).
  { apply funding_sum_ext. intros source inside. symmetry.
    eapply selected_partial_row_equals_neighbor_capacity; eauto. }
  rewrite funding_assignment_rows_equal_columns in rows.
  assert (columns : funding_sum obligations (obligation_draw sources (selected_funding_flow selected flow)) =
    funding_sum obligations (selected_funding_demand selected (obligation_draw sources flow))).
  { apply funding_sum_ext. intros obligation inside.
    eapply selected_funding_columns_exact; [exact (proj1 partial)|exact inside]. }
  assert (complement :
    funding_sum obligations (selected_funding_demand (fun index => negb (selected index)) (obligation_draw sources flow)) =
    funding_sum obligations (selected_funding_demand (fun index => negb (selected index)) demand)).
  { apply funding_sum_ext. intros obligation inside.
    unfold selected_funding_demand, selected.
    destruct (reached_obligation obligation) eqn:reached; simpl; [|reflexivity].
    apply (proj2 (proj2 (proj2 closed))); assumption. }
  pose proof (funding_sum_partition obligations selected (obligation_draw sources flow)) as flow_partition.
  pose proof (funding_sum_partition obligations selected demand) as demand_partition.
  unfold funding_deficit_check. apply Nat.ltb_lt.
  change (funding_sum sources (neighbor_funding_capacity obligations eligible selected capacity) <
    funding_sum obligations (selected_funding_demand selected demand)).
  rewrite rows, columns. lia.
Qed.

Theorem incomplete_residual_cut_excludes_every_valid_assignment : forall sources obligations eligible capacity demand flow reached_source reached_obligation,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  residual_cut_closed sources obligations eligible capacity demand flow reached_source reached_obligation ->
  funding_sum obligations (obligation_draw sources flow) < funding_sum obligations demand ->
  ~ exists assignment, assignment_valid sources obligations eligible capacity demand assignment.
Proof.
  intros sources obligations eligible capacity demand flow reached_source reached_obligation partial closed short.
  apply (funding_deficit_certificate_sound sources obligations eligible capacity demand
    (fun obligation => negb (reached_obligation obligation))).
  eapply incomplete_residual_cut_has_checked_deficit; eauto.
Qed.

Print Assumptions unreachable_source_uses_original_capacity.
Print Assumptions complete_partial_assignment_is_valid.
Print Assumptions total_clipped_capacity_preserves_assignments.
Print Assumptions selected_neighbor_is_unreachable.
Print Assumptions unreachable_source_only_funds_selected_obligations.
Print Assumptions selected_partial_row_equals_neighbor_capacity.
Print Assumptions incomplete_residual_cut_has_checked_deficit.
Print Assumptions incomplete_residual_cut_excludes_every_valid_assignment.
