(* ===========================================================================
   Recovery.v - Record-driven recovery: NO-DOUBLE-APPLY (T-NDA).

   When the keep-one merge drops a concurrent whole-cell write (or any conflicting
   deploy), the loser is RECOVERED: re-executed on top of the winner in a later
   block (canonical_won_sigs / rejected-deploy re-proposal). The safety obligation
   is that recovery re-applies a dropped effect AT MOST ONCE -- a winner already
   reflected in the state must never be applied a second time, and a recovered
   loser must not be re-applied on every subsequent merge. For an additive
   (IntegerAdd) channel a double-apply would corrupt the running total; for a
   single-value cell it would resurrect a stale write.

   The protection is that the merge tracks the SET of deploy effects already
   reflected in the state and only applies an effect not already present. Modeled
   here as `apply_effect`, which adds a deploy id only if absent. Re-application is
   then idempotent (T-NDA): applying an already-applied effect is a no-op, and the
   membership set is unchanged.

   (Recovery LIVENESS -- every dropped loser is eventually re-proposed -- is a
   scheduling property, verified in TLA+ (Liveness_Progress) and exercised
   empirically by the convergence gates. Under SUSTAINED single-cell overload the
   recovery backlog can outgrow the deploy-lifespan window, a capacity bound (A10)
   documented with the map_cell soak; it is orthogonal to this safety property.)

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                  | Property        | Rust Implementation
   ----------------------+-----------------+-------------------------------------
   apply_effect          | dedup re-apply  | rejected-deploy recovery + scope dedup
   apply_effect_mem      | effect recorded | applied-sig set membership
   apply_idem            | T-NDA           | canonical_won_sigs one-shot re-proposal
   no_double_apply       | T-NDA (set)     | additive channel not double-counted
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
Import ListNotations.

(* A state's reflected effects, as the set of applied deploy ids. *)
Definition Effects := list nat.

Definition applied (s : Effects) (d : nat) : Prop := In d s.

(* Apply a deploy effect, deduplicating: add it only if not already reflected.
   This is the one-shot semantics the recovery path enforces. *)
Definition apply_effect (s : Effects) (d : nat) : Effects :=
  if in_dec Nat.eq_dec d s then s else d :: s.

(* After applying `d`, `d` is reflected in the state (whether it was new or not). *)
Lemma apply_effect_mem : forall s d, applied (apply_effect s d) d.
Proof.
  intros s d. unfold apply_effect, applied.
  destruct (in_dec Nat.eq_dec d s) as [Hin | Hnin].
  - exact Hin.
  - left. reflexivity.
Qed.

(* T-NDA: applying the same effect a second time is a no-op. A recovered loser
   (or a winner already reflected) is never double-applied. *)
Theorem apply_idem :
  forall s d, apply_effect (apply_effect s d) d = apply_effect s d.
Proof.
  intros s d. unfold apply_effect at 1.
  destruct (in_dec Nat.eq_dec d (apply_effect s d)) as [Hin | Hnin].
  - reflexivity.
  - exfalso. apply Hnin. apply apply_effect_mem.
Qed.

(* T-NDA (set form): re-applying an already-applied effect does not change which
   effects are reflected -- so an additive channel's total is not double-counted. *)
Theorem no_double_apply :
  forall s d, applied s d -> forall x, applied (apply_effect s d) x <-> applied s x.
Proof.
  intros s d Hd x. unfold apply_effect.
  destruct (in_dec Nat.eq_dec d s) as [Hin | Hnin].
  - split; intro H; exact H.
  - exfalso. apply Hnin. exact Hd.
Qed.
