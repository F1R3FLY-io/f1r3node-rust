From Stdlib Require Import Bool List Arith Lia NArith.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublicationBase
  PathMapCanonicalStreamPublication
  TwoLevelRSpaceAuthority
  TwoLevelRSpaceMatchTransaction
  TwoLevelRSpaceMatchPreparation
  TwoLevelRSpaceDelta
  TwoLevelRSpaceContinuationAuthority.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceMatchTransaction.
Import TwoLevelRSpaceMatchPreparation.
Import TwoLevelRSpaceDelta.
Import TwoLevelRSpaceContinuationAuthority.
Set Implicit Arguments.

(** A live publication certificate for the current PathMap RSpace model.
    This unit deliberately does not depend on the retired commented bridge
    vocabulary in [TwoLevelRSpaceMatchTransaction].  Every authority update
    is witnessed by an executable checked preparation result and an exact
    successor certificate. *)
Module TwoLevelRSpaceCurrentPublication.

Record CurrentAcceptedProposal : Type := CurrentAcceptedProposalState {
  accepted_proposal_search : CandidateSearch;
  accepted_proposal_well_formed :
    WellFormedCandidateSearch accepted_proposal_search;
  accepted_proposal_acceptance :
    CertifiedSearchAcceptance accepted_proposal_search;
  accepted_proposal_requested_channels_are_exact :
    selected_ordinal_channels
      (accepted_selected_ordinals accepted_proposal_acceptance) =
    search_requested_channels accepted_proposal_search
}.

Definition partition_marker (group : PartitionedChannelDelta) : Path :=
  partitioned_channel_marker group.

Definition current_bag_delta_exactly_refines_flat
    (flat : FlatBagDelta) (delta : BagDelta) : Prop :=
  bag_delta_removal_mode delta = StrictSubtract /\
  bag_delta_subtractions delta = flat_delta_removals flat /\
  bag_delta_deletions delta = [] /\
  bag_delta_additions delta = flat_delta_additions flat.

Definition partition_refines_prepared_delta {before}
    (changes : PreparedMatchChanges)
    (delta : PreparedRSpaceDelta before) : Prop :=
  Forall2
    (fun group channel =>
       partition_marker group = @prepared_delta_channel before channel /\
       current_bag_delta_exactly_refines_flat
         (partitioned_channel_delta group)
         (@prepared_delta_change before channel))
    (fully_partitioned_groups
      (prepared_partition_permit changes))
    (prepared_channel_deltas delta).

Record ExactDatumSuccessor
    (before : TwoLevelAuthority) (changes : PreparedMatchChanges) : Type :=
  ExactDatumSuccessorState {
  exact_datum_delta : PreparedRSpaceDelta before;
  exact_datum_partition_refines :
    partition_refines_prepared_delta changes exact_datum_delta;
  exact_datum_successor : TwoLevelAuthority;
  exact_datum_successor_is_application :
    exact_datum_successor = apply_prepared_rspace_delta exact_datum_delta
}.

Inductive ExactContinuationSuccessor
    (observed before : ContinuationAuthority)
    (group : GroupKey) (joins : list Path)
    : ContinuationAuthority -> Type :=
| ExactImmediateContinuation :
    forall (record : ContinuationRecord),
    ExactContinuationSuccessor observed before group joins before
| ExactStoredContinuation :
    forall capability expected after,
    resolve_exact_continuation observed before group capability expected <> None ->
    commit_exact_continuation observed before group capability expected joins =
      Some after ->
    ExactContinuationSuccessor observed before group joins after.

Record CurrentPreparedPublication : Type := CurrentPreparedPublicationState {
  current_publication_before : TwoLevelAuthority;
  current_publication_proposal : CurrentAcceptedProposal;
  current_publication_input : MatchPreparationInput;
  current_publication_input_matches_proposal :
    preparation_observed_authority current_publication_input =
      search_observed_authority
        (accepted_proposal_search current_publication_proposal);
  current_publication_selected_candidates_are_exact :
    preparation_selected_candidates current_publication_input =
      selected_ordinal_values
        (accepted_selected_ordinals
          (accepted_proposal_acceptance current_publication_proposal));
  current_publication_preparation : PreparedMatchChanges;
  current_publication_prepare_is_exact :
    prepare_match_changes current_publication_input =
      PreparationReady current_publication_preparation;
  current_publication_datum_successor :
    ExactDatumSuccessor current_publication_before
      current_publication_preparation;
  current_publication_continuation_before : ContinuationAuthority;
  current_publication_continuation_group : GroupKey;
  current_publication_continuation_joins : list Path;
  current_publication_continuation_after : ContinuationAuthority;
  current_publication_continuation_is_exact :
    ExactContinuationSuccessor
      current_publication_continuation_before
      current_publication_continuation_before
      current_publication_continuation_group
      current_publication_continuation_joins
      current_publication_continuation_after
}.

Inductive CurrentPublicationPhase : Type :=
| CurrentPublicationProposed
| CurrentPublicationPublished (publication : CurrentPreparedPublication)
| CurrentPublicationCommObserved (publication : CurrentPreparedPublication)
| CurrentPublicationRefused.

Definition current_publication_datum_authority
    (publication : CurrentPreparedPublication) : TwoLevelAuthority :=
  exact_datum_successor
    (current_publication_datum_successor publication).

Definition current_publication_continuation_authority
    (publication : CurrentPreparedPublication) : ContinuationAuthority :=
  current_publication_continuation_after publication.

Theorem current_publication_datum_is_not_caller_supplied : forall publication,
  current_publication_datum_authority publication =
    apply_prepared_rspace_delta
      (exact_datum_delta (current_publication_datum_successor publication)).
Proof.
  intros publication.
  unfold current_publication_datum_authority.
  apply exact_datum_successor_is_application.
Qed.

Definition current_phase_publishes (phase : CurrentPublicationPhase) : bool :=
  match phase with
  | CurrentPublicationPublished _ | CurrentPublicationCommObserved _ => true
  | CurrentPublicationProposed | CurrentPublicationRefused => false
  end.

Theorem current_refusal_is_atomic :
  current_phase_publishes CurrentPublicationRefused = false.
Proof. reflexivity. Qed.

End TwoLevelRSpaceCurrentPublication.
