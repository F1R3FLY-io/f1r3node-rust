From Stdlib Require Import List Bool Arith NArith ZArith Lia.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3SemanticReducer
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3IndependentOracleProcess RPB3IndependentOracleProjection.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Target := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Process := RPB3IndependentOracleProcess.
Module Projection := RPB3IndependentOracleProjection.

Definition projected_process_children
    (children : list Core.source_exact_summary)
    : list Target.semantic_summary :=
  map Projection.project_source_summary children.

(** The fields of [process_child_correspondence] that read a scalar, or that
    apply to one family's arm, are stated only where a family reducer reads
    them.  The unrestricted forms are false on the children the oracle
    actually produces: a varint field of a non-bool kind carrying the byte
    [1] reads as [true] at both target boolean readers while the oracle reads
    a non-bool scalar.  The closed refutations are in
    [RPB3IndependentOracleProcessChildCorrespondence].  Each slot function
    below lists exactly the (family, tag) pairs its reader is applied to by
    the score functions in this theory. *)
Definition process_scalar_true_slot (family tag : nat) : bool :=
  match family, tag with
  | 8, 2 | 8, 3 | 15, 3 | 15, 6 | 23, 5 => true
  | _, _ => false
  end.

Definition process_exact_bool_slot (family tag : nat) : bool :=
  match family, tag with
  | 19, 3 | 19, 4 | 19, 7 | 22, 5 => true
  | _, _ => false
  end.

Definition process_i32_slot (family tag : nat) : bool :=
  match family, tag with
  | 19, 5 | 21, 3 => true
  | _, _ => false
  end.

Record process_child_correspondence
    (family : nat) (children : list Core.source_exact_summary) : Prop := {
  process_child_score_corresponds : forall tag,
    Target.exact_child_score tag (projected_process_children children) =
      option_map Core.project_score (Process.source_child_score tag children);
  process_selected_scores_correspond : forall tag,
    Target.exact_selected_scores tag (projected_process_children children) =
      option_map (map Core.project_score)
        (Process.source_selected_scores tag children);
  process_optional_score_corresponds : forall tag,
    Target.exact_optional_child_score tag
      (projected_process_children children) =
      option_map (option_map Core.project_score)
        (Process.source_optional_score tag children);
  process_scalar_true_corresponds : forall tag,
    process_scalar_true_slot family tag = true ->
    Target.scalar_true_or_absent tag (projected_process_children children) =
      Process.source_bool_or_absent tag children;
  process_bool_corresponds : forall tag,
    process_exact_bool_slot family tag = true ->
    Target.exact_bool_or_absent tag (projected_process_children children) =
      Process.source_bool_or_absent tag children;
  process_i32_corresponds : forall tag,
    process_i32_slot family tag = true ->
    Target.exact_pbint32_or_absent tag
      (projected_process_children children) =
      Process.source_i32_or_absent tag children;
  process_new_word_corresponds :
    family = 20 ->
    Target.new_bind_count_word (projected_process_children children) =
      option_map Process.source_signed_word
        (Process.source_i32_or_absent 1 children);
  process_typed_child_corresponds : forall tag type_id,
    Target.exact_typed_child_score tag type_id
      (projected_process_children children) =
      option_map Core.project_score
        (Process.source_typed_child_score tag type_id children);
  process_typed_selected_corresponds : forall tag type_id,
    Target.exact_typed_selected_scores tag type_id
      (projected_process_children children) =
      option_map (map Core.project_score)
        (Process.source_typed_selected_scores tag type_id children);
  process_typed_optional_corresponds : forall tag type_id,
    Target.exact_typed_optional_child_score tag type_id
      (projected_process_children children) =
      option_map (option_map Core.project_score)
        (Process.source_typed_optional_score tag type_id children);
  process_typed_fallback_corresponds : forall tag type_id fallback,
    Target.exact_typed_optional_score_or tag type_id
      (Core.project_score fallback) (projected_process_children children) =
      option_map Core.project_score
        (Process.source_typed_optional_score_or tag type_id fallback
          children);
  process_expr_arm_corresponds :
    family = 24 -> forall child,
    In child children ->
    Target.exact_expr_score [Projection.project_source_summary child] =
      option_map Core.project_score (Process.source_expr_score [child]);
  process_scalar_arm_corresponds :
    family = 24 -> forall child,
    In child children ->
    Schema.payload_eqb
      (Schema.raw_payload (Core.source_summary_raw child)) Schema.Scalar =
      match Core.source_summary_value child with
      | Core.TypedSourceScalar _ _ => true
      | _ => false
      end;
  process_receive_default_atom_corresponds :
    family = 19 -> forall child,
    In child children ->
    ((Core.source_summary_tag child =? 8) &&
      negb (Core.source_summary_refused child) &&
      match Process.source_par_typed_default
          (Core.source_summary_value child) with
      | Some true => true | _ => false end) =
    ((Target.sem_tag (Projection.project_source_summary child) =? 8) &&
      (Target.sem_type_id (Projection.project_source_summary child) =? 1) &&
      Schema.wire_eqb
        (Target.sem_wire (Projection.project_source_summary child))
        Schema.LengthDelimited &&
      Schema.payload_eqb
        (Target.sem_payload (Projection.project_source_summary child))
        Schema.Message &&
      negb (Target.sem_definitely_refused
        (Projection.project_source_summary child)) &&
      Target.par_source_is_semantic_default
        (Target.sem_body (Projection.project_source_summary child)));
  process_match_default_atom_corresponds :
    family = 22 -> forall child,
    In child children ->
    Process.source_match_case_has_default_guard child =
      Target.match_case_has_present_default_guard
        (Projection.project_source_summary child)
}.

Lemma projected_process_seen_slot :
  forall source_state target_state slot,
    Admission.source_scan_seen_slots source_state =
      Schema.scan_seen target_state ->
    Admission.source_seen_slot source_state slot =
      Target.seen_slot target_state slot.
Proof.
  intros source_state target_state slot seen.
  unfold Admission.source_seen_slot, Target.seen_slot.
  now rewrite <- seen.
Qed.

Lemma projected_process_bind_sources :
  forall children,
    Process.source_all_bind_sources_present children =
      Target.all_bind_sources_present
        (projected_process_children children).
Proof.
  induction children as [|child rest IH]; simpl; [reflexivity|].
  unfold projected_process_children in *.
  cbn [map forallb].
  change (Target.sem_tag (Projection.project_source_summary child))
    with (Core.source_summary_tag child).
  change (Target.sem_bind_source_present
      (Projection.project_source_summary child))
    with (Core.source_family_bind_source_present
      (Core.source_summary_analysis child)).
  now rewrite <- IH.
Qed.

Lemma projected_process_case_requirements :
  forall children,
    Process.source_all_match_cases_required children =
      Target.all_match_cases_required
        (projected_process_children children).
Proof.
  induction children as [|child rest IH]; simpl; [reflexivity|].
  unfold projected_process_children in *.
  cbn [map forallb].
  change (Target.sem_tag (Projection.project_source_summary child))
    with (Core.source_summary_tag child).
  change (Target.sem_case_required_present
      (Projection.project_source_summary child))
    with (Core.source_family_case_required_present
      (Core.source_summary_analysis child)).
  now rewrite <- IH.
Qed.

Lemma projected_process_completeness :
  forall type_id source_state target_state children,
    In type_id [8; 15; 16; 19; 20; 21; 22; 23; 24] ->
    Admission.source_scan_seen_slots source_state =
      Schema.scan_seen target_state ->
    process_child_correspondence type_id children ->
    Process.source_process_completeness type_id source_state children =
      Target.component_completeness type_id target_state
        (projected_process_children children).
Proof.
  intros type_id source_state target_state children family seen correspond.
  assert (slot : forall index,
    Admission.source_seen_slot source_state index =
      Target.seen_slot target_state index).
  { intro index. now apply projected_process_seen_slot. }
  destruct family as
    [family_eq|[family_eq|[family_eq|[family_eq|[family_eq|
    [family_eq|[family_eq|[family_eq|[family_eq|[]]]]]]]]]];
    subst type_id;
    cbn [Process.source_process_completeness
      Target.component_completeness];
    try rewrite (slot 0); try rewrite (slot 1);
    try rewrite (slot 2);
    try rewrite projected_process_bind_sources;
    try rewrite projected_process_case_requirements;
    try reflexivity.
  destruct children as [|child [|second rest]]; simpl; try reflexivity.
  unfold projected_process_children.
  cbn [map].
  rewrite (process_scalar_arm_corresponds _ _ correspond eq_refl child
    (or_introl eq_refl)).
  destruct (Core.source_summary_value child); reflexivity.
Qed.

Lemma projected_process_bytes_leq :
  forall left right,
    Process.source_bytes_leq left right = Target.body_leq left right.
Proof.
  induction left as [|first rest IH]; intros [|second suffix];
    simpl; try reflexivity.
Qed.

Lemma projected_process_byte_sequence_ordered :
  forall bodies,
    Process.source_byte_sequence_ordered bodies =
      Target.body_sequence_ordered bodies.
Proof.
  intros bodies.
  reflexivity.
Qed.

Lemma projected_process_existsb :
  forall (source_predicate : Core.source_exact_summary -> bool)
    (target_predicate : Target.semantic_summary -> bool) children,
    (forall child, In child children ->
      source_predicate child =
        target_predicate (Projection.project_source_summary child)) ->
    existsb source_predicate children =
      existsb target_predicate (projected_process_children children).
Proof.
  intros source_predicate target_predicate children.
  induction children as [|child rest IH]; intros corresponds;
    [reflexivity|].
  cbn [existsb projected_process_children map].
  rewrite (corresponds child (or_introl eq_refl)).
  f_equal.
  apply IH.
  intros member present.
  apply corresponds.
  now right.
Qed.

Lemma projected_process_new_uri_order :
  forall children,
    Process.source_new_uri_ordered children =
      Target.new_uri_ordered (projected_process_children children).
Proof.
  intro children.
  unfold Process.source_new_uri_ordered, Target.new_uri_ordered.
  rewrite projected_process_byte_sequence_ordered.
  f_equal.
  induction children as [|child rest IH]; simpl; [reflexivity|].
  unfold projected_process_children in *.
  cbn [map filter].
  destruct (Core.source_summary_tag child =? 3) eqn:tag;
    cbn [map filter];
    change (Projection.Schema.raw_tag
      (Projection.Core.source_summary_raw child) =? 3)
      with (Core.source_summary_tag child =? 3);
    rewrite tag; cbn [map]; now rewrite <- IH.
Qed.

Lemma projected_process_receive_default :
  forall children,
    process_child_correspondence 19 children ->
    Process.source_present_default_condition children =
      Target.receive_has_present_default_condition
        (projected_process_children children).
Proof.
  intros children correspond.
  unfold Process.source_present_default_condition,
    Target.receive_has_present_default_condition.
  apply projected_process_existsb.
  intros child present.
  exact (process_receive_default_atom_corresponds _ _ correspond eq_refl child
    present).
Qed.

Lemma projected_process_match_default :
  forall children,
    process_child_correspondence 22 children ->
    Process.source_present_default_match_guard children =
      existsb Target.match_case_has_present_default_guard
        (projected_process_children children).
Proof.
  intros children correspond.
  unfold Process.source_present_default_match_guard.
  apply projected_process_existsb.
  intros child present.
  exact (process_match_default_atom_corresponds _ _ correspond eq_refl child
    present).
Qed.

Lemma projected_process_bundle_child_score :
  forall children,
    Target.exact_bundle_score children =
      match Target.exact_child_score 1 children,
          Target.scalar_true_or_absent 2 children,
          Target.scalar_true_or_absent 3 children with
      | Some body, Some write, Some read =>
          Some (Target.score_constructor
            (Target.bundle_constructor read write) [body])
      | _, _, _ => None
      end.
Proof.
  intro children.
  unfold Target.exact_bundle_score, Target.exact_child_score.
  destruct (find (fun child => Target.sem_tag child =? 1) children)
    as [body|];
    destruct (Target.scalar_true_or_absent 2 children) as [write|];
    destruct (Target.scalar_true_or_absent 3 children) as [read|];
    try reflexivity.
  all: destruct (Target.sem_score_evidence body); reflexivity.
Qed.

Ltac finish_process_score :=
  cbn [option_map];
  repeat rewrite Core.project_source_score_constructor;
  repeat rewrite Core.project_source_score_forest_of_list;
  repeat rewrite map_app;
  cbn [map Core.project_score Process.source_bool_word
    Process.source_bundle_constructor Target.bundle_constructor];
  try reflexivity.

Lemma projected_process_bundle_score :
  forall children,
    process_child_correspondence 8 children ->
    Target.exact_bundle_score (projected_process_children children) =
      option_map Core.project_score (Process.source_bundle_score children).
Proof.
  intros children correspond.
  rewrite projected_process_bundle_child_score.
  rewrite (process_child_score_corresponds _ _ correspond 1).
  rewrite (process_scalar_true_corresponds _ _ correspond 2 eq_refl).
  rewrite (process_scalar_true_corresponds _ _ correspond 3 eq_refl).
  unfold Process.source_bundle_score.
  destruct (Process.source_child_score 1 children) as [body|];
    destruct (Process.source_bool_or_absent 2 children) as [write|];
    destruct (Process.source_bool_or_absent 3 children) as [read|];
    try reflexivity.
Qed.

Lemma projected_process_send_score :
  forall children,
    process_child_correspondence 15 children ->
    Target.exact_send_score (projected_process_children children) =
      option_map Core.project_score (Process.source_send_score children).
Proof.
  intros children correspond.
  unfold Target.exact_send_score, Process.source_send_score.
  rewrite (process_scalar_true_corresponds _ _ correspond 3 eq_refl).
  rewrite (process_child_score_corresponds _ _ correspond 1).
  rewrite (process_selected_scores_correspond _ _ correspond 2).
  rewrite (process_scalar_true_corresponds _ _ correspond 6 eq_refl).
  rewrite (process_optional_score_corresponds _ _ correspond 7).
  destruct (Process.source_bool_or_absent 3 children) as [persistent|];
    destruct (Process.source_child_score 1 children) as [channel|];
    destruct (Process.source_selected_scores 2 children) as [data|];
    destruct (Process.source_bool_or_absent 6 children) as [connective|];
    destruct (Process.source_optional_score 7 children) as [[cost|]|];
    try reflexivity;
    finish_process_score.
  all: repeat rewrite map_app;
    cbn [map Process.source_bool_word]; reflexivity.
Qed.

Lemma projected_process_if_score :
  forall children,
    process_child_correspondence 23 children ->
    Target.exact_if_score (projected_process_children children) =
      option_map Core.project_score (Process.source_if_score children).
Proof.
  intros children correspond.
  unfold Target.exact_if_score, Process.source_if_score.
  rewrite (process_child_score_corresponds _ _ correspond 1).
  rewrite (process_child_score_corresponds _ _ correspond 2).
  rewrite (process_child_score_corresponds _ _ correspond 3).
  rewrite (process_scalar_true_corresponds _ _ correspond 5 eq_refl).
  destruct (Process.source_child_score 1 children) as [condition|];
    destruct (Process.source_child_score 2 children) as [if_true|];
    destruct (Process.source_child_score 3 children) as [if_false|];
    destruct (Process.source_bool_or_absent 5 children) as [connective|];
    try reflexivity;
    finish_process_score.
Qed.

Lemma projected_process_receive_bind_score :
  forall children,
    process_child_correspondence 16 children ->
    Target.exact_receive_bind_score
      (projected_process_children children) =
      option_map Core.project_score
        (Process.source_receive_bind_score children).
Proof.
  intros children correspond.
  unfold Target.exact_receive_bind_score,
    Process.source_receive_bind_score.
  rewrite (process_typed_child_corresponds _ _ correspond 2 1).
  rewrite (process_typed_selected_corresponds _ _ correspond 1 1).
  change (Target.ScoreInteger 0)
    with (Core.project_score (Core.SourceInteger 0)).
  rewrite (process_typed_fallback_corresponds _ _ correspond 3 6
    (Core.SourceInteger 0)).
  rewrite (process_typed_optional_corresponds _ _ correspond 5 12).
  destruct (Process.source_typed_child_score 2 1 children) as [source|];
    destruct (Process.source_typed_selected_scores 1 1 children)
      as [patterns|];
    destruct (Process.source_typed_optional_score_or 3 6
      (Core.SourceInteger 0) children) as [remainder|];
    destruct (Process.source_typed_optional_score 5 12 children)
      as [[cost|]|];
    try reflexivity;
    finish_process_score.
  all: rewrite Core.project_source_score_forest_of_list;
    cbn [map]; repeat rewrite map_app;
    cbn [map]; reflexivity.
Qed.

Lemma projected_process_empty_par_score :
  Core.project_score Process.source_empty_par_score =
    Target.empty_par_score.
Proof. reflexivity. Qed.

Lemma projected_process_match_case_score :
  forall children,
    process_child_correspondence 21 children ->
    Target.exact_match_case_score
      (projected_process_children children) =
      option_map Core.project_score
        (Process.source_match_case_score children).
Proof.
  intros children correspond.
  unfold Target.exact_match_case_score,
    Process.source_match_case_score.
  rewrite (process_typed_child_corresponds _ _ correspond 1 1).
  rewrite (process_typed_child_corresponds _ _ correspond 2 1).
  rewrite (process_i32_corresponds _ _ correspond 3 eq_refl).
  rewrite <- projected_process_empty_par_score.
  rewrite (process_typed_fallback_corresponds _ _ correspond 4 1
    Process.source_empty_par_score).
  destruct (Process.source_typed_child_score 1 1 children) as [pattern|];
    destruct (Process.source_typed_child_score 2 1 children) as [source|];
    destruct (Process.source_i32_or_absent 3 children) as [count|];
    destruct (Process.source_typed_optional_score_or 4 1
      Process.source_empty_par_score children) as [guard|];
    try reflexivity;
    finish_process_score.
Qed.

Lemma projected_process_new_uris :
  forall children,
    map Core.project_score (Process.source_new_uris children) =
      map (fun child => Target.ScoreString (Target.sem_body child))
        (filter (fun child => Target.sem_tag child =? 3)
          (projected_process_children children)).
Proof.
  induction children as [|child rest IH]; [reflexivity|].
  unfold Process.source_new_uris, projected_process_children in *.
  cbn [map filter].
  change (Target.sem_tag (Projection.project_source_summary child))
    with (Core.source_summary_tag child).
  change (Target.sem_body (Projection.project_source_summary child))
    with (Core.source_summary_body child).
  destruct (Core.source_summary_tag child =? 3);
    cbn [map Core.project_score]; now rewrite <- IH.
Qed.

Lemma projected_process_scores_or_absent :
  forall scores,
    map Core.project_score (Process.source_scores_or_absent scores) =
      Target.scores_or_absent (map Core.project_score scores).
Proof.
  intros [|first rest]; reflexivity.
Qed.

Lemma projected_process_new_score :
  forall children,
    process_child_correspondence 20 children ->
    Target.exact_new_score (projected_process_children children) =
      option_map Core.project_score (Process.source_new_score children).
Proof.
  intros children correspond.
  unfold Target.exact_new_score, Process.source_new_score.
  rewrite (process_new_word_corresponds _ _ correspond eq_refl).
  rewrite (process_child_score_corresponds _ _ correspond 2).
  rewrite (process_selected_scores_correspond _ _ correspond 4).
  destruct (Process.source_i32_or_absent 1 children) as [count|];
    destruct (Process.source_child_score 2 children) as [body|];
    destruct (Process.source_selected_scores 4 children) as [injections|];
    try reflexivity.
  cbn [option_map].
  rewrite Core.project_source_score_constructor.
  cbn [map].
  repeat rewrite map_app.
  rewrite <- projected_process_new_uris.
  repeat rewrite projected_process_scores_or_absent.
  cbn [map Core.project_score].
  reflexivity.
Qed.

Lemma projected_process_receive_score :
  forall children,
    process_child_correspondence 19 children ->
    Target.exact_receive_score (projected_process_children children) =
      option_map Core.project_score (Process.source_receive_score children).
Proof.
  intros children correspond.
  unfold Target.exact_receive_score, Process.source_receive_score.
  rewrite <- (projected_process_receive_default children correspond).
  destruct (Process.source_present_default_condition children) eqn:default;
    [reflexivity|].
  rewrite (process_bool_corresponds _ _ correspond 3 eq_refl).
  rewrite (process_bool_corresponds _ _ correspond 4 eq_refl).
  rewrite (process_typed_selected_corresponds _ _ correspond 1 16).
  rewrite (process_typed_child_corresponds _ _ correspond 2 1).
  rewrite (process_i32_corresponds _ _ correspond 5 eq_refl).
  rewrite (process_bool_corresponds _ _ correspond 7 eq_refl).
  rewrite <- projected_process_empty_par_score.
  rewrite (process_typed_fallback_corresponds _ _ correspond 8 1
    Process.source_empty_par_score).
  rewrite (process_typed_optional_corresponds _ _ correspond 9 12).
  destruct (Process.source_bool_or_absent 3 children) as [persistent|];
    destruct (Process.source_bool_or_absent 4 children) as [peek|];
    destruct (Process.source_typed_selected_scores 1 16 children)
      as [binds|];
    destruct (Process.source_typed_child_score 2 1 children) as [body|];
    destruct (Process.source_i32_or_absent 5 children) as [count|];
    destruct (Process.source_bool_or_absent 7 children) as [connective|];
    destruct (Process.source_typed_optional_score_or 8 1
      Process.source_empty_par_score children) as [condition|];
    destruct (Process.source_typed_optional_score 9 12 children)
      as [[cost|]|];
    try reflexivity;
    finish_process_score.
Qed.

Lemma projected_process_match_score :
  forall children,
    process_child_correspondence 22 children ->
    Target.exact_match_score (projected_process_children children) =
      option_map Core.project_score (Process.source_match_score children).
Proof.
  intros children correspond.
  unfold Target.exact_match_score, Process.source_match_score.
  rewrite <- (projected_process_match_default children correspond).
  destruct (Process.source_present_default_match_guard children)
    eqn:default; [reflexivity|].
  rewrite (process_typed_child_corresponds _ _ correspond 1 1).
  rewrite (process_typed_selected_corresponds _ _ correspond 2 21).
  rewrite (process_bool_corresponds _ _ correspond 5 eq_refl).
  destruct (Process.source_typed_child_score 1 1 children) as [target|];
    destruct (Process.source_typed_selected_scores 2 21 children)
      as [cases|];
    destruct (Process.source_bool_or_absent 5 children) as [connective|];
    try reflexivity;
    finish_process_score.
  all: repeat rewrite map_app;
    cbn [map Process.source_bool_word]; reflexivity.
Qed.

Lemma projected_process_expr_score :
  forall children,
    process_child_correspondence 24 children ->
    Target.exact_expr_score (projected_process_children children) =
      option_map Core.project_score (Process.source_expr_score children).
Proof.
  intros children correspond.
  destruct children as [|child [|second rest]]; simpl; try reflexivity.
  exact (process_expr_arm_corresponds _ _ correspond eq_refl child
    (or_introl eq_refl)).
Qed.

Theorem projected_process_family_score :
  forall type_id complete children,
    In type_id [8; 15; 16; 19; 20; 21; 22; 23; 24] ->
    process_child_correspondence type_id children ->
    Target.score_evidence_for type_id complete
      (projected_process_children children) =
      Projection.project_source_evidence
        (Process.source_process_family_score type_id complete children).
Proof.
  intros type_id complete children family correspond.
  destruct family as
    [family_eq|[family_eq|[family_eq|[family_eq|[family_eq|
    [family_eq|[family_eq|[family_eq|[family_eq|[]]]]]]]]]];
    subst type_id;
    destruct complete as [[|]|];
    cbn [Target.score_evidence_for
      Process.source_process_family_score
      Target.incomplete_component_uses_nonsemantic_score];
    unfold Target.exact_or_unknown,
      Projection.project_source_evidence;
    try rewrite (projected_process_bundle_score children correspond);
    try rewrite (projected_process_send_score children correspond);
    try rewrite (projected_process_receive_bind_score children correspond);
    try rewrite (projected_process_receive_score children correspond);
    try rewrite (projected_process_new_score children correspond);
    try rewrite (projected_process_match_case_score children correspond);
    try rewrite (projected_process_match_score children correspond);
    try rewrite (projected_process_if_score children correspond);
    try rewrite (projected_process_expr_score children correspond);
    try reflexivity;
    try (destruct (Process.source_bundle_score children); reflexivity);
    try (destruct (Process.source_send_score children); reflexivity);
    try (destruct (Process.source_receive_bind_score children); reflexivity);
    try (destruct (Process.source_receive_score children); reflexivity);
    try (destruct (Process.source_new_score children); reflexivity);
    try (destruct (Process.source_match_case_score children); reflexivity);
    try (destruct (Process.source_match_score children); reflexivity);
    try (destruct (Process.source_if_score children); reflexivity);
    try (destruct (Process.source_expr_score children); reflexivity).
Qed.

Theorem projected_process_family_refusal :
  forall type_id complete source_state target_state children,
    In type_id [8; 15; 16; 19; 20; 21; 22; 23; 24] ->
    Admission.source_scan_seen_slots source_state =
      Schema.scan_seen target_state ->
    process_child_correspondence type_id children ->
    Process.source_process_family_refused type_id complete source_state
      children =
      ((if (type_id =? 20) && Target.seen_slot target_state 1
        then negb (Target.new_uri_ordered
          (projected_process_children children)) else false) ||
       Target.complete_receive_has_default_condition type_id complete
         (projected_process_children children) ||
       Target.complete_match_has_default_guard type_id complete
         (projected_process_children children)).
Proof.
  intros type_id complete source_state target_state children
    family seen correspond.
  destruct family as
    [family_eq|[family_eq|[family_eq|[family_eq|[family_eq|
    [family_eq|[family_eq|[family_eq|[family_eq|[]]]]]]]]]];
    subst type_id;
    destruct complete as [[|]|];
    cbn [Process.source_process_family_refused
      Target.complete_receive_has_default_condition
      Target.complete_match_has_default_guard Nat.eqb];
    try rewrite <- (projected_process_receive_default children correspond);
    try rewrite <- (projected_process_match_default children correspond);
    try rewrite <- (projected_process_new_uri_order children);
    try rewrite <- (projected_process_seen_slot source_state
      target_state 1 seen);
    cbn [andb orb negb];
    repeat rewrite orb_false_r;
    try reflexivity;
    destruct (Admission.source_seen_slot source_state 1);
      destruct (Process.source_new_uri_ordered children); reflexivity.
Qed.

Lemma projected_process_target_order_refusal :
  forall type_id state children,
    In type_id [8; 15; 16; 19; 20; 21; 22; 23; 24] ->
    Target.order_definitely_bad
      (if type_id =? 1 then Target.raw_par_component_order children
       else if (type_id =? 20) && Target.seen_slot state 1
       then Some (Target.new_uri_ordered children)
       else Some true) =
      if (type_id =? 20) && Target.seen_slot state 1
      then negb (Target.new_uri_ordered children) else false.
Proof.
  intros type_id state children family.
  destruct family as
    [family_eq|[family_eq|[family_eq|[family_eq|[family_eq|
    [family_eq|[family_eq|[family_eq|[family_eq|[]]]]]]]]]];
    subst type_id;
    cbn [Target.order_definitely_bad];
    try reflexivity.
  destruct (Target.seen_slot state 1);
    destruct (Target.new_uri_ordered children); reflexivity.
Qed.

Theorem source_process_message_projects_to_target :
  forall raw type_id program children,
    In type_id [8; 15; 16; 19; 20; 21; 22; 23; 24] ->
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some type_id ->
    Schema.lookup_projected_program type_id [] [] = Some program ->
    Schema.program_family program = type_id ->
    Admission.source_raw_bytes_well_formed raw = true ->
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = true ->
    Admission.source_required_pathmaps_present program
      (Admission.source_scan_children program children) = true ->
    process_child_correspondence type_id children ->
    let source_state := Admission.source_scan_children program children in
    let target_summary := Target.reduce_semantic_field raw
      (projected_process_children children) in
    Target.sem_complete target_summary =
      Process.source_process_completeness type_id source_state children /\
    Target.sem_score_evidence target_summary =
      Projection.project_source_evidence
        (Process.source_process_family_score type_id
          (Process.source_process_completeness type_id source_state children)
          children) /\
    Target.sem_definitely_refused target_summary =
      Process.source_process_family_refused type_id
        (Process.source_process_completeness type_id source_state children)
        source_state children.
Proof.
  intros raw type_id program children family payload program_id lookup
    program_family body_ok source_valid paths_present correspond.
  assert (resolved : Target.generated_wire_program raw = Some program).
  { unfold Target.generated_wire_program.
    now rewrite payload, program_id, lookup. }
  pose proof (Projection.source_scan_children_project_valid program
    children source_valid) as [target_valid [_ seen_same]].
  change (Schema.scan_valid
    (fold_left (Target.scan_child program)
      (projected_process_children children) Schema.empty_scan) = true)
    in target_valid.
  pose proof (Projection.required_pathmaps_project program
    (Admission.source_scan_children program children)
    (fold_left (Target.scan_child program)
      (projected_process_children children) Schema.empty_scan)
    seen_same paths_present) as target_paths.
  pose proof (projected_process_completeness type_id
    (Admission.source_scan_children program children)
    (fold_left (Target.scan_child program)
      (projected_process_children children) Schema.empty_scan)
    children family seen_same correspond) as complete.
  pose proof (projected_process_family_refusal type_id
    (Process.source_process_completeness type_id
      (Admission.source_scan_children program children) children)
    (Admission.source_scan_children program children)
    (fold_left (Target.scan_child program)
      (projected_process_children children) Schema.empty_scan)
    children family seen_same correspond) as refused.
  change (Schema.valid_raw_body raw = true) in body_ok.
  unfold Target.reduce_semantic_field.
  rewrite resolved.
  cbn iota.
  cbn [Target.sem_complete Target.sem_score_evidence
    Target.sem_definitely_refused].
  rewrite program_family, payload, body_ok.
  rewrite target_valid, target_paths.
  rewrite <- complete.
  rewrite (projected_process_target_order_refusal type_id
    (fold_left (Target.scan_child program)
      (projected_process_children children) Schema.empty_scan)
    (projected_process_children children) family).
  rewrite (projected_process_family_score type_id
    (Process.source_process_completeness type_id
      (Admission.source_scan_children program children) children)
    children family correspond).
  split; [reflexivity|].
  split; [reflexivity|].
  repeat (replace (negb true) with false by reflexivity).
  repeat rewrite orb_false_l.
  assert (not_fixed_point_family :
    Target.payload_fixed_point_violated type_id
      (Process.source_process_completeness type_id
        (Admission.source_scan_children program children) children)
      (projected_process_children children) = false).
  { apply Target.payload_fixed_point_violated_other_family;
      intro same; subst type_id; cbn [In] in family; lia. }
  rewrite not_fixed_point_family, orb_false_r.
  rewrite <- refused.
  reflexivity.
Qed.

Print Assumptions projected_process_seen_slot.
Print Assumptions projected_process_bind_sources.
Print Assumptions projected_process_case_requirements.
Print Assumptions projected_process_completeness.
Print Assumptions projected_process_new_uri_order.
Print Assumptions projected_process_receive_default.
Print Assumptions projected_process_match_default.
Print Assumptions projected_process_bundle_score.
Print Assumptions projected_process_send_score.
Print Assumptions projected_process_if_score.
Print Assumptions projected_process_receive_bind_score.
Print Assumptions projected_process_match_case_score.
Print Assumptions projected_process_new_score.
Print Assumptions projected_process_receive_score.
Print Assumptions projected_process_match_score.
Print Assumptions projected_process_expr_score.
Print Assumptions projected_process_family_score.
Print Assumptions projected_process_family_refusal.
Print Assumptions source_process_message_projects_to_target.
