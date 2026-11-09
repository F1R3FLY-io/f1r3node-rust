From Stdlib Require Import List NArith.
From StackSafePDA Require Import RPB3IndependentOracleCore
  RPB3IndependentOracleRecursive RPB3SemanticReducer
  RPB3RecursiveAuxiliarySourceReference
  RPB3GeneratedFieldRefinement.

Import ListNotations.

Module Core := RPB3IndependentOracleCore.
Module Oracle := RPB3IndependentOracleRecursive.
Module Target := RPB3SemanticReducer.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.
Module Schema := RPB3GeneratedFieldRefinement.

(** A generated Par tag 5 contains Expr tag 36 or 37.  The inner message
    carries no score; its Expr parent constructs the score from decoded
    numeric fields.  Absent inner fields use production's empty-byte and
    zero-scale defaults. *)
Definition bigrat_par_score (numerator denominator : list nat)
    : Target.canonical_score :=
  Target.score_constructor 999
    [Target.score_constructor 16
      [Target.ScoreBytes numerator; Target.ScoreBytes denominator];
     Target.ScoreInteger 0].

Definition fixed_point_par_score (unscaled : list nat) (scale : N)
    : Target.canonical_score :=
  Target.score_constructor 999
    [Target.score_constructor 17
      [Target.ScoreBytes unscaled;
       Target.ScoreNode
         (Target.ScoreCons (Target.ScoreInteger scale) Target.ScoreNil)];
     Target.ScoreInteger 0].

Definition public_exact_score (body : list nat)
    (expected : Target.canonical_score) : Prop :=
  exists source_summary target_summary,
    Oracle.source_par_reference (fun _ => 0) body =
      Some (false, source_summary) /\
    Recursive.recursive_par_reference (fun _ => 0) body =
      Some (false, target_summary) /\
    option_map Core.project_score
      (Core.source_summary_exact_score source_summary) = Some expected /\
    Target.sem_score_evidence target_summary = Target.ExactScore expected.

Definition public_rejects (body : list nat) : Prop :=
  Oracle.source_par_reference (fun _ => 0) body = None /\
  Recursive.recursive_par_reference (fun _ => 0) body = None.

Example bigrat_absent_fields_score :
  public_exact_score [42; 3; 162; 2; 0] (bigrat_par_score [] []).
Proof. vm_compute. repeat eexists; repeat split; reflexivity. Qed.

Example bigrat_numerator_only_score :
  public_exact_score [42; 6; 162; 2; 3; 10; 1; 1]
    (bigrat_par_score [1] []).
Proof. vm_compute. repeat eexists; repeat split; reflexivity. Qed.

Example bigrat_denominator_only_score :
  public_exact_score [42; 6; 162; 2; 3; 18; 1; 2]
    (bigrat_par_score [] [2]).
Proof. vm_compute. repeat eexists; repeat split; reflexivity. Qed.

Example bigrat_two_fields_score :
  public_exact_score [42; 9; 162; 2; 6; 10; 1; 1; 18; 1; 2]
    (bigrat_par_score [1] [2]).
Proof. vm_compute. repeat eexists; repeat split; reflexivity. Qed.

Example fixed_point_absent_fields_score :
  public_exact_score [42; 3; 170; 2; 0]
    (fixed_point_par_score [] 0).
Proof. vm_compute. repeat eexists; repeat split; reflexivity. Qed.

Example fixed_point_unscaled_only_score :
  public_exact_score [42; 6; 170; 2; 3; 10; 1; 1]
    (fixed_point_par_score [1] 0).
Proof. vm_compute. repeat eexists; repeat split; reflexivity. Qed.

Example fixed_point_scale_only_score :
  public_exact_score [42; 5; 170; 2; 2; 16; 2]
    (fixed_point_par_score [] 2).
Proof. vm_compute. repeat eexists; repeat split; reflexivity. Qed.

Example fixed_point_two_fields_score :
  public_exact_score [42; 8; 170; 2; 5; 10; 1; 1; 16; 2]
    (fixed_point_par_score [1] 2).
Proof. vm_compute. repeat eexists; repeat split; reflexivity. Qed.

(** The target's nested structural decoder alone accepts these shapes;
    the generated scalar rules must reject explicit defaults.  Production's
    every_descriptor_scalar_field_rejects_explicit_default_presence covers
    the same descriptor-wide boundary. *)
Example bigrat_explicit_empty_numerator_rejected :
  public_rejects [42; 5; 162; 2; 2; 10; 0].
Proof. vm_compute. split; reflexivity. Qed.

Example bigrat_explicit_empty_denominator_rejected :
  public_rejects [42; 5; 162; 2; 2; 18; 0].
Proof. vm_compute. split; reflexivity. Qed.

Example fixed_point_explicit_empty_unscaled_rejected :
  public_rejects [42; 5; 170; 2; 2; 10; 0].
Proof. vm_compute. split; reflexivity. Qed.

Example fixed_point_explicit_zero_scale_rejected :
  public_rejects [42; 5; 170; 2; 2; 16; 0].
Proof. vm_compute. split; reflexivity. Qed.

(** These synthetic unrefused child summaries isolate the Expr parent's
    generated-scalar guard.  The structural nested decoder accepts their
    field shapes, but explicit scalar defaults and invalid uint32 encodings
    cannot produce a canonical parent score. *)
Definition bigrat_guard_probe (body : list nat) : Target.semantic_summary :=
  Target.SemanticSummary 36 Schema.LengthDelimited Schema.Message 25 body
    false (Some true) Target.ScoreNotDerived false false.

Definition fixed_point_guard_probe (body : list nat)
    : Target.semantic_summary :=
  Target.SemanticSummary 37 Schema.LengthDelimited Schema.Message 26 body
    false (Some true) Target.ScoreNotDerived false false.

Example bigrat_present_empty_bytes_do_not_score :
  Target.exact_bigrat_parent_score (bigrat_guard_probe [10; 0]) = None.
Proof. vm_compute. reflexivity. Qed.

Example fixed_point_present_zero_scale_does_not_score :
  Target.exact_fixed_point_parent_score
    (fixed_point_guard_probe [16; 0]) = None.
Proof. vm_compute. reflexivity. Qed.

Example fixed_point_overflow_scale_does_not_score :
  Target.exact_fixed_point_parent_score
    (fixed_point_guard_probe [16; 128; 128; 128; 128; 16]) = None.
Proof. vm_compute. reflexivity. Qed.

Example fixed_point_noncanonical_scale_does_not_score :
  Target.exact_fixed_point_parent_score
    (fixed_point_guard_probe [16; 128; 0]) = None.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions bigrat_absent_fields_score.
Print Assumptions bigrat_numerator_only_score.
Print Assumptions bigrat_denominator_only_score.
Print Assumptions bigrat_two_fields_score.
Print Assumptions fixed_point_absent_fields_score.
Print Assumptions fixed_point_unscaled_only_score.
Print Assumptions fixed_point_scale_only_score.
Print Assumptions fixed_point_two_fields_score.
Print Assumptions bigrat_explicit_empty_numerator_rejected.
Print Assumptions bigrat_explicit_empty_denominator_rejected.
Print Assumptions fixed_point_explicit_empty_unscaled_rejected.
Print Assumptions fixed_point_explicit_zero_scale_rejected.
Print Assumptions bigrat_present_empty_bytes_do_not_score.
Print Assumptions fixed_point_present_zero_scale_does_not_score.
Print Assumptions fixed_point_overflow_scale_does_not_score.
Print Assumptions fixed_point_noncanonical_scale_does_not_score.
