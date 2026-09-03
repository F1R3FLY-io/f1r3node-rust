From Stdlib Require Import
  Bool List Arith Lia NArith ZArith Sorting.Sorted.
From StackSafePDA Require Import
  StackSafePDA SpatialMatcher PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority TwoLevelRSpaceOperations TwoLevelRSpaceDelta
  TwoLevelRSpaceMatchPreparation TwoLevelRSpaceContinuationAuthority.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceOperations.
Import TwoLevelRSpaceDelta.
Import TwoLevelRSpaceMatchPreparation.
Import TwoLevelRSpaceContinuationAuthority.
Set Implicit Arguments.
Local Open Scope nat_scope.

(** End-to-end refinement of one RSpace match transaction. Candidate lists
    are ghost observations of ordered PathMap cursors. They are not flattened
    datum bags, and neither implementation below constructs a Cartesian
    product. *)
Module TwoLevelRSpaceMatchTransaction.

(** Query-pruned cursor observations ----------------------------------- *)

Inductive OrderPreservingSubsequence {A : Type} : list A -> list A -> Prop :=
| SubsequenceNil : OrderPreservingSubsequence [] []
| SubsequenceDrop : forall head full selected,
    OrderPreservingSubsequence selected full ->
    OrderPreservingSubsequence selected (head :: full)
| SubsequenceKeep : forall head full selected,
    OrderPreservingSubsequence selected full ->
    OrderPreservingSubsequence (head :: selected) (head :: full).

Lemma subsequence_length_is_bounded : forall A (selected full : list A),
  OrderPreservingSubsequence selected full ->
  (length selected <= length full)%nat.
Proof. intros A selected full H; induction H; simpl; lia. Qed.

Lemma subsequence_member_is_full_member : forall A (selected full : list A) x,
  OrderPreservingSubsequence selected full -> In x selected -> In x full.
Proof.
  intros A selected full x Hsub. induction Hsub; intro Hin.
  - contradiction.
  - right. now apply IHHsub.
  - destruct Hin as [Heq | Hin]; [now left | right; now apply IHHsub].
Qed.

Record MatchProgram : Type := MatchProgramState {
  match_program_tree : Path -> MatchCandidate -> @tree matcher_label;
  match_program_guard : free_map -> bool
}.

Definition candidate_spatial_result
    (program : MatchProgram) (channel : Path)
    (candidate : OrdinalCandidate) (bindings : free_map) : option free_map :=
  pda_spatial_match
    (match_program_tree program channel (ordinal_candidate_value candidate))
    bindings.

Definition candidate_recursive_spatial_result
    (program : MatchProgram) (channel : Path)
    (candidate : OrdinalCandidate) (bindings : free_map) : option free_map :=
  recursive_spatial_match
    (match_program_tree program channel (ordinal_candidate_value candidate))
    bindings.

(* Adapters from the transaction vocabulary to the current exact candidate
   representation.  These expose cursor metadata directly; they do not build
   a flattened datum projection. *)
Inductive CandidateOrigin : Type :=
| CandidateFresh
| CandidateStored (ordinal : N) (absolute_path : Path).

Definition candidate_origin (candidate : MatchCandidate) : CandidateOrigin :=
  match candidate_address candidate with
  | FreshAddress => CandidateFresh
  | StoredAddress absolute => CandidateStored 0%N absolute
  end.

Definition candidate_channel (candidate : MatchCandidate) : Path :=
  candidate_channel_marker candidate.

Definition candidate_datum (candidate : MatchCandidate) : Path :=
  candidate_relative_datum candidate.

Definition lookup_catalog (channel : Path) (catalogs : CandidateCatalogSet)
    : option CandidateCatalog :=
  lookup_candidate_catalog channel catalogs.

Definition combined_candidate_projection
    (arrival : option MatchCandidate) (catalog : CandidateCatalog)
    : list OrdinalCandidate :=
  enumerate_candidates_from 0%N
    (match arrival with
     | Some fresh => fresh :: candidate_catalog_authority catalog
     | None => candidate_catalog_authority catalog
     end).

Lemma lookup_candidate_catalog_some_is_member : forall channel catalogs catalog,
  lookup_candidate_catalog channel catalogs = Some catalog ->
  In catalog (candidate_catalogs catalogs).
Proof.
  intros channel catalogs catalog Hlookup.
  revert Hlookup.
  destruct catalogs as [requested unique catalogs].
  simpl in *.
  induction catalogs as [|head tail IH]; simpl in *; [intros Hlookup; discriminate|].
  destruct (path_eq_dec channel (candidate_catalog_channel head)) as [Heq|Hneq].
  - intros Hlookup. cbv [lookup_candidate_catalog
      lookup_candidate_catalog_list] in Hlookup.
    rewrite Heq in Hlookup. simpl in Hlookup.
    destruct (path_eq_dec (candidate_catalog_channel head)
      (candidate_catalog_channel head)) as [_|Hne].
    + simpl in Hlookup. inversion Hlookup; subst catalog; exact (or_introl eq_refl).
    + contradiction.
  - intros Hlookup.
    change ((if path_eq_dec channel (candidate_catalog_channel head)
      then Some head else lookup_candidate_catalog_list channel tail) =
      Some catalog) in Hlookup.
    destruct (path_eq_dec channel (candidate_catalog_channel head)) as [Heq|Hne].
    + contradiction.
    + right. apply IH. exact Hlookup.
Qed.

Record QueryPrunedScan
    (program : MatchProgram) (channel : Path)
    (full : list OrdinalCandidate) : Type := QueryPrunedScanState {
  query_pruned_projection : list OrdinalCandidate;
  query_pruned_order_is_preserved :
    OrderPreservingSubsequence query_pruned_projection full;
  query_pruned_is_match_complete :
    forall candidate bindings next,
      In candidate full ->
      candidate_spatial_result program channel candidate bindings = Some next ->
      In candidate query_pruned_projection
}.

(** Semantic production reservation identity. Catalog ordinals are cursor
    navigation only and never authorize sharing or exclusion. The stored path
    is the absolute datum address supplied by PREPARE. *)
Inductive ReservationIdentity : Type :=
| FreshReservation
| StoredReservation (absolute_datum_path : Path).

Definition reservation_identity_eq_dec : forall left right : ReservationIdentity,
    {left = right} + {left <> right}.
Proof. decide equality; apply path_eq_dec. Defined.

Record CandidateSearch : Type := CandidateSearchState {
  search_observed_authority : TwoLevelAuthority;
  search_requested_channels : list Path;
  search_catalogs : CandidateCatalogSet;
  search_arrival : option MatchCandidate;
  search_excluded_count : ReservationIdentity -> Multiplicity;
  search_fresh_available : bool;
  search_program : MatchProgram;
  search_scan : forall channel catalog,
    QueryPrunedScan search_program channel
      (combined_candidate_projection search_arrival catalog);
  search_exclusions_are_capacity_bounded :
    forall channel catalog candidate,
      lookup_catalog channel search_catalogs = Some catalog ->
      In candidate (combined_candidate_projection search_arrival catalog) ->
      (search_excluded_count
        (match candidate_origin (ordinal_candidate_value candidate) with
         | CandidateFresh => FreshReservation
         | CandidateStored _ absolute_path => StoredReservation absolute_path
         end) <=
       match candidate_origin (ordinal_candidate_value candidate) with
       | CandidateFresh => 1%N
       | CandidateStored _ _ =>
           lookup_multiplicity search_observed_authority
             (candidate_channel (ordinal_candidate_value candidate))
             (candidate_datum (ordinal_candidate_value candidate))
       end)%N;
  search_exclusions_are_all_or_none :
    forall channel catalog candidate,
      lookup_catalog channel search_catalogs = Some catalog ->
      In candidate (combined_candidate_projection search_arrival catalog) ->
      search_excluded_count
        (match candidate_origin (ordinal_candidate_value candidate) with
         | CandidateFresh => FreshReservation
         | CandidateStored _ absolute_path => StoredReservation absolute_path
         end) = 0%N \/
      search_excluded_count
        (match candidate_origin (ordinal_candidate_value candidate) with
         | CandidateFresh => FreshReservation
         | CandidateStored _ absolute_path => StoredReservation absolute_path
         end) =
       match candidate_origin (ordinal_candidate_value candidate) with
       | CandidateFresh => 1%N
       | CandidateStored _ _ =>
           lookup_multiplicity search_observed_authority
             (candidate_channel (ordinal_candidate_value candidate))
             (candidate_datum (ordinal_candidate_value candidate))
       end;
  search_fresh_replay_filter_is_exact :
    forall channel catalog fresh,
      lookup_catalog channel search_catalogs = Some catalog ->
      hd_error (combined_candidate_projection search_arrival catalog) =
        Some fresh ->
      candidate_origin (ordinal_candidate_value fresh) = CandidateFresh ->
      search_excluded_count FreshReservation =
        if search_fresh_available then 0%N else 1%N
}.

Definition search_projection
    (search : CandidateSearch) (channel : Path) (catalog : CandidateCatalog)
    : list OrdinalCandidate :=
  query_pruned_projection (search_scan search channel catalog).

Lemma search_projection_is_order_preserving : forall search channel catalog,
  OrderPreservingSubsequence
    (search_projection search channel catalog)
    (combined_candidate_projection (search_arrival search) catalog).
Proof. intros; apply query_pruned_order_is_preserved. Qed.

Lemma query_pruning_cannot_omit_a_spatial_match : forall
    search channel catalog candidate bindings next,
  In candidate
    (combined_candidate_projection (search_arrival search) catalog) ->
  candidate_spatial_result (search_program search) channel candidate bindings =
      Some next ->
  In candidate (search_projection search channel catalog).
Proof. intros; eapply query_pruned_is_match_complete; eassumption. Qed.

(** Shared exact-channel reservation authority ------------------------- *)

Definition ReservationKey : Type := ReservationIdentity.
Definition reservation_key_eq_dec := reservation_identity_eq_dec.

Definition ReservationAuthority : Type := ReservationKey -> Multiplicity.

Record ReservationTicket : Type := ReservationTicketState {
  reservation_ticket_key : ReservationKey;
  reservation_previous_count : Multiplicity;
  reservation_previous_authority : ReservationAuthority
}.

Definition candidate_reservation_key (candidate : OrdinalCandidate)
    : ReservationKey :=
  match candidate_origin (ordinal_candidate_value candidate) with
  | CandidateFresh => FreshReservation
  | CandidateStored _ absolute_path => StoredReservation absolute_path
  end.

Definition set_reservation_count
    (before : ReservationAuthority) (selected : ReservationKey)
    (count : Multiplicity) : ReservationAuthority :=
  fun key => if reservation_key_eq_dec key selected then count else before key.

Definition initial_reservations (search : CandidateSearch) : ReservationAuthority :=
  search_excluded_count search.

Definition candidate_capacity
    (search : CandidateSearch) (candidate : OrdinalCandidate) : Multiplicity :=
  let value := ordinal_candidate_value candidate in
  match candidate_origin value with
  | CandidateFresh => 1%N
  | CandidateStored _ _ =>
      lookup_multiplicity (search_observed_authority search)
        (candidate_channel value) (candidate_datum value)
  end.

Definition candidate_available
    (search : CandidateSearch) (channel : Path) (candidate : OrdinalCandidate)
    (reservations : ReservationAuthority) : bool :=
  match candidate_origin (ordinal_candidate_value candidate) with
  | CandidateFresh =>
      if search_fresh_available search
      then N.ltb
        (reservations FreshReservation) (candidate_capacity search candidate)
      else false
  | CandidateStored _ _ =>
      N.ltb
        (reservations (candidate_reservation_key candidate))
        (candidate_capacity search candidate)
  end.

Definition reserve_candidate
    (search : CandidateSearch) (channel : Path) (candidate : OrdinalCandidate)
    (reservations : ReservationAuthority)
    : option (ReservationAuthority * option ReservationTicket) :=
  if candidate_persistent (ordinal_candidate_value candidate)
  then Some (reservations, None)
  else
    let key := candidate_reservation_key candidate in
    let previous := reservations key in
    if N.ltb previous (candidate_capacity search candidate)
    then Some
      (set_reservation_count reservations key (N.succ previous),
       Some (ReservationTicketState key previous reservations))
    else None.

Lemma candidate_available_implies_below_capacity : forall
    search channel candidate reservations,
  candidate_available search channel candidate reservations = true ->
  (reservations (candidate_reservation_key candidate) <
    candidate_capacity search candidate)%N.
Proof.
  intros search channel candidate reservations Havailable.
  unfold candidate_available, candidate_reservation_key in *.
  destruct (candidate_origin (ordinal_candidate_value candidate)); simpl in *.
  - destruct (search_fresh_available search); [now apply N.ltb_lt|discriminate].
  - now apply N.ltb_lt.
Qed.

Definition rollback_reservation
    (ticket : option ReservationTicket) (reservations : ReservationAuthority)
    : ReservationAuthority :=
  match ticket with
  | None => reservations
  | Some selected => reservation_previous_authority selected
  end.

Lemma set_reservation_count_target : forall before key count,
  set_reservation_count before key count key = count.
Proof.
  intros. unfold set_reservation_count.
  destruct (reservation_key_eq_dec key key); [reflexivity|contradiction].
Qed.

Lemma set_reservation_count_other : forall before key count other,
  other <> key ->
  set_reservation_count before key count other = before other.
Proof.
  intros. unfold set_reservation_count.
  destruct (reservation_key_eq_dec other key); [contradiction|reflexivity].
Qed.

Lemma persistent_candidate_reserves_nothing : forall search channel candidate reserved,
  candidate_persistent (ordinal_candidate_value candidate) = true ->
  reserve_candidate search channel candidate reserved = Some (reserved, None).
Proof. intros. unfold reserve_candidate. now rewrite H. Qed.

Lemma successful_nonpersistent_reservation_is_exact : forall
    search channel candidate before after ticket,
  candidate_persistent (ordinal_candidate_value candidate) = false ->
  reserve_candidate search channel candidate before = Some (after, Some ticket) ->
  reservation_ticket_key ticket = candidate_reservation_key candidate /\
  reservation_previous_count ticket =
      before (candidate_reservation_key candidate) /\
  after (candidate_reservation_key candidate) =
      N.succ (before (candidate_reservation_key candidate)) /\
  rollback_reservation (Some ticket) after = before.
Proof.
  intros search channel candidate before after ticket Hpersistent Hreserve.
  unfold reserve_candidate in Hreserve. rewrite Hpersistent in Hreserve.
  destruct (N.ltb
    (before (candidate_reservation_key candidate))
    (candidate_capacity search candidate)) eqn:Hcapacity; [|discriminate].
  inversion Hreserve; subst. repeat split; try reflexivity.
  apply set_reservation_count_target.
Qed.

Lemma nonpersistent_candidate_is_exhausted_at_capacity : forall
    search channel candidate reservations,
  candidate_persistent (ordinal_candidate_value candidate) = false ->
  reservations (candidate_reservation_key candidate) =
      candidate_capacity search candidate ->
  reserve_candidate search channel candidate reservations = None.
Proof.
  intros. unfold reserve_candidate. rewrite H, H0, N.ltb_irrefl. reflexivity.
Qed.

Lemma nonpersistent_candidate_remains_available_below_capacity : forall
    search channel candidate reservations,
  candidate_persistent (ordinal_candidate_value candidate) = false ->
  (candidate_origin (ordinal_candidate_value candidate) = CandidateFresh ->
    search_fresh_available search = true) ->
  (reservations (candidate_reservation_key candidate) <
    candidate_capacity search candidate)%N ->
  candidate_available search channel candidate reservations = true.
Proof.
  intros search channel candidate reservations Hpersistent Hfresh Hbelow.
  unfold candidate_available, candidate_reservation_key in *.
  destruct (candidate_origin (ordinal_candidate_value candidate))
    as [|stored_channel absolute_path] eqn:Horigin.
  - rewrite Hfresh by reflexivity. now apply N.ltb_lt.
  - now apply N.ltb_lt.
Qed.

Lemma successful_reservation_rolls_back_exactly : forall
    search channel candidate before after ticket,
  reserve_candidate search channel candidate before = Some (after, ticket) ->
  rollback_reservation ticket after = before.
Proof.
  intros search channel candidate before after ticket Hreserve.
  destruct (candidate_persistent (ordinal_candidate_value candidate))
    eqn:Hpersistent.
  - rewrite (@persistent_candidate_reserves_nothing
      search channel candidate before Hpersistent) in Hreserve.
    inversion Hreserve. reflexivity.
  - destruct ticket as [ticket|].
    + pose proof (@successful_nonpersistent_reservation_is_exact
        search channel candidate before after ticket Hpersistent Hreserve)
        as [_ [_ [_ Hrollback]]]. exact Hrollback.
    + unfold reserve_candidate in Hreserve. rewrite Hpersistent in Hreserve.
      destruct (N.ltb
        (before (candidate_reservation_key candidate))
        (candidate_capacity search candidate)); discriminate.
Qed.

Theorem nonpersistent_multiplicity_can_reserve_repeatedly_below_capacity : forall
    search channel candidate before after_first first_ticket,
  candidate_persistent (ordinal_candidate_value candidate) = false ->
  reserve_candidate search channel candidate before =
      Some (after_first, Some first_ticket) ->
  candidate_available search channel candidate after_first = true ->
  exists after_second second_ticket,
    reserve_candidate search channel candidate after_first =
      Some (after_second, Some second_ticket) /\
    rollback_reservation (Some second_ticket) after_second = after_first /\
    rollback_reservation (Some first_ticket) after_first = before.
Proof.
  intros search channel candidate before after_first first_ticket
    Hpersistent Hfirst Havailable.
  pose proof (@candidate_available_implies_below_capacity
    search channel candidate after_first Havailable) as Hbelow.
  unfold reserve_candidate. rewrite Hpersistent, (proj2 (N.ltb_lt _ _) Hbelow).
  eexists. eexists. split; [reflexivity|]. split.
  - reflexivity.
  - now apply (@successful_reservation_rolls_back_exactly
      search channel candidate before after_first (Some first_ticket)).
Qed.

Theorem eligible_persistent_candidate_is_reusable_without_reservation : forall
    search channel candidate reservations,
  candidate_persistent (ordinal_candidate_value candidate) = true ->
  candidate_available search channel candidate reservations = true ->
  reserve_candidate search channel candidate reservations =
      Some (reservations, None) /\
  reserve_candidate search channel candidate reservations =
      Some (reservations, None).
Proof.
  intros. split; apply persistent_candidate_reserves_nothing; assumption.
Qed.

Theorem replay_exclusions_are_never_partial : forall
    search channel catalog candidate,
  lookup_catalog channel (search_catalogs search) = Some catalog ->
  In candidate
    (combined_candidate_projection (search_arrival search) catalog) ->
  search_excluded_count search (candidate_reservation_key candidate) = 0%N \/
  search_excluded_count search (candidate_reservation_key candidate) =
    candidate_capacity search candidate.
Proof.
  intros search channel catalog candidate Hlookup Hin.
  apply (search_exclusions_are_all_or_none search channel candidate
    Hlookup Hin).
Qed.

(** Recursive reference search, without a candidate product ------------- *)

Definition SelectedOrdinal : Type := (Path * OrdinalCandidate)%type.
Definition OrdinalSelection : Type := list SelectedOrdinal.

Definition selected_ordinal_channels (selected : OrdinalSelection) : list Path :=
  map fst selected.

Definition selected_ordinal_values (selected : OrdinalSelection)
    : list MatchCandidate :=
  map (fun item => ordinal_candidate_value (snd item)) selected.

Inductive SearchFailure : Type :=
| NoSpatialMatch
| GuardRejected.

Inductive CandidateSearchResult : Type :=
| SearchAccepted (selected : OrdinalSelection) (bindings : free_map)
| SearchRefused (reason : SearchFailure).

Definition failure_reached_leaf (failure : SearchFailure) : bool :=
  match failure with GuardRejected => true | NoSpatialMatch => false end.

Fixpoint scan_reference
    (attempt : OrdinalCandidate -> CandidateSearchResult)
    (candidates : list OrdinalCandidate) (any_leaf : bool)
    : CandidateSearchResult :=
  match candidates with
  | [] => SearchRefused (if any_leaf then GuardRejected else NoSpatialMatch)
  | candidate :: tail =>
      match attempt candidate with
      | SearchAccepted selected bindings => SearchAccepted selected bindings
      | SearchRefused failure =>
          scan_reference attempt tail
            (any_leaf || failure_reached_leaf failure)
      end
  end.

Fixpoint recursive_candidate_search
    (search : CandidateSearch) (channels : list Path)
    (reservations : ReservationAuthority) (bindings : free_map)
    : CandidateSearchResult :=
  match channels with
  | [] =>
      if match_program_guard (search_program search) bindings
      then SearchAccepted [] bindings
      else SearchRefused GuardRejected
  | channel :: tail =>
      match lookup_catalog channel (search_catalogs search) with
      | None => SearchRefused NoSpatialMatch
      | Some catalog =>
          scan_reference
            (fun candidate =>
              if candidate_available search channel candidate reservations then
                match candidate_spatial_result
                    (search_program search) channel candidate bindings with
                | None => SearchRefused NoSpatialMatch
                | Some next =>
                    match reserve_candidate search channel candidate reservations with
                    | None => SearchRefused NoSpatialMatch
                    | Some (reserved, _) =>
                        match recursive_candidate_search
                            search tail reserved next with
                        | SearchAccepted selected final_bindings =>
                            SearchAccepted ((channel, candidate) :: selected)
                              final_bindings
                        | SearchRefused failure => SearchRefused failure
                        end
                    end
                end
              else SearchRefused NoSpatialMatch)
            (search_projection search channel catalog) false
      end
  end.

Fixpoint recursive_candidate_search_oracle
    (search : CandidateSearch) (channels : list Path)
    (reservations : ReservationAuthority) (bindings : free_map)
    : CandidateSearchResult :=
  match channels with
  | [] =>
      if match_program_guard (search_program search) bindings
      then SearchAccepted [] bindings
      else SearchRefused GuardRejected
  | channel :: tail =>
      match lookup_catalog channel (search_catalogs search) with
      | None => SearchRefused NoSpatialMatch
      | Some catalog =>
          scan_reference
            (fun candidate =>
              if candidate_available search channel candidate reservations then
                match candidate_recursive_spatial_result
                    (search_program search) channel candidate bindings with
                | None => SearchRefused NoSpatialMatch
                | Some next =>
                    match reserve_candidate search channel candidate reservations with
                    | None => SearchRefused NoSpatialMatch
                    | Some (reserved, _) =>
                        match recursive_candidate_search_oracle
                            search tail reserved next with
                        | SearchAccepted selected final_bindings =>
                            SearchAccepted ((channel, candidate) :: selected)
                              final_bindings
                        | SearchRefused failure => SearchRefused failure
                        end
                    end
                end
              else SearchRefused NoSpatialMatch)
            (search_projection search channel catalog) false
      end
  end.

Lemma candidate_spatial_pda_is_recursive : forall program channel candidate bindings,
  candidate_spatial_result program channel candidate bindings =
  candidate_recursive_spatial_result program channel candidate bindings.
Proof.
  intros. unfold candidate_spatial_result, candidate_recursive_spatial_result.
  apply spatial_match_pda_equivalent_to_recursive_match.
Qed.

Lemma scan_reference_is_pointwise_extensional : forall
    (left right : OrdinalCandidate -> CandidateSearchResult)
    candidates any_leaf,
  (forall candidate, In candidate candidates -> left candidate = right candidate) ->
  scan_reference left candidates any_leaf = scan_reference right candidates any_leaf.
Proof.
  intros left right candidates. induction candidates as [|head tail IH];
    intros any_leaf Hequal; simpl; [reflexivity|].
  rewrite (Hequal head (or_introl eq_refl)).
  destruct (right head) as [selected bindings|failure]; [reflexivity|].
  apply IH. intros. apply Hequal. now right.
Qed.

Theorem recursive_candidate_search_uses_exact_recursive_spatial_oracle : forall
    search channels reservations bindings,
  recursive_candidate_search search channels reservations bindings =
  recursive_candidate_search_oracle search channels reservations bindings.
Proof.
  intros search channels. induction channels as [|channel tail IH];
    intros reservations bindings; simpl; [reflexivity|].
  destruct (lookup_catalog channel (search_catalogs search))
    as [catalog|] eqn:Hcatalog; [|reflexivity].
  apply scan_reference_is_pointwise_extensional.
  intros candidate Hin.
  destruct (candidate_available search channel candidate reservations);
    [|reflexivity].
  rewrite candidate_spatial_pda_is_recursive.
  destruct (candidate_recursive_spatial_result
    (search_program search) channel candidate bindings) as [next|];
    [|reflexivity].
  destruct (reserve_candidate search channel candidate reservations)
    as [[reserved ticket]|]; [|reflexivity].
  rewrite IH. reflexivity.
Qed.

Lemma scan_reference_acceptance_is_first : forall attempt candidates any_leaf
    selected bindings,
  scan_reference attempt candidates any_leaf = SearchAccepted selected bindings ->
  exists prefix candidate suffix,
    candidates = prefix ++ candidate :: suffix /\
    (forall earlier, In earlier prefix ->
      exists failure, attempt earlier = SearchRefused failure) /\
    attempt candidate = SearchAccepted selected bindings.
Proof.
  intros attempt candidates. induction candidates as [|head tail IH];
    intros any_leaf selected bindings Haccepted; simpl in Haccepted;
    [discriminate|].
  destruct (attempt head) as [head_selected head_bindings|failure] eqn:Hhead.
  - inversion Haccepted; subst. exists [], head, tail. simpl.
    repeat split; [intros; contradiction|assumption].
  - destruct (IH _ _ _ Haccepted) as
      [prefix [candidate [suffix [Heq [Hprior Hhit]]]]].
    exists (head :: prefix), candidate, suffix. subst tail. simpl.
    split; [reflexivity|]. split.
    + intros earlier [Heq|Hin].
      * subst. now exists failure.
      * now apply Hprior.
    + exact Hhit.
Qed.

(** Production-shaped explicit-frame PDA ------------------------------- *)

Record CandidateWalkFrame : Type := CandidateWalkFrameState {
  frame_channel : Path;
  frame_tail_channels : list Path;
  frame_remaining_scan : list OrdinalCandidate;
  frame_bindings_before : free_map;
  frame_reserved_ticket : option ReservationTicket;
  frame_any_leaf_reached : bool
}.

Inductive CandidateWalkControl : Type :=
| WalkDescend
    (remaining_channels : list Path)
    (reservations : ReservationAuthority)
    (chosen_reversed : OrdinalSelection)
    (bindings : free_map)
    (frames : list CandidateWalkFrame)
| WalkScan
    (channel : Path)
    (tail_channels : list Path)
    (remaining_scan : list OrdinalCandidate)
    (reservations : ReservationAuthority)
    (chosen_reversed : OrdinalSelection)
    (bindings_before : free_map)
    (frames : list CandidateWalkFrame)
    (any_leaf_reached : bool)
| WalkReturn
    (result : CandidateSearchResult)
    (reservations : ReservationAuthority)
    (chosen_reversed : OrdinalSelection)
    (frames : list CandidateWalkFrame)
| WalkDone (result : CandidateSearchResult).

Definition initial_walk_control (search : CandidateSearch) : CandidateWalkControl :=
  WalkDescend (search_requested_channels search)
    (initial_reservations search) [] empty_free_map [].

Definition propagate_failure
    (prior_leaf : bool) (failure : SearchFailure) : bool :=
  prior_leaf || failure_reached_leaf failure.

Definition candidate_walk_step
    (search : CandidateSearch) (control : CandidateWalkControl)
    : CandidateWalkControl :=
  match control with
  | WalkDescend [] reservations chosen bindings frames =>
      if match_program_guard (search_program search) bindings
      then WalkReturn (SearchAccepted (rev chosen) bindings)
        reservations chosen frames
      else WalkReturn (SearchRefused GuardRejected)
        reservations chosen frames
  | WalkDescend (channel :: tail) reservations chosen bindings frames =>
      match lookup_catalog channel (search_catalogs search) with
      | None => WalkReturn (SearchRefused NoSpatialMatch)
          reservations chosen frames
      | Some catalog => WalkScan channel tail
          (search_projection search channel catalog)
          reservations chosen bindings frames false
      end
  | WalkScan channel tail [] reservations chosen bindings frames any_leaf =>
      WalkReturn
        (SearchRefused (if any_leaf then GuardRejected else NoSpatialMatch))
        reservations chosen frames
  | WalkScan channel tail (candidate :: remaining)
      reservations chosen bindings frames any_leaf =>
      if candidate_available search channel candidate reservations then
        match candidate_spatial_result
            (search_program search) channel candidate bindings with
        | None => WalkScan channel tail remaining reservations chosen
            bindings frames any_leaf
        | Some next =>
            match reserve_candidate search channel candidate reservations with
            | None => WalkScan channel tail remaining reservations chosen
                bindings frames any_leaf
            | Some (reserved, ticket) =>
                WalkDescend tail reserved ((channel, candidate) :: chosen) next
                  (CandidateWalkFrameState channel tail remaining bindings ticket
                    any_leaf :: frames)
            end
        end
      else WalkScan channel tail remaining reservations chosen
        bindings frames any_leaf
  | WalkReturn (SearchAccepted selected bindings) reservations chosen frames =>
      WalkDone (SearchAccepted selected bindings)
  | WalkReturn (SearchRefused failure) reservations chosen [] =>
      WalkDone (SearchRefused failure)
  | WalkReturn (SearchRefused failure) reservations chosen (frame :: frames) =>
      WalkScan
        (frame_channel frame) (frame_tail_channels frame)
        (frame_remaining_scan frame)
        (rollback_reservation (frame_reserved_ticket frame) reservations)
        (tl chosen) (frame_bindings_before frame) frames
        (propagate_failure (frame_any_leaf_reached frame) failure)
  | WalkDone result => WalkDone result
  end.

(** The mixed-radix measure counts unexplored cursor work. A frame stores
    exactly its residual scan budget; this is not an execution fuel. *)

Definition catalog_scan_width (search : CandidateSearch)
    (catalog : CandidateCatalog) : nat :=
  length (search_projection search (candidate_catalog_channel catalog) catalog).

Definition search_max_width (search : CandidateSearch) : nat :=
  fold_right Nat.max 0
    (map (catalog_scan_width search) (candidate_catalogs (search_catalogs search))).

Fixpoint level_capacity (width depth : nat) : nat :=
  match depth with
  | 0 => 2
  | S tail_depth =>
      S (2 + width * (S (level_capacity width tail_depth)))
  end.

Definition scan_capacity (width candidate_count tail_depth : nat) : nat :=
  2 + candidate_count * (S (level_capacity width tail_depth)).

Definition frame_capacity (search : CandidateSearch)
    (frame : CandidateWalkFrame) : nat :=
  scan_capacity (search_max_width search)
    (length (frame_remaining_scan frame))
    (length (frame_tail_channels frame)).

Definition frames_capacity (search : CandidateSearch)
    (frames : list CandidateWalkFrame) : nat :=
  fold_right (fun frame rest => frame_capacity search frame + rest) 0 frames.

Definition descend_capacity (search : CandidateSearch) (channels : list Path) : nat :=
  match channels with
  | [] => 2
  | channel :: tail =>
      match lookup_catalog channel (search_catalogs search) with
      | None => 2
      | Some catalog =>
          S (scan_capacity (search_max_width search)
            (length (search_projection search channel catalog)) (length tail))
      end
  end.

Definition candidate_walk_work
    (search : CandidateSearch) (control : CandidateWalkControl) : nat :=
  match control with
  | WalkDescend channels _ _ _ frames =>
      descend_capacity search channels + frames_capacity search frames
  | WalkScan _ tail candidates _ _ _ frames _ =>
      scan_capacity (search_max_width search) (length candidates) (length tail) +
      frames_capacity search frames
  | WalkReturn _ _ _ frames => S (frames_capacity search frames)
  | WalkDone _ => 0
  end.

Lemma fold_max_member_bound : forall values value,
  In value values -> (value <= fold_right Nat.max 0 values)%nat.
Proof.
  induction values as [|head tail IH]; intros value Hin; [contradiction|].
  simpl. destruct Hin as [Heq|Hin].
  - subst. apply Nat.le_max_l.
  - eapply Nat.le_trans; [now apply IH|apply Nat.le_max_r].
Qed.

Lemma lookup_catalog_is_catalog_member : forall catalogs channel catalog,
  lookup_catalog channel catalogs = Some catalog ->
  In catalog (candidate_catalogs catalogs).
Proof.
  intros catalogs channel catalog Hlookup.
  cbv [lookup_catalog lookup_candidate_catalog
    lookup_candidate_catalog_list] in Hlookup.
  now apply lookup_candidate_catalog_some_is_member with channel.
Qed.

Lemma lookup_catalog_channel_is_exact : forall catalogs channel catalog,
  lookup_catalog channel catalogs = Some catalog ->
  candidate_catalog_channel catalog = channel.
Proof.
  intros catalogs channel catalog Hlookup.
  cbv [lookup_catalog lookup_candidate_catalog
    lookup_candidate_catalog_list] in Hlookup.
  destruct catalogs as [requested unique catalogs].
  simpl in Hlookup.
  revert Hlookup.
  induction catalogs as [|head tail IH]; simpl; [intros H; discriminate|].
  destruct (path_eq_dec channel (candidate_catalog_channel head)) as [Heq|Hneq].
  - intros H. simpl in H. destruct (path_eq_dec channel
      (candidate_catalog_channel head)); [|contradiction].
    inversion H; subst catalog; exact (eq_sym Heq).
  - intros H. simpl in H. destruct (path_eq_dec channel
      (candidate_catalog_channel head)); [contradiction|].
    apply IH. exact H.
Qed.

Lemma selected_scan_width_is_bounded : forall search channel catalog,
  lookup_catalog channel (search_catalogs search) = Some catalog ->
  (length (search_projection search channel catalog) <= search_max_width search)%nat.
Proof.
  intros search channel catalog Hlookup.
  unfold search_max_width.
  apply fold_max_member_bound.
  apply in_map_iff. exists catalog. split.
  - unfold catalog_scan_width.
    rewrite (@lookup_catalog_channel_is_exact
      (search_catalogs search) channel catalog Hlookup). reflexivity.
  - now apply (@lookup_catalog_is_catalog_member
      (search_catalogs search) channel catalog Hlookup).
Qed.

Lemma descend_capacity_is_level_bounded : forall search channels,
  (descend_capacity search channels <=
   level_capacity (search_max_width search) (length channels))%nat.
Proof.
  intros search channels. destruct channels as [|channel tail]; simpl;
    [lia|].
  unfold descend_capacity. simpl.
  destruct (lookup_catalog channel (search_catalogs search)) as [catalog|]
    eqn:Hlookup; simpl; [|lia].
  pose proof (@selected_scan_width_is_bounded search channel catalog Hlookup)
    as Hwidth.
  unfold scan_capacity. nia.
Qed.

Theorem every_nonterminal_candidate_walk_step_strictly_decreases_work : forall
    search control,
  (match control with WalkDone _ => False | _ => True end) ->
  (candidate_walk_work search (candidate_walk_step search control) <
   candidate_walk_work search control)%nat.
Proof.
  intros search control Hlive.
  destruct control as
      [channels reservations chosen bindings frames
      |channel tail candidates reservations chosen bindings frames any_leaf
      |result reservations chosen frames
      |result]; try contradiction.
  - destruct channels as [|head rest].
    + simpl. destruct (match_program_guard (search_program search) bindings);
        simpl; lia.
    + simpl. destruct (lookup_catalog head (search_catalogs search)); simpl; lia.
  - destruct candidates as [|candidate remaining].
    + simpl. destruct any_leaf; simpl; lia.
    + simpl candidate_walk_step.
      destruct (candidate_available search channel candidate reservations) eqn:Havailable.
      * destruct (candidate_spatial_result
          (search_program search) channel candidate bindings) as [next|]
          eqn:Hmatch.
        -- destruct (reserve_candidate search channel candidate reservations)
             as [[reserved ticket]|] eqn:Hreserve.
           ++ simpl candidate_walk_work.
              pose proof (@descend_capacity_is_level_bounded search tail) as Hbound.
              unfold frame_capacity, scan_capacity. simpl. nia.
           ++ simpl. unfold scan_capacity. nia.
        -- simpl. unfold scan_capacity. nia.
      * simpl. unfold scan_capacity. nia.
  - destruct result as [selected final_bindings|failure].
    + simpl. lia.
    + destruct frames as [|frame tail_frames].
      * simpl. lia.
      * simpl. unfold frame_capacity. lia.
Qed.

Fixpoint run_candidate_walk_rank
    (search : CandidateSearch) (unexplored_rank : nat)
    (control : CandidateWalkControl) : option CandidateSearchResult :=
  match control with
  | WalkDone result => Some result
  | _ =>
      match unexplored_rank with
      | 0 => None
      | S remaining_rank =>
          run_candidate_walk_rank search remaining_rank
            (candidate_walk_step search control)
      end
  end.

(** The rank is internal and is computed from live cursors.  It is not an
    input, traversal limit, or semantic fuel. *)
Definition pda_candidate_search (search : CandidateSearch)
    : option CandidateSearchResult :=
  let initial := initial_walk_control search in
  run_candidate_walk_rank search (candidate_walk_work search initial) initial.

Definition prepend_selection
    (prefix : OrdinalSelection) (result : CandidateSearchResult)
    : CandidateSearchResult :=
  match result with
  | SearchAccepted selected bindings =>
      SearchAccepted (prefix ++ selected) bindings
  | SearchRefused failure => SearchRefused failure
  end.

Fixpoint recursive_scan_state
    (search : CandidateSearch) (channel : Path) (tail : list Path)
    (candidates : list OrdinalCandidate)
    (reservations : ReservationAuthority) (chosen_reversed : OrdinalSelection)
    (bindings : free_map) (any_leaf : bool) : CandidateSearchResult :=
  match candidates with
  | [] => SearchRefused (if any_leaf then GuardRejected else NoSpatialMatch)
  | candidate :: remaining =>
      if candidate_available search channel candidate reservations then
        match candidate_spatial_result
            (search_program search) channel candidate bindings with
        | None => recursive_scan_state search channel tail remaining
            reservations chosen_reversed bindings any_leaf
        | Some next =>
            match reserve_candidate search channel candidate reservations with
            | None => recursive_scan_state search channel tail remaining
                reservations chosen_reversed bindings any_leaf
            | Some (reserved, _) =>
                match recursive_candidate_search search tail reserved next with
                | SearchAccepted suffix final_bindings =>
                    SearchAccepted
                      (rev chosen_reversed ++ (channel, candidate) :: suffix)
                      final_bindings
                | SearchRefused failure =>
                    recursive_scan_state search channel tail remaining
                      reservations chosen_reversed bindings
                      (propagate_failure any_leaf failure)
                end
            end
        end
      else recursive_scan_state search channel tail remaining
        reservations chosen_reversed bindings any_leaf
  end.

Fixpoint resume_frames
    (search : CandidateSearch)
    (reservations : ReservationAuthority) (chosen_reversed : OrdinalSelection)
    (frames : list CandidateWalkFrame) (result : CandidateSearchResult)
    : CandidateSearchResult :=
  match result with
  | SearchAccepted selected bindings => SearchAccepted selected bindings
  | SearchRefused failure =>
      match frames with
      | [] => SearchRefused failure
      | frame :: outer =>
          let rolled_back := rollback_reservation
            (frame_reserved_ticket frame) reservations in
          let resumed := recursive_scan_state search
            (frame_channel frame) (frame_tail_channels frame)
            (frame_remaining_scan frame) rolled_back (tl chosen_reversed)
            (frame_bindings_before frame)
            (propagate_failure (frame_any_leaf_reached frame) failure) in
          resume_frames search rolled_back (tl chosen_reversed) outer resumed
      end
  end.

Lemma resume_frames_accepts_immediately : forall search reservations chosen frames
    selected bindings,
  resume_frames search reservations chosen frames
    (SearchAccepted selected bindings) = SearchAccepted selected bindings.
Proof.
  intros search reservations chosen frames. induction frames;
    intros selected bindings; reflexivity.
Qed.

Definition recursive_control_observation
    (search : CandidateSearch) (control : CandidateWalkControl)
    : CandidateSearchResult :=
  match control with
  | WalkDescend channels reservations chosen bindings frames =>
      resume_frames search reservations chosen frames
        (prepend_selection (rev chosen)
          (recursive_candidate_search search channels reservations bindings))
  | WalkScan channel tail candidates reservations chosen bindings frames leaf =>
      resume_frames search reservations chosen frames
        (recursive_scan_state search channel tail candidates reservations
          chosen bindings leaf)
  | WalkReturn result reservations chosen frames =>
      resume_frames search reservations chosen frames result
  | WalkDone result => result
  end.

Lemma recursive_scan_state_is_reference_scan : forall
    search channel tail candidates reservations chosen bindings any_leaf,
  recursive_scan_state search channel tail candidates reservations chosen
      bindings any_leaf =
  prepend_selection (rev chosen)
    (scan_reference
      (fun candidate =>
        if candidate_available search channel candidate reservations then
          match candidate_spatial_result
              (search_program search) channel candidate bindings with
          | None => SearchRefused NoSpatialMatch
          | Some next =>
              match reserve_candidate search channel candidate reservations with
              | None => SearchRefused NoSpatialMatch
              | Some (reserved, _) =>
                  match recursive_candidate_search search tail reserved next with
                  | SearchAccepted suffix final_bindings =>
                      SearchAccepted ((channel, candidate) :: suffix)
                        final_bindings
                  | SearchRefused failure => SearchRefused failure
                  end
              end
          end
        else SearchRefused NoSpatialMatch)
      candidates any_leaf).
Proof.
  intros search channel tail candidates. induction candidates as [|candidate rest IH];
    intros reservations chosen bindings any_leaf; simpl.
  - destruct any_leaf; reflexivity.
  - destruct (candidate_available search channel candidate reservations);
      [|rewrite Bool.orb_false_r; apply IH].
    destruct (candidate_spatial_result
      (search_program search) channel candidate bindings) as [next|];
      [|rewrite Bool.orb_false_r; apply IH].
    destruct (reserve_candidate search channel candidate reservations)
      as [[reserved ticket]|]; [|rewrite Bool.orb_false_r; apply IH].
    destruct (recursive_candidate_search search tail reserved next)
      as [suffix final_bindings|failure].
    + reflexivity.
    + simpl. apply IH.
Qed.

Lemma recursive_control_observation_preserved_by_step : forall search control,
  recursive_control_observation search (candidate_walk_step search control) =
  recursive_control_observation search control.
Proof.
  intros search control.
  destruct control as
      [channels reservations chosen bindings frames
      |channel tail candidates reservations chosen bindings frames any_leaf
      |result reservations chosen frames
      |result].
  - destruct channels as [|head rest]; simpl.
    + destruct (match_program_guard (search_program search) bindings);
        simpl; rewrite ?app_nil_r; reflexivity.
    + destruct (lookup_catalog head (search_catalogs search)) as [catalog|]
        eqn:Hlookup; simpl; [|reflexivity].
      rewrite recursive_scan_state_is_reference_scan. reflexivity.
  - destruct candidates as [|candidate remaining]; simpl.
    + destruct any_leaf; reflexivity.
    + destruct (candidate_available search channel candidate reservations); [|reflexivity].
      destruct (candidate_spatial_result
        (search_program search) channel candidate bindings) as [next|];
        [|reflexivity].
      destruct (reserve_candidate search channel candidate reservations)
        as [[reserved ticket]|] eqn:Hreserve; [|reflexivity].
      destruct (recursive_candidate_search search tail reserved next)
        as [suffix final_bindings|failure] eqn:Hchild; simpl.
      * rewrite Hchild. cbn [prepend_selection].
        rewrite !resume_frames_accepts_immediately.
        f_equal. exact (eq_sym
          (app_assoc (rev chosen) [(channel, candidate)] suffix)).
      * rewrite Hchild. simpl.
        rewrite (@successful_reservation_rolls_back_exactly
          search channel candidate reservations reserved ticket Hreserve).
        reflexivity.
  - destruct result as [selected final_bindings|failure]; simpl;
      [rewrite resume_frames_accepts_immediately; reflexivity|].
    destruct frames as [|frame outer]; simpl; reflexivity.
  - reflexivity.
Qed.

Theorem rank_driven_candidate_walk_returns_exact_observation : forall
    search unexplored_rank control,
  (candidate_walk_work search control <= unexplored_rank)%nat ->
  run_candidate_walk_rank search unexplored_rank control =
    Some (recursive_control_observation search control).
Proof.
  intros search unexplored_rank. induction unexplored_rank as [|rank IH];
    intros control Hbound.
  - destruct control as
        [channels reservations chosen bindings frames
        |channel tail candidates reservations chosen bindings frames leaf
        |result reservations chosen frames
        |result].
    + exfalso. unfold candidate_walk_work, descend_capacity in Hbound.
      destruct channels as [|head rest]; [lia|].
      destruct (lookup_catalog head (search_catalogs search)); lia.
    + exfalso. unfold candidate_walk_work, scan_capacity in Hbound. lia.
    + exfalso. unfold candidate_walk_work in Hbound. lia.
    + reflexivity.
  - destruct control as
        [channels reservations chosen bindings frames
        |channel tail candidates reservations chosen bindings frames leaf
        |result reservations chosen frames
        |result]; [| | |reflexivity].
    + cbn [run_candidate_walk_rank].
      rewrite IH.
      * rewrite recursive_control_observation_preserved_by_step. reflexivity.
      * pose proof (@every_nonterminal_candidate_walk_step_strictly_decreases_work
          search (WalkDescend channels reservations chosen bindings frames) I).
        lia.
    + cbn [run_candidate_walk_rank].
      rewrite IH.
      * rewrite recursive_control_observation_preserved_by_step. reflexivity.
      * pose proof (@every_nonterminal_candidate_walk_step_strictly_decreases_work
          search (WalkScan channel tail candidates reservations chosen bindings
            frames leaf) I).
        lia.
    + cbn [run_candidate_walk_rank].
      rewrite IH.
      * rewrite recursive_control_observation_preserved_by_step. reflexivity.
      * pose proof (@every_nonterminal_candidate_walk_step_strictly_decreases_work
          search (WalkReturn result reservations chosen frames) I).
        lia.
Qed.

Theorem generated_candidate_pda_equals_pda_matcher_reference : forall search,
  pda_candidate_search search =
  Some (recursive_candidate_search search (search_requested_channels search)
    (initial_reservations search) empty_free_map).
Proof.
  intro search. unfold pda_candidate_search.
  rewrite rank_driven_candidate_walk_returns_exact_observation by lia.
  unfold initial_walk_control, recursive_control_observation.
  simpl.
  destruct (recursive_candidate_search search
    (search_requested_channels search) (initial_reservations search)
    empty_free_map); reflexivity.
Qed.

Theorem generated_candidate_pda_equals_recursive_oracle : forall search,
  pda_candidate_search search =
  Some (recursive_candidate_search_oracle search
    (search_requested_channels search) (initial_reservations search)
    empty_free_map).
Proof.
  intro search.
  rewrite generated_candidate_pda_equals_pda_matcher_reference.
  f_equal.
  apply recursive_candidate_search_uses_exact_recursive_spatial_oracle.
Qed.

(** Selector-produced proposal evidence -------------------------------- *)

Fixpoint replay_selection_reservations
    (search : CandidateSearch) (selected : OrdinalSelection)
    (reservations : ReservationAuthority) : option ReservationAuthority :=
  match selected with
  | [] => Some reservations
  | (channel, candidate) :: tail =>
      if candidate_available search channel candidate reservations then
        match reserve_candidate search channel candidate reservations with
        | None => None
        | Some (after, _) => replay_selection_reservations search tail after
        end
      else None
  end.

Record WellFormedCandidateSearch (search : CandidateSearch) : Type := {
  well_formed_search_observed_is_well_formed :
    authority_well_formedb (search_observed_authority search) = true;
  well_formed_search_codec : CanonicalDatumCodec;
  well_formed_search_catalogs_are_exact :
    build_candidate_catalog_set well_formed_search_codec
      (search_observed_authority search)
      (search_requested_channels search) =
    BuildOk (search_catalogs search);
  well_formed_search_has_every_requested_catalog :
    forall channel,
      In channel (search_requested_channels search) ->
      exists catalog,
        lookup_catalog channel (search_catalogs search) = Some catalog;
  well_formed_search_arrival_is_exact :
    match search_arrival search with
    | None => True
    | Some arrival =>
        candidate_address arrival = FreshAddress /\
        candidate_observed_multiplicity arrival = 1%N /\
        candidate_codec_exactb well_formed_search_codec arrival = true
    end
}.

Record CertifiedSearchAcceptance (search : CandidateSearch) : Type := {
  accepted_selected_ordinals : OrdinalSelection;
  accepted_bindings : free_map;
  accepted_search_result_is_selector_produced :
    pda_candidate_search search =
      Some (SearchAccepted accepted_selected_ordinals accepted_bindings);
  accepted_reservation_authority : ReservationAuthority;
  accepted_selection_replays_exactly :
    replay_selection_reservations search accepted_selected_ordinals
      (initial_reservations search) = Some accepted_reservation_authority
}.

Theorem certified_acceptance_is_recursive_first_admissible : forall
    search (accepted : CertifiedSearchAcceptance search),
  recursive_candidate_search search (search_requested_channels search)
      (initial_reservations search) empty_free_map =
    SearchAccepted (accepted_selected_ordinals accepted)
      (accepted_bindings accepted).
Proof.
  intros search accepted.
  pose proof (generated_candidate_pda_equals_pda_matcher_reference search)
    as Hequiv.
  rewrite (accepted_search_result_is_selector_produced accepted) in Hequiv.
  now injection Hequiv.
Qed.

Theorem certified_acceptance_carries_exact_reservation_feasibility : forall
    search (accepted : CertifiedSearchAcceptance search),
  exists final,
    replay_selection_reservations search
      (accepted_selected_ordinals accepted) (initial_reservations search) =
    Some final.
Proof. intros; eexists; apply accepted_selection_replays_exactly. Qed.

Theorem recursive_candidate_acceptance_has_exact_channel_vector : forall
    search channels reservations bindings selected final_bindings,
  recursive_candidate_search search channels reservations bindings =
    SearchAccepted selected final_bindings ->
  selected_ordinal_channels selected = channels.
Proof.
  intros search channels. induction channels as [|channel tail IH];
    intros reservations bindings selected final_bindings Haccepted.
  - simpl in Haccepted.
    destruct (match_program_guard (search_program search) bindings);
      inversion Haccepted. reflexivity.
  - simpl in Haccepted.
    destruct (lookup_catalog channel (search_catalogs search))
      as [catalog|] eqn:Hcatalog; [|discriminate].
    destruct (@scan_reference_acceptance_is_first
      (fun candidate =>
        if candidate_available search channel candidate reservations then
          match candidate_spatial_result
              (search_program search) channel candidate bindings with
          | Some next =>
              match reserve_candidate search channel candidate reservations with
              | Some (reserved, _) =>
                  match recursive_candidate_search search tail reserved next with
                  | SearchAccepted suffix final =>
                      SearchAccepted ((channel, candidate) :: suffix) final
                  | SearchRefused failure => SearchRefused failure
                  end
              | None => SearchRefused NoSpatialMatch
              end
          | None => SearchRefused NoSpatialMatch
          end
        else SearchRefused NoSpatialMatch)
      (search_projection search channel catalog) false selected final_bindings
      Haccepted) as [prefix [candidate [suffix [Hscan [Hprior Hhit]]]]].
    destruct (candidate_available search channel candidate reservations);
      [|discriminate].
    destruct (candidate_spatial_result
      (search_program search) channel candidate bindings) as [next|];
      [|discriminate].
    destruct (reserve_candidate search channel candidate reservations)
      as [[reserved ticket]|]; [|discriminate].
    destruct (recursive_candidate_search search tail reserved next)
      as [selected_tail tail_bindings|failure] eqn:Htail; [|discriminate].
    inversion Hhit; subst selected final_bindings.
    simpl. f_equal. now apply IH with reserved next tail_bindings.
Qed.

Corollary certified_acceptance_has_exact_channel_vector : forall
    search (accepted : CertifiedSearchAcceptance search),
  selected_ordinal_channels (accepted_selected_ordinals accepted) =
  search_requested_channels search.
Proof.
  intros. eapply recursive_candidate_acceptance_has_exact_channel_vector.
  apply certified_acceptance_is_recursive_first_admissible.
Qed.

Definition LexicographicallyFirstAdmissible
    (search : CandidateSearch) (selected : OrdinalSelection)
    (bindings : free_map) : Prop :=
  recursive_candidate_search_oracle search
    (search_requested_channels search) (initial_reservations search)
    empty_free_map = SearchAccepted selected bindings.

Theorem certified_acceptance_is_first_lexicographically_admissible : forall
    search (accepted : CertifiedSearchAcceptance search),
  LexicographicallyFirstAdmissible search
    (accepted_selected_ordinals accepted) (accepted_bindings accepted).
Proof.
  intros search accepted.
  unfold LexicographicallyFirstAdmissible.
  rewrite <- recursive_candidate_search_uses_exact_recursive_spatial_oracle.
  apply certified_acceptance_is_recursive_first_admissible.
Qed.

Theorem leaf_guard_rejection_is_distinct : forall search reservations bindings,
  match_program_guard (search_program search) bindings = false ->
  recursive_candidate_search_oracle search [] reservations bindings =
    SearchRefused GuardRejected.
Proof. intros; simpl; now rewrite H. Qed.

Theorem missing_channel_is_no_spatial_match : forall
    search channel tail reservations bindings,
  lookup_catalog channel (search_catalogs search) = None ->
  recursive_candidate_search_oracle search (channel :: tail)
    reservations bindings = SearchRefused NoSpatialMatch.
Proof. intros; simpl; now rewrite H. Qed.

(*
(** Exact continuation transaction ------------------------------------ *)

Inductive ContinuationUse : Type :=
| ImmediateContinuation (record : ContinuationRecord)
| StoredContinuation
    (capability : ContinuationCapability)
    (expected : ContinuationExpectation).

Definition commit_match_continuation
    (observed commit : ContinuationAuthority)
    (group join_channels : list Path) (use : ContinuationUse)
    : option (ContinuationAuthority * ContinuationRecord) :=
  match use with
  | ImmediateContinuation record => Some (commit, record)
  | StoredContinuation capability expected =>
      match resolve_exact_continuation observed commit group capability expected with
      | None => None
      | Some target =>
          match commit_exact_continuation observed commit group capability expected
              join_channels with
          | None => None
          | Some after => Some (after, target_record target)
          end
      end
  end.

Theorem immediate_continuation_preserves_commit_authority : forall
    observed commit group joins record,
  commit_match_continuation observed commit group joins
      (ImmediateContinuation record) = Some (commit, record).
Proof. reflexivity. Qed.

Theorem stored_continuation_result_matches_exact_expectation : forall
    observed commit group joins capability expected after record,
  commit_match_continuation observed commit group joins
      (StoredContinuation capability expected) = Some (after, record) ->
  continuation_source record = expected_source expected /\
  continuation_persistent record = expected_persistent expected /\
  continuation_peeks record = expected_peeks expected.
Proof.
  intros observed commit group joins capability expected after record Hcommit.
  unfold commit_match_continuation in Hcommit.
  destruct (resolve_exact_continuation observed commit group capability expected)
    as [target|] eqn:Hresolve; [|discriminate].
  destruct (commit_exact_continuation observed commit group capability expected joins)
    as [committed|] eqn:Hauthority; [|discriminate].
  inversion Hcommit; subst.
  now apply exact_resolution_preserves_expected_source_persistence_and_peeks
    in Hresolve.
Qed.

(** Phase-typed state publication -------------------------------------- *)

Definition CatalogCache : Type := Path -> option CandidateCatalog.

Record MatchStoreState : Type := MatchStoreStateData {
  match_store_data : TwoLevelAuthority;
  match_store_continuations : ContinuationAuthority;
  match_store_catalog_cache : CatalogCache;
  match_store_logical_generation : nat;
  match_store_datum_root_generation : nat;
  match_store_durable_semantic_generation : nat
}.

Record MatchProposal
    (observed_data : TwoLevelAuthority)
    (observed_continuations : ContinuationAuthority) : Type := MatchProposalState {
  proposal_search : CandidateSearch;
  proposal_search_observed_authority_is_exact :
    search_observed_authority proposal_search = observed_data;
  proposal_search_is_well_formed : WellFormedCandidateSearch proposal_search;
  proposal_acceptance : CertifiedSearchAcceptance proposal_search;
  proposal_observed_selection : ObservedExactSelection observed_data;
  proposal_selected_values_are_exact :
    selected_ordinal_values
      (accepted_selected_ordinals proposal_acceptance) =
    exact_selection_candidates
      (observed_exact_selection proposal_observed_selection);
  proposal_catalogs_are_exact :
    search_catalogs proposal_search =
    exact_selection_catalogs
      (observed_exact_selection proposal_observed_selection);
  proposal_arrival_is_exact :
    search_arrival proposal_search =
    exact_selection_arrival
      (observed_exact_selection proposal_observed_selection);
  proposal_continuation_use : ContinuationUse
}.

Definition proposal_selected_channels {observed_data observed_continuations}
    (proposal : MatchProposal observed_data observed_continuations) : list Path :=
  selected_ordinal_channels
    (accepted_selected_ordinals (proposal_acceptance proposal)).

Definition proposal_local_pool_observation {observed_data observed_continuations}
    (proposal : MatchProposal observed_data observed_continuations)
    : list (Path * list OrdinalCandidate) :=
  map
    (fun channel =>
      (channel,
        match lookup_catalog channel
          (search_catalogs (proposal_search proposal)) with
        | None => []
        | Some catalog =>
            search_projection (proposal_search proposal) channel catalog
        end))
    (search_requested_channels (proposal_search proposal)).

Lemma proposal_preserves_duplicate_channel_vector : forall
    observed_data observed_continuations
    (proposal : MatchProposal observed_data observed_continuations),
  proposal_selected_channels proposal =
  search_requested_channels (proposal_search proposal).
Proof.
  intros. unfold proposal_selected_channels.
  apply certified_acceptance_has_exact_channel_vector.
Qed.

Lemma proposal_every_requested_channel_was_cataloged : forall
    observed_data observed_continuations
    (proposal : MatchProposal observed_data observed_continuations) channel,
  In channel (search_requested_channels (proposal_search proposal)) ->
  exists catalog,
    lookup_catalog channel (search_catalogs (proposal_search proposal)) =
      Some catalog.
Proof.
  intros observed_data observed_continuations proposal channel Hin.
  now apply (well_formed_search_has_every_requested_catalog
    (proposal_search_is_well_formed proposal)).
Qed.

Definition bridge_touched_channels {before changes}
    (bridge : ExactPreparedRSpaceBridge before changes) : list Path :=
  prepared_delta_channels
    (prepared_channel_deltas (exact_bridge_delta bridge)).

Definition invalidate_touched_catalogs
    (before : CatalogCache) (touched : list Path) : CatalogCache :=
  fun channel =>
    if in_dec path_eq_dec channel touched then None else before channel.

Record PreparedPublication
    (observed_data : TwoLevelAuthority)
    (observed_continuations : ContinuationAuthority)
    (before : MatchStoreState)
    (proposal : MatchProposal observed_data observed_continuations) : Type :=
  PreparedPublicationState {
  publication_live_selection :
    LiveExactSelection observed_data (match_store_data before);
  publication_live_selection_is_proposal :
    live_observed_exact_selection publication_live_selection =
      proposal_observed_selection proposal;
  publication_package :
    { changes : PreparedMatchChanges &
      ExactPreparedRSpaceBridge (match_store_data before) changes };
  publication_prepare_and_check_is_exact :
    prepare_and_check_exact_rspace_delta
      {| preparation_live_selection := publication_live_selection |} =
      Some publication_package;
  publication_continuation_after : ContinuationAuthority;
  publication_continuation_record : ContinuationRecord;
  publication_continuation_commit_is_exact :
    commit_match_continuation observed_continuations
      (match_store_continuations before)
      (proposal_selected_channels proposal)
      (proposal_selected_channels proposal)
      (proposal_continuation_use proposal) =
    Some (publication_continuation_after, publication_continuation_record);
  publication_continuation_changed : bool;
  publication_continuation_changed_is_exact :
    publication_continuation_changed = true ->
      publication_continuation_after <> match_store_continuations before;
  publication_continuation_unchanged_is_exact :
    publication_continuation_changed = false ->
      publication_continuation_after = match_store_continuations before
}.

Definition publication_changes
    {observed_data observed_continuations before proposal}
    (prepared : PreparedPublication observed_data observed_continuations
      before proposal) : PreparedMatchChanges :=
  projT1 (publication_package prepared).

Definition publication_bridge
    {observed_data observed_continuations before proposal}
    (prepared : PreparedPublication observed_data observed_continuations
      before proposal)
    : ExactPreparedRSpaceBridge (match_store_data before)
        (publication_changes prepared) :=
  projT2 (publication_package prepared).

Definition publication_datum_changed
    {observed_data observed_continuations before proposal}
    (prepared : PreparedPublication observed_data observed_continuations
      before proposal) : bool :=
  match prepared_channel_deltas
      (exact_bridge_delta (publication_bridge prepared)) with
  | [] => false
  | _ :: _ => true
  end.

Definition publication_datum_root_count
    {observed_data observed_continuations before proposal}
    (prepared : PreparedPublication observed_data observed_continuations
      before proposal) : nat :=
  if publication_datum_changed prepared then 1 else 0.

Definition publication_durable_semantic_changed
    {observed_data observed_continuations before proposal}
    (prepared : PreparedPublication observed_data observed_continuations
      before proposal) : bool :=
  publication_datum_changed prepared || publication_continuation_changed prepared.

Definition publication_durable_semantic_count
    {observed_data observed_continuations before proposal}
    (prepared : PreparedPublication observed_data observed_continuations
      before proposal) : nat :=
  if publication_durable_semantic_changed prepared then 1 else 0.

Definition publish_prepared_state
    {observed_data observed_continuations before proposal}
    (prepared : PreparedPublication observed_data observed_continuations
      before proposal) : MatchStoreState :=
  let bridge := publication_bridge prepared in
  let touched := bridge_touched_channels bridge in
  MatchStoreStateData
    (apply_prepared_rspace_delta (exact_bridge_delta bridge))
    (publication_continuation_after prepared)
    (invalidate_touched_catalogs (match_store_catalog_cache before) touched)
    (S (match_store_logical_generation before))
    (match_store_datum_root_generation before +
      publication_datum_root_count prepared)
    (match_store_durable_semantic_generation before +
      publication_durable_semantic_count prepared).

Record MatchComm : Type := MatchCommState {
  match_comm_selected : list MatchCandidate;
  match_comm_bindings : free_map;
  match_comm_continuation : ContinuationRecord;
  match_comm_peeks : list Z
}.

Definition publication_comm
    {observed_data observed_continuations before proposal}
    (prepared : PreparedPublication observed_data observed_continuations
      before proposal) : MatchComm :=
  MatchCommState
    (exact_selection_candidates
      (live_exact_selection (publication_live_selection prepared)))
    (accepted_bindings (proposal_acceptance proposal))
    (publication_continuation_record prepared)
    (continuation_peeks (publication_continuation_record prepared)).

Inductive MatchTransactionPhase
    (observed_data : TwoLevelAuthority)
    (observed_continuations : ContinuationAuthority)
    (before : MatchStoreState)
    (proposal : MatchProposal observed_data observed_continuations) : Type :=
| Proposed
| StatePublished
    (prepared : PreparedPublication observed_data observed_continuations
      before proposal)
| CommObserved
    (prepared : PreparedPublication observed_data observed_continuations
      before proposal)
| Refused.

Inductive match_transaction_transition
    {observed_data observed_continuations before proposal} :
    MatchTransactionPhase observed_data observed_continuations before proposal ->
    MatchTransactionPhase observed_data observed_continuations before proposal ->
    Prop :=
| PublishState : forall prepared,
    match_transaction_transition Proposed (StatePublished prepared)
| ObserveComm : forall prepared,
    match_transaction_transition (StatePublished prepared) (CommObserved prepared)
| RefuseExactly :
    (forall prepared : PreparedPublication observed_data observed_continuations
      before proposal, False) ->
    match_transaction_transition Proposed Refused.

Definition phase_state
    {observed_data observed_continuations before proposal}
    (phase : MatchTransactionPhase observed_data observed_continuations
      before proposal) : MatchStoreState :=
  match phase with
  | Proposed | Refused => before
  | StatePublished prepared | CommObserved prepared =>
      publish_prepared_state prepared
  end.

Definition phase_comm
    {observed_data observed_continuations before proposal}
    (phase : MatchTransactionPhase observed_data observed_continuations
      before proposal) : option MatchComm :=
  match phase with
  | CommObserved prepared => Some (publication_comm prepared)
  | _ => None
  end.

Definition phase_logical_commit_count
    {observed_data observed_continuations before proposal}
    (phase : MatchTransactionPhase observed_data observed_continuations
      before proposal) : nat :=
  match phase with StatePublished _ | CommObserved _ => 1 | _ => 0 end.

Definition phase_datum_root_count
    {observed_data observed_continuations before proposal}
    (phase : MatchTransactionPhase observed_data observed_continuations
      before proposal) : nat :=
  match phase with
  | StatePublished prepared | CommObserved prepared =>
      publication_datum_root_count prepared
  | _ => 0
  end.

Definition phase_durable_semantic_count
    {observed_data observed_continuations before proposal}
    (phase : MatchTransactionPhase observed_data observed_continuations
      before proposal) : nat :=
  match phase with
  | StatePublished prepared | CommObserved prepared =>
      publication_durable_semantic_count prepared
  | _ => 0
  end.

Definition phase_comm_count
    {observed_data observed_continuations before proposal}
    (phase : MatchTransactionPhase observed_data observed_continuations
      before proposal) : nat :=
  match phase with CommObserved _ => 1 | _ => 0 end.

Theorem state_is_published_before_comm : forall
    observed_data observed_continuations before proposal prepared,
  @match_transaction_transition observed_data observed_continuations before proposal
    (StatePublished prepared) (CommObserved prepared) /\
  phase_state (StatePublished prepared) = phase_state (CommObserved prepared) /\
  phase_comm (StatePublished prepared) = None /\
  phase_comm (CommObserved prepared) = Some (publication_comm prepared).
Proof. intros. repeat split; try constructor; reflexivity. Qed.

Theorem refusal_is_strongly_atomic : forall
    observed_data observed_continuations before proposal,
  phase_state (@Refused observed_data observed_continuations before proposal) =
      before /\
  phase_comm (@Refused observed_data observed_continuations before proposal) =
      None /\
  phase_logical_commit_count
      (@Refused observed_data observed_continuations before proposal) = 0 /\
  phase_datum_root_count
      (@Refused observed_data observed_continuations before proposal) = 0 /\
  phase_durable_semantic_count
      (@Refused observed_data observed_continuations before proposal) = 0 /\
  phase_comm_count
      (@Refused observed_data observed_continuations before proposal) = 0.
Proof. intros. repeat split; reflexivity. Qed.

Theorem state_publication_counts_are_exact : forall
    observed_data observed_continuations before proposal prepared,
  phase_logical_commit_count
      (@StatePublished observed_data observed_continuations before proposal prepared) = 1 /\
  phase_comm_count
      (@StatePublished observed_data observed_continuations before proposal prepared) = 0 /\
  phase_datum_root_count
      (@StatePublished observed_data observed_continuations before proposal prepared) =
      publication_datum_root_count prepared /\
  phase_durable_semantic_count
      (@StatePublished observed_data observed_continuations before proposal prepared) =
      publication_durable_semantic_count prepared /\
  (publication_datum_root_count prepared = 0 \/
   publication_datum_root_count prepared = 1) /\
  (publication_durable_semantic_count prepared = 0 \/
   publication_durable_semantic_count prepared = 1).
Proof.
  intros. repeat split; try reflexivity.
  - unfold publication_datum_root_count.
    destruct (publication_datum_changed prepared); auto.
  - unfold publication_durable_semantic_count.
    destruct (publication_durable_semantic_changed prepared); auto.
Qed.

Theorem accepted_match_uses_exact_duplicate_preserving_join_channels : forall
    observed_data observed_continuations before proposal prepared,
  commit_match_continuation observed_continuations
      (match_store_continuations before)
      (search_requested_channels (proposal_search proposal))
      (search_requested_channels (proposal_search proposal))
      (proposal_continuation_use proposal) =
    Some (publication_continuation_after prepared,
      publication_continuation_record prepared).
Proof.
  intros. rewrite <- proposal_preserves_duplicate_channel_vector.
  apply publication_continuation_commit_is_exact.
Qed.

Theorem accepted_match_revalidates_selected_endpoints_at_commit : forall
    observed_data observed_continuations before proposal prepared,
  selection_authority_gate (match_store_data before)
    (exact_selection_candidates
      (observed_exact_selection (proposal_observed_selection proposal))) = true.
Proof.
  intros observed_data observed_continuations before proposal prepared.
  pose proof (live_exact_selection_authority_gate
    (publication_live_selection prepared)) as Hlive.
  rewrite (publication_live_selection_is_proposal prepared) in Hlive.
  exact Hlive.
Qed.

Theorem accepted_match_uses_constructed_exact_delta : forall
    observed_data observed_continuations before proposal prepared,
  prepare_and_check_exact_rspace_delta
      {| preparation_live_selection := publication_live_selection prepared |} =
      Some (publication_package prepared) /\
  match_store_data (publish_prepared_state prepared) =
    apply_prepared_rspace_delta
      (exact_bridge_delta (publication_bridge prepared)).
Proof.
  intros. split.
  - apply publication_prepare_and_check_is_exact.
  - reflexivity.
Qed.

Theorem accepted_match_preserves_every_untouched_channel : forall
    observed_data observed_continuations before proposal prepared channel,
  ~ In channel (bridge_touched_channels (publication_bridge prepared)) ->
  lookup_channel channel (match_store_data (publish_prepared_state prepared)) =
  lookup_channel channel (match_store_data before).
Proof.
  intros. simpl. apply prepared_rspace_delta_preserves_every_untouched_channel.
  exact H.
Qed.

Theorem accepted_match_isolates_prefix_related_untouched_channels : forall
    observed_data observed_continuations before proposal prepared channel touched,
  In touched (bridge_touched_channels (publication_bridge prepared)) ->
  ~ In channel (bridge_touched_channels (publication_bridge prepared)) ->
  ((exists suffix, channel = touched ++ suffix) \/
   (exists suffix, touched = channel ++ suffix)) ->
  lookup_channel channel (match_store_data (publish_prepared_state prepared)) =
  lookup_channel channel (match_store_data before).
Proof. intros. now apply accepted_match_preserves_every_untouched_channel. Qed.

Theorem accepted_match_invalidates_every_touched_catalog : forall
    observed_data observed_continuations before proposal prepared channel,
  In channel (bridge_touched_channels (publication_bridge prepared)) ->
  match_store_catalog_cache (publish_prepared_state prepared) channel = None.
Proof.
  intros. simpl. unfold invalidate_touched_catalogs.
  destruct (in_dec path_eq_dec channel
    (bridge_touched_channels (publication_bridge prepared)));
    [reflexivity|contradiction].
Qed.

Theorem accepted_match_preserves_every_untouched_catalog : forall
    observed_data observed_continuations before proposal prepared channel,
  ~ In channel (bridge_touched_channels (publication_bridge prepared)) ->
  match_store_catalog_cache (publish_prepared_state prepared) channel =
  match_store_catalog_cache before channel.
Proof.
  intros. simpl. unfold invalidate_touched_catalogs.
  destruct (in_dec path_eq_dec channel
    (bridge_touched_channels (publication_bridge prepared)));
    [contradiction|reflexivity].
Qed.

Theorem accepted_match_preserves_loaded_empty_output : forall
    observed_data observed_continuations before proposal prepared delta,
  In delta (prepared_channel_deltas
    (exact_bridge_delta (publication_bridge prepared))) ->
  prepared_delta_output delta = [] ->
  loaded (match_store_data (publish_prepared_state prepared))
    (prepared_delta_channel delta).
Proof.
  intros observed_data observed_continuations before proposal prepared delta
    Hin Hempty.
  exists []. simpl.
  pose proof (@prepared_rspace_delta_member_output_is_exact
    (match_store_data before)
    (exact_bridge_delta (publication_bridge prepared)) delta Hin) as Hlookup.
  now rewrite Hempty in Hlookup.
Qed.

Theorem proposal_local_pools_do_not_enter_published_state : forall
    observed_data observed_continuations before proposal prepared,
  match_store_catalog_cache (publish_prepared_state prepared) =
  invalidate_touched_catalogs (match_store_catalog_cache before)
    (bridge_touched_channels (publication_bridge prepared)).
Proof. reflexivity. Qed.

Theorem accepted_match_transaction_capstone : forall
    observed_data observed_continuations before proposal prepared,
  proposal_selected_channels proposal =
      search_requested_channels (proposal_search proposal) /\
  selection_authority_gate (match_store_data before)
      (exact_selection_candidates
        (observed_exact_selection (proposal_observed_selection proposal))) = true /\
  prepare_and_check_exact_rspace_delta
      {| preparation_live_selection := publication_live_selection prepared |} =
      Some (publication_package prepared) /\
  phase_comm
      (@StatePublished observed_data observed_continuations before proposal prepared) =
      None /\
  phase_comm
      (@CommObserved observed_data observed_continuations before proposal prepared) =
      Some (publication_comm prepared) /\
  phase_logical_commit_count
      (@StatePublished observed_data observed_continuations before proposal prepared) = 1 /\
  phase_comm_count
      (@CommObserved observed_data observed_continuations before proposal prepared) = 1.
Proof.
  intros. repeat split.
  - apply proposal_preserves_duplicate_channel_vector.
  - apply accepted_match_revalidates_selected_endpoints_at_commit.
  - apply publication_prepare_and_check_is_exact.
  - reflexivity.
  - reflexivity.
  - reflexivity.
  - reflexivity.
Qed.

Inductive match_transaction_invariant_id : Type :=
| MatchInvPrunedOrder | MatchInvPrunedCompleteness
| MatchInvReplayAllOrNone | MatchInvPersistentEligibility
| MatchInvPersistentNoReservation | MatchInvMultiplicityReservation
| MatchInvNonpersistentExactRollback | MatchInvSharedSemanticReservations
| MatchInvLeafOnlyGuard | MatchInvNoSpatialVsGuardRejected
| MatchInvFirstLexicographicSelection | MatchInvStrictUnexploredWork
| MatchInvGeneratedPdaOracleEquivalence | MatchInvSelectorFeasibility
| MatchInvImmutableProposalPools | MatchInvEveryRequestedChannelCataloged
| MatchInvDuplicatePreservingChannelVector | MatchInvCommitEndpointLiveness
| MatchInvConstructedExactDelta | MatchInvImmediateVsStoredContinuation
| MatchInvStateBeforeComm | MatchInvConditionalDatumRoot
| MatchInvConditionalDurableSemantic | MatchInvCatalogIsolation
| MatchInvLoadedEmpty | MatchInvPrefixIsolation | MatchInvStrongRefusal.

Definition match_transaction_invariant_catalog :=
  [MatchInvPrunedOrder; MatchInvPrunedCompleteness; MatchInvReplayAllOrNone;
   MatchInvPersistentEligibility; MatchInvPersistentNoReservation;
   MatchInvMultiplicityReservation; MatchInvNonpersistentExactRollback;
   MatchInvSharedSemanticReservations; MatchInvLeafOnlyGuard;
   MatchInvNoSpatialVsGuardRejected; MatchInvFirstLexicographicSelection;
   MatchInvStrictUnexploredWork; MatchInvGeneratedPdaOracleEquivalence;
   MatchInvSelectorFeasibility; MatchInvImmutableProposalPools;
   MatchInvEveryRequestedChannelCataloged;
   MatchInvDuplicatePreservingChannelVector; MatchInvCommitEndpointLiveness;
   MatchInvConstructedExactDelta; MatchInvImmediateVsStoredContinuation;
   MatchInvStateBeforeComm; MatchInvConditionalDatumRoot;
   MatchInvConditionalDurableSemantic; MatchInvCatalogIsolation;
   MatchInvLoadedEmpty; MatchInvPrefixIsolation; MatchInvStrongRefusal].

Theorem match_transaction_invariant_catalog_is_duplicate_free_and_exact :
  NoDup match_transaction_invariant_catalog /\
  length match_transaction_invariant_catalog = 27%nat.
Proof. split; [repeat constructor; simpl; intuition discriminate | reflexivity]. Qed.

Print Assumptions query_pruning_cannot_omit_a_spatial_match.
Print Assumptions successful_nonpersistent_reservation_is_exact.
Print Assumptions every_nonterminal_candidate_walk_step_strictly_decreases_work.
Print Assumptions generated_candidate_pda_equals_recursive_oracle.
Print Assumptions certified_acceptance_is_recursive_first_admissible.
Print Assumptions stored_continuation_result_matches_exact_expectation.
Print Assumptions accepted_match_uses_constructed_exact_delta.
Print Assumptions accepted_match_uses_exact_duplicate_preserving_join_channels.
Print Assumptions state_is_published_before_comm.
Print Assumptions refusal_is_strongly_atomic.
Print Assumptions accepted_match_preserves_loaded_empty_output.
Print Assumptions accepted_match_transaction_capstone.
Print Assumptions match_transaction_invariant_catalog_is_duplicate_free_and_exact.

*)

(**
  The publication refinement intentionally has no declaration in this module
  yet.  Its authority must come from the executable checked-pool package that
  the repaired [TwoLevelRSpaceMatchPreparation] interface will expose.  In
  particular, an absolute stored-datum address must authorize multiplicity and
  reservation identity; decoded channel/datum projections and caller-supplied
  liveness proofs cannot authorize publication.  The final continuation phase
  must likewise consume the expectation-based capability API.  Consequently
  the independent selector results below make no commit/publication claim.
 *)

(* The publication block above is retired while its old bridge vocabulary is
   being replaced.  Keep the live phase boundary explicit and dependent only
   on the checked preparation package so recovery proofs have a stable type. *)
Inductive CurrentMatchPhase : Type :=
| CurrentProposed
| CurrentPrepared (changes : PreparedMatchChanges)
| CurrentRefused.

Definition current_phase_has_authority (phase : CurrentMatchPhase) : bool :=
  match phase with
  | CurrentProposed | CurrentRefused => false
  | CurrentPrepared _ => true
  end.

Theorem current_refusal_has_no_authority :
  current_phase_has_authority CurrentRefused = false.
Proof. reflexivity. Qed.

Theorem current_prepared_has_authority : forall changes,
  current_phase_has_authority (CurrentPrepared changes) = true.
Proof. reflexivity. Qed.

Inductive independent_match_invariant_id : Type :=
| IndependentInvPrunedOrder
| IndependentInvPrunedCompleteness
| IndependentInvReplayAllOrNone
| IndependentInvFreshEligibilityIndependentOfPruning
| IndependentInvPersistentEligibility
| IndependentInvPersistentNoReservation
| IndependentInvMultiplicityReservation
| IndependentInvExactRollback
| IndependentInvSharedSemanticReservation
| IndependentInvLeafOnlyGuard
| IndependentInvNoSpatialVsGuardRejected
| IndependentInvFirstLexicographicSelection
| IndependentInvStrictUnexploredWork
| IndependentInvSpatialMatcherEquivalence
| IndependentInvGeneratedPdaOracleEquivalence
| IndependentInvSelectorReservationFeasibility
| IndependentInvExactRequestedChannelVector
| IndependentInvEveryRequestedCatalogPresent.

Definition independent_match_invariant_catalog :=
  [IndependentInvPrunedOrder;
   IndependentInvPrunedCompleteness;
   IndependentInvReplayAllOrNone;
   IndependentInvFreshEligibilityIndependentOfPruning;
   IndependentInvPersistentEligibility;
   IndependentInvPersistentNoReservation;
   IndependentInvMultiplicityReservation;
   IndependentInvExactRollback;
   IndependentInvSharedSemanticReservation;
   IndependentInvLeafOnlyGuard;
   IndependentInvNoSpatialVsGuardRejected;
   IndependentInvFirstLexicographicSelection;
   IndependentInvStrictUnexploredWork;
   IndependentInvSpatialMatcherEquivalence;
   IndependentInvGeneratedPdaOracleEquivalence;
   IndependentInvSelectorReservationFeasibility;
   IndependentInvExactRequestedChannelVector;
   IndependentInvEveryRequestedCatalogPresent].

Theorem independent_match_invariant_catalog_is_duplicate_free_and_exact :
  NoDup independent_match_invariant_catalog /\
  length independent_match_invariant_catalog = 18%nat.
Proof.
  split; [repeat constructor; simpl; intuition discriminate | reflexivity].
Qed.

Print Assumptions query_pruning_cannot_omit_a_spatial_match.
Print Assumptions successful_nonpersistent_reservation_is_exact.
Print Assumptions nonpersistent_multiplicity_can_reserve_repeatedly_below_capacity.
Print Assumptions replay_exclusions_are_never_partial.
Print Assumptions recursive_candidate_search_uses_exact_recursive_spatial_oracle.
Print Assumptions scan_reference_acceptance_is_first.
Print Assumptions every_nonterminal_candidate_walk_step_strictly_decreases_work.
Print Assumptions generated_candidate_pda_equals_pda_matcher_reference.
Print Assumptions generated_candidate_pda_equals_recursive_oracle.
Print Assumptions certified_acceptance_is_first_lexicographically_admissible.
Print Assumptions certified_acceptance_carries_exact_reservation_feasibility.
Print Assumptions certified_acceptance_has_exact_channel_vector.
Print Assumptions leaf_guard_rejection_is_distinct.
Print Assumptions missing_channel_is_no_spatial_match.
Print Assumptions independent_match_invariant_catalog_is_duplicate_free_and_exact.

End TwoLevelRSpaceMatchTransaction.
