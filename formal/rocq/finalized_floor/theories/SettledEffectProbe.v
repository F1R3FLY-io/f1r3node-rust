(* SettledEffectProbe — the settled-effect probe's walk algebra.
 *
 * Source: casper/src/rust/finality/deploy_lifecycle.rs (`effect_in_state_of`,
 * `effect_in_state_of_above`) and its merge-time probe sites
 * casper/src/rust/util/rholang/interpreter_util.rs (`sig_settled_in_base`,
 * `sig_settled_in_floor`). Claim document:
 * docs/claims/settled-effect-probe-equivalence.md (CLAIM-FINALITY-001).
 *
 * The probe decides whether a sig's effect is already committed on a state
 * lineage: it walks the merge-base chain from a tip down to a height bound
 * and answers TRUE iff some block on the walked segment applied the sig
 * (a non-failed `deploys` entry or an `applied_from_scope` entry).
 *
 * Soak run 33099406770 (issue #24) measured this per-sig walk at 92% of
 * `dag_merger::merge` wall time: ~30 probes per merge each re-walk the same
 * lineage segment. The remediation batches the walk — one pass collects
 * every applied sig, then each probe is a set-membership test — and reuses
 * answers across merges through segment memoization (the `checked_below`
 * early stop). This module proves the three facts that reshape rests on:
 *
 *   1. `walk_collect_equiv`      — the per-sig walk equals membership in the
 *                                  one-pass collection (batching is sound).
 *   2. `walk_segment_composition`— the walk distributes over segment
 *                                  concatenation (splitting is sound).
 *   3. `walk_memo_false_stable`  — a segment known FALSE for a sig can be
 *                                  skipped without changing any answer
 *                                  (`checked_below` memoization is sound).
 *
 * The model abstracts a lineage block to the finite set of sigs whose
 * effects it applies, and a walked segment to the tip-first list of such
 * blocks already truncated at the walk bound. What the model deliberately
 * does NOT capture — and the claim document records as premises — is the
 * walk-bound soundness premise (no block below `floor - deploy_lifespan`
 * can hold a scope-live sig's effect) and the availability rule (an absent
 * block body is `BlockNotHeld` deferral, never an answer).
 *
 * Zero `Admitted`. No custom `Axiom` or `Parameter`.
 *)

From Stdlib Require Import Lists.List.
From Stdlib Require Import Bool.Bool.
Import ListNotations.

Section SettledEffectProbe.

Context {Sig : Type}.
Hypothesis sig_eq_dec : forall a b : Sig, {a = b} + {a <> b}.

(* A lineage block, abstracted to the sigs whose effects it applies:
   its non-failed `deploys` entries plus its `applied_from_scope` list. *)
Definition lineage_block := list Sig.

(* A walked lineage segment, tip-first, truncated at the walk bound. *)
Definition segment := list lineage_block.

(* Reference semantics: the per-sig walk `effect_in_state_of` performs.
   TRUE iff some block on the segment applied the sig. *)
Fixpoint walk (seg : segment) (sig : Sig) : bool :=
  match seg with
  | [] => false
  | b :: rest =>
      if in_dec sig_eq_dec sig b then true else walk rest sig
  end.

(* Batched form: one pass down the segment collecting every applied sig. *)
Definition collect (seg : segment) : list Sig := concat seg.

(* 1 — Batching soundness: the per-sig walk answers TRUE exactly when the
   sig is a member of the one-pass collection. An optimized probe that
   builds `collect seg` once and answers by membership is extensionally
   equal to the reference walk. *)
Theorem walk_collect_equiv :
  forall (seg : segment) (sig : Sig),
    walk seg sig = true <-> In sig (collect seg).
Proof.
  induction seg as [| b rest IH]; intros sig; simpl.
  - split; [discriminate | contradiction].
  - destruct (in_dec sig_eq_dec sig b) as [Hin | Hnotin].
    + split; intros _; [apply in_or_app; left; exact Hin | reflexivity].
    + rewrite IH. split.
      * intros Hrest. apply in_or_app. right. exact Hrest.
      * intros Happ. apply in_app_or in Happ.
        destruct Happ as [Hb | Hrest]; [contradiction | exact Hrest].
Qed.

(* 2 — Segment composition: the walk over a concatenation is the boolean
   disjunction of the walks over the parts. Splitting one long walk into
   per-merge segments — or joining cached segments — changes no answer. *)
Theorem walk_segment_composition :
  forall (s1 s2 : segment) (sig : Sig),
    walk (s1 ++ s2) sig = walk s1 sig || walk s2 sig.
Proof.
  induction s1 as [| b rest IH]; intros s2 sig; simpl.
  - reflexivity.
  - destruct (in_dec sig_eq_dec sig b); [reflexivity | apply IH].
Qed.

(* 3 — FALSE-memo stability (`checked_below` soundness): when a lower
   segment is already known to answer FALSE for a sig, walking only the
   new upper segment gives the same answer as walking both. *)
Theorem walk_memo_false_stable :
  forall (s1 s2 : segment) (sig : Sig),
    walk s2 sig = false ->
    walk (s1 ++ s2) sig = walk s1 sig.
Proof.
  intros s1 s2 sig Hfalse.
  rewrite walk_segment_composition, Hfalse.
  apply orb_false_r.
Qed.

(* TRUE-stability, the complementary direction: a TRUE answer on any part
   survives extension. A settled sig stays settled however far above the
   memoized segment later merges walk. *)
Corollary walk_true_stable :
  forall (s1 s2 : segment) (sig : Sig),
    walk s2 sig = true ->
    walk (s1 ++ s2) sig = true.
Proof.
  intros s1 s2 sig Htrue.
  rewrite walk_segment_composition, Htrue.
  apply orb_true_r.
Qed.

End SettledEffectProbe.
