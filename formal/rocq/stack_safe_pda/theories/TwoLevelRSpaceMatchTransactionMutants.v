From Stdlib Require Import Bool List Arith Lia NArith.
From StackSafePDA Require Import
  StackSafePDA SpatialMatcher PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority
  TwoLevelRSpaceOperations TwoLevelRSpaceMatchPreparation
  TwoLevelRSpaceMatchTransaction.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceOperations.
Import TwoLevelRSpaceMatchPreparation.
Import TwoLevelRSpaceMatchTransaction.
Set Implicit Arguments.
Local Open Scope nat_scope.

(** Executable causal negative controls for the independent candidate selector.
    Each mutant changes one production choice and exposes a concrete bad
    observation. Candidate lists are proof observations of ordered PathMap
    cursors, never a proposed production representation. *)
Module TwoLevelRSpaceMatchTransactionMutants.

Definition fixture_authority : TwoLevelAuthority :=
  [([9], [([1], 2%N)])].

Definition fixture_stored : MatchCandidate :=
  MatchCandidateState (StoredAddress [9; 1]) [9] [1] [] [11] [] false 2%N.

Definition fixture_stored_alias : MatchCandidate :=
  MatchCandidateState (StoredAddress [9; 1]) [90] [10] [7] [12] [13] false 2%N.

Definition fixture_persistent : MatchCandidate :=
  MatchCandidateState (StoredAddress [9; 1]) [9] [1] [] [11] [] true 2%N.

Definition fixture_fresh : MatchCandidate :=
  MatchCandidateState FreshAddress [9] [1] [] [14] [] false 1%N.

Definition fixture_first : OrdinalCandidate :=
  OrdinalCandidateState 0%N fixture_stored.
Definition fixture_second : OrdinalCandidate :=
  OrdinalCandidateState 1%N fixture_stored_alias.
Definition fixture_persistent_ordinal : OrdinalCandidate :=
  OrdinalCandidateState 0%N fixture_persistent.
Definition fixture_fresh_ordinal : OrdinalCandidate :=
  OrdinalCandidateState 0%N fixture_fresh.

Definition accepting_tree : @tree matcher_label :=
  @StackSafePDA.Node matcher_label (Exact 0 0)
    (@StackSafePDA.FNil matcher_label).

Definition accepting_program : MatchProgram :=
  MatchProgramState (fun _ _ => accepting_tree) (fun _ => true).

Definition rejecting_guard_program : MatchProgram :=
  MatchProgramState (fun _ _ => accepting_tree) (fun _ => false).

Lemma subsequence_reflexive : forall A (values : list A),
  OrderPreservingSubsequence values values.
Proof.
  intros A values. induction values as [|head tail IH].
  - constructor.
  - now constructor.
Qed.

Lemma equal_length_subsequence_is_equal : forall A (selected full : list A),
  OrderPreservingSubsequence selected full ->
  length selected = length full -> selected = full.
Proof.
  intros A selected full Hsub. induction Hsub; intro Hlength.
  - reflexivity.
  - pose proof (@subsequence_length_is_bounded A selected full Hsub).
    simpl in Hlength. lia.
  - simpl in Hlength. f_equal. apply IHHsub. lia.
Qed.

Definition identity_query_scan
    (program : MatchProgram) (channel : Path) (full : list OrdinalCandidate)
    : QueryPrunedScan program channel full.
Proof.
  refine {| query_pruned_projection := full;
    query_pruned_order_is_preserved := subsequence_reflexive full;
    query_pruned_is_match_complete := _ |}.
  intros candidate bindings next Hin _. exact Hin.
Defined.

Definition empty_catalogs : CandidateCatalogSet :=
  CandidateCatalogSetState [] [] [].

Definition fixture_search_with_program (program : MatchProgram)
    : CandidateSearch.
Proof.
  refine {| search_observed_authority := fixture_authority;
            search_requested_channels := [];
            search_catalogs := empty_catalogs;
            search_arrival := None;
            search_excluded_count := fun _ => 0%N;
            search_fresh_available := true;
            search_program := program;
            search_scan := fun channel catalog =>
              identity_query_scan program channel
                (combined_candidate_projection None catalog) |}.
  - intros channel catalog candidate Hlookup _. discriminate Hlookup.
  - intros channel catalog candidate Hlookup _. discriminate Hlookup.
  - intros channel catalog fresh Hlookup _ _. discriminate Hlookup.
Defined.

Definition fixture_search : CandidateSearch :=
  fixture_search_with_program accepting_program.

Definition fixture_rejecting_guard_search : CandidateSearch :=
  fixture_search_with_program rejecting_guard_program.

Definition zero_reservations : ReservationAuthority := fun _ => 0%N.

(** 1. Reversing cursor output does not preserve canonical subsequence order. *)
Definition mutant_reverse_pruning {A : Type} (values : list A) : list A :=
  rev values.

Example pruned_order_rejects_reverse_mutant :
  OrderPreservingSubsequence [1; 2] [1; 2] /\
  ~ OrderPreservingSubsequence (mutant_reverse_pruning [1; 2]) [1; 2].
Proof.
  split; [apply subsequence_reflexive|].
  intro H.
  pose proof (@equal_length_subsequence_is_equal nat [2; 1] [1; 2]
    H eq_refl) as Hequal.
  discriminate Hequal.
Qed.

(** 2. Dropping an accepted candidate violates match completeness. *)
Definition mutant_drop_pruned_candidates
    (_ : list OrdinalCandidate) : list OrdinalCandidate := [].

Example pruned_completeness_rejects_drop_mutant :
  In fixture_first [fixture_first] /\
  mutant_drop_pruned_candidates [fixture_first] = [] /\
  ~ In fixture_first (mutant_drop_pruned_candidates [fixture_first]).
Proof. simpl. intuition. Qed.

(** 3. A replay exclusion strictly between zero and capacity is partial. *)
Definition mutant_partial_replay_exclusion : Multiplicity := 1%N.

Example replay_all_or_none_rejects_partial_mutant :
  mutant_partial_replay_exclusion <> 0%N /\
  mutant_partial_replay_exclusion <>
    candidate_capacity fixture_search fixture_first.
Proof. vm_compute. split; discriminate. Qed.

(** 4. Fresh eligibility comes from replay authority, not query pruning. *)
Definition mutant_fresh_available_after_pruning
    (retained_by_query : bool) : bool := retained_by_query.

Example fresh_eligibility_rejects_pruning_coupling_mutant :
  candidate_available fixture_search [9] fixture_fresh_ordinal
      zero_reservations = true /\
  mutant_fresh_available_after_pruning false = false.
Proof. vm_compute. auto. Qed.

(** 5. Persistence does not make a live candidate ineligible. *)
Definition mutant_persistent_candidates_are_ineligible
    (candidate : OrdinalCandidate) : bool :=
  negb (candidate_persistent (ordinal_candidate_value candidate)).

Example persistent_eligibility_rejects_ineligible_mutant :
  candidate_available fixture_search [9] fixture_persistent_ordinal
      zero_reservations = true /\
  mutant_persistent_candidates_are_ineligible fixture_persistent_ordinal = false.
Proof. vm_compute. auto. Qed.

(** 6. Persistent candidates create no reservation ticket or count change. *)
Definition mutant_reserve_persistent
    (reservations : ReservationAuthority) (candidate : OrdinalCandidate)
    : ReservationAuthority :=
  set_reservation_count reservations (candidate_reservation_key candidate)
    (N.succ (reservations (candidate_reservation_key candidate))).

Example persistent_no_reservation_rejects_increment_mutant :
  reserve_candidate fixture_search [9] fixture_persistent_ordinal
      zero_reservations = Some (zero_reservations, None) /\
  mutant_reserve_persistent zero_reservations fixture_persistent_ordinal
      (candidate_reservation_key fixture_persistent_ordinal) = 1%N.
Proof. vm_compute. auto. Qed.

(** 7. Bag multiplicity permits exactly two reservations at capacity two. *)
Definition reservation_after_first : ReservationAuthority :=
  set_reservation_count zero_reservations
    (candidate_reservation_key fixture_first) 1%N.

Definition reservation_after_second : ReservationAuthority :=
  set_reservation_count reservation_after_first
    (candidate_reservation_key fixture_first) 2%N.

Definition mutant_presence_only_capacity (_ : OrdinalCandidate) : Multiplicity :=
  1%N.

Example multiplicity_reservation_rejects_presence_only_mutant :
  reserve_candidate fixture_search [9] fixture_first zero_reservations =
    Some (reservation_after_first,
      Some (ReservationTicketState (candidate_reservation_key fixture_first)
        0%N zero_reservations)) /\
  reserve_candidate fixture_search [9] fixture_first reservation_after_first =
    Some (reservation_after_second,
      Some (ReservationTicketState (candidate_reservation_key fixture_first)
        1%N reservation_after_first)) /\
  mutant_presence_only_capacity fixture_first = 1%N /\
  candidate_capacity fixture_search fixture_first = 2%N.
Proof. vm_compute. repeat split; reflexivity. Qed.

(** 8. Rollback restores the exact prior authority, not the incremented one. *)
Definition fixture_ticket : ReservationTicket :=
  ReservationTicketState (candidate_reservation_key fixture_first)
    0%N zero_reservations.

Definition mutant_rollback_keeps_increment
    (_ : option ReservationTicket) (current : ReservationAuthority)
    : ReservationAuthority := current.

Example exact_rollback_rejects_keep_increment_mutant :
  rollback_reservation (Some fixture_ticket) reservation_after_first
      (candidate_reservation_key fixture_first) = 0%N /\
  mutant_rollback_keeps_increment (Some fixture_ticket) reservation_after_first
      (candidate_reservation_key fixture_first) = 1%N.
Proof. vm_compute. auto. Qed.

(** 9. Reservation sharing is keyed by absolute semantic address, not cursor
    ordinal or decoded channel/datum projections. *)
Definition mutant_ordinal_reservation_identity
    (candidate : OrdinalCandidate) : N := ordinal_candidate_index candidate.

Example semantic_reservation_rejects_ordinal_identity_mutant :
  candidate_reservation_key fixture_first =
    candidate_reservation_key fixture_second /\
  mutant_ordinal_reservation_identity fixture_first <>
    mutant_ordinal_reservation_identity fixture_second.
Proof. vm_compute. split; [reflexivity|discriminate]. Qed.

(** 10. The guard is evaluated only after all requested channels are chosen. *)
Definition mutant_guard_before_channel_lookup
    (search : CandidateSearch) : CandidateSearchResult :=
  if match_program_guard (search_program search) empty_free_map
  then SearchAccepted [] empty_free_map else SearchRefused GuardRejected.

Example leaf_only_guard_rejects_eager_guard_mutant :
  recursive_candidate_search fixture_rejecting_guard_search [[9]]
      zero_reservations empty_free_map = SearchRefused NoSpatialMatch /\
  mutant_guard_before_channel_lookup fixture_rejecting_guard_search =
      SearchRefused GuardRejected.
Proof. vm_compute. auto. Qed.

(** 11. Spatial exhaustion and reached-leaf guard rejection are distinct. *)
Definition mutant_collapse_failure (_ : SearchFailure) : SearchFailure :=
  NoSpatialMatch.

Example failure_distinction_rejects_collapse_mutant :
  failure_reached_leaf NoSpatialMatch = false /\
  failure_reached_leaf GuardRejected = true /\
  mutant_collapse_failure GuardRejected = NoSpatialMatch.
Proof. repeat split; reflexivity. Qed.

(** 12. Reversing a scan chooses a different first admissible candidate. *)
Definition accept_candidate (candidate : OrdinalCandidate)
    : CandidateSearchResult := SearchAccepted [([9], candidate)] empty_free_map.

Definition mutant_reverse_scan
    (candidates : list OrdinalCandidate) : CandidateSearchResult :=
  scan_reference accept_candidate (rev candidates) false.

Example first_selection_rejects_reverse_scan_mutant :
  scan_reference accept_candidate [fixture_first; fixture_second] false =
    SearchAccepted [([9], fixture_first)] empty_free_map /\
  mutant_reverse_scan [fixture_first; fixture_second] =
    SearchAccepted [([9], fixture_second)] empty_free_map.
Proof. vm_compute. auto. Qed.

(** 13. Retaining a control state is a non-progressing PDA cycle. *)
Definition mutant_stationary_walk (_ : CandidateSearch)
    (control : CandidateWalkControl) : CandidateWalkControl := control.

Example strict_work_rejects_stationary_walk_mutant :
  candidate_walk_work fixture_search
      (candidate_walk_step fixture_search
        (initial_walk_control fixture_search)) = 1%nat /\
  candidate_walk_work fixture_search
      (mutant_stationary_walk fixture_search
        (initial_walk_control fixture_search)) = 2%nat.
Proof. vm_compute. auto. Qed.

(** 14. Unconditional recursive failure breaks exact generated-PDA equality. *)
Definition mutant_recursive_spatial_failure
    (_ : MatchProgram) (_ : Path) (_ : OrdinalCandidate) (_ : free_map)
    : option free_map := None.

Example spatial_equivalence_rejects_failure_mutant :
  candidate_spatial_result accepting_program [9] fixture_first empty_free_map =
      Some empty_free_map /\
  mutant_recursive_spatial_failure accepting_program [9] fixture_first
      empty_free_map = None.
Proof. vm_compute. auto. Qed.

(** 15. Returning no result loses the generated PDA's exact oracle result. *)
Definition mutant_generated_pda_returns_none
    (_ : CandidateSearch) : option CandidateSearchResult := None.

Example generated_pda_equivalence_rejects_none_mutant :
  pda_candidate_search fixture_search =
      Some (SearchAccepted [] empty_free_map) /\
  mutant_generated_pda_returns_none fixture_search = None.
Proof. vm_compute. auto. Qed.

(** 16. Three uses of a multiplicity-two endpoint cannot replay exactly. *)
Definition over_capacity_selection : OrdinalSelection :=
  [([9], fixture_first); ([9], fixture_first); ([9], fixture_first)].

Definition mutant_accept_without_reservation_replay
    (_ : OrdinalSelection) : bool := true.

Example reservation_feasibility_rejects_over_capacity_mutant :
  replay_selection_reservations fixture_search over_capacity_selection
      zero_reservations = None /\
  mutant_accept_without_reservation_replay over_capacity_selection = true.
Proof. vm_compute. auto. Qed.

(** 17. Deduplicating selected channels destroys the exact request vector. *)
Definition repeated_selection : OrdinalSelection :=
  [([9], fixture_first); ([9], fixture_second)].

Definition mutant_deduplicate_selected_channels (selected : OrdinalSelection)
    : list Path := nodup path_eq_dec (selected_ordinal_channels selected).

Example exact_channel_vector_rejects_dedup_mutant :
  selected_ordinal_channels repeated_selection = [[9]; [9]] /\
  mutant_deduplicate_selected_channels repeated_selection = [[9]].
Proof. vm_compute. auto. Qed.

(** 18. A requested channel with no exact catalog cannot satisfy the
    well-formed-search catalog obligation. *)
Definition mutant_missing_requested_catalogs : CandidateCatalogSet :=
  CandidateCatalogSetState [[9]] [[9]] [].

Example requested_catalog_presence_rejects_missing_mutant :
  In [9] (candidate_catalog_requested_channels
    mutant_missing_requested_catalogs) /\
  lookup_catalog [9] mutant_missing_requested_catalogs = None /\
  ~ exists catalog,
      lookup_catalog [9] mutant_missing_requested_catalogs = Some catalog.
Proof.
  split; [now left|]. split; [reflexivity|].
  intros [catalog Hcatalog]. discriminate Hcatalog.
Qed.

Inductive independent_match_mutant_id : Type :=
| MutantReversePruning
| MutantDropAcceptedCandidate
| MutantPartialReplayExclusion
| MutantFreshEligibilityDependsOnPruning
| MutantPersistentIneligible
| MutantPersistentReservation
| MutantPresenceOnlyMultiplicity
| MutantInexactRollback
| MutantOrdinalReservationIdentity
| MutantEagerGuard
| MutantCollapsedFailure
| MutantReverseFirstSelection
| MutantStationaryWalk
| MutantSpatialFailure
| MutantGeneratedPdaNone
| MutantSkipReservationReplay
| MutantDeduplicateChannelVector
| MutantMissingRequestedCatalog.

Definition independent_match_mutant_catalog :=
  [MutantReversePruning;
   MutantDropAcceptedCandidate;
   MutantPartialReplayExclusion;
   MutantFreshEligibilityDependsOnPruning;
   MutantPersistentIneligible;
   MutantPersistentReservation;
   MutantPresenceOnlyMultiplicity;
   MutantInexactRollback;
   MutantOrdinalReservationIdentity;
   MutantEagerGuard;
   MutantCollapsedFailure;
   MutantReverseFirstSelection;
   MutantStationaryWalk;
   MutantSpatialFailure;
   MutantGeneratedPdaNone;
   MutantSkipReservationReplay;
   MutantDeduplicateChannelVector;
   MutantMissingRequestedCatalog].

Theorem independent_match_mutant_catalog_is_duplicate_free_and_exact :
  NoDup independent_match_mutant_catalog /\
  length independent_match_mutant_catalog = 18%nat /\
  length independent_match_mutant_catalog =
    length independent_match_invariant_catalog.
Proof.
  split.
  - repeat constructor; simpl; intuition discriminate.
  - split; reflexivity.
Qed.

Print Assumptions pruned_order_rejects_reverse_mutant.
Print Assumptions replay_all_or_none_rejects_partial_mutant.
Print Assumptions multiplicity_reservation_rejects_presence_only_mutant.
Print Assumptions exact_rollback_rejects_keep_increment_mutant.
Print Assumptions semantic_reservation_rejects_ordinal_identity_mutant.
Print Assumptions leaf_only_guard_rejects_eager_guard_mutant.
Print Assumptions first_selection_rejects_reverse_scan_mutant.
Print Assumptions strict_work_rejects_stationary_walk_mutant.
Print Assumptions generated_pda_equivalence_rejects_none_mutant.
Print Assumptions reservation_feasibility_rejects_over_capacity_mutant.
Print Assumptions independent_match_mutant_catalog_is_duplicate_free_and_exact.

End TwoLevelRSpaceMatchTransactionMutants.
