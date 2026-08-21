(* ═══════════════════════════════════════════════════════════════════════════
   Confluence.v — Cost Determinism via Confluence of Cost-Accounted Reduction
   ═══════════════════════════════════════════════════════════════════════════

   Proves the headline result: the total cost (tokens consumed) when a
   system reaches a terminal state is independent of the reduction order.
   This is the formal justification for using [FuturesUnordered] in the
   evaluator: validators arrive at the same cost regardless of how their
   async runtime schedules COMM events.

   The proof proceeds in four stages:

   1. **Irreducibility of [SSigned] and [SToken] in isolation** (Section 1):
      these cannot take a [ca_step] alone, which means COMM rules at the
      top level are deterministic and do not compete with PAR rules.

   2. **Local confluence** (Section 2): any two one-step divergences from
      the same system can be joined in one step each. The proof is by
      case analysis on the two [ca_step] derivations.

      Per-rule determinism lemmas ([ca_step_rule1_det] through
      [ca_step_rule5_det]) isolate each [inversion] to a small context,
      avoiding the proof-term blowup that results from running [inversion]
      inside the large accumulated context of [ca_local_confluence] itself.

   3. **Newman's lemma** (Section 3): local confluence + strong
      normalisation (from StrongNormalization.v) implies full confluence.
      This follows Coquand's 1994 constructive proof — no classical
      axioms are needed.

   4. **Cost determinism** (Section 4): full confluence implies that
      terminal states (normal forms) are unique, which immediately gives
      unique terminal token counts.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Theorem                 │ Paper Property
   ─────────────────────────────┼────────────────────────────────────────────
   ca_step_rule1_det            │ "Rule 1 (and 3) output is uniquely
                                │  determined by the source pattern"
   ca_step_rule2_det            │ "Rule 2 output is uniquely determined"
   ca_step_rule4_det            │ "Rule 4 output is uniquely determined"
   ca_step_rule5_det            │ "Rule 5 output is uniquely determined"
   ca_local_confluence          │ "Local diamond: any two one-step
                                │  divergences can be joined"
   newman                       │ "Newman's lemma: WCR + SN → CR
                                │  (Coquand 1994, constructive)"
   ca_confluent                 │ "Full confluence of ca_step"
   ca_normal_form_unique        │ "Normal forms are unique"
   ca_cost_deterministic        │ "Cost determinism: all terminal states
                                │  from S have the same token count"
   ─────────────────────────────────────────────────────────────────────────

   References:
   [1] T. Coquand, "A proof of Newman's lemma using well-founded
       induction," Manuscript, Chalmers University, 1994.

   Dependencies: RhoSyntax, CostAccountedSyntax, CostAccountedReduction,
                 TokenConservation, StrongNormalization (this project)
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Lia.

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CostAccountedReduction.
From CostAccountedRho Require Import TokenConservation.
From CostAccountedRho Require Import StrongNormalization.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Irreducibility of SSigned and SToken in Isolation
   ═══════════════════════════════════════════════════════════════════════════

   Every [ca_step] rule requires a parallel composition of at least one
   [SSigned] and one [SToken] sub-system. Therefore neither [SSigned]
   nor [SToken] alone can step. This eliminates many cases in the
   confluence proof: when a COMM rule fires at the top level, no PAR
   rule can also fire, because the sub-systems cannot step alone.        *)

Lemma SSigned_no_step : forall P s S',
  ~ ca_step (SSigned P s) S'.
Proof.
  intros P s S' Hstep. inversion Hstep.
Qed.

Lemma SToken_no_step : forall t S',
  ~ ca_step (SToken t) S'.
Proof.
  intros t S' Hstep. inversion Hstep.
Qed.

(** A signature cannot equal a strictly-larger signature containing it
    as a direct sub-term — symmetric versions for either side of the
    equation. Used to rule out COMM-vs-COMM inner firings that would
    force a signature to equal a proper sub-signature of itself (e.g.
    [SAnd s1 s2 = s1]), which is structurally impossible because the
    LHS is strictly larger than the RHS. The measure [sig_size] from
    [CostAccountedSyntax] gives the proof by a single [f_equal]. *)
Lemma SAnd_acyclic_left : forall a b, SAnd a b <> a.
Proof.
  intros a b Heq.
  apply (f_equal sig_size) in Heq; simpl in Heq; lia.
Qed.

Lemma SAnd_acyclic_right : forall a b, a <> SAnd a b.
Proof.
  intros a b Heq. symmetry in Heq. revert Heq. apply SAnd_acyclic_left.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Local Confluence (Diamond Property)
   ═══════════════════════════════════════════════════════════════════════════

   If [S ⤳ S1] and [S ⤳ S2], then either [S1 = S2] (same step) or
   there exists [S'] reachable from both [S1] and [S2] in one step.

   The proof is by induction on the first derivation with case analysis
   on the second. The key observations are:

   (a) COMM rules match specific syntactic patterns. When a COMM rule
       applies at the top level, the sub-systems ([SSigned], [SToken])
       cannot step alone, so no PAR rule can also apply.

   (b) When two different COMM rules apply to the same system, they
       produce identical results (the patterns overlap only when the
       rules agree on the result).

   (c) When two PAR rules fire in different branches, the steps
       commute and can be completed to [SPar S1' S2'].

   (d) When two PAR rules fire in the same branch, the inductive
       hypothesis provides the diamond in the sub-system.

   ── Proof engineering note ────────────────────────────────────────────
   A naive [all: try (inversion Hstep2; subst; ...)] in the body of
   [ca_local_confluence] runs [inversion] in the full accumulated context
   of the proof (many hypotheses from the outer induction). With 7
   constructors in [ca_step], [subst] then generates proof terms that
   are quadratic in the number of hypotheses, causing an OOM kill after
   ~50 minutes.

   The fix: extract one small determinism lemma per COMM rule. Each
   lemma runs [inversion H; subst] in a TINY context (only a handful of
   universally-quantified variables plus the single hypothesis H), so
   proof terms remain small. The main proof then dispatches each COMM
   case with a trivial [exact (ca_step_ruleN_det ... Hstep2)].          *)

(** Helper: no [ca_step] whose source is a sub-system of a COMM rule's
    LHS can also be lifted to the same top-level system as a PAR rule.
    This tactic discharges the impossible COMM-vs-PAR overlaps by
    inverting into the sub-system and finding an SSigned or SToken
    that cannot step, or a structurally-impossible signature equation
    (a proper sub-signature equal to one of its [SAnd] parents). *)
Ltac solve_no_substep :=
  match goal with
  | [ H : ca_step (SSigned _ _) _ |- _ ] =>
      exfalso; eapply SSigned_no_step; exact H
  | [ H : ca_step (SToken _) _ |- _ ] =>
      exfalso; eapply SToken_no_step; exact H
  | [ H : SAnd ?a ?b = ?a |- _ ] =>
      exfalso; eapply SAnd_acyclic_left; exact H
  | [ H : ?a = SAnd ?a ?b |- _ ] =>
      exfalso; eapply SAnd_acyclic_right; exact H
  | [ H : ca_step (SPar (SSigned _ _) (SSigned _ _)) _ |- _ ] =>
      inversion H; subst; solve_no_substep
  | [ H : ca_step (SPar (SSigned _ _) (SToken _)) _ |- _ ] =>
      inversion H; subst; solve_no_substep
  | [ H : ca_step (SPar (SPar (SSigned _ _) (SSigned _ _)) (SToken _)) _ |- _ ] =>
      inversion H; subst; solve_no_substep
  end.

(* ── Per-rule determinism lemmas ─────────────────────────────────────────
   Each lemma states: given a [ca_step] from the exact LHS pattern of
   rule k, the result must be the RHS of rule k.

   Note: [ca_step_rule1_det] covers BOTH ca_rule1 AND ca_rule3, because
   both rules share the same LHS shape
     SPar (SSigned (PPar (PInput x P) (POutput x Q)) s) (SToken (TGate s t))
   (ca_rule3 is the special case s = SAnd s1 s2) and produce the same RHS
     SPar (SSigned (subst_proc P 0 (Quote Q)) s) (SToken t).            *)

Lemma ca_step_rule1_det : forall x P Q s t Sb,
  ca_step
    (SPar (SSigned (PPar (PInput x P) (POutput x Q)) s)
          (SToken (TGate s t)))
    Sb ->
  Sb = SPar (SSigned (subst_proc P 0 (Quote Q)) s) (SToken t).
Proof.
  intros x P Q s t Sb H.
  inversion H; subst.
  - (* ca_rule1 *) reflexivity.
  - (* ca_rule3: s = SAnd s1 s2, same RHS *) reflexivity.
  - (* ca_par_l: left sub-system SSigned cannot step *)
    exfalso; eapply SSigned_no_step; eassumption.
  - (* ca_par_r: right sub-system SToken cannot step *)
    exfalso; eapply SToken_no_step; eassumption.
Qed.

Lemma ca_step_rule2_det : forall x P Q s1 s2 t1 t2 Sb,
  ca_step
    (SPar (SPar (SSigned (PPar (PInput x P) (POutput x Q)) (SAnd s1 s2))
                (SToken (TGate s1 t1)))
          (SToken (TGate s2 t2)))
    Sb ->
  Sb = SPar (SPar (SSigned (subst_proc P 0 (Quote Q)) (SAnd s1 s2))
                  (SToken t1))
            (SToken t2).
Proof.
  intros x P Q s1 s2 t1 t2 Sb H.
  inversion H; subst.
  - (* ca_rule2 *) reflexivity.
  - (* ca_par_l: left = SPar (SSigned ...) (SToken ...).
       Four cases from inner inversion of the left sub-step:
         (a) ca_rule1 firing inside — yields SAnd s1 s2 = s1, impossible.
         (b) ca_rule3 firing inside — also yields SAnd s1 s2 = s1.
         (c) ca_par_l in the inner SPar — inner SSigned cannot step.
         (d) ca_par_r in the inner SPar — inner SToken cannot step. *)
    inversion H3; subst; solve_no_substep.
  - (* ca_par_r: right = SToken (TGate s2 t2) cannot step *)
    solve_no_substep.
Qed.

Lemma ca_step_rule4_det : forall x P Q s1 s2 t Sb,
  ca_step
    (SPar (SPar (SSigned (PInput x P) s1)
                (SSigned (POutput x Q) s2))
          (SToken (TGate (SAnd s1 s2) t)))
    Sb ->
  Sb = SPar (SSigned (subst_proc P 0 (Quote Q)) (SAnd s1 s2)) (SToken t).
Proof.
  intros x P Q s1 s2 t Sb H.
  inversion H; subst.
  - (* ca_rule4 *) reflexivity.
  - (* ca_par_l: left = SPar (SSigned (PInput ...) s1) (SSigned (POutput ...) s2).
       Inner inversion yields only ca_par_l / ca_par_r cases — each has an
       SSigned sub-step that [solve_no_substep] discharges. *)
    inversion H3; subst; solve_no_substep.
  - (* ca_par_r: right = SToken cannot step *)
    solve_no_substep.
Qed.

Lemma ca_step_rule5_det : forall x P Q s1 s2 t1 t2 Sb,
  ca_step
    (SPar (SPar (SPar (SSigned (PInput x P) s1)
                      (SSigned (POutput x Q) s2))
                (SToken (TGate s1 t1)))
          (SToken (TGate s2 t2)))
    Sb ->
  Sb = SPar (SPar (SSigned (subst_proc P 0 (Quote Q)) (SAnd s1 s2))
                  (SToken t1))
            (SToken t2).
Proof.
  intros x P Q s1 s2 t1 t2 Sb H.
  inversion H; subst.
  - (* ca_rule5 *) reflexivity.
  - (* ca_par_l: left = SPar (SPar (SSigned ...) (SSigned ...)) (SToken ...).
       Inner inversion yields three cases:
         (a) ca_rule4 firing inside — yields SAnd s1 s2 = s1, impossible.
         (b) ca_par_l on the inner-inner SPar (SSigned ...) (SSigned ...) —
             further inversion lands on an SSigned sub-step.
         (c) ca_par_r on the inner SToken — SToken cannot step.
       [solve_no_substep] is recursive and peels these layers uniformly. *)
    inversion H3; subst; solve_no_substep.
  - (* ca_par_r: right = SToken cannot step *)
    solve_no_substep.
Qed.

Lemma ca_local_confluence : forall S Sa Sb,
  ca_step S Sa ->
  ca_step S Sb ->
  Sa = Sb \/
  exists S', ca_step Sa S' /\ ca_step Sb S'.
Proof.
  intros S Sa Sb Hstep1.
  revert Sb.
  induction Hstep1; intros Sb Hstep2.

  - (* ca_rule1: SPar (SSigned (PPar (PInput x P) (POutput x Q)) s) (SToken (TGate s t)) *)
    left. symmetry. exact (ca_step_rule1_det _ _ _ _ _ _ Hstep2).

  - (* ca_rule2: SPar (SPar (SSigned (PPar ...) (SAnd s1 s2)) (SToken (TGate s1 t1))) (SToken (TGate s2 t2)) *)
    left. symmetry. exact (ca_step_rule2_det _ _ _ _ _ _ _ _ Hstep2).

  - (* ca_rule3: same LHS shape as ca_rule1 with s = SAnd s1 s2 *)
    left. symmetry. exact (ca_step_rule1_det _ _ _ _ _ _ Hstep2).

  - (* ca_rule4: SPar (SPar (SSigned (PInput x P) s1) (SSigned (POutput x Q) s2)) (SToken (TGate (SAnd s1 s2) t)) *)
    left. symmetry. exact (ca_step_rule4_det _ _ _ _ _ _ _ Hstep2).

  - (* ca_rule5: SPar (SPar (SPar (SSigned (PInput x P) s1) (SSigned (POutput x Q) s2)) (SToken (TGate s1 t1))) (SToken (TGate s2 t2)) *)
    left. symmetry. exact (ca_step_rule5_det _ _ _ _ _ _ _ _ Hstep2).

  - (* Hstep1 = ca_par_l S1 S1' S2 : ca_step (SPar S1 S2) (SPar S1' S2).
       Hstep2 : ca_step (SPar S1 S2) Sb. *)
    inversion Hstep2; subst.
    all: try solve_no_substep.
    + (* ca_par_l: both steps in left branch → use IH *)
      destruct (IHHstep1 _ H2) as [Heq | [S' [Hs1 Hs2]]].
      * left. subst. reflexivity.
      * right. exists (SPar S' S2).
        split; apply ca_par_l; assumption.
    + (* ca_par_r: steps in different branches → commute *)
      right. exists (SPar S1' S2').
      split.
      * apply ca_par_r. exact H2.
      * apply ca_par_l. exact Hstep1.

  - (* Hstep1 = ca_par_r S1 S2 S2' : ca_step (SPar S1 S2) (SPar S1 S2').
       Hstep2 : ca_step (SPar S1 S2) Sb. *)
    inversion Hstep2; subst.
    all: try solve_no_substep.
    + (* ca_par_l: steps in different branches → commute *)
      right. exists (SPar S1' S2').
      split.
      * apply ca_par_l. exact H2.
      * apply ca_par_r. exact Hstep1.
    + (* ca_par_r: both steps in right branch → use IH *)
      destruct (IHHstep1 _ H2) as [Heq | [S' [Hs1 Hs2]]].
      * left. subst. reflexivity.
      * right. exists (SPar S1 S').
        split; apply ca_par_r; assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Newman's Lemma (Constructive)
   ═══════════════════════════════════════════════════════════════════════════

   Newman's lemma: if a relation is locally confluent and well-founded
   (strongly normalising), then it is fully confluent. This follows the
   constructive proof of Coquand (1994), which uses double well-founded
   induction — outer induction on the starting term, inner induction on
   one of the reduction sequences.

   We instantiate it for [ca_step].                                       *)

Definition confluent (S : system) : Prop :=
  forall T1 T2,
    ca_reachable S T1 ->
    ca_reachable S T2 ->
    exists T, ca_reachable T1 T /\ ca_reachable T2 T.

(** Auxiliary: reachability is compatible with single steps and
    transitivity. *)
Lemma ca_reachable_step_trans : forall S1 S2 S3,
  ca_step S1 S2 -> ca_reachable S2 S3 -> ca_reachable S1 S3.
Proof.
  intros. eapply car_step; eassumption.
Qed.

(** Newman's lemma for [ca_step].

    By well-founded induction on [S] (via [Acc ca_step_inv]):
    - IH: every one-step successor of [S] is confluent.
    - Goal: [S] is confluent.

    Given [ca_reachable S T1] and [ca_reachable S T2], we proceed by
    induction on the first sequence, then the second. When both take
    at least one step, local confluence provides a join point, and
    the IH on the successors and the join point closes the diagram.

    Reference: T. Coquand, "A proof of Newman's lemma using
    well-founded induction," Chalmers University, 1994.                *)
Theorem newman : forall S, Acc ca_step_inv S -> confluent S.
Proof.
  intros S Hacc.
  induction Hacc as [S _ IH_wf].
  unfold confluent.
  intros T1 T2 Hreach1 Hreach2.
  revert T2 Hreach2.
  induction Hreach1 as [| S S1 T1 Hstep1 Htail1 IH_path1].
  - (* S ⤳* T1 by car_refl: T1 = S. Take T = T2. *)
    intros T2 Hreach2.
    exists T2. split.
    + exact Hreach2.
    + apply car_refl.
  - (* S ⤳ S1 ⤳* T1. *)
    intros T2 Hreach2.
    induction Hreach2 as [| S S2 T2 Hstep2 Htail2 IH_path2].
    + (* S ⤳* T2 by car_refl: T2 = S. Take T = T1. *)
      exists T1. split.
      * apply car_refl.
      * eapply car_step; eassumption.
    + (* S ⤳ S2 ⤳* T2. Both paths take at least one step. *)
      (* Local confluence gives a join point for S ⤳ S1 and S ⤳ S2. *)
      destruct (ca_local_confluence S S1 S2 Hstep1 Hstep2)
        as [Heq | [S' [Hs1s' Hs2s']]].
      * (* S1 = S2: both tails start from the same state *)
        subst S2.
        (* By IH_wf on S1 (one-step successor of S): S1 is confluent. *)
        assert (Hconf_S1 : confluent S1).
        { apply IH_wf. unfold ca_step_inv. exact Hstep1. }
        exact (Hconf_S1 T1 T2 Htail1 Htail2).
      * (* S1 ⤳ S' and S2 ⤳ S'. *)
        (* S1 is confluent by IH_wf. *)
        assert (Hconf_S1 : confluent S1).
        { apply IH_wf. unfold ca_step_inv. exact Hstep1. }
        (* S2 is confluent by IH_wf. *)
        assert (Hconf_S2 : confluent S2).
        { apply IH_wf. unfold ca_step_inv. exact Hstep2. }

        (* Join T1 and S' via confluent S1:
           S1 ⤳* T1 (Htail1) and S1 ⤳ S' (one step, hence reachable) *)
        assert (Hreach_S1_S' : ca_reachable S1 S').
        { apply ca_reachable_one. exact Hs1s'. }
        destruct (Hconf_S1 T1 S' Htail1 Hreach_S1_S')
          as [D1 [HrT1D1 HrS'D1]].

        (* Join T2 and S' via confluent S2:
           S2 ⤳* T2 (Htail2) and S2 ⤳ S' (one step, hence reachable) *)
        assert (Hreach_S2_S' : ca_reachable S2 S').
        { apply ca_reachable_one. exact Hs2s'. }
        destruct (Hconf_S2 T2 S' Htail2 Hreach_S2_S')
          as [D2 [HrT2D2 HrS'D2]].

        (* Now join D1 and D2 via confluent S1:
           S1 ⤳* D1 (via S1 ⤳* T1 ⤳* D1) and S1 ⤳* D2 (via S1 ⤳ S' ⤳* D2) *)
        assert (Hreach_S1_D1 : ca_reachable S1 D1).
        { eapply ca_reachable_trans; eassumption. }
        assert (Hreach_S1_D2 : ca_reachable S1 D2).
        { eapply ca_reachable_trans. exact Hreach_S1_S'. exact HrS'D2. }
        destruct (Hconf_S1 D1 D2 Hreach_S1_D1 Hreach_S1_D2)
          as [D [HrD1D HrD2D]].

        (* Final: T1 ⤳* D (via T1 ⤳* D1 ⤳* D) and T2 ⤳* D (via T2 ⤳* D2 ⤳* D) *)
        exists D. split.
        -- eapply ca_reachable_trans; eassumption.
        -- eapply ca_reachable_trans; eassumption.
Qed.

(** Full confluence of [ca_step]: every system is confluent. *)
Theorem ca_confluent : forall S, confluent S.
Proof.
  intro S.
  apply newman.
  apply ca_strongly_normalizing.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Normal Form Uniqueness and Cost Determinism
   ═══════════════════════════════════════════════════════════════════════════

   With full confluence established, terminal states (normal forms) are
   unique: if [S ⤳* T1] and [S ⤳* T2] with both [T1] and [T2] terminal,
   then [T1 = T2]. This immediately gives cost determinism since equal
   systems have equal token counts.                                        *)

(** Terminal states reachable from the same system are equal. *)
Theorem ca_normal_form_unique : forall S T1 T2,
  ca_reachable S T1 -> ca_terminal T1 ->
  ca_reachable S T2 -> ca_terminal T2 ->
  T1 = T2.
Proof.
  intros S T1 T2 Hreach1 Hterm1 Hreach2 Hterm2.
  destruct (ca_confluent S T1 T2 Hreach1 Hreach2) as [T [HrT1 HrT2]].
  (* T1 ⤳* T and T1 is terminal → T = T1 *)
  inversion HrT1 as [T1' | T1' Smid T' Hstep_mid Htail_mid]; subst.
  - (* car_refl: T = T1 *)
    (* T2 ⤳* T2 (since T = T1 = T2 must be shown) *)
    inversion HrT2 as [T2' | T2' Smid2 T2'' Hstep_mid2 Htail_mid2]; subst.
    + reflexivity.
    + exfalso. exact (Hterm2 Smid2 Hstep_mid2).
  - (* car_step: T1 ⤳ Smid, contradicts T1 terminal *)
    exfalso. exact (Hterm1 Smid Hstep_mid).
Qed.

(** Headline theorem: the token count at any terminal state reachable
    from [S] is uniquely determined by [S]. Two validators evaluating
    the same deploy in different orders reach terminal states with
    identical token counts, hence identical total costs.

    This is the property that makes [FuturesUnordered] safe for
    consensus. *)
Theorem ca_cost_deterministic : forall S T1 T2,
  ca_reachable S T1 -> ca_terminal T1 ->
  ca_reachable S T2 -> ca_terminal T2 ->
  system_token_count T1 = system_token_count T2.
Proof.
  intros S T1 T2 Hreach1 Hterm1 Hreach2 Hterm2.
  replace T2 with T1.
  - reflexivity.
  - exact (ca_normal_form_unique S T1 T2 Hreach1 Hterm1 Hreach2 Hterm2).
Qed.
