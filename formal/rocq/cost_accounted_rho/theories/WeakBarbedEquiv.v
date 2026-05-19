(* ═══════════════════════════════════════════════════════════════════════════
   WeakBarbedEquiv.v — Weak barbed equivalence (coinduction-free form)
   ═══════════════════════════════════════════════════════════════════════════

   This module defines weak input/output barbs and the hidden-channel
   observation relation used by [Replication.v]. The currently
   mechanized replication result is the axiom-free body-to-wrapper
   direction: weak barbs of P propagate to both [PReplicate P] and
   [bang_encoding x P].

   The equivalence is stated as a conjunctive property on reachable
   barbs — no cofix, no coinductive relation, no guardedness obligations.
   The relation is still available as a specification tool for
   observation modulo a hidden coordination channel, but the proof
   development does not assume a bidirectional replication equivalence.

   Key definitions:
   - [weak_barb_input  P x] : exists P', P ⇝* P' /\ input_barb  P' x.
   - [weak_barb_output P x] : exists P', P ⇝* P' /\ output_barb P' x.
   - [weak_barbed_equiv_except x P Q] : quadruple iff on visible weak barbs.

   Key lemmas (congruence laws needed by Replication.v Section 12):
   - [weak_barb_input_par_l],   [weak_barb_input_par_r],  [weak_barb_input_replicate]
   - [weak_barb_output_par_l],  [weak_barb_output_par_r], [weak_barb_output_replicate]
   - [weak_barb_reachable]    : weak barbs transport through reachability.

   Dependencies: Rocq 9.1.1 stdlib, RhoSyntax, RhoReduction (this project).
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Lists.List.
Import ListNotations.

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import StructEquivInversion.
From CostAccountedRho Require Import RhoReduction.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Weak input and output barbs
   ═══════════════════════════════════════════════════════════════════════════

   A weak input barb on channel x is the ability to REACH a state that
   exhibits a top-level input on a channel ≡N-equivalent to x. Dually
   for outputs. The ≡N-closure on the observed channel is required
   because [input_barb]/[output_barb] are Leibniz on the channel but
   structural equivalence of processes (PInput x B ≡ PInput x' B' with
   x ≡N x') can shift the observed channel within its ≡N-class. Without
   this closure, [weak_barb_*] would not be ≡-invariant on the source
   process P. The body-to-wrapper replication lemmas use [se_name_refl]
   to supply the trivial witness in the common case.                    *)

Definition weak_barb_input (P : proc) (x : name) : Prop :=
  exists P' y, rho_reachable P P' /\ x ≡N y /\ input_barb P' y.

Definition weak_barb_output (P : proc) (x : name) : Prop :=
  exists P' y, rho_reachable P P' /\ x ≡N y /\ output_barb P' y.

(* Immediate barbs are weak barbs (take zero steps; ≡N-witness is refl). *)
Lemma input_barb_weak : forall P x, input_barb P x -> weak_barb_input P x.
Proof.
  intros P x H. exists P, x.
  split; [apply rr_refl | split; [apply se_name_refl | exact H]].
Qed.

Lemma output_barb_weak : forall P x, output_barb P x -> weak_barb_output P x.
Proof.
  intros P x H. exists P, x.
  split; [apply rr_refl | split; [apply se_name_refl | exact H]].
Qed.

(* ≡N-closure on the observed channel: if x ≡N x', then x and x' are
   interchangeable as the subject of a weak barb. *)
Lemma weak_barb_input_name_se : forall P x x',
  x ≡N x' -> weak_barb_input P x -> weak_barb_input P x'.
Proof.
  intros P x x' Hxx' [P' [y [Hreach [Hxy Hb]]]].
  exists P', y.
  split; [exact Hreach | split; [| exact Hb]].
  eapply se_name_trans; [apply se_name_sym; exact Hxx' | exact Hxy].
Qed.

Lemma weak_barb_output_name_se : forall P x x',
  x ≡N x' -> weak_barb_output P x -> weak_barb_output P x'.
Proof.
  intros P x x' Hxx' [P' [y [Hreach [Hxy Hb]]]].
  exists P', y.
  split; [exact Hreach | split; [| exact Hb]].
  eapply se_name_trans; [apply se_name_sym; exact Hxx' | exact Hxy].
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Transport through reachability
   ═══════════════════════════════════════════════════════════════════════════

   If P ⇝* Q and Q has a weak barb on x, then P also has a weak barb on x
   (compose the reachability witnesses).                                     *)

Lemma weak_barb_input_reachable : forall P Q x,
  rho_reachable P Q -> weak_barb_input Q x -> weak_barb_input P x.
Proof.
  intros P Q x Hreach [Q' [y [HrQ [Hxy Hb]]]].
  exists Q', y.
  split; [eapply rho_reachable_trans; [exact Hreach | exact HrQ]
        | split; [exact Hxy | exact Hb]].
Qed.

Lemma weak_barb_output_reachable : forall P Q x,
  rho_reachable P Q -> weak_barb_output Q x -> weak_barb_output P x.
Proof.
  intros P Q x Hreach [Q' [y [HrQ [Hxy Hb]]]].
  exists Q', y.
  split; [eapply rho_reachable_trans; [exact Hreach | exact HrQ]
        | split; [exact Hxy | exact Hb]].
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Parallel composition and reachability
   ═══════════════════════════════════════════════════════════════════════════

   Reduction preserves the right-hand side of a par when the left side
   steps, and vice versa. These are foundational for parallel-congruence
   of weak barbs. They mirror [rs_par_l] / [rs_par_r] at the reachability
   level.                                                                    *)

Lemma rho_reachable_par_l : forall P P' Q,
  rho_reachable P P' -> rho_reachable (PPar P Q) (PPar P' Q).
Proof.
  intros P P' Q Hreach. induction Hreach.
  - apply rr_refl.
  - eapply rr_step.
    + apply rs_par_l. exact H.
    + exact IHHreach.
Qed.

Lemma rho_reachable_par_r : forall P P' Q,
  rho_reachable P P' -> rho_reachable (PPar Q P) (PPar Q P').
Proof.
  intros P P' Q Hreach. induction Hreach.
  - apply rr_refl.
  - eapply rr_step.
    + apply rs_par_r. exact H.
    + exact IHHreach.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Parallel and replication congruence for weak barbs
   ═══════════════════════════════════════════════════════════════════════════

   A weak barb in one arm of a parallel lifts to a weak barb on the
   parallel composition. The reachable witness sits on the same arm.       *)

Lemma weak_barb_input_par_l : forall P Q x,
  weak_barb_input P x -> weak_barb_input (PPar P Q) x.
Proof.
  intros P Q x [P' [y [Hreach [Hxy Hb]]]].
  exists (PPar P' Q), y.
  split; [apply rho_reachable_par_l; exact Hreach
        | split; [exact Hxy | apply input_barb_par_l; exact Hb]].
Qed.

Lemma weak_barb_input_par_r : forall P Q x,
  weak_barb_input P x -> weak_barb_input (PPar Q P) x.
Proof.
  intros P Q x [P' [y [Hreach [Hxy Hb]]]].
  exists (PPar Q P'), y.
  split; [apply rho_reachable_par_r; exact Hreach
        | split; [exact Hxy | apply input_barb_par_r; exact Hb]].
Qed.

Lemma weak_barb_output_par_l : forall P Q x,
  weak_barb_output P x -> weak_barb_output (PPar P Q) x.
Proof.
  intros P Q x [P' [y [Hreach [Hxy Hb]]]].
  exists (PPar P' Q), y.
  split; [apply rho_reachable_par_l; exact Hreach
        | split; [exact Hxy | apply output_barb_par_l; exact Hb]].
Qed.

Lemma weak_barb_output_par_r : forall P Q x,
  weak_barb_output P x -> weak_barb_output (PPar Q P) x.
Proof.
  intros P Q x [P' [y [Hreach [Hxy Hb]]]].
  exists (PPar Q P'), y.
  split; [apply rho_reachable_par_r; exact Hreach
        | split; [exact Hxy | apply output_barb_par_r; exact Hb]].
Qed.

(* Replication propagates weak barbs from its body. By [rs_replicate],
   [PReplicate P] reaches [PPar P (PReplicate P)] in one step; any barb
   of P then appears as a parallel-left barb. *)
Lemma weak_barb_input_replicate_body : forall P x,
  weak_barb_input P x -> weak_barb_input (PReplicate P) x.
Proof.
  intros P x [P' [y [Hreach [Hxy Hb]]]].
  exists (PPar P' (PReplicate P)), y.
  split.
  - eapply rr_step.
    + apply rs_replicate.
    + apply rho_reachable_par_l. exact Hreach.
  - split; [exact Hxy | apply input_barb_par_l; exact Hb].
Qed.

Lemma weak_barb_output_replicate_body : forall P x,
  weak_barb_output P x -> weak_barb_output (PReplicate P) x.
Proof.
  intros P x [P' [y [Hreach [Hxy Hb]]]].
  exists (PPar P' (PReplicate P)), y.
  split.
  - eapply rr_step.
    + apply rs_replicate.
    + apply rho_reachable_par_l. exact Hreach.
  - split; [exact Hxy | apply output_barb_par_l; exact Hb].
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 5: Weak barbed equivalence modulo a hidden channel
   ═══════════════════════════════════════════════════════════════════════════

   Two processes P and Q are weakly barbed equivalent modulo the hidden
   channel x when they exhibit the same weak input/output barbs on every
   channel y that is not structurally equivalent to x. This is the
   coinduction-free statement of the equivalence: a conjunction of four
   bi-implications, no step-matching clause, no cofix. The content of
   "Q can do whatever P can do" is encoded directly as "every reachable
   observable in P is matched by a reachable observable in Q", and vice
   versa.                                                                   *)

Definition weak_barbed_equiv_except (x : name) (P Q : proc) : Prop :=
  (forall y, ~ (x ≡N y) ->
     weak_barb_input P y <-> weak_barb_input Q y)
  /\ (forall y, ~ (x ≡N y) ->
     weak_barb_output P y <-> weak_barb_output Q y).

(* Reflexivity: every process is weakly equivalent to itself on any
   hidden channel. *)
Lemma weak_barbed_equiv_except_refl : forall x P,
  weak_barbed_equiv_except x P P.
Proof.
  intros x P. split; intros y _; reflexivity.
Qed.

(* Symmetry: the relation is preserved by swapping P and Q. *)
Lemma weak_barbed_equiv_except_sym : forall x P Q,
  weak_barbed_equiv_except x P Q -> weak_barbed_equiv_except x Q P.
Proof.
  intros x P Q [Hi Ho]. split; intros y Hne.
  - rewrite Hi by exact Hne. reflexivity.
  - rewrite Ho by exact Hne. reflexivity.
Qed.

(* Transitivity: chaining two equivalences at the same hidden channel. *)
Lemma weak_barbed_equiv_except_trans : forall x P Q R,
  weak_barbed_equiv_except x P Q ->
  weak_barbed_equiv_except x Q R ->
  weak_barbed_equiv_except x P R.
Proof.
  intros x P Q R [HiPQ HoPQ] [HiQR HoQR]. split; intros y Hne.
  - rewrite HiPQ by exact Hne. apply HiQR. exact Hne.
  - rewrite HoPQ by exact Hne. apply HoQR. exact Hne.
Qed.
