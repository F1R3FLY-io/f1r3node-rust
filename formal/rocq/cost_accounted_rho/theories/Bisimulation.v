(* ═══════════════════════════════════════════════════════════════════════════
   Bisimulation.v — Bisimilarity between undecorated and cost-accounted
                    Rholang programs
   ═══════════════════════════════════════════════════════════════════════════

   This module establishes the bisimulation framework for proving that an
   undecorated Rholang program (a process P in pure rho calculus) is
   behaviorally equivalent to its cost-accounting translation when wrapped
   under the unit signature.

   Approach: We use a coinductive definition of strong bisimilarity based
   on the standard rho-calculus reduction relation. This is the simpler
   sibling of the companion-based approach from rocq-coinduction; we use
   this direct definition to keep the dependencies minimal and the proof
   structure concrete.

   The key result aims to show that:

       S_tr (SSigned P SUnit)  ~  P  ⊕  (intermediate fuel-gate state)

   That is, the translation of a unit-signed process can take exactly one
   "fuel-gate" step that, after the gate fires, leaves a state that's
   structurally equivalent to the original process P. This captures the
   intuition that "wrapping P under the unit signature is operationally
   transparent — the fuel gate fires immediately and the process P runs
   unchanged."

   We do not use rocq-coinduction directly in this file; the built-in
   [CoInductive] definition is enough for the theorem statements below
   and keeps the dependency surface minimal.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition           │ Paper Property
   ──────────────────────────┼──────────────────────────────────────────
   simulates                 │ Forward simulation: every step matched
   bisimilar (CoInductive)   │ Strong bisimilarity
   unit_translation_one_step_to_body │ S_tr (P^()) takes one fuel-gate step
   unit_post_gate_canonical  │ After fuel gate fires, body is exposed
   ─────────────────────────────────────────────────────────────────────────

   Dependencies: RhoSyntax, RhoReduction, CostAccountedSyntax, Translation
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.
From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import StructEquivInversion.
From CostAccountedRho Require Import StructEquivHeads.
From CostAccountedRho Require Import RhoReduction.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CostAccountedReduction.
From CostAccountedRho Require Import Translation.
From CostAccountedRho Require Import TranslationFaithfulness.

Section BisimulationProofs.

Variable hash_process : list bool -> proc.
Hypothesis hash_process_injective :
  forall b1 b2, hash_process b1 = hash_process b2 -> b1 = b2.
Hypothesis hash_process_closed : forall bs, closed_proc (hash_process bs).
(* Cryptographic atomicity: a hash process has exactly one head.
   Not currently used by any theorem in this section, but included
   for surface uniformity with Section FaithfulnessProofs in
   TranslationFaithfulness.v and for future-proofing. *)
Hypothesis hash_process_head_count_one :
  forall bs, head_count (hash_process bs) = 1.

(* The ground-axis canonical process (Def 3.3 axis [g]) and its
   structural/cryptographic hypotheses, mirroring the [hash_process_*]
   block so the translation functions are uniformly parameterised by both
   reflection axes. *)
Variable ground_process : list bool -> proc.
Hypothesis ground_process_injective :
  forall b1 b2, ground_process b1 = ground_process b2 -> b1 = b2.
Hypothesis ground_process_closed : forall bs, closed_proc (ground_process bs).
Hypothesis ground_process_head_count_one :
  forall bs, head_count (ground_process bs) = 1.
Hypothesis ground_hash_disjoint :
  forall b1 b2, ground_process b1 <> hash_process b2.

(* Local notations for the section's translation functions. *)
Notation N := (N_tr hash_process ground_process).
Notation T := (T_tr hash_process ground_process).
Notation Pf := (P_tr hash_process ground_process).
Notation Sy := (S_tr hash_process ground_process).

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Simulation Relations
   ═══════════════════════════════════════════════════════════════════════════

   A relation R between processes is a (forward) simulation if, for every
   pair (P, Q) in R and every step P ⇝ P', there exists Q' such that
   Q ⇝* Q' and (P', Q') ∈ R. This is the inductive notion of "Q can
   match every move P makes." A bisimulation is a simulation whose
   converse is also a simulation.

   Strong bisimilarity is the largest bisimulation, defined coinductively.
   For the cost-accounted setting, we want a weaker relation: weak
   bisimilarity, which absorbs internal "tau" steps (the fuel-gate firings
   that don't correspond to any cost-accounted step). For now, we use
   strong bisimilarity and prove the unit-signature case explicitly.    *)

Definition simulation (R : proc -> proc -> Prop) : Prop :=
  forall P Q, R P Q ->
  forall P', rho_step P P' -> exists Q', rho_reachable Q Q' /\ R P' Q'.

Definition bisimulation (R : proc -> proc -> Prop) : Prop :=
  simulation R /\ simulation (fun P Q => R Q P).

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: The Unit-Signature Translation Step
   ═══════════════════════════════════════════════════════════════════════════

   When a process P is wrapped under the unit signature (), its
   translation takes exactly one fuel-gate step before exposing P. We
   prove this concretely.

   Recall:  Sy (SSigned P SUnit) = Pf P SUnit
                                = PInput (N SUnit) (PPar P (PDeref (NVar 0)))
                                = PInput (Quote PNil) (PPar P (PDeref (NVar 0)))

   And:    T TUnit = PNil

   To "fire" this fuel gate, we need a parallel POutput on the channel
   N SUnit = Quote PNil. The token T TUnit = PNil does NOT provide such
   an output (the unit token is the empty process). So the fuel gate
   does not fire on its own — it must be paired with a non-empty token.

   For a NON-trivial unit token like TGate SUnit TUnit (one unit of
   fuel), the translation gives:
       T (TGate SUnit TUnit) = POutput (N SUnit) (T TUnit)
                            = POutput (Quote PNil) PNil

   And the full translation of  SPar (SSigned P SUnit) (SToken (TGate SUnit TUnit))
   is:
       PPar (PInput (Quote PNil) (PPar P (PDeref (NVar 0))))
            (POutput (Quote PNil) PNil)

   This is a redex! By rs_comm, it reduces in one step to:
       subst_proc (PPar P (PDeref (NVar 0))) 0 (Quote PNil)

   Substitution distributes through PPar and replaces NVar 0 with the
   quoted name [Quote PNil], yielding:
       PPar (subst_proc P 0 (Quote PNil)) (PNil)

   The right component is a stuck dereference of a quoted PNil — it
   does no observable work and is bisimilar to PNil.                     *)

(* The unit-signature fuel gate fires when paired with a unit token. *)
Theorem unit_fuel_gate_fires : forall P,
  rho_step
    (PPar (Pf P SUnit) (T (TGate SUnit TUnit)))
    (subst_proc (PPar (lift_proc 1 0 P) (PDeref (NVar 0))) 0 (Quote (T TUnit))).
Proof.
  intros P.
  unfold Pf, T.
  simpl.
  apply rs_comm.
Qed.

(* The same statement at the system level: the translation of
   (P^()) | () takes one COMM step. *)
Theorem unit_system_fuel_gate_fires : forall P,
  rho_step
    (Sy (SPar (SSigned P SUnit) (SToken (TGate SUnit TUnit))))
    (subst_proc (PPar (lift_proc 1 0 P) (PDeref (NVar 0))) 0 (Quote (T TUnit))).
Proof.
  intros P.
  unfold Sy. simpl.
  unfold Pf, T.
  simpl.
  apply rs_comm.
Qed.

(* The ground-signature analogue: the SGround unit-token gate fires in one
   COMM step on the channel (Quote (ground_process bs)). The post-state
   shape is identical to the SUnit case because the unit token's payload
   (T TUnit = PNil) does not depend on the signature. *)
(* Under semantic subst, the [PDeref (NVar 0)] in the gate body
   collapses to the token payload [T TUnit = PNil] directly; the old
   [PNil] residue no longer arises. *)
Theorem ground_system_fuel_gate_fires : forall bs P,
  rho_step
    (Sy (SPar (SSigned P (SGround bs)) (SToken (TGate (SGround bs) TUnit))))
    (PPar P PNil).
Proof.
  intros bs P.
  unfold Sy. simpl.
  unfold Pf, T.
  simpl.
  eapply rs_struct.
  - apply se_refl.
  - apply rs_comm.
  - rewrite subst_proc_par.
    rewrite subst_lift_zero.
    rewrite subst_proc_deref_nvar_eq_quote.
    apply se_refl.
Qed.

(* The cryptographic-quote-signature analogue: the SQuote unit-token gate
   fires in one COMM step on the channel (Quote (hash_process bs)). *)
Theorem quote_system_fuel_gate_fires : forall bs P,
  rho_step
    (Sy (SPar (SSigned P (SQuote bs)) (SToken (TGate (SQuote bs) TUnit))))
    (PPar P PNil).
Proof.
  intros bs P.
  unfold Sy. simpl.
  unfold Pf, T.
  simpl.
  eapply rs_struct.
  - apply se_refl.
  - apply rs_comm.
  - rewrite subst_proc_par.
    rewrite subst_lift_zero.
    rewrite subst_proc_deref_nvar_eq_quote.
    apply se_refl.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Reachability via the Fuel Gate
   ═══════════════════════════════════════════════════════════════════════════

   Packaging the single-step result as reachability for use in larger
   bisimulation arguments.                                                *)

Theorem unit_system_reaches_body : forall P,
  rho_reachable
    (Sy (SPar (SSigned P SUnit) (SToken (TGate SUnit TUnit))))
    (subst_proc (PPar (lift_proc 1 0 P) (PDeref (NVar 0))) 0 (Quote (T TUnit))).
Proof.
  intros P.
  apply rho_reachable_one.
  apply unit_system_fuel_gate_fires.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Bisimulation Candidate for the Unit-Signature Case
   ═══════════════════════════════════════════════════════════════════════════

   We define a candidate relation R such that R P Q holds iff Q is the
   translation of (P^()) composed with a unit token. The bisimulation
   property would say: every step P ⇝ P' is matched by some step
   Q ⇝* Q' such that R P' Q' holds.

   The technical challenge is the post-gate residue.  After semantic
   substitution, the residue is [PNil], so the post-gate state has the
   canonical form [PPar P PNil].  Section 10 proves the corresponding
   strong bisimulation directly by cofixpoint.                            *)

(* The candidate bisimulation relation: P is related to Q iff Q is the
   "post-fuel-gate" form of the unit-signed translation of P. *)
Definition unit_post_gate (P : proc) (Q : proc) : Prop :=
  Q = subst_proc (PPar (lift_proc 1 0 P) (PDeref (NVar 0))) 0 (Quote (T TUnit)).

(* Sanity check: the relation is well-defined and inhabited. *)
Lemma unit_post_gate_canonical : forall P,
  unit_post_gate P
    (subst_proc (PPar (lift_proc 1 0 P) (PDeref (NVar 0))) 0 (Quote (T TUnit))).
Proof.
  intros P. unfold unit_post_gate. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 5: The Bisimulation Theorem (Statement)
   ═══════════════════════════════════════════════════════════════════════════

   The full bisimilarity theorem says:

       For every process P, the translation of (P under the unit
       signature, paired with one unit of unit fuel) is bisimilar to P.

   The proof is discharged in two steps:
   1. The fuel-gate fires (proven above as [unit_system_fuel_gate_fires]).
   2. The post-gate state is strongly bisimilar to P
      ([post_gate_bisim], proven in Section 10).

   The theorem below records the exact one-step post-gate form used by
   the later strong-bisimilarity theorem.                                *)

Theorem unit_translation_one_step_to_body : forall P,
  exists Q,
    rho_reachable
      (Sy (SPar (SSigned P SUnit) (SToken (TGate SUnit TUnit))))
      Q
    /\ unit_post_gate P Q.
Proof.
  intros P.
  exists (subst_proc (PPar (lift_proc 1 0 P) (PDeref (NVar 0))) 0 (Quote (T TUnit))).
  split.
  - apply unit_system_reaches_body.
  - apply unit_post_gate_canonical.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 6: Forward Simulation — Translated Form Mimics the Original
   ═══════════════════════════════════════════════════════════════════════════

   Strong bisimulation between the post-fuel-gate state and the
   original process P would require both:
   (a) every step of P matched by a step of the translated form, AND
   (b) every step of the translated form matched by a step of P.

   Direction (a) — the FORWARD simulation — is proved first. Direction
   (b) — the BACKWARD simulation — is closed later by
   [backward_sim_par_stuck] and [post_gate_bisim_strong], using the
   stuck-process lemmas from [RhoReduction.v].

   The forward direction alone is enough to say "the translation
   subsumes the original program's behavior" — every observable
   behavior of P also occurs in the translated form. *)

(* The post-fuel-gate state, after substitution and lift cancellation,
   is structurally [PPar P (PNil)]. We prove the
   forward simulation on this concrete state. *)

(* Under semantic subst, the post-gate state simplifies directly to
   [PPar P (T TUnit)] — the [PDeref (NVar 0)] dequote collapses at the
   substitution site, so no residual [PDeref (Quote _)] survives. *)
Lemma post_gate_simplifies : forall P,
  subst_proc (PPar (lift_proc 1 0 P) (PDeref (NVar 0))) 0 (Quote (T TUnit))
  = PPar P (T TUnit).
Proof.
  intros P.
  rewrite subst_proc_par.
  rewrite subst_lift_zero.
  rewrite subst_proc_deref_nvar_eq_quote.
  reflexivity.
Qed.

(* The post-gate state in the unit case where T TUnit = PNil. *)
Lemma post_gate_unit : forall P,
  subst_proc (PPar (lift_proc 1 0 P) (PDeref (NVar 0))) 0 (Quote (T TUnit))
  = PPar P PNil.
Proof.
  intros P.
  rewrite post_gate_simplifies.
  unfold T. reflexivity.
Qed.

(* Forward simulation: every step of P is matched by a step of
   PPar P (PNil). The matching step is rs_par_l on the
   left component, leaving the inert PNil untouched. *)
Theorem post_gate_simulates_forward : forall P P',
  rho_step P P' ->
  rho_step (PPar P (PNil)) (PPar P' (PNil)).
Proof.
  intros P P' Hstep.
  apply rs_par_l.
  exact Hstep.
Qed.

(* Lifted to multi-step reachability: every reduction sequence from P
   is matched by a corresponding sequence from the translated form. *)
Theorem post_gate_simulates_forward_reachable : forall P P',
  rho_reachable P P' ->
  rho_reachable (PPar P (PNil)) (PPar P' (PNil)).
Proof.
  intros P P' Hreach.
  induction Hreach as [P0 | P0 P1 P2 Hstep _ IH].
  - apply rr_refl.
  - eapply rr_step.
    + apply post_gate_simulates_forward. exact Hstep.
    + exact IH.
Qed.

(* The headline forward-simulation theorem: starting from the full
   translation of (P^()) | () = unit-signed P with one unit fuel,
   for every reduction P -> P', the translation reaches a state that
   directly contains P' (composed with the inert dequoted residue). *)
Theorem unit_translation_simulates_forward : forall P P',
  rho_step P P' ->
  rho_reachable
    (Sy (SPar (SSigned P SUnit) (SToken (TGate SUnit TUnit))))
    (PPar P' (PNil)).
Proof.
  intros P P' Hstep.
  (* First, fire the fuel gate. *)
  eapply rr_step.
  - apply unit_system_fuel_gate_fires.
  - (* Now we are at the post-gate state, which simplifies to
       PPar P (PNil). Apply the forward simulation. *)
    rewrite post_gate_unit.
    eapply rr_step.
    + apply post_gate_simulates_forward. exact Hstep.
    + apply rr_refl.
Qed.

(* And the multi-step version. *)
Theorem unit_translation_simulates_forward_reachable : forall P P',
  rho_reachable P P' ->
  rho_reachable
    (Sy (SPar (SSigned P SUnit) (SToken (TGate SUnit TUnit))))
    (PPar P' (PNil)).
Proof.
  intros P P' Hreach.
  eapply rr_step.
  - apply unit_system_fuel_gate_fires.
  - rewrite post_gate_unit.
    apply post_gate_simulates_forward_reachable.
    exact Hreach.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 8: Strong Coinductive Bisimilarity (Forward Simulation Form)
   ═══════════════════════════════════════════════════════════════════════════

   Defines strong bisimilarity coinductively and proves the forward
   simulation half of the bisimulation theorem for the unit signature.
   The forward direction states: every reduction of the original
   undecorated process [P] is matched by a reduction of the post-fuel-gate
   state [PPar P (PNil)]. This is the operational
   completeness of the translation: anything P can do, the translation
   can also do.

   The full BIDIRECTIONAL strong bisimilarity additionally requires the
   BACKWARD direction (every reduction of the post-gate state corresponds
   to a reduction of P). The backward direction is captured separately
   below via [post_gate_barb_equivalent], which proves observational
   equivalence at the level of barbs (which is the semantically
   meaningful content of bisimilarity for a stuck residue).

   The combination of forward simulation + barb equivalence is the
   formally proven operational equivalence of the post-gate state and P.
   It captures everything that strong bisimilarity captures EXCEPT for
   the "every step is matched by a step" symmetric clause; instead, we
   have "every step is matched by a step (forward)" plus "the same
   observable barbs at every state (backward, observationally)".         *)

(* Strong bisimilarity coinductive definition. No external dependencies
   on rocq-coinduction; uses Coq's built-in [CoInductive]. *)
CoInductive bisim : proc -> proc -> Prop :=
  | bisim_intro : forall P Q,
      (forall P', rho_step P P' ->
                  exists Q', rho_step Q Q' /\ bisim P' Q') ->
      (forall Q', rho_step Q Q' ->
                  exists P', rho_step P P' /\ bisim P' Q') ->
      bisim P Q.

Notation "P ~~ Q" := (bisim P Q) (at level 70).

(* Reflexivity of strong bisimilarity. *)
Lemma bisim_refl : forall P, bisim P P.
Proof.
  cofix CH.
  intros P. apply bisim_intro.
  - intros P' Hstep. exists P'. split; [exact Hstep | apply CH].
  - intros P' Hstep. exists P'. split; [exact Hstep | apply CH].
Qed.

(* Symmetry of strong bisimilarity. *)
Lemma bisim_sym : forall P Q, bisim P Q -> bisim Q P.
Proof.
  cofix CH.
  intros P Q HPQ.
  inversion HPQ as [P0 Q0 Hf Hb HeqP HeqQ]; subst.
  apply bisim_intro.
  - intros Q' Hstep.
    destruct (Hb Q' Hstep) as [P' [HstepP HrelP]].
    exists P'. split; [exact HstepP | apply CH; exact HrelP].
  - intros P' Hstep.
    destruct (Hf P' Hstep) as [Q' [HstepQ HrelQ]].
    exists Q'. split; [exact HstepQ | apply CH; exact HrelQ].
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 9: Barb Equivalence of the Post-Gate State
   ═══════════════════════════════════════════════════════════════════════════

   The post-fuel-gate state [PPar P (PNil)] has the SAME
   set of observable barbs as the original process [P]. The proof is by
   direct inversion on the [barb] constructors, exploiting the fact that
   [PDeref _] has no barbs of its own (no constructor of [barb] applies
   to a top-level [PDeref]).

   This is the formally proven "operational equivalence" of the post-gate
   state with P at the level of immediate observations.                  *)

(* PDeref of any name has no barbs. *)
Lemma deref_no_barb : forall n x, ~ barb (PDeref n) x.
Proof.
  intros n x H. inversion H.
Qed.

(* Equivalent: PNil has no barbs. *)
Lemma pnil_no_barb : forall x, ~ barb PNil x.
Proof. intros x H. inversion H. Qed.

(* The barb equivalence: the post-gate state and the original process
   barb on exactly the same channels. *)
Theorem post_gate_barb_equivalent : forall P x,
  barb (PPar P (PNil)) x <-> barb P x.
Proof.
  intros P x. split.
  - (* Forward: barb of the post-gate state implies barb of P. *)
    intros H. inversion H; subst.
    + (* barb_par_l: barb P x — direct *)
      assumption.
    + (* barb_par_r: barb (PNil) x — impossible *)
      exfalso. eapply pnil_no_barb. eassumption.
  - (* Backward: barb P x implies barb of the post-gate state. *)
    intros H. apply barb_par_l. exact H.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 10: Reachability-Preserved Forward Simulation
   ═══════════════════════════════════════════════════════════════════════════

   A coinductive forward simulation: at every reachable state of [P],
   there is a corresponding reachable state of the post-gate translation
   that's structurally equivalent (modulo the inert PDeref residue). This
   is the strongest "forward" direction of bisimilarity that's provable
   without backward inversion machinery.                                  *)

(* Forward simulation as a co-inductively defined "P is simulated by Q":
   every step of P is matched by some reduction sequence of Q, and the
   resulting states remain in the simulation. *)
CoInductive forward_sim : proc -> proc -> Prop :=
  | fsim_intro : forall P Q,
      (forall P', rho_step P P' ->
                  exists Q', rho_reachable Q Q' /\ forward_sim P' Q') ->
      forward_sim P Q.

(* Forward simulation is reflexive. *)
Lemma forward_sim_refl : forall P, forward_sim P P.
Proof.
  cofix CH.
  intros P. apply fsim_intro.
  intros P' Hstep.
  exists P'. split.
  - apply rho_reachable_one. exact Hstep.
  - apply CH.
Qed.

(* The post-gate state forward-simulates the original P: every reduction
   of P is matched by a one-step reduction of the post-gate state via
   [rs_par_l], and the resulting post-gate state contains the reduced P. *)
Theorem post_gate_forward_sim : forall P,
  forward_sim P (PPar P (PNil)).
Proof.
  cofix CH.
  intros P. apply fsim_intro.
  intros P' Hstep.
  exists (PPar P' (PNil)).
  split.
  - apply rho_reachable_one. apply rs_par_l. exact Hstep.
  - apply CH.
Qed.

(* The headline theorem: the translation of (P^()) | () forward-simulates
   P, with one initial fuel-gate firing followed by a forward simulation. *)
Theorem unit_translation_forward_simulates : forall P,
  exists W,
    rho_reachable
      (Sy (SPar (SSigned P SUnit) (SToken (TGate SUnit TUnit))))
      W
    /\ forward_sim P W.
Proof.
  intros P.
  exists (PPar P (PNil)).
  split.
  - eapply rr_step.
    + apply unit_system_fuel_gate_fires.
    + rewrite post_gate_unit. apply rr_refl.
  - apply post_gate_forward_sim.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 11: Combined Operational Equivalence
   ═══════════════════════════════════════════════════════════════════════════

   The headline operational equivalence theorem combines all three
   tractable correspondences:

   1. FORWARD simulation: every reduction of P is matched by a
      reduction of the post-gate state (via the forward_sim coinductive
      definition).
   2. BARB equivalence: the post-gate state and P barb on the same
      channels at every reachable state.
   3. ONE-STEP REACHABILITY: the translation reaches the post-gate state
      in exactly one fuel-gate firing.

   Together, these constitute the operational equivalence of the
   translation with the original undecorated process P.                  *)

Theorem unit_translation_operational_equivalence : forall P,
  exists W,
    (* Reachability: the translation reaches W in finitely many steps *)
    rho_reachable
      (Sy (SPar (SSigned P SUnit) (SToken (TGate SUnit TUnit))))
      W
    /\
    (* Forward simulation: every step of P is matched by W *)
    forward_sim P W
    /\
    (* Barb equivalence: W and P observe the same channels *)
    (forall x, barb W x <-> barb P x).
Proof.
  intros P.
  exists (PPar P (PNil)).
  split; [|split].
  - (* Reachability: one fuel-gate step. *)
    eapply rr_step.
    + apply unit_system_fuel_gate_fires.
    + rewrite post_gate_unit. apply rr_refl.
  - (* Forward simulation. *)
    apply post_gate_forward_sim.
  - (* Barb equivalence. *)
    intros x. apply post_gate_barb_equivalent.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 7: Discussion and Future Work
   ═══════════════════════════════════════════════════════════════════════════

   What we have proven:
   - The translation of a unit-signed process composed with a unit-token
     reduces in exactly one rho_step (the fuel gate firing).
   - The post-fuel-gate state is captured by the [unit_post_gate]
     relation, which makes the connection between the translated and
     undecorated processes explicit.

   The post-gate bisimulation is proven below by
   [post_gate_bisim_strong].                                               *)

(* ═══════════════════════════════════════════════════════════════════════════
   Section 12: Full Bidirectional Strong Bisimilarity (IDEAL form)
   ═══════════════════════════════════════════════════════════════════════════

   The IDEAL form of Closure C: full bidirectional strong bisimilarity
   between the post-fuel-gate state [PPar P (PNil)] and
   the original undecorated process [P]. The proof uses the heads-list
   inversion machinery from [StructEquivHeads.v] to handle the [rs_struct]
   case of the backward simulation.                                         *)

(* ─── Helper 1: list_equiv splits around a distinguished middle element ── *)

Lemma list_equiv_split_mid :
  forall xs zs1 d zs2,
    list_equiv xs (zs1 ++ d :: zs2) ->
    exists xs1 x xs2,
      xs = xs1 ++ x :: xs2 /\
      list_equiv xs1 zs1 /\
      x ≡ d /\
      list_equiv xs2 zs2.
Proof.
  intros xs zs1. revert xs.
  induction zs1 as [|z zs1' IH]; intros xs d zs2 Hle.
  - simpl in Hle.
    inversion Hle as [|a b xs0 ys0 Hab Hrest Hsplit Hyseq]; subst.
    exists (@nil proc), a, xs0. simpl.
    split; [reflexivity | split; [constructor | split; assumption]].
  - simpl in Hle.
    inversion Hle as [|a b xs0 ys0 Hab Hrest Hsplit Hyseq]; subst.
    destruct (IH xs0 d zs2 Hrest)
      as [xs1' [x [xs2' [Heq_xs0 [Hle1 [Hxd Hle2]]]]]].
    subst xs0.
    exists (a :: xs1'), x, xs2'. simpl.
    split; [reflexivity
           | split; [constructor; assumption
                    | split; assumption]].
Qed.

(* ─── Helper 2: concatenation equation locates a distinguished element ─── *)

Lemma app_eq_middle_proc :
  forall (l r m_l m_r : list proc) (m : proc),
    l ++ r = m_l ++ m :: m_r ->
    (exists l', l = m_l ++ m :: l' /\ m_r = l' ++ r) \/
    (exists r', m_l = l ++ r' /\ r = r' ++ m :: m_r).
Proof.
  intros l. induction l as [|h l' IH]; intros r m_l m_r m Heq.
  - right. exists m_l. simpl. split; [reflexivity | exact Heq].
  - simpl in Heq. destruct m_l as [|z m_l'].
    + simpl in Heq. injection Heq as Hhm Hrest.
      subst h. left. exists l'. simpl.
      split; [reflexivity | symmetry; exact Hrest].
    + simpl in Heq. injection Heq as Hhz Hrest.
      subst z.
      destruct (IH r m_l' m_r m Hrest)
        as [[xs2 [Hl Hr]] | [ys1 [Hml Hr]]].
      * left. exists xs2.
        split; [simpl; rewrite Hl; reflexivity | exact Hr].
      * right. exists ys1.
        split; [simpl; rewrite Hml; reflexivity | exact Hr].
Qed.

(* ─── Headline: backward simulation for the PNil-residue par form ─────────

   Under semantic substitution, the post-fuel-gate residue collapses to
   [PNil] (the structural identity), so this lemma reduces to a direct
   application of [se_par_nil]: [PPar P PNil ≡ P].                        *)

Lemma backward_sim_par_stuck :
  forall S T,
    rho_step S T ->
    forall P,
      S ≡ PPar P PNil ->
      exists P',
        rho_step P P' /\
        T ≡ PPar P' PNil.
Proof.
  intros S T Hstep P Heq.
  assert (HSeqP : S ≡ P).
  { eapply se_trans; [exact Heq | apply se_par_nil]. }
  exists T. split.
  - eapply rs_struct; [apply se_sym; exact HSeqP | exact Hstep | apply se_refl].
  - apply se_sym. apply se_par_nil.
Qed.

(* The bidirectional strong bisimilarity, parameterized over the
   structural equivalence to the canonical form. The cofix takes the
   equivalence as a parameter to handle Coq's guardedness check. *)
CoFixpoint post_gate_bisim_strong : forall P W,
  W ≡ PPar P (PNil) ->
  bisim W P :=
  fun P W HWeq =>
    bisim_intro W P
      (fun W' HstepW =>
         match backward_sim_par_stuck W W' HstepW P HWeq
         with ex_intro _ P' (conj HstepP HeqW') =>
           ex_intro _ P' (conj HstepP (post_gate_bisim_strong P' W' HeqW'))
         end)
      (fun P' HstepP =>
         ex_intro _ (PPar P' (PNil))
           (conj
              (rho_step_struct W (PPar P (PNil))
                 (PPar P' (PNil))
                 HWeq
                 (rs_par_l P P' (PNil) HstepP))
              (post_gate_bisim_strong P' (PPar P' (PNil))
                 (se_refl _)))).

(* The corollary: the post-gate state is bisimilar to P. *)
Theorem post_gate_bisim : forall P,
  bisim (PPar P (PNil)) P.
Proof.
  intros P. apply post_gate_bisim_strong. apply se_refl.
Qed.

(* The headline IDEAL theorem: the unit-translation reaches a state in
   one fuel-gate firing that is strongly bisimilar to the original P. *)
Theorem unit_translation_strong_bisimilar :
  forall P,
    exists W,
      rho_reachable
        (Sy (SPar (SSigned P SUnit) (SToken (TGate SUnit TUnit))))
        W
      /\ bisim W P.
Proof.
  intros P.
  exists (PPar P (PNil)).
  split.
  - eapply rr_step.
    + apply unit_system_fuel_gate_fires.
    + rewrite post_gate_unit. apply rr_refl.
  - apply post_gate_bisim.
Qed.

(* The ground-signature analogue of [unit_translation_strong_bisimilar].
   Like the unit case, the SGround unit-token gate fires in one COMM step
   to land at the post-gate state [PPar P (PNil)], which
   is strongly bisimilar to P via [post_gate_bisim]. *)
Theorem ground_translation_strong_bisimilar :
  forall bs P,
    exists W,
      rho_reachable
        (Sy (SPar (SSigned P (SGround bs)) (SToken (TGate (SGround bs) TUnit))))
        W
      /\ bisim W P.
Proof.
  intros bs P.
  exists (PPar P (PNil)).
  split.
  - apply rho_reachable_one. apply ground_system_fuel_gate_fires.
  - apply post_gate_bisim.
Qed.

(* The cryptographic-quote-signature analogue of
   [unit_translation_strong_bisimilar]. *)
Theorem quote_translation_strong_bisimilar :
  forall bs P,
    exists W,
      rho_reachable
        (Sy (SPar (SSigned P (SQuote bs)) (SToken (TGate (SQuote bs) TUnit))))
        W
      /\ bisim W P.
Proof.
  intros bs P.
  exists (PPar P (PNil)).
  split.
  - apply rho_reachable_one. apply quote_system_fuel_gate_fires.
  - apply post_gate_bisim.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 13: Multi-Stuck Residue Bisimilarity
   ═══════════════════════════════════════════════════════════════════════════

   Generalises [post_gate_bisim] to handle ANY stuck residue — a process
   whose top-level heads contain no PInput, no POutput, and no PReplicate.
   Such a residue contributes no barbs and cannot fire any reduction, so
   composing it in parallel with P is bisimilar to P alone.                *)

(* ─── Stuck residue predicate and counting function ─────────────────── *)

Definition is_stuck (R : proc) : Prop :=
  count_inputs R = 0 /\ count_outputs R = 0 /\ count_replicates R = 0.

Definition head_count_inputs_outputs (P : proc) : nat :=
  count_inputs P + count_outputs P + count_replicates P.

(* ─── A stuck process cannot take any reduction step ─────────────────── *)

Lemma stuck_is_irreducible :
  forall R T,
    count_inputs R = 0 ->
    count_outputs R = 0 ->
    count_replicates R = 0 ->
    ~ rho_step R T.
Proof.
  intros R T Hi Ho Hr Hstep.
  revert Hi Ho Hr.
  induction Hstep as
    [ x B C
    | A A' B Hstep1 IH1
    | A A' B Hstep2 IH2
    | S0 S1 T1 T0 Hpre Hstep0 IH Hpost
    | P0_rep
    ]; intros Hi Ho Hr; simpl in *.
  - discriminate Hi.
  - apply IH1; lia.
  - apply IH2; lia.
  - apply IH.
    + pose proof (count_inputs_se _ _ Hpre) as Hse. lia.
    + pose proof (count_outputs_se _ _ Hpre) as Hse. lia.
    + pose proof (count_replicates_se _ _ Hpre) as Hse. lia.
  - (* rs_replicate: count_replicates (PReplicate P0_rep) = 1 ≠ 0. *)
    lia.
Qed.

(* ─── Left-≡-transport for [bisim] (no cofix needed) ─────────────────── *)

Lemma bisim_struct_equiv_l :
  forall P P' Q,
    P ≡ P' ->
    bisim P' Q ->
    bisim P Q.
Proof.
  intros P P' Q Heq Hbis.
  inversion Hbis as [P0 Q0 Hf Hb HeqP0 HeqQ0]; subst.
  apply bisim_intro.
  - intros P'' HstepP.
    assert (HstepP' : rho_step P' P'').
    { eapply rs_struct; [apply se_sym; exact Heq | exact HstepP | apply se_refl]. }
    destruct (Hf P'' HstepP') as [Q' [HstepQ HbisPQ']].
    exists Q'. split; assumption.
  - intros Q' HstepQ.
    destruct (Hb Q' HstepQ) as [P'' [HstepP' HbisPQ']].
    exists P''. split.
    + eapply rs_struct; [exact Heq | exact HstepP' | apply se_refl].
    + assumption.
Qed.

(* ─── Transitivity of [bisim] (cofix, mirrors [bisim_sym] pattern) ───── *)

Lemma bisim_trans : forall P Q R, bisim P Q -> bisim Q R -> bisim P R.
Proof.
  cofix CH.
  intros P Q R HPQ HQR.
  inversion HPQ as [P0 Q0 Hf_PQ Hb_PQ HeqP0 HeqQ0]; subst.
  inversion HQR as [Q0' R0 Hf_QR Hb_QR HeqQ0' HeqR0]; subst.
  apply bisim_intro.
  - intros P' HstepP.
    destruct (Hf_PQ P' HstepP) as [Q' [HstepQ HbisPQ']].
    destruct (Hf_QR Q' HstepQ) as [R' [HstepR HbisQR']].
    exists R'. split; [exact HstepR | eapply CH; eassumption].
  - intros R' HstepR.
    destruct (Hb_QR R' HstepR) as [Q' [HstepQ HbisQR']].
    destruct (Hb_PQ Q' HstepQ) as [P' [HstepP HbisPQ']].
    exists P'. split; [exact HstepP | eapply CH; eassumption].
Qed.

(* ─── Generalised par-stuck case split for ANY [PDeref n] residue ────── *)

Lemma par_stuck_case_split_any_pderef :
  forall A B P n,
    PPar A B ≡ PPar P (PDeref n) ->
    (exists A_rest,
       A ≡ PPar A_rest (PDeref n) /\
       PPar A_rest B ≡ P) \/
    (exists B_rest,
       B ≡ PPar B_rest (PDeref n) /\
       PPar A B_rest ≡ P).
Proof.
  intros A B P n Heq.
  pose proof (struct_equiv_heads_perm _ _ Heq) as Hperm_eq.
  simpl in Hperm_eq.
  destruct Hperm_eq as [zs [Hle Hperm]].
  assert (HinQN : In (PDeref n) zs).
  { eapply Permutation_in; [apply Permutation_sym; exact Hperm |].
    apply in_or_app. right. left. reflexivity. }
  apply in_split in HinQN.
  destruct HinQN as [zs1 [zs2 Heq_zs]].
  subst zs.
  apply list_equiv_split_mid in Hle.
  destruct Hle as [xs1 [x [xs2 [Heq_xs [Hle1 [Hxd Hle2]]]]]].
  assert (Hin_x : In x (heads A ++ heads B)).
  { rewrite Heq_xs. apply in_or_app. right. left. reflexivity. }
  assert (Hhead_x : is_head x).
  { apply in_app_or in Hin_x. destruct Hin_x.
    - eapply heads_are_heads; eauto.
    - eapply heads_are_heads; eauto. }
  pose proof (se_PDeref_to_head _ _ (se_sym _ _ Hxd) Hhead_x) as Hpd_x.
  destruct Hpd_x as [m [Heq_x_pd Hm_name]].
  subst x.
  assert (HpermP : Permutation (zs1 ++ zs2) (heads P)).
  { rewrite <- (app_nil_r (heads P)).
    eapply Permutation_app_inv with (a := PDeref n).
    simpl. exact Hperm. }
  apply (app_eq_middle_proc (heads A) (heads B) xs1 xs2 (PDeref m))
    in Heq_xs.
  destruct Heq_xs as [[l' [HA_eq Hxs2_eq]] | [r' [Hxs1_eq HB_eq]]].
  - left. exists (heads_to_proc (xs1 ++ l')).
    split.
    + eapply se_trans. { apply se_sym, heads_to_proc_heads_se. }
      rewrite HA_eq.
      eapply se_trans. { apply heads_to_proc_app. }
      simpl.
      eapply se_trans. { apply se_par_cong_r. apply se_par_comm. }
      eapply se_trans. { apply se_sym, se_par_assoc. }
      eapply se_trans.
      { apply se_par_cong_l. apply se_sym, heads_to_proc_app. }
      apply se_par_cong_r. apply se_deref_cong.
      apply se_name_sym. exact Hm_name.
    + eapply se_trans.
      { apply se_par_cong_r. apply se_sym, heads_to_proc_heads_se. }
      eapply se_trans. { apply se_sym, heads_to_proc_app. }
      rewrite <- app_assoc.
      rewrite <- Hxs2_eq.
      eapply se_trans with (Q := heads_to_proc (zs1 ++ zs2)).
      { apply heads_to_proc_list_equiv.
        apply list_equiv_app; assumption. }
      eapply se_trans with (Q := heads_to_proc (heads P)).
      { apply heads_to_proc_Permutation. exact HpermP. }
      apply heads_to_proc_heads_se.
  - right. exists (heads_to_proc (r' ++ xs2)).
    split.
    + eapply se_trans. { apply se_sym, heads_to_proc_heads_se. }
      rewrite HB_eq.
      eapply se_trans. { apply heads_to_proc_app. }
      simpl.
      eapply se_trans. { apply se_par_cong_r. apply se_par_comm. }
      eapply se_trans. { apply se_sym, se_par_assoc. }
      eapply se_trans.
      { apply se_par_cong_l. apply se_sym, heads_to_proc_app. }
      apply se_par_cong_r. apply se_deref_cong.
      apply se_name_sym. exact Hm_name.
    + eapply se_trans.
      { apply se_par_cong_l. apply se_sym, heads_to_proc_heads_se. }
      eapply se_trans. { apply se_sym, heads_to_proc_app. }
      rewrite app_assoc.
      rewrite <- Hxs1_eq.
      eapply se_trans with (Q := heads_to_proc (zs1 ++ zs2)).
      { apply heads_to_proc_list_equiv.
        apply list_equiv_app; assumption. }
      eapply se_trans with (Q := heads_to_proc (heads P)).
      { apply heads_to_proc_Permutation. exact HpermP. }
      apply heads_to_proc_heads_se.
Qed.

(* ─── Generalised backward simulation for ANY [PDeref n] residue ────── *)

Lemma backward_sim_par_stuck_any_pderef :
  forall S T,
    rho_step S T ->
    forall P n,
      S ≡ PPar P (PDeref n) ->
      exists P',
        rho_step P P' /\
        T ≡ PPar P' (PDeref n).
Proof.
  intros S T Hstep.
  induction Hstep as
    [ x B0 C0
    | A1 A1' B1 Hstep1 IH1
    | A2 A2' B2 Hstep2 IH2
    | S0 S1 T1 T0 Hpre Hstep0 IH Hpost
    | P0_rep
    ]; intros P n Heq.
  - exfalso.
    apply count_derefs_se in Heq. simpl in Heq. lia.
  - destruct (par_stuck_case_split_any_pderef _ _ _ _ Heq) as
      [[A_rest [HAeq HPeq]] | [B_rest [HBeq HPeq]]].
    + destruct (IH1 _ _ HAeq) as [A_rest' [Hstep_rest HA1'_eq]].
      exists (PPar A_rest' B1).
      split.
      * eapply rho_step_struct.
        -- apply se_sym. exact HPeq.
        -- apply rs_par_l. exact Hstep_rest.
      * eapply se_trans.
        { apply se_par_cong_l. exact HA1'_eq. }
        eapply se_trans. { apply se_par_assoc. }
        eapply se_trans.
        { apply se_par_cong_r. apply se_par_comm. }
        apply se_sym. apply se_par_assoc.
    + exists (PPar A1' B_rest).
      split.
      * eapply rho_step_struct.
        -- apply se_sym. exact HPeq.
        -- apply rs_par_l. exact Hstep1.
      * eapply se_trans.
        { apply se_par_cong_r. exact HBeq. }
        apply se_sym. apply se_par_assoc.
  - destruct (par_stuck_case_split_any_pderef _ _ _ _ Heq) as
      [[B_rest [HBeq HPeq]] | [A_rest [HAeq HPeq]]].
    + exists (PPar B_rest A2').
      split.
      * eapply rho_step_struct.
        -- apply se_sym. exact HPeq.
        -- apply rs_par_r. exact Hstep2.
      * eapply se_trans.
        { apply se_par_cong_l. exact HBeq. }
        eapply se_trans. { apply se_par_assoc. }
        eapply se_trans.
        { apply se_par_cong_r. apply se_par_comm. }
        apply se_sym. apply se_par_assoc.
    + destruct (IH2 _ _ HAeq) as [A_rest' [Hstep_rest HA2'_eq]].
      exists (PPar B2 A_rest').
      split.
      * eapply rho_step_struct.
        -- apply se_sym. exact HPeq.
        -- apply rs_par_r. exact Hstep_rest.
      * eapply se_trans.
        { apply se_par_cong_r. exact HA2'_eq. }
        apply se_sym. apply se_par_assoc.
  - assert (HS1eq : S1 ≡ PPar P (PDeref n)).
    { eapply se_trans; [apply se_sym; exact Hpre | exact Heq]. }
    destruct (IH _ _ HS1eq) as [P' [HstepP HT1_eq]].
    exists P'. split; [exact HstepP |].
    eapply se_trans; [apply se_sym; exact Hpost | exact HT1_eq].
  - (* rs_replicate: source is PReplicate P0_rep, which has
       count_replicates = 1. But PPar P (PDeref n) has
       count_replicates = count_replicates P + 0 = count_replicates P.
       Also head_count (PReplicate P0_rep) = 1, while
       head_count (PPar P (PDeref n)) = head_count P + 1,
       so head_count P = 0. By fh_hc_zero_se_PNil, P ≡ PNil, giving
       count_replicates P = 0. But count_replicates_se gives 1 = 0.
       Contradiction. *)
    exfalso.
    pose proof (count_replicates_se _ _ Heq) as Hcr.
    simpl in Hcr.
    pose proof (head_count_se _ _ Heq) as Hhc.
    simpl in Hhc.
    assert (HhcP : head_count P = 0) by lia.
    apply fh_hc_zero_se_PNil in HhcP.
    apply count_replicates_se in HhcP. simpl in HhcP. lia.
Qed.

(* ─── Generalised bidirectional bisimilarity for ANY [PDeref n] ─────── *)

CoFixpoint post_gate_bisim_strong_any_pderef : forall P n W,
  W ≡ PPar P (PDeref n) ->
  bisim W P :=
  fun P n W HWeq =>
    bisim_intro W P
      (fun W' HstepW =>
         match backward_sim_par_stuck_any_pderef W W' HstepW P n HWeq
         with ex_intro _ P' (conj HstepP HeqW') =>
           ex_intro _ P' (conj HstepP
             (post_gate_bisim_strong_any_pderef P' n W' HeqW'))
         end)
      (fun P' HstepP =>
         ex_intro _ (PPar P' (PDeref n))
           (conj
              (rho_step_struct W (PPar P (PDeref n))
                 (PPar P' (PDeref n))
                 HWeq
                 (rs_par_l P P' (PDeref n) HstepP))
              (post_gate_bisim_strong_any_pderef P' n
                 (PPar P' (PDeref n))
                 (se_refl _)))).

(* ─── Corollary: bisim holds for any single [PDeref n] residue ──────── *)

Lemma bisim_par_pderef_any : forall P n, bisim (PPar P (PDeref n)) P.
Proof.
  intros P n.
  apply (post_gate_bisim_strong_any_pderef P n (PPar P (PDeref n))).
  apply se_refl.
Qed.

(* ─── Headline: multi-stuck-residue bisimulation ─────────────────────── *)

Lemma multi_stuck_residue_bisim :
  forall (residues P : proc),
    head_count_inputs_outputs residues = 0 ->
    bisim (PPar P residues) P.
Proof.
  intros residues P Hcount.
  unfold head_count_inputs_outputs in Hcount.
  assert (Hi : count_inputs residues = 0) by lia.
  assert (Ho : count_outputs residues = 0) by lia.
  assert (Hr : count_replicates residues = 0) by lia.
  clear Hcount.
  revert P Hi Ho Hr.
  induction residues as
    [ | x B IH_in
      | x B IH_out
      | R1 IH1 R2 IH2
      | n
      | R_body IH_rep ];
    intros P Hi Ho Hr; simpl in Hi, Ho, Hr.
  - apply bisim_struct_equiv_l with (P' := P).
    + apply se_par_nil.
    + apply bisim_refl.
  - discriminate Hi.
  - discriminate Ho.
  - assert (Hi1 : count_inputs R1 = 0) by lia.
    assert (Hi2 : count_inputs R2 = 0) by lia.
    assert (Ho1 : count_outputs R1 = 0) by lia.
    assert (Ho2 : count_outputs R2 = 0) by lia.
    assert (Hr1 : count_replicates R1 = 0) by lia.
    assert (Hr2 : count_replicates R2 = 0) by lia.
    specialize (IH1 P Hi1 Ho1 Hr1).
    specialize (IH2 (PPar P R1) Hi2 Ho2 Hr2).
    apply bisim_struct_equiv_l with (P' := PPar (PPar P R1) R2).
    + apply se_sym. apply se_par_assoc.
    + eapply bisim_trans; [exact IH2 | exact IH1].
  - apply bisim_par_pderef_any.
  - (* PReplicate: count_replicates (PReplicate R_body) = 1 ≠ 0. *)
    discriminate Hr.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 14: Compound (SAnd) Translation Bisimilarity
   ═══════════════════════════════════════════════════════════════════════════

   The SAnd unit-token gate is a nested 2-PInput structure that needs a
   Split mediator in parallel context to fire. The reduction trace is:

     Step 1: Split fires on the combined token, producing two atomic outputs.
     Step 2: Outer compound gate fires on the s1-output (PNil payload).
     Step 3: Inner compound gate fires on the s2-output ((PNil)
             payload).
     Final state: PPar P (PPar (PNil)
                                PNil).

   This final state has TWO stuck residues, and is bisimilar to P via
   [multi_stuck_residue_bisim].                                           *)

(* ─── Definitions for the compound LHS, intermediate, and final states. *)

Definition compound_canonical (s1 s2 : sig) (P : proc) : proc :=
  PPar (PPar (Pf P (SAnd s1 s2))
             (POutput (N (SAnd s1 s2)) PNil))
       (Split hash_process ground_process s1 s2).

Definition compound_post_split (s1 s2 : sig) (P : proc) : proc :=
  PPar (PPar (Pf P (SAnd s1 s2))
             (POutput (N s1) PNil))
       (POutput (N s2) (PNil)).

Definition compound_final (P : proc) : proc :=
  PPar P (PPar (PNil)
               PNil).

(* The post-Split state reaches the 4-step final via the compound half
   helper from TranslationFaithfulness.v. *)
Lemma cg_post_split_reach_final :
  forall s1 s2 P,
    rho_reachable (compound_post_split s1 s2 P) (compound_final P).
Proof.
  intros s1 s2 P.
  unfold compound_post_split, compound_final.
  rewrite (P_tr_and hash_process ground_process P s1 s2).
  apply (compound_half_fires_two_step hash_process hash_process_closed ground_process ground_process_closed
           P s1 s2 PNil (PNil)
           closed_PNil
           (closed_PDeref_Quote PNil closed_PNil)).
Qed.

(* The SAnd compound bisim headline. Requires a Split mediator in the
   parallel context to bridge the combined-token channel to the gate's
   atomic listening channels. *)
Theorem compound_translation_strong_bisimilar :
  forall (s1 s2 : sig) (P : proc),
    exists W,
      rho_reachable
        (PPar (Sy (SPar (SSigned P (SAnd s1 s2))
                        (SToken (TGate (SAnd s1 s2) TUnit))))
              (Split hash_process ground_process s1 s2))
        W
      /\ bisim W P.
Proof.
  intros s1 s2 P.
  exists (PPar P (PPar (PNil)
                       PNil)).
  split.
  - (* Reachability via 3 reduction steps: Split, outer gate, inner gate. *)
    unfold Sy. cbn [S_tr].
    rewrite (T_tr_gate hash_process ground_process (SAnd s1 s2) TUnit).
    rewrite (P_tr_and hash_process ground_process P s1 s2).
    cbn [T_tr].
    (* State: ((PInput-nested-gate | POutput (N(SAnd s1 s2)) PNil) | Split). *)
    (* Step 1: rearrange so Split and tok are adjacent, fire Split, then
       rearrange so the post-state is left-associative
       [(gate | s1-out) | s2-out] for the compound_half_fires_two_step
       helper. *)
    eapply rr_step.
    { eapply rs_struct.
      - (* ((gate | tok) | Split) ≡ (gate | (Split | tok)) *)
        eapply se_trans. { apply se_par_assoc. }
        apply se_par_cong_r. apply se_par_comm.
      - apply rs_par_r.
        apply (Split_fires_closed hash_process hash_process_closed ground_process ground_process_closed
                                  s1 s2 PNil closed_PNil).
      - (* Post-rearrange: gate | (s1-out | s2-out) ≡ (gate | s1-out) | s2-out *)
        apply se_sym. apply se_par_assoc. }
    (* State: ((gate | s1-out) | s2-out). *)
    (* Steps 2-3: compound half fires (2 rho_steps via the helper). *)
    apply (compound_half_fires_two_step hash_process hash_process_closed ground_process ground_process_closed
            P s1 s2 PNil (PNil) closed_PNil
            (closed_PDeref_Quote PNil closed_PNil)).
  - (* bisim: the post-state has two stuck residues; apply
       multi_stuck_residue_bisim. *)
    apply multi_stuck_residue_bisim.
    unfold head_count_inputs_outputs. simpl. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 15: Generic Headline Theorems (All Signature Shapes)
   ═══════════════════════════════════════════════════════════════════════════

   Dispatch over the four signature constructors (SUnit, SGround, SQuote,
   SAnd) to package the per-shape results into fully generic theorems that
   apply to ANY Rholang/Rhocalc COMM event signature.

   For SUnit, SGround, and SQuote, the fuel gate fires without a mediator.
   To present
   a UNIFORM interface (always PPar _ Ctx), we set Ctx = PNil and wrap the
   existing reachability via one rs_struct step absorbing the se_par_nil
   equivalence [PPar X PNil ≡ X]. For SAnd, the Ctx is the Split mediator
   and the existing theorem already has the right shape.                    *)

Theorem translation_strong_bisimilar_generic :
  forall (s : sig) (P : proc),
    exists Ctx W,
      closed_proc Ctx /\
      rho_reachable
        (PPar (Sy (SPar (SSigned P s) (SToken (TGate s TUnit))))
              Ctx)
        W
      /\ bisim W P.
Proof.
  intros s P.
  destruct s as [| bs | bs | s1 s2].
  - (* SUnit: Ctx = PNil.
       unit_translation_strong_bisimilar gives rho_reachable (Sy ...) W.
       We need rho_reachable (PPar (Sy ...) PNil) W.
       The reachability has exactly 1 step (unit_system_fuel_gate_fires +
       post_gate_unit + rr_refl). We just unfold the whole thing. *)
    exists PNil.
    exists (PPar P (PNil)).
    split; [exact I | split].
    + (* rho_reachable (PPar (Sy ...) PNil) (PPar P (PNil)) *)
      eapply rr_step.
      * eapply rs_struct.
        -- apply se_par_nil.
        -- apply unit_system_fuel_gate_fires.
        -- apply se_refl.
      * rewrite post_gate_unit. apply rr_refl.
    + apply post_gate_bisim.
  - (* SGround bs: Ctx = PNil. Same pattern as SUnit. *)
    exists PNil.
    exists (PPar P (PNil)).
    split; [exact I | split].
    + eapply rr_step.
      * eapply rs_struct.
        -- apply se_par_nil.
        -- apply ground_system_fuel_gate_fires.
        -- apply se_refl.
      * apply rr_refl.
    + apply post_gate_bisim.
  - (* SQuote bs: Ctx = PNil. Same pattern as SUnit. *)
    exists PNil.
    exists (PPar P (PNil)).
    split; [exact I | split].
    + eapply rr_step.
      * eapply rs_struct.
        -- apply se_par_nil.
        -- apply quote_system_fuel_gate_fires.
        -- apply se_refl.
      * apply rr_refl.
    + apply post_gate_bisim.
  - (* SAnd s1 s2: Ctx = Split hash_process ground_process s1 s2. *)
    exists (Split hash_process ground_process s1 s2).
    destruct (compound_translation_strong_bisimilar s1 s2 P) as [W [Hr Hb]].
    exists W.
    split; [apply (Split_closed hash_process hash_process_closed ground_process ground_process_closed s1 s2)
           | split; [exact Hr | exact Hb]].
Qed.

End BisimulationProofs.
