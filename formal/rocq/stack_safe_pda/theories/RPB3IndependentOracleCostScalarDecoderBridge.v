From Stdlib Require Import List Bool Arith NArith ZArith.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3SourceScalarReduction
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3IndependentOracleCost RPB3IndependentOracleProjection
  RPB3IndependentOracleProcessScalarDecoderBridge.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Scalar := RPB3SourceScalarReduction.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Cost := RPB3IndependentOracleCost.
Module Projection := RPB3IndependentOracleProjection.
Module Decoder := RPB3IndependentOracleProcessScalarDecoderBridge.

Theorem cost_pbsint32_scalar_equivalence :
  forall tag body value,
    (Core.independently_decode_scalar Data.PBSint32 true body =
      Some (Core.SourceI32 value) <->
     Scalar.scalar_semantic_from_source
       (Scalar.ScalarRule tag Data.PBSint32 true) body =
       Some (Scalar.ScalarI32 value)).
Proof.
  intros tag body value.
  unfold Core.independently_decode_scalar,
    Scalar.scalar_semantic_from_source, Scalar.scalar_body_valid.
  cbn [Scalar.scalar_kind Scalar.scalar_default_forbidden].
  unfold Scalar.numeric_scalar_valid.
  rewrite <- !Decoder.full_u64_decoder_agreement.
  destruct (Core.independently_decoded_u64 body) as [number|];
    [|cbn; split; discriminate].
  unfold Core.source_numeric_scalar, Core.source_numeric_nondefault.
  cbn [negb orb].
  destruct (N.eqb number 0), (N.leb number 4294967295);
    cbn; split; intro accepted; try discriminate;
    inversion accepted; subst; reflexivity.
Qed.

Theorem cost_pbsint32_refusal_equivalence :
  forall tag body,
    Core.independently_decode_scalar Data.PBSint32 true body = None <->
    Scalar.scalar_semantic_from_source
      (Scalar.ScalarRule tag Data.PBSint32 true) body = None.
Proof.
  intros tag body.
  unfold Core.independently_decode_scalar,
    Scalar.scalar_semantic_from_source, Scalar.scalar_body_valid.
  cbn [Scalar.scalar_kind Scalar.scalar_default_forbidden].
  unfold Scalar.numeric_scalar_valid.
  rewrite <- !Decoder.full_u64_decoder_agreement.
  destruct (Core.independently_decoded_u64 body) as [number|];
    [|cbn; split; reflexivity].
  unfold Core.source_numeric_scalar, Core.source_numeric_nondefault.
  cbn [negb orb].
  destruct (N.eqb number 0), (N.leb number 4294967295);
    cbn; split; intro refused; try reflexivity; discriminate.
Qed.

Theorem cost_pbuint32_scalar_equivalence :
  forall tag body value,
    (Core.independently_decode_scalar Data.PBUint32 true body =
      Some (Core.SourceU32 value) <->
     Scalar.scalar_semantic_from_source
       (Scalar.ScalarRule tag Data.PBUint32 true) body =
       Some (Scalar.ScalarU32 value)).
Proof.
  intros tag body value.
  unfold Core.independently_decode_scalar,
    Scalar.scalar_semantic_from_source, Scalar.scalar_body_valid.
  cbn [Scalar.scalar_kind Scalar.scalar_default_forbidden].
  unfold Scalar.numeric_scalar_valid.
  rewrite <- !Decoder.full_u64_decoder_agreement.
  destruct (Core.independently_decoded_u64 body) as [number|];
    [|cbn; split; discriminate].
  unfold Core.source_numeric_scalar, Core.source_numeric_nondefault.
  cbn [negb orb].
  destruct (N.eqb number 0), (N.leb number 4294967295);
    cbn; split; intro accepted; try discriminate;
    inversion accepted; subst; reflexivity.
Qed.

Theorem cost_pbuint32_refusal_equivalence :
  forall tag body,
    Core.independently_decode_scalar Data.PBUint32 true body = None <->
    Scalar.scalar_semantic_from_source
      (Scalar.ScalarRule tag Data.PBUint32 true) body = None.
Proof.
  intros tag body.
  unfold Core.independently_decode_scalar,
    Scalar.scalar_semantic_from_source, Scalar.scalar_body_valid.
  cbn [Scalar.scalar_kind Scalar.scalar_default_forbidden].
  unfold Scalar.numeric_scalar_valid.
  rewrite <- !Decoder.full_u64_decoder_agreement.
  destruct (Core.independently_decoded_u64 body) as [number|];
    [|cbn; split; reflexivity].
  unfold Core.source_numeric_scalar, Core.source_numeric_nondefault.
  cbn [negb orb].
  destruct (N.eqb number 0), (N.leb number 4294967295);
    cbn; split; intro refused; try reflexivity; discriminate.
Qed.

Theorem cost_pbuint64_scalar_equivalence :
  forall tag body value,
    (Core.independently_decode_scalar Data.PBUint64 true body =
      Some (Core.SourceU64 value) <->
     Scalar.scalar_semantic_from_source
       (Scalar.ScalarRule tag Data.PBUint64 true) body =
       Some (Scalar.ScalarU64 value)).
Proof.
  intros tag body value.
  unfold Core.independently_decode_scalar,
    Scalar.scalar_semantic_from_source, Scalar.scalar_body_valid.
  cbn [Scalar.scalar_kind Scalar.scalar_default_forbidden].
  unfold Scalar.numeric_scalar_valid.
  rewrite <- !Decoder.full_u64_decoder_agreement.
  destruct (Core.independently_decoded_u64 body) as [number|];
    [|cbn; split; discriminate].
  unfold Core.source_numeric_scalar, Core.source_numeric_nondefault.
  cbn [negb orb].
  destruct (N.eqb number 0);
    cbn; split; intro accepted; try discriminate;
    inversion accepted; subst; reflexivity.
Qed.

Theorem cost_pbuint64_refusal_equivalence :
  forall tag body,
    Core.independently_decode_scalar Data.PBUint64 true body = None <->
    Scalar.scalar_semantic_from_source
      (Scalar.ScalarRule tag Data.PBUint64 true) body = None.
Proof.
  intros tag body.
  unfold Core.independently_decode_scalar,
    Scalar.scalar_semantic_from_source, Scalar.scalar_body_valid.
  cbn [Scalar.scalar_kind Scalar.scalar_default_forbidden].
  unfold Scalar.numeric_scalar_valid.
  rewrite <- !Decoder.full_u64_decoder_agreement.
  destruct (Core.independently_decoded_u64 body) as [number|];
    [|cbn; split; reflexivity].
  unfold Core.source_numeric_scalar, Core.source_numeric_nondefault.
  cbn [negb orb].
  destruct (N.eqb number 0);
    cbn; split; intro refused; try reflexivity; discriminate.
Qed.

Lemma generated_cost_bound_i32_contract :
  Admission.source_scalar_contract_for 11 1 =
    Some (Admission.SourceScalarContract Data.PBSint32 true).
Proof. vm_compute. reflexivity. Qed.

Lemma generated_cost_region_u32_contract :
  forall tag,
    In tag [1; 7] ->
    Admission.source_scalar_contract_for 12 tag =
      Some (Admission.SourceScalarContract Data.PBUint32 true).
Proof.
  intros tag member.
  destruct member as [member|[member|[]]];
    subst tag; vm_compute; reflexivity.
Qed.

Lemma generated_cost_region_u64_contract :
  forall tag,
    In tag [2; 3; 4; 5] ->
    Admission.source_scalar_contract_for 12 tag =
      Some (Admission.SourceScalarContract Data.PBUint64 true).
Proof.
  intros tag member.
  destruct member as [member|[member|[member|[member|[]]]]];
    subst tag; vm_compute; reflexivity.
Qed.

Lemma generated_cost_numeric_admission :
  forall family program raw kind source,
    Schema.lookup_projected_program family [] [] = Some program ->
    Admission.source_scalar_contract_for family (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract kind true) ->
    Admission.source_scalar_wire kind = Some Schema.Varint ->
    Admission.source_typed_scalar program raw =
      Some (Core.TypedSourceScalar raw source) ->
    Schema.raw_wire raw = Schema.Varint /\
    Schema.raw_payload raw = Schema.Scalar /\
    Core.independently_decode_scalar kind true (Schema.raw_body raw) =
      Some source.
Proof.
  intros family program raw kind source found contract kind_wire accepted.
  pose proof (Decoder.generated_program_has_requested_family
    family program found) as family_exact.
  unfold Admission.source_typed_scalar,
    Admission.source_scalar_value_of_raw in accepted.
  rewrite family_exact, contract in accepted.
  destruct (Schema.raw_payload raw) eqn:payload; try discriminate.
  destruct (Schema.raw_program_type_id raw) as [type_id|]
    eqn:no_type; try discriminate.
  destruct (Schema.resolve_tag (Schema.program_fields program)
      (Schema.raw_tag raw)) as [descriptor|] eqn:resolved;
    [|discriminate].
  cbn [Admission.source_contract_kind
    Admission.source_contract_default_forbidden] in accepted.
  rewrite kind_wire in accepted.
  destruct (Admission.source_raw_bytes_well_formed raw &&
      Schema.wire_eqb (Schema.raw_wire raw) Schema.Varint &&
      Schema.wire_eqb Schema.Varint (Schema.descriptor_wire descriptor) &&
      Schema.payload_eqb (Schema.descriptor_payload descriptor)
        Schema.Scalar) eqn:guard; [|discriminate].
  destruct (Core.independently_decode_scalar kind true
      (Schema.raw_body raw)) as [decoded|] eqn:decoder;
    [|discriminate].
  cbn [option_map] in accepted.
  inversion accepted; subst decoded.
  assert (wire : Schema.raw_wire raw = Schema.Varint).
  { repeat rewrite andb_true_iff in guard.
    destruct guard as [[[bytes wire_match] descriptor_wire] descriptor_payload].
    destruct (Schema.raw_wire raw); cbn in wire_match;
      try discriminate; reflexivity. }
  repeat split; try reflexivity; assumption.
Qed.

Theorem generated_cost_bound_i32_decoder_bridge :
  forall program raw value,
    Schema.lookup_projected_program 11 [] [] = Some program ->
    Schema.raw_tag raw = 1 ->
    Admission.source_typed_scalar program raw =
      Some (Core.TypedSourceScalar raw (Core.SourceI32 value)) ->
    Schema.raw_wire raw = Schema.Varint /\
    Schema.raw_payload raw = Schema.Scalar /\
    Scalar.scalar_semantic_from_source
      (Scalar.ScalarRule (Schema.raw_tag raw) Data.PBSint32 true)
      (Schema.raw_body raw) = Some (Scalar.ScalarI32 value).
Proof.
  intros program raw value found tag accepted.
  pose proof (generated_cost_numeric_admission 11 program raw
    Data.PBSint32 (Core.SourceI32 value) found
    ltac:(rewrite tag; exact generated_cost_bound_i32_contract)
    ltac:(reflexivity) accepted) as [wire [payload decoded]].
  repeat split; try assumption.
  apply (proj1 (cost_pbsint32_scalar_equivalence
    (Schema.raw_tag raw) (Schema.raw_body raw) value)).
  exact decoded.
Qed.

Theorem generated_cost_region_u32_decoder_bridge :
  forall program raw value,
    Schema.lookup_projected_program 12 [] [] = Some program ->
    In (Schema.raw_tag raw) [1; 7] ->
    Admission.source_typed_scalar program raw =
      Some (Core.TypedSourceScalar raw (Core.SourceU32 value)) ->
    Schema.raw_wire raw = Schema.Varint /\
    Schema.raw_payload raw = Schema.Scalar /\
    Scalar.scalar_semantic_from_source
      (Scalar.ScalarRule (Schema.raw_tag raw) Data.PBUint32 true)
      (Schema.raw_body raw) = Some (Scalar.ScalarU32 value).
Proof.
  intros program raw value found tag accepted.
  pose proof (generated_cost_numeric_admission 12 program raw
    Data.PBUint32 (Core.SourceU32 value) found
    (generated_cost_region_u32_contract _ tag)
    ltac:(reflexivity) accepted) as [wire [payload decoded]].
  repeat split; try assumption.
  apply (proj1 (cost_pbuint32_scalar_equivalence
    (Schema.raw_tag raw) (Schema.raw_body raw) value)).
  exact decoded.
Qed.

Theorem generated_cost_region_u64_decoder_bridge :
  forall program raw value,
    Schema.lookup_projected_program 12 [] [] = Some program ->
    In (Schema.raw_tag raw) [2; 3; 4; 5] ->
    Admission.source_typed_scalar program raw =
      Some (Core.TypedSourceScalar raw (Core.SourceU64 value)) ->
    Schema.raw_wire raw = Schema.Varint /\
    Schema.raw_payload raw = Schema.Scalar /\
    Scalar.scalar_semantic_from_source
      (Scalar.ScalarRule (Schema.raw_tag raw) Data.PBUint64 true)
      (Schema.raw_body raw) = Some (Scalar.ScalarU64 value).
Proof.
  intros program raw value found tag accepted.
  pose proof (generated_cost_numeric_admission 12 program raw
    Data.PBUint64 (Core.SourceU64 value) found
    (generated_cost_region_u64_contract _ tag)
    ltac:(reflexivity) accepted) as [wire [payload decoded]].
  repeat split; try assumption.
  apply (proj1 (cost_pbuint64_scalar_equivalence
    (Schema.raw_tag raw) (Schema.raw_body raw) value)).
  exact decoded.
Qed.

Theorem generated_cost_bound_i32_child_binds :
  forall program raw child value,
    Schema.lookup_projected_program 11 [] [] = Some program ->
    Schema.raw_tag raw = 1 ->
    Cost.source_scalar_leaf program raw = Some child ->
    Core.source_summary_value child =
      Core.TypedSourceScalar raw (Core.SourceI32 value) ->
    Projection.i32_child_binds child.
Proof.
  intros program raw child value found tag leaf shape.
  unfold Cost.source_scalar_leaf in leaf.
  destruct (Admission.source_typed_scalar program raw) as [typed|]
    eqn:accepted; [|discriminate].
  cbn [option_map] in leaf.
  inversion leaf; subst child.
  cbn [Core.source_summary_value] in shape.
  subst typed.
  unfold Projection.i32_child_binds.
  cbn [Core.source_summary_value Core.source_summary_tag
    Core.source_summary_body Core.source_summary_raw Core.source_value_raw].
  pose proof (generated_cost_bound_i32_decoder_bridge
    program raw value found tag accepted) as [_ [_ decoder]].
  exact decoder.
Qed.

Theorem generated_cost_region_u32_child_binds :
  forall program raw child value,
    Schema.lookup_projected_program 12 [] [] = Some program ->
    In (Schema.raw_tag raw) [1; 7] ->
    Cost.source_scalar_leaf program raw = Some child ->
    Core.source_summary_value child =
      Core.TypedSourceScalar raw (Core.SourceU32 value) ->
    Projection.u32_child_binds child.
Proof.
  intros program raw child value found tag leaf shape.
  unfold Cost.source_scalar_leaf in leaf.
  destruct (Admission.source_typed_scalar program raw) as [typed|]
    eqn:accepted; [|discriminate].
  cbn [option_map] in leaf.
  inversion leaf; subst child.
  cbn [Core.source_summary_value] in shape.
  subst typed.
  unfold Projection.u32_child_binds.
  cbn [Core.source_summary_value Core.source_summary_tag
    Core.source_summary_body Core.source_summary_raw Core.source_value_raw].
  pose proof (generated_cost_region_u32_decoder_bridge
    program raw value found tag accepted) as [_ [_ decoder]].
  exact decoder.
Qed.

Theorem generated_cost_region_u64_child_binds :
  forall program raw child value,
    Schema.lookup_projected_program 12 [] [] = Some program ->
    In (Schema.raw_tag raw) [2; 3; 4; 5] ->
    Cost.source_scalar_leaf program raw = Some child ->
    Core.source_summary_value child =
      Core.TypedSourceScalar raw (Core.SourceU64 value) ->
    Projection.u64_child_binds child.
Proof.
  intros program raw child value found tag leaf shape.
  unfold Cost.source_scalar_leaf in leaf.
  destruct (Admission.source_typed_scalar program raw) as [typed|]
    eqn:accepted; [|discriminate].
  cbn [option_map] in leaf.
  inversion leaf; subst child.
  cbn [Core.source_summary_value] in shape.
  subst typed.
  unfold Projection.u64_child_binds.
  cbn [Core.source_summary_value Core.source_summary_tag
    Core.source_summary_body Core.source_summary_raw Core.source_value_raw].
  pose proof (generated_cost_region_u64_decoder_bridge
    program raw value found tag accepted) as [_ [_ decoder]].
  exact decoder.
Qed.

Print Assumptions cost_pbsint32_scalar_equivalence.
Print Assumptions cost_pbsint32_refusal_equivalence.
Print Assumptions cost_pbuint32_scalar_equivalence.
Print Assumptions cost_pbuint32_refusal_equivalence.
Print Assumptions cost_pbuint64_scalar_equivalence.
Print Assumptions cost_pbuint64_refusal_equivalence.
Print Assumptions generated_cost_numeric_admission.
Print Assumptions generated_cost_bound_i32_decoder_bridge.
Print Assumptions generated_cost_region_u32_decoder_bridge.
Print Assumptions generated_cost_region_u64_decoder_bridge.
Print Assumptions generated_cost_bound_i32_child_binds.
Print Assumptions generated_cost_region_u32_child_binds.
Print Assumptions generated_cost_region_u64_child_binds.
