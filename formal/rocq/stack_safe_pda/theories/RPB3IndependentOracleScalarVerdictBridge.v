From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3SourceScalarReduction RPB3AuxiliaryTypedReference
  RPB3IndependentOracleRecursive RPB3IndependentOracleRecursiveProjection
  RPB3RecursiveAuxiliarySourceReference
  RPB3IndependentOracleProcessChildProvenance
  RPB3IndependentOracleScalarContractBridge
  RPB3IndependentOracleScalarDecoderBridge
  RPB3IndependentOracleRegionChildBridge.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Scalar := RPB3SourceScalarReduction.
Module Auxiliary := RPB3AuxiliaryTypedReference.
Module Oracle := RPB3IndependentOracleRecursive.
Module RecursiveProjection := RPB3IndependentOracleRecursiveProjection.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.
Module ProcessChild := RPB3IndependentOracleProcessChildProvenance.
Module ContractBridge := RPB3IndependentOracleScalarContractBridge.
Module DecoderBridge := RPB3IndependentOracleScalarDecoderBridge.
Module RegionChild := RPB3IndependentOracleRegionChildBridge.

Lemma wire_eqb_true_same_wire :
  forall left right, Schema.wire_eqb left right = true -> left = right.
Proof. intros [] [] same; try discriminate same; reflexivity. Qed.

Lemma scalar_wire_tables_agree :
  forall kind, Admission.source_scalar_wire kind = Scalar.scalar_rule_wire kind.
Proof. intros []; reflexivity. Qed.

(** The reducer's rule search and the oracle's contract search read the same
    generated row selected by the program's family, so they agree for every
    program, generated or not; [generated_scalar_rule_matches_source_contract]
    states the same fact for looked-up programs. *)
Lemma generated_scalar_rule_agrees_with_family_contract :
  forall program tag,
    option_map (fun r => (Scalar.scalar_kind r, Scalar.scalar_default_forbidden r))
      (Scalar.generated_scalar_rule program tag) =
    option_map (fun c => (Admission.source_contract_kind c,
                          Admission.source_contract_default_forbidden c))
      (Admission.source_scalar_contract_for (Schema.program_family program) tag).
Proof.
  intros program tag.
  unfold Scalar.generated_scalar_rule, Admission.source_scalar_contract_for.
  destruct (find (fun row => Data.program_type_id row =? Schema.program_family program)
      Data.generated_programs) as [row|] eqn:selected; [|reflexivity].
  destruct (Scalar.found_generated_row_is_member_and_matches
      (Schema.program_family program) Data.generated_programs row selected)
    as [member _].
  exact (ContractBridge.row_rules_agree_with_contract_search row tag member).
Qed.

(** The decoder bridge covers twelve kinds.  The finite certificate below shows
    that every rule in the generated table has one of them, so the bridge is
    enough for every rule the reducer can select. *)
Definition decoder_bridged_kind (kind : Data.protobuf_kind) : bool :=
  match kind with
  | Data.PBBool | Data.PBInt32 | Data.PBSint32 | Data.PBUint32
  | Data.PBInt64 | Data.PBSint64 | Data.PBUint64 | Data.PBFixed64
  | Data.PBString | Data.PBRepeatedString
  | Data.PBBytes | Data.PBRepeatedBytes => true
  | _ => false
  end.

Lemma decoder_bridged_kind_is_listed :
  forall kind,
    decoder_bridged_kind kind = true ->
    In kind [Data.PBBool; Data.PBInt32; Data.PBSint32; Data.PBUint32;
      Data.PBInt64; Data.PBSint64; Data.PBUint64; Data.PBFixed64;
      Data.PBString; Data.PBRepeatedString; Data.PBBytes;
      Data.PBRepeatedBytes].
Proof.
  intros [] bridged; try discriminate bridged; cbn;
    repeat (first [left; reflexivity | right]).
Qed.

Lemma decoder_bridged_kind_has_wire :
  forall kind,
    decoder_bridged_kind kind = true ->
    exists wire, Scalar.scalar_rule_wire kind = Some wire.
Proof. intros [] bridged; try discriminate bridged; eexists; reflexivity. Qed.

Definition generated_rule_kinds_are_decoder_bridged : bool :=
  forallb (fun row =>
    forallb (fun rule => decoder_bridged_kind (Scalar.scalar_kind rule))
      (flat_map Scalar.slot_scalar_rules (Data.program_slots row)))
    Data.generated_programs.

Theorem finite_generated_rule_kinds_are_decoder_bridged :
  generated_rule_kinds_are_decoder_bridged = true.
Proof. vm_compute. reflexivity. Qed.

Lemma generated_rule_kind_is_decoder_bridged :
  forall program tag rule,
    Scalar.generated_scalar_rule program tag = Some rule ->
    decoder_bridged_kind (Scalar.scalar_kind rule) = true.
Proof.
  intros program tag rule found.
  unfold Scalar.generated_scalar_rule in found.
  destruct (find (fun row => Data.program_type_id row =? Schema.program_family program)
      Data.generated_programs) as [row|] eqn:selected; [|discriminate].
  destruct (Scalar.found_generated_row_is_member_and_matches
      (Schema.program_family program) Data.generated_programs row selected)
    as [member _].
  apply find_some in found as [rule_member _].
  pose proof finite_generated_rule_kinds_are_decoder_bridged as certificate.
  unfold generated_rule_kinds_are_decoder_bridged in certificate.
  rewrite forallb_forall in certificate.
  specialize (certificate row member).
  rewrite forallb_forall in certificate.
  exact (certificate rule rule_member).
Qed.

(** Every-wire form of [ProcessChild.independent_generated_scalar_gate]. *)
Lemma independent_scalar_gate_for_contract_wire :
  forall bytes program span field raw kind forbidden wire,
    Source.source_descriptor program span = Some field ->
    Source.raw_field_of_source bytes (Source.SourceScalar span field) =
      Some raw ->
    Schema.descriptor_wire field = wire ->
    Admission.source_scalar_contract_for (Schema.program_family program)
      (Source.Wire.span_number span) =
      Some (Admission.SourceScalarContract kind forbidden) ->
    Admission.source_scalar_wire kind = Some wire ->
    Admission.source_typed_scalar program raw =
      if Schema.valid_raw_body raw then
        option_map (Core.TypedSourceScalar raw)
          (Core.independently_decode_scalar kind forbidden
            (Schema.raw_body raw))
      else None.
Proof.
  intros bytes program span field raw kind forbidden wire
    selected converted field_wire contract kind_wire.
  pose proof (Source.raw_field_of_source_preserves_wire_and_body
    bytes (Source.SourceScalar span field) raw converted)
    as [raw_tag [raw_wire _]].
  cbn [Source.source_span Source.source_field] in raw_tag, raw_wire.
  pose proof (ProcessChild.converted_scalar_raw_has_scalar_shape
    bytes span field raw converted)
    as [raw_payload [no_type field_payload]].
  pose proof (Source.resolved_source_descriptor_has_actual_tag_and_wire
    program span field selected) as [_ span_wire_matches].
  rewrite field_wire in span_wire_matches.
  assert (raw_wire_same : Schema.raw_wire raw = wire).
  { rewrite raw_wire. exact (wire_eqb_true_same_wire _ _ span_wire_matches). }
  unfold Source.source_descriptor in selected.
  destruct (Schema.resolve_tag (Schema.program_fields program)
    (Source.Wire.span_number span)) as [candidate|] eqn:resolved;
    [|discriminate].
  destruct (Schema.wire_eqb (Source.wire_kind_of_span span)
    (Schema.descriptor_wire candidate)); [|discriminate].
  inversion selected; subst candidate.
  unfold Admission.source_typed_scalar,
    Admission.source_scalar_value_of_raw.
  rewrite raw_payload, no_type, raw_tag, resolved, contract.
  cbn [Admission.source_contract_kind
    Admission.source_contract_default_forbidden].
  rewrite kind_wire, raw_wire_same, field_wire, field_payload.
  change (Admission.source_raw_bytes_well_formed raw)
    with (Schema.valid_raw_body raw).
  destruct wire; cbn [Schema.wire_eqb Schema.payload_eqb andb];
    destruct (Schema.valid_raw_body raw); reflexivity.
Qed.

(** Every-rule form of [ProcessChild.recursive_generated_scalar_gate]. *)
Lemma recursive_scalar_gate_for_rule :
  forall bytes program span field raw rule,
    Source.source_descriptor program span = Some field ->
    Source.raw_field_of_source bytes (Source.SourceScalar span field) =
      Some raw ->
    Scalar.generated_scalar_rule program (Source.Wire.span_number span) =
      Some rule ->
    Recursive.checked_generated_scalar_raw bytes program
      (Source.SourceScalar span field) =
      if Schema.valid_raw_body raw &&
          Scalar.scalar_body_valid rule (Schema.raw_body raw)
      then Some raw else None.
Proof.
  intros bytes program span field raw rule selected converted found_rule.
  pose proof (ProcessChild.converted_scalar_raw_has_scalar_shape
    bytes span field raw converted) as [_ [_ scalar_payload]].
  unfold Recursive.checked_generated_scalar_raw,
    Recursive.checked_generated_raw, Scalar.source_scalar_raw.
  cbn [Source.source_span Source.source_field].
  rewrite selected, converted, found_rule, scalar_payload.
  cbn [Schema.payload_eqb andb].
  destruct (Auxiliary.descriptor_eq_dec field field) as [_|different];
    [|contradiction].
  destruct (Schema.valid_raw_body raw),
    (Scalar.scalar_body_valid rule (Schema.raw_body raw));
    reflexivity.
Qed.

(** Every-wire form of [ProcessChild.selected_generated_scalar_node_has_child_verdict],
    with the validity of the raw body already established for decoded nodes. *)
Lemma scalar_node_verdict_of_matching_rule :
  forall recurse par_child hash_fn bytes program span field raw rule wire,
    Source.source_descriptor program span = Some field ->
    Source.raw_field_of_source bytes (Source.SourceScalar span field) =
      Some raw ->
    Schema.valid_raw_body raw = true ->
    Admission.source_scalar_contract_for (Schema.program_family program)
      (Source.Wire.span_number span) =
      Some (Admission.SourceScalarContract
        (Scalar.scalar_kind rule) (Scalar.scalar_default_forbidden rule)) ->
    Scalar.generated_scalar_rule program (Source.Wire.span_number span) =
      Some rule ->
    Scalar.scalar_rule_wire (Scalar.scalar_kind rule) = Some wire ->
    wire = Schema.descriptor_wire field ->
    (Core.independently_decode_scalar (Scalar.scalar_kind rule)
       (Scalar.scalar_default_forbidden rule) (Schema.raw_body raw) = None <->
     Scalar.scalar_body_valid rule (Schema.raw_body raw) = false) ->
    RecursiveProjection.source_target_child_verdict
      (Oracle.source_evaluate_node recurse hash_fn bytes program
        (Source.SourceScalar span field))
      (Recursive.recursive_source_reference par_child hash_fn bytes program
        (Source.SourceScalar span field)).
Proof.
  intros recurse par_child hash_fn bytes program span field raw rule wire
    selected converted valid contract found_rule kind_wire wire_is_descriptor
    decoder_refusal.
  pose proof (ProcessChild.converted_scalar_raw_has_scalar_shape
    bytes span field raw converted) as [payload [no_type _]].
  pose proof (independent_scalar_gate_for_contract_wire bytes program span field
    raw (Scalar.scalar_kind rule) (Scalar.scalar_default_forbidden rule) wire
    selected converted (eq_sym wire_is_descriptor) contract
    (eq_trans (scalar_wire_tables_agree (Scalar.scalar_kind rule)) kind_wire))
    as source_gate.
  pose proof (recursive_scalar_gate_for_rule bytes program span field raw rule
    selected converted found_rule) as target_gate.
  unfold Oracle.source_evaluate_node.
  rewrite converted, source_gate.
  cbn [Recursive.recursive_source_reference].
  rewrite target_gate.
  rewrite valid.
  cbn [andb option_map].
  destruct (Scalar.scalar_body_valid rule (Schema.raw_body raw)) eqn:grammar.
  - destruct (Core.independently_decode_scalar (Scalar.scalar_kind rule)
      (Scalar.scalar_default_forbidden rule) (Schema.raw_body raw))
      as [scalar|] eqn:decoded.
    + rewrite (ProcessChild.generated_scalar_summary_projects_to_target_reduction
        raw scalar payload no_type valid).
      simpl. split; reflexivity.
    + pose proof (proj1 decoder_refusal eq_refl) as invalid.
      discriminate invalid.
  - pose proof (proj2 decoder_refusal eq_refl) as decoded.
    rewrite decoded. exact I.
Qed.

Lemma source_typed_scalar_without_contract :
  forall program raw,
    Admission.source_scalar_contract_for (Schema.program_family program)
      (Schema.raw_tag raw) = None ->
    Admission.source_typed_scalar program raw = None.
Proof.
  intros program raw missing.
  unfold Admission.source_typed_scalar, Admission.source_scalar_value_of_raw.
  rewrite missing.
  destruct (Schema.raw_payload raw), (Schema.raw_program_type_id raw),
    (Schema.resolve_tag (Schema.program_fields program) (Schema.raw_tag raw));
    reflexivity.
Qed.

Lemma checked_scalar_gate_without_rule :
  forall bytes program span field,
    Scalar.generated_scalar_rule program (Source.Wire.span_number span) = None ->
    Recursive.checked_generated_scalar_raw bytes program
      (Source.SourceScalar span field) = None.
Proof.
  intros bytes program span field missing.
  unfold Recursive.checked_generated_scalar_raw, Scalar.source_scalar_raw.
  rewrite missing.
  destruct (Source.raw_field_of_source bytes (Source.SourceScalar span field));
    destruct (Source.source_descriptor program span);
    destruct (Recursive.checked_generated_raw bytes program
      (Source.SourceScalar span field));
    reflexivity.
Qed.

(** The verdict for an arbitrary program, given one body-independent premise
    on the node: when the node's rule has a wire, the descriptor carries it.
    Wire disagreement is the only failure mode.  The independent gate compares
    the rule's wire with the raw wire and the descriptor, while the target only
    checks the body against the rule, so a mismatched wire that the rule's body
    grammar accepts splits the verdict (see
    [generated_scalar_node_verdict_requires_generated_program] below). *)
Theorem wire_coherent_scalar_node_verdict :
  forall recurse par_child hash_fn bytes program fuel start finish nodes span field,
    Source.decode_region fuel bytes program start finish = Some nodes ->
    In (Source.SourceScalar span field) nodes ->
    (Schema.descriptor_payload field = Schema.Scalar ->
     forall rule wire,
       Scalar.generated_scalar_rule program (Source.Wire.span_number span) =
         Some rule ->
       Scalar.scalar_rule_wire (Scalar.scalar_kind rule) = Some wire ->
       wire = Schema.descriptor_wire field) ->
    RecursiveProjection.source_target_child_verdict
      (Oracle.source_evaluate_node recurse hash_fn bytes program
        (Source.SourceScalar span field))
      (Recursive.recursive_source_reference par_child hash_fn bytes program
        (Source.SourceScalar span field)).
Proof.
  intros recurse par_child hash_fn bytes program fuel start finish nodes span
    field decoded member coherent.
  destruct (Source.source_forall_member _ _ _ _
    (Source.decoded_region_nodes_match_generated_source
      fuel bytes program start finish nodes decoded) member) as [selected _].
  cbn [Source.source_span Source.source_field] in selected.
  destruct (Source.raw_field_of_source bytes (Source.SourceScalar span field))
    as [raw|] eqn:converted.
  - pose proof (ProcessChild.converted_scalar_raw_has_scalar_shape
      bytes span field raw converted) as [_ [_ field_payload]].
    pose proof (RegionChild.decoded_node_raw_body_is_valid fuel bytes program
      start finish nodes (Source.SourceScalar span field) raw decoded member
      converted) as valid.
    pose proof (Source.raw_field_of_source_preserves_wire_and_body
      bytes (Source.SourceScalar span field) raw converted) as [raw_tag _].
    cbn [Source.source_span] in raw_tag.
    pose proof (generated_scalar_rule_agrees_with_family_contract program
      (Source.Wire.span_number span)) as agreement.
    destruct (Scalar.generated_scalar_rule program
        (Source.Wire.span_number span)) as [rule|] eqn:found_rule.
    + destruct (Admission.source_scalar_contract_for
          (Schema.program_family program) (Source.Wire.span_number span))
        as [[contract_kind contract_forbidden]|] eqn:found_contract;
        [|discriminate agreement].
      cbn [option_map Admission.source_contract_kind
        Admission.source_contract_default_forbidden] in agreement.
      injection agreement as same_kind same_forbidden.
      subst contract_kind contract_forbidden.
      pose proof (generated_rule_kind_is_decoder_bridged program
        (Source.Wire.span_number span) rule found_rule) as bridged.
      destruct (decoder_bridged_kind_has_wire (Scalar.scalar_kind rule) bridged)
        as [wire kind_wire].
      pose proof (coherent field_payload rule wire eq_refl kind_wire)
        as wire_is_descriptor.
      assert (in_range : Core.bytes_in_range (Schema.raw_body raw) = true).
      { pose proof valid as valid_copy.
        unfold Schema.valid_raw_body in valid_copy.
        apply andb_true_iff in valid_copy as [_ in_range].
        exact in_range. }
      exact (scalar_node_verdict_of_matching_rule recurse par_child hash_fn
        bytes program span field raw rule wire selected converted valid
        found_contract found_rule kind_wire wire_is_descriptor
        (DecoderBridge.independently_decode_scalar_iff_body_valid rule
          (Schema.raw_body raw) (decoder_bridged_kind_is_listed _ bridged)
          in_range)).
    + destruct (Admission.source_scalar_contract_for
          (Schema.program_family program) (Source.Wire.span_number span))
        as [contract|] eqn:missing_contract; [discriminate agreement|].
      assert (source_none : Admission.source_typed_scalar program raw = None).
      { apply source_typed_scalar_without_contract.
        rewrite raw_tag. exact missing_contract. }
      pose proof (checked_scalar_gate_without_rule bytes program span field
        found_rule) as target_none.
      unfold Oracle.source_evaluate_node.
      rewrite converted, source_none.
      cbn [Recursive.recursive_source_reference].
      rewrite target_none.
      exact I.
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

(** Generated programs satisfy the coherence premise: the table certificate
    [Scalar.every_generated_scalar_descriptor_selects_a_rule] already compares
    the rule's wire with every scalar descriptor of a looked-up program. *)
Lemma generated_scalar_descriptor_wire_is_rule_wire :
  forall family program span field rule wire,
    Schema.lookup_projected_program family [] [] = Some program ->
    Source.source_descriptor program span = Some field ->
    Schema.descriptor_payload field = Schema.Scalar ->
    Scalar.generated_scalar_rule program (Source.Wire.span_number span) =
      Some rule ->
    Scalar.scalar_rule_wire (Scalar.scalar_kind rule) = Some wire ->
    wire = Schema.descriptor_wire field.
Proof.
  intros family program span field rule wire looked_up selected scalar
    found_rule kind_wire.
  pose proof (Source.resolved_source_descriptor_has_actual_tag_and_wire
    program span field selected) as [same_tag _].
  assert (field_member : In field (Schema.program_fields program)).
  { unfold Source.source_descriptor in selected.
    destruct (Schema.resolve_tag (Schema.program_fields program)
      (Source.Wire.span_number span)) as [candidate|] eqn:resolved;
      [|discriminate].
    destruct (Schema.wire_eqb (Source.wire_kind_of_span span)
      (Schema.descriptor_wire candidate)); [|discriminate].
    inversion selected; subst candidate.
    eapply ProcessChild.resolved_generated_field_is_member; exact resolved. }
  destruct (Scalar.every_generated_scalar_descriptor_selects_a_rule
    family program field looked_up field_member scalar)
    as [selected_rule [selected_wire [found_by_tag [rule_wire descriptor_wire]]]].
  rewrite same_tag, found_rule in found_by_tag.
  inversion found_by_tag; subst selected_rule.
  rewrite kind_wire in rule_wire.
  inversion rule_wire; subst selected_wire.
  exact (wire_eqb_true_same_wire _ _ descriptor_wire).
Qed.

(** The node verdict holds for every wire kind on any program looked up from
    the generated table.  This is the premise of
    [ProcessChild.selected_generated_scalar_node_has_child_verdict], the
    Varint-only lemma this theorem generalizes; the theorem below
    [generated_scalar_node_verdict_requires_generated_program] shows it cannot
    be omitted. *)
Theorem generated_scalar_node_verdict :
  forall recurse par_child hash_fn bytes family program fuel start finish nodes
      span field,
    Schema.lookup_projected_program family [] [] = Some program ->
    Source.decode_region fuel bytes program start finish = Some nodes ->
    In (Source.SourceScalar span field) nodes ->
    RecursiveProjection.source_target_child_verdict
      (Oracle.source_evaluate_node recurse hash_fn bytes program
        (Source.SourceScalar span field))
      (Recursive.recursive_source_reference par_child hash_fn bytes program
        (Source.SourceScalar span field)).
Proof.
  intros recurse par_child hash_fn bytes family program fuel start finish nodes
    span field looked_up decoded member.
  apply (wire_coherent_scalar_node_verdict recurse par_child hash_fn bytes
    program fuel start finish nodes span field decoded member).
  intros field_payload rule wire found_rule kind_wire.
  destruct (Source.source_forall_member _ _ _ _
    (Source.decoded_region_nodes_match_generated_source
      fuel bytes program start finish nodes decoded) member) as [selected _].
  cbn [Source.source_span Source.source_field] in selected.
  exact (generated_scalar_descriptor_wire_is_rule_wire family program span
    field rule wire looked_up selected field_payload found_rule kind_wire).
Qed.

(** Negative control, matching the Example-style checks in the surrounding
    theories.  Generated Par (family 1) makes tag 10 a [PBBool] varint.  A
    program that keeps the family and tag but declares the descriptor
    length-delimited is decoded and refused by the independent gate (the rule
    wire differs from the raw wire), while the target only checks the body
    against the rule and accepts it, so the verdict is [False]. *)
Definition wire_mismatched_field : Schema.field_descriptor :=
  Schema.FieldDescriptor 10 0 Schema.LengthDelimited false Schema.Scalar None None.

Definition wire_mismatched_program : Schema.generated_program :=
  Schema.GeneratedProgram 1 1 [wire_mismatched_field] [] [].

(** Field key [(10 << 3) | 2 = 82], declared length 1, payload byte 1. *)
Definition wire_mismatched_bytes : list nat := [82; 1; 1].

Definition wire_mismatched_span : Source.Wire.field_span :=
  Source.Wire.FieldSpan 10 Source.Wire.WireLength 0 1 2 3 None.

Definition wire_mismatched_node : Source.source_tree :=
  Source.SourceScalar wire_mismatched_span wire_mismatched_field.

Theorem generated_scalar_node_verdict_requires_generated_program :
  ~ (forall recurse par_child hash_fn bytes program fuel start finish nodes span field,
      Source.decode_region fuel bytes program start finish = Some nodes ->
      In (Source.SourceScalar span field) nodes ->
      RecursiveProjection.source_target_child_verdict
        (Oracle.source_evaluate_node recurse hash_fn bytes program
          (Source.SourceScalar span field))
        (Recursive.recursive_source_reference par_child hash_fn bytes program
          (Source.SourceScalar span field))).
Proof.
  intro unguarded.
  assert (decoded : Source.decode_region 1 wire_mismatched_bytes
      wire_mismatched_program 0 3 = Some [wire_mismatched_node])
    by (vm_compute; reflexivity).
  pose proof (unguarded (fun _ => None) (fun _ => None) (fun _ => 0)
    wire_mismatched_bytes wire_mismatched_program 1 0 3 [wire_mismatched_node]
    wire_mismatched_span wire_mismatched_field decoded (or_introl eq_refl))
    as verdict.
  vm_compute in verdict.
  exact verdict.
Qed.

Print Assumptions wire_eqb_true_same_wire.
Print Assumptions scalar_wire_tables_agree.
Print Assumptions generated_scalar_rule_agrees_with_family_contract.
Print Assumptions decoder_bridged_kind_is_listed.
Print Assumptions decoder_bridged_kind_has_wire.
Print Assumptions finite_generated_rule_kinds_are_decoder_bridged.
Print Assumptions generated_rule_kind_is_decoder_bridged.
Print Assumptions independent_scalar_gate_for_contract_wire.
Print Assumptions recursive_scalar_gate_for_rule.
Print Assumptions scalar_node_verdict_of_matching_rule.
Print Assumptions source_typed_scalar_without_contract.
Print Assumptions checked_scalar_gate_without_rule.
Print Assumptions wire_coherent_scalar_node_verdict.
Print Assumptions generated_scalar_descriptor_wire_is_rule_wire.
Print Assumptions generated_scalar_node_verdict.
Print Assumptions generated_scalar_node_verdict_requires_generated_program.
