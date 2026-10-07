From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import EligibleFundingAssignment FundingCellBound PrepaidResourceDischarge.

Definition counted_payment_valid sources allowed demand draw :=
  funding_sum sources draw = demand /\
  forall source, source < sources -> allowed source = false -> draw source = 0.

Definition counted_payment_first demand draw := funding_row_take demand draw.
Definition counted_payment_rest demand draw source :=
  draw source - counted_payment_first demand draw source.

Theorem split_counted_payment_preserves_every_wallet : forall demand draw source,
  counted_payment_first demand draw source + counted_payment_rest demand draw source = draw source.
Proof.
  intros. unfold counted_payment_rest, counted_payment_first.
  pose proof (funding_row_take_bounded demand draw source). lia.
Qed.

Theorem split_counted_payment_is_exact : forall sources allowed first second draw,
  counted_payment_valid sources allowed (first + second) draw ->
  counted_payment_valid sources allowed first (counted_payment_first first draw) /\
  counted_payment_valid sources allowed second (counted_payment_rest first draw).
Proof.
  intros sources allowed first second draw [total permissions].
  assert (taken : funding_sum sources (counted_payment_first first draw) = first).
  { unfold counted_payment_first. rewrite funding_row_take_total, total. apply Nat.min_l. lia. }
  assert (remaining : funding_sum sources (counted_payment_rest first draw) = second).
  { assert (equal : funding_sum sources (fun source =>
        counted_payment_first first draw source + counted_payment_rest first draw source) =
        funding_sum sources draw).
    { apply funding_sum_ext. intros. apply split_counted_payment_preserves_every_wallet. }
    rewrite funding_sum_add, taken, total in equal. lia. }
  split; split; try assumption; intros source inside excluded;
    specialize (permissions source inside excluded).
  - unfold counted_payment_first, funding_row_take. rewrite permissions. reflexivity.
  - unfold counted_payment_rest. rewrite permissions. reflexivity.
Qed.

Theorem join_counted_payments_is_exact : forall sources allowed first second left right,
  counted_payment_valid sources allowed first left ->
  counted_payment_valid sources allowed second right ->
  counted_payment_valid sources allowed (first + second) (fun source => left source + right source).
Proof.
  intros sources allowed first second left right [left_total left_permissions]
    [right_total right_permissions]. split.
  - rewrite funding_sum_add, left_total, right_total. reflexivity.
  - intros source inside excluded.
    rewrite (left_permissions source inside excluded), (right_permissions source inside excluded).
    reflexivity.
Qed.

Theorem counted_split_keeps_capacity_bounds : forall demand draw other capacity,
  (forall source, draw source + other source <= capacity source) ->
  forall source,
    counted_payment_first demand draw source + counted_payment_rest demand draw source +
    other source <= capacity source.
Proof. intros. rewrite split_counted_payment_preserves_every_wallet. auto. Qed.

Theorem counted_split_keeps_wallet_projection : forall (A : Type) sources demand draw
    (projection : list nat -> A),
  projection (map (fun source => counted_payment_first demand draw source +
    counted_payment_rest demand draw source) (seq 0 sources)) =
  projection (map draw (seq 0 sources)).
Proof.
  intros. f_equal. apply map_ext. intros source.
  apply split_counted_payment_preserves_every_wallet.
Qed.

Print Assumptions split_counted_payment_preserves_every_wallet.
Print Assumptions split_counted_payment_is_exact.
Print Assumptions join_counted_payments_is_exact.
Print Assumptions counted_split_keeps_capacity_bounds.
Print Assumptions counted_split_keeps_wallet_projection.

Fixpoint counted_lex_compare (left right : list nat) : comparison :=
  match left, right with
  | nil, nil => Eq
  | nil, _ => Lt
  | _, nil => Gt
  | l :: ls, r :: rs =>
      match Nat.compare l r with Eq => counted_lex_compare ls rs | order => order end
  end.

Theorem counted_lex_common_prefix_cancels : forall prefix left right,
  counted_lex_compare (prefix ++ left) (prefix ++ right) = counted_lex_compare left right.
Proof.
  induction prefix; intros; simpl; [reflexivity|]. rewrite Nat.compare_refl. apply IHprefix.
Qed.

Theorem counted_run_step_preserves_expanded_order : forall key first second left right,
  counted_lex_compare (repeat key first ++ left) (repeat key second ++ right) =
  counted_lex_compare (repeat key (first - Nat.min first second) ++ left)
    (repeat key (second - Nat.min first second) ++ right).
Proof.
  intros key first second left right.
  assert (first_split : first = Nat.min first second + (first - Nat.min first second))
    by (pose proof (Nat.le_min_l first second); lia).
  assert (second_split : second = Nat.min first second + (second - Nat.min first second))
    by (pose proof (Nat.le_min_r first second); lia).
  assert (lf : repeat key first = repeat key (Nat.min first second) ++
      repeat key (first - Nat.min first second)).
  { rewrite <- repeat_app. now rewrite <- first_split. }
  assert (rf : repeat key second = repeat key (Nat.min first second) ++
      repeat key (second - Nat.min first second)).
  { rewrite <- repeat_app. now rewrite <- second_split. }
  rewrite lf at 1. rewrite rf at 1.
  rewrite <- !app_assoc. apply counted_lex_common_prefix_cancels.
Qed.

Theorem counted_run_step_exhausts_an_input : forall first second,
  first - Nat.min first second = 0 \/ second - Nat.min first second = 0.
Proof.
  intros. destruct (Nat.le_ge_cases first second).
  - rewrite Nat.min_l by assumption. left. lia.
  - rewrite Nat.min_r by assumption. right. lia.
Qed.

Theorem counted_run_different_heads_decide_order : forall first second left_count right_count left right,
  first <> second -> 0 < left_count -> 0 < right_count ->
  counted_lex_compare (repeat first left_count ++ left) (repeat second right_count ++ right) =
  Nat.compare first second.
Proof.
  intros first second [|left_count] [|right_count] left right different lp rp; try lia.
  simpl. destruct (Nat.compare first second) eqn:order; try reflexivity.
  apply Nat.compare_eq in order. contradiction.
Qed.

Print Assumptions counted_run_step_preserves_expanded_order.
Print Assumptions counted_run_step_exhausts_an_input.
Print Assumptions counted_run_different_heads_decide_order.
