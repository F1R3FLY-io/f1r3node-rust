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
