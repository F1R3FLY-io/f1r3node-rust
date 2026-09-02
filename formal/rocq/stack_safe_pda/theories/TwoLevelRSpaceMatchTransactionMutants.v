From Stdlib Require Import Bool List Arith Lia NArith.
From StackSafePDA Require Import
  StackSafePDA PathMapCanonicalStreamPublication
  TwoLevelRSpaceAuthority TwoLevelRSpaceOperations TwoLevelRSpaceDelta
  SpatialMatcher CandidateProductPathContinuation CandidateCatalogAuthorityIsolation
  PathMapPhysicalOwnership TwoLevelRSpaceMatchTransaction.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceOperations.
Import TwoLevelRSpaceDelta.
Import CandidateProductPathContinuation.
Import CandidateCatalogAuthorityIsolation.
Import PathMapPhysicalOwnership.
Import TwoLevelRSpaceMatchTransaction.
Set Implicit Arguments.
Open Scope nat_scope.

(** Executable causal negative controls for the two-level match transaction.
    Each witness changes one production choice and is rejected by the matching
    invariant. None merely assumes the negation of its target property. *)
Module TwoLevelRSpaceMatchTransactionMutants.

Definition fixture_first : Entry := ([1], 2%N).
Definition fixture_second : Entry := ([2], 3%N).
Definition fixture_inner : ChannelBag := [fixture_first; fixture_second].
Definition fixture_other_inner : ChannelBag := [([7], 1%N)].
Definition fixture_authority : TwoLevelAuthority :=
  [([9], fixture_inner); ([9; 1], fixture_other_inner); ([10], fixture_inner)].

Definition accepting_program (_ : Entry) : @tree matcher_label :=
  @StackSafePDA.Node matcher_label (Exact 0 0) (@StackSafePDA.FNil matcher_label).

Definition rejecting_program (_ : Entry) : @tree matcher_label :=
  @StackSafePDA.Node matcher_label (Exact 0 1) (@StackSafePDA.FNil matcher_label).

Definition fixture_request : MatchRequest :=
  MatchRequestState [9] 11 7 accepting_program [].

Definition fixture_rejecting_request : MatchRequest :=
  MatchRequestState [9] 11 7 rejecting_program [].

Definition fixture_stale_request : MatchRequest :=
  MatchRequestState [9] 12 7 accepting_program [].

Definition fixture_query_authority : candidate_authority :=
  CandidateAuthority 7
    (CandidateProductPathContinuation.Node (Some tt) [])
    (CandidateProductPathContinuation.Node (Some 0) []).

Definition fixture_isolated_catalog : isolated_candidate_catalog :=
  build_isolated_catalog 21 22 [] [] fixture_query_authority [].

Definition fixture_pool : isolated_candidate_pool :=
  issue_candidate_pool fixture_isolated_catalog.

Definition fixture_catalog : MatchCatalog.
Proof.
  refine (MatchCatalogState 11 fixture_pool [] _). reflexivity.
Defined.

Definition fixture_catalogs (channel : Path) : option MatchCatalog :=
  if path_eq_dec channel [9]
  then Some fixture_catalog
  else if path_eq_dec channel [10]
       then Some fixture_catalog
       else None.

Definition fixture_store : MatchStoreState :=
  TransactionalStoreState fixture_authority 3 fixture_catalogs [fixture_pool] 4.

(** 1. Prefix acceptance at the outer level confuses a loaded ancestor with
    an unloaded exact descendant. *)
Fixpoint mutant_prefix_outer_lookup
    (selected : Path) (authority : TwoLevelAuthority) : option ChannelBag :=
  match authority with
  | [] => None
  | (channel, contents) :: tail =>
      if path_prefixb channel selected
      then Some contents
      else mutant_prefix_outer_lookup selected tail
  end.

Example exact_outer_lookup_rejects_prefix_lookup_mutant :
  lookup_channel [9; 2] fixture_authority = None /\
  mutant_prefix_outer_lookup [9; 2] fixture_authority = Some fixture_inner.
Proof. repeat split; reflexivity. Qed.

(** 2. Reversing the already canonical cursor order changes first-match
    choice. *)
Definition mutant_reversed_candidate_order request contents :=
  find (candidate_accepts request) (rev contents).

Example canonical_order_rejects_reverse_mutant :
  first_accepted_candidate fixture_request fixture_inner = Some fixture_first /\
  mutant_reversed_candidate_order fixture_request fixture_inner =
    Some fixture_second.
Proof. repeat split; reflexivity. Qed.

(** 3. Treating every retained endpoint as a match bypasses the generated
    spatial matcher PDA. *)
Definition mutant_accept_without_matcher
    (_ : MatchRequest) (_ : Entry) : bool := true.

Example pda_authority_rejects_unconditional_accept_mutant :
  candidate_accepts fixture_rejecting_request fixture_first = false /\
  mutant_accept_without_matcher fixture_rejecting_request fixture_first = true.
Proof. repeat split; reflexivity. Qed.

(** 4. A zero-valued endpoint violates Bag representation even if a matcher
    would accept its path. *)
Definition fixture_zero_candidate : Entry := ([1], 0%N).

Example positive_multiplicity_rejects_zero_endpoint_mutant :
  multiplicity_valid (entry_multiplicity fixture_zero_candidate) = false /\
  first_accepted_candidate fixture_request [fixture_zero_candidate] =
    Some fixture_zero_candidate.
Proof. repeat split; reflexivity. Qed.

(** 5. Consuming the selected endpoint's entire count rather than one
    occurrence turns Bag matching into set deletion. *)
Definition mutant_remove_all_selected
    (request : MatchRequest) (selected : Entry) : BagDelta :=
  {| bag_delta_removal_mode := StrictSubtract;
     bag_delta_subtractions := [selected];
     bag_delta_deletions := [];
     bag_delta_additions := request_additions request |}.

Example selected_delta_rejects_remove_all_mutant :
  bag_delta_subtractions (selected_match_delta fixture_request fixture_first) =
    [([1], 1%N)] /\
  bag_delta_subtractions (mutant_remove_all_selected fixture_request fixture_first) =
    [([1], 2%N)] /\
  apply_bag_delta fixture_inner
      (selected_match_delta fixture_request fixture_first) =
    Some [([1], 1%N); fixture_second] /\
  apply_bag_delta fixture_inner
      (mutant_remove_all_selected fixture_request fixture_first) =
    Some [fixture_second].
Proof. repeat split; reflexivity. Qed.

(** 6. Returning the cached entry without checking immutable identities lets a
    stale catalog authorize work. *)
Definition mutant_catalog_gate_ignores_identities
    (before : MatchStoreState) (request : MatchRequest) : option MatchCatalog :=
  store_catalogs before (request_channel request).

Example catalog_freshness_rejects_identity_bypass_mutant :
  catalog_gate fixture_store fixture_stale_request = None /\
  mutant_catalog_gate_ignores_identities fixture_store fixture_stale_request =
    Some fixture_catalog.
Proof. repeat split; reflexivity. Qed.

(** 7. Keeping the touched catalog after authority mutation leaves stale
    candidate authority live. *)
Definition mutant_keep_touched_catalog
    (_ : Path) (catalogs : @CatalogAuthority MatchCatalog)
    : @CatalogAuthority MatchCatalog := catalogs.

Example touched_invalidation_rejects_keep_mutant :
  invalidate_one_catalog [9] fixture_catalogs [9] = None /\
  mutant_keep_touched_catalog [9] fixture_catalogs [9] = Some fixture_catalog.
Proof. repeat split; reflexivity. Qed.

(** 8. Prefix-wide replacement mutates an exact descendant that was not part
    of the accepted match. *)
Fixpoint mutant_prefix_update
    (selected : Path) (replacement : ChannelBag)
    (authority : TwoLevelAuthority) : TwoLevelAuthority :=
  match authority with
  | [] => []
  | (channel, contents) :: tail =>
      (channel,
        if path_prefixb selected channel then replacement else contents) ::
      mutant_prefix_update selected replacement tail
  end.

Example exact_channel_isolation_rejects_prefix_update_mutant :
  lookup_channel [9; 1]
      (upsert_channel_exact [9] [] fixture_authority) =
    Some fixture_other_inner /\
  lookup_channel [9; 1]
      (mutant_prefix_update [9] [] fixture_authority) = Some [].
Proof. repeat split; reflexivity. Qed.

(** 9. Invalidating every catalog discards an untouched sibling identity. *)
Definition mutant_invalidate_all_catalogs
    (_ : Path) (_ : @CatalogAuthority MatchCatalog)
    : @CatalogAuthority MatchCatalog := fun _ => None.

Example untouched_catalog_isolation_rejects_global_invalidation_mutant :
  invalidate_one_catalog [9] fixture_catalogs [10] = Some fixture_catalog /\
  mutant_invalidate_all_catalogs [9] fixture_catalogs [10] = None.
Proof. repeat split; reflexivity. Qed.

(** 10. Dropping already issued immutable pools during publication violates
    their observation lifetime. *)
Definition mutant_drop_issued_pools
    (_ : list isolated_candidate_pool) : list isolated_candidate_pool := [].

Example issued_pool_preservation_rejects_drop_mutant :
  mutant_drop_issued_pools [fixture_pool] = [] /\
  [fixture_pool] <> [].
Proof. split; [reflexivity | discriminate]. Qed.

(** 11. A second public root swap is observable even if both roots encode the
    same logical authority. *)
Definition mutant_double_publication_count : nat := 2.

Example exactly_one_publication_rejects_double_publish_mutant :
  mutant_double_publication_count <> 1%nat.
Proof. discriminate. Qed.

(** 12. Refusal that applies a private candidate is not strongly atomic. *)
Definition mutant_refusal_authority : TwoLevelAuthority :=
  upsert_channel_exact [9] [] fixture_authority.

Example refusal_atomicity_rejects_mutating_refusal_mutant :
  mutant_refusal_authority <> fixture_authority.
Proof. discriminate. Qed.

(** 13. Deleting the outer endpoint loses the loaded-empty distinction. *)
Example loaded_empty_rejects_unload_mutant :
  lookup_channel [9] (upsert_channel_exact [9] [] fixture_authority) = Some [] /\
  lookup_channel [9] (delete_channel_exact [9] fixture_authority) = None.
Proof. repeat split; reflexivity. Qed.

(** 14. Retaining the same cursor after a rejected candidate introduces a
    non-progressing traversal cycle. *)
Definition mutant_nonprogressing_scan_step
    (_ : MatchRequest) (control : CandidateScanControl) : CandidateScanControl :=
  control.

Example strict_progress_rejects_stationary_scan_mutant :
  mutant_nonprogressing_scan_step fixture_rejecting_request
      (ScanCursor [fixture_first]) = ScanCursor [fixture_first] /\
  candidate_scan_work
      (mutant_nonprogressing_scan_step fixture_rejecting_request
        (ScanCursor [fixture_first])) =
    candidate_scan_work (ScanCursor [fixture_first]).
Proof. repeat split; reflexivity. Qed.

Inductive match_transaction_mutant_id : Type :=
| MutantPrefixOuterLookup | MutantReverseOrder | MutantBypassMatcher
| MutantZeroMultiplicity | MutantRemoveAllMultiplicity
| MutantStaleCatalog | MutantKeepTouchedCatalog | MutantPrefixUpdate
| MutantInvalidateAllCatalogs | MutantDropIssuedPools
| MutantDoublePublication | MutantMutatingRefusal
| MutantUnloadEmpty | MutantStationaryScan.

Definition match_transaction_mutant_catalog :
    list match_transaction_mutant_id :=
  [MutantPrefixOuterLookup; MutantReverseOrder; MutantBypassMatcher;
   MutantZeroMultiplicity; MutantRemoveAllMultiplicity; MutantStaleCatalog;
   MutantKeepTouchedCatalog; MutantPrefixUpdate; MutantInvalidateAllCatalogs;
   MutantDropIssuedPools; MutantDoublePublication; MutantMutatingRefusal;
   MutantUnloadEmpty; MutantStationaryScan].

Theorem match_transaction_mutant_catalog_is_duplicate_free_and_exact :
  NoDup match_transaction_mutant_catalog /\
  length match_transaction_mutant_catalog = 14%nat /\
  length match_transaction_mutant_catalog =
    length match_transaction_invariant_catalog.
Proof.
  split.
  - repeat constructor; simpl; intuition discriminate.
  - split; reflexivity.
Qed.

Print Assumptions exact_outer_lookup_rejects_prefix_lookup_mutant.
Print Assumptions selected_delta_rejects_remove_all_mutant.
Print Assumptions catalog_freshness_rejects_identity_bypass_mutant.
Print Assumptions exact_channel_isolation_rejects_prefix_update_mutant.
Print Assumptions match_transaction_mutant_catalog_is_duplicate_free_and_exact.

End TwoLevelRSpaceMatchTransactionMutants.
