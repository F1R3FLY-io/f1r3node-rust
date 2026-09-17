From Stdlib Require Import Bool List Arith NArith.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority
  RhoPathV1Datum
  TwoLevelRSpaceMatchPreparation.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import RhoPathV1Datum.
Import TwoLevelRSpaceMatchPreparation.
Open Scope nat_scope.

Module TwoLevelRSpaceMatchPreparationMutants.

Definition marker_a : Path := datum_channel_marker [1] [0; 2].
Definition marker_b : Path := datum_channel_marker [1] [0; 3].
Definition relative_a : Path := relative_datum_key [4; 0] [5; 0].
Definition absolute_a : Path := marker_a ++ relative_a.
Definition absolute_b : Path := marker_b ++ relative_a.
Definition malformed_path : Path := [255].

Definition fixture_delta : FlatBagDelta :=
  FlatBagDeltaState [(absolute_a, 2%N)] [(absolute_b, 3%N)].

Definition expected_groups : list PartitionedChannelDelta :=
  [PartitionedChannelDeltaState marker_a
     (FlatBagDeltaState [(relative_a, 2%N)] []);
   PartitionedChannelDeltaState marker_b
     (FlatBagDeltaState [] [(relative_a, 3%N)])].

Example exact_partition_positive_control :
  flat_delta_well_formedb fixture_delta = true /\
  partition_flat_delta fixture_delta = BuildOk expected_groups.
Proof. vm_compute. auto. Qed.

Example wrong_channel_partition_is_distinguished :
  partition_flat_delta fixture_delta <>
    BuildOk [PartitionedChannelDeltaState marker_b
      (FlatBagDeltaState [(relative_a, 2%N)] [(relative_a, 3%N)])].
Proof. vm_compute. discriminate. Qed.

Example shifted_channel_boundary_is_distinguished :
  skipn (S (length marker_a)) absolute_a <> relative_a.
Proof. vm_compute. discriminate. Qed.

Example polarity_swap_is_distinguished :
  partition_flat_delta fixture_delta <>
    BuildOk [PartitionedChannelDeltaState marker_a
      (FlatBagDeltaState [] [(relative_a, 2%N)]);
    PartitionedChannelDeltaState marker_b
      (FlatBagDeltaState [(relative_a, 3%N)] [])].
Proof. vm_compute. discriminate. Qed.

Example omitted_partition_is_distinguished :
  partition_flat_delta fixture_delta <>
    BuildOk [PartitionedChannelDeltaState marker_a
      (FlatBagDeltaState [(relative_a, 2%N)] [])].
Proof. vm_compute. discriminate. Qed.

Example opposite_supports_are_rejected_even_when_net_zero :
  flat_delta_well_formedb
    (FlatBagDeltaState [(absolute_a, 2%N)] [(absolute_a, 2%N)]) = false.
Proof. vm_compute. reflexivity. Qed.

Example exact_cancellation_distinguishes_skipping_and_doubling :
  flat_record_removal (FlatBagDeltaState [(absolute_a, 5%N)] []) absolute_a 3%N =
    Some (FlatBagDeltaState [(absolute_a, 2%N)] []) /\
  flat_record_removal (FlatBagDeltaState [(absolute_a, 5%N)] []) absolute_a 3%N <>
    Some (FlatBagDeltaState [(absolute_a, 5%N)] [(absolute_a, 3%N)]) /\
  flat_record_removal (FlatBagDeltaState [(absolute_a, 5%N)] []) absolute_a 3%N <>
    Some (FlatBagDeltaState [] [(absolute_a, 1%N)]).
Proof. vm_compute. repeat split; discriminate. Qed.

Example same_key_arrival_and_removal_cancel :
  flat_record_addition (FlatBagDeltaState [] [(absolute_a, 1%N)]) absolute_a 1%N =
    Some empty_flat_delta.
Proof. vm_compute. reflexivity. Qed.

Example overflow_record_is_refused :
  flat_record_addition (FlatBagDeltaState [(absolute_a, multiplicity_max)] [])
    absolute_a 1%N = None /\
  flat_record_removal (FlatBagDeltaState [] [(absolute_a, multiplicity_max)])
    absolute_a 1%N = None.
Proof. vm_compute. auto. Qed.

Example valid_prefix_then_malformed_partition_is_refused :
  partition_flat_delta
    (FlatBagDeltaState [(absolute_a, 1%N); (malformed_path, 1%N)] []) =
    BuildRefused (RefuseFlatDeltaPartition malformed_path).
Proof. vm_compute. reflexivity. Qed.

Example malformed_fresh_construction_is_refused : forall codec,
  construct_fresh_candidate codec malformed_path =
    BuildRefused (RefuseMalformedFreshPath malformed_path).
Proof. intros codec. reflexivity. Qed.

Example malformed_frame_and_nonbyte_construction_are_refused :
  parse_absolute_datum (marker_a ++ [0; 254]) = None /\
  absolute_datum_key_checked [1] [2] [256] [3] = None.
Proof. vm_compute. auto. Qed.

Definition preparation_fixture_codec : CanonicalDatumCodec :=
  CanonicalDatumCodecState
    (fun identity =>
      if path_eqb identity [5; 0] then
        Some (DecodedCanonicalDatumState [4; 0] false [5; 0])
      else if path_eqb identity [6; 0] then
        Some (DecodedCanonicalDatumState [4; 0] true [6; 0])
      else None)
    (fun _ => repeat 0 32).

Definition fixture_candidate (marker : Path) (persistent : bool)
    (count : Multiplicity) : MatchCandidate :=
  let identity := if persistent then [6; 0] else [5; 0] in
  let relative := relative_datum_key [4; 0] identity in
  MatchCandidateState (StoredAddress (marker ++ relative)) marker relative
    [4; 0] identity (repeat 0 32) persistent count.

Definition fixture_commit (persistent : bool) (count : Multiplicity) : TwoLevelAuthority :=
  [(marker_a, if N.eqb count 0 then [] else
    [(relative_datum_key [4; 0] (if persistent then [6; 0] else [5; 0]), count)])].

Definition preparation_fixture (observed commit : TwoLevelAuthority)
    (selected : list MatchCandidate) (fresh : option Path) : MatchPreparationInput :=
  MatchPreparationInputState preparation_fixture_codec observed commit
    (selected_channels selected) fresh true [] selected.

Definition preparation_succeeds (input : MatchPreparationInput) : bool :=
  match prepare_match_changes input with
  | PreparationReady _ => true
  | PreparationRefused _ => false
  end.

Example nonempty_preparation_uses_constructed_candidate :
  construct_candidate_at preparation_fixture_codec (StoredAddress absolute_a)
    absolute_a 2%N = BuildOk (fixture_candidate marker_a false 2%N) /\
  preparation_succeeds (preparation_fixture
    (fixture_commit false 2%N) (fixture_commit false 2%N)
    [fixture_candidate marker_a false 2%N; fixture_candidate marker_a false 2%N] None) = true.
Proof. vm_compute. auto. Qed.

Example repeated_selection_refuses_stale_commit_count :
  prepare_match_changes (preparation_fixture
    (fixture_commit false 2%N) (fixture_commit false 1%N)
    [fixture_candidate marker_a false 2%N; fixture_candidate marker_a false 2%N] None) =
    PreparationRefused RefuseCommitLiveness.
Proof. vm_compute. reflexivity. Qed.

Example persistent_selection_requires_commit_presence :
  preparation_succeeds (preparation_fixture
    (fixture_commit true 1%N) (fixture_commit true 1%N)
    [fixture_candidate marker_a true 1%N] None) = true /\
  prepare_match_changes (preparation_fixture
    (fixture_commit true 1%N) (fixture_commit true 0%N)
    [fixture_candidate marker_a true 1%N] None) =
    PreparationRefused RefuseCommitLiveness.
Proof. vm_compute. auto. Qed.

Example forged_observed_multiplicity_cannot_authorize_preparation :
  prepare_match_changes (preparation_fixture
    (fixture_commit false 1%N) (fixture_commit false 2%N)
    [fixture_candidate marker_a false 2%N] None) =
    PreparationRefused RefuseObservedPoolFeasibility.
Proof. vm_compute. reflexivity. Qed.

Example unselected_arrival_checks_current_not_observed_multiplicity :
  preparation_succeeds (preparation_fixture
    (fixture_commit false 0%N) (fixture_commit false (N.pred multiplicity_max))
    [] (Some absolute_a)) = true /\
  prepare_match_changes (preparation_fixture
    (fixture_commit false 0%N) (fixture_commit false multiplicity_max)
    [] (Some absolute_a)) = PreparationRefused RefuseCommitDelta.
Proof. vm_compute. auto. Qed.

Example unrelated_commit_channel_does_not_refuse_preparation :
  preparation_succeeds (preparation_fixture
    (fixture_commit false 2%N)
    (fixture_commit false 2%N ++ [(marker_b, [(relative_a, multiplicity_max)])])
    [fixture_candidate marker_a false 2%N; fixture_candidate marker_a false 2%N] None) = true.
Proof. vm_compute. reflexivity. Qed.

Example later_stale_channel_refuses_complete_preparation :
  prepare_match_changes (preparation_fixture
    [(marker_a, [(relative_a, 1%N)]); (marker_b, [(relative_a, 1%N)])]
    [(marker_a, [(relative_a, 1%N)]); (marker_b, [])]
    [fixture_candidate marker_a false 1%N; fixture_candidate marker_b false 1%N] None) =
    PreparationRefused RefuseCommitLiveness.
Proof. vm_compute. reflexivity. Qed.

Example malformed_arrival_refuses_complete_preparation :
  prepare_match_changes (preparation_fixture
    (fixture_commit false 1%N) (fixture_commit false 1%N)
    [fixture_candidate marker_a false 1%N] (Some malformed_path)) =
    PreparationRefused (RefuseMalformedFreshPath malformed_path).
Proof. vm_compute. reflexivity. Qed.

Example forged_absolute_address_refuses_preparation :
  prepare_match_changes (preparation_fixture
    (fixture_commit false 1%N) (fixture_commit false 1%N)
    [MatchCandidateState (StoredAddress malformed_path) marker_a relative_a
      [4; 0] [5; 0] (repeat 0 32) false 1%N] None) =
    PreparationRefused RefuseObservedPoolFeasibility.
Proof. vm_compute. reflexivity. Qed.

Example preflight_respects_aggregated_strict_subtraction :
  strict_flat_preflightb (fun _ => 2%N)
    (FlatBagDeltaState [] [(absolute_a, 2%N)]) = true /\
  strict_flat_preflightb (fun _ => 1%N)
    (FlatBagDeltaState [] [(absolute_a, 2%N)]) = false.
Proof. vm_compute. auto. Qed.

Example preflight_does_not_require_selection_order :
  let delta := FlatBagDeltaState [(absolute_b, 1%N); (absolute_a, 1%N)] [] in
  flat_delta_well_formedb delta = true /\
  entries_orderedb (flat_delta_additions delta) = false /\
  strict_flat_preflightb (fun _ => 1%N) delta = true.
Proof. vm_compute. auto. Qed.

Example arrival_cancellation_does_not_authorize_a_stale_stored_selection :
  preparation_succeeds (preparation_fixture
    (fixture_commit false 1%N) (fixture_commit false 1%N)
    [fixture_candidate marker_a false 1%N] (Some absolute_a)) = true /\
  prepare_match_changes (preparation_fixture
    (fixture_commit false 1%N) (fixture_commit false 0%N)
    [fixture_candidate marker_a false 1%N] (Some absolute_a)) =
    PreparationRefused RefuseCommitLiveness.
Proof. vm_compute. auto. Qed.

Example no_cancellation_needs_no_guard :
  guarded_record_addition (initialize_guarded_removals [(absolute_a, 2%N)]) absolute_b 1%N =
    Some (CancellationGuardedDeltaState
      (FlatBagDeltaState [(absolute_b, 1%N)] [(absolute_a, 2%N)]) []).
Proof. vm_compute. reflexivity. Qed.

Example partial_cancellation_retains_full_not_cancelled_demand :
  guarded_record_addition (initialize_guarded_removals [(absolute_a, 3%N)]) absolute_a 1%N =
    Some (CancellationGuardedDeltaState
      (FlatBagDeltaState [] [(absolute_a, 2%N)]) [(absolute_a, 3%N)]).
Proof. vm_compute. reflexivity. Qed.

Example full_cancellation_retains_liveness_when_delta_is_empty :
  guarded_record_addition (initialize_guarded_removals [(absolute_a, 1%N)]) absolute_a 1%N =
    Some (CancellationGuardedDeltaState empty_flat_delta [(absolute_a, 1%N)]).
Proof. vm_compute. reflexivity. Qed.

Example repeated_cancellation_never_lowers_original_demand :
  record_guarded_additions (initialize_guarded_removals [(absolute_a, 3%N)])
    [(absolute_a, 1%N); (absolute_a, 1%N); (absolute_a, 1%N); (absolute_a, 1%N)] =
    Some (CancellationGuardedDeltaState
      (FlatBagDeltaState [(absolute_a, 1%N)] []) [(absolute_a, 3%N)]).
Proof. vm_compute. reflexivity. Qed.

Example failed_guarded_record_preserves_guard_and_delta :
  let before := CancellationGuardedDeltaState
    (FlatBagDeltaState [(absolute_b, multiplicity_max)] []) [(absolute_a, 3%N)] in
  guarded_record_addition before absolute_b 1%N = None /\
  atomic_guarded_addition before absolute_b 1%N = before.
Proof. vm_compute. auto. Qed.

Example eager_decoding_is_not_an_encoded_construction_step :
  let absolute := marker_a ++ relative_datum_key [4; 0] [99] in
  let codec := preparation_fixture_codec in
  (match construct_encoded_candidate_at (codec_digest_raw_identity codec)
      (StoredAddress absolute) absolute 1%N with
   | BuildOk _ => true
   | BuildRefused _ => false
   end) = true /\
  construct_stored_candidate codec absolute 1%N =
    BuildRefused (RefuseNoncanonicalDatumIdentity [99]).
Proof. vm_compute. auto. Qed.

Example encoded_digest_width_and_byte_range_controls :
  construct_encoded_candidate_at (fun _ => repeat 0 31)
    (StoredAddress absolute_a) absolute_a 1%N =
    BuildRefused (RefuseDigestWidth [5; 0] (repeat 0 31)) /\
  construct_encoded_candidate_at (fun _ => repeat 256 32)
    (StoredAddress absolute_a) absolute_a 1%N =
    BuildRefused (RefuseDigestWidth [5; 0] (repeat 256 32)).
Proof. vm_compute. auto. Qed.

Example unrequested_malformed_bag_does_not_poison_catalog :
  build_candidate_catalog_set preparation_fixture_codec
    [(marker_a, [(relative_a, 2%N)]); (marker_b, [(malformed_path, 1%N)])]
    [marker_a; marker_a] =
  build_candidate_catalog_set preparation_fixture_codec
    [(marker_a, [(relative_a, 2%N)])] [marker_a; marker_a] /\
  build_candidate_catalog preparation_fixture_codec
    [(marker_b, [(malformed_path, 1%N)])] marker_b =
    BuildRefused (RefuseMalformedStoredPath (marker_b ++ malformed_path)).
Proof. vm_compute. auto. Qed.

Example colliding_digest_sort_retains_both_identities_and_multiplicities :
  sort_candidates [fixture_candidate marker_a true 3%N;
    fixture_candidate marker_a false 2%N] =
    [fixture_candidate marker_a false 2%N; fixture_candidate marker_a true 3%N] /\
  catalog_unique_identityb (sort_candidates
    [fixture_candidate marker_a false 2%N;
     fixture_candidate marker_a false 3%N]) = false.
Proof. vm_compute. auto. Qed.

Example order_check_must_inspect_the_second_and_third_candidates :
  catalog_orderedb [fixture_candidate marker_a false 1%N;
    fixture_candidate marker_a true 1%N;
    fixture_candidate marker_a false 2%N] = false.
Proof. vm_compute. reflexivity. Qed.

Example duplicate_check_must_inspect_the_second_and_third_candidates :
  catalog_unique_identityb [fixture_candidate marker_a false 1%N;
    fixture_candidate marker_a true 1%N;
    fixture_candidate marker_a true 2%N] = false.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions order_check_must_inspect_the_second_and_third_candidates.
Example authority_checks_must_inspect_the_second_and_third_entries :
  entries_orderedb [([1], 1%N); ([3], 1%N); ([2], 1%N)] = false /\
  channels_orderedb [([1], []); ([3], []); ([2], [])] = false.
Proof. vm_compute. auto. Qed.

Print Assumptions authority_checks_must_inspect_the_second_and_third_entries.
Print Assumptions duplicate_check_must_inspect_the_second_and_third_candidates.
Print Assumptions eager_decoding_is_not_an_encoded_construction_step.
Print Assumptions encoded_digest_width_and_byte_range_controls.
Print Assumptions unrequested_malformed_bag_does_not_poison_catalog.
Print Assumptions colliding_digest_sort_retains_both_identities_and_multiplicities.
Print Assumptions exact_partition_positive_control.
Print Assumptions wrong_channel_partition_is_distinguished.
Print Assumptions shifted_channel_boundary_is_distinguished.
Print Assumptions polarity_swap_is_distinguished.
Print Assumptions omitted_partition_is_distinguished.
Print Assumptions opposite_supports_are_rejected_even_when_net_zero.
Print Assumptions exact_cancellation_distinguishes_skipping_and_doubling.
Print Assumptions same_key_arrival_and_removal_cancel.
Print Assumptions overflow_record_is_refused.
Print Assumptions valid_prefix_then_malformed_partition_is_refused.
Print Assumptions malformed_fresh_construction_is_refused.
Print Assumptions malformed_frame_and_nonbyte_construction_are_refused.
Print Assumptions nonempty_preparation_uses_constructed_candidate.
Print Assumptions repeated_selection_refuses_stale_commit_count.
Print Assumptions persistent_selection_requires_commit_presence.
Print Assumptions forged_observed_multiplicity_cannot_authorize_preparation.
Print Assumptions unselected_arrival_checks_current_not_observed_multiplicity.
Print Assumptions unrelated_commit_channel_does_not_refuse_preparation.
Print Assumptions later_stale_channel_refuses_complete_preparation.
Print Assumptions malformed_arrival_refuses_complete_preparation.
Print Assumptions forged_absolute_address_refuses_preparation.
Print Assumptions preflight_respects_aggregated_strict_subtraction.
Print Assumptions preflight_does_not_require_selection_order.
Print Assumptions arrival_cancellation_does_not_authorize_a_stale_stored_selection.
Print Assumptions no_cancellation_needs_no_guard.
Print Assumptions partial_cancellation_retains_full_not_cancelled_demand.
Print Assumptions full_cancellation_retains_liveness_when_delta_is_empty.
Print Assumptions repeated_cancellation_never_lowers_original_demand.
Print Assumptions failed_guarded_record_preserves_guard_and_delta.

End TwoLevelRSpaceMatchPreparationMutants.
