From Stdlib Require Import List Bool Arith NArith Lia.
From StackSafePDA Require Import EPM1 RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3SourceScalarReduction
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3IndependentOracleName RPB3SemanticReducer.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Scalar := RPB3SourceScalarReduction.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Name := RPB3IndependentOracleName.
Module Target := RPB3SemanticReducer.

Lemma small_full_varint_one_byte :
  forall body decoded,
    EPM1.decode_varint body = Some (decoded, []) ->
    decoded < 128 -> body = [decoded].
Proof.
  intros body decoded parsed small.
  destruct body as [|first rest].
  - rewrite EPM1.decode_varint_nil in parsed. discriminate.
  - rewrite EPM1.decode_varint_cons in parsed.
    destruct (first <? 128) eqn:head_small.
    + inversion parsed; subst. reflexivity.
    + destruct (first <? 256) eqn:head_byte; [|discriminate].
      destruct (EPM1.decode_varint rest) as [[quotient suffix]|]
        eqn:tail; [|discriminate].
      destruct (quotient =? 0) eqn:nonzero; [discriminate|].
      inversion parsed; subst.
      apply Nat.eqb_neq in nonzero.
      lia.
Qed.

Lemma independent_bool_body_is_one_byte :
  forall body value,
    Core.independently_decode_scalar Data.PBBool false body =
      Some (Core.SourceBool value) ->
    body = [if value then 1 else 0].
Proof.
  intros body value accepted.
  cbn [Core.independently_decode_scalar] in accepted.
  destruct (Core.independently_decoded_u64 body) as [number|]
    eqn:decoded; [|discriminate].
  unfold Core.source_numeric_scalar, Core.source_numeric_nondefault
    in accepted.
  simpl in accepted.
  destruct (N.leb number 1) eqn:bounded;
    simpl in accepted; [|discriminate].
  inversion accepted; subst value.
  apply Core.independently_decoded_u64_has_canonical_full_body
    in decoded as [natural [parsed [numeric maximum]]].
  apply N.leb_le in bounded.
  subst number.
  assert (small : natural < 128) by lia.
  apply small_full_varint_one_byte in parsed; [|exact small].
  rewrite parsed.
  destruct natural as [|[|higher]]; [reflexivity|reflexivity|lia].
Qed.

Lemma generated_connective_bool_contract :
  forall tag,
    In tag [5; 6; 7; 8; 9] ->
    Admission.source_scalar_contract_for 55 tag =
      Some (Admission.SourceScalarContract Data.PBBool false).
Proof.
  intros tag member.
  destruct member as [member|[member|[member|[member|[member|[]]]]]];
    subst tag; vm_compute; reflexivity.
Qed.

Lemma generated_connective_program_family :
  forall program,
    Schema.lookup_projected_program 55 [] [] = Some program ->
    Schema.program_family program = 55.
Proof.
  intros program found.
  unfold Schema.lookup_projected_program in found.
  destruct (find (fun row => Data.program_type_id row =? 55)
      Data.generated_programs) as [row|] eqn:selected;
    [|discriminate].
  inversion found; subst program.
  apply find_some in selected as [_ family].
  cbn [Schema.program_family Schema.project_generated_program].
  now apply Nat.eqb_eq in family.
Qed.

Lemma target_bool_one_byte_agrees :
  forall tag (value : bool),
    Scalar.scalar_semantic_from_source
      (Scalar.ScalarRule tag Data.PBBool false)
      [if value then 1 else 0] =
    Some (Scalar.ScalarBool value).
Proof.
  intros tag value.
  destruct value; vm_compute; reflexivity.
Qed.

Theorem generated_connective_bool_decoder_bridge :
  forall program raw value,
    Schema.lookup_projected_program 55 [] [] = Some program ->
    In (Schema.raw_tag raw) [5; 6; 7; 8; 9] ->
    Admission.source_typed_scalar program raw =
      Some (Core.TypedSourceScalar raw (Core.SourceBool value)) ->
    Schema.raw_wire raw = Schema.Varint /\
    Schema.raw_payload raw = Schema.Scalar /\
    Scalar.scalar_semantic_from_source
      (Scalar.ScalarRule (Schema.raw_tag raw) Data.PBBool false)
      (Schema.raw_body raw) = Some (Scalar.ScalarBool value).
Proof.
  intros program raw value found member accepted.
  pose proof (generated_connective_program_family program found) as family.
  pose proof (generated_connective_bool_contract
    (Schema.raw_tag raw) member) as contract.
  unfold Admission.source_typed_scalar,
    Admission.source_scalar_value_of_raw in accepted.
  rewrite family, contract in accepted.
  destruct (Schema.raw_payload raw) eqn:payload; try discriminate.
  destruct (Schema.raw_program_type_id raw) as [type_id|]
    eqn:program_type; [discriminate|].
  destruct (Schema.resolve_tag (Schema.program_fields program)
      (Schema.raw_tag raw)) as [descriptor|] eqn:resolved;
    [|discriminate].
  cbn [Admission.source_contract_kind Admission.source_scalar_wire
    Admission.source_contract_default_forbidden] in accepted.
  destruct (Admission.source_raw_bytes_well_formed raw &&
      Schema.wire_eqb (Schema.raw_wire raw) Schema.Varint &&
      Schema.wire_eqb Schema.Varint (Schema.descriptor_wire descriptor) &&
      Schema.payload_eqb (Schema.descriptor_payload descriptor)
        Schema.Scalar) eqn:guard; [|discriminate].
  destruct (Core.independently_decode_scalar Data.PBBool false
      (Schema.raw_body raw)) as [decoded_value|] eqn:decoder;
    [|discriminate].
  cbn [option_map] in accepted.
  inversion accepted; subst decoded_value.
  assert (wire : Schema.raw_wire raw = Schema.Varint).
  { repeat rewrite andb_true_iff in guard.
    destruct guard as [[[bytes wire_match] descriptor_wire] descriptor_payload].
    destruct (Schema.raw_wire raw); cbn in wire_match;
      try discriminate; reflexivity. }
  split; [exact wire|].
  split; [reflexivity|].
  apply independent_bool_body_is_one_byte in decoder.
  rewrite decoder.
  apply target_bool_one_byte_agrees.
Qed.

Print Assumptions small_full_varint_one_byte.
Print Assumptions independent_bool_body_is_one_byte.
Print Assumptions generated_connective_bool_contract.
Print Assumptions generated_connective_program_family.
Print Assumptions target_bool_one_byte_agrees.
Print Assumptions generated_connective_bool_decoder_bridge.
