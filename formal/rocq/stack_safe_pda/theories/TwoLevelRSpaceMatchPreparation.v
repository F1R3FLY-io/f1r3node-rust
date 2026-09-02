From Stdlib Require Import
  Bool List Arith Lia NArith ZArith Ring Sorting.Sorted
  Program.Equality.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublicationBase
  PathMapCanonicalStreamPublication
  TwoLevelRSpaceAuthority
  TwoLevelRSpaceOperations
  TwoLevelRSpaceDelta.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceOperations.
Import TwoLevelRSpaceDelta.
Set Implicit Arguments.

(** Exact, constructive PREPARE authority for one accepted two-level RSpace
    match.  Lists in this module are proof observations of ordered PathMap
    cursors.  In particular, the candidate order is the production
    digest-plus-canonical-identity order and the delta order is inherited from
    a nested PathMap cursor; neither order is obtained by sorting a flattened
    [Vec]. *)
Module TwoLevelRSpaceMatchPreparation.

(** Candidate catalogs --------------------------------------------------- *)

Inductive CandidateOrigin : Type :=
| CandidateFresh
| CandidateStored (channel datum : Path).

Record MatchCandidate : Type := MatchCandidateState {
  candidate_channel : Path;
  candidate_datum : Path;
  candidate_digest : Path;
  candidate_canonical_identity : Path;
  candidate_persistent : bool;
  candidate_origin : CandidateOrigin
}.

Record StoredMetadata : Type := StoredMetadataState {
  stored_metadata_channel : Path;
  stored_metadata_datum : Path;
  stored_metadata_digest : Path;
  stored_metadata_canonical_identity : Path;
  stored_metadata_persistent : bool
}.

Definition stored_metadata_candidate (metadata : StoredMetadata)
    : MatchCandidate :=
  MatchCandidateState
    (stored_metadata_channel metadata)
    (stored_metadata_datum metadata)
    (stored_metadata_digest metadata)
    (stored_metadata_canonical_identity metadata)
    (stored_metadata_persistent metadata)
    (CandidateStored
      (stored_metadata_channel metadata) (stored_metadata_datum metadata)).

Definition StoredMetadataAuthority : Type := list StoredMetadata.

(** A concrete codec value supplies the production canonical parser and
    digest operation.  Exactness below binds metadata to those computations;
    there is no uninterpreted digest axiom. *)
Record CanonicalDatumCodec : Type := CanonicalDatumCodecState {
  parse_canonical_datum : Path -> option (Path * bool);
  digest_canonical_identity : Path -> Path
}.

Definition stored_metadata_codec_exact
    (codec : CanonicalDatumCodec) (metadata : StoredMetadata) : Prop :=
  parse_canonical_datum codec (stored_metadata_datum metadata) =
    Some (stored_metadata_canonical_identity metadata,
      stored_metadata_persistent metadata) /\
  digest_canonical_identity codec
    (stored_metadata_canonical_identity metadata) =
    stored_metadata_digest metadata.

Definition stored_metadata_address (metadata : StoredMetadata)
    : (Path * Path)%type :=
  (stored_metadata_channel metadata, stored_metadata_datum metadata).

(** The metadata table is exact, not merely a cache of candidates happened to
    be selected.  Every metadata row names a live endpoint, every live datum
    endpoint has exactly one metadata row, and address identity is unique. *)
Record StoredMetadataAuthorityExact
    (before : TwoLevelAuthority) (codec : CanonicalDatumCodec)
    (metadata : StoredMetadataAuthority) : Prop :=
  StoredMetadataAuthorityExactState {
    stored_metadata_codec_bindings_are_exact :
      Forall (stored_metadata_codec_exact codec) metadata;
    stored_metadata_paths_are_valid :
      Forall
        (fun row =>
          path_valid (stored_metadata_channel row) = true /\
          path_valid (stored_metadata_datum row) = true /\
          path_valid (stored_metadata_digest row) = true /\
          path_valid (stored_metadata_canonical_identity row) = true)
        metadata;
    stored_metadata_rows_are_live :
      Forall
        (fun row =>
          (0 < lookup_multiplicity before
            (stored_metadata_channel row) (stored_metadata_datum row))%N)
        metadata;
    stored_metadata_addresses_are_unique :
      NoDup (map stored_metadata_address metadata);
    stored_metadata_covers_every_live_endpoint :
      forall channel contents datum multiplicity,
        lookup_channel channel before = Some contents ->
        In (datum, multiplicity) contents ->
        exists row,
          In row metadata /\
          stored_metadata_channel row = channel /\
          stored_metadata_datum row = datum
  }.

Theorem exact_metadata_row_is_parser_and_digest_bound : forall
    before codec metadata row,
  StoredMetadataAuthorityExact before codec metadata ->
  In row metadata ->
  parse_canonical_datum codec (stored_metadata_datum row) =
      Some (stored_metadata_canonical_identity row,
        stored_metadata_persistent row) /\
  digest_canonical_identity codec
      (stored_metadata_canonical_identity row) = stored_metadata_digest row.
Proof.
  intros before codec metadata row Hexact Hin.
  pose proof (stored_metadata_codec_bindings_are_exact Hexact) as Hall.
  now apply Forall_forall with (x := row) in Hall.
Qed.

Theorem malformed_datum_cannot_inhabit_exact_metadata : forall
    before codec metadata row,
  StoredMetadataAuthorityExact before codec metadata ->
  In row metadata ->
  parse_canonical_datum codec (stored_metadata_datum row) = None -> False.
Proof.
  intros before codec metadata row Hexact Hin Hmalformed.
  destruct (@exact_metadata_row_is_parser_and_digest_bound
    before codec metadata row Hexact Hin)
    as [Hparsed _]. congruence.
Qed.

Definition candidate_origin_exact (candidate : MatchCandidate) : Prop :=
  match candidate_origin candidate with
  | CandidateFresh => True
  | CandidateStored channel datum =>
      channel = candidate_channel candidate /\
      datum = candidate_datum candidate
  end.

Definition candidate_paths_valid (candidate : MatchCandidate) : Prop :=
  path_valid (candidate_channel candidate) = true /\
  path_valid (candidate_datum candidate) = true /\
  path_valid (candidate_digest candidate) = true /\
  path_valid (candidate_canonical_identity candidate) = true.

Definition CandidateOrderKey : Type := (Path * Path)%type.

Definition candidate_order_key (candidate : MatchCandidate) : CandidateOrderKey :=
  (candidate_digest candidate, candidate_canonical_identity candidate).

Definition candidate_order_key_lt
    (left right : CandidateOrderKey) : Prop :=
  path_lex_lt (fst left) (fst right) \/
  (fst left = fst right /\ path_lex_lt (snd left) (snd right)).

Definition candidate_lt (left right : MatchCandidate) : Prop :=
  candidate_order_key_lt (candidate_order_key left) (candidate_order_key right).

Theorem forced_digest_collision_uses_canonical_identity_order : forall left right,
  candidate_digest left = candidate_digest right ->
  candidate_lt left right ->
  path_lex_lt
    (candidate_canonical_identity left)
    (candidate_canonical_identity right).
Proof.
  intros left right Hdigest Hlt.
  unfold candidate_lt, candidate_order_key_lt, candidate_order_key in Hlt.
  simpl in Hlt. destruct Hlt as [Hdigest_lt | [_ Hidentity]]; [|exact Hidentity].
  rewrite Hdigest in Hdigest_lt.
  exact (False_rect _ (path_lex_lt_irreflexive _ Hdigest_lt)).
Qed.

Record OrdinalCandidate : Type := OrdinalCandidateState {
  ordinal_candidate_index : nat;
  ordinal_candidate_value : MatchCandidate
}.

Fixpoint enumerate_candidates_from
    (ordinal : nat) (candidates : list MatchCandidate)
    : list OrdinalCandidate :=
  match candidates with
  | [] => []
  | candidate :: tail =>
      OrdinalCandidateState ordinal candidate ::
      enumerate_candidates_from (S ordinal) tail
  end.

Definition enumerate_candidates := enumerate_candidates_from 0.

Lemma enumerate_candidates_from_values : forall ordinal candidates,
  map ordinal_candidate_value (enumerate_candidates_from ordinal candidates) =
  candidates.
Proof.
  intros ordinal candidates. revert ordinal.
  induction candidates as [|candidate tail IH]; intro ordinal; simpl;
    [reflexivity | now rewrite IH].
Qed.

Lemma enumerate_candidates_from_ordinals : forall ordinal candidates,
  map ordinal_candidate_index (enumerate_candidates_from ordinal candidates) =
  seq ordinal (length candidates).
Proof.
  intros ordinal candidates. revert ordinal.
  induction candidates as [|candidate tail IH]; intro ordinal; simpl;
    [reflexivity | now rewrite IH].
Qed.

Record CandidateCatalog : Type := CandidateCatalogState {
  candidate_catalog_channel : Path;
  candidate_catalog_authority : list MatchCandidate;
  candidate_catalog_channel_valid :
    path_valid candidate_catalog_channel = true;
  candidate_catalog_members_are_exact_stored_metadata :
    Forall
      (fun candidate =>
        candidate_origin candidate =
          CandidateStored candidate_catalog_channel (candidate_datum candidate) /\
        candidate_channel candidate = candidate_catalog_channel /\
        candidate_origin_exact candidate)
      candidate_catalog_authority;
  candidate_catalog_keys_are_unique :
    NoDup (map candidate_order_key candidate_catalog_authority);
  candidate_catalog_order_is_digest_identity :
    StronglySorted candidate_lt candidate_catalog_authority
}.

Definition candidate_catalog_projection (catalog : CandidateCatalog)
    : list OrdinalCandidate :=
  enumerate_candidates (candidate_catalog_authority catalog).

(** Production prepends the optional fresh arrival at distinguished ordinal
    zero.  Only stored candidates participate in digest/identity order; their
    ordinals are shifted by one when Fresh is present. *)
Definition catalog_fresh_arrival
    (arrival : option MatchCandidate) (catalog : CandidateCatalog)
    : option MatchCandidate :=
  match arrival with
  | None => None
  | Some fresh =>
      if path_eq_dec (candidate_channel fresh) (candidate_catalog_channel catalog)
      then Some fresh else None
  end.

Definition combined_candidate_projection
    (arrival : option MatchCandidate) (catalog : CandidateCatalog)
    : list OrdinalCandidate :=
  match catalog_fresh_arrival arrival catalog with
  | None => enumerate_candidates_from 0 (candidate_catalog_authority catalog)
  | Some fresh =>
      OrdinalCandidateState 0 fresh ::
      enumerate_candidates_from 1 (candidate_catalog_authority catalog)
  end.

Theorem combined_projection_fresh_is_distinguished_zero : forall catalog fresh,
  candidate_channel fresh = candidate_catalog_channel catalog ->
  hd_error (combined_candidate_projection (Some fresh) catalog) =
  Some (OrdinalCandidateState 0 fresh).
Proof.
  intros catalog fresh Hchannel.
  unfold combined_candidate_projection, catalog_fresh_arrival.
  destruct (path_eq_dec (candidate_channel fresh)
    (candidate_catalog_channel catalog)); [reflexivity | contradiction].
Qed.

Theorem combined_projection_stored_ordinals_are_shifted : forall catalog fresh,
  candidate_channel fresh = candidate_catalog_channel catalog ->
  map ordinal_candidate_index
    (tl (combined_candidate_projection (Some fresh) catalog)) =
  seq 1 (length (candidate_catalog_authority catalog)).
Proof.
  intros catalog fresh Hchannel.
  unfold combined_candidate_projection, catalog_fresh_arrival.
  destruct (path_eq_dec (candidate_channel fresh)
    (candidate_catalog_channel catalog));
    [simpl; apply enumerate_candidates_from_ordinals | contradiction].
Qed.

Theorem combined_projection_stored_metadata_is_exact : forall catalog fresh,
  candidate_channel fresh = candidate_catalog_channel catalog ->
  map ordinal_candidate_value
    (tl (combined_candidate_projection (Some fresh) catalog)) =
  candidate_catalog_authority catalog.
Proof.
  intros catalog fresh Hchannel.
  unfold combined_candidate_projection, catalog_fresh_arrival.
  destruct (path_eq_dec (candidate_channel fresh)
    (candidate_catalog_channel catalog));
    [simpl; apply enumerate_candidates_from_values | contradiction].
Qed.

Theorem combined_projection_unrelated_channel_is_stored_only : forall
    catalog fresh,
  candidate_channel fresh <> candidate_catalog_channel catalog ->
  combined_candidate_projection (Some fresh) catalog =
  candidate_catalog_projection catalog.
Proof.
  intros catalog fresh Hchannel.
  unfold combined_candidate_projection, catalog_fresh_arrival,
    candidate_catalog_projection, enumerate_candidates.
  destruct (path_eq_dec (candidate_channel fresh)
    (candidate_catalog_channel catalog)); [contradiction | reflexivity].
Qed.

Theorem candidate_catalog_projection_preserves_exact_metadata : forall catalog,
  map ordinal_candidate_value (candidate_catalog_projection catalog) =
  candidate_catalog_authority catalog.
Proof.
  intro catalog. apply enumerate_candidates_from_values.
Qed.

Theorem candidate_catalog_projection_has_exact_total_ordinals : forall catalog,
  map ordinal_candidate_index (candidate_catalog_projection catalog) =
  seq 0 (length (candidate_catalog_authority catalog)).
Proof.
  intro catalog. apply enumerate_candidates_from_ordinals.
Qed.

Theorem candidate_catalog_projection_ordinals_are_unique : forall catalog,
  NoDup (map ordinal_candidate_index (candidate_catalog_projection catalog)).
Proof.
  intro catalog. rewrite candidate_catalog_projection_has_exact_total_ordinals.
  apply seq_NoDup.
Qed.

Theorem candidate_catalog_projection_keys_are_unique : forall catalog,
  NoDup
    (map (fun ordinal => candidate_order_key (ordinal_candidate_value ordinal))
      (candidate_catalog_projection catalog)).
Proof.
  intro catalog.
  rewrite <- map_map, candidate_catalog_projection_preserves_exact_metadata.
  exact (candidate_catalog_keys_are_unique catalog).
Qed.

Definition catalog_channel (catalog : CandidateCatalog) : Path :=
  candidate_catalog_channel catalog.

Record CandidateCatalogSet : Type := CandidateCatalogSetState {
  candidate_catalogs : list CandidateCatalog;
  candidate_catalog_channels_are_unique :
    NoDup (map catalog_channel candidate_catalogs)
}.

Fixpoint lookup_candidate_catalog
    (channel : Path) (catalogs : list CandidateCatalog)
    : option CandidateCatalog :=
  match catalogs with
  | [] => None
  | catalog :: tail =>
      if path_eq_dec channel (candidate_catalog_channel catalog)
      then Some catalog
      else lookup_candidate_catalog channel tail
  end.

Definition lookup_catalog
    (channel : Path) (catalogs : CandidateCatalogSet)
    : option CandidateCatalog :=
  lookup_candidate_catalog channel (candidate_catalogs catalogs).

Definition metadata_at_channel
    (channel : Path) (metadata : StoredMetadataAuthority)
    : list StoredMetadata :=
  filter
    (fun row =>
      if path_eq_dec channel (stored_metadata_channel row)
      then true else false)
    metadata.

Definition candidate_catalog_coherent
    (metadata : StoredMetadataAuthority) (catalog : CandidateCatalog) : Prop :=
  candidate_catalog_authority catalog =
    map stored_metadata_candidate
      (metadata_at_channel (candidate_catalog_channel catalog) metadata).

Definition candidate_catalog_set_coherent
    (metadata : StoredMetadataAuthority)
    (catalogs : CandidateCatalogSet) : Prop :=
  forall catalog,
    In catalog (candidate_catalogs catalogs) ->
    candidate_catalog_coherent metadata catalog.

Lemma lookup_candidate_catalog_some_is_member : forall catalogs channel catalog,
  lookup_candidate_catalog channel catalogs = Some catalog ->
  In catalog catalogs.
Proof.
  intros catalogs. induction catalogs as [|head tail IH];
    intros channel catalog Hlookup; simpl in Hlookup; [discriminate|].
  destruct (path_eq_dec channel (candidate_catalog_channel head)).
  - inversion Hlookup; subst. now left.
  - right. now apply IH with channel.
Qed.

Theorem coherent_catalog_is_exact_metadata_projection : forall
    metadata catalogs channel catalog,
  candidate_catalog_set_coherent metadata catalogs ->
  lookup_catalog channel catalogs = Some catalog ->
  map ordinal_candidate_value (candidate_catalog_projection catalog) =
    map stored_metadata_candidate
      (metadata_at_channel (candidate_catalog_channel catalog) metadata).
Proof.
  intros metadata catalogs channel catalog Hcoherent Hlookup.
  rewrite candidate_catalog_projection_preserves_exact_metadata.
  apply Hcoherent.
  unfold lookup_catalog in Hlookup.
  now apply lookup_candidate_catalog_some_is_member with channel.
Qed.

Theorem coherent_combined_pool_is_exact_fresh_then_stored_projection : forall
    metadata catalogs channel catalog fresh,
  candidate_catalog_set_coherent metadata catalogs ->
  lookup_catalog channel catalogs = Some catalog ->
  candidate_channel fresh = candidate_catalog_channel catalog ->
  map ordinal_candidate_value
    (combined_candidate_projection (Some fresh) catalog) =
  fresh :: map stored_metadata_candidate
    (metadata_at_channel (candidate_catalog_channel catalog) metadata) /\
  map ordinal_candidate_index
    (combined_candidate_projection (Some fresh) catalog) =
  seq 0 (S (length (candidate_catalog_authority catalog))).
Proof.
  intros metadata catalogs channel catalog fresh Hcoherent Hlookup Hchannel.
  unfold combined_candidate_projection, catalog_fresh_arrival.
  destruct (path_eq_dec (candidate_channel fresh)
    (candidate_catalog_channel catalog)); [|contradiction].
  simpl. split.
  - rewrite enumerate_candidates_from_values.
    f_equal.
    rewrite <- candidate_catalog_projection_preserves_exact_metadata.
    exact (@coherent_catalog_is_exact_metadata_projection
      metadata catalogs channel catalog Hcoherent Hlookup).
  - rewrite enumerate_candidates_from_ordinals. reflexivity.
Qed.

Definition candidate_origin_eq_dec : forall left right : CandidateOrigin,
    {left = right} + {left <> right}.
Proof. decide equality; apply path_eq_dec. Defined.

Definition match_candidate_eq_dec : forall left right : MatchCandidate,
    {left = right} + {left <> right}.
Proof. decide equality; try apply Bool.bool_dec; try apply path_eq_dec;
  try apply candidate_origin_eq_dec. Defined.

Definition candidate_authorized
    (catalogs : CandidateCatalogSet)
    (arrival : option MatchCandidate)
    (candidate : MatchCandidate) : Prop :=
  match candidate_origin candidate with
  | CandidateFresh => arrival = Some candidate
  | CandidateStored channel datum =>
      channel = candidate_channel candidate /\
      datum = candidate_datum candidate /\
      exists catalog,
        lookup_catalog channel catalogs = Some catalog /\
        In candidate (candidate_catalog_authority catalog)
  end.

Definition selected_channels (selected : list MatchCandidate) : list Path :=
  map candidate_channel selected.

Fixpoint nonpersistent_fresh_reservation_count
    (selected : list MatchCandidate) : nat :=
  match selected with
  | [] => 0
  | candidate :: tail =>
      match candidate_origin candidate with
      | CandidateFresh =>
          if candidate_persistent candidate
          then nonpersistent_fresh_reservation_count tail
          else S (nonpersistent_fresh_reservation_count tail)
      | CandidateStored _ _ => nonpersistent_fresh_reservation_count tail
      end
  end.

Record ExactSelection : Type := ExactSelectionState {
  exact_selection_requested_channels : list Path;
  exact_selection_catalogs : CandidateCatalogSet;
  exact_selection_arrival : option MatchCandidate;
  exact_selection_candidates : list MatchCandidate;
  exact_selection_channels_are_exact :
    selected_channels exact_selection_candidates =
      exact_selection_requested_channels;
  exact_selection_every_requested_channel_has_catalog :
    forall channel,
      In channel exact_selection_requested_channels ->
      exists catalog,
        lookup_catalog channel exact_selection_catalogs = Some catalog;
  exact_selection_candidates_are_authorized :
    Forall
      (candidate_authorized exact_selection_catalogs exact_selection_arrival)
      exact_selection_candidates;
  exact_selection_candidate_paths_are_valid :
    Forall candidate_paths_valid exact_selection_candidates;
  exact_selection_nonpersistent_fresh_reservation_is_unique :
    (nonpersistent_fresh_reservation_count
      exact_selection_candidates <= 1)%nat;
  exact_selection_arrival_is_fresh :
    match exact_selection_arrival with
    | None => True
    | Some arrival =>
        candidate_origin arrival = CandidateFresh /\
        candidate_origin_exact arrival /\
        candidate_paths_valid arrival
    end
}.

Theorem requested_channel_has_observed_catalog : forall selection channel,
  In channel (exact_selection_requested_channels selection) ->
  exists catalog,
    lookup_catalog channel (exact_selection_catalogs selection) = Some catalog.
Proof.
  intros selection channel Hin.
  now apply exact_selection_every_requested_channel_has_catalog.
Qed.

Theorem exact_selection_has_at_most_one_nonpersistent_fresh_reservation :
    forall selection,
  (nonpersistent_fresh_reservation_count
    (exact_selection_candidates selection) <= 1)%nat.
Proof.
  intro selection.
  exact (exact_selection_nonpersistent_fresh_reservation_is_unique selection).
Qed.

Lemma persistent_fresh_head_reserves_no_additional_position : forall
    candidate tail,
  candidate_origin candidate = CandidateFresh ->
  candidate_persistent candidate = true ->
  nonpersistent_fresh_reservation_count (candidate :: tail) =
    nonpersistent_fresh_reservation_count tail.
Proof.
  intros candidate tail Hfresh Hpersistent. simpl.
  now rewrite Hfresh, Hpersistent.
Qed.

Theorem selected_fresh_candidate_is_the_exact_arrival : forall
    selection candidate,
  In candidate (exact_selection_candidates selection) ->
  candidate_origin candidate = CandidateFresh ->
  exact_selection_arrival selection = Some candidate.
Proof.
  intros selection candidate Hin Hfresh.
  pose proof (exact_selection_candidates_are_authorized selection) as Hall.
  apply Forall_forall with (x := candidate) in Hall; [|exact Hin].
  unfold candidate_authorized in Hall. now rewrite Hfresh in Hall.
Qed.

Theorem selected_stored_candidate_retains_exact_catalog_metadata : forall
    selection candidate channel datum,
  In candidate (exact_selection_candidates selection) ->
  candidate_origin candidate = CandidateStored channel datum ->
  channel = candidate_channel candidate /\
  datum = candidate_datum candidate /\
  exists catalog,
    lookup_catalog channel (exact_selection_catalogs selection) = Some catalog /\
    In candidate (candidate_catalog_authority catalog).
Proof.
  intros selection candidate channel datum Hin Hstored.
  pose proof (exact_selection_candidates_are_authorized selection) as Hall.
  apply Forall_forall with (x := candidate) in Hall; [|exact Hin].
  unfold candidate_authorized in Hall. now rewrite Hstored in Hall.
Qed.

(** Live demand --------------------------------------------------------- *)

Fixpoint nonpersistent_stored_demand
    (channel datum : Path) (selected : list MatchCandidate) : Multiplicity :=
  match selected with
  | [] => 0%N
  | candidate :: tail =>
      let rest := nonpersistent_stored_demand channel datum tail in
      match candidate_origin candidate with
      | CandidateFresh => rest
      | CandidateStored _ _ =>
          if candidate_persistent candidate then rest
          else if path_eq_dec channel (candidate_channel candidate) then
            if path_eq_dec datum (candidate_datum candidate)
            then N.succ rest else rest
          else rest
      end
  end.

Definition selected_live_requirement
    (selected : list MatchCandidate) (candidate : MatchCandidate)
    : Multiplicity :=
  match candidate_origin candidate with
  | CandidateFresh => 0%N
  | CandidateStored _ _ =>
      if candidate_persistent candidate
      then 1%N
      else nonpersistent_stored_demand
        (candidate_channel candidate) (candidate_datum candidate) selected
  end.

Definition selection_authority_gate
    (before : TwoLevelAuthority) (selected : list MatchCandidate) : bool :=
  forallb
    (fun candidate =>
      N.leb (selected_live_requirement selected candidate)
        (lookup_multiplicity before
          (candidate_channel candidate) (candidate_datum candidate)))
    selected.

(** Candidate discovery is authorized by the immutable authority observed
    while the pool was built.  Commit may see unrelated mutations—even on a
    selected channel—so only the selected endpoints are revalidated there. *)
Record ObservedExactSelection (observed : TwoLevelAuthority) : Type :=
  ObservedExactSelectionState {
    observed_exact_selection : ExactSelection;
    observed_canonical_datum_codec : CanonicalDatumCodec;
    observed_stored_metadata_authority : StoredMetadataAuthority;
    observed_stored_metadata_is_exact :
      StoredMetadataAuthorityExact observed observed_canonical_datum_codec
        observed_stored_metadata_authority;
    observed_candidate_catalogs_are_exact_metadata_projections :
      candidate_catalog_set_coherent observed_stored_metadata_authority
        (exact_selection_catalogs observed_exact_selection)
  }.

Record SelectionLiveAt (observed commit : TwoLevelAuthority)
    (selection : ObservedExactSelection observed) : Type :=
  SelectionLiveAtState {
    selection_commit_is_canonical : canonical_channels commit;
    selection_commit_authority_gate :
      selection_authority_gate commit
        (exact_selection_candidates
          (observed_exact_selection selection)) = true
  }.

Record LiveExactSelection
    (observed commit : TwoLevelAuthority) : Type :=
  LiveExactSelectionState {
    live_observed_exact_selection : ObservedExactSelection observed;
    live_selection_at_commit :
      SelectionLiveAt commit live_observed_exact_selection
  }.

Definition live_exact_selection {observed commit}
    (selection : LiveExactSelection observed commit) : ExactSelection :=
  observed_exact_selection (live_observed_exact_selection selection).

Definition live_canonical_datum_codec {observed commit}
    (selection : LiveExactSelection observed commit) : CanonicalDatumCodec :=
  observed_canonical_datum_codec (live_observed_exact_selection selection).

Definition live_stored_metadata_authority {observed commit}
    (selection : LiveExactSelection observed commit) : StoredMetadataAuthority :=
  observed_stored_metadata_authority (live_observed_exact_selection selection).

Lemma live_stored_metadata_is_exact : forall observed commit
    (selection : LiveExactSelection observed commit),
  StoredMetadataAuthorityExact observed
    (live_canonical_datum_codec selection)
    (live_stored_metadata_authority selection).
Proof.
  intros observed commit selection.
  exact (observed_stored_metadata_is_exact
    (live_observed_exact_selection selection)).
Qed.

Lemma live_candidate_catalogs_are_exact_metadata_projections :
    forall observed commit (selection : LiveExactSelection observed commit),
  candidate_catalog_set_coherent (live_stored_metadata_authority selection)
    (exact_selection_catalogs (live_exact_selection selection)).
Proof.
  intros observed commit selection.
  exact (observed_candidate_catalogs_are_exact_metadata_projections
    (live_observed_exact_selection selection)).
Qed.

Lemma live_commit_is_canonical : forall observed commit
    (selection : LiveExactSelection observed commit),
  canonical_channels commit.
Proof.
  intros observed commit selection.
  exact (selection_commit_is_canonical (live_selection_at_commit selection)).
Qed.

Lemma live_exact_selection_authority_gate : forall observed commit
    (selection : LiveExactSelection observed commit),
  selection_authority_gate commit
    (exact_selection_candidates (live_exact_selection selection)) = true.
Proof.
  intros observed commit selection.
  exact (selection_commit_authority_gate (live_selection_at_commit selection)).
Qed.

Lemma canonical_lookup_channel_contents : forall before channel contents,
  canonical_channels before ->
  lookup_channel channel before = Some contents ->
  canonical_entries contents.
Proof.
  intros before channel contents Hcanonical.
  induction Hcanonical as
      [|head_channel head_contents tail Hvalid Hhead Hafter Htail IH];
    intro Hlookup; simpl in Hlookup; [discriminate|].
  destruct (path_eq_dec channel head_channel) as [Heq | Hneq].
  - inversion Hlookup; subst. exact Hhead.
  - now apply IH.
Qed.

Lemma canonical_lookup_multiplicity_in_bag_is_bounded : forall contents datum,
  canonical_entries contents ->
  (lookup_multiplicity_in_bag datum contents <= multiplicity_max)%N.
Proof.
  intros contents datum Hcanonical.
  induction Hcanonical as
      [|head multiplicity tail Hpath Hmultiplicity Hafter Htail IH]; simpl.
  - apply N.le_0_l.
  - destruct (path_eq_dec datum head) as [Heq | Hneq].
    + apply andb_true_iff in Hmultiplicity as [_ Hbounded].
      now apply N.leb_le.
    + exact IH.
Qed.

Lemma canonical_lookup_multiplicity_is_bounded : forall before channel datum,
  canonical_channels before ->
  (lookup_multiplicity before channel datum <= multiplicity_max)%N.
Proof.
  intros before channel datum Hcanonical.
  unfold lookup_multiplicity.
  destruct (lookup_channel channel before) as [contents|] eqn:Hlookup.
  - apply canonical_lookup_multiplicity_in_bag_is_bounded.
    now apply canonical_lookup_channel_contents with before channel.
  - apply N.le_0_l.
Qed.

Theorem live_selected_stored_candidate_has_exact_metadata_row : forall
    observed commit (selection : LiveExactSelection observed commit)
    candidate channel datum,
  In candidate
    (exact_selection_candidates (live_exact_selection selection)) ->
  candidate_origin candidate = CandidateStored channel datum ->
  exists row,
    In row (live_stored_metadata_authority selection) /\
    candidate = stored_metadata_candidate row /\
    stored_metadata_channel row = channel /\
    stored_metadata_datum row = datum /\
    stored_metadata_digest row = candidate_digest candidate /\
    stored_metadata_canonical_identity row =
      candidate_canonical_identity candidate /\
    stored_metadata_persistent row = candidate_persistent candidate.
Proof.
  intros observed commit selection candidate channel datum Hin Hstored.
  destruct (@selected_stored_candidate_retains_exact_catalog_metadata
    (live_exact_selection selection) candidate channel datum Hin Hstored)
    as [Hchannel [Hdatum [catalog [Hlookup Hincandidate]]]].
  pose proof (live_candidate_catalogs_are_exact_metadata_projections selection)
    as Hcoherent.
  assert (Hcatalog_member : In catalog
    (candidate_catalogs
      (exact_selection_catalogs (live_exact_selection selection)))).
  { unfold lookup_catalog in Hlookup.
    now apply lookup_candidate_catalog_some_is_member with channel. }
  specialize (Hcoherent catalog Hcatalog_member).
  rewrite Hcoherent in Hincandidate.
  apply in_map_iff in Hincandidate.
  destruct Hincandidate as [row [Heq Hinrow_filtered]].
  apply filter_In in Hinrow_filtered as [Hinrow Hrow_channel].
  exists row. split; [exact Hinrow|].
  split; [symmetry; exact Heq|].
  unfold metadata_at_channel in Hrow_channel.
  destruct (path_eq_dec
    (candidate_catalog_channel catalog) (stored_metadata_channel row));
    try discriminate.
  subst candidate. simpl in *.
  inversion Hstored; subst. repeat split; reflexivity.
Qed.

Theorem live_stored_persistent_selection_requires_presence : forall
    observed commit (selection : LiveExactSelection observed commit)
    candidate channel datum,
  In candidate
    (exact_selection_candidates (live_exact_selection selection)) ->
  candidate_origin candidate = CandidateStored channel datum ->
  candidate_persistent candidate = true ->
  (1 <= lookup_multiplicity commit channel datum)%N.
Proof.
  intros observed commit selection candidate channel datum
    Hin Hstored Hpersistent.
  pose proof (live_exact_selection_authority_gate selection) as Hgate.
  unfold selection_authority_gate in Hgate.
  apply forallb_forall with (x := candidate) in Hgate; [|exact Hin].
  unfold selected_live_requirement in Hgate. rewrite Hstored, Hpersistent in Hgate.
  apply N.leb_le in Hgate.
  pose proof (@selected_stored_candidate_retains_exact_catalog_metadata
    (live_exact_selection selection) candidate channel datum Hin Hstored)
    as [Hchannel [Hdatum _]].
  now rewrite <- Hchannel, <- Hdatum in Hgate.
Qed.

Theorem live_stored_nonpersistent_selection_requires_aggregate_demand : forall
    observed commit (selection : LiveExactSelection observed commit)
    candidate channel datum,
  In candidate
    (exact_selection_candidates (live_exact_selection selection)) ->
  candidate_origin candidate = CandidateStored channel datum ->
  candidate_persistent candidate = false ->
  (nonpersistent_stored_demand channel datum
      (exact_selection_candidates (live_exact_selection selection)) <=
    lookup_multiplicity commit channel datum)%N.
Proof.
  intros observed commit selection candidate channel datum
    Hin Hstored Hpersistent.
  pose proof (live_exact_selection_authority_gate selection) as Hgate.
  unfold selection_authority_gate in Hgate.
  apply forallb_forall with (x := candidate) in Hgate; [|exact Hin].
  unfold selected_live_requirement in Hgate. rewrite Hstored, Hpersistent in Hgate.
  apply N.leb_le in Hgate.
  pose proof (@selected_stored_candidate_retains_exact_catalog_metadata
    (live_exact_selection selection) candidate channel datum Hin Hstored)
    as [Hchannel [Hdatum _]].
  now rewrite <- Hchannel, <- Hdatum in Hgate.
Qed.

Lemma positive_nonpersistent_demand_has_selected_witness : forall
    selected channel datum,
  (0 < nonpersistent_stored_demand channel datum selected)%N ->
  exists candidate stored_channel stored_datum,
    In candidate selected /\
    candidate_origin candidate =
      CandidateStored stored_channel stored_datum /\
    candidate_persistent candidate = false /\
    candidate_channel candidate = channel /\
    candidate_datum candidate = datum.
Proof.
  intros selected. induction selected as [|candidate tail IH];
    intros channel datum Hpositive; simpl in Hpositive; [nia|].
  destruct (candidate_origin candidate)
    as [|stored_channel stored_datum] eqn:Horigin.
  - destruct (IH channel datum Hpositive)
      as [witness [witness_channel [witness_datum Hproperties]]].
    exists witness, witness_channel, witness_datum.
    destruct Hproperties as [Hin Hproperties].
    split; [now right|exact Hproperties].
  - destruct (candidate_persistent candidate) eqn:Hpersistent.
    + destruct (IH channel datum Hpositive)
        as [witness [witness_channel [witness_datum Hproperties]]].
      exists witness, witness_channel, witness_datum.
      destruct Hproperties as [Hin Hproperties].
      split; [now right|exact Hproperties].
    + destruct (path_eq_dec channel (candidate_channel candidate))
        as [Hchannel | Hchannel].
      * destruct (path_eq_dec datum (candidate_datum candidate))
          as [Hdatum | Hdatum].
        -- exists candidate, stored_channel, stored_datum.
           repeat split.
           ++ now left.
           ++ exact Horigin.
           ++ exact Hpersistent.
           ++ now symmetry.
           ++ now symmetry.
        -- destruct (IH channel datum Hpositive)
             as [witness [witness_channel [witness_datum Hproperties]]].
           exists witness, witness_channel, witness_datum.
           destruct Hproperties as [Hin Hproperties].
           split; [now right|exact Hproperties].
      * destruct (IH channel datum Hpositive)
          as [witness [witness_channel [witness_datum Hproperties]]].
        exists witness, witness_channel, witness_datum.
        destruct Hproperties as [Hin Hproperties].
        split; [now right|exact Hproperties].
Qed.

Theorem live_nonpersistent_demand_is_multiplicity_bounded : forall
    observed commit (selection : LiveExactSelection observed commit)
    channel datum,
  (nonpersistent_stored_demand channel datum
      (exact_selection_candidates (live_exact_selection selection)) <=
    multiplicity_max)%N.
Proof.
  intros observed commit selection channel datum.
  destruct (N.eq_dec
    (nonpersistent_stored_demand channel datum
      (exact_selection_candidates (live_exact_selection selection))) 0%N)
    as [Hzero | Hnonzero].
  - rewrite Hzero. apply N.le_0_l.
  - assert (Hpositive :
      (0 < nonpersistent_stored_demand channel datum
        (exact_selection_candidates (live_exact_selection selection)))%N)
      by now apply N.neq_0_lt_0.
    destruct (@positive_nonpersistent_demand_has_selected_witness
      (exact_selection_candidates (live_exact_selection selection))
      channel datum Hpositive)
      as (candidate & stored_channel & stored_datum & Hin & Horigin &
          Hpersistent & Hchannel & Hdatum).
    pose proof (@selected_stored_candidate_retains_exact_catalog_metadata
      (live_exact_selection selection) candidate stored_channel stored_datum
      Hin Horigin) as [Horigin_channel [Horigin_datum _]].
    pose proof (@live_stored_nonpersistent_selection_requires_aggregate_demand
      observed commit selection candidate stored_channel stored_datum
      Hin Horigin Hpersistent) as Hlive.
    rewrite Horigin_channel, Horigin_datum, Hchannel, Hdatum in Hlive.
    eapply N.le_trans; [exact Hlive|].
    apply canonical_lookup_multiplicity_is_bounded.
    exact (@live_commit_is_canonical observed commit selection).
Qed.

Lemma fresh_head_contributes_zero_stored_demand : forall
    tail candidate channel datum,
  candidate_origin candidate = CandidateFresh ->
  nonpersistent_stored_demand channel datum (candidate :: tail) =
  nonpersistent_stored_demand channel datum tail.
Proof.
  intros tail candidate channel datum Hfresh. simpl. now rewrite Hfresh.
Qed.

Lemma persistent_head_contributes_zero_nonpersistent_demand : forall
    tail candidate channel datum stored_channel stored_datum,
  candidate_origin candidate = CandidateStored stored_channel stored_datum ->
  candidate_persistent candidate = true ->
  nonpersistent_stored_demand channel datum (candidate :: tail) =
  nonpersistent_stored_demand channel datum tail.
Proof.
  intros tail candidate channel datum stored_channel stored_datum
    Hstored Hpersistent.
  simpl. now rewrite Hstored, Hpersistent.
Qed.

(** Signed PathMap delta accumulation ----------------------------------- *)

Definition LogicalKey : Type := (Path * Path)%type.

Definition logical_key_eq_dec : forall left right : LogicalKey,
    {left = right} + {left <> right}.
Proof. decide equality; apply path_eq_dec. Defined.

Inductive ChangePolarity : Type := ChangeAddition | ChangeRemoval.

Record ChangeEvent : Type := ChangeEventState {
  change_event_key : LogicalKey;
  change_event_polarity : ChangePolarity;
  change_event_amount : Multiplicity
}.

Definition signed_event_amount (event : ChangeEvent) : Z :=
  match change_event_polarity event with
  | ChangeAddition => Z.of_N (change_event_amount event)
  | ChangeRemoval => - Z.of_N (change_event_amount event)
  end.

Definition SignedAccumulator : Type := LogicalKey -> Z.
Definition empty_signed_accumulator : SignedAccumulator := fun _ => 0%Z.

Definition accumulator_update
    (key : LogicalKey) (value : Z) (before : SignedAccumulator)
    : SignedAccumulator :=
  fun selected =>
    if logical_key_eq_dec selected key then value else before selected.

Lemma accumulator_update_target : forall key value before,
  accumulator_update key value before key = value.
Proof.
  intros key value before. unfold accumulator_update.
  destruct (logical_key_eq_dec key key); [reflexivity|contradiction].
Qed.

Lemma accumulator_update_other : forall key value before other,
  other <> key ->
  accumulator_update key value before other = before other.
Proof.
  intros key value before other Hother. unfold accumulator_update.
  destruct (logical_key_eq_dec other key); [contradiction|reflexivity].
Qed.

Definition signed_cell_within_multiplicity (cell : Z) : bool :=
  Z.leb (Z.abs cell) (Z.of_N multiplicity_max).

Definition record_change
    (before : SignedAccumulator) (event : ChangeEvent)
    : option SignedAccumulator :=
  let after :=
    (before (change_event_key event) + signed_event_amount event)%Z in
  if signed_cell_within_multiplicity after
  then Some (accumulator_update (change_event_key event) after before)
  else None.

Fixpoint record_changes
    (before : SignedAccumulator) (events : list ChangeEvent)
    : option SignedAccumulator :=
  match events with
  | [] => Some before
  | event :: tail =>
      match record_change before event with
      | None => None
      | Some after => record_changes after tail
      end
  end.

Fixpoint event_net (events : list ChangeEvent) (key : LogicalKey) : Z :=
  match events with
  | [] => 0%Z
  | event :: tail =>
      ((if logical_key_eq_dec key (change_event_key event)
        then signed_event_amount event else 0%Z) + event_net tail key)%Z
  end.

Lemma event_net_singleton_at_own_key : forall key polarity amount,
  event_net [ChangeEventState key polarity amount] key =
  signed_event_amount (ChangeEventState key polarity amount).
Proof.
  intros key polarity amount. simpl.
  destruct (logical_key_eq_dec key key); [lia | contradiction].
Qed.

Lemma record_change_target_is_exact : forall before event after,
  record_change before event = Some after ->
  after (change_event_key event) =
    (before (change_event_key event) + signed_event_amount event)%Z.
Proof.
  intros before event after Hrecord.
  unfold record_change in Hrecord.
  destruct (signed_cell_within_multiplicity
    (before (change_event_key event) + signed_event_amount event)) eqn:Hbound;
    inversion Hrecord; subst; clear Hrecord.
  unfold accumulator_update.
  destruct (logical_key_eq_dec (change_event_key event)
    (change_event_key event)); [reflexivity | contradiction].
Qed.

Lemma record_change_preserves_every_other_key : forall before event after key,
  record_change before event = Some after ->
  key <> change_event_key event ->
  after key = before key.
Proof.
  intros before event after key Hrecord Hneq.
  unfold record_change in Hrecord.
  destruct (signed_cell_within_multiplicity
    (before (change_event_key event) + signed_event_amount event));
    inversion Hrecord; subst; clear Hrecord.
  unfold accumulator_update.
  destruct (logical_key_eq_dec key (change_event_key event));
    [contradiction | reflexivity].
Qed.

Definition accumulator_bounded (accumulator : SignedAccumulator) : Prop :=
  forall key,
    (Z.abs (accumulator key) <= Z.of_N multiplicity_max)%Z.

Lemma empty_signed_accumulator_is_bounded :
  accumulator_bounded empty_signed_accumulator.
Proof.
  intro key. unfold empty_signed_accumulator. simpl.
  pose proof (N.le_0_l multiplicity_max). lia.
Qed.

Lemma successful_record_change_preserves_boundedness : forall before event after,
  accumulator_bounded before ->
  record_change before event = Some after ->
  accumulator_bounded after.
Proof.
  intros before event after Hbounded Hrecord key.
  destruct (logical_key_eq_dec key (change_event_key event)) as [Heq | Hneq].
  - subst key. rewrite (@record_change_target_is_exact before event after Hrecord).
    unfold record_change in Hrecord.
    destruct (signed_cell_within_multiplicity
      (before (change_event_key event) + signed_event_amount event)) eqn:Hbound;
      try discriminate.
    unfold signed_cell_within_multiplicity in Hbound.
    now apply Z.leb_le.
  - rewrite (@record_change_preserves_every_other_key before event after key
      Hrecord Hneq).
    apply Hbounded.
Qed.

Theorem successful_record_changes_are_bounded : forall before events after,
  accumulator_bounded before ->
  record_changes before events = Some after ->
  accumulator_bounded after.
Proof.
  intros before events. revert before.
  induction events as [|event tail IH]; intros before after Hbounded Hrecord.
  - simpl in Hrecord. inversion Hrecord; subst. exact Hbounded.
  - simpl in Hrecord.
    destruct (record_change before event) as [middle|] eqn:Hstep;
      try discriminate.
    eapply IH; [|exact Hrecord].
    eapply successful_record_change_preserves_boundedness; eauto.
Qed.

Theorem successful_record_changes_equal_exact_signed_fold : forall
    before events after key,
  record_changes before events = Some after ->
  after key = (before key + event_net events key)%Z.
Proof.
  intros before events. revert before.
  induction events as [|event tail IH]; intros before after key Hrecord.
  - simpl in Hrecord. inversion Hrecord; subst. simpl. lia.
  - simpl in Hrecord.
    destruct (record_change before event) as [middle|] eqn:Hstep;
      try discriminate.
    specialize (IH middle after key Hrecord).
    rewrite IH. simpl.
    destruct (logical_key_eq_dec key (change_event_key event)) as [Heq | Hneq].
    + subst key.
      rewrite (@record_change_target_is_exact before event middle Hstep).
      destruct (logical_key_eq_dec (change_event_key event)
        (change_event_key event)); [lia | contradiction].
    + rewrite (@record_change_preserves_every_other_key before event middle key
        Hstep Hneq).
      destruct (logical_key_eq_dec key (change_event_key event));
        [contradiction | lia].
Qed.

Corollary successful_preparation_accumulator_is_exact : forall events after key,
  record_changes empty_signed_accumulator events = Some after ->
  after key = event_net events key.
Proof.
  intros events after key Hrecord.
  pose proof (@successful_record_changes_equal_exact_signed_fold
    empty_signed_accumulator events after key Hrecord) as Hexact.
  unfold empty_signed_accumulator in Hexact. lia.
Qed.

Inductive PrefixBounded : SignedAccumulator -> list ChangeEvent -> Prop :=
| PrefixBoundedNil : forall before, PrefixBounded before []
| PrefixBoundedCons : forall before event tail after,
    signed_cell_within_multiplicity
      (before (change_event_key event) + signed_event_amount event) = true ->
    after = accumulator_update (change_event_key event)
      (before (change_event_key event) + signed_event_amount event) before ->
    PrefixBounded after tail ->
    PrefixBounded before (event :: tail).

Theorem prefix_bounded_changes_construct_an_accumulator : forall before events,
  PrefixBounded before events ->
  exists after, record_changes before events = Some after.
Proof.
  intros before events Hbounded. induction Hbounded.
  - exists before. reflexivity.
  - destruct IHHbounded as [final Hfinal]. exists final. simpl.
    unfold record_change. rewrite H. subst after. exact Hfinal.
Qed.

Theorem successful_record_changes_are_prefix_bounded : forall before events after,
  record_changes before events = Some after ->
  PrefixBounded before events.
Proof.
  intros before events. revert before.
  induction events as [|event tail IH]; intros before after Hrecord.
  - constructor.
  - simpl in Hrecord.
    destruct (record_change before event) as [middle|] eqn:Hstep;
      [|discriminate].
    eapply PrefixBoundedCons with (after := middle).
    + unfold record_change in Hstep.
      destruct (signed_cell_within_multiplicity
        (before (change_event_key event) + signed_event_amount event))
        eqn:Hbound; [reflexivity|discriminate].
    + unfold record_change in Hstep.
      destruct (signed_cell_within_multiplicity
        (before (change_event_key event) + signed_event_amount event));
        inversion Hstep. reflexivity.
    + now apply IH with after.
Qed.

Lemma record_changes_append : forall before left right,
  record_changes before (left ++ right) =
  match record_changes before left with
  | None => None
  | Some middle => record_changes middle right
  end.
Proof.
  intros before left. revert before.
  induction left as [|event tail IH]; intros before right; simpl;
    [reflexivity|].
  destruct (record_change before event); [apply IH|reflexivity].
Qed.

(** Match-derived events ------------------------------------------------ *)

Definition candidate_key (candidate : MatchCandidate) : LogicalKey :=
  (candidate_channel candidate, candidate_datum candidate).

Fixpoint selected_removal_events (selected : list MatchCandidate)
    : list ChangeEvent :=
  match selected with
  | [] => []
  | candidate :: tail =>
      let rest := selected_removal_events tail in
      match candidate_origin candidate with
      | CandidateFresh => rest
      | CandidateStored _ _ =>
          if candidate_persistent candidate
          then rest
          else ChangeEventState (candidate_key candidate) ChangeRemoval 1%N :: rest
      end
  end.

Lemma nonpersistent_demand_tail_is_monotone : forall
    candidate tail channel datum,
  (nonpersistent_stored_demand channel datum tail <=
   nonpersistent_stored_demand channel datum (candidate :: tail))%N.
Proof.
  intros candidate tail channel datum. simpl.
  destruct (candidate_origin candidate); [apply N.le_refl|].
  destruct (candidate_persistent candidate); [apply N.le_refl|].
  destruct (path_eq_dec channel (candidate_channel candidate));
    [|apply N.le_refl].
  destruct (path_eq_dec datum (candidate_datum candidate));
    [apply N.le_succ_diag_r|apply N.le_refl].
Qed.

Lemma nonpersistent_demand_at_emitted_head_is_successor : forall
    candidate tail stored_channel stored_datum,
  candidate_origin candidate = CandidateStored stored_channel stored_datum ->
  candidate_persistent candidate = false ->
  nonpersistent_stored_demand
    (candidate_channel candidate) (candidate_datum candidate)
    (candidate :: tail) =
  N.succ (nonpersistent_stored_demand
    (candidate_channel candidate) (candidate_datum candidate) tail).
Proof.
  intros candidate tail stored_channel stored_datum Horigin Hpersistent.
  simpl. rewrite Horigin, Hpersistent.
  destruct (path_eq_dec (candidate_channel candidate)
    (candidate_channel candidate)); [|contradiction].
  destruct (path_eq_dec (candidate_datum candidate)
    (candidate_datum candidate)); [reflexivity|contradiction].
Qed.

Lemma nonpersistent_demand_at_other_key_ignores_emitted_head : forall
    candidate tail stored_channel stored_datum channel datum,
  candidate_origin candidate = CandidateStored stored_channel stored_datum ->
  candidate_persistent candidate = false ->
  (channel, datum) <> candidate_key candidate ->
  nonpersistent_stored_demand channel datum (candidate :: tail) =
  nonpersistent_stored_demand channel datum tail.
Proof.
  intros candidate tail stored_channel stored_datum channel datum
    Horigin Hpersistent Hother.
  simpl. rewrite Horigin, Hpersistent.
  destruct (path_eq_dec channel (candidate_channel candidate))
    as [Hchannel | Hchannel]; [|reflexivity].
  destruct (path_eq_dec datum (candidate_datum candidate))
    as [Hdatum | Hdatum]; [|reflexivity].
  exfalso. apply Hother. unfold candidate_key. now subst.
Qed.

Definition removal_accumulator_matches
    (full remaining : list MatchCandidate)
    (accumulator : SignedAccumulator) : Prop :=
  forall channel datum,
    accumulator (channel, datum) =
      (- (Z.of_N (nonpersistent_stored_demand channel datum full) -
          Z.of_N (nonpersistent_stored_demand channel datum remaining)))%Z.

Lemma selected_removals_are_prefix_bounded_from_matching_accumulator :
    forall full remaining accumulator,
  removal_accumulator_matches full remaining accumulator ->
  (forall channel datum,
    (nonpersistent_stored_demand channel datum remaining <=
     nonpersistent_stored_demand channel datum full)%N) ->
  (forall channel datum,
    (nonpersistent_stored_demand channel datum full <=
     multiplicity_max)%N) ->
  PrefixBounded accumulator (selected_removal_events remaining).
Proof.
  intros full remaining. revert full.
  induction remaining as [|candidate tail IH];
    intros full accumulator Hmatches Hremaining Hfull; simpl.
  - constructor.
  - destruct (candidate_origin candidate)
      as [|stored_channel stored_datum] eqn:Horigin.
    + apply IH with (full := full).
      * intros channel datum. specialize (Hmatches channel datum).
        unfold removal_accumulator_matches in Hmatches |- *.
        simpl in Hmatches. now rewrite Horigin in Hmatches.
      * intros channel datum. eapply N.le_trans.
        -- apply nonpersistent_demand_tail_is_monotone.
        -- apply Hremaining.
      * exact Hfull.
    + destruct (candidate_persistent candidate) eqn:Hpersistent.
      * apply IH with (full := full).
        -- intros channel datum. specialize (Hmatches channel datum).
           unfold removal_accumulator_matches in Hmatches |- *.
           simpl in Hmatches. now rewrite Horigin, Hpersistent in Hmatches.
        -- intros channel datum. eapply N.le_trans.
           ++ apply nonpersistent_demand_tail_is_monotone.
           ++ apply Hremaining.
        -- exact Hfull.
      * eapply PrefixBoundedCons with (after := accumulator_update
          (candidate_key candidate)
          (accumulator (candidate_key candidate) - 1)%Z accumulator).
        -- unfold signed_cell_within_multiplicity, signed_event_amount.
           unfold candidate_key. cbn [change_event_key].
           rewrite (Hmatches
             (candidate_channel candidate) (candidate_datum candidate)).
           rewrite (@nonpersistent_demand_at_emitted_head_is_successor
             candidate tail stored_channel stored_datum Horigin Hpersistent).
           apply Z.leb_le. rewrite N2Z.inj_succ. cbn. ring_simplify.
           rewrite Z.abs_neq.
           ++ match goal with
              | |- (?left <= _)%Z =>
                  change (left <= Z.of_N multiplicity_max)%Z
              end.
              pose proof (Hremaining
                (candidate_channel candidate) (candidate_datum candidate))
                as Hremaining_bound.
              pose proof (Hfull
                (candidate_channel candidate) (candidate_datum candidate))
                as Hfull_bound.
              apply N2Z.inj_le in Hremaining_bound.
              apply N2Z.inj_le in Hfull_bound. lia.
           ++ pose proof (Hremaining
                (candidate_channel candidate) (candidate_datum candidate))
                as Hremaining_bound.
              rewrite (@nonpersistent_demand_at_emitted_head_is_successor
                candidate tail stored_channel stored_datum
                Horigin Hpersistent) in Hremaining_bound.
              apply N2Z.inj_le in Hremaining_bound.
              rewrite N2Z.inj_succ in Hremaining_bound. lia.
        -- unfold signed_event_amount. cbn. reflexivity.
        -- apply IH with (full := full).
           ++ intros channel datum.
              destruct (logical_key_eq_dec (channel, datum)
                (candidate_key candidate)) as [Hkey | Hkey].
              ** inversion Hkey; subst channel datum.
                 rewrite accumulator_update_target.
                 unfold candidate_key.
                 rewrite (Hmatches
                   (candidate_channel candidate) (candidate_datum candidate)).
                 rewrite (@nonpersistent_demand_at_emitted_head_is_successor
                   candidate tail stored_channel stored_datum
                   Horigin Hpersistent).
                 rewrite N2Z.inj_succ. ring.
              ** rewrite accumulator_update_other by exact Hkey.
                 rewrite (Hmatches channel datum).
                 rewrite (@nonpersistent_demand_at_other_key_ignores_emitted_head
                   candidate tail stored_channel stored_datum channel datum
                   Horigin Hpersistent Hkey). reflexivity.
           ++ intros channel datum. eapply N.le_trans.
              ** apply nonpersistent_demand_tail_is_monotone.
              ** apply Hremaining.
           ++ exact Hfull.
Qed.

Theorem selected_removals_are_prefix_bounded : forall selected,
  (forall channel datum,
    (nonpersistent_stored_demand channel datum selected <=
     multiplicity_max)%N) ->
  PrefixBounded empty_signed_accumulator
    (selected_removal_events selected).
Proof.
  intros selected Hbounded.
  eapply selected_removals_are_prefix_bounded_from_matching_accumulator
    with (full := selected) (remaining := selected).
  - intros channel datum. unfold empty_signed_accumulator. lia.
  - intros channel datum. apply N.le_refl.
  - exact Hbounded.
Qed.

Definition candidate_selectedb
    (candidate : MatchCandidate) (selected : list MatchCandidate) : bool :=
  if in_dec match_candidate_eq_dec candidate selected then true else false.

Definition unselected_arrival_events
    (arrival : option MatchCandidate) (selected : list MatchCandidate)
    : list ChangeEvent :=
  match arrival with
  | None => []
  | Some candidate =>
      if candidate_selectedb candidate selected
      then []
      else [ChangeEventState (candidate_key candidate) ChangeAddition 1%N]
  end.

Definition exact_match_change_events (selection : ExactSelection)
    : list ChangeEvent :=
  selected_removal_events (exact_selection_candidates selection) ++
  unselected_arrival_events
    (exact_selection_arrival selection)
    (exact_selection_candidates selection).

Lemma persistent_candidate_emits_no_direct_removal : forall candidate tail,
  candidate_persistent candidate = true ->
  selected_removal_events (candidate :: tail) =
  selected_removal_events tail.
Proof.
  intros candidate tail Hpersistent. simpl.
  destruct (candidate_origin candidate); [reflexivity |].
  now rewrite Hpersistent.
Qed.

Lemma fresh_candidate_emits_no_direct_removal : forall candidate tail,
  candidate_origin candidate = CandidateFresh ->
  selected_removal_events (candidate :: tail) =
  selected_removal_events tail.
Proof.
  intros candidate tail Hfresh. simpl. now rewrite Hfresh.
Qed.

Lemma selected_fresh_arrival_emits_no_addition : forall selection arrival,
  exact_selection_arrival selection = Some arrival ->
  In arrival (exact_selection_candidates selection) ->
  unselected_arrival_events
    (exact_selection_arrival selection)
    (exact_selection_candidates selection) = [].
Proof.
  intros selection arrival Harrival Hin.
  unfold unselected_arrival_events. rewrite Harrival.
  unfold candidate_selectedb.
  destruct (in_dec match_candidate_eq_dec arrival
    (exact_selection_candidates selection)); [reflexivity | contradiction].
Qed.

Lemma unselected_fresh_arrival_emits_one_exact_addition : forall
    selection arrival,
  exact_selection_arrival selection = Some arrival ->
  ~ In arrival (exact_selection_candidates selection) ->
  unselected_arrival_events
    (exact_selection_arrival selection)
    (exact_selection_candidates selection) =
  [ChangeEventState (candidate_key arrival) ChangeAddition 1%N].
Proof.
  intros selection arrival Harrival Hnotin.
  unfold unselected_arrival_events. rewrite Harrival.
  unfold candidate_selectedb.
  destruct (in_dec match_candidate_eq_dec arrival
    (exact_selection_candidates selection)); [contradiction | reflexivity].
Qed.

Lemma selected_removal_event_net_is_exact_demand : forall
    selected channel datum,
  event_net (selected_removal_events selected) (channel, datum) =
    (- Z.of_N (nonpersistent_stored_demand channel datum selected))%Z.
Proof.
  intros selected. induction selected as [|candidate tail IH];
    intros channel datum; simpl; [reflexivity |].
  destruct (candidate_origin candidate) as [|stored_channel stored_datum] eqn:Horigin.
  - apply IH.
  - destruct (candidate_persistent candidate) eqn:Hpersistent.
    + apply IH.
    + simpl. unfold candidate_key. simpl.
      destruct (path_eq_dec channel (candidate_channel candidate))
        as [Hchannel | Hchannel].
      * subst channel.
        destruct (path_eq_dec datum (candidate_datum candidate))
          as [Hdatum | Hdatum].
        -- subst datum.
           change ((-1 + event_net (selected_removal_events tail)
             (candidate_channel candidate, candidate_datum candidate))%Z =
             (- Z.of_N (N.succ (nonpersistent_stored_demand
               (candidate_channel candidate) (candidate_datum candidate) tail)))%Z).
           rewrite (IH (candidate_channel candidate)
             (candidate_datum candidate)), N2Z.inj_succ. lia.
        -- destruct (logical_key_eq_dec (candidate_channel candidate, datum)
             (candidate_channel candidate, candidate_datum candidate)) as [Heq | Hneq].
           ++ inversion Heq. contradiction.
           ++ exact (IH (candidate_channel candidate) datum).
      * destruct (logical_key_eq_dec (channel, datum)
          (candidate_channel candidate, candidate_datum candidate)) as [Heq | Hneq].
        -- inversion Heq. contradiction.
        -- exact (IH channel datum).
Qed.

Lemma event_net_append : forall left right key,
  event_net (left ++ right) key =
    (event_net left key + event_net right key)%Z.
Proof.
  intros left. induction left as [|event tail IH]; intros right key; simpl.
  - lia.
  - rewrite IH. lia.
Qed.

Theorem exact_match_event_net_is_demand_plus_optional_arrival : forall
    selection channel datum,
  event_net (exact_match_change_events selection) (channel, datum) =
  (- Z.of_N (nonpersistent_stored_demand channel datum
      (exact_selection_candidates selection)) +
    event_net
      (unselected_arrival_events
        (exact_selection_arrival selection)
        (exact_selection_candidates selection))
      (channel, datum))%Z.
Proof.
  intros selection channel datum.
  unfold exact_match_change_events. rewrite event_net_append.
  rewrite selected_removal_event_net_is_exact_demand. reflexivity.
Qed.

Theorem exact_live_match_changes_record_constructively : forall
    observed commit (selection : LiveExactSelection observed commit),
  exists accumulator,
    record_changes empty_signed_accumulator
      (exact_match_change_events (live_exact_selection selection)) =
    Some accumulator.
Proof.
  intros observed commit selection.
  set (selected := exact_selection_candidates
    (live_exact_selection selection)).
  assert (Hremoval_prefix : PrefixBounded empty_signed_accumulator
      (selected_removal_events selected)).
  { apply selected_removals_are_prefix_bounded.
    intros channel datum. subst selected.
    exact (@live_nonpersistent_demand_is_multiplicity_bounded
      observed commit selection channel datum). }
  destruct (prefix_bounded_changes_construct_an_accumulator Hremoval_prefix)
    as [after_removals Hremovals].
  unfold exact_match_change_events.
  fold selected.
  rewrite record_changes_append, Hremovals.
  unfold unselected_arrival_events.
  destruct (exact_selection_arrival (live_exact_selection selection))
    as [arrival|] eqn:Harrival; simpl.
  - destruct (candidate_selectedb arrival selected) eqn:Hselected.
    + now exists after_removals.
    + assert (Hafter_removals :
          after_removals (candidate_key arrival) =
          (- Z.of_N (nonpersistent_stored_demand
            (candidate_channel arrival) (candidate_datum arrival)
            selected))%Z).
      { rewrite (@successful_preparation_accumulator_is_exact
          (selected_removal_events selected) after_removals
          (candidate_key arrival) Hremovals).
        unfold candidate_key.
        apply selected_removal_event_net_is_exact_demand. }
      assert (Hdemand_bound :
          (nonpersistent_stored_demand
            (candidate_channel arrival) (candidate_datum arrival) selected <=
           multiplicity_max)%N).
      { subst selected.
        exact (@live_nonpersistent_demand_is_multiplicity_bounded
          observed commit selection
          (candidate_channel arrival) (candidate_datum arrival)). }
      assert (Hcell : signed_cell_within_multiplicity
          (after_removals (candidate_key arrival) + 1)%Z = true).
      { unfold signed_cell_within_multiplicity.
        apply Z.leb_le.
        match goal with
        | |- (Z.abs ?cell <= _)%Z =>
            change (Z.abs cell <= Z.of_N multiplicity_max)%Z
        end.
        rewrite Hafter_removals.
        apply N2Z.inj_le in Hdemand_bound.
        assert (Hone : (1 <= Z.of_N multiplicity_max)%Z).
        { change (1 <= 18446744073709551615)%Z. lia. }
        apply Z.abs_le. lia. }
      exists (accumulator_update (candidate_key arrival)
        (after_removals (candidate_key arrival) + 1)%Z after_removals).
      simpl. unfold record_change, signed_event_amount. simpl.
      now rewrite Hcell.
  - now exists after_removals.
Qed.

Theorem exact_live_match_changes_are_automatically_prefix_bounded : forall
    observed commit (selection : LiveExactSelection observed commit),
  PrefixBounded empty_signed_accumulator
    (exact_match_change_events (live_exact_selection selection)).
Proof.
  intros observed commit selection.
  destruct (@exact_live_match_changes_record_constructively
    observed commit selection) as [accumulator Hrecord].
  now apply successful_record_changes_are_prefix_bounded with accumulator.
Qed.

(** Canonical nested key construction ---------------------------------- *)

(** This is the logical model of exact PathMap insertion used while PREPARE
    accumulates keys.  It inserts at the comparison point and never performs
    a post-hoc sort.  Counts are fixed to one because the index records key
    presence; signed multiplicity remains solely in [SignedAccumulator]. *)
Fixpoint upsert_index_datum (selected : Path) (contents : ChannelBag)
    : ChannelBag :=
  match contents with
  | [] => [(selected, 1%N)]
  | (datum, count) :: tail =>
      if path_eq_dec selected datum
      then (datum, 1%N) :: tail
      else if path_ltb selected datum
           then (selected, 1%N) :: (datum, count) :: tail
           else (datum, count) :: upsert_index_datum selected tail
  end.

Lemma one_is_valid_multiplicity : multiplicity_valid 1%N = true.
Proof. reflexivity. Qed.

Lemma upsert_index_datum_preserves_key_forall : forall
    (predicate : Path -> Prop) contents selected,
  predicate selected ->
  Forall (fun entry => predicate (entry_path entry)) contents ->
  Forall (fun entry => predicate (entry_path entry))
    (upsert_index_datum selected contents).
Proof.
  intros predicate contents. induction contents as [|[datum count] tail IH];
    intros selected Hselected Hall; inversion Hall; subst; simpl.
  - repeat constructor; assumption.
  - destruct (path_eq_dec selected datum).
    + constructor; assumption.
    + destruct (path_ltb selected datum).
      * repeat constructor; assumption.
      * constructor; [assumption |]. now apply IH.
Qed.

Lemma upsert_index_datum_target_is_one : forall contents selected,
  lookup_multiplicity_in_bag selected
    (upsert_index_datum selected contents) = 1%N.
Proof.
  intros contents. induction contents as [|[datum count] tail IH];
    intro selected; simpl.
  - destruct (path_eq_dec selected selected); [reflexivity | contradiction].
  - destruct (path_eq_dec selected datum) as [Heq | Hneq].
    + subst. simpl. destruct (path_eq_dec datum datum);
        [reflexivity | contradiction].
    + destruct (path_ltb selected datum); simpl.
      * destruct (path_eq_dec selected selected); [reflexivity | contradiction].
      * destruct (path_eq_dec selected datum); [contradiction | apply IH].
Qed.

Lemma upsert_index_datum_preserves_other : forall contents selected other,
  other <> selected ->
  lookup_multiplicity_in_bag other (upsert_index_datum selected contents) =
  lookup_multiplicity_in_bag other contents.
Proof.
  intros contents. induction contents as [|[datum count] tail IH];
    intros selected other Hneq; simpl.
  - destruct (path_eq_dec other selected); [contradiction | reflexivity].
  - destruct (path_eq_dec selected datum) as [Heq | Hnot].
    + subst datum. simpl.
      destruct (path_eq_dec other selected); [contradiction | reflexivity].
    + destruct (path_ltb selected datum); simpl.
      * destruct (path_eq_dec other selected); [contradiction | reflexivity].
      * destruct (path_eq_dec other datum); [reflexivity |].
        now apply IH.
Qed.

Lemma canonical_upsert_index_datum : forall contents selected,
  canonical_entries contents ->
  path_valid selected = true ->
  canonical_entries (upsert_index_datum selected contents).
Proof.
  intros contents selected Hcanonical. revert selected.
  induction Hcanonical as
      [|datum count tail Hdatum Hcount Hafter Htail IH];
    intros selected Hselected; simpl.
  - constructor; [exact Hselected | apply one_is_valid_multiplicity |
      constructor | constructor].
  - destruct (path_eq_dec selected datum) as [Heq | Hneq].
    + subst. constructor; [exact Hdatum | apply one_is_valid_multiplicity |
        exact Hafter | exact Htail].
    + destruct (path_ltb selected datum) eqn:Hbefore.
      * assert (Hselected_before_datum : path_lex_lt selected datum).
        { now apply path_ltb_spec. }
        constructor.
        -- exact Hselected.
        -- apply one_is_valid_multiplicity.
        -- constructor.
           ++ exact Hselected_before_datum.
           ++ eapply Forall_impl; [|exact Hafter].
              intros entry Hdatum_before.
              eapply path_lex_lt_transitive;
                [exact Hselected_before_datum | exact Hdatum_before].
        -- constructor; assumption.
      * constructor; try assumption.
        -- apply upsert_index_datum_preserves_key_forall.
           ++ apply TwoLevelRSpaceOperations.strict_order_from_distinct_not_less;
                assumption.
           ++ exact Hafter.
        -- now apply IH.
Qed.

Definition insert_index_key
    (index : TwoLevelAuthority) (key : LogicalKey) : TwoLevelAuthority :=
  let channel := fst key in
  let datum := snd key in
  let contents := channel_contents_or_empty index channel in
  upsert_channel_exact channel (upsert_index_datum datum contents) index.

Fixpoint build_key_index
    (index : TwoLevelAuthority) (events : list ChangeEvent)
    : TwoLevelAuthority :=
  match events with
  | [] => index
  | event :: tail =>
      build_key_index
        (insert_index_key index (change_event_key event)) tail
  end.

Definition event_key_valid (event : ChangeEvent) : Prop :=
  path_valid (fst (change_event_key event)) = true /\
  path_valid (snd (change_event_key event)) = true.

Lemma canonical_index_lookup_or_empty : forall index channel,
  canonical_channels index ->
  canonical_entries (channel_contents_or_empty index channel).
Proof.
  intros index channel Hcanonical.
  unfold channel_contents_or_empty.
  destruct (lookup_channel channel index) as [contents|] eqn:Hlookup.
  - now apply canonical_lookup_returns_canonical_bag with index channel.
  - constructor.
Qed.

Lemma insert_index_key_preserves_canonicality : forall index key,
  canonical_channels index ->
  path_valid (fst key) = true ->
  path_valid (snd key) = true ->
  canonical_channels (insert_index_key index key).
Proof.
  intros index [channel datum] Hcanonical Hchannel Hdatum.
  unfold insert_index_key. simpl.
  apply canonical_upsert_is_preserved; try assumption.
  apply canonical_upsert_index_datum; [|exact Hdatum].
  now apply canonical_index_lookup_or_empty.
Qed.

Theorem build_key_index_is_canonical : forall index events,
  canonical_channels index ->
  Forall event_key_valid events ->
  canonical_channels (build_key_index index events).
Proof.
  intros index events. revert index.
  induction events as [|event tail IH]; intros index Hcanonical Hvalid;
    inversion Hvalid; subst; simpl; [exact Hcanonical |].
  apply IH; [|assumption].
  destruct H1. now apply insert_index_key_preserves_canonicality.
Qed.

Lemma insert_index_key_target_is_exact : forall index channel datum,
  lookup_multiplicity (insert_index_key index (channel, datum)) channel datum = 1%N.
Proof.
  intros index channel datum.
  unfold insert_index_key, lookup_multiplicity. simpl.
  rewrite upsert_target_lookup_is_exact.
  apply upsert_index_datum_target_is_one.
Qed.

Lemma insert_index_key_preserves_other_logical_key : forall
    index inserted_channel inserted_datum channel datum,
  (channel, datum) <> (inserted_channel, inserted_datum) ->
  lookup_multiplicity
    (insert_index_key index (inserted_channel, inserted_datum)) channel datum =
  lookup_multiplicity index channel datum.
Proof.
  intros index inserted_channel inserted_datum channel datum Hneq.
  unfold insert_index_key, lookup_multiplicity. simpl.
  destruct (path_eq_dec channel inserted_channel) as [Hchannel | Hchannel].
  - subst channel. rewrite upsert_target_lookup_is_exact.
    unfold channel_contents_or_empty.
    destruct (lookup_channel inserted_channel index) as [contents|] eqn:Hlookup.
    + simpl. apply upsert_index_datum_preserves_other. congruence.
    + simpl. destruct (path_eq_dec datum inserted_datum);
        [congruence | reflexivity].
  - rewrite upsert_preserves_every_other_exact_channel by exact Hchannel.
    reflexivity.
Qed.

Lemma build_key_index_preserves_recorded_key : forall events index key,
  (0 < lookup_multiplicity index (fst key) (snd key))%N ->
  (0 < lookup_multiplicity (build_key_index index events)
    (fst key) (snd key))%N.
Proof.
  intros events. induction events as [|event tail IH];
    intros index [channel datum] Hpresent; simpl; [exact Hpresent |].
  apply (IH (insert_index_key index (change_event_key event))
    (channel, datum)).
  destruct (logical_key_eq_dec (channel, datum) (change_event_key event))
    as [Heq | Hneq].
  - destruct (change_event_key event) as [inserted_channel inserted_datum].
    inversion Heq; subst. rewrite insert_index_key_target_is_exact. lia.
  - destruct (change_event_key event) as [inserted_channel inserted_datum].
    rewrite insert_index_key_preserves_other_logical_key; assumption.
Qed.

Lemma build_key_index_covers_event_from : forall events event index,
  In event events ->
  (0 < lookup_multiplicity
    (build_key_index index events)
    (fst (change_event_key event)) (snd (change_event_key event)))%N.
Proof.
  intros events. induction events as [|head tail IH];
    intros event index Hin; [contradiction |].
  simpl. destruct Hin as [Heq | Hin].
  - subst event. apply build_key_index_preserves_recorded_key.
    destruct (change_event_key head) as [channel datum]. simpl.
    rewrite insert_index_key_target_is_exact. lia.
  - now apply IH with (index := insert_index_key index (change_event_key head)).
Qed.

Theorem build_key_index_covers_every_event : forall events event,
  In event events ->
  (0 < lookup_multiplicity
    (build_key_index [] events)
    (fst (change_event_key event)) (snd (change_event_key event)))%N.
Proof.
  intros. now apply build_key_index_covers_event_from.
Qed.

Lemma exact_match_change_events_are_valid : forall selection,
  Forall candidate_paths_valid (exact_selection_candidates selection) ->
  (match exact_selection_arrival selection with
   | None => True
   | Some arrival => candidate_paths_valid arrival
   end) ->
  Forall event_key_valid (exact_match_change_events selection).
Proof.
  intros selection Hselected Harrival.
  unfold exact_match_change_events.
  apply Forall_app. split.
  - induction Hselected as [|candidate tail Hcandidate Htail IH]; simpl.
    + constructor.
    + destruct (candidate_origin candidate); [exact IH |].
      destruct (candidate_persistent candidate); [exact IH |].
      constructor; [destruct Hcandidate as [Hchannel [Hdatum _]];
        split; assumption | exact IH].
  - destruct (exact_selection_arrival selection) as [arrival|] eqn:Harrival_eq;
      simpl; [|constructor].
    unfold candidate_selectedb.
    destruct (in_dec match_candidate_eq_dec arrival
      (exact_selection_candidates selection)); [constructor |].
    constructor; [destruct Harrival as [Hchannel [Hdatum _]];
      split; assumption | constructor].
Qed.

(** Canonical cursor emission from the internally built index ------------ *)

Definition active_cellb (cell : Z) : bool := negb (Z.eqb cell 0%Z).
Definition SignedDatumChange : Type := (Path * Z)%type.
Definition PreparedChannelChanges : Type := (Path * list SignedDatumChange)%type.

Definition active_index_entries
    (accumulator : SignedAccumulator) (channel : Path) (contents : ChannelBag)
    : list Entry :=
  filter
    (fun entry => active_cellb (accumulator (channel, entry_path entry)))
    contents.

Definition emit_index_datum_changes
    (accumulator : SignedAccumulator) (channel : Path) (contents : ChannelBag)
    : list SignedDatumChange :=
  map
    (fun entry => (entry_path entry, accumulator (channel, entry_path entry)))
    (active_index_entries accumulator channel contents).

Definition index_channel_activeb
    (accumulator : SignedAccumulator) (entry : ChannelEntry) : bool :=
  match active_index_entries accumulator (channel_path entry) (channel_bag entry) with
  | [] => false
  | _ :: _ => true
  end.

Definition active_index_channels
    (accumulator : SignedAccumulator) (index : TwoLevelAuthority)
    : TwoLevelAuthority :=
  filter (index_channel_activeb accumulator) index.

Definition emit_grouped_changes
    (accumulator : SignedAccumulator) (index : TwoLevelAuthority)
    : list PreparedChannelChanges :=
  map
    (fun entry =>
      (channel_path entry,
        emit_index_datum_changes accumulator
          (channel_path entry) (channel_bag entry)))
    (active_index_channels accumulator index).

Lemma StronglySorted_filter : forall (A : Type) (relation : A -> A -> Prop)
    (predicate : A -> bool) values,
  StronglySorted relation values ->
  StronglySorted relation (filter predicate values).
Proof.
  intros A relation predicate values Hsorted.
  induction Hsorted; simpl.
  - constructor.
  - destruct (predicate a).
    + constructor.
      * exact IHHsorted.
      * apply Forall_forall. intros item Hin.
        apply filter_In in Hin as [Hin _].
        now apply Forall_forall with (x := item) in H.
    + exact IHHsorted.
Qed.

Lemma canonical_entries_have_strong_path_order : forall entries,
  canonical_entries entries ->
  StronglySorted path_lex_lt (map entry_path entries).
Proof.
  intros entries Hcanonical. induction Hcanonical; simpl; constructor.
  - exact IHHcanonical.
  - apply Forall_forall. intros selected_path Hin.
    apply in_map_iff in Hin. destruct Hin as [entry [Heq Hin]]. subst selected_path.
    now apply Forall_forall with (x := entry) in H1.
Qed.

Lemma canonical_channels_have_strong_path_order : forall index,
  canonical_channels index ->
  StronglySorted path_lex_lt (map channel_path index).
Proof.
  intros index Hcanonical. induction Hcanonical; simpl; constructor.
  - exact IHHcanonical.
  - apply Forall_forall. intros selected_path Hin.
    apply in_map_iff in Hin. destruct Hin as [entry [Heq Hin]]. subst selected_path.
    now apply Forall_forall with (x := entry) in H1.
Qed.

Lemma canonical_channel_member_has_canonical_bag : forall
    index channel contents,
  canonical_channels index ->
  In (channel, contents) index ->
  canonical_entries contents.
Proof.
  intros index channel contents Hcanonical Hin.
  induction Hcanonical as
      [|head_channel head_contents tail Hvalid Hhead Hafter Htail IH];
    [contradiction |].
  destruct Hin as [Heq | Hin].
  - inversion Heq; subst. exact Hhead.
  - now apply IH.
Qed.

Lemma emitted_index_datum_changes_retain_canonical_order : forall
    accumulator channel contents,
  canonical_entries contents ->
  StronglySorted path_lex_lt
    (map fst (emit_index_datum_changes accumulator channel contents)).
Proof.
  intros accumulator channel contents Hcanonical.
  induction Hcanonical as
      [|datum count tail Hvalid Hcount Hafter Htail IH]; simpl;
    [constructor |].
  unfold emit_index_datum_changes, active_index_entries in *. simpl in *.
  destruct (active_cellb (accumulator (channel, datum))).
  - simpl. constructor.
    + exact IH.
    + apply Forall_forall. intros selected_path Hin.
      rewrite map_map in Hin. simpl in Hin.
      apply in_map_iff in Hin.
      destruct Hin as [entry [Heq Hin]]. subst selected_path.
      apply filter_In in Hin as [Hin _].
      now apply Forall_forall with (x := entry) in Hafter.
  - exact IH.
Qed.

Lemma emitted_group_channels_retain_canonical_order : forall accumulator index,
  canonical_channels index ->
  StronglySorted path_lex_lt
    (map fst (emit_grouped_changes accumulator index)).
Proof.
  intros accumulator index Hcanonical.
  induction Hcanonical as
      [|channel contents tail Hvalid Hcontents Hafter Htail IH]; simpl;
    [constructor |].
  unfold emit_grouped_changes, active_index_channels in *. simpl in *.
  destruct (index_channel_activeb accumulator (channel, contents)).
  - simpl. constructor.
    + exact IH.
    + apply Forall_forall. intros selected_path Hin.
      rewrite map_map in Hin. simpl in Hin.
      apply in_map_iff in Hin.
      destruct Hin as [entry [Heq Hin]]. subst selected_path.
      apply filter_In in Hin as [Hin _].
      now apply Forall_forall with (x := entry) in Hafter.
  - exact IH.
Qed.

(** Executable preparation --------------------------------------------- *)

Record MatchPreparationInput
    (observed commit : TwoLevelAuthority) : Type := {
  preparation_live_selection : LiveExactSelection observed commit
}.

Record PreparedMatchChanges : Type := PreparedMatchChangesState {
  prepared_signed_accumulator : SignedAccumulator;
  prepared_key_index : TwoLevelAuthority;
  prepared_grouped_changes : list PreparedChannelChanges
}.

Definition prepare_match_changes {observed commit}
    (input : MatchPreparationInput observed commit)
    : option PreparedMatchChanges :=
  let selection := live_exact_selection (preparation_live_selection input) in
  let events := exact_match_change_events selection in
  match record_changes empty_signed_accumulator events with
  | None => None
  | Some accumulator =>
      let index := build_key_index [] events in
      Some (PreparedMatchChangesState accumulator index
        (emit_grouped_changes accumulator index))
  end.

Theorem prepare_match_changes_is_sound : forall observed commit
    (input : MatchPreparationInput observed commit) prepared,
  prepare_match_changes input = Some prepared ->
  let selection := live_exact_selection (preparation_live_selection input) in
  let events := exact_match_change_events selection in
  record_changes empty_signed_accumulator events =
      Some (prepared_signed_accumulator prepared) /\
  prepared_key_index prepared = build_key_index [] events /\
  prepared_grouped_changes prepared =
      emit_grouped_changes (prepared_signed_accumulator prepared)
        (prepared_key_index prepared) /\
  accumulator_bounded (prepared_signed_accumulator prepared) /\
  canonical_channels (prepared_key_index prepared) /\
  forall event, In event events ->
    (0 < lookup_multiplicity (prepared_key_index prepared)
      (fst (change_event_key event)) (snd (change_event_key event)))%N.
Proof.
  intros observed commit input prepared Hprepare.
  unfold prepare_match_changes in Hprepare.
  destruct (record_changes empty_signed_accumulator
    (exact_match_change_events
      (live_exact_selection (preparation_live_selection input))))
    as [accumulator|] eqn:Hrecord; try discriminate.
  inversion Hprepare; subst prepared; clear Hprepare. simpl. repeat split.
  - exact Hrecord.
  - eapply successful_record_changes_are_bounded; eauto.
    apply empty_signed_accumulator_is_bounded.
  - apply build_key_index_is_canonical.
    + constructor.
    + apply exact_match_change_events_are_valid.
      * exact (exact_selection_candidate_paths_are_valid
          (live_exact_selection (preparation_live_selection input))).
      * destruct (exact_selection_arrival
          (live_exact_selection (preparation_live_selection input)))
          as [arrival|] eqn:Harrival; [|exact I].
        pose proof (exact_selection_arrival_is_fresh
          (live_exact_selection (preparation_live_selection input))) as Hfresh.
        rewrite Harrival in Hfresh. tauto.
  - intros event Hin. now apply build_key_index_covers_every_event.
Qed.

Theorem prepare_match_changes_is_complete : forall observed commit
    (input : MatchPreparationInput observed commit) accumulator,
  record_changes empty_signed_accumulator
      (exact_match_change_events
        (live_exact_selection (preparation_live_selection input))) =
    Some accumulator ->
  prepare_match_changes input =
    let events := exact_match_change_events
      (live_exact_selection (preparation_live_selection input)) in
    let index := build_key_index [] events in
    Some (PreparedMatchChangesState accumulator index
      (emit_grouped_changes accumulator index)).
Proof.
  intros observed commit input accumulator Hrecord.
  unfold prepare_match_changes. now rewrite Hrecord.
Qed.

Theorem prefix_bounded_match_changes_are_constructively_preparable : forall
    observed commit (input : MatchPreparationInput observed commit),
  PrefixBounded empty_signed_accumulator
    (exact_match_change_events
      (live_exact_selection (preparation_live_selection input))) ->
  exists prepared, prepare_match_changes input = Some prepared.
Proof.
  intros observed commit input Hbounded.
  destruct (prefix_bounded_changes_construct_an_accumulator Hbounded)
    as [accumulator Hrecord].
  eexists. eapply prepare_match_changes_is_complete. exact Hrecord.
Qed.

Theorem exact_live_match_changes_are_constructively_preparable : forall
    observed commit (input : MatchPreparationInput observed commit),
  exists prepared, prepare_match_changes input = Some prepared.
Proof.
  intros observed commit input.
  apply prefix_bounded_match_changes_are_constructively_preparable.
  exact (@exact_live_match_changes_are_automatically_prefix_bounded
    observed commit (preparation_live_selection input)).
Qed.

Theorem prepared_accumulator_exactly_cancels_arrival_against_removals : forall
    observed commit (input : MatchPreparationInput observed commit) prepared key,
  prepare_match_changes input = Some prepared ->
  prepared_signed_accumulator prepared key =
    event_net
      (exact_match_change_events
        (live_exact_selection (preparation_live_selection input))) key.
Proof.
  intros observed commit input prepared key Hprepare.
  pose proof (@prepare_match_changes_is_sound observed commit
    input prepared Hprepare)
    as Hsound.
  simpl in Hsound. destruct Hsound as [Hrecord _].
  now apply successful_preparation_accumulator_is_exact with
    (events := exact_match_change_events
      (live_exact_selection (preparation_live_selection input))).
Qed.

Theorem selected_arrival_is_not_added_to_prepared_delta : forall
    observed commit (input : MatchPreparationInput observed commit)
    prepared arrival,
  prepare_match_changes input = Some prepared ->
  exact_selection_arrival
    (live_exact_selection (preparation_live_selection input)) = Some arrival ->
  In arrival
    (exact_selection_candidates
      (live_exact_selection (preparation_live_selection input))) ->
  prepared_signed_accumulator prepared (candidate_key arrival) =
  (- Z.of_N (nonpersistent_stored_demand
    (candidate_channel arrival) (candidate_datum arrival)
    (exact_selection_candidates
      (live_exact_selection (preparation_live_selection input)))))%Z.
Proof.
  intros observed commit input prepared arrival Hprepare Harrival Hin.
  rewrite (@prepared_accumulator_exactly_cancels_arrival_against_removals
    observed commit input prepared (candidate_key arrival) Hprepare).
  unfold candidate_key.
  rewrite exact_match_event_net_is_demand_plus_optional_arrival.
  rewrite (@selected_fresh_arrival_emits_no_addition
    (live_exact_selection (preparation_live_selection input)) arrival
    Harrival Hin). simpl. lia.
Qed.

Theorem unselected_arrival_exactly_cancels_one_removal : forall
    observed commit (input : MatchPreparationInput observed commit)
    prepared arrival,
  prepare_match_changes input = Some prepared ->
  exact_selection_arrival
    (live_exact_selection (preparation_live_selection input)) = Some arrival ->
  ~ In arrival
    (exact_selection_candidates
      (live_exact_selection (preparation_live_selection input))) ->
  prepared_signed_accumulator prepared (candidate_key arrival) =
  (- Z.of_N (nonpersistent_stored_demand
    (candidate_channel arrival) (candidate_datum arrival)
    (exact_selection_candidates
      (live_exact_selection (preparation_live_selection input)))) + 1)%Z.
Proof.
  intros observed commit input prepared arrival Hprepare Harrival Hnotin.
  rewrite (@prepared_accumulator_exactly_cancels_arrival_against_removals
    observed commit input prepared (candidate_key arrival) Hprepare).
  unfold candidate_key.
  rewrite exact_match_event_net_is_demand_plus_optional_arrival.
  rewrite (@unselected_fresh_arrival_emits_one_exact_addition
    (live_exact_selection (preparation_live_selection input)) arrival
    Harrival Hnotin).
  rewrite event_net_singleton_at_own_key. simpl.
  reflexivity.
Qed.

Theorem prepared_output_preserves_canonical_nested_cursor_order : forall
    observed commit (input : MatchPreparationInput observed commit) prepared,
  prepare_match_changes input = Some prepared ->
  StronglySorted path_lex_lt (map fst (prepared_grouped_changes prepared)) /\
  Forall
    (fun group => StronglySorted path_lex_lt (map fst (snd group)))
    (prepared_grouped_changes prepared).
Proof.
  intros observed commit input prepared Hprepare.
  pose proof (@prepare_match_changes_is_sound observed commit
    input prepared Hprepare)
    as Hsound.
  simpl in Hsound.
  destruct Hsound as [_ [_ [Houtput [_ [Hcanonical _]]]]].
  rewrite Houtput. split.
  - now apply emitted_group_channels_retain_canonical_order.
  - unfold emit_grouped_changes. apply Forall_forall.
    intros [channel changes] Hin. apply in_map_iff in Hin.
    destruct Hin as [[selected_channel contents] [Heq Hin]].
    inversion Heq; subst; clear Heq.
    apply emitted_index_datum_changes_retain_canonical_order.
    eapply canonical_channel_member_has_canonical_bag; [exact Hcanonical |].
    unfold active_index_channels in Hin.
    apply filter_In in Hin as [Hin_index Hactive]. exact Hin_index.
Qed.

(** Exact bridge to the already-proved transactional delta layer.  A bridge
    cannot be constructed from a guessed list: each member is a
    [PreparedChannelDelta], whose type contains canonical-operation validity
    and successful execution evidence. *)
Definition signed_group_channel (group : PreparedChannelChanges) : Path := fst group.

Definition signed_group_additions (group : PreparedChannelChanges) : list Entry :=
  fold_right
    (fun change tail =>
      if Z.ltb 0 (snd change)
      then (fst change, Z.to_N (snd change)) :: tail
      else tail)
    [] (snd group).

Definition signed_group_removals (group : PreparedChannelChanges) : list Entry :=
  fold_right
    (fun change tail =>
      if Z.ltb (snd change) 0
      then (fst change, Z.to_N (- snd change)) :: tail
      else tail)
    [] (snd group).

Definition signed_group_bag_delta (group : PreparedChannelChanges) : BagDelta :=
  {| bag_delta_removal_mode := StrictSubtract;
     bag_delta_subtractions := signed_group_removals group;
     bag_delta_deletions := [];
     bag_delta_additions := signed_group_additions group |}.

Record ExactPreparedRSpaceBridge
    (before : TwoLevelAuthority) (changes : PreparedMatchChanges) : Type := {
  exact_bridge_delta : PreparedRSpaceDelta before;
  exact_bridge_every_delta_is_emitted :
    forall delta,
      In delta (prepared_channel_deltas exact_bridge_delta) ->
      exists group,
        In group (prepared_grouped_changes changes) /\
        prepared_delta_channel delta = signed_group_channel group /\
        prepared_delta_change delta = signed_group_bag_delta group;
  exact_bridge_every_emitted_group_is_executable :
    forall group,
      In group (prepared_grouped_changes changes) ->
      exists delta,
        In delta (prepared_channel_deltas exact_bridge_delta) /\
        prepared_delta_channel delta = signed_group_channel group /\
        prepared_delta_change delta = signed_group_bag_delta group
}.

Theorem exact_bridge_carries_successful_execution_evidence : forall
    before changes (bridge : ExactPreparedRSpaceBridge before changes) group,
  In group (prepared_grouped_changes changes) ->
  exists delta,
    In delta (prepared_channel_deltas (exact_bridge_delta bridge)) /\
    execute_logical_operation
      (channel_contents_or_empty before (signed_group_channel group))
      (OpBagDelta (signed_group_bag_delta group)) =
    Some (prepared_delta_output delta).
Proof.
  intros before changes bridge group Hin.
  destruct (exact_bridge_every_emitted_group_is_executable bridge group Hin)
    as [delta [Hdelta [Hchannel Hchange]]].
  exists delta. split; [exact Hdelta|].
  rewrite <- Hchannel, <- Hchange.
  exact (prepared_delta_execution_succeeds delta).
Qed.

(** The bridge checker receives only static validity facts.  It computes each
    [execute_logical_operation]; a failed subtraction or overflow returns
    [None], so successful execution is never assumed by the caller. *)
Record SignedGroupStaticEvidence
    (before : TwoLevelAuthority) (group : PreparedChannelChanges) : Prop :=
  SignedGroupStaticEvidenceState {
    signed_group_channel_valid :
      path_valid (signed_group_channel group) = true;
    signed_group_absent_requires_addition :
      loaded before (signed_group_channel group) \/
      bag_delta_additions (signed_group_bag_delta group) <> [];
    signed_group_operation_is_valid :
      valid_logical_operation
        (channel_contents_or_empty before (signed_group_channel group))
        (OpBagDelta (signed_group_bag_delta group))
  }.

Definition canonical_entries_dec : forall entries,
    {canonical_entries entries} + {~ canonical_entries entries}.
Proof.
  intro entries. induction entries as [|[path multiplicity] tail IH].
  - left. constructor.
  - destruct (Bool.bool_dec (path_valid path) true)
      as [Hpath | Hpath].
    + destruct (Bool.bool_dec (multiplicity_valid multiplicity) true)
        as [Hmultiplicity | Hmultiplicity].
      * destruct (Forall_dec (fun entry => path_lex_lt path (entry_path entry))
          (fun entry =>
            match Bool.bool_dec (path_ltb path (entry_path entry)) true with
            | left Hlt => left (proj1 (path_ltb_spec _ _) Hlt)
            | right Hnot =>
                right (fun Hlt => Hnot (proj2 (path_ltb_spec _ _) Hlt))
            end) tail) as [Hafter | Hafter].
        -- destruct IH as [Htail | Htail].
           ++ left. now constructor.
           ++ right. intro Hcanonical. inversion Hcanonical. contradiction.
        -- right. intro Hcanonical. inversion Hcanonical. contradiction.
      * right. intro Hcanonical. inversion Hcanonical. contradiction.
    + right. intro Hcanonical. inversion Hcanonical. contradiction.
Defined.

Fixpoint path_nodup_dec (paths : list Path) :
    {NoDup paths} + {~ NoDup paths}.
Proof.
  destruct paths as [|path tail].
  - left. constructor.
  - destruct (in_dec path_eq_dec path tail) as [Hin | Hnotin].
    + right. intro Hnodup. inversion Hnodup. contradiction.
    + destruct (path_nodup_dec tail) as [Htail | Htail].
      * left. now constructor.
      * right. intro Hnodup. inversion Hnodup. contradiction.
Defined.

Lemma signed_group_static_parts_are_canonical : forall before group,
  SignedGroupStaticEvidence before group ->
  canonical_entries (signed_group_removals group) /\
  canonical_entries (signed_group_additions group).
Proof.
  intros before group Hstatic.
  pose proof (signed_group_operation_is_valid Hstatic) as Hvalid.
  unfold signed_group_bag_delta in Hvalid.
  inversion Hvalid; subst. now split.
Qed.

Definition signed_group_static_evidence_dec : forall before
    (before_canonical : canonical_channels before) group,
  {SignedGroupStaticEvidence before group} +
  {~ SignedGroupStaticEvidence before group}.
Proof.
  intros before before_canonical group.
  destruct (Bool.bool_dec (path_valid (signed_group_channel group)) true)
    as [Hchannel | Hchannel].
  2: { right. intro Hstatic.
       apply Hchannel. exact (signed_group_channel_valid Hstatic). }
  destruct (canonical_entries_dec (signed_group_removals group))
    as [Hremovals | Hremovals].
  2: { right. intro Hstatic. apply Hremovals.
       exact (proj1 (signed_group_static_parts_are_canonical Hstatic)). }
  destruct (canonical_entries_dec (signed_group_additions group))
    as [Hadditions | Hadditions].
  2: { right. intro Hstatic. apply Hadditions.
       exact (proj2 (signed_group_static_parts_are_canonical Hstatic)). }
  destruct (lookup_channel (signed_group_channel group) before)
    as [contents|] eqn:Hlookup.
  - left. refine (SignedGroupStaticEvidenceState Hchannel _ _).
    + left. now exists contents.
    + unfold signed_group_bag_delta. apply ValidStrictBagDelta.
      * unfold channel_contents_or_empty. rewrite Hlookup.
        now apply canonical_lookup_channel_contents with before
          (signed_group_channel group).
      * exact Hremovals.
      * exact Hadditions.
  - destruct (signed_group_additions group) as [|addition additions]
      eqn:Haddition.
    + right. intro Hstatic.
      destruct (signed_group_absent_requires_addition Hstatic)
        as [[loaded_contents Hloaded] | Hnonempty].
      * rewrite Hlookup in Hloaded. discriminate.
      * unfold signed_group_bag_delta in Hnonempty. simpl in Hnonempty.
        rewrite Haddition in Hnonempty. contradiction.
    + left. refine (SignedGroupStaticEvidenceState Hchannel _ _).
      * right. unfold signed_group_bag_delta. simpl.
        rewrite Haddition. discriminate.
      * unfold signed_group_bag_delta. apply ValidStrictBagDelta.
        -- unfold channel_contents_or_empty. rewrite Hlookup. constructor.
        -- exact Hremovals.
        -- rewrite Haddition. exact Hadditions.
Defined.

Definition check_signed_group_static_evidence {before}
    (before_canonical : canonical_channels before)
    (group : PreparedChannelChanges)
    : option (SignedGroupStaticEvidence before group) :=
  match signed_group_static_evidence_dec before_canonical group with
  | left static => Some static
  | right _ => None
  end.

Lemma check_signed_group_static_evidence_is_complete : forall before
    (before_canonical : canonical_channels before) group,
  SignedGroupStaticEvidence before group ->
  exists checked,
    check_signed_group_static_evidence before_canonical group = Some checked.
Proof.
  intros before before_canonical group Hstatic.
  unfold check_signed_group_static_evidence.
  destruct (signed_group_static_evidence_dec before_canonical group)
    as [checked | Himpossible].
  - now exists checked.
  - contradiction.
Qed.

Record CheckedGroupDelta
    (before : TwoLevelAuthority) (group : PreparedChannelChanges) : Type :=
  CheckedGroupDeltaState {
    checked_group_delta : PreparedChannelDelta before;
    checked_group_channel_is_exact :
      prepared_delta_channel checked_group_delta = signed_group_channel group;
    checked_group_change_is_exact :
      prepared_delta_change checked_group_delta = signed_group_bag_delta group
  }.

Definition decide_signed_group_execution (before : TwoLevelAuthority)
    (group : PreparedChannelChanges) :
    { output : ChannelBag |
      execute_logical_operation
        (channel_contents_or_empty before (signed_group_channel group))
        (OpBagDelta (signed_group_bag_delta group)) = Some output } +
    { execute_logical_operation
        (channel_contents_or_empty before (signed_group_channel group))
        (OpBagDelta (signed_group_bag_delta group)) = None }.
Proof.
  destruct (execute_logical_operation
    (channel_contents_or_empty before (signed_group_channel group))
    (OpBagDelta (signed_group_bag_delta group))) as [output|] eqn:Hexecution.
  - left. now exists output.
  - right. reflexivity.
Defined.

Definition check_signed_group_execution {before group}
    (static : SignedGroupStaticEvidence before group)
    : option (CheckedGroupDelta before group).
Proof.
  destruct (decide_signed_group_execution before group)
    as [[output Hexecution]|Hexecution].
  - refine (Some (CheckedGroupDeltaState
      {| prepared_delta_channel := signed_group_channel group;
         prepared_delta_change := signed_group_bag_delta group;
         prepared_delta_output := output;
         prepared_delta_channel_valid := signed_group_channel_valid static;
         prepared_delta_absent_channel_has_addition :=
           signed_group_absent_requires_addition static;
         prepared_delta_operation_valid := signed_group_operation_is_valid static;
         prepared_delta_execution_succeeds := Hexecution |}
      eq_refl eq_refl)).
  - exact None.
Defined.

Inductive CheckedDeltaVector (before : TwoLevelAuthority)
    : list PreparedChannelChanges -> Type :=
| CheckedDeltaVectorNil : CheckedDeltaVector before []
| CheckedDeltaVectorCons : forall group tail,
    CheckedGroupDelta before group ->
    CheckedDeltaVector before tail ->
    CheckedDeltaVector before (group :: tail).

Fixpoint checked_vector_deltas {before groups}
    (checked : CheckedDeltaVector before groups)
    : list (PreparedChannelDelta before) :=
  match checked with
  | CheckedDeltaVectorNil _ => []
  | CheckedDeltaVectorCons head tail =>
      checked_group_delta head :: checked_vector_deltas tail
  end.

Fixpoint check_delta_vector (before : TwoLevelAuthority)
    (before_canonical : canonical_channels before)
    (groups : list PreparedChannelChanges)
    : option (CheckedDeltaVector before groups) :=
  match groups as current return option (CheckedDeltaVector before current) with
  | [] => Some (CheckedDeltaVectorNil before)
  | head :: tail =>
      match @check_signed_group_static_evidence before before_canonical head with
      | None => None
      | Some static =>
          match check_signed_group_execution static with
          | None => None
          | Some checked_head =>
              match @check_delta_vector before before_canonical tail with
              | None => None
              | Some checked_tail =>
                  Some (@CheckedDeltaVectorCons before head tail
                    checked_head checked_tail)
              end
          end
      end
  end.

Lemma checked_vector_channels_are_exact : forall before groups
    (checked : CheckedDeltaVector before groups),
  map (fun delta => prepared_delta_channel delta) (checked_vector_deltas checked) =
  map signed_group_channel groups.
Proof.
  intros before groups checked. induction checked; simpl; [reflexivity|].
  rewrite checked_group_channel_is_exact, IHchecked. reflexivity.
Qed.

Lemma checked_vector_delta_member_is_exact : forall before groups
    (checked : CheckedDeltaVector before groups) delta,
  In delta (checked_vector_deltas checked) ->
  exists group,
    In group groups /\
    prepared_delta_channel delta = signed_group_channel group /\
    prepared_delta_change delta = signed_group_bag_delta group.
Proof.
  intros before groups checked. induction checked;
    intros delta Hin; simpl in Hin; [contradiction|].
  destruct Hin as [Heq | Hin].
  - subst delta. exists group. split; [now left|]. split.
    + apply checked_group_channel_is_exact.
    + apply checked_group_change_is_exact.
  - destruct (IHchecked delta Hin) as [selected [Hselected [Hchannel Hchange]]].
    exists selected. split; [now right|]. split; assumption.
Qed.

Lemma checked_vector_group_member_is_exact : forall before groups
    (checked : CheckedDeltaVector before groups) group,
  In group groups ->
  exists delta,
    In delta (checked_vector_deltas checked) /\
    prepared_delta_channel delta = signed_group_channel group /\
    prepared_delta_change delta = signed_group_bag_delta group.
Proof.
  intros before groups checked. induction checked;
    intros selected Hin; [contradiction|].
  destruct Hin as [Heq | Hin].
  - subst selected. exists (checked_group_delta c). split; [now left|]. split.
    + apply checked_group_channel_is_exact.
    + apply checked_group_change_is_exact.
  - destruct (IHchecked selected Hin) as [delta [Hdelta [Hchannel Hchange]]].
    exists delta. split; [now right|]. split; assumption.
Qed.

Definition checked_vector_as_prepared_rspace_delta {before groups}
    (checked : CheckedDeltaVector before groups)
    (channels_unique : NoDup (map signed_group_channel groups))
    : PreparedRSpaceDelta before.
Proof.
  refine {| prepared_channel_deltas := checked_vector_deltas checked |}.
  unfold prepared_delta_channels.
  rewrite checked_vector_channels_are_exact. exact channels_unique.
Defined.

Definition checked_vector_as_exact_bridge {before changes}
    (checked : CheckedDeltaVector before (prepared_grouped_changes changes))
    (channels_unique :
      NoDup (map signed_group_channel (prepared_grouped_changes changes)))
    : ExactPreparedRSpaceBridge before changes.
Proof.
  refine {| exact_bridge_delta :=
      checked_vector_as_prepared_rspace_delta checked channels_unique |}.
  - intros delta Hin. simpl in Hin.
    now apply checked_vector_delta_member_is_exact with (checked := checked).
  - intros group Hin. simpl.
    now apply checked_vector_group_member_is_exact with (checked := checked).
Defined.

Definition check_exact_prepared_rspace_bridge {before}
    (before_canonical : canonical_channels before)
    (changes : PreparedMatchChanges)
    : option (ExactPreparedRSpaceBridge before changes) :=
  match path_nodup_dec
      (map signed_group_channel (prepared_grouped_changes changes)) with
  | left Hunique =>
      match @check_delta_vector before before_canonical
          (prepared_grouped_changes changes) with
      | None => None
      | Some checked =>
          Some (checked_vector_as_exact_bridge checked Hunique)
      end
  | right _ => None
  end.

Definition prepare_and_check_exact_rspace_delta {observed commit}
    (input : MatchPreparationInput observed commit)
    : option { changes : PreparedMatchChanges &
        ExactPreparedRSpaceBridge commit changes } :=
  match prepare_match_changes input with
  | None => None
  | Some changes =>
      match check_exact_prepared_rspace_bridge
          (@live_commit_is_canonical observed commit
            (preparation_live_selection input)) changes with
      | None => None
      | Some bridge => Some (existT _ changes bridge)
      end
  end.

Theorem successful_prepare_and_check_returns_exact_transaction_evidence :
    forall observed commit (input : MatchPreparationInput observed commit)
      package,
  prepare_and_check_exact_rspace_delta input = Some package ->
  exists changes (bridge : ExactPreparedRSpaceBridge commit changes),
    package = existT _ changes bridge /\
    prepare_match_changes input = Some changes /\
    check_exact_prepared_rspace_bridge
      (@live_commit_is_canonical observed commit
        (preparation_live_selection input)) changes = Some bridge.
Proof.
  intros observed commit input package Hchecked.
  unfold prepare_and_check_exact_rspace_delta in Hchecked.
  destruct (prepare_match_changes input) as [changes|] eqn:Hprepared;
    [|discriminate].
  destruct (check_exact_prepared_rspace_bridge
    (@live_commit_is_canonical observed commit
      (preparation_live_selection input)) changes)
    as [bridge|] eqn:Hbridge; [|discriminate].
  inversion Hchecked; subst package.
  exists changes, bridge. repeat split; assumption.
Qed.

Lemma check_signed_group_execution_success_iff : forall before group
    (static : SignedGroupStaticEvidence before group),
  (exists checked, check_signed_group_execution static = Some checked) <->
  exists output,
    execute_logical_operation
      (channel_contents_or_empty before (signed_group_channel group))
      (OpBagDelta (signed_group_bag_delta group)) = Some output.
Proof.
  intros before group static.
  unfold check_signed_group_execution.
  destruct (decide_signed_group_execution before group)
    as [[output Hexecution]|Hexecution]; simpl.
  - split.
    + intros _. exists output. exact Hexecution.
    + intros _. eexists. reflexivity.
  - split.
    + intros [witness H]. discriminate.
    + intros [output Houtput]. exfalso.
      unfold execute_logical_operation in Hexecution. congruence.
Qed.

Lemma check_delta_vector_is_complete : forall before
    (before_canonical : canonical_channels before) groups,
  (forall group, In group groups ->
    SignedGroupStaticEvidence before group) ->
  (forall group, In group groups ->
    exists output,
      execute_logical_operation
        (channel_contents_or_empty before (signed_group_channel group))
        (OpBagDelta (signed_group_bag_delta group)) = Some output) ->
  exists checked,
    @check_delta_vector before before_canonical groups = Some checked.
Proof.
  intros before before_canonical groups.
  induction groups as [|head tail IH]; intros Hstatic Hexecution.
  - exists (CheckedDeltaVectorNil before). reflexivity.
  - destruct (@check_signed_group_static_evidence_is_complete
      before before_canonical head (Hstatic head (or_introl eq_refl)))
      as [static_head Hstatic_head].
    destruct (proj2 (check_signed_group_execution_success_iff static_head)
      (Hexecution head (or_introl eq_refl)))
      as [checked_head Hchecked_head].
    destruct (IH
      (fun group Hin => Hstatic group (or_intror Hin))
      (fun group Hin => Hexecution group (or_intror Hin)))
      as [checked_tail Hchecked_tail].
    cbn [check_delta_vector].
    rewrite Hstatic_head, Hchecked_head, Hchecked_tail.
    eexists. reflexivity.
Qed.

Theorem check_exact_prepared_rspace_bridge_is_complete : forall before
    (before_canonical : canonical_channels before) changes,
  NoDup (map signed_group_channel (prepared_grouped_changes changes)) ->
  (forall group, In group (prepared_grouped_changes changes) ->
    SignedGroupStaticEvidence before group) ->
  (forall group, In group (prepared_grouped_changes changes) ->
    exists output,
      execute_logical_operation
        (channel_contents_or_empty before (signed_group_channel group))
        (OpBagDelta (signed_group_bag_delta group)) = Some output) ->
  exists bridge,
    check_exact_prepared_rspace_bridge before_canonical changes = Some bridge.
Proof.
  intros before before_canonical changes Hunique Hstatic Hexecution.
  destruct (path_nodup_dec
    (map signed_group_channel (prepared_grouped_changes changes)))
    as [checked_unique | Hduplicate]; [|contradiction].
  destruct (@check_delta_vector_is_complete before before_canonical
    (prepared_grouped_changes changes) Hstatic Hexecution)
    as [checked Hchecked].
  unfold check_exact_prepared_rspace_bridge.
  destruct (path_nodup_dec
    (map signed_group_channel (prepared_grouped_changes changes)))
    as [computed_unique | Himpossible]; [|contradiction].
  rewrite Hchecked. eexists. reflexivity.
Qed.

Print Assumptions exact_metadata_row_is_parser_and_digest_bound.
Print Assumptions malformed_datum_cannot_inhabit_exact_metadata.
Print Assumptions forced_digest_collision_uses_canonical_identity_order.
Print Assumptions candidate_catalog_projection_has_exact_total_ordinals.
Print Assumptions coherent_combined_pool_is_exact_fresh_then_stored_projection.
Print Assumptions requested_channel_has_observed_catalog.
Print Assumptions exact_selection_has_at_most_one_nonpersistent_fresh_reservation.
Print Assumptions selected_fresh_candidate_is_the_exact_arrival.
Print Assumptions live_selected_stored_candidate_has_exact_metadata_row.
Print Assumptions live_stored_persistent_selection_requires_presence.
Print Assumptions live_stored_nonpersistent_selection_requires_aggregate_demand.
Print Assumptions successful_record_changes_equal_exact_signed_fold.
Print Assumptions exact_live_match_changes_are_automatically_prefix_bounded.
Print Assumptions exact_live_match_changes_are_constructively_preparable.
Print Assumptions prepared_accumulator_exactly_cancels_arrival_against_removals.
Print Assumptions prepared_output_preserves_canonical_nested_cursor_order.
Print Assumptions check_signed_group_static_evidence_is_complete.
Print Assumptions check_exact_prepared_rspace_bridge_is_complete.
Print Assumptions successful_prepare_and_check_returns_exact_transaction_evidence.

End TwoLevelRSpaceMatchPreparation.
