(* ═══════════════════════════════════════════════════════════════════════════
   TokenConservation.v — Token Count Invariant
   ═══════════════════════════════════════════════════════════════════════════

   Proves that the total number of fuel tokens in a system never increases
   under cost-accounted reduction. This is the fundamental conservation law
   of the cost-accounted rho calculus: fuel is neither minted out of thin
   air nor smuggled in through PAR contexts; it can only be consumed by the
   five COMM rules.

   Each of the five rules strips at least one outermost gate from a token
   that authorises the redex, replacing it with the token's suffix. The
   structural rules ca_par_l and ca_par_r are contextual closures that
   propagate the per-rule decrease through parallel composition without
   ever introducing new tokens. Adding the reflexive-transitive closure
   on top of single steps gives the multi-step monotone-decrease theorem.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Theorem                 │ Paper Property
   ─────────────────────────────┼────────────────────────────────────────────
   token_monotone_step          │ "Single step never creates fuel:
                                │   S ⤳ S' ⇒ ‖S'‖ ≤ ‖S‖"
   token_monotone_reachable     │ "Many steps never create fuel:
                                │   S ⤳* S' ⇒ ‖S'‖ ≤ ‖S‖"
   rule1_decreases_by_one       │ "Rule 1 consumes exactly one fuel unit"
   rule2_decreases_by_two       │ "Rule 2 consumes exactly two fuel units"
   rule3_decreases_by_one       │ "Rule 3 consumes exactly one fuel unit"
   rule4_decreases_by_one       │ "May Rule 5 consumes one fuel unit" (April Rule 4)
   rule5_decreases_by_two       │ "May Rule 4 consumes two fuel units" (April Rule 5)
   (Lemma suffixes track the ca_rule4/ca_rule5 constructors; the May-2026 spec
    Section 3.6 swaps the labels — see the canonical note in CostAccountedReduction.v.)
   ─────────────────────────────────────────────────────────────────────────

   Dependencies: Rocq 9.1.1 stdlib, CostAccountedSyntax,
                 CostAccountedReduction (this project)
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Lia.

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CostAccountedReduction.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Single-Step Conservation
   ═══════════════════════════════════════════════════════════════════════════

   The headline single-step lemma. By induction on the derivation of
   [ca_step S S'] each of the five COMM rules unfolds [system_token_count]
   on both sides into a closed arithmetic identity that [lia] discharges
   immediately. The PAR cases are dispatched the same way: the inductive
   hypothesis hands us the per-side inequality, and the additive shape of
   [system_token_count] on [SPar] turns it into a sum-respecting bound.
                                                                            *)

Theorem token_monotone_step : forall S S',
  ca_step S S' ->
  system_token_count S' <= system_token_count S.
Proof.
  intros S S' Hstep.
  induction Hstep; simpl.
  - (* ca_rule1: lhs = 0 + (1 + token_size t)
                 rhs = 0 + token_size t
       Net decrease: 1. *)
    lia.
  - (* ca_rule2: lhs = (0 + (1 + token_size t1)) + (1 + token_size t2)
                 rhs = (0 + token_size t1) + token_size t2
       Net decrease: 2. *)
    lia.
  - (* ca_rule3: same shape as ca_rule1 (decrease by 1). *)
    lia.
  - (* ca_rule4: lhs = ((0 + 0) + (1 + token_size t))
                 rhs = (0 + token_size t)
       Net decrease: 1. *)
    lia.
  - (* ca_rule5: same shape as ca_rule2 (decrease by 2). *)
    lia.
  - (* ca_par_l: contextual closure on the left subsystem.
                 IHHstep : count S1' <= count S1
       so       count (S1' ∥ S2) = count S1' + count S2
                                 <= count S1 + count S2
                                 = count (S1 ∥ S2). *)
    lia.
  - (* ca_par_r: symmetric to ca_par_l. *)
    lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Multi-Step Conservation (Reachability)
   ═══════════════════════════════════════════════════════════════════════════

   Lifts [token_monotone_step] across the reflexive-transitive closure
   [ca_reachable]. The base case [car_refl] gives [count S <= count S]
   trivially; the inductive [car_step] case chains the per-step decrease
   from [token_monotone_step] with the inductive hypothesis on the
   remainder of the reduction sequence.                                     *)

Theorem token_monotone_reachable : forall S S',
  ca_reachable S S' ->
  system_token_count S' <= system_token_count S.
Proof.
  intros S S' Hreach.
  induction Hreach as [S0 | S1 S2 S3 Hstep Hreach' IH].
  - (* car_refl: empty sequence, count is unchanged. *)
    lia.
  - (* car_step: S1 ⤳ S2 and S2 ⤳* S3.
                 IH : count S3 <= count S2
       Hstep gives count S2 <= count S1 via token_monotone_step,
       so by transitivity count S3 <= count S1. *)
    apply token_monotone_step in Hstep.
    lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Exact Per-Rule Decrease
   ═══════════════════════════════════════════════════════════════════════════

   The five lemmas below pin down the exact amount by which the token
   count drops on each individual rule. They are stronger than
   [token_monotone_step] in that they give an equality rather than an
   inequality, but each is dispatched by [simpl; lia] because the rule's
   source and target have closed-form token counts.                         *)

Lemma rule1_decreases_by_one : forall x P Q s t,
  system_token_count
    (SPar (SSigned (PPar (PInput x P) (POutput x Q)) s)
          (SToken (TGate s t)))
  = 1 + system_token_count
    (SPar (SSigned (subst_proc P 0 (Quote Q)) s)
          (SToken t)).
Proof.
  intros. simpl. lia.
Qed.

Lemma rule2_decreases_by_two : forall x P Q s1 s2 t1 t2,
  system_token_count
    (SPar (SPar (SSigned (PPar (PInput x P) (POutput x Q)) (SAnd s1 s2))
                (SToken (TGate s1 t1)))
          (SToken (TGate s2 t2)))
  = 2 + system_token_count
    (SPar (SPar (SSigned (subst_proc P 0 (Quote Q)) (SAnd s1 s2))
                (SToken t1))
          (SToken t2)).
Proof.
  intros. simpl. lia.
Qed.

Lemma rule3_decreases_by_one : forall x P Q s1 s2 t,
  system_token_count
    (SPar (SSigned (PPar (PInput x P) (POutput x Q)) (SAnd s1 s2))
          (SToken (TGate (SAnd s1 s2) t)))
  = 1 + system_token_count
    (SPar (SSigned (subst_proc P 0 (Quote Q)) (SAnd s1 s2))
          (SToken t)).
Proof.
  intros. simpl. lia.
Qed.

Lemma rule4_decreases_by_one : forall x P Q s1 s2 t,
  system_token_count
    (SPar (SPar (SSigned (PInput x P) s1)
                (SSigned (POutput x Q) s2))
          (SToken (TGate (SAnd s1 s2) t)))
  = 1 + system_token_count
    (SPar (SSigned (subst_proc P 0 (Quote Q)) (SAnd s1 s2))
          (SToken t)).
Proof.
  intros. simpl. lia.
Qed.

Lemma rule5_decreases_by_two : forall x P Q s1 s2 t1 t2,
  system_token_count
    (SPar (SPar (SPar (SSigned (PInput x P) s1)
                      (SSigned (POutput x Q) s2))
                (SToken (TGate s1 t1)))
          (SToken (TGate s2 t2)))
  = 2 + system_token_count
    (SPar (SPar (SSigned (subst_proc P 0 (Quote Q)) (SAnd s1 s2))
                (SToken t1))
          (SToken t2)).
Proof.
  intros. simpl. lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Exact-Decrease Theorem (the conservation invariant)
   ═══════════════════════════════════════════════════════════════════════════

   The headline theorem: every cost-accounted reduction step consumes a
   STRICTLY POSITIVE amount of fuel. Combined with non-negativity of
   token counts, this gives a termination measure for cost-accounted
   reductions: no infinite reduction sequence is possible from a
   finite-fuel system.

   The proof case-splits on the rule and uses the per-rule decrease
   lemmas. For the contextual closure cases (ca_par_l, ca_par_r), the
   inductive hypothesis carries the existence of the consumed quantum
   through the parallel composition.                                       *)

Theorem token_consumed_per_step : forall S S',
  ca_step S S' ->
  exists k, k > 0 /\ system_token_count S = k + system_token_count S'.
Proof.
  intros S S' Hstep.
  induction Hstep.
  - (* ca_rule1: decreases by 1 *)
    exists 1. split; [lia |]. simpl. lia.
  - (* ca_rule2: decreases by 2 *)
    exists 2. split; [lia |]. simpl. lia.
  - (* ca_rule3: decreases by 1 *)
    exists 1. split; [lia |]. simpl. lia.
  - (* ca_rule4: decreases by 1 *)
    exists 1. split; [lia |]. simpl. lia.
  - (* ca_rule5: decreases by 2 *)
    exists 2. split; [lia |]. simpl. lia.
  - (* ca_par_l: lift the existential through the parallel context *)
    destruct IHHstep as [k [Hk Heq]].
    exists k. split; [exact Hk |]. simpl. lia.
  - (* ca_par_r: symmetric *)
    destruct IHHstep as [k [Hk Heq]].
    exists k. split; [exact Hk |]. simpl. lia.
Qed.

(* Corollary: cost-accounted reduction is strictly decreasing on the
   token count, hence well-founded (no infinite reductions). *)
Corollary token_strictly_decreases : forall S S',
  ca_step S S' ->
  system_token_count S' < system_token_count S.
Proof.
  intros S S' Hstep.
  apply token_consumed_per_step in Hstep.
  destruct Hstep as [k [Hk Heq]].
  lia.
Qed.
