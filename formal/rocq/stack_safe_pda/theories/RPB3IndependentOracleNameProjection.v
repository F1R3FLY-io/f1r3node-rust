From Stdlib Require Import List Bool Arith NArith Lia.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3SourceScalarReduction
  RPB3SemanticReducer RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission RPB3IndependentOracleName
  RPB3IndependentOracleProjection.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Scalar := RPB3SourceScalarReduction.
Module Target := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Name := RPB3IndependentOracleName.
Module Projection := RPB3IndependentOracleProjection.

Definition projected_name_children
    (children : list Core.source_exact_summary)
    : list Target.semantic_summary :=
  map Projection.project_source_summary children.

Definition source_score_at (tag : nat)
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match Name.source_child_at tag children with
  | Some child => Core.source_summary_exact_score child
  | None => None
  end.

Lemma projected_source_score_at :
  forall tag children,
    Target.exact_child_score tag (projected_name_children children) =
      option_map Core.project_score (source_score_at tag children).
Proof.
  intros tag children.
  unfold Target.exact_child_score, projected_name_children,
    source_score_at, Name.source_child_at.
  rewrite <- Projection.projected_find_tag.
  destruct (find (fun child => Core.source_summary_tag child =? tag)
      children) as [child|]; [|reflexivity].
  cbn [option_map].
  change (Target.sem_score_evidence
      (Projection.project_source_summary child))
    with (Projection.project_source_evidence
      (Core.source_summary_exact_score child)).
  unfold Projection.project_source_evidence.
  destruct (Core.source_summary_exact_score child);
    reflexivity.
Qed.

Lemma projected_pathmap_auxiliary_score :
  forall raw,
    Core.project_score (Name.source_pathmap_auxiliary_score raw) =
      Target.ScoreBytes (Schema.raw_body raw).
Proof. reflexivity. Qed.

Lemma projected_empty_par_score :
  Core.project_score Name.source_empty_par_score =
    Target.empty_par_score.
Proof.
  unfold Name.source_empty_par_score, Target.empty_par_score.
  now rewrite Core.project_source_score_constructor.
Qed.

Lemma projected_string_map_absent_score :
  forall key,
    option_map Core.project_score
      (Name.source_string_map_auxiliary_score key None) =
    Some (Target.ScoreNode (Target.score_forest_of_list
      [Target.ScoreString key; Target.empty_par_score])).
Proof.
  intro key.
  unfold Name.source_string_map_auxiliary_score.
  cbn [option_map].
  reflexivity.
Qed.

Lemma projected_string_map_present_score :
  forall key summary score,
    Core.source_summary_refused summary = false ->
    Core.source_summary_exact_score summary = Some score ->
    option_map Core.project_score
      (Name.source_string_map_auxiliary_score key
        (Some (false, summary))) =
    Some (Target.ScoreNode (Target.score_forest_of_list
      [Target.ScoreString key; Core.project_score score])).
Proof.
  intros key summary score not_refused exact.
  unfold Name.source_string_map_auxiliary_score.
  rewrite not_refused, exact.
  cbn [option_map].
  reflexivity.
Qed.

Lemma projected_pathmap_auxiliary_summary :
  forall raw snapshot values summary,
    Core.source_summary_value summary =
      Core.TypedSourcePathMap raw snapshot values ->
    Core.source_summary_analysis summary =
      Core.SourceFamilyAnalysis (Some true)
        (Some (Name.source_pathmap_auxiliary_score raw))
        false false false ->
    Target.sem_payload (Projection.project_source_summary summary) =
      Schema.raw_payload raw /\
    Target.sem_body (Projection.project_source_summary summary) =
      Schema.raw_body raw /\
    Target.sem_complete (Projection.project_source_summary summary) =
      Some true /\
    Target.sem_definitely_refused
      (Projection.project_source_summary summary) = false /\
    Target.sem_score_evidence
      (Projection.project_source_summary summary) =
      Target.ExactScore (Target.ScoreBytes (Schema.raw_body raw)).
Proof.
  intros raw snapshot values summary source_value analysis.
  unfold Projection.project_source_summary, Core.source_summary_raw.
  rewrite source_value, analysis.
  repeat split; reflexivity.
Qed.

Lemma projected_string_map_auxiliary_summary :
  forall raw key value summary source_score,
    Core.source_summary_value summary =
      Core.TypedSourceStringMap raw key value ->
    Core.source_summary_analysis summary =
      Core.SourceFamilyAnalysis (Some true) (Some source_score)
        false false false ->
    Target.sem_payload (Projection.project_source_summary summary) =
      Schema.raw_payload raw /\
    Target.sem_body (Projection.project_source_summary summary) =
      Schema.raw_body raw /\
    Target.sem_complete (Projection.project_source_summary summary) =
      Some true /\
    Target.sem_definitely_refused
      (Projection.project_source_summary summary) = false /\
    Target.sem_score_evidence
      (Projection.project_source_summary summary) =
      Target.ExactScore (Core.project_score source_score).
Proof.
  intros raw key value summary source_score source_value analysis.
  unfold Projection.project_source_summary, Core.source_summary_raw.
  rewrite source_value, analysis.
  repeat split; reflexivity.
Qed.

Lemma source_epathmap_score_via_score_at :
  forall children,
    Name.source_epathmap_score children =
    match source_score_at 9 children,
        Name.source_bool_or_absent 4 children with
    | Some (Core.SourceBytes snapshot), Some connective =>
        Some (Core.source_score_constructor 13
          [Core.SourceBytes snapshot;
           Core.SourceInteger (if connective then 1 else 0)])
    | _, _ => None
    end.
Proof.
  intro children.
  unfold Name.source_epathmap_score, source_score_at.
  destruct (Name.source_child_at 9 children) as [child|];
    destruct (Name.source_bool_or_absent 4 children) as [flag|];
    try reflexivity.
  destruct (Core.source_summary_exact_score child) as [score|];
    try reflexivity.
  destruct score; reflexivity.
Qed.

Lemma projected_epathmap_score :
  forall children,
    Target.scalar_true_or_absent 4 (projected_name_children children) =
      Name.source_bool_or_absent 4 children ->
    Target.exact_epathmap_score (projected_name_children children) =
      option_map Core.project_score
        (Name.source_epathmap_score children).
Proof.
  intros children bool_corresponds.
  unfold Target.exact_epathmap_score.
  rewrite projected_source_score_at, bool_corresponds.
  rewrite source_epathmap_score_via_score_at.
  destruct (source_score_at 9 children) as [score|];
    destruct (Name.source_bool_or_absent 4 children) as [flag|];
    try reflexivity.
  all: destruct score; cbn; reflexivity.
Qed.

Lemma projected_pathmap_snapshot :
  forall source_score,
    Target.pathmap_snapshot_from_score
      (Core.project_score source_score) =
    Name.source_pathmap_snapshot_from_score source_score.
Proof.
  intro source_score.
  destruct source_score as [word|string|bytes|forest];
    try reflexivity.
  destruct forest as [|first rest]; try reflexivity.
  destruct first as [constructor|string|bytes|nested];
    try reflexivity.
  destruct rest as [|second rest]; try reflexivity.
  destruct second as [word|string|snapshot|nested];
    try reflexivity.
  destruct rest as [|third rest]; try reflexivity.
  destruct third as [flag|string|bytes|nested];
    try reflexivity.
  destruct rest; reflexivity.
Qed.

Lemma source_ezipper_score_via_score_at :
  forall children,
    Name.source_ezipper_score children =
    match source_score_at 1 children,
        Name.source_bool_or_absent 5 children with
    | Some pathmap, Some connective =>
        option_map (fun snapshot => Core.source_score_constructor 14
          [Core.SourceBytes snapshot;
           Core.SourceInteger (if connective then 1 else 0)])
          (Name.source_pathmap_snapshot_from_score pathmap)
    | _, _ => None
    end.
Proof.
  intro children.
  unfold Name.source_ezipper_score, source_score_at.
  destruct (Name.source_child_at 1 children) as [child|];
    destruct (Name.source_bool_or_absent 5 children) as [flag|];
    try reflexivity.
  destruct (Core.source_summary_exact_score child);
    reflexivity.
Qed.

Lemma projected_ezipper_score :
  forall children,
    Target.scalar_true_or_absent 5 (projected_name_children children) =
      Name.source_bool_or_absent 5 children ->
    Target.exact_ezipper_score (projected_name_children children) =
      option_map Core.project_score (Name.source_ezipper_score children).
Proof.
  intros children bool_corresponds.
  unfold Target.exact_ezipper_score.
  rewrite projected_source_score_at, bool_corresponds.
  rewrite source_ezipper_score_via_score_at.
  destruct (source_score_at 1 children) as [score|];
    destruct (Name.source_bool_or_absent 5 children) as [flag|];
    try reflexivity.
  cbn [option_map].
  rewrite projected_pathmap_snapshot.
  destruct (Name.source_pathmap_snapshot_from_score score)
    as [snapshot|]; [|reflexivity].
  cbn [option_map].
  reflexivity.
Qed.

Lemma projected_name_completeness :
  forall type_id source_state target_state children,
    In type_id [31; 32; 55; 60] ->
    Admission.source_scan_seen_slots source_state =
      Schema.scan_seen target_state ->
    Name.source_name_completeness type_id source_state =
      Target.component_completeness type_id target_state
        (projected_name_children children).
Proof.
  intros type_id source_state target_state children family seen.
  assert (slot : forall tag,
    Admission.source_seen_slot source_state tag =
      Target.seen_slot target_state tag).
  { intro tag.
    unfold Admission.source_seen_slot, Target.seen_slot.
    now rewrite <- seen. }
  destruct family as [family|[family|[family|[family|[]]]]];
    subst type_id;
    cbn [Name.source_name_completeness
      Target.component_completeness];
    try rewrite (slot 3); try rewrite (slot 0);
    reflexivity.
Qed.

Definition connective_child_corresponds
    (child : Core.source_exact_summary) : Prop :=
  Name.source_connective_scalar_constructor
      (Core.source_summary_tag child) =
    Target.connective_scalar_constructor
      (Core.source_summary_tag child) /\
  match Core.source_summary_value child with
  | Core.TypedSourceScalar _ (Core.SourceBool value) =>
      Schema.raw_wire (Core.source_summary_raw child) =
        Schema.Varint /\
      Schema.raw_payload (Core.source_summary_raw child) =
        Schema.Scalar /\
      Scalar.scalar_semantic_from_source
        (Scalar.ScalarRule (Core.source_summary_tag child)
          Data.PBBool false) (Core.source_summary_body child) =
        Some (Scalar.ScalarBool value)
  | Core.TypedSourceMessage _ _ =>
      Schema.raw_wire (Core.source_summary_raw child) =
        Schema.LengthDelimited /\
      Schema.raw_payload (Core.source_summary_raw child) =
        Schema.Message
  | _ => True
  end.

Definition unforgeable_child_corresponds
    (child : Core.source_exact_summary) : Prop :=
  Name.source_unforgeable_contract (Core.source_summary_tag child) =
    Target.unforgeable_arm_contract (Core.source_summary_tag child) /\
  forall type_id constructor bytes,
    Name.source_unforgeable_contract
      (Core.source_summary_tag child) =
      Some (type_id, constructor, true) ->
    Name.source_unforgeable_leaf_bytes child = Some bytes ->
    Target.exact_unforgeable_leaf_bytes type_id
      (Core.source_summary_body child) = Some bytes.

Lemma projected_connective_score_success :
  forall children score,
    Forall connective_child_corresponds children ->
    Name.source_connective_score children = Some score ->
    Target.exact_connective_score
      (projected_name_children children) =
      Some (Core.project_score score).
Proof.
  intros children score correspond accepted.
  destruct children as [|child [|second rest]].
  - inversion accepted; subst score; reflexivity.
  - inversion correspond as [|? ? child_corresponds ?]; subst.
    destruct child_corresponds as [constructor_corresponds shape].
    unfold Name.source_connective_score in accepted.
    unfold Target.exact_connective_score,
      projected_name_children.
    cbn [map].
    change (Target.sem_tag (Projection.project_source_summary child))
      with (Core.source_summary_tag child).
    change (Target.sem_wire (Projection.project_source_summary child))
      with (Schema.raw_wire (Core.source_summary_raw child)).
    change (Target.sem_payload (Projection.project_source_summary child))
      with (Schema.raw_payload (Core.source_summary_raw child)).
    change (Target.sem_body (Projection.project_source_summary child))
      with (Core.source_summary_body child).
    change (Target.sem_definitely_refused
        (Projection.project_source_summary child))
      with (Core.source_summary_refused child).
    destruct (Core.source_summary_refused child) eqn:refused;
      [discriminate|].
    destruct (Core.source_summary_value child) as
      [raw scalar|raw values|raw key value|raw snapshot values]
      eqn:source_value; try discriminate.
    + destruct scalar; try discriminate.
      destruct shape as [wire [payload decoder]].
      destruct (Name.source_connective_scalar_constructor
          (Core.source_summary_tag child)) as [constructor|]
        eqn:source_constructor; [|discriminate].
      cbn [option_map] in accepted.
      inversion accepted; subst score.
      rewrite wire, payload.
      rewrite <- constructor_corresponds.
      rewrite decoder.
      rewrite Core.project_source_score_constructor.
      reflexivity.
    + destruct shape as [wire payload].
      destruct ((Core.source_summary_tag child =? 3) &&
          match Core.source_summary_type_id child with
          | Some type_id => type_id =? 1
          | None => false
          end) eqn:arm; [|discriminate].
      apply andb_true_iff in arm as [tag_is_three type_is_one].
      destruct (Core.source_summary_type_id child)
        as [observed|] eqn:source_type; [|discriminate].
      apply Nat.eqb_eq in type_is_one.
      subst observed.
      destruct (Core.source_summary_exact_score child)
        as [nested|] eqn:exact; [|discriminate].
      cbn [option_map] in accepted.
      inversion accepted; subst score.
      rewrite wire, payload, tag_is_three.
      change (Target.sem_type_id
          (Projection.project_source_summary child))
        with (match Core.source_summary_type_id child with
          | Some type_id => type_id | None => 0 end).
      rewrite source_type.
      change (Target.sem_score_evidence
          (Projection.project_source_summary child))
        with (Projection.project_source_evidence
          (Core.source_summary_exact_score child)).
      unfold Projection.project_source_evidence.
      rewrite exact.
      rewrite Core.project_source_score_constructor.
      reflexivity.
  - discriminate.
Qed.

Lemma projected_unforgeable_score_success :
  forall children score,
    Forall unforgeable_child_corresponds children ->
    Name.source_unforgeable_score children = Some score ->
    Target.exact_unforgeable_score
      (projected_name_children children) =
      Some (Core.project_score score).
Proof.
  intros children score correspond accepted.
  destruct children as [|child [|second rest]].
  - inversion accepted; subst score.
    rewrite Core.project_source_score_constructor.
    reflexivity.
  - inversion correspond as [|? ? child_corresponds ?]; subst.
    destruct child_corresponds as [contract_corresponds leaf_corresponds].
    unfold Name.source_unforgeable_score in accepted.
    unfold Target.exact_unforgeable_score,
      projected_name_children.
    cbn [map].
    change (Target.sem_tag (Projection.project_source_summary child))
      with (Core.source_summary_tag child).
    change (Target.sem_wire (Projection.project_source_summary child))
      with (Schema.raw_wire (Core.source_summary_raw child)).
    change (Target.sem_payload (Projection.project_source_summary child))
      with (Schema.raw_payload (Core.source_summary_raw child)).
    change (Target.sem_body (Projection.project_source_summary child))
      with (Core.source_summary_body child).
    change (Target.sem_definitely_refused
        (Projection.project_source_summary child))
      with (Core.source_summary_refused child).
    change (Target.sem_type_id
        (Projection.project_source_summary child))
      with (match Core.source_summary_type_id child with
        | Some type_id => type_id | None => 0 end).
    destruct (Core.source_summary_refused child) eqn:refused;
      [discriminate|].
    destruct (Name.source_unforgeable_contract
        (Core.source_summary_tag child)) as
        [[[type_id constructor] bytes_required]|]
      eqn:source_contract; [|discriminate].
    rewrite <- contract_corresponds.
    destruct (Core.source_summary_type_id child)
      as [observed|] eqn:source_type; [|discriminate].
    destruct ((observed =? type_id) &&
        Schema.wire_eqb
          (Schema.raw_wire (Core.source_summary_raw child))
          Schema.LengthDelimited &&
        Schema.payload_eqb
          (Schema.raw_payload (Core.source_summary_raw child))
          Schema.Message) eqn:source_guard; [|discriminate].
    repeat rewrite andb_true_iff in source_guard.
    destruct source_guard as [[type_matches wire_matches]
      payload_matches].
    apply Nat.eqb_eq in type_matches.
    subst observed.
    destruct (Schema.raw_wire (Core.source_summary_raw child))
      eqn:wire; cbn [Schema.wire_eqb] in wire_matches;
      try discriminate.
    destruct (Schema.raw_payload (Core.source_summary_raw child))
      eqn:payload; cbn [Schema.payload_eqb] in payload_matches;
      try discriminate.
    rewrite Nat.eqb_refl.
    destruct bytes_required.
    + destruct (Name.source_unforgeable_leaf_bytes child)
        as [bytes|] eqn:source_leaf; [|discriminate].
      cbn [option_map] in accepted.
      inversion accepted; subst score.
      cbn [option_map].
      rewrite (leaf_corresponds type_id constructor bytes
        eq_refl eq_refl).
      rewrite Core.project_source_score_constructor.
      reflexivity.
    + destruct (length (Core.source_summary_body child) =? 0)
        eqn:empty; [|discriminate].
      inversion accepted; subst score.
      rewrite Core.project_source_score_constructor.
      reflexivity.
  - discriminate.
Qed.

Theorem projected_connective_family_score_success :
  forall children score,
    Forall connective_child_corresponds children ->
    Name.source_name_family_score 55 (Some true) children = Some score ->
    Target.score_evidence_for 55 (Some true)
      (projected_name_children children) =
      Target.ExactScore (Core.project_score score).
Proof.
  intros children score correspond accepted.
  change (Name.source_connective_score children = Some score)
    in accepted.
  cbn [Target.score_evidence_for].
  unfold Target.exact_or_unknown.
  now rewrite (projected_connective_score_success children score
    correspond accepted).
Qed.

Theorem projected_unforgeable_family_score_success :
  forall children score,
    Forall unforgeable_child_corresponds children ->
    Name.source_name_family_score 60 (Some true) children = Some score ->
    Target.score_evidence_for 60 (Some true)
      (projected_name_children children) =
      Target.ExactScore (Core.project_score score).
Proof.
  intros children score correspond accepted.
  change (Name.source_unforgeable_score children = Some score)
    in accepted.
  cbn [Target.score_evidence_for].
  unfold Target.exact_or_unknown.
  now rewrite (projected_unforgeable_score_success children score
    correspond accepted).
Qed.

Print Assumptions projected_source_score_at.
Print Assumptions projected_pathmap_auxiliary_score.
Print Assumptions projected_empty_par_score.
Print Assumptions projected_string_map_absent_score.
Print Assumptions projected_string_map_present_score.
Print Assumptions projected_pathmap_auxiliary_summary.
Print Assumptions projected_string_map_auxiliary_summary.
Print Assumptions projected_epathmap_score.
Print Assumptions projected_pathmap_snapshot.
Print Assumptions projected_ezipper_score.
Print Assumptions projected_name_completeness.
Print Assumptions projected_connective_score_success.
Print Assumptions projected_unforgeable_score_success.
Print Assumptions projected_connective_family_score_success.
Print Assumptions projected_unforgeable_family_score_success.
