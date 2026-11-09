From Stdlib Require Import List Bool Arith NArith ZArith Lia.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3SourceScalarReduction
  RPB3SemanticReducer RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission RPB3IndependentOracleCost.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Scalar := RPB3SourceScalarReduction.
Module Target := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Cost := RPB3IndependentOracleCost.

Definition project_source_evidence
    (score : option Core.source_score) : Target.score_evidence :=
  match score with
  | Some value => Target.ExactScore (Core.project_score value)
  | None => Target.ScoreNotDerived
  end.

Definition project_source_type_id
    (raw : Schema.raw_field) : nat :=
  match Schema.raw_program_type_id raw with
  | Some type_id => type_id
  | None => 0
  end.

Definition project_source_summary
    (source : Core.source_exact_summary) : Target.semantic_summary :=
  let raw := Core.source_summary_raw source in
  let analysis := Core.source_summary_analysis source in
  Target.SemanticSummary
    (Schema.raw_tag raw) (Schema.raw_wire raw)
    (Schema.raw_payload raw) (project_source_type_id raw)
    (Schema.raw_body raw) (Core.source_family_refused analysis)
    (Core.source_family_complete analysis)
    (project_source_evidence (Core.source_family_score analysis))
    (Core.source_family_bind_source_present analysis)
    (Core.source_family_case_required_present analysis).

Definition i32_child_binds (source : Core.source_exact_summary) : Prop :=
  match Core.source_summary_value source with
  | Core.TypedSourceScalar _ (Core.SourceI32 value) =>
      Scalar.scalar_semantic_from_source
        (Scalar.ScalarRule (Core.source_summary_tag source)
          Data.PBSint32 true) (Core.source_summary_body source) =
        Some (Scalar.ScalarI32 value)
  | _ => False
  end.

Definition u32_child_binds (source : Core.source_exact_summary) : Prop :=
  match Core.source_summary_value source with
  | Core.TypedSourceScalar _ (Core.SourceU32 value) =>
      Scalar.scalar_semantic_from_source
        (Scalar.ScalarRule (Core.source_summary_tag source)
          Data.PBUint32 true) (Core.source_summary_body source) =
        Some (Scalar.ScalarU32 value)
  | _ => False
  end.

Definition u64_child_binds (source : Core.source_exact_summary) : Prop :=
  match Core.source_summary_value source with
  | Core.TypedSourceScalar _ (Core.SourceU64 value) =>
      Scalar.scalar_semantic_from_source
        (Scalar.ScalarRule (Core.source_summary_tag source)
          Data.PBUint64 true) (Core.source_summary_body source) =
        Some (Scalar.ScalarU64 value)
  | _ => False
  end.

Definition bytes_child_binds
    (source : Core.source_exact_summary) : Prop :=
  match Core.source_summary_value source with
  | Core.TypedSourceScalar _ (Core.SourceBlob bytes) =>
      bytes = Core.source_summary_body source
  | _ => False
  end.

Definition cost_atom_payload_corresponds
    (source : Core.source_exact_summary) : Prop :=
  match Core.source_summary_value source with
  | Core.TypedSourceScalar _ _ =>
      Schema.raw_payload (Core.source_summary_raw source) = Schema.Scalar
  | Core.TypedSourceMessage _ _ =>
      Schema.raw_payload (Core.source_summary_raw source) = Schema.Message
  | _ => True
  end.

Definition cost_child_corresponds (type_id : nat)
    (source : Core.source_exact_summary) : Prop :=
  let tag := Core.source_summary_tag source in
  (type_id = 10 ->
    (tag = 1 \/ tag = 2 \/ tag = 4) -> bytes_child_binds source) /\
  (type_id = 11 -> tag = 1 -> i32_child_binds source) /\
  (type_id = 12 -> (tag = 1 \/ tag = 7) -> u32_child_binds source) /\
  (type_id = 12 -> (tag = 2 \/ tag = 3 \/ tag = 4 \/ tag = 5) ->
    u64_child_binds source) /\
  (type_id = 10 -> cost_atom_payload_corresponds source).

Lemma projected_source_fields :
  forall source,
    Target.sem_tag (project_source_summary source) =
      Core.source_summary_tag source /\
    Target.sem_wire (project_source_summary source) =
      Schema.raw_wire (Core.source_summary_raw source) /\
    Target.sem_payload (project_source_summary source) =
      Schema.raw_payload (Core.source_summary_raw source) /\
    Target.sem_type_id (project_source_summary source) =
      project_source_type_id (Core.source_summary_raw source) /\
    Target.sem_body (project_source_summary source) =
      Core.source_summary_body source /\
    Target.sem_definitely_refused (project_source_summary source) =
      Core.source_summary_refused source /\
    Target.sem_score_evidence (project_source_summary source) =
      project_source_evidence (Core.source_summary_exact_score source).
Proof.
  intros source. repeat split; reflexivity.
Qed.

Lemma projected_selected_scores :
  forall tag children scores,
    Cost.selected_source_scores tag children = Some scores ->
    Target.exact_selected_scores tag
      (map project_source_summary children) =
      Some (map Core.project_score scores).
Proof.
  intros tag children.
  induction children as [|child rest IH]; intros scores selected.
  - inversion selected; reflexivity.
  - cbn [Cost.selected_source_scores] in selected.
    destruct (Cost.selected_source_scores tag rest) as [tail|]
      eqn:rest_scores; [|discriminate].
      specialize (IH tail eq_refl).
    cbn [map Target.exact_selected_scores].
    rewrite IH.
    change (Target.sem_tag (project_source_summary child))
      with (Core.source_summary_tag child).
    destruct (Core.source_summary_tag child =? tag) eqn:chosen.
    + destruct (Core.source_summary_exact_score child) as [score|]
        eqn:exact; [|discriminate].
      inversion selected; subst scores.
      change (Target.sem_score_evidence (project_source_summary child))
        with (project_source_evidence (Core.source_summary_exact_score child)).
      unfold project_source_evidence.
      rewrite exact. reflexivity.
    + inversion selected; subst scores. reflexivity.
Qed.

Lemma projected_find_tag :
  forall tag children,
    option_map project_source_summary
      (find (fun child => Core.source_summary_tag child =? tag)
        children) =
    find (fun child => Target.sem_tag child =? tag)
      (map project_source_summary children).
Proof.
  intros tag children.
  induction children as [|child rest IH]; [reflexivity|].
  cbn [find map].
  change (Target.sem_tag (project_source_summary child))
    with (Core.source_summary_tag child).
  destruct (Core.source_summary_tag child =? tag); auto.
Qed.

Lemma projected_i32_or_zero :
  forall children,
    Forall (cost_child_corresponds 11) children ->
    Target.exact_i32_or_absent 1
      (map project_source_summary children) =
    Cost.scalar_i32_or_zero 1 children.
Proof.
  intros children correspond.
  unfold Target.exact_i32_or_absent, Cost.scalar_i32_or_zero.
  rewrite <- projected_find_tag.
  destruct (find (fun child => Core.source_summary_tag child =? 1)
      children) as [child|] eqn:found; [|reflexivity].
  apply find_some in found as [present tag].
  apply Nat.eqb_eq in tag.
  pose proof (proj1 (Forall_forall _ _) correspond child present)
    as child_correspond.
  destruct child_correspond as [_ [bind _]].
  specialize (bind eq_refl tag).
  unfold i32_child_binds in bind.
  destruct (Core.source_summary_value child) as
    [raw value|raw values|raw key value|raw snapshot values];
    try contradiction.
  destruct value; try contradiction.
  cbn [option_map].
  change (Target.sem_body (project_source_summary child))
    with (Core.source_summary_body child).
  rewrite <- tag. now rewrite bind.
Qed.

Lemma projected_u32_or_zero :
  forall tag children,
    (tag = 1 \/ tag = 7) ->
    Forall (cost_child_corresponds 12) children ->
    Target.exact_u32_or_absent tag
      (map project_source_summary children) =
    Cost.scalar_u32_or_zero tag children.
Proof.
  intros tag children expected correspond.
  unfold Target.exact_u32_or_absent, Cost.scalar_u32_or_zero.
  rewrite <- projected_find_tag.
  destruct (find (fun child => Core.source_summary_tag child =? tag)
      children) as [child|] eqn:found; [|reflexivity].
  apply find_some in found as [present chosen].
  apply Nat.eqb_eq in chosen.
  pose proof (proj1 (Forall_forall _ _) correspond child present)
    as child_correspond.
  destruct child_correspond as [_ [_ [bind _]]].
  specialize (bind eq_refl).
  assert (selected : Core.source_summary_tag child = 1 \/
      Core.source_summary_tag child = 7).
  { rewrite chosen. exact expected. }
  specialize (bind selected).
  unfold u32_child_binds in bind.
  destruct (Core.source_summary_value child) as
    [raw value|raw values|raw key value|raw snapshot values];
    try contradiction.
  destruct value; try contradiction.
  cbn [option_map].
  change (Target.sem_body (project_source_summary child))
    with (Core.source_summary_body child).
  rewrite <- chosen. now rewrite bind.
Qed.

Lemma projected_u64_or_zero :
  forall tag children,
    (tag = 2 \/ tag = 3 \/ tag = 4 \/ tag = 5) ->
    Forall (cost_child_corresponds 12) children ->
    Target.exact_u64_or_absent tag
      (map project_source_summary children) =
    Cost.scalar_u64_or_zero tag children.
Proof.
  intros tag children expected correspond.
  unfold Target.exact_u64_or_absent, Cost.scalar_u64_or_zero.
  rewrite <- projected_find_tag.
  destruct (find (fun child => Core.source_summary_tag child =? tag)
      children) as [child|] eqn:found; [|reflexivity].
  apply find_some in found as [present chosen].
  apply Nat.eqb_eq in chosen.
  pose proof (proj1 (Forall_forall _ _) correspond child present)
    as child_correspond.
  destruct child_correspond as [_ [_ [_ [bind _]]]].
  specialize (bind eq_refl).
  assert (selected : Core.source_summary_tag child = 2 \/
      Core.source_summary_tag child = 3 \/
      Core.source_summary_tag child = 4 \/
      Core.source_summary_tag child = 5).
  { rewrite chosen. exact expected. }
  specialize (bind selected).
  unfold u64_child_binds in bind.
  destruct (Core.source_summary_value child) as
    [raw value|raw values|raw key value|raw snapshot values];
    try contradiction.
  destruct value; try contradiction.
  cbn [option_map].
  change (Target.sem_body (project_source_summary child))
    with (Core.source_summary_body child).
  rewrite <- chosen. now rewrite bind.
Qed.

Lemma projected_optional_score :
  forall tag children score,
    Cost.optional_source_score tag children = Some score ->
    Target.exact_optional_child_score tag
      (map project_source_summary children) =
      Some (option_map Core.project_score score).
Proof.
  intros tag children score selected.
  unfold Cost.optional_source_score,
    Target.exact_optional_child_score in *.
  rewrite <- projected_find_tag.
  destruct (find (fun child => Core.source_summary_tag child =? tag)
      children) as [child|] eqn:found.
  - destruct (Core.source_summary_exact_score child) as [child_score|]
      eqn:exact; [|discriminate].
    inversion selected; subst score.
    cbn [option_map].
    change (Target.sem_score_evidence (project_source_summary child))
      with (project_source_evidence (Core.source_summary_exact_score child)).
    unfold project_source_evidence.
    rewrite exact. reflexivity.
  - inversion selected; reflexivity.
Qed.

Lemma projected_cost_atom_score :
  forall children score,
    Forall (cost_child_corresponds 10) children ->
    Cost.cost_atom_score children = Some score ->
    Target.exact_cost_atom_score
      (map project_source_summary children) =
      Some (Core.project_score score).
Proof.
  intros children score correspond accepted.
  destruct children as [|child [|second rest]].
  - inversion accepted; subst score.
    cbn [map Target.exact_cost_atom_score].
    rewrite Core.project_source_score_constructor.
    reflexivity.
  - inversion correspond as [|? ? child_binds ?]; subst.
    unfold cost_child_corresponds in child_binds.
    destruct child_binds as [bytes_binds [_ [_ [_ payload_binds]]]].
    specialize (payload_binds eq_refl).
    unfold cost_atom_payload_corresponds in payload_binds.
    cbn [Cost.cost_atom_score] in accepted.
    cbn [map Target.exact_cost_atom_score].
    change (Target.sem_tag (project_source_summary child))
      with (Core.source_summary_tag child).
    change (Target.sem_payload (project_source_summary child))
      with (Schema.raw_payload (Core.source_summary_raw child)).
    change (Target.sem_body (project_source_summary child))
      with (Core.source_summary_body child).
    destruct (Core.source_summary_tag child) as
      [|[|[|[|[|extra]]]]] eqn:tag;
      destruct (Core.source_summary_value child) as
      [raw value|raw values|raw key value|raw snapshot values]
      eqn:value_kind;
      try discriminate;
      try (match goal with
        | scalar : Core.source_scalar_value |- _ =>
            destruct scalar; try discriminate
        end).
    all: try (specialize (bytes_binds eq_refl (or_introl eq_refl));
      unfold bytes_child_binds in bytes_binds;
      rewrite value_kind in bytes_binds;
      cbn in bytes_binds;
      rewrite payload_binds;
      inversion accepted; subst score;
      rewrite Core.project_source_score_constructor;
      rewrite bytes_binds; reflexivity).
    all: try (specialize (bytes_binds eq_refl
      (or_intror (or_introl eq_refl)));
      unfold bytes_child_binds in bytes_binds;
      rewrite value_kind in bytes_binds;
      cbn in bytes_binds;
      rewrite payload_binds;
      inversion accepted; subst score;
      rewrite Core.project_source_score_constructor;
      rewrite bytes_binds; reflexivity).
    all: try (specialize (bytes_binds eq_refl
        (or_intror (or_intror eq_refl)));
      unfold bytes_child_binds in bytes_binds;
      rewrite value_kind in bytes_binds;
      cbn in bytes_binds;
      rewrite payload_binds;
      inversion accepted; subst score;
      rewrite Core.project_source_score_constructor;
      rewrite bytes_binds; reflexivity).
    all: try (destruct (Core.source_summary_exact_score child)
      as [nested|] eqn:exact; [|discriminate];
      inversion accepted; subst score;
      rewrite payload_binds;
      change (Target.sem_score_evidence (project_source_summary child))
        with (project_source_evidence
          (Core.source_summary_exact_score child));
      unfold project_source_evidence;
      rewrite exact; reflexivity).
  - discriminate.
Qed.

Lemma projected_cost_bound_score :
  forall children score,
    Forall (cost_child_corresponds 11) children ->
    Cost.cost_bound_score children = Some score ->
    Target.exact_cost_bound_score
      (map project_source_summary children) =
      Some (Core.project_score score).
Proof.
  intros children score correspond accepted.
  unfold Cost.cost_bound_score in accepted.
  destruct (Cost.scalar_i32_or_zero 1 children) as [index|]
    eqn:decoded; [|discriminate].
  inversion accepted; subst score.
  unfold Target.exact_cost_bound_score.
  rewrite projected_i32_or_zero by exact correspond.
  rewrite decoded.
  unfold Cost.signed_source_word, Target.signed_score_word.
  cbn [option_map].
  rewrite Core.project_source_score_constructor.
  reflexivity.
Qed.

Lemma projected_cost_region_score :
  forall children score,
    Forall (cost_child_corresponds 12) children ->
    Cost.cost_region_score children = Some score ->
    Target.exact_cost_region_score
      (map project_source_summary children) =
      Some (Core.project_score score).
Proof.
  intros children score correspond accepted.
  unfold Cost.cost_region_score in accepted.
  destruct (Cost.scalar_u32_or_zero 1 children) as [version|]
    eqn:version_seen; [|discriminate].
  destruct (Cost.scalar_u64_or_zero 2 children) as [start_line|]
    eqn:start_line_seen; [|discriminate].
  destruct (Cost.scalar_u64_or_zero 3 children) as [start_column|]
    eqn:start_column_seen; [|discriminate].
  destruct (Cost.scalar_u64_or_zero 4 children) as [end_line|]
    eqn:end_line_seen; [|discriminate].
  destruct (Cost.scalar_u64_or_zero 5 children) as [end_column|]
    eqn:end_column_seen; [|discriminate].
  destruct (Cost.optional_source_score 6 children) as [signer|]
    eqn:signer_seen; [|discriminate].
  destruct (Cost.scalar_u32_or_zero 7 children) as [ordinal|]
    eqn:ordinal_seen; [|discriminate].
  inversion accepted; subst score.
  unfold Target.exact_cost_region_score.
  rewrite (projected_u32_or_zero 1 children (or_introl eq_refl)
    correspond), version_seen.
  rewrite (projected_u64_or_zero 2 children (or_introl eq_refl)
    correspond), start_line_seen.
  rewrite (projected_u64_or_zero 3 children
    (or_intror (or_introl eq_refl)) correspond), start_column_seen.
  rewrite (projected_u64_or_zero 4 children
    (or_intror (or_intror (or_introl eq_refl))) correspond),
    end_line_seen.
  rewrite (projected_u64_or_zero 5 children
    (or_intror (or_intror (or_intror eq_refl))) correspond),
    end_column_seen.
  rewrite (projected_optional_score 6 children signer signer_seen).
  rewrite (projected_u32_or_zero 7 children (or_intror eq_refl)
    correspond), ordinal_seen.
  destruct signer as [signer_score|];
    cbn [option_map Core.project_score];
    rewrite Core.project_source_score_constructor;
    reflexivity.
Qed.

Lemma projected_cost_family_score :
  forall type_id children score,
    In type_id [9; 10; 11; 12; 13] ->
    Forall (cost_child_corresponds type_id) children ->
    Cost.cost_family_score type_id children = Some score ->
    Target.score_evidence_for type_id
      (if type_id =? 13 then Some true else None)
      (map project_source_summary children) =
      Target.ExactScore (Core.project_score score).
Proof.
  intros type_id children score family correspond accepted.
  destruct family as [family|[family|[family|[family|[family|[]]]]]];
    subst type_id.
  - cbn [Cost.cost_family_score] in accepted.
    destruct (Cost.selected_source_scores 1 children) as [atoms|]
      eqn:selected; [|discriminate].
    inversion accepted; subst score.
    cbn [Target.score_evidence_for].
    unfold Target.exact_or_unknown, Target.exact_cost_collection_score.
    rewrite (projected_selected_scores 1 children atoms selected).
    now rewrite Core.project_source_score_constructor.
  - cbn [Cost.cost_family_score] in accepted.
    cbn [Target.score_evidence_for Target.exact_or_unknown].
    rewrite (projected_cost_atom_score children score correspond accepted).
    reflexivity.
  - cbn [Cost.cost_family_score] in accepted.
    cbn [Target.score_evidence_for Target.exact_or_unknown].
    rewrite (projected_cost_bound_score children score correspond accepted).
    reflexivity.
  - cbn [Cost.cost_family_score] in accepted.
    cbn [Target.score_evidence_for Target.exact_or_unknown].
    rewrite (projected_cost_region_score children score correspond accepted).
    reflexivity.
  - cbn [Cost.cost_family_score] in accepted.
    destruct (Cost.selected_source_scores 1 children) as [layers|]
      eqn:selected; [|discriminate].
    inversion accepted; subst score.
    cbn [Target.score_evidence_for].
    unfold Target.exact_or_unknown, Target.exact_cost_collection_score.
    rewrite (projected_selected_scores 1 children layers selected).
    now rewrite Core.project_source_score_constructor.
Qed.

Lemma source_descriptor_guard_projects :
  forall descriptor child,
    Admission.source_child_matches_descriptor descriptor child = true ->
    (negb (Target.sem_definitely_refused
        (project_source_summary child)) &&
     Schema.wire_eqb
       (Target.sem_wire (project_source_summary child))
       (Schema.descriptor_wire descriptor) &&
     Schema.payload_eqb
       (Target.sem_payload (project_source_summary child))
       (Schema.descriptor_payload descriptor) &&
     match Schema.descriptor_payload descriptor with
     | Schema.Message =>
         match Schema.descriptor_child_type_id descriptor with
         | Some expected =>
             Target.sem_type_id (project_source_summary child) =?
               expected
         | None => false
         end
     | _ => true
     end) = true.
Proof.
  intros descriptor child checked.
  unfold Admission.source_child_matches_descriptor in checked.
  repeat rewrite andb_true_iff in checked.
  assert (not_refused : negb (Core.source_summary_refused child) = true)
    by tauto.
  assert (wire : Schema.wire_eqb
    (Schema.raw_wire (Core.source_summary_raw child))
    (Schema.descriptor_wire descriptor) = true) by tauto.
  assert (payload : Schema.payload_eqb
    (Schema.raw_payload (Core.source_summary_raw child))
    (Schema.descriptor_payload descriptor) = true) by tauto.
  assert (source_type :
    match Schema.descriptor_payload descriptor with
    | Schema.Message =>
        match Schema.descriptor_child_type_id descriptor,
            Core.source_summary_type_id child with
        | Some expected, Some observed => expected =? observed
        | _, _ => false
        end
    | _ => true
    end = true) by tauto.
  assert (target_type :
    match Schema.descriptor_payload descriptor with
    | Schema.Message =>
        match Schema.descriptor_child_type_id descriptor with
        | Some expected =>
            Target.sem_type_id (project_source_summary child) =?
              expected
        | None => false
        end
    | _ => true
    end = true).
  { destruct (Schema.descriptor_payload descriptor);
      try reflexivity.
    destruct (Schema.descriptor_child_type_id descriptor)
      as [expected|]; [|discriminate].
    destruct (Core.source_summary_type_id child)
      as [observed|] eqn:source_id; [|discriminate].
    change (Target.sem_type_id (project_source_summary child))
      with (match Core.source_summary_type_id child with
        | Some type_id => type_id
        | None => 0
        end).
    rewrite source_id.
    rewrite Nat.eqb_sym.
    exact source_type. }
  change (Target.sem_definitely_refused (project_source_summary child))
    with (Core.source_summary_refused child).
  change (Target.sem_wire (project_source_summary child))
    with (Schema.raw_wire (Core.source_summary_raw child)).
  change (Target.sem_payload (project_source_summary child))
    with (Schema.raw_payload (Core.source_summary_raw child)).
  rewrite not_refused, wire, payload.
  exact target_type.
Qed.

Lemma source_scan_step_projects :
  forall program source_state target_state child,
    Admission.source_scan_valid source_state = true ->
    Schema.scan_valid target_state = true ->
    Admission.source_scan_last_slot source_state =
      Schema.scan_last_slot target_state ->
    Admission.source_scan_seen_slots source_state =
      Schema.scan_seen target_state ->
    Admission.source_scan_valid
      (Admission.source_scan_step program source_state child) = true ->
    Schema.scan_valid
      (Target.scan_child program target_state
        (project_source_summary child)) = true /\
    Admission.source_scan_last_slot
      (Admission.source_scan_step program source_state child) =
      Schema.scan_last_slot
        (Target.scan_child program target_state
          (project_source_summary child)) /\
    Admission.source_scan_seen_slots
      (Admission.source_scan_step program source_state child) =
      Schema.scan_seen
        (Target.scan_child program target_state
          (project_source_summary child)).
Proof.
  intros program source_state target_state child source_valid
    target_valid last_seen seen_seen next_valid.
  unfold Admission.source_scan_step in next_valid |- *.
  unfold Target.scan_child.
  rewrite source_valid in next_valid |- *.
  rewrite target_valid.
  change (Target.sem_tag (project_source_summary child))
    with (Core.source_summary_tag child).
  destruct (Schema.resolve_tag (Schema.program_fields program)
      (Core.source_summary_tag child)) as [descriptor|]
    eqn:resolved; [|discriminate].
  set (slot := Schema.descriptor_slot descriptor) in *.
  destruct (Admission.source_child_matches_descriptor descriptor child &&
      (slot <? Schema.program_slot_count program) &&
      Admission.source_slot_follows
        (Admission.source_scan_last_slot source_state) slot
        (Schema.descriptor_repeated descriptor) &&
      (Schema.descriptor_repeated descriptor ||
        negb (existsb (Nat.eqb slot)
          (Admission.source_scan_seen_slots source_state))))
    eqn:source_guard; [|discriminate].
  repeat rewrite andb_true_iff in source_guard.
  assert (child_guard :
    Admission.source_child_matches_descriptor descriptor child = true)
    by tauto.
  assert (slot_bound : (slot <? Schema.program_slot_count program) = true)
    by tauto.
  assert (slot_order : Admission.source_slot_follows
    (Admission.source_scan_last_slot source_state) slot
    (Schema.descriptor_repeated descriptor) = true) by tauto.
  assert (slot_unique :
    (Schema.descriptor_repeated descriptor ||
      negb (existsb (Nat.eqb slot)
        (Admission.source_scan_seen_slots source_state))) = true)
    by tauto.
  pose proof (source_descriptor_guard_projects descriptor child
    child_guard) as target_guard.
  rewrite last_seen, seen_seen in *.
  change (Schema.slot_follows
    (Schema.scan_last_slot target_state) slot
    (Schema.descriptor_repeated descriptor) = true) in slot_order.
  rewrite target_guard, slot_bound, slot_order, slot_unique.
  repeat split; reflexivity.
Qed.

Lemma source_scan_invalid_stays_invalid :
  forall program children state,
    Admission.source_scan_valid state = false ->
    Admission.source_scan_valid
      (fold_left (Admission.source_scan_step program)
        children state) = false.
Proof.
  intros program children.
  induction children as [|child rest IH]; intros state invalid;
    cbn [fold_left].
  - exact invalid.
  - rewrite (Admission.source_scan_step_invalid_sticky
      program state child invalid).
    apply IH. exact invalid.
Qed.

Lemma source_scan_children_project_valid :
  forall program children,
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = true ->
    Schema.scan_valid
      (fold_left (Target.scan_child program)
        (map project_source_summary children) Schema.empty_scan) = true /\
    Admission.source_scan_last_slot
      (Admission.source_scan_children program children) =
      Schema.scan_last_slot
        (fold_left (Target.scan_child program)
          (map project_source_summary children) Schema.empty_scan) /\
    Admission.source_scan_seen_slots
      (Admission.source_scan_children program children) =
      Schema.scan_seen
        (fold_left (Target.scan_child program)
          (map project_source_summary children) Schema.empty_scan).
Proof.
  intros program children.
  unfold Admission.source_scan_children.
  revert children.
  assert (general : forall children source_state target_state,
    Admission.source_scan_valid source_state = true ->
    Schema.scan_valid target_state = true ->
    Admission.source_scan_last_slot source_state =
      Schema.scan_last_slot target_state ->
    Admission.source_scan_seen_slots source_state =
      Schema.scan_seen target_state ->
    Admission.source_scan_valid
      (fold_left (Admission.source_scan_step program)
        children source_state) = true ->
    Schema.scan_valid
      (fold_left (Target.scan_child program)
        (map project_source_summary children) target_state) = true /\
    Admission.source_scan_last_slot
      (fold_left (Admission.source_scan_step program)
        children source_state) =
      Schema.scan_last_slot
        (fold_left (Target.scan_child program)
          (map project_source_summary children) target_state) /\
    Admission.source_scan_seen_slots
      (fold_left (Admission.source_scan_step program)
        children source_state) =
      Schema.scan_seen
        (fold_left (Target.scan_child program)
          (map project_source_summary children) target_state)).
  { induction children as [|child rest IH];
      intros source_state target_state source_valid target_valid
        last_seen seen_seen final_valid;
      cbn [fold_left map] in *.
    - repeat split; assumption.
    - assert (step_valid :
        Admission.source_scan_valid
          (Admission.source_scan_step program source_state child) = true).
      { destruct (Admission.source_scan_valid
          (Admission.source_scan_step program source_state child))
          eqn:step; [reflexivity|].
        pose proof (source_scan_invalid_stays_invalid program rest
          (Admission.source_scan_step program source_state child)
          step) as impossible.
        rewrite final_valid in impossible. discriminate. }
      pose proof (source_scan_step_projects program source_state
        target_state child source_valid target_valid last_seen
        seen_seen step_valid) as [next_valid [next_last next_seen]].
      apply IH; assumption. }
  intros children final_valid.
  apply general; try reflexivity.
  exact final_valid.
Qed.

Lemma required_pathmaps_project :
  forall program source_state target_state,
    Admission.source_scan_seen_slots source_state =
      Schema.scan_seen target_state ->
    Admission.source_required_pathmaps_present program source_state =
      true ->
    Target.mandatory_pathmaps_present program target_state = true.
Proof.
  intros program source_state target_state same_seen source_required.
  unfold Admission.source_required_pathmaps_present in source_required.
  unfold Target.mandatory_pathmaps_present.
  unfold Admission.source_seen_slot in source_required.
  unfold Target.seen_slot.
  rewrite <- same_seen.
  induction (Schema.program_fields program) as [|descriptor rest IH];
    [reflexivity|].
  cbn [forallb] in *.
  apply andb_true_iff in source_required as [head tail].
  rewrite IH by exact tail.
  destruct (Schema.descriptor_payload descriptor);
    cbn [Schema.payload_eqb]; try reflexivity.
  now rewrite head.
Qed.

Lemma cost_program_projects_to_target :
  forall raw type_id program,
    Cost.cost_program raw = Some (type_id, program) ->
    Target.generated_wire_program raw = Some program /\
    Schema.raw_payload raw = Schema.Message /\
    Schema.raw_program_type_id raw = Some type_id /\
    In type_id [9; 10; 11; 12; 13].
Proof.
  intros raw type_id program selected.
  unfold Cost.cost_program in selected.
  destruct (Schema.raw_payload raw) eqn:payload;
    try discriminate.
  destruct (Schema.raw_program_type_id raw) as [observed|]
    eqn:program_id; [|discriminate].
  destruct (Cost.cost_family observed) eqn:family;
    [|discriminate].
  destruct (Schema.lookup_projected_program observed [] [])
    as [found|] eqn:lookup; [|discriminate].
  inversion selected; subst type_id program.
  unfold Target.generated_wire_program.
  rewrite payload, program_id, lookup.
  repeat split; try reflexivity.
  unfold Cost.cost_family in family.
  apply andb_true_iff in family as [lower upper].
  apply Nat.leb_le in lower.
  apply Nat.leb_le in upper.
  simpl. lia.
Qed.

Lemma cost_family_completion_projects :
  forall type_id scan children,
    In type_id [9; 10; 11; 12; 13] ->
    Target.component_completeness type_id scan children =
      if type_id =? 13 then Some true else None.
Proof.
  intros type_id scan children family.
  destruct family as [family|[family|[family|[family|[family|[]]]]]];
    subst type_id; reflexivity.
Qed.

Theorem accepted_cost_message_projects_exactly :
  forall raw type_id program children summary,
    Cost.cost_program raw = Some (type_id, program) ->
    Forall (cost_child_corresponds type_id) children ->
    Cost.source_cost_message raw type_id program children =
      Some summary ->
    Target.sem_definitely_refused
      (Target.reduce_semantic_field raw
        (map project_source_summary children)) = false /\
    Target.sem_score_evidence
      (Target.reduce_semantic_field raw
        (map project_source_summary children)) =
      project_source_evidence
        (Core.source_summary_exact_score summary) /\
    Target.sem_complete
      (Target.reduce_semantic_field raw
        (map project_source_summary children)) =
      Core.source_family_complete
        (Core.source_summary_analysis summary).
Proof.
  intros raw type_id program children summary selected correspond accepted.
  pose proof (cost_program_projects_to_target raw type_id program
    selected) as [resolved [payload [program_id family]]].
  unfold Cost.source_cost_message in accepted.
  destruct (Admission.source_raw_bytes_well_formed raw &&
      Admission.source_scan_valid
        (Admission.source_scan_children program children) &&
      Admission.source_required_pathmaps_present program
        (Admission.source_scan_children program children) &&
      (Schema.program_family program =? type_id)) eqn:guard;
    [|discriminate].
  destruct (Cost.cost_family_score type_id children) as [score|]
    eqn:scored; [|discriminate].
  inversion accepted; subst summary.
  repeat rewrite andb_true_iff in guard.
  assert (body_ok : Admission.source_raw_bytes_well_formed raw = true)
    by tauto.
  assert (source_valid : Admission.source_scan_valid
    (Admission.source_scan_children program children) = true) by tauto.
  assert (paths_present : Admission.source_required_pathmaps_present
    program (Admission.source_scan_children program children) = true)
    by tauto.
  assert (program_family : Schema.program_family program = type_id).
  { assert (same : Schema.program_family program =? type_id = true)
      by tauto.
    now apply Nat.eqb_eq. }
  pose proof (source_scan_children_project_valid program children
    source_valid) as [target_valid [_ seen_same]].
  pose proof (required_pathmaps_project program
    (Admission.source_scan_children program children)
    (fold_left (Target.scan_child program)
      (map project_source_summary children) Schema.empty_scan)
    seen_same paths_present) as target_paths.
  pose proof (projected_cost_family_score type_id children score
    family correspond scored) as target_score.
  unfold Target.reduce_semantic_field.
  rewrite resolved.
  rewrite program_family.
  rewrite (cost_family_completion_projects type_id
    (fold_left (Target.scan_child program)
      (map project_source_summary children) Schema.empty_scan)
    (map project_source_summary children) family).
  rewrite payload.
  change (Schema.valid_raw_body raw = true) in body_ok.
  rewrite body_ok, target_valid, target_paths.
  destruct family as [family|[family|[family|[family|[family|[]]]]]];
    subst type_id;
    cbn [Target.order_definitely_bad
      Target.complete_receive_has_default_condition
      Target.complete_match_has_default_guard
      Target.score_evidence_for] in *;
    repeat split; try reflexivity;
    exact target_score.
Qed.

Print Assumptions projected_source_fields.
Print Assumptions projected_selected_scores.
Print Assumptions projected_find_tag.
Print Assumptions projected_i32_or_zero.
Print Assumptions projected_u32_or_zero.
Print Assumptions projected_u64_or_zero.
Print Assumptions projected_optional_score.
Print Assumptions projected_cost_atom_score.
Print Assumptions projected_cost_bound_score.
Print Assumptions projected_cost_region_score.
Print Assumptions projected_cost_family_score.
Print Assumptions source_descriptor_guard_projects.
Print Assumptions source_scan_step_projects.
Print Assumptions source_scan_invalid_stays_invalid.
Print Assumptions source_scan_children_project_valid.
Print Assumptions required_pathmaps_project.
Print Assumptions cost_program_projects_to_target.
Print Assumptions cost_family_completion_projects.
Print Assumptions accepted_cost_message_projects_exactly.
