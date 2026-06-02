(* ════════════════════════════════════════════════════════════════════════
   CACostDeterminism.v — native confluence + cost determinism (Stage 3c, cont.).

   Newman's lemma (Coquand 1994, constructive) on the funded fragment: local
   confluence + strong normalization ⇒ confluence ⇒ unique normal forms ⇒
   cost determinism. Native SN is conditional (CAStrongNormalization), so this is
   conditioned on HEREDITARY funding [HF S] — every state reachable from S is
   linearly funded. This is exactly the consensus-relevant class: an admitted
   deploy is funded (LinearLogicResources strict-reject), and its reductions stay
   within the funded supply, so cost determinism on HF is the consensus statement.
   Avoiding a funded-preservation lemma, HF is threaded through Newman's induction
   directly (any successor of an HF state is HF). Axiom-free.                   *)

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CATokenConservation.
From CostAccountedRho Require Import CAStrongNormalization.
From CostAccountedRho Require Import CAConfluence.

(* ── hereditary funding ─────────────────────────────────────────────────── *)

Definition HF (S : signed_term) : Prop :=
  forall S', ca_reachable S S' -> funded_linear S'.

Lemma HF_funded : forall S, HF S -> funded_linear S.
Proof. intros S H. apply H. apply car_refl. Qed.

Lemma HF_step : forall S S', HF S -> ca_step S S' -> HF S'.
Proof. intros S S' H Hstep S'' Hr. apply H. eapply car_step; eassumption. Qed.

(* ── confluence ─────────────────────────────────────────────────────────── *)

Definition confluent (S : signed_term) : Prop :=
  forall T1 T2, ca_reachable S T1 -> ca_reachable S T2 ->
    exists T, ca_reachable T1 T /\ ca_reachable T2 T.

(* Newman's lemma on the hereditarily-funded fragment. *)
Theorem newman_funded : forall S, HF S -> confluent S.
Proof.
  intro S. revert S.
  assert (Hwf : forall S, Acc funded_step_inv S -> HF S -> confluent S).
  { intros S Hacc. induction Hacc as [S _ IH_wf]. intro HFS.
    unfold confluent. intros T1 T2 Hreach1 Hreach2. revert T2 Hreach2.
    induction Hreach1 as [| S S1 T1 Hstep1 Htail1 IH_path1].
    - intros T2 Hreach2. exists T2. split; [ exact Hreach2 | apply car_refl ].
    - intros T2 Hreach2.
      induction Hreach2 as [| S S2 T2 Hstep2 Htail2 IH_path2].
      + exists T1. split; [ apply car_refl | eapply car_step; eassumption ].
      + destruct (ca_local_confluence S S1 S2 Hstep1 Hstep2)
          as [Heq | [S' [Hs1s' Hs2s']]].
        * subst S2.
          assert (Hconf_S1 : confluent S1).
          { apply IH_wf;
              [ split; [ apply HF_funded; exact HFS | exact Hstep1 ]
              | eapply HF_step; eassumption ]. }
          exact (Hconf_S1 T1 T2 Htail1 Htail2).
        * assert (Hconf_S1 : confluent S1).
          { apply IH_wf;
              [ split; [ apply HF_funded; exact HFS | exact Hstep1 ]
              | eapply HF_step; eassumption ]. }
          assert (Hconf_S2 : confluent S2).
          { apply IH_wf;
              [ split; [ apply HF_funded; exact HFS | exact Hstep2 ]
              | eapply HF_step; eassumption ]. }
          assert (Hr1 : ca_reachable S1 S') by (apply car_one; exact Hs1s').
          destruct (Hconf_S1 T1 S' Htail1 Hr1) as [D1 [HrT1D1 HrS'D1]].
          assert (Hr2 : ca_reachable S2 S') by (apply car_one; exact Hs2s').
          destruct (Hconf_S2 T2 S' Htail2 Hr2) as [D2 [HrT2D2 HrS'D2]].
          assert (HrS1D1 : ca_reachable S1 D1) by (eapply car_trans; eassumption).
          assert (HrS1D2 : ca_reachable S1 D2)
            by (eapply car_trans; [ exact Hr1 | exact HrS'D2 ]).
          destruct (Hconf_S1 D1 D2 HrS1D1 HrS1D2) as [D [HrD1D HrD2D]].
          exists D. split; eapply car_trans; eassumption. }
  intros S HFS. apply Hwf; [ apply ca_SN_funded | exact HFS ].
Qed.

(* ── normal-form uniqueness + cost determinism ──────────────────────────── *)

Theorem ca_normal_form_unique_funded : forall S T1 T2,
  HF S ->
  ca_reachable S T1 -> ca_terminal T1 ->
  ca_reachable S T2 -> ca_terminal T2 -> T1 = T2.
Proof.
  intros S T1 T2 HFS Hreach1 Hterm1 Hreach2 Hterm2.
  destruct (newman_funded S HFS T1 T2 Hreach1 Hreach2) as [T [HrT1 HrT2]].
  inversion HrT1 as [T1' | T1' Smid T' Hstep_mid Htail_mid]; subst.
  - inversion HrT2 as [T2' | T2' Smid2 T2'' Hstep_mid2 Htail_mid2]; subst.
    + reflexivity.
    + exfalso. exact (Hterm2 Smid2 Hstep_mid2).
  - exfalso. exact (Hterm1 Smid Hstep_mid).
Qed.

(* Headline: the total fuel at any terminal state reachable from a
   hereditarily-funded S is uniquely determined by S — cost determinism for the
   consensus-relevant (funded) class. *)
Theorem ca_cost_deterministic_funded : forall S T1 T2,
  HF S ->
  ca_reachable S T1 -> ca_terminal T1 ->
  ca_reachable S T2 -> ca_terminal T2 ->
  st_total_fuel T1 = st_total_fuel T2.
Proof.
  intros S T1 T2 HFS Hreach1 Hterm1 Hreach2 Hterm2.
  replace T2 with T1 by
    (exact (ca_normal_form_unique_funded S T1 T2 HFS Hreach1 Hterm1 Hreach2 Hterm2)).
  reflexivity.
Qed.
