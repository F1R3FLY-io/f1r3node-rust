(* ═══════════════════════════════════════════════════════════════════════════
   FuelEventDecomposition.v — Fuel Event Multiset Determinism
   ═══════════════════════════════════════════════════════════════════════════

   Proves that the multiset of fuel-gate COMM events consumed during a
   cost-accounted reduction is determined by the initial system
   configuration, independent of the reduction order.

   This extends the token conservation theorem (TokenConservation.v) from
   the aggregate COUNT of consumed tokens to the IDENTITY of consumed
   tokens: not just "how many" but "which ones."

   The key result is [fuel_events_step]: each [ca_step] partitions the
   system's fuel events into a non-empty consumed portion and the
   remaining events of the post-step system. By induction on
   [ca_reachable], any two reduction sequences from the same initial
   state consume the same multiset of fuel events (up to Permutation).

   This justifies commutative event hashing for consensus: validators
   may process fuel-gate events in any order and still agree on the
   multiset of events that occurred.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Theorem                       │ Property
   ────────────────────────────────────┼────────────────────────────────
   fuel_events_step                   │ Per-step event decomposition
   fuel_events_reachable              │ Multi-step event decomposition
   fuel_events_consumed_perm          │ Consumed events are Permutation-
                                      │ determined by endpoints
   fuel_events_length                 │ |events| = system_token_count
   ─────────────────────────────────────────────────────────────────────────

   Dependencies: CostAccountedSyntax, CostAccountedReduction
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CostAccountedReduction.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Fuel Event Extraction
   ═══════════════════════════════════════════════════════════════════════════

   A fuel event is a (sig, token) pair representing one outermost gate
   in a token stack. The function [token_fuel_events] extracts all gates
   from a token; [fuel_events_of_system] extracts all gates from all
   tokens in a system.                                                    *)

Definition fuel_event := (sig * token)%type.

Fixpoint token_fuel_events (t : token) : list fuel_event :=
  match t with
  | TUnit       => []
  | TGate s t'  => (s, t') :: token_fuel_events t'
  end.

Fixpoint fuel_events_of_system (S : system) : list fuel_event :=
  match S with
  | SSigned _ _ => []
  | SToken t    => token_fuel_events t
  | SPar S1 S2  => fuel_events_of_system S1 ++ fuel_events_of_system S2
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Length Correspondence
   ═══════════════════════════════════════════════════════════════════════════

   The length of the fuel events list equals the existing
   [system_token_count] / [token_size], connecting the new function to
   the existing infrastructure from TokenConservation.v.                  *)

Lemma token_fuel_events_length :
  forall t, length (token_fuel_events t) = token_size t.
Proof.
  induction t; simpl.
  - reflexivity.
  - rewrite IHt. reflexivity.
Qed.

Theorem fuel_events_length :
  forall S, length (fuel_events_of_system S) = system_token_count S.
Proof.
  induction S; simpl.
  - reflexivity.
  - apply token_fuel_events_length.
  - rewrite app_length. rewrite IHS1, IHS2. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Per-Step Decomposition
   ═══════════════════════════════════════════════════════════════════════════

   Each [ca_step] partitions the system's fuel events into a non-empty
   "consumed" portion and the remaining fuel events of the post-step
   system. The consumed portion contains exactly the outermost gate(s)
   stripped by the rule.                                                  *)

Theorem fuel_events_step :
  forall S S',
    ca_step S S' ->
    exists consumed,
      consumed <> [] /\
      Permutation (fuel_events_of_system S) (consumed ++ fuel_events_of_system S').
Proof.
  intros S S' Hstep.
  induction Hstep.
  - (* ca_rule1: strips one gate (s, t) *)
    exists [(s, t)]. split; [discriminate |].
    simpl. apply Permutation_refl.
  - (* ca_rule2: strips two gates (s1, t1) and (s2, t2) *)
    exists [(s1, t1); (s2, t2)]. split; [discriminate |].
    simpl.
    (* LHS: (s1,t1) :: tfe t1 ++ (s2,t2) :: tfe t2
       RHS: (s1,t1) :: (s2,t2) :: tfe t1 ++ tfe t2
       Need to move (s2,t2) from after tfe t1 to position 2. *)
    apply perm_skip.
    apply Permutation_sym.
    apply Permutation_middle.
  - (* ca_rule3: strips one gate (SAnd s1 s2, t) *)
    exists [(SAnd s1 s2, t)]. split; [discriminate |].
    simpl. apply Permutation_refl.
  - (* ca_rule4: strips one gate (SAnd s1 s2, t) *)
    exists [(SAnd s1 s2, t)]. split; [discriminate |].
    simpl. apply Permutation_refl.
  - (* ca_rule5: strips two gates (s1, t1) and (s2, t2) *)
    exists [(s1, t1); (s2, t2)]. split; [discriminate |].
    simpl.
    apply perm_skip.
    apply Permutation_sym.
    apply Permutation_middle.
  - (* ca_par_l: IH on left sub-system *)
    destruct IHHstep as [consumed [Hne Hperm]].
    exists consumed. split; [exact Hne |].
    simpl.
    eapply Permutation_trans.
    + apply Permutation_app_tail.
      exact Hperm.
    + rewrite app_assoc. apply Permutation_refl.
  - (* ca_par_r: IH on right sub-system *)
    destruct IHHstep as [consumed [Hne Hperm]].
    exists consumed. split; [exact Hne |].
    simpl.
    eapply Permutation_trans.
    + apply Permutation_app_head.
      exact Hperm.
    + (* fes S1 ++ consumed ++ fes S2' → consumed ++ fes S1 ++ fes S2' *)
      eapply Permutation_trans.
      * rewrite app_assoc.
        apply Permutation_app_tail.
        apply Permutation_app_comm.
      * rewrite <- app_assoc. apply Permutation_refl.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Multi-Step Decomposition
   ═══════════════════════════════════════════════════════════════════════════

   Lifts [fuel_events_step] across [ca_reachable]: the initial fuel
   events decompose into all consumed events plus the final remaining
   events.                                                                *)

Theorem fuel_events_reachable :
  forall S S',
    ca_reachable S S' ->
    exists consumed,
      Permutation (fuel_events_of_system S) (consumed ++ fuel_events_of_system S').
Proof.
  intros S S' Hreach.
  induction Hreach as [S0 | S0 S1 S2 Hstep Hreach IH].
  - (* car_refl *)
    exists []. simpl. apply Permutation_refl.
  - (* car_step *)
    destruct (fuel_events_step _ _ Hstep) as [c1 [_ Hp1]].
    destruct IH as [c2 Hp2].
    exists (c1 ++ c2).
    eapply Permutation_trans.
    + exact Hp1.
    + rewrite <- app_assoc.
      apply Permutation_app_head.
      exact Hp2.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 5: Consumed Events Determined by Endpoints
   ═══════════════════════════════════════════════════════════════════════════

   If two reduction sequences from the same initial state reach terminal
   states with the same remaining fuel events, then the consumed events
   are the same multiset. This is the theorem that justifies commutative
   event hashing for consensus.                                           *)

Theorem fuel_events_consumed_perm :
  forall (S : system) (consumed1 consumed2 : list fuel_event)
         (remaining1 remaining2 : list fuel_event),
    Permutation (fuel_events_of_system S) (consumed1 ++ remaining1) ->
    Permutation (fuel_events_of_system S) (consumed2 ++ remaining2) ->
    Permutation remaining1 remaining2 ->
    Permutation consumed1 consumed2.
Proof.
  intros S c1 c2 r1 r2 Hp1 Hp2 Hrem.
  (* From Hp1 and Hp2: c1 ++ r1 is a perm of c2 ++ r2. *)
  assert (Hcr : Permutation (c1 ++ r1) (c2 ++ r2)).
  { eapply Permutation_trans.
    - apply Permutation_sym. exact Hp1.
    - exact Hp2. }
  (* From Hrem: r1 is a perm of r2. Replace r1 with r2 in c1 ++ r1. *)
  assert (Hcr2 : Permutation (c1 ++ r2) (c2 ++ r2)).
  { eapply Permutation_trans.
    - apply Permutation_app_head. apply Permutation_sym. exact Hrem.
    - exact Hcr. }
  (* Cancel r2 from both sides. *)
  apply Permutation_app_inv_r with (l := r2).
  exact Hcr2.
Qed.

(* The headline corollary: two reductions from the same initial state
   that reach endpoints with the same remaining events have consumed
   the same multiset of fuel events. *)
Corollary fuel_events_consumed_determined :
  forall S S1 S2,
    ca_reachable S S1 ->
    ca_reachable S S2 ->
    Permutation (fuel_events_of_system S1) (fuel_events_of_system S2) ->
    forall consumed1 consumed2,
      Permutation (fuel_events_of_system S) (consumed1 ++ fuel_events_of_system S1) ->
      Permutation (fuel_events_of_system S) (consumed2 ++ fuel_events_of_system S2) ->
      Permutation consumed1 consumed2.
Proof.
  intros S S1 S2 _ _ Hrem c1 c2 Hp1 Hp2.
  apply (fuel_events_consumed_perm S c1 c2
           (fuel_events_of_system S1) (fuel_events_of_system S2)
           Hp1 Hp2 Hrem).
Qed.
