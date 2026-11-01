From Stdlib Require Import Bool List Arith ZArith.
From StackSafePDA Require Import PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority RhoPathV1Datum TwoLevelRSpaceContinuationAuthority
  TwoLevelRSpaceReceiveAccelerator TwoLevelRSpaceReceiveAcceleratorChannelScope.
Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceContinuationAuthority.
Import OrderedReceivePositions.
Import TwoLevelRSpaceReceiveAccelerator.
Import TwoLevelRSpaceReceiveAcceleratorChannelScope.

Open Scope nat_scope.

Module TwoLevelRSpaceReceiveAcceleratorChannelScopeMutants.

Fixpoint prefixb (prefix key : Path) : bool :=
  match prefix, key with
  | [], _ => true
  | head :: tail, key_head :: key_tail =>
      Nat.eqb head key_head && prefixb tail key_tail
  | _ :: _, [] => false
  end.

Definition group_suffix (group : GroupKey) : Path :=
  flat_map RhoPathV1Datum.encode_frame group.

Definition occurrence_key (frame : Path -> Path) (occurrence : Occurrence)
    : Path :=
  frame (occurrence_channel occurrence) ++
  group_suffix (occurrence_group occurrence).

Definition keyed_lookup (frame : Path -> Path) (entries : list Occurrence)
    (channel : Path) : list Occurrence :=
  filter (fun occurrence =>
    prefixb (frame channel) (occurrence_key frame occurrence)) entries.

Definition raw_frame (channel : Path) : Path := channel.

Lemma prefixb_app : forall prefix suffix,
  prefixb prefix (prefix ++ suffix) = true.
Proof.
  induction prefix as [|head tail IH]; intro suffix; [reflexivity|].
  cbn [app prefixb]. rewrite Nat.eqb_refl, IH. reflexivity.
Qed.

Lemma prefixb_sound : forall prefix key,
  prefixb prefix key = true -> exists suffix, key = prefix ++ suffix.
Proof.
  induction prefix as [|head tail IH]; intros key Hprefix.
  - exists key. reflexivity.
  - destruct key as [|key_head key_tail]; [discriminate|].
    cbn [prefixb] in Hprefix. apply andb_true_iff in Hprefix.
    destruct Hprefix as [Hhead Htail]. apply Nat.eqb_eq in Hhead.
    subst key_head.
    destruct (IH key_tail Htail) as [suffix Hsuffix].
    exists suffix. rewrite Hsuffix. reflexivity.
Qed.

Lemma encode_frame_nil : RhoPathV1Datum.encode_frame [] = [0; 0].
Proof. reflexivity. Qed.

Lemma encode_frame_zero : forall tail,
  RhoPathV1Datum.encode_frame (0 :: tail) =
  0 :: 255 :: RhoPathV1Datum.encode_frame tail.
Proof. intro tail. reflexivity. Qed.

Lemma encode_frame_successor : forall value tail,
  RhoPathV1Datum.encode_frame (S value :: tail) =
  S value :: RhoPathV1Datum.encode_frame tail.
Proof. intros value tail. reflexivity. Qed.

Lemma encode_frame_prefix_free : forall channel other suffix other_suffix,
  RhoPathV1Datum.encode_frame channel ++ suffix =
    RhoPathV1Datum.encode_frame other ++ other_suffix ->
  channel = other /\ suffix = other_suffix.
Proof.
  induction channel as [|value tail IH];
    intros [|other_value other_tail] suffix other_suffix Hsame.
  - rewrite encode_frame_nil in Hsame. cbn [app] in Hsame.
    injection Hsame as Hsuffix. split; [reflexivity|exact Hsuffix].
  - exfalso. rewrite encode_frame_nil in Hsame.
    destruct other_value as [|other_value];
      [rewrite encode_frame_zero in Hsame|
       rewrite encode_frame_successor in Hsame];
      cbn [app] in Hsame; discriminate.
  - exfalso. rewrite encode_frame_nil in Hsame.
    destruct value as [|value];
      [rewrite encode_frame_zero in Hsame|
       rewrite encode_frame_successor in Hsame];
      cbn [app] in Hsame; discriminate.
  - destruct value as [|value]; destruct other_value as [|other_value];
      rewrite ?encode_frame_zero, ?encode_frame_successor in Hsame;
      cbn [app] in Hsame.
    + injection Hsame as Hrest.
      destruct (IH other_tail suffix other_suffix Hrest) as [Htail Hsuffix].
      subst other_tail other_suffix. split; reflexivity.
    + discriminate.
    + discriminate.
    + injection Hsame as Hvalue Hrest.
      destruct (IH other_tail suffix other_suffix Hrest) as [Htail Hsuffix].
      subst other_value other_tail other_suffix. split; reflexivity.
Qed.

Lemma framed_lookup_is_model_lookup : forall entries channel,
  keyed_lookup RhoPathV1Datum.encode_frame entries channel =
  accelerator_lookup entries channel.
Proof.
  intros entries channel. unfold keyed_lookup, accelerator_lookup.
  apply filter_ext. intro occurrence. cbv beta. unfold occurrence_key.
  destruct (path_eq_dec (occurrence_channel occurrence) channel)
    as [Hsame|Hdifferent]; cbv beta iota.
  - rewrite Hsame. apply prefixb_app.
  - apply not_true_is_false. intro Hprefix. apply Hdifferent.
    destruct (prefixb_sound _ _ Hprefix) as [suffix Hsuffix].
    destruct (encode_frame_prefix_free _ _ _ _ Hsuffix) as [Hchannel _].
    exact Hchannel.
Qed.

Record Switches : Type := SwitchesState {
  switch_skipped_join_scope : bool;
  switch_skipped_lookup_scope : bool;
  switch_unframed_keys : bool;
  switch_live_join_scope : bool
}.

Definition switches_with (flipped : nat) : Switches :=
  SwitchesState (Nat.eqb flipped 1) (Nat.eqb flipped 2) (Nat.eqb flipped 3)
    (Nat.eqb flipped 4).

Definition correct_switches : Switches := switches_with 0.

Definition switch_vector (switches : Switches) : list bool :=
  [switch_skipped_join_scope switches; switch_skipped_lookup_scope switches;
   switch_unframed_keys switches; switch_live_join_scope switches].

Definition impl_lookup (switches : Switches) (entries : list Occurrence)
    (channel : Path) : list Occurrence :=
  if switch_unframed_keys switches then keyed_lookup raw_frame entries channel
  else keyed_lookup RhoPathV1Datum.encode_frame entries channel.

Definition impl_scope_check (switches : Switches) (view : JoinView)
    (scope live_scope : list GroupKey) (entries : list Occurrence)
    (channel : Path) : bool :=
  match view channel with
  | Some joins =>
      (if switch_skipped_join_scope switches then true
       else joins_in_scopeb
         (if switch_live_join_scope switches then live_scope else scope)
         joins) &&
      (if switch_skipped_lookup_scope switches then true
       else occurrences_in_scopeb scope (impl_lookup switches entries channel))
  | None => false
  end.

Definition impl_classify (switches : Switches) (w : World) (view : JoinView)
    (live_scope : list GroupKey) (accelerator : AcceleratorState)
    (channel : Path) : ChannelClass :=
  match accelerator with
  | AcceleratorAbsent => ChannelAbsent
  | AcceleratorBuilding _ _ _ => ChannelBuilding
  | AcceleratorPublished bound lineage scope entries artifacts =>
      if semantic_roots_eq_dec lineage (world_manifest w) then
        if EqualityEpoch.eq_dec (revision_generation bound)
            (revision_generation (world_revision w)) then
          if revision_eq_dec bound (world_revision w) then
            if artifacts_eq_dec artifacts (artifacts_at bound) then
              if impl_scope_check switches view scope live_scope entries channel
              then ChannelCoherent else ChannelIncomplete
            else ChannelStale
          else ChannelStale
        else ChannelForeign
      else ChannelForeign
  end.

Definition impl_answer (switches : Switches) (w : World) (view : JoinView)
    (live_scope : list GroupKey) (accelerator : AcceleratorState)
    (channel : Path) : Answer :=
  match accelerator with
  | AcceleratorPublished _ _ _ entries _ =>
      match impl_classify switches w view live_scope accelerator channel with
      | ChannelCoherent => ExactAnswer (impl_lookup switches entries channel)
      | _ => FallbackAnswer (derive_world w channel)
      end
  | _ => FallbackAnswer (derive_world w channel)
  end.

Lemma impl_lookup_framed : forall switches entries channel,
  switch_unframed_keys switches = false ->
  impl_lookup switches entries channel = accelerator_lookup entries channel.
Proof.
  intros switches entries channel Hframed. unfold impl_lookup.
  rewrite Hframed. cbv beta iota. apply framed_lookup_is_model_lookup.
Qed.

Lemma impl_scope_check_correct : forall view scope live_scope entries channel,
  impl_scope_check correct_switches view scope live_scope entries channel =
  channel_scope_check view scope entries channel.
Proof.
  intros view scope live_scope entries channel.
  unfold impl_scope_check, channel_scope_check, lookup_in_scopeb.
  rewrite (impl_lookup_framed correct_switches entries channel eq_refl).
  reflexivity.
Qed.

Lemma impl_classify_correct : forall w view live_scope accelerator channel,
  impl_classify correct_switches w view live_scope accelerator channel =
  channel_classify w view accelerator channel.
Proof.
  intros w view live_scope accelerator channel.
  destruct accelerator as [|base scope entries|bound lineage scope entries
    artifacts]; [reflexivity|reflexivity|].
  cbn [impl_classify channel_classify].
  rewrite impl_scope_check_correct. reflexivity.
Qed.

Lemma impl_answer_correct : forall w view live_scope accelerator channel,
  impl_answer correct_switches w view live_scope accelerator channel =
  channel_answer w view accelerator channel.
Proof.
  intros w view live_scope accelerator channel.
  destruct accelerator as [|base scope entries|bound lineage scope entries
    artifacts]; [reflexivity|reflexivity|].
  cbn [impl_answer channel_answer].
  rewrite impl_classify_correct,
    (impl_lookup_framed correct_switches entries channel eq_refl).
  reflexivity.
Qed.

Theorem channel_scope_implementation_with_correct_switches_is_the_model :
  switch_vector correct_switches = repeat false 4 /\
  map (fun flipped =>
    count_occ Bool.bool_dec (switch_vector (switches_with flipped)) true)
    (seq 1 4) = repeat 1 4 /\
  map (fun flipped =>
    nth (pred flipped) (switch_vector (switches_with flipped)) false)
    (seq 1 4) = repeat true 4 /\
  (forall entries channel,
     impl_lookup correct_switches entries channel =
     accelerator_lookup entries channel) /\
  (forall w view live_scope accelerator channel,
     impl_classify correct_switches w view live_scope accelerator channel =
     channel_classify w view accelerator channel) /\
  (forall w view live_scope accelerator channel,
     impl_answer correct_switches w view live_scope accelerator channel =
     channel_answer w view accelerator channel).
Proof.
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [intros entries channel; apply impl_lookup_framed; reflexivity|].
  split; [exact impl_classify_correct|exact impl_answer_correct].
Qed.

Definition channel_a : Path := [1].
Definition channel_b : Path := [1; 0].
Definition group_aba : GroupKey := [channel_a; channel_b; channel_a].
Definition group_b : GroupKey := [channel_b].

Definition record_x1 : ContinuationRecord :=
  ContinuationRecordState [11] [21] false [].
Definition record_x2 : ContinuationRecord :=
  ContinuationRecordState [12] [22] false [].
Definition record_x3 : ContinuationRecord :=
  ContinuationRecordState [13] [23] false [].
Definition record_y : ContinuationRecord :=
  ContinuationRecordState [14] [24] false [].

Definition base_roots : SemanticRoots :=
  SemanticRootsState []
    [(group_aba, [record_x1; record_x2]); (group_b, [record_x3])]
    [(channel_a, [group_aba]); (channel_b, [group_aba; group_b])].

Definition initial_revision : Revision :=
  RevisionState (EqualityEpoch.Token 1) (EqualityEpoch.Token 1)
    (EqualityEpoch.Token 1).

Definition world_at (roots : SemanticRoots) (slots : InstalledSlots) : World :=
  WorldState roots slots initial_revision [EqualityEpoch.Token 1] roots
    (EqualityEpoch.Token 1) [].

Definition base_world : World := world_at base_roots [].
Definition installed_world : World :=
  world_at base_roots [(group_b, record_y)].

Definition omitting_view (w : World) (omitted : GroupKey) : JoinView :=
  fun channel =>
    Some (remove group_eq_dec omitted
      (joins_of (world_roots w) (world_installed w) channel)).

Definition published_scope (accelerator : AcceleratorState)
    : list GroupKey :=
  match accelerator with
  | AcceleratorAbsent => []
  | AcceleratorBuilding _ scope _ => scope
  | AcceleratorPublished _ _ scope _ _ => scope
  end.

Definition occurrence_x3 : Occurrence :=
  OccurrenceState channel_b group_b [0] (resting_target group_b record_x3).
Definition installed_occurrence_y : Occurrence :=
  OccurrenceState channel_b group_b [0] (installed_target group_b record_y).

Definition occurrence_eq_dec (left right : Occurrence)
    : {left = right} + {left <> right}.
Proof.
  destruct left as [left_channel left_group left_positions left_target].
  destruct right as [right_channel right_group right_positions right_target].
  destruct (path_eq_dec left_channel right_channel) as [Hchannel|Hchannel];
    [subst right_channel|right; congruence].
  destruct (group_eq_dec left_group right_group) as [Hgroup|Hgroup];
    [subst right_group|right; congruence].
  destruct (list_eq_dec Nat.eq_dec left_positions right_positions)
    as [Hpositions|Hpositions];
    [subst right_positions|right; congruence].
  destruct (receive_target_eq_dec left_target right_target)
    as [Htarget|Htarget];
    [subst right_target; left; reflexivity|right; congruence].
Defined.

Lemma member_of_decision : forall (A : Type)
    (eq_dec : forall left right : A, {left = right} + {left <> right})
    (item : A) (items : list A),
  (if in_dec eq_dec item items then true else false) = true -> In item items.
Proof.
  intros A eq_dec item items.
  destruct (in_dec eq_dec item items) as [Hin|Hnotin];
    [intros _; exact Hin|intro Hfalse; discriminate].
Qed.

Lemma absent_of_decision : forall (A : Type)
    (eq_dec : forall left right : A, {left = right} + {left <> right})
    (item : A) (items : list A),
  (if in_dec eq_dec item items then true else false) = false ->
  ~ In item items.
Proof.
  intros A eq_dec item items.
  destruct (in_dec eq_dec item items) as [Hin|Hnotin];
    [intro Hfalse; discriminate|intros _; exact Hnotin].
Qed.

Lemma distinct_of_decision : forall (A : Type)
    (eq_dec : forall left right : A, {left = right} + {left <> right})
    (left right : A),
  (if eq_dec left right then false else true) = true -> left <> right.
Proof.
  intros A eq_dec left right.
  destruct (eq_dec left right) as [Hsame|Hdifferent];
    [intro Hfalse; discriminate|intros _; exact Hdifferent].
Qed.

Lemma missing_breaks_same_members : forall (A : Type) (left right : list A)
    (missing : A),
  ~ In missing left -> In missing right -> ~ same_members left right.
Proof.
  intros A left right missing Hleft Hright Hsame.
  apply Hleft. apply (proj2 (Hsame missing)). exact Hright.
Qed.

Lemma channel_a_is_not_channel_b : channel_a <> channel_b.
Proof.
  apply (distinct_of_decision _ path_eq_dec). vm_compute. reflexivity.
Qed.

Lemma base_group_records_spec : forall group,
  group_records base_roots group <> [] <-> group = group_aba \/ group = group_b.
Proof.
  intro group.
  destruct (group_eq_dec group group_aba) as [Haba|Haba].
  - subst group.
    assert (Hrecords : group_records base_roots group_aba =
      [record_x1; record_x2]) by (vm_compute; reflexivity).
    rewrite Hrecords. split.
    + intros _. left. reflexivity.
    + intros _. discriminate.
  - destruct (group_eq_dec group group_b) as [Hgb|Hgb].
    + subst group.
      assert (Hrecords : group_records base_roots group_b = [record_x3])
        by (vm_compute; reflexivity).
      rewrite Hrecords. split.
      * intros _. right. reflexivity.
      * intros _. discriminate.
    + assert (Hnone : assoc_lookup group_eq_dec
        (root_continuations base_roots) group = None).
      { apply assoc_lookup_none. change (~ In group [group_aba; group_b]).
        cbn [In]. intros [Hin|[Hin|[]]].
        - exact (Haba (eq_sym Hin)).
        - exact (Hgb (eq_sym Hin)). }
      assert (Hempty : group_records base_roots group = []).
      { unfold group_records, assoc_get. rewrite Hnone. reflexivity. }
      rewrite Hempty. split.
      * intro Hnonempty. exfalso. apply Hnonempty. reflexivity.
      * intros [Hgroup|Hgroup]; exfalso;
          [exact (Haba Hgroup)|exact (Hgb Hgroup)].
Qed.

Lemma base_resting_joins_spec : forall channel group,
  In group (resting_joins_of base_roots channel) <->
  (channel = channel_a /\ group = group_aba) \/
  (channel = channel_b /\ (group = group_aba \/ group = group_b)).
Proof.
  intros channel group.
  destruct (path_eq_dec channel channel_a) as [Ha|Ha].
  - subst channel.
    assert (Hjoins : resting_joins_of base_roots channel_a = [group_aba])
      by (vm_compute; reflexivity).
    rewrite Hjoins. cbn [In]. split.
    + intros [Hgroup|[]]. left. split; [reflexivity|symmetry; exact Hgroup].
    + intros [[_ Hgroup]|[Hchannel _]].
      * left. symmetry. exact Hgroup.
      * exfalso. exact (channel_a_is_not_channel_b Hchannel).
  - destruct (path_eq_dec channel channel_b) as [Hb|Hb].
    + subst channel.
      assert (Hjoins : resting_joins_of base_roots channel_b =
        [group_aba; group_b]) by (vm_compute; reflexivity).
      rewrite Hjoins. cbn [In]. split.
      * intros [Hgroup|[Hgroup|[]]].
        -- right. split; [reflexivity|left; symmetry; exact Hgroup].
        -- right. split; [reflexivity|right; symmetry; exact Hgroup].
      * intros [[Hchannel _]|[_ [Hgroup|Hgroup]]].
        -- exfalso. exact (channel_a_is_not_channel_b (eq_sym Hchannel)).
        -- left. symmetry. exact Hgroup.
        -- right. left. symmetry. exact Hgroup.
    + assert (Hnone : assoc_lookup path_eq_dec (root_joins base_roots)
        channel = None).
      { apply assoc_lookup_none. change (~ In channel [channel_a; channel_b]).
        cbn [In]. intros [Hin|[Hin|[]]].
        - exact (Ha (eq_sym Hin)).
        - exact (Hb (eq_sym Hin)). }
      assert (Hjoins : resting_joins_of base_roots channel = []).
      { unfold resting_joins_of, assoc_get. rewrite Hnone. reflexivity. }
      rewrite Hjoins. cbn [In]. split.
      * intros [].
      * intros [[Hchannel _]|[Hchannel _]]; exfalso;
          [exact (Ha Hchannel)|exact (Hb Hchannel)].
Qed.

Lemma base_roots_joins_cover : joins_cover base_roots.
Proof.
  intros channel group.
  pose proof (base_resting_joins_spec channel group) as Hjoins.
  pose proof (base_group_records_spec group) as Hrecords.
  split.
  - intro Hin. apply (proj1 Hjoins) in Hin.
    destruct Hin as [[Hchannel Hgroup]|[Hchannel [Hgroup|Hgroup]]];
      subst channel group.
    + split; [apply (member_of_decision _ path_eq_dec); vm_compute;
        reflexivity|].
      apply (proj2 Hrecords). left. reflexivity.
    + split; [apply (member_of_decision _ path_eq_dec); vm_compute;
        reflexivity|].
      apply (proj2 Hrecords). left. reflexivity.
    + split; [apply (member_of_decision _ path_eq_dec); vm_compute;
        reflexivity|].
      apply (proj2 Hrecords). right. reflexivity.
  - intros [Hin Hnonempty]. apply (proj2 Hjoins).
    destruct (proj1 Hrecords Hnonempty) as [Hgroup|Hgroup]; subst group.
    + unfold group_aba in Hin. cbn [In] in Hin.
      destruct Hin as [Hchannel|[Hchannel|[Hchannel|[]]]]; subst channel.
      * left. split; reflexivity.
      * right. split; [reflexivity|left; reflexivity].
      * left. split; reflexivity.
    + unfold group_b in Hin. cbn [In] in Hin.
      destruct Hin as [Hchannel|[]]. subst channel.
      right. split; [reflexivity|right; reflexivity].
Qed.

Theorem skipped_join_scope_check_is_refuted :
  exists w view scope entries channel occurrences missing,
    joins_cover (world_roots w) /\ join_view_exact w view /\
    production_entries w scope entries /\
    channel_classify w view (publish_at w scope entries) channel =
      ChannelIncomplete /\
    impl_answer (switches_with 1) w view scope (publish_at w scope entries)
      channel = ExactAnswer occurrences /\
    In missing (derive_world w channel) /\
    ~ In (occurrence_group missing) scope /\
    ~ In missing occurrences /\
    ~ same_members occurrences (derive_world w channel).
Proof.
  assert (Hmissing : In occurrence_x3 (derive_world base_world channel_b))
    by (apply (member_of_decision _ occurrence_eq_dec); vm_compute;
        reflexivity).
  assert (Hlost : ~ In occurrence_x3 (accelerator_lookup
    (production_index base_world [group_aba]) channel_b))
    by (apply (absent_of_decision _ occurrence_eq_dec); vm_compute;
        reflexivity).
  exists base_world, (exact_join_view base_world), [group_aba],
    (production_index base_world [group_aba]), channel_b,
    (accelerator_lookup (production_index base_world [group_aba]) channel_b),
    occurrence_x3.
  split; [exact base_roots_joins_cover|].
  split; [apply exact_join_view_is_exact|].
  split; [apply production_index_is_production|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [exact Hmissing|].
  split; [apply (absent_of_decision _ group_eq_dec); vm_compute;
    reflexivity|].
  split; [exact Hlost|].
  exact (missing_breaks_same_members _ _ _ occurrence_x3 Hlost Hmissing).
Qed.

Theorem skipped_lookup_scope_check_is_refuted :
  exists w view scope entries channel occurrences answered missing,
    joins_cover (world_roots w) /\ ~ join_view_exact w view /\
    view channel <> None /\
    production_entries w scope entries /\
    channel_classify w view (publish_at w scope entries) channel =
      ChannelIncomplete /\
    impl_answer (switches_with 2) w view scope (publish_at w scope entries)
      channel = ExactAnswer occurrences /\
    (forall occurrence, In occurrence occurrences ->
       In occurrence (derive_world w channel)) /\
    In answered occurrences /\ ~ In (occurrence_group answered) scope /\
    In missing (derive_world w channel) /\
    occurrence_group missing = occurrence_group answered /\
    ~ In missing occurrences /\
    ~ same_members occurrences (derive_world w channel).
Proof.
  assert (Hmissing : In occurrence_x3
    (derive_world installed_world channel_b))
    by (apply (member_of_decision _ occurrence_eq_dec); vm_compute;
        reflexivity).
  assert (Hlost : ~ In occurrence_x3 (accelerator_lookup
    (production_index installed_world [group_aba]) channel_b))
    by (apply (absent_of_decision _ occurrence_eq_dec); vm_compute;
        reflexivity).
  exists installed_world, (omitting_view installed_world group_b),
    [group_aba], (production_index installed_world [group_aba]), channel_b,
    (accelerator_lookup (production_index installed_world [group_aba])
      channel_b),
    installed_occurrence_y, occurrence_x3.
  split; [exact base_roots_joins_cover|].
  split.
  { intro Hexact.
    assert (Hjoin : In group_b (joins_of (world_roots installed_world)
      (world_installed installed_world) channel_b))
      by (apply (member_of_decision _ group_eq_dec); vm_compute;
          reflexivity).
    apply (proj2 (Hexact channel_b _ eq_refl group_b)) in Hjoin.
    exact (remove_In group_eq_dec _ group_b Hjoin). }
  split; [unfold omitting_view; discriminate|].
  split; [apply production_index_is_production|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split.
  { intros occurrence Hin.
    exact (production_lookup_is_sound installed_world [group_aba]
      (production_index installed_world [group_aba]) channel_b occurrence
      (production_index_is_production installed_world [group_aba]) Hin). }
  split; [apply (member_of_decision _ occurrence_eq_dec); vm_compute;
    reflexivity|].
  split; [apply (absent_of_decision _ group_eq_dec); vm_compute;
    reflexivity|].
  split; [exact Hmissing|].
  split; [reflexivity|].
  split; [exact Hlost|].
  exact (missing_breaks_same_members _ _ _ occurrence_x3 Hlost Hmissing).
Qed.

Theorem unframed_occurrence_keys_are_refuted :
  exists w view scope entries channel occurrences aliased,
    joins_cover (world_roots w) /\ join_view_exact w view /\
    production_entries w scope entries /\
    channel_classify w view (publish_at w scope entries) channel =
      ChannelCoherent /\
    impl_answer (switches_with 3) w view scope (publish_at w scope entries)
      channel = ExactAnswer occurrences /\
    prefixb (raw_frame channel) (occurrence_key raw_frame aliased) = true /\
    prefixb (RhoPathV1Datum.encode_frame channel)
      (occurrence_key RhoPathV1Datum.encode_frame aliased) = false /\
    occurrence_channel aliased <> channel /\
    ~ In channel (occurrence_group aliased) /\
    In aliased occurrences /\
    ~ In aliased (derive_world w channel) /\
    ~ same_members occurrences (derive_world w channel).
Proof.
  assert (Haliased : In occurrence_x3 (impl_lookup (switches_with 3)
    (production_index base_world [group_aba; group_b]) channel_a))
    by (apply (member_of_decision _ occurrence_eq_dec); vm_compute;
        reflexivity).
  assert (Hforeign : ~ In occurrence_x3 (derive_world base_world channel_a))
    by (apply (absent_of_decision _ occurrence_eq_dec); vm_compute;
        reflexivity).
  exists base_world, (exact_join_view base_world), [group_aba; group_b],
    (production_index base_world [group_aba; group_b]), channel_a,
    (impl_lookup (switches_with 3)
      (production_index base_world [group_aba; group_b]) channel_a),
    occurrence_x3.
  split; [exact base_roots_joins_cover|].
  split; [apply exact_join_view_is_exact|].
  split; [apply production_index_is_production|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [apply (distinct_of_decision _ path_eq_dec); vm_compute;
    reflexivity|].
  split; [apply (absent_of_decision _ path_eq_dec); vm_compute;
    reflexivity|].
  split; [exact Haliased|].
  split; [exact Hforeign|].
  intro Hsame. apply Hforeign. apply (proj1 (Hsame occurrence_x3)).
  exact Haliased.
Qed.

Theorem live_scope_join_check_is_refuted :
  exists w view group channel live_scope,
    joins_cover (world_roots w) /\ join_view_exact w view /\
    production_entries w [] [] /\
    step_world (HistoryLoad group) w = w /\
    live_scope = published_scope
      (update_accelerator (HistoryLoad group) w (publish_at w [] [])) /\
    In group live_scope /\
    channel_classify w view
      (update_accelerator (HistoryLoad group) w (publish_at w [] []))
      channel = ChannelCoherent /\
    channel_classify w view (publish_at w [] []) channel =
      ChannelIncomplete /\
    impl_answer (switches_with 4) w view live_scope (publish_at w [] [])
      channel = ExactAnswer [] /\
    derive_world w channel <> [].
Proof.
  exists base_world, (exact_join_view base_world), group_aba, channel_a,
    (published_scope (update_accelerator (HistoryLoad group_aba) base_world
      (publish_at base_world [] []))).
  split; [exact base_roots_joins_cover|].
  split; [apply exact_join_view_is_exact|].
  split; [exact (production_index_is_production base_world [])|].
  split; [reflexivity|].
  split; [reflexivity|].
  split; [apply (member_of_decision _ group_eq_dec); vm_compute;
    reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  intro Hempty. vm_compute in Hempty. discriminate.
Qed.

Print Assumptions channel_scope_implementation_with_correct_switches_is_the_model.
Print Assumptions skipped_join_scope_check_is_refuted.
Print Assumptions skipped_lookup_scope_check_is_refuted.
Print Assumptions unframed_occurrence_keys_are_refuted.
Print Assumptions live_scope_join_check_is_refuted.

End TwoLevelRSpaceReceiveAcceleratorChannelScopeMutants.
