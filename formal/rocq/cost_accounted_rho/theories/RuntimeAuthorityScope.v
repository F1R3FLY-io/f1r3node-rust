From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List.
Import ListNotations.

Inductive runtime_authority : Type :=
  | RuntimeUnit
  | RuntimePayer (lane : nat).

Definition runtime_authority_demand (authority : runtime_authority) : list nat :=
  match authority with
  | RuntimeUnit => []
  | RuntimePayer lane => [lane]
  end.

Definition nested_authority_demand
  (outer inner : runtime_authority)
  : list nat :=
  runtime_authority_demand outer ++ runtime_authority_demand inner.

Theorem runtime_unit_has_zero_demand :
  runtime_authority_demand RuntimeUnit = [].
Proof.
  reflexivity.
Qed.

Theorem runtime_unit_is_left_neutral : forall authority,
  nested_authority_demand RuntimeUnit authority =
  runtime_authority_demand authority.
Proof.
  reflexivity.
Qed.

Theorem runtime_unit_is_right_neutral : forall authority,
  nested_authority_demand authority RuntimeUnit =
  runtime_authority_demand authority.
Proof.
  intros authority.
  unfold nested_authority_demand.
  now rewrite app_nil_r.
Qed.

Record scope_owners := {
  scope_a_owned : bool;
  scope_b_owned : bool
}.

Definition empty_scope_owners : scope_owners :=
  {| scope_a_owned := false; scope_b_owned := false |}.

Definition enter_scope_a (owners : scope_owners) : scope_owners :=
  {| scope_a_owned := true; scope_b_owned := scope_b_owned owners |}.

Definition enter_scope_b (owners : scope_owners) : scope_owners :=
  {| scope_a_owned := scope_a_owned owners; scope_b_owned := true |}.

Definition exit_scope_a (owners : scope_owners) : scope_owners :=
  {| scope_a_owned := false; scope_b_owned := scope_b_owned owners |}.

Definition exit_scope_b (owners : scope_owners) : scope_owners :=
  {| scope_a_owned := scope_a_owned owners; scope_b_owned := false |}.

Definition bool_count (value : bool) : nat :=
  if value then 1 else 0.

Definition accounting_scope_count (owners : scope_owners) : nat :=
  bool_count (scope_a_owned owners) + bool_count (scope_b_owned owners).

Definition accounting_scope_active (owners : scope_owners) : bool :=
  negb (accounting_scope_count owners =? 0).

Theorem concurrent_scope_entry_is_order_independent :
  enter_scope_b (enter_scope_a empty_scope_owners) =
  enter_scope_a (enter_scope_b empty_scope_owners).
Proof.
  reflexivity.
Qed.

Theorem first_scope_exit_preserves_other_owner_a :
  accounting_scope_active
    (exit_scope_a (enter_scope_b (enter_scope_a empty_scope_owners))) = true.
Proof.
  reflexivity.
Qed.

Theorem first_scope_exit_preserves_other_owner_b :
  accounting_scope_active
    (exit_scope_b (enter_scope_b (enter_scope_a empty_scope_owners))) = true.
Proof.
  reflexivity.
Qed.

Theorem final_scope_exit_deactivates_accounting :
  accounting_scope_active
    (exit_scope_b
      (exit_scope_a (enter_scope_b (enter_scope_a empty_scope_owners)))) = false.
Proof.
  reflexivity.
Qed.
