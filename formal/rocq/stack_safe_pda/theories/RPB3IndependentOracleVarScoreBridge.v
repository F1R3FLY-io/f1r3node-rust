(** Bridge theorem for the Var family, program type 6 (leaf L03 of the
    score-arms design, sections 2 and 5).

    [var_score_projects] states that on the children the independent oracle
    actually produces for a decoded node of the generated Var program, the
    target helper [exact_var_score] applied to the projected children equals
    the projected oracle score [source_var_score], and that the target and
    oracle completeness helpers agree.

    The scan-validity premise is part of the statement form of the pattern
    theorem [generated_name_family_score_projects_totally].  It is not used:
    both Var score helpers read only the children, and the Var completeness of
    both helpers is the constant [Some true].

    Every theory is referred to by its full logical name and no module alias
    is declared.  The placed target helpers of RPB3SemanticReducer and the oracle
    helpers are imported and used unqualified. *)

From Stdlib Require Import List Bool Arith NArith ZArith.
From StackSafePDA Require RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree RPB3SemanticReducer
  RPB3SourceScalarReduction RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission RPB3IndependentOracleRecursive
  RPB3IndependentOracleProjection RPB3IndependentOracleRecursiveProjection
  RPB3IndependentOracleProcessChildProvenance
  RPB3IndependentOracleProcessScalarDecoderBridge.
From StackSafePDA Require Import RPB3SemanticReducer
  RPB3IndependentOracleExprPayload.

Import ListNotations.

(** ** Decoded nodes and their evaluated children *)

(** A decoded node and the child the oracle evaluated from it.  The first
    conjunct is the descriptor the decoder selected; the second is the node's
    evaluation, in the shape produced by
    [RPB3IndependentOracleProcessChildProvenance.generated_selected_children_are_aligned]. *)
Definition aligned_child (fuel : nat) (hash_fn : list nat -> nat)
    (body : list nat) (program : RPB3GeneratedFieldRefinement.generated_program)
    (node : RPB3RawRegionTree.source_tree) (child : RPB3IndependentOracleCore.source_exact_summary) : Prop :=
  RPB3RawRegionTree.source_node_matches_generated_descriptor program node /\
  RPB3IndependentOracleRecursive.source_evaluate_node
    (RPB3IndependentOracleRecursive.source_evaluate_message fuel hash_fn) hash_fn body program node =
    Some child.

Lemma wire_eqb_true_same :
  forall wire other, RPB3GeneratedFieldRefinement.wire_eqb wire other = true -> wire = other.
Proof. intros [] [] same; try discriminate same; reflexivity. Qed.

Lemma existsb_eqb_member :
  forall tag tags, existsb (Nat.eqb tag) tags = true -> In tag tags.
Proof.
  intros tag tags found.
  apply existsb_exists in found as [candidate [member same]].
  apply Nat.eqb_eq in same.
  subst candidate.
  exact member.
Qed.

Lemma forall2_right_forall :
  forall (A B : Type) (relation : A -> B -> Prop) (predicate : B -> Prop)
      lefts rights,
    (forall left_value right_value,
      relation left_value right_value -> predicate right_value) ->
    Forall2 relation lefts rights -> Forall predicate rights.
Proof.
  intros A B relation predicate lefts rights implied related.
  induction related as [|left_value right_value lefts' rights' head rest IH].
  - constructor.
  - constructor.
    + exact (implied left_value right_value head).
    + exact IH.
Qed.

Lemma source_descriptor_field_member :
  forall program span field,
    RPB3RawRegionTree.source_descriptor program span = Some field ->
    In field (RPB3GeneratedFieldRefinement.program_fields program).
Proof.
  intros program span field selected.
  unfold RPB3RawRegionTree.source_descriptor in selected.
  destruct (RPB3GeneratedFieldRefinement.resolve_tag (RPB3GeneratedFieldRefinement.program_fields program)
    (RPB3RawRegionTree.Wire.span_number span)) as [candidate|] eqn:resolved;
    [|discriminate].
  destruct (RPB3GeneratedFieldRefinement.wire_eqb (RPB3RawRegionTree.wire_kind_of_span span)
    (RPB3GeneratedFieldRefinement.descriptor_wire candidate)); [|discriminate].
  inversion selected; subst candidate.
  eapply RPB3IndependentOracleProcessChildProvenance.resolved_generated_field_is_member; exact resolved.
Qed.

(** Every summary the node evaluator returns carries the raw field computed
    for that node, including the two auxiliary-map branches. *)
Lemma evaluated_node_retains_source_raw :
  forall fuel hash_fn body program node child,
    RPB3IndependentOracleRecursive.source_evaluate_node
      (RPB3IndependentOracleRecursive.source_evaluate_message fuel hash_fn) hash_fn body program node =
      Some child ->
    RPB3RawRegionTree.raw_field_of_source body node =
      Some (RPB3IndependentOracleCore.source_summary_raw child).
Proof.
  intros fuel hash_fn body program node child evaluated.
  unfold RPB3IndependentOracleRecursive.source_evaluate_node in evaluated.
  destruct (RPB3RawRegionTree.raw_field_of_source body node) as [child_raw|] eqn:converted;
    [|discriminate].
  destruct node as [span field|span field nested|span field].
  - destruct (RPB3IndependentOracleAdmission.source_typed_scalar program child_raw)
      as [value|] eqn:typed; [|discriminate].
    inversion evaluated; subst child.
    change (Some child_raw = Some (RPB3IndependentOracleCore.source_value_raw value)).
    rewrite (RPB3IndependentOracleRecursiveProjection.accepted_source_typed_scalar_retains_raw
      program child_raw value typed).
    reflexivity.
  - rewrite (RPB3IndependentOracleRecursiveProjection.source_evaluate_message_retains_raw
      fuel hash_fn child_raw child evaluated).
    reflexivity.
  - destruct (RPB3GeneratedFieldRefinement.descriptor_payload field) eqn:field_kind;
      try discriminate evaluated;
      repeat (match type of evaluated with
        | (match ?scrutinee with _ => _ end) = _ => destruct scrutinee
        end; try discriminate evaluated);
      inversion evaluated; subst child;
      reflexivity.
Qed.

(** The summary of a scalar node, with its decoded value. *)
Lemma evaluated_scalar_node_shape :
  forall recurse hash_fn body program span field child,
    RPB3IndependentOracleRecursive.source_evaluate_node recurse hash_fn body program
      (RPB3RawRegionTree.SourceScalar span field) = Some child ->
    exists raw scalar,
      RPB3RawRegionTree.raw_field_of_source body (RPB3RawRegionTree.SourceScalar span field) =
        Some raw /\
      RPB3IndependentOracleAdmission.source_scalar_value_of_raw program raw = Some scalar /\
      RPB3IndependentOracleAdmission.source_typed_scalar program raw =
        Some (RPB3IndependentOracleCore.TypedSourceScalar raw scalar) /\
      child = RPB3IndependentOracleCore.SourceExactSummary (RPB3IndependentOracleCore.TypedSourceScalar raw scalar)
        (RPB3IndependentOracleCore.SourceFamilyAnalysis None None false false false) None.
Proof.
  intros recurse hash_fn body program span field child evaluated.
  unfold RPB3IndependentOracleRecursive.source_evaluate_node in evaluated.
  destruct (RPB3RawRegionTree.raw_field_of_source body (RPB3RawRegionTree.SourceScalar span field))
    as [raw|] eqn:converted; [|discriminate].
  destruct (RPB3IndependentOracleAdmission.source_typed_scalar program raw)
    as [value|] eqn:typed; [|discriminate].
  inversion evaluated; subst child.
  assert (shape : exists scalar,
      RPB3IndependentOracleAdmission.source_scalar_value_of_raw program raw = Some scalar /\
      value = RPB3IndependentOracleCore.TypedSourceScalar raw scalar).
  { unfold RPB3IndependentOracleAdmission.source_typed_scalar in typed.
    destruct (RPB3IndependentOracleAdmission.source_scalar_value_of_raw program raw)
      as [scalar|] eqn:decoded; [|discriminate].
    inversion typed; subst value.
    exists scalar.
    split; reflexivity. }
  destruct shape as [scalar [decoded same_value]].
  subst value.
  exists raw, scalar.
  split; [first [reflexivity|exact converted]|].
  split; [exact decoded|].
  split; [exact typed|].
  reflexivity.
Qed.

(** The independent scalar decoder is applied with the kind and default policy
    of the contract found for the raw field's tag. *)
Lemma scalar_value_decodes_with_contract :
  forall program raw scalar kind forbidden,
    RPB3IndependentOracleAdmission.source_scalar_value_of_raw program raw = Some scalar ->
    RPB3IndependentOracleAdmission.source_scalar_contract_for (RPB3GeneratedFieldRefinement.program_family program)
      (RPB3GeneratedFieldRefinement.raw_tag raw) =
      Some (RPB3IndependentOracleAdmission.SourceScalarContract kind forbidden) ->
    RPB3IndependentOracleCore.independently_decode_scalar kind forbidden (RPB3GeneratedFieldRefinement.raw_body raw) =
      Some scalar.
Proof.
  intros program raw scalar kind forbidden accepted contract.
  unfold RPB3IndependentOracleAdmission.source_scalar_value_of_raw in accepted.
  rewrite contract in accepted.
  destruct (RPB3GeneratedFieldRefinement.raw_payload raw); try discriminate accepted.
  destruct (RPB3GeneratedFieldRefinement.raw_program_type_id raw); [discriminate accepted|].
  destruct (RPB3GeneratedFieldRefinement.resolve_tag (RPB3GeneratedFieldRefinement.program_fields program)
    (RPB3GeneratedFieldRefinement.raw_tag raw)) as [descriptor|]; [|discriminate accepted].
  cbn [RPB3IndependentOracleAdmission.source_contract_kind
    RPB3IndependentOracleAdmission.source_contract_default_forbidden] in accepted.
  destruct (RPB3IndependentOracleAdmission.source_scalar_wire kind) as [wire|];
    [|discriminate accepted].
  destruct (RPB3IndependentOracleAdmission.source_raw_bytes_well_formed raw &&
    RPB3GeneratedFieldRefinement.wire_eqb (RPB3GeneratedFieldRefinement.raw_wire raw) wire &&
    RPB3GeneratedFieldRefinement.wire_eqb wire (RPB3GeneratedFieldRefinement.descriptor_wire descriptor) &&
    RPB3GeneratedFieldRefinement.payload_eqb (RPB3GeneratedFieldRefinement.descriptor_payload descriptor) RPB3GeneratedFieldRefinement.Scalar);
    [exact accepted|discriminate accepted].
Qed.

(** The children selected by the oracle are positional with the decoded nodes,
    and every node matches its generated descriptor. *)
Lemma decoded_selection_is_aligned :
  forall fuel hash_fn body program nodes children,
    RPB3RawRegionTree.decode_region (S (length body)) body program 0 (length body) =
      Some nodes ->
    RPB3RawRegionTree.sequence_source_results
      (map (RPB3IndependentOracleRecursive.source_evaluate_node
             (RPB3IndependentOracleRecursive.source_evaluate_message fuel hash_fn) hash_fn body program)
        nodes)
      = Some children ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children.
Proof.
  intros fuel hash_fn body program nodes children decoded selected.
  exact (RPB3IndependentOracleProcessChildProvenance.generated_selected_children_are_aligned program
    (RPB3IndependentOracleRecursive.source_evaluate_node (RPB3IndependentOracleRecursive.source_evaluate_message fuel hash_fn)
      hash_fn body program) nodes children
    (RPB3RawRegionTree.decoded_region_nodes_match_generated_source _ _ _ _ _ _ decoded)
    (RPB3IndependentOracleRecursiveProjection.selected_source_evaluator_outputs_are_positional
      _ _ _ _ _ selected)).
Qed.

(** ** The level decoders agree

    The oneof allows an explicit zero level, so the contract of tags 1 and 2
    does not forbid the default.  Both decoders then read the same canonical
    varint, bound it by the same 32-bit limit and apply the same zigzag map. *)
Lemma independent_sint32_agrees_with_target_decoder :
  forall tag body scalar,
    RPB3IndependentOracleCore.independently_decode_scalar RPB3DescriptorProjectionData.GeneratedData.PBSint32 false body = Some scalar ->
    exists level,
      scalar = RPB3IndependentOracleCore.SourceI32 level /\
      RPB3SourceScalarReduction.scalar_semantic_from_source
        (RPB3SourceScalarReduction.ScalarRule tag RPB3DescriptorProjectionData.GeneratedData.PBSint32 false) body =
        Some (RPB3SourceScalarReduction.ScalarI32 level).
Proof.
  intros tag body scalar accepted.
  cbn [RPB3IndependentOracleCore.independently_decode_scalar] in accepted.
  rewrite RPB3IndependentOracleProcessScalarDecoderBridge.full_u64_decoder_agreement in accepted.
  destruct (RPB3SourceScalarReduction.compact_varint_value body) as [number|] eqn:parsed;
    [|discriminate accepted].
  unfold RPB3IndependentOracleCore.source_numeric_scalar, RPB3IndependentOracleCore.source_numeric_nondefault in accepted.
  cbn [negb orb] in accepted.
  destruct (N.leb number 4294967295) eqn:bounded; [|discriminate accepted].
  injection accepted as same_scalar.
  exists (RPB3IndependentOracleCore.source_zigzag number).
  split; [symmetry; exact same_scalar|].
  unfold RPB3SourceScalarReduction.scalar_semantic_from_source, RPB3SourceScalarReduction.scalar_body_valid.
  cbn [RPB3SourceScalarReduction.scalar_kind RPB3SourceScalarReduction.scalar_default_forbidden].
  unfold RPB3SourceScalarReduction.numeric_scalar_valid.
  rewrite parsed.
  cbv beta iota zeta.
  rewrite bounded.
  reflexivity.
Qed.

(** ** The generated Var program *)

(** Every field of the generated Var program is either a varint scalar arm at
    tag 1 or 2, or a length-delimited message arm. *)
Definition var_field_is_classified (field : RPB3GeneratedFieldRefinement.field_descriptor) : bool :=
  match RPB3GeneratedFieldRefinement.descriptor_payload field with
  | RPB3GeneratedFieldRefinement.Scalar =>
      RPB3GeneratedFieldRefinement.wire_eqb (RPB3GeneratedFieldRefinement.descriptor_wire field) RPB3GeneratedFieldRefinement.Varint &&
      existsb (Nat.eqb (RPB3GeneratedFieldRefinement.descriptor_tag field)) [1; 2]
  | RPB3GeneratedFieldRefinement.Message =>
      RPB3GeneratedFieldRefinement.wire_eqb (RPB3GeneratedFieldRefinement.descriptor_wire field) RPB3GeneratedFieldRefinement.LengthDelimited
  | RPB3GeneratedFieldRefinement.StringMap | RPB3GeneratedFieldRefinement.PathMap => false
  end.

Definition var_fields_classified : bool :=
  match RPB3GeneratedFieldRefinement.lookup_projected_program 6 [] [] with
  | None => false
  | Some program =>
      forallb var_field_is_classified (RPB3GeneratedFieldRefinement.program_fields program)
  end.

Lemma var_fields_classified_true : var_fields_classified = true.
Proof. vm_compute. reflexivity. Qed.

Lemma generated_var_field_is_classified :
  forall program field,
    RPB3GeneratedFieldRefinement.lookup_projected_program 6 [] [] = Some program ->
    In field (RPB3GeneratedFieldRefinement.program_fields program) ->
    var_field_is_classified field = true.
Proof.
  intros program field looked_up member.
  pose proof var_fields_classified_true as certificate.
  unfold var_fields_classified in certificate.
  rewrite looked_up in certificate.
  change (forallb var_field_is_classified
    (RPB3GeneratedFieldRefinement.program_fields program) = true) in certificate.
  rewrite forallb_forall in certificate.
  exact (certificate field member).
Qed.

(** The contract of the bound and free arms: a sint32 whose default is
    allowed. *)
Lemma var_level_contract :
  forall tag,
    In tag [1; 2] ->
    RPB3IndependentOracleAdmission.source_scalar_contract_for 6 tag =
      Some (RPB3IndependentOracleAdmission.SourceScalarContract RPB3DescriptorProjectionData.GeneratedData.PBSint32 false).
Proof.
  intros tag member.
  destruct member as [same|[same|[]]]; subst tag; vm_compute; reflexivity.
Qed.

(** What the target's Var reducer observes of a decoded child.  The scalar
    case records the arm tag and the target decoder's verdict. *)
Definition var_child_total (child : RPB3IndependentOracleCore.source_exact_summary) : Prop :=
  match RPB3IndependentOracleCore.source_summary_value child with
  | RPB3IndependentOracleCore.TypedSourceScalar _ (RPB3IndependentOracleCore.SourceI32 level) =>
      RPB3GeneratedFieldRefinement.raw_wire (RPB3IndependentOracleCore.source_summary_raw child) = RPB3GeneratedFieldRefinement.Varint /\
      RPB3GeneratedFieldRefinement.raw_payload (RPB3IndependentOracleCore.source_summary_raw child) = RPB3GeneratedFieldRefinement.Scalar /\
      In (RPB3IndependentOracleCore.source_summary_tag child) [1; 2] /\
      RPB3SourceScalarReduction.scalar_semantic_from_source
        (RPB3SourceScalarReduction.ScalarRule (RPB3IndependentOracleCore.source_summary_tag child) RPB3DescriptorProjectionData.GeneratedData.PBSint32 false)
        (RPB3IndependentOracleCore.source_summary_body child) = Some (RPB3SourceScalarReduction.ScalarI32 level)
  | RPB3IndependentOracleCore.TypedSourceMessage _ _ =>
      RPB3GeneratedFieldRefinement.raw_wire (RPB3IndependentOracleCore.source_summary_raw child) =
        RPB3GeneratedFieldRefinement.LengthDelimited /\
      RPB3GeneratedFieldRefinement.raw_payload (RPB3IndependentOracleCore.source_summary_raw child) = RPB3GeneratedFieldRefinement.Message
  | _ => False
  end.

Lemma var_child_is_total :
  forall fuel hash_fn body program node child,
    RPB3GeneratedFieldRefinement.lookup_projected_program 6 [] [] = Some program ->
    aligned_child fuel hash_fn body program node child ->
    var_child_total child.
Proof.
  intros fuel hash_fn body program node child looked_up
    [[selected _] evaluated].
  pose proof (generated_var_field_is_classified program
    (RPB3RawRegionTree.source_field node) looked_up
    (source_descriptor_field_member program (RPB3RawRegionTree.source_span node)
      (RPB3RawRegionTree.source_field node) selected)) as classified.
  destruct (RPB3RawRegionTree.resolved_source_descriptor_has_actual_tag_and_wire program
    (RPB3RawRegionTree.source_span node) (RPB3RawRegionTree.source_field node) selected)
    as [field_tag field_wire].
  destruct node as [span field|span field nested|span field];
    cbn [RPB3RawRegionTree.source_span RPB3RawRegionTree.source_field]
      in classified, field_tag, field_wire, selected.
  - (* A scalar node of the Var program decodes to a level arm. *)
    destruct (evaluated_scalar_node_shape _ hash_fn body program span field
      child evaluated) as [raw [scalar [same_raw [decoded [typed shape]]]]].
    assert (raw_is : RPB3IndependentOracleCore.source_summary_raw child = raw)
      by (rewrite shape; reflexivity).
    assert (value_is : RPB3IndependentOracleCore.source_summary_value child =
        RPB3IndependentOracleCore.TypedSourceScalar raw scalar) by (rewrite shape; reflexivity).
    destruct (RPB3IndependentOracleProcessChildProvenance.converted_scalar_raw_has_scalar_shape body span
      field raw same_raw) as [raw_is_scalar [_ field_payload]].
    destruct (RPB3RawRegionTree.raw_field_of_source_preserves_wire_and_body body
      (RPB3RawRegionTree.SourceScalar span field) raw same_raw)
      as [raw_tag_is [raw_wire_is _]].
    cbn [RPB3RawRegionTree.source_span] in raw_tag_is, raw_wire_is.
    unfold var_field_is_classified in classified.
    rewrite field_payload in classified.
    cbv beta iota zeta in classified.
    apply andb_true_iff in classified as [wire_classified tag_member].
    assert (wire_varint : RPB3GeneratedFieldRefinement.raw_wire raw = RPB3GeneratedFieldRefinement.Varint).
    { rewrite raw_wire_is, (wire_eqb_true_same _ _ field_wire).
      exact (wire_eqb_true_same _ _ wire_classified). }
    assert (member : In (RPB3GeneratedFieldRefinement.raw_tag raw) [1; 2]).
    { rewrite raw_tag_is, <- field_tag.
      exact (existsb_eqb_member _ _ tag_member). }
    assert (contract_at_tag :
      RPB3IndependentOracleAdmission.source_scalar_contract_for (RPB3GeneratedFieldRefinement.program_family program)
        (RPB3GeneratedFieldRefinement.raw_tag raw) =
      Some (RPB3IndependentOracleAdmission.SourceScalarContract RPB3DescriptorProjectionData.GeneratedData.PBSint32 false)).
    { rewrite (RPB3IndependentOracleProcessScalarDecoderBridge.generated_program_has_requested_family 6 program
        looked_up).
      exact (var_level_contract _ member). }
    destruct (independent_sint32_agrees_with_target_decoder (RPB3GeneratedFieldRefinement.raw_tag raw)
      (RPB3GeneratedFieldRefinement.raw_body raw) scalar
      (scalar_value_decodes_with_contract program raw scalar RPB3DescriptorProjectionData.GeneratedData.PBSint32 false
        decoded contract_at_tag)) as [level [same_scalar semantic]].
    subst scalar.
    unfold var_child_total.
    rewrite value_is.
    change (RPB3GeneratedFieldRefinement.raw_wire (RPB3IndependentOracleCore.source_summary_raw child) = RPB3GeneratedFieldRefinement.Varint /\
      RPB3GeneratedFieldRefinement.raw_payload (RPB3IndependentOracleCore.source_summary_raw child) = RPB3GeneratedFieldRefinement.Scalar /\
      In (RPB3GeneratedFieldRefinement.raw_tag (RPB3IndependentOracleCore.source_summary_raw child)) [1; 2] /\
      RPB3SourceScalarReduction.scalar_semantic_from_source
        (RPB3SourceScalarReduction.ScalarRule (RPB3GeneratedFieldRefinement.raw_tag (RPB3IndependentOracleCore.source_summary_raw child))
          RPB3DescriptorProjectionData.GeneratedData.PBSint32 false)
        (RPB3GeneratedFieldRefinement.raw_body (RPB3IndependentOracleCore.source_summary_raw child)) =
        Some (RPB3SourceScalarReduction.ScalarI32 level)).
    rewrite raw_is.
    exact (conj wire_varint (conj raw_is_scalar (conj member semantic))).
  - (* A message node carries a length-delimited, typed message raw. *)
    destruct
      (RPB3IndependentOracleProcessChildProvenance.accepted_generated_message_node_has_descriptor_provenance
        fuel hash_fn body 6 program span field nested child looked_up selected
        evaluated)
      as [raw [grandchildren [child_type
        [_ [typed_message [raw_is [_ [wire_length [raw_message _]]]]]]]]].
    unfold var_child_total.
    rewrite typed_message.
    change (RPB3GeneratedFieldRefinement.raw_wire (RPB3IndependentOracleCore.source_summary_raw child) =
        RPB3GeneratedFieldRefinement.LengthDelimited /\
      RPB3GeneratedFieldRefinement.raw_payload (RPB3IndependentOracleCore.source_summary_raw child) = RPB3GeneratedFieldRefinement.Message).
    rewrite raw_is.
    exact (conj wire_length raw_message).
  - (* The classified program has no auxiliary payload kinds. *)
    exfalso.
    pose proof (evaluated_node_retains_source_raw fuel hash_fn body program
      (RPB3RawRegionTree.SourceAuxiliary span field) child evaluated) as converted.
    unfold RPB3RawRegionTree.raw_field_of_source in converted.
    cbn [RPB3RawRegionTree.source_span RPB3RawRegionTree.source_field] in converted.
    unfold var_field_is_classified in classified.
    destruct (RPB3GeneratedFieldRefinement.descriptor_payload field);
      [discriminate converted|discriminate converted
      |discriminate classified|discriminate classified].
Qed.

Lemma var_children_are_total :
  forall fuel hash_fn body program nodes children,
    RPB3GeneratedFieldRefinement.lookup_projected_program 6 [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    Forall var_child_total children.
Proof.
  intros fuel hash_fn body program nodes children looked_up aligned.
  eapply forall2_right_forall; [|exact aligned].
  intros node child pair.
  exact (var_child_is_total fuel hash_fn body program node child
    looked_up pair).
Qed.

(** ** The score equation on a child the target can observe *)

Lemma var_single_child_projects :
  forall child,
    var_child_total child ->
    exact_var_score (map RPB3IndependentOracleProjection.project_source_summary [child]) =
    option_map RPB3IndependentOracleCore.project_score (source_var_score [child]).
Proof.
  intros child facts.
  unfold exact_var_score, exact_var_level_score, source_var_score.
  cbn [map].
  change (RPB3SemanticReducer.sem_tag (RPB3IndependentOracleProjection.project_source_summary child))
    with (RPB3IndependentOracleCore.source_summary_tag child).
  change (RPB3SemanticReducer.sem_wire (RPB3IndependentOracleProjection.project_source_summary child))
    with (RPB3GeneratedFieldRefinement.raw_wire (RPB3IndependentOracleCore.source_summary_raw child)).
  change (RPB3SemanticReducer.sem_payload (RPB3IndependentOracleProjection.project_source_summary child))
    with (RPB3GeneratedFieldRefinement.raw_payload (RPB3IndependentOracleCore.source_summary_raw child)).
  change (RPB3SemanticReducer.sem_body (RPB3IndependentOracleProjection.project_source_summary child))
    with (RPB3IndependentOracleCore.source_summary_body child).
  change (RPB3SemanticReducer.sem_definitely_refused
      (RPB3IndependentOracleProjection.project_source_summary child))
    with (RPB3IndependentOracleCore.source_summary_refused child).
  change (RPB3SemanticReducer.sem_type_id (RPB3IndependentOracleProjection.project_source_summary child))
    with (match RPB3IndependentOracleCore.source_summary_type_id child with
      | Some type_id => type_id | None => 0 end).
  unfold var_child_total in facts.
  destruct (RPB3IndependentOracleCore.source_summary_refused child) eqn:refused.
  - (* The target tests the wire and payload before the refusal bit. *)
    destruct (RPB3GeneratedFieldRefinement.raw_wire (RPB3IndependentOracleCore.source_summary_raw child));
      destruct (RPB3GeneratedFieldRefinement.raw_payload (RPB3IndependentOracleCore.source_summary_raw child));
      reflexivity.
  - destruct (RPB3IndependentOracleCore.source_summary_value child) as
      [raw scalar|raw msgs|raw key value|raw snapshot values]
      eqn:source_value.
    + destruct scalar as [value|level|value|value|value|bytes|bytes];
        try (exfalso; exact facts).
      cbv beta iota zeta in facts.
      destruct facts as [wire [payload [member decoder]]].
      rewrite wire, payload, decoder.
      destruct member as [tag_is_one|[tag_is_two|[]]].
      * rewrite <- tag_is_one. reflexivity.
      * rewrite <- tag_is_two. reflexivity.
    + cbv beta iota zeta in facts.
      destruct facts as [wire payload].
      rewrite wire, payload.
      destruct (RPB3IndependentOracleCore.source_summary_type_id child) as [observed|];
        cbv beta iota zeta;
        [destruct (observed =? 7)|];
        destruct (RPB3IndependentOracleCore.source_summary_tag child =? 3);
        reflexivity.
    + exfalso. exact facts.
    + exfalso. exact facts.
Qed.

Lemma var_score_projects_on_total_children :
  forall children,
    Forall var_child_total children ->
    exact_var_score (map RPB3IndependentOracleProjection.project_source_summary children) =
    option_map RPB3IndependentOracleCore.project_score (source_var_score children).
Proof.
  intros [|child [|second rest]] total.
  - reflexivity.
  - exact (var_single_child_projects child (Forall_inv total)).
  - reflexivity.
Qed.

(** ** The bridge theorem *)

Theorem var_score_projects :
  forall fuel hash_fn program body nodes children,
    RPB3GeneratedFieldRefinement.lookup_projected_program 6 [] [] = Some program ->
    RPB3RawRegionTree.decode_region (S (length body)) body program 0 (length body) =
      Some nodes ->
    RPB3RawRegionTree.sequence_source_results
      (map (RPB3IndependentOracleRecursive.source_evaluate_node
             (RPB3IndependentOracleRecursive.source_evaluate_message fuel hash_fn) hash_fn body program)
        nodes)
      = Some children ->
    RPB3IndependentOracleAdmission.source_scan_valid
      (RPB3IndependentOracleAdmission.source_scan_children program children) = true ->
    exact_var_score (map RPB3IndependentOracleProjection.project_source_summary children) =
      option_map RPB3IndependentOracleCore.project_score (source_var_score children) /\
    expr_payload_completeness 6
      (fold_left (RPB3SemanticReducer.scan_child program)
        (map RPB3IndependentOracleProjection.project_source_summary children) RPB3GeneratedFieldRefinement.empty_scan)
      (map RPB3IndependentOracleProjection.project_source_summary children) =
    source_expr_payload_completeness 6
      (RPB3IndependentOracleAdmission.source_scan_children program children) children.
Proof.
  intros fuel hash_fn program body nodes children looked_up decoded selected _.
  split.
  - apply var_score_projects_on_total_children.
    exact (var_children_are_total fuel hash_fn body program nodes children
      looked_up (decoded_selection_is_aligned fuel hash_fn body program nodes
        children decoded selected)).
  - reflexivity.
Qed.

(** ** Red control: swapping the bound and free constructors

    The mutant differs from [exact_var_score] only in the two level arms.  The
    witness is the generated decoder and evaluator run on a concrete body, so
    every premise of the bridge holds of it by computation.  The body is the
    key byte [(1 << 3) | 0 = 8] and the varint [6], whose zigzag reading is
    the bound level [3].  [fuel] 1 suffices since the witness has no message
    child. *)

Definition swapped_bound_free_var_score
    (children : list RPB3SemanticReducer.semantic_summary)
    : option RPB3SemanticReducer.canonical_score :=
  match children with
  | [] => Some (RPB3SemanticReducer.ScoreInteger 0)
  | [child] =>
      match RPB3SemanticReducer.sem_wire child, RPB3SemanticReducer.sem_payload child,
          RPB3SemanticReducer.sem_definitely_refused child with
      | RPB3GeneratedFieldRefinement.Varint, RPB3GeneratedFieldRefinement.Scalar, false =>
          if RPB3SemanticReducer.sem_tag child =? 1
          then exact_var_level_score 51 child
          else if RPB3SemanticReducer.sem_tag child =? 2
          then exact_var_level_score 50 child
          else None
      | RPB3GeneratedFieldRefinement.LengthDelimited, RPB3GeneratedFieldRefinement.Message, false =>
          if (RPB3SemanticReducer.sem_tag child =? 3) && (RPB3SemanticReducer.sem_type_id child =? 7)
          then Some (RPB3SemanticReducer.score_constructor 52 [])
          else None
      | _, _, _ => None
      end
  | _ => None
  end.

Definition witness_hash_fn (_ : list nat) : nat := 0.

Definition witness_program : RPB3GeneratedFieldRefinement.generated_program :=
  match RPB3GeneratedFieldRefinement.lookup_projected_program 6 [] [] with
  | Some program => program
  | None => RPB3GeneratedFieldRefinement.empty_program
  end.

Definition bound_var_body : list nat := [8; 6].

Definition witness_nodes : list RPB3RawRegionTree.source_tree :=
  match RPB3RawRegionTree.decode_region (S (length bound_var_body)) bound_var_body
      witness_program 0 (length bound_var_body) with
  | Some nodes => nodes
  | None => []
  end.

Definition witness_evaluation : option (list RPB3IndependentOracleCore.source_exact_summary) :=
  RPB3RawRegionTree.sequence_source_results
    (map (RPB3IndependentOracleRecursive.source_evaluate_node
           (RPB3IndependentOracleRecursive.source_evaluate_message 1 witness_hash_fn) witness_hash_fn
           bound_var_body witness_program)
      witness_nodes).

Definition witness_children : list RPB3IndependentOracleCore.source_exact_summary :=
  match witness_evaluation with
  | Some children => children
  | None => []
  end.

Lemma witness_program_is_looked_up :
  RPB3GeneratedFieldRefinement.lookup_projected_program 6 [] [] = Some witness_program.
Proof. vm_compute. reflexivity. Qed.

Lemma witness_nodes_are_decoded :
  RPB3RawRegionTree.decode_region (S (length bound_var_body)) bound_var_body
    witness_program 0 (length bound_var_body) = Some witness_nodes.
Proof. vm_compute. reflexivity. Qed.

Lemma witness_children_are_selected :
  RPB3RawRegionTree.sequence_source_results
    (map (RPB3IndependentOracleRecursive.source_evaluate_node
           (RPB3IndependentOracleRecursive.source_evaluate_message 1 witness_hash_fn) witness_hash_fn
           bound_var_body witness_program)
      witness_nodes) = Some witness_children.
Proof. vm_compute. reflexivity. Qed.

Lemma witness_scan_is_valid :
  RPB3IndependentOracleAdmission.source_scan_valid
    (RPB3IndependentOracleAdmission.source_scan_children witness_program witness_children) = true.
Proof. vm_compute. reflexivity. Qed.

Example witness_child_is_the_bound_level :
  map RPB3IndependentOracleCore.source_summary_value witness_children =
    [RPB3IndependentOracleCore.TypedSourceScalar
      (RPB3GeneratedFieldRefinement.RawField 1 RPB3GeneratedFieldRefinement.Varint RPB3GeneratedFieldRefinement.Scalar [6] 1 None [] [])
      (RPB3IndependentOracleCore.SourceI32 3%Z)].
Proof. vm_compute. reflexivity. Qed.

(** The mutant and the projected oracle disagree on the witness, while the
    real target agrees with the oracle. *)
Example swapped_bound_free_disagrees_with_oracle :
  swapped_bound_free_var_score
    (map RPB3IndependentOracleProjection.project_source_summary witness_children) =
    Some (RPB3SemanticReducer.score_constructor 51 [RPB3SemanticReducer.ScoreInteger 3]) /\
  option_map RPB3IndependentOracleCore.project_score (source_var_score witness_children) =
    Some (RPB3SemanticReducer.score_constructor 50 [RPB3SemanticReducer.ScoreInteger 3]) /\
  exact_var_score (map RPB3IndependentOracleProjection.project_source_summary witness_children) =
    Some (RPB3SemanticReducer.score_constructor 50 [RPB3SemanticReducer.ScoreInteger 3]) /\
  swapped_bound_free_var_score
    (map RPB3IndependentOracleProjection.project_source_summary witness_children) <>
  option_map RPB3IndependentOracleCore.project_score (source_var_score witness_children).
Proof.
  split; [|split; [|split]].
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. intro same. discriminate same.
Qed.

(** Hence the bridge statement is false of the mutant. *)
Theorem swapped_bound_free_bridge_is_false :
  ~ (forall fuel hash_fn program body nodes children,
      RPB3GeneratedFieldRefinement.lookup_projected_program 6 [] [] = Some program ->
      RPB3RawRegionTree.decode_region (S (length body)) body program 0 (length body) =
        Some nodes ->
      RPB3RawRegionTree.sequence_source_results
        (map (RPB3IndependentOracleRecursive.source_evaluate_node
               (RPB3IndependentOracleRecursive.source_evaluate_message fuel hash_fn) hash_fn body
               program)
          nodes)
        = Some children ->
      RPB3IndependentOracleAdmission.source_scan_valid
        (RPB3IndependentOracleAdmission.source_scan_children program children) = true ->
      swapped_bound_free_var_score
        (map RPB3IndependentOracleProjection.project_source_summary children) =
      option_map RPB3IndependentOracleCore.project_score (source_var_score children)).
Proof.
  intro bridge.
  pose proof (bridge 1 witness_hash_fn witness_program bound_var_body
    witness_nodes witness_children witness_program_is_looked_up
    witness_nodes_are_decoded witness_children_are_selected
    witness_scan_is_valid) as disagreement.
  vm_compute in disagreement.
  discriminate disagreement.
Qed.

Print Assumptions wire_eqb_true_same.
Print Assumptions existsb_eqb_member.
Print Assumptions forall2_right_forall.
Print Assumptions source_descriptor_field_member.
Print Assumptions evaluated_node_retains_source_raw.
Print Assumptions evaluated_scalar_node_shape.
Print Assumptions scalar_value_decodes_with_contract.
Print Assumptions decoded_selection_is_aligned.
Print Assumptions independent_sint32_agrees_with_target_decoder.
Print Assumptions var_fields_classified_true.
Print Assumptions generated_var_field_is_classified.
Print Assumptions var_level_contract.
Print Assumptions var_child_is_total.
Print Assumptions var_children_are_total.
Print Assumptions var_single_child_projects.
Print Assumptions var_score_projects_on_total_children.
Print Assumptions var_score_projects.
Print Assumptions witness_program_is_looked_up.
Print Assumptions witness_nodes_are_decoded.
Print Assumptions witness_children_are_selected.
Print Assumptions witness_scan_is_valid.
Print Assumptions witness_child_is_the_bound_level.
Print Assumptions swapped_bound_free_disagrees_with_oracle.
Print Assumptions swapped_bound_free_bridge_is_false.
