From Stdlib Require Import Arith.PeanoNat Lia.

Definition rounded_input (numerator denominator output : nat) : nat :=
  let product := numerator * output in
  product / denominator + if Nat.eqb (product mod denominator) 0 then 0 else 1.

Definition quoted_input (numerator denominator fee output : nat) : nat :=
  if Nat.eqb output 0 then 0 else rounded_input numerator denominator output + fee.

Theorem quoted_zero_has_no_fee : forall numerator denominator fee,
  quoted_input numerator denominator fee 0 = 0.
Proof. intros. reflexivity. Qed.

Theorem rounded_input_is_least_covering_integer : forall numerator denominator output,
  denominator > 0 ->
  numerator * output <= rounded_input numerator denominator output * denominator /\
  forall smaller,
    smaller < rounded_input numerator denominator output ->
    smaller * denominator < numerator * output.
Proof.
  intros numerator denominator output positive.
  unfold rounded_input.
  set (product := numerator * output).
  assert (nonzero_denominator : denominator <> 0) by lia.
  pose proof (Nat.div_mod product denominator nonzero_denominator) as division.
  pose proof (Nat.mod_upper_bound product denominator nonzero_denominator) as remainder.
  destruct (Nat.eqb_spec (product mod denominator) 0) as [exact|nonzero].
  - rewrite exact in division. simpl. split; intros; nia.
  - simpl. split; intros; nia.
Qed.

Theorem quoted_positive_has_exact_fee : forall numerator denominator fee output,
  output > 0 ->
  quoted_input numerator denominator fee output =
    rounded_input numerator denominator output + fee.
Proof.
  intros numerator denominator fee output positive.
  unfold quoted_input.
  destruct output as [|output]; [lia|reflexivity].
Qed.

Theorem rounded_input_monotone : forall numerator denominator smaller larger,
  denominator > 0 ->
  smaller <= larger ->
  rounded_input numerator denominator smaller <=
    rounded_input numerator denominator larger.
Proof.
  intros numerator denominator smaller larger positive ordered.
  destruct (Nat.le_gt_cases
    (rounded_input numerator denominator smaller)
    (rounded_input numerator denominator larger)) as [bounded|reversed].
  - exact bounded.
  - pose proof (rounded_input_is_least_covering_integer
      numerator denominator smaller positive) as [_ minimal].
    pose proof (rounded_input_is_least_covering_integer
      numerator denominator larger positive) as [covered _].
    specialize (minimal (rounded_input numerator denominator larger) reversed).
    assert (numerator * smaller <= numerator * larger) by nia.
    lia.
Qed.

Theorem quoted_input_within_authorized_cap : forall numerator denominator fee output cap,
  denominator > 0 ->
  output > 0 ->
  rounded_input numerator denominator output + fee <= cap ->
  quoted_input numerator denominator fee output <= cap.
Proof.
  intros numerator denominator fee output cap _ positive bounded.
  rewrite quoted_positive_has_exact_fee by assumption.
  exact bounded.
Qed.

Print Assumptions quoted_zero_has_no_fee.
Print Assumptions rounded_input_is_least_covering_integer.
Print Assumptions rounded_input_monotone.
Print Assumptions quoted_input_within_authorized_cap.
