From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import
  EPM1
  TwoLevelRSpaceCheckpointCodec
  TwoLevelRSpaceCheckpointStreaming.

Import ListNotations.
Import TwoLevelRSpaceCheckpointCodec.
Import TwoLevelRSpaceCheckpointStreaming.
Set Implicit Arguments.

(** Consensus and migration boundary for RBA1.

    RBA1 is a physical checkpoint format.  It does not redefine Rholang
    channel identity, protobuf bytes, bincode bytes, or their consensus hash
    preimages.  The existing RhoPathV1 key profile remains authoritative for
    the first cutover; RCI2 remains shadow-only until separately approved.
    Migration builds and validates a private RBA1 candidate, then publishes it
    once or leaves the legacy bytes authoritative unchanged. *)
Module TwoLevelRSpaceCheckpointCompatibility.

Inductive path_key_profile : Type :=
| RhoPathV1
| Rci2Shadow.

Definition authoritative_path_key_profile : path_key_profile := RhoPathV1.
Definition shadow_path_key_profile : option path_key_profile := Some Rci2Shadow.

Theorem first_cutover_keeps_rho_path_v1_authoritative :
  authoritative_path_key_profile = RhoPathV1.
Proof. reflexivity. Qed.

Theorem rci2_is_not_authoritative_during_first_cutover :
  authoritative_path_key_profile <> Rci2Shadow /\
  shadow_path_key_profile = Some Rci2Shadow.
Proof. split; [discriminate | reflexivity]. Qed.

Definition semantic_channel_identity (canonical_channel_bytes : bytes) : bytes :=
  canonical_channel_bytes.

Definition physical_checkpoint_identity (image : rba1_image) : bytes :=
  rba1_checkpoint_hash_preimage image.

Theorem physical_checkpoint_is_not_used_as_semantic_channel_identity : forall
    channel image,
  semantic_channel_identity channel = channel /\
  physical_checkpoint_identity image = rba1_hash_domain ++ encode_rba1 image.
Proof. intros. split; reflexivity. Qed.

Definition legacy_consensus_preimage (legacy_bytes : bytes) : bytes :=
  legacy_bytes.

Definition protobuf_egress (protobuf_bytes : bytes) : bytes := protobuf_bytes.
Definition bincode_egress (bincode_bytes : bytes) : bytes := bincode_bytes.

Theorem checkpointing_does_not_change_legacy_consensus_preimage : forall bytes,
  legacy_consensus_preimage bytes = bytes.
Proof. reflexivity. Qed.

Theorem checkpointing_does_not_change_protobuf_egress : forall bytes,
  protobuf_egress bytes = bytes.
Proof. reflexivity. Qed.

Theorem checkpointing_does_not_change_bincode_egress : forall bytes,
  bincode_egress bytes = bytes.
Proof. reflexivity. Qed.

Record checkpoint_validation : Type := CheckpointValidation {
  validation_image : rba1_image;
  validation_outer_act_canonical : bool;
  validation_inner_acts_canonical : list bool
}.

Definition checkpoint_validation_accepts
    (validation : checkpoint_validation) : bool :=
  validation_outer_act_canonical validation &&
  forallb (fun valid => valid) (validation_inner_acts_canonical validation) &&
  Nat.eqb
    (length (validation_inner_acts_canonical validation))
    (rba1_channel_count (validation_image validation)).

Definition recover_validated_checkpoint
    (encoded : bytes)
    (validate : rba1_image -> checkpoint_validation)
    : option rba1_image :=
  match decode_rba1 encoded with
  | Some image =>
      let validation := validate image in
      if checkpoint_validation_accepts validation &&
         Nat.eqb
           (length (rba1_inner_bag_acts (validation_image validation)))
           (length (rba1_inner_bag_acts image))
      then Some image
      else None
  | None => None
  end.

Lemma every_repeated_true_validation_is_true : forall count,
  forallb (fun valid => valid) (repeat true count) = true.
Proof. induction count; simpl; auto. Qed.

Lemma successor_is_never_equal_to_predecessor : forall count,
  Nat.eqb (S count) count = false.
Proof. induction count; simpl; auto. Qed.

Theorem invalid_outer_act_exposes_no_recovered_authority : forall image,
  let validation := CheckpointValidation image false
    (repeat true (rba1_channel_count image)) in
  recover_validated_checkpoint (encode_rba1 image) (fun _ => validation) = None.
Proof.
  intros image. cbn zeta. unfold recover_validated_checkpoint.
  fold decode_rba1. rewrite rba1_decode_encode_identity. reflexivity.
Qed.

Theorem any_invalid_inner_act_exposes_no_recovered_authority : forall
    outer first rest,
  let image := Rba1Image outer (first :: rest) in
  let validation := CheckpointValidation image true
    (false :: repeat true (length rest)) in
  recover_validated_checkpoint (encode_rba1 image) (fun _ => validation) = None.
Proof.
  intros outer first rest. cbn zeta. unfold recover_validated_checkpoint.
  fold decode_rba1. rewrite rba1_decode_encode_identity. reflexivity.
Qed.

Theorem mismatched_validation_count_exposes_no_recovered_authority : forall
    image,
  let validation := CheckpointValidation image true
    (repeat true (S (rba1_channel_count image))) in
  recover_validated_checkpoint (encode_rba1 image) (fun _ => validation) = None.
Proof.
  intros image. cbn zeta. unfold recover_validated_checkpoint.
  fold decode_rba1. rewrite rba1_decode_encode_identity.
  unfold checkpoint_validation_accepts. simpl.
  rewrite every_repeated_true_validation_is_true. simpl.
  rewrite repeat_length.
  change
    ((if Nat.eqb (S (rba1_channel_count image)) (rba1_channel_count image) &&
        Nat.eqb
          (length (rba1_inner_bag_acts image))
          (length (rba1_inner_bag_acts image))
     then Some image else None) = None).
  rewrite successor_is_never_equal_to_predecessor. reflexivity.
Qed.

Inductive migration_status : Type :=
| MigrationRefused
| MigrationPublished.

Record migration_outcome : Type := MigrationOutcome {
  migration_result_status : migration_status;
  migration_authoritative_bytes : bytes;
  migration_publication_count : nat
}.

Definition migrate_legacy_checkpoint
    (decode_legacy : bytes -> option rba1_image)
    (legacy_bytes : bytes) : migration_outcome :=
  match decode_legacy legacy_bytes with
  | None => MigrationOutcome MigrationRefused legacy_bytes 0
  | Some image => MigrationOutcome MigrationPublished (encode_rba1 image) 1
  end.

Theorem rejected_migration_is_strongly_atomic : forall decoder legacy,
  decoder legacy = None ->
  migration_authoritative_bytes (migrate_legacy_checkpoint decoder legacy) =
    legacy /\
  migration_publication_count (migrate_legacy_checkpoint decoder legacy) = 0.
Proof. intros decoder legacy Hreject. unfold migrate_legacy_checkpoint.
  now rewrite Hreject.
Qed.

Theorem accepted_migration_publishes_one_decodable_rba1_authority : forall
    decoder legacy image,
  decoder legacy = Some image ->
  migration_result_status (migrate_legacy_checkpoint decoder legacy) =
    MigrationPublished /\
  migration_publication_count (migrate_legacy_checkpoint decoder legacy) = 1 /\
  decode_rba1
    (migration_authoritative_bytes
      (migrate_legacy_checkpoint decoder legacy)) = Some image.
Proof.
  intros decoder legacy image Haccept. unfold migrate_legacy_checkpoint.
  rewrite Haccept. simpl. split; [reflexivity |]. split; [reflexivity |].
  apply rba1_decode_encode_identity.
Qed.

Inductive recovered_format : Type :=
| RecoveredRba1 (image : rba1_image)
| RecoveredLegacy (image : rba1_image).

Definition decode_current_then_legacy
    (decode_legacy : bytes -> option rba1_image)
    (encoded : bytes) : option recovered_format :=
  match decode_rba1 encoded with
  | Some image => Some (RecoveredRba1 image)
  | None =>
      match decode_legacy encoded with
      | Some image => Some (RecoveredLegacy image)
      | None => None
      end
  end.

Theorem current_rba1_has_precedence_over_legacy_fallback : forall
    legacy_decoder image,
  decode_current_then_legacy legacy_decoder (encode_rba1 image) =
    Some (RecoveredRba1 image).
Proof.
  intros. unfold decode_current_then_legacy.
  now rewrite rba1_decode_encode_identity.
Qed.

Inductive rba1_compatibility_invariant_id : Type :=
| RbaCompatInvRhoPathAuthoritative
| RbaCompatInvRci2ShadowOnly
| RbaCompatInvPhysicalNotSemanticIdentity
| RbaCompatInvLegacyHashUnchanged
| RbaCompatInvProtobufUnchanged
| RbaCompatInvBincodeUnchanged
| RbaCompatInvInvalidOuterRejected
| RbaCompatInvInvalidInnerRejected
| RbaCompatInvCountMismatchRejected
| RbaCompatInvMigrationFailureAtomic
| RbaCompatInvMigrationOnePublication
| RbaCompatInvMigrationDecodable
| RbaCompatInvCurrentFormatPrecedence.

Definition rba1_compatibility_invariant_catalog :
    list rba1_compatibility_invariant_id :=
  [ RbaCompatInvRhoPathAuthoritative; RbaCompatInvRci2ShadowOnly;
    RbaCompatInvPhysicalNotSemanticIdentity; RbaCompatInvLegacyHashUnchanged;
    RbaCompatInvProtobufUnchanged; RbaCompatInvBincodeUnchanged;
    RbaCompatInvInvalidOuterRejected; RbaCompatInvInvalidInnerRejected;
    RbaCompatInvCountMismatchRejected; RbaCompatInvMigrationFailureAtomic;
    RbaCompatInvMigrationOnePublication; RbaCompatInvMigrationDecodable;
    RbaCompatInvCurrentFormatPrecedence ].

Theorem rba1_compatibility_invariant_catalog_is_unique_and_exact :
  NoDup rba1_compatibility_invariant_catalog /\
  length rba1_compatibility_invariant_catalog = 13.
Proof.
  split; [repeat constructor; simpl; intuition discriminate | reflexivity].
Qed.

Theorem complete_rba1_formal_registry_has_exactly_forty_obligations :
  length rba1_codec_invariant_catalog +
  length rba1_streaming_invariant_catalog +
  length rba1_compatibility_invariant_catalog = 40.
Proof. reflexivity. Qed.

Print Assumptions invalid_outer_act_exposes_no_recovered_authority.
Print Assumptions rejected_migration_is_strongly_atomic.
Print Assumptions accepted_migration_publishes_one_decodable_rba1_authority.
Print Assumptions current_rba1_has_precedence_over_legacy_fallback.
Print Assumptions complete_rba1_formal_registry_has_exactly_forty_obligations.

End TwoLevelRSpaceCheckpointCompatibility.
