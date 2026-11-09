From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import StackSafePDA RPB3GeneratedFieldRefinement
  RPB3SemanticReducer RPB3SemanticWorklist.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Semantic := RPB3SemanticReducer.
Module Worklist := RPB3SemanticWorklist.

Definition mutant_skip_component_order (field : Schema.raw_field)
    (children : list Semantic.semantic_summary)
    : Semantic.semantic_summary :=
  let original := Semantic.reduce_semantic_field field children in
  let resolved := Semantic.generated_wire_program field in
  let program := match resolved with
    | Some program => program
    | None => Schema.empty_program
    end in
  let scanned := fold_left (Semantic.scan_child program)
    children Schema.empty_scan in
  let scalar_has_no_children :=
    match Schema.raw_payload field, children with
    | Schema.Scalar, _ :: _ => false
    | _, _ => true
    end in
  let refused_without_order :=
    negb (match resolved with Some _ => true | None => false end) ||
    negb (Schema.valid_raw_body field) ||
    negb scalar_has_no_children ||
    negb (Schema.scan_valid scanned) ||
    negb (Semantic.mandatory_pathmaps_present program scanned) in
  Semantic.SemanticSummary
    (Semantic.sem_tag original) (Semantic.sem_wire original)
    (Semantic.sem_payload original) (Semantic.sem_type_id original)
    (Semantic.sem_body original) refused_without_order
    (Semantic.sem_complete original) (Semantic.sem_score_evidence original)
    (Semantic.sem_bind_source_present original)
    (Semantic.sem_case_required_present original).

Definition reversed_bundle_root_field : Schema.raw_field :=
  match Semantic.reversed_bundle_scores with
  | Node field _ => field
  end.

Theorem omitting_exact_score_order_accepts_reversed_bundles :
  let children :=
    [Semantic.semantic_reduce Semantic.write_only_bundle;
     Semantic.semantic_reduce Semantic.read_only_bundle] in
  Semantic.sem_definitely_refused
    (Semantic.reduce_semantic_field reversed_bundle_root_field children) = true /\
  Semantic.sem_definitely_refused
    (mutant_skip_component_order reversed_bundle_root_field children) = false.
Proof. vm_compute. split; reflexivity. Qed.

Definition mutant_reverse_completed_children (field : Schema.raw_field)
    (children : list Semantic.semantic_summary)
    : Semantic.semantic_summary :=
  Semantic.reduce_semantic_field field (rev children).

Theorem reversing_completed_children_changes_bundle_order_and_score :
  let children :=
    [Semantic.semantic_reduce Semantic.write_only_bundle;
     Semantic.semantic_reduce Semantic.read_only_bundle] in
  let correct := Semantic.reduce_semantic_field
    reversed_bundle_root_field children in
  let reversed := mutant_reverse_completed_children
    reversed_bundle_root_field children in
  Semantic.sem_definitely_refused correct = true /\
  Semantic.sem_definitely_refused reversed = false /\
  Semantic.sem_score_evidence correct <>
    Semantic.sem_score_evidence reversed.
Proof. vm_compute. repeat split; try reflexivity; discriminate. Qed.

Definition mutant_discard_original_body (field : Schema.raw_field)
    (children : list Semantic.semantic_summary)
    : Semantic.semantic_summary :=
  let original := Semantic.reduce_semantic_field field children in
  Semantic.SemanticSummary
    (Semantic.sem_tag original) (Semantic.sem_wire original)
    (Semantic.sem_payload original) (Semantic.sem_type_id original)
    [] (Semantic.sem_definitely_refused original)
    (Semantic.sem_complete original) (Semantic.sem_score_evidence original)
    (Semantic.sem_bind_source_present original)
    (Semantic.sem_case_required_present original).

Definition nonempty_root_field : Schema.raw_field :=
  Schema.RawField 0 Schema.LengthDelimited Schema.Message [8; 1] 2
    (Some 1) [] [].

Theorem discarding_the_original_root_body_changes_the_summary :
  Semantic.sem_body (Semantic.reduce_semantic_field nonempty_root_field []) =
    [8; 1] /\
  Semantic.sem_body (mutant_discard_original_body nonempty_root_field []) = [].
Proof. vm_compute. split; reflexivity. Qed.

Print Assumptions omitting_exact_score_order_accepts_reversed_bundles.
Print Assumptions reversing_completed_children_changes_bundle_order_and_score.
Print Assumptions discarding_the_original_root_body_changes_the_summary.
