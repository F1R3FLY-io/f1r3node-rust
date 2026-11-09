From Stdlib Require Import List Bool Arith NArith.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement
  RPB3RawRegionTree RPB3IndependentOracleCore
  RPB3IndependentOracleRecursive RPB3IndependentOracleProcess
  RPB3SemanticReducer RPB3IndependentOracleProjection
  RPB3IndependentOracleProcessChildProvenance
  RPB3IndependentOracleExprNestedGroundScalarBridge.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Core := RPB3IndependentOracleCore.
Module Oracle := RPB3IndependentOracleRecursive.
Module Process := RPB3IndependentOracleProcess.
Module Target := RPB3SemanticReducer.
Module Projection := RPB3IndependentOracleProjection.
Module ChildProvenance := RPB3IndependentOracleProcessChildProvenance.
Module ScalarBridge := RPB3IndependentOracleExprNestedGroundScalarBridge.

(** This finite check concerns the generated Expr descriptor alone.  It does
    not evaluate a protobuf body or normalize a decoded numeric word. *)
Definition expr_ground_arm_type_sound (field : Schema.field_descriptor)
    : bool :=
  match Schema.descriptor_tag field with
  | 36 =>
      match Schema.descriptor_child_type_id field with
      | Some family => family =? 25
      | None => false
      end
  | 37 =>
      match Schema.descriptor_child_type_id field with
      | Some family => family =? 26
      | None => false
      end
  | _ => true
  end.

Lemma generated_expr_ground_arm_types :
  forall program field,
    Schema.lookup_projected_program 24 [] [] = Some program ->
    In field (Schema.program_fields program) ->
    (Schema.descriptor_tag field = 36 ->
      Schema.descriptor_child_type_id field = Some 25) /\
    (Schema.descriptor_tag field = 37 ->
      Schema.descriptor_child_type_id field = Some 26).
Proof.
  intros program field looked_up member.
  assert (checked :
      option_map (fun selected => forallb expr_ground_arm_type_sound
        (Schema.program_fields selected))
        (Schema.lookup_projected_program 24 [] []) = Some true).
  { vm_compute. reflexivity. }
  rewrite looked_up in checked.
  inversion checked as [every]; clear checked.
  pose proof (proj1 (@forallb_forall Schema.field_descriptor
    expr_ground_arm_type_sound (Schema.program_fields program)) every
    field member) as sound.
  split; intro tag.
  - unfold expr_ground_arm_type_sound in sound.
    rewrite tag in sound.
    cbn in sound.
    destruct (Schema.descriptor_child_type_id field) as [family|]
      eqn:child_type; [|discriminate].
    apply Nat.eqb_eq in sound.
    now subst family.
  - unfold expr_ground_arm_type_sound in sound.
    rewrite tag in sound.
    cbn in sound.
    destruct (Schema.descriptor_child_type_id field) as [family|]
      eqn:child_type; [|discriminate].
    apply Nat.eqb_eq in sound.
    now subst family.
Qed.

Lemma selected_generated_expr_ground_arm_type :
  forall program span field,
    Schema.lookup_projected_program 24 [] [] = Some program ->
    Source.source_descriptor program span = Some field ->
    (Schema.descriptor_tag field = 36 ->
      Schema.descriptor_child_type_id field = Some 25) /\
    (Schema.descriptor_tag field = 37 ->
      Schema.descriptor_child_type_id field = Some 26).
Proof.
  intros program span field looked_up selected.
  assert (member : In field (Schema.program_fields program)).
  { unfold Source.source_descriptor in selected.
    destruct (Schema.resolve_tag (Schema.program_fields program)
      (Source.Wire.span_number span)) as [candidate|] eqn:resolved;
      [|discriminate].
    destruct (Schema.wire_eqb (Source.wire_kind_of_span span)
      (Schema.descriptor_wire candidate)); [|discriminate].
    inversion selected; subst candidate.
    eapply ChildProvenance.resolved_generated_field_is_member;
      exact resolved. }
  eapply generated_expr_ground_arm_types; eassumption.
Qed.

(** The score algebra uses option equality only.  The later actual-child
    theorem derives those equalities from the successful source evaluation. *)
Lemma bigrat_field_values_project_parent_score :
  forall source_numerator source_denominator target_numerator
      target_denominator,
    source_numerator = target_numerator ->
    source_denominator = target_denominator ->
    match target_numerator, target_denominator with
    | Some numerator, Some denominator =>
        Some (Target.score_constructor 16
          [Target.ScoreBytes numerator; Target.ScoreBytes denominator])
    | _, _ => None
    end =
    option_map Core.project_score
      (match source_numerator, source_denominator with
       | Some numerator, Some denominator =>
           Some (Core.source_score_constructor 16
             [Core.SourceBytes numerator; Core.SourceBytes denominator])
       | _, _ => None
       end).
Proof.
  intros source_numerator source_denominator target_numerator
    target_denominator numerator denominator.
  rewrite <- numerator, <- denominator.
  destruct source_numerator as [first|];
    destruct source_denominator as [second|];
    cbn [option_map]; try reflexivity;
    now rewrite Core.project_source_score_constructor.
Qed.

Lemma fixed_point_field_values_project_parent_score :
  forall source_unscaled source_scale target_unscaled target_scale,
    source_unscaled = target_unscaled ->
    source_scale = target_scale ->
    match target_unscaled, target_scale with
    | Some unscaled, Some scale =>
        Some (Target.score_constructor 17
          [Target.ScoreBytes unscaled;
           Target.ScoreNode
             (Target.ScoreCons (Target.ScoreInteger scale) Target.ScoreNil)])
    | _, _ => None
    end =
    option_map Core.project_score
      (match source_unscaled, source_scale with
       | Some unscaled, Some scale =>
           Some (Core.source_score_constructor 17
             [Core.SourceBytes unscaled;
              Core.SourceNode
                (Core.SourceCons (Core.SourceInteger scale) Core.SourceNil)])
       | _, _ => None
       end).
Proof.
  intros source_unscaled source_scale target_unscaled target_scale
    unscaled scale.
  rewrite <- unscaled, <- scale.
  destruct source_unscaled as [bytes|];
    destruct source_scale as [word|];
    cbn [option_map]; try reflexivity;
    now rewrite Core.project_source_score_constructor.
Qed.

Lemma projected_bigrat_parent_score_from_fields :
  forall raw summary values program nodes,
    Core.source_summary_raw summary = raw ->
    Schema.raw_wire raw = Schema.LengthDelimited ->
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some 25 ->
    Schema.lookup_projected_program 25 [] [] = Some program ->
    Source.decode_region (S (length (Schema.raw_body raw)))
      (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)) =
      Some nodes ->
    Process.source_ground_blob_or_absent 1 values =
      Target.exact_ground_bytes_or_absent program 1
        (Schema.raw_body raw) nodes ->
    Process.source_ground_blob_or_absent 2 values =
      Target.exact_ground_bytes_or_absent program 2
        (Schema.raw_body raw) nodes ->
    Target.exact_bigrat_parent_score
      (Projection.project_source_summary summary) =
      option_map Core.project_score
        (if Core.source_summary_refused summary then None
         else Process.source_ground_bigrat_score values).
Proof.
  intros raw summary values program nodes retained wire payload family
    looked_up decoded numerator denominator.
  unfold Target.exact_bigrat_parent_score.
  change (Target.sem_type_id (Projection.project_source_summary summary))
    with (Projection.project_source_type_id
      (Core.source_summary_raw summary)).
  change (Target.sem_wire (Projection.project_source_summary summary))
    with (Schema.raw_wire (Core.source_summary_raw summary)).
  change (Target.sem_payload (Projection.project_source_summary summary))
    with (Schema.raw_payload (Core.source_summary_raw summary)).
  change (Target.sem_body (Projection.project_source_summary summary))
    with (Schema.raw_body (Core.source_summary_raw summary)).
  change (Target.sem_definitely_refused
      (Projection.project_source_summary summary))
    with (Core.source_summary_refused summary).
  rewrite retained, wire, payload.
  unfold Projection.project_source_type_id.
  rewrite family.
  cbn [Nat.eqb Schema.wire_eqb Schema.payload_eqb andb].
  destruct (Core.source_summary_refused summary) eqn:refused;
    cbn [negb andb]; [reflexivity|].
  rewrite looked_up, decoded.
  unfold Process.source_ground_bigrat_score.
  eapply bigrat_field_values_project_parent_score;
    [exact numerator|exact denominator].
Qed.

Lemma projected_fixed_point_parent_score_from_fields :
  forall raw summary values program nodes,
    Core.source_summary_raw summary = raw ->
    Schema.raw_wire raw = Schema.LengthDelimited ->
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some 26 ->
    Schema.lookup_projected_program 26 [] [] = Some program ->
    Source.decode_region (S (length (Schema.raw_body raw)))
      (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)) =
      Some nodes ->
    Process.source_ground_blob_or_absent 1 values =
      Target.exact_ground_bytes_or_absent program 1
        (Schema.raw_body raw) nodes ->
    Process.source_ground_u32_or_absent 2 values =
      Target.exact_ground_u32_or_absent program 2
        (Schema.raw_body raw) nodes ->
    Target.exact_fixed_point_parent_score
      (Projection.project_source_summary summary) =
      option_map Core.project_score
        (if Core.source_summary_refused summary then None
         else Process.source_ground_fixed_point_score values).
Proof.
  intros raw summary values program nodes retained wire payload family
    looked_up decoded unscaled scale.
  unfold Target.exact_fixed_point_parent_score.
  change (Target.sem_type_id (Projection.project_source_summary summary))
    with (Projection.project_source_type_id
      (Core.source_summary_raw summary)).
  change (Target.sem_wire (Projection.project_source_summary summary))
    with (Schema.raw_wire (Core.source_summary_raw summary)).
  change (Target.sem_payload (Projection.project_source_summary summary))
    with (Schema.raw_payload (Core.source_summary_raw summary)).
  change (Target.sem_body (Projection.project_source_summary summary))
    with (Schema.raw_body (Core.source_summary_raw summary)).
  change (Target.sem_definitely_refused
      (Projection.project_source_summary summary))
    with (Core.source_summary_refused summary).
  rewrite retained, wire, payload.
  unfold Projection.project_source_type_id.
  rewrite family.
  cbn [Nat.eqb Schema.wire_eqb Schema.payload_eqb andb].
  destruct (Core.source_summary_refused summary) eqn:refused;
    cbn [negb andb]; [reflexivity|].
  rewrite looked_up, decoded.
  unfold Process.source_ground_fixed_point_score.
  eapply fixed_point_field_values_project_parent_score;
    [exact unscaled|exact scale].
Qed.

Lemma generated_expr_bigrat_outer_score_reduces :
  forall raw summary values,
    Core.source_summary_value summary =
      Core.TypedSourceMessage raw values ->
    Core.source_summary_raw summary = raw ->
    Schema.raw_tag raw = 36 ->
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some 25 ->
    Target.exact_expr_score
      [Projection.project_source_summary summary] =
      Target.exact_bigrat_parent_score
        (Projection.project_source_summary summary) /\
    Process.source_expr_score [summary] =
      if Core.source_summary_refused summary then None
      else Process.source_ground_bigrat_score values.
Proof.
  intros raw summary values typed retained tag payload family.
  split.
  - unfold Target.exact_expr_score.
    change (Target.sem_payload (Projection.project_source_summary summary))
      with (Schema.raw_payload (Core.source_summary_raw summary)).
    change (Target.sem_tag (Projection.project_source_summary summary))
      with (Schema.raw_tag (Core.source_summary_raw summary)).
    now rewrite retained, payload, tag.
  - unfold Process.source_expr_score.
    rewrite typed.
    change (Core.source_summary_tag summary)
      with (Schema.raw_tag (Core.source_summary_raw summary)).
    change (Core.source_summary_type_id summary)
      with (Schema.raw_program_type_id (Core.source_summary_raw summary)).
    now rewrite retained, tag, family.
Qed.

Lemma generated_expr_fixed_point_outer_score_reduces :
  forall raw summary values,
    Core.source_summary_value summary =
      Core.TypedSourceMessage raw values ->
    Core.source_summary_raw summary = raw ->
    Schema.raw_tag raw = 37 ->
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some 26 ->
    Target.exact_expr_score
      [Projection.project_source_summary summary] =
      Target.exact_fixed_point_parent_score
        (Projection.project_source_summary summary) /\
    Process.source_expr_score [summary] =
      if Core.source_summary_refused summary then None
      else Process.source_ground_fixed_point_score values.
Proof.
  intros raw summary values typed retained tag payload family.
  split.
  - unfold Target.exact_expr_score.
    change (Target.sem_payload (Projection.project_source_summary summary))
      with (Schema.raw_payload (Core.source_summary_raw summary)).
    change (Target.sem_tag (Projection.project_source_summary summary))
      with (Schema.raw_tag (Core.source_summary_raw summary)).
    now rewrite retained, payload, tag.
  - unfold Process.source_expr_score.
    rewrite typed.
    change (Core.source_summary_tag summary)
      with (Schema.raw_tag (Core.source_summary_raw summary)).
    change (Core.source_summary_type_id summary)
      with (Schema.raw_program_type_id (Core.source_summary_raw summary)).
    now rewrite retained, tag, family.
Qed.

(** The hypotheses describe the actual generated Expr node and its
    successful recursive source evaluation.  Both nested field equalities
    are derived from that evaluation; neither a child list nor an alignment
    relation is supplied by the caller.  Refused children are covered too:
    both parent scorers return None before reading their fields. *)
Theorem actual_generated_expr_bigrat_parent_score_projects :
  forall fuel hash_fn bytes expr_program span field nested summary,
    Schema.lookup_projected_program 24 [] [] = Some expr_program ->
    Source.source_descriptor expr_program span = Some field ->
    Schema.descriptor_tag field = 36 ->
    Oracle.source_evaluate_node
      (Oracle.source_evaluate_message (S fuel) hash_fn)
      hash_fn bytes expr_program
      (Source.SourceMessage span field nested) = Some summary ->
    Target.exact_expr_score
      [Projection.project_source_summary summary] =
      option_map Core.project_score
        (Process.source_expr_score [summary]).
Proof.
  intros fuel hash_fn bytes expr_program span field nested summary
    expr_program_found selected tag evaluated.
  destruct (ChildProvenance.accepted_generated_message_node_has_descriptor_provenance
    (S fuel) hash_fn bytes 24 expr_program span field nested summary
    expr_program_found selected evaluated)
    as [raw [values [child_type
      [converted [typed [retained [raw_tag [wire
        [payload [raw_type field_type]]]]]]]]]].
  pose proof (proj1 (selected_generated_expr_ground_arm_type
    expr_program span field expr_program_found selected) tag)
    as expected_type.
  rewrite expected_type in field_type.
  inversion field_type; subst child_type.
  assert (actual_tag : Schema.raw_tag raw = 36).
  { now rewrite raw_tag. }
  assert (message_evaluated :
      Oracle.source_evaluate_message (S fuel) hash_fn raw =
        Some summary).
  { unfold Oracle.source_evaluate_node in evaluated.
    now rewrite converted in evaluated. }
  destruct (ScalarBridge.actual_bigrat_both_fields_agree
    fuel hash_fn raw summary raw_type message_evaluated)
    as [ground_program [nodes [children
      [ground_program_found [decoded [shape
        [numerator denominator]]]]]]].
  assert (typed_values : Core.source_summary_value summary =
      Core.TypedSourceMessage raw
        (map Core.source_summary_value children)).
  { now rewrite shape. }
  pose proof (generated_expr_bigrat_outer_score_reduces
    raw summary (map Core.source_summary_value children)
    typed_values retained actual_tag payload raw_type)
    as [target_outer source_outer].
  rewrite target_outer, source_outer.
  eapply projected_bigrat_parent_score_from_fields;
    eassumption.
Qed.

Theorem actual_generated_expr_fixed_point_parent_score_projects :
  forall fuel hash_fn bytes expr_program span field nested summary,
    Schema.lookup_projected_program 24 [] [] = Some expr_program ->
    Source.source_descriptor expr_program span = Some field ->
    Schema.descriptor_tag field = 37 ->
    Oracle.source_evaluate_node
      (Oracle.source_evaluate_message (S fuel) hash_fn)
      hash_fn bytes expr_program
      (Source.SourceMessage span field nested) = Some summary ->
    Target.exact_expr_score
      [Projection.project_source_summary summary] =
      option_map Core.project_score
        (Process.source_expr_score [summary]).
Proof.
  intros fuel hash_fn bytes expr_program span field nested summary
    expr_program_found selected tag evaluated.
  destruct (ChildProvenance.accepted_generated_message_node_has_descriptor_provenance
    (S fuel) hash_fn bytes 24 expr_program span field nested summary
    expr_program_found selected evaluated)
    as [raw [values [child_type
      [converted [typed [retained [raw_tag [wire
        [payload [raw_type field_type]]]]]]]]]].
  pose proof (proj2 (selected_generated_expr_ground_arm_type
    expr_program span field expr_program_found selected) tag)
    as expected_type.
  rewrite expected_type in field_type.
  inversion field_type; subst child_type.
  assert (actual_tag : Schema.raw_tag raw = 37).
  { now rewrite raw_tag. }
  assert (message_evaluated :
      Oracle.source_evaluate_message (S fuel) hash_fn raw =
        Some summary).
  { unfold Oracle.source_evaluate_node in evaluated.
    now rewrite converted in evaluated. }
  destruct (ScalarBridge.actual_fixed_point_both_fields_agree
    fuel hash_fn raw summary raw_type message_evaluated)
    as [ground_program [nodes [children
      [ground_program_found [decoded [shape
        [unscaled scale]]]]]]].
  assert (typed_values : Core.source_summary_value summary =
      Core.TypedSourceMessage raw
        (map Core.source_summary_value children)).
  { now rewrite shape. }
  pose proof (generated_expr_fixed_point_outer_score_reduces
    raw summary (map Core.source_summary_value children)
    typed_values retained actual_tag payload raw_type)
    as [target_outer source_outer].
  rewrite target_outer, source_outer.
  eapply projected_fixed_point_parent_score_from_fields;
    eassumption.
Qed.

Print Assumptions generated_expr_ground_arm_types.
Print Assumptions selected_generated_expr_ground_arm_type.
Print Assumptions bigrat_field_values_project_parent_score.
Print Assumptions fixed_point_field_values_project_parent_score.
Print Assumptions projected_bigrat_parent_score_from_fields.
Print Assumptions projected_fixed_point_parent_score_from_fields.
Print Assumptions generated_expr_bigrat_outer_score_reduces.
Print Assumptions generated_expr_fixed_point_outer_score_reduces.
Print Assumptions actual_generated_expr_bigrat_parent_score_projects.
Print Assumptions actual_generated_expr_fixed_point_parent_score_projects.
