(** Bridge theorem for the binary expression payloads, program types 38 through
    54 (leaf L05 of the score-arms design, sections
    2 and 5).

    The 17 families share one constructor table and one lemma over it,
    [binary_family_projects_by_table].  [binary_payload_projects] is its
    corollary for the family list.  Each states that on the children the
    independent oracle actually produces for a decoded node of the family, the
    target's payload dispatch applied to the projected children equals the
    projected oracle score, and that the target completeness on the projected
    scan state equals the oracle completeness on the oracle scan state.

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

(** ** The score helper for one constructor *)

Lemma binary_payload_score_projects :
  forall constructor children,
    Forall (typed_score_agrees 1) children ->
    exact_binary_payload_score constructor
      (map RPB3IndependentOracleProjection.project_source_summary children) =
    option_map RPB3IndependentOracleCore.project_score
      (source_binary_payload_score constructor children).
Proof.
  intros constructor children agreement.
  unfold exact_binary_payload_score, source_binary_payload_score.
  rewrite (typed_child_score_projects 1 1 children agreement).
  rewrite (typed_child_score_projects 2 1 children agreement).
  destruct (source_payload_typed_child_score 1 1 children) as [first|];
    [|reflexivity].
  destruct (source_payload_typed_child_score 2 1 children) as [second|];
    [|reflexivity].
  cbn [option_map].
  rewrite RPB3IndependentOracleCore.project_source_score_constructor.
  reflexivity.
Qed.

(** ** The constructor table *)

Definition binary_constructor_table : list (nat * N) :=
  [(38, 102%N); (39, 103%N); (40, 122%N); (41, 104%N); (42, 105%N);
   (43, 106%N); (44, 107%N); (45, 108%N); (46, 109%N); (47, 110%N);
   (48, 111%N); (49, 113%N); (50, 114%N); (51, 118%N); (52, 119%N);
   (53, 120%N); (54, 121%N)].

(** Each row of the table is what both dispatches compute for that family, and
    both completeness rules require the two operand slots. *)
Lemma binary_table_dispatch :
  forall family constructor,
    In (family, constructor) binary_constructor_table ->
    (forall children,
      exact_expr_payload_score family children =
      exact_binary_payload_score constructor children) /\
    (forall children,
      source_expr_payload_score family children =
      source_binary_payload_score constructor children) /\
    (forall state children,
      expr_payload_completeness family state children =
      Some (RPB3SemanticReducer.seen_slot state 0 && RPB3SemanticReducer.seen_slot state 1)) /\
    (forall state children,
      source_expr_payload_completeness family state children =
      Some (RPB3IndependentOracleAdmission.source_seen_slot state 0 &&
        RPB3IndependentOracleAdmission.source_seen_slot state 1)).
Proof.
  intros family constructor member.
  unfold binary_constructor_table in member.
  repeat (destruct member as [same|member];
    [inversion same; subst;
     split; [|split; [|split]]; intros; reflexivity|]).
  destruct member.
Qed.

(** ** The bridge, as one lemma over the table *)

Lemma binary_family_projects_by_table :
  forall family constructor,
    In (family, constructor) binary_constructor_table ->
    forall fuel hash_fn program body nodes children,
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
      exact_expr_payload_score family
        (map RPB3IndependentOracleProjection.project_source_summary children) =
        option_map RPB3IndependentOracleCore.project_score
          (source_expr_payload_score family children)
      /\
      expr_payload_completeness family
        (fold_left (RPB3SemanticReducer.scan_child program)
          (map RPB3IndependentOracleProjection.project_source_summary children) RPB3GeneratedFieldRefinement.empty_scan)
        (map RPB3IndependentOracleProjection.project_source_summary children) =
      source_expr_payload_completeness family
        (RPB3IndependentOracleAdmission.source_scan_children program children) children.
Proof.
  intros family constructor row fuel hash_fn program body nodes children
    looked_up decoded selected scan_is_valid.
  destruct (binary_table_dispatch family constructor row)
    as [target_dispatch [oracle_dispatch [target_complete oracle_complete]]].
  pose proof (evaluated_children_typed_scores_agree fuel hash_fn body family
    program nodes children 1 looked_up
    (decoded_selection_is_aligned fuel hash_fn body program nodes children
      decoded selected)) as agreement.
  split.
  - rewrite target_dispatch, oracle_dispatch.
    exact (binary_payload_score_projects constructor children agreement).
  - rewrite target_complete, oracle_complete.
    rewrite (projected_seen_slot_agrees program children 0 scan_is_valid),
      (projected_seen_slot_agrees program children 1 scan_is_valid).
    reflexivity.
Qed.

(** ** The bridge theorem *)

Theorem binary_payload_projects :
  forall fuel hash_fn family program body nodes children,
    In family [38; 39; 40; 41; 42; 43; 44; 45; 46; 47; 48; 49; 50; 51; 52; 53;
      54] ->
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
  intros fuel hash_fn family program body nodes children family_member.
  change (In family (map fst binary_constructor_table)) in family_member.
  apply in_map_iff in family_member
    as [[row_family constructor] [same_family row]].
  cbn [fst] in same_family.
  subst row_family.
  exact (binary_family_projects_by_table family constructor row
    fuel hash_fn program body nodes children).
Qed.

(** ** Computed cases on the generated decoder

    Each witness is the family's generated decoder and evaluator run on a
    concrete body.  [hash_fn] is arbitrary since no witness decodes a path
    map, and [fuel] 1 suffices since the evaluated Par operands have no
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

(** The key byte [(1 << 3) | 2 = 10] and the length [0]: the first operand is
    present and is the empty Par; the second operand is absent. *)
Definition missing_second_operand_body : list nat := [10; 0].

Lemma missing_second_operand_witness_is_actual :
  witness_is_actual 41 missing_second_operand_body.
Proof.
  unfold witness_is_actual.
  split; [|split; [|split]]; vm_compute; reflexivity.
Qed.

(** A body with one operand is incomplete and unscored on both sides. *)
Example missing_second_operand_is_incomplete :
  expr_payload_completeness 41
    (target_witness_scan 41 missing_second_operand_body)
    (map RPB3IndependentOracleProjection.project_source_summary
      (witness_children 41 missing_second_operand_body)) = Some false /\
  source_expr_payload_completeness 41
    (RPB3IndependentOracleAdmission.source_scan_children (witness_program 41)
      (witness_children 41 missing_second_operand_body))
    (witness_children 41 missing_second_operand_body) = Some false /\
  exact_expr_payload_score 41
    (map RPB3IndependentOracleProjection.project_source_summary
      (witness_children 41 missing_second_operand_body)) = None /\
  source_expr_payload_score 41
    (witness_children 41 missing_second_operand_body) = None.
Proof.
  split; [|split; [|split]]; vm_compute; reflexivity.
Qed.

(** ** Red control: swapping the operands

    The mutant differs from [exact_binary_payload_score] only in the order of
    the two operand scores, and is dispatched through the same table.  The
    witness is family 41 with two operands that score differently: the empty
    Par at tag 1, and at tag 2 (key [(2 << 3) | 2 = 18], length [2]) the Par
    whose body sets [connective_used] (key [(10 << 3) | 0 = 80], value [1]). *)

Definition swapped_operands_binary_score (constructor : N)
    (children : list RPB3SemanticReducer.semantic_summary)
    : option RPB3SemanticReducer.canonical_score :=
  match RPB3SemanticReducer.exact_typed_child_score 1 1 children,
      RPB3SemanticReducer.exact_typed_child_score 2 1 children with
  | Some first, Some second =>
      Some (RPB3SemanticReducer.score_constructor constructor [second; first])
  | _, _ => None
  end.

Definition swapped_operands_payload_score (family : nat)
    (children : list RPB3SemanticReducer.semantic_summary)
    : option RPB3SemanticReducer.canonical_score :=
  match find (fun row => fst row =? family) binary_constructor_table with
  | Some row => swapped_operands_binary_score (snd row) children
  | None => None
  end.

Definition operand_pair_body : list nat := [10; 0; 18; 2; 80; 1].

Definition first_operand_score : RPB3SemanticReducer.canonical_score :=
  RPB3SemanticReducer.score_constructor 999 [RPB3SemanticReducer.ScoreInteger 0].

Definition second_operand_score : RPB3SemanticReducer.canonical_score :=
  RPB3SemanticReducer.score_constructor 999 [RPB3SemanticReducer.ScoreInteger 1].

Lemma operand_pair_witness_is_actual :
  witness_is_actual 41 operand_pair_body.
Proof.
  unfold witness_is_actual.
  split; [|split; [|split]]; vm_compute; reflexivity.
Qed.

(** Both operands present: complete on both sides.  The mutant and the
    projected oracle disagree on the witness, while the real target agrees
    with the oracle. *)
Example swapped_operands_disagree_with_oracle :
  expr_payload_completeness 41 (target_witness_scan 41 operand_pair_body)
    (map RPB3IndependentOracleProjection.project_source_summary
      (witness_children 41 operand_pair_body)) = Some true /\
  source_expr_payload_completeness 41
    (RPB3IndependentOracleAdmission.source_scan_children (witness_program 41)
      (witness_children 41 operand_pair_body))
    (witness_children 41 operand_pair_body) = Some true /\
  swapped_operands_payload_score 41
    (map RPB3IndependentOracleProjection.project_source_summary
      (witness_children 41 operand_pair_body)) =
    Some (RPB3SemanticReducer.score_constructor 104
      [second_operand_score; first_operand_score]) /\
  option_map RPB3IndependentOracleCore.project_score
    (source_expr_payload_score 41 (witness_children 41 operand_pair_body)) =
    Some (RPB3SemanticReducer.score_constructor 104
      [first_operand_score; second_operand_score]) /\
  exact_expr_payload_score 41
    (map RPB3IndependentOracleProjection.project_source_summary
      (witness_children 41 operand_pair_body)) =
    Some (RPB3SemanticReducer.score_constructor 104
      [first_operand_score; second_operand_score]) /\
  swapped_operands_payload_score 41
    (map RPB3IndependentOracleProjection.project_source_summary
      (witness_children 41 operand_pair_body)) <>
  option_map RPB3IndependentOracleCore.project_score
    (source_expr_payload_score 41 (witness_children 41 operand_pair_body)).
Proof.
  split; [|split; [|split; [|split; [|split]]]].
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. intro same. discriminate same.
Qed.

(** Hence the bridge statement is false of the mutant. *)
Theorem swapped_operands_bridge_is_false :
  ~ (forall fuel hash_fn family program body nodes children,
      In family [38; 39; 40; 41; 42; 43; 44; 45; 46; 47; 48; 49; 50; 51; 52;
        53; 54] ->
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
      swapped_operands_payload_score family
        (map RPB3IndependentOracleProjection.project_source_summary children) =
      option_map RPB3IndependentOracleCore.project_score
        (source_expr_payload_score family children)).
Proof.
  intro bridge.
  destruct operand_pair_witness_is_actual
    as [looked_up [decoded [selected scan]]].
  pose proof (bridge 1 witness_hash_fn 41 (witness_program 41)
    operand_pair_body (witness_nodes 41 operand_pair_body)
    (witness_children 41 operand_pair_body)
    (or_intror (or_intror (or_intror (or_introl eq_refl))))
    looked_up decoded selected scan) as disagreement.
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
Print Assumptions binary_payload_score_projects.
Print Assumptions binary_table_dispatch.
Print Assumptions binary_family_projects_by_table.
Print Assumptions binary_payload_projects.
Print Assumptions missing_second_operand_witness_is_actual.
Print Assumptions missing_second_operand_is_incomplete.
Print Assumptions operand_pair_witness_is_actual.
Print Assumptions swapped_operands_disagree_with_oracle.
Print Assumptions swapped_operands_bridge_is_false.
