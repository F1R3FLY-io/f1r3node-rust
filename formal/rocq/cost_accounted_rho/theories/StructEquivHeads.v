(* ═══════════════════════════════════════════════════════════════════════════
   StructEquivHeads.v — Heads-list permutation machinery for ≡
   ═══════════════════════════════════════════════════════════════════════════

   The standard π-calculus mechanization technique: structural equivalence
   on processes is characterized by pairwise-≡-related permutations of
   "heads lists" (the top-level non-PPar/non-PNil components after
   flattening). This file provides the headline lemma
   [struct_equiv_heads_perm] and derives the inversion lemmas needed for
   bidirectional bisimulation and per-step reverse simulation.

   Required by:
   - [Bisimulation.v] Closure C IDEAL: full bidirectional bisimilarity.
   - [TranslationFaithfulness.v] Closure D IDEAL: per-step reverse
     simulation.

   Dependencies: RhoSyntax, StructEquivInversion, Stdlib lists/permutation
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import StructEquivInversion.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: The Heads List
   ═══════════════════════════════════════════════════════════════════════════ *)

Fixpoint heads (P : proc) : list proc :=
  match P with
  | PNil          => []
  | PInput _ _    => [P]
  | POutput _ _   => [P]
  | PDeref _      => [P]
  | PPar P1 P2    => heads P1 ++ heads P2
  | PReplicate _  => [P]
  end.

Lemma heads_length_eq_head_count : forall P,
  length (heads P) = head_count P.
Proof.
  induction P; simpl; auto.
  - (* PPar *) rewrite app_length. lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: List Equivalence (Pointwise ≡)
   ═══════════════════════════════════════════════════════════════════════════ *)

Inductive list_equiv : list proc -> list proc -> Prop :=
  | le_nil  : list_equiv [] []
  | le_cons : forall a b xs ys,
      a ≡ b ->
      list_equiv xs ys ->
      list_equiv (a :: xs) (b :: ys).

Lemma list_equiv_refl : forall xs, list_equiv xs xs.
Proof.
  induction xs; constructor; [apply se_refl | assumption].
Qed.

Lemma list_equiv_sym : forall xs ys,
  list_equiv xs ys -> list_equiv ys xs.
Proof.
  intros xs ys H. induction H.
  - constructor.
  - constructor; [apply se_sym; assumption | assumption].
Qed.

Lemma list_equiv_trans : forall xs ys zs,
  list_equiv xs ys -> list_equiv ys zs -> list_equiv xs zs.
Proof.
  intros xs ys zs Hxy. revert zs.
  induction Hxy; intros zs Hyz.
  - assumption.
  - inversion Hyz; subst.
    constructor; [eapply se_trans; eauto | apply IHHxy; assumption].
Qed.

Lemma list_equiv_app : forall a c b d,
  list_equiv a c -> list_equiv b d ->
  list_equiv (a ++ b) (c ++ d).
Proof.
  intros a c b d Hac Hbd.
  induction Hac; simpl; [assumption | constructor; assumption].
Qed.

Lemma list_equiv_length : forall xs ys,
  list_equiv xs ys -> length xs = length ys.
Proof.
  intros xs ys H. induction H; simpl; auto.
Qed.

(* App-inversion for list_equiv: if [a ++ b] is pointwise-≡ to some list
   zs, then zs splits at the |a| boundary into [za ++ zb] with the two
   halves individually ≡. Used to decompose per-arm heads of a PPar. *)
Lemma list_equiv_app_inv : forall a b zs,
  list_equiv (a ++ b) zs ->
  exists za zb, zs = za ++ zb /\ list_equiv a za /\ list_equiv b zb.
Proof.
  intros a. induction a as [|h t IH]; intros b zs H.
  - (* a = []: a ++ b = b. list_equiv b zs. Take za = [], zb = zs. *)
    simpl in H.
    exists [], zs. split; [reflexivity | split; [constructor | exact H]].
  - (* a = h :: t: (h :: t) ++ b = h :: (t ++ b). *)
    simpl in H. inversion H as [| a0 b0 xs ys Hab Hrest]; subst.
    destruct (IH b ys Hrest) as [zat [zb [Heq_ys [Hle_t Hle_b]]]].
    exists (b0 :: zat), zb. split.
    + simpl. f_equal. exact Heq_ys.
    + split; [constructor; assumption | exact Hle_b].
Qed.

(* Pointwise ≡ transports membership: if list_equiv xs ys and a ∈ xs,
   then there is some b ∈ ys with a ≡ b. Used to locate an element
   across a list_equiv. *)
Lemma list_equiv_in_transport : forall xs ys a,
  list_equiv xs ys -> In a xs -> exists b, In b ys /\ a ≡ b.
Proof.
  intros xs ys a Hle HIn.
  induction Hle as [|x y xs' ys' Hxy Hrest IH].
  - inversion HIn.
  - destruct HIn as [Heq | HIn_tail].
    + subst x. exists y. split; [left; reflexivity | exact Hxy].
    + destruct (IH HIn_tail) as [b [HIn_b Hab]].
      exists b. split; [right; exact HIn_b | exact Hab].
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Commutation of List Equivalence and Permutation
   ═══════════════════════════════════════════════════════════════════════════ *)

(* The "zigzag" lemma: composing pointwise-≡ with permutation can be
   reordered. From [xs ~= ys] and [ys ~P zs], derive intermediate [us]
   with [xs ~P us] and [us ~= zs]. Proved by induction on the
   permutation derivation.                                                  *)
Lemma list_equiv_Permutation_commute :
  forall xs ys zs,
    list_equiv xs ys ->
    Permutation ys zs ->
    exists us, Permutation xs us /\ list_equiv us zs.
Proof.
  intros xs ys zs Hle Hperm.
  revert xs Hle.
  induction Hperm; intros xs Hle.
  - (* perm_nil: ys = [], zs = []. Hle : list_equiv xs []. So xs = []. *)
    inversion Hle. subst.
    exists []. split; [apply Permutation_refl | constructor].
  - (* perm_skip x l l': ys = x :: l, zs = x :: l'. *)
    inversion Hle as [|x_xs y_ys xs_rest ys_rest Hxy_eq Hrest_eq Hsplit Hyseq];
      subst.
    destruct (IHHperm xs_rest Hrest_eq) as [us [Hperm_us Hle_us]].
    exists (x_xs :: us). split.
    + apply perm_skip; assumption.
    + constructor; assumption.
  - (* perm_swap x y l: ys = y :: x :: l, zs = x :: y :: l. *)
    inversion Hle as [|y_xs y_ys xs_tail ys_tail H_y_eq Hrest_eq Hsplit Hyseq];
      subst.
    inversion Hrest_eq as [|x_xs x_ys xs_inner ys_inner H_x_eq Hinner_eq Hsplit2 Hyseq2];
      subst.
    exists (x_xs :: y_xs :: xs_inner). split.
    + apply perm_swap.
    + constructor; [exact H_x_eq |].
      constructor; [exact H_y_eq | exact Hinner_eq].
  - (* perm_trans: l ~P l' ~P l''. *)
    destruct (IHHperm1 xs Hle) as [us1 [Hperm_us1 Hle_us1]].
    destruct (IHHperm2 us1 Hle_us1) as [us2 [Hperm_us2 Hle_us2]].
    exists us2. split; [eapply Permutation_trans; eassumption | assumption].
Qed.

(* The dual: from a permutation followed by pointwise-≡, derive
   pointwise-≡ followed by a permutation. Derived from the commute lemma
   via symmetry. *)
Lemma Permutation_list_equiv_commute :
  forall xs ys zs,
    Permutation xs ys ->
    list_equiv ys zs ->
    exists us, list_equiv xs us /\ Permutation us zs.
Proof.
  intros xs ys zs Hperm Hle.
  apply list_equiv_sym in Hle.
  apply Permutation_sym in Hperm.
  destruct (list_equiv_Permutation_commute _ _ _ Hle Hperm)
    as [us [Hperm' Hle']].
  exists us. split.
  - apply list_equiv_sym. exact Hle'.
  - apply Permutation_sym. exact Hperm'.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Permutation-Equivalence (≡-aware permutation)
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition perm_equiv (xs ys : list proc) : Prop :=
  exists zs, list_equiv xs zs /\ Permutation zs ys.

Lemma perm_equiv_refl : forall xs, perm_equiv xs xs.
Proof.
  intros xs. exists xs. split; [apply list_equiv_refl | apply Permutation_refl].
Qed.

Lemma perm_equiv_sym : forall xs ys,
  perm_equiv xs ys -> perm_equiv ys xs.
Proof.
  intros xs ys [zs [Hle Hperm]].
  (* Hle : list_equiv xs zs, Hperm : Permutation zs ys *)
  (* Goal: exists w, list_equiv ys w /\ Permutation w xs *)
  (* By Permutation_sym on Hperm: Permutation ys zs *)
  apply Permutation_sym in Hperm.
  (* Apply Permutation_list_equiv_commute on Hperm and (list_equiv zs xs)
     which is sym of Hle. *)
  destruct (Permutation_list_equiv_commute _ _ _ Hperm (list_equiv_sym _ _ Hle))
    as [us [Hle_us Hperm_us]].
  exists us. split; assumption.
Qed.

Lemma perm_equiv_trans : forall xs ys zs,
  perm_equiv xs ys -> perm_equiv ys zs -> perm_equiv xs zs.
Proof.
  intros xs ys zs [ws1 [Hle1 Hperm1]] [ws2 [Hle2 Hperm2]].
  (* Hle1: list_equiv xs ws1, Hperm1: Permutation ws1 ys *)
  (* Hle2: list_equiv ys ws2, Hperm2: Permutation ws2 zs *)
  (* Use Permutation_list_equiv_commute on Hperm1 (ws1 ~P ys) and Hle2 (ys ~= ws2) *)
  destruct (Permutation_list_equiv_commute _ _ _ Hperm1 Hle2)
    as [w3 [Hle3 Hperm3]].
  (* Hle3 : list_equiv ws1 w3, Hperm3 : Permutation w3 ws2 *)
  exists w3. split.
  - eapply list_equiv_trans; eassumption.
  - eapply Permutation_trans; eassumption.
Qed.

Lemma perm_equiv_app : forall a c b d,
  perm_equiv a c -> perm_equiv b d ->
  perm_equiv (a ++ b) (c ++ d).
Proof.
  intros a c b d [za [Hlea Hperma]] [zb [Hleb Hpermb]].
  exists (za ++ zb). split.
  - apply list_equiv_app; assumption.
  - apply Permutation_app; assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 5: Heads Permutation Theorem
   ═══════════════════════════════════════════════════════════════════════════

   The headline result: structurally-equivalent processes have
   permutation-equivalent heads lists. Proof by induction on the [≡]
   derivation; each rule maps to a permutation or pointwise-equivalence
   step on heads.                                                          *)

Lemma struct_equiv_heads_perm :
  forall P Q, P ≡ Q -> perm_equiv (heads P) (heads Q).
Proof.
  intros P Q Heq. induction Heq.
  - (* se_refl P *) apply perm_equiv_refl.
  - (* se_sym P Q (P ≡ Q from Q ≡ P) *) apply perm_equiv_sym; assumption.
  - (* se_trans *) eapply perm_equiv_trans; eassumption.
  - (* se_par_comm P Q : PPar P Q ≡ PPar Q P *)
    simpl. exists (heads P ++ heads Q). split.
    + apply list_equiv_refl.
    + apply Permutation_app_comm.
  - (* se_par_assoc P Q R : PPar (PPar P Q) R ≡ PPar P (PPar Q R) *)
    simpl. rewrite app_assoc.
    apply perm_equiv_refl.
  - (* se_par_nil P : PPar P PNil ≡ P *)
    simpl. rewrite app_nil_r. apply perm_equiv_refl.
  - (* se_par_cong P P' Q Q' : P ≡ P' -> Q ≡ Q' -> PPar P Q ≡ PPar P' Q' *)
    simpl. apply perm_equiv_app; assumption.
  - (* se_input_cong x x' P P' : x ≡N x' -> P ≡ P' -> PInput x P ≡ PInput x' P' *)
    simpl. exists [PInput x' P']. split.
    + constructor; [apply se_input_cong; assumption | constructor].
    + apply Permutation_refl.
  - (* se_output_cong *)
    simpl. exists [POutput x' Q']. split.
    + constructor; [apply se_output_cong; assumption | constructor].
    + apply Permutation_refl.
  - (* se_deref_cong *)
    simpl. exists [PDeref x']. split.
    + constructor; [apply se_deref_cong; assumption | constructor].
    + apply Permutation_refl.
  - (* se_replicate_cong *)
    simpl. exists [PReplicate P']. split.
    + constructor; [apply se_replicate_cong; assumption | constructor].
    + apply Permutation_refl.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 6: Heads-to-Process Reconstruction
   ═══════════════════════════════════════════════════════════════════════════

   The inverse direction: any list of "head shapes" can be flattened back
   into a process via right-leaning PPar nesting. The key fact is that
   [heads_to_proc (heads P) ≡ P], i.e., flattening then reconstructing
   yields a structurally-equivalent process.                                *)

Fixpoint heads_to_proc (L : list proc) : proc :=
  match L with
  | [] => PNil
  | h :: rest => PPar h (heads_to_proc rest)
  end.

Lemma heads_to_proc_app : forall L1 L2,
  heads_to_proc (L1 ++ L2) ≡ PPar (heads_to_proc L1) (heads_to_proc L2).
Proof.
  induction L1 as [|h L1' IH]; intros L2.
  - simpl. apply se_sym, se_nil_par.
  - simpl. eapply se_trans.
    + apply se_par_cong_r. apply IH.
    + apply se_sym, se_par_assoc.
Qed.

Lemma heads_to_proc_heads_se : forall P,
  heads_to_proc (heads P) ≡ P.
Proof.
  induction P; simpl.
  - apply se_refl.
  - apply se_par_nil.
  - apply se_par_nil.
  - eapply se_trans.
    + apply heads_to_proc_app.
    + apply se_par_cong; assumption.
  - apply se_par_nil.
  - (* PReplicate *) apply se_par_nil.
Qed.

Lemma heads_to_proc_list_equiv : forall L1 L2,
  list_equiv L1 L2 -> heads_to_proc L1 ≡ heads_to_proc L2.
Proof.
  intros L1 L2 H. induction H; simpl.
  - apply se_refl.
  - apply se_par_cong; assumption.
Qed.

Lemma heads_to_proc_Permutation : forall L1 L2,
  Permutation L1 L2 -> heads_to_proc L1 ≡ heads_to_proc L2.
Proof.
  intros L1 L2 H. induction H; simpl.
  - apply se_refl.
  - apply se_par_cong; [apply se_refl | assumption].
  - (* perm_swap x y l *)
    eapply se_trans.
    + apply se_sym, se_par_assoc.
    + eapply se_trans.
      * apply se_par_cong_l. apply se_par_comm.
      * apply se_par_assoc.
  - (* perm_trans *)
    eapply se_trans; eassumption.
Qed.

(* The key bridge: a [perm_equiv] of heads lists implies structural
   equivalence of the corresponding [heads_to_proc] reconstructions. *)
Lemma heads_to_proc_perm_equiv : forall L1 L2,
  perm_equiv L1 L2 -> heads_to_proc L1 ≡ heads_to_proc L2.
Proof.
  intros L1 L2 [zs [Hle Hperm]].
  eapply se_trans.
  - apply heads_to_proc_list_equiv. exact Hle.
  - apply heads_to_proc_Permutation. exact Hperm.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 7: PDeref Inversion via ≡
   ═══════════════════════════════════════════════════════════════════════════

   Specific inversion: a process structurally equivalent to [PDeref n]
   must itself be a [PDeref n'] for some [n' ≡N n]. Proved by induction
   on the [≡] derivation in BOTH directions simultaneously to avoid the
   se_sym case getting stuck.                                              *)

(* The PDeref-inversion lemma. Uses ≡ in the conclusion (not =) because
   ≡ does not preserve syntactic equality (e.g., PPar (PDeref n) PNil ≡
   PDeref n). The two-direction form handles the se_sym case cleanly. *)
Lemma se_PDeref_inv_both :
  forall P R, P ≡ R ->
    (forall n, P = PDeref n ->
       exists n', R ≡ PDeref n' /\ n ≡N n') /\
    (forall n, R = PDeref n ->
       exists n', P ≡ PDeref n' /\ n' ≡N n).
Proof.
  intros P R Heq. induction Heq.
  - (* se_refl P *)
    split.
    + intros n0 Heqp. subst. exists n0. split; [apply se_refl | apply se_name_refl].
    + intros n0 Heqp. subst. exists n0. split; [apply se_refl | apply se_name_refl].
  - (* se_sym P Q (P0 ≡ Q0 from Q0 ≡ P0) *)
    destruct IHHeq as [Hfwd Hbwd].
    split; intros n0 Heqp.
    + destruct (Hbwd n0 Heqp) as [n' [HeqP Hname]].
      exists n'. split; [exact HeqP | apply se_name_sym; exact Hname].
    + destruct (Hfwd n0 Heqp) as [n' [HeqQ Hname]].
      exists n'. split; [exact HeqQ | apply se_name_sym; exact Hname].
  - (* se_trans P Q R *)
    destruct IHHeq1 as [Hfwd1 Hbwd1].
    destruct IHHeq2 as [Hfwd2 Hbwd2].
    split; intros n0 Heqp.
    + destruct (Hfwd1 n0 Heqp) as [n_q [HeqQ Hn1]].
      (* HeqQ : Q ≡ PDeref n_q. Apply Hfwd2: forall n, Q = PDeref n -> ...
         But we have Q ≡ PDeref n_q, not Q = PDeref n_q. So we need to
         compose differently — apply Hbwd2 backwards via se_sym. *)
      (* Strategy: chain Q ≡ PDeref n_q with R ≡ PDeref n_r via the
         original transitive equivalence. *)
      exists n_q. split.
      * (* R ≡ PDeref n_q. We have Q ≡ PDeref n_q (HeqQ) and Q ≡ R (Heq2). *)
        eapply se_trans; [apply se_sym; eassumption | exact HeqQ].
      * exact Hn1.
    + destruct (Hbwd2 n0 Heqp) as [n_q [HeqQ Hn2]].
      exists n_q. split.
      * (* P ≡ PDeref n_q. We have Q ≡ PDeref n_q (HeqQ) and P ≡ Q (Heq1). *)
        eapply se_trans; eassumption.
      * exact Hn2.
  - (* se_par_comm P Q : PPar P Q ≡ PPar Q P *)
    split; intros n0 Heqp; discriminate.
  - (* se_par_assoc *)
    split; intros n0 Heqp; discriminate.
  - (* se_par_nil P : PPar P PNil ≡ P. P is generic — could be PDeref. *)
    split; intros n0 Heqp.
    + (* PPar P PNil = PDeref n0 — discriminate. *)
      discriminate.
    + (* P = PDeref n0 — possible. Then PPar (PDeref n0) PNil ≡ PDeref n0. *)
      subst. exists n0. split.
      * apply se_par_nil.
      * apply se_name_refl.
  - (* se_par_cong: PPar P Q ≡ PPar P' Q' *)
    split; intros n0 Heqp; discriminate.
  - (* se_input_cong *)
    split; intros n0 Heqp; discriminate.
  - (* se_output_cong *)
    split; intros n0 Heqp; discriminate.
  - (* se_deref_cong x x' : x ≡N x' -> PDeref x ≡ PDeref x'.
       The constructor binds: x x' : name, and H : x ≡N x'. *)
    rename H into Hxn.
    split; intros n0 Heqp.
    + (* P = PDeref x, R = PDeref x', P = PDeref n0 → x = n0.
         Goal: exists n', PDeref x' ≡ PDeref n' /\ n0 ≡N n'.
         Take n' := x'. Need n0 ≡N x'. After subst x = n0, Hxn : n0 ≡N x'. *)
      injection Heqp as Heqx. subst x.
      exists x'. split; [apply se_refl | exact Hxn].
    + (* P = PDeref x, R = PDeref x', R = PDeref n0 → x' = n0.
         Goal: exists n', PDeref x ≡ PDeref n' /\ n' ≡N n0.
         Take n' := x. Need x ≡N n0. After subst x' = n0, Hxn : x ≡N n0. *)
      injection Heqp as Heqx'. subst x'.
      exists x. split; [apply se_refl | exact Hxn].
  - (* se_replicate_cong: PReplicate P ≡ PReplicate P' *)
    split; intros n0 Heqp; discriminate.
Qed.

(* Convenience corollary: forward direction. *)
Lemma se_PDeref_inv :
  forall n R, PDeref n ≡ R -> exists n', R ≡ PDeref n' /\ n ≡N n'.
Proof.
  intros n R H.
  destruct (se_PDeref_inv_both _ _ H) as [Hfwd _].
  apply Hfwd. reflexivity.
Qed.

(* Convenience: backward direction. *)
Lemma se_PDeref_inv_rev :
  forall P n, P ≡ PDeref n -> exists n', P ≡ PDeref n' /\ n' ≡N n.
Proof.
  intros P n H.
  destruct (se_PDeref_inv_both _ _ H) as [_ Hbwd].
  apply Hbwd. reflexivity.
Qed.

(* The "is_head" predicate: a process is a head shape if it is NOT PPar
   and NOT PNil. The elements of [heads P] are always head shapes. *)
Definition is_head (h : proc) : Prop :=
  h <> PNil /\ (forall P1 P2, h <> PPar P1 P2).

(* Every element of [heads P] is a head shape. *)
Lemma heads_are_heads :
  forall P h, In h (heads P) -> is_head h.
Proof.
  induction P; intros h Hin; simpl in Hin; try contradiction.
  - (* PInput n p *)
    destruct Hin as [Heq | []]. subst h.
    split; [discriminate | intros; discriminate].
  - (* POutput n p *)
    destruct Hin as [Heq | []]. subst h.
    split; [discriminate | intros; discriminate].
  - (* PPar P1 P2 *)
    apply in_app_or in Hin. destruct Hin; auto.
  - (* PDeref n *)
    destruct Hin as [Heq | []]. subst h.
    split; [discriminate | intros; discriminate].
  - (* PReplicate P *)
    destruct Hin as [Heq | []]. subst h.
    split; [discriminate | intros; discriminate].
Qed.

(* The "only_deref" predicate: a process whose only top-level head is
   a single PDeref of the given name. This characterizes "structurally
   PDeref-shaped" processes that may have PNil padding. *)
Inductive only_deref : proc -> name -> Prop :=
  | OD_base  : forall n, only_deref (PDeref n) n
  | OD_par_l : forall P Q n,
      only_deref P n -> head_count Q = 0 -> only_deref (PPar P Q) n
  | OD_par_r : forall P Q n,
      head_count P = 0 -> only_deref Q n -> only_deref (PPar P Q) n.

(* The bidirectional "only_deref preserves under ≡" lemma. Proves that
   structural equivalence on processes that are "only-deref n" produces
   processes that are also "only-deref m" for an ≡N-related m. The key
   technical fact for [se_PDeref_inj] below.                            *)
Lemma onlyderef_se_both :
  forall P R, P ≡ R ->
  (forall n, only_deref P n -> exists m, only_deref R m /\ n ≡N m) /\
  (forall n, only_deref R n -> exists m, only_deref P m /\ n ≡N m).
Proof.
  intros P R Heq. induction Heq.
  - (* se_refl *)
    split; intros n0 Hod; exists n0; split;
      first [exact Hod | apply se_name_refl].
  - (* se_sym *)
    destruct IHHeq as [Hf Hb]. split; assumption.
  - (* se_trans *)
    destruct IHHeq1 as [Hf1 Hb1].
    destruct IHHeq2 as [Hf2 Hb2].
    split; intros n0 Hod.
    + destruct (Hf1 n0 Hod) as [nq [Hod_q Hn1]].
      destruct (Hf2 nq Hod_q) as [mr [Hod_r Hn2]].
      exists mr. split; [exact Hod_r | eapply se_name_trans; eassumption].
    + destruct (Hb2 n0 Hod) as [nq [Hod_q Hn1]].
      destruct (Hb1 nq Hod_q) as [mp [Hod_p Hn2]].
      exists mp. split; [exact Hod_p | eapply se_name_trans; eassumption].
  - (* se_par_comm *)
    split; intros n0 Hod; inversion Hod; subst.
    + exists n0. split; [apply OD_par_r; assumption | apply se_name_refl].
    + exists n0. split; [apply OD_par_l; assumption | apply se_name_refl].
    + exists n0. split; [apply OD_par_r; assumption | apply se_name_refl].
    + exists n0. split; [apply OD_par_l; assumption | apply se_name_refl].
  - (* se_par_assoc *)
    split.
    + intros n0 Hod. inversion Hod; subst.
      * match goal with
        | [ Hinner : only_deref (PPar P Q) n0 |- _ ] =>
            inversion Hinner; subst
        end.
        -- exists n0. split; [|apply se_name_refl].
           apply OD_par_l; [assumption | simpl; lia].
        -- exists n0. split; [|apply se_name_refl].
           apply OD_par_r; [assumption | apply OD_par_l; assumption].
      * match goal with
        | [ Hpz : head_count (PPar P Q) = 0 |- _ ] => simpl in Hpz
        end.
        exists n0. split; [|apply se_name_refl].
        apply OD_par_r; [lia | apply OD_par_r; [lia | assumption]].
    + intros n0 Hod. inversion Hod; subst.
      * match goal with
        | [ Hqz : head_count (PPar Q R) = 0 |- _ ] => simpl in Hqz
        end.
        exists n0. split; [|apply se_name_refl].
        apply OD_par_l; [apply OD_par_l; [assumption | lia] | lia].
      * match goal with
        | [ Hinner : only_deref (PPar Q R) n0 |- _ ] =>
            inversion Hinner; subst
        end.
        -- exists n0. split; [|apply se_name_refl].
           apply OD_par_l; [apply OD_par_r; assumption | assumption].
        -- exists n0. split; [|apply se_name_refl].
           apply OD_par_r; [simpl; lia | assumption].
  - (* se_par_nil *)
    split; intros n0 Hod.
    + inversion Hod; subst.
      * exists n0. split; [assumption | apply se_name_refl].
      * match goal with
        | [ Himp : only_deref PNil _ |- _ ] => inversion Himp
        end.
    + exists n0. split; [|apply se_name_refl].
      apply OD_par_l; [assumption | reflexivity].
  - (* se_par_cong *)
    destruct IHHeq1 as [Hf1 Hb1].
    destruct IHHeq2 as [Hf2 Hb2].
    pose proof (head_count_se _ _ Heq1) as Hhc1.
    pose proof (head_count_se _ _ Heq2) as Hhc2.
    split; intros n0 Hod; inversion Hod; subst.
    + match goal with
      | [ Hod_p : only_deref P n0 |- _ ] =>
          destruct (Hf1 n0 Hod_p) as [np [Hod_p' Hname]]
      end.
      exists np. split; [|exact Hname].
      apply OD_par_l; [exact Hod_p' | lia].
    + match goal with
      | [ Hod_q : only_deref Q n0 |- _ ] =>
          destruct (Hf2 n0 Hod_q) as [nq [Hod_q' Hname]]
      end.
      exists nq. split; [|exact Hname].
      apply OD_par_r; [lia | exact Hod_q'].
    + match goal with
      | [ Hod_p : only_deref P' n0 |- _ ] =>
          destruct (Hb1 n0 Hod_p) as [np [Hod_p' Hname]]
      end.
      exists np. split; [|exact Hname].
      apply OD_par_l; [exact Hod_p' | lia].
    + match goal with
      | [ Hod_q : only_deref Q' n0 |- _ ] =>
          destruct (Hb2 n0 Hod_q) as [nq [Hod_q' Hname]]
      end.
      exists nq. split; [|exact Hname].
      apply OD_par_r; [lia | exact Hod_q'].
  - (* se_input_cong *)
    split; intros n0 Hod; inversion Hod.
  - (* se_output_cong *)
    split; intros n0 Hod; inversion Hod.
  - (* se_deref_cong *)
    split; intros n0 Hod; inversion Hod; subst.
    + exists x'. split; [apply OD_base | assumption].
    + exists x. split; [apply OD_base | apply se_name_sym; assumption].
  - (* se_replicate_cong *)
    split; intros n0 Hod; inversion Hod.
Qed.

(* PDeref injectivity: if PDeref n ≡ PDeref m, then n ≡N m. The proof
   uses the "only_deref" predicate as a witness: PDeref n is only_deref n,
   which by [onlyderef_se_both] gives an only_deref of PDeref m, which
   by inversion forces the names to be ≡N-related. *)
Lemma se_PDeref_inj : forall n m, PDeref n ≡ PDeref m -> n ≡N m.
Proof.
  intros n m H.
  destruct (onlyderef_se_both _ _ H) as [Hf _].
  destruct (Hf n (OD_base n)) as [m' [Hod Hname]].
  inversion Hod; subst. exact Hname.
Qed.

(* If P ≡ a head-shape h with the structural constraint that h is not
   PPar/PNil, and P is a PDeref, then h is also (syntactically) a PDeref.
   This is the "PDeref-stays-PDeref-among-heads" lemma. *)
Lemma se_PDeref_to_head :
  forall n h,
    PDeref n ≡ h ->
    is_head h ->
    exists m, h = PDeref m /\ n ≡N m.
Proof.
  intros n h Hsym Hhead.
  destruct Hhead as [Hnnil Hnpar].
  pose proof (count_derefs_se _ _ Hsym) as Hcd.
  pose proof (head_count_se _ _ Hsym) as Hhc.
  simpl in Hcd, Hhc.
  destruct h as [|y B|y B|h1 h2|m|hP].
  - exfalso. apply Hnnil. reflexivity.
  - simpl in Hcd. lia.
  - simpl in Hcd. lia.
  - exfalso. eapply Hnpar. reflexivity.
  - exists m. split; [reflexivity |].
    apply se_PDeref_inj. exact Hsym.
  - (* PReplicate hP: count_derefs = 0 but count_derefs (PDeref n) = 1 *)
    simpl in Hcd. lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 8: List Snoc Inversion
   ═══════════════════════════════════════════════════════════════════════════ *)

Lemma list_equiv_snoc_inv :
  forall xs y ys,
    list_equiv (xs ++ [y]) ys ->
    exists xs' y',
      ys = xs' ++ [y'] /\
      list_equiv xs xs' /\
      y ≡ y'.
Proof.
  induction xs as [|h xs' IH]; intros y ys H.
  - (* xs = [], so xs ++ [y] = [y]. *)
    simpl in H. inversion H as [|a b xs0 ys0 Hab Hrest Hsplit Hyseq]; subst.
    inversion Hrest. subst.
    exists [], b. split; [reflexivity | split; [constructor | exact Hab]].
  - (* xs = h :: xs'. *)
    simpl in H. inversion H as [|a b zs0 ys0 Hab Hrest Hsplit Hyseq]; subst.
    destruct (IH y ys0 Hrest) as [xs'' [y'' [Heq_ys [Hle Hyy]]]].
    exists (b :: xs''), y''. split.
    + simpl. f_equal. exact Heq_ys.
    + split.
      * constructor; assumption.
      * exact Hyy.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 9: The Load-bearing Inversion Lemma — Stuck-Residue Extraction
   ═══════════════════════════════════════════════════════════════════════════

   The key inversion lemma: if a process is structurally equivalent to
   [PPar Q (PDeref (Quote PNil))], then the process can be split into a
   "Q-part" (≡ to Q) and a [PDeref n'] (with n' ≡N Quote PNil) part. This
   is the load-bearing fact for the backward simulation in Closure C.   *)

Lemma se_inv_par_pderef_unit :
  forall Q R,
    PPar Q (PDeref (Quote PNil)) ≡ R ->
    exists Q' n',
      R ≡ PPar Q' (PDeref n') /\
      Q ≡ Q' /\
      n' ≡N Quote PNil.
Proof.
  intros Q R Heq.
  pose proof (struct_equiv_heads_perm _ _ Heq) as Hperm_eq.
  destruct Hperm_eq as [zs [Hle Hperm]].
  (* heads (PPar Q (PDeref (Quote PNil))) = heads Q ++ [PDeref (Quote PNil)] *)
  simpl in Hle.
  (* Hle : list_equiv (heads Q ++ [PDeref (Quote PNil)]) zs *)
  apply list_equiv_snoc_inv in Hle.
  destruct Hle as [zs_pre [last_h [Heq_zs [Hpre Hlast]]]].
  subst zs.
  (* Hpre : list_equiv (heads Q) zs_pre
     Hlast : PDeref (Quote PNil) ≡ last_h *)
  (* Show last_h is a head shape (is_head) so we can apply
     se_PDeref_to_head to extract syntactic equality. *)
  assert (Hin_last : In last_h (heads R)).
  { eapply Permutation_in.
    - exact Hperm.
    - apply in_or_app. right. simpl. left. reflexivity. }
  pose proof (heads_are_heads R last_h Hin_last) as Hhead_last.
  apply (se_PDeref_to_head _ _ Hlast) in Hhead_last.
  destruct Hhead_last as [n' [Heq_last Hname]].
  subst last_h.
  (* Hperm : Permutation (zs_pre ++ [PDeref n']) (heads R) *)
  apply Permutation_sym in Hperm.
  (* Hperm : Permutation (heads R) (zs_pre ++ [PDeref n']) *)
  (* Use eapply to let Coq infer the args. *)
  edestruct Permutation_vs_elt_inv as [L1 [L2 Heq_R]]; [exact Hperm |].
  (* Heq_R : heads R = L1 ++ PDeref n' :: L2 *)
  rewrite Heq_R in Hperm.
  apply Permutation_app_inv with (a := PDeref n') in Hperm.
  (* Hperm : Permutation (L1 ++ L2) (zs_pre ++ []) *)
  rewrite app_nil_r in Hperm.
  (* Now construct the witness: Q' := heads_to_proc (L1 ++ L2) *)
  exists (heads_to_proc (L1 ++ L2)), n'.
  split; [|split].
  - (* R ≡ PPar (heads_to_proc (L1 ++ L2)) (PDeref n') *)
    eapply se_trans. { apply se_sym, heads_to_proc_heads_se. }
    rewrite Heq_R.
    eapply se_trans. { apply heads_to_proc_app. }
    (* heads_to_proc L1 | heads_to_proc (PDeref n' :: L2)
       = heads_to_proc L1 | (PDeref n' | heads_to_proc L2) *)
    simpl.
    (* (heads_to_proc L1) | (PDeref n' | heads_to_proc L2)
       want: heads_to_proc (L1 ++ L2) | PDeref n' *)
    eapply se_trans.
    { apply se_par_cong_r. apply se_par_comm. }
    (* (heads_to_proc L1) | (heads_to_proc L2 | PDeref n') *)
    eapply se_trans. { apply se_sym, se_par_assoc. }
    (* (heads_to_proc L1 | heads_to_proc L2) | PDeref n' *)
    apply se_par_cong_l.
    apply se_sym, heads_to_proc_app.
  - (* Q ≡ heads_to_proc (L1 ++ L2) *)
    eapply se_trans. { apply se_sym, heads_to_proc_heads_se. }
    eapply se_trans. { apply heads_to_proc_list_equiv. exact Hpre. }
    apply heads_to_proc_Permutation.
    apply Permutation_sym. exact Hperm.
  - (* n' ≡N Quote PNil *)
    apply se_name_sym. exact Hname.
Qed.

(* Convenience: only the "factored" form is needed for backward simulation. *)
Lemma se_par_stuck_extract :
  forall Q R,
    PPar Q (PDeref (Quote PNil)) ≡ R ->
    exists Q', R ≡ PPar Q' (PDeref (Quote PNil)) /\ Q ≡ Q'.
Proof.
  intros Q R Heq.
  destruct (se_inv_par_pderef_unit _ _ Heq) as [Q' [n' [HR [HQ Hname]]]].
  exists Q'. split.
  - eapply se_trans; [exact HR | apply se_par_cong_r; apply se_deref_cong; exact Hname].
  - exact HQ.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 10: PInput Constructor Injectivity
   ═══════════════════════════════════════════════════════════════════════════

   Analogous to [se_PDeref_inj], but for PInput. Uses an "only_input"
   predicate (with prefixed variables [ax, aB] to avoid collisions with
   the outer scope) and a symmetric bidirectional preservation lemma.
   The key trick: both halves of the conjunction use the same variable
   order so that [split; assumption] handles [se_sym] cleanly.            *)

Inductive only_input : proc -> name -> proc -> Prop :=
  | OI_base  : forall ax aB, only_input (PInput ax aB) ax aB
  | OI_par_l : forall aP aQ ax aB,
      only_input aP ax aB -> head_count aQ = 0 -> only_input (PPar aP aQ) ax aB
  | OI_par_r : forall aP aQ ax aB,
      head_count aP = 0 -> only_input aQ ax aB -> only_input (PPar aP aQ) ax aB.

Lemma onlyinput_se_both :
  forall P R, P ≡ R ->
  (forall aa bb, only_input P aa bb ->
     exists yy cc, only_input R yy cc /\ aa ≡N yy /\ bb ≡ cc) /\
  (forall aa bb, only_input R aa bb ->
     exists yy cc, only_input P yy cc /\ aa ≡N yy /\ bb ≡ cc).
Proof.
  intros P R Heq. induction Heq.
  - (* se_refl *)
    split; intros aa0 bb0 Hoi;
      exists aa0, bb0;
      (split; [exact Hoi
              | split; [apply se_name_refl | apply se_refl]]).
  - (* se_sym *)
    destruct IHHeq as [Hf Hb]. split; assumption.
  - (* se_trans *)
    destruct IHHeq1 as [Hf1 Hb1].
    destruct IHHeq2 as [Hf2 Hb2].
    split; intros aa0 bb0 Hoi.
    + destruct (Hf1 aa0 bb0 Hoi) as [yq [cq [Hoi_q [Hn1 Hbe1]]]].
      destruct (Hf2 yq cq Hoi_q) as [yr [cr [Hoi_r [Hn2 Hbe2]]]].
      exists yr, cr.
      split; [exact Hoi_r
             | split; [eapply se_name_trans; eassumption
                      | eapply se_trans; eassumption]].
    + destruct (Hb2 aa0 bb0 Hoi) as [yq [cq [Hoi_q [Hn1 Hbe1]]]].
      destruct (Hb1 yq cq Hoi_q) as [yp [cp [Hoi_p [Hn2 Hbe2]]]].
      exists yp, cp.
      split; [exact Hoi_p
             | split; [eapply se_name_trans; eassumption
                      | eapply se_trans; eassumption]].
  - (* se_par_comm *)
    split; intros aa0 bb0 Hoi; inversion Hoi; subst.
    + exists aa0, bb0.
      split; [apply OI_par_r; assumption
             | split; [apply se_name_refl | apply se_refl]].
    + exists aa0, bb0.
      split; [apply OI_par_l; assumption
             | split; [apply se_name_refl | apply se_refl]].
    + exists aa0, bb0.
      split; [apply OI_par_r; assumption
             | split; [apply se_name_refl | apply se_refl]].
    + exists aa0, bb0.
      split; [apply OI_par_l; assumption
             | split; [apply se_name_refl | apply se_refl]].
  - (* se_par_assoc *)
    split.
    + intros aa0 bb0 Hoi. inversion Hoi; subst.
      * match goal with
        | [ Hinner : only_input (PPar P Q) aa0 bb0 |- _ ] =>
            inversion Hinner; subst
        end.
        -- exists aa0, bb0.
           split; [apply OI_par_l; [assumption | simpl; lia]
                  | split; [apply se_name_refl | apply se_refl]].
        -- exists aa0, bb0.
           split; [apply OI_par_r; [assumption | apply OI_par_l; assumption]
                  | split; [apply se_name_refl | apply se_refl]].
      * match goal with
        | [ Hpz : head_count (PPar P Q) = 0 |- _ ] => simpl in Hpz
        end.
        exists aa0, bb0.
        split; [apply OI_par_r; [lia | apply OI_par_r; [lia | assumption]]
               | split; [apply se_name_refl | apply se_refl]].
    + intros aa0 bb0 Hoi. inversion Hoi; subst.
      * match goal with
        | [ Hqz : head_count (PPar Q R) = 0 |- _ ] => simpl in Hqz
        end.
        exists aa0, bb0.
        split; [apply OI_par_l; [apply OI_par_l; [assumption | lia] | lia]
               | split; [apply se_name_refl | apply se_refl]].
      * match goal with
        | [ Hinner : only_input (PPar Q R) aa0 bb0 |- _ ] =>
            inversion Hinner; subst
        end.
        -- exists aa0, bb0.
           split; [apply OI_par_l; [apply OI_par_r; assumption | assumption]
                  | split; [apply se_name_refl | apply se_refl]].
        -- exists aa0, bb0.
           split; [apply OI_par_r; [simpl; lia | assumption]
                  | split; [apply se_name_refl | apply se_refl]].
  - (* se_par_nil *)
    split; intros aa0 bb0 Hoi.
    + inversion Hoi; subst.
      * exists aa0, bb0.
        split; [assumption | split; [apply se_name_refl | apply se_refl]].
      * match goal with
        | [ Himp : only_input PNil _ _ |- _ ] => inversion Himp
        end.
    + exists aa0, bb0.
      split; [apply OI_par_l; [assumption | reflexivity]
             | split; [apply se_name_refl | apply se_refl]].
  - (* se_par_cong *)
    destruct IHHeq1 as [Hf1 Hb1].
    destruct IHHeq2 as [Hf2 Hb2].
    pose proof (head_count_se _ _ Heq1) as Hhc1.
    pose proof (head_count_se _ _ Heq2) as Hhc2.
    split; intros aa0 bb0 Hoi; inversion Hoi; subst.
    + match goal with
      | [ Hop : only_input P aa0 bb0 |- _ ] =>
          destruct (Hf1 aa0 bb0 Hop) as [yp [cp [Hop' [Hnp Hbp]]]]
      end.
      exists yp, cp.
      split; [apply OI_par_l; [exact Hop' | lia]
             | split; [exact Hnp | exact Hbp]].
    + match goal with
      | [ Hoq : only_input Q aa0 bb0 |- _ ] =>
          destruct (Hf2 aa0 bb0 Hoq) as [yq [cq [Hoq' [Hnq Hbq]]]]
      end.
      exists yq, cq.
      split; [apply OI_par_r; [lia | exact Hoq']
             | split; [exact Hnq | exact Hbq]].
    + match goal with
      | [ Hop : only_input P' aa0 bb0 |- _ ] =>
          destruct (Hb1 aa0 bb0 Hop) as [yp [cp [Hop' [Hnp Hbp]]]]
      end.
      exists yp, cp.
      split; [apply OI_par_l; [exact Hop' | lia]
             | split; [exact Hnp | exact Hbp]].
    + match goal with
      | [ Hoq : only_input Q' aa0 bb0 |- _ ] =>
          destruct (Hb2 aa0 bb0 Hoq) as [yq [cq [Hoq' [Hnq Hbq]]]]
      end.
      exists yq, cq.
      split; [apply OI_par_r; [lia | exact Hoq']
             | split; [exact Hnq | exact Hbq]].
  - (* se_input_cong *)
    split; intros aa0 bb0 Hoi; inversion Hoi; subst.
    + exists x', P'.
      split; [apply OI_base | split; assumption].
    + exists x, P.
      split; [apply OI_base
             | split; [apply se_name_sym; assumption
                      | apply se_sym; assumption]].
  - (* se_output_cong *)
    split; intros aa0 bb0 Hoi; inversion Hoi.
  - (* se_deref_cong *)
    split; intros aa0 bb0 Hoi; inversion Hoi.
  - (* se_replicate_cong *)
    split; intros aa0 bb0 Hoi; inversion Hoi.
Qed.

Lemma se_PInput_inj : forall x B y C,
  PInput x B ≡ PInput y C -> x ≡N y /\ B ≡ C.
Proof.
  intros x B y C H.
  destruct (onlyinput_se_both _ _ H) as [Hf _].
  destruct (Hf x B (OI_base x B)) as [y' [C' [Hoi [Hn Hbe]]]].
  inversion Hoi; subst. split; assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 11: POutput Constructor Injectivity
   ═══════════════════════════════════════════════════════════════════════════ *)

Inductive only_output : proc -> name -> proc -> Prop :=
  | OO_base  : forall ax aB, only_output (POutput ax aB) ax aB
  | OO_par_l : forall aP aQ ax aB,
      only_output aP ax aB -> head_count aQ = 0 -> only_output (PPar aP aQ) ax aB
  | OO_par_r : forall aP aQ ax aB,
      head_count aP = 0 -> only_output aQ ax aB -> only_output (PPar aP aQ) ax aB.

Lemma onlyoutput_se_both :
  forall P R, P ≡ R ->
  (forall aa bb, only_output P aa bb ->
     exists yy cc, only_output R yy cc /\ aa ≡N yy /\ bb ≡ cc) /\
  (forall aa bb, only_output R aa bb ->
     exists yy cc, only_output P yy cc /\ aa ≡N yy /\ bb ≡ cc).
Proof.
  intros P R Heq. induction Heq.
  - (* se_refl *)
    split; intros aa0 bb0 Hoo;
      exists aa0, bb0;
      (split; [exact Hoo
              | split; [apply se_name_refl | apply se_refl]]).
  - (* se_sym *)
    destruct IHHeq as [Hf Hb]. split; assumption.
  - (* se_trans *)
    destruct IHHeq1 as [Hf1 Hb1].
    destruct IHHeq2 as [Hf2 Hb2].
    split; intros aa0 bb0 Hoo.
    + destruct (Hf1 aa0 bb0 Hoo) as [yq [cq [Hoo_q [Hn1 Hbe1]]]].
      destruct (Hf2 yq cq Hoo_q) as [yr [cr [Hoo_r [Hn2 Hbe2]]]].
      exists yr, cr.
      split; [exact Hoo_r
             | split; [eapply se_name_trans; eassumption
                      | eapply se_trans; eassumption]].
    + destruct (Hb2 aa0 bb0 Hoo) as [yq [cq [Hoo_q [Hn1 Hbe1]]]].
      destruct (Hb1 yq cq Hoo_q) as [yp [cp [Hoo_p [Hn2 Hbe2]]]].
      exists yp, cp.
      split; [exact Hoo_p
             | split; [eapply se_name_trans; eassumption
                      | eapply se_trans; eassumption]].
  - (* se_par_comm *)
    split; intros aa0 bb0 Hoo; inversion Hoo; subst.
    + exists aa0, bb0.
      split; [apply OO_par_r; assumption
             | split; [apply se_name_refl | apply se_refl]].
    + exists aa0, bb0.
      split; [apply OO_par_l; assumption
             | split; [apply se_name_refl | apply se_refl]].
    + exists aa0, bb0.
      split; [apply OO_par_r; assumption
             | split; [apply se_name_refl | apply se_refl]].
    + exists aa0, bb0.
      split; [apply OO_par_l; assumption
             | split; [apply se_name_refl | apply se_refl]].
  - (* se_par_assoc *)
    split.
    + intros aa0 bb0 Hoo. inversion Hoo; subst.
      * match goal with
        | [ Hinner : only_output (PPar P Q) aa0 bb0 |- _ ] =>
            inversion Hinner; subst
        end.
        -- exists aa0, bb0.
           split; [apply OO_par_l; [assumption | simpl; lia]
                  | split; [apply se_name_refl | apply se_refl]].
        -- exists aa0, bb0.
           split; [apply OO_par_r; [assumption | apply OO_par_l; assumption]
                  | split; [apply se_name_refl | apply se_refl]].
      * match goal with
        | [ Hpz : head_count (PPar P Q) = 0 |- _ ] => simpl in Hpz
        end.
        exists aa0, bb0.
        split; [apply OO_par_r; [lia | apply OO_par_r; [lia | assumption]]
               | split; [apply se_name_refl | apply se_refl]].
    + intros aa0 bb0 Hoo. inversion Hoo; subst.
      * match goal with
        | [ Hqz : head_count (PPar Q R) = 0 |- _ ] => simpl in Hqz
        end.
        exists aa0, bb0.
        split; [apply OO_par_l; [apply OO_par_l; [assumption | lia] | lia]
               | split; [apply se_name_refl | apply se_refl]].
      * match goal with
        | [ Hinner : only_output (PPar Q R) aa0 bb0 |- _ ] =>
            inversion Hinner; subst
        end.
        -- exists aa0, bb0.
           split; [apply OO_par_l; [apply OO_par_r; assumption | assumption]
                  | split; [apply se_name_refl | apply se_refl]].
        -- exists aa0, bb0.
           split; [apply OO_par_r; [simpl; lia | assumption]
                  | split; [apply se_name_refl | apply se_refl]].
  - (* se_par_nil *)
    split; intros aa0 bb0 Hoo.
    + inversion Hoo; subst.
      * exists aa0, bb0.
        split; [assumption | split; [apply se_name_refl | apply se_refl]].
      * match goal with
        | [ Himp : only_output PNil _ _ |- _ ] => inversion Himp
        end.
    + exists aa0, bb0.
      split; [apply OO_par_l; [assumption | reflexivity]
             | split; [apply se_name_refl | apply se_refl]].
  - (* se_par_cong *)
    destruct IHHeq1 as [Hf1 Hb1].
    destruct IHHeq2 as [Hf2 Hb2].
    pose proof (head_count_se _ _ Heq1) as Hhc1.
    pose proof (head_count_se _ _ Heq2) as Hhc2.
    split; intros aa0 bb0 Hoo; inversion Hoo; subst.
    + match goal with
      | [ Hop : only_output P aa0 bb0 |- _ ] =>
          destruct (Hf1 aa0 bb0 Hop) as [yp [cp [Hop' [Hnp Hbp]]]]
      end.
      exists yp, cp.
      split; [apply OO_par_l; [exact Hop' | lia]
             | split; [exact Hnp | exact Hbp]].
    + match goal with
      | [ Hoq : only_output Q aa0 bb0 |- _ ] =>
          destruct (Hf2 aa0 bb0 Hoq) as [yq [cq [Hoq' [Hnq Hbq]]]]
      end.
      exists yq, cq.
      split; [apply OO_par_r; [lia | exact Hoq']
             | split; [exact Hnq | exact Hbq]].
    + match goal with
      | [ Hop : only_output P' aa0 bb0 |- _ ] =>
          destruct (Hb1 aa0 bb0 Hop) as [yp [cp [Hop' [Hnp Hbp]]]]
      end.
      exists yp, cp.
      split; [apply OO_par_l; [exact Hop' | lia]
             | split; [exact Hnp | exact Hbp]].
    + match goal with
      | [ Hoq : only_output Q' aa0 bb0 |- _ ] =>
          destruct (Hb2 aa0 bb0 Hoq) as [yq [cq [Hoq' [Hnq Hbq]]]]
      end.
      exists yq, cq.
      split; [apply OO_par_r; [lia | exact Hoq']
             | split; [exact Hnq | exact Hbq]].
  - (* se_input_cong *)
    split; intros aa0 bb0 Hoo; inversion Hoo.
  - (* se_output_cong *)
    split; intros aa0 bb0 Hoo; inversion Hoo; subst.
    + exists x', Q'.
      split; [apply OO_base | split; assumption].
    + exists x, Q.
      split; [apply OO_base
             | split; [apply se_name_sym; assumption
                      | apply se_sym; assumption]].
  - (* se_deref_cong *)
    split; intros aa0 bb0 Hoo; inversion Hoo.
  - (* se_replicate_cong *)
    split; intros aa0 bb0 Hoo; inversion Hoo.
Qed.

Lemma se_POutput_inj : forall x B y C,
  POutput x B ≡ POutput y C -> x ≡N y /\ B ≡ C.
Proof.
  intros x B y C H.
  destruct (onlyoutput_se_both _ _ H) as [Hf _].
  destruct (Hf x B (OO_base x B)) as [y' [C' [Hoo [Hn Hbe]]]].
  inversion Hoo; subst. split; assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 12: Substitution and Lifting Congruence Lemmas
   ═══════════════════════════════════════════════════════════════════════════

   Both lift_proc/lift_name and subst_proc/subst_name respect structural
   equivalence on every argument that can carry an equivalence. The proofs
   form two tightly-coupled mutually-recursive groups:

   1. lift_proc_cong / lift_name_cong: induction on the derivation of
      P ≡ P' / x ≡N x' (via a Combined Scheme over ≡ and ≡N).

   2. subst_proc_name_cong / subst_name_name_cong (varying the substituent):
      structural induction on proc/name (via proc_name_mutind).
      The PInput case needs lift_name_cong, already proved.

   3. subst_proc_cong / subst_name_cong (varying the target): induction on
      P ≡ P' / x ≡N x' again.                                                *)

Scheme se_proc_mut := Induction for struct_equiv Sort Prop
  with se_name_mut := Induction for struct_equiv_name Sort Prop.

Combined Scheme se_combined_mut from se_proc_mut, se_name_mut.

(* --- Group 1: lifting respects ≡ / ≡N ------------------------------------ *)

Lemma lift_cong_combined :
  (forall P P' (_ : P ≡ P'),
     forall d c, lift_proc d c P ≡ lift_proc d c P') /\
  (forall x x' (_ : x ≡N x'),
     forall d c, lift_name d c x ≡N lift_name d c x').
Proof.
  apply (se_combined_mut
    (fun P P' _ => forall d c, lift_proc d c P ≡ lift_proc d c P')
    (fun x x' _ => forall d c, lift_name d c x ≡N lift_name d c x')).
  - (* se_refl *) intros P d c. apply se_refl.
  - (* se_sym *) intros P Q _ IH d c. apply se_sym. apply IH.
  - (* se_trans *) intros P Q R _ IH1 _ IH2 d c.
    eapply se_trans; [apply IH1 | apply IH2].
  - (* se_par_comm *) intros P Q d c. simpl. apply se_par_comm.
  - (* se_par_assoc *) intros P Q R d c. simpl. apply se_par_assoc.
  - (* se_par_nil *) intros P d c. simpl. apply se_par_nil.
  - (* se_par_cong *) intros P P' Q Q' _ IHP _ IHQ d c. simpl.
    apply se_par_cong; [apply IHP | apply IHQ].
  - (* se_input_cong *) intros x x' P P' _ IHx _ IHP d c. simpl.
    apply se_input_cong; [apply IHx | apply IHP].
  - (* se_output_cong *) intros x x' Q Q' _ IHx _ IHQ d c. simpl.
    apply se_output_cong; [apply IHx | apply IHQ].
  - (* se_deref_cong *) intros x x' _ IHx d c. simpl.
    apply se_deref_cong. apply IHx.
  - (* se_replicate_cong *) intros P0 P0' _ IHP0 d c. simpl.
    apply se_replicate_cong. apply IHP0.
  - (* se_name_quote *) intros P P' _ IH d c. simpl.
    apply se_name_quote. apply IH.
  - (* se_name_var_refl *) intros k d c. simpl.
    destruct (c <=? k); apply se_name_var_refl.
Qed.

Lemma lift_proc_cong : forall P P' d c,
  P ≡ P' -> lift_proc d c P ≡ lift_proc d c P'.
Proof.
  intros P P' d c H.
  destruct lift_cong_combined as [HP _].
  apply HP; assumption.
Qed.

Lemma lift_name_cong : forall x x' d c,
  x ≡N x' -> lift_name d c x ≡N lift_name d c x'.
Proof.
  intros x x' d c H.
  destruct lift_cong_combined as [_ HN].
  apply HN; assumption.
Qed.

(* --- Group 2: substituent-side congruence (vary n, not P) ---------------- *)

Lemma subst_name_cong_combined :
  (forall P k n n', n ≡N n' -> subst_proc P k n ≡ subst_proc P k n') /\
  (forall x k n n', n ≡N n' -> subst_name x k n ≡N subst_name x k n').
Proof.
  apply (proc_name_mutind
    (fun P => forall k n n', n ≡N n' -> subst_proc P k n ≡ subst_proc P k n')
    (fun x => forall k n n', n ≡N n' -> subst_name x k n ≡N subst_name x k n')).
  - (* PNil *) intros k n n' _. simpl. apply se_refl.
  - (* PInput *) intros x IHx P IHP k n n' Hnn'. simpl.
    apply se_input_cong.
    + apply IHx. assumption.
    + apply IHP. apply lift_name_cong. assumption.
  - (* POutput *) intros x IHx Q IHQ k n n' Hnn'. simpl.
    apply se_output_cong; [apply IHx | apply IHQ]; assumption.
  - (* PPar *) intros P1 IH1 P2 IH2 k n n' Hnn'. simpl.
    apply se_par_cong; [apply IH1 | apply IH2]; assumption.
  - (* PDeref : case-analyse on the name shape to mirror the new
       semantic [subst_proc] definition. *)
    intros x IHx k n n' Hnn'.
    destruct x as [Pi | j].
    + (* Quote Pi: both sides reduce to PDeref (Quote (subst_proc Pi k _));
         the name-level IH gives the inner ≡N which implies the proc-
         level ≡ via [se_name_quote] inversion. *)
      simpl. apply se_deref_cong.
      specialize (IHx k n n' Hnn'). simpl in IHx. exact IHx.
    + (* NVar j: case-split on [Nat.compare j k]. Only the Eq case
         interacts non-trivially with the semantic collapse; Lt/Gt just
         preserve the PDeref shape unchanged. *)
      simpl. destruct (Nat.compare j k).
      * (* Eq: nested match on [n], [n']. Hnn' constrains them to
           be either both [Quote _] or both [NVar _ same index]. *)
        destruct n as [Qn | jn]; destruct n' as [Qn' | jn'];
          inversion Hnn'; subst; try apply se_refl.
        -- (* Quote Qn ≡N Quote Qn' with Qn ≡ Qn' *)
           assumption.
      * (* Lt: both sides = PDeref (NVar j). *) apply se_refl.
      * (* Gt: both sides = PDeref (NVar (j - 1)). *) apply se_refl.
  - (* PReplicate *) intros P0 IHP0 k n n' Hnn'. simpl.
    apply se_replicate_cong. apply IHP0. assumption.
  - (* Quote *) intros P IHP k n n' Hnn'. simpl.
    apply se_name_quote. apply IHP. assumption.
  - (* NVar *) intros j k n n' Hnn'. simpl.
    destruct (Nat.compare j k).
    + assumption.
    + apply se_name_var_refl.
    + apply se_name_var_refl.
Qed.

Lemma subst_proc_name_cong : forall P k n n',
  n ≡N n' -> subst_proc P k n ≡ subst_proc P k n'.
Proof. apply subst_name_cong_combined. Qed.

Lemma subst_name_name_cong : forall x k n n',
  n ≡N n' -> subst_name x k n ≡N subst_name x k n'.
Proof. apply subst_name_cong_combined. Qed.

(* --- Group 3: target-side congruence (vary P, keep n fixed) -------------- *)

Lemma subst_cong_combined :
  (forall P P' (_ : P ≡ P'),
     forall k n, subst_proc P k n ≡ subst_proc P' k n) /\
  (forall x x' (_ : x ≡N x'),
     forall k n, subst_name x k n ≡N subst_name x' k n).
Proof.
  apply (se_combined_mut
    (fun P P' _ => forall k n, subst_proc P k n ≡ subst_proc P' k n)
    (fun x x' _ => forall k n, subst_name x k n ≡N subst_name x' k n)).
  - (* se_refl *) intros P k n. apply se_refl.
  - (* se_sym *) intros P Q _ IH k n. apply se_sym. apply IH.
  - (* se_trans *) intros P Q R _ IH1 _ IH2 k n.
    eapply se_trans; [apply IH1 | apply IH2].
  - (* se_par_comm *) intros P Q k n. simpl. apply se_par_comm.
  - (* se_par_assoc *) intros P Q R k n. simpl. apply se_par_assoc.
  - (* se_par_nil *) intros P k n. simpl. apply se_par_nil.
  - (* se_par_cong *) intros P P' Q Q' _ IHP _ IHQ k n. simpl.
    apply se_par_cong; [apply IHP | apply IHQ].
  - (* se_input_cong *) intros x x' P P' _ IHx _ IHP k n. simpl.
    apply se_input_cong; [apply IHx | apply IHP].
  - (* se_output_cong *) intros x x' Q Q' _ IHx _ IHQ k n. simpl.
    apply se_output_cong; [apply IHx | apply IHQ].
  - (* se_deref_cong : under semantic subst, the PDeref case case-
       analyses on the name shape. We invert [x ≡N x'] to narrow to
       the two possible name pairings (both [Quote _] or both
       [NVar _]) and close each. *)
    intros x x' Hxx' IHx k n.
    inversion Hxx' as [P P' Hproc Heq1 Heq2 | j0 Heq1]; subst.
    + (* Quote P ≡N Quote P' from P ≡ P' *)
      simpl. apply se_deref_cong.
      specialize (IHx k n). simpl in IHx. exact IHx.
    + (* NVar j0 ≡N NVar j0 : both sides identical. *)
      apply se_refl.
  - (* se_replicate_cong *) intros P0 P0' _ IHP0 k n. simpl.
    apply se_replicate_cong. apply IHP0.
  - (* se_name_quote *) intros P P' _ IH k n. simpl.
    apply se_name_quote. apply IH.
  - (* se_name_var_refl *) intros j k n. apply se_name_refl.
Qed.

Lemma subst_proc_cong : forall P P' k n,
  P ≡ P' -> subst_proc P k n ≡ subst_proc P' k n.
Proof.
  intros P P' k n H.
  destruct subst_cong_combined as [HP _].
  apply HP; assumption.
Qed.

Lemma subst_name_cong : forall x x' k n,
  x ≡N x' -> subst_name x k n ≡N subst_name x' k n.
Proof.
  intros x x' k n H.
  destruct subst_cong_combined as [_ HN].
  apply HN; assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 13: PReplicate Constructor Injectivity
   ═══════════════════════════════════════════════════════════════════════════

   The fourth of the [se_*_inj] family (after PDeref, PInput, POutput):
   [PReplicate X ≡ PReplicate Y → X ≡ Y]. Follows the same [only_*]
   predicate + bidirectional ≡-preservation template; the proof of
   [onlyreplicate_se_both] is a systematic copy-adapt from
   [onlyoutput_se_both] above, differing only where the replication
   constructor itself is involved (in the [se_replicate_cong] case).   *)

Inductive only_replicate : proc -> proc -> Prop :=
  | OR_base  : forall B, only_replicate (PReplicate B) B
  | OR_par_l : forall P Q B,
      only_replicate P B -> head_count Q = 0 -> only_replicate (PPar P Q) B
  | OR_par_r : forall P Q B,
      head_count P = 0 -> only_replicate Q B -> only_replicate (PPar P Q) B.

Lemma onlyreplicate_se_both :
  forall P R, P ≡ R ->
    (forall B, only_replicate P B -> exists B', only_replicate R B' /\ B ≡ B') /\
    (forall B, only_replicate R B -> exists B', only_replicate P B' /\ B ≡ B').
Proof.
  intros P R Heq. induction Heq.
  - (* se_refl *)
    split; intros B0 Hor;
      exists B0;
      (split; [exact Hor | apply se_refl]).
  - (* se_sym *)
    destruct IHHeq as [Hf Hb]. split; assumption.
  - (* se_trans *)
    destruct IHHeq1 as [Hf1 Hb1].
    destruct IHHeq2 as [Hf2 Hb2].
    split; intros B0 Hor.
    + destruct (Hf1 B0 Hor) as [Bq [Hor_q Hbe1]].
      destruct (Hf2 Bq Hor_q) as [Br [Hor_r Hbe2]].
      exists Br.
      split; [exact Hor_r | eapply se_trans; eassumption].
    + destruct (Hb2 B0 Hor) as [Bq [Hor_q Hbe1]].
      destruct (Hb1 Bq Hor_q) as [Bp [Hor_p Hbe2]].
      exists Bp.
      split; [exact Hor_p | eapply se_trans; eassumption].
  - (* se_par_comm *)
    split; intros B0 Hor; inversion Hor; subst.
    + exists B0.
      split; [apply OR_par_r; assumption | apply se_refl].
    + exists B0.
      split; [apply OR_par_l; assumption | apply se_refl].
    + exists B0.
      split; [apply OR_par_r; assumption | apply se_refl].
    + exists B0.
      split; [apply OR_par_l; assumption | apply se_refl].
  - (* se_par_assoc *)
    split.
    + intros B0 Hor. inversion Hor; subst.
      * match goal with
        | [ Hinner : only_replicate (PPar P Q) B0 |- _ ] =>
            inversion Hinner; subst
        end.
        -- exists B0.
           split; [apply OR_par_l; [assumption | simpl; lia] | apply se_refl].
        -- exists B0.
           split; [apply OR_par_r; [assumption | apply OR_par_l; assumption]
                  | apply se_refl].
      * match goal with
        | [ Hpz : head_count (PPar P Q) = 0 |- _ ] => simpl in Hpz
        end.
        exists B0.
        split; [apply OR_par_r; [lia | apply OR_par_r; [lia | assumption]]
               | apply se_refl].
    + intros B0 Hor. inversion Hor; subst.
      * match goal with
        | [ Hqz : head_count (PPar Q R) = 0 |- _ ] => simpl in Hqz
        end.
        exists B0.
        split; [apply OR_par_l; [apply OR_par_l; [assumption | lia] | lia]
               | apply se_refl].
      * match goal with
        | [ Hinner : only_replicate (PPar Q R) B0 |- _ ] =>
            inversion Hinner; subst
        end.
        -- exists B0.
           split; [apply OR_par_l; [apply OR_par_r; assumption | assumption]
                  | apply se_refl].
        -- exists B0.
           split; [apply OR_par_r; [simpl; lia | assumption] | apply se_refl].
  - (* se_par_nil *)
    split; intros B0 Hor.
    + inversion Hor; subst.
      * exists B0.
        split; [assumption | apply se_refl].
      * match goal with
        | [ Himp : only_replicate PNil _ |- _ ] => inversion Himp
        end.
    + exists B0.
      split; [apply OR_par_l; [assumption | reflexivity] | apply se_refl].
  - (* se_par_cong *)
    destruct IHHeq1 as [Hf1 Hb1].
    destruct IHHeq2 as [Hf2 Hb2].
    pose proof (head_count_se _ _ Heq1) as Hhc1.
    pose proof (head_count_se _ _ Heq2) as Hhc2.
    split; intros B0 Hor; inversion Hor; subst.
    + match goal with
      | [ Hop : only_replicate P B0 |- _ ] =>
          destruct (Hf1 B0 Hop) as [Bp [Hop' Hbp]]
      end.
      exists Bp.
      split; [apply OR_par_l; [exact Hop' | lia] | exact Hbp].
    + match goal with
      | [ Hoq : only_replicate Q B0 |- _ ] =>
          destruct (Hf2 B0 Hoq) as [Bq [Hoq' Hbq]]
      end.
      exists Bq.
      split; [apply OR_par_r; [lia | exact Hoq'] | exact Hbq].
    + match goal with
      | [ Hop : only_replicate P' B0 |- _ ] =>
          destruct (Hb1 B0 Hop) as [Bp [Hop' Hbp]]
      end.
      exists Bp.
      split; [apply OR_par_l; [exact Hop' | lia] | exact Hbp].
    + match goal with
      | [ Hoq : only_replicate Q' B0 |- _ ] =>
          destruct (Hb2 B0 Hoq) as [Bq [Hoq' Hbq]]
      end.
      exists Bq.
      split; [apply OR_par_r; [lia | exact Hoq'] | exact Hbq].
  - (* se_input_cong *)
    split; intros B0 Hor; inversion Hor.
  - (* se_output_cong *)
    split; intros B0 Hor; inversion Hor.
  - (* se_deref_cong *)
    split; intros B0 Hor; inversion Hor.
  - (* se_replicate_cong P0 P0' Hpp IHpp *)
    split; intros B0 Hor; inversion Hor; subst.
    + exists P'. split; [apply OR_base | exact Heq].
    + exists P. split; [apply OR_base | apply se_sym; exact Heq].
Qed.

Lemma se_PReplicate_inj : forall X Y,
  PReplicate X ≡ PReplicate Y -> X ≡ Y.
Proof.
  intros X Y H.
  destruct (onlyreplicate_se_both _ _ H) as [Hf _].
  destruct (Hf X (OR_base X)) as [Y' [Hor HeqB]].
  inversion Hor; subst. exact HeqB.
Qed.

(* Head-count-zero-implies-PNil. Required by [only_replicate_se_PReplicate]
   below (and re-used widely in subsequent reverse-direction proofs).
   Proof: any process P with head_count P = 0 has heads P = [] (by
   [heads_length_eq_head_count]); so [heads_to_proc (heads P) = PNil], which
   is ≡ P by [heads_to_proc_heads_se]. *)
Lemma head_count_zero_se_nil : forall P, head_count P = 0 -> P ≡ PNil.
Proof.
  intros P H.
  pose proof (heads_to_proc_heads_se P) as Hse.
  pose proof (heads_length_eq_head_count P) as Hlen.
  rewrite H in Hlen.
  destruct (heads P) as [|h t] eqn:Heqh.
  - simpl in Hse. apply se_sym. exact Hse.
  - simpl in Hlen. discriminate.
Qed.

(* Bridge lemma: [only_replicate P B] implies [P ≡ PReplicate B]. Used
   in the [step_PReplicate_inv_se] proof to collapse [only_replicate]
   witnesses back to structural equivalence. *)
Lemma only_replicate_se_PReplicate : forall P B,
  only_replicate P B -> P ≡ PReplicate B.
Proof.
  intros P B H. induction H.
  - (* OR_base *) apply se_refl.
  - (* OR_par_l: only_replicate P B, head_count Q = 0.
       IH: P ≡ PReplicate B.
       Need: PPar P Q ≡ PReplicate B. *)
    eapply se_trans.
    + apply se_par_cong_r. apply head_count_zero_se_nil. assumption.
    + eapply se_trans; [apply se_par_nil | exact IHonly_replicate].
  - (* OR_par_r: head_count P = 0, only_replicate Q B.
       IH: Q ≡ PReplicate B. *)
    eapply se_trans.
    + apply se_par_cong_l. apply head_count_zero_se_nil. assumption.
    + eapply se_trans; [apply se_nil_par | exact IHonly_replicate].
Qed.
