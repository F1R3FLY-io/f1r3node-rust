From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement
  RPB3RawRegionTree RPB3SemanticReducer RPB3IndependentOracleCore
  RPB3IndependentOracleName RPB3IndependentOracleProjection
  RPB3IndependentOracleRecursive RPB3IndependentOracleRecursiveProjection
  RPB3RecursiveAuxiliarySourceReference RPB3IndependentOracleParAnswer
  RPB3IndependentOracleRegionRebaseBridge
  RPB3IndependentOracleRegionChildBridge
  RPB3IndependentOracleStringMapTypedRebaseBridge.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Semantic := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Name := RPB3IndependentOracleName.
Module Projection := RPB3IndependentOracleProjection.
Module Oracle := RPB3IndependentOracleRecursive.
Module RecursiveProjection := RPB3IndependentOracleRecursiveProjection.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.
Module ParAnswer := RPB3IndependentOracleParAnswer.
Module Rebase := RPB3IndependentOracleRegionRebaseBridge.
Module RegionChild := RPB3IndependentOracleRegionChildBridge.
Module TypedRebase := RPB3IndependentOracleStringMapTypedRebaseBridge.

(** Oracle/target verdict for a generated StringMap node of a decoded region.

    The oracle reads the node through [Oracle.source_evaluate_node] and the
    target through [Recursive.recursive_source_reference].  They agree because

    - the key gates differ only by the oracle's byte-range check, which the
      enclosing [region_ok] window discharges, and by the scan form of the
      UTF-8 test, which [Source.indexed_scan_utf8_accepts_exactly_compact_utf8]
      equates with the compact form;
    - the target's [par_child] is, on every strictly shorter body, the
      projection of the oracle's scored Par answer, so the oracle's nested
      selection and the target's [reduce_map_par] keep the same values.

    The definitions below name both node results over abstract key and value
    sources, so each gate is analysed once over variables instead of inside
    the unfolded evaluators. *)

(** The oracle's nested value selection, over the value's evaluation. *)
Definition oracle_value_selection (evaluation : option Core.source_exact_summary)
    : option (option (bool * Core.source_exact_summary)) :=
  match evaluation with
  | Some summary =>
      match Core.source_summary_semantic_default summary with
      | Some false =>
          if Core.source_summary_refused summary then None
          else Some (Some (false, summary))
      | _ => None
      end
  | None => None
  end.

(** The oracle's result once the key has been accepted. *)
Definition oracle_keyed_result (raw : Schema.raw_field) (key : list nat)
    (selection : option (option (bool * Core.source_exact_summary)))
    : option Core.source_exact_summary :=
  match selection with
  | None => None
  | Some selected =>
      match Name.source_string_map_auxiliary_score key selected with
      | None => None
      | Some score =>
          Some (Core.SourceExactSummary
            (Core.TypedSourceStringMap raw key
              (option_map (fun paired =>
                (fst paired, Core.source_summary_value (snd paired))) selected))
            (Core.SourceFamilyAnalysis (Some true) (Some score)
              false false false) None)
      end
  end.

(** The oracle's StringMap result over abstract key and value sources. *)
Definition oracle_sources_result
    (recurse : Schema.raw_field -> option Core.source_exact_summary)
    (raw : Schema.raw_field)
    (key_source value_source : option (Source.Wire.field_span * list nat))
    : option Core.source_exact_summary :=
  match Oracle.source_valid_string_map_key key_source with
  | None => None
  | Some key =>
      oracle_keyed_result raw key
        (match value_source with
         | None => Some None
         | Some (_, value_body) =>
             oracle_value_selection
               (recurse (Oracle.source_par_raw value_body))
         end)
  end.

(** The target's answer for a reduced map entry. *)
Definition target_keyed_answer (raw : Schema.raw_field)
    (returned : Source.typed_map_return Semantic.semantic_summary)
    : option Recursive.recursive_mixed_result :=
  option_map (Recursive.RecursiveAuxiliaryResult
      (Recursive.RecursiveStringMapResult raw returned))
    (Recursive.recursive_auxiliary_semantic
      (Recursive.RecursiveStringMapResult raw returned)).

(** The target's result once the key has been accepted. *)
Definition target_keyed_result (raw : Schema.raw_field) (key : list nat)
    (key_span : option Source.Wire.field_span)
    (outcome : option (option Semantic.semantic_summary *
      option Source.Wire.field_span))
    : option Recursive.recursive_mixed_result :=
  match
    match outcome with
    | Some (value, value_span) =>
        Some (Source.TypedMapReturn Semantic.semantic_summary key key_span
          value value_span)
    | None => None
    end
  with
  | Some returned => target_keyed_answer raw returned
  | None => None
  end.

(** The target's StringMap result over abstract key and value sources. *)
Definition target_sources_result
    (par_child : list nat -> option (bool * Semantic.semantic_summary))
    (raw : Schema.raw_field)
    (key_source value_source : option (Source.Wire.field_span * list nat))
    : option Recursive.recursive_mixed_result :=
  match
    match Source.reduce_map_key key_source,
        Source.reduce_map_par par_child value_source with
    | Some (key, key_span), Some (value, value_span) =>
        Some (Source.TypedMapReturn Semantic.semantic_summary key key_span
          value value_span)
    | _, _ => None
    end
  with
  | Some returned => target_keyed_answer raw returned
  | None => None
  end.

(** On a source whose key bytes are in range the two key gates coincide. *)
Lemma string_map_key_gates_agree :
  forall source : option (Source.Wire.field_span * list nat),
    (forall span key, source = Some (span, key) ->
      Core.bytes_in_range key = true) ->
    Oracle.source_valid_string_map_key source =
      option_map fst (Source.reduce_map_key source).
Proof.
  intros [[span key]|] in_range.
  - pose proof (in_range span key eq_refl) as in_bytes.
    unfold Oracle.source_valid_string_map_key, Source.reduce_map_key.
    rewrite in_bytes, Source.indexed_scan_utf8_accepts_exactly_compact_utf8.
    destruct (Source.utf8_bytes key); destruct key; reflexivity.
  - reflexivity.
Qed.

(** A refined entry's key lies inside the outer payload window, which
    [region_ok] has already checked byte by byte. *)
Lemma refined_string_map_key_is_in_byte_range :
  forall bytes program outer field typed key_span key,
    Source.refine_string_map_entry bytes program
      (Source.SourceAuxiliary outer field) = Some typed ->
    Source.typed_map_key_source typed = Some (key_span, key) ->
    Core.bytes_in_range key = true.
Proof.
  intros bytes program outer field typed key_span key refined source_key.
  unfold Source.refine_string_map_entry in refined.
  destruct (Source.decode_string_map_entry_source bytes program
    (Source.SourceAuxiliary outer field)) as [entry|] eqn:decoded;
    [|discriminate].
  destruct (Schema.lookup_projected_program
    (Source.map_value_type_id entry) [] []) as [value_program|];
    [|discriminate].
  inversion refined; subst typed.
  simpl in source_key.
  destruct (Source.map_key_span entry) as [selected|] eqn:key_present;
    [|discriminate].
  inversion source_key; subst key_span key.
  destruct (TypedRebase.decoded_string_map_selected_spans_are_bounded
    bytes program outer field entry selected decoded (or_introl key_present))
    as [key_after [key_ordered key_inside]].
  destruct (Source.decoded_string_map_entry_has_actual_constructor_shape
    bytes program (Source.SourceAuxiliary outer field) entry decoded)
    as [actual_outer [carried [entry_spans [node_shape
      [_ [_ [_ [_ [framed _]]]]]]]]].
  inversion node_shape; subst actual_outer carried.
  assert (guard : Source.Wire.region_ok bytes
      (Source.Wire.span_payload_start outer)
      (Source.Wire.span_end outer) = true).
  { unfold Source.Wire.worklist_region in framed.
    destruct (Source.Wire.region_ok bytes
      (Source.Wire.span_payload_start outer)
      (Source.Wire.span_end outer)) eqn:region_checked;
      [reflexivity|discriminate]. }
  unfold Source.Wire.region_ok in guard.
  apply andb_true_iff in guard as [_ window_guard].
  change (forallb Source.Wire.byte_ok (Source.map_span_body bytes selected)
    = true).
  rewrite forallb_forall in window_guard |- *.
  intros byte member.
  apply window_guard.
  change (Source.map_span_body bytes selected) with
    (Rebase.region_slice bytes (Source.Wire.span_payload_start selected)
      (Source.Wire.span_end selected)) in member.
  rewrite <- (Rebase.region_subslice_rebases bytes
    (Source.Wire.span_payload_start outer) (Source.Wire.span_end outer)
    (Source.Wire.span_payload_start selected)
    (Source.Wire.span_end selected)) in member by lia.
  apply Source.firstn_member_is_original in member.
  apply Source.skipn_member_is_original in member.
  exact member.
Qed.

(** Refinement re-selects the outer descriptor, so on a node carrying its
    generated descriptor the entry's field is the node's own field. *)
Lemma refined_string_map_entry_keeps_node_field :
  forall bytes program span field typed,
    Source.source_descriptor program span = Some field ->
    Source.refine_string_map_entry bytes program
      (Source.SourceAuxiliary span field) = Some typed ->
    Source.typed_map_outer_field typed = field.
Proof.
  intros bytes program span field typed selected refined.
  destruct (Source.typed_map_entry_refines_source_selection bytes program
    (Source.SourceAuxiliary span field) typed refined)
    as [entry [decoded [same_span [_ [_ [_ [descriptor _]]]]]]].
  destruct (Source.decoded_string_map_entry_has_actual_constructor_shape
    bytes program (Source.SourceAuxiliary span field) entry decoded)
    as [actual_span [carried [entry_spans [node_shape [entry_span _]]]]].
  assert (same_outer : actual_span = span) by congruence.
  rewrite same_span, entry_span, same_outer, selected in descriptor.
  injection descriptor as same.
  exact (eq_sym same).
Qed.

(** The slot test of [reduce_source_map_node] passes on a refined entry whose
    outer field is the node's own field. *)
Lemma reduce_source_map_node_of_refined :
  forall (A : Type) bytes program span field
      (reduce_par : list nat -> option (bool * A)) typed,
    Source.refine_string_map_entry bytes program
      (Source.SourceAuxiliary span field) = Some typed ->
    Source.typed_map_outer_field typed = field ->
    Source.reduce_source_map_node bytes program
      (Schema.descriptor_slot field) reduce_par
      (Source.SourceAuxiliary span field) =
      Source.reduce_typed_map_entry reduce_par typed.
Proof.
  intros A bytes program span field reduce_par typed refined outer.
  unfold Source.reduce_source_map_node.
  rewrite refined, outer, Nat.eqb_refl.
  reflexivity.
Qed.

(** The oracle's StringMap arm in terms of [oracle_sources_result]. *)
Lemma oracle_string_map_node_unfold :
  forall recurse hash_fn bytes program span field,
    Schema.descriptor_payload field = Schema.StringMap ->
    Oracle.source_evaluate_node recurse hash_fn bytes program
      (Source.SourceAuxiliary span field) =
    match Source.raw_field_of_source bytes
        (Source.SourceAuxiliary span field) with
    | None => None
    | Some raw =>
        match Source.refine_string_map_entry bytes program
            (Source.SourceAuxiliary span field) with
        | None => None
        | Some entry =>
            oracle_sources_result recurse raw
              (Source.typed_map_key_source entry)
              (Source.typed_map_value_source entry)
        end
    end.
Proof.
  intros recurse hash_fn bytes program span field payload.
  unfold Oracle.source_evaluate_node.
  destruct (Source.raw_field_of_source bytes
    (Source.SourceAuxiliary span field)) as [raw|]; [|reflexivity].
  rewrite payload.
  destruct (Source.refine_string_map_entry bytes program
    (Source.SourceAuxiliary span field)) as [entry|]; [|reflexivity].
  reflexivity.
Qed.

(** The target's StringMap arm in terms of [target_sources_result]. *)
Lemma target_string_map_node_unfold :
  forall par_child hash_fn bytes program span field,
    Schema.descriptor_payload field = Schema.StringMap ->
    Source.source_descriptor program span = Some field ->
    Recursive.recursive_source_reference par_child hash_fn bytes program
      (Source.SourceAuxiliary span field) =
    match Recursive.checked_generated_raw bytes program
        (Source.SourceAuxiliary span field) with
    | None => None
    | Some raw =>
        match Source.refine_string_map_entry bytes program
            (Source.SourceAuxiliary span field) with
        | None => None
        | Some typed =>
            target_sources_result par_child raw
              (Source.typed_map_key_source typed)
              (Source.typed_map_value_source typed)
        end
    end.
Proof.
  intros par_child hash_fn bytes program span field payload selected.
  cbn [Recursive.recursive_source_reference].
  unfold Recursive.recursive_auxiliary_reference.
  destruct (Recursive.checked_generated_raw bytes program
    (Source.SourceAuxiliary span field)) as [raw|]; [|reflexivity].
  rewrite Nat.eqb_refl, payload.
  destruct (Source.refine_string_map_entry bytes program
    (Source.SourceAuxiliary span field)) as [typed|] eqn:refined.
  - rewrite (reduce_source_map_node_of_refined _ bytes program span field
      par_child typed refined
      (refined_string_map_entry_keeps_node_field bytes program span field
        typed selected refined)).
    unfold Source.reduce_typed_map_entry, target_sources_result,
      target_keyed_answer.
    destruct (Source.reduce_map_key (Source.typed_map_key_source typed))
      as [[key key_span]|]; [|reflexivity].
    destruct (Source.reduce_map_par par_child
      (Source.typed_map_value_source typed))
      as [[value value_span]|]; reflexivity.
  - unfold Source.reduce_source_map_node.
    rewrite refined.
    reflexivity.
Qed.

(** An unrefused value with an exact score yields the same accepted summary on
    both sides. *)
Lemma accepted_keyed_verdict :
  forall (raw : Schema.raw_field) (key : list nat)
      (key_span : option Source.Wire.field_span)
      (value_span : Source.Wire.field_span)
      (summary : Core.source_exact_summary)
      (value_score : Core.source_score),
    Core.source_summary_refused summary = false ->
    Core.source_summary_exact_score summary = Some value_score ->
    RecursiveProjection.source_target_child_verdict
      (oracle_keyed_result raw key (Some (Some (false, summary))))
      (target_keyed_answer raw
        (Source.TypedMapReturn Semantic.semantic_summary key key_span
          (Some (Projection.project_source_summary summary))
          (Some value_span))).
Proof.
  intros raw key key_span value_span summary value_score not_refused scored.
  assert (auxiliary :
    Name.source_string_map_auxiliary_score key (Some (false, summary)) =
    Some (Core.SourceNode (Core.source_score_forest_of_list
      [Core.SourceString key; value_score]))).
  { unfold Name.source_string_map_auxiliary_score.
    rewrite not_refused, scored. reflexivity. }
  assert (evidence :
    Semantic.sem_score_evidence (Projection.project_source_summary summary) =
    Semantic.ExactScore (Core.project_score value_score)).
  { change (Projection.project_source_evidence
      (Core.source_summary_exact_score summary) =
      Semantic.ExactScore (Core.project_score value_score)).
    rewrite scored. reflexivity. }
  unfold oracle_keyed_result, target_keyed_answer,
    Recursive.recursive_auxiliary_semantic.
  cbn [Recursive.recursive_auxiliary_raw Source.returned_map_par
    Source.returned_map_key].
  rewrite auxiliary, evidence.
  split; reflexivity.
Qed.

(** An unrefused value without an exact score is rejected by the oracle's
    auxiliary score, matching the target's rejection of the scored Par. *)
Lemma unscored_keyed_verdict :
  forall (raw : Schema.raw_field) (key : list nat)
      (summary : Core.source_exact_summary),
    Core.source_summary_refused summary = false ->
    Core.source_summary_exact_score summary = None ->
    RecursiveProjection.source_target_child_verdict
      (oracle_keyed_result raw key (Some (Some (false, summary)))) None.
Proof.
  intros raw key summary not_refused missing.
  assert (auxiliary :
    Name.source_string_map_auxiliary_score key (Some (false, summary)) = None).
  { unfold Name.source_string_map_auxiliary_score.
    rewrite not_refused, missing. reflexivity. }
  unfold oracle_keyed_result.
  rewrite auxiliary.
  exact I.
Qed.

(** An absent value scores as the empty Par on both sides. *)
Lemma keyed_absent_value_verdict :
  forall (raw : Schema.raw_field) (key : list nat)
      (key_span : option Source.Wire.field_span),
    RecursiveProjection.source_target_child_verdict
      (oracle_keyed_result raw key (Some None))
      (target_keyed_result raw key key_span (Some (None, None))).
Proof.
  intros raw key key_span.
  split; reflexivity.
Qed.

(** The value gates agree for an arbitrary evaluation, given the Par answer
    that the projection of the scored result determines. *)
Lemma keyed_value_gates_agree :
  forall (raw : Schema.raw_field) (key : list nat)
      (key_span : option Source.Wire.field_span)
      (value_span : Source.Wire.field_span)
      (evaluation : option Core.source_exact_summary)
      (par_answer : option (bool * Semantic.semantic_summary)),
    par_answer = option_map ParAnswer.project_par_answer
      (match evaluation with
       | Some summary => ParAnswer.source_scored_par_result summary
       | None => None
       end) ->
    RecursiveProjection.source_target_child_verdict
      (oracle_keyed_result raw key (oracle_value_selection evaluation))
      (target_keyed_result raw key key_span
        (match par_answer with
         | Some (false, value) => Some (Some value, Some value_span)
         | _ => None
         end)).
Proof.
  intros raw key key_span value_span evaluation par_answer gate.
  subst par_answer.
  destruct evaluation as [summary|]; [|exact I].
  unfold ParAnswer.source_scored_par_result, oracle_value_selection.
  destruct (Core.source_summary_semantic_default summary) as [[|]|].
  - destruct (Core.source_summary_refused summary);
      [|destruct (Core.source_summary_exact_score summary)]; exact I.
  - destruct (Core.source_summary_refused summary) eqn:refused; [exact I|].
    destruct (Core.source_summary_exact_score summary)
      as [value_score|] eqn:scored.
    + exact (accepted_keyed_verdict raw key key_span value_span summary
        value_score refused scored).
    + exact (unscored_keyed_verdict raw key summary refused scored).
  - destruct (Core.source_summary_refused summary);
      [|destruct (Core.source_summary_exact_score summary)]; exact I.
Qed.

(** The verdict over abstract sources, from the key and value gate facts. *)
Lemma string_map_sources_verdict :
  forall (recurse : Schema.raw_field -> option Core.source_exact_summary)
      (par_child : list nat -> option (bool * Semantic.semantic_summary))
      (raw : Schema.raw_field)
      (key_source value_source : option (Source.Wire.field_span * list nat)),
    (forall span key, key_source = Some (span, key) ->
      Core.bytes_in_range key = true) ->
    (forall value_span value_body,
      value_source = Some (value_span, value_body) ->
      par_child value_body =
        option_map ParAnswer.project_par_answer
          (match recurse (Oracle.source_par_raw value_body) with
           | Some summary => ParAnswer.source_scored_par_result summary
           | None => None
           end)) ->
    RecursiveProjection.source_target_child_verdict
      (oracle_sources_result recurse raw key_source value_source)
      (target_sources_result par_child raw key_source value_source).
Proof.
  intros recurse par_child raw key_source value_source key_in_range value_gate.
  unfold oracle_sources_result, target_sources_result.
  rewrite (string_map_key_gates_agree key_source key_in_range).
  destruct (Source.reduce_map_key key_source) as [[key key_span]|];
    [|exact I].
  destruct value_source as [[value_span value_body]|].
  - specialize (value_gate value_span value_body eq_refl).
    exact (keyed_value_gates_agree raw key key_span value_span
      (recurse (Oracle.source_par_raw value_body)) (par_child value_body)
      value_gate).
  - exact (keyed_absent_value_verdict raw key key_span).
Qed.

Theorem generated_string_map_node_verdict :
  forall recurse par_child hash_fn bytes program fuel start finish nodes span field,
    (forall value, length value < length bytes ->
       recurse (Oracle.source_par_raw value) =
       Oracle.source_evaluate_message (S (length value)) hash_fn
         (Oracle.source_par_raw value)) ->
    (forall value, length value < length bytes ->
       par_child value = Recursive.recursive_par_reference hash_fn value) ->
    (forall value, length value < length bytes ->
       Recursive.recursive_par_reference hash_fn value =
       option_map ParAnswer.project_par_answer
         (ParAnswer.oracle_scored_par_reference hash_fn value)) ->
    Source.decode_region fuel bytes program start finish = Some nodes ->
    In (Source.SourceAuxiliary span field) nodes ->
    Schema.descriptor_payload field = Schema.StringMap ->
    RecursiveProjection.source_target_child_verdict
      (Oracle.source_evaluate_node recurse hash_fn bytes program
        (Source.SourceAuxiliary span field))
      (Recursive.recursive_source_reference par_child hash_fn bytes program
        (Source.SourceAuxiliary span field)).
Proof.
  intros recurse par_child hash_fn bytes program fuel start finish nodes span
    field recurse_is_evaluation par_child_is_reference par_induction decoded
    member payload.
  destruct (Source.source_forall_member _ _ _ _
    (Source.decoded_region_nodes_match_generated_source
      fuel bytes program start finish nodes decoded) member) as [selected _].
  cbn [Source.source_span Source.source_field] in selected.
  pose proof (RegionChild.decoded_node_checked_raw_agrees fuel bytes program
    start finish nodes (Source.SourceAuxiliary span field) decoded member)
    as checked.
  rewrite (oracle_string_map_node_unfold recurse hash_fn bytes program span
    field payload).
  rewrite (target_string_map_node_unfold par_child hash_fn bytes program span
    field payload selected).
  rewrite checked.
  clear checked.
  destruct (Source.raw_field_of_source bytes
    (Source.SourceAuxiliary span field)) as [raw|]; [|exact I].
  destruct (Source.refine_string_map_entry bytes program
    (Source.SourceAuxiliary span field)) as [typed|] eqn:refined; [|exact I].
  refine (string_map_sources_verdict recurse par_child raw
    (Source.typed_map_key_source typed)
    (Source.typed_map_value_source typed) _ _).
  - intros key_span key source_key.
    exact (refined_string_map_key_is_in_byte_range bytes program span field
      typed key_span key refined source_key).
  - intros value_span value_body present.
    destruct (Recursive.refined_string_map_value_satisfies_recursive_length_guard
      bytes program span field typed value_span value_body
      (fun _ _ => None) refined present) as [shorter _].
    unfold Recursive.shorter_body in shorter.
    rewrite (par_child_is_reference value_body shorter).
    rewrite (par_induction value_body shorter).
    unfold ParAnswer.oracle_scored_par_reference.
    rewrite <- (recurse_is_evaluation value_body shorter).
    reflexivity.
Qed.

Print Assumptions string_map_key_gates_agree.
Print Assumptions refined_string_map_key_is_in_byte_range.
Print Assumptions refined_string_map_entry_keeps_node_field.
Print Assumptions reduce_source_map_node_of_refined.
Print Assumptions oracle_string_map_node_unfold.
Print Assumptions target_string_map_node_unfold.
Print Assumptions accepted_keyed_verdict.
Print Assumptions unscored_keyed_verdict.
Print Assumptions keyed_absent_value_verdict.
Print Assumptions keyed_value_gates_agree.
Print Assumptions string_map_sources_verdict.
Print Assumptions generated_string_map_node_verdict.
