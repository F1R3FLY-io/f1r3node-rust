From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import EPM1 EPM1ACTree03
  RPB3GeneratedFieldRefinement RPB3RawRegionTree RPB3SemanticReducer
  RPB3IndependentOracleCore RPB3IndependentOracleName
  RPB3IndependentOracleNameProjection RPB3IndependentOracleProjection
  RPB3IndependentOracleRecursive RPB3IndependentOracleRecursiveProjection
  RPB3RecursiveAuxiliarySourceReference RPB3IndependentOracleParAnswer
  RPB3IndependentOracleRegionChildBridge.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Semantic := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Name := RPB3IndependentOracleName.
Module NameProjection := RPB3IndependentOracleNameProjection.
Module Projection := RPB3IndependentOracleProjection.
Module Oracle := RPB3IndependentOracleRecursive.
Module RecursiveProjection := RPB3IndependentOracleRecursiveProjection.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.
Module ParAnswer := RPB3IndependentOracleParAnswer.
Module RegionChild := RPB3IndependentOracleRegionChildBridge.

(** Verdict of one decoded PathMap auxiliary node.  The independent oracle and
    the recursive target are compared through [source_target_child_verdict].
    The two value hooks stay premises here, because the assembly discharges
    them later: fuel saturation of the oracle's message recursion, and the
    target's length-guarded Par child.  The third premise is the Par induction
    hypothesis at every strictly shorter body.

    The oracle's PathMap value arm reads [Some semantic_default, Some _]: a
    value is selected whenever it is scored and unrefused, whatever its
    semantic default.  The case split in [generated_pathmap_node_verdict] is
    made on that default so the [Some true] case carries the rule: it equates
    the arm's [Some (true, summary)] with the scored Par reference, and it
    could not be closed under the former [Some false] reading of the arm. *)

(** The summary built by the PathMap arm of [Oracle.source_evaluate_node] when
    every value body is accepted.  Its analysis does not depend on the values:
    the auxiliary score is the raw snapshot bytes. *)
Definition oracle_pathmap_accepted_summary
    (raw : Schema.raw_field) (snapshot : EPM1.epm1_snapshot)
    (values : list (bool * Core.source_exact_summary))
    : Core.source_exact_summary :=
  Core.SourceExactSummary
    (Core.TypedSourcePathMap raw snapshot
      (map (fun paired =>
        (fst paired, Core.source_summary_value (snd paired))) values))
    (Core.SourceFamilyAnalysis (Some true)
      (Some (Name.source_pathmap_auxiliary_score raw))
      false false false) None.

(** Each value body of a physically decoded snapshot is strictly shorter than
    the enclosing buffer.  The guard theorem states this for the target's
    length-guarded Par child; [shorter_body] unfolds to the length bound. *)
Lemma decoded_pathmap_value_is_strictly_shorter :
  forall parent node hash_fn snapshot trie (value : list nat),
    EPM1ACTree03.decode_epm1_actree03 hash_fn
      (Source.source_body parent node) = Some (snapshot, trie) ->
    In value (EPM1.value_bodies snapshot) ->
    length value < length parent.
Proof.
  intros parent node hash_fn snapshot trie value physical member.
  destruct (Recursive.decoded_pathmap_values_satisfy_recursive_length_guard
    parent node hash_fn snapshot trie value (fun _ _ => None) physical member)
    as [shorter _].
  exact shorter.
Qed.

(** Pointwise agreement of the selected values lifts to the two ordered
    sequences, by the sequence projection lemma of the recursive projection
    theory. *)
Lemma sequence_source_results_projects_pointwise :
  forall (select : list nat -> option (bool * Core.source_exact_summary))
      (par_child : list nat -> option (bool * Semantic.semantic_summary))
      (bodies : list (list nat)),
    (forall value, In value bodies ->
      par_child value =
        option_map ParAnswer.project_par_answer (select value)) ->
    Source.sequence_source_results (map par_child bodies) =
    option_map (map ParAnswer.project_par_answer)
      (Source.sequence_source_results (map select bodies)).
Proof.
  intros select par_child bodies pointwise.
  rewrite RecursiveProjection.source_sequence_projects.
  rewrite map_map.
  f_equal.
  apply map_ext_in.
  exact pointwise.
Qed.

(** The accepted PathMap summary is unrefused, and its projection is the
    semantic summary the recursive target computes for the auxiliary result:
    the target's record is built from the raw field alone, exactly as the
    projection rebuilds it from the oracle's summary. *)
Lemma accepted_pathmap_summary_projects_to_target_semantic :
  forall raw snapshot trie recursive_values values,
    Core.source_summary_refused
      (oracle_pathmap_accepted_summary raw snapshot values) = false /\
    Recursive.recursive_auxiliary_semantic
      (Recursive.RecursivePathMapResult raw snapshot trie recursive_values) =
    Some (Projection.project_source_summary
      (oracle_pathmap_accepted_summary raw snapshot values)).
Proof.
  intros raw snapshot trie recursive_values values.
  destruct (NameProjection.projected_pathmap_auxiliary_summary raw snapshot
      (map (fun paired =>
        (fst paired, Core.source_summary_value (snd paired))) values)
      (oracle_pathmap_accepted_summary raw snapshot values) eq_refl eq_refl)
    as [_ [_ [_ [not_refused _]]]].
  split; [exact not_refused|reflexivity].
Qed.

(** The target's PathMap node reference with its gates exposed: the checked
    generated raw field, the physical decode, and the ordered value sequence.
    The slot comparison is reflexive and the payload selects the PathMap arm. *)
Lemma recursive_pathmap_node_normal_form :
  forall par_child hash_fn bytes program span field,
    Schema.descriptor_payload field = Schema.PathMap ->
    Recursive.recursive_source_reference par_child hash_fn bytes program
      (Source.SourceAuxiliary span field) =
    match Recursive.checked_generated_raw bytes program
        (Source.SourceAuxiliary span field) with
    | None => None
    | Some raw =>
        match EPM1ACTree03.decode_epm1_actree03 hash_fn
            (Source.source_body bytes (Source.SourceAuxiliary span field)) with
        | None => None
        | Some (snapshot, trie) =>
            match Source.sequence_source_results
                (map par_child (EPM1.value_bodies snapshot)) with
            | None => None
            | Some values =>
                option_map
                  (Recursive.RecursiveAuxiliaryResult
                    (Recursive.RecursivePathMapResult raw snapshot trie values))
                  (Recursive.recursive_auxiliary_semantic
                    (Recursive.RecursivePathMapResult raw snapshot trie values))
            end
        end
    end.
Proof.
  intros par_child hash_fn bytes program span field payload.
  cbn [Recursive.recursive_source_reference].
  unfold Recursive.recursive_auxiliary_reference.
  destruct (Recursive.checked_generated_raw bytes program
      (Source.SourceAuxiliary span field)) as [raw|]; [|reflexivity].
  cbv beta iota zeta.
  rewrite (Nat.eqb_refl (Schema.descriptor_slot field)), payload.
  cbv beta iota zeta.
  destruct (EPM1ACTree03.decode_epm1_actree03 hash_fn
      (Source.source_body bytes (Source.SourceAuxiliary span field)))
    as [[snapshot trie]|]; [|reflexivity].
  cbv beta iota zeta.
  destruct (Source.sequence_source_results
      (map par_child (EPM1.value_bodies snapshot))) as [values|]; reflexivity.
Qed.

(** Once the arm's per-value selection agrees pointwise with the target's Par
    child, the node verdict follows for the arm's acceptance list. *)
Lemma pathmap_selected_values_verdict :
  forall (select : list nat -> option (bool * Core.source_exact_summary))
      (par_child : list nat -> option (bool * Semantic.semantic_summary))
      raw snapshot trie,
    (forall value : list nat, In value (EPM1.value_bodies snapshot) ->
      par_child value =
        option_map ParAnswer.project_par_answer (select value)) ->
    RecursiveProjection.source_target_child_verdict
      (match Source.sequence_source_results
          (map select (EPM1.value_bodies snapshot)) with
       | None => None
       | Some values =>
           Some (oracle_pathmap_accepted_summary raw snapshot values)
       end)
      (match Source.sequence_source_results
          (map par_child (EPM1.value_bodies snapshot)) with
       | None => None
       | Some converted =>
           option_map
             (Recursive.RecursiveAuxiliaryResult
               (Recursive.RecursivePathMapResult raw snapshot trie converted))
             (Recursive.recursive_auxiliary_semantic
               (Recursive.RecursivePathMapResult raw snapshot trie converted))
       end).
Proof.
  intros select par_child raw snapshot trie pointwise.
  rewrite (sequence_source_results_projects_pointwise select par_child
    (EPM1.value_bodies snapshot) pointwise).
  destruct (Source.sequence_source_results
      (map select (EPM1.value_bodies snapshot))) as [values|].
  - destruct (accepted_pathmap_summary_projects_to_target_semantic raw snapshot
        trie (map ParAnswer.project_par_answer values) values)
      as [not_refused target_semantic].
    cbn [option_map].
    rewrite target_semantic.
    split; [exact not_refused|reflexivity].
  - exact I.
Qed.

Theorem generated_pathmap_node_verdict :
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
    Schema.descriptor_payload field = Schema.PathMap ->
    RecursiveProjection.source_target_child_verdict
      (Oracle.source_evaluate_node recurse hash_fn bytes program
        (Source.SourceAuxiliary span field))
      (Recursive.recursive_source_reference par_child hash_fn bytes program
        (Source.SourceAuxiliary span field)).
Proof.
  intros recurse par_child hash_fn bytes program fuel start finish nodes span
    field recurse_saturated par_child_is_reference reference_answer decoded
    member payload.
  pose proof (RegionChild.decoded_node_checked_raw_agrees fuel bytes program
    start finish nodes (Source.SourceAuxiliary span field) decoded member)
    as checked_agrees.
  rewrite (recursive_pathmap_node_normal_form par_child hash_fn bytes program
    span field payload).
  rewrite checked_agrees.
  unfold Oracle.source_evaluate_node.
  destruct (Source.raw_field_of_source bytes
      (Source.SourceAuxiliary span field)) as [raw|] eqn:converted;
    [|exact I].
  pose proof (Source.raw_field_of_source_preserves_wire_and_body bytes
    (Source.SourceAuxiliary span field) raw converted)
    as [_ [_ [_ [raw_body_is_source _]]]].
  cbv beta iota zeta.
  rewrite payload.
  cbv beta iota zeta.
  rewrite raw_body_is_source.
  destruct (EPM1ACTree03.decode_epm1_actree03 hash_fn
      (Source.source_body bytes (Source.SourceAuxiliary span field)))
    as [[snapshot trie]|] eqn:physical; [|exact I].
  cbv beta iota zeta.
  apply (pathmap_selected_values_verdict _ par_child raw snapshot trie).
  intros value value_member.
  assert (shorter : length value < length bytes)
    by exact (decoded_pathmap_value_is_strictly_shorter bytes
      (Source.SourceAuxiliary span field) hash_fn snapshot trie value
      physical value_member).
  rewrite (par_child_is_reference value shorter).
  rewrite (reference_answer value shorter).
  f_equal.
  cbv beta iota zeta.
  unfold ParAnswer.oracle_scored_par_reference.
  rewrite (recurse_saturated value shorter).
  destruct (Oracle.source_evaluate_message (S (length value)) hash_fn
      (Oracle.source_par_raw value)) as [summary|]; [|reflexivity].
  cbv beta iota zeta.
  unfold ParAnswer.source_scored_par_result.
  (* The arm reads [Some semantic_default, Some _], so the split on the
     semantic default is the point where its acceptance rule is used. *)
  destruct (Core.source_summary_semantic_default summary) as [[|]|].
  - (* [Some true]: a default-valued Par value is selected when scored and
       unrefused.  Under the former [Some false] arm this case fails: the
       scored reference is [Some (true, summary)] and the arm yields [None]. *)
    destruct (Core.source_summary_exact_score summary) as [score|];
      destruct (Core.source_summary_refused summary); reflexivity.
  - (* [Some false]: the non-default values accepted by both rules. *)
    destruct (Core.source_summary_exact_score summary) as [score|];
      destruct (Core.source_summary_refused summary); reflexivity.
  - (* [None]: no semantic default, so both the arm and the reference refuse. *)
    destruct (Core.source_summary_exact_score summary) as [score|];
      destruct (Core.source_summary_refused summary); reflexivity.
Qed.

Print Assumptions decoded_pathmap_value_is_strictly_shorter.
Print Assumptions sequence_source_results_projects_pointwise.
Print Assumptions accepted_pathmap_summary_projects_to_target_semantic.
Print Assumptions recursive_pathmap_node_normal_form.
Print Assumptions pathmap_selected_values_verdict.
Print Assumptions generated_pathmap_node_verdict.
