(* ════════════════════════════════════════════════════════════════════════
   CAMintingInjection.v — native minting-injection layering (Stage 5).

   The native re-homing of MintingInjection.v's core onto the four-sort grammar.
   Minting is exogenous token injection — NEVER a cost-accounted step — so the
   conservation / SN / determinism results survive verbatim alongside it.

   As with CASettlement, native fuel monotonicity is on the funded fragment (a
   funded ca_step strictly decreases fuel), whereas the OLD model's
   token_monotone_step was unconditional. The "minting is not a ca_step" fact
   thus holds for funded systems (the admitted deploys) — exactly matching the
   conditional-SN finding (DR-21). Axiom-free.                                  *)

From Stdlib Require Import Lia.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CATokenConservation.
From CostAccountedRho Require Import CAStrongNormalization.

(* Native exogenous mint injection: place a fresh token stack in parallel. *)
Definition mint_inject_st (S : signed_term) (t : token) : signed_term :=
  STPar S (STStack t).

(* A minted system has exactly the prior fuel plus the injected stack size. *)
Lemma mint_inject_st_fuel : forall S t,
  st_total_fuel (mint_inject_st S t) = st_total_fuel S + token_size t.
Proof. intros S t. unfold mint_inject_st. simpl. reflexivity. Qed.

(* No FUNDED cost-accounted step ever mints (raises fuel): the [<=] form. *)
Theorem funded_ca_step_does_not_mint : forall S S',
  funded_linear S -> ca_step S S' -> st_total_fuel S' <= st_total_fuel S.
Proof.
  intros S S' Hf Hstep.
  pose proof (funded_step_decreases S S' Hf Hstep). lia.
Qed.

(* Hence injecting a NON-EMPTY stack into a funded system cannot be realized by
   any ca_step — it would have to raise the fuel strictly. *)
Theorem mint_inject_st_not_ca_step : forall S t,
  funded_linear S -> token_size t > 0 -> ~ ca_step S (mint_inject_st S t).
Proof.
  intros S t Hf Hpos Hstep.
  apply (funded_ca_step_does_not_mint S (mint_inject_st S t) Hf) in Hstep.
  rewrite mint_inject_st_fuel in Hstep. lia.
Qed.

(* The native interleaved administration model: a running system evolves by user
   ca_steps (never mint) or authorized exogenous mints (create exactly the
   injected stack). The labelled one-step relation. *)
Inductive ca_admin_op : Type :=
  | CAStep : ca_admin_op
  | CAMint : token -> ca_admin_op.

Inductive ca_admin_trans : ca_admin_op -> signed_term -> signed_term -> Prop :=
  | cat_step : forall S S', ca_step S S' -> ca_admin_trans CAStep S S'
  | cat_mint : forall S t, ca_admin_trans (CAMint t) S (mint_inject_st S t).

(* Fuel motion is exactly classified by the operation: a funded user step never
   raises fuel; a mint raises it by exactly the injected stack size. *)
Theorem ca_admin_fuel_classified : forall op S S',
  funded_linear S -> ca_admin_trans op S S' ->
  match op with
  | CAStep   => st_total_fuel S' <= st_total_fuel S
  | CAMint t => st_total_fuel S' = st_total_fuel S + token_size t
  end.
Proof.
  intros op S S' Hf Htr. destruct Htr.
  - apply funded_ca_step_does_not_mint; assumption.
  - apply mint_inject_st_fuel.
Qed.
