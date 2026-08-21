(* ════════════════════════════════════════════════════════════════════════
   CAModulus.v — "stack consumption is the modulus" (Stage 3e).

   The monad paper's Proposition "Stack consumption is the modulus, lazily
   realised" (continued-gslt-cost-v2.tex): the consumed token stack is "an exact
   operational modulus for the cut elimination, realised by running the reduction
   rather than by a separate traversal." Natively (where the measure is conditional
   on funding), this is realized as: a hereditarily-funded reduction's LENGTH is
   bounded by its total fuel [st_total_fuel], because every funded step strictly
   drains it. Evaluation IS the bound extraction — the consumed stack bounds the
   run length, tight by construction and lazy. Axiom-free.                      *)

From Stdlib Require Import Lia.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CATokenConservation.
From CostAccountedRho Require Import CAStrongNormalization.
From CostAccountedRho Require Import CACostDeterminism.
From CostAccountedRho Require Import CAStepDeterminism.   (* ca_reachable_n *)

(* The modulus: an n-step hereditarily-funded reduction has n at most the total
   fuel of the source — the consumed stack is the realised cost of the run. *)
Theorem funded_run_bounded : forall n S T,
  HF S -> ca_reachable_n n S T -> n <= st_total_fuel S.
Proof.
  intros n S T HFS Hpath. revert HFS.
  induction Hpath as [S | n' S S1 T Hstep Htail IH]; intro HFS.
  - lia.
  - assert (Hdec : st_total_fuel S1 < st_total_fuel S)
      by (apply funded_step_decreases; [ apply HF_funded; exact HFS | exact Hstep ]).
    assert (HFS1 : HF S1) by (eapply HF_step; eassumption).
    specialize (IH HFS1). lia.
Qed.

(* Consequently the total fuel strictly bounds every funded run, so funded
   reductions are finite — the operational face of ca_SN_funded, with the
   explicit step budget. *)
Corollary funded_run_terminates_within_budget : forall S,
  HF S -> forall n T, ca_reachable_n n S T -> n <= st_total_fuel S.
Proof. intros S HFS n T Hpath. eapply funded_run_bounded; eassumption. Qed.
