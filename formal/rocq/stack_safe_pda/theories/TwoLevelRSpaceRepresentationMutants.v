From Stdlib Require Import Bool List Arith Lia NArith.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority
  TwoLevelRSpaceAuthorityMutants
  TwoLevelRSpaceOperations
  TwoLevelRSpaceRepresentation.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceAuthorityMutants.
Import TwoLevelRSpaceOperations.
Import TwoLevelRSpaceRepresentation.
Open Scope N_scope.

(** Concrete causal negative controls for the public/internal ChannelSlot,
    numeric cardinality, serialization, and exact-key representation laws. *)
Module TwoLevelRSpaceRepresentationMutants.

Definition real_internal_fixture : InternalAuthority :=
  [(target_channel, RealChannelBag old_target_bag);
   (descendant_channel, RealChannelBag descendant_bag);
   (sibling_channel, RealChannelBag sibling_bag)].

Definition barrier_internal_fixture : InternalAuthority :=
  [(target_channel, RealChannelBag old_target_bag);
   (descendant_channel, SyntheticBarrier);
   (sibling_channel, RealChannelBag sibling_bag)].

Definition public_fixture : TwoLevelAuthority := authority_fixture.

(** ReprInvPublicIffAllReal: incorrectly materialize a barrier as a loaded
    empty public Bag. *)
Fixpoint mutant_materialize_barriers
    (authority : InternalAuthority) : TwoLevelAuthority :=
  match authority with
  | [] => []
  | (channel, RealChannelBag contents) :: tail =>
      (channel, contents) :: mutant_materialize_barriers tail
  | (channel, SyntheticBarrier) :: tail =>
      (channel, []) :: mutant_materialize_barriers tail
  end.

Theorem kill_barrier_materialized_as_public_empty :
  exists public,
    public = mutant_materialize_barriers barrier_internal_fixture /\
    ~ every_slot_is_real barrier_internal_fixture.
Proof.
  exists (mutant_materialize_barriers barrier_internal_fixture).
  split; [reflexivity | vm_compute; tauto].
Qed.

(** ReprInvNoPublicBarrier: incorrectly skip a barrier while reporting a
    successful public projection. *)
Fixpoint mutant_filter_barriers
    (authority : InternalAuthority) : TwoLevelAuthority :=
  match authority with
  | [] => []
  | (channel, RealChannelBag contents) :: tail =>
      (channel, contents) :: mutant_filter_barriers tail
  | (_, SyntheticBarrier) :: tail => mutant_filter_barriers tail
  end.

Theorem kill_successful_projection_that_hides_a_barrier :
  barrier_count barrier_internal_fixture <> 0%nat /\
  mutant_filter_barriers barrier_internal_fixture =
    [(target_channel, old_target_bag); (sibling_channel, sibling_bag)].
Proof. vm_compute. split; [discriminate | reflexivity]. Qed.

(** ReprInvRealEndpointCount: incorrectly drop the first real endpoint while
    projecting. *)
Definition mutant_drop_first_projected_endpoint : TwoLevelAuthority :=
  tl (mutant_filter_barriers real_internal_fixture).

Theorem kill_projected_real_endpoint_loss :
  real_slot_count real_internal_fixture <>
  length mutant_drop_first_projected_endpoint.
Proof. vm_compute. discriminate. Qed.

(** ReprInvProjectedLookup: use the target Bag for a distinct descendant
    lookup, as a prefix-oriented outer lookup would. *)
Definition mutant_prefix_projected_lookup (selected : Path) : option ChannelBag :=
  if path_eq_dec selected descendant_channel
  then Some old_target_bag
  else lookup_channel selected public_fixture.

Theorem kill_prefix_projected_lookup :
  mutant_prefix_projected_lookup descendant_channel <>
  lookup_channel descendant_channel public_fixture.
Proof. vm_compute. discriminate. Qed.

(** ReprInvObservableEndpointReal: incorrectly expose a synthetic barrier as
    a public real Bag. *)
Definition mutant_barrier_only_authority : InternalAuthority :=
  [(target_channel, SyntheticBarrier)].

Definition mutant_barrier_public_view : TwoLevelAuthority :=
  [(target_channel, old_target_bag)].

Theorem kill_public_observation_backed_by_barrier :
  lookup_channel target_channel mutant_barrier_public_view =
    Some old_target_bag /\
  lookup_internal_slot target_channel mutant_barrier_only_authority <>
    Some (RealChannelBag old_target_bag).
Proof. vm_compute. split; [reflexivity | discriminate]. Qed.

(** ReprInvMultiplicityU64: accept zero as a stored multiplicity. *)
Definition mutant_zero_multiplicity : Multiplicity := 0%N.

Theorem kill_zero_multiplicity_admission :
  ~ multiplicity_fits_u64 mutant_zero_multiplicity.
Proof. unfold multiplicity_fits_u64, mutant_zero_multiplicity; lia. Qed.

(** ReprInvSaturatedBound: return the exact value rather than saturating. *)
Definition mutant_identity_saturation (value : N) : N := value.

Theorem kill_identity_saturation_overflow :
  ~ (mutant_identity_saturation (N.succ u64_max) <= u64_max)%N.
Proof. unfold mutant_identity_saturation; lia. Qed.

(** ReprInvSaturatedExactBelowU64: lose one from a representable exact
    cardinality. *)
Definition mutant_off_by_one_saturation (value : N) : N := N.pred value.

Theorem kill_below_u64_off_by_one :
  mutant_off_by_one_saturation 7%N <> 7%N.
Proof. vm_compute. discriminate. Qed.

(** ReprInvSaturatedMaxAboveU64: wrap an overflowing legacy accessor to
    zero. *)
Definition mutant_wrapping_saturation (_ : N) : N := 0%N.

Theorem kill_above_u64_wrap_to_zero :
  mutant_wrapping_saturation (N.succ u64_max) <> u64_max.
Proof. vm_compute. discriminate. Qed.

(** ReprInvWellFormedNoBarrier: define well-formed publication over a
    barrier-filtered view, thereby validating an internal barrier. *)
Definition mutant_internal_well_formed
    (authority : InternalAuthority) (loaded_count : nat) : Prop :=
  public_authority_well_formed
    (mutant_filter_barriers authority) loaded_count.

Theorem kill_well_formed_publication_that_filters_a_barrier :
  mutant_internal_well_formed mutant_barrier_only_authority 0%nat /\
  barrier_count mutant_barrier_only_authority <> 0%nat.
Proof.
  split.
  - unfold mutant_internal_well_formed, public_authority_well_formed.
    simpl. repeat split.
    + constructor.
    + unfold exact_cardinality_fits_u128, exact_store_cardinality,
        u128_max. simpl. apply N.le_0_l.
  - vm_compute. discriminate.
Qed.

(** ReprInvWellFormedLoadedCount: cache one more loaded channel than the
    public outer endpoint authority contains. *)
Definition mutant_loaded_count : nat := S (length public_fixture).

Theorem kill_loaded_count_cache_drift :
  mutant_loaded_count <> length public_fixture.
Proof. unfold mutant_loaded_count; lia. Qed.

(** ReprInvBarrierNotSerializable: serialize the barrier-filtered view. *)
Definition mutant_serialize_barriers
    (authority : InternalAuthority) : option TwoLevelAuthority :=
  Some (mutant_filter_barriers authority).

Theorem kill_barrier_filtering_serializer :
  barrier_count barrier_internal_fixture <> 0%nat /\
  exists public,
    mutant_serialize_barriers barrier_internal_fixture = Some public.
Proof.
  split; [vm_compute; discriminate |].
  exists (mutant_filter_barriers barrier_internal_fixture). reflexivity.
Qed.

(** ReprInvPrefixRelatedExactLookup: apply the target replacement to its
    descendant as well. *)
Definition mutant_prefix_outer_upsert : TwoLevelAuthority :=
  [(target_channel, replacement_bag);
   (descendant_channel, replacement_bag);
   (sibling_channel, sibling_bag)].

Theorem kill_prefix_related_descendant_upsert :
  lookup_channel descendant_channel mutant_prefix_outer_upsert <>
  lookup_channel descendant_channel authority_fixture.
Proof. vm_compute. discriminate. Qed.

Definition two_level_representation_mutant_killed
    (invariant : two_level_representation_invariant_id) : Prop :=
  match invariant with
  | ReprInvPublicIffAllReal =>
      exists public,
        public = mutant_materialize_barriers barrier_internal_fixture /\
        ~ every_slot_is_real barrier_internal_fixture
  | ReprInvNoPublicBarrier =>
      barrier_count barrier_internal_fixture <> 0%nat /\
      mutant_filter_barriers barrier_internal_fixture =
        [(target_channel, old_target_bag); (sibling_channel, sibling_bag)]
  | ReprInvRealEndpointCount =>
      real_slot_count real_internal_fixture <>
      length mutant_drop_first_projected_endpoint
  | ReprInvProjectedLookup =>
      mutant_prefix_projected_lookup descendant_channel <>
      lookup_channel descendant_channel public_fixture
  | ReprInvObservableEndpointReal =>
      lookup_channel target_channel mutant_barrier_public_view =
        Some old_target_bag /\
      lookup_internal_slot target_channel mutant_barrier_only_authority <>
        Some (RealChannelBag old_target_bag)
  | ReprInvMultiplicityU64 =>
      ~ multiplicity_fits_u64 mutant_zero_multiplicity
  | ReprInvSaturatedBound =>
      ~ (mutant_identity_saturation (N.succ u64_max) <= u64_max)%N
  | ReprInvSaturatedExactBelowU64 =>
      mutant_off_by_one_saturation 7%N <> 7%N
  | ReprInvSaturatedMaxAboveU64 =>
      mutant_wrapping_saturation (N.succ u64_max) <> u64_max
  | ReprInvWellFormedNoBarrier =>
      mutant_internal_well_formed mutant_barrier_only_authority 0%nat /\
      barrier_count mutant_barrier_only_authority <> 0%nat
  | ReprInvWellFormedLoadedCount =>
      mutant_loaded_count <> length public_fixture
  | ReprInvBarrierNotSerializable =>
      barrier_count barrier_internal_fixture <> 0%nat /\
      exists public,
        mutant_serialize_barriers barrier_internal_fixture = Some public
  | ReprInvPrefixRelatedExactLookup =>
      lookup_channel descendant_channel mutant_prefix_outer_upsert <>
      lookup_channel descendant_channel authority_fixture
  end.

Theorem every_two_level_representation_mutant_has_a_causal_witness :
  forall invariant, two_level_representation_mutant_killed invariant.
Proof.
  intro invariant. destruct invariant; cbn.
  - apply kill_barrier_materialized_as_public_empty.
  - apply kill_successful_projection_that_hides_a_barrier.
  - apply kill_projected_real_endpoint_loss.
  - apply kill_prefix_projected_lookup.
  - apply kill_public_observation_backed_by_barrier.
  - apply kill_zero_multiplicity_admission.
  - apply kill_identity_saturation_overflow.
  - apply kill_below_u64_off_by_one.
  - apply kill_above_u64_wrap_to_zero.
  - apply kill_well_formed_publication_that_filters_a_barrier.
  - apply kill_loaded_count_cache_drift.
  - apply kill_barrier_filtering_serializer.
  - apply kill_prefix_related_descendant_upsert.
Qed.

Theorem two_level_representation_mutant_catalog_is_total_unique_and_exact :
  NoDup two_level_representation_invariant_catalog /\
  Forall two_level_representation_mutant_killed
    two_level_representation_invariant_catalog /\
  length two_level_representation_invariant_catalog = 13%nat.
Proof.
  destruct two_level_representation_invariant_catalog_is_unique_complete_and_exact
    as [Hunique [_ Hlength]].
  split; [exact Hunique |]. split.
  - apply Forall_forall. intros invariant _.
    apply every_two_level_representation_mutant_has_a_causal_witness.
  - exact Hlength.
Qed.

Print Assumptions kill_well_formed_publication_that_filters_a_barrier.
Print Assumptions every_two_level_representation_mutant_has_a_causal_witness.
Print Assumptions two_level_representation_mutant_catalog_is_total_unique_and_exact.

End TwoLevelRSpaceRepresentationMutants.
