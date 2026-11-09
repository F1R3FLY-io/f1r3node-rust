From Stdlib Require Import List Bool Arith NArith Lia.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree
  RPB3SourceScalarReduction RPB3SemanticReducer
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3IndependentOracleProcess RPB3IndependentOracleRecursive
  RPB3IndependentOracleProjection
  RPB3IndependentOracleRecursiveProjection
  RPB3IndependentOracleGeneratedResolverVerdict
  RPB3IndependentOracleNameDecoderBridge
  RPB3IndependentOracleProcessScalarDecoderBridge
  RPB3IndependentOracleProcessChildProvenance
  RPB3RecursiveAuxiliarySourceReference.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Scalar := RPB3SourceScalarReduction.
Module Target := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Process := RPB3IndependentOracleProcess.
Module Oracle := RPB3IndependentOracleRecursive.
Module Projection := RPB3IndependentOracleProjection.
Module RecursiveProjection := RPB3IndependentOracleRecursiveProjection.
Module ResolverVerdict := RPB3IndependentOracleGeneratedResolverVerdict.
Module NameBridge := RPB3IndependentOracleNameDecoderBridge.
Module ScalarBridge := RPB3IndependentOracleProcessScalarDecoderBridge.
Module ChildProvenance := RPB3IndependentOracleProcessChildProvenance.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.

(** The generated Par connective forbids the encoded default [0].  The
    independent unary decoder and the target binary decoder therefore have
    the same refusal set, including noncanonical and out-of-range varints. *)
Lemma par_connective_bool_decoder_refusal_equivalence :
  forall tag body,
    Core.independently_decode_scalar Data.PBBool true body = None <->
    Scalar.scalar_semantic_from_source
      (Scalar.ScalarRule tag Data.PBBool true) body = None.
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
  destruct (N.leb number 1), (N.eqb number 0);
    cbn; split; intro refused; try reflexivity; discriminate.
Qed.

Lemma generated_par_connective_bool_rule :
  forall program,
    Schema.lookup_projected_program 1 [] [] = Some program ->
    Scalar.generated_scalar_rule program 10 =
      Some (Scalar.ScalarRule 10 Data.PBBool true).
Proof.
  intros program looked_up.
  rewrite Source.generated_par_source_lookup_is_present in looked_up.
  inversion looked_up; subst program.
  vm_compute. reflexivity.
Qed.

(** This instantiates the already proved generated scalar gate with the
    actual Par descriptor, rather than assuming a relation between two
    separately supplied child forests. *)
Theorem generated_par_connective_scalar_child_verdict :
  forall recurse par_child hash_fn bytes program span field,
    Schema.lookup_projected_program 1 [] [] = Some program ->
    Source.Wire.span_number span = 10 ->
    Source.source_descriptor program span = Some field ->
    RecursiveProjection.source_target_child_verdict
      (Oracle.source_evaluate_node recurse hash_fn bytes program
        (Source.SourceScalar span field))
      (Recursive.recursive_source_reference par_child hash_fn bytes
        program (Source.SourceScalar span field)).
Proof.
  intros recurse par_child hash_fn bytes program span field
    looked_up tag selected.
  destruct (Source.raw_field_of_source bytes
    (Source.SourceScalar span field)) as [raw|] eqn:converted.
  - eapply (ChildProvenance.selected_generated_scalar_node_has_child_verdict
      recurse par_child hash_fn bytes 1 program span field raw
      Data.PBBool true).
    + exact looked_up.
    + exact selected.
    + exact converted.
    + rewrite tag.
      rewrite Source.generated_par_source_lookup_is_present in looked_up.
      inversion looked_up; subst program.
      exact RecursiveProjection.generated_par_connective_source_contract.
    + rewrite tag.
      now apply generated_par_connective_bool_rule.
    + reflexivity.
    + reflexivity.
    + split; intro refused.
      * apply (proj1 (ChildProvenance.scalar_semantic_none_iff_body_invalid
          (Scalar.ScalarRule (Source.Wire.span_number span)
            Data.PBBool true) (Schema.raw_body raw))).
        apply (proj1 (par_connective_bool_decoder_refusal_equivalence
          (Source.Wire.span_number span) (Schema.raw_body raw))).
        exact refused.
      * apply (proj2 (par_connective_bool_decoder_refusal_equivalence
          (Source.Wire.span_number span) (Schema.raw_body raw))).
        apply (proj2 (ChildProvenance.scalar_semantic_none_iff_body_invalid
          (Scalar.ScalarRule (Source.Wire.span_number span)
            Data.PBBool true) (Schema.raw_body raw))).
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

(** A manufactured summary with the forbidden encoded default demonstrates
    why the Par score algebra needs actual generated child admission. *)
Definition synthetic_default_connective : Core.source_exact_summary :=
  Core.SourceExactSummary
    (Core.TypedSourceScalar
      (Schema.RawField 10 Schema.Varint Schema.Scalar [0] 1 None [] [])
      (Core.SourceBool false))
    (Core.SourceFamilyAnalysis None None false false false) None.

Theorem synthetic_default_connective_breaks_unqualified_score_projection :
  Oracle.source_par_score [synthetic_default_connective] =
    Some (Core.source_score_constructor 999 [Core.SourceInteger 0]) /\
  Target.exact_par_score
    [Projection.project_source_summary synthetic_default_connective] =
    None.
Proof. vm_compute. split; reflexivity. Qed.

Definition plain_par_child (child : Core.source_exact_summary) : Prop :=
  match Core.source_summary_value child with
  | Core.TypedSourceScalar raw _ =>
      Schema.raw_payload raw = Schema.Scalar
  | Core.TypedSourceMessage raw _ =>
      Schema.raw_payload raw = Schema.Message
  | Core.TypedSourceStringMap _ _ _
  | Core.TypedSourcePathMap _ _ _ => False
  end.

Lemma actual_generated_par_node_is_plain :
  forall fuel hash_fn bytes node child,
    Source.source_node_matches_generated_descriptor
      Source.generated_par_source_program node ->
    Oracle.source_evaluate_node
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn bytes
      Source.generated_par_source_program node = Some child ->
    plain_par_child child.
Proof.
  intros fuel hash_fn bytes [span field|span field nested|span field]
    child matched evaluated.
  - destruct (ChildProvenance.accepted_source_scalar_node_has_typed_value
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn bytes
      Source.generated_par_source_program span field child evaluated)
      as [raw [scalar [_ [typed payload]]]].
    unfold plain_par_child.
    now rewrite typed.
  - destruct (ChildProvenance.accepted_source_message_node_has_typed_value
      fuel hash_fn bytes Source.generated_par_source_program span field
      nested child evaluated) as [raw [children [_ [typed payload]]]].
    unfold plain_par_child.
    now rewrite typed.
  - pose proof (RecursiveProjection.matched_par_node_field_is_member
      (Source.SourceAuxiliary span field) matched) as member.
    destruct (RecursiveProjection.generated_par_descriptor_payload_is_plain
      field member) as [scalar|message].
    + unfold Oracle.source_evaluate_node,
        Source.raw_field_of_source in evaluated.
      cbn [Source.source_span Source.source_field] in evaluated.
      rewrite scalar in evaluated. discriminate.
    + unfold Oracle.source_evaluate_node,
        Source.raw_field_of_source in evaluated.
      cbn [Source.source_span Source.source_field] in evaluated.
      rewrite message in evaluated. discriminate.
Qed.

Lemma actual_generated_par_children_are_plain :
  forall fuel hash_fn bytes summary,
    Oracle.source_evaluate_message (S fuel) hash_fn
      (Oracle.source_par_raw bytes) = Some summary ->
    exists nodes children,
      Source.decode_region (S (length bytes)) bytes
        Source.generated_par_source_program 0 (length bytes) = Some nodes /\
      summary = Oracle.source_message_summary
        (Oracle.source_par_raw bytes)
        Source.generated_par_source_program children /\
      Forall2 (fun node child =>
        Source.source_node_matches_generated_descriptor
          Source.generated_par_source_program node /\
        Oracle.source_evaluate_node
          (Oracle.source_evaluate_message fuel hash_fn) hash_fn bytes
          Source.generated_par_source_program node = Some child)
        nodes children /\
      Forall plain_par_child children.
Proof.
  intros fuel hash_fn bytes summary evaluated.
  destruct (ChildProvenance.accepted_independent_message_has_generated_children
    fuel hash_fn (Oracle.source_par_raw bytes) summary evaluated)
    as [type_id [program [nodes [children
      [_ [raw_type [looked_up [decoded [shape aligned]]]]]]]]].
  cbn [Oracle.source_par_raw Schema.raw_program_type_id] in raw_type.
  inversion raw_type; subst type_id.
  rewrite Source.generated_par_source_lookup_is_present in looked_up.
  inversion looked_up; subst program.
  exists nodes, children.
  split.
  - cbn [Oracle.source_par_raw Schema.raw_body] in decoded.
    exact decoded.
  - split; [exact shape|].
    split.
    + cbn [Oracle.source_par_raw Schema.raw_body] in aligned.
      exact aligned.
    + clear shape decoded.
      induction aligned as [|node child nodes children
        [matched chosen] rest IH]; constructor.
      * eapply actual_generated_par_node_is_plain;
          eassumption.
      * exact IH.
Qed.

(** The oracle restates production's family order independently of the
    target; the two literals are one list. *)
Lemma source_par_score_family_order_is_target_order :
  Oracle.source_par_score_family_order = Target.par_score_family_order.
Proof. reflexivity. Qed.

(** The projection of one labelled score: the tag is kept, and the source
    score is carried to the target score. *)
Definition project_tagged_score (entry : nat * Core.source_score)
    : nat * Target.canonical_score :=
  (fst entry, Core.project_score (snd entry)).

Lemma filter_family_commutes_with_tagged_score_projection :
  forall family tagged,
    filter (fun entry => fst entry =? family)
      (map project_tagged_score tagged) =
    map project_tagged_score
      (filter (fun entry => fst entry =? family) tagged).
Proof.
  intros family tagged.
  induction tagged as [|[tag score] rest IH]; [reflexivity|].
  cbn [map filter fst project_tagged_score].
  destruct (tag =? family); cbn [map]; rewrite IH; reflexivity.
Qed.

(** Family-by-family concatenation commutes with score projection for any
    family list: the projection keeps each entry's tag, so each family filter
    selects the same positions on the source and on the target. *)
Lemma family_ordered_scores_project_for_any_order :
  forall (order : list nat) tagged,
    flat_map (fun family =>
      map snd (filter (fun entry => fst entry =? family)
        (map project_tagged_score tagged))) order =
    map Core.project_score
      (flat_map (fun family =>
        map snd (filter (fun entry => fst entry =? family) tagged)) order).
Proof.
  induction order as [|family rest IH]; intro tagged; [reflexivity|].
  cbn [flat_map].
  rewrite map_app.
  rewrite (IH tagged).
  rewrite filter_family_commutes_with_tagged_score_projection.
  f_equal.
  rewrite !map_map.
  apply map_ext.
  intros [tag score]; reflexivity.
Qed.

(** The labelled, wire-ordered collection projects entry by entry.  This is the
    former element-wise argument: a plain child is a scalar, which contributes
    nothing on both sides, or a message, whose projected evidence is exactly
    the source score. *)
Lemma plain_par_tagged_component_scores_project :
  forall children,
    Forall plain_par_child children ->
    Target.exact_par_tagged_component_scores
      (map Projection.project_source_summary children) =
    option_map (map project_tagged_score)
      (Oracle.source_par_tagged_component_scores children).
Proof.
  induction children as [|child rest IH]; intro plain;
    [reflexivity|].
  inversion plain as [|? ? child_plain rest_plain]; subst.
  cbn [map Target.exact_par_tagged_component_scores
    Oracle.source_par_tagged_component_scores].
  rewrite (IH rest_plain).
  destruct (Oracle.source_par_tagged_component_scores rest)
    as [tagged|] eqn:collected; [|reflexivity].
  unfold plain_par_child in child_plain.
  destruct (Core.source_summary_value child)
    as [raw scalar|raw nested|raw key value|raw snapshot values]
    eqn:typed;
    cbn in child_plain; try contradiction.
  - change (Target.sem_payload (Projection.project_source_summary child))
      with (Schema.raw_payload (Core.source_summary_raw child)).
    unfold Core.source_summary_raw, Core.source_value_raw.
    rewrite typed.
    rewrite child_plain.
    reflexivity.
  - change (Target.sem_payload (Projection.project_source_summary child))
      with (Schema.raw_payload (Core.source_summary_raw child)).
    change (Target.sem_score_evidence
      (Projection.project_source_summary child)) with
      (Projection.project_source_evidence
        (Core.source_summary_exact_score child)).
    change (Target.sem_tag (Projection.project_source_summary child))
      with (Core.source_summary_tag child).
    unfold Core.source_summary_raw, Core.source_value_raw.
    rewrite typed.
    rewrite child_plain.
    destruct (Core.source_summary_exact_score child);
      reflexivity.
Qed.

Lemma plain_par_component_scores_project :
  forall children,
    Forall plain_par_child children ->
    Target.exact_par_component_scores
      (map Projection.project_source_summary children) =
    option_map (map Core.project_score)
      (Oracle.source_par_component_scores children).
Proof.
  intros children plain.
  unfold Target.exact_par_component_scores,
    Oracle.source_par_component_scores.
  rewrite (plain_par_tagged_component_scores_project children plain).
  destruct (Oracle.source_par_tagged_component_scores children)
    as [tagged|]; [|reflexivity].
  cbn [option_map].
  unfold Target.par_family_ordered_scores,
    Oracle.source_par_family_ordered_scores.
  rewrite source_par_score_family_order_is_target_order.
  f_equal.
  apply family_ordered_scores_project_for_any_order.
Qed.

Lemma projected_par_connective_find :
  forall children,
    find (fun child => Target.sem_tag child =? 10)
      (map Projection.project_source_summary children) =
    option_map Projection.project_source_summary
      (Process.source_child_at 10 children).
Proof.
  induction children as [|child rest IH]; [reflexivity|].
  cbn [map find option_map Process.source_child_at].
  change (Target.sem_tag (Projection.project_source_summary child))
    with (Core.source_summary_tag child).
  destruct (Core.source_summary_tag child =? 10);
    [reflexivity|exact IH].
Qed.

Lemma source_par_connective_value_find :
  forall children,
    find (fun value => Process.source_value_tag value =? 10)
      (map Core.source_summary_value children) =
    option_map Core.source_summary_value
      (Process.source_child_at 10 children).
Proof.
  induction children as [|child rest IH]; [reflexivity|].
  cbn [map find option_map Process.source_child_at].
  change (Process.source_value_tag (Core.source_summary_value child))
    with (Core.source_summary_tag child).
  destruct (Core.source_summary_tag child =? 10);
    [reflexivity|exact IH].
Qed.

Lemma actual_par_default_projects_connective_score_flag :
  forall bytes children semantic_default,
    Process.source_par_typed_default
      (Core.TypedSourceMessage (Oracle.source_par_raw bytes)
        (map Core.source_summary_value children)) =
      Some semantic_default ->
    Forall plain_par_child children ->
    Forall (fun child => Core.source_summary_refused child = false)
      children ->
    Target.scalar_true_or_absent 10
      (map Projection.project_source_summary children) =
    Process.source_bool_or_absent 10 children.
Proof.
  intros bytes children semantic_default default plain unrefused.
  unfold Process.source_par_typed_default in default.
  cbn [Oracle.source_par_raw Schema.raw_program_type_id] in default.
  rewrite source_par_connective_value_find in default.
  unfold Target.scalar_true_or_absent,
    Process.source_bool_or_absent, Process.source_child_at.
  rewrite projected_par_connective_find.
  unfold Process.source_child_at in *.
  destruct (find
      (fun child => Core.source_summary_tag child =? 10) children)
    as [child|] eqn:selected; [|reflexivity].
  cbn [option_map] in default.
  cbn [option_map].
  apply find_some in selected as [member _].
  assert (child_plain : plain_par_child child).
  { eapply Source.source_forall_member;
      [exact plain|exact member]. }
  assert (child_unrefused : Core.source_summary_refused child = false).
  { exact (@Source.source_forall_member Core.source_exact_summary
      (fun candidate => Core.source_summary_refused candidate = false)
      children child unrefused member). }
  destruct (Core.source_summary_value child)
    as [raw scalar|raw nested|raw key value|raw snapshot values]
    eqn:typed;
    try rewrite typed in default;
    try discriminate default.
  destruct scalar as [flag|i32|u32|i64|u64|text|blob];
    try discriminate default.
  destruct flag; [|discriminate default].
  assert (body_one : Schema.raw_body raw = [1]).
  { destruct (Schema.raw_body raw) as [|first [|second suffix]]
      eqn:body; try discriminate default.
    - destruct first as [|[|later]];
        try discriminate default; reflexivity.
    - destruct first as [|[|later]];
        cbn in default; discriminate default. }
  unfold plain_par_child in child_plain.
  try rewrite typed in child_plain.
  cbn in child_plain.
  rewrite child_unrefused.
  try rewrite typed.
  assert (projected_payload :
    Target.sem_payload (Projection.project_source_summary child) =
      Schema.raw_payload raw).
  { change (Target.sem_payload (Projection.project_source_summary child))
      with (Schema.raw_payload (Core.source_summary_raw child)).
    unfold Core.source_summary_raw, Core.source_value_raw.
    now rewrite typed. }
  assert (projected_body :
    Target.sem_body (Projection.project_source_summary child) =
      Schema.raw_body raw).
  { change (Target.sem_body (Projection.project_source_summary child))
      with (Core.source_summary_body child).
    unfold Core.source_summary_body, Core.source_summary_raw,
      Core.source_value_raw.
    now rewrite typed. }
  now rewrite projected_payload, projected_body, child_plain, body_one.
Qed.

Lemma par_score_projects_from_admitted_children :
  forall bytes children semantic_default,
    Process.source_par_typed_default
      (Core.TypedSourceMessage (Oracle.source_par_raw bytes)
        (map Core.source_summary_value children)) =
      Some semantic_default ->
    Forall plain_par_child children ->
    Forall (fun child => Core.source_summary_refused child = false)
      children ->
    Target.exact_par_score
      (map Projection.project_source_summary children) =
    option_map Core.project_score (Oracle.source_par_score children).
Proof.
  intros bytes children semantic_default default plain unrefused.
  pose proof (plain_par_component_scores_project children plain)
    as components.
  pose proof (actual_par_default_projects_connective_score_flag
    bytes children semantic_default default plain unrefused)
    as connective.
  unfold Target.exact_par_score, Oracle.source_par_score.
  rewrite components, connective.
  destruct (Oracle.source_par_component_scores children)
    as [scores|]; [|reflexivity].
  destruct (Process.source_bool_or_absent 10 children)
    as [flag|]; [|reflexivity].
  cbn [option_map].
  rewrite Core.project_source_score_constructor.
  rewrite map_app.
  destruct flag; reflexivity.
Qed.

Lemma generated_par_source_summary_score_and_default :
  forall bytes children,
    Core.source_summary_exact_score
      (Oracle.source_message_summary (Oracle.source_par_raw bytes)
        Source.generated_par_source_program children) =
      Oracle.source_par_score children /\
    Core.source_summary_semantic_default
      (Oracle.source_message_summary (Oracle.source_par_raw bytes)
        Source.generated_par_source_program children) =
      Process.source_par_typed_default
        (Core.TypedSourceMessage (Oracle.source_par_raw bytes)
          (map Core.source_summary_value children)).
Proof.
  intros bytes children.
  split.
  - unfold Core.source_summary_exact_score,
      Oracle.source_message_summary.
    cbn [Core.source_summary_analysis Core.source_family_score
      Schema.program_family Oracle.source_message_score].
    reflexivity.
  - unfold Oracle.source_message_summary.
    cbn [Core.source_summary_semantic_default Schema.program_family].
    reflexivity.
Qed.

(** The children here come from the independent decoder of the public Par
    body.  No caller-supplied target forest or child-alignment premise is
    used; the remaining recursive target-list correspondence is separate. *)
Theorem accepted_independent_par_root_has_projected_score :
  forall hash_fn bytes semantic_default summary,
    Oracle.source_par_reference hash_fn bytes =
      Some (semantic_default, summary) ->
    exists nodes children,
      Source.decode_region (S (length bytes)) bytes
        Source.generated_par_source_program 0 (length bytes) = Some nodes /\
      summary = Oracle.source_message_summary
        (Oracle.source_par_raw bytes)
        Source.generated_par_source_program children /\
      Forall2 (fun node child =>
        Source.source_node_matches_generated_descriptor
          Source.generated_par_source_program node /\
        Oracle.source_evaluate_node
          (Oracle.source_evaluate_message (length bytes) hash_fn)
          hash_fn bytes Source.generated_par_source_program node =
          Some child) nodes children /\
      Target.score_evidence_for 1 None
        (map Projection.project_source_summary children) =
      Projection.project_source_evidence
        (Core.source_summary_exact_score summary).
Proof.
  intros hash_fn bytes semantic_default summary accepted.
  apply (proj1 (ResolverVerdict.independent_par_acceptance_gate_iff
    hash_fn bytes semantic_default summary)) in accepted.
  destruct accepted as [evaluated [unrefused default]].
  assert (evaluated_message :
    Oracle.source_evaluate_message (S (length bytes)) hash_fn
      (Oracle.source_par_raw bytes) = Some summary).
  { rewrite Oracle.source_evaluate_message_unfold.
    exact evaluated. }
  destruct (actual_generated_par_children_are_plain
    (length bytes) hash_fn bytes summary evaluated_message)
    as [nodes [children [decoded [shape [aligned plain]]]]].
  pose proof (ChildProvenance.accepted_source_message_children_are_unrefused
    (Oracle.source_par_raw bytes) Source.generated_par_source_program
    children) as children_unrefused.
  assert (unrefused_children :
    Forall (fun child => Core.source_summary_refused child = false)
      children).
  { apply children_unrefused.
    now rewrite <- shape. }
  pose proof (generated_par_source_summary_score_and_default
    bytes children) as [source_score source_default].
  rewrite shape in default.
  rewrite source_default in default.
  pose proof (par_score_projects_from_admitted_children
    bytes children semantic_default default plain unrefused_children)
    as projected.
  exists nodes, children.
  repeat split; try assumption.
  cbn [Target.score_evidence_for].
  rewrite projected, shape, source_score.
  destruct (Oracle.source_par_score children); reflexivity.
Qed.

Lemma par_reducer_uses_exact_par_score :
  forall bytes children,
    Target.sem_score_evidence
      (Target.reduce_semantic_field (Oracle.source_par_raw bytes)
        children) =
    Target.score_evidence_for 1 None children.
Proof.
  intros bytes children.
  unfold Target.reduce_semantic_field, Target.generated_wire_program.
  cbn [Oracle.source_par_raw Schema.raw_payload
    Schema.raw_program_type_id].
  rewrite Source.generated_par_source_lookup_is_present.
  cbn [Target.sem_score_evidence Schema.program_family
    Target.score_evidence_for].
  reflexivity.
Qed.

Corollary accepted_independent_par_root_reducer_score_projects :
  forall hash_fn bytes semantic_default summary,
    Oracle.source_par_reference hash_fn bytes =
      Some (semantic_default, summary) ->
    exists nodes children,
      Source.decode_region (S (length bytes)) bytes
        Source.generated_par_source_program 0 (length bytes) = Some nodes /\
      summary = Oracle.source_message_summary
        (Oracle.source_par_raw bytes)
        Source.generated_par_source_program children /\
      Target.sem_score_evidence
        (Target.reduce_semantic_field (Oracle.source_par_raw bytes)
          (map Projection.project_source_summary children)) =
      Projection.project_source_evidence
        (Core.source_summary_exact_score summary).
Proof.
  intros hash_fn bytes semantic_default summary accepted.
  destruct (accepted_independent_par_root_has_projected_score
    hash_fn bytes semantic_default summary accepted)
    as [nodes [children [decoded [shape [_ score]]]]].
  exists nodes, children.
  repeat split; try assumption.
Qed.

Print Assumptions par_connective_bool_decoder_refusal_equivalence.
Print Assumptions generated_par_connective_bool_rule.
Print Assumptions generated_par_connective_scalar_child_verdict.
Print Assumptions synthetic_default_connective_breaks_unqualified_score_projection.
Print Assumptions actual_generated_par_node_is_plain.
Print Assumptions actual_generated_par_children_are_plain.
Print Assumptions plain_par_component_scores_project.
Print Assumptions projected_par_connective_find.
Print Assumptions source_par_connective_value_find.
Print Assumptions actual_par_default_projects_connective_score_flag.
Print Assumptions par_score_projects_from_admitted_children.
Print Assumptions generated_par_source_summary_score_and_default.
Print Assumptions accepted_independent_par_root_has_projected_score.
Print Assumptions par_reducer_uses_exact_par_score.
Print Assumptions accepted_independent_par_root_reducer_score_projects.
