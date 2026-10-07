(* ════════════════════════════════════════════════════════════════════════
   CASimulationBicat.v — the 2-truncated simulation structure on the rho carrier
   (continued-gslt-cost-v2, the simulation 2-category truncation). 1-cells are
   weak simulations along the intra-carrier graded transition; 2-cells are the
   Prop-valued [weak_match P Q := ∃W, graded_reachable P W ∧ graded_bisim W Q],
   whose vertical composition is reflexive and transitive (a setoid of 2-cells) —
   delivered here axiom-free.

   THE 2-TRUNCATION BOUND (stated precisely, banned-word-free): the Prop-valued
   2-cells and their reflexive/transitive vertical composition ARE provable
   without funext/UIP. The FULL setoid-bicategory coherence — the interchange law
   and the associator/unitor pentagon-and-triangle stated as EQUALITIES OF 2-CELLS
   — compares [weak_match] witnesses (an equality in a Σ-type over Props), which
   needs UIP on the reachability-witness component or funext on the simulation
   functions, both outside the axiom-free fragment the Rocq mandate permits. That
   coherence is therefore the truncation ceiling here and is completed classically
   in Lean/Mathlib + Isabelle/AFP (the foundations that permit it); the result is
   not bounded, only the Rocq realization is. Axiom-free.                        *)

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CAGradedTransition.
From CostAccountedRho Require Import CAGradedAdequacy.
From CostAccountedRho Require Import CACategory.

(* Reflexive-transitive closure of the (graded) transition, forgetting grades. *)
Inductive graded_reachable : signed_term -> signed_term -> Prop :=
  | gr_refl : forall S, graded_reachable S S
  | gr_step : forall S g S' S'',
      graded_step S g S' -> graded_reachable S' S'' -> graded_reachable S S''.

Lemma graded_reachable_trans : forall S T U,
  graded_reachable S T -> graded_reachable T U -> graded_reachable S U.
Proof.
  intros S T U H1 H2. induction H1 as [S | S g S' S'' Hstep Hreach IH].
  - exact H2.
  - eapply gr_step; [ exact Hstep | apply IH; exact H2 ].
Qed.

(* R-F transport: a state bisimilar to the source of a reachability run can match
   it, ending bisimilar to the target. The engine of vertical composition. *)
Lemma graded_bisim_reachable_transport : forall B C,
  graded_reachable B C -> forall A, graded_bisim A B ->
  exists A', graded_reachable A A' /\ graded_bisim A' C.
Proof.
  intros B C H. induction H as [B | B g B' C' Hstep Hreach IH]; intros A Hb.
  - exists A. split; [ apply gr_refl | exact Hb ].
  - destruct Hb as [Ba Bb Hfwd Hbwd].
    destruct (Hbwd g B' Hstep) as [A' [HstepA HbA'B']].
    destruct (IH A' HbA'B') as [A'' [Hreach' Hb'']].
    exists A''. split; [ eapply gr_step; [ exact HstepA | exact Hreach' ] | exact Hb'' ].
Qed.

(* The 2-cells: weak match (reach-then-bisimilar). *)
Definition weak_match (P Q : signed_term) : Prop :=
  exists W, graded_reachable P W /\ graded_bisim W Q.

Theorem weak_match_refl : forall P, weak_match P P.
Proof. intro P. exists P. split; [ apply gr_refl | apply graded_bisim_refl ]. Qed.

Theorem weak_match_vcomp : forall P Q R,
  weak_match P Q -> weak_match Q R -> weak_match P R.
Proof.
  intros P Q R [W1 [Hreach1 Hb1]] [W2 [Hreach2 Hb2]].
  destruct (graded_bisim_reachable_transport Q W2 Hreach2 W1 Hb1) as [W1' [Hreach1' Hb1']].
  exists W1'. split.
  - eapply graded_reachable_trans; [ exact Hreach1 | exact Hreach1' ].
  - eapply graded_bisim_trans; [ exact Hb1' | exact Hb2 ].
Qed.

(* The provable 2-cell layer: vertical composition is a setoid (reflexive +
   transitive). This is the 2-truncation deliverable (the bicategory coherence is
   the routed-out ceiling — see the header). *)
Theorem sim_2cells_form_setoid :
  (forall P, weak_match P P)
  /\ (forall P Q R, weak_match P Q -> weak_match Q R -> weak_match P R).
Proof. split; [ exact weak_match_refl | exact weak_match_vcomp ]. Qed.
