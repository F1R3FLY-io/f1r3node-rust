(** * Assembly of the oracle message summary against the target reducer

    [Oracle.source_message_summary] computes a summary from a raw field, its
    generated program and the child summaries; [Target.reduce_semantic_field]
    reduces the projected children.  This theory compares the two on the
    children the oracle actually produces.

    The comparison requested for this theory, an unconditional equality of the
    two [SemanticSummary] records, is false.  The oracle refuses a generated
    [New] message (family 20) whose string-map entries appear out of key order
    ([Oracle.source_string_map_keys_ordered]); the target reducer has no such
    check.  [actual_unrefused_children_do_not_project_to_reducer_without_key_order]
    below is the closed refutation over a concrete body with two entries in
    descending key order.  The refusal bit is the only component that differs:
    the file proves [source_message_summary_up_to_map_key_order], which holds
    without any ordering premise and adds the oracle's key-order refusal to the
    target's, and the theorem
    [source_message_summary_projects_to_reducer_given_family_results] with the
    single extra premise
    [type_id = 20 -> Oracle.source_string_map_keys_ordered children = true].
    Family 20 is the only family the premise constrains: the generated table
    declares string-map fields nowhere else, so
    [actual_children_of_non_new_family_are_ordered] discharges the key order
    for every other family and
    [source_message_summary_projects_to_reducer_outside_new_family] is the
    requested equation with [type_id <> 20] in place of any ordering premise.
    [source_message_summary_equals_reducer_iff_map_keys_ordered_or_target_refuses]
    shows that the premise is minimal: on actual children the two summaries are
    equal exactly when the keys are ordered or the target already refuses.

    Structure: the three family-result statements of the assembly; the
    decomposition of both records; one family-agreement lemma per family group
    (Par, Cost, Process, Name, unscored); one lemma per record component; the
    certificate that only family 20 carries string-map children; the assembled
    theorems; and the closed refutation. *)

From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree RPB3SemanticReducer
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3IndependentOracleCost RPB3IndependentOracleProcess
  RPB3IndependentOracleName RPB3IndependentOracleRecursive
  RPB3IndependentOracleProjection RPB3IndependentOracleProcessProjection
  RPB3IndependentOracleNameProjection RPB3IndependentOracleRecursiveProjection
  RPB3IndependentOracleProcessChildProvenance
  RPB3IndependentOracleProcessScalarDecoderBridge
  RPB3IndependentOracleProcessChildCorrespondence
  RPB3IndependentOracleParOrderBridge
  RPB3IndependentOracleParScalarScoreBridge
  RPB3IndependentOracleUnscoredFamilyBridge
  RPB3IndependentOracleCostVerdictBridge
  RPB3IndependentOracleCostFamilyBridge.
From StackSafePDA Require Import RPB3IndependentOracleNameFamilyBridge
  RPB3IndependentOracleProcessChildAssembly.
From StackSafePDA Require RPB3IndependentOracleExprPayload
  RPB3IndependentOracleExprPayloadAgreement.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Target := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Oracle := RPB3IndependentOracleRecursive.
Module Process := RPB3IndependentOracleProcess.
Module Name := RPB3IndependentOracleName.
Module Projection := RPB3IndependentOracleProjection.
Module ProcessProjection := RPB3IndependentOracleProcessProjection.
Module Correspondence := RPB3IndependentOracleProcessChildCorrespondence.

(** ** The statements of the assembly *)

(** The children the oracle produces for one body of one program: the program
    decodes the body, the evaluator reads every decoded node and the source
    scan accepts the result. *)
Definition actual_unrefused_children (program : Schema.generated_program)
    (children : list Core.source_exact_summary) : Prop :=
  exists fuel hash_fn body nodes,
    Source.decode_region (S (length body)) body program 0 (length body) =
      Some nodes /\
    Source.sequence_source_results
      (map (Oracle.source_evaluate_node
             (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program)
        nodes) = Some children /\
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = true.

(** The per-family correspondence of the process families, stated as a premise
    because it is proved by a separate theory. *)
Definition process_assembly_statement : Prop :=
  forall fuel hash_fn family program body children,
    Correspondence.process_actual_children fuel hash_fn family program body
      children ->
    ProcessProjection.process_child_correspondence family children.

(** The score agreement of the name families, stated as a premise for the same
    reason. *)
Definition name_family_statement : Prop :=
  forall fuel hash_fn family program body nodes children,
    In family [31; 32; 55; 60] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Source.decode_region (S (length body)) body program 0 (length body) =
      Some nodes ->
    Source.sequence_source_results
      (map (Oracle.source_evaluate_node
             (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program)
        nodes) = Some children ->
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = true ->
    Target.score_evidence_for family
      (Oracle.source_message_completeness family
        (Admission.source_scan_children program children) children)
      (map Projection.project_source_summary children) =
    Projection.project_source_evidence
      (Name.source_name_family_score family
        (Oracle.source_message_completeness family
          (Admission.source_scan_children program children) children)
        children).

Local Notation projected children :=
  (map Projection.project_source_summary children) (only parsing).
Local Notation source_scan program children :=
  (Admission.source_scan_children program children) (only parsing).
Local Notation target_scan program children :=
  (fold_left (Target.scan_child program)
    (map Projection.project_source_summary children) Schema.empty_scan)
  (only parsing).

(** ** The two records as explicit constructor applications *)

Lemma raw_bytes_well_formed_is_valid_raw_body :
  forall raw,
    Admission.source_raw_bytes_well_formed raw = Schema.valid_raw_body raw.
Proof. reflexivity. Qed.

(** The oracle's summary, projected, field by field.  All of the following
    equations hold by unfolding only. *)
Lemma projected_source_message_summary :
  forall raw program children,
    Projection.project_source_summary
      (Oracle.source_message_summary raw program children) =
    Target.SemanticSummary (Schema.raw_tag raw) (Schema.raw_wire raw)
      (Schema.raw_payload raw) (Projection.project_source_type_id raw)
      (Schema.raw_body raw)
      (negb (Admission.source_raw_bytes_well_formed raw) ||
       negb (Admission.source_scan_valid (source_scan program children)) ||
       negb (Admission.source_required_pathmaps_present program
         (source_scan program children)) ||
       negb (Oracle.source_string_map_keys_ordered children) ||
       Oracle.source_message_extra_refused (Schema.program_family program)
         (Oracle.source_message_completeness (Schema.program_family program)
           (source_scan program children) children)
         (source_scan program children) children)
      (Oracle.source_message_completeness (Schema.program_family program)
        (source_scan program children) children)
      (Projection.project_source_evidence
        (Oracle.source_message_score (Schema.program_family program)
          (Oracle.source_message_completeness (Schema.program_family program)
            (source_scan program children) children) children))
      (if Schema.program_family program =? 16
       then Process.source_bind_source_present (source_scan program children)
       else false)
      (if Schema.program_family program =? 21
       then Process.source_case_required_present (source_scan program children)
       else false).
Proof. reflexivity. Qed.

(** The target's reduction of a resolved message field, field by field, with
    the two guards that hold for a resolved [Message] field left as the literal
    [negb true]. *)
Lemma reduce_semantic_field_resolved :
  forall raw type_id program children,
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some type_id ->
    Schema.lookup_projected_program type_id [] [] = Some program ->
    Schema.program_family program = type_id ->
    Target.reduce_semantic_field raw children =
    Target.SemanticSummary (Schema.raw_tag raw) (Schema.raw_wire raw)
      (Schema.raw_payload raw) type_id (Schema.raw_body raw)
      (negb true || negb (Schema.valid_raw_body raw) || negb true ||
       negb (Schema.scan_valid
         (fold_left (Target.scan_child program) children Schema.empty_scan)) ||
       negb (Target.mandatory_pathmaps_present program
         (fold_left (Target.scan_child program) children Schema.empty_scan)) ||
       Target.order_definitely_bad
         (if type_id =? 1 then Target.raw_par_component_order children
          else if (type_id =? 20) &&
                  Target.seen_slot
                    (fold_left (Target.scan_child program) children
                      Schema.empty_scan) 1
          then Some (Target.new_uri_ordered children)
          else Some true) ||
       Target.complete_receive_has_default_condition type_id
         (Target.component_completeness type_id
           (fold_left (Target.scan_child program) children Schema.empty_scan)
           children) children ||
       Target.complete_match_has_default_guard type_id
         (Target.component_completeness type_id
           (fold_left (Target.scan_child program) children Schema.empty_scan)
           children) children ||
       Target.payload_fixed_point_violated type_id
         (Target.component_completeness type_id
           (fold_left (Target.scan_child program) children Schema.empty_scan)
           children) children)
      (Target.component_completeness type_id
        (fold_left (Target.scan_child program) children Schema.empty_scan)
        children)
      (Target.score_evidence_for type_id
        (Target.component_completeness type_id
          (fold_left (Target.scan_child program) children Schema.empty_scan)
          children) children)
      (if type_id =? 16
       then Target.seen_slot
         (fold_left (Target.scan_child program) children Schema.empty_scan) 1
       else false)
      (if type_id =? 21
       then Target.seen_slot
              (fold_left (Target.scan_child program) children Schema.empty_scan)
              0 &&
            Target.seen_slot
              (fold_left (Target.scan_child program) children Schema.empty_scan)
              1
       else false).
Proof.
  intros raw type_id program children payload program_id looked_up same_family.
  assert (resolved : Target.generated_wire_program raw = Some program).
  { unfold Target.generated_wire_program.
    rewrite payload, program_id.
    exact looked_up. }
  unfold Target.reduce_semantic_field.
  rewrite resolved.
  cbn iota.
  rewrite same_family, payload.
  reflexivity.
Qed.

(** Two summaries are equal when every component is equal. *)
Lemma semantic_summary_components_equal :
  forall left right : Target.semantic_summary,
    Target.sem_tag left = Target.sem_tag right ->
    Target.sem_wire left = Target.sem_wire right ->
    Target.sem_payload left = Target.sem_payload right ->
    Target.sem_type_id left = Target.sem_type_id right ->
    Target.sem_body left = Target.sem_body right ->
    Target.sem_definitely_refused left =
      Target.sem_definitely_refused right ->
    Target.sem_complete left = Target.sem_complete right ->
    Target.sem_score_evidence left = Target.sem_score_evidence right ->
    Target.sem_bind_source_present left =
      Target.sem_bind_source_present right ->
    Target.sem_case_required_present left =
      Target.sem_case_required_present right ->
    left = right.
Proof.
  intros [tag wire payload type_id body refused complete score bind case_required]
    [tag' wire' payload' type_id' body' refused' complete' score' bind'
      case_required'];
    cbn [Target.sem_tag Target.sem_wire Target.sem_payload Target.sem_type_id
      Target.sem_body Target.sem_definitely_refused Target.sem_complete
      Target.sem_score_evidence Target.sem_bind_source_present
      Target.sem_case_required_present];
    intros ? ? ? ? ? ? ? ? ? ?; subst; reflexivity.
Qed.

Lemma required_pathmaps_agree :
  forall program source_state target_state,
    Admission.source_scan_seen_slots source_state =
      Schema.scan_seen target_state ->
    Admission.source_required_pathmaps_present program source_state =
    Target.mandatory_pathmaps_present program target_state.
Proof.
  intros program source_state target_state same_seen.
  unfold Admission.source_required_pathmaps_present,
    Target.mandatory_pathmaps_present,
    Admission.source_seen_slot, Target.seen_slot.
  rewrite same_seen.
  induction (Schema.program_fields program) as [|descriptor rest IH];
    [reflexivity|].
  cbn [forallb].
  rewrite IH.
  destruct (Schema.descriptor_payload descriptor); reflexivity.
Qed.

(** ** What each family group must supply *)

(** The three facts that each family supplies for a pair of scan states.  The
    completeness of the target is rewritten to the oracle's in the other two
    fields, because [family_completeness_agrees] equates them. *)
Record family_agreement (type_id : nat)
    (scan : Admission.source_admission_state)
    (scanned : Schema.scan_state)
    (children : list Core.source_exact_summary) : Prop := {
  family_completeness_agrees :
    Oracle.source_message_completeness type_id scan children =
      Target.component_completeness type_id scanned (projected children);
  family_score_agrees :
    Projection.project_source_evidence
      (Oracle.source_message_score type_id
        (Oracle.source_message_completeness type_id scan children) children) =
    Target.score_evidence_for type_id
      (Oracle.source_message_completeness type_id scan children)
      (projected children);
  family_refusal_agrees :
    Oracle.source_message_extra_refused type_id
      (Oracle.source_message_completeness type_id scan children) scan children =
    (Target.order_definitely_bad
       (if type_id =? 1 then Target.raw_par_component_order (projected children)
        else if (type_id =? 20) && Target.seen_slot scanned 1
        then Some (Target.new_uri_ordered (projected children))
        else Some true) ||
     Target.complete_receive_has_default_condition type_id
       (Oracle.source_message_completeness type_id scan children)
       (projected children) ||
     Target.complete_match_has_default_guard type_id
       (Oracle.source_message_completeness type_id scan children)
       (projected children) ||
     Target.payload_fixed_point_violated type_id
       (Oracle.source_message_completeness type_id scan children)
       (projected children))
}.

(** ** Family group: Par (family 1) *)

(** The reader of one scalar tag on any children produced by evaluating decoded
    nodes.  The statement generalizes the process-family corollary in
    [RPB3IndependentOracleProcessChildCorrespondence] to any program; only the
    contract of the read tag matters.  For the Par connective it is used at
    tag 10, whose contract is the default-forbidden [PBBool]. *)
Lemma actual_children_scalar_true_corresponds :
  forall fuel hash_fn family program body nodes children tag,
    Schema.program_family program = family ->
    Source.sequence_source_results
      (map (Oracle.source_evaluate_node
             (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program)
        nodes) = Some children ->
    Correspondence.scalar_true_faithful_contract
      (Admission.source_scalar_contract_for family tag) = true ->
    Target.scalar_true_or_absent tag
      (ProcessProjection.projected_process_children children) =
    Process.source_bool_or_absent tag children.
Proof.
  intros fuel hash_fn family program body nodes children tag same_family
    sequenced faithful.
  rewrite Correspondence.scalar_true_or_absent_selected_child,
    Correspondence.source_bool_or_absent_selected_child.
  destruct (Process.source_child_at tag children) as [child|] eqn:selected;
    [|reflexivity].
  change (Correspondence.target_scalar_true_view child =
    Correspondence.source_bool_view child).
  unfold Process.source_child_at in selected.
  apply find_some in selected as [member tag_match].
  apply Nat.eqb_eq in tag_match.
  destruct (Correspondence.child_of_sequenced_nodes_has_node _ _
      (Oracle.source_evaluate_node
        (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program)
      nodes children child sequenced member) as [node [_ evaluated]].
  destruct (Correspondence.evaluated_node_cases fuel hash_fn body program node
      child evaluated)
    as [[raw [scalar [typed [unrefused [payload accepted]]]]]
       |[not_scalar not_value]].
  - destruct (Correspondence.source_scalar_value_gate program raw scalar accepted)
      as [contract [wire [contract_found [wire_found [wire_same decoded]]]]].
    assert (raw_tag_is_selected : Schema.raw_tag raw = tag).
    { rewrite <- tag_match.
      unfold Core.source_summary_tag, Core.source_summary_raw.
      rewrite typed.
      reflexivity. }
    rewrite same_family, raw_tag_is_selected in contract_found.
    rewrite contract_found in faithful.
    cbn [Correspondence.scalar_true_faithful_contract] in faithful.
    rewrite (Correspondence.target_scalar_true_view_of_scalar child raw scalar
      typed payload).
    rewrite (Correspondence.source_bool_view_of_scalar child raw scalar typed
      unrefused).
    exact (Correspondence.accepted_scalar_value_matches_scalar_true _ _ _ _
      faithful decoded).
  - destruct (Correspondence.non_scalar_child_views_are_absent child not_scalar
      not_value) as [true_view [_ source_view]].
    rewrite true_view, source_view.
    reflexivity.
Qed.

Lemma aligned_par_children_are_plain :
  forall fuel hash_fn body nodes children,
    Forall2 (fun node child =>
      Source.source_node_matches_generated_descriptor
        Source.generated_par_source_program node /\
      Oracle.source_evaluate_node
        (Oracle.source_evaluate_message fuel hash_fn) hash_fn body
        Source.generated_par_source_program node = Some child)
      nodes children ->
    Forall RPB3IndependentOracleParScalarScoreBridge.plain_par_child children.
Proof.
  intros fuel hash_fn body nodes children aligned.
  induction aligned as
    [|node child rest_nodes rest_children [matched evaluated] rest IH];
    constructor.
  - exact (RPB3IndependentOracleParScalarScoreBridge.actual_generated_par_node_is_plain
      fuel hash_fn body node child matched evaluated).
  - exact IH.
Qed.

Lemma par_family_agreement :
  forall fuel hash_fn body nodes program children,
    Schema.lookup_projected_program 1 [] [] = Some program ->
    Source.decode_region (S (length body)) body program 0 (length body) =
      Some nodes ->
    Source.sequence_source_results
      (map (Oracle.source_evaluate_node
             (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program)
        nodes) = Some children ->
    family_agreement 1 (source_scan program children) (target_scan program children)
      children.
Proof.
  intros fuel hash_fn body nodes program children looked_up decoded sequenced.
  assert (program_is_par : program = Source.generated_par_source_program).
  { rewrite Source.generated_par_source_lookup_is_present in looked_up.
    injection looked_up as same_program. symmetry. exact same_program. }
  subst program.
  pose proof (Source.decoded_region_nodes_match_generated_source
    (S (length body)) body Source.generated_par_source_program 0 (length body)
    nodes decoded) as generated.
  pose proof (RPB3IndependentOracleRecursiveProjection.selected_source_evaluator_outputs_are_positional
    Source.source_tree Core.source_exact_summary
    (Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
      hash_fn body Source.generated_par_source_program)
    nodes children sequenced) as positional.
  pose proof (RPB3IndependentOracleProcessChildProvenance.generated_selected_children_are_aligned
    Source.generated_par_source_program
    (Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
      hash_fn body Source.generated_par_source_program)
    nodes children generated positional) as aligned.
  pose proof (aligned_par_children_are_plain fuel hash_fn body nodes children
    aligned) as plain.
  assert (same_family :
    Schema.program_family Source.generated_par_source_program = 1).
  { exact (RPB3IndependentOracleProcessScalarDecoderBridge.generated_program_has_requested_family
      1 Source.generated_par_source_program
      Source.generated_par_source_lookup_is_present). }
  assert (faithful :
    Correspondence.scalar_true_faithful_contract
      (Admission.source_scalar_contract_for 1 10) = true)
    by (vm_compute; reflexivity).
  assert (connective :
    Target.scalar_true_or_absent 10 (projected children) =
      Process.source_bool_or_absent 10 children)
    by exact (actual_children_scalar_true_corresponds fuel hash_fn 1
      Source.generated_par_source_program body nodes children 10 same_family
      sequenced faithful).
  assert (score :
    Target.exact_par_score (projected children) =
      option_map Core.project_score (Oracle.source_par_score children)).
  { unfold Target.exact_par_score, Oracle.source_par_score.
    rewrite (RPB3IndependentOracleParScalarScoreBridge.plain_par_component_scores_project
      children plain).
    rewrite connective.
    destruct (Oracle.source_par_component_scores children) as [scores|];
      [|reflexivity].
    destruct (Process.source_bool_or_absent 10 children) as [flag|];
      [|reflexivity].
    cbn [option_map].
    rewrite Core.project_source_score_constructor.
    rewrite map_app.
    destruct flag; reflexivity. }
  constructor.
  - reflexivity.
  - change (Projection.project_source_evidence (Oracle.source_par_score children) =
      match Target.exact_par_score (projected children) with
      | Some value => Target.ExactScore value
      | None => Target.ScoreNotDerived
      end).
    rewrite score.
    destruct (Oracle.source_par_score children); reflexivity.
  - change (Oracle.source_order_definitely_bad
      (Oracle.source_par_component_order children) || false =
      Target.order_definitely_bad
        (Target.raw_par_component_order (projected children)) || false || false
        || false).
    rewrite RPB3IndependentOracleParOrderBridge.source_par_component_order_projects.
    destruct (Target.raw_par_component_order (projected children))
      as [[|]|]; reflexivity.
Qed.

(** ** Family group: Cost (families 9 through 13) *)

Lemma cost_family_agreement :
  forall fuel hash_fn family body nodes program children,
    In family [9; 10; 11; 12; 13] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Source.decode_region (S (length body)) body program 0 (length body) =
      Some nodes ->
    Source.sequence_source_results
      (map (Oracle.source_evaluate_node
             (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program)
        nodes) = Some children ->
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = true ->
    family_agreement family (source_scan program children)
      (target_scan program children) children.
Proof.
  intros fuel hash_fn family body nodes program children member looked_up
    decoded sequenced valid.
  destruct (RPB3IndependentOracleCostFamilyBridge.generated_cost_message_children_correspond
    fuel hash_fn family program body nodes children member looked_up decoded
    sequenced valid) as [corresponds plain].
  pose proof (RPB3IndependentOracleCostVerdictBridge.cost_family_score_projects_totally_for_actual_children
    family children member corresponds plain) as total_score.
  clear corresponds plain decoded sequenced valid looked_up.
  destruct member as [same|[same|[same|[same|[same|[]]]]]]; subst family;
    (constructor; [reflexivity | symmetry; exact total_score | reflexivity]).
Qed.

(** ** Family group: Process (families 8, 15, 16, 19 through 24) *)

Local Ltac destruct_process_completeness :=
  match goal with
  | |- context [Process.source_process_completeness ?family ?scan ?children] =>
      destruct (Process.source_process_completeness family scan children)
  end.

Local Ltac destruct_process_score :=
  match goal with
  | |- context [Process.source_process_family_score ?family ?complete ?children] =>
      destruct (Process.source_process_family_score family complete children)
  end.

Lemma source_message_completeness_on_process_family :
  forall family scan children,
    In family [8; 15; 16; 19; 20; 21; 22; 23; 24] ->
    Oracle.source_message_completeness family scan children =
    Process.source_process_completeness family scan children.
Proof.
  intros family scan children member.
  unfold Oracle.source_message_completeness.
  destruct member as [same|[same|[same|[same|[same|[same|[same|[same|[same|[]]]]]]]]]];
    subst family; destruct_process_completeness; reflexivity.
Qed.

Lemma source_message_score_on_process_family :
  forall family complete children,
    In family [8; 15; 16; 19; 20; 21; 22; 23; 24] ->
    Oracle.source_message_score family complete children =
    Process.source_process_family_score family complete children.
Proof.
  intros family complete children member.
  unfold Oracle.source_message_score.
  destruct member as [same|[same|[same|[same|[same|[same|[same|[same|[same|[]]]]]]]]]];
    subst family; destruct complete as [[|]|]; destruct_process_score;
    reflexivity.
Qed.

Lemma source_message_extra_refused_on_process_family :
  forall family complete scan children,
    In family [8; 15; 16; 19; 20; 21; 22; 23; 24] ->
    Oracle.source_message_extra_refused family complete scan children =
    Process.source_process_family_refused family complete scan children.
Proof.
  intros family complete scan children member.
  unfold Oracle.source_message_extra_refused.
  destruct member as [same|[same|[same|[same|[same|[same|[same|[same|[same|[]]]]]]]]]];
    subst family;
    cbn [Nat.eqb Oracle.source_collection_outside_fixed_point];
    repeat rewrite orb_false_r; reflexivity.
Qed.

Lemma process_family_agreement :
  forall family scan scanned children,
    In family [8; 15; 16; 19; 20; 21; 22; 23; 24] ->
    Admission.source_scan_seen_slots scan = Schema.scan_seen scanned ->
    ProcessProjection.process_child_correspondence family children ->
    family_agreement family scan scanned children.
Proof.
  intros family scan scanned children member same_seen correspond.
  assert (not_eset : family <> 29)
    by (intro same; subst family; cbn [In] in member; lia).
  assert (not_emap : family <> 30)
    by (intro same; subst family; cbn [In] in member; lia).
  constructor.
  - rewrite (source_message_completeness_on_process_family family scan children
      member).
    exact (ProcessProjection.projected_process_completeness family scan scanned
      children member same_seen correspond).
  - rewrite (source_message_score_on_process_family family
      (Oracle.source_message_completeness family scan children) children member).
    symmetry.
    exact (ProcessProjection.projected_process_family_score family
      (Oracle.source_message_completeness family scan children) children member
      correspond).
  - rewrite (source_message_extra_refused_on_process_family family
      (Oracle.source_message_completeness family scan children) scan children
      member).
    rewrite (ProcessProjection.projected_process_target_order_refusal family
      scanned (projected children) member).
    rewrite (Target.payload_fixed_point_violated_other_family family
      (Oracle.source_message_completeness family scan children)
      (projected children) not_eset not_emap).
    rewrite orb_false_r.
    exact (ProcessProjection.projected_process_family_refusal family
      (Oracle.source_message_completeness family scan children) scan scanned
      children member same_seen correspond).
Qed.

(** ** Family group: Name (families 31, 32, 55 and 60) *)

Lemma source_message_completeness_on_name_family :
  forall family scan children,
    In family [31; 32; 55; 60] ->
    Oracle.source_message_completeness family scan children =
    Name.source_name_completeness family scan.
Proof.
  intros family scan children [same|[same|[same|[same|[]]]]]; subst family;
    reflexivity.
Qed.

Lemma source_message_score_on_name_family :
  forall family complete children,
    In family [31; 32; 55; 60] ->
    Oracle.source_message_score family complete children =
    Name.source_name_family_score family complete children.
Proof.
  intros family complete children [same|[same|[same|[same|[]]]]]; subst family;
    destruct complete as [[|]|];
    lazymatch goal with
    | |- _ = Name.source_name_family_score ?listed ?complete_value ?kids =>
        change (match Name.source_name_family_score listed complete_value kids with
                | Some score => Some score
                | None => None
                end = Name.source_name_family_score listed complete_value kids);
        destruct (Name.source_name_family_score listed complete_value kids);
        reflexivity
    end.
Qed.

Lemma name_family_agreement :
  forall family scan scanned children,
    In family [31; 32; 55; 60] ->
    Admission.source_scan_seen_slots scan = Schema.scan_seen scanned ->
    Target.score_evidence_for family
      (Oracle.source_message_completeness family scan children)
      (projected children) =
    Projection.project_source_evidence
      (Name.source_name_family_score family
        (Oracle.source_message_completeness family scan children) children) ->
    family_agreement family scan scanned children.
Proof.
  intros family scan scanned children member same_seen score_agrees.
  constructor.
  - rewrite (source_message_completeness_on_name_family family scan children
      member).
    exact (RPB3IndependentOracleNameProjection.projected_name_completeness
      family scan scanned children member same_seen).
  - rewrite (source_message_score_on_name_family family
      (Oracle.source_message_completeness family scan children) children member).
    symmetry. exact score_agrees.
  - destruct member as [same|[same|[same|[same|[]]]]]; subst family; reflexivity.
Qed.

(** ** Family group: the remaining, unscored families *)

(** The default arm of the process refusal, for a family that is not one of
    its three listed literals. *)
Lemma source_process_family_refused_default :
  forall family complete state children,
    family <> 19 -> family <> 20 -> family <> 22 ->
    Process.source_process_family_refused family complete state children = false.
Proof.
  intros family complete state children not_receive not_new not_match.
  do 23 (destruct family as [|family];
    [first [reflexivity | (exfalso; lia)] |]).
  reflexivity.
Qed.

Lemma cost_family_excludes_other_families :
  forall family,
    family <> 9 -> family <> 10 -> family <> 11 -> family <> 12 -> family <> 13 ->
    RPB3IndependentOracleCost.cost_family family = false.
Proof.
  intros family not_nine not_ten not_eleven not_twelve not_thirteen.
  unfold RPB3IndependentOracleCost.cost_family.
  destruct (9 <=? family) eqn:low; [|reflexivity].
  destruct (family <=? 13) eqn:high; [|reflexivity].
  apply Nat.leb_le in low. apply Nat.leb_le in high.
  exfalso. lia.
Qed.

(** Absence from the listed families forces both completeness functions to
    their default arms, which agree for every remaining identifier. *)
Lemma unscored_family_completeness_agrees :
  forall type_id scan scanned children target_children,
    ~ In type_id [1;6;8;9;10;11;12;13;15;16;19;20;21;22;23;24;27;28;29;30;31;32;33;34;
       35;36;37;38;39;40;41;42;43;44;45;46;47;48;49;50;51;52;53;54;55;56;57;60] ->
    Oracle.source_message_completeness type_id scan children =
    Target.component_completeness type_id scanned target_children.
Proof.
  intros type_id scan scanned children target_children unscored.
  pose proof (RPB3IndependentOracleUnscoredFamilyBridge.absent_type_id_is_unlisted
    type_id _ unscored) as unlisted.
  clear unscored.
  do 61 (destruct type_id as [|type_id];
         [first [discriminate unlisted | reflexivity] |]).
  first [discriminate unlisted | reflexivity].
Qed.

(** A type identifier outside the four family groups is absent from the list
    the unscored bridge excludes.  The nineteen literals of that list are
    consumed one at a time; each is dispatched to the group holding it. *)
Lemma unscored_when_outside_every_family_group :
  forall type_id,
    type_id <> 1 ->
    ~ In type_id [9; 10; 11; 12; 13] ->
    ~ In type_id [8; 15; 16; 19; 20; 21; 22; 23; 24] ->
    ~ In type_id [31; 32; 55; 60] ->
    ~ In type_id
      RPB3IndependentOracleExprPayloadAgreement.expr_payload_family_ids ->
    ~ In type_id [1;6;8;9;10;11;12;13;15;16;19;20;21;22;23;24;27;28;29;30;31;32;33;34;
       35;36;37;38;39;40;41;42;43;44;45;46;47;48;49;50;51;52;53;54;55;56;57;60].
Proof.
  intros type_id not_par not_cost not_process not_name not_payload listed.
  unfold RPB3IndependentOracleExprPayloadAgreement.expr_payload_family_ids
    in not_payload.
  repeat (destruct listed as [same | listed];
    [ subst type_id;
      first
        [ exact (not_par eq_refl)
        | (apply not_cost; cbn [In]; tauto)
        | (apply not_process; cbn [In]; tauto)
        | (apply not_name; cbn [In]; tauto)
        | (apply not_payload; cbn [In]; tauto) ]
    | ]).
  exact listed.
Qed.

(** The default arm of the oracle's fixed-point refusal, the counterpart of
    [Target.payload_fixed_point_violated_other_family]. *)
Lemma source_collection_outside_fixed_point_default :
  forall family complete children,
    family <> 29 -> family <> 30 ->
    Oracle.source_collection_outside_fixed_point family complete children =
    false.
Proof.
  intros family complete children not_eset not_emap.
  do 31 (destruct family as [|family];
    [first [reflexivity | (exfalso; lia)] |]).
  reflexivity.
Qed.

(** The oracle's payload fall-through also has no score outside the listed
    families: every payload arm of [source_expr_payload_family_score] and of
    [source_expr_payload_score] is listed, and the rest is the default arm. *)
Lemma unscored_family_has_no_expr_payload_score :
  forall type_id complete children,
    ~ In type_id [1;6;8;9;10;11;12;13;15;16;19;20;21;22;23;24;27;28;29;30;31;32;33;34;
       35;36;37;38;39;40;41;42;43;44;45;46;47;48;49;50;51;52;53;54;55;56;57;60] ->
    RPB3IndependentOracleExprPayload.source_expr_payload_family_score
      type_id complete children = None.
Proof.
  intros type_id complete children unscored.
  pose proof (RPB3IndependentOracleUnscoredFamilyBridge.absent_type_id_is_unlisted
    type_id _ unscored) as unlisted.
  clear unscored.
  destruct complete as [[|]|];
    do 61 (destruct type_id as [|type_id];
           [first [discriminate unlisted | reflexivity]|]);
    first [discriminate unlisted | reflexivity].
Qed.

Lemma unscored_family_agreement :
  forall family scan scanned children,
    ~ In family [1;6;8;9;10;11;12;13;15;16;19;20;21;22;23;24;27;28;29;30;31;32;33;34;
       35;36;37;38;39;40;41;42;43;44;45;46;47;48;49;50;51;52;53;54;55;56;57;60] ->
    family_agreement family scan scanned children.
Proof.
  intros family scan scanned children unscored.
  pose proof (RPB3IndependentOracleUnscoredFamilyBridge.absent_type_id_is_unlisted
    family _ unscored) as unlisted.
  assert (not_par : family <> 1)
    by (intro same; subst family; discriminate unlisted).
  assert (not_nine : family <> 9)
    by (intro same; subst family; discriminate unlisted).
  assert (not_ten : family <> 10)
    by (intro same; subst family; discriminate unlisted).
  assert (not_eleven : family <> 11)
    by (intro same; subst family; discriminate unlisted).
  assert (not_twelve : family <> 12)
    by (intro same; subst family; discriminate unlisted).
  assert (not_thirteen : family <> 13)
    by (intro same; subst family; discriminate unlisted).
  assert (not_receive : family <> 19)
    by (intro same; subst family; discriminate unlisted).
  assert (not_new : family <> 20)
    by (intro same; subst family; discriminate unlisted).
  assert (not_match : family <> 22)
    by (intro same; subst family; discriminate unlisted).
  assert (not_eset : family <> 29)
    by (intro same; subst family; discriminate unlisted).
  assert (not_emap : family <> 30)
    by (intro same; subst family; discriminate unlisted).
  constructor.
  - exact (unscored_family_completeness_agrees family scan scanned children _
      unscored).
  - destruct (RPB3IndependentOracleUnscoredFamilyBridge.unscored_family_summaries_agree
      family (Oracle.source_message_completeness family scan children) children
      unscored) as [target_unscored [process_none name_none]].
    assert (source_none :
      Oracle.source_message_score family
        (Oracle.source_message_completeness family scan children) children = None).
    { unfold Oracle.source_message_score.
      rewrite (proj2 (Nat.eqb_neq family 1) not_par).
      rewrite (cost_family_excludes_other_families family not_nine not_ten
        not_eleven not_twelve not_thirteen).
      rewrite process_none.
      rewrite name_none.
      exact (unscored_family_has_no_expr_payload_score
        family (Oracle.source_message_completeness family scan children) children
        unscored). }
    rewrite source_none, target_unscored.
    reflexivity.
  - unfold Oracle.source_message_extra_refused,
      Target.complete_receive_has_default_condition,
      Target.complete_match_has_default_guard.
    rewrite (source_process_family_refused_default family
      (Oracle.source_message_completeness family scan children) scan children
      not_receive not_new not_match).
    rewrite (proj2 (Nat.eqb_neq family 1) not_par),
      (proj2 (Nat.eqb_neq family 19) not_receive),
      (proj2 (Nat.eqb_neq family 20) not_new),
      (proj2 (Nat.eqb_neq family 22) not_match).
    rewrite (source_collection_outside_fixed_point_default family
      (Oracle.source_message_completeness family scan children) children
      not_eset not_emap),
      (Target.payload_fixed_point_violated_other_family family
        (Oracle.source_message_completeness family scan children)
        (projected children) not_eset not_emap).
    reflexivity.
Qed.

(** ** The component lemmas *)

Record message_summary_premises (raw : Schema.raw_field) (type_id : nat)
    (program : Schema.generated_program)
    (children : list Core.source_exact_summary) : Prop := {
  premise_payload : Schema.raw_payload raw = Schema.Message;
  premise_type_id : Schema.raw_program_type_id raw = Some type_id;
  premise_lookup : Schema.lookup_projected_program type_id [] [] = Some program;
  premise_family : Schema.program_family program = type_id;
  premise_scan_valid :
    Admission.source_scan_valid (source_scan program children) = true;
  premise_agreement :
    family_agreement type_id (source_scan program children)
      (target_scan program children) children
}.

Local Notation reduced raw children :=
  (Target.reduce_semantic_field raw
    (map Projection.project_source_summary children)) (only parsing).

(** The tag, wire, payload and body of both records are read from the raw
    field.  Each side is first put in constructor form and then projected, so
    that no conversion check has to compare the two refusal expressions. *)
Lemma message_summary_tag_component :
  forall raw program children,
    Target.sem_tag (Projection.project_source_summary
      (Oracle.source_message_summary raw program children)) =
    Target.sem_tag (reduced raw children).
Proof.
  intros raw program children.
  rewrite projected_source_message_summary.
  unfold Target.reduce_semantic_field.
  cbn [Target.sem_tag].
  reflexivity.
Qed.

Lemma message_summary_wire_component :
  forall raw program children,
    Target.sem_wire (Projection.project_source_summary
      (Oracle.source_message_summary raw program children)) =
    Target.sem_wire (reduced raw children).
Proof.
  intros raw program children.
  rewrite projected_source_message_summary.
  unfold Target.reduce_semantic_field.
  cbn [Target.sem_wire].
  reflexivity.
Qed.

Lemma message_summary_payload_component :
  forall raw program children,
    Target.sem_payload (Projection.project_source_summary
      (Oracle.source_message_summary raw program children)) =
    Target.sem_payload (reduced raw children).
Proof.
  intros raw program children.
  rewrite projected_source_message_summary.
  unfold Target.reduce_semantic_field.
  cbn [Target.sem_payload].
  reflexivity.
Qed.

Lemma message_summary_body_component :
  forall raw program children,
    Target.sem_body (Projection.project_source_summary
      (Oracle.source_message_summary raw program children)) =
    Target.sem_body (reduced raw children).
Proof.
  intros raw program children.
  rewrite projected_source_message_summary.
  unfold Target.reduce_semantic_field.
  cbn [Target.sem_body].
  reflexivity.
Qed.

Lemma message_summary_type_id_component :
  forall raw type_id program children,
    message_summary_premises raw type_id program children ->
    Target.sem_type_id (Projection.project_source_summary
      (Oracle.source_message_summary raw program children)) =
    Target.sem_type_id (reduced raw children).
Proof.
  intros raw type_id program children
    [payload program_id looked_up same_family _ _].
  rewrite (reduce_semantic_field_resolved raw type_id program
    (projected children) payload program_id looked_up same_family).
  rewrite projected_source_message_summary.
  cbn [Target.sem_type_id].
  unfold Projection.project_source_type_id.
  rewrite program_id.
  reflexivity.
Qed.

(** The oracle's source refusal, as a target-style refusal plus the key-order
    refusal.  The boolean rearrangement is first a closed lemma so that the
    scan agreement can be rewritten before the final comparison. *)
Lemma refusal_rearrangement :
  forall well_formed required ordered order_bad receive_bad guard_bad fixed_bad
      extra : bool,
    extra = (order_bad || receive_bad || guard_bad || fixed_bad) ->
    negb well_formed || negb true || negb required || negb ordered || extra =
    negb ordered ||
      (negb true || negb well_formed || negb true || negb true ||
       negb required || order_bad || receive_bad || guard_bad || fixed_bad).
Proof. intros [] [] [] [] [] [] [] extra same; subst extra; reflexivity. Qed.

Lemma message_summary_refusal_component :
  forall raw type_id program children,
    message_summary_premises raw type_id program children ->
    Target.sem_definitely_refused (Projection.project_source_summary
      (Oracle.source_message_summary raw program children)) =
    negb (Oracle.source_string_map_keys_ordered children) ||
    Target.sem_definitely_refused (reduced raw children).
Proof.
  intros raw type_id program children
    [payload program_id looked_up same_family valid agreement].
  destruct (Projection.source_scan_children_project_valid program children valid)
    as [target_valid [_ same_seen]].
  rewrite (reduce_semantic_field_resolved raw type_id program
    (projected children) payload program_id looked_up same_family).
  rewrite projected_source_message_summary.
  cbn [Target.sem_definitely_refused].
  rewrite same_family, valid, target_valid.
  rewrite (required_pathmaps_agree program (source_scan program children)
    (target_scan program children) same_seen).
  rewrite <- (family_completeness_agrees _ _ _ _ agreement).
  change (Admission.source_raw_bytes_well_formed raw)
    with (Schema.valid_raw_body raw).
  exact (refusal_rearrangement _ _ _ _ _ _ _ _
    (family_refusal_agrees _ _ _ _ agreement)).
Qed.

Lemma message_summary_completeness_component :
  forall raw type_id program children,
    message_summary_premises raw type_id program children ->
    Target.sem_complete (Projection.project_source_summary
      (Oracle.source_message_summary raw program children)) =
    Target.sem_complete (reduced raw children).
Proof.
  intros raw type_id program children
    [payload program_id looked_up same_family valid agreement].
  rewrite (reduce_semantic_field_resolved raw type_id program
    (projected children) payload program_id looked_up same_family).
  rewrite projected_source_message_summary.
  cbn [Target.sem_complete].
  rewrite same_family.
  exact (family_completeness_agrees _ _ _ _ agreement).
Qed.

Lemma message_summary_score_component :
  forall raw type_id program children,
    message_summary_premises raw type_id program children ->
    Target.sem_score_evidence (Projection.project_source_summary
      (Oracle.source_message_summary raw program children)) =
    Target.sem_score_evidence (reduced raw children).
Proof.
  intros raw type_id program children
    [payload program_id looked_up same_family valid agreement].
  rewrite (reduce_semantic_field_resolved raw type_id program
    (projected children) payload program_id looked_up same_family).
  rewrite projected_source_message_summary.
  cbn [Target.sem_score_evidence].
  rewrite same_family.
  rewrite <- (family_completeness_agrees _ _ _ _ agreement).
  exact (family_score_agrees _ _ _ _ agreement).
Qed.

Lemma message_summary_bind_source_component :
  forall raw type_id program children,
    message_summary_premises raw type_id program children ->
    Target.sem_bind_source_present (Projection.project_source_summary
      (Oracle.source_message_summary raw program children)) =
    Target.sem_bind_source_present (reduced raw children).
Proof.
  intros raw type_id program children
    [payload program_id looked_up same_family valid agreement].
  destruct (Projection.source_scan_children_project_valid program children valid)
    as [_ [_ same_seen]].
  rewrite (reduce_semantic_field_resolved raw type_id program
    (projected children) payload program_id looked_up same_family).
  rewrite projected_source_message_summary.
  cbn [Target.sem_bind_source_present].
  rewrite same_family.
  unfold Process.source_bind_source_present.
  rewrite (ProcessProjection.projected_process_seen_slot _ _ 1 same_seen).
  reflexivity.
Qed.

Lemma message_summary_case_required_component :
  forall raw type_id program children,
    message_summary_premises raw type_id program children ->
    Target.sem_case_required_present (Projection.project_source_summary
      (Oracle.source_message_summary raw program children)) =
    Target.sem_case_required_present (reduced raw children).
Proof.
  intros raw type_id program children
    [payload program_id looked_up same_family valid agreement].
  destruct (Projection.source_scan_children_project_valid program children valid)
    as [_ [_ same_seen]].
  rewrite (reduce_semantic_field_resolved raw type_id program
    (projected children) payload program_id looked_up same_family).
  rewrite projected_source_message_summary.
  cbn [Target.sem_case_required_present].
  rewrite same_family.
  unfold Process.source_case_required_present.
  rewrite (ProcessProjection.projected_process_seen_slot _ _ 0 same_seen).
  rewrite (ProcessProjection.projected_process_seen_slot _ _ 1 same_seen).
  reflexivity.
Qed.

(** ** The target summary with the oracle's key-order refusal added *)

Definition with_definite_refusal (summary : Target.semantic_summary)
    (refused : bool) : Target.semantic_summary :=
  Target.SemanticSummary (Target.sem_tag summary) (Target.sem_wire summary)
    (Target.sem_payload summary) (Target.sem_type_id summary)
    (Target.sem_body summary) refused (Target.sem_complete summary)
    (Target.sem_score_evidence summary)
    (Target.sem_bind_source_present summary)
    (Target.sem_case_required_present summary).

Lemma with_definite_refusal_same :
  forall summary,
    with_definite_refusal summary (Target.sem_definitely_refused summary) =
      summary.
Proof. intros [] ; reflexivity. Qed.

Lemma with_definite_refusal_equation_iff :
  forall (summary : Target.semantic_summary) (ordered : bool),
    with_definite_refusal summary
      (negb ordered || Target.sem_definitely_refused summary) = summary <->
    ordered = true \/ Target.sem_definitely_refused summary = true.
Proof.
  intros [tag wire payload type_id body refused complete score bind case_required]
    ordered.
  cbn [with_definite_refusal Target.sem_tag Target.sem_wire Target.sem_payload
    Target.sem_type_id Target.sem_body Target.sem_definitely_refused
    Target.sem_complete Target.sem_score_evidence
    Target.sem_bind_source_present Target.sem_case_required_present].
  destruct ordered, refused; cbn [negb orb]; split; intro same;
    first [reflexivity | left; reflexivity | right; reflexivity
      | destruct same as [same|same]; discriminate same | discriminate same].
Qed.

(** ** Only the New family can carry a string-map child

    The oracle's key-order refusal reads [TypedSourceStringMap] children only.
    The evaluator produces one only for a descriptor whose payload is
    [StringMap], and the generated table declares such a descriptor in family
    20 alone, so the key order holds vacuously for every other family. *)

Definition program_has_no_string_map (program : Schema.generated_program)
    : bool :=
  forallb (fun descriptor =>
    match Schema.descriptor_payload descriptor with
    | Schema.StringMap => false
    | _ => true
    end) (Schema.program_fields program).

Definition child_is_not_a_string_map (child : Core.source_exact_summary)
    : Prop :=
  match Core.source_summary_value child with
  | Core.TypedSourceStringMap _ _ _ => False
  | _ => True
  end.

(** The finite certificate over the generated table. *)
Lemma only_new_program_declares_string_maps :
  forallb (fun row =>
    (RPB3DescriptorProjectionData.GeneratedData.program_type_id row =? 20) ||
    program_has_no_string_map (Schema.project_generated_program row [] []))
    RPB3DescriptorProjectionData.GeneratedData.generated_programs = true.
Proof. vm_compute. reflexivity. Qed.

Lemma looked_up_program_other_than_new_has_no_string_map :
  forall type_id program,
    type_id <> 20 ->
    Schema.lookup_projected_program type_id [] [] = Some program ->
    program_has_no_string_map program = true.
Proof.
  intros type_id program not_new found.
  unfold Schema.lookup_projected_program in found.
  destruct (find (fun row =>
      RPB3DescriptorProjectionData.GeneratedData.program_type_id row =? type_id)
      RPB3DescriptorProjectionData.GeneratedData.generated_programs)
    as [row|] eqn:selected; [|discriminate].
  inversion found; subst program.
  apply find_some in selected as [member same_id].
  apply Nat.eqb_eq in same_id.
  pose proof (proj1 (forallb_forall _ _)
    only_new_program_declares_string_maps row member) as certified.
  cbn beta in certified.
  apply orb_true_iff in certified as [is_new|no_map].
  - apply Nat.eqb_eq in is_new.
    exfalso. apply not_new.
    rewrite <- same_id.
    exact is_new.
  - exact no_map.
Qed.

Lemma no_string_map_field_has_other_payload :
  forall program field,
    program_has_no_string_map program = true ->
    In field (Schema.program_fields program) ->
    Schema.descriptor_payload field <> Schema.StringMap.
Proof.
  intros program field no_map member same_payload.
  unfold program_has_no_string_map in no_map.
  pose proof (proj1 (forallb_forall _ _) no_map field member) as field_free.
  cbn beta in field_free.
  rewrite same_payload in field_free.
  discriminate field_free.
Qed.

(** The generalization to any program of
    [RPB3IndependentOracleRecursiveProjection.matched_par_node_field_is_member]. *)
Lemma matched_node_field_is_member :
  forall program node,
    Source.source_node_matches_generated_descriptor program node ->
    In (Source.source_field node) (Schema.program_fields program).
Proof.
  intros program node [matched _].
  unfold Source.source_descriptor in matched.
  destruct (Schema.resolve_tag (Schema.program_fields program)
      (Source.Wire.span_number (Source.source_span node)))
    as [selected|] eqn:resolved; [|discriminate].
  destruct (Schema.wire_eqb
      (Source.wire_kind_of_span (Source.source_span node))
      (Schema.descriptor_wire selected)) eqn:wire_ok;
    [|discriminate].
  inversion matched; subst selected.
  eapply RPB3IndependentOracleRecursiveProjection.resolved_generated_field_is_member;
    exact resolved.
Qed.

(** The only auxiliary branch that builds a string-map value is the one guarded
    by a [StringMap] descriptor payload. *)
Lemma evaluated_auxiliary_node_without_string_map_payload :
  forall recurse hash_fn body program span field child,
    Schema.descriptor_payload field <> Schema.StringMap ->
    Oracle.source_evaluate_node recurse hash_fn body program
      (Source.SourceAuxiliary span field) = Some child ->
    child_is_not_a_string_map child.
Proof.
  intros recurse hash_fn body program span field child not_map evaluated.
  unfold Oracle.source_evaluate_node in evaluated.
  destruct (Source.raw_field_of_source body (Source.SourceAuxiliary span field))
    as [raw|] eqn:converted; [|discriminate evaluated].
  destruct (Schema.descriptor_payload field) eqn:descriptor_kind;
    [discriminate evaluated|discriminate evaluated|contradiction|].
  finish_auxiliary evaluated.
  injection evaluated as same_child; subst child.
  exact I.
Qed.

Lemma evaluated_child_of_map_free_program_is_not_a_string_map :
  forall fuel hash_fn body program node child,
    program_has_no_string_map program = true ->
    Source.source_node_matches_generated_descriptor program node ->
    Oracle.source_evaluate_node
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program node =
      Some child ->
    child_is_not_a_string_map child.
Proof.
  intros fuel hash_fn body program [span field|span field nested|span field]
    child no_map matched evaluated.
  - destruct (RPB3IndependentOracleProcessChildProvenance.accepted_source_scalar_node_has_typed_value
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program span
      field child evaluated) as [raw [scalar [_ [typed _]]]].
    unfold child_is_not_a_string_map.
    rewrite typed.
    exact I.
  - destruct (RPB3IndependentOracleProcessChildProvenance.accepted_source_message_node_has_typed_value
      fuel hash_fn body program span field nested child evaluated)
      as [raw [children [_ [typed _]]]].
    unfold child_is_not_a_string_map.
    rewrite typed.
    exact I.
  - apply (evaluated_auxiliary_node_without_string_map_payload
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program span
      field child); [|exact evaluated].
    apply (no_string_map_field_has_other_payload program field no_map).
    exact (matched_node_field_is_member program
      (Source.SourceAuxiliary span field) matched).
Qed.

Lemma aligned_children_of_map_free_program_are_not_string_maps :
  forall fuel hash_fn body program nodes children,
    program_has_no_string_map program = true ->
    Forall2 (fun node child =>
      Source.source_node_matches_generated_descriptor program node /\
      Oracle.source_evaluate_node
        (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program node =
        Some child)
      nodes children ->
    Forall child_is_not_a_string_map children.
Proof.
  intros fuel hash_fn body program nodes children no_map aligned.
  induction aligned as
    [|node child rest_nodes rest_children [matched evaluated] rest IH];
    constructor.
  - exact (evaluated_child_of_map_free_program_is_not_a_string_map fuel hash_fn
      body program node child no_map matched evaluated).
  - exact IH.
Qed.

Lemma string_map_free_children_are_ordered :
  forall children previous,
    Forall child_is_not_a_string_map children ->
    Oracle.source_string_map_keys_ordered_after previous children = true.
Proof.
  induction children as [|child rest IH]; intros previous free; [reflexivity|].
  inversion free as [|? ? child_free rest_free]; subst.
  revert child_free.
  cbn [Oracle.source_string_map_keys_ordered_after].
  unfold child_is_not_a_string_map.
  destruct (Core.source_summary_value child); intro child_free;
    try contradiction;
    exact (IH None rest_free).
Qed.

(** Outside family 20 the key-order premise is a consequence of the evaluation
    itself, so the requested equation needs it for family 20 only. *)
Theorem actual_children_of_non_new_family_are_ordered :
  forall type_id program children,
    type_id <> 20 ->
    Schema.lookup_projected_program type_id [] [] = Some program ->
    actual_unrefused_children program children ->
    Oracle.source_string_map_keys_ordered children = true.
Proof.
  intros type_id program children not_new looked_up
    [fuel [hash_fn [body [nodes [decoded [sequenced _]]]]]].
  pose proof (looked_up_program_other_than_new_has_no_string_map type_id program
    not_new looked_up) as no_map.
  pose proof (Source.decoded_region_nodes_match_generated_source
    (S (length body)) body program 0 (length body) nodes decoded) as generated.
  pose proof (RPB3IndependentOracleRecursiveProjection.selected_source_evaluator_outputs_are_positional
    Source.source_tree Core.source_exact_summary
    (Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
      hash_fn body program)
    nodes children sequenced) as positional.
  pose proof (RPB3IndependentOracleProcessChildProvenance.generated_selected_children_are_aligned
    program
    (Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
      hash_fn body program)
    nodes children generated positional) as aligned.
  unfold Oracle.source_string_map_keys_ordered.
  exact (string_map_free_children_are_ordered children None
    (aligned_children_of_map_free_program_are_not_string_maps fuel hash_fn body
      program nodes children no_map aligned)).
Qed.

(** ** The assembled statement *)

(** The two records agree at every component except the refusal bit, which on
    the oracle's side additionally carries the string-map key order.  This is
    the unconditional form of the requested theorem. *)
Theorem message_summary_from_agreement :
  forall raw type_id program children,
    message_summary_premises raw type_id program children ->
    Projection.project_source_summary
      (Oracle.source_message_summary raw program children) =
    with_definite_refusal (reduced raw children)
      (negb (Oracle.source_string_map_keys_ordered children) ||
       Target.sem_definitely_refused (reduced raw children)).
Proof.
  intros raw type_id program children premises.
  apply semantic_summary_components_equal;
    cbn [with_definite_refusal Target.sem_tag Target.sem_wire Target.sem_payload
      Target.sem_type_id Target.sem_body Target.sem_definitely_refused
      Target.sem_complete Target.sem_score_evidence
      Target.sem_bind_source_present Target.sem_case_required_present].
  - exact (message_summary_tag_component raw program children).
  - exact (message_summary_wire_component raw program children).
  - exact (message_summary_payload_component raw program children).
  - exact (message_summary_type_id_component raw type_id program children
      premises).
  - exact (message_summary_body_component raw program children).
  - exact (message_summary_refusal_component raw type_id program children
      premises).
  - exact (message_summary_completeness_component raw type_id program children
      premises).
  - exact (message_summary_score_component raw type_id program children
      premises).
  - exact (message_summary_bind_source_component raw type_id program children
      premises).
  - exact (message_summary_case_required_component raw type_id program children
      premises).
Qed.

Theorem message_summary_premises_from_actual_children :
  process_assembly_statement ->
  name_family_statement ->
  forall raw type_id program children,
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some type_id ->
    Schema.lookup_projected_program type_id [] [] = Some program ->
    actual_unrefused_children program children ->
    message_summary_premises raw type_id program children.
Proof.
  intros process_statement name_statement raw type_id program children payload
    program_id looked_up actual.
  destruct actual as [fuel [hash_fn [body [nodes [decoded [sequenced valid]]]]]].
  assert (same_family : Schema.program_family program = type_id)
    by exact (RPB3IndependentOracleProcessScalarDecoderBridge.generated_program_has_requested_family
      type_id program looked_up).
  destruct (Projection.source_scan_children_project_valid program children valid)
    as [_ [_ same_seen]].
  assert (agreement :
    family_agreement type_id (source_scan program children)
      (target_scan program children) children).
  { destruct (Nat.eq_dec type_id 1) as [is_par|not_par].
    - rewrite is_par in looked_up |- *.
      exact (par_family_agreement fuel hash_fn body nodes program children
        looked_up decoded sequenced).
    - destruct (in_dec Nat.eq_dec type_id [9; 10; 11; 12; 13])
        as [is_cost|not_cost].
      + exact (cost_family_agreement fuel hash_fn type_id body nodes program
          children is_cost looked_up decoded sequenced valid).
      + destruct (in_dec Nat.eq_dec type_id [8; 15; 16; 19; 20; 21; 22; 23; 24])
          as [is_process|not_process].
        * apply (process_family_agreement type_id (source_scan program children)
            (target_scan program children) children is_process same_seen).
          apply (process_statement fuel hash_fn type_id program body children).
          unfold Correspondence.process_actual_children.
          exact (conj is_process (conj looked_up (conj valid
            (ex_intro _ nodes (conj decoded sequenced))))).
        * destruct (in_dec Nat.eq_dec type_id [31; 32; 55; 60])
            as [is_name|not_name].
          -- apply (name_family_agreement type_id (source_scan program children)
               (target_scan program children) children is_name same_seen).
             exact (name_statement fuel hash_fn type_id program body nodes
               children is_name looked_up decoded sequenced valid).
          -- destruct (in_dec Nat.eq_dec type_id
               RPB3IndependentOracleExprPayloadAgreement.expr_payload_family_ids)
               as [is_payload|not_payload].
             ++ destruct (RPB3IndependentOracleExprPayloadAgreement.expr_payload_family_agreement
                  fuel hash_fn type_id program body nodes children is_payload
                  looked_up decoded sequenced valid)
                  as [same_complete [same_score same_refused]].
                exact (Build_family_agreement type_id
                  (source_scan program children) (target_scan program children)
                  children same_complete same_score same_refused).
             ++ apply unscored_family_agreement.
                exact (unscored_when_outside_every_family_group type_id not_par
                  not_cost not_process not_name not_payload). }
  exact (Build_message_summary_premises raw type_id program children payload
    program_id looked_up same_family valid agreement).
Qed.

Theorem source_message_summary_up_to_map_key_order :
  process_assembly_statement ->
  name_family_statement ->
  forall raw type_id program children,
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some type_id ->
    Schema.lookup_projected_program type_id [] [] = Some program ->
    actual_unrefused_children program children ->
    Projection.project_source_summary
      (Oracle.source_message_summary raw program children) =
    with_definite_refusal (reduced raw children)
      (negb (Oracle.source_string_map_keys_ordered children) ||
       Target.sem_definitely_refused (reduced raw children)).
Proof.
  intros process_statement name_statement raw type_id program children payload
    program_id looked_up actual.
  apply (message_summary_from_agreement raw type_id program children).
  exact (message_summary_premises_from_actual_children process_statement
    name_statement raw type_id program children payload program_id looked_up
    actual).
Qed.

(** The requested theorem with one additional premise: when the family is New,
    the children's string-map keys are in ascending order.  For every other
    family the premise holds by [actual_children_of_non_new_family_are_ordered],
    and the closed refutation at the end of this file shows it cannot be dropped
    for family 20. *)
Theorem source_message_summary_projects_to_reducer_given_family_results :
  process_assembly_statement ->
  name_family_statement ->
  forall raw type_id program children,
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some type_id ->
    Schema.lookup_projected_program type_id [] [] = Some program ->
    actual_unrefused_children program children ->
    (type_id = 20 -> Oracle.source_string_map_keys_ordered children = true) ->
    Projection.project_source_summary
      (Oracle.source_message_summary raw program children) =
    Target.reduce_semantic_field raw
      (map Projection.project_source_summary children).
Proof.
  intros process_statement name_statement raw type_id program children payload
    program_id looked_up actual new_ordered.
  assert (ordered : Oracle.source_string_map_keys_ordered children = true).
  { destruct (Nat.eq_dec type_id 20) as [is_new|not_new].
    - exact (new_ordered is_new).
    - exact (actual_children_of_non_new_family_are_ordered type_id program
        children not_new looked_up actual). }
  rewrite (source_message_summary_up_to_map_key_order process_statement
    name_statement raw type_id program children payload program_id looked_up
    actual).
  rewrite ordered.
  change (negb true) with false.
  rewrite orb_false_l.
  exact (with_definite_refusal_same (reduced raw children)).
Qed.

(** The requested equation holds without any additional premise for every
    family other than New. *)
Theorem source_message_summary_projects_to_reducer_outside_new_family :
  process_assembly_statement ->
  name_family_statement ->
  forall raw type_id program children,
    type_id <> 20 ->
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some type_id ->
    Schema.lookup_projected_program type_id [] [] = Some program ->
    actual_unrefused_children program children ->
    Projection.project_source_summary
      (Oracle.source_message_summary raw program children) =
    Target.reduce_semantic_field raw
      (map Projection.project_source_summary children).
Proof.
  intros process_statement name_statement raw type_id program children not_new
    payload program_id looked_up actual.
  apply (source_message_summary_projects_to_reducer_given_family_results
    process_statement name_statement raw type_id program children payload
    program_id looked_up actual).
  intro is_new.
  exfalso.
  exact (not_new is_new).
Qed.

(** The premise is minimal: on actual children, the equality holds exactly when
    the string-map keys are ordered or the target already refuses for another
    reason. *)
Theorem source_message_summary_equals_reducer_iff_map_keys_ordered_or_target_refuses :
  process_assembly_statement ->
  name_family_statement ->
  forall raw type_id program children,
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some type_id ->
    Schema.lookup_projected_program type_id [] [] = Some program ->
    actual_unrefused_children program children ->
    (Projection.project_source_summary
       (Oracle.source_message_summary raw program children) =
     reduced raw children <->
     Oracle.source_string_map_keys_ordered children = true \/
     Target.sem_definitely_refused (reduced raw children) = true).
Proof.
  intros process_statement name_statement raw type_id program children payload
    program_id looked_up actual.
  rewrite (source_message_summary_up_to_map_key_order process_statement
    name_statement raw type_id program children payload program_id looked_up
    actual).
  apply with_definite_refusal_equation_iff.
Qed.

(** ** Closed refutation of the statement without the key-order premise

    The witness is a [New] body with two [injections] entries, keys ["b"] and
    ["a"], in that order.  Each entry encodes a key field (tag 1) and no value
    field; the outer tag 4 and length byte (34, 3) is followed by the entry
    bytes [10; 1; 98] and [10; 1; 97].  The evaluator produces two string-map
    children, the scan accepts them (the slot is repeated), and the oracle
    refuses them for key order while the target accepts. *)
Definition unordered_injections_body : list nat :=
  [34; 3; 10; 1; 98; 34; 3; 10; 1; 97].

Definition unordered_injections_raw : Schema.raw_field :=
  Schema.RawField 0 Schema.LengthDelimited Schema.Message
    unordered_injections_body (length unordered_injections_body) (Some 20)
    [] [].

Definition unordered_injections_hash (_ : list nat) : nat := 0.

Definition unordered_injections_nodes : list Source.source_tree :=
  match Source.decode_region (S (length unordered_injections_body))
      unordered_injections_body Source.generated_new_program 0
      (length unordered_injections_body) with
  | Some nodes => nodes
  | None => []
  end.

Definition unordered_injections_children : list Core.source_exact_summary :=
  match Source.sequence_source_results
      (map (Oracle.source_evaluate_node
             (Oracle.source_evaluate_message 3 unordered_injections_hash)
             unordered_injections_hash unordered_injections_body
             Source.generated_new_program)
        unordered_injections_nodes) with
  | Some children => children
  | None => []
  end.

Lemma unordered_injections_children_are_actual :
  actual_unrefused_children Source.generated_new_program
    unordered_injections_children.
Proof.
  unfold actual_unrefused_children.
  exists 3, unordered_injections_hash, unordered_injections_body,
    unordered_injections_nodes.
  split; [|split]; vm_compute; reflexivity.
Qed.

Lemma unordered_injections_are_unordered :
  Oracle.source_string_map_keys_ordered unordered_injections_children = false.
Proof. vm_compute. reflexivity. Qed.

Lemma unordered_injections_lookup :
  Schema.lookup_projected_program 20 [] [] = Some Source.generated_new_program.
Proof. vm_compute. reflexivity. Qed.

Lemma unordered_injections_refusals_differ :
  Target.sem_definitely_refused
    (Projection.project_source_summary
      (Oracle.source_message_summary unordered_injections_raw
        Source.generated_new_program unordered_injections_children)) = true /\
  Target.sem_definitely_refused
    (Target.reduce_semantic_field unordered_injections_raw
      (map Projection.project_source_summary unordered_injections_children)) =
    false.
Proof. vm_compute. split; reflexivity. Qed.

Theorem actual_unrefused_children_do_not_project_to_reducer_without_key_order :
  ~ (forall raw type_id program children,
      Schema.raw_payload raw = Schema.Message ->
      Schema.raw_program_type_id raw = Some type_id ->
      Schema.lookup_projected_program type_id [] [] = Some program ->
      actual_unrefused_children program children ->
      Projection.project_source_summary
        (Oracle.source_message_summary raw program children) =
      Target.reduce_semantic_field raw
        (map Projection.project_source_summary children)).
Proof.
  intro unconditional.
  pose proof (unconditional unordered_injections_raw 20
    Source.generated_new_program unordered_injections_children eq_refl eq_refl
    unordered_injections_lookup unordered_injections_children_are_actual)
    as equal_summaries.
  apply (f_equal Target.sem_definitely_refused) in equal_summaries.
  destruct unordered_injections_refusals_differ as [source_refused target_accepted].
  rewrite source_refused, target_accepted in equal_summaries.
  discriminate equal_summaries.
Qed.

(** The requested form carries the two family statements as premises, so the
    refutation above does not by itself refute it: it is the implication
    [P -> Q -> body] with [body] false.  What holds is that the requested form
    contradicts its own premises, so it can only be proved by refuting one of
    the two family statements. *)
Theorem requested_statement_contradicts_its_family_results :
  (process_assembly_statement ->
   name_family_statement ->
   forall raw type_id program children,
     Schema.raw_payload raw = Schema.Message ->
     Schema.raw_program_type_id raw = Some type_id ->
     Schema.lookup_projected_program type_id [] [] = Some program ->
     actual_unrefused_children program children ->
     Projection.project_source_summary
       (Oracle.source_message_summary raw program children) =
     Target.reduce_semantic_field raw
       (map Projection.project_source_summary children)) ->
  process_assembly_statement ->
  name_family_statement ->
  False.
Proof.
  intros requested process_statement name_statement.
  exact (actual_unrefused_children_do_not_project_to_reducer_without_key_order
    (requested process_statement name_statement)).
Qed.

(** ** The assembled statement

    The two family-result statements of the assembly are the theorems of the
    Process child assembly and of the Name family bridge, the latter at the
    oracle's own completeness verdict. The remaining premise is the key order
    of family 20, which
    [actual_unrefused_children_do_not_project_to_reducer_without_key_order]
    shows cannot be dropped. *)

Theorem process_assembly_statement_holds : process_assembly_statement.
Proof.
  exact RPB3IndependentOracleProcessChildAssembly.process_actual_children_correspondence.
Qed.

Theorem name_family_statement_holds : name_family_statement.
Proof.
  intros fuel hash_fn family program body nodes children
    family_member looked_up decoded evaluated scan_valid.
  exact (RPB3IndependentOracleNameFamilyBridge.generated_name_family_score_projects_totally
    fuel hash_fn family program body nodes children _
    family_member looked_up decoded evaluated scan_valid).
Qed.

Theorem source_message_summary_projects_to_reducer :
  forall raw type_id program children,
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some type_id ->
    Schema.lookup_projected_program type_id [] [] = Some program ->
    actual_unrefused_children program children ->
    (type_id = 20 -> Oracle.source_string_map_keys_ordered children = true) ->
    Projection.project_source_summary
      (Oracle.source_message_summary raw program children) =
    Target.reduce_semantic_field raw
      (map Projection.project_source_summary children).
Proof.
  exact (source_message_summary_projects_to_reducer_given_family_results
    process_assembly_statement_holds name_family_statement_holds).
Qed.

Print Assumptions raw_bytes_well_formed_is_valid_raw_body.
Print Assumptions projected_source_message_summary.
Print Assumptions reduce_semantic_field_resolved.
Print Assumptions semantic_summary_components_equal.
Print Assumptions required_pathmaps_agree.
Print Assumptions actual_children_scalar_true_corresponds.
Print Assumptions aligned_par_children_are_plain.
Print Assumptions par_family_agreement.
Print Assumptions cost_family_agreement.
Print Assumptions source_message_completeness_on_process_family.
Print Assumptions source_message_score_on_process_family.
Print Assumptions source_message_extra_refused_on_process_family.
Print Assumptions process_family_agreement.
Print Assumptions source_message_completeness_on_name_family.
Print Assumptions source_message_score_on_name_family.
Print Assumptions name_family_agreement.
Print Assumptions source_process_family_refused_default.
Print Assumptions cost_family_excludes_other_families.
Print Assumptions unscored_family_completeness_agrees.
Print Assumptions unscored_when_outside_every_family_group.
Print Assumptions unscored_family_agreement.
Print Assumptions message_summary_tag_component.
Print Assumptions message_summary_wire_component.
Print Assumptions message_summary_payload_component.
Print Assumptions message_summary_body_component.
Print Assumptions message_summary_type_id_component.
Print Assumptions refusal_rearrangement.
Print Assumptions message_summary_refusal_component.
Print Assumptions message_summary_completeness_component.
Print Assumptions message_summary_score_component.
Print Assumptions message_summary_bind_source_component.
Print Assumptions message_summary_case_required_component.
Print Assumptions with_definite_refusal_same.
Print Assumptions with_definite_refusal_equation_iff.
Print Assumptions only_new_program_declares_string_maps.
Print Assumptions looked_up_program_other_than_new_has_no_string_map.
Print Assumptions no_string_map_field_has_other_payload.
Print Assumptions matched_node_field_is_member.
Print Assumptions evaluated_auxiliary_node_without_string_map_payload.
Print Assumptions evaluated_child_of_map_free_program_is_not_a_string_map.
Print Assumptions aligned_children_of_map_free_program_are_not_string_maps.
Print Assumptions string_map_free_children_are_ordered.
Print Assumptions actual_children_of_non_new_family_are_ordered.
Print Assumptions message_summary_from_agreement.
Print Assumptions message_summary_premises_from_actual_children.
Print Assumptions source_message_summary_up_to_map_key_order.
Print Assumptions source_message_summary_projects_to_reducer_given_family_results.
Print Assumptions source_message_summary_projects_to_reducer_outside_new_family.
Print Assumptions source_message_summary_equals_reducer_iff_map_keys_ordered_or_target_refuses.
Print Assumptions unordered_injections_children_are_actual.
Print Assumptions unordered_injections_are_unordered.
Print Assumptions unordered_injections_lookup.
Print Assumptions unordered_injections_refusals_differ.
Print Assumptions actual_unrefused_children_do_not_project_to_reducer_without_key_order.
Print Assumptions requested_statement_contradicts_its_family_results.
Print Assumptions process_assembly_statement_holds.
Print Assumptions name_family_statement_holds.
Print Assumptions source_message_summary_projects_to_reducer.
