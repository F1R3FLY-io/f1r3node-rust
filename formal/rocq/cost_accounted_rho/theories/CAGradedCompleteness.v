(* ════════════════════════════════════════════════════════════════════════
   CAGradedCompleteness.v — constructive graded Hennessy–Milner completeness, at
   every finite modal depth (CL6).

   The converse of CAGradedAdequacy.graded_adequacy_sound, mechanised
   CONSTRUCTIVELY (no Classical / funext / choice, per the session mandate): the
   graded HML CHARACTERISES the depth-stratified graded bisimulation. The dichotomy
     ∀ n S T, graded_bisim_n n S T  ∨  (∃ φ of modal-depth ≤ n distinguishing them)
   is built by induction on n, iterating the image-finite successor enumeration
   (CAGradedSuccPairs.graded_succ_all) and forming the finite conjunction of
   distinguishing sub-formulae. The non-constructive obstacle (extracting a
   distinguishing formula from ¬logically-equivalent) is removed by image-
   finiteness + the bounded depth. Hence the finitary adequacy
     graded_bisim_n n S T  ⟺  S, T agree on all graded-HML formulae of depth ≤ n.

   This is the strongest adequacy provable WITHOUT a non-intuitionistic principle:
   the full coinductive completeness (∀n. graded_bisim_n n ⇒ graded_bisim) needs
   the constructive infinite pigeonhole over the finite successor set, which is
   not intuitionistically derivable (it implies a weak Markov-style principle).
   The finitary adequacy below is the constructive content. Axiom-free.        *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
Import ListNotations.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CAGradedTransition.
From CostAccountedRho Require Import CAGradedImageFinite.
From CostAccountedRho Require Import CAGradedSuccPairs.

(* Modal depth: only the diamond ⟨g⟩ increases depth; ∧/¬ do not. *)
Fixpoint gdepth (phi : GForm) : nat :=
  match phi with
  | GTrue     => 0
  | GAnd p q  => max (gdepth p) (gdepth q)
  | GNot p    => gdepth p
  | GDia _ p  => S (gdepth p)
  end.

(* Depth-stratified graded bisimulation. *)
Fixpoint graded_bisim_n (n : nat) (S T : signed_term) : Prop :=
  match n with
  | 0 => True
  | S n' =>
      (forall g S', graded_step S g S' -> exists T', graded_step T g T' /\ graded_bisim_n n' S' T')
      /\ (forall g T', graded_step T g T' -> exists S', graded_step S g S' /\ graded_bisim_n n' S' T')
  end.

(* Depth-n bisimulation is symmetric (used to re-orient the backward clause). *)
Lemma graded_bisim_n_sym : forall n S T, graded_bisim_n n S T -> graded_bisim_n n T S.
Proof.
  induction n as [| n' IH]; intros S T H; simpl in *.
  - exact I.
  - destruct H as [Hfwd Hbwd]. split.
    + intros g T' Hstep. destruct (Hbwd g T' Hstep) as [S' [HS Hbis]].
      exists S'. split; [ exact HS | apply IH; exact Hbis ].
    + intros g S' Hstep. destruct (Hfwd g S' Hstep) as [T' [HT Hbis]].
      exists T'. split; [ exact HT | apply IH; exact Hbis ].
Qed.

(* ── Distinguish S' from a finite candidate list, or find a bisimilar one ─── *)
Lemma distinguish_list : forall n',
  (forall S T, graded_bisim_n n' S T \/ (exists phi, gdepth phi <= n' /\ gsat S phi /\ ~ gsat T phi)) ->
  forall S' Ts,
  (exists T', In T' Ts /\ graded_bisim_n n' S' T') \/
  (exists Phi, gdepth Phi <= n' /\ gsat S' Phi /\ (forall T', In T' Ts -> ~ gsat T' Phi)).
Proof.
  intros n' dich S' Ts. induction Ts as [| T0 rest IH].
  - right. exists GTrue. split; [ simpl; lia | split; [ exact I | intros T' [] ] ].
  - destruct (dich S' T0) as [Hbis | [phi0 [Hd0 [Hs0 Ht0]]]].
    + left. exists T0. split; [ left; reflexivity | exact Hbis ].
    + destruct IH as [[T' [Hin Hbis]] | [Phi' [Hd' [Hs' Ht']]]].
      * left. exists T'. split; [ right; exact Hin | exact Hbis ].
      * right. exists (GAnd phi0 Phi'). split.
        -- simpl. apply Nat.max_lub; assumption.
        -- split.
           ++ simpl. split; [ exact Hs0 | exact Hs' ].
           ++ intros T' Hin Hsat. simpl in Hsat. destruct Hsat as [HsatA HsatB].
              destruct Hin as [Heq | Hin'].
              ** subst T'. apply Ht0; exact HsatA.
              ** apply (Ht' T' Hin'); exact HsatB.
Qed.

(* ── Check every successor pair in a list, or expose a distinguishing one ─── *)
Lemma forward_check : forall n',
  (forall S T, graded_bisim_n n' S T \/ (exists phi, gdepth phi <= n' /\ gsat S phi /\ ~ gsat T phi)) ->
  forall T Ps,
  (forall g S', In (g, S') Ps -> exists T', graded_step T g T' /\ graded_bisim_n n' S' T') \/
  (exists g S' Phi, In (g, S') Ps /\ gdepth Phi <= n' /\ gsat S' Phi
                    /\ (forall T', graded_step T g T' -> ~ gsat T' Phi)).
Proof.
  intros n' dich T Ps. induction Ps as [| [g S'] rest IH].
  - left. intros g S' [].
  - destruct (distinguish_list n' dich S' (graded_succ T g)) as [[T' [Hin Hbis]] | [Phi [Hd [Hs Ht]]]].
    + (* head matches *)
      destruct IH as [Hall | [g2 [S2 [Phi2 [Hin2 Hrest]]]]].
      * left. intros g0 S0 [Heq | Hin0].
        -- inversion Heq; subst. exists T'. split; [ apply graded_succ_sound; exact Hin | exact Hbis ].
        -- apply Hall; exact Hin0.
      * right. exists g2, S2, Phi2. split; [ right; exact Hin2 | exact Hrest ].
    + (* head distinguishes *)
      right. exists g, S', Phi. split; [ left; reflexivity | ].
      split; [ exact Hd | split; [ exact Hs | ] ].
      intros T' Hstep. apply Ht. apply graded_succ_complete. exact Hstep.
Qed.

(* ── The dichotomy ───────────────────────────────────────────────────────── *)
Theorem graded_dichotomy : forall n S T,
  graded_bisim_n n S T \/ (exists phi, gdepth phi <= n /\ gsat S phi /\ ~ gsat T phi).
Proof.
  induction n as [| n' IHn]; intros S T.
  - left. exact I.
  - destruct (forward_check n' IHn T (graded_succ_all S)) as [Hfwd | [g [S' [Phi [Hin [Hd [Hs Ht]]]]]]].
    + destruct (forward_check n' IHn S (graded_succ_all T)) as [Hbwd | [g [T' [Phi [Hin [Hd [Hs Ht]]]]]]].
      * (* both directions match — bisimilar *)
        left. simpl. split.
        -- intros g S' Hstep. apply Hfwd. apply graded_succ_all_complete. exact Hstep.
        -- intros g T' Hstep.
           destruct (Hbwd g T' (graded_succ_all_complete _ _ _ Hstep)) as [S' [HS Hbis]].
           exists S'. split; [ exact HS | apply graded_bisim_n_sym; exact Hbis ].
      * (* a T-successor T' (grade g) is distinguished by Phi; S cannot match — use GNot ⟨g⟩Phi *)
        right. exists (GNot (GDia g Phi)). split; [ simpl; apply le_n_S; exact Hd | ].
        split.
        -- (* S sat GNot ⟨g⟩Phi : no S-successor on g satisfies Phi *)
           simpl. intros [S' [Hstep Hsat]]. apply (Ht S' Hstep). exact Hsat.
        -- (* T does NOT sat GNot ⟨g⟩Phi : T -g-> T' with T' sat Phi *)
           simpl. intro Hcon. apply Hcon. exists T'. split.
           ++ apply graded_succ_all_sound. exact Hin.
           ++ exact Hs.
    + (* an S-successor S' (grade g) is distinguished by Phi; T cannot match — use ⟨g⟩Phi *)
      right. exists (GDia g Phi). split; [ simpl; apply le_n_S; exact Hd | ].
      split.
      * simpl. exists S'. split; [ apply graded_succ_all_sound; exact Hin | exact Hs ].
      * simpl. intros [T' [Hstep Hsat]]. apply (Ht T' Hstep). exact Hsat.
Qed.

(* ── Finitary adequacy ───────────────────────────────────────────────────── *)

(* Soundness at depth n: a depth-n bisimulation respects all formulae of depth ≤ n. *)
Lemma graded_bisim_n_sound : forall phi n S T,
  gdepth phi <= n -> graded_bisim_n n S T -> (gsat S phi <-> gsat T phi).
Proof.
  induction phi as [ | p IHp q IHq | p IHp | g p IHp ]; intros n S T Hd Hbis.
  - split; intro; exact I.
  - simpl in Hd.
    assert (Hdp : gdepth p <= n) by lia.
    assert (Hdq : gdepth q <= n) by lia.
    destruct (IHp n S T Hdp Hbis) as [Hp1 Hp2].
    destruct (IHq n S T Hdq Hbis) as [Hq1 Hq2].
    simpl. split.
    + intros [HA HB]. split; [ apply Hp1; exact HA | apply Hq1; exact HB ].
    + intros [HA HB]. split; [ apply Hp2; exact HA | apply Hq2; exact HB ].
  - simpl in Hd.
    destruct (IHp n S T Hd Hbis) as [Hp1 Hp2].
    simpl. split.
    + intros Hn Hc. apply Hn. apply Hp2; exact Hc.
    + intros Hn Hc. apply Hn. apply Hp1; exact Hc.
  - simpl in Hd. destruct n as [| n']; [ lia | ].
    assert (Hp : gdepth p <= n') by lia.
    destruct Hbis as [Hfwd Hbwd]. simpl. split.
    + intros [S' [Hstep Hsat]].
      destruct (Hfwd g S' Hstep) as [T' [HT Hbis']].
      exists T'. split; [ exact HT | apply (proj1 (IHp n' S' T' Hp Hbis')); exact Hsat ].
    + intros [T' [Hstep Hsat]].
      destruct (Hbwd g T' Hstep) as [S' [HS Hbis']].
      exists S'. split; [ exact HS | apply (proj2 (IHp n' S' T' Hp Hbis')); exact Hsat ].
Qed.

(* The finitary graded Hennessy–Milner adequacy: at every finite modal depth n,
   depth-n graded bisimilarity is EXACTLY agreement on all graded-HML formulae of
   depth ≤ n. *)
Theorem graded_finitary_adequacy : forall n S T,
  graded_bisim_n n S T <-> (forall phi, gdepth phi <= n -> (gsat S phi <-> gsat T phi)).
Proof.
  intros n S T. split.
  - intros Hbis phi Hd. apply (graded_bisim_n_sound phi n S T Hd Hbis).
  - intros Hequiv.
    destruct (graded_dichotomy n S T) as [Hbis | [phi [Hd [Hs Ht]]]].
    + exact Hbis.
    + exfalso. apply Ht. apply (Hequiv phi Hd). exact Hs.
Qed.
