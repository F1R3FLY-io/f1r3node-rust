From Stdlib Require Import List Arith Bool Lia.
From StackSafePDA Require Import EPM1.

Import ListNotations.
Set Implicit Arguments.

Module SubstitutionRegionFraming.

Lemma decoded_varint_reencodes_with_fuel : forall input value rest fuel,
  decode_varint input = Some (value, rest) ->
  value < fuel ->
  input = encode_varint_fuel fuel value ++ rest.
Proof.
  induction input as [|head tail IH]; intros value rest fuel Parsed Bound.
  - rewrite decode_varint_nil in Parsed; discriminate.
  - rewrite decode_varint_cons in Parsed.
    destruct fuel as [|fuel]; [lia|].
    destruct (head <? 128) eqn:Small.
    + inversion Parsed; subst; cbn [encode_varint_fuel].
      now rewrite Small.
    + destruct (head <? 256) eqn:Byte; try discriminate.
      destruct (decode_varint tail) as [[quotient suffix]|] eqn:Child; try discriminate.
      destruct (quotient =? 0) eqn:Nonzero; try discriminate.
      inversion Parsed; subst value rest; clear Parsed.
      apply Nat.ltb_ge in Small; apply Nat.ltb_lt in Byte.
      apply Nat.eqb_neq in Nonzero.
      assert (QBound : quotient < fuel) by lia.
      assert (Large : (head - 128 + 128 * quotient <? 128) = false)
        by (apply Nat.ltb_ge; lia).
      assert (Div : (head - 128 + 128 * quotient) / 128 = quotient).
      { symmetry; apply Nat.div_unique with (r := head - 128); lia. }
      assert (Mod : (head - 128 + 128 * quotient) mod 128 = head - 128).
      { symmetry; apply Nat.mod_unique with (q := quotient); lia. }
      change (head :: tail =
        (if head - 128 + 128 * quotient <? 128
         then [head - 128 + 128 * quotient]
         else (128 + (head - 128 + 128 * quotient) mod 128) ::
           encode_varint_fuel fuel ((head - 128 + 128 * quotient) / 128)) ++ suffix).
      rewrite Large, Div, Mod.
      replace (128 + (head - 128)) with head by lia.
      rewrite (IH _ _ fuel eq_refl QBound); reflexivity.
Qed.

Theorem successful_varint_reencodes_consumed_prefix : forall input value rest,
  decode_varint input = Some (value, rest) ->
  input = encode_varint value ++ rest.
Proof.
  intros; unfold encode_varint.
  eapply decoded_varint_reencodes_with_fuel; eauto; lia.
Qed.

Theorem successful_frame_reencodes_consumed_prefix : forall input payload rest,
  decode_frame input = Some (payload, rest) ->
  input = encode_frame payload ++ rest.
Proof.
  intros input payload rest Parsed; unfold decode_frame in Parsed.
  destruct (decode_varint input) as [[size suffix]|] eqn:Header; try discriminate.
  destruct (size <=? length suffix) eqn:Enough; try discriminate.
  inversion Parsed; subst payload rest.
  apply Nat.leb_le in Enough.
  rewrite (@successful_varint_reencodes_consumed_prefix input size suffix Header).
  unfold encode_frame.
  rewrite length_firstn, Nat.min_l by exact Enough.
  rewrite <- app_assoc, firstn_skipn; reflexivity.
Qed.

Theorem successful_frames_reencode_consumed_prefix : forall count input payloads rest,
  decode_frames count input = Some (payloads, rest) ->
  input = encode_frames payloads ++ rest.
Proof.
  induction count as [|count IH]; intros input payloads rest Parsed.
  - cbn [decode_frames] in Parsed; inversion Parsed; reflexivity.
  - cbn [decode_frames] in Parsed.
    destruct (decode_frame input) as [[body suffix]|] eqn:Head; try discriminate.
    destruct (decode_frames count suffix) as [[bodies tail]|] eqn:Tail; try discriminate.
    inversion Parsed; subst payloads rest; clear Parsed.
    rewrite (@successful_frame_reencodes_consumed_prefix input body suffix Head), (IH _ _ _ Tail).
    cbn [encode_frames]; apply app_assoc.
Qed.

Theorem epm_value_frames_reconstruct_complete_suffix : forall prefix input count bodies,
  decode_frames count input = Some (bodies, []) ->
  prefix ++ input = prefix ++ encode_frames bodies.
Proof.
  intros; rewrite (@successful_frames_reencode_consumed_prefix count input bodies [] H), app_nil_r.
  reflexivity.
Qed.

Definition interval (input : bytes) (lo hi : nat) : bytes :=
  firstn (hi - lo) (skipn lo input).

Inductive adjacent_fields (input : bytes) : nat -> list bytes -> nat -> Prop :=
| AdjacentEmpty : forall position, adjacent_fields input position [] position
| AdjacentNext : forall position next finish fields,
    position < next -> next <= length input ->
    adjacent_fields input next fields finish ->
    adjacent_fields input position (interval input position next :: fields) finish.

Lemma adjacent_fields_bounds : forall input start fields finish,
  adjacent_fields input start fields finish ->
  start <= finish.
Proof. intros input start fields finish H; induction H; lia. Qed.

Lemma interval_then_suffix : forall input lo hi,
  lo <= hi ->
  interval input lo hi ++ skipn hi input = skipn lo input.
Proof.
  intros; unfold interval.
  replace (skipn hi input) with (skipn (hi - lo) (skipn lo input)).
  - apply firstn_skipn.
  - rewrite skipn_skipn; f_equal; lia.
Qed.

Theorem successful_fields_partition_consumed_suffix : forall input start fields finish,
  adjacent_fields input start fields finish ->
  concat fields ++ skipn finish input = skipn start input.
Proof.
  intros input start fields finish H; induction H.
  - reflexivity.
  - cbn [concat]; rewrite <- app_assoc, IHadjacent_fields.
    apply interval_then_suffix; lia.
Qed.

Theorem eof_completes_source_partition : forall input fields,
  adjacent_fields input 0 fields (length input) ->
  concat fields = input.
Proof.
  intros input fields H.
  pose proof (@successful_fields_partition_consumed_suffix input 0 fields (length input) H) as E.
  rewrite skipn_all, app_nil_r in E; exact E.
Qed.

Theorem successful_fields_make_bounded_progress : forall input start fields finish,
  adjacent_fields input start fields finish ->
  length fields <= finish - start.
Proof.
  intros input start fields finish H; induction H; cbn [length]; [lia|].
  pose proof (adjacent_fields_bounds H1); lia.
Qed.

Example framed_empty_values_are_not_omitted :
  decode_frames 3 [0; 1; 42; 0] = Some ([[]; [42]; []], []).
Proof. reflexivity. Qed.

Example wrong_same_length_value_is_not_reconstruction :
  encode_frames [[41]] <> [1; 42].
Proof. discriminate. Qed.

Example truncated_frame_is_not_a_partition :
  decode_frame [2; 42] = None.
Proof. reflexivity. Qed.

Print Assumptions successful_varint_reencodes_consumed_prefix.
Print Assumptions successful_frame_reencodes_consumed_prefix.
Print Assumptions successful_frames_reencode_consumed_prefix.
Print Assumptions epm_value_frames_reconstruct_complete_suffix.
Print Assumptions adjacent_fields_bounds.
Print Assumptions interval_then_suffix.
Print Assumptions successful_fields_partition_consumed_suffix.
Print Assumptions eof_completes_source_partition.
Print Assumptions successful_fields_make_bounded_progress.
Print Assumptions framed_empty_values_are_not_omitted.
Print Assumptions wrong_same_length_value_is_not_reconstruction.
Print Assumptions truncated_frame_is_not_a_partition.


Fixpoint scan_groups (remaining : nat) (first : bool) (input : bytes)
  : option (nat * bytes) :=
  match remaining, input with
  | S budget, current :: rest =>
      if (if budget =? 0 then current <=? 1 else current <? 256) then
        if current <? 128 then
          if first || negb (current =? 0) then Some (current, rest) else None
        else
          match scan_groups budget false rest with
          | Some (quotient, suffix) =>
              Some (current - 128 + 128 * quotient, suffix)
          | None => None
          end
      else None
  | _, _ => None
  end.

Lemma successful_groups_refine_decoder : forall budget first input value suffix,
  scan_groups budget first input = Some (value, suffix) ->
  decode_varint input = Some (value, suffix) /\
  (first = false -> 0 < value).
Proof.
  induction budget as [|budget IH]; intros first input value suffix Parsed.
  - discriminate.
  - destruct input as [|current rest]; [discriminate|].
    cbn [scan_groups] in Parsed.
    destruct (if budget =? 0 then current <=? 1 else current <? 256) eqn:Physical;
      try discriminate.
    assert (Byte : (current <? 256) = true).
    { destruct (budget =? 0); [apply Nat.leb_le in Physical;
        apply Nat.ltb_lt; lia | exact Physical]. }
    destruct (current <? 128) eqn:Small.
    + destruct (first || negb (current =? 0)) eqn:Minimal; try discriminate.
      inversion Parsed; subst; split.
      * rewrite decode_varint_cons, Small; reflexivity.
      * intros ->; cbn in Minimal; apply Bool.negb_true_iff in Minimal.
        apply Nat.eqb_neq in Minimal; lia.
    + destruct (scan_groups budget false rest) as [[quotient tail]|] eqn:Child;
        try discriminate.
      inversion Parsed; subst; clear Parsed.
      destruct (IH _ _ _ _ Child) as [Decoded Positive].
      specialize (Positive eq_refl).
      split.
      * rewrite decode_varint_cons, Small, Byte, Decoded.
        assert (Nonzero : (quotient =? 0) = false) by (apply Nat.eqb_neq; lia).
        now rewrite Nonzero.
      * intro; lia.
Qed.

Theorem successful_bounded_varint_reencodes_consumed_prefix :
  forall input value suffix,
  scan_groups 10 true input = Some (value, suffix) ->
  input = encode_varint value ++ suffix.
Proof.
  intros input value suffix Parsed.
  destruct (@successful_groups_refine_decoder 10 true input value suffix Parsed) as [Decoded _].
  exact (@successful_varint_reencodes_consumed_prefix input value suffix Decoded).
Qed.

Theorem successful_groups_consume_at_most_budget : forall budget first input value suffix,
  scan_groups budget first input = Some (value, suffix) ->
  exists consumed, input = consumed ++ suffix /\
    0 < length consumed /\ length consumed <= budget.
Proof.
  induction budget as [|budget IH]; intros first input value suffix Parsed.
  - discriminate.
  - destruct input as [|current rest]; [discriminate|].
    cbn [scan_groups] in Parsed.
    destruct (if budget =? 0 then current <=? 1 else current <? 256); try discriminate.
    destruct (current <? 128).
    + destruct (first || negb (current =? 0)); try discriminate.
      inversion Parsed; subst; exists [value]; cbn; auto with arith.
    + destruct (scan_groups budget false rest) as [[quotient tail]|] eqn:Child;
        try discriminate.
      inversion Parsed; subst; clear Parsed.
      destruct (IH _ _ _ _ Child) as [consumed [Source [Positive Bounded]]].
      exists (current :: consumed); cbn; split; [now rewrite Source|lia].
Qed.

Example bounded_scanner_accepts_multibyte_value :
  scan_groups 10 true [150; 1; 42] = Some (150, [42]).
Proof. reflexivity. Qed.

Example bounded_scanner_rejects_redundant_terminal :
  scan_groups 10 true [128; 0] = None.
Proof. reflexivity. Qed.

Example bounded_scanner_rejects_tenth_byte_overflow :
  scan_groups 10 true (repeat 128 9 ++ [2]) = None.
Proof. reflexivity. Qed.

Example bounded_scanner_rejects_eleventh_group :
  scan_groups 10 true (repeat 128 10 ++ [1]) = None.
Proof. reflexivity. Qed.

Print Assumptions successful_groups_refine_decoder.
Print Assumptions successful_bounded_varint_reencodes_consumed_prefix.
Print Assumptions successful_groups_consume_at_most_budget.
Print Assumptions bounded_scanner_accepts_multibyte_value.
Print Assumptions bounded_scanner_rejects_redundant_terminal.
Print Assumptions bounded_scanner_rejects_tenth_byte_overflow.
Print Assumptions bounded_scanner_rejects_eleventh_group.

End SubstitutionRegionFraming.
