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
  candidate_wal_base_generation : nat;
  candidate_checkpoint_generation : nat;
  candidate_expected_receipt : checkpoint_binding_state;
  candidate_observed_receipt : checkpoint_binding_state
}.

Definition restart_grade_rba1
    (candidate : rba1_generation_candidate)
    (validate : rba1_image -> checkpoint_validation) : bool :=
  Nat.eqb (candidate_checkpoint_generation candidate)
      (S (candidate_wal_base_generation candidate)) &&
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
  candidate_checkpoint_generation candidate <>
    S (candidate_wal_base_generation candidate) ->
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
  destruct (Nat.eqb (candidate_checkpoint_generation candidate)
    (S (candidate_wal_base_generation candidate))); [|reflexivity].
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
  destruct (Nat.eqb (candidate_checkpoint_generation candidate)
    (S (candidate_wal_base_generation candidate)));
  destruct (checkpoint_binding_eqb
    (candidate_expected_receipt candidate)
    (candidate_observed_receipt candidate)); reflexivity.
Qed.

Theorem selected_new_implies_complete_valid_exactly_bound_successor : forall
    candidate validate image,
  recover_rba1_generation candidate validate = RecoveredNewAuthority image ->
  candidate_slot_image candidate = DurableCompleteNewImage /\
  candidate_checkpoint_generation candidate =
    S (candidate_wal_base_generation candidate) /\
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
  destruct (Nat.eqb (candidate_checkpoint_generation candidate)
      (S (candidate_wal_base_generation candidate))) eqn:Hgeneration;
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
  split; [now apply checkpoint_binding_eqb_reflects_exact_receipt|].
  now apply exact_recovery_is_sound.
Qed.

Theorem loaded_empty_rba1_survives_restart_exactly : forall
    candidate outer_act empty_bag_act,
  candidate_slot_image candidate = DurableCompleteNewImage ->
  candidate_new_authority_bytes candidate =
    encode_rba1 (loaded_empty_image outer_act empty_bag_act) ->
  candidate_checkpoint_generation candidate =
    S (candidate_wal_base_generation candidate) ->
  receipt_binding_is_exact
    (candidate_expected_receipt candidate)
    (candidate_observed_receipt candidate) ->
  recover_rba1_generation candidate canonical_checkpoint_validation =
    RecoveredNewAuthority (loaded_empty_image outer_act empty_bag_act).
Proof.
  intros candidate outer_act empty_bag_act Hslot Hbytes Hgeneration Hreceipt.
  apply complete_restart_grade_candidate_recovers_exact_successor with
    (image := loaded_empty_image outer_act empty_bag_act).
  - exact Hslot.
  - unfold restart_grade_rba1. apply Nat.eqb_eq in Hgeneration.
    rewrite Hgeneration.
    apply checkpoint_binding_eqb_reflects_exact_receipt in Hreceipt.
    rewrite Hreceipt, Hbytes,
      canonical_exact_recovery_recovers_exact_image. reflexivity.
  - rewrite Hbytes. apply canonical_exact_recovery_recovers_exact_image.
Qed.

Theorem accepted_migration_can_become_restart_authority_only_after_validation :
  forall decoder legacy image candidate,
  decoder legacy = Some image ->
  candidate_slot_image candidate = DurableCompleteNewImage ->
  candidate_new_authority_bytes candidate =
    migration_authoritative_bytes (migrate_legacy_checkpoint decoder legacy) ->
  candidate_checkpoint_generation candidate =
    S (candidate_wal_base_generation candidate) ->
  receipt_binding_is_exact
    (candidate_expected_receipt candidate)
    (candidate_observed_receipt candidate) ->
  recover_rba1_generation candidate canonical_checkpoint_validation =
    RecoveredNewAuthority image.
Proof.
  intros decoder legacy image candidate Hmigration Hslot Hbytes Hgeneration Hreceipt.
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
    rewrite Hreceipt, Hexact. reflexivity.
Qed.

(** A generation artifact binds the logical two-level store, its exact RBA1
    image, the one content-addressed object admitted to the checkpoint
    inventory, and the WAL receipt. WAL sequence and store generation remain
    distinct fields; only the index generation is bound to logical store
    publication generation. *)
Record rba1_generation_artifact
    (Object Catalog ExternalSnapshots : Type) : Type := Rba1GenerationArtifact {
  artifact_image : rba1_image;
  artifact_bytes : bytes;
  artifact_store : @transactional_store_state Catalog ExternalSnapshots;
  artifact_generation : nat;
  artifact_object : Object;
  artifact_expected_members : list Object;
  artifact_expected_binding : checkpoint_binding_state;
  artifact_observed_binding : checkpoint_binding_state
}.

Arguments artifact_image {Object Catalog ExternalSnapshots} _.
Arguments artifact_bytes {Object Catalog ExternalSnapshots} _.
Arguments artifact_store {Object Catalog ExternalSnapshots} _.
Arguments artifact_generation {Object Catalog ExternalSnapshots} _.
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

Definition wal_checkpoint_bound {Object Catalog ExternalSnapshots}
    (artifact : rba1_generation_artifact Object Catalog ExternalSnapshots)
    : Prop :=
  receipt_binding_is_exact
    (artifact_expected_binding artifact)
    (artifact_observed_binding artifact) /\
  binding_index_generation (artifact_expected_binding artifact) =
    artifact_generation artifact /\
  artifact_generation artifact =
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

Theorem rba1_object_binding_is_exact : forall
    (Object Catalog ExternalSnapshots : Type)
    (object_key canonical_bytes : Object -> nat)
    (hash_bytes : bytes -> nat)
    (artifact : rba1_generation_artifact Object Catalog ExternalSnapshots),
  rba1_object_bound object_key canonical_bytes hash_bytes artifact ->
  In (artifact_object artifact) (artifact_expected_members artifact) /\
  object_key (artifact_object artifact) =
    hash_bytes (rba1_checkpoint_hash_preimage (artifact_image artifact)) /\
  canonical_bytes (artifact_object artifact) =
    length (artifact_bytes artifact) /\
  artifact_bytes artifact = encode_rba1 (artifact_image artifact).
Proof. intros. exact H. Qed.

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

Theorem accepted_match_binds_artifact_to_exact_successor_generation : forall
    before request prepared outcome Object
    (artifact : rba1_generation_artifact Object MatchCatalog
      (list isolated_candidate_pool)) validate,
  @match_transaction_transition before request (Some prepared) outcome ->
  artifact_store artifact = match_outcome_state outcome ->
  wal_checkpoint_bound artifact ->
  compatibility_admitted artifact validate ->
  artifact_generation artifact =
    S (store_publication_generation before) /\
  exact_validation_evidence
    (artifact_bytes artifact) validate (artifact_image artifact).
Proof.
  intros before request prepared outcome Object artifact validate
    Htransition Hstore Hwal Hadmitted.
  destruct Hwal as [_ [_ Hgeneration]]. split; [|exact Hadmitted].
  rewrite Hstore in Hgeneration.
  inversion Htransition; subst outcome; cbn in Hgeneration.
  exact Hgeneration.
Qed.

Theorem refused_match_cannot_publish_successor_artifact : forall
    before request outcome Object
    (artifact : rba1_generation_artifact Object MatchCatalog
      (list isolated_candidate_pool)),
  @match_transaction_transition before request None outcome ->
  artifact_store artifact = match_outcome_state outcome ->
  wal_checkpoint_bound artifact ->
  artifact_generation artifact = S (store_publication_generation before) ->
  False.
Proof.
  intros before request outcome Object artifact Htransition Hstore Hwal Hsuccessor.
  destruct Hwal as [_ [_ Hgeneration]].
  pose proof (@refused_match_transaction_is_strongly_atomic
    before request outcome Htransition) as [Hunchanged _].
  rewrite Hstore, Hunchanged in Hgeneration. lia.
Qed.

Definition runtime_new_authority_visible {Object SemanticState}
    (state : @publication_state Object SemanticState) : bool :=
  result_is_acknowledged state.

Theorem runtime_new_authority_requires_acknowledged_exact_readmission : forall
    (Object : Type)
    initial_accounting initial_physical_diagnostics expected semantic
    (state : @publication_state Object MatchStoreState),
  @reachable Object MatchStoreState initial_accounting
    initial_physical_diagnostics expected semantic state ->
  runtime_new_authority_visible state = true ->
  mutable_owners state = [NewWriter] /\
  selected_authority state = NewAuthority /\
  physical_authority state = Some NewAuthority /\
  semantics_are_readmitted state = true /\
  semantic_after state = Some (semantic_before state) /\
  In CatalogDirectorySyncCertificate (certificates state).
Proof.
  intros Object initial_accounting initial_physical_diagnostics expected
    semantic state Hreachable Hvisible.
  exact (@acknowledgement_requires_recovery_readmission_and_same_semantics
    Object MatchStoreState initial_accounting initial_physical_diagnostics
    expected semantic state Hreachable Hvisible).
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
| RecoveryInvWalStoreGenerationBinding
| RecoveryInvMatchSuccessorGeneration
| RecoveryInvNoPartialAuthority
| RecoveryInvRestartAndLoadedEmptyExact.

Definition rba1_recovery_invariant_catalog : list rba1_recovery_invariant_id :=
  [RecoveryInvExactValidationSubject; RecoveryInvObjectInventoryBinding;
   RecoveryInvWalStoreGenerationBinding; RecoveryInvMatchSuccessorGeneration;
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
Print Assumptions accepted_match_binds_artifact_to_exact_successor_generation.
Print Assumptions refused_match_cannot_publish_successor_artifact.
Print Assumptions runtime_new_authority_requires_acknowledged_exact_readmission.
Print Assumptions invalid_current_rba1_never_falls_through_to_legacy.
Print Assumptions rba1_recovery_invariant_catalog_is_duplicate_free_and_exact.

End TwoLevelRSpaceCheckpointRecovery.
