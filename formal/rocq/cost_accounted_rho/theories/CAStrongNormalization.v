(* ════════════════════════════════════════════════════════════════════════
   CAStrongNormalization.v — native termination on the funded fragment (Stage 3b).

   Native [ca_step] does NOT strongly normalize unconditionally: a non-linear
   continuation (a received quote dereferenced twice) duplicates a token-bearing
   payload, so [st_total_fuel] can strictly INCREASE
   ([st_total_fuel_can_increase_off_funded] exhibits a concrete witness). SN
   therefore holds on the LINEARLY-FUNDED fragment — the consensus-relevant class,
   since only funded deploys are admitted. There, every COMM strictly decreases
   [st_total_fuel] (by the consumed gate), and well-foundedness of [<] gives SN.

   Key lemma (the bridge): under a linear continuation, substitution materializes
   the payload at most once, so it adds at most the payload's fuel — the
   term-level image of LinearLogicResources' no-contraction. Axiom-free.        *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
Import ListNotations.
From Stdlib Require Import Wf_nat.
From Stdlib Require Import Wellfounded.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CABinding.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CATokenConservation.

(* ── lifting preserves total fuel (it shifts indices, adds/removes no gate) ── *)
Lemma lift_fuel_inv :
  (forall P d c, caproc_total_fuel (lift_caproc d c P) = caproc_total_fuel P)
  /\ (forall x d c, caname_total_fuel (lift_caname d c x) = caname_total_fuel x)
  /\ (forall T d c, st_total_fuel (lift_st d c T) = st_total_fuel T).
Proof.
  apply ca_deep_ind; intros; simpl;
    repeat (match goal with H : forall _ _ : nat, _ = _ |- _ => rewrite H end);
    try reflexivity.
  - (* CPJoin xs T: the channel fold matches via the caname Forall *)
    f_equal. induction H as [| x xs' Hx HF IH]; simpl;
      [ reflexivity | rewrite Hx, IH; reflexivity ].
  - destruct (c <=? n); reflexivity.   (* CNVar *)
Qed.

Lemma lift_caname_fuel_inv : forall x d c,
  caname_total_fuel (lift_caname d c x) = caname_total_fuel x.
Proof. apply lift_fuel_inv. Qed.

(* dequote loses fuel: a raw stack dequotes to CPNil (its fuel vanishes) ── *)
Lemma st_to_proc_fuel_le : forall U, caproc_total_fuel (st_to_proc U) <= st_total_fuel U.
Proof.
  induction U as [P s | U1 IH1 U2 IH2 | t]; simpl.
  - lia.                          (* STSigned P _: st_to_proc = P, equal *)
  - lia.                          (* STPar: sum, by IHs *)
  - lia.                          (* STStack: CPNil (0) <= token_size t *)
Qed.

(* ── the bridge: substitution adds at most (#occurrences × payload fuel) ──── *)
Lemma subst_fuel_bound :
  (forall P n N, caproc_total_fuel (subst_caproc P n N)
       <= caproc_total_fuel P + deref_count_caproc n P * caname_total_fuel N)
  /\ (forall x n N, caname_total_fuel (subst_caname x n N)
       <= caname_total_fuel x + deref_count_caname n x * caname_total_fuel N)
  /\ (forall T n N, st_total_fuel (subst_st T n N)
       <= st_total_fuel T + deref_count_st n T * caname_total_fuel N).
Proof.
  apply ca_deep_ind.
  - (* CPNil *) intros n N. simpl. lia.
  - (* CPInput x T *)
    intros x IHx T IHT n N. simpl.
    specialize (IHx n N). specialize (IHT (S n) (lift_caname 1 0 N)).
    rewrite lift_caname_fuel_inv in IHT. nia.
  - (* CPOutput x U *)
    intros x IHx U IHU n N. simpl.
    specialize (IHx n N). specialize (IHU n N). nia.
  - (* CPPar P1 P2 *)
    intros P1 IH1 P2 IH2 n N. simpl.
    specialize (IH1 n N). specialize (IH2 n N). nia.
  - (* CPDeref x *)
    intros x IHx n N. destruct x as [Ti | k]; simpl.
    + (* CQuote Ti *) specialize (IHx n N). simpl in IHx. nia.
    + (* CNVar k *)
      destruct (Nat.compare k n) eqn:Hcmp; simpl.
      * apply Nat.compare_eq in Hcmp. subst k. rewrite Nat.eqb_refl.
        destruct N as [U | j]; simpl.
        -- pose proof (st_to_proc_fuel_le U). nia.
        -- nia.
      * apply Nat.compare_lt_iff in Hcmp.
        assert (Nat.eqb k n = false) as Heq by (apply Nat.eqb_neq; lia).
        rewrite Heq. simpl. nia.
      * apply Nat.compare_gt_iff in Hcmp.
        assert (Nat.eqb k n = false) as Heq by (apply Nat.eqb_neq; lia).
        rewrite Heq. simpl. nia.
  - (* CPJoin xs T: channels via the Forall fold-bound, continuation via the
       N-shifted st IH (lift_caname_fuel_inv normalises the payload fuel). *)
    intros xs HForall T IHT n N. simpl.
    specialize (IHT (length xs + n) (lift_caname (length xs) 0 N)).
    rewrite lift_caname_fuel_inv in IHT.
    assert (Hchan : fold_right (fun x acc => caname_total_fuel x + acc) 0
                      (map (fun x => subst_caname x n N) xs)
                    <= fold_right (fun x acc => caname_total_fuel x + acc) 0 xs
                       + fold_right (fun x acc => deref_count_caname n x + acc) 0 xs
                         * caname_total_fuel N).
    { clear IHT T. induction HForall as [| x xs' Hx HF IH]; simpl; [ lia | ].
      specialize (Hx n N). rewrite Nat.mul_add_distr_r. nia. }
    rewrite Nat.mul_add_distr_r. nia.
  - (* CQuote T *)
    intros T IHT n N. simpl. specialize (IHT n N). nia.
  - (* CNVar k *)
    intros k n N. simpl.
    destruct (Nat.compare k n) eqn:Hcmp; simpl.
    + apply Nat.compare_eq in Hcmp. subst k. rewrite Nat.eqb_refl. nia.
    + apply Nat.compare_lt_iff in Hcmp.
      assert (Nat.eqb k n = false) as Heq by (apply Nat.eqb_neq; lia). rewrite Heq. simpl. nia.
    + apply Nat.compare_gt_iff in Hcmp.
      assert (Nat.eqb k n = false) as Heq by (apply Nat.eqb_neq; lia). rewrite Heq. simpl. nia.
  - (* STSigned P s *)
    intros P IHP s n N. simpl. specialize (IHP n N). nia.
  - (* STPar T1 T2 *)
    intros T1 IH1 T2 IH2 n N. simpl.
    specialize (IH1 n N). specialize (IH2 n N). nia.
  - (* STStack t *) intros t n N. simpl. lia.
Qed.

Lemma subst_st_fuel_bound : forall T n N,
  st_total_fuel (subst_st T n N)
    <= st_total_fuel T + deref_count_st n T * caname_total_fuel N.
Proof. apply subst_fuel_bound. Qed.

(* Under a linear continuation (deref ≤ 1), substitution adds at most the
   payload's fuel once. *)
Lemma linear_subst_fuel_le : forall T U,
  linear_cont T ->
  st_total_fuel (subst_st T 0 (CQuote U)) <= st_total_fuel T + st_total_fuel U.
Proof.
  intros T U Hlin. unfold linear_cont in Hlin.
  pose proof (subst_st_fuel_bound T 0 (CQuote U)) as Hb. simpl in Hb.
  (* deref_count_st 0 T <= 1, caname_total_fuel (CQuote U) = st_total_fuel U *)
  assert (deref_count_st 0 T * st_total_fuel U <= st_total_fuel U) by (nia).
  lia.
Qed.

(* ── N-ary join bridge: closed payloads inject no dereferences ──────────────
   A closed term forces no free de Bruijn name (every CPDeref is bound), so its
   free-dereference count is 0 at every queried index — the quantitative image of
   closedness used to propagate linearity through the N-fold join substitution. *)
Lemma closed_deref_zero_ca :
  (forall P k, closed_caproc_at k P -> forall j, k <= j -> deref_count_caproc j P = 0)
  /\ (forall x k, closed_caname_at k x -> forall j, k <= j -> deref_count_caname j x = 0)
  /\ (forall T k, closed_st_at k T -> forall j, k <= j -> deref_count_st j T = 0).
Proof.
  apply ca_deep_ind.
  - (* CPNil *) intros k _ j _. reflexivity.
  - (* CPInput x T *) intros x IHx T IHT k Hcl j Hkj. simpl in *.
    destruct Hcl as [Hx HT]. rewrite (IHx k Hx j Hkj).
    rewrite (IHT (S k) HT (S j) ltac:(lia)). reflexivity.
  - (* CPOutput x U *) intros x IHx U IHU k Hcl j Hkj. simpl in *.
    destruct Hcl as [Hx HU]. rewrite (IHx k Hx j Hkj). rewrite (IHU k HU j Hkj). reflexivity.
  - (* CPPar P1 P2 *) intros P1 IH1 P2 IH2 k Hcl j Hkj. simpl in *.
    destruct Hcl as [H1 H2]. rewrite (IH1 k H1 j Hkj). rewrite (IH2 k H2 j Hkj). reflexivity.
  - (* CPDeref x *) intros x IHx k Hcl j Hkj. simpl in *. exact (IHx k Hcl j Hkj).
  - (* CPJoin xs T *) intros xs Hxs T IHT k Hcl j Hkj. simpl in *.
    destruct Hcl as [HF HT]. apply fold_right_closed_Forall in HF.
    assert (Hcont : deref_count_st (length xs + j) T = 0)
      by (apply (IHT (length xs + k) HT); lia).
    rewrite Hcont, Nat.add_0_r. clear Hcont IHT HT.
    revert HF. induction Hxs as [| x xs' Hx Htl IHtl]; intros HF; simpl; [ reflexivity | ].
    inversion HF as [| ? ? HFx HFtl ]; subst.
    rewrite (Hx k HFx j Hkj). simpl. apply IHtl. exact HFtl.
  - (* CQuote T *) intros T IHT k Hcl j Hkj. simpl in *. exact (IHT k Hcl j Hkj).
  - (* CNVar m *) intros m k Hcl j Hkj. simpl in *.
    assert (Nat.eqb m j = false) as Heq by (apply Nat.eqb_neq; lia). rewrite Heq. reflexivity.
  - (* STSigned P s *) intros P IHP s k Hcl j Hkj. simpl in *. exact (IHP k Hcl j Hkj).
  - (* STPar T1 T2 *) intros T1 IH1 T2 IH2 k Hcl j Hkj. simpl in *.
    destruct Hcl as [H1 H2]. rewrite (IH1 k H1 j Hkj). rewrite (IH2 k H2 j Hkj). reflexivity.
  - (* STStack t *) intros t k _ j _. reflexivity.
Qed.

(* Substituting a CLOSED payload at index 0 (no inter-step lifting needed — a
   closed term is lift-invariant) shifts every dereference count down by one:
   [deref_count i] of the result = [deref_count (S i)] of the source, for i ≥ n.
   The replaced occurrences become the closed (dereference-free) payload, and the
   indices above n decrement. This is what propagates linearity through the fold. *)
Lemma deref_subst_closed_ca : forall U, closed_st U ->
  (forall P n i, n <= i ->
     deref_count_caproc i (subst_caproc P n (CQuote U)) = deref_count_caproc (S i) P)
  /\ (forall x n i, n <= i ->
     deref_count_caname i (subst_caname x n (CQuote U)) = deref_count_caname (S i) x)
  /\ (forall T n i, n <= i ->
     deref_count_st i (subst_st T n (CQuote U)) = deref_count_st (S i) T).
Proof.
  intros U HU. apply ca_deep_ind.
  - (* CPNil *) intros n i Hni. reflexivity.
  - (* CPInput x T *) intros x IHx T IHT n i Hni. simpl.
    rewrite (closed_st_lift_zero U 1 0 HU).
    rewrite (IHx n i Hni). rewrite (IHT (S n) (S i) (le_n_S _ _ Hni)). reflexivity.
  - (* CPOutput x U0 *) intros x IHx U0 IHU0 n i Hni. simpl.
    rewrite (IHx n i Hni). rewrite (IHU0 n i Hni). reflexivity.
  - (* CPPar P1 P2 *) intros P1 IH1 P2 IH2 n i Hni. simpl.
    rewrite (IH1 n i Hni). rewrite (IH2 n i Hni). reflexivity.
  - (* CPDeref x *) intros x IHx n i Hni. destruct x as [Ti | k].
    + (* CQuote Ti *) simpl in *. exact (IHx n i Hni).
    + (* CNVar k *) simpl. destruct (Nat.compare k n) eqn:Hcmp.
      * apply Nat.compare_eq in Hcmp. subst k.
        rewrite (proj1 closed_deref_zero_ca (st_to_proc U) 0
                   (closed_st_to_proc U 0 HU) i (Nat.le_0_l i)).
        simpl. assert (Nat.eqb n (S i) = false) as Heq by (apply Nat.eqb_neq; lia).
        rewrite Heq. reflexivity.
      * apply Nat.compare_lt_iff in Hcmp. simpl.
        assert (Nat.eqb k i = false) as Ha by (apply Nat.eqb_neq; lia).
        assert (Nat.eqb k (S i) = false) as Hb by (apply Nat.eqb_neq; lia).
        rewrite Ha, Hb. reflexivity.
      * apply Nat.compare_gt_iff in Hcmp. simpl.
        destruct (Nat.eqb (k - 1) i) eqn:E1; destruct (Nat.eqb k (S i)) eqn:E2; try reflexivity.
        -- apply Nat.eqb_eq in E1. apply Nat.eqb_neq in E2. lia.
        -- apply Nat.eqb_neq in E1. apply Nat.eqb_eq in E2. lia.
  - (* CPJoin xs T *) intros xs Hxs T IHT n i Hni. simpl.
    rewrite (closed_st_lift_zero U (length xs) 0 HU).
    rewrite length_map.
    rewrite (IHT (length xs + n) (length xs + i) ltac:(lia)).
    f_equal.
    + (* channels *)
      induction Hxs as [| x xs' Hx Htl IHtl]; simpl; [ reflexivity | ].
      rewrite (Hx n i Hni). rewrite IHtl. reflexivity.
    + (* continuation index arithmetic *)
      f_equal. lia.
  - (* CQuote T *) intros T IHT n i Hni. simpl. exact (IHT n i Hni).
  - (* CNVar m *) intros m n i Hni. simpl. destruct (Nat.compare m n) eqn:Hcmp.
    + apply Nat.compare_eq in Hcmp. subst m. simpl.
      rewrite (proj2 (proj2 closed_deref_zero_ca) U 0 HU i (Nat.le_0_l i)).
      assert (Nat.eqb n (S i) = false) as Heq by (apply Nat.eqb_neq; lia).
      rewrite Heq. reflexivity.
    + apply Nat.compare_lt_iff in Hcmp. simpl.
      assert (Nat.eqb m i = false) as Ha by (apply Nat.eqb_neq; lia).
      assert (Nat.eqb m (S i) = false) as Hb by (apply Nat.eqb_neq; lia).
      rewrite Ha, Hb. reflexivity.
    + apply Nat.compare_gt_iff in Hcmp. simpl.
      destruct (Nat.eqb (m - 1) i) eqn:E1; destruct (Nat.eqb m (S i)) eqn:E2; try reflexivity.
      -- apply Nat.eqb_eq in E1. apply Nat.eqb_neq in E2. lia.
      -- apply Nat.eqb_neq in E1. apply Nat.eqb_eq in E2. lia.
  - (* STSigned P s *) intros P IHP s n i Hni. simpl. exact (IHP n i Hni).
  - (* STPar T1 T2 *) intros T1 IH1 T2 IH2 n i Hni. simpl.
    rewrite (IH1 n i Hni). rewrite (IH2 n i Hni). reflexivity.
  - (* STStack t *) intros t n i Hni. reflexivity.
Qed.

(* The fuel of the join's sender bundle x1!(U1) | … | xN!(UN): the channel fuels
   plus the payload fuels (matching arities). *)
Lemma join_sends_fuel : forall xs Us,
  length xs = length Us ->
  caproc_total_fuel (join_sends xs Us)
    = fold_right (fun x acc => caname_total_fuel x + acc) 0 xs
      + fold_right (fun U acc => st_total_fuel U + acc) 0 Us.
Proof.
  induction xs as [| x xs' IH]; intros [| U Us'] Hlen; simpl in *; try discriminate.
  - reflexivity.
  - rewrite (IH Us' ltac:(lia)). lia.
Qed.

(* The fuel of the J2 separately-signed sender bundle: the channel fuels plus the
   payload fuels (the per-sender seals carry no fuel — st_total_fuel ignores the
   seal). Independent of the sender signatures ts. *)
Lemma signed_sends_fuel : forall xs Us ts,
  length xs = length Us -> length xs = length ts ->
  st_total_fuel (signed_sends xs Us ts)
    = fold_right (fun x acc => caname_total_fuel x + acc) 0 xs
      + fold_right (fun U acc => st_total_fuel U + acc) 0 Us.
Proof.
  induction xs as [| x xs' IH]; intros [| U Us'] [| tt ts'] HU Ht;
    simpl in *; try discriminate; try reflexivity.
  rewrite (IH Us' ts' ltac:(lia) ltac:(lia)). lia.
Qed.

(* Keystone (Risk R2): under a join continuation linear at every bound index and
   CLOSED payloads, the N-simultaneous substitution adds at most the payloads'
   total fuel — the N-ary image of [linear_subst_fuel_le]. List induction over the
   payloads; each step is the binary lemma, with [deref_subst_closed_ca] carrying
   the linearity hypothesis to the next index. The one-step unfolding now goes
   through the genuine simultaneous fold's cons equation [subst_st_many_cons]
   (which lifts the tail payloads); under [Forall closed_st Us] that per-step lift
   is the identity ([map_lift_closed_id]), so the recursive list collapses back to
   [Us'] and the binary-lemma structure goes through verbatim. *)
Lemma linear_subst_many_fuel_le : forall Us T,
  (forall i, i < length Us -> deref_count_st i T <= 1) ->
  Forall closed_st Us ->
  st_total_fuel (subst_st_many T Us)
    <= st_total_fuel T + fold_right (fun U acc => st_total_fuel U + acc) 0 Us.
Proof.
  induction Us as [| U Us' IH]; intros T Hlin Hclosed.
  - simpl. rewrite subst_st_many_nil. lia.
  - inversion Hclosed as [| ? ? HU HUs' ]; subst.
    rewrite subst_st_many_cons. rewrite (map_lift_closed_id Us' HUs').
    simpl (fold_right _ _ (U :: Us')).
    assert (Hlin0 : deref_count_st 0 T <= 1) by (apply Hlin; simpl; lia).
    pose proof (linear_subst_fuel_le T U Hlin0) as Hstep.
    assert (Hlin' : forall i, i < length Us' ->
              deref_count_st i (subst_st T 0 (CQuote U)) <= 1).
    { intros i Hi.
      rewrite (proj2 (proj2 (deref_subst_closed_ca U HU)) T 0 i (Nat.le_0_l i)).
      apply Hlin. simpl. lia. }
    specialize (IH (subst_st T 0 (CQuote U)) Hlin' HUs'). lia.
Qed.

(* ── strict decrease on the funded fragment ─────────────────────────────── *)
Theorem funded_step_decreases : forall T T',
  funded_linear T -> ca_step T T' -> st_total_fuel T' < st_total_fuel T.
Proof.
  intros T T' Hf Hstep. revert Hf.
  induction Hstep; intro Hf; unfold funded_linear in *; simpl in *.
  - (* ca_rule1 *)
    destruct Hf as [[[Hlin _] _] _].
    pose proof (linear_subst_fuel_le T U Hlin). lia.
  - (* ca_rule2 *)
    destruct Hf as [[[[Hlin _] _] _] _].
    pose proof (linear_subst_fuel_le T U Hlin). lia.
  - (* ca_rule3 *)
    destruct Hf as [[[Hlin _] _] _].
    pose proof (linear_subst_fuel_le T U Hlin). lia.
  - (* ca_rule4 *)
    destruct Hf as [[[Hlin _] _] _].
    pose proof (linear_subst_fuel_le T U Hlin). lia.
  - (* ca_rule5 *)
    destruct Hf as [[[[Hlin _] _] _] _].
    pose proof (linear_subst_fuel_le T U Hlin). lia.
  - (* ca_join1: closed payloads + linear continuation ⇒ the consumed gate forces
       the strict drop (N-ary analogue of ca_rule1; payload fuel bounded by the
       keystone, the channel fuels appear on both sides). The payload closedness
       [Hcl] is no longer a rule premise — it is EXTRACTED from the funded sender
       bundle [Hsnds : funded_linear_caproc (join_sends xs Us)] via
       [funded_join_sends_closed] (funded_linear's CPOutput clause carries it). *)
    subst snds. destruct Hf as [[[Hidx _] Hsnds] _].
    match goal with
    | [ Hlen : length xs = length Us |- _ ] =>
        pose proof (funded_join_sends_closed xs Us Hlen Hsnds) as Hcl;
        rewrite (join_sends_fuel xs Us Hlen);
        rewrite Hlen in Hidx;
        pose proof (linear_subst_many_fuel_le Us _ Hidx Hcl) as Hbound;
        lia
    end.
  - (* ca_join2: combined token consumed forces the drop; the separately-signed
       senders' fuel appears on both sides, payload fuel bounded by the keystone.
       Closedness [Hcl] is extracted from the funded separately-signed bundle
       [Hsnds : funded_linear_st (signed_sends xs Us ts)] via
       [funded_signed_sends_closed]. *)
    subst snds. destruct Hf as [[[Hidx _] Hsnds] _].
    match goal with
    | [ HU : length xs = length Us, Ht : length xs = length ts |- _ ] =>
        pose proof (funded_signed_sends_closed xs Us ts HU Ht Hsnds) as Hcl;
        rewrite (signed_sends_fuel xs Us ts HU Ht);
        rewrite HU in Hidx;
        pose proof (linear_subst_many_fuel_le Us _ Hidx Hcl) as Hbound;
        lia
    end.
  - (* ca_par_l *) destruct Hf as [Hf1 _]. specialize (IHHstep Hf1). lia.
  - (* ca_par_r *) destruct Hf as [_ Hf2]. specialize (IHHstep Hf2). lia.
Qed.

(* ── strong normalization on the funded fragment ────────────────────────── *)

Definition funded_step_inv (T' T : signed_term) : Prop :=
  funded_linear T /\ ca_step T T'.

Theorem ca_well_founded_funded : well_founded funded_step_inv.
Proof.
  apply (wf_incl signed_term funded_step_inv (ltof signed_term st_total_fuel)).
  - intros T' T H. unfold funded_step_inv in H. destruct H as [Hf Hstep].
    unfold ltof. apply funded_step_decreases; assumption.
  - apply well_founded_ltof.
Qed.

Corollary ca_SN_funded : forall T, Acc funded_step_inv T.
Proof. apply ca_well_founded_funded. Qed.

(* Every funded reduction sequence has length at most st_total_fuel T. *)
Theorem ca_max_steps_bound_funded : forall T T',
  funded_linear T -> ca_step T T' -> st_total_fuel T' < st_total_fuel T.
Proof. exact funded_step_decreases. Qed.

(* ── the divergence witness: OFF the funded fragment, the measure can rise ──
   A non-linear continuation (CNVar 0 dereferenced twice) duplicates a
   token-bearing payload, so st_total_fuel strictly INCREASES. This refutes
   unconditional SN-by-measure and is exactly why funding is required. *)

Definition payload_with_fuel : signed_term :=
  STSigned (CPInput (CNVar 0) (STStack (TGate SUnit (TGate SUnit TUnit)))) SUnit.

Definition nonlinear_cont : signed_term :=
  STSigned (CPPar (CPDeref (CNVar 0)) (CPDeref (CNVar 0))) SUnit.

Theorem st_total_fuel_can_increase_off_funded :
  exists T T',
    ca_step T T' /\ ~ funded_linear T /\ st_total_fuel T < st_total_fuel T'.
Proof.
  exists (STPar (STSigned (CPPar (CPInput (CNVar 0) nonlinear_cont)
                                 (CPOutput (CNVar 0) payload_with_fuel)) SUnit)
                (STStack (TGate SUnit TUnit))).
  exists (STPar (subst_st nonlinear_cont 0 (CQuote payload_with_fuel))
                (STStack TUnit)).
  split; [| split].
  - apply ca_rule1.
  - intro Hf. vm_compute in Hf. decompose [and] Hf. lia.
  - vm_compute. apply Nat.lt_succ_diag_r.
Qed.
