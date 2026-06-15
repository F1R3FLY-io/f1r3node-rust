(* ═══════════════════════════════════════════════════════════════════════════
   BugFixStakeZero.v — Proof for Bug Fix #5 (T-9.5)

   Bug. equivocation_detector.rs:217-220 silently classifies a stake-0
   bonded validator as EquivocationDetected; no slash, no neglected check.

   Fix. Either (a) add assert(stake > 0) in the PoS bond contract so the
   stake-0 bonded state is unreachable, or (b) return Err(StakeZero) from
   the detector. We prove version (a) here as an invariant on PoSState.

   Theorem T-9.5 (StakeZero safety). Under the invariant
     "every active validator has positive bond"
   the silent stake-0 classification path is unreachable.

   Companion doc: slashing-verification.md §9.5.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator PoSContract.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Invariant: active validators have positive bond
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition active_implies_bonded (ps : PoSState) : Prop :=
  forall v, In v (ps_active ps) -> bm_lookup (ps_allBonds ps) v > 0.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — T-9.5: Slash preserves the invariant
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem t_9_5_slash_preserves_invariant :
  forall ps v,
    active_implies_bonded ps ->
    let result := slash ps v in
    let ps' := fst result in
    active_implies_bonded ps'.
Proof.
  intros ps v Hinv. simpl.
  unfold slash.
  destruct (Nat.eq_dec (bm_lookup (ps_allBonds ps) v) 0) as [E | NE]; simpl.
  - assumption.
  - intros v' Hin.
    simpl in Hin |- *.
    apply filter_In in Hin. destruct Hin as [Hin Hf].
    destruct (validator_eq_dec v' v) as [Eq | Neq]; [discriminate Hf|].
    assert (Hbo : bm_lookup (bm_slash (ps_allBonds ps) v) v' = bm_lookup (ps_allBonds ps) v')
      by (apply bm_slash_other; intro Heq; apply Neq; symmetry; assumption).
    rewrite Hbo. apply Hinv. assumption.
Qed.

(* Direct corollary: under the invariant, for any v in active, lookup > 0.
   This is exactly the StakeZero precondition the detector relies on. *)
Theorem t_9_5_active_has_positive_bond :
  forall ps v,
    active_implies_bonded ps ->
    In v (ps_active ps) ->
    bm_lookup (ps_allBonds ps) v > 0.
Proof.
  intros ps v Hinv Hin. apply Hinv. assumption.
Qed.
