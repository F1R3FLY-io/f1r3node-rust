(* ════════════════════════════════════════════════════════════════════════
   CAUntypedLambdaCI.v — the untyped-λ instance as a SECOND object under the
   cost endofunctor (DR-25).

   CAUntypedLambda.v mechanizes the rigid-K λ instance at the operational level
   (R1-only, funded run-bound, unconditional funded SN, the Ω halting seam,
   erasure to pure β). This module connects it to the ABSTRACT layer: it
   exhibits the metered λ calculus as an object [Lambda_ciGSLT] of the
   continued-interactive GSLT category [CICat] (CACategory), so the cost
   endofunctor [CostCI : Functor CICat CICat] (CACostFunctorCI) applies to it
   exactly as to [Rho_ciGSLT]. Cost's genericity is then witnessed by TWO
   concrete objects — an AC contact (rho ⇒ five rules) and a rigid contact
   (λ ⇒ R1 only) — not by the rho instance alone.

   [lca_graded_step] is the operational [lca_step] labelled by the consumed gate
   signature (the grade); [lca_graded_step_sound] / [lca_step_gradable] show the
   two relations are inter-derivable, so the categorical [cstep] is genuinely the
   metered λ reduction. Behavioural equivalence is syntactic equality (a valid
   bisimulation, discharging the CIObj refl/sym/trans obligations). Axiom-free. *)

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CategoryInterface.
From CostAccountedRho Require Import CACategory.
From CostAccountedRho Require Import CACostFunctorCI.
From CostAccountedRho Require Import CAUntypedLambda.

(* The gate-graded refinement of [lca_step]: the label is the consumed gate. *)
Inductive lca_graded_step : lsys -> sig -> lsys -> Prop :=
  | lcg_beta : forall (M N : lterm) (s : sig) (t : token),
      lca_graded_step
        (LSPar (LWrap (LApp (LAbs M) N) s) (LStack (TGate s t))) s
        (LSPar (LWrap (lsubst M 0 N) s)    (LStack t))
  | lcg_par_l : forall S1 g S1' S2,
      lca_graded_step S1 g S1' -> lca_graded_step (LSPar S1 S2) g (LSPar S1' S2)
  | lcg_par_r : forall S1 g S2 S2',
      lca_graded_step S2 g S2' -> lca_graded_step (LSPar S1 S2) g (LSPar S1 S2').

(* The graded step refines the plain step (forget the grade). *)
Theorem lca_graded_step_sound : forall S g S',
  lca_graded_step S g S' -> lca_step S S'.
Proof.
  intros S g S' H.
  induction H as [M N s t | S1 g1 S1' S2 Hsub IH | S1 g1 S2 S2' Hsub IH].
  - apply lca_beta_r1.
  - apply lca_par_l. exact IH.
  - apply lca_par_r. exact IH.
Qed.

(* Conversely every plain step carries a grade (the consumed gate). *)
Theorem lca_step_gradable : forall S S',
  lca_step S S' -> exists g, lca_graded_step S g S'.
Proof.
  intros S S' H.
  induction H as [M N s t | S1 S1' S2 Hsub IH | S1 S2 S2' Hsub IH].
  - eexists. apply lcg_beta.
  - destruct IH as [g Hg]. exists g. apply lcg_par_l. exact Hg.
  - destruct IH as [g Hg]. exists g. apply lcg_par_r. exact Hg.
Qed.

(* The metered untyped-λ calculus as an object of the ciGSLT category. *)
Definition Lambda_ciGSLT : CIObj :=
  {| carrier := lsys;
     cstep   := lca_graded_step;
     cbisim  := @eq lsys;
     reachable_sig := fun _ => True;
     cstep_reachable_sig := fun _ _ _ _ => I;
     cbisim_refl  := @eq_refl lsys;
     cbisim_sym   := @eq_sym lsys;
     cbisim_trans := @eq_trans lsys |}.

(* Cost applies to the λ object exactly as to the rho object: a second concrete
   object of the cost-endofunctor category (cf. CACostFunctorCI.cost_ci_nonvacuous
   for Rho_ciGSLT). This is the abstract-layer closure of the genericity claim —
   genericity grounded by two instances, not one. *)
Theorem Lambda_ciGSLT_nonvacuous : exists G : Obj CICat, G = CostObj Lambda_ciGSLT.
Proof. exists (CostObj Lambda_ciGSLT). reflexivity. Qed.
