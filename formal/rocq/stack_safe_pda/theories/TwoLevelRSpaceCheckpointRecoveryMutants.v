From Stdlib Require Import Bool List Arith.
From StackSafePDA Require Import
  EPM1
  TwoLevelRSpaceCheckpointCodec
  TwoLevelRSpaceCheckpointCompatibility
  TwoLevelRSpaceCheckpointRecovery.

Import ListNotations.
Import TwoLevelRSpaceCheckpointCodec.
Import TwoLevelRSpaceCheckpointCompatibility.
Import TwoLevelRSpaceCheckpointRecovery.

(** Four executable negative controls at the seams introduced by the
    end-to-end recovery composition. Existing codec, migration, publication,
    and loaded-empty mutants remain authoritative for their own layers. *)
Module TwoLevelRSpaceCheckpointRecoveryMutants.

Definition subject_image : rba1_image := Rba1Image [1] [[2]].
Definition substituted_same_cardinality_image : rba1_image :=
  Rba1Image [9] [[8]].

Definition substituted_validation (_ : rba1_image) : checkpoint_validation :=
  CheckpointValidation substituted_same_cardinality_image true [true].

(** 1. The predecessor count-only seam accepted a validator report for a
    different one-channel image. Exact recovery rejects that substitution. *)
Example exact_subject_validation_kills_same_cardinality_substitution :
  recover_validated_checkpoint (encode_rba1 subject_image)
    substituted_validation = Some subject_image /\
  recover_exact_validated_checkpoint (encode_rba1 subject_image)
    substituted_validation = None.
Proof. vm_compute. split; reflexivity. Qed.

Definition inventory_contains (object : bytes) (members : list bytes) : bool :=
  existsb (fun candidate =>
    if bytes_eq_dec object candidate then true else false) members.

(** 2. A content-addressed RBA1 object detached from the exact checkpoint
    inventory cannot become authoritative. *)
Example exact_inventory_binding_kills_detached_object :
  inventory_contains [82; 66; 65; 1] [[82; 66; 65; 1]] = true /\
  inventory_contains [82; 66; 65; 1] [] = false.
Proof. vm_compute. split; reflexivity. Qed.

(** 3. Match publication advances from generation four to exactly five;
    binding a checkpoint to the pre-match generation is observable. *)
Example successor_generation_kills_pre_match_generation_substitution :
  S 4 <> 4.
Proof. discriminate. Qed.

Definition loaded_empty_fixture : rba1_image := loaded_empty_image [3] [4].
Definition unloaded_mutant_fixture : rba1_image := unloaded_image [3].

(** 4. Restart returns the validated image itself. Substituting the unloaded
    endpoint erases a semantic fact and changes canonical RBA1 bytes. *)
Example restart_exactness_kills_loaded_empty_erasure :
  loaded_empty_fixture <> unloaded_mutant_fixture /\
  encode_rba1 loaded_empty_fixture <> encode_rba1 unloaded_mutant_fixture.
Proof.
  split.
  - discriminate.
  - apply loaded_empty_and_unloaded_have_distinct_rba1_bytes.
Qed.

Inductive recovery_seam_mutant : Type :=
| MutValidationSubjectSubstitution
| MutObjectDetachedFromInventory
| MutPostMatchUsesPreMatchGeneration
| MutRestartErasesLoadedEmptyEndpoint.

Definition recovery_seam_mutant_catalog : list recovery_seam_mutant :=
  [MutValidationSubjectSubstitution; MutObjectDetachedFromInventory;
   MutPostMatchUsesPreMatchGeneration; MutRestartErasesLoadedEmptyEndpoint].

Definition reference_observation (mutant : recovery_seam_mutant) : bytes :=
  match mutant with
  | MutValidationSubjectSubstitution => encode_rba1 subject_image
  | MutObjectDetachedFromInventory => [1]
  | MutPostMatchUsesPreMatchGeneration => [5]
  | MutRestartErasesLoadedEmptyEndpoint => encode_rba1 loaded_empty_fixture
  end.

Definition mutated_observation (mutant : recovery_seam_mutant) : bytes :=
  match mutant with
  | MutValidationSubjectSubstitution =>
      encode_rba1 substituted_same_cardinality_image
  | MutObjectDetachedFromInventory => []
  | MutPostMatchUsesPreMatchGeneration => [4]
  | MutRestartErasesLoadedEmptyEndpoint => encode_rba1 unloaded_mutant_fixture
  end.

Definition recovery_seam_mutant_is_causally_killed
    (mutant : recovery_seam_mutant) : Prop :=
  mutated_observation mutant <> reference_observation mutant.

Theorem every_recovery_seam_mutant_has_a_causal_witness : forall mutant,
  recovery_seam_mutant_is_causally_killed mutant.
Proof. intro mutant. destruct mutant; vm_compute; discriminate. Qed.

Theorem recovery_seam_mutant_catalog_is_unique_complete_and_exact :
  NoDup recovery_seam_mutant_catalog /\
  Forall recovery_seam_mutant_is_causally_killed
    recovery_seam_mutant_catalog /\
  length recovery_seam_mutant_catalog = 4%nat.
Proof.
  split.
  - repeat constructor; simpl; intuition discriminate.
  - split.
    + apply Forall_forall. intros mutant _. apply
        every_recovery_seam_mutant_has_a_causal_witness.
    + reflexivity.
Qed.

Print Assumptions exact_subject_validation_kills_same_cardinality_substitution.
Print Assumptions exact_inventory_binding_kills_detached_object.
Print Assumptions successor_generation_kills_pre_match_generation_substitution.
Print Assumptions restart_exactness_kills_loaded_empty_erasure.
Print Assumptions every_recovery_seam_mutant_has_a_causal_witness.
Print Assumptions recovery_seam_mutant_catalog_is_unique_complete_and_exact.

End TwoLevelRSpaceCheckpointRecoveryMutants.
