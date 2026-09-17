From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import EligibleFundingAssignment FundingDeficitCertificate FundingResidualCut FundingResidualGraph.
From CostAccountedRho Require Import FundingNetworkAugmentation FundingNetworkProjection FundingGraphInitialization FundingPairRealization.
From CostAccountedRho Require Import FundingParentPath FundingPathEncoding FundingDiscovery FundingFifoSearch.
From CostAccountedRho Require Import FundingDomainCuts FundingDomainWitness.
Import ListNotations.

Lemma backward_funding_path_has_indexed_operations : forall path sources obligations eligible capacity demand flow root node,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  (exists tail, path = node :: tail) -> last path root = root ->
  Forall (fun vertex => vertex < funding_vertex_count sources obligations) path ->
  backward_path_links (funding_residual_graph sources obligations eligible capacity demand flow) path ->
  exists operations,
    linked_operations
      (fun index => pair_from sources (initial_kind (initial_funding_layout sources obligations eligible) index))
      (fun index => pair_to sources obligations (initial_kind (initial_funding_layout sources obligations eligible) index))
      root operations node /\
    operation_vertices
      (fun index => pair_from sources (initial_kind (initial_funding_layout sources obligations eligible) index))
      (fun index => pair_to sources obligations (initial_kind (initial_funding_layout sources obligations eligible) index))
      root operations = rev path /\
    (forall op, In op operations -> fst op < length (initial_funding_layout sources obligations eligible) /\
      0 < operation_capacity (realized_funding_network sources obligations eligible capacity demand flow) op).
Proof.
  induction path as [|head tail IH]; intros sources obligations eligible capacity demand flow root node valid starts ends bounded links.
  - destruct starts as [tail impossible]. discriminate.
  - destruct starts as [rest same]. inversion same; subst node rest.
    destruct tail as [|previous rest].
    + simpl in ends. subst head. exists []. split; [reflexivity|].
      split; [reflexivity|]. intros op impossible. contradiction.
    + inversion bounded as [|ignored ignored_tail head_inside tail_inside]; subst.
      destruct links as [edge links].
      destruct (IH sources obligations eligible capacity demand flow root previous valid
        ltac:(eexists; reflexivity) ends tail_inside links) as [operations [linked [vertices permitted]]].
      inversion tail_inside as [|ignored ignored_tail previous_inside rest_inside]; subst.
      destruct (positive_matrix_edge_has_positive_indexed_operation sources obligations eligible capacity demand flow
        previous head valid previous_inside head_inside edge) as [op [inside [from [to positive]]]].
      exists (operations ++ [op]). split.
      * eapply linked_operations_append; [exact linked|]. simpl. auto.
      * split.
        -- unfold operation_vertices in *. rewrite map_app. simpl. rewrite to.
           change ((root :: map (operation_to
             (fun index => pair_from sources (initial_kind (initial_funding_layout sources obligations eligible) index))
             (fun index => pair_to sources obligations (initial_kind (initial_funding_layout sources obligations eligible) index))) operations) ++ [head] = rev (previous :: rest) ++ [head]).
           now rewrite vertices.
        -- intros selected member. apply in_app_iff in member. destruct member as [old|[selected_same|impossible]].
           ++ now apply permitted.
           ++ subst selected. auto.
           ++ contradiction.
Qed.

Theorem incomplete_funding_flow_has_progress_or_deficit : forall sources obligations eligible capacity demand flow,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  funding_sum obligations (obligation_draw sources flow) < funding_sum obligations demand ->
  (exists next, partial_funding_valid sources obligations eligible capacity demand next /\
    funding_sum obligations (obligation_draw sources flow) < funding_sum obligations (obligation_draw sources next)) \/
  (exists selected, funding_deficit_check sources obligations eligible capacity demand selected = true).
Proof.
  intros sources obligations eligible capacity demand flow valid short.
  set (count := funding_vertex_count sources obligations).
  set (edge := funding_residual_graph sources obligations eligible capacity demand flow).
  set (sink := funding_sink sources obligations).
  set (neighbors := fun _ : nat => seq 0 count).
  assert (neighbors_valid : fifo_neighbors_valid count edge neighbors).
  { intros source inside. split.
    - apply Forall_forall. intros target member. apply in_seq in member. lia.
    - intros target linked. unfold neighbors. rewrite in_seq. lia. }
  assert (root_inside : 0 < count) by (unfold count, funding_vertex_count; lia).
  destruct (fifo_search_completes_with_vertex_fuel count count 0 sink edge neighbors (discovery_initial 0) 0
    neighbors_valid (fifo_search_initial count 0 edge root_inside) ltac:(lia))
    as [state [cursor [result [invariant exit]]]].
  destruct (discovery_known state sink) eqn:found.
  - left.
    destruct invariant as [discovery [queue_bound [cursor_bound exploration]]].
    destruct discovery as [distinct [root_known [membership [ranks parents]]]].
    destruct (ranks sink found) as [rank_bound at_rank].
    destruct (strictly_ranked_parents_extract_a_simple_path (length (discovery_queue state)) 0
      (discovery_known state) (discovery_parent state) (discovery_rank state) edge sink parents found ltac:(lia))
      as [path [extracted [starts ends simple known links path_ranks path_length]]].
    assert (path_bound : Forall (fun vertex => vertex < count) path).
    { apply Forall_forall. intros vertex member.
      apply Forall_forall with (x := vertex) in known; [|exact member].
      apply membership in known. apply Forall_forall with (x := vertex) in queue_bound; assumption. }
    destruct (backward_funding_path_has_indexed_operations path sources obligations eligible capacity demand flow 0 sink
      valid starts ends path_bound links) as [operations [linked [vertices permitted]]].
    set (layout := initial_funding_layout sources obligations eligible).
    set (state_flow := realized_funding_network sources obligations eligible capacity demand flow).
    assert (network_valid : funding_network_valid (length layout) sources obligations (initial_kind layout)
      state_flow eligible capacity demand) by (apply realized_partial_network_is_valid; exact valid).
    assert (network_bound : network_bounded (length layout) (funding_sum obligations demand) state_flow)
      by (apply realized_network_fits_machine_limit; [exact valid|lia]).
    assert (projected_total : projected_funding (length layout) sources obligations (initial_kind layout) state_flow =
      funding_sum obligations (obligation_draw sources flow)).
    { apply funding_sum_ext. intros obligation obligation_inside. apply funding_sum_ext. intros source source_inside.
      apply realized_partial_network_projects_original_flow; assumption. }
    assert (operations_simple : NoDup (operation_vertices
      (fun index => pair_from sources (initial_kind layout index))
      (fun index => pair_to sources obligations (initial_kind layout index)) 0 operations)).
    { unfold layout. rewrite vertices. now apply NoDup_rev. }
    destruct (positive_simple_path_makes_strict_funding_progress (length layout) sources obligations (initial_kind layout)
      state_flow eligible capacity demand operations (funding_sum obligations demand) network_valid network_bound
      ltac:(rewrite projected_total; exact short) operations_simple permitted linked)
      as [positive [next [transition [next_valid [next_bound [increase [capped decrease]]]]]]].
    exists (projected_assignment (length layout) (initial_kind layout) next). split.
    + destruct next_valid as [kinds [source_caps [obligation_caps [source_balance obligation_balance]]]].
      eapply conserved_network_projects_to_partial_funding; eauto.
    + rewrite projected_total in increase. unfold projected_funding in increase. lia.
  - right.
    pose proof (completed_fifo_without_sink_has_exact_reachability count 0 sink edge state cursor invariant
      ltac:(rewrite found; exact exit) found) as reached.
    pose proof (unreachable_funding_sink_establishes_cut_premises sources obligations eligible capacity demand flow
      (discovery_known state) valid short reached found) as cut.
    exists (fun obligation => negb (discovery_known state (funding_obligation_vertex sources obligation))).
    eapply incomplete_residual_cut_has_checked_deficit; eauto.
Qed.

Theorem bounded_funding_completion : forall fuel sources obligations eligible capacity demand flow,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  funding_sum obligations demand - funding_sum obligations (obligation_draw sources flow) <= fuel ->
  (exists complete, assignment_valid sources obligations eligible capacity demand complete) \/
  (exists selected, funding_deficit_check sources obligations eligible capacity demand selected = true).
Proof.
  induction fuel as [|fuel IH]; intros sources obligations eligible capacity demand flow valid enough.
  - left. exists flow. apply complete_partial_assignment_is_valid; [exact valid|].
    assert (funding_sum obligations (obligation_draw sources flow) <= funding_sum obligations demand).
    { apply funding_sum_monotone. exact (proj2 valid). }
    lia.
  - destruct (Nat.eq_dec (funding_sum obligations (obligation_draw sources flow)) (funding_sum obligations demand)) as [complete|incomplete].
    + left. exists flow. now apply complete_partial_assignment_is_valid.
    + assert (short : funding_sum obligations (obligation_draw sources flow) < funding_sum obligations demand).
      { assert (funding_sum obligations (obligation_draw sources flow) <= funding_sum obligations demand).
        { apply funding_sum_monotone. exact (proj2 valid). }
        lia. }
      destruct (incomplete_funding_flow_has_progress_or_deficit sources obligations eligible capacity demand flow valid short)
        as [[next [next_valid increase]]|deficit]; [|now right].
      apply (IH sources obligations eligible capacity demand next next_valid). lia.
Qed.

Theorem fixed_funding_assignment_or_checked_deficit : forall sources obligations eligible capacity demand,
  (exists complete, assignment_valid sources obligations eligible capacity demand complete) \/
  (exists selected, funding_deficit_check sources obligations eligible capacity demand selected = true).
Proof.
  intros sources obligations eligible capacity demand.
  apply (bounded_funding_completion (funding_sum obligations demand) sources obligations eligible capacity demand (fun _ _ => 0)).
  - split.
    + split.
      * intros source inside. split; [unfold source_draw; rewrite funding_sum_zero; lia|intros; reflexivity].
      * intros. reflexivity.
    + intros obligation inside. unfold obligation_draw. rewrite funding_sum_zero. lia.
  - lia.
Qed.

Theorem fixed_flow_feasible_iff_no_deficit : forall sources obligations eligible capacity demand,
  (exists flow, assignment_valid sources obligations eligible capacity demand flow) <->
  (forall selected, funding_deficit_check sources obligations eligible capacity demand selected = false).
Proof.
  intros sources obligations eligible capacity demand. split.
  - intros [flow valid] selected. eapply valid_assignment_never_has_deficit; eauto.
  - intros no_deficit.
    destruct (fixed_funding_assignment_or_checked_deficit sources obligations eligible capacity demand) as [complete|[selected deficit]].
    + exact complete.
    + specialize (no_deficit selected). congruence.
Qed.

Theorem complete_contribution_domain_iff_cut_domain : forall sources obligations eligible capacity demand,
  funding_sum obligations demand <= funding_sum sources capacity ->
  (forall contribution, capped_contribution sources capacity (funding_sum obligations demand) contribution ->
    exists flow, assignment_valid sources obligations eligible contribution demand flow) <->
  contribution_cut_domain sources obligations eligible capacity demand.
Proof.
  intros sources obligations eligible capacity demand enough. split.
  - apply contribution_domain_coverage_requires_all_cuts. exact enough.
  - intros cuts contribution capped. apply fixed_flow_feasible_iff_no_deficit.
    now apply (contribution_cuts_cover_every_capped_vector sources obligations eligible capacity demand contribution).
Qed.

Theorem complete_contribution_domain_iff_transposed_assignments : forall sources obligations eligible capacity demand,
  funding_sum obligations demand <= funding_sum sources capacity ->
  (forall contribution, capped_contribution sources capacity (funding_sum obligations demand) contribution ->
    exists flow, assignment_valid sources obligations eligible contribution demand flow) <->
  (forall obligation, obligation < obligations -> 0 < demand obligation ->
    exists flow, assignment_valid obligations sources (fun target source => eligible source target) demand
      (fun source => if eligible source obligation then 0 else capacity source) flow).
Proof.
  intros sources obligations eligible capacity demand enough.
  rewrite complete_contribution_domain_iff_cut_domain by exact enough.
  rewrite contribution_cuts_iff_transposed_cuts. unfold transposed_cut_domain.
  split; intros all obligation inside positive.
  - apply fixed_flow_feasible_iff_no_deficit. apply all; assumption.
  - apply fixed_flow_feasible_iff_no_deficit. apply all; assumption.
Qed.

Print Assumptions backward_funding_path_has_indexed_operations.
Print Assumptions incomplete_funding_flow_has_progress_or_deficit.
Print Assumptions bounded_funding_completion.
Print Assumptions fixed_funding_assignment_or_checked_deficit.
Print Assumptions fixed_flow_feasible_iff_no_deficit.
Print Assumptions complete_contribution_domain_iff_cut_domain.
Print Assumptions complete_contribution_domain_iff_transposed_assignments.
