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
      graded_step
        (STPar (STSigned (CPPar (CPJoin xs T) snds) s) (STStack (TGate s t)))
        s
        (STPar (subst_st_many T Us) (STStack t))
  | g_join2 : forall xs Us ts T s1 t snds,
      snds = signed_sends xs Us ts ->
      length xs = length Us ->
      length xs = length ts ->
      graded_step
        (STPar (STPar (STSigned (CPJoin xs T) s1) snds)
               (STStack (TGate (join_token_key s1 ts) t)))
        (join_token_key s1 ts)
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
  - apply ca_join2; assumption.
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
  - eexists; apply g_join2; eassumption.
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

Theorem sealed_terms_have_no_internal_graded_step : forall body authority grade target,
  ~ graded_step (STSigned body authority) grade target.
Proof.
  intros body authority grade target Hstep. inversion Hstep.
Qed.

Theorem isolated_token_stacks_have_no_graded_step : forall tokens grade target,
  ~ graded_step (STStack tokens) grade target.
Proof.
  intros tokens grade target Hstep. inversion Hstep.
Qed.

Theorem lollipop_outer_step_preserves_continuation_seal :
  forall channel body payload outer inner remaining,
    graded_step
      (STPar
        (STSigned (CPPar (CPInput channel (STSigned body inner))
          (CPOutput channel payload)) outer)
        (STStack (TGate outer remaining)))
      outer
      (STPar (STSigned (subst_caproc body 0 (CQuote payload)) inner)
        (STStack remaining)).
Proof.
  intros. apply g_rule1.
Qed.

Theorem split_lollipop_step_preserves_continuation_seal :
  forall channel body payload outer inner sender remaining,
    graded_step
      (STPar
        (STPar (STSigned (CPInput channel (STSigned body inner)) outer)
          (STSigned (CPOutput channel payload) sender))
        (STStack (TGate (SAnd outer sender) remaining)))
      (SAnd outer sender)
      (STPar (STSigned (subst_caproc body 0 (CQuote payload)) inner)
        (STStack remaining)).
Proof.
  intros. apply g_rule4.
Qed.

Theorem split_tokens_preserve_continuation_seal :
  forall channel body payload outer inner sender outer_tail sender_tail,
    graded_step
      (STPar
        (STPar
          (STPar (STSigned (CPInput channel (STSigned body inner)) outer)
            (STSigned (CPOutput channel payload) sender))
          (STStack (TGate outer outer_tail)))
        (STStack (TGate sender sender_tail)))
      (SAnd outer sender)
      (STPar
        (STPar (STSigned (subst_caproc body 0 (CQuote payload)) inner)
          (STStack outer_tail))
        (STStack sender_tail)).
Proof.
  intros. apply g_rule5.
Qed.

Theorem whole_redex_step_has_exact_grade_and_residual :
  forall channel continuation payload outer remaining grade target,
    graded_step
      (STPar (STSigned (CPPar (CPInput channel continuation)
        (CPOutput channel payload)) outer) (STStack (TGate outer remaining)))
      grade target ->
    grade = outer /\
    target = STPar (subst_st continuation 0 (CQuote payload)) (STStack remaining).
Proof.
  intros channel continuation payload outer remaining grade target Hstep.
  inversion Hstep; subst; try (split; reflexivity).
  - exfalso. eapply sealed_terms_have_no_internal_graded_step. eassumption.
  - exfalso. eapply isolated_token_stacks_have_no_graded_step. eassumption.
Qed.

Theorem waiting_lollipop_cannot_force_its_continuation :
  forall channel body outer inner tokens grade target,
    ~ graded_step
      (STPar (STSigned (CPInput channel (STSigned body inner)) outer) (STStack tokens))
      grade target.
Proof.
  intros channel body outer inner tokens grade target Hstep.
  inversion Hstep; subst.
  - eapply sealed_terms_have_no_internal_graded_step. eassumption.
  - eapply isolated_token_stacks_have_no_graded_step. eassumption.
Qed.

Theorem independent_graded_steps_have_both_orders :
  forall first first_grade first_result second second_grade second_result,
    graded_step first first_grade first_result ->
    graded_step second second_grade second_result ->
    graded_step (STPar first second) first_grade (STPar first_result second) /\
    graded_step (STPar first_result second) second_grade (STPar first_result second_result) /\
    graded_step (STPar first second) second_grade (STPar first second_result) /\
    graded_step (STPar first second_result) first_grade (STPar first_result second_result).
Proof.
  intros first first_grade first_result second second_grade second_result Hfirst Hsecond.
  repeat split; [apply g_par_l | apply g_par_r | apply g_par_r | apply g_par_l]; assumption.
Qed.

Fixpoint parallel_terms (terms : list signed_term) : signed_term :=
  match terms with
  | [] => STSigned CPNil SUnit
  | term :: rest => STPar term (parallel_terms rest)
  end.

Theorem graded_step_at_any_parallel_position :
  forall prefix source suffix grade target,
    graded_step source grade target ->
    graded_step (parallel_terms (prefix ++ source :: suffix)) grade
      (parallel_terms (prefix ++ target :: suffix)).
Proof.
  induction prefix as [| head rest IH]; intros source suffix grade target Hstep; simpl.
  - now apply g_par_l.
  - apply g_par_r. now apply IH.
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
