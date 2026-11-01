From Stdlib Require Import Bool List Arith ZArith.
From StackSafePDA Require Import PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority RhoPathV1Datum TwoLevelRSpaceContinuationAuthority
  TwoLevelRSpaceReceiveAccelerator.
Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceContinuationAuthority.
Import OrderedReceivePositions.
Import TwoLevelRSpaceReceiveAccelerator.

Open Scope nat_scope.

Module TwoLevelRSpaceReceiveAcceleratorMutants.

Record Switches : Type := SwitchesState {
  switch_hash_only_receive_id : bool;
  switch_ordinal_receive_id : bool;
  switch_absent_as_empty : bool;
  switch_stale_snapshot_answer : bool;
  switch_foreign_accelerator_adoption : bool;
  switch_incomplete_scope_answer : bool;
  switch_collapsed_repeated_positions : bool;
  switch_installed_resting_collapse : bool;
  switch_order_sensitive_derivation : bool;
  switch_split_artifact_publication : bool;
  switch_failed_update_leak : bool;
  switch_accelerator_in_semantic_identity : bool;
  switch_last_source_wins : bool;
  switch_unchecked_candidate_commit : bool;
  switch_metadata_erasing_target : bool
}.

Definition switches_with (flipped : nat) : Switches :=
  SwitchesState (Nat.eqb flipped 1) (Nat.eqb flipped 2) (Nat.eqb flipped 3)
    (Nat.eqb flipped 4) (Nat.eqb flipped 5) (Nat.eqb flipped 6)
    (Nat.eqb flipped 7) (Nat.eqb flipped 8) (Nat.eqb flipped 9)
    (Nat.eqb flipped 10) (Nat.eqb flipped 11) (Nat.eqb flipped 12)
    (Nat.eqb flipped 13) (Nat.eqb flipped 14) (Nat.eqb flipped 15).

Definition correct_switches : Switches := switches_with 0.

Definition switch_vector (switches : Switches) : list bool :=
  [switch_hash_only_receive_id switches; switch_ordinal_receive_id switches;
   switch_absent_as_empty switches; switch_stale_snapshot_answer switches;
   switch_foreign_accelerator_adoption switches;
   switch_incomplete_scope_answer switches;
   switch_collapsed_repeated_positions switches;
   switch_installed_resting_collapse switches;
   switch_order_sensitive_derivation switches;
   switch_split_artifact_publication switches;
   switch_failed_update_leak switches;
   switch_accelerator_in_semantic_identity switches;
   switch_last_source_wins switches;
   switch_unchecked_candidate_commit switches;
   switch_metadata_erasing_target switches].

Definition answer_with
    (classify : World -> AcceleratorState -> AcceleratorClass)
    (derive absent : World -> Path -> list Occurrence)
    (w : World) (accelerator : AcceleratorState) (channel : Path) : Answer :=
  match accelerator with
  | AcceleratorAbsent => FallbackAnswer (absent w channel)
  | AcceleratorBuilding _ _ _ => FallbackAnswer (derive w channel)
  | AcceleratorPublished _ _ _ entries _ =>
      match classify w accelerator with
      | ClassCoherent => ExactAnswer (accelerator_lookup entries channel)
      | _ => FallbackAnswer (derive w channel)
      end
  end.

Definition read_with
    (classify : World -> AcceleratorState -> AcceleratorClass)
    (derive absent : World -> Path -> list Occurrence)
    (w : World) (accelerator : AcceleratorState) (channel : Path)
    : list Occurrence :=
  answer_occurrences (answer_with classify derive absent w accelerator channel).

Definition empty_read (w : World) (channel : Path) : list Occurrence := [].

Definition classify_ignoring_revision (w : World)
    (accelerator : AcceleratorState) : AcceleratorClass :=
  match accelerator with
  | AcceleratorAbsent => ClassAbsent
  | AcceleratorBuilding _ _ _ => ClassBuilding
  | AcceleratorPublished bound lineage scope _ artifacts =>
      if semantic_roots_eq_dec lineage (world_manifest w) then
        if EqualityEpoch.eq_dec (revision_generation bound)
            (revision_generation (world_revision w)) then
          if artifacts_eq_dec artifacts (artifacts_at bound) then
            if coversb scope w then ClassCoherent else ClassIncomplete
          else ClassStale
        else ClassForeign
      else ClassForeign
  end.

Definition classify_ignoring_scope (w : World)
    (accelerator : AcceleratorState) : AcceleratorClass :=
  match accelerator with
  | AcceleratorAbsent => ClassAbsent
  | AcceleratorBuilding _ _ _ => ClassBuilding
  | AcceleratorPublished bound lineage _ _ artifacts =>
      if semantic_roots_eq_dec lineage (world_manifest w) then
        if EqualityEpoch.eq_dec (revision_generation bound)
            (revision_generation (world_revision w)) then
          if revision_eq_dec bound (world_revision w) then
            if artifacts_eq_dec artifacts (artifacts_at bound) then ClassCoherent
            else ClassStale
          else ClassStale
        else ClassForeign
      else ClassForeign
  end.

Definition collapsed_derive (w : World) (channel : Path) : list Occurrence :=
  flat_map (fun entry =>
    map (OccurrenceState channel (fst entry) (firstn 1 (snd entry)))
      (group_targets (world_roots w) (world_installed w) (fst entry)))
    (channel_groups (world_roots w) (world_installed w) channel).

Definition kind_erased_derive (w : World) (channel : Path) : list Occurrence :=
  flat_map (fun entry =>
    map (fun target => OccurrenceState channel (fst entry) (snd entry)
      (ReceiveTargetState RestingKind (receive_target_group target)
        (receive_target_record target)))
      (group_targets (world_roots w) (world_installed w) (fst entry)))
    (channel_groups (world_roots w) (world_installed w) channel).

Definition first_group_derive (w : World) (channel : Path) : list Occurrence :=
  flat_map (fun entry =>
    map (OccurrenceState channel (fst entry) (snd entry))
      (group_targets (world_roots w) (world_installed w) (fst entry)))
    (firstn 1 (channel_groups (world_roots w) (world_installed w) channel)).

Definition impl_read (switches : Switches) (w : World)
    (accelerator : AcceleratorState) (channel : Path) : list Occurrence :=
  if switch_absent_as_empty switches then
    read_with classify_accelerator derive_world empty_read w accelerator channel
  else if switch_stale_snapshot_answer switches then
    read_with classify_ignoring_revision derive_world derive_world w accelerator
      channel
  else if switch_incomplete_scope_answer switches then
    read_with classify_ignoring_scope derive_world derive_world w accelerator
      channel
  else if switch_collapsed_repeated_positions switches then
    read_with classify_accelerator collapsed_derive collapsed_derive w accelerator
      channel
  else if switch_installed_resting_collapse switches then
    read_with classify_accelerator kind_erased_derive kind_erased_derive w
      accelerator channel
  else if switch_order_sensitive_derivation switches then
    read_with classify_accelerator first_group_derive first_group_derive w
      accelerator channel
  else accelerator_read w accelerator channel.

Definition issue_receive_id (issued_at : World) (target : ReceiveTarget)
    : ReceiveId :=
  ReceiveIdState (world_revision issued_at) target.

Definition resolve_by (select : ContinuationRecord -> ContinuationRecord -> bool)
    (w : World) (id : ReceiveId) : option ContinuationRecord :=
  if revision_eq_dec (receive_id_revision id) (world_revision w)
  then find (select (receive_target_record (receive_id_target id)))
    (live_records w (receive_id_target id))
  else None.

Definition bucket_matches (expected record : ContinuationRecord) : bool :=
  Nat.eqb (length (continuation_identity record))
    (length (continuation_identity expected)).

Definition bytes_match (expected record : ContinuationRecord) : bool :=
  if path_eq_dec (continuation_identity record) (continuation_identity expected)
  then
    if path_eq_dec (continuation_source record) (continuation_source expected)
    then true else false
  else false.

Definition ordinal_resolve (issued_at w : World) (target : ReceiveTarget)
    : option ContinuationRecord :=
  nth_error (live_records w target)
    (record_ordinal (receive_target_record target)
      (live_records issued_at target)).

Definition impl_resolve (switches : Switches) (issued_at w : World)
    (target : ReceiveTarget) : option ContinuationRecord :=
  if switch_hash_only_receive_id switches then
    resolve_by bucket_matches w (issue_receive_id issued_at target)
  else if switch_ordinal_receive_id switches then
    ordinal_resolve issued_at w target
  else if switch_metadata_erasing_target switches then
    resolve_by bytes_match w (issue_receive_id issued_at target)
  else resolve_receive w (issue_receive_id issued_at target).

Inductive CommitRequest : Type :=
| CommitOperation (operation : Operation) (w : World)
    (accelerator : AcceleratorState) (fault : bool)
| CommitCandidate (observed current : World) (target : ReceiveTarget).

Inductive CommitResult : Type :=
| OperationCommitted (w : World) (accelerator : AcceleratorState)
| CandidateCommitted (resolved : option ResolvedTarget).

Definition commit_with (fault_world step : Operation -> World -> World)
    (update : Operation -> World -> AcceleratorState -> AcceleratorState)
    (candidate : World -> World -> ReceiveTarget -> option ResolvedTarget)
    (request : CommitRequest) : CommitResult :=
  match request with
  | CommitOperation operation w accelerator fault =>
      OperationCommitted
        (fst (if fault then (fault_world operation w, accelerator)
              else (step operation w, update operation w accelerator)))
        (snd (if fault then (fault_world operation w, accelerator)
              else (step operation w, update operation w accelerator)))
  | CommitCandidate observed current target =>
      CandidateCommitted (candidate observed current target)
  end.

Definition keep_world (operation : Operation) (w : World) : World := w.

Definition model_commit (request : CommitRequest) : CommitResult :=
  commit_with keep_world step_world update_accelerator candidate_commit request.

Definition adopting_update (operation : Operation) (w : World)
    (accelerator : AcceleratorState) : AcceleratorState :=
  match operation_effect operation w with
  | Replaced target slots (Some stored) =>
      publish_at (step_effect (Replaced target slots (Some stored)) w)
        (persisted_scope stored ++ map fst slots)
        (persisted_entries stored ++ installed_entries slots)
  | effect => update_effect effect w accelerator
  end.

Definition split_publish_at (previous : Revision) (w : World)
    (scope : list GroupKey) (entries : list Occurrence) : AcceleratorState :=
  AcceleratorPublished (world_revision w) (world_manifest w) scope entries
    (ArtifactsState (world_revision w) (world_revision w) previous).

Definition split_update (operation : Operation) (w : World)
    (accelerator : AcceleratorState) : AcceleratorState :=
  match operation_effect operation w with
  | NoEffect => accelerator
  | Replaced target slots persisted =>
      update_effect (Replaced target slots persisted) w accelerator
  | effect =>
      match accelerator with
      | AcceleratorPublished bound lineage scope entries artifacts =>
          if bound_is_current w bound lineage artifacts then
            split_publish_at (world_revision w) (step_effect effect w)
              (fst (maintained_view effect w scope entries))
              (snd (maintained_view effect w scope entries))
          else accelerator
      | _ => accelerator
      end
  end.

Definition leaked_world (operation : Operation) (w : World) : World :=
  WorldState (world_roots (step_world operation w))
    (world_installed (step_world operation w)) (world_revision w)
    (world_issued w) (world_manifest w) (world_base_epoch w)
    (world_group_epochs w).

Definition alias_replaced_records (record : ContinuationRecord)
    (records : list ContinuationRecord) : list ContinuationRecord :=
  map (fun existing =>
    if path_eq_dec (continuation_identity existing)
        (continuation_identity record)
    then record else existing) records.

Definition alias_replacing_step (operation : Operation) (w : World) : World :=
  match operation with
  | InsertResting group record =>
      match first_identity (continuation_identity record)
          (group_records (world_roots w) group) with
      | Some _ =>
          bump_group w
            (SemanticRootsState (root_data (world_roots w))
              (assoc_set group_eq_dec (root_continuations (world_roots w)) group
                (alias_replaced_records record
                  (group_records (world_roots w) group)))
              (root_joins (world_roots w)))
            (world_installed w) group false
      | None => step_world operation w
      end
  | _ => step_world operation w
  end.

Definition unchecked_candidate (observed current : World) (target : ReceiveTarget)
    : option ResolvedTarget :=
  resolve_exact_continuation (authority_of observed) (authority_of observed)
    (receive_target_group target) (candidate_capability observed target)
    (expectation_of_record (receive_target_record target)).

Definition impl_commit (switches : Switches) (request : CommitRequest)
    : CommitResult :=
  if switch_foreign_accelerator_adoption switches then
    commit_with keep_world step_world adopting_update candidate_commit request
  else if switch_split_artifact_publication switches then
    commit_with keep_world step_world split_update candidate_commit request
  else if switch_failed_update_leak switches then
    commit_with leaked_world step_world update_accelerator candidate_commit
      request
  else if switch_last_source_wins switches then
    commit_with keep_world alias_replacing_step update_accelerator
      candidate_commit request
  else if switch_unchecked_candidate_commit switches then
    commit_with keep_world step_world update_accelerator unchecked_candidate
      request
  else model_commit request.

Record CheckpointImage : Type := CheckpointImageState {
  image_roots : SemanticRoots;
  image_installed : InstalledSlots;
  image_index : list Occurrence
}.

Definition accelerator_entries (accelerator : AcceleratorState)
    : list Occurrence :=
  match accelerator with
  | AcceleratorAbsent => []
  | AcceleratorBuilding _ _ entries => entries
  | AcceleratorPublished _ _ _ entries _ => entries
  end.

Definition model_identity (w : World) (accelerator : AcceleratorState)
    : CheckpointImage :=
  CheckpointImageState (world_roots w) (world_installed w) [].

Definition polluted_identity (w : World) (accelerator : AcceleratorState)
    : CheckpointImage :=
  CheckpointImageState (world_roots w) (world_installed w)
    (accelerator_entries accelerator).

Definition impl_identity (switches : Switches) (w : World)
    (accelerator : AcceleratorState) : CheckpointImage :=
  if switch_accelerator_in_semantic_identity switches
  then polluted_identity w accelerator
  else model_identity w accelerator.

Theorem implementation_with_correct_switches_is_the_model :
  switch_vector correct_switches = repeat false 15 /\
  map (fun flipped =>
    count_occ Bool.bool_dec (switch_vector (switches_with flipped)) true)
    (seq 1 15) = repeat 1 15 /\
  map (fun flipped =>
    nth (pred flipped) (switch_vector (switches_with flipped)) false)
    (seq 1 15) = repeat true 15 /\
  (forall w accelerator channel,
     impl_read correct_switches w accelerator channel =
     accelerator_read w accelerator channel) /\
  (forall w accelerator channel,
     read_with classify_accelerator derive_world derive_world w accelerator
       channel = accelerator_read w accelerator channel) /\
  (forall issued_at w target,
     impl_resolve correct_switches issued_at w target =
     resolve_receive w (issue_receive_id issued_at target)) /\
  (forall w id, resolve_by record_is w id = resolve_receive w id) /\
  (forall request, impl_commit correct_switches request = model_commit request) /\
  (forall operation w accelerator fault,
     model_commit (CommitOperation operation w accelerator fault) =
     OperationCommitted (fst (commit_operation operation w accelerator fault))
       (snd (commit_operation operation w accelerator fault))) /\
  (forall observed current target,
     model_commit (CommitCandidate observed current target) =
     CandidateCommitted (candidate_commit observed current target)) /\
  (forall w accelerator,
     impl_identity correct_switches w accelerator = model_identity w accelerator) /\
  (forall w left right, model_identity w left = model_identity w right).
Proof.
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  repeat split; intros; reflexivity.
Qed.

Definition channel_a : Path := [1].
Definition channel_b : Path := [1; 0].
Definition group_aba : GroupKey := [channel_a; channel_b; channel_a].
Definition group_b : GroupKey := [channel_b].

Definition record_x1 : ContinuationRecord :=
  ContinuationRecordState [11] [21] false [].
Definition record_x2 : ContinuationRecord :=
  ContinuationRecordState [12] [22] false [].
Definition record_x3 : ContinuationRecord :=
  ContinuationRecordState [13] [23] false [].
Definition record_y : ContinuationRecord :=
  ContinuationRecordState [11] [21] false [].
Definition record_alias : ContinuationRecord :=
  ContinuationRecordState [11] [24] false [].
Definition record_new : ContinuationRecord :=
  ContinuationRecordState [15] [25] false [].
Definition record_peeked : ContinuationRecord :=
  ContinuationRecordState [14] [26] false [5%Z].
Definition record_unpeeked : ContinuationRecord :=
  ContinuationRecordState [14] [26] false [].

Definition roots_with
    (continuations : list (GroupKey * list ContinuationRecord))
    : SemanticRoots :=
  SemanticRootsState [] continuations
    [(channel_a, [group_aba]); (channel_b, [group_aba; group_b])].

Definition initial_revision : Revision :=
  RevisionState (EqualityEpoch.Token 1) (EqualityEpoch.Token 1)
    (EqualityEpoch.Token 1).

Definition world_at (roots : SemanticRoots) (slots : InstalledSlots) : World :=
  WorldState roots slots initial_revision [EqualityEpoch.Token 1] roots
    (EqualityEpoch.Token 1) [].

Definition base_roots : SemanticRoots :=
  roots_with [(group_aba, [record_x1; record_x2]); (group_b, [record_x3])].
Definition base_world : World := world_at base_roots [].
Definition base_accelerator : AcceleratorState := build_accelerator base_world.

Definition target_x1 : ReceiveTarget := resting_target group_aba record_x1.
Definition target_unpeeked : ReceiveTarget :=
  resting_target group_aba record_unpeeked.

Definition collision_world : World :=
  world_at
    (roots_with [(group_aba, [record_x2; record_x1]); (group_b, [record_x3])])
    [].
Definition removed_world : World :=
  step_world (RemoveResting group_aba record_x1) base_world.
Definition inserted_world : World :=
  step_world (InsertResting group_aba record_new) base_world.
Definition installed_world : World :=
  world_at (roots_with [(group_aba, [record_x1])]) [(group_aba, record_y)].
Definition left_world : World :=
  world_at (roots_with [(group_aba, [record_x1]); (group_b, [record_x3])]) [].
Definition right_world : World :=
  world_at (roots_with [(group_b, [record_x3]); (group_aba, [record_x1])]) [].
Definition peek_world : World :=
  world_at (roots_with [(group_aba, [record_peeked])]) [].

Definition foreign_roots : SemanticRoots :=
  roots_with [(group_b, [record_x3])].
Definition foreign_persisted : PersistedAccelerator :=
  PersistedAcceleratorState foreign_roots [group_aba; group_b]
    (scoped_entries_of foreign_roots [] [group_aba; group_b]).
Definition partial_accelerator : AcceleratorState :=
  publish_at base_world [group_aba] (scoped_entries base_world [group_aba]).

Definition new_occurrence : Occurrence :=
  OccurrenceState channel_a group_aba [0; 2]
    (resting_target group_aba record_new).
Definition occurrence_x3 : Occurrence :=
  OccurrenceState channel_b group_b [0] (resting_target group_b record_x3).

Definition foreign_reset_request : CommitRequest :=
  CommitOperation (ResetState base_roots [] (Some foreign_persisted)) base_world
    AcceleratorAbsent false.
Definition split_request : CommitRequest :=
  CommitOperation (InsertResting group_aba record_new) base_world
    base_accelerator false.
Definition leak_request : CommitRequest :=
  CommitOperation (InsertResting group_aba record_new) base_world
    base_accelerator true.
Definition alias_request : CommitRequest :=
  CommitOperation (InsertResting group_aba record_alias) base_world
    AcceleratorAbsent false.
Definition candidate_request : CommitRequest :=
  CommitCandidate base_world removed_world target_x1.

Definition occurrence_eq_dec (left right : Occurrence)
    : {left = right} + {left <> right}.
Proof.
  destruct left as [left_channel left_group left_positions left_target].
  destruct right as [right_channel right_group right_positions right_target].
  destruct (path_eq_dec left_channel right_channel) as [Hchannel|Hchannel];
    [subst right_channel|right; congruence].
  destruct (group_eq_dec left_group right_group) as [Hgroup|Hgroup];
    [subst right_group|right; congruence].
  destruct (list_eq_dec Nat.eq_dec left_positions right_positions)
    as [Hpositions|Hpositions];
    [subst right_positions|right; congruence].
  destruct (receive_target_eq_dec left_target right_target)
    as [Htarget|Htarget];
    [subst right_target; left; reflexivity|right; congruence].
Defined.

Definition has_occurrence (expected : Occurrence)
    (occurrences : list Occurrence) : bool :=
  existsb (fun occurrence =>
    if occurrence_eq_dec occurrence expected then true else false) occurrences.

Definition distinct_count (occurrences : list Occurrence) : nat :=
  length (nodup occurrence_eq_dec occurrences).

Definition read_after (result : CommitResult) (channel : Path)
    : list Occurrence :=
  match result with
  | OperationCommitted w accelerator => accelerator_read w accelerator channel
  | CandidateCommitted _ => []
  end.

Definition derive_after (result : CommitResult) (channel : Path)
    : list Occurrence :=
  match result with
  | OperationCommitted w _ => derive_world w channel
  | CandidateCommitted _ => []
  end.

Definition class_after (result : CommitResult) : option AcceleratorClass :=
  match result with
  | OperationCommitted w accelerator => Some (classify_accelerator w accelerator)
  | CandidateCommitted _ => None
  end.

Definition records_after (result : CommitResult) (group : GroupKey)
    : list ContinuationRecord :=
  match result with
  | OperationCommitted w _ => group_records (world_roots w) group
  | CandidateCommitted _ => []
  end.

Definition roots_unchanged (w : World) (result : CommitResult) : bool :=
  match result with
  | OperationCommitted committed _ =>
      if semantic_roots_eq_dec (world_roots committed) (world_roots w)
      then true else false
  | CandidateCommitted _ => false
  end.

Definition artifacts_agree (result : CommitResult) : bool :=
  match result with
  | OperationCommitted _ (AcceleratorPublished _ _ _ _ artifacts) =>
      (if revision_eq_dec (catalog_revision artifacts)
          (occurrence_revision artifacts) then true else false) &&
      (if revision_eq_dec (occurrence_revision artifacts)
          (automaton_revision artifacts) then true else false)
  | _ => false
  end.

Definition committed_record (result : CommitResult) : option ContinuationRecord :=
  match result with
  | CandidateCommitted resolved => option_map target_record resolved
  | OperationCommitted _ _ => None
  end.

Example hash_only_receive_id_is_refuted :
  impl_resolve correct_switches collision_world collision_world target_x1 =
    Some record_x1 /\
  impl_resolve (switches_with 1) collision_world collision_world target_x1 =
    Some record_x2.
Proof. vm_compute. split; reflexivity. Qed.

Example ordinal_receive_id_is_refuted :
  impl_resolve correct_switches base_world removed_world target_x1 = None /\
  impl_resolve (switches_with 2) base_world removed_world target_x1 =
    Some record_x2.
Proof. vm_compute. split; reflexivity. Qed.

Example absent_as_empty_is_refuted :
  impl_read correct_switches base_world AcceleratorAbsent channel_a =
    derive_world base_world channel_a /\
  length (derive_world base_world channel_a) = 2 /\
  impl_read (switches_with 3) base_world AcceleratorAbsent channel_a = [].
Proof. vm_compute. split; [reflexivity|split; reflexivity]. Qed.

Example stale_snapshot_answer_is_refuted :
  has_occurrence new_occurrence
    (impl_read correct_switches inserted_world base_accelerator channel_a) =
    true /\
  has_occurrence new_occurrence
    (impl_read (switches_with 4) inserted_world base_accelerator channel_a) =
    false.
Proof. vm_compute. split; reflexivity. Qed.

Example foreign_accelerator_adoption_is_refuted :
  read_after (impl_commit correct_switches foreign_reset_request) channel_a =
    derive_after (impl_commit correct_switches foreign_reset_request)
      channel_a /\
  length (derive_after (impl_commit (switches_with 5) foreign_reset_request)
    channel_a) = 2 /\
  class_after (impl_commit (switches_with 5) foreign_reset_request) =
    Some ClassCoherent /\
  read_after (impl_commit (switches_with 5) foreign_reset_request) channel_a =
    [].
Proof.
  vm_compute. split; [reflexivity|split; [reflexivity|split; reflexivity]].
Qed.

Example incomplete_scope_answer_is_refuted :
  has_occurrence occurrence_x3
    (impl_read correct_switches base_world partial_accelerator channel_b) =
    true /\
  has_occurrence occurrence_x3
    (impl_read (switches_with 6) base_world partial_accelerator channel_b) =
    false.
Proof. vm_compute. split; reflexivity. Qed.

Example collapsed_repeated_positions_are_refuted :
  map occurrence_positions
    (impl_read correct_switches base_world AcceleratorAbsent channel_a) =
    [[0; 2]; [0; 2]] /\
  map occurrence_positions
    (impl_read (switches_with 7) base_world AcceleratorAbsent channel_a) =
    [[0]; [0]].
Proof. vm_compute. split; reflexivity. Qed.

Example installed_resting_collapse_is_refuted :
  distinct_count
    (impl_read correct_switches installed_world AcceleratorAbsent channel_a) =
    2 /\
  distinct_count
    (impl_read (switches_with 8) installed_world AcceleratorAbsent channel_a) =
    1.
Proof. vm_compute. split; reflexivity. Qed.

Example order_sensitive_derivation_is_refuted :
  map occurrence_group
    (impl_read correct_switches left_world AcceleratorAbsent channel_b) =
    [group_aba; group_b] /\
  map occurrence_group
    (impl_read correct_switches right_world AcceleratorAbsent channel_b) =
    [group_b; group_aba] /\
  map occurrence_group
    (impl_read (switches_with 9) left_world AcceleratorAbsent channel_b) =
    [group_aba] /\
  map occurrence_group
    (impl_read (switches_with 9) right_world AcceleratorAbsent channel_b) =
    [group_b].
Proof.
  vm_compute. split; [reflexivity|split; [reflexivity|split; reflexivity]].
Qed.

Example split_artifact_publication_is_refuted :
  artifacts_agree (impl_commit correct_switches split_request) = true /\
  artifacts_agree (impl_commit (switches_with 10) split_request) = false.
Proof. vm_compute. split; reflexivity. Qed.

Example failed_update_leak_is_refuted :
  roots_unchanged base_world (impl_commit correct_switches leak_request) =
    true /\
  roots_unchanged base_world (impl_commit (switches_with 11) leak_request) =
    false /\
  class_after (impl_commit (switches_with 11) leak_request) =
    Some ClassCoherent /\
  has_occurrence new_occurrence
    (derive_after (impl_commit (switches_with 11) leak_request) channel_a) =
    true /\
  has_occurrence new_occurrence
    (read_after (impl_commit (switches_with 11) leak_request) channel_a) =
    false.
Proof.
  vm_compute.
  split; [reflexivity|split; [reflexivity|split; [reflexivity|split; reflexivity]]].
Qed.

Example accelerator_in_semantic_identity_is_refuted :
  impl_identity correct_switches base_world AcceleratorAbsent =
    impl_identity correct_switches base_world base_accelerator /\
  impl_identity (switches_with 12) base_world AcceleratorAbsent <>
    impl_identity (switches_with 12) base_world base_accelerator.
Proof. vm_compute. split; [reflexivity|discriminate]. Qed.

Example last_source_wins_is_refuted :
  records_after (impl_commit correct_switches alias_request) group_aba =
    [record_x1; record_x2] /\
  records_after (impl_commit (switches_with 13) alias_request) group_aba =
    [record_alias; record_x2].
Proof. vm_compute. split; reflexivity. Qed.

Example unchecked_candidate_commit_is_refuted :
  existsb (record_is record_x1)
    (group_records (world_roots removed_world) group_aba) = false /\
  committed_record (impl_commit correct_switches candidate_request) = None /\
  committed_record (impl_commit (switches_with 14) candidate_request) =
    Some record_x1.
Proof. vm_compute. split; [reflexivity|split; reflexivity]. Qed.

Example metadata_erasing_target_is_refuted :
  impl_resolve correct_switches peek_world peek_world target_unpeeked = None /\
  impl_resolve (switches_with 15) peek_world peek_world target_unpeeked =
    Some record_peeked.
Proof. vm_compute. split; reflexivity. Qed.

Print Assumptions implementation_with_correct_switches_is_the_model.
Print Assumptions hash_only_receive_id_is_refuted.
Print Assumptions ordinal_receive_id_is_refuted.
Print Assumptions absent_as_empty_is_refuted.
Print Assumptions stale_snapshot_answer_is_refuted.
Print Assumptions foreign_accelerator_adoption_is_refuted.
Print Assumptions incomplete_scope_answer_is_refuted.
Print Assumptions collapsed_repeated_positions_are_refuted.
Print Assumptions installed_resting_collapse_is_refuted.
Print Assumptions order_sensitive_derivation_is_refuted.
Print Assumptions split_artifact_publication_is_refuted.
Print Assumptions failed_update_leak_is_refuted.
Print Assumptions accelerator_in_semantic_identity_is_refuted.
Print Assumptions last_source_wins_is_refuted.
Print Assumptions unchecked_candidate_commit_is_refuted.
Print Assumptions metadata_erasing_target_is_refuted.

End TwoLevelRSpaceReceiveAcceleratorMutants.
