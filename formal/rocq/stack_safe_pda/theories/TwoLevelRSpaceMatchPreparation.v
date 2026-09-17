From Stdlib Require Import
  Bool List Arith Lia NArith Sorting.Sorted Sorting.Permutation.
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
  (** Semantic interpretation boundary. Concrete decoder, re-encoder and
      digest correspondence require independent native binding evidence;
      this record alone imposes no laws on its supplied functions. *)
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
| RefuseCommitDelta
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

Theorem parsed_relative_offset_is_exact : forall absolute view,
  parse_absolute_datum absolute = Some view ->
  (length (decoded_view_marker view) <= length absolute)%nat /\
  skipn (length (decoded_view_marker view)) absolute = decoded_view_relative view.
Proof.
  intros absolute view Hparse.
  destruct (@parse_absolute_datum_sound absolute view Hparse) as [_ Hreconstruct].
  change (absolute = decoded_view_marker view ++ decoded_view_relative view)
    in Hreconstruct.
  rewrite Hreconstruct. split.
  - rewrite app_length. lia.
  - rewrite skipn_app, skipn_all, Nat.sub_diag. reflexivity.
Qed.

Lemma exact_suffix_offset : forall (prefix suffix : Path),
  skipn (length (prefix ++ suffix) - length suffix) (prefix ++ suffix) = suffix.
Proof.
  intros prefix suffix. rewrite app_length.
  replace (length prefix + length suffix - length suffix)%nat
    with (length prefix) by lia.
  rewrite skipn_app, skipn_all, Nat.sub_diag. reflexivity.
Qed.

Theorem parsed_identity_offset_is_exact : forall absolute view,
  parse_absolute_datum absolute = Some view ->
  (length (datum_view_datum_identity view) <= length absolute)%nat /\
  skipn (length absolute - length (datum_view_datum_identity view)) absolute =
    datum_view_datum_identity view.
Proof.
  intros absolute view Hparse.
  destruct (@parse_absolute_datum_sound absolute view Hparse) as [_ Hreconstruct].
  rewrite Hreconstruct. unfold absolute_datum_key, relative_datum_key.
  rewrite app_assoc. split.
  - rewrite app_length. lia.
  - apply exact_suffix_offset.
Qed.

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

Record EncodedMatchCandidate : Type := EncodedMatchCandidateState {
  encoded_candidate_address : CandidateAddress;
  encoded_candidate_view : DatumPathView;
  encoded_candidate_digest : Path;
  encoded_candidate_multiplicity : Multiplicity
}.

Definition construct_encoded_candidate_at
    (digest_identity : Path -> Path) (address : CandidateAddress)
    (absolute_path : Path) (multiplicity : Multiplicity)
    : BuildResult EncodedMatchCandidate :=
  match parse_absolute_datum absolute_path with
  | None => BuildRefused (RefuseMalformedStoredPath absolute_path)
  | Some view =>
      let identity := datum_view_datum_identity view in
      let digest := digest_identity identity in
      if digest_width_is_exact digest then
        if multiplicity_valid multiplicity then
          BuildOk (EncodedMatchCandidateState address view digest multiplicity)
        else BuildRefused (RefuseMultiplicity absolute_path)
      else BuildRefused (RefuseDigestWidth identity digest)
  end.

Definition interpret_encoded_candidate
    (codec : CanonicalDatumCodec) (encoded : EncodedMatchCandidate)
    : BuildResult MatchCandidate :=
  let view := encoded_candidate_view encoded in
  let identity := datum_view_datum_identity view in
  match codec_decode_and_reencode_identity codec identity with
  | None => BuildRefused (RefuseNoncanonicalDatumIdentity identity)
  | Some decoded =>
      if path_eqb (decoded_datum_reencoded_identity decoded) identity then
        if path_eqb (decoded_datum_structure decoded)
            (datum_view_datum_structure view) then
          BuildOk (MatchCandidateState (encoded_candidate_address encoded)
            (decoded_view_marker view) (decoded_view_relative view)
            (datum_view_datum_structure view) identity
            (encoded_candidate_digest encoded)
            (decoded_datum_persistent decoded)
            (encoded_candidate_multiplicity encoded))
        else BuildRefused (RefuseDatumStructureMismatch identity)
      else BuildRefused (RefuseNoncanonicalDatumIdentity identity)
  end.

Definition canonical_candidate_admitted
    (codec : CanonicalDatumCodec) (absolute : Path) : Prop :=
  forall view, parse_absolute_datum absolute = Some view ->
    exists decoded,
      codec_decode_and_reencode_identity codec
        (datum_view_datum_identity view) = Some decoded /\
      decoded_datum_reencoded_identity decoded = datum_view_datum_identity view /\
      decoded_datum_structure decoded = datum_view_datum_structure view.

Theorem encoded_candidate_construction_is_decoder_independent : forall
    digest decode_left decode_right address absolute multiplicity,
  construct_encoded_candidate_at
    (codec_digest_raw_identity (CanonicalDatumCodecState decode_left digest))
    address absolute multiplicity =
  construct_encoded_candidate_at
    (codec_digest_raw_identity (CanonicalDatumCodecState decode_right digest))
    address absolute multiplicity.
Proof. reflexivity. Qed.

Theorem admitted_lazy_candidate_interpretation_refines_eager : forall
    codec address absolute multiplicity,
  canonical_candidate_admitted codec absolute ->
  (match construct_encoded_candidate_at (codec_digest_raw_identity codec)
      address absolute multiplicity with
   | BuildOk encoded => interpret_encoded_candidate codec encoded
   | BuildRefused reason => BuildRefused reason
   end) = construct_candidate_at codec address absolute multiplicity.
Proof.
  intros codec address absolute multiplicity Hadmitted.
  unfold construct_encoded_candidate_at, construct_candidate_at.
  destruct (parse_absolute_datum absolute) as [view|] eqn:Hparse;
    [|reflexivity].
  destruct (Hadmitted view Hparse) as [decoded [Hdecode [Hidentity Hstructure]]].
  rewrite Hdecode, Hidentity, Hstructure.
  assert (Hidentityb : path_eqb (datum_view_datum_identity view)
      (datum_view_datum_identity view) = true) by (apply path_eqb_spec; reflexivity).
  assert (Hstructureb : path_eqb (datum_view_datum_structure view)
      (datum_view_datum_structure view) = true) by (apply path_eqb_spec; reflexivity).
  rewrite Hidentityb, Hstructureb.
  destruct (digest_width_is_exact
    (codec_digest_raw_identity codec (datum_view_datum_identity view)));
    [|reflexivity].
  destruct (multiplicity_valid multiplicity); [|reflexivity].
  unfold interpret_encoded_candidate; simpl.
  rewrite Hdecode, Hidentity, Hstructure, Hidentityb, Hstructureb. reflexivity.
Qed.

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

Definition candidate_key_lt (left right : CandidateOrderKey) : Prop :=
  path_lex_lt (fst left) (fst right) \/
  (fst left = fst right /\ path_lex_lt (snd left) (snd right)).

Definition candidate_key_le (left right : CandidateOrderKey) : Prop :=
  candidate_key_lt left right \/ left = right.

Lemma candidate_key_lt_irreflexive : forall key, ~ candidate_key_lt key key.
Proof.
  intros [digest identity] [Hlt|[_ Hlt]];
    now apply (path_lex_lt_irreflexive _) in Hlt.
Qed.

Lemma candidate_key_lt_transitive : forall left middle right,
  candidate_key_lt left middle -> candidate_key_lt middle right ->
  candidate_key_lt left right.
Proof.
  intros [ld li] [md mi] [rd ri]. unfold candidate_key_lt; simpl.
  intros [Hlm|[Hdm Him]] [Hmr|[Hdr Hir]]; subst;
    intuition eauto using path_lex_lt_transitive.
Qed.

Lemma candidate_key_order_total : forall left right,
  candidate_key_lt left right \/ left = right \/ candidate_key_lt right left.
Proof.
  intros [ld li] [rd ri].
  destruct (path_lex_order_total ld rd) as [Hd|[Hlt|Hgt]].
  - subst rd. destruct (path_lex_order_total li ri) as [Hi|[Hlt|Hgt]].
    + subst ri. right. now left.
    + left. right. now split.
    + right. right. right. now split.
  - left. now left.
  - right. right. now left.
Qed.

Lemma candidate_key_le_transitive : forall left middle right,
  candidate_key_le left middle -> candidate_key_le middle right ->
  candidate_key_le left right.
Proof.
  intros left middle right [Hlm|Hlm] [Hmr|Hmr]; subst;
    unfold candidate_key_le; eauto using candidate_key_lt_transitive.
Qed.

Lemma candidate_beforeb_spec : forall left right,
  candidate_beforeb left right = true <->
    candidate_key_lt (candidate_order_key left) (candidate_order_key right).
Proof.
  intros left right. unfold candidate_beforeb, candidate_key_lt,
    candidate_order_key; simpl.
  destruct (path_compare (candidate_digest left) (candidate_digest right))
    eqn:Hcompare.
  - apply path_compare_eq_iff in Hcompare. rewrite Hcompare.
    rewrite path_ltb_spec.
    split; [intro H; right; now split|].
    intros [H|[_ H]]; [now apply path_lex_lt_irreflexive in H|exact H].
  - apply path_compare_lt_iff in Hcompare. split; [intro; now left|reflexivity].
  - apply path_compare_gt_iff in Hcompare. split; [discriminate|].
    intros [H|[Heq _]].
    + exfalso. apply (path_lex_lt_irreflexive (candidate_digest left)).
      eapply path_lex_lt_transitive; eassumption.
    + rewrite Heq in Hcompare. now apply path_lex_lt_irreflexive in Hcompare.
Qed.

Lemma candidate_order_key_eqb_spec : forall left right,
  candidate_order_key_eqb left right = true <->
  candidate_order_key left = candidate_order_key right.
Proof.
  intros left right. unfold candidate_order_key_eqb, candidate_order_key.
  rewrite andb_true_iff, !path_eqb_spec.
  split; [intros [Hd Hi]; now rewrite Hd, Hi|intro H; inversion H; auto].
Qed.

Lemma candidate_before_or_equalb_spec : forall left right,
  candidate_before_or_equalb left right = true <->
  candidate_key_le (candidate_order_key left) (candidate_order_key right).
Proof.
  intros left right. unfold candidate_before_or_equalb, candidate_key_le.
  now rewrite orb_true_iff, candidate_beforeb_spec, candidate_order_key_eqb_spec.
Qed.

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

Lemma insert_candidate_is_permutation : forall candidate ordered,
  Permutation (candidate :: ordered) (insert_candidate candidate ordered).
Proof.
  intros candidate ordered. induction ordered as [|head tail IH]; simpl.
  - apply Permutation_refl.
  - destruct (candidate_beforeb candidate head).
    + apply Permutation_refl.
    + eapply Permutation_trans.
      * apply perm_swap.
      * now apply perm_skip.
Qed.

Theorem sort_candidates_is_permutation : forall candidates,
  Permutation candidates (sort_candidates candidates).
Proof.
  intro candidates. induction candidates as [|head tail IH]; simpl.
  - apply Permutation_refl.
  - eapply Permutation_trans with (l' := head :: sort_candidates tail).
    + apply perm_skip. exact IH.
    + apply insert_candidate_is_permutation.
Qed.

Fixpoint catalog_orderedb (candidates : list MatchCandidate) : bool :=
  match candidates with
  | [] | [_] => true
  | first :: ((second :: _) as tail) =>
      candidate_before_or_equalb first second && catalog_orderedb tail
  end.

Fixpoint catalog_unique_identityb (candidates : list MatchCandidate) : bool :=
  match candidates with
  | [] | [_] => true
  | first :: ((second :: _) as tail) =>
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

Definition candidate_le (left right : MatchCandidate) : Prop :=
  candidate_key_le (candidate_order_key left) (candidate_order_key right).

Lemma insert_candidate_is_sorted : forall candidate ordered,
  StronglySorted candidate_le ordered ->
  StronglySorted candidate_le (insert_candidate candidate ordered).
Proof.
  intros candidate ordered Hsorted.
  induction Hsorted as [|head tail Htail IH Hhead]; simpl.
  - constructor; constructor.
  - destruct (candidate_beforeb candidate head) eqn:Hbefore.
    + apply candidate_beforeb_spec in Hbefore.
      assert (Hle : candidate_le candidate head) by (now left).
      constructor.
      * now constructor.
      * constructor; [exact Hle|].
        eapply Forall_impl; [|exact Hhead].
        intros item Hitem. unfold candidate_le in *.
        eapply candidate_key_le_transitive; eassumption.
    + constructor; [exact IH|].
      apply insert_candidate_preserves_forall; [|exact Hhead].
      unfold candidate_le, candidate_key_le.
      destruct (candidate_key_order_total
        (candidate_order_key candidate) (candidate_order_key head))
        as [Hlt|[Heq|Hgt]].
      * apply candidate_beforeb_spec in Hlt. congruence.
      * right. symmetry. exact Heq.
      * now left.
Qed.

Theorem sort_candidates_is_strongly_sorted : forall candidates,
  StronglySorted candidate_le (sort_candidates candidates).
Proof.
  intro candidates. induction candidates; simpl; [constructor|].
  now apply insert_candidate_is_sorted.
Qed.

Lemma strongly_sorted_candidates_pass_order_check : forall candidates,
  StronglySorted candidate_le candidates -> catalog_orderedb candidates = true.
Proof.
  intros candidates Hsorted.
  induction Hsorted as [|head tail Htail IH Hhead]; [reflexivity|].
  destruct tail as [|next rest]; [reflexivity|].
  change (candidate_before_or_equalb head next &&
    catalog_orderedb (next :: rest) = true).
  rewrite IH, andb_true_r.
  apply candidate_before_or_equalb_spec.
  inversion Hhead. assumption.
Qed.

Theorem candidate_sort_always_passes_order_check : forall candidates,
  catalog_orderedb (sort_candidates candidates) = true.
Proof.
  intro candidates. apply strongly_sorted_candidates_pass_order_check.
  apply sort_candidates_is_strongly_sorted.
Qed.

Definition candidate_lt (left right : MatchCandidate) : Prop :=
  candidate_key_lt (candidate_order_key left) (candidate_order_key right).

Lemma ordered_unique_catalog_is_strictly_sorted : forall candidates,
  catalog_orderedb candidates = true ->
  catalog_unique_identityb candidates = true ->
  StronglySorted candidate_lt candidates.
Proof.
  intro candidates. induction candidates as [|head tail IH];
    intros Hordered Hunique; [constructor|].
  destruct tail as [|next rest]; [constructor; constructor|].
  change (candidate_before_or_equalb head next &&
    catalog_orderedb (next :: rest) = true) in Hordered.
  change (negb (candidate_order_key_eqb head next) &&
    catalog_unique_identityb (next :: rest) = true) in Hunique.
  apply andb_true_iff in Hordered as [Hbefore Hordered].
  apply andb_true_iff in Hunique as [Hdifferent Hunique].
  apply negb_true_iff in Hdifferent.
  unfold candidate_before_or_equalb in Hbefore.
  rewrite Hdifferent, orb_false_r in Hbefore.
  apply candidate_beforeb_spec in Hbefore.
  specialize (IH Hordered Hunique).
  constructor; [exact IH|].
  inversion IH as [|next' rest' Hrest Hnext]; subst.
  constructor; [exact Hbefore|].
  eapply Forall_impl; [|exact Hnext].
  intros item Hitem. unfold candidate_lt in *.
  eapply candidate_key_lt_transitive; eassumption.
Qed.

Definition catalog_digest_coherent
    (digest : Path -> Path) (candidates : list MatchCandidate) : Prop :=
  Forall (fun candidate => candidate_digest candidate =
    digest (candidate_canonical_identity candidate)) candidates.

Lemma strictly_sorted_coherent_catalog_has_unique_identities : forall
    digest candidates,
  StronglySorted candidate_lt candidates ->
  catalog_digest_coherent digest candidates ->
  NoDup (map candidate_canonical_identity candidates).
Proof.
  intros digest candidates Hsorted.
  induction Hsorted as [|head tail Htail IH Hhead]; intros Hcoherent;
    simpl; [constructor|].
  inversion Hcoherent as [|head' tail' Hhd Htl]; subst.
  constructor.
  - intro Hin. apply in_map_iff in Hin as [item [Hidentity Hin]].
    rewrite Forall_forall in Hhead.
    pose proof (Hhead item Hin) as Hlt.
    rewrite Forall_forall in Htl.
    specialize (Htl item Hin).
    unfold candidate_lt, candidate_order_key in Hlt.
    rewrite Hhd, Htl, Hidentity in Hlt.
    now apply candidate_key_lt_irreflexive in Hlt.
  - apply IH. exact Htl.
Qed.

Theorem ordered_coherent_duplicate_check_implies_global_identity_uniqueness :
  forall digest candidates,
  catalog_orderedb candidates = true ->
  catalog_unique_identityb candidates = true ->
  catalog_digest_coherent digest candidates ->
  NoDup (map candidate_canonical_identity candidates).
Proof.
  intros digest candidates Hordered Hunique Hcoherent.
  eapply strictly_sorted_coherent_catalog_has_unique_identities;
    [now apply ordered_unique_catalog_is_strictly_sorted|exact Hcoherent].
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

Theorem candidate_catalog_depends_only_on_requested_bag : forall
    codec left right channel,
  lookup_channel channel left = lookup_channel channel right ->
  build_candidate_catalog codec left channel =
    build_candidate_catalog codec right channel.
Proof.
  intros codec left right channel Hlookup.
  unfold build_candidate_catalog. now rewrite Hlookup.
Qed.

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

Lemma deduplicated_requested_channel_was_requested : forall requested channel,
  In channel (deduplicate_requested_channels requested) -> In channel requested.
Proof.
  intro requested. induction requested as [|head tail IH]; intros channel Hin;
    simpl in *; [contradiction|].
  destruct (path_memberb head (deduplicate_requested_channels tail)).
  - right. now apply IH.
  - destruct Hin as [Heq|Hin]; [now left|right; now apply IH].
Qed.

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

Lemma catalog_list_depends_only_on_requested_bags : forall
    codec channels left right,
  (forall channel, In channel channels ->
    lookup_channel channel left = lookup_channel channel right) ->
  build_catalog_list codec left channels = build_catalog_list codec right channels.
Proof.
  intros codec channels. induction channels as [|head tail IH];
    intros left right Hagree; simpl; [reflexivity|].
  rewrite (@candidate_catalog_depends_only_on_requested_bag
    codec left right head (Hagree head (or_introl eq_refl))).
  rewrite (IH left right).
  - reflexivity.
  - intros channel Hin. apply Hagree. now right.
Qed.

Theorem catalog_set_ignores_unrequested_authority : forall
    codec requested left right,
  (forall channel, In channel requested ->
    lookup_channel channel left = lookup_channel channel right) ->
  build_candidate_catalog_set codec left requested =
    build_candidate_catalog_set codec right requested.
Proof.
  intros codec requested left right Hagree.
  unfold build_candidate_catalog_set.
  rewrite (@catalog_list_depends_only_on_requested_bags
    codec (deduplicate_requested_channels requested) left right).
  - reflexivity.
  - intros channel Hin. apply Hagree.
    now apply deduplicated_requested_channel_was_requested.
Qed.

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

Lemma flat_set_count_other : forall bag path count other,
  path <> other ->
  flat_lookup other (flat_set_count path count bag) = flat_lookup other bag.
Proof.
  intros bag. induction bag as [|[current value] tail IH];
    intros path count other Hdifferent; simpl.
  - destruct (N.eqb count 0); simpl; [reflexivity|].
    destruct (path_eq_dec other path); congruence.
  - destruct (path_eq_dec path current) as [Hequal|Hnot_current].
    + subst current. destruct (N.eqb count 0); simpl;
        destruct (path_eq_dec other path); congruence.
    + simpl. destruct (path_eq_dec other current); [reflexivity|].
      now apply IH.
Qed.

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

Theorem flat_record_removal_is_exact_opposite_cancellation : forall
    delta path amount additions removals,
  flat_paths_unique_from [] (flat_delta_additions delta) = true ->
  flat_paths_unique_from [] (flat_delta_removals delta) = true ->
  flat_record_removal delta path amount =
      Some (FlatBagDeltaState additions removals) ->
  let cancelled := N.min (flat_lookup path (flat_delta_additions delta)) amount in
  flat_lookup path additions =
      flat_lookup path (flat_delta_additions delta) - cancelled /\
  flat_lookup path removals =
      flat_lookup path (flat_delta_removals delta) + (amount - cancelled).
Proof.
  intros [before_additions before_removals] path amount additions removals
    Hadd_unique Hremove_unique Hrecord.
  apply (@flat_record_addition_is_exact_opposite_cancellation
    (FlatBagDeltaState before_removals before_additions)
    path amount removals additions Hremove_unique Hadd_unique).
  unfold flat_record_addition, flat_record_removal in *. simpl in *.
  destruct (flat_record_change before_removals before_additions path amount)
    as [[after_removals after_additions]|]; inversion Hrecord; reflexivity.
Qed.

Lemma flat_record_change_preserves_other : forall
    destination cancellation path amount after_destination after_cancellation other,
  path <> other ->
  flat_record_change destination cancellation path amount =
    Some (after_destination, after_cancellation) ->
  flat_lookup other after_destination = flat_lookup other destination /\
  flat_lookup other after_cancellation = flat_lookup other cancellation.
Proof.
  intros destination cancellation path amount after_destination after_cancellation
    other Hdifferent Hrecord.
  unfold flat_record_change, checked_flat_add in Hrecord.
  destruct (path_valid path && multiplicity_valid amount); [|discriminate].
  destruct (N.leb
    (flat_lookup path destination + (amount - N.min (flat_lookup path cancellation) amount))
    multiplicity_max); inversion Hrecord; subst.
  unfold flat_subtract. split; now apply flat_set_count_other.
Qed.

Theorem flat_record_addition_preserves_other : forall delta path amount after other,
  path <> other ->
  flat_record_addition delta path amount = Some after ->
  flat_lookup other (flat_delta_additions after) =
    flat_lookup other (flat_delta_additions delta) /\
  flat_lookup other (flat_delta_removals after) =
    flat_lookup other (flat_delta_removals delta).
Proof.
  intros delta path amount after other Hdifferent Hrecord.
  unfold flat_record_addition in Hrecord.
  destruct (flat_record_change (flat_delta_additions delta)
    (flat_delta_removals delta) path amount) as [[additions removals]|]
    eqn:Hchange; [|discriminate].
  inversion Hrecord; subst after; simpl.
  now apply (@flat_record_change_preserves_other
    (flat_delta_additions delta) (flat_delta_removals delta)
    path amount additions removals other Hdifferent Hchange).
Qed.

Theorem flat_record_removal_preserves_other : forall delta path amount after other,
  path <> other ->
  flat_record_removal delta path amount = Some after ->
  flat_lookup other (flat_delta_additions after) =
    flat_lookup other (flat_delta_additions delta) /\
  flat_lookup other (flat_delta_removals after) =
    flat_lookup other (flat_delta_removals delta).
Proof.
  intros delta path amount after other Hdifferent Hrecord.
  unfold flat_record_removal in Hrecord.
  destruct (flat_record_change (flat_delta_removals delta)
    (flat_delta_additions delta) path amount) as [[removals additions]|]
    eqn:Hchange; [|discriminate].
  inversion Hrecord; subst after; simpl.
  pose proof (@flat_record_change_preserves_other
    (flat_delta_removals delta) (flat_delta_additions delta)
    path amount removals additions other Hdifferent Hchange) as [Hremove Hadd].
  now split.
Qed.

Lemma flat_set_count_key_membership : forall bag path count other,
  In other (map fst (flat_set_count path count bag)) ->
  other = path \/ In other (map fst bag).
Proof.
  intros bag. induction bag as [|[current value] tail IH];
    intros path count other Hin; simpl in *.
  - destruct (N.eqb count 0); simpl in Hin; intuition congruence.
  - destruct (path_eq_dec path current) as [Hequal|Hdifferent].
    + subst current. destruct (N.eqb count 0); simpl in Hin;
        intuition congruence.
    + simpl in Hin. destruct Hin as [Hequal|Hin].
      * right. now left.
      * destruct (IH path count other Hin) as [Hequal|Htail].
        -- now left.
        -- right. now right.
Qed.

Lemma flat_set_count_preserves_unique : forall bag path count,
  NoDup (map fst bag) -> NoDup (map fst (flat_set_count path count bag)).
Proof.
  intros bag. induction bag as [|[current value] tail IH];
    intros path count Hunique; simpl in *.
  - destruct (N.eqb count 0); simpl; repeat constructor; simpl; tauto.
  - inversion Hunique as [|current' keys Habsent Htail]; subst.
    destruct (path_eq_dec path current) as [Hequal|Hdifferent].
    + subst current. destruct (N.eqb count 0); simpl; [exact Htail|].
      now constructor.
    + simpl. constructor.
      * intro Hin. apply flat_set_count_key_membership in Hin.
        destruct Hin as [Hequal|Hin]; congruence.
      * now apply IH.
Qed.

Lemma nodup_implies_flat_paths_unique : forall bag seen,
  NoDup (map fst bag) ->
  (forall path, In path seen -> ~ In path (map fst bag)) ->
  flat_paths_unique_from seen bag = true.
Proof.
  intros bag. induction bag as [|[current value] tail IH];
    intros seen Hunique Hdisjoint; simpl in *; [reflexivity|].
  inversion Hunique as [|current' keys Habsent Htail]; subst.
  apply andb_true_iff. split.
  - apply negb_true_iff, path_memberb_false_spec.
    intro Hin. apply (Hdisjoint current Hin). now left.
  - apply IH; [exact Htail|].
    intros path [Hequal|Hin] Hmember.
    + subst path. contradiction.
    + apply (Hdisjoint path Hin). now right.
Qed.

Lemma flat_set_count_preserves_valid_entries : forall bag path count,
  forallb (fun entry => path_valid (fst entry) && multiplicity_valid (snd entry)) bag = true ->
  path_valid path = true -> count <= multiplicity_max ->
  forallb (fun entry => path_valid (fst entry) && multiplicity_valid (snd entry))
    (flat_set_count path count bag) = true.
Proof.
  intros bag path count Hvalid Hpath Hbound.
  assert (Hcount : count <> 0 -> multiplicity_valid count = true).
  { intro Hpositive. unfold multiplicity_valid.
    apply andb_true_iff. split; [apply N.ltb_lt|apply N.leb_le]; lia. }
  induction bag as [|[current value] tail IH]; simpl in *.
  - destruct (N.eqb count 0) eqn:Hzero; [reflexivity|].
    simpl. rewrite Hpath, Hcount; [reflexivity|now apply N.eqb_neq].
  - apply andb_true_iff in Hvalid as [Hhead Htail].
    destruct (path_eq_dec path current) as [Hequal|Hdifferent].
    + subst current. destruct (N.eqb count 0) eqn:Hzero; [exact Htail|].
      simpl. rewrite Hpath, Hcount, Htail; [reflexivity|now apply N.eqb_neq].
    + simpl. rewrite Hhead, IH; [reflexivity|exact Htail].
Qed.

Lemma flat_set_count_preserves_well_formed : forall bag path count,
  flat_bag_well_formedb bag = true ->
  path_valid path = true -> count <= multiplicity_max ->
  flat_bag_well_formedb (flat_set_count path count bag) = true.
Proof.
  intros bag path count Hwellformed Hpath Hbound.
  unfold flat_bag_well_formedb in *. apply andb_true_iff in Hwellformed as [Hvalid Hunique].
  apply andb_true_iff. split.
  - now apply flat_set_count_preserves_valid_entries.
  - apply nodup_implies_flat_paths_unique.
    + apply flat_set_count_preserves_unique. now apply flat_paths_unique_implies_nodup.
    + intros other Hin. contradiction.
Qed.

Lemma flat_lookup_well_formed_bound : forall bag path,
  flat_bag_well_formedb bag = true -> flat_lookup path bag <= multiplicity_max.
Proof.
  intros bag path Hwellformed.
  apply andb_true_iff in Hwellformed as [Hvalid _].
  induction bag as [|[current value] tail IH]; simpl in *; [lia|].
  apply andb_true_iff in Hvalid as [Hhead Htail].
  destruct (path_eq_dec path current); [|now apply IH].
  apply andb_true_iff in Hhead as [_ Hvalue].
  unfold multiplicity_valid in Hvalue.
  apply andb_true_iff in Hvalue as [_ Hbound]. now apply N.leb_le.
Qed.

Lemma flat_record_change_preserves_well_formed : forall
    destination cancellation path amount after_destination after_cancellation,
  flat_bag_well_formedb destination = true ->
  flat_bag_well_formedb cancellation = true ->
  flat_record_change destination cancellation path amount =
    Some (after_destination, after_cancellation) ->
  flat_bag_well_formedb after_destination = true /\
  flat_bag_well_formedb after_cancellation = true.
Proof.
  intros destination cancellation path amount after_destination after_cancellation
    Hdestination Hcancellation Hrecord.
  unfold flat_record_change, checked_flat_add in Hrecord.
  destruct (path_valid path && multiplicity_valid amount) eqn:Hvalid; [|discriminate].
  apply andb_true_iff in Hvalid as [Hpath Hamount].
  destruct (N.leb
    (flat_lookup path destination + (amount - N.min (flat_lookup path cancellation) amount))
    multiplicity_max) eqn:Hbound; [|discriminate].
  inversion Hrecord; subst. split.
  - apply flat_set_count_preserves_well_formed; auto. now apply N.leb_le.
  - unfold flat_subtract. apply flat_set_count_preserves_well_formed; auto.
    pose proof (@flat_lookup_well_formed_bound cancellation path Hcancellation). lia.
Qed.

Lemma flat_lookup_member_value : forall bag path amount,
  NoDup (map fst bag) -> In (path, amount) bag -> flat_lookup path bag = amount.
Proof.
  intros bag. induction bag as [|[current value] tail IH];
    intros path amount Hunique Hin; simpl in *; [contradiction|].
  inversion Hunique as [|head keys Habsent Htail]; subst.
  destruct Hin as [Hequal|Hin].
  - inversion Hequal; subst. destruct (path_eq_dec path path); congruence.
  - destruct (path_eq_dec path current) as [Hequal|Hdifferent].
    + subst current. exfalso. apply Habsent. now apply in_map with (f := fst) in Hin.
    + now apply IH.
Qed.

Lemma flat_lookup_nonzero_member : forall bag path,
  flat_lookup path bag <> 0 -> In path (map fst bag).
Proof.
  intros bag. induction bag as [|[current value] tail IH];
    intros path Hnonzero; simpl in *; [contradiction|].
  destruct (path_eq_dec path current) as [Hequal|Hdifferent].
  - left. symmetry. exact Hequal.
  - right. now apply IH.
Qed.

Lemma flat_disjoint_is_pointwise : forall additions removals,
  flat_bag_well_formedb additions = true ->
  flat_delta_disjointb (FlatBagDeltaState additions removals) = true <->
  forall path, flat_lookup path additions = 0 \/ flat_lookup path removals = 0.
Proof.
  intros additions removals Hwellformed.
  unfold flat_delta_disjointb; simpl. rewrite forallb_forall.
  split.
  - intros Hdisjoint path. destruct (N.eq_dec (flat_lookup path additions) 0)
      as [Hzero|Hnonzero]; [now left|right].
    apply flat_lookup_nonzero_member in Hnonzero.
    apply in_map_iff in Hnonzero as [[current value] [Hequal Hin]].
    simpl in Hequal. subst current.
    apply N.eqb_eq. exact (Hdisjoint (path, value) Hin).
  - intros Hdisjoint [path value] Hin; simpl.
    destruct (Hdisjoint path) as [Hzero|Hzero]; [|now apply N.eqb_eq].
    apply andb_true_iff in Hwellformed as [Hvalid Hunique].
    pose proof (@flat_lookup_member_value additions path value
      (@flat_paths_unique_implies_nodup additions Hunique) Hin) as Hlookup.
    rewrite Hlookup in Hzero. subst value.
    apply forallb_forall with (x := (path, 0)) in Hvalid; [|exact Hin].
    cbn [multiplicity_valid] in Hvalid.
    rewrite andb_false_r in Hvalid. discriminate.
Qed.

Lemma flat_record_change_preserves_disjoint : forall
    destination cancellation path amount after_destination after_cancellation,
  flat_bag_well_formedb destination = true ->
  flat_bag_well_formedb cancellation = true ->
  (forall other, flat_lookup other destination = 0 \/ flat_lookup other cancellation = 0) ->
  flat_record_change destination cancellation path amount =
    Some (after_destination, after_cancellation) ->
  forall other, flat_lookup other after_destination = 0 \/
    flat_lookup other after_cancellation = 0.
Proof.
  intros destination cancellation path amount after_destination after_cancellation
    Hdestination Hcancellation Hdisjoint Hrecord other.
  destruct (path_eq_dec path other) as [Hequal|Hdifferent].
  - subst other.
    assert (Hadd : flat_record_addition (FlatBagDeltaState destination cancellation)
        path amount = Some (FlatBagDeltaState after_destination after_cancellation)).
    { unfold flat_record_addition. simpl. now rewrite Hrecord. }
    apply andb_true_iff in Hdestination as [_ Hdestination].
    apply andb_true_iff in Hcancellation as [_ Hcancellation].
    pose proof (@flat_record_addition_is_exact_opposite_cancellation
      (FlatBagDeltaState destination cancellation) path amount
      after_destination after_cancellation Hdestination Hcancellation Hadd)
      as [Hcancel Hdest]. simpl in Hcancel, Hdest.
    rewrite Hcancel, Hdest.
    destruct (Hdisjoint path) as [Hzero|Hzero].
    + rewrite Hzero.
      destruct (N.le_ge_cases (flat_lookup path cancellation) amount) as [Hle|Hge].
      * right. rewrite N.min_l by exact Hle. lia.
      * left. rewrite N.min_r by exact Hge. lia.
    + right. rewrite Hzero. reflexivity.
  - pose proof (@flat_record_change_preserves_other destination cancellation path amount
      after_destination after_cancellation other Hdifferent Hrecord) as [Hdest Hcancel].
    rewrite Hdest, Hcancel. apply Hdisjoint.
Qed.

Theorem flat_record_addition_preserves_normalization : forall delta path amount after,
  flat_delta_well_formedb delta = true ->
  flat_record_addition delta path amount = Some after ->
  flat_delta_well_formedb after = true.
Proof.
  intros [additions removals] path amount after Hnormalized Hrecord.
  unfold flat_delta_well_formedb in Hnormalized.
  apply andb_true_iff in Hnormalized as [Hbags Hdisjoint].
  apply andb_true_iff in Hbags as [Hadd Hremove]. simpl in *.
  unfold flat_record_addition in Hrecord. simpl in Hrecord.
  destruct (flat_record_change additions removals path amount)
    as [[after_additions after_removals]|] eqn:Hchange; [|discriminate].
  inversion Hrecord; subst after.
  pose proof (@flat_record_change_preserves_well_formed additions removals path amount
    after_additions after_removals Hadd Hremove Hchange) as [Hafter_add Hafter_remove].
  unfold flat_delta_well_formedb; simpl. rewrite Hafter_add, Hafter_remove; simpl.
  apply (proj2 (@flat_disjoint_is_pointwise after_additions after_removals Hafter_add)).
  apply (@flat_record_change_preserves_disjoint additions removals path amount
    after_additions after_removals Hadd Hremove); [|exact Hchange].
  now apply (proj1 (@flat_disjoint_is_pointwise additions removals Hadd)).
Qed.

Theorem flat_record_removal_preserves_normalization : forall delta path amount after,
  flat_delta_well_formedb delta = true ->
  flat_record_removal delta path amount = Some after ->
  flat_delta_well_formedb after = true.
Proof.
  intros [additions removals] path amount after Hnormalized Hrecord.
  unfold flat_delta_well_formedb in Hnormalized.
  apply andb_true_iff in Hnormalized as [Hbags Hdisjoint].
  apply andb_true_iff in Hbags as [Hadd Hremove]. simpl in *.
  unfold flat_record_removal in Hrecord. simpl in Hrecord.
  destruct (flat_record_change removals additions path amount)
    as [[after_removals after_additions]|] eqn:Hchange; [|discriminate].
  inversion Hrecord; subst after.
  pose proof (@flat_record_change_preserves_well_formed removals additions path amount
    after_removals after_additions Hremove Hadd Hchange) as [Hafter_remove Hafter_add].
  unfold flat_delta_well_formedb; simpl. rewrite Hafter_add, Hafter_remove; simpl.
  apply (proj2 (@flat_disjoint_is_pointwise after_additions after_removals Hafter_add)).
  assert (Hswapped : forall other,
    flat_lookup other removals = 0 \/ flat_lookup other additions = 0).
  { intro other. apply (proj1 (@flat_disjoint_is_pointwise additions removals Hadd))
      with (path := other) in Hdisjoint. tauto. }
  intro other.
  pose proof (@flat_record_change_preserves_disjoint removals additions path amount
    after_removals after_additions Hremove Hadd Hswapped Hchange other). tauto.
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

Lemma absent_fresh_flag_does_not_change_authorization : forall
    observed requested catalogs excluded first second candidate,
  candidate_authorizedb (CandidatePoolState observed requested catalogs None first excluded) candidate =
  candidate_authorizedb (CandidatePoolState observed requested catalogs None second excluded) candidate.
Proof.
  intros observed requested catalogs excluded first second candidate.
  unfold candidate_authorizedb.
  destruct (candidate_address candidate); simpl; [now rewrite !andb_false_r|reflexivity].
Qed.

Theorem absent_fresh_flag_does_not_change_feasibility : forall
    observed requested catalogs excluded first second selected,
  observed_pool_feasibleb (CandidatePoolState observed requested catalogs None first excluded) selected =
  observed_pool_feasibleb (CandidatePoolState observed requested catalogs None second excluded) selected.
Proof.
  intros observed requested catalogs excluded first second selected.
  unfold observed_pool_feasibleb. simpl. f_equal.
  assert (Hext : forall (A : Type) (f g : A -> bool) values,
    (forall value, f value = g value) -> forallb f values = forallb g values).
  { intros A f g values Hsame. induction values; simpl; [reflexivity|].
    now rewrite Hsame, IHvalues. }
  apply Hext. intro candidate. unfold selected_candidate_feasibleb.
  rewrite (@absent_fresh_flag_does_not_change_authorization observed requested catalogs
    excluded first second candidate). reflexivity.
Qed.

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
  | (first, _) :: (((second, _) :: _) as tail) =>
      path_ltb first second && entries_orderedb tail
  end.

Definition channel_entry_well_formedb (entry : ChannelEntry) : bool :=
  path_valid (channel_path entry) &&
  flat_bag_well_formedb (channel_bag entry) &&
  entries_orderedb (channel_bag entry).

Fixpoint channels_orderedb (authority : TwoLevelAuthority) : bool :=
  match authority with
  | [] | [_] => true
  | (first, _) :: (((second, _) :: _) as tail) =>
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

Lemma built_candidate_pool_input_projections : forall
    codec observed requested fresh_absolute fresh_available excluded pool,
  build_candidate_pool codec observed requested fresh_absolute fresh_available excluded =
    BuildOk pool ->
  pool_observed_authority pool = observed /\ pool_requested_channels pool = requested.
Proof.
  intros codec observed requested fresh_absolute fresh_available excluded pool Hbuild.
  unfold build_candidate_pool in Hbuild.
  destruct (build_candidate_catalog_set codec observed requested)
    as [catalogs|reason] eqn:Hcatalogs; [|discriminate].
  destruct fresh_absolute as [absolute|].
  - destruct (construct_fresh_candidate codec absolute) as [fresh|reason] eqn:Hfresh;
      [|discriminate].
    destruct (replay_exclusions_validb catalogs excluded) eqn:Hexcluded.
    + inversion Hbuild; auto.
    + destruct excluded as [|[path count] rest]; discriminate.
  - destruct (replay_exclusions_validb catalogs excluded) eqn:Hexcluded.
    + inversion Hbuild; auto.
    + destruct excluded as [|[path count] rest]; discriminate.
Qed.

Definition certify_candidate_pool codec observed requested fresh available excluded pool
    (Hbuild : build_candidate_pool codec observed requested fresh available excluded = BuildOk pool)
    (Hobserved : authority_well_formedb observed = true) selected reserved
    (Hfeasible : observed_pool_feasibleb pool selected = true)
    (Hreserved : reserve_selected_stored selected [] = Some reserved) : CheckedObservedPool.
Proof.
  refine {| checked_pool_codec := codec;
    checked_pool_value := pool;
    checked_pool_observed_authority_well_formed := _;
    checked_pool_selection := selected;
    checked_pool_reserved := reserved;
    checked_pool_fresh_reserved := negb (N.eqb (nonpersistent_fresh_demand selected) 0);
    checked_pool_catalog_construction := _;
    checked_pool_replay_exclusions_are_full_or_none := _;
    checked_pool_fresh_codec_is_exact := _;
    checked_pool_feasibility := Hfeasible;
    checked_pool_reservations_are_exact := Hreserved;
    checked_pool_fresh_reservation_is_exact := eq_refl |}.
  - rewrite (proj1 (@built_candidate_pool_input_projections _ _ _ _ _ _ _ Hbuild)).
    exact Hobserved.
  - rewrite (proj1 (@built_candidate_pool_input_projections _ _ _ _ _ _ _ Hbuild)),
      (proj2 (@built_candidate_pool_input_projections _ _ _ _ _ _ _ Hbuild)).
    exact (proj1 (@built_candidate_pool_is_exact _ _ _ _ _ _ _ Hbuild)).
  - exact (proj1 (proj2 (@built_candidate_pool_is_exact _ _ _ _ _ _ _ Hbuild))).
  - exact (proj2 (proj2 (@built_candidate_pool_is_exact _ _ _ _ _ _ _ Hbuild))).
Defined.

Definition check_observed_pool_with_inputs
    (codec : CanonicalDatumCodec) (observed : TwoLevelAuthority)
    (observed_well_formed : authority_well_formedb observed = true)
    (requested : list Path) (fresh_absolute : option Path)
    (fresh_available : bool) (excluded : FlatBag)
    (selected : list MatchCandidate) : BuildResult
      { checked : CheckedObservedPool |
        checked_pool_selection checked = selected /\
        pool_observed_authority (checked_pool_value checked) = observed }.
Proof.
  destruct (build_candidate_pool codec observed requested fresh_absolute
    fresh_available excluded) as [pool|reason] eqn:Hpool.
  - destruct (observed_pool_feasibleb pool selected) eqn:Hfeasible.
    + destruct (reserve_selected_stored selected []) as [reserved|] eqn:Hreserved.
      * refine (BuildOk (exist _ (@certify_candidate_pool codec observed requested fresh_absolute
          fresh_available excluded pool Hpool observed_well_formed selected reserved
          Hfeasible Hreserved) _)).
        split; [reflexivity|].
        exact (proj1 (@built_candidate_pool_input_projections _ _ _ _ _ _ _ Hpool)).
      * exact (BuildRefused (RefuseReservationOverflow [])).
    + exact (BuildRefused RefuseObservedPoolFeasibility).
  - exact (BuildRefused reason).
Defined.

Definition check_observed_pool codec observed Hwell requested fresh available excluded selected
    : BuildResult CheckedObservedPool :=
  match check_observed_pool_with_inputs codec observed Hwell requested fresh available excluded selected with
  | BuildOk checked => BuildOk (proj1_sig checked)
  | BuildRefused reason => BuildRefused reason
  end.

Lemma checked_pool_matches_supplied_input : forall
    codec observed Hwell requested fresh available excluded selected checked,
  check_observed_pool codec observed Hwell requested fresh available excluded selected =
    BuildOk checked ->
  checked_pool_selection checked = selected /\
  pool_observed_authority (checked_pool_value checked) = observed.
Proof.
  intros codec observed Hwell requested fresh available excluded selected checked Hchecked.
  unfold check_observed_pool in Hchecked.
  destruct (check_observed_pool_with_inputs codec observed Hwell requested fresh
    available excluded selected) as [[value Hinputs]|reason]; [|discriminate].
  inversion Hchecked; subst checked. exact Hinputs.
Qed.

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

Definition partition_side (polarity : ChangePolarity) (delta : FlatBagDelta) : FlatBag :=
  match polarity with
  | ChangeAddition => flat_delta_additions delta
  | ChangeRemoval => flat_delta_removals delta
  end.

Definition partition_record (polarity : ChangePolarity) (delta : FlatBagDelta) :=
  match polarity with
  | ChangeAddition => flat_record_addition delta
  | ChangeRemoval => flat_record_removal delta
  end.

Definition partition_polarity_eq_dec : forall left right : ChangePolarity,
    {left = right} + {left <> right}.
Proof. decide equality. Defined.

Definition parsed_pair (change : ParsedFlatChange) : Path * Path :=
  (parsed_change_marker change, parsed_change_relative change).

Definition partition_pair_eq_dec : forall left right : Path * Path,
    {left = right} + {left <> right}.
Proof. decide equality; apply path_eq_dec. Defined.

Definition change_contribution
    (polarity : ChangePolarity) (marker relative : Path)
    (change : ParsedFlatChange) : N :=
  if partition_pair_eq_dec (marker, relative) (parsed_pair change) then
    if partition_polarity_eq_dec polarity (parsed_change_polarity change)
    then parsed_change_amount change else 0
  else 0.

Fixpoint independent_change_mass
    (polarity : ChangePolarity) (marker relative : Path)
    (changes : list ParsedFlatChange) : N :=
  match changes with
  | [] => 0
  | change :: tail => change_contribution polarity marker relative change +
      independent_change_mass polarity marker relative tail
  end.

Definition grouped_count
    (polarity : ChangePolarity) (marker relative : Path)
    (groups : list PartitionedChannelDelta) : N :=
  match lookup_partitioned_delta marker groups with
  | None => 0
  | Some delta => flat_lookup relative (partition_side polarity delta)
  end.

Definition partition_groups_valid (groups : list PartitionedChannelDelta) : Prop :=
  NoDup (map partitioned_channel_marker groups) /\
  Forall (fun group => flat_delta_well_formedb (partitioned_channel_delta group) = true) groups.

Definition pending_pair_fresh
    (groups : list PartitionedChannelDelta) (change : ParsedFlatChange) : Prop :=
  forall polarity, grouped_count polarity (parsed_change_marker change)
    (parsed_change_relative change) groups = 0.

Definition parsed_change_valid (change : ParsedFlatChange) : Prop :=
  path_valid (parsed_change_relative change) = true /\
  multiplicity_valid (parsed_change_amount change) = true.

Lemma partition_side_well_formed : forall delta polarity,
  flat_delta_well_formedb delta = true ->
  flat_bag_well_formedb (partition_side polarity delta) = true.
Proof.
  intros delta polarity Hvalid. unfold flat_delta_well_formedb in Hvalid.
  apply andb_true_iff in Hvalid as [Hbags _].
  apply andb_true_iff in Hbags as [Hadd Hremove]. destruct polarity; assumption.
Qed.

Lemma partition_record_normalized : forall polarity delta path amount after,
  flat_delta_well_formedb delta = true ->
  partition_record polarity delta path amount = Some after ->
  flat_delta_well_formedb after = true.
Proof.
  intros polarity. destruct polarity; [apply flat_record_addition_preserves_normalization|
    apply flat_record_removal_preserves_normalization].
Qed.

Lemma partition_empty_count : forall polarity path,
  flat_lookup path (partition_side polarity empty_flat_delta) = 0.
Proof. intros polarity path. destruct polarity; reflexivity. Qed.

Lemma partition_record_fresh_exists : forall polarity delta path amount,
  path_valid path = true -> multiplicity_valid amount = true ->
  flat_lookup path (flat_delta_additions delta) = 0 ->
  flat_lookup path (flat_delta_removals delta) = 0 ->
  exists after, partition_record polarity delta path amount = Some after.
Proof.
  intros polarity delta path amount Hpath Hamount Hadd Hremove.
  assert (Hbound : N.leb amount multiplicity_max = true).
  { unfold multiplicity_valid in Hamount. now apply andb_true_iff in Hamount as [_ Hamount]. }
  destruct polarity; unfold partition_record, flat_record_addition, flat_record_removal,
    flat_record_change, checked_flat_add;
    rewrite Hpath, Hamount, Hadd, Hremove, N.min_0_l, N.sub_0_r, N.add_0_l, Hbound;
    eexists; reflexivity.
Qed.

Lemma partition_record_fresh_observation : forall written delta path amount after,
  flat_delta_well_formedb delta = true ->
  (forall polarity, flat_lookup path (partition_side polarity delta) = 0) ->
  partition_record written delta path amount = Some after ->
  forall polarity other,
  flat_lookup other (partition_side polarity after) =
    flat_lookup other (partition_side polarity delta) +
      (if path_eq_dec other path then
         if partition_polarity_eq_dec polarity written then amount else 0
       else 0).
Proof.
  intros written delta path amount after Hvalid Hfresh Hrecord polarity other.
  pose proof (Hfresh ChangeAddition) as Hzadd.
  pose proof (Hfresh ChangeRemoval) as Hzremove.
  cbn [partition_side] in Hzadd, Hzremove.
  destruct (path_eq_dec other path) as [Hequal|Hdifferent].
  - subst other.
    pose proof (@partition_side_well_formed delta ChangeAddition Hvalid) as Hadd.
    pose proof (@partition_side_well_formed delta ChangeRemoval Hvalid) as Hremove.
    apply andb_true_iff in Hadd as [_ Hadd].
    apply andb_true_iff in Hremove as [_ Hremove].
    destruct after as [additions removals].
    destruct written.
    + pose proof (@flat_record_addition_is_exact_opposite_cancellation
        delta path amount additions removals Hadd Hremove Hrecord) as [Hr Ha].
      specialize (Hfresh ChangeRemoval) as Hzero.
      change (flat_lookup path (flat_delta_removals delta) = 0) in Hzero.
      rewrite Hzero in Hr, Ha. simpl in Hr, Ha.
      destruct polarity; cbn [partition_side flat_delta_additions flat_delta_removals];
        repeat match goal with
        | |- context [partition_polarity_eq_dec ?left ?right] =>
            destruct (partition_polarity_eq_dec left right); try congruence
        end; rewrite ?Ha, ?Hr, ?Hzadd, ?Hzremove; lia.
    + pose proof (@flat_record_removal_is_exact_opposite_cancellation
        delta path amount additions removals Hadd Hremove Hrecord) as [Ha Hr].
      specialize (Hfresh ChangeAddition) as Hzero.
      change (flat_lookup path (flat_delta_additions delta) = 0) in Hzero.
      rewrite Hzero in Hr, Ha. simpl in Hr, Ha.
      destruct polarity; cbn [partition_side flat_delta_additions flat_delta_removals];
        repeat match goal with
        | |- context [partition_polarity_eq_dec ?left ?right] =>
            destruct (partition_polarity_eq_dec left right); try congruence
        end; rewrite ?Ha, ?Hr, ?Hzadd, ?Hzremove; lia.
  - assert (Hneq : path <> other) by congruence.
    destruct written.
    + pose proof (@flat_record_addition_preserves_other delta path amount after other Hneq Hrecord).
      destruct polarity; simpl in *; lia.
    + pose proof (@flat_record_removal_preserves_other delta path amount after other Hneq Hrecord).
      destruct polarity; simpl in *; lia.
Qed.

Lemma grouped_count_cons : forall polarity marker relative group groups,
  grouped_count polarity marker relative (group :: groups) =
    if path_eq_dec marker (partitioned_channel_marker group)
    then flat_lookup relative (partition_side polarity (partitioned_channel_delta group))
    else grouped_count polarity marker relative groups.
Proof.
  intros. unfold grouped_count; simpl.
  destruct (path_eq_dec marker (partitioned_channel_marker group)); reflexivity.
Qed.

Lemma partition_record_marker_domain : forall change groups after,
  record_partitioned_change change groups = BuildOk after ->
  forall marker, In marker (map partitioned_channel_marker after) <->
    marker = parsed_change_marker change \/ In marker (map partitioned_channel_marker groups).
Proof.
  intros change groups. induction groups as [|group tail IH]; intros after Hrecord marker;
    simpl in Hrecord.
  - destruct (match parsed_change_polarity change with
      | ChangeAddition => flat_record_addition empty_flat_delta
      | ChangeRemoval => flat_record_removal empty_flat_delta
      end (parsed_change_relative change) (parsed_change_amount change));
      inversion Hrecord; subst; simpl; intuition congruence.
  - destruct (path_eq_dec (parsed_change_marker change) (partitioned_channel_marker group))
      as [Hequal|Hdifferent].
    + destruct (match parsed_change_polarity change with
        | ChangeAddition => flat_record_addition (partitioned_channel_delta group)
        | ChangeRemoval => flat_record_removal (partitioned_channel_delta group)
        end (parsed_change_relative change) (parsed_change_amount change));
        inversion Hrecord; subst; simpl; intuition congruence.
    + destruct (record_partitioned_change change tail) as [next|reason] eqn:Hnext;
        inversion Hrecord; subst; simpl.
      rewrite (IH next eq_refl marker). intuition congruence.
Qed.

Lemma change_contribution_same_marker : forall change polarity relative,
  change_contribution polarity (parsed_change_marker change) relative change =
    if path_eq_dec relative (parsed_change_relative change) then
      if partition_polarity_eq_dec polarity (parsed_change_polarity change)
      then parsed_change_amount change else 0
    else 0.
Proof.
  intros. unfold change_contribution, parsed_pair.
  destruct (partition_pair_eq_dec _ _) as [Hequal|Hdifferent];
    destruct (path_eq_dec relative (parsed_change_relative change));
    try reflexivity; congruence.
Qed.

Lemma change_contribution_other_marker : forall change polarity marker relative,
  marker <> parsed_change_marker change ->
  change_contribution polarity marker relative change = 0.
Proof.
  intros. unfold change_contribution, parsed_pair.
  destruct (partition_pair_eq_dec _ _); [congruence|reflexivity].
Qed.

Lemma change_contribution_other_pair : forall change polarity marker relative,
  (marker, relative) <> parsed_pair change ->
  change_contribution polarity marker relative change = 0.
Proof.
  intros. unfold change_contribution.
  destruct (partition_pair_eq_dec _ _); congruence.
Qed.

Lemma record_partitioned_groups_valid : forall change groups after,
  partition_groups_valid groups ->
  record_partitioned_change change groups = BuildOk after ->
  partition_groups_valid after.
Proof.
  intros change groups. induction groups as [|group tail IH]; intros after [Hunique Hvalid] Hrecord;
    cbn [record_partitioned_change] in Hrecord.
  - change (match partition_record (parsed_change_polarity change) empty_flat_delta
        (parsed_change_relative change) (parsed_change_amount change) with
      | Some delta => BuildOk [PartitionedChannelDeltaState (parsed_change_marker change) delta]
      | None => BuildRefused (RefuseFlatDeltaPartition (parsed_change_absolute change))
      end = BuildOk after) in Hrecord.
    destruct (partition_record _ _ _ _) as [delta|] eqn:Hdelta; [|discriminate].
    inversion Hrecord; subst after. split.
    + simpl. repeat constructor. simpl. tauto.
    + constructor; [|constructor]. simpl.
      exact (@partition_record_normalized (parsed_change_polarity change)
        empty_flat_delta (parsed_change_relative change) (parsed_change_amount change)
        delta eq_refl Hdelta).
  - inversion Hunique as [|head keys Habsent Htail_unique]; subst.
    inversion Hvalid as [|head remaining Hhead_valid Htail_valid]; subst.
    destruct (path_eq_dec (parsed_change_marker change) (partitioned_channel_marker group))
      as [Hequal|Hdifferent].
    + change (match partition_record (parsed_change_polarity change) (partitioned_channel_delta group)
          (parsed_change_relative change) (parsed_change_amount change) with
        | Some delta => BuildOk (PartitionedChannelDeltaState (partitioned_channel_marker group) delta :: tail)
        | None => BuildRefused (RefuseFlatDeltaPartition (parsed_change_absolute change))
        end = BuildOk after) in Hrecord.
      destruct (partition_record _ _ _ _) as [delta|] eqn:Hdelta; [|discriminate].
      inversion Hrecord; subst after. split; [exact Hunique|].
      constructor; [|exact Htail_valid]. simpl.
      eapply partition_record_normalized; eauto.
    + destruct (record_partitioned_change change tail) as [next|reason] eqn:Hnext;
        [|discriminate]. inversion Hrecord; subst after.
      destruct (IH next (conj Htail_unique Htail_valid) eq_refl) as [Hnext_unique Hnext_valid].
      split.
      * simpl. constructor; [|exact Hnext_unique]. intro Hin.
        apply (@partition_record_marker_domain change tail next Hnext) in Hin.
        destruct Hin; congruence.
      * now constructor.
Qed.

Lemma record_partitioned_fresh_exists : forall change groups,
  parsed_change_valid change -> pending_pair_fresh groups change ->
  exists after, record_partitioned_change change groups = BuildOk after.
Proof.
  intros change groups. induction groups as [|group tail IH]; intros [Hpath Hamount] Hfresh.
  - destruct (@partition_record_fresh_exists (parsed_change_polarity change)
      empty_flat_delta (parsed_change_relative change) (parsed_change_amount change)
      Hpath Hamount eq_refl eq_refl) as [delta Hdelta].
    cbn [record_partitioned_change]. change (exists after,
      match partition_record (parsed_change_polarity change) empty_flat_delta
        (parsed_change_relative change) (parsed_change_amount change) with
      | Some next => BuildOk [PartitionedChannelDeltaState (parsed_change_marker change) next]
      | None => BuildRefused (RefuseFlatDeltaPartition (parsed_change_absolute change))
      end = BuildOk after).
    rewrite Hdelta. eauto.
  - cbn [record_partitioned_change].
    destruct (path_eq_dec (parsed_change_marker change) (partitioned_channel_marker group))
      as [Hequal|Hdifferent].
    + assert (Hlocal : forall polarity, flat_lookup (parsed_change_relative change)
        (partition_side polarity (partitioned_channel_delta group)) = 0).
      { intro polarity. specialize (Hfresh polarity).
        unfold pending_pair_fresh in Hfresh. rewrite grouped_count_cons in Hfresh.
        destruct (path_eq_dec _ _); congruence. }
      destruct (@partition_record_fresh_exists (parsed_change_polarity change)
        (partitioned_channel_delta group) (parsed_change_relative change) (parsed_change_amount change)
        Hpath Hamount (Hlocal ChangeAddition) (Hlocal ChangeRemoval)) as [delta Hdelta].
      change (exists after, match partition_record (parsed_change_polarity change)
          (partitioned_channel_delta group) (parsed_change_relative change) (parsed_change_amount change) with
        | Some next => BuildOk (PartitionedChannelDeltaState (partitioned_channel_marker group) next :: tail)
        | None => BuildRefused (RefuseFlatDeltaPartition (parsed_change_absolute change))
        end = BuildOk after).
      rewrite Hdelta. eauto.
    + assert (Htail : pending_pair_fresh tail change).
      { intro polarity. specialize (Hfresh polarity).
        rewrite grouped_count_cons in Hfresh. destruct (path_eq_dec _ _); congruence. }
      destruct (IH (conj Hpath Hamount) Htail) as [after Hafter].
      rewrite Hafter. eauto.
Qed.

Lemma record_partitioned_fresh_observation : forall change groups after,
  partition_groups_valid groups -> pending_pair_fresh groups change ->
  record_partitioned_change change groups = BuildOk after ->
  forall polarity marker relative,
  grouped_count polarity marker relative after =
    grouped_count polarity marker relative groups +
    change_contribution polarity marker relative change.
Proof.
  intros change groups. induction groups as [|group tail IH];
    intros after [Hunique Hvalid] Hfresh Hrecord polarity marker relative;
    cbn [record_partitioned_change] in Hrecord.
  - change (match partition_record (parsed_change_polarity change) empty_flat_delta
        (parsed_change_relative change) (parsed_change_amount change) with
      | Some next => BuildOk [PartitionedChannelDeltaState (parsed_change_marker change) next]
      | None => BuildRefused (RefuseFlatDeltaPartition (parsed_change_absolute change))
      end = BuildOk after) in Hrecord.
    destruct (partition_record _ _ _ _) as [delta|] eqn:Hdelta; [|discriminate].
    inversion Hrecord; subst after. rewrite grouped_count_cons. simpl partitioned_channel_marker.
    destruct (path_eq_dec marker (parsed_change_marker change)) as [Hequal|Hdifferent].
    + subst marker. rewrite change_contribution_same_marker.
      pose proof (@partition_record_fresh_observation (parsed_change_polarity change)
        empty_flat_delta (parsed_change_relative change) (parsed_change_amount change)
        delta eq_refl (fun p => partition_empty_count p (parsed_change_relative change))
        Hdelta polarity relative) as Hcount.
      rewrite partition_empty_count in Hcount.
      exact Hcount.
    + rewrite change_contribution_other_marker by exact Hdifferent. reflexivity.
  - inversion Hunique as [|head keys Habsent Htail_unique]; subst.
    inversion Hvalid as [|head remaining Hhead_valid Htail_valid]; subst.
    destruct (path_eq_dec (parsed_change_marker change) (partitioned_channel_marker group))
      as [Hequal|Hdifferent].
    + change (match partition_record (parsed_change_polarity change) (partitioned_channel_delta group)
          (parsed_change_relative change) (parsed_change_amount change) with
        | Some next => BuildOk (PartitionedChannelDeltaState (partitioned_channel_marker group) next :: tail)
        | None => BuildRefused (RefuseFlatDeltaPartition (parsed_change_absolute change))
        end = BuildOk after) in Hrecord.
      destruct (partition_record _ _ _ _) as [delta|] eqn:Hdelta; [|discriminate].
      inversion Hrecord; subst after.
      assert (Hlocal : forall p, flat_lookup (parsed_change_relative change)
        (partition_side p (partitioned_channel_delta group)) = 0).
      { intro p. specialize (Hfresh p). rewrite grouped_count_cons in Hfresh.
        destruct (path_eq_dec _ _); congruence. }
      rewrite !grouped_count_cons. simpl partitioned_channel_marker.
      destruct (path_eq_dec marker (partitioned_channel_marker group)) as [Hmarker|Hmarker].
      * rewrite <- Hequal in Hmarker. subst marker.
        rewrite change_contribution_same_marker.
        exact (@partition_record_fresh_observation (parsed_change_polarity change)
          (partitioned_channel_delta group) (parsed_change_relative change) (parsed_change_amount change)
          delta Hhead_valid Hlocal Hdelta polarity relative).
      * rewrite change_contribution_other_marker by congruence. lia.
    + destruct (record_partitioned_change change tail) as [next|reason] eqn:Hnext;
        [|discriminate]. inversion Hrecord; subst after.
      assert (Htail : pending_pair_fresh tail change).
      { intro p. specialize (Hfresh p). rewrite grouped_count_cons in Hfresh.
        destruct (path_eq_dec _ _); congruence. }
      rewrite !grouped_count_cons.
      destruct (path_eq_dec marker (partitioned_channel_marker group)) as [Hmarker|Hmarker].
      * rewrite change_contribution_other_marker by congruence. lia.
      * exact (IH next (conj Htail_unique Htail_valid) Htail eq_refl polarity marker relative).
Qed.

Theorem fresh_partition_refines_independent_mass : forall changes groups,
  partition_groups_valid groups ->
  NoDup (map parsed_pair changes) ->
  Forall parsed_change_valid changes ->
  Forall (pending_pair_fresh groups) changes ->
  exists after,
    partition_parsed_changes changes groups = BuildOk after /\
    partition_groups_valid after /\
    forall polarity marker relative,
      grouped_count polarity marker relative after =
        grouped_count polarity marker relative groups +
        independent_change_mass polarity marker relative changes.
Proof.
  intros changes. induction changes as [|change tail IH];
    intros groups Hgroups Hunique Hvalid Hfresh.
  - exists groups. split; [reflexivity|]. split; [exact Hgroups|].
    intros. simpl. lia.
  - inversion Hunique as [|head keys Habsent Htail_unique]; subst.
    inversion Hvalid as [|head rest Hchange_valid Htail_valid]; subst.
    inversion Hfresh as [|head rest Hchange_fresh Htail_fresh]; subst.
    destruct (@record_partitioned_fresh_exists change groups Hchange_valid Hchange_fresh)
      as [next Hnext].
    pose proof (@record_partitioned_groups_valid change groups next Hgroups Hnext) as Hnext_valid.
    pose proof (@record_partitioned_fresh_observation change groups next Hgroups Hchange_fresh Hnext)
      as Hnext_count.
    assert (Hnext_fresh : Forall (pending_pair_fresh next) tail).
    { apply Forall_forall. intros candidate Hin polarity.
      rewrite Hnext_count.
      pose proof ((proj1 (Forall_forall _ _)) Htail_fresh candidate Hin) as Hprior.
      rewrite Hprior.
      rewrite change_contribution_other_pair; [reflexivity|].
      intro Hequal. apply Habsent.
      apply in_map_iff. exists candidate. split; [exact Hequal|exact Hin]. }
    destruct (IH next Hnext_valid Htail_unique Htail_valid Hnext_fresh)
      as [after [Hafter [Hafter_valid Hafter_count]]].
    exists after. split.
    + cbn [partition_parsed_changes]. now rewrite Hnext.
    + split; [exact Hafter_valid|]. intros polarity marker relative.
      rewrite Hafter_count, Hnext_count. cbn [independent_change_mass]. lia.
Qed.

Theorem partition_parsed_marker_domain : forall changes groups after,
  partition_parsed_changes changes groups = BuildOk after ->
  forall marker, In marker (map partitioned_channel_marker after) <->
    In marker (map parsed_change_marker changes) \/
    In marker (map partitioned_channel_marker groups).
Proof.
  intros changes. induction changes as [|change tail IH]; intros groups after Hpartition marker;
    cbn [partition_parsed_changes] in Hpartition.
  - inversion Hpartition; subst. simpl. tauto.
  - destruct (record_partitioned_change change groups) as [next|reason] eqn:Hnext;
      [|discriminate].
    rewrite (IH next after Hpartition marker).
    rewrite (@partition_record_marker_domain change groups next Hnext marker).
    simpl. intuition congruence.
Qed.

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

Definition parsed_change_origin (change : ParsedFlatChange) : Prop :=
  parse_flat_change (parsed_change_polarity change)
    (parsed_change_absolute change, parsed_change_amount change) = BuildOk change.

Lemma parse_flat_change_fields : forall polarity path amount change,
  parse_flat_change polarity (path, amount) = BuildOk change ->
  parsed_change_absolute change = path /\ parsed_change_amount change = amount /\
  parsed_change_polarity change = polarity /\ parsed_change_origin change.
Proof.
  intros polarity path amount change Hparse. unfold parse_flat_change in Hparse; simpl in Hparse.
  destruct (parse_absolute_datum path) as [view|] eqn:Hview; [|discriminate].
  inversion Hparse; subst change. repeat split.
  unfold parsed_change_origin, parse_flat_change; simpl. now rewrite Hview.
Qed.

Lemma path_valid_from_bytes_valid : forall path,
  bytes_valid path = true -> path_valid path = true.
Proof.
  intros path Hvalid. unfold bytes_valid in Hvalid. unfold path_valid.
  apply forallb_forall. intros byte Hin.
  apply forallb_forall with (x := byte) in Hvalid; [|exact Hin].
  unfold RhoPathV1Datum.byte_valid in Hvalid.
  apply Nat.ltb_lt in Hvalid. apply Nat.leb_le. lia.
Qed.

Lemma partition_path_valid_skipn : forall count path,
  path_valid path = true -> path_valid (skipn count path) = true.
Proof.
  intros count. induction count as [|count IH]; intros path Hvalid; [exact Hvalid|].
  destruct path as [|byte tail]; [reflexivity|]. simpl.
  apply IH. apply andb_true_iff in Hvalid as [_ Htail]. exact Htail.
Qed.

Lemma parse_flat_change_is_valid : forall polarity path amount change,
  multiplicity_valid amount = true ->
  parse_flat_change polarity (path, amount) = BuildOk change -> parsed_change_valid change.
Proof.
  intros polarity path amount change Hamount Hparse.
  unfold parse_flat_change in Hparse; simpl in Hparse.
  destruct (parse_absolute_datum path) as [view|] eqn:Hview; [|discriminate].
  inversion Hparse; subst change. split; [|exact Hamount]. simpl.
  destruct (@parsed_relative_offset_is_exact path view Hview) as [_ Hrelative].
  rewrite <- Hrelative. apply partition_path_valid_skipn, path_valid_from_bytes_valid.
  exact (proj1 (@parse_absolute_datum_sound path view Hview)).
Qed.

Lemma parse_flat_side_source_keys : forall polarity entries changes,
  parse_flat_side polarity entries = BuildOk changes ->
  map parsed_change_absolute changes = map fst entries.
Proof.
  intros polarity entries. induction entries as [|[path amount] tail IH]; intros changes Hparse;
    cbn [parse_flat_side] in Hparse.
  - inversion Hparse. reflexivity.
  - destruct (parse_flat_change polarity (path, amount)) as [change|reason] eqn:Hchange;
      [|discriminate].
    destruct (parse_flat_side polarity tail) as [rest|reason] eqn:Hrest; [|discriminate].
    inversion Hparse; subst changes. simpl.
    rewrite (proj1 (@parse_flat_change_fields polarity path amount change Hchange)).
    f_equal. exact (IH rest eq_refl).
Qed.

Lemma parse_flat_side_valid_origins : forall polarity entries changes,
  forallb (fun entry => path_valid (fst entry) && multiplicity_valid (snd entry)) entries = true ->
  parse_flat_side polarity entries = BuildOk changes ->
  Forall (fun change => parsed_change_valid change /\ parsed_change_origin change) changes.
Proof.
  intros polarity entries. induction entries as [|[path amount] tail IH];
    intros changes Hvalid Hparse; cbn [parse_flat_side] in Hparse.
  - inversion Hparse; constructor.
  - apply andb_true_iff in Hvalid as [Hhead Htail].
    apply andb_true_iff in Hhead as [_ Hamount].
    destruct (parse_flat_change polarity (path, amount)) as [change|reason] eqn:Hchange;
      [|discriminate].
    destruct (parse_flat_side polarity tail) as [rest|reason] eqn:Hrest; [|discriminate].
    inversion Hparse; subst changes. constructor.
    + split.
      * exact (@parse_flat_change_is_valid polarity path amount change Hamount Hchange).
      * exact (proj2 (proj2 (proj2 (@parse_flat_change_fields polarity path amount change Hchange)))).
    + exact (IH rest Htail eq_refl).
Qed.

Lemma flat_well_formed_key_nonzero : forall bag path,
  flat_bag_well_formedb bag = true -> In path (map fst bag) -> flat_lookup path bag <> 0.
Proof.
  intros bag path Hvalid Hin.
  apply andb_true_iff in Hvalid as [Hentries Hunique].
  apply in_map_iff in Hin as [[key amount] [Hequal Hin]]. simpl in Hequal. subst key.
  rewrite (@flat_lookup_member_value bag path amount
    (@flat_paths_unique_implies_nodup bag Hunique) Hin).
  apply forallb_forall with (x := (path, amount)) in Hentries; [|exact Hin].
  apply andb_true_iff in Hentries as [_ Hamount].
  unfold multiplicity_valid in Hamount. apply andb_true_iff in Hamount as [Hpositive _].
  apply N.ltb_lt in Hpositive. cbn in Hpositive. lia.
Qed.

Lemma partition_nodup_app : forall (A : Type) (left right : list A),
  NoDup left -> NoDup right ->
  (forall item, In item left -> ~ In item right) -> NoDup (left ++ right).
Proof.
  intros A left right Hleft. induction Hleft as [|head tail Habsent Htail IH];
    intros Hright Hcross; simpl; [exact Hright|].
  constructor.
  - intro Hin. apply in_app_or in Hin. destruct Hin; [contradiction|].
    apply (Hcross head); [now left|assumption].
  - apply IH; [exact Hright|]. intros item Hin. apply Hcross. now right.
Qed.

Lemma normalized_flat_source_keys_unique : forall delta,
  flat_delta_well_formedb delta = true ->
  NoDup (map fst (flat_delta_additions delta) ++ map fst (flat_delta_removals delta)).
Proof.
  intros delta Hvalid.
  pose proof (@partition_side_well_formed delta ChangeAddition Hvalid) as Hadd.
  pose proof (@partition_side_well_formed delta ChangeRemoval Hvalid) as Hremove.
  unfold flat_delta_well_formedb in Hvalid.
  apply andb_true_iff in Hvalid as [_ Hdisjoint].
  apply partition_nodup_app.
  - apply flat_paths_unique_implies_nodup.
    exact (proj2 ((proj1 (andb_true_iff _ _)) Hadd)).
  - apply flat_paths_unique_implies_nodup.
    exact (proj2 ((proj1 (andb_true_iff _ _)) Hremove)).
  - intros path Hin_add Hin_remove.
    pose proof (@flat_well_formed_key_nonzero _ path Hadd Hin_add) as Hnonzero_add.
    pose proof (@flat_well_formed_key_nonzero _ path Hremove Hin_remove) as Hnonzero_remove.
    pose proof ((proj1 (@flat_disjoint_is_pointwise
      (flat_delta_additions delta) (flat_delta_removals delta) Hadd)) Hdisjoint path).
    tauto.
Qed.

Lemma partition_nodup_map_inv : forall (A B : Type) (f : A -> B) items,
  NoDup (map f items) -> NoDup items.
Proof.
  intros A B f items. induction items as [|head tail IH]; intros Hunique; [constructor|].
  inversion Hunique as [|key keys Habsent Htail]; subst. constructor.
  - intro Hin. apply Habsent. now apply in_map.
  - now apply IH.
Qed.

Lemma parsed_origins_reconstruct_pairs : forall changes,
  Forall parsed_change_origin changes ->
  map parsed_change_absolute changes = map (fun key => fst key ++ snd key) (map parsed_pair changes).
Proof.
  intros changes Horigins. induction Horigins as [|change tail Horigin Htail IH]; simpl;
    [reflexivity|]. f_equal; [|exact IH].
  unfold parsed_change_origin in Horigin.
  exact (@parsed_flat_change_reconstructs_absolute (parsed_change_polarity change)
    (parsed_change_absolute change, parsed_change_amount change) change Horigin).
Qed.

Lemma parsed_normalized_delta_inputs : forall delta changes,
  flat_delta_well_formedb delta = true -> parse_all_flat_changes delta = BuildOk changes ->
  NoDup (map parsed_pair changes) /\ Forall parsed_change_valid changes /\
  Forall parsed_change_origin changes.
Proof.
  intros delta changes Hvalid Hparse.
  pose proof (@partition_side_well_formed delta ChangeAddition Hvalid) as Hadd.
  pose proof (@partition_side_well_formed delta ChangeRemoval Hvalid) as Hremove.
  unfold parse_all_flat_changes in Hparse.
  destruct (parse_flat_side ChangeAddition (flat_delta_additions delta)) as [adds|reason]
    eqn:Hadds; [|discriminate].
  destruct (parse_flat_side ChangeRemoval (flat_delta_removals delta)) as [removes|reason]
    eqn:Hremoves; [|discriminate]. inversion Hparse; subst changes.
  apply andb_true_iff in Hadd as [Hadd _].
  apply andb_true_iff in Hremove as [Hremove _].
  pose proof (@parse_flat_side_valid_origins ChangeAddition _ _ Hadd Hadds) as Ha.
  pose proof (@parse_flat_side_valid_origins ChangeRemoval _ _ Hremove Hremoves) as Hr.
  assert (Hboth : Forall (fun c => parsed_change_valid c /\ parsed_change_origin c) (adds ++ removes)).
  { apply Forall_app. auto. }
  assert (Horigins : Forall parsed_change_origin (adds ++ removes)).
  { eapply Forall_impl; [|exact Hboth]. intros c Hc. exact (proj2 Hc). }
  split.
  - apply (@partition_nodup_map_inv _ _ (fun key => fst key ++ snd key)).
    rewrite <- (parsed_origins_reconstruct_pairs Horigins), map_app.
    rewrite (@parse_flat_side_source_keys ChangeAddition _ _ Hadds),
      (@parse_flat_side_source_keys ChangeRemoval _ _ Hremoves).
    now apply normalized_flat_source_keys_unique.
  - split; [|exact Horigins].
    eapply Forall_impl; [|exact Hboth]. intros c Hc. exact (proj1 Hc).
Qed.

Lemma parsed_change_contribution_matches_absolute : forall written path amount change absolute view polarity,
  parse_flat_change written (path, amount) = BuildOk change ->
  parse_absolute_datum absolute = Some view ->
  change_contribution polarity (decoded_view_marker view) (decoded_view_relative view) change =
    if path_eq_dec absolute path then
      if partition_polarity_eq_dec polarity written then amount else 0
    else 0.
Proof.
  intros written path amount change absolute view polarity Hchange Hquery.
  unfold parse_flat_change in Hchange; simpl in Hchange.
  destruct (parse_absolute_datum path) as [source|] eqn:Hsource; [|discriminate].
  inversion Hchange; subst change.
  unfold change_contribution, parsed_pair.
  cbn [parsed_change_marker parsed_change_relative parsed_change_polarity parsed_change_amount].
  destruct (partition_pair_eq_dec _ _) as [Hpair|Hpair];
    destruct (path_eq_dec absolute path) as [Hpath|Hpath]; try reflexivity.
  - pose proof (f_equal (fun key : Path * Path => fst key ++ snd key) Hpair) as Hconcat.
    cbn in Hconcat.
    pose proof (proj2 (@parse_absolute_datum_sound absolute view Hquery)) as Habsolute.
    pose proof (proj2 (@parse_absolute_datum_sound path source Hsource)) as Hstored.
    change (absolute = decoded_view_marker view ++ decoded_view_relative view) in Habsolute.
    change (path = decoded_view_marker source ++ decoded_view_relative source) in Hstored.
    congruence.
  - subst path. rewrite Hquery in Hsource. inversion Hsource; subst source. contradiction.
Qed.

Lemma independent_mass_app : forall polarity marker relative left right,
  independent_change_mass polarity marker relative (left ++ right) =
    independent_change_mass polarity marker relative left +
    independent_change_mass polarity marker relative right.
Proof.
  intros polarity marker relative left. induction left; intros right; simpl; [reflexivity|].
  rewrite IHleft. lia.
Qed.

Lemma parsed_side_mass_matches_flat_lookup : forall written entries changes,
  NoDup (map fst entries) -> parse_flat_side written entries = BuildOk changes ->
  forall absolute view polarity,
  parse_absolute_datum absolute = Some view ->
  independent_change_mass polarity (decoded_view_marker view) (decoded_view_relative view) changes =
    if partition_polarity_eq_dec polarity written then flat_lookup absolute entries else 0.
Proof.
  intros written entries. induction entries as [|[path amount] tail IH];
    intros changes Hunique Hparse absolute view polarity Hquery;
    cbn [parse_flat_side] in Hparse.
  - inversion Hparse; subst changes. destruct (partition_polarity_eq_dec _ _); reflexivity.
  - inversion Hunique as [|head keys Habsent Htail_unique]; subst.
    destruct (parse_flat_change written (path, amount)) as [change|reason] eqn:Hchange;
      [|discriminate].
    destruct (parse_flat_side written tail) as [rest|reason] eqn:Hrest; [|discriminate].
    inversion Hparse; subst changes. cbn [independent_change_mass].
    rewrite (@parsed_change_contribution_matches_absolute written path amount change
      absolute view polarity Hchange Hquery).
    rewrite (IH rest Htail_unique eq_refl absolute view polarity Hquery).
    change ((if path_eq_dec absolute path then
        if partition_polarity_eq_dec polarity written then amount else 0 else 0) +
      (if partition_polarity_eq_dec polarity written then flat_lookup absolute tail else 0) =
      if partition_polarity_eq_dec polarity written then
        if path_eq_dec absolute path then amount else flat_lookup absolute tail else 0).
    destruct (path_eq_dec absolute path) as [Hequal|Hdifferent];
      destruct (partition_polarity_eq_dec polarity written); try lia.
    subst absolute. rewrite flat_lookup_absent by exact Habsent. lia.
Qed.

Theorem parsed_delta_mass_matches_flat_lookup : forall delta changes,
  flat_delta_well_formedb delta = true -> parse_all_flat_changes delta = BuildOk changes ->
  forall absolute view polarity,
  parse_absolute_datum absolute = Some view ->
  independent_change_mass polarity (decoded_view_marker view) (decoded_view_relative view) changes =
    flat_lookup absolute (partition_side polarity delta).
Proof.
  intros delta changes Hvalid Hparse absolute view polarity Hquery.
  pose proof (@partition_side_well_formed delta ChangeAddition Hvalid) as Hadd.
  pose proof (@partition_side_well_formed delta ChangeRemoval Hvalid) as Hremove.
  apply andb_true_iff in Hadd as [_ Hadd].
  apply andb_true_iff in Hremove as [_ Hremove].
  unfold parse_all_flat_changes in Hparse.
  destruct (parse_flat_side ChangeAddition (flat_delta_additions delta)) as [adds|reason]
    eqn:Hadds; [|discriminate].
  destruct (parse_flat_side ChangeRemoval (flat_delta_removals delta)) as [removes|reason]
    eqn:Hremoves; [|discriminate]. inversion Hparse; subst changes.
  rewrite independent_mass_app.
  rewrite (@parsed_side_mass_matches_flat_lookup ChangeAddition _ _
      (@flat_paths_unique_implies_nodup _ Hadd) Hadds absolute view polarity Hquery).
  rewrite (@parsed_side_mass_matches_flat_lookup ChangeRemoval _ _
      (@flat_paths_unique_implies_nodup _ Hremove) Hremoves absolute view polarity Hquery).
  destruct polarity; cbn [partition_side];
    repeat match goal with
    | |- context [partition_polarity_eq_dec ?left ?right] =>
        destruct (partition_polarity_eq_dec left right); try congruence
    end; lia.
Qed.

Theorem normalized_parsed_partition_exists : forall delta changes,
  flat_delta_well_formedb delta = true -> parse_all_flat_changes delta = BuildOk changes ->
  exists groups, partition_flat_delta delta = BuildOk groups /\
    partition_groups_valid groups /\
    (forall polarity marker relative,
      grouped_count polarity marker relative groups =
        independent_change_mass polarity marker relative changes) /\
    (forall marker, In marker (map partitioned_channel_marker groups) <->
      In marker (map parsed_change_marker changes)).
Proof.
  intros delta changes Hvalid Hparse.
  destruct (@parsed_normalized_delta_inputs delta changes Hvalid Hparse)
    as [Hunique [Hchanges Horigins]].
  assert (Hempty : partition_groups_valid []).
  { split; constructor. }
  assert (Hfresh : Forall (pending_pair_fresh []) changes).
  { apply Forall_forall. intros change Hin polarity. reflexivity. }
  destruct (@fresh_partition_refines_independent_mass changes [] Hempty Hunique Hchanges Hfresh)
    as [groups [Hpartition [Hgroups Hcounts]]].
  exists groups. split.
  - unfold partition_flat_delta. now rewrite Hparse.
  - split; [exact Hgroups|]. split.
    + intros. exact (Hcounts polarity marker relative).
    + intro marker. rewrite (@partition_parsed_marker_domain changes [] groups Hpartition marker).
      simpl. tauto.
Qed.

Theorem grouped_partition_preserves_both_flat_sides : forall delta groups,
  flat_delta_well_formedb delta = true -> partition_flat_delta delta = BuildOk groups ->
  partition_groups_valid groups /\
  forall absolute view,
    parse_absolute_datum absolute = Some view ->
    grouped_count ChangeAddition (decoded_view_marker view) (decoded_view_relative view) groups =
      flat_lookup absolute (flat_delta_additions delta) /\
    grouped_count ChangeRemoval (decoded_view_marker view) (decoded_view_relative view) groups =
      flat_lookup absolute (flat_delta_removals delta).
Proof.
  intros delta groups Hvalid Hpartition.
  unfold partition_flat_delta in Hpartition.
  destruct (parse_all_flat_changes delta) as [changes|reason] eqn:Hparse; [|discriminate].
  destruct (@normalized_parsed_partition_exists delta changes Hvalid Hparse)
    as [expected [Hexpected [Hgroups [Hcounts Hmarkers]]]].
  unfold partition_flat_delta in Hexpected. rewrite Hparse in Hexpected.
  rewrite Hpartition in Hexpected. inversion Hexpected; subst expected.
  split; [exact Hgroups|]. intros absolute view Hquery.
  rewrite !Hcounts. split;
    exact (@parsed_delta_mass_matches_flat_lookup delta changes Hvalid Hparse absolute view _ Hquery).
Qed.

Lemma independent_mass_nonzero_witness : forall changes polarity marker relative,
  independent_change_mass polarity marker relative changes <> 0 ->
  exists change, In change changes /\ parsed_pair change = (marker, relative) /\
    parsed_change_polarity change = polarity /\ parsed_change_amount change <> 0.
Proof.
  intros changes. induction changes as [|change tail IH]; intros polarity marker relative Hnonzero;
    cbn [independent_change_mass] in Hnonzero; [contradiction|].
  destruct (N.eq_dec (change_contribution polarity marker relative change) 0) as [Hzero|Hpositive].
  - rewrite Hzero in Hnonzero. simpl in Hnonzero.
    destruct (IH polarity marker relative Hnonzero) as [found [Hin Hfound]].
    exists found. split; [now right|exact Hfound].
  - unfold change_contribution in Hpositive.
    destruct (partition_pair_eq_dec _ _) as [Hpair|Hpair]; [|contradiction].
    destruct (partition_polarity_eq_dec _ _) as [Hpolarity|Hpolarity]; [|contradiction].
    exists change. split; [now left|]. split; [symmetry; exact Hpair|].
    split; [symmetry; exact Hpolarity|exact Hpositive].
Qed.

Theorem grouped_partition_has_no_extra_endpoints : forall delta groups polarity marker relative,
  flat_delta_well_formedb delta = true -> partition_flat_delta delta = BuildOk groups ->
  grouped_count polarity marker relative groups <> 0 ->
  exists changes change,
    parse_all_flat_changes delta = BuildOk changes /\ In change changes /\
    parsed_change_origin change /\ parsed_pair change = (marker, relative) /\
    parsed_change_polarity change = polarity /\ parsed_change_amount change <> 0.
Proof.
  intros delta groups polarity marker relative Hvalid Hpartition Hnonzero.
  unfold partition_flat_delta in Hpartition.
  destruct (parse_all_flat_changes delta) as [changes|reason] eqn:Hparse; [|discriminate].
  destruct (@normalized_parsed_partition_exists delta changes Hvalid Hparse)
    as [expected [Hexpected [Hgroups [Hcounts Hmarkers]]]].
  unfold partition_flat_delta in Hexpected. rewrite Hparse, Hpartition in Hexpected.
  inversion Hexpected; subst expected. rewrite Hcounts in Hnonzero.
  destruct (@independent_mass_nonzero_witness changes polarity marker relative Hnonzero)
    as [change [Hin Hproperties]].
  destruct (@parsed_normalized_delta_inputs delta changes Hvalid Hparse) as [_ [_ Horigins]].
  exists changes, change. split; [reflexivity|]. split; [exact Hin|]. split.
  - exact ((proj1 (Forall_forall _ _)) Horigins change Hin).
  - exact Hproperties.
Qed.

Theorem normalized_partition_refusal_is_exactly_parse_refusal : forall delta reason,
  flat_delta_well_formedb delta = true ->
  (partition_flat_delta delta = BuildRefused reason <->
   parse_all_flat_changes delta = BuildRefused reason).
Proof.
  intros delta reason Hvalid. split.
  - intro Hrefused. destruct (parse_all_flat_changes delta) as [changes|failure] eqn:Hparse.
    + destruct (@normalized_parsed_partition_exists delta changes Hvalid Hparse)
        as [groups [Hgroups _]]. congruence.
    + unfold partition_flat_delta in Hrefused. rewrite Hparse in Hrefused.
      now inversion Hrefused.
  - intro Hparse. unfold partition_flat_delta. now rewrite Hparse.
Qed.

Lemma parsed_side_member_source : forall polarity entries changes,
  parse_flat_side polarity entries = BuildOk changes ->
  forall change, In change changes ->
  In (parsed_change_absolute change, parsed_change_amount change) entries /\
  parsed_change_polarity change = polarity.
Proof.
  intros polarity entries. induction entries as [|[path amount] tail IH];
    intros changes Hparse change Hin; cbn [parse_flat_side] in Hparse.
  - inversion Hparse; subst. contradiction.
  - destruct (parse_flat_change polarity (path, amount)) as [head|reason] eqn:Hhead;
      [|discriminate].
    destruct (parse_flat_side polarity tail) as [rest|reason] eqn:Hrest; [|discriminate].
    inversion Hparse; subst changes. destruct Hin as [Hequal|Hin].
    + subst change. destruct (@parse_flat_change_fields polarity path amount head Hhead)
        as [Hpath [Hamount [Hpolarity Horigin]]].
      split; [|exact Hpolarity]. left. now rewrite Hpath, Hamount.
    + destruct (IH rest eq_refl change Hin) as [Hmember Hpolarity].
      split; [now right|exact Hpolarity].
Qed.

Lemma parsed_delta_member_source : forall delta changes change polarity,
  parse_all_flat_changes delta = BuildOk changes -> In change changes ->
  parsed_change_polarity change = polarity ->
  In (parsed_change_absolute change, parsed_change_amount change) (partition_side polarity delta).
Proof.
  intros delta changes change polarity Hparse Hin Hpolarity.
  unfold parse_all_flat_changes in Hparse.
  destruct (parse_flat_side ChangeAddition (flat_delta_additions delta)) as [adds|reason]
    eqn:Hadds; [|discriminate].
  destruct (parse_flat_side ChangeRemoval (flat_delta_removals delta)) as [removes|reason]
    eqn:Hremoves; [|discriminate]. inversion Hparse; subst changes.
  apply in_app_or in Hin. destruct Hin as [Hin|Hin].
  - destruct (@parsed_side_member_source ChangeAddition _ _ Hadds change Hin) as [Hmember Hside].
    rewrite Hpolarity in Hside. rewrite Hside. exact Hmember.
  - destruct (@parsed_side_member_source ChangeRemoval _ _ Hremoves change Hin) as [Hmember Hside].
    rewrite Hpolarity in Hside. rewrite Hside. exact Hmember.
Qed.

Theorem grouped_partition_has_exact_source_endpoints : forall delta groups polarity marker relative,
  flat_delta_well_formedb delta = true -> partition_flat_delta delta = BuildOk groups ->
  grouped_count polarity marker relative groups <> 0 ->
  exists absolute amount view,
    In (absolute, amount) (partition_side polarity delta) /\
    parse_absolute_datum absolute = Some view /\
    decoded_view_marker view = marker /\ decoded_view_relative view = relative /\
    grouped_count polarity marker relative groups = amount.
Proof.
  intros delta groups polarity marker relative Hvalid Hpartition Hnonzero.
  destruct (@grouped_partition_has_no_extra_endpoints delta groups polarity marker relative
    Hvalid Hpartition Hnonzero) as [changes [change [Hparse [Hin [Horigin [Hpair [Hpolarity Hamount]]]]]]].
  pose proof (@parsed_delta_member_source delta changes change polarity Hparse Hin Hpolarity) as Hmember.
  unfold parsed_change_origin, parse_flat_change in Horigin; simpl in Horigin.
  destruct (parse_absolute_datum (parsed_change_absolute change)) as [view|] eqn:Hview;
    [|discriminate].
  injection Horigin as Hrepresentation.
  pose proof (f_equal parsed_change_marker Hrepresentation) as Hmarker.
  pose proof (f_equal parsed_change_relative Hrepresentation) as Hrelative.
  cbn [parsed_change_marker parsed_change_relative] in Hmarker, Hrelative.
  unfold parsed_pair in Hpair. injection Hpair as Hpair_marker Hpair_relative.
  rewrite Hpair_marker in Hmarker. rewrite Hpair_relative in Hrelative.
  exists (parsed_change_absolute change), (parsed_change_amount change), view.
  split; [exact Hmember|]. split; [exact Hview|]. split; [exact Hmarker|].
  split; [exact Hrelative|].
  pose proof (proj2 (@grouped_partition_preserves_both_flat_sides delta groups Hvalid Hpartition)
    (parsed_change_absolute change) view Hview) as Hcounts.
  rewrite Hmarker, Hrelative in Hcounts.
  assert (Hexact : flat_lookup (parsed_change_absolute change) (partition_side polarity delta) =
      parsed_change_amount change).
  { apply flat_lookup_member_value; [|exact Hmember]. apply flat_paths_unique_implies_nodup.
    pose proof (@partition_side_well_formed delta polarity Hvalid) as Hside.
    now apply andb_true_iff in Hside as [_ Hside]. }
  destruct polarity; simpl in Hexact; intuition congruence.
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

Definition check_commit_liveness_with_input
    (checked : CheckedObservedPool) (commit : TwoLevelAuthority)
    : BuildResult { live : CheckedCommitLiveness checked | checked_commit_authority live = commit }.
Proof.
  destruct (authority_well_formedb commit) eqn:Hauthority.
  - destruct (commit_selection_liveb commit (checked_pool_selection checked))
      eqn:Hlive.
    + exact (BuildOk (exist _ (CheckedCommitLivenessState checked commit Hlive) eq_refl)).
    + exact (BuildRefused RefuseCommitLiveness).
  - exact (BuildRefused RefuseCommitAuthority).
Defined.

Definition check_commit_liveness checked commit : BuildResult (CheckedCommitLiveness checked) :=
  match check_commit_liveness_with_input checked commit with
  | BuildOk live => BuildOk (proj1_sig live)
  | BuildRefused reason => BuildRefused reason
  end.

Definition checked_operation_succeeds {A : Type} (result : option A) : bool :=
  match result with Some _ => true | None => false end.

Lemma checked_liveness_matches_supplied_authority : forall checked commit live,
  check_commit_liveness checked commit = BuildOk live ->
  checked_commit_authority live = commit.
Proof.
  intros checked commit live Hlive. unfold check_commit_liveness in Hlive.
  destruct (check_commit_liveness_with_input checked commit) as [[value Hinput]|reason];
    [|discriminate].
  inversion Hlive; subst live. exact Hinput.
Qed.

Definition strict_flat_preflightb
    (count : Path -> Multiplicity) (delta : FlatBagDelta) : bool :=
  forallb (fun entry => checked_operation_succeeds
    (checked_subtract_multiplicity (count (fst entry)) (snd entry)))
    (flat_delta_removals delta) &&
  forallb (fun entry => checked_operation_succeeds
    (checked_add_multiplicity
      (count (fst entry) - flat_lookup (fst entry) (flat_delta_removals delta))
      (snd entry))) (flat_delta_additions delta).

Definition strict_flat_feasible
    (count : Path -> Multiplicity) (delta : FlatBagDelta) : Prop :=
  (forall path, flat_lookup path (flat_delta_removals delta) <= count path) /\
  (forall path, flat_lookup path (flat_delta_additions delta) <> 0 ->
    count path - flat_lookup path (flat_delta_removals delta) +
      flat_lookup path (flat_delta_additions delta) <= multiplicity_max).

Lemma checked_subtraction_success_iff : forall available removed,
  checked_operation_succeeds (checked_subtract_multiplicity available removed) = true
    <-> removed <= available.
Proof.
  intros available removed. unfold checked_subtract_multiplicity.
  destruct (N.leb removed available) eqn:Hle.
  - apply N.leb_le in Hle. destruct (N.eqb removed available); simpl; tauto.
  - apply N.leb_gt in Hle. simpl. split; [discriminate|lia].
Qed.

Lemma checked_addition_success_iff : forall available added,
  0 < added ->
  (checked_operation_succeeds (checked_add_multiplicity available added) = true
    <-> available + added <= multiplicity_max).
Proof.
  intros available added Hpositive. unfold checked_add_multiplicity.
  assert (Hsum : (0 <? available + added) = true) by (apply N.ltb_lt; lia).
  rewrite Hsum. simpl.
  destruct (N.leb (available + added) multiplicity_max) eqn:Hle; simpl.
  - apply N.leb_le in Hle. tauto.
  - apply N.leb_gt in Hle. split; [discriminate|lia].
Qed.

Lemma well_formed_flat_member : forall bag path amount,
  flat_bag_well_formedb bag = true -> In (path, amount) bag ->
  flat_lookup path bag = amount /\ 0 < amount.
Proof.
  intros bag path amount Hvalid Hin.
  apply andb_true_iff in Hvalid as [Hentries Hunique]. split.
  - apply flat_lookup_member_value; [now apply flat_paths_unique_implies_nodup|exact Hin].
  - apply forallb_forall with (x := (path, amount)) in Hentries; [|exact Hin].
    apply andb_true_iff in Hentries as [_ Hamount].
    apply andb_true_iff in Hamount as [Hpositive _]. now apply N.ltb_lt.
Qed.

Theorem strict_flat_preflight_reflects_feasibility : forall count delta,
  flat_delta_well_formedb delta = true ->
  (strict_flat_preflightb count delta = true <-> strict_flat_feasible count delta).
Proof.
  intros count delta Hvalid.
  pose proof (@partition_side_well_formed delta ChangeAddition Hvalid) as Hadd.
  pose proof (@partition_side_well_formed delta ChangeRemoval Hvalid) as Hremove.
  cbn [partition_side] in Hadd, Hremove.
  unfold strict_flat_preflightb, strict_flat_feasible.
  rewrite andb_true_iff, !forallb_forall. split.
  - intros [Hsub Hplus]. split.
    + intro path. destruct (N.eq_dec (flat_lookup path (flat_delta_removals delta)) 0)
        as [Hzero|Hnonzero]; [rewrite Hzero; lia|].
      apply flat_lookup_nonzero_member in Hnonzero.
      apply in_map_iff in Hnonzero as [[current amount] [Heq Hin]]. simpl in Heq.
      subst current. destruct (@well_formed_flat_member _ _ _ Hremove Hin) as [Hlookup _].
      rewrite Hlookup. apply checked_subtraction_success_iff.
      exact (Hsub (path, amount) Hin).
    + intros path Hnonzero. apply flat_lookup_nonzero_member in Hnonzero.
      apply in_map_iff in Hnonzero as [[current amount] [Heq Hin]]. simpl in Heq.
      subst current. destruct (@well_formed_flat_member _ _ _ Hadd Hin) as [Hlookup Hpositive].
      rewrite Hlookup. apply (proj1 (@checked_addition_success_iff _ amount Hpositive)).
      exact (Hplus (path, amount) Hin).
  - intros [Hsub Hplus]. split; intros [path amount] Hin; simpl.
    + destruct (@well_formed_flat_member _ _ _ Hremove Hin) as [Hlookup _].
      apply checked_subtraction_success_iff. rewrite <- Hlookup. apply Hsub.
    + destruct (@well_formed_flat_member _ _ _ Hadd Hin) as [Hlookup Hpositive].
      apply (proj2 (@checked_addition_success_iff _ amount Hpositive)).
      rewrite <- Hlookup. apply Hplus. rewrite Hlookup. lia.
Qed.

Theorem strict_flat_preflight_is_extensional : forall count first second,
  flat_delta_well_formedb first = true -> flat_delta_well_formedb second = true ->
  (forall path, flat_lookup path (flat_delta_additions first) =
    flat_lookup path (flat_delta_additions second)) ->
  (forall path, flat_lookup path (flat_delta_removals first) =
    flat_lookup path (flat_delta_removals second)) ->
  (strict_flat_preflightb count first = true <-> strict_flat_preflightb count second = true).
Proof.
  intros count first second Hfirst Hsecond Hadd Hremove.
  rewrite (@strict_flat_preflight_reflects_feasibility count first Hfirst),
    (@strict_flat_preflight_reflects_feasibility count second Hsecond).
  unfold strict_flat_feasible.
  setoid_rewrite Hadd. setoid_rewrite Hremove. reflexivity.
Qed.

Theorem strict_flat_preflight_ignores_unrelated_counts : forall before after delta,
  flat_delta_well_formedb delta = true ->
  (forall path, flat_lookup path (flat_delta_removals delta) <> 0 \/
    flat_lookup path (flat_delta_additions delta) <> 0 -> before path = after path) ->
  (strict_flat_preflightb before delta = true <-> strict_flat_preflightb after delta = true).
Proof.
  intros before after delta Hvalid Hsame.
  rewrite (@strict_flat_preflight_reflects_feasibility before delta Hvalid),
    (@strict_flat_preflight_reflects_feasibility after delta Hvalid).
  unfold strict_flat_feasible. split; intros [Hremove Hadd]; split.
  - intro path. destruct (N.eq_dec (flat_lookup path (flat_delta_removals delta)) 0).
    + rewrite e. lia.
    + rewrite <- Hsame; [apply Hremove|now left].
  - intros path Hnonzero. rewrite <- Hsame; [now apply Hadd|now right].
  - intro path. destruct (N.eq_dec (flat_lookup path (flat_delta_removals delta)) 0).
    + rewrite e. lia.
    + rewrite Hsame; [apply Hremove|now left].
  - intros path Hnonzero. rewrite Hsame; [now apply Hadd|now right].
Qed.

Record CheckedCommitDelta (commit : TwoLevelAuthority) (delta : FlatBagDelta) : Type :=
  CheckedCommitDeltaState {
    checked_commit_delta_preflight :
      strict_flat_preflightb (commit_absolute_count commit) delta = true
  }.

Definition check_commit_delta (commit : TwoLevelAuthority) (delta : FlatBagDelta)
    : BuildResult (CheckedCommitDelta commit delta).
Proof.
  destruct (strict_flat_preflightb (commit_absolute_count commit) delta) eqn:Hchecked.
  - exact (BuildOk (@CheckedCommitDeltaState commit delta Hchecked)).
  - exact (BuildRefused RefuseCommitDelta).
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
  prepared_commit_delta : CheckedCommitDelta
    (checked_commit_authority prepared_checked_commit) prepared_flat_delta;
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

Definition preparation_matches_input input prepared : Prop :=
  checked_pool_selection (prepared_checked_pool prepared) = preparation_selected_candidates input /\
  pool_observed_authority (checked_pool_value (prepared_checked_pool prepared)) =
    preparation_observed_authority input /\
  checked_commit_authority (prepared_checked_commit prepared) = preparation_commit_authority input.

Definition prepare_match_changes_with_input (input : MatchPreparationInput)
    : BuildResult { prepared : PreparedMatchChanges | preparation_matches_input input prepared }.
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
           ++ destruct (check_commit_delta (checked_commit_authority live) delta)
                as [arithmetic|arithmetic_reason] eqn:Harithmetic.
              ** apply BuildOk.
              refine (exist _ {| prepared_checked_pool := checked;
                prepared_checked_commit := live;
                prepared_flat_delta := delta;
                prepared_partition_permit := partition;
                prepared_commit_delta := arithmetic;
                prepared_observed_authority_is_checked :=
                  checked_pool_observed_authority_well_formed checked;
                prepared_delta_is_primary_executable_output := Hdelta;
                prepared_delta_invariant := Hdelta_valid |} _).
                 unfold preparation_matches_input. simpl.
                 destruct (@checked_pool_matches_supplied_input _ _ _ _ _ _ _ _ _ Hpool)
                   as [Hselection Hauthority].
                 split; [exact Hselection|]. split; [exact Hauthority|].
                 exact (@checked_liveness_matches_supplied_authority _ _ _ Hlive).
              ** exact (BuildRefused arithmetic_reason).
           ++ exact (BuildRefused partition_reason).
        -- exact (BuildRefused delta_reason).
      * exact (BuildRefused live_reason).
    + exact (BuildRefused reason).
  - exact (BuildRefused RefuseObservedAuthority).
Defined.

Definition prepare_match_changes input : MatchPreparationOutcome :=
  match prepare_match_changes_with_input input with
  | BuildOk prepared => PreparationReady (proj1_sig prepared)
  | BuildRefused reason => PreparationRefused reason
  end.

Theorem preparation_ready_is_bound_to_supplied_input : forall input prepared,
  prepare_match_changes input = PreparationReady prepared -> preparation_matches_input input prepared.
Proof.
  intros input prepared Hready. unfold prepare_match_changes in Hready.
  destruct (prepare_match_changes_with_input input) as [[value Hinput]|reason]; [|discriminate].
  inversion Hready; subst prepared. exact Hinput.
Qed.

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

Theorem preparation_ready_has_strict_commit_delta : forall input prepared,
  prepare_match_changes input = PreparationReady prepared ->
  strict_flat_feasible
    (commit_absolute_count (checked_commit_authority (prepared_checked_commit prepared)))
    (prepared_flat_delta prepared).
Proof.
  intros input prepared _.
  apply (proj1 (@strict_flat_preflight_reflects_feasibility _ _
    (prepared_delta_invariant prepared))).
  exact (checked_commit_delta_preflight (prepared_commit_delta prepared)).
Qed.

Theorem preparation_ready_checks_actual_input_counts : forall input prepared,
  prepare_match_changes input = PreparationReady prepared ->
  commit_selection_liveb (preparation_commit_authority input)
    (preparation_selected_candidates input) = true /\
  strict_flat_feasible (commit_absolute_count (preparation_commit_authority input))
    (prepared_flat_delta prepared).
Proof.
  intros input prepared Hready.
  destruct (@preparation_ready_is_bound_to_supplied_input input prepared Hready)
    as [Hselected [Hobserved Hcommit]].
  split.
  - rewrite <- Hcommit, <- Hselected.
    exact (checked_commit_liveness (prepared_checked_commit prepared)).
  - rewrite <- Hcommit. now apply (@preparation_ready_has_strict_commit_delta input prepared).
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

Record CancellationGuardedDelta : Type := CancellationGuardedDeltaState {
  guarded_delta : FlatBagDelta;
  cancellation_guards : FlatBag
}.

Definition initialize_guarded_removals (removals : FlatBag) : CancellationGuardedDelta :=
  CancellationGuardedDeltaState (FlatBagDeltaState [] removals) [].

Definition remember_cancelled_demand (guards : FlatBag) (path : Path)
    (removal : Multiplicity) : FlatBag :=
  if N.eqb removal 0 then guards
  else flat_set_count path (N.max (flat_lookup path guards) removal) guards.

Definition guarded_record_addition (before : CancellationGuardedDelta)
    (path : Path) (amount : Multiplicity) : option CancellationGuardedDelta :=
  match flat_record_addition (guarded_delta before) path amount with
  | None => None
  | Some after => Some (CancellationGuardedDeltaState after
      (remember_cancelled_demand (cancellation_guards before) path
        (flat_lookup path (flat_delta_removals (guarded_delta before)))))
  end.

Definition guarded_original_demand (state : CancellationGuardedDelta) (path : Path) :=
  N.max (flat_lookup path (cancellation_guards state))
    (flat_lookup path (flat_delta_removals (guarded_delta state))).

Definition guarded_delta_well_formed (state : CancellationGuardedDelta) : Prop :=
  flat_delta_well_formedb (guarded_delta state) = true /\
  flat_bag_well_formedb (cancellation_guards state) = true.

Lemma remembered_demand_at_target : forall guards path removal,
  flat_bag_well_formedb guards = true ->
  flat_lookup path (remember_cancelled_demand guards path removal) =
    N.max (flat_lookup path guards) removal.
Proof.
  intros guards path removal Hvalid. unfold remember_cancelled_demand.
  destruct (N.eqb removal 0) eqn:Hzero.
  - apply N.eqb_eq in Hzero. subst removal. now rewrite N.max_0_r.
  - apply flat_set_count_target. apply flat_paths_unique_implies_nodup.
    now apply andb_true_iff in Hvalid as [_ Hvalid].
Qed.

Lemma remembered_demand_at_other : forall guards path removal other,
  path <> other ->
  flat_lookup other (remember_cancelled_demand guards path removal) = flat_lookup other guards.
Proof.
  intros guards path removal other Hdifferent. unfold remember_cancelled_demand.
  destruct (N.eqb removal 0); [reflexivity|now apply flat_set_count_other].
Qed.

Lemma remembered_demand_preserves_well_formedness : forall guards path removal,
  flat_bag_well_formedb guards = true -> path_valid path = true ->
  removal <= multiplicity_max ->
  flat_bag_well_formedb (remember_cancelled_demand guards path removal) = true.
Proof.
  intros guards path removal Hguards Hpath Hbound. unfold remember_cancelled_demand.
  destruct (N.eqb removal 0); [exact Hguards|].
  apply flat_set_count_preserves_well_formed; auto.
  apply N.max_lub; [now apply flat_lookup_well_formed_bound|exact Hbound].
Qed.

Lemma flat_record_addition_checks_path : forall delta path amount after,
  flat_record_addition delta path amount = Some after -> path_valid path = true.
Proof.
  intros delta path amount after Hrecord.
  unfold flat_record_addition, flat_record_change in Hrecord.
  destruct (path_valid path && multiplicity_valid amount) eqn:Hvalid; [|discriminate].
  now apply andb_true_iff in Hvalid as [Hpath _].
Qed.

Theorem guarded_addition_preserves_original_demand : forall before path amount after,
  guarded_delta_well_formed before -> guarded_record_addition before path amount = Some after ->
  guarded_delta_well_formed after /\
  forall endpoint, guarded_original_demand after endpoint = guarded_original_demand before endpoint.
Proof.
  intros [delta guards] path amount after [Hdelta Hguards] Hrecord.
  change (match flat_record_addition delta path amount with
    | None => None
    | Some updated => Some (CancellationGuardedDeltaState updated
        (remember_cancelled_demand guards path (flat_lookup path (flat_delta_removals delta))))
    end = Some after) in Hrecord.
  destruct (flat_record_addition delta path amount) as [updated|] eqn:Hupdate; [|discriminate].
  inversion Hrecord; subst after. clear Hrecord.
  pose proof (@partition_side_well_formed delta ChangeAddition Hdelta) as Hadd.
  pose proof (@partition_side_well_formed delta ChangeRemoval Hdelta) as Hremove.
  cbn [partition_side] in Hadd, Hremove.
  split.
  - split; cbn [guarded_delta cancellation_guards].
    + now apply (@flat_record_addition_preserves_normalization delta path amount updated).
    + apply remembered_demand_preserves_well_formedness; auto.
      * now apply (@flat_record_addition_checks_path delta path amount updated).
      * now apply flat_lookup_well_formed_bound.
  - intro endpoint. unfold guarded_original_demand.
    cbn [guarded_delta cancellation_guards].
    destruct (path_eq_dec path endpoint) as [Hequal|Hdifferent].
    + subst endpoint. rewrite remembered_demand_at_target; [|exact Hguards].
      destruct updated as [additions removals].
      apply andb_true_iff in Hadd as [_ Hadd].
      apply andb_true_iff in Hremove as [_ Hremove].
      pose proof (proj1 (@flat_record_addition_is_exact_opposite_cancellation
        delta path amount additions removals Hadd Hremove Hupdate)) as Hremaining.
      cbn [flat_delta_removals]. rewrite Hremaining.
      apply N.max_l. pose proof (N.le_max_r (flat_lookup path guards)
        (flat_lookup path (flat_delta_removals delta))). lia.
    + rewrite remembered_demand_at_other; [|exact Hdifferent].
      rewrite (proj2 (@flat_record_addition_preserves_other delta path amount updated
        endpoint Hdifferent Hupdate)). reflexivity.
Qed.

Theorem initial_guarded_removals_are_exact : forall removals,
  flat_bag_well_formedb removals = true ->
  guarded_delta_well_formed (initialize_guarded_removals removals) /\
  forall path, guarded_original_demand (initialize_guarded_removals removals) path =
    flat_lookup path removals.
Proof.
  intros removals Hvalid. split.
  - split.
    + change ((true && flat_bag_well_formedb removals) && true = true).
      now rewrite Hvalid.
    + reflexivity.
  - intro path. change (N.max 0 (flat_lookup path removals) = flat_lookup path removals).
    apply N.max_0_l.
Qed.

Fixpoint record_guarded_additions (before : CancellationGuardedDelta) (additions : list Entry)
    : option CancellationGuardedDelta :=
  match additions with
  | [] => Some before
  | (path, amount) :: tail =>
      match guarded_record_addition before path amount with
      | None => None
      | Some after => record_guarded_additions after tail
      end
  end.

Theorem finite_guarded_additions_preserve_original_demand : forall additions before after,
  guarded_delta_well_formed before -> record_guarded_additions before additions = Some after ->
  guarded_delta_well_formed after /\
  forall path, guarded_original_demand after path = guarded_original_demand before path.
Proof.
  induction additions as [|[path amount] tail IH]; intros before after Hvalid Hrecord; simpl in Hrecord.
  - inversion Hrecord; subst after. auto.
  - destruct (guarded_record_addition before path amount) as [next|] eqn:Hnext; [|discriminate].
    destruct (@guarded_addition_preserves_original_demand before path amount next Hvalid Hnext)
      as [Hnext_valid Hnext_demand].
    destruct (IH next after Hnext_valid Hrecord) as [Hafter_valid Hafter_demand].
    split; [exact Hafter_valid|]. intro endpoint. now rewrite Hafter_demand, Hnext_demand.
Qed.

Theorem guards_and_net_removals_check_full_demand : forall count state,
  ((forall path, flat_lookup path (cancellation_guards state) <= count path) /\
   (forall path, flat_lookup path (flat_delta_removals (guarded_delta state)) <= count path)) <->
  (forall path, guarded_original_demand state path <= count path).
Proof.
  intros count state. unfold guarded_original_demand. split.
  - intros [Hguard Hnet] path. apply N.max_lub; auto.
  - intro Hmax. split; intro path; specialize (Hmax path).
    + eapply N.le_trans; [apply N.le_max_l|exact Hmax].
    + eapply N.le_trans; [apply N.le_max_r|exact Hmax].
Qed.

Definition atomic_guarded_addition before path amount :=
  match guarded_record_addition before path amount with
  | Some after => after
  | None => before
  end.

Theorem failed_guarded_addition_preserves_both_authorities : forall before path amount,
  guarded_record_addition before path amount = None ->
  atomic_guarded_addition before path amount = before.
Proof. intros before path amount Hfailure. unfold atomic_guarded_addition. now rewrite Hfailure. Qed.

(** Closed capstones used by the proof gate. *)
Print Assumptions constructed_candidate_is_codec_exact.
Print Assumptions encoded_candidate_construction_is_decoder_independent.
Print Assumptions admitted_lazy_candidate_interpretation_refines_eager.
Print Assumptions sort_candidates_is_permutation.
Print Assumptions sort_candidates_is_strongly_sorted.
Print Assumptions candidate_sort_always_passes_order_check.
Print Assumptions ordered_coherent_duplicate_check_implies_global_identity_uniqueness.
Print Assumptions candidate_catalog_depends_only_on_requested_bag.
Print Assumptions catalog_set_ignores_unrequested_authority.
Print Assumptions flat_record_addition_preserves_normalization.
Print Assumptions flat_record_removal_preserves_normalization.
Print Assumptions parsed_relative_offset_is_exact.
Print Assumptions parsed_identity_offset_is_exact.
Print Assumptions flat_record_removal_is_exact_opposite_cancellation.
Print Assumptions flat_record_addition_preserves_other.
Print Assumptions flat_record_removal_preserves_other.
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
Print Assumptions fresh_partition_refines_independent_mass.
Print Assumptions partition_parsed_marker_domain.
Print Assumptions parsed_delta_mass_matches_flat_lookup.
Print Assumptions normalized_parsed_partition_exists.
Print Assumptions grouped_partition_preserves_both_flat_sides.
Print Assumptions grouped_partition_has_exact_source_endpoints.
Print Assumptions normalized_partition_refusal_is_exactly_parse_refusal.
Print Assumptions strict_flat_preflight_reflects_feasibility.
Print Assumptions strict_flat_preflight_is_extensional.
Print Assumptions strict_flat_preflight_ignores_unrelated_counts.
Print Assumptions preparation_ready_has_strict_commit_delta.
Print Assumptions preparation_ready_is_bound_to_supplied_input.
Print Assumptions absent_fresh_flag_does_not_change_feasibility.
Print Assumptions preparation_ready_checks_actual_input_counts.
Print Assumptions guarded_addition_preserves_original_demand.
Print Assumptions initial_guarded_removals_are_exact.
Print Assumptions finite_guarded_additions_preserve_original_demand.
Print Assumptions guards_and_net_removals_check_full_demand.
Print Assumptions failed_guarded_addition_preserves_both_authorities.

End TwoLevelRSpaceMatchPreparation.
