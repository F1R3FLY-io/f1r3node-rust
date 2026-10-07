From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import EligibleFundingAssignment FundingDeficitCertificate FundingDomainCuts.

Definition box_funding_valid sources obligations eligible lower upper demand flow :=
  assignment_valid sources obligations eligible upper demand flow /\
  forall source, source < sources -> lower source <= source_draw obligations flow source.

Definition lower_funding_cut_check sources obligations eligible lower demand selected :=
  source_neighborhood_demand sources obligations eligible selected demand <?
  source_subset_capacity sources selected lower.

Theorem transpose_complete_funding_assignment : forall sources obligations eligible upper demand flow,
  assignment_valid sources obligations eligible upper demand flow ->
  assignment_valid obligations sources (fun j i => eligible i j) demand
    (source_draw obligations flow) (fun j i => flow i j).
Proof.
  intros sources obligations eligible upper demand flow [rows columns].
  split; [|reflexivity]. intros j inside. split.
  - unfold source_draw. change (obligation_draw sources flow j <= demand j).
    rewrite columns by assumption. lia.
  - intros i bounded excluded. apply (proj2 (rows i bounded) j inside excluded).
Qed.

Theorem bounded_assignment_covers_lower_cuts : forall sources obligations eligible lower upper demand flow selected,
  box_funding_valid sources obligations eligible lower upper demand flow ->
  source_subset_capacity sources selected lower <=
  source_neighborhood_demand sources obligations eligible selected demand.
Proof.
  intros sources obligations eligible lower upper demand flow selected [valid lower_bound].
  pose proof (transpose_complete_funding_assignment _ _ _ _ _ _ valid) as transposed.
  pose proof (valid_assignment_covers_every_selected_cut _ _ _ _ _ _ selected transposed) as covered.
  eapply Nat.le_trans; [|exact covered].
  apply funding_sum_monotone. intros i inside. unfold selected_funding_demand.
  destruct (selected i); [apply lower_bound; assumption|lia].
Qed.

Theorem lower_funding_cut_rejects_every_bounded_assignment : forall sources obligations eligible lower upper demand selected,
  lower_funding_cut_check sources obligations eligible lower demand selected = true ->
  ~ exists flow, box_funding_valid sources obligations eligible lower upper demand flow.
Proof.
  intros sources obligations eligible lower upper demand selected failed [flow valid].
  unfold lower_funding_cut_check in failed. apply Nat.ltb_lt in failed.
  pose proof (bounded_assignment_covers_lower_cuts _ _ _ _ _ _ _ selected valid). lia.
Qed.

Definition lower_rebalance_closed sources obligations eligible flow selected :=
  forall i j, i < sources -> j < obligations ->
    source_neighborhood sources eligible selected j = true ->
    0 < flow i j -> selected i = true.

Theorem closed_lower_set_accounts_for_neighbor_demand : forall sources obligations eligible upper demand flow selected,
  assignment_valid sources obligations eligible upper demand flow ->
  lower_rebalance_closed sources obligations eligible flow selected ->
  source_subset_capacity sources selected (source_draw obligations flow) =
  source_neighborhood_demand sources obligations eligible selected demand.
Proof.
  intros sources obligations eligible upper demand flow selected valid closed.
  pose proof (transpose_complete_funding_assignment _ _ _ _ _ _ valid) as transposed.
  set (chosen := selected_funding_flow selected (fun j i => flow i j)).
  assert (columns : forall i, i < sources ->
    obligation_draw obligations chosen i = selected_funding_demand selected (source_draw obligations flow) i).
  { intros. apply (selected_funding_columns_exact _ _ _ _ _ _ selected _ transposed H). }
  unfold source_subset_capacity. rewrite <- (funding_sum_ext sources _ _ columns).
  rewrite <- funding_assignment_rows_equal_columns.
  apply funding_sum_ext. intros j inside.
  unfold source_neighborhood_demand, selected_funding_demand.
  destruct (source_neighborhood sources eligible selected j) eqn:neighbor.
  - transitivity (obligation_draw sources flow j); [|apply (proj2 valid); assumption].
    unfold source_draw, obligation_draw, chosen, selected_funding_flow.
    apply funding_sum_ext. intros i bounded.
    destruct (selected i) eqn:member; [reflexivity|].
    destruct (Nat.eq_dec (flow i j) 0); [lia|].
    assert (selected i = true) by (apply (closed i j); auto; lia). congruence.
  - unfold chosen. apply (nonneighbor_selected_flow_zero _ _ _ _ _ _ selected j transposed inside neighbor).
Qed.

Theorem exhausted_lower_rebalance_has_deficit : forall sources obligations eligible lower upper demand flow selected receiver,
  assignment_valid sources obligations eligible upper demand flow ->
  lower_rebalance_closed sources obligations eligible flow selected ->
  receiver < sources -> selected receiver = true ->
  source_draw obligations flow receiver < lower receiver ->
  (forall i, i < sources -> selected i = true -> source_draw obligations flow i <= lower i) ->
  lower_funding_cut_check sources obligations eligible lower demand selected = true.
Proof.
  intros sources obligations eligible lower upper demand flow selected receiver valid closed inside chosen short no_donor.
  unfold lower_funding_cut_check. apply Nat.ltb_lt.
  rewrite <- (closed_lower_set_accounts_for_neighbor_demand _ _ _ _ _ _ _ valid closed).
  apply funding_sum_strict_entry with (index := receiver).
  - intros i bounded. unfold selected_funding_demand. destruct (selected i) eqn:member; [apply no_donor; assumption|lia].
  - exact inside.
  - unfold selected_funding_demand. rewrite chosen. exact short.
Qed.

Definition funding_handoff (flow : funding_flow) receiver donor obligation amount : funding_flow :=
  fun i j => if Nat.eqb j obligation then
    if Nat.eqb i receiver then flow i j + amount
    else if Nat.eqb i donor then flow i j - amount else flow i j
  else flow i j.

Lemma funding_sum_replace : forall count values index replacement,
  index < count ->
  funding_sum count (fun i => if Nat.eqb i index then replacement else values i) + values index =
  funding_sum count values + replacement.
Proof.
  induction count; intros values index replacement inside; [lia|].
  simpl. destruct (Nat.eq_dec index count) as [->|different].
  - rewrite Nat.eqb_refl.
    assert (prefix : funding_sum count (fun i => if Nat.eqb i count then replacement else values i) = funding_sum count values).
    { apply funding_sum_ext. intros i bounded.
      assert (Nat.eqb i count = false) by (apply Nat.eqb_neq; lia). now rewrite H. }
    rewrite prefix. lia.
  - assert (last : Nat.eqb count index = false) by (apply Nat.eqb_neq; lia).
    rewrite last. specialize (IHcount values index replacement ltac:(lia)). lia.
Qed.

Lemma funding_sum_pulse : forall count index amount,
  index < count -> funding_sum count (fun i => if Nat.eqb i index then amount else 0) = amount.
Proof.
  intros count index amount inside.
  pose proof (funding_sum_replace count (fun _ => 0) index amount inside).
  rewrite funding_sum_zero in H. lia.
Qed.

Theorem handoff_preserves_columns : forall sources flow receiver donor obligation amount j,
  receiver < sources -> donor < sources -> receiver <> donor -> amount <= flow donor obligation ->
  obligation_draw sources (funding_handoff flow receiver donor obligation amount) j = obligation_draw sources flow j.
Proof.
  intros sources flow receiver donor obligation amount j ri di different bounded.
  unfold obligation_draw.
  destruct (Nat.eqb j obligation) eqn:same.
  - apply Nat.eqb_eq in same. subst j.
    assert (balance : forall i, i < sources ->
      funding_handoff flow receiver donor obligation amount i obligation + (if i =? donor then amount else 0) =
      flow i obligation + (if i =? receiver then amount else 0)).
    { intros i _. unfold funding_handoff. rewrite Nat.eqb_refl.
      destruct (i =? receiver) eqn:r, (i =? donor) eqn:d.
      - apply Nat.eqb_eq in r, d. congruence.
      - lia.
      - apply Nat.eqb_eq in d. subst i. lia.
      - lia. }
    pose proof (funding_sum_ext sources _ _ balance) as totals.
    rewrite !funding_sum_add, !funding_sum_pulse in totals by assumption. lia.
  - apply funding_sum_ext. intros. unfold funding_handoff. now rewrite same.
Qed.

Theorem handoff_preserves_eligibility : forall sources obligations eligible flow receiver donor obligation amount,
  eligible receiver obligation = true ->
  (forall i j, i < sources -> j < obligations -> eligible i j = false -> flow i j = 0) ->
  forall i j, i < sources -> j < obligations -> eligible i j = false -> funding_handoff flow receiver donor obligation amount i j = 0.
Proof.
  intros sources obligations eligible flow receiver donor obligation amount permitted original i j inside bounded excluded.
  unfold funding_handoff. destruct (j =? obligation) eqn:at_obligation.
  - apply Nat.eqb_eq in at_obligation. subst j. destruct (i =? receiver) eqn:at_receiver.
    + apply Nat.eqb_eq in at_receiver. subst i. congruence.
    + rewrite original by assumption. destruct (i =? donor); reflexivity.
  - apply original; assumption.
Qed.

Theorem handoff_source_balance : forall obligations flow receiver donor obligation amount i,
  receiver <> donor -> obligation < obligations -> amount <= flow donor obligation ->
  source_draw obligations (funding_handoff flow receiver donor obligation amount) i +
    (if Nat.eqb i donor then amount else 0) =
  source_draw obligations flow i + (if Nat.eqb i receiver then amount else 0).
Proof.
  intros obligations flow receiver donor obligation amount i different inside bounded.
  unfold source_draw, funding_handoff.
  assert (shape : forall j, j < obligations -> (if j =? obligation then
    if i =? receiver then flow i j + amount else if i =? donor then flow i j - amount else flow i j
    else flow i j) =
    (if j =? obligation then
      (if i =? receiver then flow i obligation + amount else if i =? donor then flow i obligation - amount else flow i obligation)
      else flow i j)).
  { intros j _. destruct (Nat.eqb j obligation) eqn:same; [apply Nat.eqb_eq in same; now subst|reflexivity]. }
  rewrite (funding_sum_ext _ _ _ shape).
  pose proof (funding_sum_replace obligations (flow i) obligation
    (if i =? receiver then flow i obligation + amount else if i =? donor then flow i obligation - amount else flow i obligation) inside).
  destruct (Nat.eqb i receiver) eqn:r, (Nat.eqb i donor) eqn:d.
  - apply Nat.eqb_eq in r, d. congruence.
  - lia.
  - apply Nat.eqb_eq in d. subst. lia.
  - lia.
Qed.

Inductive funding_handoff_path sources obligations eligible : funding_flow -> funding_flow -> nat -> nat -> nat -> Prop :=
| funding_handoff_stay : forall flow receiver amount,
    receiver < sources -> funding_handoff_path sources obligations eligible flow flow receiver receiver amount
| funding_handoff_step : forall initial final receiver middle donor obligation amount,
    receiver < sources -> middle < sources -> obligation < obligations -> receiver <> middle ->
    eligible receiver obligation = true -> amount <= initial middle obligation ->
    funding_handoff_path sources obligations eligible
      (funding_handoff initial receiver middle obligation amount) final middle donor amount ->
    funding_handoff_path sources obligations eligible initial final receiver donor amount.

Theorem handoff_path_source_balance : forall sources obligations eligible initial final receiver donor amount,
  funding_handoff_path sources obligations eligible initial final receiver donor amount ->
  forall i, source_draw obligations final i + (if i =? donor then amount else 0) =
    source_draw obligations initial i + (if i =? receiver then amount else 0).
Proof.
  intros sources obligations eligible initial final receiver donor amount path.
  induction path; intros i; [reflexivity|]. specialize (IHpath i).
  pose proof (handoff_source_balance obligations initial receiver middle obligation amount i H2 H1 H4).
  lia.
Qed.

Theorem handoff_path_preserves_columns : forall sources obligations eligible initial final receiver donor amount,
  funding_handoff_path sources obligations eligible initial final receiver donor amount ->
  forall j, obligation_draw sources final j = obligation_draw sources initial j.
Proof.
  intros sources obligations eligible initial final receiver donor amount path.
  induction path; intros j; [reflexivity|]. rewrite IHpath.
  now apply handoff_preserves_columns.
Qed.

Theorem handoff_path_preserves_eligibility : forall sources obligations eligible initial final receiver donor amount,
  funding_handoff_path sources obligations eligible initial final receiver donor amount ->
  (forall i j, i < sources -> j < obligations -> eligible i j = false -> initial i j = 0) ->
  forall i j, i < sources -> j < obligations -> eligible i j = false -> final i j = 0.
Proof.
  intros sources obligations eligible initial final receiver donor amount path.
  induction path; intros original; [exact original|].
  apply IHpath. eapply handoff_preserves_eligibility; eauto.
Qed.

Definition funding_lower_deficit sources obligations lower flow :=
  funding_sum sources (fun i => lower i - source_draw obligations flow i).

Theorem handoff_path_reduces_lower_deficit : forall sources obligations eligible initial final receiver donor amount lower,
  funding_handoff_path sources obligations eligible initial final receiver donor amount ->
  receiver < sources -> receiver <> donor ->
  source_draw obligations initial receiver + amount <= lower receiver ->
  lower donor + amount <= source_draw obligations initial donor ->
  funding_lower_deficit sources obligations lower final + amount =
    funding_lower_deficit sources obligations lower initial.
Proof.
  intros sources obligations eligible initial final receiver donor amount lower path inside distinct receiver_bound donor_bound.
  assert (delta : forall i, i < sources ->
    (lower i - source_draw obligations final i) + (if i =? receiver then amount else 0) =
    lower i - source_draw obligations initial i).
  { intros i _. pose proof (handoff_path_source_balance _ _ _ _ _ _ _ _ path i) as balance.
    destruct (i =? receiver) eqn:r, (i =? donor) eqn:d.
    - apply Nat.eqb_eq in r, d. congruence.
    - apply Nat.eqb_eq in r. subst i. lia.
    - apply Nat.eqb_eq in d. subst i. lia.
    - lia. }
  pose proof (funding_sum_ext sources _ _ delta) as totals.
  rewrite funding_sum_add, funding_sum_pulse in totals by assumption. exact totals.
Qed.

Theorem handoff_path_preserves_upper_and_lower_progress : forall sources obligations eligible initial final receiver donor amount lower upper,
  funding_handoff_path sources obligations eligible initial final receiver donor amount ->
  receiver <> donor ->
  source_draw obligations initial receiver + amount <= lower receiver ->
  lower donor + amount <= source_draw obligations initial donor ->
  (forall i, i < sources -> lower i <= upper i /\ source_draw obligations initial i <= upper i) ->
  forall i, i < sources -> source_draw obligations final i <= upper i /\
    Nat.min (lower i) (source_draw obligations initial i) <= source_draw obligations final i.
Proof.
  intros sources obligations eligible initial final receiver donor amount lower upper path distinct receiver_bound donor_bound bounded i inside.
  pose proof (handoff_path_source_balance _ _ _ _ _ _ _ _ path i) as balance.
  specialize (bounded i inside).
  destruct (i =? receiver) eqn:r, (i =? donor) eqn:d.
  - apply Nat.eqb_eq in r, d. congruence.
  - apply Nat.eqb_eq in r. subst i. split; [lia|]. eapply Nat.le_trans; [apply Nat.le_min_r|lia].
  - apply Nat.eqb_eq in d. subst i. split; [lia|]. eapply Nat.le_trans; [apply Nat.le_min_l|lia].
  - split; [lia|]. eapply Nat.le_trans; [apply Nat.le_min_r|lia].
Qed.

Theorem lower_rebalance_preserves_assignment_and_progress : forall sources obligations eligible initial final receiver donor amount lower upper demand,
  assignment_valid sources obligations eligible upper demand initial ->
  (forall i, i < sources -> lower i <= upper i) ->
  funding_handoff_path sources obligations eligible initial final receiver donor amount ->
  receiver <> donor ->
  source_draw obligations initial receiver + amount <= lower receiver ->
  lower donor + amount <= source_draw obligations initial donor ->
  assignment_valid sources obligations eligible upper demand final /\
  forall i, i < sources -> Nat.min (lower i) (source_draw obligations initial i) <= source_draw obligations final i.
Proof.
  intros sources obligations eligible initial final receiver donor amount lower upper demand valid bounds path distinct receiver_bound donor_bound.
  assert (progress : forall i, i < sources -> source_draw obligations final i <= upper i /\
    Nat.min (lower i) (source_draw obligations initial i) <= source_draw obligations final i).
  { eapply handoff_path_preserves_upper_and_lower_progress; eauto.
    intros i inside. split; [apply bounds; assumption|apply (proj1 (proj1 valid i inside))]. }
  split.
  - split.
    + intros i inside. split; [apply (proj1 (progress i inside))|].
      intros j bounded excluded.
      eapply handoff_path_preserves_eligibility; [exact path| |exact inside|exact bounded|exact excluded].
      intros a b ai bi edge. apply (proj2 (proj1 valid a ai) b bi edge).
    + intros j inside. rewrite (handoff_path_preserves_columns _ _ _ _ _ _ _ _ path).
      apply (proj2 valid); assumption.
  - intros i inside. apply (proj2 (progress i inside)).
Qed.

Theorem positive_lower_rebalance_strictly_progresses : forall sources obligations eligible initial final receiver donor amount lower,
  funding_handoff_path sources obligations eligible initial final receiver donor amount ->
  receiver < sources -> receiver <> donor -> 0 < amount ->
  source_draw obligations initial receiver + amount <= lower receiver ->
  lower donor + amount <= source_draw obligations initial donor ->
  funding_lower_deficit sources obligations lower final < funding_lower_deficit sources obligations lower initial.
Proof.
  intros. pose proof (handoff_path_reduces_lower_deficit _ _ _ _ _ _ _ _ _ H H0 H1 H3 H4). lia.
Qed.
