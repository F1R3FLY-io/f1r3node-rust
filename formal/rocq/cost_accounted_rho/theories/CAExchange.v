(* ════════════════════════════════════════════════════════════════════════
   CAExchange.v — native blessed Exchange (Stage 5).

   The native re-homing of Exchange.v onto the four-sort grammar. The 1:1 swap
   CONSERVATION is carrier-independent count arithmetic (Exchange.carriers /
   exchange_total_conserved / exchange_conserves_per_channel / exchange_swaps_values)
   — it is about the abstract carriers record, not the calculus carrier, so it
   holds verbatim for the native model and is re-exported here.

   The only calculus-dependent fact is "an Exchange swap is a cost-accounted step,
   never an exogenous mint": viewed at the fuel layer a funded ca_step never
   raises fuel, and minting is never such a step. This is the native analogue of
   Exchange.exchange_is_ca_step_not_amint, built from the native funded/mint
   lemmas. Axiom-free.                                                          *)

From Stdlib Require Import Lia.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CATokenConservation.
From CostAccountedRho Require Import CAStrongNormalization.
From CostAccountedRho Require Import CAMintingInjection.
From CostAccountedRho Require Import Exchange.

(* Re-export the carrier-independent swap conservation for the native model: the
   two carriers' total count is invariant across the swap (no mint, no destroy). *)
Theorem ca_exchange_total_conserved : forall cs,
  carriers_total (exchange_swap cs) = carriers_total cs.
Proof. exact exchange_total_conserved. Qed.

Theorem ca_exchange_swaps_values : forall cs,
  carrier_c (exchange_swap cs) = carrier_v cs
  /\ carrier_v (exchange_swap cs) = carrier_c cs.
Proof. exact exchange_swaps_values. Qed.

(* The native calculus characterization: an exchange swap, viewed at the fuel
   layer, is a FUNDED ca_step (fuel-non-increasing — it moves tokens between
   carriers, it does not mint them), and exogenous minting is never such a step. *)
Theorem ca_exchange_is_step_not_mint : forall S S',
  funded_linear S -> ca_step S S' ->
  st_total_fuel S' <= st_total_fuel S
  /\ (forall t, token_size t > 0 -> ~ ca_step S (mint_inject_st S t)).
Proof.
  intros S S' Hf Hstep. split.
  - apply funded_ca_step_does_not_mint; assumption.
  - intros t Hpos. apply mint_inject_st_not_ca_step; assumption.
Qed.
