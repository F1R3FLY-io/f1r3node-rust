(** * Red controls for the Par family score order

    Production concatenates the component scores of a Par family by family in
    [PAR_SCORE_TAGS] order (models/src/rust/rholang/canonical_semantics.rs:140-151,
    walked by [reduce_raw_par] at protobuf_admission.rs:1549).  By Par field tag
    that order is 1, 2, 5, 4, 6, 11, 8, 7, 12, 13: Send, Receive, Expr, New,
    Match, Bundle, Connective, GUnforgeable, If, CostTokenStack.  The children
    of a canonical Par arrive in ascending field order, 1, 2, 4, 5, 6, 7, 8,
    11, 12, 13, so the two concatenations can disagree only on a Par that
    mixes New with Expr, or two of Bundle, Connective and GUnforgeable.

    [RPB3SemanticReducer.exact_par_component_scores] and
    [RPB3IndependentOracleRecursive.source_par_component_scores] use the
    production order.  The concatenation in child order, which they used
    before, is kept here only as the mutant [wire_order_par_score].  Every
    control is a closed computation; the PDA-versus-reference equivalence
    cannot see a wrong shared order, so these controls are what pin it down. *)

From Stdlib Require Import List Arith NArith.
From StackSafePDA Require RPB3GeneratedFieldRefinement RPB3SemanticReducer
  RPB3IndependentOracleCore RPB3IndependentOracleRecursive
  RPB3IndependentOracleProjection RPB3RecursiveAuxiliarySourceReference
  RPB3RootLocallyFreeQuotient.

Import ListNotations.

(** ** The order itself *)

(** The ten Par component field tags in ascending field order. *)
Definition par_field_order_tags : list nat :=
  [1; 2; 4; 5; 6; 7; 8; 11; 12; 13].

Theorem production_family_order_reorders_exactly_the_field_tags :
  length RPB3SemanticReducer.par_score_family_order =
    length par_field_order_tags /\
  forallb (fun tag =>
    count_occ Nat.eq_dec RPB3SemanticReducer.par_score_family_order tag =? 1)
    par_field_order_tags = true.
Proof. vm_compute. split; reflexivity. Qed.

Theorem production_family_order_differs_from_field_order :
  RPB3SemanticReducer.par_score_family_order <> par_field_order_tags.
Proof. vm_compute. intro same; discriminate same. Qed.

Theorem oracle_family_order_is_target_family_order :
  RPB3IndependentOracleRecursive.source_par_score_family_order =
    RPB3SemanticReducer.par_score_family_order.
Proof. reflexivity. Qed.

(** The ten tags are exactly the message fields of the generated Par program,
    so a family filter cannot miss a scored component of an actual Par. *)
Theorem generated_par_message_fields_are_the_field_order_tags :
  map RPB3GeneratedFieldRefinement.descriptor_tag
    (filter (fun field =>
      RPB3GeneratedFieldRefinement.payload_eqb
        (RPB3GeneratedFieldRefinement.descriptor_payload field)
        RPB3GeneratedFieldRefinement.Message)
      (RPB3GeneratedFieldRefinement.program_fields
        RPB3RootLocallyFreeQuotient.par_program)) =
  par_field_order_tags.
Proof. vm_compute. reflexivity. Qed.

(** ** The removed concatenation, kept as the mutant *)

Fixpoint wire_order_component_scores
    (children : list RPB3SemanticReducer.semantic_summary)
    : option (list RPB3SemanticReducer.canonical_score) :=
  match children with
  | [] => Some []
  | child :: rest =>
      match wire_order_component_scores rest with
      | None => None
      | Some scores =>
          match RPB3SemanticReducer.sem_payload child with
          | RPB3GeneratedFieldRefinement.Message =>
              match RPB3SemanticReducer.sem_score_evidence child with
              | RPB3SemanticReducer.ExactScore score => Some (score :: scores)
              | RPB3SemanticReducer.ScoreNotDerived => None
              end
          | RPB3GeneratedFieldRefinement.Scalar => Some scores
          | _ => None
          end
      end
  end.

Definition wire_order_par_score
    (children : list RPB3SemanticReducer.semantic_summary)
    : option RPB3SemanticReducer.canonical_score :=
  match wire_order_component_scores children,
      RPB3SemanticReducer.scalar_true_or_absent 10 children with
  | Some scores, Some connective_used =>
      Some (RPB3SemanticReducer.score_constructor 999%N
        (scores ++ [RPB3SemanticReducer.ScoreInteger
          (if connective_used then 1%N else 0%N)]))
  | _, _ => None
  end.

(** ** Red control: a real New-plus-Expr Par body

    The body is a Par with a New at field 4 ([34; 2; 18; 0]: its [p] field is an
    empty Par) followed by an Expr at field 5 ([42; 2; 16; 14]: GInt 7).  The
    two components therefore arrive New first, which is exactly where the
    wire-order concatenation and production disagree. *)
Definition new_plus_expr_par_body : list nat :=
  [34; 2; 18; 0; 42; 2; 16; 14].

Definition empty_par_score : RPB3SemanticReducer.canonical_score :=
  RPB3SemanticReducer.score_constructor 999%N
    [RPB3SemanticReducer.ScoreInteger 0%N].

Definition empty_new_score : RPB3SemanticReducer.canonical_score :=
  RPB3SemanticReducer.score_constructor 303%N
    [RPB3SemanticReducer.ScoreInteger 0%N;
     RPB3SemanticReducer.ScoreInteger 0%N;
     RPB3SemanticReducer.ScoreInteger 0%N;
     empty_par_score].

Definition gint_seven_score : RPB3SemanticReducer.canonical_score :=
  RPB3SemanticReducer.score_constructor 2%N
    [RPB3SemanticReducer.ScoreInteger 7%N].

(** Production order: Expr (family 5) before New (family 4). *)
Definition new_plus_expr_production_score : RPB3SemanticReducer.canonical_score :=
  RPB3SemanticReducer.score_constructor 999%N
    [gint_seven_score; empty_new_score; RPB3SemanticReducer.ScoreInteger 0%N].

(** Wire order: the New component first, as it arrives. *)
Definition new_plus_expr_wire_score : RPB3SemanticReducer.canonical_score :=
  RPB3SemanticReducer.score_constructor 999%N
    [empty_new_score; gint_seven_score; RPB3SemanticReducer.ScoreInteger 0%N].

(** The mutant and the reducer are compared on the children the reference
    itself computes for the body. *)
Example new_plus_expr_children_are_the_reference_children :
  option_map snd
    (RPB3RootLocallyFreeQuotient.wire_reference new_plus_expr_par_body) =
  RPB3RootLocallyFreeQuotient.wire_reduction
    RPB3SemanticReducer.reduce_semantic_field new_plus_expr_par_body.
Proof. vm_compute. reflexivity. Qed.

Theorem new_plus_expr_par_scores_differ_under_wire_and_production_order :
  option_map RPB3SemanticReducer.exact_par_score
    (RPB3RootLocallyFreeQuotient.witness_root_children
      new_plus_expr_par_body) =
    Some (Some new_plus_expr_production_score) /\
  option_map wire_order_par_score
    (RPB3RootLocallyFreeQuotient.witness_root_children
      new_plus_expr_par_body) =
    Some (Some new_plus_expr_wire_score) /\
  new_plus_expr_production_score <> new_plus_expr_wire_score.
Proof.
  vm_compute.
  split; [reflexivity|].
  split; [reflexivity|].
  intro same; discriminate same.
Qed.

(** Both reducers, from the public bytes, give the production-order score. *)
Theorem both_references_score_new_plus_expr_in_production_order :
  exists source_summary target_summary,
    RPB3IndependentOracleRecursive.source_par_reference (fun _ => 0)
      new_plus_expr_par_body = Some (false, source_summary) /\
    RPB3RecursiveAuxiliarySourceReference.recursive_par_reference (fun _ => 0)
      new_plus_expr_par_body = Some (false, target_summary) /\
    option_map RPB3IndependentOracleCore.project_score
      (RPB3IndependentOracleCore.source_summary_exact_score source_summary) =
    Some new_plus_expr_production_score /\
    RPB3SemanticReducer.sem_score_evidence target_summary =
      RPB3SemanticReducer.ExactScore new_plus_expr_production_score.
Proof.
  vm_compute.
  eexists. eexists.
  repeat split; reflexivity.
Qed.

(** ** Synthetic controls on labelled components *)

Definition target_component (tag : nat) (value : N)
    : RPB3SemanticReducer.semantic_summary :=
  RPB3SemanticReducer.SemanticSummary tag
    RPB3GeneratedFieldRefinement.LengthDelimited
    RPB3GeneratedFieldRefinement.Message 0 [] false None
    (RPB3SemanticReducer.ExactScore (RPB3SemanticReducer.ScoreInteger value))
    false false.

Definition target_par_score_of (values : list N)
    : RPB3SemanticReducer.canonical_score :=
  RPB3SemanticReducer.score_constructor 999%N
    (map RPB3SemanticReducer.ScoreInteger values ++
      [RPB3SemanticReducer.ScoreInteger 0%N]).

(** Bundle (11), Connective (8) and GUnforgeable (7) are reversed by production
    relative to their field order. *)
Example three_way_mix_uses_the_production_order :
  RPB3SemanticReducer.exact_par_score
    [target_component 7 7; target_component 8 8; target_component 11 11] =
    Some (target_par_score_of [11; 8; 7]%N) /\
  wire_order_par_score
    [target_component 7 7; target_component 8 8; target_component 11 11] =
    Some (target_par_score_of [7; 8; 11]%N).
Proof. vm_compute. split; reflexivity. Qed.

(** Occurrences inside one family keep their wire order; only the families are
    rearranged. *)
Example family_occurrences_keep_their_wire_order :
  RPB3SemanticReducer.exact_par_score
    [target_component 4 1; target_component 4 2; target_component 5 3] =
    Some (target_par_score_of [3; 1; 2]%N) /\
  wire_order_par_score
    [target_component 4 1; target_component 4 2; target_component 5 3] =
    Some (target_par_score_of [1; 2; 3]%N).
Proof. vm_compute. split; reflexivity. Qed.

(** The orders separate every pair that production reorders relative to the
    field order. *)
Definition orders_separate_pair (first second : nat) : Prop :=
  RPB3SemanticReducer.exact_par_score
    [target_component first 1; target_component second 2] <>
  wire_order_par_score
    [target_component first 1; target_component second 2].

Theorem every_reordered_family_pair_separates_the_orders :
  orders_separate_pair 4 5 /\
  orders_separate_pair 7 8 /\
  orders_separate_pair 7 11 /\
  orders_separate_pair 8 11.
Proof.
  unfold orders_separate_pair.
  vm_compute.
  repeat split; intro same; discriminate same.
Qed.

(** The mutant is not simply a different score: a mix that production and the
    field order arrange alike is scored the same by both. *)
Example send_and_expr_arrange_alike_under_both_orders :
  RPB3SemanticReducer.exact_par_score
    [target_component 1 1; target_component 5 2] =
  wire_order_par_score
    [target_component 1 1; target_component 5 2].
Proof. vm_compute. reflexivity. Qed.

(** The independent oracle uses the production order as well. *)
Definition source_component (tag : nat) (value : N)
    : RPB3IndependentOracleCore.source_exact_summary :=
  RPB3IndependentOracleCore.SourceExactSummary
    (RPB3IndependentOracleCore.TypedSourceMessage
      (RPB3GeneratedFieldRefinement.RawField tag
        RPB3GeneratedFieldRefinement.LengthDelimited
        RPB3GeneratedFieldRefinement.Message [] 0 None [] [])
      [])
    (RPB3IndependentOracleCore.SourceFamilyAnalysis None
      (Some (RPB3IndependentOracleCore.SourceInteger value))
      false false false)
    None.

Theorem oracle_scores_labelled_new_plus_expr_in_production_order :
  RPB3IndependentOracleRecursive.source_par_component_scores
    [source_component 4 1; source_component 5 2] =
    Some [RPB3IndependentOracleCore.SourceInteger 2%N;
          RPB3IndependentOracleCore.SourceInteger 1%N] /\
  option_map RPB3IndependentOracleCore.project_score
    (RPB3IndependentOracleRecursive.source_par_score
      [source_component 4 1; source_component 5 2]) =
  RPB3SemanticReducer.exact_par_score
    (map RPB3IndependentOracleProjection.project_source_summary
      [source_component 4 1; source_component 5 2]).
Proof. vm_compute. split; reflexivity. Qed.

Print Assumptions production_family_order_reorders_exactly_the_field_tags.
Print Assumptions production_family_order_differs_from_field_order.
Print Assumptions oracle_family_order_is_target_family_order.
Print Assumptions generated_par_message_fields_are_the_field_order_tags.
Print Assumptions new_plus_expr_children_are_the_reference_children.
Print Assumptions new_plus_expr_par_scores_differ_under_wire_and_production_order.
Print Assumptions both_references_score_new_plus_expr_in_production_order.
Print Assumptions three_way_mix_uses_the_production_order.
Print Assumptions family_occurrences_keep_their_wire_order.
Print Assumptions every_reordered_family_pair_separates_the_orders.
Print Assumptions send_and_expr_arrange_alike_under_both_orders.
Print Assumptions oracle_scores_labelled_new_plus_expr_in_production_order.
