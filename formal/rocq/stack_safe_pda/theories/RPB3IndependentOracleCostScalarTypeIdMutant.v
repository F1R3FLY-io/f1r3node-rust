From Stdlib Require Import List ZArith.
From StackSafePDA Require Import StackSafePDA
  RPB3GeneratedFieldRefinement RPB3RawRegionTree RPB3SemanticReducer
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3IndependentOracleCost.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Target := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Cost := RPB3IndependentOracleCost.

(** The old scalar admission branch ignored [raw_program_type_id].  Erasing
    only that field before the corrected admission check models that omission. *)
Definition erase_scalar_type_id (raw : Schema.raw_field)
    : Schema.raw_field :=
  Schema.RawField (Schema.raw_tag raw) (Schema.raw_wire raw)
    (Schema.raw_payload raw) (Schema.raw_body raw)
    (Schema.raw_declared_length raw) None
    (Schema.raw_wire_required raw) (Schema.raw_semantic_required raw).

Definition permissive_scalar_value_mutant
    (program : Schema.generated_program) (raw : Schema.raw_field)
    : option Core.source_scalar_value :=
  Admission.source_scalar_value_of_raw program (erase_scalar_type_id raw).

Definition permissive_scalar_leaf_mutant
    (program : Schema.generated_program) (raw : Schema.raw_field)
    : option Core.source_exact_summary :=
  option_map (fun scalar =>
    Core.SourceExactSummary (Core.TypedSourceScalar raw scalar)
      (Core.SourceFamilyAnalysis None None false false false) None)
    (permissive_scalar_value_mutant program raw).

Definition bound_program : Schema.generated_program :=
  match Schema.lookup_projected_program 11 [] [] with
  | Some program => program
  | None => Schema.empty_program
  end.

Definition good_scalar : Schema.raw_field :=
  Schema.RawField 1 Schema.Varint Schema.Scalar [2] 1 None [] [].

Definition bad_scalar : Schema.raw_field :=
  Schema.RawField 1 Schema.Varint Schema.Scalar [2] 1 (Some 11) [] [].

Definition bound_root : Schema.raw_field :=
  Schema.RawField 0 Schema.LengthDelimited Schema.Message [8; 2] 2
    (Some 11) [] [].

Definition bound_tree (scalar : Schema.raw_field)
    : @tree Schema.raw_field :=
  Node bound_root (FCons (Node scalar FNil) FNil).

Definition permissive_bad_bound_root : option Core.source_exact_summary :=
  match permissive_scalar_leaf_mutant bound_program bad_scalar with
  | Some child => Cost.source_cost_message bound_root 11 bound_program [child]
  | None => None
  end.

Theorem scalar_type_id_is_the_causal_admission_guard :
  Schema.lookup_projected_program 11 [] [] = Some bound_program /\
  Admission.source_scalar_value_of_raw bound_program good_scalar =
    Some (Core.SourceI32 1%Z) /\
  Admission.source_scalar_value_of_raw bound_program bad_scalar = None /\
  permissive_scalar_value_mutant bound_program bad_scalar =
    Some (Core.SourceI32 1%Z) /\
  Target.sem_definitely_refused
    (Target.semantic_reduce (Node good_scalar FNil)) = false /\
  Target.sem_definitely_refused
    (Target.semantic_reduce (Node bad_scalar FNil)) = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Theorem scalar_type_id_mutant_exposes_cost_tree_mismatch :
  (exists good mutant,
    Cost.source_cost_root (bound_tree good_scalar) = Some good /\
    permissive_bad_bound_root = Some mutant /\
    Core.source_summary_exact_score good =
      Core.source_summary_exact_score mutant) /\
  Cost.source_cost_root (bound_tree bad_scalar) = None /\
  Target.sem_definitely_refused
    (Target.semantic_reduce (bound_tree good_scalar)) = false /\
  Target.sem_definitely_refused
    (Target.semantic_reduce (bound_tree bad_scalar)) = true.
Proof.
  vm_compute.
  repeat split; try reflexivity.
  eexists. eexists. repeat split; reflexivity.
Qed.

Lemma erase_scalar_type_id_is_identity_on_none :
  forall raw,
    Schema.raw_program_type_id raw = None ->
    erase_scalar_type_id raw = raw.
Proof.
  intros [tag wire payload body declared type_id wire_required
    semantic_required] no_type.
  cbn in no_type. subst type_id. reflexivity.
Qed.

Lemma converted_source_scalar_has_no_type_id :
  forall bytes span descriptor raw,
    Source.raw_field_of_source bytes
      (Source.SourceScalar span descriptor) = Some raw ->
    Schema.raw_program_type_id raw = None.
Proof.
  intros bytes span descriptor raw converted.
  unfold Source.raw_field_of_source in converted.
  cbn [Source.source_span Source.source_field] in converted.
  destruct (Schema.payload_eqb (Schema.descriptor_payload descriptor)
      Schema.Scalar); [|discriminate].
  inversion converted; reflexivity.
Qed.

Theorem production_scalar_conversion_is_unchanged :
  forall bytes span descriptor raw program,
    Source.raw_field_of_source bytes
      (Source.SourceScalar span descriptor) = Some raw ->
    Admission.source_scalar_value_of_raw program raw =
      permissive_scalar_value_mutant program raw.
Proof.
  intros bytes span descriptor raw program converted.
  unfold permissive_scalar_value_mutant.
  rewrite (erase_scalar_type_id_is_identity_on_none raw
    (converted_source_scalar_has_no_type_id
      bytes span descriptor raw converted)).
  reflexivity.
Qed.

Print Assumptions scalar_type_id_is_the_causal_admission_guard.
Print Assumptions scalar_type_id_mutant_exposes_cost_tree_mismatch.
Print Assumptions production_scalar_conversion_is_unchanged.
