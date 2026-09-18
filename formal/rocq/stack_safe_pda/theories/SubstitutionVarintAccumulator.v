From Stdlib Require Import List Arith Bool Lia NArith.
From StackSafePDA Require Import EPM1 SubstitutionRegionFraming.

Import ListNotations.
Import SubstitutionRegionFraming.SubstitutionRegionFraming.
Set Implicit Arguments.

Module SubstitutionVarintAccumulator.

Definition update (acc shift : N) (byte : nat) : N :=
  N.lor acc (N.shiftl (N.land (N.of_nat byte) 127) shift).

Theorem low_accumulator_and_shifted_digit_are_disjoint : forall acc digit shift,
  (acc < 2 ^ shift)%N ->
  N.land acc (N.shiftl digit shift) = 0%N.
Proof.
  intros acc digit shift Bound.
  apply N.bits_inj; intro bit.
  rewrite N.land_spec, N.bits_0.
  destruct (N.lt_ge_cases bit shift) as [Low|High].
  - rewrite N.shiftl_mul_pow2, N.mul_pow2_bits_low by exact Low.
    apply Bool.andb_false_r.
  - rewrite <- (N.mod_small acc (2 ^ shift) Bound) at 1.
    rewrite N.mod_pow2_bits_high by exact High; reflexivity.
Qed.

Theorem masked_shift_or_equals_radix_accumulation : forall acc shift byte,
  (acc < 2 ^ shift)%N ->
  update acc shift byte = (acc + (N.of_nat byte mod 128) * 2 ^ shift)%N.
Proof.
  intros acc shift byte Bound; unfold update.
  assert (Mask : N.land (N.of_nat byte) 127 = (N.of_nat byte mod 128)%N).
  { replace 127%N with (N.ones 7) by reflexivity.
    rewrite N.land_ones; reflexivity. }
  rewrite Mask.
  pose proof (@low_accumulator_and_shifted_digit_are_disjoint acc
    (N.of_nat byte mod 128) shift Bound) as Disjoint.
  rewrite <- N.lxor_lor by exact Disjoint.
  rewrite <- N.add_nocarry_lxor by exact Disjoint.
  now rewrite N.shiftl_mul_pow2.
Qed.

Theorem forward_step_preserves_accumulator_invariant : forall acc shift byte,
  (acc < 2 ^ shift)%N ->
  (update acc shift byte < 2 ^ (shift + 7))%N.
Proof.
  intros acc shift byte Bound.
  rewrite masked_shift_or_equals_radix_accumulation by exact Bound.
  pose proof (N.mod_lt (N.of_nat byte) 128 ltac:(discriminate)) as Digit.
  pose proof (N.pow_nonzero 2 shift ltac:(discriminate)) as Positive.
  rewrite N.pow_add_r.
  change (acc + (N.of_nat byte mod 128) * 2 ^ shift < 2 ^ shift * 128)%N.
  nia.
Qed.

Fixpoint forward_scan (remaining : nat) (first : bool)
  (shift acc : N) (input : bytes) : option (N * bytes) :=
  match remaining, input with
  | S budget, current :: rest =>
      if (if budget =? 0 then current <=? 1 else current <? 256) then
        let next := update acc shift current in
        if current <? 128 then
          if first || negb (current =? 0) then Some (next, rest) else None
        else forward_scan budget false (shift + 7)%N next rest
      else None
  | _, _ => None
  end.

Theorem forward_scanner_refines_group_decomposition :
  forall budget first input shift acc,
  (acc < 2 ^ shift)%N ->
  forward_scan budget first shift acc input =
    option_map (fun parsed => ((acc + N.of_nat (fst parsed) * 2 ^ shift)%N, snd parsed))
      (scan_groups budget first input).
Proof.
  induction budget as [|budget IH]; intros first input shift acc Bound.
  - reflexivity.
  - destruct input as [|current rest]; [reflexivity|].
    cbn [forward_scan scan_groups].
    destruct (if budget =? 0 then current <=? 1 else current <? 256) eqn:Physical;
      [|reflexivity].
    assert (Byte : current < 256).
    { destruct (budget =? 0); [apply Nat.leb_le in Physical|apply Nat.ltb_lt in Physical]; lia. }
    destruct (current <? 128) eqn:Small.
    + destruct (first || negb (current =? 0)); [|reflexivity].
      cbn [option_map fst snd].
      rewrite masked_shift_or_equals_radix_accumulation by exact Bound.
      apply Nat.ltb_lt in Small.
      rewrite N.mod_small by lia; reflexivity.
    + rewrite IH by (apply forward_step_preserves_accumulator_invariant; exact Bound).
      destruct (scan_groups budget false rest) as [[quotient suffix]|]; [|reflexivity].
      cbn [option_map fst snd].
      rewrite masked_shift_or_equals_radix_accumulation by exact Bound.
      apply Nat.ltb_ge in Small.
      assert (Digit : (N.of_nat current mod 128)%N = N.of_nat (current - 128)).
      { symmetry; apply N.mod_unique with (q := 1%N); lia. }
      rewrite Digit, Nat2N.inj_add, Nat2N.inj_mul, N.pow_add_r.
      change (Some ((acc + N.of_nat (current - 128) * 2 ^ shift +
        N.of_nat quotient * (2 ^ shift * 128))%N, suffix) =
        Some ((acc + (N.of_nat (current - 128) + 128 * N.of_nat quotient) * 2 ^ shift)%N, suffix)).
      f_equal; f_equal; nia.
Qed.

Theorem whole_forward_scanner_equals_group_scanner : forall input,
  forward_scan 10 true 0 0 input =
  option_map (fun parsed => (N.of_nat (fst parsed), snd parsed)) (scan_groups 10 true input).
Proof.
  intro input; rewrite forward_scanner_refines_group_decomposition by reflexivity.
  destruct (scan_groups 10 true input) as [[value rest]|]; cbn [option_map fst snd].
  - rewrite N.pow_0_r, N.mul_1_r, N.add_0_l; reflexivity.
  - reflexivity.
Qed.

Theorem successful_forward_scan_reencodes_consumed_bytes : forall input value suffix,
  forward_scan 10 true 0 0 input = Some (value, suffix) ->
  input = encode_varint (N.to_nat value) ++ suffix.
Proof.
  intros input value suffix H.
  rewrite whole_forward_scanner_equals_group_scanner in H.
  destruct (scan_groups 10 true input) as [[parsed rest]|] eqn:Scan; try discriminate.
  cbn [option_map fst snd] in H; inversion H; subst value suffix.
  rewrite Nat2N.id.
  exact (@successful_bounded_varint_reencodes_consumed_prefix input parsed rest Scan).
Qed.

Theorem all_native_shifts_are_below_word_width : forall index,
  (index <= 9 -> 7 * index <= 63)%N.
Proof. intros; lia. Qed.

Theorem tenth_group_guard_preserves_u64_bound : forall index acc byte,
  (index <= 9)%N ->
  (acc < 2 ^ (7 * index))%N ->
  (byte < 256)%nat ->
  (index = 9%N -> byte <= 1)%nat ->
  (update acc (7 * index) byte < 2 ^ 64)%N.
Proof.
  intros index acc byte Index Bound Byte Last.
  destruct (N.eq_dec index 9) as [->|Earlier].
  - rewrite masked_shift_or_equals_radix_accumulation by exact Bound.
    specialize (Last eq_refl).
    rewrite N.mod_small by lia.
    change (acc < 9223372036854775808)%N in Bound.
    change (acc + N.of_nat byte * 9223372036854775808 < 18446744073709551616)%N.
    nia.
  - pose proof (@forward_step_preserves_accumulator_invariant acc (7 * index) byte Bound) as Next.
    assert (W : (2 ^ (7 * index + 7) <= 2 ^ 64)%N).
    { apply N.pow_le_mono_r; lia. }
    lia.
Qed.

Definition canonical_width_at (value index : N) : Prop :=
  (value < 2 ^ (7 * (index + 1)))%N /\
  (index = 0%N \/ (2 ^ (7 * index) <= value)%N).

Theorem terminal_width_equals_nonzero_high_digit : forall index acc digit,
  (acc < 2 ^ (7 * index))%N ->
  (digit < 128)%N ->
  (canonical_width_at (acc + digit * 2 ^ (7 * index)) index <->
    index = 0%N \/ digit <> 0%N).
Proof.
  intros index acc digit Bound Digit.
  pose proof (N.pow_nonzero 2 (7 * index) ltac:(discriminate)) as Positive.
  unfold canonical_width_at.
  replace (7 * (index + 1))%N with (7 * index + 7)%N by nia.
  rewrite N.pow_add_r.
  change ((acc + digit * 2 ^ (7 * index) < 2 ^ (7 * index) * 128)%N /\
    (index = 0%N \/ (2 ^ (7 * index) <= acc + digit * 2 ^ (7 * index))%N) <->
    index = 0%N \/ digit <> 0%N).
  split.
  - intros [_ [Zero|Lower]]; [now left|right; nia].
  - intros [Zero|Nonzero]; split; intuition nia.
Qed.

Example forward_scanner_accepts_full_u64 :
  forward_scan 10 true 0 0 (repeat 255 9 ++ [1; 42]) =
    Some (18446744073709551615%N, [42]).
Proof. vm_compute; reflexivity. Qed.

Example forward_scanner_refuses_tenth_byte_two :
  forward_scan 10 true 0 0 (repeat 255 9 ++ [2]) = None.
Proof. vm_compute; reflexivity. Qed.

Example forward_scanner_refuses_redundant_terminal :
  forward_scan 10 true 0 0 [128; 0] = None.
Proof. reflexivity. Qed.

Print Assumptions low_accumulator_and_shifted_digit_are_disjoint.
Print Assumptions masked_shift_or_equals_radix_accumulation.
Print Assumptions forward_step_preserves_accumulator_invariant.
Print Assumptions forward_scanner_refines_group_decomposition.
Print Assumptions whole_forward_scanner_equals_group_scanner.
Print Assumptions successful_forward_scan_reencodes_consumed_bytes.
Print Assumptions all_native_shifts_are_below_word_width.
Print Assumptions tenth_group_guard_preserves_u64_bound.
Print Assumptions terminal_width_equals_nonzero_high_digit.
Print Assumptions forward_scanner_accepts_full_u64.
Print Assumptions forward_scanner_refuses_tenth_byte_two.
Print Assumptions forward_scanner_refuses_redundant_terminal.


Definition prost_width (value : N) : N :=
  ((N.log2 (N.lor value 1) * 9 + 73) / 64)%N.

Lemma width_formula_for_all_u64_logarithms : forall bits,
  bits < 64 ->
  ((N.of_nat bits * 9 + 73) / 64 = 1 + N.of_nat bits / 7)%N.
Proof.
  intros bits Bound.
  do 64 (destruct bits as [|bits]; [reflexivity|]).
  lia.
Qed.

Theorem prost_width_is_logarithmic_radix_width : forall value,
  (value < 2 ^ 64)%N ->
  prost_width value = (1 + N.log2 value / 7)%N.
Proof.
  intros value Bound; unfold prost_width.
  rewrite N.log2_lor.
  change ((N.max (N.log2 value) 0 * 9 + 73) / 64 = 1 + N.log2 value / 7)%N.
  rewrite N.max_0_r.
  assert (Log : (N.log2 value < 64)%N).
  { destruct (N.eq_dec value 0) as [->|Positive]; [reflexivity|].
    apply (proj1 (N.log2_lt_pow2 value 64 ltac:(lia))); exact Bound. }
  replace (N.log2 value) with (N.of_nat (N.to_nat (N.log2 value))) by
    (rewrite N2Nat.id; reflexivity).
  apply width_formula_for_all_u64_logarithms; lia.
Qed.

Lemma radix_quotient_interval : forall bits index,
  (bits / 7 = index <-> 7 * index <= bits < 7 * (index + 1))%N.
Proof.
  intros bits index.
  pose proof (N.div_mod bits 7 ltac:(discriminate)) as Parts.
  pose proof (N.mod_lt bits 7 ltac:(discriminate)) as Mod.
  split.
  - intro Equal; rewrite Equal in Parts.
    split.
    + rewrite Parts at 1; apply N.le_add_r.
    + replace (7 * (index + 1))%N with (7 * index + 7)%N by ring.
      rewrite Parts at 1; apply N.add_lt_mono_l; exact Mod.
  - intros [Lower Upper]; symmetry.
    apply N.div_unique with (r := (bits - 7 * index)%N); nia.
Qed.

Theorem prost_width_characterizes_canonical_range : forall value index,
  (value < 2 ^ 64)%N ->
  (prost_width value = (index + 1)%N <-> canonical_width_at value index).
Proof.
  intros value index Bound.
  rewrite prost_width_is_logarithmic_radix_width by exact Bound.
  destruct (N.eq_dec value 0) as [->|Nonzero].
  - unfold canonical_width_at.
    change ((1 + 0 / 7 = index + 1)%N <->
      (0 < 2 ^ (7 * (index + 1)))%N /\
      (index = 0%N \/ (2 ^ (7 * index) <= 0)%N)).
    split.
    + intro Equal; change (1 = index + 1)%N in Equal.
      assert (Zero : index = 0%N) by lia.
      subst index; split; [reflexivity|now left].
    + intros [_ [Zero|Impossible]].
      * now subst index.
      * pose proof (N.pow_nonzero 2 (7 * index) ltac:(discriminate)); exfalso; lia.
  - assert (Positive : (0 < value)%N) by lia.
    unfold canonical_width_at.
    rewrite N.log2_lt_pow2 by exact Positive.
    rewrite N.log2_le_pow2 by exact Positive.
    pose proof (@radix_quotient_interval (N.log2 value) index) as Range.
    split.
    + intro Equal.
      assert (Quotient : (N.log2 value / 7)%N = index) by lia.
      apply Range in Quotient; split; [lia|right; lia].
    + intros [Upper [Zero|Lower]].
      * subst index; assert (Q : (N.log2 value / 7)%N = 0%N).
        { apply Range; split; lia. }
        now rewrite Q.
      * assert (Q : (N.log2 value / 7)%N = index).
        { apply Range; split; lia. }
        rewrite Q; lia.
Qed.

Theorem terminal_prost_width_equals_nonzero_high_digit : forall index acc byte,
  (index <= 9)%N ->
  (acc < 2 ^ (7 * index))%N ->
  byte < 128 ->
  (index = 9%N -> byte <= 1) ->
  (prost_width (update acc (7 * index) byte) = (index + 1)%N <->
    index = 0%N \/ byte <> 0).
Proof.
  intros index acc byte Index Bound Byte Last.
  rewrite prost_width_characterizes_canonical_range.
  - rewrite masked_shift_or_equals_radix_accumulation by exact Bound.
    rewrite N.mod_small by lia.
    rewrite terminal_width_equals_nonzero_high_digit by lia.
    intuition lia.
  - apply tenth_group_guard_preserves_u64_bound; auto; lia.
Qed.

Fixpoint epm_forward_scan (remaining : nat) (index acc : N) (input : bytes)
  : option (N * bytes) :=
  match remaining, input with
  | S budget, current :: rest =>
      if (if budget =? 0 then current <=? 1 else current <? 256) then
        let next := update acc (7 * index) current in
        if current <? 128 then
          if N.eqb (prost_width next) (index + 1) then Some (next, rest) else None
        else epm_forward_scan budget (index + 1)%N next rest
      else None
  | _, _ => None
  end.

Theorem width_checked_loop_equals_terminal_checked_loop :
  forall budget index acc input,
  (index + N.of_nat budget = 10)%N ->
  (acc < 2 ^ (7 * index))%N ->
  epm_forward_scan budget index acc input =
    forward_scan budget (N.eqb index 0) (7 * index) acc input.
Proof.
  induction budget as [|budget IH]; intros index acc input Position Bound.
  - reflexivity.
  - destruct input as [|current rest]; [reflexivity|].
    cbn [epm_forward_scan forward_scan].
    destruct (if budget =? 0 then current <=? 1 else current <? 256) eqn:Physical;
      [|reflexivity].
    assert (Index : (index <= 9)%N) by lia.
    assert (Last : index = 9%N -> current <= 1).
    { intro AtEnd; assert (B : budget = 0) by lia.
      subst budget; cbn in Physical; apply Nat.leb_le; exact Physical. }
    destruct (current <? 128) eqn:Small.
    + apply Nat.ltb_lt in Small.
      assert (Guard : N.eqb (prost_width (update acc (7 * index) current)) (index + 1) =
        (N.eqb index 0 || negb (current =? 0))).
      { apply Bool.eq_true_iff_eq.
        rewrite N.eqb_eq, Bool.orb_true_iff, N.eqb_eq, Bool.negb_true_iff, Nat.eqb_neq.
        apply terminal_prost_width_equals_nonzero_high_digit; assumption. }
      now rewrite Guard.
    + rewrite IH.
      * assert (Nonzero : N.eqb (index + 1) 0 = false) by (apply N.eqb_neq; lia).
        rewrite Nonzero.
        replace (7 * (index + 1))%N with (7 * index + 7)%N by nia.
        reflexivity.
      * lia.
      * replace (7 * (index + 1))%N with (7 * index + 7)%N by nia.
        apply forward_step_preserves_accumulator_invariant; exact Bound.
Qed.

Theorem both_scanners_return_same_value_and_suffix : forall input,
  epm_forward_scan 10 0 0 input = forward_scan 10 true 0 0 input.
Proof.
  intro input; rewrite width_checked_loop_equals_terminal_checked_loop;
    try reflexivity.
Qed.

Theorem successful_epm_forward_scan_reencodes_consumed_bytes : forall input value suffix,
  epm_forward_scan 10 0 0 input = Some (value, suffix) ->
  input = encode_varint (N.to_nat value) ++ suffix.
Proof.
  intros input value suffix H.
  rewrite both_scanners_return_same_value_and_suffix in H.
  eapply successful_forward_scan_reencodes_consumed_bytes; exact H.
Qed.

Print Assumptions width_formula_for_all_u64_logarithms.
Print Assumptions prost_width_is_logarithmic_radix_width.
Print Assumptions radix_quotient_interval.
Print Assumptions prost_width_characterizes_canonical_range.
Print Assumptions terminal_prost_width_equals_nonzero_high_digit.
Print Assumptions width_checked_loop_equals_terminal_checked_loop.
Print Assumptions both_scanners_return_same_value_and_suffix.
Print Assumptions successful_epm_forward_scan_reencodes_consumed_bytes.

End SubstitutionVarintAccumulator.
