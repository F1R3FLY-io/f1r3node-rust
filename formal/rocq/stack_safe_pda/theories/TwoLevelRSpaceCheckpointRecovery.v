From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import
  EPM1
  GenerationCheckpointPublication
  TwoLevelRSpaceCheckpointCodec
  TwoLevelRSpaceCheckpointStreaming
  TwoLevelRSpaceCheckpointCompatibility
  TwoLevelRSpaceAuthority
  TwoLevelRSpaceOperations
  CandidateCatalogAuthorityIsolation
  TwoLevelRSpaceMatchTransaction.

Import ListNotations.
Import TwoLevelRSpaceCheckpointCodec.
Import TwoLevelRSpaceCheckpointStreaming.
Import TwoLevelRSpaceCheckpointCompatibility.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceOperations.
Import CandidateCatalogAuthorityIsolation.
Import TwoLevelRSpaceMatchTransaction.
Set Implicit Arguments.

(** Crash-safe generation selection for the RBA1 two-level authority.

    The durable slot classifier and affine publication protocol remain the
    authorities from [GenerationCheckpointPublication]. This theory supplies
    the RBA1-specific restart-grade predicate: exact predecessor/successor WAL
    generation binding, exact receipt binding, exact RBA1 framing, and complete
    outer/inner ACT validation. A torn slot always selects the predecessor. A
    complete successor is selected only after all of those checks succeed.
    No rejected candidate exposes a partially decoded RSpace authority. *)
Module TwoLevelRSpaceCheckpointRecovery.

Definition bytes_eq_dec (left right : bytes) : {left = right} + {left <> right} :=
  list_eq_dec Nat.eq_dec left right.

Definition rba1_image_eq_dec (left right : rba1_image) : {left = right} + {left <> right}.
Proof.
  destruct (bytes_eq_dec (encode_rba1 left) (encode_rba1 right)) as [Heq|Hneq].
  - left. now apply rba1_encoding_is_injective.
  - right. intro Heq. subst. contradiction.
Defined.

Definition canonical_checkpoint_validation (image : rba1_image)
    : checkpoint_validation :=
  CheckpointValidation image true (repeat true (rba1_channel_count image)).

Theorem canonical_checkpoint_validation_recovers_exact_image : forall image,
  recover_validated_checkpoint (encode_rba1 image)
    canonical_checkpoint_validation = Some image.
Proof.
  intros image. unfold recover_validated_checkpoint.
  rewrite rba1_decode_encode_identity.
  unfold canonical_checkpoint_validation, checkpoint_validation_accepts.
  simpl. rewrite every_repeated_true_validation_is_true, repeat_length.
  rewrite Nat.eqb_refl. simpl. rewrite Nat.eqb_refl. reflexivity.
Qed.

(** The compatibility-layer validator predates generation publication and
    returns the decoded image. At this composition seam, recovery additionally
    requires that validation addressed that exact image and that the durable
    bytes are its unique canonical RBA1 encoding. Equal frame counts alone are
    deliberately insufficient. *)
Definition exact_validation_evidence
    (encoded : bytes)
    (validate : rba1_image -> checkpoint_validation)
    (image : rba1_image) : Prop :=
  recover_validated_checkpoint encoded validate = Some image /\
  validation_image (validate image) = image /\
  encoded = encode_rba1 image.

Definition recover_exact_validated_checkpoint
    (encoded : bytes)
    (validate : rba1_image -> checkpoint_validation) : option rba1_image :=
  match recover_validated_checkpoint encoded validate with
  | Some image =>
      if rba1_image_eq_dec (validation_image (validate image)) image then
        if bytes_eq_dec encoded (encode_rba1 image) then Some image else None
      else None
  | None => None
  end.

Theorem exact_recovery_is_sound : forall encoded validate image,
  recover_exact_validated_checkpoint encoded validate = Some image ->
  exact_validation_evidence encoded validate image.
Proof.
  intros encoded validate image Hrecover.
  unfold recover_exact_validated_checkpoint in Hrecover.
  destruct (recover_validated_checkpoint encoded validate) as [decoded|]
    eqn:Hvalidated; try discriminate.
  destruct (rba1_image_eq_dec
    (validation_image (validate decoded)) decoded) as [Hsubject|Hsubject];
    try discriminate.
  destruct (bytes_eq_dec encoded (encode_rba1 decoded)) as [Hbytes|Hbytes];
    try discriminate.
  inversion Hrecover; subst decoded. repeat split; assumption.
Qed.

Theorem exact_validation_evidence_is_complete : forall encoded validate image,
  exact_validation_evidence encoded validate image ->
  recover_exact_validated_checkpoint encoded validate = Some image.
Proof.
  intros encoded validate image [Hvalidated [Hsubject Hbytes]].
  unfold recover_exact_validated_checkpoint. rewrite Hvalidated.
  destruct (rba1_image_eq_dec
    (validation_image (validate image)) image); [|contradiction].
  destruct (bytes_eq_dec encoded (encode_rba1 image)); [reflexivity|contradiction].
Qed.

Theorem canonical_exact_recovery_recovers_exact_image : forall image,
  recover_exact_validated_checkpoint (encode_rba1 image)
    canonical_checkpoint_validation = Some image.
Proof.
  intro image. apply exact_validation_evidence_is_complete.
  split; [apply canonical_checkpoint_validation_recovers_exact_image|].
  split; reflexivity.
Qed.

Definition checkpoint_binding_eqb
    (left right : checkpoint_binding_state) : bool :=
  Nat.eqb (binding_sequence left) (binding_sequence right) &&
  Nat.eqb (binding_store left) (binding_store right) &&
  Nat.eqb (binding_previous_terminal left) (binding_previous_terminal right) &&
  Nat.eqb (binding_prepare_pack left) (binding_prepare_pack right) &&
  Nat.eqb (binding_prepare_offset left) (binding_prepare_offset right) &&
  Nat.eqb (binding_prepare_length left) (binding_prepare_length right) &&
  Nat.eqb (binding_prepare_digest left) (binding_prepare_digest right) &&
  Nat.eqb (binding_descriptor left) (binding_descriptor right) &&
  Nat.eqb (binding_index_first_live left) (binding_index_first_live right) &&
  Nat.eqb (binding_index_generation left) (binding_index_generation right) &&
  Nat.eqb (binding_index_tail left) (binding_index_tail right) &&
  Nat.eqb (binding_index_count left) (binding_index_count right) &&
  Nat.eqb (binding_index_digest left) (binding_index_digest right) &&
  Nat.eqb (binding_index_retention left) (binding_index_retention right) &&
  Nat.eqb (binding_index_current_bytes left) (binding_index_current_bytes right) &&
  Nat.eqb (binding_index_retained_bytes left) (binding_index_retained_bytes right).

Theorem checkpoint_binding_eqb_reflects_exact_receipt : forall left right,
  checkpoint_binding_eqb left right = true <->
  receipt_binding_is_exact left right.
Proof.
  intros [ls lst lp lpp lpo lpl lpd ld lif lig lit lic lid lir licb lirb]
    [rs rst rp rpp rpo rpl rpd rd rif rig rit ric rid rir ricb rirb].
  unfold checkpoint_binding_eqb, receipt_binding_is_exact. simpl.
  split.
  - intro H. repeat rewrite andb_true_iff in H.
    repeat rewrite Nat.eqb_eq in H.
    repeat match goal with
      | joined : _ /\ _ |- _ => destruct joined
      end.
    repeat split; congruence.
  - intro H. repeat rewrite andb_true_iff.
    repeat rewrite Nat.eqb_eq.
    repeat match goal with
      | joined : _ /\ _ |- _ => destruct joined
      end.
    repeat split; congruence.
Qed.

Record rba1_generation_candidate : Type := Rba1GenerationCandidate {
  candidate_slot_image : durable_slot_image;
  candidate_old_authority_bytes : bytes;
  candidate_new_authority_bytes : bytes;
  candidate_wal_base_index_generation : nat;
  candidate_checkpoint_index_generation : nat;
  candidate_expected_receipt : checkpoint_binding_state;
  candidate_observed_receipt : checkpoint_binding_state
}.

Definition restart_grade_rba1
    (candidate : rba1_generation_candidate)
    (validate : rba1_image -> checkpoint_validation) : bool :=
  Nat.eqb (candidate_checkpoint_index_generation candidate)
      (S (candidate_wal_base_index_generation candidate)) &&
  Nat.eqb
      (binding_index_generation (candidate_expected_receipt candidate))
      (candidate_checkpoint_index_generation candidate) &&
  checkpoint_binding_eqb
      (candidate_expected_receipt candidate)
      (candidate_observed_receipt candidate) &&
  match recover_exact_validated_checkpoint
      (candidate_new_authority_bytes candidate) validate with
  | Some _ => true
  | None => false
  end.

Definition rba1_recovery_selection
    (candidate : rba1_generation_candidate)
    (validate : rba1_image -> checkpoint_validation) : recovery_selection :=
  select_after_crash (candidate_slot_image candidate)
    (restart_grade_rba1 candidate validate).

Inductive recovered_rspace_authority : Type :=
| RecoveredOldAuthority (bytes : bytes)
| RecoveredNewAuthority (image : rba1_image)
| RejectedDamagedNewAuthority.

Definition recover_rba1_generation
    (candidate : rba1_generation_candidate)
    (validate : rba1_image -> checkpoint_validation)
    : recovered_rspace_authority :=
  match rba1_recovery_selection candidate validate with
  | SelectOld =>
      RecoveredOldAuthority (candidate_old_authority_bytes candidate)
  | SelectNew =>
      match recover_exact_validated_checkpoint
          (candidate_new_authority_bytes candidate) validate with
      | Some image => RecoveredNewAuthority image
      | None => RejectedDamagedNewAuthority
      end
  | RejectDamagedNew => RejectedDamagedNewAuthority
  end.

Theorem old_or_torn_slot_recovers_exact_predecessor : forall candidate validate,
  candidate_slot_image candidate = DurableOldImage \/
  candidate_slot_image candidate = DurableTornImage ->
  recover_rba1_generation candidate validate =
    RecoveredOldAuthority (candidate_old_authority_bytes candidate).
Proof.
  intros candidate validate [Hold | Htorn];
    unfold recover_rba1_generation, rba1_recovery_selection;
    rewrite Hold || rewrite Htorn; reflexivity.
Qed.

Theorem complete_restart_grade_candidate_recovers_exact_successor : forall
    candidate validate image,
  candidate_slot_image candidate = DurableCompleteNewImage ->
  restart_grade_rba1 candidate validate = true ->
  recover_exact_validated_checkpoint
      (candidate_new_authority_bytes candidate) validate = Some image ->
  recover_rba1_generation candidate validate = RecoveredNewAuthority image.
Proof.
  intros candidate validate image Hslot Hgrade Hrecover.
  unfold recover_rba1_generation, rba1_recovery_selection.
  rewrite Hslot, Hgrade, Hrecover. reflexivity.
Qed.

Theorem complete_non_restart_grade_candidate_exposes_no_new_authority : forall
    candidate validate,
  candidate_slot_image candidate = DurableCompleteNewImage ->
  restart_grade_rba1 candidate validate = false ->
  recover_rba1_generation candidate validate = RejectedDamagedNewAuthority.
Proof.
  intros candidate validate Hslot Hgrade.
  unfold recover_rba1_generation, rba1_recovery_selection.
  now rewrite Hslot, Hgrade.
Qed.

Theorem wrong_successor_generation_is_rejected : forall candidate validate,
  candidate_slot_image candidate = DurableCompleteNewImage ->
  candidate_checkpoint_index_generation candidate <>
    S (candidate_wal_base_index_generation candidate) ->
  recover_rba1_generation candidate validate = RejectedDamagedNewAuthority.
Proof.
  intros candidate validate Hslot Hwrong.
  apply complete_non_restart_grade_candidate_exposes_no_new_authority; [exact Hslot|].
  unfold restart_grade_rba1. apply Nat.eqb_neq in Hwrong. now rewrite Hwrong.
Qed.

Theorem receipt_mismatch_is_rejected : forall candidate validate,
  candidate_slot_image candidate = DurableCompleteNewImage ->
  ~ receipt_binding_is_exact
      (candidate_expected_receipt candidate)
      (candidate_observed_receipt candidate) ->
  recover_rba1_generation candidate validate = RejectedDamagedNewAuthority.
Proof.
  intros candidate validate Hslot Hwrong.
  apply complete_non_restart_grade_candidate_exposes_no_new_authority; [exact Hslot|].
  unfold restart_grade_rba1.
  destruct (Nat.eqb (candidate_checkpoint_index_generation candidate)
    (S (candidate_wal_base_index_generation candidate))); [|reflexivity].
  destruct (Nat.eqb
      (binding_index_generation (candidate_expected_receipt candidate))
      (candidate_checkpoint_index_generation candidate)); [|reflexivity].
  destruct (checkpoint_binding_eqb
      (candidate_expected_receipt candidate)
      (candidate_observed_receipt candidate)) eqn:Heq; [|reflexivity].
  apply checkpoint_binding_eqb_reflects_exact_receipt in Heq. contradiction.
Qed.

Theorem invalid_rba1_is_rejected_without_partial_authority : forall
    candidate validate,
  candidate_slot_image candidate = DurableCompleteNewImage ->
  recover_exact_validated_checkpoint
      (candidate_new_authority_bytes candidate) validate = None ->
  recover_rba1_generation candidate validate = RejectedDamagedNewAuthority.
Proof.
  intros candidate validate Hslot Hinvalid.
  apply complete_non_restart_grade_candidate_exposes_no_new_authority; [exact Hslot|].
  unfold restart_grade_rba1. rewrite Hinvalid.
  destruct (Nat.eqb (candidate_checkpoint_index_generation candidate)
    (S (candidate_wal_base_index_generation candidate)));
  destruct (Nat.eqb
    (binding_index_generation (candidate_expected_receipt candidate))
    (candidate_checkpoint_index_generation candidate));
  destruct (checkpoint_binding_eqb
    (candidate_expected_receipt candidate)
    (candidate_observed_receipt candidate)); reflexivity.
Qed.

Theorem selected_new_implies_complete_valid_exactly_bound_successor : forall
    candidate validate image,
  recover_rba1_generation candidate validate = RecoveredNewAuthority image ->
  candidate_slot_image candidate = DurableCompleteNewImage /\
  candidate_checkpoint_index_generation candidate =
    S (candidate_wal_base_index_generation candidate) /\
  binding_index_generation (candidate_expected_receipt candidate) =
    candidate_checkpoint_index_generation candidate /\
  receipt_binding_is_exact
    (candidate_expected_receipt candidate)
    (candidate_observed_receipt candidate) /\
  exact_validation_evidence
    (candidate_new_authority_bytes candidate) validate image.
Proof.
  intros candidate validate image Hrecovered.
  unfold recover_rba1_generation, rba1_recovery_selection,
    restart_grade_rba1 in Hrecovered.
  destruct (candidate_slot_image candidate) eqn:Hslot; try discriminate.
  destruct (Nat.eqb (candidate_checkpoint_index_generation candidate)
      (S (candidate_wal_base_index_generation candidate))) eqn:Hgeneration;
    try discriminate.
  destruct (Nat.eqb
      (binding_index_generation (candidate_expected_receipt candidate))
      (candidate_checkpoint_index_generation candidate)) eqn:Hreceipt_generation;
    try discriminate.
  destruct (checkpoint_binding_eqb
      (candidate_expected_receipt candidate)
      (candidate_observed_receipt candidate)) eqn:Hreceipt;
    try discriminate.
  destruct (recover_exact_validated_checkpoint
      (candidate_new_authority_bytes candidate) validate) eqn:Hvalidation;
    try discriminate.
  inversion Hrecovered; subst.
  split; [reflexivity|].
  split; [now apply Nat.eqb_eq|].
  split; [now apply Nat.eqb_eq|].
  split; [now apply checkpoint_binding_eqb_reflects_exact_receipt|].
  now apply exact_recovery_is_sound.
Qed.

Theorem loaded_empty_rba1_survives_restart_exactly : forall
    candidate outer_act empty_bag_act,
  candidate_slot_image candidate = DurableCompleteNewImage ->
  candidate_new_authority_bytes candidate =
    encode_rba1 (loaded_empty_image outer_act empty_bag_act) ->
  candidate_checkpoint_index_generation candidate =
    S (candidate_wal_base_index_generation candidate) ->
  binding_index_generation (candidate_expected_receipt candidate) =
    candidate_checkpoint_index_generation candidate ->
  receipt_binding_is_exact
    (candidate_expected_receipt candidate)
    (candidate_observed_receipt candidate) ->
  recover_rba1_generation candidate canonical_checkpoint_validation =
    RecoveredNewAuthority (loaded_empty_image outer_act empty_bag_act).
Proof.
  intros candidate outer_act empty_bag_act Hslot Hbytes Hgeneration
    Hreceipt_generation Hreceipt.
  apply complete_restart_grade_candidate_recovers_exact_successor with
    (image := loaded_empty_image outer_act empty_bag_act).
  - exact Hslot.
  - unfold restart_grade_rba1. apply Nat.eqb_eq in Hgeneration.
    rewrite Hgeneration.
    apply checkpoint_binding_eqb_reflects_exact_receipt in Hreceipt.
    apply Nat.eqb_eq in Hreceipt_generation.
    rewrite Hreceipt_generation, Hreceipt, Hbytes,
      canonical_exact_recovery_recovers_exact_image. reflexivity.
  - rewrite Hbytes. apply canonical_exact_recovery_recovers_exact_image.
Qed.

Theorem accepted_migration_can_become_restart_authority_only_after_validation :
  forall decoder legacy image candidate,
  decoder legacy = Some image ->
  candidate_slot_image candidate = DurableCompleteNewImage ->
  candidate_new_authority_bytes candidate =
    migration_authoritative_bytes (migrate_legacy_checkpoint decoder legacy) ->
  candidate_checkpoint_index_generation candidate =
    S (candidate_wal_base_index_generation candidate) ->
  binding_index_generation (candidate_expected_receipt candidate) =
    candidate_checkpoint_index_generation candidate ->
  receipt_binding_is_exact
    (candidate_expected_receipt candidate)
    (candidate_observed_receipt candidate) ->
  recover_rba1_generation candidate canonical_checkpoint_validation =
    RecoveredNewAuthority image.
Proof.
  intros decoder legacy image candidate Hmigration Hslot Hbytes Hgeneration
    Hreceipt_generation Hreceipt.
  unfold migrate_legacy_checkpoint in Hbytes. rewrite Hmigration in Hbytes.
  cbn in Hbytes.
  assert (Hexact : recover_exact_validated_checkpoint
      (candidate_new_authority_bytes candidate)
      canonical_checkpoint_validation = Some image).
  { rewrite Hbytes. apply canonical_exact_recovery_recovers_exact_image. }
  apply complete_restart_grade_candidate_recovers_exact_successor with
    (image := image); try assumption.
  - unfold restart_grade_rba1. apply Nat.eqb_eq in Hgeneration.
    rewrite Hgeneration.
    apply checkpoint_binding_eqb_reflects_exact_receipt in Hreceipt.
    apply Nat.eqb_eq in Hreceipt_generation.
    rewrite Hreceipt_generation, Hreceipt, Hexact. reflexivity.
Qed.

(** A generation artifact binds the logical two-level store, its exact RBA1
    image, the one content-addressed object admitted to the checkpoint
    inventory, and the WAL receipt. Physical index generation and semantic
    RSpace generation are independent authorities: maintenance may advance
    the former while retaining the latter. *)
Record rba1_generation_artifact
    (Object Catalog ExternalSnapshots : Type) : Type := Rba1GenerationArtifact {
  artifact_image : rba1_image;
  artifact_bytes : bytes;
  artifact_store : @transactional_store_state Catalog ExternalSnapshots;
  artifact_state_generation : nat;
  artifact_index_generation : nat;
  artifact_descriptor_state_generation : nat;
  artifact_object : Object;
  artifact_expected_members : list Object;
  artifact_expected_binding : checkpoint_binding_state;
  artifact_observed_binding : checkpoint_binding_state
}.

Arguments artifact_image {Object Catalog ExternalSnapshots} _.
Arguments artifact_bytes {Object Catalog ExternalSnapshots} _.
Arguments artifact_store {Object Catalog ExternalSnapshots} _.
Arguments artifact_state_generation {Object Catalog ExternalSnapshots} _.
Arguments artifact_index_generation {Object Catalog ExternalSnapshots} _.
Arguments artifact_descriptor_state_generation
  {Object Catalog ExternalSnapshots} _.
Arguments artifact_object {Object Catalog ExternalSnapshots} _.
Arguments artifact_expected_members {Object Catalog ExternalSnapshots} _.
Arguments artifact_expected_binding {Object Catalog ExternalSnapshots} _.
Arguments artifact_observed_binding {Object Catalog ExternalSnapshots} _.

Definition compatibility_admitted {Object Catalog ExternalSnapshots}
    (artifact : rba1_generation_artifact Object Catalog ExternalSnapshots)
    (validate : rba1_image -> checkpoint_validation) : Prop :=
  exact_validation_evidence
    (artifact_bytes artifact) validate (artifact_image artifact).

Definition rba1_object_bound {Object Catalog ExternalSnapshots}
    (object_key canonical_bytes : Object -> nat)
    (hash_bytes : bytes -> nat)
    (artifact : rba1_generation_artifact Object Catalog ExternalSnapshots)
    : Prop :=
  In (artifact_object artifact) (artifact_expected_members artifact) /\
  object_key (artifact_object artifact) =
    hash_bytes (rba1_checkpoint_hash_preimage (artifact_image artifact)) /\
  canonical_bytes (artifact_object artifact) =
    length (artifact_bytes artifact) /\
  artifact_bytes artifact = encode_rba1 (artifact_image artifact).

Definition rba1_object_constructed_and_inserted
    {Object Catalog ExternalSnapshots}
    (object_key canonical_bytes : Object -> nat)
    (make_object : nat -> nat -> Object)
    (hash_bytes : bytes -> nat)
    (retained : list Object)
    (artifact : rba1_generation_artifact Object Catalog ExternalSnapshots)
    : Prop :=
  artifact_bytes artifact = encode_rba1 (artifact_image artifact) /\
  artifact_object artifact = make_object
    (hash_bytes (rba1_checkpoint_hash_preimage (artifact_image artifact)))
    (length (artifact_bytes artifact)) /\
  (forall key encoded_length,
    object_key (make_object key encoded_length) = key) /\
  (forall key encoded_length,
    canonical_bytes (make_object key encoded_length) = encoded_length) /\
  insert_unique_by_key object_key (artifact_object artifact) retained =
    Some (artifact_expected_members artifact).

Definition wal_checkpoint_bound {Object Catalog ExternalSnapshots}
    (artifact : rba1_generation_artifact Object Catalog ExternalSnapshots)
    : Prop :=
  receipt_binding_is_exact
    (artifact_expected_binding artifact)
    (artifact_observed_binding artifact) /\
  binding_index_generation (artifact_expected_binding artifact) =
    artifact_index_generation artifact /\
  artifact_descriptor_state_generation artifact =
    artifact_state_generation artifact /\
  artifact_state_generation artifact =
    store_publication_generation (artifact_store artifact).

Definition rba1_refines_store {Object Catalog ExternalSnapshots}
    (encode_outer_act : TwoLevelAuthority -> bytes)
    (encode_bag_act : ChannelBag -> bytes)
    (artifact : rba1_generation_artifact Object Catalog ExternalSnapshots)
    : Prop :=
  artifact_image artifact = Rba1Image
    (encode_outer_act (store_authority (artifact_store artifact)))
    (encode_inner_bags_in_outer_order encode_bag_act
      (store_authority (artifact_store artifact))).

Theorem constructed_rba1_object_is_exactly_inventory_bound : forall
    (Object Catalog ExternalSnapshots : Type)
    (object_key canonical_bytes : Object -> nat)
    (make_object : nat -> nat -> Object)
    (hash_bytes : bytes -> nat)
    retained
    (artifact : rba1_generation_artifact Object Catalog ExternalSnapshots),
  rba1_object_constructed_and_inserted object_key canonical_bytes make_object
    hash_bytes retained artifact ->
  rba1_object_bound object_key canonical_bytes hash_bytes artifact.
Proof.
  intros Object Catalog ExternalSnapshots object_key canonical_bytes
    make_object hash_bytes retained artifact
    [Hbytes [Hobject [Hkey [Hlength Hinsert]]]].
  unfold rba1_object_bound. repeat split.
  - pose proof (@successful_merge_membership_is_exact Object object_key
      (artifact_object artifact) retained (artifact_expected_members artifact)
      (artifact_object artifact) Hinsert) as Hmembership.
    apply Hmembership. now left.
  - rewrite Hobject. apply Hkey.
  - rewrite Hobject. apply Hlength.
  - exact Hbytes.
Qed.

Theorem rba1_store_refinement_uses_exact_outer_and_inner_cursor_order : forall
    (Object Catalog ExternalSnapshots : Type)
    encode_outer_act encode_bag_act
    (artifact : rba1_generation_artifact Object Catalog ExternalSnapshots),
  rba1_refines_store encode_outer_act encode_bag_act artifact ->
  rba1_outer_act (artifact_image artifact) =
    encode_outer_act (store_authority (artifact_store artifact)) /\
  rba1_inner_bag_acts (artifact_image artifact) =
    map (fun entry => encode_bag_act (snd entry))
      (store_authority (artifact_store artifact)).
Proof.
  intros Object Catalog ExternalSnapshots encode_outer_act encode_bag_act
    artifact Hrefines.
  unfold rba1_refines_store, encode_inner_bags_in_outer_order in Hrefines.
  rewrite Hrefines. split; reflexivity.
Qed.

Definition immediate_match_state_commit {Object}
    (outcome : MatchOutcome)
    (artifact : rba1_generation_artifact Object MatchCatalog
      (list isolated_candidate_pool)) : Prop :=
  artifact_store artifact = match_outcome_state outcome.

Theorem accepted_match_binds_artifact_to_exact_successor_state_generation : forall
    before request prepared outcome Object
    (artifact : rba1_generation_artifact Object MatchCatalog
      (list isolated_candidate_pool)) validate,
  @match_transaction_transition before request (Some prepared) outcome ->
  immediate_match_state_commit outcome artifact ->
  wal_checkpoint_bound artifact ->
  compatibility_admitted artifact validate ->
  artifact_state_generation artifact =
    S (store_publication_generation before) /\
  exact_validation_evidence
    (artifact_bytes artifact) validate (artifact_image artifact).
Proof.
  intros before request prepared outcome Object artifact validate
    Htransition Hstore Hwal Hadmitted.
  destruct Hwal as [_ [_ [_ Hcommit]]].
  split; [|exact Hadmitted].
  rewrite Hstore in Hcommit.
  inversion Htransition; subst outcome; cbn in Hcommit. exact Hcommit.
Qed.

Theorem refused_match_cannot_publish_successor_semantic_state : forall
    before request outcome Object
    (artifact : rba1_generation_artifact Object MatchCatalog
      (list isolated_candidate_pool)),
  @match_transaction_transition before request None outcome ->
  immediate_match_state_commit outcome artifact ->
  wal_checkpoint_bound artifact ->
  artifact_state_generation artifact = S (store_publication_generation before) ->
  False.
Proof.
  intros before request outcome Object artifact Htransition Hstore Hwal Hsuccessor.
  destruct Hwal as [_ [_ [_ Hcommit]]].
  pose proof (@refused_match_transaction_is_strongly_atomic
    before request outcome Htransition) as [Hunchanged _].
  rewrite Hstore, Hunchanged in Hcommit. lia.
Qed.

Definition runtime_new_authority_visible {Object SemanticState}
    (state : @publication_state Object SemanticState) : bool :=
  result_is_acknowledged state.

Definition rba1_readmission_is_coupled
    {Object Catalog ExternalSnapshots}
    (encode_outer_act : TwoLevelAuthority -> bytes)
    (encode_bag_act : ChannelBag -> bytes)
    (candidate : rba1_generation_candidate)
    (validate : rba1_image -> checkpoint_validation)
    (artifact : rba1_generation_artifact Object Catalog ExternalSnapshots)
    (state : @publication_state Object
      (@transactional_store_state Catalog ExternalSnapshots)) : Prop :=
  candidate_new_authority_bytes candidate = artifact_bytes artifact /\
  candidate_expected_receipt candidate = artifact_expected_binding artifact /\
  candidate_observed_receipt candidate = artifact_observed_binding artifact /\
  candidate_checkpoint_index_generation candidate =
    artifact_index_generation artifact /\
  recover_rba1_generation candidate validate =
    RecoveredNewAuthority (artifact_image artifact) /\
  rba1_refines_store encode_outer_act encode_bag_act artifact /\
  semantic_after state = Some (artifact_store artifact).

Theorem runtime_new_authority_requires_acknowledged_exact_readmission : forall
    (Object : Type)
    initial_accounting initial_physical_diagnostics expected semantic
    encode_outer_act encode_bag_act candidate validate
    (artifact : rba1_generation_artifact Object MatchCatalog
      (list isolated_candidate_pool))
    (state : @publication_state Object MatchStoreState),
  @reachable Object MatchStoreState initial_accounting
    initial_physical_diagnostics expected semantic state ->
  rba1_readmission_is_coupled encode_outer_act encode_bag_act
    candidate validate artifact state ->
  wal_checkpoint_bound artifact ->
  runtime_new_authority_visible state = true ->
  exact_validation_evidence
      (artifact_bytes artifact) validate (artifact_image artifact) /\
  rba1_refines_store encode_outer_act encode_bag_act artifact /\
  candidate_expected_receipt candidate = artifact_expected_binding artifact /\
  candidate_observed_receipt candidate = artifact_observed_binding artifact /\
  candidate_checkpoint_index_generation candidate =
    artifact_index_generation artifact /\
  receipt_binding_is_exact
    (artifact_expected_binding artifact)
    (artifact_observed_binding artifact) /\
  binding_index_generation (artifact_expected_binding artifact) =
    artifact_index_generation artifact /\
  artifact_descriptor_state_generation artifact =
    artifact_state_generation artifact /\
  artifact_state_generation artifact =
    store_publication_generation (artifact_store artifact) /\
  semantic_after state = Some (artifact_store artifact) /\
  mutable_owners state = [NewWriter] /\
  selected_authority state = NewAuthority /\
  physical_authority state = Some NewAuthority /\
  semantics_are_readmitted state = true /\
  semantic_after state = Some (semantic_before state) /\
  In CatalogDirectorySyncCertificate (certificates state).
Proof.
  intros Object initial_accounting initial_physical_diagnostics expected
    semantic encode_outer_act encode_bag_act candidate validate artifact state
    Hreachable
    [Hbytes [Hexpected [Hobserved
      [Hindex [Hrecovered [Hrefines Hsemantic]]]]]]
    Hwal Hvisible.
  pose proof (@selected_new_implies_complete_valid_exactly_bound_successor
    candidate validate (artifact_image artifact) Hrecovered)
    as [_ [_ [_ [_ Hexact]]]].
  destruct Hwal as [Hbinding [Hartifact_index
    [Hdescriptor_state Hartifact_state]]].
  rewrite Hbytes in Hexact.
  pose proof (@acknowledgement_requires_recovery_readmission_and_same_semantics
    Object MatchStoreState initial_accounting initial_physical_diagnostics
    expected semantic state Hreachable Hvisible) as Hpublication.
  destruct Hpublication as
    [Howners [Hselected [Hphysical [Hadmitted [Hsame Hdirectory]]]]].
  split; [exact Hexact|].
  split; [exact Hrefines|].
  split; [exact Hexpected|].
  split; [exact Hobserved|].
  split; [exact Hindex|].
  split; [exact Hbinding|].
  split; [exact Hartifact_index|].
  split; [exact Hdescriptor_state|].
  split; [exact Hartifact_state|].
  repeat split; assumption.
Qed.

Definition recovery_witness_encode_outer_act
    (_ : TwoLevelAuthority) : bytes := [1].
Definition recovery_witness_encode_bag_act (_ : ChannelBag) : bytes := [2].
Definition recovery_witness_authority : TwoLevelAuthority := [([1], [])].
Definition recovery_witness_image : rba1_image := Rba1Image [1] [[2]].
Definition recovery_witness_hash (encoded : bytes) : nat := length encoded.
Definition recovery_witness_object (key encoded_length : nat) : nat * nat :=
  (key, encoded_length).
Definition recovery_witness_object_key (object : nat * nat) : nat := fst object.
Definition recovery_witness_object_length (object : nat * nat) : nat := snd object.
Definition recovery_witness_binding : checkpoint_binding_state :=
  CheckpointBindingState 1 2 3 4 5 6 7 8 9 11 12 13 14 15 16 17.
Definition recovery_witness_store : @transactional_store_state unit unit :=
  TransactionalStoreState recovery_witness_authority 0 (fun _ => None) tt 5.
Definition recovery_witness_content_object : nat * nat :=
  recovery_witness_object
    (recovery_witness_hash
      (rba1_checkpoint_hash_preimage recovery_witness_image))
    (length (encode_rba1 recovery_witness_image)).
Definition recovery_witness_artifact :
    rba1_generation_artifact (nat * nat) unit unit :=
  Rba1GenerationArtifact recovery_witness_image
    (encode_rba1 recovery_witness_image) recovery_witness_store 5 11 5
    recovery_witness_content_object [recovery_witness_content_object]
    recovery_witness_binding recovery_witness_binding.

Example rba1_object_construction_and_inventory_binding_are_inhabited :
  rba1_object_constructed_and_inserted
    recovery_witness_object_key recovery_witness_object_length
    recovery_witness_object recovery_witness_hash []
    recovery_witness_artifact /\
  rba1_object_bound recovery_witness_object_key
    recovery_witness_object_length recovery_witness_hash
    recovery_witness_artifact.
Proof.
  assert (Hconstructed : rba1_object_constructed_and_inserted
      recovery_witness_object_key recovery_witness_object_length
      recovery_witness_object recovery_witness_hash []
      recovery_witness_artifact).
  { unfold rba1_object_constructed_and_inserted, recovery_witness_artifact,
      recovery_witness_content_object, recovery_witness_object,
      recovery_witness_object_key, recovery_witness_object_length.
    repeat split; intros; reflexivity. }
  split; [exact Hconstructed|].
  exact (@constructed_rba1_object_is_exactly_inventory_bound
    (nat * nat) unit unit recovery_witness_object_key
    recovery_witness_object_length recovery_witness_object
    recovery_witness_hash [] recovery_witness_artifact Hconstructed).
Qed.

Definition recovery_match_store : MatchStoreState :=
  TransactionalStoreState recovery_witness_authority 0 (fun _ => None) [] 5.
Definition recovery_match_artifact :
    rba1_generation_artifact (nat * nat) MatchCatalog
      (list isolated_candidate_pool) :=
  Rba1GenerationArtifact recovery_witness_image
    (encode_rba1 recovery_witness_image) recovery_match_store 5 11 5
    recovery_witness_content_object [recovery_witness_content_object]
    recovery_witness_binding recovery_witness_binding.
Definition recovery_generation_candidate : rba1_generation_candidate :=
  Rba1GenerationCandidate DurableCompleteNewImage []
    (encode_rba1 recovery_witness_image) 10 11
    recovery_witness_binding recovery_witness_binding.
Definition recovery_runtime_budget : runtime_budget_snapshot :=
  RuntimeBudgetSnapshot 1 2 3 4 None [5].
Definition recovery_physical_diagnostics : physical_diagnostic_snapshot :=
  PhysicalDiagnosticSnapshot 1 2 3 4 5.
Definition recovery_expected_objects : list (nat * nat) :=
  [recovery_witness_content_object].
Definition recovery_publication_s0 :=
  @initial_publication_state (nat * nat) MatchStoreState
    recovery_runtime_budget recovery_physical_diagnostics recovery_match_store.
Definition recovery_publication_s1 :=
  transfer_and_advance Preflighted Publisher PreflightCertificate
    recovery_publication_s0.
Definition recovery_publication_s2 :=
  advance CleanupComplete CleanupCertificate recovery_publication_s1.
Definition recovery_publication_s3 :=
  write_prepare recovery_expected_objects recovery_publication_s2.
Definition recovery_publication_s4 :=
  advance PrepareDurable PrepareSyncCertificate recovery_publication_s3.
Definition recovery_publication_s5 := write_index recovery_publication_s4.
Definition recovery_publication_s6 :=
  advance IndexDurable IndexSyncCertificate recovery_publication_s5.
Definition recovery_publication_s7 :=
  advance CommitWritten CommitWriteCertificate recovery_publication_s6.
Definition recovery_publication_s8 :=
  advance CommitDurable CommitSyncCertificate recovery_publication_s7.
Definition recovery_publication_s9 :=
  advance ArtifactDirectoryDurable ArtifactDirectoryCertificate
    recovery_publication_s8.
Definition recovery_publication_s10 :=
  advance CasVerified ExactCatalogCasCertificate recovery_publication_s9.
Definition recovery_publication_s11 :=
  advance CatalogWriteStarted CatalogWriteStartCertificate
    recovery_publication_s10.
Definition recovery_publication_s12 :=
  advance CatalogWritten CatalogWriteCompleteCertificate
    recovery_publication_s11.
Definition recovery_publication_s13 :=
  select_new_catalog recovery_publication_s12.
Definition recovery_publication_s14 :=
  advance CatalogDirectoryDurable CatalogDirectorySyncCertificate
    recovery_publication_s13.
Definition recovery_publication_s15 := recover_new recovery_publication_s14.
Definition recovery_publication_s16 := readmit recovery_publication_s15.
Definition recovery_publication_s17 := acknowledge recovery_publication_s16.

Example acknowledged_exact_rba1_readmission_is_reachable_and_inhabited :
  @reachable (nat * nat) MatchStoreState recovery_runtime_budget
      recovery_physical_diagnostics recovery_expected_objects
      recovery_match_store recovery_publication_s17 /\
  rba1_readmission_is_coupled recovery_witness_encode_outer_act
      recovery_witness_encode_bag_act recovery_generation_candidate
      canonical_checkpoint_validation recovery_match_artifact
      recovery_publication_s17 /\
  wal_checkpoint_bound recovery_match_artifact /\
  runtime_new_authority_visible recovery_publication_s17 = true.
Proof.
  split.
  - assert (H0 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s0).
    { apply ReachInitial. }
    assert (H1 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s1).
    { eapply ReachStep; [exact H0|apply StepPreflight; reflexivity]. }
    assert (H2 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s2).
    { eapply ReachStep; [exact H1|apply StepCleanup; reflexivity]. }
    assert (H3 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s3).
    { eapply ReachStep; [exact H2|apply StepWritePrepare; reflexivity]. }
    assert (H4 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s4).
    { eapply ReachStep; [exact H3|apply StepSyncPrepare; reflexivity]. }
    assert (H5 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s5).
    { eapply ReachStep; [exact H4|apply StepWriteIndex; reflexivity]. }
    assert (H6 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s6).
    { eapply ReachStep; [exact H5|apply StepSyncIndex; reflexivity]. }
    assert (H7 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s7).
    { eapply ReachStep; [exact H6|apply StepWriteCommit; reflexivity]. }
    assert (H8 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s8).
    { eapply ReachStep; [exact H7|apply StepSyncCommit; reflexivity]. }
    assert (H9 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s9).
    { eapply ReachStep; [exact H8|apply StepSyncArtifactDirectory; reflexivity]. }
    assert (H10 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s10).
    { eapply ReachStep; [exact H9|apply StepVerifyCas; reflexivity]. }
    assert (H11 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s11).
    { eapply ReachStep; [exact H10|apply StepStartCatalogWrite; reflexivity]. }
    assert (H12 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s12).
    { eapply ReachStep; [exact H11|apply StepCompleteCatalogWrite; reflexivity]. }
    assert (H13 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s13).
    { eapply ReachStep; [exact H12|apply StepSyncCatalogFile; reflexivity]. }
    assert (H14 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s14).
    { eapply ReachStep; [exact H13|apply StepSyncCatalogDirectory; reflexivity]. }
    assert (H15 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s15).
    { eapply ReachStep; [exact H14|apply StepRecover; reflexivity]. }
    assert (H16 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s16).
    { eapply ReachStep; [exact H15|apply StepReadmit; reflexivity]. }
    assert (H17 : @reachable (nat * nat) MatchStoreState
        recovery_runtime_budget recovery_physical_diagnostics
        recovery_expected_objects recovery_match_store recovery_publication_s17).
    { eapply ReachStep; [exact H16|apply StepAcknowledge; reflexivity]. }
    exact H17.
  - repeat split; vm_compute; repeat split; reflexivity.
Qed.

(** A decodable current-format prefix owns format selection. If exact
    validation fails, recovery rejects it rather than interpreting the same
    bytes as legacy data. *)
Inductive exact_format_recovery : Type :=
| ExactCurrentRba1 (image : rba1_image)
| ExactLegacyCandidate (image : rba1_image)
| RejectCurrentOrLegacy.

Definition recover_exact_current_then_legacy
    (validate : rba1_image -> checkpoint_validation)
    (decode_legacy : bytes -> option rba1_image)
    (encoded : bytes) : exact_format_recovery :=
  match decode_rba1 encoded with
  | Some _ =>
      match recover_exact_validated_checkpoint encoded validate with
      | Some image => ExactCurrentRba1 image
      | None => RejectCurrentOrLegacy
      end
  | None =>
      match decode_legacy encoded with
      | Some image => ExactLegacyCandidate image
      | None => RejectCurrentOrLegacy
      end
  end.

Theorem invalid_current_rba1_never_falls_through_to_legacy : forall
    validate decode_legacy encoded decoded,
  decode_rba1 encoded = Some decoded ->
  recover_exact_validated_checkpoint encoded validate = None ->
  recover_exact_current_then_legacy validate decode_legacy encoded =
    RejectCurrentOrLegacy.
Proof.
  intros validate decode_legacy encoded decoded Hcurrent Hinvalid.
  unfold recover_exact_current_then_legacy. now rewrite Hcurrent, Hinvalid.
Qed.

Theorem canonical_current_rba1_preempts_every_legacy_decoder : forall
    decode_legacy image,
  recover_exact_current_then_legacy canonical_checkpoint_validation
    decode_legacy (encode_rba1 image) = ExactCurrentRba1 image.
Proof.
  intros. unfold recover_exact_current_then_legacy.
  rewrite rba1_decode_encode_identity,
    canonical_exact_recovery_recovers_exact_image. reflexivity.
Qed.

Inductive rba1_recovery_invariant_id : Type :=
| RecoveryInvExactValidationSubject
| RecoveryInvObjectInventoryBinding
| RecoveryInvSeparatedGenerationBinding
| RecoveryInvMatchSuccessorGeneration
| RecoveryInvNoPartialAuthority
| RecoveryInvRestartAndLoadedEmptyExact.

Definition rba1_recovery_invariant_catalog : list rba1_recovery_invariant_id :=
  [RecoveryInvExactValidationSubject; RecoveryInvObjectInventoryBinding;
   RecoveryInvSeparatedGenerationBinding; RecoveryInvMatchSuccessorGeneration;
   RecoveryInvNoPartialAuthority; RecoveryInvRestartAndLoadedEmptyExact].

Theorem rba1_recovery_invariant_catalog_is_duplicate_free_and_exact :
  NoDup rba1_recovery_invariant_catalog /\
  length rba1_recovery_invariant_catalog = 6%nat.
Proof. split; [repeat constructor; simpl; intuition discriminate | reflexivity]. Qed.

Print Assumptions canonical_checkpoint_validation_recovers_exact_image.
Print Assumptions exact_recovery_is_sound.
Print Assumptions checkpoint_binding_eqb_reflects_exact_receipt.
Print Assumptions selected_new_implies_complete_valid_exactly_bound_successor.
Print Assumptions loaded_empty_rba1_survives_restart_exactly.
Print Assumptions accepted_migration_can_become_restart_authority_only_after_validation.
Print Assumptions constructed_rba1_object_is_exactly_inventory_bound.
Print Assumptions rba1_object_construction_and_inventory_binding_are_inhabited.
Print Assumptions accepted_match_binds_artifact_to_exact_successor_state_generation.
Print Assumptions refused_match_cannot_publish_successor_semantic_state.
Print Assumptions runtime_new_authority_requires_acknowledged_exact_readmission.
Print Assumptions acknowledged_exact_rba1_readmission_is_reachable_and_inhabited.
Print Assumptions invalid_current_rba1_never_falls_through_to_legacy.
Print Assumptions rba1_recovery_invariant_catalog_is_duplicate_free_and_exact.

End TwoLevelRSpaceCheckpointRecovery.
