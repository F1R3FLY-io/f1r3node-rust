From Stdlib Require Import List Bool Arith NArith ZArith Lia.
From StackSafePDA Require Import StackSafePDA
  RPB3DescriptorProjectionData RPB3GeneratedFieldRefinement
  RPB3SourceScalarReduction RPB3SemanticReducer
  RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission RPB3IndependentOracleCost
  RPB3IndependentOracleProjection
  RPB3IndependentOracleProcessScalarDecoderBridge
  RPB3IndependentOracleCostScalarDecoderBridge
  RPB3IndependentOracleCostChildCorrespondence.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Target := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Cost := RPB3IndependentOracleCost.
Module Projection := RPB3IndependentOracleProjection.
Module CostDecoder := RPB3IndependentOracleCostScalarDecoderBridge.
Module CostChildren := RPB3IndependentOracleCostChildCorrespondence.

(** These finite controls fix the verdict boundary before the structural
    argument. A Cost source [None] can mean target refusal or unknown score;
    neither target signal alone characterizes source rejection. *)
Definition zero_bound_field : Schema.raw_field :=
  Schema.RawField 0 Schema.LengthDelimited Schema.Message [8; 0] 2
    (Some 11) [] [].

Definition zero_bound_scalar : Schema.raw_field :=
  Schema.RawField 1 Schema.Varint Schema.Scalar [0] 1 None [] [].

Definition zero_bound_tree : @tree Schema.raw_field :=
  Node zero_bound_field (FCons (Node zero_bound_scalar FNil) FNil).

Theorem explicit_zero_cost_bound_is_unknown_without_refusal :
  Cost.source_cost_root zero_bound_tree = None /\
  Target.sem_definitely_refused
    (Target.semantic_reduce zero_bound_tree) = false /\
  Target.sem_score_evidence
    (Target.semantic_reduce zero_bound_tree) = Target.ScoreNotDerived.
Proof. vm_compute. repeat split; reflexivity. Qed.

Definition malformed_atom_field : Schema.raw_field :=
  Schema.RawField 0 Schema.LengthDelimited Schema.Message [10; 1; 97] 3
    (Some 10) [] [].

Definition malformed_atom_scalar : Schema.raw_field :=
  Schema.RawField 1 Schema.LengthDelimited Schema.Scalar [97] 2 None [] [].

Definition malformed_atom_tree : @tree Schema.raw_field :=
  Node malformed_atom_field
    (FCons (Node malformed_atom_scalar FNil) FNil).

Theorem malformed_cost_atom_may_keep_an_exact_score_while_refused :
  Cost.source_cost_root malformed_atom_tree = None /\
  Target.sem_definitely_refused
    (Target.semantic_reduce malformed_atom_tree) = true /\
  exists score,
    Target.sem_score_evidence
      (Target.semantic_reduce malformed_atom_tree) =
      Target.ExactScore score.
Proof.
  vm_compute. repeat split; try reflexivity.
  eexists. reflexivity.
Qed.

Definition noncost_par_field : Schema.raw_field :=
  Schema.RawField 0 Schema.LengthDelimited Schema.Message [] 0
    (Some 1) [] [].

Definition noncost_par_tree : @tree Schema.raw_field :=
  Node noncost_par_field FNil.

Theorem cost_root_restriction_is_necessary :
  Cost.source_cost_root noncost_par_tree = None /\
  Target.sem_definitely_refused
    (Target.semantic_reduce noncost_par_tree) = false /\
  exists score,
    Target.sem_score_evidence
      (Target.semantic_reduce noncost_par_tree) =
      Target.ExactScore score.
Proof.
  vm_compute. repeat split; try reflexivity.
  eexists. reflexivity.
Qed.

(** Unlike the accepted-score projection, these equations also preserve
    failure. They are used only after actual child-tree provenance has been
    recovered; a synthetic auxiliary CostAtom child is not a parsed child. *)
Lemma selected_cost_scores_project_totally :
  forall tag children,
    Target.exact_selected_scores tag
      (map Projection.project_source_summary children) =
    option_map (map Core.project_score)
      (Cost.selected_source_scores tag children).
Proof.
  intros tag children.
  induction children as [|child rest IH]; [reflexivity|].
  cbn [map Target.exact_selected_scores Cost.selected_source_scores].
  rewrite IH.
  destruct (Cost.selected_source_scores tag rest) as [tail|]
    eqn:tail_score; cbn [option_map]; [|reflexivity].
  change (Target.sem_tag (Projection.project_source_summary child))
    with (Core.source_summary_tag child).
  destruct (Core.source_summary_tag child =? tag);
    [|reflexivity].
  change (Target.sem_score_evidence
      (Projection.project_source_summary child))
    with (Projection.project_source_evidence
      (Core.source_summary_exact_score child)).
  destruct (Core.source_summary_exact_score child);
    reflexivity.
Qed.

(** A successful repeated-message score certifies every selected child.
    This is the recursive-message branch of the reverse induction, and does
    not assume that the source evaluator has already accepted the forest. *)
Lemma exact_selected_scores_certifies_children :
  forall tag children scores,
    Target.exact_selected_scores tag children = Some scores ->
    Forall (fun child => Target.sem_tag child = tag ->
      exists score, Target.sem_score_evidence child =
        Target.ExactScore score) children.
Proof.
  intros tag children.
  induction children as [|child rest IH]; intros scores exact.
  - constructor.
  - cbn [Target.exact_selected_scores] in exact.
    destruct (Target.exact_selected_scores tag rest) as [tail|]
      eqn:rest_exact; [|discriminate].
    constructor.
    + intros tag_same.
      rewrite tag_same, Nat.eqb_refl in exact.
      destruct (Target.sem_score_evidence child) as [score|]
        eqn:child_score; [|discriminate].
      now exists score.
    + destruct (Target.sem_tag child =? tag) eqn:chosen.
      * destruct (Target.sem_score_evidence child) as [score|];
          [|discriminate].
        apply IH with (scores := tail). reflexivity.
      * apply IH with (scores := tail). reflexivity.
Qed.

Definition cost_child_ready (family : nat)
    (child : Target.semantic_summary) : Prop :=
  let tag := Target.sem_tag child in
  match family with
  | 9 | 13 => tag = 1 ->
      exists score, Target.sem_score_evidence child =
        Target.ExactScore score
  | 10 => tag = 3 ->
      exists score, Target.sem_score_evidence child =
        Target.ExactScore score
  | 11 => tag = 1 ->
      exists value,
        RPB3SourceScalarReduction.scalar_semantic_from_source
          (RPB3SourceScalarReduction.ScalarRule tag Data.PBSint32 true)
          (Target.sem_body child) =
        Some (RPB3SourceScalarReduction.ScalarI32 value)
  | 12 =>
      ((tag = 1 \/ tag = 7) -> exists value,
        RPB3SourceScalarReduction.scalar_semantic_from_source
          (RPB3SourceScalarReduction.ScalarRule tag Data.PBUint32 true)
          (Target.sem_body child) =
        Some (RPB3SourceScalarReduction.ScalarU32 value)) /\
      ((tag = 2 \/ tag = 3 \/ tag = 4 \/ tag = 5) -> exists value,
        RPB3SourceScalarReduction.scalar_semantic_from_source
          (RPB3SourceScalarReduction.ScalarRule tag Data.PBUint64 true)
          (Target.sem_body child) =
        Some (RPB3SourceScalarReduction.ScalarU64 value)) /\
      (tag = 6 -> exists score,
        Target.sem_score_evidence child = Target.ExactScore score)
  | _ => True
  end.

Lemma repeated_cost_score_certifies_children :
  forall family children score,
    (family = 9 \/ family = 13) ->
    Target.score_evidence_for family
      (if family =? 13 then Some true else None) children =
      Target.ExactScore score ->
    Forall (cost_child_ready family) children.
Proof.
  intros family children score [family_id|family_id] exact.
  - subst family.
    unfold Target.score_evidence_for, Target.exact_or_unknown,
      Target.exact_cost_collection_score in exact.
    cbn in exact.
    destruct (Target.exact_selected_scores 1 children)
      as [selected|] eqn:selected_exact; cbn in exact; [|discriminate].
    apply exact_selected_scores_certifies_children in selected_exact.
    eapply Forall_impl; [|exact selected_exact].
    intros child selected_child. exact selected_child.
  - subst family.
    unfold Target.score_evidence_for, Target.exact_or_unknown,
      Target.exact_cost_collection_score in exact.
    cbn in exact.
    destruct (Target.exact_selected_scores 1 children)
      as [selected|] eqn:selected_exact; cbn in exact; [|discriminate].
    apply exact_selected_scores_certifies_children in selected_exact.
    eapply Forall_impl; [|exact selected_exact].
    intros child selected_child. exact selected_child.
Qed.

(** A parent scan can be valid only if every child was resolved by the
    generated descriptor and its already-reduced target verdict was clear.
    This fact is needed before the per-family score certificate can be used
    to reparse a child without assuming source/target forest alignment. *)
Lemma target_scan_invalid_stays_invalid :
  forall program children state,
    Schema.scan_valid state = false ->
    Schema.scan_valid
      (fold_left (Target.scan_child program) children state) = false.
Proof.
  intros program children.
  induction children as [|child rest IH]; intros state invalid;
    cbn [fold_left].
  - exact invalid.
  - unfold Target.scan_child.
    rewrite invalid.
    apply IH. exact invalid.
Qed.

Lemma valid_target_scan_matches_each_child :
  forall program state children,
    Schema.scan_valid
      (fold_left (Target.scan_child program) children state) = true ->
    Forall (fun child => exists descriptor,
      Schema.resolve_tag (Schema.program_fields program)
        (Target.sem_tag child) = Some descriptor /\
      Target.sem_definitely_refused child = false /\
      Schema.wire_eqb (Target.sem_wire child)
        (Schema.descriptor_wire descriptor) = true /\
      Schema.payload_eqb (Target.sem_payload child)
        (Schema.descriptor_payload descriptor) = true /\
      match Schema.descriptor_payload descriptor with
      | Schema.Message =>
          match Schema.descriptor_child_type_id descriptor with
          | Some type_id => Target.sem_type_id child =? type_id
          | None => false
          end = true
      | _ => True
      end) children.
Proof.
  intros program state children.
  revert state.
  induction children as [|child rest IH]; intros state final.
  - constructor.
  - cbn [fold_left] in final.
    assert (step_valid : Schema.scan_valid
      (Target.scan_child program state child) = true).
    { destruct (Schema.scan_valid (Target.scan_child program state child))
        eqn:step; [reflexivity|].
      pose proof (target_scan_invalid_stays_invalid program rest
        (Target.scan_child program state child) step) as invalid.
      rewrite invalid in final. discriminate. }
    assert (state_valid : Schema.scan_valid state = true).
    { destruct (Schema.scan_valid state) eqn:state_valid;
        [reflexivity|].
      unfold Target.scan_child in step_valid.
      rewrite state_valid in step_valid.
      cbn in step_valid.
      rewrite state_valid in step_valid. discriminate. }
    pose proof step_valid as checked.
    unfold Target.scan_child in checked.
    rewrite state_valid in checked.
    destruct (Schema.resolve_tag (Schema.program_fields program)
      (Target.sem_tag child)) as [descriptor|] eqn:resolved;
      [|discriminate].
    destruct (negb (Target.sem_definitely_refused child) &&
        Schema.wire_eqb (Target.sem_wire child)
          (Schema.descriptor_wire descriptor) &&
        Schema.payload_eqb (Target.sem_payload child)
          (Schema.descriptor_payload descriptor) &&
        (match Schema.descriptor_payload descriptor with
         | Schema.Message =>
             match Schema.descriptor_child_type_id descriptor with
             | Some type_id => Target.sem_type_id child =? type_id
             | None => false
             end
         | _ => true
         end) &&
        (Schema.descriptor_slot descriptor <?
          Schema.program_slot_count program) &&
        Schema.slot_follows (Schema.scan_last_slot state)
          (Schema.descriptor_slot descriptor)
          (Schema.descriptor_repeated descriptor) &&
        (Schema.descriptor_repeated descriptor ||
          negb (existsb (Nat.eqb (Schema.descriptor_slot descriptor))
            (Schema.scan_seen state)))) eqn:guard;
      [|discriminate].
    repeat rewrite andb_true_iff in guard.
    constructor.
    + exists descriptor.
      repeat split; try exact resolved.
      * apply negb_true_iff. tauto.
      * tauto.
      * tauto.
      * destruct (Schema.descriptor_payload descriptor);
          cbn in *; tauto.
    + apply IH with (state := Target.scan_child program state child).
      exact final.
Qed.

Lemma unrejected_target_scalar_has_source_shape :
  forall raw subjects,
    Schema.raw_payload raw = Schema.Scalar ->
    Target.sem_definitely_refused
      (Target.semantic_reduce (Node raw subjects)) = false ->
    subjects = FNil /\
    Schema.raw_program_type_id raw = None /\
    Admission.source_raw_bytes_well_formed raw = true.
Proof.
  intros raw subjects payload accepted.
  destruct subjects as [|subject rest].
  - destruct (Schema.raw_program_type_id raw) as [type_id|]
      eqn:no_type.
    + unfold Target.semantic_reduce, Target.reduce_semantic_field,
        Target.generated_wire_program in accepted.
      rewrite payload, no_type in accepted.
      cbn in accepted.
      discriminate.
    + unfold Target.semantic_reduce, Target.reduce_semantic_field,
        Target.generated_wire_program in accepted.
      rewrite payload, no_type in accepted.
      cbn in accepted.
      destruct (Schema.valid_raw_body raw) eqn:body;
        [|discriminate].
      repeat split; try reflexivity.
      change (Admission.source_raw_bytes_well_formed raw = true).
      exact body.
  - unfold Target.semantic_reduce, Target.reduce_semantic_field in accepted.
    rewrite payload in accepted.
    cbn [Target.sem_definitely_refused Target.semantic_reduce_forest]
      in accepted.
    destruct (Target.generated_wire_program raw);
      cbn [orb negb] in accepted;
      try rewrite orb_true_r in accepted;
      cbn [orb] in accepted;
      discriminate.
Qed.

Lemma unique_target_tag_is_the_score_selector :
  forall children child,
    NoDup (map Target.sem_tag children) ->
    In child children ->
    find (fun candidate =>
      Target.sem_tag candidate =? Target.sem_tag child) children =
      Some child.
Proof.
  intros children.
  induction children as [|head rest IH]; intros child unique present.
  - contradiction.
  - inversion unique as [|? ? head_absent rest_unique]; subst.
    destruct present as [same|later].
    + subst child. cbn [find]. now rewrite Nat.eqb_refl.
    + cbn [find].
      assert (different : Target.sem_tag head <>
        Target.sem_tag child).
      { intro equal.
        apply head_absent.
        rewrite in_map_iff.
        exists child. split; [symmetry; exact equal|exact later]. }
      apply Nat.eqb_neq in different.
      rewrite different.
      apply IH; assumption.
Qed.

Lemma target_scan_valid_prefix :
  forall program state child rest,
    Schema.scan_valid
      (fold_left (Target.scan_child program) (child :: rest) state) = true ->
    Schema.scan_valid (Target.scan_child program state child) = true.
Proof.
  intros program state child rest final.
  cbn [fold_left] in final.
  destruct (Schema.scan_valid (Target.scan_child program state child))
    eqn:prefix; [reflexivity|].
  pose proof (target_scan_invalid_stays_invalid program rest
    (Target.scan_child program state child) prefix) as invalid.
  rewrite invalid in final. discriminate.
Qed.

Lemma valid_target_scan_step_keeps_seen :
  forall program state child slot,
    Schema.scan_valid (Target.scan_child program state child) = true ->
    In slot (Schema.scan_seen state) ->
    In slot (Schema.scan_seen
      (Target.scan_child program state child)).
Proof.
  intros program state child slot valid present.
  unfold Target.scan_child in valid |- *.
  destruct (Schema.scan_valid state) eqn:state_valid;
    [|rewrite state_valid in valid; discriminate].
  destruct (Schema.resolve_tag (Schema.program_fields program)
      (Target.sem_tag child)) as [descriptor|] eqn:resolved;
    [|discriminate].
  destruct (negb (Target.sem_definitely_refused child) &&
      Schema.wire_eqb (Target.sem_wire child)
        (Schema.descriptor_wire descriptor) &&
      Schema.payload_eqb (Target.sem_payload child)
        (Schema.descriptor_payload descriptor) &&
      (match Schema.descriptor_payload descriptor with
       | Schema.Message =>
           match Schema.descriptor_child_type_id descriptor with
           | Some type_id => Target.sem_type_id child =? type_id
           | None => false
           end
       | _ => true
       end) &&
      (Schema.descriptor_slot descriptor <?
        Schema.program_slot_count program) &&
      Schema.slot_follows (Schema.scan_last_slot state)
        (Schema.descriptor_slot descriptor)
        (Schema.descriptor_repeated descriptor) &&
      (Schema.descriptor_repeated descriptor ||
        negb (existsb (Nat.eqb (Schema.descriptor_slot descriptor))
          (Schema.scan_seen state)))) eqn:guard;
    [|discriminate].
  cbn [Schema.scan_seen]. right. exact present.
Qed.

Lemma valid_target_scan_forbids_a_seen_singular_tag :
  forall program children state tag descriptor,
    Schema.resolve_tag (Schema.program_fields program) tag =
      Some descriptor ->
    Schema.descriptor_repeated descriptor = false ->
    In (Schema.descriptor_slot descriptor) (Schema.scan_seen state) ->
    Schema.scan_valid
      (fold_left (Target.scan_child program) children state) = true ->
    Forall (fun child => Target.sem_tag child <> tag) children.
Proof.
  intros program children.
  induction children as [|child rest IH];
    intros state tag descriptor resolved singular present final.
  - constructor.
  - pose proof (target_scan_valid_prefix program state child rest final)
      as step_valid.
    constructor.
    + intro same_tag.
      unfold Target.scan_child in step_valid.
      rewrite same_tag, resolved in step_valid.
      assert (already_seen : existsb
        (Nat.eqb (Schema.descriptor_slot descriptor))
        (Schema.scan_seen state) = true).
      { apply existsb_exists.
        exists (Schema.descriptor_slot descriptor).
        split; [exact present|apply Nat.eqb_refl]. }
      rewrite singular, already_seen in step_valid.
      cbn [orb negb] in step_valid.
      rewrite andb_false_r in step_valid.
      cbn [Schema.scan_valid] in step_valid.
      destruct (Schema.scan_valid state) eqn:state_valid;
        cbn [Schema.scan_valid] in step_valid;
        try rewrite state_valid in step_valid; discriminate.
    + apply IH with (state := Target.scan_child program state child)
        (tag := tag) (descriptor := descriptor).
      * exact resolved.
      * exact singular.
      * eapply valid_target_scan_step_keeps_seen; eauto.
      * exact final.
Qed.

Lemma resolved_descriptor_belongs_to_program :
  forall fields tag descriptor,
    Schema.resolve_tag fields tag = Some descriptor ->
    In descriptor fields.
Proof.
  intros fields.
  induction fields as [|head rest IH]; intros tag descriptor resolved;
    cbn [Schema.resolve_tag] in resolved; [discriminate|].
  destruct (Schema.descriptor_tag head =? tag) eqn:chosen.
  - inversion resolved; subst descriptor. now left.
  - right. eapply IH. exact resolved.
Qed.

Lemma target_nonrepeated_scan_has_distinct_tags :
  forall program state children,
    Forall (fun descriptor =>
      Schema.descriptor_repeated descriptor = false)
      (Schema.program_fields program) ->
    Schema.scan_valid
      (fold_left (Target.scan_child program) children state) = true ->
    NoDup (map Target.sem_tag children).
Proof.
  intros program state children singular_descriptors.
  revert state.
  induction children as [|child rest IH]; intros state final.
  - constructor.
  - pose proof (target_scan_valid_prefix program state child rest final)
      as step_valid.
    assert (state_valid : Schema.scan_valid state = true).
    { destruct (Schema.scan_valid state) eqn:valid;
        [reflexivity|].
      unfold Target.scan_child in step_valid.
      rewrite valid in step_valid.
      cbn in step_valid.
      rewrite valid in step_valid. discriminate. }
    destruct (Schema.resolve_tag (Schema.program_fields program)
      (Target.sem_tag child)) as [descriptor|] eqn:resolved.
    2: { unfold Target.scan_child in step_valid.
         rewrite state_valid, resolved in step_valid.
         discriminate. }
    assert (singular : Schema.descriptor_repeated descriptor = false).
    { apply (proj1 (Forall_forall _ _) singular_descriptors).
      eapply resolved_descriptor_belongs_to_program.
      exact resolved. }
    assert (recorded : In (Schema.descriptor_slot descriptor)
      (Schema.scan_seen (Target.scan_child program state child))).
    { unfold Target.scan_child in step_valid |- *.
      rewrite state_valid, resolved in step_valid |- *.
      destruct (negb (Target.sem_definitely_refused child) &&
          Schema.wire_eqb (Target.sem_wire child)
            (Schema.descriptor_wire descriptor) &&
          Schema.payload_eqb (Target.sem_payload child)
            (Schema.descriptor_payload descriptor) &&
          (match Schema.descriptor_payload descriptor with
           | Schema.Message =>
               match Schema.descriptor_child_type_id descriptor with
               | Some type_id => Target.sem_type_id child =? type_id
               | None => false
               end
           | _ => true
           end) &&
          (Schema.descriptor_slot descriptor <?
            Schema.program_slot_count program) &&
          Schema.slot_follows (Schema.scan_last_slot state)
            (Schema.descriptor_slot descriptor)
            (Schema.descriptor_repeated descriptor) &&
          (Schema.descriptor_repeated descriptor ||
            negb (existsb (Nat.eqb (Schema.descriptor_slot descriptor))
              (Schema.scan_seen state)))) eqn:guard;
        [|discriminate].
      cbn [Schema.scan_seen]. now left. }
    pose proof (valid_target_scan_forbids_a_seen_singular_tag
      program rest (Target.scan_child program state child)
      (Target.sem_tag child) descriptor resolved singular recorded final)
      as later_different.
    constructor.
    + intro present_tag.
      apply in_map_iff in present_tag as [later [same_tag member]].
      pose proof (proj1 (Forall_forall _ _) later_different
        later member) as different.
      apply different. exact same_tag.
    + apply IH with (state := Target.scan_child program state child).
      exact final.
Qed.

Lemma generated_cost_region_descriptors_are_singular :
  forall program,
    Schema.lookup_projected_program 12 [] [] = Some program ->
    Forall (fun descriptor =>
      Schema.descriptor_repeated descriptor = false)
      (Schema.program_fields program).
Proof.
  intros program found.
  vm_compute in found.
  inversion found; subst program.
  vm_compute. repeat constructor.
Qed.

Lemma generated_cost_bound_descriptors_are_singular :
  forall program,
    Schema.lookup_projected_program 11 [] [] = Some program ->
    Forall (fun descriptor =>
      Schema.descriptor_repeated descriptor = false)
      (Schema.program_fields program).
Proof.
  intros program found.
  vm_compute in found.
  inversion found; subst program.
  vm_compute. repeat constructor.
Qed.

Lemma valid_cost_region_scan_has_distinct_tags :
  forall program children,
    Schema.lookup_projected_program 12 [] [] = Some program ->
    Schema.scan_valid
      (fold_left (Target.scan_child program) children
        Schema.empty_scan) = true ->
    NoDup (map Target.sem_tag children).
Proof.
  intros program children found valid.
  eapply target_nonrepeated_scan_has_distinct_tags.
  - exact (generated_cost_region_descriptors_are_singular program found).
  - exact valid.
Qed.

Lemma exact_u32_selector_certifies_present_child :
  forall tag children child value,
    NoDup (map Target.sem_tag children) ->
    In child children ->
    Target.sem_tag child = tag ->
    Target.exact_u32_or_absent tag children = Some value ->
    exists decoded,
      RPB3SourceScalarReduction.scalar_semantic_from_source
        (RPB3SourceScalarReduction.ScalarRule tag Data.PBUint32 true)
        (Target.sem_body child) =
      Some (RPB3SourceScalarReduction.ScalarU32 decoded).
Proof.
  intros tag children child value distinct present child_tag selected.
  subst tag.
  unfold Target.exact_u32_or_absent in selected.
  rewrite (unique_target_tag_is_the_score_selector
    children child distinct present) in selected.
  destruct (RPB3SourceScalarReduction.scalar_semantic_from_source
      (RPB3SourceScalarReduction.ScalarRule
        (Target.sem_tag child) Data.PBUint32 true)
      (Target.sem_body child)) as [decoded|] eqn:parsed;
    [|discriminate].
  destruct decoded; try discriminate.
  eexists. reflexivity.
Qed.

Lemma exact_u64_selector_certifies_present_child :
  forall tag children child value,
    NoDup (map Target.sem_tag children) ->
    In child children ->
    Target.sem_tag child = tag ->
    Target.exact_u64_or_absent tag children = Some value ->
    exists decoded,
      RPB3SourceScalarReduction.scalar_semantic_from_source
        (RPB3SourceScalarReduction.ScalarRule tag Data.PBUint64 true)
        (Target.sem_body child) =
      Some (RPB3SourceScalarReduction.ScalarU64 decoded).
Proof.
  intros tag children child value distinct present child_tag selected.
  subst tag.
  unfold Target.exact_u64_or_absent in selected.
  rewrite (unique_target_tag_is_the_score_selector
    children child distinct present) in selected.
  destruct (RPB3SourceScalarReduction.scalar_semantic_from_source
      (RPB3SourceScalarReduction.ScalarRule
        (Target.sem_tag child) Data.PBUint64 true)
      (Target.sem_body child)) as [decoded|] eqn:parsed;
    [|discriminate].
  destruct decoded; try discriminate.
  eexists. reflexivity.
Qed.

Lemma exact_i32_selector_certifies_present_child :
  forall tag children child value,
    NoDup (map Target.sem_tag children) ->
    In child children ->
    Target.sem_tag child = tag ->
    Target.exact_i32_or_absent tag children = Some value ->
    exists decoded,
      RPB3SourceScalarReduction.scalar_semantic_from_source
        (RPB3SourceScalarReduction.ScalarRule tag Data.PBSint32 true)
        (Target.sem_body child) =
      Some (RPB3SourceScalarReduction.ScalarI32 decoded).
Proof.
  intros tag children child value distinct present child_tag selected.
  subst tag.
  unfold Target.exact_i32_or_absent in selected.
  rewrite (unique_target_tag_is_the_score_selector
    children child distinct present) in selected.
  destruct (RPB3SourceScalarReduction.scalar_semantic_from_source
      (RPB3SourceScalarReduction.ScalarRule
        (Target.sem_tag child) Data.PBSint32 true)
      (Target.sem_body child)) as [decoded|] eqn:parsed;
    [|discriminate].
  destruct decoded; try discriminate.
  eexists. reflexivity.
Qed.

Lemma exact_optional_selector_certifies_present_child :
  forall tag children child selected,
    NoDup (map Target.sem_tag children) ->
    In child children ->
    Target.sem_tag child = tag ->
    Target.exact_optional_child_score tag children = Some selected ->
    exists score,
      Target.sem_score_evidence child = Target.ExactScore score.
Proof.
  intros tag children child selected distinct present child_tag exact.
  subst tag.
  unfold Target.exact_optional_child_score in exact.
  rewrite (unique_target_tag_is_the_score_selector
    children child distinct present) in exact.
  destruct (Target.sem_score_evidence child) as [score|]
    eqn:child_score; [|discriminate].
  now exists score.
Qed.

Lemma exact_region_score_certifies_children :
  forall children score,
    NoDup (map Target.sem_tag children) ->
    Target.score_evidence_for 12 None children =
      Target.ExactScore score ->
    Forall (cost_child_ready 12) children.
Proof.
  intros children score distinct exact.
  cbn [Target.score_evidence_for Target.exact_or_unknown] in exact.
  destruct (Target.exact_cost_region_score children) as [scored|]
    eqn:region_score; [|discriminate].
  unfold Target.exact_cost_region_score in region_score.
  destruct (Target.exact_u32_or_absent 1 children) as [version|]
    eqn:version_score; [|discriminate].
  destruct (Target.exact_u64_or_absent 2 children) as [start_line|]
    eqn:start_line_score; [|discriminate].
  destruct (Target.exact_u64_or_absent 3 children) as [start_column|]
    eqn:start_column_score; [|discriminate].
  destruct (Target.exact_u64_or_absent 4 children) as [end_line|]
    eqn:end_line_score; [|discriminate].
  destruct (Target.exact_u64_or_absent 5 children) as [end_column|]
    eqn:end_column_score; [|discriminate].
  destruct (Target.exact_optional_child_score 6 children) as [signer|]
    eqn:signer_score; [|discriminate].
  destruct (Target.exact_u32_or_absent 7 children) as [ordinal|]
    eqn:ordinal_score; [|discriminate].
  apply Forall_forall. intros child present.
  unfold cost_child_ready.
  repeat split.
  - intros [version_tag|ordinal_tag].
    + rewrite version_tag.
      eapply exact_u32_selector_certifies_present_child;
        eauto using version_score.
    + rewrite ordinal_tag.
      eapply exact_u32_selector_certifies_present_child;
        eauto using ordinal_score.
  - intros [start_line_tag|[start_column_tag|
      [end_line_tag|end_column_tag]]].
    + rewrite start_line_tag.
      eapply exact_u64_selector_certifies_present_child;
        eauto using start_line_score.
    + rewrite start_column_tag.
      eapply exact_u64_selector_certifies_present_child;
        eauto using start_column_score.
    + rewrite end_line_tag.
      eapply exact_u64_selector_certifies_present_child;
        eauto using end_line_score.
    + rewrite end_column_tag.
      eapply exact_u64_selector_certifies_present_child;
        eauto using end_column_score.
  - intro signer_tag.
    eapply exact_optional_selector_certifies_present_child;
      eauto using signer_score.
Qed.

Lemma exact_bound_score_certifies_children :
  forall children score,
    NoDup (map Target.sem_tag children) ->
    Target.score_evidence_for 11 None children =
      Target.ExactScore score ->
    Forall (cost_child_ready 11) children.
Proof.
  intros children score distinct exact.
  cbn [Target.score_evidence_for Target.exact_or_unknown] in exact.
  destruct (Target.exact_cost_bound_score children) as [scored|]
    eqn:bound_score; [|discriminate].
  unfold Target.exact_cost_bound_score in bound_score.
  destruct (Target.exact_i32_or_absent 1 children) as [index|]
    eqn:index_score; [|discriminate].
  apply Forall_forall. intros child present.
  unfold cost_child_ready.
  intro index_tag.
  rewrite index_tag.
  eapply exact_i32_selector_certifies_present_child;
    eauto using index_score.
Qed.

Lemma exact_atom_score_certifies_children :
  forall children score,
    Target.score_evidence_for 10 None children =
      Target.ExactScore score ->
    Forall (cost_child_ready 10) children.
Proof.
  intros children score exact.
  cbn [Target.score_evidence_for Target.exact_or_unknown] in exact.
  destruct children as [|child [|second rest]].
  - constructor.
  - constructor; [|constructor].
    unfold cost_child_ready. intro bound_tag.
    cbn [Target.exact_cost_atom_score] in exact.
    rewrite bound_tag in exact.
    destruct (Target.sem_payload child); try discriminate.
    destruct (Target.sem_score_evidence child) as [child_score|]
      eqn:child_exact; [|discriminate].
    now exists child_score.
  - cbn [Target.exact_cost_atom_score] in exact. discriminate.
Qed.

Lemma exact_generated_cost_score_certifies_children :
  forall raw family program children score,
    Cost.cost_program raw = Some (family, program) ->
    Schema.scan_valid
      (fold_left (Target.scan_child program) children
        Schema.empty_scan) = true ->
    Target.score_evidence_for family
      (if family =? 13 then Some true else None) children =
      Target.ExactScore score ->
    Forall (cost_child_ready family) children.
Proof.
  intros raw family program children score selected valid exact.
  destruct (CostChildren.selected_cost_program_is_generated
    raw family program selected) as [member found].
  destruct member as
    [member|[member|[member|[member|[member|[]]]]]];
    subst family.
  - eapply repeated_cost_score_certifies_children;
      [left; reflexivity|exact exact].
  - exact (exact_atom_score_certifies_children children score exact).
  - eapply exact_bound_score_certifies_children; [|exact exact].
    eapply target_nonrepeated_scan_has_distinct_tags;
      [exact (generated_cost_bound_descriptors_are_singular
        program found)|exact valid].
  - eapply exact_region_score_certifies_children; [|exact exact].
    exact (valid_cost_region_scan_has_distinct_tags
      program children found valid).
  - eapply repeated_cost_score_certifies_children;
      [right; reflexivity|exact exact].
Qed.

Definition reparsed_cost_child_guard
    (child : Core.source_exact_summary) : Prop :=
  Admission.source_raw_bytes_well_formed
    (Core.source_summary_raw child) = true /\
  Core.source_summary_refused child = false /\
  (Schema.raw_payload (Core.source_summary_raw child) = Schema.Message ->
    exists type_id,
      Schema.raw_program_type_id (Core.source_summary_raw child) =
        Some type_id).

Lemma accepted_cost_child_has_reparse_guard :
  forall parent subject child,
    Cost.source_cost_tree parent subject = Some child ->
    reparsed_cost_child_guard child.
Proof.
  intros parent [raw subjects] child accepted.
  cbn [Cost.source_cost_tree] in accepted.
  destruct (Schema.raw_payload raw) eqn:payload;
    try discriminate.
  - destruct parent as [program|]; [|discriminate].
    destruct subjects as [|subject rest]; [|discriminate].
    unfold Cost.source_scalar_leaf,
      Admission.source_typed_scalar,
      Admission.source_scalar_value_of_raw in accepted.
    rewrite payload in accepted.
    destruct (Schema.raw_program_type_id raw) as [type_id|];
      [discriminate|].
    destruct (Schema.resolve_tag (Schema.program_fields program)
      (Schema.raw_tag raw)) as [descriptor|]; [|discriminate].
    destruct (Admission.source_scalar_contract_for
      (Schema.program_family program) (Schema.raw_tag raw))
      as [contract|]; [|discriminate].
    destruct (Admission.source_scalar_wire
      (Admission.source_contract_kind contract)) as [wire|];
      [|discriminate].
    destruct (Admission.source_raw_bytes_well_formed raw &&
        Schema.wire_eqb (Schema.raw_wire raw) wire &&
        Schema.wire_eqb wire (Schema.descriptor_wire descriptor) &&
        Schema.payload_eqb (Schema.descriptor_payload descriptor)
          Schema.Scalar) eqn:guard; [|discriminate].
    destruct (Core.independently_decode_scalar
      (Admission.source_contract_kind contract)
      (Admission.source_contract_default_forbidden contract)
      (Schema.raw_body raw)) as [scalar|]; [|discriminate].
    cbn [option_map] in accepted.
    inversion accepted; subst child.
    repeat rewrite andb_true_iff in guard.
    unfold reparsed_cost_child_guard.
    cbn [Core.source_summary_raw Core.source_value_raw
      Core.source_summary_refused Core.source_summary_analysis
      Core.source_family_refused].
    split; [tauto|].
    split; [reflexivity|].
    intros impossible.
    cbn [Core.source_summary_raw Core.source_value_raw
      Core.source_summary_value] in impossible.
    rewrite payload in impossible. discriminate.
  - destruct (Cost.cost_program raw) as [[family program]|]
      eqn:selected; [|discriminate].
    destruct (Cost.source_cost_forest program subjects)
      as [children|]; [|discriminate].
    unfold Cost.source_cost_message in accepted.
    destruct (Admission.source_raw_bytes_well_formed raw &&
        Admission.source_scan_valid
          (Admission.source_scan_children program children) &&
        Admission.source_required_pathmaps_present program
          (Admission.source_scan_children program children) &&
        (Schema.program_family program =? family)) eqn:guard;
      [|discriminate].
    destruct (Cost.cost_family_score family children) as [score|];
      [|discriminate].
    cbn [option_map] in accepted.
    inversion accepted; subst child.
    repeat rewrite andb_true_iff in guard.
    destruct (Projection.cost_program_projects_to_target
      raw family program selected) as [_ [_ [raw_id _]]].
    unfold reparsed_cost_child_guard.
    cbn [Core.source_summary_raw Core.source_value_raw
      Core.source_summary_refused Core.source_summary_analysis
      Core.source_family_refused].
    split; [tauto|].
    split; [reflexivity|].
    intros _. exists family. exact raw_id.
Qed.

Lemma accepted_cost_forest_children_have_reparse_guards :
  forall program subjects children,
    Cost.source_cost_forest program subjects = Some children ->
    Forall reparsed_cost_child_guard children.
Proof.
  intros program subjects children accepted.
  pose proof (CostChildren.cost_forest_children_have_source_trees
    program subjects children accepted) as origins.
  apply Forall_forall. intros child present.
  destruct (proj1 (Forall_forall _ _) origins child present)
    as [subject evaluated].
  exact (accepted_cost_child_has_reparse_guard
    (Some program) subject child evaluated).
Qed.

Lemma reparsed_cost_child_descriptor_guard_agrees :
  forall descriptor child,
    reparsed_cost_child_guard child ->
    Admission.source_child_matches_descriptor descriptor child =
    (negb (Target.sem_definitely_refused
        (Projection.project_source_summary child)) &&
     Schema.wire_eqb
       (Target.sem_wire (Projection.project_source_summary child))
       (Schema.descriptor_wire descriptor) &&
     Schema.payload_eqb
       (Target.sem_payload (Projection.project_source_summary child))
       (Schema.descriptor_payload descriptor) &&
     match Schema.descriptor_payload descriptor with
     | Schema.Message =>
         match Schema.descriptor_child_type_id descriptor with
         | Some expected =>
             Target.sem_type_id
               (Projection.project_source_summary child) =? expected
         | None => false
         end
     | _ => true
     end).
Proof.
  intros descriptor child [body [clear typed_id]].
  unfold Admission.source_child_matches_descriptor.
  change (Target.sem_definitely_refused
    (Projection.project_source_summary child))
    with (Core.source_summary_refused child).
  change (Target.sem_wire (Projection.project_source_summary child))
    with (Schema.raw_wire (Core.source_summary_raw child)).
  change (Target.sem_payload (Projection.project_source_summary child))
    with (Schema.raw_payload (Core.source_summary_raw child)).
  rewrite body, clear.
  destruct (Schema.descriptor_payload descriptor),
    (Schema.raw_payload (Core.source_summary_raw child));
    cbn [Schema.payload_eqb];
    try (repeat rewrite andb_false_r; cbn [andb]; reflexivity);
    try reflexivity.
  destruct (typed_id eq_refl) as [observed observed_id].
  unfold Core.source_summary_type_id.
  change (Target.sem_type_id
    (Projection.project_source_summary child)) with
    (match Schema.raw_program_type_id (Core.source_summary_raw child) with
     | Some type_id => type_id
     | None => 0
     end).
  rewrite observed_id.
  destruct (Schema.descriptor_child_type_id descriptor) as [expected|];
    [rewrite Nat.eqb_sym|]; reflexivity.
Qed.

Lemma valid_target_scan_step_reconstructs_source :
  forall program source_state target_state child,
    Admission.source_scan_valid source_state = true ->
    Schema.scan_valid target_state = true ->
    Admission.source_scan_last_slot source_state =
      Schema.scan_last_slot target_state ->
    Admission.source_scan_seen_slots source_state =
      Schema.scan_seen target_state ->
    reparsed_cost_child_guard child ->
    Schema.scan_valid
      (Target.scan_child program target_state
        (Projection.project_source_summary child)) = true ->
    Admission.source_scan_valid
      (Admission.source_scan_step program source_state child) = true /\
    Admission.source_scan_last_slot
      (Admission.source_scan_step program source_state child) =
      Schema.scan_last_slot
        (Target.scan_child program target_state
          (Projection.project_source_summary child)) /\
    Admission.source_scan_seen_slots
      (Admission.source_scan_step program source_state child) =
      Schema.scan_seen
        (Target.scan_child program target_state
          (Projection.project_source_summary child)).
Proof.
  intros program source_state target_state child source_valid target_valid
    same_last same_seen child_guard next_valid.
  unfold Admission.source_scan_step, Target.scan_child
    in next_valid |- *.
  rewrite source_valid, target_valid.
  rewrite target_valid in next_valid.
  change (Target.sem_tag (Projection.project_source_summary child))
    with (Core.source_summary_tag child) in next_valid |- *.
  destruct (Schema.resolve_tag (Schema.program_fields program)
    (Core.source_summary_tag child)) as [descriptor|] eqn:resolved;
    [|discriminate].
  rewrite (reparsed_cost_child_descriptor_guard_agrees
    descriptor child child_guard).
  rewrite same_last, same_seen.
  change (Admission.source_slot_follows
    (Schema.scan_last_slot target_state)
    (Schema.descriptor_slot descriptor)
    (Schema.descriptor_repeated descriptor)) with
    (Schema.slot_follows (Schema.scan_last_slot target_state)
      (Schema.descriptor_slot descriptor)
      (Schema.descriptor_repeated descriptor)).
  destruct (negb (Target.sem_definitely_refused
        (Projection.project_source_summary child)) &&
      Schema.wire_eqb
        (Target.sem_wire (Projection.project_source_summary child))
        (Schema.descriptor_wire descriptor) &&
      Schema.payload_eqb
        (Target.sem_payload (Projection.project_source_summary child))
        (Schema.descriptor_payload descriptor) &&
      (match Schema.descriptor_payload descriptor with
       | Schema.Message =>
           match Schema.descriptor_child_type_id descriptor with
           | Some expected =>
               Target.sem_type_id
                 (Projection.project_source_summary child) =? expected
           | None => false
           end
       | _ => true
       end) &&
      (Schema.descriptor_slot descriptor <?
        Schema.program_slot_count program) &&
      Schema.slot_follows (Schema.scan_last_slot target_state)
        (Schema.descriptor_slot descriptor)
        (Schema.descriptor_repeated descriptor) &&
      (Schema.descriptor_repeated descriptor ||
        negb (existsb (Nat.eqb (Schema.descriptor_slot descriptor))
          (Schema.scan_seen target_state)))) eqn:guard_ok;
    [repeat split; reflexivity|discriminate].
Qed.

Lemma valid_target_scan_reconstructs_source :
  forall program children,
    Forall reparsed_cost_child_guard children ->
    Schema.scan_valid
      (fold_left (Target.scan_child program)
        (map Projection.project_source_summary children)
        Schema.empty_scan) = true ->
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = true /\
    Admission.source_scan_last_slot
      (Admission.source_scan_children program children) =
      Schema.scan_last_slot
        (fold_left (Target.scan_child program)
          (map Projection.project_source_summary children)
          Schema.empty_scan) /\
    Admission.source_scan_seen_slots
      (Admission.source_scan_children program children) =
      Schema.scan_seen
        (fold_left (Target.scan_child program)
          (map Projection.project_source_summary children)
          Schema.empty_scan).
Proof.
  intros program children ready valid.
  unfold Admission.source_scan_children.
  assert (general : forall children source_state target_state,
    Forall reparsed_cost_child_guard children ->
    Admission.source_scan_valid source_state = true ->
    Schema.scan_valid target_state = true ->
    Admission.source_scan_last_slot source_state =
      Schema.scan_last_slot target_state ->
    Admission.source_scan_seen_slots source_state =
      Schema.scan_seen target_state ->
    Schema.scan_valid
      (fold_left (Target.scan_child program)
        (map Projection.project_source_summary children)
        target_state) = true ->
    Admission.source_scan_valid
      (fold_left (Admission.source_scan_step program)
        children source_state) = true /\
    Admission.source_scan_last_slot
      (fold_left (Admission.source_scan_step program)
        children source_state) =
      Schema.scan_last_slot
        (fold_left (Target.scan_child program)
          (map Projection.project_source_summary children)
          target_state) /\
    Admission.source_scan_seen_slots
      (fold_left (Admission.source_scan_step program)
        children source_state) =
      Schema.scan_seen
        (fold_left (Target.scan_child program)
          (map Projection.project_source_summary children)
          target_state)).
  { intros remaining.
    induction remaining as [|child rest IH];
      intros source_state target_state ready_children source_valid target_valid
        same_last same_seen final;
      cbn [fold_left map] in *.
    - repeat split; assumption.
    - inversion ready_children as [|? ? child_ready rest_ready]; subst.
      pose proof (target_scan_valid_prefix program target_state
        (Projection.project_source_summary child)
        (map Projection.project_source_summary rest) final)
        as next_valid.
      pose proof (valid_target_scan_step_reconstructs_source
        program source_state target_state child source_valid target_valid
        same_last same_seen child_ready next_valid)
        as [next_source_valid [next_last next_seen]].
      apply IH with
        (source_state := Admission.source_scan_step program
          source_state child)
        (target_state := Target.scan_child program target_state
          (Projection.project_source_summary child));
        assumption. }
  apply general with
    (source_state := Admission.source_empty_scan)
    (target_state := Schema.empty_scan); try assumption; reflexivity.
Qed.

Lemma selected_cost_program_needs_no_pathmaps :
  forall raw family program state,
    Cost.cost_program raw = Some (family, program) ->
    Admission.source_required_pathmaps_present program state = true.
Proof.
  intros raw family program state selected.
  destruct (CostChildren.selected_cost_program_is_generated
    raw family program selected) as [member found].
  destruct member as
    [member|[member|[member|[member|[member|[]]]]]];
    subst family;
    vm_compute in found; inversion found; subst program;
    reflexivity.
Qed.

Lemma generated_cost_scalar_reparses_after_decoder_success :
  forall program raw descriptor kind forbidden value,
    Schema.raw_payload raw = Schema.Scalar ->
    Schema.raw_program_type_id raw = None ->
    Schema.resolve_tag (Schema.program_fields program)
      (Schema.raw_tag raw) = Some descriptor ->
    Admission.source_scalar_contract_for
      (Schema.program_family program) (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract kind forbidden) ->
    Admission.source_scalar_wire kind =
      Some (Schema.raw_wire raw) ->
    Schema.descriptor_wire descriptor = Schema.raw_wire raw ->
    Schema.descriptor_payload descriptor = Schema.Scalar ->
    Admission.source_raw_bytes_well_formed raw = true ->
    Core.independently_decode_scalar kind forbidden
      (Schema.raw_body raw) = Some value ->
    Cost.source_scalar_leaf program raw =
      Some (Core.SourceExactSummary
        (Core.TypedSourceScalar raw value)
        (Core.SourceFamilyAnalysis None None false false false) None).
Proof.
  intros program raw descriptor kind forbidden value payload no_type
    resolved contract kind_wire descriptor_wire descriptor_payload body
    decoded.
  unfold Cost.source_scalar_leaf, Admission.source_typed_scalar,
    Admission.source_scalar_value_of_raw.
  rewrite payload, no_type, resolved, contract.
  cbn [Admission.source_contract_kind
    Admission.source_contract_default_forbidden].
  rewrite kind_wire, body, descriptor_wire, descriptor_payload.
  rewrite decoded.
  destruct (Schema.raw_wire raw); reflexivity.
Qed.

Inductive generated_cost_child_rule : Type :=
| CostBytesRule
| CostI32Rule
| CostU32Rule
| CostU64Rule
| CostMessageRule (child_family : nat).

Definition generated_cost_child_rule_for (family tag : nat)
    : option generated_cost_child_rule :=
  match family, tag with
  | 9, 1 => Some (CostMessageRule 10)
  | 10, 1 | 10, 2 | 10, 4 => Some CostBytesRule
  | 10, 3 => Some (CostMessageRule 11)
  | 11, 1 => Some CostI32Rule
  | 12, 1 | 12, 7 => Some CostU32Rule
  | 12, 2 | 12, 3 | 12, 4 | 12, 5 => Some CostU64Rule
  | 12, 6 => Some (CostMessageRule 9)
  | 13, 1 => Some (CostMessageRule 9)
  | _, _ => None
  end.

Definition generated_cost_child_rule_matches
    (family tag : nat) (descriptor : Schema.field_descriptor)
    (rule : generated_cost_child_rule) : Prop :=
  match rule with
  | CostBytesRule =>
      Schema.descriptor_wire descriptor = Schema.LengthDelimited /\
      Schema.descriptor_payload descriptor = Schema.Scalar /\
      Admission.source_scalar_contract_for family tag =
        Some (Admission.SourceScalarContract Data.PBBytes false)
  | CostI32Rule =>
      Schema.descriptor_wire descriptor = Schema.Varint /\
      Schema.descriptor_payload descriptor = Schema.Scalar /\
      Admission.source_scalar_contract_for family tag =
        Some (Admission.SourceScalarContract Data.PBSint32 true)
  | CostU32Rule =>
      Schema.descriptor_wire descriptor = Schema.Varint /\
      Schema.descriptor_payload descriptor = Schema.Scalar /\
      Admission.source_scalar_contract_for family tag =
        Some (Admission.SourceScalarContract Data.PBUint32 true)
  | CostU64Rule =>
      Schema.descriptor_wire descriptor = Schema.Varint /\
      Schema.descriptor_payload descriptor = Schema.Scalar /\
      Admission.source_scalar_contract_for family tag =
        Some (Admission.SourceScalarContract Data.PBUint64 true)
  | CostMessageRule child_family =>
      Schema.descriptor_wire descriptor = Schema.LengthDelimited /\
      Schema.descriptor_payload descriptor = Schema.Message /\
      Schema.descriptor_child_type_id descriptor = Some child_family /\
      In child_family [9; 10; 11; 12; 13]
  end.

Lemma generated_cost_descriptor_has_exact_child_rule :
  forall family program tag descriptor,
    In family [9; 10; 11; 12; 13] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Schema.resolve_tag (Schema.program_fields program) tag =
      Some descriptor ->
    exists rule,
      generated_cost_child_rule_for family tag = Some rule /\
      generated_cost_child_rule_matches family tag descriptor rule.
Proof.
  intros family program tag descriptor member found resolved.
  destruct member as
    [member|[member|[member|[member|[member|[]]]]]];
    subst family;
    vm_compute in found; inversion found; subst program;
    destruct tag as [|[|[|[|[|[|[|[|extra]]]]]]]];
    vm_compute in resolved; try discriminate;
    inversion resolved; subst descriptor;
    eexists; split; vm_compute; repeat split; auto.
Qed.

Lemma unrejected_target_cost_message_has_selected_program :
  forall raw subjects expected,
    Schema.raw_payload raw = Schema.Message ->
    In expected [9; 10; 11; 12; 13] ->
    Target.sem_definitely_refused
      (Target.semantic_reduce (Node raw subjects)) = false ->
    Target.sem_type_id
      (Target.semantic_reduce (Node raw subjects)) = expected ->
    exists program,
      Cost.cost_program raw = Some (expected, program).
Proof.
  intros raw subjects expected payload member unrejected target_id.
  destruct (Schema.raw_program_type_id raw) as [observed|]
    eqn:raw_id.
  - destruct (Schema.lookup_projected_program observed [] [])
      as [program|] eqn:found.
    + assert (requested : Schema.program_family program = observed).
      { exact (RPB3IndependentOracleProcessScalarDecoderBridge.generated_program_has_requested_family
          observed program found). }
      unfold Target.semantic_reduce, Target.reduce_semantic_field,
        Target.generated_wire_program in target_id.
      rewrite payload, raw_id, found in target_id.
      cbn [Target.sem_type_id] in target_id.
      rewrite requested in target_id.
      subst observed.
      exists program.
      unfold Cost.cost_program.
      rewrite payload, raw_id, found.
      destruct member as
        [member|[member|[member|[member|[member|[]]]]]];
        subst expected; reflexivity.
    + unfold Target.semantic_reduce, Target.reduce_semantic_field,
        Target.generated_wire_program in unrejected.
      rewrite payload, raw_id, found in unrejected.
      cbn [Target.sem_definitely_refused] in unrejected.
      discriminate.
  - unfold Target.semantic_reduce, Target.reduce_semantic_field,
      Target.generated_wire_program in unrejected.
    rewrite payload, raw_id in unrejected.
    cbn [Target.sem_definitely_refused] in unrejected.
    discriminate.
Qed.

Lemma target_wire_match_is_equality :
  forall left right,
    Schema.wire_eqb left right = true -> left = right.
Proof.
  intros left right matched.
  destruct left, right; cbn in matched; try discriminate;
    reflexivity.
Qed.

Lemma target_payload_match_is_equality :
  forall left right,
    Schema.payload_eqb left right = true -> left = right.
Proof.
  intros left right matched.
  destruct left, right; cbn in matched; try discriminate;
    reflexivity.
Qed.

Lemma generated_cost_child_rule_inverse :
  forall family tag rule,
    In family [9; 10; 11; 12; 13] ->
    generated_cost_child_rule_for family tag = Some rule ->
    match rule with
    | CostBytesRule => family = 10 /\ In tag [1; 2; 4]
    | CostI32Rule => family = 11 /\ tag = 1
    | CostU32Rule => family = 12 /\ In tag [1; 7]
    | CostU64Rule => family = 12 /\ In tag [2; 3; 4; 5]
    | CostMessageRule expected =>
        (family = 9 /\ tag = 1 /\ expected = 10) \/
        (family = 10 /\ tag = 3 /\ expected = 11) \/
        (family = 12 /\ tag = 6 /\ expected = 9) \/
        (family = 13 /\ tag = 1 /\ expected = 9)
    end.
Proof.
  intros family tag rule member classified.
  destruct member as
    [member|[member|[member|[member|[member|[]]]]]];
    subst family;
    destruct tag as [|[|[|[|[|[|[|[|extra]]]]]]]];
    vm_compute in classified; try discriminate;
    inversion classified; subst rule;
    cbn; intuition (try reflexivity; auto).
Qed.

Definition generated_cost_scalar_rule_kind
    (rule : generated_cost_child_rule)
    : option (Data.protobuf_kind * bool) :=
  match rule with
  | CostBytesRule => Some (Data.PBBytes, false)
  | CostI32Rule => Some (Data.PBSint32, true)
  | CostU32Rule => Some (Data.PBUint32, true)
  | CostU64Rule => Some (Data.PBUint64, true)
  | CostMessageRule _ => None
  end.

Lemma ready_generated_cost_scalar_has_source_decoder :
  forall family tag rule kind forbidden child,
    In family [9; 10; 11; 12; 13] ->
    generated_cost_child_rule_for family tag = Some rule ->
    generated_cost_scalar_rule_kind rule = Some (kind, forbidden) ->
    Target.sem_tag child = tag ->
    cost_child_ready family child ->
    Core.bytes_in_range (Target.sem_body child) = true ->
    exists value,
      Core.independently_decode_scalar kind forbidden
        (Target.sem_body child) = Some value.
Proof.
  intros family tag rule kind forbidden child member classified kind_rule
    child_tag ready bytes.
  pose proof (generated_cost_child_rule_inverse
    family tag rule member classified) as inverse.
  destruct rule as [| | | |expected];
    cbn [generated_cost_scalar_rule_kind] in kind_rule;
    try discriminate;
    inversion kind_rule; subst kind forbidden.
  - exists (Core.SourceBlob (Target.sem_body child)).
    unfold Core.independently_decode_scalar.
    rewrite bytes. reflexivity.
  - destruct inverse as [family_id tag_id].
    subst family.
    unfold cost_child_ready in ready.
    rewrite child_tag in ready.
    destruct (ready tag_id) as [value parsed].
    exists (Core.SourceI32 value).
    apply (proj2 (CostDecoder.cost_pbsint32_scalar_equivalence
      tag (Target.sem_body child) value)).
    exact parsed.
  - destruct inverse as [family_id tag_member].
    subst family.
    destruct tag_member as [tag_id|[tag_id|[]]];
      unfold cost_child_ready in ready;
      rewrite child_tag in ready;
      destruct ready as [ready_u32 [_ _]];
      [specialize (ready_u32 (or_introl (eq_sym tag_id)))|
       specialize (ready_u32 (or_intror (eq_sym tag_id)))];
      destruct ready_u32 as [value parsed];
      exists (Core.SourceU32 value);
      apply (proj2 (CostDecoder.cost_pbuint32_scalar_equivalence
        tag (Target.sem_body child) value));
      exact parsed.
  - destruct inverse as [family_id tag_member].
    subst family.
    destruct tag_member as
      [tag_id|[tag_id|[tag_id|[tag_id|[]]]]];
      unfold cost_child_ready in ready;
      rewrite child_tag in ready;
      destruct ready as [_ [ready_u64 _]];
      [specialize (ready_u64 (or_introl (eq_sym tag_id)))|
       specialize (ready_u64 (or_intror (or_introl (eq_sym tag_id))))|
       specialize (ready_u64
         (or_intror (or_intror (or_introl (eq_sym tag_id)))))|
       specialize (ready_u64
         (or_intror (or_intror (or_intror (eq_sym tag_id)))))];
      destruct ready_u64 as [value parsed];
      exists (Core.SourceU64 value);
      apply (proj2 (CostDecoder.cost_pbuint64_scalar_equivalence
        tag (Target.sem_body child) value));
      exact parsed.
Qed.

Lemma generated_cost_scalar_rule_supplies_admission_contract :
  forall family tag descriptor rule kind forbidden,
    generated_cost_child_rule_matches family tag descriptor rule ->
    generated_cost_scalar_rule_kind rule = Some (kind, forbidden) ->
    Schema.descriptor_payload descriptor = Schema.Scalar /\
    Admission.source_scalar_contract_for family tag =
      Some (Admission.SourceScalarContract kind forbidden) /\
    Admission.source_scalar_wire kind =
      Some (Schema.descriptor_wire descriptor).
Proof.
  intros family tag descriptor rule kind forbidden matched scalar_kind.
  destruct rule as [| | | |expected];
    cbn [generated_cost_scalar_rule_kind] in scalar_kind;
    try discriminate;
    inversion scalar_kind; subst kind forbidden;
    cbn [generated_cost_child_rule_matches] in matched;
    destruct matched as [wire [payload contract]];
    repeat split; try assumption;
    rewrite wire; reflexivity.
Qed.

Lemma ready_generated_cost_message_has_exact_score :
  forall family tag expected child,
    In family [9; 10; 11; 12; 13] ->
    generated_cost_child_rule_for family tag =
      Some (CostMessageRule expected) ->
    Target.sem_tag child = tag ->
    cost_child_ready family child ->
    exists score,
      Target.sem_score_evidence child = Target.ExactScore score.
Proof.
  intros family tag expected child member classified child_tag ready.
  pose proof (generated_cost_child_rule_inverse family tag
    (CostMessageRule expected) member classified) as inverse.
  destruct inverse as
    [[family_id [tag_id expected_id]]|
     [[family_id [tag_id expected_id]]|
      [[family_id [tag_id expected_id]]|
       [family_id [tag_id expected_id]]]]];
    subst family expected;
    unfold cost_child_ready in ready;
    rewrite child_tag in ready;
    try exact (ready tag_id).
  destruct ready as [_ [_ signer_ready]].
  exact (signer_ready tag_id).
Qed.

Lemma wellformed_cost_pbbytes_reparse :
  forall raw,
    Admission.source_raw_bytes_well_formed raw = true ->
    Core.independently_decode_scalar Data.PBBytes false
      (Schema.raw_body raw) =
      Some (Core.SourceBlob (Schema.raw_body raw)).
Proof.
  intros raw body.
  unfold Admission.source_raw_bytes_well_formed in body.
  apply andb_true_iff in body as [_ bytes].
  unfold Core.independently_decode_scalar.
  rewrite bytes. reflexivity.
Qed.

Lemma selected_cost_target_score_is_family_score :
  forall raw family program subjects,
    Cost.cost_program raw = Some (family, program) ->
    Target.sem_score_evidence
      (Target.semantic_reduce (Node raw subjects)) =
    Target.score_evidence_for family
      (if family =? 13 then Some true else None)
      (Target.semantic_reduce_forest subjects).
Proof.
  intros raw family program subjects selected.
  destruct (Projection.cost_program_projects_to_target
    raw family program selected) as
    [resolved [_ [_ member]]].
  destruct (CostChildren.selected_cost_program_is_generated
    raw family program selected) as [_ found].
  pose proof (RPB3IndependentOracleProcessScalarDecoderBridge.generated_program_has_requested_family
    family program found)
    as program_family.
  unfold Target.semantic_reduce, Target.reduce_semantic_field.
  rewrite resolved, program_family.
  cbn [Target.sem_score_evidence].
  destruct member as
    [family_id|[family_id|[family_id|[family_id|[family_id|[]]]]]];
    subst family; reflexivity.
Qed.

Lemma unrejected_selected_cost_target_has_valid_body_and_scan :
  forall raw family program subjects,
    Cost.cost_program raw = Some (family, program) ->
    Target.sem_definitely_refused
      (Target.semantic_reduce (Node raw subjects)) = false ->
    Admission.source_raw_bytes_well_formed raw = true /\
    Schema.scan_valid
      (fold_left (Target.scan_child program)
        (Target.semantic_reduce_forest subjects)
        Schema.empty_scan) = true.
Proof.
  intros raw family program subjects selected accepted.
  destruct (Projection.cost_program_projects_to_target
    raw family program selected) as [resolved _].
  change (Target.sem_definitely_refused
    (Target.reduce_semantic_field raw
      (Target.semantic_reduce_forest subjects)) = false) in accepted.
  unfold Target.reduce_semantic_field in accepted.
  rewrite resolved in accepted.
  cbn [Target.sem_definitely_refused] in accepted.
  destruct (Schema.valid_raw_body raw) eqn:body; [|discriminate].
  destruct (Schema.scan_valid
      (fold_left (Target.scan_child program)
        (Target.semantic_reduce_forest subjects)
        Schema.empty_scan)) eqn:scan.
  2: { cbn [negb] in accepted.
       rewrite orb_true_r in accepted.
       cbn [orb] in accepted. discriminate. }
  split; [change (Schema.valid_raw_body raw = true); exact body|reflexivity].
Qed.

Lemma accepted_cost_scalar_leaf_projects_fully :
  forall program raw summary,
    Cost.source_scalar_leaf program raw = Some summary ->
    Target.semantic_reduce (Node raw FNil) =
      Projection.project_source_summary summary.
Proof.
  intros program raw summary accepted.
  unfold Cost.source_scalar_leaf, Admission.source_typed_scalar,
    Admission.source_scalar_value_of_raw in accepted.
  destruct (Schema.raw_payload raw) eqn:payload; try discriminate.
  destruct (Schema.raw_program_type_id raw) as [type_id|]
    eqn:no_type; [discriminate|].
  destruct (Schema.resolve_tag (Schema.program_fields program)
    (Schema.raw_tag raw)) as [descriptor|]; [|discriminate].
  destruct (Admission.source_scalar_contract_for
    (Schema.program_family program) (Schema.raw_tag raw))
    as [contract|]; [|discriminate].
  destruct (Admission.source_scalar_wire
    (Admission.source_contract_kind contract)) as [wire|];
    [|discriminate].
  destruct (Admission.source_raw_bytes_well_formed raw &&
      Schema.wire_eqb (Schema.raw_wire raw) wire &&
      Schema.wire_eqb wire (Schema.descriptor_wire descriptor) &&
      Schema.payload_eqb (Schema.descriptor_payload descriptor)
        Schema.Scalar) eqn:guard; [|discriminate].
  destruct (Core.independently_decode_scalar
      (Admission.source_contract_kind contract)
      (Admission.source_contract_default_forbidden contract)
      (Schema.raw_body raw)) as [value|]; [|discriminate].
  cbn [option_map] in accepted.
  inversion accepted; subst summary.
  repeat rewrite andb_true_iff in guard.
  assert (body : Schema.valid_raw_body raw = true).
  { change (Admission.source_raw_bytes_well_formed raw = true).
    tauto. }
  unfold Target.semantic_reduce, Target.reduce_semantic_field,
    Target.generated_wire_program,
    Projection.project_source_summary.
  rewrite payload, no_type, body.
  cbn.
  unfold Projection.project_source_type_id.
  rewrite payload, no_type.
  reflexivity.
Qed.

Lemma ready_generated_cost_scalar_child_reparses :
  forall family program raw subjects descriptor rule kind forbidden,
    In family [9; 10; 11; 12; 13] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Schema.resolve_tag (Schema.program_fields program)
      (Schema.raw_tag raw) = Some descriptor ->
    generated_cost_child_rule_for family (Schema.raw_tag raw) =
      Some rule ->
    generated_cost_child_rule_matches family (Schema.raw_tag raw)
      descriptor rule ->
    generated_cost_scalar_rule_kind rule = Some (kind, forbidden) ->
    Schema.wire_eqb (Schema.raw_wire raw)
      (Schema.descriptor_wire descriptor) = true ->
    Schema.payload_eqb (Schema.raw_payload raw)
      (Schema.descriptor_payload descriptor) = true ->
    Target.sem_definitely_refused
      (Target.semantic_reduce (Node raw subjects)) = false ->
    cost_child_ready family
      (Target.semantic_reduce (Node raw subjects)) ->
    exists summary,
      Cost.source_cost_tree (Some program) (Node raw subjects) =
        Some summary /\
      Target.semantic_reduce (Node raw subjects) =
        Projection.project_source_summary summary.
Proof.
  intros family program raw subjects descriptor rule kind forbidden
    member found resolved classified matched scalar_kind wire_match
    payload_match unrejected ready.
  destruct (generated_cost_scalar_rule_supplies_admission_contract
    family (Schema.raw_tag raw) descriptor rule kind forbidden
    matched scalar_kind) as
    [descriptor_payload [contract kind_wire]].
  apply target_wire_match_is_equality in wire_match.
  apply target_payload_match_is_equality in payload_match.
  assert (payload : Schema.raw_payload raw = Schema.Scalar)
    by congruence.
  destruct (unrejected_target_scalar_has_source_shape
    raw subjects payload unrejected) as
    [no_children [no_type body]].
  subst subjects.
  assert (bytes : Core.bytes_in_range (Schema.raw_body raw) = true).
  { unfold Admission.source_raw_bytes_well_formed in body.
    apply andb_true_iff in body as [_ bytes]. exact bytes. }
  pose proof (ready_generated_cost_scalar_has_source_decoder
    family (Schema.raw_tag raw) rule kind forbidden
    (Target.semantic_reduce (Node raw FNil)) member classified
    scalar_kind eq_refl ready bytes) as [value decoded].
  change (Target.sem_body (Target.semantic_reduce (Node raw FNil)))
    with (Schema.raw_body raw) in decoded.
  pose proof (RPB3IndependentOracleProcessScalarDecoderBridge.generated_program_has_requested_family
    family program found) as program_family.
  rewrite <- program_family in contract.
  rewrite <- wire_match in kind_wire.
  pose proof (generated_cost_scalar_reparses_after_decoder_success
    program raw descriptor kind forbidden value payload no_type
    resolved contract kind_wire (eq_sym wire_match)
    descriptor_payload body
    decoded) as leaf.
  exists (Core.SourceExactSummary
    (Core.TypedSourceScalar raw value)
    (Core.SourceFamilyAnalysis None None false false false) None).
  split.
  - cbn [Cost.source_cost_tree]. rewrite payload. exact leaf.
  - exact (accepted_cost_scalar_leaf_projects_fully program raw _ leaf).
Qed.

Lemma optional_cost_score_projects_totally :
  forall tag children,
    Target.exact_optional_child_score tag
      (map Projection.project_source_summary children) =
    match Cost.optional_source_score tag children with
    | Some score => Some (option_map Core.project_score score)
    | None => None
    end.
Proof.
  intros tag children.
  unfold Target.exact_optional_child_score,
    Cost.optional_source_score.
  rewrite <- Projection.projected_find_tag.
  destruct (find (fun child => Core.source_summary_tag child =? tag)
      children) as [child|]; [|reflexivity].
  cbn [option_map].
  change (Target.sem_score_evidence
      (Projection.project_source_summary child))
    with (Projection.project_source_evidence
      (Core.source_summary_exact_score child)).
  destruct (Core.source_summary_exact_score child);
    reflexivity.
Qed.

Lemma bound_cost_score_projects_totally :
  forall children,
    Forall (Projection.cost_child_corresponds 11) children ->
    Target.exact_cost_bound_score
      (map Projection.project_source_summary children) =
    option_map Core.project_score (Cost.cost_bound_score children).
Proof.
  intros children correspond.
  unfold Target.exact_cost_bound_score, Cost.cost_bound_score.
  rewrite Projection.projected_i32_or_zero by exact correspond.
  destruct (Cost.scalar_i32_or_zero 1 children) as [index|];
    [|reflexivity].
  cbn [option_map].
  unfold Target.signed_score_word, Cost.signed_source_word.
  now rewrite Core.project_source_score_constructor.
Qed.

Lemma region_cost_score_projects_totally :
  forall children,
    Forall (Projection.cost_child_corresponds 12) children ->
    Target.exact_cost_region_score
      (map Projection.project_source_summary children) =
    option_map Core.project_score (Cost.cost_region_score children).
Proof.
  intros children correspond.
  unfold Target.exact_cost_region_score, Cost.cost_region_score.
  rewrite (Projection.projected_u32_or_zero 1 children
    (or_introl eq_refl) correspond).
  rewrite (Projection.projected_u64_or_zero 2 children
    (or_introl eq_refl) correspond).
  rewrite (Projection.projected_u64_or_zero 3 children
    (or_intror (or_introl eq_refl)) correspond).
  rewrite (Projection.projected_u64_or_zero 4 children
    (or_intror (or_intror (or_introl eq_refl))) correspond).
  rewrite (Projection.projected_u64_or_zero 5 children
    (or_intror (or_intror (or_intror eq_refl))) correspond).
  rewrite optional_cost_score_projects_totally.
  rewrite (Projection.projected_u32_or_zero 7 children
    (or_intror eq_refl) correspond).
  destruct (Cost.scalar_u32_or_zero 1 children) as [version|];
    destruct (Cost.scalar_u64_or_zero 2 children) as [start_line|];
    destruct (Cost.scalar_u64_or_zero 3 children) as [start_column|];
    destruct (Cost.scalar_u64_or_zero 4 children) as [end_line|];
    destruct (Cost.scalar_u64_or_zero 5 children) as [end_column|];
    destruct (Cost.optional_source_score 6 children) as [signer|];
    destruct (Cost.scalar_u32_or_zero 7 children) as [ordinal|];
    try reflexivity.
  destruct signer as [signer_score|];
    cbn [option_map];
    rewrite Core.project_source_score_constructor;
    reflexivity.
Qed.

Definition cost_plain_child (child : Core.source_exact_summary) : Prop :=
  match Core.source_summary_value child with
  | Core.TypedSourceScalar _ _ | Core.TypedSourceMessage _ _ => True
  | _ => False
  end.

Lemma actual_cost_forest_children_are_plain :
  forall program subjects children,
    Cost.source_cost_forest program subjects = Some children ->
    Forall cost_plain_child children.
Proof.
  intros program subjects children accepted.
  pose proof (CostChildren.cost_forest_children_have_source_trees
    program subjects children accepted) as origins.
  apply Forall_forall. intros child present.
  destruct (proj1 (Forall_forall _ _) origins child present)
    as [subject evaluated].
  destruct (CostChildren.cost_tree_child_shape
    program subject child evaluated) as
    [[raw [scalar [_ [value _]]]]|
     [raw [values [_ value]]]].
  - unfold cost_plain_child. now rewrite value.
  - unfold cost_plain_child. now rewrite value.
Qed.

Lemma atom_cost_score_projects_totally_for_actual_child_shapes :
  forall children,
    Forall (Projection.cost_child_corresponds 10) children ->
    Forall cost_plain_child children ->
    Target.exact_cost_atom_score
      (map Projection.project_source_summary children) =
    option_map Core.project_score (Cost.cost_atom_score children).
Proof.
  intros children correspond plain.
  destruct children as [|child [|second rest]];
    [reflexivity| |reflexivity].
  inversion correspond as [|? ? child_correspond _]; subst.
  inversion plain as [|? ? child_plain _]; subst.
  unfold Projection.cost_child_corresponds in child_correspond.
  destruct child_correspond as [bytes [_ [_ [_ payload]]]].
  specialize (payload eq_refl).
  unfold Projection.cost_atom_payload_corresponds in payload.
  unfold cost_plain_child in child_plain.
  destruct child as [value analysis default].
  destruct value as [raw scalar|raw values|raw key value|raw snapshot values];
    try contradiction.
  - cbn [Core.source_summary_value Core.source_summary_tag
      Core.source_summary_raw Core.source_summary_body Core.source_value_raw]
      in bytes, payload.
    cbn [map Cost.cost_atom_score Target.exact_cost_atom_score
      Projection.project_source_summary Target.sem_tag Target.sem_payload
      Target.sem_body Core.source_summary_value Core.source_summary_tag
      Core.source_summary_raw Core.source_summary_body
      Core.source_value_raw].
    destruct (Schema.raw_tag raw) as [|[|[|[|[|extra]]]]]
      eqn:tag;
      unfold Core.source_summary_tag, Core.source_summary_body,
        Core.source_summary_raw,
        Core.source_value_raw in *;
      cbn [Core.source_summary_value] in *;
      rewrite tag;
      cbn [Cost.cost_atom_score Target.exact_cost_atom_score option_map
        Core.source_summary_tag Core.source_summary_raw
        Core.source_value_raw];
      try reflexivity.
    + assert (chosen : Schema.raw_tag raw = 1 \/
          Schema.raw_tag raw = 2 \/ Schema.raw_tag raw = 4)
        by (rewrite tag; left; reflexivity).
      specialize (bytes eq_refl chosen).
      destruct scalar; try contradiction.
      cbn in bytes.
      unfold Core.source_summary_body in bytes.
      cbn in bytes. rewrite payload, <- bytes.
      cbn [option_map].
      now rewrite Core.project_source_score_constructor.
    + assert (chosen : Schema.raw_tag raw = 1 \/
          Schema.raw_tag raw = 2 \/ Schema.raw_tag raw = 4)
        by (rewrite tag; right; left; reflexivity).
      specialize (bytes eq_refl chosen).
      destruct scalar; try contradiction.
      cbn in bytes.
      unfold Core.source_summary_body in bytes.
      cbn in bytes. rewrite payload, <- bytes.
      cbn [option_map].
      now rewrite Core.project_source_score_constructor.
    + rewrite payload. reflexivity.
    + assert (chosen : Schema.raw_tag raw = 1 \/
          Schema.raw_tag raw = 2 \/ Schema.raw_tag raw = 4)
        by (rewrite tag; right; right; reflexivity).
      specialize (bytes eq_refl chosen).
      destruct scalar; try contradiction.
      cbn in bytes.
      unfold Core.source_summary_body in bytes.
      cbn in bytes. rewrite payload, <- bytes.
      cbn [option_map].
      now rewrite Core.project_source_score_constructor.
  - cbn [Core.source_summary_value Core.source_summary_tag
      Core.source_summary_raw Core.source_summary_body Core.source_value_raw]
      in payload.
    cbn [map Cost.cost_atom_score Target.exact_cost_atom_score
      Projection.project_source_summary Target.sem_tag Target.sem_payload
      Target.sem_score_evidence Core.source_summary_value
      Core.source_summary_tag Core.source_summary_raw
      Core.source_value_raw Core.source_summary_exact_score
      Core.source_summary_analysis Core.source_family_score].
    destruct (Schema.raw_tag raw) as [|[|[|[|[|extra]]]]]
      eqn:tag;
      unfold Core.source_summary_tag, Core.source_summary_raw,
        Core.source_value_raw in *;
      cbn [Core.source_summary_value] in *;
      rewrite tag;
      cbn [Cost.cost_atom_score Target.exact_cost_atom_score option_map];
      try rewrite payload;
      try reflexivity.
    unfold Core.source_summary_exact_score.
    cbn [Core.source_summary_analysis].
    destruct (Core.source_family_score analysis);
      cbn [Projection.project_source_evidence option_map]; reflexivity.
Qed.

Lemma cost_family_score_projects_totally_for_actual_children :
  forall family children,
    In family [9; 10; 11; 12; 13] ->
    Forall (Projection.cost_child_corresponds family) children ->
    Forall cost_plain_child children ->
    Target.score_evidence_for family
      (if family =? 13 then Some true else None)
      (map Projection.project_source_summary children) =
    Projection.project_source_evidence
      (Cost.cost_family_score family children).
Proof.
  intros family children member correspond plain.
  destruct member as
    [member|[member|[member|[member|[member|[]]]]]];
    subst family.
  - cbn [Target.score_evidence_for Cost.cost_family_score].
    unfold Target.exact_or_unknown,
      Target.exact_cost_collection_score,
      Projection.project_source_evidence.
    rewrite selected_cost_scores_project_totally.
    destruct (Cost.selected_source_scores 1 children)
      as [scores|]; cbn [option_map];
      [rewrite Core.project_source_score_constructor|];
      reflexivity.
  - cbn [Target.score_evidence_for Cost.cost_family_score].
    unfold Target.exact_or_unknown,
      Projection.project_source_evidence.
    rewrite (atom_cost_score_projects_totally_for_actual_child_shapes
      children correspond plain).
    destruct (Cost.cost_atom_score children); reflexivity.
  - cbn [Target.score_evidence_for Cost.cost_family_score].
    unfold Target.exact_or_unknown,
      Projection.project_source_evidence.
    rewrite (bound_cost_score_projects_totally children correspond).
    destruct (Cost.cost_bound_score children); reflexivity.
  - cbn [Target.score_evidence_for Cost.cost_family_score].
    unfold Target.exact_or_unknown,
      Projection.project_source_evidence.
    rewrite (region_cost_score_projects_totally children correspond).
    destruct (Cost.cost_region_score children); reflexivity.
  - cbn [Target.score_evidence_for Cost.cost_family_score].
    unfold Target.exact_or_unknown,
      Target.exact_cost_collection_score,
      Projection.project_source_evidence.
    rewrite selected_cost_scores_project_totally.
    destruct (Cost.selected_source_scores 1 children)
      as [scores|]; cbn [option_map];
      [rewrite Core.project_source_score_constructor|];
      reflexivity.
Qed.


Lemma accepted_cost_message_projects_fully :
  forall raw family program children summary,
    Cost.cost_program raw = Some (family, program) ->
    Forall (Projection.cost_child_corresponds family) children ->
    Forall cost_plain_child children ->
    Cost.source_cost_message raw family program children =
      Some summary ->
    Target.reduce_semantic_field raw
      (map Projection.project_source_summary children) =
      Projection.project_source_summary summary.
Proof.
  intros raw family program children summary selected correspond plain
    accepted.
  destruct (Projection.cost_program_projects_to_target
    raw family program selected) as
    [resolved [payload [raw_id member]]].
  unfold Cost.source_cost_message in accepted.
  destruct (Admission.source_raw_bytes_well_formed raw &&
      Admission.source_scan_valid
        (Admission.source_scan_children program children) &&
      Admission.source_required_pathmaps_present program
        (Admission.source_scan_children program children) &&
      (Schema.program_family program =? family)) eqn:guard;
    [|discriminate].
  destruct (Cost.cost_family_score family children) as [score|]
    eqn:scored; [|discriminate].
  cbn [option_map] in accepted.
  inversion accepted; subst summary.
  repeat rewrite andb_true_iff in guard.
  assert (body : Schema.valid_raw_body raw = true).
  { change (Admission.source_raw_bytes_well_formed raw = true).
    tauto. }
  assert (source_valid : Admission.source_scan_valid
    (Admission.source_scan_children program children) = true) by tauto.
  assert (source_paths : Admission.source_required_pathmaps_present
    program (Admission.source_scan_children program children) = true)
    by tauto.
  assert (program_family : Schema.program_family program = family).
  { apply Nat.eqb_eq. tauto. }
  destruct (Projection.source_scan_children_project_valid
    program children source_valid) as [target_valid [_ same_seen]].
  pose proof (Projection.required_pathmaps_project program
    (Admission.source_scan_children program children)
    (fold_left (Target.scan_child program)
      (map Projection.project_source_summary children)
      Schema.empty_scan) same_seen source_paths) as target_paths.
  pose proof (cost_family_score_projects_totally_for_actual_children
    family children member correspond plain) as target_score.
  rewrite scored in target_score.
  set (projected := map Projection.project_source_summary children) in *.
  unfold Target.reduce_semantic_field,
    Projection.project_source_summary,
    Projection.project_source_type_id.
  cbn [Core.source_summary_raw Core.source_summary_value
    Core.source_value_raw Core.source_summary_analysis].
  unfold Projection.project_source_type_id.
  rewrite resolved, program_family, payload, raw_id.
  rewrite (Projection.cost_family_completion_projects family _ _ member).
  rewrite body, target_valid, target_paths.
  rewrite target_score.
  destruct member as
    [member|[member|[member|[member|[member|[]]]]]];
    subst family; reflexivity.
Qed.

(** This is an internal induction step. Its forest-alignment hypothesis is
    constructed by the simultaneous tree/forest induction below; it is not
    a premise of the public verdict theorem. *)
Lemma reconstructed_selected_cost_message_accepts :
  forall raw family program subjects children,
    Cost.cost_program raw = Some (family, program) ->
    Cost.source_cost_forest program subjects = Some children ->
    Target.semantic_reduce_forest subjects =
      map Projection.project_source_summary children ->
    Target.sem_definitely_refused
      (Target.semantic_reduce (Node raw subjects)) = false ->
    (exists score,
      Target.sem_score_evidence
        (Target.semantic_reduce (Node raw subjects)) =
        Target.ExactScore score) ->
    exists summary,
      Cost.source_cost_message raw family program children =
        Some summary /\
      Target.semantic_reduce (Node raw subjects) =
        Projection.project_source_summary summary.
Proof.
  intros raw family program subjects children selected forest aligned
    unrejected [target_score exact].
  destruct (CostChildren.selected_cost_program_is_generated
    raw family program selected) as [member found].
  destruct (unrejected_selected_cost_target_has_valid_body_and_scan
    raw family program subjects selected unrejected)
    as [body target_scan].
  rewrite aligned in target_scan.
  pose proof (accepted_cost_forest_children_have_reparse_guards
    program subjects children forest) as guards.
  destruct (valid_target_scan_reconstructs_source
    program children guards target_scan)
    as [source_scan [_ _]].
  pose proof (CostChildren.generated_cost_forest_children_correspond
    family program subjects children member found forest source_scan)
    as correspond.
  pose proof (actual_cost_forest_children_are_plain
    program subjects children forest) as plain.
  pose proof (cost_family_score_projects_totally_for_actual_children
    family children member correspond plain) as score_projection.
  pose proof (selected_cost_target_score_is_family_score
    raw family program subjects selected) as family_score.
  rewrite aligned in family_score.
  rewrite family_score in exact.
  rewrite score_projection in exact.
  destruct (Cost.cost_family_score family children) as [source_score|]
    eqn:scored; [|discriminate].
  assert (paths : Admission.source_required_pathmaps_present program
    (Admission.source_scan_children program children) = true).
  { exact (selected_cost_program_needs_no_pathmaps raw family program
      (Admission.source_scan_children program children) selected). }
  pose proof (RPB3IndependentOracleProcessScalarDecoderBridge.generated_program_has_requested_family
    family program found) as program_family.
  assert (message : exists summary,
      Cost.source_cost_message raw family program children =
        Some summary).
  { unfold Cost.source_cost_message.
    rewrite body, source_scan, paths, program_family, Nat.eqb_refl,
      scored.
    eexists. reflexivity. }
  destruct message as [summary message].
  exists summary. split; [exact message|].
  cbn [Target.semantic_reduce].
  rewrite aligned.
  exact (accepted_cost_message_projects_fully raw family program
    children summary selected correspond plain message).
Qed.

Theorem accepted_cost_tree_and_forest_project_fully :
  (forall subject parent summary,
    Cost.source_cost_tree parent subject = Some summary ->
    Target.semantic_reduce subject =
      Projection.project_source_summary summary) /\
  (forall subjects program children,
    Cost.source_cost_forest program subjects = Some children ->
    Target.semantic_reduce_forest subjects =
      map Projection.project_source_summary children).
Proof.
  apply tree_forest_induction.
  - intros raw subjects forest_IH parent summary accepted.
    cbn [Cost.source_cost_tree] in accepted.
    destruct (Schema.raw_payload raw) eqn:payload;
      try discriminate.
    + destruct parent as [program|]; [|discriminate].
      destruct subjects as [|subject rest]; [|discriminate].
      exact (accepted_cost_scalar_leaf_projects_fully
        program raw summary accepted).
    + destruct (Cost.cost_program raw) as [[family program]|]
        eqn:selected; [|discriminate].
      destruct (Cost.source_cost_forest program subjects) as [children|]
        eqn:forest; [|discriminate].
      pose proof (CostChildren.accepted_cost_message_children_correspond
        raw family program subjects children summary selected forest
        accepted) as correspond.
      pose proof (actual_cost_forest_children_are_plain
        program subjects children forest) as plain.
      cbn [Target.semantic_reduce].
      rewrite (forest_IH program children forest).
      exact (accepted_cost_message_projects_fully
        raw family program children summary selected correspond plain
        accepted).
  - intros program children accepted.
    cbn [Cost.source_cost_forest] in accepted.
    inversion accepted; reflexivity.
  - intros subject tree_IH rest forest_IH program children accepted.
    cbn [Cost.source_cost_forest] in accepted.
    destruct (Cost.source_cost_tree (Some program) subject) as [child|]
      eqn:head; [|discriminate].
    destruct (Cost.source_cost_forest program rest) as [siblings|]
      eqn:tail; [|discriminate].
    inversion accepted; subst children.
    cbn [Target.semantic_reduce_forest map].
    rewrite (tree_IH (Some program) child head).
    rewrite (forest_IH program siblings tail).
    reflexivity.
Qed.

Definition selected_cost_tree (subject : @tree Schema.raw_field)
    (family : nat) (program : Schema.generated_program) : Prop :=
  match subject with
  | Node raw _ => Cost.cost_program raw = Some (family, program)
  end.

Theorem exact_cost_tree_and_ready_forest_reparse :
  (forall subject family program,
    selected_cost_tree subject family program ->
    Target.sem_definitely_refused (Target.semantic_reduce subject) = false ->
    (exists score,
      Target.sem_score_evidence (Target.semantic_reduce subject) =
        Target.ExactScore score) ->
    exists summary,
      Cost.source_cost_tree None subject = Some summary /\
      Target.semantic_reduce subject =
        Projection.project_source_summary summary) /\
  (forall subjects family program state,
    In family [9; 10; 11; 12; 13] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Schema.scan_valid
      (fold_left (Target.scan_child program)
        (Target.semantic_reduce_forest subjects) state) = true ->
    Forall (cost_child_ready family)
      (Target.semantic_reduce_forest subjects) ->
    exists children,
      Cost.source_cost_forest program subjects = Some children /\
      Target.semantic_reduce_forest subjects =
        map Projection.project_source_summary children).
Proof.
  apply tree_forest_induction.
  - intros raw subjects forest_IH family program selected clear exact.
    unfold selected_cost_tree in selected.
    destruct (CostChildren.selected_cost_program_is_generated
      raw family program selected) as [member found].
    destruct (unrejected_selected_cost_target_has_valid_body_and_scan
      raw family program subjects selected clear) as [_ target_scan].
    destruct exact as [score exact].
    pose proof exact as target_exact.
    pose proof (selected_cost_target_score_is_family_score
      raw family program subjects selected) as family_score.
    rewrite family_score in exact.
    pose proof (exact_generated_cost_score_certifies_children
      raw family program (Target.semantic_reduce_forest subjects)
      score selected target_scan exact) as ready.
    destruct (forest_IH family program Schema.empty_scan member found
      target_scan ready) as [children [forest aligned]].
    destruct (reconstructed_selected_cost_message_accepts
      raw family program subjects children selected forest aligned
      clear (ex_intro _ score target_exact))
      as [summary [message projection]].
    exists summary. split; [|exact projection].
    cbn [Cost.source_cost_tree].
    destruct (Projection.cost_program_projects_to_target
      raw family program selected) as [_ [payload _]].
    rewrite payload, selected, forest.
    exact message.
  - intros family program state member found valid ready.
    exists []. split; reflexivity.
  - intros subject subject_IH rest rest_IH family program state
      member found final ready.
    cbn [Target.semantic_reduce_forest] in ready, final |- *.
    inversion ready as [|? ? head_ready tail_ready]; subst.
    pose proof (valid_target_scan_matches_each_child
      program state
      (Target.semantic_reduce subject ::
        Target.semantic_reduce_forest rest) final) as all_matched.
    inversion all_matched as [|? ? head_matched _]; subst.
    destruct head_matched as
      [descriptor [resolved [unrejected [wire_match
        [payload_match target_type]]]]].
    destruct subject as [raw subjects].
    change (Target.sem_tag
      (Target.semantic_reduce (Node raw subjects)))
      with (Schema.raw_tag raw) in resolved.
    change (Target.sem_wire
      (Target.semantic_reduce (Node raw subjects)))
      with (Schema.raw_wire raw) in wire_match.
    change (Target.sem_payload
      (Target.semantic_reduce (Node raw subjects)))
      with (Schema.raw_payload raw) in payload_match.
    pose proof (generated_cost_descriptor_has_exact_child_rule
      family program (Schema.raw_tag raw) descriptor
      member found resolved) as [rule [classified matched]].
    assert (child_result : exists child,
      Cost.source_cost_tree (Some program) (Node raw subjects) =
        Some child /\
      Target.semantic_reduce (Node raw subjects) =
        Projection.project_source_summary child).
    { destruct rule as [| | | |expected].
      - eapply ready_generated_cost_scalar_child_reparses;
          eauto; reflexivity.
      - eapply ready_generated_cost_scalar_child_reparses;
          eauto; reflexivity.
      - eapply ready_generated_cost_scalar_child_reparses;
          eauto; reflexivity.
      - eapply ready_generated_cost_scalar_child_reparses;
          eauto; reflexivity.
      - cbn [generated_cost_child_rule_matches] in matched.
        destruct matched as [_ [descriptor_payload
          [child_id child_member]]].
        apply target_payload_match_is_equality in payload_match.
        assert (payload : Schema.raw_payload raw = Schema.Message)
          by congruence.
        rewrite descriptor_payload, child_id in target_type.
        apply Nat.eqb_eq in target_type.
        destruct (unrejected_target_cost_message_has_selected_program
          raw subjects expected payload child_member unrejected
          target_type) as [child_program child_selected].
        pose proof (ready_generated_cost_message_has_exact_score
          family (Schema.raw_tag raw) expected
          (Target.semantic_reduce (Node raw subjects)) member
          classified eq_refl head_ready) as child_exact.
        destruct (subject_IH expected child_program child_selected
          unrejected child_exact) as [child [source_child projection]].
        exists child. split; [|exact projection].
        cbn [Cost.source_cost_tree] in source_child |- *.
        rewrite payload in source_child |- *.
        exact source_child. }
    destruct child_result as [child [source_child projection]].
    pose proof (target_scan_valid_prefix program state
      (Target.semantic_reduce (Node raw subjects))
      (Target.semantic_reduce_forest rest) final) as step_valid.
    destruct (rest_IH family program
      (Target.scan_child program state
        (Target.semantic_reduce (Node raw subjects)))
      member found final tail_ready) as
      [siblings [source_siblings projected_siblings]].
    exists (child :: siblings).
    split.
    + cbn [Cost.source_cost_forest].
      rewrite source_child, source_siblings. reflexivity.
    + cbn [Target.semantic_reduce_forest map].
      rewrite projection, projected_siblings. reflexivity.
Qed.

Lemma accepted_selected_cost_root_has_exact_source_score :
  forall raw subjects family program summary,
    Cost.cost_program raw = Some (family, program) ->
    Cost.source_cost_root (Node raw subjects) = Some summary ->
    Core.source_summary_refused summary = false /\
    exists score,
      Core.source_summary_exact_score summary = Some score.
Proof.
  intros raw subjects family program summary selected accepted.
  unfold Cost.source_cost_root in accepted.
  cbn [Cost.source_cost_tree] in accepted.
  destruct (Projection.cost_program_projects_to_target
    raw family program selected) as [_ [payload _]].
  rewrite payload, selected in accepted.
  destruct (Cost.source_cost_forest program subjects) as [children|];
    [|discriminate].
  unfold Cost.source_cost_message in accepted.
  destruct (Admission.source_raw_bytes_well_formed raw &&
      Admission.source_scan_valid
        (Admission.source_scan_children program children) &&
      Admission.source_required_pathmaps_present program
        (Admission.source_scan_children program children) &&
      (Schema.program_family program =? family));
    [|discriminate].
  destruct (Cost.cost_family_score family children) as [score|];
    [|discriminate].
  cbn [option_map] in accepted.
  inversion accepted; subst summary.
  split; [reflexivity|].
  now exists score.
Qed.

Theorem selected_cost_root_verdict_equivalence :
  forall raw subjects family program,
    Cost.cost_program raw = Some (family, program) ->
    (forall summary,
      Cost.source_cost_root (Node raw subjects) = Some summary ->
      Target.semantic_reduce (Node raw subjects) =
        Projection.project_source_summary summary) /\
    (Cost.source_cost_root (Node raw subjects) = None <->
      Target.sem_definitely_refused
        (Target.semantic_reduce (Node raw subjects)) = true \/
      Target.sem_score_evidence
        (Target.semantic_reduce (Node raw subjects)) =
        Target.ScoreNotDerived).
Proof.
  intros raw subjects family program selected.
  split.
  - intros summary accepted.
    exact ((proj1 accepted_cost_tree_and_forest_project_fully)
      (Node raw subjects) None summary accepted).
  - split.
    + intros rejected.
      destruct (Target.sem_definitely_refused
        (Target.semantic_reduce (Node raw subjects)))
        eqn:refusal; [now left|].
      destruct (Target.sem_score_evidence
        (Target.semantic_reduce (Node raw subjects)))
        as [score|] eqn:evidence; [|now right].
      destruct ((proj1 exact_cost_tree_and_ready_forest_reparse)
        (Node raw subjects) family program selected refusal
        (ex_intro _ score evidence)) as [summary [accepted _]].
      unfold Cost.source_cost_root in rejected.
      rewrite accepted in rejected. discriminate.
    + intros [refusal|unknown].
      * destruct (Cost.source_cost_root (Node raw subjects))
          as [summary|] eqn:accepted; [|reflexivity].
        destruct (accepted_selected_cost_root_has_exact_source_score
          raw subjects family program summary selected accepted)
          as [clear _].
        pose proof ((proj1 accepted_cost_tree_and_forest_project_fully)
          (Node raw subjects) None summary accepted) as projected.
        rewrite projected in refusal.
        change (Core.source_summary_refused summary = true)
          in refusal.
        rewrite clear in refusal. discriminate.
      * destruct (Cost.source_cost_root (Node raw subjects))
          as [summary|] eqn:accepted; [|reflexivity].
        destruct (accepted_selected_cost_root_has_exact_source_score
          raw subjects family program summary selected accepted)
          as [_ [score exact]].
        pose proof ((proj1 accepted_cost_tree_and_forest_project_fully)
          (Node raw subjects) None summary accepted) as projected.
        rewrite projected in unknown.
        change (Projection.project_source_evidence
          (Core.source_summary_exact_score summary) =
          Target.ScoreNotDerived) in unknown.
        rewrite exact in unknown. discriminate.
Qed.

Corollary selected_cost_root_acceptance_iff :
  forall raw subjects family program,
    Cost.cost_program raw = Some (family, program) ->
    (exists summary,
      Cost.source_cost_root (Node raw subjects) = Some summary) <->
    Target.sem_definitely_refused
      (Target.semantic_reduce (Node raw subjects)) = false /\
    exists score,
      Target.sem_score_evidence
        (Target.semantic_reduce (Node raw subjects)) =
        Target.ExactScore score.
Proof.
  intros raw subjects family program selected.
  split.
  - intros [summary accepted].
    destruct (accepted_selected_cost_root_has_exact_source_score
      raw subjects family program summary selected accepted)
      as [clear [score exact]].
    pose proof ((proj1 accepted_cost_tree_and_forest_project_fully)
      (Node raw subjects) None summary accepted) as projected.
    rewrite projected.
    split.
    + change (Core.source_summary_refused summary = false).
      exact clear.
    + exists (Core.project_score score).
      change (Projection.project_source_evidence
        (Core.source_summary_exact_score summary) =
        Target.ExactScore (Core.project_score score)).
      now rewrite exact.
  - intros [clear [score exact]].
    destruct ((proj1 exact_cost_tree_and_ready_forest_reparse)
      (Node raw subjects) family program selected clear
      (ex_intro _ score exact)) as [summary [accepted _]].
    now exists summary.
Qed.

Print Assumptions explicit_zero_cost_bound_is_unknown_without_refusal.
Print Assumptions malformed_cost_atom_may_keep_an_exact_score_while_refused.
Print Assumptions cost_root_restriction_is_necessary.
Print Assumptions selected_cost_scores_project_totally.
Print Assumptions exact_selected_scores_certifies_children.
Print Assumptions repeated_cost_score_certifies_children.
Print Assumptions target_scan_invalid_stays_invalid.
Print Assumptions valid_target_scan_matches_each_child.
Print Assumptions unrejected_target_scalar_has_source_shape.
Print Assumptions unique_target_tag_is_the_score_selector.
Print Assumptions target_scan_valid_prefix.
Print Assumptions valid_target_scan_step_keeps_seen.
Print Assumptions valid_target_scan_forbids_a_seen_singular_tag.
Print Assumptions resolved_descriptor_belongs_to_program.
Print Assumptions target_nonrepeated_scan_has_distinct_tags.
Print Assumptions generated_cost_region_descriptors_are_singular.
Print Assumptions generated_cost_bound_descriptors_are_singular.
Print Assumptions valid_cost_region_scan_has_distinct_tags.
Print Assumptions exact_u32_selector_certifies_present_child.
Print Assumptions exact_u64_selector_certifies_present_child.
Print Assumptions exact_i32_selector_certifies_present_child.
Print Assumptions exact_optional_selector_certifies_present_child.
Print Assumptions exact_region_score_certifies_children.
Print Assumptions exact_bound_score_certifies_children.
Print Assumptions exact_atom_score_certifies_children.
Print Assumptions exact_generated_cost_score_certifies_children.
Print Assumptions accepted_cost_child_has_reparse_guard.
Print Assumptions accepted_cost_forest_children_have_reparse_guards.
Print Assumptions reparsed_cost_child_descriptor_guard_agrees.
Print Assumptions valid_target_scan_step_reconstructs_source.
Print Assumptions valid_target_scan_reconstructs_source.
Print Assumptions selected_cost_program_needs_no_pathmaps.
Print Assumptions generated_cost_scalar_reparses_after_decoder_success.
Print Assumptions generated_cost_descriptor_has_exact_child_rule.
Print Assumptions unrejected_target_cost_message_has_selected_program.
Print Assumptions target_wire_match_is_equality.
Print Assumptions target_payload_match_is_equality.
Print Assumptions generated_cost_child_rule_inverse.
Print Assumptions ready_generated_cost_scalar_has_source_decoder.
Print Assumptions generated_cost_scalar_rule_supplies_admission_contract.
Print Assumptions ready_generated_cost_message_has_exact_score.
Print Assumptions wellformed_cost_pbbytes_reparse.
Print Assumptions selected_cost_target_score_is_family_score.
Print Assumptions unrejected_selected_cost_target_has_valid_body_and_scan.
Print Assumptions accepted_cost_scalar_leaf_projects_fully.
Print Assumptions ready_generated_cost_scalar_child_reparses.
Print Assumptions optional_cost_score_projects_totally.
Print Assumptions bound_cost_score_projects_totally.
Print Assumptions region_cost_score_projects_totally.
Print Assumptions actual_cost_forest_children_are_plain.
Print Assumptions atom_cost_score_projects_totally_for_actual_child_shapes.
Print Assumptions cost_family_score_projects_totally_for_actual_children.
Print Assumptions accepted_cost_message_projects_fully.
Print Assumptions reconstructed_selected_cost_message_accepts.
Print Assumptions accepted_cost_tree_and_forest_project_fully.
Print Assumptions exact_cost_tree_and_ready_forest_reparse.
Print Assumptions accepted_selected_cost_root_has_exact_source_score.
Print Assumptions selected_cost_root_verdict_equivalence.
Print Assumptions selected_cost_root_acceptance_iff.
