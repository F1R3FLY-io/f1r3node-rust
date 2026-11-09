From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement
  RPB3RawRegionTree RPB3SemanticReducer
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3IndependentOracleProcess RPB3IndependentOracleRecursive
  RPB3IndependentOracleRecursiveProjection
  RPB3IndependentOracleProcessChildProvenance.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Target := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Process := RPB3IndependentOracleProcess.
Module Oracle := RPB3IndependentOracleRecursive.
Module RecursiveProjection := RPB3IndependentOracleRecursiveProjection.
Module ChildProvenance := RPB3IndependentOracleProcessChildProvenance.

(** Families 25 and 26 have two singular scalar slots each.  The finite
    descriptor check is over generated schema data, never over a body or a
    unary decoded integer. *)
Definition ground_fields_are_scalar (program : Schema.generated_program)
    : bool :=
  forallb (fun field =>
    Schema.payload_eqb (Schema.descriptor_payload field) Schema.Scalar)
    (Schema.program_fields program).

Lemma payload_eqb_scalar_reflects_scalar :
  forall payload,
    Schema.payload_eqb payload Schema.Scalar = true ->
    payload = Schema.Scalar.
Proof.
  intros [| | |] accepted;
    cbn [Schema.payload_eqb] in accepted;
    try discriminate; reflexivity.
Qed.

Lemma generated_ground_program_has_only_scalar_fields :
  forall type_id program,
    In type_id [25; 26] ->
    Schema.lookup_projected_program type_id [] [] = Some program ->
    ground_fields_are_scalar program = true.
Proof.
  intros type_id program member looked_up.
  destruct member as [family|[family|[]]]; subst type_id.
  - assert (checked :
      option_map ground_fields_are_scalar
        (Schema.lookup_projected_program 25 [] []) = Some true).
    { vm_compute. reflexivity. }
    now rewrite looked_up in checked; inversion checked.
  - assert (checked :
      option_map ground_fields_are_scalar
        (Schema.lookup_projected_program 26 [] []) = Some true).
    { vm_compute. reflexivity. }
    now rewrite looked_up in checked; inversion checked.
Qed.

Lemma selected_ground_descriptor_is_scalar :
  forall program span field,
    ground_fields_are_scalar program = true ->
    Source.source_descriptor program span = Some field ->
    Schema.descriptor_payload field = Schema.Scalar.
Proof.
  intros program span field all_scalar selected.
  unfold Source.source_descriptor in selected.
  destruct (Schema.resolve_tag (Schema.program_fields program)
    (Source.Wire.span_number span)) as [candidate|] eqn:resolved;
    [|discriminate].
  destruct (Schema.wire_eqb (Source.wire_kind_of_span span)
    (Schema.descriptor_wire candidate)); [|discriminate].
  inversion selected; subst candidate.
  unfold ground_fields_are_scalar in all_scalar.
  pose proof (proj1 (@forallb_forall Schema.field_descriptor
    (fun selected_field => Schema.payload_eqb
      (Schema.descriptor_payload selected_field) Schema.Scalar)
    (Schema.program_fields program)) all_scalar) as every_field.
  assert (member : In field (Schema.program_fields program)).
  { eapply ChildProvenance.resolved_generated_field_is_member.
    exact resolved. }
  specialize (every_field field member).
  exact (payload_eqb_scalar_reflects_scalar _ every_field).
Qed.

(** Successful evaluation of a generated ground node cannot choose a
    Message or auxiliary constructor.  Its typed value retains the exact
    tag selected by the generated descriptor. *)
Lemma evaluated_ground_node_keeps_selected_tag :
  forall recurse hash_fn body program node child,
    ground_fields_are_scalar program = true ->
    Source.source_node_matches_generated_descriptor program node ->
    Oracle.source_evaluate_node recurse hash_fn body program node =
      Some child ->
    Schema.descriptor_tag (Source.source_field node) =
      Schema.raw_tag (Core.source_value_raw
        (Core.source_summary_value child)).
Proof.
  intros recurse hash_fn body program node child all_scalar
    [selected _] evaluated.
  pose proof (selected_ground_descriptor_is_scalar program
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
    pose proof (RecursiveProjection.accepted_source_typed_scalar_retains_raw
      program raw value typed) as retained.
    pose proof (Source.raw_field_of_source_preserves_wire_and_body
      body (Source.SourceScalar span field) raw converted)
      as [raw_tag _].
    cbn [Source.source_span Source.source_field] in raw_tag.
    pose proof (Source.resolved_source_descriptor_has_actual_tag_and_wire
      program span field selected) as [field_tag _].
    cbn [Core.source_summary_value Core.source_value_raw].
    now rewrite retained, raw_tag, <- field_tag.
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

Lemma source_ground_field_at_selected_summary :
  forall tag children,
    Process.source_ground_field_at tag
      (map Core.source_summary_value children) =
    option_map Core.source_summary_value
      (find (fun child => Schema.raw_tag
        (Core.source_value_raw (Core.source_summary_value child)) =? tag)
        children).
Proof.
  intros tag children.
  induction children as [|child rest IH];
    unfold Process.source_ground_field_at in *; simpl;
    [reflexivity|].
  destruct (Schema.raw_tag
    (Core.source_value_raw (Core.source_summary_value child)) =? tag);
    [reflexivity|exact IH].
Qed.

Lemma positional_ground_field_selection :
  forall recurse hash_fn body program nodes children tag,
    ground_fields_are_scalar program = true ->
    Forall2 (fun node child =>
      Source.source_node_matches_generated_descriptor program node /\
      Oracle.source_evaluate_node recurse hash_fn body program node =
        Some child) nodes children ->
    Process.source_ground_field_at tag
      (map Core.source_summary_value children) =
    match Target.exact_ground_node_at tag nodes with
    | None => None
    | Some node =>
        option_map Core.source_summary_value
          (Oracle.source_evaluate_node recurse hash_fn body program node)
    end.
Proof.
  intros recurse hash_fn body program nodes children tag all_scalar
    aligned.
  rewrite source_ground_field_at_selected_summary.
  induction aligned as [|node child nodes children
      [matched evaluated] rest IH];
    unfold Target.exact_ground_node_at in *; simpl;
    [reflexivity|].
  pose proof (evaluated_ground_node_keeps_selected_tag
    recurse hash_fn body program node child all_scalar matched
    evaluated) as same_tag.
  rewrite same_tag.
  destruct (Schema.raw_tag
    (Core.source_value_raw (Core.source_summary_value child)) =? tag);
    [now rewrite evaluated|exact IH].
Qed.

(** The alignment in the conclusion comes from this particular successful
    independent evaluation and this particular generated body decode.  No
    arbitrary typed-child list or external forest alignment is supplied. *)
Theorem actual_generated_ground_message_selects_same_child :
  forall fuel hash_fn raw summary type_id,
    In type_id [25; 26] ->
    Schema.raw_program_type_id raw = Some type_id ->
    Oracle.source_evaluate_message (S fuel) hash_fn raw =
      Some summary ->
    exists program nodes children,
      Schema.lookup_projected_program type_id [] [] = Some program /\
      Source.decode_region (S (length (Schema.raw_body raw)))
        (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)) =
        Some nodes /\
      summary = Oracle.source_message_summary raw program children /\
      forall tag,
        Process.source_ground_field_at tag
          (map Core.source_summary_value children) =
        match Target.exact_ground_node_at tag nodes with
        | None => None
        | Some node =>
            option_map Core.source_summary_value
              (Oracle.source_evaluate_node
                (Oracle.source_evaluate_message fuel hash_fn) hash_fn
                (Schema.raw_body raw) program node)
        end.
Proof.
  intros fuel hash_fn raw summary type_id member raw_type evaluated.
  destruct (ChildProvenance.accepted_independent_message_has_generated_children
    fuel hash_fn raw summary evaluated)
    as [actual_type [program [nodes [children
      [_ [actual_raw_type [looked_up [decoded [shape aligned]]]]]]]]].
  rewrite raw_type in actual_raw_type.
  inversion actual_raw_type; subst actual_type.
  exists program, nodes, children.
  repeat split; try assumption.
  intro tag.
  eapply positional_ground_field_selection.
  - eapply generated_ground_program_has_only_scalar_fields;
      [exact member|exact looked_up].
  - exact aligned.
Qed.

Print Assumptions generated_ground_program_has_only_scalar_fields.
Print Assumptions payload_eqb_scalar_reflects_scalar.
Print Assumptions selected_ground_descriptor_is_scalar.
Print Assumptions evaluated_ground_node_keeps_selected_tag.
Print Assumptions source_ground_field_at_selected_summary.
Print Assumptions positional_ground_field_selection.
Print Assumptions actual_generated_ground_message_selects_same_child.
