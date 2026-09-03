From Stdlib Require Import
  Bool List Arith Lia NArith Sorting.Sorted.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority
  RhoPathV1Datum.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import RhoPathV1Datum.
Set Implicit Arguments.
Open Scope N_scope.

(**
  Executable PREPARE refinement for an accepted RSpace datum match.

  The production representation has two levels: a PathMap from an exact
  RhoPath channel marker to a channel-relative Bag, and a Bag from the
  relative datum key to its u64 multiplicity.  This development observes
  those cursors as lists solely for proof.  Candidate catalogs are the one
  deliberate production materialization: the PathMap cursor is collected in
  a Vec, decorated with Blake2b-256(identity), sorted by
  (digest, canonical identity), checked for duplicate identity, and retained
  behind Arc.  No such projection or sort is part of Bag serialization.

  Stored datum authority is always the complete absolute RhoPath.  Channel,
  relative datum, digest, and ordinal are decoded metadata or navigation aids;
  none can substitute for that absolute semantic identity.
*)
Module TwoLevelRSpaceMatchPreparation.

(** Exact candidate codec boundary ------------------------------------- *)

Record DecodedCanonicalDatum : Type := DecodedCanonicalDatumState {
  decoded_datum_structure : Path;
  decoded_datum_persistent : bool;
  decoded_datum_reencoded_identity : Path
}.

(** These are executable functions supplied by the Rust refinement.  The
    model never invents a surrogate byte codec. [codec_digest_raw_identity]
    receives exactly the unframed canonical identity bytes; there is no
    domain separator. *)
Record CanonicalDatumCodec : Type := CanonicalDatumCodecState {
  (** This one executable boundary performs production decode followed by
      production re-encode.  Returning the re-encoded bytes from the decoded
      value prevents the formal interface from being satisfied by an
      identity-only echo function masquerading as an encoder. *)
  codec_decode_and_reencode_identity : Path -> option DecodedCanonicalDatum;
  codec_digest_raw_identity : Path -> Path
}.

Inductive CandidateAddress : Type :=
| FreshAddress
| StoredAddress (absolute_path : Path).

Record MatchCandidate : Type := MatchCandidateState {
  candidate_address : CandidateAddress;
  candidate_channel_marker : Path;
  candidate_relative_datum : Path;
  candidate_datum_structure : Path;
  candidate_canonical_identity : Path;
  candidate_digest : Path;
  candidate_persistent : bool;
  candidate_observed_multiplicity : Multiplicity
}.

Definition candidate_storage_path (candidate : MatchCandidate) : Path :=
  candidate_channel_marker candidate ++ candidate_relative_datum candidate.

Definition candidate_address_path (candidate : MatchCandidate) : option Path :=
  match candidate_address candidate with
  | FreshAddress => None
  | StoredAddress path => Some path
  end.

Definition candidate_address_eq_dec : forall left right : CandidateAddress,
    {left = right} + {left <> right}.
Proof. decide equality; apply path_eq_dec. Defined.

Definition match_candidate_eq_dec : forall left right : MatchCandidate,
    {left = right} + {left <> right}.
Proof.
  decide equality; try apply N.eq_dec; try apply Bool.bool_dec;
    try apply path_eq_dec; try apply candidate_address_eq_dec.
Defined.

Definition match_candidate_eqb (left right : MatchCandidate) : bool :=
  if match_candidate_eq_dec left right then true else false.

Lemma match_candidate_eqb_spec : forall left right,
  match_candidate_eqb left right = true <-> left = right.
Proof.
  intros left right. unfold match_candidate_eqb.
  destruct (match_candidate_eq_dec left right) as [Heq|Hneq].
  - split; [intro; exact Heq|intro; reflexivity].
  - split; [discriminate|intro Heq; contradiction].
Qed.

Inductive PreparationRefusal : Type :=
| RefuseObservedAuthority
| RefuseCommitAuthority
| RefuseMalformedStoredPath (absolute_path : Path)
| RefuseNoncanonicalDatumIdentity (identity : Path)
| RefuseDatumStructureMismatch (identity : Path)
| RefuseDigestWidth (identity digest : Path)
| RefuseMultiplicity (absolute_path : Path)
| RefuseDuplicateDatumIdentity (channel identity : Path)
| RefuseCatalogOrder (channel : Path)
| RefuseMalformedFreshPath (absolute_path : Path)
| RefuseInvalidReplayExclusion (absolute_path : Path)
| RefuseSelectionShape
| RefuseObservedPoolFeasibility
| RefuseReservationOverflow (absolute_path : Path)
| RefuseCommitLiveness
| RefuseFlatDelta (absolute_path : Path)
| RefuseFlatDeltaInvariant
| RefuseFlatDeltaPartition (absolute_path : Path).

Inductive BuildResult (A : Type) : Type :=
| BuildOk (value : A)
| BuildRefused (reason : PreparationRefusal).

Arguments BuildOk {A} _.
Arguments BuildRefused {A} _.

Definition digest_width_is_exact (digest : Path) : bool :=
  Nat.eqb (length digest) 32 && path_valid digest.

Definition decoded_view_marker (view : DatumPathView) : Path :=
  datum_channel_marker
    (datum_view_channel_structure view)
    (datum_view_channel_identity view).

Definition decoded_view_relative (view : DatumPathView) : Path :=
  relative_datum_key
    (datum_view_datum_structure view)
    (datum_view_datum_identity view).

Definition construct_candidate_at
    (codec : CanonicalDatumCodec) (address : CandidateAddress)
    (absolute_path : Path) (multiplicity : Multiplicity)
    : BuildResult MatchCandidate :=
  match parse_absolute_datum absolute_path with
  | None => BuildRefused (RefuseMalformedStoredPath absolute_path)
  | Some view =>
      let identity := datum_view_datum_identity view in
      match codec_decode_and_reencode_identity codec identity with
      | None => BuildRefused (RefuseNoncanonicalDatumIdentity identity)
      | Some decoded =>
              if path_eqb (decoded_datum_reencoded_identity decoded) identity then
                if path_eqb (decoded_datum_structure decoded)
                    (datum_view_datum_structure view) then
                  let digest := codec_digest_raw_identity codec identity in
                  if digest_width_is_exact digest then
                    if multiplicity_valid multiplicity then
                      BuildOk (MatchCandidateState address
                        (decoded_view_marker view) (decoded_view_relative view)
                        (datum_view_datum_structure view) identity digest
                        (decoded_datum_persistent decoded) multiplicity)
                    else BuildRefused (RefuseMultiplicity absolute_path)
                  else BuildRefused (RefuseDigestWidth identity digest)
                else BuildRefused (RefuseDatumStructureMismatch identity)
              else BuildRefused (RefuseNoncanonicalDatumIdentity identity)
      end
  end.

Definition construct_stored_candidate
    (codec : CanonicalDatumCodec) (absolute_path : Path)
    (multiplicity : Multiplicity) : BuildResult MatchCandidate :=
  construct_candidate_at codec (StoredAddress absolute_path)
    absolute_path multiplicity.

Definition construct_fresh_candidate
    (codec : CanonicalDatumCodec) (absolute_path : Path)
    : BuildResult MatchCandidate :=
  match construct_candidate_at codec FreshAddress absolute_path 1 with
  | BuildOk candidate => BuildOk candidate
  | BuildRefused (RefuseMalformedStoredPath _) =>
      BuildRefused (RefuseMalformedFreshPath absolute_path)
  | BuildRefused reason => BuildRefused reason
  end.

Definition candidate_codec_exactb
    (codec : CanonicalDatumCodec) (candidate : MatchCandidate) : bool :=
  match parse_absolute_datum (candidate_storage_path candidate) with
  | None => false
  | Some view =>
      let identity := datum_view_datum_identity view in
      match codec_decode_and_reencode_identity codec identity with
      | Some decoded =>
          path_eqb (decoded_view_marker view)
              (candidate_channel_marker candidate) &&
          path_eqb (decoded_view_relative view)
              (candidate_relative_datum candidate) &&
          path_eqb (datum_view_datum_structure view)
              (candidate_datum_structure candidate) &&
          path_eqb identity (candidate_canonical_identity candidate) &&
          path_eqb (decoded_datum_reencoded_identity decoded) identity &&
          path_eqb (decoded_datum_structure decoded)
              (datum_view_datum_structure view) &&
          Bool.eqb (decoded_datum_persistent decoded)
              (candidate_persistent candidate) &&
          path_eqb (codec_digest_raw_identity codec identity)
              (candidate_digest candidate) &&
          digest_width_is_exact (candidate_digest candidate) &&
          multiplicity_valid (candidate_observed_multiplicity candidate)
      | None => false
      end
  end.

(** Rust must differentially bind this executable boundary to the real
    bincode canonical decoder/re-encoder, RhoPath structure encoder, and
    Blake2b256Hash::new(raw_identity).  The invariant deliberately mentions
    no alternate hash, domain tag, or formal surrogate. *)
Definition rust_codec_differential_invariant
    (codec : CanonicalDatumCodec) (absolute_paths : list Path) : Prop :=
  forall absolute multiplicity candidate,
    In absolute absolute_paths ->
    construct_stored_candidate codec absolute multiplicity = BuildOk candidate ->
    candidate_codec_exactb codec candidate = true /\
    length (candidate_digest candidate) = 32%nat /\
    candidate_digest candidate =
      codec_digest_raw_identity codec (candidate_canonical_identity candidate).

Lemma constructed_candidate_is_codec_exact : forall
    codec address absolute multiplicity candidate,
  construct_candidate_at codec address absolute multiplicity = BuildOk candidate ->
  candidate_codec_exactb codec candidate = true.
Proof.
  intros codec address absolute multiplicity candidate Hbuild.
  unfold construct_candidate_at in Hbuild.
  destruct (parse_absolute_datum absolute) as [view|] eqn:Hparse;
    [|discriminate].
  destruct (codec_decode_and_reencode_identity codec
    (datum_view_datum_identity view))
    as [decoded|] eqn:Hdecode; [|discriminate].
  destruct (path_eqb (decoded_datum_reencoded_identity decoded)
    (datum_view_datum_identity view)) eqn:Hcanonical;
    [|discriminate].
  destruct (path_eqb (decoded_datum_structure decoded)
    (datum_view_datum_structure view)) eqn:Hstructure; [|discriminate].
  destruct (digest_width_is_exact
    (codec_digest_raw_identity codec (datum_view_datum_identity view)))
    eqn:Hwidth; [|discriminate].
  destruct (multiplicity_valid multiplicity) eqn:Hmultiplicity; [|discriminate].
  inversion Hbuild; subst candidate; clear Hbuild.
  pose proof (@parse_absolute_datum_sound absolute view Hparse)
    as [_ Habsolute].
  assert (Hstorage : candidate_storage_path
    (MatchCandidateState address (decoded_view_marker view)
      (decoded_view_relative view) (datum_view_datum_structure view)
      (datum_view_datum_identity view)
      (codec_digest_raw_identity codec (datum_view_datum_identity view))
      (decoded_datum_persistent decoded) multiplicity) = absolute).
  { unfold candidate_storage_path, decoded_view_marker, decoded_view_relative.
    exact (eq_sym Habsolute). }
  unfold candidate_codec_exactb. rewrite Hstorage, Hparse, Hdecode. simpl.
  repeat rewrite path_eqb_reflexive.
  repeat first [rewrite Hcanonical | rewrite Hstructure | rewrite Hwidth |
    rewrite Hmultiplicity | rewrite Bool.eqb_refl].
  destruct (decoded_datum_persistent decoded); simpl;
    reflexivity.
Qed.

Lemma constructed_candidate_retains_address_and_multiplicity : forall
    codec address absolute multiplicity candidate,
  construct_candidate_at codec address absolute multiplicity = BuildOk candidate ->
  candidate_address candidate = address /\
  candidate_observed_multiplicity candidate = multiplicity.
Proof.
  intros codec address absolute multiplicity candidate Hbuild.
  unfold construct_candidate_at in Hbuild.
  destruct (parse_absolute_datum absolute) as [view|]; [|discriminate].
  destruct (codec_decode_and_reencode_identity codec
    (datum_view_datum_identity view)) as [decoded|]; [|discriminate].
  destruct (path_eqb (decoded_datum_reencoded_identity decoded)
    (datum_view_datum_identity view)); [|discriminate].
  destruct (path_eqb (decoded_datum_structure decoded)
    (datum_view_datum_structure view)); [|discriminate].
  destruct (digest_width_is_exact
    (codec_digest_raw_identity codec (datum_view_datum_identity view)));
    [|discriminate].
  destruct (multiplicity_valid multiplicity); [|discriminate].
  inversion Hbuild; subst candidate. split; reflexivity.
Qed.

Lemma constructed_stored_address_is_absolute : forall
    codec absolute multiplicity candidate,
  construct_stored_candidate codec absolute multiplicity = BuildOk candidate ->
  candidate_address candidate = StoredAddress absolute /\
  candidate_storage_path candidate = absolute.
Proof.
  intros codec absolute multiplicity candidate Hbuild.
  unfold construct_stored_candidate in Hbuild.
  unfold construct_candidate_at in Hbuild.
  destruct (parse_absolute_datum absolute) as [view|] eqn:Hparse;
    [|discriminate].
  repeat match goal with
  | H : context [match ?x with _ => _ end] |- _ => destruct x eqn:?; try discriminate
  | H : context [if ?x then _ else _] |- _ => destruct x eqn:?; try discriminate
  end.
  inversion Hbuild; subst candidate; clear Hbuild. split; [reflexivity|].
  unfold candidate_storage_path; simpl.
  pose proof (@parse_absolute_datum_sound absolute view Hparse) as [_ Hsound].
  exact (eq_sym Hsound).
Qed.

(** Requested-channel-local catalog construction ----------------------- *)

Definition CandidateOrderKey : Type := (Path * Path)%type.

Definition candidate_order_key (candidate : MatchCandidate) : CandidateOrderKey :=
  (candidate_digest candidate, candidate_canonical_identity candidate).

Definition candidate_beforeb (left right : MatchCandidate) : bool :=
  match path_compare (candidate_digest left) (candidate_digest right) with
  | Lt => true
  | Eq => path_ltb (candidate_canonical_identity left)
      (candidate_canonical_identity right)
  | Gt => false
  end.

Definition candidate_order_key_eqb
    (left right : MatchCandidate) : bool :=
  path_eqb (candidate_digest left) (candidate_digest right) &&
  path_eqb (candidate_canonical_identity left)
    (candidate_canonical_identity right).

Definition candidate_before_or_equalb
    (left right : MatchCandidate) : bool :=
  candidate_beforeb left right || candidate_order_key_eqb left right.

Fixpoint insert_candidate
    (candidate : MatchCandidate) (ordered : list MatchCandidate)
    : list MatchCandidate :=
  match ordered with
  | [] => [candidate]
  | head :: tail =>
      if candidate_beforeb candidate head
      then candidate :: ordered
      else head :: insert_candidate candidate tail
  end.

Fixpoint sort_candidates (candidates : list MatchCandidate)
    : list MatchCandidate :=
  match candidates with
  | [] => []
  | head :: tail => insert_candidate head (sort_candidates tail)
  end.

Fixpoint catalog_orderedb (candidates : list MatchCandidate) : bool :=
  match candidates with
  | [] | [_] => true
  | first :: (second :: _ as tail) =>
      candidate_before_or_equalb first second && catalog_orderedb tail
  end.

Fixpoint catalog_unique_identityb (candidates : list MatchCandidate) : bool :=
  match candidates with
  | [] | [_] => true
  | first :: (second :: _ as tail) =>
      negb (path_eqb (candidate_digest first) (candidate_digest second) &&
        path_eqb (candidate_canonical_identity first)
          (candidate_canonical_identity second)) &&
      catalog_unique_identityb tail
  end.

Record CandidateCatalog : Type := CandidateCatalogState {
  candidate_catalog_channel : Path;
  candidate_catalog_channel_was_loaded : bool;
  candidate_catalog_authority : list MatchCandidate
}.

Definition datum_channel_marker_validb (marker : Path) : bool :=
  if RhoPathV1Datum.bytes_valid marker then
    match strip_exact_prefix rho_path_v1_magic marker with
    | Some (kind :: framed) =>
        if Nat.eqb kind rho_path_v1_datum_kind then
          match parse_frame framed with
          | Some (_, after_structure) =>
              match parse_frame after_structure with
              | Some (_, []) => true
              | _ => false
              end
          | None => false
          end
        else false
    | _ => false
    end
  else false.

Fixpoint build_unsorted_catalog
    (codec : CanonicalDatumCodec) (channel : Path) (contents : ChannelBag)
    : BuildResult (list MatchCandidate) :=
  match contents with
  | [] => BuildOk []
  | (relative, multiplicity) :: tail =>
      let absolute := channel ++ relative in
      match construct_stored_candidate codec absolute multiplicity with
      | BuildRefused reason => BuildRefused reason
      | BuildOk candidate =>
          if path_eqb (candidate_channel_marker candidate) channel &&
              path_eqb (candidate_relative_datum candidate) relative then
            match build_unsorted_catalog codec channel tail with
            | BuildRefused reason => BuildRefused reason
            | BuildOk candidates => BuildOk (candidate :: candidates)
            end
          else BuildRefused (RefuseMalformedStoredPath absolute)
      end
  end.

Definition candidate_is_exactly_under_channel
    (codec : CanonicalDatumCodec) (channel : Path)
    (candidate : MatchCandidate) : Prop :=
  candidate_channel_marker candidate = channel /\
  candidate_address candidate =
    StoredAddress (channel ++ candidate_relative_datum candidate) /\
  candidate_codec_exactb codec candidate = true.

Lemma insert_candidate_preserves_forall : forall
    (predicate : MatchCandidate -> Prop) candidate ordered,
  predicate candidate -> Forall predicate ordered ->
  Forall predicate (insert_candidate candidate ordered).
Proof.
  intros predicate candidate ordered Hcandidate Hordered.
  induction Hordered as [|head tail Hhead Htail IH]; simpl.
  - now constructor.
  - destruct (candidate_beforeb candidate head).
    + constructor; [exact Hcandidate|now constructor].
    + constructor; [exact Hhead|exact IH].
Qed.

Lemma sort_candidates_preserves_forall : forall predicate candidates,
  Forall predicate candidates -> Forall predicate (sort_candidates candidates).
Proof.
  intros predicate candidates Hforall. induction Hforall; simpl; [constructor|].
  now apply insert_candidate_preserves_forall.
Qed.

Lemma built_unsorted_catalog_stays_under_requested_channel : forall
    codec channel contents candidates,
  build_unsorted_catalog codec channel contents = BuildOk candidates ->
  Forall (candidate_is_exactly_under_channel codec channel) candidates.
Proof.
  intros codec channel contents. induction contents as [|[relative amount] tail IH];
    intros candidates Hbuild; simpl in Hbuild.
  - inversion Hbuild. constructor.
  - destruct (construct_stored_candidate codec (channel ++ relative) amount)
      as [candidate|reason] eqn:Hcandidate; [|discriminate].
    destruct (path_eqb (candidate_channel_marker candidate) channel &&
      path_eqb (candidate_relative_datum candidate) relative) eqn:Hexact;
      [|discriminate].
    destruct (build_unsorted_catalog codec channel tail) as [rest|reason']
      eqn:Htail; [|discriminate].
    inversion Hbuild; subst candidates. constructor.
    + unfold candidate_is_exactly_under_channel.
      apply andb_true_iff in Hexact as [Hchannel Hrelative].
      apply path_eqb_spec in Hchannel. apply path_eqb_spec in Hrelative.
      pose proof (@constructed_stored_address_is_absolute
        codec (channel ++ relative) amount candidate Hcandidate)
        as [Haddress Hstorage].
      pose proof (@constructed_candidate_is_codec_exact codec
        (StoredAddress (channel ++ relative)) (channel ++ relative)
        amount candidate Hcandidate) as Hcodec.
      split; [exact Hchannel|]. split.
      * rewrite Hrelative. exact Haddress.
      * exact Hcodec.
    + now apply IH.
Qed.

Definition build_candidate_catalog
    (codec : CanonicalDatumCodec) (observed : TwoLevelAuthority)
    (channel : Path) : BuildResult CandidateCatalog :=
  if datum_channel_marker_validb channel then
    let observed_contents := lookup_channel channel observed in
    let contents := match observed_contents with Some bag => bag | None => [] end in
    match build_unsorted_catalog codec channel contents with
    | BuildRefused reason => BuildRefused reason
    | BuildOk candidates =>
        let ordered := sort_candidates candidates in
        if catalog_unique_identityb ordered then
          if catalog_orderedb ordered then
            BuildOk (CandidateCatalogState channel
              (match observed_contents with Some _ => true | None => false end)
              ordered)
          else BuildRefused (RefuseCatalogOrder channel)
        else
          match ordered with
          | first :: _ => BuildRefused (RefuseDuplicateDatumIdentity channel
              (candidate_canonical_identity first))
          | [] => BuildRefused (RefuseDuplicateDatumIdentity channel [])
          end
    end
  else BuildRefused (RefuseMalformedStoredPath channel).

Fixpoint path_memberb (path : Path) (paths : list Path) : bool :=
  match paths with
  | [] => false
  | head :: tail => path_eqb path head || path_memberb path tail
  end.

Fixpoint deduplicate_requested_channels (requested : list Path) : list Path :=
  match requested with
  | [] => []
  | channel :: tail =>
      let unique_tail := deduplicate_requested_channels tail in
      if path_memberb channel unique_tail then unique_tail
      else channel :: unique_tail
  end.

Record CandidateCatalogSet : Type := CandidateCatalogSetState {
  candidate_catalog_requested_channels : list Path;
  candidate_catalog_unique_channels : list Path;
  candidate_catalogs : list CandidateCatalog
}.

Fixpoint build_catalog_list
    (codec : CanonicalDatumCodec) (observed : TwoLevelAuthority)
    (channels : list Path) : BuildResult (list CandidateCatalog) :=
  match channels with
  | [] => BuildOk []
  | channel :: tail =>
      match build_candidate_catalog codec observed channel with
      | BuildRefused reason => BuildRefused reason
      | BuildOk catalog =>
          match build_catalog_list codec observed tail with
          | BuildRefused reason => BuildRefused reason
          | BuildOk catalogs => BuildOk (catalog :: catalogs)
          end
      end
  end.

Definition build_candidate_catalog_set
    (codec : CanonicalDatumCodec) (observed : TwoLevelAuthority)
    (requested : list Path) : BuildResult CandidateCatalogSet :=
  let unique := deduplicate_requested_channels requested in
  match build_catalog_list codec observed unique with
  | BuildRefused reason => BuildRefused reason
  | BuildOk catalogs => BuildOk (CandidateCatalogSetState requested unique catalogs)
  end.

Fixpoint lookup_candidate_catalog_list
    (channel : Path) (catalogs : list CandidateCatalog)
    : option CandidateCatalog :=
  match catalogs with
  | [] => None
  | catalog :: tail =>
      if path_eq_dec channel (candidate_catalog_channel catalog)
      then Some catalog else lookup_candidate_catalog_list channel tail
  end.

Definition lookup_candidate_catalog
    (channel : Path) (catalogs : CandidateCatalogSet)
    : option CandidateCatalog :=
  lookup_candidate_catalog_list channel (candidate_catalogs catalogs).

Lemma built_candidate_catalog_has_exact_channel : forall
    codec observed channel catalog,
  build_candidate_catalog codec observed channel = BuildOk catalog ->
  candidate_catalog_channel catalog = channel.
Proof.
  intros codec observed channel catalog Hbuild.
  unfold build_candidate_catalog in Hbuild.
  destruct (datum_channel_marker_validb channel); [|discriminate].
  remember (match lookup_channel channel observed with
    | Some bag => bag | None => [] end) as contents.
  destruct (build_unsorted_catalog codec channel contents) as [raw|reason];
    [|discriminate].
  destruct (catalog_unique_identityb (sort_candidates raw));
    [|destruct (sort_candidates raw); discriminate].
  destruct (catalog_orderedb (sort_candidates raw)); [|discriminate].
  inversion Hbuild; reflexivity.
Qed.

Lemma built_catalog_list_has_exact_channels : forall codec observed channels catalogs,
  build_catalog_list codec observed channels = BuildOk catalogs ->
  map candidate_catalog_channel catalogs = channels.
Proof.
  intros codec observed channels. induction channels as [|channel tail IH];
    intros catalogs Hbuild; simpl in Hbuild.
  - inversion Hbuild. reflexivity.
  - destruct (build_candidate_catalog codec observed channel) as [catalog|reason]
      eqn:Hcatalog; [|discriminate].
    destruct (build_catalog_list codec observed tail) as [rest|reason']
      eqn:Htail; [|discriminate].
    inversion Hbuild; subst catalogs. simpl.
    rewrite (@built_candidate_catalog_has_exact_channel
      codec observed channel catalog Hcatalog).
    f_equal. now apply IH.
Qed.

Theorem built_catalog_set_is_requested_local : forall
    codec observed requested catalogs,
  build_candidate_catalog_set codec observed requested = BuildOk catalogs ->
  candidate_catalog_requested_channels catalogs = requested /\
  candidate_catalog_unique_channels catalogs =
    deduplicate_requested_channels requested /\
  map candidate_catalog_channel (candidate_catalogs catalogs) =
    deduplicate_requested_channels requested.
Proof.
  intros codec observed requested catalogs Hbuild.
  unfold build_candidate_catalog_set in Hbuild.
  destruct (build_catalog_list codec observed
    (deduplicate_requested_channels requested)) as [built|reason] eqn:Hlist;
    [|discriminate].
  inversion Hbuild; subst catalogs. simpl. repeat split.
  now apply built_catalog_list_has_exact_channels in Hlist.
Qed.

Theorem requested_missing_or_loaded_empty_channel_has_explicit_empty_catalog :
  forall codec observed channel catalog,
  datum_channel_marker_validb channel = true ->
  (lookup_channel channel observed = None \/
   lookup_channel channel observed = Some []) ->
  build_candidate_catalog codec observed channel = BuildOk catalog ->
  candidate_catalog_channel catalog = channel /\
  candidate_catalog_authority catalog = [].
Proof.
  intros codec observed channel catalog Hvalid Hempty Hbuild.
  unfold build_candidate_catalog in Hbuild. rewrite Hvalid in Hbuild.
  destruct Hempty as [Hempty|Hempty]; rewrite Hempty in Hbuild; simpl in Hbuild;
    inversion Hbuild; subst catalog; split; reflexivity.
Qed.

Theorem built_catalog_is_sorted_and_duplicate_checked : forall
    codec observed channel catalog,
  build_candidate_catalog codec observed channel = BuildOk catalog ->
  catalog_orderedb (candidate_catalog_authority catalog) = true /\
  catalog_unique_identityb (candidate_catalog_authority catalog) = true.
Proof.
  intros codec observed channel catalog Hbuild.
  unfold build_candidate_catalog in Hbuild.
  destruct (datum_channel_marker_validb channel); [|discriminate].
  remember (match lookup_channel channel observed with
    | Some bag => bag | None => [] end) as contents.
  destruct (build_unsorted_catalog codec channel contents) as [raw|reason]
    eqn:Hraw; [|discriminate].
  destruct (catalog_unique_identityb (sort_candidates raw)) eqn:Hunique;
    [|destruct (sort_candidates raw); discriminate].
  destruct (catalog_orderedb (sort_candidates raw)) eqn:Hordered;
    [|discriminate].
  inversion Hbuild; subst catalog. split; assumption.
Qed.

Theorem built_catalog_candidates_stay_under_requested_channel : forall
    codec observed channel catalog,
  build_candidate_catalog codec observed channel = BuildOk catalog ->
  Forall (candidate_is_exactly_under_channel codec channel)
    (candidate_catalog_authority catalog).
Proof.
  intros codec observed channel catalog Hbuild.
  unfold build_candidate_catalog in Hbuild.
  destruct (datum_channel_marker_validb channel); [|discriminate].
  remember (match lookup_channel channel observed with
    | Some bag => bag | None => [] end) as contents.
  destruct (build_unsorted_catalog codec channel contents) as [raw|reason]
    eqn:Hraw; [|discriminate].
  destruct (catalog_unique_identityb (sort_candidates raw));
    [|destruct (sort_candidates raw); discriminate].
  destruct (catalog_orderedb (sort_candidates raw)); [|discriminate].
  inversion Hbuild; subst catalog.
  apply sort_candidates_preserves_forall.
  now apply built_unsorted_catalog_stays_under_requested_channel in Hraw.
Qed.

(** Flat Bag and exact BagDelta cancellation --------------------------- *)

Definition FlatBag : Type := list Entry.

Definition flat_lookup (path : Path) (bag : FlatBag) : Multiplicity :=
  lookup_multiplicity_in_bag path bag.

Fixpoint flat_set_count
    (path : Path) (count : Multiplicity) (bag : FlatBag) : FlatBag :=
  match bag with
  | [] => if N.eqb count 0 then [] else [(path, count)]
  | (current, value) :: tail =>
      if path_eq_dec path current then
        if N.eqb count 0 then tail else (current, count) :: tail
      else (current, value) :: flat_set_count path count tail
  end.

Fixpoint flat_paths_unique_from (seen : list Path) (bag : FlatBag) : bool :=
  match bag with
  | [] => true
  | (path, _) :: tail =>
      negb (path_memberb path seen) && flat_paths_unique_from (path :: seen) tail
  end.

Lemma path_memberb_spec : forall path paths,
  path_memberb path paths = true <-> In path paths.
Proof.
  intros path paths. induction paths as [|head tail IH]; simpl.
  - split; [discriminate|contradiction].
  - rewrite Bool.orb_true_iff, path_eqb_spec, IH. simpl.
    split; intros [Heq|Hin].
    + left. symmetry. exact Heq.
    + right. exact Hin.
    + left. symmetry. exact Heq.
    + right. exact Hin.
Qed.

Lemma path_memberb_false_spec : forall path paths,
  path_memberb path paths = false <-> ~ In path paths.
Proof.
  intros path paths. rewrite <- Bool.not_true_iff_false, path_memberb_spec.
  reflexivity.
Qed.

Lemma flat_paths_unique_tail : forall seen path count tail,
  flat_paths_unique_from seen ((path, count) :: tail) = true ->
  flat_paths_unique_from (path :: seen) tail = true.
Proof.
  intros. simpl in H. now apply andb_true_iff in H as [_ H].
Qed.

Lemma flat_paths_unique_from_excludes_seen : forall seen bag path,
  flat_paths_unique_from seen bag = true ->
  In path seen ->
  ~ In path (map fst bag).
Proof.
  intros seen bag. revert seen.
  induction bag as [|[head value] tail IH];
    intros seen path Hunique Hseen Hin; simpl in Hin; [exact Hin|].
  simpl in Hunique. apply andb_true_iff in Hunique as [Hhead Htail].
  destruct Hin as [Heq|Hin].
  - apply negb_true_iff in Hhead. apply path_memberb_false_spec in Hhead.
    apply Hhead. subst head. exact Hseen.
  - apply (IH (head :: seen) path Htail).
    + right. exact Hseen.
    + exact Hin.
Qed.

Lemma flat_paths_unique_head_not_in_tail : forall path count tail,
  flat_paths_unique_from [] ((path, count) :: tail) = true ->
  ~ In path (map fst tail).
Proof.
  intros path count tail Hunique.
  simpl in Hunique.
  eapply flat_paths_unique_from_excludes_seen.
  - exact Hunique.
  - now left.
Qed.

Lemma flat_paths_unique_from_weaken_seen : forall larger smaller bag,
  (forall path, In path smaller -> In path larger) ->
  flat_paths_unique_from larger bag = true ->
  flat_paths_unique_from smaller bag = true.
Proof.
  intros larger smaller bag. revert larger smaller.
  induction bag as [|[path count] tail IH]; intros larger smaller Hsubset Hunique;
    simpl in *; [reflexivity|].
  apply andb_true_iff in Hunique as [Hnot_large Htail].
  apply andb_true_iff. split.
  - apply negb_true_iff. apply path_memberb_false_spec.
    apply negb_true_iff in Hnot_large.
    apply path_memberb_false_spec in Hnot_large.
    intro Hin. apply Hnot_large. now apply Hsubset.
  - apply (IH (path :: larger) (path :: smaller)).
    + intros selected Hin.
      change (path = selected \/ In selected smaller) in Hin.
      change (path = selected \/ In selected larger).
      destruct Hin as [Heq|Hin].
      * left. exact Heq.
      * right. now apply Hsubset.
    + exact Htail.
Qed.

Lemma flat_paths_unique_implies_nodup : forall bag,
  flat_paths_unique_from [] bag = true -> NoDup (map fst bag).
Proof.
  intros bag Hunique. induction bag as [|[path count] tail IH]; simpl.
  - constructor.
  - constructor.
    + now apply flat_paths_unique_head_not_in_tail with (count := count).
    + apply IH.
      simpl in Hunique.
      eapply flat_paths_unique_from_weaken_seen.
      * intros selected Hin. contradiction.
      * exact Hunique.
Qed.

Lemma flat_lookup_absent : forall bag path,
  ~ In path (map fst bag) -> flat_lookup path bag = 0.
Proof.
  intros bag. induction bag as [|[current value] tail IH];
    intros path Hnot; simpl; [reflexivity|].
  destruct (path_eq_dec path current) as [Heq|Hneq].
  - exfalso. apply Hnot. simpl. now left.
  - apply IH. intro Hin. apply Hnot. simpl. now right.
Qed.

Lemma flat_set_count_target : forall bag path count,
  NoDup (map fst bag) ->
  flat_lookup path (flat_set_count path count bag) = count.
Proof.
  intros bag. induction bag as [|[current value] tail IH];
    intros path count Hunique; simpl in Hunique |- *.
  - destruct (N.eqb count 0) eqn:Hzero.
    + now apply N.eqb_eq in Hzero.
    + simpl. destruct (path_eq_dec path path); [reflexivity|contradiction].
  - destruct (path_eq_dec path current) as [Heq|Hneq].
    + subst current. destruct (N.eqb count 0) eqn:Hzero.
      * apply N.eqb_eq in Hzero. subst count.
        apply flat_lookup_absent.
        now inversion Hunique.
      * simpl. destruct (path_eq_dec path path); [reflexivity|contradiction].
    + simpl. destruct (path_eq_dec path current); [contradiction|].
      apply IH. now inversion Hunique.
Qed.

Definition checked_flat_add
    (path : Path) (amount : Multiplicity) (bag : FlatBag)
    : option FlatBag :=
  let after := flat_lookup path bag + amount in
  if N.leb after multiplicity_max
  then Some (flat_set_count path after bag)
  else None.

Definition flat_subtract
    (path : Path) (amount : Multiplicity) (bag : FlatBag) : FlatBag :=
  flat_set_count path (flat_lookup path bag - amount) bag.

Record FlatBagDelta : Type := FlatBagDeltaState {
  flat_delta_additions : FlatBag;
  flat_delta_removals : FlatBag
}.

Definition empty_flat_delta : FlatBagDelta := FlatBagDeltaState [] [].

(** Production order: compute cancellation, preflight destination u64
    addition, and only then return both updated sides.  A failed record is
    therefore non-mutating. *)
Definition flat_record_change
    (destination cancellation : FlatBag) (path : Path)
    (amount : Multiplicity) : option (FlatBag * FlatBag) :=
  if path_valid path && multiplicity_valid amount then
    let cancelled := N.min (flat_lookup path cancellation) amount in
    let remaining := amount - cancelled in
    match checked_flat_add path remaining destination with
    | None => None
    | Some destination' =>
        Some (destination', flat_subtract path cancelled cancellation)
    end
  else None.

Definition flat_record_addition
    (delta : FlatBagDelta) (path : Path) (amount : Multiplicity)
    : option FlatBagDelta :=
  match flat_record_change (flat_delta_additions delta)
      (flat_delta_removals delta) path amount with
  | None => None
  | Some (additions, removals) => Some (FlatBagDeltaState additions removals)
  end.

Definition flat_record_removal
    (delta : FlatBagDelta) (path : Path) (amount : Multiplicity)
    : option FlatBagDelta :=
  match flat_record_change (flat_delta_removals delta)
      (flat_delta_additions delta) path amount with
  | None => None
  | Some (removals, additions) => Some (FlatBagDeltaState additions removals)
  end.

Definition flat_bag_well_formedb (bag : FlatBag) : bool :=
  forallb (fun entry => path_valid (fst entry) && multiplicity_valid (snd entry)) bag &&
  flat_paths_unique_from [] bag.

Definition flat_delta_disjointb (delta : FlatBagDelta) : bool :=
  forallb (fun entry => N.eqb (flat_lookup (fst entry)
    (flat_delta_removals delta)) 0) (flat_delta_additions delta).

Definition flat_delta_well_formedb (delta : FlatBagDelta) : bool :=
  flat_bag_well_formedb (flat_delta_additions delta) &&
  flat_bag_well_formedb (flat_delta_removals delta) &&
  flat_delta_disjointb delta.

Lemma successful_checked_flat_add_target : forall path amount bag after,
  flat_paths_unique_from [] bag = true ->
  checked_flat_add path amount bag = Some after ->
  flat_lookup path after = flat_lookup path bag + amount.
Proof.
  intros path amount bag after Hunique Hadd.
  unfold checked_flat_add in Hadd.
  destruct (N.leb (flat_lookup path bag + amount) multiplicity_max);
    inversion Hadd; subst after.
  apply flat_set_count_target.
  now apply flat_paths_unique_implies_nodup.
Qed.

Lemma flat_record_addition_is_exact_opposite_cancellation : forall
    delta path amount additions removals,
  flat_paths_unique_from [] (flat_delta_additions delta) = true ->
  flat_paths_unique_from [] (flat_delta_removals delta) = true ->
  flat_record_addition delta path amount =
      Some (FlatBagDeltaState additions removals) ->
  let cancelled := N.min (flat_lookup path (flat_delta_removals delta)) amount in
  flat_lookup path removals =
      flat_lookup path (flat_delta_removals delta) - cancelled /\
  flat_lookup path additions =
      flat_lookup path (flat_delta_additions delta) + (amount - cancelled).
Proof.
  intros delta path amount additions removals Hadditions_unique
    Hremovals_unique Hrecord cancelled.
  unfold flat_record_addition, flat_record_change in Hrecord.
  destruct (path_valid path && multiplicity_valid amount); [|discriminate].
  destruct (checked_flat_add path
    (amount - N.min (flat_lookup path (flat_delta_removals delta)) amount)
    (flat_delta_additions delta)) as [destination|] eqn:Hadd; [|discriminate].
  inversion Hrecord; subst additions removals; clear Hrecord.
  split.
  - unfold flat_subtract, cancelled. apply flat_set_count_target.
    now apply flat_paths_unique_implies_nodup.
  - unfold cancelled.
    now apply successful_checked_flat_add_target in Hadd.
Qed.

(** Candidate-pool authority ------------------------------------------ *)

Record OrdinalCandidate : Type := OrdinalCandidateState {
  ordinal_candidate_index : N;
  ordinal_candidate_value : MatchCandidate
}.

Fixpoint enumerate_candidates_from
    (ordinal : N) (candidates : list MatchCandidate)
    : list OrdinalCandidate :=
  match candidates with
  | [] => []
  | candidate :: tail => OrdinalCandidateState ordinal candidate ::
      enumerate_candidates_from (N.succ ordinal) tail
  end.

Fixpoint find_candidate_by_absolute
    (absolute : Path) (catalogs : list CandidateCatalog)
    : option MatchCandidate :=
  match catalogs with
  | [] => None
  | catalog :: tail =>
      match find (fun candidate =>
          match candidate_address candidate with
          | FreshAddress => false
          | StoredAddress path => path_eqb path absolute
          end) (candidate_catalog_authority catalog) with
      | Some candidate => Some candidate
      | None => find_candidate_by_absolute absolute tail
      end
  end.

Definition catalog_absolute_availability
    (catalogs : CandidateCatalogSet) (absolute : Path) : Multiplicity :=
  match find_candidate_by_absolute absolute (candidate_catalogs catalogs) with
  | None => 0
  | Some candidate => candidate_observed_multiplicity candidate
  end.

Definition replay_exclusions_validb
    (catalogs : CandidateCatalogSet) (excluded : FlatBag) : bool :=
  flat_bag_well_formedb excluded &&
  forallb (fun entry =>
    N.eqb (snd entry) (catalog_absolute_availability catalogs (fst entry)))
    excluded.

Record CandidatePool : Type := CandidatePoolState {
  pool_observed_authority : TwoLevelAuthority;
  pool_requested_channels : list Path;
  pool_catalogs : CandidateCatalogSet;
  pool_fresh_arrival : option MatchCandidate;
  pool_fresh_available : bool;
  pool_replay_exclusions : FlatBag
}.

Definition build_candidate_pool
    (codec : CanonicalDatumCodec) (observed : TwoLevelAuthority)
    (requested : list Path) (fresh_absolute : option Path)
    (fresh_available : bool) (excluded : FlatBag)
    : BuildResult CandidatePool :=
  match build_candidate_catalog_set codec observed requested with
  | BuildRefused reason => BuildRefused reason
  | BuildOk catalogs =>
      let fresh_result :=
        match fresh_absolute with
        | None => BuildOk None
        | Some absolute =>
            match construct_fresh_candidate codec absolute with
            | BuildOk candidate => BuildOk (Some candidate)
            | BuildRefused reason => BuildRefused reason
            end
        end in
      match fresh_result with
      | BuildRefused reason => BuildRefused reason
      | BuildOk fresh =>
          if replay_exclusions_validb catalogs excluded then
            BuildOk (CandidatePoolState observed requested catalogs fresh
              (fresh_available &&
                match fresh with Some _ => true | None => false end) excluded)
          else
            match excluded with
            | (path, _) :: _ => BuildRefused (RefuseInvalidReplayExclusion path)
            | [] => BuildRefused (RefuseInvalidReplayExclusion [])
            end
      end
  end.

Definition stored_candidate_not_excludedb
    (pool : CandidatePool) (candidate : MatchCandidate) : bool :=
  match candidate_address candidate with
  | FreshAddress => false
  | StoredAddress absolute =>
      N.eqb (flat_lookup absolute (pool_replay_exclusions pool)) 0
  end.

Definition candidate_in_catalogb
    (pool : CandidatePool) (candidate : MatchCandidate) : bool :=
  match candidate_address candidate with
  | FreshAddress => false
  | StoredAddress absolute =>
      match find_candidate_by_absolute absolute
          (candidate_catalogs (pool_catalogs pool)) with
      | None => false
      | Some authoritative => match_candidate_eqb candidate authoritative
      end
  end.

Definition candidate_authorizedb
    (pool : CandidatePool) (candidate : MatchCandidate) : bool :=
  match candidate_address candidate with
  | FreshAddress =>
      pool_fresh_available pool &&
      match pool_fresh_arrival pool with
      | None => false
      | Some arrival => match_candidate_eqb candidate arrival
      end
  | StoredAddress _ =>
      candidate_in_catalogb pool candidate &&
      stored_candidate_not_excludedb pool candidate
  end.

Definition available_stored_count
    (pool : CandidatePool) (absolute : Path) : Multiplicity :=
  catalog_absolute_availability (pool_catalogs pool) absolute -
    flat_lookup absolute (pool_replay_exclusions pool).

Fixpoint stored_nonpersistent_demand
    (absolute : Path) (selected : list MatchCandidate) : Multiplicity :=
  match selected with
  | [] => 0
  | candidate :: tail =>
      let rest := stored_nonpersistent_demand absolute tail in
      match candidate_address candidate with
      | FreshAddress => rest
      | StoredAddress selected_path =>
          if candidate_persistent candidate then rest
          else if path_eq_dec absolute selected_path then N.succ rest else rest
      end
  end.

Fixpoint nonpersistent_fresh_demand
    (selected : list MatchCandidate) : Multiplicity :=
  match selected with
  | [] => 0
  | candidate :: tail =>
      let rest := nonpersistent_fresh_demand tail in
      match candidate_address candidate with
      | StoredAddress _ => rest
      | FreshAddress =>
          if candidate_persistent candidate then rest else N.succ rest
      end
  end.

Definition selected_candidate_feasibleb
    (pool : CandidatePool) (selected : list MatchCandidate)
    (candidate : MatchCandidate) : bool :=
  candidate_authorizedb pool candidate &&
  match candidate_address candidate with
  | FreshAddress =>
      if candidate_persistent candidate then true
      else N.leb (nonpersistent_fresh_demand selected) 1
  | StoredAddress absolute =>
      if candidate_persistent candidate
      then N.leb 1 (available_stored_count pool absolute)
      else N.leb (stored_nonpersistent_demand absolute selected)
          (available_stored_count pool absolute)
  end.

Definition selected_channels (selected : list MatchCandidate) : list Path :=
  map candidate_channel_marker selected.

Fixpoint path_list_eqb (left right : list Path) : bool :=
  match left, right with
  | [], [] => true
  | l :: lt, r :: rt => path_eqb l r && path_list_eqb lt rt
  | _, _ => false
  end.

Definition observed_pool_feasibleb
    (pool : CandidatePool) (selected : list MatchCandidate) : bool :=
  path_list_eqb (selected_channels selected) (pool_requested_channels pool) &&
  forallb (selected_candidate_feasibleb pool selected) selected.

Fixpoint reserve_selected_stored
    (selected : list MatchCandidate) (reserved : FlatBag)
    : option FlatBag :=
  match selected with
  | [] => Some reserved
  | candidate :: tail =>
      match candidate_address candidate with
      | FreshAddress => reserve_selected_stored tail reserved
      | StoredAddress absolute =>
          if candidate_persistent candidate
          then reserve_selected_stored tail reserved
          else
            match checked_flat_add absolute 1 reserved with
            | None => None
            | Some after => reserve_selected_stored tail after
            end
      end
  end.

Fixpoint entries_orderedb (entries : list Entry) : bool :=
  match entries with
  | [] | [_] => true
  | (first, _) :: ((second, _) :: _ as tail) =>
      path_ltb first second && entries_orderedb tail
  end.

Definition channel_entry_well_formedb (entry : ChannelEntry) : bool :=
  path_valid (channel_path entry) &&
  flat_bag_well_formedb (channel_bag entry) &&
  entries_orderedb (channel_bag entry).

Fixpoint channels_orderedb (authority : TwoLevelAuthority) : bool :=
  match authority with
  | [] | [_] => true
  | (first, _) :: ((second, _) :: _ as tail) =>
      path_ltb first second && channels_orderedb tail
  end.

Definition authority_well_formedb (authority : TwoLevelAuthority) : bool :=
  forallb channel_entry_well_formedb authority && channels_orderedb authority.

Record CheckedObservedPool : Type := CheckedObservedPoolState {
  checked_pool_codec : CanonicalDatumCodec;
  checked_pool_value : CandidatePool;
  checked_pool_observed_authority_well_formed :
    authority_well_formedb (pool_observed_authority checked_pool_value) = true;
  checked_pool_selection : list MatchCandidate;
  checked_pool_reserved : FlatBag;
  checked_pool_fresh_reserved : bool;
  checked_pool_catalog_construction :
      build_candidate_catalog_set checked_pool_codec
        (pool_observed_authority checked_pool_value)
        (pool_requested_channels checked_pool_value) =
      BuildOk (pool_catalogs checked_pool_value);
  checked_pool_replay_exclusions_are_full_or_none :
    replay_exclusions_validb (pool_catalogs checked_pool_value)
      (pool_replay_exclusions checked_pool_value) = true;
  checked_pool_fresh_codec_is_exact :
    match pool_fresh_arrival checked_pool_value with
    | None => True
    | Some fresh =>
        candidate_address fresh = FreshAddress /\
        candidate_observed_multiplicity fresh = 1 /\
        candidate_codec_exactb checked_pool_codec fresh = true
    end;
  checked_pool_feasibility :
    observed_pool_feasibleb checked_pool_value checked_pool_selection = true;
  checked_pool_reservations_are_exact :
    reserve_selected_stored checked_pool_selection [] =
      Some checked_pool_reserved;
  checked_pool_fresh_reservation_is_exact :
    checked_pool_fresh_reserved =
      negb (N.eqb (nonpersistent_fresh_demand checked_pool_selection) 0)
}.

Lemma constructed_fresh_candidate_is_exact : forall codec absolute candidate,
  construct_fresh_candidate codec absolute = BuildOk candidate ->
  candidate_address candidate = FreshAddress /\
  candidate_observed_multiplicity candidate = 1 /\
  candidate_codec_exactb codec candidate = true.
Proof.
  intros codec absolute candidate Hbuild.
  unfold construct_fresh_candidate in Hbuild.
  destruct (construct_candidate_at codec FreshAddress absolute 1)
    as [fresh|reason] eqn:Hfresh.
  - inversion Hbuild; subst candidate.
    pose proof (@constructed_candidate_retains_address_and_multiplicity
      codec FreshAddress absolute 1 fresh Hfresh) as [Haddress Hcount].
    split; [exact Haddress|]. split; [exact Hcount|].
    now apply (@constructed_candidate_is_codec_exact codec FreshAddress
      absolute 1 fresh Hfresh).
  - destruct reason; discriminate.
Qed.

Lemma built_candidate_pool_is_exact : forall
    codec observed requested fresh_absolute fresh_available excluded pool,
  build_candidate_pool codec observed requested fresh_absolute fresh_available excluded =
    BuildOk pool ->
  build_candidate_catalog_set codec observed requested =
      BuildOk (pool_catalogs pool) /\
  replay_exclusions_validb (pool_catalogs pool)
      (pool_replay_exclusions pool) = true /\
  match pool_fresh_arrival pool with
  | None => True
  | Some fresh =>
      candidate_address fresh = FreshAddress /\
      candidate_observed_multiplicity fresh = 1 /\
      candidate_codec_exactb codec fresh = true
  end.
Proof.
  intros codec observed requested fresh_absolute fresh_available excluded pool Hbuild.
  unfold build_candidate_pool in Hbuild.
  destruct (build_candidate_catalog_set codec observed requested)
    as [catalogs|reason] eqn:Hcatalogs; [|discriminate].
  destruct fresh_absolute as [absolute|].
  - destruct (construct_fresh_candidate codec absolute)
      as [fresh|fresh_reason] eqn:Hfresh; [|discriminate].
    destruct (replay_exclusions_validb catalogs excluded) eqn:Hexcluded.
    + inversion Hbuild; subst pool. simpl.
      split; [reflexivity|]. split; [exact Hexcluded|].
      now apply (@constructed_fresh_candidate_is_exact
        codec absolute fresh Hfresh).
    + destruct excluded as [|[excluded_path excluded_count] excluded_tail];
        discriminate Hbuild.
  - destruct (replay_exclusions_validb catalogs excluded) eqn:Hexcluded.
    + inversion Hbuild; subst pool. simpl.
      split; [reflexivity|]. split; [exact Hexcluded|exact I].
    + destruct excluded as [|[excluded_path excluded_count] excluded_tail];
        discriminate Hbuild.
Qed.

Definition check_observed_pool
    (codec : CanonicalDatumCodec) (observed : TwoLevelAuthority)
    (observed_well_formed : authority_well_formedb observed = true)
    (requested : list Path) (fresh_absolute : option Path)
    (fresh_available : bool) (excluded : FlatBag)
    (selected : list MatchCandidate) : BuildResult CheckedObservedPool.
Proof.
  destruct (build_candidate_pool codec observed requested fresh_absolute
    fresh_available excluded) as [pool|reason] eqn:Hpool.
  - unfold build_candidate_pool in Hpool.
    destruct (build_candidate_catalog_set codec observed requested)
      as [catalogs|catalog_reason] eqn:Hcatalogs; [|discriminate].
    destruct fresh_absolute as [fresh_absolute|].
    + destruct (construct_fresh_candidate codec fresh_absolute)
        as [fresh|fresh_reason] eqn:Hfresh; [|discriminate].
      destruct (replay_exclusions_validb catalogs excluded) eqn:Hexcluded.
      * inversion Hpool; subst pool.
      destruct (observed_pool_feasibleb
        (CandidatePoolState observed requested catalogs (Some fresh)
          (fresh_available && true) excluded) selected) eqn:Hfeasible;
        [ destruct (reserve_selected_stored selected []) as [reserved|]
            eqn:Hreserved;
          [ refine (BuildOk {| checked_pool_codec := codec;
            checked_pool_value := CandidatePoolState observed requested catalogs
              (Some fresh) (fresh_available && true) excluded;
            checked_pool_observed_authority_well_formed := observed_well_formed;
            checked_pool_selection := selected;
            checked_pool_reserved := reserved;
            checked_pool_fresh_reserved :=
              negb (N.eqb (nonpersistent_fresh_demand selected) 0);
            checked_pool_catalog_construction := Hcatalogs;
            checked_pool_replay_exclusions_are_full_or_none := Hexcluded;
            checked_pool_fresh_codec_is_exact :=
              (@constructed_fresh_candidate_is_exact codec fresh_absolute
                fresh Hfresh);
            checked_pool_feasibility := Hfeasible;
            checked_pool_reservations_are_exact := Hreserved;
            checked_pool_fresh_reservation_is_exact := eq_refl |})
          | exact (BuildRefused (RefuseReservationOverflow [])) ]
        | exact (BuildRefused RefuseObservedPoolFeasibility) ].
      * destruct excluded as [|[excluded_path excluded_count] excluded_tail];
          discriminate Hpool.
    + destruct (replay_exclusions_validb catalogs excluded) eqn:Hexcluded;
        [|idtac].
      * inversion Hpool; subst pool.
      destruct (observed_pool_feasibleb
        (CandidatePoolState observed requested catalogs None
          fresh_available excluded) selected) eqn:Hfeasible;
        [ destruct (reserve_selected_stored selected []) as [reserved|]
            eqn:Hreserved;
          [ refine (BuildOk {| checked_pool_codec := codec;
            checked_pool_value := CandidatePoolState observed requested catalogs
              None fresh_available excluded;
            checked_pool_observed_authority_well_formed := observed_well_formed;
            checked_pool_selection := selected;
            checked_pool_reserved := reserved;
            checked_pool_fresh_reserved :=
              negb (N.eqb (nonpersistent_fresh_demand selected) 0);
            checked_pool_catalog_construction := Hcatalogs;
            checked_pool_replay_exclusions_are_full_or_none := Hexcluded;
            checked_pool_fresh_codec_is_exact := I;
            checked_pool_feasibility := Hfeasible;
            checked_pool_reservations_are_exact := Hreserved;
            checked_pool_fresh_reservation_is_exact := eq_refl |})
          | exact (BuildRefused (RefuseReservationOverflow [])) ]
        | exact (BuildRefused RefuseObservedPoolFeasibility) ].
      * destruct excluded as [|[excluded_path excluded_count] excluded_tail];
          discriminate Hpool.
  - exact (BuildRefused reason).
Defined.

Theorem checked_pool_replay_exclusion_is_full_multiplicity : forall
    checked absolute amount,
  In (absolute, amount)
    (pool_replay_exclusions (checked_pool_value checked)) ->
  amount = catalog_absolute_availability
    (pool_catalogs (checked_pool_value checked)) absolute.
Proof.
  intros checked absolute amount Hin.
  pose proof (checked_pool_replay_exclusions_are_full_or_none checked) as Hvalid.
  unfold replay_exclusions_validb in Hvalid.
  apply andb_true_iff in Hvalid as [_ Hall].
  apply forallb_forall with (x := (absolute, amount)) in Hall; [|exact Hin].
  simpl in Hall. now apply N.eqb_eq.
Qed.

Theorem checked_nonpersistent_stored_reservation_respects_multiplicity : forall
    checked candidate absolute,
  In candidate (checked_pool_selection checked) ->
  candidate_address candidate = StoredAddress absolute ->
  candidate_persistent candidate = false ->
  (stored_nonpersistent_demand absolute (checked_pool_selection checked) <=
    available_stored_count (checked_pool_value checked) absolute)%N.
Proof.
  intros checked candidate absolute Hin Haddress Hpersistent.
  pose proof (checked_pool_feasibility checked) as Hfeasible.
  unfold observed_pool_feasibleb in Hfeasible.
  apply andb_true_iff in Hfeasible as [_ Hall].
  apply forallb_forall with (x := candidate) in Hall; [|exact Hin].
  unfold selected_candidate_feasibleb in Hall.
  apply andb_true_iff in Hall as [_ Hcapacity].
  rewrite Haddress, Hpersistent in Hcapacity. now apply N.leb_le.
Qed.

Theorem checked_persistent_stored_candidate_requires_observed_presence : forall
    checked candidate absolute,
  In candidate (checked_pool_selection checked) ->
  candidate_address candidate = StoredAddress absolute ->
  candidate_persistent candidate = true ->
  (1 <= available_stored_count (checked_pool_value checked) absolute)%N.
Proof.
  intros checked candidate absolute Hin Haddress Hpersistent.
  pose proof (checked_pool_feasibility checked) as Hfeasible.
  unfold observed_pool_feasibleb in Hfeasible.
  apply andb_true_iff in Hfeasible as [_ Hall].
  apply forallb_forall with (x := candidate) in Hall; [|exact Hin].
  unfold selected_candidate_feasibleb in Hall.
  apply andb_true_iff in Hall as [_ Hcapacity].
  rewrite Haddress, Hpersistent in Hcapacity. now apply N.leb_le.
Qed.

Theorem checked_nonpersistent_fresh_is_single_use : forall checked,
  (nonpersistent_fresh_demand (checked_pool_selection checked) <= 1)%N.
Proof.
  intro checked.
  destruct (N.eq_dec (nonpersistent_fresh_demand
    (checked_pool_selection checked)) 0) as [Hzero|Hnonzero].
  - rewrite Hzero. lia.
  - assert (exists fresh,
      In fresh (checked_pool_selection checked) /\
      candidate_address fresh = FreshAddress /\
      candidate_persistent fresh = false) as [fresh [Hin [Haddress Hpersistent]]].
    { induction (checked_pool_selection checked) as [|candidate tail IH];
        simpl in Hnonzero; [contradiction|].
      destruct (candidate_address candidate) eqn:Haddress.
      - destruct (candidate_persistent candidate) eqn:Hpersistent.
        + destruct IH as [fresh [Hin [Ha Hp]]].
          * exact Hnonzero.
          * exists fresh. repeat split; try assumption. now right.
        + exists candidate. repeat split; try assumption. now left.
      - destruct IH as [fresh [Hin [Ha Hp]]].
        + exact Hnonzero.
        + exists fresh. repeat split; try assumption. now right. }
    pose proof (checked_pool_feasibility checked) as Hfeasible.
    unfold observed_pool_feasibleb in Hfeasible.
    apply andb_true_iff in Hfeasible as [_ Hall].
    apply forallb_forall with (x := fresh) in Hall; [|exact Hin].
    unfold selected_candidate_feasibleb in Hall.
    apply andb_true_iff in Hall as [_ Hcapacity].
    now rewrite Haddress, Hpersistent in Hcapacity; apply N.leb_le.
Qed.

Theorem persistent_fresh_feasibility_has_no_reservation_cap : forall
    pool selected fresh,
  candidate_address fresh = FreshAddress ->
  candidate_persistent fresh = true ->
  selected_candidate_feasibleb pool selected fresh =
    candidate_authorizedb pool fresh.
Proof.
  intros pool selected fresh Haddress Hpersistent.
  unfold selected_candidate_feasibleb. rewrite Haddress, Hpersistent.
  now rewrite Bool.andb_true_r.
Qed.

Definition full_scan_candidates
    (pool : CandidatePool) (channel : Path) : list MatchCandidate :=
  let stored :=
    match lookup_candidate_catalog channel (pool_catalogs pool) with
    | None => []
    | Some catalog => filter (candidate_authorizedb pool)
        (candidate_catalog_authority catalog)
    end in
  match pool_fresh_arrival pool with
  | Some fresh =>
      if path_eqb channel (candidate_channel_marker fresh) &&
          pool_fresh_available pool
      then fresh :: stored else stored
  | None => stored
  end.

Definition full_scan_projection
    (pool : CandidatePool) (channel : Path) : list OrdinalCandidate :=
  enumerate_candidates_from 0 (full_scan_candidates pool channel).

(** Query pruning is permitted only behind this certificate. The subsequence
    law preserves the production catalog order; match completeness is the
    bridge instantiated by SpatialMatcher's exact/hole/ordered compiler
    theorems. Unsupported RhoPath patterns use [certified_full_scan]. *)
Record CertifiedCandidateScan
    (full : list OrdinalCandidate)
    (matcher_accepts : OrdinalCandidate -> bool) : Type := {
  certified_scan_projection : list OrdinalCandidate;
  certified_scan_is_ordered_subsequence :
    exists mask : OrdinalCandidate -> bool,
      certified_scan_projection = filter mask full;
  certified_scan_is_match_complete : forall candidate,
    In candidate full -> matcher_accepts candidate = true ->
    In candidate certified_scan_projection
}.

Definition certified_full_scan
    (full : list OrdinalCandidate)
    (matcher_accepts : OrdinalCandidate -> bool)
    : CertifiedCandidateScan full matcher_accepts.
Proof.
  refine {| certified_scan_projection := full;
            certified_scan_is_ordered_subsequence := _;
            certified_scan_is_match_complete := _ |}.
  - exists (fun _ => true). induction full; simpl; [reflexivity|].
    f_equal. exact IHfull.
  - intros candidate Hin _. exact Hin.
Defined.

(** Commit liveness is computed, never supplied by a caller -------------- *)

Definition commit_absolute_count
    (commit : TwoLevelAuthority) (absolute : Path) : Multiplicity :=
  match parse_absolute_datum absolute with
  | None => 0
  | Some view => lookup_multiplicity commit
      (decoded_view_marker view) (decoded_view_relative view)
  end.

Definition selected_commit_liveb
    (commit : TwoLevelAuthority) (selected : list MatchCandidate)
    (candidate : MatchCandidate) : bool :=
  match candidate_address candidate with
  | FreshAddress => true
  | StoredAddress absolute =>
      if candidate_persistent candidate
      then N.leb 1 (commit_absolute_count commit absolute)
      else N.leb (stored_nonpersistent_demand absolute selected)
          (commit_absolute_count commit absolute)
  end.

Definition commit_selection_liveb
    (commit : TwoLevelAuthority) (selected : list MatchCandidate) : bool :=
  forallb (selected_commit_liveb commit selected) selected.

Record CheckedCommitLiveness (checked : CheckedObservedPool) : Type := CheckedCommitLivenessState {
  checked_commit_authority : TwoLevelAuthority;
  checked_commit_liveness :
    commit_selection_liveb checked_commit_authority
      (checked_pool_selection checked) = true
}.

Theorem checked_persistent_stored_candidate_is_live : forall pool (checked : CheckedCommitLiveness pool) candidate absolute,
  In candidate (checked_pool_selection pool) ->
  candidate_address candidate = StoredAddress absolute ->
  candidate_persistent candidate = true ->
  (1 <= commit_absolute_count (checked_commit_authority checked) absolute)%N.
Proof.
  intros pool checked candidate absolute Hin Haddress Hpersistent.
  pose proof (checked_commit_liveness checked) as Hlive.
  unfold commit_selection_liveb in Hlive.
  apply forallb_forall with (x := candidate) in Hlive; [|exact Hin].
  unfold selected_commit_liveb in Hlive. rewrite Haddress, Hpersistent in Hlive.
  now apply N.leb_le.
Qed.

Theorem checked_nonpersistent_stored_demand_is_live : forall
    pool (checked : CheckedCommitLiveness pool) candidate absolute,
  In candidate (checked_pool_selection pool) ->
  candidate_address candidate = StoredAddress absolute ->
  candidate_persistent candidate = false ->
  (stored_nonpersistent_demand absolute
      (checked_pool_selection pool) <=
    commit_absolute_count (checked_commit_authority checked) absolute)%N.
Proof.
  intros pool checked candidate absolute Hin Haddress Hpersistent.
  pose proof (checked_commit_liveness checked) as Hlive.
  unfold commit_selection_liveb in Hlive.
  apply forallb_forall with (x := candidate) in Hlive; [|exact Hin].
  unfold selected_commit_liveb in Hlive. rewrite Haddress, Hpersistent in Hlive.
  now apply N.leb_le.
Qed.

(** Flat match delta and parse-before-partition ------------------------- *)

Fixpoint record_selected_removals
    (selected : list MatchCandidate) (delta : FlatBagDelta)
    : BuildResult FlatBagDelta :=
  match selected with
  | [] => BuildOk delta
  | candidate :: tail =>
      match candidate_address candidate with
      | FreshAddress => record_selected_removals tail delta
      | StoredAddress absolute =>
          if candidate_persistent candidate
          then record_selected_removals tail delta
          else
            match flat_record_removal delta absolute 1 with
            | None => BuildRefused (RefuseFlatDelta absolute)
            | Some after => record_selected_removals tail after
            end
      end
  end.

Definition selected_contains_freshb (selected : list MatchCandidate) : bool :=
  existsb (fun candidate =>
    match candidate_address candidate with
    | FreshAddress => true
    | StoredAddress _ => false
    end) selected.

Definition record_unselected_arrival
    (arrival : option MatchCandidate) (selected : list MatchCandidate)
    (delta : FlatBagDelta) : BuildResult FlatBagDelta :=
  match arrival with
  | None => BuildOk delta
  | Some fresh =>
      if selected_contains_freshb selected then BuildOk delta
      else
        let absolute := candidate_storage_path fresh in
        match flat_record_addition delta absolute 1 with
        | None => BuildRefused (RefuseFlatDelta absolute)
        | Some after => BuildOk after
        end
  end.

Definition build_match_flat_delta
    (pool : CandidatePool) (selected : list MatchCandidate)
    : BuildResult FlatBagDelta :=
  match record_selected_removals selected empty_flat_delta with
  | BuildRefused reason => BuildRefused reason
  | BuildOk removals =>
      match record_unselected_arrival (pool_fresh_arrival pool) selected removals with
      | BuildRefused reason => BuildRefused reason
      | BuildOk delta =>
          if flat_delta_well_formedb delta then BuildOk delta
          else BuildRefused RefuseFlatDeltaInvariant
      end
  end.

Inductive ChangePolarity : Type := ChangeAddition | ChangeRemoval.

Record ParsedFlatChange : Type := ParsedFlatChangeState {
  parsed_change_absolute : Path;
  parsed_change_marker : Path;
  parsed_change_relative : Path;
  parsed_change_polarity : ChangePolarity;
  parsed_change_amount : Multiplicity
}.

Definition parse_flat_change
    (polarity : ChangePolarity) (entry : Entry)
    : BuildResult ParsedFlatChange :=
  let absolute := fst entry in
  match parse_absolute_datum absolute with
  | None => BuildRefused (RefuseFlatDeltaPartition absolute)
  | Some view => BuildOk (ParsedFlatChangeState absolute
      (decoded_view_marker view) (decoded_view_relative view)
      polarity (snd entry))
  end.

Fixpoint parse_flat_side
    (polarity : ChangePolarity) (entries : FlatBag)
    : BuildResult (list ParsedFlatChange) :=
  match entries with
  | [] => BuildOk []
  | entry :: tail =>
      match parse_flat_change polarity entry with
      | BuildRefused reason => BuildRefused reason
      | BuildOk change =>
          match parse_flat_side polarity tail with
          | BuildRefused reason => BuildRefused reason
          | BuildOk changes => BuildOk (change :: changes)
          end
      end
  end.

Definition parse_all_flat_changes
    (delta : FlatBagDelta) : BuildResult (list ParsedFlatChange) :=
  match parse_flat_side ChangeAddition (flat_delta_additions delta) with
  | BuildRefused reason => BuildRefused reason
  | BuildOk additions =>
      match parse_flat_side ChangeRemoval (flat_delta_removals delta) with
      | BuildRefused reason => BuildRefused reason
      | BuildOk removals => BuildOk (additions ++ removals)
      end
  end.

Record PartitionedChannelDelta : Type := PartitionedChannelDeltaState {
  partitioned_channel_marker : Path;
  partitioned_channel_delta : FlatBagDelta
}.

Fixpoint record_partitioned_change
    (change : ParsedFlatChange) (groups : list PartitionedChannelDelta)
    : BuildResult (list PartitionedChannelDelta) :=
  match groups with
  | [] =>
      let recorded :=
        match parsed_change_polarity change with
        | ChangeAddition => flat_record_addition empty_flat_delta
        | ChangeRemoval => flat_record_removal empty_flat_delta
        end (parsed_change_relative change) (parsed_change_amount change) in
      match recorded with
      | None => BuildRefused (RefuseFlatDeltaPartition
          (parsed_change_absolute change))
      | Some delta => BuildOk [PartitionedChannelDeltaState
          (parsed_change_marker change) delta]
      end
  | group :: tail =>
      if path_eq_dec (parsed_change_marker change)
          (partitioned_channel_marker group) then
        let recorded :=
          match parsed_change_polarity change with
          | ChangeAddition => flat_record_addition (partitioned_channel_delta group)
          | ChangeRemoval => flat_record_removal (partitioned_channel_delta group)
          end (parsed_change_relative change) (parsed_change_amount change) in
        match recorded with
        | None => BuildRefused (RefuseFlatDeltaPartition
            (parsed_change_absolute change))
        | Some delta => BuildOk (PartitionedChannelDeltaState
            (partitioned_channel_marker group) delta :: tail)
        end
      else
        match record_partitioned_change change tail with
        | BuildRefused reason => BuildRefused reason
        | BuildOk after => BuildOk (group :: after)
        end
  end.

Fixpoint partition_parsed_changes
    (changes : list ParsedFlatChange) (groups : list PartitionedChannelDelta)
    : BuildResult (list PartitionedChannelDelta) :=
  match changes with
  | [] => BuildOk groups
  | change :: tail =>
      match record_partitioned_change change groups with
      | BuildRefused reason => BuildRefused reason
      | BuildOk after => partition_parsed_changes tail after
      end
  end.

Definition partition_flat_delta
    (delta : FlatBagDelta) : BuildResult (list PartitionedChannelDelta) :=
  (** No group exists until every absolute path has parsed successfully. *)
  match parse_all_flat_changes delta with
  | BuildRefused reason => BuildRefused reason
  | BuildOk changes => partition_parsed_changes changes []
  end.

Fixpoint lookup_partitioned_delta
    (marker : Path) (groups : list PartitionedChannelDelta)
    : option FlatBagDelta :=
  match groups with
  | [] => None
  | group :: tail =>
      if path_eq_dec marker (partitioned_channel_marker group)
      then Some (partitioned_channel_delta group)
      else lookup_partitioned_delta marker tail
  end.

Theorem parsed_flat_change_reconstructs_absolute : forall polarity entry change,
  parse_flat_change polarity entry = BuildOk change ->
  parsed_change_absolute change =
    parsed_change_marker change ++ parsed_change_relative change.
Proof.
  intros polarity [absolute amount] change Hparse.
  unfold parse_flat_change in Hparse. simpl in Hparse.
  destruct (parse_absolute_datum absolute) as [view|] eqn:Hview;
    [|discriminate].
  inversion Hparse; subst change. simpl.
  pose proof (@parse_absolute_datum_sound absolute view Hview) as [_ Hsound].
  exact Hsound.
Qed.

Record FullyParsedPartition (delta : FlatBagDelta) : Type :=
  FullyParsedPartitionState {
    fully_parsed_changes : list ParsedFlatChange;
    fully_partitioned_groups : list PartitionedChannelDelta;
    fully_parsed_every_absolute_path :
      parse_all_flat_changes delta = BuildOk fully_parsed_changes;
    fully_partitioned_only_after_parse :
      partition_parsed_changes fully_parsed_changes [] =
        BuildOk fully_partitioned_groups
  }.

Definition authorize_fully_parsed_partition
    (delta : FlatBagDelta) : BuildResult (FullyParsedPartition delta).
Proof.
  destruct (parse_all_flat_changes delta) as [changes|reason] eqn:Hparse.
  - destruct (partition_parsed_changes changes []) as [groups|reason]
      eqn:Hpartition.
    + exact (BuildOk (@FullyParsedPartitionState delta changes groups
        Hparse Hpartition)).
    + exact (BuildRefused reason).
  - exact (BuildRefused reason).
Defined.

Theorem malformed_partition_cannot_construct_publication_evidence : forall
    delta reason (permit : FullyParsedPartition delta),
  parse_all_flat_changes delta = BuildRefused reason -> False.
Proof.
  intros delta reason permit Hrefused.
  destruct permit as [changes groups Hparsed Hpartition]. congruence.
Qed.

Lemma partition_flat_delta_of_fully_parsed_components : forall
    delta changes groups,
  parse_all_flat_changes delta = BuildOk changes ->
  partition_parsed_changes changes [] = BuildOk groups ->
  partition_flat_delta delta = BuildOk groups.
Proof.
  intros delta changes groups Hparse Hpartition.
  unfold partition_flat_delta. rewrite Hparse. exact Hpartition.
Qed.

Theorem fully_parsed_partition_is_exact_flat_partition : forall
    delta (permit : FullyParsedPartition delta),
  partition_flat_delta delta = BuildOk (fully_partitioned_groups permit).
Proof.
  intros delta [changes groups Hparse Hpartition].
  eapply partition_flat_delta_of_fully_parsed_components.
  - exact Hparse.
  - exact Hpartition.
Qed.

(** End-to-end executable PREPARE -------------------------------------- *)

Definition check_commit_liveness
    (checked : CheckedObservedPool) (commit : TwoLevelAuthority)
    : BuildResult (CheckedCommitLiveness checked).
Proof.
  destruct (authority_well_formedb commit) eqn:Hauthority.
  - destruct (commit_selection_liveb commit (checked_pool_selection checked))
      eqn:Hlive.
    + exact (BuildOk (CheckedCommitLivenessState checked commit Hlive)).
    + exact (BuildRefused RefuseCommitLiveness).
  - exact (BuildRefused RefuseCommitAuthority).
Defined.

Lemma built_match_flat_delta_is_well_formed : forall pool selected delta,
  build_match_flat_delta pool selected = BuildOk delta ->
  flat_delta_well_formedb delta = true.
Proof.
  intros pool selected delta Hbuild.
  unfold build_match_flat_delta in Hbuild.
  destruct (record_selected_removals selected empty_flat_delta)
    as [removals|reason] eqn:Hremovals; [|discriminate].
  destruct (record_unselected_arrival (pool_fresh_arrival pool)
    selected removals) as [prepared|reason'] eqn:Harrival; [|discriminate].
  destruct (flat_delta_well_formedb prepared) eqn:Hwellformed;
    [|discriminate].
  inversion Hbuild; subst delta. exact Hwellformed.
Qed.

Record MatchPreparationInput : Type := MatchPreparationInputState {
  preparation_codec : CanonicalDatumCodec;
  preparation_observed_authority : TwoLevelAuthority;
  preparation_commit_authority : TwoLevelAuthority;
  preparation_requested_channels : list Path;
  preparation_fresh_absolute : option Path;
  preparation_fresh_available : bool;
  preparation_replay_exclusions : FlatBag;
  preparation_selected_candidates : list MatchCandidate
}.

Record PreparedMatchChanges : Type := PreparedMatchChangesState {
  prepared_checked_pool : CheckedObservedPool;
  prepared_checked_commit :
    CheckedCommitLiveness prepared_checked_pool;
  prepared_flat_delta : FlatBagDelta;
  prepared_partition_permit : FullyParsedPartition prepared_flat_delta;
  prepared_observed_authority_is_checked :
    authority_well_formedb
      (pool_observed_authority
        (checked_pool_value prepared_checked_pool)) = true;
  prepared_delta_is_primary_executable_output :
    build_match_flat_delta
      (checked_pool_value prepared_checked_pool)
      (checked_pool_selection prepared_checked_pool) =
    BuildOk prepared_flat_delta;
  prepared_delta_invariant : flat_delta_well_formedb prepared_flat_delta = true;
}.

Definition prepared_partitioned_delta
    (prepared : PreparedMatchChanges) : list PartitionedChannelDelta :=
  fully_partitioned_groups (prepared_partition_permit prepared).

Inductive MatchPreparationOutcome : Type :=
| PreparationReady (prepared : PreparedMatchChanges)
| PreparationRefused (reason : PreparationRefusal).

Lemma built_candidate_pool_preserves_observed_authority : forall
    codec observed requested fresh_absolute fresh_available excluded pool,
  build_candidate_pool codec observed requested fresh_absolute fresh_available
    excluded = BuildOk pool ->
  pool_observed_authority pool = observed.
Proof.
  intros codec observed requested fresh_absolute fresh_available excluded pool
    Hbuild.
  unfold build_candidate_pool in Hbuild.
  destruct (build_candidate_catalog_set codec observed requested)
    as [catalogs|catalog_reason] eqn:Hcatalogs; [|discriminate].
  destruct fresh_absolute as [absolute|].
  - destruct (construct_fresh_candidate codec absolute)
      as [fresh|fresh_reason] eqn:Hfresh; [|discriminate].
    destruct (replay_exclusions_validb catalogs excluded) eqn:Hexcluded.
    + inversion Hbuild; reflexivity.
    + destruct excluded as [|[excluded_path excluded_count] excluded_tail];
        discriminate Hbuild.
  - destruct (replay_exclusions_validb catalogs excluded) eqn:Hexcluded.
    + inversion Hbuild; reflexivity.
    + destruct excluded as [|[excluded_path excluded_count] excluded_tail];
        discriminate Hbuild.
Qed.

Definition checked_observed_authority_or_default
    (default : TwoLevelAuthority)
    (result : BuildResult CheckedObservedPool) : TwoLevelAuthority :=
  match result with
  | BuildOk checked => pool_observed_authority (checked_pool_value checked)
  | BuildRefused _ => default
  end.

Definition prepare_match_changes
    (input : MatchPreparationInput) : MatchPreparationOutcome.
Proof.
  destruct (authority_well_formedb (preparation_observed_authority input))
    eqn:Hobserved.
  - destruct (check_observed_pool
      (preparation_codec input)
      (preparation_observed_authority input)
      Hobserved
      (preparation_requested_channels input)
      (preparation_fresh_absolute input)
      (preparation_fresh_available input)
      (preparation_replay_exclusions input)
      (preparation_selected_candidates input)) as [checked|reason]
      eqn:Hpool.
    + destruct (check_commit_liveness checked
        (preparation_commit_authority input)) as [live|live_reason]
        eqn:Hlive.
      * destruct (build_match_flat_delta (checked_pool_value checked)
          (checked_pool_selection checked)) as [delta|delta_reason]
          eqn:Hdelta.
        -- pose proof (@built_match_flat_delta_is_well_formed
            (checked_pool_value checked) (checked_pool_selection checked)
            delta Hdelta) as Hdelta_valid.
           destruct (authorize_fully_parsed_partition delta)
             as [partition|partition_reason] eqn:Hpartition.
           ++ apply PreparationReady.
              refine ({| prepared_checked_pool := checked;
                prepared_checked_commit := live;
                prepared_flat_delta := delta;
                prepared_partition_permit := partition;
                prepared_observed_authority_is_checked := _;
                prepared_delta_is_primary_executable_output := Hdelta;
                prepared_delta_invariant := Hdelta_valid |}).
              ** exact (checked_pool_observed_authority_well_formed checked).
           ++ exact (PreparationRefused partition_reason).
        -- exact (PreparationRefused delta_reason).
      * exact (PreparationRefused live_reason).
    + exact (PreparationRefused reason).
  - exact (PreparationRefused RefuseObservedAuthority).
Defined.

(** A publication permit can only be extracted from [PreparationReady].  It
    carries the fully parsed partition and exact checked authorities; a
    refusal contains no authority value and cannot reach downstream mutation. *)
Record MatchPublicationPermit : Type := MatchPublicationPermitState {
  publication_prepared_match : PreparedMatchChanges
}.

Definition preparation_publication_gate
    (outcome : MatchPreparationOutcome) : option MatchPublicationPermit :=
  match outcome with
  | PreparationReady prepared =>
      Some (MatchPublicationPermitState prepared)
  | PreparationRefused _ => None
  end.

Theorem preparation_refusal_has_no_publication_permit : forall input reason,
  prepare_match_changes input = PreparationRefused reason ->
  preparation_publication_gate (prepare_match_changes input) = None.
Proof. intros input reason Hrefused. now rewrite Hrefused. Qed.

Theorem preparation_ready_constructs_checked_observed_pool : forall input prepared,
  prepare_match_changes input = PreparationReady prepared ->
  observed_pool_feasibleb
    (checked_pool_value (prepared_checked_pool prepared))
    (checked_pool_selection (prepared_checked_pool prepared)) = true /\
  reserve_selected_stored
    (checked_pool_selection (prepared_checked_pool prepared)) [] =
    Some (checked_pool_reserved (prepared_checked_pool prepared)).
Proof.
  intros input prepared _. split.
  - exact (checked_pool_feasibility (prepared_checked_pool prepared)).
  - exact (checked_pool_reservations_are_exact (prepared_checked_pool prepared)).
Qed.

Theorem preparation_ready_constructs_executable_commit_liveness : forall
    input prepared,
  prepare_match_changes input = PreparationReady prepared ->
  commit_selection_liveb
    (checked_commit_authority (prepared_checked_commit prepared))
    (checked_pool_selection (prepared_checked_pool prepared)) = true.
Proof.
  intros input prepared _.
  exact (checked_commit_liveness (prepared_checked_commit prepared)).
Qed.

Theorem preparation_ready_has_exact_flat_delta_and_partition : forall
    input prepared,
  prepare_match_changes input = PreparationReady prepared ->
  build_match_flat_delta
    (checked_pool_value (prepared_checked_pool prepared))
    (checked_pool_selection (prepared_checked_pool prepared)) =
      BuildOk (prepared_flat_delta prepared) /\
  partition_flat_delta (prepared_flat_delta prepared) =
      BuildOk (prepared_partitioned_delta prepared) /\
  flat_delta_well_formedb (prepared_flat_delta prepared) = true.
Proof.
  intros input prepared _. repeat split.
  - exact (prepared_delta_is_primary_executable_output prepared).
  - unfold prepared_partitioned_delta.
    exact (fully_parsed_partition_is_exact_flat_partition
      (prepared_partition_permit prepared)).
  - exact (prepared_delta_invariant prepared).
Qed.

Theorem preparation_refusal_is_atomic : forall input reason prepared,
  prepare_match_changes input = PreparationRefused reason ->
  preparation_publication_gate (prepare_match_changes input) <>
    Some (MatchPublicationPermitState prepared).
Proof.
  intros input reason prepared Hrefused.
  rewrite Hrefused. discriminate.
Qed.

Theorem unrelated_partition_channel_is_untouched : forall delta groups channel,
  partition_flat_delta delta = BuildOk groups ->
  lookup_partitioned_delta channel groups = None ->
  ~ In channel (map partitioned_channel_marker groups).
Proof.
  intros delta groups channel _ Hlookup Hin.
  induction groups as [|group tail IH]; simpl in *; [contradiction|].
  destruct Hin as [Heq|Hin].
  - subst channel.
    destruct (path_eq_dec (partitioned_channel_marker group)
      (partitioned_channel_marker group)); [discriminate|contradiction].
  - destruct (path_eq_dec channel (partitioned_channel_marker group));
      [discriminate|]. now apply IH.
Qed.

Theorem loaded_empty_catalog_preserves_loaded_state : forall
    codec observed channel catalog,
  datum_channel_marker_validb channel = true ->
  lookup_channel channel observed = Some [] ->
  build_candidate_catalog codec observed channel = BuildOk catalog ->
  candidate_catalog_channel_was_loaded catalog = true /\
  candidate_catalog_authority catalog = [].
Proof.
  intros codec observed channel catalog Hvalid Hloaded Hbuild.
  unfold build_candidate_catalog in Hbuild. rewrite Hvalid, Hloaded in Hbuild.
  simpl in Hbuild. inversion Hbuild; subst catalog. split; reflexivity.
Qed.

(** Closed capstones used by the proof gate. *)
Print Assumptions constructed_candidate_is_codec_exact.
Print Assumptions constructed_stored_address_is_absolute.
Print Assumptions built_catalog_set_is_requested_local.
Print Assumptions requested_missing_or_loaded_empty_channel_has_explicit_empty_catalog.
Print Assumptions built_catalog_is_sorted_and_duplicate_checked.
Print Assumptions built_catalog_candidates_stay_under_requested_channel.
Print Assumptions flat_record_addition_is_exact_opposite_cancellation.
Print Assumptions checked_pool_replay_exclusion_is_full_multiplicity.
Print Assumptions checked_nonpersistent_stored_reservation_respects_multiplicity.
Print Assumptions checked_persistent_stored_candidate_requires_observed_presence.
Print Assumptions checked_nonpersistent_fresh_is_single_use.
Print Assumptions persistent_fresh_feasibility_has_no_reservation_cap.
Print Assumptions checked_persistent_stored_candidate_is_live.
Print Assumptions checked_nonpersistent_stored_demand_is_live.
Print Assumptions parsed_flat_change_reconstructs_absolute.
Print Assumptions malformed_partition_cannot_construct_publication_evidence.
Print Assumptions fully_parsed_partition_is_exact_flat_partition.
Print Assumptions preparation_refusal_has_no_publication_permit.
Print Assumptions preparation_ready_constructs_checked_observed_pool.
Print Assumptions preparation_ready_constructs_executable_commit_liveness.
Print Assumptions preparation_ready_has_exact_flat_delta_and_partition.
Print Assumptions preparation_refusal_is_atomic.
Print Assumptions unrelated_partition_channel_is_untouched.
Print Assumptions loaded_empty_catalog_preserves_loaded_state.

End TwoLevelRSpaceMatchPreparation.
