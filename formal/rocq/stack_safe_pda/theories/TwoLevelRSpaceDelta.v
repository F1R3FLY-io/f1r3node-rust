From Stdlib Require Import Bool List Arith Lia NArith.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublication
  TwoLevelRSpaceAuthority
  TwoLevelRSpaceOperations.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceOperations.
Set Implicit Arguments.

(** Refinement of one flat, normalized production [BagDelta] into independent
    exact-channel inner-Bag updates.  Every prepared channel delta is derived
    from the same immutable outer authority; duplicate channel targets are
    rejected before a single new outer root is published. *)
Module TwoLevelRSpaceDelta.

Definition channel_contents_or_empty
    (authority : TwoLevelAuthority) (channel : Path) : ChannelBag :=
  match lookup_channel channel authority with
  | Some contents => contents
  | None => []
  end.

Record PreparedChannelDelta (before : TwoLevelAuthority) : Type := {
  prepared_delta_channel : Path;
  prepared_delta_change : BagDelta;
  prepared_delta_output : ChannelBag;
  prepared_delta_channel_valid : path_valid prepared_delta_channel = true;
  prepared_delta_absent_channel_has_addition :
    loaded before prepared_delta_channel \/
    bag_delta_additions prepared_delta_change <> [];
  prepared_delta_operation_valid :
    valid_logical_operation
      (channel_contents_or_empty before prepared_delta_channel)
      (OpBagDelta prepared_delta_change);
  prepared_delta_execution_succeeds :
    execute_logical_operation
      (channel_contents_or_empty before prepared_delta_channel)
      (OpBagDelta prepared_delta_change) = Some prepared_delta_output
}.

Theorem prepared_delta_output_is_canonical : forall
    (before : TwoLevelAuthority) (prepared : PreparedChannelDelta before),
  canonical_entries (prepared_delta_output prepared).
Proof.
  intros before prepared.
  eapply successful_valid_logical_operation_is_canonical.
  - exact (prepared_delta_operation_valid prepared).
  - exact (prepared_delta_execution_succeeds prepared).
Qed.

Theorem prepared_delta_rejects_an_absent_no_addition_channel : forall
    (before : TwoLevelAuthority) (prepared : PreparedChannelDelta before),
  ~ loaded before (prepared_delta_channel prepared) ->
  bag_delta_additions (prepared_delta_change prepared) <> [].
Proof.
  intros before prepared Hunloaded.
  destruct (prepared_delta_absent_channel_has_addition prepared)
    as [Hloaded | Haddition]; [contradiction | exact Haddition].
Qed.

Definition prepared_delta_as_operation {before}
    (prepared : PreparedChannelDelta before) : prepared_channel_operation :=
  @PreparedUpsert
    (prepared_delta_channel prepared)
    (prepared_delta_output prepared)
    (prepared_delta_channel_valid prepared)
    (prepared_delta_output_is_canonical prepared).

Definition prepared_delta_channels {before}
    (deltas : list (PreparedChannelDelta before)) : list Path :=
  map (fun prepared => prepared_delta_channel prepared) deltas.

Definition prepared_delta_operations {before}
    (deltas : list (PreparedChannelDelta before))
    : list prepared_channel_operation :=
  map (fun prepared => prepared_delta_as_operation prepared) deltas.

Lemma prepared_delta_operation_channels_are_exact : forall before
    (deltas : list (PreparedChannelDelta before)),
  touched_channels (prepared_delta_operations deltas) =
  prepared_delta_channels deltas.
Proof.
  intros before deltas.
  unfold touched_channels, prepared_delta_operations, prepared_delta_channels.
  rewrite map_map. apply map_ext. intro prepared.
  reflexivity.
Qed.

Record PreparedRSpaceDelta (before : TwoLevelAuthority) : Type := {
  prepared_channel_deltas : list (PreparedChannelDelta before);
  prepared_delta_channels_are_unique :
    NoDup (prepared_delta_channels prepared_channel_deltas)
}.

Definition prepared_rspace_delta_as_batch {before}
    (prepared : PreparedRSpaceDelta before) : PreparedChannelBatch.
Proof.
  refine (PrepareChannelBatch
    (prepared_delta_operations (prepared_channel_deltas prepared)) _).
  rewrite prepared_delta_operation_channels_are_exact.
  exact (prepared_delta_channels_are_unique prepared).
Defined.

Definition apply_prepared_rspace_delta {before}
    (prepared : PreparedRSpaceDelta before) : TwoLevelAuthority :=
  apply_prepared_channel_operations before
    (prepared_delta_operations (prepared_channel_deltas prepared)).

Theorem prepared_delta_head_output_is_exact : forall before
    (head : PreparedChannelDelta before) tail,
  ~ In (prepared_delta_channel head) (prepared_delta_channels tail) ->
  lookup_channel (prepared_delta_channel head)
    (apply_prepared_channel_operations
      before (prepared_delta_operations (head :: tail))) =
  Some (prepared_delta_output head).
Proof.
  intros before head tail Hunique.
  change
    (lookup_channel (prepared_delta_channel head)
      (apply_prepared_channel_operations
        (upsert_channel_exact
          (prepared_delta_channel head) (prepared_delta_output head) before)
        (prepared_delta_operations tail)) =
      Some (prepared_delta_output head)).
  transitivity
    (lookup_channel (prepared_delta_channel head)
      (upsert_channel_exact
        (prepared_delta_channel head) (prepared_delta_output head) before)).
  - apply untouched_channel_is_preserved_by_prepared_operations.
    rewrite prepared_delta_operation_channels_are_exact. exact Hunique.
  - apply upsert_target_lookup_is_exact.
Qed.

Theorem prepared_rspace_delta_head_output_is_exact : forall before
    (prepared : PreparedRSpaceDelta before) head tail,
  prepared_channel_deltas prepared = head :: tail ->
  lookup_channel (prepared_delta_channel head)
    (apply_prepared_rspace_delta prepared) =
  Some (prepared_delta_output head).
Proof.
  intros before prepared head tail Heq.
  unfold apply_prepared_rspace_delta. rewrite Heq.
  apply prepared_delta_head_output_is_exact.
  pose proof (prepared_delta_channels_are_unique prepared) as Hunique.
  rewrite Heq in Hunique.
  inversion Hunique. assumption.
Qed.

Theorem prepared_rspace_delta_preserves_every_untouched_channel : forall
    before (prepared : PreparedRSpaceDelta before) channel,
  ~ In channel (prepared_delta_channels (prepared_channel_deltas prepared)) ->
  lookup_channel channel (apply_prepared_rspace_delta prepared) =
  lookup_channel channel before.
Proof.
  intros before prepared channel Hnot_touched.
  unfold apply_prepared_rspace_delta.
  apply untouched_channel_is_preserved_by_prepared_operations.
  now rewrite prepared_delta_operation_channels_are_exact.
Qed.

Theorem prepared_rspace_delta_preserves_canonicality : forall before
    (prepared : PreparedRSpaceDelta before),
  canonical_channels before ->
  canonical_channels (apply_prepared_rspace_delta prepared).
Proof.
  intros before prepared Hcanonical.
  unfold apply_prepared_rspace_delta.
  now apply prepared_operations_preserve_canonicality.
Qed.

Section Publication.

Context {Catalog ExternalSnapshots : Type}.

Definition publish_prepared_rspace_delta
    (before : @transactional_store_state Catalog ExternalSnapshots)
    (prepared : PreparedRSpaceDelta (store_authority before))
    : transactional_store_state :=
  publish_prepared_channel_batch before
    (prepared_rspace_delta_as_batch prepared).

Inductive rspace_delta_decision : Type :=
| RSpaceDeltaRefused
| RSpaceDeltaPublished.

Record rspace_delta_outcome : Type := RSpaceDeltaOutcome {
  rspace_delta_outcome_decision : rspace_delta_decision;
  rspace_delta_outcome_state :
    @transactional_store_state Catalog ExternalSnapshots;
  rspace_delta_publication_count : nat
}.

Inductive rspace_delta_transition
    (before : @transactional_store_state Catalog ExternalSnapshots)
    : option (PreparedRSpaceDelta (store_authority before)) ->
      rspace_delta_outcome -> Prop :=
| RefuseRSpaceDelta :
    rspace_delta_transition before None
      (RSpaceDeltaOutcome RSpaceDeltaRefused before 0)
| PublishRSpaceDelta : forall prepared,
    rspace_delta_transition before (Some prepared)
      (RSpaceDeltaOutcome RSpaceDeltaPublished
        (publish_prepared_rspace_delta before prepared) 1).

Theorem refused_rspace_delta_preserves_every_public_observation : forall
    before outcome,
  rspace_delta_transition before None outcome ->
  rspace_delta_outcome_state outcome = before /\
  rspace_delta_publication_count outcome = 0%nat /\
  rspace_delta_outcome_decision outcome = RSpaceDeltaRefused.
Proof.
  intros before outcome Htransition.
  inversion Htransition; subst. repeat split; reflexivity.
Qed.

Theorem accepted_rspace_delta_publishes_exactly_one_complete_root : forall
    before prepared outcome,
  rspace_delta_transition before (Some prepared) outcome ->
  rspace_delta_outcome_state outcome =
      publish_prepared_rspace_delta before prepared /\
  rspace_delta_publication_count outcome = 1%nat /\
  rspace_delta_outcome_decision outcome = RSpaceDeltaPublished.
Proof.
  intros before prepared outcome Htransition.
  inversion Htransition; subst. repeat split; reflexivity.
Qed.

Theorem accepted_rspace_delta_preserves_every_untouched_channel : forall
    before prepared outcome channel,
  rspace_delta_transition before (Some prepared) outcome ->
  ~ In channel
      (prepared_delta_channels (prepared_channel_deltas prepared)) ->
  lookup_channel channel (store_authority (rspace_delta_outcome_state outcome)) =
  lookup_channel channel (store_authority before).
Proof.
  intros before prepared outcome channel Htransition Hnot_touched.
  inversion Htransition; subst.
  unfold publish_prepared_rspace_delta, publish_prepared_channel_batch; cbn.
  apply prepared_rspace_delta_preserves_every_untouched_channel.
  exact Hnot_touched.
Qed.

Theorem accepted_rspace_delta_invalidates_every_touched_catalog : forall
    before prepared outcome channel,
  rspace_delta_transition before (Some prepared) outcome ->
  In channel (prepared_delta_channels (prepared_channel_deltas prepared)) ->
  store_catalogs (rspace_delta_outcome_state outcome) channel = None.
Proof.
  intros before prepared outcome channel Htransition Htouched.
  inversion Htransition; subst.
  unfold publish_prepared_rspace_delta.
  apply successful_publication_invalidates_every_touched_catalog.
  simpl. rewrite prepared_delta_operation_channels_are_exact. exact Htouched.
Qed.

Theorem accepted_rspace_delta_preserves_store_well_formedness : forall
    before prepared,
  store_state_well_formed before ->
  store_state_well_formed (publish_prepared_rspace_delta before prepared).
Proof.
  intros. unfold publish_prepared_rspace_delta.
  now apply successful_publication_preserves_store_well_formedness.
Qed.

Theorem rspace_delta_transition_is_deterministic : forall
    before proposal left right,
  rspace_delta_transition before proposal left ->
  rspace_delta_transition before proposal right ->
  left = right.
Proof.
  intros before proposal left right Hleft Hright.
  inversion Hleft; inversion Hright; subst; congruence.
Qed.

End Publication.

Inductive two_level_delta_invariant_id : Type :=
| DeltaInvOutputCanonical
| DeltaInvAbsentRequiresAddition
| DeltaInvOperationChannelsExact
| DeltaInvTargetsUnique
| DeltaInvHeadOutputExact
| DeltaInvUntouchedChannelPreserved
| DeltaInvAuthorityCanonical
| DeltaInvFailureAtomic
| DeltaInvAcceptedOneRoot
| DeltaInvAcceptedUntouchedChannel
| DeltaInvTouchedCatalogInvalidated
| DeltaInvPublishedStoreWellFormed
| DeltaInvTransitionDeterministic.

Definition two_level_delta_invariant_catalog :
    list two_level_delta_invariant_id :=
  [ DeltaInvOutputCanonical;
    DeltaInvAbsentRequiresAddition;
    DeltaInvOperationChannelsExact;
    DeltaInvTargetsUnique;
    DeltaInvHeadOutputExact;
    DeltaInvUntouchedChannelPreserved;
    DeltaInvAuthorityCanonical;
    DeltaInvFailureAtomic;
    DeltaInvAcceptedOneRoot;
    DeltaInvAcceptedUntouchedChannel;
    DeltaInvTouchedCatalogInvalidated;
    DeltaInvPublishedStoreWellFormed;
    DeltaInvTransitionDeterministic ].

Definition two_level_delta_invariant_binding
    (invariant : two_level_delta_invariant_id) : Prop :=
  match invariant with
  | DeltaInvOutputCanonical =>
      forall (before : TwoLevelAuthority)
        (prepared : PreparedChannelDelta before),
        canonical_entries (prepared_delta_output prepared)
  | DeltaInvAbsentRequiresAddition =>
      forall (before : TwoLevelAuthority)
        (prepared : PreparedChannelDelta before),
        ~ loaded before (prepared_delta_channel prepared) ->
        bag_delta_additions (prepared_delta_change prepared) <> []
  | DeltaInvOperationChannelsExact =>
      forall before (deltas : list (PreparedChannelDelta before)),
        touched_channels (prepared_delta_operations deltas) =
        prepared_delta_channels deltas
  | DeltaInvTargetsUnique =>
      forall before (prepared : PreparedRSpaceDelta before),
        NoDup (prepared_delta_channels (prepared_channel_deltas prepared))
  | DeltaInvHeadOutputExact =>
      forall before (prepared : PreparedRSpaceDelta before) head tail,
        prepared_channel_deltas prepared = head :: tail ->
        lookup_channel (prepared_delta_channel head)
          (apply_prepared_rspace_delta prepared) =
        Some (prepared_delta_output head)
  | DeltaInvUntouchedChannelPreserved =>
      forall before (prepared : PreparedRSpaceDelta before) channel,
        ~ In channel
            (prepared_delta_channels (prepared_channel_deltas prepared)) ->
        lookup_channel channel (apply_prepared_rspace_delta prepared) =
        lookup_channel channel before
  | DeltaInvAuthorityCanonical =>
      forall before (prepared : PreparedRSpaceDelta before),
        canonical_channels before ->
        canonical_channels (apply_prepared_rspace_delta prepared)
  | DeltaInvFailureAtomic =>
      forall (Catalog ExternalSnapshots : Type)
        (before : @transactional_store_state Catalog ExternalSnapshots) outcome,
        rspace_delta_transition before None outcome ->
        rspace_delta_outcome_state outcome = before /\
        rspace_delta_publication_count outcome = 0%nat /\
        rspace_delta_outcome_decision outcome = RSpaceDeltaRefused
  | DeltaInvAcceptedOneRoot =>
      forall (Catalog ExternalSnapshots : Type)
        (before : @transactional_store_state Catalog ExternalSnapshots)
        prepared outcome,
        rspace_delta_transition before (Some prepared) outcome ->
        rspace_delta_outcome_state outcome =
            publish_prepared_rspace_delta before prepared /\
        rspace_delta_publication_count outcome = 1%nat /\
        rspace_delta_outcome_decision outcome = RSpaceDeltaPublished
  | DeltaInvAcceptedUntouchedChannel =>
      forall (Catalog ExternalSnapshots : Type)
        (before : @transactional_store_state Catalog ExternalSnapshots)
        prepared outcome channel,
        rspace_delta_transition before (Some prepared) outcome ->
        ~ In channel
            (prepared_delta_channels (prepared_channel_deltas prepared)) ->
        lookup_channel channel
            (store_authority (rspace_delta_outcome_state outcome)) =
        lookup_channel channel (store_authority before)
  | DeltaInvTouchedCatalogInvalidated =>
      forall (Catalog ExternalSnapshots : Type)
        (before : @transactional_store_state Catalog ExternalSnapshots)
        prepared outcome channel,
        rspace_delta_transition before (Some prepared) outcome ->
        In channel
          (prepared_delta_channels (prepared_channel_deltas prepared)) ->
        store_catalogs (rspace_delta_outcome_state outcome) channel = None
  | DeltaInvPublishedStoreWellFormed =>
      forall (Catalog ExternalSnapshots : Type)
        (before : @transactional_store_state Catalog ExternalSnapshots) prepared,
        store_state_well_formed before ->
        store_state_well_formed
          (publish_prepared_rspace_delta before prepared)
  | DeltaInvTransitionDeterministic =>
      forall (Catalog ExternalSnapshots : Type)
        (before : @transactional_store_state Catalog ExternalSnapshots)
        proposal left right,
        rspace_delta_transition before proposal left ->
        rspace_delta_transition before proposal right -> left = right
  end.

Theorem every_two_level_delta_invariant_is_bound : forall invariant,
  two_level_delta_invariant_binding invariant.
Proof.
  intro invariant. destruct invariant; cbn.
  - apply prepared_delta_output_is_canonical.
  - apply prepared_delta_rejects_an_absent_no_addition_channel.
  - apply prepared_delta_operation_channels_are_exact.
  - apply prepared_delta_channels_are_unique.
  - apply prepared_rspace_delta_head_output_is_exact.
  - apply prepared_rspace_delta_preserves_every_untouched_channel.
  - apply prepared_rspace_delta_preserves_canonicality.
  - intros Catalog ExternalSnapshots before outcome Htransition.
    exact (@refused_rspace_delta_preserves_every_public_observation
      Catalog ExternalSnapshots before outcome Htransition).
  - intros Catalog ExternalSnapshots before prepared outcome Htransition.
    exact (@accepted_rspace_delta_publishes_exactly_one_complete_root
      Catalog ExternalSnapshots before prepared outcome Htransition).
  - intros Catalog ExternalSnapshots before prepared outcome channel
      Htransition Hnot_touched.
    exact (@accepted_rspace_delta_preserves_every_untouched_channel
      Catalog ExternalSnapshots before prepared outcome channel
      Htransition Hnot_touched).
  - intros Catalog ExternalSnapshots before prepared outcome channel
      Htransition Htouched.
    exact (@accepted_rspace_delta_invalidates_every_touched_catalog
      Catalog ExternalSnapshots before prepared outcome channel
      Htransition Htouched).
  - intros Catalog ExternalSnapshots before prepared Hwell_formed.
    exact (@accepted_rspace_delta_preserves_store_well_formedness
      Catalog ExternalSnapshots before prepared Hwell_formed).
  - intros Catalog ExternalSnapshots before proposal left right Hleft Hright.
    exact (@rspace_delta_transition_is_deterministic
      Catalog ExternalSnapshots before proposal left right Hleft Hright).
Qed.

Theorem two_level_delta_invariant_catalog_is_unique_complete_and_exact :
  NoDup two_level_delta_invariant_catalog /\
  Forall two_level_delta_invariant_binding two_level_delta_invariant_catalog /\
  length two_level_delta_invariant_catalog = 13%nat.
Proof.
  split.
  - unfold two_level_delta_invariant_catalog.
    repeat match goal with
    | |- NoDup (_ :: _) => constructor; [simpl; intuition discriminate |]
    | |- NoDup [] => constructor
    end.
  - split.
    + apply Forall_forall. intros invariant _.
      apply every_two_level_delta_invariant_is_bound.
    + reflexivity.
Qed.

Print Assumptions prepared_delta_output_is_canonical.
Print Assumptions prepared_rspace_delta_head_output_is_exact.
Print Assumptions prepared_rspace_delta_preserves_every_untouched_channel.
Print Assumptions refused_rspace_delta_preserves_every_public_observation.
Print Assumptions accepted_rspace_delta_publishes_exactly_one_complete_root.
Print Assumptions accepted_rspace_delta_invalidates_every_touched_catalog.
Print Assumptions two_level_delta_invariant_catalog_is_unique_complete_and_exact.

End TwoLevelRSpaceDelta.
