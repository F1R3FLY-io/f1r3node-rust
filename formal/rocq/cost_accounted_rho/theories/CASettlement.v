(* ════════════════════════════════════════════════════════════════════════
   CASettlement.v — native post-evaluation fee settlement (Stage 5).

   The native re-homing of Settlement.v onto the four-sort grammar. The fee
   arithmetic (the fee_settlement record + charged/refund/escrow laws) is
   carrier-independent and reused verbatim from Settlement; the only
   calculus-dependent link is token conservation, which here is the NATIVE
   st_total_fuel measure along native ca_reachable.

   Unlike the old model (token_monotone_reachable was UNCONDITIONAL), native fuel
   monotonicity holds on the HEREDITARILY-FUNDED fragment (HF) — exactly the
   conditional-SN finding (DR-21): off the funded fragment st_total_fuel can rise
   (st_total_fuel_can_increase_off_funded), but the admitted (funded) deploys are
   precisely the HF ones, on which settlement conserves. Axiom-free.            *)

From Stdlib Require Import Lia.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CATokenConservation.
From CostAccountedRho Require Import CAStrongNormalization.
From CostAccountedRho Require Import CACostDeterminism.
From CostAccountedRho Require Import Settlement.

(* Native fuel monotonicity on the hereditarily-funded fragment: a funded
   evaluation never synthesizes fuel along reachability. *)
Theorem ca_funded_reachable_monotone : forall S S',
  HF S -> ca_reachable S S' -> st_total_fuel S' <= st_total_fuel S.
Proof.
  intros S S' HFS Hreach. induction Hreach.
  - lia.
  - assert (st_total_fuel S2 < st_total_fuel S1) as Hdec
      by (apply funded_step_decreases; [ apply HF_funded; assumption | assumption ]).
    assert (HF S2) as HFS2 by (eapply HF_step; eassumption).
    specialize (IHHreach HFS2). lia.
Qed.

(* A single funded evaluation step strictly decreases fuel (never mints). *)
Theorem ca_evaluation_step_cannot_mint_fuel : forall S S',
  funded_linear S -> ca_step S S' -> st_total_fuel S' < st_total_fuel S.
Proof. intros S S' Hf Hstep. apply funded_step_decreases; assumption. Qed.

(* The native consumed-token count along a funded run. *)
Definition ca_consumed (S S' : signed_term) : nat :=
  st_total_fuel S - st_total_fuel S'.

(* Post-evaluation settlement balances (no mint) on the HF fragment: with the
   draw limit = the initial fuel and the token cost = the native consumed count,
   charged + refund = escrow (the carrier-independent fee arithmetic applies,
   the consumed count being bounded by the limit). *)
Theorem ca_post_evaluation_settlement_no_mint : forall S S' price,
  HF S -> ca_reachable S S' ->
  settled_amount {| settlement_limit := st_total_fuel S;
                    settlement_price := price;
                    settlement_token_cost := ca_consumed S S' |}
  = escrowed_amount {| settlement_limit := st_total_fuel S;
                       settlement_price := price;
                       settlement_token_cost := ca_consumed S S' |}.
Proof.
  intros S S' price HFS Hreach.
  apply charged_plus_refund_eq_escrow. simpl. unfold ca_consumed. lia.
Qed.

(* The refund is exactly the un-consumed fuel (drawn = charged + released). *)
Theorem ca_settlement_refund_is_unconsumed : forall S S' price,
  HF S -> ca_reachable S S' ->
  refund_amount {| settlement_limit := st_total_fuel S;
                   settlement_price := price;
                   settlement_token_cost := ca_consumed S S' |}
  = st_total_fuel S' * price.
Proof.
  intros S S' price HFS Hreach.
  pose proof (ca_funded_reachable_monotone S S' HFS Hreach) as Hmono.
  unfold refund_amount, ca_consumed. simpl. f_equal. lia.
Qed.
