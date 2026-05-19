(* ═══════════════════════════════════════════════════════════════════════════
   Settlement.v — Post-Evaluation Fee Settlement
   ═══════════════════════════════════════════════════════════════════════════

   The cost-accounted rho calculus controls computation by consuming fuel
   tokens during reduction. Casper payment movement is a different layer:
   a deploy escrows phlo before evaluation and receives a refund only after
   evaluation has produced its final consumed-token count.

   This file records that separation as small arithmetic theorems. The
   calculus-side theorem is imported from TokenConservation: reachable
   evaluation states cannot synthesize fuel. The settlement-side theorems
   show that, when the runtime reports a consumed-token count bounded by the
   deploy's limit, post-evaluation charged and refunded phlo exactly account
   for the escrowed amount.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.PeanoNat Lia.

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CostAccountedReduction.
From CostAccountedRho Require Import TokenConservation.

Record fee_settlement := {
  settlement_limit : nat;
  settlement_price : nat;
  settlement_token_cost : nat
}.

Definition escrowed_amount (s : fee_settlement) : nat :=
  settlement_limit s * settlement_price s.

Definition charged_amount (s : fee_settlement) : nat :=
  settlement_token_cost s * settlement_price s.

Definition refund_amount (s : fee_settlement) : nat :=
  (settlement_limit s - settlement_token_cost s) * settlement_price s.

Definition settled_amount (s : fee_settlement) : nat :=
  charged_amount s + refund_amount s.

Theorem refund_le_escrow : forall s,
  refund_amount s <= escrowed_amount s.
Proof.
  intros s.
  unfold refund_amount, escrowed_amount.
  apply Nat.mul_le_mono_r.
  lia.
Qed.

Theorem charged_le_escrow_when_bounded : forall s,
  settlement_token_cost s <= settlement_limit s ->
  charged_amount s <= escrowed_amount s.
Proof.
  intros s Hbounded.
  unfold charged_amount, escrowed_amount.
  apply Nat.mul_le_mono_r.
  exact Hbounded.
Qed.

Theorem charged_plus_refund_eq_escrow : forall s,
  settlement_token_cost s <= settlement_limit s ->
  settled_amount s = escrowed_amount s.
Proof.
  intros s Hbounded.
  unfold settled_amount, charged_amount, refund_amount, escrowed_amount.
  rewrite <- Nat.mul_add_distr_r.
  assert (settlement_token_cost s +
          (settlement_limit s - settlement_token_cost s) =
          settlement_limit s) by lia.
  rewrite H.
  reflexivity.
Qed.

Theorem refund_zero_when_exhausted : forall s,
  settlement_limit s <= settlement_token_cost s ->
  refund_amount s = 0.
Proof.
  intros s Hexhausted.
  unfold refund_amount.
  assert (settlement_limit s - settlement_token_cost s = 0) by lia.
  rewrite H.
  lia.
Qed.

Theorem settlement_deterministic : forall a b,
  settlement_limit a = settlement_limit b ->
  settlement_price a = settlement_price b ->
  settlement_token_cost a = settlement_token_cost b ->
  escrowed_amount a = escrowed_amount b /\
  charged_amount a = charged_amount b /\
  refund_amount a = refund_amount b /\
  settled_amount a = settled_amount b.
Proof.
  intros a b Hlimit Hprice Hcost.
  cbv [escrowed_amount charged_amount refund_amount settled_amount].
  rewrite Hlimit, Hprice, Hcost.
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

Theorem post_evaluation_settlement_no_mint : forall S S' price,
  ca_reachable S S' ->
  let consumed := system_token_count S - system_token_count S' in
  let settlement := {|
    settlement_limit := system_token_count S;
    settlement_price := price;
    settlement_token_cost := consumed
  |} in
  settled_amount settlement = escrowed_amount settlement.
Proof.
  intros S S' price Hreach.
  pose proof (token_monotone_reachable S S' Hreach) as Hmono.
  cbv [settled_amount charged_amount refund_amount escrowed_amount].
  cbn.
  replace ((system_token_count S - system_token_count S') * price +
           (system_token_count S -
            (system_token_count S - system_token_count S')) * price)
    with ((system_token_count S - system_token_count S' +
           (system_token_count S -
            (system_token_count S - system_token_count S'))) * price)
    by (rewrite Nat.mul_add_distr_r; reflexivity).
  f_equal.
  lia.
Qed.
