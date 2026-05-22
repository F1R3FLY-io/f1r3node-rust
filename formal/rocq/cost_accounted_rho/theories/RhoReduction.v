(* ═══════════════════════════════════════════════════════════════════════════
   RhoReduction.v — Operational semantics of the pure rho calculus
   ═══════════════════════════════════════════════════════════════════════════

   Defines the small-step reduction relation for the reflective higher-order
   process calculus (rho calculus) on top of the syntax and structural
   equivalence introduced in RhoSyntax.v.

   The operational semantics is given in three layers:

   1. The COMM rule  -- the only computational base case. A receiver
        for(y ← x){P}  in parallel with a sender  x!(Q)  reduces to
        P{@Q/y}, where the bound name variable y (NVar 0 in P) is
        replaced by the name @Q (i.e., Quote Q).

   2. Contextual closure under parallel composition  -- the PAR rule.
        Reduction is preserved on either side of a parallel composition,
        so subterms can take steps independently.

   3. Closure under structural equivalence  -- the STRUCT rule.
        Reduction respects the algebraic laws of parallel composition
        (associativity, commutativity, identity), allowing communicating
        partners to "find" each other across associativity boundaries.

   On top of single-step reduction we build:
   - the reflexive-transitive closure  P ⇝* Q
   - the barb predicate  barb P x  capturing observational ports

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition          │ Paper Notation        │ Rust Implementation
   ─────────────────────────┼───────────────────────┼─────────────────────
   rho_step / "P ⇝ Q"       │ P → Q (one step)      │ Reducer::reduce
   rs_comm                  │ COMM rule             │ Comm channel match
   rs_par_l, rs_par_r       │ PAR rule              │ Parallel scheduler
   rs_struct                │ STRUCT rule           │ (normalized form)
   rho_reachable / "⇝*"     │ →* (many steps)       │ Reducer loop
   barb P x                 │ P ↓ x                 │ Outstanding port
   ─────────────────────────────────────────────────────────────────────────

   Dependencies: Rocq 9.1.1 stdlib, RhoSyntax (this project)
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Lia.
Import ListNotations.

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import StructEquivInversion.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Single-Step Reduction
   ═══════════════════════════════════════════════════════════════════════════

   The fundamental computational rule of the rho calculus is COMM:
   when a sender and a receiver agree on a channel, the receiver's body
   is instantiated with the name that was sent. We use de Bruijn index 0
   (i.e., NVar 0) for the bound name variable of the input.

   The substituent is [Quote Q], the name @Q built from the sent
   process Q. Substitution replaces every NVar 0 inside the body with
   this name, capturing the paper's semantics for P{@Q/y}.            *)

Reserved Notation "P ⇝ Q" (at level 70, no associativity).

Inductive rho_step : proc -> proc -> Prop :=
  (* COMM rule: communication on a shared channel x. The bound name
     variable of the input (NVar 0) is replaced inside P by the name
     [Quote Q] obtained by quoting the sent process Q. *)
  | rs_comm : forall (x : name) (P Q : proc),
      rho_step (PPar (PInput x P) (POutput x Q))
               (subst_proc P 0 (Quote Q))

  (* PAR rule (left): a step on the left component of a parallel
     composition lifts to a step on the whole composition. *)
  | rs_par_l : forall P P' Q,
      rho_step P P' ->
      rho_step (PPar P Q) (PPar P' Q)

  (* PAR rule (right): symmetric to rs_par_l. *)
  | rs_par_r : forall P P' Q,
      rho_step P P' ->
      rho_step (PPar Q P) (PPar Q P')

  (* STRUCT rule: structurally equivalent processes step to structurally
     equivalent results. This allows the COMM and PAR rules to "see
     through" the algebraic laws of parallel composition. *)
  | rs_struct : forall P P' Q' Q,
      P ≡ P' ->
      rho_step P' Q' ->
      Q' ≡ Q ->
      rho_step P Q

  (* REPLICATE rule: a replicated process unfolds to one copy in
     parallel with the replication, modelling infinite availability.
     This is the operational counterpart of the structural axiom
     !P ≡ P | !P, but given as a directed reduction step. *)
  | rs_replicate : forall P,
      rho_step (PReplicate P) (PPar P (PReplicate P))

where "P ⇝ Q" := (rho_step P Q).

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Reflexive-Transitive Closure
   ═══════════════════════════════════════════════════════════════════════════

   [rho_reachable P Q] holds when Q is obtained from P by zero or more
   reduction steps. This is the standard inductive definition of the
   reflexive-transitive closure of [rho_step].                              *)

Inductive rho_reachable : proc -> proc -> Prop :=
  | rr_refl : forall P, rho_reachable P P
  | rr_step : forall P Q R,
      rho_step P Q ->
      rho_reachable Q R ->
      rho_reachable P R.

Notation "P ⇝* Q" := (rho_reachable P Q) (at level 70, no associativity).

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Reachability Lemmas
   ═══════════════════════════════════════════════════════════════════════════ *)

(* A single step is a reachability sequence of length one. *)
Lemma rho_reachable_one : forall P Q,
  rho_step P Q -> rho_reachable P Q.
Proof.
  intros P Q Hstep.
  eapply rr_step.
  - exact Hstep.
  - apply rr_refl.
Qed.

(* Reachability is transitive: composing two reduction sequences yields
   a reduction sequence. The proof proceeds by induction on the first
   sequence; at each step we re-apply the inductive hypothesis to the
   second sequence, which we revert into the goal so that it tracks the
   intermediate term faithfully. *)
Lemma rho_reachable_trans : forall P Q R,
  rho_reachable P Q ->
  rho_reachable Q R ->
  rho_reachable P R.
Proof.
  intros P Q R H1.
  revert R.
  induction H1 as [P0 | P0 Q0 R0 Hstep Hreach IH]; intros R H2.
  - (* rr_refl: the first sequence is empty, so the second IS the result. *)
    exact H2.
  - (* rr_step: P0 ⇝ Q0 and Q0 ⇝* R0; we need P0 ⇝* R given R0 ⇝* R. *)
    eapply rr_step.
    + exact Hstep.
    + apply IH. exact H2.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Convenience Constructors
   ═══════════════════════════════════════════════════════════════════════════

   These lemmas give explicit names to the most common ways of building
   a reduction step, for use in client proofs.                              *)

(* Closure under structural equivalence on the left:
   if P ≡ P' and P' steps to Q, then P also steps to Q. *)
Lemma rho_step_struct : forall P P' Q,
  P ≡ P' ->
  rho_step P' Q ->
  rho_step P Q.
Proof.
  intros P P' Q Heq Hstep.
  eapply rs_struct.
  - exact Heq.
  - exact Hstep.
  - apply se_refl.
Qed.

(* Re-export rs_par_l under a more descriptive name. *)
Lemma rho_step_par_l_intro : forall P P' Q,
  rho_step P P' ->
  rho_step (PPar P Q) (PPar P' Q).
Proof.
  intros P P' Q Hstep.
  apply rs_par_l. exact Hstep.
Qed.

(* Re-export rs_par_r symmetrically. *)
Lemma rho_step_par_r_intro : forall P P' Q,
  rho_step P P' ->
  rho_step (PPar Q P) (PPar Q P').
Proof.
  intros P P' Q Hstep.
  apply rs_par_r. exact Hstep.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 5: Barbs (Observability)
   ═══════════════════════════════════════════════════════════════════════════

   A barb captures the notion of an "observable port": a process P barbs
   on a name x, written  P ↓ x, when P has at the top level (modulo
   parallel composition) an input or output on the channel x. Barbs are
   the basic observations used to define behavioural equivalences such
   as barbed bisimilarity.                                                  *)

Inductive barb : proc -> name -> Prop :=
  | barb_input  : forall x P,
      barb (PInput x P) x
  | barb_output : forall x Q,
      barb (POutput x Q) x
  | barb_par_l  : forall P Q x,
      barb P x ->
      barb (PPar P Q) x
  | barb_par_r  : forall P Q x,
      barb P x ->
      barb (PPar Q P) x
  | barb_replicate : forall P x,
      barb P x ->
      barb (PReplicate P) x.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 6: Sanity Checks on Barbs
   ═══════════════════════════════════════════════════════════════════════════ *)

(* A COMM redex barbs on its shared channel via the receiver position
   (left of the parallel composition, by barb_par_l + barb_input). *)
Lemma barb_comm_input : forall x P Q,
  barb (PPar (PInput x P) (POutput x Q)) x.
Proof.
  intros x P Q.
  apply barb_par_l. apply barb_input.
Qed.

(* The same COMM redex also barbs via the sender position
   (right of the parallel composition, by barb_par_r + barb_output). *)
Lemma barb_comm_output : forall x P Q,
  barb (PPar (PInput x P) (POutput x Q)) x.
Proof.
  intros x P Q.
  apply barb_par_r. apply barb_output.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 7: Stuck Processes
   ═══════════════════════════════════════════════════════════════════════════

   Foundational stuck lemmas: certain process forms cannot take any
   reduction step. The proof technique uses the [head_count] measure
   from [StructEquivInversion.v]: every reducible process has at least
   2 heads at the top level (a sender + a receiver), so any process
   with strictly fewer than 2 heads is stuck.

   This bounds the reducibility of [PNil] (count 0), [PDeref _], a single
   [PInput _ _], and a single [POutput _ _] (each count 1). Each is
   provably stuck.                                                          *)

(* Every rho_step source has at least one head. This is unconditionally
   true: rs_replicate gives head_count = 1, all other constructors
   give >= 2. *)
Lemma rho_step_head_count_ge_one : forall P P',
  rho_step P P' -> 1 <= head_count P.
Proof.
  intros P P' Hstep.
  induction Hstep; simpl in *.
  - (* rs_comm *) lia.
  - (* rs_par_l *) lia.
  - (* rs_par_r *) lia.
  - (* rs_struct *) apply head_count_se in H. lia.
  - (* rs_replicate: head_count (PReplicate P) = 1 *) lia.
Qed.

(* The fundamental "head count lower bound" for reducibility of
   non-replicate processes. Every rho_step whose source contains no
   top-level replicates requires at least two heads. When the source
   IS a replicate, the hypothesis [count_replicates P = 0] gives a
   contradiction. *)
Lemma rho_step_head_count_ge_two : forall P P',
  rho_step P P' -> count_replicates P = 0 -> 2 <= head_count P.
Proof.
  intros P P' Hstep.
  induction Hstep; intro Hno_rep; simpl in *.
  - (* rs_comm: source = PPar (PInput x P) (POutput x Q),
       head_count = 1 + 1 = 2. *)
    lia.
  - (* rs_par_l: head_count (PPar P Q) = head_count P + head_count Q
       and IH gives head_count P >= 2 (after passing count_replicates
       hypothesis through the sum). *)
    assert (Hrp : count_replicates P = 0) by lia.
    specialize (IHHstep Hrp).
    lia.
  - (* rs_par_r: symmetric. *)
    assert (Hrp : count_replicates P = 0) by lia.
    specialize (IHHstep Hrp).
    lia.
  - (* rs_struct: head_count_se gives head_count P = head_count P',
       count_replicates_se gives count_replicates P = count_replicates P',
       and IH gives head_count P' >= 2. *)
    pose proof (head_count_se _ _ H) as Hhc.
    pose proof (count_replicates_se _ _ H) as Hcr.
    assert (Hrp' : count_replicates P' = 0) by lia.
    specialize (IHHstep Hrp').
    lia.
  - (* rs_replicate: count_replicates (PReplicate P) = 1 <> 0,
       contradicts hypothesis. *)
    lia.
Qed.

(* Corollary lemmas for the stuck constructors. Each follows from
   [rho_step_head_count_ge_two] by computing the head count. *)

Theorem PNil_stuck : forall R, ~ rho_step PNil R.
Proof.
  intros R H.
  pose proof (rho_step_head_count_ge_two _ _ H eq_refl) as Hhc.
  simpl in Hhc. lia.
Qed.

Theorem PDeref_stuck : forall x R, ~ rho_step (PDeref x) R.
Proof.
  intros x R H.
  pose proof (rho_step_head_count_ge_two _ _ H eq_refl) as Hhc.
  simpl in Hhc. lia.
Qed.

(* Convenience corollary for the [PDeref (Quote PNil)] residue used
   pervasively in Bisimulation.v's post-fuel-gate state. *)
Corollary PDeref_Quote_stuck : forall P R,
  ~ rho_step (PDeref (Quote P)) R.
Proof. intros. apply PDeref_stuck. Qed.

Theorem PInput_alone_stuck : forall x B R, ~ rho_step (PInput x B) R.
Proof.
  intros x B R H.
  pose proof (rho_step_head_count_ge_two _ _ H eq_refl) as Hhc.
  simpl in Hhc. lia.
Qed.

Theorem POutput_alone_stuck : forall x B R, ~ rho_step (POutput x B) R.
Proof.
  intros x B R H.
  pose proof (rho_step_head_count_ge_two _ _ H eq_refl) as Hhc.
  simpl in Hhc. lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 8: Replication Reduces
   ═══════════════════════════════════════════════════════════════════════════

   A replicated process always reduces to one copy in parallel with the
   replication. This is the positive counterpart of the stuck lemmas above:
   PReplicate is never stuck.                                                *)

Lemma PReplicate_reduces : forall P,
  rho_step (PReplicate P) (PPar P (PReplicate P)).
Proof.
  intros. apply rs_replicate.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 9: Split Barbs — distinguishing input barbs from output barbs
   ═══════════════════════════════════════════════════════════════════════════

   The [barb] relation above conflates top-level inputs and outputs into
   a single predicate. For weak-barbed-equivalence proofs that distinguish
   observable reception capabilities ("fors") from observable transmission
   capabilities ("sends"), we refine barbs into two disjoint families:

   - [input_barb  P x]  : P can receive on channel x at the top level.
   - [output_barb P x]  : P can send    on channel x at the top level.

   The existing [barb] is the disjoint union of these two:
     barb P x  <->  input_barb P x  \/  output_barb P x.

   Each family is closed under the same structural constructors as [barb]
   (parallel, replication) but with exactly one of the input/output leaves,
   not both.                                                                 *)

Inductive input_barb : proc -> name -> Prop :=
  | input_barb_here      : forall x P, input_barb (PInput x P) x
  | input_barb_par_l     : forall P Q x, input_barb P x -> input_barb (PPar P Q) x
  | input_barb_par_r     : forall P Q x, input_barb P x -> input_barb (PPar Q P) x
  | input_barb_replicate : forall P x,   input_barb P x -> input_barb (PReplicate P) x.

Inductive output_barb : proc -> name -> Prop :=
  | output_barb_here      : forall x Q, output_barb (POutput x Q) x
  | output_barb_par_l     : forall P Q x, output_barb P x -> output_barb (PPar P Q) x
  | output_barb_par_r     : forall P Q x, output_barb P x -> output_barb (PPar Q P) x
  | output_barb_replicate : forall P x,   output_barb P x -> output_barb (PReplicate P) x.

(* [barb] decomposes as the disjoint union of input and output barbs. *)
Lemma barb_iff_input_or_output : forall P x,
  barb P x <-> input_barb P x \/ output_barb P x.
Proof.
  intros P x. split.
  - intros Hb. induction Hb.
    + left. apply input_barb_here.
    + right. apply output_barb_here.
    + destruct IHHb as [Hi | Ho].
      * left. apply input_barb_par_l. exact Hi.
      * right. apply output_barb_par_l. exact Ho.
    + destruct IHHb as [Hi | Ho].
      * left. apply input_barb_par_r. exact Hi.
      * right. apply output_barb_par_r. exact Ho.
    + destruct IHHb as [Hi | Ho].
      * left. apply input_barb_replicate. exact Hi.
      * right. apply output_barb_replicate. exact Ho.
  - intros [Hi | Ho].
    + induction Hi.
      * apply barb_input.
      * apply barb_par_l. exact IHHi.
      * apply barb_par_r. exact IHHi.
      * apply barb_replicate. exact IHHi.
    + induction Ho.
      * apply barb_output.
      * apply barb_par_l. exact IHHo.
      * apply barb_par_r. exact IHHo.
      * apply barb_replicate. exact IHHo.
Qed.

(* Each direction as a standalone implication for convenience. *)
Lemma input_barb_to_barb : forall P x, input_barb P x -> barb P x.
Proof.
  intros P x H. apply barb_iff_input_or_output. left. exact H.
Qed.

Lemma output_barb_to_barb : forall P x, output_barb P x -> barb P x.
Proof.
  intros P x H. apply barb_iff_input_or_output. right. exact H.
Qed.

(* The stuck-process lemmas transfer trivially to the split barbs. *)
Lemma PNil_no_input_barb  : forall x, ~ input_barb  PNil x.
Proof. intros x H. inversion H. Qed.

Lemma PNil_no_output_barb : forall x, ~ output_barb PNil x.
Proof. intros x H. inversion H. Qed.

Lemma PDeref_no_input_barb  : forall n x, ~ input_barb  (PDeref n) x.
Proof. intros n x H. inversion H. Qed.

Lemma PDeref_no_output_barb : forall n x, ~ output_barb (PDeref n) x.
Proof. intros n x H. inversion H. Qed.
