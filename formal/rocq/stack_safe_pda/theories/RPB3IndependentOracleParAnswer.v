From Stdlib Require Import List.
From StackSafePDA Require Import EPM1 EPathMap
  RPB3AuxiliaryTypedReference RPB3RecursiveAuxiliarySourceReference
  RPB3IndependentOracleCore RPB3IndependentOracleRecursive
  RPB3IndependentOracleProjection.

Import ListNotations.

Module Recursive := RPB3RecursiveAuxiliarySourceReference.
Module Oracle := RPB3IndependentOracleRecursive.
Module Core := RPB3IndependentOracleCore.
Module Projection := RPB3IndependentOracleProjection.
Module Auxiliary := RPB3AuxiliaryTypedReference.

(** The fragment on which the oracle and the recursive target are compared:
    an unrefused Par body that carries both its semantic-default bit and an
    exact canonical score. *)
Definition source_scored_par_result (summary : Core.source_exact_summary)
    : option (bool * Core.source_exact_summary) :=
  if Core.source_summary_refused summary then None
  else match Core.source_summary_semantic_default summary,
             Core.source_summary_exact_score summary with
       | Some semantic_default, Some _ => Some (semantic_default, summary)
       | _, _ => None
       end.

Definition oracle_scored_par_reference (hash_fn : list nat -> nat)
    (body : list nat) : option (bool * Core.source_exact_summary) :=
  match Oracle.source_evaluate_message (S (length body)) hash_fn
          (Oracle.source_par_raw body) with
  | Some summary => source_scored_par_result summary
  | None => None
  end.

Definition project_par_answer (answer : bool * Core.source_exact_summary) :=
  (fst answer, Projection.project_source_summary (snd answer)).

Definition constant_hash : list nat -> nat := fun _ => 0.

Definition pathmap_par_with_values (values : list (list nat)) : list nat :=
  let snapshot := EPM1.Epm1Snapshot MapMode
    Auxiliary.default_value_actree03_arena values in
  [42] ++ EPM1.encode_frame ([130; 2] ++ EPM1.encode_frame
    ([74] ++ EPM1.encode_frame (EPM1.encode_epm1 snapshot))).

Example oracle_admits_default_pathmap_value :
  option_map fst (oracle_scored_par_reference constant_hash
    (pathmap_par_with_values [[]])) = Some false.
Proof. vm_compute. reflexivity. Qed.

Example target_admits_default_pathmap_value :
  option_map fst (Recursive.recursive_par_reference constant_hash
    (pathmap_par_with_values [[]])) = Some false.
Proof. vm_compute. reflexivity. Qed.

Example oracle_admits_scored_pathmap_value :
  option_map fst (oracle_scored_par_reference constant_hash
    (pathmap_par_with_values [[80; 1]])) = Some false.
Proof. vm_compute. reflexivity. Qed.

Example oracle_refuses_unscored_pathmap_value :
  oracle_scored_par_reference constant_hash
    (pathmap_par_with_values [[42; 2; 42; 0]]) = None.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions oracle_admits_default_pathmap_value.
Print Assumptions target_admits_default_pathmap_value.
Print Assumptions oracle_admits_scored_pathmap_value.
Print Assumptions oracle_refuses_unscored_pathmap_value.
