(* ═══════════════════════════════════════════════════════════════════════════
   Settlement.v — RevVault Reservation Settlement
   ═══════════════════════════════════════════════════════════════════════════

   The cost-accounted rho calculus controls computation by consuming finite
   authority during reduction. Casper reserves the physical-authority bound,
   quantitative-byte bound, and fee from the payer's RevVault purse before
   execution. Settlement burns the realized physical and byte costs, transfers
   the fee, and releases only the unused reservation.

   This file records that separation as small arithmetic theorems. The
   calculus-side theorem is imported from TokenConservation: reachable
   evaluation states cannot synthesize fuel. The settlement-side theorems
   show that realized costs bounded component-wise by their certificate-bound
   maxima conserve the reservation exactly. The fee is never refundable and
   is credited once to the proposer.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.PeanoNat Lia.

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CostAccountedReduction.
From CostAccountedRho Require Import TokenConservation.

Record fee_settlement := {
  settlement_physical_bound : nat;
  settlement_byte_bound : nat;
  settlement_fee : nat;
  settlement_physical_cost : nat;
  settlement_byte_cost : nat
}.

Definition reserved_amount (s : fee_settlement) : nat :=
  settlement_physical_bound s + settlement_byte_bound s + settlement_fee s.

Definition burned_amount (s : fee_settlement) : nat :=
  settlement_physical_cost s + settlement_byte_cost s.

Definition debited_amount (s : fee_settlement) : nat :=
  burned_amount s + settlement_fee s.

Definition refund_amount (s : fee_settlement) : nat :=
  (settlement_physical_bound s - settlement_physical_cost s) +
  (settlement_byte_bound s - settlement_byte_cost s).

Definition settled_amount (s : fee_settlement) : nat :=
  debited_amount s + refund_amount s.

Theorem refund_le_reservation : forall s,
  refund_amount s <= reserved_amount s.
Proof.
  intros s.
  unfold refund_amount, reserved_amount.
  lia.
Qed.

Theorem debit_le_reservation_when_bounded : forall s,
  settlement_physical_cost s <= settlement_physical_bound s ->
  settlement_byte_cost s <= settlement_byte_bound s ->
  debited_amount s <= reserved_amount s.
Proof.
  intros s Hphysical Hbyte.
  unfold debited_amount, burned_amount, reserved_amount.
  lia.
Qed.

Theorem debit_plus_refund_eq_reservation : forall s,
  settlement_physical_cost s <= settlement_physical_bound s ->
  settlement_byte_cost s <= settlement_byte_bound s ->
  settled_amount s = reserved_amount s.
Proof.
  intros s Hphysical Hbyte.
  unfold settled_amount, debited_amount, burned_amount, refund_amount,
    reserved_amount.
  lia.
Qed.

Theorem refund_zero_when_components_exhausted : forall s,
  settlement_physical_bound s <= settlement_physical_cost s ->
  settlement_byte_bound s <= settlement_byte_cost s ->
  refund_amount s = 0.
Proof.
  intros s Hphysical Hbyte.
  unfold refund_amount.
  lia.
Qed.

Theorem settlement_deterministic : forall a b,
  settlement_physical_bound a = settlement_physical_bound b ->
  settlement_byte_bound a = settlement_byte_bound b ->
  settlement_fee a = settlement_fee b ->
  settlement_physical_cost a = settlement_physical_cost b ->
  settlement_byte_cost a = settlement_byte_cost b ->
  reserved_amount a = reserved_amount b /\
  burned_amount a = burned_amount b /\
  debited_amount a = debited_amount b /\
  refund_amount a = refund_amount b /\
  settled_amount a = settled_amount b.
Proof.
  intros a b Hphysical_bound Hbyte_bound Hfee Hphysical_cost Hbyte_cost.
  cbv [reserved_amount burned_amount debited_amount refund_amount settled_amount].
  rewrite Hphysical_bound, Hbyte_bound, Hfee, Hphysical_cost, Hbyte_cost.
  repeat split; reflexivity.
Qed.

Theorem evaluation_cannot_receive_refund_fuel : forall S S',
  ca_reachable S S' ->
  system_token_count S' <= system_token_count S.
Proof.
  intros S S' Hreach.
  exact (token_monotone_reachable S S' Hreach).
Qed.

Theorem evaluation_step_cannot_mint_fuel : forall S S',
  ca_step S S' ->
  system_token_count S' < system_token_count S.
Proof.
  intros S S' Hstep.
  exact (token_strictly_decreases S S' Hstep).
Qed.

Theorem post_evaluation_settlement_no_mint : forall S S',
  ca_reachable S S' ->
  let consumed := system_token_count S - system_token_count S' in
  let settlement := {|
    settlement_physical_bound := system_token_count S;
    settlement_byte_bound := 0;
    settlement_fee := 0;
    settlement_physical_cost := consumed;
    settlement_byte_cost := 0
  |} in
  settled_amount settlement = reserved_amount settlement.
Proof.
  intros S S' Hreach.
  pose proof (token_monotone_reachable S S' Hreach) as Hmono.
  cbv [settled_amount debited_amount burned_amount refund_amount reserved_amount].
  cbn.
  lia.
Qed.
