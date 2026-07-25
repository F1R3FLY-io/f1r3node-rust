(* ═══════════════════════════════════════════════════════════════════════════
   ForkChoice.v — Slashed-validator exclusion from fork choice

   Models the property that fork-choice estimator filters out the latest
   messages of slashed validators. Theorem T-10.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition       │ Rust Implementation                         │
   ──────────────────────┼─────────────────────────────────────────────┤
   ForkChoiceState       │ Estimator state                             │
   slashedSet            │ set of v with bonds_map[v] = 0 after slash  │
   filter_slashed        │ Estimator filtering by bonds                │
   ─────────────────────────────────────────────────────────────────────

   Companion doc: slashing-verification.md §3.11, §6.4.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator Block.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Latest-messages map
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition LatestMessages := list (Validator * BlockHash).

Definition fc_lookup (lm : LatestMessages) (v : Validator) : option BlockHash :=
  match find (fun p => if validator_eq_dec (fst p) v then true else false) lm with
  | Some (_, h) => Some h
  | None => None
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Slashed-validator filter
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition filter_slashed (lm : LatestMessages) (bonds : BondMap)
  : LatestMessages :=
  filter (fun p =>
            match p with
            | (v, _) => Nat.ltb 0 (bm_lookup bonds v)
            end)
         lm.

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — T-10: Fork-choice exclusion
   ═══════════════════════════════════════════════════════════════════════════

   A slashed validator's latest message is filtered out — i.e., when the
   bond map has the offender at 0, the lookup in the filtered list
   returns None. *)

Theorem fork_choice_exclusion :
  forall lm bonds v,
    bm_lookup bonds v = 0 ->
    fc_lookup (filter_slashed lm bonds) v = None.
Proof.
  intros lm bonds v Hb.
  unfold fc_lookup, filter_slashed.
  induction lm as [| [v' h'] rest IH]; simpl.
  - reflexivity.
  - destruct (Nat.ltb 0 (bm_lookup bonds v')) eqn:Eb; simpl.
    + destruct (validator_eq_dec v' v) as [Eq | Neq].
      * subst v'. rewrite Hb in Eb. discriminate.
      * apply IH.
    + apply IH.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — Active validators are preserved
   ═══════════════════════════════════════════════════════════════════════════

   For non-slashed validators, the lookup is unchanged. *)

Theorem fork_choice_preserves_active :
  forall lm bonds v h,
    fc_lookup lm v = Some h ->
    bm_lookup bonds v > 0 ->
    fc_lookup (filter_slashed lm bonds) v = Some h.
Proof.
  intros lm bonds v h Hl Hb.
  unfold fc_lookup, filter_slashed in *.
  induction lm as [| [v' h'] rest IH]; simpl in Hl |- *.
  - discriminate.
  - destruct (validator_eq_dec v' v) as [Eq | Neq].
    + simpl in Hl.
      destruct (validator_eq_dec v' v) as [_ | C]; [|contradiction].
      inversion Hl. subst h'.
      assert (Hbtrue : Nat.ltb 0 (bm_lookup bonds v') = true).
      { subst v'. apply Nat.ltb_lt. assumption. }
      rewrite Hbtrue. simpl.
      destruct (validator_eq_dec v' v) as [_ | C]; [|contradiction].
      reflexivity.
    + simpl in Hl.
      destruct (validator_eq_dec v' v) as [C | _]; [contradiction|].
      destruct (Nat.ltb 0 (bm_lookup bonds v')) eqn:Eb; simpl.
      * destruct (validator_eq_dec v' v) as [C | _]; [contradiction|].
        apply IH. assumption.
      * apply IH. assumption.
Qed.
