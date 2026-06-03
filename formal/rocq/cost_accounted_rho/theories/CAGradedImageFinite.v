(* ════════════════════════════════════════════════════════════════════════
   CAGradedImageFinite.v — image-finiteness of the graded LTS (CL6 foundation).

   The signature-graded transition relation graded_step is IMAGE-FINITE: for each
   state S and grade g, the set { S' | graded_step S g S' } is finite — exhibited
   by an explicit successor enumeration graded_succ with a soundness+completeness
   characterisation (graded_step S g S' <-> In S' (graded_succ S g)). This is the
   constructive foundation the Hennessy–Milner COMPLETENESS direction needs (to
   form the finite conjunction of distinguishing formulae); it avoids
   Classical/funext/choice. Axiom-free.                                         *)

From Stdlib Require Import Lists.List.
Import ListNotations.
From Stdlib Require Import PeanoNat.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CABinding.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CAGradedTransition.

(* The redex successors at the TOP of STPar A B for grade g (covering the five
   gated rules; rules 1 and 3 share the same top shape and contractum). Each rule
   requires the receiver and sender to share the SAME channel (caname_eq_dec). *)
Definition redex_succ (A B : signed_term) (g : sig) : list signed_term :=
  match B with
  | STStack (TGate sg t) =>
      match A with
      (* rules 1 / 3: whole redex, single token on the gate's own signature. *)
      | STSigned (CPPar (CPInput xf T) (CPOutput xs U)) s =>
          match caname_eq_dec xf xs, sig_eq_dec s sg, sig_eq_dec g s with
          | left _, left _, left _ => [ STPar (subst_st T 0 (CQuote U)) (STStack t) ]
          | _, _, _ => []
          end
      (* N-ary join J1 (ca_join1): whole-join under one seal s, single s-token; the
         payloads are read back from the sender bundle and must reconstruct it
         (caproc_eq_dec) at matching arity and be closed (the firing precondition). *)
      | STSigned (CPPar (CPJoin xsj Tj) snds) s =>
          match Nat.eq_dec (length xsj) (length (extract_sends snds)),
                caproc_eq_dec snds (join_sends xsj (extract_sends snds)),
                Forall_dec closed_st closed_st_dec (extract_sends snds),
                sig_eq_dec s sg, sig_eq_dec g s with
          | left _, left _, left _, left _, left _ =>
              [ STPar (subst_st_many Tj (extract_sends snds)) (STStack t) ]
          | _, _, _, _, _ => []
          end
      | STPar A1 A2 =>
          match A1, A2 with
          (* rule 2: compound whole redex, split tokens (inner on s1, outer on s2=sg). *)
          | STSigned (CPPar (CPInput xf T) (CPOutput xs U)) (SAnd s1 s2), STStack (TGate s1' t1) =>
              match caname_eq_dec xf xs, sig_eq_dec s1 s1', sig_eq_dec sg s2, sig_eq_dec g (SAnd s1 s2) with
              | left _, left _, left _, left _ =>
                  [ STPar (STPar (subst_st T 0 (CQuote U)) (STStack t1)) (STStack t) ]
              | _, _, _, _ => []
              end
          (* rule 4: split processes, combined token. *)
          | STSigned (CPInput xf T) s1, STSigned (CPOutput xs U) s2 =>
              match caname_eq_dec xf xs, sig_eq_dec sg (SAnd s1 s2), sig_eq_dec g (SAnd s1 s2) with
              | left _, left _, left _ => [ STPar (subst_st T 0 (CQuote U)) (STStack t) ]
              | _, _, _ => []
              end
          (* rule 5: split processes, split tokens. *)
          | STPar (STSigned (CPInput xf T) s1) (STSigned (CPOutput xs U) s2), STStack (TGate s1' t1) =>
              match caname_eq_dec xf xs, sig_eq_dec s1 s1', sig_eq_dec sg s2, sig_eq_dec g (SAnd s1 s2) with
              | left _, left _, left _, left _ =>
                  [ STPar (STPar (subst_st T 0 (CQuote U)) (STStack t1)) (STStack t) ]
              | _, _, _, _ => []
              end
          (* J2: separately-signed receiver (CPJoin) + sender bundle, one combined
             token keyed s1 ∘ t1 ∘ … ∘ tN. Recover payloads/sigs from the bundle,
             require it to reconstruct (st_eq_dec) at matching arity, closed, and the
             token key to be the fused key. *)
          | STSigned (CPJoin xsj Tj) s1, snds =>
              match Nat.eq_dec (length xsj) (length (sb_pays snds)),
                    Nat.eq_dec (length xsj) (length (sb_sigs snds)),
                    st_eq_dec snds (signed_sends xsj (sb_pays snds) (sb_sigs snds)),
                    Forall_dec closed_st closed_st_dec (sb_pays snds),
                    sig_eq_dec sg (join_token_key s1 (sb_sigs snds)), sig_eq_dec g sg with
              | left _, left _, left _, left _, left _, left _ =>
                  [ STPar (subst_st_many Tj (sb_pays snds)) (STStack t) ]
              | _, _, _, _, _, _ => []
              end
          | _, _ => []
          end
      | _ => []
      end
  | _ => []
  end.

(* The full successor enumeration: redex steps at the top, plus the par steps
   into the left and right components. *)
Fixpoint graded_succ (S : signed_term) (g : sig) : list signed_term :=
  match S with
  | STPar A B =>
      redex_succ A B g
      ++ map (fun A' => STPar A' B) (graded_succ A g)
      ++ map (fun B' => STPar A B') (graded_succ B g)
  | _ => []
  end.

(* Soundness: every enumerated successor is a real graded step. The redex case is
   a deep but mechanical structural decomposition; the residual single-element
   lists then match the corresponding rule constructor. *)
Lemma graded_succ_sound : forall S g S',
  In S' (graded_succ S g) -> graded_step S g S'.
Proof.
  induction S as [P s | A IHA B IHB | t]; intros g S' Hin; simpl in Hin.
  - contradiction.
  - apply in_app_or in Hin. destruct Hin as [Hredex | Hpar].
    + (* redex successors *)
      unfold redex_succ in Hredex.
      destruct B as [PB sB | B1 B2 | tB]; try contradiction.
      destruct tB as [| sg t]; try contradiction.
      destruct A as [PA sA | A1 A2 | tA]; try contradiction.
      * (* A = STSigned PA sA — rules 1/3 (CPInput head) or the N-ary join (CPJoin head) *)
        destruct PA as [ | xA TA | xA UA | PA1 PA2 | xA | xsj Tj ]; try contradiction.
        destruct PA1 as [ | xf Ti | | | | xj Tcont ]; try contradiction.
        -- (* CPInput head — rules 1/3 *)
           destruct PA2 as [ | | xs Uo | | | ]; try contradiction.
           destruct (caname_eq_dec xf xs) as [Hx | ]; try contradiction.
           destruct (sig_eq_dec sA sg) as [Hs | ]; try contradiction.
           destruct (sig_eq_dec g sA) as [Hg | ]; try contradiction.
           simpl in Hredex. destruct Hredex as [Heq | []].
           subst. apply g_rule1.
        -- (* CPJoin head — the whole-join (ca_join1 image) *)
           destruct (Nat.eq_dec (length xj) (length (extract_sends PA2))) as [Hlen | ]; try contradiction.
           destruct (caproc_eq_dec PA2 (join_sends xj (extract_sends PA2))) as [Hrec | ]; try contradiction.
           destruct (Forall_dec closed_st closed_st_dec (extract_sends PA2)) as [Hcl | ]; try contradiction.
           destruct (sig_eq_dec sA sg) as [Hs | ]; try contradiction.
           destruct (sig_eq_dec g sA) as [Hg | ]; try contradiction.
           simpl in Hredex. destruct Hredex as [Heq | []].
           subst. apply g_join1; [ exact Hrec | exact Hlen | exact Hcl ].
      * (* A = STPar A1 A2 — rules 2/4/5 *)
        destruct A1 as [P1 s1' | A11 A12 | t1']; try contradiction.
        -- (* A1 = STSigned P1 s1' *)
           destruct P1 as [ | xf T1 | xs U1 | P11 P12 | x1 | xsj Tj ]; try contradiction.
           ++ (* A1 = STSigned (CPInput xf T1) s1' — rule 4 *)
              destruct A2 as [P2 s2' | | ]; try contradiction.
              destruct P2 as [ | | xs U2 | | | ]; try contradiction.
              destruct (caname_eq_dec xf xs) as [Hx | ]; try contradiction.
              destruct (sig_eq_dec sg (SAnd s1' s2')) as [Hsg | ]; try contradiction.
              destruct (sig_eq_dec g (SAnd s1' s2')) as [Hg | ]; try contradiction.
              simpl in Hredex. destruct Hredex as [Heq | []].
              subst. apply g_rule4.
           ++ (* A1 = STSigned (CPPar (CPInput ..)(CPOutput ..)) (SAnd ..) — rule 2 *)
              destruct P11 as [ | xf T2 | | | | ]; try contradiction.
              destruct P12 as [ | | xs U2 | | | ]; try contradiction.
              destruct s1' as [ | | | s1a s1b ]; try contradiction.
              destruct A2 as [ | | tA2 ]; try contradiction.
              destruct tA2 as [ | s1'' t1 ]; try contradiction.
              destruct (caname_eq_dec xf xs) as [Hx | ]; try contradiction.
              destruct (sig_eq_dec s1a s1'') as [H1 | ]; try contradiction.
              destruct (sig_eq_dec sg s1b) as [H2 | ]; try contradiction.
              destruct (sig_eq_dec g (SAnd s1a s1b)) as [Hg | ]; try contradiction.
              simpl in Hredex. destruct Hredex as [Heq | []].
              subst. apply g_rule2.
           ++ (* A1 = STSigned (CPJoin xsj Tj) s1' — J2 (separately-signed, combined token) *)
              destruct (Nat.eq_dec (length xsj) (length (sb_pays A2))) as [HU | ]; try contradiction.
              destruct (Nat.eq_dec (length xsj) (length (sb_sigs A2))) as [Ht | ]; try contradiction.
              destruct (st_eq_dec A2 (signed_sends xsj (sb_pays A2) (sb_sigs A2))) as [Hrec | ];
                try contradiction.
              destruct (Forall_dec closed_st closed_st_dec (sb_pays A2)) as [Hcl | ]; try contradiction.
              destruct (sig_eq_dec sg (join_token_key s1' (sb_sigs A2))) as [Hsg | ]; try contradiction.
              destruct (sig_eq_dec g sg) as [Hg | ]; try contradiction.
              simpl in Hredex. destruct Hredex as [Heq | []].
              subst. apply g_join2; [ exact Hrec | exact HU | exact Ht | exact Hcl ].
        -- (* A1 = STPar A11 A12 — rule 5 *)
           destruct A11 as [P11 s1' | | ]; try contradiction.
           destruct P11 as [ | xf T1 | | | | ]; try contradiction.
           destruct A12 as [P12 s2' | | ]; try contradiction.
           destruct P12 as [ | | xs U2 | | | ]; try contradiction.
           destruct A2 as [ | | tA2 ]; try contradiction.
           destruct tA2 as [ | s1'' t1 ]; try contradiction.
           destruct (caname_eq_dec xf xs) as [Hx | ]; try contradiction.
           destruct (sig_eq_dec s1' s1'') as [H1 | ]; try contradiction.
           destruct (sig_eq_dec sg s2') as [H2 | ]; try contradiction.
           destruct (sig_eq_dec g (SAnd s1' s2')) as [Hg | ]; try contradiction.
           simpl in Hredex. destruct Hredex as [Heq | []].
           subst. apply g_rule5.
    + (* par successors *)
      apply in_app_or in Hpar. destruct Hpar as [HparL | HparR].
      * apply in_map_iff in HparL. destruct HparL as [A' [Heq Hin']]; subst.
        apply g_par_l. apply IHA. exact Hin'.
      * apply in_map_iff in HparR. destruct HparR as [B' [Heq Hin']]; subst.
        apply g_par_r. apply IHB. exact Hin'.
  - contradiction.
Qed.

(* eq_dec-reflexivity helpers: rewriting with these avoids destructing an eq_dec
   on a compound term (which Coq cannot abstract over). *)
Lemma sig_eq_dec_same : forall (a : sig), exists e, sig_eq_dec a a = left e.
Proof. intro a. destruct (sig_eq_dec a a) as [e | n]; [ exists e; reflexivity | exfalso; apply n; reflexivity ]. Qed.

Lemma caname_eq_dec_same : forall (a : caname), exists e, caname_eq_dec a a = left e.
Proof. intro a. destruct (caname_eq_dec a a) as [e | n]; [ exists e; reflexivity | exfalso; apply n; reflexivity ]. Qed.

Ltac sig_refl a := let e := fresh in let H := fresh in destruct (sig_eq_dec_same a) as [e H]; rewrite H.
Ltac caname_refl a := let e := fresh in let H := fresh in destruct (caname_eq_dec_same a) as [e H]; rewrite H.

(* Completeness: every real graded step is enumerated. *)
Lemma graded_succ_complete : forall S g S',
  graded_step S g S' -> In S' (graded_succ S g).
Proof.
  intros S g S' Hstep. induction Hstep; simpl.
  - (* g_rule1 *) apply in_or_app. left.
    caname_refl x. simpl. sig_refl s. simpl. left; reflexivity.
  - (* g_rule2 *) apply in_or_app. left.
    caname_refl x. simpl. sig_refl s1. simpl. sig_refl s2. simpl. left; reflexivity.
  - (* g_rule3 — grade SAnd reduces to leaf comparisons on s1, s2 *) apply in_or_app. left.
    caname_refl x. simpl. sig_refl s1. simpl. sig_refl s2. simpl. left; reflexivity.
  - (* g_rule4 *) apply in_or_app. left.
    caname_refl x. simpl. sig_refl s1. simpl. sig_refl s2. simpl. left; reflexivity.
  - (* g_rule5 *) apply in_or_app. left.
    caname_refl x. simpl. sig_refl s1. simpl. sig_refl s2. simpl. left; reflexivity.
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
  - (* g_par_l *) apply in_or_app. right. apply in_or_app. left.
    apply in_map_iff. exists S1'. split; [ reflexivity | exact IHHstep ].
  - (* g_par_r *) apply in_or_app. right. apply in_or_app. right.
    apply in_map_iff. exists S2'. split; [ reflexivity | exact IHHstep ].
Qed.

(* Image-finiteness: the enumeration exactly captures the graded successors. *)
Theorem graded_image_finite : forall S g S',
  graded_step S g S' <-> In S' (graded_succ S g).
Proof.
  intros S g S'. split; [ apply graded_succ_complete | apply graded_succ_sound ].
Qed.
