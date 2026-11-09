From Stdlib Require Import List NArith ZArith Lia.
From StackSafePDA Require Import RPB3SourceScalarReduction
  RPB3IndependentOracleCore RPB3IndependentOracleProcess
  RPB3SemanticReducer.

Import ListNotations.

Module Scalar := RPB3SourceScalarReduction.
Module Core := RPB3IndependentOracleCore.
Module Process := RPB3IndependentOracleProcess.
Module Target := RPB3SemanticReducer.

(** Rust's [u64 as i64] is the signed interpretation of the same 64-bit
    word.  The score algebra stores an i64 and casts it back to u64 when it
    emits its sign-flipped big-endian integer bytes. *)
Lemma signed_fixed64_word_round_trip :
  forall bits,
    (bits < 18446744073709551616)%N ->
    Z.to_N (Z.modulo
      (if N.ltb bits 9223372036854775808
       then Z.of_N bits
       else (Z.of_N bits - 18446744073709551616)%Z)
      18446744073709551616) = bits.
Proof.
  intros bits bounded.
  apply N2Z.inj_lt in bounded.
  change (Z.of_N bits < 18446744073709551616)%Z in bounded.
  pose proof (N2Z.is_nonneg bits) as nonnegative.
  destruct (N.ltb bits 9223372036854775808) eqn:sign.
  - rewrite Z.mod_small by lia.
    apply N2Z.id.
  - replace (Z.of_N bits - 18446744073709551616)%Z with
      (Z.of_N bits + (-1) * 18446744073709551616)%Z by lia.
    rewrite Z.mod_add by lia.
    rewrite Z.mod_small by lia.
    apply N2Z.id.
Qed.

Theorem source_and_target_fixed64_cast_preserve_bitword :
  forall bits,
    (bits < 18446744073709551616)%N ->
    Target.signed_score_word (Scalar.signed_64 bits) = bits /\
    Process.source_signed_word (Core.source_signed_64 bits) = bits.
Proof.
  intros bits bounded.
  split.
  - unfold Target.signed_score_word, Scalar.signed_64.
    now apply signed_fixed64_word_round_trip.
  - unfold Process.source_signed_word, Core.source_signed_64.
    now apply signed_fixed64_word_round_trip.
Qed.

Theorem projected_gdouble_score_matches_signed_cast :
  forall bits,
    (bits < 18446744073709551616)%N ->
    Core.project_score
      (Core.source_score_constructor 14 [Core.SourceInteger bits]) =
    Target.score_constructor 14
      [Target.ScoreInteger
        (Target.signed_score_word (Scalar.signed_64 bits))].
Proof.
  intros bits bounded.
  rewrite Core.project_source_score_constructor.
  simpl.
  rewrite (proj1 (source_and_target_fixed64_cast_preserve_bitword
    bits bounded)).
  reflexivity.
Qed.

Example fixed64_sign_bit_emits_negative_integer_score_bytes :
  Target.score_integer_bytes
    (Target.signed_score_word
      (Scalar.signed_64 9223372036854775808)) =
    [1; 1; 0; 0; 0; 0; 0; 0; 0; 0].
Proof. vm_compute. reflexivity. Qed.

Example fixed64_max_emits_minus_one_integer_score_bytes :
  Target.score_integer_bytes
    (Target.signed_score_word
      (Scalar.signed_64 18446744073709551615)) =
    [1; 1; 127; 255; 255; 255; 255; 255; 255; 255].
Proof. vm_compute. reflexivity. Qed.

Print Assumptions source_and_target_fixed64_cast_preserve_bitword.
Print Assumptions projected_gdouble_score_matches_signed_cast.
Print Assumptions fixed64_sign_bit_emits_negative_integer_score_bytes.
Print Assumptions fixed64_max_emits_minus_one_integer_score_bytes.
