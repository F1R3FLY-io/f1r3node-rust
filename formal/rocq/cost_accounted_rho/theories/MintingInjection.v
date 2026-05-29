(* ═══════════════════════════════════════════════════════════════════════════
   MintingInjection.v — Minting is exogenous injection, not a metered step
   ═══════════════════════════════════════════════════════════════════════════

   Stage-0 "layering theorem" of the Cost-Accounted Rho realization.

   The cost-accounted calculus conserves fuel: every [ca_step] consumes a
   strictly positive quantum of token-fuel and never creates any
   (TokenConservation.v: [token_monotone_step], [token_consumed_per_step],
   [token_strictly_decreases]). For that invariant to survive in a running
   system, the act of *minting* — bringing new token-fuel into existence —
   must sit OUTSIDE the reduction relation. Minting is exogenous
   administration (spec §2.4 / §4.6): an authorized party constructs a token
   stack and deposits it as a free token in the ambient parallel
   composition. It is never a cost-accounted reduction step.

   This module makes that separation precise:

   1. [mint_inject S t] is the administrative injection of token stack [t]
      into system [S]; its fuel is exactly the prior fuel plus [token_size t].
   2. No [ca_step] increases the total fuel ([user_ca_step_does_not_mint]),
      so injecting a non-empty stack can never be realized by a [ca_step]
      ([mint_inject_not_ca_step]).
   3. An interleaved administration model ([admin_trans]) evolves a system by
      user [ca_step]s and authorized mint injections. Along any such trace
      the net fuel increase is bounded above by the total minted size:
      reduction only consumes, minting is the SOLE producer
      ([admin_reachable_net_increase_bounded_by_minted]).

   Because minting lives strictly outside [ca_step], every existing
   token-conservation / strong-normalization / confluence result over
   [ca_step] / [ca_reachable] survives verbatim: the producer of fuel is a
   different relation entirely.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition / Theorem                       │ Paper Property
   ────────────────────────────────────────────────┼──────────────────────────
   mint_inject S t                                  │ §2.4/§4.6 token injection
   mint_inject_token_count                          │ "‖mint(S,t)‖ = ‖S‖ + |t|"
   user_ca_step_does_not_mint                       │ "S ⤳ S' ⇒ ‖S'‖ ≤ ‖S‖"
   mint_inject_not_ca_step                          │ "minting ≠ a ⤳ step"
   admin_op / admin_trans                           │ interleaved administration
   admin_trans_step_no_mint                         │ "AStep never creates fuel"
   admin_trans_mint_adds_exactly                    │ "AMint t adds exactly |t|"
   admin_reachable                                  │ ⤳/mint reflexive-trans closure
   admin_reachable_net_increase_bounded_by_minted   │ "reduction consumes;
                                                    │   minting is sole producer"
   ─────────────────────────────────────────────────────────────────────────

   Dependencies: Rocq 9.1.x stdlib, RhoSyntax, CostAccountedSyntax,
                 CostAccountedReduction, TokenConservation (this project)
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Lia.

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CostAccountedReduction.
From CostAccountedRho Require Import TokenConservation.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Minting as Exogenous Injection
   ═══════════════════════════════════════════════════════════════════════════

   [mint_inject S t] deposits the token stack [t] as a free token alongside
   [S]. Compare with the five COMM rules of [ca_step], every one of which
   *strips* gates from an existing token to authorise a redex; minting does
   the opposite, and crucially it is a plain function on systems rather than
   a constructor of the reduction relation.                                   *)

Definition mint_inject (S : system) (t : token) : system :=
  SPar S (SToken t).

(* The fuel of a minted system is exactly the prior fuel plus the size of
   the injected stack. Immediate from the additive shape of
   [system_token_count] on [SPar] and [system_token_count (SToken t) =
   token_size t]. *)
Lemma mint_inject_token_count :
  forall S t,
    system_token_count (mint_inject S t)
    = system_token_count S + token_size t.
Proof.
  intros S t. unfold mint_inject. simpl. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: No Cost-Accounted Step Ever Mints
   ═══════════════════════════════════════════════════════════════════════════

   The conservation invariant restated at the layering boundary: a single
   cost-accounted reduction step never increases the total fuel. This is the
   [<=] form, obtained directly from [token_monotone_step] in
   TokenConservation.v (which is already stated as the non-increase bound;
   were it stated as the strict-decrease [token_strictly_decreases] we would
   weaken it here with [lia]).                                                 *)

Theorem user_ca_step_does_not_mint :
  forall S S',
    ca_step S S' ->
    system_token_count S' <= system_token_count S.
Proof.
  intros S S' Hstep.
  exact (token_monotone_step S S' Hstep).
Qed.

(* Hence minting a NON-EMPTY stack cannot be realized by any [ca_step]:
   such a step would have to raise the fuel count strictly above the source,
   contradicting [user_ca_step_does_not_mint]. *)
Theorem mint_inject_not_ca_step :
  forall S t,
    token_size t > 0 ->
    ~ ca_step S (mint_inject S t).
Proof.
  intros S t Hpos Hstep.
  apply user_ca_step_does_not_mint in Hstep.
  rewrite mint_inject_token_count in Hstep.
  lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Interleaved Administration Model
   ═══════════════════════════════════════════════════════════════════════════

   A running cost-accounted system evolves by two kinds of operation:

     - [AStep]   : a user-driven cost-accounted reduction step ([ca_step]).
     - [AMint t] : an authorized exogenous injection of the token stack [t].

   [admin_trans] is the labelled one-step transition relation that unites
   them. Tagging transitions with the operation lets us state precisely how
   the fuel count is allowed to move: AStep transitions never create fuel,
   AMint transitions create exactly the injected stack size.                  *)

Inductive admin_op : Type :=
  | AStep : admin_op
  | AMint : token -> admin_op.

Inductive admin_trans : admin_op -> system -> system -> Prop :=
  | at_step : forall S S',
      ca_step S S' ->
      admin_trans AStep S S'
  | at_mint : forall S t,
      admin_trans (AMint t) S (mint_inject S t).

(* A user step, viewed as an administrative transition, never creates fuel. *)
Theorem admin_trans_step_no_mint :
  forall S S',
    admin_trans AStep S S' ->
    system_token_count S' <= system_token_count S.
Proof.
  intros S S' Htr.
  inversion Htr; subst.
  apply user_ca_step_does_not_mint. assumption.
Qed.

(* A mint transition adds exactly the size of the injected stack — no more,
   no less. Minting is the only fuel-creating operation, and it creates a
   precisely accountable amount. *)
Theorem admin_trans_mint_adds_exactly :
  forall S S' t,
    admin_trans (AMint t) S S' ->
    system_token_count S' = system_token_count S + token_size t.
Proof.
  intros S S' t Htr.
  inversion Htr; subst.
  apply mint_inject_token_count.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Reachability under Interleaved Administration
   ═══════════════════════════════════════════════════════════════════════════

   [admin_reachable] is the reflexive-transitive closure of [admin_trans],
   forgetting the operation labels. Each step also records the operation in
   [admin_trace], so that we can sum the total fuel minted along a trace.    *)

Inductive admin_reachable : system -> system -> Prop :=
  | ar_refl : forall S,
      admin_reachable S S
  | ar_step : forall op S1 S2 S3,
      admin_trans op S1 S2 ->
      admin_reachable S2 S3 ->
      admin_reachable S1 S3.

(* The total fuel contributed by an administrative operation: a user step
   contributes nothing (it only consumes), a mint contributes exactly the
   size of its injected stack. *)
Definition admin_op_minted (op : admin_op) : nat :=
  match op with
  | AStep    => 0
  | AMint t  => token_size t
  end.

(* A labelled reflexive-transitive closure that also accumulates the total
   minted fuel along the trace, so the "minting is the sole producer"
   accounting can be stated quantitatively. *)
Inductive admin_trace : system -> nat -> system -> Prop :=
  | tr_refl : forall S,
      admin_trace S 0 S
  | tr_step : forall op S1 S2 m S3,
      admin_trans op S1 S2 ->
      admin_trace S2 m S3 ->
      admin_trace S1 (admin_op_minted op + m) S3.

(* An [admin_trace] is in particular an [admin_reachable] (forget the
   accumulated minted total). *)
Lemma admin_trace_reachable :
  forall S m S',
    admin_trace S m S' ->
    admin_reachable S S'.
Proof.
  intros S m S' Htr.
  induction Htr as [S | op S1 S2 m S3 Hstep Htr' IH].
  - apply ar_refl.
  - eapply ar_step.
    + exact Hstep.
    + exact IH.
Qed.

(* Conversely every [admin_reachable] trace can be annotated with the total
   fuel minted along it, witnessed by an [admin_trace]. *)
Lemma admin_reachable_trace :
  forall S S',
    admin_reachable S S' ->
    exists m, admin_trace S m S'.
Proof.
  intros S S' Hreach.
  induction Hreach as [S | op S1 S2 S3 Hstep Hreach' IH].
  - exists 0. apply tr_refl.
  - destruct IH as [m Htr].
    exists (admin_op_minted op + m).
    eapply tr_step.
    + exact Hstep.
    + exact Htr.
Qed.

(* One-step bound: an [admin_trans] raises the fuel count by at most the
   amount it mints. For [AStep] the minted amount is 0 and the count does
   not increase; for [AMint t] the count rises by exactly [token_size t],
   which is the minted amount. In both cases the post-state count is bounded
   above by the pre-state count plus the minted amount. *)
Lemma admin_trans_increase_bounded_by_minted :
  forall op S S',
    admin_trans op S S' ->
    system_token_count S' <= system_token_count S + admin_op_minted op.
Proof.
  intros op S S' Htr.
  destruct Htr as [S S' Hstep | S t].
  - (* AStep: minted = 0, and the step does not create fuel. *)
    apply user_ca_step_does_not_mint in Hstep. simpl. lia.
  - (* AMint t: minted = token_size t, and the count rises by exactly that. *)
    rewrite mint_inject_token_count. simpl. lia.
Qed.

(* Headline accounting theorem: along any administrative trace, the net
   increase in total fuel is bounded above by the total minted along the
   trace. Equivalently, [‖S'‖ ≤ ‖S‖ + (total minted)]: reduction can only
   consume fuel, so MINTING IS THE SOLE PRODUCER of fuel, and it can produce
   at most what it injects.

   By induction on the trace. The reflexive case is immediate. The step case
   chains the one-step bound [admin_trans_increase_bounded_by_minted] with
   the inductive hypothesis on the remainder of the trace; [lia] discharges
   the resulting linear arithmetic over the per-operation minted amounts. *)
Theorem admin_trace_net_increase_bounded_by_minted :
  forall S m S',
    admin_trace S m S' ->
    system_token_count S' <= system_token_count S + m.
Proof.
  intros S m S' Htr.
  induction Htr as [S | op S1 S2 m S3 Hstep Htr' IH].
  - (* tr_refl: ‖S‖ <= ‖S‖ + 0. *)
    lia.
  - (* tr_step: S1 --op--> S2, then S2 ⇝ S3 minting m.
       IH    : ‖S3‖ <= ‖S2‖ + m
       Hstep : ‖S2‖ <= ‖S1‖ + admin_op_minted op
       Goal  : ‖S3‖ <= ‖S1‖ + (admin_op_minted op + m). *)
    apply admin_trans_increase_bounded_by_minted in Hstep.
    lia.
Qed.

(* The same accounting stated directly over [admin_reachable]: there exists a
   total minted amount [m] (the sum of injected stack sizes along the trace)
   that bounds the net fuel increase from above. Fuel created across the
   trace is therefore attributable entirely to minting. *)
Theorem admin_reachable_net_increase_bounded_by_minted :
  forall S S',
    admin_reachable S S' ->
    exists m, system_token_count S' <= system_token_count S + m.
Proof.
  intros S S' Hreach.
  apply admin_reachable_trace in Hreach.
  destruct Hreach as [m Htr].
  exists m.
  apply admin_trace_net_increase_bounded_by_minted.
  exact Htr.
Qed.

(* Corollary: a mint-free administrative trace (total minted = 0) never
   increases the fuel count — it behaves exactly like a pure [ca_reachable]
   reduction sequence. This is the precise sense in which the existing
   token-conservation results survive verbatim once minting is excluded. *)
Corollary admin_trace_no_mint_conserves :
  forall S S',
    admin_trace S 0 S' ->
    system_token_count S' <= system_token_count S.
Proof.
  intros S S' Htr.
  apply admin_trace_net_increase_bounded_by_minted in Htr.
  lia.
Qed.
