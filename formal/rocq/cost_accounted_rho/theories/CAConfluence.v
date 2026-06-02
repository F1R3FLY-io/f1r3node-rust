(* ════════════════════════════════════════════════════════════════════════
   CAConfluence.v — native per-rule determinism + local confluence (Stage 3c).

   Ports the Confluence.v skeleton to the native grammar: per-rule determinism
   ([ca_step_rule1_det] … [ca_step_rule5_det]) and the local-confluence diamond
   ([ca_local_confluence]). Local confluence does NOT need strong normalization,
   so it holds unconditionally. The "no redex steps in isolation" facts are the
   native [no_leak_requires_token] / [no_leak_stack_inert]. Full confluence +
   cost determinism (which need Newman = local confluence + SN on the funded
   fragment) are built on top in CACostDeterminism. Axiom-free.               *)

From Stdlib Require Import Lia.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import WrappingSubjectReduction.

(* ── irreducibility in isolation + signature acyclicity ─────────────────── *)

Lemma SAnd_acyclic_left : forall a b, SAnd a b <> a.
Proof. intros a b Heq. apply (f_equal sig_size) in Heq; simpl in Heq; lia. Qed.

Lemma SAnd_acyclic_right : forall a b, a <> SAnd a b.
Proof. intros a b Heq. symmetry in Heq. revert Heq. apply SAnd_acyclic_left. Qed.

(* Discharge impossible COMM-vs-PAR overlaps: a wrapped redex or a stack cannot
   step in isolation, and a proper sub-signature cannot equal its SAnd parent. *)
Ltac solve_no_substep :=
  match goal with
  | [ H : ca_step (STSigned _ _) _ |- _ ] =>
      exfalso; eapply no_leak_requires_token; exact H
  | [ H : ca_step (STStack _) _ |- _ ] =>
      exfalso; eapply no_leak_stack_inert; exact H
  | [ H : SAnd ?a ?b = ?a |- _ ] =>
      exfalso; eapply SAnd_acyclic_left; exact H
  | [ H : ?a = SAnd ?a ?b |- _ ] =>
      exfalso; eapply SAnd_acyclic_right; exact H
  | [ H : ca_step (STPar (STSigned _ _) (STSigned _ _)) _ |- _ ] =>
      inversion H; subst; solve_no_substep
  | [ H : ca_step (STPar (STSigned _ _) (STStack _)) _ |- _ ] =>
      inversion H; subst; solve_no_substep
  | [ H : ca_step (STPar (STPar (STSigned _ _) (STSigned _ _)) (STStack _)) _ |- _ ] =>
      inversion H; subst; solve_no_substep
  end.

(* ── per-rule determinism (each rule's RHS is determined by the LHS) ──────── *)

Lemma ca_step_rule1_det : forall x T U s t Sb,
  ca_step (STPar (STSigned (CPPar (CPInput x T) (CPOutput x U)) s)
                 (STStack (TGate s t))) Sb ->
  Sb = STPar (subst_st T 0 (CQuote U)) (STStack t).
Proof.
  intros x T U s t Sb H. inversion H; subst;
    try reflexivity;
    try (exfalso; eapply no_leak_requires_token; eassumption);
    try (exfalso; eapply no_leak_stack_inert; eassumption).
Qed.

Lemma ca_step_rule2_det : forall x T U s1 s2 t1 t2 Sb,
  ca_step (STPar (STPar (STSigned (CPPar (CPInput x T) (CPOutput x U)) (SAnd s1 s2))
                        (STStack (TGate s1 t1)))
                 (STStack (TGate s2 t2))) Sb ->
  Sb = STPar (STPar (subst_st T 0 (CQuote U)) (STStack t1)) (STStack t2).
Proof.
  intros x T U s1 s2 t1 t2 Sb H. inversion H; subst;
    try reflexivity; try solve_no_substep.
Qed.

Lemma ca_step_rule4_det : forall x T U s1 s2 t Sb,
  ca_step (STPar (STPar (STSigned (CPInput x T) s1)
                        (STSigned (CPOutput x U) s2))
                 (STStack (TGate (SAnd s1 s2) t))) Sb ->
  Sb = STPar (subst_st T 0 (CQuote U)) (STStack t).
Proof.
  intros x T U s1 s2 t Sb H. inversion H; subst;
    try reflexivity; try solve_no_substep.
Qed.

Lemma ca_step_rule5_det : forall x T U s1 s2 t1 t2 Sb,
  ca_step (STPar (STPar (STPar (STSigned (CPInput x T) s1)
                              (STSigned (CPOutput x U) s2))
                        (STStack (TGate s1 t1)))
                 (STStack (TGate s2 t2))) Sb ->
  Sb = STPar (STPar (subst_st T 0 (CQuote U)) (STStack t1)) (STStack t2).
Proof.
  intros x T U s1 s2 t1 t2 Sb H. inversion H; subst;
    try reflexivity; try solve_no_substep.
Qed.

(* ── terminal states ────────────────────────────────────────────────────── *)

Definition ca_terminal (T : signed_term) : Prop := forall T', ~ ca_step T T'.

(* ── local confluence (the diamond) — unconditional ─────────────────────── *)

Lemma ca_local_confluence : forall S Sa Sb,
  ca_step S Sa -> ca_step S Sb ->
  Sa = Sb \/ exists S', ca_step Sa S' /\ ca_step Sb S'.
Proof.
  intros S Sa Sb Hstep1. revert Sb.
  induction Hstep1; intros Sb Hstep2.
  - left. symmetry. exact (ca_step_rule1_det _ _ _ _ _ _ Hstep2).
  - left. symmetry. exact (ca_step_rule2_det _ _ _ _ _ _ _ _ Hstep2).
  - left. symmetry. exact (ca_step_rule1_det _ _ _ _ _ _ Hstep2).
  - left. symmetry. exact (ca_step_rule4_det _ _ _ _ _ _ _ Hstep2).
  - left. symmetry. exact (ca_step_rule5_det _ _ _ _ _ _ _ _ Hstep2).
  - (* ca_par_l *)
    inversion Hstep2; subst.
    all: try solve_no_substep.
    + destruct (IHHstep1 _ H2) as [Heq | [S' [Hs1 Hs2]]].
      * left. subst. reflexivity.
      * right. exists (STPar S' S2). split; apply ca_par_l; assumption.
    + right. exists (STPar S1' S2'). split.
      * apply ca_par_r. exact H2.
      * apply ca_par_l. exact Hstep1.
  - (* ca_par_r *)
    inversion Hstep2; subst.
    all: try solve_no_substep.
    + right. exists (STPar S1' S2'). split.
      * apply ca_par_l. exact H2.
      * apply ca_par_r. exact Hstep1.
    + destruct (IHHstep1 _ H2) as [Heq | [S' [Hs1 Hs2]]].
      * left. subst. reflexivity.
      * right. exists (STPar S1 S'). split; apply ca_par_r; assumption.
Qed.
