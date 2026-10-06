From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List ZArith Lia.
From CostAccountedRho Require Import EligibleFundingAssignment CompleteFundingCandidates FundingNetworkAugmentation FundingNetworkProjection.
Import ListNotations.

Definition assignment_row obligations eligible source :=
  map (EligibleAssignment source) (filter (eligible source) (seq 0 obligations)).

Fixpoint assignment_rows sources obligations eligible :=
  match sources with
  | 0 => []
  | S count => assignment_rows count obligations eligible ++ assignment_row obligations eligible count
  end.

Definition initial_funding_layout sources obligations eligible :=
  map PayerCapacity (seq 0 sources) ++ assignment_rows sources obligations eligible ++
  map ObligationCapacity (seq 0 obligations).

Lemma assignment_row_membership : forall obligations eligible source kind,
  In kind (assignment_row obligations eligible source) <->
  exists obligation, obligation < obligations /\ eligible source obligation = true /\ kind = EligibleAssignment source obligation.
Proof.
  intros. unfold assignment_row. rewrite in_map_iff. split.
  - intros [obligation [equal member]]. apply filter_In in member. destruct member as [inside permitted].
    apply in_seq in inside. exists obligation. repeat split; auto; lia.
  - intros [obligation [inside [permitted equal]]]. exists obligation. split; [congruence|].
    apply filter_In. split; [apply in_seq; lia|exact permitted].
Qed.

Lemma assignment_rows_membership : forall sources obligations eligible kind,
  In kind (assignment_rows sources obligations eligible) <->
  exists source obligation, source < sources /\ obligation < obligations /\ eligible source obligation = true /\ kind = EligibleAssignment source obligation.
Proof.
  induction sources as [|sources IH]; intros obligations eligible kind; simpl.
  - split; [contradiction|intros [source [obligation [inside _]]]; lia].
  - rewrite in_app_iff, IH, assignment_row_membership. split.
    + intros [[source [obligation [inside [bounded [permitted equal]]]]]|[obligation [bounded [permitted equal]]]].
      * exists source, obligation. repeat split; auto; lia.
      * exists sources, obligation. repeat split; auto.
    + intros [source [obligation [inside [bounded [permitted equal]]]]].
      destruct (Nat.eq_dec source sources) as [same|different].
      * right. subst. exists obligation. auto.
      * left. exists source, obligation. repeat split; auto; lia.
Qed.

Theorem initial_layout_has_exact_membership : forall sources obligations eligible kind,
  In kind (initial_funding_layout sources obligations eligible) <-> kind_valid sources obligations eligible kind.
Proof.
  intros. unfold initial_funding_layout. rewrite !in_app_iff, assignment_rows_membership, !in_map_iff.
  destruct kind as [source|source obligation|obligation]; unfold kind_valid; split.
  - intros [payer|[assigned|outgoing]].
    + destruct payer as [i [equal inside]]. inversion equal; subst. apply in_seq in inside. lia.
    + destruct assigned as [i [j [_ [_ [_ equal]]]]]. discriminate.
    + destruct outgoing as [j [equal _]]. discriminate.
  - intros inside. left. exists source. split; [reflexivity|apply in_seq; lia].
  - intros [payer|[assigned|outgoing]].
    + destruct payer as [i [equal _]]. discriminate.
    + destruct assigned as [i [j [inside [bounded [permitted equal]]]]]. inversion equal; subst. auto.
    + destruct outgoing as [j [equal _]]. discriminate.
  - intros [inside [bounded permitted]]. right. left. exists source, obligation. auto.
  - intros [payer|[assigned|outgoing]].
    + destruct payer as [i [equal _]]. discriminate.
    + destruct assigned as [i [j [_ [_ [_ equal]]]]]. discriminate.
    + destruct outgoing as [j [equal inside]]. inversion equal; subst. apply in_seq in inside. lia.
  - intros inside. right. right. exists obligation. split; [reflexivity|apply in_seq; lia].
Qed.

Lemma injective_map_preserves_nodup : forall (A B : Type) (f : A -> B) values,
  (forall x y, f x = f y -> x = y) -> NoDup values -> NoDup (map f values).
Proof.
  intros A B f values injective distinct. induction distinct; simpl; constructor; auto.
  intros member. apply in_map_iff in member. destruct member as [other [same inside]].
  apply injective in same. subst. contradiction.
Qed.

Lemma assignment_row_is_unique : forall obligations eligible source,
  NoDup (assignment_row obligations eligible source).
Proof.
  intros. apply injective_map_preserves_nodup.
  - intros x y same. now inversion same.
  - apply NoDup_filter. apply seq_NoDup.
Qed.

Lemma assignment_rows_are_unique : forall sources obligations eligible,
  NoDup (assignment_rows sources obligations eligible).
Proof.
  induction sources as [|sources IH]; intros; simpl; [constructor|].
  apply NoDup_app; [apply IH|apply assignment_row_is_unique|].
  intros kind previous current. apply assignment_rows_membership in previous. apply assignment_row_membership in current.
  destruct previous as [source [obligation [inside [_ [_ equal]]]]].
  destruct current as [other [_ [_ same]]]. rewrite equal in same. inversion same. lia.
Qed.

Theorem initial_layout_has_unique_pairs : forall sources obligations eligible,
  NoDup (initial_funding_layout sources obligations eligible).
Proof.
  intros. unfold initial_funding_layout. apply NoDup_app.
  - apply injective_map_preserves_nodup; [intros x y same; now inversion same|apply seq_NoDup].
  - apply NoDup_app.
    + apply assignment_rows_are_unique.
    + apply injective_map_preserves_nodup; [intros x y same; now inversion same|apply seq_NoDup].
    + intros kind assigned capacity_edge. apply assignment_rows_membership in assigned. apply in_map_iff in capacity_edge.
      destruct assigned as [i [j [_ [_ [_ same]]]]]. destruct capacity_edge as [k [other _]]. congruence.
  - intros kind capacity_edge rest. apply in_map_iff in capacity_edge. destruct capacity_edge as [i [same _]].
    apply in_app_iff in rest. destruct rest as [assigned|outgoing].
    + apply assignment_rows_membership in assigned. destruct assigned as [j [k [_ [_ [_ other]]]]]. congruence.
    + apply in_map_iff in outgoing. destruct outgoing as [j [other _]]. congruence.
Qed.

Definition initial_kind layout index := nth index layout (PayerCapacity 0).

Definition initial_pair_capacity obligations capacity demand kind :=
  match kind with
  | PayerCapacity source => Nat.min (capacity source) (funding_sum obligations demand)
  | EligibleAssignment _ _ => funding_sum obligations demand
  | ObligationCapacity obligation => demand obligation
  end.

Definition initial_network layout obligations capacity demand : residual_network :=
  fun index => (initial_pair_capacity obligations capacity demand (initial_kind layout index), 0).

Theorem initial_network_has_zero_projected_flow : forall layout obligations capacity demand sources source obligation,
  projected_assignment (length layout) (initial_kind layout) (initial_network layout obligations capacity demand) source obligation = 0 /\
  network_divergence (length layout) (fun index => pair_from sources (initial_kind layout index))
    (fun index => pair_to sources obligations (initial_kind layout index)) (initial_network layout obligations capacity demand) source = 0%Z.
Proof.
  intros. split.
  - unfold projected_assignment, initial_network. transitivity (funding_sum (length layout) (fun _ => 0)); [|apply funding_sum_zero].
    apply funding_sum_ext. intros index inside. destruct (initial_kind layout index); simpl; try reflexivity.
    now destruct (Nat.eqb source source0 && Nat.eqb obligation obligation0).
  - unfold network_divergence, initial_network. transitivity (network_sum (length layout) (fun _ => 0%Z)); [|apply network_sum_zero].
    apply network_sum_ext. intros. reflexivity.
Qed.

Theorem initial_network_has_valid_kinds : forall sources obligations eligible index,
  index < length (initial_funding_layout sources obligations eligible) ->
  kind_valid sources obligations eligible (initial_kind (initial_funding_layout sources obligations eligible) index).
Proof.
  intros. apply initial_layout_has_exact_membership. unfold initial_kind. apply nth_In. exact H.
Qed.

Theorem initial_network_fits_machine_limit : forall sources obligations eligible capacity demand limit,
  funding_sum obligations demand <= limit ->
  network_bounded (length (initial_funding_layout sources obligations eligible)) limit
    (initial_network (initial_funding_layout sources obligations eligible) obligations capacity demand).
Proof.
  intros sources obligations eligible capacity demand limit total index inside.
  pose proof (initial_network_has_valid_kinds sources obligations eligible index inside) as valid.
  unfold initial_network, initial_pair_capacity. simpl.
  destruct (initial_kind (initial_funding_layout sources obligations eligible) index) as [i|i j|j] eqn:kind; simpl in *.
  - pose proof (Nat.le_min_r (capacity i) (funding_sum obligations demand)). lia.
  - lia.
  - pose proof (funding_sum_contains_entry obligations demand j valid). lia.
Qed.

Lemma funding_sum_single_support_bound : forall count value bound,
  (forall index, index < count -> value index <= bound) ->
  (forall i j, i < count -> j < count -> value i <> 0 -> value j <> 0 -> i = j) ->
  funding_sum count value <= bound.
Proof.
  induction count as [|count IH]; intros value bound bounded unique; simpl; [lia|].
  destruct (Nat.eq_dec (value count) 0) as [zero|positive].
  - rewrite zero, Nat.add_0_r. apply IH; intros; [apply bounded; lia|eapply unique; eauto; lia].
  - assert (prefix_zero : funding_sum count value = 0).
    { transitivity (funding_sum count (fun _ => 0)); [|apply funding_sum_zero].
      apply funding_sum_ext. intros index inside.
      destruct (Nat.eq_dec (value index) 0) as [zero|nonzero]; [exact zero|].
      pose proof (unique index count ltac:(lia) ltac:(lia) nonzero positive). lia. }
    rewrite prefix_zero. apply bounded. lia.
Qed.

Lemma unique_layout_indices : forall layout i j,
  NoDup layout -> i < length layout -> j < length layout ->
  initial_kind layout i = initial_kind layout j -> i = j.
Proof.
  intros layout i j unique inside other same.
  apply (proj1 (NoDup_nth layout (PayerCapacity 0)) unique i j inside other). exact same.
Qed.

Theorem initial_network_has_source_capacity_bounds : forall sources obligations eligible capacity demand source,
  projected_incoming (length (initial_funding_layout sources obligations eligible))
    (initial_kind (initial_funding_layout sources obligations eligible))
    (fun index => fst (initial_network (initial_funding_layout sources obligations eligible) obligations capacity demand index) +
      snd (initial_network (initial_funding_layout sources obligations eligible) obligations capacity demand index)) source <= capacity source.
Proof.
  intros. unfold projected_incoming, initial_network.
  apply funding_sum_single_support_bound.
  - intros index inside. destruct (initial_kind (initial_funding_layout sources obligations eligible) index) as [i|i j|j]; simpl; try lia.
    destruct (Nat.eqb source i) eqn:same; [apply Nat.eqb_eq in same; subst; pose proof (Nat.le_min_l (capacity i) (funding_sum obligations demand)); lia|lia].
  - intros i j inside other positive_i positive_j.
    apply (unique_layout_indices (initial_funding_layout sources obligations eligible)); [apply initial_layout_has_unique_pairs|exact inside|exact other|].
    destruct (initial_kind (initial_funding_layout sources obligations eligible) i) as [a|a b|b] eqn:first; try (simpl in positive_i; contradiction).
    destruct (initial_kind (initial_funding_layout sources obligations eligible) j) as [c|c d|d] eqn:second; try (simpl in positive_j; contradiction).
    destruct (Nat.eqb source a) eqn:same_a; [|simpl in positive_i; contradiction].
    destruct (Nat.eqb source c) eqn:same_c; [|simpl in positive_j; contradiction].
    apply Nat.eqb_eq in same_a. apply Nat.eqb_eq in same_c. congruence.
Qed.

Theorem initial_network_has_obligation_capacity_bounds : forall sources obligations eligible capacity demand obligation,
  projected_outgoing (length (initial_funding_layout sources obligations eligible))
    (initial_kind (initial_funding_layout sources obligations eligible))
    (fun index => fst (initial_network (initial_funding_layout sources obligations eligible) obligations capacity demand index) +
      snd (initial_network (initial_funding_layout sources obligations eligible) obligations capacity demand index)) obligation <= demand obligation.
Proof.
  intros. unfold projected_outgoing, initial_network.
  apply funding_sum_single_support_bound.
  - intros index inside. destruct (initial_kind (initial_funding_layout sources obligations eligible) index) as [i|i j|j]; simpl; try lia.
    destruct (Nat.eqb obligation j) eqn:same; [apply Nat.eqb_eq in same; subst; lia|lia].
  - intros i j inside other positive_i positive_j.
    apply (unique_layout_indices (initial_funding_layout sources obligations eligible)); [apply initial_layout_has_unique_pairs|exact inside|exact other|].
    destruct (initial_kind (initial_funding_layout sources obligations eligible) i) as [a|a b|b] eqn:first; try (simpl in positive_i; contradiction).
    destruct (initial_kind (initial_funding_layout sources obligations eligible) j) as [c|c d|d] eqn:second; try (simpl in positive_j; contradiction).
    destruct (Nat.eqb obligation b) eqn:same_b; [|simpl in positive_i; contradiction].
    destruct (Nat.eqb obligation d) eqn:same_d; [|simpl in positive_j; contradiction].
    apply Nat.eqb_eq in same_b. apply Nat.eqb_eq in same_d. congruence.
Qed.

Theorem initial_network_is_valid : forall sources obligations eligible capacity demand,
  funding_network_valid (length (initial_funding_layout sources obligations eligible)) sources obligations
    (initial_kind (initial_funding_layout sources obligations eligible))
    (initial_network (initial_funding_layout sources obligations eligible) obligations capacity demand) eligible capacity demand.
Proof.
  intros. split; [apply initial_network_has_valid_kinds|].
  split; [intros; apply initial_network_has_source_capacity_bounds|].
  split; [intros; apply initial_network_has_obligation_capacity_bounds|].
  split; intros; apply (proj2 (initial_network_has_zero_projected_flow
    (initial_funding_layout sources obligations eligible) obligations capacity demand sources _ 0)).
Qed.

Print Assumptions initial_network_is_valid.
Print Assumptions initial_network_has_source_capacity_bounds.
Print Assumptions initial_network_has_obligation_capacity_bounds.
Print Assumptions initial_layout_has_exact_membership.
Print Assumptions initial_layout_has_unique_pairs.
Print Assumptions initial_network_has_zero_projected_flow.
Print Assumptions initial_network_has_valid_kinds.
Print Assumptions initial_network_fits_machine_limit.
