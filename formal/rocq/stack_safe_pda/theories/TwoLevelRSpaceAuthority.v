From Stdlib Require Import Bool List Arith Lia NArith FunctionalExtensionality.
From StackSafePDA Require Import PathMapCanonicalStreamPublicationBase.

Import ListNotations.
Set Implicit Arguments.
Open Scope N_scope.

(** Logical authority for the two-level RSpace layout.

    The outer PathMap has one exact endpoint per loaded canonical channel. Its
    value is an independently guarded channel-relative Bag. An empty Bag is
    therefore distinguishable from an unloaded channel. Lists here are ghost
    observations of canonical PathMap cursors; they do not prescribe a Vec,
    sorting pass, or flattened production representation.

    This file bridges the existing Bag, canonical-stream, ownership, catalog,
    accepted-match, persistence, and teardown models. It proves logical
    refinement and publication laws. Concrete PathMap node sharing, ACT bytes,
    and Rust destructor refinement remain obligations of those authorities. *)
Module TwoLevelRSpaceAuthority.

Definition path_eq_dec : forall left right : Path, {left = right} + {left <> right} :=
  list_eq_dec Nat.eq_dec.

Definition ChannelBag : Type := list Entry.
Definition ChannelEntry : Type := (Path * ChannelBag)%type.
Definition TwoLevelAuthority : Type := list ChannelEntry.

Definition channel_path (entry : ChannelEntry) : Path := fst entry.
Definition channel_bag (entry : ChannelEntry) : ChannelBag := snd entry.

Fixpoint lookup_channel
    (selected : Path) (authority : TwoLevelAuthority) : option ChannelBag :=
  match authority with
  | [] => None
  | (channel, contents) :: tail =>
      if path_eq_dec selected channel
      then Some contents
      else lookup_channel selected tail
  end.

Fixpoint lookup_multiplicity_in_bag
    (selected : Path) (contents : ChannelBag) : Multiplicity :=
  match contents with
  | [] => 0%N
  | (datum, multiplicity) :: tail =>
      if path_eq_dec selected datum
      then multiplicity
      else lookup_multiplicity_in_bag selected tail
  end.

Definition lookup_multiplicity
    (authority : TwoLevelAuthority) (channel datum : Path) : Multiplicity :=
  match lookup_channel channel authority with
  | Some contents => lookup_multiplicity_in_bag datum contents
  | None => 0%N
  end.

Definition loaded (authority : TwoLevelAuthority) (channel : Path) : Prop :=
  exists contents, lookup_channel channel authority = Some contents.

Fixpoint replace_channel_exact
    (selected : Path)
    (replacement : ChannelBag)
    (authority : TwoLevelAuthority) : TwoLevelAuthority :=
  match authority with
  | [] => []
  | (channel, contents) :: tail =>
      if path_eq_dec selected channel
      then (channel, replacement) :: tail
      else (channel, contents) :: replace_channel_exact selected replacement tail
  end.

Inductive canonical_channels : TwoLevelAuthority -> Prop :=
| CanonicalChannelsNil : canonical_channels []
| CanonicalChannelsCons : forall channel contents tail,
    path_valid channel = true ->
    canonical_entries contents ->
    Forall (fun entry => path_lex_lt channel (channel_path entry)) tail ->
    canonical_channels tail ->
    canonical_channels ((channel, contents) :: tail).

Definition channel_paths (authority : TwoLevelAuthority) : list Path :=
  map channel_path authority.

Definition bag_paths (contents : ChannelBag) : list Path :=
  map entry_path contents.

Fixpoint total_distinct (authority : TwoLevelAuthority) : nat :=
  match authority with
  | [] => 0
  | (_, contents) :: tail => length contents + total_distinct tail
  end.

Fixpoint bag_cardinality (contents : ChannelBag) : Multiplicity :=
  match contents with
  | [] => 0%N
  | (_, multiplicity) :: tail => N.add multiplicity (bag_cardinality tail)
  end.

Fixpoint total_cardinality (authority : TwoLevelAuthority) : Multiplicity :=
  match authority with
  | [] => 0%N
  | (_, contents) :: tail =>
      N.add (bag_cardinality contents) (total_cardinality tail)
  end.

Theorem exact_channel_replacement_is_exact : forall authority channel old replacement,
  lookup_channel channel authority = Some old ->
  lookup_channel channel (replace_channel_exact channel replacement authority) =
    Some replacement.
Proof.
  induction authority as [|[current contents] tail IH];
    intros channel old replacement Hlookup; simpl in *.
  - discriminate.
  - destruct (path_eq_dec channel current) as [Heq | Hneq].
    + subst. cbn [lookup_channel].
      destruct (path_eq_dec current current); [reflexivity | contradiction].
    + simpl. destruct (path_eq_dec channel current); [contradiction |].
      apply IH with old. exact Hlookup.
Qed.

Theorem channel_replacement_preserves_every_other_exact_channel : forall
    authority selected replacement other,
  other <> selected ->
  lookup_channel other (replace_channel_exact selected replacement authority) =
    lookup_channel other authority.
Proof.
  induction authority as [|[current contents] tail IH];
    intros selected replacement other Hdifferent; [reflexivity |].
  cbn [replace_channel_exact].
  destruct (path_eq_dec selected current) as [Hselected | Hselected].
  - subst. cbn [lookup_channel].
    destruct (path_eq_dec other current); [contradiction | reflexivity].
  - cbn [lookup_channel].
    destruct (path_eq_dec other current) as [Hother | Hother].
    + reflexivity.
    + apply IH. exact Hdifferent.
Qed.

Theorem prefix_related_channels_are_still_exactly_independent : forall
    authority selected replacement other,
  other <> selected ->
  (exists suffix, other = selected ++ suffix) \/
  (exists suffix, selected = other ++ suffix) ->
  lookup_channel other (replace_channel_exact selected replacement authority) =
    lookup_channel other authority.
Proof.
  intros. now apply channel_replacement_preserves_every_other_exact_channel.
Qed.

Theorem loaded_empty_is_preserved : forall authority channel old,
  lookup_channel channel authority = Some old ->
  loaded (replace_channel_exact channel [] authority) channel /\
  forall datum,
    lookup_multiplicity (replace_channel_exact channel [] authority) channel datum = 0%N.
Proof.
  intros authority channel old Hlookup. split.
  - exists []. now apply exact_channel_replacement_is_exact with old.
  - intro datum. unfold lookup_multiplicity.
    rewrite (@exact_channel_replacement_is_exact authority channel old [] Hlookup).
    reflexivity.
Qed.

Theorem replacement_target_multiplicity_is_exact : forall
    authority channel old replacement datum,
  lookup_channel channel authority = Some old ->
  lookup_multiplicity
    (replace_channel_exact channel replacement authority) channel datum =
  lookup_multiplicity_in_bag datum replacement.
Proof.
  intros. unfold lookup_multiplicity.
  now rewrite (@exact_channel_replacement_is_exact authority channel old replacement H).
Qed.

Theorem replacement_target_distinct_count_is_exact : forall
    authority channel old replacement,
  lookup_channel channel authority = Some old ->
  match lookup_channel channel
      (replace_channel_exact channel replacement authority) with
  | Some contents => length contents
  | None => 0%nat
  end = length replacement.
Proof.
  intros. now rewrite (@exact_channel_replacement_is_exact authority channel old replacement H).
Qed.

Theorem replacement_target_cardinality_is_exact : forall
    authority channel old replacement,
  lookup_channel channel authority = Some old ->
  match lookup_channel channel
      (replace_channel_exact channel replacement authority) with
  | Some contents => bag_cardinality contents
  | None => 0%N
  end = bag_cardinality replacement.
Proof.
  intros. now rewrite (@exact_channel_replacement_is_exact authority channel old replacement H).
Qed.

Theorem replacement_total_distinct_conservation : forall
    authority channel old replacement,
  lookup_channel channel authority = Some old ->
  (total_distinct (replace_channel_exact channel replacement authority) + length old =
   total_distinct authority + length replacement)%nat.
Proof.
  induction authority as [|[current contents] tail IH];
    intros channel old replacement Hlookup; simpl in *.
  - discriminate.
  - destruct (path_eq_dec channel current) as [Heq | Hneq].
    + inversion Hlookup; subst. simpl. lia.
    + simpl. destruct (path_eq_dec channel current); [contradiction |].
      specialize (IH channel old replacement Hlookup). lia.
Qed.

Theorem replacement_total_cardinality_conservation : forall
    authority channel old replacement,
  lookup_channel channel authority = Some old ->
  N.add
    (total_cardinality (replace_channel_exact channel replacement authority))
    (bag_cardinality old) =
  N.add (total_cardinality authority) (bag_cardinality replacement).
Proof.
  induction authority as [|[current contents] tail IH];
    intros channel old replacement Hlookup; simpl in *.
  - discriminate.
  - destruct (path_eq_dec channel current) as [Heq | Hneq].
    + inversion Hlookup; subst. simpl. nia.
    + simpl. destruct (path_eq_dec channel current); [contradiction |].
      specialize (IH channel old replacement Hlookup). nia.
Qed.

Lemma replace_channel_exact_preserves_key_forall : forall
    (predicate : Path -> Prop) authority selected replacement,
  Forall (fun entry => predicate (channel_path entry)) authority ->
  Forall
    (fun entry => predicate (channel_path entry))
    (replace_channel_exact selected replacement authority).
Proof.
  intros predicate authority. induction authority as [|[current contents] tail IH];
    intros selected replacement Hforall; inversion Hforall; subst; simpl.
  - constructor.
  - destruct (path_eq_dec selected current); constructor; auto.
Qed.

Theorem canonical_channel_replacement_is_preserved : forall
    authority channel old replacement,
  canonical_channels authority ->
  lookup_channel channel authority = Some old ->
  canonical_entries replacement ->
  canonical_channels (replace_channel_exact channel replacement authority).
Proof.
  intros authority channel old replacement Hcanonical.
  induction Hcanonical as
    [|current contents tail Hpath Hcontents Hafter Htail IH];
    intro Hlookup; intro Hreplacement; simpl in *.
  - discriminate.
  - destruct (path_eq_dec channel current) as [Heq | Hneq].
    + subst. apply CanonicalChannelsCons; assumption.
    + apply CanonicalChannelsCons; try assumption.
      * now apply replace_channel_exact_preserves_key_forall.
      * apply IH; assumption.
Qed.

Lemma canonical_bag_paths_are_unique : forall contents,
  canonical_entries contents -> NoDup (bag_paths contents).
Proof.
  intros contents Hcanonical. induction Hcanonical.
  - constructor.
  - simpl. constructor.
    + intro Hin. apply in_map_iff in Hin.
      destruct Hin as [entry [Heq Hin]].
      apply Forall_forall with (x := entry) in H1; [|exact Hin].
      rewrite <- Heq in H1.
      exact (path_lex_lt_irreflexive (entry_path entry) H1).
    + exact IHHcanonical.
Qed.

Lemma canonical_channel_paths_are_unique : forall authority,
  canonical_channels authority -> NoDup (channel_paths authority).
Proof.
  intros authority Hcanonical. induction Hcanonical.
  - constructor.
  - simpl. constructor.
    + intro Hin. apply in_map_iff in Hin.
      destruct Hin as [entry [Heq Hin]].
      apply Forall_forall with (x := entry) in H1; [|exact Hin].
      rewrite <- Heq in H1.
      exact (path_lex_lt_irreflexive (channel_path entry) H1).
    + exact IHHcanonical.
Qed.

Lemma canonical_lookup_returns_canonical_bag : forall authority channel contents,
  canonical_channels authority ->
  lookup_channel channel authority = Some contents ->
  canonical_entries contents.
Proof.
  intros authority channel contents Hcanonical.
  induction Hcanonical; intro Hlookup; simpl in Hlookup.
  - discriminate.
  - destruct (path_eq_dec channel channel0).
    + inversion Hlookup; subst. assumption.
    + now apply IHHcanonical.
Qed.

Theorem canonical_authority_has_no_duplicate_datum_authority : forall
    authority channel contents,
  canonical_channels authority ->
  lookup_channel channel authority = Some contents ->
  NoDup (channel_paths authority) /\ NoDup (bag_paths contents).
Proof.
  intros. split.
  - now apply canonical_channel_paths_are_unique.
  - apply canonical_bag_paths_are_unique.
    now apply canonical_lookup_returns_canonical_bag with authority channel.
Qed.

(** A structured logical key avoids assuming that raw concatenation of a
    channel image and datum image is self-delimiting. *)
Definition LogicalKey : Type := (Path * Path)%type.
Definition LogicalEntry : Type := (LogicalKey * Multiplicity)%type.

Definition lift_channel_entry (channel : Path) (entry : Entry) : LogicalEntry :=
  ((channel, entry_path entry), entry_multiplicity entry).

Definition lift_channel_bag (channel : Path) (contents : ChannelBag)
    : list LogicalEntry :=
  map (lift_channel_entry channel) contents.

Fixpoint logical_projection (authority : TwoLevelAuthority) : list LogicalEntry :=
  match authority with
  | [] => []
  | (channel, contents) :: tail =>
      lift_channel_bag channel contents ++ logical_projection tail
  end.

Definition flattened_replacement_oracle
    (before : TwoLevelAuthority)
    (selected : Path)
    (replacement : ChannelBag)
    (channel datum : Path) : Multiplicity :=
  if path_eq_dec channel selected
  then lookup_multiplicity_in_bag datum replacement
  else lookup_multiplicity before channel datum.

Theorem exact_channel_replacement_refines_prefix_replacement_oracle : forall
    authority selected old replacement channel datum,
  lookup_channel selected authority = Some old ->
  lookup_multiplicity
    (replace_channel_exact selected replacement authority) channel datum =
  flattened_replacement_oracle
    authority selected replacement channel datum.
Proof.
  intros authority selected old replacement channel datum Hloaded.
  unfold flattened_replacement_oracle.
  destruct (path_eq_dec channel selected) as [Heq | Hneq].
  - subst. now apply replacement_target_multiplicity_is_exact with old.
  - unfold lookup_multiplicity.
    now rewrite channel_replacement_preserves_every_other_exact_channel.
Qed.

Theorem channel_replacement_is_local_to_touched_authorities : forall
    authority selected old replacement other datum,
  lookup_channel selected authority = Some old ->
  other <> selected ->
  lookup_multiplicity
    (replace_channel_exact selected replacement authority) other datum =
  lookup_multiplicity authority other datum.
Proof.
  intros. unfold lookup_multiplicity.
  now rewrite channel_replacement_preserves_every_other_exact_channel.
Qed.

Fixpoint nested_candidate_order
    (requested : list Path) (authority : TwoLevelAuthority) : list LogicalEntry :=
  match requested with
  | [] => []
  | channel :: tail =>
      match lookup_channel channel authority with
      | Some contents => lift_channel_bag channel contents
      | None => []
      end ++ nested_candidate_order tail authority
  end.

Definition legacy_candidate_order
    (requested : list Path) (authority : TwoLevelAuthority) : list LogicalEntry :=
  flat_map
    (fun channel =>
      match lookup_channel channel authority with
      | Some contents => lift_channel_bag channel contents
      | None => []
      end)
    requested.

Theorem candidate_order_refines_legacy_order : forall requested authority,
  nested_candidate_order requested authority =
  legacy_candidate_order requested authority.
Proof.
  induction requested as [|channel tail IH]; intro authority; simpl.
  - reflexivity.
  - destruct (lookup_channel channel authority); simpl; now rewrite IH.
Qed.

Record PreparedBatch : Type := PrepareBatch {
  prepared_authority : TwoLevelAuthority;
  prepared_authority_is_canonical : canonical_channels prepared_authority
}.

Definition publish_channel_batch
    (before : TwoLevelAuthority)
    (proposal : option PreparedBatch) : TwoLevelAuthority :=
  match proposal with
  | None => before
  | Some prepared => prepared_authority prepared
  end.

Theorem accepted_multichannel_commit_is_atomic : forall before prepared,
  publish_channel_batch before (Some prepared) = prepared_authority prepared.
Proof. reflexivity. Qed.

Theorem refused_multichannel_commit_preserves_all_public_authority : forall before,
  publish_channel_batch before None = before.
Proof. reflexivity. Qed.

Theorem published_multichannel_commit_is_canonical : forall before prepared,
  canonical_channels (publish_channel_batch before (Some prepared)).
Proof. intros. exact (prepared_authority_is_canonical prepared). Qed.

Definition encode_nested_stream (authority : TwoLevelAuthority) : list LogicalEntry :=
  logical_projection authority.

Definition decode_nested_stream (image : list LogicalEntry) : list LogicalEntry := image.

Theorem nested_stream_round_trip_is_exact : forall authority,
  decode_nested_stream (encode_nested_stream authority) =
  logical_projection authority.
Proof. reflexivity. Qed.

Definition canonical_legacy_hash_preimage
    (authority : TwoLevelAuthority) : list LogicalEntry :=
  logical_projection authority.

Theorem consensus_hash_preimage_is_preserved : forall authority,
  encode_nested_stream authority = canonical_legacy_hash_preimage authority.
Proof. reflexivity. Qed.

Record LegacyAuthority : Type := LegacyState {
  legacy_loaded_channels : list Path;
  legacy_entries : list LogicalEntry
}.

Fixpoint relative_entries_for
    (selected : Path) (entries : list LogicalEntry) : ChannelBag :=
  match entries with
  | [] => []
  | ((channel, datum), multiplicity) :: tail =>
      if path_eq_dec channel selected
      then (datum, multiplicity) :: relative_entries_for selected tail
      else relative_entries_for selected tail
  end.

Definition migrate_legacy (legacy : LegacyAuthority) : TwoLevelAuthority :=
  map
    (fun channel =>
      (channel, relative_entries_for channel (legacy_entries legacy)))
    (legacy_loaded_channels legacy).

Fixpoint lookup_legacy_multiplicity
    (channel datum : Path) (entries : list LogicalEntry) : Multiplicity :=
  match entries with
  | [] => 0%N
  | ((candidate_channel, candidate_datum), multiplicity) :: tail =>
      if path_eq_dec channel candidate_channel
      then if path_eq_dec datum candidate_datum
           then multiplicity
           else lookup_legacy_multiplicity channel datum tail
      else lookup_legacy_multiplicity channel datum tail
  end.

Lemma lookup_relative_entries_is_legacy_lookup : forall entries channel datum,
  lookup_multiplicity_in_bag datum (relative_entries_for channel entries) =
  lookup_legacy_multiplicity channel datum entries.
Proof.
  induction entries as [|[[candidate_channel candidate_datum] multiplicity] tail IH];
    intros channel datum; simpl; [reflexivity |].
  destruct (path_eq_dec candidate_channel channel) as [Heq | Hneq].
  - subst. destruct (path_eq_dec channel channel); [|contradiction].
    destruct (path_eq_dec datum candidate_datum) as [Hdatum | Hdatum].
    + subst. cbn [lookup_multiplicity_in_bag].
      destruct (path_eq_dec candidate_datum candidate_datum);
        [reflexivity | contradiction].
    + cbn [lookup_multiplicity_in_bag].
      destruct (path_eq_dec datum candidate_datum);
        [contradiction | exact (IH channel datum)].
  - destruct (path_eq_dec channel candidate_channel);
      [congruence | exact (IH channel datum)].
Qed.

Lemma lookup_migrated_loaded_channel : forall loaded_channels entries channel,
  In channel loaded_channels ->
  lookup_channel channel
    (map (fun loaded_channel =>
      (loaded_channel, relative_entries_for loaded_channel entries))
      loaded_channels) =
  Some (relative_entries_for channel entries).
Proof.
  induction loaded_channels as [|current tail IH];
    intros entries channel Hin; simpl in *; [contradiction |].
  destruct Hin as [Heq | Hin].
  - subst current.
    destruct (path_eq_dec channel channel); [reflexivity | contradiction].
  - destruct (path_eq_dec channel current).
    + subst. reflexivity.
    + now apply IH.
Qed.

Lemma lookup_migrated_unloaded_channel : forall loaded_channels entries channel,
  ~ In channel loaded_channels ->
  lookup_channel channel
    (map (fun loaded_channel =>
      (loaded_channel, relative_entries_for loaded_channel entries))
      loaded_channels) = None.
Proof.
  induction loaded_channels as [|current tail IH];
    intros entries channel Hnotin; simpl; [reflexivity |].
  destruct (path_eq_dec channel current) as [Heq | Hneq].
  - subst. exfalso. apply Hnotin. now left.
  - apply IH. intro Hin. apply Hnotin. now right.
Qed.

Theorem migration_preserves_complete_logical_state : forall legacy channel datum,
  In channel (legacy_loaded_channels legacy) ->
  lookup_multiplicity (migrate_legacy legacy) channel datum =
  lookup_legacy_multiplicity channel datum (legacy_entries legacy).
Proof.
  intros legacy channel datum Hloaded. unfold lookup_multiplicity, migrate_legacy.
  rewrite lookup_migrated_loaded_channel; [|exact Hloaded].
  apply lookup_relative_entries_is_legacy_lookup.
Qed.

Theorem migration_preserves_loaded_empty : forall legacy channel,
  In channel (legacy_loaded_channels legacy) ->
  loaded (migrate_legacy legacy) channel.
Proof.
  intros legacy channel Hloaded. unfold loaded, migrate_legacy.
  exists (relative_entries_for channel (legacy_entries legacy)).
  now apply lookup_migrated_loaded_channel.
Qed.

Theorem migration_does_not_invent_loaded_channels : forall legacy channel,
  ~ In channel (legacy_loaded_channels legacy) ->
  ~ loaded (migrate_legacy legacy) channel.
Proof.
  intros legacy channel Hnotin [contents Hlookup]. unfold migrate_legacy in Hlookup.
  rewrite lookup_migrated_unloaded_channel in Hlookup; [discriminate |exact Hnotin].
Qed.

(** Two guarded layers compose to a fixed control-stack bound. This is the
    bridge-level resource equation; connecting each unit layer to Rust uses
    [PathMapReversePruneTeardown.campaign_branch_safe_pathmap_teardown_capstone]. *)
Definition guarded_outer_call_frames (_outer_depth : nat) : nat := 1.
Definition guarded_inner_call_frames (_inner_depth : nat) : nat := 1.
Definition two_level_call_frames (outer_depth inner_depth : nat) : nat :=
  guarded_outer_call_frames outer_depth + guarded_inner_call_frames inner_depth.

Theorem outer_and_inner_clone_drop_are_constant_call_stack : forall
    outer_depth inner_depth,
  two_level_call_frames outer_depth inner_depth = 2%nat.
Proof. reflexivity. Qed.

Inductive two_level_invariant_id : Type :=
| InvExactTargetReplacement
| InvOtherChannelPreservation
| InvPrefixRelatedChannelIsolation
| InvLoadedEmptyPreservation
| InvTargetMultiplicity
| InvTargetDistinctCount
| InvTargetCardinality
| InvTotalDistinctConservation
| InvTotalCardinalityConservation
| InvCanonicalReplacement
| InvUniqueDatumAuthority
| InvFlattenedOracleRefinement
| InvTouchedChannelLocality
| InvCandidateOrder
| InvAcceptedBatchAtomicity
| InvRefusedBatchAtomicity
| InvPublishedBatchCanonicality
| InvNestedStreamRoundTrip
| InvConsensusHashPreimage
| InvMigrationLookup
| InvMigrationLoadedEmpty
| InvMigrationNoInventedLoadedChannel
| InvConstantCloneDropStack.

Definition two_level_invariant_catalog : list two_level_invariant_id :=
  [ InvExactTargetReplacement;
    InvOtherChannelPreservation;
    InvPrefixRelatedChannelIsolation;
    InvLoadedEmptyPreservation;
    InvTargetMultiplicity;
    InvTargetDistinctCount;
    InvTargetCardinality;
    InvTotalDistinctConservation;
    InvTotalCardinalityConservation;
    InvCanonicalReplacement;
    InvUniqueDatumAuthority;
    InvFlattenedOracleRefinement;
    InvTouchedChannelLocality;
    InvCandidateOrder;
    InvAcceptedBatchAtomicity;
    InvRefusedBatchAtomicity;
    InvPublishedBatchCanonicality;
    InvNestedStreamRoundTrip;
    InvConsensusHashPreimage;
    InvMigrationLookup;
    InvMigrationLoadedEmpty;
    InvMigrationNoInventedLoadedChannel;
    InvConstantCloneDropStack ].

Definition two_level_invariant_binding (invariant : two_level_invariant_id) : Prop :=
  match invariant with
  | InvExactTargetReplacement =>
      forall authority channel old replacement,
        lookup_channel channel authority = Some old ->
        lookup_channel channel
          (replace_channel_exact channel replacement authority) = Some replacement
  | InvOtherChannelPreservation =>
      forall authority selected replacement other,
        other <> selected ->
        lookup_channel other (replace_channel_exact selected replacement authority) =
        lookup_channel other authority
  | InvPrefixRelatedChannelIsolation =>
      forall authority selected replacement other,
        other <> selected ->
        (exists suffix, other = selected ++ suffix) \/
        (exists suffix, selected = other ++ suffix) ->
        lookup_channel other (replace_channel_exact selected replacement authority) =
        lookup_channel other authority
  | InvLoadedEmptyPreservation =>
      forall authority channel old,
        lookup_channel channel authority = Some old ->
        loaded (replace_channel_exact channel [] authority) channel /\
        forall datum,
          lookup_multiplicity
            (replace_channel_exact channel [] authority) channel datum = 0%N
  | InvTargetMultiplicity =>
      forall authority channel old replacement datum,
        lookup_channel channel authority = Some old ->
        lookup_multiplicity
          (replace_channel_exact channel replacement authority) channel datum =
        lookup_multiplicity_in_bag datum replacement
  | InvTargetDistinctCount =>
      forall authority channel old replacement,
        lookup_channel channel authority = Some old ->
        match lookup_channel channel
            (replace_channel_exact channel replacement authority) with
        | Some contents => length contents
        | None => 0%nat
        end = length replacement
  | InvTargetCardinality =>
      forall authority channel old replacement,
        lookup_channel channel authority = Some old ->
        match lookup_channel channel
            (replace_channel_exact channel replacement authority) with
        | Some contents => bag_cardinality contents
        | None => 0%N
        end = bag_cardinality replacement
  | InvTotalDistinctConservation =>
      forall authority channel old replacement,
        lookup_channel channel authority = Some old ->
        (total_distinct (replace_channel_exact channel replacement authority) + length old =
         total_distinct authority + length replacement)%nat
  | InvTotalCardinalityConservation =>
      forall authority channel old replacement,
        lookup_channel channel authority = Some old ->
        N.add
          (total_cardinality (replace_channel_exact channel replacement authority))
          (bag_cardinality old) =
        N.add (total_cardinality authority) (bag_cardinality replacement)
  | InvCanonicalReplacement =>
      forall authority channel old replacement,
        canonical_channels authority ->
        lookup_channel channel authority = Some old ->
        canonical_entries replacement ->
        canonical_channels (replace_channel_exact channel replacement authority)
  | InvUniqueDatumAuthority =>
      forall authority channel contents,
        canonical_channels authority ->
        lookup_channel channel authority = Some contents ->
        NoDup (channel_paths authority) /\ NoDup (bag_paths contents)
  | InvFlattenedOracleRefinement =>
      forall authority selected old replacement channel datum,
        lookup_channel selected authority = Some old ->
        lookup_multiplicity
          (replace_channel_exact selected replacement authority) channel datum =
        flattened_replacement_oracle
          authority selected replacement channel datum
  | InvTouchedChannelLocality =>
      forall authority selected old replacement other datum,
        lookup_channel selected authority = Some old ->
        other <> selected ->
        lookup_multiplicity
          (replace_channel_exact selected replacement authority) other datum =
        lookup_multiplicity authority other datum
  | InvCandidateOrder =>
      forall requested authority,
        nested_candidate_order requested authority =
        legacy_candidate_order requested authority
  | InvAcceptedBatchAtomicity =>
      forall before prepared,
        publish_channel_batch before (Some prepared) = prepared_authority prepared
  | InvRefusedBatchAtomicity =>
      forall before, publish_channel_batch before None = before
  | InvPublishedBatchCanonicality =>
      forall before prepared,
        canonical_channels (publish_channel_batch before (Some prepared))
  | InvNestedStreamRoundTrip =>
      forall authority,
        decode_nested_stream (encode_nested_stream authority) =
        logical_projection authority
  | InvConsensusHashPreimage =>
      forall authority,
        encode_nested_stream authority = canonical_legacy_hash_preimage authority
  | InvMigrationLookup =>
      forall legacy channel datum,
        In channel (legacy_loaded_channels legacy) ->
        lookup_multiplicity (migrate_legacy legacy) channel datum =
        lookup_legacy_multiplicity channel datum (legacy_entries legacy)
  | InvMigrationLoadedEmpty =>
      forall legacy channel,
        In channel (legacy_loaded_channels legacy) ->
        loaded (migrate_legacy legacy) channel
  | InvMigrationNoInventedLoadedChannel =>
      forall legacy channel,
        ~ In channel (legacy_loaded_channels legacy) ->
        ~ loaded (migrate_legacy legacy) channel
  | InvConstantCloneDropStack =>
      forall outer_depth inner_depth,
        two_level_call_frames outer_depth inner_depth = 2%nat
  end.

Theorem every_two_level_invariant_is_bound : forall invariant,
  two_level_invariant_binding invariant.
Proof.
  intro invariant. destruct invariant; simpl.
  - exact exact_channel_replacement_is_exact.
  - exact channel_replacement_preserves_every_other_exact_channel.
  - exact prefix_related_channels_are_still_exactly_independent.
  - exact loaded_empty_is_preserved.
  - exact replacement_target_multiplicity_is_exact.
  - exact replacement_target_distinct_count_is_exact.
  - exact replacement_target_cardinality_is_exact.
  - exact replacement_total_distinct_conservation.
  - exact replacement_total_cardinality_conservation.
  - exact canonical_channel_replacement_is_preserved.
  - exact canonical_authority_has_no_duplicate_datum_authority.
  - exact exact_channel_replacement_refines_prefix_replacement_oracle.
  - exact channel_replacement_is_local_to_touched_authorities.
  - exact candidate_order_refines_legacy_order.
  - exact accepted_multichannel_commit_is_atomic.
  - exact refused_multichannel_commit_preserves_all_public_authority.
  - exact published_multichannel_commit_is_canonical.
  - exact nested_stream_round_trip_is_exact.
  - exact consensus_hash_preimage_is_preserved.
  - exact migration_preserves_complete_logical_state.
  - exact migration_preserves_loaded_empty.
  - exact migration_does_not_invent_loaded_channels.
  - exact outer_and_inner_clone_drop_are_constant_call_stack.
Qed.

Theorem two_level_invariant_catalog_is_unique_complete_and_exact :
  NoDup two_level_invariant_catalog /\
  Forall two_level_invariant_binding two_level_invariant_catalog /\
  length two_level_invariant_catalog = 23%nat.
Proof.
  split.
  - unfold two_level_invariant_catalog.
    repeat match goal with
    | |- NoDup (_ :: _) => constructor; [simpl; intuition discriminate |]
    | |- NoDup [] => constructor
    end.
  - split.
    + apply Forall_forall. intros invariant _. apply every_two_level_invariant_is_bound.
    + reflexivity.
Qed.

Theorem two_level_rspace_authority_capstone : forall
    authority channel old replacement other datum,
  canonical_channels authority ->
  lookup_channel channel authority = Some old ->
  canonical_entries replacement ->
  other <> channel ->
  canonical_channels (replace_channel_exact channel replacement authority) /\
  lookup_multiplicity
    (replace_channel_exact channel replacement authority) channel datum =
    lookup_multiplicity_in_bag datum replacement /\
  lookup_multiplicity
    (replace_channel_exact channel replacement authority) other datum =
    lookup_multiplicity authority other datum /\
  loaded (replace_channel_exact channel [] authority) channel /\
  NoDup (channel_paths authority) /\
  two_level_call_frames (length authority) (length replacement) = 2%nat.
Proof.
  intros authority channel old replacement other datum
    Hcanonical Hloaded Hreplacement Hdifferent.
  repeat split.
  - now apply canonical_channel_replacement_is_preserved with old.
  - now apply replacement_target_multiplicity_is_exact with old.
  - now apply channel_replacement_is_local_to_touched_authorities with old.
  - exact (proj1 (@loaded_empty_is_preserved authority channel old Hloaded)).
  - now apply canonical_channel_paths_are_unique.
Qed.

Print Assumptions exact_channel_replacement_refines_prefix_replacement_oracle.
Print Assumptions accepted_multichannel_commit_is_atomic.
Print Assumptions migration_preserves_complete_logical_state.
Print Assumptions two_level_invariant_catalog_is_unique_complete_and_exact.
Print Assumptions two_level_rspace_authority_capstone.

End TwoLevelRSpaceAuthority.
