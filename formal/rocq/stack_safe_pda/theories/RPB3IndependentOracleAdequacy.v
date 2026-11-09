From Stdlib Require Import List Arith Lia.
From StackSafePDA Require Import EPM1 EPM1ACTree03
  RPB3GeneratedFieldRefinement RPB3RawRegionTree
  RPB3IndependentOracleCore RPB3IndependentOracleRecursive
  RPB3IndependentOracleFuel.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Core := RPB3IndependentOracleCore.
Module Oracle := RPB3IndependentOracleRecursive.
Module Fuel := RPB3IndependentOracleFuel.

Lemma source_sequence_agrees_on_members :
  forall (A B : Type) (left right : A -> option B) members,
    (forall member, In member members -> left member = right member) ->
    Source.sequence_source_results (map left members) =
      Source.sequence_source_results (map right members).
Proof.
  intros A B left right members agree.
  f_equal. apply map_ext_in. exact agree.
Qed.

Definition source_selected_unrefused_par
    (candidate : option Core.source_exact_summary)
    : option (bool * Core.source_exact_summary) :=
  match candidate with
  | Some summary =>
      match Core.source_summary_semantic_default summary,
            Core.source_summary_exact_score summary with
      | Some semantic_default, Some _ =>
          if Core.source_summary_refused summary
          then None else Some (semantic_default, summary)
      | _, _ => None
      end
  | None => None
  end.

(** Selectively unfold only the two outer Fixpoint applications.  Their
    decoders and source-node lists are identical.  Each recursive message,
    StringMap Par, and PathMap value call has the strictly smaller body
    established in [RPB3IndependentOracleFuel]. *)
Theorem source_evaluate_message_stable_one_more_fuel :
  forall fuel hash_fn raw,
    length (Schema.raw_body raw) < fuel ->
    Oracle.source_evaluate_message (S fuel) hash_fn raw =
      Oracle.source_evaluate_message fuel hash_fn raw.
Proof.
  induction fuel as [|fuel IH]; intros hash_fn raw enough; [lia|].
  change
    (Oracle.source_evaluate_message_step
       (Oracle.source_evaluate_message (S fuel) hash_fn) hash_fn raw =
     Oracle.source_evaluate_message_step
       (Oracle.source_evaluate_message fuel hash_fn) hash_fn raw).
  unfold Oracle.source_evaluate_message_step at 1 2.
  cbn beta iota zeta.
  destruct (Schema.raw_payload raw) eqn:payload; try reflexivity.
  destruct (Schema.raw_program_type_id raw) as [type_id|] eqn:family;
    [|reflexivity].
  destruct (Schema.lookup_projected_program type_id [] [])
    as [program|] eqn:looked_up; [|reflexivity].
  set (body := Schema.raw_body raw) in *.
  destruct (Source.decode_region (S (length body)) body program 0
      (length body)) as [nodes|] eqn:decoded; [|reflexivity].
  f_equal.
  apply source_sequence_agrees_on_members.
  intros node member.
  unfold Oracle.source_evaluate_node.
  destruct (Source.raw_field_of_source body node)
    as [child_raw|] eqn:converted; [|reflexivity].
  destruct node as [span field|span field nested|span field];
    cbn beta iota zeta; try reflexivity.
  - assert (strict_child :
        length (Schema.raw_body child_raw) < fuel).
    {
      pose proof (Fuel.decoded_source_raw_child_body_is_strictly_smaller
        (S (length body)) body program nodes
        (Source.SourceMessage span field nested) child_raw
        decoded member converted) as strict.
      lia.
    }
    exact (IH hash_fn child_raw strict_child).
  - destruct (Schema.descriptor_payload field) eqn:role;
      try reflexivity.
    + destruct (Source.refine_string_map_entry body program
          (Source.SourceAuxiliary span field))
        as [entry|] eqn:refined; [|reflexivity].
      destruct (Oracle.source_valid_string_map_key
          (Source.typed_map_key_source entry))
        as [key|] eqn:key_valid; [|reflexivity].
      destruct (Source.typed_map_value_source entry)
        as [[value_span value_body]|] eqn:value_source;
        [|reflexivity].
      assert (strict_value : length
          (Schema.raw_body (Oracle.source_par_raw value_body)) < fuel).
      {
        pose proof (Fuel.refined_string_map_par_body_is_strictly_smaller
          body program span field entry value_span value_body
          refined value_source) as strict.
        cbn [Oracle.source_par_raw Schema.raw_body].
        lia.
      }
      rewrite (IH hash_fn (Oracle.source_par_raw value_body)
        strict_value).
      reflexivity.
    + destruct (EPM1ACTree03.decode_epm1_actree03 hash_fn
          (Schema.raw_body child_raw)) as [[snapshot trie]|]
        eqn:decoded_epm; [|reflexivity].
      assert (selected_agree :
        Source.sequence_source_results
          (map (fun value_body =>
            source_selected_unrefused_par
              (Oracle.source_evaluate_message (S fuel) hash_fn
                (Oracle.source_par_raw value_body)))
            (EPM1.value_bodies snapshot)) =
        Source.sequence_source_results
          (map (fun value_body =>
            source_selected_unrefused_par
              (Oracle.source_evaluate_message fuel hash_fn
                (Oracle.source_par_raw value_body)))
            (EPM1.value_bodies snapshot))).
      {
        apply source_sequence_agrees_on_members.
        intros value_body member_value.
        assert (strict_value : length
            (Schema.raw_body (Oracle.source_par_raw value_body)) < fuel).
        {
          pose proof (Fuel.decoded_pathmap_par_body_is_strictly_smaller
            body (Source.SourceAuxiliary span field) child_raw
            hash_fn snapshot trie value_body converted decoded_epm
            member_value) as strict.
          change (length value_body < fuel).
          eapply Nat.lt_le_trans; [exact strict|].
          lia.
        }
        now rewrite (IH hash_fn (Oracle.source_par_raw value_body)
          strict_value).
      }
      unfold source_selected_unrefused_par in selected_agree.
      rewrite selected_agree.
      reflexivity.
Qed.

Theorem source_evaluate_message_saturates_at_body_length :
  forall extra hash_fn raw,
    Oracle.source_evaluate_message
      (S (length (Schema.raw_body raw)) + extra) hash_fn raw =
    Oracle.source_evaluate_message
      (S (length (Schema.raw_body raw))) hash_fn raw.
Proof.
  induction extra as [|extra IH]; intros hash_fn raw.
  - now rewrite Nat.add_0_r.
  - replace (S (length (Schema.raw_body raw)) + S extra)
      with (S (S (length (Schema.raw_body raw)) + extra)) by lia.
    rewrite source_evaluate_message_stable_one_more_fuel by lia.
    exact (IH hash_fn raw).
Qed.

Corollary source_par_root_fuel_is_saturated :
  forall extra hash_fn body,
    Oracle.source_evaluate_message
      (S (length body) + extra) hash_fn
      (Oracle.source_par_raw body) =
    Oracle.source_evaluate_message (S (length body)) hash_fn
      (Oracle.source_par_raw body).
Proof.
  intros extra hash_fn body.
  change (Oracle.source_evaluate_message
      (S (length (Schema.raw_body (Oracle.source_par_raw body))) + extra)
      hash_fn (Oracle.source_par_raw body) =
    Oracle.source_evaluate_message
      (S (length (Schema.raw_body (Oracle.source_par_raw body))))
      hash_fn (Oracle.source_par_raw body)).
  apply source_evaluate_message_saturates_at_body_length.
Qed.

Print Assumptions source_sequence_agrees_on_members.
Print Assumptions source_evaluate_message_stable_one_more_fuel.
Print Assumptions source_evaluate_message_saturates_at_body_length.
Print Assumptions source_par_root_fuel_is_saturated.
