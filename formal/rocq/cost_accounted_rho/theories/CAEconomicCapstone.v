(* ════════════════════════════════════════════════════════════════════════
   CAEconomicCapstone.v — the native economic layer, consolidated (Stage 5).

   Assembles the native carrier-dependent economic core (CASettlement,
   CAMintingInjection, CAExchange) into one guarantee on the hereditarily-funded
   fragment: along any funded evaluation,
     (1) fuel is conserved (never minted) — ca_funded_reachable_monotone;
     (2) fee settlement balances exactly (charged + refund = escrow) —
         ca_post_evaluation_settlement_no_mint;
     (3) no exogenous mint is realizable as a cost-accounted step —
         mint_inject_st_not_ca_step.

   CARRIER-INDEPENDENCE NOTE. The remaining economic modules are carrier-
   independent — they reference the PoS-state / runtime / resource abstractions,
   NOT the calculus carrier (system vs signed_term), so they hold for the native
   model verbatim with no re-homing:
     - MintingHalt (halted-validator supply interface; PoS-state abstraction);
     - RuntimeBudgetRefinement (bounded-memory budget refinement; runtime model);
     - MultiSignerRefinement (Map-in-MVar PoS refinement; runtime model);
     - LinearLogicResources / LLIdentities (the linear-resource calculus +
       multiplicative/additive/exponential identities; Stdlib-level).
   The SlashingComposition / MergeableChannelAccounting boundary conservation
   (system_token_count non-increase along ca_reachable) reduces to the native
   ca_funded_reachable_monotone on the funded fragment. Axiom-free.            *)

From Stdlib Require Import Lia.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CATokenConservation.
From CostAccountedRho Require Import CACostDeterminism.
From CostAccountedRho Require Import Settlement.
From CostAccountedRho Require Import CASettlement.
From CostAccountedRho Require Import CAMintingInjection.

Theorem ca_economic_conservation : forall S S',
  HF S -> ca_reachable S S' ->
  st_total_fuel S' <= st_total_fuel S
  /\ (forall price,
        settled_amount {| settlement_limit := st_total_fuel S;
                          settlement_price := price;
                          settlement_token_cost := ca_consumed S S' |}
        = escrowed_amount {| settlement_limit := st_total_fuel S;
                             settlement_price := price;
                             settlement_token_cost := ca_consumed S S' |})
  /\ (forall t, token_size t > 0 -> ~ ca_step S (mint_inject_st S t)).
Proof.
  intros S S' HFS Hreach. repeat split.
  - apply ca_funded_reachable_monotone; assumption.
  - intro price. apply ca_post_evaluation_settlement_no_mint; assumption.
  - intros t Hpos. apply mint_inject_st_not_ca_step; [ apply HF_funded; assumption | assumption ].
Qed.
