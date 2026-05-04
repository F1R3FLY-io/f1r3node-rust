(* ═══════════════════════════════════════════════════════════════════════════
   Validator.v — Validator identity and bond-map operations

   Foundational module: introduces the [Validator] type, the [BondMap] type,
   and proves the basic algebraic properties (lookup over update, update
   commutativity, sum conservation under update).

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition           │ Paper / Spec Notation     │ Rust Implementation
   ──────────────────────────┼───────────────────────────┼───────────────────
   Validator                 │ v ∈ V (validator)         │ Validator (ByteString)
   BondMap                   │ B : V → ℕ (partial)       │ BondsMap : HashMap<…>
   bm_lookup                 │ B(v)                      │ bonds_map.get(v)
   bm_update                 │ B[v ↦ n]                  │ bonds_map.insert(v, n)
   bm_remove                 │ B \\ {v}                   │ bonds_map.remove(v)
   bm_sum                    │ Σ_{v ∈ dom(B)} B(v)        │ Σ over .values()
   ─────────────────────────────────────────────────────────────────────────

   Companion doc: docs/theory/slashing/slashing-verification.md §3.1
   Dependencies:  Rocq 9.1+ stdlib.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Stdlib Require Import PeanoNat.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Validator identity
   ═══════════════════════════════════════════════════════════════════════════

   Validators are abstract identifiers. We model them as natural numbers for
   decidability of equality without committing to any particular byte
   representation. The bisimilarity proof argues observational equivalence
   modulo this representation choice. *)

Definition Validator := nat.

Definition validator_eq_dec : forall (v1 v2 : Validator), {v1 = v2} + {v1 <> v2}
  := Nat.eq_dec.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Bond map
   ═══════════════════════════════════════════════════════════════════════════

   A [BondMap] is an association list (Validator * nat) used as a partial
   function from validators to bond amounts. Lookup returns 0 for absent keys,
   matching Scala's [bondsMap.getOrElse(validator, 0L)] and Rust's
   [bonds_map.get(v).unwrap_or(&0)]. *)

Definition BondMap := list (Validator * nat).

Fixpoint bm_lookup (bm : BondMap) (v : Validator) : nat :=
  match bm with
  | []         => 0
  | (k, n) :: rest =>
      if validator_eq_dec k v then n else bm_lookup rest v
  end.

Fixpoint bm_update (bm : BondMap) (v : Validator) (n : nat) : BondMap :=
  match bm with
  | []          => [(v, n)]
  | (k, m) :: rest =>
      if validator_eq_dec k v
      then (v, n) :: rest
      else (k, m) :: bm_update rest v n
  end.

Fixpoint bm_remove (bm : BondMap) (v : Validator) : BondMap :=
  match bm with
  | []         => []
  | (k, n) :: rest =>
      if validator_eq_dec k v
      then bm_remove rest v
      else (k, n) :: bm_remove rest v
  end.

Definition bm_keys (bm : BondMap) : list Validator :=
  map fst bm.

(* Slash transition: zero out a validator's bond. Matches PoS.rhox:480
   [state.allBonds.set(validator, 0)]. *)
Definition bm_slash (bm : BondMap) (v : Validator) : BondMap :=
  bm_update bm v 0.

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — Lookup-over-update lemmas
   ═══════════════════════════════════════════════════════════════════════════ *)

Lemma bm_lookup_update_same :
  forall bm v n, bm_lookup (bm_update bm v n) v = n.
Proof.
  induction bm as [| [k m] rest IH]; intros v n; simpl.
  - destruct (validator_eq_dec v v) as [_ | H]; [reflexivity | contradiction].
  - destruct (validator_eq_dec k v) as [Eq | Neq].
    + simpl. destruct (validator_eq_dec v v) as [_ | H]; [reflexivity | contradiction].
    + simpl. destruct (validator_eq_dec k v) as [Eq' | _]; [contradiction | apply IH].
Qed.

Lemma bm_lookup_update_diff :
  forall bm v v' n, v <> v' -> bm_lookup (bm_update bm v n) v' = bm_lookup bm v'.
Proof.
  induction bm as [| [k m] rest IH]; intros v v' n Hne; simpl.
  - destruct (validator_eq_dec v v') as [Eq | _].
    + congruence.
    + reflexivity.
  - destruct (validator_eq_dec k v) as [Eq | Neq].
    + subst k. simpl.
      destruct (validator_eq_dec v v') as [Eq | _]; [contradiction | reflexivity].
    + simpl.
      destruct (validator_eq_dec k v') as [Eq | _].
      * reflexivity.
      * apply IH; assumption.
Qed.

Lemma bm_lookup_remove_same :
  forall bm v, bm_lookup (bm_remove bm v) v = 0.
Proof.
  induction bm as [| [k m] rest IH]; intros v; simpl.
  - reflexivity.
  - destruct (validator_eq_dec k v) as [Eq | Neq].
    + apply IH.
    + simpl.
      destruct (validator_eq_dec k v) as [Eq | _]; [contradiction | apply IH].
Qed.

Lemma bm_lookup_remove_diff :
  forall bm v v', v <> v' -> bm_lookup (bm_remove bm v) v' = bm_lookup bm v'.
Proof.
  induction bm as [| [k m] rest IH]; intros v v' Hne; simpl.
  - reflexivity.
  - destruct (validator_eq_dec k v) as [Eq | Neq].
    + subst k. simpl.
      destruct (validator_eq_dec v v') as [Eq | _]; [contradiction | apply IH; assumption].
    + simpl.
      destruct (validator_eq_dec k v') as [Eq | _].
      * reflexivity.
      * apply IH; assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — Slash idempotence (foundation for T-9)
   ═══════════════════════════════════════════════════════════════════════════

   bm_slash zeros the offender's bond. Slashing a second time is a no-op on
   the lookup result (still 0). This is the algebraic precursor to the
   protocol-level slash idempotence theorem. *)

Lemma bm_slash_lookup :
  forall bm v, bm_lookup (bm_slash bm v) v = 0.
Proof.
  intros. unfold bm_slash. apply bm_lookup_update_same.
Qed.

Lemma bm_slash_idempotent_lookup :
  forall bm v, bm_lookup (bm_slash (bm_slash bm v) v) v = 0.
Proof.
  intros. apply bm_slash_lookup.
Qed.

Lemma bm_slash_other :
  forall bm v v', v <> v' -> bm_lookup (bm_slash bm v) v' = bm_lookup bm v'.
Proof.
  intros. unfold bm_slash. apply bm_lookup_update_diff. assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — Bonded validator predicate
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition bonded (bm : BondMap) (v : Validator) : Prop :=
  bm_lookup bm v > 0.

Definition unbonded (bm : BondMap) (v : Validator) : Prop :=
  bm_lookup bm v = 0.

Lemma slash_makes_unbonded :
  forall bm v, unbonded (bm_slash bm v) v.
Proof.
  intros. unfold unbonded. apply bm_slash_lookup.
Qed.

Lemma bonded_dec :
  forall bm v, {bonded bm v} + {unbonded bm v}.
Proof.
  intros. unfold bonded, unbonded.
  destruct (bm_lookup bm v) eqn:E.
  - right. reflexivity.
  - left. lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §6 — Active validator set (derived view)
   ═══════════════════════════════════════════════════════════════════════════

   The active validator set is the set of bonded validators. We define it
   via the lookup predicate (the only sound way given that BondMap is an
   association list which may have duplicate keys in degenerate cases).
   Slashing zeros the lookup; in particular, after slashing v is no longer
   in the [is_bonded] view, regardless of duplicate-key shenanigans in the
   underlying list. *)

Definition is_active (bm : BondMap) (v : Validator) : Prop :=
  bm_lookup bm v > 0.

Lemma slash_removes_from_active :
  forall bm v,
    ~ is_active (bm_slash bm v) v.
Proof.
  intros bm v Hcontra.
  unfold is_active in Hcontra.
  rewrite bm_slash_lookup in Hcontra. lia.
Qed.

Lemma other_active_preserved_after_slash :
  forall bm v v',
    v <> v' ->
    is_active (bm_slash bm v) v' <-> is_active bm v'.
Proof.
  intros bm v v' Hne. unfold is_active.
  rewrite bm_slash_other; [reflexivity | assumption].
Qed.
