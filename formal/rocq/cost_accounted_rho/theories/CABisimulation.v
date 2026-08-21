(* ════════════════════════════════════════════════════════════════════════
   CABisimulation.v — native single-gate strong bisimulation (Stage 4b).

   The native analogue of Bisimulation.translation_strong_bisimilar_generic: when
   a native signed term {P}_s is fired by a co-present unit token, the post-gate
   residue is STRONGLY BISIMILAR to the released body translation p_tr P. This is
   exactly the strength the old (bare-proc-continuation) model establishes,
   ported to the four-sort grammar.

   It is the bisimulation that holds CLEANLY: a single gate fires (gate-unwrap
   COMM + the inert unit-token residue), and there is NO inner substitution of a
   signed term into a force position (the *x dereference) — so the force-collapse obstruction
   (docs/theory/cost-accounting-native-faithfulness-design.md §3a), which blocks a
   strong bisimulation across an arbitrary multi-COMM ca_step, does not arise
   here. The residue PPar (p_tr P) (T_tr TUnit) is bisimilar to p_tr P because the
   unit-token image is the stuck inert PNil (multi_stuck_residue_bisim). The
   operational forward progress for the full ca_step is ca_translation_progresses
   (CATranslationFaithfulness). Axiom-free modulo the audited hash/ground
   Section hypotheses.                                                          *)

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import RhoReduction.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CATranslation.
From CostAccountedRho Require Import CATranslationFaithfulness.
From CostAccountedRho Require Import Bisimulation.

Section CABisimulationSec.

Variable hash_process : list bool -> proc.
Hypothesis hash_process_injective :
  forall b1 b2, hash_process b1 = hash_process b2 -> b1 = b2.
Hypothesis hash_process_closed : forall bs, closed_proc (hash_process bs).
Variable ground_process : list bool -> proc.
Hypothesis ground_process_injective :
  forall b1 b2, ground_process b1 = ground_process b2 -> b1 = b2.
Hypothesis ground_process_closed : forall bs, closed_proc (ground_process bs).
Hypothesis ground_hash_disjoint :
  forall b1 b2, ground_process b1 <> hash_process b2.

Local Notation Nt := (N_tr hash_process ground_process).
Local Notation Pt := (p_tr hash_process ground_process).
Local Notation Tt := (T_tr hash_process ground_process).
Local Notation St := (st_tr hash_process ground_process).

(* The native single-gate strong bisimulation (atomic signature). Firing the gate
   of {P}_s against a co-present unit token reaches a residue strongly bisimilar
   to the released body p_tr P. *)
Theorem ca_single_gate_bisimilar : forall P s,
  (forall a b, s <> SAnd a b) ->
  exists W,
    rho_reachable (PPar (St (STSigned P s)) (Tt (TGate s TUnit))) W
    /\ bisim W (Pt P).
Proof.
  intros P s Hns.
  exists (PPar (Pt P) (Tt TUnit)). split.
  - assert (fire : forall n,
      rho_reachable
        (PPar (PInput n (PPar (lift_proc 1 0 (Pt P)) (PDeref (NVar 0)))) (POutput n (Tt TUnit)))
        (PPar (Pt P) (Tt TUnit))).
    { intro n. eapply rr_step. { apply rs_comm. }
      rewrite gate_body_subst. apply rr_refl. }
    destruct s as [| bs | bs | a b]; try (exfalso; eapply Hns; reflexivity); apply fire.
  - apply multi_stuck_residue_bisim. reflexivity.
Qed.

End CABisimulationSec.
