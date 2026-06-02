(* ════════════════════════════════════════════════════════════════════════
   CATranslationLemmas.v — native locally-nameless infrastructure for the
   translation faithfulness proof (Stage 4b, design doc §3 module 1).

   These are PURE CASyntax facts (no translation, no Section): the de Bruijn
   commutation lemmas the faithfulness proof's depth-aware commutation (L3) and
   gate-unwrap (L5) consume.

   - L2  : st_to_proc (the dequote-collapse target) commutes with lift / subst.
   - lift_lift_comm : the general lift/lift permutation (cutoffs c2 ≤ c1).
   - L1  : the lift/subst commutation RhoSyntax lacks, native 3-way mutual.

   All by [ca_mutind] mutual induction. Axiom-free.                            *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.

(* ── L2: st_to_proc commutes with lift and subst ────────────────────────── *)

Lemma lift_st_to_proc : forall T d c,
  lift_caproc d c (st_to_proc T) = st_to_proc (lift_st d c T).
Proof.
  induction T as [P s | T1 IH1 T2 IH2 | t]; intros d c; simpl.
  - reflexivity.
  - rewrite IH1, IH2; reflexivity.
  - reflexivity.
Qed.

Lemma subst_st_to_proc : forall T n N,
  subst_caproc (st_to_proc T) n N = st_to_proc (subst_st T n N).
Proof.
  induction T as [P s | T1 IH1 T2 IH2 | t]; intros n N; simpl.
  - reflexivity.
  - rewrite IH1, IH2; reflexivity.
  - reflexivity.
Qed.

(* ── The general lift/lift permutation (cutoffs c2 ≤ c1) ─────────────────── *)

Lemma lift_lift_comm :
  (forall P d1 c1 d2 c2, c2 <= c1 ->
     lift_caproc d1 (c1 + d2) (lift_caproc d2 c2 P) = lift_caproc d2 c2 (lift_caproc d1 c1 P))
  /\ (forall x d1 c1 d2 c2, c2 <= c1 ->
     lift_caname d1 (c1 + d2) (lift_caname d2 c2 x) = lift_caname d2 c2 (lift_caname d1 c1 x))
  /\ (forall T d1 c1 d2 c2, c2 <= c1 ->
     lift_st d1 (c1 + d2) (lift_st d2 c2 T) = lift_st d2 c2 (lift_st d1 c1 T)).
Proof.
  apply ca_mutind.
  - (* CPNil *) intros; reflexivity.
  - (* CPInput x T *)
    intros x IHx T IHT d1 c1 d2 c2 Hle; simpl; f_equal.
    + apply IHx; assumption.
    + replace (S (c1 + d2)) with ((S c1) + d2) by lia. apply IHT; lia.
  - (* CPOutput x U *)
    intros x IHx U IHU d1 c1 d2 c2 Hle; simpl; f_equal.
    + apply IHx; assumption.
    + apply IHU; assumption.
  - (* CPPar *)
    intros P1 IH1 P2 IH2 d1 c1 d2 c2 Hle; simpl; f_equal; [ apply IH1 | apply IH2 ]; assumption.
  - (* CPDeref x *)
    intros x IHx d1 c1 d2 c2 Hle; simpl; f_equal; apply IHx; assumption.
  - (* CQuote T *)
    intros T IHT d1 c1 d2 c2 Hle; simpl; f_equal; apply IHT; assumption.
  - (* CNVar k *)
    intros k d1 c1 d2 c2 Hle.
    repeat (simpl; match goal with |- context[?a <=? ?b] => destruct (a <=? b) eqn:?Hleb end);
    repeat match goal with
           | H : (_ <=? _) = true |- _ => apply Nat.leb_le in H
           | H : (_ <=? _) = false |- _ => apply Nat.leb_gt in H
           end;
    try (f_equal; lia); try lia; try reflexivity.
  - (* STSigned P s *)
    intros P IHP s d1 c1 d2 c2 Hle; simpl; f_equal; apply IHP; assumption.
  - (* STPar *)
    intros T1 IH1 T2 IH2 d1 c1 d2 c2 Hle; simpl; f_equal; [ apply IH1 | apply IH2 ]; assumption.
  - (* STStack t *)
    intros t d1 c1 d2 c2 Hle; reflexivity.
Qed.

(* The 0-cutoff instance used at binders: lift past (S c) commutes with lift 1 0. *)
Lemma lift_lift_0_caname : forall x d c,
  lift_caname d (S c) (lift_caname 1 0 x) = lift_caname 1 0 (lift_caname d c x).
Proof.
  intros x d c.
  replace (S c) with (c + 1) by lia.
  apply (proj1 (proj2 lift_lift_comm)). lia.
Qed.

(* ── L1: the native lift/subst commutation (cutoff c ≤ index n) ──────────── *)

Lemma lift_subst_ca :
  (forall P d c n N, c <= n ->
     lift_caproc d c (subst_caproc P n N) = subst_caproc (lift_caproc d c P) (n + d) (lift_caname d c N))
  /\ (forall x d c n N, c <= n ->
     lift_caname d c (subst_caname x n N) = subst_caname (lift_caname d c x) (n + d) (lift_caname d c N))
  /\ (forall T d c n N, c <= n ->
     lift_st d c (subst_st T n N) = subst_st (lift_st d c T) (n + d) (lift_caname d c N)).
Proof.
  apply ca_mutind.
  - (* CPNil *) intros; reflexivity.
  - (* CPInput x T *)
    intros x IHx T IHT d c n N Hle; simpl; f_equal.
    + apply IHx; assumption.
    + rewrite IHT by lia.
      replace (S n + d) with (S (n + d)) by lia.
      rewrite lift_lift_0_caname. reflexivity.
  - (* CPOutput x U *)
    intros x IHx U IHU d c n N Hle; simpl; f_equal.
    + apply IHx; assumption.
    + apply IHU; assumption.
  - (* CPPar *)
    intros P1 IH1 P2 IH2 d c n N Hle; simpl; f_equal; [ apply IH1 | apply IH2 ]; assumption.
  - (* CPDeref x *)
    intros x IHx d c n N Hle.
    destruct x as [T | k].
    + (* CQuote T *) simpl; f_equal; apply IHx; assumption.
    + (* CNVar k *) simpl.
      destruct (Nat.compare k n) eqn:Ecmp.
      * (* Eq: k = n *)
        apply Nat.compare_eq in Ecmp; subst k.
        replace (c <=? n) with true by (symmetry; apply Nat.leb_le; lia).
        replace (Nat.compare (n + d) (n + d)) with Eq by (symmetry; apply Nat.compare_eq_iff; reflexivity).
        destruct N as [U | m]; simpl.
        -- apply lift_st_to_proc.
        -- destruct (c <=? m); reflexivity.
      * (* Lt: k < n — no dequote *)
        apply Nat.compare_lt_iff in Ecmp;
        repeat (simpl; match goal with
          | |- context[?a <=? ?b] => destruct (a <=? b) eqn:?Hl
          | |- context[Nat.compare ?a ?b] => destruct (Nat.compare a b) eqn:?Hc
          end);
        repeat match goal with
          | H : (_ <=? _) = true |- _ => apply Nat.leb_le in H
          | H : (_ <=? _) = false |- _ => apply Nat.leb_gt in H
          | H : Nat.compare _ _ = Lt |- _ => apply Nat.compare_lt_iff in H
          | H : Nat.compare _ _ = Eq |- _ => apply Nat.compare_eq in H
          | H : Nat.compare _ _ = Gt |- _ => apply Nat.compare_gt_iff in H
          end;
        try (exfalso; lia); try (f_equal; f_equal; lia); try reflexivity; try lia.
      * (* Gt: k > n — no dequote *)
        apply Nat.compare_gt_iff in Ecmp;
        repeat (simpl; match goal with
          | |- context[?a <=? ?b] => destruct (a <=? b) eqn:?Hl
          | |- context[Nat.compare ?a ?b] => destruct (Nat.compare a b) eqn:?Hc
          end);
        repeat match goal with
          | H : (_ <=? _) = true |- _ => apply Nat.leb_le in H
          | H : (_ <=? _) = false |- _ => apply Nat.leb_gt in H
          | H : Nat.compare _ _ = Lt |- _ => apply Nat.compare_lt_iff in H
          | H : Nat.compare _ _ = Eq |- _ => apply Nat.compare_eq in H
          | H : Nat.compare _ _ = Gt |- _ => apply Nat.compare_gt_iff in H
          end;
        try (exfalso; lia); try (f_equal; f_equal; lia); try reflexivity; try lia.
  - (* CQuote T *)
    intros T IHT d c n N Hle; simpl; f_equal; apply IHT; assumption.
  - (* CNVar k *)
    intros k d c n N Hle;
    repeat (simpl; match goal with
      | |- context[?a <=? ?b] => destruct (a <=? b) eqn:?Hl
      | |- context[Nat.compare ?a ?b] => destruct (Nat.compare a b) eqn:?Hc
      end);
    repeat match goal with
      | H : (_ <=? _) = true |- _ => apply Nat.leb_le in H
      | H : (_ <=? _) = false |- _ => apply Nat.leb_gt in H
      | H : Nat.compare _ _ = Lt |- _ => apply Nat.compare_lt_iff in H
      | H : Nat.compare _ _ = Eq |- _ => apply Nat.compare_eq in H
      | H : Nat.compare _ _ = Gt |- _ => apply Nat.compare_gt_iff in H
      end;
    try (exfalso; lia); try (f_equal; lia); try reflexivity; try lia.
  - (* STSigned P s *)
    intros P IHP s d c n N Hle; simpl; f_equal; apply IHP; assumption.
  - (* STPar *)
    intros T1 IH1 T2 IH2 d c n N Hle; simpl; f_equal; [ apply IH1 | apply IH2 ]; assumption.
  - (* STStack t *)
    intros t d c n N Hle; reflexivity.
Qed.
