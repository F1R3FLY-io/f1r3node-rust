(* ════════════════════════════════════════════════════════════════════════
   CAGradedSuccPairs.v — grade-tagged successor enumeration (CL6 completeness).

   The Hennessy–Milner dichotomy must iterate over ALL of a state's graded
   successors, across all grades at once. This module exhibits the finite list of
   (grade, successor) PAIRS, graded_succ_all S, with the characterisation
   graded_step S g S' <-> In (g, S') (graded_succ_all S). Built by the same
   structural enumeration as CAGradedImageFinite, now grade-tagged. Axiom-free.  *)

From Stdlib Require Import Lists.List.
Import ListNotations.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CAGradedTransition.

(* Redex (grade, successor) pairs at the TOP of STPar A B. *)
Definition redex_pairs (A B : signed_term) : list (sig * signed_term) :=
  match B with
  | STStack (TGate sg t) =>
      match A with
      | STSigned (CPPar (CPInput xf T) (CPOutput xs U)) s =>
          match caname_eq_dec xf xs, sig_eq_dec s sg with
          | left _, left _ => [ (s, STPar (subst_st T 0 (CQuote U)) (STStack t)) ]
          | _, _ => []
          end
      | STPar A1 A2 =>
          match A1, A2 with
          | STSigned (CPPar (CPInput xf T) (CPOutput xs U)) (SAnd s1 s2), STStack (TGate s1' t1) =>
              match caname_eq_dec xf xs, sig_eq_dec s1 s1', sig_eq_dec sg s2 with
              | left _, left _, left _ =>
                  [ (SAnd s1 s2, STPar (STPar (subst_st T 0 (CQuote U)) (STStack t1)) (STStack t)) ]
              | _, _, _ => []
              end
          | STSigned (CPInput xf T) s1, STSigned (CPOutput xs U) s2 =>
              match caname_eq_dec xf xs, sig_eq_dec sg (SAnd s1 s2) with
              | left _, left _ => [ (SAnd s1 s2, STPar (subst_st T 0 (CQuote U)) (STStack t)) ]
              | _, _ => []
              end
          | STPar (STSigned (CPInput xf T) s1) (STSigned (CPOutput xs U) s2), STStack (TGate s1' t1) =>
              match caname_eq_dec xf xs, sig_eq_dec s1 s1', sig_eq_dec sg s2 with
              | left _, left _, left _ =>
                  [ (SAnd s1 s2, STPar (STPar (subst_st T 0 (CQuote U)) (STStack t1)) (STStack t)) ]
              | _, _, _ => []
              end
          | _, _ => []
          end
      | _ => []
      end
  | _ => []
  end.

Fixpoint graded_succ_all (S : signed_term) : list (sig * signed_term) :=
  match S with
  | STPar A B =>
      redex_pairs A B
      ++ map (fun p => (fst p, STPar (snd p) B)) (graded_succ_all A)
      ++ map (fun p => (fst p, STPar A (snd p))) (graded_succ_all B)
  | _ => []
  end.

Lemma sig_eq_dec_same : forall (a : sig), exists e, sig_eq_dec a a = left e.
Proof. intro a. destruct (sig_eq_dec a a) as [e | n]; [ exists e; reflexivity | exfalso; apply n; reflexivity ]. Qed.
Lemma caname_eq_dec_same : forall (a : caname), exists e, caname_eq_dec a a = left e.
Proof. intro a. destruct (caname_eq_dec a a) as [e | n]; [ exists e; reflexivity | exfalso; apply n; reflexivity ]. Qed.
Ltac sig_refl a := let e := fresh in let H := fresh in destruct (sig_eq_dec_same a) as [e H]; rewrite H.
Ltac caname_refl a := let e := fresh in let H := fresh in destruct (caname_eq_dec_same a) as [e H]; rewrite H.

Lemma graded_succ_all_sound : forall S g S',
  In (g, S') (graded_succ_all S) -> graded_step S g S'.
Proof.
  induction S as [P s | A IHA B IHB | t]; intros g S' Hin; simpl in Hin.
  - contradiction.
  - apply in_app_or in Hin. destruct Hin as [Hredex | Hpar].
    + unfold redex_pairs in Hredex.
      destruct B as [PB sB | B1 B2 | tB]; try contradiction.
      destruct tB as [| sg t]; try contradiction.
      destruct A as [PA sA | A1 A2 | tA]; try contradiction.
      * destruct PA as [ | xA TA | xA UA | PA1 PA2 | xA ]; try contradiction.
        destruct PA1 as [ | xf Ti | | | ]; try contradiction.
        destruct PA2 as [ | | xs Uo | | ]; try contradiction.
        destruct (caname_eq_dec xf xs) as [Hx | ]; try contradiction.
        destruct (sig_eq_dec sA sg) as [Hs | ]; try contradiction.
        simpl in Hredex. destruct Hredex as [Heq | []].
        inversion Heq; subst. apply g_rule1.
      * destruct A1 as [P1 s1' | A11 A12 | t1']; try contradiction.
        -- destruct P1 as [ | xf T1 | xs U1 | P11 P12 | x1 ]; try contradiction.
           ++ destruct A2 as [P2 s2' | | ]; try contradiction.
              destruct P2 as [ | | xs U2 | | ]; try contradiction.
              destruct (caname_eq_dec xf xs) as [Hx | ]; try contradiction.
              destruct (sig_eq_dec sg (SAnd s1' s2')) as [Hsg | ]; try contradiction.
              simpl in Hredex. destruct Hredex as [Heq | []].
              inversion Heq; subst. apply g_rule4.
           ++ destruct P11 as [ | xf T2 | | | ]; try contradiction.
              destruct P12 as [ | | xs U2 | | ]; try contradiction.
              destruct s1' as [ | | | s1a s1b ]; try contradiction.
              destruct A2 as [ | | tA2 ]; try contradiction.
              destruct tA2 as [ | s1'' t1 ]; try contradiction.
              destruct (caname_eq_dec xf xs) as [Hx | ]; try contradiction.
              destruct (sig_eq_dec s1a s1'') as [H1 | ]; try contradiction.
              destruct (sig_eq_dec sg s1b) as [H2 | ]; try contradiction.
              simpl in Hredex. destruct Hredex as [Heq | []].
              inversion Heq; subst. apply g_rule2.
        -- destruct A11 as [P11 s1' | | ]; try contradiction.
           destruct P11 as [ | xf T1 | | | ]; try contradiction.
           destruct A12 as [P12 s2' | | ]; try contradiction.
           destruct P12 as [ | | xs U2 | | ]; try contradiction.
           destruct A2 as [ | | tA2 ]; try contradiction.
           destruct tA2 as [ | s1'' t1 ]; try contradiction.
           destruct (caname_eq_dec xf xs) as [Hx | ]; try contradiction.
           destruct (sig_eq_dec s1' s1'') as [H1 | ]; try contradiction.
           destruct (sig_eq_dec sg s2') as [H2 | ]; try contradiction.
           simpl in Hredex. destruct Hredex as [Heq | []].
           inversion Heq; subst. apply g_rule5.
    + apply in_app_or in Hpar. destruct Hpar as [HparL | HparR].
      * apply in_map_iff in HparL. destruct HparL as [[gA A'] [Heq Hin']].
        inversion Heq; subst. apply g_par_l. apply IHA. exact Hin'.
      * apply in_map_iff in HparR. destruct HparR as [[gB B'] [Heq Hin']].
        inversion Heq; subst. apply g_par_r. apply IHB. exact Hin'.
  - contradiction.
Qed.

Lemma graded_succ_all_complete : forall S g S',
  graded_step S g S' -> In (g, S') (graded_succ_all S).
Proof.
  intros S g S' Hstep. induction Hstep; simpl.
  - apply in_or_app. left. caname_refl x. simpl. sig_refl s. simpl. left; reflexivity.
  - apply in_or_app. left. caname_refl x. simpl. sig_refl s1. simpl. sig_refl s2. simpl. left; reflexivity.
  - apply in_or_app. left. caname_refl x. simpl. sig_refl s1. simpl. sig_refl s2. simpl. left; reflexivity.
  - apply in_or_app. left. caname_refl x. simpl. sig_refl s1. simpl. sig_refl s2. simpl. left; reflexivity.
  - apply in_or_app. left. caname_refl x. simpl. sig_refl s1. simpl. sig_refl s2. simpl. left; reflexivity.
  - apply in_or_app. right. apply in_or_app. left.
    apply in_map_iff. exists (g, S1'). split; [ reflexivity | exact IHHstep ].
  - apply in_or_app. right. apply in_or_app. right.
    apply in_map_iff. exists (g, S2'). split; [ reflexivity | exact IHHstep ].
Qed.

Theorem graded_image_finite_pairs : forall S g S',
  graded_step S g S' <-> In (g, S') (graded_succ_all S).
Proof.
  intros S g S'. split; [ apply graded_succ_all_complete | apply graded_succ_all_sound ].
Qed.
