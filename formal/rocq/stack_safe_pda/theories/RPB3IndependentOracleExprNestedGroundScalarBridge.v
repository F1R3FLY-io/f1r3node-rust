From Stdlib Require Import List Bool Arith NArith.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree
  RPB3SourceScalarReduction RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission
  RPB3IndependentOracleRecursive
  RPB3IndependentOracleProcessScalarDecoderBridge
  RPB3IndependentOracleCostScalarDecoderBridge
  RPB3IndependentOracleProcess
  RPB3SemanticReducer
  RPB3IndependentOracleProcessChildProvenance
  RPB3IndependentOracleExprNestedGroundChildBridge.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Scalar := RPB3SourceScalarReduction.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Oracle := RPB3IndependentOracleRecursive.
Module Decoder := RPB3IndependentOracleProcessScalarDecoderBridge.
Module CostDecoder := RPB3IndependentOracleCostScalarDecoderBridge.
Module Process := RPB3IndependentOracleProcess.
Module Target := RPB3SemanticReducer.
Module ChildProvenance := RPB3IndependentOracleProcessChildProvenance.
Module Child := RPB3IndependentOracleExprNestedGroundChildBridge.

(** PBBytes has no numeric decoder.  The independent source checks byte
    range, while the target scalar rule checks explicit-default presence;
    actual raw validity supplies the former.  Both directions hold for a
    valid raw body, regardless of the generated slot tag. *)
Theorem valid_ground_pbbytes_decoder_equivalence :
  forall tag raw bytes,
    Schema.valid_raw_body raw = true ->
    (Core.independently_decode_scalar Data.PBBytes true
      (Schema.raw_body raw) = Some (Core.SourceBlob bytes) <->
     Scalar.scalar_semantic_from_source
       (Scalar.ScalarRule tag Data.PBBytes true)
       (Schema.raw_body raw) = Some (Scalar.ScalarBytes bytes)).
Proof.
  intros tag raw bytes valid.
  unfold Schema.valid_raw_body in valid.
  apply andb_true_iff in valid as [_ in_range].
  unfold Core.independently_decode_scalar,
    Core.source_nondefault, Core.bytes_in_range,
    Scalar.scalar_semantic_from_source, Scalar.scalar_body_valid.
  cbn [Scalar.scalar_kind Scalar.scalar_default_forbidden].
  rewrite in_range.
  destruct (length (Schema.raw_body raw) =? 0);
    cbn [negb andb orb]; split; intro accepted;
    try discriminate; inversion accepted; reflexivity.
Qed.

Theorem valid_ground_pbbytes_refusal_equivalence :
  forall tag raw,
    Schema.valid_raw_body raw = true ->
    (Core.independently_decode_scalar Data.PBBytes true
      (Schema.raw_body raw) = None <->
     Scalar.scalar_semantic_from_source
       (Scalar.ScalarRule tag Data.PBBytes true)
       (Schema.raw_body raw) = None).
Proof.
  intros tag raw valid.
  unfold Schema.valid_raw_body in valid.
  apply andb_true_iff in valid as [_ in_range].
  unfold Core.independently_decode_scalar,
    Core.source_nondefault, Core.bytes_in_range,
    Scalar.scalar_semantic_from_source, Scalar.scalar_body_valid.
  cbn [Scalar.scalar_kind Scalar.scalar_default_forbidden].
  rewrite in_range.
  destruct (length (Schema.raw_body raw) =? 0);
    cbn [negb andb orb]; split; intro refused;
    try discriminate; reflexivity.
Qed.

(** The four inner slots are a finite generated-schema classification.
    This computation contains only descriptor data; body decoding remains
    symbolic, including the u32 maximum. *)
Lemma generated_ground_bytes_contract_and_rule :
  forall family tag program,
    In (family, tag) [(25, 1); (25, 2); (26, 1)] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Admission.source_scalar_contract_for family tag =
      Some (Admission.SourceScalarContract Data.PBBytes true) /\
    Scalar.generated_scalar_rule program tag =
      Some (Scalar.ScalarRule tag Data.PBBytes true).
Proof.
  intros family tag program member looked_up.
  destruct member as [member|[member|[member|[]]]];
    inversion member; subst family tag.
  - split.
    + vm_compute. reflexivity.
    + assert (checked : option_map (fun selected =>
        Scalar.generated_scalar_rule selected 1)
        (Schema.lookup_projected_program 25 [] []) =
        Some (Some (Scalar.ScalarRule 1 Data.PBBytes true))).
      { vm_compute. reflexivity. }
      rewrite looked_up in checked.
      now inversion checked.
  - split.
    + vm_compute. reflexivity.
    + assert (checked : option_map (fun selected =>
        Scalar.generated_scalar_rule selected 2)
        (Schema.lookup_projected_program 25 [] []) =
        Some (Some (Scalar.ScalarRule 2 Data.PBBytes true))).
      { vm_compute. reflexivity. }
      rewrite looked_up in checked.
      now inversion checked.
  - split.
    + vm_compute. reflexivity.
    + assert (checked : option_map (fun selected =>
        Scalar.generated_scalar_rule selected 1)
        (Schema.lookup_projected_program 26 [] []) =
        Some (Some (Scalar.ScalarRule 1 Data.PBBytes true))).
      { vm_compute. reflexivity. }
      rewrite looked_up in checked.
      now inversion checked.
Qed.

Lemma generated_ground_scale_contract_and_rule :
  forall program,
    Schema.lookup_projected_program 26 [] [] = Some program ->
    Admission.source_scalar_contract_for 26 2 =
      Some (Admission.SourceScalarContract Data.PBUint32 true) /\
    Scalar.generated_scalar_rule program 2 =
      Some (Scalar.ScalarRule 2 Data.PBUint32 true).
Proof.
  intros program looked_up.
  split.
  - vm_compute. reflexivity.
  - assert (checked : option_map (fun selected =>
      Scalar.generated_scalar_rule selected 2)
      (Schema.lookup_projected_program 26 [] []) =
      Some (Some (Scalar.ScalarRule 2 Data.PBUint32 true))).
    { vm_compute. reflexivity. }
    rewrite looked_up in checked.
    now inversion checked.
Qed.

(** This admission inversion keeps the raw-byte guard.  It also proves the
    scalar shape that the source parent scorer checks, rather than trusting
    a fabricated TypedSourceScalar value. *)
Lemma admitted_generated_ground_scalar_has_valid_raw :
  forall family program raw kind wire scalar,
    Schema.lookup_projected_program family [] [] = Some program ->
    Admission.source_scalar_contract_for family (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract kind true) ->
    Admission.source_scalar_wire kind = Some wire ->
    Admission.source_typed_scalar program raw =
      Some (Core.TypedSourceScalar raw scalar) ->
    Schema.valid_raw_body raw = true /\
    Schema.raw_payload raw = Schema.Scalar /\
    Schema.raw_program_type_id raw = None /\
    Schema.raw_wire raw = wire /\
    Core.independently_decode_scalar kind true
      (Schema.raw_body raw) = Some scalar.
Proof.
  intros family program raw kind wire scalar looked_up contract
    kind_wire accepted.
  pose proof (Decoder.generated_program_has_requested_family
    family program looked_up) as family_exact.
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
      Schema.wire_eqb (Schema.raw_wire raw) wire &&
      Schema.wire_eqb wire (Schema.descriptor_wire descriptor) &&
      Schema.payload_eqb (Schema.descriptor_payload descriptor)
        Schema.Scalar) eqn:guard; [|discriminate].
  destruct (Core.independently_decode_scalar kind true
      (Schema.raw_body raw)) as [decoded|] eqn:decoder;
    [|discriminate].
  cbn [option_map] in accepted.
  inversion accepted; subst decoded.
  repeat rewrite andb_true_iff in guard.
  destruct guard as [[[raw_valid wire_match] _] _].
  assert (valid : Schema.valid_raw_body raw = true).
  { exact raw_valid. }
  assert (wire_exact : Schema.raw_wire raw = wire).
  { destruct (Schema.raw_wire raw), wire;
      cbn [Schema.wire_eqb] in wire_match;
      try discriminate; reflexivity. }
  repeat split; try assumption.
Qed.

Lemma successful_ground_pbbytes_is_body_blob :
  forall body scalar,
    Core.independently_decode_scalar Data.PBBytes true body =
      Some scalar ->
    scalar = Core.SourceBlob body.
Proof.
  intros body scalar decoded.
  unfold Core.independently_decode_scalar in decoded.
  destruct (Core.bytes_in_range body &&
      Core.source_nondefault true body); [|discriminate].
  now inversion decoded.
Qed.

Lemma successful_ground_pbuint32_is_u32 :
  forall body scalar,
    Core.independently_decode_scalar Data.PBUint32 true body =
      Some scalar ->
    exists value, scalar = Core.SourceU32 value.
Proof.
  intros body scalar decoded.
  unfold Core.independently_decode_scalar in decoded.
  destruct (Core.independently_decoded_u64 body) as [value|];
    [|discriminate].
  unfold Core.source_numeric_scalar in decoded.
  destruct (Core.source_numeric_nondefault true value);
    [|discriminate].
  destruct (N.leb value 4294967295); [|discriminate].
  exists value.
  now inversion decoded.
Qed.

(** This pointwise shape is obtained from the actual source node evaluator.
    Its raw-field conversion and scalar admission are exposed so subsequent
    field selection cannot substitute a fabricated typed value. *)
Lemma evaluated_ground_scalar_node_has_raw_provenance :
  forall recurse hash_fn body program node child,
    Child.ground_fields_are_scalar program = true ->
    Source.source_node_matches_generated_descriptor program node ->
    Oracle.source_evaluate_node recurse hash_fn body program node =
      Some child ->
    exists span field raw scalar,
      node = Source.SourceScalar span field /\
      Source.raw_field_of_source body node = Some raw /\
      Core.source_summary_value child =
        Core.TypedSourceScalar raw scalar /\
      Admission.source_typed_scalar program raw =
        Some (Core.TypedSourceScalar raw scalar) /\
      Schema.raw_tag raw = Schema.descriptor_tag field /\
      Schema.raw_body raw = Source.source_body body node.
Proof.
  intros recurse hash_fn body program node child all_scalar
    [selected _] evaluated.
  pose proof (Child.selected_ground_descriptor_is_scalar program
    (Source.source_span node) (Source.source_field node)
    all_scalar selected) as scalar_field.
  destruct node as [span field|span field nested|span field];
    cbn [Source.source_span Source.source_field] in *.
  - unfold Oracle.source_evaluate_node in evaluated.
    destruct (Source.raw_field_of_source body
      (Source.SourceScalar span field)) as [raw|] eqn:converted;
      [|discriminate].
    destruct (Admission.source_typed_scalar program raw)
      as [value|] eqn:typed; [|discriminate].
    inversion evaluated; subst child.
    unfold Admission.source_typed_scalar in typed.
    destruct (Admission.source_scalar_value_of_raw program raw)
      as [scalar|] eqn:decoded; [|discriminate].
    inversion typed; subst value.
    pose proof (Source.raw_field_of_source_preserves_wire_and_body
      body (Source.SourceScalar span field) raw converted)
      as [raw_tag [_ [_ [raw_body _]]]].
    cbn [Source.source_span Source.source_field] in raw_tag.
    pose proof (Source.resolved_source_descriptor_has_actual_tag_and_wire
      program span field selected) as [field_tag _].
    exists span, field, raw, scalar.
    repeat split; try reflexivity; try exact converted.
    + unfold Admission.source_typed_scalar.
      now rewrite decoded.
    + now rewrite raw_tag, <- field_tag.
    + exact raw_body.
  - unfold Oracle.source_evaluate_node in evaluated.
    unfold Source.raw_field_of_source in evaluated.
    cbn [Source.source_span Source.source_field] in evaluated.
    rewrite scalar_field in evaluated.
    cbn [Schema.payload_eqb] in evaluated.
    discriminate.
  - unfold Oracle.source_evaluate_node in evaluated.
    unfold Source.raw_field_of_source in evaluated.
    cbn [Source.source_span Source.source_field] in evaluated.
    rewrite scalar_field in evaluated.
    discriminate.
Qed.

Theorem admitted_generated_ground_bytes_match_target_rule :
  forall family tag program raw bytes,
    In (family, tag) [(25, 1); (25, 2); (26, 1)] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Schema.raw_tag raw = tag ->
    Admission.source_typed_scalar program raw =
      Some (Core.TypedSourceScalar raw (Core.SourceBlob bytes)) ->
    Schema.valid_raw_body raw = true /\
    Schema.raw_payload raw = Schema.Scalar /\
    Schema.raw_program_type_id raw = None /\
    Schema.raw_wire raw = Schema.LengthDelimited /\
    Scalar.generated_scalar_rule program tag =
      Some (Scalar.ScalarRule tag Data.PBBytes true) /\
    Scalar.scalar_semantic_from_source
      (Scalar.ScalarRule tag Data.PBBytes true)
      (Schema.raw_body raw) = Some (Scalar.ScalarBytes bytes).
Proof.
  intros family tag program raw bytes member looked_up raw_tag typed.
  destruct (generated_ground_bytes_contract_and_rule
    family tag program member looked_up) as [contract rule].
  pose proof (admitted_generated_ground_scalar_has_valid_raw
    family program raw Data.PBBytes Schema.LengthDelimited
    (Core.SourceBlob bytes) looked_up
    ltac:(rewrite raw_tag; exact contract)
    ltac:(reflexivity) typed)
    as [valid [payload [no_type [wire decoded]]]].
  repeat split; try assumption.
  apply (proj1 (valid_ground_pbbytes_decoder_equivalence
    tag raw bytes valid)).
  exact decoded.
Qed.

Theorem admitted_generated_ground_scale_matches_target_rule :
  forall program raw scale,
    Schema.lookup_projected_program 26 [] [] = Some program ->
    Schema.raw_tag raw = 2 ->
    Admission.source_typed_scalar program raw =
      Some (Core.TypedSourceScalar raw (Core.SourceU32 scale)) ->
    Schema.valid_raw_body raw = true /\
    Schema.raw_payload raw = Schema.Scalar /\
    Schema.raw_program_type_id raw = None /\
    Schema.raw_wire raw = Schema.Varint /\
    Scalar.generated_scalar_rule program 2 =
      Some (Scalar.ScalarRule 2 Data.PBUint32 true) /\
    Scalar.scalar_semantic_from_source
      (Scalar.ScalarRule 2 Data.PBUint32 true)
      (Schema.raw_body raw) = Some (Scalar.ScalarU32 scale).
Proof.
  intros program raw scale looked_up raw_tag typed.
  destruct (generated_ground_scale_contract_and_rule program looked_up)
    as [contract rule].
  pose proof (admitted_generated_ground_scalar_has_valid_raw
    26 program raw Data.PBUint32 Schema.Varint
    (Core.SourceU32 scale) looked_up
    ltac:(rewrite raw_tag; exact contract)
    ltac:(reflexivity) typed)
    as [valid [payload [no_type [wire decoded]]]].
  repeat split; try assumption.
  apply (proj1 (CostDecoder.cost_pbuint32_scalar_equivalence
    2 (Schema.raw_body raw) scale)).
  exact decoded.
Qed.

Lemma evaluated_generated_ground_bytes_node_matches_target :
  forall recurse hash_fn family tag program body node child,
    In (family, tag) [(25, 1); (25, 2); (26, 1)] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Child.ground_fields_are_scalar program = true ->
    Source.source_node_matches_generated_descriptor program node ->
    Schema.descriptor_tag (Source.source_field node) = tag ->
    Oracle.source_evaluate_node recurse hash_fn body program node =
      Some child ->
    exists span field raw bytes,
      node = Source.SourceScalar span field /\
      Core.source_summary_value child =
        Core.TypedSourceScalar raw (Core.SourceBlob bytes) /\
      Schema.raw_tag raw = tag /\
      Schema.raw_wire raw = Schema.LengthDelimited /\
      Schema.raw_payload raw = Schema.Scalar /\
      Schema.raw_program_type_id raw = None /\
      Schema.raw_body raw = Source.source_body body node /\
      Scalar.generated_scalar_rule program tag =
        Some (Scalar.ScalarRule tag Data.PBBytes true) /\
      Scalar.scalar_semantic_from_source
        (Scalar.ScalarRule tag Data.PBBytes true)
        (Source.source_body body node) =
        Some (Scalar.ScalarBytes bytes).
Proof.
  intros recurse hash_fn family tag program body node child member
    looked_up all_scalar matched node_tag evaluated.
  destruct (evaluated_ground_scalar_node_has_raw_provenance
    recurse hash_fn body program node child all_scalar matched evaluated)
    as [span [field [raw [scalar
      [node_eq [converted [typed [admitted [raw_tag raw_body]]]]]]]]].
  subst node.
  cbn [Source.source_field] in node_tag.
  assert (tagged : Schema.raw_tag raw = tag).
  { now rewrite raw_tag, node_tag. }
  destruct (generated_ground_bytes_contract_and_rule
    family tag program member looked_up) as [contract _].
  pose proof (admitted_generated_ground_scalar_has_valid_raw
    family program raw Data.PBBytes Schema.LengthDelimited scalar
    looked_up ltac:(rewrite tagged; exact contract)
    ltac:(reflexivity) admitted)
    as [_ [_ [_ [_ decoded]]]].
  pose proof (successful_ground_pbbytes_is_body_blob
    (Schema.raw_body raw) scalar decoded) as blob.
  subst scalar.
  destruct (admitted_generated_ground_bytes_match_target_rule
    family tag program raw (Schema.raw_body raw) member looked_up
    tagged admitted) as [_ [payload [no_type [wire [rule semantic]]]]].
  exists span, field, raw, (Schema.raw_body raw).
  repeat split; try assumption.
  now rewrite <- raw_body.
Qed.

Lemma evaluated_generated_ground_scale_node_matches_target :
  forall recurse hash_fn program body node child,
    Schema.lookup_projected_program 26 [] [] = Some program ->
    Child.ground_fields_are_scalar program = true ->
    Source.source_node_matches_generated_descriptor program node ->
    Schema.descriptor_tag (Source.source_field node) = 2 ->
    Oracle.source_evaluate_node recurse hash_fn body program node =
      Some child ->
    exists span field raw scale,
      node = Source.SourceScalar span field /\
      Core.source_summary_value child =
        Core.TypedSourceScalar raw (Core.SourceU32 scale) /\
      Schema.raw_tag raw = 2 /\
      Schema.raw_wire raw = Schema.Varint /\
      Schema.raw_payload raw = Schema.Scalar /\
      Schema.raw_program_type_id raw = None /\
      Schema.raw_body raw = Source.source_body body node /\
      Scalar.generated_scalar_rule program 2 =
        Some (Scalar.ScalarRule 2 Data.PBUint32 true) /\
      Scalar.scalar_semantic_from_source
        (Scalar.ScalarRule 2 Data.PBUint32 true)
        (Source.source_body body node) =
        Some (Scalar.ScalarU32 scale).
Proof.
  intros recurse hash_fn program body node child looked_up all_scalar
    matched node_tag evaluated.
  destruct (evaluated_ground_scalar_node_has_raw_provenance
    recurse hash_fn body program node child all_scalar matched evaluated)
    as [span [field [raw [scalar
      [node_eq [converted [typed [admitted [raw_tag raw_body]]]]]]]]].
  subst node.
  cbn [Source.source_field] in node_tag.
  assert (tagged : Schema.raw_tag raw = 2).
  { now rewrite raw_tag, node_tag. }
  destruct (generated_ground_scale_contract_and_rule program looked_up)
    as [contract _].
  pose proof (admitted_generated_ground_scalar_has_valid_raw
    26 program raw Data.PBUint32 Schema.Varint scalar looked_up
    ltac:(rewrite tagged; exact contract)
    ltac:(reflexivity) admitted)
    as [_ [_ [_ [_ decoded]]]].
  destruct (successful_ground_pbuint32_is_u32
    (Schema.raw_body raw) scalar decoded) as [scale u32].
  subst scalar.
  destruct (admitted_generated_ground_scale_matches_target_rule
    program raw scale looked_up tagged admitted)
    as [_ [payload [no_type [wire [rule semantic]]]]].
  exists span, field, raw, scale.
  repeat split; try assumption.
  now rewrite <- raw_body.
Qed.

Lemma positional_generated_ground_bytes_values_agree :
  forall recurse hash_fn family tag program body nodes children,
    In (family, tag) [(25, 1); (25, 2); (26, 1)] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Child.ground_fields_are_scalar program = true ->
    Forall2 (fun node child =>
      Source.source_node_matches_generated_descriptor program node /\
      Oracle.source_evaluate_node recurse hash_fn body program node =
        Some child) nodes children ->
    Process.source_ground_blob_or_absent tag
      (map Core.source_summary_value children) =
    Target.exact_ground_bytes_or_absent program tag body nodes.
Proof.
  intros recurse hash_fn family tag program body nodes children
    member looked_up all_scalar aligned.
  induction aligned as [|node child nodes children
      [matched evaluated] rest IH];
    [reflexivity|].
  pose proof (Child.evaluated_ground_node_keeps_selected_tag
    recurse hash_fn body program node child all_scalar matched evaluated)
    as same_tag.
  unfold Process.source_ground_blob_or_absent,
    Process.source_ground_field_at,
    Target.exact_ground_bytes_or_absent,
    Target.exact_ground_node_at.
  cbn [map find].
  rewrite <- same_tag.
  destruct (Schema.descriptor_tag (Source.source_field node) =? tag)
    eqn:selected.
  - apply Nat.eqb_eq in selected.
    destruct (evaluated_generated_ground_bytes_node_matches_target
      recurse hash_fn family tag program body node child
      member looked_up all_scalar matched selected evaluated)
      as [span [field [raw [bytes
        [node_eq [typed [raw_tag [wire [payload [no_type
          [raw_body [rule semantic]]]]]]]]]]]].
    subst node.
    rewrite typed.
    cbn [Core.source_value_raw].
    rewrite wire, payload, no_type, rule.
    now rewrite semantic.
  - exact IH.
Qed.

Lemma positional_generated_ground_scale_values_agree :
  forall recurse hash_fn program body nodes children,
    Schema.lookup_projected_program 26 [] [] = Some program ->
    Child.ground_fields_are_scalar program = true ->
    Forall2 (fun node child =>
      Source.source_node_matches_generated_descriptor program node /\
      Oracle.source_evaluate_node recurse hash_fn body program node =
        Some child) nodes children ->
    Process.source_ground_u32_or_absent 2
      (map Core.source_summary_value children) =
    Target.exact_ground_u32_or_absent program 2 body nodes.
Proof.
  intros recurse hash_fn program body nodes children
    looked_up all_scalar aligned.
  induction aligned as [|node child nodes children
      [matched evaluated] rest IH];
    [reflexivity|].
  pose proof (Child.evaluated_ground_node_keeps_selected_tag
    recurse hash_fn body program node child all_scalar matched evaluated)
    as same_tag.
  unfold Process.source_ground_u32_or_absent,
    Process.source_ground_field_at,
    Target.exact_ground_u32_or_absent,
    Target.exact_ground_node_at.
  cbn [map find].
  rewrite <- same_tag.
  destruct (Schema.descriptor_tag (Source.source_field node) =? 2)
    eqn:selected.
  - apply Nat.eqb_eq in selected.
    destruct (evaluated_generated_ground_scale_node_matches_target
      recurse hash_fn program body node child
      looked_up all_scalar matched selected evaluated)
      as [span [field [raw [scale
        [node_eq [typed [raw_tag [wire [payload [no_type
          [raw_body [rule semantic]]]]]]]]]]]].
    subst node.
    rewrite typed.
    cbn [Core.source_value_raw].
    rewrite wire, payload, no_type, rule.
    now rewrite semantic.
  - exact IH.
Qed.

(** The successful message evaluator itself supplies the decoded nodes
    and positional typed children.  These conclusions therefore cover the
    actual generated type-25/26 child, without a caller-supplied alignment. *)
Theorem actual_generated_ground_bytes_values_agree :
  forall fuel hash_fn raw summary family tag,
    In (family, tag) [(25, 1); (25, 2); (26, 1)] ->
    Schema.raw_program_type_id raw = Some family ->
    Oracle.source_evaluate_message (S fuel) hash_fn raw =
      Some summary ->
    exists program nodes children,
      Schema.lookup_projected_program family [] [] = Some program /\
      Source.decode_region (S (length (Schema.raw_body raw)))
        (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)) =
        Some nodes /\
      summary = Oracle.source_message_summary raw program children /\
      Process.source_ground_blob_or_absent tag
        (map Core.source_summary_value children) =
      Target.exact_ground_bytes_or_absent program tag
        (Schema.raw_body raw) nodes.
Proof.
  intros fuel hash_fn raw summary family tag member raw_type evaluated.
  destruct (ChildProvenance.accepted_independent_message_has_generated_children
    fuel hash_fn raw summary evaluated)
    as [actual_type [program [nodes [children
      [_ [actual_raw_type [looked_up [decoded [shape aligned]]]]]]]]].
  rewrite raw_type in actual_raw_type.
  inversion actual_raw_type; subst actual_type.
  assert (ground_family : In family [25; 26]).
  { destruct member as [selected|[selected|[selected|[]]]];
      inversion selected; subst family tag; simpl; auto. }
  exists program, nodes, children.
  repeat split; try assumption.
  eapply positional_generated_ground_bytes_values_agree;
    [exact member|exact looked_up| |exact aligned].
  eapply Child.generated_ground_program_has_only_scalar_fields;
    [exact ground_family|exact looked_up].
Qed.

Theorem actual_generated_ground_scale_values_agree :
  forall fuel hash_fn raw summary,
    Schema.raw_program_type_id raw = Some 26 ->
    Oracle.source_evaluate_message (S fuel) hash_fn raw =
      Some summary ->
    exists program nodes children,
      Schema.lookup_projected_program 26 [] [] = Some program /\
      Source.decode_region (S (length (Schema.raw_body raw)))
        (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)) =
        Some nodes /\
      summary = Oracle.source_message_summary raw program children /\
      Process.source_ground_u32_or_absent 2
        (map Core.source_summary_value children) =
      Target.exact_ground_u32_or_absent program 2
        (Schema.raw_body raw) nodes.
Proof.
  intros fuel hash_fn raw summary raw_type evaluated.
  destruct (ChildProvenance.accepted_independent_message_has_generated_children
    fuel hash_fn raw summary evaluated)
    as [actual_type [program [nodes [children
      [_ [actual_raw_type [looked_up [decoded [shape aligned]]]]]]]]].
  rewrite raw_type in actual_raw_type.
  inversion actual_raw_type; subst actual_type.
  exists program, nodes, children.
  repeat split; try assumption.
  eapply positional_generated_ground_scale_values_agree;
    [exact looked_up| |exact aligned].
  eapply Child.generated_ground_program_has_only_scalar_fields;
    [simpl; auto|exact looked_up].
Qed.

Theorem actual_bigrat_both_fields_agree :
  forall fuel hash_fn raw summary,
    Schema.raw_program_type_id raw = Some 25 ->
    Oracle.source_evaluate_message (S fuel) hash_fn raw =
      Some summary ->
    exists program nodes children,
      Schema.lookup_projected_program 25 [] [] = Some program /\
      Source.decode_region (S (length (Schema.raw_body raw)))
        (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)) =
        Some nodes /\
      summary = Oracle.source_message_summary raw program children /\
      Process.source_ground_blob_or_absent 1
        (map Core.source_summary_value children) =
        Target.exact_ground_bytes_or_absent program 1
          (Schema.raw_body raw) nodes /\
      Process.source_ground_blob_or_absent 2
        (map Core.source_summary_value children) =
        Target.exact_ground_bytes_or_absent program 2
          (Schema.raw_body raw) nodes.
Proof.
  intros fuel hash_fn raw summary raw_type evaluated.
  destruct (ChildProvenance.accepted_independent_message_has_generated_children
    fuel hash_fn raw summary evaluated)
    as [actual_type [program [nodes [children
      [_ [actual_raw_type [looked_up [decoded [shape aligned]]]]]]]]].
  rewrite raw_type in actual_raw_type.
  inversion actual_raw_type; subst actual_type.
  pose proof (Child.generated_ground_program_has_only_scalar_fields
    25 program ltac:(simpl; auto) looked_up) as all_scalar.
  exists program, nodes, children.
  repeat split; try assumption.
  - eapply (positional_generated_ground_bytes_values_agree
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn 25 1);
      [simpl; auto|exact looked_up|exact all_scalar|exact aligned].
  - eapply (positional_generated_ground_bytes_values_agree
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn 25 2);
      [simpl; auto|exact looked_up|exact all_scalar|exact aligned].
Qed.

Theorem actual_fixed_point_both_fields_agree :
  forall fuel hash_fn raw summary,
    Schema.raw_program_type_id raw = Some 26 ->
    Oracle.source_evaluate_message (S fuel) hash_fn raw =
      Some summary ->
    exists program nodes children,
      Schema.lookup_projected_program 26 [] [] = Some program /\
      Source.decode_region (S (length (Schema.raw_body raw)))
        (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)) =
        Some nodes /\
      summary = Oracle.source_message_summary raw program children /\
      Process.source_ground_blob_or_absent 1
        (map Core.source_summary_value children) =
        Target.exact_ground_bytes_or_absent program 1
          (Schema.raw_body raw) nodes /\
      Process.source_ground_u32_or_absent 2
        (map Core.source_summary_value children) =
        Target.exact_ground_u32_or_absent program 2
          (Schema.raw_body raw) nodes.
Proof.
  intros fuel hash_fn raw summary raw_type evaluated.
  destruct (ChildProvenance.accepted_independent_message_has_generated_children
    fuel hash_fn raw summary evaluated)
    as [actual_type [program [nodes [children
      [_ [actual_raw_type [looked_up [decoded [shape aligned]]]]]]]]].
  rewrite raw_type in actual_raw_type.
  inversion actual_raw_type; subst actual_type.
  pose proof (Child.generated_ground_program_has_only_scalar_fields
    26 program ltac:(simpl; auto) looked_up) as all_scalar.
  exists program, nodes, children.
  repeat split; try assumption.
  - eapply (positional_generated_ground_bytes_values_agree
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn 26 1);
      [simpl; auto|exact looked_up|exact all_scalar|exact aligned].
  - eapply positional_generated_ground_scale_values_agree;
      [exact looked_up|exact all_scalar|exact aligned].
Qed.

Print Assumptions valid_ground_pbbytes_decoder_equivalence.
Print Assumptions valid_ground_pbbytes_refusal_equivalence.
Print Assumptions generated_ground_bytes_contract_and_rule.
Print Assumptions generated_ground_scale_contract_and_rule.
Print Assumptions admitted_generated_ground_scalar_has_valid_raw.
Print Assumptions successful_ground_pbbytes_is_body_blob.
Print Assumptions successful_ground_pbuint32_is_u32.
Print Assumptions evaluated_ground_scalar_node_has_raw_provenance.
Print Assumptions admitted_generated_ground_bytes_match_target_rule.
Print Assumptions admitted_generated_ground_scale_matches_target_rule.
Print Assumptions evaluated_generated_ground_bytes_node_matches_target.
Print Assumptions evaluated_generated_ground_scale_node_matches_target.
Print Assumptions positional_generated_ground_bytes_values_agree.
Print Assumptions positional_generated_ground_scale_values_agree.
Print Assumptions actual_generated_ground_bytes_values_agree.
Print Assumptions actual_generated_ground_scale_values_agree.
Print Assumptions actual_bigrat_both_fields_agree.
Print Assumptions actual_fixed_point_both_fields_agree.
