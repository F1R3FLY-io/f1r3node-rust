From Stdlib Require Import List Bool Arith NArith.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement
  RPB3RawRegionTree RPB3SemanticReducer
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3IndependentOracleProcess RPB3IndependentOracleProjection
  RPB3IndependentOracleProcessProjection
  RPB3SourceScalarReduction
  RPB3IndependentOracleProcessScalarDecoderBridge
  RPB3IndependentOracleNameDecoderBridge
  RPB3AuxiliaryTypedReference
  RPB3IndependentOracleRecursive
  RPB3IndependentOracleRecursiveProjection
  RPB3RecursiveAuxiliarySourceReference.

Import ListNotations.

Module Target := RPB3SemanticReducer.
Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Process := RPB3IndependentOracleProcess.
Module Projection := RPB3IndependentOracleProjection.
Module ProcessProjection := RPB3IndependentOracleProcessProjection.
Module Scalar := RPB3SourceScalarReduction.
Module ScalarBridge := RPB3IndependentOracleProcessScalarDecoderBridge.
Module NameBridge := RPB3IndependentOracleNameDecoderBridge.
Module Auxiliary := RPB3AuxiliaryTypedReference.
Module Oracle := RPB3IndependentOracleRecursive.
Module RecursiveProjection := RPB3IndependentOracleRecursiveProjection.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.

Lemma connective_bool_scalar_decoder_refusal_equivalence :
  forall tag body,
    Core.independently_decode_scalar Data.PBBool false body = None <->
    Scalar.scalar_semantic_from_source
      (Scalar.ScalarRule tag Data.PBBool false) body = None.
Proof.
  intros tag body.
  unfold Core.independently_decode_scalar,
    Scalar.scalar_semantic_from_source, Scalar.scalar_body_valid.
  cbn [Scalar.scalar_kind Scalar.scalar_default_forbidden].
  unfold Scalar.numeric_scalar_valid.
  rewrite <- !ScalarBridge.full_u64_decoder_agreement.
  destruct (Core.independently_decoded_u64 body) as [number|];
    [|cbn; tauto].
  unfold Core.source_numeric_scalar, Core.source_numeric_nondefault.
  cbn [negb orb].
  destruct (N.leb number 1); cbn; split; intro refused;
    try reflexivity; discriminate.
Qed.

Lemma generated_process_pbint32_scalar_rule :
  forall family program tag,
    In (family, tag) ScalarBridge.process_pbint32_slots ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Scalar.generated_scalar_rule program tag =
      Some (Scalar.ScalarRule tag Data.PBInt32 true).
Proof.
  intros family program tag member looked_up.
  unfold ScalarBridge.process_pbint32_slots in member.
  destruct member as [member|[member|[member|[member|[]]]]];
    inversion member; subst family tag;
    vm_compute in looked_up; inversion looked_up; subst program;
    vm_compute; reflexivity.
Qed.

Lemma generated_connective_bool_scalar_rule :
  forall program tag,
    Schema.lookup_projected_program 55 [] [] = Some program ->
    In tag [5; 6; 7; 8; 9] ->
    Scalar.generated_scalar_rule program tag =
      Some (Scalar.ScalarRule tag Data.PBBool false).
Proof.
  intros program tag looked_up member.
  destruct member as [member|[member|[member|[member|[member|[]]]]]];
    subst tag;
    vm_compute in looked_up; inversion looked_up; subst program;
    vm_compute; reflexivity.
Qed.

Lemma generated_scalar_summary_projects_to_target_reduction :
  forall raw scalar,
    Schema.raw_payload raw = Schema.Scalar ->
    Schema.raw_program_type_id raw = None ->
    Schema.valid_raw_body raw = true ->
    Target.reduce_semantic_field raw [] =
      Projection.project_source_summary
        (Core.SourceExactSummary (Core.TypedSourceScalar raw scalar)
          (Core.SourceFamilyAnalysis None None false false false) None).
Proof.
  intros raw scalar payload no_type valid.
  unfold Projection.project_source_summary,
    Projection.project_source_type_id,
    Projection.project_source_evidence.
  cbn [Core.source_summary_raw Core.source_summary_value
    Core.source_value_raw
    Core.source_summary_analysis Core.source_family_refused
    Core.source_family_complete Core.source_family_score
    Core.source_family_bind_source_present
    Core.source_family_case_required_present].
  unfold Target.reduce_semantic_field, Target.generated_wire_program.
  rewrite payload, no_type, valid.
  reflexivity.
Qed.

Lemma scalar_semantic_none_iff_body_invalid :
  forall rule body,
    Scalar.scalar_semantic_from_source rule body = None <->
    Scalar.scalar_body_valid rule body = false.
Proof.
  intros rule body.
  destruct (Scalar.scalar_body_valid rule body) eqn:valid.
  - destruct (Scalar.accepted_scalar_body_has_semantic_value
      rule body valid) as [value semantic].
    rewrite semantic. split; discriminate.
  - rewrite (Scalar.scalar_semantic_value_is_rejected_with_invalid_body
      rule body valid).
    split; reflexivity.
Qed.

Lemma converted_scalar_raw_has_scalar_shape :
  forall bytes span field raw,
    Source.raw_field_of_source bytes (Source.SourceScalar span field) =
      Some raw ->
    Schema.raw_payload raw = Schema.Scalar /\
    Schema.raw_program_type_id raw = None /\
    Schema.descriptor_payload field = Schema.Scalar.
Proof.
  intros bytes span field raw converted.
  unfold Source.raw_field_of_source in converted.
  cbn [Source.source_span Source.source_field] in converted.
  destruct (Schema.descriptor_payload field) eqn:payload;
    cbn [Schema.payload_eqb] in converted; try discriminate.
  inversion converted; subst raw.
  repeat split; reflexivity.
Qed.

(** These score selectors depend only on the fields of an independent
    summary and its explicit projection.  They therefore hold for every
    source list, before the decoder-specific scalar and default proofs. *)
Lemma projected_process_child_at :
  forall tag children,
    find (fun semantic => Target.sem_tag semantic =? tag)
      (ProcessProjection.projected_process_children children) =
    option_map Projection.project_source_summary
      (Process.source_child_at tag children).
Proof.
  intros tag children.
  unfold ProcessProjection.projected_process_children,
    Process.source_child_at.
  rewrite RecursiveProjection.find_projected_source_values.
  reflexivity.
Qed.

Lemma projected_process_child_score_unconditional :
  forall tag children,
    Target.exact_child_score tag
      (ProcessProjection.projected_process_children children) =
    option_map Core.project_score
      (Process.source_child_score tag children).
Proof.
  intros tag children.
  unfold Target.exact_child_score, Process.source_child_score.
  rewrite projected_process_child_at.
  destruct (Process.source_child_at tag children) as [child|];
    simpl; [|reflexivity].
  unfold Core.source_summary_exact_score.
  destruct (Core.source_family_score (Core.source_summary_analysis child));
    reflexivity.
Qed.

Lemma projected_process_selected_scores_unconditional :
  forall tag children,
    Target.exact_selected_scores tag
      (ProcessProjection.projected_process_children children) =
    option_map (map Core.project_score)
      (Process.source_selected_scores tag children).
Proof.
  intros tag children.
  unfold ProcessProjection.projected_process_children.
  induction children as [|child rest IH]; simpl; [reflexivity|].
  rewrite IH.
  destruct (Process.source_selected_scores tag rest)
    as [scores|]; simpl; [|reflexivity].
  replace (Projection.Schema.raw_tag
    (Projection.Core.source_summary_raw child) =? tag)
    with (Core.source_summary_tag child =? tag)
    by reflexivity.
  destruct (Core.source_summary_tag child =? tag);
    unfold Core.source_summary_exact_score;
    destruct (Core.source_family_score (Core.source_summary_analysis child))
      as [score|];
    reflexivity.
Qed.

Lemma projected_process_optional_score_unconditional :
  forall tag children,
    Target.exact_optional_child_score tag
      (ProcessProjection.projected_process_children children) =
    option_map (option_map Core.project_score)
      (Process.source_optional_score tag children).
Proof.
  intros tag children.
  unfold Target.exact_optional_child_score,
    Process.source_optional_score.
  rewrite projected_process_child_at.
  destruct (Process.source_child_at tag children) as [child|];
    simpl; [|reflexivity].
  unfold Core.source_summary_exact_score.
  destruct (Core.source_family_score (Core.source_summary_analysis child));
    reflexivity.
Qed.

(** A successful recursive message evaluation constructs its typed message
    from the decoded children.  The result's shape comes from the source
    evaluator itself, without consulting the target semantic reducer. *)
Lemma accepted_independent_message_has_typed_value :
  forall fuel hash_fn raw summary,
    Oracle.source_evaluate_message fuel hash_fn raw = Some summary ->
    exists children,
      Core.source_summary_value summary =
        Core.TypedSourceMessage raw children /\
      Schema.raw_payload raw = Schema.Message.
Proof.
  intros [|fuel] hash_fn raw summary evaluated;
    [discriminate|].
  rewrite Oracle.source_evaluate_message_unfold in evaluated.
  unfold Oracle.source_evaluate_message_step in evaluated.
  destruct (Schema.raw_payload raw) eqn:payload;
    try discriminate.
  destruct (Schema.raw_program_type_id raw) as [type_id|];
    [|discriminate].
  destruct (Schema.lookup_projected_program type_id [] [])
    as [program|]; [|discriminate].
  destruct (Source.decode_region (S (length (Schema.raw_body raw)))
    (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)))
    as [nodes|]; [|discriminate].
  destruct (Source.sequence_source_results
    (map (Oracle.source_evaluate_node
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn
      (Schema.raw_body raw) program) nodes))
    as [children|]; [|discriminate].
  inversion evaluated; subst summary.
  exists (map Core.source_summary_value children).
  split; reflexivity.
Qed.

Lemma accepted_source_message_node_has_typed_value :
  forall fuel hash_fn bytes program span field nested summary,
    Oracle.source_evaluate_node
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn bytes
      program (Source.SourceMessage span field nested) = Some summary ->
    exists raw children,
      Source.raw_field_of_source bytes
        (Source.SourceMessage span field nested) = Some raw /\
      Core.source_summary_value summary =
        Core.TypedSourceMessage raw children /\
      Schema.raw_payload raw = Schema.Message.
Proof.
  intros fuel hash_fn bytes program span field nested summary evaluated.
  unfold Oracle.source_evaluate_node in evaluated.
  destruct (Source.raw_field_of_source bytes
    (Source.SourceMessage span field nested))
    as [raw|] eqn:converted; [|discriminate].
  destruct (accepted_independent_message_has_typed_value
    fuel hash_fn raw summary evaluated) as [children [typed payload]].
  exists raw, children.
  repeat split; assumption.
Qed.

Lemma accepted_source_scalar_node_has_typed_value :
  forall recurse hash_fn bytes program span field summary,
    Oracle.source_evaluate_node recurse hash_fn bytes program
      (Source.SourceScalar span field) = Some summary ->
    exists raw scalar,
      Source.raw_field_of_source bytes
        (Source.SourceScalar span field) = Some raw /\
      Core.source_summary_value summary =
        Core.TypedSourceScalar raw scalar /\
      Schema.raw_payload raw = Schema.Scalar.
Proof.
  intros recurse hash_fn bytes program span field summary evaluated.
  unfold Oracle.source_evaluate_node in evaluated.
  destruct (Source.raw_field_of_source bytes
    (Source.SourceScalar span field))
    as [raw|] eqn:converted; [|discriminate].
  destruct (Admission.source_typed_scalar program raw)
    as [value|] eqn:typed; [|discriminate].
  inversion evaluated; subst summary.
  unfold Admission.source_typed_scalar in typed.
  destruct (Admission.source_scalar_value_of_raw program raw)
    as [scalar|] eqn:decoded; [|discriminate].
  inversion typed; subst value.
  exists raw, scalar.
  repeat split; try reflexivity; try exact converted.
  unfold Source.raw_field_of_source in converted.
  cbn [Source.source_span Source.source_field] in converted.
  destruct (Schema.payload_eqb (Schema.descriptor_payload field)
    Schema.Scalar); [|discriminate].
  inversion converted; reflexivity.
Qed.

Definition generated_message_wires_checked : bool :=
  forallb (fun generated =>
    forallb (fun field =>
      negb (Schema.payload_eqb (Schema.descriptor_payload field)
        Schema.Message) ||
      Schema.wire_eqb (Schema.descriptor_wire field)
        Schema.LengthDelimited)
      (Schema.program_fields
        (Schema.project_generated_program generated [] [])))
    Data.generated_programs.

Lemma generated_message_wires_checked_true :
  generated_message_wires_checked = true.
Proof. vm_compute. reflexivity. Qed.

Lemma resolved_generated_field_is_member :
  forall fields tag field,
    Schema.resolve_tag fields tag = Some field -> In field fields.
Proof.
  induction fields as [|head rest IH]; intros tag field resolved;
    simpl in resolved; [discriminate|].
  destruct (Schema.descriptor_tag head =? tag);
    [inversion resolved; subst; now left|].
  right. eapply IH; exact resolved.
Qed.

Lemma generated_message_descriptor_has_length_wire :
  forall parent_type program field,
    Schema.lookup_projected_program parent_type [] [] = Some program ->
    In field (Schema.program_fields program) ->
    Schema.descriptor_payload field = Schema.Message ->
    Schema.descriptor_wire field = Schema.LengthDelimited.
Proof.
  intros parent_type program field looked_up member payload.
  unfold Schema.lookup_projected_program in looked_up.
  destruct (find (fun generated =>
      Data.program_type_id generated =? parent_type)
    Data.generated_programs) as [generated|] eqn:found;
    [|discriminate].
  inversion looked_up; subst program.
  apply find_some in found as [generated_member _].
  pose proof generated_message_wires_checked_true as checked.
  unfold generated_message_wires_checked in checked.
  rewrite forallb_forall in checked.
  specialize (checked generated generated_member).
  rewrite forallb_forall in checked.
  specialize (checked field member).
  rewrite payload in checked.
  simpl in checked.
  destruct (Schema.descriptor_wire field);
    simpl in checked; try discriminate; reflexivity.
Qed.

Lemma generated_selected_children_are_aligned :
  forall program (evaluate : Source.source_tree ->
      option Core.source_exact_summary) nodes children,
    Forall (Source.source_node_matches_generated_descriptor program) nodes ->
    Forall2 (fun node child => evaluate node = Some child) nodes children ->
    Forall2 (fun node child =>
      Source.source_node_matches_generated_descriptor program node /\
      evaluate node = Some child) nodes children.
Proof.
  intros program evaluate nodes children generated selected.
  revert generated.
  induction selected as [|node child nodes children chosen rest IH];
    intro generated;
    inversion generated as [|? ? matched tail]; subst.
  - constructor.
  - constructor.
    + now split.
    + now apply IH.
Qed.

Theorem accepted_independent_message_has_generated_children :
  forall fuel hash_fn raw summary,
    Oracle.source_evaluate_message (S fuel) hash_fn raw = Some summary ->
    exists type_id program nodes children,
      Schema.raw_payload raw = Schema.Message /\
      Schema.raw_program_type_id raw = Some type_id /\
      Schema.lookup_projected_program type_id [] [] = Some program /\
      Source.decode_region (S (length (Schema.raw_body raw)))
        (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)) =
        Some nodes /\
      summary = Oracle.source_message_summary raw program children /\
      Forall2 (fun node child =>
        Source.source_node_matches_generated_descriptor program node /\
        Oracle.source_evaluate_node
          (Oracle.source_evaluate_message fuel hash_fn) hash_fn
          (Schema.raw_body raw) program node = Some child)
        nodes children.
Proof.
  intros fuel hash_fn raw summary evaluated.
  rewrite Oracle.source_evaluate_message_unfold in evaluated.
  unfold Oracle.source_evaluate_message_step in evaluated.
  destruct (Schema.raw_payload raw) eqn:payload;
    try discriminate.
  destruct (Schema.raw_program_type_id raw) as [type_id|]
    eqn:family; [|discriminate].
  destruct (Schema.lookup_projected_program type_id [] [])
    as [program|] eqn:looked_up; [|discriminate].
  destruct (Source.decode_region (S (length (Schema.raw_body raw)))
    (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)))
    as [nodes|] eqn:decoded; [|discriminate].
  destruct (Source.sequence_source_results
    (map (Oracle.source_evaluate_node
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn
      (Schema.raw_body raw) program) nodes))
    as [children|] eqn:selected; [|discriminate].
  inversion evaluated; subst summary.
  exists type_id, program, nodes, children.
  repeat split; try assumption; try reflexivity.
  pose proof (Source.decoded_region_nodes_match_generated_source
    (S (length (Schema.raw_body raw))) (Schema.raw_body raw)
    program 0 (length (Schema.raw_body raw)) nodes decoded)
    as generated.
  pose proof (RecursiveProjection.selected_source_evaluator_outputs_are_positional
    Source.source_tree Core.source_exact_summary
    (Oracle.source_evaluate_node
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn
      (Schema.raw_body raw) program)
    nodes children selected) as positional.
  eapply generated_selected_children_are_aligned;
    [exact generated|exact positional].
Qed.

Lemma accepted_generated_message_node_has_descriptor_provenance :
  forall fuel hash_fn bytes parent_type program span field nested summary,
    Schema.lookup_projected_program parent_type [] [] = Some program ->
    Source.source_descriptor program span = Some field ->
    Oracle.source_evaluate_node
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn bytes program
      (Source.SourceMessage span field nested) = Some summary ->
    exists raw children child_type,
      Source.raw_field_of_source bytes
        (Source.SourceMessage span field nested) = Some raw /\
      Core.source_summary_value summary =
        Core.TypedSourceMessage raw children /\
      Core.source_summary_raw summary = raw /\
      Schema.raw_tag raw = Schema.descriptor_tag field /\
      Schema.raw_wire raw = Schema.LengthDelimited /\
      Schema.raw_payload raw = Schema.Message /\
      Schema.raw_program_type_id raw = Some child_type /\
      Schema.descriptor_child_type_id field = Some child_type.
Proof.
  intros fuel hash_fn bytes parent_type program span field nested
    summary looked_up resolved evaluated.
  destruct (accepted_source_message_node_has_typed_value
    fuel hash_fn bytes program span field nested summary evaluated)
    as [raw [children [converted [typed payload]]]].
  unfold Oracle.source_evaluate_node in evaluated.
  rewrite converted in evaluated.
  change (Oracle.source_evaluate_message fuel hash_fn raw = Some summary)
    in evaluated.
  destruct fuel as [|smaller]; [discriminate|].
  destruct (accepted_independent_message_has_generated_children
    smaller hash_fn raw summary evaluated)
    as [child_type [child_program [nodes [child_summaries
      [_ [raw_type _]]]]]].
  pose proof (Source.raw_field_of_source_preserves_wire_and_body
    bytes (Source.SourceMessage span field nested) raw converted)
    as [raw_tag [raw_wire [_ _]]].
  cbn [Source.source_span Source.source_field] in raw_tag, raw_wire.
  pose proof (Source.resolved_source_descriptor_has_actual_tag_and_wire
    program span field resolved) as [descriptor_tag descriptor_wire].
  assert (field_member : In field (Schema.program_fields program)).
  { unfold Source.source_descriptor in resolved.
    destruct (Schema.resolve_tag (Schema.program_fields program)
      (Source.Wire.span_number span)) as [selected|] eqn:chosen;
      [|discriminate].
    destruct (Schema.wire_eqb (Source.wire_kind_of_span span)
      (Schema.descriptor_wire selected)); [|discriminate].
    inversion resolved; subst selected.
    eapply resolved_generated_field_is_member; exact chosen. }
  assert (field_payload : Schema.descriptor_payload field =
      Schema.Message).
  { destruct (Source.raw_field_of_source_preserves_wire_and_body
      bytes (Source.SourceMessage span field nested) raw converted)
      as [_ [_ [same_payload _]]].
    change (Schema.raw_payload raw = Schema.descriptor_payload field)
      in same_payload.
    now rewrite <- same_payload. }
  pose proof (generated_message_descriptor_has_length_wire
    parent_type program field looked_up field_member field_payload)
    as length_wire.
  rewrite length_wire in descriptor_wire.
  destruct (Source.wire_kind_of_span span) eqn:actual_wire;
    simpl in descriptor_wire; try discriminate.
  exists raw, children, child_type.
  repeat split; try assumption.
  - unfold Core.source_summary_raw.
    now rewrite typed.
  - now rewrite raw_tag, <- descriptor_tag.
  - rewrite <- (Source.raw_message_field_uses_generated_child_type
      bytes span field nested raw converted).
    exact raw_type.
Qed.

Lemma accepted_generated_message_node_typed_score_projects :
  forall fuel hash_fn bytes parent_type program span field nested summary
      expected_type,
    Schema.lookup_projected_program parent_type [] [] = Some program ->
    Source.source_descriptor program span = Some field ->
    Oracle.source_evaluate_node
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn bytes program
      (Source.SourceMessage span field nested) = Some summary ->
    Target.exact_typed_message_score expected_type
      (Projection.project_source_summary summary) =
    option_map Core.project_score
      (Process.source_typed_message_score expected_type summary).
Proof.
  intros fuel hash_fn bytes parent_type program span field nested summary
    expected_type looked_up resolved evaluated.
  destruct (accepted_generated_message_node_has_descriptor_provenance
    fuel hash_fn bytes parent_type program span field nested summary
    looked_up resolved evaluated)
    as [raw [children [child_type
      [_ [typed [retained [_ [wire [payload [raw_type _]]]]]]]]]].
  unfold Target.exact_typed_message_score,
    Process.source_typed_message_score.
  change (Target.sem_type_id (Projection.project_source_summary summary))
    with (Projection.project_source_type_id
      (Core.source_summary_raw summary)).
  change (Target.sem_wire (Projection.project_source_summary summary))
    with (Schema.raw_wire (Core.source_summary_raw summary)).
  change (Target.sem_payload (Projection.project_source_summary summary))
    with (Schema.raw_payload (Core.source_summary_raw summary)).
  change (Target.sem_definitely_refused
      (Projection.project_source_summary summary))
    with (Core.source_summary_refused summary).
  change (Target.sem_score_evidence
      (Projection.project_source_summary summary))
    with (Projection.project_source_evidence
      (Core.source_summary_exact_score summary)).
  unfold Core.source_summary_type_id.
  rewrite typed, retained, wire, payload.
  unfold Projection.project_source_type_id.
  rewrite raw_type.
  simpl.
  destruct (child_type =? expected_type);
    destruct (Core.source_summary_refused summary);
    destruct (Core.source_summary_exact_score summary);
    reflexivity.
Qed.

Lemma invalid_source_scan_stays_invalid :
  forall program children state,
    Admission.source_scan_valid state = false ->
    Admission.source_scan_valid
      (fold_left (Admission.source_scan_step program) children state) =
      false.
Proof.
  intros program children.
  induction children as [|child rest IH]; intros state invalid;
    simpl; [exact invalid|].
  rewrite (Admission.source_scan_step_invalid_sticky
    program state child invalid).
  now apply IH.
Qed.

Lemma valid_source_scan_children_are_unrefused :
  forall program children state,
    Admission.source_scan_valid
      (fold_left (Admission.source_scan_step program) children state) =
      true ->
    Forall (fun child => Core.source_summary_refused child = false)
      children.
Proof.
  intros program children.
  induction children as [|child rest IH]; intros state valid;
    [constructor|].
  simpl in valid.
  set (next := Admission.source_scan_step program state child) in *.
  assert (next_valid : Admission.source_scan_valid next = true).
  { destruct (Admission.source_scan_valid next) eqn:observed;
      [reflexivity|].
    pose proof (invalid_source_scan_stays_invalid
      program rest next observed) as impossible.
    rewrite valid in impossible. discriminate. }
  assert (state_valid : Admission.source_scan_valid state = true).
  { destruct (Admission.source_scan_valid state) eqn:observed;
      [reflexivity|].
    unfold next in next_valid.
    rewrite (Admission.source_scan_step_invalid_sticky
      program state child observed) in next_valid.
    now rewrite observed in next_valid. }
  assert (child_unrefused : Core.source_summary_refused child = false).
  { destruct (Core.source_summary_refused child) eqn:observed;
      [|reflexivity].
    destruct (Admission.source_scan_step_checks_prior_refusal
      program state child observed) as [next_invalid|state_invalid].
    - unfold next in next_valid.
      rewrite next_invalid in next_valid. discriminate.
    - rewrite state_invalid in state_valid. discriminate. }
  constructor; [exact child_unrefused|].
  now apply IH with (state := next).
Qed.

Lemma accepted_source_message_children_are_unrefused :
  forall raw program children,
    Core.source_summary_refused
      (Oracle.source_message_summary raw program children) = false ->
    Forall (fun child => Core.source_summary_refused child = false)
      children.
Proof.
  intros raw program children accepted.
  assert (valid : Admission.source_scan_valid
      (Admission.source_scan_children program children) = true).
  { destruct (Admission.source_scan_valid
      (Admission.source_scan_children program children)) eqn:scan;
      [reflexivity|].
    unfold Core.source_summary_refused,
      Oracle.source_message_summary in accepted.
    cbn [Core.source_summary_analysis Core.source_family_refused]
      in accepted.
    rewrite scan in accepted.
    cbn [negb] in accepted.
    repeat rewrite orb_true_r in accepted.
    discriminate. }
  unfold Admission.source_scan_children in valid.
  eapply valid_source_scan_children_are_unrefused; exact valid.
Qed.

Lemma aligned_evaluated_children_are_selected :
  forall program (evaluate : Source.source_tree ->
      option Core.source_exact_summary) nodes children,
    Forall2 (fun node child =>
      Source.source_node_matches_generated_descriptor program node /\
      evaluate node = Some child) nodes children ->
    Source.sequence_source_results (map evaluate nodes) = Some children.
Proof.
  intros program evaluate nodes children aligned.
  induction aligned as [|node child nodes children [matched chosen]
      rest IH]; simpl; [reflexivity|].
  now rewrite chosen, IH.
Qed.

Theorem accepted_independent_message_children_project_to_recursive :
  forall fuel hash_fn raw summary par_child,
    Oracle.source_evaluate_message (S fuel) hash_fn raw = Some summary ->
    exists type_id program nodes children,
      Schema.raw_payload raw = Schema.Message /\
      Schema.raw_program_type_id raw = Some type_id /\
      Schema.lookup_projected_program type_id [] [] = Some program /\
      Source.decode_region (S (length (Schema.raw_body raw)))
        (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)) =
        Some nodes /\
      summary = Oracle.source_message_summary raw program children /\
      Forall2 (fun node child =>
        Source.source_node_matches_generated_descriptor program node /\
        Oracle.source_evaluate_node
          (Oracle.source_evaluate_message fuel hash_fn) hash_fn
          (Schema.raw_body raw) program node = Some child)
        nodes children /\
      ((forall node, In node nodes ->
        RecursiveProjection.source_target_child_verdict
          (Oracle.source_evaluate_node
            (Oracle.source_evaluate_message fuel hash_fn) hash_fn
            (Schema.raw_body raw) program node)
          (Recursive.recursive_source_reference par_child hash_fn
            (Schema.raw_body raw) program node)) ->
      Core.source_summary_refused summary = false ->
      exists target_results,
        Recursive.recursive_source_children par_child hash_fn
          (Schema.raw_body raw) program nodes = Some target_results /\
        map Recursive.recursive_result_semantic target_results =
          map Projection.project_source_summary children).
Proof.
  intros fuel hash_fn raw summary par_child evaluated.
  destruct (accepted_independent_message_has_generated_children
    fuel hash_fn raw summary evaluated)
    as [type_id [program [nodes [children
      [payload [family [looked_up [decoded [shape aligned]]]]]]]]].
  exists type_id, program, nodes, children.
  repeat split; try assumption.
  intros agree accepted.
  rewrite shape in accepted.
  pose proof (accepted_source_message_children_are_unrefused
    raw program children accepted) as unrefused.
  pose proof (aligned_evaluated_children_are_selected
    program (Oracle.source_evaluate_node
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn
      (Schema.raw_body raw) program) nodes children aligned)
    as selected.
  eapply RecursiveProjection.accepted_source_child_list_projects_to_recursive;
    [exact agree|exact selected|exact unrefused].
Qed.

Theorem refused_independent_message_child_blocks_recursive_list :
  forall fuel hash_fn raw summary par_child,
    Oracle.source_evaluate_message (S fuel) hash_fn raw = Some summary ->
    exists type_id program nodes children,
      Schema.raw_payload raw = Schema.Message /\
      Schema.raw_program_type_id raw = Some type_id /\
      Schema.lookup_projected_program type_id [] [] = Some program /\
      Source.decode_region (S (length (Schema.raw_body raw)))
        (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)) =
        Some nodes /\
      summary = Oracle.source_message_summary raw program children /\
      Forall2 (fun node child =>
        Source.source_node_matches_generated_descriptor program node /\
        Oracle.source_evaluate_node
          (Oracle.source_evaluate_message fuel hash_fn) hash_fn
          (Schema.raw_body raw) program node = Some child)
        nodes children /\
      ((forall node, In node nodes ->
        RecursiveProjection.source_target_child_verdict
          (Oracle.source_evaluate_node
            (Oracle.source_evaluate_message fuel hash_fn) hash_fn
            (Schema.raw_body raw) program node)
          (Recursive.recursive_source_reference par_child hash_fn
            (Schema.raw_body raw) program node)) ->
      forall child, In child children ->
        Core.source_summary_refused child = true ->
        Recursive.recursive_source_children par_child hash_fn
          (Schema.raw_body raw) program nodes = None).
Proof.
  intros fuel hash_fn raw summary par_child evaluated.
  destruct (accepted_independent_message_has_generated_children
    fuel hash_fn raw summary evaluated)
    as [type_id [program [nodes [children
      [payload [family [looked_up [decoded [shape aligned]]]]]]]]].
  exists type_id, program, nodes, children.
  repeat split; try assumption.
  intros agree child member refused.
  pose proof (aligned_evaluated_children_are_selected
    program (Oracle.source_evaluate_node
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn
      (Schema.raw_body raw) program) nodes children aligned)
    as selected.
  pose proof (RecursiveProjection.source_target_child_lists_project
    par_child hash_fn (Schema.raw_body raw) program nodes
    (Oracle.source_evaluate_node
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn
      (Schema.raw_body raw) program) agree) as projected.
  unfold RecursiveProjection.source_admitted_children in projected.
  destruct (Source.sequence_source_results
    (map RecursiveProjection.source_unrejected_child
      (map (Oracle.source_evaluate_node
        (Oracle.source_evaluate_message fuel hash_fn) hash_fn
        (Schema.raw_body raw) program) nodes)))
    as [admitted|] eqn:source_admitted.
  - destruct (RecursiveProjection.source_unrejected_results_recover
      (map (Oracle.source_evaluate_node
        (Oracle.source_evaluate_message fuel hash_fn) hash_fn
        (Schema.raw_body raw) program) nodes)
      admitted source_admitted) as [same all_unrefused].
    rewrite selected in same.
    inversion same; subst admitted.
    rewrite Forall_forall in all_unrefused.
    specialize (all_unrefused child member).
    rewrite refused in all_unrefused. discriminate.
  - destruct (Recursive.recursive_source_children par_child hash_fn
      (Schema.raw_body raw) program nodes) as [results|]
      eqn:target_results; [simpl in projected; discriminate|reflexivity].
Qed.

Lemma generated_scalar_descriptor_has_varint_wire :
  forall family program span field kind forbidden,
    Schema.lookup_projected_program family [] [] = Some program ->
    Source.source_descriptor program span = Some field ->
    Schema.descriptor_payload field = Schema.Scalar ->
    Scalar.generated_scalar_rule program (Source.Wire.span_number span) =
      Some (Scalar.ScalarRule (Source.Wire.span_number span)
        kind forbidden) ->
    Scalar.scalar_rule_wire kind = Some Schema.Varint ->
    Schema.descriptor_wire field = Schema.Varint.
Proof.
  intros family program span field kind forbidden looked_up selected
    scalar rule_selected kind_wire.
  pose proof (Source.resolved_source_descriptor_has_actual_tag_and_wire
    program span field selected) as [tag _].
  assert (member : In field (Schema.program_fields program)).
  { unfold Source.source_descriptor in selected.
    destruct (Schema.resolve_tag (Schema.program_fields program)
      (Source.Wire.span_number span)) as [candidate|] eqn:resolved;
      [|discriminate].
    destruct (Schema.wire_eqb (Source.wire_kind_of_span span)
      (Schema.descriptor_wire candidate)); [|discriminate].
    inversion selected; subst candidate.
    eapply resolved_generated_field_is_member; exact resolved. }
  destruct (Scalar.every_generated_scalar_descriptor_selects_a_rule
    family program field looked_up member scalar)
    as [rule [wire [found [rule_wire descriptor_wire]]]].
  rewrite tag, rule_selected in found.
  inversion found; subst rule.
  cbn [Scalar.scalar_kind] in rule_wire.
  rewrite kind_wire in rule_wire.
  inversion rule_wire; subst wire.
  destruct (Schema.descriptor_wire field);
    cbn [Schema.wire_eqb] in descriptor_wire;
    try discriminate; reflexivity.
Qed.

Lemma independent_generated_scalar_gate :
  forall bytes program span field raw family kind forbidden,
    Schema.lookup_projected_program family [] [] = Some program ->
    Source.source_descriptor program span = Some field ->
    Source.raw_field_of_source bytes (Source.SourceScalar span field) =
      Some raw ->
    Schema.descriptor_wire field = Schema.Varint ->
    Admission.source_scalar_contract_for family
      (Source.Wire.span_number span) =
      Some (Admission.SourceScalarContract kind forbidden) ->
    Admission.source_scalar_wire kind = Some Schema.Varint ->
    Admission.source_typed_scalar program raw =
      if Schema.valid_raw_body raw then
        option_map (Core.TypedSourceScalar raw)
          (Core.independently_decode_scalar kind forbidden
            (Schema.raw_body raw))
      else None.
Proof.
  intros bytes program span field raw family kind forbidden looked_up
    selected converted field_wire contract kind_wire.
  pose proof (ScalarBridge.generated_program_has_requested_family
    family program looked_up) as family_exact.
  pose proof (Source.raw_field_of_source_preserves_wire_and_body
    bytes (Source.SourceScalar span field) raw converted)
    as [raw_tag [raw_wire [_ _]]].
  cbn [Source.source_span Source.source_field] in raw_tag, raw_wire.
  pose proof (converted_scalar_raw_has_scalar_shape
    bytes span field raw converted)
    as [raw_payload [no_type field_payload]].
  pose proof (Source.resolved_source_descriptor_has_actual_tag_and_wire
    program span field selected) as [_ span_wire_matches].
  rewrite field_wire in span_wire_matches.
  destruct (Source.wire_kind_of_span span) eqn:span_wire;
    cbn [Schema.wire_eqb] in span_wire_matches;
    try discriminate.
  assert (raw_wire_varint : Schema.raw_wire raw = Schema.Varint).
  { now rewrite raw_wire. }
  unfold Source.source_descriptor in selected.
  destruct (Schema.resolve_tag (Schema.program_fields program)
    (Source.Wire.span_number span)) as [candidate|] eqn:resolved;
    [|discriminate].
  destruct (Schema.wire_eqb (Source.wire_kind_of_span span)
    (Schema.descriptor_wire candidate)); [|discriminate].
  inversion selected; subst candidate.
  unfold Admission.source_typed_scalar,
    Admission.source_scalar_value_of_raw.
  rewrite raw_payload, no_type, raw_tag, resolved, family_exact, contract.
  cbn [Admission.source_contract_kind
    Admission.source_contract_default_forbidden].
  rewrite kind_wire, raw_wire_varint, field_wire, field_payload.
  cbn [Schema.wire_eqb Schema.payload_eqb andb].
  change (Admission.source_raw_bytes_well_formed raw)
    with (Schema.valid_raw_body raw).
  destruct (Schema.valid_raw_body raw); reflexivity.
Qed.

Lemma recursive_generated_scalar_gate :
  forall bytes program span field raw kind forbidden,
    Source.source_descriptor program span = Some field ->
    Source.raw_field_of_source bytes (Source.SourceScalar span field) =
      Some raw ->
    Scalar.generated_scalar_rule program (Source.Wire.span_number span) =
      Some (Scalar.ScalarRule (Source.Wire.span_number span)
        kind forbidden) ->
    Recursive.checked_generated_scalar_raw bytes program
      (Source.SourceScalar span field) =
      if Schema.valid_raw_body raw &&
          Scalar.scalar_body_valid
            (Scalar.ScalarRule (Source.Wire.span_number span)
              kind forbidden) (Schema.raw_body raw)
      then Some raw else None.
Proof.
  intros bytes program span field raw kind forbidden selected converted rule.
  pose proof (converted_scalar_raw_has_scalar_shape
    bytes span field raw converted) as [_ [_ scalar_payload]].
  unfold Recursive.checked_generated_scalar_raw,
    Recursive.checked_generated_raw, Scalar.source_scalar_raw.
  cbn [Source.source_span Source.source_field].
  rewrite selected, converted, rule, scalar_payload.
  cbn [Schema.payload_eqb andb].
  destruct (Auxiliary.descriptor_eq_dec field field) as [_|different];
    [|contradiction].
  destruct (Schema.valid_raw_body raw),
    (Scalar.scalar_body_valid
      (Scalar.ScalarRule (Source.Wire.span_number span)
        kind forbidden) (Schema.raw_body raw));
    reflexivity.
Qed.

Lemma selected_generated_scalar_node_has_child_verdict :
  forall recurse par_child hash_fn bytes family program span field raw
      kind forbidden,
    Schema.lookup_projected_program family [] [] = Some program ->
    Source.source_descriptor program span = Some field ->
    Source.raw_field_of_source bytes (Source.SourceScalar span field) =
      Some raw ->
    Admission.source_scalar_contract_for family
      (Source.Wire.span_number span) =
      Some (Admission.SourceScalarContract kind forbidden) ->
    Scalar.generated_scalar_rule program (Source.Wire.span_number span) =
      Some (Scalar.ScalarRule (Source.Wire.span_number span)
        kind forbidden) ->
    Admission.source_scalar_wire kind = Some Schema.Varint ->
    Scalar.scalar_rule_wire kind = Some Schema.Varint ->
    (Core.independently_decode_scalar kind forbidden
       (Schema.raw_body raw) = None <->
     Scalar.scalar_body_valid
       (Scalar.ScalarRule (Source.Wire.span_number span)
         kind forbidden) (Schema.raw_body raw) = false) ->
    RecursiveProjection.source_target_child_verdict
      (Oracle.source_evaluate_node recurse hash_fn bytes program
        (Source.SourceScalar span field))
      (Recursive.recursive_source_reference par_child hash_fn bytes
        program (Source.SourceScalar span field)).
Proof.
  intros recurse par_child hash_fn bytes family program span field raw
    kind forbidden looked_up selected converted contract rule
    admission_wire rule_wire decoder_refusal.
  pose proof (converted_scalar_raw_has_scalar_shape
    bytes span field raw converted) as [payload [no_type scalar_payload]].
  pose proof (generated_scalar_descriptor_has_varint_wire
    family program span field kind forbidden looked_up selected
    scalar_payload rule rule_wire) as field_wire.
  pose proof (independent_generated_scalar_gate
    bytes program span field raw family kind forbidden looked_up
    selected converted field_wire contract admission_wire) as source_gate.
  pose proof (recursive_generated_scalar_gate
    bytes program span field raw kind forbidden selected converted rule)
    as target_gate.
  unfold Oracle.source_evaluate_node.
  rewrite converted, source_gate.
  cbn [Recursive.recursive_source_reference].
  rewrite target_gate.
  destruct (Schema.valid_raw_body raw) eqn:valid;
    cbn [andb option_map]; [|exact I].
  destruct (Scalar.scalar_body_valid
    (Scalar.ScalarRule (Source.Wire.span_number span)
      kind forbidden) (Schema.raw_body raw)) eqn:grammar.
  - destruct (Core.independently_decode_scalar kind forbidden
      (Schema.raw_body raw)) as [scalar|] eqn:decoded.
    + rewrite (generated_scalar_summary_projects_to_target_reduction
        raw scalar payload no_type valid).
      simpl. split; reflexivity.
    + pose proof (proj1 decoder_refusal eq_refl) as invalid.
      discriminate invalid.
  - pose proof (proj2 decoder_refusal eq_refl) as decoded.
    rewrite decoded. exact I.
Qed.

Theorem generated_process_pbint32_scalar_child_verdict :
  forall recurse par_child hash_fn bytes family program span field,
    In (family, Source.Wire.span_number span)
      ScalarBridge.process_pbint32_slots ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Source.source_descriptor program span = Some field ->
    RecursiveProjection.source_target_child_verdict
      (Oracle.source_evaluate_node recurse hash_fn bytes program
        (Source.SourceScalar span field))
      (Recursive.recursive_source_reference par_child hash_fn bytes
        program (Source.SourceScalar span field)).
Proof.
  intros recurse par_child hash_fn bytes family program span field
    member looked_up selected.
  destruct (Source.raw_field_of_source bytes
    (Source.SourceScalar span field)) as [raw|] eqn:converted.
  - eapply (selected_generated_scalar_node_has_child_verdict
      recurse par_child hash_fn bytes family program span field raw
      Data.PBInt32 true).
    + exact looked_up.
    + exact selected.
    + exact converted.
    + apply ScalarBridge.generated_process_pbint32_contract.
      exact member.
    + exact (generated_process_pbint32_scalar_rule family program
        (Source.Wire.span_number span) member looked_up).
    + reflexivity.
    + reflexivity.
    + split; intro refused.
      * apply (proj1 (scalar_semantic_none_iff_body_invalid
          (Scalar.ScalarRule (Source.Wire.span_number span)
            Data.PBInt32 true) (Schema.raw_body raw))).
        apply (proj1 (ScalarBridge.pbint32_scalar_decoder_refusal_equivalence
          (Source.Wire.span_number span) (Schema.raw_body raw))).
        exact refused.
      * apply (proj2 (ScalarBridge.pbint32_scalar_decoder_refusal_equivalence
          (Source.Wire.span_number span) (Schema.raw_body raw))).
        apply (proj2 (scalar_semantic_none_iff_body_invalid
          (Scalar.ScalarRule (Source.Wire.span_number span)
            Data.PBInt32 true) (Schema.raw_body raw))).
        exact refused.
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

Theorem generated_connective_bool_scalar_child_verdict :
  forall recurse par_child hash_fn bytes program span field,
    In (Source.Wire.span_number span) [5; 6; 7; 8; 9] ->
    Schema.lookup_projected_program 55 [] [] = Some program ->
    Source.source_descriptor program span = Some field ->
    RecursiveProjection.source_target_child_verdict
      (Oracle.source_evaluate_node recurse hash_fn bytes program
        (Source.SourceScalar span field))
      (Recursive.recursive_source_reference par_child hash_fn bytes
        program (Source.SourceScalar span field)).
Proof.
  intros recurse par_child hash_fn bytes program span field
    member looked_up selected.
  destruct (Source.raw_field_of_source bytes
    (Source.SourceScalar span field)) as [raw|] eqn:converted.
  - eapply (selected_generated_scalar_node_has_child_verdict
      recurse par_child hash_fn bytes 55 program span field raw
      Data.PBBool false).
    + exact looked_up.
    + exact selected.
    + exact converted.
    + apply NameBridge.generated_connective_bool_contract.
      exact member.
    + exact (generated_connective_bool_scalar_rule program
        (Source.Wire.span_number span) looked_up member).
    + reflexivity.
    + reflexivity.
    + split; intro refused.
      * apply (proj1 (scalar_semantic_none_iff_body_invalid
          (Scalar.ScalarRule (Source.Wire.span_number span)
            Data.PBBool false) (Schema.raw_body raw))).
        apply (proj1 (connective_bool_scalar_decoder_refusal_equivalence
          (Source.Wire.span_number span) (Schema.raw_body raw))).
        exact refused.
      * apply (proj2 (connective_bool_scalar_decoder_refusal_equivalence
          (Source.Wire.span_number span) (Schema.raw_body raw))).
        apply (proj2 (scalar_semantic_none_iff_body_invalid
          (Scalar.ScalarRule (Source.Wire.span_number span)
            Data.PBBool false) (Schema.raw_body raw))).
        exact refused.
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

Print Assumptions connective_bool_scalar_decoder_refusal_equivalence.
Print Assumptions generated_process_pbint32_scalar_rule.
Print Assumptions generated_connective_bool_scalar_rule.
Print Assumptions generated_scalar_summary_projects_to_target_reduction.
Print Assumptions generated_process_pbint32_scalar_child_verdict.
Print Assumptions generated_connective_bool_scalar_child_verdict.
Print Assumptions projected_process_child_at.
Print Assumptions projected_process_child_score_unconditional.
Print Assumptions projected_process_selected_scores_unconditional.
Print Assumptions projected_process_optional_score_unconditional.
Print Assumptions accepted_independent_message_has_typed_value.
Print Assumptions accepted_source_message_node_has_typed_value.
Print Assumptions accepted_source_scalar_node_has_typed_value.
Print Assumptions generated_message_wires_checked_true.
Print Assumptions generated_message_descriptor_has_length_wire.
Print Assumptions accepted_independent_message_has_generated_children.
Print Assumptions accepted_generated_message_node_has_descriptor_provenance.
Print Assumptions accepted_generated_message_node_typed_score_projects.
Print Assumptions accepted_independent_message_children_project_to_recursive.
Print Assumptions refused_independent_message_child_blocks_recursive_list.
