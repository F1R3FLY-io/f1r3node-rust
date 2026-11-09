(** RPB3IndependentOracleRootMatch: the independent oracle and the recursive
    target agree on the root message of every generated type.

    [recursive_mixed_reference_matches_oracle] compares the two on a Par body
    and [recursive_root_children_match_oracle] on the children of a decoded
    root.  Neither says anything about the summary of the root itself when the
    root is not a Par.  [generated_root_matches_oracle] closes that gap: for
    every type id and byte string, the oracle's evaluation of the generated
    root field and the recursive target's closed root stand in the
    [source_target_child_verdict] relation.  The proof reuses the message-level
    agreement of [RPB3IndependentOracleRecursiveMatch]; the only input that was
    missing is that the root's nodes are the ones the verdict by fuel covers.

    [source_indexed_root_summary_matches_oracle] and
    [source_indexed_root_accepts_matches_oracle] then state the agreement for
    the source-indexed frame machine, through the frame theorem
    [SourceIndexedFrames.source_indexed_root_has_universal_recursive_summary].

    The red control closes the root over its children in reverse order.  On a
    New whose map keys are in order the oracle admits and the reversed close
    returns nothing, so [reversed_close_breaks_the_root_match] refutes the
    statement of [generated_root_matches_oracle] over the mutated close. *)

From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import EPM1 EPM1ACTree03 RPB3RecursiveParUnfold
  RPB3IndependentOracleAdequacy RPB3IndependentOracleScalarVerdictBridge
  RPB3IndependentOracleStringMapVerdictBridge
  RPB3IndependentOraclePathMapVerdictBridge
  RPB3IndependentOracleRegionChildBridge
  RPB3IndependentOracleStringMapTypedRebaseBridge
  RPB3IndependentOracleMessageAssembly
  RPB3IndependentOracleParMessageBodyDecodeBridge
  RPB3IndependentOracleProcessChildProvenance
  RPB3IndependentOracleProcessScalarDecoderBridge
  RPB3IndependentOracleMutants
  RPB3IndependentOracleRecursiveMatch
  RPB3SourceIndexedFrames.

Import ListNotations.

Definition root_raw (type_id : nat) (bytes : list nat) : Schema.raw_field :=
  SourceIndexedFrames.source_generated_root_field type_id bytes.

(** The oracle's answer for the root: the fuel saturates the body. *)
Definition oracle_root (hash_fn : list nat -> nat) (type_id : nat)
    (bytes : list nat) : option Core.source_exact_summary :=
  Oracle.source_evaluate_message (S (length bytes)) hash_fn
    (root_raw type_id bytes).

(** The recursive target's root: the decoded children, closed by the same step
    the source-indexed frame machine applies last. *)
Definition target_root (hash_fn : list nat -> nat) (type_id : nat)
    (bytes : list nat) : option Recursive.recursive_mixed_result :=
  match Recursive.recursive_mixed_root_reference hash_fn type_id bytes with
  | Some results =>
      SourceIndexedFrames.close_message (root_raw type_id bytes) type_id results
  | None => None
  end.

Lemma target_root_of_decoded :
  forall hash_fn type_id bytes program nodes,
    Schema.lookup_projected_program type_id [] [] = Some program ->
    Source.decode_region (S (length bytes)) bytes program 0 (length bytes) =
      Some nodes ->
    target_root hash_fn type_id bytes =
    target_message_result (Recursive.recursive_par_reference hash_fn) hash_fn
      bytes (root_raw type_id bytes) type_id program nodes.
Proof.
  intros hash_fn type_id bytes program nodes looked_up decoded.
  unfold target_root, Recursive.recursive_mixed_root_reference, Source.decode_root.
  rewrite looked_up.
  cbv beta iota.
  rewrite decoded.
  unfold target_message_result.
  destruct (Recursive.recursive_source_children
    (Recursive.recursive_par_reference hash_fn) hash_fn bytes program nodes)
    as [results|]; reflexivity.
Qed.

(** Under the fuel the oracle uses for the root, and at saturation, the decoded
    root's nodes evaluate to the same sequence. *)
Lemma root_fueled_sequence :
  forall hash_fn bytes program nodes,
    Source.decode_region (S (length bytes)) bytes program 0 (length bytes) =
      Some nodes ->
    Source.sequence_source_results
      (map (Oracle.source_evaluate_node
        (Oracle.source_evaluate_message (length bytes) hash_fn) hash_fn bytes
        program) nodes) =
    Source.sequence_source_results
      (map (Oracle.source_evaluate_node (saturated_oracle hash_fn) hash_fn bytes
        program) nodes).
Proof.
  intros hash_fn bytes program nodes decoded.
  apply source_sequence_agrees_on_members.
  intros node member.
  apply (source_evaluate_node_callback_ext
    (Oracle.source_evaluate_message (length bytes) hash_fn)
    (saturated_oracle hash_fn) hash_fn bytes program (S (length bytes))
    nodes node); [|exact decoded|exact member].
  intros child_raw shorter.
  apply source_evaluate_message_beyond_body_length.
  exact shorter.
Qed.

Lemma root_children_agreement :
  forall hash_fn type_id bytes program nodes,
    Schema.lookup_projected_program type_id [] [] = Some program ->
    Source.decode_region (S (length bytes)) bytes program 0 (length bytes) =
      Some nodes ->
    message_children_outcome (root_raw type_id bytes) type_id program
      (Recursive.recursive_par_reference hash_fn) hash_fn bytes nodes
      (Oracle.source_evaluate_node (saturated_oracle hash_fn) hash_fn bytes
        program).
Proof.
  intros hash_fn type_id bytes program nodes looked_up decoded.
  set (par_child := Recursive.recursive_par_reference hash_fn).
  set (evaluate := Oracle.source_evaluate_node (saturated_oracle hash_fn)
    hash_fn bytes program).
  assert (verdicts : forall node, In node nodes ->
    RecursiveProjection.source_target_child_verdict (evaluate node)
      (Recursive.recursive_source_reference par_child hash_fn bytes program node)).
  { intros node member.
    unfold evaluate, par_child.
    apply (decoded_node_verdict_by_fuel hash_fn bytes
      (Recursive.recursive_par_reference hash_fn)
      (fun value _ => eq_refl)
      (fun value _ => recursive_mixed_reference_matches_oracle hash_fn value)
      (length bytes) type_id program 0 (length bytes) nodes looked_up);
      [lia|lia|exact decoded|exact member]. }
  pose proof (root_fueled_sequence hash_fn bytes program nodes decoded)
    as fueled_sequence.
  assert (faithful : forall summaries,
    Source.sequence_source_results (map evaluate nodes) = Some summaries ->
    Forall child_raw_is_faithful summaries).
  { intros summaries selected.
    apply (sequenced_results_satisfy Source.source_tree
      Core.source_exact_summary evaluate child_raw_is_faithful nodes summaries
      selected).
    intros node summary member evaluated.
    exact (decoded_node_child_is_faithful (saturated_oracle hash_fn) hash_fn
      bytes program (S (length bytes)) 0 (length bytes) nodes node summary
      (saturated_oracle_retains_raw hash_fn) decoded member evaluated). }
  assert (witness : forall summaries,
    Source.sequence_source_results (map evaluate nodes) = Some summaries ->
    Admission.source_scan_valid
      (Admission.source_scan_children program summaries) = true ->
    actual_unrefused_children program summaries).
  { intros summaries selected valid.
    unfold actual_unrefused_children.
    exists (length bytes), hash_fn, bytes, nodes.
    split; [exact decoded|].
    split; [|exact valid].
    rewrite fueled_sequence.
    exact selected. }
  assert (map_order : forall summaries results,
    type_id = 20 ->
    Source.sequence_source_results (map evaluate nodes) = Some summaries ->
    Recursive.recursive_source_children par_child hash_fn bytes program nodes =
      Some results ->
    Admission.source_scan_valid
      (Admission.source_scan_children program summaries) = true ->
    Oracle.source_string_map_keys_ordered summaries =
    Recursive.ordered_recursive_new_map_returnsb None
      (Recursive.recursive_new_map_returns results)).
  { intros summaries results is_new selected target_children scan.
    subst type_id.
    assert (same_program : program = Source.generated_new_program).
    { rewrite unordered_injections_lookup in looked_up.
      injection looked_up as same.
      exact (eq_sym same). }
    subst program.
    apply (new_order_bridge summaries results); [|exact scan].
    pose proof (RecursiveProjection.selected_source_evaluator_outputs_are_positional
      Source.source_tree Core.source_exact_summary evaluate nodes summaries
      selected) as source_positional.
    pose proof (Recursive.accepted_recursive_children_preserve_source_order
      par_child hash_fn bytes Source.generated_new_program nodes results
      target_children) as target_positional.
    exact (positional_results_zip Source.source_tree
      Core.source_exact_summary Recursive.recursive_mixed_result
      evaluate
      (Recursive.recursive_source_reference par_child hash_fn bytes
        Source.generated_new_program)
      new_map_child_view nodes summaries results
      source_positional target_positional
      (fun child_node summary result child_member evaluated targeted =>
        new_map_child_view_of_node (saturated_oracle hash_fn) par_child
          hash_fn bytes Source.generated_new_program (S (length bytes))
          0 (length bytes) nodes child_node summary result
          (saturated_oracle_has_message_shape hash_fn)
          decoded child_member evaluated targeted)). }
  exact (message_children_agreement (root_raw type_id bytes) type_id program
    par_child hash_fn bytes nodes evaluate eq_refl eq_refl looked_up verdicts
    faithful witness map_order).
Qed.

Theorem generated_root_matches_oracle :
  forall hash_fn type_id bytes,
    RecursiveProjection.source_target_child_verdict
      (oracle_root hash_fn type_id bytes)
      (target_root hash_fn type_id bytes).
Proof.
  intros hash_fn type_id bytes.
  destruct (Schema.lookup_projected_program type_id [] [])
    as [program|] eqn:looked_up.
  - destruct (Source.decode_region (S (length bytes)) bytes program 0
      (length bytes)) as [nodes|] eqn:decoded.
    + rewrite (target_root_of_decoded hash_fn type_id bytes program nodes
        looked_up decoded).
      unfold oracle_root.
      rewrite Oracle.source_evaluate_message_unfold.
      rewrite (Oracle.source_evaluate_message_step_selected_children
        (Oracle.source_evaluate_message (length bytes) hash_fn) hash_fn
        (root_raw type_id bytes) type_id program nodes eq_refl eq_refl
        looked_up decoded).
      change (Schema.raw_body (root_raw type_id bytes)) with bytes.
      rewrite (root_fueled_sequence hash_fn bytes program nodes decoded).
      exact (message_node_verdict_from_outcome (root_raw type_id bytes) type_id
        program (Recursive.recursive_par_reference hash_fn) hash_fn bytes nodes
        (Oracle.source_evaluate_node (saturated_oracle hash_fn) hash_fn bytes
          program)
        (root_children_agreement hash_fn type_id bytes program nodes looked_up
          decoded)).
    + assert (oracle_none : oracle_root hash_fn type_id bytes = None).
      { unfold oracle_root.
        rewrite Oracle.source_evaluate_message_unfold.
        exact (source_evaluate_message_step_undecoded
          (Oracle.source_evaluate_message (length bytes) hash_fn) hash_fn
          (root_raw type_id bytes) type_id program eq_refl eq_refl looked_up
          decoded). }
      assert (target_none : target_root hash_fn type_id bytes = None).
      { unfold target_root, Recursive.recursive_mixed_root_reference,
          Source.decode_root.
        rewrite looked_up.
        cbv beta iota.
        rewrite decoded.
        reflexivity. }
      rewrite oracle_none, target_none.
      exact I.
  - assert (oracle_none : oracle_root hash_fn type_id bytes = None).
    { unfold oracle_root.
      rewrite Oracle.source_evaluate_message_unfold.
      unfold Oracle.source_evaluate_message_step.
      change (Schema.raw_payload (root_raw type_id bytes)) with Schema.Message.
      change (Schema.raw_program_type_id (root_raw type_id bytes))
        with (Some type_id).
      cbv beta iota.
      rewrite looked_up.
      reflexivity. }
    assert (target_none : target_root hash_fn type_id bytes = None).
    { unfold target_root, Recursive.recursive_mixed_root_reference.
      rewrite looked_up.
      reflexivity. }
    rewrite oracle_none, target_none.
    exact I.
Qed.

Definition oracle_root_admits (hash_fn : list nat -> nat) (type_id : nat)
    (bytes : list nat) : bool :=
  match oracle_root hash_fn type_id bytes with
  | Some summary => negb (Core.source_summary_refused summary)
  | None => false
  end.

(** The summary the frame machine reports is the projection of the oracle's
    unrefused root summary. *)
Theorem source_indexed_root_summary_matches_oracle :
  forall hash_fn type_id bytes,
    SourceIndexedFrames.source_indexed_root_summary hash_fn type_id bytes =
    option_map (fun summary => [Projection.project_source_summary summary])
      (RecursiveProjection.source_unrejected_child
        (oracle_root hash_fn type_id bytes)).
Proof.
  intros hash_fn type_id bytes.
  rewrite SourceIndexedFrames.source_indexed_root_has_universal_recursive_summary.
  pose proof (RecursiveProjection.source_target_child_verdict_projects
    (oracle_root hash_fn type_id bytes) (target_root hash_fn type_id bytes)
    (generated_root_matches_oracle hash_fn type_id bytes)) as projected.
  assert (singleton_of_projection :
    option_map (fun summary => [Projection.project_source_summary summary])
      (RecursiveProjection.source_unrejected_child
        (oracle_root hash_fn type_id bytes)) =
    option_map (fun semantic => [semantic])
      (option_map Projection.project_source_summary
        (RecursiveProjection.source_unrejected_child
          (oracle_root hash_fn type_id bytes)))).
  { destruct (RecursiveProjection.source_unrejected_child
      (oracle_root hash_fn type_id bytes)); reflexivity. }
  rewrite singleton_of_projection, projected.
  unfold target_root, root_raw.
  destruct (Recursive.recursive_mixed_root_reference hash_fn type_id bytes)
    as [results|]; [|reflexivity].
  destruct (SourceIndexedFrames.close_message
    (SourceIndexedFrames.source_generated_root_field type_id bytes)
    type_id results); reflexivity.
Qed.

(** The admission verdict of the frame machine is the oracle's. *)
Theorem source_indexed_root_accepts_matches_oracle :
  forall hash_fn type_id bytes,
    SourceIndexedFrames.source_indexed_root_accepts hash_fn type_id bytes =
    oracle_root_admits hash_fn type_id bytes.
Proof.
  intros hash_fn type_id bytes.
  unfold SourceIndexedFrames.source_indexed_root_accepts, oracle_root_admits.
  rewrite source_indexed_root_summary_matches_oracle.
  destruct (oracle_root hash_fn type_id bytes) as [summary|]; [|reflexivity].
  cbn [RecursiveProjection.source_unrejected_child].
  destruct (Core.source_summary_refused summary) eqn:refused.
  - reflexivity.
  - cbn [option_map].
    change (negb (Semantic.sem_definitely_refused
      (Projection.project_source_summary summary)) = negb false).
    assert (unrefused : Semantic.sem_definitely_refused
      (Projection.project_source_summary summary) = false) by exact refused.
    rewrite unrefused.
    reflexivity.
Qed.

(** * Red control: a root closed over its children in reverse order *)

Definition target_root_over_reversed_children (hash_fn : list nat -> nat)
    (type_id : nat) (bytes : list nat) : option Recursive.recursive_mixed_result :=
  match Recursive.recursive_mixed_root_reference hash_fn type_id bytes with
  | Some results =>
      SourceIndexedFrames.close_message (root_raw type_id bytes) type_id
        (rev results)
  | None => None
  end.

Definition root_control_hash (_ : list nat) : nat := 0.

Definition root_control_new_body : list nat :=
  [18; 0; 34; 3; 10; 1; 97; 34; 3; 10; 1; 98].

Example root_control_oracle_admits :
  oracle_root_admits root_control_hash 20 root_control_new_body = true.
Proof. vm_compute. reflexivity. Qed.

Example root_control_reversed_close_refuses :
  target_root_over_reversed_children root_control_hash 20 root_control_new_body
    = None.
Proof. vm_compute. reflexivity. Qed.

Theorem reversed_close_breaks_the_root_match :
  ~ (forall hash_fn type_id bytes,
      RecursiveProjection.source_target_child_verdict
        (oracle_root hash_fn type_id bytes)
        (target_root_over_reversed_children hash_fn type_id bytes)).
Proof.
  intros claimed.
  pose proof (claimed root_control_hash 20 root_control_new_body) as verdict.
  rewrite root_control_reversed_close_refuses in verdict.
  pose proof root_control_oracle_admits as admitted.
  unfold oracle_root_admits in admitted.
  destruct (oracle_root root_control_hash 20 root_control_new_body)
    as [summary|] eqn:evaluated.
  - cbn [RecursiveProjection.source_target_child_verdict] in verdict.
    rewrite verdict in admitted.
    discriminate admitted.
  - discriminate admitted.
Qed.

Print Assumptions target_root_of_decoded.
Print Assumptions root_fueled_sequence.
Print Assumptions root_children_agreement.
Print Assumptions generated_root_matches_oracle.
Print Assumptions source_indexed_root_summary_matches_oracle.
Print Assumptions source_indexed_root_accepts_matches_oracle.
Print Assumptions root_control_oracle_admits.
Print Assumptions root_control_reversed_close_refuses.
Print Assumptions reversed_close_breaks_the_root_match.
