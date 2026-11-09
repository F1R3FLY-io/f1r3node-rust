(** * Bridge for the ordered collection payload scores: EList 27, ETuple 28,
    ESet 29.

    On the children that the oracle actually produces for a decoded node of
    one of these families, the target helper
    [exact_ordered_collection_score] agrees with the projection of the oracle
    helper [source_ordered_collection_score], and the target's completeness
    agrees with the oracle's.

    The argument follows [generated_name_family_score_projects_totally]:

    - [generated_selected_children_are_aligned] turns the sequenced evaluation
      into a positional [Forall2] between the decoded nodes and the children;
    - [valid_source_scan_children_are_unrefused] gives that no child is
      refused;
    - every reader used by the helpers agrees on aligned children, proved by a
      case analysis on the node that produced the child;
    - the connective flag at tag 4 is a default-forbidden [bool], so a present
      child is the encoded [true], on which [scalar_true_or_absent] and
      [source_payload_bool_or_absent] agree.

    The remainder default -1 is the one place a closed value enters both sides;
    the file ends with the red control showing that the default 0 breaks the
    bridge. *)

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

(** The oracle's typed-score reader has exactly the body of the Process reader,
    so the provenance lemmas apply to it by conversion. *)
Lemma payload_typed_message_score_is_process :
  forall type_id child,
    source_payload_typed_message_score type_id child =
    Process.source_typed_message_score type_id child.
Proof. reflexivity. Qed.

(** The typed reader agrees on every actual child: a scalar or auxiliary node
    is not a message on either side, and a message node is the provenance
    lemma. *)
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

Lemma payload_typed_optional_score_or_projects :
  forall fuel hash_fn family program body nodes children tag type_id fallback,
    Schema.lookup_projected_program family [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    exact_typed_optional_score_or tag type_id (Core.project_score fallback)
      (map Projection.project_source_summary children) =
    option_map Core.project_score
      (source_payload_typed_optional_score_or tag type_id fallback children).
Proof.
  intros fuel hash_fn family program body nodes children tag type_id fallback
    looked_up aligned.
  unfold exact_typed_optional_score_or, exact_typed_optional_child_score,
    source_payload_typed_optional_score_or,
    source_payload_typed_optional_score, source_payload_child_at.
  rewrite <- Projection.projected_find_tag.
  destruct (find (fun child => Core.source_summary_tag child =? tag) children)
    as [child|] eqn:found; cbn [option_map].
  - apply find_some in found as [member _].
    destruct (aligned_child_of_member fuel hash_fn body program nodes children
      child aligned member) as [node pair].
    rewrite (aligned_child_typed_message_score_projects fuel hash_fn family
      program body node child type_id looked_up pair).
    destruct (source_payload_typed_message_score type_id child) as [score|];
      reflexivity.
  - reflexivity.
Qed.

(** A flag child of a default-forbidden tag reads [true] on both sides: this is
    the one reader where a refused child must be excluded, because the oracle
    consults the refusal bit and the target does not. *)
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

(** ** The ordered collection helpers *)

Lemma absent_remainder_score_is_projected :
  exact_absent_remainder_score = Core.project_score source_minus_one.
Proof. vm_compute. reflexivity. Qed.

Lemma collection_remainder_projects :
  forall fuel hash_fn family program body nodes children has_remainder,
    Schema.lookup_projected_program family [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    exact_collection_remainder has_remainder
      (map Projection.project_source_summary children) =
    option_map (map Core.project_score)
      (source_collection_remainder has_remainder children).
Proof.
  intros fuel hash_fn family program body nodes children has_remainder
    looked_up aligned.
  unfold exact_collection_remainder, source_collection_remainder.
  destruct has_remainder; [|reflexivity].
  rewrite absent_remainder_score_is_projected.
  rewrite (payload_typed_optional_score_or_projects fuel hash_fn family program
    body nodes children 5 6 source_minus_one looked_up aligned).
  destruct (source_payload_typed_optional_score_or 5 6 source_minus_one
    children) as [score|]; reflexivity.
Qed.

Lemma ordered_collection_score_projects :
  forall fuel hash_fn family constructor has_remainder program body nodes
      children,
    flag_tag_scalar_in_family family 4 = true ->
    Admission.source_scalar_contract_for family 4 =
      Some (Admission.SourceScalarContract Data.PBBool true) ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    Forall (fun child => Core.source_summary_refused child = false) children ->
    exact_ordered_collection_score constructor has_remainder
      (map Projection.project_source_summary children) =
    option_map Core.project_score
      (source_ordered_collection_score constructor has_remainder children).
Proof.
  intros fuel hash_fn family constructor has_remainder program body nodes
    children certificate contract looked_up aligned unrefused.
  assert (flags : Forall (connective_flag_child_true 4) children).
  { eapply forall2_right_forall; [|exact aligned].
    intros node child pair.
    exact (forbidden_flag_child_is_true fuel hash_fn body program family 4
      node child looked_up certificate contract pair). }
  pose proof (payload_bool_or_absent_projects 4 children flags unrefused)
    as flag.
  pose proof (payload_typed_selected_scores_project fuel hash_fn family program
    body nodes children 1 1 looked_up aligned) as elements.
  pose proof (collection_remainder_projects fuel hash_fn family program body
    nodes children has_remainder looked_up aligned) as remainder.
  unfold exact_ordered_collection_score, source_ordered_collection_score.
  rewrite remainder, elements, flag.
  destruct (source_collection_remainder has_remainder children)
    as [remainder_scores|]; cbn [option_map]; [|reflexivity].
  destruct (source_payload_typed_selected_scores 1 1 children)
    as [element_scores|]; cbn [option_map]; [|reflexivity].
  destruct (source_payload_bool_or_absent 4 children) as [connective|];
    cbn [option_map]; [|reflexivity].
  rewrite Core.project_source_score_constructor, !map_app.
  cbn [map].
  unfold source_payload_bool_word.
  destruct connective; reflexivity.
Qed.

(** ** The bridge *)

(** The decoded nodes of a collection family are aligned with the children the
    oracle evaluated from them, and on those children both the score and the
    completeness of the target helper and of the oracle helper agree. *)
Theorem ordered_collection_projects :
  forall fuel hash_fn family constructor has_remainder program body nodes
      children,
    In (family, constructor, has_remainder)
      [(27, 6%N, true); (28, 7%N, false); (29, 8%N, true)] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Source.decode_region (S (length body)) body program 0 (length body) =
      Some nodes ->
    Source.sequence_source_results
      (map (Oracle.source_evaluate_node
             (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program)
        nodes) = Some children ->
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = true ->
    exact_ordered_collection_score constructor has_remainder
      (map Projection.project_source_summary children) =
    option_map Core.project_score
      (source_ordered_collection_score constructor has_remainder children) /\
    expr_payload_completeness family
      (fold_left (Target.scan_child program)
        (map Projection.project_source_summary children) Schema.empty_scan)
      (map Projection.project_source_summary children) =
    source_expr_payload_completeness family
      (Admission.source_scan_children program children) children.
Proof.
  intros fuel hash_fn family constructor has_remainder program body nodes
    children member looked_up decoded selected valid.
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
  assert (certificates :
    flag_tag_scalar_in_family family 4 = true /\
    Admission.source_scalar_contract_for family 4 =
      Some (Admission.SourceScalarContract Data.PBBool true)).
  { destruct member as [same|[same|[same|[]]]];
      inversion same; subst;
      split; vm_compute; reflexivity. }
  destruct certificates as [certificate contract].
  split.
  - exact (ordered_collection_score_projects fuel hash_fn family constructor
      has_remainder program body nodes children certificate contract looked_up
      aligned unrefused).
  - destruct member as [same|[same|[same|[]]]];
      inversion same; subst; reflexivity.
Qed.

(** ** Red control: remainder default 0

    The mutant is the target helper with the absent-remainder default replaced
    by the integer 0.  The witness is the empty EList: the body is empty, so
    the decoded nodes and the children are empty and the hypotheses of the
    bridge hold by computation; the only difference left is the default. *)
Definition mutant_zero_remainder_collection_score (constructor : N)
    (has_remainder : bool) (children : list semantic_summary)
    : option canonical_score :=
  match (if has_remainder
         then option_map (fun remainder => [remainder])
           (exact_typed_optional_score_or 5 6 (ScoreInteger 0) children)
         else Some []),
      exact_typed_selected_scores 1 1 children,
      scalar_true_or_absent 4 children with
  | Some remainder, Some elements, Some connective =>
      Some (score_constructor constructor
        (remainder ++ elements ++
          [ScoreInteger (if connective then 1 else 0)]))
  | _, _, _ => None
  end.

Definition empty_elist_children : list Core.source_exact_summary :=
  Correspondence.witness_children 27 [].

Lemma empty_elist_witness_satisfies_the_hypotheses :
  Schema.lookup_projected_program 27 [] [] =
    Some (Correspondence.witness_program 27) /\
  Source.decode_region (S (length (@nil nat))) [] (Correspondence.witness_program 27)
    0 (length (@nil nat)) = Some (Correspondence.witness_nodes 27 []) /\
  Source.sequence_source_results
    (map (Oracle.source_evaluate_node
           (Oracle.source_evaluate_message 1 Correspondence.witness_hash_fn)
           Correspondence.witness_hash_fn [] (Correspondence.witness_program 27))
      (Correspondence.witness_nodes 27 [])) = Some empty_elist_children /\
  Admission.source_scan_valid
    (Admission.source_scan_children (Correspondence.witness_program 27)
      empty_elist_children) = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example zero_remainder_mutant_disagrees_with_the_oracle :
  mutant_zero_remainder_collection_score 6 true
    (map Projection.project_source_summary empty_elist_children) <>
  option_map Core.project_score
    (source_ordered_collection_score 6 true empty_elist_children).
Proof. vm_compute. discriminate. Qed.

(** The real helper is equal to the oracle on the same witness, so the
    disagreement above is the mutant's, not the witness's. *)
Example real_helper_agrees_on_the_red_control_witness :
  exact_ordered_collection_score 6 true
    (map Projection.project_source_summary empty_elist_children) =
  option_map Core.project_score
    (source_ordered_collection_score 6 true empty_elist_children).
Proof. vm_compute. reflexivity. Qed.

Theorem zero_remainder_mutant_fails_the_bridge :
  ~ (forall fuel hash_fn program body nodes children,
      Schema.lookup_projected_program 27 [] [] = Some program ->
      Source.decode_region (S (length body)) body program 0 (length body) =
        Some nodes ->
      Source.sequence_source_results
        (map (Oracle.source_evaluate_node
               (Oracle.source_evaluate_message fuel hash_fn) hash_fn body
               program) nodes) = Some children ->
      Admission.source_scan_valid
        (Admission.source_scan_children program children) = true ->
      mutant_zero_remainder_collection_score 6 true
        (map Projection.project_source_summary children) =
      option_map Core.project_score
        (source_ordered_collection_score 6 true children)).
Proof.
  intro bridge.
  destruct empty_elist_witness_satisfies_the_hypotheses
    as [looked_up [decoded [selected scan_valid]]].
  exact (zero_remainder_mutant_disagrees_with_the_oracle
    (bridge 1 Correspondence.witness_hash_fn
      (Correspondence.witness_program 27) [] (Correspondence.witness_nodes 27 [])
      empty_elist_children looked_up decoded selected scan_valid)).
Qed.

Print Assumptions payload_typed_message_score_is_process.
Print Assumptions aligned_child_typed_message_score_projects.
Print Assumptions aligned_child_of_member.
Print Assumptions payload_typed_selected_scores_project.
Print Assumptions payload_typed_optional_score_or_projects.
Print Assumptions payload_bool_or_absent_projects.
Print Assumptions absent_remainder_score_is_projected.
Print Assumptions collection_remainder_projects.
Print Assumptions ordered_collection_score_projects.
Print Assumptions ordered_collection_projects.
Print Assumptions empty_elist_witness_satisfies_the_hypotheses.
Print Assumptions zero_remainder_mutant_disagrees_with_the_oracle.
Print Assumptions real_helper_agrees_on_the_red_control_witness.
Print Assumptions zero_remainder_mutant_fails_the_bridge.
