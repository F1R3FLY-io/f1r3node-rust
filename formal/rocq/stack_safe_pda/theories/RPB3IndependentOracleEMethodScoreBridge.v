(** * Bridge for the EMethod payload score, type 33.

    On the children that the oracle actually produces for a decoded EMethod,
    the target helper [exact_emethod_score] agrees with the projection of the
    oracle helper [source_emethod_score], and the target's completeness agrees
    with the oracle's: [Some (seen_slot state 1)] on both sides.

    The score is constructor 115 over [ScoreString] of the method name (tag 1,
    empty when absent), the target (tag 2, type 1), the arguments (tag 3,
    type 1) and the connective flag (tag 6).  The proof reuses the reader
    agreements of the sibling collection bridge, restated here because each
    bridge is a self-contained theory.  The method-name reader is the one
    reader this family adds: a present tag-1 child is the default-forbidden
    string, so its value is [SourceText] of its body, on a length-delimited
    scalar, which is exactly what the target's reader demands.

    The red control at the end shows that reading the flag from tag 5 instead of
    tag 6 breaks the bridge. *)

From Stdlib Require Import List Bool Arith NArith ZArith Lia.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree RPB3SemanticReducer
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3IndependentOracleProjection RPB3IndependentOracleRecursive
  RPB3IndependentOracleRecursiveProjection
  RPB3IndependentOracleProcessChildProvenance
  RPB3IndependentOracleNameFamilyBridge
  RPB3IndependentOracleProcessTypedChildren
  RPB3SemanticReducer RPB3IndependentOracleExprPayload.

Import ListNotations.

(** ** Readers on one actual child *)

Lemma payload_typed_message_score_is_process :
  forall type_id child,
    source_payload_typed_message_score type_id child =
    Process.source_typed_message_score type_id child.
Proof. reflexivity. Qed.

Lemma aligned_child_typed_message_score_projects :
  forall fuel hash_fn family program body node child type_id,
    Schema.lookup_projected_program family [] [] = Some program ->
    aligned_child fuel hash_fn body program node child ->
    Target.exact_typed_message_score type_id
      (Projection.project_source_summary child) =
    option_map Core.project_score
      (source_payload_typed_message_score type_id child).
Proof.
  intros fuel hash_fn family program body node child type_id looked_up
    [matched evaluated].
  rewrite payload_typed_message_score_is_process.
  destruct node as [span field|span field nested|span field].
  - destruct (accepted_source_scalar_node_has_typed_value
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program
      span field child evaluated) as [raw [scalar [_ [typed payload]]]].
    destruct (typed_scalar_child_is_not_message child raw scalar typed payload)
      as [not_message not_value].
    exact (non_message_child_typed_score_projects child type_id not_message
      not_value).
  - unfold Source.source_node_matches_generated_descriptor in matched.
    destruct matched as [resolved _].
    cbn [Source.source_span Source.source_field] in resolved.
    exact (accepted_generated_message_node_typed_score_projects
      fuel hash_fn body family program span field nested child type_id
      looked_up resolved evaluated).
  - destruct (evaluated_auxiliary_node_is_not_message
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program
      span field child evaluated) as [not_message not_value].
    exact (non_message_child_typed_score_projects child type_id not_message
      not_value).
Qed.

Lemma aligned_child_of_member :
  forall fuel hash_fn body program nodes children child,
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    In child children ->
    exists node, aligned_child fuel hash_fn body program node child.
Proof.
  intros fuel hash_fn body program nodes children child aligned.
  induction aligned as [|node head nodes' children' pair rest IH].
  - intros [].
  - intros [same|later].
    + subst head. exists node. exact pair.
    + exact (IH later).
Qed.

(** ** Readers on the list of actual children *)

Lemma payload_typed_child_score_projects :
  forall fuel hash_fn family program body nodes children tag type_id,
    Schema.lookup_projected_program family [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    exact_typed_child_score tag type_id
      (map Projection.project_source_summary children) =
    option_map Core.project_score
      (source_payload_typed_child_score tag type_id children).
Proof.
  intros fuel hash_fn family program body nodes children tag type_id looked_up
    aligned.
  unfold exact_typed_child_score, source_payload_typed_child_score,
    source_payload_child_at.
  rewrite <- Projection.projected_find_tag.
  destruct (find (fun child => Core.source_summary_tag child =? tag) children)
    as [child|] eqn:found; cbn [option_map]; [|reflexivity].
  apply find_some in found as [member _].
  destruct (aligned_child_of_member fuel hash_fn body program nodes children
    child aligned member) as [node pair].
  exact (aligned_child_typed_message_score_projects fuel hash_fn family program
    body node child type_id looked_up pair).
Qed.

Lemma payload_typed_selected_scores_project :
  forall fuel hash_fn family program body nodes children tag type_id,
    Schema.lookup_projected_program family [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    exact_typed_selected_scores tag type_id
      (map Projection.project_source_summary children) =
    option_map (map Core.project_score)
      (source_payload_typed_selected_scores tag type_id children).
Proof.
  intros fuel hash_fn family program body nodes children tag type_id looked_up
    aligned.
  induction aligned as [|node child nodes' children' pair rest IH];
    [reflexivity|].
  cbn [map exact_typed_selected_scores source_payload_typed_selected_scores].
  rewrite IH.
  destruct (source_payload_typed_selected_scores tag type_id children')
    as [scores|]; cbn [option_map]; [|reflexivity].
  change (Target.sem_tag (Projection.project_source_summary child))
    with (Core.source_summary_tag child).
  destruct (Core.source_summary_tag child =? tag); [|reflexivity].
  rewrite (aligned_child_typed_message_score_projects fuel hash_fn family
    program body node child type_id looked_up pair).
  destruct (source_payload_typed_message_score type_id child); reflexivity.
Qed.

Lemma payload_bool_or_absent_projects :
  forall tag children,
    Forall (connective_flag_child_true tag) children ->
    Forall (fun child => Core.source_summary_refused child = false) children ->
    scalar_true_or_absent tag
      (map Projection.project_source_summary children) =
    source_payload_bool_or_absent tag children.
Proof.
  intros tag children flags unrefused.
  rewrite Forall_forall in flags, unrefused.
  unfold scalar_true_or_absent, source_payload_bool_or_absent,
    source_payload_child_at.
  rewrite <- Projection.projected_find_tag.
  destruct (find (fun child => Core.source_summary_tag child =? tag) children)
    as [child|] eqn:found; [|reflexivity].
  apply find_some in found as [member same_tag].
  apply Nat.eqb_eq in same_tag.
  destruct (flags child member same_tag) as [value [payload body]].
  cbn [option_map].
  change (sem_payload (Projection.project_source_summary child))
    with (Schema.raw_payload (Core.source_summary_raw child)).
  change (sem_body (Projection.project_source_summary child))
    with (Schema.raw_body (Core.source_summary_raw child)).
  rewrite payload, body, value, (unrefused child member).
  reflexivity.
Qed.

Lemma projected_seen_slot :
  forall program children slot,
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = true ->
    Target.seen_slot
      (fold_left (Target.scan_child program)
        (map Projection.project_source_summary children) Schema.empty_scan)
      slot =
    Admission.source_seen_slot
      (Admission.source_scan_children program children) slot.
Proof.
  intros program children slot valid.
  destruct (Projection.source_scan_children_project_valid program children
    valid) as [_ [_ seen]].
  unfold Target.seen_slot, Admission.source_seen_slot.
  rewrite seen.
  reflexivity.
Qed.

(** ** The method name reader *)

(** A present tag-1 child of an EMethod is a length-delimited scalar whose
    oracle value is [SourceText] of its own body.  The value comes from the
    default-forbidden [string] decoder; the wire comes from the scalar
    contract's kind. *)
Definition method_name_child_is_text
    (child : Core.source_exact_summary) : Prop :=
  Core.source_summary_tag child = 1 ->
  Core.source_summary_value child =
    Core.TypedSourceScalar (Core.source_summary_raw child)
      (Core.SourceText (Schema.raw_body (Core.source_summary_raw child))) /\
  Schema.raw_wire (Core.source_summary_raw child) = Schema.LengthDelimited /\
  Schema.raw_payload (Core.source_summary_raw child) = Schema.Scalar.

Lemma independent_forbidden_string_is_text_body :
  forall body scalar,
    Core.independently_decode_scalar Data.PBString true body = Some scalar ->
    scalar = Core.SourceText body.
Proof.
  intros body scalar accepted.
  cbn [Core.independently_decode_scalar] in accepted.
  destruct (Core.bytes_in_range body && Source.indexed_scan_utf8_accepts body &&
    Core.source_nondefault true body); [|discriminate accepted].
  injection accepted as same_scalar.
  symmetry. exact same_scalar.
Qed.

Lemma scalar_value_has_contract_wire :
  forall program raw scalar kind forbidden wire,
    Admission.source_scalar_value_of_raw program raw = Some scalar ->
    Admission.source_scalar_contract_for (Schema.program_family program)
      (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract kind forbidden) ->
    Admission.source_scalar_wire kind = Some wire ->
    Schema.raw_wire raw = wire.
Proof.
  intros program raw scalar kind forbidden wire accepted contract kind_wire.
  unfold Admission.source_scalar_value_of_raw in accepted.
  rewrite contract in accepted.
  destruct (Schema.raw_payload raw); try discriminate accepted.
  destruct (Schema.raw_program_type_id raw); [discriminate accepted|].
  destruct (Schema.resolve_tag (Schema.program_fields program)
    (Schema.raw_tag raw)) as [descriptor|]; [|discriminate accepted].
  cbn [Admission.source_contract_kind
    Admission.source_contract_default_forbidden] in accepted.
  rewrite kind_wire in accepted.
  destruct (Admission.source_raw_bytes_well_formed raw &&
    Schema.wire_eqb (Schema.raw_wire raw) wire &&
    Schema.wire_eqb wire (Schema.descriptor_wire descriptor) &&
    Schema.payload_eqb (Schema.descriptor_payload descriptor) Schema.Scalar)
    eqn:checks; [|discriminate accepted].
  repeat rewrite andb_true_iff in checks.
  destruct checks as [[[_ wire_ok] _] _].
  exact (wire_eqb_true_same _ _ wire_ok).
Qed.

Lemma emethod_name_tag_is_scalar : flag_tag_scalar_in_family 33 1 = true.
Proof. vm_compute. reflexivity. Qed.

Lemma emethod_name_contract :
  Admission.source_scalar_contract_for 33 1 =
    Some (Admission.SourceScalarContract Data.PBString true).
Proof. vm_compute. reflexivity. Qed.

Lemma emethod_flag_tag_is_scalar : flag_tag_scalar_in_family 33 6 = true.
Proof. vm_compute. reflexivity. Qed.

Lemma emethod_flag_contract :
  Admission.source_scalar_contract_for 33 6 =
    Some (Admission.SourceScalarContract Data.PBBool true).
Proof. vm_compute. reflexivity. Qed.

Lemma aligned_method_name_child_is_text :
  forall fuel hash_fn body program node child,
    Schema.lookup_projected_program 33 [] [] = Some program ->
    aligned_child fuel hash_fn body program node child ->
    method_name_child_is_text child.
Proof.
  intros fuel hash_fn body program node child looked_up
    [[selected _] evaluated] same_tag.
  pose proof (evaluated_node_retains_source_raw fuel hash_fn body program node
    child evaluated) as converted.
  destruct (Source.raw_field_of_source_preserves_wire_and_body body node
    (Core.source_summary_raw child) converted) as [raw_tag_is _].
  destruct (Source.resolved_source_descriptor_has_actual_tag_and_wire program
    (Source.source_span node) (Source.source_field node) selected)
    as [field_tag _].
  assert (field_scalar :
    Schema.descriptor_payload (Source.source_field node) = Schema.Scalar).
  { apply (generated_flag_tag_field_is_scalar 33 1 program
      (Source.source_field node) emethod_name_tag_is_scalar looked_up).
    - exact (source_descriptor_field_member program (Source.source_span node)
        (Source.source_field node) selected).
    - rewrite field_tag, <- raw_tag_is. exact same_tag. }
  destruct node as [span field|span field nested|span field].
  - destruct (evaluated_scalar_node_shape _ hash_fn body program span field
      child evaluated) as [raw [scalar [same_raw [decoded [typed shape]]]]].
    assert (raw_is : Core.source_summary_raw child = raw)
      by (rewrite shape; reflexivity).
    assert (value_is : Core.source_summary_value child =
        Core.TypedSourceScalar raw scalar) by (rewrite shape; reflexivity).
    assert (tag_is : Schema.raw_tag raw = 1).
    { rewrite <- raw_is. exact same_tag. }
    assert (contract_at_tag :
      Admission.source_scalar_contract_for (Schema.program_family program)
        (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract Data.PBString true)).
    { rewrite (looked_up_program_family 33 program looked_up), tag_is.
      exact emethod_name_contract. }
    pose proof (independent_forbidden_string_is_text_body (Schema.raw_body raw)
      scalar (scalar_value_decodes_with_contract program raw scalar
        Data.PBString true decoded contract_at_tag)) as same_scalar.
    subst scalar.
    destruct (converted_scalar_raw_has_scalar_shape body span field raw
      same_raw) as [raw_payload _].
    pose proof (scalar_value_has_contract_wire program raw
      (Core.SourceText (Schema.raw_body raw)) Data.PBString true
      Schema.LengthDelimited decoded contract_at_tag eq_refl) as raw_wire.
    rewrite raw_is, value_is.
    exact (conj eq_refl (conj raw_wire raw_payload)).
  - exfalso.
    cbn [Source.source_field] in field_scalar.
    unfold Source.raw_field_of_source in converted.
    cbn [Source.source_span Source.source_field] in converted.
    rewrite field_scalar in converted.
    discriminate converted.
  - exfalso.
    cbn [Source.source_field] in field_scalar.
    unfold Source.raw_field_of_source in converted.
    cbn [Source.source_span Source.source_field] in converted.
    rewrite field_scalar in converted.
    discriminate converted.
Qed.

Lemma method_name_or_empty_projects :
  forall fuel hash_fn body program nodes children,
    Schema.lookup_projected_program 33 [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    Forall (fun child => Core.source_summary_refused child = false) children ->
    exact_method_name_or_empty
      (map Projection.project_source_summary children) =
    source_emethod_name_or_empty children.
Proof.
  intros fuel hash_fn body program nodes children looked_up aligned unrefused.
  rewrite Forall_forall in unrefused.
  unfold exact_method_name_or_empty, source_emethod_name_or_empty,
    source_payload_child_at.
  rewrite <- Projection.projected_find_tag.
  destruct (find (fun child => Core.source_summary_tag child =? 1) children)
    as [child|] eqn:found; [|reflexivity].
  apply find_some in found as [member same_tag].
  apply Nat.eqb_eq in same_tag.
  destruct (aligned_child_of_member fuel hash_fn body program nodes children
    child aligned member) as [node pair].
  destruct (aligned_method_name_child_is_text fuel hash_fn body program node
    child looked_up pair same_tag) as [value [wire payload]].
  cbn [option_map].
  change (sem_wire (Projection.project_source_summary child))
    with (Schema.raw_wire (Core.source_summary_raw child)).
  change (sem_payload (Projection.project_source_summary child))
    with (Schema.raw_payload (Core.source_summary_raw child)).
  change (sem_definitely_refused (Projection.project_source_summary child))
    with (Core.source_summary_refused child).
  change (sem_body (Projection.project_source_summary child))
    with (Schema.raw_body (Core.source_summary_raw child)).
  rewrite wire, payload, (unrefused child member), value.
  reflexivity.
Qed.

(** ** The EMethod score *)

Lemma emethod_score_projects :
  forall fuel hash_fn program body nodes children,
    Schema.lookup_projected_program 33 [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    Forall (fun child => Core.source_summary_refused child = false) children ->
    exact_emethod_score (map Projection.project_source_summary children) =
    option_map Core.project_score (source_emethod_score children).
Proof.
  intros fuel hash_fn program body nodes children looked_up aligned unrefused.
  assert (flags : Forall (connective_flag_child_true 6) children).
  { eapply forall2_right_forall; [|exact aligned].
    intros node child pair.
    exact (forbidden_flag_child_is_true fuel hash_fn body program 33 6
      node child looked_up emethod_flag_tag_is_scalar emethod_flag_contract
      pair). }
  pose proof (payload_bool_or_absent_projects 6 children flags unrefused)
    as flag.
  pose proof (method_name_or_empty_projects fuel hash_fn body program nodes
    children looked_up aligned unrefused) as name.
  pose proof (payload_typed_child_score_projects fuel hash_fn 33 program body
    nodes children 2 1 looked_up aligned) as target.
  pose proof (payload_typed_selected_scores_project fuel hash_fn 33 program
    body nodes children 3 1 looked_up aligned) as arguments.
  unfold exact_emethod_score, source_emethod_score.
  rewrite name, target, arguments, flag.
  destruct (source_emethod_name_or_empty children) as [method|];
    cbn [option_map]; [|reflexivity].
  destruct (source_payload_typed_child_score 2 1 children) as [target_score|];
    cbn [option_map]; [|reflexivity].
  destruct (source_payload_typed_selected_scores 3 1 children)
    as [argument_scores|]; cbn [option_map]; [|reflexivity].
  destruct (source_payload_bool_or_absent 6 children) as [connective|];
    cbn [option_map]; [|reflexivity].
  rewrite Core.project_source_score_constructor.
  cbn [map Core.project_score].
  rewrite map_app.
  cbn [map].
  unfold source_payload_bool_word.
  destruct connective; reflexivity.
Qed.

(** ** The bridge *)

Theorem emethod_payload_projects :
  forall fuel hash_fn program body nodes children,
    Schema.lookup_projected_program 33 [] [] = Some program ->
    Source.decode_region (S (length body)) body program 0 (length body) =
      Some nodes ->
    Source.sequence_source_results
      (map (Oracle.source_evaluate_node
             (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program)
        nodes) = Some children ->
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = true ->
    exact_emethod_score (map Projection.project_source_summary children) =
    option_map Core.project_score (source_emethod_score children) /\
    expr_payload_completeness 33
      (fold_left (Target.scan_child program)
        (map Projection.project_source_summary children) Schema.empty_scan)
      (map Projection.project_source_summary children) =
    source_expr_payload_completeness 33
      (Admission.source_scan_children program children) children.
Proof.
  intros fuel hash_fn program body nodes children looked_up decoded selected
    valid.
  assert (aligned :
    Forall2 (aligned_child fuel hash_fn body program) nodes children).
  { exact (generated_selected_children_are_aligned program
      (Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
        hash_fn body program) nodes children
      (Source.decoded_region_nodes_match_generated_source _ _ _ _ _ _ decoded)
      (RecursiveProjection.selected_source_evaluator_outputs_are_positional
        _ _ _ _ _ selected)). }
  assert (unrefused :
    Forall (fun child => Core.source_summary_refused child = false) children).
  { unfold Admission.source_scan_children in valid.
    exact (valid_source_scan_children_are_unrefused program children
      Admission.source_empty_scan valid). }
  split.
  - exact (emethod_score_projects fuel hash_fn program body nodes children
      looked_up aligned unrefused).
  - cbn [expr_payload_completeness source_expr_payload_completeness].
    rewrite (projected_seen_slot program children 1 valid).
    reflexivity.
Qed.

(** ** Red control: the connective flag read from tag 5

    The mutant is the target helper reading [scalar_true_or_absent] at tag 5,
    the locally_free bytes, instead of tag 6.  The witness has the target at
    tag 2 (an empty Par) and the locally_free bytes [[1]] at tag 5, so the
    mutant reads an encoded [true] that is no flag at all: the real flag at
    tag 6 is absent, reading 0. *)
Definition mutant_flag_from_tag5_emethod_score
    (children : list semantic_summary) : option canonical_score :=
  match exact_method_name_or_empty children,
      exact_typed_child_score 2 1 children,
      exact_typed_selected_scores 3 1 children,
      scalar_true_or_absent 5 children with
  | Some method, Some target, Some arguments, Some connective =>
      Some (score_constructor 115
        (ScoreString method :: target ::
          (arguments ++ [ScoreInteger (if connective then 1 else 0)])))
  | _, _, _, _ => None
  end.

Definition emethod_witness_body : list nat := [18; 0; 42; 1; 1].

Definition emethod_witness_children : list Core.source_exact_summary :=
  Correspondence.witness_children 33 emethod_witness_body.

Lemma emethod_witness_satisfies_the_hypotheses :
  Schema.lookup_projected_program 33 [] [] =
    Some (Correspondence.witness_program 33) /\
  Source.decode_region (S (length emethod_witness_body)) emethod_witness_body
    (Correspondence.witness_program 33) 0 (length emethod_witness_body) =
    Some (Correspondence.witness_nodes 33 emethod_witness_body) /\
  Source.sequence_source_results
    (map (Oracle.source_evaluate_node
           (Oracle.source_evaluate_message 1 Correspondence.witness_hash_fn)
           Correspondence.witness_hash_fn emethod_witness_body
           (Correspondence.witness_program 33))
      (Correspondence.witness_nodes 33 emethod_witness_body)) =
    Some emethod_witness_children /\
  Admission.source_scan_valid
    (Admission.source_scan_children (Correspondence.witness_program 33)
      emethod_witness_children) = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example flag_from_tag5_mutant_disagrees_with_the_oracle :
  mutant_flag_from_tag5_emethod_score
    (map Projection.project_source_summary emethod_witness_children) <>
  option_map Core.project_score (source_emethod_score emethod_witness_children).
Proof. vm_compute. discriminate. Qed.

(** The real helper agrees with the oracle on this witness and has a score:
    the disagreement is the mutant's, not an artefact of a refused child. *)
Example real_helper_agrees_on_the_red_control_witness :
  exists score,
    exact_emethod_score
      (map Projection.project_source_summary emethod_witness_children) =
      Some score /\
    option_map Core.project_score
      (source_emethod_score emethod_witness_children) = Some score.
Proof. eexists. split; vm_compute; reflexivity. Qed.

Theorem flag_from_tag5_mutant_fails_the_bridge :
  ~ (forall fuel hash_fn program body nodes children,
      Schema.lookup_projected_program 33 [] [] = Some program ->
      Source.decode_region (S (length body)) body program 0 (length body) =
        Some nodes ->
      Source.sequence_source_results
        (map (Oracle.source_evaluate_node
               (Oracle.source_evaluate_message fuel hash_fn) hash_fn body
               program) nodes) = Some children ->
      Admission.source_scan_valid
        (Admission.source_scan_children program children) = true ->
      mutant_flag_from_tag5_emethod_score
        (map Projection.project_source_summary children) =
      option_map Core.project_score (source_emethod_score children)).
Proof.
  intro bridge.
  destruct emethod_witness_satisfies_the_hypotheses
    as [looked_up [decoded [selected scan_valid]]].
  exact (flag_from_tag5_mutant_disagrees_with_the_oracle
    (bridge 1 Correspondence.witness_hash_fn
      (Correspondence.witness_program 33) emethod_witness_body
      (Correspondence.witness_nodes 33 emethod_witness_body)
      emethod_witness_children looked_up decoded selected scan_valid)).
Qed.

Print Assumptions payload_typed_message_score_is_process.
Print Assumptions aligned_child_typed_message_score_projects.
Print Assumptions aligned_child_of_member.
Print Assumptions payload_typed_child_score_projects.
Print Assumptions payload_typed_selected_scores_project.
Print Assumptions payload_bool_or_absent_projects.
Print Assumptions projected_seen_slot.
Print Assumptions independent_forbidden_string_is_text_body.
Print Assumptions scalar_value_has_contract_wire.
Print Assumptions emethod_name_tag_is_scalar.
Print Assumptions emethod_name_contract.
Print Assumptions emethod_flag_tag_is_scalar.
Print Assumptions emethod_flag_contract.
Print Assumptions aligned_method_name_child_is_text.
Print Assumptions method_name_or_empty_projects.
Print Assumptions emethod_score_projects.
Print Assumptions emethod_payload_projects.
Print Assumptions emethod_witness_satisfies_the_hypotheses.
Print Assumptions flag_from_tag5_mutant_disagrees_with_the_oracle.
Print Assumptions real_helper_agrees_on_the_red_control_witness.
Print Assumptions flag_from_tag5_mutant_fails_the_bridge.
