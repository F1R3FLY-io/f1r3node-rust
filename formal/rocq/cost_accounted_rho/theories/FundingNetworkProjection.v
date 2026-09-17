From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List ZArith Lia Ring.
From CostAccountedRho Require Import EligibleFundingAssignment FundingDeficitCertificate FundingResidualCut FundingResidualTransfer FundingResidualGraph FundingNetworkAugmentation.

Inductive funding_pair_kind :=
| PayerCapacity (source : nat)
| EligibleAssignment (source obligation : nat)
| ObligationCapacity (obligation : nat).

Definition pair_from sources (kind : funding_pair_kind) :=
  match kind with
  | PayerCapacity _ => 0
  | EligibleAssignment source _ => S source
  | ObligationCapacity obligation => funding_obligation_vertex sources obligation
  end.

Definition pair_to sources obligations (kind : funding_pair_kind) :=
  match kind with
  | PayerCapacity source => S source
  | EligibleAssignment _ obligation => funding_obligation_vertex sources obligation
  | ObligationCapacity _ => funding_sink sources obligations
  end.

Definition kind_valid sources obligations eligible (kind : funding_pair_kind) :=
  match kind with
  | PayerCapacity source => source < sources
  | EligibleAssignment source obligation => source < sources /\ obligation < obligations /\ eligible source obligation = true
  | ObligationCapacity obligation => obligation < obligations
  end.

Definition projected_assignment count kinds (state : residual_network) source obligation :=
  funding_sum count (fun index =>
    match kinds index with
    | EligibleAssignment i j => if Nat.eqb source i && Nat.eqb obligation j then snd (state index) else 0
    | _ => 0
    end).

Definition projected_incoming count kinds (value : nat -> nat) source :=
  funding_sum count (fun index =>
    match kinds index with PayerCapacity i => if Nat.eqb source i then value index else 0 | _ => 0 end).

Definition projected_outgoing count kinds (value : nat -> nat) obligation :=
  funding_sum count (fun index =>
    match kinds index with ObligationCapacity j => if Nat.eqb obligation j then value index else 0 | _ => 0 end).

Lemma funding_sum_indicator : forall count selected amount,
  selected < count -> funding_sum count (fun index => if Nat.eqb index selected then amount else 0) = amount.
Proof.
  induction count as [|count IH]; intros selected amount inside; [lia|].
  simpl. destruct (Nat.eq_dec count selected) as [equal|different].
  - subst selected. rewrite Nat.eqb_refl.
    assert (zero : funding_sum count (fun index => if Nat.eqb index count then amount else 0) = 0).
    { transitivity (funding_sum count (fun _ => 0)); [|apply funding_sum_zero].
      apply funding_sum_ext. intros index bounded.
      assert (Nat.eqb index count = false) by (apply Nat.eqb_neq; lia). now rewrite H. }
    rewrite zero. lia.
  - assert (Nat.eqb count selected = false) by (apply Nat.eqb_neq; exact different).
    rewrite H, IH by lia. lia.
Qed.

Lemma projected_row_step : forall count obligations kinds state source,
  (forall index, index <= count -> match kinds index with EligibleAssignment _ j => j < obligations | _ => True end) ->
  source_draw obligations (projected_assignment (S count) kinds state) source =
    source_draw obligations (projected_assignment count kinds state) source +
    match kinds count with EligibleAssignment i _ => if Nat.eqb source i then snd (state count) else 0 | _ => 0 end.
Proof.
  intros count obligations kinds state source valid.
  unfold source_draw, projected_assignment. simpl funding_sum.
  rewrite funding_sum_add. f_equal.
  specialize (valid count ltac:(lia)). destruct (kinds count) as [i|i j|j]; try apply funding_sum_zero.
  destruct (Nat.eqb source i); simpl; [apply funding_sum_indicator; exact valid|apply funding_sum_zero].
Qed.

Lemma projected_column_step : forall count sources kinds state obligation,
  (forall index, index <= count -> match kinds index with EligibleAssignment i _ => i < sources | _ => True end) ->
  obligation_draw sources (projected_assignment (S count) kinds state) obligation =
    obligation_draw sources (projected_assignment count kinds state) obligation +
    match kinds count with EligibleAssignment _ j => if Nat.eqb obligation j then snd (state count) else 0 | _ => 0 end.
Proof.
  intros count sources kinds state obligation valid.
  unfold obligation_draw, projected_assignment. simpl funding_sum. rewrite funding_sum_add. f_equal.
  specialize (valid count ltac:(lia)). destruct (kinds count) as [i|i j|j]; try apply funding_sum_zero.
  destruct (Nat.eqb obligation j) eqn:equal.
  - transitivity (funding_sum sources (fun source => if Nat.eqb source i then snd (state count) else 0)).
    + apply funding_sum_ext. intros source inside. now destruct (Nat.eqb source i).
    + apply funding_sum_indicator. exact valid.
  - transitivity (funding_sum sources (fun _ => 0)); [|apply funding_sum_zero].
    apply funding_sum_ext. intros source inside. now destruct (Nat.eqb source i).
Qed.

Lemma payer_edge_boundary : forall sources obligations kind source,
  source < sources ->
  (match kind with PayerCapacity i => i < sources | EligibleAssignment i j => i < sources /\ j < obligations | ObligationCapacity j => j < obligations end) ->
  (vertex_indicator (S source) (pair_from sources kind) - vertex_indicator (S source) (pair_to sources obligations kind))%Z =
  match kind with
  | PayerCapacity i => if Nat.eqb source i then (-1)%Z else 0%Z
  | EligibleAssignment i _ => if Nat.eqb source i then 1%Z else 0%Z
  | ObligationCapacity _ => 0%Z
  end.
Proof.
  intros sources obligations kind source inside valid.
  unfold pair_from, pair_to, vertex_indicator, funding_obligation_vertex, funding_sink.
  destruct kind as [i|i j|j]; cbn [pair_from pair_to] in *.
  - change ((0 - (if Nat.eqb source i then 1 else 0))%Z = (if Nat.eqb source i then (-1)%Z else 0%Z)).
    destruct (Nat.eqb source i); reflexivity.
  - assert (Nat.eqb (S source) (sources + 1 + j) = false) by (apply Nat.eqb_neq; lia).
    rewrite H. simpl. destruct (Nat.eqb source i); reflexivity.
  - assert (Nat.eqb (S source) (sources + 1 + j) = false) by (apply Nat.eqb_neq; lia).
    assert (Nat.eqb (S source) (sources + obligations + 1) = false) by (apply Nat.eqb_neq; lia).
    now rewrite H, H0.
Qed.

Lemma obligation_edge_boundary : forall sources obligations kind obligation,
  obligation < obligations ->
  (match kind with PayerCapacity i => i < sources | EligibleAssignment i j => i < sources /\ j < obligations | ObligationCapacity j => j < obligations end) ->
  (vertex_indicator (funding_obligation_vertex sources obligation) (pair_from sources kind) -
    vertex_indicator (funding_obligation_vertex sources obligation) (pair_to sources obligations kind))%Z =
  match kind with
  | PayerCapacity _ => 0%Z
  | EligibleAssignment _ j => if Nat.eqb obligation j then (-1)%Z else 0%Z
  | ObligationCapacity j => if Nat.eqb obligation j then 1%Z else 0%Z
  end.
Proof.
  intros sources obligations kind obligation inside valid.
  unfold pair_from, pair_to, vertex_indicator, funding_obligation_vertex, funding_sink.
  assert (zero : Nat.eqb (sources + 1 + obligation) 0 = false) by (apply Nat.eqb_neq; lia).
  assert (sink : Nat.eqb (sources + 1 + obligation) (sources + obligations + 1) = false) by (apply Nat.eqb_neq; lia).
  assert (same : forall j, Nat.eqb (sources + 1 + obligation) (sources + 1 + j) = Nat.eqb obligation j).
  { intros. destruct (Nat.eqb obligation j) eqn:equal.
    - apply Nat.eqb_eq in equal. subst. apply Nat.eqb_refl.
    - apply Nat.eqb_neq. apply Nat.eqb_neq in equal. lia. }
  destruct kind as [i|i j|j]; cbn [pair_from pair_to] in *.
  - assert (Nat.eqb (sources + 1 + obligation) (S i) = false) by (apply Nat.eqb_neq; lia). now rewrite zero, H.
  - assert (Nat.eqb (sources + 1 + obligation) (S i) = false) by (apply Nat.eqb_neq; lia).
    rewrite H, same. destruct (Nat.eqb obligation j); reflexivity.
  - rewrite same, sink. destruct (Nat.eqb obligation j); reflexivity.
Qed.

Lemma valid_kind_bounds : forall sources obligations eligible kind,
  kind_valid sources obligations eligible kind ->
  match kind with PayerCapacity i => i < sources | EligibleAssignment i j => i < sources /\ j < obligations | ObligationCapacity j => j < obligations end.
Proof. intros. destruct kind; simpl in *; tauto. Qed.

Theorem network_payer_divergence_is_row_balance : forall count sources obligations kinds state eligible source,
  source < sources -> (forall index, index < count -> kind_valid sources obligations eligible (kinds index)) ->
  network_divergence count (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index)) state (S source) =
  (Z.of_nat (source_draw obligations (projected_assignment count kinds state) source) -
    Z.of_nat (projected_incoming count kinds (fun index => snd (state index)) source))%Z.
Proof.
  induction count as [|count IH]; intros sources obligations kinds state eligible source inside valid.
  - unfold network_divergence, source_draw, projected_assignment, projected_incoming. simpl. rewrite funding_sum_zero. reflexivity.
  - change ((network_divergence count (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index)) state (S source) +
      Z.of_nat (snd (state count)) * (vertex_indicator (S source) (pair_from sources (kinds count)) - vertex_indicator (S source) (pair_to sources obligations (kinds count))))%Z =
      (Z.of_nat (source_draw obligations (projected_assignment (S count) kinds state) source) -
        Z.of_nat (projected_incoming (S count) kinds (fun index => snd (state index)) source))%Z).
    rewrite (IH sources obligations kinds state eligible source inside) by (intros; apply valid; lia).
    rewrite projected_row_step.
    + rewrite (payer_edge_boundary sources obligations (kinds count) source inside (valid_kind_bounds _ _ _ _ (valid count ltac:(lia)))).
      unfold projected_incoming. simpl funding_sum. rewrite !Nat2Z.inj_add.
      destruct (kinds count); simpl; try destruct (Nat.eqb source source0); ring.
    + intros index bounded. specialize (valid index ltac:(lia)). destruct (kinds index); simpl in *; tauto.
Qed.

Theorem network_obligation_divergence_is_column_balance : forall count sources obligations kinds state eligible obligation,
  obligation < obligations -> (forall index, index < count -> kind_valid sources obligations eligible (kinds index)) ->
  network_divergence count (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index)) state (funding_obligation_vertex sources obligation) =
  (Z.of_nat (projected_outgoing count kinds (fun index => snd (state index)) obligation) -
    Z.of_nat (obligation_draw sources (projected_assignment count kinds state) obligation))%Z.
Proof.
  induction count as [|count IH]; intros sources obligations kinds state eligible obligation inside valid.
  - unfold network_divergence, obligation_draw, projected_assignment, projected_outgoing. simpl. rewrite funding_sum_zero. reflexivity.
  - change ((network_divergence count (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index)) state (funding_obligation_vertex sources obligation) +
      Z.of_nat (snd (state count)) * (vertex_indicator (funding_obligation_vertex sources obligation) (pair_from sources (kinds count)) - vertex_indicator (funding_obligation_vertex sources obligation) (pair_to sources obligations (kinds count))))%Z =
      (Z.of_nat (projected_outgoing (S count) kinds (fun index => snd (state index)) obligation) -
        Z.of_nat (obligation_draw sources (projected_assignment (S count) kinds state) obligation))%Z).
    rewrite (IH sources obligations kinds state eligible obligation inside) by (intros; apply valid; lia).
    rewrite projected_column_step.
    + rewrite (obligation_edge_boundary sources obligations (kinds count) obligation inside (valid_kind_bounds _ _ _ _ (valid count ltac:(lia)))).
      unfold projected_outgoing. simpl funding_sum. rewrite !Nat2Z.inj_add.
      destruct (kinds count); simpl; try destruct (Nat.eqb obligation obligation0); ring.
    + intros index bounded. specialize (valid index ltac:(lia)). destruct (kinds index); simpl in *; tauto.
Qed.

Theorem network_projection_preserves_eligibility : forall count sources obligations kinds state eligible source obligation,
  (forall index, index < count -> kind_valid sources obligations eligible (kinds index)) ->
  eligible source obligation = false -> projected_assignment count kinds state source obligation = 0.
Proof.
  intros count sources obligations kinds state eligible source obligation valid forbidden.
  unfold projected_assignment. transitivity (funding_sum count (fun _ => 0)); [|apply funding_sum_zero].
  apply funding_sum_ext. intros index inside. specialize (valid index inside).
  destruct (kinds index) as [i|i j|j]; [reflexivity| |reflexivity].
  destruct (Nat.eqb source i && Nat.eqb obligation j) eqn:matches; [|reflexivity].
  apply andb_true_iff in matches. destruct matches as [first second].
  apply Nat.eqb_eq in first. apply Nat.eqb_eq in second. subst. simpl in valid. intuition congruence.
Qed.

Theorem conserved_network_projects_to_partial_funding : forall count sources obligations kinds state eligible capacity demand,
  (forall index, index < count -> kind_valid sources obligations eligible (kinds index)) ->
  (forall source, source < sources ->
    projected_incoming count kinds (fun index => fst (state index) + snd (state index)) source <= capacity source) ->
  (forall obligation, obligation < obligations ->
    projected_outgoing count kinds (fun index => fst (state index) + snd (state index)) obligation <= demand obligation) ->
  (forall source, source < sources ->
    network_divergence count (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index)) state (S source) = 0%Z) ->
  (forall obligation, obligation < obligations ->
    network_divergence count (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index)) state (funding_obligation_vertex sources obligation) = 0%Z) ->
  partial_funding_valid sources obligations eligible capacity demand (projected_assignment count kinds state).
Proof.
  intros count sources obligations kinds state eligible capacity demand valid source_caps obligation_caps source_conservation obligation_conservation.
  assert (rows : forall source, source < sources -> source_valid obligations eligible capacity (projected_assignment count kinds state) source).
  { intros source inside. split.
    - specialize (source_conservation source inside).
      rewrite (network_payer_divergence_is_row_balance count sources obligations kinds state eligible source inside valid) in source_conservation.
      specialize (source_caps source inside).
      assert (bound : projected_incoming count kinds (fun index => snd (state index)) source <=
        projected_incoming count kinds (fun index => fst (state index) + snd (state index)) source).
      { unfold projected_incoming. apply funding_sum_monotone. intros index bounded.
        destruct (kinds index); simpl; try destruct (Nat.eqb source source0); lia. }
      lia.
    - intros obligation bounded forbidden. eapply network_projection_preserves_eligibility; eauto. }
  split; [split; [exact rows|reflexivity]|].
  intros obligation inside. specialize (obligation_conservation obligation inside).
  rewrite (network_obligation_divergence_is_column_balance count sources obligations kinds state eligible obligation inside valid) in obligation_conservation.
  specialize (obligation_caps obligation inside).
  assert (bound : projected_outgoing count kinds (fun index => snd (state index)) obligation <=
    projected_outgoing count kinds (fun index => fst (state index) + snd (state index)) obligation).
  { unfold projected_outgoing. apply funding_sum_monotone. intros index bounded.
    destruct (kinds index); simpl; try destruct (Nat.eqb obligation obligation0); lia. }
  lia.
Qed.

Definition funding_network_valid count sources obligations kinds state eligible capacity demand :=
  (forall index, index < count -> kind_valid sources obligations eligible (kinds index)) /\
  (forall source, source < sources ->
    projected_incoming count kinds (fun index => fst (state index) + snd (state index)) source <= capacity source) /\
  (forall obligation, obligation < obligations ->
    projected_outgoing count kinds (fun index => fst (state index) + snd (state index)) obligation <= demand obligation) /\
  (forall source, source < sources ->
    network_divergence count (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index)) state (S source) = 0%Z) /\
  (forall obligation, obligation < obligations ->
    network_divergence count (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index)) state (funding_obligation_vertex sources obligation) = 0%Z).

Theorem reverse_path_preserves_funding_validity : forall count sources obligations kinds state eligible capacity demand operations limit amount,
  funding_network_valid count sources obligations kinds state eligible capacity demand ->
  network_bounded count limit state ->
  List.NoDup (List.map fst operations) ->
  (forall op, List.In op operations -> fst op < count /\ amount <= operation_capacity state op) ->
  linked_operations (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index))
    0 operations (funding_sink sources obligations) ->
  exists next,
    network_augment limit state (List.rev operations) amount = Some next /\
    funding_network_valid count sources obligations kinds next eligible capacity demand /\
    partial_funding_valid sources obligations eligible capacity demand (projected_assignment count kinds next).
Proof.
  intros count sources obligations kinds state eligible capacity demand operations limit amount valid bounded distinct permitted linked.
  destruct (complete_reverse_path_augmentation operations count limit
    (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index))
    state amount 0 (funding_sink sources obligations) distinct bounded permitted linked)
    as [next [history [next_bounded [pairs divergence]]]].
  destruct valid as [kinds_valid [source_caps [obligation_caps [sources_conserve obligations_conserve]]]].
  assert (next_source_caps : forall source, source < sources ->
    projected_incoming count kinds (fun index => fst (next index) + snd (next index)) source <= capacity source).
  { intros source inside. rewrite <- (source_caps source inside).
    unfold projected_incoming. apply funding_sum_monotone. intros index bound.
    rewrite pairs. lia. }
  assert (next_obligation_caps : forall obligation, obligation < obligations ->
    projected_outgoing count kinds (fun index => fst (next index) + snd (next index)) obligation <= demand obligation).
  { intros obligation inside. rewrite <- (obligation_caps obligation inside).
    unfold projected_outgoing. apply funding_sum_monotone. intros index bound.
    rewrite pairs. lia. }
  assert (next_sources_conserve : forall source, source < sources ->
    network_divergence count (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index)) next (S source) = 0%Z).
  { intros source inside. rewrite divergence, sources_conserve by exact inside.
    unfold residual_edge_boundary, vertex_indicator, funding_sink.
    assert (Nat.eqb (S source) 0 = false) by reflexivity.
    assert (Nat.eqb (S source) (sources + obligations + 1) = false) by (apply Nat.eqb_neq; lia).
    rewrite H, H0. ring. }
  assert (next_obligations_conserve : forall obligation, obligation < obligations ->
    network_divergence count (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index)) next (funding_obligation_vertex sources obligation) = 0%Z).
  { intros obligation inside. rewrite divergence, obligations_conserve by exact inside.
    unfold residual_edge_boundary, vertex_indicator, funding_sink, funding_obligation_vertex.
    assert (Nat.eqb (sources + 1 + obligation) 0 = false) by (apply Nat.eqb_neq; lia).
    assert (Nat.eqb (sources + 1 + obligation) (sources + obligations + 1) = false) by (apply Nat.eqb_neq; lia).
    rewrite H, H0. ring. }
  exists next. split; [exact history|]. split.
  - repeat split; assumption.
  - eapply conserved_network_projects_to_partial_funding; eauto.
Qed.

Theorem completed_funding_network_passes_checker : forall count sources obligations kinds state eligible capacity demand,
  funding_network_valid count sources obligations kinds state eligible capacity demand ->
  funding_sum obligations (obligation_draw sources (projected_assignment count kinds state)) = funding_sum obligations demand ->
  assignment_check sources obligations eligible capacity demand (projected_assignment count kinds state) = true.
Proof.
  intros count sources obligations kinds state eligible capacity demand [valid [source_caps [obligation_caps [source_conservation obligation_conservation]]]] total.
  apply assignment_check_exact. apply complete_partial_assignment_is_valid; [|exact total].
  eapply conserved_network_projects_to_partial_funding; eauto.
Qed.

Lemma projected_incoming_total : forall count sources kinds value,
  (forall index, index < count -> match kinds index with PayerCapacity i => i < sources | _ => True end) ->
  funding_sum sources (projected_incoming count kinds value) =
    funding_sum count (fun index => match kinds index with PayerCapacity _ => value index | _ => 0 end).
Proof.
  induction count as [|count IH]; intros sources kinds value valid.
  - unfold projected_incoming. simpl. apply funding_sum_zero.
  - unfold projected_incoming at 1. cbn [funding_sum]. rewrite funding_sum_add.
    fold (projected_incoming count kinds value).
    rewrite IH by (intros; apply valid; lia). f_equal.
    specialize (valid count ltac:(lia)). destruct (kinds count) as [i|i j|j];
      [apply funding_sum_indicator; exact valid|apply funding_sum_zero|apply funding_sum_zero].
Qed.

Lemma source_edge_boundary : forall sources obligations kind,
  (vertex_indicator 0 (pair_from sources kind) - vertex_indicator 0 (pair_to sources obligations kind))%Z =
  match kind with PayerCapacity _ => 1%Z | _ => 0%Z end.
Proof.
  intros. unfold vertex_indicator, pair_from, pair_to, funding_sink, funding_obligation_vertex.
  destruct kind as [i|i j|j].
  - reflexivity.
  - assert (Nat.eqb 0 (sources + 1 + j) = false) by (apply Nat.eqb_neq; lia). now rewrite H.
  - assert (Nat.eqb 0 (sources + 1 + j) = false) by (apply Nat.eqb_neq; lia).
    assert (Nat.eqb 0 (sources + obligations + 1) = false) by (apply Nat.eqb_neq; lia). now rewrite H, H0.
Qed.

Lemma source_divergence_counts_incoming : forall count sources obligations kinds state,
  network_divergence count (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index)) state 0 =
    Z.of_nat (funding_sum count (fun index => match kinds index with PayerCapacity _ => snd (state index) | _ => 0 end)).
Proof.
  induction count as [|count IH]; intros; [reflexivity|].
  unfold network_divergence in *. cbn [network_sum funding_sum].
  rewrite IH, source_edge_boundary, Nat2Z.inj_add. destruct (kinds count); ring.
Qed.

Theorem source_divergence_equals_total_assignment : forall count sources obligations kinds state eligible capacity demand,
  funding_network_valid count sources obligations kinds state eligible capacity demand ->
  network_divergence count (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index)) state 0 =
    Z.of_nat (funding_sum obligations (obligation_draw sources (projected_assignment count kinds state))).
Proof.
  intros count sources obligations kinds state eligible capacity demand [valid [_ [_ [conserved _]]]].
  rewrite <- funding_assignment_rows_equal_columns.
  assert (rows : forall source, source < sources ->
    source_draw obligations (projected_assignment count kinds state) source =
    projected_incoming count kinds (fun index => snd (state index)) source).
  { intros source inside. specialize (conserved source inside).
    rewrite (network_payer_divergence_is_row_balance count sources obligations kinds state eligible source inside valid) in conserved. lia. }
  rewrite (funding_sum_ext sources _ _ rows), projected_incoming_total.
  - apply source_divergence_counts_incoming.
  - intros index inside. specialize (valid index inside). destruct (kinds index); simpl in *; tauto.
Qed.

Theorem reverse_path_increases_funding_exactly : forall count sources obligations kinds state eligible capacity demand operations limit amount,
  funding_network_valid count sources obligations kinds state eligible capacity demand ->
  network_bounded count limit state ->
  List.NoDup (List.map fst operations) ->
  (forall op, List.In op operations -> fst op < count /\ amount <= operation_capacity state op) ->
  linked_operations (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index))
    0 operations (funding_sink sources obligations) ->
  exists next,
    network_augment limit state (List.rev operations) amount = Some next /\
    funding_network_valid count sources obligations kinds next eligible capacity demand /\
    funding_sum obligations (obligation_draw sources (projected_assignment count kinds next)) =
      funding_sum obligations (obligation_draw sources (projected_assignment count kinds state)) + amount /\
    funding_sum obligations (obligation_draw sources (projected_assignment count kinds next)) <= funding_sum obligations demand.
Proof.
  intros count sources obligations kinds state eligible capacity demand operations limit amount valid bounded distinct permitted linked.
  destruct (reverse_path_preserves_funding_validity count sources obligations kinds state eligible capacity demand operations limit amount
    valid bounded distinct permitted linked) as [next [history [next_valid partial]]].
  pose proof (network_history_changes_divergence (List.rev operations) count limit
    (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index))
    state amount next 0 ltac:(intros op member; apply in_rev in member; exact (proj1 (permitted op member))) history) as change.
  rewrite reverse_operations_preserve_boundary, (linked_operation_boundary operations _ _ 0 (funding_sink sources obligations) amount 0 linked) in change.
  rewrite (source_divergence_equals_total_assignment count sources obligations kinds state eligible capacity demand valid) in change.
  rewrite (source_divergence_equals_total_assignment count sources obligations kinds next eligible capacity demand next_valid) in change.
  unfold residual_edge_boundary, vertex_indicator, funding_sink in change.
  assert (Nat.eqb 0 (sources + obligations + 1) = false) by (apply Nat.eqb_neq; lia).
  rewrite Nat.eqb_refl, H in change.
  exists next. split; [exact history|]. split; [exact next_valid|]. split; [lia|].
  apply funding_sum_monotone. exact (proj2 partial).
Qed.

Print Assumptions source_divergence_equals_total_assignment.
Print Assumptions reverse_path_increases_funding_exactly.
Print Assumptions reverse_path_preserves_funding_validity.
Print Assumptions completed_funding_network_passes_checker.
Print Assumptions network_payer_divergence_is_row_balance.
Print Assumptions network_obligation_divergence_is_column_balance.
Print Assumptions network_projection_preserves_eligibility.
Print Assumptions conserved_network_projects_to_partial_funding.
