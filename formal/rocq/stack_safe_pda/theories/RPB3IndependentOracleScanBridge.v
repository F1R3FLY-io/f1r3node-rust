From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement
  RPB3RawRegionTree RPB3SemanticReducer RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission RPB3IndependentOracleRecursive
  RPB3IndependentOracleProjection.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Target := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Recursive := RPB3IndependentOracleRecursive.
Module Projection := RPB3IndependentOracleProjection.

(** Total form of the oracle/target child scan agreement.  The valid-only
    bridge [Projection.source_scan_children_project_valid] says nothing when
    the oracle rejects; this file proves the two scanners agree on validity
    and seen slots for every child list of the decoded shape, whether the
    oracle accepts or rejects. *)

(** The premise a scan needs beyond well-formed bytes: a child whose payload
    kind is [Message] names a program type id.  Every child produced by the
    decoder has one (see [source_evaluate_node_child_has_type_id]); without
    it the scanners can disagree (see
    [well_formed_children_alone_do_not_force_agreement]). *)
Definition source_message_child_has_type_id
    (child : Core.source_exact_summary) : Prop :=
  Schema.raw_payload (Core.source_summary_raw child) = Schema.Message ->
  exists type_id,
    Schema.raw_program_type_id (Core.source_summary_raw child) =
      Some type_id.

(** The fold invariant: the oracle and target scan states agree on validity,
    last slot, and seen slots, whether the scan is still valid or has already
    failed. *)
Definition source_target_scan_agree
    (source_state : Admission.source_admission_state)
    (target_state : Schema.scan_state) : Prop :=
  Admission.source_scan_valid source_state =
    Schema.scan_valid target_state /\
  Admission.source_scan_last_slot source_state =
    Schema.scan_last_slot target_state /\
  Admission.source_scan_seen_slots source_state =
    Schema.scan_seen target_state.

Lemma source_empty_scan_agrees_with_target :
  source_target_scan_agree Admission.source_empty_scan Schema.empty_scan.
Proof. repeat split; reflexivity. Qed.

(** On well-formed bytes the oracle's child check equals the target's, not
    merely implies it.  No refusal assumption is needed; only a [Message]
    child without a type id can make the two sides differ. *)
Lemma source_child_guard_agrees_with_target :
  forall descriptor child,
    Admission.source_raw_bytes_well_formed
      (Core.source_summary_raw child) = true ->
    source_message_child_has_type_id child ->
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
  intros descriptor child well_formed message_has_type.
  unfold Admission.source_child_matches_descriptor.
  change (Target.sem_definitely_refused
    (Projection.project_source_summary child))
    with (Core.source_summary_refused child).
  change (Target.sem_wire (Projection.project_source_summary child))
    with (Schema.raw_wire (Core.source_summary_raw child)).
  change (Target.sem_payload (Projection.project_source_summary child))
    with (Schema.raw_payload (Core.source_summary_raw child)).
  rewrite well_formed, andb_true_r.
  destruct (Schema.descriptor_payload descriptor) eqn:descriptor_kind;
    try reflexivity.
  destruct (Schema.raw_payload (Core.source_summary_raw child))
    eqn:raw_kind; cbn [Schema.payload_eqb];
    try (rewrite andb_false_r; reflexivity).
  destruct (message_has_type raw_kind) as [observed observed_id].
  change (Target.sem_type_id (Projection.project_source_summary child))
    with (match Schema.raw_program_type_id
        (Core.source_summary_raw child) with
      | Some type_id => type_id
      | None => 0
      end).
  rewrite observed_id.
  destruct (Schema.descriptor_child_type_id descriptor) as [expected|];
    [rewrite Nat.eqb_sym|]; reflexivity.
Qed.

Lemma source_scan_step_agrees_with_target :
  forall program source_state target_state child,
    source_target_scan_agree source_state target_state ->
    Admission.source_raw_bytes_well_formed
      (Core.source_summary_raw child) = true ->
    source_message_child_has_type_id child ->
    source_target_scan_agree
      (Admission.source_scan_step program source_state child)
      (Target.scan_child program target_state
        (Projection.project_source_summary child)).
Proof.
  intros program
    [source_valid source_last source_seen]
    [target_valid target_last target_seen target_ordered target_scores]
    child [same_valid [same_last same_seen]] well_formed message_has_type.
  cbn [Admission.source_scan_valid Admission.source_scan_last_slot
    Admission.source_scan_seen_slots Schema.scan_valid
    Schema.scan_last_slot Schema.scan_seen]
    in same_valid, same_last, same_seen.
  subst source_valid source_last source_seen.
  destruct target_valid.
  - unfold source_target_scan_agree, Admission.source_scan_step,
      Target.scan_child.
    cbn [Admission.source_scan_valid Admission.source_scan_last_slot
      Admission.source_scan_seen_slots Schema.scan_valid
      Schema.scan_last_slot Schema.scan_seen Schema.scan_ordered_slots
      Schema.scan_ordered_scores].
    change (Target.sem_tag (Projection.project_source_summary child))
      with (Core.source_summary_tag child).
    destruct (Schema.resolve_tag (Schema.program_fields program)
      (Core.source_summary_tag child)) as [descriptor|].
    + rewrite (source_child_guard_agrees_with_target descriptor child
        well_formed message_has_type).
      change (Admission.source_slot_follows target_last
          (Schema.descriptor_slot descriptor)
          (Schema.descriptor_repeated descriptor))
        with (Schema.slot_follows target_last
          (Schema.descriptor_slot descriptor)
          (Schema.descriptor_repeated descriptor)).
      match goal with
      | |- context [if ?guard then _ else _] => destruct guard
      end;
        repeat split; reflexivity.
    + repeat split; reflexivity.
  - unfold source_target_scan_agree.
    repeat split; reflexivity.
Qed.

Lemma source_scan_fold_agrees_with_target :
  forall program children source_state target_state,
    Forall (fun child =>
      Admission.source_raw_bytes_well_formed
        (Core.source_summary_raw child) = true) children ->
    Forall source_message_child_has_type_id children ->
    source_target_scan_agree source_state target_state ->
    source_target_scan_agree
      (fold_left (Admission.source_scan_step program) children source_state)
      (fold_left (Target.scan_child program)
        (map Projection.project_source_summary children) target_state).
Proof.
  intros program children.
  induction children as [|child rest IH];
    intros source_state target_state well_formed message_typed agree;
    cbn [fold_left map].
  - exact agree.
  - inversion well_formed as [|? ? child_well_formed rest_well_formed];
      subst.
    inversion message_typed as [|? ? child_typed rest_typed]; subst.
    apply IH; [exact rest_well_formed|exact rest_typed|].
    apply source_scan_step_agrees_with_target;
      [exact agree|exact child_well_formed|exact child_typed].
Qed.

Theorem source_scan_children_project_total :
  forall program children,
    Forall (fun c => Admission.source_raw_bytes_well_formed
                       (Core.source_summary_raw c) = true) children ->
    Forall source_message_child_has_type_id children ->
    Admission.source_scan_valid (Admission.source_scan_children program children) =
    Schema.scan_valid (fold_left (Target.scan_child program)
      (map Projection.project_source_summary children) Schema.empty_scan) /\
    Admission.source_scan_seen_slots (Admission.source_scan_children program children) =
    Schema.scan_seen (fold_left (Target.scan_child program)
      (map Projection.project_source_summary children) Schema.empty_scan).
Proof.
  intros program children well_formed message_typed.
  unfold Admission.source_scan_children.
  destruct (source_scan_fold_agrees_with_target program children
    Admission.source_empty_scan Schema.empty_scan well_formed message_typed
    source_empty_scan_agrees_with_target)
    as [same_valid [_ same_seen]].
  exact (conj same_valid same_seen).
Qed.

(** Why [source_message_child_has_type_id] is required: a message child with
    no type id, under a descriptor expecting child type [0].  The oracle
    compares [Some 0] against [None] and rejects; the target reads the
    projected default id [0] and accepts. *)
Definition zero_child_type_program : Schema.generated_program :=
  Schema.GeneratedProgram 0 1
    [Schema.FieldDescriptor 1 0 Schema.LengthDelimited false
      Schema.Message None (Some 0)] [] [].

Definition typeless_message_child : Core.source_exact_summary :=
  Core.SourceExactSummary
    (Core.TypedSourceMessage
      (Schema.RawField 1 Schema.LengthDelimited Schema.Message
        [] 0 None [] []) [])
    (Core.SourceFamilyAnalysis None None false false false)
    None.

Lemma typeless_message_child_separates_scanners :
  Admission.source_scan_valid
    (Admission.source_scan_children zero_child_type_program
      [typeless_message_child]) = false /\
  Schema.scan_valid
    (fold_left (Target.scan_child zero_child_type_program)
      (map Projection.project_source_summary [typeless_message_child])
      Schema.empty_scan) = true.
Proof. split; vm_compute; reflexivity. Qed.

Theorem well_formed_children_alone_do_not_force_agreement :
  ~ (forall program children,
      Forall (fun c => Admission.source_raw_bytes_well_formed
                         (Core.source_summary_raw c) = true) children ->
      Admission.source_scan_valid
        (Admission.source_scan_children program children) =
      Schema.scan_valid (fold_left (Target.scan_child program)
        (map Projection.project_source_summary children)
        Schema.empty_scan) /\
      Admission.source_scan_seen_slots
        (Admission.source_scan_children program children) =
      Schema.scan_seen (fold_left (Target.scan_child program)
        (map Projection.project_source_summary children)
        Schema.empty_scan)).
Proof.
  intros claim.
  destruct (claim zero_child_type_program [typeless_message_child]
    ltac:(repeat constructor)) as [same_valid _].
  destruct typeless_message_child_separates_scanners
    as [source_rejects target_accepts].
  rewrite source_rejects, target_accepts in same_valid.
  discriminate same_valid.
Qed.

(** Decoded children satisfy the premise: a [Message] raw field reaches the
    recursive evaluator only with a program type id, and scalar and
    auxiliary nodes never carry the [Message] payload kind. *)
Lemma evaluated_message_retains_typed_raw :
  forall fuel hash_fn raw summary,
    Recursive.source_evaluate_message fuel hash_fn raw = Some summary ->
    Core.source_summary_raw summary = raw /\
    exists type_id, Schema.raw_program_type_id raw = Some type_id.
Proof.
  intros [|fuel] hash_fn raw summary evaluated; [discriminate|].
  rewrite Recursive.source_evaluate_message_unfold in evaluated.
  unfold Recursive.source_evaluate_message_step in evaluated.
  destruct (Schema.raw_payload raw); try discriminate.
  destruct (Schema.raw_program_type_id raw) as [type_id|]; [|discriminate].
  destruct (Schema.lookup_projected_program type_id [] [])
    as [program|]; [|discriminate].
  destruct (Source.decode_region (S (length (Schema.raw_body raw)))
    (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)))
    as [nodes|]; [|discriminate].
  destruct (Source.sequence_source_results
    (map (Recursive.source_evaluate_node
      (Recursive.source_evaluate_message fuel hash_fn) hash_fn
      (Schema.raw_body raw) program) nodes))
    as [children|]; [|discriminate].
  inversion evaluated; subst summary.
  split; [reflexivity|exists type_id; reflexivity].
Qed.

Lemma source_evaluate_node_child_has_type_id :
  forall fuel hash_fn body program node child,
    Recursive.source_evaluate_node
      (Recursive.source_evaluate_message fuel hash_fn) hash_fn
      body program node = Some child ->
    source_message_child_has_type_id child.
Proof.
  intros fuel hash_fn body program node child evaluated message_kind.
  unfold Recursive.source_evaluate_node in evaluated.
  destruct (Source.raw_field_of_source body node)
    as [child_raw|] eqn:converted; [|discriminate].
  destruct node as [span field|span field nested|span field].
  - destruct (Admission.source_typed_scalar program child_raw)
      as [value|] eqn:typed; [|discriminate].
    inversion evaluated; subst child.
    unfold Admission.source_typed_scalar in typed.
    destruct (Admission.source_scalar_value_of_raw program child_raw)
      as [scalar|]; [|discriminate].
    inversion typed; subst value.
    unfold Source.raw_field_of_source in converted.
    cbn [Source.source_span Source.source_field] in converted.
    destruct (Schema.payload_eqb (Schema.descriptor_payload field)
      Schema.Scalar); [|discriminate].
    inversion converted; subst.
    discriminate message_kind.
  - destruct (evaluated_message_retains_typed_raw fuel hash_fn child_raw
      child evaluated) as [retained [type_id typed]].
    exists type_id. rewrite retained. exact typed.
  - destruct (Source.raw_field_of_source_preserves_wire_and_body body
      (Source.SourceAuxiliary span field) child_raw converted)
      as [_ [_ [raw_payload_is_field _]]].
    cbn [Source.source_field] in raw_payload_is_field.
    destruct (Schema.descriptor_payload field) eqn:field_kind;
      try discriminate evaluated;
      repeat (match type of evaluated with
        | (match ?scrutinee with _ => _ end) = _ => destruct scrutinee
        end; try discriminate evaluated);
      inversion evaluated; subst child;
      change (Schema.raw_payload child_raw = Schema.Message) in message_kind;
      congruence.
Qed.

Print Assumptions source_empty_scan_agrees_with_target.
Print Assumptions source_child_guard_agrees_with_target.
Print Assumptions source_scan_step_agrees_with_target.
Print Assumptions source_scan_fold_agrees_with_target.
Print Assumptions source_scan_children_project_total.
Print Assumptions typeless_message_child_separates_scanners.
Print Assumptions well_formed_children_alone_do_not_force_agreement.
Print Assumptions evaluated_message_retains_typed_raw.
Print Assumptions source_evaluate_node_child_has_type_id.
