(* ═══════════════════════════════════════════════════════════════════════════
   StepDeterminism.v — Step Determinism for Single-Token Systems
   ═══════════════════════════════════════════════════════════════════════════

   Proves that within a single-deploy execution — where at most one [SToken]
   node is ever in flight at any moment — the cost-accounted reduction
   relation [ca_step] is DETERMINISTIC: from any given system state there
   is at most one possible successor.

   This is the formal justification for the "consistent ordering"
   interpretation of the cost-accounting paper: within a single deploy,
   the token chain structure
       T⟦σ: T'⟧ = send(N⟦σ⟧, T⟦T'⟧)
   releases exactly one token message at a time, so all validators fire
   fuel-gate events in the same canonical order — not because of external
   serialization, but because the process structure admits only one
   reduction sequence. Ordered (rather than commutative) event hashing for
   single-deploy execution is therefore correct.

   The proof uses the per-rule determinism lemmas from [Confluence.v]
   ([ca_step_rule1_det] etc.) together with [SSigned_no_step] and
   [SToken_no_step] to rule out competing reductions.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition / Theorem      │ Design property
   ───────────────────────────────┼──────────────────────────────────────────
   sys_token_node_count           │ "number of SToken constructors in S"
   single_token_sys               │ "at most one SToken node in S"
   ca_step_requires_token_node    │ "every ca_step needs ≥ 1 SToken node"
   no_token_no_step               │ "no SToken node → stuck"
   token_split_zero               │ arithmetic helper
   ca_step_deterministic          │ "ca_step is deterministic under
                                  │  single_token_sys" (headline theorem)
   single_token_path_unique       │ "reduction path length is unique
                                  │  (hence path is unique) under
                                  │  single_token_sys" (corollary)
   ─────────────────────────────────────────────────────────────────────────

   Dependencies: RhoSyntax, CostAccountedSyntax, CostAccountedReduction,
                 TokenConservation, StrongNormalization, Confluence
                 (this project)
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Lia.

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CostAccountedReduction.
From CostAccountedRho Require Import TokenConservation.
From CostAccountedRho Require Import StrongNormalization.
From CostAccountedRho Require Import Confluence.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Single-Token Invariant
   ═══════════════════════════════════════════════════════════════════════════

   [sys_token_node_count] counts the NUMBER OF [SToken] CONSTRUCTORS in a
   system — distinct from [system_token_count] (which counts the depth /
   fuel units inside tokens). A system with [sys_token_node_count S = 1]
   has exactly one [SToken] leaf anywhere in its parallel tree.

   [single_token_sys S] is the invariant: at most one such leaf exists.
   Within a single-deploy execution this holds at every point: the token
   chain encoding releases one token message at a time, so there is never
   more than one [SToken] in flight.                                       *)

Fixpoint sys_token_node_count (S : system) : nat :=
  match S with
  | SSigned _ _ => 0
  | SToken _    => 1
  | SPar S1 S2  => sys_token_node_count S1 + sys_token_node_count S2
  end.

Definition single_token_sys (S : system) : Prop :=
  sys_token_node_count S <= 1.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Supporting Lemmas
   ═══════════════════════════════════════════════════════════════════════════ *)

(** Every [ca_step] requires at least one [SToken] node in the source.
    Proof: case analysis on the constructor — every rule either directly
    matches on an [SToken] or reduces a sub-system that contains one
    (shown by induction on PAR rules). *)
Lemma ca_step_requires_token_node : forall S T,
  ca_step S T -> sys_token_node_count S >= 1.
Proof.
  intros S T Hstep.
  induction Hstep; simpl; lia.
Qed.

(** A system with no [SToken] nodes cannot step.
    Direct corollary of [ca_step_requires_token_node]. *)
Lemma no_token_no_step : forall S,
  sys_token_node_count S = 0 -> forall T, ~ ca_step S T.
Proof.
  intros S Hzero T Hstep.
  apply ca_step_requires_token_node in Hstep.
  lia.
Qed.

(** Arithmetic helper: if a + b ≤ 1 and a ≥ 1, then b = 0. *)
Lemma token_split_zero : forall a b,
  a + b <= 1 -> a >= 1 -> b = 0.
Proof. intros. lia. Qed.

(** Every [ca_step] monotonically decreases (or preserves) the number
    of [SToken] constructors. All five COMM rules consume one [SToken]
    leaf and produce a fresh one from its inner gate, keeping the count
    the same per leaf (Rule 2 / Rule 5 collapse two SToken leaves into
    two SToken leaves, preserving count; Rule 4 likewise preserves
    total). PAR rules recurse. *)
Lemma sys_token_node_count_monotonic : forall S S',
  ca_step S S' ->
  sys_token_node_count S' <= sys_token_node_count S.
Proof.
  intros S S' Hstep.
  induction Hstep; simpl in *; lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Step Determinism
   ═══════════════════════════════════════════════════════════════════════════

   Headline theorem: in a [single_token_sys], [ca_step] is deterministic.

   Proof strategy: induction on [Hstep1], case analysis on [Hstep2].

   ── COMM rule cases (ca_rule1 … ca_rule5) ──────────────────────────────
   Each COMM rule fires on a specific syntactic pattern that includes
   exactly one [SToken] constructor. We dispatch to the corresponding
   per-rule determinism lemma from [Confluence.v]:
     [ca_step_rule1_det], [ca_step_rule2_det],
     [ca_step_rule4_det], [ca_step_rule5_det].
   ([ca_step_rule1_det] covers both ca_rule1 and ca_rule3.)

   ── PAR rule cases (ca_par_l, ca_par_r) ────────────────────────────────
   When [Hstep1 = ca_par_l S1 S1' S2]:
   - [ca_step S1 S1'] requires [sys_token_node_count S1 ≥ 1].
   - [single_token_sys (SPar S1 S2)] forces [sys_token_node_count S2 = 0].
   - [Hstep2] cannot be [ca_par_r] (step in S2 needs a token, S2 has none).
   - [Hstep2] cannot be any COMM rule matching the whole [SPar S1 S2] as a
     COMM pattern: all COMM patterns have their left sub-term as [SSigned]
     (possibly nested in [SPar]), which cannot step; but [Hstep1] says [S1]
     DOES step, making [S1] an [SSigned] would contradict [SSigned_no_step].
     [solve_no_substep] discharges these contradictions.
   - [Hstep2] must also be [ca_par_l] (step in S1). By the inductive
     hypothesis — which applies because [single_token_sys S1] holds
     (S1's token count ≤ total ≤ 1) — the inner steps are equal, giving
     [T1 = T2].

   The [ca_par_r] case is symmetric.                                      *)

Theorem ca_step_deterministic : forall S T1 T2,
  single_token_sys S ->
  ca_step S T1 ->
  ca_step S T2 ->
  T1 = T2.
Proof.
  intros S T1 T2 Hsingle Hstep1.
  revert T2.
  induction Hstep1; intros T2 Hstep2.

  - (* ca_rule1 *)
    symmetry. exact (ca_step_rule1_det _ _ _ _ _ _ Hstep2).

  - (* ca_rule2: system has TWO SToken nodes (TGate s1 t1 and TGate s2 t2).
       [single_token_sys] requires ≤ 1, so this case is impossible. *)
    unfold single_token_sys in Hsingle. simpl in Hsingle. lia.

  - (* ca_rule3 — same LHS shape as ca_rule1 *)
    symmetry. exact (ca_step_rule1_det _ _ _ _ _ _ Hstep2).

  - (* ca_rule4 *)
    symmetry. exact (ca_step_rule4_det _ _ _ _ _ _ _ Hstep2).

  - (* ca_rule5: system has TWO SToken nodes — impossible under single_token_sys *)
    unfold single_token_sys in Hsingle. simpl in Hsingle. lia.

  - (* ca_par_l: S = SPar S1 S2, Hstep1 : ca_step S1 S1', T1 = SPar S1' S2 *)
    (* Determine that S2 has no SToken nodes. *)
    unfold single_token_sys in Hsingle.
    simpl in Hsingle.
    assert (Hcount_S1 : sys_token_node_count S1 >= 1).
    { eapply ca_step_requires_token_node. eassumption. }
    assert (HzeroS2 : sys_token_node_count S2 = 0).
    { eapply token_split_zero; eassumption. }
    (* Invert Hstep2. *)
    inversion Hstep2; subst.
    (* COMM rules: each has a specific left-sub-term that is SSigned (or
       SPar of SSigned nodes), which cannot step. But Hstep1 says S1 steps.
       solve_no_substep closes these cases. *)
    all: try solve_no_substep.
    + (* ca_par_l: both steps in left branch *)
      assert (Hsingle_S1 : single_token_sys S1).
      { unfold single_token_sys. lia. }
      f_equal.
      exact (IHHstep1 Hsingle_S1 _ H2).
    + (* ca_par_r: step in S2, but S2 has no tokens → contradiction *)
      exfalso.
      eapply no_token_no_step; eassumption.

  - (* ca_par_r: S = SPar S1 S2, Hstep1 : ca_step S2 S2', T1 = SPar S1 S2' *)
    unfold single_token_sys in Hsingle.
    simpl in Hsingle.
    assert (Hcount_S2 : sys_token_node_count S2 >= 1).
    { eapply ca_step_requires_token_node. eassumption. }
    assert (HzeroS1 : sys_token_node_count S1 = 0).
    { lia. }
    inversion Hstep2; subst.
    all: try solve_no_substep.
    + (* ca_par_l: step in S1, but S1 has no tokens → contradiction *)
      exfalso.
      eapply no_token_no_step; eassumption.
    + (* ca_par_r: both steps in right branch *)
      assert (Hsingle_S2 : single_token_sys S2).
      { unfold single_token_sys. lia. }
      f_equal.
      exact (IHHstep1 Hsingle_S2 _ H2).
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Unique Reduction Path Length
   ═══════════════════════════════════════════════════════════════════════════

   In a single-token system, since [ca_step] is deterministic, all
   reduction sequences from [S] to a terminal state have the same length
   and pass through the same intermediate states. The corollary below
   formalises the length claim via [ca_reachable_n] (n-step reachability).

   Together with [ca_step_deterministic], this justifies ordered event
   hashing for single-deploy execution: the k-th fuel-gate event is
   always the same event for every validator.                             *)

(** n-step reachability: [ca_reachable_n n S T] means T is reachable
    from S in exactly n steps. *)
Inductive ca_reachable_n : nat -> system -> system -> Prop :=
  | carn_refl : forall S,
      ca_reachable_n 0 S S
  | carn_step : forall n S1 S2 S3,
      ca_step S1 S2 ->
      ca_reachable_n n S2 S3 ->
      ca_reachable_n (S n) S1 S3.

(** A terminal system reached by an n-step path must be reached in
    exactly n steps from any other path in a single-token system. *)
Corollary single_token_path_unique : forall S,
  single_token_sys S ->
  forall T1 T2 n1 n2,
    ca_reachable_n n1 S T1 -> ca_terminal T1 ->
    ca_reachable_n n2 S T2 -> ca_terminal T2 ->
    n1 = n2.
Proof.
  intros S Hsingle T1 T2 n1 n2 Hpath1 Hterm1 Hpath2 Hterm2.
  (* By ca_step_deterministic, at each step both paths are forced to
     take the same step, so n1 steps of the first path and n2 steps of
     the second both reach the same unique terminal (by ca_cost_deterministic
     and ca_normal_form_unique). We show n1 = n2 by induction on Hpath1,
     generalising over Hpath2. *)
  revert T2 n2 Hpath2 Hterm2 Hsingle.
  induction Hpath1 as [S | n1' S S' T1 Hstep1 Htail1 IH];
    intros T2 n2 Hpath2 Hterm2 Hsingle.
  - (* n1 = 0: T1 = S, which is terminal. *)
    inversion Hpath2 as [S_eq | n2' S_src S_mid S_tgt Hstep Htail]; subst.
    + (* n2 = 0: T2 = S. Done. *)
      reflexivity.
    + (* n2 ≥ 1: S steps to S_mid, but S = T1 is terminal. Contradiction. *)
      exfalso. exact (Hterm1 S_mid Hstep).
  - (* n1 = S n1': S steps to S' in one step, then S' ⤳^n1' T1. *)
    inversion Hpath2 as [S_eq | n2' S_src S_mid S_tgt Hstep Htail]; subst.
    + (* n2 = 0: T2 = S, which is terminal. But S steps via Hstep1. Contradiction. *)
      exfalso. exact (Hterm2 S' Hstep1).
    + (* n2 = S n2': S steps to S_mid in one step, then S_mid ⤳^n2' T2. *)
      (* By determinism, S' = S_mid. *)
      assert (Heq : S' = S_mid).
      { exact (ca_step_deterministic S S' S_mid Hsingle Hstep1 Hstep). }
      subst S_mid.
      (* S' inherits single_token_sys from S via monotonicity of the
         token-node count along a single step. *)
      assert (Hsingle' : single_token_sys S').
      {
        unfold single_token_sys in *.
        assert (Hle : sys_token_node_count S' <= sys_token_node_count S)
          by (apply sys_token_node_count_monotonic; exact Hstep1).
        lia.
      }
      f_equal.
      exact (IH Hterm1 T2 n2' Htail Hterm2 Hsingle').
Qed.
