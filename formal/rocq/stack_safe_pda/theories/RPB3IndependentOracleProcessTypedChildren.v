(** * Correspondence of the score readers on actual process children

    This theory compares seven score readers of the semantic reducer with
    their counterparts in the independent process oracle, on the children
    that the oracle's decoder and evaluator actually produce for a process
    family ([Correspondence.process_actual_children]).

    The three untyped readers ([Target.exact_child_score],
    [Target.exact_selected_scores], [Target.exact_optional_child_score]) read
    only the projected score evidence.  The unconditional lemmas of
    [RPB3IndependentOracleProcessChildProvenance] establish them for every
    list of children, so the forms stated here simply discard the premise.

    The four typed readers ([Target.exact_typed_child_score],
    [Target.exact_typed_selected_scores],
    [Target.exact_typed_optional_child_score],
    [Target.exact_typed_optional_score_or]) also test that the selected child
    is a message of the requested type, and the two sides perform that test on
    different data: [Target.exact_typed_message_score] reads the projected
    type id (a raw type id of [None] projects to [0]), wire, payload and
    refusal bit, while [Process.source_typed_message_score] reads the typed
    value, the optional raw type id and the refusal bit.  All four typed
    statements reduce to one per-child equation,
    [actual_child_typed_message_score_projects], proved by a case analysis on
    the source node that produced the child:

    - A scalar node yields a [TypedSourceScalar] value with [Scalar] payload.
      The target reader refuses the payload and the source reader refuses the
      value, whatever the type id.
    - A message node is the provenance lemma
      [accepted_generated_message_node_typed_score_projects]: the generated
      descriptor fixes the length-delimited wire, the [Message] payload and
      the observed child type id, and the refusal bits agree by projection.
      Its premise that the node's descriptor resolves comes from
      [Source.decoded_region_nodes_match_generated_source].
    - An auxiliary node evaluates only for a [StringMap] or [PathMap]
      descriptor, whose payload is not [Message] and whose value is not a
      [TypedSourceMessage], so both readers refuse.

    None of the proofs uses the ordered-scan premise of
    [Correspondence.process_actual_children], and refused message children
    need no separate case because the provenance lemma compares the two
    refusal tests directly.  No restriction of [family], [tag] or [type_id]
    is required: each statement holds for all actual children. *)

From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement RPB3RawRegionTree
  RPB3SemanticReducer RPB3IndependentOracleCore RPB3IndependentOracleProcess
  RPB3IndependentOracleProcessProjection RPB3IndependentOracleRecursive
  RPB3IndependentOracleProjection RPB3IndependentOracleProcessChildProvenance
  RPB3IndependentOracleProcessChildCorrespondence.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Target := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Process := RPB3IndependentOracleProcess.
Module Projection := RPB3IndependentOracleProjection.
Module ProcessProjection := RPB3IndependentOracleProcessProjection.
Module Oracle := RPB3IndependentOracleRecursive.
Module ChildProvenance := RPB3IndependentOracleProcessChildProvenance.
Module Correspondence := RPB3IndependentOracleProcessChildCorrespondence.

(** ** The typed readers on a child that is not a message *)

Definition value_is_message (value : Core.typed_source_value) : bool :=
  match value with
  | Core.TypedSourceMessage _ _ => true
  | _ => false
  end.

(** The target reader tests the payload and the source reader tests the typed
    value, so a child that passes neither test reads as absent on both
    sides. *)
Lemma non_message_child_typed_score_projects :
  forall child type_id,
    Schema.payload_eqb (Schema.raw_payload (Core.source_summary_raw child))
      Schema.Message = false ->
    value_is_message (Core.source_summary_value child) = false ->
    Target.exact_typed_message_score type_id
      (Projection.project_source_summary child) =
    option_map Core.project_score
      (Process.source_typed_message_score type_id child).
Proof.
  intros child type_id not_message not_value.
  assert (target_absent :
    Target.exact_typed_message_score type_id
      (Projection.project_source_summary child) = None).
  { unfold Target.exact_typed_message_score.
    change (Target.sem_payload (Projection.project_source_summary child))
      with (Schema.raw_payload (Core.source_summary_raw child)).
    rewrite not_message.
    rewrite andb_false_r.
    reflexivity. }
  assert (source_absent :
    Process.source_typed_message_score type_id child = None).
  { unfold Process.source_typed_message_score.
    destruct (Core.source_summary_value child);
      try discriminate not_value;
      destruct (Core.source_summary_type_id child); reflexivity. }
  rewrite target_absent, source_absent.
  reflexivity.
Qed.

Lemma typed_scalar_child_is_not_message :
  forall child raw scalar,
    Core.source_summary_value child = Core.TypedSourceScalar raw scalar ->
    Schema.raw_payload raw = Schema.Scalar ->
    Schema.payload_eqb (Schema.raw_payload (Core.source_summary_raw child))
      Schema.Message = false /\
    value_is_message (Core.source_summary_value child) = false.
Proof.
  intros child raw scalar typed payload.
  unfold Core.source_summary_raw.
  rewrite typed.
  cbn [Core.source_value_raw value_is_message].
  rewrite payload.
  split; reflexivity.
Qed.

(** An auxiliary node evaluates only under a [StringMap] or [PathMap]
    descriptor; the evaluator refuses the other payloads.  The proof walks
    the evaluator's nested matches with the tactic the sibling lemma
    [evaluated_auxiliary_node_is_not_scalar] uses. *)
Lemma evaluated_auxiliary_node_is_not_message :
  forall recurse hash_fn body program span field child,
    Oracle.source_evaluate_node recurse hash_fn body program
      (Source.SourceAuxiliary span field) = Some child ->
    Schema.payload_eqb (Schema.raw_payload (Core.source_summary_raw child))
      Schema.Message = false /\
    value_is_message (Core.source_summary_value child) = false.
Proof.
  intros recurse hash_fn body program span field child evaluated.
  unfold Oracle.source_evaluate_node in evaluated.
  destruct (Source.raw_field_of_source body (Source.SourceAuxiliary span field))
    as [raw|] eqn:converted; [|discriminate evaluated].
  pose proof (Source.raw_field_of_source_preserves_wire_and_body
    body (Source.SourceAuxiliary span field) raw converted)
    as [_ [_ [raw_payload_is_descriptor _]]].
  cbn [Source.source_field] in raw_payload_is_descriptor.
  destruct (Schema.descriptor_payload field) eqn:descriptor_kind;
    [discriminate evaluated|discriminate evaluated| |].
  all: finish_auxiliary evaluated.
  all: injection evaluated as same_child; subst child.
  all: cbn [Core.source_summary_raw Core.source_value_raw
    Core.source_summary_value value_is_message].
  all: rewrite raw_payload_is_descriptor.
  all: split; reflexivity.
Qed.

(** ** The typed reader on one actual child *)

(** Every actual child comes from a decoded node, and the decoder resolved
    that node's descriptor in the family's generated program. *)
Lemma actual_child_has_matched_node :
  forall fuel hash_fn family program body children child,
    Correspondence.process_actual_children fuel hash_fn family program body
      children ->
    In child children ->
    Schema.lookup_projected_program family [] [] = Some program /\
    exists node,
      Source.source_node_matches_generated_descriptor program node /\
      Oracle.source_evaluate_node
        (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program
        node = Some child.
Proof.
  intros fuel hash_fn family program body children child actual member.
  unfold Correspondence.process_actual_children in actual.
  destruct actual as [_ [looked_up [_ [nodes [decoded sequenced]]]]].
  split; [exact looked_up|].
  destruct (Correspondence.child_of_sequenced_nodes_has_node _ _
    (Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
      hash_fn body program) nodes children child sequenced member)
    as [node [node_member evaluated]].
  pose proof (Source.decoded_region_nodes_match_generated_source
    (S (length body)) body program 0 (length body) nodes decoded) as generated.
  rewrite Forall_forall in generated.
  exists node.
  split; [exact (generated node node_member)|exact evaluated].
Qed.

Lemma actual_child_typed_message_score_projects :
  forall fuel hash_fn family program body children type_id child,
    Correspondence.process_actual_children fuel hash_fn family program body
      children ->
    In child children ->
    Target.exact_typed_message_score type_id
      (Projection.project_source_summary child) =
    option_map Core.project_score
      (Process.source_typed_message_score type_id child).
Proof.
  intros fuel hash_fn family program body children type_id child actual member.
  destruct (actual_child_has_matched_node fuel hash_fn family program body
    children child actual member)
    as [looked_up [node [matched evaluated]]].
  destruct node as [span field|span field nested|span field].
  - destruct (ChildProvenance.accepted_source_scalar_node_has_typed_value
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program
      span field child evaluated)
      as [raw [scalar [_ [typed payload]]]].
    destruct (typed_scalar_child_is_not_message child raw scalar typed payload)
      as [not_message not_value].
    exact (non_message_child_typed_score_projects child type_id not_message
      not_value).
  - unfold Source.source_node_matches_generated_descriptor in matched.
    destruct matched as [resolved _].
    cbn [Source.source_span Source.source_field] in resolved.
    exact (ChildProvenance.accepted_generated_message_node_typed_score_projects
      fuel hash_fn body family program span field nested child type_id
      looked_up resolved evaluated).
  - destruct (evaluated_auxiliary_node_is_not_message _ hash_fn body program
      span field child evaluated) as [not_message not_value].
    exact (non_message_child_typed_score_projects child type_id not_message
      not_value).
Qed.

Lemma selected_actual_child_typed_message_score_projects :
  forall fuel hash_fn family program body children tag type_id child,
    Correspondence.process_actual_children fuel hash_fn family program body
      children ->
    Process.source_child_at tag children = Some child ->
    Target.exact_typed_message_score type_id
      (Projection.project_source_summary child) =
    option_map Core.project_score
      (Process.source_typed_message_score type_id child).
Proof.
  intros fuel hash_fn family program body children tag type_id child actual
    selected.
  unfold Process.source_child_at in selected.
  apply find_some in selected as [member _].
  exact (actual_child_typed_message_score_projects fuel hash_fn family program
    body children type_id child actual member).
Qed.

(** ** The list-level reader for selected typed scores

    The fold over the list needs the per-child equation on every element.
    Stating it as a premise keeps the induction independent of the
    actual-children hypothesis, which would otherwise have to be generalized
    over the list. *)
Lemma typed_selected_scores_correspond_on_agreeing_children :
  forall tag type_id children,
    (forall child, In child children ->
      Target.exact_typed_message_score type_id
        (Projection.project_source_summary child) =
      option_map Core.project_score
        (Process.source_typed_message_score type_id child)) ->
    Target.exact_typed_selected_scores tag type_id
      (ProcessProjection.projected_process_children children) =
    option_map (map Core.project_score)
      (Process.source_typed_selected_scores tag type_id children).
Proof.
  intros tag type_id children.
  unfold ProcessProjection.projected_process_children.
  induction children as [|child rest IH]; intro agrees; [reflexivity|].
  cbn [map Target.exact_typed_selected_scores
    Process.source_typed_selected_scores].
  rewrite IH by (intros candidate member; apply agrees; right; exact member).
  destruct (Process.source_typed_selected_scores tag type_id rest)
    as [scores|]; [|reflexivity].
  cbn [option_map map].
  change (Target.sem_tag (Projection.project_source_summary child))
    with (Core.source_summary_tag child).
  destruct (Core.source_summary_tag child =? tag); [|reflexivity].
  rewrite (agrees child (or_introl eq_refl)).
  destruct (Process.source_typed_message_score type_id child); reflexivity.
Qed.

(** ** The untyped readers on actual children *)

Theorem process_actual_children_child_score_corresponds :
  forall fuel hash_fn family program body children,
    Correspondence.process_actual_children fuel hash_fn family program body
      children ->
    forall tag,
      Target.exact_child_score tag
        (ProcessProjection.projected_process_children children) =
      option_map Core.project_score
        (Process.source_child_score tag children).
Proof.
  intros fuel hash_fn family program body children _ tag.
  exact (ChildProvenance.projected_process_child_score_unconditional tag
    children).
Qed.

Theorem process_actual_children_selected_scores_correspond :
  forall fuel hash_fn family program body children,
    Correspondence.process_actual_children fuel hash_fn family program body
      children ->
    forall tag,
      Target.exact_selected_scores tag
        (ProcessProjection.projected_process_children children) =
      option_map (map Core.project_score)
        (Process.source_selected_scores tag children).
Proof.
  intros fuel hash_fn family program body children _ tag.
  exact (ChildProvenance.projected_process_selected_scores_unconditional tag
    children).
Qed.

Theorem process_actual_children_optional_score_corresponds :
  forall fuel hash_fn family program body children,
    Correspondence.process_actual_children fuel hash_fn family program body
      children ->
    forall tag,
      Target.exact_optional_child_score tag
        (ProcessProjection.projected_process_children children) =
      option_map (option_map Core.project_score)
        (Process.source_optional_score tag children).
Proof.
  intros fuel hash_fn family program body children _ tag.
  exact (ChildProvenance.projected_process_optional_score_unconditional tag
    children).
Qed.

(** ** The typed readers on actual children *)

Theorem process_actual_children_typed_child_corresponds :
  forall fuel hash_fn family program body children,
    Correspondence.process_actual_children fuel hash_fn family program body
      children ->
    forall tag type_id,
      Target.exact_typed_child_score tag type_id
        (ProcessProjection.projected_process_children children) =
      option_map Core.project_score
        (Process.source_typed_child_score tag type_id children).
Proof.
  intros fuel hash_fn family program body children actual tag type_id.
  unfold Target.exact_typed_child_score, Process.source_typed_child_score.
  rewrite ChildProvenance.projected_process_child_at.
  destruct (Process.source_child_at tag children) as [child|] eqn:selected;
    [|reflexivity].
  cbn [option_map].
  exact (selected_actual_child_typed_message_score_projects fuel hash_fn family
    program body children tag type_id child actual selected).
Qed.

Theorem process_actual_children_typed_selected_corresponds :
  forall fuel hash_fn family program body children,
    Correspondence.process_actual_children fuel hash_fn family program body
      children ->
    forall tag type_id,
      Target.exact_typed_selected_scores tag type_id
        (ProcessProjection.projected_process_children children) =
      option_map (map Core.project_score)
        (Process.source_typed_selected_scores tag type_id children).
Proof.
  intros fuel hash_fn family program body children actual tag type_id.
  apply typed_selected_scores_correspond_on_agreeing_children.
  intros child member.
  exact (actual_child_typed_message_score_projects fuel hash_fn family program
    body children type_id child actual member).
Qed.

Theorem process_actual_children_typed_optional_corresponds :
  forall fuel hash_fn family program body children,
    Correspondence.process_actual_children fuel hash_fn family program body
      children ->
    forall tag type_id,
      Target.exact_typed_optional_child_score tag type_id
        (ProcessProjection.projected_process_children children) =
      option_map (option_map Core.project_score)
        (Process.source_typed_optional_score tag type_id children).
Proof.
  intros fuel hash_fn family program body children actual tag type_id.
  unfold Target.exact_typed_optional_child_score,
    Process.source_typed_optional_score.
  rewrite ChildProvenance.projected_process_child_at.
  destruct (Process.source_child_at tag children) as [child|] eqn:selected;
    [|reflexivity].
  cbn [option_map].
  rewrite (selected_actual_child_typed_message_score_projects fuel hash_fn
    family program body children tag type_id child actual selected).
  destruct (Process.source_typed_message_score type_id child); reflexivity.
Qed.

Theorem process_actual_children_typed_fallback_corresponds :
  forall fuel hash_fn family program body children,
    Correspondence.process_actual_children fuel hash_fn family program body
      children ->
    forall tag type_id fallback,
      Target.exact_typed_optional_score_or tag type_id
        (Core.project_score fallback)
        (ProcessProjection.projected_process_children children) =
      option_map Core.project_score
        (Process.source_typed_optional_score_or tag type_id fallback children).
Proof.
  intros fuel hash_fn family program body children actual tag type_id fallback.
  unfold Target.exact_typed_optional_score_or,
    Process.source_typed_optional_score_or.
  rewrite (process_actual_children_typed_optional_corresponds fuel hash_fn
    family program body children actual tag type_id).
  destruct (Process.source_typed_optional_score tag type_id children)
    as [[score|]|]; reflexivity.
Qed.

Print Assumptions non_message_child_typed_score_projects.
Print Assumptions typed_scalar_child_is_not_message.
Print Assumptions evaluated_auxiliary_node_is_not_message.
Print Assumptions actual_child_has_matched_node.
Print Assumptions actual_child_typed_message_score_projects.
Print Assumptions selected_actual_child_typed_message_score_projects.
Print Assumptions typed_selected_scores_correspond_on_agreeing_children.
Print Assumptions process_actual_children_child_score_corresponds.
Print Assumptions process_actual_children_selected_scores_correspond.
Print Assumptions process_actual_children_optional_score_corresponds.
Print Assumptions process_actual_children_typed_child_corresponds.
Print Assumptions process_actual_children_typed_selected_corresponds.
Print Assumptions process_actual_children_typed_optional_corresponds.
Print Assumptions process_actual_children_typed_fallback_corresponds.
