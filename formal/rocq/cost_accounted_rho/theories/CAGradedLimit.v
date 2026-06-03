(* ════════════════════════════════════════════════════════════════════════
   CAGradedLimit.v — the FULL (non-stratified) constructive graded
   Hennessy–Milner theorem, and the coinductive bridge (CL6, capstone of the
   graded-adequacy stack).

   CAGradedCompleteness gives the adequacy at each finite modal depth n. This
   module quantifies n away to obtain the full, non-stratified HM theorem with
   bisimilarity taken as the approximant LIMIT — the strongest statement
   intuitionistic logic permits here:

       (∀n, graded_bisim_n n S T)  ⟺  (∀φ, gsat S φ ↔ gsat T φ).

   It also bridges to the coinductive greatest-fixed-point bisimulation
   (CAGradedAdequacy.graded_bisim): the coinductive relation REFINES every finite
   approximant, hence (through the limit) implies graded-HML equivalence. This
   pins down EXACTLY one implication that is not intuitionistically available —
   the converse `(∀n graded_bisim_n n) → graded_bisim`, i.e. lifting the
   approximant limit to the coinductive gfp. For an image-finite LTS that step is
   the infinite pigeonhole over the finite successor set, equivalent to a weak
   omniscience principle (Markov-/LPO-flavoured), and so cannot be discharged
   without an axiom that would break the closed-under-global-context gate. It is
   therefore stated nowhere and assumed nowhere; everything provable WITHOUT it is
   proved here, axiom-free. This is the precise constructive ceiling, exhibited as
   theorems rather than left as a remark.                                        *)

From Stdlib Require Import Arith.Arith.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CAGradedTransition.
From CostAccountedRho Require Import CAGradedAdequacy.
From CostAccountedRho Require Import CAGradedImageFinite.
From CostAccountedRho Require Import CAGradedSuccPairs.
From CostAccountedRho Require Import CAGradedCompleteness.

(* The coinductive graded bisimulation refines every finite approximant. *)
Lemma graded_bisim_refines_approximants : forall n S T,
  graded_bisim S T -> graded_bisim_n n S T.
Proof.
  induction n as [| n' IH]; intros S T H; simpl.
  - exact I.
  - inversion H as [S0 T0 Hf Hb Heq1 Heq2]; subst. split.
    + intros g S' Hstep. destruct (Hf g S' Hstep) as [T' [HT Hb']].
      exists T'. split; [ exact HT | apply IH; exact Hb' ].
    + intros g T' Hstep. destruct (Hb g T' Hstep) as [S' [HS Hb']].
      exists S'. split; [ exact HS | apply IH; exact Hb' ].
Qed.

(* The FULL constructive graded Hennessy–Milner theorem: approximant-limit graded
   bisimilarity is EXACTLY graded-HML equivalence (every formula, no depth bound). *)
Theorem graded_limit_adequacy : forall S T,
  (forall n, graded_bisim_n n S T) <-> (forall phi, gsat S phi <-> gsat T phi).
Proof.
  intros S T. split.
  - intros H phi.
    apply (graded_bisim_n_sound phi (gdepth phi) S T (le_n (gdepth phi)) (H (gdepth phi))).
  - intros H n.
    apply (proj2 (graded_finitary_adequacy n S T)). intros phi _. apply H.
Qed.

(* Hence the coinductive graded bisimulation is graded-HML sound THROUGH the limit:
   graded_bisim ⊆ approximant-limit = HML-equivalence — all constructive. The
   converse (HML-equivalence ⇒ the coinductive gfp) is the single non-intuitionistic
   step (see header); it is neither used nor assumed anywhere. *)
Corollary graded_bisim_implies_hml : forall S T,
  graded_bisim S T -> (forall phi, gsat S phi <-> gsat T phi).
Proof.
  intros S T H. apply graded_limit_adequacy. intro n.
  apply graded_bisim_refines_approximants. exact H.
Qed.
