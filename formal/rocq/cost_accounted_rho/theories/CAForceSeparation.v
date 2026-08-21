(* ════════════════════════════════════════════════════════════════════════
   CAForceSeparation.v — the force-point over-gating obstruction, PROVEN
   (docs §3a, now a theorem rather than a documented remark).

   docs/.../native-faithfulness-design.md §3a records that the native gate
   translation OVER-GATES at force positions: at a force *x of a received signed
   term U, the source dequote st_to_proc U strips the gate and runs the content,
   whereas the translation St U is a stuck gated receiver. This module turns that
   obstruction into a SETTLED mathematical fact:

     • gated_translation_stuck — for EVERY signature s, st_tr (STSigned P s) is a
       lone PInput, hence has NO rho_step (PInput_alone_stuck): the gated
       translation of a signed term is operationally STUCK as a standalone term.
     • stuck_not_bisim_stepping — a stuck process is never strongly bisimilar to
       one that can step (immediate from the backward simulation clause of bisim).
     • ca_force_overgating_separation — therefore, whenever the dequoted source
       force Pt (st_to_proc (STSigned P s)) = Pt P can make progress, the gated
       translation St (STSigned P s) is NOT strongly bisimilar to it.

   Consequence: the "full metered-translation strong bisimulation at force points"
   is FALSE for the naive translation — not an open task but a disproven
   strengthening. A force-faithful translation would require the force-cashing /
   two-level quote refinement (§3a), which is a different translation and outside
   the current spec's committed scope (neither paper asserts this bisimulation;
   the spec's faithfulness is ca_translation_progresses + the unit-grade Adjunction
   II retraction + ca_single_gate_bisimilar, all proven). Axiom-free.            *)

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import RhoReduction.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CATranslation.
From CostAccountedRho Require Import Bisimulation.

Section CAForceSeparationSec.

Variable hash_process : list bool -> proc.
Variable ground_process : list bool -> proc.

Local Notation Pt := (p_tr hash_process ground_process).
Local Notation St := (st_tr hash_process ground_process).

(* A stuck process is never strongly bisimilar to one that can step: the backward
   clause of bisim would force the stuck side to match the step. *)
Lemma stuck_not_bisim_stepping : forall X Y,
  (~ exists X', rho_step X X') -> (exists Y', rho_step Y Y') -> ~ bisim X Y.
Proof.
  intros X Y Hstuck [Y' HY] Hbisim.
  inversion Hbisim as [X0 Y0 Hf Hb HeqX HeqY]; subst.
  destruct (Hb Y' HY) as [X' [HX _]].
  apply Hstuck. exists X'. exact HX.
Qed.

(* The gated translation of a signed term is a lone receiver — operationally STUCK
   as a standalone process, for EVERY signature (atomic or compound SAnd). *)
Lemma gated_translation_stuck : forall P s,
  ~ exists W, rho_step (St (STSigned P s)) W.
Proof.
  intros P s [W HW]. destruct s; simpl in HW; eapply PInput_alone_stuck; exact HW.
Qed.

(* §3a, proven: when the dequoted source force can run, the naive gated translation
   (stuck) is NOT strongly bisimilar to it — the translation provably over-gates at
   force points. *)
Theorem ca_force_overgating_separation : forall P s,
  (exists R, rho_step (Pt (st_to_proc (STSigned P s))) R) ->
  ~ bisim (St (STSigned P s)) (Pt (st_to_proc (STSigned P s))).
Proof.
  intros P s Hstep.
  apply stuck_not_bisim_stepping; [ apply gated_translation_stuck | exact Hstep ].
Qed.

(* The separation is NON-VACUOUS: a concrete signed term whose dequoted force is a
   matching COMM redex (so Pt of it steps via rs_comm) while the gated translation
   is stuck — an actual non-bisimilarity, not a vacuously-satisfied implication. *)
Theorem ca_force_overgating_nonvacuous : exists P s,
  (exists R, rho_step (Pt (st_to_proc (STSigned P s))) R)
  /\ ~ bisim (St (STSigned P s)) (Pt (st_to_proc (STSigned P s))).
Proof.
  exists (CPPar (CPInput (CNVar 0) (STStack TUnit)) (CPOutput (CNVar 0) (STStack TUnit))), SUnit.
  assert (Hstep : exists R,
      rho_step (Pt (st_to_proc (STSigned
        (CPPar (CPInput (CNVar 0) (STStack TUnit)) (CPOutput (CNVar 0) (STStack TUnit))) SUnit))) R).
  { eexists. simpl. apply rs_comm. }
  split; [ exact Hstep | apply ca_force_overgating_separation; exact Hstep ].
Qed.

End CAForceSeparationSec.
