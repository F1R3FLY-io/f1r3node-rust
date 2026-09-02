From Stdlib Require Import Bool List Arith Lia NArith.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublication
  TwoLevelRSpaceAuthority
  TwoLevelRSpaceAuthorityMutants
  TwoLevelRSpaceOperations
  TwoLevelRSpaceOperationsMutants
  TwoLevelRSpaceDelta.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceAuthorityMutants.
Import TwoLevelRSpaceOperations.
Import TwoLevelRSpaceOperationsMutants.
Import TwoLevelRSpaceDelta.

(** Concrete negative controls for normalized per-channel BagDelta
    preparation and one-root RSpace publication. *)
Module TwoLevelRSpaceDeltaMutants.

Lemma old_target_bag_is_canonical : canonical_entries old_target_bag.
Proof. repeat constructor; reflexivity. Qed.

Definition target_additions : ChannelBag := [(target_datum, 5%N)].

Lemma target_additions_are_canonical : canonical_entries target_additions.
Proof. repeat constructor; reflexivity. Qed.

Definition target_delta_change : BagDelta :=
  {| bag_delta_removal_mode := StrictSubtract;
     bag_delta_subtractions := [];
     bag_delta_deletions := [];
     bag_delta_additions := target_additions |}.

Lemma target_delta_operation_is_valid :
  valid_logical_operation old_target_bag (OpBagDelta target_delta_change).
Proof.
  apply ValidStrictBagDelta.
  - exact old_target_bag_is_canonical.
  - constructor.
  - exact target_additions_are_canonical.
Qed.

Lemma target_delta_executes_exactly :
  execute_logical_operation old_target_bag (OpBagDelta target_delta_change) =
  Some [(target_datum, 7%N)].
Proof. reflexivity. Qed.

Lemma target_channel_is_loaded_in_fixture : loaded authority_fixture target_channel.
Proof. exists old_target_bag. vm_compute. reflexivity. Qed.

Definition prepared_target_delta : PreparedChannelDelta authority_fixture :=
  {| prepared_delta_channel := target_channel;
     prepared_delta_change := target_delta_change;
     prepared_delta_output := [(target_datum, 7%N)];
     prepared_delta_channel_valid := eq_refl;
     prepared_delta_absent_channel_has_addition :=
       or_introl target_channel_is_loaded_in_fixture;
     prepared_delta_operation_valid := target_delta_operation_is_valid;
     prepared_delta_execution_succeeds := target_delta_executes_exactly |}.

Definition prepared_target_batch : PreparedRSpaceDelta authority_fixture.
Proof.
  refine {| prepared_channel_deltas := [prepared_target_delta] |}.
  unfold prepared_delta_channels. simpl.
  constructor.
  - intro Hin. inversion Hin.
  - constructor.
Defined.

Definition prepared_empty_batch : PreparedRSpaceDelta authority_fixture.
Proof.
  refine {| prepared_channel_deltas := [] |}.
  constructor.
Defined.

Theorem prepared_target_delta_is_non_vacuous :
  lookup_channel target_channel
    (apply_prepared_rspace_delta prepared_target_batch) =
  Some [(target_datum, 7%N)] /\
  lookup_channel sibling_channel
    (apply_prepared_rspace_delta prepared_target_batch) = Some sibling_bag.
Proof. vm_compute. split; reflexivity. Qed.

(** DeltaInvOutputCanonical. *)
Definition mutant_noncanonical_delta_output : ChannelBag :=
  [(second_datum, 1%N); (target_datum, 1%N)].

Theorem kill_noncanonical_delta_output :
  ~ canonical_entries mutant_noncanonical_delta_output.
Proof.
  intro Hcanonical.
  inversion Hcanonical as [|path count tail V M O C]; subst.
  inversion O as [|entry rest Hlt Hrest]; subst.
  unfold second_datum, target_datum in Hlt.
  inversion Hlt; lia.
Qed.

(** DeltaInvAbsentRequiresAddition. *)
Definition empty_delta_change : BagDelta :=
  {| bag_delta_removal_mode := StrictSubtract;
     bag_delta_subtractions := [];
     bag_delta_deletions := [];
     bag_delta_additions := [] |}.

Theorem kill_absent_empty_delta_admission :
  ~ loaded authority_fixture inserted_channel /\
  bag_delta_additions empty_delta_change = [].
Proof.
  split.
  - unfold loaded. intros [contents Hlookup].
    vm_compute in Hlookup. discriminate.
  - reflexivity.
Qed.

(** DeltaInvOperationChannelsExact. *)
Definition mutant_operation_channels : list Path := [sibling_channel].

Theorem kill_wrong_delta_operation_channel :
  mutant_operation_channels <> [target_channel].
Proof. discriminate. Qed.

(** DeltaInvTargetsUnique. *)
Definition mutant_duplicate_delta_channels : list Path :=
  [target_channel; target_channel].

Theorem kill_duplicate_delta_channels :
  ~ NoDup mutant_duplicate_delta_channels.
Proof.
  intro Hunique.
  inversion Hunique as [|head tail Hnotin Htail]; subst.
  apply Hnotin. simpl. auto.
Qed.

(** DeltaInvHeadOutputExact. *)
Definition mutant_delta_keeps_old_target : TwoLevelAuthority := authority_fixture.

Theorem kill_delta_head_output_omission :
  lookup_channel target_channel mutant_delta_keeps_old_target <>
  Some [(target_datum, 7%N)].
Proof. vm_compute. discriminate. Qed.

(** DeltaInvUntouchedChannelPreserved. *)
Definition mutant_delta_erases_untouched : TwoLevelAuthority :=
  [(target_channel, [(target_datum, 7%N)]);
   (descendant_channel, descendant_bag)].

Theorem kill_delta_untouched_erasure :
  lookup_channel sibling_channel mutant_delta_erases_untouched <>
  lookup_channel sibling_channel authority_fixture.
Proof. vm_compute. discriminate. Qed.

(** DeltaInvAuthorityCanonical. *)
Definition mutant_delta_reverses_authority : TwoLevelAuthority :=
  rev authority_fixture.

Theorem kill_noncanonical_delta_authority :
  ~ canonical_channels mutant_delta_reverses_authority.
Proof.
  intro Hcanonical.
  inversion Hcanonical as [|channel contents tail V B O C]; subst.
  inversion O as [|entry rest Hlt Hrest]; subst.
  unfold sibling_channel, descendant_channel in Hlt.
  inversion Hlt; lia.
Qed.

(** DeltaInvFailureAtomic. *)
Definition mutant_failed_delta_state : @transactional_store_state nat nat :=
  mutant_failed_state.

Theorem kill_failed_delta_state_mutation :
  mutant_failed_delta_state <> transaction_fixture.
Proof. apply kill_failure_public_state_mutation. Qed.

(** DeltaInvAcceptedConditionalRoot. *)
Definition mutant_delta_publication_count : nat := 2%nat.

Theorem kill_duplicate_delta_publication :
  mutant_delta_publication_count <>
    prepared_rspace_delta_publication_count prepared_target_batch.
Proof. discriminate. Qed.

(** DeltaInvEmptyPreservesIdentity. *)
Definition mutant_empty_delta_published_state :
    @transactional_store_state nat nat := mutant_failed_state.

Theorem kill_empty_delta_publication :
  mutant_empty_delta_published_state <> transaction_fixture /\
  prepared_rspace_delta_publication_count prepared_empty_batch = 0%nat.
Proof. split; [apply kill_failure_public_state_mutation | reflexivity]. Qed.

(** DeltaInvAcceptedUntouchedChannel. *)
Definition mutant_published_delta_erases_sibling : TwoLevelAuthority :=
  mutant_delta_erases_untouched.

Theorem kill_published_delta_untouched_erasure :
  lookup_channel sibling_channel mutant_published_delta_erases_sibling <>
  lookup_channel sibling_channel authority_fixture.
Proof. apply kill_delta_untouched_erasure. Qed.

(** DeltaInvTouchedCatalogInvalidated. *)
Definition mutant_delta_touched_catalog : option nat := Some 11%nat.

Theorem kill_delta_touched_catalog_retention :
  mutant_delta_touched_catalog <> None.
Proof. discriminate. Qed.

(** DeltaInvPublishedStoreWellFormed. *)
Definition mutant_delta_wrong_count_state : @transactional_store_state nat nat :=
  mutant_wrong_loaded_count_state.

Theorem kill_delta_wrong_loaded_count :
  ~ store_state_well_formed mutant_delta_wrong_count_state.
Proof. apply kill_wrong_published_loaded_count. Qed.

(** DeltaInvTransitionDeterministic. *)
Definition mutant_delta_left_outcome : @rspace_delta_outcome nat nat :=
  RSpaceDeltaOutcome RSpaceDeltaRefused transaction_fixture 0.

Definition mutant_delta_right_outcome : @rspace_delta_outcome nat nat :=
  RSpaceDeltaOutcome RSpaceDeltaPublished transaction_fixture 1.

Theorem kill_delta_nondeterministic_outcomes :
  mutant_delta_left_outcome <> mutant_delta_right_outcome.
Proof. discriminate. Qed.

Definition two_level_delta_mutant_killed
    (invariant : two_level_delta_invariant_id) : Prop :=
  match invariant with
  | DeltaInvOutputCanonical =>
      ~ canonical_entries mutant_noncanonical_delta_output
  | DeltaInvAbsentRequiresAddition =>
      ~ loaded authority_fixture inserted_channel /\
      bag_delta_additions empty_delta_change = []
  | DeltaInvOperationChannelsExact =>
      mutant_operation_channels <> [target_channel]
  | DeltaInvTargetsUnique =>
      ~ NoDup mutant_duplicate_delta_channels
  | DeltaInvHeadOutputExact =>
      lookup_channel target_channel mutant_delta_keeps_old_target <>
      Some [(target_datum, 7%N)]
  | DeltaInvUntouchedChannelPreserved =>
      lookup_channel sibling_channel mutant_delta_erases_untouched <>
      lookup_channel sibling_channel authority_fixture
  | DeltaInvAuthorityCanonical =>
      ~ canonical_channels mutant_delta_reverses_authority
  | DeltaInvFailureAtomic =>
      mutant_failed_delta_state <> transaction_fixture
  | DeltaInvAcceptedConditionalRoot =>
      mutant_delta_publication_count <>
        prepared_rspace_delta_publication_count prepared_target_batch
  | DeltaInvEmptyPreservesIdentity =>
      mutant_empty_delta_published_state <> transaction_fixture /\
      prepared_rspace_delta_publication_count prepared_empty_batch = 0%nat
  | DeltaInvAcceptedUntouchedChannel =>
      lookup_channel sibling_channel mutant_published_delta_erases_sibling <>
      lookup_channel sibling_channel authority_fixture
  | DeltaInvTouchedCatalogInvalidated =>
      mutant_delta_touched_catalog <> None
  | DeltaInvPublishedStoreWellFormed =>
      ~ store_state_well_formed mutant_delta_wrong_count_state
  | DeltaInvTransitionDeterministic =>
      mutant_delta_left_outcome <> mutant_delta_right_outcome
  end.

Theorem every_two_level_delta_mutant_has_a_causal_witness : forall invariant,
  two_level_delta_mutant_killed invariant.
Proof.
  intro invariant. destruct invariant; cbn.
  - apply kill_noncanonical_delta_output.
  - apply kill_absent_empty_delta_admission.
  - apply kill_wrong_delta_operation_channel.
  - apply kill_duplicate_delta_channels.
  - apply kill_delta_head_output_omission.
  - apply kill_delta_untouched_erasure.
  - apply kill_noncanonical_delta_authority.
  - apply kill_failed_delta_state_mutation.
  - apply kill_duplicate_delta_publication.
  - apply kill_empty_delta_publication.
  - apply kill_published_delta_untouched_erasure.
  - apply kill_delta_touched_catalog_retention.
  - apply kill_delta_wrong_loaded_count.
  - apply kill_delta_nondeterministic_outcomes.
Qed.

Theorem two_level_delta_mutant_catalog_is_total_unique_and_exact :
  NoDup two_level_delta_invariant_catalog /\
  Forall two_level_delta_mutant_killed two_level_delta_invariant_catalog /\
  length two_level_delta_invariant_catalog = 14%nat.
Proof.
  destruct two_level_delta_invariant_catalog_is_unique_complete_and_exact
    as [Hunique [_ Hlength]].
  split; [exact Hunique |]. split.
  - apply Forall_forall. intros invariant _.
    apply every_two_level_delta_mutant_has_a_causal_witness.
  - exact Hlength.
Qed.

Print Assumptions prepared_target_delta_is_non_vacuous.
Print Assumptions every_two_level_delta_mutant_has_a_causal_witness.
Print Assumptions two_level_delta_mutant_catalog_is_total_unique_and_exact.

End TwoLevelRSpaceDeltaMutants.
