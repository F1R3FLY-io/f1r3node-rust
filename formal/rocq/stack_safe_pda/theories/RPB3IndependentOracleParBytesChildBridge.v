From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree
  RPB3SourceScalarReduction RPB3SemanticReducer
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3IndependentOracleRecursive RPB3IndependentOracleProjection
  RPB3IndependentOracleRecursiveProjection
  RPB3IndependentOracleProcessChildProvenance
  RPB3IndependentOracleCostVerdictBridge
  RPB3RecursiveAuxiliarySourceReference.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Scalar := RPB3SourceScalarReduction.
Module Target := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Oracle := RPB3IndependentOracleRecursive.
Module Projection := RPB3IndependentOracleProjection.
Module RecursiveProjection := RPB3IndependentOracleRecursiveProjection.
Module ChildProvenance := RPB3IndependentOracleProcessChildProvenance.
Module CostVerdict := RPB3IndependentOracleCostVerdictBridge.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.

(** An unguarded PBBytes decoder equivalence is false.  The target scalar
    rule tests only nonempty length; the generated raw-body guard rejects the
    out-of-range byte before either child can be admitted. *)
Theorem par_bytes_body_only_decoder_counterexample :
  Core.independently_decode_scalar Data.PBBytes true [256] = None /\
  Scalar.scalar_body_valid
    (Scalar.ScalarRule 9 Data.PBBytes true) [256] = true.
Proof. vm_compute. split; reflexivity. Qed.

Lemma generated_par_bytes_descriptor :
  Schema.resolve_tag
    (Schema.program_fields Source.generated_par_source_program) 9 =
    Some (Schema.FieldDescriptor 9 7 Schema.LengthDelimited false
      Schema.Scalar None None).
Proof. vm_compute. reflexivity. Qed.

Lemma generated_par_bytes_source_contract :
  Admission.source_scalar_contract_for 1 9 =
    Some (Admission.SourceScalarContract Data.PBBytes true).
Proof. vm_compute. reflexivity. Qed.

Lemma generated_par_bytes_target_rule :
  Scalar.generated_scalar_rule Source.generated_par_source_program 9 =
    Some (Scalar.ScalarRule 9 Data.PBBytes true).
Proof. vm_compute. reflexivity. Qed.

Lemma valid_par_bytes_decoder_matches_rule :
  forall raw,
    Schema.valid_raw_body raw = true ->
    Core.independently_decode_scalar Data.PBBytes true
      (Schema.raw_body raw) =
      if Scalar.scalar_body_valid
        (Scalar.ScalarRule 9 Data.PBBytes true)
        (Schema.raw_body raw)
      then Some (Core.SourceBlob (Schema.raw_body raw))
      else None.
Proof.
  intros raw valid.
  unfold Schema.valid_raw_body in valid.
  apply andb_true_iff in valid as [_ in_range].
  unfold Core.independently_decode_scalar, Core.bytes_in_range,
    Core.source_nondefault, Scalar.scalar_body_valid.
  cbn [Scalar.scalar_kind Scalar.scalar_default_forbidden].
  rewrite in_range.
  destruct (length (Schema.raw_body raw) =? 0);
    reflexivity.
Qed.

Lemma generated_par_bytes_source_gate :
  forall bytes span field raw,
    Source.Wire.span_number span = 9 ->
    Source.source_descriptor Source.generated_par_source_program span =
      Some field ->
    Source.raw_field_of_source bytes
      (Source.SourceScalar span field) = Some raw ->
    Admission.source_typed_scalar
      Source.generated_par_source_program raw =
      if Schema.valid_raw_body raw then
        option_map (Core.TypedSourceScalar raw)
          (Core.independently_decode_scalar Data.PBBytes true
            (Schema.raw_body raw))
      else None.
Proof.
  intros bytes span field raw tag selected converted.
  pose proof (ChildProvenance.converted_scalar_raw_has_scalar_shape
    bytes span field raw converted) as [payload [no_type _]].
  pose proof (Source.raw_field_of_source_preserves_wire_and_body
    bytes (Source.SourceScalar span field) raw converted)
    as [raw_tag [raw_wire _]].
  cbn [Source.source_span] in raw_tag, raw_wire.
  unfold Source.source_descriptor in selected.
  rewrite tag, generated_par_bytes_descriptor in selected.
  cbn [Schema.descriptor_wire] in selected.
  destruct (Schema.wire_eqb (Source.wire_kind_of_span span)
    Schema.LengthDelimited) eqn:wire; [|discriminate].
  inversion selected; subst field.
  apply CostVerdict.target_wire_match_is_equality in wire.
  rewrite wire in raw_wire.
  rewrite tag in raw_tag.
  unfold Admission.source_typed_scalar,
    Admission.source_scalar_value_of_raw.
  rewrite payload, no_type, raw_tag.
  replace (Schema.program_family Source.generated_par_source_program)
    with 1 by reflexivity.
  rewrite generated_par_bytes_descriptor,
    generated_par_bytes_source_contract.
  cbn [Admission.source_contract_kind
    Admission.source_contract_default_forbidden
    Admission.source_scalar_wire].
  rewrite raw_wire.
  cbn [Schema.wire_eqb Schema.payload_eqb andb].
  change (Admission.source_raw_bytes_well_formed raw)
    with (Schema.valid_raw_body raw).
  destruct (Schema.valid_raw_body raw); reflexivity.
Qed.

(** This is pointwise on the actual generated Par node, and includes
    conversion failure, malformed raw bytes, empty bytes, and success. *)
Theorem generated_par_bytes_scalar_child_verdict :
  forall recurse par_child hash_fn bytes program span field,
    Schema.lookup_projected_program 1 [] [] = Some program ->
    Source.Wire.span_number span = 9 ->
    Source.source_descriptor program span = Some field ->
    RecursiveProjection.source_target_child_verdict
      (Oracle.source_evaluate_node recurse hash_fn bytes program
        (Source.SourceScalar span field))
      (Recursive.recursive_source_reference par_child hash_fn bytes
        program (Source.SourceScalar span field)).
Proof.
  intros recurse par_child hash_fn bytes program span field
    looked_up tag selected.
  rewrite Source.generated_par_source_lookup_is_present in looked_up.
  inversion looked_up; subst program.
  destruct (Source.raw_field_of_source bytes
    (Source.SourceScalar span field)) as [raw|] eqn:converted.
  - pose proof (generated_par_bytes_source_gate
      bytes span field raw tag selected converted) as source_gate.
    assert (rule :
      Scalar.generated_scalar_rule Source.generated_par_source_program
        (Source.Wire.span_number span) =
      Some (Scalar.ScalarRule (Source.Wire.span_number span)
        Data.PBBytes true)).
    { rewrite tag. exact generated_par_bytes_target_rule. }
    pose proof (ChildProvenance.recursive_generated_scalar_gate
      bytes Source.generated_par_source_program span field raw
      Data.PBBytes true selected converted rule) as target_gate.
    pose proof (ChildProvenance.converted_scalar_raw_has_scalar_shape
      bytes span field raw converted) as [payload [no_type _]].
    unfold Oracle.source_evaluate_node.
    rewrite converted, source_gate.
    cbn [Recursive.recursive_source_reference].
    rewrite target_gate.
    destruct (Schema.valid_raw_body raw) eqn:valid;
      cbn [andb option_map]; [|exact I].
    rewrite (valid_par_bytes_decoder_matches_rule raw valid).
    rewrite tag.
    destruct (Scalar.scalar_body_valid
      (Scalar.ScalarRule (Source.Wire.span_number span)
        Data.PBBytes true) (Schema.raw_body raw)) eqn:grammar.
    + rewrite tag in grammar.
      rewrite grammar.
      cbn [option_map].
      rewrite (ChildProvenance.generated_scalar_summary_projects_to_target_reduction
        raw (Core.SourceBlob (Schema.raw_body raw))
        payload no_type valid).
      simpl. split; reflexivity.
    + rewrite tag in grammar.
      rewrite grammar.
      cbn [option_map]. exact I.
  - unfold RecursiveProjection.source_target_child_verdict,
      Oracle.source_evaluate_node.
    rewrite converted.
    cbn [Recursive.recursive_source_reference].
    unfold Recursive.checked_generated_scalar_raw,
      Recursive.checked_generated_raw, Scalar.source_scalar_raw.
    cbn [Source.source_span Source.source_field].
    rewrite selected, converted.
    exact I.
Qed.

Print Assumptions par_bytes_body_only_decoder_counterexample.
Print Assumptions generated_par_bytes_descriptor.
Print Assumptions generated_par_bytes_source_contract.
Print Assumptions generated_par_bytes_target_rule.
Print Assumptions valid_par_bytes_decoder_matches_rule.
Print Assumptions generated_par_bytes_source_gate.
Print Assumptions generated_par_bytes_scalar_child_verdict.
