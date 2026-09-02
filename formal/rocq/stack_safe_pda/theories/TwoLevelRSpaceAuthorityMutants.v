From Stdlib Require Import Bool List Arith Lia NArith.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Open Scope N_scope.

(** Concrete causal negative controls for every invariant in the selected
    two-level RSpace authority. *)
Module TwoLevelRSpaceAuthorityMutants.

Definition target_channel : Path := [1%nat].
Definition descendant_channel : Path := [1%nat; 2%nat].
Definition sibling_channel : Path := [2%nat].

Definition target_datum : Path := [3%nat].
Definition second_datum : Path := [4%nat].
Definition descendant_datum : Path := [5%nat].
Definition sibling_datum : Path := [6%nat].

Definition old_target_bag : ChannelBag :=
  [(target_datum, 2%N)].

Definition replacement_bag : ChannelBag :=
  [(target_datum, 7%N); (second_datum, 11%N)].

Definition descendant_bag : ChannelBag :=
  [(descendant_datum, 13%N)].

Definition sibling_bag : ChannelBag :=
  [(sibling_datum, 17%N)].

Definition authority_fixture : TwoLevelAuthority :=
  [(target_channel, old_target_bag);
   (descendant_channel, descendant_bag);
   (sibling_channel, sibling_bag)].

Theorem authority_fixture_is_non_vacuous :
  lookup_channel target_channel authority_fixture = Some old_target_bag /\
  lookup_channel descendant_channel authority_fixture = Some descendant_bag /\
  lookup_channel sibling_channel authority_fixture = Some sibling_bag /\
  lookup_multiplicity authority_fixture target_channel target_datum = 2%N /\
  lookup_multiplicity authority_fixture descendant_channel descendant_datum =
    13%N /\
  lookup_multiplicity authority_fixture sibling_channel sibling_datum = 17%N.
Proof. vm_compute. repeat split; reflexivity. Qed.

(** InvExactTargetReplacement: acknowledge the command but retain the old
    target channel. *)
Definition mutant_skip_exact_target_replacement : TwoLevelAuthority :=
  authority_fixture.

Theorem kill_skipped_exact_target_replacement :
  lookup_channel target_channel mutant_skip_exact_target_replacement <>
  lookup_channel target_channel
    (replace_channel_exact target_channel replacement_bag authority_fixture).
Proof. vm_compute. discriminate. Qed.

(** InvOtherChannelPreservation: replace the target but erase an unrelated
    exact channel. *)
Definition mutant_erase_unrelated_channel : TwoLevelAuthority :=
  [(target_channel, replacement_bag);
   (descendant_channel, descendant_bag);
   (sibling_channel, [])].

Theorem kill_unrelated_exact_channel_erasure :
  lookup_channel sibling_channel mutant_erase_unrelated_channel <>
  lookup_channel sibling_channel
    (replace_channel_exact target_channel replacement_bag authority_fixture).
Proof. vm_compute. discriminate. Qed.

(** InvPrefixRelatedChannelIsolation: treat exact channel replacement as a
    prefix operation and erase a descendant channel. *)
Definition mutant_prefix_aware_channel_replacement : TwoLevelAuthority :=
  [(target_channel, replacement_bag);
   (descendant_channel, []);
   (sibling_channel, sibling_bag)].

Theorem kill_prefix_related_channel_mutation :
  lookup_channel descendant_channel mutant_prefix_aware_channel_replacement <>
  lookup_channel descendant_channel
    (replace_channel_exact target_channel replacement_bag authority_fixture).
Proof. vm_compute. discriminate. Qed.

(** InvLoadedEmptyPreservation: conflate a loaded empty channel with an
    unloaded channel by deleting its outer endpoint. *)
Definition mutant_drop_loaded_empty_endpoint : TwoLevelAuthority :=
  [(descendant_channel, descendant_bag);
   (sibling_channel, sibling_bag)].

Theorem kill_loaded_empty_endpoint_removal :
  ~ loaded mutant_drop_loaded_empty_endpoint target_channel.
Proof.
  unfold loaded. intros [contents Hlookup].
  vm_compute in Hlookup. discriminate.
Qed.

(** InvTargetMultiplicity: collapse a replacement multiplicity to set
    membership. *)
Definition mutant_flatten_target_multiplicity : TwoLevelAuthority :=
  replace_channel_exact
    target_channel [(target_datum, 1%N)] authority_fixture.

Theorem kill_target_multiplicity_flattening :
  lookup_multiplicity
      mutant_flatten_target_multiplicity target_channel target_datum <>
  lookup_multiplicity_in_bag target_datum replacement_bag.
Proof. vm_compute. discriminate. Qed.

(** InvTargetDistinctCount: omit one distinct replacement endpoint. *)
Definition mutant_skip_distinct_replacement_entry : TwoLevelAuthority :=
  replace_channel_exact
    target_channel [(target_datum, 7%N)] authority_fixture.

Theorem kill_target_distinct_count_loss :
  (match lookup_channel
      target_channel mutant_skip_distinct_replacement_entry with
   | Some contents => length contents
   | None => 0%nat
   end) <> length replacement_bag.
Proof. vm_compute. discriminate. Qed.

(** InvTargetCardinality: retain the endpoint set but corrupt both
    multiplicities. *)
Definition mutant_corrupt_target_cardinality : TwoLevelAuthority :=
  replace_channel_exact
    target_channel
    [(target_datum, 1%N); (second_datum, 1%N)]
    authority_fixture.

Theorem kill_target_cardinality_corruption :
  (match lookup_channel target_channel mutant_corrupt_target_cardinality with
   | Some contents => bag_cardinality contents
   | None => 0%N
   end) <> bag_cardinality replacement_bag.
Proof. vm_compute. discriminate. Qed.

(** InvTotalDistinctConservation: publish the replacement while losing an
    unrelated distinct endpoint. *)
Definition mutant_total_distinct_loss : TwoLevelAuthority :=
  [(target_channel, replacement_bag);
   (descendant_channel, descendant_bag)].

Theorem kill_total_distinct_conservation_failure :
  (total_distinct mutant_total_distinct_loss + length old_target_bag <>
   total_distinct authority_fixture + length replacement_bag)%nat.
Proof. vm_compute. discriminate. Qed.

(** InvTotalCardinalityConservation: preserve the topology but publish
    incorrect replacement counts. *)
Definition mutant_total_cardinality_corruption : TwoLevelAuthority :=
  mutant_corrupt_target_cardinality.

Theorem kill_total_cardinality_conservation_failure :
  N.add
      (total_cardinality mutant_total_cardinality_corruption)
      (bag_cardinality old_target_bag) <>
  N.add
      (total_cardinality authority_fixture)
      (bag_cardinality replacement_bag).
Proof. vm_compute. discriminate. Qed.

(** InvCanonicalReplacement: replay replacement endpoints in descending byte
    order. *)
Definition mutant_noncanonical_replacement : ChannelBag :=
  [(second_datum, 11%N); (target_datum, 7%N)].

Theorem kill_noncanonical_replacement_order :
  ~ canonical_entries mutant_noncanonical_replacement.
Proof.
  intro Hcanonical.
  inversion Hcanonical as
      [|path multiplicity tail Hpath Hmultiplicity Hordered Htail];
    subst.
  inversion Hordered as [|entry rest Hlt Hrest]; subst.
  unfold second_datum, target_datum in Hlt.
  inversion Hlt; lia.
Qed.

(** InvUniqueDatumAuthority: install the same relative datum twice in one
    authoritative channel Bag. *)
Definition mutant_duplicate_datum_bag : ChannelBag :=
  [(target_datum, 2%N); (target_datum, 3%N)].

Theorem kill_duplicate_datum_authority :
  ~ NoDup (bag_paths mutant_duplicate_datum_bag).
Proof.
  unfold bag_paths, mutant_duplicate_datum_bag.
  simpl. intro Hnodup.
  inversion Hnodup as [|first tail Hnotin Htail]; subst.
  apply Hnotin. simpl. auto.
Qed.

(** InvFlattenedOracleRefinement: use prefix-aware subtraction when the
    flattened oracle requires exact channel replacement. *)
Definition mutant_flattened_prefix_prune : TwoLevelAuthority :=
  mutant_prefix_aware_channel_replacement.

Theorem kill_flattened_prefix_subtraction :
  lookup_multiplicity
      mutant_flattened_prefix_prune
      descendant_channel
      descendant_datum <>
  flattened_replacement_oracle
      authority_fixture
      target_channel
      replacement_bag
      descendant_channel
      descendant_datum.
Proof. vm_compute. discriminate. Qed.

(** InvTouchedChannelLocality: mutate a sibling channel while publishing the
    selected channel. *)
Definition mutant_cross_channel_write : TwoLevelAuthority :=
  [(target_channel, replacement_bag);
   (descendant_channel, descendant_bag);
   (sibling_channel, [(sibling_datum, 1%N)])].

Theorem kill_cross_channel_write :
  lookup_multiplicity
      mutant_cross_channel_write sibling_channel sibling_datum <>
  lookup_multiplicity authority_fixture sibling_channel sibling_datum.
Proof. vm_compute. discriminate. Qed.

(** InvCandidateOrder: reverse the canonical nested logical stream and use it
    as candidate order. *)
Definition mutant_reverse_candidate_order : list LogicalEntry :=
  rev (logical_projection authority_fixture).

Theorem kill_reversed_candidate_order :
  mutant_reverse_candidate_order <> logical_projection authority_fixture.
Proof. vm_compute. discriminate. Qed.

Definition empty_prepared_batch : PreparedBatch :=
  @PrepareBatch [] CanonicalChannelsNil.

(** InvAcceptedBatchAtomicity: report success but retain the pre-transaction
    authority. *)
Definition mutant_accepted_batch_keeps_before : TwoLevelAuthority :=
  authority_fixture.

Theorem kill_accepted_batch_without_publication :
  mutant_accepted_batch_keeps_before <>
  publish_channel_batch authority_fixture (Some empty_prepared_batch).
Proof. vm_compute. discriminate. Qed.

(** InvRefusedBatchAtomicity: publish the rejected candidate on failure. *)
Definition mutant_refused_batch_publishes_candidate : TwoLevelAuthority :=
  prepared_authority empty_prepared_batch.

Theorem kill_refused_batch_publication :
  mutant_refused_batch_publishes_candidate <>
  publish_channel_batch authority_fixture None.
Proof. vm_compute. discriminate. Qed.

(** InvPublishedBatchCanonicality: publish outer channels in descending byte
    order. *)
Definition mutant_noncanonical_batch_publication : TwoLevelAuthority :=
  [(sibling_channel, []); (target_channel, [])].

Theorem kill_noncanonical_batch_publication :
  ~ canonical_channels mutant_noncanonical_batch_publication.
Proof.
  intro Hcanonical.
  inversion Hcanonical as
      [|channel contents tail Hpath Hcontents Hordered Htail];
    subst.
  inversion Hordered as [|entry rest Hlt Hrest]; subst.
  unfold sibling_channel, target_channel in Hlt.
  inversion Hlt; lia.
Qed.

(** InvNestedStreamRoundTrip: lose the first nested logical endpoint while
    decoding. *)
Definition mutant_decode_nested_stream
    (image : list LogicalEntry) : list LogicalEntry :=
  tl image.

Theorem kill_lossy_nested_stream_round_trip :
  mutant_decode_nested_stream (encode_nested_stream authority_fixture) <>
  logical_projection authority_fixture.
Proof. vm_compute. discriminate. Qed.

(** InvConsensusHashPreimage: hash a physical reverse traversal instead of the
    canonical logical byte order. *)
Definition mutant_consensus_hash_preimage : list LogicalEntry :=
  rev (encode_nested_stream authority_fixture).

Theorem kill_physical_order_hash_preimage :
  mutant_consensus_hash_preimage <>
  canonical_legacy_hash_preimage authority_fixture.
Proof. vm_compute. discriminate. Qed.

Definition legacy_fixture : LegacyAuthority :=
  LegacyState
    [target_channel; descendant_channel]
    [((target_channel, target_datum), 2%N);
     ((target_channel, second_datum), 7%N)].

(** InvMigrationLookup: flatten a migrated multiplicity to one. *)
Definition mutant_migration_flattens_multiplicity : TwoLevelAuthority :=
  [(target_channel, [(target_datum, 1%N); (second_datum, 7%N)]);
   (descendant_channel, [])].

Theorem kill_migration_lookup_corruption :
  lookup_multiplicity
      mutant_migration_flattens_multiplicity
      target_channel
      target_datum <>
  lookup_legacy_multiplicity
      target_channel
      target_datum
      (legacy_entries legacy_fixture).
Proof. vm_compute. discriminate. Qed.

(** InvMigrationLoadedEmpty: omit a loaded channel whose legacy logical entry
    set is empty. *)
Definition mutant_migration_omits_loaded_empty : TwoLevelAuthority :=
  [(target_channel,
    [(target_datum, 2%N); (second_datum, 7%N)])].

Theorem kill_migration_loaded_empty_omission :
  ~ loaded mutant_migration_omits_loaded_empty descendant_channel /\
  loaded (migrate_legacy legacy_fixture) descendant_channel.
Proof.
  split.
  - unfold loaded. intros [contents Hlookup].
    vm_compute in Hlookup. discriminate.
  - unfold loaded. exists [].
    vm_compute. reflexivity.
Qed.

(** InvMigrationNoInventedLoadedChannel: synthesize an unrelated loaded-empty
    channel absent from the legacy loaded-channel authority. *)
Definition mutant_migration_invents_loaded_channel : TwoLevelAuthority :=
  migrate_legacy legacy_fixture ++ [(sibling_channel, [])].

Theorem kill_migration_invented_loaded_channel :
  ~ In sibling_channel (legacy_loaded_channels legacy_fixture) /\
  loaded mutant_migration_invents_loaded_channel sibling_channel.
Proof.
  split.
  - vm_compute. intuition discriminate.
  - unfold loaded. exists [].
    vm_compute. reflexivity.
Qed.

(** InvConstantCloneDropStack: implement nested teardown by recursive calls
    proportional to both trie depths. *)
Definition mutant_recursive_call_frames
    (outer_depth inner_depth : nat) : nat :=
  S outer_depth + S inner_depth.

Theorem kill_depth_proportional_clone_drop_stack :
  mutant_recursive_call_frames 4%nat 8%nat <>
  two_level_call_frames 4%nat 8%nat.
Proof. vm_compute. discriminate. Qed.

(** The mutant authority is deliberately indexed by the production invariant
    identifier. This makes the binding total by construction and prevents a
    second, silently divergent name catalog. *)
Definition two_level_mutant_killed
    (invariant : two_level_invariant_id) : Prop :=
  match invariant with
  | InvExactTargetReplacement =>
      lookup_channel target_channel mutant_skip_exact_target_replacement <>
      lookup_channel target_channel
        (replace_channel_exact
          target_channel replacement_bag authority_fixture)
  | InvOtherChannelPreservation =>
      lookup_channel sibling_channel mutant_erase_unrelated_channel <>
      lookup_channel sibling_channel
        (replace_channel_exact
          target_channel replacement_bag authority_fixture)
  | InvPrefixRelatedChannelIsolation =>
      lookup_channel
          descendant_channel mutant_prefix_aware_channel_replacement <>
      lookup_channel descendant_channel
        (replace_channel_exact
          target_channel replacement_bag authority_fixture)
  | InvLoadedEmptyPreservation =>
      ~ loaded mutant_drop_loaded_empty_endpoint target_channel
  | InvTargetMultiplicity =>
      lookup_multiplicity
          mutant_flatten_target_multiplicity
          target_channel
          target_datum <>
      lookup_multiplicity_in_bag target_datum replacement_bag
  | InvTargetDistinctCount =>
      (match lookup_channel
          target_channel mutant_skip_distinct_replacement_entry with
       | Some contents => length contents
       | None => 0%nat
       end) <> length replacement_bag
  | InvTargetCardinality =>
      (match lookup_channel
          target_channel mutant_corrupt_target_cardinality with
       | Some contents => bag_cardinality contents
       | None => 0%N
       end) <> bag_cardinality replacement_bag
  | InvTotalDistinctConservation =>
      (total_distinct mutant_total_distinct_loss + length old_target_bag <>
       total_distinct authority_fixture + length replacement_bag)%nat
  | InvTotalCardinalityConservation =>
      N.add
          (total_cardinality mutant_total_cardinality_corruption)
          (bag_cardinality old_target_bag) <>
      N.add
          (total_cardinality authority_fixture)
          (bag_cardinality replacement_bag)
  | InvCanonicalReplacement =>
      ~ canonical_entries mutant_noncanonical_replacement
  | InvUniqueDatumAuthority =>
      ~ NoDup (bag_paths mutant_duplicate_datum_bag)
  | InvFlattenedOracleRefinement =>
      lookup_multiplicity
          mutant_flattened_prefix_prune
          descendant_channel
          descendant_datum <>
      flattened_replacement_oracle
          authority_fixture
          target_channel
          replacement_bag
          descendant_channel
          descendant_datum
  | InvTouchedChannelLocality =>
      lookup_multiplicity
          mutant_cross_channel_write sibling_channel sibling_datum <>
      lookup_multiplicity authority_fixture sibling_channel sibling_datum
  | InvCandidateOrder =>
      mutant_reverse_candidate_order <> logical_projection authority_fixture
  | InvAcceptedBatchAtomicity =>
      mutant_accepted_batch_keeps_before <>
      publish_channel_batch authority_fixture (Some empty_prepared_batch)
  | InvRefusedBatchAtomicity =>
      mutant_refused_batch_publishes_candidate <>
      publish_channel_batch authority_fixture None
  | InvPublishedBatchCanonicality =>
      ~ canonical_channels mutant_noncanonical_batch_publication
  | InvNestedStreamRoundTrip =>
      mutant_decode_nested_stream
          (encode_nested_stream authority_fixture) <>
      logical_projection authority_fixture
  | InvConsensusHashPreimage =>
      mutant_consensus_hash_preimage <>
      canonical_legacy_hash_preimage authority_fixture
  | InvMigrationLookup =>
      lookup_multiplicity
          mutant_migration_flattens_multiplicity
          target_channel
          target_datum <>
      lookup_legacy_multiplicity
          target_channel
          target_datum
          (legacy_entries legacy_fixture)
  | InvMigrationLoadedEmpty =>
      ~ loaded mutant_migration_omits_loaded_empty descendant_channel /\
      loaded (migrate_legacy legacy_fixture) descendant_channel
  | InvMigrationNoInventedLoadedChannel =>
      ~ In sibling_channel (legacy_loaded_channels legacy_fixture) /\
      loaded mutant_migration_invents_loaded_channel sibling_channel
  | InvConstantCloneDropStack =>
      mutant_recursive_call_frames 4%nat 8%nat <>
      two_level_call_frames 4%nat 8%nat
  end.

Theorem every_two_level_mutant_has_a_concrete_causal_witness :
  forall invariant, two_level_mutant_killed invariant.
Proof.
  intro invariant. destruct invariant; cbn.
  - apply kill_skipped_exact_target_replacement.
  - apply kill_unrelated_exact_channel_erasure.
  - apply kill_prefix_related_channel_mutation.
  - apply kill_loaded_empty_endpoint_removal.
  - apply kill_target_multiplicity_flattening.
  - apply kill_target_distinct_count_loss.
  - apply kill_target_cardinality_corruption.
  - apply kill_total_distinct_conservation_failure.
  - apply kill_total_cardinality_conservation_failure.
  - apply kill_noncanonical_replacement_order.
  - apply kill_duplicate_datum_authority.
  - apply kill_flattened_prefix_subtraction.
  - apply kill_cross_channel_write.
  - apply kill_reversed_candidate_order.
  - apply kill_accepted_batch_without_publication.
  - apply kill_refused_batch_publication.
  - apply kill_noncanonical_batch_publication.
  - apply kill_lossy_nested_stream_round_trip.
  - apply kill_physical_order_hash_preimage.
  - apply kill_migration_lookup_corruption.
  - apply kill_migration_loaded_empty_omission.
  - apply kill_migration_invented_loaded_channel.
  - apply kill_depth_proportional_clone_drop_stack.
Qed.

Definition two_level_mutant_catalog : list two_level_invariant_id :=
  two_level_invariant_catalog.

Theorem two_level_mutant_catalog_is_total_unique_and_exact :
  NoDup two_level_mutant_catalog /\
  Forall two_level_mutant_killed two_level_mutant_catalog /\
  length two_level_mutant_catalog = 23%nat.
Proof.
  destruct two_level_invariant_catalog_is_unique_complete_and_exact
    as [Hunique [_ Hlength]].
  split.
  - exact Hunique.
  - split.
    + apply Forall_forall.
    intros invariant _.
    apply every_two_level_mutant_has_a_concrete_causal_witness.
    + exact Hlength.
Qed.

Print Assumptions authority_fixture_is_non_vacuous.
Print Assumptions every_two_level_mutant_has_a_concrete_causal_witness.
Print Assumptions two_level_mutant_catalog_is_total_unique_and_exact.

End TwoLevelRSpaceAuthorityMutants.
