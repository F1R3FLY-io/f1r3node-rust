From Stdlib Require Import Bool List Arith Lia ZArith Sorting.Permutation Sorting.Sorted.
From StackSafePDA Require Import PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority RhoPathV1Datum TwoLevelRSpaceContinuationAuthority
  TwoLevelRSpaceReceiveAccelerator.
Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceContinuationAuthority.
Import OrderedReceivePositions.
Import TwoLevelRSpaceReceiveAccelerator.

Open Scope nat_scope.

Module TwoLevelRSpaceReceiveAcceleratorChannelScope.

Definition channel_scope_complete (w : World) (scope : list GroupKey)
    (channel : Path) : Prop :=
  forall group, In channel group -> targets_at w group <> [] -> In group scope.

Definition JoinView : Type := Path -> option (list GroupKey).

Definition join_view_exact (w : World) (view : JoinView) : Prop :=
  forall channel joins, view channel = Some joins ->
    same_members joins (joins_of (world_roots w) (world_installed w) channel).

Definition exact_join_view (w : World) : JoinView :=
  fun channel => Some (joins_of (world_roots w) (world_installed w) channel).

Definition installed_only_entries (w : World) (scope : list GroupKey)
    : list Occurrence :=
  flat_map (fun group =>
    if in_dec group_eq_dec group scope then []
    else flat_map (target_occurrences group)
      (installed_targets (world_installed w) group))
    (map fst (world_installed w)).

Definition production_index (w : World) (scope : list GroupKey)
    : list Occurrence :=
  scoped_entries w scope ++ installed_only_entries w scope.

Definition production_entries (w : World) (scope : list GroupKey)
    (entries : list Occurrence) : Prop :=
  same_members entries (production_index w scope).

Definition lookup_in_scope (scope : list GroupKey) (entries : list Occurrence)
    (channel : Path) : Prop :=
  forall occurrence, In occurrence (accelerator_lookup entries channel) ->
    In (occurrence_group occurrence) scope.

Definition in_scopeb (scope : list GroupKey) (group : GroupKey) : bool :=
  if in_dec group_eq_dec group scope then true else false.

Definition joins_in_scopeb (scope joins : list GroupKey) : bool :=
  forallb (in_scopeb scope) joins.

Definition occurrences_in_scopeb (scope : list GroupKey)
    (occurrences : list Occurrence) : bool :=
  forallb (fun occurrence => in_scopeb scope (occurrence_group occurrence))
    occurrences.

Definition lookup_in_scopeb (scope : list GroupKey) (entries : list Occurrence)
    (channel : Path) : bool :=
  occurrences_in_scopeb scope (accelerator_lookup entries channel).

Definition channel_scope_check (view : JoinView) (scope : list GroupKey)
    (entries : list Occurrence) (channel : Path) : bool :=
  match view channel with
  | Some joins =>
      joins_in_scopeb scope joins && lookup_in_scopeb scope entries channel
  | None => false
  end.

Inductive ChannelClass : Type :=
| ChannelAbsent
| ChannelBuilding
| ChannelForeign
| ChannelStale
| ChannelIncomplete
| ChannelCoherent.

Definition channel_classify (w : World) (view : JoinView)
    (accelerator : AcceleratorState) (channel : Path) : ChannelClass :=
  match accelerator with
  | AcceleratorAbsent => ChannelAbsent
  | AcceleratorBuilding _ _ _ => ChannelBuilding
  | AcceleratorPublished bound lineage scope entries artifacts =>
      if semantic_roots_eq_dec lineage (world_manifest w) then
        if EqualityEpoch.eq_dec (revision_generation bound)
            (revision_generation (world_revision w)) then
          if revision_eq_dec bound (world_revision w) then
            if artifacts_eq_dec artifacts (artifacts_at bound) then
              if channel_scope_check view scope entries channel
              then ChannelCoherent else ChannelIncomplete
            else ChannelStale
          else ChannelStale
        else ChannelForeign
      else ChannelForeign
  end.

Definition channel_answer (w : World) (view : JoinView)
    (accelerator : AcceleratorState) (channel : Path) : Answer :=
  match accelerator with
  | AcceleratorPublished _ _ _ entries _ =>
      match channel_classify w view accelerator channel with
      | ChannelCoherent => ExactAnswer (accelerator_lookup entries channel)
      | _ => FallbackAnswer (derive_world w channel)
      end
  | _ => FallbackAnswer (derive_world w channel)
  end.

Definition channel_read (w : World) (view : JoinView)
    (accelerator : AcceleratorState) (channel : Path) : list Occurrence :=
  answer_occurrences (channel_answer w view accelerator channel).

Lemma in_scopeb_spec : forall scope group,
  in_scopeb scope group = true <-> In group scope.
Proof.
  intros scope group. unfold in_scopeb.
  destruct (in_dec group_eq_dec group scope) as [Hin|Hnotin].
  - split; [intros _; exact Hin|intros _; reflexivity].
  - split; [intro Hfalse; discriminate|intro Hin; contradiction].
Qed.

Lemma joins_in_scopeb_spec : forall scope joins,
  joins_in_scopeb scope joins = true <->
  (forall group, In group joins -> In group scope).
Proof.
  intros scope joins. unfold joins_in_scopeb. rewrite forallb_forall.
  split.
  - intros Hall group Hin. apply in_scopeb_spec. apply Hall. exact Hin.
  - intros Hall group Hin. apply in_scopeb_spec. apply Hall. exact Hin.
Qed.

Lemma lookup_in_scopeb_spec : forall scope entries channel,
  lookup_in_scopeb scope entries channel = true <->
  lookup_in_scope scope entries channel.
Proof.
  intros scope entries channel.
  unfold lookup_in_scopeb, occurrences_in_scopeb, lookup_in_scope.
  rewrite forallb_forall. split.
  - intros Hall occurrence Hin. specialize (Hall occurrence Hin).
    cbv beta in Hall. apply in_scopeb_spec. exact Hall.
  - intros Hall occurrence Hin. cbv beta. apply in_scopeb_spec.
    apply Hall. exact Hin.
Qed.

Lemma channel_scope_check_spec : forall view scope entries channel,
  channel_scope_check view scope entries channel = true <->
  exists joins, view channel = Some joins /\
    (forall group, In group joins -> In group scope) /\
    lookup_in_scope scope entries channel.
Proof.
  intros view scope entries channel. unfold channel_scope_check.
  destruct (view channel) as [joins|]; cbv beta iota.
  - rewrite andb_true_iff, joins_in_scopeb_spec, lookup_in_scopeb_spec.
    split.
    + intros [Hjoins Hlookup]. exists joins.
      split; [reflexivity|split; [exact Hjoins|exact Hlookup]].
    + intros [found [Hfound [Hjoins Hlookup]]].
      injection Hfound as Hsame. subst found.
      split; [exact Hjoins|exact Hlookup].
  - split; [intro Hfalse; discriminate|].
    intros [found [Hfound _]]. discriminate.
Qed.

Lemma channel_coherent_spec : forall w view bound lineage scope entries
    artifacts channel,
  channel_classify w view
    (AcceleratorPublished bound lineage scope entries artifacts) channel =
    ChannelCoherent ->
  lineage = world_manifest w /\ bound = world_revision w /\
  artifacts = artifacts_at bound /\
  exists joins, view channel = Some joins /\
    (forall group, In group joins -> In group scope) /\
    lookup_in_scope scope entries channel.
Proof.
  intros w view bound lineage scope entries artifacts channel Hclass.
  cbn [channel_classify] in Hclass. revert Hclass.
  destruct (semantic_roots_eq_dec lineage (world_manifest w)) as [Hlineage|];
    [|intro Hfalse; discriminate].
  destruct (EqualityEpoch.eq_dec (revision_generation bound)
    (revision_generation (world_revision w))) as [_|];
    [|intro Hfalse; discriminate].
  destruct (revision_eq_dec bound (world_revision w)) as [Hbound|];
    [|intro Hfalse; discriminate].
  destruct (artifacts_eq_dec artifacts (artifacts_at bound)) as [Hartifacts|];
    [|intro Hfalse; discriminate].
  destruct (channel_scope_check view scope entries channel) eqn:Hcheck;
    [|intro Hfalse; discriminate].
  intros _. split; [exact Hlineage|]. split; [exact Hbound|].
  split; [exact Hartifacts|]. apply channel_scope_check_spec. exact Hcheck.
Qed.

Lemma exact_channel_answer_parts : forall w view bound lineage scope entries
    artifacts channel occurrences,
  channel_answer w view
    (AcceleratorPublished bound lineage scope entries artifacts) channel =
    ExactAnswer occurrences ->
  channel_classify w view
    (AcceleratorPublished bound lineage scope entries artifacts) channel =
    ChannelCoherent /\
  occurrences = accelerator_lookup entries channel.
Proof.
  intros w view bound lineage scope entries artifacts channel occurrences
    Hanswer.
  cbn [channel_answer] in Hanswer. revert Hanswer.
  destruct (channel_classify w view
    (AcceleratorPublished bound lineage scope entries artifacts) channel);
    intro Hanswer; try discriminate.
  injection Hanswer as Hsame. split; [reflexivity|symmetry; exact Hsame].
Qed.

Lemma exact_join_view_is_exact : forall w, join_view_exact w (exact_join_view w).
Proof.
  intros w channel joins Hview. unfold exact_join_view in Hview.
  injection Hview as Hsame. subst joins. apply same_members_refl.
Qed.

Lemma production_index_is_production : forall w scope,
  production_entries w scope (production_index w scope).
Proof. intros w scope. apply same_members_refl. Qed.

Lemma installed_targets_in_group_targets : forall roots slots group target,
  In target (installed_targets slots group) ->
  In target (group_targets roots slots group).
Proof.
  intros roots slots group target Hin. unfold group_targets.
  apply in_app_iff. left. exact Hin.
Qed.

Lemma installed_targets_key : forall slots group target,
  In target (installed_targets slots group) -> In group (map fst slots).
Proof.
  intros slots group target. unfold installed_targets.
  destruct (group_installed slots group) as [record|] eqn:Hinstalled;
    [intros _|intros []].
  eapply assoc_lookup_some_key. exact Hinstalled.
Qed.

Lemma installed_only_entries_spec : forall w scope occurrence,
  In occurrence (installed_only_entries w scope) <->
  ~ In (occurrence_group occurrence) scope /\
  In (occurrence_target occurrence)
    (installed_targets (world_installed w) (occurrence_group occurrence)) /\
  occurrence_shape occurrence.
Proof.
  intros w scope occurrence. unfold installed_only_entries.
  rewrite in_flat_map. split.
  - intros [group [Hkey Hin]]. cbv beta in Hin. revert Hin.
    destruct (in_dec group_eq_dec group scope) as [Hscope|Hscope];
      cbv beta iota; intro Hin; [destruct Hin|].
    apply in_flat_map in Hin. destruct Hin as [target [Htarget Hin]].
    apply target_occurrences_spec in Hin.
    destruct Hin as [Hsame [Hgroup Hshape]].
    rewrite Hsame, Hgroup.
    split; [exact Hscope|split; [exact Htarget|exact Hshape]].
  - intros [Hscope [Htarget Hshape]].
    exists (occurrence_group occurrence).
    split; [eapply installed_targets_key; exact Htarget|].
    cbv beta.
    destruct (in_dec group_eq_dec (occurrence_group occurrence) scope)
      as [Hin|Hnotin]; [contradiction|cbv beta iota].
    apply in_flat_map. exists (occurrence_target occurrence).
    split; [exact Htarget|].
    apply target_occurrences_spec.
    split; [reflexivity|split; [reflexivity|exact Hshape]].
Qed.

Lemma production_lookup_is_sound : forall w scope entries channel occurrence,
  production_entries w scope entries ->
  In occurrence (accelerator_lookup entries channel) ->
  In occurrence (derive_world w channel).
Proof.
  intros w scope entries channel occurrence Hentries Hin.
  apply accelerator_lookup_spec in Hin. destruct Hin as [Hin Hchannel].
  pose proof (proj1 (Hentries occurrence) Hin) as Hindex.
  unfold production_index in Hindex. apply in_app_iff in Hindex.
  unfold derive_world. apply derive_occurrences_spec.
  split; [exact Hchannel|].
  destruct Hindex as [Hscoped|Hinstalled].
  - apply scoped_entries_world_spec in Hscoped.
    destruct Hscoped as [_ [Htarget Hshape]].
    split; [exact Htarget|exact Hshape].
  - apply installed_only_entries_spec in Hinstalled.
    destruct Hinstalled as [_ [Htarget Hshape]].
    split; [|exact Hshape].
    apply installed_targets_in_group_targets. exact Htarget.
Qed.

Lemma production_lookup_is_complete_in_scope :
    forall w scope entries channel occurrence,
  production_entries w scope entries ->
  In occurrence (derive_world w channel) ->
  In (occurrence_group occurrence) scope ->
  In occurrence (accelerator_lookup entries channel).
Proof.
  intros w scope entries channel occurrence Hentries Hderived Hscope.
  unfold derive_world in Hderived. apply derive_occurrences_spec in Hderived.
  destruct Hderived as [Hchannel [Htarget Hshape]].
  apply accelerator_lookup_spec. split; [|exact Hchannel].
  apply (proj2 (Hentries occurrence)). unfold production_index.
  apply in_app_iff. left.
  apply scoped_entries_world_spec.
  split; [exact Hscope|split; [exact Htarget|exact Hshape]].
Qed.

Lemma derived_occurrence_group : forall w channel occurrence,
  In occurrence (derive_world w channel) ->
  In channel (occurrence_group occurrence) /\
  targets_at w (occurrence_group occurrence) <> [].
Proof.
  intros w channel occurrence Hderived.
  unfold derive_world in Hderived. apply derive_occurrences_spec in Hderived.
  destruct Hderived as [Hchannel [Htarget [Hmember _]]].
  rewrite Hchannel in Hmember. split; [exact Hmember|].
  intro Hempty. unfold targets_at in Hempty. rewrite Hempty in Htarget.
  destruct Htarget.
Qed.

Lemma installed_join_has_targets : forall roots slots channel group,
  In group (installed_joins_of slots channel) ->
  group_targets roots slots group <> [].
Proof.
  intros roots slots channel group Hin.
  unfold installed_joins_of in Hin. apply filter_In in Hin.
  destruct Hin as [Hkey _].
  destruct (assoc_lookup_key_present group_eq_dec slots group Hkey)
    as [record Hrecord].
  unfold group_targets, installed_targets, group_installed.
  rewrite Hrecord. cbn [app]. discriminate.
Qed.

Lemma resting_join_has_targets : forall roots slots channel group,
  joins_cover roots ->
  In group (resting_joins_of roots channel) ->
  group_targets roots slots group <> [].
Proof.
  intros roots slots channel group Hcover Hin.
  destruct (proj1 (Hcover channel group) Hin) as [_ Hrecords].
  intro Hempty. apply Hrecords.
  unfold group_targets in Hempty. apply app_eq_nil in Hempty.
  destruct Hempty as [_ Hresting]. apply map_eq_nil in Hresting.
  exact Hresting.
Qed.

Lemma joins_have_targets : forall roots slots channel group,
  joins_cover roots ->
  In group (joins_of roots slots channel) ->
  group_targets roots slots group <> [].
Proof.
  intros roots slots channel group Hcover Hin.
  unfold joins_of in Hin. apply in_app_iff in Hin.
  destruct Hin as [Hresting|Hinstalled].
  - exact (resting_join_has_targets roots slots channel group Hcover Hresting).
  - exact (installed_join_has_targets roots slots channel group Hinstalled).
Qed.

Theorem join_coverage_completes_the_channel_scope : forall w scope channel,
  joins_cover (world_roots w) ->
  (forall group,
     In group (joins_of (world_roots w) (world_installed w) channel) ->
     In group scope) ->
  channel_scope_complete w scope channel.
Proof.
  intros w scope channel Hcover Hjoins group Hmember Htargets.
  apply Hjoins.
  assert (Hprojected : In group
    (map fst (channel_groups (world_roots w) (world_installed w) channel))).
  { apply in_map_iff. exists (group, positions group channel).
    split; [reflexivity|].
    apply channel_groups_spec.
    split; [apply receive_support_spec; exact Htargets|].
    split; [reflexivity|]. apply positions_nonempty_iff. exact Hmember. }
  apply (join_root_projection_is_exact (world_roots w) (world_installed w)
    channel group Hcover) in Hprojected.
  destruct Hprojected as [Hjoin _]. exact Hjoin.
Qed.

Theorem complete_channel_scope_lookup_is_derivation :
    forall w scope entries channel,
  channel_scope_complete w scope channel ->
  production_entries w scope entries ->
  lookup_in_scope scope entries channel /\
  same_members (accelerator_lookup entries channel) (derive_world w channel).
Proof.
  intros w scope entries channel Hcomplete Hentries.
  assert (Hsound : forall occurrence,
    In occurrence (accelerator_lookup entries channel) ->
    In occurrence (derive_world w channel)).
  { intros occurrence Hin.
    exact (production_lookup_is_sound w scope entries channel occurrence
      Hentries Hin). }
  assert (Hscope : forall occurrence,
    In occurrence (derive_world w channel) ->
    In (occurrence_group occurrence) scope).
  { intros occurrence Hderived.
    destruct (derived_occurrence_group w channel occurrence Hderived)
      as [Hmember Htargets].
    exact (Hcomplete _ Hmember Htargets). }
  split.
  - intros occurrence Hin. apply Hscope. apply Hsound. exact Hin.
  - intro occurrence. split.
    + intro Hin. exact (Hsound occurrence Hin).
    + intro Hderived.
      apply (production_lookup_is_complete_in_scope w scope entries channel
        occurrence Hentries Hderived).
      exact (Hscope occurrence Hderived).
Qed.

Theorem exact_channel_read_is_current_sound_and_group_complete :
    forall w view bound lineage scope entries artifacts channel occurrences,
  channel_answer w view
    (AcceleratorPublished bound lineage scope entries artifacts) channel =
    ExactAnswer occurrences ->
  (bound = world_revision w /\ lineage = world_manifest w /\
   artifacts = artifacts_at bound) /\
  (production_entries w scope entries ->
   (forall occurrence, In occurrence occurrences ->
      In occurrence (derive_world w channel)) /\
   (forall occurrence other, In occurrence occurrences ->
      In other (derive_world w channel) ->
      occurrence_group other = occurrence_group occurrence ->
      In other occurrences)).
Proof.
  intros w view bound lineage scope entries artifacts channel occurrences
    Hanswer.
  destruct (exact_channel_answer_parts w view bound lineage scope entries
    artifacts channel occurrences Hanswer) as [Hclass Hoccurrences].
  destruct (channel_coherent_spec w view bound lineage scope entries artifacts
    channel Hclass) as [Hlineage [Hbound [Hartifacts [joins [_ [_ Hlookup]]]]]].
  split; [split; [exact Hbound|split; [exact Hlineage|exact Hartifacts]]|].
  intro Hentries. subst occurrences. split.
  - intros occurrence Hin.
    exact (production_lookup_is_sound w scope entries channel occurrence
      Hentries Hin).
  - intros occurrence other Hin Hother Hgroup.
    apply (production_lookup_is_complete_in_scope w scope entries channel other
      Hentries Hother).
    rewrite Hgroup. exact (Hlookup occurrence Hin).
Qed.

Theorem covered_exact_channel_read_is_the_derivation :
    forall w view bound lineage scope entries artifacts channel occurrences,
  joins_cover (world_roots w) ->
  join_view_exact w view ->
  production_entries w scope entries ->
  channel_answer w view
    (AcceleratorPublished bound lineage scope entries artifacts) channel =
    ExactAnswer occurrences ->
  same_members occurrences (derive_world w channel).
Proof.
  intros w view bound lineage scope entries artifacts channel occurrences
    Hcover Hexact Hentries Hanswer.
  destruct (exact_channel_answer_parts w view bound lineage scope entries
    artifacts channel occurrences Hanswer) as [Hclass Hoccurrences].
  destruct (channel_coherent_spec w view bound lineage scope entries artifacts
    channel Hclass) as [_ [_ [_ [joins [Hview [Hjoins _]]]]]].
  subst occurrences.
  assert (Hcomplete : channel_scope_complete w scope channel).
  { apply join_coverage_completes_the_channel_scope; [exact Hcover|].
    intros group Hgroup. apply Hjoins.
    apply (proj2 (Hexact channel joins Hview group)). exact Hgroup. }
  exact (proj2 (complete_channel_scope_lookup_is_derivation w scope entries
    channel Hcomplete Hentries)).
Qed.

Theorem global_coherence_implies_channel_coherence :
    forall w view accelerator channel,
  joins_cover (world_roots w) ->
  join_view_exact w view ->
  view channel <> None ->
  accelerator_invariant w accelerator ->
  classify_accelerator w accelerator = ClassCoherent ->
  channel_classify w view accelerator channel = ChannelCoherent /\
  channel_answer w view accelerator channel =
    accelerator_answer w accelerator channel.
Proof.
  intros w view accelerator channel Hcover Hexact Hloaded Hinvariant Hclass.
  destruct accelerator as [|base scope entries|bound lineage scope entries
    artifacts]; [discriminate|discriminate|].
  destruct (classify_coherent_spec w bound lineage scope entries artifacts
    Hclass) as [Hlineage [Hbound [Hartifacts Hcovers]]].
  destruct Hinvariant as [_ Hentries].
  specialize (Hentries Hbound Hlineage).
  assert (Hjoins : exists joins, view channel = Some joins).
  { revert Hloaded. destruct (view channel) as [joins|]; intro Hloaded;
      [exists joins; reflexivity|exfalso; apply Hloaded; reflexivity]. }
  destruct Hjoins as [joins Hview].
  assert (Hcheck : channel_scope_check view scope entries channel = true).
  { apply channel_scope_check_spec. exists joins.
    split; [exact Hview|]. split.
    - intros group Hgroup. apply Hcovers.
      apply (joins_have_targets (world_roots w) (world_installed w) channel
        group Hcover).
      apply (proj1 (Hexact channel joins Hview group)). exact Hgroup.
    - intros occurrence Hin. apply accelerator_lookup_spec in Hin.
      destruct Hin as [Hin _]. apply (proj1 (Hentries occurrence)) in Hin.
      apply scoped_entries_world_spec in Hin. destruct Hin as [Hscope _].
      exact Hscope. }
  assert (Hchannel : channel_classify w view
    (AcceleratorPublished bound lineage scope entries artifacts) channel =
    ChannelCoherent).
  { cbn [channel_classify].
    destruct (semantic_roots_eq_dec lineage (world_manifest w)) as [_|Hne];
      [|contradiction].
    destruct (EqualityEpoch.eq_dec (revision_generation bound)
      (revision_generation (world_revision w))) as [_|Hne];
      [|exfalso; apply Hne; rewrite Hbound; reflexivity].
    destruct (revision_eq_dec bound (world_revision w)) as [_|Hne];
      [|contradiction].
    destruct (artifacts_eq_dec artifacts (artifacts_at bound)) as [_|Hne];
      [|contradiction].
    rewrite Hcheck. reflexivity. }
  split; [exact Hchannel|].
  unfold channel_answer, accelerator_answer. rewrite Hchannel, Hclass.
  reflexivity.
Qed.

Theorem exact_channel_read_contains_the_joins_derivation :
    forall w view bound lineage scope entries artifacts channel occurrences,
  channel_answer w view
    (AcceleratorPublished bound lineage scope entries artifacts) channel =
    ExactAnswer occurrences ->
  production_entries w scope entries ->
  join_view_exact w view ->
  forall group target,
    In group (joins_of (world_roots w) (world_installed w) channel) ->
    In channel group ->
    In target (targets_at w group) ->
    In (OccurrenceState channel group (positions group channel) target)
      occurrences.
Proof.
  intros w view bound lineage scope entries artifacts channel occurrences
    Hanswer Hentries Hexact group target Hjoin Hmember Htarget.
  destruct (exact_channel_answer_parts w view bound lineage scope entries
    artifacts channel occurrences Hanswer) as [Hclass Hoccurrences].
  destruct (channel_coherent_spec w view bound lineage scope entries artifacts
    channel Hclass) as [_ [_ [_ [joins [Hview [Hjoins _]]]]]].
  subst occurrences.
  apply (production_lookup_is_complete_in_scope w scope entries channel _
    Hentries).
  - unfold derive_world. apply derive_occurrences_spec. unfold occurrence_shape.
    cbn [occurrence_channel occurrence_group occurrence_positions
      occurrence_target].
    split; [reflexivity|]. split; [exact Htarget|].
    split; [exact Hmember|reflexivity].
  - cbn [occurrence_group]. apply Hjoins.
    apply (proj2 (Hexact channel joins Hview group)). exact Hjoin.
Qed.

Theorem framed_occurrence_keys_enumerate_exactly_the_channel_groups :
  (forall channel other suffix other_suffix,
     RhoPathV1Datum.bytes_valid channel = true ->
     RhoPathV1Datum.bytes_valid other = true ->
     RhoPathV1Datum.encode_frame channel ++ suffix =
       RhoPathV1Datum.encode_frame other ++ other_suffix ->
     channel = other /\ suffix = other_suffix) /\
  (forall (pairs : list (Path * Path)) channel suffix,
     Forall (fun pair => RhoPathV1Datum.bytes_valid (fst pair) = true) pairs ->
     RhoPathV1Datum.bytes_valid channel = true ->
     (In (RhoPathV1Datum.encode_frame channel ++ suffix)
        (map (fun pair => RhoPathV1Datum.encode_frame (fst pair) ++ snd pair)
          pairs) <->
      In suffix (map snd (filter (fun pair =>
        if path_eq_dec (fst pair) channel then true else false) pairs)))).
Proof.
  assert (Hinjective : forall channel other suffix other_suffix,
     RhoPathV1Datum.bytes_valid channel = true ->
     RhoPathV1Datum.bytes_valid other = true ->
     RhoPathV1Datum.encode_frame channel ++ suffix =
       RhoPathV1Datum.encode_frame other ++ other_suffix ->
     channel = other /\ suffix = other_suffix).
  { intros channel other suffix other_suffix Hchannel Hother Hsame.
    pose proof (RhoPathV1Datum.frame_encode_parse_round_trip channel suffix
      Hchannel) as Hleft.
    pose proof (RhoPathV1Datum.frame_encode_parse_round_trip other
      other_suffix Hother) as Hright.
    rewrite Hsame, Hright in Hleft. injection Hleft as Hfirst Hsecond.
    split; symmetry; assumption. }
  split; [exact Hinjective|].
  intros pairs channel suffix Hpairs Hchannel.
  rewrite in_map_iff, in_map_iff. split.
  - intros [[first second] [Hkey Hin]]. cbn [fst snd] in Hkey.
    assert (Hvalid : RhoPathV1Datum.bytes_valid first = true).
    { rewrite Forall_forall in Hpairs. exact (Hpairs (first, second) Hin). }
    destruct (Hinjective first channel second suffix Hvalid Hchannel Hkey)
      as [Hfirst Hsecond].
    exists (first, second). cbn [snd]. split; [exact Hsecond|].
    apply filter_In. split; [exact Hin|]. cbn [fst].
    destruct (path_eq_dec first channel) as [_|Hdifferent];
      [reflexivity|contradiction].
  - intros [[first second] [Hsuffix Hin]]. cbn [snd] in Hsuffix.
    apply filter_In in Hin. destruct Hin as [Hin Hdecision].
    cbn [fst] in Hdecision. revert Hdecision.
    destruct (path_eq_dec first channel) as [Hfirst|Hfirst];
      intro Hdecision; [clear Hdecision|discriminate].
    exists (first, second). cbn [fst snd]. subst first second.
    split; [reflexivity|exact Hin].
Qed.

Theorem equal_store_tokens_imply_equal_revisions : forall initial w,
  WorldRun initial w ->
  In (revision_store (world_revision initial)) (world_issued initial) ->
  revision_store (world_revision w) = revision_store (world_revision initial) ->
  world_revision w = world_revision initial.
Proof.
  intros initial w Hrun Hissued.
  induction Hrun as [|current operation Hrun IH Hadmissible].
  - intros _. reflexivity.
  - intro Hsame. unfold step_world in Hsame |- *.
    destruct (bumps_decidable (operation_effect operation current))
      as [Hbumps|Hquiet].
    + exfalso.
      destruct (step_effect_fresh (operation_effect operation current) current
        Hbumps) as [Hfresh _].
      rewrite Hfresh in Hsame. apply (fresh_token_is_fresh current).
      rewrite Hsame. apply (world_run_issued initial current); assumption.
    + rewrite step_effect_quiet in Hsame |- * by exact Hquiet.
      apply IH. exact Hsame.
Qed.

Print Assumptions join_coverage_completes_the_channel_scope.
Print Assumptions complete_channel_scope_lookup_is_derivation.
Print Assumptions exact_channel_read_is_current_sound_and_group_complete.
Print Assumptions covered_exact_channel_read_is_the_derivation.
Print Assumptions global_coherence_implies_channel_coherence.
Print Assumptions exact_channel_read_contains_the_joins_derivation.
Print Assumptions framed_occurrence_keys_enumerate_exactly_the_channel_groups.
Print Assumptions equal_store_tokens_imply_equal_revisions.

End TwoLevelRSpaceReceiveAcceleratorChannelScope.
