From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3IndependentOracleProcess RPB3IndependentOracleRecursive
  RPB3IndependentOracleProjection
  RPB3RecursiveAuxiliarySourceReference.

Import ListNotations.

Module Source := RPB3RawRegionTree.
Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Process := RPB3IndependentOracleProcess.
Module Oracle := RPB3IndependentOracleRecursive.
Module Projection := RPB3IndependentOracleProjection.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.

(** The target refuses a child as soon as its semantic result is refused.
    The independent evaluator retains that child and marks its parent
    refused.  This local gate makes those two list policies comparable. *)
Definition source_unrejected_child
    (result : option Core.source_exact_summary)
    : option Core.source_exact_summary :=
  match result with
  | None => None
  | Some summary =>
      if Core.source_summary_refused summary then None else Some summary
  end.

Definition source_target_child_verdict
    (source : option Core.source_exact_summary)
    (target : option Recursive.recursive_mixed_result) : Prop :=
  match source, target with
  | None, None => True
  | Some summary, None => Core.source_summary_refused summary = true
  | Some summary, Some result =>
      Core.source_summary_refused summary = false /\
      Recursive.recursive_result_semantic result =
        Projection.project_source_summary summary
  | None, Some _ => False
  end.

Lemma source_target_child_verdict_projects :
  forall source target,
    source_target_child_verdict source target ->
    option_map Projection.project_source_summary
      (source_unrejected_child source) =
    option_map Recursive.recursive_result_semantic target.
Proof.
  intros [summary|] [result|] verdict; simpl in verdict |- *.
  - destruct verdict as [accepted projected].
    rewrite accepted. simpl.
    now rewrite projected.
  - now rewrite verdict.
  - contradiction.
  - reflexivity.
Qed.

Lemma source_sequence_projects :
  forall (A B : Type) (project : A -> B) results,
    option_map (map project) (Source.sequence_source_results results) =
      Source.sequence_source_results
        (map (option_map project) results).
Proof.
  intros A B project results.
  induction results as [|[head|] rest IH]; simpl;
    try reflexivity.
  rewrite <- IH.
  destruct (Source.sequence_source_results rest); reflexivity.
Qed.

Lemma recursive_source_children_as_sequence :
  forall par_child hash_fn bytes program nodes,
    Recursive.recursive_source_children par_child hash_fn bytes
      program nodes =
    Source.sequence_source_results
      (map (Recursive.recursive_source_reference par_child hash_fn
        bytes program) nodes).
Proof.
  intros par_child hash_fn bytes program nodes.
  induction nodes as [|node rest IH];
    cbn [Recursive.recursive_source_children map
      Source.sequence_source_results]; [reflexivity|].
  rewrite IH.
  destruct (Recursive.recursive_source_reference par_child hash_fn
    bytes program node);
    destruct (Source.sequence_source_results
      (map (Recursive.recursive_source_reference par_child hash_fn
        bytes program) rest)); reflexivity.
Qed.

Definition source_admitted_children
    (source_child : Source.source_tree ->
      option Core.source_exact_summary)
    (nodes : list Source.source_tree)
    : option (list Core.source_exact_summary) :=
  Source.sequence_source_results
    (map source_unrejected_child (map source_child nodes)).

(** This is a list-algebra bridge, not yet a recursive node theorem.  The
    pointwise premise must be discharged for actual scalar, message, and
    auxiliary nodes before it can establish universal Par equivalence. *)
Theorem source_target_child_lists_project :
  forall par_child hash_fn bytes program nodes source_child,
    (forall node, In node nodes ->
      source_target_child_verdict (source_child node)
        (Recursive.recursive_source_reference par_child hash_fn
          bytes program node)) ->
    option_map (map Projection.project_source_summary)
      (source_admitted_children source_child nodes) =
    option_map (map Recursive.recursive_result_semantic)
      (Recursive.recursive_source_children par_child hash_fn
        bytes program nodes).
Proof.
  intros par_child hash_fn bytes program nodes source_child agree.
  unfold source_admitted_children.
  rewrite recursive_source_children_as_sequence.
  rewrite !source_sequence_projects.
  rewrite !map_map.
  f_equal.
  apply map_ext_in.
  intros node member.
  apply source_target_child_verdict_projects.
  now apply agree.
Qed.

Lemma source_unrejected_results_accepts :
  forall results summaries,
    Source.sequence_source_results results = Some summaries ->
    Forall (fun summary =>
      Core.source_summary_refused summary = false) summaries ->
    Source.sequence_source_results
      (map source_unrejected_child results) = Some summaries.
Proof.
  induction results as [|[head|] rest IH]; intros summaries accepted
    unrefused; simpl in accepted |- *.
  - inversion accepted; reflexivity.
  - destruct (Source.sequence_source_results rest) as [tail|]
      eqn:rest_result; [|discriminate].
    inversion accepted; subst summaries.
    inversion unrefused as [|source_head source_tail head_ok tail_ok];
      subst.
    rewrite head_ok. simpl.
    now rewrite (IH tail eq_refl tail_ok).
  - discriminate.
Qed.

Lemma source_unrejected_results_recover :
  forall results summaries,
    Source.sequence_source_results
      (map source_unrejected_child results) = Some summaries ->
    Source.sequence_source_results results = Some summaries /\
    Forall (fun summary =>
      Core.source_summary_refused summary = false) summaries.
Proof.
  induction results as [|[head|] rest IH]; intros summaries admitted;
    simpl in admitted |- *.
  - inversion admitted; subst. split; [reflexivity|constructor].
  - destruct (Core.source_summary_refused head) eqn:head_ok;
      simpl in admitted; [discriminate|].
    destruct (Source.sequence_source_results
      (map source_unrejected_child rest)) as [tail|]
      eqn:rest_admitted; [|discriminate].
    inversion admitted; subst summaries.
    destruct (IH tail eq_refl) as [rest_result tail_ok].
    split.
    + now rewrite rest_result.
    + constructor; assumption.
  - discriminate.
Qed.

Theorem accepted_source_child_list_projects_to_recursive :
  forall par_child hash_fn bytes program nodes source_child summaries,
    (forall node, In node nodes ->
      source_target_child_verdict (source_child node)
        (Recursive.recursive_source_reference par_child hash_fn
          bytes program node)) ->
    Source.sequence_source_results (map source_child nodes) =
      Some summaries ->
    Forall (fun summary =>
      Core.source_summary_refused summary = false) summaries ->
    exists results,
      Recursive.recursive_source_children par_child hash_fn bytes
        program nodes = Some results /\
      map Recursive.recursive_result_semantic results =
        map Projection.project_source_summary summaries.
Proof.
  intros par_child hash_fn bytes program nodes source_child summaries
    agree selected unrefused.
  pose proof (source_target_child_lists_project par_child hash_fn
    bytes program nodes source_child agree) as projected.
  unfold source_admitted_children in projected.
  rewrite (source_unrejected_results_accepts
    (map source_child nodes) summaries selected unrefused) in projected.
  destruct (Recursive.recursive_source_children par_child hash_fn
    bytes program nodes) as [results|] eqn:target_result;
    simpl in projected; [|discriminate].
  injection projected as projected_scores.
  exists results. split; [reflexivity|].
  symmetry. exact projected_scores.
Qed.

Theorem accepted_recursive_child_list_recovers_source :
  forall par_child hash_fn bytes program nodes source_child results,
    (forall node, In node nodes ->
      source_target_child_verdict (source_child node)
        (Recursive.recursive_source_reference par_child hash_fn
          bytes program node)) ->
    Recursive.recursive_source_children par_child hash_fn bytes
      program nodes = Some results ->
    exists summaries,
      Source.sequence_source_results (map source_child nodes) =
        Some summaries /\
      Forall (fun summary =>
        Core.source_summary_refused summary = false) summaries /\
      map Recursive.recursive_result_semantic results =
        map Projection.project_source_summary summaries.
Proof.
  intros par_child hash_fn bytes program nodes source_child results
    agree accepted.
  pose proof (source_target_child_lists_project par_child hash_fn
    bytes program nodes source_child agree) as projected.
  rewrite accepted in projected.
  unfold source_admitted_children in projected.
  destruct (Source.sequence_source_results
    (map source_unrejected_child (map source_child nodes)))
    as [summaries|] eqn:admitted; simpl in projected;
    [|discriminate].
  destruct (source_unrejected_results_recover
    (map source_child nodes) summaries admitted)
    as [selected unrefused].
  injection projected as projected_scores.
  exists summaries.
  repeat split; try assumption.
  symmetry. exact projected_scores.
Qed.

(** The Par connective's scalar contract is read from the generated row.
    Successful independent decoding of its exact one-byte body determines
    the typed Boolean; the result is not supplied by a caller. *)
Lemma generated_par_connective_source_contract :
  Admission.source_scalar_contract_for
    (Schema.program_family Source.generated_par_source_program) 10 =
    Some (Admission.SourceScalarContract Data.PBBool true).
Proof. vm_compute. reflexivity. Qed.

Lemma accepted_one_byte_par_connective_is_true :
  forall raw value,
    Schema.raw_tag raw = 10 ->
    Schema.raw_body raw = [1] ->
    Admission.source_typed_scalar Source.generated_par_source_program raw =
      Some value ->
    value = Core.TypedSourceScalar raw (Core.SourceBool true).
Proof.
  intros raw value tag body accepted.
  unfold Admission.source_typed_scalar,
    Admission.source_scalar_value_of_raw in accepted.
  rewrite tag, body in accepted.
  rewrite Source.generated_par_connective_field_is_varint_scalar
    in accepted.
  rewrite generated_par_connective_source_contract in accepted.
  destruct (Schema.raw_payload raw); try discriminate.
  destruct (Schema.raw_program_type_id raw) as [type_id|]
    eqn:program_type; [discriminate|].
  cbn [Admission.source_contract_kind
    Admission.source_contract_default_forbidden
    Admission.source_scalar_wire Schema.wire_eqb
    Schema.descriptor_wire Schema.descriptor_payload
    Schema.payload_eqb] in accepted.
  destruct (Admission.source_raw_bytes_well_formed raw &&
      Schema.wire_eqb (Schema.raw_wire raw) Schema.Varint)
    eqn:well_typed; [|discriminate].
  change (option_map (Core.TypedSourceScalar raw)
      (Core.independently_decode_scalar Data.PBBool true [1]) =
    Some value) in accepted.
  cbn in accepted.
  now inversion accepted.
Qed.

Definition par_default_child_decoded (bytes : list nat)
    (node : Source.source_tree)
    (summary : Core.source_exact_summary) : Prop :=
  let value := Core.source_summary_value summary in
  Source.source_par_component node =
    existsb (Nat.eqb (Process.source_value_tag value))
      Process.source_par_component_tags /\
  Source.source_par_connective_node node =
    (Process.source_value_tag value =? 10) /\
  Core.source_summary_tag summary =
    Schema.descriptor_tag (Source.source_field node) /\
  Core.source_summary_body summary = Source.source_body bytes node /\
  (Source.source_par_connective_node node = true ->
    Admission.source_typed_scalar Source.generated_par_source_program
      (Core.source_summary_raw summary) = Some value).

Lemma paired_par_component_presence :
  forall bytes nodes summaries,
    Forall2 (par_default_child_decoded bytes) nodes summaries ->
    existsb Source.source_par_component nodes =
      existsb (fun summary =>
        existsb (Nat.eqb (Process.source_value_tag
          (Core.source_summary_value summary)))
          Process.source_par_component_tags) summaries.
Proof.
  intros bytes nodes summaries aligned.
  induction aligned as [|node summary nodes summaries
      [component [_ [_ [_ _]]]] aligned IH]; simpl;
    [reflexivity|].
  now rewrite component, IH.
Qed.

Lemma paired_par_connective_find :
  forall bytes nodes summaries,
    Forall2 (par_default_child_decoded bytes) nodes summaries ->
    match find Source.source_par_connective_node nodes,
        find (fun summary =>
          Process.source_value_tag (Core.source_summary_value summary) =? 10)
          summaries with
    | None, None => True
    | Some node, Some summary => par_default_child_decoded bytes node summary
    | _, _ => False
    end.
Proof.
  intros bytes nodes summaries aligned.
  induction aligned as [|node summary nodes summaries
      paired aligned IH]; simpl; [exact I|].
  pose proof (proj1 (proj2 paired)) as connective.
  rewrite <- connective.
  destruct (Source.source_par_connective_node node);
    [exact paired|exact IH].
Qed.

Lemma find_projected_source_values :
  forall (A B : Type) (project : A -> B) predicate members,
    find predicate (map project members) =
      option_map project
        (find (fun member => predicate (project member)) members).
Proof.
  intros A B project predicate members.
  induction members as [|member rest IH]; simpl; [reflexivity|].
  destruct (predicate (project member)); simpl; [reflexivity|exact IH].
Qed.

Lemma par_component_count_zero_is_absence :
  forall nodes,
    (Source.source_par_component_count nodes =? 0) =
      negb (existsb Source.source_par_component nodes).
Proof.
  induction nodes as [|node rest IH]; [reflexivity|].
  unfold Source.source_par_component_count in *.
  simpl.
  destruct (Source.source_par_component node); simpl;
    [reflexivity|exact IH].
Qed.

Definition typed_par_connective_used
    (value : Core.typed_source_value) : option bool :=
  match value with
  | Core.TypedSourceScalar raw (Core.SourceBool true) =>
      match Schema.raw_body raw with
      | [1] => Some true
      | _ => None
      end
  | _ => None
  end.

Lemma decoded_par_connective_used_agrees :
  forall bytes node summary,
    par_default_child_decoded bytes node summary ->
    Source.source_par_connective_node node = true ->
    (if Source.source_par_true_body bytes node then Some true else None) =
      typed_par_connective_used (Core.source_summary_value summary).
Proof.
  intros bytes node summary paired connective.
  destruct paired as [_ [_ [tag [body decoded]]]].
  destruct (Source.source_par_true_body bytes node) eqn:one.
  - apply (proj1 (Source.source_par_true_body_has_exact_original_byte
      bytes node)) in one.
    assert (raw_tag : Core.source_summary_tag summary = 10).
    {
      rewrite tag.
      unfold Source.source_par_connective_node in connective.
      apply andb_true_iff in connective.
      apply Nat.eqb_eq. exact (proj1 connective).
    }
    assert (raw_body :
      Schema.raw_body (Core.source_summary_raw summary) = [1]).
    { exact (eq_trans body one). }
    pose proof (accepted_one_byte_par_connective_is_true
      (Core.source_summary_raw summary)
      (Core.source_summary_value summary)
      raw_tag raw_body (decoded connective)) as value.
    unfold typed_par_connective_used.
    rewrite value, raw_body. reflexivity.
  - unfold typed_par_connective_used.
    unfold Core.source_summary_body, Core.source_summary_raw in body.
    destruct (Core.source_summary_value summary)
      as [raw scalar|raw children|raw key value|raw snapshot values];
      simpl; try reflexivity.
    destruct scalar as [flag|signed32|unsigned32|signed64|unsigned64|
      text|blob]; simpl; try reflexivity.
    destruct flag; simpl; [|reflexivity].
    simpl in body.
    rewrite body.
    unfold Source.source_par_true_body in one.
    destruct (Source.source_body bytes node)
      as [|first [|second rest]]; simpl in one |- *;
      try reflexivity.
    destruct first as [|[|larger]]; simpl in one |- *;
      try reflexivity; discriminate.
    destruct first as [|first']; simpl; [reflexivity|].
    destruct first'; reflexivity.
Qed.

Lemma paired_par_connective_used :
  forall bytes nodes summaries,
    Forall2 (par_default_child_decoded bytes) nodes summaries ->
    Source.source_par_connective_used bytes nodes =
      match find (fun value => Process.source_value_tag value =? 10)
          (map Core.source_summary_value summaries) with
      | None => Some false
      | Some value => typed_par_connective_used value
      end.
Proof.
  intros bytes nodes summaries aligned.
  pose proof (paired_par_connective_find bytes nodes summaries aligned)
    as selected.
  unfold Source.source_par_connective_used.
  rewrite find_projected_source_values.
  destruct (find Source.source_par_connective_node nodes)
    as [node|] eqn:source_selected;
  destruct (find (fun summary =>
      Process.source_value_tag (Core.source_summary_value summary) =? 10)
      summaries) as [summary|] eqn:typed_selected;
    simpl in selected |- *; try contradiction.
  - apply decoded_par_connective_used_agrees; [exact selected|].
    exact (proj2 (Source.selected_par_connective_is_source_member
      nodes node source_selected)).
  - reflexivity.
Qed.

Lemma existsb_projected_source_values :
  forall (predicate : Core.typed_source_value -> bool) summaries,
    existsb predicate (map Core.source_summary_value summaries) =
      existsb (fun summary =>
        predicate (Core.source_summary_value summary)) summaries.
Proof.
  intros predicate summaries.
  induction summaries as [|summary rest IH]; simpl;
    [reflexivity|now rewrite IH].
Qed.

Lemma typed_par_default_connective_algebra :
  forall has_component selected,
    (match selected with
    | None => Some (negb has_component)
    | Some (Core.TypedSourceScalar raw (Core.SourceBool true)) =>
        match Schema.raw_body raw with
        | [1] => Some false
        | _ => None
        end
    | Some _ => None
    end) =
    option_map (fun connective =>
      negb has_component && negb connective)
      (match selected with
      | None => Some false
      | Some value => typed_par_connective_used value
      end).
Proof.
  intros has_component [value|]; simpl.
  2: { now rewrite andb_true_r. }
  destruct value as [raw scalar|raw children|raw key value|
    raw snapshot values]; simpl; try reflexivity.
  destruct scalar as [flag|signed32|unsigned32|signed64|unsigned64|
    text|blob]; simpl; try reflexivity.
  destruct flag; simpl; try reflexivity.
  destruct (Schema.raw_body raw) as [|first tail]; simpl;
    try reflexivity.
  destruct tail as [|second rest]; simpl.
  - destruct first as [|[|later]]; simpl; try reflexivity.
    now rewrite andb_false_r.
  - destruct first as [|[|later]]; reflexivity.
Qed.

Lemma typed_par_default_via_connective :
  forall raw values,
    Schema.raw_program_type_id raw = Some 1 ->
    Process.source_par_typed_default
      (Core.TypedSourceMessage raw values) =
    option_map (fun connective =>
      negb (existsb (fun child =>
        existsb (Nat.eqb (Process.source_value_tag child))
          Process.source_par_component_tags) values) &&
      negb connective)
      (match find (fun child => Process.source_value_tag child =? 10)
          values with
      | None => Some false
      | Some value => typed_par_connective_used value
      end).
Proof.
  intros raw values root_type.
  unfold Process.source_par_typed_default.
  rewrite root_type.
  exact (typed_par_default_connective_algebra
    (existsb (fun child =>
      existsb (Nat.eqb (Process.source_value_tag child))
        Process.source_par_component_tags) values)
    (find (fun child => Process.source_value_tag child =? 10)
      values)).
Qed.

(** Positional equality requires the actual independent scalar decoding of
    the selected connective, plus descriptor/typed-child alignment.  The
    later recursive theorem must establish those premises for every accepted
    generated Par child; this lemma does not assume a final verdict. *)
Theorem positional_par_root_default_agrees :
  forall bytes raw nodes summaries,
    Schema.raw_program_type_id raw = Some 1 ->
    Forall2 (par_default_child_decoded bytes) nodes summaries ->
    Process.source_par_typed_default
      (Core.TypedSourceMessage raw
        (map Core.source_summary_value summaries)) =
    Source.source_par_semantic_default bytes nodes.
Proof.
  intros bytes raw nodes summaries root_type aligned.
  rewrite (typed_par_default_via_connective raw
    (map Core.source_summary_value summaries) root_type).
  unfold Source.source_par_semantic_default.
  rewrite (paired_par_connective_used bytes nodes summaries aligned).
  rewrite par_component_count_zero_is_absence.
  rewrite (paired_par_component_presence bytes nodes summaries aligned).
  rewrite existsb_projected_source_values.
  reflexivity.
Qed.

(** These role facts come from the generated Par descriptor table.  They
    identify the repeated message tags and the sole connective slot without
    consulting the target semantic reducer. *)
Lemma generated_par_descriptor_roles_match_typed_tags :
  forall field,
    In field (Schema.program_fields Source.generated_par_source_program) ->
    (Schema.descriptor_repeated field &&
      Schema.payload_eqb (Schema.descriptor_payload field)
        Schema.Message) =
      existsb (Nat.eqb (Schema.descriptor_tag field))
        Process.source_par_component_tags /\
    ((Schema.descriptor_tag field =? 10) &&
      (Schema.descriptor_slot field =? 8)) =
      (Schema.descriptor_tag field =? 10).
Proof.
  intros field member.
  vm_compute in member.
  repeat (destruct member as [member|member];
    [subst field; vm_compute; split; reflexivity|]).
  contradiction.
Qed.

Lemma generated_par_descriptor_payload_is_plain :
  forall field,
    In field (Schema.program_fields Source.generated_par_source_program) ->
    Schema.descriptor_payload field = Schema.Scalar \/
    Schema.descriptor_payload field = Schema.Message.
Proof.
  intros field member.
  vm_compute in member.
  repeat (destruct member as [member|member];
    [subst field; vm_compute;
      first [left; reflexivity | right; reflexivity]|]).
  contradiction.
Qed.

Lemma generated_par_descriptor_tag10_is_scalar :
  forall field,
    In field (Schema.program_fields Source.generated_par_source_program) ->
    Schema.descriptor_tag field = 10 ->
    Schema.descriptor_payload field = Schema.Scalar.
Proof.
  intros field member tag.
  vm_compute in member.
  repeat (destruct member as [member|member];
    [subst field; vm_compute in tag; vm_compute; congruence|]).
  contradiction.
Qed.

Lemma resolved_generated_field_is_member :
  forall fields tag field,
    Schema.resolve_tag fields tag = Some field -> In field fields.
Proof.
  induction fields as [|head rest IH]; intros tag field selected;
    simpl in selected; [discriminate|].
  destruct (Schema.descriptor_tag head =? tag) eqn:matches.
  - inversion selected; subst field. now left.
  - right. now apply IH with (tag := tag).
Qed.

Lemma matched_par_node_field_is_member :
  forall node,
    Source.source_node_matches_generated_descriptor
      Source.generated_par_source_program node ->
    In (Source.source_field node)
      (Schema.program_fields Source.generated_par_source_program).
Proof.
  intros node [matched _].
  unfold Source.source_descriptor in matched.
  destruct (Schema.resolve_tag
      (Schema.program_fields Source.generated_par_source_program)
      (Source.Wire.span_number (Source.source_span node)))
    as [selected|] eqn:resolved; [|discriminate].
  destruct (Schema.wire_eqb
      (Source.wire_kind_of_span (Source.source_span node))
      (Schema.descriptor_wire selected)) eqn:wire_ok;
    [|discriminate].
  inversion matched; subst selected.
  eapply resolved_generated_field_is_member; exact resolved.
Qed.

Lemma matched_par_node_roles_match_typed_tags :
  forall node,
    Source.source_node_matches_generated_descriptor
      Source.generated_par_source_program node ->
    Source.source_par_component node =
      existsb (Nat.eqb
        (Schema.descriptor_tag (Source.source_field node)))
        Process.source_par_component_tags /\
    Source.source_par_connective_node node =
      (Schema.descriptor_tag (Source.source_field node) =? 10).
Proof.
  intros node [matched _].
  unfold Source.source_descriptor in matched.
  destruct (Schema.resolve_tag
      (Schema.program_fields Source.generated_par_source_program)
      (Source.Wire.span_number (Source.source_span node)))
    as [selected|] eqn:resolved; [|discriminate].
  destruct (Schema.wire_eqb
      (Source.wire_kind_of_span (Source.source_span node))
      (Schema.descriptor_wire selected)) eqn:wire_ok;
    [|discriminate].
  inversion matched; subst selected.
  pose proof (generated_par_descriptor_roles_match_typed_tags
    (Source.source_field node)
    (resolved_generated_field_is_member _ _ _ resolved))
    as [component connective].
  unfold Source.source_par_component,
    Source.source_par_connective_node.
  now split.
Qed.

Lemma matched_par_raw_keeps_descriptor_tag_and_body :
  forall bytes node raw,
    Source.source_node_matches_generated_descriptor
      Source.generated_par_source_program node ->
    Source.raw_field_of_source bytes node = Some raw ->
    Schema.raw_tag raw =
      Schema.descriptor_tag (Source.source_field node) /\
    Schema.raw_body raw = Source.source_body bytes node.
Proof.
  intros bytes node raw [matched _] converted.
  destruct (Source.raw_field_of_source_preserves_wire_and_body
    bytes node raw converted) as [raw_tag [_ [_ [raw_body _]]]].
  destruct (Source.resolved_source_descriptor_has_actual_tag_and_wire
    Source.generated_par_source_program (Source.source_span node)
    (Source.source_field node) matched) as [field_tag _].
  split; [now rewrite raw_tag, field_tag|exact raw_body].
Qed.

Lemma selected_source_evaluator_outputs_are_positional :
  forall (A B : Type) (evaluate : A -> option B) nodes results,
    Source.sequence_source_results (map evaluate nodes) = Some results ->
    Forall2 (fun node result => evaluate node = Some result)
      nodes results.
Proof.
  intros A B evaluate nodes.
  induction nodes as [|node rest IH]; intros results selected;
    simpl in selected.
  - inversion selected; constructor.
  - destruct (evaluate node) as [head|] eqn:evaluated;
      [|discriminate].
    destruct (Source.sequence_source_results (map evaluate rest))
      as [tail|] eqn:remaining; [|discriminate].
    inversion selected; subst results.
    constructor; [exact evaluated|].
    exact (IH tail eq_refl).
Qed.

Lemma source_evaluate_message_step_retains_raw :
  forall recurse hash_fn raw summary,
    Oracle.source_evaluate_message_step recurse hash_fn raw =
      Some summary ->
    Core.source_summary_raw summary = raw.
Proof.
  intros recurse hash_fn raw summary evaluated.
  unfold Oracle.source_evaluate_message_step in evaluated.
  destruct (Schema.raw_payload raw); try discriminate.
  destruct (Schema.raw_program_type_id raw) as [type_id|];
    [|discriminate].
  destruct (Schema.lookup_projected_program type_id [] [])
    as [program|]; [|discriminate].
  destruct (Source.decode_region
      (S (length (Schema.raw_body raw))) (Schema.raw_body raw)
      program 0 (length (Schema.raw_body raw))) as [nodes|];
    [|discriminate].
  destruct (Source.sequence_source_results
      (map (Oracle.source_evaluate_node recurse hash_fn
        (Schema.raw_body raw) program) nodes)) as [children|];
    [|discriminate].
  inversion evaluated; reflexivity.
Qed.

Lemma source_evaluate_message_retains_raw :
  forall fuel hash_fn raw summary,
    Oracle.source_evaluate_message fuel hash_fn raw = Some summary ->
    Core.source_summary_raw summary = raw.
Proof.
  intros [|fuel] hash_fn raw summary evaluated;
    [discriminate|].
  rewrite Oracle.source_evaluate_message_unfold in evaluated.
  eapply source_evaluate_message_step_retains_raw; exact evaluated.
Qed.

Lemma par_default_child_decoded_from_raw :
  forall bytes node raw summary,
    Source.source_node_matches_generated_descriptor
      Source.generated_par_source_program node ->
    Source.raw_field_of_source bytes node = Some raw ->
    Core.source_summary_raw summary = raw ->
    (Source.source_par_connective_node node = true ->
      Admission.source_typed_scalar Source.generated_par_source_program
        raw = Some (Core.source_summary_value summary)) ->
    par_default_child_decoded bytes node summary.
Proof.
  intros bytes node raw summary matched converted summary_raw
    decoded_connective.
  pose proof (matched_par_node_roles_match_typed_tags node matched)
    as [component connective].
  pose proof (matched_par_raw_keeps_descriptor_tag_and_body
    bytes node raw matched converted) as [raw_tag raw_body].
  unfold par_default_child_decoded.
  replace (Process.source_value_tag (Core.source_summary_value summary))
    with (Schema.descriptor_tag (Source.source_field node)).
  2: {
    unfold Process.source_value_tag.
    change (Schema.descriptor_tag (Source.source_field node) =
      Schema.raw_tag (Core.source_summary_raw summary)).
    now rewrite summary_raw, raw_tag.
  }
  split; [exact component|].
  split; [exact connective|].
  split.
  - unfold Core.source_summary_tag.
    now rewrite summary_raw.
  - split.
    + unfold Core.source_summary_body.
      now rewrite summary_raw.
    + intro selected.
      rewrite summary_raw.
      now apply decoded_connective.
Qed.

Lemma accepted_source_typed_scalar_retains_raw :
  forall program raw value,
    Admission.source_typed_scalar program raw = Some value ->
    Core.source_value_raw value = raw.
Proof.
  intros program raw value accepted.
  unfold Admission.source_typed_scalar in accepted.
  destruct (Admission.source_scalar_value_of_raw program raw)
    as [scalar|] eqn:decoded; [|discriminate].
  inversion accepted; reflexivity.
Qed.

Lemma evaluated_par_node_has_positional_default_evidence :
  forall fuel hash_fn bytes node summary,
    Source.source_node_matches_generated_descriptor
      Source.generated_par_source_program node ->
    Oracle.source_evaluate_node
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn bytes
      Source.generated_par_source_program node = Some summary ->
    par_default_child_decoded bytes node summary.
Proof.
  intros fuel hash_fn bytes node summary matched evaluated.
  pose proof (matched_par_node_field_is_member node matched)
    as field_member.
  unfold Oracle.source_evaluate_node in evaluated.
  destruct (Source.raw_field_of_source bytes node)
    as [raw|] eqn:converted; [|discriminate].
  destruct node as [span field|span field nested|span field];
    cbn [Source.source_field Source.source_span] in *.
  - destruct (Admission.source_typed_scalar
        Source.generated_par_source_program raw)
      as [value|] eqn:typed; [|discriminate].
    inversion evaluated; subst summary.
    eapply par_default_child_decoded_from_raw;
      [exact matched|exact converted| |].
    + exact (accepted_source_typed_scalar_retains_raw
        Source.generated_par_source_program raw value typed).
    + intros _. exact typed.
  - pose proof (source_evaluate_message_retains_raw
      fuel hash_fn raw summary evaluated) as retained.
    eapply par_default_child_decoded_from_raw;
      [exact matched|exact converted|exact retained|].
    intro connective.
    unfold Source.source_par_connective_node in connective.
    cbn [Source.source_field] in connective.
    apply andb_true_iff in connective as [tag_ok _].
    apply Nat.eqb_eq in tag_ok.
    pose proof (generated_par_descriptor_tag10_is_scalar
      field field_member tag_ok) as scalar.
    unfold Source.raw_field_of_source in converted.
    cbn [Source.source_span Source.source_field] in converted.
    rewrite scalar in converted.
    simpl in converted. discriminate.
  - destruct (generated_par_descriptor_payload_is_plain
      field field_member) as [scalar|message];
      unfold Source.raw_field_of_source in converted;
      cbn [Source.source_span Source.source_field] in converted;
      [rewrite scalar in converted|rewrite message in converted];
      simpl in converted; discriminate.
Qed.

Lemma matched_selected_par_children_are_positional :
  forall fuel hash_fn bytes nodes summaries,
    Forall (Source.source_node_matches_generated_descriptor
      Source.generated_par_source_program) nodes ->
    Forall2 (fun node summary =>
      Oracle.source_evaluate_node
        (Oracle.source_evaluate_message fuel hash_fn) hash_fn bytes
        Source.generated_par_source_program node = Some summary)
      nodes summaries ->
    Forall2 (par_default_child_decoded bytes) nodes summaries.
Proof.
  intros fuel hash_fn bytes nodes summaries generated selected.
  revert generated.
  induction selected as [|node summary nodes summaries
      evaluated selected IH]; intro generated; [constructor|].
  inversion generated as [|? ? matched remaining]; subst.
  constructor.
  - eapply evaluated_par_node_has_positional_default_evidence;
      eassumption.
  - exact (IH remaining).
Qed.

Theorem selected_independent_par_children_are_positional :
  forall fuel hash_fn bytes nodes summaries,
    Source.decode_region (S (length bytes)) bytes
      Source.generated_par_source_program 0 (length bytes) = Some nodes ->
    Source.sequence_source_results
      (map (Oracle.source_evaluate_node
        (Oracle.source_evaluate_message fuel hash_fn) hash_fn bytes
        Source.generated_par_source_program) nodes) = Some summaries ->
    Forall2 (par_default_child_decoded bytes) nodes summaries.
Proof.
  intros fuel hash_fn bytes nodes summaries decoded selected.
  pose proof (Source.decoded_region_nodes_match_generated_source
    (S (length bytes)) bytes Source.generated_par_source_program
    0 (length bytes) nodes decoded) as generated.
  pose proof (selected_source_evaluator_outputs_are_positional
    Source.source_tree Core.source_exact_summary
    (Oracle.source_evaluate_node
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn bytes
      Source.generated_par_source_program)
    nodes summaries selected) as positional.
  eapply matched_selected_par_children_are_positional;
    [exact generated|exact positional].
Qed.

Theorem accepted_independent_par_root_has_positional_children :
  forall fuel hash_fn bytes summary,
    Oracle.source_evaluate_message (S fuel) hash_fn
      (Oracle.source_par_raw bytes) = Some summary ->
    exists nodes children,
      Source.decode_root 1 bytes = Some nodes /\
      summary = Oracle.source_message_summary
        (Oracle.source_par_raw bytes)
        Source.generated_par_source_program children /\
      Forall2 (par_default_child_decoded bytes) nodes children.
Proof.
  intros fuel hash_fn bytes summary evaluated.
  rewrite Oracle.source_evaluate_message_unfold in evaluated.
  unfold Oracle.source_evaluate_message_step in evaluated.
  cbn [Oracle.source_par_raw Schema.raw_payload
    Schema.raw_program_type_id Schema.raw_body] in evaluated.
  rewrite Source.generated_par_source_lookup_is_present in evaluated.
  destruct (Source.decode_region (S (length bytes)) bytes
      Source.generated_par_source_program 0 (length bytes))
    as [nodes|] eqn:decoded; [|discriminate].
  destruct (Source.sequence_source_results
      (map (Oracle.source_evaluate_node
        (Oracle.source_evaluate_message fuel hash_fn) hash_fn bytes
        Source.generated_par_source_program) nodes))
    as [children|] eqn:selected; [|discriminate].
  inversion evaluated; subst summary.
  exists nodes, children.
  split.
  - unfold Source.decode_root.
    rewrite Source.generated_par_source_lookup_is_present.
    exact decoded.
  - split; [reflexivity|].
    eapply selected_independent_par_children_are_positional;
      eassumption.
Qed.

Corollary accepted_independent_par_root_default_matches_source :
  forall fuel hash_fn bytes summary,
    Oracle.source_evaluate_message (S fuel) hash_fn
      (Oracle.source_par_raw bytes) = Some summary ->
    exists nodes,
      Source.decode_root 1 bytes = Some nodes /\
      Core.source_summary_semantic_default summary =
        Source.source_par_semantic_default bytes nodes.
Proof.
  intros fuel hash_fn bytes summary evaluated.
  destruct (accepted_independent_par_root_has_positional_children
    fuel hash_fn bytes summary evaluated)
    as [nodes [children [decoded [shape aligned]]]].
  exists nodes. split; [exact decoded|].
  subst summary.
  unfold Oracle.source_message_summary.
  change (Schema.program_family Source.generated_par_source_program)
    with 1.
  cbn [Core.source_summary_semantic_default Nat.eqb].
  apply positional_par_root_default_agrees; [reflexivity|exact aligned].
Qed.

(** This compares the two accepted root verdicts using their separately
    established source defaults.  Acceptance equivalence and semantic-score
    projection require the remaining recursive family bridge. *)
Theorem jointly_accepted_par_roots_agree_on_default :
  forall hash_fn bytes source_default source_summary
      target_default target_summary,
    Oracle.source_par_reference hash_fn bytes =
      Some (source_default, source_summary) ->
    Recursive.recursive_par_reference hash_fn bytes =
      Some (target_default, target_summary) ->
    source_default = target_default.
Proof.
  intros hash_fn bytes source_default source_summary target_default
    target_summary source_accepted target_accepted.
  unfold Oracle.source_par_reference in source_accepted.
  destruct (Oracle.source_evaluate_message (S (length bytes)) hash_fn
      (Oracle.source_par_raw bytes)) as [summary|] eqn:evaluated;
    [|discriminate].
  destruct (Core.source_summary_refused summary) eqn:refused;
    [discriminate|].
  destruct (Core.source_summary_semantic_default summary)
    as [observed|] eqn:source_selected; [|discriminate].
  inversion source_accepted; subst source_default source_summary.
  destruct (accepted_independent_par_root_default_matches_source
      (length bytes) hash_fn bytes summary evaluated)
    as [nodes [decoded source_default]].
  destruct (Recursive.accepted_recursive_par_has_original_body_and_source_default
      hash_fn bytes target_default target_summary target_accepted)
    as [program [target_nodes [_ [target_decoded [target_default_eq _]]]]].
  rewrite decoded in target_decoded.
  inversion target_decoded; subst target_nodes.
  rewrite source_selected in source_default.
  rewrite target_default_eq in source_default.
  now inversion source_default.
Qed.

Print Assumptions source_target_child_verdict_projects.
Print Assumptions source_sequence_projects.
Print Assumptions recursive_source_children_as_sequence.
Print Assumptions source_target_child_lists_project.
Print Assumptions source_unrejected_results_accepts.
Print Assumptions source_unrejected_results_recover.
Print Assumptions accepted_source_child_list_projects_to_recursive.
Print Assumptions accepted_recursive_child_list_recovers_source.
Print Assumptions generated_par_connective_source_contract.
Print Assumptions accepted_one_byte_par_connective_is_true.
Print Assumptions paired_par_component_presence.
Print Assumptions paired_par_connective_find.
Print Assumptions find_projected_source_values.
Print Assumptions par_component_count_zero_is_absence.
Print Assumptions decoded_par_connective_used_agrees.
Print Assumptions paired_par_connective_used.
Print Assumptions existsb_projected_source_values.
Print Assumptions typed_par_default_connective_algebra.
Print Assumptions typed_par_default_via_connective.
Print Assumptions positional_par_root_default_agrees.
Print Assumptions generated_par_descriptor_roles_match_typed_tags.
Print Assumptions generated_par_descriptor_payload_is_plain.
Print Assumptions generated_par_descriptor_tag10_is_scalar.
Print Assumptions resolved_generated_field_is_member.
Print Assumptions matched_par_node_field_is_member.
Print Assumptions matched_par_node_roles_match_typed_tags.
Print Assumptions matched_par_raw_keeps_descriptor_tag_and_body.
Print Assumptions selected_source_evaluator_outputs_are_positional.
Print Assumptions source_evaluate_message_step_retains_raw.
Print Assumptions source_evaluate_message_retains_raw.
Print Assumptions par_default_child_decoded_from_raw.
Print Assumptions accepted_source_typed_scalar_retains_raw.
Print Assumptions evaluated_par_node_has_positional_default_evidence.
Print Assumptions matched_selected_par_children_are_positional.
Print Assumptions selected_independent_par_children_are_positional.
Print Assumptions accepted_independent_par_root_has_positional_children.
Print Assumptions accepted_independent_par_root_default_matches_source.
Print Assumptions jointly_accepted_par_roots_agree_on_default.
