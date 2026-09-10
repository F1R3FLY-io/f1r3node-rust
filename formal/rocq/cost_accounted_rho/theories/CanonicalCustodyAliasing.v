From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

Section CanonicalCustodyAliasing.

Context {lane purse : Type}.
Context (purse_eq_dec : forall left right : purse, {left = right} + {left <> right}).

Definition lane_debit := lane -> nat.
Definition purse_balance := purse -> nat.
Definition custody_plan := lane -> purse.

Definition physical_draw
  (lanes : list lane)
  (purse_of : custody_plan)
  (draw : lane_debit)
  (selected : purse)
  : nat :=
  fold_right
    (fun current total =>
       if purse_eq_dec (purse_of current) selected
       then draw current + total
       else total)
    0
    lanes.

Definition physically_admissible
  (lanes : list lane)
  (purse_of : custody_plan)
  (balance : purse_balance)
  (draw : lane_debit)
  : Prop :=
  forall selected, physical_draw lanes purse_of draw selected <= balance selected.

Definition settled_balance
  (lanes : list lane)
  (purse_of : custody_plan)
  (balance : purse_balance)
  (draw : lane_debit)
  : purse_balance :=
  fun selected => balance selected - physical_draw lanes purse_of draw selected.

Definition physical_conservation
  (lanes : list lane)
  (purse_of : custody_plan)
  (initial final : purse_balance)
  (draw : lane_debit)
  : Prop :=
  forall selected,
    final selected + physical_draw lanes purse_of draw selected = initial selected.

Definition per_lane_admissible
  (lanes : list lane)
  (purse_of : custody_plan)
  (balance : purse_balance)
  (draw : lane_debit)
  : Prop :=
  forall current,
    In current lanes -> draw current <= balance (purse_of current).

Theorem physically_admissible_prevents_double_capacity :
  forall lanes purse_of balance draw selected,
    physically_admissible lanes purse_of balance draw ->
    physical_draw lanes purse_of draw selected <= balance selected.
Proof.
  intros lanes purse_of balance draw selected admissible.
  apply admissible.
Qed.

Theorem physical_settlement_conserves :
  forall lanes purse_of balance draw,
    physically_admissible lanes purse_of balance draw ->
    physical_conservation
      lanes
      purse_of
      balance
      (settled_balance lanes purse_of balance draw)
      draw.
Proof.
  intros lanes purse_of balance draw admissible selected.
  unfold settled_balance.
  specialize (admissible selected).
  lia.
Qed.

Theorem physical_draw_is_permutation_invariant :
  forall left right purse_of draw selected,
    Permutation left right ->
    physical_draw left purse_of draw selected =
    physical_draw right purse_of draw selected.
Proof.
  intros left right purse_of draw selected permutation.
  induction permutation.
  - reflexivity.
  - simpl.
    destruct (purse_eq_dec (purse_of x) selected); now rewrite IHpermutation.
  - simpl.
    destruct (purse_eq_dec (purse_of x) selected);
      destruct (purse_eq_dec (purse_of y) selected); lia.
  - now rewrite IHpermutation1, IHpermutation2.
Qed.

Definition stack_admissible
  (available draw : lane_debit)
  : Prop :=
  forall current, draw current <= available current.

Definition settled_stack
  (available draw : lane_debit)
  : lane_debit :=
  fun current => available current - draw current.

Definition stack_conservation
  (initial final draw : lane_debit)
  : Prop :=
  forall current, final current + draw current = initial current.

Theorem stack_settlement_preserves_authority :
  forall available draw,
    stack_admissible available draw ->
    stack_conservation available (settled_stack available draw) draw.
Proof.
  intros available draw admissible current.
  unfold settled_stack.
  specialize (admissible current).
  lia.
Qed.

Theorem physical_alias_does_not_merge_stack_authority :
  forall available draw left right (purse_of : custody_plan),
    left <> right ->
    purse_of left = purse_of right ->
    draw right = 0 ->
    settled_stack available draw right = available right.
Proof.
  intros available draw left right purse_of distinct aliased absent.
  unfold settled_stack.
  now rewrite absent, Nat.sub_0_r.
Qed.

End CanonicalCustodyAliasing.

Definition alias_purse (_ : bool) : unit := tt.
Definition alias_balance (_ : unit) : nat := 1.
Definition alias_draw (_ : bool) : nat := 1.

Definition unit_eq_dec (left right : unit) : {left = right} + {left <> right}.
Proof.
  decide equality.
Defined.

Example per_lane_capacity_can_double_count_one_physical_purse :
  per_lane_admissible [true; false] alias_purse alias_balance alias_draw /\
  ~ physically_admissible unit_eq_dec [true; false] alias_purse alias_balance alias_draw.
Proof.
  split.
  - intros current present.
    destruct current; reflexivity.
  - intro admissible.
    specialize (admissible tt).
    cbv [physical_draw alias_purse alias_balance alias_draw unit_eq_dec] in admissible.
    exact (Nat.nle_succ_diag_l 1 admissible).
Qed.

Print Assumptions physically_admissible_prevents_double_capacity.
Print Assumptions physical_settlement_conserves.
Print Assumptions physical_draw_is_permutation_invariant.
Print Assumptions stack_settlement_preserves_authority.
Print Assumptions physical_alias_does_not_merge_stack_authority.
Print Assumptions per_lane_capacity_can_double_count_one_physical_purse.
