(** * Totality of the bind, name and PathMap families (proof leaf L17)

    Statement of the leaf: the semantic reducer returns an acceptable result
    on every typed-valid node of the six types 16 (ReceiveBind), 21
    (MatchCase), 31 (EPathMap), 32 (EZipper), 55 (Connective) and 60
    (GUnforgeable), given that every direct child already has an acceptable
    result.  These are the types [score_class_of] places in [ClassBindPayload],
    [ClassPathMapPayload] and [ClassNameComponent].

    Decomposition.  A node is refused (acceptable by its first disjunct) or it
    is not; an unrefused node resolves a generated program of its own family,
    its child scan is valid, and every child fits the descriptor its tag
    selects.  The six family lemmas below then discharge
    [class_result_ok]:

    - 31: every EPathMap is refused ([RPB3TypedValid.v]).
    - 32: the complete row is refused; the incomplete row has no score, which
      is the accepted row of [ClassPathMapPayload].
    - 16, 21: when the required children are present each one is an exact Par
      (or an exact Var or cost part); otherwise
      [bind_payload_required_child_absent] holds.
    - 55: every arm reads a child whose score the node can compute.  The Not
      arm reads an acceptable Par; the VarRef arm reads a typed-valid VarRef
      carrier; the scalar arms read a scalar checked by TV2.
    - 60: the arms read the raw body of a leaf.  TV3 restricts that body to
      the reread grammar, and the [bytes_leaf_has_exact_bytes] bridge
      discharges the reducer's own re-decoding of it through
      [RPB3RawRegionTree.decode_region] for every body (the sweep in
      [RPB3TypedValid.v] only covers bodies of bounded length).

    Deviation from the requested statement.  The And and Or arms of 55 read
    [exact_typed_message_score 57], so they need the ConnectiveBody carrier's
    score, which needs every Par element's score.  The hypothesis
    [Forall_forest acceptable children] gives nothing there: [score_class_of]
    puts 57 in [ClassParentCarrier], whose row of [class_result_ok] is [true]
    with no condition.  Rust has no such gap: [reduce_connective_body] collects
    [ParSummary] values (models/src/rust/rholang/protobuf_admission.rs, lines
    1160-1168) whose [score] field is not optional (lines 487-493), and the
    And/Or arm of [reduce_connective] reads that field (lines 2956-2971).  The
    model separates a carrier's acceptance from its score.  Hence the
    requested statement [name_and_pathmap_result_on_typed_valid] does not
    follow from its own hypotheses at those two arms of family 55.  It is not
    known to be false: a tree-level counterexample would have to be a
    typed-valid unrefused Par with no exact score, i.e. a failure of R3 itself,
    and none is known.  The closed witness
    [connective_arm_gap_at_summary_level] shows the local step failing on
    summaries.

    The theorems below therefore state:
    - the minimal strengthening
      [name_and_pathmap_result_on_typed_valid_given_exact_carriers], whose
      only extra hypothesis is required of type-55 nodes and concerns only
      their ConnectiveBody carriers (it is necessary by
      [connective_body_arm_needs_exact_carrier]);
    - the requested statement verbatim for the five other types
      ([name_and_pathmap_result_on_typed_valid_off_connective_family]);
    - the requested statement for 55 when no child is a carrier, which covers
      every arm but And/Or
      ([name_and_pathmap_result_on_typed_valid_off_connective_body_arms]);
    - the requested statement given the R3-level fact that typed-valid
      unrefused Pars are exact
      ([name_and_pathmap_result_on_typed_valid_given_deep_par_exactness]).

    The invariant a tree induction has to add, [connective_body_carrier_exact],
    is established at every node by
    [connective_body_carrier_exact_of_acceptable_children], from the
    acceptability of that node's own children, so it can be carried alongside
    acceptance. *)

From Stdlib Require Import List Bool Arith Lia NArith ZArith.
From StackSafePDA Require Import StackSafePDA RPB3GeneratedFieldRefinement
  RPB3SourceScalarReduction RPB3SemanticReducer RPB3ScoreClass RPB3TypedValid.
From StackSafePDA Require RPB3WireCursor RPB3RawRegionTree
  RPB3DescriptorProjectionData.

Import ListNotations.

(** ** Acceptance from refusal or from the class table *)

Lemma refused_result_is_acceptable :
  forall summary,
    sem_definitely_refused summary = true ->
    reducer_result_acceptable summary = true.
Proof.
  intros summary refused.
  unfold reducer_result_acceptable.
  rewrite refused.
  reflexivity.
Qed.

Lemma class_result_ok_makes_acceptable :
  forall summary,
    class_result_ok summary = true ->
    reducer_result_acceptable summary = true.
Proof.
  intros summary class_ok.
  unfold reducer_result_acceptable.
  rewrite class_ok.
  apply orb_true_r.
Qed.

Lemma acceptable_unrefused_summary_class_ok :
  forall summary,
    reducer_result_acceptable summary = true ->
    sem_definitely_refused summary = false ->
    payload_eqb (sem_payload summary) Scalar = false ->
    class_result_ok summary = true.
Proof.
  intros summary accepted unrefused not_scalar.
  unfold reducer_result_acceptable in accepted.
  rewrite unrefused, not_scalar in accepted.
  exact accepted.
Qed.

(** ** The classes of the types read here *)

Lemma score_class_of_par : score_class_of 1 = ClassPar.
Proof. vm_compute. reflexivity. Qed.

Lemma score_class_of_var : score_class_of 6 = ClassExprPayload.
Proof. vm_compute. reflexivity. Qed.

Lemma score_class_of_cost_region : score_class_of 12 = ClassCostPart.
Proof. vm_compute. reflexivity. Qed.

Lemma score_class_of_receive_bind : score_class_of 16 = ClassBindPayload.
Proof. vm_compute. reflexivity. Qed.

Lemma score_class_of_match_case : score_class_of 21 = ClassBindPayload.
Proof. vm_compute. reflexivity. Qed.

Lemma score_class_of_ezipper : score_class_of 32 = ClassPathMapPayload.
Proof. vm_compute. reflexivity. Qed.

Lemma score_class_of_connective : score_class_of 55 = ClassNameComponent.
Proof. vm_compute. reflexivity. Qed.

Lemma score_class_of_unforgeable : score_class_of 60 = ClassNameComponent.
Proof. vm_compute. reflexivity. Qed.

Lemma class_result_ok_of_bind_payload :
  forall summary,
    score_class_of (sem_type_id summary) = ClassBindPayload ->
    exact_evidence summary = true \/
      bind_payload_required_child_absent summary = true ->
    class_result_ok summary = true.
Proof.
  intros summary class_is [exact_true|absent].
  - unfold class_result_ok.
    rewrite class_is, exact_true.
    reflexivity.
  - unfold class_result_ok.
    rewrite class_is, absent.
    apply orb_true_r.
Qed.

Lemma class_result_ok_of_name_component :
  forall summary,
    score_class_of (sem_type_id summary) = ClassNameComponent ->
    sem_complete summary = Some true ->
    exact_evidence summary = true ->
    class_result_ok summary = true.
Proof.
  intros summary class_is complete exact_true.
  unfold class_result_ok.
  rewrite class_is, complete.
  exact exact_true.
Qed.

Lemma class_result_ok_of_incomplete_pathmap :
  forall summary,
    score_class_of (sem_type_id summary) = ClassPathMapPayload ->
    sem_complete summary = Some false ->
    sem_score_evidence summary = ScoreNotDerived ->
    class_result_ok summary = true.
Proof.
  intros summary class_is complete evidence.
  unfold class_result_ok.
  rewrite class_is, complete, evidence.
  reflexivity.
Qed.

Lemma bind_payload_required_child_absent_of_receive_bind :
  forall summary,
    sem_type_id summary = 16 ->
    sem_bind_source_present summary = false ->
    bind_payload_required_child_absent summary = true.
Proof.
  intros summary type_is absent.
  unfold bind_payload_required_child_absent.
  rewrite type_is, absent.
  reflexivity.
Qed.

Lemma bind_payload_required_child_absent_of_match_case :
  forall summary,
    sem_type_id summary = 21 ->
    sem_case_required_present summary = false ->
    bind_payload_required_child_absent summary = true.
Proof.
  intros summary type_is absent.
  unfold bind_payload_required_child_absent.
  rewrite type_is, absent.
  reflexivity.
Qed.

Lemma payload_eqb_true_implies_equal :
  forall left right,
    payload_eqb left right = true -> left = right.
Proof.
  intros left right same.
  destruct left, right; try reflexivity; discriminate same.
Qed.

(** ** Equations of [reduce_semantic_field] *)

Lemma reduce_semantic_field_score_evidence :
  forall field children program,
    generated_wire_program field = Some program ->
    sem_score_evidence (reduce_semantic_field field children) =
    score_evidence_for (program_family program)
      (component_completeness (program_family program)
        (fold_left (scan_child program) children empty_scan) children)
      children.
Proof.
  intros field children program resolved.
  unfold reduce_semantic_field.
  rewrite resolved.
  reflexivity.
Qed.

Lemma reduce_semantic_field_bind_source_present :
  forall field children program,
    generated_wire_program field = Some program ->
    sem_bind_source_present (reduce_semantic_field field children) =
    (if program_family program =? 16
     then seen_slot (fold_left (scan_child program) children empty_scan) 1
     else false).
Proof.
  intros field children program resolved.
  unfold reduce_semantic_field.
  rewrite resolved.
  reflexivity.
Qed.

Lemma reduce_semantic_field_case_required_present :
  forall field children program,
    generated_wire_program field = Some program ->
    sem_case_required_present (reduce_semantic_field field children) =
    (if program_family program =? 21
     then seen_slot (fold_left (scan_child program) children empty_scan) 0 &&
          seen_slot (fold_left (scan_child program) children empty_scan) 1
     else false).
Proof.
  intros field children program resolved.
  unfold reduce_semantic_field.
  rewrite resolved.
  reflexivity.
Qed.

Lemma unrefused_node_has_valid_scan :
  forall field children program,
    generated_wire_program field = Some program ->
    sem_definitely_refused (reduce_semantic_field field children) = false ->
    scan_valid (fold_left (scan_child program) children empty_scan) = true.
Proof.
  intros field children program resolved unrefused.
  destruct (scan_valid (fold_left (scan_child program) children empty_scan))
    eqn:valid.
  - reflexivity.
  - rewrite (invalid_scan_is_refused field children program resolved valid)
      in unrefused.
    discriminate unrefused.
Qed.

Lemma unrefused_node_is_resolved :
  forall field children,
    sem_definitely_refused (reduce_semantic_field field children) = false ->
    exists program, generated_wire_program field = Some program.
Proof.
  intros field children unrefused.
  destruct (generated_wire_program field) as [program|] eqn:resolved.
  - exists program. reflexivity.
  - rewrite (no_program_field_is_refused_by_the_current_reducer field children
      resolved) in unrefused.
    discriminate unrefused.
Qed.

Lemma unrefused_node_body_is_valid :
  forall field children,
    sem_definitely_refused (reduce_semantic_field field children) = false ->
    valid_raw_body field = true.
Proof.
  intros field children unrefused.
  destruct (valid_raw_body field) eqn:body_valid; [reflexivity|].
  exfalso.
  unfold reduce_semantic_field in unrefused.
  cbn [sem_definitely_refused] in unrefused.
  rewrite body_valid in unrefused.
  cbn [negb] in unrefused.
  rewrite orb_true_r in unrefused.
  cbn [orb] in unrefused.
  discriminate unrefused.
Qed.

Lemma reduce_semantic_field_no_program_type :
  forall field children,
    generated_wire_program field = None ->
    sem_type_id (reduce_semantic_field field children) = 0.
Proof.
  intros field children no_program.
  unfold reduce_semantic_field.
  rewrite no_program.
  reflexivity.
Qed.

Lemma semantic_reduce_exposes_field :
  forall subject,
    sem_tag (semantic_reduce subject) = raw_tag (tree_field subject) /\
    sem_wire (semantic_reduce subject) = raw_wire (tree_field subject) /\
    sem_payload (semantic_reduce subject) = raw_payload (tree_field subject) /\
    sem_body (semantic_reduce subject) = raw_body (tree_field subject).
Proof.
  intros [field children].
  repeat split; reflexivity.
Qed.

(** ** The child scan

    [scan_child_stays_valid_only_if] of [RPB3TypedValid.v] does not expose the
    wire comparison or the last slot, so the step is restated in full. *)

Definition child_fits (program : generated_program)
    (child : semantic_summary) : Prop :=
  exists descriptor,
    resolve_tag (program_fields program) (sem_tag child) = Some descriptor /\
    sem_definitely_refused child = false /\
    wire_eqb (sem_wire child) (descriptor_wire descriptor) = true /\
    payload_eqb (sem_payload child) (descriptor_payload descriptor) = true /\
    descriptor_accepts_child_type descriptor child = true.

Lemma scan_child_valid_step :
  forall program state child next,
    scan_child program state child = next ->
    scan_valid next = true ->
    exists descriptor,
      scan_valid state = true /\
      resolve_tag (program_fields program) (sem_tag child) = Some descriptor /\
      sem_definitely_refused child = false /\
      wire_eqb (sem_wire child) (descriptor_wire descriptor) = true /\
      payload_eqb (sem_payload child) (descriptor_payload descriptor) = true /\
      descriptor_accepts_child_type descriptor child = true /\
      slot_follows (scan_last_slot state) (descriptor_slot descriptor)
        (descriptor_repeated descriptor) = true /\
      scan_last_slot next = Some (descriptor_slot descriptor) /\
      scan_seen next = descriptor_slot descriptor :: scan_seen state.
Proof.
  intros program state child next stepped valid.
  unfold scan_child in stepped.
  destruct (scan_valid state) eqn:state_valid.
  2: { cbv beta iota in stepped.
       subst next.
       rewrite state_valid in valid.
       discriminate valid. }
  destruct (resolve_tag (program_fields program) (sem_tag child))
    as [descriptor|] eqn:resolved.
  2: { cbv beta iota in stepped.
       subst next.
       discriminate valid. }
  cbv beta iota zeta in stepped.
  match type of stepped with
  | (if ?test then _ else _) = _ => destruct test eqn:admissible
  end.
  2: { cbv beta iota in stepped.
       subst next.
       discriminate valid. }
  cbv beta iota in stepped.
  apply andb_prop in admissible as [first_six unique_ok].
  apply andb_prop in first_six as [first_five follows_ok].
  apply andb_prop in first_five as [first_four range_ok].
  apply andb_prop in first_four as [first_three type_ok].
  apply andb_prop in first_three as [first_two payload_ok].
  apply andb_prop in first_two as [not_refused wire_ok].
  subst next.
  exists descriptor.
  split; [first [exact state_valid|reflexivity]|].
  split; [first [exact resolved|reflexivity]|].
  split; [exact (proj1 (negb_true_iff _) not_refused)|].
  split; [exact wire_ok|].
  split; [exact payload_ok|].
  split; [exact type_ok|].
  split; [exact follows_ok|].
  split; reflexivity.
Qed.

Lemma fold_scan_valid_after_step :
  forall program state child rest,
    scan_valid (fold_left (scan_child program) (child :: rest) state) = true ->
    scan_valid (scan_child program state child) = true.
Proof.
  intros program state child rest valid.
  destruct (scan_valid (scan_child program state child)) eqn:step_valid;
    [reflexivity|].
  exfalso.
  cbn [fold_left] in valid.
  rewrite (fold_scan_keeps_invalid program rest _ step_valid) in valid.
  rewrite step_valid in valid.
  discriminate valid.
Qed.

Lemma scanned_children_fit :
  forall program children state,
    scan_valid (fold_left (scan_child program) children state) = true ->
    forall child, In child children -> child_fits program child.
Proof.
  intros program children.
  induction children as [|head rest IH]; intros state valid child member.
  - destruct member.
  - pose proof (fold_scan_valid_after_step program state head rest valid)
      as step_valid.
    destruct member as [here|later].
    + subst child.
      destruct (scan_child_valid_step program state head _ eq_refl step_valid)
        as [descriptor [_ [resolved [unrefused [wire_ok [payload_ok
          [type_ok _]]]]]]].
      exists descriptor.
      repeat split; assumption.
    + cbn [fold_left] in valid.
      exact (IH _ valid child later).
Qed.

Lemma resolve_tag_has_requested_tag :
  forall fields tag descriptor,
    resolve_tag fields tag = Some descriptor ->
    descriptor_tag descriptor = tag.
Proof.
  induction fields as [|field rest IH]; intros tag descriptor resolved.
  - discriminate resolved.
  - cbn [resolve_tag] in resolved.
    destruct (descriptor_tag field =? tag) eqn:same.
    + inversion resolved; subst descriptor.
      apply Nat.eqb_eq.
      exact same.
    + exact (IH tag descriptor resolved).
Qed.

Lemma seen_slot_has_child :
  forall program children state slot,
    scan_valid (fold_left (scan_child program) children state) = true ->
    seen_slot (fold_left (scan_child program) children state) slot = true ->
    seen_slot state slot = true \/
    exists child descriptor,
      In child children /\
      resolve_tag (program_fields program) (sem_tag child) = Some descriptor /\
      descriptor_slot descriptor = slot.
Proof.
  intros program children.
  induction children as [|head rest IH]; intros state slot valid seen.
  - left. exact seen.
  - pose proof (fold_scan_valid_after_step program state head rest valid)
      as step_valid.
    cbn [fold_left] in valid, seen.
    destruct (IH (scan_child program state head) slot valid seen)
      as [seen_step|[child [descriptor [member [resolved same_slot]]]]].
    + destruct (scan_child_valid_step program state head _ eq_refl step_valid)
        as [head_descriptor [_ [head_resolved [_ [_ [_ [_ [_ [_ head_seen]]]]]]]]].
      unfold seen_slot in seen_step.
      rewrite head_seen in seen_step.
      cbn [existsb] in seen_step.
      apply orb_prop in seen_step as [same|earlier].
      * right.
        exists head, head_descriptor.
        split; [left; reflexivity|].
        split; [exact head_resolved|].
        apply Nat.eqb_eq in same.
        symmetry. exact same.
      * left. exact earlier.
    + right.
      exists child, descriptor.
      split; [right; exact member|].
      split; [exact resolved|exact same_slot].
Qed.

Definition slot_holds_tag (program : generated_program)
    (slot tag : nat) : bool :=
  forallb (fun descriptor =>
    negb (descriptor_slot descriptor =? slot) ||
    (descriptor_tag descriptor =? tag)) (program_fields program).

Lemma seen_slot_child_has_tag :
  forall program children slot tag,
    slot_holds_tag program slot tag = true ->
    scan_valid (fold_left (scan_child program) children empty_scan) = true ->
    seen_slot (fold_left (scan_child program) children empty_scan) slot = true ->
    exists child, In child children /\ sem_tag child = tag.
Proof.
  intros program children slot tag table valid seen.
  destruct (seen_slot_has_child program children empty_scan slot valid seen)
    as [impossible|[child [descriptor [member [resolved same_slot]]]]].
  - unfold seen_slot in impossible.
    cbn in impossible.
    discriminate impossible.
  - exists child.
    split; [exact member|].
    pose proof (proj1 (forallb_forall _ _) table descriptor
      (resolve_tag_member _ _ _ resolved)) as here.
    cbv beta in here.
    rewrite same_slot, Nat.eqb_refl in here.
    cbn [negb orb] in here.
    apply Nat.eqb_eq in here.
    rewrite <- here.
    symmetry.
    exact (resolve_tag_has_requested_tag _ _ _ resolved).
Qed.

Definition all_fields_in_slot_zero (program : generated_program) : bool :=
  forallb (fun descriptor =>
    (descriptor_slot descriptor =? 0) && negb (descriptor_repeated descriptor))
    (program_fields program).

Lemma all_fields_in_slot_zero_facts :
  forall program,
    all_fields_in_slot_zero program = true ->
    forall descriptor,
      In descriptor (program_fields program) ->
      descriptor_slot descriptor = 0 /\ descriptor_repeated descriptor = false.
Proof.
  intros program table descriptor member.
  pose proof (proj1 (forallb_forall _ _) table descriptor member) as here.
  cbv beta in here.
  apply andb_prop in here as [slot_zero not_repeated].
  apply Nat.eqb_eq in slot_zero.
  apply negb_true_iff in not_repeated.
  exact (conj slot_zero not_repeated).
Qed.

(** A oneof whose arms all share slot zero has at most one child: the
    second child would have to follow slot zero with a non-repeated field. *)
Lemma slot_zero_scan_has_at_most_one_child :
  forall program children,
    all_fields_in_slot_zero program = true ->
    scan_valid (fold_left (scan_child program) children empty_scan) = true ->
    children = [] \/ exists child, children = [child].
Proof.
  intros program children table valid.
  destruct children as [|first [|second rest]].
  - left. reflexivity.
  - right. exists first. reflexivity.
  - exfalso.
    pose proof (fold_scan_valid_after_step program empty_scan first
      (second :: rest) valid) as first_valid.
    assert (after_first :
      scan_valid (fold_left (scan_child program) (second :: rest)
        (scan_child program empty_scan first)) = true) by exact valid.
    pose proof (fold_scan_valid_after_step program
      (scan_child program empty_scan first) second rest after_first)
      as second_valid.
    destruct (scan_child_valid_step program empty_scan first _ eq_refl
      first_valid) as [first_descriptor [_ [first_resolved
        [_ [_ [_ [_ [_ [first_last _]]]]]]]]].
    destruct (scan_child_valid_step program
      (scan_child program empty_scan first) second _ eq_refl second_valid)
      as [second_descriptor [_ [second_resolved
        [_ [_ [_ [_ [second_follows _]]]]]]]].
    destruct (all_fields_in_slot_zero_facts program table first_descriptor
      (resolve_tag_member _ _ _ first_resolved)) as [first_zero _].
    destruct (all_fields_in_slot_zero_facts program table second_descriptor
      (resolve_tag_member _ _ _ second_resolved))
      as [second_zero second_single].
    rewrite first_last, first_zero, second_zero, second_single
      in second_follows.
    vm_compute in second_follows.
    discriminate second_follows.
Qed.

(** ** Facts about children, from the forest *)

Lemma forall_forest_member :
  forall (P : @tree raw_field -> Prop) (children : @forest raw_field) child,
    Forall_forest P children ->
    In child (semantic_reduce_forest children) ->
    exists subject,
      child = semantic_reduce subject /\
      P subject /\
      In (tree_field subject) (forest_fields children).
Proof.
  intros P children child all.
  induction all as [|subject rest head_holds rest_holds IH]; intro member.
  - cbn [semantic_reduce_forest In] in member.
    destruct member.
  - cbn [semantic_reduce_forest In] in member.
    destruct member as [here|later].
    + exists subject.
      split; [symmetry; exact here|].
      split; [exact head_holds|left; reflexivity].
    + destruct (IH later) as [found [same [found_holds found_member]]].
      exists found.
      split; [exact same|].
      split; [exact found_holds|right; exact found_member].
Qed.

Lemma forall_forest_conj :
  forall (P Q : @tree raw_field -> Prop) (children : @forest raw_field),
    Forall_forest P children ->
    Forall_forest Q children ->
    Forall_forest (fun subject => P subject /\ Q subject) children.
Proof.
  intros P Q children has_p.
  induction has_p as [|subject rest head_p rest_p IH]; intro has_q.
  - constructor.
  - inversion has_q as [|subject' rest' head_q rest_q]; subst.
    constructor.
    + split; assumption.
    + apply IH. assumption.
Qed.

Lemma forall_forest_impl :
  forall (P Q : @tree raw_field -> Prop) (children : @forest raw_field),
    (forall subject, P subject -> Q subject) ->
    Forall_forest P children ->
    Forall_forest Q children.
Proof.
  intros P Q children impl all.
  induction all as [|subject rest head_p rest_p IH].
  - constructor.
  - constructor.
    + apply impl. exact head_p.
    + exact IH.
Qed.

Lemma forest_member_is_accepted :
  forall (children : @forest raw_field) child,
    Forall_forest (fun subject =>
      reducer_result_acceptable (semantic_reduce subject) = true) children ->
    In child (semantic_reduce_forest children) ->
    reducer_result_acceptable child = true.
Proof.
  intros children child accepted member.
  destruct (forall_forest_member _ children child accepted member)
    as [subject [same [holds _]]].
  rewrite same.
  exact holds.
Qed.

Lemma typed_valid_node_children :
  forall field children,
    typed_valid_tree (Node field children) ->
    Forall_forest typed_valid_tree children.
Proof.
  intros field children valid.
  destruct (proj1 (typed_valid_tree_node_iff field children) valid)
    as [_ [_ [_ children_valid]]].
  exact children_valid.
Qed.

(** ** Scores from child evidence *)

Lemma exact_evidence_gives_score :
  forall child,
    exact_evidence child = true ->
    exists score, sem_score_evidence child = ExactScore score.
Proof.
  intros child exact_true.
  unfold exact_evidence in exact_true.
  destruct (sem_score_evidence child) as [score|].
  - exists score. reflexivity.
  - discriminate exact_true.
Qed.

Lemma exact_evidence_of_score :
  forall summary score,
    sem_score_evidence summary = ExactScore score ->
    exact_evidence summary = true.
Proof.
  intros summary score evidence.
  unfold exact_evidence.
  rewrite evidence.
  reflexivity.
Qed.

Lemma typed_message_score_of_evidence :
  forall type_id child score,
    sem_type_id child = type_id ->
    sem_wire child = LengthDelimited ->
    sem_payload child = Message ->
    sem_definitely_refused child = false ->
    sem_score_evidence child = ExactScore score ->
    exact_typed_message_score type_id child = Some score.
Proof.
  intros type_id child score type_is wire_is payload_is unrefused evidence.
  unfold exact_typed_message_score.
  rewrite type_is, wire_is, payload_is, unrefused, evidence, Nat.eqb_refl.
  reflexivity.
Qed.

Lemma typed_message_score_of_exact :
  forall type_id child,
    sem_type_id child = type_id ->
    sem_wire child = LengthDelimited ->
    sem_payload child = Message ->
    sem_definitely_refused child = false ->
    exact_evidence child = true ->
    exists score, exact_typed_message_score type_id child = Some score.
Proof.
  intros type_id child type_is wire_is payload_is unrefused exact_true.
  destruct (exact_evidence_gives_score child exact_true) as [score evidence].
  exists score.
  exact (typed_message_score_of_evidence type_id child score type_is wire_is
    payload_is unrefused evidence).
Qed.

Lemma find_tag_selects_member :
  forall tag children child,
    find (fun candidate => sem_tag candidate =? tag) children = Some child ->
    In child children /\ sem_tag child = tag.
Proof.
  intros tag children child selected.
  destruct (find_some _ _ selected) as [member matched].
  cbv beta in matched.
  apply Nat.eqb_eq in matched.
  exact (conj member matched).
Qed.

Lemma exact_typed_child_score_exists :
  forall tag type_id children,
    (exists child, In child children /\ sem_tag child = tag) ->
    (forall child, In child children -> sem_tag child = tag ->
       exists score, exact_typed_message_score type_id child = Some score) ->
    exists score, exact_typed_child_score tag type_id children = Some score.
Proof.
  intros tag type_id children [witness [witness_member witness_tag]] each.
  unfold exact_typed_child_score.
  destruct (find (fun candidate => sem_tag candidate =? tag) children)
    as [selected|] eqn:found.
  - destruct (find_tag_selects_member tag children selected found)
      as [selected_member selected_tag].
    exact (each selected selected_member selected_tag).
  - exfalso.
    pose proof (find_none _ _ found witness witness_member) as mismatch.
    cbv beta in mismatch.
    rewrite witness_tag, Nat.eqb_refl in mismatch.
    discriminate mismatch.
Qed.

Lemma exact_typed_selected_scores_exists :
  forall tag type_id children,
    (forall child, In child children -> sem_tag child = tag ->
       exists score, exact_typed_message_score type_id child = Some score) ->
    exists scores, exact_typed_selected_scores tag type_id children = Some scores.
Proof.
  intros tag type_id children.
  induction children as [|head rest IH]; intro each.
  - exists []. reflexivity.
  - destruct IH as [scores later].
    { intros child member same.
      apply each; [right; exact member|exact same]. }
    cbn [exact_typed_selected_scores].
    rewrite later.
    destruct (sem_tag head =? tag) eqn:head_tag.
    + destruct (each head (or_introl eq_refl) (proj1 (Nat.eqb_eq _ _) head_tag))
        as [score head_score].
      rewrite head_score.
      cbn [option_map].
      exists (score :: scores).
      reflexivity.
    + exists scores.
      reflexivity.
Qed.

Lemma exact_typed_optional_child_score_exists :
  forall tag type_id children,
    (forall child, In child children -> sem_tag child = tag ->
       exists score, exact_typed_message_score type_id child = Some score) ->
    exists result, exact_typed_optional_child_score tag type_id children = Some result.
Proof.
  intros tag type_id children each.
  unfold exact_typed_optional_child_score.
  destruct (find (fun candidate => sem_tag candidate =? tag) children)
    as [selected|] eqn:found.
  - destruct (find_tag_selects_member tag children selected found)
      as [selected_member selected_tag].
    destruct (each selected selected_member selected_tag) as [score score_found].
    cbv beta iota.
    rewrite score_found.
    exists (Some score).
    reflexivity.
  - exists None.
    reflexivity.
Qed.

Lemma exact_typed_optional_score_or_exists :
  forall tag type_id fallback children,
    (forall child, In child children -> sem_tag child = tag ->
       exists score, exact_typed_message_score type_id child = Some score) ->
    exists score,
      exact_typed_optional_score_or tag type_id fallback children = Some score.
Proof.
  intros tag type_id fallback children each.
  destruct (exact_typed_optional_child_score_exists tag type_id children each)
    as [result found].
  unfold exact_typed_optional_score_or.
  rewrite found.
  destruct result as [score|].
  - exists score. reflexivity.
  - exists fallback. reflexivity.
Qed.

Lemma exact_receive_bind_score_exists :
  forall children,
    (exists source, exact_typed_child_score 2 1 children = Some source) ->
    (exists patterns, exact_typed_selected_scores 1 1 children = Some patterns) ->
    (exists remainder,
       exact_typed_optional_score_or 3 6 (ScoreInteger 0) children = Some remainder) ->
    (exists cost, exact_typed_optional_child_score 5 12 children = Some cost) ->
    exists score, exact_receive_bind_score children = Some score.
Proof.
  intros children [source source_found] [patterns patterns_found]
    [remainder remainder_found] [cost cost_found].
  unfold exact_receive_bind_score.
  rewrite source_found, patterns_found, remainder_found, cost_found.
  eexists. reflexivity.
Qed.

Lemma exact_match_case_score_exists :
  forall children,
    (exists pattern, exact_typed_child_score 1 1 children = Some pattern) ->
    (exists source, exact_typed_child_score 2 1 children = Some source) ->
    (exists free_count, exact_pbint32_or_absent 3 children = Some free_count) ->
    (exists guard,
       exact_typed_optional_score_or 4 1 empty_par_score children = Some guard) ->
    exists score, exact_match_case_score children = Some score.
Proof.
  intros children [pattern pattern_found] [source source_found]
    [free_count free_found] [guard guard_found].
  unfold exact_match_case_score.
  rewrite pattern_found, source_found, free_found, guard_found.
  eexists. reflexivity.
Qed.

(** The [Some (ScalarI32 _)] shape of the checked integer decoders. *)
Lemma exact_pbint32_or_absent_exists :
  forall children,
    (forall child, In child children -> sem_tag child = 3 ->
       sem_wire child = Varint /\
       sem_payload child = Scalar /\
       sem_definitely_refused child = false /\
       exists value,
         scalar_semantic_from_source
           (RPB3SourceScalarReduction.ScalarRule 3
             RPB3DescriptorProjectionData.GeneratedData.PBInt32 true)
           (sem_body child) = Some (RPB3SourceScalarReduction.ScalarI32 value)) ->
    exists value, exact_pbint32_or_absent 3 children = Some value.
Proof.
  intros children each.
  unfold exact_pbint32_or_absent.
  destruct (find (fun child => sem_tag child =? 3) children) as [child|] eqn:found.
  - destruct (find_tag_selects_member 3 children child found) as [member same].
    destruct (each child member same)
      as [wire_is [payload_is [unrefused [value decoded]]]].
    cbv beta iota.
    rewrite wire_is, payload_is, unrefused.
    cbv beta iota.
    rewrite decoded.
    exists value.
    reflexivity.
  - exists 0%Z.
    reflexivity.
Qed.

Lemma exact_i32_or_absent_exists :
  forall tag children,
    (forall child, In child children -> sem_tag child = tag ->
       exists value,
         scalar_semantic_from_source
           (RPB3SourceScalarReduction.ScalarRule tag
             RPB3DescriptorProjectionData.GeneratedData.PBSint32 true)
           (sem_body child) = Some (RPB3SourceScalarReduction.ScalarI32 value)) ->
    exists value, exact_i32_or_absent tag children = Some value.
Proof.
  intros tag children each.
  unfold exact_i32_or_absent.
  destruct (find (fun child => sem_tag child =? tag) children)
    as [child|] eqn:found.
  - destruct (find_tag_selects_member tag children child found)
      as [member same].
    destruct (each child member same) as [value decoded].
    cbv beta iota.
    rewrite decoded.
    exists value.
    reflexivity.
  - exists 0%Z.
    reflexivity.
Qed.

(** ** The decoded value of a valid scalar body *)

Lemma pbbool_scalar_has_value :
  forall tag forbidden body,
    scalar_body_valid
      (RPB3SourceScalarReduction.ScalarRule tag
        RPB3DescriptorProjectionData.GeneratedData.PBBool forbidden) body = true ->
    exists value,
      scalar_semantic_from_source
        (RPB3SourceScalarReduction.ScalarRule tag
          RPB3DescriptorProjectionData.GeneratedData.PBBool forbidden) body =
      Some (RPB3SourceScalarReduction.ScalarBool value).
Proof.
  intros tag forbidden body valid.
  unfold scalar_semantic_from_source.
  rewrite valid.
  unfold scalar_body_valid in valid.
  simpl in valid |- *.
  destruct (compact_varint_value body) as [number|] eqn:parsed;
    [|discriminate valid].
  eexists. reflexivity.
Qed.

Lemma pbint32_scalar_has_value :
  forall tag forbidden body,
    scalar_body_valid
      (RPB3SourceScalarReduction.ScalarRule tag
        RPB3DescriptorProjectionData.GeneratedData.PBInt32 forbidden) body = true ->
    exists value,
      scalar_semantic_from_source
        (RPB3SourceScalarReduction.ScalarRule tag
          RPB3DescriptorProjectionData.GeneratedData.PBInt32 forbidden) body =
      Some (RPB3SourceScalarReduction.ScalarI32 value).
Proof.
  intros tag forbidden body valid.
  unfold scalar_semantic_from_source.
  rewrite valid.
  unfold scalar_body_valid in valid.
  simpl in valid |- *.
  destruct (compact_varint_value body) as [number|] eqn:parsed;
    [|discriminate valid].
  eexists. reflexivity.
Qed.

Lemma pbsint32_scalar_has_value :
  forall tag forbidden body,
    scalar_body_valid
      (RPB3SourceScalarReduction.ScalarRule tag
        RPB3DescriptorProjectionData.GeneratedData.PBSint32 forbidden) body = true ->
    exists value,
      scalar_semantic_from_source
        (RPB3SourceScalarReduction.ScalarRule tag
          RPB3DescriptorProjectionData.GeneratedData.PBSint32 forbidden) body =
      Some (RPB3SourceScalarReduction.ScalarI32 value).
Proof.
  intros tag forbidden body valid.
  unfold scalar_semantic_from_source.
  rewrite valid.
  unfold scalar_body_valid in valid.
  simpl in valid |- *.
  destruct (compact_varint_value body) as [number|] eqn:parsed;
    [|discriminate valid].
  eexists. reflexivity.
Qed.

Lemma forbidden_bytes_scalar_has_value :
  forall tag payload,
    0 < length payload ->
    scalar_semantic_from_source
      (RPB3SourceScalarReduction.ScalarRule tag
        RPB3DescriptorProjectionData.GeneratedData.PBBytes true) payload =
    Some (RPB3SourceScalarReduction.ScalarBytes payload).
Proof.
  intros tag [|first rest] nonempty.
  - cbn [length] in nonempty.
    lia.
  - reflexivity.
Qed.

(** TV2: the body of a scalar child passes the rule its parent generates for
    the child's tag. *)
Lemma scalar_child_body_valid :
  forall field children program subject rule,
    typed_valid_tree (Node field children) ->
    generated_wire_program field = Some program ->
    In (tree_field subject) (forest_fields children) ->
    raw_payload (tree_field subject) = Scalar ->
    generated_scalar_rule program (raw_tag (tree_field subject)) = Some rule ->
    scalar_body_valid rule (raw_body (tree_field subject)) = true.
Proof.
  intros field children program subject rule valid resolved member scalar rule_is.
  pose proof (typed_valid_tree_scalar_child_contract field children program
    (tree_field subject) valid resolved member) as contract.
  unfold scalar_contract_holds in contract.
  rewrite scalar in contract.
  cbv beta iota in contract.
  destruct (resolve_tag (program_fields program) (raw_tag (tree_field subject)))
    as [descriptor|] eqn:found.
  2: { cbv beta iota in contract. discriminate contract. }
  rewrite rule_is in contract.
  cbv beta iota in contract.
  apply andb_prop in contract as [front _].
  apply andb_prop in front as [_ body_valid].
  exact body_valid.
Qed.

(** ** Table lookups on generated descriptors

    The tables below are finite and closed, so the facts about each concrete
    field are proved once by [vm_compute]. *)

Definition message_field_at (program : generated_program)
    (tag type_id : nat) : bool :=
  match resolve_tag (program_fields program) tag with
  | Some descriptor =>
      wire_eqb (descriptor_wire descriptor) LengthDelimited &&
      payload_eqb (descriptor_payload descriptor) Message &&
      match descriptor_child_type_id descriptor with
      | Some declared => declared =? type_id
      | None => false
      end
  | None => false
  end.

Lemma message_child_facts :
  forall program child tag type_id,
    child_fits program child ->
    sem_tag child = tag ->
    message_field_at program tag type_id = true ->
    sem_definitely_refused child = false /\
    sem_wire child = LengthDelimited /\
    sem_payload child = Message /\
    sem_type_id child = type_id.
Proof.
  intros program child tag type_id
    [descriptor [resolved [unrefused [wire_ok [payload_ok type_ok]]]]]
    same_tag table.
  rewrite same_tag in resolved.
  unfold message_field_at in table.
  rewrite resolved in table.
  cbv beta iota in table.
  apply andb_prop in table as [front child_type_ok].
  apply andb_prop in front as [wire_field payload_field].
  pose proof (payload_eqb_true_implies_equal _ _ payload_field) as payload_is_message.
  split; [exact unrefused|].
  split.
  { rewrite (wire_eqb_true_implies_equal _ _ wire_ok).
    exact (wire_eqb_true_implies_equal _ _ wire_field). }
  split.
  { rewrite (payload_eqb_true_implies_equal _ _ payload_ok).
    exact payload_is_message. }
  unfold descriptor_accepts_child_type in type_ok.
  rewrite payload_is_message in type_ok.
  destruct (descriptor_child_type_id descriptor) as [declared|].
  2: { discriminate type_ok. }
  cbv beta iota in type_ok, child_type_ok.
  apply Nat.eqb_eq in type_ok.
  apply Nat.eqb_eq in child_type_ok.
  congruence.
Qed.

Definition scalar_field_at (program : generated_program)
    (tag : nat) (wire : wire_kind) : bool :=
  match resolve_tag (program_fields program) tag with
  | Some descriptor =>
      wire_eqb (descriptor_wire descriptor) wire &&
      payload_eqb (descriptor_payload descriptor) Scalar
  | None => false
  end.

Lemma scalar_child_facts :
  forall program child tag wire,
    child_fits program child ->
    sem_tag child = tag ->
    scalar_field_at program tag wire = true ->
    sem_definitely_refused child = false /\
    sem_wire child = wire /\
    sem_payload child = Scalar.
Proof.
  intros program child tag wire
    [descriptor [resolved [unrefused [wire_ok [payload_ok _]]]]]
    same_tag table.
  rewrite same_tag in resolved.
  unfold scalar_field_at in table.
  rewrite resolved in table.
  cbv beta iota in table.
  apply andb_prop in table as [wire_field payload_field].
  split; [exact unrefused|].
  split.
  - rewrite (wire_eqb_true_implies_equal _ _ wire_ok).
    exact (wire_eqb_true_implies_equal _ _ wire_field).
  - rewrite (payload_eqb_true_implies_equal _ _ payload_ok).
    exact (payload_eqb_true_implies_equal _ _ payload_field).
Qed.

Lemma fitted_tag_in_list :
  forall program child tags,
    forallb (fun descriptor => listed_score_type (descriptor_tag descriptor) tags)
      (program_fields program) = true ->
    child_fits program child ->
    In (sem_tag child) tags.
Proof.
  intros program child tags table [descriptor [resolved _]].
  pose proof (proj1 (forallb_forall _ _) table descriptor
    (resolve_tag_member _ _ _ resolved)) as here.
  cbv beta in here.
  apply (proj1 (listed_score_type_iff _ _)) in here.
  rewrite (resolve_tag_has_requested_tag _ _ _ resolved) in here.
  exact here.
Qed.

Lemma accepted_par_field_has_score :
  forall program (children : @forest raw_field) tag child,
    (forall candidate, In candidate (semantic_reduce_forest children) ->
       child_fits program candidate) ->
    Forall_forest (fun subject =>
      reducer_result_acceptable (semantic_reduce subject) = true) children ->
    message_field_at program tag 1 = true ->
    In child (semantic_reduce_forest children) ->
    sem_tag child = tag ->
    exists score, exact_typed_message_score 1 child = Some score.
Proof.
  intros program children tag child fits accepted table member same_tag.
  destruct (message_child_facts program child tag 1 (fits child member)
    same_tag table) as [unrefused [wire_is [payload_is type_is]]].
  pose proof (forest_member_is_accepted children child accepted member)
    as child_accepted.
  pose proof (acceptable_unrefused_summary_class_ok child child_accepted
    unrefused (ltac:(rewrite payload_is; reflexivity))) as class_ok.
  unfold class_result_ok in class_ok.
  rewrite type_is, score_class_of_par in class_ok.
  exact (typed_message_score_of_exact 1 child type_is wire_is payload_is
    unrefused class_ok).
Qed.

Lemma par_child_is_exact :
  forall child,
    reducer_result_acceptable child = true ->
    sem_definitely_refused child = false ->
    sem_payload child = Message ->
    sem_type_id child = 1 ->
    exact_evidence child = true.
Proof.
  intros child accepted unrefused payload_is type_is.
  pose proof (acceptable_unrefused_summary_class_ok child accepted unrefused
    (ltac:(rewrite payload_is; reflexivity))) as class_ok.
  unfold class_result_ok in class_ok.
  rewrite type_is, score_class_of_par in class_ok.
  exact class_ok.
Qed.

Lemma cost_child_is_exact :
  forall child,
    reducer_result_acceptable child = true ->
    sem_definitely_refused child = false ->
    sem_payload child = Message ->
    sem_type_id child = 12 ->
    exact_evidence child = true.
Proof.
  intros child accepted unrefused payload_is type_is.
  pose proof (acceptable_unrefused_summary_class_ok child accepted unrefused
    (ltac:(rewrite payload_is; reflexivity))) as class_ok.
  unfold class_result_ok in class_ok.
  rewrite type_is, score_class_of_cost_region in class_ok.
  exact class_ok.
Qed.

Lemma var_child_is_exact :
  forall child,
    reducer_result_acceptable child = true ->
    sem_definitely_refused child = false ->
    sem_payload child = Message ->
    sem_type_id child = 6 ->
    sem_complete child = Some true ->
    exact_evidence child = true.
Proof.
  intros child accepted unrefused payload_is type_is complete.
  pose proof (acceptable_unrefused_summary_class_ok child accepted unrefused
    (ltac:(rewrite payload_is; reflexivity))) as class_ok.
  unfold class_result_ok in class_ok.
  rewrite type_is, score_class_of_var in class_ok.
  rewrite complete in class_ok.
  unfold exact_evidence.
  destruct (sem_score_evidence child) as [score|].
  - reflexivity.
  - cbv beta iota in class_ok.
    discriminate class_ok.
Qed.

Lemma var_summary_is_complete :
  forall subject,
    sem_definitely_refused (semantic_reduce subject) = false ->
    sem_type_id (semantic_reduce subject) = 6 ->
    sem_complete (semantic_reduce subject) = Some true.
Proof.
  intros [field grandchildren] unrefused type_is.
  cbn [semantic_reduce] in unrefused, type_is |- *.
  destruct (unrefused_node_is_resolved field _ unrefused) as [program resolved].
  rewrite (reduce_semantic_field_type_id field _ program resolved) in type_is.
  rewrite (reduce_semantic_field_complete field _ program resolved), type_is.
  reflexivity.
Qed.

(** ** Family 16 *)

Lemma score_evidence_for_receive_bind :
  forall state children,
    score_evidence_for 16 (component_completeness 16 state children) children =
    exact_or_unknown (exact_receive_bind_score children).
Proof. intros state children. reflexivity. Qed.

Lemma receive_bind_tables :
  slot_holds_tag (generated_program_of 16) 1 2 = true /\
  message_field_at (generated_program_of 16) 1 1 = true /\
  message_field_at (generated_program_of 16) 2 1 = true /\
  message_field_at (generated_program_of 16) 3 6 = true /\
  message_field_at (generated_program_of 16) 5 12 = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Lemma family_16_class_ok :
  forall field children program,
    generated_wire_program field = Some program ->
    program_family program = 16 ->
    Forall_forest (fun subject =>
      reducer_result_acceptable (semantic_reduce subject) = true) children ->
    sem_definitely_refused
      (reduce_semantic_field field (semantic_reduce_forest children)) = false ->
    class_result_ok
      (reduce_semantic_field field (semantic_reduce_forest children)) = true.
Proof.
  intros field children program resolved family accepted unrefused.
  pose proof (generated_program_is_lookup_of_its_family field program 16
    resolved family (ltac:(discriminate) : 16 <> 0)) as same_program.
  subst program.
  pose proof (unrefused_node_has_valid_scan field _ _ resolved unrefused)
    as scan_ok.
  pose proof (scanned_children_fit _ _ _ scan_ok) as fits.
  destruct receive_bind_tables
    as [slot_table [table_one [table_two [table_three table_five]]]].
  apply class_result_ok_of_bind_payload.
  { rewrite (reduce_semantic_field_type_id field _ _ resolved), family.
    apply score_class_of_receive_bind. }
  destruct (seen_slot (fold_left (scan_child (generated_program_of 16))
      (semantic_reduce_forest children) empty_scan) 1) eqn:seen.
  - left.
    destruct (exact_typed_child_score_exists 2 1
      (semantic_reduce_forest children)) as [source source_found].
    { exact (seen_slot_child_has_tag (generated_program_of 16) _ 1 2
        slot_table scan_ok seen). }
    { intros child member same_tag.
      exact (accepted_par_field_has_score _ children 2 child fits accepted
        table_two member same_tag). }
    destruct (exact_typed_selected_scores_exists 1 1
      (semantic_reduce_forest children)) as [patterns patterns_found].
    { intros child member same_tag.
      exact (accepted_par_field_has_score _ children 1 child fits accepted
        table_one member same_tag). }
    destruct (exact_typed_optional_score_or_exists 3 6 (ScoreInteger 0)
      (semantic_reduce_forest children)) as [remainder remainder_found].
    { intros child member same_tag.
      destruct (message_child_facts _ child 3 6 (fits child member) same_tag
        table_three) as [child_unrefused [child_wire [child_payload child_type]]].
      destruct (semantic_reduce_forest_member children child member)
        as [subject same_subject].
      assert (complete : sem_complete child = Some true).
      { rewrite same_subject in child_unrefused, child_type |- *.
        exact (var_summary_is_complete subject child_unrefused child_type). }
      pose proof (forest_member_is_accepted children child accepted member)
        as child_accepted.
      pose proof (var_child_is_exact child child_accepted child_unrefused
        child_payload child_type complete) as exact_true.
      exact (typed_message_score_of_exact 6 child child_type child_wire
        child_payload child_unrefused exact_true). }
    destruct (exact_typed_optional_child_score_exists 5 12
      (semantic_reduce_forest children)) as [cost cost_found].
    { intros child member same_tag.
      destruct (message_child_facts _ child 5 12 (fits child member) same_tag
        table_five) as [child_unrefused [child_wire [child_payload child_type]]].
      pose proof (forest_member_is_accepted children child accepted member)
        as child_accepted.
      pose proof (cost_child_is_exact child child_accepted child_unrefused
        child_payload child_type) as exact_true.
      exact (typed_message_score_of_exact 12 child child_type child_wire
        child_payload child_unrefused exact_true). }
    destruct (exact_receive_bind_score_exists (semantic_reduce_forest children)
      (ex_intro _ source source_found)
      (ex_intro _ patterns patterns_found)
      (ex_intro _ remainder remainder_found)
      (ex_intro _ cost cost_found)) as [score found].
    apply (exact_evidence_of_score _ score).
    rewrite (reduce_semantic_field_score_evidence field _ _ resolved), family,
      score_evidence_for_receive_bind, found.
    reflexivity.
  - right.
    apply bind_payload_required_child_absent_of_receive_bind.
    + rewrite (reduce_semantic_field_type_id field _ _ resolved).
      exact family.
    + rewrite (reduce_semantic_field_bind_source_present field _ _ resolved),
        family.
      exact seen.
Qed.

(** ** Family 21 *)

Lemma score_evidence_for_match_case :
  forall state children,
    score_evidence_for 21 (component_completeness 21 state children) children =
    exact_or_unknown (exact_match_case_score children).
Proof. intros state children. reflexivity. Qed.

Lemma match_case_tables :
  slot_holds_tag (generated_program_of 21) 0 1 = true /\
  slot_holds_tag (generated_program_of 21) 1 2 = true /\
  message_field_at (generated_program_of 21) 1 1 = true /\
  message_field_at (generated_program_of 21) 2 1 = true /\
  message_field_at (generated_program_of 21) 4 1 = true /\
  scalar_field_at (generated_program_of 21) 3 Varint = true /\
  generated_scalar_rule (generated_program_of 21) 3 =
    Some (RPB3SourceScalarReduction.ScalarRule 3
      RPB3DescriptorProjectionData.GeneratedData.PBInt32 true).
Proof. vm_compute. repeat split; reflexivity. Qed.

Lemma family_21_class_ok :
  forall field children program,
    typed_valid_tree (Node field children) ->
    generated_wire_program field = Some program ->
    program_family program = 21 ->
    Forall_forest (fun subject =>
      reducer_result_acceptable (semantic_reduce subject) = true) children ->
    sem_definitely_refused
      (reduce_semantic_field field (semantic_reduce_forest children)) = false ->
    class_result_ok
      (reduce_semantic_field field (semantic_reduce_forest children)) = true.
Proof.
  intros field children program valid resolved family accepted unrefused.
  pose proof (generated_program_is_lookup_of_its_family field program 21
    resolved family (ltac:(discriminate) : 21 <> 0)) as same_program.
  subst program.
  pose proof (unrefused_node_has_valid_scan field _ _ resolved unrefused)
    as scan_ok.
  pose proof (scanned_children_fit _ _ _ scan_ok) as fits.
  pose proof (typed_valid_node_children field children valid) as children_valid.
  destruct match_case_tables
    as [slot_zero_table [slot_one_table [table_one [table_two [table_four
      [scalar_table rule_three]]]]]].
  apply class_result_ok_of_bind_payload.
  { rewrite (reduce_semantic_field_type_id field _ _ resolved), family.
    apply score_class_of_match_case. }
  destruct (seen_slot (fold_left (scan_child (generated_program_of 21))
      (semantic_reduce_forest children) empty_scan) 0) eqn:seen_zero.
  - destruct (seen_slot (fold_left (scan_child (generated_program_of 21))
        (semantic_reduce_forest children) empty_scan) 1) eqn:seen_one.
    + left.
      destruct (exact_typed_child_score_exists 1 1
        (semantic_reduce_forest children)) as [pattern pattern_found].
      { exact (seen_slot_child_has_tag (generated_program_of 21) _ 0 1
          slot_zero_table scan_ok seen_zero). }
      { intros child member same_tag.
        exact (accepted_par_field_has_score _ children 1 child fits accepted
          table_one member same_tag). }
      destruct (exact_typed_child_score_exists 2 1
        (semantic_reduce_forest children)) as [source source_found].
      { exact (seen_slot_child_has_tag (generated_program_of 21) _ 1 2
          slot_one_table scan_ok seen_one). }
      { intros child member same_tag.
        exact (accepted_par_field_has_score _ children 2 child fits accepted
          table_two member same_tag). }
      destruct (exact_pbint32_or_absent_exists (semantic_reduce_forest children))
        as [free_count free_found].
      { intros child member same_tag.
        destruct (scalar_child_facts _ child 3 Varint (fits child member)
          same_tag scalar_table) as [child_unrefused [child_wire child_payload]].
        split; [exact child_wire|].
        split; [exact child_payload|].
        split; [exact child_unrefused|].
        destruct (forall_forest_member typed_valid_tree children child
          children_valid member) as [subject [same_subject [_ subject_member]]].
        subst child.
        destruct (semantic_reduce_exposes_field subject)
          as [tag_is [_ [payload_is body_is]]].
        rewrite payload_is in child_payload.
        rewrite body_is.
        apply pbint32_scalar_has_value.
        apply (scalar_child_body_valid field children
          (generated_program_of 21) subject _ valid resolved subject_member
          child_payload).
        rewrite <- tag_is, same_tag.
        exact rule_three. }
      destruct (exact_typed_optional_score_or_exists 4 1 empty_par_score
        (semantic_reduce_forest children)) as [guard guard_found].
      { intros child member same_tag.
        exact (accepted_par_field_has_score _ children 4 child fits accepted
          table_four member same_tag). }
      destruct (exact_match_case_score_exists (semantic_reduce_forest children)
        (ex_intro _ pattern pattern_found)
        (ex_intro _ source source_found)
        (ex_intro _ free_count free_found)
        (ex_intro _ guard guard_found)) as [score found].
      apply (exact_evidence_of_score _ score).
      rewrite (reduce_semantic_field_score_evidence field _ _ resolved), family,
        score_evidence_for_match_case, found.
      reflexivity.
    + right.
      apply bind_payload_required_child_absent_of_match_case.
      * rewrite (reduce_semantic_field_type_id field _ _ resolved).
        exact family.
      * rewrite (reduce_semantic_field_case_required_present field _ _ resolved),
          family, Nat.eqb_refl, seen_zero, seen_one.
        reflexivity.
  - right.
    apply bind_payload_required_child_absent_of_match_case.
    + rewrite (reduce_semantic_field_type_id field _ _ resolved).
      exact family.
    + rewrite (reduce_semantic_field_case_required_present field _ _ resolved),
        family, Nat.eqb_refl, seen_zero.
      reflexivity.
Qed.

(** ** Families 31 and 32 *)

Lemma family_31_result :
  forall field children program,
    generated_wire_program field = Some program ->
    program_family program = 31 ->
    reducer_result_acceptable
      (reduce_semantic_field field (semantic_reduce_forest children)) = true.
Proof.
  intros field children program resolved family.
  apply refused_result_is_acceptable.
  exact (epathmap_is_always_refused_by_the_current_reducer field children
    program resolved family).
Qed.

Lemma family_32_result :
  forall field children program,
    generated_wire_program field = Some program ->
    program_family program = 32 ->
    reducer_result_acceptable
      (reduce_semantic_field field (semantic_reduce_forest children)) = true.
Proof.
  intros field children program resolved family.
  destruct (sem_complete
      (reduce_semantic_field field (semantic_reduce_forest children)))
    as [[|]|] eqn:complete.
  - apply refused_result_is_acceptable.
    exact (ezipper_complete_row_is_refused_by_the_current_reducer field children
      program resolved family complete).
  - apply class_result_ok_makes_acceptable.
    assert (incomplete :
      component_completeness 32
        (fold_left (scan_child program) (semantic_reduce_forest children)
          empty_scan) (semantic_reduce_forest children) = Some false).
    { rewrite (reduce_semantic_field_complete field _ program resolved), family
        in complete.
      exact complete. }
    apply class_result_ok_of_incomplete_pathmap.
    + rewrite (reduce_semantic_field_type_id field _ program resolved), family.
      apply score_class_of_ezipper.
    + exact complete.
    + rewrite (reduce_semantic_field_score_evidence field _ program resolved),
        family, incomplete.
      reflexivity.
  - exfalso.
    rewrite (reduce_semantic_field_complete field _ program resolved), family
      in complete.
    cbn [component_completeness] in complete.
    discriminate complete.
Qed.

(** ** The VarRef and ConnectiveBody carriers *)

Lemma score_evidence_for_var_ref :
  forall state children,
    score_evidence_for 56 (component_completeness 56 state children) children =
    exact_or_unknown (exact_var_ref_carrier children).
Proof. intros state children. reflexivity. Qed.

Lemma score_evidence_for_connective_body :
  forall state children,
    score_evidence_for 57 (component_completeness 57 state children) children =
    exact_or_unknown (exact_connective_body_carrier children).
Proof. intros state children. reflexivity. Qed.

Lemma var_ref_tables :
  scalar_field_at (generated_program_of 56) 1 Varint = true /\
  scalar_field_at (generated_program_of 56) 2 Varint = true /\
  generated_scalar_rule (generated_program_of 56) 1 =
    Some (RPB3SourceScalarReduction.ScalarRule 1
      RPB3DescriptorProjectionData.GeneratedData.PBSint32 true) /\
  generated_scalar_rule (generated_program_of 56) 2 =
    Some (RPB3SourceScalarReduction.ScalarRule 2
      RPB3DescriptorProjectionData.GeneratedData.PBSint32 true).
Proof. vm_compute. repeat split; reflexivity. Qed.

(** The VarRef carrier's score reads two scalars checked by TV2, so a
    typed-valid unrefused VarRef carries an exact score. *)
Lemma var_ref_carrier_is_exact :
  forall subject,
    typed_valid_tree subject ->
    sem_type_id (semantic_reduce subject) = 56 ->
    sem_definitely_refused (semantic_reduce subject) = false ->
    exact_evidence (semantic_reduce subject) = true.
Proof.
  intros [field children] valid type_is unrefused.
  cbn [semantic_reduce] in type_is, unrefused |- *.
  destruct (unrefused_node_is_resolved field _ unrefused) as [program resolved].
  rewrite (reduce_semantic_field_type_id field _ program resolved) in type_is.
  pose proof (generated_program_is_lookup_of_its_family field program 56
    resolved type_is (ltac:(discriminate) : 56 <> 0)) as same_program.
  subst program.
  pose proof (unrefused_node_has_valid_scan field _ _ resolved unrefused)
    as scan_ok.
  pose proof (scanned_children_fit _ _ _ scan_ok) as fits.
  pose proof (typed_valid_node_children field children valid) as children_valid.
  destruct var_ref_tables as [table_one [table_two [rule_one rule_two]]].
  assert (decodes : forall tag,
    (scalar_field_at (generated_program_of 56) tag Varint = true) ->
    generated_scalar_rule (generated_program_of 56) tag =
      Some (RPB3SourceScalarReduction.ScalarRule tag
        RPB3DescriptorProjectionData.GeneratedData.PBSint32 true) ->
    exists value, exact_i32_or_absent tag (semantic_reduce_forest children) = Some value).
  { intros tag table rule.
    apply exact_i32_or_absent_exists.
    intros child member same_tag.
    destruct (scalar_child_facts _ child tag Varint (fits child member) same_tag
      table) as [_ [_ child_payload]].
    destruct (forall_forest_member typed_valid_tree children child
      children_valid member) as [subject [same_subject [_ subject_member]]].
    subst child.
    destruct (semantic_reduce_exposes_field subject)
      as [tag_is [_ [payload_is body_is]]].
    rewrite payload_is in child_payload.
    rewrite body_is.
    apply pbsint32_scalar_has_value.
    apply (scalar_child_body_valid field children (generated_program_of 56)
      subject _ valid resolved subject_member child_payload).
    rewrite <- tag_is, same_tag.
    exact rule. }
  destruct (decodes 1 table_one rule_one) as [index index_found].
  destruct (decodes 2 table_two rule_two) as [depth depth_found].
  unfold exact_evidence.
  rewrite (reduce_semantic_field_score_evidence field _ _ resolved), type_is,
    score_evidence_for_var_ref.
  unfold exact_var_ref_carrier.
  rewrite index_found, depth_found.
  reflexivity.
Qed.

(** The Par elements read by a ConnectiveBody carrier decide its exactness. *)
Lemma connective_body_tables :
  message_field_at (generated_program_of 57) 1 1 = true.
Proof. vm_compute. reflexivity. Qed.

Lemma connective_body_carrier_is_exact_of_exact_elements :
  forall field children,
    sem_type_id (reduce_semantic_field field (semantic_reduce_forest children)) = 57 ->
    sem_definitely_refused
      (reduce_semantic_field field (semantic_reduce_forest children)) = false ->
    (forall child, In child (semantic_reduce_forest children) ->
       sem_tag child = 1 -> exact_evidence child = true) ->
    exact_evidence
      (reduce_semantic_field field (semantic_reduce_forest children)) = true.
Proof.
  intros field children type_is unrefused elements.
  destruct (unrefused_node_is_resolved field _ unrefused) as [program resolved].
  rewrite (reduce_semantic_field_type_id field _ program resolved) in type_is.
  pose proof (generated_program_is_lookup_of_its_family field program 57
    resolved type_is (ltac:(discriminate) : 57 <> 0)) as same_program.
  subst program.
  pose proof (unrefused_node_has_valid_scan field _ _ resolved unrefused)
    as scan_ok.
  pose proof (scanned_children_fit _ _ _ scan_ok) as fits.
  destruct (exact_typed_selected_scores_exists 1 1
    (semantic_reduce_forest children)) as [scores found].
  { intros child member same_tag.
    destruct (message_child_facts _ child 1 1 (fits child member) same_tag
      connective_body_tables)
      as [child_unrefused [child_wire [child_payload child_type]]].
    exact (typed_message_score_of_exact 1 child child_type child_wire
      child_payload child_unrefused (elements child member same_tag)). }
  unfold exact_evidence.
  rewrite (reduce_semantic_field_score_evidence field _ _ resolved), type_is,
    score_evidence_for_connective_body.
  unfold exact_connective_body_carrier.
  rewrite found.
  reflexivity.
Qed.

Theorem connective_body_carrier_is_exact_of_acceptable_children :
  forall field children,
    sem_type_id (semantic_reduce (Node field children)) = 57 ->
    sem_definitely_refused (semantic_reduce (Node field children)) = false ->
    Forall_forest (fun subject =>
      reducer_result_acceptable (semantic_reduce subject) = true) children ->
    exact_evidence (semantic_reduce (Node field children)) = true.
Proof.
  intros field children type_is unrefused accepted.
  cbn [semantic_reduce] in type_is, unrefused |- *.
  destruct (unrefused_node_is_resolved field _ unrefused) as [program resolved].
  pose proof (reduce_semantic_field_type_id field (semantic_reduce_forest children)
    program resolved) as program_type.
  rewrite type_is in program_type.
  symmetry in program_type.
  pose proof (generated_program_is_lookup_of_its_family field program 57
    resolved program_type (ltac:(discriminate) : 57 <> 0)) as same_program.
  subst program.
  pose proof (unrefused_node_has_valid_scan field _ _ resolved unrefused)
    as scan_ok.
  pose proof (scanned_children_fit _ _ _ scan_ok) as fits.
  apply (connective_body_carrier_is_exact_of_exact_elements field children
    type_is unrefused).
  intros child member same_tag.
  destruct (message_child_facts _ child 1 1 (fits child member) same_tag
    connective_body_tables)
    as [child_unrefused [_ [child_payload child_type]]].
  exact (par_child_is_exact child
    (forest_member_is_accepted children child accepted member)
    child_unrefused child_payload child_type).
Qed.

(** The shape every exact ConnectiveBody carrier has: constructor 0 applied to
    its Par element scores. *)
Lemma connective_body_carrier_score_shape :
  forall subject score,
    sem_type_id (semantic_reduce subject) = 57 ->
    sem_score_evidence (semantic_reduce subject) = ExactScore score ->
    exists scores, score = score_constructor 0 scores.
Proof.
  intros [field children] score type_is evidence.
  cbn [semantic_reduce] in type_is, evidence.
  destruct (generated_wire_program field) as [program|] eqn:resolved.
  - rewrite (reduce_semantic_field_type_id field _ program resolved) in type_is.
    rewrite (reduce_semantic_field_score_evidence field _ program resolved),
      type_is, score_evidence_for_connective_body in evidence.
    unfold exact_connective_body_carrier in evidence.
    destruct (exact_typed_selected_scores 1 1 (semantic_reduce_forest children))
      as [scores|].
    + cbn [option_map exact_or_unknown] in evidence.
      inversion evidence as [same_score].
      exists scores.
      reflexivity.
    + cbn [option_map exact_or_unknown] in evidence.
      discriminate evidence.
  - exfalso.
    rewrite (reduce_semantic_field_no_program_type field _ resolved) in type_is.
    discriminate type_is.
Qed.

(** ** Family 55 *)

Definition connective_body_carrier_exact (subject : @tree raw_field) : Prop :=
  sem_type_id (semantic_reduce subject) = 57 ->
  sem_definitely_refused (semantic_reduce subject) = false ->
  exact_evidence (semantic_reduce subject) = true.

(** The carrier invariant holds of every node whose children are acceptable,
    whatever the node's family, so a tree induction can carry it alongside
    acceptance: the induction hypothesis on a node's children yields both of
    the facts the family-55 step consumes. *)
Lemma connective_body_carrier_exact_of_acceptable_children :
  forall field children,
    Forall_forest (fun subject =>
      reducer_result_acceptable (semantic_reduce subject) = true) children ->
    connective_body_carrier_exact (Node field children).
Proof.
  intros field children accepted type_is unrefused.
  exact (connective_body_carrier_is_exact_of_acceptable_children field children
    type_is unrefused accepted).
Qed.

Lemma score_evidence_for_connective :
  forall state children,
    score_evidence_for 55 (component_completeness 55 state children) children =
    exact_or_unknown (exact_connective_score children).
Proof. intros state children. reflexivity. Qed.

Lemma connective_tables :
  all_fields_in_slot_zero (generated_program_of 55) = true /\
  forallb (fun descriptor =>
    listed_score_type (descriptor_tag descriptor) [1; 2; 3; 4; 5; 6; 7; 8; 9])
    (program_fields (generated_program_of 55)) = true /\
  message_field_at (generated_program_of 55) 1 57 = true /\
  message_field_at (generated_program_of 55) 2 57 = true /\
  message_field_at (generated_program_of 55) 3 1 = true /\
  message_field_at (generated_program_of 55) 4 56 = true /\
  scalar_field_at (generated_program_of 55) 5 Varint = true /\
  scalar_field_at (generated_program_of 55) 6 Varint = true /\
  scalar_field_at (generated_program_of 55) 7 Varint = true /\
  scalar_field_at (generated_program_of 55) 8 Varint = true /\
  scalar_field_at (generated_program_of 55) 9 Varint = true /\
  generated_scalar_rule (generated_program_of 55) 5 =
    Some (RPB3SourceScalarReduction.ScalarRule 5
      RPB3DescriptorProjectionData.GeneratedData.PBBool false) /\
  generated_scalar_rule (generated_program_of 55) 6 =
    Some (RPB3SourceScalarReduction.ScalarRule 6
      RPB3DescriptorProjectionData.GeneratedData.PBBool false) /\
  generated_scalar_rule (generated_program_of 55) 7 =
    Some (RPB3SourceScalarReduction.ScalarRule 7
      RPB3DescriptorProjectionData.GeneratedData.PBBool false) /\
  generated_scalar_rule (generated_program_of 55) 8 =
    Some (RPB3SourceScalarReduction.ScalarRule 8
      RPB3DescriptorProjectionData.GeneratedData.PBBool false) /\
  generated_scalar_rule (generated_program_of 55) 9 =
    Some (RPB3SourceScalarReduction.ScalarRule 9
      RPB3DescriptorProjectionData.GeneratedData.PBBool false).
Proof. vm_compute. repeat split; reflexivity. Qed.

Lemma connective_score_of_message_arm :
  forall child score,
    sem_wire child = LengthDelimited ->
    sem_definitely_refused child = false ->
    sem_payload child = Message ->
    (sem_tag child =? 3) = false ->
    exact_connective_message_arm child = Some score ->
    exact_connective_score [child] = Some score.
Proof.
  intros child score wire_is unrefused payload_is not_three arm.
  unfold exact_connective_score.
  cbv beta iota.
  rewrite wire_is, unrefused, payload_is.
  cbv beta iota.
  rewrite not_three.
  destruct (connective_scalar_constructor (sem_tag child));
    cbn [andb]; exact arm.
Qed.

Lemma connective_score_of_not_arm :
  forall child,
    sem_wire child = LengthDelimited ->
    sem_definitely_refused child = false ->
    sem_payload child = Message ->
    sem_tag child = 3 ->
    sem_type_id child = 1 ->
    exact_evidence child = true ->
    exists score, exact_connective_score [child] = Some score.
Proof.
  intros child wire_is unrefused payload_is tag_is type_is exact_true.
  destruct (exact_evidence_gives_score child exact_true) as [score evidence].
  exists (score_constructor 400 [score]).
  unfold exact_connective_score.
  cbv beta iota.
  rewrite wire_is, unrefused, payload_is.
  cbv beta iota.
  rewrite tag_is, type_is, evidence.
  reflexivity.
Qed.

Lemma connective_body_arm_of_carrier :
  forall constructor child scores,
    sem_wire child = LengthDelimited ->
    sem_payload child = Message ->
    sem_definitely_refused child = false ->
    sem_type_id child = 57 ->
    sem_score_evidence child = ExactScore (score_constructor 0 scores) ->
    exists score, exact_connective_body_arm constructor child = Some score.
Proof.
  intros constructor child scores wire_is payload_is unrefused type_is evidence.
  exists (ScoreNode (ScoreCons (ScoreInteger constructor)
    (score_forest_of_list scores))).
  unfold exact_connective_body_arm.
  rewrite (typed_message_score_of_evidence 57 child _ type_is wire_is payload_is
    unrefused evidence).
  reflexivity.
Qed.

Lemma connective_and_or_arm_score :
  forall constructor child scores,
    (sem_tag child = 1 /\ constructor = 401%N \/
     sem_tag child = 2 /\ constructor = 402%N) ->
    sem_wire child = LengthDelimited ->
    sem_definitely_refused child = false ->
    sem_payload child = Message ->
    sem_type_id child = 57 ->
    sem_score_evidence child = ExactScore (score_constructor 0 scores) ->
    exists score, exact_connective_score [child] = Some score.
Proof.
  intros constructor child scores tag_case wire_is unrefused payload_is type_is
    evidence.
  destruct (connective_body_arm_of_carrier constructor child scores wire_is
    payload_is unrefused type_is evidence) as [score arm_found].
  exists score.
  apply (connective_score_of_message_arm child score wire_is unrefused
    payload_is).
  - destruct tag_case as [[tag _]|[tag _]]; rewrite tag; reflexivity.
  - unfold exact_connective_message_arm.
    destruct tag_case as [[tag same_constructor]|[tag same_constructor]];
      rewrite tag; subst constructor; exact arm_found.
Qed.

Lemma connective_var_ref_arm_score :
  forall child,
    sem_wire child = LengthDelimited ->
    sem_definitely_refused child = false ->
    sem_payload child = Message ->
    sem_tag child = 4 ->
    sem_type_id child = 56 ->
    exact_evidence child = true ->
    exists score, exact_connective_score [child] = Some score.
Proof.
  intros child wire_is unrefused payload_is tag_is type_is exact_true.
  destruct (typed_message_score_of_exact 56 child type_is wire_is payload_is
    unrefused exact_true) as [score found].
  exists score.
  apply (connective_score_of_message_arm child score wire_is unrefused
    payload_is).
  - rewrite tag_is. reflexivity.
  - unfold exact_connective_message_arm.
    rewrite tag_is.
    exact found.
Qed.

Lemma connective_scalar_arm_score :
  forall field children program subject tag,
    typed_valid_tree (Node field children) ->
    generated_wire_program field = Some program ->
    In (tree_field subject) (forest_fields children) ->
    child_fits program (semantic_reduce subject) ->
    sem_tag (semantic_reduce subject) = tag ->
    scalar_field_at program tag Varint = true ->
    generated_scalar_rule program tag =
      Some (RPB3SourceScalarReduction.ScalarRule tag
        RPB3DescriptorProjectionData.GeneratedData.PBBool false) ->
    ((5 <=? tag) && (tag <=? 9)) = true ->
    exists score, exact_connective_score [semantic_reduce subject] = Some score.
Proof.
  intros field children program subject tag valid resolved subject_member
    fits_child same_tag table rule_is range.
  destruct (scalar_child_facts program _ tag Varint fits_child same_tag table)
    as [unrefused [wire_is payload_is]].
  destruct (semantic_reduce_exposes_field subject)
    as [tag_is [_ [raw_payload_is body_is]]].
  assert (body_valid :
    scalar_body_valid
      (RPB3SourceScalarReduction.ScalarRule tag
        RPB3DescriptorProjectionData.GeneratedData.PBBool false)
      (raw_body (tree_field subject)) = true).
  { apply (scalar_child_body_valid field children program subject _ valid
      resolved subject_member).
    - rewrite <- raw_payload_is. exact payload_is.
    - rewrite <- tag_is, same_tag. exact rule_is. }
  destruct (pbbool_scalar_has_value tag false _ body_valid) as [value decoded].
  exists (score_constructor (N.of_nat (tag + 399))
    [ScoreInteger (if value then 1%N else 0%N)]).
  unfold exact_connective_score.
  cbv beta iota.
  rewrite wire_is, unrefused, payload_is, same_tag.
  assert (constructor_is :
    connective_scalar_constructor tag = Some (N.of_nat (tag + 399))).
  { unfold connective_scalar_constructor.
    rewrite range.
    reflexivity. }
  rewrite constructor_is.
  cbv beta iota.
  rewrite body_is, decoded.
  reflexivity.
Qed.

Lemma connective_score_exists :
  forall field children,
    typed_valid_tree (Node field children) ->
    generated_wire_program field = Some (generated_program_of 55) ->
    scan_valid (fold_left (scan_child (generated_program_of 55))
      (semantic_reduce_forest children) empty_scan) = true ->
    Forall_forest (fun subject =>
      reducer_result_acceptable (semantic_reduce subject) = true) children ->
    Forall_forest connective_body_carrier_exact children ->
    exists score, exact_connective_score (semantic_reduce_forest children) = Some score.
Proof.
  intros field children valid resolved scan_ok accepted carriers.
  pose proof (scanned_children_fit _ _ _ scan_ok) as fits.
  pose proof (typed_valid_node_children field children valid) as children_valid.
  destruct connective_tables
    as [single_table [tags_table [table_one [table_two [table_three [table_four
      [table_five [table_six [table_seven [table_eight [table_nine
      [rule_five [rule_six [rule_seven [rule_eight rule_nine]]]]]]]]]]]]]]].
  destruct (slot_zero_scan_has_at_most_one_child _ _ single_table scan_ok)
    as [empty|[only same_list]].
  - rewrite empty.
    exists (ScoreInteger 0).
    reflexivity.
  - assert (member : In only (semantic_reduce_forest children))
      by (rewrite same_list; left; reflexivity).
    destruct (forall_forest_member _ children only
      (forall_forest_conj _ _ children children_valid
        (forall_forest_conj _ _ children accepted carriers)) member)
      as [subject [same_subject [[subject_valid [subject_accepted subject_carrier]]
        subject_member]]].
    subst only.
    pose proof (fits _ member) as only_fits.
    rewrite same_list.
    pose proof (fitted_tag_in_list _ _ _ tags_table only_fits) as tag_range.
    cbn [In] in tag_range.
    destruct tag_range
      as [tag|[tag|[tag|[tag|[tag|[tag|[tag|[tag|[tag|[]]]]]]]]]];
      apply eq_sym in tag.
    + destruct (message_child_facts _ _ 1 57 only_fits tag table_one)
        as [unrefused [wire_is [payload_is type_is]]].
      destruct (exact_evidence_gives_score _
        (subject_carrier type_is unrefused)) as [score evidence].
      destruct (connective_body_carrier_score_shape subject score type_is
        evidence) as [scores same_score].
      subst score.
      exact (connective_and_or_arm_score 401 _ scores (or_introl (conj tag eq_refl))
        wire_is unrefused payload_is type_is evidence).
    + destruct (message_child_facts _ _ 2 57 only_fits tag table_two)
        as [unrefused [wire_is [payload_is type_is]]].
      destruct (exact_evidence_gives_score _
        (subject_carrier type_is unrefused)) as [score evidence].
      destruct (connective_body_carrier_score_shape subject score type_is
        evidence) as [scores same_score].
      subst score.
      exact (connective_and_or_arm_score 402 _ scores (or_intror (conj tag eq_refl))
        wire_is unrefused payload_is type_is evidence).
    + destruct (message_child_facts _ _ 3 1 only_fits tag table_three)
        as [unrefused [wire_is [payload_is type_is]]].
      exact (connective_score_of_not_arm _ wire_is unrefused payload_is tag
        type_is (par_child_is_exact _ subject_accepted unrefused payload_is
          type_is)).
    + destruct (message_child_facts _ _ 4 56 only_fits tag table_four)
        as [unrefused [wire_is [payload_is type_is]]].
      exact (connective_var_ref_arm_score _ wire_is unrefused payload_is tag
        type_is (var_ref_carrier_is_exact subject subject_valid type_is
          unrefused)).
    + exact (connective_scalar_arm_score field children _ subject 5 valid
        resolved subject_member only_fits tag table_five rule_five eq_refl).
    + exact (connective_scalar_arm_score field children _ subject 6 valid
        resolved subject_member only_fits tag table_six rule_six eq_refl).
    + exact (connective_scalar_arm_score field children _ subject 7 valid
        resolved subject_member only_fits tag table_seven rule_seven eq_refl).
    + exact (connective_scalar_arm_score field children _ subject 8 valid
        resolved subject_member only_fits tag table_eight rule_eight eq_refl).
    + exact (connective_scalar_arm_score field children _ subject 9 valid
        resolved subject_member only_fits tag table_nine rule_nine eq_refl).
Qed.

Lemma family_55_class_ok :
  forall field children program,
    typed_valid_tree (Node field children) ->
    generated_wire_program field = Some program ->
    program_family program = 55 ->
    Forall_forest (fun subject =>
      reducer_result_acceptable (semantic_reduce subject) = true) children ->
    Forall_forest connective_body_carrier_exact children ->
    sem_definitely_refused
      (reduce_semantic_field field (semantic_reduce_forest children)) = false ->
    class_result_ok
      (reduce_semantic_field field (semantic_reduce_forest children)) = true.
Proof.
  intros field children program valid resolved family accepted carriers unrefused.
  pose proof (generated_program_is_lookup_of_its_family field program 55
    resolved family (ltac:(discriminate) : 55 <> 0)) as same_program.
  subst program.
  pose proof (unrefused_node_has_valid_scan field _ _ resolved unrefused)
    as scan_ok.
  apply class_result_ok_of_name_component.
  - rewrite (reduce_semantic_field_type_id field _ _ resolved), family.
    apply score_class_of_connective.
  - rewrite (reduce_semantic_field_complete field _ _ resolved), family.
    reflexivity.
  - destruct (connective_score_exists field children valid resolved scan_ok
      accepted carriers) as [score found].
    apply (exact_evidence_of_score _ score).
    rewrite (reduce_semantic_field_score_evidence field _ _ resolved), family,
      score_evidence_for_connective, found.
    reflexivity.
Qed.

(** ** Family 60 and the bridge from the reread grammar to [decode_region]

    The grammar of [RPB3TypedValid.v] speaks of bytes; the reducer re-decodes
    the leaf body with [RPB3RawRegionTree.decode_region].  The bridge is proved
    for every body, not by enumeration. *)

Lemma varint_groups_from_suffix_is_shorter :
  forall fuel group accumulated input result suffix,
    varint_groups_from fuel group accumulated input = Some (result, suffix) ->
    length suffix < length input.
Proof.
  induction fuel as [|fuel IH]; intros group accumulated input result suffix read.
  - discriminate read.
  - destruct input as [|byte rest]; [discriminate read|].
    cbn [varint_groups_from] in read.
    destruct ((255 <? byte) || ((group =? 9) && (1 <? byte))) eqn:guard;
      [discriminate read|].
    cbv beta iota zeta in read.
    destruct (byte <? 128).
    + destruct ((0 <? group) && (byte =? 0)); [discriminate read|].
      inversion read; subst.
      cbn [length].
      lia.
    + apply IH in read.
      cbn [length].
      lia.
Qed.

(** The byte-level varint reader of the grammar agrees with the cursor at
    every offset. *)
Lemma varint_groups_from_agrees_with_cursor :
  forall fuel group accumulated input result suffix prefix region_end,
    varint_groups_from fuel group accumulated input = Some (result, suffix) ->
    region_end = length prefix + length input ->
    RPB3WireCursor.varint_groups fuel (prefix ++ input) region_end
      (length prefix) group accumulated =
    Some (result, length prefix + length input - length suffix).
Proof.
  induction fuel as [|fuel IH];
    intros group accumulated input result suffix prefix region_end read bound.
  - discriminate read.
  - destruct input as [|byte rest]; [discriminate read|].
    cbn [varint_groups_from] in read.
    cbn [RPB3WireCursor.varint_groups].
    cbn [length] in bound.
    assert (within : (length prefix <? region_end) = true).
    { apply Nat.ltb_lt. lia. }
    rewrite within.
    rewrite nth_error_app2 by lia.
    rewrite Nat.sub_diag.
    cbn [nth_error].
    destruct ((255 <? byte) || ((group =? 9) && (1 <? byte))) eqn:guard;
      [discriminate read|].
    apply orb_false_elim in guard as [small group_guard].
    assert (byte_fine : RPB3WireCursor.byte_ok byte = true).
    { unfold RPB3WireCursor.byte_ok.
      apply Nat.ltb_lt.
      apply Nat.ltb_ge in small.
      lia. }
    rewrite byte_fine, group_guard.
    cbv beta iota zeta in read |- *.
    destruct (byte <? 128) eqn:terminal.
    + destruct ((0 <? group) && (byte =? 0)) eqn:nonzero; [discriminate read|].
      inversion read; subst result suffix.
      f_equal.
      f_equal.
      cbn [length].
      lia.
    + assert (rebuilt : prefix ++ byte :: rest = (prefix ++ [byte]) ++ rest).
      { rewrite <- app_assoc. reflexivity. }
      assert (prefix_length : length (prefix ++ [byte]) = S (length prefix)).
      { rewrite length_app. cbn [length]. lia. }
      assert (rest_bound :
        region_end = length (prefix ++ [byte]) + length rest).
      { rewrite prefix_length. lia. }
      rewrite rebuilt.
      pose proof (IH _ _ rest result suffix (prefix ++ [byte]) region_end read
        rest_bound) as recursive.
      rewrite prefix_length in recursive.
      rewrite recursive.
      f_equal.
      f_equal.
      cbn [length].
      lia.
Qed.

(** One length-delimited field whose body reaches the end of the region is
    the only field of the region. *)
Lemma framing_field_length_delimited :
  forall bytes region_end offset last key key_end number payload_length body_start,
    RPB3WireCursor.canonical_varint bytes region_end offset = Some (key, key_end) ->
    (N.ltb 0 (key / 8)%N &&
      RPB3WireCursor.field_number_within_limit (key / 8)%N) = true ->
    N.to_nat (key / 8)%N = number ->
    N.to_nat (key mod 8)%N = 2 ->
    RPB3WireCursor.canonical_varint bytes region_end key_end =
      Some (payload_length, body_start) ->
    N.leb payload_length (N.of_nat (region_end - body_start)) = true ->
    RPB3WireCursor.framing_field bytes region_end offset last false =
      Some (RPB3WireCursor.FieldSpan number RPB3WireCursor.WireLength offset
        key_end body_start (body_start + N.to_nat payload_length) None).
Proof.
  intros bytes region_end offset last key key_end number payload_length body_start
    key_read number_ok number_is wire_two length_read fits.
  unfold RPB3WireCursor.framing_field.
  rewrite key_read.
  cbv beta iota zeta.
  rewrite number_ok.
  cbn [negb orb].
  rewrite wire_two, length_read.
  cbv beta iota zeta.
  rewrite fits, number_is.
  reflexivity.
Qed.

Lemma checked_field_length_delimited :
  forall bytes region_end offset last key key_end number payload_length body_start,
    RPB3WireCursor.canonical_varint bytes region_end offset = Some (key, key_end) ->
    (N.ltb 0 (key / 8)%N &&
      RPB3WireCursor.field_number_within_limit (key / 8)%N) = true ->
    N.to_nat (key / 8)%N = number ->
    N.to_nat (key mod 8)%N = 2 ->
    RPB3WireCursor.canonical_varint bytes region_end key_end =
      Some (payload_length, body_start) ->
    N.leb payload_length (N.of_nat (region_end - body_start)) = true ->
    offset < body_start + N.to_nat payload_length ->
    body_start + N.to_nat payload_length <= region_end ->
    RPB3WireCursor.checked_field bytes region_end offset last false =
      Some (RPB3WireCursor.FieldSpan number RPB3WireCursor.WireLength offset
        key_end body_start (body_start + N.to_nat payload_length) None).
Proof.
  intros bytes region_end offset last key key_end number payload_length body_start
    key_read number_ok number_is wire_two length_read fits progress bounded.
  unfold RPB3WireCursor.checked_field.
  rewrite (framing_field_length_delimited bytes region_end offset last key key_end
    number payload_length body_start key_read number_ok number_is wire_two
    length_read fits).
  cbn [RPB3WireCursor.span_start RPB3WireCursor.span_end].
  rewrite Nat.eqb_refl.
  assert (progress_bit :
    (offset <? body_start + N.to_nat payload_length) = true).
  { apply Nat.ltb_lt. exact progress. }
  assert (bounded_bit :
    (body_start + N.to_nat payload_length <=? region_end) = true).
  { apply Nat.leb_le. exact bounded. }
  rewrite progress_bit, bounded_bit.
  reflexivity.
Qed.

Lemma reference_remaining_at_end :
  forall fuel bytes region_end last numeric_order,
    RPB3WireCursor.reference_remaining fuel bytes region_end region_end last
      numeric_order = RPB3WireCursor.Parsed [].
Proof.
  intros [|fuel] bytes region_end last numeric_order;
    cbn [RPB3WireCursor.reference_remaining];
    rewrite Nat.eqb_refl;
    reflexivity.
Qed.

Lemma worklist_single_field :
  forall bytes region_end field,
    RPB3WireCursor.region_ok bytes 0 region_end = true ->
    RPB3WireCursor.checked_field bytes region_end 0 0 false = Some field ->
    RPB3WireCursor.span_end field = region_end ->
    0 < region_end ->
    RPB3WireCursor.worklist_region bytes 0 region_end false =
      RPB3WireCursor.Parsed [field].
Proof.
  intros bytes region_end field region_valid checked ended positive.
  rewrite RPB3WireCursor.worklist_and_reference_agree_on_every_finite_region.
  unfold RPB3WireCursor.reference_region.
  rewrite region_valid, Nat.sub_0_r.
  destruct region_end as [|smaller]; [lia|].
  cbn [RPB3WireCursor.reference_remaining].
  assert (not_finished : (0 =? S smaller) = false) by reflexivity.
  rewrite not_finished.
  cbv beta iota.
  rewrite checked.
  cbv beta iota.
  rewrite ended.
  rewrite reference_remaining_at_end.
  reflexivity.
Qed.

Lemma whole_region_is_ok :
  forall bytes,
    forallb RPB3WireCursor.byte_ok bytes = true ->
    RPB3WireCursor.region_ok bytes 0 (length bytes) = true.
Proof.
  intros bytes bytes_ok.
  unfold RPB3WireCursor.region_ok.
  rewrite Nat.sub_0_r.
  cbn [skipn].
  rewrite firstn_all.
  rewrite bytes_ok, Nat.leb_refl.
  reflexivity.
Qed.

Lemma decode_region_single_scalar_field :
  forall fuel bytes program region_start region_end span descriptor,
    RPB3WireCursor.worklist_region bytes region_start region_end false =
      RPB3WireCursor.Parsed [span] ->
    RPB3RawRegionTree.source_descriptor program span = Some descriptor ->
    RPB3RawRegionTree.slot_admissible program
      (RPB3RawRegionTree.DecodeAccumulator None [] []) descriptor = true ->
    descriptor_payload descriptor = Scalar ->
    RPB3RawRegionTree.decode_region (S fuel) bytes program region_start region_end =
      Some [RPB3RawRegionTree.SourceScalar span descriptor].
Proof.
  intros fuel bytes program region_start region_end span descriptor parsed
    resolved admissible scalar.
  cbn [RPB3RawRegionTree.decode_region].
  rewrite parsed.
  cbn [fold_left].
  rewrite resolved.
  cbv beta iota.
  rewrite admissible.
  cbv beta iota.
  rewrite scalar.
  reflexivity.
Qed.

Definition leaf_bytes_descriptor : field_descriptor :=
  FieldDescriptor 1 0 LengthDelimited false Scalar None None.

Definition leaf_bytes_span (start finish : nat) : RPB3WireCursor.field_span :=
  RPB3WireCursor.FieldSpan 1 RPB3WireCursor.WireLength 0 1 start finish None.

Lemma leaf_program_facts :
  forall type_id,
    In type_id [61; 62; 63] ->
    lookup_projected_program type_id [] [] = Some (generated_program_of type_id) /\
    resolve_tag (program_fields (generated_program_of type_id)) 1 =
      Some leaf_bytes_descriptor /\
    program_slot_count (generated_program_of type_id) = 1 /\
    generated_scalar_rule (generated_program_of type_id) 1 =
      Some (RPB3SourceScalarReduction.ScalarRule 1
        RPB3DescriptorProjectionData.GeneratedData.PBBytes true).
Proof.
  intros type_id member.
  cbn [In] in member.
  destruct member as [<-|[<-|[<-|[]]]]; vm_compute; repeat split; reflexivity.
Qed.

Lemma leaf_span_source_descriptor :
  forall program start finish,
    resolve_tag (program_fields program) 1 = Some leaf_bytes_descriptor ->
    RPB3RawRegionTree.source_descriptor program (leaf_bytes_span start finish) =
      Some leaf_bytes_descriptor.
Proof.
  intros program start finish resolved.
  unfold RPB3RawRegionTree.source_descriptor, leaf_bytes_span.
  cbn [RPB3WireCursor.span_number].
  rewrite resolved.
  reflexivity.
Qed.

Lemma leaf_descriptor_slot_admissible :
  forall program,
    program_slot_count program = 1 ->
    RPB3RawRegionTree.slot_admissible program
      (RPB3RawRegionTree.DecodeAccumulator None [] []) leaf_bytes_descriptor = true.
Proof.
  intros program slots.
  unfold RPB3RawRegionTree.slot_admissible.
  rewrite slots.
  reflexivity.
Qed.

Lemma leaf_span_body_length :
  forall bytes start finish descriptor,
    start <= finish ->
    finish <= length bytes ->
    length (RPB3RawRegionTree.source_body bytes
      (RPB3RawRegionTree.SourceScalar (leaf_bytes_span start finish) descriptor)) =
    finish - start.
Proof.
  intros bytes start finish descriptor ordered bounded.
  exact (RPB3RawRegionTree.bounded_source_body_has_exact_length bytes
    (RPB3RawRegionTree.SourceScalar (leaf_bytes_span start finish) descriptor)
    ordered bounded).
Qed.

Lemma exact_unforgeable_leaf_bytes_of_decoded :
  forall type_id body program span descriptor,
    lookup_projected_program type_id [] [] = Some program ->
    RPB3RawRegionTree.decode_region (S (length body)) body program 0 (length body) =
      Some [RPB3RawRegionTree.SourceScalar span descriptor] ->
    descriptor_tag descriptor = 1 ->
    generated_scalar_rule program 1 =
      Some (RPB3SourceScalarReduction.ScalarRule 1
        RPB3DescriptorProjectionData.GeneratedData.PBBytes true) ->
    0 < length (RPB3RawRegionTree.source_body body
      (RPB3RawRegionTree.SourceScalar span descriptor)) ->
    exists bytes, exact_unforgeable_leaf_bytes type_id body = Some bytes.
Proof.
  intros type_id body program span descriptor found decoded tag_one rule_is
    nonempty.
  unfold exact_unforgeable_leaf_bytes.
  rewrite found.
  cbv beta iota.
  rewrite decoded.
  cbv beta iota.
  rewrite tag_one, Nat.eqb_refl.
  cbv beta iota.
  rewrite rule_is.
  cbv beta iota.
  rewrite forbidden_bytes_scalar_has_value by exact nonempty.
  eexists.
  reflexivity.
Qed.

(** The reread grammar names the payload: [reread_body_matches] on the field
    of a bytes leaf gives a canonical length prefix followed by that many
    bytes, and nothing else. *)
Lemma reread_bytes_grammar_parts :
  forall rest,
    reread_body_matches [RereadField 10 RereadBytes] (10 :: rest) = true ->
    exists declared after_varint,
      varint_groups_from 10 0 0%N rest = Some (declared, after_varint) /\
      N.leb declared (N.of_nat (length after_varint)) = true /\
      0 < N.to_nat declared /\
      skipn (N.to_nat declared) after_varint = [].
Proof.
  intros rest grammar.
  cbn [reread_body_matches reread_key] in grammar.
  rewrite Nat.eqb_refl in grammar.
  cbv beta iota in grammar.
  cbn [reread_field_tail reread_kind] in grammar.
  unfold bytes_field_tail, canonical_varint_prefix in grammar.
  destruct (varint_groups_from 10 0 0%N rest)
    as [[declared after_varint]|] eqn:varint.
  2: { cbv beta iota in grammar. discriminate grammar. }
  cbv beta iota zeta in grammar.
  destruct (N.leb declared (N.of_nat (length after_varint))) eqn:fits.
  2: { cbv beta iota in grammar. discriminate grammar. }
  cbv beta iota zeta in grammar.
  destruct ((0 <? N.to_nat declared) &&
    forallb (fun byte => byte <? 256) (firstn (N.to_nat declared) after_varint))
    eqn:guard.
  2: { cbv beta iota in grammar. discriminate grammar. }
  cbv beta iota in grammar.
  destruct (skipn (N.to_nat declared) after_varint) as [|byte remainder]
    eqn:tail_empty.
  2: { cbn [reread_body_matches] in grammar. discriminate grammar. }
  apply andb_prop in guard as [positive _].
  apply Nat.ltb_lt in positive.
  exists declared, after_varint.
  repeat split; try assumption.
Qed.

Lemma bytes_leaf_has_exact_bytes :
  forall type_id body,
    In type_id [61; 62; 63] ->
    forallb (fun byte => byte <? 256) body = true ->
    reread_body_matches [RereadField 10 RereadBytes] body = true ->
    exists bytes, exact_unforgeable_leaf_bytes type_id body = Some bytes.
Proof.
  intros type_id body member bytes_ok grammar.
  destruct (leaf_program_facts type_id member)
    as [lookup [resolve_one [slot_one rule_one]]].
  destruct body as [|first rest].
  - cbn [In] in member.
    destruct member as [<-|[<-|[<-|[]]]]; exists []; vm_compute; reflexivity.
  - destruct (first =? 10) eqn:key_byte.
    + apply Nat.eqb_eq in key_byte.
      subst first.
      destruct (reread_bytes_grammar_parts rest grammar)
        as [declared [after_varint [varint [fits [positive skip_empty]]]]].
      pose proof (varint_groups_from_suffix_is_shorter 10 0 0%N rest declared
        after_varint varint) as shorter.
      assert (count_at_most : N.to_nat declared <= length after_varint).
      { rewrite RPB3WireCursor.binary_and_nat_bounds_agree in fits.
        rewrite Nat2N.id in fits.
        apply Nat.leb_le.
        exact fits. }
      assert (count_at_least : length after_varint <= N.to_nat declared).
      { pose proof (length_skipn (N.to_nat declared) after_varint) as skip_length.
        rewrite skip_empty in skip_length.
        cbn [length] in skip_length.
        lia. }
      set (start := 1 + length rest - length after_varint).
      set (finish := start + N.to_nat declared).
      assert (finish_is_end : finish = length (10 :: rest)).
      { unfold finish, start.
        cbn [length].
        lia. }
      assert (start_ordered : start <= finish).
      { unfold finish. lia. }
      assert (key_read :
        RPB3WireCursor.canonical_varint (10 :: rest) (length (10 :: rest)) 0 =
        Some (10%N, 1)).
      { reflexivity. }
      assert (length_read :
        RPB3WireCursor.canonical_varint (10 :: rest) (length (10 :: rest)) 1 =
        Some (declared, start)).
      { unfold RPB3WireCursor.canonical_varint.
        exact (varint_groups_from_agrees_with_cursor 10 0 0%N rest declared
          after_varint [10] (length (10 :: rest)) varint eq_refl). }
      assert (gap :
        length (10 :: rest) - start = length after_varint).
      { unfold start.
        cbn [length].
        lia. }
      assert (checked :
        RPB3WireCursor.checked_field (10 :: rest) (length (10 :: rest)) 0 0 false =
        Some (leaf_bytes_span start finish)).
      { unfold leaf_bytes_span, finish.
        apply (checked_field_length_delimited (10 :: rest) (length (10 :: rest))
          0 0 10%N 1 1 declared start key_read).
        - reflexivity.
        - reflexivity.
        - reflexivity.
        - exact length_read.
        - rewrite gap. exact fits.
        - lia.
        - rewrite <- finish_is_end.
          unfold finish.
          lia. }
      assert (ended : RPB3WireCursor.span_end (leaf_bytes_span start finish) =
        length (10 :: rest)).
      { unfold leaf_bytes_span.
        cbn [RPB3WireCursor.span_end].
        exact finish_is_end. }
      assert (parsed :
        RPB3WireCursor.worklist_region (10 :: rest) 0 (length (10 :: rest)) false =
        RPB3WireCursor.Parsed [leaf_bytes_span start finish]).
      { apply worklist_single_field.
        - apply whole_region_is_ok.
          exact bytes_ok.
        - exact checked.
        - exact ended.
        - cbn [length].
          lia. }
      assert (decoded :
        RPB3RawRegionTree.decode_region (S (length (10 :: rest))) (10 :: rest)
          (generated_program_of type_id) 0 (length (10 :: rest)) =
        Some [RPB3RawRegionTree.SourceScalar (leaf_bytes_span start finish)
          leaf_bytes_descriptor]).
      { apply decode_region_single_scalar_field.
        - exact parsed.
        - apply leaf_span_source_descriptor.
          exact resolve_one.
        - apply leaf_descriptor_slot_admissible.
          exact slot_one.
        - reflexivity. }
      apply (exact_unforgeable_leaf_bytes_of_decoded type_id (10 :: rest)
        (generated_program_of type_id) (leaf_bytes_span start finish)
        leaf_bytes_descriptor lookup decoded eq_refl rule_one).
      rewrite leaf_span_body_length.
      * unfold finish.
        lia.
      * exact start_ordered.
      * rewrite <- finish_is_end.
        lia.
    + exfalso.
      cbn [reread_body_matches reread_key] in grammar.
      rewrite key_byte in grammar.
      cbv beta iota in grammar.
      cbn [reread_body_matches] in grammar.
      discriminate grammar.
Qed.

(** A typed-valid unrefused message node obeys the reread grammar of its
    type. *)
Lemma reread_grammar_of_typed_valid_node :
  forall subject type_id expected,
    typed_valid_tree subject ->
    sem_definitely_refused (semantic_reduce subject) = false ->
    sem_payload (semantic_reduce subject) = Message ->
    sem_type_id (semantic_reduce subject) = type_id ->
    reread_fields_of type_id = Some expected ->
    reread_body_matches expected (sem_body (semantic_reduce subject)) = true.
Proof.
  intros [field children] type_id expected valid unrefused payload_is type_is
    fields_are.
  cbn [semantic_reduce] in unrefused, payload_is, type_is |- *.
  destruct (proj1 (typed_valid_tree_node_iff field children) valid)
    as [_ [_ [reread _]]].
  destruct (unrefused_node_is_resolved field _ unrefused) as [program resolved].
  rewrite (reduce_semantic_field_payload field) in payload_is.
  unfold reread_body_canonical in reread.
  rewrite payload_is in reread.
  unfold generated_wire_program in resolved.
  rewrite payload_is in resolved.
  destruct (raw_program_type_id field) as [declared|] eqn:declared_is;
    [|discriminate resolved].
  cbv beta iota in resolved, reread.
  pose proof (lookup_projected_program_family _ _ resolved) as family.
  rewrite (reduce_semantic_field_type_id field _ program) in type_is.
  - assert (same_type : declared = type_id) by congruence.
    rewrite same_type, fields_are in reread.
    exact reread.
  - unfold generated_wire_program.
    rewrite payload_is, declared_is.
    exact resolved.
Qed.

Lemma score_evidence_for_unforgeable :
  forall state children,
    score_evidence_for 60 (component_completeness 60 state children) children =
    exact_or_unknown (exact_unforgeable_score children).
Proof. intros state children. reflexivity. Qed.

Lemma unforgeable_tables :
  all_fields_in_slot_zero (generated_program_of 60) = true /\
  forallb (fun descriptor =>
    listed_score_type (descriptor_tag descriptor) [1; 2; 3; 4])
    (program_fields (generated_program_of 60)) = true /\
  message_field_at (generated_program_of 60) 1 61 = true /\
  message_field_at (generated_program_of 60) 2 62 = true /\
  message_field_at (generated_program_of 60) 3 63 = true /\
  message_field_at (generated_program_of 60) 4 64 = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Lemma unforgeable_score_of_bytes_arm :
  forall child tag type_id constructor,
    unforgeable_arm_contract tag = Some (type_id, constructor, true) ->
    sem_tag child = tag ->
    sem_wire child = LengthDelimited ->
    sem_payload child = Message ->
    sem_definitely_refused child = false ->
    sem_type_id child = type_id ->
    (exists bytes, exact_unforgeable_leaf_bytes type_id (sem_body child) = Some bytes) ->
    exists score, exact_unforgeable_score [child] = Some score.
Proof.
  intros child tag type_id constructor contract same_tag wire_is payload_is
    unrefused same_type [bytes found].
  unfold exact_unforgeable_score.
  cbv beta iota.
  rewrite same_tag, contract, wire_is, payload_is, unrefused.
  cbv beta iota.
  rewrite same_type, Nat.eqb_refl, found.
  eexists.
  reflexivity.
Qed.

Lemma unforgeable_score_of_token_arm :
  forall child,
    sem_tag child = 4 ->
    sem_wire child = LengthDelimited ->
    sem_payload child = Message ->
    sem_definitely_refused child = false ->
    sem_type_id child = 64 ->
    sem_body child = [] ->
    exists score, exact_unforgeable_score [child] = Some score.
Proof.
  intros child same_tag wire_is payload_is unrefused same_type empty_body.
  unfold exact_unforgeable_score.
  cbv beta iota.
  rewrite same_tag.
  cbn [unforgeable_arm_contract].
  rewrite wire_is, payload_is, unrefused.
  cbv beta iota.
  rewrite same_type, empty_body.
  eexists.
  reflexivity.
Qed.

Lemma unforgeable_child_score :
  forall (subject : @tree raw_field),
    typed_valid_tree subject ->
    child_fits (generated_program_of 60) (semantic_reduce subject) ->
    exists score, exact_unforgeable_score [semantic_reduce subject] = Some score.
Proof.
  intros subject subject_valid only_fits.
  destruct unforgeable_tables
    as [_ [tags_table [table_one [table_two [table_three table_four]]]]].
  pose proof (fitted_tag_in_list _ _ _ tags_table only_fits) as tag_range.
  cbn [In] in tag_range.
  destruct subject as [field children] eqn:subject_is.
  assert (child_body_valid :
    sem_definitely_refused (semantic_reduce (Node field children)) = false ->
    forallb (fun byte => byte <? 256)
      (sem_body (semantic_reduce (Node field children))) = true).
  { intro unrefused.
    cbn [semantic_reduce] in unrefused |- *.
    pose proof (unrefused_node_body_is_valid field _ unrefused) as body_valid.
    unfold valid_raw_body in body_valid.
    apply andb_prop in body_valid as [_ range].
    exact range. }
  destruct tag_range as [tag|[tag|[tag|[tag|[]]]]]; apply eq_sym in tag.
  - destruct (message_child_facts _ _ 1 61 only_fits tag table_one)
      as [unrefused [wire_is [payload_is type_is]]].
    assert (grammar :
      reread_body_matches [RereadField 10 RereadBytes]
        (sem_body (semantic_reduce (Node field children))) = true).
    { apply (reread_grammar_of_typed_valid_node _ 61 _ subject_valid unrefused
        payload_is type_is).
      reflexivity. }
    exact (unforgeable_score_of_bytes_arm _ 1 61 5%N eq_refl tag wire_is
      payload_is unrefused type_is
      (bytes_leaf_has_exact_bytes 61 _ (or_introl eq_refl)
        (child_body_valid unrefused) grammar)).
  - destruct (message_child_facts _ _ 2 62 only_fits tag table_two)
      as [unrefused [wire_is [payload_is type_is]]].
    assert (grammar :
      reread_body_matches [RereadField 10 RereadBytes]
        (sem_body (semantic_reduce (Node field children))) = true).
    { apply (reread_grammar_of_typed_valid_node _ 62 _ subject_valid unrefused
        payload_is type_is).
      reflexivity. }
    exact (unforgeable_score_of_bytes_arm _ 2 62 11%N eq_refl tag wire_is
      payload_is unrefused type_is
      (bytes_leaf_has_exact_bytes 62 _ (or_intror (or_introl eq_refl))
        (child_body_valid unrefused) grammar)).
  - destruct (message_child_facts _ _ 3 63 only_fits tag table_three)
      as [unrefused [wire_is [payload_is type_is]]].
    assert (grammar :
      reread_body_matches [RereadField 10 RereadBytes]
        (sem_body (semantic_reduce (Node field children))) = true).
    { apply (reread_grammar_of_typed_valid_node _ 63 _ subject_valid unrefused
        payload_is type_is).
      reflexivity. }
    exact (unforgeable_score_of_bytes_arm _ 3 63 10%N eq_refl tag wire_is
      payload_is unrefused type_is
      (bytes_leaf_has_exact_bytes 63 _ (or_intror (or_intror (or_introl eq_refl)))
        (child_body_valid unrefused) grammar)).
  - destruct (message_child_facts _ _ 4 64 only_fits tag table_four)
      as [unrefused [wire_is [payload_is type_is]]].
    assert (grammar :
      reread_body_matches []
        (sem_body (semantic_reduce (Node field children))) = true).
    { apply (reread_grammar_of_typed_valid_node _ 64 _ subject_valid unrefused
        payload_is type_is).
      reflexivity. }
    apply (unforgeable_score_of_token_arm _ tag wire_is payload_is unrefused
      type_is).
    destruct (sem_body (semantic_reduce (Node field children))) as [|byte rest];
      [reflexivity|].
    cbn [reread_body_matches] in grammar.
    discriminate grammar.
Qed.

Lemma unforgeable_score_exists :
  forall field children,
    typed_valid_tree (Node field children) ->
    scan_valid (fold_left (scan_child (generated_program_of 60))
      (semantic_reduce_forest children) empty_scan) = true ->
    exists score, exact_unforgeable_score (semantic_reduce_forest children) = Some score.
Proof.
  intros field children valid scan_ok.
  pose proof (scanned_children_fit _ _ _ scan_ok) as fits.
  pose proof (typed_valid_node_children field children valid) as children_valid.
  destruct unforgeable_tables as [single_table _].
  destruct (slot_zero_scan_has_at_most_one_child _ _ single_table scan_ok)
    as [empty|[only same_list]].
  - rewrite empty.
    exists (score_constructor 0 []).
    reflexivity.
  - assert (member : In only (semantic_reduce_forest children))
      by (rewrite same_list; left; reflexivity).
    destruct (forall_forest_member _ children only children_valid member)
      as [subject [same_subject [subject_valid _]]].
    subst only.
    rewrite same_list.
    exact (unforgeable_child_score subject subject_valid (fits _ member)).
Qed.

Lemma family_60_class_ok :
  forall field children program,
    typed_valid_tree (Node field children) ->
    generated_wire_program field = Some program ->
    program_family program = 60 ->
    sem_definitely_refused
      (reduce_semantic_field field (semantic_reduce_forest children)) = false ->
    class_result_ok
      (reduce_semantic_field field (semantic_reduce_forest children)) = true.
Proof.
  intros field children program valid resolved family unrefused.
  pose proof (generated_program_is_lookup_of_its_family field program 60
    resolved family (ltac:(discriminate) : 60 <> 0)) as same_program.
  subst program.
  pose proof (unrefused_node_has_valid_scan field _ _ resolved unrefused)
    as scan_ok.
  apply class_result_ok_of_name_component.
  - rewrite (reduce_semantic_field_type_id field _ _ resolved), family.
    apply score_class_of_unforgeable.
  - rewrite (reduce_semantic_field_complete field _ _ resolved), family.
    reflexivity.
  - destruct (unforgeable_score_exists field children valid scan_ok)
      as [score found].
    apply (exact_evidence_of_score _ score).
    rewrite (reduce_semantic_field_score_evidence field _ _ resolved), family,
      score_evidence_for_unforgeable, found.
    reflexivity.
Qed.

(** ** The classes in the statement are exactly the six families *)

Lemma name_and_pathmap_class_members :
  forall type_id,
    In (score_class_of type_id)
      [ClassBindPayload; ClassPathMapPayload; ClassNameComponent] ->
    type_id = 16 \/ type_id = 21 \/ type_id = 31 \/ type_id = 32 \/
    type_id = 55 \/ type_id = 60.
Proof.
  intros type_id member.
  unfold score_class_of in member.
  destruct (Nat.eqb type_id 1) eqn:case_par.
  { exfalso. cbn [In] in member.
    destruct member as [same|[same|[same|[]]]]; discriminate same. }
  destruct (listed_score_type type_id [8; 15; 19; 20; 22; 23]) eqn:case_process.
  { exfalso. cbn [In] in member.
    destruct member as [same|[same|[same|[]]]]; discriminate same. }
  destruct (Nat.eqb type_id 24) eqn:case_expr.
  { exfalso. cbn [In] in member.
    destruct member as [same|[same|[same|[]]]]; discriminate same. }
  destruct (Nat.eqb type_id 13) eqn:case_cost_composite.
  { exfalso. cbn [In] in member.
    destruct member as [same|[same|[same|[]]]]; discriminate same. }
  destruct (listed_score_type type_id [9; 10; 11; 12]) eqn:case_cost.
  { exfalso. cbn [In] in member.
    destruct member as [same|[same|[same|[]]]]; discriminate same. }
  destruct (listed_score_type type_id [16; 21]) eqn:case_bind.
  { apply (proj1 (listed_score_type_iff _ _)) in case_bind.
    cbn [In] in case_bind.
    destruct case_bind as [same|[same|[]]]; lia. }
  destruct (listed_score_type type_id [55; 60]) eqn:case_name.
  { apply (proj1 (listed_score_type_iff _ _)) in case_name.
    cbn [In] in case_name.
    destruct case_name as [same|[same|[]]]; lia. }
  destruct (listed_score_type type_id [31; 32]) eqn:case_pathmap.
  { apply (proj1 (listed_score_type_iff _ _)) in case_pathmap.
    cbn [In] in case_pathmap.
    destruct case_pathmap as [same|[same|[]]]; lia. }
  destruct (listed_score_type type_id expr_payload_type_ids) eqn:case_payload.
  { exfalso. cbn [In] in member.
    destruct member as [same|[same|[same|[]]]]; discriminate same. }
  destruct (listed_score_type type_id [34; 56; 57]) eqn:case_carrier.
  { exfalso. cbn [In] in member.
    destruct member as [same|[same|[same|[]]]]; discriminate same. }
  destruct (listed_score_type type_id [7; 25; 26; 61; 62; 63; 64]) eqn:case_leaf.
  { exfalso. cbn [In] in member.
    destruct member as [same|[same|[same|[]]]]; discriminate same. }
  destruct (listed_score_type type_id [2; 3; 4; 5; 14; 17; 18; 58; 59]) eqn:case_opaque.
  { exfalso. cbn [In] in member.
    destruct member as [same|[same|[same|[]]]]; discriminate same. }
  exfalso. cbn [In] in member.
  destruct member as [same|[same|[same|[]]]]; discriminate same.
Qed.

(** ** The strengthened result *)

Theorem name_and_pathmap_result_on_typed_valid_given_exact_carriers :
  forall field children,
    typed_valid_tree (Node field children) ->
    Forall_forest (fun child =>
      reducer_result_acceptable (semantic_reduce child) = true) children ->
    (sem_type_id (semantic_reduce (Node field children)) = 55 ->
       Forall_forest connective_body_carrier_exact children) ->
    In (score_class_of (sem_type_id (semantic_reduce (Node field children))))
      [ClassBindPayload; ClassPathMapPayload; ClassNameComponent] ->
    reducer_result_acceptable (semantic_reduce (Node field children)) = true.
Proof.
  intros field children valid accepted carriers_if_connective member.
  cbn [semantic_reduce] in carriers_if_connective, member |- *.
  destruct (sem_definitely_refused
      (reduce_semantic_field field (semantic_reduce_forest children))) eqn:refused.
  - apply refused_result_is_acceptable.
    exact refused.
  - destruct (generated_wire_program field) as [program|] eqn:resolved.
    2: { rewrite (no_program_field_is_refused_by_the_current_reducer field _
           resolved) in refused.
         discriminate refused. }
    rewrite (reduce_semantic_field_type_id field _ program resolved) in member.
    destruct (name_and_pathmap_class_members _ member)
      as [family|[family|[family|[family|[family|family]]]]].
    + apply class_result_ok_makes_acceptable.
      exact (family_16_class_ok field children program resolved family accepted
        refused).
    + apply class_result_ok_makes_acceptable.
      exact (family_21_class_ok field children program valid resolved family
        accepted refused).
    + exact (family_31_result field children program resolved family).
    + exact (family_32_result field children program resolved family).
    + apply class_result_ok_makes_acceptable.
      assert (carriers : Forall_forest connective_body_carrier_exact children).
      { apply carriers_if_connective.
        rewrite (reduce_semantic_field_type_id field _ program resolved).
        exact family. }
      exact (family_55_class_ok field children program valid resolved family
        accepted carriers refused).
    + apply class_result_ok_makes_acceptable.
      exact (family_60_class_ok field children program valid resolved family
        refused).
Qed.

(** The requested statement, verbatim, for the five families other than 55:
    the carrier hypothesis is only ever required of type-55 nodes. *)
Corollary name_and_pathmap_result_on_typed_valid_off_connective_family :
  forall field children,
    typed_valid_tree (Node field children) ->
    Forall_forest (fun child =>
      reducer_result_acceptable (semantic_reduce child) = true) children ->
    sem_type_id (semantic_reduce (Node field children)) <> 55 ->
    In (score_class_of (sem_type_id (semantic_reduce (Node field children))))
      [ClassBindPayload; ClassPathMapPayload; ClassNameComponent] ->
    reducer_result_acceptable (semantic_reduce (Node field children)) = true.
Proof.
  intros field children valid accepted not_connective member.
  apply (name_and_pathmap_result_on_typed_valid_given_exact_carriers field
    children valid accepted); [|exact member].
  intro is_connective.
  exfalso.
  exact (not_connective is_connective).
Qed.

(** The requested statement on every arm that is not And or Or: when no child
    has the ConnectiveBody type, the carrier hypothesis is vacuous. *)
Corollary name_and_pathmap_result_on_typed_valid_off_connective_body_arms :
  forall field children,
    typed_valid_tree (Node field children) ->
    Forall_forest (fun child =>
      reducer_result_acceptable (semantic_reduce child) = true) children ->
    Forall_forest (fun child => sem_type_id (semantic_reduce child) <> 57) children ->
    In (score_class_of (sem_type_id (semantic_reduce (Node field children))))
      [ClassBindPayload; ClassPathMapPayload; ClassNameComponent] ->
    reducer_result_acceptable (semantic_reduce (Node field children)) = true.
Proof.
  intros field children valid accepted not_carrier member.
  apply (name_and_pathmap_result_on_typed_valid_given_exact_carriers field
    children valid accepted); [|exact member].
  intros _.
  apply (forall_forest_impl
    (fun child => sem_type_id (semantic_reduce child) <> 57) _ children);
    [|exact not_carrier].
  intros subject type_differs type_is _.
  exfalso.
  exact (type_differs type_is).
Qed.

(** The requested statement, given the R3 fact the And/Or arms depend on.  A
    tree induction can discharge the hypothesis with any invariant that keeps
    typed-valid unrefused Pars exact. *)
Theorem name_and_pathmap_result_on_typed_valid_given_deep_par_exactness :
  (forall subject,
    typed_valid_tree subject ->
    sem_type_id (semantic_reduce subject) = 1 ->
    sem_definitely_refused (semantic_reduce subject) = false ->
    exact_evidence (semantic_reduce subject) = true) ->
  forall field children,
    typed_valid_tree (Node field children) ->
    Forall_forest (fun child =>
      reducer_result_acceptable (semantic_reduce child) = true) children ->
    In (score_class_of (sem_type_id (semantic_reduce (Node field children))))
      [ClassBindPayload; ClassPathMapPayload; ClassNameComponent] ->
    reducer_result_acceptable (semantic_reduce (Node field children)) = true.
Proof.
  intros deep_par field children valid accepted member.
  apply (name_and_pathmap_result_on_typed_valid_given_exact_carriers field
    children valid accepted); [|exact member].
  intros _.
  apply (forall_forest_impl typed_valid_tree _ children);
    [|exact (typed_valid_node_children field children valid)].
  intros [child_field grandchildren] child_valid type_is unrefused.
  cbn [semantic_reduce] in type_is, unrefused |- *.
  destruct (unrefused_node_is_resolved child_field _ unrefused)
    as [program resolved].
  pose proof (reduce_semantic_field_type_id child_field
    (semantic_reduce_forest grandchildren) program resolved) as program_type.
  rewrite type_is in program_type.
  symmetry in program_type.
  pose proof (generated_program_is_lookup_of_its_family child_field program 57
    resolved program_type (ltac:(discriminate) : 57 <> 0)) as same_program.
  subst program.
  pose proof (unrefused_node_has_valid_scan child_field _ _ resolved unrefused)
    as scan_ok.
  pose proof (scanned_children_fit _ _ _ scan_ok) as fits.
  pose proof (typed_valid_node_children child_field grandchildren child_valid)
    as grandchildren_valid.
  apply (connective_body_carrier_is_exact_of_exact_elements child_field
    grandchildren type_is unrefused).
  intros element member' same_tag.
  destruct (message_child_facts _ element 1 1 (fits element member') same_tag
    connective_body_tables)
    as [element_unrefused [_ [_ element_type]]].
  destruct (forall_forest_member typed_valid_tree grandchildren element
    grandchildren_valid member') as [subject [same [subject_valid _]]].
  subst element.
  exact (deep_par subject subject_valid element_type element_unrefused).
Qed.

(** ** Necessity of the carrier hypothesis

    An unrefused 55 node whose single And or Or child is an unrefused carrier
    with no exact score has no exact score itself and is not acceptable.  So
    the added hypothesis is not a convenience: on those arms it is exactly
    what the conclusion needs. *)
Theorem connective_body_arm_needs_exact_carrier :
  forall field child,
    sem_type_id (semantic_reduce (Node field (FCons child FNil))) = 55 ->
    sem_definitely_refused (semantic_reduce (Node field (FCons child FNil))) = false ->
    (sem_tag (semantic_reduce child) = 1 \/ sem_tag (semantic_reduce child) = 2) ->
    sem_score_evidence (semantic_reduce child) = ScoreNotDerived ->
    reducer_result_acceptable (semantic_reduce (Node field (FCons child FNil))) = false.
Proof.
  intros field child type_is unrefused tag_case not_derived.
  cbn [semantic_reduce semantic_reduce_forest] in type_is, unrefused |- *.
  destruct (unrefused_node_is_resolved field _ unrefused) as [program resolved].
  pose proof (reduce_semantic_field_type_id field [semantic_reduce child] program
    resolved) as program_type.
  rewrite type_is in program_type.
  symmetry in program_type.
  pose proof (generated_program_is_lookup_of_its_family field program 55 resolved
    program_type (ltac:(discriminate) : 55 <> 0)) as same_program.
  subst program.
  pose proof (unrefused_node_has_valid_scan field _ _ resolved unrefused)
    as scan_ok.
  pose proof (scanned_children_fit _ _ _ scan_ok) as fits.
  pose proof (fits (semantic_reduce child) (or_introl eq_refl)) as only_fits.
  destruct connective_tables
    as [_ [_ [table_one [table_two _]]]].
  assert (child_facts :
    sem_definitely_refused (semantic_reduce child) = false /\
    sem_wire (semantic_reduce child) = LengthDelimited /\
    sem_payload (semantic_reduce child) = Message).
  { destruct tag_case as [tag|tag].
    - destruct (message_child_facts _ _ 1 57 only_fits tag table_one)
        as [child_unrefused [child_wire [child_payload _]]].
      exact (conj child_unrefused (conj child_wire child_payload)).
    - destruct (message_child_facts _ _ 2 57 only_fits tag table_two)
        as [child_unrefused [child_wire [child_payload _]]].
      exact (conj child_unrefused (conj child_wire child_payload)). }
  destruct child_facts as [child_unrefused [child_wire child_payload]].
  assert (typed_score_absent :
    exact_typed_message_score 57 (semantic_reduce child) = None).
  { unfold exact_typed_message_score.
    rewrite not_derived.
    destruct ((sem_type_id (semantic_reduce child) =? 57) &&
      wire_eqb (sem_wire (semantic_reduce child)) LengthDelimited &&
      payload_eqb (sem_payload (semantic_reduce child)) Message &&
      negb (sem_definitely_refused (semantic_reduce child)));
      reflexivity. }
  assert (arm_absent :
    exact_connective_message_arm (semantic_reduce child) = None).
  { unfold exact_connective_message_arm.
    destruct tag_case as [tag|tag]; rewrite tag;
      unfold exact_connective_body_arm;
      rewrite typed_score_absent;
      reflexivity. }
  assert (score_absent :
    exact_connective_score [semantic_reduce child] = None).
  { unfold exact_connective_score.
    cbv beta iota.
    rewrite child_wire, child_unrefused, child_payload.
    cbv beta iota.
    assert (not_three : (sem_tag (semantic_reduce child) =? 3) = false).
    { destruct tag_case as [tag|tag]; rewrite tag; reflexivity. }
    rewrite not_three.
    destruct (connective_scalar_constructor (sem_tag (semantic_reduce child)));
      cbn [andb]; exact arm_absent. }
  assert (no_evidence :
    sem_score_evidence (reduce_semantic_field field [semantic_reduce child]) =
    ScoreNotDerived).
  { rewrite (reduce_semantic_field_score_evidence field _ _ resolved),
      program_type, score_evidence_for_connective, score_absent.
    reflexivity. }
  assert (complete :
    sem_complete (reduce_semantic_field field [semantic_reduce child]) = Some true).
  { rewrite (reduce_semantic_field_complete field _ _ resolved), program_type.
    reflexivity. }
  assert (payload_message : raw_payload field = Message).
  { unfold generated_wire_program in resolved.
    destruct (raw_payload field) eqn:field_payload;
      destruct (raw_program_type_id field) as [declared|];
      try discriminate resolved;
      try reflexivity. }
  unfold reducer_result_acceptable.
  rewrite unrefused.
  rewrite (reduce_semantic_field_payload field), payload_message.
  cbn [orb payload_eqb].
  unfold class_result_ok.
  rewrite (reduce_semantic_field_type_id field _ _ resolved), program_type,
    score_class_of_connective.
  cbv beta iota.
  rewrite complete.
  cbv beta iota.
  unfold exact_evidence.
  rewrite no_evidence.
  reflexivity.
Qed.

(** ** Why the requested statement is not derivable locally

    The class table makes a 57 carrier acceptable with no condition, yet the
    And/Or arm of a 55 parent needs that carrier's exact score.  This closed
    statement exhibits the gap at the level of summaries; lifting it to a
    typed-valid tree would need a typed-valid unrefused Par without an exact
    score, i.e. a counterexample to R3 itself. *)
Definition carrier_summary_without_score : semantic_summary :=
  SemanticSummary 1 LengthDelimited Message 57 [] false None ScoreNotDerived
    false false.

Definition connective_field_for_gap : raw_field :=
  RawField 8 LengthDelimited Message [] 0 (Some 55) [] [].

Theorem connective_arm_gap_at_summary_level :
  reducer_result_acceptable carrier_summary_without_score = true /\
  sem_definitely_refused
    (reduce_semantic_field connective_field_for_gap
      [carrier_summary_without_score]) = false /\
  sem_complete
    (reduce_semantic_field connective_field_for_gap
      [carrier_summary_without_score]) = Some true /\
  sem_score_evidence
    (reduce_semantic_field connective_field_for_gap
      [carrier_summary_without_score]) = ScoreNotDerived /\
  reducer_result_acceptable
    (reduce_semantic_field connective_field_for_gap
      [carrier_summary_without_score]) = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

(** ** Green control: the And arm on a typed-valid tree

    The wire-faithful tree of a Connective holding [ConnAndBody] over one
    empty Par.  The carrier hypothesis of the strengthened theorem is
    satisfied on it and the real reducer accepts the node, so that hypothesis
    is not vacuous at the arm that needs it. *)
Definition empty_par_element : @tree raw_field :=
  message_node 1 1 [] FNil.

Definition and_body_carrier : @tree raw_field :=
  message_node 1 57 [10; 0] (FCons empty_par_element FNil).

Definition and_connective_node : @tree raw_field :=
  message_node 8 55 [10; 2; 10; 0] (FCons and_body_carrier FNil).

Theorem and_arm_green_control :
  typed_valid_tree and_connective_node /\
  sem_type_id (semantic_reduce and_body_carrier) = 57 /\
  sem_definitely_refused (semantic_reduce and_body_carrier) = false /\
  exact_evidence (semantic_reduce and_body_carrier) = true /\
  sem_definitely_refused (semantic_reduce and_connective_node) = false /\
  sem_complete (semantic_reduce and_connective_node) = Some true /\
  exact_evidence (semantic_reduce and_connective_node) = true /\
  reducer_result_acceptable (semantic_reduce and_connective_node) = true.
Proof.
  split.
  - apply (proj1 (typed_valid_treeb_reflect _)).
    vm_compute.
    reflexivity.
  - vm_compute.
    repeat split; reflexivity.
Qed.

(** ** Red control: a missing case of one family fails the theorem

    The mutant removes the [60, Some true] arm of [score_evidence_for]; such a
    call then falls through to the expression-payload default, which has no
    arm for type 60, so the node has no score.  On the typed-valid witness the
    original reducer's result is acceptable and the mutant's is not. *)
Definition score_evidence_for_without_60 (type_id : nat) (complete : option bool)
    (children : list semantic_summary) : score_evidence :=
  if type_id =? 60
  then exact_or_unknown (exact_expr_payload_score type_id children)
  else score_evidence_for type_id complete children.

Definition reduce_without_arm_60 (subject : @tree raw_field) : semantic_summary :=
  match subject with
  | Node field children =>
      let summary := reduce_semantic_field field (semantic_reduce_forest children) in
      SemanticSummary (sem_tag summary) (sem_wire summary) (sem_payload summary)
        (sem_type_id summary) (sem_body summary)
        (sem_definitely_refused summary) (sem_complete summary)
        (score_evidence_for_without_60 (sem_type_id summary)
          (sem_complete summary) (semantic_reduce_forest children))
        (sem_bind_source_present summary)
        (sem_case_required_present summary)
  end.

Theorem red_control_missing_arm_60_breaks_the_theorem :
  typed_valid_tree gunforgeable_token_witness /\
  reducer_result_acceptable (semantic_reduce gunforgeable_token_witness) = true /\
  sem_definitely_refused (reduce_without_arm_60 gunforgeable_token_witness) = false /\
  sem_complete (reduce_without_arm_60 gunforgeable_token_witness) = Some true /\
  sem_score_evidence (reduce_without_arm_60 gunforgeable_token_witness) =
    ScoreNotDerived /\
  reducer_result_acceptable (reduce_without_arm_60 gunforgeable_token_witness) =
    false.
Proof.
  split.
  - apply (proj1 (typed_valid_treeb_reflect _)).
    vm_compute.
    reflexivity.
  - vm_compute.
    repeat split; reflexivity.
Qed.

(** ** Assumption audit: one line per exported lemma *)

Print Assumptions refused_result_is_acceptable.
Print Assumptions class_result_ok_makes_acceptable.
Print Assumptions acceptable_unrefused_summary_class_ok.
Print Assumptions score_class_of_par.
Print Assumptions score_class_of_var.
Print Assumptions score_class_of_cost_region.
Print Assumptions score_class_of_receive_bind.
Print Assumptions score_class_of_match_case.
Print Assumptions score_class_of_ezipper.
Print Assumptions score_class_of_connective.
Print Assumptions score_class_of_unforgeable.
Print Assumptions class_result_ok_of_bind_payload.
Print Assumptions class_result_ok_of_name_component.
Print Assumptions class_result_ok_of_incomplete_pathmap.
Print Assumptions bind_payload_required_child_absent_of_receive_bind.
Print Assumptions bind_payload_required_child_absent_of_match_case.
Print Assumptions payload_eqb_true_implies_equal.
Print Assumptions reduce_semantic_field_score_evidence.
Print Assumptions reduce_semantic_field_bind_source_present.
Print Assumptions reduce_semantic_field_case_required_present.
Print Assumptions unrefused_node_has_valid_scan.
Print Assumptions unrefused_node_is_resolved.
Print Assumptions unrefused_node_body_is_valid.
Print Assumptions reduce_semantic_field_no_program_type.
Print Assumptions semantic_reduce_exposes_field.
Print Assumptions scan_child_valid_step.
Print Assumptions fold_scan_valid_after_step.
Print Assumptions scanned_children_fit.
Print Assumptions resolve_tag_has_requested_tag.
Print Assumptions seen_slot_has_child.
Print Assumptions seen_slot_child_has_tag.
Print Assumptions all_fields_in_slot_zero_facts.
Print Assumptions slot_zero_scan_has_at_most_one_child.
Print Assumptions forall_forest_member.
Print Assumptions forall_forest_conj.
Print Assumptions forall_forest_impl.
Print Assumptions forest_member_is_accepted.
Print Assumptions typed_valid_node_children.
Print Assumptions exact_evidence_gives_score.
Print Assumptions exact_evidence_of_score.
Print Assumptions typed_message_score_of_evidence.
Print Assumptions typed_message_score_of_exact.
Print Assumptions find_tag_selects_member.
Print Assumptions exact_typed_child_score_exists.
Print Assumptions exact_typed_selected_scores_exists.
Print Assumptions exact_typed_optional_child_score_exists.
Print Assumptions exact_typed_optional_score_or_exists.
Print Assumptions exact_receive_bind_score_exists.
Print Assumptions exact_match_case_score_exists.
Print Assumptions exact_pbint32_or_absent_exists.
Print Assumptions exact_i32_or_absent_exists.
Print Assumptions pbbool_scalar_has_value.
Print Assumptions pbint32_scalar_has_value.
Print Assumptions pbsint32_scalar_has_value.
Print Assumptions forbidden_bytes_scalar_has_value.
Print Assumptions scalar_child_body_valid.
Print Assumptions message_child_facts.
Print Assumptions scalar_child_facts.
Print Assumptions fitted_tag_in_list.
Print Assumptions accepted_par_field_has_score.
Print Assumptions par_child_is_exact.
Print Assumptions cost_child_is_exact.
Print Assumptions var_child_is_exact.
Print Assumptions var_summary_is_complete.
Print Assumptions score_evidence_for_receive_bind.
Print Assumptions receive_bind_tables.
Print Assumptions family_16_class_ok.
Print Assumptions score_evidence_for_match_case.
Print Assumptions match_case_tables.
Print Assumptions family_21_class_ok.
Print Assumptions family_31_result.
Print Assumptions family_32_result.
Print Assumptions score_evidence_for_var_ref.
Print Assumptions score_evidence_for_connective_body.
Print Assumptions var_ref_tables.
Print Assumptions var_ref_carrier_is_exact.
Print Assumptions connective_body_tables.
Print Assumptions connective_body_carrier_is_exact_of_exact_elements.
Print Assumptions connective_body_carrier_is_exact_of_acceptable_children.
Print Assumptions connective_body_carrier_score_shape.
Print Assumptions connective_body_carrier_exact_of_acceptable_children.
Print Assumptions score_evidence_for_connective.
Print Assumptions connective_tables.
Print Assumptions connective_score_of_message_arm.
Print Assumptions connective_score_of_not_arm.
Print Assumptions connective_body_arm_of_carrier.
Print Assumptions connective_and_or_arm_score.
Print Assumptions connective_var_ref_arm_score.
Print Assumptions connective_scalar_arm_score.
Print Assumptions connective_score_exists.
Print Assumptions family_55_class_ok.
Print Assumptions varint_groups_from_suffix_is_shorter.
Print Assumptions varint_groups_from_agrees_with_cursor.
Print Assumptions framing_field_length_delimited.
Print Assumptions checked_field_length_delimited.
Print Assumptions reference_remaining_at_end.
Print Assumptions worklist_single_field.
Print Assumptions whole_region_is_ok.
Print Assumptions decode_region_single_scalar_field.
Print Assumptions leaf_program_facts.
Print Assumptions leaf_span_source_descriptor.
Print Assumptions leaf_descriptor_slot_admissible.
Print Assumptions leaf_span_body_length.
Print Assumptions exact_unforgeable_leaf_bytes_of_decoded.
Print Assumptions reread_bytes_grammar_parts.
Print Assumptions bytes_leaf_has_exact_bytes.
Print Assumptions reread_grammar_of_typed_valid_node.
Print Assumptions score_evidence_for_unforgeable.
Print Assumptions unforgeable_tables.
Print Assumptions unforgeable_score_of_bytes_arm.
Print Assumptions unforgeable_score_of_token_arm.
Print Assumptions unforgeable_child_score.
Print Assumptions unforgeable_score_exists.
Print Assumptions family_60_class_ok.
Print Assumptions name_and_pathmap_class_members.
Print Assumptions name_and_pathmap_result_on_typed_valid_given_exact_carriers.
Print Assumptions name_and_pathmap_result_on_typed_valid_off_connective_family.
Print Assumptions name_and_pathmap_result_on_typed_valid_off_connective_body_arms.
Print Assumptions name_and_pathmap_result_on_typed_valid_given_deep_par_exactness.
Print Assumptions connective_body_arm_needs_exact_carrier.
Print Assumptions connective_arm_gap_at_summary_level.
Print Assumptions and_arm_green_control.
Print Assumptions red_control_missing_arm_60_breaks_the_theorem.
