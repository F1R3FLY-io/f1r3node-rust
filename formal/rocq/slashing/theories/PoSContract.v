(* ═══════════════════════════════════════════════════════════════════════════
   PoSContract.v — The on-chain Proof-of-Stake slash transition

   Models the slash method of the PoS Rholang contract at
     casper/src/main/resources/PoS.rhox:432-495
   abstractly as a state-transition function on an idealized PoSState.

   Theorems:
     T-7 (slash_zeros_bond)        — successful slash zeros the bond
     T-8 (slash_transfers_stake)   — successful slash transfers to Coop vault
     T-9 (slash_idempotent)        — second slash is a no-op

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition       │ Rholang/Paper                │ Rust Implementation
   ──────────────────────┼──────────────────────────────┼─────────────────────
   PoSState              │ state in PoS.rhox            │ on-chain Rholang state
   ps_allBonds           │ state.allBonds : V → ℕ       │ same
   ps_delegatedTotals    │ state.delegatedTotals        │ same
   ps_pendingUndelegationTotals │ pending principal by validator │ same
   ps_active             │ state.activeValidators       │ same
   ps_coopVault          │ posVault balance             │ same
   slash                 │ @PoS!("slash", …)            │ SlashDeploy → invokes contract
   ─────────────────────────────────────────────────────────────────────────

   Companion doc: slashing-verification.md §6.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — PoS state record
   ═══════════════════════════════════════════════════════════════════════════ *)

Record PoSState : Type := mkPoSState {
  ps_allBonds                   : BondMap;
  ps_delegatedTotals            : BondMap;
  ps_pendingUndelegationTotals  : BondMap;
  ps_active                     : list Validator;
  ps_coopVault                  : nat
}.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — The slash transition
   ═══════════════════════════════════════════════════════════════════════════

   The transition takes a PoSState and an offender, and returns the new
   state plus a Boolean success flag.

   - If the offender's self-bond, delegated total, and pending undelegation
     exposure are all 0, slash is a no-op (idempotence).
   - Otherwise, that total exposure is moved to the Coop vault, the offender is
     removed from active, and the slash exposure maps are cleared.

   We assume the auth-token check has already passed at this entry point. *)

Definition slash_exposure (ps : PoSState) (v : Validator) : nat :=
  bm_lookup (ps_allBonds ps) v
  + bm_lookup (ps_delegatedTotals ps) v
  + bm_lookup (ps_pendingUndelegationTotals ps) v.

Definition slash (ps : PoSState) (v : Validator) : PoSState * bool :=
  let exposure := slash_exposure ps v in
  if Nat.eq_dec exposure 0
  then (ps, true)  (* idempotent no-op *)
  else
    (mkPoSState
       (bm_slash (ps_allBonds ps) v)
       (bm_remove (ps_delegatedTotals ps) v)
       (bm_remove (ps_pendingUndelegationTotals ps) v)
       (filter (fun v' => if validator_eq_dec v' v then false else true)
               (ps_active ps))
       (ps_coopVault ps + exposure),
     true).

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — T-7: slash zeros bond
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem slash_zeros_bond :
  forall ps v,
    let (ps', _) := slash ps v in
    bm_lookup (ps_allBonds ps') v = 0.
Proof.
  intros ps v.
  unfold slash.
  destruct (Nat.eq_dec (slash_exposure ps v) 0) as [E | NE].
  - simpl. unfold slash_exposure in E. lia.
  - simpl. apply bm_slash_lookup.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — T-8: slash transfers stake to Coop vault
   ═══════════════════════════════════════════════════════════════════════════

   When the offender's slash exposure is positive, the Coop vault balance
   increases by exactly the offender's pre-slash self-bond plus active
   delegated total plus pending undelegation exposure. *)

Theorem slash_transfers_stake :
  forall ps v,
    let exposure := slash_exposure ps v in
    let (ps', _) := slash ps v in
    exposure > 0 ->
    ps_coopVault ps' = ps_coopVault ps + exposure.
Proof.
  intros ps v.
  simpl.
  unfold slash.
  destruct (Nat.eq_dec (slash_exposure ps v) 0) as [E | NE]; simpl.
  - intro H. lia.
  - intro H. reflexivity.
Qed.

Theorem slash_zero_exposure_noop :
  forall ps v,
    slash_exposure ps v = 0 ->
    slash ps v = (ps, true).
Proof.
  intros ps v H.
  unfold slash.
  rewrite H.
  destruct (Nat.eq_dec 0 0) as [_ | Hneq]; [reflexivity | contradiction].
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — T-9: slash idempotence
   ═══════════════════════════════════════════════════════════════════════════

   Slashing the same validator twice yields the same state as slashing
   once. *)

Theorem slash_idempotent :
  forall ps v,
    let (ps1, _)  := slash ps  v in
    let (ps2, _)  := slash ps1 v in
    ps_allBonds ps2 = ps_allBonds ps1
    /\ ps_delegatedTotals ps2 = ps_delegatedTotals ps1
    /\ ps_pendingUndelegationTotals ps2 = ps_pendingUndelegationTotals ps1
    /\ ps_coopVault ps2 = ps_coopVault ps1
    /\ ps_active ps2 = ps_active ps1.
Proof.
  intros ps v.
  unfold slash at 1.
  destruct (Nat.eq_dec (slash_exposure ps v) 0) as [E | NE]; simpl.
  - (* First slash is a no-op; second is also a no-op since exposure is still 0. *)
    unfold slash. simpl.
    rewrite E. simpl. repeat split; reflexivity.
  - (* First slash clears all slash exposure; second is a no-op. *)
    unfold slash. simpl.
    unfold slash_exposure. simpl.
    rewrite bm_slash_lookup.
    rewrite bm_lookup_remove_same.
    rewrite bm_lookup_remove_same.
    simpl.
    destruct (Nat.eq_dec 0 0) as [_ | Hneq]; [simpl; repeat split; reflexivity | contradiction].
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §6 — Slash leaves other validators' bonds unchanged
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem slash_other_unchanged :
  forall ps v v',
    v <> v' ->
    let (ps', _) := slash ps v in
    bm_lookup (ps_allBonds ps') v' = bm_lookup (ps_allBonds ps) v'.
Proof.
  intros ps v v' Hne.
  unfold slash.
  destruct (Nat.eq_dec (slash_exposure ps v) 0); simpl.
  - reflexivity.
  - apply bm_slash_other. assumption.
Qed.
