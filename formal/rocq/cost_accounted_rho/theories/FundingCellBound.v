From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import EligibleFundingAssignment FundingDeficitCertificate CompleteFundingCandidates FundingBox FundingMinimaxCertificate.

Definition funding_row_take limit row index := Nat.min (row index) (limit - funding_sum index row).

Lemma funding_row_take_bounded : forall limit row index,
  funding_row_take limit row index <= row index.
Proof. intros. apply Nat.le_min_l. Qed.

Theorem funding_row_take_total : forall count limit row,
  funding_sum count (funding_row_take limit row) = Nat.min limit (funding_sum count row).
Proof.
  induction count; intros limit row; simpl; [lia|].
  rewrite IHcount. unfold funding_row_take.
  destruct (Nat.le_ge_cases limit (funding_sum count row)).
  - rewrite Nat.min_l by assumption. replace (limit - funding_sum count row) with 0 by lia.
    rewrite Nat.min_r by lia. rewrite Nat.min_l by lia. lia.
  - rewrite Nat.min_r by assumption. destruct (Nat.le_ge_cases (row count) (limit - funding_sum count row)).
    + rewrite Nat.min_l by assumption. rewrite Nat.min_r by lia. lia.
    + rewrite Nat.min_r by assumption. rewrite Nat.min_l by lia. lia.
Qed.

Definition funding_priority_row limit row target index :=
  if Nat.eqb index target then row target
  else funding_row_take (limit - row target) (fun j => if Nat.eqb j target then 0 else row j) index.

Theorem funding_priority_row_preserves_target : forall limit row target,
  funding_priority_row limit row target target = row target.
Proof. intros. unfold funding_priority_row. now rewrite Nat.eqb_refl. Qed.

Theorem funding_priority_row_is_bounded : forall limit row target index,
  funding_priority_row limit row target index <= row index.
Proof.
  intros. unfold funding_priority_row. destruct (Nat.eqb index target) eqn:same.
  - apply Nat.eqb_eq in same. subst. lia.
  - eapply Nat.le_trans; [apply funding_row_take_bounded|]. rewrite same. lia.
Qed.

Theorem funding_priority_row_has_exact_total : forall count limit row target,
  target < count -> row target <= limit -> limit <= funding_sum count row ->
  funding_sum count (funding_priority_row limit row target) = limit.
Proof.
  intros count limit row target inside included bounded.
  set (remaining := fun j => if Nat.eqb j target then 0 else row j).
  assert (remaining_total : funding_sum count remaining + row target = funding_sum count row).
  { pose proof (funding_sum_replace count row target 0 inside). unfold remaining. lia. }
  assert (taken_at_target : funding_row_take (limit - row target) remaining target = 0).
  { unfold funding_row_take, remaining. rewrite Nat.eqb_refl. simpl. reflexivity. }
  pose proof (funding_sum_replace count (funding_row_take (limit - row target) remaining) target (row target) inside) as total.
  rewrite taken_at_target, funding_row_take_total in total.
  rewrite Nat.min_l in total by lia.
  change (funding_sum count (fun i => if i =? target then row target else funding_row_take (limit - row target) remaining i) = limit).
  lia.
Qed.

Definition funding_cell_cap sources capacity source limit i :=
  if Nat.eqb i sources then capacity source - limit
  else if Nat.eqb i source then limit else capacity i.

Definition funding_cell_edges sources eligible source target i j :=
  if Nat.eqb i sources then eligible source j && negb (Nat.eqb j target) else eligible i j.

Definition funding_cell_split sources flow source target limit i j :=
  if Nat.eqb i sources then flow source j - funding_priority_row limit (flow source) target j
  else if Nat.eqb i source then funding_priority_row limit (flow source) target j else flow i j.

Definition funding_cell_join sources (flow : funding_flow) source i j :=
  if Nat.eqb i source then flow source j + flow sources j else flow i j.

Theorem funding_cell_split_preserves_columns : forall sources flow source target limit j,
  source < sources ->
  obligation_draw (S sources) (funding_cell_split sources flow source target limit) j = obligation_draw sources flow j.
Proof.
  intros sources flow source target limit j inside.
  unfold obligation_draw. simpl.
  assert (prefix : funding_sum sources (fun i => funding_cell_split sources flow source target limit i j) =
    funding_sum sources (fun i => if Nat.eqb i source then funding_priority_row limit (flow source) target j else flow i j)).
  { apply funding_sum_ext. intros i bounded. unfold funding_cell_split.
    assert (Nat.eqb i sources = false) by (apply Nat.eqb_neq; lia). now rewrite H. }
  rewrite prefix. unfold funding_cell_split. rewrite Nat.eqb_refl.
  pose proof (funding_sum_replace sources (fun i => flow i j) source (funding_priority_row limit (flow source) target j) inside).
  pose proof (funding_priority_row_is_bounded limit (flow source) target j). lia.
Qed.

Theorem funding_cell_split_is_valid : forall sources obligations eligible capacity demand flow source target limit,
  source < sources -> target < obligations -> limit <= capacity source ->
  assignment_valid sources obligations eligible capacity demand flow ->
  funding_sum sources capacity = funding_sum obligations demand ->
  flow source target <= limit ->
  assignment_valid (S sources) obligations (funding_cell_edges sources eligible source target)
    (funding_cell_cap sources capacity source limit) demand
    (funding_cell_split sources flow source target limit).
Proof.
  intros sources obligations eligible capacity demand flow source target limit inside target_inside limit_bound valid total cell_bound.
  pose proof (capacity_total_forces_every_source_exact _ _ _ _ _ _ valid total source inside) as exact_row.
  unfold source_draw in exact_row.
  pose proof (funding_priority_row_has_exact_total obligations limit (flow source) target target_inside cell_bound ltac:(lia)) as taken_total.
  split.
  - intros i bounded. unfold source_valid. destruct (Nat.eq_dec i sources) as [->|original].
    + assert (row : forall j, funding_cell_split sources flow source target limit sources j =
        flow source j - funding_priority_row limit (flow source) target j).
      { intro j. unfold funding_cell_split. now rewrite Nat.eqb_refl. }
      split.
      * unfold source_draw. rewrite (funding_sum_ext obligations _ _ (fun j _ => row j)).
        assert (balance : funding_sum obligations (fun j => flow source j - funding_priority_row limit (flow source) target j) +
          funding_sum obligations (funding_priority_row limit (flow source) target) = source_draw obligations flow source).
        { rewrite <- funding_sum_add. unfold source_draw. apply funding_sum_ext. intros j _.
          pose proof (funding_priority_row_is_bounded limit (flow source) target j). lia. }
        unfold source_draw in balance. unfold funding_cell_cap. rewrite Nat.eqb_refl. lia.
      * intros j ji excluded. rewrite row. unfold funding_cell_edges in excluded. rewrite Nat.eqb_refl in excluded.
        apply andb_false_iff in excluded. destruct excluded as [excluded|target_equal].
        -- pose proof (proj2 (proj1 valid source inside) j ji excluded). lia.
        -- apply negb_false_iff, Nat.eqb_eq in target_equal. subst j.
           rewrite funding_priority_row_preserves_target. lia.
    + assert (si : i < sources) by lia.
      assert (last : Nat.eqb i sources = false) by (apply Nat.eqb_neq; assumption).
      destruct (Nat.eq_dec i source) as [->|other].
      * assert (row : forall j, funding_cell_split sources flow source target limit source j = funding_priority_row limit (flow source) target j).
        { intro j. unfold funding_cell_split. rewrite last, Nat.eqb_refl. reflexivity. }
        split.
        -- unfold source_draw. rewrite (funding_sum_ext obligations _ _ (fun j _ => row j)).
           unfold funding_cell_cap. rewrite last, Nat.eqb_refl. lia.
        -- intros j ji excluded. rewrite row. unfold funding_cell_edges in excluded. rewrite last in excluded.
           pose proof (proj2 (proj1 valid source inside) j ji excluded).
           pose proof (funding_priority_row_is_bounded limit (flow source) target j). lia.
      * assert (different : Nat.eqb i source = false) by (apply Nat.eqb_neq; assumption).
        assert (row : forall j, funding_cell_split sources flow source target limit i j = flow i j).
        { intro j. unfold funding_cell_split. now rewrite last, different. }
        destruct (proj1 valid i si) as [bound edges]. split.
        -- unfold source_draw. rewrite (funding_sum_ext obligations _ _ (fun j _ => row j)).
           unfold funding_cell_cap. rewrite last, different. exact bound.
        -- intros j ji excluded. rewrite row. apply edges; [exact ji|].
           unfold funding_cell_edges in excluded. now rewrite last in excluded.
  - intros j ji. rewrite funding_cell_split_preserves_columns by exact inside. apply (proj2 valid). exact ji.
Qed.

Theorem funding_cell_join_preserves_columns : forall sources flow source j,
  source < sources ->
  obligation_draw sources (funding_cell_join sources flow source) j = obligation_draw (S sources) flow j.
Proof.
  intros sources flow source j inside. unfold obligation_draw, funding_cell_join. simpl.
  pose proof (funding_sum_replace sources (fun i => flow i j) source (flow source j + flow sources j) inside). lia.
Qed.

Theorem funding_cell_join_is_valid : forall sources obligations eligible capacity demand flow source target limit,
  source < sources -> target < obligations -> limit <= capacity source ->
  assignment_valid (S sources) obligations (funding_cell_edges sources eligible source target)
    (funding_cell_cap sources capacity source limit) demand flow ->
  assignment_valid sources obligations eligible capacity demand (funding_cell_join sources flow source) /\
    funding_cell_join sources flow source source target <= limit.
Proof.
  intros sources obligations eligible capacity demand flow source target limit inside target_inside limit_bound [rows columns].
  assert (original : source < S sources) by lia.
  assert (source_last : Nat.eqb source sources = false) by (apply Nat.eqb_neq; lia).
  destruct (rows source original) as [first_bound first_edges].
  destruct (rows sources ltac:(lia)) as [last_bound last_edges].
  unfold funding_cell_cap in first_bound, last_bound. rewrite source_last, Nat.eqb_refl in first_bound.
  rewrite Nat.eqb_refl in last_bound.
  split.
  - split.
    + intros i ii. destruct (Nat.eq_dec i source) as [->|other].
      * split.
        -- unfold source_draw, funding_cell_join. rewrite Nat.eqb_refl, funding_sum_add. unfold source_draw in *. lia.
        -- intros j ji excluded. unfold funding_cell_join. rewrite Nat.eqb_refl.
           assert (first : flow source j = 0).
           { apply first_edges; [exact ji|]. unfold funding_cell_edges. now rewrite source_last. }
           assert (last : flow sources j = 0).
           { apply last_edges; [exact ji|]. unfold funding_cell_edges. rewrite Nat.eqb_refl, excluded. reflexivity. }
           lia.
      * assert (different : Nat.eqb i source = false) by (apply Nat.eqb_neq; assumption).
        assert (last : Nat.eqb i sources = false) by (apply Nat.eqb_neq; lia).
        destruct (rows i ltac:(lia)) as [bound edges]. split.
        -- unfold source_draw, funding_cell_join. rewrite different. unfold funding_cell_cap in bound. now rewrite last, different in bound.
        -- intros j ji excluded. unfold funding_cell_join. rewrite different. apply edges; [exact ji|].
           unfold funding_cell_edges. now rewrite last.
    + intros j ji. rewrite funding_cell_join_preserves_columns by exact inside. now apply columns.
  - assert (last : flow sources target = 0).
    { apply last_edges; [exact target_inside|]. unfold funding_cell_edges. rewrite !Nat.eqb_refl. simpl. apply andb_false_r. }
    unfold funding_cell_join. rewrite Nat.eqb_refl, last.
    pose proof (funding_sum_contains_entry obligations (flow source) target target_inside).
    unfold source_draw in first_bound. lia.
Qed.

Theorem funding_cell_bound_reduction_exact : forall sources obligations eligible capacity demand source target limit,
  source < sources -> target < obligations -> limit <= capacity source ->
  funding_sum sources capacity = funding_sum obligations demand ->
  ((exists flow, assignment_valid sources obligations eligible capacity demand flow /\ flow source target <= limit) <->
   exists flow, assignment_valid (S sources) obligations (funding_cell_edges sources eligible source target)
      (funding_cell_cap sources capacity source limit) demand flow).
Proof.
  intros sources obligations eligible capacity demand source target limit inside target_inside bounded total. split.
  - intros [flow [valid cell]]. exists (funding_cell_split sources flow source target limit).
    now apply funding_cell_split_is_valid.
  - intros [flow valid]. exists (funding_cell_join sources flow source).
    now apply funding_cell_join_is_valid.
Qed.

Theorem funding_cell_query_is_monotone : forall sources obligations eligible capacity demand source target lower upper,
  source < sources -> target < obligations -> lower <= upper -> upper <= capacity source ->
  funding_sum sources capacity = funding_sum obligations demand ->
  (exists flow, assignment_valid (S sources) obligations (funding_cell_edges sources eligible source target)
    (funding_cell_cap sources capacity source lower) demand flow) ->
  exists flow, assignment_valid (S sources) obligations (funding_cell_edges sources eligible source target)
    (funding_cell_cap sources capacity source upper) demand flow.
Proof.
  intros sources obligations eligible capacity demand source target lower upper inside ti ordered bounded total feasible.
  apply (proj2 (funding_cell_bound_reduction_exact sources obligations eligible capacity demand source target lower inside ti ltac:(lia) total)) in feasible.
  destruct feasible as [flow [valid cell]].
  apply (proj1 (funding_cell_bound_reduction_exact sources obligations eligible capacity demand source target upper inside ti bounded total)).
  exists flow. split; [exact valid|lia].
Qed.

Theorem funding_cell_cut_proves_lower_bound : forall sources obligations eligible capacity demand source target limit selected,
  source < sources -> target < obligations -> limit <= capacity source ->
  funding_sum sources capacity = funding_sum obligations demand ->
  funding_deficit_check (S sources) obligations (funding_cell_edges sources eligible source target)
    (funding_cell_cap sources capacity source limit) demand selected = true ->
  forall flow, assignment_valid sources obligations eligible capacity demand flow -> limit < flow source target.
Proof.
  intros sources obligations eligible capacity demand source target limit selected inside ti bounded total deficit flow valid.
  destruct (Nat.le_gt_cases (flow source target) limit) as [cell|larger]; [|exact larger].
  exfalso. apply (funding_deficit_certificate_sound _ _ _ _ _ _ deficit).
  exists (funding_cell_split sources flow source target limit). now apply funding_cell_split_is_valid.
Qed.
