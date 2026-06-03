(* ════════════════════════════════════════════════════════════════════════
   CAStepDeterminism.v — native step determinism for single-token systems
   (Stage 3d). Direct port of StepDeterminism.v to the native grammar; SN- and
   funding-INDEPENDENT (uses only the per-rule determinism lemmas + the
   single-token-node invariant). Within a single deploy at most one [STStack]
   node is in flight, so [ca_step] is deterministic — the justification for
   ordered (rather than commutative) single-deploy event hashing. Axiom-free.  *)

From Stdlib Require Import Lia.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import WrappingSubjectReduction.
From CostAccountedRho Require Import CAConfluence.

(* ── single-token-node invariant ────────────────────────────────────────── *)

Fixpoint st_token_node_count (T : signed_term) : nat :=
  match T with
  | STSigned _ _ => 0
  | STStack _    => 1
  | STPar A B    => st_token_node_count A + st_token_node_count B
  end.

Definition single_token_st (T : signed_term) : Prop := st_token_node_count T <= 1.

Lemma ca_step_requires_token_node : forall S T,
  ca_step S T -> st_token_node_count S >= 1.
Proof. intros S T H. induction H; simpl in *; lia. Qed.

Lemma no_token_no_step : forall S,
  st_token_node_count S = 0 -> forall T, ~ ca_step S T.
Proof.
  intros S Hzero T Hstep. apply ca_step_requires_token_node in Hstep. lia.
Qed.

Lemma token_split_zero : forall a b, a + b <= 1 -> a >= 1 -> b = 0.
Proof. intros. lia. Qed.

(* NOTE: unlike the old model, st_token_node_count is NOT monotone natively — a
   for-continuation that releases token nodes (a located purse) surfaces nodes
   that were guarded under its STSigned wrapper. So [single_token_st] is not
   preserved unconditionally; [single_token_path_unique] therefore carries the
   single-token invariant along the path as an explicit hypothesis (it holds for
   the single-token-chain deploys whose ordering this result justifies). *)

(* ── step determinism ───────────────────────────────────────────────────── *)

Theorem ca_step_deterministic : forall S T1 T2,
  single_token_st S -> ca_step S T1 -> ca_step S T2 -> T1 = T2.
Proof.
  intros S T1 T2 Hsingle Hstep1. revert T2.
  induction Hstep1; intros T2 Hstep2.
  - (* ca_rule1 *) symmetry. exact (ca_step_rule1_det _ _ _ _ _ _ Hstep2).
  - (* ca_rule2: two STStack nodes — impossible under single_token_st *)
    unfold single_token_st in Hsingle. simpl in Hsingle. lia.
  - (* ca_rule3: same LHS shape as ca_rule1 *)
    symmetry. exact (ca_step_rule1_det _ _ _ _ _ _ Hstep2).
  - (* ca_rule4 *) symmetry. exact (ca_step_rule4_det _ _ _ _ _ _ _ Hstep2).
  - (* ca_rule5: two STStack nodes — impossible *)
    unfold single_token_st in Hsingle. simpl in Hsingle. lia.
  - (* ca_join1: single combined token, unique residual via ca_step_join1_det *)
    subst snds. symmetry.
    match goal with
    | [ Hl : length xs = length Us |- _ ] => exact (ca_step_join1_det xs Us T s t T2 Hl Hstep2)
    end.
  - (* ca_par_l *)
    unfold single_token_st in Hsingle. simpl in Hsingle.
    assert (Hc1 : st_token_node_count S1 >= 1)
      by (eapply ca_step_requires_token_node; eassumption).
    assert (Hz2 : st_token_node_count S2 = 0)
      by (eapply token_split_zero; eassumption).
    inversion Hstep2; subst.
    all: try solve_no_substep.
    + assert (Hs1 : single_token_st S1) by (unfold single_token_st; lia).
      f_equal. exact (IHHstep1 Hs1 _ H2).
    + exfalso. eapply no_token_no_step; eassumption.
  - (* ca_par_r *)
    unfold single_token_st in Hsingle. simpl in Hsingle.
    assert (Hc2 : st_token_node_count S2 >= 1)
      by (eapply ca_step_requires_token_node; eassumption).
    assert (Hz1 : st_token_node_count S1 = 0) by lia.
    inversion Hstep2; subst.
    all: try solve_no_substep.
    + exfalso. eapply no_token_no_step; eassumption.
    + assert (Hs2 : single_token_st S2) by (unfold single_token_st; lia).
      f_equal. exact (IHHstep1 Hs2 _ H2).
Qed.

(* ── unique reduction-path length ───────────────────────────────────────── *)

Inductive ca_reachable_n : nat -> signed_term -> signed_term -> Prop :=
  | carn_refl : forall S, ca_reachable_n 0 S S
  | carn_step : forall n S1 S2 S3,
      ca_step S1 S2 -> ca_reachable_n n S2 S3 -> ca_reachable_n (S n) S1 S3.

Corollary single_token_path_unique : forall S,
  (forall k Sk, ca_reachable_n k S Sk -> single_token_st Sk) ->
  forall T1 T2 n1 n2,
    ca_reachable_n n1 S T1 -> ca_terminal T1 ->
    ca_reachable_n n2 S T2 -> ca_terminal T2 ->
    n1 = n2.
Proof.
  intros S Hsing T1 T2 n1 n2 Hpath1 Hterm1 Hpath2 Hterm2.
  revert T2 n2 Hpath2 Hterm2 Hsing.
  induction Hpath1 as [S0 | n1' S0 S' T1 Hstep1 Htail1 IH];
    intros T2 n2 Hpath2 Hterm2 Hsing.
  - inversion Hpath2 as [S_eq | n2' S_src S_mid S_tgt Hstep Htail]; subst.
    + reflexivity.
    + exfalso. exact (Hterm1 S_mid Hstep).
  - inversion Hpath2 as [S_eq | n2' S_src S_mid S_tgt Hstep Htail]; subst.
    + exfalso. exact (Hterm2 S' Hstep1).
    + assert (Hsingle0 : single_token_st S0) by (apply (Hsing 0 S0); constructor).
      assert (Heq : S' = S_mid)
        by (exact (ca_step_deterministic S0 S' S_mid Hsingle0 Hstep1 Hstep)).
      subst S_mid.
      assert (Hsing' : forall k Sk, ca_reachable_n k S' Sk -> single_token_st Sk).
      { intros k Sk Hk. apply (Hsing (S k) Sk). econstructor; eassumption. }
      f_equal. exact (IH Hterm1 T2 n2' Htail Hterm2 Hsing').
Qed.
