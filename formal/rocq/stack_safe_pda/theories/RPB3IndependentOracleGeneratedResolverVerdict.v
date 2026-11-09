From Stdlib Require Import List Bool Arith Wf_nat.
From StackSafePDA Require Import RPB3WireCursor
  RPB3GeneratedFieldRefinement RPB3RawRegionTree
  RPB3SemanticReducer RPB3IndependentOracleCore
  RPB3IndependentOracleRecursive
  RPB3RecursiveAuxiliarySourceReference.

Import ListNotations.

Module Wire := RPB3WireCursor.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Target := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Oracle := RPB3IndependentOracleRecursive.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.

(** Resolver failure is a shared rejection gate for generated Message roots.
    The target reducer receives a child list as an argument, whereas the
    independent evaluator obtains its children by parsing the original body.
    The gate below holds for every such list; the successful branch exposes
    the actual parsed nodes and evaluated children without supplying a
    forest-alignment premise. *)
Theorem unresolved_generated_message_verdict :
  forall recurse hash_fn raw target_children,
    Schema.raw_payload raw = Schema.Message ->
    Target.generated_wire_program raw = None ->
    Oracle.source_evaluate_message_step recurse hash_fn raw = None /\
    Target.sem_definitely_refused
      (Target.reduce_semantic_field raw target_children) = true.
Proof.
  intros recurse hash_fn raw target_children payload unresolved.
  split.
  - unfold Oracle.source_evaluate_message_step.
    rewrite payload.
    destruct (Schema.raw_program_type_id raw) as [type_id|]
      eqn:raw_type; [|reflexivity].
    unfold Target.generated_wire_program in unresolved.
    rewrite payload, raw_type in unresolved.
    now rewrite unresolved.
  - unfold Target.reduce_semantic_field.
    rewrite unresolved.
    cbn [Target.sem_definitely_refused negb orb].
    reflexivity.
Qed.

Corollary unresolved_generated_message_at_every_positive_fuel :
  forall fuel hash_fn raw target_children,
    Schema.raw_payload raw = Schema.Message ->
    Target.generated_wire_program raw = None ->
    Oracle.source_evaluate_message (S fuel) hash_fn raw = None /\
    Target.sem_definitely_refused
      (Target.reduce_semantic_field raw target_children) = true.
Proof.
  intros fuel hash_fn raw target_children payload unresolved.
  rewrite Oracle.source_evaluate_message_unfold.
  now apply unresolved_generated_message_verdict.
Qed.

Theorem generated_message_step_success_iff :
  forall recurse hash_fn raw summary,
    Oracle.source_evaluate_message_step recurse hash_fn raw =
      Some summary <->
    exists type_id program nodes children,
      Schema.raw_payload raw = Schema.Message /\
      Schema.raw_program_type_id raw = Some type_id /\
      Schema.lookup_projected_program type_id [] [] = Some program /\
      Source.decode_region (S (length (Schema.raw_body raw)))
        (Schema.raw_body raw) program 0
        (length (Schema.raw_body raw)) = Some nodes /\
      Source.sequence_source_results
        (map (Oracle.source_evaluate_node recurse hash_fn
          (Schema.raw_body raw) program) nodes) = Some children /\
      summary = Oracle.source_message_summary raw program children.
Proof.
  intros recurse hash_fn raw summary.
  split.
  - intro accepted.
    unfold Oracle.source_evaluate_message_step in accepted.
    destruct (Schema.raw_payload raw) eqn:payload;
      try discriminate.
    destruct (Schema.raw_program_type_id raw) as [type_id|]
      eqn:raw_type; [|discriminate].
    destruct (Schema.lookup_projected_program type_id [] [])
      as [program|] eqn:looked_up; [|discriminate].
    destruct (Source.decode_region (S (length (Schema.raw_body raw)))
      (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)))
      as [nodes|] eqn:decoded; [|discriminate].
    destruct (Source.sequence_source_results
      (map (Oracle.source_evaluate_node recurse hash_fn
        (Schema.raw_body raw) program) nodes))
      as [children|] eqn:evaluated; [|discriminate].
    cbn [option_map] in accepted.
    inversion accepted; subst summary.
    exists type_id, program, nodes, children.
    repeat split; assumption || reflexivity.
  - intros [type_id [program [nodes [children
      [payload [raw_type [looked_up [decoded [evaluated summary_eq]]]]]]]]].
    subst summary.
    unfold Oracle.source_evaluate_message_step.
    now rewrite payload, raw_type, looked_up, decoded, evaluated.
Qed.

Corollary successful_generated_message_resolves_on_target :
  forall recurse hash_fn raw summary,
    Oracle.source_evaluate_message_step recurse hash_fn raw =
      Some summary ->
    exists program,
      Target.generated_wire_program raw = Some program.
Proof.
  intros recurse hash_fn raw summary accepted.
  apply generated_message_step_success_iff in accepted.
  destruct accepted as [type_id [program [nodes [children
    [payload [raw_type [looked_up _]]]]]]].
  exists program.
  unfold Target.generated_wire_program.
  now rewrite payload, raw_type, looked_up.
Qed.

(** This is the exact public Par acceptance gate.  Composing its first
    conjunct with [generated_message_step_success_iff] recovers the original
    generated program, parsed nodes, and child evaluations for every
    accepted Par, without guessing a target child forest. *)
Theorem independent_par_acceptance_gate_iff :
  forall hash_fn body semantic_default summary,
    Oracle.source_par_reference hash_fn body =
      Some (semantic_default, summary) <->
    Oracle.source_evaluate_message_step
      (Oracle.source_evaluate_message (length body) hash_fn) hash_fn
      (Oracle.source_par_raw body) = Some summary /\
    Core.source_summary_refused summary = false /\
    Core.source_summary_semantic_default summary =
      Some semantic_default.
Proof.
  intros hash_fn body semantic_default summary.
  unfold Oracle.source_par_reference.
  rewrite Oracle.source_evaluate_message_unfold.
  split.
  - intro accepted.
    destruct (Oracle.source_evaluate_message_step
      (Oracle.source_evaluate_message (length body) hash_fn) hash_fn
      (Oracle.source_par_raw body)) as [found|] eqn:evaluated;
      [|discriminate accepted].
    destruct (Core.source_summary_refused found) eqn:refused;
      [discriminate accepted|].
    destruct (Core.source_summary_semantic_default found)
      as [observed|] eqn:source_default; [|discriminate accepted].
    cbn [option_map] in accepted.
    inversion accepted; subst semantic_default summary.
    repeat split; assumption.
  - intros [evaluated [refused source_default]].
    now rewrite evaluated, refused, source_default.
Qed.

(** The public Par gate supplies its own ordered children by decoding the
    original body.  In particular, the child list below is not an arbitrary
    argument to the target reducer.  This source provenance is a necessary
    premise for the later full-summary/worklist correspondence. *)
Theorem accepted_par_has_exact_source_children :
  forall hash_fn body semantic_default summary,
    Oracle.source_par_reference hash_fn body =
      Some (semantic_default, summary) <->
    exists program nodes children,
      Schema.lookup_projected_program 1 [] [] = Some program /\
      Source.decode_region (S (length body)) body program 0
        (length body) = Some nodes /\
      Source.sequence_source_results
        (map (Oracle.source_evaluate_node
          (Oracle.source_evaluate_message (length body) hash_fn)
          hash_fn body program) nodes) = Some children /\
      summary = Oracle.source_message_summary
        (Oracle.source_par_raw body) program children /\
      Core.source_summary_refused summary = false /\
      Core.source_summary_semantic_default summary =
        Some semantic_default.
Proof.
  intros hash_fn body semantic_default summary.
  split.
  - intro accepted.
    apply independent_par_acceptance_gate_iff in accepted.
    destruct accepted as [evaluated [unrefused default]].
    apply generated_message_step_success_iff in evaluated.
    destruct evaluated as [type_id [program [nodes [children
      [payload [family [looked_up [decoded [selected result]]]]]]]]].
    unfold Oracle.source_par_raw in family.
    cbn [Schema.raw_program_type_id] in family.
    inversion family; subst type_id.
    exists program, nodes, children.
    repeat split;
      first [exact looked_up | exact decoded | exact selected |
        exact result | exact unrefused | exact default | reflexivity].
  - intros [program [nodes [children
      [looked_up [decoded [selected [result [unrefused default]]]]]]]].
    apply independent_par_acceptance_gate_iff.
    split.
    + apply generated_message_step_success_iff.
      exists 1, program, nodes, children.
      repeat split;
        first [exact looked_up | exact decoded | exact selected |
          exact result | reflexivity].
    + split; assumption.
Qed.

(** A one-byte unfinished key is a finite control for the provenance cut.
    An arbitrary target child list cannot stand in for the independently
    decoded children, even when the generated Par resolver succeeds. *)
Lemma unfinished_key_is_rejected_by_cursor :
  Wire.worklist_region [255] 0 1 false = Wire.Rejected.
Proof. reflexivity. Qed.

Lemma unfinished_key_has_no_decoded_children :
  forall program,
    Source.decode_region 2 [255] program 0 1 = None.
Proof.
  intro program.
  cbn [Source.decode_region].
  now rewrite unfinished_key_is_rejected_by_cursor.
Qed.

Lemma unfinished_key_has_no_decoded_par_root :
  Source.decode_root 1 [255] = None.
Proof.
  unfold Source.decode_root.
  destruct (Schema.lookup_projected_program 1 [] [])
    as [program|]; [|reflexivity].
  now rewrite unfinished_key_has_no_decoded_children.
Qed.

Lemma unfinished_par_key_has_no_independent_summary :
  forall recurse hash_fn,
    Oracle.source_evaluate_message_step recurse hash_fn
      (Oracle.source_par_raw [255]) = None.
Proof.
  intros recurse hash_fn.
  unfold Oracle.source_evaluate_message_step, Oracle.source_par_raw.
  cbn [Schema.raw_payload Schema.raw_program_type_id
    Schema.raw_body length].
  destruct (Schema.lookup_projected_program 1 [] [])
    as [program|]; [|reflexivity].
  now rewrite unfinished_key_has_no_decoded_children.
Qed.

Theorem supplied_empty_children_hide_unfinished_par_key :
  forall hash_fn,
    Oracle.source_par_reference hash_fn [255] = None /\
    Recursive.recursive_par_reference hash_fn [255] = None /\
    Target.sem_definitely_refused
      (Target.reduce_semantic_field
        (Oracle.source_par_raw [255]) []) = false /\
    exists score,
      Target.sem_score_evidence
        (Target.reduce_semantic_field
          (Oracle.source_par_raw [255]) []) = Target.ExactScore score.
Proof.
  intro hash_fn.
  split.
  - unfold Oracle.source_par_reference.
    rewrite Oracle.source_evaluate_message_unfold.
    rewrite unfinished_par_key_has_no_independent_summary.
    reflexivity.
  - split.
    + unfold Recursive.recursive_par_reference, Fix.
      destruct (Recursive.shorter_body_is_well_founded [255])
        as [recurse].
      cbn [Fix_F].
      unfold Recursive.recursive_par_step.
      rewrite unfinished_key_has_no_decoded_par_root.
      destruct (Schema.lookup_projected_program 1 [] []);
        reflexivity.
    + split.
      * vm_compute. reflexivity.
      * eexists. vm_compute. reflexivity.
Qed.

Print Assumptions unresolved_generated_message_verdict.
Print Assumptions unresolved_generated_message_at_every_positive_fuel.
Print Assumptions generated_message_step_success_iff.
Print Assumptions successful_generated_message_resolves_on_target.
Print Assumptions independent_par_acceptance_gate_iff.
Print Assumptions accepted_par_has_exact_source_children.
Print Assumptions unfinished_key_is_rejected_by_cursor.
Print Assumptions unfinished_key_has_no_decoded_children.
Print Assumptions unfinished_key_has_no_decoded_par_root.
Print Assumptions unfinished_par_key_has_no_independent_summary.
Print Assumptions supplied_empty_children_hide_unfinished_par_key.
