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
   ps_active             │ state.activeValidators       │ same
   ps_coopVault          │ posVault balance              │ same
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
  ps_allBonds  : BondMap;
  ps_active    : list Validator;       (* set, modeled as list *)
  ps_coopVault : nat
}.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — The slash transition
   ═══════════════════════════════════════════════════════════════════════════

   The transition takes a PoSState and an offender, and returns the new
   state plus a Boolean success flag.

   - If the offender's bond is already 0, slash is a no-op (idempotence).
   - Otherwise, the bond is moved to the Coop vault, the offender is
     removed from active, and the bond map is zeroed.

   We assume the auth-token check has already passed at this entry point. *)

Definition slash (ps : PoSState) (v : Validator) : PoSState * bool :=
  let bond := bm_lookup (ps_allBonds ps) v in
  if Nat.eq_dec bond 0
  then (ps, true)  (* idempotent no-op *)
  else
    (mkPoSState
       (bm_slash (ps_allBonds ps) v)
       (filter (fun v' => if validator_eq_dec v' v then false else true)
               (ps_active ps))
       (ps_coopVault ps + bond),
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
  destruct (Nat.eq_dec (bm_lookup (ps_allBonds ps) v) 0) as [E | NE].
  - simpl. assumption.
  - simpl. apply bm_slash_lookup.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — T-8: slash transfers stake to Coop vault
   ═══════════════════════════════════════════════════════════════════════════

   When the offender's bond is positive, the Coop vault balance increases
   by exactly the offender's pre-slash bond. (For a stake-0 offender,
   slash is a no-op and the vault is unchanged.) *)

Theorem slash_transfers_stake :
  forall ps v,
    let bond := bm_lookup (ps_allBonds ps) v in
    let (ps', _) := slash ps v in
    bond > 0 ->
    ps_coopVault ps' = ps_coopVault ps + bond.
Proof.
  intros ps v.
  simpl.
  unfold slash.
  destruct (Nat.eq_dec (bm_lookup (ps_allBonds ps) v) 0) as [E | NE]; simpl.
  - intro H. lia.
  - intro H. reflexivity.
Qed.

Theorem slash_zero_bond_noop :
  forall ps v,
    bm_lookup (ps_allBonds ps) v = 0 ->
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
    ps_allBonds  ps2 = ps_allBonds  ps1
    /\ ps_coopVault ps2 = ps_coopVault ps1
    /\ ps_active   ps2 = ps_active   ps1.
Proof.
  intros ps v.
  unfold slash at 1.
  destruct (Nat.eq_dec (bm_lookup (ps_allBonds ps) v) 0) as [E | NE]; simpl.
  - (* First slash is a no-op; second is also no-op since bond still 0. *)
    unfold slash. simpl.
    rewrite E. simpl. repeat split; reflexivity.
  - (* First slash sets bond to 0; second is a no-op since lookup is now 0. *)
    unfold slash. simpl.
    rewrite bm_slash_lookup. simpl. repeat split; reflexivity.
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
  destruct (Nat.eq_dec (bm_lookup (ps_allBonds ps) v) 0); simpl.
  - reflexivity.
  - apply bm_slash_other. assumption.
Qed.
