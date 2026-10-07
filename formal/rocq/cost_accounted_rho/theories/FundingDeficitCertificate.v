From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import EligibleFundingAssignment.
Import ListNotations.

Definition funding_neighbor obligations (eligible : nat -> nat -> bool) selected source :=
  existsb (fun obligation => selected obligation && eligible source obligation)
    (seq 0 obligations).

Definition selected_funding_demand (selected : nat -> bool) (demand : nat -> nat) obligation :=
  if selected obligation then demand obligation else 0.

Definition neighbor_funding_capacity obligations eligible selected capacity source :=
  if funding_neighbor obligations eligible selected source then capacity source else 0.

Definition selected_funding_flow (selected : nat -> bool) (flow : funding_flow) source obligation :=
  if selected obligation then flow source obligation else 0.

Definition funding_deficit_check sources obligations eligible capacity demand selected :=
  funding_sum sources (neighbor_funding_capacity obligations eligible selected capacity) <?
  funding_sum obligations (selected_funding_demand selected demand).

Lemma funding_sum_monotone : forall count lhs rhs,
  (forall index, index < count -> lhs index <= rhs index) ->
  funding_sum count lhs <= funding_sum count rhs.
Proof.
  induction count; intros lhs rhs bounded; simpl; [lia|].
  assert (funding_sum count lhs <= funding_sum count rhs).
  { apply IHcount. intros. apply bounded. lia. }
  specialize (bounded count ltac:(lia)). lia.
Qed.

Lemma funding_neighbor_exact : forall obligations eligible selected source,
  funding_neighbor obligations eligible selected source = true <->
  exists obligation, obligation < obligations /\ selected obligation = true /\
    eligible source obligation = true.
Proof.
  intros. unfold funding_neighbor. rewrite existsb_exists.
  split; intros [obligation [inside matched]]; exists obligation.
  - apply in_seq in inside. apply andb_true_iff in matched. tauto.
  - destruct matched as [chosen permitted]. split.
    + apply in_seq. lia.
    + apply andb_true_iff. auto.
Qed.

Lemma nonneighbor_selected_flow_zero : forall sources obligations eligible capacity demand flow selected source,
  assignment_valid sources obligations eligible capacity demand flow ->
  source < sources -> funding_neighbor obligations eligible selected source = false ->
  source_draw obligations (selected_funding_flow selected flow) source = 0.
Proof.
  intros sources obligations eligible capacity demand flow selected source [rows _] inside absent.
  destruct (rows source inside) as [_ edges].
  unfold source_draw. rewrite <- (funding_sum_zero obligations).
  apply funding_sum_ext. intros obligation bounded.
  unfold selected_funding_flow. destruct (selected obligation) eqn:chosen; [|reflexivity].
  apply edges; [exact bounded|].
  destruct (eligible source obligation) eqn:permitted; [|reflexivity].
  assert (funding_neighbor obligations eligible selected source = true).
  { apply funding_neighbor_exact. exists obligation. auto. }
  congruence.
Qed.

Lemma selected_funding_rows_bounded : forall sources obligations eligible capacity demand flow selected source,
  assignment_valid sources obligations eligible capacity demand flow -> source < sources ->
  source_draw obligations (selected_funding_flow selected flow) source <=
  neighbor_funding_capacity obligations eligible selected capacity source.
Proof.
  intros sources obligations eligible capacity demand flow selected source valid inside.
  unfold neighbor_funding_capacity.
  destruct (funding_neighbor obligations eligible selected source) eqn:neighbor.
  - destruct valid as [rows _]. destruct (rows source inside) as [bound _].
    eapply Nat.le_trans; [|exact bound]. unfold source_draw.
    apply funding_sum_monotone. intros obligation _.
    unfold selected_funding_flow. destruct (selected obligation); lia.
  - erewrite nonneighbor_selected_flow_zero; eauto.
Qed.

Lemma selected_funding_columns_exact : forall sources obligations eligible capacity demand flow selected obligation,
  assignment_valid sources obligations eligible capacity demand flow -> obligation < obligations ->
  obligation_draw sources (selected_funding_flow selected flow) obligation =
  selected_funding_demand selected demand obligation.
Proof.
  intros sources obligations eligible capacity demand flow selected obligation [_ columns] inside.
  unfold obligation_draw, selected_funding_flow, selected_funding_demand.
  destruct (selected obligation).
  - apply columns. exact inside.
  - apply funding_sum_zero.
Qed.

Theorem valid_assignment_covers_every_selected_cut : forall sources obligations eligible capacity demand flow selected,
  assignment_valid sources obligations eligible capacity demand flow ->
  funding_sum obligations (selected_funding_demand selected demand) <=
  funding_sum sources (neighbor_funding_capacity obligations eligible selected capacity).
Proof.
  intros sources obligations eligible capacity demand flow selected valid.
  assert (columns : funding_sum obligations (selected_funding_demand selected demand) =
    funding_sum obligations (obligation_draw sources (selected_funding_flow selected flow))).
  { apply funding_sum_ext. intros. symmetry. eapply selected_funding_columns_exact; eauto. }
  rewrite columns, <- funding_assignment_rows_equal_columns.
  apply funding_sum_monotone. intros. eapply selected_funding_rows_bounded; eauto.
Qed.

Theorem funding_deficit_certificate_sound : forall sources obligations eligible capacity demand selected,
  funding_deficit_check sources obligations eligible capacity demand selected = true ->
  ~ exists flow, assignment_valid sources obligations eligible capacity demand flow.
Proof.
  intros sources obligations eligible capacity demand selected deficit [flow valid].
  unfold funding_deficit_check in deficit. apply Nat.ltb_lt in deficit.
  pose proof (valid_assignment_covers_every_selected_cut sources obligations eligible capacity demand flow selected valid).
  lia.
Qed.

Theorem valid_assignment_never_has_deficit : forall sources obligations eligible capacity demand flow selected,
  assignment_valid sources obligations eligible capacity demand flow ->
  funding_deficit_check sources obligations eligible capacity demand selected = false.
Proof.
  intros. unfold funding_deficit_check. apply Nat.ltb_ge.
  eapply valid_assignment_covers_every_selected_cut; eauto.
Qed.

Example aggregate_balance_does_not_cover_restricted_demand :
  funding_deficit_check 2 1 (fun source _ => Nat.eqb source 0)
    (fun source => if Nat.eqb source 0 then 1 else 100)
    (fun _ => 2) (fun _ => true) = true.
Proof. reflexivity. Qed.

Example empty_selection_is_not_a_deficit : forall sources obligations eligible capacity demand,
  funding_deficit_check sources obligations eligible capacity demand (fun _ => false) = false.
Proof.
  intros. unfold funding_deficit_check, selected_funding_demand.
  rewrite funding_sum_zero. apply Nat.ltb_ge. lia.
Qed.

Print Assumptions funding_neighbor_exact.
Print Assumptions valid_assignment_covers_every_selected_cut.
Print Assumptions funding_deficit_certificate_sound.
Print Assumptions valid_assignment_never_has_deficit.
