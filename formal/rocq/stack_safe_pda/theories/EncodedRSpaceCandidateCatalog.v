From Stdlib Require Import Bool List Arith NArith Lia
  Structures.OrdersEx Sorting.Mergesort Sorting.Permutation Sorting.Sorted.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority
  RhoPathV1Datum
  TwoLevelRSpaceMatchPreparation.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import RhoPathV1Datum.
Import TwoLevelRSpaceMatchPreparation.
Set Implicit Arguments.

Module EncodedRSpaceCandidateCatalog.

Definition encoded_identity (candidate : EncodedMatchCandidate) : Path :=
  datum_view_datum_identity (encoded_candidate_view candidate).

Definition encoded_beforeb (left right : EncodedMatchCandidate) : bool :=
  match path_compare (encoded_candidate_digest left) (encoded_candidate_digest right) with
  | Lt => true
  | Eq => path_ltb (encoded_identity left) (encoded_identity right)
  | Gt => false
  end.

Fixpoint insert_encoded (candidate : EncodedMatchCandidate)
    (ordered : list EncodedMatchCandidate) : list EncodedMatchCandidate :=
  match ordered with
  | [] => [candidate]
  | head :: tail =>
      if encoded_beforeb candidate head then candidate :: ordered
      else head :: insert_encoded candidate tail
  end.

Fixpoint sort_encoded (candidates : list EncodedMatchCandidate)
    : list EncodedMatchCandidate :=
  match candidates with
  | [] => []
  | head :: tail => insert_encoded head (sort_encoded tail)
  end.

Fixpoint encoded_unique_identityb (candidates : list EncodedMatchCandidate) : bool :=
  match candidates with
  | [] | [_] => true
  | first :: ((second :: _) as tail) =>
      negb (path_eqb (encoded_candidate_digest first) (encoded_candidate_digest second) &&
        path_eqb (encoded_identity first) (encoded_identity second)) &&
      encoded_unique_identityb tail
  end.

Fixpoint interpret_encoded_list (codec : CanonicalDatumCodec)
    (candidates : list EncodedMatchCandidate) : BuildResult (list MatchCandidate) :=
  match candidates with
  | [] => BuildOk []
  | head :: tail =>
      match interpret_encoded_candidate codec head with
      | BuildRefused reason => BuildRefused reason
      | BuildOk candidate =>
          match interpret_encoded_list codec tail with
          | BuildRefused reason => BuildRefused reason
          | BuildOk rest => BuildOk (candidate :: rest)
          end
      end
  end.

Definition represents (codec : CanonicalDatumCodec)
    (encoded : EncodedMatchCandidate) (semantic : MatchCandidate) : Prop :=
  interpret_encoded_candidate codec encoded = BuildOk semantic.

Lemma interpretation_retains_encoded_fields : forall codec encoded semantic,
  represents codec encoded semantic ->
  candidate_digest semantic = encoded_candidate_digest encoded /\
  candidate_canonical_identity semantic = encoded_identity encoded /\
  candidate_channel_marker semantic = decoded_view_marker (encoded_candidate_view encoded) /\
  candidate_relative_datum semantic = decoded_view_relative (encoded_candidate_view encoded).
Proof.
  intros codec encoded semantic Hrep.
  unfold represents, interpret_encoded_candidate in Hrep.
  destruct (codec_decode_and_reencode_identity codec
    (datum_view_datum_identity (encoded_candidate_view encoded))) as [decoded|];
    [|discriminate].
  destruct (path_eqb (decoded_datum_reencoded_identity decoded)
    (datum_view_datum_identity (encoded_candidate_view encoded))); [|discriminate].
  destruct (path_eqb (decoded_datum_structure decoded)
    (datum_view_datum_structure (encoded_candidate_view encoded))); [|discriminate].
  inversion Hrep. repeat split; reflexivity.
Qed.

Lemma represented_comparison_is_exact : forall codec left right lhs rhs,
  represents codec left lhs -> represents codec right rhs ->
  encoded_beforeb left right = candidate_beforeb lhs rhs.
Proof.
  intros codec left right lhs rhs Hl Hr.
  destruct (interpretation_retains_encoded_fields Hl) as [Hld [Hli _]].
  destruct (interpretation_retains_encoded_fields Hr) as [Hrd [Hri _]].
  unfold encoded_beforeb, candidate_beforeb.
  now rewrite Hld, Hli, Hrd, Hri.
Qed.

Lemma insertion_preserves_interpretation : forall codec encoded semantic left right,
  represents codec encoded semantic -> Forall2 (represents codec) left right ->
  Forall2 (represents codec) (insert_encoded encoded left)
    (insert_candidate semantic right).
Proof.
  intros codec encoded semantic left right Hrep Hlist.
  induction Hlist as [|head candidate tail rest Hhead Htail IH]; simpl.
  - now constructor.
  - rewrite (@represented_comparison_is_exact codec encoded head semantic candidate
      Hrep Hhead).
    destruct (candidate_beforeb semantic candidate).
    + constructor; [exact Hrep|now constructor].
    + now constructor.
Qed.

Theorem sorting_commutes_with_lazy_interpretation : forall codec encoded semantic,
  Forall2 (represents codec) encoded semantic ->
  Forall2 (represents codec) (sort_encoded encoded) (sort_candidates semantic).
Proof.
  intros codec encoded semantic Hlist. induction Hlist; simpl; [constructor|].
  now apply insertion_preserves_interpretation.
Qed.

Lemma list_interpretation_is_exact : forall codec encoded semantic,
  Forall2 (represents codec) encoded semantic ->
  interpret_encoded_list codec encoded = BuildOk semantic.
Proof.
  intros codec encoded semantic Hlist. induction Hlist; simpl; [reflexivity|].
  unfold represents in H. now rewrite H, IHHlist.
Qed.

Lemma duplicate_checks_commute_with_interpretation : forall codec encoded semantic,
  Forall2 (represents codec) encoded semantic ->
  encoded_unique_identityb encoded = catalog_unique_identityb semantic.
Proof.
  intros codec encoded semantic Hlist.
  induction Hlist as [|head candidate tail rest Hhead Htail IH]; [reflexivity|].
  destruct Htail as [|next next_candidate tail rest Hnext Hrest]; [reflexivity|].
  change (negb (path_eqb (encoded_candidate_digest head) (encoded_candidate_digest next) &&
    path_eqb (encoded_identity head) (encoded_identity next)) &&
    encoded_unique_identityb (next :: tail) =
    negb (path_eqb (candidate_digest candidate) (candidate_digest next_candidate) &&
    path_eqb (candidate_canonical_identity candidate)
      (candidate_canonical_identity next_candidate)) &&
    catalog_unique_identityb (next_candidate :: rest)).
  destruct (interpretation_retains_encoded_fields Hhead) as [Hhd [Hhi _]].
  destruct (interpretation_retains_encoded_fields Hnext) as [Hnd [Hni _]].
  now rewrite Hhd, Hhi, Hnd, Hni, IH.
Qed.

Fixpoint build_unsorted_encoded (digest : Path -> Path)
    (channel : Path) (contents : ChannelBag)
    : BuildResult (list EncodedMatchCandidate) :=
  match contents with
  | [] => BuildOk []
  | (relative, multiplicity) :: tail =>
      let absolute := channel ++ relative in
      match construct_encoded_candidate_at digest (StoredAddress absolute) absolute multiplicity with
      | BuildRefused reason => BuildRefused reason
      | BuildOk candidate =>
          let view := encoded_candidate_view candidate in
          if path_eqb (decoded_view_marker view) channel &&
              path_eqb (decoded_view_relative view) relative then
            match build_unsorted_encoded digest channel tail with
            | BuildRefused reason => BuildRefused reason
            | BuildOk candidates => BuildOk (candidate :: candidates)
            end
          else BuildRefused (RefuseMalformedStoredPath absolute)
      end
  end.

Definition channel_canonically_admitted (codec : CanonicalDatumCodec)
    (channel : Path) (contents : ChannelBag) : Prop :=
  Forall (fun entry => canonical_candidate_admitted codec (channel ++ fst entry)) contents.

Lemma admitted_encoded_candidate_has_interpretation : forall codec address absolute amount encoded,
  canonical_candidate_admitted codec absolute ->
  construct_encoded_candidate_at (codec_digest_raw_identity codec)
    address absolute amount = BuildOk encoded ->
  exists semantic, represents codec encoded semantic.
Proof.
  intros codec address absolute amount encoded Hadmit Hencoded.
  unfold construct_encoded_candidate_at in Hencoded.
  destruct (parse_absolute_datum absolute) as [view|] eqn:Hparse; [|discriminate].
  destruct (digest_width_is_exact
    (codec_digest_raw_identity codec (datum_view_datum_identity view))); [|discriminate].
  destruct (multiplicity_valid amount); [|discriminate].
  inversion Hencoded; subst encoded.
  destruct (Hadmit view Hparse) as [decoded [Hdecode [Hidentity Hstructure]]].
  unfold represents, interpret_encoded_candidate; simpl.
  rewrite Hdecode, Hidentity, Hstructure, !path_eqb_reflexive.
  eexists. reflexivity.
Qed.

Theorem admitted_unsorted_encoded_catalog_refines_semantic : forall codec channel contents encoded,
  channel_canonically_admitted codec channel contents ->
  build_unsorted_encoded (codec_digest_raw_identity codec) channel contents = BuildOk encoded ->
  exists semantic,
    build_unsorted_catalog codec channel contents = BuildOk semantic /\
    Forall2 (represents codec) encoded semantic.
Proof.
  intros codec channel contents.
  induction contents as [|[relative amount] tail IH]; intros encoded Hadmit Hbuild;
    cbn [build_unsorted_encoded] in Hbuild.
  - inversion Hbuild; subst encoded. exists []. split; [reflexivity|constructor].
  - inversion Hadmit as [|entry entries Hentry Htail]; subst.
    destruct (construct_encoded_candidate_at (codec_digest_raw_identity codec)
      (StoredAddress (channel ++ relative)) (channel ++ relative) amount)
      as [head|reason] eqn:Hhead; [|discriminate].
    destruct (path_eqb (decoded_view_marker (encoded_candidate_view head)) channel &&
      path_eqb (decoded_view_relative (encoded_candidate_view head)) relative)
      eqn:Hunder; [|discriminate].
    destruct (build_unsorted_encoded (codec_digest_raw_identity codec) channel tail)
      as [rest|reason'] eqn:Hrest; [|discriminate].
    inversion Hbuild; subst encoded.
    destruct (@admitted_encoded_candidate_has_interpretation codec
      (StoredAddress (channel ++ relative)) (channel ++ relative) amount head Hentry Hhead)
      as [candidate Hcandidate].
    destruct (IH rest Htail eq_refl) as [semantic [Hsemantic Hrelation]].
    exists (candidate :: semantic). split; [|now constructor].
    simpl. unfold construct_stored_candidate.
    rewrite <- (@admitted_lazy_candidate_interpretation_refines_eager codec
      (StoredAddress (channel ++ relative)) (channel ++ relative) amount Hentry).
    rewrite Hhead. unfold represents in Hcandidate. rewrite Hcandidate.
    destruct (interpretation_retains_encoded_fields Hcandidate) as [_ [_ [Hmarker Hrelative]]].
    now rewrite Hmarker, Hrelative, Hunder, Hsemantic.
Qed.

Theorem admitted_unsorted_encoded_refusal_refines_semantic : forall codec channel contents reason,
  channel_canonically_admitted codec channel contents ->
  build_unsorted_encoded (codec_digest_raw_identity codec) channel contents = BuildRefused reason ->
  build_unsorted_catalog codec channel contents = BuildRefused reason.
Proof.
  intros codec channel contents.
  induction contents as [|[relative amount] tail IH]; intros reason Hadmit Hbuild;
    cbn [build_unsorted_encoded] in Hbuild; [discriminate|].
  inversion Hadmit as [|entry entries Hentry Htail]; subst.
  cbn [build_unsorted_catalog]. unfold construct_stored_candidate.
  rewrite <- (@admitted_lazy_candidate_interpretation_refines_eager codec
    (StoredAddress (channel ++ relative)) (channel ++ relative) amount Hentry).
  destruct (construct_encoded_candidate_at (codec_digest_raw_identity codec)
    (StoredAddress (channel ++ relative)) (channel ++ relative) amount)
    as [head|failure] eqn:Hhead; [|inversion Hbuild; reflexivity].
  destruct (@admitted_encoded_candidate_has_interpretation codec
    (StoredAddress (channel ++ relative)) (channel ++ relative) amount head Hentry Hhead)
    as [candidate Hcandidate].
  unfold represents in Hcandidate. rewrite Hcandidate.
  destruct (interpretation_retains_encoded_fields Hcandidate) as [_ [_ [Hmarker Hrelative]]].
  rewrite Hmarker, Hrelative.
  destruct (path_eqb (decoded_view_marker (encoded_candidate_view head)) channel &&
    path_eqb (decoded_view_relative (encoded_candidate_view head)) relative);
    [|inversion Hbuild; reflexivity].
  destruct (build_unsorted_encoded (codec_digest_raw_identity codec) channel tail)
    as [rest|failure] eqn:Hrest; [discriminate|].
  inversion Hbuild; subst failure.
  now rewrite (IH reason Htail eq_refl).
Qed.

Record EncodedCatalog : Type := EncodedCatalogState {
  encoded_catalog_channel : Path;
  encoded_catalog_loaded : bool;
  encoded_catalog_entries : list EncodedMatchCandidate
}.

Definition finish_encoded_catalog (channel : Path) (loaded : bool)
    (raw : list EncodedMatchCandidate) : BuildResult EncodedCatalog :=
  let ordered := sort_encoded raw in
  if encoded_unique_identityb ordered then
    BuildOk (EncodedCatalogState channel loaded ordered)
  else
    match ordered with
    | first :: _ => BuildRefused (RefuseDuplicateDatumIdentity channel (encoded_identity first))
    | [] => BuildRefused (RefuseDuplicateDatumIdentity channel [])
    end.

Definition finish_semantic_catalog (channel : Path) (loaded : bool)
    (raw : list MatchCandidate) : BuildResult CandidateCatalog :=
  let ordered := sort_candidates raw in
  if catalog_unique_identityb ordered then
    if catalog_orderedb ordered then BuildOk (CandidateCatalogState channel loaded ordered)
    else BuildRefused (RefuseCatalogOrder channel)
  else
    match ordered with
    | first :: _ => BuildRefused
        (RefuseDuplicateDatumIdentity channel (candidate_canonical_identity first))
    | [] => BuildRefused (RefuseDuplicateDatumIdentity channel [])
    end.

Definition interpret_catalog_result (codec : CanonicalDatumCodec)
    (result : BuildResult EncodedCatalog) : BuildResult CandidateCatalog :=
  match result with
  | BuildRefused reason => BuildRefused reason
  | BuildOk catalog =>
      match interpret_encoded_list codec (encoded_catalog_entries catalog) with
      | BuildRefused reason => BuildRefused reason
      | BuildOk semantic => BuildOk (CandidateCatalogState
          (encoded_catalog_channel catalog) (encoded_catalog_loaded catalog) semantic)
      end
  end.

Lemma final_catalog_construction_commutes_with_interpretation : forall
    codec channel loaded encoded semantic,
  Forall2 (represents codec) encoded semantic ->
  interpret_catalog_result codec (finish_encoded_catalog channel loaded encoded) =
    finish_semantic_catalog channel loaded semantic.
Proof.
  intros codec channel loaded encoded semantic Hrep.
  pose proof (sorting_commutes_with_lazy_interpretation Hrep) as Hsorted.
  unfold finish_encoded_catalog, finish_semantic_catalog.
  rewrite (duplicate_checks_commute_with_interpretation Hsorted),
    candidate_sort_always_passes_order_check.
  destruct (catalog_unique_identityb (sort_candidates semantic)).
  - cbn [interpret_catalog_result encoded_catalog_entries encoded_catalog_channel
      encoded_catalog_loaded].
    now rewrite (list_interpretation_is_exact Hsorted).
  - destruct Hsorted as [|first candidate tail rest Hfirst Htail]; [reflexivity|].
    cbn [interpret_catalog_result].
    destruct (interpretation_retains_encoded_fields Hfirst) as [_ [Hidentity _]].
    now rewrite Hidentity.
Qed.

Definition observed_channel_contents (observed : TwoLevelAuthority) (channel : Path)
    : ChannelBag :=
  match lookup_channel channel observed with Some bag => bag | None => [] end.

Definition build_encoded_catalog (digest : Path -> Path)
    (observed : TwoLevelAuthority) (channel : Path) : BuildResult EncodedCatalog :=
  if datum_channel_marker_validb channel then
    match build_unsorted_encoded digest channel (observed_channel_contents observed channel) with
    | BuildRefused reason => BuildRefused reason
    | BuildOk raw => finish_encoded_catalog channel
        (match lookup_channel channel observed with Some _ => true | None => false end) raw
    end
  else BuildRefused (RefuseMalformedStoredPath channel).

Theorem complete_lazy_catalog_refines_semantic_catalog : forall codec observed channel,
  channel_canonically_admitted codec channel (observed_channel_contents observed channel) ->
  interpret_catalog_result codec
    (build_encoded_catalog (codec_digest_raw_identity codec) observed channel) =
    build_candidate_catalog codec observed channel.
Proof.
  intros codec observed channel Hadmit.
  unfold build_encoded_catalog, build_candidate_catalog.
  destruct (datum_channel_marker_validb channel); [|reflexivity].
  fold (observed_channel_contents observed channel).
  destruct (build_unsorted_encoded (codec_digest_raw_identity codec)
    channel (observed_channel_contents observed channel)) as [raw|reason] eqn:Hraw.
  - destruct (@admitted_unsorted_encoded_catalog_refines_semantic codec channel
      (observed_channel_contents observed channel) raw Hadmit Hraw)
      as [semantic [Hsemantic Hrep]].
    rewrite Hsemantic.
    exact (@final_catalog_construction_commutes_with_interpretation codec channel
      (match lookup_channel channel observed with Some _ => true | None => false end)
      raw semantic Hrep).
  - rewrite (@admitted_unsorted_encoded_refusal_refines_semantic codec channel
      (observed_channel_contents observed channel) reason Hadmit Hraw).
    reflexivity.
Qed.

Theorem encoded_catalog_observes_only_requested_bag : forall digest left right channel,
  lookup_channel channel left = lookup_channel channel right ->
  build_encoded_catalog digest left channel = build_encoded_catalog digest right channel.
Proof.
  intros digest left right channel Hagree.
  unfold build_encoded_catalog, observed_channel_contents. now rewrite Hagree.
Qed.

Theorem encoded_catalog_construction_is_decoder_independent : forall
    digest decode_left decode_right observed channel,
  build_encoded_catalog
    (codec_digest_raw_identity (CanonicalDatumCodecState decode_left digest)) observed channel =
  build_encoded_catalog
    (codec_digest_raw_identity (CanonicalDatumCodecState decode_right digest)) observed channel.
Proof. reflexivity. Qed.

Lemma codec_exact_candidate_has_raw_digest : forall codec candidate,
  candidate_codec_exactb codec candidate = true ->
  candidate_digest candidate =
    codec_digest_raw_identity codec (candidate_canonical_identity candidate).
Proof.
  intros codec candidate Hcodec. unfold candidate_codec_exactb in Hcodec.
  destruct (parse_absolute_datum (candidate_storage_path candidate)) as [view|];
    [|discriminate].
  destruct (codec_decode_and_reencode_identity codec
    (datum_view_datum_identity view)) as [decoded|]; [|discriminate].
  repeat rewrite andb_true_iff in Hcodec.
  decompose [and] Hcodec.
  repeat match goal with
  | H : path_eqb _ _ = true |- _ => apply path_eqb_spec in H
  end.
  congruence.
Qed.

Theorem constructed_catalog_has_global_identity_uniqueness : forall codec observed channel catalog,
  build_candidate_catalog codec observed channel = BuildOk catalog ->
  NoDup (map candidate_canonical_identity (candidate_catalog_authority catalog)).
Proof.
  intros codec observed channel catalog Hbuild.
  destruct (@built_catalog_is_sorted_and_duplicate_checked codec observed channel catalog Hbuild)
    as [Hordered Hunique].
  apply (@ordered_coherent_duplicate_check_implies_global_identity_uniqueness
    (codec_digest_raw_identity codec)); [exact Hordered|exact Hunique|].
  pose proof (@built_catalog_candidates_stay_under_requested_channel
    codec observed channel catalog Hbuild) as Hunder.
  eapply Forall_impl; [|exact Hunder].
  intros candidate [_ [_ Hcodec]]. now apply codec_exact_candidate_has_raw_digest.
Qed.

Module PositionOrder.
  Definition t := N.
  Definition leb := N.leb.
  Infix "<=?" := leb (at level 70, no associativity).
  Lemma leb_total : forall left right, leb left right = true \/ leb right left = true.
  Proof.
    intros left right. unfold leb.
    destruct (N.le_ge_cases left right); [left|right]; now apply N.leb_le.
  Qed.
End PositionOrder.

Module PositionSort := Mergesort.Sort PositionOrder.

Definition normalize_positions (positions : list N) : list N :=
  nodup N.eq_dec (PositionSort.sort positions).

Theorem normalized_positions_preserve_exact_membership : forall positions position,
  In position (normalize_positions positions) <-> In position positions.
Proof.
  intros positions position. unfold normalize_positions. rewrite nodup_In.
  split; intro Hin.
  - eapply Permutation_in; [apply Permutation_sym, PositionSort.Permuted_sort|exact Hin].
  - eapply Permutation_in; [apply PositionSort.Permuted_sort|exact Hin].
Qed.

Theorem normalized_positions_are_unique : forall positions,
  NoDup (normalize_positions positions).
Proof. intro positions. apply NoDup_nodup. Qed.

Lemma position_sort_is_nonstrictly_sorted : forall positions,
  StronglySorted N.le (PositionSort.sort positions).
Proof.
  intro positions.
  assert (Htrans : forall x y z, N.leb x y = true -> N.leb y z = true -> N.leb x z = true).
  { intros x y z Hxy Hyz. apply N.leb_le in Hxy, Hyz.
    apply N.leb_le. now transitivity y. }
  pose proof (PositionSort.StronglySorted_sort positions Htrans) as Hsorted.
  induction Hsorted as [|head tail Htail IH Hhead]; [constructor|].
  constructor; [exact IH|].
  eapply Forall_impl; [|exact Hhead]. intros item Hitem. now apply N.leb_le.
Qed.

Lemma position_nodup_preserves_nonstrict_order : forall positions,
  StronglySorted N.le positions -> StronglySorted N.le (nodup N.eq_dec positions).
Proof.
  intros positions Hsorted.
  induction Hsorted as [|head tail Htail IH Hhead]; simpl; [constructor|].
  destruct (in_dec N.eq_dec head tail); [exact IH|].
  constructor; [exact IH|].
  apply Forall_forall. intros item Hin.
  apply nodup_In in Hin. now apply (proj1 (Forall_forall _ _) Hhead).
Qed.

Lemma unique_nonstrict_positions_are_strict : forall positions,
  StronglySorted N.le positions -> NoDup positions -> StronglySorted N.lt positions.
Proof.
  intros positions Hsorted.
  induction Hsorted as [|head tail Htail IH Hhead]; intros Hunique; [constructor|].
  inversion Hunique as [|head' tail' Hnotin Htailunique]; subst.
  constructor; [now apply IH|].
  apply Forall_forall. intros item Hin.
  pose proof (proj1 (Forall_forall _ _) Hhead item Hin) as Hle.
  assert (head <> item) by (intro Heq; subst; contradiction).
  lia.
Qed.

Theorem normalized_positions_are_strictly_increasing : forall positions,
  StronglySorted N.lt (normalize_positions positions).
Proof.
  intro positions. apply unique_nonstrict_positions_are_strict.
  - apply position_nodup_preserves_nonstrict_order, position_sort_is_nonstrictly_sorted.
  - apply normalized_positions_are_unique.
Qed.

Theorem normalization_preserves_position_bounds : forall positions limit,
  Forall (fun position => (position < limit)%N) positions ->
  Forall (fun position => (position < limit)%N) (normalize_positions positions).
Proof.
  intros positions limit Hbounds. apply Forall_forall. intros position Hin.
  apply (proj1 (normalized_positions_preserve_exact_membership positions position)) in Hin.
  exact (proj1 (Forall_forall _ _) Hbounds position Hin).
Qed.

Definition position_memberb (position : N) (positions : list N) : bool :=
  existsb (N.eqb position) positions.

Lemma position_memberb_spec : forall position positions,
  position_memberb position positions = true <-> In position positions.
Proof.
  intros position positions. unfold position_memberb. rewrite existsb_exists.
  split.
  - intros [found [Hin Heq]]. apply N.eqb_eq in Heq. now subst found.
  - intro Hin. exists position. split; [exact Hin|apply N.eqb_refl].
Qed.

Theorem normalized_position_membership_test_is_exact : forall positions position,
  position_memberb position (normalize_positions positions) = position_memberb position positions.
Proof.
  intros positions position.
  destruct (position_memberb position (normalize_positions positions)) eqn:Hnormal;
    destruct (position_memberb position positions) eqn:Horiginal; try reflexivity.
  - apply position_memberb_spec in Hnormal.
    apply (proj1 (normalized_positions_preserve_exact_membership positions position)) in Hnormal.
    apply position_memberb_spec in Hnormal. congruence.
  - apply position_memberb_spec in Horiginal.
    apply (proj2 (normalized_positions_preserve_exact_membership positions position)) in Horiginal.
    apply position_memberb_spec in Horiginal. congruence.
Qed.

Definition select_catalog_ordinals (positions : list N)
    (catalog : list OrdinalCandidate) : list OrdinalCandidate :=
  filter (fun candidate => position_memberb (ordinal_candidate_index candidate) positions) catalog.

Theorem normalization_does_not_renumber_selected_candidates : forall positions catalog,
  select_catalog_ordinals (normalize_positions positions) catalog =
    select_catalog_ordinals positions catalog.
Proof.
  intros positions catalog. unfold select_catalog_ordinals.
  apply filter_ext. intro candidate. apply normalized_position_membership_test_is_exact.
Qed.

Lemma enumeration_erases_only_ordinals : forall candidates start,
  map ordinal_candidate_value (enumerate_candidates_from start candidates) = candidates.
Proof.
  intro candidates. induction candidates as [|head tail IH]; intro start; simpl;
    [reflexivity|now rewrite IH].
Qed.

Theorem post_filter_enumeration_preserves_candidate_values : forall available positions catalog,
  let retained := filter available (select_catalog_ordinals positions catalog) in
  map ordinal_candidate_value
    (enumerate_candidates_from 0 (map ordinal_candidate_value retained)) =
    map ordinal_candidate_value retained.
Proof. intros. apply enumeration_erases_only_ordinals. Qed.

Lemma mapped_ordinal_filter : forall mask catalog,
  map ordinal_candidate_value
    (filter (fun candidate => mask (ordinal_candidate_value candidate)) catalog) =
  filter mask (map ordinal_candidate_value catalog).
Proof.
  intros mask catalog. induction catalog as [|head tail IH]; simpl; [reflexivity|].
  destruct (mask (ordinal_candidate_value head)); simpl; now rewrite IH.
Qed.

Lemma selected_live_ordinal_values : forall available mask positions catalog,
  (forall candidate, In candidate catalog -> available (ordinal_candidate_value candidate) = true ->
    position_memberb (ordinal_candidate_index candidate) positions =
      mask (ordinal_candidate_value candidate)) ->
  map ordinal_candidate_value
    (filter (fun candidate => available (ordinal_candidate_value candidate))
      (select_catalog_ordinals positions catalog)) =
  filter mask (filter available (map ordinal_candidate_value catalog)).
Proof.
  intros available mask positions catalog.
  induction catalog as [|head tail IH]; intro Hagree; [reflexivity|].
  assert (Htail : forall candidate, In candidate tail ->
    available (ordinal_candidate_value candidate) = true ->
    position_memberb (ordinal_candidate_index candidate) positions =
      mask (ordinal_candidate_value candidate)).
  { intros candidate Hin. apply Hagree. now right. }
  specialize (IH Htail).
  unfold select_catalog_ordinals in IH.
  cbn [select_catalog_ordinals filter map].
  destruct (available (ordinal_candidate_value head)) eqn:Havailable.
  - rewrite (Hagree head (or_introl eq_refl) Havailable).
    destruct (mask (ordinal_candidate_value head)) eqn:Hmask; simpl;
      rewrite ?Havailable; simpl; now rewrite IH, Hmask.
  - destruct (position_memberb (ordinal_candidate_index head) positions); simpl;
      rewrite ?Havailable; simpl; exact IH.
Qed.

Theorem native_catalog_indices_and_filtered_scan_agree_on_values : forall
    available mask positions candidates original_start,
  (forall candidate, In candidate (enumerate_candidates_from original_start candidates) ->
    available (ordinal_candidate_value candidate) = true ->
    position_memberb (ordinal_candidate_index candidate) positions =
      mask (ordinal_candidate_value candidate)) ->
  map ordinal_candidate_value
    (filter (fun candidate => available (ordinal_candidate_value candidate))
      (select_catalog_ordinals (normalize_positions positions)
        (enumerate_candidates_from original_start candidates))) =
  map ordinal_candidate_value
    (filter (fun candidate => mask (ordinal_candidate_value candidate))
      (enumerate_candidates_from 0 (filter available candidates))).
Proof.
  intros available mask positions candidates original_start Hagree.
  rewrite normalization_does_not_renumber_selected_candidates,
    (@selected_live_ordinal_values available mask positions
      (enumerate_candidates_from original_start candidates) Hagree), mapped_ordinal_filter,
    !enumeration_erases_only_ordinals.
  reflexivity.
Qed.

Local Open Scope nat_scope.

Fixpoint indexed_candidates_nat (start : nat) (candidates : list MatchCandidate)
    (positions : list nat) : list MatchCandidate :=
  match positions with
  | [] => []
  | position :: tail =>
      match nth_error candidates (position - start) with
      | Some candidate => candidate :: indexed_candidates_nat start candidates tail
      | None => indexed_candidates_nat start candidates tail
      end
  end.

Fixpoint select_candidates_nat (start : nat) (positions : list nat)
    (candidates : list MatchCandidate) : list MatchCandidate :=
  match candidates with
  | [] => []
  | candidate :: tail =>
      if existsb (Nat.eqb start) positions then
        candidate :: select_candidates_nat (S start) positions tail
      else select_candidates_nat (S start) positions tail
  end.

Lemma indexed_candidates_skip_head : forall candidates head start positions,
  Forall (fun position => start < position) positions ->
  indexed_candidates_nat start (head :: candidates) positions =
    indexed_candidates_nat (S start) candidates positions.
Proof.
  intros candidates head start positions Hafter.
  induction Hafter as [|position positions Hposition Hpositions IH]; [reflexivity|].
  cbn [indexed_candidates_nat].
  replace (position - start) with (S (position - S start)) by lia.
  cbn [nth_error]. destruct (nth_error candidates (position - S start)); now rewrite IH.
Qed.

Lemma select_candidates_before_start_absent : forall candidates start position positions,
  position < start ->
  select_candidates_nat start (position :: positions) candidates =
    select_candidates_nat start positions candidates.
Proof.
  intro candidates. induction candidates as [|head tail IH];
    intros start position positions Hbefore; [reflexivity|].
  cbn [select_candidates_nat existsb].
  assert (Hneq : Nat.eqb start position = false) by (apply Nat.eqb_neq; lia).
  rewrite Hneq. cbn [orb].
  destruct (existsb (Nat.eqb start) positions); rewrite IH by lia; reflexivity.
Qed.

Lemma select_candidates_no_positions : forall candidates start,
  select_candidates_nat start [] candidates = [].
Proof.
  intro candidates. induction candidates; intro start; simpl; auto.
Qed.

Lemma indexed_candidates_are_catalog_selection : forall candidates start positions,
  StronglySorted Nat.lt positions ->
  Forall (fun position => start <= position < start + length candidates) positions ->
  indexed_candidates_nat start candidates positions =
    select_candidates_nat start positions candidates.
Proof.
  intro candidates. induction candidates as [|head tail IH]; intros start positions Hsorted Hbounds.
  - destruct positions as [|position positions]; [reflexivity|].
    inversion Hbounds; simpl in *; lia.
  - destruct positions as [|position positions].
    + cbn [indexed_candidates_nat]. symmetry. apply select_candidates_no_positions.
    + inversion Hbounds as [|position' positions' Hposition Htailbounds]; subst.
      inversion Hsorted as [|position' positions' Htailsorted Hafter]; subst.
      destruct (Nat.eq_dec start position) as [Hequal|Hdifferent].
      * subst position. cbn [indexed_candidates_nat]. rewrite Nat.sub_diag. cbn [nth_error].
        cbn [select_candidates_nat existsb]. rewrite Nat.eqb_refl. cbn [orb].
        rewrite select_candidates_before_start_absent by lia.
        f_equal. rewrite indexed_candidates_skip_head by exact Hafter.
        apply IH; [exact Htailsorted|].
        apply Forall_forall. intros position Hin.
        pose proof (proj1 (Forall_forall _ _) Hafter position Hin).
        pose proof (proj1 (Forall_forall _ _) Htailbounds position Hin).
        simpl in *. lia.
      * assert (Hgreater : start < position) by lia.
        assert (Hallafter : Forall (fun index => start < index) (position :: positions)).
        { constructor; [exact Hgreater|]. eapply Forall_impl; [|exact Hafter].
          intros index Hindex. lia. }
        rewrite indexed_candidates_skip_head by exact Hallafter.
        cbn [select_candidates_nat].
        assert (Habsent : existsb (Nat.eqb start) (position :: positions) = false).
        { destruct (existsb (Nat.eqb start) (position :: positions)) eqn:Hmember; [|reflexivity].
          apply existsb_exists in Hmember as [index [Hin Heq]].
          apply Nat.eqb_eq in Heq.
          pose proof (proj1 (Forall_forall _ _) Hallafter index Hin) as Hlt.
          change (start < index) in Hlt. exfalso. lia. }
        rewrite Habsent. apply IH; [exact Hsorted|].
        apply Forall_forall. intros index Hin.
        pose proof (proj1 (Forall_forall _ _) Hallafter index Hin).
        pose proof (proj1 (Forall_forall _ _) Hbounds index Hin).
        simpl in *. lia.
Qed.

Lemma natural_position_membership_is_exact : forall start positions,
  existsb (Nat.eqb (N.to_nat start)) (map N.to_nat positions) =
    position_memberb start positions.
Proof.
  intros start positions. induction positions as [|position tail IH]; [reflexivity|].
  cbn [map existsb position_memberb].
  assert (Heq : Nat.eqb (N.to_nat start) (N.to_nat position) = N.eqb start position).
  { destruct (N.eq_dec start position) as [->|Hdifferent].
    - now rewrite Nat.eqb_refl, N.eqb_refl.
    - assert (Hnat : Nat.eqb (N.to_nat start) (N.to_nat position) = false).
      { apply Nat.eqb_neq. intro Heq. apply Hdifferent. now apply N2Nat.inj. }
      assert (Hbinary : N.eqb start position = false) by (now apply N.eqb_neq).
      now rewrite Hnat, Hbinary. }
  now rewrite Heq, IH.
Qed.

Lemma natural_selection_is_ordinal_selection : forall candidates start positions,
  select_candidates_nat (N.to_nat start) (map N.to_nat positions) candidates =
  map ordinal_candidate_value
    (select_catalog_ordinals positions (enumerate_candidates_from start candidates)).
Proof.
  intro candidates. induction candidates as [|head tail IH]; intros start positions; [reflexivity|].
  cbn [select_candidates_nat enumerate_candidates_from select_catalog_ordinals filter map
    ordinal_candidate_index ordinal_candidate_value].
  rewrite natural_position_membership_is_exact, <- N2Nat.inj_succ.
  destruct (position_memberb start positions); cbn [map ordinal_candidate_value];
    now rewrite IH.
Qed.

Definition retrieve_catalog_positions (start : N) (candidates : list MatchCandidate)
    (positions : list N) : list MatchCandidate :=
  indexed_candidates_nat (N.to_nat start) candidates (map N.to_nat positions).

Theorem normalized_indexed_retrieval_equals_catalog_selection : forall start candidates positions,
  Forall (fun position => (start <= position < start + N.of_nat (length candidates))%N)
    positions ->
  retrieve_catalog_positions start candidates (normalize_positions positions) =
  map ordinal_candidate_value
    (select_catalog_ordinals (normalize_positions positions)
      (enumerate_candidates_from start candidates)).
Proof.
  intros start candidates positions Hbounds.
  unfold retrieve_catalog_positions.
  rewrite indexed_candidates_are_catalog_selection.
  - apply natural_selection_is_ordinal_selection.
  - pose proof (normalized_positions_are_strictly_increasing positions) as Hsorted.
    induction Hsorted as [|head tail Htail IH Hhead]; simpl; [constructor|].
    constructor; [exact IH|]. apply Forall_forall. intros index Hin.
    apply in_map_iff in Hin as [position [<- Hin]].
    pose proof (proj1 (Forall_forall _ _) Hhead position Hin). lia.
  - apply Forall_forall. intros index Hin.
    apply in_map_iff in Hin as [position [<- Hin]].
    apply (proj1 (normalized_positions_preserve_exact_membership positions position)) in Hin.
    destruct (proj1 (Forall_forall _ _) Hbounds position Hin) as [Hlower Hupper].
    apply N.compare_lt_iff in Hupper.
    rewrite N2Nat.inj_compare, N2Nat.inj_add, Nat2N.id in Hupper.
    apply Nat.compare_lt_iff in Hupper. split; [lia|exact Hupper].
Qed.

Theorem normalized_indexed_retrieval_refines_filtered_scan : forall
    available mask positions candidates original_start,
  Forall (fun position =>
    (original_start <= position < original_start + N.of_nat (length candidates))%N) positions ->
  (forall candidate, In candidate (enumerate_candidates_from original_start candidates) ->
    available (ordinal_candidate_value candidate) = true ->
    position_memberb (ordinal_candidate_index candidate) positions =
      mask (ordinal_candidate_value candidate)) ->
  filter available
    (retrieve_catalog_positions original_start candidates (normalize_positions positions)) =
  map ordinal_candidate_value
    (filter (fun candidate => mask (ordinal_candidate_value candidate))
      (enumerate_candidates_from 0 (filter available candidates))).
Proof.
  intros available mask positions candidates original_start Hbounds Hagree.
  rewrite normalized_indexed_retrieval_equals_catalog_selection by exact Hbounds.
  rewrite <- mapped_ordinal_filter.
  now apply native_catalog_indices_and_filtered_scan_agree_on_values.
Qed.

Local Close Scope nat_scope.

Definition certified_masked_scan (full : list OrdinalCandidate)
    (matcher_accepts : OrdinalCandidate -> bool) (mask : MatchCandidate -> bool)
    (complete : forall candidate, In candidate full -> matcher_accepts candidate = true ->
      mask (ordinal_candidate_value candidate) = true)
    : CertifiedCandidateScan full matcher_accepts.
Proof.
  refine {| certified_scan_projection :=
    filter (fun candidate => mask (ordinal_candidate_value candidate)) full;
    certified_scan_is_ordered_subsequence := _;
    certified_scan_is_match_complete := _ |}.
  - eexists. reflexivity.
  - intros candidate Hin Hmatched. apply filter_In. split; [exact Hin|].
    now apply complete.
Defined.

Print Assumptions normalized_positions_preserve_exact_membership.
Print Assumptions normalized_positions_are_unique.
Print Assumptions normalized_positions_are_strictly_increasing.
Print Assumptions normalization_preserves_position_bounds.
Print Assumptions normalization_does_not_renumber_selected_candidates.
Print Assumptions post_filter_enumeration_preserves_candidate_values.
Print Assumptions native_catalog_indices_and_filtered_scan_agree_on_values.
Print Assumptions normalized_indexed_retrieval_equals_catalog_selection.
Print Assumptions normalized_indexed_retrieval_refines_filtered_scan.
Print Assumptions certified_masked_scan.
Print Assumptions sorting_commutes_with_lazy_interpretation.
Print Assumptions duplicate_checks_commute_with_interpretation.
Print Assumptions admitted_unsorted_encoded_catalog_refines_semantic.
Print Assumptions admitted_unsorted_encoded_refusal_refines_semantic.
Print Assumptions complete_lazy_catalog_refines_semantic_catalog.
Print Assumptions encoded_catalog_observes_only_requested_bag.
Print Assumptions encoded_catalog_construction_is_decoder_independent.
Print Assumptions constructed_catalog_has_global_identity_uniqueness.

End EncodedRSpaceCandidateCatalog.
