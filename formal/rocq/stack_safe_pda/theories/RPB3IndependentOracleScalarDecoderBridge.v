From Stdlib Require Import List Bool Arith NArith Lia.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3RawRegionTree RPB3SourceScalarReduction RPB3IndependentOracleCore
  RPB3IndependentOracleProcessScalarDecoderBridge.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Source := RPB3RawRegionTree.
Module Scalar := RPB3SourceScalarReduction.
Module Core := RPB3IndependentOracleCore.
Module Decoder := RPB3IndependentOracleProcessScalarDecoderBridge.

(** The decoder compares the little-endian value with zero while the
    validity predicate compares each byte with zero; both decide the same
    all-zero property. *)
Lemma source_little_endian_zero_detects_all_zero_bytes :
  forall body,
    N.eqb (Core.source_little_endian body) 0 =
      forallb (Nat.eqb 0) body.
Proof.
  induction body as [|first rest IH]; [reflexivity|].
  cbn [Core.source_little_endian forallb].
  rewrite <- IH.
  apply eq_iff_eq_true.
  rewrite andb_true_iff, !N.eqb_eq, Nat.eqb_eq.
  lia.
Qed.

(** Non-numeric kinds refuse on both sides; for the numeric kinds the two
    definitions test the same default and range guards. *)
Lemma numeric_scalar_refusal_agrees :
  forall kind forbidden value,
    Core.source_numeric_scalar kind forbidden value = None <->
    Scalar.numeric_scalar_valid kind forbidden value = false.
Proof.
  intros kind forbidden value.
  unfold Core.source_numeric_scalar, Core.source_numeric_nondefault,
    Scalar.numeric_scalar_valid.
  destruct kind; cbv beta iota zeta;
    try solve
      [ destruct (negb forbidden || negb (N.eqb value 0));
        split; intro refused; reflexivity ].
  - destruct (negb forbidden || negb (N.eqb value 0)), (N.leb value 1);
      cbn; split; intro refused; try reflexivity; discriminate.
  - destruct (negb forbidden || negb (N.eqb value 0)),
      (N.leb value 2147483647 || N.leb 18446744071562067968 value);
      cbn; split; intro refused; try reflexivity; discriminate.
  - destruct (negb forbidden || negb (N.eqb value 0)),
      (N.leb value 4294967295);
      cbn; split; intro refused; try reflexivity; discriminate.
  - destruct (negb forbidden || negb (N.eqb value 0)),
      (N.leb value 4294967295);
      cbn; split; intro refused; try reflexivity; discriminate.
  - destruct (negb forbidden || negb (N.eqb value 0));
      cbn; split; intro refused; try reflexivity; discriminate.
  - destruct (negb forbidden || negb (N.eqb value 0));
      cbn; split; intro refused; try reflexivity; discriminate.
  - destruct (negb forbidden || negb (N.eqb value 0));
      cbn; split; intro refused; try reflexivity; discriminate.
Qed.

Lemma fixed64_decode_refusal_agrees :
  forall forbidden body,
    Core.bytes_in_range body = true ->
    (Core.source_fixed_scalar Data.PBFixed64 forbidden body = None <->
     Scalar.fixed_scalar_valid Data.PBFixed64 forbidden body = false).
Proof.
  intros forbidden body in_range.
  unfold Core.source_fixed_scalar, Scalar.fixed_scalar_valid.
  rewrite in_range.
  cbv beta iota zeta.
  rewrite source_little_endian_zero_detects_all_zero_bytes.
  destruct (Nat.eqb (length body) 8 &&
      (negb forbidden || negb (forallb (Nat.eqb 0) body)));
    split; intro refused; try reflexivity; discriminate.
Qed.

(** Refusal of the independent decoder coincides with rejection by the
    target validity predicate on in-range bodies. The varint kinds read
    through [Decoder.full_u64_decoder_agreement], and the text kinds through
    [Source.indexed_scan_utf8_accepts_exactly_compact_utf8]. *)
Theorem independently_decode_scalar_iff_body_valid :
  forall rule body,
    In (Scalar.scalar_kind rule)
      [Data.PBBool; Data.PBInt32; Data.PBSint32; Data.PBUint32; Data.PBInt64;
       Data.PBSint64; Data.PBUint64; Data.PBFixed64; Data.PBString;
       Data.PBRepeatedString; Data.PBBytes; Data.PBRepeatedBytes] ->
    Core.bytes_in_range body = true ->
    (Core.independently_decode_scalar (Scalar.scalar_kind rule)
       (Scalar.scalar_default_forbidden rule) body = None <->
     Scalar.scalar_body_valid rule body = false).
Proof.
  intros [tag kind forbidden] body listed in_range.
  cbn [Scalar.scalar_kind] in listed.
  unfold Core.independently_decode_scalar, Scalar.scalar_body_valid.
  cbn [Scalar.scalar_kind Scalar.scalar_default_forbidden].
  destruct listed as
    [listed|[listed|[listed|[listed|[listed|[listed|[listed|
     [listed|[listed|[listed|[listed|[listed|[]]]]]]]]]]]]];
    subst kind; cbv beta iota zeta.
  1-7:
    rewrite <- Decoder.full_u64_decoder_agreement;
    destruct (Core.independently_decoded_u64 body) as [value|];
    [ exact (numeric_scalar_refusal_agrees _ forbidden value)
    | split; intro refused; reflexivity ].
  - exact (fixed64_decode_refusal_agrees forbidden body in_range).
  - rewrite in_range, andb_true_l,
      Source.indexed_scan_utf8_accepts_exactly_compact_utf8.
    unfold Core.source_nondefault.
    destruct (Source.utf8_bytes body &&
        (negb forbidden || negb (Nat.eqb (length body) 0)));
      split; intro refused; try reflexivity; discriminate.
  - rewrite in_range, andb_true_l,
      Source.indexed_scan_utf8_accepts_exactly_compact_utf8.
    unfold Core.source_nondefault.
    destruct (Source.utf8_bytes body &&
        (negb forbidden || negb (Nat.eqb (length body) 0)));
      split; intro refused; try reflexivity; discriminate.
  - rewrite in_range, andb_true_l.
    unfold Core.source_nondefault.
    destruct (negb forbidden || negb (Nat.eqb (length body) 0));
      split; intro refused; try reflexivity; discriminate.
  - rewrite in_range, andb_true_l.
    unfold Core.source_nondefault.
    destruct (negb forbidden || negb (Nat.eqb (length body) 0));
      split; intro refused; try reflexivity; discriminate.
Qed.

Print Assumptions source_little_endian_zero_detects_all_zero_bytes.
Print Assumptions numeric_scalar_refusal_agrees.
Print Assumptions fixed64_decode_refusal_agrees.
Print Assumptions independently_decode_scalar_iff_body_valid.
