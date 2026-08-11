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
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
Import ListNotations.
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

(* ── The exact non-constructive step, ISOLATED and the reduction MECHANISED ──

   The converse of graded_bisim_refines_approximants — lifting the approximant
   limit (∀n graded_bisim_n n) to the coinductive gfp graded_bisim — is the one
   implication intuitionistic logic cannot supply. Rather than leave that as a
   claim, we (a) name the EXACT principle it needs and (b) PROVE the reduction:
   the gfp-completeness follows from that one principle, axiom-free.             *)

(* Approximant monotonicity: a deeper approximant refines a shallower one. *)
Lemma graded_bisim_n_monotone : forall n m S T,
  n <= m -> graded_bisim_n m S T -> graded_bisim_n n S T.
Proof.
  induction n as [| n' IH]; intros m S T Hle Hbis.
  - exact I.
  - destruct m as [| m']; [ inversion Hle | ].
    simpl in Hbis. destruct Hbis as [Hf Hb]. assert (Hn'm' : n' <= m') by lia.
    simpl. split.
    + intros g S' Hstep. destruct (Hf g S' Hstep) as [T' [HT Hbis']].
      exists T'. split; [ exact HT | apply (IH m' S' T' Hn'm' Hbis') ].
    + intros g T' Hstep. destruct (Hb g T' Hstep) as [S' [HS Hbis']].
      exists S'. split; [ exact HS | apply (IH m' S' T' Hn'm' Hbis') ].
Qed.

(* The isolated principle: over a FINITE list, a depth-indexed, downward-closed
   family that is inhabited at every depth has a SINGLE element inhabited at all
   depths. This is the infinite pigeonhole / fan-style stabilisation for an
   image-finite branching set — equivalent to a weak omniscience principle and NOT
   intuitionistically derivable (already false-without-omniscience at |L|=2). It is
   stated as a hypothesis, never as an Axiom, so nothing below leaks a non-
   constructive assumption into the global context. *)
Definition image_finite_stabilization : Prop :=
  forall (L : list signed_term) (Q : nat -> signed_term -> Prop),
    (forall n m T', n <= m -> Q m T' -> Q n T') ->
    (forall n, exists T', In T' L /\ Q n T') ->
    exists T', In T' L /\ forall n, Q n T'.

(* The reduction, MECHANISED: the coinductive (greatest-fixed-point) graded
   Hennessy–Milner completeness follows from image_finite_stabilization alone.
   Composed with graded_limit_adequacy this gives the FULL coinductive HM theorem
   modulo exactly one explicitly-named classical principle — the precise location
   of the constructive ceiling, proven rather than asserted. *)
Theorem graded_coinductive_completeness_modulo :
  image_finite_stabilization ->
  forall S T, (forall n, graded_bisim_n n S T) -> graded_bisim S T.
Proof.
  intros HStab. cofix CH. intros S T H. apply gbisim_intro.
  - intros g S' Hstep.
    assert (Hall : forall n, exists T', In T' (graded_succ T g) /\ graded_bisim_n n S' T').
    { intro n. specialize (H (Datatypes.S n)). simpl in H. destruct H as [Hf _].
      destruct (Hf g S' Hstep) as [T' [HT Hbis]].
      exists T'. split; [ apply graded_succ_complete; exact HT | exact Hbis ]. }
    destruct (HStab (graded_succ T g) (fun n T' => graded_bisim_n n S' T')
                    (fun n m T' Hnm HQ => graded_bisim_n_monotone n m S' T' Hnm HQ) Hall)
      as [T' [HinT' Hforall]].
    exists T'. split; [ apply graded_succ_sound; exact HinT' | apply CH; exact Hforall ].
  - intros g T' Hstep.
    assert (Hall : forall n, exists S', In S' (graded_succ S g) /\ graded_bisim_n n S' T').
    { intro n. specialize (H (Datatypes.S n)). simpl in H. destruct H as [_ Hb].
      destruct (Hb g T' Hstep) as [S' [HS Hbis]].
      exists S'. split; [ apply graded_succ_complete; exact HS | exact Hbis ]. }
    destruct (HStab (graded_succ S g) (fun n S' => graded_bisim_n n S' T')
                    (fun n m S' Hnm HQ => graded_bisim_n_monotone n m S' T' Hnm HQ) Hall)
      as [S' [HinS' Hforall]].
    exists S'. split; [ apply graded_succ_sound; exact HinS' | apply CH; exact Hforall ].
Qed.

(* The full coinductive graded Hennessy–Milner theorem, modulo the one isolated
   principle: graded-HML equivalence ⇒ coinductive graded bisimilarity. *)
Corollary graded_coinductive_hml_completeness_modulo :
  image_finite_stabilization ->
  forall S T, (forall phi, gsat S phi <-> gsat T phi) -> graded_bisim S T.
Proof.
  intros HStab S T Hhml. apply graded_coinductive_completeness_modulo; [ exact HStab | ].
  apply graded_limit_adequacy. exact Hhml.
Qed.
