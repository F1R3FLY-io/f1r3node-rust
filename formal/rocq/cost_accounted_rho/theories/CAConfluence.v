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

(* The separately-signed sender bundle (J2) has NO token within, so it cannot
   step in isolation — defined here (ahead of [solve_no_substep]) so the tactic
   can discharge the J2 receiver|senders block. *)
Lemma signed_sends_no_step : forall xs Us ts S', ~ ca_step (signed_sends xs Us ts) S'.
Proof.
  induction xs as [| x xs' IH]; intros Us ts S' Hstep.
  - simpl in Hstep. eapply no_leak_requires_token; eassumption.
  - destruct Us as [| U Us']; destruct ts as [| tt ts']; simpl in Hstep;
      try (eapply no_leak_requires_token; eassumption).
    inversion Hstep; subst.
    + eapply no_leak_requires_token; eassumption.
    + eapply IH; eassumption.
Qed.

(* Discharge impossible COMM-vs-PAR overlaps: a wrapped redex, a stack, or a
   separately-signed sender bundle cannot step in isolation, and a proper
   sub-signature cannot equal its SAnd parent. *)
Ltac solve_no_substep :=
  match goal with
  | [ H : ca_step (STSigned _ _) _ |- _ ] =>
      exfalso; eapply no_leak_requires_token; exact H
  | [ H : ca_step (STStack _) _ |- _ ] =>
      exfalso; eapply no_leak_stack_inert; exact H
  | [ H : ca_step (signed_sends _ _ _) _ |- _ ] =>
      exfalso; eapply signed_sends_no_step; exact H
  | [ H : SAnd ?a ?b = ?a |- _ ] =>
      exfalso; eapply SAnd_acyclic_left; exact H
  | [ H : ?a = SAnd ?a ?b |- _ ] =>
      exfalso; eapply SAnd_acyclic_right; exact H
  | [ H : ca_step (STPar (STSigned (CPJoin _ _) _) _) _ |- _ ] =>
      inversion H; subst; solve_no_substep
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

(* join_sends is injective in its payloads (given matching arities): the redex
   structure pins the substituted payloads, hence the join RHS is unique. *)
Lemma join_sends_injective : forall xs Us Us',
  length xs = length Us -> length xs = length Us' ->
  join_sends xs Us = join_sends xs Us' -> Us = Us'.
Proof.
  induction xs as [| x xs' IH]; intros [| U Us0] [| U' Us0'] HlU HlU' Heq;
    simpl in *; try discriminate; try reflexivity.
  inversion Heq; subst. f_equal. apply (IH Us0 Us0'); [ lia | lia | assumption ].
Qed.

(* Determinism of the join firing: the N-ary whole-join LHS has a unique residual
   (the snds-variable rule keeps this inversion terminating, Risk R3/R4 resolved). *)
Lemma ca_step_join1_det : forall xs Us T s t Sb,
  length xs = length Us ->
  ca_step (STPar (STSigned (CPPar (CPJoin xs T) (join_sends xs Us)) s)
                 (STStack (TGate s t))) Sb ->
  Sb = STPar (subst_st_many T Us) (STStack t).
Proof.
  intros xs Us T s t Sb Hlen H. inversion H; subst.
  - (* ca_join1 — pin the payloads via join_sends injectivity *)
    match goal with
    | [ Heq : join_sends xs Us = join_sends xs ?Us0, Hl0 : length xs = length ?Us0 |- _ ] =>
        rewrite (join_sends_injective xs Us Us0 Hlen Hl0 Heq); reflexivity
    end.
  - (* ca_par_l — receiver-side sub-step impossible *)
    exfalso; eapply no_leak_requires_token; eauto.
  - (* ca_par_r — token-stack sub-step impossible *)
    exfalso; eapply no_leak_stack_inert; eauto.
Qed.

(* ── J2 (separately-signed senders, combined token) determinism ─────────────
   signed_sends is injective in payloads AND sender-signatures (given arities):
   the redex structure pins both, hence the J2 residual is unique. *)
Lemma signed_sends_injective : forall xs Us Us' ts ts',
  length xs = length Us -> length xs = length Us' ->
  length xs = length ts -> length xs = length ts' ->
  signed_sends xs Us ts = signed_sends xs Us' ts' -> Us = Us' /\ ts = ts'.
Proof.
  induction xs as [| x xs' IH];
    intros [| U Us0] [| U' Us0'] [| tt ts0] [| tt' ts0'] HU HU' Ht Ht' Heq;
    simpl in *; try discriminate; try (split; reflexivity).
  inversion Heq; subst.
  destruct (IH Us0 Us0' ts0 ts0' ltac:(lia) ltac:(lia) ltac:(lia) ltac:(lia) H2)
    as [HeU Hets].
  subst. split; reflexivity.
Qed.

(* Determinism of the J2 firing: the separately-signed N-ary join with one combined
   token has a unique residual (signed_sends injectivity pins the payloads). J2
   holds ONE token cell, so this is a genuine determinism lemma (Risk R4). *)
Lemma ca_step_join2_det : forall xs Us ts T s1 t Sb,
  length xs = length Us -> length xs = length ts ->
  ca_step (STPar (STPar (STSigned (CPJoin xs T) s1) (signed_sends xs Us ts))
                 (STStack (TGate (join_token_key s1 ts) t))) Sb ->
  Sb = STPar (subst_st_many T Us) (STStack t).
Proof.
  intros xs Us ts T s1 t Sb HU Ht H. inversion H; subst.
  - (* ca_join2 — pin payloads via signed_sends injectivity *)
    match goal with
    | [ Heq : signed_sends xs Us ts = signed_sends xs ?Us0 ?ts0,
        HU0 : length xs = length ?Us0, Ht0 : length xs = length ?ts0 |- _ ] =>
        destruct (signed_sends_injective xs Us Us0 ts ts0 HU HU0 Ht Ht0 Heq) as [HeU Hets];
        rewrite HeU; reflexivity
    end.
  - (* ca_par_l — the receiver|senders block has no token, cannot step *)
    exfalso.
    match goal with
    | [ Hb : ca_step (STPar (STSigned (CPJoin _ _) _) (signed_sends _ _ _)) _ |- _ ] =>
        inversion Hb; subst;
        [ eapply no_leak_requires_token; eassumption
        | eapply signed_sends_no_step; eassumption ]
    end.
  - (* ca_par_r — token-stack sub-step impossible *)
    exfalso; eapply no_leak_stack_inert; eauto.
Qed.

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
  - (* ca_join1 — unique residual via ca_step_join1_det *)
    subst snds. left. symmetry.
    match goal with
    | [ Hl : length xs = length Us |- _ ] => exact (ca_step_join1_det xs Us T s t Sb Hl Hstep2)
    end.
  - (* ca_join2 — unique residual via ca_step_join2_det *)
    subst snds. left. symmetry.
    match goal with
    | [ HU : length xs = length Us, Ht : length xs = length ts |- _ ] =>
        exact (ca_step_join2_det xs Us ts T s1 t Sb HU Ht Hstep2)
    end.
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
