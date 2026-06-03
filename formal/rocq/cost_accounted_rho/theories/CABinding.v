(* ════════════════════════════════════════════════════════════════════════
   CABinding.v — Locally-nameless binding metatheory for the native four-sort
   grammar (DR-21 Option B).

   Mirrors the locally-nameless lemma set of [RhoSyntax] (lift_zero,
   subst_lift_cancel/_zero/_strong/_two_one, the closed_* family) for the three
   native mutually-inductive sorts [caproc] / [caname] / [signed_term] of
   [CASyntax]. Each family lemma is proved once as a 3-way conjunction via the
   combined scheme [ca_mutind], then projected to the per-sort named lemmas the
   translation stack consumes. The [STStack t] case is trivial throughout
   (tokens carry no de Bruijn names). Axiom-free.                              *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Bool.
From Stdlib Require Import Lists.List.
Import ListNotations.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.

(* A map whose function is pointwise the identity on a list (witnessed by Forall)
   leaves the list unchanged — the workhorse for the CPJoin channel-list cases. *)
Lemma map_id_Forall {A} (f : A -> A) (xs : list A) :
  Forall (fun x => f x = x) xs -> map f xs = xs.
Proof. induction 1; simpl; [ reflexivity | rewrite H, IHForall; reflexivity ]. Qed.

(* ── Section 1: lift 0 is the identity ──────────────────────────────────── *)
(* (the conjunction [lift_zero_ca] is proved in CASyntax; project it here.) *)

Lemma lift_zero_caproc : forall P c, lift_caproc 0 c P = P.
Proof. apply lift_zero_ca. Qed.
Lemma lift_zero_caname : forall x c, lift_caname 0 c x = x.
Proof. apply lift_zero_ca. Qed.
Lemma lift_zero_st : forall T c, lift_st 0 c T = T.
Proof. apply lift_zero_ca. Qed.

(* ── Section 2: lift-substitution cancellation ──────────────────────────── *)

Lemma subst_lift_cancel_ca :
  (forall P c N, subst_caproc (lift_caproc 1 c P) c N = P)
  /\ (forall x c N, subst_caname (lift_caname 1 c x) c N = x)
  /\ (forall T c N, subst_st (lift_st 1 c T) c N = T).
Proof.
  apply ca_deep_ind; intros; simpl.
  - (* CPNil *) reflexivity.
  - (* CPInput x T *) rewrite H. rewrite H0. reflexivity.
  - (* CPOutput x U *) rewrite H. rewrite H0. reflexivity.
  - (* CPPar *) rewrite H. rewrite H0. reflexivity.
  - (* CPDeref x: x is the caname [c]; the cutoff is [c0] *)
    destruct c as [Ti | k]; simpl.
    + (* CQuote Ti: name-level IH collapses the inner substitution *)
      specialize (H c0 N). simpl in H. injection H as Hinner.
      rewrite Hinner. reflexivity.
    + (* CNVar k *)
      destruct (c0 <=? k) eqn:Hck; simpl.
      * apply Nat.leb_le in Hck.
        assert (Hgt : Nat.compare (k + 1) c0 = Gt)
          by (apply Nat.compare_gt_iff; lia).
        rewrite Hgt. f_equal. f_equal. lia.
      * apply Nat.leb_gt in Hck.
        assert (Hlt : Nat.compare k c0 = Lt)
          by (apply Nat.compare_lt_iff; lia).
        rewrite Hlt. reflexivity.
  - (* CPJoin xs T *) simpl. f_equal.
    + rewrite map_map. apply map_id_Forall.
      eapply Forall_impl; [| exact H]. intros a Ha. apply Ha.
    + rewrite length_map. apply H0.
  - (* CQuote T *) rewrite H. reflexivity.
  - (* CNVar k *)
    destruct (c <=? n) eqn:Hcn.
    + simpl. apply Nat.leb_le in Hcn.
      assert (Hgt: Nat.compare (n + 1) c = Gt) by (apply Nat.compare_gt_iff; lia).
      rewrite Hgt. f_equal. lia.
    + simpl. apply Nat.leb_gt in Hcn.
      assert (Hlt: Nat.compare n c = Lt) by (apply Nat.compare_lt_iff; lia).
      rewrite Hlt. reflexivity.
  - (* STSigned P s *) rewrite H. reflexivity.
  - (* STPar T1 T2 *) rewrite H. rewrite H0. reflexivity.
  - (* STStack t *) reflexivity.
Qed.

Lemma subst_lift_cancel_caproc :
  forall P c N, subst_caproc (lift_caproc 1 c P) c N = P.
Proof. apply subst_lift_cancel_ca. Qed.
Lemma subst_lift_cancel_st :
  forall T c N, subst_st (lift_st 1 c T) c N = T.
Proof. apply subst_lift_cancel_ca. Qed.

Lemma subst_lift_zero_caproc : forall P N, subst_caproc (lift_caproc 1 0 P) 0 N = P.
Proof. intros. apply subst_lift_cancel_caproc. Qed.
Lemma subst_lift_zero_st : forall T N, subst_st (lift_st 1 0 T) 0 N = T.
Proof. intros. apply subst_lift_cancel_st. Qed.

(* ── Section 3: generalized lift-substitution commutation ───────────────── *)

Lemma subst_lift_strong_ca :
  (forall P d c k N, c <= k -> k < c + S d ->
     subst_caproc (lift_caproc (S d) c P) k N = lift_caproc d c P)
  /\ (forall x d c k N, c <= k -> k < c + S d ->
     subst_caname (lift_caname (S d) c x) k N = lift_caname d c x)
  /\ (forall T d c k N, c <= k -> k < c + S d ->
     subst_st (lift_st (S d) c T) k N = lift_st d c T).
Proof.
  apply ca_deep_ind; intros.
  - (* CPNil *) reflexivity.
  - (* CPInput *) simpl. rewrite H by lia. rewrite H0 by lia. reflexivity.
  - (* CPOutput *) simpl. rewrite H by lia. rewrite H0 by lia. reflexivity.
  - (* CPPar *) simpl. rewrite H by lia. rewrite H0 by lia. reflexivity.
  - (* CPDeref: x is the caname [c]; the cutoff is [c0] *)
    destruct c as [Ti | k0].
    + simpl. specialize (H d c0 k N H0 H1). simpl in H. injection H as Hinner.
      rewrite Hinner. reflexivity.
    + simpl. destruct (c0 <=? k0) eqn:Hck0.
      * apply Nat.leb_le in Hck0.
        assert (Hgt : Nat.compare (k0 + S d) k = Gt) by (apply Nat.compare_gt_iff; lia).
        rewrite Hgt.
        destruct (c0 <=? k0) eqn:Hck0'; [| apply Nat.leb_gt in Hck0'; lia].
        f_equal. f_equal. lia.
      * apply Nat.leb_gt in Hck0.
        assert (Hlt : Nat.compare k0 k = Lt) by (apply Nat.compare_lt_iff; lia).
        rewrite Hlt.
        destruct (c0 <=? k0) eqn:Hck0'; [apply Nat.leb_le in Hck0'; lia |].
        reflexivity.
  - (* CPJoin xs T *) simpl. f_equal.
    + rewrite map_map. apply map_ext_Forall.
      eapply Forall_impl; [| exact H]. intros a Ha. apply Ha; assumption.
    + rewrite length_map. apply H0; lia.
  - (* CQuote *) simpl. rewrite H by lia. reflexivity.
  - (* CNVar n *)
    destruct (Nat.leb_spec c n) as [Hcn | Hcn].
    + assert (Hcn_eq : (c <=? n) = true) by (apply Nat.leb_le; assumption).
      cbn [lift_caname]. rewrite Hcn_eq. cbn [subst_caname].
      assert (Hgt: Nat.compare (n + S d) k = Gt) by (apply Nat.compare_gt_iff; lia).
      rewrite Hgt. f_equal. lia.
    + assert (Hcn_eq : (c <=? n) = false) by (apply Nat.leb_gt; assumption).
      cbn [lift_caname]. rewrite Hcn_eq. cbn [subst_caname].
      assert (Hlt: Nat.compare n k = Lt) by (apply Nat.compare_lt_iff; lia).
      rewrite Hlt. reflexivity.
  - (* STSigned *) simpl. rewrite H by lia. reflexivity.
  - (* STPar *) simpl. rewrite H by lia. rewrite H0 by lia. reflexivity.
  - (* STStack *) reflexivity.
Qed.

Lemma subst_lift_strong_caproc :
  forall P d c k N, c <= k -> k < c + S d ->
    subst_caproc (lift_caproc (S d) c P) k N = lift_caproc d c P.
Proof. apply subst_lift_strong_ca. Qed.

Lemma subst_lift_two_one_caproc : forall P N,
  subst_caproc (lift_caproc 2 0 P) 1 N = lift_caproc 1 0 P.
Proof. intros. apply (subst_lift_strong_caproc P 1 0 1 N); lia. Qed.

(* ── Section 4: closedness predicate ────────────────────────────────────── *)

Fixpoint closed_caproc_at (k : nat) (P : caproc) : Prop :=
  match P with
  | CPNil         => True
  | CPInput x T   => closed_caname_at k x /\ closed_st_at (S k) T
  | CPOutput x U  => closed_caname_at k x /\ closed_st_at k U
  | CPPar P1 P2   => closed_caproc_at k P1 /\ closed_caproc_at k P2
  | CPDeref x     => closed_caname_at k x
  | CPJoin xs T   =>
      fold_right (fun x acc => closed_caname_at k x /\ acc) True xs
      /\ closed_st_at (length xs + k) T
  end
with closed_caname_at (k : nat) (x : caname) : Prop :=
  match x with
  | CQuote T => closed_st_at k T
  | CNVar j  => j < k
  end
with closed_st_at (k : nat) (T : signed_term) : Prop :=
  match T with
  | STSigned P _ => closed_caproc_at k P
  | STPar T1 T2  => closed_st_at k T1 /\ closed_st_at k T2
  | STStack _    => True
  end.

Definition closed_caproc (P : caproc) : Prop := closed_caproc_at 0 P.
Definition closed_caname (x : caname) : Prop := closed_caname_at 0 x.
Definition closed_st (T : signed_term) : Prop := closed_st_at 0 T.

(* The CPJoin channel-closedness (a guard-friendly fold_right) is the Forall the
   binding lemmas reason with. *)
Lemma fold_right_closed_Forall : forall k xs,
  fold_right (fun x acc => closed_caname_at k x /\ acc) True xs
  <-> Forall (closed_caname_at k) xs.
Proof.
  intros k xs. induction xs as [| x xs' IH]; simpl.
  - split; [ intros _; constructor | reflexivity ].
  - split.
    + intros [Hh Ht]. constructor; [ exact Hh | apply IH; exact Ht ].
    + intros HF. inversion HF; subst. split; [ assumption | apply IH; assumption ].
Qed.

Lemma closed_at_mono_ca :
  (forall P k k', k <= k' -> closed_caproc_at k P -> closed_caproc_at k' P)
  /\ (forall x k k', k <= k' -> closed_caname_at k x -> closed_caname_at k' x)
  /\ (forall T k k', k <= k' -> closed_st_at k T -> closed_st_at k' T).
Proof.
  apply ca_deep_ind; intros; simpl in *.
  - exact I.
  - destruct H2. split; [eapply H; eauto | eapply H0; [|eassumption]; lia].
  - destruct H2. split; [eapply H; eauto | eapply H0; eauto].
  - destruct H2. split; [eapply H; eauto | eapply H0; eauto].
  - eapply H; eauto.
  - (* CPJoin xs T *) destruct H2 as [HF HT]. apply fold_right_closed_Forall in HF. split.
    + apply fold_right_closed_Forall.
      rewrite Forall_forall in H, HF |- *. intros x Hx.
      apply (H x Hx k k' H1). exact (HF x Hx).
    + apply (H0 (length xs + k) (length xs + k')); [ lia | exact HT ].
  - eapply H; eauto.
  - lia.
  - eapply H; eauto.
  - destruct H2. split; [eapply H; eauto | eapply H0; eauto].
  - exact I.
Qed.

Lemma closed_caproc_at_mono : forall P k k',
  k <= k' -> closed_caproc_at k P -> closed_caproc_at k' P.
Proof. apply closed_at_mono_ca. Qed.
Lemma closed_caname_at_mono : forall x k k',
  k <= k' -> closed_caname_at k x -> closed_caname_at k' x.
Proof. apply closed_at_mono_ca. Qed.
Lemma closed_st_at_mono : forall T k k',
  k <= k' -> closed_st_at k T -> closed_st_at k' T.
Proof. apply closed_at_mono_ca. Qed.

(* ── Section 5: substitution/lift are identity on closed terms ──────────── *)

Lemma closed_subst_ca :
  (forall P k N, closed_caproc_at k P -> subst_caproc P k N = P)
  /\ (forall x k N, closed_caname_at k x -> subst_caname x k N = x)
  /\ (forall T k N, closed_st_at k T -> subst_st T k N = T).
Proof.
  apply ca_deep_ind; intros; simpl in *.
  - reflexivity.
  - destruct H1. rewrite H by assumption. rewrite H0 by assumption. reflexivity.
  - destruct H1. rewrite H by assumption. rewrite H0 by assumption. reflexivity.
  - destruct H1. rewrite H by assumption. rewrite H0 by assumption. reflexivity.
  - (* CPDeref *)
    destruct c as [Ti | j]; simpl in *.
    + specialize (H k N H0). simpl in H. injection H as Hinner.
      rewrite Hinner. reflexivity.
    + assert (Hlt : Nat.compare j k = Lt) by (apply Nat.compare_lt_iff; assumption).
      rewrite Hlt. reflexivity.
  - (* CPJoin xs T *) destruct H1 as [HF HT]. apply fold_right_closed_Forall in HF. f_equal.
    + apply map_id_Forall. rewrite Forall_forall in H, HF |- *.
      intros x Hx. apply (H x Hx k N). exact (HF x Hx).
    + apply H0. exact HT.
  - rewrite H by assumption. reflexivity.
  - assert (Hlt : Nat.compare n k = Lt) by (apply Nat.compare_lt_iff; assumption).
    rewrite Hlt. reflexivity.
  - rewrite H by assumption. reflexivity.
  - destruct H1. rewrite H by assumption. rewrite H0 by assumption. reflexivity.
  - reflexivity.
Qed.

Lemma closed_caproc_subst : forall P k N, closed_caproc_at k P -> subst_caproc P k N = P.
Proof. apply closed_subst_ca. Qed.
Lemma closed_st_subst : forall T k N, closed_st_at k T -> subst_st T k N = T.
Proof. apply closed_subst_ca. Qed.

Lemma closed_lift_ca :
  (forall P d c, closed_caproc_at c P -> lift_caproc d c P = P)
  /\ (forall x d c, closed_caname_at c x -> lift_caname d c x = x)
  /\ (forall T d c, closed_st_at c T -> lift_st d c T = T).
Proof.
  apply ca_deep_ind; intros; simpl in *.
  - reflexivity.
  - destruct H1. rewrite H by assumption. rewrite H0 by assumption. reflexivity.
  - destruct H1. rewrite H by assumption. rewrite H0 by assumption. reflexivity.
  - destruct H1. rewrite H by assumption. rewrite H0 by assumption. reflexivity.
  - rewrite H by assumption. reflexivity.
  - (* CPJoin xs T *) destruct H1 as [HF HT]. apply fold_right_closed_Forall in HF. f_equal.
    + apply map_id_Forall. rewrite Forall_forall in H, HF |- *.
      intros x Hx. apply (H x Hx d c). exact (HF x Hx).
    + apply H0. exact HT.
  - rewrite H by assumption. reflexivity.
  - assert (Hleb : c <=? n = false) by (apply Nat.leb_gt; assumption).
    rewrite Hleb. reflexivity.
  - rewrite H by assumption. reflexivity.
  - destruct H1. rewrite H by assumption. rewrite H0 by assumption. reflexivity.
  - reflexivity.
Qed.

Lemma closed_caproc_lift : forall P d c, closed_caproc_at c P -> lift_caproc d c P = P.
Proof. apply closed_lift_ca. Qed.
Lemma closed_st_lift : forall T d c, closed_st_at c T -> lift_st d c T = T.
Proof. apply closed_lift_ca. Qed.

(* ── Section 6: closed-at-0 corollaries ─────────────────────────────────── *)

Lemma closed_caproc_subst_zero : forall P k N, closed_caproc P -> subst_caproc P k N = P.
Proof.
  unfold closed_caproc. intros. apply closed_caproc_subst.
  eapply closed_caproc_at_mono; [| eassumption]. lia.
Qed.
Lemma closed_caproc_lift_zero : forall P d c, closed_caproc P -> lift_caproc d c P = P.
Proof.
  unfold closed_caproc. intros. apply closed_caproc_lift.
  eapply closed_caproc_at_mono; [| eassumption]. lia.
Qed.
Lemma closed_st_subst_zero : forall T k N, closed_st T -> subst_st T k N = T.
Proof.
  unfold closed_st. intros. apply closed_st_subst.
  eapply closed_st_at_mono; [| eassumption]. lia.
Qed.
Lemma closed_st_lift_zero : forall T d c, closed_st T -> lift_st d c T = T.
Proof.
  unfold closed_st. intros. apply closed_st_lift.
  eapply closed_st_at_mono; [| eassumption]. lia.
Qed.

(* ── Section 7: closedness introduction lemmas ──────────────────────────── *)

Lemma closed_CPNil : closed_caproc CPNil.
Proof. unfold closed_caproc. simpl. exact I. Qed.
Lemma closed_CPDeref : forall x, closed_caname x -> closed_caproc (CPDeref x).
Proof. unfold closed_caproc, closed_caname. intros. simpl. assumption. Qed.
Lemma closed_CQuote : forall T, closed_st T -> closed_caname (CQuote T).
Proof. unfold closed_caname, closed_st. intros. simpl. assumption. Qed.
Lemma closed_CPPar : forall P Q,
  closed_caproc P -> closed_caproc Q -> closed_caproc (CPPar P Q).
Proof. unfold closed_caproc. intros. simpl. split; assumption. Qed.
Lemma closed_CPOutput : forall x U,
  closed_caname x -> closed_st U -> closed_caproc (CPOutput x U).
Proof. unfold closed_caproc, closed_caname, closed_st. intros. simpl. split; assumption. Qed.
Lemma closed_CPInput : forall x T,
  closed_caname x -> closed_st_at 1 T -> closed_caproc (CPInput x T).
Proof. unfold closed_caproc, closed_caname. intros. simpl. split; assumption. Qed.
Lemma closed_STSigned : forall P s, closed_caproc P -> closed_st (STSigned P s).
Proof. unfold closed_caproc, closed_st. intros. simpl. assumption. Qed.
Lemma closed_STStack : forall t, closed_st (STStack t).
Proof. unfold closed_st. intros. simpl. exact I. Qed.

(* ── Section 8: dequotation preserves closedness ─────────────────────────────
   [st_to_proc] only re-tags the process content of a closed term, so the
   dequoted residue is closed at the same level. Used by the join's
   strong-normalization bridge (a closed payload injects no free dereferences). *)
Lemma closed_st_to_proc : forall U k, closed_st_at k U -> closed_caproc_at k (st_to_proc U).
Proof.
  induction U as [P s | U1 IH1 U2 IH2 | t]; intros k H; simpl in *.
  - exact H.
  - destruct H as [H1 H2]. split; [ apply IH1 | apply IH2 ]; assumption.
  - exact I.
Qed.

(* ── Section 9: decidable (boolean) closedness ───────────────────────────────
   A boolean mirror of [closed_*_at], guard-friendly (the channel list uses the
   same inlined [fold_right] as the Prop predicate, NOT [forallb], to keep the
   mutual fixpoint structurally decreasing). The reflection [closed_b_spec_ca]
   lets the finite-image successor enumeration (CAGradedSuccPairs) DECIDE the
   join's closed-payload side-condition. *)
Fixpoint closed_caprocb (k : nat) (P : caproc) : bool :=
  match P with
  | CPNil         => true
  | CPInput x T   => closed_canameb k x && closed_stb (S k) T
  | CPOutput x U  => closed_canameb k x && closed_stb k U
  | CPPar P1 P2   => closed_caprocb k P1 && closed_caprocb k P2
  | CPDeref x     => closed_canameb k x
  | CPJoin xs T   =>
      fold_right (fun x acc => closed_canameb k x && acc) true xs
      && closed_stb (length xs + k) T
  end
with closed_canameb (k : nat) (x : caname) : bool :=
  match x with
  | CQuote T => closed_stb k T
  | CNVar j  => Nat.ltb j k
  end
with closed_stb (k : nat) (T : signed_term) : bool :=
  match T with
  | STSigned P _ => closed_caprocb k P
  | STPar T1 T2  => closed_stb k T1 && closed_stb k T2
  | STStack _    => true
  end.

(* The boolean channel-fold reflects the Prop channel-fold, element-wise. *)
Lemma fold_right_closedb_iff : forall k xs,
  (forall x, In x xs -> (closed_canameb k x = true <-> closed_caname_at k x)) ->
  (fold_right (fun x acc => closed_canameb k x && acc) true xs = true
   <-> fold_right (fun x acc => closed_caname_at k x /\ acc) True xs).
Proof.
  intros k xs Hx. induction xs as [| x xs' IH]; simpl.
  - split; intros _; [ exact I | reflexivity ].
  - rewrite andb_true_iff. rewrite (Hx x (or_introl eq_refl)).
    rewrite IH by (intros y Hy; apply Hx; right; exact Hy). reflexivity.
Qed.

Lemma closed_b_spec_ca :
  (forall P k, closed_caprocb k P = true <-> closed_caproc_at k P)
  /\ (forall x k, closed_canameb k x = true <-> closed_caname_at k x)
  /\ (forall T k, closed_stb k T = true <-> closed_st_at k T).
Proof.
  apply ca_deep_ind; intros; simpl.
  - split; intros _; [ exact I | reflexivity ].
  - rewrite andb_true_iff, H, H0. reflexivity.
  - rewrite andb_true_iff, H, H0. reflexivity.
  - rewrite andb_true_iff, H, H0. reflexivity.
  - apply H.
  - (* CPJoin xs T *) rewrite andb_true_iff, H0. rewrite fold_right_closedb_iff.
    + reflexivity.
    + rewrite Forall_forall in H. intros x Hin. apply H. exact Hin.
  - apply H.
  - apply Nat.ltb_lt.
  - apply H.
  - rewrite andb_true_iff, H, H0. reflexivity.
  - split; intros _; [ exact I | reflexivity ].
Qed.

Lemma closed_stb_spec : forall T k, closed_stb k T = true <-> closed_st_at k T.
Proof. apply closed_b_spec_ca. Qed.

(* Whole-list closedness (level 0) reflects [Forall closed_st], for the join's
   closed-payload enumeration side-condition. *)
Lemma forallb_closed_st_iff : forall Us,
  forallb (fun U => closed_stb 0 U) Us = true <-> Forall closed_st Us.
Proof.
  induction Us as [| U Us' IH]; simpl.
  - split; [ intros _; constructor | reflexivity ].
  - rewrite andb_true_iff, IH. unfold closed_st. rewrite closed_stb_spec.
    split.
    + intros [HU HUs]. constructor; assumption.
    + intros HF. inversion HF; subst. split; assumption.
Qed.

(* Closedness is decidable — the side-condition the join's successor enumeration
   (CAGradedSuccPairs) checks before emitting a [g_join1] transition. *)
Lemma closed_st_dec : forall T, {closed_st T} + {~ closed_st T}.
Proof.
  intro T. unfold closed_st. destruct (closed_stb 0 T) eqn:E.
  - left. apply closed_stb_spec. exact E.
  - right. intro H. apply closed_stb_spec in H. rewrite H in E. discriminate.
Qed.
