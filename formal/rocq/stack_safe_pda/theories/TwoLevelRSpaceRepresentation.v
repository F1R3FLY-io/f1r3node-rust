From Stdlib Require Import Bool List Arith Lia NArith.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority
  TwoLevelRSpaceOperations.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceOperations.
Set Implicit Arguments.
Open Scope N_scope.

(** Public and internal value representations for the selected two-level
    RSpace layout.

    A public outer endpoint always stores a real independently guarded Bag.
    [SyntheticBarrier] is an internal-only value used by guarded terminal
    teardown; it is forbidden in every public root, observation, count,
    serialization, hash preimage, and publication. *)
Module TwoLevelRSpaceRepresentation.

Inductive ChannelSlot : Type :=
| RealChannelBag : ChannelBag -> ChannelSlot
| SyntheticBarrier : ChannelSlot.

Definition InternalChannelEntry : Type := (Path * ChannelSlot)%type.
Definition InternalAuthority : Type := list InternalChannelEntry.

Definition internal_channel_path (entry : InternalChannelEntry) : Path :=
  fst entry.

Definition internal_channel_slot (entry : InternalChannelEntry) : ChannelSlot :=
  snd entry.

Fixpoint project_public_authority
    (authority : InternalAuthority) : option TwoLevelAuthority :=
  match authority with
  | [] => Some []
  | (channel, RealChannelBag contents) :: tail =>
      match project_public_authority tail with
      | Some projected => Some ((channel, contents) :: projected)
      | None => None
      end
  | (_, SyntheticBarrier) :: _ => None
  end.

Fixpoint every_slot_is_real (authority : InternalAuthority) : Prop :=
  match authority with
  | [] => True
  | (_, RealChannelBag _) :: tail => every_slot_is_real tail
  | (_, SyntheticBarrier) :: _ => False
  end.

Fixpoint real_slot_count (authority : InternalAuthority) : nat :=
  match authority with
  | [] => 0
  | (_, RealChannelBag _) :: tail => S (real_slot_count tail)
  | (_, SyntheticBarrier) :: tail => real_slot_count tail
  end.

Fixpoint barrier_count (authority : InternalAuthority) : nat :=
  match authority with
  | [] => 0
  | (_, RealChannelBag _) :: tail => barrier_count tail
  | (_, SyntheticBarrier) :: tail => S (barrier_count tail)
  end.

Theorem public_projection_exists_iff_every_slot_is_real : forall authority,
  (exists public, project_public_authority authority = Some public) <->
  every_slot_is_real authority.
Proof.
  induction authority as [|[channel slot] tail IH]; simpl.
  - split.
    + intros. exact I.
    + intros. exists []. reflexivity.
  - destruct slot as [contents |].
    + split.
      * intros [public Hproject].
        destruct (project_public_authority tail) as [projected |] eqn:Htail;
          [|discriminate].
        apply IH. now exists projected.
      * intro Hreal.
        apply IH in Hreal as [projected Hprojected].
        exists ((channel, contents) :: projected).
        now rewrite Hprojected.
    + split; [intros [public H]; discriminate | contradiction].
Qed.

Theorem successful_public_projection_contains_no_barrier : forall authority public,
  project_public_authority authority = Some public ->
  barrier_count authority = 0%nat.
Proof.
  induction authority as [|[channel slot] tail IH];
    intros public Hproject; [reflexivity |].
  simpl in Hproject.
  destruct slot as [contents |]; [|discriminate].
  destruct (project_public_authority tail) as [projected |] eqn:Htail;
    [|discriminate].
  inversion Hproject; subst. simpl.
  exact (IH projected eq_refl).
Qed.

Theorem successful_public_projection_counts_exactly_real_endpoints : forall
    authority public,
  project_public_authority authority = Some public ->
  real_slot_count authority = length public.
Proof.
  induction authority as [|[channel slot] tail IH];
    intros public Hproject.
  - inversion Hproject. reflexivity.
  - simpl in Hproject.
    destruct slot as [contents |]; [|discriminate].
    destruct (project_public_authority tail) as [projected |] eqn:Htail;
      [|discriminate].
    inversion Hproject; subst. simpl. f_equal.
    exact (IH projected eq_refl).
Qed.

Fixpoint lookup_internal_slot
    (selected : Path) (authority : InternalAuthority) : option ChannelSlot :=
  match authority with
  | [] => None
  | (channel, slot) :: tail =>
      if path_eq_dec selected channel
      then Some slot
      else lookup_internal_slot selected tail
  end.

Definition lookup_public_bag
    (selected : Path) (authority : InternalAuthority) : option ChannelBag :=
  match lookup_internal_slot selected authority with
  | Some (RealChannelBag contents) => Some contents
  | _ => None
  end.

Theorem projected_lookup_refines_public_lookup : forall authority public channel,
  project_public_authority authority = Some public ->
  lookup_public_bag channel authority = lookup_channel channel public.
Proof.
  induction authority as [|[current slot] tail IH];
    intros public channel Hproject.
  - inversion Hproject. reflexivity.
  - simpl in Hproject.
    destruct slot as [contents |]; [|discriminate].
    destruct (project_public_authority tail) as [projected |] eqn:Htail;
      [|discriminate].
    inversion Hproject; subst.
    unfold lookup_public_bag. simpl.
    destruct (path_eq_dec channel current); [reflexivity |].
    exact (IH projected channel eq_refl).
Qed.

Theorem a_publicly_observable_endpoint_is_never_a_barrier : forall
    authority public channel contents,
  project_public_authority authority = Some public ->
  lookup_channel channel public = Some contents ->
  lookup_internal_slot channel authority = Some (RealChannelBag contents).
Proof.
  induction authority as [|[current slot] tail IH];
    intros public channel contents Hproject Hlookup.
  - inversion Hproject; subst. simpl in Hlookup. discriminate.
  - simpl in Hproject.
    destruct slot as [current_contents |]; [|discriminate].
    destruct (project_public_authority tail) as [projected |] eqn:Htail;
      [|discriminate].
    inversion Hproject; subst. simpl in Hlookup. simpl.
    destruct (path_eq_dec channel current) as [Heq | Hneq].
    + inversion Hlookup. subst. reflexivity.
    + exact (IH projected channel contents eq_refl Hlookup).
Qed.

(** Production numeric contract. Individual multiplicities are positive
    native [u64] values. Bag and store cardinalities accumulate exactly in
    [u128] and only the legacy public accessor saturates to [u64]. *)
Definition u64_max : N := multiplicity_max.
Definition u128_max : N := N.pred (N.pow 2 128).

Definition multiplicity_fits_u64 (value : Multiplicity) : Prop :=
  (0 < value <= u64_max)%N.

Definition exact_bag_cardinality : ChannelBag -> N := bag_cardinality.
Definition exact_store_cardinality : TwoLevelAuthority -> N := total_cardinality.

Definition exact_cardinality_fits_u128 (value : N) : Prop :=
  (value <= u128_max)%N.

Definition saturated_u64_cardinality (value : N) : N := N.min value u64_max.

Theorem saturated_cardinality_is_always_u64_bounded : forall value,
  (saturated_u64_cardinality value <= u64_max)%N.
Proof.
  intro value. unfold saturated_u64_cardinality. apply N.le_min_r.
Qed.

Theorem saturated_cardinality_equals_exact_when_exact_fits_u64 : forall value,
  (value <= u64_max)%N -> saturated_u64_cardinality value = value.
Proof.
  intros value Hfits. unfold saturated_u64_cardinality.
  now apply N.min_l.
Qed.

Theorem saturated_cardinality_is_u64_max_above_u64 : forall value,
  (u64_max <= value)%N -> saturated_u64_cardinality value = u64_max.
Proof.
  intros value Habove. unfold saturated_u64_cardinality.
  now apply N.min_r.
Qed.

Lemma canonical_entry_multiplicity_fits_u64 : forall contents path multiplicity,
  canonical_entries contents ->
  In (path, multiplicity) contents ->
  multiplicity_fits_u64 multiplicity.
Proof.
  intros contents path multiplicity Hcanonical.
  induction Hcanonical as
      [|current current_multiplicity tail Hpath Hmultiplicity Hafter Htail IH];
    intro Hin; [contradiction |].
  destruct Hin as [Heq | Hin].
  - inversion Heq; subst.
    unfold multiplicity_fits_u64, multiplicity_valid in Hmultiplicity.
    apply andb_true_iff in Hmultiplicity as [Hpositive Hbounded].
    apply N.ltb_lt in Hpositive. apply N.leb_le in Hbounded.
    split; assumption.
  - now apply IH.
Qed.

Definition public_authority_well_formed
    (authority : TwoLevelAuthority) (loaded_count : nat) : Prop :=
  canonical_channels authority /\
  loaded_count = length authority /\
  exact_cardinality_fits_u128 (exact_store_cardinality authority).

Definition internal_publication_well_formed
    (authority : InternalAuthority) (loaded_count : nat) : Prop :=
  exists public,
    project_public_authority authority = Some public /\
    public_authority_well_formed public loaded_count.

Theorem a_well_formed_internal_publication_has_no_barrier : forall
    authority loaded_count,
  internal_publication_well_formed authority loaded_count ->
  barrier_count authority = 0%nat.
Proof.
  intros authority loaded_count [public [Hproject Hwell_formed]].
  now apply successful_public_projection_contains_no_barrier with public.
Qed.

Theorem a_well_formed_internal_publication_counts_outer_endpoints_exactly : forall
    authority loaded_count,
  internal_publication_well_formed authority loaded_count ->
  real_slot_count authority = loaded_count.
Proof.
  intros authority loaded_count [public [Hproject [_ [Hcount _]]]].
  pose proof
    (@successful_public_projection_counts_exactly_real_endpoints
      authority public Hproject) as Hreal_count.
  rewrite Hreal_count.
  symmetry. exact Hcount.
Qed.

Definition serializable_public_authority
    (authority : InternalAuthority) : option TwoLevelAuthority :=
  project_public_authority authority.

Theorem a_barrier_bearing_authority_is_not_serializable : forall authority,
  (0 < barrier_count authority)%nat ->
  serializable_public_authority authority = None.
Proof.
  intros authority Hbarrier.
  unfold serializable_public_authority.
  destruct (project_public_authority authority) as [public |] eqn:Hproject;
    [|reflexivity].
  pose proof
    (@successful_public_projection_contains_no_barrier
      authority public Hproject).
  lia.
Qed.

Theorem exact_outer_lookup_separates_prefix_related_channels : forall
    authority selected replacement other,
  other <> selected ->
  (exists suffix, other = selected ++ suffix) \/
  (exists suffix, selected = other ++ suffix) ->
  lookup_channel other (upsert_channel_exact selected replacement authority) =
  lookup_channel other authority.
Proof.
  intros. now apply upsert_preserves_every_other_exact_channel.
Qed.

Inductive two_level_representation_invariant_id : Type :=
| ReprInvPublicIffAllReal
| ReprInvNoPublicBarrier
| ReprInvRealEndpointCount
| ReprInvProjectedLookup
| ReprInvObservableEndpointReal
| ReprInvMultiplicityU64
| ReprInvSaturatedBound
| ReprInvSaturatedExactBelowU64
| ReprInvSaturatedMaxAboveU64
| ReprInvWellFormedNoBarrier
| ReprInvWellFormedLoadedCount
| ReprInvBarrierNotSerializable
| ReprInvPrefixRelatedExactLookup.

Definition two_level_representation_invariant_catalog :
    list two_level_representation_invariant_id :=
  [ ReprInvPublicIffAllReal;
    ReprInvNoPublicBarrier;
    ReprInvRealEndpointCount;
    ReprInvProjectedLookup;
    ReprInvObservableEndpointReal;
    ReprInvMultiplicityU64;
    ReprInvSaturatedBound;
    ReprInvSaturatedExactBelowU64;
    ReprInvSaturatedMaxAboveU64;
    ReprInvWellFormedNoBarrier;
    ReprInvWellFormedLoadedCount;
    ReprInvBarrierNotSerializable;
    ReprInvPrefixRelatedExactLookup ].

Definition two_level_representation_invariant_binding
    (invariant : two_level_representation_invariant_id) : Prop :=
  match invariant with
  | ReprInvPublicIffAllReal =>
      forall authority,
        (exists public, project_public_authority authority = Some public) <->
        every_slot_is_real authority
  | ReprInvNoPublicBarrier =>
      forall authority public,
        project_public_authority authority = Some public ->
        barrier_count authority = 0%nat
  | ReprInvRealEndpointCount =>
      forall authority public,
        project_public_authority authority = Some public ->
        real_slot_count authority = length public
  | ReprInvProjectedLookup =>
      forall authority public channel,
        project_public_authority authority = Some public ->
        lookup_public_bag channel authority = lookup_channel channel public
  | ReprInvObservableEndpointReal =>
      forall authority public channel contents,
        project_public_authority authority = Some public ->
        lookup_channel channel public = Some contents ->
        lookup_internal_slot channel authority = Some (RealChannelBag contents)
  | ReprInvMultiplicityU64 =>
      forall contents path multiplicity,
        canonical_entries contents ->
        In (path, multiplicity) contents ->
        multiplicity_fits_u64 multiplicity
  | ReprInvSaturatedBound =>
      forall value, (saturated_u64_cardinality value <= u64_max)%N
  | ReprInvSaturatedExactBelowU64 =>
      forall value, (value <= u64_max)%N ->
        saturated_u64_cardinality value = value
  | ReprInvSaturatedMaxAboveU64 =>
      forall value, (u64_max <= value)%N ->
        saturated_u64_cardinality value = u64_max
  | ReprInvWellFormedNoBarrier =>
      forall authority loaded_count,
        internal_publication_well_formed authority loaded_count ->
        barrier_count authority = 0%nat
  | ReprInvWellFormedLoadedCount =>
      forall authority loaded_count,
        internal_publication_well_formed authority loaded_count ->
        real_slot_count authority = loaded_count
  | ReprInvBarrierNotSerializable =>
      forall authority, (0 < barrier_count authority)%nat ->
        serializable_public_authority authority = None
  | ReprInvPrefixRelatedExactLookup =>
      forall authority selected replacement other,
        other <> selected ->
        (exists suffix, other = selected ++ suffix) \/
        (exists suffix, selected = other ++ suffix) ->
        lookup_channel other
          (upsert_channel_exact selected replacement authority) =
        lookup_channel other authority
  end.

Theorem every_two_level_representation_invariant_is_bound : forall invariant,
  two_level_representation_invariant_binding invariant.
Proof.
  intro invariant. destruct invariant; cbn.
  - apply public_projection_exists_iff_every_slot_is_real.
  - apply successful_public_projection_contains_no_barrier.
  - apply successful_public_projection_counts_exactly_real_endpoints.
  - apply projected_lookup_refines_public_lookup.
  - apply a_publicly_observable_endpoint_is_never_a_barrier.
  - apply canonical_entry_multiplicity_fits_u64.
  - apply saturated_cardinality_is_always_u64_bounded.
  - apply saturated_cardinality_equals_exact_when_exact_fits_u64.
  - apply saturated_cardinality_is_u64_max_above_u64.
  - apply a_well_formed_internal_publication_has_no_barrier.
  - apply a_well_formed_internal_publication_counts_outer_endpoints_exactly.
  - apply a_barrier_bearing_authority_is_not_serializable.
  - apply exact_outer_lookup_separates_prefix_related_channels.
Qed.

Theorem two_level_representation_invariant_catalog_is_unique_complete_and_exact :
  NoDup two_level_representation_invariant_catalog /\
  Forall two_level_representation_invariant_binding
    two_level_representation_invariant_catalog /\
  length two_level_representation_invariant_catalog = 13%nat.
Proof.
  split.
  - unfold two_level_representation_invariant_catalog.
    repeat match goal with
    | |- NoDup (_ :: _) => constructor; [simpl; intuition discriminate |]
    | |- NoDup [] => constructor
    end.
  - split.
    + apply Forall_forall. intros invariant _.
      apply every_two_level_representation_invariant_is_bound.
    + reflexivity.
Qed.

Print Assumptions public_projection_exists_iff_every_slot_is_real.
Print Assumptions projected_lookup_refines_public_lookup.
Print Assumptions canonical_entry_multiplicity_fits_u64.
Print Assumptions a_barrier_bearing_authority_is_not_serializable.
Print Assumptions two_level_representation_invariant_catalog_is_unique_complete_and_exact.

End TwoLevelRSpaceRepresentation.
