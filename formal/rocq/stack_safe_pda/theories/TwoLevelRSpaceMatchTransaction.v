From Stdlib Require Import Bool List Arith Lia NArith.
From StackSafePDA Require Import
  StackSafePDA PathMapCanonicalStreamPublication
  TwoLevelRSpaceAuthority TwoLevelRSpaceOperations TwoLevelRSpaceDelta
  SpatialMatcher CandidateProductPathContinuation
  CandidateCatalogAuthorityIsolation.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceOperations.
Import TwoLevelRSpaceDelta.
Import CandidateProductPathContinuation.
Import CandidateCatalogAuthorityIsolation.
Set Implicit Arguments.

(** End-to-end refinement of one accepted RSpace match over the two-level
    PathMap authority. Lists are ghost observations of native canonical
    cursors; they do not authorize flattening a Bag, constructing [Vec<Par>],
    sorting candidate keys, or decoding RhoPath keys for selection. All
    fallible work precedes the sole one-root delta publication. *)
Module TwoLevelRSpaceMatchTransaction.

Record MatchCatalog : Type := MatchCatalogState {
  match_catalog_identity : nat;
  match_catalog_pool : isolated_candidate_pool;
  match_catalog_paths : list Path;
  match_catalog_order_is_ordinal :
    catalog_canonical_order (pool_catalog match_catalog_pool) =
      seq 0 (length match_catalog_paths)
}.

Definition match_catalog_authority_identity (catalog : MatchCatalog) : nat :=
  authority_identity
    (catalog_query_authority (pool_catalog (match_catalog_pool catalog))).

Definition MatchStoreState : Type :=
  @transactional_store_state MatchCatalog (list isolated_candidate_pool).

Record MatchRequest : Type := MatchRequestState {
  request_channel : Path;
  (** Snapshot identities are diagnostic provenance only.  Production commits
      do not reject a still-live exact selection merely because an unrelated
      datum changed the same channel's candidate catalog. *)
  request_catalog_identity : nat;
  request_catalog_authority_identity : nat;
  request_match_program : Entry -> @tree matcher_label;
  request_additions : list Entry
}.

Definition selected_authority_gate
    (before : MatchStoreState) (request : MatchRequest) (selected : Entry)
    : bool :=
  match lookup_channel (request_channel request) (store_authority before) with
  | None => false
  | Some contents =>
      N.leb 1%N
        (lookup_multiplicity_in_bag (entry_path selected) contents)
  end.

Definition candidate_match_result
    (request : MatchRequest) (candidate : Entry) : option free_map :=
  pda_spatial_match (request_match_program request candidate) empty_free_map.

Definition candidate_accepts
    (request : MatchRequest) (candidate : Entry) : bool :=
  match candidate_match_result request candidate with
  | Some _ => true
  | None => false
  end.

Definition first_accepted_candidate
    (request : MatchRequest) (contents : ChannelBag) : option Entry :=
  find (candidate_accepts request) contents.

(** Explicit one-control-state candidate PDA. The list is a proof-only cursor
    remainder; each production step advances one borrowed Bag endpoint. *)
Inductive CandidateScanControl : Type :=
| ScanCursor (remaining : ChannelBag)
| ScanAccepted (selected : Entry)
| ScanExhausted.

Definition candidate_scan_step request control : CandidateScanControl :=
  match control with
  | ScanCursor [] => ScanExhausted
  | ScanCursor (candidate :: tail) =>
      if candidate_accepts request candidate
      then ScanAccepted candidate else ScanCursor tail
  | ScanAccepted selected => ScanAccepted selected
  | ScanExhausted => ScanExhausted
  end.

Definition candidate_scan_work control : nat :=
  match control with
  | ScanCursor remaining => S (length remaining)
  | ScanAccepted _ | ScanExhausted => 0
  end.

Definition candidate_scan_observation request control : option Entry :=
  match control with
  | ScanCursor remaining => first_accepted_candidate request remaining
  | ScanAccepted selected => Some selected
  | ScanExhausted => None
  end.

Inductive candidate_scan_transition (request : MatchRequest)
    : CandidateScanControl -> CandidateScanControl -> Prop :=
| CandidateScanTransition : forall before after,
    candidate_scan_step request before = after ->
    before <> after ->
    candidate_scan_transition request before after.

Inductive candidate_scan_transitions (request : MatchRequest)
    : CandidateScanControl -> CandidateScanControl -> Prop :=
| CandidateScanTransitionsRefl : forall state,
    candidate_scan_transitions request state state
| CandidateScanTransitionsStep : forall first middle last,
    candidate_scan_transition request first middle ->
    candidate_scan_transitions request middle last ->
    candidate_scan_transitions request first last.

Theorem candidate_scan_step_preserves_exact_observation : forall request before,
  candidate_scan_observation request (candidate_scan_step request before) =
  candidate_scan_observation request before.
Proof.
  intros request before. destruct before as [remaining | selected |].
  - destruct remaining as [|candidate tail]; [reflexivity |].
    destruct (candidate_accepts request candidate) eqn:Haccepted.
    + unfold candidate_scan_step. rewrite Haccepted.
      cbn [candidate_scan_observation].
      unfold first_accepted_candidate. simpl. now rewrite Haccepted.
    + unfold candidate_scan_step. rewrite Haccepted.
      cbn [candidate_scan_observation].
      unfold first_accepted_candidate. simpl. now rewrite Haccepted.
  - reflexivity.
  - reflexivity.
Qed.

Theorem every_live_candidate_scan_transition_strictly_decreases_work : forall
    request before after,
  candidate_scan_transition request before after ->
  (candidate_scan_work after < candidate_scan_work before)%nat.
Proof.
  intros request before after Hstep.
  destruct Hstep as [scan_before scan_after Hadvance Hchanged].
  destruct scan_before as [remaining | selected |].
  - destruct remaining as [|candidate tail].
    + inversion Hadvance. simpl. lia.
    + cbn [candidate_scan_step] in Hadvance.
      destruct (candidate_accepts request candidate);
        inversion Hadvance; simpl; lia.
  - simpl in Hchanged. contradiction.
  - simpl in Hchanged. contradiction.
Qed.

Theorem candidate_scan_transitions_preserve_exact_observation : forall
    request first last,
  candidate_scan_transitions request first last ->
  candidate_scan_observation request last =
  candidate_scan_observation request first.
Proof.
  intros request first last Hsteps. induction Hsteps; [reflexivity |].
  rewrite IHHsteps. inversion H; subst.
  now apply candidate_scan_step_preserves_exact_observation.
Qed.

Lemma first_accepted_candidate_is_in_canonical_cursor_order : forall
    request contents selected,
  first_accepted_candidate request contents = Some selected ->
  In selected contents.
Proof.
  intros request contents. induction contents as [|candidate tail IH];
    intros selected Hselected; unfold first_accepted_candidate in Hselected.
  - simpl in Hselected. discriminate.
  - change
      ((if candidate_accepts request candidate
        then Some candidate
        else find (candidate_accepts request) tail) = Some selected)
      in Hselected.
    destruct (candidate_accepts request candidate) eqn:Haccepted.
    + inversion Hselected; now left.
    + right. apply IH. exact Hselected.
Qed.

Lemma canonical_member_has_valid_positive_multiplicity : forall
    contents selected,
  canonical_entries contents -> In selected contents ->
  path_valid (entry_path selected) = true /\
  multiplicity_valid (entry_multiplicity selected) = true.
Proof.
  intros contents selected Hcanonical Hin.
  induction Hcanonical as
      [|path multiplicity tail Hpath Hmultiplicity Hafter Htail IH].
  - contradiction.
  - destruct Hin as [Heq | Hin].
    + subst selected. exact (conj Hpath Hmultiplicity).
    + now apply IH.
Qed.

Definition selected_match_delta
    (request : MatchRequest) (selected : Entry) : BagDelta :=
  {| bag_delta_removal_mode := StrictSubtract;
     bag_delta_subtractions := [(entry_path selected, 1%N)];
     bag_delta_deletions := [];
     bag_delta_additions := request_additions request |}.

Record PreparedMatch
    (before : MatchStoreState) (request : MatchRequest) : Type := {
  prepared_match_catalog : MatchCatalog;
  prepared_match_inner : ChannelBag;
  prepared_match_selected : Entry;
  prepared_match_bindings : free_map;
  prepared_match_continuation : path_continuation;
  prepared_match_checkpoint : path_checkpoint;
  prepared_match_pool_was_issued :
    In (match_catalog_pool prepared_match_catalog)
      (store_external_snapshots before);
  prepared_match_selection_is_live :
    selected_authority_gate before request prepared_match_selected = true;
  prepared_match_exact_outer_lookup :
    lookup_channel (request_channel request) (store_authority before) =
      Some prepared_match_inner;
  prepared_match_catalog_order_is_exact :
    match_catalog_paths prepared_match_catalog = bag_paths prepared_match_inner;
  prepared_match_is_first_in_order :
    first_accepted_candidate request prepared_match_inner =
      Some prepared_match_selected;
  prepared_match_pda_accepts :
    candidate_match_result request prepared_match_selected =
      Some prepared_match_bindings;
  prepared_match_checkpoint_is_exact :
    make_checkpoint
      (catalog_query_authority
        (pool_catalog (match_catalog_pool prepared_match_catalog)))
      prepared_match_continuation = Some prepared_match_checkpoint;
  prepared_match_channel_delta : PreparedChannelDelta (store_authority before);
  prepared_match_delta_targets_exact_channel :
    prepared_delta_channel prepared_match_channel_delta = request_channel request;
  prepared_match_delta_is_selected_multiplicity_change :
    prepared_delta_change prepared_match_channel_delta =
      selected_match_delta request prepared_match_selected
}.

Definition prepared_match_as_rspace_delta {before request}
    (prepared : PreparedMatch before request)
    : PreparedRSpaceDelta (store_authority before).
Proof.
  refine {| prepared_channel_deltas := [prepared_match_channel_delta prepared] |}.
  cbn [prepared_delta_channels]. repeat constructor; simpl; tauto.
Defined.

Definition publish_prepared_match {before request}
    (prepared : PreparedMatch before request) : MatchStoreState :=
  publish_prepared_rspace_delta before (prepared_match_as_rspace_delta prepared).

Lemma prepared_match_batch_touches_exact_channel : forall
    before request (prepared : PreparedMatch before request),
  In (request_channel request)
    (touched_channels
      (prepared_operations
        (prepared_rspace_delta_as_batch
          (prepared_match_as_rspace_delta prepared)))).
Proof.
  intros before request prepared.
  change (In (request_channel request)
    (touched_channels
      (prepared_delta_operations [prepared_match_channel_delta prepared]))).
  rewrite prepared_delta_operation_channels_are_exact.
  cbn [prepared_delta_channels].
  left. exact (prepared_match_delta_targets_exact_channel prepared).
Qed.

Lemma prepared_match_batch_does_not_touch_other_channel : forall
    before request (prepared : PreparedMatch before request) other,
  other <> request_channel request ->
  ~ In other
    (touched_channels
      (prepared_operations
        (prepared_rspace_delta_as_batch
          (prepared_match_as_rspace_delta prepared)))).
Proof.
  intros before request prepared other Hdifferent.
  change (~ In other
    (touched_channels
      (prepared_delta_operations [prepared_match_channel_delta prepared]))).
  rewrite prepared_delta_operation_channels_are_exact.
  unfold prepared_delta_channels. simpl.
  rewrite (prepared_match_delta_targets_exact_channel prepared). simpl.
  intros [Heq | Hfalse].
  - apply Hdifferent. symmetry. exact Heq.
  - contradiction.
Qed.

Inductive MatchDecision : Type := MatchRefused | MatchPublished.

Record MatchOutcome : Type := MatchOutcomeState {
  match_outcome_decision : MatchDecision;
  match_outcome_state : MatchStoreState;
  match_outcome_selected : option Entry;
  match_outcome_publication_count : nat
}.

Inductive match_transaction_transition
    (before : MatchStoreState) (request : MatchRequest)
    : option (PreparedMatch before request) -> MatchOutcome -> Prop :=
| RefuseMatchTransaction :
    @match_transaction_transition before request None
      (MatchOutcomeState MatchRefused before None 0)
| PublishMatchTransaction : forall prepared,
    @match_transaction_transition before request (Some prepared)
      (MatchOutcomeState MatchPublished (publish_prepared_match prepared)
        (Some (prepared_match_selected prepared)) 1).

Theorem selected_authority_gate_rejects_unloaded_channel : forall
    before request selected,
  lookup_channel (request_channel request) (store_authority before) = None ->
  selected_authority_gate before request selected = false.
Proof. intros. unfold selected_authority_gate. now rewrite H. Qed.

Theorem selected_authority_gate_rejects_absent_or_exhausted_identity : forall
    before request selected contents,
  lookup_channel (request_channel request) (store_authority before) =
    Some contents ->
  lookup_multiplicity_in_bag (entry_path selected) contents = 0%N ->
  selected_authority_gate before request selected = false.
Proof.
  intros before request selected contents Hchannel Hexhausted.
  unfold selected_authority_gate. rewrite Hchannel, Hexhausted. reflexivity.
Qed.

Theorem stale_selection_authority_cannot_prepare_a_match : forall
    before request selected,
  selected_authority_gate before request selected = false ->
  forall prepared : PreparedMatch before request,
    prepared_match_selected prepared = selected -> False.
Proof.
  intros before request selected Hstale prepared Hselected.
  pose proof (prepared_match_selection_is_live prepared) as Hlive.
  rewrite Hselected in Hlive. rewrite Hstale in Hlive. discriminate.
Qed.

Theorem unrelated_catalog_identity_changes_do_not_reject_a_live_selection : forall
    before request selected,
  selected_authority_gate before request selected = true ->
  forall catalog_identity authority_identity,
    selected_authority_gate before
      (MatchRequestState
        (request_channel request) catalog_identity authority_identity
        (request_match_program request) (request_additions request))
      selected = true.
Proof. intros. exact H. Qed.

Theorem prepared_match_selected_candidate_is_authoritative : forall
    before request (prepared : PreparedMatch before request),
  store_state_well_formed before ->
  In (prepared_match_selected prepared) (prepared_match_inner prepared) /\
  In (entry_path (prepared_match_selected prepared))
    (match_catalog_paths (prepared_match_catalog prepared)) /\
  path_valid (entry_path (prepared_match_selected prepared)) = true /\
  multiplicity_valid (entry_multiplicity (prepared_match_selected prepared)) = true /\
  recursive_spatial_match
      (request_match_program request (prepared_match_selected prepared))
      empty_free_map = Some (prepared_match_bindings prepared).
Proof.
  intros before request prepared Hwell.
  assert (Hin : In (prepared_match_selected prepared)
      (prepared_match_inner prepared)).
  { eapply first_accepted_candidate_is_in_canonical_cursor_order.
    exact (prepared_match_is_first_in_order prepared). }
  assert (Hcanonical : canonical_entries (prepared_match_inner prepared)).
  { eapply canonical_lookup_returns_canonical_bag.
    - exact (proj1 Hwell).
    - exact (prepared_match_exact_outer_lookup prepared). }
  pose proof (@canonical_member_has_valid_positive_multiplicity
    (prepared_match_inner prepared) (prepared_match_selected prepared)
    Hcanonical Hin) as Hvalid.
  repeat split.
  - exact Hin.
  - rewrite (prepared_match_catalog_order_is_exact prepared).
    apply in_map. exact Hin.
  - exact (proj1 Hvalid).
  - exact (proj2 Hvalid).
  - rewrite <- spatial_match_pda_equivalent_to_recursive_match.
    exact (prepared_match_pda_accepts prepared).
Qed.

Theorem accepted_match_transaction_capstone : forall
    before request prepared outcome,
  store_state_well_formed before ->
  @match_transaction_transition before request (Some prepared) outcome ->
  lookup_channel (request_channel request)
      (store_authority (match_outcome_state outcome)) =
    Some (prepared_delta_output (prepared_match_channel_delta prepared)) /\
  (forall other, other <> request_channel request ->
    lookup_channel other (store_authority (match_outcome_state outcome)) =
    lookup_channel other (store_authority before)) /\
  store_catalogs (match_outcome_state outcome) (request_channel request) = None /\
  store_external_snapshots (match_outcome_state outcome) =
    store_external_snapshots before /\
  store_publication_generation (match_outcome_state outcome) =
    S (store_publication_generation before) /\
  match_outcome_selected outcome = Some (prepared_match_selected prepared) /\
  match_outcome_publication_count outcome = 1%nat /\
  prepared_delta_change (prepared_match_channel_delta prepared) =
    selected_match_delta request (prepared_match_selected prepared).
Proof.
  intros before request prepared outcome Hwell Htransition.
  inversion Htransition; subst outcome; clear Htransition.
  assert (Hhead : prepared_channel_deltas
      (prepared_match_as_rspace_delta prepared) =
      [prepared_match_channel_delta prepared]) by reflexivity.
  repeat split.
  - unfold publish_prepared_match, publish_prepared_rspace_delta,
      publish_prepared_channel_batch; cbn.
    pose proof (@prepared_rspace_delta_head_output_is_exact
      (store_authority before) (prepared_match_as_rspace_delta prepared)
      (prepared_match_channel_delta prepared) [] Hhead) as Hlookup.
    cbn in Hlookup.
    rewrite (prepared_match_delta_targets_exact_channel prepared) in Hlookup.
    rewrite (prepared_match_delta_targets_exact_channel prepared).
    exact Hlookup.
  - intros other Hdifferent.
    unfold publish_prepared_match, publish_prepared_rspace_delta,
      publish_prepared_channel_batch; cbn.
    rewrite (prepared_match_delta_targets_exact_channel prepared).
    apply upsert_preserves_every_other_exact_channel. exact Hdifferent.
  - unfold publish_prepared_match.
    apply successful_publication_invalidates_every_touched_catalog.
    change (In (request_channel request)
      (touched_channels
        (prepared_delta_operations [prepared_match_channel_delta prepared]))).
    rewrite prepared_delta_operation_channels_are_exact.
    cbn [prepared_delta_channels].
    left. exact (prepared_match_delta_targets_exact_channel prepared).
  - exact (prepared_match_delta_is_selected_multiplicity_change prepared).
Qed.

Theorem refused_match_transaction_is_strongly_atomic : forall
    before request outcome,
  @match_transaction_transition before request None outcome ->
  match_outcome_state outcome = before /\
  match_outcome_selected outcome = None /\
  match_outcome_publication_count outcome = 0%nat /\
  match_outcome_decision outcome = MatchRefused.
Proof.
  intros before request outcome Htransition.
  inversion Htransition; subst. repeat split; reflexivity.
Qed.

Theorem accepted_match_preserves_loaded_empty_channel : forall
    before request prepared outcome,
  @match_transaction_transition before request (Some prepared) outcome ->
  prepared_delta_output (prepared_match_channel_delta prepared) = [] ->
  loaded (store_authority (match_outcome_state outcome)) (request_channel request).
Proof.
  intros before request prepared outcome Htransition Hempty.
  inversion Htransition; subst outcome; clear Htransition. exists [].
  unfold publish_prepared_match, publish_prepared_rspace_delta,
    publish_prepared_channel_batch; cbn.
  rewrite (prepared_match_delta_targets_exact_channel prepared).
  rewrite Hempty. apply upsert_target_lookup_is_exact.
Qed.

Theorem accepted_match_isolates_prefix_related_channels : forall
    before request prepared outcome other,
  @match_transaction_transition before request (Some prepared) outcome ->
  other <> request_channel request ->
  (exists suffix, other = request_channel request ++ suffix) \/
  (exists suffix, request_channel request = other ++ suffix) ->
  lookup_channel other (store_authority (match_outcome_state outcome)) =
  lookup_channel other (store_authority before).
Proof.
  intros before request prepared outcome other Htransition Hdifferent _.
  inversion Htransition; subst outcome; clear Htransition.
  unfold publish_prepared_match, publish_prepared_rspace_delta,
    publish_prepared_channel_batch; cbn.
  rewrite (prepared_match_delta_targets_exact_channel prepared).
  apply upsert_preserves_every_other_exact_channel. exact Hdifferent.
Qed.

Theorem accepted_match_invalidates_the_formal_conservative_catalog : forall
    (before : MatchStoreState) request (prepared : PreparedMatch before request),
  store_catalogs (publish_prepared_match prepared) (request_channel request) = None.
Proof.
  intros before request prepared.
  unfold publish_prepared_match.
  apply successful_publication_invalidates_every_touched_catalog.
  apply prepared_match_batch_touches_exact_channel.
Qed.

Theorem accepted_match_preserves_every_untouched_catalog_identity : forall
    (before : MatchStoreState) request
    (prepared : PreparedMatch before request) other,
  other <> request_channel request ->
  store_catalogs (publish_prepared_match prepared) other = store_catalogs before other.
Proof.
  intros before request prepared other Hdifferent.
  unfold publish_prepared_match.
  apply successful_publication_preserves_untouched_catalog_identity.
  now apply prepared_match_batch_does_not_touch_other_channel.
Qed.

Inductive match_transaction_invariant_id : Type :=
| MatchInvExactOuterLookup | MatchInvCanonicalCandidateOrder
| MatchInvPdaMatcherAuthority | MatchInvPositiveMultiplicity
| MatchInvSelectedBagDelta | MatchInvSelectedAuthorityFreshness
| MatchInvTouchedCatalogInvalidation | MatchInvUntouchedChannelIsolation
| MatchInvUntouchedCatalogIsolation | MatchInvIssuedPoolPreservation
| MatchInvExactlyOnePublication | MatchInvRefusalAtomicity
| MatchInvLoadedEmptyPreservation | MatchInvStrictScanProgress.

Definition match_transaction_invariant_catalog :
    list match_transaction_invariant_id :=
  [MatchInvExactOuterLookup; MatchInvCanonicalCandidateOrder;
   MatchInvPdaMatcherAuthority; MatchInvPositiveMultiplicity;
   MatchInvSelectedBagDelta; MatchInvSelectedAuthorityFreshness;
   MatchInvTouchedCatalogInvalidation; MatchInvUntouchedChannelIsolation;
   MatchInvUntouchedCatalogIsolation; MatchInvIssuedPoolPreservation;
   MatchInvExactlyOnePublication; MatchInvRefusalAtomicity;
   MatchInvLoadedEmptyPreservation; MatchInvStrictScanProgress].

Theorem match_transaction_invariant_catalog_is_duplicate_free_and_exact :
  NoDup match_transaction_invariant_catalog /\
  length match_transaction_invariant_catalog = 14%nat.
Proof. split; [repeat constructor; simpl; intuition discriminate | reflexivity]. Qed.

Print Assumptions candidate_scan_transitions_preserve_exact_observation.
Print Assumptions prepared_match_selected_candidate_is_authoritative.
Print Assumptions accepted_match_transaction_capstone.
Print Assumptions refused_match_transaction_is_strongly_atomic.
Print Assumptions selected_authority_gate_rejects_absent_or_exhausted_identity.
Print Assumptions unrelated_catalog_identity_changes_do_not_reject_a_live_selection.
Print Assumptions accepted_match_invalidates_the_formal_conservative_catalog.
Print Assumptions match_transaction_invariant_catalog_is_duplicate_free_and_exact.

End TwoLevelRSpaceMatchTransaction.
