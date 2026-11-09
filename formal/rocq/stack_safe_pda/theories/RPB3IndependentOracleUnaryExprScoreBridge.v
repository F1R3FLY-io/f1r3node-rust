(** Bridge theorem for the unary expression payloads EVar 35, ENot 36 and
    ENeg 37 (leaf L04 of the score-arms design,
    sections 2 and 5).

    [unary_payload_projects] states that on the children the independent
    oracle actually produces for a decoded node of one of these programs, the
    target's payload dispatch applied to the projected children equals the
    projected oracle score, and that the target completeness on the projected
    scan state equals the oracle completeness on the oracle scan state.

    Absent and present EVar payloads are recorded after the theorem by
    computed examples on the generated decoder.  A note on scope: the
    compiled oracle does not yet dispatch a score for program type 6, so the
    Var child it really evaluates has no score of its own, and on that child
    the theorem equates [None] with [None].  The example
    [evar_present_empty_var_with_its_score] applies the helpers to that same
    child carrying the score [source_var_score []], and the lemma
    [evar_present_scored_var_constructs_100] states the general case.  The
    proof of the theorem never unfolds the oracle's message score, so it does
    not depend on whether that dispatch exists.  The computed example
    [evar_present_empty_var_as_evaluated] does record the present [None]: it
    is expected to change, and to fail to compile, when the dispatch is added.

    Every theory is referred to by its full logical name and no module alias
    is declared.  The placed target helpers of RPB3SemanticReducer and the oracle
    helpers are imported and used unqualified. *)

From Stdlib Require Import List Bool Arith NArith ZArith.
From StackSafePDA Require RPB3GeneratedFieldRefinement RPB3RawRegionTree
  RPB3SemanticReducer RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission RPB3IndependentOracleRecursive
  RPB3IndependentOracleProjection RPB3IndependentOracleRecursiveProjection
  RPB3IndependentOracleProcessChildProvenance.
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

(** An auxiliary node is neither a message for the target nor a typed message
    for the oracle. *)
Lemma evaluated_auxiliary_child_not_message :
  forall recurse hash_fn body program span field child,
    RPB3IndependentOracleRecursive.source_evaluate_node recurse hash_fn body program
      (RPB3RawRegionTree.SourceAuxiliary span field) = Some child ->
    RPB3GeneratedFieldRefinement.payload_eqb (RPB3GeneratedFieldRefinement.raw_payload (RPB3IndependentOracleCore.source_summary_raw child))
      RPB3GeneratedFieldRefinement.Message = false /\
    (forall raw nested,
      RPB3IndependentOracleCore.source_summary_value child <> RPB3IndependentOracleCore.TypedSourceMessage raw nested).
Proof.
  intros recurse hash_fn body program span field child evaluated.
  unfold RPB3IndependentOracleRecursive.source_evaluate_node in evaluated.
  destruct (RPB3RawRegionTree.raw_field_of_source body (RPB3RawRegionTree.SourceAuxiliary span field))
    as [raw|] eqn:converted; [|discriminate evaluated].
  pose proof (RPB3RawRegionTree.raw_field_of_source_preserves_wire_and_body
    body (RPB3RawRegionTree.SourceAuxiliary span field) raw converted)
    as [_ [_ [raw_payload_is_descriptor _]]].
  cbn [RPB3RawRegionTree.source_field] in raw_payload_is_descriptor.
  destruct (RPB3GeneratedFieldRefinement.descriptor_payload field) eqn:descriptor_kind;
    [discriminate evaluated|discriminate evaluated| |].
  all: finish_auxiliary evaluated.
  all: injection evaluated as same_child; subst child.
  all: cbn [RPB3IndependentOracleCore.source_summary_raw RPB3IndependentOracleCore.source_value_raw
    RPB3IndependentOracleCore.source_summary_value].
  all: rewrite raw_payload_is_descriptor.
  all: split; [reflexivity|intros other nested same_value; discriminate same_value].
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

(** ** Reading one typed child *)

Lemma exact_typed_message_score_not_message :
  forall type_id child,
    RPB3GeneratedFieldRefinement.payload_eqb (RPB3SemanticReducer.sem_payload child) RPB3GeneratedFieldRefinement.Message = false ->
    RPB3SemanticReducer.exact_typed_message_score type_id child = None.
Proof.
  intros type_id child not_message.
  unfold RPB3SemanticReducer.exact_typed_message_score.
  rewrite not_message, andb_false_r.
  reflexivity.
Qed.

Lemma source_typed_message_score_not_message :
  forall type_id child,
    (forall raw nested,
      RPB3IndependentOracleCore.source_summary_value child <> RPB3IndependentOracleCore.TypedSourceMessage raw nested) ->
    source_payload_typed_message_score type_id child = None.
Proof.
  intros type_id child not_message.
  unfold source_payload_typed_message_score.
  destruct (RPB3IndependentOracleCore.source_summary_value child) as
    [raw scalar|raw nested|raw key value|raw snapshot values];
    [reflexivity| |reflexivity|reflexivity].
  exfalso.
  exact (not_message raw nested eq_refl).
Qed.

(** The target's and the oracle's reading of one child as a typed message. *)
Definition typed_score_agrees (type_id : nat)
    (child : RPB3IndependentOracleCore.source_exact_summary) : Prop :=
  RPB3SemanticReducer.exact_typed_message_score type_id
    (RPB3IndependentOracleProjection.project_source_summary child) =
  option_map RPB3IndependentOracleCore.project_score
    (source_payload_typed_message_score type_id child).

(** Every evaluated child reads the same: a scalar or an auxiliary entry is
    no typed message on either side, and a message node is the case proved for
    the Process families. *)
Lemma evaluated_child_typed_score_agrees :
  forall fuel hash_fn body parent_type program node child type_id,
    RPB3GeneratedFieldRefinement.lookup_projected_program parent_type [] [] = Some program ->
    aligned_child fuel hash_fn body program node child ->
    typed_score_agrees type_id child.
Proof.
  intros fuel hash_fn body parent_type program node child type_id looked_up
    [[selected _] evaluated].
  unfold typed_score_agrees.
  destruct node as [span field|span field nested|span field];
    cbn [RPB3RawRegionTree.source_span RPB3RawRegionTree.source_field] in selected.
  - destruct (evaluated_scalar_node_shape _ hash_fn body program span field
      child evaluated) as [raw [scalar [same_raw [_ [_ shape]]]]].
    destruct (RPB3IndependentOracleProcessChildProvenance.converted_scalar_raw_has_scalar_shape body span
      field raw same_raw) as [raw_is_scalar _].
    assert (raw_is : RPB3IndependentOracleCore.source_summary_raw child = raw)
      by (rewrite shape; reflexivity).
    assert (value_is : RPB3IndependentOracleCore.source_summary_value child =
        RPB3IndependentOracleCore.TypedSourceScalar raw scalar) by (rewrite shape; reflexivity).
    assert (target_none :
      RPB3SemanticReducer.exact_typed_message_score type_id
        (RPB3IndependentOracleProjection.project_source_summary child) = None).
    { apply exact_typed_message_score_not_message.
      change (RPB3GeneratedFieldRefinement.payload_eqb
        (RPB3GeneratedFieldRefinement.raw_payload (RPB3IndependentOracleCore.source_summary_raw child))
        RPB3GeneratedFieldRefinement.Message = false).
      rewrite raw_is, raw_is_scalar.
      reflexivity. }
    assert (oracle_none :
      source_payload_typed_message_score type_id child = None).
    { apply source_typed_message_score_not_message.
      intros other nested same_value.
      rewrite value_is in same_value.
      discriminate same_value. }
    rewrite target_none, oracle_none.
    reflexivity.
  - exact (RPB3IndependentOracleProcessChildProvenance.accepted_generated_message_node_typed_score_projects
      fuel hash_fn body parent_type program span field nested child type_id
      looked_up selected evaluated).
  - destruct (evaluated_auxiliary_child_not_message _ hash_fn body program
      span field child evaluated) as [raw_not_message value_not_message].
    rewrite (exact_typed_message_score_not_message type_id
      (RPB3IndependentOracleProjection.project_source_summary child) raw_not_message).
    rewrite (source_typed_message_score_not_message type_id child
      value_not_message).
    reflexivity.
Qed.

Lemma evaluated_children_typed_scores_agree :
  forall fuel hash_fn body parent_type program nodes children type_id,
    RPB3GeneratedFieldRefinement.lookup_projected_program parent_type [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    Forall (typed_score_agrees type_id) children.
Proof.
  intros fuel hash_fn body parent_type program nodes children type_id
    looked_up aligned.
  eapply forall2_right_forall; [|exact aligned].
  intros node child pair.
  exact (evaluated_child_typed_score_agrees fuel hash_fn body parent_type
    program node child type_id looked_up pair).
Qed.

(** ** Reading a typed child by tag *)

Lemma typed_child_score_projects :
  forall tag type_id children,
    Forall (typed_score_agrees type_id) children ->
    RPB3SemanticReducer.exact_typed_child_score tag type_id
      (map RPB3IndependentOracleProjection.project_source_summary children) =
    option_map RPB3IndependentOracleCore.project_score
      (source_payload_typed_child_score tag type_id children).
Proof.
  intros tag type_id children agreement.
  unfold RPB3SemanticReducer.exact_typed_child_score, source_payload_typed_child_score,
    source_payload_child_at.
  rewrite <- RPB3IndependentOracleProjection.projected_find_tag.
  destruct (find (fun child => RPB3IndependentOracleCore.source_summary_tag child =? tag) children)
    as [child|] eqn:found.
  - cbn [option_map].
    apply find_some in found as [member _].
    rewrite Forall_forall in agreement.
    exact (agreement child member).
  - reflexivity.
Qed.

(** A valid oracle scan records the same seen slots as the target scan of the
    projected children. *)
Lemma projected_seen_slot_agrees :
  forall program children slot,
    RPB3IndependentOracleAdmission.source_scan_valid
      (RPB3IndependentOracleAdmission.source_scan_children program children) = true ->
    RPB3SemanticReducer.seen_slot
      (fold_left (RPB3SemanticReducer.scan_child program)
        (map RPB3IndependentOracleProjection.project_source_summary children) RPB3GeneratedFieldRefinement.empty_scan)
      slot =
    RPB3IndependentOracleAdmission.source_seen_slot
      (RPB3IndependentOracleAdmission.source_scan_children program children) slot.
Proof.
  intros program children slot valid.
  destruct (RPB3IndependentOracleProjection.source_scan_children_project_valid program children
    valid) as [_ [_ same_seen]].
  unfold RPB3SemanticReducer.seen_slot, RPB3IndependentOracleAdmission.source_seen_slot.
  rewrite same_seen.
  reflexivity.
Qed.

(** ** The score helpers *)

Lemma evar_score_projects :
  forall children,
    Forall (typed_score_agrees 6) children ->
    exact_evar_score (map RPB3IndependentOracleProjection.project_source_summary children) =
    option_map RPB3IndependentOracleCore.project_score (source_evar_score children).
Proof.
  intros children agreement.
  unfold exact_evar_score, source_evar_score.
  rewrite (typed_child_score_projects 1 6 children agreement).
  destruct (source_payload_typed_child_score 1 6 children) as [score|];
    [|reflexivity].
  cbn [option_map].
  rewrite RPB3IndependentOracleCore.project_source_score_constructor.
  reflexivity.
Qed.

Lemma unary_payload_score_projects :
  forall constructor children,
    Forall (typed_score_agrees 1) children ->
    exact_unary_payload_score constructor
      (map RPB3IndependentOracleProjection.project_source_summary children) =
    option_map RPB3IndependentOracleCore.project_score
      (source_unary_payload_score constructor children).
Proof.
  intros constructor children agreement.
  unfold exact_unary_payload_score, source_unary_payload_score.
  rewrite (typed_child_score_projects 1 1 children agreement).
  destruct (source_payload_typed_child_score 1 1 children) as [score|];
    [|reflexivity].
  cbn [option_map].
  rewrite RPB3IndependentOracleCore.project_source_score_constructor.
  reflexivity.
Qed.

(** ** The bridge theorem *)

Theorem unary_payload_projects :
  forall fuel hash_fn family program body nodes children,
    In family [35; 36; 37] ->
    RPB3GeneratedFieldRefinement.lookup_projected_program family [] [] = Some program ->
    RPB3RawRegionTree.decode_region (S (length body)) body program 0 (length body) =
      Some nodes ->
    RPB3RawRegionTree.sequence_source_results
      (map (RPB3IndependentOracleRecursive.source_evaluate_node
             (RPB3IndependentOracleRecursive.source_evaluate_message fuel hash_fn) hash_fn body program)
        nodes)
      = Some children ->
    RPB3IndependentOracleAdmission.source_scan_valid
      (RPB3IndependentOracleAdmission.source_scan_children program children) = true ->
    exact_expr_payload_score family
      (map RPB3IndependentOracleProjection.project_source_summary children) =
      option_map RPB3IndependentOracleCore.project_score (source_expr_payload_score family children)
    /\
    expr_payload_completeness family
      (fold_left (RPB3SemanticReducer.scan_child program)
        (map RPB3IndependentOracleProjection.project_source_summary children) RPB3GeneratedFieldRefinement.empty_scan)
      (map RPB3IndependentOracleProjection.project_source_summary children) =
    source_expr_payload_completeness family
      (RPB3IndependentOracleAdmission.source_scan_children program children) children.
Proof.
  intros fuel hash_fn family program body nodes children
    family_member looked_up decoded selected scan_is_valid.
  pose proof (decoded_selection_is_aligned fuel hash_fn body program nodes
    children decoded selected) as aligned.
  destruct family_member as [same|[same|[same|[]]]]; subst family.
  - split.
    + exact (evar_score_projects children
        (evaluated_children_typed_scores_agree fuel hash_fn body 35 program
          nodes children 6 looked_up aligned)).
    + cbn [expr_payload_completeness source_expr_payload_completeness].
      rewrite (projected_seen_slot_agrees program children 0 scan_is_valid).
      reflexivity.
  - split.
    + exact (unary_payload_score_projects 112 children
        (evaluated_children_typed_scores_agree fuel hash_fn body 36 program
          nodes children 1 looked_up aligned)).
    + cbn [expr_payload_completeness source_expr_payload_completeness].
      rewrite (projected_seen_slot_agrees program children 0 scan_is_valid).
      reflexivity.
  - split.
    + exact (unary_payload_score_projects 101 children
        (evaluated_children_typed_scores_agree fuel hash_fn body 37 program
          nodes children 1 looked_up aligned)).
    + cbn [expr_payload_completeness source_expr_payload_completeness].
      rewrite (projected_seen_slot_agrees program children 0 scan_is_valid).
      reflexivity.
Qed.

(** ** A scored Var payload

    Whenever the oracle's typed reading of the tag-1 child yields a score, EVar
    is constructor 100 over that score on both sides. *)
Lemma evar_present_scored_var_constructs_100 :
  forall child score,
    RPB3IndependentOracleCore.source_summary_tag child = 1 ->
    source_payload_typed_message_score 6 child = Some score ->
    typed_score_agrees 6 child ->
    exact_evar_score (map RPB3IndependentOracleProjection.project_source_summary [child]) =
      Some (RPB3SemanticReducer.score_constructor 100 [RPB3IndependentOracleCore.project_score score]) /\
    option_map RPB3IndependentOracleCore.project_score (source_evar_score [child]) =
      Some (RPB3SemanticReducer.score_constructor 100 [RPB3IndependentOracleCore.project_score score]).
Proof.
  intros child score tag_is_one scored agreement.
  assert (oracle_value : source_evar_score [child] =
      Some (RPB3IndependentOracleCore.source_score_constructor 100 [score])).
  { unfold source_evar_score, source_payload_typed_child_score,
      source_payload_child_at.
    cbn [find].
    rewrite tag_is_one.
    cbn [Nat.eqb].
    rewrite scored.
    reflexivity. }
  assert (oracle_projected :
      option_map RPB3IndependentOracleCore.project_score (source_evar_score [child]) =
      Some (RPB3SemanticReducer.score_constructor 100 [RPB3IndependentOracleCore.project_score score])).
  { rewrite oracle_value.
    cbn [option_map].
    rewrite RPB3IndependentOracleCore.project_source_score_constructor.
    reflexivity. }
  split; [|exact oracle_projected].
  rewrite (evar_score_projects [child]
    (@Forall_cons _ _ child [] agreement (@Forall_nil _ _))).
  exact oracle_projected.
Qed.

(** ** Computed EVar payload cases on the generated decoder

    Each witness is the family's generated decoder and evaluator run on a
    concrete body.  [hash_fn] is arbitrary since no witness decodes a path
    map, and [fuel] 1 suffices since the evaluated message children have no
    message child of their own. *)

Definition witness_hash_fn (_ : list nat) : nat := 0.

Definition witness_program (family : nat) : RPB3GeneratedFieldRefinement.generated_program :=
  match RPB3GeneratedFieldRefinement.lookup_projected_program family [] [] with
  | Some program => program
  | None => RPB3GeneratedFieldRefinement.empty_program
  end.

Definition witness_nodes (family : nat) (body : list nat)
    : list RPB3RawRegionTree.source_tree :=
  match RPB3RawRegionTree.decode_region (S (length body)) body (witness_program family)
      0 (length body) with
  | Some nodes => nodes
  | None => []
  end.

Definition witness_evaluation (family : nat) (body : list nat)
    : option (list RPB3IndependentOracleCore.source_exact_summary) :=
  RPB3RawRegionTree.sequence_source_results
    (map (RPB3IndependentOracleRecursive.source_evaluate_node
           (RPB3IndependentOracleRecursive.source_evaluate_message 1 witness_hash_fn) witness_hash_fn
           body (witness_program family))
      (witness_nodes family body)).

Definition witness_children (family : nat) (body : list nat)
    : list RPB3IndependentOracleCore.source_exact_summary :=
  match witness_evaluation family body with
  | Some children => children
  | None => []
  end.

Definition target_witness_scan (family : nat) (body : list nat)
    : RPB3GeneratedFieldRefinement.scan_state :=
  fold_left (RPB3SemanticReducer.scan_child (witness_program family))
    (map RPB3IndependentOracleProjection.project_source_summary (witness_children family body))
    RPB3GeneratedFieldRefinement.empty_scan.

(** The witness satisfies every premise of the bridge theorem. *)
Definition witness_is_actual (family : nat) (body : list nat) : Prop :=
  RPB3GeneratedFieldRefinement.lookup_projected_program family [] [] =
    Some (witness_program family) /\
  RPB3RawRegionTree.decode_region (S (length body)) body (witness_program family)
    0 (length body) = Some (witness_nodes family body) /\
  witness_evaluation family body = Some (witness_children family body) /\
  RPB3IndependentOracleAdmission.source_scan_valid
    (RPB3IndependentOracleAdmission.source_scan_children (witness_program family)
      (witness_children family body)) = true.

Definition absent_payload_body : list nat := [].

(** The key byte [(1 << 3) | 2 = 10] and the length [0]: tag 1 is present and
    its message body is empty. *)
Definition present_empty_payload_body : list nat := [10; 0].

Lemma evar_absent_payload_witness_is_actual :
  witness_is_actual 35 absent_payload_body.
Proof.
  unfold witness_is_actual.
  split; [|split; [|split]]; vm_compute; reflexivity.
Qed.

(** An absent payload is incomplete on both sides and unscored on both. *)
Example evar_absent_payload_is_incomplete :
  witness_children 35 absent_payload_body = [] /\
  expr_payload_completeness 35 (target_witness_scan 35 absent_payload_body)
    (map RPB3IndependentOracleProjection.project_source_summary
      (witness_children 35 absent_payload_body)) = Some false /\
  source_expr_payload_completeness 35
    (RPB3IndependentOracleAdmission.source_scan_children (witness_program 35)
      (witness_children 35 absent_payload_body))
    (witness_children 35 absent_payload_body) = Some false /\
  exact_expr_payload_score 35
    (map RPB3IndependentOracleProjection.project_source_summary
      (witness_children 35 absent_payload_body)) = None /\
  source_expr_payload_score 35 (witness_children 35 absent_payload_body) =
    None.
Proof.
  split; [|split; [|split; [|split]]]; vm_compute; reflexivity.
Qed.

Lemma evar_present_empty_var_witness_is_actual :
  witness_is_actual 35 present_empty_payload_body.
Proof.
  unfold witness_is_actual.
  split; [|split; [|split]]; vm_compute; reflexivity.
Qed.

(** The Var child the compiled oracle really produces: tag 1, type 6, not
    refused.  It is complete for its parent; since the compiled oracle gives
    type 6 no score, both score sides are [None]. *)
Example evar_present_empty_var_as_evaluated :
  map RPB3IndependentOracleCore.source_summary_tag (witness_children 35 present_empty_payload_body)
    = [1] /\
  map RPB3IndependentOracleCore.source_summary_type_id
    (witness_children 35 present_empty_payload_body) = [Some 6] /\
  map RPB3IndependentOracleCore.source_summary_refused
    (witness_children 35 present_empty_payload_body) = [false] /\
  expr_payload_completeness 35
    (target_witness_scan 35 present_empty_payload_body)
    (map RPB3IndependentOracleProjection.project_source_summary
      (witness_children 35 present_empty_payload_body)) = Some true /\
  source_expr_payload_completeness 35
    (RPB3IndependentOracleAdmission.source_scan_children (witness_program 35)
      (witness_children 35 present_empty_payload_body))
    (witness_children 35 present_empty_payload_body) = Some true /\
  exact_expr_payload_score 35
    (map RPB3IndependentOracleProjection.project_source_summary
      (witness_children 35 present_empty_payload_body)) = None /\
  source_expr_payload_score 35
    (witness_children 35 present_empty_payload_body) = None.
Proof.
  split; [|split; [|split; [|split; [|split; [|split]]]]];
    vm_compute; reflexivity.
Qed.

(** The child above with the score its own payload has, [source_var_score []].
    Every other field is what the generated decoder and evaluator produced. *)
Definition with_empty_var_score (child : RPB3IndependentOracleCore.source_exact_summary)
    : RPB3IndependentOracleCore.source_exact_summary :=
  RPB3IndependentOracleCore.SourceExactSummary (RPB3IndependentOracleCore.source_summary_value child)
    (RPB3IndependentOracleCore.SourceFamilyAnalysis
      (RPB3IndependentOracleCore.source_family_complete (RPB3IndependentOracleCore.source_summary_analysis child))
      (source_var_score [])
      (RPB3IndependentOracleCore.source_family_refused (RPB3IndependentOracleCore.source_summary_analysis child))
      (RPB3IndependentOracleCore.source_family_bind_source_present
        (RPB3IndependentOracleCore.source_summary_analysis child))
      (RPB3IndependentOracleCore.source_family_case_required_present
        (RPB3IndependentOracleCore.source_summary_analysis child)))
    (RPB3IndependentOracleCore.source_summary_semantic_default child).

Definition scored_present_empty_var_children
    : list RPB3IndependentOracleCore.source_exact_summary :=
  map with_empty_var_score (witness_children 35 present_empty_payload_body).

(** A present empty Var is complete and is constructor 100 over the integer 0,
    on both sides.  Both scan states are computed from the scored children. *)
Example evar_present_empty_var_with_its_score :
  RPB3IndependentOracleAdmission.source_scan_valid
    (RPB3IndependentOracleAdmission.source_scan_children (witness_program 35)
      scored_present_empty_var_children) = true /\
  expr_payload_completeness 35
    (fold_left (RPB3SemanticReducer.scan_child (witness_program 35))
      (map RPB3IndependentOracleProjection.project_source_summary
        scored_present_empty_var_children) RPB3GeneratedFieldRefinement.empty_scan)
    (map RPB3IndependentOracleProjection.project_source_summary
      scored_present_empty_var_children) = Some true /\
  source_expr_payload_completeness 35
    (RPB3IndependentOracleAdmission.source_scan_children (witness_program 35)
      scored_present_empty_var_children)
    scored_present_empty_var_children = Some true /\
  exact_expr_payload_score 35
    (map RPB3IndependentOracleProjection.project_source_summary
      scored_present_empty_var_children) =
    Some (RPB3SemanticReducer.score_constructor 100 [RPB3SemanticReducer.ScoreInteger 0]) /\
  option_map RPB3IndependentOracleCore.project_score
    (source_expr_payload_score 35 scored_present_empty_var_children) =
    Some (RPB3SemanticReducer.score_constructor 100 [RPB3SemanticReducer.ScoreInteger 0]).
Proof.
  split; [|split; [|split; [|split]]]; vm_compute; reflexivity.
Qed.

(** ** Red control: swapping the ENot and ENeg constructors

    The mutant dispatch differs from [exact_expr_payload_score] only at 36 and
    37.  The witness is ENot over a present empty Par, whose own score is
    constructor 999 over the absent connective flag. *)

Definition swapped_unary_constructor_score (family : nat)
    (children : list RPB3SemanticReducer.semantic_summary)
    : option RPB3SemanticReducer.canonical_score :=
  match family with
  | 36 => exact_unary_payload_score 101 children
  | 37 => exact_unary_payload_score 112 children
  | _ => exact_expr_payload_score family children
  end.

Definition empty_par_score : RPB3SemanticReducer.canonical_score :=
  RPB3SemanticReducer.score_constructor 999 [RPB3SemanticReducer.ScoreInteger 0].

Lemma enot_witness_is_actual :
  witness_is_actual 36 present_empty_payload_body.
Proof.
  unfold witness_is_actual.
  split; [|split; [|split]]; vm_compute; reflexivity.
Qed.

(** The mutant and the projected oracle disagree on the witness, while the
    real target agrees with the oracle. *)
Example swapped_unary_constructors_disagree_with_oracle :
  swapped_unary_constructor_score 36
    (map RPB3IndependentOracleProjection.project_source_summary
      (witness_children 36 present_empty_payload_body)) =
    Some (RPB3SemanticReducer.score_constructor 101 [empty_par_score]) /\
  option_map RPB3IndependentOracleCore.project_score
    (source_expr_payload_score 36
      (witness_children 36 present_empty_payload_body)) =
    Some (RPB3SemanticReducer.score_constructor 112 [empty_par_score]) /\
  exact_expr_payload_score 36
    (map RPB3IndependentOracleProjection.project_source_summary
      (witness_children 36 present_empty_payload_body)) =
    Some (RPB3SemanticReducer.score_constructor 112 [empty_par_score]) /\
  swapped_unary_constructor_score 36
    (map RPB3IndependentOracleProjection.project_source_summary
      (witness_children 36 present_empty_payload_body)) <>
  option_map RPB3IndependentOracleCore.project_score
    (source_expr_payload_score 36
      (witness_children 36 present_empty_payload_body)).
Proof.
  split; [|split; [|split]].
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. intro same. discriminate same.
Qed.

(** Hence the bridge statement is false of the mutant. *)
Theorem swapped_unary_constructors_bridge_is_false :
  ~ (forall fuel hash_fn family program body nodes children,
      In family [35; 36; 37] ->
      RPB3GeneratedFieldRefinement.lookup_projected_program family [] [] = Some program ->
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
      swapped_unary_constructor_score family
        (map RPB3IndependentOracleProjection.project_source_summary children) =
      option_map RPB3IndependentOracleCore.project_score
        (source_expr_payload_score family children)).
Proof.
  intro bridge.
  destruct enot_witness_is_actual as [looked_up [decoded [selected scan]]].
  pose proof (bridge 1 witness_hash_fn 36 (witness_program 36)
    present_empty_payload_body
    (witness_nodes 36 present_empty_payload_body)
    (witness_children 36 present_empty_payload_body)
    (or_intror (or_introl eq_refl)) looked_up decoded selected scan)
    as disagreement.
  vm_compute in disagreement.
  discriminate disagreement.
Qed.

Print Assumptions forall2_right_forall.
Print Assumptions evaluated_scalar_node_shape.
Print Assumptions evaluated_auxiliary_child_not_message.
Print Assumptions decoded_selection_is_aligned.
Print Assumptions exact_typed_message_score_not_message.
Print Assumptions source_typed_message_score_not_message.
Print Assumptions evaluated_child_typed_score_agrees.
Print Assumptions evaluated_children_typed_scores_agree.
Print Assumptions typed_child_score_projects.
Print Assumptions projected_seen_slot_agrees.
Print Assumptions evar_score_projects.
Print Assumptions unary_payload_score_projects.
Print Assumptions unary_payload_projects.
Print Assumptions evar_present_scored_var_constructs_100.
Print Assumptions evar_absent_payload_witness_is_actual.
Print Assumptions evar_absent_payload_is_incomplete.
Print Assumptions evar_present_empty_var_witness_is_actual.
Print Assumptions evar_present_empty_var_as_evaluated.
Print Assumptions evar_present_empty_var_with_its_score.
Print Assumptions enot_witness_is_actual.
Print Assumptions swapped_unary_constructors_disagree_with_oracle.
Print Assumptions swapped_unary_constructors_bridge_is_false.
