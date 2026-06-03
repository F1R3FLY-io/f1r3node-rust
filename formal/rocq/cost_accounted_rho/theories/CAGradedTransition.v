(* ════════════════════════════════════════════════════════════════════════
   CAGradedTransition.v — the graded transition system (continued-gslt-cost-v2
   §"Graded adequacy", Stage 6 / CL6 basis).

   The monad paper grades the transitions of Cost(G) by the signature monoid: "a
   forced step is labelled by the signature(s) it consumes" (:783). Applying OSLF
   to this graded LTS yields the graded Hennessy–Milner logic with modalities
   ⟨a⟩_s. This module mechanizes the graded LTS itself: [graded_step S g S']
   relabels each native [ca_step] by the signature [g] it consumes (the gate's
   authority — atomic [s] for the single-token rules, the compound [s1 ∘ s2] for
   the compound rules). It is a faithful relabelling: forgetting the grade
   recovers [ca_step] (sound), and every [ca_step] carries a unique-shaped grade
   (complete). Phlogiston is thus the conserved grade — invariant under ≡,
   consumed along →, in the order the stack records. Axiom-free.               *)

From Stdlib Require Import Lists.List.
Import ListNotations.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CABinding.
From CostAccountedRho Require Import CAReduction.

(* The graded transition relation: [ca_step] relabelled by the consumed grade. *)
Inductive graded_step : signed_term -> sig -> signed_term -> Prop :=
  | g_rule1 : forall x T U s t,
      graded_step
        (STPar (STSigned (CPPar (CPInput x T) (CPOutput x U)) s) (STStack (TGate s t)))
        s
        (STPar (subst_st T 0 (CQuote U)) (STStack t))
  | g_rule2 : forall x T U s1 s2 t1 t2,
      graded_step
        (STPar (STPar (STSigned (CPPar (CPInput x T) (CPOutput x U)) (SAnd s1 s2))
                      (STStack (TGate s1 t1)))
               (STStack (TGate s2 t2)))
        (SAnd s1 s2)
        (STPar (STPar (subst_st T 0 (CQuote U)) (STStack t1)) (STStack t2))
  | g_rule3 : forall x T U s1 s2 t,
      graded_step
        (STPar (STSigned (CPPar (CPInput x T) (CPOutput x U)) (SAnd s1 s2))
               (STStack (TGate (SAnd s1 s2) t)))
        (SAnd s1 s2)
        (STPar (subst_st T 0 (CQuote U)) (STStack t))
  | g_rule4 : forall x T U s1 s2 t,
      graded_step
        (STPar (STPar (STSigned (CPInput x T) s1) (STSigned (CPOutput x U) s2))
               (STStack (TGate (SAnd s1 s2) t)))
        (SAnd s1 s2)
        (STPar (subst_st T 0 (CQuote U)) (STStack t))
  | g_rule5 : forall x T U s1 s2 t1 t2,
      graded_step
        (STPar (STPar (STPar (STSigned (CPInput x T) s1) (STSigned (CPOutput x U) s2))
                      (STStack (TGate s1 t1)))
               (STStack (TGate s2 t2)))
        (SAnd s1 s2)
        (STPar (STPar (subst_st T 0 (CQuote U)) (STStack t1)) (STStack t2))
  | g_join1 : forall xs Us T s t snds,
      snds = join_sends xs Us ->
      length xs = length Us ->
      Forall closed_st Us ->
      graded_step
        (STPar (STSigned (CPPar (CPJoin xs T) snds) s) (STStack (TGate s t)))
        s
        (STPar (subst_st_many T Us) (STStack t))
  | g_par_l : forall S1 g S1' S2,
      graded_step S1 g S1' -> graded_step (STPar S1 S2) g (STPar S1' S2)
  | g_par_r : forall S1 g S2 S2',
      graded_step S2 g S2' -> graded_step (STPar S1 S2) g (STPar S1 S2').

(* The grading is a faithful relabelling of ca_step. *)
Theorem graded_step_sound : forall S g S', graded_step S g S' -> ca_step S S'.
Proof.
  intros S g S' H. induction H.
  - apply ca_rule1.
  - apply ca_rule2.
  - apply ca_rule3.
  - apply ca_rule4.
  - apply ca_rule5.
  - apply ca_join1; assumption.
  - apply ca_par_l; assumption.
  - apply ca_par_r; assumption.
Qed.

Theorem graded_step_complete : forall S S', ca_step S S' -> exists g, graded_step S g S'.
Proof.
  intros S S' H. induction H.
  - eexists; apply g_rule1.
  - eexists; apply g_rule2.
  - eexists; apply g_rule3.
  - eexists; apply g_rule4.
  - eexists; apply g_rule5.
  - eexists; apply g_join1; eassumption.
  - destruct IHca_step as [g Hg]. exists g. apply g_par_l; assumption.
  - destruct IHca_step as [g Hg]. exists g. apply g_par_r; assumption.
Qed.

(* The graded LTS and the bare LTS have the same transitions (up to grade): a
   step exists iff a graded step exists. *)
Theorem graded_iff_step : forall S S',
  ca_step S S' <-> exists g, graded_step S g S'.
Proof.
  intros S S'. split.
  - apply graded_step_complete.
  - intros [g Hg]. exact (graded_step_sound S g S' Hg).
Qed.

(* A graded modal logic (graded Hennessy–Milner) over the graded LTS: the
   diamond ⟨g⟩φ holds at S when S can take a g-graded step to a state at φ. *)
Inductive GForm : Type :=
  | GTrue  : GForm
  | GAnd   : GForm -> GForm -> GForm
  | GNot   : GForm -> GForm
  | GDia   : sig -> GForm -> GForm.       (* ⟨g⟩φ — a g-graded transition to φ *)

Fixpoint gsat (S : signed_term) (phi : GForm) : Prop :=
  match phi with
  | GTrue       => True
  | GAnd p q    => gsat S p /\ gsat S q
  | GNot p      => ~ gsat S p
  | GDia g p    => exists S', graded_step S g S' /\ gsat S' p
  end.

(* Soundness of the graded modality against the graded LTS: ⟨g⟩φ is witnessed by
   exactly a g-graded transition (the modality reads the grade off the step). *)
Theorem gdia_sound : forall S g phi,
  gsat S (GDia g phi) -> exists S', graded_step S g S' /\ gsat S' phi.
Proof. intros S g phi H. exact H. Qed.

Theorem gdia_complete : forall S g phi S',
  graded_step S g S' -> gsat S' phi -> gsat S (GDia g phi).
Proof. intros S g phi S' Hstep Hphi. exists S'. split; assumption. Qed.
