From Stdlib Require Import Bool List Arith NArith ZArith.
From StackSafePDA Require Import
  StackSafePDA
  SpatialMatcher
  PathMapCanonicalStreamPublicationBase
  PathMapCanonicalStreamPublication
  TwoLevelRSpaceAuthority
  TwoLevelRSpaceOperations
  TwoLevelRSpaceDelta
  TwoLevelRSpaceMatchPreparation
  TwoLevelRSpaceContinuationAuthority
  TwoLevelRSpaceMatchTransaction.

Import ListNotations.
Import PathMapCanonicalStreamPublication.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceOperations.
Import TwoLevelRSpaceDelta.
Import TwoLevelRSpaceMatchPreparation.
Import TwoLevelRSpaceContinuationAuthority.
Import TwoLevelRSpaceMatchTransaction.
Set Implicit Arguments.
Local Open Scope nat_scope.

(** Exact End-to-end publication authority for the live two-level RSpace
    interfaces.  This unit deliberately does not depend on the retired
    publication block in [TwoLevelRSpaceMatchTransaction]. *)
Module TwoLevelRSpaceCurrentPublication.

Inductive CurrentContinuationUse : Type :=
| CurrentImmediateContinuation (record : ContinuationRecord)
| CurrentStoredContinuation
    (observed : ContinuationAuthority)
    (capability : ContinuationCapability)
    (expected : ContinuationExpectation).

Record CurrentAcceptedProposal : Type := CurrentAcceptedProposalState {
  current_proposal_search : CandidateSearch;
  current_proposal_search_is_well_formed :
    WellFormedCandidateSearch current_proposal_search;
  current_proposal_acceptance :
    CertifiedSearchAcceptance current_proposal_search;
  current_proposal_continuation_use : CurrentContinuationUse
}.

Definition current_proposal_selected
    (proposal : CurrentAcceptedProposal) : list MatchCandidate :=
  selected_ordinal_values
    (accepted_selected_ordinals (current_proposal_acceptance proposal)).

Definition current_proposal_channels
    (proposal : CurrentAcceptedProposal) : list Path :=
  selected_ordinal_channels
    (accepted_selected_ordinals (current_proposal_acceptance proposal)).

Theorem current_accepted_proposal_preserves_requested_channel_vector : forall proposal,
  current_proposal_channels proposal =
  search_requested_channels (current_proposal_search proposal).
Proof.
  intros proposal. unfold current_proposal_channels.
  apply certified_acceptance_has_exact_channel_vector.
Qed.

Definition exact_preparation_input
    (proposal : CurrentAcceptedProposal)
    (input : MatchPreparationInput)
    (changes : PreparedMatchChanges) : Prop :=
  preparation_codec input =
      well_formed_search_codec
        (current_proposal_search_is_well_formed proposal) /\
  preparation_observed_authority input =
      search_observed_authority (current_proposal_search proposal) /\
  preparation_requested_channels input =
      search_requested_channels (current_proposal_search proposal) /\
  preparation_fresh_available input =
      search_fresh_available (current_proposal_search proposal) /\
  preparation_selected_candidates input =
      current_proposal_selected proposal /\
  prepare_match_changes input = PreparationReady changes /\
  pool_observed_authority
      (checked_pool_value (prepared_checked_pool changes)) =
      search_observed_authority (current_proposal_search proposal) /\
  pool_catalogs (checked_pool_value (prepared_checked_pool changes)) =
      search_catalogs (current_proposal_search proposal) /\
  pool_fresh_arrival (checked_pool_value (prepared_checked_pool changes)) =
      search_arrival (current_proposal_search proposal) /\
  pool_fresh_available (checked_pool_value (prepared_checked_pool changes)) =
      search_fresh_available (current_proposal_search proposal) /\
  checked_pool_selection (prepared_checked_pool changes) =
      current_proposal_selected proposal /\
  (forall absolute,
    search_excluded_count (current_proposal_search proposal)
      (StoredReservation absolute) =
    flat_lookup absolute (preparation_replay_exclusions input)).

Definition bag_delta_exactly_refines_prepared_flat
    (flat : FlatBagDelta) (delta : BagDelta) : Prop :=
  bag_delta_removal_mode delta = StrictSubtract /\
  bag_delta_subtractions delta = flat_delta_removals flat /\
  bag_delta_deletions delta = [] /\
  bag_delta_additions delta = flat_delta_additions flat.

Definition exact_prepared_channel_refinement {before}
    (flat : PartitionedChannelDelta)
    (channel : PreparedChannelDelta before) : Prop :=
  partitioned_channel_marker flat = prepared_delta_channel channel /\
  bag_delta_exactly_refines_prepared_flat
    (partitioned_channel_delta flat) (prepared_delta_change channel).

Record ExactDatumSuccessor
    (before : TwoLevelAuthority) (changes : PreparedMatchChanges) : Type :=
  ExactDatumSuccessorState {
    exact_datum_delta : PreparedRSpaceDelta before;
    exact_datum_partition_refinement :
      Forall2 exact_prepared_channel_refinement
        (prepared_partitioned_delta changes)
        (prepared_channel_deltas exact_datum_delta)
  }.

Definition exact_datum_after {before changes}
    (successor : ExactDatumSuccessor before changes) : TwoLevelAuthority :=
  apply_prepared_rspace_delta (exact_datum_delta successor).

Definition execute_current_continuation
    (before : ContinuationAuthority) (channels : list Path)
    (use : CurrentContinuationUse)
    : option (ContinuationAuthority * ContinuationRecord) :=
  match use with
  | CurrentImmediateContinuation record => Some (before, record)
  | CurrentStoredContinuation observed capability expected =>
      match resolve_exact_continuation
          observed before channels capability expected with
      | None => None
      | Some target =>
          match commit_exact_continuation
              observed before channels capability expected channels with
          | None => None
          | Some after => Some (after, target_record target)
          end
      end
  end.

Record ExactContinuationSuccessor
    (before : ContinuationAuthority) (channels : list Path)
    (use : CurrentContinuationUse) : Type := ExactContinuationSuccessorState {
  exact_continuation_after : ContinuationAuthority;
  exact_continuation_record : ContinuationRecord;
  exact_continuation_execution :
    execute_current_continuation before channels use =
      Some (exact_continuation_after, exact_continuation_record);
  exact_continuation_record_boundary_valid :
    continuation_record_boundary_validb exact_continuation_record = true;
  exact_continuation_changed : bool;
  exact_continuation_changed_is_exact :
    exact_continuation_changed = true ->
    exact_continuation_after <> before;
  exact_continuation_unchanged_is_exact :
    exact_continuation_changed = false ->
    exact_continuation_after = before
}.

Definition CurrentCatalogCache : Type := Path -> option CandidateCatalog.

Record CurrentPublicationStoreState : Type := CurrentPublicationStoreStateData {
  current_publication_data : TwoLevelAuthority;
  current_publication_continuations : ContinuationAuthority;
  current_publication_catalog_cache : CurrentCatalogCache;
  current_publication_logical_generation : nat;
  current_publication_datum_root_generation : nat;
  current_publication_durable_semantic_generation : nat
}.

Definition current_datum_changed {before changes}
    (successor : ExactDatumSuccessor before changes) : bool :=
  match prepared_channel_deltas (exact_datum_delta successor) with
  | [] => false
  | _ :: _ => true
  end.

Definition invalidate_current_catalogs
    (before : CurrentCatalogCache) (touched : list Path)
    : CurrentCatalogCache :=
  fun channel =>
    if in_dec path_eq_dec channel touched then None else before channel.

Record CurrentPreparedPublication
    (before : CurrentPublicationStoreState)
    (proposal : CurrentAcceptedProposal)
    (changes : PreparedMatchChanges) : Type := CurrentPreparedPublicationState {
  current_publication_input : MatchPreparationInput;
  current_publication_input_is_exact :
    exact_preparation_input proposal current_publication_input changes;
  current_publication_commit_data_is_exact :
    preparation_commit_authority current_publication_input =
      current_publication_data before;
  current_publication_datum_successor :
    ExactDatumSuccessor (current_publication_data before) changes;
  current_publication_continuation_successor :
    ExactContinuationSuccessor
      (current_publication_continuations before)
      (current_proposal_channels proposal)
      (current_proposal_continuation_use proposal)
}.

Definition publish_current_state {before proposal changes}
    (publication : CurrentPreparedPublication before proposal changes)
    : CurrentPublicationStoreState :=
  let datum := current_publication_datum_successor publication in
  let continuation := current_publication_continuation_successor publication in
  CurrentPublicationStoreStateData
    (exact_datum_after datum)
    (exact_continuation_after continuation)
    (invalidate_current_catalogs
      (current_publication_catalog_cache before)
      (prepared_delta_channels
        (prepared_channel_deltas (exact_datum_delta datum))))
    (S (current_publication_logical_generation before))
    (current_publication_datum_root_generation before +
      if current_datum_changed datum then 1 else 0)
    (current_publication_durable_semantic_generation before +
      if current_datum_changed datum ||
          exact_continuation_changed continuation
      then 1 else 0).

Record CurrentMatchComm : Type := CurrentMatchCommState {
  current_comm_selected : list MatchCandidate;
  current_comm_bindings : free_map;
  current_comm_continuation : ContinuationRecord;
  current_comm_peeks : list Z
}.

Definition current_publication_comm {before proposal changes}
    (publication : CurrentPreparedPublication before proposal changes)
    : CurrentMatchComm :=
  let continuation := current_publication_continuation_successor publication in
  CurrentMatchCommState
    (current_proposal_selected proposal)
    (accepted_bindings (current_proposal_acceptance proposal))
    (exact_continuation_record continuation)
    (continuation_peeks (exact_continuation_record continuation)).

Theorem current_publication_preparation_is_exact : forall
    before proposal changes
    (publication : CurrentPreparedPublication before proposal changes),
  prepare_match_changes (current_publication_input publication) =
      PreparationReady changes /\
  checked_pool_selection (prepared_checked_pool changes) =
      current_proposal_selected proposal /\
  pool_observed_authority
      (checked_pool_value (prepared_checked_pool changes)) =
      search_observed_authority (current_proposal_search proposal) /\
  preparation_commit_authority (current_publication_input publication) =
      current_publication_data before.
Proof.
  intros before proposal changes publication.
  pose proof (current_publication_input_is_exact publication) as Hexact.
  unfold exact_preparation_input in Hexact.
  destruct Hexact as [_ [_ [_ [_ [_
    [Hprepare [Hobserved [_ [_ [_ [Hselection _]]]]]]]]]]].
  repeat split; try assumption.
  apply current_publication_commit_data_is_exact.
Qed.

Theorem current_publication_successors_are_executable : forall
    before proposal changes
    (publication : CurrentPreparedPublication before proposal changes),
  Forall2 exact_prepared_channel_refinement
      (prepared_partitioned_delta changes)
      (prepared_channel_deltas
        (exact_datum_delta
          (current_publication_datum_successor publication))) /\
  execute_current_continuation
      (current_publication_continuations before)
      (current_proposal_channels proposal)
      (current_proposal_continuation_use proposal) =
    Some
      (exact_continuation_after
        (current_publication_continuation_successor publication),
       exact_continuation_record
        (current_publication_continuation_successor publication)).
Proof.
  intros. split.
  - apply exact_datum_partition_refinement.
  - apply exact_continuation_execution.
Qed.

Inductive CurrentPublicationPhase
    (before : CurrentPublicationStoreState)
    (proposal : CurrentAcceptedProposal) : Type :=
| CurrentPublicationProposed
| CurrentPublicationStatePublished : forall changes,
    CurrentPreparedPublication before proposal changes ->
    CurrentPublicationPhase before proposal
| CurrentPublicationCommObserved : forall changes,
    CurrentPreparedPublication before proposal changes ->
    CurrentPublicationPhase before proposal
| CurrentPublicationRefused.

Inductive current_publication_transition {before proposal} :
    CurrentPublicationPhase before proposal ->
    CurrentPublicationPhase before proposal -> Prop :=
| PublishCurrentState : forall changes publication,
    current_publication_transition
      (@CurrentPublicationProposed before proposal)
      (@CurrentPublicationStatePublished before proposal changes publication)
| ObserveCurrentComm : forall changes publication,
    current_publication_transition
      (@CurrentPublicationStatePublished before proposal changes publication)
      (@CurrentPublicationCommObserved before proposal changes publication)
| RefuseCurrentAtomically :
    (forall changes, CurrentPreparedPublication before proposal changes -> False) ->
    current_publication_transition
      (@CurrentPublicationProposed before proposal)
      (@CurrentPublicationRefused before proposal).

Definition current_phase_state {before proposal}
    (phase : CurrentPublicationPhase before proposal)
    : CurrentPublicationStoreState :=
  match phase with
  | @CurrentPublicationProposed _ _
  | @CurrentPublicationRefused _ _ => before
  | @CurrentPublicationStatePublished _ _ _ publication
  | @CurrentPublicationCommObserved _ _ _ publication =>
      publish_current_state publication
  end.

Definition current_phase_comm {before proposal}
    (phase : CurrentPublicationPhase before proposal)
    : option CurrentMatchComm :=
  match phase with
  | @CurrentPublicationCommObserved _ _ _ publication =>
      Some (current_publication_comm publication)
  | _ => None
  end.

Definition current_phase_publication_count {before proposal}
    (phase : CurrentPublicationPhase before proposal) : nat :=
  match phase with
  | @CurrentPublicationStatePublished _ _ _ _
  | @CurrentPublicationCommObserved _ _ _ _ => 1
  | _ => 0
  end.

Definition current_phase_datum_root_count {before proposal}
    (phase : CurrentPublicationPhase before proposal) : nat :=
  match phase with
  | @CurrentPublicationStatePublished _ _ _ publication
  | @CurrentPublicationCommObserved _ _ _ publication =>
      if current_datum_changed
          (current_publication_datum_successor publication)
      then 1 else 0
  | _ => 0
  end.

Definition current_phase_durable_semantic_count {before proposal}
    (phase : CurrentPublicationPhase before proposal) : nat :=
  match phase with
  | @CurrentPublicationStatePublished _ _ _ publication
  | @CurrentPublicationCommObserved _ _ _ publication =>
      if current_datum_changed
            (current_publication_datum_successor publication) ||
          exact_continuation_changed
            (current_publication_continuation_successor publication)
      then 1 else 0
  | _ => 0
  end.

Definition current_phase_comm_count {before proposal}
    (phase : CurrentPublicationPhase before proposal) : nat :=
  match phase with
  | @CurrentPublicationCommObserved _ _ _ _ => 1
  | _ => 0
  end.

Theorem current_refusal_is_strongly_atomic : forall before proposal,
  current_phase_state
      (@CurrentPublicationRefused before proposal) = before /\
  current_phase_comm
      (@CurrentPublicationRefused before proposal) = None /\
  current_phase_publication_count
      (@CurrentPublicationRefused before proposal) = 0 /\
  current_phase_datum_root_count
      (@CurrentPublicationRefused before proposal) = 0 /\
  current_phase_durable_semantic_count
      (@CurrentPublicationRefused before proposal) = 0 /\
  current_phase_comm_count
      (@CurrentPublicationRefused before proposal) = 0.
Proof. intros. repeat split; reflexivity. Qed.

Theorem current_refusal_transition_requires_no_publication : forall
    before proposal,
  current_publication_transition
      (@CurrentPublicationProposed before proposal)
      (@CurrentPublicationRefused before proposal) ->
  forall changes,
    CurrentPreparedPublication before proposal changes -> False.
Proof.
  intros before proposal Htransition.
  inversion Htransition; subst. exact H.
Qed.

Theorem current_state_is_published_before_comm : forall
    before proposal changes
    (publication : CurrentPreparedPublication before proposal changes),
  current_publication_transition
      (@CurrentPublicationStatePublished before proposal changes publication)
      (@CurrentPublicationCommObserved before proposal changes publication) /\
  current_phase_state
      (@CurrentPublicationStatePublished before proposal changes publication) =
    current_phase_state
      (@CurrentPublicationCommObserved before proposal changes publication) /\
  current_phase_comm
      (@CurrentPublicationStatePublished before proposal changes publication) = None /\
  current_phase_comm
      (@CurrentPublicationCommObserved before proposal changes publication) =
    Some (current_publication_comm publication).
Proof. intros. repeat split; try constructor; reflexivity. Qed.

Theorem current_publication_uses_exact_prepared_successors : forall
    before proposal changes
    (publication : CurrentPreparedPublication before proposal changes),
  current_publication_data (publish_current_state publication) =
      exact_datum_after (current_publication_datum_successor publication) /\
  current_publication_continuations (publish_current_state publication) =
      exact_continuation_after
        (current_publication_continuation_successor publication) /\
  current_publication_logical_generation (publish_current_state publication) =
      S (current_publication_logical_generation before) /\
  current_publication_datum_root_generation (publish_current_state publication) =
      current_publication_datum_root_generation before +
        (if current_datum_changed
            (current_publication_datum_successor publication)
         then 1 else 0) /\
  current_publication_durable_semantic_generation
      (publish_current_state publication) =
      current_publication_durable_semantic_generation before +
        (if current_datum_changed
              (current_publication_datum_successor publication) ||
            exact_continuation_changed
              (current_publication_continuation_successor publication)
         then 1 else 0).
Proof. intros. repeat split; reflexivity. Qed.

Print Assumptions current_accepted_proposal_preserves_requested_channel_vector.
Print Assumptions current_publication_preparation_is_exact.
Print Assumptions current_publication_successors_are_executable.
Print Assumptions current_refusal_is_strongly_atomic.
Print Assumptions current_refusal_transition_requires_no_publication.
Print Assumptions current_state_is_published_before_comm.
Print Assumptions current_publication_uses_exact_prepared_successors.

End TwoLevelRSpaceCurrentPublication.
