From Stdlib Require Import List NArith.
From StackSafePDA Require Import RPB3IndependentOracleCore
  RPB3IndependentOracleRecursive RPB3SemanticReducer
  RPB3RecursiveAuxiliarySourceReference.

Import ListNotations.

Module Core := RPB3IndependentOracleCore.
Module Oracle := RPB3IndependentOracleRecursive.
Module Target := RPB3SemanticReducer.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.

(** Generated Par tag 5 wraps an Expr tag 34 fixed64 GDouble.  The eight
    little-endian payload bytes encode bit pattern one.  Production's
    expr_ground_structural_and_nonsemantic_finish_source_property and the
    sorter canonical golden both cover this accepted score constructor. *)
Definition gdouble_par_body : list nat :=
  [42; 10; 145; 2; 1; 0; 0; 0; 0; 0; 0; 0].

Theorem generated_gdouble_par_has_matching_exact_scores :
  exists source_summary target_summary,
    Oracle.source_par_reference (fun _ => 0) gdouble_par_body =
      Some (false, source_summary) /\
    Recursive.recursive_par_reference (fun _ => 0) gdouble_par_body =
      Some (false, target_summary) /\
    Core.source_summary_exact_score source_summary =
      Some (Core.source_score_constructor 999%N
        [Core.source_score_constructor 14%N [Core.SourceInteger 1%N];
         Core.SourceInteger 0%N]) /\
    Target.sem_score_evidence target_summary =
      Target.ExactScore (Target.score_constructor 999%N
        [Target.score_constructor 14%N [Target.ScoreInteger 1%N];
         Target.ScoreInteger 0%N]).
Proof.
  vm_compute.
  eexists. eexists.
  repeat split; reflexivity.
Qed.

Print Assumptions generated_gdouble_par_has_matching_exact_scores.
