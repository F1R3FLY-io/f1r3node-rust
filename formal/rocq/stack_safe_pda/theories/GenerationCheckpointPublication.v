From Stdlib Require Import Bool List Arith Lia.
From Stdlib Require Import Sorting.Permutation Sorting.Sorted.

Import ListNotations.
Set Implicit Arguments.

(**
  Formal-first refinement theory for a fresh RSpace generation checkpoint.

  The first section proves the allocation-free ordered merge independently of
  any concrete object type.  Equality of digest keys is an error, matching the
  Rust visitor; a successful merge is an exact permutation, preserves strict
  digest order, adds exactly one object and its declared canonical bytes, and
  carries the exact six L/R/D inventory fields.

  The second section models the successful affine publication path with an
  independent state record and guarded one-step relation.  Barriers are added
  one at a time; selection, recovery, semantic equality, acknowledgement, and
  authority transfer are established only by their corresponding transition.
  TLA+ separately exhausts failures, raw torn-slot crash images, restart, and
  outcome classification.  Real filesystem synchronization, checksums,
  bounded parsers, and Rust ownership remain executable refinement duties.
*)

Section OrderedCheckpointInventory.

Context {Object : Type}.
Variable object_key : Object -> nat.
Variable canonical_bytes : Object -> nat.

Fixpoint insert_unique_by_key
    (item : Object)
    (retained : list Object) : option (list Object) :=
  match retained with
  | [] => Some [item]
  | head :: tail =>
      match Nat.compare (object_key item) (object_key head) with
      | Lt => Some (item :: retained)
      | Eq => None
      | Gt =>
          match insert_unique_by_key item tail with
          | Some merged => Some (head :: merged)
          | None => None
          end
      end
  end.

Fixpoint canonical_byte_sum (objects : list Object) : nat :=
  match objects with
  | [] => 0
  | head :: tail => canonical_bytes head + canonical_byte_sum tail
  end.

Lemma canonical_byte_sum_permutation :
  forall left right,
    Permutation left right ->
    canonical_byte_sum left = canonical_byte_sum right.
Proof.
  intros left right permutation; induction permutation; simpl; lia.
Qed.

Theorem successful_merge_is_exact_permutation :
  forall item retained merged,
    insert_unique_by_key item retained = Some merged ->
    Permutation merged (item :: retained).
Proof.
  intros item retained; induction retained as [|head tail IH]; intros merged merge.
  - simpl in merge. inversion merge; subst; apply Permutation_refl.
  - simpl in merge.
    destruct (Nat.compare (object_key item) (object_key head)) eqn:comparison.
    + discriminate.
    + inversion merge; subst; apply Permutation_refl.
    + destruct (insert_unique_by_key item tail) as [tail_merged|] eqn:tail_merge;
        try discriminate.
      inversion merge; subst.
      eapply Permutation_trans.
      * apply perm_skip. exact (IH tail_merged eq_refl).
      * apply perm_swap.
Qed.

Arguments successful_merge_is_exact_permutation item retained merged _ : clear implicits.

Theorem successful_merge_adds_exactly_one_member :
  forall item retained merged,
    insert_unique_by_key item retained = Some merged ->
    length merged = S (length retained).
Proof.
  intros item retained merged merge.
  pose proof (successful_merge_is_exact_permutation
    item retained merged merge) as permutation.
  apply Permutation_length in permutation. simpl in permutation. exact permutation.
Qed.

Arguments successful_merge_adds_exactly_one_member item retained merged _ : clear implicits.

Theorem successful_merge_adds_exactly_declared_bytes :
  forall item retained merged,
    insert_unique_by_key item retained = Some merged ->
    canonical_byte_sum merged =
      canonical_bytes item + canonical_byte_sum retained.
Proof.
  intros item retained merged merge.
  change (canonical_byte_sum merged = canonical_byte_sum (item :: retained)).
  apply canonical_byte_sum_permutation.
  exact (successful_merge_is_exact_permutation item retained merged merge).
Qed.

Arguments successful_merge_adds_exactly_declared_bytes item retained merged _ : clear implicits.

Theorem successful_merge_membership_is_exact :
  forall item retained merged candidate,
    insert_unique_by_key item retained = Some merged ->
    (In candidate merged <-> candidate = item \/ In candidate retained).
Proof.
  intros item retained merged candidate merge.
  pose proof (successful_merge_is_exact_permutation
    item retained merged merge) as permutation.
  split; intro member.
  - pose proof (@Permutation_in Object merged (item :: retained)
    candidate permutation member) as exact_member.
    simpl in exact_member. destruct exact_member as [same|retained_member].
    + left. symmetry. exact same.
    + right. exact retained_member.
  - apply (@Permutation_in Object (item :: retained) merged candidate
      (Permutation_sym permutation)). simpl.
    destruct member as [same|retained_member].
    + left. symmetry. exact same.
    + right. exact retained_member.
Qed.

Definition key_lt (left right : Object) : Prop :=
  object_key left < object_key right.

Theorem successful_merge_preserves_strict_digest_order :
  forall item retained merged,
    StronglySorted key_lt retained ->
    insert_unique_by_key item retained = Some merged ->
    StronglySorted key_lt merged.
Proof.
  intros item retained; induction retained as [|head tail IH];
    intros merged sorted merge.
  - simpl in merge. inversion merge; constructor; constructor.
  - inversion sorted as [|? ? tail_sorted head_before_tail]; subst.
    simpl in merge.
    destruct (Nat.compare (object_key item) (object_key head)) eqn:comparison.
    + discriminate.
    + inversion merge; subst.
      constructor.
      * exact sorted.
      * apply Forall_forall. intros candidate member.
        simpl in member. destruct member as [candidate_is_head|candidate_in_tail].
        -- subst. apply Nat.compare_lt_iff in comparison. exact comparison.
        -- apply Forall_forall with (x := candidate) in head_before_tail;
             try exact candidate_in_tail.
           unfold key_lt in *. apply Nat.compare_lt_iff in comparison. lia.
    + destruct (insert_unique_by_key item tail) as [tail_merged|] eqn:tail_merge;
        try discriminate.
      inversion merge; subst.
      constructor.
      * exact (IH tail_merged tail_sorted eq_refl).
      * apply Forall_forall. intros candidate candidate_in_merged.
        apply successful_merge_membership_is_exact
          with (candidate := candidate) in tail_merge.
        apply tail_merge in candidate_in_merged.
        destruct candidate_in_merged as [candidate_is_item|candidate_in_tail].
        -- subst. apply Nat.compare_gt_iff in comparison. exact comparison.
        -- apply Forall_forall with (x := candidate) in head_before_tail;
             assumption.
Qed.

Theorem existing_digest_is_rejected :
  forall item retained,
    StronglySorted key_lt retained ->
    (exists candidate,
      In candidate retained /\ object_key candidate = object_key item) ->
    insert_unique_by_key item retained = None.
Proof.
  intros item retained; induction retained as [|head tail IH];
    intros sorted alias.
  - destruct alias as [candidate [member _]]. inversion member.
  - inversion sorted as [|? ? tail_sorted head_before_tail]; subst.
    destruct alias as [candidate [[candidate_is_head|candidate_in_tail] key_equal]].
    + subst candidate. simpl. rewrite <- key_equal, Nat.compare_refl. reflexivity.
    + simpl.
      destruct (Nat.compare (object_key item) (object_key head)) eqn:comparison.
      * reflexivity.
      * apply Nat.compare_lt_iff in comparison.
        apply Forall_forall with (x := candidate) in head_before_tail;
          try exact candidate_in_tail.
        unfold key_lt in head_before_tail. lia.
      * rewrite (IH tail_sorted).
        -- reflexivity.
        -- exists candidate. split; assumption.
Qed.

Record checkpoint_descriptor : Type := CheckpointDescriptor {
  current_objects : nat;
  current_canonical_bytes : nat;
  retained_objects : nat;
  retained_canonical_bytes : nat;
  data_objects : nat;
  data_canonical_bytes : nat;
  ordered_members : list Object
}.

Definition make_checkpoint_descriptor
    (current retained merged : list Object) : checkpoint_descriptor :=
  CheckpointDescriptor
    (length current)
    (canonical_byte_sum current)
    (length retained)
    (canonical_byte_sum retained)
    (length merged)
    (canonical_byte_sum merged)
    merged.

Definition descriptor_refines_merge
    (current retained : list Object)
    (retention_manifest : Object)
    (descriptor : checkpoint_descriptor) : Prop :=
  exists merged,
    insert_unique_by_key retention_manifest retained = Some merged /\
    descriptor = make_checkpoint_descriptor current retained merged.

Theorem refined_descriptor_carries_exact_six_inventories :
  forall current retained retention_manifest descriptor,
    descriptor_refines_merge current retained retention_manifest descriptor ->
    current_objects descriptor = length current /\
    current_canonical_bytes descriptor = canonical_byte_sum current /\
    retained_objects descriptor = length retained /\
    retained_canonical_bytes descriptor = canonical_byte_sum retained /\
    data_objects descriptor = S (length retained) /\
    data_canonical_bytes descriptor =
      canonical_bytes retention_manifest + canonical_byte_sum retained.
Proof.
  intros current retained retention_manifest descriptor
    [merged [merge descriptor_is_exact]].
  subst descriptor. simpl.
  rewrite (successful_merge_adds_exactly_one_member
    retention_manifest retained merged merge).
  rewrite (successful_merge_adds_exactly_declared_bytes
    retention_manifest retained merged merge).
  repeat split; reflexivity.
Qed.

Definition prepare_fixed_len : nat := 288.
Definition prepare_reference_len : nat := 40.
Definition prepare_checksum_len : nat := 8.
Definition pack_header_len : nat := 128.
Definition index_header_len : nat := 64.
Definition index_record_len : nat := 72.
Definition commit_record_len : nat := 400.

Definition checkpoint_prepare_len (descriptor : checkpoint_descriptor) : nat :=
  prepare_fixed_len +
  prepare_reference_len * data_objects descriptor +
  data_canonical_bytes descriptor +
  prepare_checksum_len.

Definition checkpoint_index_tail (descriptor : checkpoint_descriptor) : nat :=
  index_header_len + index_record_len * data_objects descriptor.

Definition checkpoint_commit_address (descriptor : checkpoint_descriptor) : nat :=
  pack_header_len + checkpoint_prepare_len descriptor.

Definition checkpoint_pack_tail (descriptor : checkpoint_descriptor) : nat :=
  checkpoint_commit_address descriptor + commit_record_len.

Theorem refined_descriptor_has_exact_wire_layout :
  forall current retained retention_manifest descriptor,
    descriptor_refines_merge current retained retention_manifest descriptor ->
    checkpoint_prepare_len descriptor =
      prepare_fixed_len +
      prepare_reference_len * S (length retained) +
      canonical_bytes retention_manifest +
      canonical_byte_sum retained +
      prepare_checksum_len /\
    checkpoint_index_tail descriptor =
      index_header_len + index_record_len * S (length retained) /\
    checkpoint_commit_address descriptor =
      pack_header_len + checkpoint_prepare_len descriptor /\
    checkpoint_pack_tail descriptor =
      checkpoint_commit_address descriptor + commit_record_len.
Proof.
  intros current retained retention_manifest descriptor refinement.
  pose proof (refined_descriptor_carries_exact_six_inventories refinement)
    as [_ [_ [_ [_ [objects bytes]]]]].
  repeat split.
  - unfold checkpoint_prepare_len. rewrite objects, bytes. lia.
  - unfold checkpoint_index_tail. rewrite objects. lia.
Qed.

End OrderedCheckpointInventory.

Section CrashSelection.

Inductive durable_slot_image : Type :=
| DurableOldImage
| DurableTornImage
| DurableCompleteNewImage.

Inductive recovery_selection : Type :=
| SelectOld
| SelectNew
| RejectDamagedNew.

Definition select_after_crash
    (slot : durable_slot_image)
    (restart_grade_generation : bool) : recovery_selection :=
  match slot with
  | DurableOldImage | DurableTornImage => SelectOld
  | DurableCompleteNewImage =>
      if restart_grade_generation then SelectNew else RejectDamagedNew
  end.

Theorem raw_slot_recovery_selects_old_complete_new_or_rejects :
  forall slot restart_grade_generation,
    select_after_crash slot restart_grade_generation = SelectOld \/
    (select_after_crash slot restart_grade_generation = SelectNew /\
     slot = DurableCompleteNewImage /\
     restart_grade_generation = true) \/
    (select_after_crash slot restart_grade_generation = RejectDamagedNew /\
     slot = DurableCompleteNewImage /\
     restart_grade_generation = false).
Proof.
  intros slot restart_grade_generation.
  destruct slot; simpl; try (left; reflexivity).
  destruct restart_grade_generation; simpl.
  - right; left; repeat split; reflexivity.
  - right; right; repeat split; reflexivity.
Qed.

End CrashSelection.

Section AffinePublicationProtocol.

Context {Object SemanticState : Type}.

Inductive publication_phase : Type :=
| OldAdmitted
| Preflighted
| CleanupComplete
| PrepareWritten
| PrepareDurable
| IndexWritten
| IndexDurable
| CommitWritten
| CommitDurable
| ArtifactDirectoryDurable
| CasVerified
| CatalogWriteStarted
| CatalogWritten
| CatalogFileDurable
| CatalogDirectoryDurable
| PhysicalRecovered
| SemanticReadmitted
| Acknowledged.

Inductive owner_token : Type :=
| OldWriter
| Publisher
| NewWriter.

Inductive barrier_certificate : Type :=
| PreflightCertificate
| CleanupCertificate
| PrepareWriteCertificate
| PrepareSyncCertificate
| IndexWriteCertificate
| IndexSyncCertificate
| CommitWriteCertificate
| CommitSyncCertificate
| ArtifactDirectoryCertificate
| ExactCatalogCasCertificate
| CatalogWriteStartCertificate
| CatalogWriteCompleteCertificate
| CatalogFileSyncCertificate
| CatalogDirectorySyncCertificate
| PhysicalRecoveryCertificate
| SemanticReadmissionCertificate.

Inductive authority : Type := OldAuthority | NewAuthority.

Record publication_state : Type := PublicationState {
  state_phase : publication_phase;
  mutable_owners : list owner_token;
  certificates : list barrier_certificate;
  prepare_members : list Object;
  index_members : list Object;
  selected_authority : authority;
  physical_authority : option authority;
  semantic_before : SemanticState;
  semantic_after : option SemanticState;
  semantics_are_readmitted : bool;
  result_is_acknowledged : bool;
  introduction_charge : nat;
  delivery_charge : nat;
  trace_charge : nat
}.

Definition initial_publication_state (semantic : SemanticState) : publication_state :=
  PublicationState OldAdmitted [OldWriter] [] [] [] OldAuthority None
    semantic None false false 0 0 0.

Definition advance
    (next_phase : publication_phase)
    (certificate : barrier_certificate)
    (state : publication_state) : publication_state :=
  PublicationState next_phase (mutable_owners state)
    (certificate :: certificates state)
    (prepare_members state) (index_members state)
    (selected_authority state) (physical_authority state)
    (semantic_before state) (semantic_after state)
    (semantics_are_readmitted state) (result_is_acknowledged state)
    (introduction_charge state) (delivery_charge state) (trace_charge state).

Definition transfer_and_advance
    (next_phase : publication_phase)
    (owner : owner_token)
    (certificate : barrier_certificate)
    (state : publication_state) : publication_state :=
  PublicationState next_phase [owner] (certificate :: certificates state)
    (prepare_members state) (index_members state)
    (selected_authority state) (physical_authority state)
    (semantic_before state) (semantic_after state)
    (semantics_are_readmitted state) (result_is_acknowledged state)
    (introduction_charge state) (delivery_charge state) (trace_charge state).

Definition write_prepare
    (expected : list Object)
    (state : publication_state) : publication_state :=
  PublicationState PrepareWritten (mutable_owners state)
    (PrepareWriteCertificate :: certificates state)
    expected (index_members state)
    (selected_authority state) (physical_authority state)
    (semantic_before state) (semantic_after state)
    (semantics_are_readmitted state) (result_is_acknowledged state)
    (introduction_charge state) (delivery_charge state) (trace_charge state).

Definition write_index
    (state : publication_state) : publication_state :=
  PublicationState IndexWritten (mutable_owners state)
    (IndexWriteCertificate :: certificates state)
    (prepare_members state) (prepare_members state)
    (selected_authority state) (physical_authority state)
    (semantic_before state) (semantic_after state)
    (semantics_are_readmitted state) (result_is_acknowledged state)
    (introduction_charge state) (delivery_charge state) (trace_charge state).

Definition select_new_catalog (state : publication_state) : publication_state :=
  PublicationState CatalogFileDurable (mutable_owners state)
    (CatalogFileSyncCertificate :: certificates state)
    (prepare_members state) (index_members state)
    NewAuthority (physical_authority state)
    (semantic_before state) (semantic_after state)
    (semantics_are_readmitted state) (result_is_acknowledged state)
    (introduction_charge state) (delivery_charge state) (trace_charge state).

Definition recover_new (state : publication_state) : publication_state :=
  PublicationState PhysicalRecovered (mutable_owners state)
    (PhysicalRecoveryCertificate :: certificates state)
    (prepare_members state) (index_members state)
    (selected_authority state) (Some NewAuthority)
    (semantic_before state) (semantic_after state)
    (semantics_are_readmitted state) (result_is_acknowledged state)
    (introduction_charge state) (delivery_charge state) (trace_charge state).

Definition readmit (state : publication_state) : publication_state :=
  PublicationState SemanticReadmitted (mutable_owners state)
    (SemanticReadmissionCertificate :: certificates state)
    (prepare_members state) (index_members state)
    (selected_authority state) (physical_authority state)
    (semantic_before state) (Some (semantic_before state))
    true (result_is_acknowledged state)
    (introduction_charge state) (delivery_charge state) (trace_charge state).

Definition acknowledge (state : publication_state) : publication_state :=
  PublicationState Acknowledged [NewWriter] (certificates state)
    (prepare_members state) (index_members state)
    (selected_authority state) (physical_authority state)
    (semantic_before state) (semantic_after state)
    (semantics_are_readmitted state) true
    (introduction_charge state) (delivery_charge state) (trace_charge state).

Inductive publication_step
    (expected : list Object) : publication_state -> publication_state -> Prop :=
| StepPreflight : forall before,
    state_phase before = OldAdmitted ->
    mutable_owners before = [OldWriter] ->
    publication_step expected before
      (transfer_and_advance Preflighted Publisher PreflightCertificate before)
| StepCleanup : forall before,
    state_phase before = Preflighted ->
    publication_step expected before
      (advance CleanupComplete CleanupCertificate before)
| StepWritePrepare : forall before,
    state_phase before = CleanupComplete ->
    publication_step expected before (write_prepare expected before)
| StepSyncPrepare : forall before,
    state_phase before = PrepareWritten ->
    prepare_members before = expected ->
    publication_step expected before
      (advance PrepareDurable PrepareSyncCertificate before)
| StepWriteIndex : forall before,
    state_phase before = PrepareDurable ->
    prepare_members before = expected ->
    publication_step expected before (write_index before)
| StepSyncIndex : forall before,
    state_phase before = IndexWritten ->
    index_members before = expected ->
    publication_step expected before
      (advance IndexDurable IndexSyncCertificate before)
| StepWriteCommit : forall before,
    state_phase before = IndexDurable ->
    publication_step expected before
      (advance CommitWritten CommitWriteCertificate before)
| StepSyncCommit : forall before,
    state_phase before = CommitWritten ->
    publication_step expected before
      (advance CommitDurable CommitSyncCertificate before)
| StepSyncArtifactDirectory : forall before,
    state_phase before = CommitDurable ->
    publication_step expected before
      (advance ArtifactDirectoryDurable ArtifactDirectoryCertificate before)
| StepVerifyCas : forall before,
    state_phase before = ArtifactDirectoryDurable ->
    publication_step expected before
      (advance CasVerified ExactCatalogCasCertificate before)
| StepStartCatalogWrite : forall before,
    state_phase before = CasVerified ->
    publication_step expected before
      (advance CatalogWriteStarted CatalogWriteStartCertificate before)
| StepCompleteCatalogWrite : forall before,
    state_phase before = CatalogWriteStarted ->
    publication_step expected before
      (advance CatalogWritten CatalogWriteCompleteCertificate before)
| StepSyncCatalogFile : forall before,
    state_phase before = CatalogWritten ->
    publication_step expected before (select_new_catalog before)
| StepSyncCatalogDirectory : forall before,
    state_phase before = CatalogFileDurable ->
    selected_authority before = NewAuthority ->
    publication_step expected before
      (advance CatalogDirectoryDurable CatalogDirectorySyncCertificate before)
| StepRecover : forall before,
    state_phase before = CatalogDirectoryDurable ->
    selected_authority before = NewAuthority ->
    publication_step expected before (recover_new before)
| StepReadmit : forall before,
    state_phase before = PhysicalRecovered ->
    physical_authority before = Some NewAuthority ->
    publication_step expected before (readmit before)
| StepAcknowledge : forall before,
    state_phase before = SemanticReadmitted ->
    semantic_after before = Some (semantic_before before) ->
    publication_step expected before (acknowledge before).

Inductive reachable
    (expected : list Object)
    (semantic : SemanticState) : publication_state -> Prop :=
| ReachInitial : reachable expected semantic (initial_publication_state semantic)
| ReachStep : forall before after,
    reachable expected semantic before ->
    publication_step expected before after ->
    reachable expected semantic after.

Theorem reachable_authority_is_affine_and_continuous :
  forall expected semantic state,
    reachable expected semantic state ->
    mutable_owners state = [OldWriter] \/
    mutable_owners state = [Publisher] \/
    mutable_owners state = [NewWriter].
Proof.
  intros expected semantic state path; induction path.
  - left; reflexivity.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge;
      simpl; auto.
Qed.

Definition expected_certificates_for_phase
    (phase : publication_phase) : list barrier_certificate :=
  match phase with
  | OldAdmitted => []
  | Preflighted => [PreflightCertificate]
  | CleanupComplete => [CleanupCertificate; PreflightCertificate]
  | PrepareWritten =>
      [PrepareWriteCertificate; CleanupCertificate; PreflightCertificate]
  | PrepareDurable =>
      [PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | IndexWritten =>
      [IndexWriteCertificate; PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | IndexDurable =>
      [IndexSyncCertificate; IndexWriteCertificate; PrepareSyncCertificate;
       PrepareWriteCertificate; CleanupCertificate; PreflightCertificate]
  | CommitWritten =>
      [CommitWriteCertificate; IndexSyncCertificate; IndexWriteCertificate;
       PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | CommitDurable =>
      [CommitSyncCertificate; CommitWriteCertificate; IndexSyncCertificate;
       IndexWriteCertificate; PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | ArtifactDirectoryDurable =>
      [ArtifactDirectoryCertificate; CommitSyncCertificate;
       CommitWriteCertificate; IndexSyncCertificate; IndexWriteCertificate;
       PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | CasVerified =>
      [ExactCatalogCasCertificate; ArtifactDirectoryCertificate;
       CommitSyncCertificate; CommitWriteCertificate; IndexSyncCertificate;
       IndexWriteCertificate; PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | CatalogWriteStarted =>
      [CatalogWriteStartCertificate; ExactCatalogCasCertificate;
       ArtifactDirectoryCertificate; CommitSyncCertificate;
       CommitWriteCertificate; IndexSyncCertificate; IndexWriteCertificate;
       PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | CatalogWritten =>
      [CatalogWriteCompleteCertificate; CatalogWriteStartCertificate;
       ExactCatalogCasCertificate; ArtifactDirectoryCertificate;
       CommitSyncCertificate; CommitWriteCertificate; IndexSyncCertificate;
       IndexWriteCertificate; PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | CatalogFileDurable =>
      [CatalogFileSyncCertificate; CatalogWriteCompleteCertificate;
       CatalogWriteStartCertificate; ExactCatalogCasCertificate;
       ArtifactDirectoryCertificate; CommitSyncCertificate;
       CommitWriteCertificate; IndexSyncCertificate; IndexWriteCertificate;
       PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | CatalogDirectoryDurable =>
      [CatalogDirectorySyncCertificate; CatalogFileSyncCertificate;
       CatalogWriteCompleteCertificate; CatalogWriteStartCertificate;
       ExactCatalogCasCertificate; ArtifactDirectoryCertificate;
       CommitSyncCertificate; CommitWriteCertificate; IndexSyncCertificate;
       IndexWriteCertificate; PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | PhysicalRecovered =>
      [PhysicalRecoveryCertificate; CatalogDirectorySyncCertificate;
       CatalogFileSyncCertificate; CatalogWriteCompleteCertificate;
       CatalogWriteStartCertificate; ExactCatalogCasCertificate;
       ArtifactDirectoryCertificate; CommitSyncCertificate;
       CommitWriteCertificate; IndexSyncCertificate; IndexWriteCertificate;
       PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | SemanticReadmitted | Acknowledged =>
      [SemanticReadmissionCertificate; PhysicalRecoveryCertificate;
       CatalogDirectorySyncCertificate; CatalogFileSyncCertificate;
       CatalogWriteCompleteCertificate; CatalogWriteStartCertificate;
       ExactCatalogCasCertificate; ArtifactDirectoryCertificate;
       CommitSyncCertificate; CommitWriteCertificate; IndexSyncCertificate;
       IndexWriteCertificate; PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  end.

Lemma reachable_certificates_are_exact :
  forall expected semantic state,
    reachable expected semantic state ->
    certificates state = expected_certificates_for_phase (state_phase state).
Proof.
  intros expected semantic state path; induction path.
  - reflexivity.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge;
      simpl in *.
    all: repeat match goal with
      | phase_guard : state_phase ?before = ?phase |- _ =>
          rewrite phase_guard in IHpath
      end.
    all: simpl in IHpath; rewrite IHpath; reflexivity.
Qed.

Lemma reachable_prepare_members_are_exact_after_write :
  forall expected semantic state,
    reachable expected semantic state ->
    In PrepareWriteCertificate (certificates state) ->
    prepare_members state = expected.
Proof.
  intros expected semantic state path; induction path; intro prepared.
  - inversion prepared.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in *;
      simpl in *.
    all: try reflexivity.
    all: apply IHpath.
    all: firstorder congruence.
Qed.

Lemma reachable_index_members_are_exact_after_write :
  forall expected semantic state,
    reachable expected semantic state ->
    In IndexWriteCertificate (certificates state) ->
    index_members state = expected.
Proof.
  intros expected semantic state path; induction path; intro indexed.
  - inversion indexed.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in *;
      simpl in *.
    all: try assumption.
    all: try reflexivity.
    all: apply IHpath.
    all: firstorder congruence.
Qed.

Lemma selected_new_has_catalog_file_certificate :
  forall expected semantic state,
    reachable expected semantic state ->
    selected_authority state = NewAuthority ->
    In CatalogFileSyncCertificate (certificates state).
Proof.
  intros expected semantic state path; induction path; intro selected.
  - discriminate.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in *;
      simpl in *; try (apply IHpath; assumption); auto.
Qed.

Lemma selected_new_has_published_phase :
  forall expected semantic state,
    reachable expected semantic state ->
    selected_authority state = NewAuthority ->
    state_phase state = CatalogFileDurable \/
    state_phase state = CatalogDirectoryDurable \/
    state_phase state = PhysicalRecovered \/
    state_phase state = SemanticReadmitted \/
    state_phase state = Acknowledged.
Proof.
  intros expected semantic state path; induction path; intro selected.
  - discriminate.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in *;
      simpl in *; try (apply IHpath in selected; firstorder congruence);
      firstorder congruence.
Qed.

Theorem selected_new_has_exact_members_and_all_durable_barriers :
  forall expected semantic state,
    reachable expected semantic state ->
    selected_authority state = NewAuthority ->
    prepare_members state = expected /\
    index_members state = expected /\
    In PrepareSyncCertificate (certificates state) /\
    In IndexSyncCertificate (certificates state) /\
    In CommitSyncCertificate (certificates state) /\
    In ArtifactDirectoryCertificate (certificates state) /\
    In ExactCatalogCasCertificate (certificates state) /\
    In CatalogWriteStartCertificate (certificates state) /\
    In CatalogWriteCompleteCertificate (certificates state) /\
    In CatalogFileSyncCertificate (certificates state).
Proof.
  intros expected semantic state path selected.
  pose proof (selected_new_has_catalog_file_certificate path selected)
    as catalog_file.
  pose proof (reachable_certificates_are_exact path) as exact_certificates.
  pose proof (reachable_prepare_members_are_exact_after_write path) as prepare_exact.
  pose proof (reachable_index_members_are_exact_after_write path) as index_exact.
  pose proof (selected_new_has_published_phase path selected) as published_phase.
  assert (prepare_written : In PrepareWriteCertificate (certificates state)).
  { rewrite exact_certificates.
    destruct published_phase as [phase | [phase | [phase | [phase | phase]]]];
      rewrite phase; simpl; tauto. }
  assert (index_written : In IndexWriteCertificate (certificates state)).
  { rewrite exact_certificates.
    destruct published_phase as [phase | [phase | [phase | [phase | phase]]]];
      rewrite phase; simpl; tauto. }
  specialize (prepare_exact prepare_written).
  specialize (index_exact index_written).
  split; [exact prepare_exact |].
  split; [exact index_exact |].
  destruct published_phase as [phase | [phase | [phase | [phase | phase]]]];
    rewrite exact_certificates, phase; simpl; repeat split;
    repeat first [left; reflexivity | right].
Qed.

Lemma physical_new_has_selected_new :
  forall expected semantic state,
    reachable expected semantic state ->
    physical_authority state = Some NewAuthority ->
    selected_authority state = NewAuthority.
Proof.
  intros expected semantic state path; induction path; intro recovered.
  - discriminate.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in *;
      simpl in *; try (apply IHpath; assumption); auto.
Qed.

Lemma physical_new_has_recovered_phase :
  forall expected semantic state,
    reachable expected semantic state ->
    physical_authority state = Some NewAuthority ->
    state_phase state = PhysicalRecovered \/
    state_phase state = SemanticReadmitted \/
    state_phase state = Acknowledged.
Proof.
  intros expected semantic state path; induction path; intro recovered.
  - discriminate.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in *;
      simpl in *; try (apply IHpath in recovered; firstorder congruence);
      firstorder congruence.
Qed.

Lemma physical_new_has_catalog_directory_certificate :
  forall expected semantic state,
    reachable expected semantic state ->
    physical_authority state = Some NewAuthority ->
    In CatalogDirectorySyncCertificate (certificates state).
Proof.
  intros expected semantic state path recovered.
  pose proof (reachable_certificates_are_exact path) as exact_certificates.
  pose proof (physical_new_has_recovered_phase path recovered) as phase.
  destruct phase as [phase | [phase | phase]];
    rewrite exact_certificates, phase; simpl;
    repeat first [left; reflexivity | right].
Qed.

Theorem physical_recovery_requires_post_catalog_directory_barrier :
  forall expected semantic state,
    reachable expected semantic state ->
    physical_authority state = Some NewAuthority ->
    selected_authority state = NewAuthority /\
    In CatalogDirectorySyncCertificate (certificates state).
Proof.
  intros expected semantic state path recovered.
  split.
  - exact (physical_new_has_selected_new path recovered).
  - exact (physical_new_has_catalog_directory_certificate path recovered).
Qed.

Lemma semantic_readmitted_phase_is_exact :
  forall expected semantic state,
    reachable expected semantic state ->
    state_phase state = SemanticReadmitted ->
    selected_authority state = NewAuthority /\
    physical_authority state = Some NewAuthority /\
    semantics_are_readmitted state = true /\
    semantic_after state = Some (semantic_before state) /\
    In CatalogDirectorySyncCertificate (certificates state).
Proof.
  intros expected semantic state path phase.
  induction path.
  - discriminate.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in phase;
      simpl in phase; try discriminate.
    match goal with
    | recovered : physical_authority ?before = Some NewAuthority |- _ =>
        pose proof
          (physical_recovery_requires_post_catalog_directory_barrier
            path recovered) as recovered_contract
    end.
    destruct recovered_contract as [selected directory].
    unfold readmit; simpl.
    split; [exact selected |].
    split; [assumption |].
    split; [reflexivity |].
    split; [reflexivity |].
    right; exact directory.
Qed.

Lemma acknowledged_has_acknowledged_phase :
  forall expected semantic state,
    reachable expected semantic state ->
    result_is_acknowledged state = true ->
    state_phase state = Acknowledged.
Proof.
  intros expected semantic state path; induction path; intro acknowledged.
  - discriminate.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in *;
      simpl in *; try (apply IHpath in acknowledged; congruence); reflexivity.
Qed.

Lemma acknowledged_phase_is_exact :
  forall expected semantic state,
    reachable expected semantic state ->
    state_phase state = Acknowledged ->
    mutable_owners state = [NewWriter] /\
    selected_authority state = NewAuthority /\
    physical_authority state = Some NewAuthority /\
    semantics_are_readmitted state = true /\
    semantic_after state = Some (semantic_before state) /\
    In CatalogDirectorySyncCertificate (certificates state).
Proof.
  intros expected semantic state path phase.
  induction path.
  - discriminate.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in phase;
      simpl in phase; try discriminate.
    match goal with
    | readmitted : state_phase ?before = SemanticReadmitted |- _ =>
        pose proof
          (semantic_readmitted_phase_is_exact path readmitted)
          as readmitted_contract
    end.
    destruct readmitted_contract as
      [selected [recovered [readmitted [same_semantics directory]]]].
    unfold acknowledge; simpl.
    repeat split; assumption || reflexivity.
Qed.

Theorem acknowledgement_requires_recovery_readmission_and_same_semantics :
  forall expected semantic state,
    reachable expected semantic state ->
    result_is_acknowledged state = true ->
    mutable_owners state = [NewWriter] /\
    selected_authority state = NewAuthority /\
    physical_authority state = Some NewAuthority /\
    semantics_are_readmitted state = true /\
    semantic_after state = Some (semantic_before state) /\
    In CatalogDirectorySyncCertificate (certificates state).
Proof.
  intros expected semantic state path acknowledged.
  apply acknowledged_phase_is_exact with
    (expected := expected) (semantic := semantic).
  - exact path.
  - exact (acknowledged_has_acknowledged_phase path acknowledged).
Qed.

Theorem checkpoint_publication_preserves_zero_semantic_charges :
  forall expected semantic state,
    reachable expected semantic state ->
    introduction_charge state = 0 /\
    delivery_charge state = 0 /\
    trace_charge state = 0.
Proof.
  intros expected semantic state path; induction path.
  - repeat split; reflexivity.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge;
      simpl; assumption.
Qed.

End AffinePublicationProtocol.

Print Assumptions successful_merge_is_exact_permutation.
Print Assumptions successful_merge_adds_exactly_one_member.
Print Assumptions successful_merge_adds_exactly_declared_bytes.
Print Assumptions successful_merge_preserves_strict_digest_order.
Print Assumptions existing_digest_is_rejected.
Print Assumptions refined_descriptor_carries_exact_six_inventories.
Print Assumptions refined_descriptor_has_exact_wire_layout.
Print Assumptions raw_slot_recovery_selects_old_complete_new_or_rejects.
Print Assumptions reachable_authority_is_affine_and_continuous.
Print Assumptions selected_new_has_exact_members_and_all_durable_barriers.
Print Assumptions physical_recovery_requires_post_catalog_directory_barrier.
Print Assumptions acknowledgement_requires_recovery_readmission_and_same_semantics.
Print Assumptions checkpoint_publication_preserves_zero_semantic_charges.
