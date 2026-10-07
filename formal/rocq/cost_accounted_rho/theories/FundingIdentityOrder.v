From Stdlib Require Import Arith.PeanoNat Lists.List Lia Sorting.Permutation.
From CostAccountedRho Require Import EligibleFundingAssignment FundingResidualCut.
Import ListNotations.

Definition funding_presentation_matches sources obligations rows columns
    (proposed selected : funding_flow) : bool :=
  forallb (fun i => forallb (fun j => Nat.eqb (proposed (rows i) (columns j)) (selected i j))
    (seq 0 obligations)) (seq 0 sources).

Theorem funding_presentation_matches_exact : forall sources obligations rows columns proposed selected,
  funding_presentation_matches sources obligations rows columns proposed selected = true <->
  forall i j, i < sources -> j < obligations -> proposed (rows i) (columns j) = selected i j.
Proof.
  intros. unfold funding_presentation_matches. rewrite forallb_forall.
  split; intros checked.
  - intros i j row column. specialize (checked i).
    assert (member : In i (seq 0 sources)) by (apply in_seq; lia).
    specialize (checked member). rewrite forallb_forall in checked.
    apply Nat.eqb_eq. apply checked. apply in_seq. lia.
  - intros i member. apply in_seq in member. apply forallb_forall.
    intros j included. apply in_seq in included. apply Nat.eqb_eq. apply checked; lia.
Qed.

Theorem funding_presentation_rejects_different_entry : forall sources obligations rows columns
    proposed selected i j,
  i < sources -> j < obligations -> proposed (rows i) (columns j) <> selected i j ->
  funding_presentation_matches sources obligations rows columns proposed selected = false.
Proof.
  intros sources obligations rows columns proposed selected i j row column different.
  destruct (funding_presentation_matches sources obligations rows columns proposed selected) eqn:accepted; auto.
  exfalso. apply different.
  apply (proj1 (funding_presentation_matches_exact sources obligations rows columns proposed selected)
    accepted i j row column).
Qed.

Print Assumptions funding_presentation_matches_exact.
Print Assumptions funding_presentation_rejects_different_entry.

Definition funding_index_permutation count index :=
  Permutation (map index (seq 0 count)) (seq 0 count).

Lemma funding_list_sum_seed : forall values seed,
  fold_right Nat.add seed values = fold_right Nat.add 0 values + seed.
Proof. induction values; intros; simpl; [lia|rewrite IHvalues; lia]. Qed.

Lemma funding_sum_is_list_sum : forall count amount,
  funding_sum count amount = fold_right Nat.add 0 (map amount (seq 0 count)).
Proof.
  induction count; intros; [reflexivity|]. rewrite seq_S, map_app, fold_right_app. simpl.
  rewrite funding_list_sum_seed. rewrite <- IHcount. simpl. lia.
Qed.

Lemma funding_list_sum_permutation : forall left right,
  Permutation left right -> fold_right Nat.add 0 left = fold_right Nat.add 0 right.
Proof. intros left right same. induction same; simpl; lia. Qed.

Theorem funding_reindex_preserves_sum : forall count index amount,
  funding_index_permutation count index ->
  funding_sum count (fun i => amount (index i)) = funding_sum count amount.
Proof.
  intros count index amount same. rewrite !funding_sum_is_list_sum.
  rewrite <- map_map. apply funding_list_sum_permutation. now apply Permutation_map.
Qed.

Theorem funding_index_permutation_stays_in_bounds : forall count index i,
  funding_index_permutation count index -> i < count -> index i < count.
Proof.
  intros count index i same inside. assert (member : In (index i) (seq 0 count)).
  { eapply Permutation_in; [exact same|]. apply in_map. apply in_seq. lia. }
  apply in_seq in member. lia.
Qed.

Definition reindex_funding_flow (rows columns : nat -> nat) (flow : funding_flow) :=
  fun i j => flow (rows i) (columns j).

Theorem funding_reindex_preserves_source_draw : forall obligations rows columns flow source,
  funding_index_permutation obligations columns ->
  source_draw obligations (reindex_funding_flow rows columns flow) source =
    source_draw obligations flow (rows source).
Proof. intros. unfold source_draw, reindex_funding_flow. now apply funding_reindex_preserves_sum. Qed.

Theorem funding_reindex_preserves_obligation_draw : forall sources rows columns flow target,
  funding_index_permutation sources rows ->
  obligation_draw sources (reindex_funding_flow rows columns flow) target =
    obligation_draw sources flow (columns target).
Proof.
  intros. unfold obligation_draw, reindex_funding_flow.
  exact (funding_reindex_preserves_sum sources rows (fun source => flow source (columns target)) H).
Qed.

Theorem funding_reindex_preserves_assignment : forall sources obligations rows columns eligible capacity demand flow,
  funding_index_permutation sources rows -> funding_index_permutation obligations columns ->
  assignment_valid sources obligations eligible capacity demand flow ->
  assignment_valid sources obligations (fun i j => eligible (rows i) (columns j))
    (fun i => capacity (rows i)) (fun j => demand (columns j)) (reindex_funding_flow rows columns flow).
Proof.
  intros sources obligations rows columns eligible capacity demand flow row_order column_order [valid_rows valid_columns].
  split.
  - intros i inside. unfold source_valid. rewrite funding_reindex_preserves_source_draw by exact column_order.
    destruct (valid_rows (rows i) (funding_index_permutation_stays_in_bounds _ _ _ row_order inside)) as [bound edges].
    split; [exact bound|]. intros j included denied. unfold reindex_funding_flow.
    apply edges; [eapply funding_index_permutation_stays_in_bounds; eauto|exact denied].
  - intros j inside. rewrite funding_reindex_preserves_obligation_draw by exact row_order.
    apply valid_columns. eapply funding_index_permutation_stays_in_bounds; eauto.
Qed.

Theorem inverse_funding_reindex_restores_entries : forall rows columns inverse_rows inverse_columns flow i j,
  rows (inverse_rows i) = i -> columns (inverse_columns j) = j ->
  reindex_funding_flow inverse_rows inverse_columns (reindex_funding_flow rows columns flow) i j = flow i j.
Proof. intros. unfold reindex_funding_flow. now rewrite H, H0. Qed.

Theorem funding_reindex_preserves_branch_holds : forall branches obligations rows columns plans source,
  (forall branch, In branch branches -> funding_index_permutation obligations (columns branch)) ->
  branch_reservation branches obligations
    (fun branch => reindex_funding_flow rows (columns branch) (plans branch)) source =
  branch_reservation branches obligations plans (rows source).
Proof.
  induction branches as [|branch rest IH]; intros obligations rows columns plans source orders; simpl; [reflexivity|].
  rewrite funding_reindex_preserves_source_draw by (apply orders; now left).
  rewrite IH; [reflexivity|]. intros other member. apply orders. now right.
Qed.

Theorem funding_reindex_preserves_captured_refunds : forall branches obligations rows columns plans branch source,
  In branch branches ->
  (forall selected, In selected branches -> funding_index_permutation obligations (columns selected)) ->
  selected_branch_refund branches obligations
    (fun selected => reindex_funding_flow rows (columns selected) (plans selected)) branch source =
  selected_branch_refund branches obligations plans branch (rows source).
Proof.
  intros branches obligations rows columns plans branch source member orders.
  unfold selected_branch_refund. rewrite funding_reindex_preserves_branch_holds by exact orders.
  rewrite funding_reindex_preserves_source_draw by (apply orders; exact member). reflexivity.
Qed.

Fixpoint variable_branch_reservation (branches : list nat) (obligations : nat -> nat)
  (plans : nat -> funding_flow) source : nat :=
  match branches with
  | [] => 0
  | branch :: rest => Nat.max (source_draw (obligations branch) (plans branch) source)
      (variable_branch_reservation rest obligations plans source)
  end.

Definition variable_branch_refund branches obligations plans branch source :=
  variable_branch_reservation branches obligations plans source -
    source_draw (obligations branch) (plans branch) source.

Theorem funding_reindex_preserves_variable_branch_holds : forall branches obligations rows columns plans source,
  (forall branch, In branch branches -> funding_index_permutation (obligations branch) (columns branch)) ->
  variable_branch_reservation branches obligations
    (fun branch => reindex_funding_flow rows (columns branch) (plans branch)) source =
  variable_branch_reservation branches obligations plans (rows source).
Proof.
  induction branches as [|branch rest IH]; intros obligations rows columns plans source orders; simpl; [reflexivity|].
  rewrite funding_reindex_preserves_source_draw by (apply orders; now left).
  rewrite IH; [reflexivity|]. intros other member. apply orders. now right.
Qed.

Theorem funding_reindex_preserves_variable_branch_refunds : forall branches obligations rows columns plans branch source,
  In branch branches ->
  (forall selected, In selected branches -> funding_index_permutation (obligations selected) (columns selected)) ->
  variable_branch_refund branches obligations
    (fun selected => reindex_funding_flow rows (columns selected) (plans selected)) branch source =
  variable_branch_refund branches obligations plans branch (rows source).
Proof.
  intros branches obligations rows columns plans branch source member orders.
  unfold variable_branch_refund. rewrite funding_reindex_preserves_variable_branch_holds by exact orders.
  rewrite funding_reindex_preserves_source_draw by (apply orders; exact member). reflexivity.
Qed.

Theorem variable_branch_reservation_contains_selected : forall branches obligations plans branch source,
  In branch branches ->
  source_draw (obligations branch) (plans branch) source <=
    variable_branch_reservation branches obligations plans source.
Proof.
  induction branches as [|head rest IH]; intros obligations plans branch source member; simpl in *; [contradiction|].
  destruct member as [same|member]; [subst; apply Nat.le_max_l|].
  eapply Nat.le_trans; [apply IH; exact member|apply Nat.le_max_r].
Qed.

Theorem variable_branch_hold_equals_debit_and_refund : forall branches obligations plans branch source,
  In branch branches ->
  variable_branch_reservation branches obligations plans source =
    source_draw (obligations branch) (plans branch) source +
      variable_branch_refund branches obligations plans branch source.
Proof.
  intros. unfold variable_branch_refund.
  pose proof (variable_branch_reservation_contains_selected branches obligations plans branch source H). lia.
Qed.
