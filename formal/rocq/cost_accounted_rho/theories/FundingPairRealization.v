From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List ZArith Lia.
From CostAccountedRho Require Import EligibleFundingAssignment FundingDeficitCertificate CompleteFundingCandidates.
From CostAccountedRho Require Import FundingResidualCut FundingResidualGraph FundingNetworkAugmentation FundingNetworkProjection FundingGraphInitialization.
Import ListNotations.

Lemma unique_layout_sum_at_kind : forall layout target value,
  NoDup layout -> In target layout ->
  (forall kind, kind <> target -> value kind = 0) ->
  funding_sum (length layout) (fun index => value (initial_kind layout index)) = value target.
Proof.
  intros layout target value unique member support.
  destruct (In_nth layout target (PayerCapacity 0) member) as [position [inside at_position]].
  transitivity (funding_sum (length layout) (fun index => if Nat.eqb index position then value target else 0)).
  - apply funding_sum_ext. intros index bounded.
    destruct (Nat.eqb_spec index position) as [->|different].
    + unfold initial_kind. now rewrite at_position.
    + apply support. intros same.
      assert (index = position).
      { apply (unique_layout_indices layout index position unique bounded inside).
        unfold initial_kind in *. now rewrite same, at_position. }
      congruence.
  - apply funding_sum_indicator. exact inside.
Qed.

Lemma layout_payer_value : forall sources obligations eligible value source,
  source < sources ->
  projected_incoming (length (initial_funding_layout sources obligations eligible))
    (initial_kind (initial_funding_layout sources obligations eligible))
    (fun index => value (initial_kind (initial_funding_layout sources obligations eligible) index)) source =
  value (PayerCapacity source).
Proof.
  intros sources obligations eligible value source inside. unfold projected_incoming.
  transitivity ((fun kind => match kind with
    | PayerCapacity i => if Nat.eqb source i then value kind else 0 | _ => 0 end) (PayerCapacity source)).
  - apply (unique_layout_sum_at_kind (initial_funding_layout sources obligations eligible) (PayerCapacity source)
      (fun kind => match kind with PayerCapacity i => if Nat.eqb source i then value kind else 0 | _ => 0 end)).
    + apply initial_layout_has_unique_pairs.
    + apply initial_layout_has_exact_membership. exact inside.
    + intros [i|i j|j] different; simpl; try reflexivity.
      destruct (Nat.eqb_spec source i); [congruence|reflexivity].
  - simpl. now rewrite Nat.eqb_refl.
Qed.

Lemma layout_obligation_value : forall sources obligations eligible value obligation,
  obligation < obligations ->
  projected_outgoing (length (initial_funding_layout sources obligations eligible))
    (initial_kind (initial_funding_layout sources obligations eligible))
    (fun index => value (initial_kind (initial_funding_layout sources obligations eligible) index)) obligation =
  value (ObligationCapacity obligation).
Proof.
  intros sources obligations eligible value obligation inside. unfold projected_outgoing.
  transitivity ((fun kind => match kind with
    | ObligationCapacity j => if Nat.eqb obligation j then value kind else 0 | _ => 0 end) (ObligationCapacity obligation)).
  - apply (unique_layout_sum_at_kind (initial_funding_layout sources obligations eligible) (ObligationCapacity obligation)
      (fun kind => match kind with ObligationCapacity j => if Nat.eqb obligation j then value kind else 0 | _ => 0 end)).
    + apply initial_layout_has_unique_pairs.
    + apply initial_layout_has_exact_membership. exact inside.
    + intros [i|i j|j] different; simpl; try reflexivity.
      destruct (Nat.eqb_spec obligation j); [congruence|reflexivity].
  - simpl. now rewrite Nat.eqb_refl.
Qed.

Definition funding_kind_flow sources obligations flow kind :=
  match kind with
  | PayerCapacity source => source_draw obligations flow source
  | EligibleAssignment source obligation => flow source obligation
  | ObligationCapacity obligation => obligation_draw sources flow obligation
  end.

Definition realized_funding_network sources obligations eligible capacity demand flow : residual_network :=
  fun index =>
    let kind := initial_kind (initial_funding_layout sources obligations eligible) index in
    let amount := funding_kind_flow sources obligations flow kind in
    (initial_pair_capacity obligations capacity demand kind - amount, amount).

Theorem realized_network_projects_every_permitted_entry : forall sources obligations eligible capacity demand flow source obligation,
  source < sources -> obligation < obligations -> eligible source obligation = true ->
  projected_assignment (length (initial_funding_layout sources obligations eligible))
    (initial_kind (initial_funding_layout sources obligations eligible))
    (realized_funding_network sources obligations eligible capacity demand flow) source obligation = flow source obligation.
Proof.
  intros sources obligations eligible capacity demand flow source obligation inside bounded permitted.
  unfold projected_assignment, realized_funding_network. simpl.
  transitivity ((fun kind => match kind with
    | EligibleAssignment i j => if Nat.eqb source i && Nat.eqb obligation j
      then funding_kind_flow sources obligations flow kind else 0 | _ => 0 end)
    (EligibleAssignment source obligation)).
  - apply (unique_layout_sum_at_kind (initial_funding_layout sources obligations eligible) (EligibleAssignment source obligation)
      (fun kind => match kind with EligibleAssignment i j => if Nat.eqb source i && Nat.eqb obligation j
        then funding_kind_flow sources obligations flow kind else 0 | _ => 0 end)).
    + apply initial_layout_has_unique_pairs.
    + apply initial_layout_has_exact_membership. repeat split; assumption.
    + intros [i|i j|j] different; simpl; try reflexivity.
      destruct (Nat.eqb_spec source i), (Nat.eqb_spec obligation j); simpl; try reflexivity; congruence.
  - simpl. now rewrite !Nat.eqb_refl.
Qed.

Theorem realized_partial_network_projects_original_flow : forall sources obligations eligible capacity demand flow source obligation,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  source < sources -> obligation < obligations ->
  projected_assignment (length (initial_funding_layout sources obligations eligible))
    (initial_kind (initial_funding_layout sources obligations eligible))
    (realized_funding_network sources obligations eligible capacity demand flow) source obligation = flow source obligation.
Proof.
  intros sources obligations eligible capacity demand flow source obligation valid source_inside obligation_inside.
  destruct (eligible source obligation) eqn:allowed.
  - apply realized_network_projects_every_permitted_entry; assumption.
  - rewrite (network_projection_preserves_eligibility
      (length (initial_funding_layout sources obligations eligible)) sources obligations
      (initial_kind (initial_funding_layout sources obligations eligible))
      (realized_funding_network sources obligations eligible capacity demand flow) eligible source obligation).
    + symmetry. exact (proj2 (proj1 (proj1 valid) source source_inside) obligation obligation_inside allowed).
    + intros. now apply initial_network_has_valid_kinds.
    + exact allowed.
Qed.

Lemma partial_kind_flow_respects_original_capacity : forall sources obligations eligible capacity demand flow kind,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  kind_valid sources obligations eligible kind ->
  funding_kind_flow sources obligations flow kind <= initial_pair_capacity obligations capacity demand kind.
Proof.
  intros sources obligations eligible capacity demand flow kind valid kind_inside.
  assert (total : funding_sum obligations (obligation_draw sources flow) <= funding_sum obligations demand).
  { apply funding_sum_monotone. exact (proj2 valid). }
  destruct kind as [source|source obligation|obligation]; simpl in *.
  - apply Nat.min_glb.
    + exact (proj1 (proj1 (proj1 valid) source kind_inside)).
    + pose proof (funding_sum_contains_entry sources (source_draw obligations flow) source kind_inside) as row.
      rewrite funding_assignment_rows_equal_columns in row. lia.
  - destruct kind_inside as [inside [bounded permitted]].
    eapply Nat.le_trans.
    + apply (valid_funding_entry_is_bounded_by_total sources obligations eligible capacity
        (obligation_draw sources flow) flow source obligation (proj1 valid) inside bounded).
    + exact total.
  - exact (proj2 valid obligation kind_inside).
Qed.

Theorem realized_network_conserves_every_pair : forall sources obligations eligible capacity demand flow index,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  index < length (initial_funding_layout sources obligations eligible) ->
  fst (realized_funding_network sources obligations eligible capacity demand flow index) +
    snd (realized_funding_network sources obligations eligible capacity demand flow index) =
  initial_pair_capacity obligations capacity demand
    (initial_kind (initial_funding_layout sources obligations eligible) index).
Proof.
  intros sources obligations eligible capacity demand flow index valid inside.
  pose proof (partial_kind_flow_respects_original_capacity sources obligations eligible capacity demand flow
    (initial_kind (initial_funding_layout sources obligations eligible) index) valid
    (initial_network_has_valid_kinds sources obligations eligible index inside)).
  unfold realized_funding_network. simpl. lia.
Qed.

Theorem realized_network_fits_machine_limit : forall sources obligations eligible capacity demand flow limit,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  funding_sum obligations demand <= limit ->
  network_bounded (length (initial_funding_layout sources obligations eligible)) limit
    (realized_funding_network sources obligations eligible capacity demand flow).
Proof.
  intros sources obligations eligible capacity demand flow limit valid total index inside.
  rewrite realized_network_conserves_every_pair by assumption.
  pose proof (initial_network_fits_machine_limit sources obligations eligible capacity demand limit total index inside) as initial.
  unfold initial_network in initial. simpl in initial. lia.
Qed.

Theorem realized_partial_network_is_valid : forall sources obligations eligible capacity demand flow,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  funding_network_valid (length (initial_funding_layout sources obligations eligible)) sources obligations
    (initial_kind (initial_funding_layout sources obligations eligible))
    (realized_funding_network sources obligations eligible capacity demand flow) eligible capacity demand.
Proof.
  intros sources obligations eligible capacity demand flow valid.
  split; [intros; now apply initial_network_has_valid_kinds|].
  split.
  - intros source inside.
    transitivity (projected_incoming (length (initial_funding_layout sources obligations eligible))
      (initial_kind (initial_funding_layout sources obligations eligible))
      (fun index => initial_pair_capacity obligations capacity demand
        (initial_kind (initial_funding_layout sources obligations eligible) index)) source).
    + unfold projected_incoming. apply funding_sum_monotone. intros index bounded.
      rewrite realized_network_conserves_every_pair by assumption.
      destruct (initial_kind (initial_funding_layout sources obligations eligible) index); try reflexivity.
    + rewrite layout_payer_value by assumption. apply Nat.le_min_l.
  - split.
    + intros obligation inside.
      transitivity (projected_outgoing (length (initial_funding_layout sources obligations eligible))
        (initial_kind (initial_funding_layout sources obligations eligible))
        (fun index => initial_pair_capacity obligations capacity demand
          (initial_kind (initial_funding_layout sources obligations eligible) index)) obligation).
      * unfold projected_outgoing. apply funding_sum_monotone. intros index bounded.
        rewrite realized_network_conserves_every_pair by assumption.
        destruct (initial_kind (initial_funding_layout sources obligations eligible) index); try reflexivity.
      * rewrite layout_obligation_value by assumption. reflexivity.
    + split.
      * intros source inside. rewrite (network_payer_divergence_is_row_balance _ sources obligations _ _ eligible)
          by (intros; try assumption; now apply initial_network_has_valid_kinds).
        assert (row : source_draw obligations
          (projected_assignment (length (initial_funding_layout sources obligations eligible))
            (initial_kind (initial_funding_layout sources obligations eligible))
            (realized_funding_network sources obligations eligible capacity demand flow)) source =
          source_draw obligations flow source).
        { apply funding_sum_ext. intros. apply realized_partial_network_projects_original_flow; assumption. }
        rewrite row. unfold realized_funding_network. simpl.
        rewrite layout_payer_value by assumption. simpl. lia.
      * intros obligation inside. rewrite (network_obligation_divergence_is_column_balance _ sources obligations _ _ eligible)
          by (intros; try assumption; now apply initial_network_has_valid_kinds).
        assert (column : obligation_draw sources
          (projected_assignment (length (initial_funding_layout sources obligations eligible))
            (initial_kind (initial_funding_layout sources obligations eligible))
            (realized_funding_network sources obligations eligible capacity demand flow)) obligation =
          obligation_draw sources flow obligation).
        { apply funding_sum_ext. intros. apply realized_partial_network_projects_original_flow; assumption. }
        rewrite column. unfold realized_funding_network. simpl.
        rewrite layout_obligation_value by assumption. simpl. lia.
Qed.

Lemma decoded_funding_vertex_has_exact_index : forall sources obligations node,
  node < funding_vertex_count sources obligations ->
  match decode_funding_vertex sources obligations node with
  | FundingSource => node = 0
  | FundingPayer source => source < sources /\ node = S source
  | FundingObligation obligation => obligation < obligations /\ node = funding_obligation_vertex sources obligation
  | FundingSink => node = funding_sink sources obligations
  end.
Proof.
  intros sources obligations node inside. unfold decode_funding_vertex.
  destruct (Nat.eqb_spec node 0) as [zero|positive]; [exact zero|].
  destruct (node <=? sources) eqn:payer.
  - apply Nat.leb_le in payer. split; lia.
  - apply Nat.leb_gt in payer. destruct (node <? sources + 1 + obligations) eqn:obligation.
    + apply Nat.ltb_lt in obligation. unfold funding_obligation_vertex. split; lia.
    + apply Nat.ltb_ge in obligation. unfold funding_vertex_count in inside. unfold funding_sink. lia.
Qed.

Lemma realized_kind_has_indexed_operation : forall sources obligations eligible capacity demand flow kind (reversed : bool),
  kind_valid sources obligations eligible kind ->
  exists op,
    fst op < length (initial_funding_layout sources obligations eligible) /\
    operation_from (fun index => pair_from sources (initial_kind (initial_funding_layout sources obligations eligible) index))
      (fun index => pair_to sources obligations (initial_kind (initial_funding_layout sources obligations eligible) index)) op =
      (if reversed then pair_to sources obligations kind else pair_from sources kind) /\
    operation_to (fun index => pair_from sources (initial_kind (initial_funding_layout sources obligations eligible) index))
      (fun index => pair_to sources obligations (initial_kind (initial_funding_layout sources obligations eligible) index)) op =
      (if reversed then pair_from sources kind else pair_to sources obligations kind) /\
    operation_capacity (realized_funding_network sources obligations eligible capacity demand flow) op =
      (if reversed then funding_kind_flow sources obligations flow kind
       else initial_pair_capacity obligations capacity demand kind - funding_kind_flow sources obligations flow kind).
Proof.
  intros sources obligations eligible capacity demand flow kind reversed valid.
  apply initial_layout_has_exact_membership in valid.
  destruct (In_nth _ _ (PayerCapacity 0) valid) as [index [inside same]].
  exists (index, reversed). split; [exact inside|].
  unfold operation_from, operation_to, operation_capacity, realized_funding_network, initial_kind. simpl.
  rewrite same. destruct reversed; repeat split.
Qed.

Theorem positive_matrix_edge_has_positive_indexed_operation : forall sources obligations eligible capacity demand flow from to,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  from < funding_vertex_count sources obligations -> to < funding_vertex_count sources obligations ->
  funding_residual_graph sources obligations eligible capacity demand flow from to = true ->
  exists op,
    fst op < length (initial_funding_layout sources obligations eligible) /\
    operation_from (fun index => pair_from sources (initial_kind (initial_funding_layout sources obligations eligible) index))
      (fun index => pair_to sources obligations (initial_kind (initial_funding_layout sources obligations eligible) index)) op = from /\
    operation_to (fun index => pair_from sources (initial_kind (initial_funding_layout sources obligations eligible) index))
      (fun index => pair_to sources obligations (initial_kind (initial_funding_layout sources obligations eligible) index)) op = to /\
    0 < operation_capacity (realized_funding_network sources obligations eligible capacity demand flow) op.
Proof.
  intros sources obligations eligible capacity demand flow from to valid from_inside to_inside linked.
  pose proof (decoded_funding_vertex_has_exact_index sources obligations from from_inside) as origin.
  pose proof (decoded_funding_vertex_has_exact_index sources obligations to to_inside) as target.
  unfold funding_residual_graph in linked.
  destruct (decode_funding_vertex sources obligations from) as [|i|j|] eqn:from_kind;
    destruct (decode_funding_vertex sources obligations to) as [|k|l|] eqn:to_kind;
    simpl in linked, origin, target; try discriminate.
  - destruct target as [bounded same]. subst from to.
    destruct (realized_kind_has_indexed_operation sources obligations eligible capacity demand flow (PayerCapacity k) false bounded)
      as [op [inside [start [finish amount]]]].
    exists op. repeat split; try assumption. rewrite amount. simpl. apply Nat.ltb_lt in linked. lia.
  - destruct origin as [bounded same]. subst from to.
    destruct (realized_kind_has_indexed_operation sources obligations eligible capacity demand flow (PayerCapacity i) true bounded)
      as [op [inside [start [finish amount]]]].
    exists op. repeat split; try assumption. rewrite amount. simpl. now apply Nat.ltb_lt in linked.
  - destruct origin as [source_inside source_at]. destruct target as [obligation_inside obligation_at]. subst from to.
    apply andb_true_iff in linked. destruct linked as [permitted positive].
    destruct (realized_kind_has_indexed_operation sources obligations eligible capacity demand flow (EligibleAssignment i l) false
      ltac:(repeat split; assumption)) as [op [inside [start [finish amount]]]].
    exists op. repeat split; try assumption. rewrite amount. simpl. apply Nat.ltb_lt in positive. lia.
  - destruct origin as [obligation_inside obligation_at]. destruct target as [source_inside source_at]. subst from to.
    apply Nat.ltb_lt in linked.
    assert (permitted : eligible k j = true).
    { destruct (eligible k j) eqn:allowed; [reflexivity|].
      pose proof (proj2 (proj1 (proj1 valid) k source_inside) j obligation_inside allowed). lia. }
    destruct (realized_kind_has_indexed_operation sources obligations eligible capacity demand flow (EligibleAssignment k j) true
      ltac:(repeat split; assumption)) as [op [inside [start [finish amount]]]].
    exists op. repeat split; try assumption. rewrite amount. exact linked.
  - destruct origin as [bounded same]. subst from to.
    destruct (realized_kind_has_indexed_operation sources obligations eligible capacity demand flow (ObligationCapacity j) false bounded)
      as [op [inside [start [finish amount]]]].
    exists op. repeat split; try assumption. rewrite amount. simpl. apply Nat.ltb_lt in linked. lia.
  - destruct target as [bounded same]. subst from to.
    destruct (realized_kind_has_indexed_operation sources obligations eligible capacity demand flow (ObligationCapacity l) true bounded)
      as [op [inside [start [finish amount]]]].
    exists op. repeat split; try assumption. rewrite amount. simpl. now apply Nat.ltb_lt in linked.
Qed.

Print Assumptions unique_layout_sum_at_kind.
Print Assumptions layout_payer_value.
Print Assumptions layout_obligation_value.
Print Assumptions realized_network_projects_every_permitted_entry.
Print Assumptions realized_partial_network_projects_original_flow.
Print Assumptions partial_kind_flow_respects_original_capacity.
Print Assumptions realized_network_conserves_every_pair.
Print Assumptions realized_network_fits_machine_limit.
Print Assumptions realized_partial_network_is_valid.
Print Assumptions decoded_funding_vertex_has_exact_index.
Print Assumptions realized_kind_has_indexed_operation.
Print Assumptions positive_matrix_edge_has_positive_indexed_operation.
