From Stdlib Require Import Arith.PeanoNat Lists.List Lia.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

Section LocatedAuthoritySettlement.

Context {signature location purse event : Type}.
Context (purse_eq_dec : forall left right : purse, {left = right} + {left <> right}).

Record region := {
  region_signature : signature;
  region_location : location
}.

Definition purse_balance := purse -> nat.
Definition region_purse_plan := region -> purse.

Definition purse_demand
  (regions : list region)
  (plan : region_purse_plan)
  (selected : purse)
  : nat :=
  fold_right
    (fun current total =>
       if purse_eq_dec (plan current) selected then S total else total)
    0
    regions.

Definition can_debit
  (balance : purse_balance)
  (regions : list region)
  (plan : region_purse_plan)
  : Prop :=
  forall selected, purse_demand regions plan selected <= balance selected.

Definition debit
  (balance : purse_balance)
  (regions : list region)
  (plan : region_purse_plan)
  : purse_balance :=
  fun selected => balance selected - purse_demand regions plan selected.

Definition reserve_or_reject
  (balance : purse_balance)
  (regions : list region)
  (plan : region_purse_plan)
  (decision : {can_debit balance regions plan} + {~ can_debit balance regions plan})
  : purse_balance * bool :=
  if decision then (debit balance regions plan, true) else (balance, false).

Record located_plan := {
  located_regions : list region;
  located_purses : region_purse_plan;
  located_near : region -> purse -> Prop
}.

Definition plan_is_located (plan : located_plan) : Prop :=
  forall current,
    In current (located_regions plan) ->
    located_near plan current (located_purses plan current).

Definition atom_multiset (regions : list region) : list signature :=
  map region_signature regions.

Definition plans_preserve_authority
  (left right : located_plan)
  : Prop :=
  Permutation
    (atom_multiset (located_regions left))
    (atom_multiset (located_regions right)).

Definition replay_refines
  (play replay : located_plan)
  : Prop :=
  located_regions play = located_regions replay /\
  forall current,
    In current (located_regions play) ->
    located_purses play current = located_purses replay current.

Definition dependencies_committed
  (dependencies committed : list event)
  : Prop :=
  forall dependency, In dependency dependencies -> In dependency committed.

Definition event_debit
  (balance : purse_balance)
  (plan : located_plan)
  : purse_balance :=
  debit balance (located_regions plan) (located_purses plan).

Definition settlement_conserves
  (initial final : purse_balance)
  (regions : list region)
  (plan : region_purse_plan)
  : Prop :=
  forall selected,
    final selected + purse_demand regions plan selected = initial selected.

Theorem rejected_event_is_state_atomic : forall balance regions plan decision,
  ~ can_debit balance regions plan ->
  reserve_or_reject balance regions plan decision = (balance, false).
Proof.
  intros balance regions plan decision rejected.
  unfold reserve_or_reject.
  destruct decision as [funded | underfunded].
  - contradiction.
  - reflexivity.
Qed.

Theorem admitted_event_debits_exactly : forall balance regions plan decision,
  can_debit balance regions plan ->
  reserve_or_reject balance regions plan decision =
    (debit balance regions plan, true).
Proof.
  intros balance regions plan decision funded.
  unfold reserve_or_reject.
  destruct decision as [accepted | rejected].
  - reflexivity.
  - contradiction.
Qed.

Theorem debit_preserves_unselected_purse : forall balance regions plan selected,
  purse_demand regions plan selected = 0 ->
  debit balance regions plan selected = balance selected.
Proof.
  intros balance regions plan selected absent.
  unfold debit.
  rewrite absent, Nat.sub_0_r.
  reflexivity.
Qed.

Theorem debit_conserves_each_purse : forall balance regions plan selected,
  purse_demand regions plan selected <= balance selected ->
  debit balance regions plan selected
    + purse_demand regions plan selected
    = balance selected.
Proof.
  intros balance regions plan selected sufficient.
  unfold debit.
  lia.
Qed.

Theorem admitted_settlement_conserves : forall balance regions plan,
  can_debit balance regions plan ->
  settlement_conserves balance (debit balance regions plan) regions plan.
Proof.
  intros balance regions plan funded selected.
  apply debit_conserves_each_purse.
  apply funded.
Qed.

Theorem plan_permutation_preserves_authority : forall left right,
  Permutation (located_regions left) (located_regions right) ->
  plans_preserve_authority left right.
Proof.
  intros left right permutation.
  unfold plans_preserve_authority, atom_multiset.
  now apply Permutation_map.
Qed.

Theorem replay_preserves_authority : forall play replay,
  replay_refines play replay ->
  plans_preserve_authority play replay.
Proof.
  intros play replay [same_regions same_purses].
  unfold plans_preserve_authority, atom_multiset.
  now rewrite same_regions.
Qed.

Lemma purse_demand_plan_ext : forall regions left right selected,
  (forall current, In current regions -> left current = right current) ->
  purse_demand regions left selected = purse_demand regions right selected.
Proof.
  intros regions left right selected extension.
  induction regions as [| current rest IH].
  - reflexivity.
  - simpl.
    rewrite (extension current (or_introl eq_refl)).
    destruct (purse_eq_dec (right current) selected).
    + f_equal.
      apply IH.
      intros nested nested_in.
      apply extension.
      now right.
    + apply IH.
      intros nested nested_in.
      apply extension.
      now right.
Qed.

Theorem replay_preserves_purse_debit : forall balance play replay,
  replay_refines play replay ->
  forall selected,
    event_debit balance play selected =
    event_debit balance replay selected.
Proof.
  intros balance play replay [same_regions same_purses] selected.
  unfold event_debit, debit.
  rewrite same_regions.
  f_equal.
  apply purse_demand_plan_ext.
  intros current current_in.
  apply same_purses.
  now rewrite same_regions.
Qed.

Theorem continuation_requires_outer_event : forall outer continuation committed,
  dependencies_committed [outer] committed ->
  In continuation committed ->
  In outer committed.
Proof.
  intros outer continuation committed dependencies committed_continuation.
  apply dependencies.
  now left.
Qed.

Theorem cross_deploy_slot_identity_is_replay_stable :
  forall play replay first_region later_region shared_purse,
    replay_refines play replay ->
    In first_region (located_regions play) ->
    In later_region (located_regions play) ->
    located_purses play first_region = shared_purse ->
    located_purses play later_region = shared_purse ->
    located_purses replay first_region = shared_purse /\
    located_purses replay later_region = shared_purse.
Proof.
  intros play replay first_region later_region shared_purse
    [same_regions same_purses] first_in later_in first_slot later_slot.
  split.
  - rewrite <- (same_purses first_region first_in).
    exact first_slot.
  - rewrite <- (same_purses later_region later_in).
    exact later_slot.
Qed.

Theorem atomic_join_debits_all_or_none :
  forall balance plan decision,
    reserve_or_reject
      balance
      (located_regions plan)
      (located_purses plan)
      decision
      = (balance, false)
    \/
    reserve_or_reject
      balance
      (located_regions plan)
      (located_purses plan)
      decision
      = (event_debit balance plan, true).
Proof.
  intros balance plan decision.
  unfold reserve_or_reject, event_debit.
  destruct decision.
  - now right.
  - now left.
Qed.

End LocatedAuthoritySettlement.
