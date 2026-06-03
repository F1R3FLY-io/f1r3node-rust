(* ════════════════════════════════════════════════════════════════════════
   CAGradedSuccPairs.v — grade-tagged successor enumeration (CL6 completeness).

   The Hennessy–Milner dichotomy must iterate over ALL of a state's graded
   successors, across all grades at once. This module exhibits the finite list of
   (grade, successor) PAIRS, graded_succ_all S, with the characterisation
   graded_step S g S' <-> In (g, S') (graded_succ_all S). Built by the same
   structural enumeration as CAGradedImageFinite, now grade-tagged. Axiom-free.  *)

From Stdlib Require Import Lists.List.
Import ListNotations.
From Stdlib Require Import PeanoNat.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CABinding.
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
      (* N-ary join J1 (ca_join1): grade is the single funding seal s; payloads read
         back from the bundle, reconstructing it at matching arity, and closed. *)
      | STSigned (CPPar (CPJoin xsj Tj) snds) s =>
          match Nat.eq_dec (length xsj) (length (extract_sends snds)),
                caproc_eq_dec snds (join_sends xsj (extract_sends snds)),
                Forall_dec closed_st closed_st_dec (extract_sends snds),
                sig_eq_dec s sg with
          | left _, left _, left _, left _ =>
              [ (s, STPar (subst_st_many Tj (extract_sends snds)) (STStack t)) ]
          | _, _, _, _ => []
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
          (* J2: separately-signed receiver (CPJoin) + sender bundle, combined token. *)
          | STSigned (CPJoin xsj Tj) s1, snds =>
              match Nat.eq_dec (length xsj) (length (sb_pays snds)),
                    Nat.eq_dec (length xsj) (length (sb_sigs snds)),
                    st_eq_dec snds (signed_sends xsj (sb_pays snds) (sb_sigs snds)),
                    Forall_dec closed_st closed_st_dec (sb_pays snds),
                    sig_eq_dec sg (join_token_key s1 (sb_sigs snds)) with
              | left _, left _, left _, left _, left _ =>
                  [ (join_token_key s1 (sb_sigs snds),
                     STPar (subst_st_many Tj (sb_pays snds)) (STStack t)) ]
              | _, _, _, _, _ => []
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
      * destruct PA as [ | xA TA | xA UA | PA1 PA2 | xA | xsj Tj ]; try contradiction.
        destruct PA1 as [ | xf Ti | | | | xj Tcont ]; try contradiction.
        -- (* CPInput head — rules 1/3 *)
           destruct PA2 as [ | | xs Uo | | | ]; try contradiction.
           destruct (caname_eq_dec xf xs) as [Hx | ]; try contradiction.
           destruct (sig_eq_dec sA sg) as [Hs | ]; try contradiction.
           simpl in Hredex. destruct Hredex as [Heq | []].
           inversion Heq; subst. apply g_rule1.
        -- (* CPJoin head — the whole-join (ca_join1 image) *)
           destruct (Nat.eq_dec (length xj) (length (extract_sends PA2))) as [Hlen | ]; try contradiction.
           destruct (caproc_eq_dec PA2 (join_sends xj (extract_sends PA2))) as [Hrec | ]; try contradiction.
           destruct (Forall_dec closed_st closed_st_dec (extract_sends PA2)) as [Hcl | ]; try contradiction.
           destruct (sig_eq_dec sA sg) as [Hs | ]; try contradiction.
           simpl in Hredex. destruct Hredex as [Heq | []].
           inversion Heq; subst. apply g_join1; [ exact Hrec | exact Hlen | exact Hcl ].
      * destruct A1 as [P1 s1' | A11 A12 | t1']; try contradiction.
        -- destruct P1 as [ | xf T1 | xs U1 | P11 P12 | x1 | xsj Tj ]; try contradiction.
           ++ destruct A2 as [P2 s2' | | ]; try contradiction.
              destruct P2 as [ | | xs U2 | | | ]; try contradiction.
              destruct (caname_eq_dec xf xs) as [Hx | ]; try contradiction.
              destruct (sig_eq_dec sg (SAnd s1' s2')) as [Hsg | ]; try contradiction.
              simpl in Hredex. destruct Hredex as [Heq | []].
              inversion Heq; subst. apply g_rule4.
           ++ destruct P11 as [ | xf T2 | | | | ]; try contradiction.
              destruct P12 as [ | | xs U2 | | | ]; try contradiction.
              destruct s1' as [ | | | s1a s1b ]; try contradiction.
              destruct A2 as [ | | tA2 ]; try contradiction.
              destruct tA2 as [ | s1'' t1 ]; try contradiction.
              destruct (caname_eq_dec xf xs) as [Hx | ]; try contradiction.
              destruct (sig_eq_dec s1a s1'') as [H1 | ]; try contradiction.
              destruct (sig_eq_dec sg s1b) as [H2 | ]; try contradiction.
              simpl in Hredex. destruct Hredex as [Heq | []].
              inversion Heq; subst. apply g_rule2.
           ++ (* A1 = STSigned (CPJoin xsj Tj) s1' — J2 (separately-signed, combined token) *)
              destruct (Nat.eq_dec (length xsj) (length (sb_pays A2))) as [HU | ]; try contradiction.
              destruct (Nat.eq_dec (length xsj) (length (sb_sigs A2))) as [Ht | ]; try contradiction.
              destruct (st_eq_dec A2 (signed_sends xsj (sb_pays A2) (sb_sigs A2))) as [Hrec | ];
                try contradiction.
              destruct (Forall_dec closed_st closed_st_dec (sb_pays A2)) as [Hcl | ]; try contradiction.
              destruct (sig_eq_dec sg (join_token_key s1' (sb_sigs A2))) as [Hsg | ]; try contradiction.
              simpl in Hredex. destruct Hredex as [Heq | []].
              inversion Heq; subst. apply g_join2; [ exact Hrec | exact HU | exact Ht | exact Hcl ].
        -- destruct A11 as [P11 s1' | | ]; try contradiction.
           destruct P11 as [ | xf T1 | | | | ]; try contradiction.
           destruct A12 as [P12 s2' | | ]; try contradiction.
           destruct P12 as [ | | xs U2 | | | ]; try contradiction.
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
  - (* g_join1 — recover the payloads from the bundle, discharge the firing decs *)
    apply in_or_app. left. subst snds.
    rewrite (extract_sends_join_sends xs Us H0).
    destruct (Nat.eq_dec (length xs) (length Us)) as [_ | NE]; [| exfalso; apply NE; exact H0].
    destruct (caproc_eq_dec (join_sends xs Us) (join_sends xs Us)) as [_ | NE];
      [| exfalso; apply NE; reflexivity].
    destruct (Forall_dec closed_st closed_st_dec Us) as [_ | NE]; [| exfalso; apply NE; exact H1].
    sig_refl s. simpl. left; reflexivity.
  - (* g_join2 — recover payloads/sigs from the bundle, discharge the firing decs *)
    apply in_or_app. left. subst snds.
    rewrite (sb_pays_signed_sends xs Us ts H0 H1).
    rewrite (sb_sigs_signed_sends xs Us ts H0 H1).
    destruct (Nat.eq_dec (length xs) (length Us)) as [_ | NE]; [| exfalso; apply NE; exact H0].
    destruct (Nat.eq_dec (length xs) (length ts)) as [_ | NE]; [| exfalso; apply NE; exact H1].
    destruct (st_eq_dec (signed_sends xs Us ts) (signed_sends xs Us ts)) as [_ | NE];
      [| exfalso; apply NE; reflexivity].
    destruct (Forall_dec closed_st closed_st_dec Us) as [_ | NE]; [| exfalso; apply NE; exact H2].
    sig_refl (join_token_key s1 ts). simpl. left; reflexivity.
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
