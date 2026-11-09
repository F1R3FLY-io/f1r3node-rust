(** * The Connective arms And, Or, VarRef and the carriers 56 and 57

    Leaf L09 of the score-arms design.  The target's helpers
    [exact_connective_message_arm], [exact_connective_body_carrier] and
    [exact_var_ref_carrier], applied to the projection of the children the
    oracle evaluates from a decoded body, return the projection of the
    oracle's [source_connective_message_arm], [source_connective_body_carrier]
    and [source_var_ref_carrier].

    Citations: P is models/src/rust/rholang/protobuf_admission.rs and C is
    canonical_semantics.rs in the same directory.  Rust scores And and Or as
    constructors 401 and 402 over the carrier's Par scores (P:2956-2989,
    C:738-745), and VarRef as constructor 403 over its index and depth words
    (P:1150-1168).

    The family bridge RPB3IndependentOracleNameFamilyBridge consumes this
    theory (design inventory, row of leaf L12), so this theory must not import
    it.  The few lemmas it needs from that file are restated below. *)

From Stdlib Require Import List Bool Arith NArith ZArith Lia.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree RPB3SemanticReducer
  RPB3SourceScalarReduction RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission RPB3IndependentOracleProcess
  RPB3IndependentOracleProjection RPB3IndependentOracleRecursive
  RPB3IndependentOracleRecursiveProjection
  RPB3IndependentOracleProcessChildProvenance
  RPB3IndependentOracleCostScalarDecoderBridge
  RPB3SemanticReducer RPB3IndependentOracleExprPayload.

Import ListNotations.

(** ** Decoded nodes aligned with the children the oracle evaluated *)

Definition aligned_child (fuel : nat) (hash_fn : list nat -> nat)
    (body : list nat) (program : generated_program)
    (node : source_tree) (child : source_exact_summary) : Prop :=
  source_node_matches_generated_descriptor program node /\
  source_evaluate_node
    (source_evaluate_message fuel hash_fn) hash_fn body program node =
    Some child.

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
    source_descriptor program span = Some field ->
    In field (program_fields program).
Proof.
  intros program span field selected.
  unfold source_descriptor in selected.
  destruct (resolve_tag (program_fields program)
    (RPB3RawRegionTree.Wire.span_number span)) as [candidate|] eqn:resolved;
    [|discriminate].
  destruct (wire_eqb (wire_kind_of_span span) (descriptor_wire candidate));
    [|discriminate].
  inversion selected; subst candidate.
  eapply resolved_generated_field_is_member; exact resolved.
Qed.

Lemma looked_up_program_family :
  forall type_id program,
    lookup_projected_program type_id [] [] = Some program ->
    program_family program = type_id.
Proof.
  intros type_id program found.
  unfold lookup_projected_program in found.
  destruct (find (fun row =>
      RPB3DescriptorProjectionData.GeneratedData.program_type_id row =? type_id)
      RPB3DescriptorProjectionData.GeneratedData.generated_programs)
    as [row|] eqn:selected; [|discriminate].
  inversion found; subst program.
  apply find_some in selected as [_ family].
  cbn [program_family project_generated_program].
  now apply Nat.eqb_eq in family.
Qed.

(** ** Shape of an evaluated node *)

(** The summary of a scalar node, with its decoded value. *)
Lemma evaluated_scalar_node_shape :
  forall recurse hash_fn body program span field child,
    source_evaluate_node recurse hash_fn body program
      (SourceScalar span field) = Some child ->
    exists raw scalar,
      raw_field_of_source body (SourceScalar span field) = Some raw /\
      source_scalar_value_of_raw program raw = Some scalar /\
      source_typed_scalar program raw = Some (TypedSourceScalar raw scalar) /\
      child = SourceExactSummary (TypedSourceScalar raw scalar)
        (SourceFamilyAnalysis None None false false false) None.
Proof.
  intros recurse hash_fn body program span field child evaluated.
  unfold source_evaluate_node in evaluated.
  destruct (raw_field_of_source body (SourceScalar span field))
    as [raw|] eqn:converted; [|discriminate].
  destruct (source_typed_scalar program raw) as [value|] eqn:typed;
    [|discriminate].
  inversion evaluated; subst child.
  assert (shape : exists scalar,
      source_scalar_value_of_raw program raw = Some scalar /\
      value = TypedSourceScalar raw scalar).
  { unfold source_typed_scalar in typed.
    destruct (source_scalar_value_of_raw program raw) as [scalar|] eqn:decoded;
      [|discriminate].
    inversion typed; subst value.
    exists scalar.
    split; reflexivity. }
  destruct shape as [scalar [decoded same_value]].
  subst value.
  exists raw, scalar.
  split; [reflexivity|].
  split; [exact decoded|].
  split; [exact typed|].
  reflexivity.
Qed.

(** The independent scalar decoder is applied with the kind and default policy
    of the contract found for the raw field's tag. *)
Lemma scalar_value_decodes_with_contract :
  forall program raw scalar kind forbidden,
    source_scalar_value_of_raw program raw = Some scalar ->
    source_scalar_contract_for (program_family program) (raw_tag raw) =
      Some (SourceScalarContract kind forbidden) ->
    independently_decode_scalar kind forbidden (raw_body raw) = Some scalar.
Proof.
  intros program raw scalar kind forbidden accepted contract.
  unfold source_scalar_value_of_raw in accepted.
  rewrite contract in accepted.
  destruct (raw_payload raw); try discriminate accepted.
  destruct (raw_program_type_id raw); [discriminate accepted|].
  destruct (resolve_tag (program_fields program) (raw_tag raw))
    as [descriptor|]; [|discriminate accepted].
  cbn [source_contract_kind source_contract_default_forbidden] in accepted.
  destruct (source_scalar_wire kind) as [wire|]; [|discriminate accepted].
  destruct (source_raw_bytes_well_formed raw &&
    wire_eqb (raw_wire raw) wire &&
    wire_eqb wire (descriptor_wire descriptor) &&
    payload_eqb (descriptor_payload descriptor) Scalar);
    [exact accepted|discriminate accepted].
Qed.

Lemma source_pbsint32_decodes_to_i32 :
  forall body scalar,
    independently_decode_scalar
      RPB3DescriptorProjectionData.GeneratedData.PBSint32 true body =
      Some scalar ->
    exists value, scalar = SourceI32 value.
Proof.
  intros body scalar decoded.
  cbn [independently_decode_scalar] in decoded.
  destruct (independently_decoded_u64 body) as [number|]; [|discriminate].
  unfold source_numeric_scalar in decoded.
  destruct (source_numeric_nondefault true number); [|discriminate].
  destruct (N.leb number 4294967295); [|discriminate].
  eexists. now inversion decoded.
Qed.

(** Walk the nested matches of an auxiliary-node evaluation down to its
    success constructor.  A [None] branch is refuted by the equation to
    [Some]. *)
Ltac finish_auxiliary evaluated :=
  cbv beta iota zeta in evaluated;
  lazymatch type of evaluated with
  | Some _ = Some _ => idtac
  | None = Some _ => discriminate evaluated
  | (match ?scrutinee with _ => _ end) = Some _ =>
      destruct scrutinee; finish_auxiliary evaluated
  end.

Definition value_is_message (value : typed_source_value) : bool :=
  match value with
  | TypedSourceMessage _ _ => true
  | _ => false
  end.

(** An auxiliary map node never evaluates to a message-shaped value. *)
Lemma evaluated_auxiliary_node_is_not_message :
  forall recurse hash_fn body program span field child,
    source_evaluate_node recurse hash_fn body program
      (SourceAuxiliary span field) = Some child ->
    payload_eqb (raw_payload (source_summary_raw child)) Message = false /\
    value_is_message (source_summary_value child) = false.
Proof.
  intros recurse hash_fn body program span field child evaluated.
  unfold source_evaluate_node in evaluated.
  destruct (raw_field_of_source body (SourceAuxiliary span field))
    as [raw|] eqn:converted; [|discriminate evaluated].
  pose proof (raw_field_of_source_preserves_wire_and_body
    body (SourceAuxiliary span field) raw converted)
    as [_ [_ [raw_payload_is_descriptor _]]].
  cbn [source_field] in raw_payload_is_descriptor.
  destruct (descriptor_payload field) eqn:descriptor_kind;
    [discriminate evaluated|discriminate evaluated| |].
  all: finish_auxiliary evaluated.
  all: injection evaluated as same_child; subst child.
  all: cbn [source_summary_raw source_value_raw source_summary_value
    value_is_message].
  all: rewrite raw_payload_is_descriptor.
  all: split; reflexivity.
Qed.

(** A node whose descriptor is scalar evaluates only as a scalar node. *)
Lemma evaluated_node_with_scalar_descriptor_is_scalar :
  forall recurse hash_fn body program node child,
    descriptor_payload (source_field node) = Scalar ->
    source_evaluate_node recurse hash_fn body program node = Some child ->
    exists span field, node = SourceScalar span field.
Proof.
  intros recurse hash_fn body program node child payload evaluated.
  destruct node as [span field|span field nested|span field];
    cbn [source_field] in payload.
  - exists span, field. reflexivity.
  - exfalso.
    unfold source_evaluate_node in evaluated.
    destruct (raw_field_of_source body (SourceMessage span field nested))
      as [raw|] eqn:converted; [|discriminate evaluated].
    unfold raw_field_of_source in converted.
    cbn [source_span source_field] in converted.
    rewrite payload in converted.
    cbn [payload_eqb] in converted.
    discriminate converted.
  - exfalso.
    unfold source_evaluate_node in evaluated.
    destruct (raw_field_of_source body (SourceAuxiliary span field))
      as [raw|] eqn:converted; [|discriminate evaluated].
    unfold raw_field_of_source in converted.
    cbn [source_span source_field] in converted.
    rewrite payload in converted.
    discriminate converted.
Qed.

(** ** Typed message scores project on every aligned child

    A scalar or auxiliary child is not a typed message on either side; a
    message child carries the descriptor's own child type, and the score is the
    source score. *)
Lemma typed_message_score_projects :
  forall fuel hash_fn body family program node child expected,
    lookup_projected_program family [] [] = Some program ->
    source_node_matches_generated_descriptor program node ->
    source_evaluate_node
      (source_evaluate_message fuel hash_fn) hash_fn body program node =
      Some child ->
    exact_typed_message_score expected (project_source_summary child) =
    option_map project_score
      (source_payload_typed_message_score expected child).
Proof.
  intros fuel hash_fn body family program node child expected
    looked_up matched evaluated.
  destruct matched as [selected _].
  destruct node as [span field|span field nested|span field];
    cbn [source_span source_field] in selected.
  - destruct (accepted_source_scalar_node_has_typed_value
      _ hash_fn body program span field child evaluated)
      as [raw [scalar [_ [typed payload]]]].
    assert (retained : source_summary_raw child = raw).
    { unfold source_summary_raw. rewrite typed. reflexivity. }
    unfold exact_typed_message_score, source_payload_typed_message_score.
    change (sem_payload (project_source_summary child))
      with (raw_payload (source_summary_raw child)).
    rewrite retained, payload.
    cbn [payload_eqb].
    rewrite andb_false_r.
    rewrite typed.
    reflexivity.
  - exact (accepted_generated_message_node_typed_score_projects
      fuel hash_fn body family program span field nested child expected
      looked_up selected evaluated).
  - destruct (evaluated_auxiliary_node_is_not_message
      _ hash_fn body program span field child evaluated)
      as [payload value_shape].
    unfold exact_typed_message_score, source_payload_typed_message_score.
    change (sem_payload (project_source_summary child))
      with (raw_payload (source_summary_raw child)).
    rewrite payload.
    rewrite andb_false_r.
    destruct (source_summary_value child)
      as [some_raw some_scalar|some_raw some_nested
        |some_raw some_key some_value|some_raw some_snapshot some_values];
      cbn [value_is_message] in value_shape;
      try discriminate value_shape;
      reflexivity.
Qed.

Lemma aligned_children_typed_scores_project :
  forall fuel hash_fn body family program nodes children,
    lookup_projected_program family [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    forall expected child,
      In child children ->
      exact_typed_message_score expected (project_source_summary child) =
      option_map project_score
        (source_payload_typed_message_score expected child).
Proof.
  intros fuel hash_fn body family program nodes children looked_up aligned
    expected child member.
  assert (all_children : Forall (fun candidate =>
      exact_typed_message_score expected (project_source_summary candidate) =
      option_map project_score
        (source_payload_typed_message_score expected candidate)) children).
  { eapply forall2_right_forall; [|exact aligned].
    intros node candidate [matched evaluated].
    exact (typed_message_score_projects fuel hash_fn body family program
      node candidate expected looked_up matched evaluated). }
  rewrite Forall_forall in all_children.
  exact (all_children child member).
Qed.

(** ** The connective arms, one child at a time *)

Lemma connective_body_arm_projects :
  forall constructor child,
    exact_typed_message_score 57 (project_source_summary child) =
      option_map project_score
        (source_payload_typed_message_score 57 child) ->
    exact_connective_body_arm constructor (project_source_summary child) =
    option_map project_score (source_connective_body_arm constructor child).
Proof.
  intros constructor child same.
  unfold exact_connective_body_arm, source_connective_body_arm.
  rewrite same.
  destruct (source_payload_typed_message_score 57 child) as [score|];
    [|reflexivity].
  cbn [option_map].
  destruct score as [value|bytes|bytes|forest];
    cbn [project_score]; try reflexivity.
  destruct forest as [|head tail];
    cbn [project_score_forest]; try reflexivity.
  destruct head as [carrier|head_bytes|head_bytes|head_forest];
    cbn [project_score]; try reflexivity.
  destruct (N.eqb carrier 0); reflexivity.
Qed.

Lemma connective_message_arm_of_typed_scores :
  forall child,
    (forall expected,
      exact_typed_message_score expected (project_source_summary child) =
      option_map project_score
        (source_payload_typed_message_score expected child)) ->
    exact_connective_message_arm (project_source_summary child) =
    option_map project_score (source_connective_message_arm child).
Proof.
  intros child same.
  unfold exact_connective_message_arm, source_connective_message_arm.
  change (sem_tag (project_source_summary child))
    with (source_summary_tag child).
  destruct (source_summary_tag child) as [|[|[|[|[|tag]]]]];
    try reflexivity.
  - apply connective_body_arm_projects. apply same.
  - apply connective_body_arm_projects. apply same.
  - apply same.
Qed.

(** ** The ConnectiveBody carrier 57 *)

Lemma typed_selected_scores_project :
  forall tag expected children,
    (forall child, In child children ->
      exact_typed_message_score expected (project_source_summary child) =
      option_map project_score
        (source_payload_typed_message_score expected child)) ->
    exact_typed_selected_scores tag expected
      (map project_source_summary children) =
    option_map (map project_score)
      (source_payload_typed_selected_scores tag expected children).
Proof.
  intros tag expected children same.
  induction children as [|child rest IH]; [reflexivity|].
  cbn [map exact_typed_selected_scores source_payload_typed_selected_scores].
  rewrite IH by (intros other inside; apply same; right; exact inside).
  destruct (source_payload_typed_selected_scores tag expected rest)
    as [scores|]; cbn [option_map]; [|reflexivity].
  change (sem_tag (project_source_summary child))
    with (source_summary_tag child).
  destruct (source_summary_tag child =? tag); [|reflexivity].
  rewrite (same child (or_introl eq_refl)).
  destruct (source_payload_typed_message_score expected child) as [score|];
    reflexivity.
Qed.

Lemma connective_body_carrier_projects :
  forall children,
    exact_typed_selected_scores 1 1 (map project_source_summary children) =
    option_map (map project_score)
      (source_payload_typed_selected_scores 1 1 children) ->
    exact_connective_body_carrier (map project_source_summary children) =
    option_map project_score (source_connective_body_carrier children).
Proof.
  intros children same.
  unfold exact_connective_body_carrier, source_connective_body_carrier.
  rewrite same.
  destruct (source_payload_typed_selected_scores 1 1 children) as [scores|];
    [|reflexivity].
  cbn [option_map].
  rewrite project_source_score_constructor.
  reflexivity.
Qed.

(** ** The VarRef carrier 56

    Both of VarRef's fields are default-forbidden sint32 scalars, so every
    child is a scalar with an i32 value, and the target's decoder reads the
    same value from the child's body. *)

Lemma var_ref_field_facts :
  forall program field,
    lookup_projected_program 56 [] [] = Some program ->
    In field (program_fields program) ->
    descriptor_payload field = Scalar /\
    source_scalar_contract_for 56 (descriptor_tag field) =
      Some (SourceScalarContract
        RPB3DescriptorProjectionData.GeneratedData.PBSint32 true).
Proof.
  intros program field looked_up member.
  vm_compute in looked_up.
  inversion looked_up; subst program.
  cbn [program_fields] in member.
  destruct member as [same|[same|[]]]; subst field;
    vm_compute; split; reflexivity.
Qed.

Lemma var_ref_child_is_i32 :
  forall fuel hash_fn body program node child,
    lookup_projected_program 56 [] [] = Some program ->
    source_node_matches_generated_descriptor program node ->
    source_evaluate_node
      (source_evaluate_message fuel hash_fn) hash_fn body program node =
      Some child ->
    source_summary_refused child = false /\
    exists value,
      source_summary_value child =
        TypedSourceScalar (source_summary_raw child) (SourceI32 value) /\
      scalar_semantic_from_source
        (ScalarRule (source_summary_tag child)
          RPB3DescriptorProjectionData.GeneratedData.PBSint32 true)
        (source_summary_body child) = Some (ScalarI32 value).
Proof.
  intros fuel hash_fn body program node child looked_up matched evaluated.
  destruct matched as [selected _].
  pose proof (source_descriptor_field_member program (source_span node)
    (source_field node) selected) as member.
  destruct (var_ref_field_facts program (source_field node) looked_up member)
    as [payload contract].
  destruct (evaluated_node_with_scalar_descriptor_is_scalar
    _ hash_fn body program node child payload evaluated)
    as [span [field same_node]].
  subst node.
  cbn [source_span source_field] in selected, payload, contract.
  pose proof (resolved_source_descriptor_has_actual_tag_and_wire
    program span field selected) as [descriptor_tag_is _].
  destruct (evaluated_scalar_node_shape
    _ hash_fn body program span field child evaluated)
    as [raw [scalar [converted [decoded [_ same_child]]]]].
  subst child.
  pose proof (raw_field_of_source_preserves_wire_and_body
    body (SourceScalar span field) raw converted) as [raw_tag_is _].
  cbn [source_span] in raw_tag_is.
  pose proof (looked_up_program_family 56 program looked_up) as family_is.
  assert (contract_raw : source_scalar_contract_for
      (program_family program) (raw_tag raw) =
    Some (SourceScalarContract
      RPB3DescriptorProjectionData.GeneratedData.PBSint32 true)).
  { rewrite family_is, raw_tag_is, <- descriptor_tag_is. exact contract. }
  pose proof (scalar_value_decodes_with_contract program raw scalar
    RPB3DescriptorProjectionData.GeneratedData.PBSint32 true
    decoded contract_raw) as independent.
  destruct (source_pbsint32_decodes_to_i32 (raw_body raw) scalar independent)
    as [value same_scalar].
  subst scalar.
  split; [reflexivity|].
  exists value.
  split; [reflexivity|].
  exact (proj1 (RPB3IndependentOracleCostScalarDecoderBridge
    .cost_pbsint32_scalar_equivalence (raw_tag raw) (raw_body raw) value)
    independent).
Qed.

Definition var_ref_child_faithful (tag : nat)
    (child : source_exact_summary) : Prop :=
  source_summary_tag child = tag ->
  source_summary_refused child = false /\
  exists value,
    source_summary_value child =
      TypedSourceScalar (source_summary_raw child) (SourceI32 value) /\
    scalar_semantic_from_source
      (ScalarRule tag RPB3DescriptorProjectionData.GeneratedData.PBSint32 true)
      (source_summary_body child) = Some (ScalarI32 value).

Lemma var_ref_children_are_faithful :
  forall fuel hash_fn body program nodes children tag,
    lookup_projected_program 56 [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    Forall (var_ref_child_faithful tag) children.
Proof.
  intros fuel hash_fn body program nodes children tag looked_up aligned.
  eapply forall2_right_forall; [|exact aligned].
  intros node child [matched evaluated] same_tag.
  subst tag.
  exact (var_ref_child_is_i32 fuel hash_fn body program node child
    looked_up matched evaluated).
Qed.

(** [exact_i32_or_absent] and [source_payload_i32_or_absent] read the same
    child at the same tag.  The absent default is 0 on both sides. *)
Lemma i32_or_absent_agrees :
  forall tag children,
    Forall (var_ref_child_faithful tag) children ->
    exact_i32_or_absent tag (map project_source_summary children) =
    source_payload_i32_or_absent tag children.
Proof.
  intros tag children faithful.
  rewrite Forall_forall in faithful.
  unfold exact_i32_or_absent, source_payload_i32_or_absent,
    source_payload_child_at.
  rewrite <- projected_find_tag.
  destruct (find (fun child => source_summary_tag child =? tag) children)
    as [child|] eqn:found.
  - apply find_some in found as [member same_tag].
    apply Nat.eqb_eq in same_tag.
    destruct (faithful child member same_tag)
      as [unrefused [value [shape semantic]]].
    cbn [option_map].
    change (sem_body (project_source_summary child))
      with (source_summary_body child).
    rewrite semantic, unrefused, shape.
    reflexivity.
  - reflexivity.
Qed.

Lemma var_ref_carrier_projects :
  forall children,
    Forall (var_ref_child_faithful 1) children ->
    Forall (var_ref_child_faithful 2) children ->
    exact_var_ref_carrier (map project_source_summary children) =
    option_map project_score (source_var_ref_carrier children).
Proof.
  intros children first_tag second_tag.
  unfold exact_var_ref_carrier, source_var_ref_carrier.
  rewrite (i32_or_absent_agrees 1 children first_tag),
    (i32_or_absent_agrees 2 children second_tag).
  destruct (source_payload_i32_or_absent 1 children) as [index|];
    destruct (source_payload_i32_or_absent 2 children) as [depth|];
    reflexivity.
Qed.

(** ** The bridge

    The hypotheses are those of [generated_name_family_score_projects_totally].
    The scan-validity premise of the statement is not needed, because no score
    of these helpers consults the source scan; it is kept so that the statement
    has the shape of the pattern.

    Each conclusion is conditional on the family whose body is decoded: the
    arms read the children of a Connective (55), and the carriers read the
    children of a ConnectiveBody (57) or a VarRef (56).  An arm that is not one
    of And, Or or VarRef yields [None] on both sides. *)
Theorem connective_message_arm_projects :
  forall fuel hash_fn family program body nodes children,
    lookup_projected_program family [] [] = Some program ->
    decode_region (S (length body)) body program 0 (length body) = Some nodes ->
    sequence_source_results
      (map (source_evaluate_node
             (source_evaluate_message fuel hash_fn) hash_fn body program) nodes)
      = Some children ->
    source_scan_valid (source_scan_children program children) = true ->
    (family = 55 ->
      Forall (fun child =>
        exact_connective_message_arm (project_source_summary child) =
        option_map project_score (source_connective_message_arm child))
        children) /\
    (family = 57 ->
      exact_connective_body_carrier (map project_source_summary children) =
      option_map project_score (source_connective_body_carrier children)) /\
    (family = 56 ->
      exact_var_ref_carrier (map project_source_summary children) =
      option_map project_score (source_var_ref_carrier children)).
Proof.
  intros fuel hash_fn family program body nodes children
    looked_up decoded selected _.
  assert (aligned :
    Forall2 (aligned_child fuel hash_fn body program) nodes children).
  { exact (generated_selected_children_are_aligned program
      (source_evaluate_node (source_evaluate_message fuel hash_fn)
        hash_fn body program) nodes children
      (decoded_region_nodes_match_generated_source _ _ _ _ _ _ decoded)
      (RPB3IndependentOracleRecursiveProjection
        .selected_source_evaluator_outputs_are_positional
        _ _ _ _ _ selected)). }
  pose proof (aligned_children_typed_scores_project fuel hash_fn body family
    program nodes children looked_up aligned) as typed_scores.
  repeat split; intro same_family; subst family.
  - rewrite Forall_forall.
    intros child member.
    apply connective_message_arm_of_typed_scores.
    intro expected.
    exact (typed_scores expected child member).
  - apply connective_body_carrier_projects.
    apply typed_selected_scores_project.
    intros child member.
    exact (typed_scores 1 child member).
  - apply var_ref_carrier_projects.
    + exact (var_ref_children_are_faithful fuel hash_fn body program nodes
        children 1 looked_up aligned).
    + exact (var_ref_children_are_faithful fuel hash_fn body program nodes
        children 2 looked_up aligned).
Qed.

(** ** Red control

    A mutant reducer that swaps the And and Or constructors.  On a concrete
    And child the mutant disagrees with the oracle, while the genuine arm
    agrees, so the bridge statement fails for the mutant. *)

Definition mutant_and_or_swapped_arm (child : semantic_summary)
    : option canonical_score :=
  match sem_tag child with
  | 1 => exact_connective_body_arm 402 child
  | 2 => exact_connective_body_arm 401 child
  | 4 => exact_typed_message_score 56 child
  | _ => None
  end.

Definition control_body_child (tag : nat) : source_exact_summary :=
  SourceExactSummary
    (TypedSourceMessage
      (RawField tag LengthDelimited Message [] 0 (Some 57) [] []) [])
    (SourceFamilyAnalysis (Some true)
      (Some (source_score_constructor 0 [SourceInteger 11; SourceInteger 22]))
      false false false)
    None.

Example genuine_arms_match_the_oracle_on_the_control_child :
  option_map project_score
    (source_connective_message_arm (control_body_child 1)) =
    Some (score_constructor 401 [ScoreInteger 11; ScoreInteger 22]) /\
  exact_connective_message_arm (project_source_summary (control_body_child 1)) =
    Some (score_constructor 401 [ScoreInteger 11; ScoreInteger 22]) /\
  option_map project_score
    (source_connective_message_arm (control_body_child 2)) =
    Some (score_constructor 402 [ScoreInteger 11; ScoreInteger 22]) /\
  exact_connective_message_arm (project_source_summary (control_body_child 2)) =
    Some (score_constructor 402 [ScoreInteger 11; ScoreInteger 22]).
Proof. vm_compute. repeat split. Qed.

Example and_or_swapped_mutant_is_or_on_and :
  mutant_and_or_swapped_arm (project_source_summary (control_body_child 1)) =
    Some (score_constructor 402 [ScoreInteger 11; ScoreInteger 22]).
Proof. vm_compute. reflexivity. Qed.

Example and_or_swapped_mutant_disagrees_with_oracle :
  mutant_and_or_swapped_arm (project_source_summary (control_body_child 1)) <>
  option_map project_score
    (source_connective_message_arm (control_body_child 1)).
Proof. vm_compute. intro same. discriminate same. Qed.

Print Assumptions forall2_right_forall.
Print Assumptions source_descriptor_field_member.
Print Assumptions looked_up_program_family.
Print Assumptions evaluated_scalar_node_shape.
Print Assumptions scalar_value_decodes_with_contract.
Print Assumptions source_pbsint32_decodes_to_i32.
Print Assumptions evaluated_auxiliary_node_is_not_message.
Print Assumptions evaluated_node_with_scalar_descriptor_is_scalar.
Print Assumptions typed_message_score_projects.
Print Assumptions aligned_children_typed_scores_project.
Print Assumptions connective_body_arm_projects.
Print Assumptions connective_message_arm_of_typed_scores.
Print Assumptions typed_selected_scores_project.
Print Assumptions connective_body_carrier_projects.
Print Assumptions var_ref_field_facts.
Print Assumptions var_ref_child_is_i32.
Print Assumptions var_ref_children_are_faithful.
Print Assumptions i32_or_absent_agrees.
Print Assumptions var_ref_carrier_projects.
Print Assumptions connective_message_arm_projects.
Print Assumptions genuine_arms_match_the_oracle_on_the_control_child.
Print Assumptions and_or_swapped_mutant_is_or_on_and.
Print Assumptions and_or_swapped_mutant_disagrees_with_oracle.
