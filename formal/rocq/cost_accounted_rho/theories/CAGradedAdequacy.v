(* ════════════════════════════════════════════════════════════════════════
   CAGradedAdequacy.v — graded Hennessy–Milner adequacy (CL6).

   The graded LTS (CAGradedTransition.graded_step) and its graded HML
   (GForm/gsat) support a graded bisimulation [graded_bisim] and the ADEQUACY of
   the logic: graded-bisimilar states satisfy the same graded-HML formulae
   (soundness of the logic for the bisimulation). Built constructively — NO
   Classical / FunctionalExtensionality / Choice (per the session mandate).

   This is about the native ca_step's graded transitions directly, so it is
   independent of the translation (it is NOT affected by the force-point limit of
   the strong translation bisimulation). The completeness direction (same
   formulae ⇒ bisimilar) is the standard image-finite Hennessy–Milner theorem;
   the soundness direction proved here is the half that holds without any
   image-finiteness hypothesis. Axiom-free.                                     *)

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CAGradedTransition.

(* Graded (strong) bisimulation over the signature-graded LTS. *)
CoInductive graded_bisim : signed_term -> signed_term -> Prop :=
  | gbisim_intro : forall S T,
      (forall g S', graded_step S g S' -> exists T', graded_step T g T' /\ graded_bisim S' T') ->
      (forall g T', graded_step T g T' -> exists S', graded_step S g S' /\ graded_bisim S' T') ->
      graded_bisim S T.

Lemma graded_bisim_refl : forall S, graded_bisim S S.
Proof.
  cofix CH. intro S. apply gbisim_intro.
  - intros g S' Hstep. exists S'. split; [ exact Hstep | apply CH ].
  - intros g T' Hstep. exists T'. split; [ exact Hstep | apply CH ].
Qed.

Lemma graded_bisim_sym : forall S T, graded_bisim S T -> graded_bisim T S.
Proof.
  cofix CH. intros S T H. inversion H as [S0 T0 Hf Hb HeqS HeqT]; subst.
  apply gbisim_intro.
  - intros g T' Hstep. destruct (Hb g T' Hstep) as [S' [HS Hbis]].
    exists S'. split; [ exact HS | apply CH; exact Hbis ].
  - intros g S' Hstep. destruct (Hf g S' Hstep) as [T' [HT Hbis]].
    exists T'. split; [ exact HT | apply CH; exact Hbis ].
Qed.

(* ── Adequacy (soundness): graded-bisimilar states are graded-HML-equivalent ── *)

Theorem graded_adequacy_sound : forall S T,
  graded_bisim S T -> forall phi, gsat S phi <-> gsat T phi.
Proof.
  intros S T Hbis phi. revert S T Hbis.
  induction phi as [ | p IHp q IHq | p IHp | g p IHp ]; intros S T Hbis.
  - (* GTrue *) split; intro; exact I.
  - (* GAnd *) simpl. split; intros [Hp Hq].
    + split; [ apply (IHp S T Hbis); exact Hp | apply (IHq S T Hbis); exact Hq ].
    + split; [ apply (IHp S T Hbis); exact Hp | apply (IHq S T Hbis); exact Hq ].
  - (* GNot *) simpl. split; intros Hn Hcontra; apply Hn.
    + apply (IHp S T Hbis); exact Hcontra.
    + apply (IHp S T Hbis); exact Hcontra.
  - (* GDia g p *) simpl. inversion Hbis as [S0 T0 Hf Hb HeqS HeqT]; subst.
    split.
    + intros [S' [Hstep Hsat]].
      destruct (Hf g S' Hstep) as [T' [HT Hbis']].
      exists T'. split; [ exact HT | apply (IHp S' T' Hbis'); exact Hsat ].
    + intros [T' [Hstep Hsat]].
      destruct (Hb g T' Hstep) as [S' [HS Hbis']].
      exists S'. split; [ exact HS | apply (IHp S' T' Hbis'); exact Hsat ].
Qed.

(* Contrapositive packaging: a distinguishing graded-HML formula refutes
   graded bisimilarity (the logic detects all bisimulation differences it can
   express). *)
Corollary graded_hml_distinguishes : forall S T phi,
  gsat S phi -> ~ gsat T phi -> ~ graded_bisim S T.
Proof.
  intros S T phi HS HT Hbis.
  apply HT. apply (graded_adequacy_sound S T Hbis phi). exact HS.
Qed.
