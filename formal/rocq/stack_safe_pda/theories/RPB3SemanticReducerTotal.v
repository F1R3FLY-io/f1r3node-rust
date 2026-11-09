(** RPB3SemanticReducerTotal: the semantic reducer returns a result on every
    typed-valid input.

    [semantic_reducer_total_on_typed_valid] is proved by induction on the raw
    tree.  [reducer_result_acceptable] holds of a summary that is definitely
    refused, that is Scalar, or that satisfies the acceptance row of its score
    class, and [typed_valid_tree] does not mention scores, so the hypothesis
    cannot make the theorem vacuous.

    At a node the score class of the result selects the argument:

    - Par, ProcessComponent, ExprComponent, CostComposite and CostPart go
      through [process_and_cost_result_on_typed_valid];
    - BindPayload, NameComponent and PathMapPayload go through
      [name_and_pathmap_result_on_typed_valid_given_exact_carriers];
    - ExprPayload and ParentCarrier go through
      [expr_payload_result_on_typed_valid_with_carrier_keys];
    - NoStandalone and Opaque are the types with no score arm, so the evidence
      is [ScoreNotDerived] whatever the children are;
    - Unclassified cannot arise from a resolved Message, because every
      generated type has a class ([score_class_total]); an unresolved field is
      refused and a resolved Scalar is acceptable by its payload.

    Two of the family statements take more than the acceptance of the direct
    children.  The Connective And and Or arms read the score of a
    ConnectiveBody carrier and the EMap arm reads the key score of a
    KeyValuePair carrier, and the acceptance row of a parent carrier is
    unconditional, so the acceptance of the carrier itself says nothing about
    what those arms read.  Those premises are facts about grandchildren.  The
    induction therefore proves, of each node, that it is acceptable and that
    its children are, which at the parent gives the acceptance of the
    grandchildren. *)

From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import StackSafePDA RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree RPB3SemanticReducer RPB3ScoreClass RPB3TypedValid
  RPB3TotalProcessAndCostFamilies RPB3TotalNameAndPathMapFamilies
  RPB3TotalExprPayloadFamilies.

Import ListNotations.

Local Notation acceptable :=
  (fun subject : @tree raw_field =>
    reducer_result_acceptable (semantic_reduce subject) = true).

(** * Types with no score arm *)

Definition no_score_type_ids : list nat :=
  [7; 25; 26; 61; 62; 63; 64] ++ [2; 3; 4; 5; 14; 17; 18; 58; 59].

Lemma listed_score_type_member :
  forall type_id ids, listed_score_type type_id ids = true -> In type_id ids.
Proof.
  intros type_id ids listed.
  unfold listed_score_type in listed.
  apply existsb_exists in listed as [id [member same]].
  apply Nat.eqb_eq in same.
  subst id.
  exact member.
Qed.

Lemma no_score_class_is_listed :
  forall type_id,
    score_class_of type_id = ClassNoStandalone \/
    score_class_of type_id = ClassOpaque ->
    In type_id no_score_type_ids.
Proof.
  intros type_id classified.
  unfold score_class_of in classified.
  destruct (Nat.eqb type_id 1);
    [destruct classified as [c|c]; discriminate c|].
  destruct (listed_score_type type_id [8; 15; 19; 20; 22; 23]);
    [destruct classified as [c|c]; discriminate c|].
  destruct (Nat.eqb type_id 24);
    [destruct classified as [c|c]; discriminate c|].
  destruct (Nat.eqb type_id 13);
    [destruct classified as [c|c]; discriminate c|].
  destruct (listed_score_type type_id [9; 10; 11; 12]);
    [destruct classified as [c|c]; discriminate c|].
  destruct (listed_score_type type_id [16; 21]);
    [destruct classified as [c|c]; discriminate c|].
  destruct (listed_score_type type_id [55; 60]);
    [destruct classified as [c|c]; discriminate c|].
  destruct (listed_score_type type_id [31; 32]);
    [destruct classified as [c|c]; discriminate c|].
  destruct (listed_score_type type_id expr_payload_type_ids);
    [destruct classified as [c|c]; discriminate c|].
  destruct (listed_score_type type_id [34; 56; 57]);
    [destruct classified as [c|c]; discriminate c|].
  destruct (listed_score_type type_id [7; 25; 26; 61; 62; 63; 64])
    eqn:no_standalone.
  { apply listed_score_type_member in no_standalone.
    unfold no_score_type_ids.
    apply in_or_app.
    left.
    exact no_standalone. }
  destruct (listed_score_type type_id [2; 3; 4; 5; 14; 17; 18; 58; 59])
    eqn:opaque.
  { apply listed_score_type_member in opaque.
    unfold no_score_type_ids.
    apply in_or_app.
    right.
    exact opaque. }
  destruct classified as [c|c]; discriminate c.
Qed.

Lemma no_score_type_has_no_evidence :
  forall type_id complete children,
    In type_id no_score_type_ids ->
    score_evidence_for type_id complete children = ScoreNotDerived.
Proof.
  intros type_id complete children member.
  unfold no_score_type_ids in member.
  cbn [app] in member.
  repeat (destruct member as [same|member];
    [subst type_id; destruct complete as [[|]|]; reflexivity|]).
  destruct member.
Qed.

Lemma evidence_is_dispatched_on_the_result :
  forall field children,
    sem_score_evidence (semantic_reduce (Node field children)) =
    score_evidence_for
      (sem_type_id (semantic_reduce (Node field children)))
      (sem_complete (semantic_reduce (Node field children)))
      (semantic_reduce_forest children).
Proof.
  intros field children.
  reflexivity.
Qed.

Lemma no_score_class_is_acceptable :
  forall field children,
    score_class_of (sem_type_id (semantic_reduce (Node field children)))
      = ClassNoStandalone \/
    score_class_of (sem_type_id (semantic_reduce (Node field children)))
      = ClassOpaque ->
    reducer_result_acceptable (semantic_reduce (Node field children)) = true.
Proof.
  intros field children classified.
  assert (no_exact :
    sem_score_evidence (semantic_reduce (Node field children)) =
    ScoreNotDerived).
  { rewrite evidence_is_dispatched_on_the_result.
    exact (no_score_type_has_no_evidence _ _ _
      (no_score_class_is_listed _ classified)). }
  assert (class_ok :
    class_result_ok (semantic_reduce (Node field children)) = true).
  { unfold class_result_ok, exact_evidence.
    destruct classified as [class|class]; rewrite class, no_exact; reflexivity. }
  unfold reducer_result_acceptable.
  rewrite class_ok, orb_true_r.
  reflexivity.
Qed.

(** * A generated type always has a class *)

Lemma resolved_program_is_classified :
  forall type_id program,
    lookup_projected_program type_id [] [] = Some program ->
    score_class_of (program_family program) <> Unclassified.
Proof.
  intros type_id program found unclassified.
  unfold lookup_projected_program in found.
  destruct (find (fun program => GeneratedData.program_type_id program =? type_id)
      GeneratedData.generated_programs) as [source|] eqn:searched;
    [|discriminate found].
  cbn [option_map] in found.
  injection found as same.
  subst program.
  change (score_class_of (GeneratedData.program_type_id source) = Unclassified)
    in unclassified.
  apply find_some in searched as [member _].
  assert (listed :
    In (GeneratedData.program_type_id source) generated_type_ids).
  { unfold generated_type_ids.
    exact (in_map _ _ _ member). }
  pose proof (proj2 (proj2 score_class_total)) as none.
  assert (reported :
    In (GeneratedData.program_type_id source) unclassified_types).
  { unfold unclassified_types, unclassified_ids_of.
    apply filter_In.
    split; [exact listed|].
    rewrite unclassified.
    reflexivity. }
  rewrite none in reported.
  exact reported.
Qed.

Lemma type_id_of_resolved_field :
  forall field children program,
    generated_wire_program field = Some program ->
    sem_type_id (reduce_semantic_field field children) = program_family program.
Proof.
  intros field children program resolved.
  unfold reduce_semantic_field.
  rewrite resolved.
  reflexivity.
Qed.

Lemma unclassified_node_is_acceptable :
  forall field children,
    score_class_of (sem_type_id (semantic_reduce (Node field children)))
      = Unclassified ->
    reducer_result_acceptable (semantic_reduce (Node field children)) = true.
Proof.
  intros field children unclassified.
  change (semantic_reduce (Node field children))
    with (reduce_semantic_field field (semantic_reduce_forest children)) in *.
  destruct (generated_wire_program field) as [program|] eqn:resolved.
  - pose proof resolved as resolved_program.
    unfold generated_wire_program in resolved.
    destruct (raw_payload field) eqn:payload;
      destruct (raw_program_type_id field) as [type_id|] eqn:program_id;
      try discriminate resolved.
    + assert (scalar :
        payload_eqb
          (sem_payload
            (reduce_semantic_field field (semantic_reduce_forest children)))
          Scalar = true).
      { change (sem_payload
            (reduce_semantic_field field (semantic_reduce_forest children)))
          with (raw_payload field).
        rewrite payload.
        reflexivity. }
      unfold reducer_result_acceptable.
      rewrite scalar, orb_true_r.
      reflexivity.
    + exfalso.
      rewrite (type_id_of_resolved_field field (semantic_reduce_forest children)
        program resolved_program) in unclassified.
      exact (resolved_program_is_classified type_id program resolved unclassified).
  - unfold reducer_result_acceptable.
    rewrite (no_program_field_is_refused_by_the_current_reducer field
      (semantic_reduce_forest children) resolved).
    reflexivity.
Qed.

(** * One node *)

Ltac class_listed class :=
  rewrite class;
  cbn [In];
  repeat (first [left; reflexivity | right]).

Theorem typed_valid_node_is_acceptable :
  forall field children,
    typed_valid_tree (Node field children) ->
    Forall_forest acceptable children ->
    Forall_forest
      (fun child => Forall_forest acceptable (tree_children child)) children ->
    reducer_result_acceptable (semantic_reduce (Node field children)) = true.
Proof.
  intros field children valid children_ok grandchildren_ok.
  assert (carriers_exact : Forall_forest connective_body_carrier_exact children).
  { refine (Forall_forest_impl _ _ children _ grandchildren_ok).
    intros [child_field child_children] accepted.
    exact (connective_body_carrier_exact_of_acceptable_children
      child_field child_children accepted). }
  pose proof (kvp_keys_acceptable_of_grandchildren_acceptable
    children grandchildren_ok) as keys.
  destruct (score_class_of (sem_type_id (semantic_reduce (Node field children))))
    eqn:class.
  1-5:
    apply (process_and_cost_result_on_typed_valid field children valid children_ok);
    class_listed class.
  1-3:
    apply (name_and_pathmap_result_on_typed_valid_given_exact_carriers
      field children valid children_ok (fun _ => carriers_exact));
    class_listed class.
  1-2:
    apply (expr_payload_result_on_typed_valid_with_carrier_keys
      field children valid children_ok keys);
    class_listed class.
  - exact (no_score_class_is_acceptable field children (or_introl class)).
  - exact (no_score_class_is_acceptable field children (or_intror class)).
  - exact (unclassified_node_is_acceptable field children class).
Qed.

(** * The induction *)

(** The motive also returns the acceptance of the node's children, which at the
    parent is the acceptance of the grandchildren that two family statements
    need. *)
Theorem typed_valid_acceptable_with_children :
  (forall subject : @tree raw_field,
    typed_valid_tree subject ->
    acceptable subject /\ Forall_forest acceptable (tree_children subject)) /\
  (forall subjects : @forest raw_field,
    Forall_forest typed_valid_tree subjects ->
    Forall_forest acceptable subjects /\
    Forall_forest
      (fun subject => Forall_forest acceptable (tree_children subject)) subjects).
Proof.
  apply tree_forest_induction.
  - intros field children children_step valid.
    destruct (proj1 (typed_valid_tree_node_iff field children) valid)
      as [_ [_ [_ children_valid]]].
    destruct (children_step children_valid) as [children_ok grandchildren_ok].
    split.
    + exact (typed_valid_node_is_acceptable field children valid
        children_ok grandchildren_ok).
    + exact children_ok.
  - intros _.
    split; constructor.
  - intros head head_step rest rest_step all_valid.
    inversion all_valid as [|h r head_valid rest_valid]; subst.
    destruct (head_step head_valid) as [head_ok head_children_ok].
    destruct (rest_step rest_valid) as [rest_ok rest_children_ok].
    split; constructor; assumption.
Qed.

(** The semantic reducer returns a result for every typed-valid input. *)
Theorem semantic_reducer_total_on_typed_valid :
  forall subject,
    typed_valid_tree subject ->
    reducer_result_acceptable (semantic_reduce subject) = true.
Proof.
  intros subject valid.
  exact (proj1 (proj1 typed_valid_acceptable_with_children subject valid)).
Qed.

(** * Red control: a reducer with one unhandled typed-valid case

    [arm_28_dropped_evidence] removes the score arm of ETuple, type 28, at
    [Some true].  An ETuple over one empty Par is typed-valid, the reducer
    accepts it and the mutant does not, so the theorem is false of the mutant
    and its proof depends on the arm. *)

Definition arm_28_dropped_evidence (type_id : nat) (complete : option bool)
    (children : list semantic_summary) : score_evidence :=
  match type_id, complete with
  | 28, Some true => ScoreNotDerived
  | _, _ => score_evidence_for type_id complete children
  end.

Definition arm_28_dropped_reduce_field (field : raw_field)
    (children : list semantic_summary) : semantic_summary :=
  let original := reduce_semantic_field field children in
  SemanticSummary (sem_tag original) (sem_wire original) (sem_payload original)
    (sem_type_id original) (sem_body original) (sem_definitely_refused original)
    (sem_complete original)
    (arm_28_dropped_evidence (sem_type_id original) (sem_complete original)
      children)
    (sem_bind_source_present original) (sem_case_required_present original).

Fixpoint arm_28_dropped_reduce (subject : @tree raw_field) : semantic_summary :=
  match subject with
  | Node field children =>
      arm_28_dropped_reduce_field field (arm_28_dropped_reduce_forest children)
  end
with arm_28_dropped_reduce_forest (subjects : @forest raw_field)
    : list semantic_summary :=
  match subjects with
  | FNil => []
  | FCons subject rest =>
      arm_28_dropped_reduce subject :: arm_28_dropped_reduce_forest rest
  end.

Definition etuple_witness : @tree raw_field :=
  Node (RawField 0 LengthDelimited Message [10; 0] 2 (Some 28) [] [])
    (FCons (message_node 1 1 [] FNil) FNil).

Theorem etuple_witness_is_typed_valid : typed_valid_tree etuple_witness.
Proof. typed_valid_by_compute. Qed.

Example etuple_witness_is_accepted_with_an_exact_score :
  sem_definitely_refused (semantic_reduce etuple_witness) = false /\
  sem_complete (semantic_reduce etuple_witness) = Some true /\
  exact_evidence (semantic_reduce etuple_witness) = true /\
  reducer_result_acceptable (semantic_reduce etuple_witness) = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example etuple_witness_is_not_acceptable_without_arm_28 :
  sem_definitely_refused (arm_28_dropped_reduce etuple_witness) = false /\
  sem_complete (arm_28_dropped_reduce etuple_witness) = Some true /\
  sem_score_evidence (arm_28_dropped_reduce etuple_witness) = ScoreNotDerived /\
  reducer_result_acceptable (arm_28_dropped_reduce etuple_witness) = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Theorem arm_28_mutant_falsifies_totality :
  ~ (forall subject,
      typed_valid_tree subject ->
      reducer_result_acceptable (arm_28_dropped_reduce subject) = true).
Proof.
  intros claimed.
  pose proof (claimed etuple_witness etuple_witness_is_typed_valid) as accepted.
  rewrite (proj2 (proj2 (proj2 etuple_witness_is_not_acceptable_without_arm_28)))
    in accepted.
  discriminate accepted.
Qed.

Print Assumptions listed_score_type_member.
Print Assumptions no_score_class_is_listed.
Print Assumptions no_score_type_has_no_evidence.
Print Assumptions evidence_is_dispatched_on_the_result.
Print Assumptions no_score_class_is_acceptable.
Print Assumptions resolved_program_is_classified.
Print Assumptions type_id_of_resolved_field.
Print Assumptions unclassified_node_is_acceptable.
Print Assumptions typed_valid_node_is_acceptable.
Print Assumptions typed_valid_acceptable_with_children.
Print Assumptions semantic_reducer_total_on_typed_valid.
Print Assumptions etuple_witness_is_typed_valid.
Print Assumptions etuple_witness_is_accepted_with_an_exact_score.
Print Assumptions etuple_witness_is_not_acceptable_without_arm_28.
Print Assumptions arm_28_mutant_falsifies_totality.
